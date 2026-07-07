//! Fast-forward stack merge through one GitHub PR merge.
//!
//! This is the serverless path from issue #293: validate the selected tip PR
//! once, retarget that PR to trunk, merge it via GitHub's merge API, then
//! reconcile selected lower PRs as merged or absorbed.

use crate::commands::merge::{
    PrBaseUpdate, rebase_and_finalize_remaining_branch, update_pr_base_unless_current,
};
use crate::config::Config;
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::github::pr::{CiStatus, MergeMethod, PrMergeStatus};
use crate::progress::LiveTimer;
use crate::remote::{ForgeType, RemoteInfo};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, theme::ColorfulTheme};
use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
struct StackMergeBranch {
    branch: String,
    pr_number: Option<u64>,
}

#[derive(Debug, Clone)]
struct StackMergeScope {
    to_merge: Vec<StackMergeBranch>,
    remaining: Vec<StackMergeBranch>,
    trunk: String,
    current: String,
    downstack_only: bool,
}

#[derive(Debug, Clone)]
struct ResolvedStackPr {
    branch: String,
    pr_number: u64,
    base: String,
}

#[derive(Debug, Clone)]
struct RemainingStackBranch {
    branch: String,
    pr_number: Option<u64>,
}

enum WaitResult {
    Ready(PrMergeStatus),
    Failed(String),
    Timeout,
}

/// How the stack-merge confirmation should be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackMergeConfirmation {
    /// `--yes` was provided; proceed without prompting.
    Approved,
    /// No `--yes` and non-interactive (quiet or no TTY); refuse with an error.
    RequireYes,
    /// No `--yes` but interactive; ask the user to confirm.
    Prompt,
}

/// Decide how to confirm a stack merge.
///
/// `--quiet` suppresses output but must never imply consent for this
/// destructive remote operation. The only non-interactive approval is `--yes`.
fn stack_merge_confirmation(yes: bool, quiet: bool, is_terminal: bool) -> StackMergeConfirmation {
    if yes {
        StackMergeConfirmation::Approved
    } else if quiet || !is_terminal {
        StackMergeConfirmation::RequireYes
    } else {
        StackMergeConfirmation::Prompt
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    full: bool,
    downstack_only: bool,
    dry_run: bool,
    when_ready: bool,
    method: MergeMethod,
    timeout_mins: u64,
    interval_secs: u64,
    no_delete: bool,
    no_sync: bool,
    yes: bool,
    quiet: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;

    if current == stack.trunk {
        if !quiet {
            println!(
                "{}",
                "You are on trunk. Checkout a tracked stack branch to merge.".yellow()
            );
        }
        return Ok(());
    }

    if !stack.branches.contains_key(&current) {
        if !quiet {
            println!(
                "{}",
                format!(
                    "Branch '{}' is not tracked. Run 'stax branch track' first.",
                    current
                )
                .yellow()
            );
        }
        return Ok(());
    }

    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    if remote_info.forge != ForgeType::GitHub {
        anyhow::bail!(
            "`stax merge --stack` is only supported for GitHub remotes (found {})",
            remote_info.forge
        );
    }

    let scope = calculate_stack_merge_scope(&repo, &stack, &current, full, downstack_only)?;
    if scope.to_merge.is_empty() {
        if !quiet {
            println!("{}", "No branches to stack merge.".yellow());
        }
        return Ok(());
    }

    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let client = ForgeClient::new(&remote_info).context(
        "Failed to connect to the configured forge. Check your token and remote configuration.",
    )?;

    let fetch_timer = LiveTimer::maybe_new(!quiet, "Fetching latest trunk...");
    let trunk_sha = fetch_and_verify_trunk_current(&repo, &remote_info, &scope.trunk)?;
    LiveTimer::maybe_finish_ok(fetch_timer, "done");

    verify_linear_stack(&repo, &scope)?;

    let pr_timer = LiveTimer::maybe_new(!quiet, "Fetching stack PRs...");
    let resolved = resolve_stack_prs(&rt, &client, &scope)?;
    let remaining = resolve_remaining_branches(&rt, &client, &scope.remaining)?;
    LiveTimer::maybe_finish_ok(pr_timer, "done");

    let status_timer = LiveTimer::maybe_new(!quiet, "Checking stack eligibility...");
    let statuses = fetch_stack_statuses(&rt, &client, &resolved)?;
    check_downstack_eligibility(&resolved, &statuses)?;
    LiveTimer::maybe_finish_ok(status_timer, "done");

    let tip = resolved.last().context("stack merge scope is empty")?;
    let mut tip_status = statuses.last().context("missing tip status")?.clone();

    if !when_ready {
        if let Some(reason) = tip_blocker(&tip_status) {
            anyhow::bail!(
                "Selected tip PR #{} ({}) is not ready: {}.\n\nRun `stax merge --stack --when-ready` to wait.",
                tip.pr_number,
                tip.branch,
                reason
            );
        }
    }

    if !quiet {
        println!();
        print_stack_preview(
            &resolved,
            &remaining,
            &scope.trunk,
            method,
            when_ready,
            scope.downstack_only,
        );
    }

    if dry_run {
        if !quiet {
            println!();
            println!("{}", "Dry run - no changes made.".dimmed());
        }
        return Ok(());
    }

    // `--quiet` only suppresses output; it must not imply consent for this
    // destructive remote operation. The only non-interactive approval is
    // `--yes`. Without it, prompt when interactive, otherwise fail clearly.
    match stack_merge_confirmation(yes, quiet, std::io::stdin().is_terminal()) {
        StackMergeConfirmation::Approved => {}
        StackMergeConfirmation::RequireYes => {
            anyhow::bail!(
                "`stax merge --stack` needs confirmation in non-interactive mode. Re-run with `--yes`."
            );
        }
        StackMergeConfirmation::Prompt => {
            let confirm = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Proceed with fast-forward stack merge?")
                .default(false)
                .interact()?;

            if !confirm {
                println!("{}", "Aborted.".dimmed());
                return Ok(());
            }
        }
    }

    if !quiet {
        println!();
        print_header("Stack Fast-Forward Merge");
    }

    if when_ready {
        let timeout = Duration::from_secs(timeout_mins * 60);
        let poll_interval = Duration::from_secs(interval_secs);
        match wait_for_tip_ready(&rt, &client, tip.pr_number, timeout, poll_interval, quiet)? {
            WaitResult::Ready(status) => tip_status = status,
            WaitResult::Failed(reason) => {
                anyhow::bail!(
                    "Selected tip PR #{} ({}) is not ready: {}",
                    tip.pr_number,
                    tip.branch,
                    reason
                )
            }
            WaitResult::Timeout => {
                anyhow::bail!(
                    "Timed out waiting for selected tip PR #{} ({}) to become ready",
                    tip.pr_number,
                    tip.branch
                )
            }
        }
    }

    verify_tip_head_matches_local(&repo, tip, &tip_status)?;
    ensure_trunk_unchanged(&repo, &remote_info, &scope.trunk, &trunk_sha)?;

    let original_tip_base = tip.base.clone();
    let retarget_timer = LiveTimer::maybe_new(
        !quiet,
        &format!(
            "Retargeting selected tip PR #{} to {}...",
            tip.pr_number, scope.trunk
        ),
    );
    let retargeted =
        match update_pr_base_unless_current(&rt, &client, tip.pr_number, &scope.trunk, &tip.branch)
        {
            Ok(PrBaseUpdate::Updated) => {
                LiveTimer::maybe_finish_ok(retarget_timer, "done");
                true
            }
            Ok(PrBaseUpdate::AlreadyTargeted) => {
                LiveTimer::maybe_finish_ok(retarget_timer, "already on base");
                false
            }
            Ok(PrBaseUpdate::NativeStackLocked) => {
                LiveTimer::maybe_finish_err(retarget_timer, "locked");
                anyhow::bail!(
                    "PR #{} is registered in a native GitHub Stack, which locks its base branch. \
                     Merge it via the normal stack order (`st merge`) instead of merging out of \
                     order, or run `st stack unlink` first if you need to retarget it manually.",
                    tip.pr_number
                );
            }
            Err(e) => {
                LiveTimer::maybe_finish_err(retarget_timer, "failed");
                return Err(e);
            }
        };

    if let Err(e) = ensure_trunk_unchanged(&repo, &remote_info, &scope.trunk, &trunk_sha) {
        if retargeted {
            restore_tip_base(&rt, &client, tip, &original_tip_base, quiet);
        }
        return Err(e);
    }

    let merge_timer = LiveTimer::maybe_new(
        !quiet,
        &format!(
            "Merging selected tip PR #{} ({})...",
            tip.pr_number,
            method.as_str()
        ),
    );
    let merge_result = rt.block_on(async {
        client
            .merge_pr(tip.pr_number, method, None, Some(&tip_status.head_sha))
            .await
    });

    if let Err(e) = merge_result {
        LiveTimer::maybe_finish_err(merge_timer, "failed");
        if retargeted {
            restore_tip_base(&rt, &client, tip, &original_tip_base, quiet);
        }
        return Err(e);
    }
    LiveTimer::maybe_finish_ok(merge_timer, "done");

    let github_merged_downstack = absorb_downstack_prs(&rt, &client, &resolved, tip, quiet)?;

    rebase_remaining_branches(
        &repo,
        &rt,
        &client,
        &remote_info.name,
        &scope.trunk,
        &remaining,
        quiet,
    )?;

    if !no_delete {
        cleanup_local_stack(&repo, &scope, quiet)?;
    }

    if !no_sync {
        run_post_merge_sync(quiet);
    }

    if !quiet {
        println!();
        print_header_success("Stack Merged");
        println!();
        println!(
            "Merged {} PRs through tip #{} into {}:",
            resolved.len(),
            tip.pr_number,
            scope.trunk.cyan()
        );
        for pr in &resolved {
            let action = if pr.pr_number == tip.pr_number {
                "merged"
            } else if github_merged_downstack.contains(&pr.pr_number) {
                "merged by GitHub"
            } else {
                "absorbed"
            };
            println!(
                "  {} #{} {} -> {}",
                "✓".green(),
                pr.pr_number,
                pr.branch,
                action
            );
        }
        if !remaining.is_empty() {
            println!();
            println!("Remaining in stack (rebased onto {}):", scope.trunk.cyan());
            for branch in &remaining {
                if let Some(pr_number) = branch.pr_number {
                    println!("  {} #{} {}", "○".dimmed(), pr_number, branch.branch);
                } else {
                    println!("  {} {}", "○".dimmed(), branch.branch);
                }
            }
        }
    }

    Ok(())
}

fn calculate_stack_merge_scope(
    repo: &GitRepo,
    stack: &Stack,
    current: &str,
    full: bool,
    downstack_only: bool,
) -> Result<StackMergeScope> {
    let mut to_merge = stack.ancestors(current);
    to_merge.reverse();
    to_merge.retain(|branch| branch != &stack.trunk);

    let mut remaining = stack.descendants(current);
    if downstack_only {
        remaining.insert(0, current.to_string());
    } else {
        to_merge.push(current.to_string());
    }

    if full && !remaining.is_empty() {
        to_merge.extend(std::mem::take(&mut remaining));
    }

    let to_merge = to_merge
        .into_iter()
        .map(|branch| stack_merge_branch(repo, stack, branch))
        .collect::<Result<Vec<_>>>()?;
    let remaining = remaining
        .into_iter()
        .map(|branch| stack_merge_branch(repo, stack, branch))
        .collect::<Result<Vec<_>>>()?;

    Ok(StackMergeScope {
        to_merge,
        remaining,
        trunk: stack.trunk.clone(),
        current: current.to_string(),
        downstack_only,
    })
}

fn stack_merge_branch(repo: &GitRepo, stack: &Stack, branch: String) -> Result<StackMergeBranch> {
    let pr_number = stack.branches.get(&branch).and_then(|info| info.pr_number);

    repo.branch_commit(&branch)
        .with_context(|| format!("Local branch '{}' does not exist", branch))?;

    Ok(StackMergeBranch { branch, pr_number })
}

fn fetch_and_verify_trunk_current(
    repo: &GitRepo,
    remote: &RemoteInfo,
    trunk: &str,
) -> Result<String> {
    if !repo.fetch_remote(&remote.name)? {
        anyhow::bail!("Failed to fetch remote '{}'", remote.name);
    }

    let local = repo.rev_parse(trunk)?;
    let remote_ref = format!("{}/{}", remote.name, trunk);
    let remote_sha = repo
        .rev_parse(&remote_ref)
        .with_context(|| format!("Failed to resolve remote trunk '{}'", remote_ref))?;

    if local != remote_sha {
        anyhow::bail!(
            "Local trunk '{}' is not current with '{}'. Run `stax sync --restack` and wait for the new tip CI before stack merging.",
            trunk,
            remote_ref
        );
    }

    Ok(local)
}

fn ensure_trunk_unchanged(
    repo: &GitRepo,
    remote: &RemoteInfo,
    trunk: &str,
    expected_sha: &str,
) -> Result<()> {
    if !repo.fetch_remote(&remote.name)? {
        anyhow::bail!("Failed to fetch remote '{}'", remote.name);
    }

    let remote_ref = format!("{}/{}", remote.name, trunk);
    let remote_sha = repo.rev_parse(&remote_ref)?;
    if remote_sha != expected_sha {
        anyhow::bail!(
            "Remote trunk '{}' moved while preparing the stack merge. Run `stax sync --restack` and wait for the new tip CI.",
            remote_ref
        );
    }

    Ok(())
}

fn verify_linear_stack(repo: &GitRepo, scope: &StackMergeScope) -> Result<()> {
    let mut parent = scope.trunk.as_str();
    for branch in scope.to_merge.iter().chain(scope.remaining.iter()) {
        if !repo.is_ancestor(parent, &branch.branch)? {
            anyhow::bail!(
                "Branch '{}' is not based on '{}'. Run `stax restack` and wait for tip CI before stack merging.",
                branch.branch,
                parent
            );
        }
        parent = &branch.branch;
    }

    Ok(())
}

fn resolve_stack_prs(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    scope: &StackMergeScope,
) -> Result<Vec<ResolvedStackPr>> {
    let mut resolved = Vec::with_capacity(scope.to_merge.len());

    for branch in &scope.to_merge {
        let pr_number = match branch.pr_number {
            Some(pr_number) => pr_number,
            None => rt
                .block_on(async { client.find_pr(&branch.branch).await })?
                .map(|pr| pr.number)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Branch '{}' has no PR. Run 'stax submit' first.",
                        branch.branch
                    )
                })?,
        };

        let pr = rt.block_on(async { client.get_pr_with_head(pr_number).await })?;
        if pr.head != branch.branch {
            anyhow::bail!(
                "PR #{} is for head branch '{}', expected '{}'",
                pr_number,
                pr.head,
                branch.branch
            );
        }

        resolved.push(ResolvedStackPr {
            branch: branch.branch.clone(),
            pr_number,
            base: pr.info.base,
        });
    }

    Ok(resolved)
}

fn resolve_remaining_branches(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    branches: &[StackMergeBranch],
) -> Result<Vec<RemainingStackBranch>> {
    branches
        .iter()
        .map(|branch| {
            let pr_number = match branch.pr_number {
                Some(pr_number) => Some(pr_number),
                None => rt
                    .block_on(async { client.find_pr(&branch.branch).await })?
                    .map(|pr| pr.number),
            };

            Ok(RemainingStackBranch {
                branch: branch.branch.clone(),
                pr_number,
            })
        })
        .collect()
}

fn fetch_stack_statuses(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    prs: &[ResolvedStackPr],
) -> Result<Vec<PrMergeStatus>> {
    prs.iter()
        .map(|pr| rt.block_on(async { client.get_pr_merge_status(pr.pr_number).await }))
        .collect()
}

fn check_downstack_eligibility(prs: &[ResolvedStackPr], statuses: &[PrMergeStatus]) -> Result<()> {
    for (pr, status) in prs
        .iter()
        .zip(statuses.iter())
        .take(prs.len().saturating_sub(1))
    {
        if let Some(reason) = downstack_blocker(status) {
            anyhow::bail!(
                "Downstack PR #{} ({}) is not eligible for stack merge: {}",
                pr.pr_number,
                pr.branch,
                reason
            );
        }
    }

    Ok(())
}

fn downstack_blocker(status: &PrMergeStatus) -> Option<&'static str> {
    if status.state.to_lowercase() != "open" {
        return Some("PR is closed");
    }
    if status.is_draft {
        return Some("PR is draft");
    }
    if status.changes_requested {
        return Some("changes requested");
    }
    if status.review_decision.as_deref() == Some("REVIEW_REQUIRED") {
        return Some("review required");
    }
    if status.mergeable == Some(false) {
        return Some("has merge conflicts");
    }
    None
}

fn tip_blocker(status: &PrMergeStatus) -> Option<&'static str> {
    downstack_blocker(status).or_else(|| match status.ci_status {
        CiStatus::Failure => Some("CI failed"),
        CiStatus::Pending => Some("CI is still pending"),
        CiStatus::Success | CiStatus::NoCi => {
            if status.mergeable.is_none() {
                Some("mergeability is still being checked")
            } else {
                None
            }
        }
    })
}

fn wait_for_tip_ready(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    pr_number: u64,
    timeout: Duration,
    poll_interval: Duration,
    quiet: bool,
) -> Result<WaitResult> {
    let start = Instant::now();
    let mut last_status: Option<String> = None;

    loop {
        let status = rt.block_on(async { client.get_pr_merge_status(pr_number).await })?;
        if let Some(reason) = tip_blocker(&status) {
            if matches!(
                reason,
                "CI is still pending" | "mergeability is still being checked"
            ) {
                if start.elapsed() > timeout {
                    if !quiet && last_status.is_some() {
                        println!();
                    }
                    return Ok(WaitResult::Timeout);
                }

                if !quiet {
                    let elapsed = start.elapsed().as_secs();
                    let status_text = format!(
                        "      {} Waiting for selected tip PR: {}... ({}s)",
                        "⏳".yellow(),
                        reason,
                        elapsed
                    );
                    if last_status.is_some() {
                        print!("\r{}\r", " ".repeat(96));
                    }
                    print!("{}", status_text);
                    std::io::stdout().flush().ok();
                    last_status = Some(status_text);
                }

                std::thread::sleep(poll_interval);
                continue;
            }

            if !quiet && last_status.is_some() {
                println!();
            }
            return Ok(WaitResult::Failed(reason.to_string()));
        }

        if !quiet && last_status.is_some() {
            println!();
        }
        return Ok(WaitResult::Ready(status));
    }
}

fn verify_tip_head_matches_local(
    repo: &GitRepo,
    tip: &ResolvedStackPr,
    tip_status: &PrMergeStatus,
) -> Result<()> {
    let local_sha = repo.rev_parse(&tip.branch)?;
    if local_sha != tip_status.head_sha {
        anyhow::bail!(
            "Selected tip PR #{} head SHA ({}) does not match local branch '{}' ({}). Run `stax submit` or fetch the branch before stack merging.",
            tip.pr_number,
            tip_status.head_sha,
            tip.branch,
            local_sha
        );
    }

    Ok(())
}

fn restore_tip_base(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    tip: &ResolvedStackPr,
    original_base: &str,
    quiet: bool,
) {
    let timer = LiveTimer::maybe_new(
        !quiet,
        &format!(
            "Restoring selected tip PR #{} base to {}...",
            tip.pr_number, original_base
        ),
    );
    match rt.block_on(async { client.update_pr_base(tip.pr_number, original_base).await }) {
        Ok(()) => LiveTimer::maybe_finish_ok(timer, "done"),
        Err(e) => {
            LiveTimer::maybe_finish_err(timer, "failed");
            if !quiet {
                eprintln!(
                    "{} failed to restore PR #{} base to {}: {:#}",
                    "warning:".yellow().bold(),
                    tip.pr_number,
                    original_base,
                    e
                );
            }
        }
    }
}

fn absorb_downstack_prs(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    prs: &[ResolvedStackPr],
    tip: &ResolvedStackPr,
    quiet: bool,
) -> Result<Vec<u64>> {
    let mut github_merged = Vec::new();

    for pr in prs.iter().filter(|pr| pr.pr_number != tip.pr_number) {
        let timer = LiveTimer::maybe_new(
            !quiet,
            &format!(
                "Waiting for downstack PR #{} to be marked merged...",
                pr.pr_number
            ),
        );
        if wait_for_downstack_pr_merged(rt, client, pr.pr_number)? {
            github_merged.push(pr.pr_number);
            LiveTimer::maybe_finish_ok(timer, "merged by GitHub");
            continue;
        }

        let comment = downstack_absorbed_comment(tip.pr_number);
        rt.block_on(async { client.create_issue_comment(pr.pr_number, &comment).await })?;
        rt.block_on(async { client.close_pr(pr.pr_number).await })?;
        LiveTimer::maybe_finish_ok(timer, "closed as absorbed");
    }

    Ok(github_merged)
}

fn wait_for_downstack_pr_merged(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    pr_number: u64,
) -> Result<bool> {
    let timeout = downstack_merged_wait();
    let interval = Duration::from_secs(2);
    let start = Instant::now();

    loop {
        if rt.block_on(async { client.is_pr_merged(pr_number).await })? {
            return Ok(true);
        }
        if start.elapsed() >= timeout {
            return Ok(false);
        }
        std::thread::sleep(interval);
    }
}

fn downstack_merged_wait() -> Duration {
    let seconds = std::env::var("STAX_STACK_MERGE_ABSORBED_WAIT_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);
    Duration::from_secs(seconds)
}

fn rebase_remaining_branches(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    remote_name: &str,
    trunk: &str,
    remaining: &[RemainingStackBranch],
    quiet: bool,
) -> Result<()> {
    if remaining.is_empty() {
        return Ok(());
    }

    if !quiet {
        println!();
        println!("{}", "Rebasing remaining stack branches...".dimmed());
    }

    for (idx, branch) in remaining.iter().enumerate() {
        let previous = if idx == 0 {
            None
        } else {
            Some(remaining[idx - 1].branch.as_str())
        };
        rebase_and_finalize_remaining_branch(
            repo,
            rt,
            client,
            remote_name,
            trunk,
            &branch.branch,
            branch.pr_number,
            previous,
            quiet,
        )?;
    }

    Ok(())
}

fn downstack_absorbed_comment(tip_pr_number: u64) -> String {
    format!(
        "Absorbed into stack merge of #{}. This PR's commits landed through the selected tip PR.",
        tip_pr_number
    )
}

fn cleanup_local_stack(repo: &GitRepo, scope: &StackMergeScope, quiet: bool) -> Result<()> {
    if !quiet {
        println!();
        println!("{}", "Cleaning up local stack branches...".dimmed());
    }

    let checkout_after_cleanup = if scope.downstack_only {
        &scope.current
    } else {
        &scope.trunk
    };
    let _ = repo.checkout(checkout_after_cleanup);

    for branch in &scope.to_merge {
        let local_deleted = repo.delete_branch(&branch.branch, true).is_ok();
        let _ = crate::git::refs::delete_metadata(repo.inner(), &branch.branch);

        if !quiet {
            if local_deleted {
                println!("  {} {} deleted", "✓".green(), branch.branch.dimmed());
            } else {
                println!(
                    "  {} {} kept (checked out elsewhere or already removed)",
                    "○".yellow(),
                    branch.branch.dimmed()
                );
            }
        }
    }

    Ok(())
}

fn run_post_merge_sync(quiet: bool) {
    if !quiet {
        println!();
        println!(
            "{}",
            "Running post-merge sync (no branch deletion)...".dimmed()
        );
    }

    if let Err(err) = crate::commands::sync::run(
        false, // restack
        false, // prune
        false, // full
        false, // keep branch cleanup scoped to this stack merge
        false, // delete upstream-gone branches
        true,  // force
        false, // safe
        false, // continue
        quiet,
        false, // verbose
        false, // auto_stash_pop
        &[],
    ) {
        if !quiet {
            println!();
            println!(
                "{} {}",
                "warning:".yellow().bold(),
                format!("post-merge sync failed: {}", err).yellow()
            );
            println!(
                "{}",
                "Run 'stax rs --force' manually to sync local state.".dimmed()
            );
        }
    }
}

fn print_stack_preview(
    prs: &[ResolvedStackPr],
    remaining: &[RemainingStackBranch],
    trunk: &str,
    method: MergeMethod,
    when_ready: bool,
    downstack_only: bool,
) {
    print_header("Stack Fast-Forward Merge");
    println!();
    let tip = prs
        .last()
        .expect("stack merge preview requires a selected tip PR");
    println!(
        "Will validate PR #{} once, retarget it to {}, then merge one PR:",
        tip.pr_number,
        trunk.cyan()
    );
    if downstack_only {
        println!(
            "{}",
            "Only branches below the current branch are included.".dimmed()
        );
    } else if !remaining.is_empty() {
        println!(
            "{}",
            "Branches above the current branch stay open and will be rebased/retargeted.".dimmed()
        );
    }
    println!();

    for (idx, pr) in prs.iter().enumerate() {
        let marker = if idx + 1 == prs.len() {
            "(tip, merged)"
        } else {
            "(reconciled after tip merge)"
        };
        println!(
            "  {}. {} (#{}) {}",
            (idx + 1).to_string().bold(),
            pr.branch.bold(),
            pr.pr_number,
            marker.dimmed()
        );
    }

    if !remaining.is_empty() {
        println!();
        println!("{}", "Remaining after stack merge:".dimmed());
        for (idx, branch) in remaining.iter().enumerate() {
            let target = if idx == 0 {
                trunk
            } else {
                &remaining[idx - 1].branch
            };
            let pr_text = branch
                .pr_number
                .map(|number| format!(" (#{})", number))
                .unwrap_or_default();
            println!(
                "  {} {}{} -> {}",
                "○".dimmed(),
                branch.branch.bold(),
                pr_text.dimmed(),
                target.cyan()
            );
        }
    }

    println!();
    println!(
        "Merge method: {} {}",
        method.as_str().cyan(),
        if matches!(method, MergeMethod::Rebase) {
            "(default for --stack)".dimmed()
        } else {
            "(explicit)".dimmed()
        }
    );
    if when_ready {
        println!(
            "{}",
            "Will wait only for the selected tip PR's CI and mergeability; downstack PRs are checked for review blockers."
                .dimmed()
        );
    } else {
        println!(
            "{}",
            "Requires the selected tip PR to already be green; downstack PRs are checked for review blockers."
                .dimmed()
        );
    }
}

fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| match c {
            '\x00'..='\x1f' | '\x7f' => 0,
            '\x20'..='\x7e' => 1,
            '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╯'
            | '╰' | '║' | '═' => 1,
            '←' | '→' | '↑' | '↓' => 1,
            '✓' | '✗' | '✔' | '✘' => 1,
            _ => 2,
        })
        .sum()
}

fn print_header(title: &str) {
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

fn print_header_success(title: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::stack::StackBranch;
    use std::collections::HashMap;

    #[test]
    fn stack_merge_confirmation_yes_always_approves() {
        // `--yes` is the only non-interactive approval; works with/without quiet and TTY.
        assert_eq!(
            stack_merge_confirmation(true, false, true),
            StackMergeConfirmation::Approved
        );
        assert_eq!(
            stack_merge_confirmation(true, true, false),
            StackMergeConfirmation::Approved
        );
        assert_eq!(
            stack_merge_confirmation(true, true, true),
            StackMergeConfirmation::Approved
        );
    }

    #[test]
    fn stack_merge_confirmation_quiet_without_yes_requires_yes() {
        // `--quiet` must not imply consent; without `--yes` it must refuse.
        assert_eq!(
            stack_merge_confirmation(false, true, true),
            StackMergeConfirmation::RequireYes
        );
        assert_eq!(
            stack_merge_confirmation(false, true, false),
            StackMergeConfirmation::RequireYes
        );
    }

    #[test]
    fn stack_merge_confirmation_non_tty_without_yes_requires_yes() {
        assert_eq!(
            stack_merge_confirmation(false, false, false),
            StackMergeConfirmation::RequireYes
        );
    }

    #[test]
    fn stack_merge_confirmation_interactive_without_yes_prompts() {
        assert_eq!(
            stack_merge_confirmation(false, false, true),
            StackMergeConfirmation::Prompt
        );
    }

    fn status(ci_status: CiStatus) -> PrMergeStatus {
        PrMergeStatus {
            number: 1,
            title: "Test".to_string(),
            state: "open".to_string(),
            updated_at: None,
            is_draft: false,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            ci_status,
            review_decision: Some("APPROVED".to_string()),
            approvals: 1,
            changes_requested: false,
            head_sha: "abc123".to_string(),
        }
    }

    fn tracked_stack() -> Stack {
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

    fn scope_without_repo(
        stack: &Stack,
        current: &str,
        full: bool,
        downstack_only: bool,
    ) -> (Vec<String>, Vec<String>) {
        let mut to_merge = stack.ancestors(current);
        to_merge.reverse();
        to_merge.retain(|branch| branch != &stack.trunk);

        let mut remaining = stack.descendants(current);
        if downstack_only {
            remaining.insert(0, current.to_string());
        } else {
            to_merge.push(current.to_string());
        }

        if full && !remaining.is_empty() {
            to_merge.extend(std::mem::take(&mut remaining));
        }

        (to_merge, remaining)
    }

    #[test]
    fn stack_scope_defaults_to_current_branch() {
        let stack = tracked_stack();
        let (to_merge, remaining) = scope_without_repo(&stack, "feature-b", false, false);
        assert_eq!(to_merge, vec!["feature-a", "feature-b"]);
        assert_eq!(remaining, vec!["feature-c"]);
    }

    #[test]
    fn stack_scope_full_includes_descendants() {
        let stack = tracked_stack();
        let (to_merge, remaining) = scope_without_repo(&stack, "feature-b", true, false);
        assert_eq!(to_merge, vec!["feature-a", "feature-b", "feature-c"]);
        assert!(remaining.is_empty());
    }

    #[test]
    fn stack_scope_downstack_only_excludes_current() {
        let stack = tracked_stack();
        let (to_merge, remaining) = scope_without_repo(&stack, "feature-b", false, true);
        assert_eq!(to_merge, vec!["feature-a"]);
        assert_eq!(remaining, vec!["feature-b", "feature-c"]);
    }

    #[test]
    fn stack_scope_downstack_only_direct_child_has_no_merge_targets() {
        let stack = tracked_stack();
        let (to_merge, remaining) = scope_without_repo(&stack, "feature-a", false, true);
        assert!(to_merge.is_empty());
        assert_eq!(remaining, vec!["feature-a", "feature-b", "feature-c"]);
    }

    #[test]
    fn downstack_blocker_ignores_redundant_ci_failure() {
        let mut status = status(CiStatus::Failure);
        status.review_decision = Some("APPROVED".to_string());

        assert_eq!(downstack_blocker(&status), None);
    }

    #[test]
    fn downstack_blocker_rejects_missing_required_review() {
        let mut status = status(CiStatus::Success);
        status.review_decision = Some("REVIEW_REQUIRED".to_string());

        assert_eq!(downstack_blocker(&status), Some("review required"));
    }

    #[test]
    fn tip_blocker_checks_tip_ci() {
        assert_eq!(
            tip_blocker(&status(CiStatus::Pending)),
            Some("CI is still pending")
        );
        assert_eq!(tip_blocker(&status(CiStatus::Failure)), Some("CI failed"));
        assert_eq!(tip_blocker(&status(CiStatus::Success)), None);
    }

    #[test]
    fn downstack_comment_links_to_tip() {
        assert_eq!(
            downstack_absorbed_comment(42),
            "Absorbed into stack merge of #42. This PR's commits landed through the selected tip PR."
        );
    }
}
