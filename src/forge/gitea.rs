use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    aggregate_ci_overall, build_http_client, ci_status_from_string, delete_empty, get_json,
    make_issue_comment, mergeable_bool, patch_json, post_json, stack_comment_body, AuthStyle,
    PrActivity, RepoIssueListItem, RepoPrListItem, ReviewActivity, STACK_COMMENT_MARKER,
};
use crate::ci::CheckRunInfo;
use crate::github::client::OpenPrInfo;
use crate::github::pr::{MergeMethod, PrComment, PrInfo, PrInfoWithHead, PrMergeStatus};
use crate::remote::{ForgeType, RemoteInfo};

#[derive(Clone)]
pub struct GiteaClient {
    client: Client,
    api_base_url: String,
    owner: String,
    repo: String,
}

#[derive(Debug, Deserialize)]
struct GiteaPull {
    number: u64,
    state: String,
    title: String,
    body: Option<String>,
    draft: Option<bool>,
    mergeable: Option<bool>,
    mergeable_state: Option<String>,
    merged: Option<bool>,
    head: GiteaBranchRef,
    base: GiteaBranchRef,
    user: Option<GiteaUser>,
    html_url: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct GiteaBranchRef {
    #[serde(rename = "ref")]
    ref_name: String,
    sha: Option<String>,
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GiteaUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GiteaComment {
    id: u64,
    body: String,
    created_at: DateTime<Utc>,
    user: GiteaUser,
}

#[derive(Debug, Deserialize)]
struct GiteaCommitStatus {
    context: Option<String>,
    status: Option<String>,
    target_url: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GiteaIssue {
    number: u64,
    title: String,
    html_url: Option<String>,
    user: Option<GiteaUser>,
    labels: Vec<GiteaLabel>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct GiteaLabel {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GiteaReview {
    user: Option<GiteaUser>,
    state: Option<String>,
    submitted_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct CreatePullRequest<'a> {
    head: &'a str,
    base: &'a str,
    title: &'a str,
    body: &'a str,
    draft: bool,
}

#[derive(Serialize)]
struct UpdatePullRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    base: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    draft: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
}

#[derive(Serialize)]
struct MergePullRequest<'a> {
    #[serde(rename = "MergeTitleField", skip_serializing_if = "Option::is_none")]
    merge_title: Option<&'a str>,
    #[serde(rename = "Do")]
    do_field: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    head_commit_id: Option<&'a str>,
}

#[derive(Serialize)]
struct CreateCommentRequest<'a> {
    body: &'a str,
}

impl GiteaClient {
    pub fn new(remote: &RemoteInfo) -> Result<Self> {
        if remote.forge != ForgeType::Gitea {
            bail!("Internal error: expected Gitea remote");
        }

        let token = super::forge_token(ForgeType::Gitea).context(
            "Gitea auth not configured. Use `stax auth` or set `STAX_GITEA_TOKEN`, `GITEA_TOKEN`, or `STAX_FORGE_TOKEN`.",
        )?;

        Ok(Self {
            client: build_http_client(&token, AuthStyle::AuthorizationToken)?,
            api_base_url: remote
                .api_base_url
                .clone()
                .context("Missing Gitea API base URL")?,
            owner: remote.owner().to_string(),
            repo: remote.repo.clone(),
        })
    }

    fn repo_url(&self, suffix: &str) -> String {
        format!(
            "{}/repos/{}/{}{}",
            self.api_base_url, self.owner, self.repo, suffix
        )
    }

    pub async fn find_open_pr_by_head(&self, branch: &str) -> Result<Option<PrInfoWithHead>> {
        let url = format!("{}?state=open&limit=50", self.repo_url("/pulls"));
        let prs: Vec<GiteaPull> = get_json(&self.client, &url).await?;
        Ok(prs
            .into_iter()
            .find(|pr| pr.head.ref_name == branch)
            .map(pr_to_info_with_head))
    }

    pub async fn find_pr(&self, branch: &str) -> Result<Option<PrInfo>> {
        Ok(self.find_open_pr_by_head(branch).await?.map(|pr| pr.info))
    }

    pub async fn list_open_prs_by_head(&self) -> Result<HashMap<String, PrInfoWithHead>> {
        let prs: Vec<GiteaPull> = get_json(
            &self.client,
            &format!("{}?state=open&limit=50", self.repo_url("/pulls")),
        )
        .await?;
        Ok(prs
            .into_iter()
            .map(pr_to_info_with_head)
            .map(|pr| (pr.head.clone(), pr))
            .collect())
    }

    pub async fn create_pr(
        &self,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
        is_draft: bool,
    ) -> Result<PrInfo> {
        let request = CreatePullRequest {
            head,
            base,
            title,
            body,
            draft: is_draft,
        };
        let pr: GiteaPull = post_json(&self.client, &self.repo_url("/pulls"), &request).await?;
        Ok(pr_to_info(&pr))
    }

    pub async fn get_pr(&self, number: u64) -> Result<PrInfo> {
        let pr: GiteaPull =
            get_json(&self.client, &self.repo_url(&format!("/pulls/{}", number))).await?;
        Ok(pr_to_info(&pr))
    }

    pub async fn get_pr_with_head(&self, number: u64) -> Result<PrInfoWithHead> {
        let pr: GiteaPull =
            get_json(&self.client, &self.repo_url(&format!("/pulls/{}", number))).await?;
        Ok(pr_to_info_with_head(pr))
    }

    pub async fn update_pr_base(&self, number: u64, new_base: &str) -> Result<()> {
        let request = UpdatePullRequest {
            base: Some(new_base),
            body: None,
            draft: None,
            title: None,
        };
        let _: GiteaPull = patch_json(
            &self.client,
            &self.repo_url(&format!("/pulls/{}", number)),
            &request,
        )
        .await?;
        Ok(())
    }

    pub async fn update_pr_title(&self, number: u64, title: &str) -> Result<()> {
        let request = UpdatePullRequest {
            base: None,
            body: None,
            draft: None,
            title: Some(title),
        };
        let _: GiteaPull = patch_json(
            &self.client,
            &self.repo_url(&format!("/pulls/{}", number)),
            &request,
        )
        .await?;
        Ok(())
    }

    pub async fn update_pr_body(&self, number: u64, body: &str) -> Result<()> {
        let request = UpdatePullRequest {
            base: None,
            body: Some(body),
            draft: None,
            title: None,
        };
        let _: GiteaPull = patch_json(
            &self.client,
            &self.repo_url(&format!("/pulls/{}", number)),
            &request,
        )
        .await?;
        Ok(())
    }

    pub async fn set_pr_draft(&self, number: u64, is_draft: bool) -> Result<()> {
        let request = UpdatePullRequest {
            base: None,
            body: None,
            draft: Some(is_draft),
            title: None,
        };
        let _: GiteaPull = patch_json(
            &self.client,
            &self.repo_url(&format!("/pulls/{}", number)),
            &request,
        )
        .await?;
        Ok(())
    }

    pub async fn get_pr_body(&self, number: u64) -> Result<String> {
        let pr: GiteaPull =
            get_json(&self.client, &self.repo_url(&format!("/pulls/{}", number))).await?;
        Ok(pr.body.unwrap_or_default())
    }

    pub async fn update_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()> {
        if let Some(comment_id) = self.find_stack_comment_id(number).await? {
            let body = serde_json::json!({ "body": stack_comment_body(stack_comment) });
            let _: GiteaComment = patch_json(
                &self.client,
                &self.repo_url(&format!("/issues/comments/{}", comment_id)),
                &body,
            )
            .await?;
            Ok(())
        } else {
            self.create_stack_comment(number, stack_comment).await
        }
    }

    pub async fn create_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()> {
        let request = CreateCommentRequest {
            body: &stack_comment_body(stack_comment),
        };
        let _: GiteaComment = post_json(
            &self.client,
            &self.repo_url(&format!("/issues/{}/comments", number)),
            &request,
        )
        .await?;
        Ok(())
    }

    pub async fn delete_stack_comment(&self, number: u64) -> Result<()> {
        let Some(comment_id) = self.find_stack_comment_id(number).await? else {
            return Ok(());
        };
        delete_empty(
            &self.client,
            &self.repo_url(&format!("/issues/comments/{}", comment_id)),
        )
        .await
    }

    async fn find_stack_comment_id(&self, number: u64) -> Result<Option<u64>> {
        let comments: Vec<GiteaComment> = get_json(
            &self.client,
            &self.repo_url(&format!("/issues/{}/comments?limit=50", number)),
        )
        .await?;
        Ok(comments
            .into_iter()
            .find(|comment| comment.body.contains(STACK_COMMENT_MARKER))
            .map(|comment| comment.id))
    }

    pub async fn list_all_comments(&self, number: u64) -> Result<Vec<PrComment>> {
        let comments: Vec<GiteaComment> = get_json(
            &self.client,
            &self.repo_url(&format!("/issues/{}/comments?limit=50", number)),
        )
        .await?;
        let mut comments = comments
            .into_iter()
            .map(|comment| {
                make_issue_comment(
                    comment.id,
                    comment.body,
                    comment.user.login,
                    comment.created_at,
                )
            })
            .collect::<Vec<_>>();
        comments.sort_by_key(|comment| comment.created_at());
        Ok(comments)
    }

    pub async fn merge_pr(
        &self,
        number: u64,
        method: MergeMethod,
        commit_title: Option<&str>,
        sha: Option<&str>,
    ) -> Result<()> {
        let request = MergePullRequest {
            merge_title: commit_title,
            do_field: method.as_str(),
            head_commit_id: sha,
        };
        let _: serde_json::Value = post_json(
            &self.client,
            &self.repo_url(&format!("/pulls/{}/merge", number)),
            &request,
        )
        .await?;
        Ok(())
    }

    pub async fn get_pr_merge_status(&self, number: u64) -> Result<PrMergeStatus> {
        let pr: GiteaPull =
            get_json(&self.client, &self.repo_url(&format!("/pulls/{}", number))).await?;

        let mergeable_state = pr.mergeable_state.clone().unwrap_or_else(|| {
            if pr.mergeable == Some(true) {
                "clean"
            } else {
                "unknown"
            }
            .into()
        });
        let ci_status = self
            .fetch_checks(pr.head.sha.as_deref().unwrap_or_default())
            .await
            .ok()
            .and_then(|(status, _)| status);

        let state = normalize_gitea_state(&pr);
        Ok(PrMergeStatus {
            number: pr.number,
            title: pr.title,
            state,
            is_draft: pr.draft.unwrap_or(false),
            mergeable: pr.mergeable.or_else(|| mergeable_bool(&mergeable_state)),
            mergeable_state,
            ci_status: ci_status_from_string(ci_status.as_deref()),
            review_decision: None,
            approvals: 0,
            changes_requested: false,
            head_sha: pr.head.sha.unwrap_or_default(),
        })
    }

    pub async fn is_pr_merged(&self, number: u64) -> Result<bool> {
        let pr: GiteaPull =
            get_json(&self.client, &self.repo_url(&format!("/pulls/{}", number))).await?;
        Ok(pr.merged.unwrap_or(false))
    }

    pub async fn get_pr_head_sha(&self, number: u64) -> Result<String> {
        let pr: GiteaPull =
            get_json(&self.client, &self.repo_url(&format!("/pulls/{}", number))).await?;
        Ok(pr.head.sha.unwrap_or_default())
    }

    pub async fn fetch_checks(&self, sha: &str) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
        let statuses: Vec<GiteaCommitStatus> = get_json(
            &self.client,
            &self.repo_url(&format!("/commits/{}/statuses?limit=50", sha)),
        )
        .await?;

        let checks = statuses
            .iter()
            .map(|status| CheckRunInfo {
                name: status
                    .context
                    .clone()
                    .unwrap_or_else(|| "status".to_string()),
                status: normalize_gitea_status(status.status.as_deref()),
                conclusion: status.status.as_deref().map(normalize_gitea_conclusion),
                url: status.target_url.clone(),
                started_at: status.created_at.clone(),
                completed_at: status.updated_at.clone(),
                elapsed_secs: None,
                average_secs: None,
                completion_percent: None,
            })
            .collect::<Vec<_>>();

        let overall = aggregate_ci_overall(
            statuses
                .iter()
                .filter_map(|status| status.status.as_deref()),
            |s| matches!(s, "failure" | "error"),
            |s| matches!(s, "pending"),
        );

        Ok((overall, checks))
    }

    pub async fn list_open_pull_requests(&self, limit: u8) -> Result<Vec<RepoPrListItem>> {
        let limit = limit.clamp(1, 50);
        let url = format!(
            "{}?state=open&sort=newest&limit={}",
            self.repo_url("/pulls"),
            limit
        );
        let prs: Vec<GiteaPull> = get_json(&self.client, &url).await?;
        Ok(prs
            .into_iter()
            .map(|pr| RepoPrListItem {
                number: pr.number,
                title: pr.title,
                url: pr.html_url.unwrap_or_default(),
                author: pr
                    .user
                    .map(|u| u.login)
                    .unwrap_or_else(|| "unknown".to_string()),
                head_branch: pr.head.ref_name,
                base_branch: pr.base.ref_name,
                state: normalize_gitea_state_str(&pr.state, pr.merged),
                is_draft: pr.draft.unwrap_or(false),
                created_at: pr.created_at.unwrap_or_default(),
            })
            .collect())
    }

    pub async fn list_open_issues(&self, limit: u8) -> Result<Vec<RepoIssueListItem>> {
        let limit = limit.clamp(1, 50);
        let url = format!(
            "{}?state=open&type=issues&sort=updated&limit={}",
            self.repo_url("/issues"),
            limit
        );
        let issues: Vec<GiteaIssue> = get_json(&self.client, &url).await?;
        Ok(issues
            .into_iter()
            .map(|issue| RepoIssueListItem {
                number: issue.number,
                title: issue.title,
                url: issue.html_url.unwrap_or_default(),
                author: issue
                    .user
                    .map(|u| u.login)
                    .unwrap_or_else(|| "unknown".to_string()),
                labels: issue.labels.into_iter().filter_map(|l| l.name).collect(),
                updated_at: issue.updated_at,
            })
            .collect())
    }

    pub async fn get_current_user(&self) -> Result<String> {
        let url = format!("{}/user", self.api_base_url);
        let user: GiteaUser = get_json(&self.client, &url).await?;
        Ok(user.login)
    }

    pub async fn get_user_open_prs(&self, username: &str) -> Result<Vec<OpenPrInfo>> {
        let url = format!("{}?state=open&limit=50", self.repo_url("/pulls"));
        let prs: Vec<GiteaPull> = get_json(&self.client, &url).await?;
        Ok(prs
            .into_iter()
            .filter_map(|pr| {
                let is_author = pr.user.as_ref().is_some_and(|u| u.login == username);
                if !is_author {
                    return None;
                }
                let state = normalize_gitea_state(&pr);
                let is_draft = pr.draft.unwrap_or(false);
                Some(OpenPrInfo {
                    number: pr.number,
                    head_branch: pr.head.ref_name,
                    base_branch: pr.base.ref_name,
                    state,
                    is_draft,
                })
            })
            .collect())
    }

    pub async fn get_recent_merged_prs(
        &self,
        hours: i64,
        username: &str,
    ) -> Result<Vec<PrActivity>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        let url = format!(
            "{}?state=closed&sort=recentupdate&limit=30",
            self.repo_url("/pulls")
        );
        let prs: Vec<GiteaPull> = get_json(&self.client, &url).await?;
        Ok(prs
            .into_iter()
            .filter(|pr| {
                pr.merged == Some(true)
                    && pr.user.as_ref().is_some_and(|u| u.login == username)
                    && pr.updated_at.is_some_and(|t| t >= since)
            })
            .map(|pr| PrActivity {
                number: pr.number,
                title: pr.title,
                timestamp: pr.updated_at.unwrap_or_default(),
                url: pr.html_url.unwrap_or_default(),
            })
            .collect())
    }

    pub async fn get_recent_opened_prs(
        &self,
        hours: i64,
        username: &str,
    ) -> Result<Vec<PrActivity>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        let url = format!(
            "{}?state=open&sort=newest&limit=30",
            self.repo_url("/pulls")
        );
        let prs: Vec<GiteaPull> = get_json(&self.client, &url).await?;
        Ok(prs
            .into_iter()
            .filter(|pr| {
                pr.user.as_ref().is_some_and(|u| u.login == username)
                    && pr.created_at.is_some_and(|t| t >= since)
            })
            .map(|pr| PrActivity {
                number: pr.number,
                title: pr.title,
                timestamp: pr.created_at.unwrap_or_default(),
                url: pr.html_url.unwrap_or_default(),
            })
            .collect())
    }

    pub async fn get_reviews_received(
        &self,
        hours: i64,
        username: &str,
    ) -> Result<Vec<ReviewActivity>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        // Get user's open PRs, then fetch reviews on each
        let prs_url = format!("{}?state=open&limit=20", self.repo_url("/pulls"));
        let prs: Vec<GiteaPull> = get_json(&self.client, &prs_url).await?;

        let mut reviews = Vec::new();
        for pr in prs {
            if pr.user.as_ref().is_none_or(|u| u.login != username) {
                continue;
            }
            let reviews_url = self.repo_url(&format!("/pulls/{}/reviews", pr.number));
            let pr_reviews: Vec<GiteaReview> = match get_json(&self.client, &reviews_url).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            for review in pr_reviews {
                let reviewer = match &review.user {
                    Some(u) if u.login != username => u.login.clone(),
                    _ => continue,
                };
                if let Some(ts) = review.submitted_at {
                    if ts >= since {
                        reviews.push(ReviewActivity {
                            pr_number: pr.number,
                            pr_title: pr.title.clone(),
                            reviewer,
                            state: review.state.unwrap_or_else(|| "COMMENTED".to_string()),
                            timestamp: ts,
                            is_received: true,
                        });
                    }
                }
            }
        }
        Ok(reviews)
    }

    pub async fn get_reviews_given(
        &self,
        _hours: i64,
        _username: &str,
    ) -> Result<Vec<ReviewActivity>> {
        // Not implemented: Gitea has no efficient way to query "reviews given by user".
        // Would require iterating all open PRs and checking reviews on each.
        Ok(vec![])
    }
}

fn normalize_gitea_state_str(state: &str, merged: Option<bool>) -> String {
    if merged.unwrap_or(false) {
        "MERGED".to_string()
    } else {
        state.to_uppercase()
    }
}

fn normalize_gitea_status(status: Option<&str>) -> String {
    match status.unwrap_or("") {
        "pending" => "in_progress".to_string(),
        _ => "completed".to_string(),
    }
}

fn normalize_gitea_conclusion(status: &str) -> String {
    match status {
        "success" => "success".to_string(),
        "failure" | "error" => "failure".to_string(),
        _ => status.to_string(),
    }
}

fn normalize_gitea_state(pr: &GiteaPull) -> String {
    normalize_gitea_state_str(&pr.state, pr.merged)
}

fn pr_to_info(pr: &GiteaPull) -> PrInfo {
    PrInfo {
        number: pr.number,
        state: normalize_gitea_state(pr),
        is_draft: pr.draft.unwrap_or(false),
        base: pr.base.ref_name.clone(),
    }
}

fn pr_to_info_with_head(pr: GiteaPull) -> PrInfoWithHead {
    PrInfoWithHead {
        info: pr_to_info(&pr),
        head: pr.head.ref_name.clone(),
        head_label: pr.head.label.clone(),
        title: pr.title.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn remote_info(server: &MockServer) -> RemoteInfo {
        RemoteInfo {
            name: "origin".to_string(),
            forge: ForgeType::Gitea,
            host: "gitea.example.com".to_string(),
            namespace: "org".to_string(),
            repo: "repo".to_string(),
            base_url: "https://gitea.example.com".to_string(),
            api_base_url: Some(server.uri()),
        }
    }

    #[tokio::test]
    async fn test_list_open_prs_by_head() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/repos/org/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 3,
                    "state": "open",
                    "title": "Feature",
                    "body": "body",
                    "draft": false,
                    "mergeable": true,
                    "mergeable_state": "clean",
                    "merged": false,
                    "head": { "ref": "feature-a", "sha": "abc123", "label": "org:feature-a" },
                    "base": { "ref": "main", "sha": "def456", "label": "org:main" }
                }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let prs = client.list_open_prs_by_head().await.unwrap();
        let pr = prs.get("feature-a").unwrap();
        assert_eq!(pr.info.number, 3);
        assert_eq!(pr.info.base, "main");
    }

    #[tokio::test]
    async fn test_fetch_checks_maps_gitea_statuses() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/repos/org/repo/commits/abc123/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "context": "test",
                    "status": "pending",
                    "target_url": "https://ci.example.com/1",
                    "created_at": "2024-01-01T00:00:00Z",
                    "updated_at": "2024-01-01T00:00:30Z"
                },
                {
                    "context": "lint",
                    "status": "success",
                    "target_url": "https://ci.example.com/2",
                    "created_at": "2024-01-01T00:00:00Z",
                    "updated_at": "2024-01-01T00:00:10Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let (overall, checks) = client.fetch_checks("abc123").await.unwrap();
        assert_eq!(overall.as_deref(), Some("pending"));
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].status, "in_progress");
    }

    #[tokio::test]
    async fn test_list_open_pull_requests() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/repos/org/repo/pulls"))
            .and(query_param("state", "open"))
            .and(query_param("sort", "newest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 5,
                    "state": "open",
                    "title": "Add widget",
                    "body": "description",
                    "draft": false,
                    "mergeable": true,
                    "merged": false,
                    "head": { "ref": "add-widget", "sha": "aaa111" },
                    "base": { "ref": "main", "sha": "bbb222" },
                    "user": { "login": "carol" },
                    "html_url": "https://gitea.example.com/org/repo/pulls/5",
                    "created_at": "2024-07-01T10:00:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let prs = client.list_open_pull_requests(30).await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 5);
        assert_eq!(prs[0].title, "Add widget");
        assert_eq!(prs[0].author, "carol");
        assert_eq!(prs[0].head_branch, "add-widget");
        assert_eq!(prs[0].base_branch, "main");
    }

    #[tokio::test]
    async fn test_list_open_issues() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/repos/org/repo/issues"))
            .and(query_param("state", "open"))
            .and(query_param("type", "issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 12,
                    "title": "Fix timeout",
                    "html_url": "https://gitea.example.com/org/repo/issues/12",
                    "user": { "login": "dave" },
                    "labels": [{ "name": "bug" }],
                    "updated_at": "2024-07-10T14:00:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let issues = client.list_open_issues(30).await.unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 12);
        assert_eq!(issues[0].title, "Fix timeout");
        assert_eq!(issues[0].author, "dave");
        assert_eq!(issues[0].labels, vec!["bug"]);
    }

    #[tokio::test]
    async fn test_get_current_user() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/user"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "login": "carol" })),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let user = client.get_current_user().await.unwrap();
        assert_eq!(user, "carol");
    }

    #[tokio::test]
    async fn test_get_user_open_prs_filters_by_author() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/repos/org/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 1,
                    "state": "open",
                    "title": "Carol's PR",
                    "draft": false,
                    "merged": false,
                    "head": { "ref": "carol-feature", "sha": "aaa" },
                    "base": { "ref": "main", "sha": "bbb" },
                    "user": { "login": "carol" }
                },
                {
                    "number": 2,
                    "state": "open",
                    "title": "Dave's PR",
                    "draft": true,
                    "merged": false,
                    "head": { "ref": "dave-feature", "sha": "ccc" },
                    "base": { "ref": "main", "sha": "ddd" },
                    "user": { "login": "dave" }
                }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let prs = client.get_user_open_prs("carol").await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 1);
        assert_eq!(prs[0].head_branch, "carol-feature");
    }

    #[tokio::test]
    async fn test_get_recent_merged_prs() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/repos/org/repo/pulls"))
            .and(query_param("state", "closed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 10,
                    "state": "closed",
                    "title": "Merged PR",
                    "merged": true,
                    "head": { "ref": "feat-a", "sha": "aaa" },
                    "base": { "ref": "main", "sha": "bbb" },
                    "user": { "login": "carol" },
                    "html_url": "https://gitea.example.com/org/repo/pulls/10",
                    "updated_at": "2099-01-01T12:00:00Z"
                },
                {
                    "number": 11,
                    "state": "closed",
                    "title": "Closed not merged",
                    "merged": false,
                    "head": { "ref": "feat-b", "sha": "ccc" },
                    "base": { "ref": "main", "sha": "ddd" },
                    "user": { "login": "carol" },
                    "updated_at": "2099-01-01T12:00:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let prs = client.get_recent_merged_prs(9999, "carol").await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 10);
        assert_eq!(prs[0].title, "Merged PR");
    }

    #[tokio::test]
    async fn test_get_recent_opened_prs() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/repos/org/repo/pulls"))
            .and(query_param("state", "open"))
            .and(query_param("sort", "newest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 20,
                    "state": "open",
                    "title": "New PR",
                    "merged": false,
                    "head": { "ref": "feat-new", "sha": "eee" },
                    "base": { "ref": "main", "sha": "fff" },
                    "user": { "login": "carol" },
                    "created_at": "2099-01-01T10:00:00Z"
                },
                {
                    "number": 21,
                    "state": "open",
                    "title": "Other user PR",
                    "merged": false,
                    "head": { "ref": "other", "sha": "ggg" },
                    "base": { "ref": "main", "sha": "hhh" },
                    "user": { "login": "dave" },
                    "created_at": "2099-01-01T10:00:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let prs = client.get_recent_opened_prs(9999, "carol").await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 20);
        assert_eq!(prs[0].title, "New PR");
    }

    #[tokio::test]
    async fn test_get_reviews_received() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        // Mock: user's open PRs
        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/repos/org/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 30,
                    "state": "open",
                    "title": "Carol's PR",
                    "merged": false,
                    "head": { "ref": "feat", "sha": "aaa" },
                    "base": { "ref": "main", "sha": "bbb" },
                    "user": { "login": "carol" }
                }
            ])))
            .mount(&server)
            .await;

        // Mock: reviews on PR 30
        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/repos/org/repo/pulls/30/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "user": { "login": "dave" },
                    "state": "APPROVED",
                    "submitted_at": "2099-01-01T09:00:00Z"
                },
                {
                    "user": { "login": "carol" },
                    "state": "COMMENTED",
                    "submitted_at": "2099-01-01T09:30:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let reviews = client.get_reviews_received(9999, "carol").await.unwrap();
        // carol's self-review should be excluded
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].reviewer, "dave");
        assert_eq!(reviews[0].state, "APPROVED");
        assert!(reviews[0].is_received);
    }
}
