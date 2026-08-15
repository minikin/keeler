//! The binary's contract with whoever runs it: what it prints, and — the
//! part a library test cannot see — the exit code it leaves behind.
//!
//! CI reads exit codes, not prose. A release step that fails while
//! reporting success is the one failure mode that must never happen.

use std::process::Command;

fn xtask(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .output()
        .expect("failed to run the xtask binary")
}

#[test]
fn a_command_that_succeeds_exits_zero_and_prints_its_result() {
    let path = std::env::temp_dir().join(format!("xtask-cli-ok-{}", std::process::id()));
    std::fs::write(&path, "## [1.0.0]\n\n- shipped\n").unwrap();

    let output = xtask(&["release-notes", "1.0.0", &path.display().to_string()]);

    assert!(output.status.success(), "exit code was not zero");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "- shipped"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_command_that_fails_exits_non_zero_and_says_why_on_stderr() {
    let path = std::env::temp_dir().join(format!("xtask-cli-bad-{}", std::process::id()));
    std::fs::write(&path, "## [1.0.0]\n\n- shipped\n").unwrap();

    let output = xtask(&["release-notes", "2.0.0", &path.display().to_string()]);

    assert!(
        !output.status.success(),
        "a failed extraction reported success — CI would publish on it",
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("2.0.0"),
        "the failure does not name the version",
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "a failed command printed a result to stdout",
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn an_unknown_command_exits_non_zero() {
    let output = xtask(&["fly"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("fly"));
}
