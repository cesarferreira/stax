use super::{
    BranchDetails, BranchDiff, BranchSummary, DiffLine, DiffLineKind, DiffStatLine,
    RepositorySnapshot,
};
use crate::cache::{CiCache, DiskCachedDiff, DiskDiffLine, DiskDiffStat, TuiDiffCache};
use crate::engine::{Stack, StackSnapshot};
use crate::git::GitRepo;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A reusable, thread-friendly handle to repository-backed application data.
///
/// The session stores only canonical paths and reopens libgit2 state for each
/// operation, so callers can safely recreate it inside background tasks.
#[derive(Debug, Clone)]
pub struct RepositorySession {
    repository_root: PathBuf,
    git_dir: PathBuf,
    common_git_dir: PathBuf,
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

        Ok(Self {
            repository_root,
            git_dir,
            common_git_dir,
        })
    }

    /// Returns the canonical working-tree root represented by this session.
    pub fn repository_root(&self) -> &Path {
        &self.repository_root
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
        let ci_cache = CiCache::load(&self.git_dir);
        let branches = ordered_branches(&snapshot.stack, &snapshot.current_branch, &ci_cache)?;

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
        let (ahead, behind) = branch
            .parent
            .as_deref()
            .and_then(|parent| repo.commits_ahead_behind(parent, &branch.name).ok())
            .unwrap_or((0, 0));
        let has_remote = repo.has_remote(&branch.name);
        let (unpushed, unpulled) = repo.commits_vs_remote(&branch.name).unwrap_or((0, 0));
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
        let cache = TuiDiffCache::load(&self.common_git_dir);
        if let Some(diff) = cache.get(&key) {
            return Ok(branch_diff_from_disk(diff));
        }

        let stat = repo
            .diff_stat_at_target(branch, parent, &target)
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
            .diff_against_target(branch, parent, &target)
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
        let diff = BranchDiff { stat, lines };

        let mut cache = cache;
        cache.insert(key, branch_diff_to_disk(&diff));
        let _ = cache.save(&self.common_git_dir);

        Ok(diff)
    }

    fn open_repo(&self) -> Result<GitRepo> {
        GitRepo::open_from_path(&self.git_dir).with_context(|| {
            format!(
                "Failed to reopen git repository '{}' for root '{}'",
                self.git_dir.display(),
                self.repository_root.display()
            )
        })
    }
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
            collect_branches(&mut branches, stack, current_branch, ci_cache, root, column);
        }
    }
    branches.push(branch_summary(
        stack,
        current_branch,
        ci_cache,
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
        ci_state: ci_cache.get_ci_state(branch),
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
