//! Live differential parity suite against a reference Git Bash, run by `parity/harness.rs`.

mod support;
#[path = "parity/harness.rs"]
mod harness;

use std::path::Path;

use harness::{posix, Suite};

const SUITE: Suite = Suite {
    accepted: "accepted.txt",
    host_command,
};

#[test]
#[ignore = "needs the reference Git copy from tests/parity/setup-ref.ps1, run with cargo test --test parity -- --ignored"]
fn parity() {
    let mut cases = harness::compat_cases();
    cases.extend(harness::group_cases(&harness::parity_dir().join("cases")));
    harness::run(&SUITE, cases);
}

#[test]
fn host_command_line() {
    assert_eq!(
        host_command("cd sub\necho it's", Path::new("C:\\p q\\cwd")),
        "eval 'cd sub\necho it'\"'\"'s' < /dev/null && pwd -P >| '/c/p q/cwd'"
    );
}

fn host_command(body: &str, cwd_file: &Path) -> String {
    format!("eval {} < /dev/null && pwd -P >| {}", quote(body), quote(&posix(cwd_file)))
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}
