//! Shared helpers for the `stax merge` command family (`merge`, `merge --stack`,
//! `merge --queue`, `merge --remote`, `merge --when-ready`).
//!
//! Consolidates the display, PR-retarget, CI-history, and scope-selection
//! helpers that were previously duplicated across the sibling command modules.

use crate::commands::ci::{fetch_ci_statuses, record_ci_history};
use crate::commands::merge_rebase::{
    fetch_remote_for_descendant_rebase, rebase_descendant_onto_parent_with_provenance,
    rebase_descendant_onto_remote_trunk_with_provenance,
};
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::{GitRepo, RebaseResult};
use crate::github::pr::{PrMergeStatus, is_native_stack_base_locked_error};
use crate::progress::LiveTimer;
use anyhow::{Context, Result};
use colored::Colorize;
use std::io::Write;
use std::process::Command;
use std::time::{Duration, Instant};

const DUPLICATE_PR_BASE_RECHECK_TIMEOUT: Duration = Duration::from_secs(2);
const DUPLICATE_PR_BASE_RECHECK_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrBaseUpdate {
    Updated,
    AlreadyTargeted,
    /// GitHub rejected the retarget because the PR is registered in a native
    /// Stack (private preview) and a follow-up read still shows the old
    /// base. Callers should treat this as "skipped, not fatal" — GitHub may
    /// apply the retarget itself once the merged branch is deleted, or the
    /// stack may need to be re-linked with `st stack link`.
    NativeStackLocked,
}

/// Result of waiting for a PR to be ready.
pub(crate) enum WaitResult {
    Ready(PrMergeStatus),
    Failed(String),
    Timeout,
}

/// Controls how a blocked PR's reason string is rendered when waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockedReasonStyle {
    /// `merge`: map draft/blocked states to a descriptive, actionable message.
    Detailed,
    /// `merge --when-ready` / `merge --remote`: use the raw status text.
    StatusText,
}

/// A generic bottom-up merge scope shared across the merge command family.
pub(crate) struct Scope {
    /// Branches to merge (bottom to current, or full stack with `all`).
    pub(crate) to_merge: Vec<String>,
    /// Descendants above current not included in merge (unless `all`).
    pub(crate) remaining: Vec<String>,
    /// The trunk branch name.
    pub(crate) trunk: String,
    /// The branch that was checked out when merge started.
    pub(crate) current: String,
    /// Whether the current branch is excluded from the merge scope.
    pub(crate) downstack_only: bool,
}

/// Calculate which branches to merge and which descendants remain to be rebased.
pub(crate) fn calculate_scope(
    stack: &Stack,
    current: &str,
    all: bool,
    downstack_only: bool,
) -> Scope {
    let mut to_merge = stack.ancestors(current);
    to_merge.reverse();
    to_merge.retain(|b| b != &stack.trunk);

    let mut remaining = stack.descendants(current);
    if downstack_only {
        remaining.insert(0, current.to_string());
    } else {
        to_merge.push(current.to_string());
    }

    if all && !remaining.is_empty() {
        to_merge.extend(remaining);
        remaining = Vec::new();
    }

    Scope {
        to_merge,
        remaining,
        trunk: stack.trunk.clone(),
        current: current.to_string(),
        downstack_only,
    }
}

/// Map a blocked `PrMergeStatus` to a descriptive, actionable message.
pub(crate) fn blocked_reason(status: &PrMergeStatus) -> String {
    if status.is_draft {
        return "PR is in Draft state — remove Draft status before merging".to_string();
    }
    status.status_text().to_string()
}

/// Wait for a PR to be ready to merge (CI passed, approved).
pub(crate) fn wait_for_pr_ready(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    pr_number: u64,
    timeout: Duration,
    poll_interval: Duration,
    blocked_style: BlockedReasonStyle,
    quiet: bool,
) -> Result<WaitResult> {
    let start = Instant::now();
    let mut last_status: Option<String> = None;

    loop {
        let status = rt.block_on(async { client.get_pr_merge_status(pr_number).await })?;

        // Check if ready
        if status.is_ready() {
            if !quiet && last_status.is_some() {
                println!(); // End the waiting line
            }
            return Ok(WaitResult::Ready(status));
        }

        // Check if blocked (won't become ready)
        if status.is_blocked() {
            if !quiet && last_status.is_some() {
                println!(); // End the waiting line
            }
            let reason = match blocked_style {
                BlockedReasonStyle::Detailed => blocked_reason(&status),
                BlockedReasonStyle::StatusText => status.status_text().to_string(),
            };
            return Ok(WaitResult::Failed(reason));
        }

        // Check timeout
        if start.elapsed() > timeout {
            if !quiet && last_status.is_some() {
                println!(); // End the waiting line
            }
            return Ok(WaitResult::Timeout);
        }

        // Show waiting status
        if !quiet {
            let elapsed = start.elapsed().as_secs();
            let status_text = format!(
                "      {} Waiting for {}... ({}s)",
                "⏳".yellow(),
                status.status_text().to_lowercase(),
                elapsed
            );

            // Clear and rewrite the line
            if last_status.is_some() {
                print!("\r{}\r", " ".repeat(80));
            }
            print!("{}", status_text);
            std::io::stdout().flush().ok();
            last_status = Some(status_text);
        }

        // Wait before next poll
        std::thread::sleep(poll_interval);
    }
}

/// Best-effort wait until the forge reports `expected_sha` as the PR head.
/// Silently times out so the next merge attempt surfaces the real error.
fn wait_for_github_head_sync(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    pr_number: u64,
    expected_sha: &str,
    max_wait: Duration,
) {
    let start = Instant::now();
    let poll_interval = Duration::from_millis(500);
    loop {
        if let Ok(sha) = rt.block_on(async { client.get_pr_head_sha(pr_number).await })
            && sha == expected_sha
        {
            return;
        }
        // Stop before sleeping past the deadline.
        if start.elapsed() + poll_interval >= max_wait {
            return;
        }
        std::thread::sleep(poll_interval);
    }
}

/// Extract a concise, user-visible reason from `git push` stderr output.
/// Returns the first non-empty line, or a generic fallback when stderr is empty.
fn summarize_git_stderr(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .unwrap_or("push rejected")
        .to_string()
}

/// Retarget a PR base unless the forge already reports the desired base.
///
/// The preflight read validates that the PR number still belongs to the
/// expected stack branch when that read succeeds. GitHub can sometimes return a
/// duplicate base/head validation after the retarget has effectively applied, so
/// that specific error is treated as idempotent only after a follow-up read
/// confirms the intended base.
pub(crate) fn update_pr_base_unless_current(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    pr_number: u64,
    new_base: &str,
    expected_head: &str,
) -> Result<PrBaseUpdate> {
    // Best-effort read: if this fails, let PATCH run and surface the real forge error.
    if let Ok(current_pr) = rt.block_on(async { client.get_pr_with_head(pr_number).await }) {
        ensure_pr_head_matches(pr_number, &current_pr.head, expected_head)?;

        if current_pr.info.base == new_base {
            return Ok(PrBaseUpdate::AlreadyTargeted);
        }
    }

    let update_result = rt.block_on(async { client.update_pr_base(pr_number, new_base).await });
    match update_result {
        Ok(()) => Ok(PrBaseUpdate::Updated),
        Err(e) => {
            if is_duplicate_pr_base_error(&e, new_base, expected_head)
                && pr_base_matches_after_recheck(rt, client, pr_number, new_base)
            {
                return Ok(PrBaseUpdate::AlreadyTargeted);
            }

            if is_native_stack_base_locked_error(&e) {
                // GitHub may apply the retarget itself shortly after (e.g. once
                // the merged branch is deleted) — give it a moment before
                // treating this as merely skipped rather than done.
                if pr_base_matches_after_recheck(rt, client, pr_number, new_base) {
                    return Ok(PrBaseUpdate::AlreadyTargeted);
                }
                return Ok(PrBaseUpdate::NativeStackLocked);
            }

            Err(e).with_context(|| format!("failed to retarget PR #{} to {}", pr_number, new_base))
        }
    }
}

/// Print a soft note explaining a skipped retarget on a natively-stacked PR.
///
/// Used at cascade sites (retargeting a dependent PR to trunk after its
/// predecessor merged) where GitHub owning the base transition is fine to
/// leave for it — or for `st stack link` — to resolve, rather than aborting.
pub(crate) fn print_native_stack_locked_note(quiet: bool, pr_number: u64) {
    if quiet {
        return;
    }
    println!(
        "  {} {}",
        "note:".dimmed(),
        format!(
            "#{pr_number} manages its base via GitHub's native Stack; \
             retarget it with `st stack link` if it isn't updated automatically"
        )
        .dimmed()
    );
}

fn ensure_pr_head_matches(pr_number: u64, actual_head: &str, expected_head: &str) -> Result<()> {
    if actual_head != expected_head {
        anyhow::bail!(
            "PR #{} is for head branch '{}', expected '{}'",
            pr_number,
            actual_head,
            expected_head
        );
    }

    Ok(())
}

/// Re-read a PR briefly after a retarget PATCH was rejected, to check whether
/// the base ended up correct anyway.
///
/// Covers two races: (1) GitHub reports a duplicate base/head validation even
/// though subsequent PR reads show the same PR now targets the requested
/// base, and (2) a native-Stack base lock where GitHub applies the retarget
/// itself shortly after (e.g. once the merged branch is deleted).
fn pr_base_matches_after_recheck(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    pr_number: u64,
    new_base: &str,
) -> bool {
    let deadline = Instant::now() + DUPLICATE_PR_BASE_RECHECK_TIMEOUT;

    loop {
        if let Ok(pr) = rt.block_on(async { client.get_pr(pr_number).await })
            && pr.base == new_base
        {
            return true;
        }

        if Instant::now() >= deadline {
            return false;
        }

        std::thread::sleep(DUPLICATE_PR_BASE_RECHECK_INTERVAL);
    }
}

/// Detect GitHub's duplicate base/head validation for the exact target pair.
fn is_duplicate_pr_base_error(error: &anyhow::Error, new_base: &str, expected_head: &str) -> bool {
    let message = format!("{:#}", error);
    message.contains("A pull request already exists")
        && message.contains(&format!("base branch '{}'", new_base))
        && message.contains(&format!("head branch '{}'", expected_head))
}

/// Push a rebased remaining-stack branch and — only on success — retarget its
/// PR base. Surfaces push or retarget failures via the supplied `LiveTimer`;
/// on any failure the PR base is left pointing at the previous parent so the
/// on-forge state cannot diverge from the remote branch contents.
#[allow(clippy::too_many_arguments)]
fn finalize_remaining_branch_push(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    remote_name: &str,
    branch: &str,
    pr_number: Option<u64>,
    new_base: &str,
    timer: Option<LiveTimer>,
) -> Result<()> {
    let push_output = Command::new("git")
        .args(["push", "--force-with-lease", remote_name, branch])
        .current_dir(repo.workdir()?)
        .output();

    let push_err = match &push_output {
        Ok(out) if out.status.success() => None,
        Ok(out) => Some(summarize_git_stderr(&out.stderr)),
        Err(e) => Some(e.to_string()),
    };

    if let Some(reason) = push_err {
        LiveTimer::maybe_finish_err(
            timer,
            &format!("push failed; PR base unchanged ({})", reason),
        );
        return Ok(());
    }

    if let Some(pr_num) = pr_number
        && let Err(e) = update_pr_base_unless_current(rt, client, pr_num, new_base, branch)
    {
        LiveTimer::maybe_finish_err(timer, &format!("retarget failed: {:#}", e));
        return Ok(());
    }

    LiveTimer::maybe_finish_ok(timer, "done");
    Ok(())
}

/// Fetch, rebase, push, and retarget a single remaining-stack branch while
/// preserving the relative chain: when `previous_remaining` is `None` the
/// branch rebases onto `trunk`, otherwise it rebases onto the preceding
/// remaining branch so the stack topology stays intact.
///
/// Failures (fetch, rebase conflict, push rejection, retarget) are surfaced
/// via live-timer messages but do not abort the outer loop — other remaining
/// branches can still be processed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rebase_and_finalize_remaining_branch(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    remote_name: &str,
    trunk: &str,
    branch: &str,
    pr_number: Option<u64>,
    previous_remaining: Option<&str>,
    quiet: bool,
) -> Result<()> {
    let parent_is_trunk = previous_remaining.is_none();
    let parent_branch = previous_remaining
        .map(|s| s.to_string())
        .unwrap_or_else(|| trunk.to_string());

    let fetch_timer = LiveTimer::maybe_new(!quiet, "Fetching latest...");
    let fetch_ok = fetch_remote_for_descendant_rebase(repo, remote_name)?;
    if !fetch_ok {
        LiveTimer::maybe_finish_warn(fetch_timer, "warning");
    } else {
        LiveTimer::maybe_finish_ok(fetch_timer, "done");
    }

    let remaining_timer = LiveTimer::maybe_new(
        !quiet,
        &format!("Rebasing {} onto {}...", branch, parent_branch),
    );

    let rebase_result = if parent_is_trunk {
        rebase_descendant_onto_remote_trunk_with_provenance(repo, branch, trunk, remote_name)
    } else {
        rebase_descendant_onto_parent_with_provenance(
            repo,
            branch,
            &parent_branch,
            remote_name,
            false,
        )
    };

    match rebase_result {
        Ok(RebaseResult::Success) => {
            finalize_remaining_branch_push(
                repo,
                rt,
                client,
                remote_name,
                branch,
                pr_number,
                &parent_branch,
                remaining_timer,
            )?;
        }
        Ok(RebaseResult::Conflict) => {
            let abort_dir = repo
                .branch_worktree_path(branch)?
                .unwrap_or(repo.workdir()?.to_path_buf());
            let _ = Command::new("git")
                .args(["rebase", "--abort"])
                .current_dir(&abort_dir)
                .output();
            LiveTimer::maybe_finish_warn(remaining_timer, "conflict (skipped)");
        }
        Err(_) => {
            LiveTimer::maybe_finish_err(remaining_timer, "failed");
        }
    }

    Ok(())
}

/// After a force-push, briefly wait for the forge to acknowledge the new head
/// SHA so the next merge call doesn't race against an in-flight update and
/// return `405 Base branch was modified`.
pub(crate) fn sync_head_after_push(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    pr_number: u64,
    repo: &GitRepo,
    branch: &str,
) {
    if std::env::var_os("STAX_TEST_DISABLE_HEAD_SYNC").is_some() {
        return;
    }
    if let Ok(pushed_sha) = repo.rev_parse(branch) {
        wait_for_github_head_sync(rt, client, pr_number, &pushed_sha, Duration::from_secs(15));
    }
}

/// Record CI history for a single branch after it's merged
pub(crate) fn record_ci_history_for_branch(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    stack: &Stack,
    branch: &str,
) {
    // Verify the branch still exists before fetching CI status
    if repo.branch_commit(branch).is_err() {
        return; // Branch might already be deleted
    }

    // Fetch CI statuses for this single branch
    let branches = vec![branch.to_string()];
    if let Ok(statuses) = fetch_ci_statuses(repo, rt, client, stack, &branches) {
        // Record the CI history (silently - we don't want to interrupt the merge flow)
        record_ci_history(repo, &statuses);
    }
}

/// Strip ANSI codes for length calculation
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;

    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
            continue;
        }
        result.push(c);
    }

    result
}

/// Calculate the display width of a string, accounting for ANSI codes and wide Unicode chars
fn display_width(s: &str) -> usize {
    let stripped = strip_ansi(s);
    stripped.chars().map(char_width).sum()
}

/// Get the display width of a single character
fn char_width(c: char) -> usize {
    // Use unicode_width crate logic for accurate width calculation
    // For now, use a simplified approach that works for our specific use case
    match c {
        // Control characters and zero-width
        '\x00'..='\x1f' | '\x7f' => 0,
        // ASCII is width 1
        '\x20'..='\x7e' => 1,
        // Box drawing characters are width 1
        '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╯' | '╰'
        | '║' | '═' => 1,
        // Arrows - typically width 1 in most terminals
        '←' | '→' | '↑' | '↓' => 1,
        // Checkmarks and X marks - width 1 in most monospace fonts
        '✓' | '✗' | '✔' | '✘' => 1,
        // Everything else (including emojis) - assume width 2
        _ => 2,
    }
}

pub(crate) fn print_header(title: &str) {
    let width: usize = 56;
    let title_width = display_width(title);
    let padding = width.saturating_sub(title_width) / 2;
    println!("╭{}╮", "─".repeat(width));
    println!(
        "│{}{}{}│",
        " ".repeat(padding),
        title.bold(),
        " ".repeat(width.saturating_sub(padding + title_width))
    );
    println!("╰{}╯", "─".repeat(width));
}

pub(crate) fn print_header_success(title: &str) {
    let width: usize = 56;
    let full_title = format!("✓ {}", title);
    let title_width = display_width(&full_title);
    let padding = width.saturating_sub(title_width) / 2;
    println!("╭{}╮", "─".repeat(width));
    println!(
        "│{}{}{}│",
        " ".repeat(padding),
        full_title.green().bold(),
        " ".repeat(width.saturating_sub(padding + title_width))
    );
    println!("╰{}╯", "─".repeat(width));
}

pub(crate) fn print_header_error(title: &str) {
    let width: usize = 56;
    let full_title = format!("✗ {}", title);
    let title_width = display_width(&full_title);
    let padding = width.saturating_sub(title_width) / 2;
    println!("╭{}╮", "─".repeat(width));
    println!(
        "│{}{}{}│",
        " ".repeat(padding),
        full_title.red().bold(),
        " ".repeat(width.saturating_sub(padding + title_width))
    );
    println!("╰{}╯", "─".repeat(width));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::stack::StackBranch;
    use crate::github::pr::CiStatus;
    use std::collections::HashMap;

    fn create_test_stack() -> Stack {
        let mut branches = HashMap::new();

        branches.insert(
            "main".to_string(),
            StackBranch {
                name: "main".to_string(),
                parent: None,
                parent_revision: None,
                children: vec!["feature-a".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "feature-a".to_string(),
            StackBranch {
                name: "feature-a".to_string(),
                parent: Some("main".to_string()),
                parent_revision: None,
                children: vec!["feature-b".to_string()],
                needs_restack: false,
                pr_number: Some(1),
                pr_state: Some("OPEN".to_string()),
                pr_is_draft: Some(false),
            },
        );

        branches.insert(
            "feature-b".to_string(),
            StackBranch {
                name: "feature-b".to_string(),
                parent: Some("feature-a".to_string()),
                parent_revision: None,
                children: vec!["feature-c".to_string()],
                needs_restack: false,
                pr_number: Some(2),
                pr_state: Some("OPEN".to_string()),
                pr_is_draft: Some(false),
            },
        );

        branches.insert(
            "feature-c".to_string(),
            StackBranch {
                name: "feature-c".to_string(),
                parent: Some("feature-b".to_string()),
                parent_revision: None,
                children: vec![],
                needs_restack: false,
                pr_number: Some(3),
                pr_state: Some("OPEN".to_string()),
                pr_is_draft: Some(false),
            },
        );

        Stack {
            branches,
            trunk: "main".to_string(),
        }
    }

    fn merge_status(state: &str) -> PrMergeStatus {
        PrMergeStatus {
            number: 1,
            title: "test".to_string(),
            state: state.to_string(),
            updated_at: None,
            is_draft: false,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            ci_status: CiStatus::Success,
            review_decision: None,
            approvals: 0,
            changes_requested: false,
            head_sha: "abc123".to_string(),
        }
    }

    #[test]
    fn test_strip_ansi_empty_string() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn test_strip_ansi_no_codes() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn test_strip_ansi_with_color_codes() {
        // Red text: \x1b[31mred\x1b[0m
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn test_strip_ansi_with_multiple_codes() {
        // Bold + red: \x1b[1m\x1b[31mtext\x1b[0m
        assert_eq!(strip_ansi("\x1b[1m\x1b[31mtext\x1b[0m"), "text");
    }

    #[test]
    fn test_strip_ansi_complex() {
        let colored = "\x1b[32m✓\x1b[0m \x1b[1mBold\x1b[0m \x1b[33mYellow\x1b[0m";
        assert_eq!(strip_ansi(colored), "✓ Bold Yellow");
    }

    #[test]
    fn test_strip_ansi_preserves_unicode() {
        let with_emoji = "\x1b[32m✓\x1b[0m Success 🎉";
        assert_eq!(strip_ansi(with_emoji), "✓ Success 🎉");
    }

    #[test]
    fn test_display_width_ascii() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width("hello world"), 11);
    }

    #[test]
    fn test_display_width_symbols() {
        // Check marks and X marks are width 1
        assert_eq!(display_width("✓"), 1);
        assert_eq!(display_width("✗"), 1);
        // Other emojis are width 2
        assert_eq!(display_width("⏳"), 2);
    }

    #[test]
    fn test_display_width_mixed() {
        // "✓ passed" = 1 (checkmark) + 1 (space) + 6 (passed) = 8
        assert_eq!(display_width("✓ passed"), 8);
        // "~ pending" = 1 (~) + 1 (space) + 7 (pending) = 9 (using ASCII now)
        assert_eq!(display_width("~ pending"), 9);
    }

    #[test]
    fn test_display_width_with_ansi() {
        // ANSI codes should be ignored
        assert_eq!(display_width("\x1b[32m✓\x1b[0m passed"), 8);
    }

    #[test]
    fn test_duplicate_pr_base_error_matches_requested_base_and_head() {
        let err = anyhow::anyhow!(
            "GitHub: Validation Failed\nErrors:\n- {{\"message\":\"A pull request already exists for base branch 'master' and head branch 'jonny/feature'\"}}"
        );

        assert!(is_duplicate_pr_base_error(&err, "master", "jonny/feature"));
        assert!(!is_duplicate_pr_base_error(&err, "main", "jonny/feature"));
        assert!(!is_duplicate_pr_base_error(&err, "master", "other/feature"));
    }

    #[test]
    fn test_pr_head_mismatch_bails() {
        let err = ensure_pr_head_matches(42, "wrong-head", "expected-head")
            .expect_err("head mismatch should fail");
        let message = format!("{:#}", err);

        assert!(message.contains("PR #42 is for head branch 'wrong-head'"));
        assert!(message.contains("expected 'expected-head'"));
    }

    #[test]
    fn test_wait_result_variants() {
        // Test that all variants can be created
        let _ready = WaitResult::Ready(merge_status("open"));
        let _failed = WaitResult::Failed("CI failed".to_string());
        let _timeout = WaitResult::Timeout;
    }

    #[test]
    fn test_blocked_reason_draft_explains_fix() {
        let mut status = merge_status("open");
        status.is_draft = true;

        let reason = blocked_reason(&status);
        assert!(reason.contains("Draft"));
        assert!(reason.contains("remove Draft"));
    }

    #[test]
    fn test_blocked_reason_changes_requested() {
        let mut status = merge_status("open");
        status.changes_requested = true;

        assert_eq!(blocked_reason(&status), "Changes requested");
    }

    #[test]
    fn test_blocked_reason_ci_failed() {
        let mut status = merge_status("open");
        status.ci_status = CiStatus::Failure;

        assert_eq!(blocked_reason(&status), "CI failed");
    }

    #[test]
    fn test_calculate_scope_default_from_middle_keeps_descendants_remaining() {
        let stack = create_test_stack();

        let scope = calculate_scope(&stack, "feature-b", false, false);

        assert_eq!(scope.to_merge, vec!["feature-a", "feature-b"]);
        assert_eq!(scope.remaining, vec!["feature-c"]);
        assert_eq!(scope.trunk, "main");
        assert_eq!(scope.current, "feature-b");
        assert!(!scope.downstack_only);
    }

    #[test]
    fn test_calculate_scope_all_includes_descendants() {
        let stack = create_test_stack();

        let scope = calculate_scope(&stack, "feature-b", true, false);

        assert_eq!(scope.to_merge, vec!["feature-a", "feature-b", "feature-c"]);
        assert!(scope.remaining.is_empty());
    }

    #[test]
    fn test_calculate_scope_downstack_only_excludes_current() {
        let stack = create_test_stack();

        let scope = calculate_scope(&stack, "feature-b", false, true);

        assert_eq!(scope.to_merge, vec!["feature-a"]);
        assert_eq!(scope.remaining, vec!["feature-b", "feature-c"]);
        assert_eq!(scope.current, "feature-b");
        assert!(scope.downstack_only);
    }

    #[test]
    fn test_calculate_scope_downstack_only_direct_child_has_no_merge_targets() {
        let stack = create_test_stack();

        let scope = calculate_scope(&stack, "feature-a", false, true);

        assert!(scope.to_merge.is_empty());
        assert_eq!(scope.remaining, vec!["feature-a", "feature-b", "feature-c"]);
    }
}
