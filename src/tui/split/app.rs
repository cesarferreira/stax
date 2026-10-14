use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

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

/// Resolve `refname` to its OID via a git subprocess; `None` when absent.
fn resolve_ref_oid(workdir: &Path, refname: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", refname])
        .current_dir(workdir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

impl SplitApp {
    pub fn new() -> Result<Self> {
        let repo = GitRepo::open()?;
        let stack = Stack::load(&repo)?;
        let current_branch = repo.current_branch()?;

        // Get the parent branch
        let branch_info = stack
            .branches
            .get(&current_branch)
            .context("Current branch is not tracked. Use `stax branch track` first.")?;

        let parent_branch = branch_info
            .parent
            .clone()
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
        let parent_ref = repo
            .inner()
            .find_branch(parent, BranchType::Local)
            .with_context(|| format!("Branch '{}' not found", parent))?;
        let parent_oid = parent_ref.get().peel_to_commit()?.id();

        let branch_ref = repo
            .inner()
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
            let message = commit.summary().ok().flatten().unwrap_or("").to_string();

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
        !self
            .split_points
            .iter()
            .any(|sp| sp.after_commit_index == self.selected_index)
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
        if let Some(pos) = self
            .split_points
            .iter()
            .position(|sp| sp.after_commit_index == self.selected_index)
        {
            self.split_points.remove(pos);
            self.status_message = Some("Split point removed".to_string());
        }
    }

    /// Move split point at current position up (earlier in commits)
    pub fn move_split_up(&mut self) {
        if let Some(pos) = self
            .split_points
            .iter()
            .position(|sp| sp.after_commit_index == self.selected_index)
            && self.split_points[pos].after_commit_index > 0
        {
            // Check no conflict with adjacent split
            let new_idx = self.split_points[pos].after_commit_index - 1;
            if !self
                .split_points
                .iter()
                .any(|sp| sp.after_commit_index == new_idx)
            {
                self.split_points[pos].after_commit_index = new_idx;
                self.selected_index = new_idx;
                self.split_points.sort_by_key(|sp| sp.after_commit_index);
            }
        }
    }

    /// Move split point at current position down (later in commits)
    pub fn move_split_down(&mut self) {
        if let Some(pos) = self
            .split_points
            .iter()
            .position(|sp| sp.after_commit_index == self.selected_index)
        {
            let max_idx = self.commits.len().saturating_sub(2);
            if self.split_points[pos].after_commit_index < max_idx {
                let new_idx = self.split_points[pos].after_commit_index + 1;
                if !self
                    .split_points
                    .iter()
                    .any(|sp| sp.after_commit_index == new_idx)
                {
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
        let new_branches: Vec<String> = self
            .split_points
            .iter()
            .map(|sp| sp.branch_name.clone())
            .collect();

        // Begin transaction
        let mut tx = Transaction::begin(OpKind::Split, &self.repo, false)?;
        let mut affected = new_branches.clone();
        affected.push(self.current_branch.clone());
        tx.plan_branches(&self.repo, &affected)?;
        // Every branch touched below gets its metadata rewritten too -- plan those refs so
        // `stax undo` can actually reverse a split (see #830/#835).
        for branch in &affected {
            tx.plan_metadata_ref(&self.repo, branch)?;
        }

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
        let workdir = self.repo.workdir()?.to_path_buf();

        for sp in &self.split_points {
            // The commit at sp.after_commit_index becomes the tip of the new branch
            let commit_sha = &self.commits[sp.after_commit_index].sha;

            // Create branch at this commit
            self.repo
                .create_branch_at_commit(&sp.branch_name, commit_sha)?;

            // Create metadata for the new branch. `prev_parent`'s live tip is only a
            // valid boundary if this new branch's commits are actually descended from
            // it -- if `prev_parent` advanced between opening the split TUI and
            // confirming (or, for later split points, is simply a sibling of the new
            // branch rather than a real ancestor), it isn't. The merge-base is always
            // a real ancestor by definition; verify it before trusting the live tip,
            // else fall back to it directly as a last resort (see #830).
            let parent_rev = self.repo.branch_commit(&prev_parent)?;
            let merge_base = self.repo.merge_base(&prev_parent, &sp.branch_name).ok();
            let parent_branch_revision = self.repo.resolve_child_parent_boundary(
                &sp.branch_name,
                &[merge_base.as_deref(), Some(parent_rev.as_str())],
                &parent_rev,
            );
            let meta = BranchMetadata::new(&prev_parent, &parent_branch_revision);
            meta.write(self.repo.inner(), &sp.branch_name)?;

            tx.record_known_after(
                &sp.branch_name,
                resolve_ref_oid(&workdir, &format!("refs/heads/{}", sp.branch_name)).as_deref(),
            );
            tx.record_known_metadata_after(
                &sp.branch_name,
                resolve_ref_oid(
                    &workdir,
                    &format!("refs/branch-metadata/{}", sp.branch_name),
                )
                .as_deref(),
            );

            println!(
                "Created branch '{}' with {} commits",
                sp.branch_name,
                sp.after_commit_index - prev_idx + 1
            );

            prev_parent = sp.branch_name.clone();
            prev_idx = sp.after_commit_index + 1;
        }

        // Update current branch's parent to the last split branch
        if let Some(last_split) = self.split_points.last() {
            let new_parent = &last_split.branch_name;
            let parent_rev = self.repo.branch_commit(new_parent)?;

            // Read and update existing metadata
            if let Some(mut meta) = BranchMetadata::read(self.repo.inner(), &self.current_branch)? {
                let merge_base = self.repo.merge_base(new_parent, &self.current_branch).ok();
                meta.parent_branch_name = new_parent.clone();
                meta.parent_branch_revision = self.repo.resolve_child_parent_boundary(
                    &self.current_branch,
                    &[merge_base.as_deref(), Some(parent_rev.as_str())],
                    &meta.parent_branch_revision,
                );
                meta.write(self.repo.inner(), &self.current_branch)?;

                tx.record_known_metadata_after(
                    &self.current_branch,
                    resolve_ref_oid(
                        &workdir,
                        &format!("refs/branch-metadata/{}", self.current_branch),
                    )
                    .as_deref(),
                );
            }

            println!(
                "Updated '{}' parent to '{}'",
                self.current_branch, new_parent
            );
        }
        tx.record_known_after(
            &self.current_branch,
            resolve_ref_oid(&workdir, &format!("refs/heads/{}", self.current_branch)).as_deref(),
        );

        tx.finish_ok()?;
        println!("\nSplit complete! Use `stax status` to see the new stack structure.");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_hermetic(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env(
                "GIT_CONFIG_SYSTEM",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Issue #830: if `parent_branch` advances between when an interactive split
    /// session captured its commits and when the split is actually executed, the
    /// first new branch's `parentBranchRevision` must not be stamped with the
    /// live (advanced) tip -- that tip is not actually an ancestor of the new
    /// branch's commits, which were captured before the advance.
    #[test]
    fn execute_split_keeps_first_branchs_boundary_ancestry_valid_when_parent_advances_mid_session()
    {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        git_hermetic(dir, &["init", "-b", "main"]);
        git_hermetic(dir, &["config", "user.name", "Test"]);
        git_hermetic(dir, &["config", "user.email", "test@example.com"]);
        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "base"]);
        GitRepo::open_from_path(dir)
            .unwrap()
            .set_trunk("main")
            .unwrap();

        git_hermetic(dir, &["checkout", "-b", "feature"]);
        std::fs::write(dir.join("c1.txt"), "c1\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "C1"]);
        let c1_sha = git_hermetic(dir, &["rev-parse", "HEAD"]);
        std::fs::write(dir.join("c2.txt"), "c2\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "C2"]);

        {
            let repo = GitRepo::open_from_path(dir).unwrap();
            let main_oid = git_hermetic(dir, &["rev-parse", "main"]);
            BranchMetadata::new("main", &main_oid)
                .write(repo.inner(), "feature")
                .unwrap();
        }

        // Advance main AFTER "commits were captured" (simulating the TOCTOU gap
        // between opening the interactive split TUI and confirming the split).
        git_hermetic(dir, &["checkout", "main"]);
        std::fs::write(dir.join("m1.txt"), "m1\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "M1"]);
        git_hermetic(dir, &["checkout", "feature"]);

        let repo = GitRepo::open_from_path(dir).unwrap();
        let commits = vec![
            CommitDisplay {
                sha: c1_sha.clone(),
                short_sha: c1_sha[..7].to_string(),
                message: "C1".to_string(),
            },
            CommitDisplay {
                sha: git_hermetic(dir, &["rev-parse", "feature"]),
                short_sha: "unused".to_string(),
                message: "C2".to_string(),
            },
        ];
        let mut app = SplitApp {
            repo,
            current_branch: "feature".to_string(),
            parent_branch: "main".to_string(),
            commits,
            split_points: vec![SplitPoint {
                after_commit_index: 0,
                branch_name: "feature-split-1".to_string(),
            }],
            selected_index: 0,
            mode: SplitMode::Normal,
            input_buffer: String::new(),
            input_cursor: 0,
            status_message: None,
            should_quit: false,
            execute_requested: false,
            existing_branches: vec![],
        };

        app.execute_split().unwrap();

        let repo = GitRepo::open_from_path(dir).unwrap();
        let meta = BranchMetadata::read(repo.inner(), "feature-split-1")
            .unwrap()
            .unwrap();
        assert!(
            repo.is_ancestor(&meta.parent_branch_revision, "feature-split-1")
                .unwrap(),
            "boundary {} is not an ancestor of feature-split-1 (issue #830)",
            meta.parent_branch_revision
        );
    }

    /// Issue #835: a successful split must record the metadata refs it rewrites
    /// in the op receipt, or `stax undo` cannot reverse it.
    #[test]
    fn execute_split_records_metadata_refs_in_the_op_receipt() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        git_hermetic(dir, &["init", "-b", "main"]);
        git_hermetic(dir, &["config", "user.name", "Test"]);
        git_hermetic(dir, &["config", "user.email", "test@example.com"]);
        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "base"]);
        GitRepo::open_from_path(dir)
            .unwrap()
            .set_trunk("main")
            .unwrap();

        git_hermetic(dir, &["checkout", "-b", "feature"]);
        std::fs::write(dir.join("c1.txt"), "c1\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "C1"]);
        let c1_sha = git_hermetic(dir, &["rev-parse", "HEAD"]);
        std::fs::write(dir.join("c2.txt"), "c2\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "C2"]);
        let feature_tip = git_hermetic(dir, &["rev-parse", "HEAD"]);

        {
            let repo = GitRepo::open_from_path(dir).unwrap();
            let main_oid = git_hermetic(dir, &["rev-parse", "main"]);
            BranchMetadata::new("main", &main_oid)
                .write(repo.inner(), "feature")
                .unwrap();
        }
        let feature_meta_before = git_hermetic(dir, &["rev-parse", "refs/branch-metadata/feature"]);

        let repo = GitRepo::open_from_path(dir).unwrap();
        let commits = vec![
            CommitDisplay {
                sha: c1_sha.clone(),
                short_sha: c1_sha[..7].to_string(),
                message: "C1".to_string(),
            },
            CommitDisplay {
                sha: feature_tip,
                short_sha: "unused".to_string(),
                message: "C2".to_string(),
            },
        ];
        let mut app = SplitApp {
            repo,
            current_branch: "feature".to_string(),
            parent_branch: "main".to_string(),
            commits,
            split_points: vec![SplitPoint {
                after_commit_index: 0,
                branch_name: "feature-split-1".to_string(),
            }],
            selected_index: 0,
            mode: SplitMode::Normal,
            input_buffer: String::new(),
            input_cursor: 0,
            status_message: None,
            should_quit: false,
            execute_requested: false,
            existing_branches: vec![],
        };

        app.execute_split().unwrap();

        let repo = GitRepo::open_from_path(dir).unwrap();
        let receipt = crate::ops::receipt::OpReceipt::load_latest(repo.git_dir().unwrap())
            .unwrap()
            .expect("split should have written a receipt");

        let split_meta = receipt
            .local_refs
            .iter()
            .find(|e| e.branch == "feature-split-1@meta")
            .expect("feature-split-1@meta must be tracked");
        // `plan_metadata_ref` resolves via `refs::metadata_ref_oid`, which returns
        // `None` for a ref that doesn't exist yet -- confirmed by reading
        // `Transaction::plan_metadata_ref` in src/ops/tx.rs before writing this.
        assert_eq!(split_meta.oid_before, None);
        assert!(split_meta.oid_after.is_some());
        assert!(split_meta.after_recorded);

        let feature_meta_entry = receipt
            .local_refs
            .iter()
            .find(|e| e.branch == "feature@meta")
            .expect("feature@meta must be tracked");
        let feature_meta_after = git_hermetic(dir, &["rev-parse", "refs/branch-metadata/feature"]);
        assert_eq!(
            feature_meta_entry.oid_before.as_deref(),
            Some(feature_meta_before.as_str())
        );
        assert_eq!(
            feature_meta_entry.oid_after.as_deref(),
            Some(feature_meta_after.as_str())
        );
        assert!(feature_meta_entry.after_recorded);
    }
}
