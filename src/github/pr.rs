use anyhow::{Context, Result};
use octocrab::params::pulls::Sort;
use serde::Deserialize;

use super::GitHubClient;
use crate::remote::RemoteInfo;

#[derive(Debug)]
pub struct PrInfo {
    pub number: u64,
    pub state: String,
    pub is_draft: bool,
    pub base: String,
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
    Unknown,
}

impl CiStatus {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "success" => CiStatus::Success,
            "pending" => CiStatus::Pending,
            "failure" | "error" => CiStatus::Failure,
            _ => CiStatus::Unknown,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, CiStatus::Success)
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, CiStatus::Pending)
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, CiStatus::Failure)
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
        "Unknown"
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
    /// Find existing PR for a branch
    pub async fn find_pr(&self, branch: &str) -> Result<Option<PrInfo>> {
        let prs = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .list()
            .head(format!("{}:{}", self.owner, branch))
            .sort(Sort::Created)
            .send()
            .await
            .context("Failed to list PRs")?;

        if let Some(pr) = prs.items.first() {
            Ok(Some(PrInfo {
                number: pr.number,
                state: pr.state.as_ref().map(|s| format!("{:?}", s)).unwrap_or_default(),
                is_draft: pr.draft.unwrap_or(false),
                base: pr.base.ref_field.clone(),
            }))
        } else {
            Ok(None)
        }
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
            state: pr.state.as_ref().map(|s| format!("{:?}", s)).unwrap_or_default(),
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
            state: pr.state.as_ref().map(|s| format!("{:?}", s)).unwrap_or_default(),
            is_draft: pr.draft.unwrap_or(false),
            base: pr.base.ref_field.clone(),
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

    /// Add or update the stack comment on a PR
    pub async fn update_stack_comment(
        &self,
        pr_number: u64,
        stack_comment: &str,
    ) -> Result<()> {
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
            if comment.body.as_ref().map(|b| b.contains(marker)).unwrap_or(false) {
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

        merge_builder
            .send()
            .await
            .context("Failed to merge PR")?;

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

        // Get CI status
        let ci_status = self
            .combined_status_state(&head_sha)
            .await
            .ok()
            .flatten()
            .map(|s| CiStatus::from_str(&s))
            .unwrap_or(CiStatus::Unknown);

        // Get review info via GraphQL
        let (review_decision, approvals, changes_requested) = self
            .get_pr_reviews(pr_number)
            .await
            .unwrap_or((None, 0, false));

        Ok(PrMergeStatus {
            number: pr.number,
            title: pr.title.clone().unwrap_or_default(),
            state: pr.state.as_ref().map(|s| format!("{:?}", s)).unwrap_or_default(),
            is_draft: pr.draft.unwrap_or(false),
            mergeable: pr.mergeable,
            mergeable_state: pr.mergeable_state.map(|s| format!("{:?}", s).to_lowercase()).unwrap_or_default(),
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
    lines.push("This comment was autogenerated by [stax](https://github.com/cesarferreira/stax)".to_string());

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(matches!(CiStatus::from_str("unknown"), CiStatus::Unknown));
        assert!(matches!(CiStatus::from_str("random"), CiStatus::Unknown));
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

        assert!(!CiStatus::Unknown.is_success());
        assert!(!CiStatus::Unknown.is_pending());
        assert!(!CiStatus::Unknown.is_failure());
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
}
