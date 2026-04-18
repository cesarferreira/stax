use crate::cache::CiCache;
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
        let git_dir = git_dir.to_path_buf();
        let status_set_at = initial_status.as_ref().map(|_| Instant::now());

        let mut app = Self {
            stack,
            cache,
            repo,
            git_dir,
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
        };

        app.refresh_branches()?;
        if let Some(branch) = preferred_selection {
            app.select_branch(&branch);
        } else {
            app.select_current_branch();
        }
        app.update_diff();
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
        self.update_diff();
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
            branches.push(self.create_branch_display(trunk, 0, true)?);
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
        branches.push(self.create_branch_display(trunk, 0, true)?);

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
                    result.push(self.create_branch_display(&frame.branch, frame.column, false)?);
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

    fn create_branch_display(
        &self,
        branch: &str,
        column: usize,
        is_trunk: bool,
    ) -> Result<BranchDisplay> {
        let is_current = branch == self.current_branch;
        let info = self.stack.branches.get(branch);

        let (ahead, behind) = if let Some(info) = info {
            if let Some(parent) = &info.parent {
                self.repo
                    .commits_ahead_behind(parent, branch)
                    .unwrap_or((0, 0))
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };

        let needs_restack = info.map(|i| i.needs_restack).unwrap_or(false);
        let has_remote = self.repo.has_remote(branch);

        // Get ahead/behind vs remote
        let (unpushed, unpulled) = self.repo.commits_vs_remote(branch).unwrap_or((0, 0));

        let pr_number = info.and_then(|i| i.pr_number);
        let pr_state = info.and_then(|i| i.pr_state.clone());
        let parent = info.and_then(|i| i.parent.clone());

        // Get commits for this branch
        let commits = if let Some(parent) = &parent {
            self.repo
                .commits_between(parent, branch)
                .unwrap_or_default()
                .into_iter()
                .take(10)
                .collect()
        } else {
            Vec::new()
        };

        let ci_state = self.cache.get_ci_state(branch);

        Ok(BranchDisplay {
            name: branch.to_string(),
            parent,
            column,
            is_current,
            is_trunk,
            ahead,
            behind,
            needs_restack,
            has_remote,
            unpushed,
            unpulled,
            pr_number,
            pr_state,
            ci_state,
            commits,
        })
    }

    /// Select the current branch in the list
    pub fn select_current_branch(&mut self) {
        if let Some(idx) = self.branches.iter().position(|b| b.is_current) {
            self.selected_index = idx;
        }
        self.queue_ci_refresh_for_selected();
    }

    /// Select a branch by name, falling back to current branch when not found.
    pub fn select_branch(&mut self, branch: &str) {
        if let Some(idx) = self.branches.iter().position(|b| b.name == branch) {
            self.selected_index = idx;
            self.update_diff();
            self.queue_ci_refresh_for_selected();
            return;
        }

        self.select_current_branch();
        self.update_diff();
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
            self.update_diff();
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
            self.update_diff();
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
        let candidates = build_parent_candidates(
            &all_names,
            &source_name,
            &descendants,
            &self.stack.trunk,
        );
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

    /// Update the diff for the currently selected branch
    pub fn update_diff(&mut self) {
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

        let cache_key = format!("{}...{}", parent_name, branch_name);
        if let Some(cached) = self.diff_cache.get(&cache_key) {
            self.diff_stat = cached.stat.clone();
            self.selected_diff = cached.lines.clone();
            return;
        }

        // Get diff stat
        if let Ok(stats) = self.repo.diff_stat(&branch_name, &parent_name) {
            self.diff_stat = stats
                .into_iter()
                .map(|(file, additions, deletions)| DiffStatLine {
                    file,
                    additions,
                    deletions,
                })
                .collect();
        }

        // Get full diff
        if let Ok(lines) = self.repo.diff_against_parent(&branch_name, &parent_name) {
            self.selected_diff = lines
                .into_iter()
                .map(|line| {
                    let line_type = if line.starts_with("+++") || line.starts_with("---") {
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
                    };
                    DiffLine {
                        content: line,
                        line_type,
                    }
                })
                .collect();
        }

        self.diff_cache.insert(
            cache_key,
            CachedDiff {
                stat: self.diff_stat.clone(),
                lines: self.selected_diff.clone(),
            },
        );
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
        self.poll_ci_updates();
        self.queue_ci_refresh_for_selected();
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

fn branch_average_secs(repo: &GitRepo, branch_name: &str, checks: &[CheckRunInfo]) -> Option<u64> {
    let history_key = format!("branch-overall:{}", branch_name);
    history::load_check_history(repo, &history_key)
        .ok()
        .and_then(|history| history::calculate_average(&history))
        .or_else(|| checks.iter().filter_map(|check| check.average_secs).max())
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

        let average_secs = branch_average_secs(&repo, &branch, &check_runs);
        let summary = BranchCiSummary::from_checks(overall_status, &check_runs, average_secs);
        let _ = sender.send(CiUpdate::Loaded { branch, summary });
    });

    receiver
}

#[cfg(test)]
mod tests {
    use super::{
        live_ci_summary_text, run_in_tokio_runtime, substring_filter_indices, BranchCiSummary,
    };
    use anyhow::{anyhow, Result};
    use chrono::{TimeZone, Utc};
    use std::future::Ready;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
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
