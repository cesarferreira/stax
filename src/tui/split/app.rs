use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use anyhow::{Context, Result};

/// A commit to display in the split UI
#[derive(Debug, Clone)]
pub struct CommitDisplay {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
}

/// A split point marking where a new branch starts
#[derive(Debug, Clone)]
pub struct SplitPoint {
    /// Index of the commit AFTER which the split occurs (commits 0..=index go to this branch)
    pub after_commit_index: usize,
    /// Name of the new branch
    pub branch_name: String,
}

/// Preview of the resulting branch structure
#[derive(Debug, Clone)]
pub struct PreviewBranch {
    pub name: String,
    #[allow(dead_code)]
    pub parent: String,
    pub commit_count: usize,
}

/// Application mode
#[derive(Debug, Clone, PartialEq)]
pub enum SplitMode {
    Normal,
    Naming,
    Confirm,
    Help,
}

/// Main application state for split TUI
pub struct SplitApp {
    pub repo: GitRepo,
    pub current_branch: String,
    pub parent_branch: String,
    pub commits: Vec<CommitDisplay>,
    pub split_points: Vec<SplitPoint>,
    pub selected_index: usize,
    pub mode: SplitMode,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub status_message: Option<String>,
    pub should_quit: bool,
    pub execute_requested: bool,
    pub existing_branches: Vec<String>,
}

impl SplitApp {
    pub fn new() -> Result<Self> {
        let repo = GitRepo::open()?;
        let stack = Stack::load(&repo)?;
        let current_branch = repo.current_branch()?;

        // Get the parent branch
        let branch_info = stack.branches.get(&current_branch)
            .context("Current branch is not tracked. Use `stax branch track` first.")?;

        let parent_branch = branch_info.parent.clone()
            .context("Current branch has no parent (is it trunk?)")?;

        // Get commits between parent and current
        let commits = Self::load_commits(&repo, &parent_branch, &current_branch)?;

        if commits.is_empty() {
            anyhow::bail!("No commits to split. Branch has no commits above parent.");
        }

        // Get existing branch names for validation
        let existing_branches = repo.list_branches()?;

        Ok(Self {
            repo,
            current_branch,
            parent_branch,
            commits,
            split_points: Vec::new(),
            selected_index: 0,
            mode: SplitMode::Normal,
            input_buffer: String::new(),
            input_cursor: 0,
            status_message: None,
            should_quit: false,
            execute_requested: false,
            existing_branches,
        })
    }

    fn load_commits(repo: &GitRepo, parent: &str, branch: &str) -> Result<Vec<CommitDisplay>> {
        use git2::BranchType;

        // Get OIDs from branch references
        let parent_ref = repo.inner()
            .find_branch(parent, BranchType::Local)
            .with_context(|| format!("Branch '{}' not found", parent))?;
        let parent_oid = parent_ref.get().peel_to_commit()?.id();

        let branch_ref = repo.inner()
            .find_branch(branch, BranchType::Local)
            .with_context(|| format!("Branch '{}' not found", branch))?;
        let branch_oid = branch_ref.get().peel_to_commit()?.id();

        let mut revwalk = repo.inner().revwalk()?;
        revwalk.push(branch_oid)?;
        revwalk.hide(parent_oid)?;

        let mut commits = Vec::new();
        for oid in revwalk {
            let oid = oid?;
            let commit = repo.inner().find_commit(oid)?;
            let short_sha = &oid.to_string()[..7];
            let message = commit.summary().unwrap_or("").to_string();

            commits.push(CommitDisplay {
                sha: oid.to_string(),
                short_sha: short_sha.to_string(),
                message,
            });
        }

        // Reverse to get oldest first (parent-adjacent at index 0)
        commits.reverse();
        Ok(commits)
    }

    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn select_next(&mut self) {
        if self.selected_index < self.commits.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Check if we can add a split point at the current position
    pub fn can_split_at_current(&self) -> bool {
        // Can't split after the last commit (that would be empty)
        if self.selected_index >= self.commits.len().saturating_sub(1) {
            return false;
        }
        // Can't split if there's already a split point here
        !self.split_points.iter().any(|sp| sp.after_commit_index == self.selected_index)
    }

    /// Add a split point at the current position
    pub fn add_split_at_current(&mut self, branch_name: String) {
        let split = SplitPoint {
            after_commit_index: self.selected_index,
            branch_name,
        };
        self.split_points.push(split);
        // Keep sorted by index
        self.split_points.sort_by_key(|sp| sp.after_commit_index);
        self.status_message = Some("Split point added".to_string());
    }

    /// Remove split point at current position if one exists
    pub fn remove_split_at_current(&mut self) {
        if let Some(pos) = self.split_points.iter().position(|sp| sp.after_commit_index == self.selected_index) {
            self.split_points.remove(pos);
            self.status_message = Some("Split point removed".to_string());
        }
    }

    /// Move split point at current position up (earlier in commits)
    pub fn move_split_up(&mut self) {
        if let Some(pos) = self.split_points.iter().position(|sp| sp.after_commit_index == self.selected_index) {
            if self.split_points[pos].after_commit_index > 0 {
                // Check no conflict with adjacent split
                let new_idx = self.split_points[pos].after_commit_index - 1;
                if !self.split_points.iter().any(|sp| sp.after_commit_index == new_idx) {
                    self.split_points[pos].after_commit_index = new_idx;
                    self.selected_index = new_idx;
                    self.split_points.sort_by_key(|sp| sp.after_commit_index);
                }
            }
        }
    }

    /// Move split point at current position down (later in commits)
    pub fn move_split_down(&mut self) {
        if let Some(pos) = self.split_points.iter().position(|sp| sp.after_commit_index == self.selected_index) {
            let max_idx = self.commits.len().saturating_sub(2);
            if self.split_points[pos].after_commit_index < max_idx {
                let new_idx = self.split_points[pos].after_commit_index + 1;
                if !self.split_points.iter().any(|sp| sp.after_commit_index == new_idx) {
                    self.split_points[pos].after_commit_index = new_idx;
                    self.selected_index = new_idx;
                    self.split_points.sort_by_key(|sp| sp.after_commit_index);
                }
            }
        }
    }

    /// Check if a branch name already exists
    pub fn branch_name_exists(&self, name: &str) -> bool {
        self.existing_branches.iter().any(|b| b == name)
            || self.split_points.iter().any(|sp| sp.branch_name == name)
    }

    /// Build preview of the resulting branch structure
    pub fn build_preview(&self) -> Vec<PreviewBranch> {
        let mut preview = Vec::new();
        let mut prev_parent = self.parent_branch.clone();
        let mut prev_idx = 0;

        for sp in &self.split_points {
            let commit_count = sp.after_commit_index - prev_idx + 1;
            preview.push(PreviewBranch {
                name: sp.branch_name.clone(),
                parent: prev_parent.clone(),
                commit_count,
            });
            prev_parent = sp.branch_name.clone();
            prev_idx = sp.after_commit_index + 1;
        }

        // Add the current branch (remaining commits)
        let remaining = self.commits.len() - prev_idx;
        if remaining > 0 {
            preview.push(PreviewBranch {
                name: self.current_branch.clone(),
                parent: prev_parent,
                commit_count: remaining,
            });
        }

        preview
    }

    /// Execute the split operation
    pub fn execute_split(&mut self) -> Result<()> {
        if self.split_points.is_empty() {
            return Ok(());
        }

        let config = Config::load()?;
        let _ = config; // Reserved for future branch name formatting

        // Collect branch names to create
        let new_branches: Vec<String> = self.split_points.iter()
            .map(|sp| sp.branch_name.clone())
            .collect();

        // Begin transaction
        let mut tx = Transaction::begin(OpKind::Split, &self.repo, false)?;
        let mut affected = new_branches.clone();
        affected.push(self.current_branch.clone());
        tx.plan_branches(&self.repo, &affected)?;

        let summary = PlanSummary {
            branches_to_rebase: 0,
            branches_to_push: 0,
            description: vec![format!("Split into {} new branches", new_branches.len())],
        };
        tx::print_plan(tx.kind(), &summary, false);
        tx.set_plan_summary(summary);
        tx.snapshot()?;

        // Create branches at split points
        let mut prev_parent = self.parent_branch.clone();
        let mut prev_idx = 0;

        for sp in &self.split_points {
            // The commit at sp.after_commit_index becomes the tip of the new branch
            let commit_sha = &self.commits[sp.after_commit_index].sha;

            // Create branch at this commit
            self.repo.create_branch_at_commit(&sp.branch_name, commit_sha)?;

            // Create metadata for the new branch
            let parent_rev = self.repo.branch_commit(&prev_parent)?;
            let meta = BranchMetadata::new(&prev_parent, &parent_rev);
            meta.write(self.repo.inner(), &sp.branch_name)?;

            println!("Created branch '{}' with {} commits",
                sp.branch_name,
                sp.after_commit_index - prev_idx + 1);

            prev_parent = sp.branch_name.clone();
            prev_idx = sp.after_commit_index + 1;
        }

        // Update current branch's parent to the last split branch
        if let Some(last_split) = self.split_points.last() {
            let new_parent = &last_split.branch_name;
            let parent_rev = self.repo.branch_commit(new_parent)?;

            // Read and update existing metadata
            if let Some(mut meta) = BranchMetadata::read(self.repo.inner(), &self.current_branch)? {
                meta.parent_branch_name = new_parent.clone();
                meta.parent_branch_revision = parent_rev;
                meta.write(self.repo.inner(), &self.current_branch)?;
            }

            println!("Updated '{}' parent to '{}'", self.current_branch, new_parent);
        }

        tx.finish_ok()?;
        println!("\nSplit complete! Use `stax status` to see the new stack structure.");

        Ok(())
    }
}
