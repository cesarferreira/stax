use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    aggregate_ci_overall, build_http_client, ci_status_from_string, delete_empty, get_json,
    make_issue_comment, mergeable_bool, post_json, put_json, stack_comment_body, AuthStyle,
    PrActivity, RepoIssueListItem, RepoPrListItem, ReviewActivity, STACK_COMMENT_MARKER,
};
use crate::ci::CheckRunInfo;
use crate::github::client::OpenPrInfo;
use crate::github::pr::{MergeMethod, PrComment, PrInfo, PrInfoWithHead, PrMergeStatus};
use crate::remote::{ForgeType, RemoteInfo};

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
    author: Option<GitLabUser>,
    created_at: Option<DateTime<Utc>>,
    merged_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
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

#[derive(Debug, Deserialize)]
struct GitLabIssue {
    iid: u64,
    title: String,
    web_url: Option<String>,
    author: Option<GitLabUser>,
    labels: Vec<String>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct GitLabApproval {
    user: GitLabUser,
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct GitLabApprovals {
    approved_by: Vec<GitLabApproval>,
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
struct AddToMergeTrainRequest {
    auto_merge: bool,
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
            "GitLab auth not configured. Use `stax auth` or set `STAX_GITLAB_TOKEN`, `GITLAB_TOKEN`, or `STAX_FORGE_TOKEN`.",
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
            "{}?state=opened&source_branch={}&per_page=100",
            self.project_url("/merge_requests"),
            encode_query_value(branch)
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
            &self.project_url("/merge_requests?state=opened&per_page=100"),
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
            &self.project_url(&format!("/merge_requests/{}/notes?per_page=100", number)),
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
            &self.project_url(&format!("/merge_requests/{}/notes?per_page=100", number)),
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

    /// Add an MR to GitLab's merge train.
    ///
    /// Uses `auto_merge: true` so the MR is scheduled to enter the merge train
    /// once its pipeline succeeds. Requires GitLab Premium/Ultimate and merge
    /// trains enabled in the project CI/CD settings.
    pub async fn add_to_merge_train(
        &self,
        number: u64,
    ) -> Result<crate::github::pr::EnqueueResult> {
        let request = AddToMergeTrainRequest {
            auto_merge: true,
        };
        let _: serde_json::Value = post_json(
            &self.client,
            &self.project_url(&format!("/merge_trains/merge_requests/{}", number)),
            &request,
        )
        .await
        .context(
            "Failed to add MR to merge train. Ensure merge trains are enabled \
             (GitLab Premium/Ultimate) and merge request pipelines are configured.",
        )?;
        Ok(crate::github::pr::EnqueueResult {
            merge_queue_entry: Some(crate::github::pr::MergeQueueEntry { position: None }),
        })
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
            &self.project_url(&format!(
                "/repository/commits/{}/statuses?per_page=100",
                sha
            )),
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
                conclusion: status.status.as_deref().map(normalize_gitlab_conclusion),
                url: status.target_url.clone(),
                started_at: status.started_at.clone(),
                completed_at: status.finished_at.clone(),
                elapsed_secs: None,
                average_secs: None,
                completion_percent: None,
            })
            .collect::<Vec<_>>();

        let overall = aggregate_ci_overall(
            statuses
                .iter()
                .filter_map(|status| status.status.as_deref()),
            |s| matches!(s, "failed" | "canceled"),
            |s| matches!(s, "running" | "pending" | "created"),
        );

        Ok((overall, checks))
    }

    pub async fn list_open_pull_requests(&self, limit: u8) -> Result<Vec<RepoPrListItem>> {
        let per_page = limit.clamp(1, 100);
        let url = format!(
            "{}?state=opened&per_page={}&order_by=created_at&sort=desc",
            self.project_url("/merge_requests"),
            per_page
        );
        let mrs: Vec<GitLabMr> = get_json(&self.client, &url).await?;
        Ok(mrs
            .into_iter()
            .map(|mr| RepoPrListItem {
                number: mr.iid,
                title: mr.title,
                url: mr.web_url.unwrap_or_default(),
                author: mr
                    .author
                    .map(|a| a.username)
                    .unwrap_or_else(|| "unknown".to_string()),
                head_branch: mr.source_branch,
                base_branch: mr.target_branch,
                state: normalize_gitlab_state(&mr.state),
                is_draft: mr.draft,
                created_at: mr.created_at.unwrap_or_default(),
            })
            .collect())
    }

    pub async fn list_open_issues(&self, limit: u8) -> Result<Vec<RepoIssueListItem>> {
        let per_page = limit.clamp(1, 100);
        let url = format!(
            "{}?state=opened&per_page={}&order_by=updated_at&sort=desc",
            self.project_url("/issues"),
            per_page
        );
        let issues: Vec<GitLabIssue> = get_json(&self.client, &url).await?;
        Ok(issues
            .into_iter()
            .map(|issue| RepoIssueListItem {
                number: issue.iid,
                title: issue.title,
                url: issue.web_url.unwrap_or_default(),
                author: issue
                    .author
                    .map(|a| a.username)
                    .unwrap_or_else(|| "unknown".to_string()),
                labels: issue.labels,
                updated_at: issue.updated_at,
            })
            .collect())
    }

    pub async fn get_current_user(&self) -> Result<String> {
        let url = format!("{}/user", self.api_base_url);
        let user: GitLabUser = get_json(&self.client, &url).await?;
        Ok(user.username)
    }

    pub async fn get_user_open_prs(&self, username: &str) -> Result<Vec<OpenPrInfo>> {
        let url = format!(
            "{}?state=opened&author_username={}&per_page=100",
            self.project_url("/merge_requests"),
            encode_query_value(username)
        );
        let mrs: Vec<GitLabMr> = get_json(&self.client, &url).await?;
        Ok(mrs
            .into_iter()
            .map(|mr| OpenPrInfo {
                number: mr.iid,
                head_branch: mr.source_branch,
                base_branch: mr.target_branch,
                state: normalize_gitlab_state(&mr.state),
                is_draft: mr.draft,
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
            "{}?state=merged&author_username={}&updated_after={}&per_page=30&order_by=updated_at&sort=desc",
            self.project_url("/merge_requests"),
            encode_query_value(username),
            since.to_rfc3339()
        );
        let mrs: Vec<GitLabMr> = get_json(&self.client, &url).await?;
        Ok(mrs
            .into_iter()
            .filter_map(|mr| {
                let ts = mr.merged_at.or(mr.updated_at)?;
                if ts < since {
                    return None;
                }
                Some(PrActivity {
                    number: mr.iid,
                    title: mr.title,
                    timestamp: ts,
                    url: mr.web_url.unwrap_or_default(),
                })
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
            "{}?author_username={}&created_after={}&per_page=30&order_by=created_at&sort=desc",
            self.project_url("/merge_requests"),
            encode_query_value(username),
            since.to_rfc3339()
        );
        let mrs: Vec<GitLabMr> = get_json(&self.client, &url).await?;
        Ok(mrs
            .into_iter()
            .filter_map(|mr| {
                let ts = mr.created_at?;
                if ts < since {
                    return None;
                }
                Some(PrActivity {
                    number: mr.iid,
                    title: mr.title,
                    timestamp: ts,
                    url: mr.web_url.unwrap_or_default(),
                })
            })
            .collect())
    }

    pub async fn get_reviews_received(
        &self,
        hours: i64,
        username: &str,
    ) -> Result<Vec<ReviewActivity>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        let url = format!(
            "{}?state=opened&author_username={}&per_page=20",
            self.project_url("/merge_requests"),
            encode_query_value(username)
        );
        let mrs: Vec<GitLabMr> = get_json(&self.client, &url).await?;

        let mut reviews = Vec::new();
        for mr in mrs {
            let approvals_url = self.project_url(&format!("/merge_requests/{}/approvals", mr.iid));
            let approvals: GitLabApprovals = match get_json(&self.client, &approvals_url).await {
                Ok(a) => a,
                Err(_) => continue,
            };
            for approval in approvals.approved_by {
                if approval.user.username == username {
                    continue; // skip self-approvals
                }
                let Some(ts) = approval.created_at else {
                    continue;
                };
                if ts >= since {
                    reviews.push(ReviewActivity {
                        pr_number: mr.iid,
                        pr_title: mr.title.clone(),
                        reviewer: approval.user.username,
                        state: "APPROVED".to_string(),
                        timestamp: ts,
                        is_received: true,
                    });
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
        // Not implemented: GitLab has no efficient way to query "reviews given by user"
        // across all MRs. Would require iterating all open MRs and checking approvals.
        Ok(vec![])
    }
}

/// Percent-encode a value for use in a URL query parameter.
fn encode_query_value(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            // Unreserved characters (RFC 3986 section 2.3)
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                // write! into a String is infallible
                let _ = write!(encoded, "%{:02X}", byte);
            }
        }
    }
    encoded
}

fn normalize_gitlab_check_status(status: Option<&str>) -> String {
    match status.unwrap_or("") {
        "running" | "pending" | "created" => "in_progress".to_string(),
        _ => "completed".to_string(),
    }
}

fn normalize_gitlab_conclusion(status: &str) -> String {
    match status {
        "success" => "success".to_string(),
        "failed" => "failure".to_string(),
        "canceled" => "cancelled".to_string(),
        _ => status.to_string(),
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

    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

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

    #[tokio::test]
    async fn test_list_open_prs_by_head() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
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
                }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let prs = client.list_open_prs_by_head().await.unwrap();
        let pr = prs.get("feature-a").unwrap();
        assert_eq!(pr.info.number, 7);
        assert_eq!(pr.info.state, "OPEN");
        assert_eq!(pr.info.base, "main");
    }

    #[tokio::test]
    async fn test_fetch_checks_maps_gitlab_statuses() {
        ensure_crypto_provider();
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
    async fn test_list_open_pull_requests() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("order_by", "created_at"))
            .and(query_param("sort", "desc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "iid": 10,
                    "title": "Add feature X",
                    "state": "opened",
                    "draft": true,
                    "source_branch": "feature-x",
                    "target_branch": "main",
                    "description": "body",
                    "web_url": "https://gitlab.example.com/group/subgroup/repo/-/merge_requests/10",
                    "author": { "username": "alice" },
                    "created_at": "2024-06-01T12:00:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let prs = client.list_open_pull_requests(30).await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 10);
        assert_eq!(prs[0].title, "Add feature X");
        assert_eq!(prs[0].author, "alice");
        assert_eq!(prs[0].head_branch, "feature-x");
        assert_eq!(prs[0].base_branch, "main");
        assert!(prs[0].is_draft);
        assert_eq!(prs[0].state, "OPEN");
    }

    #[tokio::test]
    async fn test_list_open_issues() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/issues"))
            .and(query_param("state", "opened"))
            .and(query_param("order_by", "updated_at"))
            .and(query_param("sort", "desc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "iid": 42,
                    "title": "Bug in login",
                    "web_url": "https://gitlab.example.com/group/subgroup/repo/-/issues/42",
                    "author": { "username": "bob" },
                    "labels": ["bug", "urgent"],
                    "updated_at": "2024-06-15T08:30:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let issues = client.list_open_issues(30).await.unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 42);
        assert_eq!(issues[0].title, "Bug in login");
        assert_eq!(issues[0].author, "bob");
        assert_eq!(issues[0].labels, vec!["bug", "urgent"]);
    }

    #[tokio::test]
    async fn test_get_current_user() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path("/user"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "username": "alice" })),
            )
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let user = client.get_current_user().await.unwrap();
        assert_eq!(user, "alice");
    }

    #[tokio::test]
    async fn test_get_user_open_prs() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("author_username", "alice"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "iid": 15,
                    "title": "My MR",
                    "state": "opened",
                    "draft": false,
                    "source_branch": "feature-y",
                    "target_branch": "main",
                    "description": "desc"
                }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let prs = client.get_user_open_prs("alice").await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 15);
        assert_eq!(prs[0].head_branch, "feature-y");
        assert_eq!(prs[0].base_branch, "main");
        assert_eq!(prs[0].state, "OPEN");
    }

    #[tokio::test]
    async fn test_get_recent_merged_prs() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .and(query_param("state", "merged"))
            .and(query_param("author_username", "alice"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "iid": 20,
                    "title": "Merged MR",
                    "state": "merged",
                    "draft": false,
                    "source_branch": "feat-z",
                    "target_branch": "main",
                    "web_url": "https://gitlab.example.com/g/s/r/-/merge_requests/20",
                    "merged_at": "2099-01-01T12:00:00Z",
                    "updated_at": "2099-01-01T12:00:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let prs = client.get_recent_merged_prs(9999, "alice").await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 20);
        assert_eq!(prs[0].title, "Merged MR");
    }

    #[tokio::test]
    async fn test_get_recent_opened_prs() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .and(query_param("author_username", "alice"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "iid": 21,
                    "title": "New MR",
                    "state": "opened",
                    "draft": false,
                    "source_branch": "feat-new",
                    "target_branch": "main",
                    "web_url": "https://gitlab.example.com/g/s/r/-/merge_requests/21",
                    "created_at": "2099-01-01T10:00:00Z"
                }
            ])))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let prs = client.get_recent_opened_prs(9999, "alice").await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 21);
        assert_eq!(prs[0].title, "New MR");
    }

    #[tokio::test]
    async fn test_get_reviews_received() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        std::env::set_var("STAX_GITLAB_TOKEN", "test-token");

        // Mock: user's open MRs
        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path("/projects/group%2Fsubgroup%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("author_username", "alice"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "iid": 30,
                    "title": "My MR",
                    "state": "opened",
                    "draft": false,
                    "source_branch": "feat",
                    "target_branch": "main"
                }
            ])))
            .mount(&server)
            .await;

        // Mock: approvals on MR 30
        Mock::given(method("GET"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .and(path(
                "/projects/group%2Fsubgroup%2Frepo/merge_requests/30/approvals",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "approved_by": [
                    {
                        "user": { "username": "bob" },
                        "created_at": "2099-01-01T09:00:00Z"
                    }
                ]
            })))
            .mount(&server)
            .await;

        let client = GitLabClient::new(&remote_info(&server)).unwrap();
        let reviews = client.get_reviews_received(9999, "alice").await.unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].reviewer, "bob");
        assert_eq!(reviews[0].state, "APPROVED");
        assert_eq!(reviews[0].pr_number, 30);
        assert!(reviews[0].is_received);
    }
}
