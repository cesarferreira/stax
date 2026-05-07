//! Pre-flight sanity check for restack.
//!
//! Compares the **stored** rebase boundary (`parentBranchRevision`) against the
//! current `merge-base(parent, branch)`. When the stored boundary would force
//! Git to replay a much larger range than the merge-base would, restack tends
//! to surface conflicts on files the user never edited on this branch (because
//! the stored boundary drifted, or the user merged trunk into the branch).
//!
//! This module corrects the rebase boundary automatically when the stored
//! boundary is clearly worse than the current merge-base. The correction is
//! intentionally conservative and only affects the current rebase invocation;
//! normal restack success still refreshes metadata to the parent tip.

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

    /// Whether this report has a merge-base boundary that can be used instead
    /// of the stored boundary.
    pub fn corrected_upstream(&self) -> Option<&str> {
        if !self.is_suspicious() {
            return None;
        }
        self.merge_base.as_deref()
    }

    /// Print a one-line notice explaining the automatic correction. Caller
    /// should gate this on `corrected_upstream()` and on the user's config.
    pub fn print_correction(&self) {
        let stored = self
            .stored_to_branch
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".to_string());
        let mb = self
            .merge_base_to_branch
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".to_string());

        println!(
            "  {} '{}' stored boundary would replay {} commit(s); using merge-base boundary ({} commit(s)).",
            "preflight:".yellow().bold(),
            self.branch,
            stored.yellow(),
            mb.cyan()
        );
        println!(
            "    {}",
            format!(
                "Stax will rebase '{}' with the merge-base boundary to avoid replaying unrelated history from '{}'.",
                self.branch,
                self.parent
            )
            .dimmed()
        );
    }
}

/// Return the upstream boundary to pass to `git rebase --onto`.
///
/// If preflight repair is enabled and the stored boundary would replay a much
/// larger range than the current merge-base, returns the merge-base instead.
/// Never fails the caller — any git error during analysis falls back to the
/// stored revision and the rebase proceeds normally.
pub fn choose_rebase_upstream(
    repo: &GitRepo,
    config: &Config,
    branch: &str,
    parent: &str,
    stored_revision: &str,
    quiet: bool,
) -> String {
    if !config.restack.preflight_auto_repair {
        return stored_revision.to_string();
    }

    if let Ok(report) = RestackPreflight::analyze(repo, branch, parent, stored_revision) {
        if let Some(upstream) = report.corrected_upstream() {
            let upstream = upstream.to_string();
            if !quiet && config.restack.preflight_warn {
                report.print_correction();
            }
            return upstream;
        }
    }

    stored_revision.to_string()
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
    fn suspicious_report_uses_merge_base_as_corrected_upstream() {
        assert_eq!(pf(Some(60), Some(2)).corrected_upstream(), Some("def"));
        assert_eq!(pf(Some(10), Some(2)).corrected_upstream(), None);
    }

    #[test]
    fn boundary_uses_absolute_headroom() {
        assert!(!pf(Some(MIN_STORED_RANGE_FOR_WARNING), Some(5)).is_suspicious());
        assert!(pf(Some(60), Some(0)).is_suspicious());
    }
}
