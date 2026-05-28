use crate::commands;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

pub(crate) const DEFAULT_GITHUB_LIST_LIMIT: u8 = 30;

#[derive(Parser)]
#[command(name = "stax")]
#[command(version)]
#[command(about = "Fast stacked Git branches and PRs", long_about = None)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Args, Clone)]
pub(crate) struct SubmitOptions {
    /// Create new PRs as drafts; convert existing PRs to draft
    #[arg(short, long, conflicts_with = "publish")]
    pub(crate) draft: bool,
    /// Create new PRs as published; convert existing draft PRs to published
    #[arg(long, conflicts_with = "draft")]
    pub(crate) publish: bool,
    /// Only push, don't create/update PRs
    #[arg(long)]
    pub(crate) no_pr: bool,
    /// Skip git fetch and use cached remote-tracking refs
    #[arg(long = "no-fetch", action = clap::ArgAction::SetTrue)]
    pub(crate) no_fetch: bool,
    /// Skip pre-push hooks when pushing branches
    #[arg(long = "no-verify", short = 'n')]
    pub(crate) no_verify: bool,
    /// Deprecated: kept for CLI compatibility (currently a no-op)
    #[arg(long, hide = true)]
    pub(crate) force: bool,
    /// Auto-approve prompts
    #[arg(long)]
    pub(crate) yes: bool,
    /// Disable interactive prompts (use defaults)
    #[arg(long)]
    pub(crate) no_prompt: bool,
    /// Assign reviewers (comma-separated or repeat)
    #[arg(long, value_delimiter = ',')]
    pub(crate) reviewers: Vec<String>,
    /// Add labels (comma-separated or repeat)
    #[arg(long, value_delimiter = ',')]
    pub(crate) labels: Vec<String>,
    /// Assign users (comma-separated or repeat)
    #[arg(long, value_delimiter = ',')]
    pub(crate) assignees: Vec<String>,
    /// Suppress extra output
    #[arg(long)]
    pub(crate) quiet: bool,
    /// Open the current branch PR in browser after submit
    #[arg(long, conflicts_with = "no_pr")]
    pub(crate) open: bool,
    /// Show detailed output
    #[arg(short, long)]
    pub(crate) verbose: bool,
    /// Specify template by name (skip picker)
    #[arg(long)]
    pub(crate) template: Option<String>,
    /// Skip template selection (no template)
    #[arg(long)]
    pub(crate) no_template: bool,
    /// Always open editor for PR body
    #[arg(long)]
    pub(crate) edit: bool,
    /// Generate PR title and body using AI
    #[arg(long)]
    pub(crate) ai: bool,
    /// With --ai, generate/update PR title only
    #[arg(long, requires = "ai")]
    pub(crate) title: bool,
    /// With --ai, generate/update PR body only
    #[arg(long, requires = "ai")]
    pub(crate) body: bool,
    /// Re-request review from existing reviewers when updating PRs
    #[arg(long)]
    pub(crate) rerequest_review: bool,
    /// Squash all commits on each branch into one before pushing
    #[arg(long)]
    pub(crate) squash: bool,
    /// Update existing PR titles when the tip commit subject has changed
    #[arg(long)]
    pub(crate) update_title: bool,
}

impl From<SubmitOptions> for commands::submit::SubmitOptions {
    fn from(submit: SubmitOptions) -> Self {
        Self {
            draft: submit.draft,
            publish: submit.publish,
            no_pr: submit.no_pr,
            no_fetch: submit.no_fetch,
            prefetched: false,
            no_verify: submit.no_verify,
            force: submit.force,
            yes: submit.yes,
            no_prompt: submit.no_prompt,
            reviewers: submit.reviewers,
            labels: submit.labels,
            assignees: submit.assignees,
            quiet: submit.quiet,
            open: submit.open,
            verbose: submit.verbose,
            template: submit.template,
            no_template: submit.no_template,
            edit: submit.edit,
            ai: submit.ai,
            title: submit.title,
            body: submit.body,
            rerequest_review: submit.rerequest_review,
            squash: submit.squash,
            update_title: submit.update_title,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum RestackSubmitAfter {
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

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum StandupSummaryStyle {
    Spoken,
    Slack,
}

impl From<StandupSummaryStyle> for commands::standup::SummaryStyle {
    fn from(value: StandupSummaryStyle) -> Self {
        match value {
            StandupSummaryStyle::Spoken => commands::standup::SummaryStyle::Spoken,
            StandupSummaryStyle::Slack => commands::standup::SummaryStyle::Slack,
        }
    }
}

#[derive(Args, Clone, Default)]
pub(crate) struct WorktreeLaunchArgs {
    /// Launch an AI agent after entering the worktree
    #[arg(long)]
    pub(crate) agent: Option<String>,
    /// Model override for the selected AI agent
    #[arg(long, requires = "agent")]
    pub(crate) model: Option<String>,
    /// Run an arbitrary shell command after entering the worktree
    #[arg(long, conflicts_with = "agent")]
    pub(crate) run: Option<String>,
    /// Create or attach to a tmux session for this worktree
    #[arg(long)]
    pub(crate) tmux: bool,
    /// Override the tmux session name (defaults to the worktree name)
    #[arg(long, requires = "tmux")]
    pub(crate) tmux_session: Option<String>,
    /// Auto-accept agent permission prompts (claude: --dangerously-skip-permissions,
    /// codex: --dangerously-bypass-approvals-and-sandbox, opencode: --dangerously-skip-permissions,
    /// gemini: --yolo). Use with care.
    #[arg(long, requires = "agent")]
    pub(crate) yolo: bool,
    /// Pass an extra argument to the launched agent (repeatable)
    #[arg(long = "agent-arg", requires = "agent")]
    pub(crate) agent_arg: Vec<String>,
    /// Arguments passed through to the launched agent or command (after `--`)
    #[arg(last = true)]
    pub(crate) args: Vec<String>,
}

#[derive(Args, Clone, Default)]
pub(crate) struct AiLaneArgs {
    /// AI agent override (claude, codex, gemini, opencode)
    #[arg(long)]
    pub(crate) agent: Option<String>,
    /// Model override for the selected AI agent
    #[arg(long, requires = "agent")]
    pub(crate) model: Option<String>,
    /// Launch directly in the terminal instead of tmux
    #[arg(long)]
    pub(crate) no_tmux: bool,
    /// Override the tmux session name (defaults to the lane name)
    #[arg(long, conflicts_with = "no_tmux")]
    pub(crate) tmux_session: Option<String>,
    /// Auto-accept agent permission prompts (claude: --dangerously-skip-permissions,
    /// codex: --dangerously-bypass-approvals-and-sandbox, opencode: --dangerously-skip-permissions,
    /// gemini: --yolo). Use with care.
    #[arg(long)]
    pub(crate) yolo: bool,
    /// Pass an extra argument to the launched agent (repeatable)
    #[arg(long = "agent-arg")]
    pub(crate) agent_arg: Vec<String>,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
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
        /// Merge ancestors below current, then rebase current branch
        #[arg(long, visible_alias = "ds", conflicts_with_all = ["all", "remote", "queue"])]
        downstack_only: bool,
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
        /// Merge ancestors below current, then rebase current branch
        #[arg(long, visible_alias = "ds", conflicts_with = "all")]
        downstack_only: bool,
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

    /// Sync trunk, restack current stack, then submit updates
    #[command(alias = "refresh")]
    Update {
        /// Push branches to remote but skip PR creation/updates
        #[arg(long)]
        no_pr: bool,
        /// Skip all remote interaction after restack (local update only)
        #[arg(long)]
        no_submit: bool,
        /// Force sync without prompts
        #[arg(short, long)]
        force: bool,
        /// Avoid hard reset when updating trunk
        #[arg(long)]
        safe: bool,
        /// Show detailed sync/restack/submit timing
        #[arg(long)]
        verbose: bool,
        /// Accept submit defaults without confirmation
        #[arg(short, long)]
        yes: bool,
        /// Use submit defaults instead of prompting for PR details
        #[arg(long)]
        no_prompt: bool,
        /// Auto-stash and auto-pop dirty target worktrees during sync/restack
        #[arg(long)]
        auto_stash_pop: bool,
    },

    /// Checkout a branch in the stack
    #[command(visible_aliases = ["co", "bco"])]
    Checkout {
        /// Branch name (interactive if not provided)
        branch: Option<String>,
        /// Checkout branch by PR number
        #[arg(long, conflicts_with = "branch")]
        pr: Option<u64>,
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

    /// Manage the installed stax CLI
    Cli {
        #[command(subcommand)]
        command: CliSubcommand,
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
    Doctor {
        /// Apply safe local repairs after showing a repair plan
        #[arg(long)]
        fix: bool,
    },

    /// Manage AI agent skill files (`stax skills update` to refresh)
    Skills {
        #[command(subcommand)]
        command: Option<SkillsCommands>,
    },

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
    #[command(visible_aliases = ["c", "add"])]
    Create {
        /// Name for the new branch
        name: Option<String>,
        /// Stage all changes (like git commit --all)
        #[arg(short, long)]
        all: bool,
        /// Commit message (also used as branch name if no name provided)
        #[arg(short, long)]
        message: Option<String>,
        /// Generate missing branch name and/or first commit message with AI
        #[arg(long)]
        ai: bool,
        /// Accept generated AI values without prompting
        #[arg(short, long)]
        yes: bool,
        /// Base branch to create from (defaults to current)
        #[arg(long)]
        from: Option<String>,
        /// Override branch prefix (e.g. "feature/")
        #[arg(long)]
        prefix: Option<String>,
        /// Insert between current branch and its children (reparent children)
        #[arg(long, conflicts_with = "below")]
        insert: bool,
        /// Insert below current branch (reparent current and descendants)
        #[arg(long, conflicts_with_all = ["insert", "from"])]
        below: bool,
        /// Skip pre-commit and commit-msg hooks
        #[arg(long = "no-verify", short = 'n')]
        no_verify: bool,
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

    /// Mark the current (or named) branch's PR as a draft
    Draft {
        /// Branch to operate on (defaults to current)
        branch: Option<String>,
    },

    /// Mark the current (or named) branch's PR as ready for review
    Undraft {
        /// Branch to operate on (defaults to current)
        branch: Option<String>,
    },

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
        /// Play success/error sounds when --watch exits; optionally pass one sound file for both
        #[arg(
            long,
            short = 'a',
            value_name = "SOUND",
            num_args = 0..=1,
            require_equals = false,
            requires = "watch",
            conflicts_with = "no_alert"
        )]
        alert: Option<Option<PathBuf>>,
        /// Disable configured CI completion alerts for this run
        #[arg(long, requires = "watch", conflicts_with = "alert")]
        no_alert: bool,
        /// Exit watch mode as soon as any check fails
        #[arg(long, requires = "watch")]
        strict: bool,
        /// Polling interval in seconds (default: 15)
        #[arg(long, default_value = "15")]
        interval: u64,
        /// Show compact summary cards instead of the full per-check table
        #[arg(long, short)]
        verbose: bool,
    },

    /// Live auto-refreshing stack status with CI and PR state
    Watch {
        /// Watch only the current stack (not all tracked branches)
        #[arg(long, short)]
        current: bool,
        /// Polling interval in seconds (overrides adaptive default)
        #[arg(long, short)]
        interval: Option<u64>,
    },

    /// tmux integration: status bar string and popup viewer
    Tmux {
        #[command(subcommand)]
        command: commands::tmux::TmuxCommand,
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

    /// Fold the current branch into its parent (collapse a branch boundary).
    ///
    /// Top-level alias of `stax branch fold`, mirroring `gt fold`. Commits are
    /// preserved (not squashed); descendants of the current branch are
    /// reparented onto the parent; siblings are rebased onto the new parent
    /// tip. With `--keep`, the surviving branch keeps the *current* name and
    /// the parent ref is deleted instead.
    Fold {
        /// Keep the current branch's name as the surviving ref (delete the
        /// parent ref instead)
        #[arg(short, long)]
        keep: bool,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Move the current branch (and its descendants) onto a new parent.
    ///
    /// Equivalent to `stax upstack onto`; kept as a top-level alias for
    /// graphite parity (`gt move`).
    #[command(visible_alias = "mv")]
    Move {
        /// Target parent branch (interactive picker if omitted)
        target: Option<String>,
        /// Accepted for backward compatibility; restack now always runs
        #[arg(long, hide = true)]
        restack: bool,
        /// Stash uncommitted changes before rebasing and restore them after
        #[arg(long)]
        auto_stash_pop: bool,
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
        /// Output raw JSON (standup data, or summary JSON when combined with --ai)
        #[arg(long)]
        json: bool,
        /// Show all stacks (not just current)
        #[arg(long)]
        all: bool,
        /// Time window in hours (default: 24)
        #[arg(long, default_value = "24")]
        hours: i64,
        /// Summarize standup using AI
        #[arg(long)]
        ai: bool,
        /// AI summary style (spoken or Slack-ready bullets)
        #[arg(long, value_enum, requires = "ai")]
        style: Option<StandupSummaryStyle>,
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

    /// Generate content using AI (interactive picker if no artifact flag is given)
    #[command(alias = "gen")]
    Generate {
        /// Generate and update the current PR's body
        #[arg(long)]
        pr_body: bool,
        /// Generate and update the current PR's title
        #[arg(long)]
        pr_title: bool,
        /// Amend the HEAD commit message using AI
        #[arg(long)]
        commit_msg: bool,
        /// Open editor to review before applying
        #[arg(long)]
        edit: bool,
        /// Disable interactive prompts (use defaults)
        #[arg(long)]
        no_prompt: bool,
        /// AI agent to use (claude, codex, gemini, opencode). Defaults to config or auto-detect
        #[arg(long)]
        agent: Option<String>,
        /// Model to use with the AI agent. Defaults to config or agent's default
        #[arg(long, requires = "agent")]
        model: Option<String>,
        /// PR template name to use (e.g. feature, bugfix). Skips template selection prompt (--pr-body only)
        #[arg(long)]
        template: Option<String>,
        /// Skip PR template entirely (--pr-body only)
        #[arg(long)]
        no_template: bool,
    },

    /// Generate changelog between refs or fuzzy-find commits with `find [query]`
    Changelog {
        /// Starting ref (tag, branch, or commit). Defaults to last tag if omitted. Use `find [query]` to fuzzy-find commits.
        from: Option<String>,
        /// Ending ref (defaults to HEAD)
        to: Option<String>,
        /// Fuzzy-find commits in the selected range. Omit QUERY to open an interactive picker.
        #[arg(long, alias = "search", value_name = "QUERY", num_args = 0..=1)]
        find: Option<Option<String>>,
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

    /// Setup shell integration and enable git rerere
    ///
    /// This command installs shell integration into your shell config file
    /// (~/.bashrc, ~/.zshrc, etc.) and enables git rerere for conflict resolution.
    ///
    /// For manual install, use --print to show the snippet, then add it to your
    /// shell config. The auto-install writes to ~/.config/stax/shell-setup.sh
    /// and sources it from your shell config.
    #[command(name = "setup", alias = "shell-setup")]
    Setup {
        /// Print shell integration snippet instead of installing
        #[arg(long)]
        print: bool,
        /// Refresh already-installed generated shell snippets in-place
        #[arg(long, hide = true, conflicts_with = "print")]
        refresh: bool,
        /// Skip the optional AI agent skills install prompt
        #[arg(long, conflicts_with_all = ["install_skills", "print", "refresh"])]
        skip_skills: bool,
        /// Install AI agent skills without prompting
        #[arg(long, conflicts_with_all = ["skip_skills", "print", "refresh"])]
        install_skills: bool,
        /// Skip the optional GitHub auth onboarding step
        #[arg(long, conflicts_with_all = ["auth_from_gh", "print", "refresh"])]
        skip_auth: bool,
        /// Import GitHub auth from `gh auth token` without prompting
        #[arg(long, conflicts_with_all = ["skip_auth", "print", "refresh"])]
        auth_from_gh: bool,
        /// Accept default setup actions without prompting
        #[arg(short, long, conflicts_with_all = ["print", "refresh"])]
        yes: bool,
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
        /// Generate missing branch name and/or first commit message with AI
        #[arg(long)]
        ai: bool,
        /// Accept generated AI values without prompting
        #[arg(short, long)]
        yes: bool,
        /// Base branch to create from (defaults to current)
        #[arg(long)]
        from: Option<String>,
        /// Override branch prefix (e.g. "feature/")
        #[arg(long)]
        prefix: Option<String>,
        /// Insert between current branch and its children (reparent children)
        #[arg(long, conflicts_with = "below")]
        insert: bool,
        /// Insert below current branch (reparent current and descendants)
        #[arg(long, conflicts_with_all = ["insert", "from"])]
        below: bool,
        /// Skip pre-commit and commit-msg hooks
        #[arg(long = "no-verify", short = 'n')]
        no_verify: bool,
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
pub(crate) enum AuthSubcommand {
    /// Show which auth source is currently active
    Status,
}

#[derive(Subcommand, Clone)]
pub(crate) enum CliSubcommand {
    /// Upgrade stax using the current installation method
    Upgrade,
}

#[derive(Subcommand)]
pub(crate) enum StackCommands {
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
pub(crate) enum BranchCommands {
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
        /// Generate missing branch name and/or first commit message with AI
        #[arg(long)]
        ai: bool,
        /// Accept generated AI values without prompting
        #[arg(short, long)]
        yes: bool,
        /// Base branch to create from (defaults to current)
        #[arg(long)]
        from: Option<String>,
        /// Override branch prefix (e.g. "feature/")
        #[arg(long)]
        prefix: Option<String>,
        /// Insert between current branch and its children (reparent children)
        #[arg(long, conflicts_with = "below")]
        insert: bool,
        /// Insert below current branch (reparent current and descendants)
        #[arg(long, conflicts_with_all = ["insert", "from"])]
        below: bool,
        /// Skip pre-commit and commit-msg hooks
        #[arg(long = "no-verify", short = 'n')]
        no_verify: bool,
    },

    /// Checkout a branch in the stack
    #[command(visible_alias = "co")]
    Checkout {
        /// Branch name (interactive if not provided)
        branch: Option<String>,
        /// Checkout branch by PR number
        #[arg(long, conflicts_with = "branch")]
        pr: Option<u64>,
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
pub(crate) enum UpstackCommands {
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
        /// Accepted for backward compatibility; restack now always runs
        #[arg(long, hide = true)]
        restack: bool,
        /// Stash uncommitted changes before rebasing and restore them after
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
pub(crate) enum DownstackCommands {
    /// Show branches below current
    Get,

    /// Submit ancestors and current branch
    Submit {
        #[command(flatten)]
        submit: SubmitOptions,
    },
}

#[derive(Subcommand)]
pub(crate) enum SkillsCommands {
    /// List installed AI agent skill files and their version status
    List,

    /// Download the latest skills from GitHub and update all installed skill files
    Update {
        /// Preview what would be updated without writing any files
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum WorktreeCommands {
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
pub(crate) enum PrCommands {
    /// Open the current branch PR in the browser
    Open,

    /// Print or edit the current branch PR description
    Body {
        /// Open the PR description in $EDITOR and update it on save
        #[arg(long)]
        edit: bool,
    },

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
pub(crate) enum IssueCommands {
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

impl Commands {
    pub(crate) fn policy(&self) -> CommandPolicy {
        match self {
            Commands::Continue | Commands::Resolve { .. } | Commands::Abort => {
                CommandPolicy::RebaseControl
            }
            Commands::Undo { .. } | Commands::Redo { .. } => CommandPolicy::RebaseSafe,
            Commands::Restack {
                r#continue: true, ..
            }
            | Commands::Sync {
                r#continue: true, ..
            } => CommandPolicy::RebaseSafe,
            _ => CommandPolicy::RequiresCleanRepoState,
        }
    }

    pub(crate) fn allows_during_rebase(&self) -> bool {
        !matches!(self.policy(), CommandPolicy::RequiresCleanRepoState)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandPolicy {
    RebaseControl,
    RebaseSafe,
    RequiresCleanRepoState,
}
