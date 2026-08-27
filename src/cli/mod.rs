use crate::{commands, config::Config, git::GitRepo, tui, update};
use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::generate;

mod args;
mod interactive;
#[cfg(test)]
mod tests;

use args::*;
use interactive::*;

fn run_submit(submit: SubmitOptions, scope: commands::submit::SubmitScope) -> Result<()> {
    commands::submit::run(scope, submit.into())
}

fn print_subcommand_help(name: &str) -> Result<()> {
    let mut cmd = Cli::command();
    let subcommand = cmd
        .find_subcommand_mut(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown subcommand '{}'", name))?;
    subcommand.print_help()?;
    println!();
    Ok(())
}

pub fn run() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();

    if cli.default_config {
        print!("{}", Config::default_toml()?);
        return Ok(());
    }

    if cli.skill {
        return commands::skills::run_print_skill();
    }

    if matches!(&cli.command, Some(Commands::UpdateCheck)) {
        update::run_background_check();
        return Ok(());
    }

    let _trace = crate::git::command::TraceGuard::start(cli.trace);
    update::spawn_background_check();

    if let Some(Commands::Web(args)) = &cli.command {
        return crate::commands::web::run(args.path.clone(), args.port, args.no_open);
    }

    if let Some(Commands::Setup {
        print,
        refresh,
        skip_skills,
        install_skills,
        skip_auth,
        auth_from_gh,
        yes,
        skills,
    }) = &cli.command
    {
        let skill_selection = skills
            .as_deref()
            .map(commands::skills::parse_harness_selection)
            .transpose()?;
        let skill_install_mode = if *skip_skills
            || matches!(
                skill_selection.as_ref(),
                Some(commands::skills::HarnessSelection::Only(ids)) if ids.is_empty()
            ) {
            commands::shell_setup::SkillInstallMode::Skip
        } else if *install_skills
            || skill_selection
                .as_ref()
                .is_some_and(|sel| !matches!(sel, commands::skills::HarnessSelection::Only(ids) if ids.is_empty()))
        {
            commands::shell_setup::SkillInstallMode::Install
        } else {
            commands::shell_setup::SkillInstallMode::Ask
        };
        let auth_setup_mode = if *auth_from_gh {
            commands::shell_setup::AuthSetupMode::ImportFromGh
        } else if *skip_auth {
            commands::shell_setup::AuthSetupMode::Skip
        } else {
            commands::shell_setup::AuthSetupMode::Ask
        };
        let setup_options = commands::shell_setup::SetupOptions {
            auto_accept: *yes,
            skill_install_mode,
            auth_setup_mode,
            skill_selection,
        };
        let result = commands::shell_setup::run(*print, *refresh, setup_options);
        update::show_update_notification();
        return result;
    }

    // Ensure config exists (creates default on first run)
    let _ = Config::ensure_exists();
    let _ = commands::shell_setup::refresh_installed_snippets();

    let (stdin_is_terminal, stdout_is_terminal) = detect_interactive_stdio();

    // Bare `st`/`stax` should only enter the TUI when both sides are interactive.
    // In shells or wrappers without a usable TTY, fall back to the regular status view.
    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            let interactive_terminal =
                check_interactive_terminal(stdin_is_terminal, stdout_is_terminal);
            if interactive_terminal.available {
                // TUI requires initialized repo
                commands::init::ensure_initialized()?;
                let result = tui::run();
                update::show_update_notification();
                return result;
            }

            print_interactive_fallback(
                interactive_terminal.reason.as_deref(),
                "dashboard",
                "falling back to status view",
            );

            Commands::Status {
                json: false,
                stack: None,
                current: false,
                compact: false,
                quiet: false,
            }
        }
    };

    // Commands that don't need repo initialization
    match &command {
        Commands::Completions { shell } => {
            generate(*shell, &mut Cli::command(), "st", &mut std::io::stdout());
            return Ok(());
        }
        Commands::Auth {
            token,
            from_gh,
            command,
        } => {
            if command.is_some() && (token.is_some() || *from_gh) {
                anyhow::bail!("`stax auth status` cannot be combined with --token or --from-gh.");
            }
            let result = match command {
                Some(AuthSubcommand::Status) => commands::auth::status(),
                None => commands::auth::run(token.clone(), *from_gh),
            };
            update::show_update_notification();
            return result;
        }
        Commands::Cli { command } => {
            let result = match command {
                CliSubcommand::Upgrade => commands::cli::run_upgrade(),
            };
            return result;
        }
        Commands::Update { force } => {
            commands::cli::run_update(*force)?;
            commands::skills::propose_skills_update()?;
            return Ok(());
        }
        Commands::Config {
            reset_ai,
            no_prompt,
            yes,
            set_ai,
        } => {
            let result = commands::config::run(*reset_ai, *no_prompt, *yes, *set_ai);
            update::show_update_notification();
            return result;
        }
        Commands::Init { trunk } => {
            let result = commands::init::run(trunk.clone());
            update::show_update_notification();
            return result;
        }
        Commands::Doctor { fix } => {
            let result = commands::doctor::run(*fix);
            update::show_update_notification();
            return result;
        }
        Commands::Skills { command } => {
            let result = match command {
                None | Some(SkillsCommands::List) => commands::skills::run_list(),
                Some(SkillsCommands::Update {
                    dry_run,
                    all,
                    skills,
                }) => {
                    let (sel, origin) = if *all {
                        (
                            commands::skills::HarnessSelection::All,
                            commands::skills::SkillsUpdateOrigin::AllFlag,
                        )
                    } else if let Some(spec) = skills.as_deref() {
                        (
                            commands::skills::parse_harness_selection(spec)?,
                            commands::skills::SkillsUpdateOrigin::Cli {
                                spec: spec.to_string(),
                            },
                        )
                    } else {
                        commands::skills::configured_selection_with_origin()
                    };
                    commands::skills::run_update_with(*dry_run, &sel, origin)
                }
            };
            update::show_update_notification();
            return result;
        }
        Commands::Demo => {
            let result = commands::demo::run();
            update::show_update_notification();
            return result;
        }
        _ => {}
    }

    // Ensure repo is initialized for all other commands
    commands::init::ensure_initialized()?;

    // Block commands that do not explicitly support running during an active rebase.
    if !command.allows_during_rebase()
        && let Ok(repo) = GitRepo::open()
        && repo.rebase_in_progress().unwrap_or(false)
    {
        anyhow::bail!(
            "A rebase is in progress. Resolve conflicts and run one of:\n  \
                     stax resolve\n  stax continue\n  stax abort"
        );
    }

    let result = match command {
        Commands::Completions { .. } => unreachable!("handled before repository initialization"),
        Commands::Web(_) => unreachable!("handled before repository initialization"),
        Commands::Status {
            json,
            stack,
            current,
            compact,
            quiet,
        } => commands::status::run(json, stack, current, compact, quiet, false),
        Commands::Ll {
            json,
            stack,
            current,
            compact,
            quiet,
        } => commands::status::run(json, stack, current, compact, quiet, true),
        Commands::Log {
            json,
            stack,
            current,
            compact,
            quiet,
        } => commands::log::run(json, stack, current, compact, quiet),
        Commands::Submit { submit } => run_submit(submit, commands::submit::SubmitScope::Stack),
        Commands::Merge {
            all,
            full,
            downstack_only,
            dry_run,
            method,
            no_delete,
            no_wait,
            timeout,
            when_ready,
            remote,
            stack,
            queue,
            interval,
            no_sync,
            yes,
            quiet,
        } => {
            let default_method = if stack { "merge" } else { "squash" };
            let merge_method = method.as_deref().unwrap_or(default_method).parse()?;
            if queue {
                commands::merge_queue::run(all, timeout, interval, no_sync, yes, quiet)
            } else if remote {
                commands::merge_remote::run(
                    all,
                    merge_method,
                    timeout,
                    interval,
                    no_delete,
                    no_sync,
                    yes,
                    quiet,
                )
            } else if stack {
                commands::merge_stack::run(
                    all || full,
                    downstack_only,
                    dry_run,
                    when_ready,
                    merge_method,
                    timeout,
                    interval,
                    no_delete,
                    no_sync,
                    yes,
                    quiet,
                )
            } else if when_ready {
                commands::merge_when_ready::run(
                    all,
                    downstack_only,
                    merge_method,
                    timeout,
                    interval,
                    no_delete,
                    no_sync,
                    yes,
                    quiet,
                )
            } else {
                commands::merge::run(
                    all,
                    downstack_only,
                    dry_run,
                    merge_method,
                    no_delete,
                    no_wait,
                    timeout,
                    no_sync,
                    yes,
                    quiet,
                )
            }
        }
        Commands::MergeWhenReady {
            all,
            downstack_only,
            method,
            timeout,
            interval,
            no_delete,
            no_sync,
            yes,
            quiet,
        } => {
            let merge_method = method.parse().unwrap_or_default();
            commands::merge_when_ready::run(
                all,
                downstack_only,
                merge_method,
                timeout,
                interval,
                no_delete,
                no_sync,
                yes,
                quiet,
            )
        }
        Commands::Sync {
            restack,
            prune,
            full,
            no_delete,
            delete_upstream_gone,
            force,
            safe,
            r#continue,
            quiet,
            verbose,
            auto_stash_pop,
            stash,
            no_stash,
            dry_run,
            json,
        } => {
            if prune {
                eprintln!("{}", commands::sync::prune_deprecation_warning());
            }
            let stash_policy = commands::sync::StashPolicy::from_flags(stash, no_stash);
            if dry_run {
                commands::sync_plan::run(commands::sync_plan::SyncPlanOptions {
                    restack,
                    full,
                    delete_merged: !no_delete,
                    delete_upstream_gone,
                    force,
                    safe,
                    quiet,
                    verbose,
                    auto_stash_pop,
                    stash_policy,
                    json,
                })
            } else {
                commands::sync::run(
                    restack,
                    full,
                    !no_delete,
                    delete_upstream_gone,
                    force,
                    safe,
                    r#continue,
                    quiet,
                    verbose,
                    auto_stash_pop,
                    stash_policy,
                    json,
                    &[],
                    false,
                )
            }
        }
        Commands::Sweep {
            delete,
            include_stale,
            force,
            stale_days,
            json,
        } => commands::sweep::run(delete, include_stale, force, stale_days, json),
        Commands::Restack {
            all,
            stop_here,
            r#continue,
            dry_run,
            yes,
            quiet,
            auto_stash_pop,
            submit_after,
        } => commands::restack::run(
            all,
            stop_here,
            r#continue,
            dry_run,
            yes,
            quiet,
            auto_stash_pop,
            submit_after.into(),
        ),
        Commands::Cascade {
            no_pr,
            no_submit,
            auto_stash_pop,
        } => commands::cascade::run(no_pr, no_submit, auto_stash_pop),
        Commands::Refresh {
            no_pr,
            no_submit,
            force,
            safe,
            verbose,
            yes,
            no_prompt,
            auto_stash_pop,
            all_stacks,
        } => commands::refresh::run(
            no_pr,
            no_submit,
            force,
            safe,
            verbose,
            yes,
            no_prompt,
            auto_stash_pop,
            all_stacks,
        ),
        Commands::Checkout {
            branch,
            pr,
            trunk,
            parent,
            child,
            shell_output,
        } => commands::checkout::run(branch, pr, trunk, parent, child, shell_output),
        Commands::Get {
            branch,
            parent,
            no_checkout,
            force,
            downstack,
            remote_upstack,
            no_restack,
            unfrozen,
        } => commands::get::run(commands::get::GetOptions {
            branch,
            parent,
            no_checkout,
            force,
            downstack,
            remote_upstack,
            no_restack,
            unfrozen,
        }),
        Commands::Continue => commands::continue_cmd::run_and_resume_restack(),
        Commands::Resolve {
            agent,
            model,
            max_rounds,
        } => commands::resolve::run(agent, model, max_rounds),
        Commands::Abort => commands::abort::run(),
        Commands::Modify {
            message,
            all,
            quiet,
            no_verify,
            restack,
        } => commands::modify::run(message, all, quiet, no_verify, restack),
        Commands::Auth { .. } => unreachable!(), // Handled above
        Commands::Cli { .. } => unreachable!(),  // Handled above
        Commands::Update { .. } => unreachable!(), // Handled above
        Commands::Config { .. } => unreachable!(), // Handled above
        Commands::Init { .. } => unreachable!(), // Handled above
        Commands::Diff { stack, all } => commands::diff::run(stack, all),
        Commands::RangeDiff { stack, all } => commands::range_diff::run(stack, all),
        Commands::Doctor { .. } => unreachable!(), // Handled above
        Commands::Skills { .. } => unreachable!(), // Handled above
        Commands::Trunk { branch } => {
            if let Some(name) = branch {
                commands::set_trunk::run(&name)
            } else {
                commands::checkout::run(None, None, true, false, None, false)
            }
        }
        Commands::Up { count } => commands::navigate::up(count),
        Commands::Down { count } => commands::navigate::down(count),
        Commands::Top => commands::navigate::top(),
        Commands::Bottom => commands::navigate::bottom(),
        Commands::Prev => commands::navigate::prev(),
        Commands::Next => commands::navigate::next(),
        Commands::Create {
            name,
            all,
            message,
            ai,
            yes,
            from,
            prefix,
            insert,
            below,
            no_verify,
        } => commands::branch::create::run(
            name, message, from, prefix, all, insert, below, no_verify, ai, yes,
        ),
        Commands::Pr { command } => match command.unwrap_or(PrCommands::Open) {
            PrCommands::Open => commands::pr::run_open(),
            PrCommands::Body { edit } => commands::pr::run_body(edit),
            PrCommands::List {
                limit,
                json,
                ready,
                all,
                current,
                stack,
                plain,
            } => commands::pr::run_list(limit, json, ready, all, current, stack, plain),
        },
        Commands::Ready {
            all,
            current,
            stack,
            json,
            plain,
            interval,
        } => commands::ready::run(
            commands::ready::ReadyScopeMode::from_flags(all, current, stack),
            json,
            plain,
            interval,
        ),
        Commands::Issue { command } => match command {
            Some(IssueCommands::List { limit, json }) => commands::issue::run_list(limit, json),
            None => print_subcommand_help("issue"),
        },
        Commands::Open => commands::open::run(),
        Commands::Draft { branch, stack } => commands::draft::run(branch, stack, true),
        Commands::Undraft { branch, stack } => commands::draft::run(branch, stack, false),
        Commands::Comments {
            plain,
            stack,
            all,
            json,
        } => commands::comments::run(plain, stack, all, json),
        Commands::Ci {
            all,
            stack,
            json,
            refresh,
            watch,
            alert,
            no_alert,
            strict,
            interval,
            verbose,
            oneline,
        } => commands::ci::run(
            all,
            stack,
            json,
            refresh,
            watch,
            alert.map(|value| match value {
                Some(path) => commands::ci::CiAlertSoundArg::Path(path),
                None => commands::ci::CiAlertSoundArg::DefaultSound,
            }),
            no_alert,
            strict,
            interval,
            verbose,
            oneline,
        ),
        Commands::Watch {
            current,
            interval,
            iterations,
        } => commands::watch::run(current, interval, iterations),
        Commands::Tmux { command } => commands::tmux::run(command),
        Commands::Split {
            hunk,
            file,
            no_verify,
        } => commands::split::run(hunk, file, no_verify),
        Commands::Absorb { dry_run, all } => commands::absorb::run(dry_run, all),
        Commands::Copy { pr } => {
            let target = if pr {
                commands::copy::CopyTarget::Pr
            } else {
                commands::copy::CopyTarget::Branch
            };
            commands::copy::run(target)
        }
        Commands::Detach { branch, yes } => commands::detach::run(branch, yes),
        Commands::Fold { keep, yes } => commands::branch::fold::run(keep, yes),
        Commands::Reorder { yes } => commands::reorder::run(yes),
        Commands::Edit { yes, no_verify } => commands::edit::run(yes, no_verify),
        Commands::Validate => commands::stack_cmd::run_validate(),
        Commands::Fix { dry_run, yes } => commands::stack_cmd::run_fix(dry_run, yes),
        Commands::Freeze { branch } => commands::freeze::run(branch, true),
        Commands::Unfreeze { branch } => commands::freeze::run(branch, false),
        Commands::Run {
            cmd,
            all,
            stack,
            fail_fast,
            parallel,
            jobs,
        }
        | Commands::Test {
            cmd,
            all,
            stack,
            fail_fast,
            parallel,
            jobs,
        } => commands::stack_cmd::run_test(cmd, all, stack, fail_fast, parallel, jobs),
        Commands::Demo => unreachable!(),        // Handled above
        Commands::UpdateCheck => unreachable!(), // Handled before setup/config work
        Commands::Standup {
            json,
            all,
            hours,
            ci,
            ai,
            style,
            jit,
            agent,
            model,
            plain_text,
        } => commands::standup::run(
            json,
            all,
            hours,
            ci,
            ai,
            jit,
            agent,
            model,
            plain_text,
            style.map(Into::into).unwrap_or_default(),
        ),
        Commands::Generate {
            pr_body,
            pr_title,
            commit_msg,
            edit,
            no_prompt,
            agent,
            model,
            template,
            no_template,
        } => commands::generate::run(
            pr_body,
            pr_title,
            commit_msg,
            edit,
            no_prompt,
            agent,
            model,
            template,
            no_template,
        ),
        Commands::Changelog {
            from,
            to,
            find,
            tag_prefix,
            path,
            json,
        } => commands::changelog::run(from, to, find, tag_prefix, path, json),
        Commands::Rename {
            name,
            edit,
            push,
            literal,
        } => commands::branch::rename::run(name, edit, push, literal),
        Commands::Undo {
            op_id,
            yes,
            no_push,
            quiet,
        } => commands::undo::run(op_id, yes, no_push, quiet),
        Commands::Redo {
            op_id,
            yes,
            no_push,
            quiet,
        } => commands::redo::run(op_id, yes, no_push, quiet),
        Commands::Branch(cmd) => match cmd {
            BranchCommands::Create {
                name,
                all,
                message,
                ai,
                yes,
                from,
                prefix,
                insert,
                below,
                no_verify,
            } => commands::branch::create::run(
                name, message, from, prefix, all, insert, below, no_verify, ai, yes,
            ),
            BranchCommands::Checkout {
                branch,
                pr,
                trunk,
                parent,
                child,
                shell_output,
            } => commands::checkout::run(branch, pr, trunk, parent, child, shell_output),
            BranchCommands::Track { parent, all_prs } => {
                commands::branch::track::run(parent, all_prs)
            }
            BranchCommands::Untrack { branch } => commands::branch::untrack::run(branch),
            BranchCommands::Reparent {
                branch,
                parent,
                restack,
            } => commands::branch::reparent::run(branch, parent, restack),
            BranchCommands::Rename {
                name,
                edit,
                push,
                literal,
            } => commands::branch::rename::run(name, edit, push, literal),
            BranchCommands::Delete { branch, force } => {
                commands::branch::delete::run(branch, force)
            }
            BranchCommands::Squash { message, yes } => commands::branch::squash::run(message, yes),
            BranchCommands::Fold { keep, yes } => commands::branch::fold::run(keep, yes),
            BranchCommands::Up { count } => commands::navigate::up(count),
            BranchCommands::Down { count } => commands::navigate::down(count),
            BranchCommands::Top => commands::navigate::top(),
            BranchCommands::Bottom => commands::navigate::bottom(),
            BranchCommands::Submit { submit } => {
                run_submit(submit, commands::submit::SubmitScope::Branch)
            }
        },
        Commands::Upstack(cmd) => match cmd {
            UpstackCommands::Restack { auto_stash_pop } => {
                commands::upstack::restack::run(auto_stash_pop)
            }
            UpstackCommands::Onto {
                target,
                restack: _,
                auto_stash_pop,
            } => commands::upstack::onto::run(target, auto_stash_pop),
            UpstackCommands::Submit { submit } => {
                run_submit(submit, commands::submit::SubmitScope::Upstack)
            }
        },
        Commands::Move {
            target,
            restack: _,
            auto_stash_pop,
        } => commands::upstack::onto::run(target, auto_stash_pop),
        Commands::Downstack(cmd) => match cmd {
            DownstackCommands::Get => {
                commands::status::run(false, None, false, false, false, false)
            }
            DownstackCommands::Submit { submit } => {
                run_submit(submit, commands::submit::SubmitScope::Downstack)
            }
        },
        Commands::Stack(cmd) => match cmd {
            StackCommands::Submit { submit } => {
                run_submit(submit, commands::submit::SubmitScope::Stack)
            }
            StackCommands::Restack {
                all,
                stop_here,
                r#continue,
                dry_run,
                yes,
                quiet,
                auto_stash_pop,
                submit_after,
            } => commands::restack::run(
                all,
                stop_here,
                r#continue,
                dry_run,
                yes,
                quiet,
                auto_stash_pop,
                submit_after.into(),
            ),
            StackCommands::Link => commands::stack_cmd::run_link(),
            StackCommands::Unlink { stack_number } => commands::stack_cmd::run_unlink(stack_number),
        },
        // Hidden shortcuts
        Commands::Bc {
            name,
            all,
            message,
            ai,
            yes,
            from,
            prefix,
            insert,
            below,
            no_verify,
        } => commands::branch::create::run(
            name, message, from, prefix, all, insert, below, no_verify, ai, yes,
        ),
        Commands::Bu { count } => commands::navigate::up(count),
        Commands::Bd { count } => commands::navigate::down(count),
        Commands::Bs { submit } => run_submit(submit, commands::submit::SubmitScope::Branch),
        Commands::Sr {
            all,
            stop_here,
            r#continue,
            dry_run,
            yes,
            quiet,
            auto_stash_pop,
            submit_after,
        } => commands::restack::run(
            all,
            stop_here,
            r#continue,
            dry_run,
            yes,
            quiet,
            auto_stash_pop,
            submit_after.into(),
        ),
        Commands::Worktree { command } => match command {
            None => {
                let interactive_terminal =
                    check_interactive_terminal(stdin_is_terminal, stdout_is_terminal);
                if interactive_terminal.available {
                    commands::init::ensure_initialized()?;
                    tui::worktree::run()
                } else {
                    print_interactive_fallback(
                        interactive_terminal.reason.as_deref(),
                        "worktree dashboard",
                        "showing worktree help",
                    );
                    print_worktree_help()
                }
            }
            Some(WorktreeCommands::Create {
                name,
                from,
                pick,
                worktree_name,
                no_verify,
                shell_output,
                launch,
            }) => commands::worktree::create::run(
                name,
                from,
                pick,
                worktree_name,
                no_verify,
                shell_output,
                launch.agent,
                launch.model,
                launch.run,
                launch.tmux,
                launch.tmux_session,
                launch.args,
                launch.yolo,
                launch.agent_arg,
            ),
            Some(WorktreeCommands::List { json }) => commands::worktree::list::run(json),
            Some(WorktreeCommands::LongList { json }) => commands::worktree::ll::run(json),
            Some(WorktreeCommands::Go {
                name,
                no_verify,
                shell_output,
                launch,
            }) => commands::worktree::go::run_go(
                name,
                no_verify,
                shell_output,
                launch.agent,
                launch.model,
                launch.run,
                launch.tmux,
                launch.tmux_session,
                launch.args,
                launch.yolo,
                launch.agent_arg,
            ),
            Some(WorktreeCommands::Path { name }) => commands::worktree::go::run_path(&name),
            Some(WorktreeCommands::Remove {
                name,
                force,
                delete_branch,
            }) => commands::worktree::remove::run(name, force, delete_branch),
            Some(WorktreeCommands::Promote { shell_output }) => {
                commands::worktree::promote::run(shell_output)
            }
            Some(WorktreeCommands::Prune) => commands::worktree::prune::run(),
            Some(WorktreeCommands::Cleanup {
                force,
                dry_run,
                yes,
            }) => commands::worktree::cleanup::run(force, yes, dry_run),
            Some(WorktreeCommands::Restack) => commands::worktree::restack::run(),
        },
        Commands::Setup { .. } => {
            unreachable!("setup returns before repo initialization")
        }
        Commands::Lane {
            name,
            prompt,
            no_verify,
            shell_output,
            ai,
        } => commands::worktree::ai::run(
            name,
            prompt,
            no_verify,
            shell_output,
            ai.agent,
            ai.model,
            ai.no_tmux,
            ai.tmux_session,
            ai.yolo,
            ai.agent_arg,
        ),
        // Hidden worktree shortcuts
        Commands::W => commands::worktree::list::run(false),
        Commands::Wtc {
            name,
            from,
            pick,
            worktree_name,
            no_verify,
            shell_output,
            launch,
        } => commands::worktree::create::run(
            name,
            from,
            pick,
            worktree_name,
            no_verify,
            shell_output,
            launch.agent,
            launch.model,
            launch.run,
            launch.tmux,
            launch.tmux_session,
            launch.args,
            launch.yolo,
            launch.agent_arg,
        ),
        Commands::Wtls => commands::worktree::list::run(false),
        Commands::Wtll { json } => commands::worktree::ll::run(json),
        Commands::Wtgo {
            name,
            no_verify,
            shell_output,
            launch,
        } => commands::worktree::go::run_go(
            name,
            no_verify,
            shell_output,
            launch.agent,
            launch.model,
            launch.run,
            launch.tmux,
            launch.tmux_session,
            launch.args,
            launch.yolo,
            launch.agent_arg,
        ),
        Commands::Wtrm {
            name,
            force,
            delete_branch,
        } => commands::worktree::remove::run(name, force, delete_branch),
        Commands::Wtprune => commands::worktree::prune::run(),
        Commands::Wtcleanup {
            force,
            dry_run,
            yes,
        } => commands::worktree::cleanup::run(force, yes, dry_run),
        Commands::Wtrs => commands::worktree::restack::run(),
    };

    // Show update notification from cache (instant — no network request here)
    update::show_update_notification();

    result
}

fn print_worktree_help() -> Result<()> {
    let mut command = Cli::command();
    if let Some(worktree) = command.find_subcommand_mut("worktree") {
        worktree.print_help()?;
        println!();
        return Ok(());
    }

    anyhow::bail!("Failed to load worktree help");
}
