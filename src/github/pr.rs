use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::params::pulls::Sort;
use octocrab::params::State;
use serde::Deserialize;
use std::collections::HashMap;

use super::GitHubClient;
use crate::remote::{ForgeType, RemoteInfo};

const STACK_COMMENT_MARKER: &str = "<!-- stax-stack-comment -->";
const STACK_LINKS_BODY_START_MARKER: &str = "<!-- stax-stack-links:start -->";
const STACK_LINKS_BODY_END_MARKER: &str = "<!-- stax-stack-links:end -->";

/// A comment on a PR issue thread (conversation comment)
#[derive(Debug, Clone)]
pub struct IssueComment {
    #[allow(dead_code)]
    pub id: u64,
    pub body: String,
    pub user: String,
    pub created_at: DateTime<Utc>,
}

/// A review comment on a PR (inline code comment)
#[derive(Debug, Clone)]
pub struct ReviewComment {
    #[allow(dead_code)]
    pub id: u64,
    pub body: String,
    pub user: String,
    pub path: String,
    pub line: Option<u32>,
    pub start_line: Option<u32>,
    pub created_at: DateTime<Utc>,
    pub diff_hunk: Option<String>,
}

/// Combined comment for unified display
#[derive(Debug, Clone)]
pub enum PrComment {
    Issue(IssueComment),
    Review(ReviewComment),
}

#[derive(Debug, Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct ApiIssueComment {
    id: u64,
    body: Option<String>,
    user: ApiUser,
    created_at: DateTime<Utc>,
}

impl PrComment {
    pub fn created_at(&self) -> DateTime<Utc> {
        match self {
            PrComment::Issue(c) => c.created_at,
            PrComment::Review(c) => c.created_at,
        }
    }

    #[allow(dead_code)]
    pub fn user(&self) -> &str {
        match self {
            PrComment::Issue(c) => &c.user,
            PrComment::Review(c) => &c.user,
        }
    }

    #[allow(dead_code)]
    pub fn body(&self) -> &str {
        match self {
            PrComment::Issue(c) => &c.body,
            PrComment::Review(c) => &c.body,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrInfo {
    pub number: u64,
    pub state: String,
    pub is_draft: bool,
    pub base: String,
}

#[derive(Debug, Clone)]
pub struct PrInfoWithHead {
    pub info: PrInfo,
    pub head: String,
    pub head_label: Option<String>,
    pub title: String,
}

/// Merge method for PRs
#[derive(Debug, Clone, Copy, Default)]
pub enum MergeMethod {
    #[default]
    Squash,
    Merge,
    Rebase,
}

impl MergeMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            MergeMethod::Squash => "squash",
            MergeMethod::Merge => "merge",
            MergeMethod::Rebase => "rebase",
        }
    }
}

impl std::str::FromStr for MergeMethod {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "squash" => Ok(MergeMethod::Squash),
            "merge" => Ok(MergeMethod::Merge),
            "rebase" => Ok(MergeMethod::Rebase),
            _ => anyhow::bail!("Invalid merge method: {}. Use: squash, merge, or rebase", s),
        }
    }
}

/// CI check status
#[derive(Debug, Clone, PartialEq)]
pub enum CiStatus {
    Pending,
    Success,
    Failure,
    /// No CI checks configured - treat as passing
    NoCi,
}

impl CiStatus {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "success" => CiStatus::Success,
            "pending" => CiStatus::Pending,
            "failure" | "error" => CiStatus::Failure,
            // GitHub returns "neutral" for skipped/cancelled checks - treat as success
            "neutral" | "skipped" | "cancelled" => CiStatus::Success,
            // Empty or unknown typically means no CI configured
            "" | "none" | "unknown" => CiStatus::NoCi,
            // Default: no CI configured (don't block on unrecognized states)
            _ => CiStatus::NoCi,
        }
    }

    pub fn is_success(&self) -> bool {
        // NoCi is treated as success (nothing to wait for)
        matches!(self, CiStatus::Success | CiStatus::NoCi)
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, CiStatus::Pending)
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, CiStatus::Failure)
    }

    #[allow(dead_code)]
    pub fn display_text(&self) -> &'static str {
        match self {
            CiStatus::Success => "passed",
            CiStatus::Pending => "running",
            CiStatus::Failure => "failed",
            CiStatus::NoCi => "no checks",
        }
    }
}

/// Detailed PR merge status
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PrMergeStatus {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub is_draft: bool,
    pub mergeable: Option<bool>,
    pub mergeable_state: String,
    pub ci_status: CiStatus,
    pub review_decision: Option<String>,
    pub approvals: usize,
    pub changes_requested: bool,
    pub head_sha: String,
}

impl PrMergeStatus {
    /// Check if PR is ready to merge (approved + CI passed + mergeable)
    pub fn is_ready(&self) -> bool {
        self.ci_status.is_success()
            && !self.is_draft
            && self.mergeable.unwrap_or(false)
            && !self.changes_requested
            && self.state.to_lowercase() == "open"
    }

    /// Check if PR is waiting (CI pending or mergeable computing)
    pub fn is_waiting(&self) -> bool {
        self.ci_status.is_pending() || self.mergeable.is_none()
    }

    /// Check if PR has a blocking issue
    pub fn is_blocked(&self) -> bool {
        self.ci_status.is_failure()
            || self.changes_requested
            || self.is_draft
            || self.mergeable == Some(false)
    }

    /// Get human-readable status
    pub fn status_text(&self) -> &'static str {
        if self.state.to_lowercase() != "open" {
            return "Closed";
        }
        if self.is_draft {
            return "Draft";
        }
        if self.changes_requested {
            return "Changes requested";
        }
        if self.ci_status.is_failure() {
            return "CI failed";
        }
        if self.mergeable == Some(false) {
            return "Has conflicts";
        }
        if self.is_waiting() {
            return "Waiting";
        }
        if self.is_ready() {
            return "Ready";
        }
        "Ready" // Default to ready if nothing is blocking
    }
}

/// Response from GitHub GraphQL API for PR reviews
#[derive(Debug, Deserialize)]
struct GraphQLResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct PrReviewData {
    repository: Option<RepositoryData>,
}

// --- Merge queue (enqueuePullRequest) GraphQL types ---

#[derive(Debug, Deserialize)]
struct PrNodeIdData {
    repository: Option<PrNodeIdRepo>,
}

#[derive(Debug, Deserialize)]
struct PrNodeIdRepo {
    #[serde(rename = "pullRequest")]
    pull_request: Option<PrNodeId>,
}

#[derive(Debug, Deserialize)]
struct PrNodeId {
    id: String,
}

#[derive(Debug, Deserialize)]
struct EnqueueData {
    #[serde(rename = "enqueuePullRequest")]
    enqueue_pull_request: Option<EnqueueResult>,
}

#[derive(Debug, Deserialize)]
pub struct EnqueueResult {
    #[serde(rename = "mergeQueueEntry")]
    pub merge_queue_entry: Option<MergeQueueEntry>,
}

#[derive(Debug, Deserialize)]
pub struct MergeQueueEntry {
    pub position: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct RepositoryData {
    #[serde(rename = "pullRequest")]
    pull_request: Option<PullRequestData>,
}

#[derive(Debug, Deserialize)]
struct PullRequestData {
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
    reviews: ReviewConnection,
}

#[derive(Debug, Deserialize)]
struct ReviewConnection {
    nodes: Vec<ReviewNode>,
}

#[derive(Debug, Deserialize)]
struct ReviewNode {
    state: String,
}

impl GitHubClient {
    /// Find existing open PR for a branch owned by `head_owner`.
    ///
    /// Uses GitHub's `head` filter first (single request) and validates the
    /// result matches the exact branch name.
    pub async fn find_open_pr_by_head(
        &self,
        head_owner: &str,
        branch: &str,
    ) -> Result<Option<PrInfoWithHead>> {
        self.record_api_call("pulls.list.head");
        let prs = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .list()
            .state(State::Open)
            .head(format!("{}:{}", head_owner, branch))
            .per_page(100u8)
            .sort(Sort::Created)
            .send()
            .await
            .context("Failed to list PRs by head")?;

        for pr in &prs.items {
            if pr.head.ref_field != branch {
                continue;
            }
            let owner_matches = pr
                .head
                .label
                .as_ref()
                .and_then(|label| label.split_once(':').map(|(owner, _)| owner == head_owner))
                .unwrap_or(true);
            if !owner_matches {
                continue;
            }

            return Ok(Some(PrInfoWithHead {
                head_label: pr.head.label.clone(),
                title: pr.title.clone().unwrap_or_default(),
                info: PrInfo {
                    number: pr.number,
                    state: pr
                        .state
                        .as_ref()
                        .map(|s| format!("{:?}", s))
                        .unwrap_or_default(),
                    is_draft: pr.draft.unwrap_or(false),
                    base: pr.base.ref_field.clone(),
                },
                head: pr.head.ref_field.clone(),
            }));
        }

        Ok(None)
    }

    /// Find existing open PR for a branch
    ///
    /// Only returns a PR if:
    /// 1. The PR is in OPEN state (not closed or merged)
    /// 2. The PR's head branch exactly matches the requested branch name
    ///
    /// Uses the `head` filter first (fast path), then falls back to scanning
    /// open PRs if needed.
    pub async fn find_pr(&self, branch: &str) -> Result<Option<PrInfo>> {
        if let Some(pr) = self.find_open_pr_by_head(&self.owner, branch).await? {
            return Ok(Some(pr.info));
        }

        let prs_by_head = self.list_open_prs_by_head().await?;
        Ok(prs_by_head.get(branch).cloned().map(|pr| pr.info))
    }

    /// List all open PRs and index them by head branch name
    pub async fn list_open_prs_by_head(&self) -> Result<HashMap<String, PrInfoWithHead>> {
        let mut page = 1u32;
        const PER_PAGE: u8 = 100;
        let mut prs_by_head = HashMap::new();

        loop {
            self.record_api_call("pulls.list.open.page");
            let prs = self
                .octocrab
                .pulls(&self.owner, &self.repo)
                .list()
                .state(State::Open)
                .per_page(PER_PAGE)
                .page(page)
                .sort(Sort::Created)
                .send()
                .await
                .context("Failed to list PRs")?;

            for pr in &prs.items {
                let head = pr.head.ref_field.clone();
                if prs_by_head.contains_key(&head) {
                    continue;
                }

                prs_by_head.insert(
                    head,
                    PrInfoWithHead {
                        head_label: pr.head.label.clone(),
                        title: pr.title.clone().unwrap_or_default(),
                        info: PrInfo {
                            number: pr.number,
                            state: pr
                                .state
                                .as_ref()
                                .map(|s| format!("{:?}", s))
                                .unwrap_or_default(),
                            is_draft: pr.draft.unwrap_or(false),
                            base: pr.base.ref_field.clone(),
                        },
                        head: pr.head.ref_field.clone(),
                    },
                );
            }

            if (prs.items.len() as u8) < PER_PAGE {
                break;
            }

            page += 1;
        }

        Ok(prs_by_head)
    }

    /// Create a new PR
    pub async fn create_pr(
        &self,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<PrInfo> {
        self.record_api_call("pulls.create");
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .create(title, branch, base)
            .body(body)
            .draft(Some(draft))
            .send()
            .await
            .context("Failed to create PR")?;

        Ok(PrInfo {
            number: pr.number,
            state: pr
                .state
                .as_ref()
                .map(|s| format!("{:?}", s))
                .unwrap_or_default(),
            is_draft: pr.draft.unwrap_or(false),
            base: pr.base.ref_field.clone(),
        })
    }

    /// Get a PR by number
    pub async fn get_pr(&self, pr_number: u64) -> Result<PrInfo> {
        self.record_api_call("pulls.get");
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .get(pr_number)
            .await
            .context("Failed to get PR")?;

        Ok(PrInfo {
            number: pr.number,
            state: pr
                .state
                .as_ref()
                .map(|s| format!("{:?}", s))
                .unwrap_or_default(),
            is_draft: pr.draft.unwrap_or(false),
            base: pr.base.ref_field.clone(),
        })
    }

    /// Get a PR by number, including head branch name
    pub async fn get_pr_with_head(&self, pr_number: u64) -> Result<PrInfoWithHead> {
        self.record_api_call("pulls.get");
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .get(pr_number)
            .await
            .context("Failed to get PR")?;

        Ok(PrInfoWithHead {
            head: pr.head.ref_field.clone(),
            head_label: pr.head.label.clone(),
            title: pr.title.clone().unwrap_or_default(),
            info: PrInfo {
                number: pr.number,
                state: pr
                    .state
                    .as_ref()
                    .map(|s| format!("{:?}", s))
                    .unwrap_or_default(),
                is_draft: pr.draft.unwrap_or(false),
                base: pr.base.ref_field.clone(),
            },
        })
    }

    /// Update PR base branch
    pub async fn update_pr_base(&self, pr_number: u64, new_base: &str) -> Result<()> {
        self.record_api_call("pulls.update.base");
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .update(pr_number)
            .base(new_base)
            .send()
            .await
            .context("Failed to update PR base")?;
        Ok(())
    }

    /// Set the draft status of an existing PR.
    ///
    /// Uses GraphQL mutations because the REST API does not support toggling draft status.
    /// - `convertPullRequestToDraft` to mark as draft
    /// - `markPullRequestReadyForReview` to publish
    pub async fn set_pr_draft(&self, pr_number: u64, is_draft: bool) -> Result<()> {
        let node_id = self.get_pr_node_id(pr_number).await?;

        let mutation = if is_draft {
            self.record_api_call("pulls.convertToDraft");
            format!(
                r#"
                mutation {{
                    convertPullRequestToDraft(input: {{ pullRequestId: "{}" }}) {{
                        pullRequest {{ isDraft }}
                    }}
                }}
                "#,
                node_id
            )
        } else {
            self.record_api_call("pulls.markReadyForReview");
            format!(
                r#"
                mutation {{
                    markPullRequestReadyForReview(input: {{ pullRequestId: "{}" }}) {{
                        pullRequest {{ isDraft }}
                    }}
                }}
                "#,
                node_id
            )
        };

        let response: GraphQLResponse<serde_json::Value> = self
            .octocrab
            .graphql(&serde_json::json!({ "query": mutation }))
            .await
            .context("Failed to update PR draft status")?;

        if let Some(errors) = response.errors {
            if !errors.is_empty() {
                anyhow::bail!(
                    "Failed to {} PR #{}: {}",
                    if is_draft {
                        "convert to draft"
                    } else {
                        "mark ready for review"
                    },
                    pr_number,
                    errors[0].message
                );
            }
        }

        Ok(())
    }

    /// Merge the pull request base branch into the head branch (GitHub "Update branch" button).
    ///
    /// See <https://docs.github.com/en/rest/pulls/pulls#update-a-pull-request-branch>.
    pub async fn update_pr_branch(&self, pr_number: u64) -> Result<()> {
        self.record_api_call("pulls.update-branch");
        let route = format!(
            "/repos/{}/{}/pulls/{}/update-branch",
            self.owner, self.repo, pr_number
        );
        let result = self
            .octocrab
            .put::<serde_json::Value, _, serde_json::Value>(&route, Some(&serde_json::json!({})))
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                // 422 when head already includes base (nothing to merge)
                if msg.contains("Update is not required")
                    || msg.contains("There are no new commits")
                {
                    Ok(())
                } else {
                    Err(e).context("Failed to update PR branch")
                }
            }
        }
    }

    /// Update PR title
    pub async fn update_pr_title(&self, pr_number: u64, title: &str) -> Result<()> {
        self.record_api_call("pulls.update.title");
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .update(pr_number)
            .title(title)
            .send()
            .await
            .context("Failed to update PR title")?;
        Ok(())
    }

    /// Update PR body text
    pub async fn update_pr_body(&self, pr_number: u64, body: &str) -> Result<()> {
        self.record_api_call("pulls.update.body");
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .update(pr_number)
            .body(body)
            .send()
            .await
            .context("Failed to update PR body")?;
        Ok(())
    }

    /// Get the current PR body text.
    pub async fn get_pr_body(&self, pr_number: u64) -> Result<String> {
        self.record_api_call("pulls.get.body");
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .get(pr_number)
            .await
            .context("Failed to get PR body")?;

        Ok(pr.body.unwrap_or_default())
    }

    /// Add or update the stack comment on a PR
    pub async fn update_stack_comment(&self, pr_number: u64, stack_comment: &str) -> Result<()> {
        if let Some(comment_id) = self.find_stack_comment_id(pr_number).await? {
            let full_comment = format!("{}\n{}", STACK_COMMENT_MARKER, stack_comment);
            self.record_api_call("issues.comments.update");
            let route = format!(
                "/repos/{}/{}/issues/comments/{}",
                self.owner, self.repo, comment_id
            );
            self.octocrab
                .patch::<serde_json::Value, _, _>(
                    &route,
                    Some(&serde_json::json!({ "body": full_comment })),
                )
                .await
                .context("Failed to update comment")?;
            return Ok(());
        }

        self.create_stack_comment(pr_number, stack_comment).await
    }

    /// Create a stax stack comment on a PR without listing existing comments.
    pub async fn create_stack_comment(&self, pr_number: u64, stack_comment: &str) -> Result<()> {
        self.record_api_call("issues.comments.create");
        let full_comment = format!("{}\n{}", STACK_COMMENT_MARKER, stack_comment);
        self.octocrab
            .issues(&self.owner, &self.repo)
            .create_comment(pr_number, &full_comment)
            .await
            .context("Failed to create comment")?;

        Ok(())
    }

    /// Delete the stax-managed stack comment on a PR, if present.
    pub async fn delete_stack_comment(&self, pr_number: u64) -> Result<()> {
        let Some(comment_id) = self.find_stack_comment_id(pr_number).await? else {
            return Ok(());
        };

        self.record_api_call("issues.comments.delete");
        self.octocrab
            .issues(&self.owner, &self.repo)
            .delete_comment(comment_id)
            .await
            .context("Failed to delete comment")?;

        Ok(())
    }

    async fn find_stack_comment_id(
        &self,
        pr_number: u64,
    ) -> Result<Option<octocrab::models::CommentId>> {
        self.record_api_call("issues.comments.list");
        let url = format!(
            "/repos/{}/{}/issues/{}/comments",
            self.owner, self.repo, pr_number
        );
        let comments: Vec<ApiIssueComment> = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to list comments")?;

        Ok(comments.into_iter().find_map(|comment| {
            comment
                .body
                .as_ref()
                .filter(|body| body.contains(STACK_COMMENT_MARKER))
                .map(|_| octocrab::models::CommentId::from(comment.id))
        }))
    }

    pub async fn request_reviewers(&self, pr_number: u64, reviewers: &[String]) -> Result<()> {
        if reviewers.is_empty() {
            return Ok(());
        }

        self.record_api_call("pulls.request_reviewers");
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .request_reviews(pr_number, reviewers.to_vec(), Vec::<String>::new())
            .await
            .context("Failed to request reviewers")?;

        Ok(())
    }

    /// Get the list of requested reviewer logins for a PR
    pub async fn get_requested_reviewers(&self, pr_number: u64) -> Result<Vec<String>> {
        self.record_api_call("pulls.get");
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .get(pr_number)
            .await
            .context("Failed to get PR for reviewers")?;

        let reviewers: Vec<String> = pr
            .requested_reviewers
            .unwrap_or_default()
            .iter()
            .map(|r| r.login.clone())
            .collect();

        Ok(reviewers)
    }

    pub async fn add_labels(&self, pr_number: u64, labels: &[String]) -> Result<()> {
        if labels.is_empty() {
            return Ok(());
        }

        self.record_api_call("issues.add_labels");
        self.octocrab
            .issues(&self.owner, &self.repo)
            .add_labels(pr_number, labels)
            .await
            .context("Failed to add labels")?;

        Ok(())
    }

    pub async fn add_assignees(&self, pr_number: u64, assignees: &[String]) -> Result<()> {
        if assignees.is_empty() {
            return Ok(());
        }

        let assignees_refs: Vec<&str> = assignees.iter().map(|s| s.as_str()).collect();
        self.record_api_call("issues.add_assignees");
        self.octocrab
            .issues(&self.owner, &self.repo)
            .add_assignees(pr_number, &assignees_refs)
            .await
            .context("Failed to add assignees")?;

        Ok(())
    }

    /// Merge a PR with the specified method
    pub async fn merge_pr(
        &self,
        pr_number: u64,
        method: MergeMethod,
        commit_title: Option<String>,
        commit_message: Option<String>,
    ) -> Result<()> {
        let merge_method = match method {
            MergeMethod::Squash => octocrab::params::pulls::MergeMethod::Squash,
            MergeMethod::Merge => octocrab::params::pulls::MergeMethod::Merge,
            MergeMethod::Rebase => octocrab::params::pulls::MergeMethod::Rebase,
        };

        let pulls = self.octocrab.pulls(&self.owner, &self.repo);
        let mut merge_builder = pulls.merge(pr_number).method(merge_method);

        if let Some(ref title) = commit_title {
            merge_builder = merge_builder.title(title);
        }

        if let Some(ref message) = commit_message {
            merge_builder = merge_builder.message(message);
        }

        merge_builder.send().await.context("Failed to merge PR")?;

        Ok(())
    }

    /// Get detailed merge status for a PR
    pub async fn get_pr_merge_status(&self, pr_number: u64) -> Result<PrMergeStatus> {
        // Get basic PR info
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .get(pr_number)
            .await
            .context("Failed to get PR")?;

        let head_sha = pr.head.sha.clone();

        // Get CI status (default to NoCi if we can't fetch - don't block on missing info)
        let ci_status = self
            .combined_status_state(&head_sha)
            .await
            .ok()
            .flatten()
            .map(|s| CiStatus::from_str(&s))
            .unwrap_or(CiStatus::NoCi);

        // Get review info via GraphQL
        let (review_decision, approvals, changes_requested) = self
            .get_pr_reviews(pr_number)
            .await
            .unwrap_or((None, 0, false));

        Ok(PrMergeStatus {
            number: pr.number,
            title: pr.title.clone().unwrap_or_default(),
            state: pr
                .state
                .as_ref()
                .map(|s| format!("{:?}", s))
                .unwrap_or_default(),
            is_draft: pr.draft.unwrap_or(false),
            mergeable: pr.mergeable,
            mergeable_state: pr
                .mergeable_state
                .map(|s| format!("{:?}", s).to_lowercase())
                .unwrap_or_default(),
            ci_status,
            review_decision,
            approvals,
            changes_requested,
            head_sha,
        })
    }

    /// Get PR review information using GraphQL API
    async fn get_pr_reviews(&self, pr_number: u64) -> Result<(Option<String>, usize, bool)> {
        let query = format!(
            r#"
            query {{
                repository(owner: "{}", name: "{}") {{
                    pullRequest(number: {}) {{
                        reviewDecision
                        reviews(last: 100) {{
                            nodes {{
                                state
                            }}
                        }}
                    }}
                }}
            }}
            "#,
            self.owner, self.repo, pr_number
        );

        let response: GraphQLResponse<PrReviewData> = self
            .octocrab
            .graphql(&serde_json::json!({ "query": query }))
            .await
            .context("Failed to query PR reviews")?;

        if let Some(errors) = response.errors {
            if !errors.is_empty() {
                anyhow::bail!("GraphQL error: {}", errors[0].message);
            }
        }

        let (review_decision, approvals, changes_requested) = response
            .data
            .and_then(|d| d.repository)
            .and_then(|r| r.pull_request)
            .map(|pr| {
                let approvals = pr
                    .reviews
                    .nodes
                    .iter()
                    .filter(|r| r.state == "APPROVED")
                    .count();
                let changes_requested = pr
                    .reviews
                    .nodes
                    .iter()
                    .any(|r| r.state == "CHANGES_REQUESTED");
                (pr.review_decision, approvals, changes_requested)
            })
            .unwrap_or((None, 0, false));

        Ok((review_decision, approvals, changes_requested))
    }

    /// Get the GraphQL node ID for a PR (needed for mutations like enqueuePullRequest).
    async fn get_pr_node_id(&self, pr_number: u64) -> Result<String> {
        let query = format!(
            r#"
            query {{
                repository(owner: "{}", name: "{}") {{
                    pullRequest(number: {}) {{
                        id
                    }}
                }}
            }}
            "#,
            self.owner, self.repo, pr_number
        );

        let response: GraphQLResponse<PrNodeIdData> = self
            .octocrab
            .graphql(&serde_json::json!({ "query": query }))
            .await
            .context("Failed to query PR node ID")?;

        if let Some(errors) = response.errors {
            if !errors.is_empty() {
                anyhow::bail!("GraphQL error: {}", errors[0].message);
            }
        }

        response
            .data
            .and_then(|d| d.repository)
            .and_then(|r| r.pull_request)
            .map(|pr| pr.id)
            .context("PR not found")
    }

    /// Enqueue a PR into GitHub's merge queue.
    ///
    /// Requires the repository to have merge queue enabled in branch protection rules.
    /// Returns the queue entry with position information.
    pub async fn enqueue_pr(&self, pr_number: u64) -> Result<EnqueueResult> {
        let node_id = self.get_pr_node_id(pr_number).await?;

        let mutation = format!(
            r#"
            mutation {{
                enqueuePullRequest(input: {{ pullRequestId: "{}" }}) {{
                    mergeQueueEntry {{
                        position
                    }}
                }}
            }}
            "#,
            node_id
        );

        let response: GraphQLResponse<EnqueueData> = self
            .octocrab
            .graphql(&serde_json::json!({ "query": mutation }))
            .await
            .context("Failed to enqueue PR into merge queue")?;

        if let Some(errors) = response.errors {
            if !errors.is_empty() {
                anyhow::bail!(
                    "Failed to enqueue PR #{} into merge queue: {}",
                    pr_number,
                    errors[0].message
                );
            }
        }

        response
            .data
            .and_then(|d| d.enqueue_pull_request)
            .context("No enqueue result returned — is merge queue enabled on this repository?")
    }

    /// Check if a PR is already merged
    pub async fn is_pr_merged(&self, pr_number: u64) -> Result<bool> {
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .get(pr_number)
            .await
            .context("Failed to get PR")?;

        Ok(pr.merged_at.is_some())
    }

    /// List all issue comments (conversation comments) on a PR
    pub async fn list_issue_comments(&self, pr_number: u64) -> Result<Vec<IssueComment>> {
        let url = format!(
            "/repos/{}/{}/issues/{}/comments",
            self.owner, self.repo, pr_number
        );
        let comments: Vec<ApiIssueComment> = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to list issue comments")?;

        Ok(comments
            .into_iter()
            .map(|c| IssueComment {
                id: c.id,
                body: c.body.unwrap_or_default(),
                user: c.user.login,
                created_at: c.created_at,
            })
            .collect())
    }

    /// List all review comments (inline code comments) on a PR
    pub async fn list_review_comments(&self, pr_number: u64) -> Result<Vec<ReviewComment>> {
        let url = format!(
            "/repos/{}/{}/pulls/{}/comments",
            self.owner, self.repo, pr_number
        );

        #[derive(Deserialize)]
        struct ApiReviewComment {
            id: u64,
            body: Option<String>,
            user: ApiUser,
            path: String,
            line: Option<u32>,
            start_line: Option<u32>,
            created_at: DateTime<Utc>,
            diff_hunk: Option<String>,
        }

        let comments: Vec<ApiReviewComment> = self
            .octocrab
            .get(&url, None::<&()>)
            .await
            .context("Failed to list review comments")?;

        Ok(comments
            .into_iter()
            .map(|c| ReviewComment {
                id: c.id,
                body: c.body.unwrap_or_default(),
                user: c.user.login,
                path: c.path,
                line: c.line,
                start_line: c.start_line,
                created_at: c.created_at,
                diff_hunk: c.diff_hunk,
            })
            .collect())
    }

    /// List all comments (both issue and review) on a PR, sorted by creation time
    pub async fn list_all_comments(&self, pr_number: u64) -> Result<Vec<PrComment>> {
        let (issue_comments, review_comments) = tokio::try_join!(
            self.list_issue_comments(pr_number),
            self.list_review_comments(pr_number)
        )?;

        let mut all_comments: Vec<PrComment> = Vec::new();

        for c in issue_comments {
            all_comments.push(PrComment::Issue(c));
        }

        for c in review_comments {
            all_comments.push(PrComment::Review(c));
        }

        // Sort by creation time
        all_comments.sort_by_key(|c| c.created_at());

        Ok(all_comments)
    }
}

/// PR info for stack comment generation
#[derive(Debug, Clone)]
pub struct StackPrInfo {
    pub branch: String,
    pub pr_number: Option<u64>,
}

/// Generate the stack links markdown shared by PR comments and PR bodies.
pub fn generate_stack_links_markdown(
    prs: &[StackPrInfo],
    current_pr_number: u64,
    remote: &RemoteInfo,
    trunk: &str,
) -> String {
    let mr_label = match remote.forge {
        ForgeType::GitLab => "MR",
        _ => "PR",
    };
    let mr_prefix = match remote.forge {
        ForgeType::GitLab => "!",
        _ => "#",
    };

    let mut lines = vec![
        "## Stack Links".to_string(),
        "".to_string(),
        format!("This {} is part of a stacked series:", mr_label),
        "".to_string(),
        format!("* `{}`", trunk),
    ];

    // Build stack from bottom (trunk-adjacent) to top (leaf)
    // First PR is closest to trunk, last is the leaf
    for (i, pr_info) in prs.iter().enumerate() {
        let is_current = pr_info.pr_number == Some(current_pr_number);
        let pointer = if is_current { " 👈" } else { "" };

        let pr_text = match pr_info.pr_number {
            Some(num) => {
                let label = format!("{} {}{}", mr_label, mr_prefix, num);
                match remote.forge {
                    // GitHub auto-links #N references; other forges need full URLs
                    ForgeType::GitHub => format!("**{}**{}", label, pointer),
                    _ => {
                        format!("[**{}**]({}){}", label, remote.pr_url(num), pointer)
                    }
                }
            }
            None => format!("`{}`{}", pr_info.branch, pointer),
        };

        // Indent based on position in stack (2 spaces per level)
        let indent = "  ".repeat(i + 1);
        lines.push(format!("{}* {}", indent, pr_text));
    }

    lines.push("".to_string());
    lines.push(
        "This comment was autogenerated by [stax](https://github.com/cesarferreira/stax)"
            .to_string(),
    );

    lines.join("\n")
}

/// Backward-compatible alias for the existing comment generator name.
pub fn generate_stack_comment(
    prs: &[StackPrInfo],
    current_pr_number: u64,
    remote: &RemoteInfo,
    trunk: &str,
) -> String {
    generate_stack_links_markdown(prs, current_pr_number, remote, trunk)
}

pub fn upsert_stack_links_in_body(existing_body: &str, stack_links: &str) -> String {
    let managed_block = format!(
        "{start}\n{stack_links}\n{end}",
        start = STACK_LINKS_BODY_START_MARKER,
        stack_links = stack_links.trim(),
        end = STACK_LINKS_BODY_END_MARKER
    );

    let body_without_existing = remove_stack_links_from_body(existing_body);
    if body_without_existing.is_empty() {
        return managed_block;
    }

    if body_without_existing.ends_with("\n\n") {
        format!("{}{}", body_without_existing, managed_block)
    } else if body_without_existing.ends_with('\n') {
        format!("{}\n{}", body_without_existing, managed_block)
    } else {
        format!("{}\n\n{}", body_without_existing, managed_block)
    }
}

pub fn remove_stack_links_from_body(existing_body: &str) -> String {
    let Some(start_idx) = existing_body.find(STACK_LINKS_BODY_START_MARKER) else {
        return existing_body.to_string();
    };
    let Some(end_marker_idx) = existing_body[start_idx..].find(STACK_LINKS_BODY_END_MARKER) else {
        return existing_body.to_string();
    };

    let end_idx = start_idx + end_marker_idx + STACK_LINKS_BODY_END_MARKER.len();
    let mut remove_start = start_idx;
    let mut remove_end = end_idx;

    if existing_body[..start_idx].ends_with("\n\n") {
        remove_start -= 2;
    } else if existing_body[..start_idx].ends_with('\n') {
        remove_start -= 1;
    } else if existing_body[end_idx..].starts_with("\n\n") {
        remove_end += 2;
    } else if existing_body[end_idx..].starts_with('\n') {
        remove_end += 1;
    }

    let mut result = String::with_capacity(existing_body.len());
    result.push_str(&existing_body[..remove_start]);
    result.push_str(&existing_body[remove_end..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use octocrab::Octocrab;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_merge_method_from_str_squash() {
        let method: MergeMethod = "squash".parse().unwrap();
        assert!(matches!(method, MergeMethod::Squash));
    }

    #[test]
    fn test_merge_method_from_str_merge() {
        let method: MergeMethod = "merge".parse().unwrap();
        assert!(matches!(method, MergeMethod::Merge));
    }

    #[test]
    fn test_merge_method_from_str_rebase() {
        let method: MergeMethod = "rebase".parse().unwrap();
        assert!(matches!(method, MergeMethod::Rebase));
    }

    #[test]
    fn test_merge_method_from_str_case_insensitive() {
        let method: MergeMethod = "SQUASH".parse().unwrap();
        assert!(matches!(method, MergeMethod::Squash));

        let method: MergeMethod = "Merge".parse().unwrap();
        assert!(matches!(method, MergeMethod::Merge));

        let method: MergeMethod = "REBASE".parse().unwrap();
        assert!(matches!(method, MergeMethod::Rebase));
    }

    #[test]
    fn test_merge_method_from_str_invalid() {
        let result: Result<MergeMethod> = "invalid".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_method_as_str() {
        assert_eq!(MergeMethod::Squash.as_str(), "squash");
        assert_eq!(MergeMethod::Merge.as_str(), "merge");
        assert_eq!(MergeMethod::Rebase.as_str(), "rebase");
    }

    #[test]
    fn test_merge_method_default() {
        let method = MergeMethod::default();
        assert!(matches!(method, MergeMethod::Squash));
    }

    #[test]
    fn test_ci_status_from_str() {
        assert!(matches!(CiStatus::from_str("success"), CiStatus::Success));
        assert!(matches!(CiStatus::from_str("pending"), CiStatus::Pending));
        assert!(matches!(CiStatus::from_str("failure"), CiStatus::Failure));
        assert!(matches!(CiStatus::from_str("error"), CiStatus::Failure));
        // Neutral/skipped/cancelled are treated as success
        assert!(matches!(CiStatus::from_str("neutral"), CiStatus::Success));
        assert!(matches!(CiStatus::from_str("skipped"), CiStatus::Success));
        // Unknown states are treated as NoCi (no blocking)
        assert!(matches!(CiStatus::from_str("unknown"), CiStatus::NoCi));
        assert!(matches!(CiStatus::from_str("random"), CiStatus::NoCi));
        assert!(matches!(CiStatus::from_str(""), CiStatus::NoCi));
    }

    #[test]
    fn test_ci_status_from_str_case_insensitive() {
        assert!(matches!(CiStatus::from_str("SUCCESS"), CiStatus::Success));
        assert!(matches!(CiStatus::from_str("PENDING"), CiStatus::Pending));
        assert!(matches!(CiStatus::from_str("FAILURE"), CiStatus::Failure));
    }

    #[test]
    fn test_ci_status_is_methods() {
        assert!(CiStatus::Success.is_success());
        assert!(!CiStatus::Success.is_pending());
        assert!(!CiStatus::Success.is_failure());

        assert!(!CiStatus::Pending.is_success());
        assert!(CiStatus::Pending.is_pending());
        assert!(!CiStatus::Pending.is_failure());

        assert!(!CiStatus::Failure.is_success());
        assert!(!CiStatus::Failure.is_pending());
        assert!(CiStatus::Failure.is_failure());

        // NoCi is treated as success (nothing blocking)
        assert!(CiStatus::NoCi.is_success());
        assert!(!CiStatus::NoCi.is_pending());
        assert!(!CiStatus::NoCi.is_failure());
    }

    #[test]
    fn test_ci_status_display_text() {
        assert_eq!(CiStatus::Success.display_text(), "passed");
        assert_eq!(CiStatus::Pending.display_text(), "running");
        assert_eq!(CiStatus::Failure.display_text(), "failed");
        assert_eq!(CiStatus::NoCi.display_text(), "no checks");
    }

    #[test]
    fn test_pr_merge_status_is_ready() {
        let status = PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "Open".to_string(),
            is_draft: false,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            ci_status: CiStatus::Success,
            review_decision: Some("APPROVED".to_string()),
            approvals: 1,
            changes_requested: false,
            head_sha: "abc123".to_string(),
        };

        assert!(status.is_ready());
        assert!(!status.is_waiting());
        assert!(!status.is_blocked());
    }

    #[test]
    fn test_pr_merge_status_is_waiting_ci_pending() {
        let status = PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "Open".to_string(),
            is_draft: false,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            ci_status: CiStatus::Pending,
            review_decision: Some("APPROVED".to_string()),
            approvals: 1,
            changes_requested: false,
            head_sha: "abc123".to_string(),
        };

        assert!(!status.is_ready());
        assert!(status.is_waiting());
        assert!(!status.is_blocked());
    }

    #[test]
    fn test_pr_merge_status_is_waiting_mergeable_computing() {
        let status = PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "Open".to_string(),
            is_draft: false,
            mergeable: None, // Still computing
            mergeable_state: "unknown".to_string(),
            ci_status: CiStatus::Success,
            review_decision: Some("APPROVED".to_string()),
            approvals: 1,
            changes_requested: false,
            head_sha: "abc123".to_string(),
        };

        assert!(!status.is_ready());
        assert!(status.is_waiting());
    }

    #[test]
    fn test_pr_merge_status_is_blocked_ci_failed() {
        let status = PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "Open".to_string(),
            is_draft: false,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            ci_status: CiStatus::Failure,
            review_decision: Some("APPROVED".to_string()),
            approvals: 1,
            changes_requested: false,
            head_sha: "abc123".to_string(),
        };

        assert!(!status.is_ready());
        assert!(status.is_blocked());
    }

    #[test]
    fn test_pr_merge_status_is_blocked_changes_requested() {
        let status = PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "Open".to_string(),
            is_draft: false,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            ci_status: CiStatus::Success,
            review_decision: Some("CHANGES_REQUESTED".to_string()),
            approvals: 0,
            changes_requested: true,
            head_sha: "abc123".to_string(),
        };

        assert!(!status.is_ready());
        assert!(status.is_blocked());
    }

    #[test]
    fn test_pr_merge_status_is_blocked_draft() {
        let status = PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "Open".to_string(),
            is_draft: true,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            ci_status: CiStatus::Success,
            review_decision: Some("APPROVED".to_string()),
            approvals: 1,
            changes_requested: false,
            head_sha: "abc123".to_string(),
        };

        assert!(!status.is_ready());
        assert!(status.is_blocked());
    }

    #[test]
    fn test_pr_merge_status_is_blocked_not_mergeable() {
        let status = PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "Open".to_string(),
            is_draft: false,
            mergeable: Some(false), // Has conflicts
            mergeable_state: "dirty".to_string(),
            ci_status: CiStatus::Success,
            review_decision: Some("APPROVED".to_string()),
            approvals: 1,
            changes_requested: false,
            head_sha: "abc123".to_string(),
        };

        assert!(!status.is_ready());
        assert!(status.is_blocked());
    }

    #[test]
    fn test_pr_merge_status_text() {
        // Ready
        let status = PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "Open".to_string(),
            is_draft: false,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            ci_status: CiStatus::Success,
            review_decision: None,
            approvals: 1,
            changes_requested: false,
            head_sha: "abc123".to_string(),
        };
        assert_eq!(status.status_text(), "Ready");

        // Draft
        let status = PrMergeStatus {
            is_draft: true,
            ..status.clone()
        };
        assert_eq!(status.status_text(), "Draft");

        // CI Failed
        let status = PrMergeStatus {
            is_draft: false,
            ci_status: CiStatus::Failure,
            ..status.clone()
        };
        assert_eq!(status.status_text(), "CI failed");

        // Changes requested
        let status = PrMergeStatus {
            ci_status: CiStatus::Success,
            changes_requested: true,
            ..status.clone()
        };
        assert_eq!(status.status_text(), "Changes requested");

        // Has conflicts
        let status = PrMergeStatus {
            changes_requested: false,
            mergeable: Some(false),
            ..status.clone()
        };
        assert_eq!(status.status_text(), "Has conflicts");

        // Closed
        let status = PrMergeStatus {
            mergeable: Some(true),
            state: "Closed".to_string(),
            ..status.clone()
        };
        assert_eq!(status.status_text(), "Closed");
    }

    #[test]
    fn test_generate_stack_comment_single_pr() {
        let remote = crate::remote::RemoteInfo {
            name: "origin".to_string(),
            forge: crate::remote::ForgeType::GitHub,
            host: "github.com".to_string(),
            namespace: "user".to_string(),
            repo: "repo".to_string(),
            base_url: "https://github.com".to_string(),
            api_base_url: Some("https://api.github.com".to_string()),
        };

        let prs = vec![StackPrInfo {
            branch: "feature".to_string(),
            pr_number: Some(1),
        }];

        let comment = generate_stack_comment(&prs, 1, &remote, "main");

        assert!(comment.contains("## Stack Links"));
        assert!(comment.contains("`main`"));
        assert!(comment.contains("PR #1"));
        assert!(comment.contains("👈")); // Current PR marker
        assert!(comment.contains("stax"));
    }

    #[test]
    fn test_generate_stack_comment_multiple_prs() {
        let remote = crate::remote::RemoteInfo {
            name: "origin".to_string(),
            forge: crate::remote::ForgeType::GitHub,
            host: "github.com".to_string(),
            namespace: "user".to_string(),
            repo: "repo".to_string(),
            base_url: "https://github.com".to_string(),
            api_base_url: Some("https://api.github.com".to_string()),
        };

        let prs = vec![
            StackPrInfo {
                branch: "feature-a".to_string(),
                pr_number: Some(1),
            },
            StackPrInfo {
                branch: "feature-b".to_string(),
                pr_number: Some(2),
            },
            StackPrInfo {
                branch: "feature-c".to_string(),
                pr_number: Some(3),
            },
        ];

        let comment = generate_stack_comment(&prs, 2, &remote, "main");

        assert!(comment.contains("PR #1"));
        assert!(comment.contains("PR #2"));
        assert!(comment.contains("PR #3"));
        // Only PR #2 should have the pointer
        assert!(comment.contains("PR #2** 👈"));
    }

    #[test]
    fn test_generate_stack_comment_without_pr() {
        let remote = crate::remote::RemoteInfo {
            name: "origin".to_string(),
            forge: crate::remote::ForgeType::GitHub,
            host: "github.com".to_string(),
            namespace: "user".to_string(),
            repo: "repo".to_string(),
            base_url: "https://github.com".to_string(),
            api_base_url: Some("https://api.github.com".to_string()),
        };

        let prs = vec![
            StackPrInfo {
                branch: "feature-a".to_string(),
                pr_number: Some(1),
            },
            StackPrInfo {
                branch: "feature-b".to_string(),
                pr_number: None, // No PR yet
            },
        ];

        let comment = generate_stack_comment(&prs, 1, &remote, "main");

        assert!(comment.contains("PR #1"));
        assert!(comment.contains("`feature-b`")); // Branch name in backticks
    }

    #[test]
    fn test_generate_stack_comment_gitlab_uses_full_urls() {
        let remote = crate::remote::RemoteInfo {
            name: "origin".to_string(),
            forge: crate::remote::ForgeType::GitLab,
            host: "gitlab.com".to_string(),
            namespace: "user".to_string(),
            repo: "repo".to_string(),
            base_url: "https://gitlab.com".to_string(),
            api_base_url: Some("https://gitlab.com/api/v4".to_string()),
        };

        let prs = vec![
            StackPrInfo {
                branch: "feature-a".to_string(),
                pr_number: Some(10),
            },
            StackPrInfo {
                branch: "feature-b".to_string(),
                pr_number: Some(11),
            },
        ];

        let comment = generate_stack_comment(&prs, 11, &remote, "main");

        // GitLab uses MR terminology and !N prefix
        assert!(comment.contains("This MR is part of a stacked series:"));
        assert!(comment.contains("[**MR !10**](https://gitlab.com/user/repo/-/merge_requests/10)"));
        assert!(
            comment.contains("[**MR !11**](https://gitlab.com/user/repo/-/merge_requests/11) 👈")
        );
    }

    #[test]
    fn test_generate_stack_comment_gitea_uses_full_urls() {
        let remote = crate::remote::RemoteInfo {
            name: "origin".to_string(),
            forge: crate::remote::ForgeType::Gitea,
            host: "gitea.example.com".to_string(),
            namespace: "org".to_string(),
            repo: "project".to_string(),
            base_url: "https://gitea.example.com".to_string(),
            api_base_url: Some("https://gitea.example.com/api/v1".to_string()),
        };

        let prs = vec![StackPrInfo {
            branch: "feature".to_string(),
            pr_number: Some(5),
        }];

        let comment = generate_stack_comment(&prs, 5, &remote, "main");

        // Gitea uses PR terminology but needs full URLs
        assert!(comment.contains("This PR is part of a stacked series:"));
        assert!(comment.contains("[**PR #5**](https://gitea.example.com/org/project/pulls/5) 👈"));
    }

    #[test]
    fn test_generate_stack_comment_gitlab_nested_namespace() {
        let remote = crate::remote::RemoteInfo {
            name: "origin".to_string(),
            forge: crate::remote::ForgeType::GitLab,
            host: "gitlab.com".to_string(),
            namespace: "group/subgroup".to_string(),
            repo: "repo".to_string(),
            base_url: "https://gitlab.com".to_string(),
            api_base_url: Some("https://gitlab.com/api/v4".to_string()),
        };

        let prs = vec![StackPrInfo {
            branch: "feature".to_string(),
            pr_number: Some(42),
        }];

        let comment = generate_stack_comment(&prs, 42, &remote, "main");

        assert!(comment.contains(
            "[**MR !42**](https://gitlab.com/group/subgroup/repo/-/merge_requests/42) 👈"
        ));
    }

    #[test]
    fn test_upsert_stack_links_in_empty_body() {
        let body = upsert_stack_links_in_body("", "## Stack Links\n\n- item");
        assert!(body.contains(STACK_LINKS_BODY_START_MARKER));
        assert!(body.contains("## Stack Links"));
        assert!(body.contains(STACK_LINKS_BODY_END_MARKER));
    }

    #[test]
    fn test_upsert_stack_links_appends_to_existing_body() {
        let body = upsert_stack_links_in_body("## Summary\n\nhello", "## Stack Links\n\n- item");
        assert!(body.starts_with("## Summary\n\nhello"));
        assert!(body.ends_with(STACK_LINKS_BODY_END_MARKER));
        assert!(body.contains("\n\n<!-- stax-stack-links:start -->"));
    }

    #[test]
    fn test_upsert_stack_links_replaces_existing_block() {
        let existing = format!(
            "## Summary\n\nhello\n\n{}\nold\n{}\n",
            STACK_LINKS_BODY_START_MARKER, STACK_LINKS_BODY_END_MARKER
        );
        let body = upsert_stack_links_in_body(&existing, "## Stack Links\n\nnew");
        assert!(!body.contains("\nold\n"));
        assert!(body.contains("new"));
        assert_eq!(body.matches(STACK_LINKS_BODY_START_MARKER).count(), 1);
    }

    #[test]
    fn test_remove_stack_links_from_body_preserves_surrounding_content() {
        let existing = format!(
            "## Summary\n\nhello\n\n{}\nmanaged\n{}\n\n## Testing\n\nok",
            STACK_LINKS_BODY_START_MARKER, STACK_LINKS_BODY_END_MARKER
        );
        let body = remove_stack_links_from_body(&existing);
        assert_eq!(body, "## Summary\n\nhello\n\n## Testing\n\nok");
    }

    #[test]
    fn test_pr_info_debug() {
        let pr = PrInfo {
            number: 42,
            state: "Open".to_string(),
            is_draft: false,
            base: "main".to_string(),
        };
        let debug_str = format!("{:?}", pr);
        assert!(debug_str.contains("42"));
        assert!(debug_str.contains("Open"));
    }

    #[test]
    fn test_merge_method_clone() {
        let method = MergeMethod::Squash;
        let cloned = method;
        assert!(matches!(cloned, MergeMethod::Squash));
    }

    #[test]
    fn test_ci_status_clone() {
        let status = CiStatus::Success;
        let cloned = status.clone();
        assert!(matches!(cloned, CiStatus::Success));
    }

    #[test]
    fn test_ci_status_eq() {
        assert_eq!(CiStatus::Success, CiStatus::Success);
        assert_ne!(CiStatus::Success, CiStatus::Failure);
    }

    #[test]
    fn test_stack_pr_info_clone() {
        let info = StackPrInfo {
            branch: "feature".to_string(),
            pr_number: Some(42),
        };
        let cloned = info.clone();
        assert_eq!(cloned.branch, "feature");
        assert_eq!(cloned.pr_number, Some(42));
    }

    #[test]
    fn test_pr_merge_status_clone() {
        let status = PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "Open".to_string(),
            is_draft: false,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            ci_status: CiStatus::Success,
            review_decision: None,
            approvals: 1,
            changes_requested: false,
            head_sha: "abc123".to_string(),
        };
        let cloned = status.clone();
        assert_eq!(cloned.number, 1);
        assert_eq!(cloned.title, "Test");
    }

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

    fn issue_comment_fixture(id: u64, body: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "node_id": format!("IC_test_{}", id),
            "url": format!("https://api.github.com/repos/test-owner/test-repo/issues/comments/{}", id),
            "html_url": format!("https://github.com/test-owner/test-repo/pull/11#issuecomment-{}", id),
            "issue_url": "https://api.github.com/repos/test-owner/test-repo/issues/11",
            "body": body,
            "user": {
                "login": "stax",
                "id": 1,
                "node_id": "MDQ6VXNlcjE=",
                "avatar_url": "https://avatars.githubusercontent.com/u/1?v=4",
                "gravatar_id": "",
                "url": "https://api.github.com/users/stax",
                "html_url": "https://github.com/stax",
                "followers_url": "https://api.github.com/users/stax/followers",
                "following_url": "https://api.github.com/users/stax/following{/other_user}",
                "gists_url": "https://api.github.com/users/stax/gists{/gist_id}",
                "starred_url": "https://api.github.com/users/stax/starred{/owner}{/repo}",
                "subscriptions_url": "https://api.github.com/users/stax/subscriptions",
                "organizations_url": "https://api.github.com/users/stax/orgs",
                "repos_url": "https://api.github.com/users/stax/repos",
                "events_url": "https://api.github.com/users/stax/events{/privacy}",
                "received_events_url": "https://api.github.com/users/stax/received_events",
                "type": "User",
                "site_admin": false
            },
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        })
    }

    #[tokio::test]
    async fn test_get_pr_body_returns_body_text() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/11"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test-owner/test-repo/pulls/11",
                "id": 11,
                "number": 11,
                "state": "open",
                "draft": false,
                "body": "## Summary\n\nhello",
                "head": { "ref": "feature-a", "sha": "aaaa", "label": "test-owner:feature-a" },
                "base": { "ref": "main", "sha": "bbbb" }
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let body = client.get_pr_body(11).await.unwrap();
        assert_eq!(body, "## Summary\n\nhello");
    }

    #[tokio::test]
    async fn test_update_stack_comment_updates_existing_comment() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues/11/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                issue_comment_fixture(101, "<!-- stax-stack-comment -->\nold")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test-owner/test-repo/issues/comments/101"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(issue_comment_fixture(
                    101,
                    "<!-- stax-stack-comment -->\nnew body",
                )),
            )
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        client.update_stack_comment(11, "new body").await.unwrap();

        let requests = mock_server.received_requests().await.unwrap();
        let patch_request = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "PATCH"
                    && request.url.path() == "/repos/test-owner/test-repo/issues/comments/101"
            })
            .expect("missing patch request");
        let body: serde_json::Value = serde_json::from_slice(&patch_request.body).unwrap();
        assert_eq!(body["body"], format!("{}\nnew body", STACK_COMMENT_MARKER));
    }

    #[tokio::test]
    async fn test_delete_stack_comment_deletes_existing_comment() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/issues/11/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                issue_comment_fixture(101, "<!-- stax-stack-comment -->\nold")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/repos/test-owner/test-repo/issues/comments/101"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        client.delete_stack_comment(11).await.unwrap();

        let requests = mock_server.received_requests().await.unwrap();
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "DELETE"
                && request.url.path() == "/repos/test-owner/test-repo/issues/comments/101"
        }));
    }

    #[tokio::test]
    async fn test_list_open_prs_by_head_indexes_prs() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test-owner/test-repo/pulls/11",
                    "id": 11,
                    "number": 11,
                    "head": { "ref": "feature-a", "sha": "aaaa", "label": "test-owner:feature-a" },
                    "base": { "ref": "main", "sha": "bbbb" },
                    "draft": false
                },
                {
                    "url": "https://api.github.com/repos/test-owner/test-repo/pulls/12",
                    "id": 12,
                    "number": 12,
                    "head": { "ref": "feature-b", "sha": "cccc", "label": "test-owner:feature-b" },
                    "base": { "ref": "main", "sha": "dddd" },
                    "draft": true
                }
            ])))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let prs = client.list_open_prs_by_head().await.unwrap();

        let pr_a = prs.get("feature-a").expect("missing feature-a");
        assert_eq!(pr_a.info.number, 11);
        assert_eq!(pr_a.info.base, "main");
        assert!(!pr_a.info.is_draft);
        assert_eq!(pr_a.head_label.as_deref(), Some("test-owner:feature-a"));

        let pr_b = prs.get("feature-b").expect("missing feature-b");
        assert_eq!(pr_b.info.number, 12);
        assert!(pr_b.info.is_draft);
        assert_eq!(pr_b.head_label.as_deref(), Some("test-owner:feature-b"));
        assert_eq!(prs.len(), 2);

        let stats = client.api_call_stats();
        assert_eq!(stats.total_requests, 1);
        assert!(stats
            .by_operation
            .iter()
            .any(|(op, count)| op == "pulls.list.open.page" && *count == 1));
    }

    #[tokio::test]
    async fn test_find_open_pr_by_head_uses_head_filter() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls"))
            .and(query_param("state", "open"))
            .and(query_param("head", "test-owner:feature-a"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test-owner/test-repo/pulls/11",
                    "id": 11,
                    "number": 11,
                    "head": { "ref": "feature-a", "sha": "aaaa", "label": "test-owner:feature-a" },
                    "base": { "ref": "main", "sha": "bbbb" },
                    "draft": false
                }
            ])))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let pr = client
            .find_open_pr_by_head("test-owner", "feature-a")
            .await
            .unwrap()
            .expect("expected matching PR");

        assert_eq!(pr.info.number, 11);
        assert_eq!(pr.head, "feature-a");

        let stats = client.api_call_stats();
        assert_eq!(stats.total_requests, 1);
        assert!(stats
            .by_operation
            .iter()
            .any(|(op, count)| op == "pulls.list.head" && *count == 1));
    }

    #[tokio::test]
    async fn test_find_pr_falls_back_to_scan_when_head_lookup_misses() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls"))
            .and(query_param("state", "open"))
            .and(query_param("head", "test-owner:feature-a"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test-owner/test-repo/pulls/11",
                    "id": 11,
                    "number": 11,
                    "head": { "ref": "feature-a", "sha": "aaaa", "label": "test-owner:feature-a" },
                    "base": { "ref": "main", "sha": "bbbb" },
                    "draft": false
                }
            ])))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let pr = client
            .find_pr("feature-a")
            .await
            .unwrap()
            .expect("expected PR");

        assert_eq!(pr.number, 11);

        let stats = client.api_call_stats();
        assert_eq!(stats.total_requests, 2);
        assert!(stats
            .by_operation
            .iter()
            .any(|(op, count)| op == "pulls.list.head" && *count == 1));
        assert!(stats
            .by_operation
            .iter()
            .any(|(op, count)| op == "pulls.list.open.page" && *count == 1));
    }

    #[tokio::test]
    async fn test_get_pr_with_head_returns_head_and_info() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/11"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test-owner/test-repo/pulls/11",
                "id": 11,
                "number": 11,
                "head": { "ref": "feature-a", "sha": "aaaa", "label": "test-owner:feature-a" },
                "base": { "ref": "main", "sha": "bbbb" },
                "draft": false
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let pr = client.get_pr_with_head(11).await.unwrap();

        assert_eq!(pr.head, "feature-a");
        assert_eq!(pr.head_label.as_deref(), Some("test-owner:feature-a"));
        assert_eq!(pr.info.number, 11);
        assert_eq!(pr.info.base, "main");
        assert!(!pr.info.is_draft);

        let stats = client.api_call_stats();
        assert_eq!(stats.total_requests, 1);
        assert!(stats
            .by_operation
            .iter()
            .any(|(op, count)| op == "pulls.get" && *count == 1));
    }

    // Note: The find_pr function now validates that the returned PR's head branch
    // matches the requested branch name. This is critical because the GitHub API's
    // head filter can fail silently (e.g., with long branch names or URL encoding
    // issues), which could otherwise cause stax to update the wrong PR.
    //
    // The function:
    // 1. Only searches for OPEN PRs (filters closed/merged PRs)
    // 2. Validates pr.head.ref_field == requested_branch before returning
    // 3. Returns None if no matching open PR is found
    //
    // Integration testing with the actual GitHub API is recommended to verify
    // this behavior in real scenarios. The fix was implemented in response to
    // a bug where stax updated PR #75188 (for branch "renovate/pypi-starlette-vulnerability")
    // when submitting a completely unrelated branch.

    // The find_pr function behavior is tested via integration tests and manual testing,
    // as wiremock tests require complex mock JSON that matches octocrab's strict
    // deserialization requirements. The function:
    // - Should only return OPEN PRs
    // - Should validate head branch matches before returning
    // - Should return None if no matching open PR exists
}
