mod commands;
mod config;
mod engine;
mod git;
mod github;

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
    Status,

    /// Show detailed stack with commits and PR info
    #[command(visible_alias = "l")]
    Log,

    /// Submit stack - push branches and create/update PRs
    #[command(visible_alias = "ss")]
    Submit {
        /// Create PRs as drafts
        #[arg(short, long)]
        draft: bool,
        /// Only push, don't create/update PRs
        #[arg(long)]
        no_pr: bool,
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
    },

    /// Restack (rebase) the current branch onto its parent
    Restack {
        /// Restack all branches in the stack
        #[arg(short, long)]
        all: bool,
    },

    /// Checkout a branch in the stack
    #[command(visible_aliases = ["co", "bco"])]
    Checkout {
        /// Branch name (interactive if not provided)
        branch: Option<String>,
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

    /// Branch management commands
    #[command(subcommand, visible_alias = "b")]
    Branch(BranchCommands),

    /// Upstack commands (operate on descendants)
    #[command(subcommand, visible_alias = "us")]
    Upstack(UpstackCommands),

    /// Downstack commands (operate on ancestors)
    #[command(subcommand, visible_alias = "ds")]
    Downstack(DownstackCommands),

    /// Move up the stack (to child branch)
    #[command(visible_alias = "bu")]
    Up,

    /// Move down the stack (to parent branch)
    #[command(visible_alias = "bd")]
    Down,

    // Hidden top-level shortcuts for convenience
    #[command(hide = true)]
    Bc {
        name: Option<String>,
        #[arg(short, long)]
        message: Option<String>,
    },
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
    },

    /// Checkout a branch in the stack
    #[command(visible_alias = "co")]
    Checkout {
        /// Branch name (interactive if not provided)
        branch: Option<String>,
    },

    /// Track an existing branch (set its parent)
    Track {
        /// Parent branch name
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
    if let Commands::Auth { token } = cli.command {
        return commands::auth::run(token);
    }

    // Ensure repo is initialized for all other commands
    commands::init::ensure_initialized()?;

    match cli.command {
        Commands::Status => commands::status::run(),
        Commands::Log => commands::log::run(),
        Commands::Submit { draft, no_pr } => commands::submit::run(draft, no_pr),
        Commands::Sync { restack, no_delete, force } => commands::sync::run(restack, !no_delete, force),
        Commands::Restack { all } => commands::restack::run(all),
        Commands::Checkout { branch } => commands::checkout::run(branch),
        Commands::Continue => commands::continue_cmd::run(),
        Commands::Auth { .. } => unreachable!(), // Handled above
        Commands::Branch(cmd) => match cmd {
            BranchCommands::Create { name, message } => commands::branch::create::run(name, message),
            BranchCommands::Checkout { branch } => commands::checkout::run(branch),
            BranchCommands::Track { parent } => commands::branch::track::run(parent),
            BranchCommands::Delete { branch, force } => {
                commands::branch::delete::run(branch, force)
            }
        },
        Commands::Upstack(cmd) => match cmd {
            UpstackCommands::Restack => commands::upstack::restack::run(),
        },
        Commands::Downstack(cmd) => match cmd {
            DownstackCommands::Get => commands::status::run(),
        },
        Commands::Up => commands::navigate::up(),
        Commands::Down => commands::navigate::down(),
        // Hidden shortcuts
        Commands::Bc { name, message } => commands::branch::create::run(name, message),
    }
}
