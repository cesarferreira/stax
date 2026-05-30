//! `stax inbox` — an attention-prioritized "PR cockpit".
//!
//! Fuses the local stack, the CI cache, live PR merge-status, and worktree
//! presence into one ranked list that answers "which PR should I touch next?".
//!
//! Each tracked branch becomes an [`InboxItem`] sorted into one of a few
//! [`Bucket`]s with a derived [`NextAction`]. Phase 1 emits this as JSON (and a
//! minimal grouped text view); richer rendering and caching land in later
//! phases.

use crate::cache::CiCache;
use crate::config::Config;
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::github::pr::{CiStatus, PrMergeStatus};
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;
use futures_util::future::join_all;
use serde::Serialize;
use std::collections::HashMap;

/// Which attention group a PR/branch falls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    /// You are blocked: CI failed, changes requested, or the branch needs a restack.
    NeedsYou,
    /// An agent is actively working this branch (it has a linked worktree).
    AgentRunning,
    /// Approved + green + mergeable.
    ReadyToMerge,
    /// Open and healthy, waiting on review or CI.
    WaitingOnOthers,
    /// Drafts and anything without a clear next step.
    Other,
}

impl Bucket {
    /// Stable display order for grouped output.
    fn order(self) -> u8 {
        match self {
            Bucket::NeedsYou => 0,
            Bucket::AgentRunning => 1,
            Bucket::ReadyToMerge => 2,
            Bucket::WaitingOnOthers => 3,
            Bucket::Other => 4,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Bucket::NeedsYou => "Needs you",
            Bucket::AgentRunning => "Agent running",
            Bucket::ReadyToMerge => "Ready to merge",
            Bucket::WaitingOnOthers => "Waiting on others",
            Bucket::Other => "Other",
        }
    }
}

/// The single suggested next step for a branch/PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    FixCi,
    AddressComments,
    Restack,
    ContinueAgent,
    Merge,
    WaitForReview,
    None,
}

impl NextAction {
    fn label(self) -> &'static str {
        match self {
            NextAction::FixCi => "fix CI",
            NextAction::AddressComments => "address comments",
            NextAction::Restack => "restack",
            NextAction::ContinueAgent => "continue agent",
            NextAction::Merge => "merge",
            NextAction::WaitForReview => "wait for review",
            NextAction::None => "-",
        }
    }
}

/// One row in the inbox.
#[derive(Debug, Clone, Serialize)]
pub struct InboxItem {
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    pub stack_position: usize,
    pub bucket: Bucket,
    pub next_action: NextAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ci: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approvals: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mergeable: Option<bool>,
    pub is_draft: bool,
    pub needs_restack: bool,
    pub agent_active: bool,
    pub is_current: bool,
    pub score: i64,
}

/// Map an internal CI status to the stable string used in JSON output.
fn ci_label(status: &CiStatus) -> Option<String> {
    match status {
        CiStatus::Success => Some("success".to_string()),
        CiStatus::Pending => Some("pending".to_string()),
        CiStatus::Failure => Some("failure".to_string()),
        CiStatus::NoCi => None,
    }
}

/// Derive the bucket + next action for a branch.
///
/// Pure function so it can be unit-tested without any git/network state.
/// Priority is deliberately ordered so the result is deterministic: blocking
/// states a human must clear win first, then active agents, then ready-to-merge,
/// then passive waiting.
pub fn classify(
    status: Option<&PrMergeStatus>,
    needs_restack: bool,
    agent_active: bool,
) -> (Bucket, NextAction) {
    if let Some(s) = status {
        if s.changes_requested {
            return (Bucket::NeedsYou, NextAction::AddressComments);
        }
        if s.ci_status.is_failure() {
            return (Bucket::NeedsYou, NextAction::FixCi);
        }
        if needs_restack {
            return (Bucket::NeedsYou, NextAction::Restack);
        }
        if agent_active {
            return (Bucket::AgentRunning, NextAction::ContinueAgent);
        }
        if s.is_ready() {
            return (Bucket::ReadyToMerge, NextAction::Merge);
        }
        if s.is_draft {
            return (Bucket::Other, NextAction::None);
        }
        return (Bucket::WaitingOnOthers, NextAction::WaitForReview);
    }

    // No PR / no live merge status available.
    if needs_restack {
        return (Bucket::NeedsYou, NextAction::Restack);
    }
    if agent_active {
        return (Bucket::AgentRunning, NextAction::ContinueAgent);
    }
    (Bucket::Other, NextAction::None)
}

/// Compute an intra-bucket priority score. Higher = touch sooner.
///
/// The buckets are the real product; this score is just a sort key within and
/// across them. Weights are intentionally simple and will become configurable
/// in a later phase.
pub fn score(action: NextAction, agent_active: bool) -> i64 {
    let mut s: i64 = match action {
        NextAction::AddressComments => 90,
        NextAction::FixCi => 80,
        NextAction::Merge => 60,
        NextAction::Restack => 50,
        NextAction::WaitForReview => 20,
        NextAction::ContinueAgent => 10,
        NextAction::None => 0,
    };
    // An agent already acting means it is less urgent for the human.
    if agent_active {
        s -= 40;
    }
    s
}

/// Fetch live merge status for the given (branch, pr_number) pairs concurrently.
///
/// Branches whose fetch fails are simply absent from the map; callers fall back
/// to cached CI state for those.
///
/// The caller must already be inside the runtime's context (`rt.enter()`) when
/// the [`ForgeClient`] is constructed — octocrab spins up a tower `Buffer`
/// worker that needs a live reactor, so building the client outside the runtime
/// and calling it here would panic with "there is no reactor running".
fn fetch_statuses(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    pr_branches: &[(String, u64)],
) -> HashMap<String, PrMergeStatus> {
    rt.block_on(async {
        let futures = pr_branches.iter().map(|(name, number)| {
            let client = client.clone();
            let name = name.clone();
            let number = *number;
            async move { (name, client.get_pr_merge_status(number).await.ok()) }
        });

        join_all(futures)
            .await
            .into_iter()
            .filter_map(|(name, status)| status.map(|s| (name, s)))
            .collect()
    })
}

/// Build the ranked inbox items for the current repo's tracked stack.
fn build_items(
    stack: &Stack,
    current: &str,
    statuses: &HashMap<String, PrMergeStatus>,
    cache: &CiCache,
    linked_worktrees: &HashMap<String, String>,
    remote_info: Option<&RemoteInfo>,
) -> Vec<InboxItem> {
    let mut candidates: Vec<String> = stack
        .branches
        .values()
        .filter(|b| b.name != stack.trunk)
        .map(|b| b.name.clone())
        .collect();
    candidates.sort();

    let mut items: Vec<InboxItem> = Vec::new();

    for name in &candidates {
        let Some(branch) = stack.branches.get(name) else {
            continue;
        };

        // Skip merged/closed PRs — they are done, not pending work.
        if let Some(state) = branch.pr_state.as_deref() {
            if state.eq_ignore_ascii_case("merged") || state.eq_ignore_ascii_case("closed") {
                continue;
            }
        }

        let status = statuses.get(name);
        let agent_active = linked_worktrees.contains_key(name);
        let needs_restack = branch.needs_restack;
        let (bucket, action) = classify(status, needs_restack, agent_active);

        // Drop pure noise: a healthy branch with no PR and nothing to do.
        if bucket == Bucket::Other
            && action == NextAction::None
            && branch.pr_number.is_none()
            && !agent_active
        {
            continue;
        }

        let ci = match status {
            Some(s) => ci_label(&s.ci_status),
            None => cache.get_ci_state(name),
        };

        let review = status.and_then(|s| {
            if s.changes_requested {
                Some("changes_requested".to_string())
            } else {
                s.review_decision.as_ref().map(|d| d.to_lowercase())
            }
        });

        let stack_position = stack
            .ancestors(name)
            .iter()
            .filter(|a| *a != &stack.trunk)
            .count()
            + 1;

        let pr_url = branch
            .pr_number
            .and_then(|n| remote_info.map(|r| r.pr_url(n)));

        items.push(InboxItem {
            branch: name.clone(),
            pr_number: branch.pr_number,
            pr_url,
            stack_position,
            bucket,
            next_action: action,
            ci,
            review,
            approvals: status.map(|s| s.approvals),
            mergeable: status.and_then(|s| s.mergeable),
            is_draft: status
                .map(|s| s.is_draft)
                .or(branch.pr_is_draft)
                .unwrap_or(false),
            needs_restack,
            agent_active,
            is_current: name == current,
            score: score(action, agent_active),
        });
    }

    // Highest score first; stable tiebreak by bucket order then branch name.
    items.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.bucket.order().cmp(&b.bucket.order()))
            .then(a.branch.cmp(&b.branch))
    });

    items
}

pub fn run(json: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let git_dir = repo.git_dir()?;

    let remote_info = RemoteInfo::from_repo(&repo, &config).ok();
    let cache = CiCache::load(git_dir);
    let linked_worktrees = repo.linked_worktree_names_by_branch().unwrap_or_default();

    // Branches with an open PR get a live merge-status fetch.
    let pr_branches: Vec<(String, u64)> = stack
        .branches
        .values()
        .filter(|b| b.name != stack.trunk)
        .filter_map(|b| {
            let number = b.pr_number?;
            let is_open = b
                .pr_state
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case("open"))
                .unwrap_or(true);
            is_open.then_some((b.name.clone(), number))
        })
        .collect();

    // Build the client and run fetches inside the Tokio runtime context.
    // octocrab's tower Buffer needs a live reactor at construction time, so the
    // client MUST be created after `rt.enter()` (mirrors `watch`/`ci`).
    let statuses = if pr_branches.is_empty() {
        HashMap::new()
    } else if let Some(remote) = remote_info.as_ref() {
        let rt = tokio::runtime::Runtime::new()?;
        let _enter = rt.enter();
        match ForgeClient::new(remote) {
            Ok(client) => fetch_statuses(&rt, &client, &pr_branches),
            Err(_) => HashMap::new(),
        }
    } else {
        HashMap::new()
    };

    let items = build_items(
        &stack,
        &current,
        &statuses,
        &cache,
        &linked_worktrees,
        remote_info.as_ref(),
    );

    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    render(&items);
    Ok(())
}

/// Minimal grouped text output. Richer rendering arrives in a later phase.
fn render(items: &[InboxItem]) {
    if items.is_empty() {
        println!("{}", "Inbox zero — nothing needs your attention.".dimmed());
        return;
    }

    let mut buckets: Vec<Bucket> = items.iter().map(|i| i.bucket).collect();
    buckets.sort_by_key(|b| b.order());
    buckets.dedup();

    for bucket in buckets {
        let group: Vec<&InboxItem> = items.iter().filter(|i| i.bucket == bucket).collect();
        if group.is_empty() {
            continue;
        }

        let header = match bucket {
            Bucket::NeedsYou => bucket.title().red().bold(),
            Bucket::AgentRunning => bucket.title().magenta().bold(),
            Bucket::ReadyToMerge => bucket.title().green().bold(),
            Bucket::WaitingOnOthers => bucket.title().yellow().bold(),
            Bucket::Other => bucket.title().dimmed(),
        };
        println!("{}", header);

        for item in group {
            let pr = item
                .pr_number
                .map(|n| format!("#{}", n))
                .unwrap_or_else(|| "  -".to_string());
            let name = if item.is_current {
                item.branch.bold().to_string()
            } else {
                item.branch.normal().to_string()
            };
            let action = item.next_action.label().cyan();
            let mut tags: Vec<String> = Vec::new();
            if let Some(ci) = &item.ci {
                tags.push(format!("CI:{}", ci));
            }
            if let Some(review) = &item.review {
                tags.push(review.clone());
            }
            if item.needs_restack {
                tags.push("restack".to_string());
            }
            if item.agent_active {
                tags.push("agent".to_string());
            }
            let tag_str = if tags.is_empty() {
                String::new()
            } else {
                format!("  ({})", tags.join(", ")).dimmed().to_string()
            };
            println!("  {:>6}  {:<32}  {}{}", pr.bright_magenta(), name, action, tag_str);
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::pr::CiStatus;

    fn status(
        ci: CiStatus,
        changes_requested: bool,
        is_draft: bool,
        mergeable: Option<bool>,
        review_decision: Option<&str>,
        state: &str,
    ) -> PrMergeStatus {
        PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: state.to_string(),
            is_draft,
            mergeable,
            mergeable_state: "clean".to_string(),
            ci_status: ci,
            review_decision: review_decision.map(str::to_string),
            approvals: if review_decision == Some("APPROVED") {
                1
            } else {
                0
            },
            changes_requested,
            head_sha: "abc".to_string(),
        }
    }

    #[test]
    fn classify_changes_requested_is_needs_you() {
        let s = status(CiStatus::Success, true, false, Some(true), Some("CHANGES_REQUESTED"), "Open");
        assert_eq!(
            classify(Some(&s), false, false),
            (Bucket::NeedsYou, NextAction::AddressComments)
        );
    }

    #[test]
    fn classify_ci_failure_is_needs_you() {
        let s = status(CiStatus::Failure, false, false, Some(true), None, "Open");
        assert_eq!(
            classify(Some(&s), false, false),
            (Bucket::NeedsYou, NextAction::FixCi)
        );
    }

    #[test]
    fn classify_changes_requested_beats_ci_failure() {
        // Both blocking; addressing comments takes priority.
        let s = status(CiStatus::Failure, true, false, Some(true), Some("CHANGES_REQUESTED"), "Open");
        assert_eq!(
            classify(Some(&s), false, false).1,
            NextAction::AddressComments
        );
    }

    #[test]
    fn classify_needs_restack_when_otherwise_healthy() {
        let s = status(CiStatus::Success, false, false, Some(true), None, "Open");
        assert_eq!(
            classify(Some(&s), true, false),
            (Bucket::NeedsYou, NextAction::Restack)
        );
    }

    #[test]
    fn classify_agent_running_when_healthy_and_agent_active() {
        let s = status(CiStatus::Success, false, false, Some(true), None, "Open");
        assert_eq!(
            classify(Some(&s), false, true),
            (Bucket::AgentRunning, NextAction::ContinueAgent)
        );
    }

    #[test]
    fn classify_ready_to_merge() {
        let s = status(CiStatus::Success, false, false, Some(true), Some("APPROVED"), "Open");
        assert_eq!(
            classify(Some(&s), false, false),
            (Bucket::ReadyToMerge, NextAction::Merge)
        );
    }

    #[test]
    fn classify_draft_is_other() {
        let s = status(CiStatus::Success, false, true, Some(true), None, "Open");
        assert_eq!(
            classify(Some(&s), false, false),
            (Bucket::Other, NextAction::None)
        );
    }

    #[test]
    fn classify_open_healthy_waits_on_others() {
        // Not ready (mergeable still computing) and not draft → waiting.
        let s = status(CiStatus::Pending, false, false, None, None, "Open");
        assert_eq!(
            classify(Some(&s), false, false),
            (Bucket::WaitingOnOthers, NextAction::WaitForReview)
        );
    }

    #[test]
    fn classify_no_status_needs_restack() {
        assert_eq!(
            classify(None, true, false),
            (Bucket::NeedsYou, NextAction::Restack)
        );
    }

    #[test]
    fn classify_no_status_agent_active() {
        assert_eq!(
            classify(None, false, true),
            (Bucket::AgentRunning, NextAction::ContinueAgent)
        );
    }

    #[test]
    fn classify_no_status_nothing_to_do() {
        assert_eq!(classify(None, false, false), (Bucket::Other, NextAction::None));
    }

    #[test]
    fn score_orders_blocking_above_waiting() {
        assert!(score(NextAction::AddressComments, false) > score(NextAction::WaitForReview, false));
        assert!(score(NextAction::FixCi, false) > score(NextAction::Merge, false));
        assert!(score(NextAction::Merge, false) > score(NextAction::Restack, false));
    }

    #[test]
    fn score_agent_active_deprioritizes() {
        assert!(score(NextAction::FixCi, true) < score(NextAction::FixCi, false));
    }

    #[test]
    fn build_items_skips_merged_and_ranks_blocking_first() {
        let mut branches = std::collections::HashMap::new();
        branches.insert(
            "main".to_string(),
            crate::engine::stack::StackBranch {
                name: "main".to_string(),
                parent: None,
                parent_revision: None,
                children: vec!["feat-a".to_string(), "feat-b".to_string(), "feat-c".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );
        for (name, pr, state) in [
            ("feat-a", 1u64, "OPEN"),
            ("feat-b", 2, "OPEN"),
            ("feat-c", 3, "MERGED"),
        ] {
            branches.insert(
                name.to_string(),
                crate::engine::stack::StackBranch {
                    name: name.to_string(),
                    parent: Some("main".to_string()),
                    parent_revision: Some("sha".to_string()),
                    children: vec![],
                    needs_restack: false,
                    pr_number: Some(pr),
                    pr_state: Some(state.to_string()),
                    pr_is_draft: Some(false),
                },
            );
        }
        let stack = Stack {
            branches,
            trunk: "main".to_string(),
        };

        let mut statuses = HashMap::new();
        // feat-a: CI failed → NeedsYou
        statuses.insert(
            "feat-a".to_string(),
            status(CiStatus::Failure, false, false, Some(true), None, "Open"),
        );
        // feat-b: ready to merge
        statuses.insert(
            "feat-b".to_string(),
            status(CiStatus::Success, false, false, Some(true), Some("APPROVED"), "Open"),
        );

        let cache = CiCache::default();
        let linked = HashMap::new();
        let items = build_items(&stack, "feat-a", &statuses, &cache, &linked, None);

        // Merged PR is excluded.
        assert!(items.iter().all(|i| i.branch != "feat-c"));
        assert_eq!(items.len(), 2);
        // Blocking (CI failed) ranks above ready-to-merge.
        assert_eq!(items[0].branch, "feat-a");
        assert_eq!(items[0].bucket, Bucket::NeedsYou);
        assert_eq!(items[0].next_action, NextAction::FixCi);
        assert!(items[0].is_current);
        assert_eq!(items[1].branch, "feat-b");
        assert_eq!(items[1].bucket, Bucket::ReadyToMerge);
    }
}
