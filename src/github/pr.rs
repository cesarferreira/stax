use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::params::pulls::Sort;
use octocrab::params::State;
use serde::Deserialize;
use std::collections::HashMap;

use super::GitHubClient;
use crate::remote::RemoteInfo;

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
    /// Find existing open PR for a branch
    ///
    /// Only returns a PR if:
    /// 1. The PR is in OPEN state (not closed or merged)
    /// 2. The PR's head branch exactly matches the requested branch name
    ///
    /// Note: We don't use the GitHub API's `head` filter because it's unreliable
    /// with long branch names or special characters. Instead, we list open PRs
    /// and filter manually, with pagination support for repos with many PRs.
    pub async fn find_pr(&self, branch: &str) -> Result<Option<PrInfo>> {
        // List open PRs without using the unreliable `head` filter
        // Use pagination to handle repos with many open PRs efficiently
        let mut page = 1u32;
        const PER_PAGE: u8 = 100;

        loop {
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

            // Find the PR with matching head branch in this page
            for pr in &prs.items {
                if pr.head.ref_field == branch {
                    return Ok(Some(PrInfo {
                        number: pr.number,
                        state: pr
                            .state
                            .as_ref()
                            .map(|s| format!("{:?}", s))
                            .unwrap_or_default(),
                        is_draft: pr.draft.unwrap_or(false),
                        base: pr.base.ref_field.clone(),
                    }));
                }
            }

            // If we got fewer items than requested, we've reached the last page
            if (prs.items.len() as u8) < PER_PAGE {
                break;
            }

            page += 1;
        }

        Ok(None) // No matching open PR found
    }

    /// List all open PRs and index them by head branch name
    pub async fn list_open_prs_by_head(&self) -> Result<HashMap<String, PrInfoWithHead>> {
        let mut page = 1u32;
        const PER_PAGE: u8 = 100;
        let mut prs_by_head = HashMap::new();

        loop {
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
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .get(pr_number)
            .await
            .context("Failed to get PR")?;

        Ok(PrInfoWithHead {
            head: pr.head.ref_field.clone(),
            head_label: pr.head.label.clone(),
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
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .update(pr_number)
            .base(new_base)
            .send()
            .await
            .context("Failed to update PR base")?;
        Ok(())
    }

    /// Update PR body text
    pub async fn update_pr_body(&self, pr_number: u64, body: &str) -> Result<()> {
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .update(pr_number)
            .body(body)
            .send()
            .await
            .context("Failed to update PR body")?;
        Ok(())
    }

    /// Add or update the stack comment on a PR
    pub async fn update_stack_comment(&self, pr_number: u64, stack_comment: &str) -> Result<()> {
        let comments = self
            .octocrab
            .issues(&self.owner, &self.repo)
            .list_comments(pr_number)
            .send()
            .await
            .context("Failed to list comments")?;

        // Look for existing stax comment
        let marker = "<!-- stax-stack-comment -->";
        let full_comment = format!("{}\n{}", marker, stack_comment);

        for comment in comments.items {
            if comment
                .body
                .as_ref()
                .map(|b| b.contains(marker))
                .unwrap_or(false)
            {
                // Update existing comment
                self.octocrab
                    .issues(&self.owner, &self.repo)
                    .update_comment(comment.id, &full_comment)
                    .await
                    .context("Failed to update comment")?;
                return Ok(());
            }
        }

        // Create new comment
        self.octocrab
            .issues(&self.owner, &self.repo)
            .create_comment(pr_number, &full_comment)
            .await
            .context("Failed to create comment")?;

        Ok(())
    }

    pub async fn request_reviewers(&self, pr_number: u64, reviewers: &[String]) -> Result<()> {
        if reviewers.is_empty() {
            return Ok(());
        }

        self.octocrab
            .pulls(&self.owner, &self.repo)
            .request_reviews(pr_number, reviewers.to_vec(), Vec::<String>::new())
            .await
            .context("Failed to request reviewers")?;

        Ok(())
    }

    pub async fn add_labels(&self, pr_number: u64, labels: &[String]) -> Result<()> {
        if labels.is_empty() {
            return Ok(());
        }

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
        let comments = self
            .octocrab
            .issues(&self.owner, &self.repo)
            .list_comments(pr_number)
            .send()
            .await
            .context("Failed to list issue comments")?;

        Ok(comments
            .items
            .into_iter()
            .map(|c| IssueComment {
                id: c.id.into_inner(),
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

        #[derive(Deserialize)]
        struct ApiUser {
            login: String,
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

/// Generate the stack comment body
pub fn generate_stack_comment(
    prs: &[StackPrInfo],
    current_pr_number: u64,
    _remote: &RemoteInfo,
    trunk: &str,
) -> String {
    let mut lines = vec![
        "Current dependencies on/for this PR:".to_string(),
        "".to_string(),
        format!("* {}:", trunk),
    ];

    // Build stack from bottom (trunk-adjacent) to top (leaf)
    // First PR is closest to trunk, last is the leaf
    for (i, pr_info) in prs.iter().enumerate() {
        let is_current = pr_info.pr_number == Some(current_pr_number);
        let pointer = if is_current { " 👈" } else { "" };

        let pr_text = match pr_info.pr_number {
            Some(num) => format!("**PR #{}**{}", num, pointer),
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

#[cfg(test)]
mod tests {
    use super::*;
    use octocrab::Octocrab;
    use wiremock::matchers::{method, path};
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

        assert!(comment.contains("Current dependencies"));
        assert!(comment.contains("main"));
        assert!(comment.contains("PR #1"));
        assert!(comment.contains("👈")); // Current PR marker
        assert!(comment.contains("stax"));
    }

    #[test]
    fn test_generate_stack_comment_multiple_prs() {
        let remote = crate::remote::RemoteInfo {
            name: "origin".to_string(),
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
        // Only PR #2 should have the pointer (format is **PR #2** 👈)
        assert!(comment.contains("#2** 👈"));
    }

    #[test]
    fn test_generate_stack_comment_without_pr() {
        let remote = crate::remote::RemoteInfo {
            name: "origin".to_string(),
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

    async fn create_test_client(server: &MockServer) -> GitHubClient {
        let octocrab = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .personal_token("test-token".to_string())
            .build()
            .unwrap();

        GitHubClient::with_octocrab(octocrab, "test-owner", "test-repo")
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
