//! Process running and line diffs shared by the `compat` and `parity` test targets.

// Each test target uses a different subset of this module.
#![allow(dead_code)]

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How long a leaked background process may hold the pipes open after the child exits.
pub const PIPE_GRACE: Duration = Duration::from_secs(3);
pub const MAX_DIFF_LINES: usize = 15;
pub const MAX_STDERR_LINES: usize = 10;
pub const MAX_LINE_CHARS: usize = 200;
/// Above this many lines on either side, fall back to a positional diff.
pub const MAX_LCS_LINES: usize = 3000;

pub struct RunOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: Option<i32>,
    pub timed_out: bool,
    pub pipes_left_open: bool,
    pub spawn_error: Option<String>,
    pub elapsed: Duration,
}

/// Runs `cmd`, killing its process tree at `timeout` and, with `kill_on_close`, on return.
pub fn run_command(cmd: Command, timeout: Duration, kill_on_close: bool) -> RunOutput {
    run_command_watched(cmd, timeout, kill_on_close, &mut |_, _| {})
}

/// Like `run_command`, calling `watch(pid, exited)` on every poll and once after exit, before the pipes drain.
pub fn run_command_watched(
    mut cmd: Command,
    timeout: Duration,
    kill_on_close: bool,
    watch: &mut dyn FnMut(u32, bool),
) -> RunOutput {
    let start = Instant::now();
    let failed = |error: String| RunOutput {
        stdout: Vec::new(),
        stderr: Vec::new(),
        code: None,
        timed_out: false,
        pipes_left_open: false,
        spawn_error: Some(error),
        elapsed: start.elapsed(),
    };
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => return failed(e.to_string()),
    };

    let job = if kill_on_close {
        ProcessJob::attach_kill_on_close(&child)
    } else {
        ProcessJob::attach(&child)
    };
    if kill_on_close && cfg!(windows) && !job.is_attached() {
        let _ = child.kill();
        let _ = child.wait();
        return failed("cannot put the child into a kill-on-close job".to_string());
    }
    let stdout = PipeReader::start(child.stdout.take().expect("stdout is piped"));
    let stderr = PipeReader::start(child.stderr.take().expect("stderr is piped"));

    let pid = child.id();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => watch(pid, false),
            Err(_) => break None,
        }
        if start.elapsed() >= timeout {
            timed_out = true;
            kill_tree(&mut child, &job);
            break child.wait().ok();
        }
        thread::sleep(Duration::from_millis(5));
    };
    watch(pid, true);

    let deadline = Instant::now() + PIPE_GRACE;
    let (stdout, out_open) = stdout.finish(deadline);
    let (stderr, err_open) = stderr.finish(deadline);
    RunOutput {
        stdout,
        stderr,
        code: status.and_then(|s| s.code()),
        timed_out,
        pipes_left_open: out_open || err_open,
        spawn_error: None,
        elapsed: start.elapsed(),
    }
}

/// Kills the child and its descendants through the job, with `taskkill /T` as a fallback.
pub fn kill_tree(child: &mut std::process::Child, job: &ProcessJob) {
    job.terminate();
    if cfg!(windows) {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

#[cfg(windows)]
mod job {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    type Handle = *mut c_void;

    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;

    #[repr(C)]
    #[derive(Default)]
    struct BasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimitInformation {
        basic: BasicLimitInformation,
        io_counters: [u64; 6],
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn TerminateJobObject(job: Handle, exit_code: u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    pub struct ProcessJob(Handle);

    impl ProcessJob {
        pub fn attach(child: &std::process::Child) -> ProcessJob {
            Self::create(child, false)
        }

        /// Like `attach`, and closing the job kills every process still in it.
        pub fn attach_kill_on_close(child: &std::process::Child) -> ProcessJob {
            Self::create(child, true)
        }

        fn create(child: &std::process::Child, kill_on_close: bool) -> ProcessJob {
            // SAFETY: plain Win32 calls on the live child handle and the job created here.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
                if job.is_null() {
                    return ProcessJob(job);
                }
                if kill_on_close {
                    let mut info = ExtendedLimitInformation::default();
                    info.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                    let ok = SetInformationJobObject(
                        job,
                        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                        &info as *const ExtendedLimitInformation as *const c_void,
                        std::mem::size_of::<ExtendedLimitInformation>() as u32,
                    );
                    if ok == 0 {
                        CloseHandle(job);
                        return ProcessJob(std::ptr::null_mut());
                    }
                }
                if AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
                    CloseHandle(job);
                    return ProcessJob(std::ptr::null_mut());
                }
                ProcessJob(job)
            }
        }

        pub fn is_attached(&self) -> bool {
            !self.0.is_null()
        }

        pub fn terminate(&self) {
            if !self.0.is_null() {
                // SAFETY: self.0 is a live job handle owned by this struct.
                unsafe {
                    TerminateJobObject(self.0, 1);
                }
            }
        }
    }

    impl Drop for ProcessJob {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: closing the handle we created exactly once.
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod job {
    pub struct ProcessJob;
    impl ProcessJob {
        pub fn attach(_child: &std::process::Child) -> ProcessJob {
            ProcessJob
        }
        pub fn attach_kill_on_close(_child: &std::process::Child) -> ProcessJob {
            ProcessJob
        }
        pub fn is_attached(&self) -> bool {
            false
        }
        pub fn terminate(&self) {}
    }
}

pub use job::ProcessJob;

/// Drains a pipe on its own thread, keeping what arrived even when EOF never comes.
pub struct PipeReader {
    data: Arc<Mutex<Vec<u8>>>,
    done: mpsc::Receiver<()>,
}

impl PipeReader {
    pub fn start<R: Read + Send + 'static>(mut pipe: R) -> PipeReader {
        let data = Arc::new(Mutex::new(Vec::new()));
        let (tx, done) = mpsc::channel();
        let sink = Arc::clone(&data);
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => sink.lock().unwrap().extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            let _ = tx.send(());
        });
        PipeReader { data, done }
    }

    /// Returns the collected bytes and whether the pipe was still open at the deadline.
    pub fn finish(self, deadline: Instant) -> (Vec<u8>, bool) {
        let wait = deadline.saturating_duration_since(Instant::now());
        let still_open = self.done.recv_timeout(wait).is_err();
        let bytes = std::mem::take(&mut *self.data.lock().unwrap());
        (bytes, still_open)
    }
}

/// Splits into lines that keep their trailing `\n` (the last one may lack it).
pub fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes.split_inclusive(|&b| b == b'\n').collect()
}

#[derive(Clone, Copy)]
enum Edit {
    Equal,
    Delete(usize), // line index in `a`
    Insert(usize), // line index in `b`
}

fn diff_lines(a: &[&[u8]], b: &[&[u8]]) -> Vec<Edit> {
    if a.len() > MAX_LCS_LINES || b.len() > MAX_LCS_LINES {
        let mut edits = Vec::new();
        for i in 0..a.len().max(b.len()) {
            match (a.get(i), b.get(i)) {
                (Some(x), Some(y)) if x == y => edits.push(Edit::Equal),
                (x, y) => {
                    if x.is_some() {
                        edits.push(Edit::Delete(i));
                    }
                    if y.is_some() {
                        edits.push(Edit::Insert(i));
                    }
                }
            }
        }
        return edits;
    }
    // lcs[i][j] = length of the LCS of a[i..] and b[j..]
    let (n, m) = (a.len(), b.len());
    let width = m + 1;
    let mut lcs = vec![0u16; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * width + j] = if a[i] == b[j] {
                lcs[(i + 1) * width + j + 1] + 1
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut edits = Vec::new();
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            edits.push(Edit::Equal);
            i += 1;
            j += 1;
        } else if j >= m || (i < n && lcs[(i + 1) * width + j] >= lcs[i * width + j + 1]) {
            edits.push(Edit::Delete(i));
            i += 1;
        } else {
            edits.push(Edit::Insert(j));
            j += 1;
        }
    }
    edits
}

pub fn stdout_diff(expected: &[u8], actual: &[u8]) -> String {
    line_diff("stdout", "expected", "actual", expected, actual, MAX_DIFF_LINES)
}

pub fn line_diff(what: &str, left_name: &str, right_name: &str, left: &[u8], right: &[u8], max_lines: usize) -> String {
    let a = split_lines(left);
    let b = split_lines(right);
    let edits = diff_lines(&a, &b);
    let changed: Vec<String> = edits
        .iter()
        .filter_map(|e| match *e {
            Edit::Equal => None,
            Edit::Delete(i) => Some(format!("    - {:>4} | {}", i + 1, show_line(a[i]))),
            Edit::Insert(j) => Some(format!("    + {:>4} | {}", j + 1, show_line(b[j]))),
        })
        .collect();
    let mut s = format!(
        "  {what} differs ({} {left_name} lines, {} {right_name}; {} changed; - {left_name}, + {right_name}):\n",
        a.len(),
        b.len(),
        changed.len()
    );
    for line in changed.iter().take(max_lines) {
        s.push_str(line);
        s.push('\n');
    }
    if changed.len() > max_lines {
        s.push_str(&format!("    ... {} more changed lines\n", changed.len() - max_lines));
    }
    s
}

pub fn stderr_excerpt(stderr: &[u8]) -> String {
    if stderr.is_empty() {
        return "  stderr: (empty)\n".to_string();
    }
    let lines = split_lines(stderr);
    let mut s = format!(
        "  stderr (first {} of {} lines):\n",
        lines.len().min(MAX_STDERR_LINES),
        lines.len()
    );
    for line in lines.iter().take(MAX_STDERR_LINES) {
        s.push_str("    | ");
        s.push_str(&show_line(line));
        s.push('\n');
    }
    s
}

/// Escapes control characters, marks a missing newline and truncates long lines.
pub fn show_line(line: &[u8]) -> String {
    let (body, has_newline) = match line.strip_suffix(b"\n") {
        Some(body) => (body, true),
        None => (line, false),
    };
    let text = String::from_utf8_lossy(body);
    let mut out = String::new();
    for (count, c) in text.chars().enumerate() {
        if count >= MAX_LINE_CHARS {
            out.push_str("...");
            break;
        }
        match c {
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    if !has_newline {
        out.push_str("  [no newline at end]");
    }
    out
}
