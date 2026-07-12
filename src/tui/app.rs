use crate::application::{
    BranchDetails, BranchDiff, BranchSummary, CiSummary, DiffLine, DiffStatLine, RepositorySession,
};
use crate::cache::{CiCache, TuiPaneVisibilityState, TuiStateCache};
use crate::engine::{Stack, StackSnapshot, build_parent_candidates};
use crate::git::GitRepo;
use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

const CI_ACTIVE_REFRESH_INTERVAL: Duration = Duration::from_secs(15);
const CI_IDLE_REFRESH_INTERVAL: Duration = Duration::from_secs(120);
const CI_ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(60);

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
        diff: BranchDiff,
    },
    Unavailable {
        request: DiffRequest,
    },
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

#[derive(Debug, Clone)]
pub enum BranchCiState {
    Loading,
    Ready {
        summary: CiSummary,
        fetched_at: Instant,
    },
    Unavailable {
        message: String,
        fetched_at: Instant,
    },
}

#[derive(Debug)]
enum CiUpdate {
    Loaded { branch: String, summary: CiSummary },
    Unavailable { branch: String, message: String },
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
    fn from_summary(summary: BranchSummary) -> Self {
        Self {
            name: summary.name,
            parent: summary.parent,
            column: summary.column,
            is_current: summary.is_current,
            is_trunk: summary.is_trunk,
            ahead: 0,
            behind: 0,
            needs_restack: summary.needs_restack,
            has_remote: false,
            unpushed: 0,
            unpulled: 0,
            pr_number: summary.pr_number,
            pr_state: summary.pr_state,
            ci_state: summary.ci_state,
            commits: Vec::new(),
            details_loaded: summary.is_trunk,
        }
    }

    fn summary(&self) -> BranchSummary {
        BranchSummary {
            name: self.name.clone(),
            parent: self.parent.clone(),
            column: self.column,
            is_current: self.is_current,
            is_trunk: self.is_trunk,
            needs_restack: self.needs_restack,
            pr_number: self.pr_number,
            pr_state: self.pr_state.clone(),
            ci_state: self.ci_state.clone(),
        }
    }

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusedPane {
    #[default]
    Stack,
    Summary,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiPane {
    Stack,
    Summary,
    Patch,
}

/// Runtime visibility for dashboard panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneVisibility {
    pub stack: bool,
    pub summary: bool,
    pub patch: bool,
}

impl Default for PaneVisibility {
    fn default() -> Self {
        Self {
            stack: true,
            summary: true,
            patch: true,
        }
    }
}

impl PaneVisibility {
    pub fn is_visible(self, pane: TuiPane) -> bool {
        match pane {
            TuiPane::Stack => self.stack,
            TuiPane::Summary => self.summary,
            TuiPane::Patch => self.patch,
        }
    }

    fn set_visible(&mut self, pane: TuiPane, visible: bool) {
        match pane {
            TuiPane::Stack => self.stack = visible,
            TuiPane::Summary => self.summary = visible,
            TuiPane::Patch => self.patch = visible,
        }
    }

    pub fn visible_count(self) -> usize {
        [self.stack, self.summary, self.patch]
            .into_iter()
            .filter(|visible| *visible)
            .count()
    }

    fn from_persisted(state: TuiPaneVisibilityState) -> Self {
        let visibility = Self {
            stack: state.stack,
            summary: state.summary,
            patch: state.patch,
        };

        if visibility.visible_count() == 0 {
            Self::default()
        } else {
            visibility
        }
    }

    fn to_persisted(self) -> TuiPaneVisibilityState {
        TuiPaneVisibilityState {
            stack: self.stack,
            summary: self.summary,
            patch: self.patch,
        }
    }
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
    session: RepositorySession,
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
    pub pane_visibility: PaneVisibility,
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
    diff_cache: HashMap<String, BranchDiff>,
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
        let session = RepositorySession::open(repo.workdir()?)?;
        let repository_snapshot = session.snapshot()?;
        let snapshot = StackSnapshot::load(&repo)?;
        let git_dir = repo.git_dir()?;
        let cache = CiCache::load(git_dir);
        let diff_cache_dir = repo.common_git_dir()?;
        let pane_visibility = TuiStateCache::load(&diff_cache_dir)
            .panes
            .map(PaneVisibility::from_persisted)
            .unwrap_or_default();
        let git_dir = git_dir.to_path_buf();
        let status_set_at = initial_status.as_ref().map(|_| Instant::now());
        let branches = repository_snapshot
            .branches
            .into_iter()
            .map(BranchDisplay::from_summary)
            .collect();

        let mut app = Self {
            stack: snapshot.stack,
            cache,
            repo,
            session,
            git_dir,
            diff_cache_dir,
            current_branch: repository_snapshot.current_branch,
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
            pane_visibility,
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

        app.needs_refresh = false;
        app.branch_details_queued = true;
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
        let snapshot = StackSnapshot::load(&self.repo)?;
        let repository_snapshot = self.session.snapshot()?;
        self.stack = snapshot.stack;
        self.current_branch = repository_snapshot.current_branch;
        self.branches = repository_snapshot
            .branches
            .into_iter()
            .map(BranchDisplay::from_summary)
            .collect();
        self.diff_cache.clear();
        self.needs_refresh = false;
        self.branch_details_queued = true;
        self.queue_diff_refresh_for_selected();
        self.queue_ci_refresh_for_selected();
        Ok(())
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

        if let Ok(Some(cached)) = self.session.cached_diff(&request.branch, &request.parent) {
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

    pub fn toggle_pane_visibility(&mut self, pane: TuiPane) {
        let currently_visible = self.pane_visibility.is_visible(pane);
        if currently_visible && self.pane_visibility.visible_count() == 1 {
            self.set_status("At least one pane must remain visible");
            return;
        }

        self.pane_visibility.set_visible(pane, !currently_visible);
        self.ensure_focus_visible();
        self.persist_tui_state();

        let pane_name = match pane {
            TuiPane::Stack => "Stack",
            TuiPane::Summary => "Summary",
            TuiPane::Patch => "Patch",
        };
        let state = if currently_visible { "hidden" } else { "shown" };
        self.set_status(format!("{} pane {}", pane_name, state));
    }

    fn persist_tui_state(&self) {
        let mut state = TuiStateCache::load(&self.diff_cache_dir);
        state.panes = Some(self.pane_visibility.to_persisted());
        let _ = state.save(&self.diff_cache_dir);
    }

    fn focused_pane_is_visible(&self) -> bool {
        match self.focused_pane {
            FocusedPane::Stack => self.pane_visibility.stack,
            FocusedPane::Summary => self.pane_visibility.summary,
            FocusedPane::Diff => self.pane_visibility.patch,
        }
    }

    fn ensure_focus_visible(&mut self) {
        if self.focused_pane_is_visible() {
            return;
        }

        self.focused_pane = if self.pane_visibility.stack {
            FocusedPane::Stack
        } else if self.pane_visibility.patch {
            FocusedPane::Diff
        } else {
            FocusedPane::Summary
        };
    }

    pub fn focus_next_visible_pane(&mut self) {
        let panes = [FocusedPane::Stack, FocusedPane::Summary, FocusedPane::Diff];
        let current = panes
            .iter()
            .position(|pane| *pane == self.focused_pane)
            .unwrap_or(0);

        for offset in 1..=panes.len() {
            let next = panes[(current + offset) % panes.len()].clone();
            let is_visible = match next {
                FocusedPane::Stack => self.pane_visibility.stack,
                FocusedPane::Summary => self.pane_visibility.summary,
                FocusedPane::Diff => self.pane_visibility.patch,
            };
            if is_visible {
                self.focused_pane = next;
                return;
            }
        }
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
            .map(BranchDisplay::summary)
            .collect::<Vec<_>>();

        self.branch_details_queued = false;
        self.branch_details_loader = (!requests.is_empty())
            .then(|| spawn_branch_details_loader(self.session.clone(), requests));
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
        self.diff_loader = Some(spawn_diff_loader(self.session.clone(), request));
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
        self.ci_loader = Some(spawn_ci_loader(self.session.clone(), branch));
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

fn live_ci_summary_text(summary: &CiSummary) -> (String, bool) {
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

fn spawn_branch_details_loader(
    session: RepositorySession,
    requests: Vec<BranchSummary>,
) -> Receiver<BranchDetailsUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        for request in requests {
            let branch = request.name.clone();
            let update = match session.branch_details(&request) {
                Ok(details) => BranchDetailsUpdate::Loaded { branch, details },
                Err(_) => BranchDetailsUpdate::Unavailable { branch },
            };
            let _ = sender.send(update);
        }

        let _ = sender.send(BranchDetailsUpdate::Done);
    });

    receiver
}

fn spawn_diff_loader(session: RepositorySession, request: DiffRequest) -> Receiver<DiffUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let update = match session.diff(&request.branch, &request.parent) {
            Ok(diff) => DiffUpdate::Loaded { request, diff },
            Err(_) => DiffUpdate::Unavailable { request },
        };
        let _ = sender.send(update);
    });

    receiver
}

fn spawn_ci_loader(session: RepositorySession, branch: String) -> Receiver<CiUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let update = match session.load_ci(&branch) {
            Ok(summary) => CiUpdate::Loaded { branch, summary },
            Err(error) => CiUpdate::Unavailable {
                branch,
                message: format!("{error:#}"),
            },
        };
        let _ = sender.send(update);
    });

    receiver
}

#[cfg(test)]
mod tests {
    use super::{
        App, BranchDetailsUpdate, BranchDisplay, CiUpdate, DiffRequest, DiffUpdate, FocusedPane,
        Mode, PaneVisibility, TuiPane, TuiPaneVisibilityState, live_ci_summary_text,
        spawn_ci_loader, spawn_diff_loader, substring_filter_indices,
    };
    use crate::application::{BranchDetails, CiSummary, DiffLineKind, RepositorySession};
    use crate::cache::{
        CiCache, DiskCachedDiff, DiskDiffLine, DiskDiffStat, TuiDiffCache, TuiStateCache,
    };
    use crate::engine::Stack;
    use crate::git::GitRepo;
    use std::process::Command;
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
        let session = RepositorySession::open(repo.workdir().expect("workdir"))
            .expect("open repository session");
        let git_dir = repo.git_dir().expect("git dir").to_path_buf();
        let diff_cache_dir = repo.common_git_dir().expect("common git dir");
        let stack = Stack::load(&repo).expect("load stack");
        App {
            stack,
            cache: CiCache::load(&git_dir),
            repo,
            session,
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
            pane_visibility: PaneVisibility::default(),
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
    fn custom_remote_details_make_selected_branch_eligible_for_ci_queueing() {
        let (_tempdir, repo) = test_repo();
        let mut app = minimal_app(repo, vec![skeleton_branch("feature", Some("main"), true)]);

        app.apply_branch_details_update(BranchDetailsUpdate::Loaded {
            branch: "feature".to_string(),
            details: BranchDetails {
                ahead: 1,
                behind: 0,
                has_remote: true,
                unpushed: 1,
                unpulled: 0,
                commits: vec!["feature commit".to_string()],
            },
        });
        app.queue_ci_refresh_for_selected();

        assert!(app.branches[0].details_loaded);
        assert!(app.branches[0].has_remote);
        assert_eq!(app.ci_queued_branch.as_deref(), Some("feature"));
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
    fn selecting_branch_cache_miss_does_not_compute_patch_synchronously() {
        let (_tempdir, repo) = test_repo();
        let workdir = repo.workdir().expect("workdir").to_path_buf();
        run_git(&workdir, &["switch", "-c", "feature"]);
        std::fs::write(workdir.join("README.md"), "hello\nfeature\n").expect("write README");
        run_git(&workdir, &["add", "README.md"]);
        run_git(&workdir, &["commit", "-m", "feature"]);
        let blob_oid = repo
            .rev_parse("feature:README.md")
            .expect("feature README blob");
        let cache_dir = repo.common_git_dir().expect("common git dir");
        let object_path = cache_dir
            .join("objects")
            .join(&blob_oid[..2])
            .join(&blob_oid[2..]);
        std::fs::remove_file(object_path).expect("remove feature blob");
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
        assert!(!cache_dir.join("stax").join("tui-diff-cache.json").exists());
    }

    #[test]
    fn toggling_patch_hides_it_and_moves_focus_to_stack() {
        let (_tempdir, repo) = test_repo();
        let mut app = minimal_app(repo, vec![skeleton_branch("main", None, true)]);
        app.focused_pane = FocusedPane::Diff;

        app.toggle_pane_visibility(TuiPane::Patch);

        assert!(!app.pane_visibility.patch);
        assert_eq!(app.focused_pane, FocusedPane::Stack);
        assert_eq!(app.status_message.as_deref(), Some("Patch pane hidden"));
    }

    #[test]
    fn toggling_pane_visibility_persists_for_next_tui_load() {
        let (_tempdir, repo) = test_repo();
        let cache_dir = repo.common_git_dir().expect("common git dir");
        let mut app = minimal_app(repo, vec![skeleton_branch("main", None, true)]);

        app.toggle_pane_visibility(TuiPane::Summary);

        let state = TuiStateCache::load(&cache_dir);
        let panes = state.panes.expect("pane visibility should be persisted");
        assert!(panes.stack);
        assert!(!panes.summary);
        assert!(panes.patch);
    }

    #[test]
    fn invalid_persisted_pane_visibility_falls_back_to_default() {
        let visibility = PaneVisibility::from_persisted(TuiPaneVisibilityState {
            stack: false,
            summary: false,
            patch: false,
        });

        assert_eq!(visibility, PaneVisibility::default());
    }

    #[test]
    fn toggling_does_not_hide_last_visible_pane() {
        let (_tempdir, repo) = test_repo();
        let mut app = minimal_app(repo, vec![skeleton_branch("main", None, true)]);
        app.pane_visibility = PaneVisibility {
            stack: true,
            summary: false,
            patch: false,
        };

        app.toggle_pane_visibility(TuiPane::Stack);

        assert!(app.pane_visibility.stack);
        assert_eq!(
            app.status_message.as_deref(),
            Some("At least one pane must remain visible")
        );
    }

    #[test]
    fn tab_skips_hidden_patch_pane() {
        let (_tempdir, repo) = test_repo();
        let mut app = minimal_app(repo, vec![skeleton_branch("main", None, true)]);
        app.pane_visibility.patch = false;

        app.focus_next_visible_pane();

        assert_eq!(app.focused_pane, FocusedPane::Summary);
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
        assert_eq!(app.selected_diff[0].kind, DiffLineKind::Context);
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

        let cache_dir = repo.common_git_dir().expect("common git dir");
        let request = DiffRequest::new("feature".to_string(), "main".to_string());
        let session = RepositorySession::open(&workdir).expect("open session");
        let receiver = spawn_diff_loader(session, request.clone());

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
    fn diff_loader_does_not_cache_git_failures_as_empty_success() {
        let (_tempdir, repo) = test_repo();
        let workdir = repo.workdir().expect("workdir").to_path_buf();
        run_git(&workdir, &["switch", "-c", "feature"]);
        std::fs::write(workdir.join("README.md"), "hello\nfeature\n").expect("write README");
        run_git(&workdir, &["add", "README.md"]);
        run_git(&workdir, &["commit", "-m", "feature"]);

        let parent_oid = repo.rev_parse("main").expect("main oid");
        let branch_oid = repo.rev_parse("feature").expect("feature oid");
        let merge_base_oid = repo.merge_base_refs("main", "feature").expect("merge base");
        let blob_oid = repo
            .rev_parse("feature:README.md")
            .expect("feature README blob");
        let cache_dir = repo.common_git_dir().expect("common git dir");
        let object_path = cache_dir
            .join("objects")
            .join(&blob_oid[..2])
            .join(&blob_oid[2..]);
        std::fs::remove_file(object_path).expect("remove feature blob");
        let request = DiffRequest::new("feature".to_string(), "main".to_string());
        let session = RepositorySession::open(&workdir).expect("open session");

        let update = spawn_diff_loader(session, request.clone())
            .recv_timeout(Duration::from_secs(15))
            .expect("diff update");

        match update {
            DiffUpdate::Unavailable { request: failed } => assert_eq!(failed, request),
            DiffUpdate::Loaded { .. } => panic!("failed diff must remain unavailable"),
        }
        let key = TuiDiffCache::key("main", "feature", &parent_oid, &branch_oid, &merge_base_oid);
        assert!(TuiDiffCache::load(&cache_dir).get(&key).is_none());
        assert!(!cache_dir.join("stax").join("tui-diff-cache.json").exists());

        let error = RepositorySession::open(&workdir)
            .expect("open session")
            .diff("feature", "main")
            .unwrap_err();
        assert!(format!("{error:#}").contains("exit status"));
        assert!(TuiDiffCache::load(&cache_dir).get(&key).is_none());
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

    #[test]
    fn live_ci_text_handles_missing_checks() {
        let summary = CiSummary {
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
    fn ci_loader_reports_actionable_session_errors() {
        let (_tempdir, repo) = test_repo();
        let session =
            RepositorySession::open(repo.workdir().expect("workdir")).expect("open session");

        let update = spawn_ci_loader(session, "main".to_string())
            .recv_timeout(Duration::from_secs(15))
            .expect("CI update");

        match update {
            CiUpdate::Unavailable { branch, message } => {
                assert_eq!(branch, "main");
                assert!(message.contains("configure a git remote"));
            }
            CiUpdate::Loaded { .. } => panic!("CI should be unavailable without a remote"),
        }
    }
}
