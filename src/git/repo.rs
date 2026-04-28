use anyhow::{Context, Result};
use git2::{BranchType, Repository};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

pub struct GitRepo {
    repo: Repository,
}

fn normalize_local_branch_name(branch: &str) -> &str {
    branch.strip_prefix("refs/heads/").unwrap_or(branch)
}

fn parse_branch_vv_worktree_path(line: &str) -> Option<PathBuf> {
    let start = line.find(" (")?;
    let rest = &line[start + 2..];
    let end = rest.find(')')?;
    Some(PathBuf::from(&rest[..end]))
}

fn worktree_basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string())
}

fn branch_leaf(branch: &str) -> String {
    branch.rsplit('/').next().unwrap_or(branch).to_string()
}

fn path_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            Component::Prefix(value) => Some(value.as_os_str().to_string_lossy().into_owned()),
            Component::RootDir | Component::CurDir | Component::ParentDir => None,
        })
        .collect()
}

fn path_suffix_label(path: &Path, depth: usize) -> String {
    let components = path_components(path);
    let start = components.len().saturating_sub(depth);
    components[start..].join("/")
}

fn minimal_unique_path_suffix(
    path: &Path,
    all_paths: &[PathBuf],
    reserved: &HashSet<String>,
) -> String {
    let component_count = path_components(path).len();
    for depth in 1..=component_count {
        let candidate = path_suffix_label(path, depth);
        if reserved.contains(&candidate) {
            continue;
        }

        let matches = all_paths
            .iter()
            .filter(|other| path_suffix_label(other, depth) == candidate)
            .count();
        if matches == 1 {
            return candidate;
        }
    }

    path.display().to_string()
}

#[derive(Debug, Clone)]
struct WorktreeLabelCandidate {
    is_main: bool,
    path: PathBuf,
    basename: String,
    branch: Option<String>,
}

fn assign_unique_label_stage<F>(
    labels: &mut [Option<String>],
    reserved: &mut HashSet<String>,
    candidates: &[WorktreeLabelCandidate],
    mut label_for: F,
) where
    F: FnMut(&WorktreeLabelCandidate) -> Option<String>,
{
    let mut counts = HashMap::new();
    for (idx, candidate) in candidates.iter().enumerate() {
        if labels[idx].is_some() {
            continue;
        }

        if let Some(label) = label_for(candidate) {
            if !reserved.contains(&label) {
                *counts.entry(label).or_insert(0usize) += 1;
            }
        }
    }

    for (idx, candidate) in candidates.iter().enumerate() {
        if labels[idx].is_some() {
            continue;
        }

        let Some(label) = label_for(candidate) else {
            continue;
        };

        if reserved.contains(&label) || counts.get(&label).copied() != Some(1) {
            continue;
        }

        reserved.insert(label.clone());
        labels[idx] = Some(label);
    }
}

fn derive_worktree_names(candidates: &[WorktreeLabelCandidate]) -> Vec<String> {
    let mut labels = vec![None; candidates.len()];
    let mut reserved = HashSet::new();

    for (idx, candidate) in candidates.iter().enumerate() {
        if candidate.is_main {
            labels[idx] = Some("main".to_string());
            reserved.insert("main".to_string());
        }
    }

    assign_unique_label_stage(&mut labels, &mut reserved, candidates, |candidate| {
        (!candidate.is_main).then(|| candidate.basename.clone())
    });
    assign_unique_label_stage(&mut labels, &mut reserved, candidates, |candidate| {
        candidate.branch.as_deref().map(branch_leaf)
    });
    assign_unique_label_stage(&mut labels, &mut reserved, candidates, |candidate| {
        candidate.branch.clone()
    });

    let all_paths = candidates
        .iter()
        .map(|candidate| candidate.path.clone())
        .collect::<Vec<_>>();
    for (idx, candidate) in candidates.iter().enumerate() {
        if labels[idx].is_none() {
            let label = minimal_unique_path_suffix(&candidate.path, &all_paths, &reserved);
            reserved.insert(label.clone());
            labels[idx] = Some(label);
        }
    }

    labels
        .into_iter()
        .map(|label| label.expect("every worktree should get a label"))
        .collect()
}

fn run_git_in(cwd: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))
}

pub fn checkout_branch_in(workdir: &Path, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["checkout", branch])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to run git checkout {}", branch))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("git checkout {} failed: {}", branch, stderr);
    }

    Ok(())
}

pub fn local_branch_exists_in(workdir: &Path, branch: &str) -> bool {
    let local_ref = format!("refs/heads/{}", branch);
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet", &local_ref])
        .current_dir(workdir)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Short display/selector label: "main" for the main worktree, otherwise a unique label
    /// derived from the basename, branch, or path suffix.
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,
    /// True for the first entry (main worktree, not a linked worktree)
    pub is_main: bool,
    /// True if this worktree's path matches the current working directory
    pub is_current: bool,
    /// True when git marks the worktree as locked
    pub is_locked: bool,
    /// Optional lock reason from `git worktree list --porcelain`
    pub lock_reason: Option<String>,
    /// True when git marks the worktree as prunable/stale
    pub is_prunable: bool,
    /// Optional prune reason from `git worktree list --porcelain`
    pub prunable_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub enum BranchDeleteSwitchTarget {
    Branch(String),
    Detach,
}

#[derive(Debug, Clone)]
pub struct BranchDeleteResolution {
    pub worktree: WorktreeInfo,
    pub remove_worktree_selector: String,
    pub switch_target: BranchDeleteSwitchTarget,
}

impl BranchDeleteResolution {
    pub fn remove_worktree_cmd(&self) -> Option<String> {
        if self.worktree.is_main {
            None
        } else {
            Some(format!("st wt rm {}", self.remove_worktree_selector))
        }
    }

    pub fn remove_worktree_and_branch_cmd(&self) -> Option<String> {
        self.remove_worktree_cmd()
            .map(|command| format!("{command} --delete-branch"))
    }

    pub fn switch_branch_cmd(&self) -> String {
        match &self.switch_target {
            BranchDeleteSwitchTarget::Branch(target) => format!(
                "git -C '{}' switch {}",
                self.worktree.path.display(),
                target
            ),
            BranchDeleteSwitchTarget::Detach => {
                format!("git -C '{}' switch --detach", self.worktree.path.display())
            }
        }
    }
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

    /// Open the repository from a known repository path without rediscovering the cwd.
    pub fn open_from_path(path: &Path) -> Result<Self> {
        let repo = Repository::open(path)
            .with_context(|| format!("Failed to open git repository at '{}'", path.display()))?;
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

    pub(crate) fn normalize_path(path: &Path) -> PathBuf {
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

    /// Count tracked files in the given worktree path.
    pub fn tracked_file_count_at(&self, cwd: &Path) -> Result<usize> {
        let output = self.run_git(cwd, &["ls-files", "-z"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git ls-files failed in '{}': {}", cwd.display(), stderr);
        }

        Ok(output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
            .count())
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

    pub fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>> {
        Self::list_worktrees_in(self.workdir()?)
    }

    /// Lightweight alternative to [`Self::list_worktrees`] for the status/ls path.
    /// Returns a `branch → display label` map using libgit2 worktree APIs, avoiding
    /// the `git worktree list --porcelain` subprocess. Use `list_worktrees()` when
    /// you need the full `WorktreeInfo` model (stale/prunable details, paths, etc.).
    pub fn linked_worktree_names_by_branch(&self) -> Result<HashMap<String, String>> {
        let main_path = self
            .repo
            .commondir()
            .parent()
            .map(Self::normalize_path)
            .unwrap_or_else(|| {
                Self::normalize_path(self.workdir().unwrap_or_else(|_| Path::new(".")))
            });

        let mut entries: Vec<(PathBuf, Option<String>, bool)> = vec![(main_path, None, false)];

        let worktree_names = self.repo.worktrees()?;
        for name in worktree_names.iter().flatten() {
            let worktree = self.repo.find_worktree(name)?;
            let is_prunable = worktree.is_prunable(None).unwrap_or(false);
            let path = Self::normalize_path(worktree.path());
            let branch = Repository::open(worktree.path()).ok().and_then(|repo| {
                repo.head()
                    .ok()
                    .filter(|head| head.is_branch())
                    .and_then(|head| head.shorthand().map(str::to_string))
            });
            entries.push((path, branch, is_prunable));
        }

        let label_candidates = entries
            .iter()
            .enumerate()
            .map(|(idx, (path, branch, _))| WorktreeLabelCandidate {
                is_main: idx == 0,
                path: path.clone(),
                basename: if idx == 0 {
                    "main".to_string()
                } else {
                    worktree_basename(path)
                },
                branch: branch.clone(),
            })
            .collect::<Vec<_>>();
        let labels = derive_worktree_names(&label_candidates);

        let mut by_branch = HashMap::new();
        for (idx, (_, branch, is_prunable)) in entries.into_iter().enumerate().skip(1) {
            if !is_prunable {
                if let Some(branch) = branch {
                    by_branch.insert(branch, labels[idx].clone());
                }
            }
        }

        Ok(by_branch)
    }

    pub fn list_worktrees_in(cwd: &Path) -> Result<Vec<WorktreeInfo>> {
        let output = run_git_in(cwd, &["worktree", "list", "--porcelain"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git worktree list failed: {}", stderr);
        }

        let cwd = std::env::current_dir().unwrap_or_default();
        let cwd_normalized = Self::normalize_path(&cwd);

        let stdout = String::from_utf8_lossy(&output.stdout);
        #[allow(clippy::type_complexity)]
        let mut raw_entries: Vec<(
            PathBuf,
            Option<String>,
            bool,
            Option<String>,
            bool,
            Option<String>,
        )> = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        let mut current_branch: Option<String> = None;
        let mut current_locked = false;
        let mut current_lock_reason: Option<String> = None;
        let mut current_prunable = false;
        let mut current_prunable_reason: Option<String> = None;

        let mut flush_entry = |path: &mut Option<PathBuf>,
                               branch: &mut Option<String>,
                               locked: &mut bool,
                               lock_reason: &mut Option<String>,
                               prunable: &mut bool,
                               prunable_reason: &mut Option<String>| {
            if let Some(p) = path.take() {
                raw_entries.push((
                    Self::normalize_path(&p),
                    branch.take(),
                    std::mem::take(locked),
                    lock_reason.take(),
                    std::mem::take(prunable),
                    prunable_reason.take(),
                ));
            }
        };

        for line in stdout.lines() {
            if line.is_empty() {
                flush_entry(
                    &mut current_path,
                    &mut current_branch,
                    &mut current_locked,
                    &mut current_lock_reason,
                    &mut current_prunable,
                    &mut current_prunable_reason,
                );
                continue;
            }

            if let Some(path) = line.strip_prefix("worktree ") {
                flush_entry(
                    &mut current_path,
                    &mut current_branch,
                    &mut current_locked,
                    &mut current_lock_reason,
                    &mut current_prunable,
                    &mut current_prunable_reason,
                );
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
                continue;
            }

            if let Some(reason) = line.strip_prefix("locked") {
                current_locked = true;
                let reason = reason.trim();
                if !reason.is_empty() {
                    current_lock_reason = Some(reason.to_string());
                }
                continue;
            }

            if let Some(reason) = line.strip_prefix("prunable") {
                current_prunable = true;
                let reason = reason.trim();
                if !reason.is_empty() {
                    current_prunable_reason = Some(reason.to_string());
                }
            }
        }

        flush_entry(
            &mut current_path,
            &mut current_branch,
            &mut current_locked,
            &mut current_lock_reason,
            &mut current_prunable,
            &mut current_prunable_reason,
        );

        let label_candidates = raw_entries
            .iter()
            .enumerate()
            .map(|(idx, (path, branch, _, _, _, _))| WorktreeLabelCandidate {
                is_main: idx == 0,
                path: path.clone(),
                basename: if idx == 0 {
                    "main".to_string()
                } else {
                    worktree_basename(path)
                },
                branch: branch.clone(),
            })
            .collect::<Vec<_>>();
        let labels = derive_worktree_names(&label_candidates);

        let worktrees = raw_entries
            .into_iter()
            .enumerate()
            .map(
                |(idx, (path, branch, is_locked, lock_reason, is_prunable, prunable_reason))| {
                    let is_main = idx == 0;
                    let is_current = path == cwd_normalized;
                    WorktreeInfo {
                        name: labels[idx].clone(),
                        path,
                        branch,
                        is_main,
                        is_current,
                        is_locked,
                        lock_reason,
                        is_prunable,
                        prunable_reason,
                    }
                },
            )
            .collect();

        Ok(worktrees)
    }

    /// Return the absolute path to the main repo's working directory.
    /// Works correctly even when called from inside a linked worktree.
    pub fn main_repo_workdir(&self) -> Result<PathBuf> {
        let worktrees = self.list_worktrees()?;
        worktrees
            .into_iter()
            .next()
            .map(|wt| wt.path)
            .context("git worktree list returned no entries")
    }

    /// Create a new linked worktree at `path` for an existing `branch`.
    pub fn worktree_create(&self, branch: &str, path: &Path) -> Result<()> {
        let main_dir = self.main_repo_workdir()?;
        let output = self.run_git(
            &main_dir,
            &[
                "worktree",
                "add",
                path.to_str().context("Non-UTF-8 worktree path")?,
                branch,
            ],
        )?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git worktree add failed: {}", stderr);
        }
        Ok(())
    }

    /// Create a new linked worktree at `path` with a brand-new `branch` stacked on `base`.
    pub fn worktree_create_new_branch(&self, branch: &str, path: &Path, base: &str) -> Result<()> {
        let main_dir = self.main_repo_workdir()?;
        let output = self.run_git(
            &main_dir,
            &[
                "worktree",
                "add",
                "-b",
                branch,
                path.to_str().context("Non-UTF-8 worktree path")?,
                base,
            ],
        )?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git worktree add -b failed: {}", stderr);
        }
        Ok(())
    }

    /// Remove a linked worktree by path.
    pub fn worktree_remove(&self, path: &Path, force: bool) -> Result<()> {
        let main_dir = self.main_repo_workdir()?;
        let path_str = path.to_str().context("Non-UTF-8 worktree path")?;
        let args: Vec<&str> = if force {
            vec!["worktree", "remove", "--force", path_str]
        } else {
            vec!["worktree", "remove", path_str]
        };
        let output = self.run_git(&main_dir, &args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git worktree remove failed: {}", stderr);
        }
        Ok(())
    }

    /// Run `git worktree prune` from the main worktree.
    pub fn worktree_prune(&self) -> Result<()> {
        let main_dir = self.main_repo_workdir()?;
        let output = self.run_git(&main_dir, &["worktree", "prune"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git worktree prune failed: {}", stderr);
        }
        Ok(())
    }

    pub fn branch_worktree(&self, branch: &str) -> Result<Option<WorktreeInfo>> {
        Self::branch_worktree_in(self.workdir()?, branch)
    }

    pub fn branch_worktree_in(cwd: &Path, branch: &str) -> Result<Option<WorktreeInfo>> {
        let branch = normalize_local_branch_name(branch);
        let worktrees = Self::list_worktrees_in(cwd)?;

        let output = run_git_in(cwd, &["branch", "-vv", "--list", branch])?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(path) = stdout.lines().find_map(parse_branch_vv_worktree_path) {
                let normalized_path = Self::normalize_path(&path);
                if let Some(worktree) = worktrees
                    .iter()
                    .find(|worktree| worktree.path == normalized_path)
                {
                    return Ok(Some(worktree.clone()));
                }
            }
        }

        Ok(worktrees.into_iter().find(|worktree| {
            worktree.branch.as_deref().map(normalize_local_branch_name) == Some(branch)
        }))
    }

    pub fn branch_worktree_path(&self, branch: &str) -> Result<Option<PathBuf>> {
        Ok(self.branch_worktree(branch)?.map(|worktree| worktree.path))
    }

    pub fn branch_delete_resolution(&self, branch: &str) -> Result<Option<BranchDeleteResolution>> {
        let branch = normalize_local_branch_name(branch);
        let Some(worktree) = self.branch_worktree(branch)? else {
            return Ok(None);
        };

        let switch_target = match self.trunk_branch() {
            Ok(trunk)
                if self
                    .branch_worktree(&trunk)?
                    .is_some_and(|target_worktree| target_worktree.path != worktree.path) =>
            {
                BranchDeleteSwitchTarget::Detach
            }
            Ok(trunk) => BranchDeleteSwitchTarget::Branch(trunk),
            Err(_) => BranchDeleteSwitchTarget::Detach,
        };

        Ok(Some(BranchDeleteResolution {
            remove_worktree_selector: worktree
                .branch
                .clone()
                .unwrap_or_else(|| worktree.path.display().to_string()),
            worktree,
            switch_target,
        }))
    }

    pub fn branch_delete_resolution_hint(&self, branch: &str) -> Result<Option<String>> {
        let Some(resolution) = self.branch_delete_resolution(branch)? else {
            return Ok(None);
        };

        let switch_cmd = resolution.switch_branch_cmd();
        let hint = if let Some(remove_cmd) = resolution.remove_worktree_cmd() {
            format!(
                "run {} to remove the linked worktree; or run {} to keep the worktree and free the branch",
                remove_cmd,
                switch_cmd
            )
        } else {
            format!(
                "run {} to free the branch in main worktree '{}'",
                switch_cmd,
                resolution.worktree.path.display()
            )
        };

        Ok(Some(hint))
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
        let base_oid = Self::resolve_to_oid_in(&self.repo, base)?;
        let head_oid = Self::resolve_to_oid_in(&self.repo, head)?;
        let (ahead, behind) = self.repo.graph_ahead_behind(head_oid, base_oid)?;
        Ok((ahead, behind))
    }

    /// Get ahead/behind counts for multiple branch pairs concurrently.
    pub fn commits_ahead_behind_many(
        &self,
        pairs: &[(String, String)],
    ) -> Vec<Result<(usize, usize)>> {
        let repo_path = self.repo.path().to_path_buf();
        let handles = pairs
            .iter()
            .map(|(base, head)| {
                let repo_path = repo_path.clone();
                let base = base.clone();
                let head = head.clone();
                thread::spawn(move || {
                    let repo = Repository::open(&repo_path).with_context(|| {
                        format!("Failed to open git repository at '{}'", repo_path.display())
                    })?;
                    let base_oid = Self::resolve_to_oid_in(&repo, &base)?;
                    let head_oid = Self::resolve_to_oid_in(&repo, &head)?;
                    let (ahead, behind) = repo.graph_ahead_behind(head_oid, base_oid)?;
                    Ok((ahead, behind))
                })
            })
            .collect::<Vec<_>>();

        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| anyhow::bail!("ahead/behind worker panicked"))
            })
            .collect()
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
        Self::resolve_to_oid_in(&self.repo, refspec)
    }

    fn resolve_to_oid_in(repo: &Repository, refspec: &str) -> Result<git2::Oid> {
        // Try as local branch first
        if let Ok(branch) = repo.find_branch(refspec, BranchType::Local) {
            if let Some(oid) = branch.get().target() {
                return Ok(oid);
            }
        }
        // Try as remote branch (e.g., "origin/main")
        if let Ok(branch) = repo.find_branch(refspec, BranchType::Remote) {
            if let Some(oid) = branch.get().target() {
                return Ok(oid);
            }
        }
        // Try as reference
        if let Ok(reference) = repo.find_reference(refspec) {
            if let Some(oid) = reference.target() {
                return Ok(oid);
            }
        }
        // Try revparse
        let obj = repo.revparse_single(refspec)?;
        Ok(obj.id())
    }

    /// Get the trunk branch name (from stored setting or auto-detect main/master)
    pub fn trunk_branch(&self) -> Result<String> {
        // First check if trunk is stored
        if let Some(trunk) = super::refs::read_trunk(&self.repo)? {
            // Validate the stored trunk branch actually exists locally
            if self.repo.find_branch(&trunk, BranchType::Local).is_ok() {
                return Ok(trunk);
            }
            // Stored trunk doesn't exist, fall through to auto-detection
        }
        // Fall back to auto-detection and persist the result
        let detected = self.detect_trunk()?;
        super::refs::write_trunk(&self.repo, &detected)?;
        Ok(detected)
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

    /// Fetch a remote and return whether the fetch succeeded.
    pub fn fetch_remote(&self, remote: &str) -> Result<bool> {
        let output = self.run_git(self.workdir()?, &["fetch", remote])?;
        Ok(output.status.success())
    }

    fn rebase_with_args_in_path(&self, cwd: &Path, args: &[&str]) -> Result<RebaseResult> {
        let output = self.run_git(cwd, args)?;
        if output.status.success() {
            return Ok(RebaseResult::Success);
        }

        if self.rebase_in_progress_at(cwd)? {
            return Ok(RebaseResult::Conflict);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "git {} failed in '{}': {}",
            args.join(" "),
            cwd.display(),
            stderr
        );
    }

    fn rebase_in_path(&self, cwd: &Path, onto: &str) -> Result<RebaseResult> {
        self.rebase_with_args_in_path(cwd, &["rebase", onto])
    }

    fn rebase_onto_upstream_in_path(
        &self,
        cwd: &Path,
        onto: &str,
        upstream: &str,
    ) -> Result<RebaseResult> {
        self.rebase_with_args_in_path(cwd, &["rebase", "--onto", onto, upstream])
    }

    fn reset_hard_in_path(&self, cwd: &Path, target: &str) -> Result<()> {
        let output = self.run_git(cwd, &["reset", "--hard", target])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!(
                "git reset --hard {} failed in '{}': {}",
                target,
                cwd.display(),
                stderr
            );
        }
        Ok(())
    }

    /// Resolve the worktree path where a branch rebase would run.
    /// Returns the linked worktree path if one exists, otherwise the main workdir.
    pub(crate) fn branch_rebase_target_workdir(&self, branch: &str) -> Result<PathBuf> {
        let current_workdir = Self::normalize_path(self.workdir()?);
        let target_workdir = self
            .branch_worktree_path(branch)?
            .unwrap_or_else(|| current_workdir.clone());
        let target_workdir = Self::normalize_path(&target_workdir);

        if target_workdir != current_workdir {
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

        Ok(target_workdir)
    }

    fn prepare_branch_rebase_context(&self, branch: &str) -> Result<(PathBuf, PathBuf)> {
        let current_workdir = Self::normalize_path(self.workdir()?);
        let target_workdir = self.branch_rebase_target_workdir(branch)?;

        if target_workdir == current_workdir && self.current_branch()? != branch {
            self.checkout(branch)?;
        }

        Ok((current_workdir, target_workdir))
    }

    /// Return true when merging `branch` into `base` would leave `base` unchanged.
    ///
    /// This catches squash merges where the branch commits are not ancestors of
    /// `base`, but the branch's net tree changes are already present there.
    fn branch_is_integrated_into(&self, base: &str, branch: &str) -> Result<bool> {
        let merge_base = match self.merge_base_refs(base, branch) {
            Ok(base) => base,
            Err(_) => return Ok(false),
        };

        let output = self.run_git(
            self.workdir()?,
            &[
                "merge-tree",
                "--write-tree",
                "--no-messages",
                "--merge-base",
                &merge_base,
                base,
                branch,
            ],
        )?;

        if !output.status.success() {
            return Ok(false);
        }

        let merged_tree = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| {
                let trimmed = line.trim();
                (!trimmed.is_empty()).then_some(trimmed.to_string())
            });
        let Some(merged_tree) = merged_tree else {
            return Ok(false);
        };

        let base_oid = self.resolve_to_oid(base)?;
        let base_tree = self.repo.find_commit(base_oid)?.tree_id().to_string();
        Ok(merged_tree == base_tree)
    }

    fn run_patch_id_from_input(&self, cwd: &Path, input: &[u8]) -> Result<Vec<String>> {
        let mut patch_id = Command::new("git")
            .args(["patch-id", "--stable"])
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to run git patch-id --stable")?;

        // Write stdin on a background thread to avoid a deadlock: git patch-id
        // buffers output as it reads input. If its stdout pipe fills up before
        // we start reading stdout, both processes stall (git patch-id can't
        // write, so it stops reading, so write_all blocks forever). Spawning a
        // writer thread lets us drain stdout on the main thread concurrently.
        let input = input.to_vec();
        let mut stdin = patch_id.stdin.take().context("Failed to open stdin")?;
        let writer = std::thread::spawn(move || -> std::io::Result<()> {
            stdin.write_all(&input)?;
            // Drop stdin here so git patch-id sees EOF and flushes output.
            Ok(())
        });

        let output = patch_id
            .wait_with_output()
            .context("Failed to read git patch-id output")?;

        writer
            .join()
            .map_err(|_| anyhow::anyhow!("stdin writer thread panicked"))?
            .context("Failed to write to git patch-id stdin")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git patch-id --stable failed: {}", stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(ToString::to_string)
            .collect())
    }

    /// Max commits in `merge_base..trunk` for patch-id merged-branch detection; beyond this we
    /// skip the expensive `git log -p` path (see `is_branch_merged_equivalent_to_trunk`).
    pub(crate) const PATCH_ID_TRUNK_COMMIT_CAP: usize = 200;

    pub(crate) fn rev_list_count(&self, cwd: &Path, range: &str) -> Result<usize> {
        let output = self.run_git(cwd, &["rev-list", "--count", range])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git rev-list --count {} failed: {}", range, stderr);
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0))
    }

    pub(crate) fn patch_ids_for_range(&self, cwd: &Path, range: &str) -> Result<HashSet<String>> {
        let output = self.run_git(cwd, &["log", "--format=%H", "-p", "--no-merges", range])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!(
                "git log --format=%H -p --no-merges {} failed: {}",
                range,
                stderr
            );
        }

        Ok(self
            .run_patch_id_from_input(cwd, &output.stdout)?
            .into_iter()
            .collect())
    }

    /// Rebase a branch onto `onto` using the stored parent revision as upstream.
    ///
    /// Runs `git rebase --onto <onto> <fallback_upstream> <branch>` when
    /// `fallback_upstream` is a valid ancestor of `branch` (the normal case).
    /// This matches freephite's restack strategy: fast, simple, no patch-id scanning.
    pub fn rebase_branch_onto_with_provenance(
        &self,
        branch: &str,
        onto: &str,
        fallback_upstream: &str,
        auto_stash_pop: bool,
    ) -> Result<RebaseResult> {
        Ok(self
            .rebase_branch_onto_with_provenance_timing(
                branch,
                onto,
                fallback_upstream,
                auto_stash_pop,
            )?
            .result)
    }

    pub fn rebase_branch_onto_with_provenance_timing(
        &self,
        branch: &str,
        onto: &str,
        fallback_upstream: &str,
        auto_stash_pop: bool,
    ) -> Result<RebaseOutcome> {
        let mut timings = RebaseTimings::default();

        let prepare_started_at = Instant::now();
        let (_current_workdir, target_workdir) = self.prepare_branch_rebase_context(branch)?;
        timings.prepare_context = prepare_started_at.elapsed();

        let mut stashed = false;
        let dirty_check_started_at = Instant::now();
        let target_is_dirty = self.is_dirty_at(&target_workdir)?;
        timings.dirty_check = dirty_check_started_at.elapsed();
        if target_is_dirty {
            if !auto_stash_pop {
                anyhow::bail!(
                    "Cannot restack '{}': worktree '{}' has uncommitted changes. \
Use --auto-stash-pop or stash/commit changes first.",
                    branch,
                    target_workdir.display()
                );
            }
            let auto_stash_push_started_at = Instant::now();
            stashed = self.stash_push_at(&target_workdir)?;
            timings.auto_stash_push = auto_stash_push_started_at.elapsed();
        }

        let git_rebase_started_at = Instant::now();
        if self
            .branch_is_integrated_into(onto, branch)
            .unwrap_or(false)
        {
            self.reset_hard_in_path(&target_workdir, onto)
                .with_context(|| format!("Failed to move '{}' to '{}'", branch, onto))?;
            timings.git_rebase = git_rebase_started_at.elapsed();

            if stashed {
                let auto_stash_pop_started_at = Instant::now();
                self.stash_pop_at(&target_workdir).with_context(|| {
                    format!(
                        "Moved '{}' to '{}' successfully, but failed to auto-pop stash in '{}'",
                        branch,
                        onto,
                        target_workdir.display()
                    )
                })?;
                timings.auto_stash_pop = auto_stash_pop_started_at.elapsed();
            }

            return Ok(RebaseOutcome {
                result: RebaseResult::Success,
                timings,
            });
        }

        // Use the stored parent revision as upstream when it is a valid ancestor
        // of the branch.  This is the same strategy freephite uses:
        //   git rebase --onto <onto> <parent_branch_revision> <branch>
        // No patch-id scanning; the old provenance path was the main source of
        // slowness (O(N) `git log -p` per branch) and spurious conflicts.
        let upstream_for_rebase = (!fallback_upstream.trim().is_empty()
            && self.is_ancestor(fallback_upstream, branch).unwrap_or(false))
        .then(|| fallback_upstream.to_string());

        let rebase_result = if let Some(upstream) = upstream_for_rebase.as_deref() {
            self.rebase_onto_upstream_in_path(&target_workdir, onto, upstream)
                .with_context(|| {
                    format!(
                        "Failed to rebase '{}' onto '{}' with upstream '{}' in '{}'",
                        branch,
                        onto,
                        upstream,
                        target_workdir.display()
                    )
                })
        } else {
            self.rebase_in_path(&target_workdir, onto).with_context(|| {
                format!(
                    "Failed to rebase '{}' onto '{}' in '{}'",
                    branch,
                    onto,
                    target_workdir.display()
                )
            })
        };
        timings.git_rebase = git_rebase_started_at.elapsed();

        let result = match rebase_result {
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
            let auto_stash_pop_started_at = Instant::now();
            self.stash_pop_at(&target_workdir).with_context(|| {
                format!(
                    "Rebased '{}' successfully, but failed to auto-pop stash in '{}'",
                    branch,
                    target_workdir.display()
                )
            })?;
            timings.auto_stash_pop = auto_stash_pop_started_at.elapsed();
        }

        Ok(RebaseOutcome { result, timings })
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
        self.rebase_branch_onto_with_provenance(branch, onto, "", auto_stash_pop)
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

    /// Check whether a rebase is in progress inside the given worktree path.
    pub fn rebase_in_progress_in(&self, cwd: &Path) -> Result<bool> {
        self.rebase_in_progress_at(cwd)
    }

    /// Check whether a merge is in progress inside the given worktree path.
    pub fn merge_in_progress_in(&self, cwd: &Path) -> Result<bool> {
        let git_dir = self.git_dir_in_path(cwd)?;
        Ok(git_dir.join("MERGE_HEAD").exists())
    }

    /// Check whether the given worktree path currently has unresolved conflicts.
    pub fn has_conflicts_in(&self, cwd: &Path) -> Result<bool> {
        let output = self.run_git(cwd, &["diff", "--name-only", "--diff-filter=U"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git diff --name-only --diff-filter=U failed: {}", stderr);
        }
        Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
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

    /// Find merge-base commit between any two refs (branch names, remote refs, or SHAs).
    pub fn merge_base_refs(&self, left: &str, right: &str) -> Result<String> {
        let left_oid = self.resolve_to_oid(left)?;
        let right_oid = self.resolve_to_oid(right)?;
        Ok(self.repo.merge_base(left_oid, right_oid)?.to_string())
    }

    /// Check whether `ancestor` is an ancestor of `descendant`.
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let ancestor_oid = self.resolve_to_oid(ancestor)?;
        let descendant_oid = self.resolve_to_oid(descendant)?;
        if ancestor_oid == descendant_oid {
            return Ok(true);
        }
        Ok(self
            .repo
            .graph_descendant_of(descendant_oid, ancestor_oid)?)
    }

    /// Delete a branch
    pub fn delete_branch(&self, name: &str, force: bool) -> Result<()> {
        let name = normalize_local_branch_name(name);
        if let Some(hint) = self.branch_delete_resolution_hint(name)? {
            anyhow::bail!(
                "Cannot delete branch '{}' because it is currently checked out in a linked worktree. To fix it, {}.",
                name,
                hint
            );
        }

        if !force {
            let branch = self.repo.find_branch(name, BranchType::Local)?;
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
        }

        // Use git CLI instead of libgit2's branch.delete() to avoid issues
        // with config cleanup on branch names containing slashes
        let flag = if force { "-D" } else { "-d" };
        let output = self.run_git(self.workdir()?, &["branch", flag, name])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to delete branch '{}': {}", name, stderr.trim());
        }
        Ok(())
    }

    /// Read an optional user-defined worktree marker for a branch from git config.
    pub fn worktree_marker(&self, branch: &str) -> Option<String> {
        let key = format!(
            "stax.worktree-marker.{}",
            branch
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
        );
        self.repo
            .config()
            .ok()
            .and_then(|cfg| cfg.get_string(&key).ok())
            .filter(|value| !value.trim().is_empty())
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

    /// Cheap merge-equivalence checks only (ancestor + identical tree). Returns `Some(())` if
    /// merged by those signals, `None` if patch-id provenance may still be needed.
    pub fn is_branch_merged_cheap(&self, branch: &str) -> Result<Option<()>> {
        if self.is_branch_merged(branch).unwrap_or(false) {
            return Ok(Some(()));
        }

        let trunk = self.trunk_branch()?;

        // Tree-level diff: if the trees are identical the branch is fully merged,
        // regardless of how many commits were squashed.
        if self.refs_have_no_diff(&trunk, branch)? {
            return Ok(Some(()));
        }

        Ok(None)
    }

    /// Return true when two refs have no content diff.
    /// Check whether a branch is merged-equivalent to trunk.
    ///
    /// Uses multiple strategies in order:
    /// 1. Ancestor check (`git branch --merged`)
    /// 2. Tree-level diff (`git diff --quiet`) — catches multi-commit squash merges
    ///    when trunk hasn't diverged
    /// 3. Patch-id provenance — catches single-commit squash/cherry-pick merges even
    ///    after trunk has advanced with unrelated commits
    pub fn is_branch_merged_equivalent_to_trunk(&self, branch: &str) -> Result<bool> {
        if self.is_branch_merged_cheap(branch)?.is_some() {
            return Ok(true);
        }

        let trunk = self.trunk_branch()?;

        // Patch-id provenance: handles the case where trunk has advanced past the
        // merge point with unrelated commits (tree diff would show false negatives).
        let merge_base = match self.merge_base(&trunk, branch) {
            Ok(base) => base,
            Err(_) => return Ok(false),
        };

        let cwd = self.workdir()?;
        let branch_range = format!("{}..{}", merge_base, branch);
        let branch_patch_ids = self.patch_ids_for_range(cwd, &branch_range)?;
        if branch_patch_ids.is_empty() {
            return Ok(true);
        }

        let trunk_range = format!("{}..{}", merge_base, trunk);
        let trunk_count = self.rev_list_count(cwd, &trunk_range)?;
        if trunk_count > Self::PATCH_ID_TRUNK_COMMIT_CAP {
            return Ok(false);
        }

        let trunk_patch_ids = self.patch_ids_for_range(cwd, &trunk_range)?;
        if trunk_patch_ids.is_empty() {
            return Ok(false);
        }

        Ok(branch_patch_ids.is_subset(&trunk_patch_ids))
    }

    /// Return true when two refs have identical trees (no content diff).
    fn refs_have_no_diff(&self, left: &str, right: &str) -> Result<bool> {
        let output = self.run_git(self.workdir()?, &["diff", "--quiet", left, right])?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                anyhow::bail!("git diff --quiet {} {} failed: {}", left, right, stderr)
            }
        }
    }

    /// Check if a branch has a remote tracking branch (origin/<branch>)
    pub fn has_remote(&self, branch: &str) -> bool {
        let remote_name = format!("origin/{}", branch);
        self.repo
            .find_branch(&remote_name, BranchType::Remote)
            .is_ok()
    }

    /// Return all branch names under `refs/remotes/<remote>/` as a set.
    /// One libgit2 ref-glob instead of one subprocess per branch.
    pub fn remote_branch_names(&self, remote: &str) -> Result<HashSet<String>> {
        let prefix = format!("refs/remotes/{}/", remote);
        let refs = self
            .repo
            .references_glob(&format!("{}*", prefix))
            .context("Failed to glob remote refs")?;
        let mut names = HashSet::new();
        for r in refs.flatten() {
            if let Some(name) = r.name() {
                if let Some(branch) = name.strip_prefix(&prefix) {
                    names.insert(branch.to_string());
                }
            }
        }
        Ok(names)
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
            if self
                .is_branch_merged_equivalent_to_trunk(&branch)
                .unwrap_or(false)
            {
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

    /// Predict conflicts for multiple branches before restacking.
    /// Returns only branches that would have conflicts.
    pub fn predict_restack_conflicts(
        &self,
        branches_with_parents: &[(String, String)],
    ) -> Vec<ConflictPrediction> {
        branches_with_parents
            .iter()
            .filter_map(
                |(branch, parent)| match self.check_rebase_conflicts(branch, parent) {
                    Ok(files) if !files.is_empty() => Some(ConflictPrediction {
                        branch: branch.clone(),
                        onto: parent.clone(),
                        conflicting_files: files,
                    }),
                    _ => None,
                },
            )
            .collect()
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

    /// List files currently in an unmerged (conflicted) state.
    pub fn conflicted_files(&self) -> Result<Vec<String>> {
        let output = self.run_git(self.workdir()?, &["diff", "--name-only", "--diff-filter=U"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("git diff --name-only --diff-filter=U failed: {}", stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect())
    }

    /// List paths currently modified, staged, unmerged, or untracked.
    pub fn changed_files(&self) -> Result<Vec<String>> {
        let output = self.run_git(
            self.workdir()?,
            &["status", "--porcelain", "--untracked-files=all"],
        )?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!(
                "git status --porcelain --untracked-files=all failed: {}",
                stderr
            );
        }

        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if line.len() < 4 {
                continue;
            }
            let mut path = line[3..].trim().to_string();
            if path.is_empty() {
                continue;
            }

            if let Some((_, new_path)) = path.rsplit_once(" -> ") {
                path = new_path.to_string();
            }

            if seen.insert(path.clone()) {
                files.push(path);
            }
        }

        Ok(files)
    }

    /// Stage an explicit list of files.
    pub fn add_files(&self, paths: &[String]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }

        let status = Command::new("git")
            .arg("add")
            .arg("--")
            .args(paths)
            .current_dir(self.workdir()?)
            .status()
            .context("Failed to run git add")?;

        if !status.success() {
            anyhow::bail!("git add failed");
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

    /// Lease-protected force push a branch to the remote.
    pub fn force_push(&self, remote: &str, branch: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["push", "--force-with-lease", "--atomic", remote, branch])
            .current_dir(self.workdir()?)
            .output()
            .context("Failed to run git push")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Fallback if remote doesn't support atomic
            if stderr.contains("--atomic") || stderr.contains("atomic push failed") {
                return self.force_push_non_atomic(remote, branch);
            }

            anyhow::bail!("git push failed: {}", stderr);
        }
        Ok(())
    }

    fn force_push_non_atomic(&self, remote: &str, branch: &str) -> Result<()> {
        let status = Command::new("git")
            .args(["push", "--force-with-lease", remote, branch])
            .current_dir(self.workdir()?)
            .status()
            .context("Failed to run git push --force-with-lease")?;

        if !status.success() {
            anyhow::bail!("git push --force-with-lease {} {} failed", remote, branch);
        }
        Ok(())
    }

    /// Hard reset to a specific ref/OID
    pub fn reset_hard(&self, target: &str) -> Result<()> {
        self.reset_hard_in_path(self.workdir()?, target)
    }

    /// Enable git rerere (reuse recorded resolution) for this repository
    pub fn enable_rerere(&self) -> Result<()> {
        let status = Command::new("git")
            .args(["config", "rerere.enabled", "true"])
            .current_dir(self.workdir()?)
            .status()
            .context("Failed to enable rerere")?;

        if !status.success() {
            anyhow::bail!("Failed to set rerere.enabled config");
        }
        Ok(())
    }

    /// Check if git rerere is enabled for this repository
    pub fn rerere_enabled(&self) -> Result<bool> {
        let output = Command::new("git")
            .args(["config", "--get", "rerere.enabled"])
            .current_dir(self.workdir()?)
            .output()
            .context("Failed to check rerere config")?;

        Ok(output.status.success() && output.stdout.starts_with(b"true"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RebaseResult {
    Success,
    Conflict,
}

#[derive(Debug, Clone, Default)]
pub struct RebaseTimings {
    pub prepare_context: Duration,
    pub dirty_check: Duration,
    pub auto_stash_push: Duration,
    pub git_rebase: Duration,
    pub auto_stash_pop: Duration,
}

impl RebaseTimings {
    pub fn total(&self) -> Duration {
        self.prepare_context
            + self.dirty_check
            + self.auto_stash_push
            + self.git_rebase
            + self.auto_stash_pop
    }
}

#[derive(Debug, Clone)]
pub struct RebaseOutcome {
    pub result: RebaseResult,
    pub timings: RebaseTimings,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub short_hash: String,
    pub message: String,
}

/// Predicted conflict for a single branch rebase
#[derive(Debug, Clone)]
pub struct ConflictPrediction {
    pub branch: String,
    pub onto: String,
    pub conflicting_files: Vec<String>,
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

    fn run_git_stdout(path: &Path, args: &[&str]) -> String {
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
        String::from_utf8_lossy(&output.stdout).trim().to_string()
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
    fn test_open_from_path_opens_repo_from_git_dir() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path();

        run_git(path, &["init", "-b", "main"]);
        run_git(path, &["config", "user.email", "test@example.com"]);
        run_git(path, &["config", "user.name", "Test User"]);

        fs::write(path.join("README.md"), "base\n").expect("write readme");
        run_git(path, &["add", "README.md"]);
        run_git(path, &["commit", "-m", "Initial commit"]);

        let repo = GitRepo::open_from_path(&path.join(".git")).expect("open repo from git dir");
        assert_eq!(
            std::fs::canonicalize(repo.workdir().expect("repo workdir"))
                .expect("canonical workdir"),
            std::fs::canonicalize(path).expect("canonical temp repo path")
        );
    }

    #[test]
    fn test_commits_ahead_behind_many_matches_single_queries() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path();

        run_git(path, &["init", "-b", "main"]);
        run_git(path, &["config", "user.email", "test@example.com"]);
        run_git(path, &["config", "user.name", "Test User"]);

        fs::write(path.join("README.md"), "base\n").expect("write readme");
        run_git(path, &["add", "README.md"]);
        run_git(path, &["commit", "-m", "Initial commit"]);

        run_git(path, &["checkout", "-b", "feature-a"]);
        fs::write(path.join("a.txt"), "a\n").expect("write a");
        run_git(path, &["add", "a.txt"]);
        run_git(path, &["commit", "-m", "Feature A"]);

        run_git(path, &["checkout", "main"]);
        fs::write(path.join("main.txt"), "main\n").expect("write main");
        run_git(path, &["add", "main.txt"]);
        run_git(path, &["commit", "-m", "Main update"]);

        run_git(path, &["checkout", "-b", "feature-b"]);
        fs::write(path.join("b.txt"), "b\n").expect("write b");
        run_git(path, &["add", "b.txt"]);
        run_git(path, &["commit", "-m", "Feature B"]);

        let repo = GitRepo {
            repo: Repository::open(path).expect("open repo"),
        };
        let pairs = vec![
            ("main".to_string(), "feature-a".to_string()),
            ("main".to_string(), "feature-b".to_string()),
        ];

        let batch = repo.commits_ahead_behind_many(&pairs);

        assert_eq!(batch.len(), pairs.len());
        for ((base, head), result) in pairs.iter().zip(batch) {
            assert_eq!(
                result.expect("batch ahead/behind"),
                repo.commits_ahead_behind(base, head)
                    .expect("single ahead/behind")
            );
        }
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

    #[test]
    fn derive_worktree_names_prefers_branch_leaf_for_duplicate_basenames() {
        let names = derive_worktree_names(&[
            WorktreeLabelCandidate {
                is_main: true,
                path: PathBuf::from("/tmp/repo"),
                basename: "main".to_string(),
                branch: Some("main".to_string()),
            },
            WorktreeLabelCandidate {
                is_main: false,
                path: PathBuf::from("/tmp/a/stax"),
                basename: "stax".to_string(),
                branch: Some("codex/fix-zsh".to_string()),
            },
            WorktreeLabelCandidate {
                is_main: false,
                path: PathBuf::from("/tmp/b/stax"),
                basename: "stax".to_string(),
                branch: Some("codex/docs".to_string()),
            },
        ]);

        assert_eq!(names, vec!["main", "fix-zsh", "docs"]);
    }

    #[test]
    fn derive_worktree_names_falls_back_to_path_suffix_for_detached_duplicates() {
        let names = derive_worktree_names(&[
            WorktreeLabelCandidate {
                is_main: true,
                path: PathBuf::from("/tmp/repo"),
                basename: "main".to_string(),
                branch: Some("main".to_string()),
            },
            WorktreeLabelCandidate {
                is_main: false,
                path: PathBuf::from("/tmp/00cb/stax"),
                basename: "stax".to_string(),
                branch: None,
            },
            WorktreeLabelCandidate {
                is_main: false,
                path: PathBuf::from("/tmp/073a/stax"),
                basename: "stax".to_string(),
                branch: None,
            },
        ]);

        assert_eq!(names, vec!["main", "00cb/stax", "073a/stax"]);
    }
}
