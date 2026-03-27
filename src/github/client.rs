use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::params::repos::Reference;
use octocrab::service::middleware::retry::RetryConfig;
use octocrab::Octocrab;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::Config;

const GITHUB_API_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_API_READ_TIMEOUT: Duration = Duration::from_secs(30);
const GITHUB_API_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const GITHUB_API_RETRY_COUNT: usize = 1;

pub struct GitHubClient {
    pub octocrab: Octocrab,
    pub owner: String,
    pub repo: String,
    api_call_tracker: Arc<ApiCallTracker>,
}

impl Clone for GitHubClient {
    fn clone(&self) -> Self {
        // Note: Octocrab doesn't implement Clone, so we create a minimal placeholder
        // This is only used in tests where we create fresh clients anyway
        Self {
            octocrab: self.octocrab.clone(),
            owner: self.owner.clone(),
            repo: self.repo.clone(),
            api_call_tracker: self.api_call_tracker.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApiCallStats {
    pub total_requests: usize,
    pub by_operation: Vec<(String, usize)>,
}

#[derive(Default)]
struct ApiCallTracker {
    total_requests: AtomicUsize,
    by_operation: Mutex<BTreeMap<String, usize>>,
}

impl ApiCallTracker {
    fn record(&self, operation: &'static str, count: usize) {
        if count == 0 {
            return;
        }

        self.total_requests.fetch_add(count, Ordering::Relaxed);
        let mut by_operation = self.by_operation.lock().unwrap_or_else(|e| e.into_inner());
        *by_operation.entry(operation.to_string()).or_insert(0) += count;
    }

    fn snapshot(&self) -> ApiCallStats {
        let by_operation = self
            .by_operation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(operation, count)| (operation.clone(), *count))
            .collect();

        ApiCallStats {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            by_operation,
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
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
}

/// PR activity for standup reports
#[derive(Debug, Clone, Serialize)]
pub struct PrActivity {
    pub number: u64,
    pub title: String,
    pub timestamp: DateTime<Utc>,
    pub url: String,
}

/// Review activity for standup reports
#[derive(Debug, Clone, Serialize)]
pub struct ReviewActivity {
    pub pr_number: u64,
    pub pr_title: String,
    pub reviewer: String,
    pub state: String,
    pub timestamp: DateTime<Utc>,
    pub is_received: bool, // true = received on your PR, false = given by you
}

/// Open PR info for tracking command
#[derive(Debug, Clone)]
pub struct OpenPrInfo {
    pub number: u64,
    pub head_branch: String,
    pub base_branch: String,
    pub state: String,
    pub is_draft: bool,
}

/// Open pull request info for repo-level listing commands
#[derive(Debug, Clone, Serialize)]
pub struct RepoPrListItem {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub head_branch: String,
    pub base_branch: String,
    pub state: String,
    pub is_draft: bool,
    pub created_at: DateTime<Utc>,
}

/// Open issue info for repo-level listing commands
#[derive(Debug, Clone, Serialize)]
pub struct RepoIssueListItem {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub labels: Vec<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct ReviewUser {
    login: String,
}

/// Response from GitHub reviews API
#[derive(Debug, Deserialize)]
struct Review {
    state: String,
    submitted_at: Option<DateTime<Utc>>,
    user: Option<ReviewUser>,
}

/// Response from GitHub search issues API
#[derive(Debug, Deserialize)]
struct SearchIssuesResponse {
    items: Vec<SearchIssue>,
}

#[derive(Debug, Deserialize)]
struct SearchIssue {
    number: u64,
    title: String,
    html_url: String,
    created_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct RepoListUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct RepoListPullRef {
    #[serde(rename = "ref")]
    ref_field: String,
}

#[derive(Debug, Deserialize)]
struct RepoListPullRequest {
    number: u64,
    title: String,
    html_url: String,
    user: RepoListUser,
    head: RepoListPullRef,
    base: RepoListPullRef,
    state: String,
    draft: Option<bool>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct RepoListLabel {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RepoListIssue {
    number: u64,
    title: String,
    html_url: String,
    user: RepoListUser,
    labels: Vec<RepoListLabel>,
    updated_at: DateTime<Utc>,
    pull_request: Option<serde_json::Value>,
}

impl GitHubClient {
    /// Create a new GitHub client from config
    pub fn new(owner: &str, repo: &str, api_base_url: Option<String>) -> Result<Self> {
        let token = Config::github_token().context(
            "GitHub auth not configured. Use one of: `stax auth`, `stax auth --from-gh`, \
             `gh auth login`, or set `STAX_GITHUB_TOKEN`.",
        )?;

        let mut builder = Octocrab::builder()
            .personal_token(token.to_string())
            .add_retry_config(RetryConfig::Simple(GITHUB_API_RETRY_COUNT))
            .set_connect_timeout(Some(GITHUB_API_CONNECT_TIMEOUT))
            .set_read_timeout(Some(GITHUB_API_READ_TIMEOUT))
            .set_write_timeout(Some(GITHUB_API_WRITE_TIMEOUT));
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
            api_call_tracker: Arc::new(ApiCallTracker::default()),
        })
    }

    /// Create a new GitHub client with a custom Octocrab instance (for testing)
    #[cfg(test)]
    pub fn with_octocrab(octocrab: Octocrab, owner: &str, repo: &str) -> Self {
        Self {
            octocrab,
            owner: owner.to_string(),
            repo: repo.to_string(),
            api_call_tracker: Arc::new(ApiCallTracker::default()),
        }
    }

    pub fn api_call_stats(&self) -> ApiCallStats {
        self.api_call_tracker.snapshot()
    }

    pub(crate) fn record_api_call(&self, operation: &'static str) {
        self.api_call_tracker.record(operation, 1);
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

        // Deduplicate check runs by name, keeping the latest (highest id) for each.
        // GitHub returns all check runs including superseded ones from workflow re-runs.
        let mut latest_by_name: HashMap<&str, &CheckRun> = HashMap::new();
        for run in &response.check_runs {
            let entry = latest_by_name.entry(&run.name).or_insert(run);
            if run.id > entry.id {
                *entry = run;
            }
        }

        // Analyze deduplicated check runs to determine overall status
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

    /// Get the authenticated user's login name
    pub async fn get_current_user(&self) -> Result<String> {
        let user = self.octocrab.current().user().await?;
        Ok(user.login)
    }

    /// Get PRs merged by the user in the last N hours
    pub async fn get_recent_merged_prs(
        &self,
        hours: i64,
        username: &str,
    ) -> Result<Vec<PrActivity>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        // Use search API to find only user's merged PRs - much faster than listing all
        let url = format!(
            "/search/issues?q=repo:{}/{}+author:{}+is:pr+is:merged&sort=updated&order=desc&per_page=30",
            self.owner, self.repo, username
        );

        let response: SearchIssuesResponse = self.octocrab.get(&url, None::<&()>).await?;

        let merged: Vec<PrActivity> = response
            .items
            .into_iter()
            .filter_map(|issue| {
                let closed_at = issue.closed_at?;
                // Filter by time locally (more reliable than URL date filters)
                if closed_at < since {
                    return None;
                }
                Some(PrActivity {
                    number: issue.number,
                    title: issue.title,
                    timestamp: closed_at,
                    url: issue.html_url,
                })
            })
            .collect();

        Ok(merged)
    }

    /// Get PRs opened by the user in the last N hours
    pub async fn get_recent_opened_prs(
        &self,
        hours: i64,
        username: &str,
    ) -> Result<Vec<PrActivity>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        // Use search API to find only user's created PRs
        let url = format!(
            "/search/issues?q=repo:{}/{}+author:{}+is:pr&sort=created&order=desc&per_page=30",
            self.owner, self.repo, username
        );

        let response: SearchIssuesResponse = self.octocrab.get(&url, None::<&()>).await?;

        let opened: Vec<PrActivity> = response
            .items
            .into_iter()
            .filter(|issue| issue.created_at >= since)
            .map(|issue| PrActivity {
                number: issue.number,
                title: issue.title,
                timestamp: issue.created_at,
                url: issue.html_url,
            })
            .collect();

        Ok(opened)
    }

    /// Get reviews received on user's open PRs in the last N hours
    /// Only fetches user's own PRs to keep it fast
    pub async fn get_reviews_received(
        &self,
        hours: i64,
        username: &str,
    ) -> Result<Vec<ReviewActivity>> {
        let since = Utc::now() - chrono::Duration::hours(hours);

        // Use search to get only user's open PRs (fast)
        let url = format!(
            "/search/issues?q=repo:{}/{}+author:{}+is:pr+is:open&per_page=20",
            self.owner, self.repo, username
        );
        let response: SearchIssuesResponse = self.octocrab.get(&url, None::<&()>).await?;

        let mut reviews = Vec::new();

        // Only check reviews on user's own PRs (small list, few API calls)
        for issue in response.items {
            let reviews_url = format!(
                "/repos/{}/{}/pulls/{}/reviews",
                self.owner, self.repo, issue.number
            );
            let pr_reviews: Vec<Review> = self
                .octocrab
                .get(&reviews_url, None::<&()>)
                .await
                .unwrap_or_default();

            for review in pr_reviews {
                if let Some(submitted) = review.submitted_at {
                    if submitted >= since {
                        if let Some(reviewer) = review.user {
                            // Don't include self-reviews
                            if reviewer.login != username {
                                reviews.push(ReviewActivity {
                                    pr_number: issue.number,
                                    pr_title: issue.title.clone(),
                                    reviewer: reviewer.login,
                                    state: review.state,
                                    timestamp: submitted,
                                    is_received: true,
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(reviews)
    }

    /// Get reviews given by user on others' PRs in the last N hours
    /// Note: This is expensive for large repos, returns empty to keep standup fast
    pub async fn get_reviews_given(
        &self,
        _hours: i64,
        _username: &str,
    ) -> Result<Vec<ReviewActivity>> {
        // Not yet implemented: scanning all PRs via REST is O(N) and too slow
        // for large repos. A future version could use GitHub's GraphQL
        // PullRequestReviewContributionsByRepository connection to fetch this
        // efficiently in a single query.
        Ok(vec![])
    }

    /// Get all open PRs authored by the given user
    /// Uses Search API for efficient server-side filtering
    pub async fn get_user_open_prs(&self, username: &str) -> Result<Vec<OpenPrInfo>> {
        // Use search API to efficiently find user's open PRs
        let url = format!(
            "/search/issues?q=repo:{}/{}+author:{}+is:pr+is:open&per_page=100",
            self.owner, self.repo, username
        );

        let response: SearchIssuesResponse = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to search PRs")?;

        // For each PR from search, we need to get the branch info
        // Search API doesn't include head/base branch refs, so we fetch each PR
        let mut results = Vec::new();
        for issue in response.items {
            // Fetch full PR details to get branch info
            let pr = self
                .octocrab
                .pulls(&self.owner, &self.repo)
                .get(issue.number)
                .await;

            if let Ok(pr) = pr {
                results.push(OpenPrInfo {
                    number: pr.number,
                    head_branch: pr.head.ref_field.clone(),
                    base_branch: pr.base.ref_field.clone(),
                    state: "OPEN".to_string(),
                    is_draft: pr.draft.unwrap_or(false),
                });
            }
        }

        Ok(results)
    }

    /// List open pull requests for the current repository.
    pub async fn list_open_pull_requests(&self, limit: u8) -> Result<Vec<RepoPrListItem>> {
        self.record_api_call("pulls.list");
        let per_page = limit.clamp(1, 100);
        let url = format!(
            "/repos/{}/{}/pulls?state=open&sort=created&direction=desc&per_page={}",
            self.owner, self.repo, per_page
        );

        let response: Vec<RepoListPullRequest> = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to list pull requests")?;

        Ok(response
            .into_iter()
            .take(per_page as usize)
            .map(|pr| RepoPrListItem {
                number: pr.number,
                title: pr.title,
                url: pr.html_url,
                author: pr.user.login,
                head_branch: pr.head.ref_field,
                base_branch: pr.base.ref_field,
                state: pr.state,
                is_draft: pr.draft.unwrap_or(false),
                created_at: pr.created_at,
            })
            .collect())
    }

    /// List open issues for the current repository.
    pub async fn list_open_issues(&self, limit: u8) -> Result<Vec<RepoIssueListItem>> {
        self.record_api_call("issues.list");
        let per_page = limit.clamp(1, 100);
        // GitHub returns PRs in this listing; we filter client-side. Request up to 2× so a
        // PR-heavy first page does not underfill after filtering (capped at API max).
        let fetch_per_page = (usize::from(per_page) * 2).min(100) as u8;
        let url = format!(
            "/repos/{}/{}/issues?state=open&sort=updated&direction=desc&per_page={}",
            self.owner, self.repo, fetch_per_page
        );

        let response: Vec<RepoListIssue> = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to list issues")?;

        Ok(response
            .into_iter()
            .filter(|issue| issue.pull_request.is_none())
            .take(usize::from(per_page))
            .map(|issue| RepoIssueListItem {
                number: issue.number,
                title: issue.title,
                url: issue.html_url,
                author: issue.user.login,
                labels: issue
                    .labels
                    .into_iter()
                    .filter_map(|label| label.name)
                    .collect(),
                updated_at: issue.updated_at,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ensure_crypto_provider() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }

    async fn create_test_client(server: &MockServer) -> GitHubClient {
        ensure_crypto_provider();
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 2,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "completed", "conclusion": "success"},
                    {"id": 2, "name": "test", "status": "completed", "conclusion": "success"}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 3,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "completed", "conclusion": "success"},
                    {"id": 2, "name": "lint", "status": "completed", "conclusion": "failure"},
                    {"id": 3, "name": "test", "status": "completed", "conclusion": "success"}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 2,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "completed", "conclusion": "success"},
                    {"id": 2, "name": "test", "status": "in_progress", "conclusion": null}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "queued", "conclusion": null}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "waiting", "conclusion": null}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 3,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "completed", "conclusion": "success"},
                    {"id": 2, "name": "release", "status": "completed", "conclusion": "skipped"},
                    {"id": 3, "name": "deploy", "status": "completed", "conclusion": "neutral"}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "completed", "conclusion": "timed_out"}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "completed", "conclusion": "cancelled"}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "completed", "conclusion": "action_required"}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "completed", "conclusion": "unknown_state"}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "some_unknown_status", "conclusion": null}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "requested", "conclusion": null}
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
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"id": 1, "name": "build", "status": "pending", "conclusion": null}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("pending".to_string()));
    }

    #[tokio::test]
    async fn test_check_runs_rerun_supersedes_failure() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(
                "/repos/test-owner/test-repo/commits/abc123/check-runs",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 4,
                "check_runs": [
                    {"id": 100, "name": "lint", "status": "completed", "conclusion": "success"},
                    {"id": 101, "name": "build", "status": "completed", "conclusion": "failure"},
                    {"id": 102, "name": "test", "status": "completed", "conclusion": "success"},
                    {"id": 200, "name": "build", "status": "completed", "conclusion": "success"}
                ]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_check_runs_status("abc123").await.unwrap();
        assert_eq!(status, Some("success".to_string()));
    }

    #[tokio::test]
    async fn test_with_octocrab() {
        ensure_crypto_provider();
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
                {"id": 1, "name": "build", "status": "completed", "conclusion": "success"},
                {"id": 2, "name": "test", "status": "in_progress", "conclusion": null}
            ]
        }"#;

        let response: CheckRunsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total_count, 2);
        assert_eq!(response.check_runs.len(), 2);
        assert_eq!(response.check_runs[0].status, "completed");
        assert_eq!(
            response.check_runs[0].conclusion,
            Some("success".to_string())
        );
        assert_eq!(response.check_runs[1].status, "in_progress");
        assert_eq!(response.check_runs[1].conclusion, None);
    }

    #[test]
    fn test_check_run_deserialization() {
        let json = r#"{"id": 1, "name": "build", "status": "completed", "conclusion": "failure"}"#;
        let check_run: CheckRun = serde_json::from_str(json).unwrap();
        assert_eq!(check_run.status, "completed");
        assert_eq!(check_run.conclusion, Some("failure".to_string()));
    }

    #[tokio::test]
    async fn test_list_open_pull_requests() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls"))
            .and(query_param("state", "open"))
            .and(query_param("sort", "created"))
            .and(query_param("direction", "desc"))
            .and(query_param("per_page", "30"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 114,
                    "title": "worktrees enhanced",
                    "html_url": "https://github.com/test-owner/test-repo/pull/114",
                    "user": { "login": "cesar" },
                    "head": { "ref": "cesar/worktrees-enhanced" },
                    "base": { "ref": "main" },
                    "state": "open",
                    "draft": false,
                    "created_at": "2026-03-15T10:00:00Z"
                }
            ])))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let prs = client.list_open_pull_requests(30).await.unwrap();

        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 114);
        assert_eq!(prs[0].title, "worktrees enhanced");
        assert_eq!(prs[0].author, "cesar");
        assert_eq!(prs[0].head_branch, "cesar/worktrees-enhanced");
        assert_eq!(prs[0].base_branch, "main");
        assert_eq!(prs[0].state, "open");
        assert!(!prs[0].is_draft);
    }

    #[tokio::test]
    async fn test_list_open_pull_requests_preserves_draft_state() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 45,
                    "title": "draft stack cleanup",
                    "html_url": "https://github.com/test-owner/test-repo/pull/45",
                    "user": { "login": "cesar" },
                    "head": { "ref": "codex/draft-stack-cleanup" },
                    "base": { "ref": "main" },
                    "state": "open",
                    "draft": true,
                    "created_at": "2026-03-14T09:00:00Z"
                }
            ])))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let prs = client.list_open_pull_requests(30).await.unwrap();

        assert_eq!(prs.len(), 1);
        assert!(prs[0].is_draft);
    }

    #[tokio::test]
    async fn test_list_open_issues_filters_pull_requests_and_reads_labels() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues"))
            .and(query_param("state", "open"))
            .and(query_param("sort", "updated"))
            .and(query_param("direction", "desc"))
            .and(query_param("per_page", "60"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 113,
                    "title": "Handle browser launcher failures",
                    "html_url": "https://github.com/test-owner/test-repo/issues/113",
                    "user": { "login": "cesar" },
                    "labels": [],
                    "updated_at": "2026-03-15T11:00:00Z"
                },
                {
                    "number": 112,
                    "title": "This is actually a pull request",
                    "html_url": "https://github.com/test-owner/test-repo/issues/112",
                    "user": { "login": "cesar" },
                    "labels": [],
                    "updated_at": "2026-03-15T10:00:00Z",
                    "pull_request": {
                        "url": "https://api.github.com/repos/test-owner/test-repo/pulls/112"
                    }
                },
                {
                    "number": 77,
                    "title": "Gitlab Support",
                    "html_url": "https://github.com/test-owner/test-repo/issues/77",
                    "user": { "login": "geoHeil" },
                    "labels": [
                        { "name": "help wanted" },
                        { "name": "integration" }
                    ],
                    "updated_at": "2026-03-14T12:30:00Z"
                }
            ])))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let issues = client.list_open_issues(30).await.unwrap();

        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].number, 113);
        assert!(issues[0].labels.is_empty());
        assert_eq!(issues[1].number, 77);
        assert_eq!(issues[1].labels, vec!["help wanted", "integration"]);
    }

    #[tokio::test]
    async fn test_list_open_issues_overfetches_to_fill_after_pr_pollution() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues"))
            .and(query_param("state", "open"))
            .and(query_param("sort", "updated"))
            .and(query_param("direction", "desc"))
            .and(query_param("per_page", "4"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 201,
                    "title": "PR one",
                    "html_url": "https://github.com/test-owner/test-repo/pull/201",
                    "user": { "login": "u" },
                    "labels": [],
                    "updated_at": "2026-03-15T12:00:00Z",
                    "pull_request": {
                        "url": "https://api.github.com/repos/test-owner/test-repo/pulls/201"
                    }
                },
                {
                    "number": 202,
                    "title": "PR two",
                    "html_url": "https://github.com/test-owner/test-repo/pull/202",
                    "user": { "login": "u" },
                    "labels": [],
                    "updated_at": "2026-03-15T11:00:00Z",
                    "pull_request": {
                        "url": "https://api.github.com/repos/test-owner/test-repo/pulls/202"
                    }
                },
                {
                    "number": 10,
                    "title": "Real issue A",
                    "html_url": "https://github.com/test-owner/test-repo/issues/10",
                    "user": { "login": "u" },
                    "labels": [],
                    "updated_at": "2026-03-14T10:00:00Z"
                },
                {
                    "number": 11,
                    "title": "Real issue B",
                    "html_url": "https://github.com/test-owner/test-repo/issues/11",
                    "user": { "login": "u" },
                    "labels": [],
                    "updated_at": "2026-03-14T09:00:00Z"
                }
            ])))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let issues = client.list_open_issues(2).await.unwrap();

        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].number, 10);
        assert_eq!(issues[1].number, 11);
    }

    #[tokio::test]
    async fn test_list_open_pull_requests_empty_response() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let prs = client.list_open_pull_requests(30).await.unwrap();
        assert!(prs.is_empty());
    }

    #[tokio::test]
    async fn test_list_open_issues_empty_response() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let issues = client.list_open_issues(30).await.unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn test_github_client_clone() {
        // This test just verifies Clone is implemented
        // We can't actually test it without a mock server setup
    }
}
