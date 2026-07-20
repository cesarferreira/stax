use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::str::FromStr;

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

impl FromStr for MergeMethod {
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

#[derive(Debug, serde::Deserialize)]
pub struct EnqueueResult {
    #[serde(rename = "mergeQueueEntry")]
    pub merge_queue_entry: Option<MergeQueueEntry>,
}

#[derive(Debug, serde::Deserialize)]
pub struct MergeQueueEntry {
    pub position: Option<u32>,
}

/// Open PR info for tracking command
#[derive(Debug, Clone)]
pub struct OpenPrInfo {
    pub number: u64,
    pub head_branch: String,
    pub base_branch: String,
    pub state: String,
    pub is_draft: bool,
}

/// PR activity for standup reports.
#[derive(Debug, Clone, Serialize)]
pub struct PrActivity {
    pub number: u64,
    pub title: String,
    pub timestamp: DateTime<Utc>,
    pub url: String,
}

/// Review activity for standup reports.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewActivity {
    pub pr_number: u64,
    pub pr_title: String,
    pub reviewer: String,
    pub state: String,
    pub timestamp: DateTime<Utc>,
    pub is_received: bool,
}

/// Open pull request info for repo-level listing commands.
#[derive(Debug, Clone, Serialize)]
pub struct RepoPrListItem {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub head_branch: String,
    pub base_branch: String,
    pub state: String,
    pub is_draft: bool,
    pub created_at: DateTime<Utc>,
}

/// Open issue info for repo-level listing commands.
#[derive(Debug, Clone, Serialize)]
pub struct RepoIssueListItem {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub labels: Vec<String>,
    pub updated_at: DateTime<Utc>,
}
