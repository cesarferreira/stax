mod cache;
mod ci;
mod commands;
mod config;
mod engine;
mod git;
mod github;
mod ops;
mod progress;
mod remote;
mod tui;
mod update;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
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
    /// Skip git fetch and use cached remote-tracking refs
    #[arg(long = "no-fetch", action = clap::ArgAction::SetTrue)]
    no_fetch: bool,
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
    /// Open the current branch PR in browser after submit
    #[arg(long, conflicts_with = "no_pr")]
    open: bool,
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
    /// Generate PR body using AI (claude, codex, or gemini)
    #[arg(long)]
    ai_body: bool,
    /// Re-request review from existing reviewers when updating PRs
    #[arg(long)]
    rerequest_review: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RestackSubmitAfter {
    Ask,
    Yes,
    No,
}

impl From<RestackSubmitAfter> for commands::restack::SubmitAfterRestack {
    fn from(value: RestackSubmitAfter) -> Self {
        match value {
            RestackSubmitAfter::Ask => commands::restack::SubmitAfterRestack::Ask,
            RestackSubmitAfter::Yes => commands::restack::SubmitAfterRestack::Yes,
            RestackSubmitAfter::No => commands::restack::SubmitAfterRestack::No,
        }
    }
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
        /// Wait for each PR to be ready (CI + approval) before merging
        #[arg(long, conflicts_with_all = ["dry_run", "no_wait"])]
        when_ready: bool,
        /// Polling interval in seconds for --when-ready mode
        #[arg(long, default_value = "15", requires = "when_ready")]
        interval: u64,
        /// Skip post-merge sync (`stax rs`)
        #[arg(long)]
        no_sync: bool,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
        /// Minimal output
        #[arg(short, long)]
        quiet: bool,
    },

    /// Deprecated: use `stax merge --when-ready`
    #[command(name = "merge-when-ready", visible_alias = "mwr", hide = true)]
    MergeWhenReady {
        /// Merge entire stack (include descendants above current)
        #[arg(long)]
        all: bool,
        /// Merge method: squash, merge, rebase
        #[arg(long, default_value = "squash")]
        method: String,
        /// Max wait time per PR in minutes (default: 30)
        #[arg(long, default_value = "30")]
        timeout: u64,
        /// Polling interval in seconds (default: 15)
        #[arg(long, default_value = "15")]
        interval: u64,
        /// Keep branches after merge (don't delete)
        #[arg(long)]
        no_delete: bool,
        /// Skip post-merge sync (`stax rs`)
        #[arg(long)]
        no_sync: bool,
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
        /// Prune stale remote-tracking refs during fetch
        #[arg(long)]
        prune: bool,
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
        /// Preview predicted conflicts without rebasing
        #[arg(long)]
        dry_run: bool,
        /// Skip conflict confirmation prompt
        #[arg(short, long)]
        yes: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
        /// Auto-stash and auto-pop dirty target worktrees during restack operations
        #[arg(long)]
        auto_stash_pop: bool,
        /// After restack, submit stack updates (`ask`, `yes`, `no`)
        #[arg(long, value_enum, default_value_t = RestackSubmitAfter::Ask)]
        submit_after: RestackSubmitAfter,
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

    /// Abort an in-progress rebase/conflict resolution
    Abort,

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
        #[arg(short, long, conflicts_with = "from_gh")]
        token: Option<String>,
        /// Import token from GitHub CLI (`gh auth token`)
        #[arg(long)]
        from_gh: bool,
        #[command(subcommand)]
        command: Option<AuthSubcommand>,
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
        /// Show all tracked branches (not just current stack)
        #[arg(long)]
        all: bool,
        /// Show all branches in the current stack (not just current branch)
        #[arg(long, short)]
        stack: bool,
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
        /// Show compact summary cards instead of the full per-check table
        #[arg(long, short)]
        verbose: bool,
    },

    /// Split the current branch into multiple stacked branches (interactive)
    Split,

    /// Copy branch name or PR URL to clipboard
    Copy {
        /// Copy PR URL instead of branch name
        #[arg(long)]
        pr: bool,
    },

    /// Remove a branch from its stack (reparent children to parent)
    Detach {
        /// Branch to detach (defaults to current)
        branch: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Interactively reorder branches within a stack
    Reorder {
        /// Skip confirmation prompts
        #[arg(long)]
        yes: bool,
    },

    /// Validate stack metadata health
    Validate,

    /// Auto-repair broken metadata
    Fix {
        /// Show what would be fixed without changing anything
        #[arg(long)]
        dry_run: bool,
        /// Auto-approve prompts
        #[arg(long)]
        yes: bool,
    },

    /// Run a command on each branch in the stack
    Test {
        /// Command to run
        #[arg(trailing_var_arg = true, required = true)]
        cmd: Vec<String>,
        /// Run on all tracked branches (not just current stack)
        #[arg(long)]
        all: bool,
        /// Stop after first failure
        #[arg(long)]
        fail_fast: bool,
    },

    /// Interactive tutorial (no auth or repo needed)
    Demo,

    /// Generate standup summary of recent activity
    Standup {
        /// Output raw JSON (standup data, or summary JSON when combined with --summary)
        #[arg(long)]
        json: bool,
        /// Show all stacks (not just current)
        #[arg(long)]
        all: bool,
        /// Time window in hours (default: 24)
        #[arg(long, default_value = "24")]
        hours: i64,
        /// Summarize standup using AI agent
        #[arg(long)]
        summary: bool,
        /// AI agent to use (claude, codex, gemini, opencode). Defaults to config or auto-detect
        #[arg(long)]
        agent: Option<String>,
        /// Output plain text with no colors or spinner (useful for piping)
        #[arg(long)]
        plain_text: bool,
    },

    /// Generate content using AI
    Generate {
        /// Generate PR body from diff and update the PR
        #[arg(long)]
        pr_body: bool,
        /// Open editor to review before updating
        #[arg(long)]
        edit: bool,
        /// AI agent to use (claude, codex, gemini, opencode). Defaults to config or auto-detect
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

    /// Manage parallel AI agent worktrees
    #[command(subcommand, visible_alias = "ag")]
    Agent(AgentCommands),

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

#[derive(Subcommand, Clone)]
enum AuthSubcommand {
    /// Show which auth source is currently active
    Status,
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

#[derive(Subcommand)]
enum AgentCommands {
    /// Create a new agent worktree + stacked branch
    Create {
        /// Human title — slugified into branch name and folder (e.g. "Add dark mode")
        title: String,
        /// Base branch to create from (defaults to current)
        #[arg(long)]
        base: Option<String>,
        /// Stack on this branch (alias for --base)
        #[arg(long)]
        stack_on: Option<String>,
        /// Open the worktree in the default editor after creation
        #[arg(long)]
        open: bool,
        /// Open in Cursor after creation
        #[arg(long)]
        open_cursor: bool,
        /// Open in Codex after creation
        #[arg(long)]
        open_codex: bool,
        /// Skip post-create hook even if configured
        #[arg(long)]
        no_hook: bool,
    },

    /// Open (reattach to) an agent worktree in the editor
    #[command(visible_alias = "attach")]
    Open {
        /// Name or slug of the worktree (interactive picker if omitted)
        name: Option<String>,
    },

    /// List all registered agent worktrees
    #[command(visible_aliases = ["ls"])]
    List,

    /// Register the current directory as a managed agent worktree
    Register,

    /// Remove an agent worktree (and optionally its branch)
    Remove {
        /// Name or slug of the worktree (interactive picker if omitted)
        name: Option<String>,
        /// Force removal even if the worktree has uncommitted changes
        #[arg(short, long)]
        force: bool,
        /// Also delete the branch and its stax metadata
        #[arg(long)]
        delete_branch: bool,
    },

    /// Remove stale registry entries and run git worktree prune
    Prune,

    /// Restack all registered agent worktrees
    Sync,
}

fn run_submit(submit: SubmitOptions, scope: commands::submit::SubmitScope) -> Result<()> {
    commands::submit::run(
        scope,
        submit.draft,
        submit.no_pr,
        submit.no_fetch,
        submit.force,
        submit.yes,
        submit.no_prompt,
        submit.reviewers,
        submit.labels,
        submit.assignees,
        submit.quiet,
        submit.open,
        submit.verbose,
        submit.template,
        submit.no_template,
        submit.edit,
        submit.ai_body,
        submit.rerequest_review,
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
        Commands::Demo => {
            let result = commands::demo::run();
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
            when_ready,
            interval,
            no_sync,
            yes,
            quiet,
        } => {
            let merge_method = method.parse().unwrap_or_default();
            if when_ready {
                commands::merge_when_ready::run(
                    all,
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
            no_delete,
            force,
            safe,
            r#continue,
            quiet,
            verbose,
            auto_stash_pop,
        } => commands::sync::run(
            restack,
            prune,
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
            dry_run,
            yes,
            quiet,
            auto_stash_pop,
            submit_after,
        } => commands::restack::run(
            all,
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
        Commands::Checkout {
            branch,
            trunk,
            parent,
            child,
        } => commands::checkout::run(branch, trunk, parent, child),
        Commands::Continue => commands::continue_cmd::run(),
        Commands::Abort => commands::abort::run(),
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
            stack,
            json,
            refresh,
            watch,
            interval,
            verbose,
        } => commands::ci::run(all, stack, json, refresh, watch, interval, verbose),
        Commands::Split => commands::split::run(),
        Commands::Copy { pr } => {
            let target = if pr {
                commands::copy::CopyTarget::Pr
            } else {
                commands::copy::CopyTarget::Branch
            };
            commands::copy::run(target)
        }
        Commands::Detach { branch, yes } => commands::detach::run(branch, yes),
        Commands::Reorder { yes } => commands::reorder::run(yes),
        Commands::Validate => commands::stack_cmd::run_validate(),
        Commands::Fix { dry_run, yes } => commands::stack_cmd::run_fix(dry_run, yes),
        Commands::Test {
            cmd,
            all,
            fail_fast,
        } => commands::stack_cmd::run_test(cmd, all, fail_fast),
        Commands::Demo => unreachable!(), // Handled above
        Commands::Standup {
            json,
            all,
            hours,
            summary,
            agent,
            plain_text,
        } => commands::standup::run(json, all, hours, summary, agent, plain_text),
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
        Commands::Agent(cmd) => match cmd {
            AgentCommands::Create {
                title,
                base,
                stack_on,
                open,
                open_cursor,
                open_codex,
                no_hook,
            } => commands::agent::create::run(
                title,
                base,
                stack_on,
                open,
                open_cursor,
                open_codex,
                no_hook,
            ),
            AgentCommands::Open { name } => commands::agent::open::run(name),
            AgentCommands::List => commands::agent::list::run(),
            AgentCommands::Register => commands::agent::register::run(),
            AgentCommands::Remove {
                name,
                force,
                delete_branch,
            } => commands::agent::remove::run(name, force, delete_branch),
            AgentCommands::Prune => commands::agent::prune::run(),
            AgentCommands::Sync => commands::agent::sync::run(),
        },
    };

    // Show update notification (from cache, instant) and spawn background check for next run
    update::show_update_notification();
    update::check_in_background();

    result
}
