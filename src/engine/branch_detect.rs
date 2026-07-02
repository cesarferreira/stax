use crate::git::GitRepo;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

/// How a branch was determined to be merged.
#[derive(Debug, Clone, PartialEq)]
pub enum MergeType {
    /// Branch is an ancestor of trunk (git branch --merged).
    Ancestor,
    /// Branch patches are a subset of trunk's patches (squash/rebase merge).
    SquashMerge,
}

#[derive(Debug, Clone)]
pub struct MergedBranchInfo {
    pub branch: String,
    pub merge_type: MergeType,
}

#[derive(Debug, Clone)]
pub struct StaleBranchInfo {
    pub branch: String,
    /// Age of the most recent commit in days.
    pub days_old: u64,
}

/// Find all local branches that are merged into trunk (ancestor or squash-merged).
///
/// Unlike sync's private `find_merged_branches`, this operates on ALL local
/// branches — not just stax-tracked ones. It uses only git-level merge detection
/// (no metadata-based heuristics), so it works equally well for untracked branches.
///
/// Detection runs in two passes:
///   1. `git branch --merged` against local trunk and (optionally) remote trunk —
///      catches plain (fast-forward / merge-commit) integrations via ancestry.
///   2. Patch-id provenance (`is_branch_merged_equivalent_to_trunk`) for branches
///      not caught by ancestry — catches squash- and rebase-merges where the
///      branch's commits were rewritten into trunk and are no longer ancestors.
///
/// `remote_trunk_ref` is e.g. `"origin/main"` — pass it if available for method 1b.
pub fn find_merged_branches_all(
    repo: &GitRepo,
    workdir: &Path,
    trunk: &str,
    remote_trunk_ref: Option<&str>,
) -> Result<Vec<MergedBranchInfo>> {
    let mut merged: Vec<MergedBranchInfo> = Vec::new();

    // Method 1: git branch --merged <trunk>
    let output = Command::new("git")
        .args(["branch", "--merged", trunk])
        .current_dir(workdir)
        .output()
        .context("Failed to list merged branches")?;

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let branch = line.trim().trim_start_matches("* ");
        if branch.is_empty() || branch == trunk {
            continue;
        }
        merged.push(MergedBranchInfo {
            branch: branch.to_string(),
            merge_type: MergeType::Ancestor,
        });
    }

    // Method 1b: git branch --merged <remote/trunk> (handles stale local trunk)
    if let Some(remote_ref) = remote_trunk_ref {
        let output = Command::new("git")
            .args(["branch", "--merged", remote_ref])
            .current_dir(workdir)
            .output();
        if let Ok(output) = output {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let branch = line.trim().trim_start_matches("* ");
                if branch.is_empty() || branch == trunk {
                    continue;
                }
                if !merged.iter().any(|m| m.branch == branch) {
                    merged.push(MergedBranchInfo {
                        branch: branch.to_string(),
                        merge_type: MergeType::Ancestor,
                    });
                }
            }
        }
    }

    // Method 2: patch-id provenance for squash/rebase merges.
    //
    // `git branch --merged` only finds branches whose tip is an ancestor of
    // trunk. Squash- and rebase-merges rewrite the branch's commits into trunk,
    // so the original branch tip is never an ancestor. We detect these by
    // comparing patch ids: a branch whose patches are all present in trunk is
    // integrated even though it is not an ancestor.
    let already_merged: HashSet<String> = merged.iter().map(|m| m.branch.clone()).collect();
    for branch in repo.list_branches()? {
        if branch == trunk || already_merged.contains(&branch) {
            continue;
        }
        // Conservative: only classify when patch equivalence is unambiguous.
        if repo
            .is_branch_merged_equivalent_to_trunk(&branch)
            .unwrap_or(false)
        {
            merged.push(MergedBranchInfo {
                branch,
                merge_type: MergeType::SquashMerge,
            });
        }
    }

    Ok(merged)
}

/// Find local branches whose remote upstream has been deleted (`[gone]`).
///
/// Requires recent fetch data; results reflect the last `git fetch`.
pub fn find_upstream_gone_branches(workdir: &Path, trunk: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname:short)%00%(upstream:short)%00%(upstream:track)",
            "refs/heads",
        ])
        .current_dir(workdir)
        .output()
        .context("Failed to list local branches with upstream tracking info")?;

    let mut branches: std::collections::BTreeSet<String> = Default::default();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split('\0');
        let branch = fields.next().unwrap_or("").trim();
        let _upstream = fields.next().unwrap_or("").trim();
        let tracking = fields.next().unwrap_or("").trim();

        if branch.is_empty() || branch == trunk {
            continue;
        }
        if tracking.contains("[gone]") {
            branches.insert(branch.to_string());
        }
    }

    Ok(branches.into_iter().collect())
}

/// Returns `true` if `branch` has at least one commit unique relative to
/// EVERY base in `bases` (e.g. local trunk and `origin/<trunk>`).
///
/// This is the safety predicate used to protect upstream-gone branches that
/// still carry local-only work from being deleted. A branch is only considered
/// "safe to delete" (returns `false`) when it has zero unique commits relative
/// to at least one reachable base. Unresolvable bases (e.g. a missing
/// `origin/<trunk>` ref) are skipped rather than treated as "no unique work",
/// so a branch is never classified as deletable on the basis of a base that
/// could not be evaluated.
pub fn has_unique_commits_since_any_base(
    workdir: &Path,
    branch: &str,
    bases: &[&str],
) -> Result<bool> {
    for base in bases {
        let range = format!("{}..{}", base, branch);
        let output = Command::new("git")
            .args(["rev-list", "--count", &range])
            .current_dir(workdir)
            .output()
            .with_context(|| format!("Failed to count unique commits for '{}'", branch))?;

        if !output.status.success() {
            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let unique_count = stdout.trim().parse::<u64>().unwrap_or(u64::MAX);
        if unique_count == 0 {
            return Ok(false);
        }
    }

    Ok(true)
}

/// Find local branches whose most recent commit is older than `stale_days` days.
///
/// Excludes `trunk`, the current branch, and any branches already in
/// `exclude_set` (e.g. already classified as merged/upstream-gone).
pub fn find_stale_branches(
    workdir: &Path,
    trunk: &str,
    current: &str,
    stale_days: u64,
    exclude_set: &HashSet<String>,
) -> Result<Vec<StaleBranchInfo>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname:short)%00%(committerdate:unix)",
            "refs/heads",
        ])
        .current_dir(workdir)
        .output()
        .context("Failed to list branches with commit dates")?;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let stale_threshold_secs = stale_days * 86_400;
    let mut result = Vec::new();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.splitn(2, '\0');
        let branch = fields.next().unwrap_or("").trim();
        let ts_str = fields.next().unwrap_or("").trim();

        if branch.is_empty() || branch == trunk || branch == current {
            continue;
        }
        if exclude_set.contains(branch) {
            continue;
        }
        let commit_ts: u64 = ts_str.parse().unwrap_or(0);
        let age_secs = now_secs.saturating_sub(commit_ts);
        if age_secs >= stale_threshold_secs {
            let days_old = age_secs / 86_400;
            result.push(StaleBranchInfo {
                branch: branch.to_string(),
                days_old,
            });
        }
    }

    Ok(result)
}
