use anyhow::{bail, Result};
use std::collections::HashMap;

use crate::ci::CheckRunInfo;
use crate::github::client::{GitHubClient, OpenPrInfo};
use crate::github::pr::{MergeMethod, PrComment, PrInfo, PrInfoWithHead, PrMergeStatus};
use crate::remote::{ForgeType, RemoteInfo};

/// HTML comment marker embedded in stack comments to identify them for updates/deletion.
pub(crate) const STACK_COMMENT_MARKER: &str = "<!-- stax-stack-comment -->";

pub fn stack_comment_body(stack_comment: &str) -> String {
    format!("{}\n{}", STACK_COMMENT_MARKER, stack_comment)
}

/// Dispatch an async method call uniformly across all forge variants.
macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* $(,)? )) => {
        match $self {
            Self::GitHub(c) => c.$method($($arg),*).await,
        }
    };
}

#[derive(Clone)]
pub enum ForgeClient {
    GitHub(GitHubClient),
}

impl ForgeClient {
    pub fn new(remote: &RemoteInfo) -> Result<Self> {
        match remote.forge {
            ForgeType::GitHub => Ok(Self::GitHub(GitHubClient::new(
                remote.owner(),
                &remote.repo,
                remote.api_base_url.clone(),
            )?)),
            ForgeType::GitLab | ForgeType::Gitea => {
                bail!(
                    "Forge type {:?} is not yet supported. Only GitHub is currently implemented.",
                    remote.forge
                )
            }
        }
    }

    pub fn api_call_stats(&self) -> Option<crate::github::client::ApiCallStats> {
        match self {
            Self::GitHub(client) => Some(client.api_call_stats()),
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
        _sha: Option<&str>,
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
        }
    }

    pub async fn request_reviewers(&self, number: u64, reviewers: &[String]) -> Result<()> {
        dispatch!(self, request_reviewers(number, reviewers))
    }

    pub async fn get_requested_reviewers(&self, number: u64) -> Result<Vec<String>> {
        dispatch!(self, get_requested_reviewers(number))
    }

    pub async fn add_labels(&self, number: u64, labels: &[String]) -> Result<()> {
        dispatch!(self, add_labels(number, labels))
    }

    pub async fn add_assignees(&self, number: u64, assignees: &[String]) -> Result<()> {
        dispatch!(self, add_assignees(number, assignees))
    }

    pub async fn get_current_user(&self) -> Result<String> {
        dispatch!(self, get_current_user())
    }

    pub async fn get_user_open_prs(&self, username: &str) -> Result<Vec<OpenPrInfo>> {
        dispatch!(self, get_user_open_prs(username))
    }
}
