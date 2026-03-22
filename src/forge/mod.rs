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
};
use crate::remote::{ForgeType, RemoteInfo};

mod gitea;
mod gitlab;

use gitea::GiteaClient;
use gitlab::GitLabClient;

/// HTML comment marker embedded in stack comments to identify them for updates/deletion.
pub(crate) const STACK_COMMENT_MARKER: &str = "<!-- stax-stack-comment -->";

pub fn stack_comment_body(stack_comment: &str) -> String {
    format!("{}\n{}", STACK_COMMENT_MARKER, stack_comment)
}

#[derive(Clone, Copy)]
pub enum AuthStyle {
    AuthorizationToken,
    PrivateToken,
}

/// Dispatch an async method call uniformly across all forge variants.
macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* $(,)? )) => {
        match $self {
            Self::GitHub(c) => c.$method($($arg),*).await,
            Self::GitLab(c) => c.$method($($arg),*).await,
            Self::Gitea(c) => c.$method($($arg),*).await,
        }
    };
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

    /// Find an open PR by head branch.
    ///
    /// GitHub uses the stored owner for fork-aware lookup; other forges
    /// filter by source branch only.
    pub async fn find_open_pr_by_head(
        &self,
        branch: &str,
    ) -> Result<Option<PrInfoWithHead>> {
        match self {
            Self::GitHub(client) => client.find_open_pr_by_head(&client.owner, branch).await,
            Self::GitLab(client) => client.find_open_pr_by_head(branch).await,
            Self::Gitea(client) => client.find_open_pr_by_head(branch).await,
        }
    }

    pub async fn find_pr(&self, branch: &str) -> Result<Option<PrInfo>> {
        dispatch!(self, find_pr(branch))
    }

    pub async fn list_open_prs_by_head(&self) -> Result<HashMap<String, PrInfoWithHead>> {
        dispatch!(self, list_open_prs_by_head())
    }

    pub async fn create_pr(
        &self,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
        is_draft: bool,
    ) -> Result<PrInfo> {
        dispatch!(self, create_pr(head, base, title, body, is_draft))
    }

    pub async fn get_pr(&self, number: u64) -> Result<PrInfo> {
        dispatch!(self, get_pr(number))
    }

    pub async fn get_pr_with_head(&self, number: u64) -> Result<PrInfoWithHead> {
        dispatch!(self, get_pr_with_head(number))
    }

    pub async fn update_pr_base(&self, number: u64, new_base: &str) -> Result<()> {
        dispatch!(self, update_pr_base(number, new_base))
    }

    pub async fn update_pr_body(&self, number: u64, body: &str) -> Result<()> {
        dispatch!(self, update_pr_body(number, body))
    }

    pub async fn get_pr_body(&self, number: u64) -> Result<String> {
        dispatch!(self, get_pr_body(number))
    }

    pub async fn update_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()> {
        dispatch!(self, update_stack_comment(number, stack_comment))
    }

    pub async fn create_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()> {
        dispatch!(self, create_stack_comment(number, stack_comment))
    }

    pub async fn delete_stack_comment(&self, number: u64) -> Result<()> {
        dispatch!(self, delete_stack_comment(number))
    }

    pub async fn list_all_comments(&self, number: u64) -> Result<Vec<PrComment>> {
        dispatch!(self, list_all_comments(number))
    }

    pub async fn merge_pr(
        &self,
        number: u64,
        method: MergeMethod,
        commit_title: Option<&str>,
        sha: Option<&str>,
    ) -> Result<()> {
        match self {
            // GitHub's merge_pr takes (number, method, commit_title, commit_message).
            // The `sha` merge-guard is not exposed by the current GitHub client,
            // so we pass None for commit_message rather than forwarding sha there.
            Self::GitHub(client) => {
                client
                    .merge_pr(number, method, commit_title.map(str::to_string), None)
                    .await
            }
            Self::GitLab(client) => client.merge_pr(number, method, commit_title, sha).await,
            Self::Gitea(client) => client.merge_pr(number, method, commit_title, sha).await,
        }
    }

    pub async fn get_pr_merge_status(&self, number: u64) -> Result<PrMergeStatus> {
        dispatch!(self, get_pr_merge_status(number))
    }

    pub async fn is_pr_merged(&self, number: u64) -> Result<bool> {
        dispatch!(self, is_pr_merged(number))
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
    Client::builder()
        .default_headers(base_headers(token, auth_style)?)
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60))
        .build()
        .context("Failed to build forge HTTP client")
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

/// Aggregate individual CI statuses into one overall result.
/// Scans all statuses so that failure always takes priority over pending.
fn aggregate_ci_overall<'a>(
    statuses: impl Iterator<Item = &'a str>,
    is_failure: impl Fn(&str) -> bool,
    is_pending: impl Fn(&str) -> bool,
) -> Option<String> {
    let mut has_any = false;
    let mut has_failure = false;
    let mut has_pending = false;
    for status in statuses {
        has_any = true;
        if is_failure(status) {
            has_failure = true;
        } else if is_pending(status) {
            has_pending = true;
        }
    }
    if has_failure {
        Some("failure".to_string())
    } else if has_pending {
        Some("pending".to_string())
    } else if has_any {
        Some("success".to_string())
    } else {
        None
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

#[cfg(test)]
mod tests {
    use super::*;

    fn is_failure(s: &str) -> bool {
        matches!(s, "failed" | "canceled" | "failure" | "error")
    }
    fn is_pending(s: &str) -> bool {
        matches!(s, "running" | "pending" | "created")
    }

    #[test]
    fn aggregate_ci_failure_takes_priority_over_pending() {
        let statuses = ["pending", "failed"];
        let result = aggregate_ci_overall(statuses.iter().copied(), is_failure, is_pending);
        assert_eq!(result.as_deref(), Some("failure"));
    }

    #[test]
    fn aggregate_ci_pending_before_failure_still_reports_failure() {
        let statuses = ["running", "success", "failed"];
        let result = aggregate_ci_overall(statuses.iter().copied(), is_failure, is_pending);
        assert_eq!(result.as_deref(), Some("failure"));
    }

    #[test]
    fn aggregate_ci_all_success() {
        let statuses = ["success", "success"];
        let result = aggregate_ci_overall(statuses.iter().copied(), is_failure, is_pending);
        assert_eq!(result.as_deref(), Some("success"));
    }

    #[test]
    fn aggregate_ci_pending_only() {
        let statuses = ["success", "running"];
        let result = aggregate_ci_overall(statuses.iter().copied(), is_failure, is_pending);
        assert_eq!(result.as_deref(), Some("pending"));
    }

    #[test]
    fn aggregate_ci_empty_returns_none() {
        let statuses: [&str; 0] = [];
        let result = aggregate_ci_overall(statuses.iter().copied(), is_failure, is_pending);
        assert_eq!(result, None);
    }
}
