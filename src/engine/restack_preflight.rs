//! Pre-flight sanity check for restack.
//!
//! Compares the **stored** rebase boundary (`parentBranchRevision`) against the
//! current `merge-base(parent, branch)`. When the stored boundary would force
//! Git to replay a much larger range than the merge-base would, restack tends
//! to surface conflicts on files the user never edited on this branch (because
//! the stored boundary drifted, or the user merged trunk into the branch).
//!
//! This module produces a non-fatal advisory: it never blocks a restack. The
//! intent is to give engineers in busy monorepos enough evidence to choose
//! between resolving conflicts or repairing metadata first via
//! `stax branch reparent --parent <p> --branch <b>`.

use crate::config::Config;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

/// Minimum stored-range size before we even consider warning. Tiny drifts on
/// short-lived branches are noise, not signal.
const MIN_STORED_RANGE_FOR_WARNING: usize = 25;

/// Stored range must exceed `merge_base_range * RANGE_RATIO + ABSOLUTE_HEADROOM`
/// before we warn. Both checks together avoid noisy advisories on short stacks
/// while still catching the “stored is ancient, real divergence is small” case
/// that produces conflicts on untouched files.
const RANGE_RATIO: usize = 4;
const ABSOLUTE_HEADROOM: usize = 5;

/// Result of a per-branch preflight analysis.
#[derive(Debug, Clone)]
pub struct RestackPreflight {
    pub branch: String,
    pub parent: String,
    /// Stored `parentBranchRevision` we evaluated against; kept for diagnostics
    /// and future repair tooling even though `is_suspicious` does not read it.
    #[allow(dead_code)]
    pub stored_revision: String,
    /// Computed `merge-base(parent, branch)` when available; same rationale as
    /// `stored_revision`.
    #[allow(dead_code)]
    pub merge_base: Option<String>,
    pub stored_to_branch: Option<usize>,
    pub merge_base_to_branch: Option<usize>,
}

impl RestackPreflight {
    /// Analyse a single branch about to be restacked. All git failures are
    /// swallowed into `None` fields so the caller can decide to skip the
    /// advisory rather than abort restack.
    pub fn analyze(
        repo: &GitRepo,
        branch: &str,
        parent: &str,
        stored_revision: &str,
    ) -> Result<Self> {
        let workdir = repo.workdir()?.to_path_buf();

        let stored_to_branch = if stored_revision.trim().is_empty() {
            None
        } else {
            repo.rev_list_count(&workdir, &format!("{stored_revision}..{branch}"))
                .ok()
        };

        let merge_base = repo.merge_base(parent, branch).ok();
        let merge_base_to_branch = match merge_base.as_deref() {
            Some(mb) => repo
                .rev_list_count(&workdir, &format!("{mb}..{branch}"))
                .ok(),
            None => None,
        };

        Ok(Self {
            branch: branch.to_string(),
            parent: parent.to_string(),
            stored_revision: stored_revision.to_string(),
            merge_base,
            stored_to_branch,
            merge_base_to_branch,
        })
    }

    /// Heuristic: stored boundary inflates the replay range enough that the
    /// rebase will likely touch files unrelated to the user's own commits.
    pub fn is_suspicious(&self) -> bool {
        let Some(stored) = self.stored_to_branch else {
            return false;
        };
        let Some(mb) = self.merge_base_to_branch else {
            return false;
        };

        if stored < MIN_STORED_RANGE_FOR_WARNING {
            return false;
        }

        stored > mb.saturating_mul(RANGE_RATIO) + ABSOLUTE_HEADROOM
    }

    /// Print a one-line warning followed by a repair hint. Caller should gate
    /// this on `is_suspicious()` and on the user's preflight config.
    pub fn print_warning(&self) {
        let stored = self
            .stored_to_branch
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".to_string());
        let mb = self
            .merge_base_to_branch
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".to_string());

        println!(
            "  {} '{}' stored boundary will replay {} commit(s); merge-base would replay {}.",
            "preflight:".yellow().bold(),
            self.branch,
            stored.yellow(),
            mb.cyan()
        );
        println!(
            "    {}",
            format!(
                "Tip: if conflicts hit unrelated files, abort and run `stax branch reparent --parent {} --branch {}` to retarget at the merge-base.",
                self.parent, self.branch
            )
            .dimmed()
        );
    }
}

/// Convenience: analyse and, if both the heuristic and config agree, print the
/// advisory. Always silent when `quiet` is true. Never fails the caller — any
/// git error during analysis is swallowed.
pub fn maybe_warn(
    repo: &GitRepo,
    config: &Config,
    branch: &str,
    parent: &str,
    stored_revision: &str,
    quiet: bool,
) {
    if quiet || !config.restack.preflight_warn {
        return;
    }
    if let Ok(report) = RestackPreflight::analyze(repo, branch, parent, stored_revision) {
        if report.is_suspicious() {
            report.print_warning();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pf(stored: Option<usize>, mb: Option<usize>) -> RestackPreflight {
        RestackPreflight {
            branch: "feat".into(),
            parent: "main".into(),
            stored_revision: "abc".into(),
            merge_base: Some("def".into()),
            stored_to_branch: stored,
            merge_base_to_branch: mb,
        }
    }

    #[test]
    fn missing_counts_are_never_suspicious() {
        assert!(!pf(None, Some(0)).is_suspicious());
        assert!(!pf(Some(0), None).is_suspicious());
        assert!(!pf(None, None).is_suspicious());
    }

    #[test]
    fn small_stored_range_is_never_suspicious() {
        assert!(!pf(Some(MIN_STORED_RANGE_FOR_WARNING - 1), Some(0)).is_suspicious());
        assert!(!pf(Some(10), Some(2)).is_suspicious());
    }

    #[test]
    fn proportional_stored_range_is_not_suspicious() {
        assert!(!pf(Some(40), Some(35)).is_suspicious());
        assert!(!pf(Some(40), Some(10)).is_suspicious());
    }

    #[test]
    fn drifted_stored_range_is_suspicious() {
        assert!(pf(Some(60), Some(2)).is_suspicious());
        assert!(pf(Some(120), Some(3)).is_suspicious());
    }

    #[test]
    fn boundary_uses_absolute_headroom() {
        assert!(!pf(Some(MIN_STORED_RANGE_FOR_WARNING), Some(5)).is_suspicious());
        assert!(pf(Some(60), Some(0)).is_suspicious());
    }
}
