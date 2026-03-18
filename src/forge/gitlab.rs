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
pub struct GitLabClient {
    client: Client,
    api_base_url: String,
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct GitLabMr {
    iid: u64,
    title: String,
    state: String,
    draft: bool,
    source_branch: String,
    target_branch: String,
    description: Option<String>,
    merge_status: Option<String>,
    detailed_merge_status: Option<String>,
    web_url: Option<String>,
    head_pipeline: Option<GitLabPipeline>,
    sha: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabPipeline {
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabUser {
    username: String,
}

#[derive(Debug, Deserialize)]
struct GitLabNote {
    id: u64,
    body: String,
    created_at: DateTime<Utc>,
    author: GitLabUser,
}

#[derive(Debug, Deserialize)]
struct GitLabCommitStatus {
    name: Option<String>,
    status: Option<String>,
    target_url: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
}

#[derive(Serialize)]
struct CreateMrRequest<'a> {
    source_branch: &'a str,
    target_branch: &'a str,
    title: &'a str,
    description: &'a str,
    remove_source_branch: bool,
    draft: bool,
}

#[derive(Serialize)]
struct UpdateMrRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    target_branch: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
}

#[derive(Serialize)]
struct MergeMrRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    merge_commit_message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha: Option<&'a str>,
    squash: bool,
}

#[derive(Serialize)]
struct CreateNoteRequest<'a> {
    body: &'a str,
}

impl GitLabClient {
    pub fn new(remote: &RemoteInfo) -> Result<Self> {
        if remote.forge != ForgeType::GitLab {
            bail!("Internal error: expected GitLab remote");
        }

        let token = super::forge_token(ForgeType::GitLab).context(
            "GitLab auth not configured. Set `STAX_GITLAB_TOKEN`, `GITLAB_TOKEN`, or `STAX_FORGE_TOKEN`.",
        )?;

        Ok(Self {
            client: build_http_client(&token, AuthStyle::PrivateToken)?,
            api_base_url: remote
                .api_base_url
                .clone()
                .context("Missing GitLab API base URL")?,
            project_id: remote.encoded_project_path(),
        })
    }

    fn project_url(&self, suffix: &str) -> String {
        format!(
            "{}/projects/{}{}",
            self.api_base_url, self.project_id, suffix
        )
    }

    pub async fn find_open_pr_by_head(&self, branch: &str) -> Result<Option<PrInfoWithHead>> {
        let url = format!(
            "{}?state=opened&source_branch={}",
            self.project_url("/merge_requests"),
            branch
        );
        let prs: Vec<GitLabMr> = get_json(&self.client, &url).await?;
        Ok(prs
            .into_iter()
            .find(|mr| mr.source_branch == branch)
            .map(mr_to_pr_with_head))
    }

    pub async fn find_pr(&self, branch: &str) -> Result<Option<PrInfo>> {
        Ok(self.find_open_pr_by_head(branch).await?.map(|mr| mr.info))
    }

    pub async fn list_open_prs_by_head(&self) -> Result<HashMap<String, PrInfoWithHead>> {
        let prs: Vec<GitLabMr> = get_json(
            &self.client,
            &self.project_url("/merge_requests?state=opened"),
        )
        .await?;
        Ok(prs
            .into_iter()
            .map(mr_to_pr_with_head)
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
        let request = CreateMrRequest {
            source_branch: head,
            target_branch: base,
            title,
            description: body,
            remove_source_branch: false,
            draft: is_draft,
        };
        let mr: GitLabMr =
            post_json(&self.client, &self.project_url("/merge_requests"), &request).await?;
        Ok(mr_to_pr_info(&mr))
    }

    pub async fn get_pr(&self, number: u64) -> Result<PrInfo> {
        let mr: GitLabMr = get_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}", number)),
        )
        .await?;
        Ok(mr_to_pr_info(&mr))
    }

    pub async fn get_pr_with_head(&self, number: u64) -> Result<PrInfoWithHead> {
        let mr: GitLabMr = get_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}", number)),
        )
        .await?;
        Ok(mr_to_pr_with_head(mr))
    }

    pub async fn update_pr_base(&self, number: u64, new_base: &str) -> Result<()> {
        let request = UpdateMrRequest {
            target_branch: Some(new_base),
            description: None,
        };
        let _: GitLabMr = put_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}", number)),
            &request,
        )
        .await?;
        Ok(())
    }

    pub async fn update_pr_body(&self, number: u64, body: &str) -> Result<()> {
        let request = UpdateMrRequest {
            target_branch: None,
            description: Some(body),
        };
        let _: GitLabMr = put_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}", number)),
            &request,
        )
        .await?;
        Ok(())
    }

    pub async fn get_pr_body(&self, number: u64) -> Result<String> {
        let mr: GitLabMr = get_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}", number)),
        )
        .await?;
        Ok(mr.description.unwrap_or_default())
    }

    pub async fn update_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()> {
        if let Some(note_id) = self.find_stack_comment_id(number).await? {
            let body = serde_json::json!({ "body": stack_comment_body(stack_comment) });
            let _: GitLabNote = put_json(
                &self.client,
                &self.project_url(&format!("/merge_requests/{}/notes/{}", number, note_id)),
                &body,
            )
            .await?;
            Ok(())
        } else {
            self.create_stack_comment(number, stack_comment).await
        }
    }

    pub async fn create_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()> {
        let request = CreateNoteRequest {
            body: &stack_comment_body(stack_comment),
        };
        let _: GitLabNote = post_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}/notes", number)),
            &request,
        )
        .await?;
        Ok(())
    }

    pub async fn delete_stack_comment(&self, number: u64) -> Result<()> {
        let Some(note_id) = self.find_stack_comment_id(number).await? else {
            return Ok(());
        };
        delete_empty(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}/notes/{}", number, note_id)),
        )
        .await
    }

    async fn find_stack_comment_id(&self, number: u64) -> Result<Option<u64>> {
        let notes: Vec<GitLabNote> = get_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}/notes", number)),
        )
        .await?;
        Ok(notes
            .into_iter()
            .find(|note| note.body.contains(STACK_COMMENT_MARKER))
            .map(|note| note.id))
    }

    pub async fn list_all_comments(&self, number: u64) -> Result<Vec<PrComment>> {
        let notes: Vec<GitLabNote> = get_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}/notes", number)),
        )
        .await?;
        let mut comments = notes
            .into_iter()
            .map(|note| {
                make_issue_comment(note.id, note.body, note.author.username, note.created_at)
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
        let request = MergeMrRequest {
            merge_commit_message: commit_title,
            sha,
            squash: matches!(method, MergeMethod::Squash),
        };
        let _: serde_json::Value = put_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}/merge", number)),
            &request,
        )
        .await?;
        Ok(())
    }

    pub async fn get_pr_merge_status(&self, number: u64) -> Result<PrMergeStatus> {
        let mr: GitLabMr = get_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}", number)),
        )
        .await?;

        let mergeable_state = mr
            .detailed_merge_status
            .clone()
            .or(mr.merge_status.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let mergeable = mergeable_bool(&mergeable_state);
        let ci_status = mr
            .head_pipeline
            .as_ref()
            .and_then(|pipeline| pipeline.status.as_deref())
            .map(|status| {
                if matches!(status, "running" | "pending" | "created") {
                    "pending"
                } else {
                    status
                }
            });

        Ok(PrMergeStatus {
            number: mr.iid,
            title: mr.title,
            state: normalize_gitlab_state(&mr.state),
            is_draft: mr.draft,
            mergeable,
            mergeable_state,
            ci_status: ci_status_from_string(ci_status),
            review_decision: None,
            approvals: 0,
            changes_requested: false,
            head_sha: mr.sha.unwrap_or_default(),
        })
    }

    pub async fn is_pr_merged(&self, number: u64) -> Result<bool> {
        let mr: GitLabMr = get_json(
            &self.client,
            &self.project_url(&format!("/merge_requests/{}", number)),
        )
        .await?;
        Ok(mr.state.eq_ignore_ascii_case("merged"))
    }

    pub async fn fetch_checks(&self, sha: &str) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
        let statuses: Vec<GitLabCommitStatus> = get_json(
            &self.client,
            &self.project_url(&format!("/repository/commits/{}/statuses", sha)),
        )
        .await?;

        let checks = statuses
            .iter()
            .map(|status| CheckRunInfo {
                name: status
                    .name
                    .clone()
                    .unwrap_or_else(|| "pipeline".to_string()),
                status: normalize_gitlab_check_status(status.status.as_deref()),
                conclusion: status.status.clone(),
                url: status.target_url.clone(),
                started_at: status.started_at.clone(),
                completed_at: status.finished_at.clone(),
                elapsed_secs: None,
                average_secs: None,
                completion_percent: None,
            })
            .collect::<Vec<_>>();

        let overall = statuses
            .iter()
            .filter_map(|status| status.status.as_deref())
            .find_map(|status| match status {
                "failed" | "canceled" => Some("failure".to_string()),
                "running" | "pending" | "created" => Some("pending".to_string()),
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

fn normalize_gitlab_check_status(status: Option<&str>) -> String {
    match status.unwrap_or("") {
        "running" | "pending" | "created" => "in_progress".to_string(),
        _ => "completed".to_string(),
    }
}

fn normalize_gitlab_state(state: &str) -> String {
    match state.to_ascii_lowercase().as_str() {
        "opened" => "OPEN".to_string(),
        "closed" => "CLOSED".to_string(),
        "merged" => "MERGED".to_string(),
        _ => state.to_ascii_uppercase(),
    }
}

fn mr_to_pr_info(mr: &GitLabMr) -> PrInfo {
    PrInfo {
        number: mr.iid,
        state: normalize_gitlab_state(&mr.state),
        is_draft: mr.draft,
        base: mr.target_branch.clone(),
    }
}

fn mr_to_pr_with_head(mr: GitLabMr) -> PrInfoWithHead {
    PrInfoWithHead {
        info: mr_to_pr_info(&mr),
        head: mr.source_branch,
        head_label: mr.web_url,
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
            forge: ForgeType::GitLab,
            host: "gitlab.example.com".to_string(),
            namespace: "group/subgroup".to_string(),
            repo: "repo".to_string(),
            base_url: "https://gitlab.example.com".to_string(),
            api_base_url: Some(server.uri()),
        }
    }

    fn mr_json(overrides: serde_json::Value) -> serde_json::Value {
        let mut base = serde_json::json!({
            "iid": 7,
            "title": "Feature",
            "state": "opened",
            "draft": false,
            "source_branch": "feature-a",
            "target_branch": "main",
            "description": "body",
            "merge_status": "can_be_merged",
            "detailed_merge_status": "mergeable",
            "web_url": "https://gitlab.example.com/group/subgroup/repo/-/merge_requests/7",
            "sha": "abc123"
        });
        if let serde_json::Value::Object(map) = overrides {
            base.as_object_mut().unwrap().extend(map);
        }
        base
    }

    fn note_json(id: u64, body: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "body": body,
            "created_at": "2024-01-01T00:00:00Z",
            "author": { "username": "bot" }
        })
    }

    #[tokio::test]
    async fn test_list_open_prs_by_head() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([mr_json(
                    serde_json::json!({})
                )])),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let prs = client.list_open_prs_by_head().await.unwrap();
        let pr = prs.get("feature-a").unwrap();
        assert_eq!(pr.info.number, 7);
        assert_eq!(pr.info.state, "OPEN");
        assert_eq!(pr.info.base, "main");
        assert!(!pr.info.is_draft);
        assert_eq!(pr.head, "feature-a");
        assert_eq!(
            pr.head_label.as_deref(),
            Some("https://gitlab.example.com/group/subgroup/repo/-/merge_requests/7")
        );
    }

    #[tokio::test]
    async fn test_list_open_prs_by_head_multiple() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                mr_json(serde_json::json!({"iid": 7, "source_branch": "feature-a"})),
                mr_json(serde_json::json!({"iid": 8, "source_branch": "feature-b", "target_branch": "develop"})),
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let prs = client.list_open_prs_by_head().await.unwrap();
        assert_eq!(prs.len(), 2);
        assert_eq!(prs["feature-a"].info.number, 7);
        assert_eq!(prs["feature-b"].info.number, 8);
        assert_eq!(prs["feature-b"].info.base, "develop");
    }

    #[tokio::test]
    async fn test_find_open_pr_by_head_found() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("source_branch", "feature-a"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([mr_json(
                    serde_json::json!({})
                )])),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let pr = client
            .find_open_pr_by_head("feature-a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pr.info.number, 7);
        assert_eq!(pr.head, "feature-a");
    }

    #[tokio::test]
    async fn test_find_pr_delegates_to_find_open_pr() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([mr_json(
                    serde_json::json!({})
                )])),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let pr = client.find_pr("feature-a").await.unwrap().unwrap();
        assert_eq!(pr.number, 7);
        assert_eq!(pr.state, "OPEN");
        // find_pr returns PrInfo, not PrInfoWithHead
        assert_eq!(pr.base, "main");
    }

    #[tokio::test]
    async fn test_find_open_pr_by_head_not_found() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        assert!(client
            .find_open_pr_by_head("no-such")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_create_pr() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("POST"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .respond_with(ResponseTemplate::new(201).set_body_json(mr_json(serde_json::json!({
                "iid": 10,
                "draft": true
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
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
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({}))),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let pr = client.get_pr(7).await.unwrap();
        assert_eq!(pr.number, 7);
        assert_eq!(pr.state, "OPEN");
        assert!(!pr.is_draft);
    }

    #[tokio::test]
    async fn test_get_pr_with_head() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({}))),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let pr = client.get_pr_with_head(7).await.unwrap();
        assert_eq!(pr.info.number, 7);
        assert_eq!(pr.info.state, "OPEN");
        assert_eq!(pr.info.base, "main");
        assert_eq!(pr.head, "feature-a");
        assert_eq!(
            pr.head_label.as_deref(),
            Some("https://gitlab.example.com/group/subgroup/repo/-/merge_requests/7")
        );
    }

    #[tokio::test]
    async fn test_update_pr_base() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("PUT"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "target_branch": "develop"
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        client.update_pr_base(7, "develop").await.unwrap();
    }

    #[tokio::test]
    async fn test_update_pr_body() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("PUT"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "description": "new body"
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        client.update_pr_body(7, "new body").await.unwrap();
    }

    #[tokio::test]
    async fn test_get_pr_body() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "description": "hello world"
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let body = client.get_pr_body(7).await.unwrap();
        assert_eq!(body, "hello world");
    }

    #[tokio::test]
    async fn test_get_pr_body_null_description() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "description": null
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let body = client.get_pr_body(7).await.unwrap();
        assert_eq!(body, "");
    }

    #[tokio::test]
    async fn test_create_stack_comment() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("POST"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/notes",
            ))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(note_json(100, "<!-- stax-stack-comment -->\nstack info")),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        client.create_stack_comment(7, "stack info").await.unwrap();
    }

    #[tokio::test]
    async fn test_update_stack_comment_existing() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        // find_stack_comment_id lists notes
        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/notes",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                note_json(50, "<!-- stax-stack-comment -->\nold stack")
            ])))
            .mount(&server)
            .await;

        // update existing note
        Mock::given(method("PUT"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/notes/50",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(note_json(50, "<!-- stax-stack-comment -->\nnew stack")),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        client
            .update_stack_comment(7, "new stack")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_update_stack_comment_creates_when_missing() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        // No existing stack comment
        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/notes",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([note_json(99, "normal comment")])),
            )
            .mount(&server)
            .await;

        // Falls back to create
        Mock::given(method("POST"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/notes",
            ))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(note_json(100, "<!-- stax-stack-comment -->\nstack")),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        client.update_stack_comment(7, "stack").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_stack_comment() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/notes",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                note_json(50, "<!-- stax-stack-comment -->\nstack")
            ])))
            .mount(&server)
            .await;

        Mock::given(method("DELETE"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/notes/50",
            ))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        client.delete_stack_comment(7).await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_stack_comment_noop_when_missing() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/notes",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        client.delete_stack_comment(7).await.unwrap();
    }

    #[tokio::test]
    async fn test_list_all_comments() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/notes",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                note_json(1, "first"),
                note_json(2, "second")
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let comments = client.list_all_comments(7).await.unwrap();
        assert_eq!(comments.len(), 2);
    }

    #[tokio::test]
    async fn test_merge_pr_squash() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("PUT"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/merge",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        client
            .merge_pr(7, MergeMethod::Squash, Some("feat: squash"), Some("abc123"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_get_pr_merge_status() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "head_pipeline": { "status": "success" }
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let status = client.get_pr_merge_status(7).await.unwrap();
        assert_eq!(status.number, 7);
        assert_eq!(status.title, "Feature");
        assert_eq!(status.state, "OPEN");
        assert!(!status.is_draft);
        assert_eq!(status.mergeable_state, "mergeable");
        assert!(status.mergeable.unwrap());
        assert_eq!(status.head_sha, "abc123");
        assert!(matches!(status.ci_status, CiStatus::Success));
    }

    #[tokio::test]
    async fn test_get_pr_merge_status_pending_pipeline() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "head_pipeline": { "status": "running" }
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let status = client.get_pr_merge_status(7).await.unwrap();
        assert!(matches!(status.ci_status, CiStatus::Pending));
    }

    #[tokio::test]
    async fn test_is_pr_merged_true() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "state": "merged"
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        assert!(client.is_pr_merged(7).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_pr_merged_false() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({}))),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        assert!(!client.is_pr_merged(7).await.unwrap());
    }

    #[tokio::test]
    async fn test_get_pr_merge_status_no_pipeline() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "head_pipeline": null
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let status = client.get_pr_merge_status(7).await.unwrap();
        // No pipeline → NoCi
        assert!(matches!(status.ci_status, CiStatus::NoCi));
    }

    #[tokio::test]
    async fn test_get_pr_merge_status_falls_back_to_merge_status() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "detailed_merge_status": null,
                "merge_status": "can_be_merged"
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let status = client.get_pr_merge_status(7).await.unwrap();
        assert_eq!(status.mergeable_state, "can_be_merged");
        assert!(status.mergeable.unwrap());
    }

    #[tokio::test]
    async fn test_get_pr_merge_status_both_null_defaults_unknown() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "detailed_merge_status": null,
                "merge_status": null
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let status = client.get_pr_merge_status(7).await.unwrap();
        assert_eq!(status.mergeable_state, "unknown");
        assert!(status.mergeable.is_none());
    }

    #[tokio::test]
    async fn test_normalize_gitlab_state() {
        assert_eq!(normalize_gitlab_state("opened"), "OPEN");
        assert_eq!(normalize_gitlab_state("closed"), "CLOSED");
        assert_eq!(normalize_gitlab_state("merged"), "MERGED");
        assert_eq!(normalize_gitlab_state("weird"), "WEIRD");
    }

    #[tokio::test]
    async fn test_fetch_checks_maps_gitlab_statuses() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/repository/commits/abc123/statuses",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "name": "test",
                    "status": "running",
                    "target_url": "https://ci.example.com/1",
                    "started_at": "2024-01-01T00:00:00Z",
                    "finished_at": null
                },
                {
                    "name": "lint",
                    "status": "success",
                    "target_url": "https://ci.example.com/2",
                    "started_at": "2024-01-01T00:00:00Z",
                    "finished_at": "2024-01-01T00:01:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let (overall, checks) = client.fetch_checks("abc123").await.unwrap();
        assert_eq!(overall.as_deref(), Some("pending"));
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].status, "in_progress");
    }

    #[tokio::test]
    async fn test_fetch_checks_failure_takes_precedence() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/repository/commits/sha1/statuses",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "name": "a", "status": "failed", "target_url": null, "started_at": null, "finished_at": null },
                { "name": "b", "status": "success", "target_url": null, "started_at": null, "finished_at": null }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let (overall, _) = client.fetch_checks("sha1").await.unwrap();
        assert_eq!(overall.as_deref(), Some("failure"));
    }

    #[tokio::test]
    async fn test_fetch_checks_all_success() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/repository/commits/sha2/statuses",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "name": "a", "status": "success", "target_url": null, "started_at": null, "finished_at": null }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let (overall, _) = client.fetch_checks("sha2").await.unwrap();
        assert_eq!(overall.as_deref(), Some("success"));
    }

    #[tokio::test]
    async fn test_fetch_checks_empty() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/repository/commits/sha3/statuses",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let (overall, checks) = client.fetch_checks("sha3").await.unwrap();
        assert!(overall.is_none());
        assert!(checks.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_checks_canceled_is_failure() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/repository/commits/sha4/statuses",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "name": "ci", "status": "canceled", "target_url": null, "started_at": null, "finished_at": null }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let (overall, checks) = client.fetch_checks("sha4").await.unwrap();
        assert_eq!(overall.as_deref(), Some("failure"));
        assert_eq!(checks[0].status, "completed");
        assert_eq!(checks[0].conclusion.as_deref(), Some("canceled"));
    }

    #[tokio::test]
    async fn test_fetch_checks_check_run_info_fields() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/repository/commits/sha5/statuses",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "name": "build",
                    "status": "success",
                    "target_url": "https://ci.example.com/42",
                    "started_at": "2024-01-01T00:00:00Z",
                    "finished_at": "2024-01-01T00:05:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
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
    async fn test_fetch_checks_null_name_defaults_to_pipeline() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/repository/commits/sha6/statuses",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "name": null, "status": "success", "target_url": null, "started_at": null, "finished_at": null }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let (_, checks) = client.fetch_checks("sha6").await.unwrap();
        assert_eq!(checks[0].name, "pipeline");
    }

    // --- API error response tests ---

    #[tokio::test]
    async fn test_get_pr_returns_error_on_404() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/999",
            ))
            .respond_with(ResponseTemplate::new(404).set_body_string(r#"{"message":"404 Not Found"}"#))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
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
        std::env::set_var("STAX_GITLAB_TOKEN", "bad-token");

        Mock::given(method("POST"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .respond_with(ResponseTemplate::new(401).set_body_string(r#"{"message":"401 Unauthorized"}"#))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
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
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("PUT"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/merge",
            ))
            .respond_with(
                ResponseTemplate::new(405)
                    .set_body_string(r#"{"message":"Method Not Allowed"}"#),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let err = client
            .merge_pr(7, MergeMethod::Merge, None, None)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("405"),
            "Error should mention 405: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_merge_pr_non_squash() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("PUT"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7/merge",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        // Merge (not squash) - should work fine
        client
            .merge_pr(7, MergeMethod::Merge, None, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_is_pr_merged_closed_is_not_merged() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        // GitLab "closed" != "merged" (unlike Gitea)
        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "state": "closed"
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        assert!(!client.is_pr_merged(7).await.unwrap());
    }

    #[tokio::test]
    async fn test_draft_mr_state() {
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(mr_json(serde_json::json!({
                "draft": true
            }))))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let pr = client.get_pr(7).await.unwrap();
        assert!(pr.is_draft);

        // Also verify merge status reports draft
        let status = client.get_pr_merge_status(7).await.unwrap();
        assert!(status.is_draft);
    }
}
