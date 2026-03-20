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

fn stax_with_home(args: &[&str], home: &std::path::Path) -> std::process::Output {
    Command::new(stax_bin())
        .args(args)
        .env("HOME", home)
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
    assert!(stdout.contains("run"));
    assert!(stdout.contains("restack"));
    assert!(stdout.contains("resolve"));
}

#[test]
fn test_status_alias_s() {
    // Both aliases should work
    let output1 = stax(&["status", "--help"]);
    let output2 = stax(&["s", "--help"]);
    assert!(output1.status.success());
    assert!(output2.status.success());
}

#[test]
fn test_submit_alias_ss() {
    let output = stax(&["ss", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("draft"));
    assert!(stdout.contains("--open"));
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
    assert!(stdout.contains("delete")); // --no-delete option
    assert!(stdout.contains("delete-upstream-gone"));
    assert!(stdout.contains("safe"));
    assert!(stdout.contains("continue"));
}

#[test]
fn test_run_command_help() {
    let output = stax(&["run", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Run a command on each branch in the stack"));
    assert!(stdout.contains("--fail-fast"));
    assert!(stdout.contains("--all"));
    assert!(stdout.contains("--stack"));
    assert!(stdout.contains("--stack[=<STACK>]"));
}

#[test]
fn test_test_command_backcompat_alias_works() {
    let output = stax(&["test", "--help"]);
    assert!(output.status.success());
}

#[test]
fn test_merge_help_flags_include_when_ready_mode() {
    let output = stax(&["merge", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--when-ready"));
    assert!(stdout.contains("--interval"));
    assert!(stdout.contains("--no-sync"));
}

#[test]
fn test_merge_when_ready_hidden_alias_still_works() {
    let output = stax(&["merge-when-ready", "--help"]);
    assert!(output.status.success());
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
    assert!(stdout.contains("untrack"));
    assert!(stdout.contains("delete"));
    assert!(stdout.contains("reparent"));
    assert!(stdout.contains("fold"));
    assert!(stdout.contains("squash"));
    assert!(stdout.contains("up"));
    assert!(stdout.contains("down"));
    assert!(stdout.contains("submit"));
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
    assert!(stdout.contains("submit"));
}

#[test]
fn test_downstack_commands() {
    let output = stax(&["downstack", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("get"));
    assert!(stdout.contains("submit"));
}

#[test]
fn test_scoped_submit_subcommand_help_flags() {
    for args in [
        ["branch", "submit", "--help"],
        ["upstack", "submit", "--help"],
        ["downstack", "submit", "--help"],
    ] {
        let output = stax(&args);
        assert!(output.status.success(), "{:?}", args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("--no-pr"), "Expected --no-pr in {:?}", args);
        assert!(
            stdout.contains("--no-fetch"),
            "Expected --no-fetch in {:?}",
            args
        );
        assert!(stdout.contains("--open"), "Expected --open in {:?}", args);
        assert!(stdout.contains("--yes"), "Expected --yes in {:?}", args);
        assert!(
            stdout.contains("--no-prompt"),
            "Expected --no-prompt in {:?}",
            args
        );
    }
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
fn test_config_help_includes_reset_ai_flag() {
    let output = stax(&["config", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--reset-ai"));
    assert!(stdout.contains("--no-prompt"));
    assert!(stdout.contains("--yes"));
}

#[test]
fn test_shell_setup_help_uses_static_install_language() {
    let output = stax(&["shell-setup", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("manual install"));
    assert!(stdout.contains("~/.config/stax"));
    assert!(!stdout.contains("eval \"$(stax shell-setup)\""));
}

#[test]
fn test_shell_setup_runs_outside_repo() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let output = Command::new(stax_bin())
        .args(["shell-setup"])
        .current_dir(tmp.path())
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .expect("run shell-setup");

    assert!(output.status.success(), "{:?}", output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("Generated by stax shell-setup"));
    assert!(
        !stdout.contains("Welcome to stax!") && !stderr.contains("Welcome to stax!"),
        "shell-setup should not trigger repo init:\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
}

#[test]
fn test_generate_help_includes_no_prompt_flag() {
    let output = stax(&["generate", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--pr-body"));
    assert!(stdout.contains("--no-prompt"));
    assert!(stdout.contains("--edit"));
}

#[test]
fn test_config_reset_ai_no_prompt_clears_saved_defaults() {
    let temp_dir = std::env::temp_dir().join(format!(
        "stax-cli-test-config-reset-ai-{}",
        std::process::id()
    ));
    let config_dir = temp_dir.join(".config").join("stax");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "[ai]\nagent = \"codex\"\nmodel = \"gpt-5.3-codex\"\n",
    )
    .unwrap();

    let output = stax_with_home(&["config", "--reset-ai", "--no-prompt", "--yes"], &temp_dir);
    assert!(output.status.success(), "{:?}", output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Cleared saved AI defaults"));
    assert!(stdout.contains("Skipped reconfiguration"));

    let updated = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(!updated.contains("agent = \"codex\""));
    assert!(!updated.contains("model = \"gpt-5.3-codex\""));

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_init_help_includes_trunk_flag() {
    let output = stax(&["init", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initialize stax"));
    assert!(stdout.contains("--trunk"));
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
    assert!(stdout.contains("stop-here"));
    assert!(stdout.contains("quiet"));
    assert!(stdout.contains("stop-here"));
    assert!(stdout.contains("submit-after"));
    assert!(stdout.contains("ask"));
    assert!(stdout.contains("yes"));
    assert!(stdout.contains("no"));
}

#[test]
fn test_resolve_help_flags() {
    let output = stax(&["resolve", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--agent"));
    assert!(stdout.contains("--model"));
    assert!(stdout.contains("--max-rounds"));
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
    assert!(stdout.contains("prefix"));
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

// ============================================================================
// Freephite (fp) Command Parity Tests
// These tests ensure stax maintains compatibility with freephite commands
// ============================================================================

#[test]
fn fp_parity_ss_submit_stack() {
    // fp ss -> stax ss (submit stack)
    let output = stax(&["ss", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Submit stack"));
}

#[test]
fn fp_parity_bs_branch_submit() {
    // fp bs -> stax bs (branch submit)
    let output = stax(&["bs", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--no-pr"));
    assert!(stdout.contains("--no-fetch"));
    assert!(stdout.contains("--open"));
    assert!(stdout.contains("--no-prompt"));
}

#[test]
fn fp_parity_rs_repo_sync() {
    // fp rs -> stax rs (repo sync)
    let output = stax(&["rs", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Sync repo"));
}

#[test]
fn fp_parity_bc_branch_create() {
    // fp bc -> stax bc (branch create)
    let output = stax(&["bc", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("message")); // -m flag
    assert!(stdout.contains("from")); // --from flag
}

#[test]
fn fp_parity_bco_branch_checkout() {
    // fp bco -> stax bco (branch checkout)
    let output = stax(&["bco", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_bu_branch_up() {
    // fp bu -> stax bu (branch up)
    let output = stax(&["bu", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("COUNT")); // supports count argument
}

#[test]
fn fp_parity_bd_branch_down() {
    // fp bd -> stax bd (branch down)
    let output = stax(&["bd", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("COUNT")); // supports count argument
}

#[test]
fn fp_parity_s_status() {
    // fp s -> stax s (status)
    let output = stax(&["s", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_ls_status() {
    // fp ls -> stax ls (status/list)
    let output = stax(&["ls", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_l_log() {
    // fp l -> stax l (log)
    let output = stax(&["l", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_co_checkout() {
    // fp co -> stax co (checkout)
    let output = stax(&["co", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_cont_continue() {
    // fp cont -> stax cont (continue)
    let output = stax(&["cont", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_b_branch() {
    // fp b -> stax b (branch subcommand)
    let output = stax(&["b", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("checkout"));
    assert!(stdout.contains("track"));
    assert!(stdout.contains("untrack"));
    assert!(stdout.contains("delete"));
}

#[test]
fn fp_parity_b_c_branch_create() {
    // fp b c -> stax b c (branch create)
    let output = stax(&["b", "c", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_b_co_branch_checkout() {
    // fp b co -> stax b co (branch checkout)
    let output = stax(&["b", "co", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_b_d_branch_delete() {
    // fp b d -> stax b d (branch delete)
    let output = stax(&["b", "d", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_b_u_branch_up() {
    // fp b u -> stax b u (branch up)
    let output = stax(&["b", "u", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_us_upstack() {
    // fp us -> stax us (upstack)
    let output = stax(&["us", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_ds_downstack() {
    // fp ds -> stax ds (downstack)
    let output = stax(&["ds", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_bc_with_message() {
    // fp bc -m "message" -> stax bc -m "message"
    let _output = stax(&["bc", "-m", "test", "--help"]);
    // This tests that -m is a valid flag (help still shows)
    let output2 = stax(&["bc", "--help"]);
    assert!(output2.status.success());
    let stdout = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout.contains("-m"));
    assert!(stdout.contains("--message"));
}

#[test]
fn fp_parity_bc_with_all_flag() {
    // fp bc -a -> stax bc -a (stage all changes)
    let output = stax(&["bc", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-a"));
    assert!(stdout.contains("--all"));
}

#[test]
fn fp_parity_rs_restack_flag() {
    // fp rs --restack -> stax rs --restack
    let output = stax(&["rs", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--restack"));
    assert!(stdout.contains("-r")); // short flag
}

#[test]
fn fp_parity_ss_draft_flag() {
    // fp ss --draft -> stax ss --draft
    let output = stax(&["ss", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--draft"));
    assert!(stdout.contains("-d")); // short flag
}

// ============================================================================
// Graphite (gt) Command Parity Tests
// These tests ensure stax also supports graphite-style commands
// ============================================================================

#[test]
fn gt_parity_create_command() {
    // gt create -> stax create
    let output = stax(&["create", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Create a new branch"));
}

#[test]
fn gt_parity_c_alias() {
    // gt c -> stax c (create alias)
    let output = stax(&["c", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_create_am_flags() {
    // gt create -am "message" -> stax create -am "message"
    let output = stax(&["create", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-a"));
    assert!(stdout.contains("--all"));
    assert!(stdout.contains("-m"));
    assert!(stdout.contains("--message"));
}

#[test]
fn gt_parity_modify_command() {
    // gt modify -> stax modify
    let output = stax(&["modify", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("amend"));
}

#[test]
fn gt_parity_m_alias() {
    // gt m -> stax m (modify alias)
    let output = stax(&["m", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_up_command() {
    // gt up -> stax up
    let output = stax(&["up", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Move up"));
    assert!(stdout.contains("COUNT"));
}

#[test]
fn gt_parity_u_alias() {
    // gt u -> stax u (up alias)
    let output = stax(&["u", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_down_command() {
    // gt down -> stax down
    let output = stax(&["down", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Move down"));
    assert!(stdout.contains("COUNT"));
}

#[test]
fn gt_parity_d_alias() {
    // gt d -> stax d (down alias)
    let output = stax(&["d", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_top_command() {
    // gt top -> stax top
    let output = stax(&["top", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("top") || stdout.contains("tip"));
}

#[test]
fn gt_parity_bottom_command() {
    // gt bottom -> stax bottom
    let output = stax(&["bottom", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("bottom") || stdout.contains("base"));
}

#[test]
fn gt_parity_trunk_command() {
    // gt checkout --trunk -> stax trunk (or stax t)
    let output = stax(&["trunk", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_t_alias() {
    // stax t -> trunk
    let output = stax(&["t", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_pr_command() {
    // gt pr -> stax pr (open PR in browser)
    let output = stax(&["pr", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("open"));
    assert!(stdout.contains("list"));
}

#[test]
fn gt_parity_pr_open_subcommand() {
    let output = stax(&["pr", "open", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_pr_list_subcommand() {
    let output = stax(&["pr", "list", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--limit"));
    assert!(stdout.contains("--json"));
}

#[test]
fn gt_parity_issue_list_subcommand() {
    let output = stax(&["issue", "list", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--limit"));
    assert!(stdout.contains("--json"));
}

#[test]
fn gt_parity_submit_command() {
    // gt submit -> stax submit
    let output = stax(&["submit", "--help"]);
    assert!(output.status.success());
}

// ============================================================================
// Rename Command Tests
// ============================================================================

#[test]
fn test_rename_help() {
    let output = stax(&["rename", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Rename"));
    assert!(stdout.contains("--edit"));
}

#[test]
fn test_branch_rename_help() {
    let output = stax(&["branch", "rename", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Rename"));
}

#[test]
fn test_branch_rename_alias() {
    // b r should work as alias
    let output = stax(&["b", "r", "--help"]);
    assert!(output.status.success());
}

// ============================================================================
// LL Command Tests
// ============================================================================

#[test]
fn test_ll_command_help() {
    let output = stax(&["ll", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PR") || stdout.contains("details") || stdout.contains("full"));
}

#[test]
fn test_ll_command_flags() {
    let output = stax(&["ll", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--json"));
    assert!(stdout.contains("--stack"));
    assert!(stdout.contains("--current"));
    assert!(stdout.contains("--compact"));
    assert!(stdout.contains("--quiet"));
}

// ============================================================================
// Rename --push Flag Tests
// ============================================================================

#[test]
fn test_rename_push_flag_help() {
    let output = stax(&["rename", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--push") || stdout.contains("-p"),
        "Expected --push flag in rename help: {}",
        stdout
    );
}

#[test]
fn test_branch_rename_push_flag_help() {
    let output = stax(&["branch", "rename", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--push") || stdout.contains("-p"),
        "Expected --push flag in branch rename help: {}",
        stdout
    );
}

// ============================================================================
// CI Command Tests
// ============================================================================

#[test]
fn test_ci_command_help() {
    let output = stax(&["ci", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("CI") || stdout.contains("status"),
        "Expected CI-related help text: {}",
        stdout
    );
}

#[test]
fn test_ci_command_flags() {
    let output = stax(&["ci", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--all"), "Expected --all flag: {}", stdout);
    assert!(
        stdout.contains("--json"),
        "Expected --json flag: {}",
        stdout
    );
    assert!(
        stdout.contains("--refresh"),
        "Expected --refresh flag: {}",
        stdout
    );
}
