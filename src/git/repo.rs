use anyhow::{Context, Result};
use git2::{BranchType, Repository};
use std::path::Path;
use std::process::Command;

pub struct GitRepo {
    repo: Repository,
}

impl GitRepo {
    /// Open the repository at the current directory or any parent
    pub fn open() -> Result<Self> {
        let repo = Repository::discover(".").context("Not in a git repository")?;
        Ok(Self { repo })
    }

    /// Get the repository root path
    pub fn workdir(&self) -> Result<&Path> {
        self.repo
            .workdir()
            .context("Repository has no working directory")
    }

    /// Get the .git directory path
    pub fn git_dir(&self) -> Result<&Path> {
        Ok(self.repo.path())
    }

    /// Get the current branch name
    pub fn current_branch(&self) -> Result<String> {
        let head = self.repo.head().context("Failed to get HEAD")?;
        let name = head
            .shorthand()
            .context("HEAD is not a branch")?
            .to_string();
        Ok(name)
    }

    /// Get all local branch names
    pub fn list_branches(&self) -> Result<Vec<String>> {
        let branches = self.repo.branches(Some(BranchType::Local))?;
        let mut names = Vec::new();
        for branch in branches {
            let (branch, _) = branch?;
            if let Some(name) = branch.name()? {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    /// Get the commit SHA for a branch
    pub fn branch_commit(&self, branch: &str) -> Result<String> {
        let reference = self
            .repo
            .find_branch(branch, BranchType::Local)
            .with_context(|| format!("Branch '{}' not found", branch))?;
        let commit = reference.get().peel_to_commit()?;
        Ok(commit.id().to_string())
    }

    /// Get commits ahead/behind between two branches (uses libgit2, no subprocess)
    pub fn commits_ahead_behind(&self, base: &str, head: &str) -> Result<(usize, usize)> {
        let base_oid = self.resolve_to_oid(base)?;
        let head_oid = self.resolve_to_oid(head)?;
        let (ahead, behind) = self.repo.graph_ahead_behind(head_oid, base_oid)?;
        Ok((ahead, behind))
    }

    /// Get commit messages between base and head (commits on head not in base)
    pub fn commits_between(&self, base: &str, head: &str) -> Result<Vec<String>> {
        let base_oid = self.resolve_to_oid(base)?;
        let head_oid = self.resolve_to_oid(head)?;

        let mut revwalk = self.repo.revwalk()?;
        revwalk.push(head_oid)?;
        revwalk.hide(base_oid)?;

        let mut commits = Vec::new();
        for oid in revwalk {
            let oid = oid?;
            if let Ok(commit) = self.repo.find_commit(oid) {
                if let Some(msg) = commit.summary() {
                    commits.push(msg.to_string());
                }
            }
        }

        Ok(commits)
    }

    /// Resolve a branch name or ref to an OID
    fn resolve_to_oid(&self, refspec: &str) -> Result<git2::Oid> {
        // Try as local branch first
        if let Ok(branch) = self.repo.find_branch(refspec, BranchType::Local) {
            if let Some(oid) = branch.get().target() {
                return Ok(oid);
            }
        }
        // Try as remote branch (e.g., "origin/main")
        if let Ok(branch) = self.repo.find_branch(refspec, BranchType::Remote) {
            if let Some(oid) = branch.get().target() {
                return Ok(oid);
            }
        }
        // Try as reference
        if let Ok(reference) = self.repo.find_reference(refspec) {
            if let Some(oid) = reference.target() {
                return Ok(oid);
            }
        }
        // Try revparse
        let obj = self.repo.revparse_single(refspec)?;
        Ok(obj.id())
    }

    /// Get the trunk branch name (from stored setting or auto-detect main/master)
    pub fn trunk_branch(&self) -> Result<String> {
        // First check if trunk is stored
        if let Some(trunk) = super::refs::read_trunk(&self.repo)? {
            return Ok(trunk);
        }
        // Fall back to auto-detection
        self.detect_trunk()
    }

    /// Auto-detect trunk branch (main or master)
    pub fn detect_trunk(&self) -> Result<String> {
        for name in ["main", "master"] {
            if self.repo.find_branch(name, BranchType::Local).is_ok() {
                return Ok(name.to_string());
            }
        }
        anyhow::bail!("No trunk branch (main/master) found")
    }

    /// Check if stax has been initialized in this repo
    pub fn is_initialized(&self) -> bool {
        super::refs::is_initialized(&self.repo)
    }

    /// Check if working tree has uncommitted changes
    pub fn is_dirty(&self) -> Result<bool> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(self.workdir()?)
            .output()
            .context("Failed to check git status")?;

        if !output.status.success() {
            anyhow::bail!("git status failed");
        }

        Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
    }

    /// Stash local changes (including untracked)
    pub fn stash_push(&self) -> Result<bool> {
        let output = Command::new("git")
            .args(["stash", "push", "-u", "-m", "stax auto-stash"])
            .current_dir(self.workdir()?)
            .output()
            .context("Failed to stash changes")?;

        if !output.status.success() {
            anyhow::bail!("git stash failed");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("No local changes") {
            return Ok(false);
        }

        Ok(true)
    }

    /// Pop the most recent stash
    pub fn stash_pop(&self) -> Result<()> {
        let status = Command::new("git")
            .args(["stash", "pop"])
            .current_dir(self.workdir()?)
            .status()
            .context("Failed to pop stash")?;

        if !status.success() {
            anyhow::bail!("git stash pop failed");
        }

        Ok(())
    }

    /// Set the trunk branch
    pub fn set_trunk(&self, trunk: &str) -> Result<()> {
        super::refs::write_trunk(&self.repo, trunk)
    }

    /// Checkout a branch
    pub fn checkout(&self, branch: &str) -> Result<()> {
        let status = Command::new("git")
            .args(["checkout", branch])
            .current_dir(self.workdir()?)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to run git checkout")?;

        if !status.success() {
            anyhow::bail!("git checkout failed");
        }
        Ok(())
    }

    /// Rebase current branch onto target
    pub fn rebase(&self, onto: &str) -> Result<RebaseResult> {
        let status = Command::new("git")
            .args(["rebase", onto])
            .current_dir(self.workdir()?)
            .status()
            .context("Failed to run git rebase")?;

        if status.success() {
            Ok(RebaseResult::Success)
        } else if self.rebase_in_progress()? {
            Ok(RebaseResult::Conflict)
        } else {
            anyhow::bail!("git rebase failed unexpectedly")
        }
    }

    /// Continue a rebase after resolving conflicts
    pub fn rebase_continue(&self) -> Result<RebaseResult> {
        let status = Command::new("git")
            .args(["rebase", "--continue"])
            .env("GIT_EDITOR", "true")
            .current_dir(self.workdir()?)
            .status()
            .context("Failed to run git rebase --continue")?;

        if status.success() {
            Ok(RebaseResult::Success)
        } else if self.rebase_in_progress()? {
            Ok(RebaseResult::Conflict)
        } else {
            anyhow::bail!("git rebase --continue failed")
        }
    }

    /// Check if a rebase is in progress
    pub fn rebase_in_progress(&self) -> Result<bool> {
        let git_dir = self.repo.path();
        Ok(git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists())
    }

    /// Create a new branch at HEAD
    pub fn create_branch(&self, name: &str) -> Result<()> {
        let head = self.repo.head()?;
        let commit = head.peel_to_commit()?;
        self.repo.branch(name, &commit, false)?;
        Ok(())
    }

    /// Create a new branch from another local branch
    pub fn create_branch_at(&self, name: &str, base_branch: &str) -> Result<()> {
        let reference = self
            .repo
            .find_branch(base_branch, BranchType::Local)
            .with_context(|| format!("Branch '{}' not found", base_branch))?;
        let commit = reference.get().peel_to_commit()?;
        self.repo.branch(name, &commit, false)?;
        Ok(())
    }

    /// Find merge-base commit between two local branches
    pub fn merge_base(&self, left: &str, right: &str) -> Result<String> {
        let left_commit = self
            .repo
            .find_branch(left, BranchType::Local)?
            .get()
            .peel_to_commit()?;
        let right_commit = self
            .repo
            .find_branch(right, BranchType::Local)?
            .get()
            .peel_to_commit()?;

        let base = self
            .repo
            .merge_base(left_commit.id(), right_commit.id())?;
        Ok(base.to_string())
    }

    /// Delete a branch
    pub fn delete_branch(&self, name: &str, force: bool) -> Result<()> {
        let mut branch = self.repo.find_branch(name, BranchType::Local)?;
        if force {
            branch.delete()?;
        } else {
            // Check if merged first
            let trunk = self.trunk_branch()?;
            let trunk_commit = self
                .repo
                .find_branch(&trunk, BranchType::Local)?
                .get()
                .peel_to_commit()?;
            let branch_commit = branch.get().peel_to_commit()?;

            if self
                .repo
                .merge_base(trunk_commit.id(), branch_commit.id())?
                == branch_commit.id()
            {
                branch.delete()?;
            } else {
                anyhow::bail!(
                    "Branch '{}' is not merged. Use --force to delete anyway.",
                    name
                );
            }
        }
        Ok(())
    }

    /// Get underlying repository (for advanced operations)
    pub fn inner(&self) -> &Repository {
        &self.repo
    }

    /// Get commits unique to a branch (not in parent)
    pub fn branch_commits(&self, branch: &str, parent: Option<&str>) -> Result<Vec<CommitInfo>> {
        let branch_ref = self.repo.find_branch(branch, BranchType::Local)?;
        let branch_commit = branch_ref.get().peel_to_commit()?;

        let mut commits = Vec::new();
        let mut revwalk = self.repo.revwalk()?;
        revwalk.push(branch_commit.id())?;

        // If parent specified, exclude its commits
        if let Some(parent_name) = parent {
            if let Ok(parent_ref) = self.repo.find_branch(parent_name, BranchType::Local) {
                if let Ok(parent_commit) = parent_ref.get().peel_to_commit() {
                    revwalk.hide(parent_commit.id())?;
                }
            }
        }

        for oid in revwalk.take(5) {
            // Max 5 commits
            let oid = oid?;
            let commit = self.repo.find_commit(oid)?;
            let message = commit.summary().unwrap_or("").to_string();
            let short_id = &oid.to_string()[..10];
            commits.push(CommitInfo {
                short_hash: short_id.to_string(),
                message,
            });
        }

        Ok(commits)
    }

    /// Get time since last commit on a branch
    pub fn branch_age(&self, branch: &str) -> Result<String> {
        let branch_ref = self.repo.find_branch(branch, BranchType::Local)?;
        let commit = branch_ref.get().peel_to_commit()?;
        let time = commit.time();
        let commit_ts = time.seconds();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let diff = now - commit_ts;

        Ok(format_duration(diff))
    }
    /// Check if a branch is merged into trunk
    pub fn is_branch_merged(&self, branch: &str) -> Result<bool> {
        let trunk = self.trunk_branch()?;
        let trunk_commit = self
            .repo
            .find_branch(&trunk, BranchType::Local)?
            .get()
            .peel_to_commit()?;
        let branch_commit = self
            .repo
            .find_branch(branch, BranchType::Local)?
            .get()
            .peel_to_commit()?;

        // Branch is merged if its commit is an ancestor of trunk
        Ok(self
            .repo
            .merge_base(trunk_commit.id(), branch_commit.id())?
            == branch_commit.id())
    }

    /// Check if a branch has a remote tracking branch (origin/<branch>)
    pub fn has_remote(&self, branch: &str) -> bool {
        let remote_name = format!("origin/{}", branch);
        self.repo.find_branch(&remote_name, BranchType::Remote).is_ok()
    }

    /// Get diff between a branch and its parent
    pub fn diff_against_parent(&self, branch: &str, parent: &str) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["diff", "--color=never", parent, branch])
            .current_dir(self.workdir()?)
            .output()
            .context("Failed to get diff")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let diff = String::from_utf8_lossy(&output.stdout);
        Ok(diff.lines().map(|s| s.to_string()).collect())
    }

    /// Get diff stat (numstat) between a branch and its parent
    pub fn diff_stat(&self, branch: &str, parent: &str) -> Result<Vec<(String, usize, usize)>> {
        let output = Command::new("git")
            .args(["diff", "--numstat", parent, branch])
            .current_dir(self.workdir()?)
            .output()
            .context("Failed to get diff stat")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stat = String::from_utf8_lossy(&output.stdout);
        let mut results = Vec::new();

        for line in stat.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let additions = parts[0].parse().unwrap_or(0);
                let deletions = parts[1].parse().unwrap_or(0);
                let file = parts[2].to_string();
                results.push((file, additions, deletions));
            }
        }

        Ok(results)
    }

    /// Get all branches that are merged into trunk (excluding trunk itself)
    pub fn merged_branches(&self) -> Result<Vec<String>> {
        let trunk = self.trunk_branch()?;
        let current = self.current_branch()?;
        let all_branches = self.list_branches()?;

        let mut merged = Vec::new();
        for branch in all_branches {
            if branch == trunk || branch == current {
                continue;
            }
            if self.is_branch_merged(&branch).unwrap_or(false) {
                merged.push(branch);
            }
        }
        Ok(merged)
    }
}

#[derive(Debug, PartialEq)]
pub enum RebaseResult {
    Success,
    Conflict,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub short_hash: String,
    pub message: String,
}

fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        "just now".to_string()
    } else if seconds < 3600 {
        let mins = seconds / 60;
        format!("{} minute{} ago", mins, if mins == 1 { "" } else { "s" })
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        format!("{} hour{} ago", hours, if hours == 1 { "" } else { "s" })
    } else {
        let days = seconds / 86400;
        format!("{} day{} ago", days, if days == 1 { "" } else { "s" })
    }
}
