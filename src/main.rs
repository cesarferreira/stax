mod cache;
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
use clap::{Parser, Subcommand};
use config::Config;

#[derive(Parser)]
#[command(name = "stax")]
#[command(version)]
#[command(about = "Fast stacked Git branches and PRs", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show the current stack (simple tree view)
    #[command(visible_aliases = ["s", "ls"])]
    Status {
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
        /// Show only the stack for this branch
        #[arg(long)]
        stack: Option<String>,
        /// Show all stacks
        #[arg(long)]
        all: bool,
        /// Compact output for scripts
        #[arg(long)]
        compact: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
    },

    /// Show stack with PR URLs and full details
    #[command(name = "ll")]
    Ll {
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
        /// Show only the stack for this branch
        #[arg(long)]
        stack: Option<String>,
        /// Show all stacks
        #[arg(long)]
        all: bool,
        /// Compact output for scripts
        #[arg(long)]
        compact: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
    },

    /// Show detailed stack with commits and PR info
    #[command(visible_alias = "l")]
    Log {
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
        /// Show only the stack for this branch
        #[arg(long)]
        stack: Option<String>,
        /// Show all stacks
        #[arg(long)]
        all: bool,
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
        #[arg(short, long)]
        parent: Option<String>,
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
}

#[derive(Subcommand)]
enum UpstackCommands {
    /// Restack all branches above current
    Restack,
}

#[derive(Subcommand)]
enum DownstackCommands {
    /// Show branches below current
    Get,
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
            all,
            compact,
            quiet,
        } => commands::status::run(json, stack, all, compact, quiet, false),
        Commands::Ll {
            json,
            stack,
            all,
            compact,
            quiet,
        } => commands::status::run(json, stack, all, compact, quiet, true),
        Commands::Log {
            json,
            stack,
            all,
            compact,
            quiet,
        } => commands::log::run(json, stack, all, compact, quiet),
        Commands::Submit {
            draft,
            no_pr,
            force,
            yes,
            no_prompt,
            reviewers,
            labels,
            assignees,
            quiet,
        } => commands::submit::run(
            draft,
            no_pr,
            force,
            yes,
            no_prompt,
            reviewers,
            labels,
            assignees,
            quiet,
        ),
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
            commands::merge::run(all, dry_run, merge_method, no_delete, no_wait, timeout, yes, quiet)
        }
        Commands::Sync {
            restack,
            no_delete,
            force,
            safe,
            r#continue,
            quiet,
            verbose,
        } => commands::sync::run(restack, !no_delete, force, safe, r#continue, quiet, verbose),
        Commands::Restack {
            all,
            r#continue,
            quiet,
        } => commands::restack::run(all, r#continue, quiet),
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
        Commands::Ci { all, json, refresh } => commands::ci::run(all, json, refresh),
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
        Commands::Rename { name, edit, push, literal } => commands::branch::rename::run(name, edit, push, literal),
        Commands::Undo { op_id, yes, no_push, quiet } => commands::undo::run(op_id, yes, no_push, quiet),
        Commands::Redo { op_id, yes, no_push, quiet } => commands::redo::run(op_id, yes, no_push, quiet),
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
            BranchCommands::Track { parent } => commands::branch::track::run(parent),
            BranchCommands::Reparent { branch, parent } => {
                commands::branch::reparent::run(branch, parent)
            }
            BranchCommands::Rename { name, edit, push, literal } => commands::branch::rename::run(name, edit, push, literal),
            BranchCommands::Delete { branch, force } => {
                commands::branch::delete::run(branch, force)
            }
            BranchCommands::Squash { message, yes } => commands::branch::squash::run(message, yes),
            BranchCommands::Fold { keep, yes } => commands::branch::fold::run(keep, yes),
            BranchCommands::Up { count } => commands::navigate::up(count),
            BranchCommands::Down { count } => commands::navigate::down(count),
            BranchCommands::Top => commands::navigate::top(),
            BranchCommands::Bottom => commands::navigate::bottom(),
        },
        Commands::Upstack(cmd) => match cmd {
            UpstackCommands::Restack => commands::upstack::restack::run(),
        },
        Commands::Downstack(cmd) => match cmd {
            DownstackCommands::Get => commands::status::run(false, None, false, false, false, false),
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
    };

    // Show update notification (from cache, instant) and spawn background check for next run
    update::show_update_notification();
    update::check_in_background();

    result
}
