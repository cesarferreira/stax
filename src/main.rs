mod commands;
mod config;
mod engine;
mod git;
mod github;
mod remote;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;

#[derive(Parser)]
#[command(name = "stax")]
#[command(about = "Fast stacked Git branches and PRs", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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

    /// Branch management commands
    #[command(subcommand, visible_alias = "b")]
    Branch(BranchCommands),

    /// Upstack commands (operate on descendants)
    #[command(subcommand, visible_alias = "us")]
    Upstack(UpstackCommands),

    /// Downstack commands (operate on ancestors)
    #[command(subcommand, visible_alias = "ds")]
    Downstack(DownstackCommands),

    // Hidden top-level shortcuts for convenience
    #[command(hide = true)]
    Bc {
        name: Option<String>,
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
        /// Child index (1-based)
        index: Option<usize>,
    },
    #[command(hide = true)]
    Bd,
}

#[derive(Subcommand)]
enum BranchCommands {
    /// Create a new branch stacked on current
    #[command(visible_alias = "c")]
    Create {
        /// Name for the new branch
        name: Option<String>,
        /// Message/description to use as branch name (spaces replaced)
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
    },

    /// Fold current branch into its parent
    #[command(visible_alias = "f")]
    Fold {
        /// Keep the branch after folding (don't delete)
        #[arg(short, long)]
        keep: bool,
    },

    /// Move up the stack (to child branch)
    #[command(visible_alias = "u")]
    Up {
        /// Child index (1-based)
        index: Option<usize>,
    },

    /// Move down the stack (to parent branch)
    Down,
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

    // Commands that don't need repo initialization
    match &cli.command {
        Commands::Auth { token } => return commands::auth::run(token.clone()),
        Commands::Config => return commands::config::run(),
        Commands::Doctor => return commands::doctor::run(),
        _ => {}
    }

    // Ensure repo is initialized for all other commands
    commands::init::ensure_initialized()?;

    match cli.command {
        Commands::Status {
            json,
            stack,
            all,
            compact,
            quiet,
        } => commands::status::run(json, stack, all, compact, quiet),
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
        Commands::Auth { .. } => unreachable!(), // Handled above
        Commands::Config => unreachable!(),      // Handled above
        Commands::Diff { stack, all } => commands::diff::run(stack, all),
        Commands::RangeDiff { stack, all } => commands::range_diff::run(stack, all),
        Commands::Doctor => unreachable!(), // Handled above
        Commands::Trunk => commands::checkout::run(None, true, false, None),
        Commands::Branch(cmd) => match cmd {
            BranchCommands::Create {
                name,
                message,
                from,
                prefix,
            } => commands::branch::create::run(name, message, from, prefix),
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
            BranchCommands::Delete { branch, force } => {
                commands::branch::delete::run(branch, force)
            }
            BranchCommands::Squash { message } => commands::branch::squash::run(message),
            BranchCommands::Fold { keep } => commands::branch::fold::run(keep),
            BranchCommands::Up { index } => commands::navigate::up(index),
            BranchCommands::Down => commands::navigate::down(),
        },
        Commands::Upstack(cmd) => match cmd {
            UpstackCommands::Restack => commands::upstack::restack::run(),
        },
        Commands::Downstack(cmd) => match cmd {
            DownstackCommands::Get => commands::status::run(false, None, false, false, false),
        },
        // Hidden shortcuts
        Commands::Bc {
            name,
            message,
            from,
            prefix,
        } => commands::branch::create::run(name, message, from, prefix),
        Commands::Bu { index } => commands::navigate::up(index),
        Commands::Bd => commands::navigate::down(),
    }
}
