//! Harness of the live differential parity suite. It runs each case through a reference Git Bash, gash, and the reference again.
//! Gash must match the reference on each observable the two reference runs agree on. Each test entry hands it a `Suite` and its cases.
//!
//! The reference runs against a suite-owned Git install copy, whose MSYS
//! `/tmp` mount the harness pins to the run's own tmp dir.
//! Run output goes to `<target>/tmp/parity/run-<stamp>/`, and only
//! the newest few run dirs are kept.
//!
//! Environment knobs:
//! * `GASH_PARITY_REF`       reference shell (default `<copy>\bin\bash.exe`).
//! * `GASH_PARITY_FILTER`    only run cases whose id contains this substring.
//! * `GASH_PARITY_JOBS`      parallel workers for cases without `serial` or `cold` (default 4).
//! * `GASH_PARITY_TIMEOUT`   seconds per shell run (default 90).
//! * `GASH_PARITY_MAX_DROP`  percent of dropped cases a group may have (default 5), one drop is always allowed.
//! * `GASH_PARITY_ENV`       `K=V;K=V` overlay for every run, with `@CASE@` in V replaced as in `env=`.
//! * `GASH_PARITY_GASH`      gash binary to test (default the one cargo built).
//! * `GASH_PARITY_ACCEPTED`  `;`-separated accepted lists that replace the entry's default list.
//!
//! Case header keys, one `# parity: key=value` line each at the top of the case file:
//! * `mode=<m>`            `script` (default) runs the case file, `c` and `lc` run its body with `-c` and `-l -c`, `host` runs it through the entry's host command.
//! * `fixture=<dir>`       copies `tests/parity/fixtures/<dir>` into the run's fixture dir.
//! * `home=<dir>`          copies `tests/parity/fixtures/<dir>` into the case home.
//! * `serial=1`            runs the case alone after the parallel ones, clears the run tmp dir before each shell run and adds it to `tree`.
//! * `cold=1`              runs the case after the others, kills every copy process and clears the run tmp dir before each shell run, and adds that dir to `tree`.
//! * `timeout=<seconds>`   timeout of each shell run of the case.
//! * `kill-after=<file>`   taskkills the shell tree once `<probe dir>/<file>` exists or after 60 s without it, observable `survivors`.
//! * `console=none`        spawns the shell with `CREATE_NO_WINDOW`.
//! * `env=K=V`             sets K for the run, with `@CASE@` in V replaced by the Windows path of the run's case dir.
//!
//! Variables the harness sets for a case:
//! * `PARITY_PROBE_DIR`      the run's probe dir, outside `tree`, which holds the `kill-after` marker and the `mode=host` cwd file.
//! * `PARITY_KILL_TAG`       digit string of a `kill-after` case, `survivors` counts the processes whose command line carries it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::support::{line_diff, run_command, run_command_watched, stderr_excerpt, ProcessJob, RunOutput};

const DEFAULT_TIMEOUT_S: u64 = 90;
const DEFAULT_JOBS: usize = 4;
const DEFAULT_MAX_DROP: f64 = 5.0;
const DETAIL_DIFF_LINES: usize = 80;
const HOLDER_SECONDS: &str = "86400";
const KILL_WAIT: Duration = Duration::from_secs(60);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Run dirs kept under `<target>/tmp/parity`, the current one included.
const KEEP_RUNS: usize = 5;

static KILL_TAGS: AtomicUsize = AtomicUsize::new(0);

const REMOVED_ENV: &[&str] = &[
    "LANG",
    "BASH_ENV",
    "ENV",
    "SHELLOPTS",
    "BASHOPTS",
    "CDPATH",
    "GLOBIGNORE",
    "POSIXLY_CORRECT",
    "MSYSTEM",
    "GASH_ROOT",
    "GASH_COREUTILS",
    "GASH_ETC_PROFILE",
    "HOME",
    // Left behind when cargo was started from a Git Bash.
    "_",
    "EXEPATH",
    "PLINK_PROTOCOL",
    "PWD",
    "OLDPWD",
    "SHLVL",
    "TERM",
    "TMPDIR",
    "PS1",
    "HOSTNAME",
    "DISPLAY",
    "SSH_ASKPASS",
    "CONFIG_SITE",
    "ACLOCAL_PATH",
    "MANPATH",
    "INFOPATH",
];
const REMOVED_ENV_PREFIXES: &[&str] = &["CARGO", "RUST", "LC_", "MINGW_", "MSYSTEM_", "ORIGINAL_", "PKG_CONFIG_"];

const OBSERVABLES: &[&str] = &["stdout", "stderr", "code", "tree", "cwd", "survivors", "all"];

/// What one test entry adds to the shared harness.
pub struct Suite {
    /// Accepted list under `tests/parity` used when `GASH_PARITY_ACCEPTED` is unset.
    pub accepted: &'static str,
    /// The `bash -c -l` argument for a `mode=host` case, built from its body and the file that receives the final `pwd -P`.
    pub host_command: fn(&str, &Path) -> String,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Script,
    C,
    Lc,
    Host,
}

pub struct Case {
    id: String,
    group: String,
    file: PathBuf,
    mode: Mode,
    body: String,
    /// Script argument and working dir for the `compat` group, which runs in place.
    in_place: Option<(String, PathBuf)>,
    fixture: Option<PathBuf>,
    home: Option<PathBuf>,
    env: Vec<(String, String)>,
    serial: bool,
    cold: bool,
    timeout: Option<Duration>,
    kill_after: Option<String>,
    no_console: bool,
}

struct Config<'a> {
    suite: &'a Suite,
    copy: PathBuf,
    ref_shell: PathBuf,
    gash_shell: PathBuf,
    gash_dir: PathBuf,
    run: PathBuf,
    tmp: PathBuf,
    timeout: Duration,
    base_env: Vec<(String, String)>,
    overlay: Vec<(String, String)>,
}

#[derive(Clone, Copy, PartialEq)]
enum Which {
    R1,
    G,
    R2,
}

impl Which {
    fn dir(self) -> &'static str {
        match self {
            Which::R1 => "r1",
            Which::G => "g",
            Which::R2 => "r2",
        }
    }
}

struct RunResult {
    out: RunOutput,
    tree: Vec<u8>,
    cwd: Option<Vec<u8>>,
    survivors: Option<Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Pass,
    Diff,
    Accepted,
    Dropped,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Diff => "DIFF",
            Status::Accepted => "ACCEPTED",
            Status::Dropped => "DROPPED",
        }
    }
}

struct Report {
    id: String,
    group: String,
    status: Status,
    marks: Vec<String>,
    detail: String,
    used_entries: Vec<usize>,
}

impl Report {
    fn line(&self) -> String {
        let mut s = format!("{} {}", self.status.label(), self.id);
        for m in &self.marks {
            s.push(' ');
            s.push_str(m);
        }
        s
    }
}

struct Entry {
    /// Index over all loaded lists.
    id: usize,
    file: String,
    line: usize,
    case_glob: String,
    observable: String,
    /// Line glob for stdout and stderr.
    sub_glob: Option<String>,
}

impl Entry {
    fn covers(&self, id: &str, observable: &str) -> bool {
        if !glob_match(self.case_glob.as_bytes(), id.as_bytes()) {
            return false;
        }
        if self.observable == "all" {
            return true;
        }
        if self.observable != observable {
            return false;
        }
        self.sub_glob.is_none()
    }

    fn covers_lines(&self, id: &str, observable: &str) -> bool {
        self.sub_glob.is_some()
            && self.observable == observable
            && glob_match(self.case_glob.as_bytes(), id.as_bytes())
    }
}

/// Path spellings of the run's directories, longest first, each with its token.
struct Tokens(Vec<(Vec<u8>, &'static str)>);

impl Tokens {
    fn new(paths: &[(&Path, &'static str)]) -> Tokens {
        let mut list = Vec::new();
        for (path, token) in paths {
            for v in path_variants(path) {
                list.push((v, *token));
            }
        }
        list.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        Tokens(list)
    }

    fn apply(&self, bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        for (pattern, token) in &self.0 {
            out = replace_all(&out, pattern, token.as_bytes());
        }
        out
    }
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn parity_dir() -> PathBuf {
    repo().join("tests").join("parity")
}

/// Panics on any failure.
pub fn run(suite: &Suite, mut cases: Vec<Case>) {
    let started = Instant::now();
    let repo = repo();
    let local = std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA is not set");
    let copy = PathBuf::from(local).join("gash-parity").join("Git");
    assert!(
        copy.join("usr").join("bin").join("cygpath.exe").is_file(),
        "reference Git copy missing at {}, run tests/parity/setup-ref.ps1 first",
        copy.display()
    );
    let ref_shell = std::env::var_os("GASH_PARITY_REF")
        .map(PathBuf::from)
        .unwrap_or_else(|| copy.join("bin").join("bash.exe"));
    assert!(ref_shell.is_file(), "reference shell missing at {}", ref_shell.display());

    let filter = std::env::var("GASH_PARITY_FILTER").unwrap_or_default();
    let jobs = env_count("GASH_PARITY_JOBS", DEFAULT_JOBS as u64) as usize;
    let max_drop = env_number("GASH_PARITY_MAX_DROP", DEFAULT_MAX_DROP);
    let timeout = Duration::from_secs(env_count("GASH_PARITY_TIMEOUT", DEFAULT_TIMEOUT_S));
    let overlay = parse_overlay(&std::env::var("GASH_PARITY_ENV").unwrap_or_default());

    let entries = load_accepted(suite, &repo);

    cases.sort_by(|a, b| a.id.cmp(&b.id));
    for pair in cases.windows(2) {
        assert!(pair[0].id != pair[1].id, "case id {} is loaded twice", pair[0].id);
    }
    cases.retain(|c| c.id.contains(filter.as_str()));
    assert!(!cases.is_empty(), "no parity cases match GASH_PARITY_FILTER={filter:?}");

    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("parity");
    prune_runs(&out_dir, KEEP_RUNS - 1);
    let run = out_dir.join(format!("run-{}", utc_stamp()));
    assert!(!run.exists(), "run dir {} already exists", run.display());
    let tmp = run.join("tmp");
    let gash_dir = run.join("gashbin");
    for dir in [&tmp, &gash_dir, &run.join("detail")] {
        fs::create_dir_all(dir).unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
    }
    let gash_exe = std::env::var_os("GASH_PARITY_GASH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_gash")));
    assert!(gash_exe.is_file(), "gash binary missing at {}", gash_exe.display());
    say(&format!("parity: gash binary {}", gash_exe.display()));
    for name in ["bash.exe", "sh.exe"] {
        fs::copy(&gash_exe, gash_dir.join(name))
            .unwrap_or_else(|e| panic!("cannot copy {} into {}: {e}", gash_exe.display(), gash_dir.display()));
    }

    let cfg = Config {
        suite,
        base_env: base_env(&copy),
        gash_shell: gash_dir.join("bash.exe"),
        gash_dir,
        copy,
        ref_shell,
        run,
        tmp,
        timeout,
        overlay,
    };
    say(&format!("parity: run dir {}", cfg.run.display()));

    let alive = copy_processes(&cfg, false);
    assert!(
        alive.is_empty(),
        "processes from {} are alive and pin its /tmp mount, stop them first:\n{}",
        cfg.copy.display(),
        alive.join("\n")
    );

    let mut reports: Vec<Report> = Vec::new();
    let (cold, warm): (Vec<Case>, Vec<Case>) = cases.into_iter().partition(|c| c.cold);
    let (serial, parallel): (Vec<Case>, Vec<Case>) = warm.into_iter().partition(|c| c.serial);

    if !parallel.is_empty() || !serial.is_empty() {
        let holder = Holder::start(&cfg);
        reports.extend(run_parallel(&cfg, &parallel, jobs, &entries));
        for case in &serial {
            let r = run_case(&cfg, case, &entries);
            say(&r.line());
            reports.push(r);
        }
        holder.stop();
    }
    for case in &cold {
        let r = run_case(&cfg, case, &entries);
        say(&r.line());
        reports.push(r);
    }
    kill_copy_processes(&cfg);

    reports.sort_by(|a, b| a.id.cmp(&b.id));
    let mut failures: Vec<String> = Vec::new();
    let count = |s: Status| reports.iter().filter(|r| r.status == s).count();
    let diffs = count(Status::Diff);
    if diffs > 0 {
        failures.push(format!("{diffs} cases diverge from the reference"));
    }

    let mut groups: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for r in &reports {
        let g = groups.entry(r.group.as_str()).or_default();
        g.0 += 1;
        if r.status == Status::Dropped {
            g.1 += 1;
        }
    }
    for (group, (ran, dropped)) in &groups {
        if *dropped > 1 && (*dropped as f64) * 100.0 / (*ran as f64) > max_drop {
            failures.push(format!("group {group} dropped {dropped} of {ran} cases, ceiling is one case or {max_drop}%"));
        }
    }

    // An entry is only checked when its case glob matches a case of this run.
    let used: BTreeSet<usize> = reports.iter().flat_map(|r| r.used_entries.iter().copied()).collect();
    for e in &entries {
        let matching: Vec<&Report> =
            reports.iter().filter(|r| glob_match(e.case_glob.as_bytes(), r.id.as_bytes())).collect();
        let unchecked = matching.iter().any(|r| r.status == Status::Dropped);
        if !matching.is_empty() && !used.contains(&e.id) && !unchecked {
            failures.push(format!("{} line {} matched no divergence", e.file, e.line));
        }
    }

    for r in &reports {
        if !r.detail.is_empty() {
            let path = cfg.run.join("detail").join(format!("{}.txt", r.id.replace('/', "__")));
            fs::write(&path, &r.detail).unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
        }
    }
    let final_line = format!(
        "parity: {} pass, {} diff, {} accepted, {} dropped ({:.1}s)",
        count(Status::Pass),
        diffs,
        count(Status::Accepted),
        count(Status::Dropped),
        started.elapsed().as_secs_f64()
    );
    let mut summary: String = reports.iter().map(|r| r.line() + "\n").collect();
    for f in &failures {
        summary.push_str(&format!("FAILURE {f}\n"));
    }
    summary.push_str(&final_line);
    summary.push('\n');
    let summary_path = cfg.run.join("summary.txt");
    fs::write(&summary_path, &summary).unwrap_or_else(|e| panic!("cannot write {}: {e}", summary_path.display()));

    let still = copy_processes(&cfg, false);
    if !still.is_empty() {
        failures.push(format!("processes from the copy still alive after the run: {}", still.join(", ")));
    }
    say(&final_line);
    if !failures.is_empty() {
        panic!("parity failed:\n{}\nsummary: {}", failures.join("\n"), summary_path.display());
    }
}

/// Prints past the test harness's output capture.
fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn env_number(name: &str, default: f64) -> f64 {
    match std::env::var(name) {
        Ok(v) => v
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| *n > 0.0)
            .unwrap_or_else(|| panic!("{name}={v:?} is not a positive number")),
        Err(_) => default,
    }
}

fn env_count(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(v) => v
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or_else(|| panic!("{name}={v:?} is not a positive integer")),
        Err(_) => default,
    }
}

/// Splits a `;`-separated knob value, dropping empty items.
fn env_list(name: &str) -> Option<Vec<String>> {
    let value = std::env::var(name).ok()?;
    let items: Vec<String> = value.split(';').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
    assert!(!items.is_empty(), "{name}={value:?} names nothing");
    Some(items)
}

fn parse_overlay(text: &str) -> Vec<(String, String)> {
    text.split(';')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let (k, v) = s.split_once('=').unwrap_or_else(|| panic!("GASH_PARITY_ENV entry {s:?} is not K=V"));
            (k.to_string(), v.to_string())
        })
        .collect()
}

/// Repo-relative paths resolve against the repo root.
fn load_accepted(suite: &Suite, repo: &Path) -> Vec<Entry> {
    let files = env_list("GASH_PARITY_ACCEPTED")
        .unwrap_or_else(|| vec![format!("tests/parity/{}", suite.accepted)]);
    let mut entries = Vec::new();
    for file in files {
        parse_accepted(&repo.join(&file), &file, &mut entries);
    }
    entries
}

fn parse_accepted(path: &Path, name: &str, entries: &mut Vec<Entry>) {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (case_glob, rest) = split_word(line);
        let (obs, mut reason) = split_word(rest);
        let (observable, mut sub_glob) = match obs.split_once(':') {
            Some((o, g)) => (o, Some(g)),
            None => (obs, None),
        };
        let mut closed = true;
        if sub_glob.is_some_and(|g| g.starts_with('"')) {
            let quoted = &rest[observable.len() + 2..];
            closed = false;
            if let Some((g, tail)) = quoted.split_once('"') {
                closed = tail.is_empty() || tail.starts_with(char::is_whitespace);
                sub_glob = Some(g);
                reason = tail.trim();
            }
        }
        let sub_ok = match sub_glob {
            None => true,
            Some(g) => !g.is_empty() && (observable == "stdout" || observable == "stderr"),
        };
        let known = OBSERVABLES.contains(&observable);
        let valid = known && sub_ok && closed && !reason.is_empty();
        assert!(
            valid,
            "{} line {}: expected `<case-glob> <observable>[:<glob>] <reason>`, got {raw:?}",
            path.display(),
            i + 1
        );
        entries.push(Entry {
            id: entries.len(),
            file: name.to_string(),
            line: i + 1,
            case_glob: case_glob.to_string(),
            observable: observable.to_string(),
            sub_glob: sub_glob.map(String::from),
        });
    }
}

fn split_word(s: &str) -> (&str, &str) {
    match s.split_once(char::is_whitespace) {
        Some((word, rest)) => (word, rest.trim_start()),
        None => (s, ""),
    }
}

/// The `tests/scripts` cases as group `compat`, each run in place.
pub fn compat_cases() -> Vec<Case> {
    let scripts = repo().join("tests").join("scripts");
    let mut cases = Vec::new();
    for file in sh_files(&scripts) {
        let mut case = parse_case("compat", &file);
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        case.in_place = Some((name, scripts.clone()));
        case.env.insert(0, ("LC_ALL".to_string(), "C".to_string()));
        cases.push(case);
    }
    cases
}

/// One group per subdirectory of `root`.
pub fn group_cases(root: &Path) -> Vec<Case> {
    let mut cases = Vec::new();
    if root.is_dir() {
        for dir in read_dir_sorted(root).into_iter().filter(|p| p.is_dir()) {
            let group = dir.file_name().unwrap().to_string_lossy().into_owned();
            cases.extend(flat_cases(&dir, &group));
        }
    }
    cases
}

/// The `*.sh` files of `dir` as one group.
fn flat_cases(dir: &Path, group: &str) -> Vec<Case> {
    sh_files(dir).iter().map(|file| parse_case(group, file)).collect()
}

fn read_dir_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display())).path())
        .collect();
    paths.sort();
    paths
}

fn sh_files(dir: &Path) -> Vec<PathBuf> {
    read_dir_sorted(dir)
        .into_iter()
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "sh"))
        .collect()
}

fn parse_case(group: &str, file: &Path) -> Case {
    let stem = file.file_stem().unwrap().to_string_lossy().into_owned();
    let text = fs::read(file).unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
    let text = String::from_utf8_lossy(&text).into_owned();
    let mut case = Case {
        id: format!("{group}/{stem}"),
        group: group.to_string(),
        file: file.to_path_buf(),
        mode: Mode::Script,
        body: String::new(),
        in_place: None,
        fixture: None,
        home: None,
        env: Vec::new(),
        serial: false,
        cold: false,
        timeout: None,
        kill_after: None,
        no_console: false,
    };
    let bad = |what: &str| -> ! { panic!("{}: {what}", file.display()) };
    let fixture_dir = |value: &str| -> PathBuf {
        let dir = parity_dir().join("fixtures").join(value);
        if !dir.is_dir() {
            bad(&format!("fixture {} does not exist", dir.display()));
        }
        dir
    };
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.peek() {
        let Some(header) = line.trim_end().strip_prefix("# parity:") else { break };
        let (key, value) = header.trim().split_once('=').unwrap_or_else(|| bad("header is not key=value"));
        match key {
            "mode" => {
                case.mode = match value {
                    "script" => Mode::Script,
                    "c" => Mode::C,
                    "lc" => Mode::Lc,
                    "host" => Mode::Host,
                    _ => bad(&format!("unknown mode {value:?}")),
                }
            }
            "fixture" => case.fixture = Some(fixture_dir(value)),
            "home" => case.home = Some(fixture_dir(value)),
            "kill-after" => {
                if value.is_empty() || value.contains(['/', '\\']) {
                    bad("kill-after needs a plain file name");
                }
                case.kill_after = Some(value.to_string());
            }
            "console" => {
                if value != "none" {
                    bad("console only takes none");
                }
                case.no_console = true;
            }
            "env" => {
                let (k, v) = value.split_once('=').unwrap_or_else(|| bad("env header needs env=K=V"));
                case.env.push((k.to_string(), v.to_string()));
            }
            "serial" => {
                if value != "1" {
                    bad("serial only takes 1");
                }
                case.serial = true;
            }
            "cold" => {
                if value != "1" {
                    bad("cold only takes 1");
                }
                case.cold = true;
            }
            "timeout" => {
                let secs = value.parse::<u64>().ok().filter(|s| *s > 0).unwrap_or_else(|| bad("timeout needs seconds"));
                case.timeout = Some(Duration::from_secs(secs));
            }
            _ => bad(&format!("unknown header key {key:?}")),
        }
        lines.next();
    }
    case.body = lines.collect::<String>().trim_start_matches(['\r', '\n']).trim_end().to_string();
    if case.mode != Mode::Script && case.body.is_empty() {
        bad("empty command body");
    }
    case
}

fn base_env(copy: &Path) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .filter(|(k, _)| {
            let upper = k.to_ascii_uppercase();
            !REMOVED_ENV.contains(&upper.as_str())
                && !REMOVED_ENV_PREFIXES.iter().any(|p| upper.starts_with(p))
        })
        .collect();
    let root = win(copy);
    for (k, v) in [
        ("GIT_EDITOR", "true"),
        ("GASH_ROOT", root.as_str()),
        ("GASH_COREUTILS", "0"),
        ("GIT_OPTIONAL_LOCKS", "0"),
        ("GIT_PAGER", "cat"),
        ("PAGER", "cat"),
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "core.fsmonitor"),
        ("GIT_CONFIG_VALUE_0", "false"),
    ]
    .into_iter()
    {
        set_env(&mut env, k, v);
    }
    env
}

/// Sets a variable, replacing an existing one whose name differs only in case.
fn set_env(env: &mut Vec<(String, String)>, key: &str, value: &str) {
    match env.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
        Some(slot) => *slot = (key.to_string(), value.to_string()),
        None => env.push((key.to_string(), value.to_string())),
    }
}

fn expand_env_value(value: &str, case_dir: &Path) -> String {
    value.replace("@CASE@", &win(case_dir))
}

/// Deletes the oldest `run-*` dirs under `dir` until `keep` are left.
fn prune_runs(dir: &Path, keep: usize) {
    if !dir.is_dir() {
        return;
    }
    let runs: Vec<PathBuf> = read_dir_sorted(dir)
        .into_iter()
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("run-")))
        .collect();
    for old in &runs[..runs.len().saturating_sub(keep)] {
        fs::remove_dir_all(old).unwrap_or_else(|e| panic!("cannot prune old run dir {}: {e}", old.display()));
    }
}

struct Holder {
    child: Child,
    job: ProcessJob,
}

impl Holder {
    /// A long sleep from the copy that pins its `/tmp` mount to the run tmp dir.
    fn start(cfg: &Config) -> Holder {
        let mut env = cfg.base_env.clone();
        set_env(&mut env, "TMP", &win(&cfg.tmp));
        set_env(&mut env, "TEMP", &win(&cfg.tmp));
        let sleep = cfg.copy.join("usr").join("bin").join("sleep.exe");
        let mut child = Command::new(&sleep)
            .arg(HOLDER_SECONDS)
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("cannot start holder {}: {e}", sleep.display()));
        let job = ProcessJob::attach_kill_on_close(&child);
        if !job.is_attached() {
            let _ = child.kill();
            panic!("cannot put the holder into a kill-on-close job");
        }
        thread::sleep(Duration::from_millis(300));

        let mut cmd = Command::new(cfg.copy.join("usr").join("bin").join("cygpath.exe"));
        cmd.args(["-w", "/tmp"]).env_clear().envs(env.iter().map(|(k, v)| (k, v))).stdin(Stdio::null());
        let out = run_command(cmd, Duration::from_secs(30), true);
        let mapped = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let alive = matches!(child.try_wait(), Ok(None));
        if !alive || !mapped.eq_ignore_ascii_case(&win(&cfg.tmp)) {
            job.terminate();
            panic!(
                "holder check failed: alive={alive}, /tmp maps to {mapped:?}, want {:?}, cygpath stderr {:?}",
                win(&cfg.tmp),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        say(&format!("parity: holder pid {} pins /tmp to {mapped}", child.id()));
        Holder { child, job }
    }

    fn stop(mut self) {
        self.job.terminate();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Lists, and with `kill` stops, every process whose image lives under the copy.
fn copy_processes(cfg: &Config, kill: bool) -> Vec<String> {
    let script = format!(
        "$p = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -and $_.Path -like '{}\\*' }}); {} foreach ($x in $p) {{ '{{0}} {{1}}' -f $x.Id, $x.Path }}",
        win(&cfg.copy).replace('\'', "''"),
        if kill { "$p | Stop-Process -Force -ErrorAction SilentlyContinue;" } else { "" }
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("cannot run powershell to list copy processes: {e}"));
    assert!(
        out.status.success(),
        "powershell process listing failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

/// Leaves no copy process alive, which resets the copy's mount table.
fn kill_copy_processes(cfg: &Config) {
    for _ in 0..20 {
        if copy_processes(cfg, true).is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    let left = copy_processes(cfg, false);
    assert!(left.is_empty(), "copy processes survive the kill: {}", left.join(", "));
    thread::sleep(Duration::from_millis(500));
}

fn run_parallel(cfg: &Config, cases: &[Case], jobs: usize, entries: &[Entry]) -> Vec<Report> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Report>> = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..jobs.min(cases.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= cases.len() {
                    break;
                }
                let r = run_case(cfg, &cases[i], entries);
                say(&r.line());
                results.lock().unwrap().push(r);
            });
        }
    });
    results.into_inner().unwrap()
}

fn run_case(cfg: &Config, case: &Case, entries: &[Entry]) -> Report {
    let case_root = cfg.run.join("cases").join(&case.group).join(case.id.split_once('/').unwrap().1);
    let dirs = [Which::R1, Which::G, Which::R2].map(|w| case_root.join(w.dir()));
    let tokens = Tokens::new(&[
        (&dirs[0], "<CASE>"),
        (&dirs[1], "<CASE>"),
        (&dirs[2], "<CASE>"),
        (&cfg.tmp, "<TMP>"),
        (&cfg.copy, "<ROOT>"),
        (&cfg.gash_dir, "<SHELLDIR>"),
    ]);
    let runs = [Which::R1, Which::G, Which::R2].map(|w| {
        if case.cold {
            kill_copy_processes(cfg);
        }
        run_one(cfg, case, w, &case_root.join(w.dir()), &tokens)
    });
    judge(case, &runs, &dirs, &tokens, entries)
}

fn run_one(cfg: &Config, case: &Case, which: Which, dir: &Path, tokens: &Tokens) -> RunResult {
    let fixture = dir.join("fixture");
    let home = dir.join("home");
    let probe = dir.join("probe");
    for d in [&fixture, &home, &probe] {
        fs::create_dir_all(d).unwrap_or_else(|e| panic!("cannot create {}: {e}", d.display()));
    }
    if let Some(template) = &case.fixture {
        copy_tree(template, &fixture);
    }
    if let Some(template) = &case.home {
        copy_tree(template, &home);
    }
    let walk_tmp = case.serial || case.cold;
    if walk_tmp {
        clear_dir(&cfg.tmp);
    }

    let shell = match which {
        Which::G => &cfg.gash_shell,
        _ => &cfg.ref_shell,
    };
    let mut cmd = Command::new(shell);
    let cwd = match &case.in_place {
        Some((name, dir)) => {
            cmd.arg(name);
            dir.clone()
        }
        None => {
            match case.mode {
                Mode::Script => {
                    cmd.arg(fwd(&case.file));
                }
                Mode::C => {
                    cmd.args(["-c", &case.body]);
                }
                Mode::Lc => {
                    cmd.args(["-l", "-c", &case.body]);
                }
                // The entry's host command goes in below.
                Mode::Host => {}
            }
            fixture.clone()
        }
    };

    let mut env = cfg.base_env.clone();
    set_env(&mut env, "HOME", &win(&home));
    set_env(&mut env, "TMP", &win(&cfg.tmp));
    set_env(&mut env, "TEMP", &win(&cfg.tmp));
    set_env(&mut env, "SHELL", &win(shell));
    set_env(&mut env, "PARITY_PROBE_DIR", &fwd(&probe));
    let kill_tag = case.kill_after.as_ref().map(|_| kill_tag());
    if let Some(tag) = &kill_tag {
        set_env(&mut env, "PARITY_KILL_TAG", tag);
    }
    for (k, v) in cfg.overlay.iter().chain(case.env.iter()) {
        let value = expand_env_value(v, dir);
        set_env(&mut env, k, &value);
    }
    let cwd_file = probe.join("cwd");
    let host = case.mode == Mode::Host && case.in_place.is_none();
    if host {
        cmd.args(["-c", "-l", &(cfg.suite.host_command)(&case.body, &cwd_file)]);
    }
    cmd.current_dir(&cwd).env_clear().envs(env.iter().map(|(k, v)| (k, v)));
    if case.no_console {
        no_window(&mut cmd);
    }
    cmd.stdin(Stdio::null());

    let timeout = case.timeout.unwrap_or(cfg.timeout);
    let mut survivors = None;
    let out = match (&case.kill_after, &kill_tag) {
        (Some(marker), Some(tag)) => {
            let mut killer = Killer::new(probe.join(marker), tag);
            let out = run_command_watched(cmd, timeout, true, &mut |pid, exited| killer.watch(pid, exited));
            survivors = Some(killer.result.into_bytes());
            out
        }
        _ => run_command(cmd, timeout, true),
    };
    if let Some(err) = &out.spawn_error {
        panic!("{}: cannot spawn {}: {err}", case.id, shell.display());
    }
    for (name, bytes) in [("stdout.txt", &out.stdout), ("stderr.txt", &out.stderr)] {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
    }

    let mut roots = vec![("fixture", fixture.clone()), ("home", home.clone())];
    if walk_tmp {
        roots.push(("tmp", cfg.tmp.clone()));
    }
    let mut tree = Vec::new();
    for (label, root) in &roots {
        walk(root, label, tokens, &mut tree);
    }
    RunResult {
        out,
        tree,
        cwd: if host { Some(read_or_missing(&cwd_file)) } else { None },
        survivors,
    }
}

fn read_or_missing(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|_| b"(missing)".to_vec())
}

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_window(_cmd: &mut Command) {}

/// A digit string no other process on the machine carries in its command line.
fn kill_tag() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock before 1970").subsec_nanos();
    format!("7{}{:09}{}", std::process::id(), nanos, KILL_TAGS.fetch_add(1, Ordering::SeqCst))
}

/// Tree-kills the shell once the case writes its marker and counts the tagged survivors.
struct Killer {
    marker: PathBuf,
    tag: String,
    started: Instant,
    fired: bool,
    marker_seen: bool,
    result: String,
}

impl Killer {
    fn new(marker: PathBuf, tag: &str) -> Killer {
        Killer {
            marker,
            tag: tag.to_string(),
            started: Instant::now(),
            fired: false,
            marker_seen: false,
            result: String::new(),
        }
    }

    fn watch(&mut self, pid: u32, exited: bool) {
        if !exited {
            if !self.fired && (self.marker.exists() || self.started.elapsed() >= KILL_WAIT) {
                self.fired = true;
                self.marker_seen = self.marker.exists();
                let _ = Command::new("taskkill")
                    .args(["/T", "/F", "/PID", &pid.to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            return;
        }
        thread::sleep(Duration::from_secs(1));
        let count = kill_tagged(&self.tag);
        self.result = match (self.fired, self.marker_seen) {
            (true, true) => count,
            (true, false) => format!("marker missing after {}s, {count}", KILL_WAIT.as_secs()),
            (false, _) => format!("shell exited before the kill, {count}"),
        };
    }
}

/// Stops every process whose command line carries `tag` and returns how many there were.
fn kill_tagged(tag: &str) -> String {
    let script = format!(
        "$p = @(Get-CimInstance Win32_Process | Where-Object {{ $_.ProcessId -ne $PID -and $_.CommandLine -like '*{tag}*' }}); \
         foreach ($x in $p) {{ Stop-Process -Id $x.ProcessId -Force -ErrorAction SilentlyContinue }}; $p.Count"
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("cannot run powershell to count tagged processes: {e}"));
    assert!(out.status.success(), "tagged process count failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn copy_tree(from: &Path, to: &Path) {
    for src in read_dir_sorted(from) {
        let dst = to.join(src.file_name().unwrap());
        if src.is_dir() {
            fs::create_dir_all(&dst).unwrap_or_else(|e| panic!("cannot create {}: {e}", dst.display()));
            copy_tree(&src, &dst);
        } else {
            fs::copy(&src, &dst).unwrap_or_else(|e| panic!("cannot copy {}: {e}", src.display()));
        }
    }
}

fn clear_dir(dir: &Path) {
    for path in read_dir_sorted(dir) {
        let res = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        res.unwrap_or_else(|e| panic!("cannot clear {}: {e}", path.display()));
    }
}

/// Appends one line per entry under `root`, hashing file content after token normalisation.
fn walk(root: &Path, label: &str, tokens: &Tokens, out: &mut Vec<u8>) {
    let Ok(entries) = fs::read_dir(root) else {
        out.extend_from_slice(format!("{label} unreadable\n").as_bytes());
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let rel = format!("{label}/{}", path.file_name().unwrap().to_string_lossy());
        let Ok(meta) = fs::symlink_metadata(&path) else {
            out.extend_from_slice(format!("{rel} unreadable\n").as_bytes());
            continue;
        };
        if meta.file_type().is_symlink() {
            let target = fs::read_link(&path).map(|t| t.to_string_lossy().into_owned()).unwrap_or_default();
            let target = String::from_utf8_lossy(&tokens.apply(target.as_bytes())).into_owned();
            out.extend_from_slice(format!("{rel} l {target}\n").as_bytes());
        } else if meta.is_dir() {
            out.extend_from_slice(format!("{rel} d\n").as_bytes());
            walk(&path, &rel, tokens, out);
        } else {
            match fs::read(&path) {
                Ok(bytes) => {
                    let norm = tokens.apply(&bytes);
                    out.extend_from_slice(format!("{rel} f {} {:016x}\n", norm.len(), fnv1a(&norm)).as_bytes());
                }
                Err(e) => out.extend_from_slice(format!("{rel} f unreadable {e}\n").as_bytes()),
            }
        }
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// One compared observable after normalisation.
struct Obs {
    name: &'static str,
    vals: [Vec<u8>; 3],
}

fn judge(case: &Case, runs: &[RunResult; 3], dirs: &[PathBuf; 3], tokens: &Tokens, entries: &[Entry]) -> Report {
    let code = |r: &RunResult| -> Vec<u8> {
        match (r.out.timed_out, r.out.code) {
            (true, _) => b"timeout".to_vec(),
            (false, Some(c)) => c.to_string().into_bytes(),
            (false, None) => b"none".to_vec(),
        }
    };
    let mut obs = vec![
        Obs { name: "stdout", vals: [0, 1, 2].map(|i| tokens.apply(&runs[i].out.stdout)) },
        Obs { name: "stderr", vals: [0, 1, 2].map(|i| shell_prefix(&tokens.apply(&runs[i].out.stderr))) },
        Obs { name: "code", vals: [0, 1, 2].map(|i| code(&runs[i])) },
        Obs { name: "tree", vals: [0, 1, 2].map(|i| runs[i].tree.clone()) },
    ];
    if runs[0].cwd.is_some() {
        obs.push(Obs { name: "cwd", vals: [0, 1, 2].map(|i| tokens.apply(runs[i].cwd.as_deref().unwrap_or_default())) });
    }
    if runs[0].survivors.is_some() {
        obs.push(Obs { name: "survivors", vals: [0, 1, 2].map(|i| runs[i].survivors.clone().unwrap_or_default()) });
    }

    let mut detail = format!("{}\ncase file: {}\n", case.id, case.file.display());
    for (i, w) in [Which::R1, Which::G, Which::R2].iter().enumerate() {
        let o = &runs[i].out;
        detail.push_str(&format!(
            "{}: dir {}, code {}, {:.1}s{}{}\n",
            w.dir(),
            dirs[i].display(),
            String::from_utf8_lossy(&code(&runs[i])),
            o.elapsed.as_secs_f64(),
            if o.timed_out { ", timed out" } else { "" },
            if o.pipes_left_open { ", pipes left open" } else { "" }
        ));
    }

    let mut marks = Vec::new();
    let mut used = Vec::new();
    let mut diverged = false;
    let mut unaccepted = false;
    let ref_timeout = runs[0].out.timed_out || runs[2].out.timed_out;
    let flaky_key = obs.iter().any(|o| (o.name == "stdout" || o.name == "code") && o.vals[0] != o.vals[2]);
    if ref_timeout || flaky_key {
        if ref_timeout {
            marks.push("ref-timeout".to_string());
        }
        for o in obs.iter().filter(|o| o.vals[0] != o.vals[2]) {
            marks.push(format!("flaky:{}", o.name));
            detail.push_str(&line_diff(o.name, "r1", "r2", &o.vals[0], &o.vals[2], DETAIL_DIFF_LINES));
        }
        return finish(case, Status::Dropped, marks, detail, used, runs, tokens);
    }

    for o in &obs {
        if o.vals[0] != o.vals[2] {
            marks.push(format!("flaky:{}", o.name));
            detail.push_str(&line_diff(&format!("flaky {}", o.name), "r1", "r2", &o.vals[0], &o.vals[2], DETAIL_DIFF_LINES));
            continue;
        }
        if o.vals[0] == o.vals[1] {
            continue;
        }
        diverged = true;
        let hit: Vec<usize> = entries.iter().filter(|e| e.covers(&case.id, o.name)).map(|e| e.id).collect();
        let line_entries: Vec<&Entry> = entries.iter().filter(|e| e.covers_lines(&case.id, o.name)).collect();
        if !hit.is_empty() {
            used.extend(hit);
            marks.push(format!("accepted:{}", o.name));
        } else if !line_entries.is_empty() {
            let (ref_kept, ref_hits) = drop_lines(&o.vals[0], &line_entries);
            let (gash_kept, gash_hits) = drop_lines(&o.vals[1], &line_entries);
            used.extend(ref_hits);
            used.extend(gash_hits);
            if ref_kept == gash_kept {
                marks.push(format!("accepted:{}(lines)", o.name));
            } else {
                unaccepted = true;
                marks.push(o.name.to_string());
                let what = format!("{} without accepted lines", o.name);
                detail.push_str(&line_diff(&what, "ref", "gash", &ref_kept, &gash_kept, DETAIL_DIFF_LINES));
            }
        } else {
            unaccepted = true;
            marks.push(o.name.to_string());
        }
        detail.push_str(&line_diff(o.name, "ref", "gash", &o.vals[0], &o.vals[1], DETAIL_DIFF_LINES));
    }

    let status = match (diverged, unaccepted) {
        (false, _) => Status::Pass,
        (true, true) => Status::Diff,
        (true, false) => Status::Accepted,
    };
    finish(case, status, marks, detail, used, runs, tokens)
}

/// Drops every line a line entry's glob matches, ignoring the line ending.
fn drop_lines(bytes: &[u8], line_entries: &[&Entry]) -> (Vec<u8>, Vec<usize>) {
    let mut kept = Vec::new();
    let mut hits = Vec::new();
    for line in bytes.split_inclusive(|&b| b == b'\n') {
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        let body = body.strip_suffix(b"\r").unwrap_or(body);
        let matched: Vec<usize> = line_entries
            .iter()
            .filter(|e| e.sub_glob.as_ref().is_some_and(|g| glob_match(g.as_bytes(), body)))
            .map(|e| e.id)
            .collect();
        if matched.is_empty() {
            kept.extend_from_slice(line);
        } else {
            hits.extend(matched);
        }
    }
    (kept, hits)
}

fn finish(
    case: &Case,
    status: Status,
    marks: Vec<String>,
    mut detail: String,
    used_entries: Vec<usize>,
    runs: &[RunResult; 3],
    tokens: &Tokens,
) -> Report {
    if status == Status::Pass {
        detail.clear();
    } else {
        detail.insert_str(0, &format!("{} ", status.label()));
        detail.push_str("ref (r1)\n");
        detail.push_str(&stderr_excerpt(&shell_prefix(&tokens.apply(&runs[0].out.stderr))));
        detail.push_str("gash\n");
        detail.push_str(&stderr_excerpt(&shell_prefix(&tokens.apply(&runs[1].out.stderr))));
    }
    Report { id: case.id.clone(), group: case.group.clone(), status, marks, detail, used_entries }
}

/// Replaces a leading shell name prefix on each stderr line with `<SH>: `.
fn shell_prefix(bytes: &[u8]) -> Vec<u8> {
    const PREFIXES: &[&[u8]] = &[
        b"bash.exe: ",
        b"bash: ",
        b"sh.exe: ",
        b"sh: ",
        b"gash.exe: ",
        b"gash: ",
        b"/usr/bin/bash.exe: ",
        b"/usr/bin/bash: ",
        b"/bin/bash.exe: ",
        b"/bin/bash: ",
        b"/usr/bin/sh.exe: ",
        b"/usr/bin/sh: ",
        b"/bin/sh.exe: ",
        b"/bin/sh: ",
    ];
    let mut out = Vec::with_capacity(bytes.len());
    for line in bytes.split_inclusive(|&b| b == b'\n') {
        match PREFIXES.iter().find(|p| line.starts_with(p)) {
            Some(p) => {
                out.extend_from_slice(b"<SH>: ");
                out.extend_from_slice(&line[p.len()..]);
            }
            None => out.extend_from_slice(line),
        }
    }
    out
}

fn replace_all(hay: &[u8], pattern: &[u8], with: &[u8]) -> Vec<u8> {
    if pattern.is_empty() || hay.len() < pattern.len() {
        return hay.to_vec();
    }
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(pattern) {
            out.extend_from_slice(with);
            i += pattern.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// `C:\a\b`, `C:/a/b` and `/c/a/b`, each with either drive letter case.
fn path_variants(path: &Path) -> Vec<Vec<u8>> {
    let w = win(path);
    let b = w.as_bytes();
    if b.len() < 3 || b[1] != b':' {
        return vec![w.into_bytes()];
    }
    let back = &w[2..];
    let fwd = back.replace('\\', "/");
    let lower = b[0].to_ascii_lowercase() as char;
    let mut out = Vec::new();
    for d in [lower.to_ascii_uppercase(), lower] {
        out.push(format!("{d}:{back}").into_bytes());
        out.push(format!("{d}:{fwd}").into_bytes());
        out.push(format!("/{d}{fwd}").into_bytes());
    }
    out
}

fn win(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\")
}

fn fwd(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn posix(path: &Path) -> String {
    let f = fwd(path);
    match f.as_bytes() {
        [d, b':', b'/', ..] => format!("/{}{}", (*d as char).to_ascii_lowercase(), &f[2..]),
        _ => f,
    }
}

/// Glob with `*` and `?`.
fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || (pattern[p] != b'*' && pattern[p] == text[t])) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, t));
            p += 1;
        } else if let Some((sp, st)) = star {
            p = sp + 1;
            t = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

/// UTC `YYYYMMDD-HHMMSS`.
fn utc_stamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock before 1970").as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!("{year:04}{month:02}{day:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}
