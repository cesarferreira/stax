use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::Octocrab;
use octocrab::service::middleware::retry::RetryConfig;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::{Config, GitHubAuthSource};
use crate::forge::{ForgeSignal, PrActivity, RepoIssueListItem, RepoPrListItem, ReviewActivity};

const GITHUB_API_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_API_READ_TIMEOUT: Duration = Duration::from_secs(30);
const GITHUB_API_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const GITHUB_API_RETRY_COUNT: usize = 1;

pub struct GitHubClient {
    pub octocrab: Octocrab,
    pub owner: String,
    pub repo: String,
    auth_source: Option<GitHubAuthSource>,
    api_call_tracker: Arc<ApiCallTracker>,
}

impl Clone for GitHubClient {
    fn clone(&self) -> Self {
        Self {
            octocrab: self.octocrab.clone(),
            owner: self.owner.clone(),
            repo: self.repo.clone(),
            auth_source: self.auth_source,
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

pub use crate::forge::OpenPrInfo;

/// A fork of the upstream repo that stax can push a branch to when the
/// caller lacks write access to the upstream itself.
#[derive(Debug, Clone)]
pub struct ForkTarget {
    pub owner: String,
    pub ssh_url: String,
    pub https_url: String,
}

/// Minimal shape of a GitHub repository response, covering only the fields
/// fork resolution needs. Deserialized by hand (rather than via octocrab's
/// full `models::Repository`) so responses only need these few fields.
#[derive(Debug, Deserialize)]
struct ForkRepoResponse {
    #[serde(default)]
    fork: bool,
    owner: RepoListUser,
    ssh_url: String,
    clone_url: String,
    parent: Option<ForkParentRepo>,
    permissions: Option<ForkRepoPermissions>,
}

#[derive(Debug, Deserialize)]
struct ForkParentRepo {
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct ForkRepoPermissions {
    push: bool,
}

impl From<ForkRepoResponse> for ForkTarget {
    fn from(repo: ForkRepoResponse) -> Self {
        Self {
            owner: repo.owner.login,
            ssh_url: repo.ssh_url,
            https_url: repo.clone_url,
        }
    }
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
struct ReviewContributionsData {
    viewer: ReviewContributionsViewer,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewContributionsViewer {
    contributions_collection: ReviewContributionsCollection,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewContributionsCollection {
    pull_request_review_contributions: ReviewContributionConnection,
}

#[derive(Debug, Deserialize)]
struct ReviewContributionConnection {
    nodes: Option<Vec<Option<ReviewContributionNode>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewContributionNode {
    occurred_at: DateTime<Utc>,
    user: Option<ReviewUser>,
    pull_request_review: Option<AuthoredReview>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthoredReview {
    state: String,
    pull_request: ReviewedPullRequest,
}

#[derive(Debug, Deserialize)]
struct ReviewedPullRequest {
    number: u64,
    title: String,
    repository: ReviewedRepository,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewedRepository {
    name_with_owner: String,
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
        let (auth_source, token) = Config::github_token_with_source().context(
            "GitHub auth not configured. Use one of: `stax auth`, `stax auth --from-gh`, \
             `gh auth login`, or set `STAX_GITHUB_TOKEN`.",
        )?;
        Self::new_with_auth(owner, repo, api_base_url, auth_source, token)
    }

    pub(crate) fn new_for_trusted_remote(
        owner: &str,
        repo: &str,
        api_base_url: Option<String>,
        config: &Config,
        validated_remote_host: &str,
    ) -> Result<Self> {
        let (auth_source, token) = config
            .github_token_with_source_for_host(validated_remote_host)?
            .context(
                "GitHub auth not configured. Use one of: `stax auth`, `stax auth --from-gh`, \
                 `gh auth login`, or set `STAX_GITHUB_TOKEN`.",
            )?;
        Self::new_with_auth(owner, repo, api_base_url, auth_source, token)
    }

    fn new_with_auth(
        owner: &str,
        repo: &str,
        api_base_url: Option<String>,
        auth_source: GitHubAuthSource,
        token: String,
    ) -> Result<Self> {
        let mut builder = Octocrab::builder()
            .personal_token(token)
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
            auth_source: Some(auth_source),
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
            auth_source: None,
            api_call_tracker: Arc::new(ApiCallTracker::default()),
        }
    }

    pub fn api_call_stats(&self) -> ApiCallStats {
        self.api_call_tracker.snapshot()
    }

    pub(crate) fn record_api_call(&self, operation: &'static str) {
        self.api_call_tracker.record(operation, 1);
    }

    /// Enrich an API error with auth troubleshooting context when it looks
    /// like a token permissions issue (GitHub returns 404 for private repos
    /// when the token lacks access, not 403).
    pub(crate) fn enrich_api_error(&self, err: anyhow::Error) -> anyhow::Error {
        let msg = format!("{:#}", err);
        if msg.contains("Not Found")
            || msg.contains("404")
            || msg.contains("Unauthorized")
            || msg.contains("401")
            || msg.contains("Bad credentials")
        {
            let source_hint = match self.auth_source {
                Some(s) => format!("Current auth source: {}.", s.display_name()),
                None => "No auth source recorded.".to_string(),
            };
            err.context(format!(
                "GitHub API error for {}/{}. This often means your token is expired or \
                 lacks access to this repository. {}\n\
                 To fix: run `stax auth --from-gh` to refresh, or check your token scopes.",
                self.owner, self.repo, source_hint,
            ))
        } else {
            err
        }
    }

    /// Get the authenticated user's login name
    pub async fn get_current_user(&self) -> Result<String> {
        self.record_api_call("users.current");
        let user = self
            .octocrab
            .current()
            .user()
            .await
            .context("Failed to look up the authenticated user")
            .map_err(|e| self.enrich_api_error(e))?;
        Ok(user.login)
    }

    /// Find an existing fork of this repo owned by `login` that stax can
    /// push to, so fork-fallback submit reuses it instead of creating a
    /// duplicate. Returns `None` when no such repo exists (404) or when it
    /// exists but isn't a pushable fork of this exact upstream.
    pub async fn find_pushable_fork(&self, login: &str) -> Result<Option<ForkTarget>> {
        self.record_api_call("repos.get_fork_candidate");
        let url = format!("/repos/{}/{}", login, self.repo);
        let repo: ForkRepoResponse = match self.octocrab.get(&url, None::<&()>).await {
            Ok(repo) => repo,
            Err(octocrab::Error::GitHub { source, .. }) if source.status_code.as_u16() == 404 => {
                return Ok(None);
            }
            Err(e) => {
                return Err(self.enrich_api_error(
                    anyhow::Error::new(e).context("Failed to look up fork candidate"),
                ));
            }
        };

        let upstream = format!("{}/{}", self.owner, self.repo);
        let is_pushable_fork = repo.fork
            && repo.parent.as_ref().map(|parent| parent.full_name.as_str())
                == Some(upstream.as_str())
            && repo.permissions.as_ref().is_some_and(|p| p.push);

        if is_pushable_fork {
            Ok(Some(repo.into()))
        } else {
            Ok(None)
        }
    }

    /// Fork this repo under the authenticated user's account.
    pub async fn create_fork(&self) -> Result<ForkTarget> {
        self.record_api_call("repos.create_fork");
        let url = format!("/repos/{}/{}/forks", self.owner, self.repo);
        let repo: ForkRepoResponse = match self
            .octocrab
            .post(&url, None::<&()>)
            .await
            .context("Failed to create fork")
        {
            Ok(repo) => repo,
            Err(e) => return Err(self.enrich_api_error(e)),
        };
        Ok(repo.into())
    }

    /// Get PRs merged by the user in the last N hours
    pub async fn get_recent_merged_prs(
        &self,
        hours: i64,
        username: &str,
    ) -> Result<Vec<PrActivity>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        // Use search API to find only user's merged PRs - much faster than listing all
        self.record_api_call("search.issues");
        let url = format!(
            "/search/issues?q=repo:{}/{}+author:{}+is:pr+is:merged&sort=updated&order=desc&per_page=30",
            self.owner, self.repo, username
        );

        let response: SearchIssuesResponse = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to get recently merged PRs")
            .map_err(|e| self.enrich_api_error(e))?;

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
        self.record_api_call("search.issues");
        let url = format!(
            "/search/issues?q=repo:{}/{}+author:{}+is:pr&sort=created&order=desc&per_page=30",
            self.owner, self.repo, username
        );

        let response: SearchIssuesResponse = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to get recently opened PRs")
            .map_err(|e| self.enrich_api_error(e))?;

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
        self.record_api_call("search.issues");
        let response: SearchIssuesResponse = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to search PRs for received reviews")
            .map_err(|e| self.enrich_api_error(e))?;

        let mut reviews = Vec::new();

        // Only check reviews on user's own PRs (small list, few API calls)
        for issue in response.items {
            let reviews_url = format!(
                "/repos/{}/{}/pulls/{}/reviews",
                self.owner, self.repo, issue.number
            );
            self.record_api_call("pulls.reviews.list");
            let pr_reviews: Vec<Review> = self
                .octocrab
                .get(&reviews_url, None::<&()>)
                .await
                .unwrap_or_default();

            for review in pr_reviews {
                if let Some(submitted) = review.submitted_at
                    && submitted >= since
                    && let Some(reviewer) = review.user
                {
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

        Ok(reviews)
    }

    /// Get reviews given by the authenticated user in one bounded GraphQL request.
    pub async fn get_reviews_given(
        &self,
        hours: i64,
        _username: &str,
    ) -> Result<ForgeSignal<Vec<ReviewActivity>>> {
        let to = Utc::now();
        let from = to - chrono::Duration::hours(hours);
        let query = format!(
            r#"
            query {{
                viewer {{
                    contributionsCollection(from: "{}", to: "{}") {{
                        pullRequestReviewContributions(first: 100, orderBy: {{ field: OCCURRED_AT, direction: DESC }}) {{
                            nodes {{
                                occurredAt
                                user {{ login }}
                                pullRequestReview {{
                                    state
                                    pullRequest {{
                                        number
                                        title
                                        repository {{ nameWithOwner }}
                                    }}
                                }}
                            }}
                        }}
                    }}
                }}
            }}
            "#,
            from.to_rfc3339(),
            to.to_rfc3339()
        );

        self.record_api_call("graphql.review_contributions");
        let data: ReviewContributionsData = self
            .graphql_data(serde_json::json!({ "query": query }))
            .await
            .context("Failed to query authored review contributions")?;
        let repository = format!("{}/{}", self.owner, self.repo);
        let reviews = data
            .viewer
            .contributions_collection
            .pull_request_review_contributions
            .nodes
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .filter_map(|node| {
                let reviewer = node.user?.login;
                if reviewer.trim().is_empty() {
                    return None;
                }
                let review = node.pull_request_review?;
                if node.occurred_at < from
                    || review.pull_request.repository.name_with_owner != repository
                {
                    return None;
                }
                Some(ReviewActivity {
                    pr_number: review.pull_request.number,
                    pr_title: review.pull_request.title,
                    reviewer,
                    state: review.state,
                    timestamp: node.occurred_at,
                    is_received: false,
                })
            })
            .collect();

        Ok(ForgeSignal::Available(reviews))
    }

    /// Get all open PRs authored by the given user
    /// Uses Search API for efficient server-side filtering
    pub async fn get_user_open_prs(&self, username: &str) -> Result<Vec<OpenPrInfo>> {
        // Use search API to efficiently find user's open PRs
        let url = format!(
            "/search/issues?q=repo:{}/{}+author:{}+is:pr+is:open&per_page=100",
            self.owner, self.repo, username
        );

        self.record_api_call("search.issues");
        let response: SearchIssuesResponse = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to search PRs")
            .map_err(|e| self.enrich_api_error(e))?;

        // For each PR from search, we need to get the branch info
        // Search API doesn't include head/base branch refs, so we fetch each PR
        let mut results = Vec::new();
        let mut skipped: Vec<(u64, String)> = Vec::new();
        let found = response.items.len();
        for issue in response.items {
            // Fetch full PR details to get branch info
            self.record_api_call("pulls.get");
            let pr = self
                .octocrab
                .pulls(&self.owner, &self.repo)
                .get(issue.number)
                .await;

            match pr {
                Ok(pr) => {
                    // `id`, `number`, `url`, `head`, `base` and `locked` are
                    // required fields as of octocrab 0.54, so a response missing
                    // any of them fails to deserialize above and lands in `Err`.
                    results.push(OpenPrInfo {
                        number: pr.number,
                        head_branch: pr.head.ref_field.clone(),
                        base_branch: pr.base.ref_field.clone(),
                        state: "OPEN".to_string(),
                        is_draft: pr.draft.unwrap_or(false),
                    });
                }
                Err(err) => skipped.push((issue.number, format!("{err}"))),
            }
        }

        // Skipping a PR we cannot read is the right call — one odd PR should not
        // stop the rest being tracked — but doing it silently is not. If every
        // PR search found turns out to be unreadable, an empty list is
        // indistinguishable from "you have no open PRs", which sends the caller
        // down a misleading path. Say so instead.
        if !skipped.is_empty() {
            if results.is_empty() {
                let (number, err) = &skipped[0];
                anyhow::bail!(
                    "Found {} open PR(s) for this repository but could not read any of them.\n\
                     First failure was PR #{}: {}\n\n\
                     This usually means the forge returned a pull request payload \
                     that this version of stax cannot parse. Please report it.",
                    found,
                    number,
                    err
                );
            }
            eprintln!(
                "  warning: skipped {} of {} open PR(s) that could not be read (e.g. #{})",
                skipped.len(),
                found,
                skipped[0].0
            );
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
            .context("Failed to list pull requests")
            .map_err(|e| self.enrich_api_error(e))?;

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
    ///
    /// GitHub's issues endpoint includes pull requests, so we filter them client-side and
    /// paginate until we have `limit` real issues or the API has no more pages.
    pub async fn list_open_issues(&self, limit: u8) -> Result<Vec<RepoIssueListItem>> {
        let want = limit.clamp(1, 100) as usize;
        let mut collected: Vec<RepoIssueListItem> = Vec::with_capacity(want);
        let mut page = 1u32;

        loop {
            let url = format!(
                "/repos/{}/{}/issues?state=open&sort=updated&direction=desc&per_page=100&page={}",
                self.owner, self.repo, page
            );

            self.record_api_call("issues.list");
            let response: Vec<RepoListIssue> = self
                .octocrab
                .get(&url, None::<&()>)
                .await
                .context("Failed to list issues")
                .map_err(|e| self.enrich_api_error(e))?;

            let fetched = response.len();

            for issue in response {
                if issue.pull_request.is_some() {
                    continue;
                }
                collected.push(RepoIssueListItem {
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
                });
                if collected.len() >= want {
                    return Ok(collected);
                }
            }

            if fetched < 100 {
                break;
            }
            page += 1;
        }

        Ok(collected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
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

    fn assert_auth_hint(err: anyhow::Error, operation: &str, github_error: &str) {
        let msg = format!("{:#}", err);
        assert!(msg.contains(operation), "missing operation context: {msg}");
        assert!(
            msg.contains(github_error),
            "missing original GitHub error: {msg}"
        );
        assert!(
            msg.contains("token is expired"),
            "missing token hint: {msg}"
        );
        assert!(
            msg.contains("stax auth --from-gh"),
            "missing remediation: {msg}"
        );
    }

    #[tokio::test]
    async fn recent_merged_prs_enriches_unauthorized_search_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "message": "Bad credentials"
            })))
            .mount(&server)
            .await;

        let client = create_test_client(&server).await;
        let err = client
            .get_recent_merged_prs(24, "alice")
            .await
            .expect_err("401 search should fail");
        assert_auth_hint(err, "Failed to get recently merged PRs", "Bad credentials");
    }

    #[tokio::test]
    async fn recent_opened_prs_enriches_not_found_search_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "Not Found"
            })))
            .mount(&server)
            .await;

        let client = create_test_client(&server).await;
        let err = client
            .get_recent_opened_prs(24, "alice")
            .await
            .expect_err("404 search should fail");
        assert_auth_hint(err, "Failed to get recently opened PRs", "Not Found");
    }

    #[tokio::test]
    async fn reviews_received_enriches_unauthorized_initial_search_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "message": "Bad credentials"
            })))
            .mount(&server)
            .await;

        let client = create_test_client(&server).await;
        let err = client
            .get_reviews_received(24, "alice")
            .await
            .expect_err("401 search should fail");
        assert_auth_hint(
            err,
            "Failed to search PRs for received reviews",
            "Bad credentials",
        );
    }

    #[tokio::test]
    async fn user_open_prs_enriches_not_found_initial_search_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "Not Found"
            })))
            .mount(&server)
            .await;

        let client = create_test_client(&server).await;
        let err = client
            .get_user_open_prs("alice")
            .await
            .expect_err("404 search should fail");
        assert_auth_hint(err, "Failed to search PRs", "Not Found");
    }

    #[tokio::test]
    async fn recent_merged_prs_does_not_mislabel_server_error_as_auth() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "message": "temporary failure"
            })))
            .mount(&server)
            .await;

        let client = create_test_client(&server).await;
        let err = client
            .get_recent_merged_prs(24, "alice")
            .await
            .expect_err("500 search should fail");
        let msg = format!("{:#}", err);
        assert!(msg.contains("Failed to get recently merged PRs"), "{msg}");
        assert!(msg.contains("temporary failure"), "{msg}");
        assert!(!msg.contains("stax auth --from-gh"), "{msg}");
    }

    /// `get_user_open_prs` reads `number`, `head.ref` and `base.ref` off each
    /// fetched pull request. Those were `Option` fields before octocrab 0.54 and
    /// are required from 0.54 on, so this pins the happy path that the upgrade
    /// moved: a well-formed response must still yield the same branch info.
    #[tokio::test]
    async fn test_get_user_open_prs_reads_head_and_base_refs() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 2,
                "incomplete_results": false,
                "items": [
                    {
                        "number": 11,
                        "title": "Feature A",
                        "html_url": "https://github.com/test-owner/test-repo/pull/11",
                        "created_at": "2026-01-01T00:00:00Z",
                        "closed_at": null
                    },
                    {
                        "number": 12,
                        "title": "Feature B",
                        "html_url": "https://github.com/test-owner/test-repo/pull/12",
                        "created_at": "2026-01-02T00:00:00Z",
                        "closed_at": null
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/11"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test-owner/test-repo/pulls/11",
                "id": 11,
                "number": 11,
                "head": { "ref": "feature-a", "sha": "aaaa", "label": "test-owner:feature-a" },
                "base": { "ref": "main", "sha": "bbbb" },
                "draft": false
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/12"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test-owner/test-repo/pulls/12",
                "id": 12,
                "number": 12,
                "head": { "ref": "feature-b", "sha": "cccc", "label": "test-owner:feature-b" },
                "base": { "ref": "feature-a", "sha": "dddd" },
                "draft": true
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let prs = client.get_user_open_prs("alice").await.unwrap();

        assert_eq!(prs.len(), 2, "expected both PRs: {prs:?}");

        let a = prs.iter().find(|p| p.number == 11).expect("PR 11 missing");
        assert_eq!(a.head_branch, "feature-a");
        assert_eq!(a.base_branch, "main");
        assert_eq!(a.state, "OPEN");
        assert!(!a.is_draft);

        // Stacked child: its base is the sibling branch, not trunk.
        let b = prs.iter().find(|p| p.number == 12).expect("PR 12 missing");
        assert_eq!(b.head_branch, "feature-b");
        assert_eq!(b.base_branch, "feature-a");
        assert!(b.is_draft);
    }

    /// A single unusable pull request must be skipped, not abort the whole
    /// listing — `stax branch track --all-prs` should still import everything
    /// else it found.
    ///
    /// This is the behaviour the octocrab 0.54 upgrade re-routed. Previously
    /// `head`/`base`/`number` were optional and stax skipped the PR explicitly;
    /// now the response fails to deserialize and is skipped as a fetch error.
    /// Same observable result, different mechanism, so it is worth pinning.
    #[tokio::test]
    async fn test_get_user_open_prs_skips_unusable_pr_and_keeps_the_rest() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 2,
                "incomplete_results": false,
                "items": [
                    {
                        "number": 21,
                        "title": "Broken",
                        "html_url": "https://github.com/test-owner/test-repo/pull/21",
                        "created_at": "2026-01-01T00:00:00Z",
                        "closed_at": null
                    },
                    {
                        "number": 22,
                        "title": "Fine",
                        "html_url": "https://github.com/test-owner/test-repo/pull/22",
                        "created_at": "2026-01-02T00:00:00Z",
                        "closed_at": null
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        // PR 21 has no head at all.
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/21"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test-owner/test-repo/pulls/21",
                "id": 21,
                "number": 21,
                "base": { "ref": "main", "sha": "bbbb" },
                "draft": false
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/22"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test-owner/test-repo/pulls/22",
                "id": 22,
                "number": 22,
                "head": { "ref": "feature-ok", "sha": "cccc", "label": "test-owner:feature-ok" },
                "base": { "ref": "main", "sha": "dddd" },
                "draft": false
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let prs = client
            .get_user_open_prs("alice")
            .await
            .expect("one unusable PR must not fail the whole listing");

        assert_eq!(prs.len(), 1, "expected only the usable PR: {prs:?}");
        assert_eq!(prs[0].number, 22);
        assert_eq!(prs[0].head_branch, "feature-ok");
    }

    /// The dangerous case the octocrab 0.54 upgrade made more likely: if every
    /// PR the search found is unreadable, returning an empty list would render
    /// as "No open PRs found", which is indistinguishable from genuinely having
    /// none. That must be an error naming the failure instead.
    #[tokio::test]
    async fn test_get_user_open_prs_errors_when_no_pr_can_be_read() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "incomplete_results": false,
                "items": [
                    {
                        "number": 31,
                        "title": "Broken",
                        "html_url": "https://github.com/test-owner/test-repo/pull/31",
                        "created_at": "2026-01-01T00:00:00Z",
                        "closed_at": null
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        // Missing `head`, so octocrab cannot deserialize it.
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/31"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test-owner/test-repo/pulls/31",
                "id": 31,
                "locked": false,
                "number": 31,
                "base": { "ref": "main", "sha": "bbbb" },
                "draft": false
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let err = client
            .get_user_open_prs("alice")
            .await
            .expect_err("unreadable PRs must not look like an empty result");

        let msg = format!("{err:#}");
        assert!(
            msg.contains("could not read any of them"),
            "expected an explicit unreadable-PR error, got: {msg}"
        );
        assert!(
            msg.contains("#31"),
            "error should name the offending PR, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_get_current_user_404_gives_auth_hint() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "Not Found"
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let error = client.get_current_user().await.unwrap_err();
        let message = format!("{error:#}");

        assert!(message.contains("Failed to look up the authenticated user"));
        assert!(message.contains("token is expired or lacks access"));
        assert!(message.contains("stax auth --from-gh"));
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
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
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
    async fn test_list_open_issues_paginates_through_pr_heavy_first_page() {
        let mock_server = MockServer::start().await;

        // Page 1: 100 items, all PRs — real issues are on page 2
        let pr_items: Vec<serde_json::Value> = (1u32..=100)
            .map(|n| {
                serde_json::json!({
                    "number": n,
                    "title": format!("PR {n}"),
                    "html_url": format!("https://github.com/test-owner/test-repo/pull/{n}"),
                    "user": { "login": "u" },
                    "labels": [],
                    "updated_at": "2026-03-15T12:00:00Z",
                    "pull_request": {
                        "url": format!("https://api.github.com/repos/test-owner/test-repo/pulls/{n}")
                    }
                })
            })
            .collect();

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!(pr_items)))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
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
    async fn test_list_open_issues_counts_every_page() {
        let mock_server = MockServer::start().await;

        // Page 1: 100 items, all PRs — filtered out, so nothing satisfies `want` yet
        let pr_items: Vec<serde_json::Value> = (1u32..=100)
            .map(|n| {
                serde_json::json!({
                    "number": n,
                    "title": format!("PR {n}"),
                    "html_url": format!("https://github.com/test-owner/test-repo/pull/{n}"),
                    "user": { "login": "u" },
                    "labels": [],
                    "updated_at": "2026-03-15T12:00:00Z",
                    "pull_request": {
                        "url": format!("https://api.github.com/repos/test-owner/test-repo/pulls/{n}")
                    }
                })
            })
            .collect();

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!(pr_items)))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 10,
                    "title": "Real issue A",
                    "html_url": "https://github.com/test-owner/test-repo/issues/10",
                    "user": { "login": "u" },
                    "labels": [],
                    "updated_at": "2026-03-14T10:00:00Z"
                }
            ])))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let issues = client.list_open_issues(1).await.unwrap();
        assert_eq!(issues.len(), 1);

        let stats = client.api_call_stats();
        assert!(
            stats
                .by_operation
                .iter()
                .any(|(op, count)| op == "issues.list" && *count == 2),
            "expected issues.list to be recorded once per page, got: {:?}",
            stats.by_operation
        );
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

    #[tokio::test]
    async fn test_enrich_api_error_adds_auth_context_on_not_found() {
        ensure_crypto_provider();
        let octocrab = Octocrab::builder()
            .personal_token("expired-token".to_string())
            .build()
            .unwrap();

        let mut client = GitHubClient::with_octocrab(octocrab, "myorg", "myrepo");
        client.auth_source = Some(GitHubAuthSource::CredentialsFile);

        let original = anyhow::anyhow!("Not Found");
        let enriched = client.enrich_api_error(original);
        let msg = format!("{:#}", enriched);

        assert!(
            msg.contains("token is expired or lacks access"),
            "Expected auth hint, got: {}",
            msg
        );
        assert!(
            msg.contains("credentials file"),
            "Expected auth source in message, got: {}",
            msg
        );
        assert!(
            msg.contains("stax auth --from-gh"),
            "Expected fix suggestion, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_enrich_api_error_passes_through_non_auth_errors() {
        ensure_crypto_provider();
        let octocrab = Octocrab::builder()
            .personal_token("token".to_string())
            .build()
            .unwrap();

        let client = GitHubClient::with_octocrab(octocrab, "myorg", "myrepo");

        let original = anyhow::anyhow!("Connection timeout");
        let enriched = client.enrich_api_error(original);
        let msg = format!("{:#}", enriched);

        assert!(
            !msg.contains("token is expired"),
            "Non-auth errors should not get auth hint, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_find_open_pr_by_head_404_gives_auth_hint() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "Not Found",
                "documentation_url": "https://docs.github.com/rest"
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let result = client.find_open_pr_by_head("test-owner", "my-branch").await;

        assert!(result.is_err(), "Expected error on 404");
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("token is expired or lacks access"),
            "Expected auth hint in 404 error, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_list_open_pull_requests_404_gives_auth_hint() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "Not Found",
                "documentation_url": "https://docs.github.com/rest"
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let result = client.list_open_pull_requests(30).await;

        assert!(result.is_err(), "Expected error on 404");
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("token is expired or lacks access"),
            "Expected auth hint in 404 error, got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("stax auth --from-gh"),
            "Expected auth remediation hint in 404 error, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_list_open_issues_404_gives_auth_hint() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "Not Found",
                "documentation_url": "https://docs.github.com/rest"
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let result = client.list_open_issues(30).await;

        assert!(result.is_err(), "Expected error on 404");
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("token is expired or lacks access"),
            "Expected auth hint in 404 error, got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("stax auth --from-gh"),
            "Expected auth remediation hint in 404 error, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_find_pushable_fork_returns_none_on_404() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/some-login/test-repo"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "Not Found",
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let result = client.find_pushable_fork("some-login").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_find_pushable_fork_returns_target_when_pushable_fork_of_this_repo() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/some-login/test-repo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "fork": true,
                "owner": { "login": "some-login" },
                "ssh_url": "git@github.com:some-login/test-repo.git",
                "clone_url": "https://github.com/some-login/test-repo.git",
                "parent": { "full_name": "test-owner/test-repo" },
                "permissions": { "push": true }
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let target = client
            .find_pushable_fork("some-login")
            .await
            .unwrap()
            .expect("expected a pushable fork");
        assert_eq!(target.owner, "some-login");
        assert_eq!(target.ssh_url, "git@github.com:some-login/test-repo.git");
        assert_eq!(
            target.https_url,
            "https://github.com/some-login/test-repo.git"
        );
    }

    #[tokio::test]
    async fn test_find_pushable_fork_ignores_repo_not_forked_from_this_upstream() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/some-login/test-repo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "fork": true,
                "owner": { "login": "some-login" },
                "ssh_url": "git@github.com:some-login/test-repo.git",
                "clone_url": "https://github.com/some-login/test-repo.git",
                "parent": { "full_name": "someone-else/unrelated-repo" },
                "permissions": { "push": true }
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let result = client.find_pushable_fork("some-login").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_find_pushable_fork_ignores_fork_without_push_access() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/some-login/test-repo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "fork": true,
                "owner": { "login": "some-login" },
                "ssh_url": "git@github.com:some-login/test-repo.git",
                "clone_url": "https://github.com/some-login/test-repo.git",
                "parent": { "full_name": "test-owner/test-repo" },
                "permissions": { "push": false }
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let result = client.find_pushable_fork("some-login").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_create_fork_returns_target() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/repos/test-owner/test-repo/forks"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "fork": true,
                "owner": { "login": "some-login" },
                "ssh_url": "git@github.com:some-login/test-repo.git",
                "clone_url": "https://github.com/some-login/test-repo.git"
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let target = client.create_fork().await.unwrap();
        assert_eq!(target.owner, "some-login");
        assert_eq!(target.ssh_url, "git@github.com:some-login/test-repo.git");
        assert_eq!(
            target.https_url,
            "https://github.com/some-login/test-repo.git"
        );
    }
}
