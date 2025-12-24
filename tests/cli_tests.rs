use std::process::Command;

/// Get path to compiled binary (built by cargo test)
fn stax_bin() -> &'static str {
    env!("CARGO_BIN_EXE_stax")
}

fn stax(args: &[&str]) -> std::process::Output {
    Command::new(stax_bin())
        .args(args)
        .output()
        .expect("Failed to execute stax")
}

#[test]
fn test_help() {
    let output = stax(&["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Fast stacked Git branches and PRs"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("submit"));
    assert!(stdout.contains("restack"));
}

#[test]
fn test_status_alias_s() {
    // Both should work and produce similar output
    let output1 = stax(&["status"]);
    let output2 = stax(&["s"]);
    assert!(output1.status.success());
    assert!(output2.status.success());
}

#[test]
fn test_submit_alias_ss() {
    let output = stax(&["ss", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("draft"));
}

#[test]
fn test_restack_alias_rs() {
    let output = stax(&["rs", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("all"));
}

#[test]
fn test_checkout_aliases() {
    // co and bco should both work
    let output1 = stax(&["co", "--help"]);
    let output2 = stax(&["bco", "--help"]);
    assert!(output1.status.success());
    assert!(output2.status.success());
}

#[test]
fn test_branch_subcommands() {
    let output = stax(&["branch", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("track"));
    assert!(stdout.contains("delete"));
}

#[test]
fn test_bc_shortcut() {
    // bc should work as hidden shortcut
    let output = stax(&["bc", "--help"]);
    assert!(output.status.success());
}

#[test]
fn test_bd_shortcut() {
    // bd should work as hidden shortcut
    let output = stax(&["bd", "--help"]);
    assert!(output.status.success());
}

#[test]
fn test_upstack_commands() {
    let output = stax(&["upstack", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("restack"));
}

#[test]
fn test_us_alias() {
    let output = stax(&["us", "--help"]);
    assert!(output.status.success());
}
