use super::client::GitHubClient;
use crate::forge::IssueComment;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;

const PER_PAGE: usize = 100;
const MAX_FILE_PAGES: u32 = 5;
const MAX_PAGES: u32 = 5;

#[derive(Debug, Clone)]
pub struct BoardPrSummary {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub head_branch: String,
    pub base_branch: String,
    pub is_draft: bool,
    pub labels: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct BoardIssueSummary {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub labels: Vec<String>,
    pub comment_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct BoardPrFile {
    pub filename: String,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Clone)]
pub struct BoardPrDetail {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub head_branch: String,
    pub base_branch: String,
    pub head_sha: String,
    pub is_draft: bool,
    pub mergeable: Option<bool>,
    pub mergeable_state: Option<String>,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    pub comment_count: u32,
    pub review_comment_count: u32,
    pub labels: Vec<String>,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct BoardIssueDetail {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub labels: Vec<String>,
    pub comment_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct BoardUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct BoardRef {
    #[serde(rename = "ref")]
    ref_field: String,
}

#[derive(Debug, Deserialize)]
struct BoardLabel {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BoardPrListItem {
    number: u64,
    title: String,
    html_url: String,
    user: BoardUser,
    head: BoardRef,
    base: BoardRef,
    draft: Option<bool>,
    #[serde(default)]
    labels: Vec<BoardLabel>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct BoardIssueListItem {
    number: u64,
    title: String,
    html_url: String,
    user: BoardUser,
    #[serde(default)]
    labels: Vec<BoardLabel>,
    #[serde(default)]
    comments: u32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct BoardPrDetailHead {
    #[serde(rename = "ref")]
    ref_field: String,
    sha: String,
}

#[derive(Debug, Deserialize)]
struct BoardPrDetailResponse {
    number: u64,
    title: String,
    body: Option<String>,
    head: BoardPrDetailHead,
    base: BoardRef,
    draft: Option<bool>,
    mergeable: Option<bool>,
    mergeable_state: Option<String>,
    additions: Option<u64>,
    deletions: Option<u64>,
    changed_files: Option<u64>,
    comments: Option<u32>,
    review_comments: Option<u32>,
    #[serde(default)]
    labels: Vec<BoardLabel>,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct BoardPrFileResponse {
    filename: String,
    status: String,
    additions: u64,
    deletions: u64,
}

#[derive(Debug, Deserialize)]
struct BoardIssueDetailResponse {
    number: u64,
    title: String,
    body: Option<String>,
    user: BoardUser,
    #[serde(default)]
    labels: Vec<BoardLabel>,
    comments: Option<u32>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct BoardIssueCommentResponse {
    id: u64,
    body: Option<String>,
    user: BoardUser,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct BoardRepoLabel {
    name: String,
}

fn label_names(labels: Vec<BoardLabel>) -> Vec<String> {
    labels.into_iter().filter_map(|label| label.name).collect()
}

impl GitHubClient {
    /// List the most recently updated open pull requests.
    pub(crate) async fn board_list_open_prs(&self, limit: u8) -> Result<Vec<BoardPrSummary>> {
        self.record_api_call("board.pulls.list");
        let per_page = limit.clamp(1, 100);
        let url = format!(
            "/repos/{}/{}/pulls?state=open&sort=updated&direction=desc&per_page={}",
            self.owner, self.repo, per_page
        );

        let response: Vec<BoardPrListItem> = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to list pull requests")
            .map_err(|e| self.enrich_api_error(e))?;

        Ok(response
            .into_iter()
            .take(per_page as usize)
            .map(|pr| BoardPrSummary {
                number: pr.number,
                title: pr.title,
                author: pr.user.login,
                head_branch: pr.head.ref_field,
                base_branch: pr.base.ref_field,
                is_draft: pr.draft.unwrap_or(false),
                labels: label_names(pr.labels),
                created_at: pr.created_at,
                updated_at: pr.updated_at,
                url: pr.html_url,
            })
            .collect())
    }

    /// List open issues (pull requests excluded), paginating until `limit`
    /// real issues are collected, the API runs out of pages, or `MAX_PAGES`
    /// is hit (the `/issues` endpoint mixes in PRs, which don't count
    /// toward `limit`, so a repo with many open PRs could otherwise walk
    /// hundreds of pages).
    pub(crate) async fn board_list_open_issues(&self, limit: u8) -> Result<Vec<BoardIssueSummary>> {
        let want = limit.clamp(1, 100) as usize;
        let mut collected: Vec<BoardIssueSummary> = Vec::with_capacity(want);
        let mut page = 1u32;

        loop {
            let url = format!(
                "/repos/{}/{}/issues?state=open&sort=updated&direction=desc&per_page={PER_PAGE}&page={page}",
                self.owner, self.repo
            );

            self.record_api_call("board.issues.list");
            let response: Vec<BoardIssueListItem> = self
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
                collected.push(BoardIssueSummary {
                    number: issue.number,
                    title: issue.title,
                    author: issue.user.login,
                    labels: label_names(issue.labels),
                    comment_count: issue.comments,
                    created_at: issue.created_at,
                    updated_at: issue.updated_at,
                    url: issue.html_url,
                });
                if collected.len() >= want {
                    return Ok(collected);
                }
            }

            if fetched < PER_PAGE || page >= MAX_PAGES {
                break;
            }
            page += 1;
        }

        Ok(collected)
    }

    pub(crate) async fn board_get_pr_detail(&self, number: u64) -> Result<BoardPrDetail> {
        self.record_api_call("board.pulls.get");
        let url = format!("/repos/{}/{}/pulls/{}", self.owner, self.repo, number);
        let pr: BoardPrDetailResponse = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to get pull request")
            .map_err(|e| self.enrich_api_error(e))?;

        Ok(BoardPrDetail {
            number: pr.number,
            title: pr.title,
            body: pr.body.unwrap_or_default(),
            head_branch: pr.head.ref_field,
            base_branch: pr.base.ref_field,
            head_sha: pr.head.sha,
            is_draft: pr.draft.unwrap_or(false),
            mergeable: pr.mergeable,
            mergeable_state: pr.mergeable_state,
            additions: pr.additions.unwrap_or_default(),
            deletions: pr.deletions.unwrap_or_default(),
            changed_files: pr.changed_files.unwrap_or_default(),
            comment_count: pr.comments.unwrap_or_default(),
            review_comment_count: pr.review_comments.unwrap_or_default(),
            labels: label_names(pr.labels),
            url: pr.html_url,
        })
    }

    pub(crate) async fn board_list_pr_files(&self, number: u64) -> Result<Vec<BoardPrFile>> {
        let base_url = format!("/repos/{}/{}/pulls/{}/files", self.owner, self.repo, number);
        let mut files = Vec::new();
        let mut page = 1u32;

        loop {
            self.record_api_call("board.pulls.files");
            let url = format!("{base_url}?per_page={PER_PAGE}&page={page}");
            let response: Vec<BoardPrFileResponse> = self
                .octocrab
                .get(&url, None::<&()>)
                .await
                .context("Failed to list pull request files")
                .map_err(|e| self.enrich_api_error(e))?;
            let fetched = response.len();
            files.extend(response.into_iter().map(|file| BoardPrFile {
                filename: file.filename,
                status: file.status,
                additions: file.additions,
                deletions: file.deletions,
            }));

            if fetched < PER_PAGE || page >= MAX_FILE_PAGES {
                break;
            }
            page += 1;
        }

        Ok(files)
    }

    pub(crate) async fn board_get_pr_diff(&self, number: u64) -> Result<String> {
        self.record_api_call("board.pulls.get_diff");
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .get_diff(number)
            .await
            .context("Failed to get pull request diff")
            .map_err(|e| self.enrich_api_error(e))
    }

    pub(crate) async fn board_get_issue_detail(&self, number: u64) -> Result<BoardIssueDetail> {
        self.record_api_call("board.issues.get");
        let url = format!("/repos/{}/{}/issues/{}", self.owner, self.repo, number);
        let issue: BoardIssueDetailResponse = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to get issue")
            .map_err(|e| self.enrich_api_error(e))?;

        Ok(BoardIssueDetail {
            number: issue.number,
            title: issue.title,
            body: issue.body.unwrap_or_default(),
            author: issue.user.login,
            labels: label_names(issue.labels),
            comment_count: issue.comments.unwrap_or_default(),
            created_at: issue.created_at,
            updated_at: issue.updated_at,
            url: issue.html_url,
        })
    }

    /// Issue-comment thread, unlike `list_issue_comments` (src/github/pr.rs)
    /// which is single-page and truncates past 30 comments. Paginates up to
    /// `MAX_PAGES` rather than being fully unbounded.
    pub(crate) async fn board_list_issue_comments(&self, number: u64) -> Result<Vec<IssueComment>> {
        let base_url = format!(
            "/repos/{}/{}/issues/{}/comments",
            self.owner, self.repo, number
        );
        let mut comments = Vec::new();
        let mut page = 1u32;

        loop {
            self.record_api_call("board.issues.comments");
            let url = format!("{base_url}?per_page={PER_PAGE}&page={page}");
            let response: Vec<BoardIssueCommentResponse> = self
                .octocrab
                .get(&url, None::<&()>)
                .await
                .context("Failed to list issue comments")
                .map_err(|e| self.enrich_api_error(e))?;
            let fetched = response.len();
            comments.extend(response.into_iter().map(|comment| IssueComment {
                id: comment.id,
                body: comment.body.unwrap_or_default(),
                user: comment.user.login,
                created_at: comment.created_at,
            }));

            if fetched < PER_PAGE || page >= MAX_PAGES {
                break;
            }
            page += 1;
        }

        Ok(comments)
    }

    pub(crate) async fn board_list_repo_labels(&self) -> Result<Vec<String>> {
        let base_url = format!("/repos/{}/{}/labels", self.owner, self.repo);
        let mut labels = Vec::new();
        let mut page = 1u32;

        loop {
            self.record_api_call("board.labels.list");
            let url = format!("{base_url}?per_page={PER_PAGE}&page={page}");
            let response: Vec<BoardRepoLabel> = self
                .octocrab
                .get(&url, None::<&()>)
                .await
                .context("Failed to list repository labels")
                .map_err(|e| self.enrich_api_error(e))?;
            let fetched = response.len();
            labels.extend(response.into_iter().map(|label| label.name));

            if fetched < PER_PAGE || page >= MAX_PAGES {
                break;
            }
            page += 1;
        }

        Ok(labels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octocrab::Octocrab;
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

    #[tokio::test]
    async fn board_get_pr_detail_parses_stats_and_labels() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "number": 42,
                "title": "Add board dashboard",
                "body": "Some description",
                "head": { "ref": "feature/board", "sha": "abc123" },
                "base": { "ref": "main" },
                "draft": false,
                "mergeable": true,
                "mergeable_state": "clean",
                "additions": 120,
                "deletions": 30,
                "changed_files": 7,
                "comments": 3,
                "review_comments": 5,
                "labels": [{ "name": "enhancement" }, { "name": "cli" }],
                "html_url": "https://github.com/test-owner/test-repo/pull/42"
            })))
            .mount(&server)
            .await;

        let client = create_test_client(&server).await;
        let detail = client.board_get_pr_detail(42).await.unwrap();

        assert_eq!(detail.number, 42);
        assert_eq!(detail.head_sha, "abc123");
        assert_eq!(detail.additions, 120);
        assert_eq!(detail.deletions, 30);
        assert_eq!(detail.changed_files, 7);
        assert_eq!(detail.comment_count, 3);
        assert_eq!(detail.review_comment_count, 5);
        assert_eq!(detail.labels, vec!["enhancement", "cli"]);
        assert_eq!(detail.mergeable, Some(true));
    }

    #[tokio::test]
    async fn board_get_pr_detail_defaults_missing_optional_fields() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "number": 7,
                "title": "Minimal PR",
                "head": { "ref": "feature/minimal", "sha": "deadbeef" },
                "base": { "ref": "main" },
                "html_url": "https://github.com/test-owner/test-repo/pull/7"
            })))
            .mount(&server)
            .await;

        let client = create_test_client(&server).await;
        let detail = client.board_get_pr_detail(7).await.unwrap();

        assert_eq!(detail.body, "");
        assert!(!detail.is_draft);
        assert_eq!(detail.mergeable, None);
        assert_eq!(detail.additions, 0);
        assert_eq!(detail.deletions, 0);
        assert_eq!(detail.changed_files, 0);
        assert!(detail.labels.is_empty());
    }

    #[tokio::test]
    async fn board_list_pr_files_paginates_and_stops_at_five_pages() {
        let server = MockServer::start().await;
        let full_page = |offset: usize| -> Vec<serde_json::Value> {
            (0..100)
                .map(|i| {
                    serde_json::json!({
                        "filename": format!("file-{}.rs", offset + i),
                        "status": "modified",
                        "additions": 1,
                        "deletions": 1,
                    })
                })
                .collect()
        };

        for page in 1..=6u32 {
            Mock::given(method("GET"))
                .and(path("/repos/test-owner/test-repo/pulls/9/files"))
                .and(query_param("page", page.to_string()))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!(full_page((page as usize - 1) * 100))),
                )
                .mount(&server)
                .await;
        }

        let client = create_test_client(&server).await;
        let files = client.board_list_pr_files(9).await.unwrap();

        // 5 pages of 100 full items each; the 6th page must never be requested.
        assert_eq!(files.len(), 500);
    }

    #[tokio::test]
    async fn board_list_repo_labels_paginates_until_short_page() {
        let server = MockServer::start().await;
        let first_page: Vec<serde_json::Value> = (0..100)
            .map(|i| serde_json::json!({ "name": format!("label-{i}") }))
            .collect();
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/labels"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!(first_page)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/labels"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "name": "last-label" }
            ])))
            .mount(&server)
            .await;

        let client = create_test_client(&server).await;
        let labels = client.board_list_repo_labels().await.unwrap();

        assert_eq!(labels.len(), 101);
        assert_eq!(labels.last().map(String::as_str), Some("last-label"));
    }

    #[tokio::test]
    async fn board_list_open_issues_filters_out_pull_requests() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 1,
                    "title": "A real issue",
                    "html_url": "https://github.com/test-owner/test-repo/issues/1",
                    "user": { "login": "alice" },
                    "labels": [],
                    "comments": 2,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-02T00:00:00Z"
                },
                {
                    "number": 2,
                    "title": "Actually a PR",
                    "html_url": "https://github.com/test-owner/test-repo/issues/2",
                    "user": { "login": "bob" },
                    "labels": [],
                    "comments": 0,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-02T00:00:00Z",
                    "pull_request": {
                        "url": "https://api.github.com/repos/test-owner/test-repo/pulls/2"
                    }
                }
            ])))
            .mount(&server)
            .await;

        let client = create_test_client(&server).await;
        let issues = client.board_list_open_issues(30).await.unwrap();

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 1);
        assert_eq!(issues[0].comment_count, 2);
    }
}
