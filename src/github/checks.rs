use super::client::GitHubClient;
use crate::ci::{CheckRunInfo, history, normalize};
use crate::git::GitRepo;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Response from the check-runs API (detailed version)
#[derive(Debug, Deserialize)]
struct CheckRunsResponse {
    total_count: usize,
    check_runs: Vec<CheckRunDetail>,
}

#[derive(Debug, Deserialize)]
struct CheckRunDetail {
    name: String,
    status: String,
    conclusion: Option<String>,
    html_url: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
}

impl GitHubClient {
    /// Fetch all checks (both check runs and commit statuses), deduplicated
    pub(crate) async fn fetch_checks(
        &self,
        repo: &GitRepo,
        sha: &str,
    ) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
        let (check_runs_overall, mut all_checks) = self.fetch_check_runs(repo, sha).await?;
        let (statuses_overall, status_checks) = self.fetch_commit_statuses(repo, sha).await?;

        all_checks.extend(status_checks);

        // Deduplicate across both sources, keeping most recent per name
        all_checks = normalize::dedup_check_runs(all_checks);

        let combined_overall = match (check_runs_overall, statuses_overall) {
            (Some(ref a), Some(ref b)) if a == "failure" || b == "failure" => {
                Some("failure".to_string())
            }
            (Some(ref a), Some(ref b)) if a == "pending" || b == "pending" => {
                Some("pending".to_string())
            }
            (Some(a), Some(_)) => Some(a),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        Ok((combined_overall, all_checks))
    }

    /// Fetch commit statuses (older CI systems like Buildkite, CircleCI, etc.)
    async fn fetch_commit_statuses(
        &self,
        repo: &GitRepo,
        commit_sha: &str,
    ) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
        let url = format!(
            "/repos/{}/{}/commits/{}/statuses",
            self.owner, self.repo, commit_sha
        );

        let statuses: Vec<normalize::CommitStatus> =
            match self.octocrab.get(&url, None::<&()>).await {
                Ok(s) => s,
                Err(_) => return Ok((None, Vec::new())),
            };

        if statuses.is_empty() {
            return Ok((None, Vec::new()));
        }

        let check_runs = normalize::normalize_commit_statuses(repo, statuses, Utc::now());

        let mut has_pending = false;
        let mut has_failure = false;
        let mut all_success = true;

        for run in &check_runs {
            match run.status.as_str() {
                "completed" => match run.conclusion.as_deref() {
                    Some("success") => {}
                    Some("failure") | Some("error") => {
                        has_failure = true;
                        all_success = false;
                    }
                    _ => {
                        all_success = false;
                    }
                },
                "in_progress" | "queued" | "pending" => {
                    has_pending = true;
                    all_success = false;
                }
                _ => {
                    all_success = false;
                }
            }
        }

        let overall = if has_failure {
            Some("failure".to_string())
        } else if has_pending {
            Some("pending".to_string())
        } else if all_success && !check_runs.is_empty() {
            Some("success".to_string())
        } else {
            None
        };

        Ok((overall, check_runs))
    }

    async fn fetch_check_runs(
        &self,
        repo: &GitRepo,
        commit_sha: &str,
    ) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
        let url = format!(
            "/repos/{}/{}/commits/{}/check-runs",
            self.owner, self.repo, commit_sha
        );

        let response: CheckRunsResponse = self.octocrab.get(&url, None::<&()>).await?;

        if response.total_count == 0 {
            return Ok((None, Vec::new()));
        }

        let now = Utc::now();
        let mut check_runs: Vec<CheckRunInfo> = Vec::new();

        for r in response.check_runs {
            let (elapsed_secs, completed_at_str) = if let Some(completed) = &r.completed_at {
                if let (Some(started), Ok(completed_time)) = (
                    r.started_at
                        .as_ref()
                        .and_then(|s| s.parse::<DateTime<Utc>>().ok()),
                    completed.parse::<DateTime<Utc>>(),
                ) {
                    let duration = completed_time.signed_duration_since(started);
                    let secs = duration.num_seconds();
                    if secs >= 0 {
                        (Some(secs as u64), Some(completed.clone()))
                    } else {
                        (None, Some(completed.clone()))
                    }
                } else {
                    (None, Some(completed.clone()))
                }
            } else if let Some(started) = &r.started_at {
                if let Ok(started_time) = started.parse::<DateTime<Utc>>() {
                    let duration = now.signed_duration_since(started_time);
                    let secs = duration.num_seconds();
                    if secs >= 0 {
                        (Some(secs as u64), None)
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            };

            let average_secs = match history::load_check_history(repo, &r.name) {
                Ok(hist) => history::calculate_average(&hist),
                Err(_) => None,
            };

            let completion_percent = if r.status == "in_progress" {
                if let (Some(elapsed), Some(avg)) = (elapsed_secs, average_secs) {
                    (elapsed * 100).checked_div(avg).map(|v| v.min(99) as u8)
                } else {
                    None
                }
            } else {
                None
            };

            check_runs.push(CheckRunInfo {
                name: r.name,
                status: r.status,
                conclusion: r.conclusion,
                url: r.html_url,
                started_at: r.started_at,
                completed_at: completed_at_str,
                elapsed_secs,
                average_secs,
                completion_percent,
            });
        }

        // Deduplicate within check runs
        check_runs = normalize::dedup_check_runs(check_runs);

        let mut has_pending = false;
        let mut has_failure = false;
        let mut all_success = true;

        for run in &check_runs {
            match run.status.as_str() {
                "completed" => match run.conclusion.as_deref() {
                    Some("success") | Some("skipped") | Some("neutral") | Some("cancelled") => {}
                    Some("failure") | Some("timed_out") | Some("action_required") => {
                        has_failure = true;
                        all_success = false;
                    }
                    _ => {
                        all_success = false;
                    }
                },
                "queued" | "in_progress" | "waiting" | "requested" | "pending" => {
                    has_pending = true;
                    all_success = false;
                }
                _ => {
                    all_success = false;
                }
            }
        }

        let overall = if has_failure {
            Some("failure".to_string())
        } else if has_pending {
            Some("pending".to_string())
        } else if all_success {
            Some("success".to_string())
        } else {
            Some("pending".to_string())
        };

        Ok((overall, check_runs))
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckRunDetail, CheckRunsResponse};

    #[test]
    fn test_check_runs_response_deserialization() {
        let json = r#"{
            "total_count": 2,
            "check_runs": [
                {"name": "build", "status": "completed", "conclusion": "success", "html_url": "https://example.com/1"},
                {"name": "test", "status": "in_progress", "conclusion": null, "html_url": null}
            ]
        }"#;

        let response: CheckRunsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total_count, 2);
        assert_eq!(response.check_runs.len(), 2);
        assert_eq!(response.check_runs[0].name, "build");
        assert_eq!(
            response.check_runs[0].conclusion,
            Some("success".to_string())
        );
        assert_eq!(response.check_runs[1].name, "test");
        assert_eq!(response.check_runs[1].conclusion, None);
    }

    #[test]
    fn test_check_run_detail_deserialization() {
        let json = r#"{"name": "lint", "status": "queued", "conclusion": null, "html_url": "https://example.com", "started_at": "2026-01-16T12:00:00Z", "completed_at": null}"#;

        let detail: CheckRunDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.name, "lint");
        assert_eq!(detail.status, "queued");
        assert_eq!(detail.conclusion, None);
        assert_eq!(detail.html_url, Some("https://example.com".to_string()));
        assert_eq!(detail.started_at, Some("2026-01-16T12:00:00Z".to_string()));
    }
}
