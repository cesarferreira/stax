use super::client::GitHubClient;
use crate::ci::{CheckRunInfo, history, normalize};
use crate::git::GitRepo;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;

const PER_PAGE: usize = 100;

/// Response from the check-runs API (detailed version)
#[derive(Debug, Deserialize)]
struct CheckRunsResponse {
    check_runs: Vec<CheckRunDetail>,
}

#[derive(Debug, Deserialize)]
struct CheckRunDetail {
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    html_url: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
}

fn compatibility_check_runs_overall(check_runs: &[CheckRunDetail]) -> Option<String> {
    let mut latest_by_name: HashMap<&str, &CheckRunDetail> = HashMap::new();
    for run in check_runs {
        let entry = latest_by_name.entry(&run.name).or_insert(run);
        if run.id > entry.id {
            *entry = run;
        }
    }

    if latest_by_name.is_empty() {
        return None;
    }

    let mut has_pending = false;
    let mut has_failure = false;
    let mut all_success = true;

    for run in latest_by_name.values() {
        match run.status.as_str() {
            "completed" => match run.conclusion.as_deref() {
                Some("success") | Some("skipped") | Some("neutral") => {}
                Some("failure")
                | Some("timed_out")
                | Some("cancelled")
                | Some("action_required") => {
                    has_failure = true;
                    all_success = false;
                }
                _ => all_success = false,
            },
            "queued" | "in_progress" | "waiting" | "requested" | "pending" => {
                has_pending = true;
                all_success = false;
            }
            _ => all_success = false,
        }
    }

    if has_failure {
        Some("failure".to_string())
    } else if has_pending {
        Some("pending".to_string())
    } else if all_success {
        Some("success".to_string())
    } else {
        Some("pending".to_string())
    }
}

fn combine_overall_states(
    check_runs_overall: Option<String>,
    statuses_overall: Option<String>,
) -> Option<String> {
    match (check_runs_overall, statuses_overall) {
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
    }
}

fn commit_status_overall(check_runs: &[CheckRunInfo]) -> Option<String> {
    let mut has_pending = false;
    let mut has_failure = false;
    let mut all_success = true;

    for run in check_runs {
        match run.status.as_str() {
            "completed" => match run.conclusion.as_deref() {
                Some("success") => {}
                Some("failure") | Some("error") => {
                    has_failure = true;
                    all_success = false;
                }
                _ => all_success = false,
            },
            "in_progress" | "queued" | "pending" => {
                has_pending = true;
                all_success = false;
            }
            _ => all_success = false,
        }
    }

    if has_failure {
        Some("failure".to_string())
    } else if has_pending {
        Some("pending".to_string())
    } else if all_success && !check_runs.is_empty() {
        Some("success".to_string())
    } else {
        None
    }
}

fn check_runs_overall(check_runs: &[CheckRunInfo]) -> Option<String> {
    if check_runs.is_empty() {
        return None;
    }

    let mut has_pending = false;
    let mut has_failure = false;
    let mut all_success = true;

    for run in check_runs {
        match run.status.as_str() {
            "completed" => match run.conclusion.as_deref() {
                Some("success") | Some("skipped") | Some("neutral") | Some("cancelled") => {}
                Some("failure") | Some("timed_out") | Some("action_required") => {
                    has_failure = true;
                    all_success = false;
                }
                _ => all_success = false,
            },
            "queued" | "in_progress" | "waiting" | "requested" | "pending" => {
                has_pending = true;
                all_success = false;
            }
            _ => all_success = false,
        }
    }

    if has_failure {
        Some("failure".to_string())
    } else if has_pending {
        Some("pending".to_string())
    } else if all_success {
        Some("success".to_string())
    } else {
        Some("pending".to_string())
    }
}

impl GitHubClient {
    /// Get combined CI status from commit statuses and GitHub Actions check runs.
    pub async fn combined_status_state(&self, commit_sha: &str) -> Result<Option<String>> {
        let details = self.fetch_check_run_pages(commit_sha).await?;
        let check_runs_overall = compatibility_check_runs_overall(&details);
        let statuses = self.fetch_commit_status_pages(commit_sha).await?;
        let status_checks =
            normalize::normalize_commit_statuses_without_history(statuses, Utc::now());

        Ok(check_runs_overall
            .or_else(|| commit_status_overall(&status_checks))
            .or(Some("pending".to_string())))
    }

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

        let combined_overall = combine_overall_states(check_runs_overall, statuses_overall);

        Ok((combined_overall, all_checks))
    }

    /// Fetch commit statuses (older CI systems like Buildkite, CircleCI, etc.)
    async fn fetch_commit_status_pages(
        &self,
        commit_sha: &str,
    ) -> Result<Vec<normalize::CommitStatus>> {
        let base_url = format!(
            "/repos/{}/{}/commits/{}/statuses",
            self.owner, self.repo, commit_sha
        );
        let mut statuses = Vec::new();
        let mut page = 1;

        loop {
            self.record_api_call("checks.commit_statuses");
            let url = format!("{base_url}?per_page={PER_PAGE}&page={page}");
            let mut response: Vec<normalize::CommitStatus> = self
                .octocrab
                .get(&url, None::<&()>)
                .await
                .context("Failed to fetch commit statuses")
                .map_err(|e| self.enrich_api_error(e))?;
            let fetched = response.len();
            statuses.append(&mut response);

            if fetched < PER_PAGE {
                break;
            }
            page += 1;
        }

        Ok(statuses)
    }

    async fn fetch_commit_statuses(
        &self,
        repo: &GitRepo,
        commit_sha: &str,
    ) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
        let statuses = self.fetch_commit_status_pages(commit_sha).await?;

        if statuses.is_empty() {
            return Ok((None, Vec::new()));
        }

        let check_runs = normalize::normalize_commit_statuses(repo, statuses, Utc::now());

        let overall = commit_status_overall(&check_runs);

        Ok((overall, check_runs))
    }

    async fn fetch_check_run_pages(&self, commit_sha: &str) -> Result<Vec<CheckRunDetail>> {
        let base_url = format!(
            "/repos/{}/{}/commits/{}/check-runs",
            self.owner, self.repo, commit_sha
        );
        let mut details = Vec::new();
        let mut page = 1;

        loop {
            self.record_api_call("checks.check_runs");
            let url = format!("{base_url}?per_page={PER_PAGE}&page={page}");
            let response: CheckRunsResponse = self
                .octocrab
                .get(&url, None::<&()>)
                .await
                .context("Failed to fetch check runs")
                .map_err(|e| self.enrich_api_error(e))?;
            let fetched = response.check_runs.len();
            details.extend(response.check_runs);

            if fetched < PER_PAGE {
                break;
            }
            page += 1;
        }

        Ok(details)
    }

    async fn fetch_check_runs(
        &self,
        repo: &GitRepo,
        commit_sha: &str,
    ) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
        let details = self.fetch_check_run_pages(commit_sha).await?;

        if details.is_empty() {
            return Ok((None, Vec::new()));
        }

        let now = Utc::now();
        let mut check_runs: Vec<CheckRunInfo> = Vec::new();

        for r in details {
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

        let overall = check_runs_overall(&check_runs);

        Ok((overall, check_runs))
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckRunDetail, CheckRunsResponse, compatibility_check_runs_overall};
    use crate::github::GitHubClient;
    use octocrab::Octocrab;
    use serde_json::Value;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn compatibility_run(status: &str, conclusion: Option<&str>) -> CheckRunDetail {
        CheckRunDetail {
            id: 1,
            name: "build".to_string(),
            status: status.to_string(),
            conclusion: conclusion.map(str::to_string),
            html_url: None,
            started_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn combined_status_state_preserves_pending_like_semantics() {
        for status in ["queued", "in_progress", "waiting", "requested", "pending"] {
            assert_eq!(
                compatibility_check_runs_overall(&[compatibility_run(status, None)]).as_deref(),
                Some("pending")
            );
        }
    }

    #[test]
    fn combined_status_state_preserves_unknown_semantics() {
        assert_eq!(
            compatibility_check_runs_overall(&[compatibility_run("unknown", None)]).as_deref(),
            Some("pending")
        );
        assert_eq!(
            compatibility_check_runs_overall(&[compatibility_run("completed", Some("unknown"))])
                .as_deref(),
            Some("pending")
        );
    }

    #[test]
    fn test_check_runs_response_deserialization() {
        let json = r#"{
            "total_count": 2,
            "check_runs": [
                {"id": 1, "name": "build", "status": "completed", "conclusion": "success", "html_url": "https://example.com/1"},
                {"id": 2, "name": "test", "status": "in_progress", "conclusion": null, "html_url": null}
            ]
        }"#;

        let response: CheckRunsResponse = serde_json::from_str(json).unwrap();
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
        let json = r#"{"id": 1, "name": "lint", "status": "queued", "conclusion": null, "html_url": "https://example.com", "started_at": "2026-01-16T12:00:00Z", "completed_at": null}"#;

        let detail: CheckRunDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.name, "lint");
        assert_eq!(detail.status, "queued");
        assert_eq!(detail.conclusion, None);
        assert_eq!(detail.html_url, Some("https://example.com".to_string()));
        assert_eq!(detail.started_at, Some("2026-01-16T12:00:00Z".to_string()));
    }

    #[tokio::test]
    async fn combined_status_state_uses_paginated_check_runs() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        let octocrab = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .personal_token("test-token".to_string())
            .build()
            .unwrap();
        let client = GitHubClient::with_octocrab(octocrab, "test-owner", "test-repo");
        let check_runs_path = "/repos/test-owner/test-repo/commits/abc123/check-runs";
        let first_page: Vec<Value> = (0..100)
            .map(|index| {
                serde_json::json!({
                    "id": index,
                    "name": if index == 0 { "late-failure".to_string() } else { format!("check-{index}") },
                    "status": "completed",
                    "conclusion": "success",
                    "html_url": null,
                    "started_at": "2026-01-03T00:00:00Z",
                    "completed_at": "2026-01-03T00:01:00Z"
                })
            })
            .collect();
        Mock::given(method("GET"))
            .and(path(check_runs_path))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 101,
                "check_runs": first_page
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(check_runs_path))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 101,
                "check_runs": [{
                    "id": 101,
                    "name": "late-failure",
                    "status": "completed",
                    "conclusion": "failure",
                    "html_url": null,
                    "started_at": "2026-01-02T00:00:00Z",
                    "completed_at": "2026-01-02T00:01:00Z"
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/statuses"))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .expect(1)
            .mount(&server)
            .await;

        let status = client.combined_status_state("abc123").await.unwrap();

        assert_eq!(status.as_deref(), Some("failure"));
        server.verify().await;
    }

    #[tokio::test]
    async fn combined_status_state_prefers_successful_check_runs_over_failed_commit_status() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        let octocrab = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .personal_token("test-token".to_string())
            .build()
            .unwrap();
        let client = GitHubClient::with_octocrab(octocrab, "test-owner", "test-repo");
        Mock::given(method("GET"))
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [{
                    "id": 1,
                    "name": "build",
                    "status": "completed",
                    "conclusion": "success",
                    "html_url": null,
                    "started_at": "2026-01-01T00:00:00Z",
                    "completed_at": "2026-01-01T00:01:00Z"
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/statuses"))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                    "context": "legacy",
                    "state": "failure",
                    "target_url": null,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:01:00Z"
                }])),
            )
            .expect(1)
            .mount(&server)
            .await;

        let status = client.combined_status_state("abc123").await.unwrap();

        assert_eq!(status.as_deref(), Some("success"));
        server.verify().await;
    }

    #[tokio::test]
    async fn combined_status_state_treats_cancelled_check_run_as_failure() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        let octocrab = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .personal_token("test-token".to_string())
            .build()
            .unwrap();
        let client = GitHubClient::with_octocrab(octocrab, "test-owner", "test-repo");
        Mock::given(method("GET"))
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [{
                    "id": 1,
                    "name": "build",
                    "status": "completed",
                    "conclusion": "cancelled",
                    "html_url": null,
                    "started_at": "2026-01-01T00:00:00Z",
                    "completed_at": "2026-01-01T00:01:00Z"
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/statuses"))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .expect(1)
            .mount(&server)
            .await;

        let status = client.combined_status_state("abc123").await.unwrap();

        assert_eq!(status.as_deref(), Some("failure"));
        server.verify().await;
    }

    #[tokio::test]
    async fn combined_status_state_returns_pending_when_all_sources_are_empty() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        let octocrab = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .personal_token("test-token".to_string())
            .build()
            .unwrap();
        let client = GitHubClient::with_octocrab(octocrab, "test-owner", "test-repo");
        Mock::given(method("GET"))
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 0,
                "check_runs": []
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/statuses"))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .expect(1)
            .mount(&server)
            .await;

        let status = client.combined_status_state("abc123").await.unwrap();

        assert_eq!(status.as_deref(), Some("pending"));
        server.verify().await;
    }
}
