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
    assert!(stdout.contains("reviewers"));
    assert!(stdout.contains("labels"));
    assert!(stdout.contains("assignees"));
    assert!(stdout.contains("no-prompt"));
    assert!(stdout.contains("yes"));
}

#[test]
fn test_sync_alias_rs() {
    let output = stax(&["rs", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("restack")); // --restack option
    assert!(stdout.contains("delete"));  // --no-delete option
    assert!(stdout.contains("safe"));
    assert!(stdout.contains("continue"));
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
    assert!(stdout.contains("reparent"));
    assert!(stdout.contains("fold"));
    assert!(stdout.contains("squash"));
    assert!(stdout.contains("up"));
    assert!(stdout.contains("down"));
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

#[test]
fn test_config_command() {
    let output = stax(&["config"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Config path:"));
    assert!(stdout.contains(".config/stax/config.toml"));
}

#[test]
fn test_status_help_flags() {
    let output = stax(&["status", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("json"));
    assert!(stdout.contains("stack"));
    assert!(stdout.contains("all"));
    assert!(stdout.contains("compact"));
    assert!(stdout.contains("quiet"));
}

#[test]
fn test_log_help_flags() {
    let output = stax(&["log", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("json"));
    assert!(stdout.contains("stack"));
    assert!(stdout.contains("all"));
    assert!(stdout.contains("compact"));
    assert!(stdout.contains("quiet"));
}

#[test]
fn test_restack_help_flags() {
    let output = stax(&["restack", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("continue"));
    assert!(stdout.contains("quiet"));
}

#[test]
fn test_checkout_help_flags() {
    let output = stax(&["checkout", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("trunk"));
    assert!(stdout.contains("parent"));
    assert!(stdout.contains("child"));
}

#[test]
fn test_branch_create_help_flags() {
    let output = stax(&["branch", "create", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("from"));
}

#[test]
fn test_diff_help_flags() {
    let output = stax(&["diff", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stack"));
    assert!(stdout.contains("all"));
}

#[test]
fn test_range_diff_help_flags() {
    let output = stax(&["range-diff", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stack"));
    assert!(stdout.contains("all"));
}

#[test]
fn test_doctor_help() {
    let output = stax(&["doctor", "--help"]);
    assert!(output.status.success());
}
