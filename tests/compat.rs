//! Differential compatibility tests against real Git Bash.
//!
//! Every `tests/scripts/*.sh` is run through the `gash` binary
//! (`gash NNN_topic.sh`, cwd = `tests/scripts`, stdin = null) and its stdout
//! bytes and exit code are compared with `NNN_topic.out` / `NNN_topic.code`,
//! which were produced by `C:\Program Files\Git\bin\bash.exe` via
//! `tests/regen_expected.ps1`. Stderr is captured for diagnostics only.
//!
//! All scripts are run; the test fails once at the end with a summary.
//!
//! Environment knobs:
//! * `GASH_COMPAT_FILTER` - only run scripts whose file name contains this substring.
//! * `GASH_COMPAT_JOBS`   - number of scripts run in parallel (default 1). MSYS
//!   process creation/fork contends badly when parallel, so raising this can
//!   cause spurious 20s timeouts (it does with real Git Bash); try it for speed.
//!
//! The child environment is normalized the same way as in `regen_expected.ps1`:
//! `LC_ALL=C`, and `LANG`, `LC_*`, `BASH_ENV`, `ENV`, `SHELLOPTS`, `BASHOPTS`,
//! `CDPATH`, `GLOBIGNORE`, `POSIXLY_CORRECT` are removed.

mod support;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use support::{run_command, stderr_excerpt, stdout_diff, RunOutput};

const TIMEOUT: Duration = Duration::from_secs(20);

const REMOVED_ENV: &[&str] = &[
    "LANG",
    "BASH_ENV",
    "ENV",
    "SHELLOPTS",
    "BASHOPTS",
    "CDPATH",
    "GLOBIGNORE",
    "POSIXLY_CORRECT",
];

struct Outcome {
    name: String,
    passed: bool,
    report: String,
}

#[test]
fn compat() {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_gash"));
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("scripts");
    let filter = std::env::var("GASH_COMPAT_FILTER").unwrap_or_default();

    let mut scripts: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.ends_with(".sh") && name.contains(filter.as_str()))
        .collect();
    scripts.sort();
    assert!(
        !scripts.is_empty(),
        "no scripts in {} match GASH_COMPAT_FILTER={filter:?}",
        dir.display()
    );

    let jobs = std::env::var("GASH_COMPAT_JOBS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1)
        .min(scripts.len());

    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Outcome>>> =
        Mutex::new((0..scripts.len()).map(|_| None).collect());
    thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= scripts.len() {
                        break;
                    }
                    let outcome = check_script(&exe, &dir, &scripts[i]);
                    results.lock().unwrap()[i] = Some(outcome);
                }
            });
        }
    });
    let outcomes: Vec<Outcome> = results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|o| o.expect("every script produces an outcome"))
        .collect();

    let total = outcomes.len();
    let passed = outcomes.iter().filter(|o| o.passed).count();
    let failed: Vec<&Outcome> = outcomes.iter().filter(|o| !o.passed).collect();

    let mut summary = String::new();
    for o in &failed {
        summary.push_str(&o.report);
        summary.push('\n');
    }
    if !failed.is_empty() {
        summary.push_str("failing scripts:");
        for o in &failed {
            summary.push(' ');
            summary.push_str(&o.name);
        }
        summary.push('\n');
    }
    let final_line = format!(
        "passed {passed}/{total} ({:.1}s, {jobs} jobs)",
        started.elapsed().as_secs_f64()
    );
    summary.push_str(&final_line);

    // Written straight to the process stdout so it is visible even when the
    // test harness captures output of passing tests.
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "\n{final_line}");
    let _ = out.flush();
    drop(out);

    if !failed.is_empty() {
        panic!("{} of {total} compat scripts failed\n\n{summary}", failed.len());
    }
}

fn check_script(exe: &Path, dir: &Path, name: &str) -> Outcome {
    let stem = name.strip_suffix(".sh").unwrap_or(name);
    let out_path = dir.join(format!("{stem}.out"));
    let code_path = dir.join(format!("{stem}.code"));

    let expected_out = fs::read(&out_path);
    let expected_code = fs::read_to_string(&code_path)
        .map_err(|e| e.to_string())
        .and_then(|s| s.trim().parse::<i32>().map_err(|e| format!("bad contents {s:?}: {e}")));

    let run = run_script(exe, dir, name);
    let mut report = String::new();
    let mut ok = true;

    if let Some(err) = &run.spawn_error {
        report.push_str(&format!("  could not spawn {}: {err}\n", exe.display()));
        ok = false;
    }
    if run.timed_out {
        report.push_str(&format!("  TIMED OUT after {}s (process killed)\n", TIMEOUT.as_secs()));
        ok = false;
    }
    if run.pipes_left_open {
        report.push_str("  note: stdout/stderr were still open after exit (background process?); output may be truncated\n");
    }

    match &expected_code {
        Ok(want) => {
            if run.code != Some(*want) {
                ok = false;
                let got = run.code.map_or("none (killed?)".to_string(), |c| c.to_string());
                report.push_str(&format!("  exit code: expected {want}, got {got}\n"));
            }
        }
        Err(e) => {
            ok = false;
            report.push_str(&format!("  cannot read {}: {e}\n", code_path.display()));
        }
    }

    match &expected_out {
        Ok(want) => {
            if *want != run.stdout {
                ok = false;
                report.push_str(&stdout_diff(want, &run.stdout));
            }
        }
        Err(e) => {
            ok = false;
            report.push_str(&format!("  cannot read {}: {e}\n", out_path.display()));
        }
    }

    if !ok {
        report.push_str(&stderr_excerpt(&run.stderr));
        report = format!(
            "FAIL {name} ({:.1}s)\n{report}",
            run.elapsed.as_secs_f64()
        );
    }
    Outcome {
        name: name.to_string(),
        passed: ok,
        report,
    }
}

fn run_script(exe: &Path, dir: &Path, name: &str) -> RunOutput {
    let mut cmd = Command::new(exe);
    cmd.arg(name).current_dir(dir).stdin(Stdio::null());
    for (key, _) in std::env::vars_os() {
        if let Some(k) = key.to_str() {
            let upper = k.to_ascii_uppercase();
            if upper.starts_with("LC_") || REMOVED_ENV.contains(&upper.as_str()) {
                cmd.env_remove(&key);
            }
        }
    }
    cmd.env("LC_ALL", "C");
    run_command(cmd, TIMEOUT, false)
}
