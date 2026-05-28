use crate::cli::args::{
    BranchCommands, Cli, CliSubcommand, CommandPolicy, Commands, RestackSubmitAfter, StackCommands,
    WorktreeCommands,
};
use crate::cli::interactive::{
    check_interactive_terminal_with_probe, detect_interactive_stdio, has_interactive_terminal,
    InteractiveTerminalCheck,
};
use clap::Parser;
use std::cell::Cell;

fn try_parse_cli(args: &[&str]) -> Result<Cli, clap::Error> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    std::thread::Builder::new()
        .name("cli-parse".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || Cli::try_parse_from(args))
        .expect("spawn parse thread")
        .join()
        .expect("join parse thread")
}

fn parse_cli(args: &[&str]) -> Cli {
    try_parse_cli(args).expect("parse CLI")
}

#[test]
fn interactive_terminal_requires_both_stdio_streams() {
    assert!(has_interactive_terminal(true, true));
    assert!(!has_interactive_terminal(true, false));
    assert!(!has_interactive_terminal(false, true));
    assert!(!has_interactive_terminal(false, false));
}

#[test]
fn interactive_stdio_can_be_forced_for_tests() {
    #[cfg(debug_assertions)]
    {
        std::env::set_var("STAX_TEST_FORCE_INTERACTIVE_TERMINAL", "1");
        assert_eq!(detect_interactive_stdio(), (true, true));
        std::env::remove_var("STAX_TEST_FORCE_INTERACTIVE_TERMINAL");
    }
}

#[test]
fn interactive_dashboard_skips_probe_without_a_tty() {
    let probe_called = Cell::new(false);
    let check = check_interactive_terminal_with_probe(true, false, || {
        probe_called.set(true);
        Ok(())
    });

    assert_eq!(
        check,
        InteractiveTerminalCheck {
            available: false,
            reason: None,
        }
    );
    assert!(!probe_called.get());
}

#[test]
fn interactive_dashboard_requires_input_reader() {
    let check =
        check_interactive_terminal_with_probe(true, true, || Err("reader init failed".into()));

    assert_eq!(
        check,
        InteractiveTerminalCheck {
            available: false,
            reason: Some("reader init failed".into()),
        }
    );
}

#[test]
fn interactive_dashboard_launches_when_probe_succeeds() {
    let check = check_interactive_terminal_with_probe(true, true, || Ok(()));

    assert_eq!(
        check,
        InteractiveTerminalCheck {
            available: true,
            reason: None,
        }
    );
}

#[test]
fn bare_worktree_command_parses_without_subcommand() {
    let cli = parse_cli(&["stax", "wt"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Worktree { command: None })
    ));
}

#[test]
fn explicit_worktree_subcommand_still_parses() {
    let cli = parse_cli(&["stax", "wt", "ls"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Worktree { command: Some(_) })
    ));
}

#[test]
fn worktree_cleanup_subcommand_parses() {
    let cli = parse_cli(&["stax", "wt", "cleanup", "--force", "--dry-run", "--yes"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Worktree {
            command: Some(WorktreeCommands::Cleanup {
                force: true,
                dry_run: true,
                yes: true
            })
        })
    ));
}

#[test]
fn lane_command_parses_without_name_for_picker() {
    let cli = parse_cli(&["stax", "lane"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Lane {
            name: None,
            prompt: None,
            ..
        })
    ));
}

#[test]
fn lane_command_parses_explicit_name_and_prompt() {
    let cli = parse_cli(&["stax", "lane", "review-pass", "fix macOS build"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Lane {
            name: Some(name),
            prompt: Some(prompt),
            ..
        }) if name == "review-pass" && prompt == "fix macOS build"
    ));
}

#[test]
fn lane_command_accepts_hidden_shell_output_after_prompt() {
    let cli = parse_cli(&[
        "stax",
        "lane",
        "review-pass",
        "fix macOS build",
        "--shell-output",
    ]);
    assert!(matches!(
        cli.command,
        Some(Commands::Lane {
            name: Some(name),
            prompt: Some(prompt),
            shell_output: true,
            ..
        }) if name == "review-pass" && prompt == "fix macOS build"
    ));
}

#[test]
fn lane_requires_agent_when_model_is_set() {
    match try_parse_cli(&["stax", "lane", "review-pass", "--model", "gpt-5.4"]) {
        Ok(_) => panic!("expected clap error"),
        Err(err) => assert!(err.to_string().contains("--agent")),
    }
}

#[test]
fn restack_defaults_to_not_submitting_after_success() {
    let cli = parse_cli(&["stax", "restack"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Restack {
            stop_here: false,
            submit_after: RestackSubmitAfter::No,
            ..
        })
    ));
}

#[test]
fn restack_parses_stop_here_flag() {
    let cli = parse_cli(&["stax", "restack", "--stop-here"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Restack {
            stop_here: true,
            ..
        })
    ));
}

#[test]
fn stack_restack_via_two_tokens() {
    let cli = parse_cli(&["stax", "s", "r"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Stack(StackCommands::Restack {
            submit_after: RestackSubmitAfter::No,
            ..
        }))
    ));
}

#[test]
fn stack_restack_via_single_token_sr() {
    let cli = parse_cli(&["stax", "sr"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Sr {
            submit_after: RestackSubmitAfter::No,
            ..
        })
    ));
}

#[test]
fn stack_submit_via_two_tokens() {
    let cli = parse_cli(&["stax", "s", "s"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Stack(StackCommands::Submit { .. }))
    ));
}

#[test]
fn ss_still_parses_as_top_level_submit() {
    let cli = parse_cli(&["stax", "ss"]);
    assert!(matches!(cli.command, Some(Commands::Submit { .. })));
}

#[test]
fn submit_ai_flags_parse_for_full_title_and_body_generation() {
    let cli = parse_cli(&["stax", "ss", "--ai", "--title", "--body", "--yes"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Submit { submit }) if submit.ai
            && submit.title
            && submit.body
            && submit.yes
    ));
}

#[test]
fn create_ai_flags_parse_for_generated_branch_details() {
    let cli = parse_cli(&["stax", "create", "--ai", "--yes"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Create {
            ai: true,
            yes: true,
            ..
        })
    ));
}

#[test]
fn create_add_alias_parses_as_create_command() {
    let cli = parse_cli(&["stax", "add", "feature-alias", "--below"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Create {
            name: Some(ref name),
            below: true,
            ..
        }) if name == "feature-alias"
    ));
}

#[test]
fn branch_create_ai_flags_parse() {
    let cli = parse_cli(&["stax", "branch", "create", "--ai", "-a"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Branch(BranchCommands::Create {
            ai: true,
            all: true,
            ..
        }))
    ));
}

#[test]
fn hidden_bc_ai_flags_parse() {
    let cli = parse_cli(&["stax", "bc", "--ai", "--yes"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Bc {
            ai: true,
            yes: true,
            ..
        })
    ));
}

#[test]
fn branch_submit_body_scope_modifier_parses() {
    let cli = parse_cli(&["stax", "bs", "--ai", "--body"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Bs { submit }) if submit.ai && !submit.title && submit.body
    ));
}

#[test]
fn title_and_body_modifiers_require_ai() {
    assert!(try_parse_cli(&["stax", "submit", "--title"]).is_err());
    assert!(try_parse_cli(&["stax", "submit", "--body"]).is_err());
}

#[test]
fn removed_legacy_body_flag_is_rejected() {
    let removed_flag = ["--ai", "-body"].concat();
    assert!(try_parse_cli(&["stax", "submit", &removed_flag]).is_err());
}

#[test]
fn ls_parses_as_status() {
    let cli = parse_cli(&["stax", "ls"]);
    assert!(matches!(cli.command, Some(Commands::Status { .. })));
}

#[test]
fn submit_backward_compat() {
    let cli = parse_cli(&["stax", "submit"]);
    assert!(matches!(cli.command, Some(Commands::Submit { .. })));
}

#[test]
fn restack_backward_compat() {
    let cli = parse_cli(&["stax", "restack"]);
    assert!(matches!(cli.command, Some(Commands::Restack { .. })));
}

#[test]
fn cli_upgrade_parses() {
    let cli = parse_cli(&["stax", "cli", "upgrade"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Cli {
            command: CliSubcommand::Upgrade
        })
    ));
}

#[test]
fn s_alone_shows_stack_group() {
    let result = try_parse_cli(&["stax", "s"]);
    assert!(result.is_err(), "bare `s` should require a subcommand");
}

#[test]
fn split_parses_file_flag() {
    let cli = parse_cli(&["stax", "split", "--file", "src/main.rs"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Split {
            hunk: false,
            ref file,
            no_verify: false,
        }) if file == &["src/main.rs"]
    ));
}

#[test]
fn split_parses_short_file_flag() {
    let cli = parse_cli(&["stax", "split", "-f", "src/main.rs", "src/lib.rs"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Split {
            hunk: false,
            ref file,
            no_verify: false,
        }) if file == &["src/main.rs", "src/lib.rs"]
    ));
}

#[test]
fn split_file_and_hunk_conflict() {
    let result = try_parse_cli(&["stax", "split", "--hunk", "--file", "foo.rs"]);
    assert!(result.is_err(), "--hunk and --file should conflict");
}

#[test]
fn continue_is_marked_as_rebase_control() {
    let cli = parse_cli(&["stax", "continue"]);
    let cmd = cli.command.expect("command");
    assert_eq!(cmd.policy(), CommandPolicy::RebaseControl);
    assert!(cmd.allows_during_rebase());
}

#[test]
fn sync_continue_is_marked_as_rebase_safe() {
    let cli = parse_cli(&["stax", "sync", "--continue"]);
    let cmd = cli.command.expect("command");
    assert_eq!(cmd.policy(), CommandPolicy::RebaseSafe);
    assert!(cmd.allows_during_rebase());
}

#[test]
fn status_requires_clean_repo_state() {
    let cli = parse_cli(&["stax", "status"]);
    let cmd = cli.command.expect("command");
    assert_eq!(cmd.policy(), CommandPolicy::RequiresCleanRepoState);
    assert!(!cmd.allows_during_rebase());
}
