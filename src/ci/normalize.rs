use super::CheckRunInfo;
use super::history;
use crate::git::GitRepo;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;

/// Response from the commit statuses API
#[derive(Debug, Deserialize)]
pub(crate) struct CommitStatus {
    pub context: String,
    pub state: String,
    pub target_url: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Deduplicate check runs by name, keeping only the most recent for each
pub(crate) fn dedup_check_runs(check_runs: Vec<CheckRunInfo>) -> Vec<CheckRunInfo> {
    let mut unique_checks: HashMap<String, CheckRunInfo> = HashMap::new();
    for check in check_runs {
        let should_replace = if let Some(existing) = unique_checks.get(&check.name) {
            match (&check.started_at, &existing.started_at) {
                (Some(new_start), Some(existing_start)) => {
                    if let (Ok(new_time), Ok(existing_time)) = (
                        new_start.parse::<DateTime<Utc>>(),
                        existing_start.parse::<DateTime<Utc>>(),
                    ) {
                        new_time > existing_time
                    } else {
                        false
                    }
                }
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => true,
            }
        } else {
            true
        };

        if should_replace {
            unique_checks.insert(check.name.clone(), check);
        }
    }

    let mut result: Vec<CheckRunInfo> = unique_checks.into_values().collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

pub(crate) fn normalize_commit_statuses(
    repo: &GitRepo,
    statuses: Vec<CommitStatus>,
    now: DateTime<Utc>,
) -> Vec<CheckRunInfo> {
    let mut by_context: HashMap<String, Vec<CommitStatus>> = HashMap::new();
    for status in statuses {
        by_context
            .entry(status.context.clone())
            .or_default()
            .push(status);
    }

    let mut check_runs = Vec::new();
    for (context, events) in by_context {
        if let Some(check_run) = normalize_commit_status_context(repo, &context, &events, now) {
            check_runs.push(check_run);
        }
    }

    check_runs.sort_by(|a, b| a.name.cmp(&b.name));
    check_runs
}

fn normalize_commit_status_context(
    repo: &GitRepo,
    context: &str,
    events: &[CommitStatus],
    now: DateTime<Utc>,
) -> Option<CheckRunInfo> {
    let latest = events
        .iter()
        .max_by_key(|status| commit_status_event_time(status))?;
    let latest_time = commit_status_event_time(latest)?;

    let average_secs = match history::load_check_history(repo, context) {
        Ok(hist) => history::calculate_average(&hist),
        Err(_) => None,
    };

    let pending_start = events
        .iter()
        .filter(|status| status.state == "pending")
        .filter_map(commit_status_event_time)
        .filter(|time| *time <= latest_time)
        .max();

    let (status, conclusion, started_at, completed_at, elapsed_secs) = match latest.state.as_str() {
        "success" => (
            "completed".to_string(),
            Some("success".to_string()),
            pending_start.map(|time| time.to_rfc3339()),
            Some(latest_time.to_rfc3339()),
            pending_start
                .map(|time| latest_time.signed_duration_since(time).num_seconds().max(0) as u64),
        ),
        "failure" | "error" => (
            "completed".to_string(),
            Some("failure".to_string()),
            pending_start.map(|time| time.to_rfc3339()),
            Some(latest_time.to_rfc3339()),
            pending_start
                .map(|time| latest_time.signed_duration_since(time).num_seconds().max(0) as u64),
        ),
        "pending" => (
            "in_progress".to_string(),
            None,
            Some(latest_time.to_rfc3339()),
            None,
            Some(now.signed_duration_since(latest_time).num_seconds().max(0) as u64),
        ),
        _ => (
            "queued".to_string(),
            None,
            latest.created_at.clone(),
            latest.updated_at.clone(),
            None,
        ),
    };

    let completion_percent = if status == "in_progress" {
        if let (Some(elapsed), Some(avg)) = (elapsed_secs, average_secs) {
            (elapsed * 100).checked_div(avg).map(|v| v.min(99) as u8)
        } else {
            None
        }
    } else {
        None
    };

    Some(CheckRunInfo {
        name: context.to_string(),
        status,
        conclusion,
        url: latest.target_url.clone(),
        started_at,
        completed_at,
        elapsed_secs,
        average_secs,
        completion_percent,
    })
}

fn commit_status_event_time(status: &CommitStatus) -> Option<DateTime<Utc>> {
    status
        .created_at
        .as_deref()
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        .or_else(|| {
            status
                .updated_at
                .as_deref()
                .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        })
}

#[cfg(test)]
mod tests {
    use super::{CheckRunInfo, CommitStatus, dedup_check_runs, normalize_commit_status_context};
    use crate::ci::history;
    use crate::git::GitRepo;
    use chrono::{TimeZone, Utc};
    use std::process::Command;
    use tempfile::TempDir;

    fn init_temp_repo() -> (TempDir, GitRepo) {
        let tempdir = TempDir::new().unwrap();
        let status = Command::new("git")
            .args(["init"])
            .current_dir(tempdir.path())
            .status()
            .unwrap();
        assert!(status.success());

        let repo = GitRepo::open_from_path(tempdir.path()).unwrap();
        (tempdir, repo)
    }

    #[test]
    fn test_dedup_check_runs_keeps_most_recent() {
        let older = CheckRunInfo {
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            url: None,
            started_at: Some("2026-01-16T12:00:00Z".to_string()),
            completed_at: Some("2026-01-16T12:02:00Z".to_string()),
            elapsed_secs: Some(120),
            average_secs: None,
            completion_percent: None,
        };
        let newer = CheckRunInfo {
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
            url: None,
            started_at: Some("2026-01-16T13:00:00Z".to_string()),
            completed_at: Some("2026-01-16T13:02:00Z".to_string()),
            elapsed_secs: Some(120),
            average_secs: None,
            completion_percent: None,
        };

        let result = dedup_check_runs(vec![older, newer]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].conclusion, Some("failure".to_string()));
    }

    #[test]
    fn test_dedup_check_runs_different_names() {
        let build = CheckRunInfo {
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            url: None,
            started_at: None,
            completed_at: None,
            elapsed_secs: None,
            average_secs: None,
            completion_percent: None,
        };
        let test = CheckRunInfo {
            name: "test".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            url: None,
            started_at: None,
            completed_at: None,
            elapsed_secs: None,
            average_secs: None,
            completion_percent: None,
        };

        let result = dedup_check_runs(vec![build, test]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_normalize_commit_status_context_pending_tracks_elapsed_from_created_at() {
        let (_tempdir, repo) = init_temp_repo();
        history::add_timing_sample(
            &repo,
            "android suite",
            1500,
            "2026-01-16T11:00:00Z".to_string(),
            None,
        )
        .unwrap();

        let now = Utc.with_ymd_and_hms(2026, 1, 16, 12, 25, 0).unwrap();
        let events = vec![CommitStatus {
            context: "android suite".to_string(),
            state: "pending".to_string(),
            target_url: None,
            created_at: Some("2026-01-16T12:00:00Z".to_string()),
            updated_at: Some("2026-01-16T12:00:00Z".to_string()),
        }];

        let run = normalize_commit_status_context(&repo, "android suite", &events, now).unwrap();
        assert_eq!(run.status, "in_progress");
        assert_eq!(run.elapsed_secs, Some(1500));
        assert_eq!(run.average_secs, Some(1500));
        assert_eq!(run.completion_percent, Some(99));
    }

    #[test]
    fn test_normalize_commit_status_context_uses_pending_to_success_duration() {
        let (_tempdir, repo) = init_temp_repo();
        let now = Utc.with_ymd_and_hms(2026, 1, 16, 12, 30, 0).unwrap();
        let events = vec![
            CommitStatus {
                context: "android suite".to_string(),
                state: "pending".to_string(),
                target_url: Some("https://example.com/pending".to_string()),
                created_at: Some("2026-01-16T12:00:00Z".to_string()),
                updated_at: Some("2026-01-16T12:00:00Z".to_string()),
            },
            CommitStatus {
                context: "android suite".to_string(),
                state: "success".to_string(),
                target_url: Some("https://example.com/success".to_string()),
                created_at: Some("2026-01-16T12:25:00Z".to_string()),
                updated_at: Some("2026-01-16T12:25:00Z".to_string()),
            },
        ];

        let run = normalize_commit_status_context(&repo, "android suite", &events, now).unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(run.conclusion.as_deref(), Some("success"));
        assert_eq!(run.started_at.as_deref(), Some("2026-01-16T12:00:00+00:00"));
        assert_eq!(
            run.completed_at.as_deref(),
            Some("2026-01-16T12:25:00+00:00")
        );
        assert_eq!(run.elapsed_secs, Some(1500));
        assert_eq!(run.url.as_deref(), Some("https://example.com/success"));
    }
}
