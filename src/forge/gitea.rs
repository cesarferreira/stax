use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    aggregate_ci_overall, build_http_client, ci_status_from_string, delete_empty, get_json,
    make_issue_comment, mergeable_bool, patch_json, post_json, stack_comment_body, AuthStyle,
    STACK_COMMENT_MARKER,
};
use crate::ci::CheckRunInfo;
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
            &self.repo_url(&format!("/issues/{}/comments?limit=50", number)),
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
        Ok(pr.merged.unwrap_or(false))
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

    #[tokio::test]
    async fn test_list_open_prs_by_head() {
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
}
