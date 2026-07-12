use super::{
    BranchDetails, BranchDiff, BranchSummary, DiffLine, DiffLineKind, DiffStatLine, OperationError,
    OperationErrorDetails, OperationErrorKind, OperationRequest, OperationSideEffects,
    RepositorySnapshot,
};
use crate::cache::{CiCache, DiskCachedDiff, DiskDiffLine, DiskDiffStat, TuiDiffCache};
use crate::config::Config;
use crate::engine::{Stack, StackSnapshot};
use crate::git::GitRepo;
use crate::git::repo::{DiffTarget, WorktreeInfo};
use anyhow::{Context, Result, anyhow};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

static MUTATION_GATES: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<bool>>>>> = OnceLock::new();

/// A reusable, thread-friendly handle to repository-backed application data.
///
/// The session stores only canonical paths and reopens libgit2 state for each
/// operation, so callers can safely recreate it inside background tasks.
#[derive(Debug, Clone)]
pub struct RepositorySession {
    repository_root: PathBuf,
    git_dir: PathBuf,
    common_git_dir: PathBuf,
    mutation_gate: Arc<Mutex<bool>>,
}

impl RepositorySession {
    /// Opens a normal repository root or linked-worktree root.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let supplied_path = path.as_ref();
        let repo = GitRepo::open_from_path(supplied_path).with_context(|| {
            format!(
                "Failed to open git repository from '{}'",
                supplied_path.display()
            )
        })?;
        let workdir = repo.workdir().with_context(|| {
            format!(
                "Git repository at '{}' has no working directory",
                supplied_path.display()
            )
        })?;
        let repository_root = std::fs::canonicalize(workdir).with_context(|| {
            format!(
                "Failed to canonicalize git repository root '{}' opened from '{}'",
                workdir.display(),
                supplied_path.display()
            )
        })?;
        let git_dir = canonicalize_repository_path(
            repo.git_dir().with_context(|| {
                format!(
                    "Failed to locate git directory for '{}'",
                    supplied_path.display()
                )
            })?,
            "git directory",
            supplied_path,
        )?;
        let common_git_dir = canonicalize_repository_path(
            &repo.common_git_dir().with_context(|| {
                format!(
                    "Failed to locate common git directory for '{}'",
                    supplied_path.display()
                )
            })?,
            "common git directory",
            supplied_path,
        )?;

        let mutation_gate = mutation_gate_for(&common_git_dir);

        Ok(Self {
            repository_root,
            git_dir,
            common_git_dir,
            mutation_gate,
        })
    }

    /// Returns the canonical working-tree root represented by this session.
    pub fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    pub(super) fn cache_dir(&self) -> &Path {
        &self.common_git_dir
    }

    #[allow(dead_code)]
    pub(super) fn common_git_dir(&self) -> &Path {
        &self.common_git_dir
    }

    /// Loads the current stack, branch metadata, and cached CI states.
    pub fn snapshot(&self) -> Result<RepositorySnapshot> {
        let repo = self.open_repo()?;
        let snapshot = StackSnapshot::load(&repo).with_context(|| {
            format!(
                "Failed to load repository snapshot for '{}'",
                self.repository_root.display()
            )
        })?;
        let ci_cache = CiCache::load(&self.common_git_dir);
        let branch_revisions = snapshot
            .stack
            .branches
            .keys()
            .filter_map(|branch| {
                repo.branch_commit(branch)
                    .ok()
                    .map(|revision| (branch.clone(), revision))
            })
            .collect::<HashMap<_, _>>();
        let branches = ordered_branches(
            &snapshot.stack,
            &snapshot.current_branch,
            &ci_cache,
            &branch_revisions,
        )?;

        Ok(RepositorySnapshot {
            repository_root: self.repository_root.clone(),
            current_branch: snapshot.current_branch,
            trunk: snapshot.stack.trunk,
            branches,
        })
    }

    /// Loads commit and remote divergence details for one branch.
    ///
    /// Optional, potentially expensive calculations degrade to zero or empty
    /// values, matching the existing TUI behavior. Reopening the repository is
    /// still an error because no trustworthy details can be produced.
    pub fn branch_details(&self, branch: &BranchSummary) -> Result<BranchDetails> {
        let repo = self.open_repo()?;
        let config = Config::load_for_repo(self.repository_root()).map_err(|_| {
            anyhow!(
                "Failed to load stax config for branch details in repository '{}'; \
                 check the global config and repository stax.toml",
                self.repository_root().display()
            )
        })?;
        let remote_name = config.remote_name();
        let (ahead, behind) = branch
            .parent
            .as_deref()
            .and_then(|parent| repo.commits_ahead_behind(parent, &branch.name).ok())
            .unwrap_or((0, 0));
        let has_remote = repo.has_remote_named(remote_name, &branch.name);
        let (unpushed, unpulled) = repo
            .commits_vs_remote_named(remote_name, &branch.name)
            .unwrap_or((0, 0));
        let commits = branch
            .parent
            .as_deref()
            .and_then(|parent| repo.commits_between(parent, &branch.name).ok())
            .unwrap_or_default()
            .into_iter()
            .take(10)
            .collect();

        Ok(BranchDetails {
            ahead,
            behind,
            has_remote,
            unpushed,
            unpulled,
            commits,
        })
    }

    /// Loads a branch's merge-base diff and the compatible TUI disk cache.
    ///
    /// Ref resolution and diff calculation errors remain actionable errors.
    /// Cache persistence is deliberately best-effort: once calculation
    /// succeeds, a cache write failure never discards the successfully loaded
    /// diff.
    pub fn diff(&self, branch: &str, parent: &str) -> Result<BranchDiff> {
        let repo = self.open_repo()?;
        let target = repo.resolve_diff_target(branch, parent).with_context(|| {
            format!(
                "Failed to prepare diff for branch '{}' against parent '{}'",
                branch, parent
            )
        })?;
        let key = TuiDiffCache::key(
            parent,
            branch,
            &target.parent_oid,
            &target.branch_oid,
            &target.merge_base_oid,
        );
        if let Ok(Some(diff)) = TuiDiffCache::read_persisted(&self.common_git_dir, &key) {
            return Ok(branch_diff_from_disk(&diff));
        }

        let diff = calculate_diff_at_target(&repo, branch, parent, &target)?;
        let _ =
            TuiDiffCache::insert_persisted(&self.common_git_dir, key, branch_diff_to_disk(&diff));

        Ok(diff)
    }

    /// Recalculates a branch diff even when a matching disk cache entry exists.
    ///
    /// The immutable object IDs are captured once before calculation. Cache
    /// persistence remains best-effort so a live diff is never discarded when
    /// the cache is unavailable or malformed.
    pub fn refresh_diff(&self, branch: &str, parent: &str) -> Result<BranchDiff> {
        let repo = self.open_repo()?;
        let target = repo.resolve_diff_target(branch, parent).with_context(|| {
            format!(
                "Failed to prepare refreshed diff for branch '{}' against parent '{}'",
                branch, parent
            )
        })?;
        let key = TuiDiffCache::key(
            parent,
            branch,
            &target.parent_oid,
            &target.branch_oid,
            &target.merge_base_oid,
        );
        let diff = calculate_diff_at_target(&repo, branch, parent, &target)?;
        let _ =
            TuiDiffCache::insert_persisted(&self.common_git_dir, key, branch_diff_to_disk(&diff));
        Ok(diff)
    }

    /// Returns a cached branch diff without calculating diffstat or patch data.
    pub fn cached_diff(&self, branch: &str, parent: &str) -> Result<Option<BranchDiff>> {
        let repo = self.open_repo()?;
        let target = repo.resolve_diff_target(branch, parent).with_context(|| {
            format!(
                "Failed to prepare cached diff lookup for branch '{}' against parent '{}'",
                branch, parent
            )
        })?;
        let key = TuiDiffCache::key(
            parent,
            branch,
            &target.parent_oid,
            &target.branch_oid,
            &target.merge_base_oid,
        );
        TuiDiffCache::read_persisted(&self.common_git_dir, &key)
            .map(|diff| diff.as_ref().map(branch_diff_from_disk))
    }

    pub(super) fn open_repo(&self) -> Result<GitRepo> {
        GitRepo::open_from_path(&self.git_dir).with_context(|| {
            format!(
                "Failed to reopen git repository '{}' for root '{}'",
                self.git_dir.display(),
                self.repository_root.display()
            )
        })
    }

    #[allow(clippy::result_large_err)]
    pub(super) fn try_begin_mutation(
        &self,
        request: &OperationRequest,
    ) -> Result<MutationLease, OperationError> {
        {
            let mut active = self.mutation_gate.lock().map_err(|_| {
                operation_error(
                    request,
                    OperationErrorKind::Internal,
                    OperationErrorDetails::None,
                    "Repository operation state is unavailable",
                    "Retry the operation",
                    "mutation gate mutex was poisoned",
                    OperationSideEffects::None,
                )
            })?;
            if *active {
                return Err(operation_error(
                    request,
                    OperationErrorKind::Busy,
                    OperationErrorDetails::None,
                    "Another repository operation is already running",
                    "Wait for the current operation to finish, then retry",
                    "common repository mutation gate is active",
                    OperationSideEffects::None,
                ));
            }
            *active = true;
        }

        Ok(MutationLease {
            gate: Arc::clone(&self.mutation_gate),
        })
    }

    #[allow(clippy::result_large_err)]
    pub(super) fn with_mutation<T>(
        &self,
        request: &OperationRequest,
        targets: MutationTargets,
        run: impl FnOnce() -> Result<T, OperationError>,
    ) -> Result<T, OperationError> {
        let _lease = self.try_begin_mutation(request)?;
        let repo = self.open_repo().map_err(|error| {
            OperationError::from_source(
                request.clone(),
                OperationErrorKind::RepositoryUnavailable,
                OperationErrorDetails::None,
                "Could not open the repository",
                "Check the repository path and retry",
                &error,
                None,
                OperationSideEffects::None,
            )
        })?;
        if !repo.is_initialized() {
            return Err(operation_error(
                request,
                OperationErrorKind::InitializationRequired,
                OperationErrorDetails::None,
                "This repository has not been initialized for stax",
                "Run `st init` in the repository, then retry",
                "stax metadata refs are not initialized",
                OperationSideEffects::None,
            ));
        }
        for worktree in self.affected_worktrees(&repo, &targets).map_err(|error| {
            OperationError::from_source(
                request.clone(),
                OperationErrorKind::LocalGit,
                OperationErrorDetails::None,
                "Could not inspect repository worktrees",
                "Resolve the Git worktree error and retry",
                &error,
                None,
                OperationSideEffects::None,
            )
        })? {
            preflight_rebase_state(request, &worktree)?;
        }
        run()
    }

    fn affected_worktrees(
        &self,
        repo: &GitRepo,
        targets: &MutationTargets,
    ) -> Result<Vec<AffectedWorktree>> {
        let mut affected = Vec::new();
        let mut seen_paths = HashSet::new();
        for worktree in repo.list_worktrees()? {
            if !targets.matches(&worktree, &self.repository_root) {
                continue;
            }
            let path = std::fs::canonicalize(&worktree.path).with_context(|| {
                format!(
                    "Failed to canonicalize worktree '{}'",
                    worktree.path.display()
                )
            })?;
            if !seen_paths.insert(path.clone()) {
                continue;
            }
            let worktree_repo = GitRepo::open_from_path(&path)?;
            let common_git_dir = canonicalize_repository_path(
                &worktree_repo.common_git_dir()?,
                "common git directory",
                &path,
            )?;
            if common_git_dir != self.common_git_dir {
                continue;
            }
            let git_dir =
                canonicalize_repository_path(worktree_repo.git_dir()?, "git directory", &path)?;
            affected.push(AffectedWorktree {
                path,
                branch: worktree.branch,
                git_dir,
            });
        }
        Ok(affected)
    }
}

#[derive(Debug)]
pub(super) struct MutationLease {
    gate: Arc<Mutex<bool>>,
}

impl Drop for MutationLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.gate.lock() {
            *active = false;
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct MutationTargets {
    include_current: bool,
    branches: HashSet<String>,
}

impl MutationTargets {
    #[allow(dead_code)]
    pub(super) fn current() -> Self {
        Self {
            include_current: true,
            branches: HashSet::new(),
        }
    }

    pub(super) fn branches(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            include_current: true,
            branches: names.into_iter().map(Into::into).collect(),
        }
    }

    fn matches(&self, worktree: &WorktreeInfo, current_root: &Path) -> bool {
        let is_current = worktree
            .path
            .canonicalize()
            .map(|path| path == current_root)
            .unwrap_or(false);
        let branch_matches = worktree
            .branch
            .as_ref()
            .map(|branch| self.branches.contains(branch))
            .unwrap_or(false);
        (self.include_current && is_current) || branch_matches
    }
}

#[derive(Debug)]
struct AffectedWorktree {
    path: PathBuf,
    branch: Option<String>,
    git_dir: PathBuf,
}

#[allow(clippy::result_large_err)]
pub(super) fn require_blocking_network_context(
    request: &OperationRequest,
) -> Result<(), OperationError> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(operation_error(
            request,
            OperationErrorKind::Runtime,
            OperationErrorDetails::None,
            "This blocking network operation cannot run on a Tokio runtime thread",
            "Run it on a blocking/background executor and retry",
            "tokio::runtime::Handle::try_current returned an active handle",
            OperationSideEffects::None,
        ));
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn preflight_rebase_state(
    request: &OperationRequest,
    worktree: &AffectedWorktree,
) -> Result<(), OperationError> {
    if worktree.git_dir.join("rebase-merge").exists()
        || worktree.git_dir.join("rebase-apply").exists()
    {
        return Err(operation_error(
            request,
            OperationErrorKind::RebaseInProgress,
            OperationErrorDetails::Rebase {
                branch: worktree.branch.clone(),
                worktree: worktree.path.clone(),
            },
            format!(
                "A rebase is already in progress in {}",
                worktree.path.display()
            ),
            "Resolve conflicts and run `st continue`, or run `st abort`, then retry",
            "repository contains rebase-merge or rebase-apply state",
            OperationSideEffects::None,
        ));
    }
    Ok(())
}

fn operation_error(
    request: &OperationRequest,
    kind: OperationErrorKind,
    details: OperationErrorDetails,
    primary: impl Into<String>,
    action: impl Into<String>,
    diagnostic_chain: impl Into<String>,
    side_effects: OperationSideEffects,
) -> OperationError {
    OperationError {
        request: request.clone(),
        kind,
        details,
        primary: primary.into(),
        action: action.into(),
        diagnostic_chain: diagnostic_chain.into(),
        receipt: None,
        side_effects,
    }
}

fn mutation_gate_for(common_git_dir: &Path) -> Arc<Mutex<bool>> {
    let registry = MUTATION_GATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut gates = registry
        .lock()
        .expect("mutation gate registry mutex should not be poisoned");
    gates.retain(|_, gate| gate.strong_count() > 0);
    if let Some(gate) = gates.get(common_git_dir).and_then(Weak::upgrade) {
        return gate;
    }
    let gate = Arc::new(Mutex::new(false));
    gates.insert(common_git_dir.to_path_buf(), Arc::downgrade(&gate));
    gate
}

#[cfg(test)]
fn registry_contains_key(common_git_dir: &Path) -> bool {
    MUTATION_GATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("mutation gate registry mutex should not be poisoned")
        .contains_key(common_git_dir)
}

#[cfg(test)]
fn registry_contains_live_key(common_git_dir: &Path) -> bool {
    MUTATION_GATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("mutation gate registry mutex should not be poisoned")
        .get(common_git_dir)
        .map(|gate| gate.strong_count() > 0)
        .unwrap_or(false)
}

fn calculate_diff_at_target(
    repo: &GitRepo,
    branch: &str,
    parent: &str,
    target: &DiffTarget,
) -> Result<BranchDiff> {
    let stat = repo
        .diff_stat_at_target(branch, parent, target)
        .with_context(|| {
            format!(
                "Failed to calculate diff stat for branch '{}' against parent '{}'",
                branch, parent
            )
        })?
        .into_iter()
        .map(|(file, additions, deletions)| DiffStatLine {
            file,
            additions,
            deletions,
        })
        .collect();
    let lines = repo
        .diff_against_target(branch, parent, target)
        .with_context(|| {
            format!(
                "Failed to calculate patch for branch '{}' against parent '{}'",
                branch, parent
            )
        })?
        .into_iter()
        .map(|content| DiffLine {
            kind: classify_diff_line(&content),
            content,
        })
        .collect();
    Ok(BranchDiff { stat, lines })
}

fn canonicalize_repository_path(path: &Path, label: &str, supplied_path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).with_context(|| {
        format!(
            "Failed to canonicalize {label} '{}' for git repository '{}'",
            path.display(),
            supplied_path.display()
        )
    })
}

fn ordered_branches(
    stack: &Stack,
    current_branch: &str,
    ci_cache: &CiCache,
    branch_revisions: &HashMap<String, String>,
) -> Result<Vec<BranchSummary>> {
    let trunk = &stack.trunk;
    let mut branches = Vec::new();
    let mut trunk_children = stack
        .branches
        .get(trunk)
        .map(|branch| branch.children.clone())
        .unwrap_or_default();

    if !trunk_children.is_empty() {
        trunk_children.sort();
        for (column, root) in trunk_children.iter().enumerate() {
            collect_branches(
                &mut branches,
                stack,
                current_branch,
                ci_cache,
                branch_revisions,
                root,
                column,
            );
        }
    }
    branches.push(branch_summary(
        stack,
        current_branch,
        ci_cache,
        branch_revisions,
        trunk,
        0,
        true,
    ));
    validate_emitted_topology(stack, &branches)?;
    Ok(branches)
}

fn collect_branches(
    result: &mut Vec<BranchSummary>,
    stack: &Stack,
    current_branch: &str,
    ci_cache: &CiCache,
    branch_revisions: &HashMap<String, String>,
    branch: &str,
    base_column: usize,
) {
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
    let mut visiting = HashSet::new();
    let mut emitted = HashSet::new();

    while let Some(frame) = stack_frames.pop() {
        if frame.expanded {
            visiting.remove(&frame.branch);
            if emitted.insert(frame.branch.clone()) {
                result.push(branch_summary(
                    stack,
                    current_branch,
                    ci_cache,
                    branch_revisions,
                    &frame.branch,
                    frame.column,
                    false,
                ));
            }
            continue;
        }

        if emitted.contains(&frame.branch) || !visiting.insert(frame.branch.clone()) {
            continue;
        }

        stack_frames.push(Frame {
            branch: frame.branch.clone(),
            column: frame.column,
            expanded: true,
        });

        if let Some(info) = stack.branches.get(&frame.branch) {
            let mut children = info.children.iter().collect::<Vec<_>>();
            children.sort();
            for (index, child) in children.into_iter().enumerate().rev() {
                if emitted.contains(child) || visiting.contains(child) {
                    continue;
                }
                stack_frames.push(Frame {
                    branch: child.clone(),
                    column: frame.column + index,
                    expanded: false,
                });
            }
        }
    }
}

fn validate_emitted_topology(stack: &Stack, branches: &[BranchSummary]) -> Result<()> {
    let mut emitted_counts = HashMap::new();
    for branch in branches {
        *emitted_counts.entry(branch.name.as_str()).or_insert(0usize) += 1;
    }

    let mut affected = stack
        .branches
        .keys()
        .filter(|name| emitted_counts.get(name.as_str()).copied() != Some(1))
        .cloned()
        .collect::<Vec<_>>();
    affected.sort();
    if !affected.is_empty() {
        anyhow::bail!(
            "Invalid stack topology: every branch must be reachable from trunk '{}' and emitted exactly once; affected branches: {}",
            stack.trunk,
            affected.join(", ")
        );
    }
    Ok(())
}

fn branch_summary(
    stack: &Stack,
    current_branch: &str,
    ci_cache: &CiCache,
    branch_revisions: &HashMap<String, String>,
    branch: &str,
    column: usize,
    is_trunk: bool,
) -> BranchSummary {
    let info = stack.branches.get(branch);
    BranchSummary {
        name: branch.to_string(),
        parent: info.and_then(|branch| branch.parent.clone()),
        column,
        is_current: branch == current_branch,
        is_trunk,
        needs_restack: info.map(|branch| branch.needs_restack).unwrap_or(false),
        pr_number: info.and_then(|branch| branch.pr_number),
        pr_state: info.and_then(|branch| branch.pr_state.clone()),
        ci_state: branch_revisions
            .get(branch)
            .and_then(|revision| ci_cache.get_ci_state_for_revision(branch, revision)),
    }
}

fn classify_diff_line(line: &str) -> DiffLineKind {
    if line.starts_with("+++") || line.starts_with("---") {
        DiffLineKind::Header
    } else if line.starts_with('+') {
        DiffLineKind::Addition
    } else if line.starts_with('-') {
        DiffLineKind::Deletion
    } else if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with("diff ") || line.starts_with("index ") {
        DiffLineKind::Header
    } else {
        DiffLineKind::Context
    }
}

fn diff_line_kind_name(kind: DiffLineKind) -> &'static str {
    match kind {
        DiffLineKind::Header => "header",
        DiffLineKind::Addition => "addition",
        DiffLineKind::Deletion => "deletion",
        DiffLineKind::Context => "context",
        DiffLineKind::Hunk => "hunk",
    }
}

fn diff_line_kind_from_name(name: &str, content: &str) -> DiffLineKind {
    match name {
        "header" => DiffLineKind::Header,
        "addition" => DiffLineKind::Addition,
        "deletion" => DiffLineKind::Deletion,
        "context" => DiffLineKind::Context,
        "hunk" => DiffLineKind::Hunk,
        _ => classify_diff_line(content),
    }
}

fn branch_diff_to_disk(diff: &BranchDiff) -> DiskCachedDiff {
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
                line_type: diff_line_kind_name(line.kind).to_string(),
            })
            .collect(),
    }
}

fn branch_diff_from_disk(diff: &DiskCachedDiff) -> BranchDiff {
    BranchDiff {
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
                kind: diff_line_kind_from_name(&line.line_type, &line.content),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use crate::application::repository::{
        MutationTargets, RepositorySession, registry_contains_key, registry_contains_live_key,
        require_blocking_network_context,
    };
    use crate::application::{
        OperationError, OperationErrorDetails, OperationErrorKind, OperationRequest,
        OperationSideEffects, PullRequestMode,
    };
    use crate::git::GitRepo;
    use std::cell::Cell;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn git(cwd: &Path, args: &[&str]) -> String {
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

    fn initialized_repository_with_linked_worktree() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-b", "main"]);
        git(root.path(), &["config", "user.name", "Test User"]);
        git(root.path(), &["config", "user.email", "test@example.com"]);
        std::fs::write(root.path().join("README.md"), "initial\n").unwrap();
        git(root.path(), &["add", "README.md"]);
        git(root.path(), &["commit", "-m", "initial"]);
        GitRepo::open_from_path(root.path())
            .unwrap()
            .set_trunk("main")
            .unwrap();
        let linked = root.path().with_file_name(format!(
            "{}-linked",
            root.path().file_name().unwrap().to_string_lossy()
        ));
        git(
            root.path(),
            &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
        );
        (root, linked)
    }

    #[test]
    fn sessions_for_main_and_linked_worktrees_share_a_private_gate() {
        let (root, linked_path) = initialized_repository_with_linked_worktree();
        let main = RepositorySession::open(root.path()).unwrap();
        let linked = RepositorySession::open(&linked_path).unwrap();
        let request = OperationRequest::Checkout {
            branch: "feature".into(),
        };

        let lease = main.try_begin_mutation(&request).unwrap();
        let error = linked.try_begin_mutation(&request).unwrap_err();
        assert_eq!(error.kind, OperationErrorKind::Busy);
        drop(lease);
        assert!(linked.try_begin_mutation(&request).is_ok());
    }

    #[test]
    fn dead_gate_entries_are_pruned_when_another_session_opens() {
        let (root, _linked) = initialized_repository_with_linked_worktree();
        let key = RepositorySession::open(root.path())
            .unwrap()
            .common_git_dir()
            .to_path_buf();
        {
            let session = RepositorySession::open(root.path()).unwrap();
            assert!(registry_contains_live_key(&key));
            drop(session);
        }
        let (other, _other_linked) = initialized_repository_with_linked_worktree();
        let _session = RepositorySession::open(other.path()).unwrap();
        assert!(!registry_contains_key(&key));
    }

    #[test]
    #[allow(clippy::result_large_err)]
    fn mutation_preflight_reports_linked_rebase_path_before_running_work() {
        let (root, linked_path) = initialized_repository_with_linked_worktree();
        let linked_git_dir = PathBuf::from(git(&linked_path, &["rev-parse", "--git-dir"]));
        let linked_git_dir = if linked_git_dir.is_absolute() {
            linked_git_dir
        } else {
            linked_path.join(linked_git_dir)
        };
        std::fs::create_dir_all(linked_git_dir.join("rebase-merge")).unwrap();
        let session = RepositorySession::open(root.path()).unwrap();
        let request = OperationRequest::CreateBranch {
            name: "child".into(),
            parent: "main".into(),
        };
        let ran = Cell::new(false);

        let error = session
            .with_mutation(
                &request,
                MutationTargets::branches(["linked"]),
                || -> std::result::Result<(), OperationError> {
                    ran.set(true);
                    unreachable!()
                },
            )
            .unwrap_err();

        assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
        assert_eq!(
            error.details,
            OperationErrorDetails::Rebase {
                branch: Some("linked".into()),
                worktree: linked_path.canonicalize().unwrap(),
            }
        );
        assert!(!ran.get());
    }

    #[tokio::test]
    async fn blocking_network_guard_returns_runtime_error_without_panicking() {
        let request = OperationRequest::SubmitStack {
            new_pull_requests: PullRequestMode::Draft,
        };
        let error = require_blocking_network_context(&request).unwrap_err();
        assert_eq!(error.kind, OperationErrorKind::Runtime);
        assert_eq!(error.side_effects, OperationSideEffects::None);
    }
}
