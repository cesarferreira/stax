use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::models::pulls::{Base, Head, PullRequest};
use octocrab::params::State;
use octocrab::params::pulls::Sort;
use serde::{Deserialize, de::DeserializeOwned};
use std::collections::HashMap;

use super::GitHubClient;
use crate::remote::{ForgeType, RemoteInfo};

const STACK_COMMENT_MARKER: &str = "<!-- stax-stack-comment -->";
const STACK_LINKS_BODY_START_MARKER: &str = "<!-- stax-stack-links:start -->";
const STACK_LINKS_BODY_END_MARKER: &str = "<!-- stax-stack-links:end -->";

/// True when a PR base-update failure is GitHub rejecting the change because
/// the PR is registered in a native GitHub Stack (private preview). GitHub
/// owns base-branch management for stacked PRs once linked, and returns this
/// validation error for `PATCH .../pulls/{n}` calls that touch `base` — even
/// when the requested value matches the PR's current base, or when the
/// caller is trying to perform a legitimate retarget (e.g. a merge cascade
/// moving the next PR onto trunk).
///
/// Matches on GitHub's exact wording ("...pull request is part of a
/// stack.") rather than the shorter "part of a stack" fragment, so a
/// hypothetical negated message like "...is not part of a stack" can never
/// be misclassified as a lock (the two phrases aren't substrings of one
/// another, since "not " breaks the contiguous match).
pub(crate) fn is_native_stack_base_locked_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .to_string()
            .to_lowercase()
            .contains("pull request is part of a stack")
    })
}

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

fn octocrab_pr_number(pr: &PullRequest) -> Result<u64> {
    pr.number.context("GitHub PR response missing number")
}

fn octocrab_pr_head(pr: &PullRequest) -> Result<&Head> {
    pr.head
        .as_deref()
        .context("GitHub PR response missing head")
}

fn octocrab_pr_base(pr: &PullRequest) -> Result<&Base> {
    pr.base
        .as_deref()
        .context("GitHub PR response missing base")
}

fn octocrab_pr_state(pr: &PullRequest) -> String {
    pr.state
        .as_ref()
        .map(|s| format!("{:?}", s))
        .unwrap_or_default()
}

fn octocrab_pr_info_with_state(pr: &PullRequest, state: String) -> Result<PrInfo> {
    Ok(PrInfo {
        number: octocrab_pr_number(pr)?,
        state,
        is_draft: pr.draft.unwrap_or(false),
        base: octocrab_pr_base(pr)?.ref_field.clone(),
    })
}

fn octocrab_pr_info(pr: &PullRequest) -> Result<PrInfo> {
    octocrab_pr_info_with_state(pr, octocrab_pr_state(pr))
}

fn octocrab_pr_info_with_head(pr: &PullRequest) -> Result<PrInfoWithHead> {
    let head = octocrab_pr_head(pr)?;

    Ok(PrInfoWithHead {
        head_label: head.label.clone(),
        title: pr.title.clone().unwrap_or_default(),
        info: octocrab_pr_info(pr)?,
        head: head.ref_field.clone(),
    })
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
    pub updated_at: Option<String>,
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
struct PrMergeStatusData {
    repository: Option<PrMergeStatusRepository>,
}

#[derive(Debug, Deserialize)]
struct PrMergeStatusRepository {
    #[serde(rename = "pullRequest")]
    pull_request: Option<PullRequestMergeStatusData>,
}

#[derive(Debug, Deserialize)]
struct PullRequestMergeStatusData {
    number: u64,
    title: String,
    state: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    mergeable: String,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
    #[serde(rename = "statusCheckRollup")]
    status_check_rollup: Option<StatusCheckRollupData>,
    reviews: ReviewConnection,
}

#[derive(Debug, Deserialize)]
struct StatusCheckRollupData {
    state: String,
    contexts: Option<RollupContextConnection>,
}

#[derive(Debug, Deserialize)]
struct RollupContextConnection {
    nodes: Vec<RollupContext>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
enum RollupContext {
    CheckRun(CheckRunContext),
    StatusContext(StatusContextContext),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct CheckRunContext {
    name: String,
    status: String,
    conclusion: Option<String>,
    #[serde(rename = "startedAt")]
    started_at: Option<String>,
    #[serde(rename = "completedAt")]
    completed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StatusContextContext {
    context: String,
    state: String,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReviewConnection {
    nodes: Vec<ReviewNode>,
}

#[derive(Debug, Deserialize)]
struct ReviewNode {
    state: String,
    // Author may be null for reviews left by deleted/ghost users.
    author: Option<ReviewAuthor>,
}

#[derive(Debug, Deserialize)]
struct ReviewAuthor {
    login: String,
}

/// Count effective approvals applying GitHub's per-reviewer latest-wins rules.
///
/// `reviews(last: 100)` returns nodes in chronological order. For each
/// reviewer only `APPROVED`/`CHANGES_REQUESTED`/`DISMISSED` change their review
/// standing — `COMMENTED` and `PENDING` do not clear a prior approval, so they
/// are skipped when determining a reviewer's effective state. The reviewer's
/// last standing-changing review wins; we count reviewers whose effective state
/// is `APPROVED`. Author-less reviews (deleted/ghost users) are grouped under a
/// single key.
fn count_effective_approvals(nodes: &[ReviewNode]) -> usize {
    use std::collections::HashMap;

    let mut effective: HashMap<&str, &str> = HashMap::new();
    for node in nodes {
        match node.state.as_str() {
            "APPROVED" | "CHANGES_REQUESTED" | "DISMISSED" => {
                let key = node.author.as_ref().map(|a| a.login.as_str()).unwrap_or("");
                effective.insert(key, node.state.as_str());
            }
            // COMMENTED / PENDING / unknown states do not change standing.
            _ => {}
        }
    }

    effective
        .values()
        .filter(|state| **state == "APPROVED")
        .count()
}

fn graphql_mergeable_bool(mergeable: &str) -> Option<bool> {
    match mergeable {
        "MERGEABLE" => Some(true),
        "CONFLICTING" => Some(false),
        "UNKNOWN" => None,
        _ => None,
    }
}

fn graphql_mergeable_state(mergeable: &str) -> String {
    match mergeable {
        "MERGEABLE" => "clean",
        "CONFLICTING" => "dirty",
        "UNKNOWN" => "unknown",
        other => other,
    }
    .to_string()
}

fn graphql_check_rollup_status(state: &str) -> CiStatus {
    match state {
        "SUCCESS" => CiStatus::Success,
        "PENDING" | "EXPECTED" => CiStatus::Pending,
        "FAILURE" | "ERROR" => CiStatus::Failure,
        _ => CiStatus::NoCi,
    }
}

/// Compute the effective CI status from a status-check rollup.
///
/// Prefers a deduplicated view of `contexts` (latest run per name wins) over
/// the raw `state` field. GitHub's `state` aggregates every historical context
/// on the commit — so a cancelled-then-rerun-successfully check stays FAILURE
/// even though the latest run passed. Deduping by name avoids that.
///
/// Falls back to `graphql_check_rollup_status(state)` when contexts are
/// unavailable (no permission, empty, or older API responses).
fn rollup_ci_status(rollup: &StatusCheckRollupData) -> CiStatus {
    let Some(connection) = rollup.contexts.as_ref() else {
        return graphql_check_rollup_status(&rollup.state);
    };
    if connection.nodes.is_empty() {
        return graphql_check_rollup_status(&rollup.state);
    }

    // Dedup by name, keeping the latest by started/created timestamp.
    let mut latest: HashMap<&str, (&RollupContext, &str)> = HashMap::new();
    for ctx in &connection.nodes {
        let (name, sort_key) = match ctx {
            RollupContext::CheckRun(c) => (
                c.name.as_str(),
                c.started_at
                    .as_deref()
                    .or(c.completed_at.as_deref())
                    .unwrap_or(""),
            ),
            RollupContext::StatusContext(s) => {
                (s.context.as_str(), s.created_at.as_deref().unwrap_or(""))
            }
            RollupContext::Unknown => continue,
        };
        match latest.get(name) {
            Some((_, existing_key)) if sort_key <= *existing_key => {}
            _ => {
                latest.insert(name, (ctx, sort_key));
            }
        }
    }

    if latest.is_empty() {
        return graphql_check_rollup_status(&rollup.state);
    }

    let mut has_failure = false;
    let mut has_pending = false;
    for (ctx, _) in latest.values() {
        match ctx {
            RollupContext::CheckRun(c) => {
                let status = c.status.to_ascii_uppercase();
                if status != "COMPLETED" {
                    has_pending = true;
                    continue;
                }
                let conclusion = c
                    .conclusion
                    .as_deref()
                    .map(|s| s.to_ascii_uppercase())
                    .unwrap_or_default();
                match conclusion.as_str() {
                    // A *latest* cancelled run (after dedup) means the required
                    // workflow never completed successfully — treat it as a
                    // failure, matching the REST check-run path in client.rs. An
                    // older cancelled run superseded by a later success/pending
                    // run won't reach here, since dedup keeps only the latest
                    // run per check name.
                    "FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
                    | "CANCELLED" => {
                        has_failure = true;
                    }
                    // SUCCESS / NEUTRAL / SKIPPED / STALE / "" → no impact
                    _ => {}
                }
            }
            RollupContext::StatusContext(s) => match s.state.to_ascii_uppercase().as_str() {
                "FAILURE" | "ERROR" => has_failure = true,
                "PENDING" | "EXPECTED" => has_pending = true,
                _ => {}
            },
            RollupContext::Unknown => {}
        }
    }

    if has_failure {
        CiStatus::Failure
    } else if has_pending {
        CiStatus::Pending
    } else {
        CiStatus::Success
    }
}

impl GitHubClient {
    async fn graphql_data<T: DeserializeOwned>(&self, payload: serde_json::Value) -> Result<T> {
        match self.octocrab.graphql(&payload).await {
            Ok(data) => Ok(data),
            Err(octocrab::Error::Graphql { source, .. }) => {
                let message = source
                    .0
                    .first()
                    .map(|error| error.message.as_str())
                    .unwrap_or("unknown GraphQL error");
                anyhow::bail!("GraphQL error: {}", message)
            }
            Err(err) => Err(anyhow::Error::new(err)),
        }
    }

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
        let prs = match self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .list()
            .state(State::Open)
            .head(format!("{}:{}", head_owner, branch))
            .per_page(100u8)
            .sort(Sort::Created)
            .send()
            .await
            .context("Failed to list PRs by head")
        {
            Ok(prs) => prs,
            Err(e) => return Err(self.enrich_api_error(e)),
        };

        for pr in &prs.items {
            let head = octocrab_pr_head(pr)?;
            if head.ref_field != branch {
                continue;
            }
            let owner_matches = head
                .label
                .as_ref()
                .and_then(|label| label.split_once(':').map(|(owner, _)| owner == head_owner))
                .unwrap_or(true);
            if !owner_matches {
                continue;
            }

            return Ok(Some(octocrab_pr_info_with_head(pr)?));
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
            let prs = match self
                .octocrab
                .pulls(&self.owner, &self.repo)
                .list()
                .state(State::Open)
                .per_page(PER_PAGE)
                .page(page)
                .sort(Sort::Created)
                .send()
                .await
                .context("Failed to list PRs")
            {
                Ok(prs) => prs,
                Err(e) => return Err(self.enrich_api_error(e)),
            };

            for pr in &prs.items {
                let head = octocrab_pr_head(pr)?.ref_field.clone();
                if prs_by_head.contains_key(&head) {
                    continue;
                }

                prs_by_head.insert(head, octocrab_pr_info_with_head(pr)?);
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

        octocrab_pr_info(&pr)
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

        let state = if pr.merged_at.is_some() {
            "MERGED".to_string()
        } else {
            pr.state
                .as_ref()
                .map(|s| format!("{:?}", s))
                .unwrap_or_default()
                .to_uppercase()
        };

        octocrab_pr_info_with_state(&pr, state)
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

        octocrab_pr_info_with_head(&pr)
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

        let _: serde_json::Value = self
            .graphql_data(serde_json::json!({ "query": mutation }))
            .await
            .context("Failed to update PR draft status")?;

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

    /// Add a plain issue comment to a PR conversation.
    pub async fn create_issue_comment(&self, pr_number: u64, body: &str) -> Result<()> {
        self.record_api_call("issues.comments.create");
        self.octocrab
            .issues(&self.owner, &self.repo)
            .create_comment(pr_number, body)
            .await
            .context("Failed to create comment")?;

        Ok(())
    }

    /// Close a PR without merging it.
    pub async fn close_pr(&self, pr_number: u64) -> Result<()> {
        self.record_api_call("pulls.update.state");
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .update(pr_number)
            .state(octocrab::params::pulls::State::Closed)
            .send()
            .await
            .context("Failed to close PR")?;

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
        sha: Option<String>,
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

        if let Some(ref sha) = sha {
            merge_builder = merge_builder.sha(sha);
        }

        merge_builder.send().await.map_err(format_merge_error)?;

        Ok(())
    }

    /// Get detailed merge status for a PR
    pub async fn get_pr_merge_status(&self, pr_number: u64) -> Result<PrMergeStatus> {
        self.record_api_call("graphql.pr_merge_status");
        let query = format!(
            r#"
            query {{
                repository(owner: "{}", name: "{}") {{
                    pullRequest(number: {}) {{
                        number
                        title
                        state
                        updatedAt
                        isDraft
                        mergeable
                        reviewDecision
                        headRefOid
                        statusCheckRollup {{
                            state
                            contexts(first: 100) {{
                                nodes {{
                                    __typename
                                    ... on CheckRun {{
                                        name
                                        status
                                        conclusion
                                        startedAt
                                        completedAt
                                    }}
                                    ... on StatusContext {{
                                        context
                                        state
                                        createdAt
                                    }}
                                }}
                            }}
                        }}
                        reviews(last: 100) {{
                            nodes {{
                                state
                                author {{
                                    login
                                }}
                            }}
                        }}
                    }}
                }}
            }}
            "#,
            self.owner, self.repo, pr_number
        );

        let data: PrMergeStatusData = self
            .graphql_data(serde_json::json!({ "query": query }))
            .await
            .context("Failed to query PR merge status")?;

        let repository = data
            .repository
            .context("GraphQL response did not include repository data")?;
        let pr = repository
            .pull_request
            .context("GraphQL response did not include pull request merge status data")?;

        let approvals = count_effective_approvals(&pr.reviews.nodes);
        // The reviews list retains historical events, so scanning it would let
        // a superseded CHANGES_REQUESTED review keep blocking the PR. Rely on
        // reviewDecision, which already applies per-reviewer latest-wins logic.
        let changes_requested = pr.review_decision.as_deref() == Some("CHANGES_REQUESTED");
        let mergeable = graphql_mergeable_bool(&pr.mergeable);
        let mergeable_state = graphql_mergeable_state(&pr.mergeable);
        let ci_status = pr
            .status_check_rollup
            .as_ref()
            .map(rollup_ci_status)
            .unwrap_or(CiStatus::NoCi);

        Ok(PrMergeStatus {
            number: pr.number,
            title: pr.title,
            state: pr.state.to_ascii_lowercase(),
            updated_at: Some(pr.updated_at),
            is_draft: pr.is_draft,
            mergeable,
            mergeable_state,
            ci_status,
            review_decision: pr.review_decision,
            approvals,
            changes_requested,
            head_sha: pr.head_ref_oid,
        })
    }

    /// Return the overall review decision for a PR (`"APPROVED"`, `"CHANGES_REQUESTED"`,
    /// `"REVIEW_REQUIRED"`, or `None` when there is no review requirement).
    pub async fn get_pr_review_decision(&self, pr_number: u64) -> Result<Option<String>> {
        let (decision, _, _) = self.get_pr_reviews(pr_number).await?;
        Ok(decision)
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
                                author {{
                                    login
                                }}
                            }}
                        }}
                    }}
                }}
            }}
            "#,
            self.owner, self.repo, pr_number
        );

        let data: PrReviewData = self
            .graphql_data(serde_json::json!({ "query": query }))
            .await
            .context("Failed to query PR reviews")?;

        let repository = data
            .repository
            .context("GraphQL response did not include repository data")?;
        let pr = repository
            .pull_request
            .context("GraphQL response did not include pull request review data")?;

        let approvals = count_effective_approvals(&pr.reviews.nodes);
        let changes_requested = pr.review_decision.as_deref() == Some("CHANGES_REQUESTED");
        let review_decision = pr.review_decision;

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

        let data: PrNodeIdData = self
            .graphql_data(serde_json::json!({ "query": query }))
            .await
            .context("Failed to query PR node ID")?;

        data.repository
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

        let data: EnqueueData = self
            .graphql_data(serde_json::json!({ "query": mutation }))
            .await
            .context("Failed to enqueue PR into merge queue")?;

        data.enqueue_pull_request
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

    /// Return the PR's current head commit SHA. One `pulls.get` call, no
    /// CI/review fan-out — used by the post-push sync helper that only
    /// needs the head ref, not full merge status.
    pub async fn get_pr_head_sha(&self, pr_number: u64) -> Result<String> {
        self.record_api_call("pulls.get");
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .get(pr_number)
            .await
            .context("Failed to get PR")?;
        Ok(octocrab_pr_head(&pr)?.sha.clone())
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

/// Build a detailed error when octocrab returns a GitHub API failure on merge.
///
/// octocrab's default Display for `Error::GitHub` doesn't surface the HTTP
/// status code, so merges that fail against `PUT /repos/.../pulls/{n}/merge`
/// previously reported only "Failed to merge PR". This helper extracts the
/// status code, server-provided message, errors array, and docs URL so
/// callers can show something actionable like
/// `GitHub API 405: Base branch was modified, review and try the merge again`.
fn format_merge_error(err: octocrab::Error) -> anyhow::Error {
    if let octocrab::Error::GitHub { source, .. } = &err {
        let mut msg = format!(
            "GitHub API {}: {}",
            source.status_code.as_u16(),
            source.message
        );
        if let Some(errors) = source.errors.as_ref() {
            for item in errors.iter() {
                msg.push_str(&format!("\n  - {}", item));
            }
        }
        if let Some(url) = source.documentation_url.as_ref()
            && !url.is_empty()
        {
            msg.push_str(&format!("\n  docs: {}", url));
        }
        return anyhow::Error::new(err).context(msg);
    }
    anyhow::Error::new(err).context("Failed to merge PR")
}

/// PR info for stack comment generation
#[derive(Debug, Clone)]
pub struct StackPrInfo {
    pub branch: String,
    pub pr_number: Option<u64>,
    pub is_imported: bool,
    /// 1-based distance from trunk (number of ancestors, trunk included).
    /// Used to indent this entry in the rendered Stack Links list — siblings
    /// sharing a parent (a forked local stack) get the same depth instead of
    /// being nested under one another by list position. Callers that only
    /// use a `StackPrInfo` as a branch/PR-number lookup key (not for
    /// rendering) may pass `0`.
    pub depth: usize,
}

fn stack_links_intro(prs: &[StackPrInfo], current_index: Option<usize>, mr_label: &str) -> String {
    let current = current_index.and_then(|index| prs.get(index).map(|pr| (index, pr)));

    match current {
        Some((_, current)) if current.is_imported => format!(
            "This {} is an imported reference. Entries below it are local stack branches; Stax keeps these links in sync without pushing or updating the imported branch:",
            mr_label
        ),
        Some((current, _))
            if prs
                .iter()
                .enumerate()
                .any(|(index, pr)| index < current && pr.is_imported) =>
        {
            format!(
                "This {} is a local stack branch. Imported downstack entries are read-only context, and local stack branches are shown in stack order:",
                mr_label
            )
        }
        Some(_) => format!("This {} is part of a local stacked series:", mr_label),
        None => format!("This {} is part of a stacked series:", mr_label),
    }
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

    let current_index = prs
        .iter()
        .position(|pr_info| pr_info.pr_number == Some(current_pr_number));

    let mut lines = vec![
        "## Stack Links".to_string(),
        "".to_string(),
        stack_links_intro(prs, current_index, mr_label),
        "".to_string(),
        format!("* `{}`", trunk),
    ];

    // Build stack from bottom (trunk-adjacent) to top (leaf)
    // First PR is closest to trunk, last is the leaf
    for (i, pr_info) in prs.iter().enumerate() {
        let pointer = if Some(i) == current_index {
            " 👈"
        } else {
            ""
        };

        let pr_text = match pr_info.pr_number {
            Some(num) => {
                let label = format!("{} {}{}", mr_label, mr_prefix, num);
                match remote.forge {
                    ForgeType::GitHub => format!("**{}**{}", label, pointer),
                    _ => format!("[**{}**]({}){}", label, remote.pr_url(num), pointer),
                }
            }
            None => format!("`{}`{}", pr_info.branch, pointer),
        };

        // Indent based on actual depth from trunk (2 spaces per level), not
        // list position — a forked stack has siblings at the same depth.
        let indent = "  ".repeat(pr_info.depth.max(1));
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
    fn is_native_stack_base_locked_error_matches_githubs_exact_wording() {
        let err = anyhow::anyhow!(
            "Cannot change the base branch because the pull request is part of a stack."
        )
        .context("Failed to update PR base");

        assert!(is_native_stack_base_locked_error(&err));
    }

    #[test]
    fn is_native_stack_base_locked_error_ignores_unrelated_validation_errors() {
        let err = anyhow::anyhow!(
            "A pull request already exists for base branch 'main' and head branch 'feature'"
        )
        .context("Failed to update PR base");

        assert!(!is_native_stack_base_locked_error(&err));
    }

    #[test]
    fn is_native_stack_base_locked_error_ignores_generic_errors() {
        let err = anyhow::anyhow!("connection reset by peer").context("Failed to update PR base");

        assert!(!is_native_stack_base_locked_error(&err));
    }

    #[test]
    fn is_native_stack_base_locked_error_does_not_misfire_on_negated_wording() {
        // Guards against a hypothetical future GitHub message that negates
        // stack membership — must not collide with the lock detector.
        let err = anyhow::anyhow!("The pull request is not part of a stack, so this is a no-op")
            .context("Failed to update PR base");

        assert!(!is_native_stack_base_locked_error(&err));
    }

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
            updated_at: None,
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
            updated_at: None,
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
            updated_at: None,
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
            updated_at: None,
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
            updated_at: None,
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
            updated_at: None,
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
            updated_at: None,
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
            updated_at: None,
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
            is_imported: false,
            depth: 1,
        }];

        let comment = generate_stack_comment(&prs, 1, &remote, "main");

        assert!(comment.contains("## Stack Links"));
        assert!(comment.contains("This PR is part of a local stacked series:"));
        assert!(comment.contains("`main`"));
        assert!(comment.contains("PR #1"));
        assert!(comment.contains("**PR #1** 👈"));
        assert!(!comment.contains("current local branch"));
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
                is_imported: false,
                depth: 1,
            },
            StackPrInfo {
                branch: "feature-b".to_string(),
                pr_number: Some(2),
                is_imported: false,
                depth: 2,
            },
            StackPrInfo {
                branch: "feature-c".to_string(),
                pr_number: Some(3),
                is_imported: false,
                depth: 3,
            },
        ];

        let comment = generate_stack_comment(&prs, 2, &remote, "main");

        assert!(comment.contains("PR #1"));
        assert!(comment.contains("PR #2"));
        assert!(comment.contains("PR #3"));
        // Only PR #2 should have the pointer
        assert!(comment.contains("  * **PR #1**\n    * **PR #2** 👈\n      * **PR #3**"));
        assert!(!comment.contains("current local branch"));
        assert!(!comment.contains("local upstack"));
    }

    #[test]
    fn test_generate_stack_comment_intro_relative_to_current_pr() {
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
                branch: "imported-base".to_string(),
                pr_number: Some(10),
                is_imported: true,
                depth: 1,
            },
            StackPrInfo {
                branch: "local-middle".to_string(),
                pr_number: Some(20),
                is_imported: false,
                depth: 2,
            },
            StackPrInfo {
                branch: "local-tip".to_string(),
                pr_number: Some(30),
                is_imported: false,
                depth: 3,
            },
        ];

        let base_comment = generate_stack_comment(&prs, 10, &remote, "main");
        assert!(base_comment.contains(
            "This PR is an imported reference. Entries below it are local stack branches; Stax keeps these links in sync without pushing or updating the imported branch:"
        ));
        assert!(base_comment.contains("  * **PR #10** 👈\n    * **PR #20**\n      * **PR #30**"));
        assert!(!base_comment.contains("current imported reference"));
        assert!(!base_comment.contains("local upstack"));

        let middle_comment = generate_stack_comment(&prs, 20, &remote, "main");
        assert!(middle_comment.contains(
            "This PR is a local stack branch. Imported downstack entries are read-only context, and local stack branches are shown in stack order:"
        ));
        assert!(middle_comment.contains("  * **PR #10**\n    * **PR #20** 👈\n      * **PR #30**"));
        assert!(!middle_comment.contains("imported reference downstack"));
        assert!(!middle_comment.contains("current local branch"));
        assert!(!middle_comment.contains("local upstack"));

        let tip_comment = generate_stack_comment(&prs, 30, &remote, "main");
        assert!(tip_comment.contains(
            "This PR is a local stack branch. Imported downstack entries are read-only context, and local stack branches are shown in stack order:"
        ));
        assert!(tip_comment.contains("  * **PR #10**\n    * **PR #20**\n      * **PR #30** 👈"));
        assert!(!tip_comment.contains("imported reference downstack"));
        assert!(!tip_comment.contains("local downstack"));
        assert!(!tip_comment.contains("current local branch"));
    }

    /// Siblings that share a parent (a forked local stack) must render at
    /// the same indent, not nested one under the other — depth comes from
    /// each entry's actual position in the branch tree, not its position in
    /// the list.
    #[test]
    fn test_generate_stack_comment_renders_forked_siblings_at_equal_depth() {
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
                branch: "bottom".to_string(),
                pr_number: Some(1),
                is_imported: false,
                depth: 1,
            },
            StackPrInfo {
                branch: "fork-a".to_string(),
                pr_number: Some(2),
                is_imported: false,
                depth: 2,
            },
            StackPrInfo {
                branch: "fork-b".to_string(),
                pr_number: Some(3),
                is_imported: false,
                depth: 2,
            },
        ];

        let comment = generate_stack_comment(&prs, 1, &remote, "main");

        assert!(
            comment.contains("  * **PR #1** 👈\n    * **PR #2**\n    * **PR #3**"),
            "fork-a and fork-b share a parent and must be indented equally, got:\n{comment}"
        );
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
                is_imported: false,
                depth: 1,
            },
            StackPrInfo {
                branch: "feature-b".to_string(),
                pr_number: None, // No PR yet
                is_imported: true,
                depth: 2,
            },
        ];

        let comment = generate_stack_comment(&prs, 1, &remote, "main");

        assert!(comment.contains("PR #1"));
        assert!(comment.contains("`feature-b`"));
        assert!(!comment.contains("imported reference upstack"));
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
                is_imported: false,
                depth: 1,
            },
            StackPrInfo {
                branch: "feature-b".to_string(),
                pr_number: Some(11),
                is_imported: false,
                depth: 2,
            },
        ];

        let comment = generate_stack_comment(&prs, 11, &remote, "main");

        // GitLab uses MR terminology and !N prefix
        assert!(comment.contains("This MR is part of a local stacked series:"));
        assert!(comment.contains("[**MR !10**](https://gitlab.com/user/repo/-/merge_requests/10)"));
        assert!(
            comment.contains("[**MR !11**](https://gitlab.com/user/repo/-/merge_requests/11) 👈")
        );
        assert!(!comment.contains("current local branch"));
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
            is_imported: false,
            depth: 1,
        }];

        let comment = generate_stack_comment(&prs, 5, &remote, "main");

        // Gitea uses PR terminology but needs full URLs
        assert!(comment.contains("This PR is part of a local stacked series:"));
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
            is_imported: false,
            depth: 1,
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
            is_imported: false,
            depth: 1,
        };
        let cloned = info.clone();
        assert_eq!(cloned.branch, "feature");
        assert_eq!(cloned.pr_number, Some(42));
        assert!(!cloned.is_imported);
    }

    #[test]
    fn test_pr_merge_status_clone() {
        let status = PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "Open".to_string(),
            updated_at: None,
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
    async fn test_get_pr_merge_status_maps_graphql_fields() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "repository": {
                        "pullRequest": {
                            "number": 11,
                            "title": "Ready-looking PR",
                            "state": "OPEN",
                            "updatedAt": "2026-06-02T10:00:00Z",
                            "isDraft": false,
                            "mergeable": "MERGEABLE",
                            "reviewDecision": "APPROVED",
                            "headRefOid": "aaaa",
                            "statusCheckRollup": { "state": "SUCCESS" },
                            "reviews": {
                                "nodes": [
                                    { "state": "APPROVED", "author": { "login": "alice" } },
                                    { "state": "COMMENTED", "author": { "login": "bob" } }
                                ]
                            }
                        }
                    }
                }
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(11).await.unwrap();

        assert_eq!(status.number, 11);
        assert_eq!(status.title, "Ready-looking PR");
        assert_eq!(status.state, "open");
        assert_eq!(status.updated_at.as_deref(), Some("2026-06-02T10:00:00Z"));
        assert_eq!(status.mergeable, Some(true));
        assert_eq!(status.mergeable_state, "clean");
        assert_eq!(status.review_decision.as_deref(), Some("APPROVED"));
        assert_eq!(status.approvals, 1);
        assert!(!status.changes_requested);
        assert_eq!(status.ci_status, CiStatus::Success);
        assert_eq!(status.head_sha, "aaaa");
    }

    // A superseded CHANGES_REQUESTED review must not block a PR whose
    // reviewDecision is APPROVED — the reviews list retains historical events.
    #[tokio::test]
    async fn test_get_pr_merge_status_ignores_stale_changes_requested_review() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "repository": {
                        "pullRequest": {
                            "number": 12,
                            "title": "Re-approved PR",
                            "state": "OPEN",
                            "updatedAt": "2026-06-03T10:00:00Z",
                            "isDraft": false,
                            "mergeable": "MERGEABLE",
                            "reviewDecision": "APPROVED",
                            "headRefOid": "bbbb",
                            "statusCheckRollup": { "state": "SUCCESS" },
                            "reviews": {
                                "nodes": [
                                    { "state": "CHANGES_REQUESTED", "author": { "login": "alice" } },
                                    { "state": "APPROVED", "author": { "login": "alice" } }
                                ]
                            }
                        }
                    }
                }
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(12).await.unwrap();

        assert_eq!(status.review_decision.as_deref(), Some("APPROVED"));
        assert!(!status.changes_requested);
        // Same reviewer requested changes then approved: latest-wins → 1 approval.
        assert_eq!(status.approvals, 1);
    }

    // Builds a merge-status mock response with the given review nodes.
    fn merge_status_body(reviews: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "data": {
                "repository": {
                    "pullRequest": {
                        "number": 20,
                        "title": "Approval counting PR",
                        "state": "OPEN",
                        "updatedAt": "2026-06-04T10:00:00Z",
                        "isDraft": false,
                        "mergeable": "MERGEABLE",
                        "reviewDecision": "APPROVED",
                        "headRefOid": "cccc",
                        "statusCheckRollup": { "state": "SUCCESS" },
                        "reviews": { "nodes": reviews }
                    }
                }
            }
        })
    }

    #[tokio::test]
    async fn test_merge_status_counts_repeat_approvals_once() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(merge_status_body(
                serde_json::json!([
                    { "state": "APPROVED", "author": { "login": "alice" } },
                    { "state": "APPROVED", "author": { "login": "alice" } }
                ]),
            )))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(20).await.unwrap();

        assert_eq!(status.approvals, 1);
    }

    #[tokio::test]
    async fn test_merge_status_approval_superseded_by_changes_requested() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(merge_status_body(
                serde_json::json!([
                    { "state": "APPROVED", "author": { "login": "alice" } },
                    { "state": "CHANGES_REQUESTED", "author": { "login": "alice" } }
                ]),
            )))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(20).await.unwrap();

        assert_eq!(status.approvals, 0);
    }

    #[tokio::test]
    async fn test_merge_status_comment_does_not_clear_approval() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(merge_status_body(
                serde_json::json!([
                    { "state": "APPROVED", "author": { "login": "alice" } },
                    { "state": "COMMENTED", "author": { "login": "alice" } }
                ]),
            )))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(20).await.unwrap();

        assert_eq!(status.approvals, 1);
    }

    #[tokio::test]
    async fn test_merge_status_counts_distinct_reviewers() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(merge_status_body(
                serde_json::json!([
                    { "state": "APPROVED", "author": { "login": "alice" } },
                    { "state": "APPROVED", "author": { "login": "bob" } }
                ]),
            )))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(20).await.unwrap();

        assert_eq!(status.approvals, 2);
    }

    fn merge_status_body_with_rollup(rollup: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "data": {
                "repository": {
                    "pullRequest": {
                        "number": 30,
                        "title": "Rollup PR",
                        "state": "OPEN",
                        "updatedAt": "2026-06-16T10:00:00Z",
                        "isDraft": false,
                        "mergeable": "MERGEABLE",
                        "reviewDecision": "APPROVED",
                        "headRefOid": "rollup-head",
                        "statusCheckRollup": rollup,
                        "reviews": {
                            "nodes": [
                                { "state": "APPROVED", "author": { "login": "alice" } }
                            ]
                        }
                    }
                }
            }
        })
    }

    // GitHub's rollup `state` aggregates every historical context, so a
    // cancelled-then-rerun-successfully check leaves the rollup at FAILURE.
    // Dedup by name (latest wins) reflects the actual current CI state.
    #[tokio::test]
    async fn test_get_pr_merge_status_dedups_cancelled_then_success() {
        let mock_server = MockServer::start().await;

        let rollup = serde_json::json!({
            "state": "FAILURE",
            "contexts": {
                "nodes": [
                    {
                        "__typename": "CheckRun",
                        "name": "checklist",
                        "status": "COMPLETED",
                        "conclusion": "CANCELLED",
                        "startedAt": "2026-06-16T22:21:51Z",
                        "completedAt": "2026-06-16T22:21:51Z"
                    },
                    {
                        "__typename": "CheckRun",
                        "name": "checklist",
                        "status": "COMPLETED",
                        "conclusion": "SUCCESS",
                        "startedAt": "2026-06-16T22:21:54Z",
                        "completedAt": "2026-06-16T22:26:55Z"
                    },
                    {
                        "__typename": "CheckRun",
                        "name": "compile_protos",
                        "status": "COMPLETED",
                        "conclusion": "CANCELLED",
                        "startedAt": "2026-06-16T22:21:50Z",
                        "completedAt": "2026-06-16T22:21:51Z"
                    },
                    {
                        "__typename": "CheckRun",
                        "name": "compile_protos",
                        "status": "COMPLETED",
                        "conclusion": "SUCCESS",
                        "startedAt": "2026-06-16T22:21:54Z",
                        "completedAt": "2026-06-16T22:22:53Z"
                    }
                ]
            }
        });

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(merge_status_body_with_rollup(rollup)),
            )
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(30).await.unwrap();

        assert_eq!(status.ci_status, CiStatus::Success);
    }

    // Cancelled re-run with a later run still in progress → pending.
    #[tokio::test]
    async fn test_get_pr_merge_status_dedups_cancelled_then_running() {
        let mock_server = MockServer::start().await;

        let rollup = serde_json::json!({
            "state": "FAILURE",
            "contexts": {
                "nodes": [
                    {
                        "__typename": "CheckRun",
                        "name": "checklist",
                        "status": "COMPLETED",
                        "conclusion": "CANCELLED",
                        "startedAt": "2026-06-16T22:21:51Z",
                        "completedAt": "2026-06-16T22:21:51Z"
                    },
                    {
                        "__typename": "CheckRun",
                        "name": "checklist",
                        "status": "IN_PROGRESS",
                        "conclusion": null,
                        "startedAt": "2026-06-16T22:21:54Z",
                        "completedAt": null
                    }
                ]
            }
        });

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(merge_status_body_with_rollup(rollup)),
            )
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(30).await.unwrap();

        assert_eq!(status.ci_status, CiStatus::Pending);
    }

    // A real failure (no later passing rerun) is preserved.
    #[tokio::test]
    async fn test_get_pr_merge_status_keeps_real_failure() {
        let mock_server = MockServer::start().await;

        let rollup = serde_json::json!({
            "state": "FAILURE",
            "contexts": {
                "nodes": [
                    {
                        "__typename": "CheckRun",
                        "name": "build",
                        "status": "COMPLETED",
                        "conclusion": "FAILURE",
                        "startedAt": "2026-06-16T22:21:51Z",
                        "completedAt": "2026-06-16T22:22:51Z"
                    },
                    {
                        "__typename": "CheckRun",
                        "name": "lint",
                        "status": "COMPLETED",
                        "conclusion": "SUCCESS",
                        "startedAt": "2026-06-16T22:21:51Z",
                        "completedAt": "2026-06-16T22:21:55Z"
                    }
                ]
            }
        });

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(merge_status_body_with_rollup(rollup)),
            )
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(30).await.unwrap();

        assert_eq!(status.ci_status, CiStatus::Failure);
    }

    // The latest run per check name is CANCELLED (never rerun) → failure.
    // Mirrors the REST check-run path, which treats cancelled as a failure.
    #[tokio::test]
    async fn test_get_pr_merge_status_latest_cancelled_is_failure() {
        let mock_server = MockServer::start().await;

        let rollup = serde_json::json!({
            "state": "FAILURE",
            "contexts": {
                "nodes": [
                    {
                        "__typename": "CheckRun",
                        "name": "build",
                        "status": "COMPLETED",
                        "conclusion": "SUCCESS",
                        "startedAt": "2026-06-16T22:21:50Z",
                        "completedAt": "2026-06-16T22:21:51Z"
                    },
                    {
                        "__typename": "CheckRun",
                        "name": "build",
                        "status": "COMPLETED",
                        "conclusion": "CANCELLED",
                        "startedAt": "2026-06-16T22:21:54Z",
                        "completedAt": "2026-06-16T22:26:55Z"
                    }
                ]
            }
        });

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(merge_status_body_with_rollup(rollup)),
            )
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(30).await.unwrap();

        assert_eq!(status.ci_status, CiStatus::Failure);
    }

    // A failed Buildkite StatusContext (legacy commit status) still counts.
    #[tokio::test]
    async fn test_get_pr_merge_status_uses_status_context_failures() {
        let mock_server = MockServer::start().await;

        let rollup = serde_json::json!({
            "state": "FAILURE",
            "contexts": {
                "nodes": [
                    {
                        "__typename": "StatusContext",
                        "context": "buildkite/presubmit",
                        "state": "FAILURE",
                        "createdAt": "2026-06-16T22:22:00Z"
                    }
                ]
            }
        });

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(merge_status_body_with_rollup(rollup)),
            )
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(30).await.unwrap();

        assert_eq!(status.ci_status, CiStatus::Failure);
    }

    // No contexts → fall back to the rollup `state` (back-compat with older responses).
    #[tokio::test]
    async fn test_get_pr_merge_status_falls_back_to_state_without_contexts() {
        let mock_server = MockServer::start().await;

        let rollup = serde_json::json!({
            "state": "FAILURE"
        });

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(merge_status_body_with_rollup(rollup)),
            )
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(30).await.unwrap();

        assert_eq!(status.ci_status, CiStatus::Failure);
    }

    #[tokio::test]
    async fn test_get_pr_merge_status_fails_on_graphql_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "errors": [{ "message": "rate limit exceeded" }]
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let error = client.get_pr_merge_status(11).await.unwrap_err();
        let error = format!("{error:#}");

        assert!(error.contains("GraphQL error: rate limit exceeded"));
    }

    #[tokio::test]
    async fn test_get_pr_merge_status_skips_reviews_for_closed_pr() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "repository": {
                        "pullRequest": {
                            "number": 11,
                            "title": "Already merged PR",
                            "state": "CLOSED",
                            "updatedAt": "2026-06-01T10:00:00Z",
                            "isDraft": false,
                            "mergeable": "MERGEABLE",
                            "reviewDecision": null,
                            "headRefOid": "aaaa",
                            "statusCheckRollup": { "state": "SUCCESS" },
                            "reviews": { "nodes": [] }
                        }
                    }
                }
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let status = client.get_pr_merge_status(11).await.unwrap();

        assert_eq!(status.status_text(), "Closed");
        assert!(!status.is_ready());
        assert_eq!(status.review_decision, None);
        assert_eq!(status.approvals, 0);
        assert_eq!(status.ci_status, CiStatus::Success);
        assert_eq!(status.mergeable, Some(true));
    }

    #[tokio::test]
    async fn test_get_pr_merge_status_fails_on_missing_pr_data() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "repository": { "pullRequest": null } }
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let error = client.get_pr_merge_status(11).await.unwrap_err();
        let error = format!("{error:#}");

        assert!(error.contains("pull request merge status data"));
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
        assert!(
            stats
                .by_operation
                .iter()
                .any(|(op, count)| op == "pulls.list.open.page" && *count == 1)
        );
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
        assert!(
            stats
                .by_operation
                .iter()
                .any(|(op, count)| op == "pulls.list.head" && *count == 1)
        );
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
        assert!(
            stats
                .by_operation
                .iter()
                .any(|(op, count)| op == "pulls.list.head" && *count == 1)
        );
        assert!(
            stats
                .by_operation
                .iter()
                .any(|(op, count)| op == "pulls.list.open.page" && *count == 1)
        );
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
        assert!(
            stats
                .by_operation
                .iter()
                .any(|(op, count)| op == "pulls.get" && *count == 1)
        );
    }

    #[tokio::test]
    async fn test_get_pr_with_head_errors_when_response_missing_head() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/11"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test-owner/test-repo/pulls/11",
                "id": 11,
                "number": 11,
                "base": { "ref": "main", "sha": "bbbb" },
                "draft": false
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let err = client
            .get_pr_with_head(11)
            .await
            .expect_err("missing head should fail");

        let msg = format!("{:#}", err);
        assert!(
            msg.contains("GitHub PR response missing head"),
            "expected missing head context, got: {msg}"
        );
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

    #[tokio::test]
    async fn test_merge_pr_surfaces_github_error_details() {
        let mock_server = MockServer::start().await;

        Mock::given(method("PUT"))
            .and(path("/repos/test-owner/test-repo/pulls/42/merge"))
            .respond_with(ResponseTemplate::new(405).set_body_json(serde_json::json!({
                "message": "Base branch was modified. Review and try the merge again.",
                "documentation_url": "https://docs.github.com/rest/pulls/pulls#merge-a-pull-request"
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let result = client
            .merge_pr(42, MergeMethod::Squash, None, None, None)
            .await;

        let err = result.expect_err("merge_pr should fail on 405");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("405"),
            "expected HTTP status in error, got: {msg}"
        );
        assert!(
            msg.contains("Base branch was modified"),
            "expected GitHub message in error, got: {msg}"
        );
        assert!(
            msg.contains("docs.github.com"),
            "expected documentation URL in error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_merge_pr_surfaces_unprocessable_entity_errors_array() {
        let mock_server = MockServer::start().await;

        Mock::given(method("PUT"))
            .and(path("/repos/test-owner/test-repo/pulls/7/merge"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "message": "Pull Request is not mergeable",
                "errors": ["Head branch was modified"],
                "documentation_url": "https://docs.github.com/rest/pulls"
            })))
            .mount(&mock_server)
            .await;

        let client = create_test_client(&mock_server).await;
        let result = client
            .merge_pr(7, MergeMethod::Merge, None, None, None)
            .await;

        let err = result.expect_err("merge_pr should fail on 422");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("422"),
            "expected HTTP status in error, got: {msg}"
        );
        assert!(
            msg.contains("Pull Request is not mergeable"),
            "expected server message in error, got: {msg}"
        );
        assert!(
            msg.contains("Head branch was modified"),
            "expected errors array item in error, got: {msg}"
        );
    }
}
