use crate::ci::CheckRunInfo;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositorySnapshot {
    pub repository_root: PathBuf,
    pub current_branch: String,
    pub trunk: String,
    pub branches: Vec<BranchSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummary {
    pub name: String,
    pub parent: Option<String>,
    pub column: usize,
    pub is_current: bool,
    pub is_trunk: bool,
    pub needs_restack: bool,
    pub pr_number: Option<u64>,
    pub pr_state: Option<String>,
    pub ci_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchDetails {
    pub ahead: usize,
    pub behind: usize,
    pub has_remote: bool,
    pub unpushed: usize,
    pub unpulled: usize,
    pub commits: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchDiff {
    pub stat: Vec<DiffStatLine>,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffStatLine {
    pub file: String,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub content: String,
    pub kind: DiffLineKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Header,
    Addition,
    Deletion,
    Context,
    Hunk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailRequestToken {
    pub repository: PathBuf,
    pub branch: String,
    pub generation: u64,
}

impl DetailRequestToken {
    pub fn new(repository: impl Into<PathBuf>, branch: impl Into<String>, generation: u64) -> Self {
        Self {
            repository: repository.into(),
            branch: branch.into(),
            generation,
        }
    }

    pub fn matches(&self, repository: impl AsRef<Path>, branch: &str, generation: u64) -> bool {
        self.repository.as_path() == repository.as_ref()
            && self.branch == branch
            && self.generation == generation
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiSummary {
    pub overall_status: Option<String>,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub running: usize,
    pub queued: usize,
    pub skipped: usize,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub average_secs: Option<u64>,
}

impl CiSummary {
    #[allow(dead_code)]
    pub(crate) fn from_checks(
        overall_status: Option<String>,
        checks: &[CheckRunInfo],
        average_secs: Option<u64>,
    ) -> Self {
        let mut passed = 0;
        let mut failed = 0;
        let mut running = 0;
        let mut queued = 0;
        let mut skipped = 0;

        for check in checks {
            match check.status.as_str() {
                "completed" => match check.conclusion.as_deref() {
                    Some("success") => passed += 1,
                    Some("skipped") | Some("neutral") | Some("cancelled") => skipped += 1,
                    _ => failed += 1,
                },
                "in_progress" => running += 1,
                "queued" | "waiting" | "requested" | "pending" => queued += 1,
                _ => queued += 1,
            }
        }

        let started_at = checks
            .iter()
            .filter_map(|check| parse_ci_timestamp(check.started_at.as_deref()))
            .min();
        let completed_at = if checks.iter().all(|check| check.status == "completed") {
            checks
                .iter()
                .filter_map(|check| parse_ci_timestamp(check.completed_at.as_deref()))
                .max()
        } else {
            None
        };

        Self {
            overall_status,
            total: checks.len(),
            passed,
            failed,
            running,
            queued,
            skipped,
            started_at,
            completed_at,
            average_secs,
        }
    }

    pub fn has_checks(&self) -> bool {
        self.total > 0
    }

    pub fn completed_count(&self) -> usize {
        self.passed + self.failed + self.skipped
    }

    pub fn is_active(&self) -> bool {
        !self.is_complete() && (self.running > 0 || self.queued > 0)
    }

    pub fn is_complete(&self) -> bool {
        self.total > 0 && self.completed_count() == self.total
    }

    pub fn progress_percent(&self, now: DateTime<Utc>) -> Option<u8> {
        if self.is_complete() {
            return Some(100);
        }

        let average_secs = self.average_secs?;
        let elapsed_secs = self.elapsed_secs(now)?;
        if average_secs == 0 {
            return Some(99);
        }

        Some(if elapsed_secs >= average_secs {
            99
        } else {
            ((elapsed_secs * 100) / average_secs).min(99) as u8
        })
    }

    pub fn elapsed_secs(&self, now: DateTime<Utc>) -> Option<u64> {
        let started_at = self.started_at?;
        let finished_at = if self.is_complete() {
            self.completed_at.unwrap_or(now)
        } else {
            now
        };
        Some(
            finished_at
                .signed_duration_since(started_at)
                .num_seconds()
                .max(0) as u64,
        )
    }

    pub fn eta_secs(&self, now: DateTime<Utc>) -> Option<u64> {
        if self.is_complete() {
            return Some(0);
        }

        let average_secs = self.average_secs?;
        let elapsed_secs = self.elapsed_secs(now)?;
        Some(average_secs.saturating_sub(elapsed_secs))
    }
}

#[allow(dead_code)]
fn parse_ci_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    value.and_then(|timestamp| timestamp.parse::<DateTime<Utc>>().ok())
}

#[cfg(test)]
mod tests {
    use super::{CiSummary, DetailRequestToken};
    use crate::ci::CheckRunInfo;
    use chrono::{TimeZone, Utc};

    fn check(name: &str, status: &str, conclusion: Option<&str>) -> CheckRunInfo {
        CheckRunInfo {
            name: name.into(),
            status: status.into(),
            conclusion: conclusion.map(str::to_string),
            url: None,
            started_at: None,
            completed_at: None,
            elapsed_secs: None,
            average_secs: None,
            completion_percent: None,
        }
    }

    #[test]
    fn request_tokens_match_only_the_same_repository_branch_and_generation() {
        let token = DetailRequestToken::new("/repo", "feature", 7);

        assert!(token.matches("/repo", "feature", 7));
        assert!(!token.matches("/other-repo", "feature", 7));
        assert!(!token.matches("/repo", "other", 7));
        assert!(!token.matches("/repo", "feature", 8));
    }

    #[test]
    fn ci_summary_counts_terminal_and_active_checks() {
        let summary = CiSummary::from_checks(
            Some("pending".into()),
            &[
                check("build", "completed", Some("success")),
                check("lint", "completed", Some("failure")),
                check("docs", "completed", Some("neutral")),
                check("test", "in_progress", None),
                check("deploy", "waiting", None),
                check("scan", "requested", None),
                check("custom", "unknown", None),
            ],
            Some(120),
        );

        assert_eq!(summary.overall_status.as_deref(), Some("pending"));
        assert_eq!(summary.total, 7);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.running, 1);
        assert_eq!(summary.queued, 3);
        assert_eq!(summary.average_secs, Some(120));
        assert_eq!(summary.completed_count(), 3);
        assert!(summary.has_checks());
        assert!(summary.is_active());
        assert!(!summary.is_complete());
    }

    #[test]
    fn empty_ci_summary_has_no_progress_or_activity() {
        let summary = CiSummary::from_checks(None, &[], None);
        let now = Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap();

        assert!(!summary.has_checks());
        assert_eq!(summary.completed_count(), 0);
        assert!(!summary.is_active());
        assert!(!summary.is_complete());
        assert_eq!(summary.started_at, None);
        assert_eq!(summary.completed_at, None);
        assert_eq!(summary.elapsed_secs(now), None);
        assert_eq!(summary.progress_percent(now), None);
        assert_eq!(summary.eta_secs(now), None);
    }

    #[test]
    fn completed_ci_summary_reports_final_progress_and_timestamps() {
        let mut build = check("build", "completed", Some("success"));
        build.started_at = Some("2026-07-12T10:00:00Z".into());
        build.completed_at = Some("2026-07-12T10:02:00Z".into());
        let mut test = check("test", "completed", Some("skipped"));
        test.started_at = Some("2026-07-12T10:01:00Z".into());
        test.completed_at = Some("2026-07-12T10:04:00Z".into());

        let summary = CiSummary::from_checks(Some("success".into()), &[build, test], Some(300));
        let now = Utc.with_ymd_and_hms(2026, 7, 12, 11, 0, 0).unwrap();

        assert!(summary.is_complete());
        assert!(!summary.is_active());
        assert_eq!(
            summary.started_at,
            Some(Utc.with_ymd_and_hms(2026, 7, 12, 10, 0, 0).unwrap())
        );
        assert_eq!(
            summary.completed_at,
            Some(Utc.with_ymd_and_hms(2026, 7, 12, 10, 4, 0).unwrap())
        );
        assert_eq!(summary.elapsed_secs(now), Some(240));
        assert_eq!(summary.progress_percent(now), Some(100));
        assert_eq!(summary.eta_secs(now), Some(0));
    }

    #[test]
    fn invalid_and_missing_timestamps_degrade_to_unknown_timing() {
        let mut running = check("build", "in_progress", None);
        running.started_at = Some("not-a-timestamp".into());
        running.completed_at = Some("also-not-a-timestamp".into());
        let queued = check("test", "queued", None);

        let summary = CiSummary::from_checks(Some("pending".into()), &[running, queued], Some(120));
        let now = Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap();

        assert!(summary.is_active());
        assert_eq!(summary.started_at, None);
        assert_eq!(summary.completed_at, None);
        assert_eq!(summary.elapsed_secs(now), None);
        assert_eq!(summary.progress_percent(now), None);
        assert_eq!(summary.eta_secs(now), None);
    }
}
