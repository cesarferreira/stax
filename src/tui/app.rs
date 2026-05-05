use crate::cache::{CiCache, DiskCachedDiff, DiskDiffLine, DiskDiffStat, TuiDiffCache};
use crate::ci::{history, CheckRunInfo};
use crate::config::Config;
use crate::engine::{build_parent_candidates, Stack};
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

const CI_ACTIVE_REFRESH_INTERVAL: Duration = Duration::from_secs(15);
const CI_IDLE_REFRESH_INTERVAL: Duration = Duration::from_secs(120);
const CI_ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// A line in a diff with its type
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub content: String,
    pub line_type: DiffLineType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiffLineType {
    Header,
    Addition,
    Deletion,
    Context,
    Hunk,
}

/// A line in diff stat output
#[derive(Debug, Clone)]
pub struct DiffStatLine {
    pub file: String,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone)]
struct CachedDiff {
    stat: Vec<DiffStatLine>,
    lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffRequest {
    branch: String,
    parent: String,
    key: String,
}

impl DiffRequest {
    fn new(branch: String, parent: String) -> Self {
        let key = format!("{}...{}", parent, branch);
        Self {
            branch,
            parent,
            key,
        }
    }
}

#[derive(Debug)]
enum DiffUpdate {
    Loaded {
        request: DiffRequest,
        diff: CachedDiff,
    },
    Unavailable {
        request: DiffRequest,
    },
}

#[derive(Debug, Clone)]
struct BranchDetails {
    ahead: usize,
    behind: usize,
    has_remote: bool,
    unpushed: usize,
    unpulled: usize,
    commits: Vec<String>,
}

#[derive(Debug, Clone)]
struct BranchDetailsRequest {
    name: String,
    parent: Option<String>,
}

#[derive(Debug)]
enum BranchDetailsUpdate {
    Loaded {
        branch: String,
        details: BranchDetails,
    },
    Unavailable {
        branch: String,
    },
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchCiSummary {
    pub overall_status: Option<String>,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub running: usize,
    pub queued: usize,
    pub skipped: usize,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub average_secs: Option<u64>,
}

impl BranchCiSummary {
    fn from_checks(
        overall_status: Option<String>,
        checks: &[CheckRunInfo],
        average_secs: Option<u64>,
    ) -> Self {
        let mut passed = 0;
        let mut failed = 0;
        let mut running = 0;
        let mut queued = 0;
        let mut skipped = 0;

        for check in checks {
            match check.status.as_str() {
                "completed" => match check.conclusion.as_deref() {
                    Some("success") => passed += 1,
                    Some("skipped") | Some("neutral") | Some("cancelled") => skipped += 1,
                    _ => failed += 1,
                },
                "in_progress" => running += 1,
                "queued" | "waiting" | "requested" | "pending" => queued += 1,
                _ => queued += 1,
            }
        }

        let started_at = checks
            .iter()
            .filter_map(|check| parse_ci_timestamp(check.started_at.as_deref()))
            .min();
        let completed_at = if checks.iter().all(|check| check.status == "completed") {
            checks
                .iter()
                .filter_map(|check| parse_ci_timestamp(check.completed_at.as_deref()))
                .max()
        } else {
            None
        };

        Self {
            overall_status,
            total: checks.len(),
            passed,
            failed,
            running,
            queued,
            skipped,
            started_at,
            completed_at,
            average_secs,
        }
    }

    pub fn has_checks(&self) -> bool {
        self.total > 0
    }

    pub fn is_active(&self) -> bool {
        !self.is_complete() && (self.running > 0 || self.queued > 0)
    }

    pub fn is_complete(&self) -> bool {
        self.total > 0 && self.completed_count() == self.total
    }

    pub fn completed_count(&self) -> usize {
        self.passed + self.failed + self.skipped
    }

    pub fn elapsed_secs(&self, now: DateTime<Utc>) -> Option<u64> {
        let started_at = self.started_at?;
        let finished_at = if self.is_complete() {
            self.completed_at.unwrap_or(now)
        } else {
            now
        };
        Some(
            finished_at
                .signed_duration_since(started_at)
                .num_seconds()
                .max(0) as u64,
        )
    }

    pub fn progress_percent(&self, now: DateTime<Utc>) -> Option<u8> {
        if self.is_complete() {
            return Some(100);
        }

        let average_secs = self.average_secs?;
        let elapsed_secs = self.elapsed_secs(now)?;
        if average_secs == 0 {
            return Some(99);
        }

        Some(if elapsed_secs >= average_secs {
            99
        } else {
            ((elapsed_secs * 100) / average_secs).min(99) as u8
        })
    }

    pub fn eta_secs(&self, now: DateTime<Utc>) -> Option<u64> {
        if self.is_complete() {
            return Some(0);
        }

        let average_secs = self.average_secs?;
        let elapsed_secs = self.elapsed_secs(now)?;
        Some(average_secs.saturating_sub(elapsed_secs))
    }
}

#[derive(Debug, Clone)]
pub enum BranchCiState {
    Loading,
    Ready {
        summary: BranchCiSummary,
        fetched_at: Instant,
    },
    Unavailable {
        message: String,
        fetched_at: Instant,
    },
}

#[derive(Debug)]
enum CiUpdate {
    Loaded {
        branch: String,
        summary: BranchCiSummary,
    },
    Unavailable {
        branch: String,
        message: String,
    },
}

/// Branch display information for the TUI
#[derive(Debug, Clone)]
pub struct BranchDisplay {
    pub name: String,
    pub parent: Option<String>,
    pub column: usize,
    pub is_current: bool,
    pub is_trunk: bool,
    pub ahead: usize,  // commits ahead of parent
    pub behind: usize, // commits behind parent
    pub needs_restack: bool,
    pub has_remote: bool,
    pub unpushed: usize, // commits ahead of remote (unpushed)
    pub unpulled: usize, // commits behind remote (unpulled)
    pub pr_number: Option<u64>,
    pub pr_state: Option<String>,
    pub ci_state: Option<String>,
    pub commits: Vec<String>,
    pub details_loaded: bool,
}

impl BranchDisplay {
    fn apply_details(&mut self, details: BranchDetails) {
        self.ahead = details.ahead;
        self.behind = details.behind;
        self.has_remote = details.has_remote;
        self.unpushed = details.unpushed;
        self.unpulled = details.unpulled;
        self.commits = details.commits;
        self.details_loaded = true;
    }
}

/// Which pane is focused
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FocusedPane {
    #[default]
    Stack,
    Diff,
}

/// Application mode
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    Search,
    Help,
    Confirm(ConfirmAction),
    Input(InputAction),
    Reorder,
    /// Fuzzy-filter picker for selecting a new parent to reparent onto
    /// (`gt move` equivalent). Parallel to `Search` — chars type into the
    /// query, Enter confirms, Esc cancels.
    MovePicker,
}

/// Actions that require text input
#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    Rename,
    NewBranch,
}

/// Actions that require confirmation
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    Delete(String),
    Restack(String),
    RestackAll,
    ApplyReorder,
}

/// Information about a potential conflict
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictInfo {
    pub file: String,
    pub branches_involved: Vec<String>,
}

/// Preview of what will happen during restack
#[derive(Debug, Clone, Default)]
pub struct ReorderPreview {
    /// branch name -> list of commit messages
    pub commits_to_rebase: Vec<(String, Vec<String>)>,
    /// potential conflicts detected
    pub potential_conflicts: Vec<ConflictInfo>,
}

/// Represents a branch and its parent in the stack chain
#[derive(Debug, Clone, PartialEq)]
pub struct StackChainEntry {
    pub name: String,
    pub parent: String,
}

/// State for reorder mode - reordering branches within a linear stack
#[derive(Debug, Clone)]
pub struct ReorderState {
    /// Original stack chain order (from trunk down) - list of (branch, parent) pairs
    pub original_chain: Vec<StackChainEntry>,
    /// New proposed chain order after reordering
    pub pending_chain: Vec<StackChainEntry>,
    /// Index of the branch being moved within the chain (0 = first branch after trunk)
    pub moving_index: usize,
    /// Computed preview of restack impact
    pub preview: ReorderPreview,
}

#[derive(Debug, Clone)]
pub struct PendingCommand {
    pub commands: Vec<Vec<String>>,
    pub success_message: String,
    pub preferred_selection: Option<String>,
}

/// Main application state
pub struct App {
    pub stack: Stack,
    pub cache: CiCache,
    pub repo: GitRepo,
    git_dir: PathBuf,
    diff_cache_dir: PathBuf,
    pub current_branch: String,
    pub selected_index: usize,
    pub branches: Vec<BranchDisplay>,
    pub mode: Mode,
    pub search_query: String,
    pub filtered_indices: Vec<usize>,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub selected_diff: Vec<DiffLine>,
    pub diff_scroll: usize,
    pub focused_pane: FocusedPane,
    pub diff_stat: Vec<DiffStatLine>,
    pub status_message: Option<String>,
    pub status_set_at: Option<Instant>,
    pub should_quit: bool,
    pub pending_command: Option<PendingCommand>,
    pub needs_refresh: bool,
    pub reorder_state: Option<ReorderState>,
    /// Branch being reparented (snapshot taken when MovePicker opens so the
    /// picker survives UI refreshes that change `selected_index`).
    pub move_picker_source: String,
    /// All eligible parent branches for the move (current + descendants
    /// already excluded). Never changes while the picker is open.
    pub move_picker_candidates: Vec<String>,
    /// User's substring query (case-insensitive match against candidates).
    pub move_picker_query: String,
    /// Index into the filtered view (see `move_picker_filtered_indices`).
    pub move_picker_selected: usize,
    diff_cache: HashMap<String, CachedDiff>,
    ci_states: HashMap<String, BranchCiState>,
    ci_loader: Option<Receiver<CiUpdate>>,
    ci_loading_branch: Option<String>,
    ci_queued_branch: Option<String>,
    branch_details_queued: bool,
    branch_details_loader: Option<Receiver<BranchDetailsUpdate>>,
    diff_loader: Option<Receiver<DiffUpdate>>,
    diff_loading: Option<DiffRequest>,
    diff_queued: Option<DiffRequest>,
}

impl App {
    pub fn new(
        initial_status: Option<String>,
        preferred_selection: Option<String>,
    ) -> Result<Self> {
        let repo = GitRepo::open()?;
        let stack = Stack::load(&repo)?;
        let current_branch = repo.current_branch()?;
        let git_dir = repo.git_dir()?;
        let cache = CiCache::load(git_dir);
        let diff_cache_dir = repo.common_git_dir()?;
        let git_dir = git_dir.to_path_buf();
        let status_set_at = initial_status.as_ref().map(|_| Instant::now());

        let mut app = Self {
            stack,
            cache,
            repo,
            git_dir,
            diff_cache_dir,
            current_branch,
            selected_index: 0,
            branches: Vec::new(),
            mode: Mode::Normal,
            search_query: String::new(),
            filtered_indices: Vec::new(),
            input_buffer: String::new(),
            input_cursor: 0,
            selected_diff: Vec::new(),
            diff_scroll: 0,
            focused_pane: FocusedPane::Stack,
            diff_stat: Vec::new(),
            status_message: initial_status,
            status_set_at,
            should_quit: false,
            pending_command: None,
            needs_refresh: true,
            reorder_state: None,
            move_picker_source: String::new(),
            move_picker_candidates: Vec::new(),
            move_picker_query: String::new(),
            move_picker_selected: 0,
            diff_cache: HashMap::new(),
            ci_states: HashMap::new(),
            ci_loader: None,
            ci_loading_branch: None,
            ci_queued_branch: None,
            branch_details_queued: false,
            branch_details_loader: None,
            diff_loader: None,
            diff_loading: None,
            diff_queued: None,
        };

        app.refresh_branches()?;
        if let Some(branch) = preferred_selection {
            app.select_branch(&branch);
        } else {
            app.select_current_branch();
        }
        app.queue_diff_refresh_for_selected();
        app.queue_ci_refresh_for_selected();

        Ok(app)
    }

    /// Refresh the branch list from the repository
    pub fn refresh_branches(&mut self) -> Result<()> {
        self.stack = Stack::load(&self.repo)?;
        self.current_branch = self.repo.current_branch()?;
        self.branches = self.build_branch_list()?;
        self.diff_cache.clear();
        self.needs_refresh = false;
        self.branch_details_queued = true;
        self.queue_diff_refresh_for_selected();
        self.queue_ci_refresh_for_selected();
        Ok(())
    }

    /// Build the ordered list of branches for display
    fn build_branch_list(&self) -> Result<Vec<BranchDisplay>> {
        let mut branches = Vec::new();
        let trunk = &self.stack.trunk;

        // Get trunk children (each starts a chain)
        let trunk_info = self.stack.branches.get(trunk);
        let trunk_children: Vec<String> =
            trunk_info.map(|b| b.children.clone()).unwrap_or_default();

        if trunk_children.is_empty() {
            // Only trunk exists
            branches.push(self.create_branch_display(trunk, 0, true));
            return Ok(branches);
        }

        let mut max_column = 0;
        let mut sorted_trunk_children = trunk_children;
        sorted_trunk_children.sort();

        // Build each stack
        for (i, root) in sorted_trunk_children.iter().enumerate() {
            self.collect_branches(&mut branches, root, i, &mut max_column)?;
        }

        // Add trunk at the end
        branches.push(self.create_branch_display(trunk, 0, true));

        Ok(branches)
    }

    fn collect_branches(
        &self,
        result: &mut Vec<BranchDisplay>,
        branch: &str,
        base_column: usize,
        max_column: &mut usize,
    ) -> Result<()> {
        #[derive(Clone)]
        struct Frame {
            branch: String,
            column: usize,
            expanded: bool,
        }

        let mut stack_frames = vec![Frame {
            branch: branch.to_string(),
            column: base_column,
            expanded: false,
        }];
        let mut visiting = std::collections::HashSet::new();
        let mut emitted = std::collections::HashSet::new();

        while let Some(frame) = stack_frames.pop() {
            if frame.expanded {
                visiting.remove(&frame.branch);
                if emitted.insert(frame.branch.clone()) {
                    result.push(self.create_branch_display(&frame.branch, frame.column, false));
                }
                continue;
            }

            if emitted.contains(&frame.branch) || !visiting.insert(frame.branch.clone()) {
                continue;
            }

            *max_column = (*max_column).max(frame.column);
            stack_frames.push(Frame {
                branch: frame.branch.clone(),
                column: frame.column,
                expanded: true,
            });

            if let Some(info) = self.stack.branches.get(&frame.branch) {
                let mut children: Vec<&String> = info.children.iter().collect();
                children.sort();

                for (i, child) in children.into_iter().enumerate().rev() {
                    if emitted.contains(child) || visiting.contains(child) {
                        continue;
                    }

                    stack_frames.push(Frame {
                        branch: child.clone(),
                        column: frame.column + i,
                        expanded: false,
                    });
                }
            }
        }

        Ok(())
    }

    fn create_branch_display(&self, branch: &str, column: usize, is_trunk: bool) -> BranchDisplay {
        let is_current = branch == self.current_branch;
        let info = self.stack.branches.get(branch);
        let needs_restack = info.map(|i| i.needs_restack).unwrap_or(false);
        let pr_number = info.and_then(|i| i.pr_number);
        let pr_state = info.and_then(|i| i.pr_state.clone());
        let parent = info.and_then(|i| i.parent.clone());
        let ci_state = self.cache.get_ci_state(branch);

        BranchDisplay {
            name: branch.to_string(),
            parent,
            column,
            is_current,
            is_trunk,
            ahead: 0,
            behind: 0,
            needs_restack,
            has_remote: false,
            unpushed: 0,
            unpulled: 0,
            pr_number,
            pr_state,
            ci_state,
            commits: Vec::new(),
            details_loaded: is_trunk,
        }
    }

    /// Select the current branch in the list
    pub fn select_current_branch(&mut self) {
        if let Some(idx) = self.branches.iter().position(|b| b.is_current) {
            self.selected_index = idx;
        }
        self.queue_diff_refresh_for_selected();
        self.queue_ci_refresh_for_selected();
    }

    /// Select a branch by name, falling back to current branch when not found.
    pub fn select_branch(&mut self, branch: &str) {
        if let Some(idx) = self.branches.iter().position(|b| b.name == branch) {
            self.selected_index = idx;
            self.queue_diff_refresh_for_selected();
            self.queue_ci_refresh_for_selected();
            return;
        }

        self.select_current_branch();
        self.queue_diff_refresh_for_selected();
    }

    /// Get the currently selected branch
    pub fn selected_branch(&self) -> Option<&BranchDisplay> {
        if self.mode == Mode::Search {
            self.filtered_indices
                .get(self.selected_index)
                .and_then(|&idx| self.branches.get(idx))
        } else {
            self.branches.get(self.selected_index)
        }
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        let len = if self.mode == Mode::Search {
            self.filtered_indices.len()
        } else {
            self.branches.len()
        };

        if len > 0 && self.selected_index > 0 {
            self.selected_index -= 1;
            self.queue_diff_refresh_for_selected();
            self.queue_ci_refresh_for_selected();
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        let len = if self.mode == Mode::Search {
            self.filtered_indices.len()
        } else {
            self.branches.len()
        };

        if len > 0 && self.selected_index < len - 1 {
            self.selected_index += 1;
            self.queue_diff_refresh_for_selected();
            self.queue_ci_refresh_for_selected();
        }
    }

    /// Update search filter
    pub fn update_search(&mut self) {
        let query = self.search_query.to_lowercase();
        self.filtered_indices = self
            .branches
            .iter()
            .enumerate()
            .filter(|(_, b)| b.name.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect();
        self.selected_index = 0;
    }

    /// Prepare state for `Mode::MovePicker` and enter it.
    ///
    /// On `Err`, the static string is a user-facing reason suitable for
    /// `set_status`. Returning a specific message from here (rather than a
    /// bool) keeps the dispatcher from having to duplicate the trunk /
    /// no-candidates checks to figure out what to show.
    ///
    /// Candidates are sourced from `repo.list_branches()` — the same
    /// universe the CLI `pick_parent_interactively` uses — so both
    /// surfaces offer the same targets, including local branches that
    /// aren't yet tracked in the stax stack.
    pub fn init_move_picker(&mut self) -> Result<(), &'static str> {
        let Some(source) = self.selected_branch() else {
            return Err("No branch selected");
        };
        if source.is_trunk {
            return Err("Cannot reparent trunk branch");
        }
        let source_name = source.name.clone();

        let all_names = self
            .repo
            .list_branches()
            .map_err(|_| "Failed to list branches")?;
        let descendants = self.stack.descendants(&source_name);
        let candidates =
            build_parent_candidates(&all_names, &source_name, &descendants, &self.stack.trunk);
        if candidates.is_empty() {
            return Err("No eligible parents to move onto");
        }

        self.move_picker_source = source_name;
        self.move_picker_candidates = candidates;
        self.move_picker_query.clear();
        self.move_picker_selected = 0;
        self.mode = Mode::MovePicker;
        Ok(())
    }

    /// Return the filtered subset of candidates as indices into
    /// `move_picker_candidates`. Case-insensitive substring match, same
    /// shape as `update_search` for the main stack view.
    pub fn move_picker_filtered_indices(&self) -> Vec<usize> {
        substring_filter_indices(&self.move_picker_candidates, &self.move_picker_query)
    }

    /// Currently highlighted candidate (after applying the filter).
    pub fn move_picker_current(&self) -> Option<&str> {
        let filtered = self.move_picker_filtered_indices();
        let idx = *filtered.get(self.move_picker_selected)?;
        self.move_picker_candidates.get(idx).map(String::as_str)
    }

    /// Move highlight up in the filtered view; clamps at 0.
    pub fn move_picker_select_previous(&mut self) {
        if self.move_picker_selected > 0 {
            self.move_picker_selected -= 1;
        }
    }

    /// Move highlight down in the filtered view; clamps at the last item.
    pub fn move_picker_select_next(&mut self) {
        let len = self.move_picker_filtered_indices().len();
        if len > 0 && self.move_picker_selected + 1 < len {
            self.move_picker_selected += 1;
        }
    }

    /// After the query changes: reset the highlight to the first match so
    /// the user doesn't land out-of-bounds when the filter shrinks.
    pub fn move_picker_on_query_change(&mut self) {
        self.move_picker_selected = 0;
    }

    /// Clear all picker state. Call when exiting the mode by any path.
    pub fn clear_move_picker(&mut self) {
        self.move_picker_source.clear();
        self.move_picker_candidates.clear();
        self.move_picker_query.clear();
        self.move_picker_selected = 0;
    }

    /// Queue a background diff refresh for the currently selected branch.
    pub fn queue_diff_refresh_for_selected(&mut self) {
        self.selected_diff.clear();
        self.diff_stat.clear();
        self.diff_scroll = 0;

        let (branch_name, parent_name) = match self.selected_branch() {
            Some(branch) => match &branch.parent {
                Some(parent) => (branch.name.clone(), parent.clone()),
                None => return,
            },
            None => return,
        };

        let request = DiffRequest::new(branch_name, parent_name);
        if let Some(cached) = self.diff_cache.get(&request.key) {
            self.diff_stat = cached.stat.clone();
            self.selected_diff = cached.lines.clone();
            return;
        }

        if let Some(cached) = self.load_persisted_diff(&request) {
            self.diff_cache.insert(request.key.clone(), cached.clone());
            self.diff_stat = cached.stat;
            self.selected_diff = cached.lines;
            return;
        }

        if self.diff_loading.as_ref() == Some(&request) {
            return;
        }

        self.diff_queued = Some(request);
    }

    pub fn is_selected_diff_loading(&self) -> bool {
        let Some(request) = self.selected_diff_request() else {
            return false;
        };

        self.diff_queued.as_ref() == Some(&request) || self.diff_loading.as_ref() == Some(&request)
    }

    fn selected_diff_request(&self) -> Option<DiffRequest> {
        let branch = self.selected_branch()?;
        let parent = branch.parent.as_ref()?;
        Some(DiffRequest::new(branch.name.clone(), parent.clone()))
    }

    /// Calculate total scrollable lines in diff view (stats header + diff content)
    pub fn total_diff_lines(&self) -> usize {
        self.selected_diff.len()
    }

    /// Set a status message (auto-clears after timeout)
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_set_at = Some(Instant::now());
    }

    pub fn queue_command(
        &mut self,
        commands: Vec<Vec<String>>,
        success_message: impl Into<String>,
        preferred_selection: Option<String>,
    ) {
        self.pending_command = Some(PendingCommand {
            commands,
            success_message: success_message.into(),
            preferred_selection,
        });
        self.should_quit = true;
    }

    /// Clear status message if it's been shown long enough
    pub fn clear_stale_status(&mut self) {
        if let Some(set_at) = self.status_set_at {
            if set_at.elapsed().as_secs() >= 2 {
                self.status_message = None;
                self.status_set_at = None;
            }
        }
    }

    pub fn refresh_background(&mut self) {
        self.poll_branch_details_updates();
        self.poll_diff_updates();
        self.poll_ci_updates();
        self.spawn_branch_details_loader_if_needed();
        self.queue_ci_refresh_for_selected();
        self.spawn_diff_loader_if_needed();
        self.spawn_ci_loader_if_needed();
    }

    pub fn ci_row_progress(&self, branch: &str) -> Option<String> {
        let summary = match self.ci_states.get(branch) {
            Some(BranchCiState::Ready { summary, .. }) if summary.is_active() => summary,
            _ => return None,
        };

        Some(format!("{}/{}", summary.completed_count(), summary.total))
    }

    pub fn ci_summary_line(&self, branch: &BranchDisplay) -> (String, bool) {
        match self.ci_states.get(&branch.name) {
            Some(BranchCiState::Loading) => ("Live CI: fetching latest checks…".to_string(), false),
            Some(BranchCiState::Unavailable { message, .. }) => {
                (format!("Live CI: {}", message), true)
            }
            Some(BranchCiState::Ready { summary, .. }) => live_ci_summary_text(summary),
            None if self.ci_queued_branch.as_deref() == Some(branch.name.as_str()) => {
                ("Live CI: fetching latest checks…".to_string(), false)
            }
            None if branch.has_remote => (
                "Live CI: select the branch for a background refresh".to_string(),
                false,
            ),
            None => (
                "Live CI: push branch to see remote checks".to_string(),
                true,
            ),
        }
    }

    fn queue_ci_refresh_for_selected(&mut self) {
        let Some(branch) = self.selected_branch().map(|branch| branch.name.clone()) else {
            return;
        };

        let Some(selected) = self.selected_branch() else {
            return;
        };

        if !selected.details_loaded {
            return;
        }

        if !selected.has_remote {
            self.ci_states.insert(
                branch,
                BranchCiState::Unavailable {
                    message: "push branch to see remote checks".to_string(),
                    fetched_at: Instant::now(),
                },
            );
            return;
        }

        let should_queue = match self.ci_states.get(&branch) {
            Some(BranchCiState::Loading) => false,
            Some(BranchCiState::Ready {
                summary,
                fetched_at,
            }) => {
                let interval = if summary.is_active() {
                    CI_ACTIVE_REFRESH_INTERVAL
                } else {
                    CI_IDLE_REFRESH_INTERVAL
                };
                fetched_at.elapsed() >= interval
            }
            Some(BranchCiState::Unavailable { fetched_at, .. }) => {
                fetched_at.elapsed() >= CI_ERROR_RETRY_INTERVAL
            }
            None => true,
        };

        if should_queue {
            self.ci_queued_branch = Some(branch);
        }
    }

    fn spawn_branch_details_loader_if_needed(&mut self) {
        if !self.branch_details_queued || self.branch_details_loader.is_some() {
            return;
        }

        let requests = self
            .branches
            .iter()
            .filter(|branch| !branch.is_trunk)
            .map(|branch| BranchDetailsRequest {
                name: branch.name.clone(),
                parent: branch.parent.clone(),
            })
            .collect::<Vec<_>>();

        self.branch_details_queued = false;
        self.branch_details_loader = (!requests.is_empty())
            .then(|| spawn_branch_details_loader(self.git_dir.clone(), requests));
    }

    fn poll_branch_details_updates(&mut self) {
        loop {
            let update = match self.branch_details_loader.as_ref() {
                Some(loader) => match loader.try_recv() {
                    Ok(update) => Some(update),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        self.branch_details_loader = None;
                        None
                    }
                },
                None => None,
            };

            let Some(update) = update else {
                break;
            };
            self.apply_branch_details_update(update);
        }
    }

    fn apply_branch_details_update(&mut self, update: BranchDetailsUpdate) {
        match update {
            BranchDetailsUpdate::Loaded { branch, details } => {
                if let Some(branch_display) =
                    self.branches.iter_mut().find(|item| item.name == branch)
                {
                    branch_display.apply_details(details);
                }
            }
            BranchDetailsUpdate::Unavailable { branch } => {
                if let Some(branch_display) =
                    self.branches.iter_mut().find(|item| item.name == branch)
                {
                    branch_display.details_loaded = true;
                }
            }
            BranchDetailsUpdate::Done => {
                self.branch_details_loader = None;
            }
        }
    }

    fn poll_diff_updates(&mut self) {
        loop {
            let update = match self.diff_loader.as_ref() {
                Some(loader) => match loader.try_recv() {
                    Ok(update) => Some(update),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        self.diff_loader = None;
                        self.diff_loading = None;
                        None
                    }
                },
                None => None,
            };

            let Some(update) = update else {
                break;
            };
            self.apply_diff_update(update);
        }
    }

    fn spawn_diff_loader_if_needed(&mut self) {
        if self.diff_loader.is_some() {
            return;
        }

        let Some(request) = self.diff_queued.take() else {
            return;
        };

        self.diff_loading = Some(request.clone());
        self.diff_loader = Some(spawn_diff_loader(
            self.git_dir.clone(),
            self.diff_cache_dir.clone(),
            request,
        ));
    }

    fn load_persisted_diff(&self, request: &DiffRequest) -> Option<CachedDiff> {
        let key = persistent_diff_cache_key(&self.repo, request)?;
        let cache = TuiDiffCache::load(&self.diff_cache_dir);
        cache.get(&key).map(cached_diff_from_disk)
    }

    fn apply_diff_update(&mut self, update: DiffUpdate) {
        let request = match update {
            DiffUpdate::Loaded { request, diff } => {
                self.diff_cache.insert(request.key.clone(), diff.clone());
                if self.selected_diff_request().as_ref() == Some(&request) {
                    self.diff_stat = diff.stat;
                    self.selected_diff = diff.lines;
                    self.diff_scroll = 0;
                }
                request
            }
            DiffUpdate::Unavailable { request } => request,
        };

        if self.diff_loading.as_ref() == Some(&request) {
            self.diff_loader = None;
            self.diff_loading = None;
        }
    }

    fn poll_ci_updates(&mut self) {
        loop {
            let update = match self.ci_loader.as_ref() {
                Some(loader) => match loader.try_recv() {
                    Ok(update) => Some(update),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        self.ci_loader = None;
                        self.ci_loading_branch = None;
                        None
                    }
                },
                None => None,
            };

            let Some(update) = update else {
                break;
            };
            self.apply_ci_update(update);
        }
    }

    fn spawn_ci_loader_if_needed(&mut self) {
        if self.ci_loader.is_some() {
            return;
        }

        let Some(branch) = self.ci_queued_branch.take() else {
            return;
        };

        self.ci_states
            .insert(branch.clone(), BranchCiState::Loading);
        self.ci_loading_branch = Some(branch.clone());
        self.ci_loader = Some(spawn_ci_loader(self.git_dir.clone(), branch));
    }

    fn apply_ci_update(&mut self, update: CiUpdate) {
        let fetched_at = Instant::now();
        let branch_name = match &update {
            CiUpdate::Loaded { branch, .. } | CiUpdate::Unavailable { branch, .. } => {
                branch.clone()
            }
        };

        match update {
            CiUpdate::Loaded { branch, summary } => {
                let ci_state = summary.overall_status.clone();
                self.ci_states.insert(
                    branch.clone(),
                    BranchCiState::Ready {
                        summary,
                        fetched_at,
                    },
                );
                if let Some(branch_display) =
                    self.branches.iter_mut().find(|item| item.name == branch)
                {
                    branch_display.ci_state = ci_state.clone();
                }
                self.cache.update(&branch, ci_state, None);
                let _ = self.cache.save(&self.git_dir);
            }
            CiUpdate::Unavailable { branch, message } => {
                self.ci_states.insert(
                    branch.clone(),
                    BranchCiState::Unavailable {
                        message,
                        fetched_at,
                    },
                );
            }
        }

        if self.ci_loading_branch.as_deref() == Some(branch_name.as_str()) {
            self.ci_loader = None;
            self.ci_loading_branch = None;
        }
    }

    /// Initialize reorder mode for the selected branch
    /// Gets the linear stack chain containing the selected branch
    pub fn init_reorder_state(&mut self) -> bool {
        let branch = match self.selected_branch() {
            Some(b) => b.clone(),
            None => return false,
        };

        // Cannot reorder trunk
        if branch.is_trunk {
            self.set_status("Cannot reorder trunk branch");
            return false;
        }

        // Build the linear stack chain from trunk to the deepest descendant
        // that contains our selected branch
        let chain = self.build_stack_chain(&branch.name);

        if chain.len() < 2 {
            self.set_status("Stack too small to reorder");
            return false;
        }

        // Find the index of the selected branch in the chain
        let moving_index = match chain.iter().position(|e| e.name == branch.name) {
            Some(idx) => idx,
            None => {
                self.set_status("Branch not found in stack chain");
                return false;
            }
        };

        self.reorder_state = Some(ReorderState {
            original_chain: chain.clone(),
            pending_chain: chain,
            moving_index,
            preview: ReorderPreview::default(),
        });

        self.update_reorder_preview();
        true
    }

    /// Build a linear stack chain containing the given branch
    /// Returns entries from first branch after trunk down to the leaf
    fn build_stack_chain(&self, branch_name: &str) -> Vec<StackChainEntry> {
        // First, find the root of this stack (direct child of trunk)
        let mut ancestors = vec![branch_name.to_string()];
        let mut current = branch_name.to_string();

        while let Some(info) = self.stack.branches.get(&current) {
            if let Some(parent) = &info.parent {
                if *parent == self.stack.trunk {
                    break; // We've reached trunk
                }
                ancestors.push(parent.clone());
                current = parent.clone();
            } else {
                break;
            }
        }

        // ancestors now contains [branch, ..., stack_root] - reverse it
        ancestors.reverse();

        // Now build the full chain from stack_root down through the selected branch
        // and continue to any single-child descendants
        let mut chain = Vec::new();

        // Add all ancestors including the selected branch
        let mut prev_parent = self.stack.trunk.clone();
        for ancestor in &ancestors {
            chain.push(StackChainEntry {
                name: ancestor.clone(),
                parent: prev_parent.clone(),
            });
            prev_parent = ancestor.clone();
        }

        // Continue down to descendants (only if linear - single child)
        let mut current = branch_name.to_string();
        while let Some(info) = self.stack.branches.get(&current) {
            if info.children.len() == 1 {
                let child = &info.children[0];
                chain.push(StackChainEntry {
                    name: child.clone(),
                    parent: current.clone(),
                });
                current = child.clone();
            } else {
                break; // Stop at branches with multiple children or no children
            }
        }

        chain
    }

    /// Move the selected branch up in the stack (becomes earlier in the chain)
    pub fn reorder_move_up(&mut self) {
        if let Some(ref mut state) = self.reorder_state {
            if state.moving_index > 0 {
                // Swap positions: branch at moving_index moves up
                let i = state.moving_index;

                // Get the parent of the branch we're swapping with
                let new_parent = state.pending_chain[i - 1].parent.clone();
                let moving_branch = state.pending_chain[i].name.clone();
                let displaced_branch = state.pending_chain[i - 1].name.clone();

                // Update parents for the swap
                state.pending_chain[i - 1].parent = moving_branch.clone();
                state.pending_chain[i].parent = new_parent;

                // Update parent of branch after the displaced one (if any)
                if i + 1 < state.pending_chain.len() {
                    state.pending_chain[i + 1].parent = displaced_branch.clone();
                }

                // Swap the entries
                state.pending_chain.swap(i, i - 1);
                state.moving_index -= 1;

                self.update_reorder_preview();
            }
        }
    }

    /// Move the selected branch down in the stack (becomes later in the chain)
    pub fn reorder_move_down(&mut self) {
        if let Some(ref mut state) = self.reorder_state {
            if state.moving_index < state.pending_chain.len() - 1 {
                // Swap positions: branch at moving_index moves down
                let i = state.moving_index;

                // Get info for the swap
                let moving_branch = state.pending_chain[i].name.clone();
                let displaced_branch = state.pending_chain[i + 1].name.clone();
                let moving_parent = state.pending_chain[i].parent.clone();

                // Update parents for the swap
                state.pending_chain[i].parent = displaced_branch.clone();
                state.pending_chain[i + 1].parent = moving_parent;

                // Update parent of branch after the moving one (if any)
                if i + 2 < state.pending_chain.len() {
                    state.pending_chain[i + 2].parent = moving_branch.clone();
                }

                // Swap the entries
                state.pending_chain.swap(i, i + 1);
                state.moving_index += 1;

                self.update_reorder_preview();
            }
        }
    }

    /// Check if reorder has pending changes
    pub fn reorder_has_changes(&self) -> bool {
        self.reorder_state
            .as_ref()
            .map(|s| s.original_chain != s.pending_chain)
            .unwrap_or(false)
    }

    /// Get the reparent operations needed to apply the reorder
    pub fn get_reparent_operations(&self) -> Vec<(String, String)> {
        let state = match &self.reorder_state {
            Some(s) => s,
            None => return Vec::new(),
        };

        let mut ops = Vec::new();

        // Compare original and pending chains to find what needs reparenting
        for pending in &state.pending_chain {
            // Find this branch in the original chain
            if let Some(original) = state.original_chain.iter().find(|e| e.name == pending.name) {
                if original.parent != pending.parent {
                    ops.push((pending.name.clone(), pending.parent.clone()));
                }
            }
        }

        ops
    }

    /// Update the preview for reorder mode
    pub fn update_reorder_preview(&mut self) {
        let state = match &self.reorder_state {
            Some(s) => s.clone(),
            None => return,
        };

        let mut commits_to_rebase = Vec::new();
        let mut potential_conflicts = Vec::new();

        // For each branch that needs reparenting, show its commits
        for entry in &state.pending_chain {
            // Find original parent
            let original_parent = state
                .original_chain
                .iter()
                .find(|e| e.name == entry.name)
                .map(|e| e.parent.clone());

            // If parent changed, this branch needs rebasing
            if original_parent.as_ref() != Some(&entry.parent) {
                // Get commits that will be rebased (using current parent)
                if let Some(orig_parent) = &original_parent {
                    let commits = self
                        .repo
                        .commits_between(orig_parent, &entry.name)
                        .unwrap_or_default();

                    if !commits.is_empty() {
                        commits_to_rebase.push((entry.name.clone(), commits));
                    }

                    // Check for potential conflicts with new parent
                    if let Ok(conflict_files) =
                        self.repo.check_rebase_conflicts(&entry.name, &entry.parent)
                    {
                        for file in conflict_files {
                            potential_conflicts.push(ConflictInfo {
                                file,
                                branches_involved: vec![entry.name.clone(), entry.parent.clone()],
                            });
                        }
                    }
                }
            }
        }

        if let Some(ref mut reorder_state) = self.reorder_state {
            reorder_state.preview = ReorderPreview {
                commits_to_rebase,
                potential_conflicts,
            };
        }
    }

    /// Clear reorder state
    pub fn clear_reorder_state(&mut self) {
        self.reorder_state = None;
    }
}

/// Case-insensitive substring filter. Returns the indices of `candidates`
/// that match `query`. Empty query returns every index in original order.
///
/// Extracted as a pure function so the filter behaviour is unit-testable
/// without spinning up a full `App` (which needs a live `GitRepo`).
fn substring_filter_indices(candidates: &[String], query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return (0..candidates.len()).collect();
    }
    candidates
        .iter()
        .enumerate()
        .filter(|(_, n)| n.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect()
}

fn parse_ci_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    value.and_then(|timestamp| timestamp.parse::<DateTime<Utc>>().ok())
}

fn live_ci_summary_text(summary: &BranchCiSummary) -> (String, bool) {
    if !summary.has_checks() {
        return (
            "Live CI: no checks reported for the latest push".to_string(),
            true,
        );
    }

    let now = Utc::now();
    let mut parts = Vec::new();

    if summary.is_active() {
        parts.push(format!(
            "{}/{} complete",
            summary.completed_count(),
            summary.total
        ));
    } else {
        parts.push(format!("{} checks", summary.total));
    }

    if summary.failed > 0 {
        parts.push(format!("{} failed", summary.failed));
    }
    if summary.running > 0 {
        parts.push(format!("{} running", summary.running));
    }
    if summary.queued > 0 {
        parts.push(format!("{} queued", summary.queued));
    }
    if summary.passed > 0 {
        parts.push(format!("{} passed", summary.passed));
    }
    if summary.skipped > 0 {
        parts.push(format!("{} skipped", summary.skipped));
    }
    if let Some(percent) = summary.progress_percent(now) {
        if summary.is_active() {
            parts.push(format!("{}%", percent));
        }
    }
    if let Some(elapsed_secs) = summary.elapsed_secs(now) {
        if summary.is_complete() {
            parts.push(format!("{} total", format_duration_compact(elapsed_secs)));
        } else {
            parts.push(format!("{} elapsed", format_duration_compact(elapsed_secs)));
        }
    }
    if let Some(eta_secs) = summary.eta_secs(now) {
        if summary.is_active() && eta_secs > 0 {
            parts.push(format!("~{} left", format_duration_compact(eta_secs)));
        }
    }

    let is_dimmed = summary.failed == 0 && !summary.is_active();
    (format!("Live CI: {}", parts.join("  •  ")), is_dimmed)
}

fn format_duration_compact(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn branch_average_secs(repo: &GitRepo, checks: &[CheckRunInfo]) -> Option<u64> {
    history::estimate_run_average(repo, checks)
        .or_else(|| checks.iter().filter_map(|check| check.average_secs).max())
}

fn diff_line_type(line: &str) -> DiffLineType {
    if line.starts_with("+++") || line.starts_with("---") {
        DiffLineType::Header
    } else if line.starts_with('+') {
        DiffLineType::Addition
    } else if line.starts_with('-') {
        DiffLineType::Deletion
    } else if line.starts_with("@@") {
        DiffLineType::Hunk
    } else if line.starts_with("diff ") || line.starts_with("index ") {
        DiffLineType::Header
    } else {
        DiffLineType::Context
    }
}

fn diff_line_type_name(line_type: &DiffLineType) -> &'static str {
    match line_type {
        DiffLineType::Header => "header",
        DiffLineType::Addition => "addition",
        DiffLineType::Deletion => "deletion",
        DiffLineType::Context => "context",
        DiffLineType::Hunk => "hunk",
    }
}

fn diff_line_type_from_name(name: &str, content: &str) -> DiffLineType {
    match name {
        "header" => DiffLineType::Header,
        "addition" => DiffLineType::Addition,
        "deletion" => DiffLineType::Deletion,
        "context" => DiffLineType::Context,
        "hunk" => DiffLineType::Hunk,
        _ => diff_line_type(content),
    }
}

fn cached_diff_to_disk(diff: &CachedDiff) -> DiskCachedDiff {
    DiskCachedDiff {
        stat: diff
            .stat
            .iter()
            .map(|line| DiskDiffStat {
                file: line.file.clone(),
                additions: line.additions,
                deletions: line.deletions,
            })
            .collect(),
        lines: diff
            .lines
            .iter()
            .map(|line| DiskDiffLine {
                content: line.content.clone(),
                line_type: diff_line_type_name(&line.line_type).to_string(),
            })
            .collect(),
    }
}

fn cached_diff_from_disk(diff: &DiskCachedDiff) -> CachedDiff {
    CachedDiff {
        stat: diff
            .stat
            .iter()
            .map(|line| DiffStatLine {
                file: line.file.clone(),
                additions: line.additions,
                deletions: line.deletions,
            })
            .collect(),
        lines: diff
            .lines
            .iter()
            .map(|line| DiffLine {
                content: line.content.clone(),
                line_type: diff_line_type_from_name(&line.line_type, &line.content),
            })
            .collect(),
    }
}

fn persistent_diff_cache_key(repo: &GitRepo, request: &DiffRequest) -> Option<String> {
    let parent_oid = repo.rev_parse(&request.parent).ok()?;
    let branch_oid = repo.rev_parse(&request.branch).ok()?;
    let merge_base_oid = repo
        .merge_base_refs(&request.parent, &request.branch)
        .ok()?;

    Some(TuiDiffCache::key(
        &request.parent,
        &request.branch,
        &parent_oid,
        &branch_oid,
        &merge_base_oid,
    ))
}

fn load_branch_details(repo: &GitRepo, request: &BranchDetailsRequest) -> BranchDetails {
    let (ahead, behind) = request
        .parent
        .as_deref()
        .and_then(|parent| repo.commits_ahead_behind(parent, &request.name).ok())
        .unwrap_or((0, 0));
    let has_remote = repo.has_remote(&request.name);
    let (unpushed, unpulled) = repo.commits_vs_remote(&request.name).unwrap_or((0, 0));
    let commits = request
        .parent
        .as_deref()
        .and_then(|parent| repo.commits_between(parent, &request.name).ok())
        .unwrap_or_default()
        .into_iter()
        .take(10)
        .collect();

    BranchDetails {
        ahead,
        behind,
        has_remote,
        unpushed,
        unpulled,
        commits,
    }
}

fn spawn_branch_details_loader(
    repo_path: PathBuf,
    requests: Vec<BranchDetailsRequest>,
) -> Receiver<BranchDetailsUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let repo = match GitRepo::open_from_path(&repo_path) {
            Ok(repo) => repo,
            Err(_) => {
                for request in requests {
                    let _ = sender.send(BranchDetailsUpdate::Unavailable {
                        branch: request.name,
                    });
                }
                let _ = sender.send(BranchDetailsUpdate::Done);
                return;
            }
        };

        for request in requests {
            let details = load_branch_details(&repo, &request);
            let _ = sender.send(BranchDetailsUpdate::Loaded {
                branch: request.name,
                details,
            });
        }

        let _ = sender.send(BranchDetailsUpdate::Done);
    });

    receiver
}

fn spawn_diff_loader(
    repo_path: PathBuf,
    diff_cache_dir: PathBuf,
    request: DiffRequest,
) -> Receiver<DiffUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let repo = match GitRepo::open_from_path(&repo_path) {
            Ok(repo) => repo,
            Err(_) => {
                let _ = sender.send(DiffUpdate::Unavailable { request });
                return;
            }
        };

        let stat = match repo.diff_stat(&request.branch, &request.parent) {
            Ok(stats) => stats
                .into_iter()
                .map(|(file, additions, deletions)| DiffStatLine {
                    file,
                    additions,
                    deletions,
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        let lines = match repo.diff_against_parent(&request.branch, &request.parent) {
            Ok(lines) => lines
                .into_iter()
                .map(|line| DiffLine {
                    line_type: diff_line_type(&line),
                    content: line,
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        let diff = CachedDiff { stat, lines };
        if let Some(key) = persistent_diff_cache_key(&repo, &request) {
            let mut cache = TuiDiffCache::load(&diff_cache_dir);
            cache.insert(key, cached_diff_to_disk(&diff));
            let _ = cache.save(&diff_cache_dir);
        }

        let _ = sender.send(DiffUpdate::Loaded { request, diff });
    });

    receiver
}

fn run_in_tokio_runtime<T, F, Fut>(operation: F) -> Result<T>
where
    F: FnOnce() -> Result<Fut>,
    Fut: Future<Output = Result<T>>,
{
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let future = operation()?;
        future.await
    })
}

fn spawn_ci_loader(repo_path: PathBuf, branch: String) -> Receiver<CiUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let repo = match GitRepo::open_from_path(&repo_path) {
            Ok(repo) => repo,
            Err(_) => {
                let _ = sender.send(CiUpdate::Unavailable {
                    branch,
                    message: "unable to open the repository".to_string(),
                });
                return;
            }
        };

        let config = match Config::load() {
            Ok(config) => config,
            Err(_) => {
                let _ = sender.send(CiUpdate::Unavailable {
                    branch,
                    message: "unable to load config".to_string(),
                });
                return;
            }
        };

        let remote = match RemoteInfo::from_repo(&repo, &config) {
            Ok(remote) => remote,
            Err(_) => {
                let _ = sender.send(CiUpdate::Unavailable {
                    branch,
                    message: "configure a git remote to fetch checks".to_string(),
                });
                return;
            }
        };

        let sha = match repo.branch_commit(&branch) {
            Ok(sha) => sha,
            Err(_) => {
                let _ = sender.send(CiUpdate::Unavailable {
                    branch,
                    message: "branch commit could not be resolved".to_string(),
                });
                return;
            }
        };

        let repo_ref = &repo;
        let sha_ref = sha.as_str();
        let (overall_status, check_runs) = match run_in_tokio_runtime(|| {
            let client = ForgeClient::new(&remote)?;
            Ok(async move { client.fetch_checks(repo_ref, sha_ref).await })
        }) {
            Ok(result) => result,
            Err(_) => {
                let _ = sender.send(CiUpdate::Unavailable {
                    branch: branch.clone(),
                    message: "live CI is temporarily unavailable".to_string(),
                });
                return;
            }
        };

        let average_secs = branch_average_secs(&repo, &check_runs);
        let summary = BranchCiSummary::from_checks(overall_status, &check_runs, average_secs);
        let _ = sender.send(CiUpdate::Loaded { branch, summary });
    });

    receiver
}

#[cfg(test)]
mod tests {
    use super::{
        live_ci_summary_text, run_in_tokio_runtime, spawn_diff_loader, substring_filter_indices,
        App, BranchCiSummary, BranchDisplay, DiffLineType, DiffRequest, DiffUpdate, FocusedPane,
        Mode,
    };
    use crate::cache::{CiCache, DiskCachedDiff, DiskDiffLine, DiskDiffStat, TuiDiffCache};
    use crate::engine::Stack;
    use crate::git::GitRepo;
    use anyhow::{anyhow, Result};
    use chrono::{TimeZone, Utc};
    use std::future::Ready;
    use std::process::Command;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;
    use tempfile::TempDir;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    fn run_git(dir: &std::path::Path, args: &[&str]) {
        let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", null_path)
            .env("GIT_CONFIG_SYSTEM", null_path)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {} failed\nstdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn test_repo() -> (TempDir, GitRepo) {
        let tempdir = TempDir::new().expect("temp repo");
        let path = tempdir.path();
        run_git(path, &["init", "-b", "main"]);
        run_git(path, &["config", "user.email", "test@example.com"]);
        run_git(path, &["config", "user.name", "Test User"]);
        std::fs::write(path.join("README.md"), "hello\n").expect("write README");
        run_git(path, &["add", "README.md"]);
        run_git(path, &["commit", "-m", "initial"]);

        let repo = GitRepo::open_from_path(&path.join(".git")).expect("open test repo");
        repo.set_trunk("main").expect("set trunk");
        (tempdir, repo)
    }

    fn skeleton_branch(name: &str, parent: Option<&str>, is_current: bool) -> BranchDisplay {
        BranchDisplay {
            name: name.to_string(),
            parent: parent.map(str::to_string),
            column: 0,
            is_current,
            is_trunk: parent.is_none(),
            ahead: 0,
            behind: 0,
            needs_restack: false,
            has_remote: false,
            unpushed: 0,
            unpulled: 0,
            pr_number: None,
            pr_state: None,
            ci_state: None,
            commits: Vec::new(),
            details_loaded: parent.is_none(),
        }
    }

    fn minimal_app(repo: GitRepo, branches: Vec<BranchDisplay>) -> App {
        let git_dir = repo.git_dir().expect("git dir").to_path_buf();
        let diff_cache_dir = repo.common_git_dir().expect("common git dir");
        let stack = Stack::load(&repo).expect("load stack");
        App {
            stack,
            cache: CiCache::load(&git_dir),
            repo,
            git_dir,
            diff_cache_dir,
            current_branch: "main".to_string(),
            selected_index: 0,
            branches,
            mode: Mode::Normal,
            search_query: String::new(),
            filtered_indices: Vec::new(),
            input_buffer: String::new(),
            input_cursor: 0,
            selected_diff: Vec::new(),
            diff_scroll: 0,
            focused_pane: FocusedPane::Stack,
            diff_stat: Vec::new(),
            status_message: None,
            status_set_at: None,
            should_quit: false,
            pending_command: None,
            needs_refresh: false,
            reorder_state: None,
            move_picker_source: String::new(),
            move_picker_candidates: Vec::new(),
            move_picker_query: String::new(),
            move_picker_selected: 0,
            diff_cache: Default::default(),
            ci_states: Default::default(),
            ci_loader: None,
            ci_loading_branch: None,
            ci_queued_branch: None,
            branch_details_queued: false,
            branch_details_loader: None,
            diff_loader: None,
            diff_loading: None,
            diff_queued: None,
        }
    }

    #[test]
    fn skeleton_branch_defers_expensive_fields_until_background_details_load() {
        let branch = skeleton_branch("feature", Some("main"), false);

        assert!(!branch.details_loaded);
        assert_eq!(branch.ahead, 0);
        assert_eq!(branch.behind, 0);
        assert!(!branch.has_remote);
        assert_eq!(branch.unpushed, 0);
        assert_eq!(branch.unpulled, 0);
        assert!(branch.commits.is_empty());
    }

    #[test]
    fn selecting_next_branch_queues_diff_without_loading_patch_synchronously() {
        let (_tempdir, repo) = test_repo();
        let mut app = minimal_app(
            repo,
            vec![
                skeleton_branch("main", None, true),
                skeleton_branch("feature", Some("main"), false),
            ],
        );

        app.select_next();

        assert_eq!(app.selected_index, 1);
        assert_eq!(
            app.diff_queued,
            Some(DiffRequest::new("feature".to_string(), "main".to_string()))
        );
        assert!(app.selected_diff.is_empty());
        assert!(app.diff_stat.is_empty());
    }

    #[test]
    fn selecting_branch_uses_persisted_diff_cache_when_refs_match() {
        let (_tempdir, repo) = test_repo();
        let workdir = repo.workdir().expect("workdir").to_path_buf();
        run_git(&workdir, &["switch", "-c", "feature"]);
        std::fs::write(workdir.join("README.md"), "hello\nfeature\n").expect("write README");
        run_git(&workdir, &["add", "README.md"]);
        run_git(&workdir, &["commit", "-m", "feature"]);

        let parent_oid = repo.rev_parse("main").expect("main oid");
        let branch_oid = repo.rev_parse("feature").expect("feature oid");
        let merge_base_oid = repo.merge_base_refs("main", "feature").expect("merge base");
        let cache_dir = repo.common_git_dir().expect("common git dir");
        let mut disk_cache = TuiDiffCache::default();
        disk_cache.insert(
            TuiDiffCache::key("main", "feature", &parent_oid, &branch_oid, &merge_base_oid),
            DiskCachedDiff {
                stat: vec![DiskDiffStat {
                    file: "README.md".to_string(),
                    additions: 1,
                    deletions: 0,
                }],
                lines: vec![DiskDiffLine {
                    content: "cached diff line".to_string(),
                    line_type: "context".to_string(),
                }],
            },
        );
        disk_cache.save(&cache_dir).expect("save cache");

        let mut app = minimal_app(
            repo,
            vec![
                skeleton_branch("main", None, false),
                skeleton_branch("feature", Some("main"), true),
            ],
        );

        app.select_next();

        assert_eq!(app.diff_queued, None);
        assert_eq!(app.selected_diff.len(), 1);
        assert_eq!(app.selected_diff[0].content, "cached diff line");
        assert_eq!(app.selected_diff[0].line_type, DiffLineType::Context);
        assert_eq!(app.diff_stat.len(), 1);
        assert_eq!(app.diff_stat[0].file, "README.md");
    }

    #[test]
    fn selecting_branch_ignores_persisted_diff_cache_when_branch_tip_changes() {
        let (_tempdir, repo) = test_repo();
        let workdir = repo.workdir().expect("workdir").to_path_buf();
        run_git(&workdir, &["switch", "-c", "feature"]);
        std::fs::write(workdir.join("README.md"), "hello\nfeature\n").expect("write README");
        run_git(&workdir, &["add", "README.md"]);
        run_git(&workdir, &["commit", "-m", "feature"]);

        let old_branch_oid = repo.rev_parse("feature").expect("old feature oid");
        let parent_oid = repo.rev_parse("main").expect("main oid");
        let merge_base_oid = repo.merge_base_refs("main", "feature").expect("merge base");
        let cache_dir = repo.common_git_dir().expect("common git dir");
        let mut disk_cache = TuiDiffCache::default();
        disk_cache.insert(
            TuiDiffCache::key(
                "main",
                "feature",
                &parent_oid,
                &old_branch_oid,
                &merge_base_oid,
            ),
            DiskCachedDiff {
                stat: vec![DiskDiffStat {
                    file: "README.md".to_string(),
                    additions: 1,
                    deletions: 0,
                }],
                lines: vec![DiskDiffLine {
                    content: "stale cached diff line".to_string(),
                    line_type: "context".to_string(),
                }],
            },
        );
        disk_cache.save(&cache_dir).expect("save cache");

        std::fs::write(workdir.join("README.md"), "hello\nfeature\nupdated\n")
            .expect("write README");
        run_git(&workdir, &["add", "README.md"]);
        run_git(&workdir, &["commit", "-m", "update feature"]);

        let mut app = minimal_app(
            repo,
            vec![
                skeleton_branch("main", None, false),
                skeleton_branch("feature", Some("main"), true),
            ],
        );

        app.select_next();

        assert_eq!(
            app.diff_queued,
            Some(DiffRequest::new("feature".to_string(), "main".to_string()))
        );
        assert!(app.selected_diff.is_empty());
        assert!(app.diff_stat.is_empty());
    }

    #[test]
    fn diff_loader_persists_loaded_diff_for_reopen() {
        let (_tempdir, repo) = test_repo();
        let workdir = repo.workdir().expect("workdir").to_path_buf();
        run_git(&workdir, &["switch", "-c", "feature"]);
        std::fs::write(workdir.join("README.md"), "hello\nfeature\n").expect("write README");
        run_git(&workdir, &["add", "README.md"]);
        run_git(&workdir, &["commit", "-m", "feature"]);

        let repo_path = repo.git_dir().expect("git dir").to_path_buf();
        let cache_dir = repo.common_git_dir().expect("common git dir");
        let request = DiffRequest::new("feature".to_string(), "main".to_string());
        let receiver = spawn_diff_loader(repo_path, cache_dir.clone(), request.clone());

        match receiver
            .recv_timeout(Duration::from_secs(15))
            .expect("diff update")
        {
            DiffUpdate::Loaded {
                request: loaded, ..
            } => assert_eq!(loaded, request),
            DiffUpdate::Unavailable { .. } => panic!("diff should load"),
        }

        let parent_oid = repo.rev_parse("main").expect("main oid");
        let branch_oid = repo.rev_parse("feature").expect("feature oid");
        let merge_base_oid = repo.merge_base_refs("main", "feature").expect("merge base");
        let key = TuiDiffCache::key("main", "feature", &parent_oid, &branch_oid, &merge_base_oid);
        let cache = TuiDiffCache::load(&cache_dir);
        let cached = cache.get(&key).expect("persisted diff");
        assert_eq!(cached.stat.len(), 1);
        assert!(cached.lines.iter().any(|line| line.content == "+feature"));
    }

    #[test]
    fn substring_filter_empty_query_returns_all_indices_in_order() {
        let candidates = names(&["main", "feat-a", "feat-b"]);
        assert_eq!(substring_filter_indices(&candidates, ""), vec![0, 1, 2]);
    }

    #[test]
    fn substring_filter_matches_case_insensitively() {
        let candidates = names(&["Main", "FEAT-A", "feat-b"]);
        assert_eq!(substring_filter_indices(&candidates, "feat"), vec![1, 2]);
        assert_eq!(substring_filter_indices(&candidates, "MAIN"), vec![0]);
    }

    #[test]
    fn substring_filter_returns_empty_when_nothing_matches() {
        let candidates = names(&["main", "feat-a"]);
        assert!(substring_filter_indices(&candidates, "xyz").is_empty());
    }

    fn sample_summary() -> BranchCiSummary {
        BranchCiSummary {
            overall_status: Some("pending".to_string()),
            total: 6,
            passed: 2,
            failed: 0,
            running: 1,
            queued: 3,
            skipped: 0,
            started_at: Some(Utc.with_ymd_and_hms(2026, 4, 14, 10, 0, 0).unwrap()),
            completed_at: None,
            average_secs: Some(900),
        }
    }

    #[test]
    fn ci_summary_tracks_completion_counts_and_eta() {
        let summary = sample_summary();
        let now = Utc.with_ymd_and_hms(2026, 4, 14, 10, 5, 0).unwrap();

        assert_eq!(summary.completed_count(), 2);
        assert_eq!(summary.elapsed_secs(now), Some(300));
        assert_eq!(summary.progress_percent(now), Some(33));
        assert_eq!(summary.eta_secs(now), Some(600));
    }

    #[test]
    fn ci_summary_marks_completed_runs_as_done() {
        let summary = BranchCiSummary {
            overall_status: Some("success".to_string()),
            total: 3,
            passed: 2,
            failed: 0,
            running: 0,
            queued: 0,
            skipped: 1,
            started_at: Some(Utc.with_ymd_and_hms(2026, 4, 14, 10, 0, 0).unwrap()),
            completed_at: Some(Utc.with_ymd_and_hms(2026, 4, 14, 10, 6, 0).unwrap()),
            average_secs: Some(600),
        };

        assert!(summary.is_complete());
        assert_eq!(summary.progress_percent(Utc::now()), Some(100));
    }

    #[test]
    fn live_ci_text_handles_missing_checks() {
        let summary = BranchCiSummary {
            overall_status: None,
            total: 0,
            passed: 0,
            failed: 0,
            running: 0,
            queued: 0,
            skipped: 0,
            started_at: None,
            completed_at: None,
            average_secs: None,
        };

        let (text, dimmed) = live_ci_summary_text(&summary);
        assert_eq!(text, "Live CI: no checks reported for the latest push");
        assert!(dimmed);
    }

    #[test]
    fn run_in_tokio_runtime_provides_runtime_to_setup_and_future() {
        let setup_has_runtime = Arc::new(AtomicBool::new(false));
        let future_has_runtime = Arc::new(AtomicBool::new(false));
        let setup_has_runtime_clone = Arc::clone(&setup_has_runtime);
        let future_has_runtime_clone = Arc::clone(&future_has_runtime);

        let result = run_in_tokio_runtime(|| {
            setup_has_runtime_clone.store(
                tokio::runtime::Handle::try_current().is_ok(),
                Ordering::SeqCst,
            );
            Ok(async move {
                future_has_runtime_clone.store(
                    tokio::runtime::Handle::try_current().is_ok(),
                    Ordering::SeqCst,
                );
                Ok::<_, anyhow::Error>(42usize)
            })
        })
        .unwrap();

        assert_eq!(result, 42);
        assert!(setup_has_runtime.load(Ordering::SeqCst));
        assert!(future_has_runtime.load(Ordering::SeqCst));
    }

    #[test]
    fn run_in_tokio_runtime_propagates_setup_errors() {
        let err =
            run_in_tokio_runtime::<(), _, Ready<Result<()>>>(|| -> Result<Ready<Result<()>>> {
                Err(anyhow!("setup failed"))
            })
            .unwrap_err();

        assert!(format!("{err:#}").contains("setup failed"));
    }

    #[test]
    fn run_in_tokio_runtime_propagates_async_errors() {
        let err = run_in_tokio_runtime(|| Ok(async { Err::<(), _>(anyhow!("fetch failed")) }))
            .unwrap_err();

        assert!(format!("{err:#}").contains("fetch failed"));
    }
}
