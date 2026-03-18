use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    build_http_client, ci_status_from_string, delete_empty, get_json, make_issue_comment,
    mergeable_bool, patch_json, post_json, put_json, stack_comment_body, AuthStyle,
};
use crate::ci::CheckRunInfo;
use crate::github::pr::{CiStatus, MergeMethod, PrComment, PrInfo, PrInfoWithHead, PrMergeStatus};
use crate::remote::{ForgeType, RemoteInfo};

const STACK_COMMENT_MARKER: &str = "<!-- stax-stack-comment -->";

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
}

#[derive(Serialize)]
struct MergePullRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    merge_title_field: Option<&'a str>,
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
            "Gitea auth not configured. Set `STAX_GITEA_TOKEN`, `GITEA_TOKEN`, or `STAX_FORGE_TOKEN`.",
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
        let url = format!("{}?state=open", self.repo_url("/pulls"));
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
            &format!("{}?state=open", self.repo_url("/pulls")),
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
            &self.repo_url(&format!("/issues/{}/comments", number)),
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
            &self.repo_url(&format!("/issues/{}/comments", number)),
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
            merge_title_field: commit_title,
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

        Ok(PrMergeStatus {
            number: pr.number,
            title: pr.title,
            state: pr.state.to_uppercase(),
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
        Ok(pr.merged.unwrap_or(false) || pr.state.eq_ignore_ascii_case("closed"))
    }

    pub async fn fetch_checks(&self, sha: &str) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
        let statuses: Vec<GiteaCommitStatus> = get_json(
            &self.client,
            &self.repo_url(&format!("/commits/{}/statuses", sha)),
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
                conclusion: status.status.clone(),
                url: status.target_url.clone(),
                started_at: status.created_at.clone(),
                completed_at: status.updated_at.clone(),
                elapsed_secs: None,
                average_secs: None,
                completion_percent: None,
            })
            .collect::<Vec<_>>();

        let overall = statuses
            .iter()
            .filter_map(|status| status.status.as_deref())
            .find_map(|status| match status {
                "failure" | "error" => Some("failure".to_string()),
                "pending" => Some("pending".to_string()),
                "success" => None,
                _ => None,
            })
            .or_else(|| {
                if statuses.is_empty() {
                    None
                } else {
                    Some("success".to_string())
                }
            });

        Ok((overall, checks))
    }
}

fn normalize_gitea_status(status: Option<&str>) -> String {
    match status.unwrap_or("") {
        "pending" => "in_progress".to_string(),
        _ => "completed".to_string(),
    }
}

fn pr_to_info(pr: &GiteaPull) -> PrInfo {
    PrInfo {
        number: pr.number,
        state: pr.state.to_uppercase(),
        is_draft: pr.draft.unwrap_or(false),
        base: pr.base.ref_name.clone(),
    }
}

fn pr_to_info_with_head(pr: GiteaPull) -> PrInfoWithHead {
    PrInfoWithHead {
        info: pr_to_info(&pr),
        head: pr.head.ref_name.clone(),
        head_label: pr.head.label.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    fn pull_json(overrides: serde_json::Value) -> serde_json::Value {
        let mut base = serde_json::json!({
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
        });
        if let serde_json::Value::Object(map) = overrides {
            base.as_object_mut().unwrap().extend(map);
        }
        base
    }

    fn comment_json(id: u64, body: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "body": body,
            "created_at": "2024-01-01T00:00:00Z",
            "user": { "login": "bot" }
        })
    }

    #[tokio::test]
    async fn test_list_open_prs_by_head() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("Authorization", "token test-token"))
            .and(path("/repos/org/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([pull_json(serde_json::json!({}))])),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let prs = client.list_open_prs_by_head().await.unwrap();
        let pr = prs.get("feature-a").unwrap();
        assert_eq!(pr.info.number, 3);
        assert_eq!(pr.info.state, "OPEN");
        assert_eq!(pr.info.base, "main");
        assert!(!pr.info.is_draft);
        assert_eq!(pr.head, "feature-a");
        assert_eq!(pr.head_label.as_deref(), Some("org:feature-a"));
    }

    #[tokio::test]
    async fn test_list_open_prs_by_head_multiple() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                pull_json(serde_json::json!({})),
                pull_json(serde_json::json!({
                    "number": 4,
                    "head": { "ref": "feature-b", "sha": "def789", "label": "org:feature-b" },
                    "base": { "ref": "develop", "sha": "aaa111", "label": "org:develop" }
                })),
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let prs = client.list_open_prs_by_head().await.unwrap();
        assert_eq!(prs.len(), 2);
        assert_eq!(prs["feature-a"].info.number, 3);
        assert_eq!(prs["feature-b"].info.number, 4);
        assert_eq!(prs["feature-b"].info.base, "develop");
    }

    #[tokio::test]
    async fn test_find_open_pr_by_head_found() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([pull_json(serde_json::json!({}))])),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let pr = client
            .find_open_pr_by_head("feature-a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pr.info.number, 3);
        assert_eq!(pr.head, "feature-a");
    }

    #[tokio::test]
    async fn test_find_pr_delegates_to_find_open_pr() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([pull_json(serde_json::json!({}))])),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let pr = client.find_pr("feature-a").await.unwrap().unwrap();
        assert_eq!(pr.number, 3);
        assert_eq!(pr.state, "OPEN");
        assert_eq!(pr.base, "main");
    }

    #[tokio::test]
    async fn test_find_open_pr_by_head_not_found() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        assert!(client
            .find_open_pr_by_head("no-such")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_create_pr() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("POST"))
            .and(path("/repos/org/repo/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(pull_json(serde_json::json!({
                "number": 10,
                "draft": true
            }))))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let pr = client
            .create_pr("feature-a", "main", "Feature", "body", true)
            .await
            .unwrap();
        assert_eq!(pr.number, 10);
        assert!(pr.is_draft);
    }

    #[tokio::test]
    async fn test_get_pr() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({}))),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let pr = client.get_pr(3).await.unwrap();
        assert_eq!(pr.number, 3);
        assert_eq!(pr.state, "OPEN");
        assert!(!pr.is_draft);
    }

    #[tokio::test]
    async fn test_get_pr_with_head() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({}))),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let pr = client.get_pr_with_head(3).await.unwrap();
        assert_eq!(pr.info.number, 3);
        assert_eq!(pr.info.state, "OPEN");
        assert_eq!(pr.info.base, "main");
        assert_eq!(pr.head, "feature-a");
        assert_eq!(pr.head_label.as_deref(), Some("org:feature-a"));
    }

    #[tokio::test]
    async fn test_update_pr_base() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("PATCH"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({
                "base": { "ref": "develop", "sha": "def456", "label": "org:develop" }
            }))))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        client.update_pr_base(3, "develop").await.unwrap();
    }

    #[tokio::test]
    async fn test_update_pr_body() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("PATCH"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({
                "body": "new body"
            }))))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        client.update_pr_body(3, "new body").await.unwrap();
    }

    #[tokio::test]
    async fn test_get_pr_body() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({
                "body": "hello world"
            }))))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let body = client.get_pr_body(3).await.unwrap();
        assert_eq!(body, "hello world");
    }

    #[tokio::test]
    async fn test_get_pr_body_null() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({
                "body": null
            }))))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let body = client.get_pr_body(3).await.unwrap();
        assert_eq!(body, "");
    }

    #[tokio::test]
    async fn test_create_stack_comment() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("POST"))
            .and(path("/repos/org/repo/issues/3/comments"))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(comment_json(100, "<!-- stax-stack-comment -->\nstack info")),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        client.create_stack_comment(3, "stack info").await.unwrap();
    }

    #[tokio::test]
    async fn test_update_stack_comment_existing() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/issues/3/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                comment_json(50, "<!-- stax-stack-comment -->\nold stack")
            ])))
            .mount(&server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/org/repo/issues/comments/50"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(comment_json(50, "<!-- stax-stack-comment -->\nnew stack")),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        client
            .update_stack_comment(3, "new stack")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_update_stack_comment_creates_when_missing() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/issues/3/comments"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([comment_json(99, "normal comment")])),
            )
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/org/repo/issues/3/comments"))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(comment_json(100, "<!-- stax-stack-comment -->\nstack")),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        client.update_stack_comment(3, "stack").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_stack_comment() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/issues/3/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                comment_json(50, "<!-- stax-stack-comment -->\nstack")
            ])))
            .mount(&server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/repos/org/repo/issues/comments/50"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        client.delete_stack_comment(3).await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_stack_comment_noop_when_missing() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/issues/3/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        client.delete_stack_comment(3).await.unwrap();
    }

    #[tokio::test]
    async fn test_list_all_comments() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/issues/3/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                comment_json(1, "first"),
                comment_json(2, "second")
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let comments = client.list_all_comments(3).await.unwrap();
        assert_eq!(comments.len(), 2);
    }

    #[tokio::test]
    async fn test_merge_pr() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("POST"))
            .and(path("/repos/org/repo/pulls/3/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        client
            .merge_pr(3, MergeMethod::Squash, Some("feat: squash"), Some("abc123"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_get_pr_merge_status() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({}))),
            )
            .mount(&server)
            .await;

        // fetch_checks is called for CI status
        Mock::given(method("GET"))
            .and(path("/repos/org/repo/commits/abc123/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "context": "ci", "status": "success", "target_url": null, "created_at": null, "updated_at": null }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let status = client.get_pr_merge_status(3).await.unwrap();
        assert_eq!(status.number, 3);
        assert_eq!(status.title, "Feature");
        assert_eq!(status.state, "OPEN");
        assert!(!status.is_draft);
        assert_eq!(status.mergeable_state, "clean");
        assert!(status.mergeable.unwrap());
        assert_eq!(status.head_sha, "abc123");
        assert!(matches!(status.ci_status, CiStatus::Success));
    }

    #[tokio::test]
    async fn test_get_pr_merge_status_not_mergeable() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({
                "mergeable": false,
                "mergeable_state": null
            }))))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/commits/abc123/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let status = client.get_pr_merge_status(3).await.unwrap();
        assert_eq!(status.mergeable_state, "unknown");
        // mergeable from PR overrides mergeable_bool for Gitea
        assert_eq!(status.mergeable, Some(false));
    }

    #[tokio::test]
    async fn test_get_pr_merge_status_pending_ci() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({}))),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/commits/abc123/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "context": "ci", "status": "pending", "target_url": null, "created_at": null, "updated_at": null }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let status = client.get_pr_merge_status(3).await.unwrap();
        assert!(matches!(status.ci_status, CiStatus::Pending));
    }

    #[tokio::test]
    async fn test_get_pr_merge_status_ci_fetch_failure_treated_as_no_ci() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({}))),
            )
            .mount(&server)
            .await;

        // fetch_checks returns 500 — get_pr_merge_status uses .ok() to swallow error
        Mock::given(method("GET"))
            .and(path("/repos/org/repo/commits/abc123/statuses"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let status = client.get_pr_merge_status(3).await.unwrap();
        // Error is swallowed → NoCi
        assert!(matches!(status.ci_status, CiStatus::NoCi));
    }

    #[tokio::test]
    async fn test_is_pr_merged_true() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({
                "merged": true,
                "state": "closed"
            }))))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        assert!(client.is_pr_merged(3).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_pr_merged_false() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({}))),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        assert!(!client.is_pr_merged(3).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_pr_merged_closed_without_merge_flag() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        // Gitea treats closed PRs as merged (see is_pr_merged impl)
        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({
                "merged": false,
                "state": "closed"
            }))))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        assert!(client.is_pr_merged(3).await.unwrap());
    }

    #[tokio::test]
    async fn test_fetch_checks_maps_gitea_statuses() {
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
    async fn test_fetch_checks_failure_takes_precedence() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/commits/sha1/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "context": "a", "status": "failure", "target_url": null, "created_at": null, "updated_at": null },
                { "context": "b", "status": "success", "target_url": null, "created_at": null, "updated_at": null }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let (overall, _) = client.fetch_checks("sha1").await.unwrap();
        assert_eq!(overall.as_deref(), Some("failure"));
    }

    #[tokio::test]
    async fn test_fetch_checks_all_success() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/commits/sha2/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "context": "a", "status": "success", "target_url": null, "created_at": null, "updated_at": null }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let (overall, _) = client.fetch_checks("sha2").await.unwrap();
        assert_eq!(overall.as_deref(), Some("success"));
    }

    #[tokio::test]
    async fn test_fetch_checks_empty() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/commits/sha3/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let (overall, checks) = client.fetch_checks("sha3").await.unwrap();
        assert!(overall.is_none());
        assert!(checks.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_checks_error_status_is_failure() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/commits/sha4/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "context": "ci", "status": "error", "target_url": null, "created_at": null, "updated_at": null }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let (overall, checks) = client.fetch_checks("sha4").await.unwrap();
        assert_eq!(overall.as_deref(), Some("failure"));
        assert_eq!(checks[0].conclusion.as_deref(), Some("error"));
    }

    #[tokio::test]
    async fn test_fetch_checks_check_run_info_fields() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/commits/sha5/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "context": "build",
                    "status": "success",
                    "target_url": "https://ci.example.com/42",
                    "created_at": "2024-01-01T00:00:00Z",
                    "updated_at": "2024-01-01T00:05:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let (_, checks) = client.fetch_checks("sha5").await.unwrap();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "build");
        assert_eq!(checks[0].status, "completed");
        assert_eq!(checks[0].conclusion.as_deref(), Some("success"));
        assert_eq!(
            checks[0].url.as_deref(),
            Some("https://ci.example.com/42")
        );
        assert_eq!(
            checks[0].started_at.as_deref(),
            Some("2024-01-01T00:00:00Z")
        );
        assert_eq!(
            checks[0].completed_at.as_deref(),
            Some("2024-01-01T00:05:00Z")
        );
    }

    #[tokio::test]
    async fn test_fetch_checks_null_context_defaults_to_status() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/commits/sha6/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "context": null, "status": "success", "target_url": null, "created_at": null, "updated_at": null }
            ])))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let (_, checks) = client.fetch_checks("sha6").await.unwrap();
        assert_eq!(checks[0].name, "status");
    }

    // --- API error response tests ---

    #[tokio::test]
    async fn test_get_pr_returns_error_on_404() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/999"))
            .respond_with(
                ResponseTemplate::new(404).set_body_string(r#"{"message":"Not Found"}"#),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let err = client.get_pr(999).await.unwrap_err();
        assert!(
            err.to_string().contains("404"),
            "Error should mention 404: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_create_pr_returns_error_on_401() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "bad-token");

        Mock::given(method("POST"))
            .and(path("/repos/org/repo/pulls"))
            .respond_with(
                ResponseTemplate::new(401).set_body_string(r#"{"message":"Unauthorized"}"#),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let err = client
            .create_pr("feat", "main", "title", "body", false)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("401"),
            "Error should mention 401: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_merge_pr_returns_error_on_405() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("POST"))
            .and(path("/repos/org/repo/pulls/3/merge"))
            .respond_with(
                ResponseTemplate::new(405)
                    .set_body_string(r#"{"message":"Method Not Allowed"}"#),
            )
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let err = client
            .merge_pr(3, MergeMethod::Merge, None, None)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("405"),
            "Error should mention 405: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_merge_pr_rebase() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("POST"))
            .and(path("/repos/org/repo/pulls/3/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        client
            .merge_pr(3, MergeMethod::Rebase, None, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_draft_pr_state() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITEA_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pull_json(serde_json::json!({
                "draft": true
            }))))
            .mount(&server)
            .await;

        let client = GiteaClient::new(&remote_info(&server)).unwrap();
        let pr = client.get_pr(3).await.unwrap();
        assert!(pr.is_draft);
    }

    #[tokio::test]
    async fn test_state_normalization() {
        // Gitea uses simple uppercase conversion
        assert_eq!("open".to_uppercase(), "OPEN");
        assert_eq!("closed".to_uppercase(), "CLOSED");
    }
}
