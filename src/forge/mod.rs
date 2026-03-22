use anyhow::{bail, Result};
use std::collections::HashMap;

use crate::ci::CheckRunInfo;
use crate::github::client::{GitHubClient, OpenPrInfo};
use crate::github::pr::{MergeMethod, PrComment, PrInfo, PrInfoWithHead, PrMergeStatus};
use crate::remote::{ForgeType, RemoteInfo};

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

    pub async fn find_open_pr_by_head(
        &self,
        head_owner: &str,
        branch: &str,
    ) -> Result<Option<PrInfoWithHead>> {
        match self {
            Self::GitHub(client) => client.find_open_pr_by_head(head_owner, branch).await,
        }
    }

    pub async fn find_pr(&self, branch: &str) -> Result<Option<PrInfo>> {
        match self {
            Self::GitHub(client) => client.find_pr(branch).await,
        }
    }

    pub async fn list_open_prs_by_head(&self) -> Result<HashMap<String, PrInfoWithHead>> {
        match self {
            Self::GitHub(client) => client.list_open_prs_by_head().await,
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
        }
    }

    pub async fn get_pr(&self, number: u64) -> Result<PrInfo> {
        match self {
            Self::GitHub(client) => client.get_pr(number).await,
        }
    }

    pub async fn get_pr_with_head(&self, number: u64) -> Result<PrInfoWithHead> {
        match self {
            Self::GitHub(client) => client.get_pr_with_head(number).await,
        }
    }

    pub async fn update_pr_base(&self, number: u64, new_base: &str) -> Result<()> {
        match self {
            Self::GitHub(client) => client.update_pr_base(number, new_base).await,
        }
    }

    pub async fn update_pr_body(&self, number: u64, body: &str) -> Result<()> {
        match self {
            Self::GitHub(client) => client.update_pr_body(number, body).await,
        }
    }

    pub async fn get_pr_body(&self, number: u64) -> Result<String> {
        match self {
            Self::GitHub(client) => client.get_pr_body(number).await,
        }
    }

    pub async fn update_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()> {
        match self {
            Self::GitHub(client) => client.update_stack_comment(number, stack_comment).await,
        }
    }

    pub async fn create_stack_comment(&self, number: u64, stack_comment: &str) -> Result<()> {
        match self {
            Self::GitHub(client) => client.create_stack_comment(number, stack_comment).await,
        }
    }

    pub async fn delete_stack_comment(&self, number: u64) -> Result<()> {
        match self {
            Self::GitHub(client) => client.delete_stack_comment(number).await,
        }
    }

    pub async fn list_all_comments(&self, number: u64) -> Result<Vec<PrComment>> {
        match self {
            Self::GitHub(client) => client.list_all_comments(number).await,
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
        }
    }

    pub async fn get_pr_merge_status(&self, number: u64) -> Result<PrMergeStatus> {
        match self {
            Self::GitHub(client) => client.get_pr_merge_status(number).await,
        }
    }

    pub async fn is_pr_merged(&self, number: u64) -> Result<bool> {
        match self {
            Self::GitHub(client) => client.is_pr_merged(number).await,
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
        }
    }

    pub async fn request_reviewers(&self, number: u64, reviewers: &[String]) -> Result<()> {
        match self {
            Self::GitHub(client) => client.request_reviewers(number, reviewers).await,
        }
    }

    pub async fn get_requested_reviewers(&self, number: u64) -> Result<Vec<String>> {
        match self {
            Self::GitHub(client) => client.get_requested_reviewers(number).await,
        }
    }

    pub async fn add_labels(&self, number: u64, labels: &[String]) -> Result<()> {
        match self {
            Self::GitHub(client) => client.add_labels(number, labels).await,
        }
    }

    pub async fn add_assignees(&self, number: u64, assignees: &[String]) -> Result<()> {
        match self {
            Self::GitHub(client) => client.add_assignees(number, assignees).await,
        }
    }

    pub async fn get_current_user(&self) -> Result<String> {
        match self {
            Self::GitHub(client) => client.get_current_user().await,
        }
    }

    pub async fn get_user_open_prs(&self, username: &str) -> Result<Vec<OpenPrInfo>> {
        match self {
            Self::GitHub(client) => client.get_user_open_prs(username).await,
        }
    }
}
