mod cache;
mod ci;
mod commands;
mod config;
mod engine;
mod git;
mod github;
mod ops;
mod remote;
mod tui;
mod update;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use config::Config;

#[derive(Parser)]
#[command(name = "stax")]
#[command(version)]
#[command(about = "Fast stacked Git branches and PRs", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Args, Clone)]
struct SubmitOptions {
    /// Create PRs as drafts
    #[arg(short, long)]
    draft: bool,
    /// Only push, don't create/update PRs
    #[arg(long)]
    no_pr: bool,
    /// Skip restack check and submit anyway
    #[arg(short, long)]
    force: bool,
    /// Auto-approve prompts
    #[arg(long)]
    yes: bool,
    /// Disable interactive prompts (use defaults)
    #[arg(long)]
    no_prompt: bool,
    /// Assign reviewers (comma-separated or repeat)
    #[arg(long, value_delimiter = ',')]
    reviewers: Vec<String>,
    /// Add labels (comma-separated or repeat)
    #[arg(long, value_delimiter = ',')]
    labels: Vec<String>,
    /// Assign users (comma-separated or repeat)
    #[arg(long, value_delimiter = ',')]
    assignees: Vec<String>,
    /// Suppress extra output
    #[arg(long)]
    quiet: bool,
    /// Show detailed output
    #[arg(short, long)]
    verbose: bool,
    /// Specify template by name (skip picker)
    #[arg(long)]
    template: Option<String>,
    /// Skip template selection (no template)
    #[arg(long)]
    no_template: bool,
    /// Always open editor for PR body
    #[arg(long)]
    edit: bool,
    /// Generate PR body using AI (claude or codex)
    #[arg(long)]
    ai_body: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Show all stacks (simple tree view)
    #[command(visible_aliases = ["s", "ls"])]
    Status {
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
        /// Show only the stack for this branch
        #[arg(long)]
        stack: Option<String>,
        /// Show only the current stack
        #[arg(short, long)]
        current: bool,
        /// Compact output for scripts
        #[arg(long)]
        compact: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
    },

    /// Show all stacks with PR URLs and full details
    #[command(name = "ll")]
    Ll {
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
        /// Show only the stack for this branch
        #[arg(long)]
        stack: Option<String>,
        /// Show only the current stack
        #[arg(short, long)]
        current: bool,
        /// Compact output for scripts
        #[arg(long)]
        compact: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
    },

    /// Show all stacks with commits and PR info
    #[command(visible_alias = "l")]
    Log {
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
        /// Show only the stack for this branch
        #[arg(long)]
        stack: Option<String>,
        /// Show only the current stack
        #[arg(short, long)]
        current: bool,
        /// Compact output for scripts
        #[arg(long)]
        compact: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
    },

    /// Submit stack - push branches and create/update PRs
    #[command(visible_alias = "ss")]
    Submit {
        #[command(flatten)]
        submit: SubmitOptions,
    },

    /// Merge PRs from bottom of stack up to current branch
    Merge {
        /// Merge entire stack (ignore current position)
        #[arg(long)]
        all: bool,
        /// Show merge plan without merging
        #[arg(long)]
        dry_run: bool,
        /// Merge method: squash, merge, rebase
        #[arg(long, default_value = "squash")]
        method: String,
        /// Keep branches after merge (don't delete)
        #[arg(long)]
        no_delete: bool,
        /// Fail if CI pending (don't poll/wait)
        #[arg(long)]
        no_wait: bool,
        /// Max wait time for CI per PR in minutes
        #[arg(long, default_value = "30")]
        timeout: u64,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
        /// Minimal output
        #[arg(short, long)]
        quiet: bool,
    },

    /// Sync repo - pull trunk, delete merged branches
    #[command(visible_alias = "rs")]
    Sync {
        /// Also restack branches after syncing
        #[arg(short, long)]
        restack: bool,
        /// Don't delete merged branches
        #[arg(long)]
        no_delete: bool,
        /// Force sync without prompts
        #[arg(short, long)]
        force: bool,
        /// Avoid hard reset when updating trunk
        #[arg(long)]
        safe: bool,
        /// Continue after resolving restack conflicts
        #[arg(long)]
        r#continue: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
        /// Show detailed output including git errors
        #[arg(short, long)]
        verbose: bool,
        /// Auto-stash and auto-pop dirty target worktrees during restack operations
        #[arg(long)]
        auto_stash_pop: bool,
    },

    /// Restack (rebase) the current branch onto its parent
    Restack {
        /// Restack all branches in the stack
        #[arg(short, long)]
        all: bool,
        /// Continue after resolving conflicts
        #[arg(long)]
        r#continue: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
        /// Auto-stash and auto-pop dirty target worktrees during restack operations
        #[arg(long)]
        auto_stash_pop: bool,
    },

    /// Restack from the bottom and submit updates
    Cascade {
        /// Push branches to remote but skip PR creation/updates
        #[arg(long)]
        no_pr: bool,
        /// Skip all remote interaction (restack locally only)
        #[arg(long)]
        no_submit: bool,
        /// Auto-stash and auto-pop dirty target worktrees during restack
        #[arg(long)]
        auto_stash_pop: bool,
    },

    /// Checkout a branch in the stack
    #[command(visible_aliases = ["co", "bco"])]
    Checkout {
        /// Branch name (interactive if not provided)
        branch: Option<String>,
        /// Jump directly to trunk
        #[arg(long)]
        trunk: bool,
        /// Jump to parent of current branch
        #[arg(long)]
        parent: bool,
        /// Jump to child branch by index (1-based)
        #[arg(long)]
        child: Option<usize>,
    },

    /// Continue after resolving conflicts
    #[command(visible_alias = "cont")]
    Continue,

    /// Stage all changes and amend them to the current commit
    #[command(visible_alias = "m")]
    Modify {
        /// New commit message (keeps existing if not provided)
        #[arg(short, long)]
        message: Option<String>,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
    },

    /// Authenticate with GitHub
    Auth {
        /// GitHub personal access token
        #[arg(short, long)]
        token: Option<String>,
    },

    /// Show config file path and contents
    Config,

    /// Show diffs for each branch vs parent plus an aggregate stack diff
    Diff {
        /// Show only the stack for this branch
        #[arg(long)]
        stack: Option<String>,
        /// Show all stacks
        #[arg(long)]
        all: bool,
    },

    /// Show range-diff for branches that need restack
    RangeDiff {
        /// Show only the stack for this branch
        #[arg(long)]
        stack: Option<String>,
        /// Show all stacks
        #[arg(long)]
        all: bool,
    },

    /// Check stax configuration and repo health
    Doctor,

    /// Switch to the trunk branch
    #[command(visible_alias = "t")]
    Trunk,

    /// Move up the stack (to child branch)
    #[command(visible_alias = "u")]
    Up {
        /// Number of branches to move up (default: 1)
        count: Option<usize>,
    },

    /// Move down the stack (to parent branch)
    #[command(visible_alias = "d")]
    Down {
        /// Number of branches to move down (default: 1)
        count: Option<usize>,
    },

    /// Move to the top of the stack (tip/leaf branch)
    Top,

    /// Move to the bottom of the stack (first branch above trunk)
    Bottom,

    /// Switch to the previous branch (like git checkout -)
    #[command(visible_alias = "p")]
    Prev,

    /// Branch management commands
    #[command(subcommand, visible_alias = "b")]
    Branch(BranchCommands),

    /// Upstack commands (operate on descendants)
    #[command(subcommand, visible_alias = "us")]
    Upstack(UpstackCommands),

    /// Downstack commands (operate on ancestors)
    #[command(subcommand, visible_alias = "ds")]
    Downstack(DownstackCommands),

    /// Create a new branch stacked on current
    #[command(visible_alias = "c")]
    Create {
        /// Name for the new branch
        name: Option<String>,
        /// Stage all changes (like git commit --all)
        #[arg(short, long)]
        all: bool,
        /// Commit message (also used as branch name if no name provided)
        #[arg(short, long)]
        message: Option<String>,
        /// Base branch to create from (defaults to current)
        #[arg(long)]
        from: Option<String>,
        /// Override branch prefix (e.g. "feature/")
        #[arg(long)]
        prefix: Option<String>,
    },

    /// Open the PR for the current branch in browser
    Pr,

    /// Open the repository in browser
    Open,

    /// Show comments on the current branch's PR
    Comments {
        /// Output raw markdown without rendering
        #[arg(long)]
        plain: bool,
    },

    /// Show CI status for all branches in the stack
    Ci {
        /// Show all stacks
        #[arg(long)]
        all: bool,
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
        /// Force refresh (bypass cache)
        #[arg(long)]
        refresh: bool,
        /// Watch CI until completion (polls periodically)
        #[arg(long, short)]
        watch: bool,
        /// Polling interval in seconds (default: 15)
        #[arg(long, default_value = "15")]
        interval: u64,
    },

    /// Split the current branch into multiple stacked branches (interactive)
    Split,

    /// Copy branch name or PR URL to clipboard
    Copy {
        /// Copy PR URL instead of branch name
        #[arg(long)]
        pr: bool,
    },

    /// Generate standup summary of recent activity
    Standup {
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
        /// Show all stacks (not just current)
        #[arg(long)]
        all: bool,
        /// Time window in hours (default: 24)
        #[arg(long, default_value = "24")]
        hours: i64,
    },

    /// Generate content using AI
    Generate {
        /// Generate PR body from diff and update the PR
        #[arg(long)]
        pr_body: bool,
        /// Open editor to review before updating
        #[arg(long)]
        edit: bool,
        /// AI agent to use (claude, codex). Defaults to config or auto-detect
        #[arg(long)]
        agent: Option<String>,
        /// Model to use with the AI agent. Defaults to config or agent's default
        #[arg(long)]
        model: Option<String>,
    },

    /// Generate changelog between two refs
    Changelog {
        /// Starting ref (tag, branch, or commit)
        from: String,
        /// Ending ref (defaults to HEAD)
        #[arg(default_value = "HEAD")]
        to: String,
        /// Filter commits to those touching this path
        #[arg(long)]
        path: Option<String>,
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
    },

    /// Rename the current branch
    Rename {
        /// New branch name (interactive if not provided)
        name: Option<String>,
        /// Edit the commit message
        #[arg(short, long)]
        edit: bool,
        /// Push new branch and delete old remote (non-interactive)
        #[arg(short, long)]
        push: bool,
        /// Use name literally without applying prefix
        #[arg(long, hide = true)]
        literal: bool,
    },

    /// Undo the last stax operation (or a specific one)
    Undo {
        /// Operation ID to undo (defaults to last)
        op_id: Option<String>,
        /// Auto-approve prompts
        #[arg(long)]
        yes: bool,
        /// Don't restore remote refs (local only)
        #[arg(long)]
        no_push: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
    },

    /// Redo the last undone stax operation
    Redo {
        /// Operation ID to redo (defaults to last)
        op_id: Option<String>,
        /// Auto-approve prompts
        #[arg(long)]
        yes: bool,
        /// Don't restore remote refs (local only)
        #[arg(long)]
        no_push: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
    },

    // Hidden top-level shortcuts for convenience
    #[command(hide = true)]
    Bc {
        name: Option<String>,
        /// Stage all changes (like git commit --all)
        #[arg(short, long)]
        all: bool,
        #[arg(short, long)]
        message: Option<String>,
        /// Base branch to create from (defaults to current)
        #[arg(long)]
        from: Option<String>,
        /// Override branch prefix (e.g. "feature/")
        #[arg(long)]
        prefix: Option<String>,
    },
    #[command(hide = true)]
    Bu {
        /// Number of branches to move up
        count: Option<usize>,
    },
    #[command(hide = true)]
    Bd {
        /// Number of branches to move down
        count: Option<usize>,
    },
    #[command(hide = true)]
    Bs {
        #[command(flatten)]
        submit: SubmitOptions,
    },
}

#[derive(Subcommand)]
enum BranchCommands {
    /// Create a new branch stacked on current
    #[command(visible_alias = "c")]
    Create {
        /// Name for the new branch
        name: Option<String>,
        /// Stage all changes (like git commit --all)
        #[arg(short, long)]
        all: bool,
        /// Commit message (also used as branch name if no name provided)
        #[arg(short, long)]
        message: Option<String>,
        /// Base branch to create from (defaults to current)
        #[arg(long)]
        from: Option<String>,
        /// Override branch prefix (e.g. "feature/")
        #[arg(long)]
        prefix: Option<String>,
    },

    /// Checkout a branch in the stack
    #[command(visible_alias = "co")]
    Checkout {
        /// Branch name (interactive if not provided)
        branch: Option<String>,
        /// Jump directly to trunk
        #[arg(long)]
        trunk: bool,
        /// Jump to parent of current branch
        #[arg(long)]
        parent: bool,
        /// Jump to child branch by index (1-based)
        #[arg(long)]
        child: Option<usize>,
    },

    /// Track an existing branch (set its parent)
    Track {
        /// Parent branch name
        #[arg(short, long, conflicts_with = "all_prs")]
        parent: Option<String>,
        /// Track all open PRs authored by you
        #[arg(long)]
        all_prs: bool,
    },

    /// Stop tracking a branch (remove stax metadata only)
    #[command(visible_alias = "ut")]
    Untrack {
        /// Branch to untrack (defaults to current branch)
        branch: Option<String>,
    },

    /// Change the parent of a tracked branch
    Reparent {
        /// Branch to reparent (defaults to current)
        #[arg(short, long)]
        branch: Option<String>,
        /// New parent branch name
        #[arg(short, long)]
        parent: Option<String>,
    },

    /// Rename the current branch
    #[command(visible_alias = "r")]
    Rename {
        /// New branch name (interactive if not provided)
        name: Option<String>,
        /// Edit the commit message
        #[arg(short, long)]
        edit: bool,
        /// Push new branch and delete old remote (non-interactive)
        #[arg(short, long)]
        push: bool,
        /// Use name literally without applying prefix
        #[arg(long, hide = true)]
        literal: bool,
    },

    /// Delete a branch and its metadata
    #[command(visible_alias = "d")]
    Delete {
        /// Branch to delete
        branch: Option<String>,
        /// Force delete even if not merged
        #[arg(short, long)]
        force: bool,
    },

    /// Squash all commits on current branch into one
    #[command(visible_alias = "sq")]
    Squash {
        /// Commit message for the squashed commit
        #[arg(short, long)]
        message: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Fold current branch into its parent
    #[command(visible_alias = "f")]
    Fold {
        /// Keep the branch after folding (don't delete)
        #[arg(short, long)]
        keep: bool,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Move up the stack (to child branch)
    #[command(visible_alias = "u")]
    Up {
        /// Number of branches to move up (default: 1)
        count: Option<usize>,
    },

    /// Move down the stack (to parent branch)
    Down {
        /// Number of branches to move down (default: 1)
        count: Option<usize>,
    },

    /// Move to the top of the stack (tip/leaf branch)
    Top,

    /// Move to the bottom of the stack (first branch above trunk)
    Bottom,

    /// Submit the current branch only
    Submit {
        #[command(flatten)]
        submit: SubmitOptions,
    },
}

#[derive(Subcommand)]
enum UpstackCommands {
    /// Restack all branches above current
    Restack {
        /// Auto-stash and auto-pop dirty target worktrees during restack operations
        #[arg(long)]
        auto_stash_pop: bool,
    },

    /// Submit current branch and descendants
    Submit {
        #[command(flatten)]
        submit: SubmitOptions,
    },
}

#[derive(Subcommand)]
enum DownstackCommands {
    /// Show branches below current
    Get,

    /// Submit ancestors and current branch
    Submit {
        #[command(flatten)]
        submit: SubmitOptions,
    },
}

fn run_submit(submit: SubmitOptions, scope: commands::submit::SubmitScope) -> Result<()> {
    commands::submit::run(
        scope,
        submit.draft,
        submit.no_pr,
        submit.force,
        submit.yes,
        submit.no_prompt,
        submit.reviewers,
        submit.labels,
        submit.assignees,
        submit.quiet,
        submit.verbose,
        submit.template,
        submit.no_template,
        submit.edit,
        submit.ai_body,
    )
}

fn main() -> Result<()> {
    // Ensure config exists (creates default on first run)
    let _ = Config::ensure_exists();

    let cli = Cli::parse();

    // No command = launch TUI
    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            // TUI requires initialized repo
            commands::init::ensure_initialized()?;
            let result = tui::run();
            update::show_update_notification();
            update::check_in_background();
            return result;
        }
    };

    // Commands that don't need repo initialization
    match &command {
        Commands::Auth { token } => {
            let result = commands::auth::run(token.clone());
            update::show_update_notification();
            update::check_in_background();
            return result;
        }
        Commands::Config => {
            let result = commands::config::run();
            update::show_update_notification();
            update::check_in_background();
            return result;
        }
        Commands::Doctor => {
            let result = commands::doctor::run();
            update::show_update_notification();
            update::check_in_background();
            return result;
        }
        _ => {}
    }

    // Ensure repo is initialized for all other commands
    commands::init::ensure_initialized()?;

    let result = match command {
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
            dry_run,
            method,
            no_delete,
            no_wait,
            timeout,
            yes,
            quiet,
        } => {
            let merge_method = method.parse().unwrap_or_default();
            commands::merge::run(
                all,
                dry_run,
                merge_method,
                no_delete,
                no_wait,
                timeout,
                yes,
                quiet,
            )
        }
        Commands::Sync {
            restack,
            no_delete,
            force,
            safe,
            r#continue,
            quiet,
            verbose,
            auto_stash_pop,
        } => commands::sync::run(
            restack,
            !no_delete,
            force,
            safe,
            r#continue,
            quiet,
            verbose,
            auto_stash_pop,
        ),
        Commands::Restack {
            all,
            r#continue,
            quiet,
            auto_stash_pop,
        } => commands::restack::run(all, r#continue, quiet, auto_stash_pop),
        Commands::Cascade { no_pr, no_submit, auto_stash_pop } => commands::cascade::run(no_pr, no_submit, auto_stash_pop),
        Commands::Checkout {
            branch,
            trunk,
            parent,
            child,
        } => commands::checkout::run(branch, trunk, parent, child),
        Commands::Continue => commands::continue_cmd::run(),
        Commands::Modify { message, quiet } => commands::modify::run(message, quiet),
        Commands::Auth { .. } => unreachable!(), // Handled above
        Commands::Config => unreachable!(),      // Handled above
        Commands::Diff { stack, all } => commands::diff::run(stack, all),
        Commands::RangeDiff { stack, all } => commands::range_diff::run(stack, all),
        Commands::Doctor => unreachable!(), // Handled above
        Commands::Trunk => commands::checkout::run(None, true, false, None),
        Commands::Up { count } => commands::navigate::up(count),
        Commands::Down { count } => commands::navigate::down(count),
        Commands::Top => commands::navigate::top(),
        Commands::Bottom => commands::navigate::bottom(),
        Commands::Prev => commands::navigate::prev(),
        Commands::Create {
            name,
            all,
            message,
            from,
            prefix,
        } => commands::branch::create::run(name, message, from, prefix, all),
        Commands::Pr => commands::pr::run(),
        Commands::Open => commands::open::run(),
        Commands::Comments { plain } => commands::comments::run(plain),
        Commands::Ci {
            all,
            json,
            refresh,
            watch,
            interval,
        } => commands::ci::run(all, json, refresh, watch, interval),
        Commands::Split => commands::split::run(),
        Commands::Copy { pr } => {
            let target = if pr {
                commands::copy::CopyTarget::Pr
            } else {
                commands::copy::CopyTarget::Branch
            };
            commands::copy::run(target)
        }
        Commands::Standup { json, all, hours } => commands::standup::run(json, all, hours),
        Commands::Generate {
            pr_body,
            edit,
            agent,
            model,
        } => {
            if !pr_body {
                anyhow::bail!("Please specify what to generate. Usage: stax generate --pr-body");
            }
            commands::generate::run(edit, agent, model)
        }
        Commands::Changelog {
            from,
            to,
            path,
            json,
        } => commands::changelog::run(from, to, path, json),
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
                from,
                prefix,
            } => commands::branch::create::run(name, message, from, prefix, all),
            BranchCommands::Checkout {
                branch,
                trunk,
                parent,
                child,
            } => commands::checkout::run(branch, trunk, parent, child),
            BranchCommands::Track { parent, all_prs } => {
                commands::branch::track::run(parent, all_prs)
            }
            BranchCommands::Untrack { branch } => commands::branch::untrack::run(branch),
            BranchCommands::Reparent { branch, parent } => {
                commands::branch::reparent::run(branch, parent)
            }
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
            UpstackCommands::Submit { submit } => {
                run_submit(submit, commands::submit::SubmitScope::Upstack)
            }
        },
        Commands::Downstack(cmd) => match cmd {
            DownstackCommands::Get => {
                commands::status::run(false, None, false, false, false, false)
            }
            DownstackCommands::Submit { submit } => {
                run_submit(submit, commands::submit::SubmitScope::Downstack)
            }
        },
        // Hidden shortcuts
        Commands::Bc {
            name,
            all,
            message,
            from,
            prefix,
        } => commands::branch::create::run(name, message, from, prefix, all),
        Commands::Bu { count } => commands::navigate::up(count),
        Commands::Bd { count } => commands::navigate::down(count),
        Commands::Bs { submit } => run_submit(submit, commands::submit::SubmitScope::Branch),
    };

    // Show update notification (from cache, instant) and spawn background check for next run
    update::show_update_notification();
    update::check_in_background();

    result
}
