use crate::cache::CiCache;
use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;
use std::time::Instant;

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

/// Branch display information for the TUI
#[derive(Debug, Clone)]
pub struct BranchDisplay {
    pub name: String,
    pub parent: Option<String>,
    pub column: usize,
    pub is_current: bool,
    pub is_trunk: bool,
    pub ahead: usize,
    pub behind: usize,
    pub needs_restack: bool,
    pub has_remote: bool,
    pub pr_number: Option<u64>,
    pub pr_state: Option<String>,
    pub pr_url: Option<String>,
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
}

/// Main application state
pub struct App {
    pub stack: Stack,
    #[allow(dead_code)] // Reserved for future CI status display
    pub cache: CiCache,
    pub repo: GitRepo,
    pub remote_info: Option<RemoteInfo>,
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
    pub needs_refresh: bool,
}

impl App {
    pub fn new() -> Result<Self> {
        let repo = GitRepo::open()?;
        let stack = Stack::load(&repo)?;
        let current_branch = repo.current_branch()?;
        let git_dir = repo.git_dir()?;
        let cache = CiCache::load(git_dir);
        let config = Config::load()?;
        let remote_info = RemoteInfo::from_repo(&repo, &config).ok();

        let mut app = Self {
            stack,
            cache,
            repo,
            remote_info,
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
            status_message: None,
            status_set_at: None,
            should_quit: false,
            needs_refresh: true,
        };

        app.refresh_branches()?;
        app.select_current_branch();
        app.update_diff();

        Ok(app)
    }

    /// Refresh the branch list from the repository
    pub fn refresh_branches(&mut self) -> Result<()> {
        self.stack = Stack::load(&self.repo)?;
        self.current_branch = self.repo.current_branch()?;
        self.branches = self.build_branch_list()?;
        self.needs_refresh = false;
        self.update_diff();
        Ok(())
    }

    /// Build the ordered list of branches for display
    fn build_branch_list(&self) -> Result<Vec<BranchDisplay>> {
        let mut branches = Vec::new();
        let trunk = &self.stack.trunk;

        // Get trunk children (each starts a chain)
        let trunk_info = self.stack.branches.get(trunk);
        let trunk_children: Vec<String> = trunk_info
            .map(|b| b.children.clone())
            .unwrap_or_default();

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
        column: usize,
        max_column: &mut usize,
    ) -> Result<()> {
        *max_column = (*max_column).max(column);

        if let Some(info) = self.stack.branches.get(branch) {
            let mut children: Vec<&String> = info.children.iter().collect();
            children.sort();

            for (i, child) in children.iter().enumerate() {
                self.collect_branches(result, child, column + i, max_column)?;
            }
        }

        result.push(self.create_branch_display(branch, column, false)?);
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
                self.repo.commits_ahead_behind(parent, branch).unwrap_or((0, 0))
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };

        let needs_restack = info.map(|i| i.needs_restack).unwrap_or(false);
        let has_remote = self.repo.has_remote(branch);
        let pr_number = info.and_then(|i| i.pr_number);
        let pr_state = info.and_then(|i| i.pr_state.clone());
        let pr_url = pr_number.and_then(|n| self.remote_info.as_ref().map(|r| r.pr_url(n)));
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
            pr_number,
            pr_state,
            pr_url,
            commits,
        })
    }

    /// Select the current branch in the list
    pub fn select_current_branch(&mut self) {
        if let Some(idx) = self.branches.iter().position(|b| b.is_current) {
            self.selected_index = idx;
        }
    }

    /// Get the currently selected branch
    pub fn selected_branch(&self) -> Option<&BranchDisplay> {
        if self.mode == Mode::Search && !self.filtered_indices.is_empty() {
            self.filtered_indices
                .get(self.selected_index)
                .and_then(|&idx| self.branches.get(idx))
        } else {
            self.branches.get(self.selected_index)
        }
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        let len = if self.mode == Mode::Search && !self.filtered_indices.is_empty() {
            self.filtered_indices.len()
        } else {
            self.branches.len()
        };

        if len > 0 && self.selected_index > 0 {
            self.selected_index -= 1;
            self.update_diff();
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        let len = if self.mode == Mode::Search && !self.filtered_indices.is_empty() {
            self.filtered_indices.len()
        } else {
            self.branches.len()
        };

        if len > 0 && self.selected_index < len - 1 {
            self.selected_index += 1;
            self.update_diff();
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

    /// Update the diff for the currently selected branch
    pub fn update_diff(&mut self) {
        self.selected_diff.clear();
        self.diff_stat.clear();
        self.diff_scroll = 0;

        if let Some(branch) = self.selected_branch() {
            if let Some(parent) = &branch.parent {
                let branch_name = branch.name.clone();
                let parent_name = parent.clone();

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
            }
        }
    }

    /// Set a status message (auto-clears after timeout)
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_set_at = Some(Instant::now());
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
}
