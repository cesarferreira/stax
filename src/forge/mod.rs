use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;

use crate::ci::CheckRunInfo;
use crate::config::Config;
use crate::github::client::{GitHubClient, OpenPrInfo};
use crate::github::pr::{
    CiStatus, IssueComment, MergeMethod, PrComment, PrInfo, PrInfoWithHead, PrMergeStatus,
    ReviewComment,
};
use crate::remote::{ForgeType, RemoteInfo};

mod gitea;
mod gitlab;

use gitea::GiteaClient;
use gitlab::GitLabClient;

#[derive(Clone, Copy)]
pub enum AuthStyle {
    AuthorizationBearer,
    AuthorizationToken,
    PrivateToken,
}

#[derive(Clone)]
pub enum ForgeClient {
    GitHub(GitHubClient),
    GitLab(GitLabClient),
    Gitea(GiteaClient),
}

impl ForgeClient {
    pub fn new(remote: &RemoteInfo) -> Result<Self> {
        match remote.forge {
            ForgeType::GitHub => Ok(Self::GitHub(GitHubClient::new(
                remote.owner(),
                &remote.repo,
                remote.api_base_url.clone(),
            )?)),
            ForgeType::GitLab => Ok(Self::GitLab(GitLabClient::new(remote)?)),
            ForgeType::Gitea => Ok(Self::Gitea(GiteaClient::new(remote)?)),
        }
    }

    pub fn api_call_stats(&self) -> Option<crate::github::client::ApiCallStats> {
        match self {
            Self::GitHub(client) => Some(client.api_call_stats()),
            Self::GitLab(_) | Self::Gitea(_) => None,
        }
    }

    pub async fn find_open_pr_by_head(
        &self,
        head_owner: &str,
        branch: &str,
    ) -> Result<Option<PrInfoWithHead>> {
        match self {
            Self::GitHub(client) => client.find_open_pr_by_head(head_owner, branch).await,
            Self::GitLab(client) => client.find_open_pr_by_head(branch).await,
            Self::Gitea(client) => client.find_open_pr_by_head(branch).await,
        }
    }

    pub async fn find_pr(&self, branch: &str) -> Result<Option<PrInfo>> {
        match self {
            Self::GitHub(client) => client.find_pr(branch).await,
            Self::GitLab(client) => client.find_pr(branch).await,
            Self::Gitea(client) => client.find_pr(branch).await,
        }
    }

    pub async fn list_open_prs_by_head(&self) -> Result<HashMap<String, PrInfoWithHead>> {
        match self {
            Self::GitHub(client) => client.list_open_prs_by_head().await,
            Self::GitLab(client) => client.list_open_prs_by_head().await,
            Self::Gitea(client) => client.list_open_prs_by_head().await,
        }
    }

    pub async fn create_pr(
        &self,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
        is_draft: bool,
    ) -> Result<PrInfo> {
        match self {
            Self::GitHub(client) => client.create_pr(head, base, title, body, is_draft).await,
            Self::GitLab(client) => client.create_pr(head, base, title, body, is_draft).await,
            Self::Gitea(client) => client.create_pr(head, base, title, body, is_draft).await,
        }
    }

    pub async fn get_pr(&self, number: u64) -> Result<PrInfo> {
        match self {
            Self::GitHub(client) => client.get_pr(number).await,
            Self::GitLab(client) => client.get_pr(number).await,
            Self::Gitea(client) => client.get_pr(number).await,
        }
    }

    pub async fn get_pr_with_head(&self, number: u64) -> Result<PrInfoWithHead> {
        match self {
            Self::GitHub(client) => client.get_pr_with_head(number).await,
            Self::GitLab(client) => client.get_pr_with_head(number).await,
            Self::Gitea(client) => client.get_pr_with_head(number).await,
        }
    }

    pub async fn update_pr_base(&self, number: u64, new_base: &str) -> Result<()> {
        match self {
            Self::GitHub(client) => client.update_pr_base(number, new_base).await,
            Self::GitLab(client) => client.update_pr_base(number, new_base).await,
            Self::Gitea(client) => client.update_pr_base(number, new_base).await,
        }
    }

    pub async fn update_pr_body(&self, number: u64, body: &str) -> Result<()> {
        match self {
            Self::GitHub(client) => client.update_pr_body(number, body).await,
            Self::GitLab(client) => client.update_pr_body(number, body).await,
            Self::Gitea(client) => client.update_pr_body(number, body).await,
        }
    }

    pub async fn get_pr_body(&self, number: u64) -> Result<String> {
        match self {
            Self::GitHub(client) => client.get_pr_body(number).await,
            Self::GitLab(client) => client.get_pr_body(number).await,
            Self::Gitea(client) => client.get_pr_body(number).await,
        }
    }

    pub async fn update_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()> {
        match self {
            Self::GitHub(client) => client.update_stack_comment(number, stack_comment).await,
            Self::GitLab(client) => client.update_stack_comment(number, stack_comment).await,
            Self::Gitea(client) => client.update_stack_comment(number, stack_comment).await,
        }
    }

    pub async fn create_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()> {
        match self {
            Self::GitHub(client) => client.create_stack_comment(number, stack_comment).await,
            Self::GitLab(client) => client.create_stack_comment(number, stack_comment).await,
            Self::Gitea(client) => client.create_stack_comment(number, stack_comment).await,
        }
    }

    pub async fn delete_stack_comment(&self, number: u64) -> Result<()> {
        match self {
            Self::GitHub(client) => client.delete_stack_comment(number).await,
            Self::GitLab(client) => client.delete_stack_comment(number).await,
            Self::Gitea(client) => client.delete_stack_comment(number).await,
        }
    }

    pub async fn list_all_comments(&self, number: u64) -> Result<Vec<PrComment>> {
        match self {
            Self::GitHub(client) => client.list_all_comments(number).await,
            Self::GitLab(client) => client.list_all_comments(number).await,
            Self::Gitea(client) => client.list_all_comments(number).await,
        }
    }

    pub async fn merge_pr(
        &self,
        number: u64,
        method: MergeMethod,
        commit_title: Option<&str>,
        sha: Option<&str>,
    ) -> Result<()> {
        match self {
            Self::GitHub(client) => {
                client
                    .merge_pr(
                        number,
                        method,
                        commit_title.map(str::to_string),
                        sha.map(str::to_string),
                    )
                    .await
            }
            Self::GitLab(client) => client.merge_pr(number, method, commit_title, sha).await,
            Self::Gitea(client) => client.merge_pr(number, method, commit_title, sha).await,
        }
    }

    pub async fn get_pr_merge_status(&self, number: u64) -> Result<PrMergeStatus> {
        match self {
            Self::GitHub(client) => client.get_pr_merge_status(number).await,
            Self::GitLab(client) => client.get_pr_merge_status(number).await,
            Self::Gitea(client) => client.get_pr_merge_status(number).await,
        }
    }

    pub async fn is_pr_merged(&self, number: u64) -> Result<bool> {
        match self {
            Self::GitHub(client) => client.is_pr_merged(number).await,
            Self::GitLab(client) => client.is_pr_merged(number).await,
            Self::Gitea(client) => client.is_pr_merged(number).await,
        }
    }

    pub async fn fetch_checks(
        &self,
        repo: &crate::git::GitRepo,
        sha: &str,
    ) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
        match self {
            Self::GitHub(client) => {
                crate::commands::ci::fetch_github_checks(repo, client, sha).await
            }
            Self::GitLab(client) => client.fetch_checks(sha).await,
            Self::Gitea(client) => client.fetch_checks(sha).await,
        }
    }

    pub async fn request_reviewers(&self, number: u64, reviewers: &[String]) -> Result<()> {
        match self {
            Self::GitHub(client) => client.request_reviewers(number, reviewers).await,
            Self::GitLab(_) | Self::Gitea(_) => Ok(()),
        }
    }

    pub async fn get_requested_reviewers(&self, number: u64) -> Result<Vec<String>> {
        match self {
            Self::GitHub(client) => client.get_requested_reviewers(number).await,
            Self::GitLab(_) | Self::Gitea(_) => Ok(Vec::new()),
        }
    }

    pub async fn add_labels(&self, number: u64, labels: &[String]) -> Result<()> {
        match self {
            Self::GitHub(client) => client.add_labels(number, labels).await,
            Self::GitLab(_) | Self::Gitea(_) => Ok(()),
        }
    }

    pub async fn add_assignees(&self, number: u64, assignees: &[String]) -> Result<()> {
        match self {
            Self::GitHub(client) => client.add_assignees(number, assignees).await,
            Self::GitLab(_) | Self::Gitea(_) => Ok(()),
        }
    }

    pub async fn get_current_user(&self) -> Result<String> {
        match self {
            Self::GitHub(client) => client.get_current_user().await,
            Self::GitLab(_) | Self::Gitea(_) => {
                bail!("`stax branch track --all-prs` is currently only supported for GitHub")
            }
        }
    }

    pub async fn get_user_open_prs(&self, username: &str) -> Result<Vec<OpenPrInfo>> {
        match self {
            Self::GitHub(client) => client.get_user_open_prs(username).await,
            Self::GitLab(_) | Self::Gitea(_) => {
                bail!("`stax branch track --all-prs` is currently only supported for GitHub")
            }
        }
    }
}

pub fn stack_comment_body(stack_comment: &str) -> String {
    format!("<!-- stax-stack-comment -->\n{}", stack_comment)
}

pub fn is_stack_comment(body: &str) -> bool {
    body.contains("<!-- stax-stack-comment -->")
}

pub fn forge_token(forge: ForgeType) -> Option<String> {
    match forge {
        ForgeType::GitHub => Config::github_token(),
        ForgeType::GitLab => read_env_token("STAX_GITLAB_TOKEN")
            .or_else(|| read_env_token("GITLAB_TOKEN"))
            .or_else(|| read_env_token("STAX_FORGE_TOKEN")),
        ForgeType::Gitea => read_env_token("STAX_GITEA_TOKEN")
            .or_else(|| read_env_token("GITEA_TOKEN"))
            .or_else(|| read_env_token("STAX_FORGE_TOKEN")),
    }
}

fn read_env_token(var_name: &str) -> Option<String> {
    std::env::var(var_name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn base_headers(token: &str, auth_style: AuthStyle) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("stax"));
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    match auth_style {
        AuthStyle::AuthorizationBearer => {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token))
                    .context("Invalid auth header")?,
            );
        }
        AuthStyle::AuthorizationToken => {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("token {}", token))
                    .context("Invalid auth header")?,
            );
        }
        AuthStyle::PrivateToken => {
            headers.insert(
                "PRIVATE-TOKEN",
                HeaderValue::from_str(token).context("Invalid private token header")?,
            );
        }
    }
    Ok(headers)
}

fn build_http_client(token: &str, auth_style: AuthStyle) -> Result<Client> {
    Ok(Client::builder()
        .default_headers(base_headers(token, auth_style)?)
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60))
        .build()
        .context("Failed to build forge HTTP client")?)
}

async fn get_json<T: DeserializeOwned>(client: &Client, url: &str) -> Result<T> {
    let response = client.get(url).send().await?;
    parse_json_response(response).await
}

async fn post_json<T: DeserializeOwned, B: Serialize>(
    client: &Client,
    url: &str,
    body: &B,
) -> Result<T> {
    let response = client.post(url).json(body).send().await?;
    parse_json_response(response).await
}

async fn put_json<T: DeserializeOwned, B: Serialize>(
    client: &Client,
    url: &str,
    body: &B,
) -> Result<T> {
    let response = client.put(url).json(body).send().await?;
    parse_json_response(response).await
}

async fn patch_json<T: DeserializeOwned, B: Serialize>(
    client: &Client,
    url: &str,
    body: &B,
) -> Result<T> {
    let response = client.patch(url).json(body).send().await?;
    parse_json_response(response).await
}

async fn delete_empty(client: &Client, url: &str) -> Result<()> {
    let response = client.delete(url).send().await?;
    if response.status().is_success() || response.status().as_u16() == 404 {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("Forge API request failed: {} {}", status, body);
    }
}

async fn parse_json_response<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("Forge API request failed: {} {}", status, body);
    }
}

fn mergeable_bool(mergeable_state: &str) -> Option<bool> {
    match mergeable_state {
        "checking" | "unchecked" | "preparing" | "unknown" => None,
        "mergeable" | "can_be_merged" | "clean" => Some(true),
        _ => Some(false),
    }
}

fn ci_status_from_string(status: Option<&str>) -> CiStatus {
    status.map(CiStatus::from_str).unwrap_or(CiStatus::NoCi)
}

fn make_issue_comment(id: u64, body: String, user: String, created_at: DateTime<Utc>) -> PrComment {
    PrComment::Issue(IssueComment {
        id,
        body,
        user,
        created_at,
    })
}

fn make_review_comment(
    id: u64,
    body: String,
    user: String,
    path: String,
    line: Option<u32>,
    created_at: DateTime<Utc>,
    diff_hunk: Option<String>,
) -> PrComment {
    PrComment::Review(ReviewComment {
        id,
        body,
        user,
        path,
        line,
        start_line: None,
        created_at,
        diff_hunk,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- stack_comment_body / is_stack_comment ---

    #[test]
    fn test_stack_comment_body_prepends_marker() {
        let result = stack_comment_body("my stack info");
        assert_eq!(result, "<!-- stax-stack-comment -->\nmy stack info");
    }

    #[test]
    fn test_is_stack_comment_detects_marker() {
        assert!(is_stack_comment(
            "<!-- stax-stack-comment -->\nsome stack"
        ));
        assert!(is_stack_comment(
            "prefix <!-- stax-stack-comment --> suffix"
        ));
    }

    #[test]
    fn test_is_stack_comment_false_for_regular_comment() {
        assert!(!is_stack_comment("just a normal comment"));
        assert!(!is_stack_comment(""));
    }

    // --- read_env_token ---

    #[test]
    fn test_read_env_token_returns_trimmed_value() {
        std::env::set_var("__STAX_TEST_TOKEN_1", "  my-token  ");
        assert_eq!(
            read_env_token("__STAX_TEST_TOKEN_1"),
            Some("my-token".to_string())
        );
        std::env::remove_var("__STAX_TEST_TOKEN_1");
    }

    #[test]
    fn test_read_env_token_returns_none_for_empty() {
        std::env::set_var("__STAX_TEST_TOKEN_2", "");
        assert_eq!(read_env_token("__STAX_TEST_TOKEN_2"), None);
        std::env::remove_var("__STAX_TEST_TOKEN_2");
    }

    #[test]
    fn test_read_env_token_returns_none_for_whitespace_only() {
        std::env::set_var("__STAX_TEST_TOKEN_3", "   ");
        assert_eq!(read_env_token("__STAX_TEST_TOKEN_3"), None);
        std::env::remove_var("__STAX_TEST_TOKEN_3");
    }

    #[test]
    fn test_read_env_token_returns_none_for_unset() {
        std::env::remove_var("__STAX_TEST_TOKEN_UNSET");
        assert_eq!(read_env_token("__STAX_TEST_TOKEN_UNSET"), None);
    }

    // --- forge_token fallback ---

    #[test]
    fn test_forge_token_gitlab_priority() {
        // Specific token takes priority over generic
        std::env::set_var("STAX_GITLAB_TOKEN", "gitlab-specific");
        std::env::set_var("GITLAB_TOKEN", "gitlab-generic");
        std::env::set_var("STAX_FORGE_TOKEN", "forge-fallback");
        assert_eq!(
            forge_token(ForgeType::GitLab),
            Some("gitlab-specific".to_string())
        );
        std::env::remove_var("STAX_GITLAB_TOKEN");
        std::env::remove_var("GITLAB_TOKEN");
        std::env::remove_var("STAX_FORGE_TOKEN");
    }

    #[test]
    fn test_forge_token_gitlab_falls_to_gitlab_token() {
        std::env::remove_var("STAX_GITLAB_TOKEN");
        std::env::set_var("GITLAB_TOKEN", "gitlab-generic");
        std::env::set_var("STAX_FORGE_TOKEN", "forge-fallback");
        assert_eq!(
            forge_token(ForgeType::GitLab),
            Some("gitlab-generic".to_string())
        );
        std::env::remove_var("GITLAB_TOKEN");
        std::env::remove_var("STAX_FORGE_TOKEN");
    }

    #[test]
    fn test_forge_token_gitlab_falls_to_stax_forge_token() {
        std::env::remove_var("STAX_GITLAB_TOKEN");
        std::env::remove_var("GITLAB_TOKEN");
        std::env::set_var("STAX_FORGE_TOKEN", "forge-fallback");
        assert_eq!(
            forge_token(ForgeType::GitLab),
            Some("forge-fallback".to_string())
        );
        std::env::remove_var("STAX_FORGE_TOKEN");
    }

    #[test]
    fn test_forge_token_gitea_priority() {
        std::env::set_var("STAX_GITEA_TOKEN", "gitea-specific");
        std::env::set_var("GITEA_TOKEN", "gitea-generic");
        std::env::set_var("STAX_FORGE_TOKEN", "forge-fallback");
        assert_eq!(
            forge_token(ForgeType::Gitea),
            Some("gitea-specific".to_string())
        );
        std::env::remove_var("STAX_GITEA_TOKEN");
        std::env::remove_var("GITEA_TOKEN");
        std::env::remove_var("STAX_FORGE_TOKEN");
    }

    #[test]
    fn test_forge_token_gitea_falls_to_gitea_token() {
        std::env::remove_var("STAX_GITEA_TOKEN");
        std::env::set_var("GITEA_TOKEN", "gitea-generic");
        std::env::set_var("STAX_FORGE_TOKEN", "forge-fallback");
        assert_eq!(
            forge_token(ForgeType::Gitea),
            Some("gitea-generic".to_string())
        );
        std::env::remove_var("GITEA_TOKEN");
        std::env::remove_var("STAX_FORGE_TOKEN");
    }

    // --- mergeable_bool ---

    #[test]
    fn test_mergeable_bool_indeterminate_states() {
        for state in &["checking", "unchecked", "preparing", "unknown"] {
            assert_eq!(mergeable_bool(state), None, "state: {}", state);
        }
    }

    #[test]
    fn test_mergeable_bool_truthy_states() {
        for state in &["mergeable", "can_be_merged", "clean"] {
            assert_eq!(mergeable_bool(state), Some(true), "state: {}", state);
        }
    }

    #[test]
    fn test_mergeable_bool_false_for_unknown_states() {
        // Unrecognized states are conservatively treated as false
        for state in &[
            "blocked",
            "conflict",
            "discussions_not_resolved",
            "dirty",
            "",
        ] {
            assert_eq!(mergeable_bool(state), Some(false), "state: {}", state);
        }
    }

    // --- ci_status_from_string ---

    #[test]
    fn test_ci_status_from_string_known_values() {
        assert_eq!(ci_status_from_string(Some("success")), CiStatus::Success);
        assert_eq!(ci_status_from_string(Some("pending")), CiStatus::Pending);
        assert_eq!(ci_status_from_string(Some("failure")), CiStatus::Failure);
        assert_eq!(ci_status_from_string(Some("error")), CiStatus::Failure);
    }

    #[test]
    fn test_ci_status_from_string_no_ci() {
        assert_eq!(ci_status_from_string(None), CiStatus::NoCi);
        assert_eq!(ci_status_from_string(Some("")), CiStatus::NoCi);
        assert_eq!(ci_status_from_string(Some("unknown")), CiStatus::NoCi);
    }

    #[test]
    fn test_ci_status_from_string_neutral_is_success() {
        // GitHub returns "neutral" for skipped/cancelled checks
        assert_eq!(ci_status_from_string(Some("neutral")), CiStatus::Success);
        assert_eq!(ci_status_from_string(Some("skipped")), CiStatus::Success);
    }
}
