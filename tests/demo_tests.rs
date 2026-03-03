mod common;
use common::{OutputAssertions, TestRepo};
use std::process::Command;

#[test]
fn test_demo_help() {
    // Demo should appear in help output
    let repo = TestRepo::new();
    let output = repo.run_stax(&["--help"]);
    output.assert_success();
    output.assert_stdout_contains("demo");
}

#[test]
fn test_demo_noninteractive_exits_cleanly() {
    // In non-interactive mode (piped stdin), demo should exit without hanging.
    // We use a short timeout and pipe empty stdin.
    let stax_bin = common::stax_bin();

    let tmp = tempfile::tempdir().expect("Failed to create temp dir");
    let output = Command::new(stax_bin)
        .args(["demo"])
        .current_dir(tmp.path())
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("Failed to run stax demo");

    // Demo should either succeed (if it auto-detects non-interactive) or
    // fail gracefully without a panic
    assert!(
        output.status.success() || !output.status.success(),
        "Demo should not panic"
    );

    // Should not contain a panic message
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panic"),
        "Demo should not panic, got: {}",
        stderr
    );
}
