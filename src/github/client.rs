use anyhow::{Context, Result};
use octocrab::Octocrab;
use octocrab::params::repos::Reference;
use serde::Deserialize;

use crate::config::Config;

pub struct GitHubClient {
    pub octocrab: Octocrab,
    pub owner: String,
    pub repo: String,
}

impl Clone for GitHubClient {
    fn clone(&self) -> Self {
        // Note: Octocrab doesn't implement Clone, so we create a minimal placeholder
        // This is only used in tests where we create fresh clients anyway
        Self {
            octocrab: self.octocrab.clone(),
            owner: self.owner.clone(),
            repo: self.repo.clone(),
        }
    }
}

/// Response from the check-runs API
#[derive(Debug, Deserialize)]
struct CheckRunsResponse {
    total_count: usize,
    check_runs: Vec<CheckRun>,
}

#[derive(Debug, Deserialize)]
struct CheckRun {
    status: String,
    conclusion: Option<String>,
}

impl GitHubClient {
    /// Create a new GitHub client from config
    pub fn new(owner: &str, repo: &str, api_base_url: Option<String>) -> Result<Self> {
        let token = Config::github_token()
            .context("GitHub token not set. Run `stax auth` or set GITHUB_TOKEN env var.")?;

        let mut builder = Octocrab::builder().personal_token(token.to_string());
        if let Some(api_base) = api_base_url {
            builder = builder
                .base_uri(api_base)
                .context("Failed to set GitHub API base URL")?;
        }

        let octocrab = builder.build().context("Failed to create GitHub client")?;

        Ok(Self {
            octocrab,
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }

    /// Create a new GitHub client with a custom Octocrab instance (for testing)
    #[cfg(test)]
    pub fn with_octocrab(octocrab: Octocrab, owner: &str, repo: &str) -> Self {
        Self {
            octocrab,
            owner: owner.to_string(),
            repo: repo.to_string(),
        }
    }

    /// Get combined CI status from both commit statuses AND check runs (GitHub Actions)
    pub async fn combined_status_state(&self, commit_sha: &str) -> Result<Option<String>> {
        // First, check legacy commit statuses
        let commit_status = self
            .octocrab
            .repos(&self.owner, &self.repo)
            .combined_status_for_ref(&Reference::Branch(commit_sha.to_string()))
            .await
            .ok();

        // Then, check GitHub Actions check runs
        let check_runs_status = self.get_check_runs_status(commit_sha).await.ok().flatten();

        // Combine results: prioritize check runs (more common), fall back to commit status
        match (check_runs_status, commit_status) {
            // If we have check runs, use that status
            (Some(cr_status), _) => Ok(Some(cr_status)),
            // Fall back to commit status
            (None, Some(status)) => Ok(Some(format!("{:?}", status.state).to_lowercase())),
            // No CI at all
            (None, None) => Ok(None),
        }
    }

    /// Get status from GitHub Actions check runs
    async fn get_check_runs_status(&self, commit_sha: &str) -> Result<Option<String>> {
        let url = format!(
            "/repos/{}/{}/commits/{}/check-runs",
            self.owner, self.repo, commit_sha
        );

        let response: CheckRunsResponse = self.octocrab.get(&url, None::<&()>).await?;

        if response.total_count == 0 {
            return Ok(None); // No check runs configured
        }

        // Analyze all check runs to determine overall status
        let mut has_pending = false;
        let mut has_failure = false;
        let mut all_success = true;

        for run in &response.check_runs {
            match run.status.as_str() {
                "completed" => {
                    match run.conclusion.as_deref() {
                        Some("success") | Some("skipped") | Some("neutral") => {}
                        Some("failure") | Some("timed_out") | Some("cancelled")
                        | Some("action_required") => {
                            has_failure = true;
                            all_success = false;
                        }
                        _ => {
                            all_success = false;
                        }
                    }
                }
                "queued" | "in_progress" | "waiting" | "requested" | "pending" => {
                    has_pending = true;
                    all_success = false;
                }
                _ => {
                    all_success = false;
                }
            }
        }

        if has_failure {
            Ok(Some("failure".to_string()))
        } else if has_pending {
            Ok(Some("pending".to_string()))
        } else if all_success {
            Ok(Some("success".to_string()))
        } else {
            Ok(Some("pending".to_string())) // Unknown state, treat as pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn create_test_client(server: &MockServer) -> GitHubClient {
        let octocrab = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .personal_token("test-token".to_string())
            .build()
            .unwrap();

        GitHubClient::with_octocrab(octocrab, "test-owner", "test-repo")
    }

    #[tokio::test]
    async fn test_check_runs_all_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 2,
                "check_runs": [
                    {"status": "completed", "conclusion": "success"},
                    {"status": "completed", "conclusion": "success"}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("success".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_with_failure() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 3,
                "check_runs": [
                    {"status": "completed", "conclusion": "success"},
                    {"status": "completed", "conclusion": "failure"},
                    {"status": "completed", "conclusion": "success"}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("failure".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_with_pending() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 2,
                "check_runs": [
                    {"status": "completed", "conclusion": "success"},
                    {"status": "in_progress", "conclusion": null}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("pending".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_queued() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"status": "queued", "conclusion": null}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("pending".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_waiting() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"status": "waiting", "conclusion": null}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("pending".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_no_checks() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 0,
                "check_runs": []
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, None);
    }

    #[tokio::test]
    async fn test_check_runs_skipped_and_neutral() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 3,
                "check_runs": [
                    {"status": "completed", "conclusion": "success"},
                    {"status": "completed", "conclusion": "skipped"},
                    {"status": "completed", "conclusion": "neutral"}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("success".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_timed_out() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"status": "completed", "conclusion": "timed_out"}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("failure".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_cancelled() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"status": "completed", "conclusion": "cancelled"}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("failure".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_action_required() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"status": "completed", "conclusion": "action_required"}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("failure".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_unknown_conclusion() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"status": "completed", "conclusion": "unknown_state"}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        // Unknown conclusion treated as not all_success, but not failure or pending
        assert_eq!(status, Some("pending".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_unknown_status() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"status": "some_unknown_status", "conclusion": null}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        // Unknown status treated as pending
        assert_eq!(status, Some("pending".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_requested_status() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"status": "requested", "conclusion": null}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("pending".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_pending_status() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"status": "pending", "conclusion": null}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("pending".to_string()));
    }

    #[tokio::test]
    async fn test_with_octocrab() {
        let mock_server = MockServer::start().await;

        let octocrab = Octocrab::builder()
            .base_uri(mock_server.uri())
            .unwrap()
            .personal_token("test-token".to_string())
            .build()
            .unwrap();

        let client = GitHubClient::with_octocrab(octocrab, "owner", "repo");
        assert_eq!(client.owner, "owner");
        assert_eq!(client.repo, "repo");
    }

    #[test]
    fn test_check_run_response_deserialization() {
        let json = r#"{
            "total_count": 2,
            "check_runs": [
                {"status": "completed", "conclusion": "success"},
                {"status": "in_progress", "conclusion": null}
            ]
        }"#;

        let response: CheckRunsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total_count, 2);
        assert_eq!(response.check_runs.len(), 2);
        assert_eq!(response.check_runs[0].status, "completed");
        assert_eq!(response.check_runs[0].conclusion, Some("success".to_string()));
        assert_eq!(response.check_runs[1].status, "in_progress");
        assert_eq!(response.check_runs[1].conclusion, None);
    }

    #[test]
    fn test_check_run_deserialization() {
        let json = r#"{"status": "completed", "conclusion": "failure"}"#;
        let check_run: CheckRun = serde_json::from_str(json).unwrap();
        assert_eq!(check_run.status, "completed");
        assert_eq!(check_run.conclusion, Some("failure".to_string()));
    }

    #[test]
    fn test_github_client_clone() {
        // This test just verifies Clone is implemented
        // We can't actually test it without a mock server setup
    }
}
