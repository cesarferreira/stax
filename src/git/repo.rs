use anyhow::{Context, Result};
use git2::{BranchType, Repository};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub struct GitRepo {
    repo: Repository,
}

#[derive(Debug, Clone)]
struct WorktreeInfo {
    path: PathBuf,
    branch: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BranchParentMetadata {
    parent_branch_name: String,
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

    fn run_git(&self, cwd: &Path, args: &[&str]) -> Result<Output> {
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .with_context(|| format!("Failed to run git {}", args.join(" ")))
    }

    fn normalize_path(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn current_branch_in_path(&self, cwd: &Path) -> Result<String> {
        let output = self.run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!(
                "git rev-parse --abbrev-ref HEAD failed in '{}': {}",
                cwd.display(),
                stderr
            );
        }

        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() || branch == "HEAD" {
            anyhow::bail!(
                "HEAD is detached in '{}'. Please checkout a branch first.",
                cwd.display()
            );
        }
        Ok(branch)
    }

    fn git_dir_in_path(&self, cwd: &Path) -> Result<PathBuf> {
        let output = self.run_git(cwd, &["rev-parse", "--git-dir"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!(
                "git rev-parse --git-dir failed in '{}': {}",
                cwd.display(),
                stderr
            );
        }

        let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if git_dir.is_empty() {
            anyhow::bail!("git rev-parse --git-dir returned empty output");
        }

        let path = PathBuf::from(git_dir);
        if path.is_absolute() {
            Ok(path)
        } else {
            Ok(cwd.join(path))
        }
    }

    fn rebase_in_progress_at(&self, cwd: &Path) -> Result<bool> {
        let git_dir = self.git_dir_in_path(cwd)?;
        Ok(git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists())
    }

    pub(crate) fn is_dirty_at(&self, cwd: &Path) -> Result<bool> {
        let output = self.run_git(cwd, &["status", "--porcelain"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git status failed in '{}': {}", cwd.display(), stderr);
        }
        Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
    }

    pub(crate) fn stash_push_at(&self, cwd: &Path) -> Result<bool> {
        let output = self.run_git(cwd, &["stash", "push", "-u", "-m", "stax auto-stash"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git stash failed in '{}': {}", cwd.display(), stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("No local changes") {
            return Ok(false);
        }
        Ok(true)
    }

    pub(crate) fn stash_pop_at(&self, cwd: &Path) -> Result<()> {
        let output = self.run_git(cwd, &["stash", "pop"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git stash pop failed in '{}': {}", cwd.display(), stderr);
        }
        Ok(())
    }

    fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>> {
        let output = self.run_git(self.workdir()?, &["worktree", "list", "--porcelain"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git worktree list failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut worktrees = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        let mut current_branch: Option<String> = None;

        let mut flush_entry = |path: &mut Option<PathBuf>, branch: &mut Option<String>| {
            if let Some(p) = path.take() {
                worktrees.push(WorktreeInfo {
                    path: Self::normalize_path(&p),
                    branch: branch.take(),
                });
            }
        };

        for line in stdout.lines() {
            if line.is_empty() {
                flush_entry(&mut current_path, &mut current_branch);
                continue;
            }

            if let Some(path) = line.strip_prefix("worktree ") {
                flush_entry(&mut current_path, &mut current_branch);
                current_path = Some(PathBuf::from(path.trim()));
                continue;
            }

            if let Some(branch) = line.strip_prefix("branch ") {
                let branch = branch
                    .trim()
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch.trim())
                    .to_string();
                current_branch = Some(branch);
            }
        }

        flush_entry(&mut current_path, &mut current_branch);
        Ok(worktrees)
    }

    pub fn branch_worktree_path(&self, branch: &str) -> Result<Option<PathBuf>> {
        for worktree in self.list_worktrees()? {
            if worktree.branch.as_deref() == Some(branch) {
                return Ok(Some(worktree.path));
            }
        }
        Ok(None)
    }

    /// Get the current branch name
    pub fn current_branch(&self) -> Result<String> {
        let head = self.repo.head().context("Failed to get HEAD")?;

        // Check if HEAD is detached
        if !head.is_branch() {
            anyhow::bail!(
                "HEAD is detached (not on a branch).\n\
                 Please checkout a branch first: stax checkout <branch>"
            );
        }

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

    /// Resolve any ref (local branch, remote branch, SHA) to a commit SHA string.
    /// Useful for resolving refs like "origin/main" to their current commit.
    pub fn resolve_ref(&self, refspec: &str) -> Result<String> {
        let oid = self.resolve_to_oid(refspec)?;
        Ok(oid.to_string())
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
        self.is_dirty_at(self.workdir()?)
    }

    /// Stash local changes (including untracked)
    pub fn stash_push(&self) -> Result<bool> {
        self.stash_push_at(self.workdir()?)
    }

    /// Pop the most recent stash
    pub fn stash_pop(&self) -> Result<()> {
        self.stash_pop_at(self.workdir()?)
    }

    /// Set the trunk branch
    pub fn set_trunk(&self, trunk: &str) -> Result<()> {
        super::refs::write_trunk(&self.repo, trunk)
    }

    /// Checkout a branch
    pub fn checkout(&self, branch: &str) -> Result<()> {
        let output = self.run_git(self.workdir()?, &["checkout", branch])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git checkout {} failed: {}", branch, stderr);
        }
        Ok(())
    }

    fn rebase_in_path(&self, cwd: &Path, onto: &str) -> Result<RebaseResult> {
        let output = self.run_git(cwd, &["rebase", onto])?;
        if output.status.success() {
            return Ok(RebaseResult::Success);
        }

        if self.rebase_in_progress_at(cwd)? {
            return Ok(RebaseResult::Conflict);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "git rebase {} failed in '{}': {}",
            onto,
            cwd.display(),
            stderr
        );
    }

    /// Rebase current branch onto target
    #[allow(dead_code)] // Kept for compatibility with existing APIs and future command flows
    pub fn rebase(&self, onto: &str) -> Result<RebaseResult> {
        self.rebase_in_path(self.workdir()?, onto)
    }

    /// Rebase the target branch onto another branch, using the branch's owning worktree.
    /// If the target branch is not checked out in any worktree, it falls back to current workdir.
    pub fn rebase_branch_onto(
        &self,
        branch: &str,
        onto: &str,
        auto_stash_pop: bool,
    ) -> Result<RebaseResult> {
        let current_workdir = Self::normalize_path(self.workdir()?);
        let target_workdir = self
            .branch_worktree_path(branch)?
            .unwrap_or_else(|| current_workdir.clone());
        let target_workdir = Self::normalize_path(&target_workdir);

        // If rebasing in the current worktree and we're not already on target, checkout first.
        if target_workdir == current_workdir {
            if self.current_branch()? != branch {
                self.checkout(branch)?;
            }
        } else {
            // Validate that the expected branch is actually checked out in that worktree.
            let current_in_target = self.current_branch_in_path(&target_workdir)?;
            if current_in_target != branch {
                anyhow::bail!(
                    "Expected '{}' in '{}', found '{}' instead.",
                    branch,
                    target_workdir.display(),
                    current_in_target
                );
            }
        }

        let mut stashed = false;
        if self.is_dirty_at(&target_workdir)? {
            if !auto_stash_pop {
                anyhow::bail!(
                    "Cannot restack '{}': worktree '{}' has uncommitted changes. \
Use --auto-stash-pop or stash/commit changes first.",
                    branch,
                    target_workdir.display()
                );
            }
            stashed = self.stash_push_at(&target_workdir)?;
        }

        let result = match self.rebase_in_path(&target_workdir, onto).with_context(|| {
            format!(
                "Failed to rebase '{}' onto '{}' in '{}'",
                branch,
                onto,
                target_workdir.display()
            )
        }) {
            Ok(result) => result,
            Err(err) => {
                if stashed {
                    return Err(err.context(format!(
                        "Auto-stash was kept in '{}' due to rebase failure.",
                        target_workdir.display()
                    )));
                }
                return Err(err);
            }
        };

        if stashed && result == RebaseResult::Success {
            self.stash_pop_at(&target_workdir).with_context(|| {
                format!(
                    "Rebased '{}' successfully, but failed to auto-pop stash in '{}'",
                    branch,
                    target_workdir.display()
                )
            })?;
        }

        Ok(result)
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
        self.rebase_in_progress_at(self.workdir()?)
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

    /// Create a new branch at a specific commit SHA
    pub fn create_branch_at_commit(&self, name: &str, commit_sha: &str) -> Result<()> {
        let oid = git2::Oid::from_str(commit_sha)
            .with_context(|| format!("Invalid commit SHA: {}", commit_sha))?;
        let commit = self
            .repo
            .find_commit(oid)
            .with_context(|| format!("Commit '{}' not found", commit_sha))?;
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

        let base = self.repo.merge_base(left_commit.id(), right_commit.id())?;
        Ok(base.to_string())
    }

    /// Delete a branch
    pub fn delete_branch(&self, name: &str, force: bool) -> Result<()> {
        let mut branch = self.repo.find_branch(name, BranchType::Local)?;
        if force {
            branch.delete()?;
        } else {
            let branch_commit = branch.get().peel_to_commit()?;
            let mut candidate_bases = vec![self.trunk_branch()?];

            if let Ok(Some(json)) = crate::git::refs::read_metadata(&self.repo, name) {
                if let Ok(meta) = serde_json::from_str::<BranchParentMetadata>(&json) {
                    if meta.parent_branch_name != name
                        && !candidate_bases.contains(&meta.parent_branch_name)
                    {
                        candidate_bases.insert(0, meta.parent_branch_name);
                    }
                }
            }

            let merged_into_any_base = candidate_bases.into_iter().any(|base| {
                let Ok(base_branch) = self.repo.find_branch(&base, BranchType::Local) else {
                    return false;
                };
                let Ok(base_commit) = base_branch.get().peel_to_commit() else {
                    return false;
                };
                self.repo
                    .merge_base(base_commit.id(), branch_commit.id())
                    .map(|base_oid| base_oid == branch_commit.id())
                    .unwrap_or(false)
            });

            if !merged_into_any_base {
                anyhow::bail!(
                    "Branch '{}' is not merged. Use --force to delete anyway.",
                    name
                );
            }

            branch.delete()?;
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

    /// Get recent commits on a branch within the last N hours
    /// Returns (branch_name, commit_count, most_recent_age)
    pub fn recent_branch_activity(
        &self,
        branch: &str,
        hours: i64,
    ) -> Result<Option<(usize, String)>> {
        let workdir = self.workdir()?;
        let since_arg = format!("--since={} hours ago", hours);

        let output = Command::new("git")
            .args(["log", &since_arg, "--oneline", branch])
            .current_dir(workdir)
            .output()
            .context("Failed to run git log")?;

        if !output.status.success() {
            return Ok(None);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let commit_count = stdout.lines().filter(|l| !l.is_empty()).count();

        if commit_count == 0 {
            return Ok(None);
        }

        // Get the age of the most recent commit
        let age = self.branch_age(branch).ok();

        Ok(Some((
            commit_count,
            age.unwrap_or_else(|| "recently".to_string()),
        )))
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
        self.repo
            .find_branch(&remote_name, BranchType::Remote)
            .is_ok()
    }

    /// Get commits ahead/behind compared to remote tracking branch (origin/branch)
    /// Returns (unpushed, unpulled) or None if no remote tracking branch exists
    pub fn commits_vs_remote(&self, branch: &str) -> Option<(usize, usize)> {
        let remote_name = format!("origin/{}", branch);
        if self
            .repo
            .find_branch(&remote_name, BranchType::Remote)
            .is_ok()
        {
            self.commits_ahead_behind(&remote_name, branch).ok()
        } else {
            None
        }
    }

    /// Get diff between a branch and its parent
    pub fn diff_against_parent(&self, branch: &str, parent: &str) -> Result<Vec<String>> {
        // Use merge-base diff (A...B) to match PR semantics and avoid showing unrelated
        // parent-side changes when the parent branch has advanced.
        let range = format!("{}...{}", parent, branch);
        let output = Command::new("git")
            .args(["diff", "--color=never", &range])
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
        let range = format!("{}...{}", parent, branch);
        let output = Command::new("git")
            .args(["diff", "--numstat", &range])
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

    /// Check if rebasing a branch onto target would produce conflicts
    /// Uses git merge-tree to detect potential conflicts without actually rebasing
    /// Returns a list of files that would have conflicts
    pub fn check_rebase_conflicts(&self, branch: &str, onto: &str) -> Result<Vec<String>> {
        // Get the merge base between the branch and onto target
        let merge_base = match self.merge_base(onto, branch) {
            Ok(base) => base,
            Err(_) => return Ok(Vec::new()),
        };

        // Use git merge-tree to check for conflicts
        // git merge-tree --write-tree <base> <onto> <branch>
        let output = Command::new("git")
            .args([
                "merge-tree",
                "--write-tree",
                "--no-messages",
                &merge_base,
                onto,
                branch,
            ])
            .current_dir(self.workdir()?)
            .output()
            .context("Failed to run git merge-tree")?;

        // If the command fails (non-zero exit), there are conflicts
        // The output will contain the conflicting files
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Parse conflict information from output
            let mut conflict_files = Vec::new();

            // The output format typically shows conflicting files
            for line in stdout.lines().chain(stderr.lines()) {
                // Look for lines indicating conflicts (file paths in merge conflicts)
                if line.contains("CONFLICT") {
                    // Extract filename from conflict message
                    // Format: "CONFLICT (content): Merge conflict in <file>"
                    if let Some(file) = line.split("Merge conflict in ").nth(1) {
                        conflict_files.push(file.trim().to_string());
                    } else if let Some(file) = line.split("CONFLICT (").nth(1) {
                        // Other conflict formats
                        if let Some(f) = file.split("):").nth(1) {
                            conflict_files.push(f.trim().to_string());
                        }
                    }
                }
            }

            return Ok(conflict_files);
        }

        Ok(Vec::new())
    }

    /// Get files modified in a branch compared to its parent
    #[allow(dead_code)] // Reserved for future conflict detection improvements
    pub fn files_modified(&self, branch: &str, parent: &str) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["diff", "--name-only", parent, branch])
            .current_dir(self.workdir()?)
            .output()
            .context("Failed to get modified files")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let files = String::from_utf8_lossy(&output.stdout);
        Ok(files.lines().map(|s| s.to_string()).collect())
    }

    /// Check for overlapping files between two branches that could cause conflicts
    #[allow(dead_code)] // Reserved for future conflict detection improvements
    pub fn check_overlapping_files(
        &self,
        branch1: &str,
        branch2: &str,
        common_parent: &str,
    ) -> Result<Vec<String>> {
        let files1 = self.files_modified(branch1, common_parent)?;
        let files2 = self.files_modified(branch2, common_parent)?;

        let files1_set: std::collections::HashSet<_> = files1.into_iter().collect();
        let overlapping: Vec<String> = files2
            .into_iter()
            .filter(|f| files1_set.contains(f))
            .collect();

        Ok(overlapping)
    }

    /// Abort an in-progress rebase
    pub fn rebase_abort(&self) -> Result<()> {
        if !self.rebase_in_progress()? {
            return Ok(());
        }

        let status = Command::new("git")
            .args(["rebase", "--abort"])
            .current_dir(self.workdir()?)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to run git rebase --abort")?;

        if !status.success() {
            anyhow::bail!("git rebase --abort failed");
        }
        Ok(())
    }

    /// Update a ref to point to a specific OID
    pub fn update_ref(&self, refname: &str, oid: &str) -> Result<()> {
        let status = Command::new("git")
            .args(["update-ref", refname, oid])
            .current_dir(self.workdir()?)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to run git update-ref")?;

        if !status.success() {
            anyhow::bail!("git update-ref {} {} failed", refname, oid);
        }
        Ok(())
    }

    /// Delete a ref
    pub fn delete_ref(&self, refname: &str) -> Result<()> {
        let status = Command::new("git")
            .args(["update-ref", "-d", refname])
            .current_dir(self.workdir()?)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to run git update-ref -d")?;

        if !status.success() {
            anyhow::bail!("git update-ref -d {} failed", refname);
        }
        Ok(())
    }

    /// Resolve a refspec to an OID (git rev-parse)
    pub fn rev_parse(&self, refspec: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", refspec])
            .current_dir(self.workdir()?)
            .output()
            .context("Failed to run git rev-parse")?;

        if !output.status.success() {
            anyhow::bail!("git rev-parse {} failed", refspec);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Force push a branch to remote
    pub fn force_push(&self, remote: &str, branch: &str) -> Result<()> {
        let status = Command::new("git")
            .args(["push", "-f", remote, branch])
            .current_dir(self.workdir()?)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to run git push -f")?;

        if !status.success() {
            anyhow::bail!("git push -f {} {} failed", remote, branch);
        }
        Ok(())
    }

    /// Hard reset to a specific ref/OID
    pub fn reset_hard(&self, target: &str) -> Result<()> {
        let status = Command::new("git")
            .args(["reset", "--hard", target])
            .current_dir(self.workdir()?)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to run git reset --hard")?;

        if !status.success() {
            anyhow::bail!("git reset --hard {} failed", target);
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn run_git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("failed to run git");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn test_format_duration_just_now() {
        assert_eq!(format_duration(0), "just now");
        assert_eq!(format_duration(30), "just now");
        assert_eq!(format_duration(59), "just now");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1 minute ago");
        assert_eq!(format_duration(120), "2 minutes ago");
        assert_eq!(format_duration(3599), "59 minutes ago");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1 hour ago");
        assert_eq!(format_duration(7200), "2 hours ago");
        assert_eq!(format_duration(86399), "23 hours ago");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(86400), "1 day ago");
        assert_eq!(format_duration(172800), "2 days ago");
        assert_eq!(format_duration(604800), "7 days ago");
    }

    #[test]
    fn test_rebase_result_eq() {
        assert_eq!(RebaseResult::Success, RebaseResult::Success);
        assert_eq!(RebaseResult::Conflict, RebaseResult::Conflict);
        assert_ne!(RebaseResult::Success, RebaseResult::Conflict);
    }

    #[test]
    fn test_rebase_result_debug() {
        let result = RebaseResult::Success;
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Success"));

        let result = RebaseResult::Conflict;
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Conflict"));
    }

    #[test]
    fn test_commit_info_clone() {
        let commit = CommitInfo {
            short_hash: "abc123".to_string(),
            message: "Test commit".to_string(),
        };
        let cloned = commit.clone();
        assert_eq!(cloned.short_hash, "abc123");
        assert_eq!(cloned.message, "Test commit");
    }

    #[test]
    fn test_commit_info_debug() {
        let commit = CommitInfo {
            short_hash: "abc123".to_string(),
            message: "Test commit".to_string(),
        };
        let debug_str = format!("{:?}", commit);
        assert!(debug_str.contains("abc123"));
        assert!(debug_str.contains("Test commit"));
    }

    #[test]
    fn test_delete_branch_non_force_allows_empty_branch_merged_into_parent() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path();

        run_git(path, &["init", "-b", "main"]);
        run_git(path, &["config", "user.email", "test@example.com"]);
        run_git(path, &["config", "user.name", "Test User"]);

        fs::write(path.join("README.md"), "# repo\n").expect("write readme");
        run_git(path, &["add", "README.md"]);
        run_git(path, &["commit", "-m", "Initial commit"]);

        run_git(path, &["checkout", "-b", "parent"]);
        fs::write(path.join("parent.txt"), "parent change\n").expect("write parent");
        run_git(path, &["add", "parent.txt"]);
        run_git(path, &["commit", "-m", "Parent commit"]);

        run_git(path, &["checkout", "-b", "child"]);
        run_git(path, &["checkout", "parent"]);

        let repo = GitRepo {
            repo: Repository::open(path).expect("open repo"),
        };

        crate::git::refs::write_metadata(
            &repo.repo,
            "child",
            r#"{"parentBranchName":"parent","parentBranchRevision":"ignored"}"#,
        )
        .expect("write metadata");

        repo.delete_branch("child", false)
            .expect("delete should succeed without force");
        assert!(repo.repo.find_branch("child", BranchType::Local).is_err());
    }
}
