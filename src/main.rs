mod commands;
mod config;
mod engine;
mod git;
mod github;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gt")]
#[command(about = "Fast stacked Git branches and PRs", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    // ===== Top-level shortcuts (freephite style) =====

    /// Submit stack - push and create/update PRs [alias: stack submit]
    #[command(name = "ss")]
    Ss {
        #[arg(short, long)]
        draft: bool,
        #[arg(long)]
        no_pr: bool,
    },

    /// Restack current branch onto parent [alias: stack restack]
    #[command(name = "rs")]
    Rs {
        #[arg(short, long)]
        all: bool,
    },

    /// Branch checkout - switch branches [alias: branch checkout]
    #[command(name = "bco")]
    Bco {
        branch: Option<String>,
    },

    /// Branch create - create stacked branch [alias: branch create]
    #[command(name = "bc")]
    Bc {
        name: String,
    },

    /// Branch delete [alias: branch delete]
    #[command(name = "bd")]
    Bd {
        branch: Option<String>,
        #[arg(short, long)]
        force: bool,
    },

    // ===== Full commands =====

    /// Show the current stack
    #[command(visible_aliases = ["s", "log", "l"])]
    Status,

    /// Restack (rebase) the current branch onto its parent
    Restack {
        #[arg(short, long)]
        all: bool,
    },

    /// Submit stack - push branches and create/update PRs
    Submit {
        #[arg(short, long)]
        draft: bool,
        #[arg(long)]
        no_pr: bool,
    },

    /// Checkout a branch in the stack
    #[command(visible_alias = "co")]
    Checkout {
        branch: Option<String>,
    },

    /// Continue after resolving conflicts
    #[command(visible_alias = "cont")]
    Continue,

    /// Authenticate with GitHub
    Auth {
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
}

#[derive(Subcommand)]
enum BranchCommands {
    /// Create a new branch stacked on current
    #[command(visible_alias = "c")]
    Create {
        name: String,
    },

    /// Checkout a branch in the stack
    #[command(visible_alias = "co")]
    Checkout {
        branch: Option<String>,
    },

    /// Track an existing branch (set its parent)
    Track {
        #[arg(short, long)]
        parent: Option<String>,
    },

    /// Delete a branch and its metadata
    #[command(visible_alias = "d")]
    Delete {
        branch: Option<String>,
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
    let cli = Cli::parse();

    match cli.command {
        // Top-level shortcuts
        Commands::Ss { draft, no_pr } => commands::submit::run(draft, no_pr),
        Commands::Rs { all } => commands::restack::run(all),
        Commands::Bco { branch } => commands::checkout::run(branch),
        Commands::Bc { name } => commands::branch::create::run(&name),
        Commands::Bd { branch, force } => commands::branch::delete::run(branch, force),

        // Full commands
        Commands::Status => commands::status::run(),
        Commands::Restack { all } => commands::restack::run(all),
        Commands::Submit { draft, no_pr } => commands::submit::run(draft, no_pr),
        Commands::Checkout { branch } => commands::checkout::run(branch),
        Commands::Continue => commands::continue_cmd::run(),
        Commands::Auth { token } => commands::auth::run(token),
        Commands::Branch(cmd) => match cmd {
            BranchCommands::Create { name } => commands::branch::create::run(&name),
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
            DownstackCommands::Get => commands::status::run(), // For now, just show status
        },
    }
}
