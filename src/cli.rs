use crate::{commands, config::Config, tui, update};
use anyhow::Result;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use std::{io::IsTerminal, time::Duration};

const DEFAULT_GITHUB_LIST_LIMIT: u8 = 30;

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
    /// Create new PRs as drafts; convert existing PRs to draft
    #[arg(short, long, conflicts_with = "publish")]
    draft: bool,
    /// Create new PRs as published; convert existing draft PRs to published
    #[arg(long, conflicts_with = "draft")]
    publish: bool,
    /// Only push, don't create/update PRs
    #[arg(long)]
    no_pr: bool,
    /// Skip git fetch and use cached remote-tracking refs
    #[arg(long = "no-fetch", action = clap::ArgAction::SetTrue)]
    no_fetch: bool,
    /// Deprecated: kept for CLI compatibility (currently a no-op)
    #[arg(short, long, hide = true)]
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
    /// Squash all commits on each branch into one before pushing
    #[arg(long)]
    squash: bool,
    /// Update existing PR titles when the tip commit subject has changed
    #[arg(long)]
    update_title: bool,
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

#[derive(Args, Clone, Default)]
struct WorktreeLaunchArgs {
    /// Launch an AI agent after entering the worktree
    #[arg(long)]
    agent: Option<String>,
    /// Model override for the selected AI agent
    #[arg(long, requires = "agent")]
    model: Option<String>,
    /// Run an arbitrary shell command after entering the worktree
    #[arg(long, conflicts_with = "agent")]
    run: Option<String>,
    /// Create or attach to a tmux session for this worktree
    #[arg(long)]
    tmux: bool,
    /// Override the tmux session name (defaults to the worktree name)
    #[arg(long, requires = "tmux")]
    tmux_session: Option<String>,
    /// Auto-accept agent permission prompts (claude: --dangerously-skip-permissions,
    /// codex: --dangerously-bypass-approvals-and-sandbox, opencode: --dangerously-skip-permissions,
    /// gemini: --yolo). Use with care.
    #[arg(long, requires = "agent")]
    yolo: bool,
    /// Pass an extra argument to the launched agent (repeatable)
    #[arg(long = "agent-arg", requires = "agent")]
    agent_arg: Vec<String>,
    /// Arguments passed through to the launched agent or command (after `--`)
    #[arg(last = true)]
    args: Vec<String>,
}

#[derive(Args, Clone, Default)]
struct AiLaneArgs {
    /// AI agent override (claude, codex, gemini, opencode)
    #[arg(long)]
    agent: Option<String>,
    /// Model override for the selected AI agent
    #[arg(long, requires = "agent")]
    model: Option<String>,
    /// Launch directly in the terminal instead of tmux
    #[arg(long)]
    no_tmux: bool,
    /// Override the tmux session name (defaults to the lane name)
    #[arg(long, conflicts_with = "no_tmux")]
    tmux_session: Option<String>,
    /// Auto-accept agent permission prompts (claude: --dangerously-skip-permissions,
    /// codex: --dangerously-bypass-approvals-and-sandbox, opencode: --dangerously-skip-permissions,
    /// gemini: --yolo). Use with care.
    #[arg(long)]
    yolo: bool,
    /// Pass an extra argument to the launched agent (repeatable)
    #[arg(long = "agent-arg")]
    agent_arg: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show all stacks (simple tree view)
    #[command(visible_alias = "ls")]
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
    #[command(visible_alias = "ss", hide = true)]
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
        #[arg(long, conflicts_with_all = ["dry_run", "no_wait", "remote", "queue"])]
        when_ready: bool,
        /// Merge via GitHub API only (no local checkout/rebase/push); GitHub updates branches remotely
        #[arg(long, conflicts_with_all = ["dry_run", "no_wait", "when_ready", "queue"])]
        remote: bool,
        /// Enqueue PRs into the forge's merge queue instead of merging one-by-one.
        /// Supported on GitHub (merge queue) and GitLab (merge trains). Not available on Gitea.
        #[arg(long, conflicts_with_all = ["dry_run", "no_wait", "when_ready", "remote"])]
        queue: bool,
        /// Polling interval in seconds for --when-ready, --remote, and --queue
        #[arg(long, default_value = "15")]
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
        /// No-op: kept for CLI compatibility (use `--full` for fetch --prune of all remote-tracking refs)
        #[arg(long)]
        prune: bool,
        /// Fetch all remote branches with `--prune` (slower; default is trunk-only fetch + ls-remote)
        #[arg(long)]
        full: bool,
        /// Don't delete merged branches
        #[arg(long)]
        no_delete: bool,
        /// Also delete local branches whose upstream is gone
        #[arg(long)]
        delete_upstream_gone: bool,
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
    #[command(hide = true)]
    Restack {
        /// Restack all branches in the stack
        #[arg(short, long)]
        all: bool,
        /// Restack ancestors + current only (skip descendants)
        #[arg(long, conflicts_with = "all")]
        stop_here: bool,
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
        #[arg(long, value_enum, default_value_t = RestackSubmitAfter::No)]
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
        /// Internal: emit shell control lines for shell integration.
        #[arg(long, hide = true)]
        shell_output: bool,
    },

    /// Continue after resolving conflicts
    #[command(visible_alias = "cont")]
    Continue,

    /// Resolve in-progress rebase conflicts using AI and continue automatically
    Resolve {
        /// AI agent override (claude, codex, gemini, opencode)
        #[arg(long)]
        agent: Option<String>,
        /// Model override for the selected agent
        #[arg(long)]
        model: Option<String>,
        /// Maximum AI resolve rounds before stopping
        #[arg(long, default_value_t = 5)]
        max_rounds: usize,
    },

    /// Abort an in-progress rebase/conflict resolution
    Abort,

    /// Stage and amend changes into the current branch tip
    /// Creates the first branch-local commit when run with -m on a fresh tracked branch
    #[command(visible_alias = "m")]
    Modify {
        /// New commit message (keeps existing if not provided)
        #[arg(short, long)]
        message: Option<String>,
        /// Stage all changes (like git commit --all)
        #[arg(short, long)]
        all: bool,
        /// Suppress extra output
        #[arg(long)]
        quiet: bool,
        /// Skip pre-commit and commit-msg hooks
        #[arg(long = "no-verify", short = 'n')]
        no_verify: bool,
        /// Restack the stack after modifying
        #[arg(short, long)]
        restack: bool,
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
    Config {
        /// Clear saved AI agent/model defaults so stax prompts again later
        #[arg(long)]
        reset_ai: bool,
        /// Clear saved AI defaults without opening the interactive picker
        #[arg(long, requires = "reset_ai")]
        no_prompt: bool,
        /// Skip confirmation when used with --reset-ai
        #[arg(short, long, requires = "reset_ai")]
        yes: bool,
        /// Interactively set AI agent/model for a specific feature (or global default)
        #[arg(long, conflicts_with = "reset_ai")]
        set_ai: bool,
    },

    /// Initialize stax or reconfigure the repo trunk branch
    Init {
        /// Set the trunk branch directly instead of prompting
        #[arg(long)]
        trunk: Option<String>,
    },

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

    /// Switch to the trunk branch, or set it with `stax trunk <branch>`
    #[command(visible_alias = "t")]
    Trunk {
        /// Set this branch as the new trunk
        branch: Option<String>,
    },

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

    /// Stack commands (submit, restack)
    #[command(subcommand, visible_alias = "s")]
    Stack(StackCommands),

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
        /// Insert between current branch and its children (reparent children)
        #[arg(long)]
        insert: bool,
    },

    /// Open the current branch PR or list repo pull requests
    Pr {
        #[command(subcommand)]
        command: Option<PrCommands>,
    },

    /// Browse open issues in the current repository
    Issue {
        #[command(subcommand)]
        command: Option<IssueCommands>,
    },

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
    Split {
        /// Split by selecting individual hunks instead of by commit
        #[arg(long, conflicts_with = "file")]
        hunk: bool,

        /// Extract changes to matching files into a new parent branch (repeatable)
        #[arg(long, short = 'f', num_args = 1.., conflicts_with = "hunk")]
        file: Vec<String>,

        /// Skip pre-commit hooks when committing split branches
        #[arg(long)]
        no_verify: bool,
    },

    /// Absorb staged changes into the correct stack branches
    Absorb {
        /// Show what would be absorbed without making changes
        #[arg(long)]
        dry_run: bool,
        /// Stage all changes before absorbing (like -a)
        #[arg(short, long)]
        all: bool,
    },

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

    /// Interactively edit commits on the current branch (reword, squash, fixup, drop)
    #[command(visible_alias = "e")]
    Edit {
        /// Skip the final confirmation prompt after interactive commit selection
        #[arg(long)]
        yes: bool,
        /// Skip pre-commit hooks when recreating commits
        #[arg(long)]
        no_verify: bool,
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
    Run {
        /// Command to run
        #[arg(trailing_var_arg = true, required = true)]
        cmd: Vec<String>,
        /// Run on all tracked branches (not just current stack)
        #[arg(long)]
        all: bool,
        /// Run only one stack (current stack by default, or a specific branch's stack with --stack=<branch>)
        #[arg(long, num_args = 0..=1, require_equals = true)]
        stack: Option<Option<String>>,
        /// Stop after first failure
        #[arg(long)]
        fail_fast: bool,
    },

    /// Backward-compatible alias for `run`
    #[command(hide = true)]
    Test {
        /// Command to run
        #[arg(trailing_var_arg = true, required = true)]
        cmd: Vec<String>,
        /// Run on all tracked branches (not just current stack)
        #[arg(long)]
        all: bool,
        /// Run only one stack (current stack by default, or a specific branch's stack with --stack=<branch>)
        #[arg(long, num_args = 0..=1, require_equals = true)]
        stack: Option<Option<String>>,
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
        /// Include Jira sprint context from `jit` (https://github.com/cesarferreira/jit)
        #[arg(long)]
        jit: bool,
        /// AI agent to use (claude, codex, gemini, opencode). Defaults to config or auto-detect
        #[arg(long)]
        agent: Option<String>,
        /// Model to use with the AI agent. Defaults to config or agent's default
        #[arg(long)]
        model: Option<String>,
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
        /// Disable interactive prompts (use defaults)
        #[arg(long)]
        no_prompt: bool,
        /// AI agent to use (claude, codex, gemini, opencode). Defaults to config or auto-detect
        #[arg(long)]
        agent: Option<String>,
        /// Model to use with the AI agent. Defaults to config or agent's default
        #[arg(long)]
        model: Option<String>,
        /// PR template name to use (e.g. feature, bugfix). Skips template selection prompt
        #[arg(long)]
        template: Option<String>,
        /// Skip PR template entirely
        #[arg(long)]
        no_template: bool,
    },

    /// Generate changelog between two refs
    Changelog {
        /// Starting ref (tag, branch, or commit). Defaults to last tag if omitted.
        from: Option<String>,
        /// Ending ref (defaults to HEAD)
        to: Option<String>,
        /// Only consider tags matching this prefix when auto-resolving (e.g. release/ios)
        #[arg(long)]
        tag_prefix: Option<String>,
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

    /// Manage worktrees for parallel branch development (`st wt` opens the dashboard in a TTY)
    #[command(visible_alias = "wt")]
    Worktree {
        #[command(subcommand)]
        command: Option<WorktreeCommands>,
    },

    /// Resume or start an AI worktree lane
    Lane {
        /// Lane name (omit to open the interactive lane picker)
        name: Option<String>,
        /// Optional prompt passed to the launched AI agent
        #[arg(allow_hyphen_values = true)]
        prompt: Option<String>,
        /// Skip worktree hooks
        #[arg(long = "no-verify")]
        no_verify: bool,
        #[arg(long, hide = true)]
        shell_output: bool,
        #[command(flatten)]
        ai: AiLaneArgs,
    },

    /// Output shell integration snippet for manual install or use `--install`
    #[command(name = "shell-setup")]
    ShellSetup {
        /// Write shell integration under ~/.config/stax and source it from your shell config
        #[arg(long)]
        install: bool,
        /// Refresh already-installed generated shell snippets in-place
        #[arg(long, hide = true, conflicts_with = "install")]
        refresh: bool,
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
        /// Insert between current branch and its children (reparent children)
        #[arg(long)]
        insert: bool,
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
    /// List worktrees (alias for `stax worktree list`)
    #[command(hide = true)]
    W,
    #[command(hide = true)]
    Wtc {
        name: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        pick: bool,
        #[arg(long = "name")]
        worktree_name: Option<String>,
        #[arg(long)]
        no_verify: bool,
        #[arg(long, hide = true)]
        shell_output: bool,
        #[command(flatten)]
        launch: WorktreeLaunchArgs,
    },
    #[command(hide = true)]
    Wtls,
    #[command(hide = true)]
    Wtll {
        #[arg(long)]
        json: bool,
    },
    #[command(hide = true)]
    Wtgo {
        name: Option<String>,
        #[arg(long)]
        no_verify: bool,
        #[arg(long, hide = true)]
        shell_output: bool,
        #[command(flatten)]
        launch: WorktreeLaunchArgs,
    },
    #[command(hide = true)]
    Wtrm {
        name: Option<String>,
        #[arg(short, long)]
        force: bool,
        #[arg(long)]
        delete_branch: bool,
    },
    #[command(hide = true)]
    Wtprune,
    #[command(hide = true)]
    Wtcleanup {
        #[arg(short, long)]
        force: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    #[command(hide = true)]
    Wtrs,
    #[command(hide = true)]
    Sr {
        #[arg(short, long)]
        all: bool,
        #[arg(long, conflicts_with = "all")]
        stop_here: bool,
        #[arg(long)]
        r#continue: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
        #[arg(long)]
        quiet: bool,
        #[arg(long)]
        auto_stash_pop: bool,
        #[arg(long, value_enum, default_value_t = RestackSubmitAfter::No)]
        submit_after: RestackSubmitAfter,
    },
}

#[derive(Subcommand, Clone)]
enum AuthSubcommand {
    /// Show which auth source is currently active
    Status,
}

#[derive(Subcommand)]
enum StackCommands {
    /// Submit stack - push branches and create/update PRs
    #[command(visible_alias = "s")]
    Submit {
        #[command(flatten)]
        submit: SubmitOptions,
    },

    /// Restack (rebase) the current branch onto its parent
    #[command(visible_alias = "r")]
    Restack {
        /// Restack all branches in the stack
        #[arg(short, long)]
        all: bool,
        /// Restack ancestors + current only (skip descendants)
        #[arg(long, conflicts_with = "all")]
        stop_here: bool,
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
        #[arg(long, value_enum, default_value_t = RestackSubmitAfter::No)]
        submit_after: RestackSubmitAfter,
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
        /// Insert between current branch and its children (reparent children)
        #[arg(long)]
        insert: bool,
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
        /// Internal: emit shell control lines for shell integration.
        #[arg(long, hide = true)]
        shell_output: bool,
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
        /// Rebase the branch onto the new parent immediately (rewrites history)
        #[arg(long)]
        restack: bool,
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

    /// Reparent current branch and all descendants onto a new parent
    Onto {
        /// Target parent branch (interactive picker if omitted)
        target: Option<String>,
        /// Restack the moved branches after reparenting
        #[arg(long)]
        restack: bool,
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
enum WorktreeCommands {
    /// Create or enter a worktree lane
    #[command(visible_alias = "c")]
    Create {
        /// Branch/worktree name to create or enter
        name: Option<String>,
        /// Create from an explicit base branch
        #[arg(long)]
        from: Option<String>,
        /// Pick an existing branch interactively
        #[arg(long)]
        pick: bool,
        /// Override the short name for the worktree directory
        #[arg(long = "name")]
        worktree_name: Option<String>,
        /// Skip worktree hooks
        #[arg(long = "no-verify")]
        no_verify: bool,
        #[arg(long, hide = true)]
        shell_output: bool,
        #[command(flatten)]
        launch: WorktreeLaunchArgs,
    },

    /// List all worktrees
    #[command(visible_aliases = ["ls"])]
    List {
        /// Output JSON instead of the compact table
        #[arg(long)]
        json: bool,
    },

    /// Show a richer worktree status view
    #[command(name = "ll")]
    LongList {
        /// Output JSON instead of the long table
        #[arg(long)]
        json: bool,
    },

    /// Navigate to a worktree (requires shell integration)
    Go {
        name: Option<String>,
        /// Skip worktree hooks
        #[arg(long = "no-verify")]
        no_verify: bool,
        #[arg(long, hide = true)]
        shell_output: bool,
        #[command(flatten)]
        launch: WorktreeLaunchArgs,
    },

    /// Print the absolute path of a worktree (for scripting / shell integration)
    Path { name: String },

    /// Remove a worktree
    #[command(visible_alias = "rm")]
    Remove {
        name: Option<String>,
        /// Force removal even if the worktree has uncommitted changes
        #[arg(short, long)]
        force: bool,
        /// Also delete the branch and stax metadata when safe
        #[arg(long)]
        delete_branch: bool,
    },

    /// Remove stale git worktree bookkeeping
    Prune,

    /// Prune stale bookkeeping and bulk-remove safe detached/merged worktrees
    #[command(visible_alias = "clean")]
    Cleanup {
        /// Force removal even if a candidate worktree has uncommitted changes
        #[arg(short, long)]
        force: bool,
        /// Preview prune/remove decisions without applying them
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Restack all stax-managed worktrees
    #[command(visible_alias = "rs")]
    Restack,
}

#[derive(Subcommand, Clone)]
enum PrCommands {
    /// Open the current branch PR in the browser
    Open,

    /// List open pull requests in the current repository
    List {
        /// Maximum number of pull requests to return (max: 100)
        #[arg(long, default_value_t = DEFAULT_GITHUB_LIST_LIMIT, value_parser = clap::value_parser!(u8).range(1..=100))]
        limit: u8,
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Clone)]
enum IssueCommands {
    /// List open issues in the current repository
    List {
        /// Maximum number of issues to return (max: 100)
        #[arg(long, default_value_t = DEFAULT_GITHUB_LIST_LIMIT, value_parser = clap::value_parser!(u8).range(1..=100))]
        limit: u8,
        /// Output JSON for scripting
        #[arg(long)]
        json: bool,
    },
}

fn run_submit(submit: SubmitOptions, scope: commands::submit::SubmitScope) -> Result<()> {
    commands::submit::run(
        scope,
        submit.draft,
        submit.publish,
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
        submit.squash,
        submit.update_title,
    )
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

    if let Some(Commands::ShellSetup { install, refresh }) = &cli.command {
        let result = commands::shell_setup::run(*install, *refresh);
        update::show_update_notification();
        update::check_in_background();
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
                update::check_in_background();
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
        Commands::Config {
            reset_ai,
            no_prompt,
            yes,
            set_ai,
        } => {
            let result = commands::config::run(*reset_ai, *no_prompt, *yes, *set_ai);
            update::show_update_notification();
            update::check_in_background();
            return result;
        }
        Commands::Init { trunk } => {
            let result = commands::init::run(trunk.clone());
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
            remote,
            queue,
            interval,
            no_sync,
            yes,
            quiet,
        } => {
            let merge_method = method.parse().unwrap_or_default();
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
            } else if when_ready {
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
            full,
            no_delete,
            delete_upstream_gone,
            force,
            safe,
            r#continue,
            quiet,
            verbose,
            auto_stash_pop,
        } => commands::sync::run(
            restack,
            prune,
            full,
            !no_delete,
            delete_upstream_gone,
            force,
            safe,
            r#continue,
            quiet,
            verbose,
            auto_stash_pop,
        ),
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
        Commands::Checkout {
            branch,
            trunk,
            parent,
            child,
            shell_output,
        } => commands::checkout::run(branch, trunk, parent, child, shell_output),
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
        Commands::Config { .. } => unreachable!(), // Handled above
        Commands::Init { .. } => unreachable!(), // Handled above
        Commands::Diff { stack, all } => commands::diff::run(stack, all),
        Commands::RangeDiff { stack, all } => commands::range_diff::run(stack, all),
        Commands::Doctor => unreachable!(), // Handled above
        Commands::Trunk { branch } => {
            if let Some(name) = branch {
                commands::set_trunk::run(&name)
            } else {
                commands::checkout::run(None, true, false, None, false)
            }
        }
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
            insert,
        } => commands::branch::create::run(name, message, from, prefix, all, insert),
        Commands::Pr { command } => match command.unwrap_or(PrCommands::Open) {
            PrCommands::Open => commands::pr::run_open(),
            PrCommands::List { limit, json } => commands::pr::run_list(limit, json),
        },
        Commands::Issue { command } => match command {
            Some(IssueCommands::List { limit, json }) => commands::issue::run_list(limit, json),
            None => print_subcommand_help("issue"),
        },
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
        Commands::Reorder { yes } => commands::reorder::run(yes),
        Commands::Edit { yes, no_verify } => commands::edit::run(yes, no_verify),
        Commands::Validate => commands::stack_cmd::run_validate(),
        Commands::Fix { dry_run, yes } => commands::stack_cmd::run_fix(dry_run, yes),
        Commands::Run {
            cmd,
            all,
            stack,
            fail_fast,
        }
        | Commands::Test {
            cmd,
            all,
            stack,
            fail_fast,
        } => commands::stack_cmd::run_test(cmd, all, stack, fail_fast),
        Commands::Demo => unreachable!(), // Handled above
        Commands::Standup {
            json,
            all,
            hours,
            summary,
            jit,
            agent,
            model,
            plain_text,
        } => commands::standup::run(json, all, hours, summary, jit, agent, model, plain_text),
        Commands::Generate {
            pr_body,
            edit,
            no_prompt,
            agent,
            model,
            template,
            no_template,
        } => {
            if !pr_body {
                anyhow::bail!("Please specify what to generate. Usage: stax generate --pr-body");
            }
            commands::generate::run(edit, no_prompt, agent, model, template, no_template)
        }
        Commands::Changelog {
            from,
            to,
            tag_prefix,
            path,
            json,
        } => commands::changelog::run(
            from,
            to.unwrap_or_else(|| "HEAD".to_string()),
            tag_prefix,
            path,
            json,
        ),
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
                insert,
            } => commands::branch::create::run(name, message, from, prefix, all, insert),
            BranchCommands::Checkout {
                branch,
                trunk,
                parent,
                child,
                shell_output,
            } => commands::checkout::run(branch, trunk, parent, child, shell_output),
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
            UpstackCommands::Onto { target, restack } => {
                commands::upstack::onto::run(target, restack)
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
        },
        // Hidden shortcuts
        Commands::Bc {
            name,
            all,
            message,
            from,
            prefix,
            insert,
        } => commands::branch::create::run(name, message, from, prefix, all, insert),
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
            Some(WorktreeCommands::Prune) => commands::worktree::prune::run(),
            Some(WorktreeCommands::Cleanup {
                force,
                dry_run,
                yes,
            }) => commands::worktree::cleanup::run(force, yes, dry_run),
            Some(WorktreeCommands::Restack) => commands::worktree::restack::run(),
        },
        Commands::ShellSetup { .. } => {
            unreachable!("shell-setup returns before repo initialization")
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

    // Show update notification (from cache, instant) and spawn background check for next run
    update::show_update_notification();
    update::check_in_background();

    result
}

fn detect_interactive_stdio() -> (bool, bool) {
    #[cfg(debug_assertions)]
    if std::env::var_os("STAX_TEST_FORCE_INTERACTIVE_TERMINAL").is_some() {
        // Integration tests use this to drive the interactive fallback path without a real PTY.
        return (true, true);
    }

    (
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    )
}

fn has_interactive_terminal(stdin_is_terminal: bool, stdout_is_terminal: bool) -> bool {
    stdin_is_terminal && stdout_is_terminal
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InteractiveTerminalCheck {
    available: bool,
    reason: Option<String>,
}

fn check_interactive_terminal(
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> InteractiveTerminalCheck {
    check_interactive_terminal_with_probe(stdin_is_terminal, stdout_is_terminal, || {
        #[cfg(debug_assertions)]
        if let Ok(reason) = std::env::var("STAX_TEST_FORCE_INPUT_READER_FAILURE") {
            // Integration tests use this to exercise the interactive fallback path deterministically.
            return Err(reason);
        }

        crossterm::event::poll(Duration::from_millis(0))
            .map(|_| ())
            .map_err(|err| err.to_string())
    })
}

fn check_interactive_terminal_with_probe<F>(
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    probe_input_reader: F,
) -> InteractiveTerminalCheck
where
    F: FnOnce() -> std::result::Result<(), String>,
{
    if !has_interactive_terminal(stdin_is_terminal, stdout_is_terminal) {
        return InteractiveTerminalCheck {
            available: false,
            reason: None,
        };
    }

    match probe_input_reader() {
        Ok(()) => InteractiveTerminalCheck {
            available: true,
            reason: None,
        },
        Err(reason) => InteractiveTerminalCheck {
            available: false,
            reason: Some(reason),
        },
    }
}

fn print_interactive_fallback(reason: Option<&str>, dashboard: &str, fallback: &str) {
    if let Some(reason) = reason {
        eprintln!(
            "stax: interactive {} unavailable ({}); {}.",
            dashboard, reason, fallback
        );
    }
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

#[cfg(test)]
mod tests {
    use super::{
        check_interactive_terminal_with_probe, detect_interactive_stdio, has_interactive_terminal,
        Cli, Commands, InteractiveTerminalCheck, RestackSubmitAfter, StackCommands,
        WorktreeCommands,
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
}
