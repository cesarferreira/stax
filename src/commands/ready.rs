use crate::cache::CiCache;
use crate::ci::CheckRunInfo;
use crate::config::Config;
use crate::engine::{BranchMetadata, PrInfo, Stack};
use crate::forge::{ForgeClient, forge_token};
use crate::git::GitRepo;
use crate::github::pr::{CiStatus, PrMergeStatus};
use crate::remote::RemoteInfo;
use anyhow::{Context, Result};
use chrono::Local;
use console::{measure_text_width, truncate_str};
use futures_util::{StreamExt, stream};
use serde::Serialize;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;

pub(crate) const READY_FETCH_CONCURRENCY: usize = 6;

const ACTION_WIDTH: usize = 7;
const PR_WIDTH: usize = 7;
const REVIEWS_MIN_WIDTH: usize = 7;
const CI_MIN_WIDTH: usize = 6;
const BRANCH_MIN_WIDTH: usize = 18;
const BRANCH_MAX_WIDTH: usize = 52;
const TITLE_MIN_WIDTH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadyAction {
    Fix,
    Merge,
    Ping,
    Wait,
    Draft,
}

impl ReadyAction {
    fn label(self) -> &'static str {
        match self {
            ReadyAction::Fix => "fix",
            ReadyAction::Merge => "merge",
            ReadyAction::Ping => "ping",
            ReadyAction::Wait => "wait",
            ReadyAction::Draft => "draft",
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            ReadyAction::Fix => "✕",
            ReadyAction::Merge => "✓",
            ReadyAction::Ping => "●",
            ReadyAction::Wait => "○",
            ReadyAction::Draft => "◌",
        }
    }

    pub fn display(self) -> String {
        format!("{} {}", self.symbol(), self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadyReason {
    Ready,
    ReviewRequired,
    CiFailed,
    ChangesRequested,
    NotMergeable,
    CiPending,
    MergeablePending,
    Draft,
    Closed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CiSummary {
    status: CiStatus,
    text: String,
}

impl CiSummary {
    fn from_checks(status: CiStatus, checks: &[CheckRunInfo]) -> Self {
        match status {
            CiStatus::Failure => {
                let failed = checks
                    .iter()
                    .filter(|check| {
                        check.status == "completed"
                            && matches!(
                                check.conclusion.as_deref(),
                                Some("failure") | Some("timed_out") | Some("action_required")
                            )
                    })
                    .count()
                    .max(1);
                Self::failed(failed)
            }
            CiStatus::Pending => Self::running(),
            CiStatus::Success => Self::passed(),
            CiStatus::NoCi => Self::no_ci(),
        }
    }

    fn passed() -> Self {
        Self {
            status: CiStatus::Success,
            text: "passed".to_string(),
        }
    }

    fn failed(count: usize) -> Self {
        Self {
            status: CiStatus::Failure,
            text: format!("{} {}", count, if count == 1 { "failed" } else { "failed" }),
        }
    }

    fn running() -> Self {
        Self {
            status: CiStatus::Pending,
            text: "running".to_string(),
        }
    }

    fn no_ci() -> Self {
        Self {
            status: CiStatus::NoCi,
            text: "no CI".to_string(),
        }
    }

    #[cfg(test)]
    fn not_run() -> Self {
        Self {
            status: CiStatus::NoCi,
            text: "not run".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrReadinessRow {
    pub branch: String,
    pub pr_number: u64,
    pub title: String,
    pub updated_at: Option<String>,
    pub action: ReadyAction,
    pub reason: ReadyReason,
    pub review_decision: Option<String>,
    pub approvals: usize,
    pub changes_requested: bool,
    pub ci_status: String,
    pub ci_summary: String,
    pub is_draft: bool,
    pub mergeable: Option<bool>,
    pub mergeable_state: String,
    pub pr_url: Option<String>,
    #[serde(skip)]
    pub review_summary: String,
    #[serde(skip)]
    pub pr_state: String,
}

impl PrReadinessRow {
    pub fn from_status(branch: &str, status: PrMergeStatus, ci_summary: CiSummary) -> Self {
        let review_summary = review_summary(&status);
        let (action, reason) = classify_status(&status, &ci_summary);
        let ci_status = match ci_summary.status {
            CiStatus::Success => "success",
            CiStatus::Pending => "pending",
            CiStatus::Failure => "failure",
            CiStatus::NoCi => "no_ci",
        }
        .to_string();

        Self {
            branch: branch.to_string(),
            pr_number: status.number,
            title: status.title,
            updated_at: status.updated_at,
            action,
            reason,
            review_decision: status.review_decision,
            approvals: status.approvals,
            changes_requested: status.changes_requested,
            ci_status,
            ci_summary: ci_summary.text,
            is_draft: status.is_draft,
            mergeable: status.mergeable,
            mergeable_state: status.mergeable_state,
            pr_url: None,
            review_summary,
            pr_state: status.state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyBranch {
    pub name: String,
    pub pr_number: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ReadyScope {
    pub git_dir: PathBuf,
    pub repo_label: String,
    pub scope_label: String,
    pub branches: Vec<ReadyBranch>,
    pub remote: RemoteInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadyRowState {
    Loading {
        branch: ReadyBranch,
    },
    Loaded(PrReadinessRow),
    Unavailable {
        branch: ReadyBranch,
        message: String,
    },
}

impl ReadyRowState {
    pub fn branch(&self) -> &str {
        match self {
            ReadyRowState::Loading { branch } => &branch.name,
            ReadyRowState::Loaded(row) => &row.branch,
            ReadyRowState::Unavailable { branch, .. } => &branch.name,
        }
    }

    pub fn pr_number(&self) -> Option<u64> {
        match self {
            ReadyRowState::Loading { branch } => branch.pr_number,
            ReadyRowState::Loaded(row) => Some(row.pr_number),
            ReadyRowState::Unavailable { branch, .. } => branch.pr_number,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyScopeMode {
    AllTracked,
    CurrentStack,
}

impl ReadyScopeMode {
    pub fn from_flags(_all: bool, current: bool, stack: bool) -> Self {
        if current || stack {
            Self::CurrentStack
        } else {
            Self::AllTracked
        }
    }

    fn include_all(self) -> bool {
        matches!(self, Self::AllTracked)
    }

    fn label(self) -> &'static str {
        match self {
            Self::AllTracked => "all tracked PRs",
            Self::CurrentStack => "current stack",
        }
    }
}

pub fn run(scope_mode: ReadyScopeMode, json: bool, plain: bool) -> Result<()> {
    if json {
        return run_static(scope_mode, true);
    }

    if !plain && std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return crate::tui::ready::run(scope_mode);
    }

    run_static(scope_mode, false)
}

fn run_static(scope_mode: ReadyScopeMode, json: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let remote = RemoteInfo::from_repo(&repo, &config)?;

    if forge_token(remote.forge).is_none() {
        anyhow::bail!(
            "{} auth not configured; live PR readiness cannot be fetched.",
            remote.forge
        );
    }

    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let client = ForgeClient::new(&remote)?;
    let (mut rows, skipped) = rt.block_on(async {
        fetch_readiness_rows(&repo, &client, &remote, &stack, &current, scope_mode).await
    })?;
    let branch_order = branch_scope(&stack, &current, scope_mode);
    sort_ready_rows(
        &mut rows,
        &branch_order
            .iter()
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>(),
    );

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    let repo_label = format!("{}/{}", remote.namespace, remote.repo);
    let summary = readiness_summary(scope_mode.label(), rows.len(), skipped);
    let width = crate::commands::github_list::terminal_width().max(80);
    println!(
        "{}",
        render_ready_table(&repo_label, &summary, &rows, width)
    );
    Ok(())
}

pub(crate) fn load_ready_scope(scope_mode: ReadyScopeMode) -> Result<ReadyScope> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let remote = RemoteInfo::from_repo(&repo, &config)?;

    if forge_token(remote.forge).is_none() {
        anyhow::bail!(
            "{} auth not configured; live PR readiness cannot be fetched.",
            remote.forge
        );
    }

    let branches = branch_scope(&stack, &current, scope_mode);
    Ok(ReadyScope {
        git_dir: repo.git_dir()?.to_path_buf(),
        repo_label: format!("{}/{}", remote.namespace, remote.repo),
        scope_label: scope_mode.label().to_string(),
        branches,
        remote,
    })
}

pub(crate) async fn fetch_row_for_branch(
    repo: &GitRepo,
    client: &ForgeClient,
    remote: &RemoteInfo,
    stack: &Stack,
    branch: &ReadyBranch,
) -> Result<Option<PrReadinessRow>> {
    let Some(pr_number) = resolve_branch_pr(client, stack, branch).await? else {
        return Ok(None);
    };

    let status = client
        .get_pr_merge_status(pr_number)
        .await
        .with_context(|| format!("Failed to fetch live readiness for PR #{}", pr_number))?;

    let ci_summary = CiSummary::from_checks(status.ci_status.clone(), &[]);
    let mut row = PrReadinessRow::from_status(&branch.name, status, ci_summary);
    row.pr_url = Some(remote.pr_url(pr_number));
    let _ = warm_caches_for_ready_row(repo, &row);
    Ok(Some(row))
}

pub(crate) fn warm_caches_for_ready_row(repo: &GitRepo, row: &PrReadinessRow) -> Result<()> {
    let git_dir = repo.git_dir()?;
    let mut cache = CiCache::load(git_dir);
    cache.update(
        &row.branch,
        Some(row.ci_status.clone()),
        Some(ready_row_pr_cache_state(row)),
    );
    cache.mark_refreshed();
    cache.save(git_dir)?;

    if let Some(mut meta) = BranchMetadata::read(repo.inner(), &row.branch)? {
        meta.pr_info = Some(PrInfo {
            number: row.pr_number,
            state: row.pr_state.clone(),
            is_draft: Some(row.is_draft),
        });
        meta.write(repo.inner(), &row.branch)?;
    }

    Ok(())
}

fn ready_row_pr_cache_state(row: &PrReadinessRow) -> String {
    if row.is_draft {
        "draft".to_string()
    } else {
        row.pr_state.clone()
    }
}

async fn fetch_readiness_rows(
    repo: &GitRepo,
    client: &ForgeClient,
    remote: &RemoteInfo,
    stack: &Stack,
    current: &str,
    scope_mode: ReadyScopeMode,
) -> Result<(Vec<PrReadinessRow>, usize)> {
    let branches = branch_scope(stack, current, scope_mode);
    let mut rows = Vec::new();
    let mut skipped = 0usize;

    let mut pending = stream::iter(
        branches
            .iter()
            .map(|branch| fetch_row_for_branch(repo, client, remote, stack, branch)),
    )
    .buffer_unordered(READY_FETCH_CONCURRENCY);

    while let Some(result) = pending.next().await {
        match result? {
            Some(row) => rows.push(row),
            None => skipped += 1,
        }
    }

    Ok((rows, skipped))
}

fn branch_scope(stack: &Stack, current: &str, scope_mode: ReadyScopeMode) -> Vec<ReadyBranch> {
    if scope_mode.include_all() {
        let mut branches = stack
            .branches
            .keys()
            .filter(|branch| *branch != &stack.trunk)
            .cloned()
            .collect::<Vec<_>>();
        branches.sort();
        branches
            .into_iter()
            .map(|name| ReadyBranch {
                pr_number: stack.branches.get(&name).and_then(|info| info.pr_number),
                name,
            })
            .collect()
    } else {
        stack
            .current_stack(current)
            .into_iter()
            .filter(|branch| branch != &stack.trunk)
            .map(|name| ReadyBranch {
                pr_number: stack.branches.get(&name).and_then(|info| info.pr_number),
                name,
            })
            .collect()
    }
}

async fn resolve_branch_pr(
    client: &ForgeClient,
    stack: &Stack,
    branch: &ReadyBranch,
) -> Result<Option<u64>> {
    if let Some(number) = branch.pr_number {
        return Ok(Some(number));
    }

    if let Some(number) = stack
        .branches
        .get(&branch.name)
        .and_then(|info| info.pr_number)
    {
        return Ok(Some(number));
    }

    Ok(client.find_pr(&branch.name).await?.map(|info| info.number))
}

fn classify_status(status: &PrMergeStatus, ci_summary: &CiSummary) -> (ReadyAction, ReadyReason) {
    if status.is_draft {
        return (ReadyAction::Draft, ReadyReason::Draft);
    }
    if !status.state.eq_ignore_ascii_case("open") {
        return (ReadyAction::Fix, ReadyReason::Closed);
    }
    if status.changes_requested || status.review_decision.as_deref() == Some("CHANGES_REQUESTED") {
        return (ReadyAction::Fix, ReadyReason::ChangesRequested);
    }
    if status.ci_status.is_failure() || ci_summary.status.is_failure() {
        return (ReadyAction::Fix, ReadyReason::CiFailed);
    }
    if status.mergeable == Some(false) {
        return (ReadyAction::Fix, ReadyReason::NotMergeable);
    }
    if status.ci_status.is_pending() || ci_summary.status.is_pending() {
        return (ReadyAction::Wait, ReadyReason::CiPending);
    }
    if status.mergeable.is_none() {
        return (ReadyAction::Wait, ReadyReason::MergeablePending);
    }
    if status.mergeable == Some(true)
        && status.ci_status.is_success()
        && matches!(status.review_decision.as_deref(), Some("APPROVED") | None)
    {
        return (ReadyAction::Merge, ReadyReason::Ready);
    }
    if status.review_decision.as_deref() == Some("REVIEW_REQUIRED") {
        return (ReadyAction::Ping, ReadyReason::ReviewRequired);
    }

    if status.mergeable == Some(true) && status.ci_status.is_success() {
        return (ReadyAction::Merge, ReadyReason::Ready);
    }

    (ReadyAction::Wait, ReadyReason::Unknown)
}

fn review_summary(status: &PrMergeStatus) -> String {
    if status.is_draft {
        return "draft".to_string();
    }
    if status.changes_requested || status.review_decision.as_deref() == Some("CHANGES_REQUESTED") {
        return "changes requested".to_string();
    }
    if status.review_decision.as_deref() == Some("REVIEW_REQUIRED") {
        return "missing review".to_string();
    }
    if status.approvals == 1 {
        return "1 approval".to_string();
    }
    if status.approvals > 1 {
        return format!("{} approvals", status.approvals);
    }
    if status.review_decision.is_none() {
        return "not required".to_string();
    }
    "unknown".to_string()
}

fn sort_ready_rows(rows: &mut [PrReadinessRow], branch_order: &[&str]) {
    let order = branch_order
        .iter()
        .enumerate()
        .map(|(idx, branch)| (*branch, idx))
        .collect::<HashMap<_, _>>();

    rows.sort_by(|a, b| {
        b.updated_at.cmp(&a.updated_at).then_with(|| {
            (
                order.get(a.branch.as_str()).copied().unwrap_or(usize::MAX),
                a.branch.as_str(),
            )
                .cmp(&(
                    order.get(b.branch.as_str()).copied().unwrap_or(usize::MAX),
                    b.branch.as_str(),
                ))
        })
    });
}

fn readiness_summary(scope: &str, row_count: usize, skipped: usize) -> String {
    let now = Local::now().format("%H:%M:%S");
    let pr_word = if row_count == 1 { "PR" } else { "PRs" };
    let skipped_suffix = if skipped > 0 {
        format!(" · {} skipped", skipped)
    } else {
        String::new()
    };
    format!("{scope} · fresh {now} · {row_count} {pr_word}{skipped_suffix}")
}

fn render_ready_table(
    repo_label: &str,
    summary: &str,
    rows: &[PrReadinessRow],
    width: usize,
) -> String {
    let mut output = String::new();
    output.push_str(&format!("{repo_label}  {summary}\n"));

    if rows.is_empty() {
        output.push_str("No pull requests in scope.\n");
        return output;
    }

    let review_width = rows
        .iter()
        .map(|row| measure_text_width(&row.review_summary))
        .max()
        .unwrap_or(REVIEWS_MIN_WIDTH)
        .max("REVIEWS".len())
        .max(REVIEWS_MIN_WIDTH);
    let ci_width = rows
        .iter()
        .map(|row| measure_text_width(&row.ci_summary))
        .max()
        .unwrap_or(CI_MIN_WIDTH)
        .max("CI".len())
        .max(CI_MIN_WIDTH);
    let branch_pref = rows
        .iter()
        .map(|row| measure_text_width(&row.branch))
        .max()
        .unwrap_or(BRANCH_MIN_WIDTH)
        .clamp(BRANCH_MIN_WIDTH, BRANCH_MAX_WIDTH);

    let fixed = ACTION_WIDTH + PR_WIDTH + review_width + ci_width + 10;
    let flexible = width.saturating_sub(fixed);
    let title_pref = rows
        .iter()
        .map(|row| measure_text_width(&row.title))
        .max()
        .unwrap_or(TITLE_MIN_WIDTH)
        .max(TITLE_MIN_WIDTH);
    let title_width = title_pref
        .min(
            flexible
                .saturating_sub(BRANCH_MIN_WIDTH)
                .max(TITLE_MIN_WIDTH),
        )
        .max(TITLE_MIN_WIDTH);
    let branch_width = flexible
        .saturating_sub(title_width)
        .clamp(BRANCH_MIN_WIDTH, branch_pref);

    output.push('\n');
    output.push_str(&format!(
        "{:<ACTION_WIDTH$}  {:<PR_WIDTH$}  {:<branch_width$}  {:<review_width$}  {:<ci_width$}  {}\n",
        "ACTION",
        "PR",
        "BRANCH",
        "REVIEWS",
        "CI",
        "TITLE"
    ));
    let divider_width =
        ACTION_WIDTH + PR_WIDTH + branch_width + review_width + ci_width + title_width + 10;
    output.push_str(&format!("{}\n", "─".repeat(divider_width.min(width))));

    for row in rows {
        output.push_str(&format!(
            "{:<ACTION_WIDTH$}  {:<PR_WIDTH$}  {:<branch_width$}  {:<review_width$}  {:<ci_width$}  {}\n",
            row.action.display(),
            format!("#{}", row.pr_number),
            fit_middle(&row.branch, branch_width),
            fit_end(&row.review_summary, review_width),
            fit_end(&row.ci_summary, ci_width),
            fit_end(&row.title, title_width),
        ));
    }

    output
}

fn fit_end(text: &str, width: usize) -> String {
    truncate_str(text, width, "...").into_owned()
}

fn fit_middle(text: &str, width: usize) -> String {
    if measure_text_width(text) <= width {
        return text.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let chars = text.chars().collect::<Vec<_>>();
    let keep = width.saturating_sub(3);
    let front = keep / 2 + keep % 2;
    let back = keep / 2;
    let prefix = chars.iter().take(front).collect::<String>();
    let suffix = chars
        .iter()
        .rev()
        .take(back)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CiCache;
    use crate::engine::BranchMetadata;
    use crate::git::GitRepo;
    use crate::github::pr::{CiStatus, PrMergeStatus};
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn status(overrides: impl FnOnce(&mut PrMergeStatus)) -> PrMergeStatus {
        let mut status = PrMergeStatus {
            number: 42,
            title: "Ready PR".to_string(),
            state: "open".to_string(),
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
        overrides(&mut status);
        status
    }

    #[test]
    fn classifies_ready_pr_as_merge() {
        let row = PrReadinessRow::from_status("feature", status(|_| {}), CiSummary::passed());

        assert_eq!(row.action, ReadyAction::Merge);
        assert_eq!(row.reason, ReadyReason::Ready);
        assert_eq!(row.review_summary, "1 approval");
        assert_eq!(row.ci_summary, "passed");
    }

    #[test]
    fn classifies_review_required_pr_as_ping() {
        let row = PrReadinessRow::from_status(
            "feature",
            status(|s| {
                s.review_decision = Some("REVIEW_REQUIRED".to_string());
                s.approvals = 0;
            }),
            CiSummary::passed(),
        );

        assert_eq!(row.action, ReadyAction::Ping);
        assert_eq!(row.reason, ReadyReason::ReviewRequired);
        assert_eq!(row.review_summary, "missing review");
    }

    #[test]
    fn classifies_no_review_requirement_as_ready() {
        let row = PrReadinessRow::from_status(
            "feature",
            status(|s| {
                s.review_decision = None;
                s.approvals = 0;
            }),
            CiSummary::passed(),
        );

        assert_eq!(row.action, ReadyAction::Merge);
        assert_eq!(row.reason, ReadyReason::Ready);
        assert_eq!(row.review_summary, "not required");
    }

    #[test]
    fn classifies_failed_ci_as_fix() {
        let row = PrReadinessRow::from_status(
            "feature",
            status(|s| s.ci_status = CiStatus::Failure),
            CiSummary::failed(2),
        );

        assert_eq!(row.action, ReadyAction::Fix);
        assert_eq!(row.reason, ReadyReason::CiFailed);
        assert_eq!(row.ci_summary, "2 failed");
    }

    #[test]
    fn classifies_changes_requested_as_fix() {
        let row = PrReadinessRow::from_status(
            "feature",
            status(|s| {
                s.review_decision = Some("CHANGES_REQUESTED".to_string());
                s.changes_requested = true;
            }),
            CiSummary::passed(),
        );

        assert_eq!(row.action, ReadyAction::Fix);
        assert_eq!(row.reason, ReadyReason::ChangesRequested);
        assert_eq!(row.review_summary, "changes requested");
    }

    #[test]
    fn classifies_pending_ci_as_wait() {
        let row = PrReadinessRow::from_status(
            "feature",
            status(|s| s.ci_status = CiStatus::Pending),
            CiSummary::running(),
        );

        assert_eq!(row.action, ReadyAction::Wait);
        assert_eq!(row.reason, ReadyReason::CiPending);
        assert_eq!(row.ci_summary, "running");
    }

    #[test]
    fn classifies_draft_before_failed_ci() {
        let row = PrReadinessRow::from_status(
            "feature",
            status(|s| {
                s.is_draft = true;
                s.ci_status = CiStatus::Failure;
            }),
            CiSummary::failed(1),
        );

        assert_eq!(row.action, ReadyAction::Draft);
        assert_eq!(row.reason, ReadyReason::Draft);
        assert_eq!(row.review_summary, "draft");
    }

    #[test]
    fn sorts_by_pr_updated_at_newest_first() {
        let mut rows = vec![
            PrReadinessRow::from_status(
                "old-fix",
                status(|s| {
                    s.ci_status = CiStatus::Failure;
                    s.updated_at = Some("2026-06-01T10:00:00Z".to_string());
                }),
                CiSummary::failed(1),
            ),
            PrReadinessRow::from_status(
                "new-ping",
                status(|s| {
                    s.review_decision = Some("REVIEW_REQUIRED".to_string());
                    s.approvals = 0;
                    s.updated_at = Some("2026-06-02T10:00:00Z".to_string());
                }),
                CiSummary::passed(),
            ),
            PrReadinessRow::from_status(
                "middle-merge",
                status(|s| {
                    s.updated_at = Some("2026-06-01T18:00:00Z".to_string());
                }),
                CiSummary::passed(),
            ),
            PrReadinessRow::from_status(
                "unknown-draft",
                status(|s| {
                    s.is_draft = true;
                    s.updated_at = None;
                }),
                CiSummary::not_run(),
            ),
        ];

        sort_ready_rows(
            &mut rows,
            &["old-fix", "new-ping", "middle-merge", "unknown-draft"],
        );

        let branches = rows
            .iter()
            .map(|row| row.branch.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            branches,
            vec!["new-ping", "middle-merge", "old-fix", "unknown-draft"]
        );
    }

    #[test]
    fn renders_table_with_titles_and_action_labels() {
        let rows = vec![PrReadinessRow::from_status(
            "feature",
            status(|_| {}),
            CiSummary::passed(),
        )];

        let rendered = render_ready_table("owner/repo", "current stack", &rows, 100);

        assert!(rendered.contains("ACTION"));
        assert!(rendered.contains("REVIEWS"));
        assert!(rendered.contains("CI"));
        assert!(rendered.contains("TITLE"));
        assert!(rendered.contains("merge"));
    }

    #[test]
    fn readiness_scope_defaults_to_all_tracked_prs() {
        assert_eq!(
            ReadyScopeMode::from_flags(false, false, false),
            ReadyScopeMode::AllTracked
        );
    }

    #[test]
    fn readiness_scope_current_or_stack_selects_current_stack() {
        assert_eq!(
            ReadyScopeMode::from_flags(false, true, false),
            ReadyScopeMode::CurrentStack
        );
        assert_eq!(
            ReadyScopeMode::from_flags(false, false, true),
            ReadyScopeMode::CurrentStack
        );
    }

    #[test]
    fn readiness_fetches_multiple_rows_per_batch() {
        assert!(READY_FETCH_CONCURRENCY > 1);
    }

    fn run_git(path: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("git command");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn temp_repo() -> (TempDir, GitRepo) {
        let temp = TempDir::new().expect("temp repo");
        let path = temp.path();
        run_git(path, &["init", "-b", "main"]);
        run_git(path, &["config", "user.email", "test@example.com"]);
        run_git(path, &["config", "user.name", "Test User"]);
        fs::write(path.join("README.md"), "base\n").expect("write readme");
        run_git(path, &["add", "README.md"]);
        run_git(path, &["commit", "-m", "Initial commit"]);
        run_git(path, &["checkout", "-b", "feature/cache-ready"]);

        let repo = GitRepo::open_from_path(path).expect("open repo");
        (temp, repo)
    }

    #[test]
    fn ready_row_warms_ci_cache_and_branch_pr_metadata() {
        let (_temp, repo) = temp_repo();
        let branch = "feature/cache-ready";
        let meta = BranchMetadata {
            pr_info: None,
            ..BranchMetadata::new("main", "abc123")
        };
        meta.write(repo.inner(), branch).expect("write metadata");

        let row = PrReadinessRow::from_status(
            branch,
            status(|s| {
                s.number = 123;
                s.state = "open".to_string();
                s.is_draft = true;
                s.ci_status = CiStatus::Success;
            }),
            CiSummary::passed(),
        );

        warm_caches_for_ready_row(&repo, &row).expect("warm cache");

        let cache = CiCache::load(repo.git_dir().expect("git dir"));
        let entry = cache.branches.get(branch).expect("cache entry");
        assert_eq!(entry.ci_state.as_deref(), Some("success"));
        assert_eq!(entry.pr_state.as_deref(), Some("draft"));

        let updated = BranchMetadata::read(repo.inner(), branch)
            .expect("read metadata")
            .expect("metadata");
        let pr = updated.pr_info.expect("pr info");
        assert_eq!(pr.number, 123);
        assert_eq!(pr.state, "open");
        assert_eq!(pr.is_draft, Some(true));
    }
}
