use crate::cache::CiCache;
use crate::commands::ci::{fetch_ci_statuses, record_ci_history};
use crate::commands::restack_conflict::{RestackConflictContext, print_restack_conflict};
use crate::commands::worktree::{
    remove::remove_worktree_with_hooks,
    shared::{compute_worktree_details, worktree_removal_blockers_for_cleanup},
};
use crate::config::Config;
use crate::engine::branch_detect::has_unique_commits_since_any_base;
use crate::engine::{BranchMetadata, PrInfo, Stack};
use crate::errors::{ConflictStopped, DirtyWorkingTree, SilentExit, exit_codes};
use crate::forge::ForgeClient;
use crate::git::repo::{BranchDeleteResolution, BranchDeleteSwitchTarget};
use crate::git::{GitRepo, RebaseResult, RebaseTimings};
use crate::github::pr::PrInfo as ForgePrInfo;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::progress::LiveTimer;
use crate::remote::{self, RemoteInfo};
use anyhow::{Context, Result, bail};
use colored::Colorize;
use dialoguer::{Confirm, Select, theme::ColorfulTheme};
use futures_util::stream::{self, StreamExt};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const PR_METADATA_REFRESH_CONCURRENCY: usize = 8;

#[derive(Debug, Default)]
pub(super) struct SyncStats {
    pub(super) trunk: Option<TrunkSummary>,
    pub(super) merged_branches_cleaned: usize,
    pub(super) deleted_branches: Vec<DeletedBranchRecord>,
    pub(super) restacked_branches: Vec<String>,
    pub(super) imported_branches_updated: Vec<String>,
    pub(super) partially_merged: Vec<PartialMergeRecord>,
    pub(super) protected_branches: Vec<String>,
    pub(super) trunk_not_updated: Option<TrunkNotUpdated>,
    pub(super) cleanup_skips: Vec<CleanupSkip>,
    pub(super) checkout_change: Option<CheckoutChange>,
    pub(super) stash: StashOutcome,
}

#[derive(Debug, Clone)]
pub(super) struct DeletedBranchRecord {
    pub(super) branch: String,
    pub(super) category: &'static str,
    pub(super) scope: &'static str,
    pub(super) tip: Option<String>,
    pub(super) metadata_deleted: bool,
}

#[derive(Debug, Clone)]
pub(super) struct PartialMergeRecord {
    pub(super) branch: String,
    pub(super) reason: &'static str,
    pub(super) pr_number: Option<u64>,
    pub(super) extra_commits: usize,
}

#[derive(Debug, Clone, Default)]
pub(super) struct StashOutcome {
    pub(super) stashed: bool,
    pub(super) restored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TrunkNotUpdated {
    pub(super) branch: String,
    pub(super) remote_ref: String,
    pub(super) failure: TrunkUpdateFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TrunkUpdateFailure {
    Diverged,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CleanupSkip {
    pub(super) branch: String,
    pub(super) reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CheckoutChange {
    pub(super) from: String,
    pub(super) to: String,
}

impl SyncStats {
    fn record_cleanup_skip(&mut self, branch: &str, reason: impl Into<String>) {
        if self.cleanup_skips.iter().any(|skip| skip.branch == branch) {
            return;
        }
        self.cleanup_skips.push(CleanupSkip {
            branch: branch.to_string(),
            reason: reason.into(),
        });
    }
}

#[derive(Debug, Clone)]
struct RestackBranchTiming {
    branch: String,
    rebase_timings: RebaseTimings,
    metadata_update: Duration,
}

impl RestackBranchTiming {
    fn total(&self) -> Duration {
        self.rebase_timings.total() + self.metadata_update
    }
}

#[derive(Debug)]
pub(super) enum TrunkSummary {
    UpToDate {
        branch: String,
    },
    Pulled {
        branch: String,
        commits: usize,
        files: usize,
        additions: usize,
        deletions: usize,
    },
    Updated {
        branch: String,
    },
}

#[derive(Debug, Clone)]
pub(super) struct BlockingWorktreeCleanup {
    pub(super) resolution: BranchDeleteResolution,
    pub(super) blockers: Vec<&'static str>,
}

impl BlockingWorktreeCleanup {
    fn can_remove_during_sync(&self) -> bool {
        !self.resolution.worktree.is_main && self.blockers.is_empty()
    }

    fn can_force_remove_dirty_worktree_during_sync(&self) -> bool {
        !self.resolution.worktree.is_main
            && !self.blockers.is_empty()
            && self.blockers.iter().all(|blocker| *blocker == "dirty")
    }

    pub(super) fn blocker_summary(&self) -> Option<String> {
        if self.resolution.worktree.is_main {
            return Some("it is the main worktree".to_string());
        }

        if self.blockers.is_empty() {
            return None;
        }

        let reasons = self
            .blockers
            .iter()
            .map(|blocker| match *blocker {
                "current" => "it is the current worktree",
                "dirty" => "it has uncommitted changes",
                "locked" => "it is locked",
                "rebase" => "a rebase is in progress",
                "merge" => "a merge is in progress",
                "conflicts" => "it has unresolved conflicts",
                other => other,
            })
            .collect::<Vec<_>>();

        Some(reasons.join(", "))
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct LocalBranchDeleteOutcome {
    deleted: bool,
    worktree_blocked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncBranchDeleteAction {
    DeleteOnly,
    PreserveWorktree,
    RemoveWorktree { force: bool },
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncFlow {
    Continue,
    Stop,
}

/// How interactive sync confirms branch deletions after the upfront plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DeleteConfirmStrategy {
    #[default]
    PerBranch,
    BulkNonBlocking,
}

/// Controls how sync handles a dirty working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StashPolicy {
    /// Prompt the user (default). In quiet/json mode, bail.
    Prompt,
    /// Stash automatically without prompting (--stash).
    Always,
    /// Never stash; bail if dirty (--no-stash). Wins over --force.
    Never,
}

impl StashPolicy {
    pub(crate) fn from_flags(stash: bool, no_stash: bool) -> Self {
        if no_stash {
            StashPolicy::Never
        } else if stash {
            StashPolicy::Always
        } else {
            StashPolicy::Prompt
        }
    }
}

pub(crate) fn prune_deprecation_warning() -> String {
    format!(
        "{}",
        "warning: --prune is deprecated and has no effect. Use --full for fetch --prune of all remote-tracking refs.".yellow()
    )
}

struct StashGuard {
    armed: bool,
}

impl StashGuard {
    fn new() -> Self {
        Self { armed: false }
    }

    fn arm(&mut self) {
        self.armed = true;
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StashGuard {
    fn drop(&mut self) {
        if self.armed {
            eprintln!("{}", stash_left_behind_warning());
        }
    }
}

fn stash_left_behind_warning() -> String {
    format!(
        "{}\n  {}",
        r#"⚠ Your changes are still stashed as "stax auto-stash"."#.yellow(),
        "Run `git stash pop` to restore them (`git stash list` to inspect).".dimmed()
    )
}

fn stale_stack_warning(consequence: &str, error: &anyhow::Error) -> String {
    format!(
        "⚠ Could not reload stack metadata; {} ({})",
        consequence, error
    )
    .yellow()
    .to_string()
}

struct SyncContext {
    workdir: PathBuf,
    config: Config,
    remote_name: String,
    remote_trunk_ref: String,
    stack: Stack,
    current: String,
    current_after_deletions: String,
    restack: bool,
    full: bool,
    delete_merged: bool,
    delete_upstream_gone: bool,
    force: bool,
    safe: bool,
    quiet: bool,
    verbose: bool,
    auto_stash_pop: bool,
    stash_policy: StashPolicy,
    json: bool,
    auto_confirm: bool,
    skip_interactive_plan: bool,
    sync_extra_fetch_refs: Vec<String>,
    imported_branches: Vec<String>,
    remote_delete_exempt_imported_branches: HashSet<String>,
    remote_branches_for_merged: Option<HashSet<String>>,
    local_trunk_before_sync: Option<String>,
    remote_trunk_after_fetch: Option<String>,
    updated_imported_branches: Vec<String>,
    stashed: bool,
    stash_restored: bool,
    stash_guard: StashGuard,
    stale_stack_warning_shown: bool,
    trunk_update_deferred: bool,
    sync_started_at: Instant,
    step_timings: Vec<(String, Duration)>,
    restack_branch_timings: Vec<RestackBranchTiming>,
    stats: SyncStats,
    /// Sync-wide undo transaction; lazily snapshotted — None until begin_transaction is called.
    tx: Option<Transaction>,
    /// Whether trunk has already been planned in the sync transaction.
    trunk_planned: bool,
    delete_confirm_strategy: DeleteConfirmStrategy,
    /// Merged-branch detection from the interactive sync plan (pre-trunk-update).
    /// Reused after trunk moves to avoid a second full patch-id scan of the stack.
    planned_merged_detection: Option<PlannedMergedDetection>,
}

#[derive(Debug, Clone)]
struct PlannedMergedDetection {
    merged: Vec<MergedBranchInfo>,
    partially_merged: Vec<PartiallyMergedNote>,
}

impl SyncContext {
    #[allow(clippy::too_many_arguments)]
    fn new(
        repo: GitRepo,
        config: Config,
        auto_confirm: bool,
        sync_started_at: Instant,
        restack: bool,
        full: bool,
        delete_merged: bool,
        delete_upstream_gone: bool,
        force: bool,
        safe: bool,
        quiet: bool,
        verbose: bool,
        auto_stash_pop: bool,
        stash_policy: StashPolicy,
        json: bool,
        extra_fetch_refs: &[String],
        skip_interactive_plan: bool,
    ) -> Result<(Self, GitRepo)> {
        let stack = Stack::load(&repo)?;
        let current = repo.current_branch()?;
        let workdir = repo.workdir()?.to_path_buf();
        let remote_name = config.remote_name().to_string();
        let remote_trunk_ref = format!("{}/{}", remote_name, stack.trunk);
        let imported_branches = imported_branches_for_remote(&repo, &stack, &remote_name)?;
        let remote_delete_exempt_imported_branches = imported_branches_for_cleanup(&repo, &stack)?;
        let mut sync_extra_fetch_refs = extra_fetch_refs.to_vec();
        for branch in &imported_branches {
            if !sync_extra_fetch_refs.contains(branch) {
                sync_extra_fetch_refs.push(branch.clone());
            }
        }
        let current_after_deletions = current.clone();
        let effective_quiet = quiet || json;
        Ok((
            Self {
                workdir,
                config,
                remote_name,
                remote_trunk_ref,
                stack,
                current,
                current_after_deletions,
                restack,
                full,
                delete_merged,
                delete_upstream_gone,
                force,
                safe,
                quiet: effective_quiet,
                verbose,
                auto_stash_pop,
                stash_policy,
                json,
                auto_confirm,
                skip_interactive_plan,
                sync_extra_fetch_refs,
                imported_branches,
                remote_delete_exempt_imported_branches,
                remote_branches_for_merged: None,
                local_trunk_before_sync: None,
                remote_trunk_after_fetch: None,
                updated_imported_branches: Vec::new(),
                stashed: false,
                stash_restored: false,
                stash_guard: StashGuard::new(),
                stale_stack_warning_shown: false,
                trunk_update_deferred: false,
                sync_started_at,
                step_timings: Vec::new(),
                restack_branch_timings: Vec::new(),
                stats: SyncStats::default(),
                tx: None,
                trunk_planned: false,
                delete_confirm_strategy: DeleteConfirmStrategy::PerBranch,
                planned_merged_detection: None,
            },
            repo,
        ))
    }

    fn handle_dirty_tree(&mut self, repo: &GitRepo) -> Result<SyncFlow> {
        if repo.is_dirty()? {
            match self.stash_policy {
                StashPolicy::Never => {
                    return Err(DirtyWorkingTree.into());
                }
                StashPolicy::Always => {
                    let stash_started_at = Instant::now();
                    self.stashed = repo.stash_push()?;
                    self.auto_stash_pop = true;
                    if self.stashed {
                        self.stash_guard.arm();
                    }
                    self.step_timings
                        .push(("stash working tree".to_string(), stash_started_at.elapsed()));
                    if !self.quiet {
                        println!("{}", "✓ Stashed working tree changes.".green());
                    }
                }
                StashPolicy::Prompt => {
                    if self.quiet {
                        return Err(DirtyWorkingTree.into());
                    }

                    let stash = if self.auto_confirm {
                        true
                    } else {
                        Confirm::with_theme(&ColorfulTheme::default())
                            .with_prompt(
                                "Working tree has uncommitted changes. Stash them before sync?",
                            )
                            .default(true)
                            .interact()?
                    };

                    if stash {
                        let stash_started_at = Instant::now();
                        self.stashed = repo.stash_push()?;
                        self.auto_stash_pop = true;
                        if self.stashed {
                            self.stash_guard.arm();
                        }
                        self.step_timings
                            .push(("stash working tree".to_string(), stash_started_at.elapsed()));
                        if !self.quiet {
                            println!("{}", "✓ Stashed working tree changes.".green());
                        }
                    } else {
                        if !self.json {
                            println!("{}", "Aborted.".red());
                        }
                        return Ok(SyncFlow::Stop);
                    }
                }
            }
        }
        Ok(SyncFlow::Continue)
    }

    fn begin_transaction(&mut self, repo: &GitRepo) -> Result<()> {
        let tx = Transaction::begin(OpKind::Sync, repo, self.quiet)?;
        self.tx = Some(tx);
        Ok(())
    }

    /// Close the sync-wide transaction.
    ///
    /// A no-op sync (nothing was snapshotted) leaves NO receipt so the previous
    /// undoable receipt stays on top of the undo stack.  When the transaction
    /// was snapshotted at least once we record the final head branch and
    /// auto-stash state before writing the success receipt.
    fn finish_transaction(&mut self, current_after: &str) -> Result<()> {
        let Some(mut tx) = self.tx.take() else {
            return Ok(());
        };
        if !tx.is_snapshotted() {
            return Ok(());
        }
        tx.set_auto_stash_pop(self.auto_stash_pop);
        tx.set_head_branch_after(current_after);
        tx.finish_ok()
    }

    fn fetch_remote(&mut self, repo: &GitRepo) -> Result<()> {
        // 1. Fetch from remote
        // Default: trunk-only fetch + `ls-remote --heads` in parallel (fast on large repos).
        // `--full`: classic `fetch --prune` for all remote-tracking refs, tags included.
        let fetch_timer = LiveTimer::maybe_new(!self.quiet, &format!("Fetch {}", self.remote_name));

        let fetch_started_at = Instant::now();
        let output;
        // Remote branch names for merged detection (`None` when `--no-delete`: trunk-only fetch).
        let remote_branches_for_merged: Option<HashSet<String>>;
        let remote_heads_for_extra_fetch = if !self.full && !self.sync_extra_fetch_refs.is_empty() {
            Some(
                remote::ls_remote_heads(&self.workdir, &self.remote_name)
                    .context("Failed to list remote heads before fetch")?,
            )
        } else {
            None
        };
        let fetch_refs = sync_fetch_refs(
            &self.stack.trunk,
            &self.sync_extra_fetch_refs,
            remote_heads_for_extra_fetch.as_ref(),
        );

        if self.full {
            let fetch_args: Vec<&str> = vec!["fetch", "--prune", self.remote_name.as_str()];
            output = Command::new("git")
                .args(&fetch_args)
                .current_dir(&self.workdir)
                .output()
                .context("Failed to fetch")?;
            remote_branches_for_merged = if self.delete_merged {
                Some(
                    repo.remote_branch_names(&self.remote_name)
                        .context("Failed to read remote-tracking branches after fetch")?,
                )
            } else {
                None
            };
        } else if self.delete_merged && remote_heads_for_extra_fetch.is_none() {
            let workdir_fetch = self.workdir.clone();
            let remote_fetch = self.remote_name.clone();
            let fetch_refs = fetch_refs.clone();
            let workdir_ls = self.workdir.clone();
            let remote_ls = self.remote_name.clone();

            let fetch_handle = std::thread::spawn(move || {
                Command::new("git")
                    .arg("fetch")
                    .arg("--no-tags")
                    .arg(remote_fetch)
                    .args(fetch_refs)
                    .current_dir(&workdir_fetch)
                    .output()
            });

            let ls_handle =
                std::thread::spawn(move || remote::ls_remote_heads(&workdir_ls, &remote_ls));

            output = fetch_handle
                .join()
                .map_err(|_| anyhow::anyhow!("fetch thread panicked"))?
                .context("Failed to fetch")?;

            let heads = ls_handle
                .join()
                .map_err(|_| anyhow::anyhow!("git ls-remote thread panicked"))??;
            if output.status.success() {
                prune_stale_remote_tracking_refs(
                    &self.workdir,
                    self.remote_name.as_str(),
                    &self.stack,
                    &heads,
                );
            }
            remote_branches_for_merged = Some(heads);
        } else if self.delete_merged {
            output = Command::new("git")
                .arg("fetch")
                .arg("--no-tags")
                .arg(self.remote_name.as_str())
                .args(&fetch_refs)
                .current_dir(&self.workdir)
                .output()
                .context("Failed to fetch")?;
            let heads = remote_heads_for_extra_fetch.expect("remote heads checked for extra refs");
            if output.status.success() {
                prune_stale_remote_tracking_refs(
                    &self.workdir,
                    self.remote_name.as_str(),
                    &self.stack,
                    &heads,
                );
            }
            remote_branches_for_merged = Some(heads);
        } else {
            output = Command::new("git")
                .arg("fetch")
                .arg("--no-tags")
                .arg(self.remote_name.as_str())
                .args(&fetch_refs)
                .current_dir(&self.workdir)
                .output()
                .context("Failed to fetch")?;
            remote_branches_for_merged = None;
        }

        self.step_timings.push((
            format!("fetch {}", self.remote_name),
            fetch_started_at.elapsed(),
        ));

        let fetch_succeeded = output.status.success();
        if fetch_succeeded {
            LiveTimer::maybe_finish_timed(fetch_timer);
            if !self.quiet && self.verbose {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    for line in stderr.lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
        } else {
            // Fetch may fail partially (lock files, etc.) but still update most refs
            LiveTimer::maybe_finish_warn(fetch_timer, "done (with warnings)");
            if !self.quiet && self.verbose {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    for line in stderr.lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
        }

        if self.restack && !fetch_succeeded {
            restore_stashed_changes(repo, self.stashed, self.quiet)?;
            self.stash_guard.disarm();
            anyhow::bail!(
                "Cannot restack because fetching {} did not succeed.\n\
             Restore access to {}, then retry.",
                self.remote_name,
                self.remote_name,
            );
        }

        self.local_trunk_before_sync = resolve_ref_oid(&self.workdir, &self.stack.trunk);
        self.remote_trunk_after_fetch = resolve_ref_oid(&self.workdir, &self.remote_trunk_ref);
        self.remote_branches_for_merged = remote_branches_for_merged;
        Ok(())
    }

    // Compute the exact trunk transition as soon as both fixed endpoints are known. This
    // overlaps the diff with trunk update, merged-branch detection, and optional restack.
    fn spawn_trunk_summary_worker(&self) -> std::thread::JoinHandle<Result<Option<TrunkSummary>>> {
        let workdir = self.workdir.clone();
        let branch = self.stack.trunk.clone();
        let local_before = self.local_trunk_before_sync.clone();
        let remote_after = self.remote_trunk_after_fetch.clone();
        std::thread::spawn(move || {
            summarize_trunk_transition(
                &workdir,
                &branch,
                local_before.as_deref(),
                remote_after.as_deref(),
            )
        })
    }

    // Update trunk before merged branch detection, so detection works correctly.
    // Note: If we're not on trunk, we use a refspec fetch which may fail if local trunk
    // has diverged. This is fine - we'll retry after branch deletions if we end up on trunk.
    fn update_trunk(&mut self, repo: &GitRepo) -> Result<()> {
        let was_on_trunk = self.current == self.stack.trunk;
        let update_trunk_started_at = Instant::now();

        if was_on_trunk {
            // We're on trunk - pull directly
            let update_timer =
                LiveTimer::maybe_new(!self.quiet, &format!("Update {}", self.stack.trunk));

            let workdir = self.workdir.clone();
            self.fast_forward_trunk_in(repo, &workdir, update_timer, false)?;
        } else {
            let update_timer =
                LiveTimer::maybe_new(!self.quiet, &format!("Update {}", self.stack.trunk));

            if let Some(trunk_worktree_path) = repo.branch_worktree_path(&self.stack.trunk)? {
                self.fast_forward_trunk_in(repo, &trunk_worktree_path, update_timer, true)?;
            } else {
                // Trunk isn't checked out in any worktree.
                // Resolve the two SHAs so we can give an accurate status message.
                let local_sha = resolve_ref_oid(&self.workdir, &self.stack.trunk);
                let remote_sha = resolve_ref_oid(&self.workdir, &self.remote_trunk_ref);

                match (local_sha, remote_sha) {
                    (Some(ref local), Some(ref remote)) if local == remote => {
                        // Already up to date — nothing to do.
                        LiveTimer::maybe_finish_timed(update_timer);
                    }
                    (Some(_), Some(_)) => {
                        // Check if a fast-forward is safe (local trunk is an ancestor of remote).
                        let ff_possible =
                            is_ancestor(&self.workdir, &self.stack.trunk, &self.remote_trunk_ref);

                        if ff_possible {
                            let workdir = self.workdir.clone();
                            self.plan_trunk_move(repo, &workdir)?;
                            let output = Command::new("git")
                                .args([
                                    "update-ref",
                                    &format!("refs/heads/{}", self.stack.trunk),
                                    &format!(
                                        "refs/remotes/{}/{}",
                                        self.remote_name, self.stack.trunk
                                    ),
                                ])
                                .current_dir(&self.workdir)
                                .output()
                                .context("Failed to fast-forward local trunk ref")?;

                            if output.status.success() {
                                LiveTimer::maybe_finish_timed(update_timer);
                                let workdir = self.workdir.clone();
                                if let Some(ref mut tx) = self.tx {
                                    let trunk_after = resolve_ref_oid(&workdir, &self.stack.trunk);
                                    tx.record_known_after(
                                        &self.stack.trunk.clone(),
                                        trunk_after.as_deref(),
                                    );
                                }
                            } else {
                                self.trunk_update_deferred = true;
                                LiveTimer::maybe_finish_skipped(
                                    update_timer,
                                    "couldn't update — run 'stax trunk' to pull",
                                );
                            }
                        } else {
                            // Local trunk has commits not on the remote — can't fast-forward.
                            self.trunk_update_deferred = true;
                            LiveTimer::maybe_finish_skipped(
                                update_timer,
                                &format!(
                                    "local {} has unpushed commits — run 'stax trunk' to sync",
                                    self.stack.trunk
                                ),
                            );
                        }
                    }
                    _ => {
                        // Couldn't resolve one or both refs (shouldn't happen after a successful fetch).
                        self.trunk_update_deferred = true;
                        LiveTimer::maybe_finish_skipped(
                            update_timer,
                            "couldn't resolve ref — run 'stax trunk' to pull",
                        );
                    }
                }
            }
        }
        self.step_timings.push((
            format!("update {}", self.stack.trunk),
            update_trunk_started_at.elapsed(),
        ));
        Ok(())
    }

    /// Snapshot the trunk branch in the sync transaction before we touch it.
    ///
    /// Idempotent: returns immediately if already planned, or if local and
    /// remote already agree (nothing will change).
    fn plan_trunk_move(&mut self, repo: &GitRepo, dir: &Path) -> Result<()> {
        if self.trunk_planned {
            return Ok(());
        }
        let trunk_name = self.stack.trunk.clone();
        let remote_trunk_ref = self.remote_trunk_ref.clone();
        let local_oid = resolve_ref_oid(dir, &trunk_name);
        let remote_oid = resolve_ref_oid(dir, &remote_trunk_ref);
        if local_oid.is_some() && local_oid == remote_oid {
            return Ok(());
        }
        if let Some(ref mut tx) = self.tx {
            tx.plan_branch(repo, &trunk_name)?;
            tx.snapshot()?;
        }
        self.trunk_planned = true;
        Ok(())
    }

    fn fast_forward_trunk_in(
        &mut self,
        repo: &GitRepo,
        dir: &Path,
        timer: Option<LiveTimer>,
        in_linked_worktree: bool,
    ) -> Result<()> {
        self.plan_trunk_move(repo, dir)?;
        let output = Command::new("git")
            .args(["merge", "--ff-only", &self.remote_trunk_ref])
            .current_dir(dir)
            .output()
            .context(if in_linked_worktree {
                "Failed to fast-forward trunk in its worktree"
            } else {
                "Failed to fast-forward trunk"
            })?;

        if output.status.success() {
            LiveTimer::maybe_finish_timed(timer);
            if let Some(ref mut tx) = self.tx {
                let trunk_after = resolve_ref_oid(dir, &self.stack.trunk);
                tx.record_known_after(&self.stack.trunk.clone(), trunk_after.as_deref());
            }
            if !in_linked_worktree && !self.quiet && self.verbose {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() {
                    for line in stdout.lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
        } else if self.safe {
            LiveTimer::maybe_finish_warn(timer, "failed (safe mode, no reset)");
            if !self.quiet && self.verbose {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    for line in stderr.lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
        } else if !is_ancestor(dir, &self.stack.trunk, &self.remote_trunk_ref) {
            LiveTimer::maybe_finish_warn(
                timer,
                "diverged (local has commits not on remote; rebase or reset trunk manually)",
            );
        } else {
            let reset_output = Command::new("git")
                .args(["reset", "--hard", &self.remote_trunk_ref])
                .current_dir(dir)
                .output()
                .context(if in_linked_worktree {
                    "Failed to reset trunk in its worktree"
                } else {
                    "Failed to reset trunk"
                })?;

            if reset_output.status.success() {
                LiveTimer::maybe_finish_warn(timer, "reset to remote");
                if let Some(ref mut tx) = self.tx {
                    let trunk_after = resolve_ref_oid(dir, &self.stack.trunk);
                    tx.record_known_after(&self.stack.trunk.clone(), trunk_after.as_deref());
                }
            } else {
                LiveTimer::maybe_finish_err(timer, "failed");
                if !self.quiet && self.verbose {
                    let stderr = String::from_utf8_lossy(&reset_output.stderr);
                    if !stderr.trim().is_empty() {
                        for line in stderr.lines() {
                            println!("    {}", line.dimmed());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn ensure_trunk_ready_for_restack(&mut self, repo: &GitRepo) -> Result<()> {
        // Restack is a history-rewriting operation, so fail closed before imported-branch
        // refresh or merged-branch cleanup can move any feature refs. Keep the later check
        // as a second boundary in case cleanup itself changes trunk state.
        if self.restack
            && !trunk_reached_remote(
                &self.workdir,
                &self.stack.trunk,
                self.remote_trunk_after_fetch.as_deref(),
            )
        {
            restore_stashed_changes(repo, self.stashed, self.quiet)?;
            self.stash_guard.disarm();
            anyhow::bail!(
                "Cannot restack because {} did not reach {}.\n\
             Inspect and reconcile {} with {}, then retry.",
                self.stack.trunk,
                self.remote_trunk_ref,
                self.stack.trunk,
                self.remote_trunk_ref,
            );
        }
        Ok(())
    }

    fn refresh_imported(&mut self, repo: &GitRepo) -> Result<()> {
        let imported_update_started_at = Instant::now();
        self.updated_imported_branches = refresh_imported_branches(
            repo,
            &self.workdir,
            &self.remote_name,
            &self.imported_branches,
            self.force,
            self.quiet,
            self.verbose,
        )?;
        self.stats.imported_branches_updated = self.updated_imported_branches.clone();
        if !self.imported_branches.is_empty() {
            self.step_timings.push((
                "update imported branches".to_string(),
                imported_update_started_at.elapsed(),
            ));
        }
        Ok(())
    }

    fn refresh_pr_states(&mut self, repo: &GitRepo) -> Result<()> {
        // Refresh live PR state before merged-branch detection so squash-merged PRs
        // (missed by `git branch --merged` when the remote branch still exists) are
        // visible via Method 2 on the first sync after merge.
        if let Some(pr_refresh_elapsed) = refresh_pr_draft_states(repo, &self.config, self.quiet) {
            self.step_timings
                .push(("refresh PR metadata".to_string(), pr_refresh_elapsed));
            self.stack = Stack::load(repo)?;
        }
        Ok(())
    }

    /// Branches in restack order for the stack we started on (trunk excluded, frozen skipped).
    fn unfrozen_restack_scope(&self, repo: &GitRepo) -> Result<Vec<String>> {
        let scope_order: Vec<String> = if self.current != self.stack.trunk
            && self.stack.branches.contains_key(&self.current)
        {
            self.stack.current_stack(&self.current)
        } else {
            Vec::new()
        };
        Ok(scope_order
            .into_iter()
            .filter(|branch| !BranchMetadata::is_frozen(repo.inner(), branch).unwrap_or(false))
            .collect())
    }

    /// Branches that will be rebased during restack: from the first stale branch through
    /// the stack tip (each rebase moves the parent pointer for branches above it).
    fn planned_restack_branches(&self, repo: &GitRepo) -> Result<Vec<String>> {
        let scope = self.unfrozen_restack_scope(repo)?;
        let Some(first_stale) = scope.iter().position(|branch| {
            self.stack
                .branches
                .get(branch)
                .map(|info| info.needs_restack)
                .unwrap_or(false)
        }) else {
            return Ok(Vec::new());
        };
        Ok(scope[first_stale..].to_vec())
    }

    /// Interactive-only: show a consolidated plan after fetch and before trunk
    /// updates or deletions. Cancelling here leaves no transaction snapshot.
    fn confirm_sync_plan(&mut self, repo: &GitRepo) -> Result<SyncFlow> {
        if self.quiet || self.auto_confirm || self.json || self.skip_interactive_plan {
            return Ok(SyncFlow::Continue);
        }

        let local_trunk = self.local_trunk_before_sync.as_deref();
        let remote_trunk = self.remote_trunk_after_fetch.as_deref();

        let mut merged_branch_names: Vec<String> = Vec::new();
        if self.delete_merged
            && let Some(remote_branches) = self.remote_branches_for_merged.as_ref()
        {
            let merged = find_merged_branches(
                repo,
                &self.workdir,
                &self.stack,
                &self.remote_name,
                remote_branches,
                false,
            )?;
            let partially_merged_notes = find_partially_merged_notes(
                repo,
                &self.workdir,
                &self.stack,
                &self.remote_name,
                remote_branches,
                &merged,
            )?;
            self.planned_merged_detection = Some(PlannedMergedDetection {
                merged: merged.clone(),
                partially_merged: partially_merged_notes,
            });
            merged_branch_names = merged.into_iter().map(|m| m.branch).collect();
        }

        let mut upstream_gone_deletable: Vec<String> = Vec::new();
        if self.delete_upstream_gone {
            let detected = find_upstream_gone_branches(&self.workdir, &self.stack.trunk)?;
            for branch in detected {
                if has_unique_commits_since_any_base(
                    &self.workdir,
                    &branch,
                    &[self.stack.trunk.as_str(), self.remote_trunk_ref.as_str()],
                )? {
                    continue;
                }
                upstream_gone_deletable.push(branch);
            }
        }

        let mut restack_candidates: Vec<String> = Vec::new();
        if self.restack {
            restack_candidates = self.planned_restack_branches(repo)?;
        }

        let has_deletion_candidates =
            !merged_branch_names.is_empty() || !upstream_gone_deletable.is_empty();

        // A trunk fast-forward is what `stax sync` is *for* — don't ask about it on its own.
        // Only branch deletions and history-rewriting restacks are worth a prompt.
        let needs_confirm = has_deletion_candidates || !restack_candidates.is_empty();

        if !needs_confirm {
            return Ok(SyncFlow::Continue);
        }

        println!();
        println!("{}", "Sync plan".bold());
        println!("  {} Trunk {}:", "▸".dimmed(), self.stack.trunk.cyan());
        match (local_trunk, remote_trunk) {
            (Some(l), Some(r)) if l == r => {
                println!(
                    "      Already up to date with {}",
                    self.remote_trunk_ref.dimmed()
                );
            }
            (Some(_), Some(_)) => {
                println!(
                    "      Would update {} to match {}",
                    self.stack.trunk.cyan(),
                    self.remote_trunk_ref.dimmed()
                );
            }
            _ => {
                println!("      Remote trunk state unavailable (offline?)");
            }
        }

        if !merged_branch_names.is_empty() {
            print_cleanup_candidates_with_stack("merged", &merged_branch_names, Some(&self.stack));
        }
        if !upstream_gone_deletable.is_empty() {
            print_cleanup_candidates_with_stack(
                "upstream-gone",
                &upstream_gone_deletable,
                Some(&self.stack),
            );
        }
        if !restack_candidates.is_empty() {
            let word = if restack_candidates.len() == 1 {
                "branch"
            } else {
                "branches"
            };
            println!(
                "    Would restack {} {}:",
                restack_candidates.len().to_string().cyan(),
                word
            );
            for branch in &restack_candidates {
                println!("      {} {}", "▸".bright_black(), branch);
            }
            println!();
        }

        let selected = if has_deletion_candidates {
            let options = [
                "Continue — delete all listed branches",
                "Choose action for each branch",
                "Cancel sync",
            ];
            Select::with_theme(&ColorfulTheme::default())
                .with_prompt("How should sync proceed?")
                .items(options)
                .default(0)
                .interact()?
        } else {
            let options = if !restack_candidates.is_empty() {
                ["Proceed with restack", "Cancel"]
            } else {
                ["Continue sync", "Cancel sync"]
            };
            Select::with_theme(&ColorfulTheme::default())
                .with_prompt("How should sync proceed?")
                .items(options)
                .default(0)
                .interact()?
        };

        if has_deletion_candidates {
            match selected {
                0 => {
                    self.delete_confirm_strategy = DeleteConfirmStrategy::BulkNonBlocking;
                    Ok(SyncFlow::Continue)
                }
                1 => Ok(SyncFlow::Continue),
                2 => Ok(SyncFlow::Stop),
                _ => Ok(SyncFlow::Continue),
            }
        } else {
            match selected {
                1 => Ok(SyncFlow::Stop),
                _ => Ok(SyncFlow::Continue),
            }
        }
    }

    fn plan_delete_blocker(
        &self,
        repo: &GitRepo,
        branch: &str,
        is_current_branch: bool,
    ) -> Result<Option<BlockingWorktreeCleanup>> {
        if is_current_branch {
            Ok(None)
        } else {
            plan_blocking_worktree_cleanup(repo, branch, self.force)
        }
    }

    fn decide_delete_action(
        &self,
        branch: &str,
        prompt: String,
        blocking: Option<&BlockingWorktreeCleanup>,
    ) -> Result<SyncBranchDeleteAction> {
        if self.auto_confirm {
            if blocking.is_some() {
                Ok(SyncBranchDeleteAction::PreserveWorktree)
            } else {
                Ok(SyncBranchDeleteAction::DeleteOnly)
            }
        } else if self.delete_confirm_strategy == DeleteConfirmStrategy::BulkNonBlocking {
            if let Some(cleanup) = blocking {
                choose_linked_worktree_delete_action(branch, cleanup)
            } else {
                Ok(SyncBranchDeleteAction::DeleteOnly)
            }
        } else if self.quiet {
            Ok(SyncBranchDeleteAction::Skip)
        } else if let Some(cleanup) = blocking {
            choose_linked_worktree_delete_action(branch, cleanup)
        } else {
            let confirm = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt(prompt)
                .default(true)
                .interact()?;
            if !confirm {
                Ok(SyncBranchDeleteAction::Skip)
            } else {
                Ok(SyncBranchDeleteAction::DeleteOnly)
            }
        }
    }

    fn record_unconfirmed_skip(&mut self, branch: &str) {
        self.stats.record_cleanup_skip(branch, "not confirmed");
        if !self.quiet {
            println!("    {} {}", branch.bright_black(), "skipped".dimmed());
        }
    }

    /// Returns `true` if the worktree was preserved (or failed with a skip recorded).
    /// Returns `false` when the resolution was missing (caller should `continue`).
    fn preserve_blocking_worktree(
        &mut self,
        repo: &GitRepo,
        branch: &str,
        blocking: Option<&BlockingWorktreeCleanup>,
    ) -> bool {
        let Some(cleanup) = blocking else {
            self.stats
                .record_cleanup_skip(branch, "worktree resolution missing");
            return false;
        };
        if let Err(error) = preserve_worktree_for_sync(repo, cleanup, self.quiet) {
            self.stats.record_cleanup_skip(
                branch,
                format!("couldn't preserve linked worktree: {}", error),
            );
            if !self.quiet {
                println!(
                    "    {} couldn't preserve linked worktree '{}': {}",
                    "↷".yellow(),
                    cleanup.resolution.worktree.name,
                    error
                );
            }
            return false;
        }
        true
    }

    fn record_checkout_failure_skip(&mut self, branch: &str, target: &str, error: String) {
        self.stats.record_cleanup_skip(branch, "checkout failed");
        if !self.quiet {
            println!(
                "    {} {}",
                branch.bright_black(),
                format!("failed to checkout '{}': {}, skipping", target, error).red()
            );
        }
    }

    fn delete_local_branch(
        &self,
        repo: &GitRepo,
        branch: &str,
        action: SyncBranchDeleteAction,
        blocking: Option<&BlockingWorktreeCleanup>,
    ) -> Result<LocalBranchDeleteOutcome> {
        delete_local_branch_for_sync(
            repo,
            &self.config,
            &self.workdir,
            branch,
            if matches!(action, SyncBranchDeleteAction::RemoveWorktree { .. }) {
                blocking
            } else {
                None
            },
            matches!(
                action,
                SyncBranchDeleteAction::RemoveWorktree { force: true }
            ),
            self.quiet,
        )
    }

    /// Returns `true` if metadata was deleted; unconditional warning on error.
    fn delete_branch_metadata(
        &mut self,
        repo: &GitRepo,
        branch: &str,
        local_still_exists: bool,
    ) -> bool {
        if local_still_exists {
            return false;
        }
        match crate::git::refs::delete_metadata(repo.inner(), branch) {
            Ok(()) => true,
            Err(e) => {
                self.stats
                    .record_cleanup_skip(branch, "metadata cleanup failed");
                let msg = format!("Warning: failed to delete metadata for '{}': {}", branch, e)
                    .yellow()
                    .to_string();
                if self.json {
                    eprintln!("{}", msg);
                } else {
                    println!("{}", msg);
                }
                false
            }
        }
    }

    fn record_local_branch_kept_skip(
        &mut self,
        branch: &str,
        blocking: Option<&BlockingWorktreeCleanup>,
    ) {
        let reason = blocking
            .and_then(BlockingWorktreeCleanup::blocker_summary)
            .unwrap_or_else(|| "local branch kept".to_string());
        self.stats.record_cleanup_skip(branch, reason);
    }

    fn cleanup_merged_branches(&mut self, repo: GitRepo) -> Result<GitRepo> {
        if self.delete_merged {
            let detect_merged_started_at = Instant::now();
            let detect_timer = LiveTimer::maybe_new(!self.quiet, "Detect merged branches");
            let remote_branches = self
                .remote_branches_for_merged
                .as_ref()
                .expect("remote branch list when deleting merged branches");
            let (merged, partially_merged_notes) =
                if let Some(planned) = self.planned_merged_detection.take() {
                    let fresh = find_merged_branches(
                        &repo,
                        &self.workdir,
                        &self.stack,
                        &self.remote_name,
                        remote_branches,
                        true,
                    )?;
                    let merged = merge_planned_merged_detection(planned.merged, fresh);
                    let partially_merged_notes: Vec<PartiallyMergedNote> = planned
                        .partially_merged
                        .into_iter()
                        .filter(|note| !merged.iter().any(|m| m.branch == note.branch))
                        .collect();
                    (merged, partially_merged_notes)
                } else {
                    let merged = find_merged_branches(
                        &repo,
                        &self.workdir,
                        &self.stack,
                        &self.remote_name,
                        remote_branches,
                        false,
                    )?;
                    let partially_merged_notes = find_partially_merged_notes(
                        &repo,
                        &self.workdir,
                        &self.stack,
                        &self.remote_name,
                        remote_branches,
                        &merged,
                    )?;
                    (merged, partially_merged_notes)
                };
            self.step_timings.push((
                "detect merged branches".to_string(),
                detect_merged_started_at.elapsed(),
            ));
            LiveTimer::maybe_finish_timed(detect_timer);

            let delete_merged_started_at = Instant::now();
            let repo = repo.refresh()?;

            // Initialize forge client once up-front for any PR base updates below.
            let forge_client = init_forge_client(&repo, &self.config);

            if !merged.is_empty() {
                let merged_branch_names: Vec<String> =
                    merged.iter().map(|m| m.branch.clone()).collect();

                if !self.quiet {
                    print_cleanup_candidates("merged", &merged_branch_names);
                }

                // Record CI history for merged branches before deleting them
                if let Some((ref rt, ref client)) = forge_client {
                    record_ci_history_for_merged(
                        &repo,
                        rt,
                        client,
                        &merged_branch_names,
                        &self.stack,
                        self.quiet,
                    );
                }
                let mut deletion_decisions = Vec::new();
                for merged_info in &merged {
                    let branch = &merged_info.branch;
                    let is_current_branch = branch == &self.current;

                    let blocking_worktree_cleanup =
                        self.plan_delete_blocker(&repo, branch, is_current_branch)?;

                    // For the prompt we use merged_branch_names (all detected merges) as
                    // the doomed set — an approximation, since the user hasn't confirmed
                    // deletions yet. The actual checkout in the second pass uses the
                    // confirmed set, so if the user declines some branches the effective
                    // parent may be closer in the chain than what the prompt suggests.
                    let prompt_parent = if is_current_branch {
                        Some(
                            resolve_fallback_parent_skipping_doomed(
                                &self.workdir,
                                &self.stack,
                                branch,
                                &merged_branch_names,
                            )
                            .0,
                        )
                    } else {
                        None
                    };
                    let prompt = sync_delete_prompt(
                        branch,
                        if is_current_branch {
                            prompt_parent.as_deref()
                        } else {
                            None
                        },
                        None,
                        blocking_worktree_cleanup.as_ref(),
                    );

                    let action = self.decide_delete_action(
                        branch,
                        prompt,
                        blocking_worktree_cleanup.as_ref(),
                    )?;

                    if action != SyncBranchDeleteAction::Skip {
                        deletion_decisions.push((
                            merged_info.clone(),
                            blocking_worktree_cleanup,
                            action,
                        ));
                    } else {
                        self.record_unconfirmed_skip(branch);
                    }
                }

                let confirmed_branch_names: Vec<String> = deletion_decisions
                    .iter()
                    .map(|(info, _, _)| info.branch.clone())
                    .collect();
                let confirmed_deletions: HashSet<String> =
                    confirmed_branch_names.iter().cloned().collect();

                for (merged_info, blocking_worktree_cleanup, action) in deletion_decisions {
                    let branch = &merged_info.branch;
                    let merge_type = &merged_info.merge_type;
                    let is_current_branch = branch == &self.current;

                    // Resolve parent branch for checkout/reparent, skipping any
                    // branch that was also confirmed for deletion in this pass.
                    let (parent_branch, parent_fallback_from) =
                        resolve_fallback_parent_skipping_doomed(
                            &self.workdir,
                            &self.stack,
                            branch,
                            &confirmed_branch_names,
                        );
                    let parent_exists_locally = local_branch_exists(&self.workdir, &parent_branch);

                    if !self.quiet
                        && let Some(missing_parent) = &parent_fallback_from
                    {
                        println!(
                            "    {} parent {} not available; using {}",
                            "↪".yellow(),
                            missing_parent.yellow(),
                            parent_branch.cyan()
                        );
                    }

                    if !parent_exists_locally {
                        self.stats.record_cleanup_skip(
                            branch,
                            format!("missing local parent {}", parent_branch),
                        );
                        if !self.quiet {
                            println!(
                            "    {} {}",
                            branch.bright_black(),
                            format!(
                                "couldn't resolve a local parent branch (wanted '{}'), skipping",
                                parent_branch
                            )
                            .red()
                        );
                        }
                        continue;
                    }

                    if action == SyncBranchDeleteAction::PreserveWorktree
                        && !self.preserve_blocking_worktree(
                            &repo,
                            branch,
                            blocking_worktree_cleanup.as_ref(),
                        )
                    {
                        continue;
                    }

                    // Handle squash-merged branches with surviving children.
                    if matches!(merge_type, MergeType::SquashMerge) {
                        let children: Vec<String> = self
                            .stack
                            .children(branch)
                            .into_iter()
                            .filter(|child| !confirmed_deletions.contains(child))
                            .collect();
                        if !children.is_empty() {
                            if !self.quiet {
                                println!(
                                    "    {} Branch '{}' was squash-merged into {}. Rebasing {} child(ren) onto {}...",
                                    "⚑".yellow(),
                                    branch.yellow(),
                                    self.stack.trunk,
                                    children.len(),
                                    self.stack.trunk
                                );
                            }

                            let mut rebased_children: Vec<String> = Vec::new();
                            for child in &children {
                                if BranchMetadata::is_frozen(repo.inner(), child)? {
                                    if !self.quiet {
                                        println!(
                                            "      {} Skipped rebase for frozen child {}; its parent metadata will still move to {}",
                                            "❄".cyan(),
                                            child.cyan(),
                                            self.stack.trunk.cyan()
                                        );
                                    }
                                    continue;
                                }

                                // Snapshot child head+metadata before rebase so undo can restore.
                                if let Some(ref mut tx) = self.tx {
                                    tx.snapshot_branch_with_metadata(&repo, child)?;
                                }

                                // Tip of the merged parent BEFORE deletion — the boundary
                                // fallback when the child did not actually move onto trunk.
                                let merged_parent_tip = repo.branch_commit(branch).ok();

                                // Use existing provenance-aware rebase
                                match repo.rebase_branch_onto_with_provenance(
                                    child,
                                    &self.stack.trunk,
                                    branch, // fallback upstream
                                    false,  // auto_stash_pop
                                ) {
                                    Ok(RebaseResult::Success) => {
                                        // Update child's parent metadata to trunk
                                        let trunk_tip = repo.rev_parse(&self.stack.trunk)?;
                                        if let Some(mut metadata) =
                                            BranchMetadata::read(repo.inner(), child)?
                                        {
                                            metadata.parent_branch_name = self.stack.trunk.clone();
                                            metadata.parent_branch_revision = repo
                                                .resolve_child_parent_boundary(
                                                    child,
                                                    &[
                                                        Some(trunk_tip.as_str()),
                                                        merged_parent_tip.as_deref(),
                                                    ],
                                                    &metadata.parent_branch_revision,
                                                );
                                            metadata.write(repo.inner(), child)?;
                                        }

                                        // Record after-OIDs so redo can replay the rebase.
                                        if let Some(ref mut tx) = self.tx {
                                            let child_head_after = resolve_ref_oid(
                                                &self.workdir,
                                                &format!("refs/heads/{}", child),
                                            );
                                            tx.record_known_after(
                                                child,
                                                child_head_after.as_deref(),
                                            );
                                            let child_meta_after = resolve_ref_oid(
                                                &self.workdir,
                                                &format!("refs/branch-metadata/{}", child),
                                            );
                                            tx.record_known_metadata_after(
                                                child,
                                                child_meta_after.as_deref(),
                                            );
                                        }

                                        if !self.quiet {
                                            println!(
                                                "      {} Rebased {} onto {}",
                                                "✓".green(),
                                                child.cyan(),
                                                self.stack.trunk.cyan()
                                            );
                                        }

                                        rebased_children.push(child.clone());
                                    }
                                    Ok(RebaseResult::Conflict) => {
                                        let trunk_name = self.stack.trunk.clone();
                                        let conflict_stack = self.stack.current_stack(child);
                                        if !self.json {
                                            print_restack_conflict(
                                                &repo,
                                                &RestackConflictContext {
                                                    branch: child,
                                                    parent_branch: &trunk_name,
                                                    completed_branches: &rebased_children,
                                                    remaining_branches: 0,
                                                    continue_commands: &[
                                                        "stax resolve",
                                                        "stax continue",
                                                        "stax sync --continue",
                                                    ],
                                                    stack_branches: &conflict_stack,
                                                },
                                            );
                                        }
                                        if self.stashed && !self.json {
                                            println!(
                                                "{}",
                                                "Stash kept to avoid conflicts. Run `git stash pop` after resolving."
                                                    .yellow()
                                            );
                                            self.stash_guard.disarm();
                                        }
                                        if let Some(tx) = self.tx.take() {
                                            tx.finish_err(
                                                "Rebase conflict",
                                                Some("cleanup-merged"),
                                                Some(child),
                                            )?;
                                        }
                                        return Err(ConflictStopped.into());
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "      {} Failed to rebase {}: {}",
                                            "✗".red(),
                                            child.yellow(),
                                            e
                                        );
                                        eprintln!(
                                            "      Stopping sync. Resolve conflicts and run `stax continue`."
                                        );
                                        return Err(e);
                                    }
                                }
                            }
                        }
                    }

                    // If we're on this branch, checkout parent first
                    if is_current_branch {
                        match checkout_branch_for_cleanup(&repo, &self.workdir, &parent_branch) {
                            Ok(()) => {
                                if !self.quiet {
                                    println!(
                                        "    {} checked out {}",
                                        "→".cyan(),
                                        parent_branch.cyan()
                                    );
                                }

                                // Pull latest changes for the parent branch
                                let pull_status = Command::new("git")
                                    .args(["pull", "--ff-only", &self.remote_name, &parent_branch])
                                    .current_dir(&self.workdir)
                                    .stdout(std::process::Stdio::null())
                                    .stderr(std::process::Stdio::null())
                                    .status();

                                if let Ok(status) = pull_status
                                    && status.success()
                                    && !self.quiet
                                {
                                    println!(
                                        "    {} pulled latest {}",
                                        "↓".cyan(),
                                        parent_branch.cyan()
                                    );
                                }
                            }
                            Err(checkout_error) => {
                                self.record_checkout_failure_skip(
                                    branch,
                                    &parent_branch,
                                    checkout_error,
                                );
                                continue;
                            }
                        }
                    }

                    // Snapshot the deleted branch + reparented children BEFORE the
                    // reparent so we capture the original metadata for undo.
                    let reparented_for_tx =
                        children_to_reparent(&self.stack, branch, &confirmed_deletions);
                    self.snapshot_deletion(&repo, branch, &reparented_for_tx)?;

                    // Reparent tracked children onto the surviving parent before
                    // deleting, preserving the old-parent boundary for later restack.
                    reparent_children_for_deletion(
                        &repo,
                        &self.stack,
                        branch,
                        &parent_branch,
                        &confirmed_deletions,
                        forge_client.as_ref(),
                        self.quiet,
                    )?;

                    let tip_before_delete = repo.branch_commit(branch).ok();
                    let local_delete = self.delete_local_branch(
                        &repo,
                        branch,
                        action,
                        blocking_worktree_cleanup.as_ref(),
                    )?;
                    let local_deleted = local_delete.deleted;
                    let local_worktree_blocked = local_delete.worktree_blocked;

                    if !local_deleted && local_branch_exists(&self.workdir, branch) {
                        self.record_local_branch_kept_skip(
                            branch,
                            blocking_worktree_cleanup.as_ref(),
                        );
                        if !self.quiet {
                            print_blocked_or_skipped(
                                branch,
                                blocking_worktree_cleanup.as_ref(),
                                local_worktree_blocked,
                            );
                            print_metadata_kept_note();
                        }
                        continue;
                    }

                    // Skip the push-delete when the remote branch is already gone
                    // (e.g. the forge auto-deleted it on merge). Each `git push
                    // --delete` is a network round-trip, so probing a whole merged
                    // stack that is already gone remotely wastes seconds per branch.
                    let remote_branch_present = self
                        .remote_branches_for_merged
                        .as_ref()
                        .is_none_or(|remotes| remotes.contains(branch));

                    // Imported branches are read-only remote references. Clean them
                    // up locally after merge, but never push-delete someone else's
                    // remote branch.
                    let will_push_delete =
                        !self.remote_delete_exempt_imported_branches.contains(branch)
                            && remote_branch_present;
                    if will_push_delete {
                        let remote_name = self.remote_name.clone();
                        if let Some(ref mut tx) = self.tx {
                            tx.plan_remote_branch(&repo, &remote_name, branch)?;
                        }
                    }
                    let remote_deleted = if will_push_delete {
                        let remote_status = Command::new("git")
                            .args(["push", &self.remote_name, "--delete", branch])
                            .current_dir(&self.workdir)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();

                        remote_status.map(|s| s.success()).unwrap_or(false)
                    } else {
                        false
                    };

                    // Only delete metadata if branch no longer exists locally.
                    let local_still_exists = local_branch_exists(&self.workdir, branch);

                    let metadata_deleted =
                        self.delete_branch_metadata(&repo, branch, local_still_exists);

                    self.record_deletion_after(branch, local_still_exists, &reparented_for_tx);

                    if metadata_deleted {
                        self.stats.merged_branches_cleaned += 1;
                    }

                    // Record JSON stat for deleted branch (outside !quiet gate)
                    if local_deleted || remote_deleted {
                        let scope = match (local_deleted, remote_deleted) {
                            (true, true) => "both",
                            (true, false) => "local",
                            (false, true) => "remote",
                            _ => "local",
                        };
                        self.stats.deleted_branches.push(DeletedBranchRecord {
                            branch: branch.clone(),
                            category: "merged",
                            scope,
                            tip: tip_before_delete.clone(),
                            metadata_deleted,
                        });
                    }

                    if !local_deleted && local_still_exists {
                        self.record_local_branch_kept_skip(
                            branch,
                            blocking_worktree_cleanup.as_ref(),
                        );
                    }

                    if !self.quiet {
                        if local_deleted && remote_deleted {
                            println!(
                                "    {} {}{}",
                                branch.bright_black(),
                                "deleted (local + remote)".green(),
                                deleted_tip_suffix(tip_before_delete.as_deref())
                            );
                        } else if local_deleted {
                            println!(
                                "    {} {}{}",
                                branch.bright_black(),
                                "deleted (local only)".green(),
                                deleted_tip_suffix(tip_before_delete.as_deref())
                            );
                        } else if remote_deleted {
                            println!(
                                "    {} {}",
                                branch.bright_black(),
                                "deleted (remote only)".green()
                            );
                            if !metadata_deleted {
                                println!(
                                    "    {} {}",
                                    "↷".yellow(),
                                    "local branch still exists, metadata kept".dimmed()
                                );
                            }
                        } else {
                            print_blocked_or_skipped(
                                branch,
                                blocking_worktree_cleanup.as_ref(),
                                local_worktree_blocked,
                            );
                            if !metadata_deleted {
                                print_metadata_kept_note();
                            }
                        }
                    }
                }
            } else if !self.quiet {
                println!("    {}", "No merged branches to delete.".dimmed());
            }

            // Record partially-merged notes OUTSIDE !quiet gate for JSON stats
            for note in &partially_merged_notes {
                let reason = match note.pr_label {
                    PartialMergeReason::PrMerged => "pr_merged",
                    PartialMergeReason::PrClosed => "pr_closed",
                    PartialMergeReason::HistoryMerged => "history_merged",
                };
                self.stats.partially_merged.push(PartialMergeRecord {
                    branch: note.branch.clone(),
                    reason,
                    pr_number: note.pr_number,
                    extra_commits: note.extra_commits,
                });
            }

            if !self.quiet {
                for note in &partially_merged_notes {
                    let commit_word = if note.extra_commits == 1 {
                        "commit"
                    } else {
                        "commits"
                    };
                    let signal = match (note.pr_label, note.pr_number) {
                        (PartialMergeReason::PrMerged, Some(n)) => format!("PR #{} merged", n),
                        (PartialMergeReason::PrMerged, None) => "PR merged".to_string(),
                        (PartialMergeReason::PrClosed, Some(n)) => format!("PR #{} closed", n),
                        (PartialMergeReason::PrClosed, None) => "PR closed".to_string(),
                        (PartialMergeReason::HistoryMerged, _) => {
                            "earlier commits already merged into trunk".to_string()
                        }
                    };
                    let detail = format!(
                        "{}, but branch has {} additional {} not on trunk or any remote — not deleting",
                        signal, note.extra_commits, commit_word
                    );
                    println!("    {} {}: {}", "⚠".yellow(), note.branch, detail.dimmed());
                }
            }

            let delete_elapsed = delete_merged_started_at.elapsed();
            self.step_timings
                .push(("delete merged branches".to_string(), delete_elapsed));
            if !self.quiet && !merged.is_empty() {
                println!(
                    "  {:<35} {}",
                    "delete merged branches",
                    format!("{:.3}s", delete_elapsed.as_secs_f64()).dimmed()
                );
            }
            Ok(repo)
        } else {
            Ok(repo)
        }
    }

    fn cleanup_upstream_gone(&mut self, repo: &GitRepo) -> Result<()> {
        if self.delete_upstream_gone {
            let detect_gone_started_at = Instant::now();
            let detect_timer = LiveTimer::maybe_new(!self.quiet, "Detect upstream-gone branches");
            let detected_gone = find_upstream_gone_branches(&self.workdir, &self.stack.trunk)?;

            // Protect upstream-gone branches that still carry local-only work
            // (commits unique relative to BOTH local trunk and origin/<trunk>).
            // A branch's remote upstream disappearing does not mean its commits
            // were integrated — users routinely add local commits after the last
            // push — so deleting such a branch would lose un-pushed work. This
            // mirrors `stax sweep`, which already classifies these branches as
            // active rather than deletable (see commands/sweep.rs).
            let mut gone: Vec<String> = Vec::with_capacity(detected_gone.len());
            let mut protected_gone: Vec<String> = Vec::new();
            for branch in detected_gone {
                if has_unique_commits_since_any_base(
                    &self.workdir,
                    &branch,
                    &[self.stack.trunk.as_str(), self.remote_trunk_ref.as_str()],
                )? {
                    protected_gone.push(branch);
                } else {
                    gone.push(branch);
                }
            }

            self.step_timings.push((
                "detect upstream-gone branches".to_string(),
                detect_gone_started_at.elapsed(),
            ));
            LiveTimer::maybe_finish_timed(detect_timer);

            // Record protected-gone branches OUTSIDE !quiet gate for JSON stats
            self.stats
                .protected_branches
                .extend(protected_gone.iter().cloned());

            if !self.quiet && !protected_gone.is_empty() {
                let branch_word = if protected_gone.len() == 1 {
                    "branch"
                } else {
                    "branches"
                };
                println!(
                    "    Protected {} upstream-gone {} with local-only commits:",
                    protected_gone.len().to_string().cyan(),
                    branch_word
                );
                for branch in &protected_gone {
                    println!(
                        "      {} {} {}",
                        "▸".bright_black(),
                        branch,
                        "(has unpushed work)".dimmed()
                    );
                }
                println!();
            }

            let delete_gone_started_at = Instant::now();

            if !gone.is_empty() {
                if !self.quiet {
                    print_cleanup_candidates("upstream-gone", &gone);
                }

                // Reload the stack so the merged-branch path's reparenting is
                // reflected before we resolve upstream-gone branches. Fall back to
                // the snapshot captured at the top of sync() if the reload fails,
                // to degrade gracefully rather than aborting a mid-sync run.
                let mut live_stack = match Stack::load(repo) {
                    Ok(s) => s,
                    Err(ref e) => {
                        self.warn_stale_stack_fallback(e);
                        self.stack.clone()
                    }
                };

                // Initialize forge client once up-front for any PR base updates below.
                let forge_client = init_forge_client(repo, &self.config);

                let gone_deletions: HashSet<String> = gone.iter().cloned().collect();
                for branch in &gone {
                    if !local_branch_exists(&self.workdir, branch) {
                        continue;
                    }

                    let is_current_branch = branch == &self.current_after_deletions;
                    let blocking_worktree_cleanup =
                        self.plan_delete_blocker(repo, branch, is_current_branch)?;

                    // Resolve the parent children will be reparented to. Walks up
                    // the recorded-parent chain skipping any branch that is itself
                    // scheduled for deletion in this pass, so a stack like A -> B -> C
                    // (where A and B both have upstream gone) lands C on trunk
                    // rather than on the soon-to-be-deleted A.
                    let (fallback_parent, parent_fallback_from) =
                        resolve_fallback_parent_skipping_doomed(
                            &self.workdir,
                            &live_stack,
                            branch,
                            &gone,
                        );

                    // Print the parent-fallback hint BEFORE the confirm prompt so the
                    // user knows why the prompt mentions a non-recorded parent.
                    if !self.quiet
                        && let Some(missing_parent) = &parent_fallback_from
                    {
                        println!(
                            "    {} parent {} not available; using {}",
                            "↪".yellow(),
                            missing_parent.yellow(),
                            fallback_parent.cyan()
                        );
                    }

                    let prompt = sync_delete_prompt(
                        branch,
                        if is_current_branch {
                            Some(fallback_parent.as_str())
                        } else {
                            None
                        },
                        Some("upstream gone"),
                        blocking_worktree_cleanup.as_ref(),
                    );

                    let action = self.decide_delete_action(
                        branch,
                        prompt,
                        blocking_worktree_cleanup.as_ref(),
                    )?;

                    if action == SyncBranchDeleteAction::Skip {
                        self.record_unconfirmed_skip(branch);
                        continue;
                    }

                    if action == SyncBranchDeleteAction::PreserveWorktree
                        && !self.preserve_blocking_worktree(
                            repo,
                            branch,
                            blocking_worktree_cleanup.as_ref(),
                        )
                    {
                        continue;
                    }

                    if is_current_branch {
                        match checkout_branch_for_cleanup(repo, &self.workdir, &fallback_parent) {
                            Ok(()) => {
                                self.current_after_deletions = fallback_parent.clone();
                                if !self.quiet {
                                    println!(
                                        "    {} checked out {}",
                                        "→".cyan(),
                                        fallback_parent.cyan()
                                    );
                                }
                            }
                            Err(checkout_error) => {
                                self.record_checkout_failure_skip(
                                    branch,
                                    &fallback_parent,
                                    checkout_error,
                                );
                                continue;
                            }
                        }
                    }

                    // Snapshot the deleted branch + reparented children BEFORE the
                    // reparent so we capture the original metadata for undo.
                    let gone_reparented =
                        children_to_reparent(&live_stack, branch, &gone_deletions);
                    self.snapshot_deletion(repo, branch, &gone_reparented)?;

                    // Reparent tracked children to the fallback parent before
                    // deleting. The shared helper also mirrors the merged-branch
                    // path's ancestor-check rationale from issue #120.
                    reparent_children_for_deletion(
                        repo,
                        &live_stack,
                        branch,
                        &fallback_parent,
                        &gone_deletions,
                        forge_client.as_ref(),
                        self.quiet,
                    )?;

                    // Refresh the in-memory stack so subsequent iterations see the
                    // just-reparented children under the new parent (preventing a
                    // later iteration from bouncing them again). Fall back to the
                    // current live_stack if the reload fails.
                    match Stack::load(repo) {
                        Ok(refreshed) => live_stack = refreshed,
                        Err(ref e) => self.warn_stale_stack_fallback(e),
                    }

                    let tip_before_delete = repo.branch_commit(branch).ok();
                    let local_delete = self.delete_local_branch(
                        repo,
                        branch,
                        action,
                        blocking_worktree_cleanup.as_ref(),
                    )?;
                    let local_deleted = local_delete.deleted;
                    let local_worktree_blocked = local_delete.worktree_blocked;

                    // Only delete metadata if branch no longer exists locally.
                    let local_still_exists = local_branch_exists(&self.workdir, branch);

                    let metadata_deleted =
                        self.delete_branch_metadata(repo, branch, local_still_exists);

                    self.record_deletion_after(branch, local_still_exists, &gone_reparented);

                    // Record JSON stat for upstream-gone deleted branch (outside !quiet gate)
                    if local_deleted {
                        self.stats.deleted_branches.push(DeletedBranchRecord {
                            branch: branch.clone(),
                            category: "upstream_gone",
                            scope: "local",
                            tip: tip_before_delete.clone(),
                            metadata_deleted,
                        });
                    }

                    if !local_deleted && local_still_exists {
                        self.record_local_branch_kept_skip(
                            branch,
                            blocking_worktree_cleanup.as_ref(),
                        );
                    }

                    if !self.quiet {
                        if local_deleted {
                            println!(
                                "    {} {}{}",
                                branch.bright_black(),
                                "deleted (local only)".green(),
                                deleted_tip_suffix(tip_before_delete.as_deref())
                            );
                        } else {
                            print_blocked_or_skipped(
                                branch,
                                blocking_worktree_cleanup.as_ref(),
                                local_worktree_blocked,
                            );
                        }

                        if !metadata_deleted && local_still_exists {
                            print_metadata_kept_note();
                        }
                    }
                }
            } else if !self.quiet {
                println!("    {}", "No upstream-gone branches to delete.".dimmed());
            }

            let delete_elapsed = delete_gone_started_at.elapsed();
            self.step_timings
                .push(("delete upstream-gone branches".to_string(), delete_elapsed));
            if !self.quiet && !gone.is_empty() {
                println!(
                    "  {:<35} {}",
                    "delete upstream-gone branches",
                    format!("{:.3}s", delete_elapsed.as_secs_f64()).dimmed()
                );
            }
        }
        Ok(())
    }

    // If we deferred trunk update (refspec fetch failed while not on trunk) and we're
    // now on trunk after branch deletions, retry with git pull which is more reliable
    fn retry_deferred_trunk_update(&mut self, repo: &GitRepo) -> Result<()> {
        if self.trunk_update_deferred && self.current_after_deletions == self.stack.trunk {
            let deferred_update_started_at = Instant::now();
            let deferred_timer =
                LiveTimer::maybe_new(!self.quiet, &format!("Update {}", self.stack.trunk));

            let workdir = self.workdir.clone();
            self.fast_forward_trunk_in(repo, &workdir, deferred_timer, false)?;

            self.step_timings.push((
                format!("retry update {}", self.stack.trunk),
                deferred_update_started_at.elapsed(),
            ));
        }
        Ok(())
    }

    fn restack_phase(&mut self, repo: &GitRepo) -> Result<()> {
        if self.restack {
            let restack_started_at = Instant::now();
            if !self.quiet {
                println!();
                println!("{}", "Restacking...".bold());
            }

            // Scope restacking to the stack we started on, even if sync switched branches
            // (for example, if the current branch was deleted after merge).
            let scope_order: Vec<String> = if self.current != self.stack.trunk
                && self.stack.branches.contains_key(&self.current)
            {
                self.stack.current_stack(&self.current)
            } else {
                Vec::new()
            };
            let mut frozen_branches = Vec::new();
            let restack_scope_order = scope_order
                .iter()
                .filter(|branch| {
                    let frozen = BranchMetadata::is_frozen(repo.inner(), branch).unwrap_or(false);
                    if frozen {
                        frozen_branches.push((*branch).clone());
                    }
                    !frozen
                })
                .cloned()
                .collect::<Vec<_>>();
            if !frozen_branches.is_empty() && !self.quiet {
                println!(
                    "  {} Skipping frozen {}: {}",
                    "▸".dimmed(),
                    if frozen_branches.len() == 1 {
                        "branch"
                    } else {
                        "branches"
                    },
                    frozen_branches.join(", ").cyan()
                );
            }
            // Load stack once; orphaned parents are handled in Stack::load (parent → trunk,
            // needs_restack). Keep the stack in memory and update it after each rebase.
            let mut live_stack = Stack::load(repo)?;
            for branch in &self.updated_imported_branches {
                if let Ok(parent_rev) = repo.branch_commit(branch) {
                    let children = live_stack
                        .branches
                        .get(branch.as_str())
                        .map(|br| br.children.clone())
                        .unwrap_or_default();
                    for child in &children {
                        if let Some(child_br) = live_stack.branches.get_mut(child) {
                            child_br.needs_restack = child_br
                                .parent_revision
                                .as_deref()
                                .map(|rev| rev != parent_rev.as_str())
                                .unwrap_or(true);
                        }
                    }
                }
            }
            let branches_to_restack: Vec<String> = restack_scope_order
                .iter()
                .filter(|branch| {
                    live_stack
                        .branches
                        .get(branch.as_str())
                        .map(|br| br.needs_restack)
                        .unwrap_or(false)
                })
                .cloned()
                .collect();

            if branches_to_restack.is_empty() {
                if !self.quiet {
                    println!(
                        "  {}",
                        if frozen_branches.is_empty() {
                            "All branches up to date."
                        } else {
                            "No unfrozen branches need restacking."
                        }
                        .dimmed()
                    );
                }
            } else {
                // Take the sync-wide transaction for the restack phase.  Using take()
                // avoids a borrow conflict: &mut self.tx cannot coexist with the
                // &mut self that all subsequent method calls on self need.
                let mut tx = self
                    .tx
                    .take()
                    .context("sync transaction was already finished")?;
                for branch in &restack_scope_order {
                    tx.plan_branch(repo, branch)?;
                    if branch != &self.stack.trunk {
                        tx.plan_metadata_ref(repo, branch)?;
                    }
                }
                let restack_count = branches_to_restack.len();
                let summary = PlanSummary {
                    branches_to_rebase: restack_count,
                    branches_to_push: 0,
                    description: vec![format!(
                        "Sync restack {} {}",
                        restack_count,
                        if restack_count == 1 {
                            "branch"
                        } else {
                            "branches"
                        }
                    )],
                };
                tx::print_plan(tx.kind(), &summary, self.quiet);
                tx.set_plan_summary(summary);
                tx.set_auto_stash_pop(self.auto_stash_pop);
                tx.snapshot()?;

                let mut summary: Vec<(String, String)> = Vec::new();
                let mut restacked_branches: Vec<String> = Vec::new();

                for (index, branch) in restack_scope_order.iter().enumerate() {
                    let needs_restack = live_stack
                        .branches
                        .get(branch.as_str())
                        .map(|br| br.needs_restack)
                        .unwrap_or(false);
                    if !needs_restack {
                        continue;
                    }

                    let (parent_branch_name, parent_branch_revision) =
                        match live_stack.branches.get(branch.as_str()) {
                            Some(br) if br.parent.is_some() && br.parent_revision.is_some() => (
                                br.parent.clone().unwrap(),
                                br.parent_revision.clone().unwrap(),
                            ),
                            _ => match BranchMetadata::read(repo.inner(), branch)? {
                                Some(m) => (m.parent_branch_name, m.parent_branch_revision),
                                None => continue,
                            },
                        };

                    let restack_timer =
                        LiveTimer::maybe_new(!self.quiet, &format!("Restack {}", branch));

                    let rebase_upstream = crate::engine::restack_preflight::choose_rebase_upstream(
                        repo,
                        &self.config,
                        branch,
                        &parent_branch_name,
                        &parent_branch_revision,
                        self.quiet,
                    );

                    let rebase = repo.rebase_branch_onto_with_provenance_timing(
                        branch,
                        &parent_branch_name,
                        &rebase_upstream,
                        self.auto_stash_pop,
                        true,
                    )?;

                    match rebase.result {
                        RebaseResult::Success => {
                            let metadata_update_started_at = Instant::now();
                            let new_parent_rev = repo.branch_commit(&parent_branch_name)?;
                            let existing_metadata = BranchMetadata::read(repo.inner(), branch)?;
                            let source_remote = existing_metadata
                                .as_ref()
                                .and_then(|meta| meta.source_remote.clone());
                            let frozen = existing_metadata.is_some_and(|meta| meta.frozen);
                            let updated_meta = BranchMetadata {
                                parent_branch_name: parent_branch_name.clone(),
                                parent_branch_revision: new_parent_rev.clone(),
                                source_remote,
                                frozen,
                                pr_info: live_stack.branches.get(branch.as_str()).and_then(|br| {
                                    br.pr_number.map(|n| PrInfo {
                                        number: n,
                                        state: br.pr_state.clone().unwrap_or_default(),
                                        is_draft: br.pr_is_draft,
                                    })
                                }),
                            };
                            updated_meta.write(repo.inner(), branch)?;

                            if let Some(br) = live_stack.branches.get_mut(branch.as_str()) {
                                br.needs_restack = false;
                                br.parent_revision = Some(new_parent_rev.clone());
                            }
                            let children: Vec<String> = live_stack
                                .branches
                                .get(branch.as_str())
                                .map(|br| br.children.clone())
                                .unwrap_or_default();
                            for child in &children {
                                if let Some(child_br) = live_stack.branches.get_mut(child) {
                                    child_br.needs_restack = child_br
                                        .parent_revision
                                        .as_deref()
                                        .map(|rev| rev != new_parent_rev.as_str())
                                        .unwrap_or(true);
                                }
                            }

                            let metadata_update = metadata_update_started_at.elapsed();

                            // Record after-OID
                            tx.record_after(repo, branch)?;
                            let meta_after = resolve_ref_oid(
                                &self.workdir,
                                &format!("refs/branch-metadata/{}", branch),
                            );
                            tx.record_known_metadata_after(branch, meta_after.as_deref());
                            tx.push_completed_branch(branch);

                            if self.verbose {
                                self.restack_branch_timings.push(RestackBranchTiming {
                                    branch: branch.clone(),
                                    rebase_timings: rebase.timings,
                                    metadata_update,
                                });
                            }

                            LiveTimer::maybe_finish_timed(restack_timer);
                            restacked_branches.push(branch.clone());
                            summary.push((branch.clone(), "ok".to_string()));
                        }
                        RebaseResult::Conflict => {
                            if self.verbose {
                                self.restack_branch_timings.push(RestackBranchTiming {
                                    branch: branch.clone(),
                                    rebase_timings: rebase.timings,
                                    metadata_update: Duration::ZERO,
                                });
                            }

                            LiveTimer::maybe_finish_warn(restack_timer, "conflict");
                            let completed_branches: Vec<String> = summary
                                .iter()
                                .filter(|(_, status)| status == "ok")
                                .map(|(name, _)| name.clone())
                                .collect();
                            let conflict_stack = live_stack.current_stack(branch);
                            if !self.json {
                                print_restack_conflict(
                                    repo,
                                    &RestackConflictContext {
                                        branch,
                                        parent_branch: &parent_branch_name,
                                        completed_branches: &completed_branches,
                                        remaining_branches: scope_order
                                            .len()
                                            .saturating_sub(index + 1),
                                        continue_commands: &[
                                            "stax resolve",
                                            "stax continue",
                                            "stax sync --continue",
                                        ],
                                        stack_branches: &conflict_stack,
                                    },
                                );
                            }
                            if self.stashed && !self.json {
                                println!(
                                    "{}",
                                    "Stash kept to avoid conflicts. Run `git stash pop` after resolving.".yellow()
                                );
                                self.stash_guard.disarm();
                            }
                            summary.push((branch.clone(), "conflict".to_string()));

                            // Finish transaction with error
                            tx.finish_err("Rebase conflict", Some("restack"), Some(branch))?;

                            return Err(ConflictStopped.into());
                        }
                    }
                }

                repo.checkout(&self.current_after_deletions)?;

                // Return the transaction to SyncContext so finish_transaction can close it.
                self.tx = Some(tx);
                self.stats.restacked_branches = restacked_branches;

                if !self.quiet && !summary.is_empty() {
                    println!();
                    println!("{}", "Restack summary:".dimmed());
                    for (branch, status) in &summary {
                        let symbol = if status == "ok" { "✓" } else { "✗" };
                        println!("  {} {} {}", symbol, branch, status);
                    }
                }
            }

            self.step_timings
                .push(("restack".to_string(), restack_started_at.elapsed()));
        }
        Ok(())
    }

    fn restore_stash(&mut self, repo: &GitRepo) -> Result<()> {
        if self.stashed {
            let stash_pop_started_at = Instant::now();
            repo.stash_pop()?;
            self.stash_guard.disarm();
            self.stash_restored = true;
            self.step_timings
                .push(("restore stash".to_string(), stash_pop_started_at.elapsed()));
            if !self.quiet {
                println!("{}", "✓ Restored stashed changes.".green());
            }
        }
        Ok(())
    }

    /// Snapshot a branch (head + metadata) that is about to be deleted, together
    /// with the metadata of each child that will be reparented.
    ///
    /// Must be called BEFORE the reparent so the original metadata is captured.
    fn snapshot_deletion(
        &mut self,
        repo: &GitRepo,
        branch: &str,
        reparented: &[String],
    ) -> Result<()> {
        if let Some(ref mut tx) = self.tx {
            tx.plan_branch(repo, branch)?;
            tx.plan_metadata_ref(repo, branch)?;
            for child in reparented {
                let child = child.clone();
                tx.plan_branch(repo, &child)?;
                tx.plan_metadata_ref(repo, &child)?;
            }
            tx.snapshot()?;
        }
        Ok(())
    }

    /// Record the after-states for a deleted branch and its reparented children.
    ///
    /// After-OIDs are resolved via `resolve_ref_oid` (git subprocess) so that
    /// libgit2's cached refdb — which may not see subprocess-written refs — is
    /// never used for absent refs.
    fn record_deletion_after(
        &mut self,
        branch: &str,
        local_still_exists: bool,
        reparented: &[String],
    ) {
        if let Some(ref mut tx) = self.tx {
            // Branch head: None when deleted, existing OID when still present.
            let branch_after = if local_still_exists {
                resolve_ref_oid(&self.workdir, &format!("refs/heads/{}", branch))
            } else {
                None
            };
            tx.record_known_after(branch, branch_after.as_deref());

            // Metadata ref: same absence logic.
            let meta_after = if local_still_exists {
                resolve_ref_oid(&self.workdir, &format!("refs/branch-metadata/{}", branch))
            } else {
                None
            };
            tx.record_known_metadata_after(branch, meta_after.as_deref());

            // Reparented children — their head did not move but their metadata did.
            for child in reparented {
                let head_after = resolve_ref_oid(&self.workdir, &format!("refs/heads/{}", child));
                tx.record_known_after(child, head_after.as_deref());
                let child_meta_after =
                    resolve_ref_oid(&self.workdir, &format!("refs/branch-metadata/{}", child));
                tx.record_known_metadata_after(child, child_meta_after.as_deref());
            }
        }
    }

    fn warn_stale_stack_fallback(&mut self, error: &anyhow::Error) {
        if self.quiet || self.stale_stack_warning_shown {
            return;
        }
        self.stale_stack_warning_shown = true;
        eprintln!(
            "{}",
            stale_stack_warning("using snapshot from sync start", error)
        );
    }

    fn finalize(
        &mut self,
        trunk_stats_worker: std::thread::JoinHandle<Result<Option<TrunkSummary>>>,
    ) -> Result<()> {
        let trunk_summary = wait_for_trunk_summary(trunk_stats_worker)?;
        let trunk_reached_remote = trunk_reached_remote(
            &self.workdir,
            &self.stack.trunk,
            self.remote_trunk_after_fetch.as_deref(),
        );
        self.stats.trunk = trunk_reached_remote.then_some(trunk_summary).flatten();

        if !trunk_reached_remote {
            self.stats.trunk_not_updated = Some(TrunkNotUpdated {
                branch: self.stack.trunk.clone(),
                remote_ref: self.remote_trunk_ref.clone(),
                failure: if self
                    .remote_trunk_after_fetch
                    .as_deref()
                    .is_some_and(|remote| !is_ancestor(&self.workdir, &self.stack.trunk, remote))
                {
                    TrunkUpdateFailure::Diverged
                } else {
                    TrunkUpdateFailure::Other
                },
            });
        }

        if self.current_after_deletions != self.current {
            self.stats.checkout_change = Some(CheckoutChange {
                from: self.current.clone(),
                to: self.current_after_deletions.clone(),
            });
        }

        self.stats
            .cleanup_skips
            .sort_by(|a, b| a.branch.cmp(&b.branch));

        if self.verbose && !self.quiet {
            println!();
            println!("{}", "Sync timing summary:".bold());
            for (step, duration) in &self.step_timings {
                println!("  {:<35} {}", step, format_duration(*duration).dimmed());
            }
            print_restack_branch_timings(&self.restack_branch_timings);
            println!(
                "  {:<35} {}",
                "total",
                format_duration(self.sync_started_at.elapsed()).cyan()
            );
        }

        if !self.quiet {
            println!();
            println!(
                "{} {}",
                "Sync complete!".green().bold(),
                render_sync_footer(&self.stats, self.sync_started_at.elapsed())
            );

            let follow_up = render_sync_follow_up(&self.stats);
            if !follow_up.is_empty() {
                println!();
                for line in follow_up {
                    if self.config.ui.tips || !line.starts_with("Next:") {
                        println!("{}", line);
                    }
                }
            }
        }

        Ok(())
    }
}

/// Sync repo: pull trunk from remote, delete merged branches, optionally restack
#[allow(clippy::too_many_arguments)]
pub fn run(
    restack: bool,
    full: bool,
    delete_merged: bool,
    delete_upstream_gone: bool,
    force: bool,
    safe: bool,
    r#continue: bool,
    quiet: bool,
    verbose: bool,
    auto_stash_pop: bool,
    stash_policy: StashPolicy,
    json: bool,
    extra_fetch_refs: &[String],
    skip_interactive_plan: bool,
) -> Result<()> {
    run_with_repo(
        GitRepo::open()?,
        Config::load()?,
        force,
        restack,
        full,
        delete_merged,
        delete_upstream_gone,
        force,
        safe,
        r#continue,
        quiet,
        verbose,
        auto_stash_pop,
        stash_policy,
        json,
        extra_fetch_refs,
        skip_interactive_plan,
    )
}

/// Run sync against an explicit repository instead of the process working
/// directory. This is the web-server boundary; CLI callers keep using `run`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_at(
    repository: &Path,
    auto_confirm: bool,
    restack: bool,
    full: bool,
    delete_merged: bool,
    delete_upstream_gone: bool,
    force: bool,
    safe: bool,
    r#continue: bool,
    quiet: bool,
    verbose: bool,
    auto_stash_pop: bool,
    stash_policy: StashPolicy,
    json: bool,
    extra_fetch_refs: &[String],
    skip_interactive_plan: bool,
) -> Result<()> {
    let repo = GitRepo::open_from_path(repository)?;
    for worktree in repo.list_worktrees()? {
        if repo.is_dirty_at(&worktree.path)? {
            bail!(
                "Working tree '{}' has uncommitted changes; Sync made no changes",
                worktree.path.display()
            );
        }
    }
    run_with_repo(
        repo,
        Config::load_for_trusted_network(repository)?,
        auto_confirm,
        restack,
        full,
        delete_merged,
        delete_upstream_gone,
        force,
        safe,
        r#continue,
        quiet,
        verbose,
        auto_stash_pop,
        stash_policy,
        json,
        extra_fetch_refs,
        skip_interactive_plan,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_with_repo(
    repo: GitRepo,
    config: Config,
    auto_confirm: bool,
    restack: bool,
    full: bool,
    delete_merged: bool,
    delete_upstream_gone: bool,
    force: bool,
    safe: bool,
    r#continue: bool,
    quiet: bool,
    verbose: bool,
    auto_stash_pop: bool,
    stash_policy: StashPolicy,
    json: bool,
    extra_fetch_refs: &[String],
    skip_interactive_plan: bool,
) -> Result<()> {
    let sync_started_at = Instant::now();
    let (mut ctx, repo) = SyncContext::new(
        repo,
        config,
        auto_confirm,
        sync_started_at,
        restack,
        full,
        delete_merged,
        delete_upstream_gone,
        force,
        safe,
        quiet,
        verbose,
        auto_stash_pop,
        stash_policy,
        json,
        extra_fetch_refs,
        skip_interactive_plan,
    )?;

    if r#continue {
        crate::commands::continue_cmd::run()?;
        if repo.rebase_in_progress()? {
            return Ok(());
        }
    }

    if json {
        if verbose {
            eprintln!(
                "{}",
                "warning: --verbose is ignored by --json (output is machine-readable)".yellow()
            );
        }
        let phase_result = match ctx.handle_dirty_tree(&repo) {
            Err(e) => Err(e),
            Ok(SyncFlow::Stop) => Ok(()),
            Ok(SyncFlow::Continue) => run_sync_phases(&mut ctx, repo),
        };

        ctx.stats.stash = StashOutcome {
            stashed: ctx.stashed,
            restored: ctx.stash_restored,
        };

        let duration = sync_started_at.elapsed();
        let trunk_branch = ctx.stack.trunk.clone();
        let remote_trunk_ref = ctx.remote_trunk_ref.clone();

        match phase_result {
            Ok(()) => {
                let output = crate::commands::sync_json::build(
                    &trunk_branch,
                    &remote_trunk_ref,
                    &ctx.stats,
                    false,
                    duration,
                    None,
                );
                println!("{}", crate::commands::sync_json::emit(&output));
                Ok(())
            }
            Err(e) => {
                let is_conflict = e.downcast_ref::<ConflictStopped>().is_some();
                let err_json = crate::commands::sync_json::classify_error(&e);
                let output = crate::commands::sync_json::build(
                    &trunk_branch,
                    &remote_trunk_ref,
                    &ctx.stats,
                    false,
                    duration,
                    Some(err_json),
                );
                println!("{}", crate::commands::sync_json::emit(&output));
                if is_conflict {
                    Err(ConflictStopped.into())
                } else {
                    Err(SilentExit(exit_codes::GENERAL).into())
                }
            }
        }
    } else {
        if ctx.handle_dirty_tree(&repo)? == SyncFlow::Stop {
            return Ok(());
        }
        run_sync_phases(&mut ctx, repo)
    }
}

fn run_sync_phases(ctx: &mut SyncContext, repo: GitRepo) -> Result<()> {
    ctx.begin_transaction(&repo)?;

    if !ctx.quiet {
        println!("{}", "Syncing repository...".bold());
    }

    ctx.fetch_remote(&repo)?;

    // Match dry-run / cleanup: refresh live PR state before merged detection in the plan.
    ctx.refresh_pr_states(&repo)?;

    if ctx.confirm_sync_plan(&repo)? == SyncFlow::Stop {
        if !ctx.quiet {
            println!("Aborted.");
        }
        ctx.restore_stash(&repo)?;
        ctx.tx = None;
        return Ok(());
    }

    let trunk_stats_worker = ctx.spawn_trunk_summary_worker();

    ctx.update_trunk(&repo)?;

    ctx.ensure_trunk_ready_for_restack(&repo)?;

    ctx.refresh_imported(&repo)?;

    ctx.refresh_pr_states(&repo)?;

    let repo = ctx.cleanup_merged_branches(repo)?;
    // Re-check current branch since it may have changed during branch deletion
    ctx.current_after_deletions = repo.current_branch()?;

    ctx.cleanup_upstream_gone(&repo)?;

    ctx.retry_deferred_trunk_update(&repo)?;

    ctx.ensure_trunk_ready_for_restack(&repo)?;

    ctx.restack_phase(&repo)?;

    ctx.restore_stash(&repo)?;

    let current_after = ctx.current_after_deletions.clone();
    ctx.finish_transaction(&current_after)?;

    ctx.finalize(trunk_stats_worker)?;

    Ok(())
}

/// Fetch live PR state from the forge for all tracked branches and update
/// both branch metadata and CiCache. Called before merged-branch detection
/// during sync so that operations like `gh pr ready`, `gh pr merge`, or
/// `gh pr edit --base` are reflected in time for cleanup.
fn refresh_pr_draft_states(repo: &GitRepo, config: &Config, quiet: bool) -> Option<Duration> {
    let started_at = Instant::now();
    let stack = match Stack::load(repo) {
        Ok(s) => s,
        Err(ref e) => {
            if !quiet {
                eprintln!("{}", stale_stack_warning("skipping PR metadata refresh", e));
            }
            return None;
        }
    };
    let tracked_pr_branches: Vec<(String, u64)> = stack
        .branches
        .iter()
        .filter_map(|(branch_name, branch_info)| {
            if branch_name == &stack.trunk {
                return None;
            }
            branch_info
                .pr_number
                .map(|pr_number| (branch_name.clone(), pr_number))
        })
        .collect();
    if tracked_pr_branches.is_empty() {
        return None;
    }

    let timer = LiveTimer::maybe_new(!quiet, "Refresh PR metadata");

    let remote_info = match RemoteInfo::from_repo(repo, config) {
        Ok(info) => info,
        Err(_) => {
            LiveTimer::maybe_finish_skipped(timer, "skipped");
            return Some(started_at.elapsed());
        }
    };
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => {
            LiveTimer::maybe_finish_skipped(timer, "skipped");
            return Some(started_at.elapsed());
        }
    };
    let _enter = rt.enter();
    let client = match ForgeClient::new(&remote_info) {
        Ok(c) => c,
        Err(_) => {
            LiveTimer::maybe_finish_skipped(timer, "skipped");
            return Some(started_at.elapsed());
        }
    };
    let cache_dir = match repo.common_git_dir() {
        Ok(p) => p,
        Err(_) => {
            LiveTimer::maybe_finish_skipped(timer, "skipped");
            return Some(started_at.elapsed());
        }
    };

    let live_prs = rt.block_on(async {
        stream::iter(
            tracked_pr_branches
                .into_iter()
                .map(|(branch_name, pr_number)| {
                    let client = client.clone();
                    async move { (branch_name, client.get_pr(pr_number).await.ok()) }
                }),
        )
        .buffer_unordered(PR_METADATA_REFRESH_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
    });

    for (branch_name, live_pr) in live_prs {
        let Some(live_pr) = live_pr else {
            continue;
        };

        apply_live_pr_state(repo, &stack, &cache_dir, &branch_name, &live_pr);
    }

    LiveTimer::maybe_finish_timed(timer);
    Some(started_at.elapsed())
}

fn apply_live_pr_state(
    repo: &GitRepo,
    stack: &Stack,
    cache_dir: &Path,
    branch_name: &str,
    live_pr: &ForgePrInfo,
) {
    let pr_state = live_pr.state.to_uppercase();

    // Update branch metadata with fresh state, is_draft, and base
    if let Ok(Some(mut meta)) = BranchMetadata::read(repo.inner(), branch_name) {
        if let Some(ref mut pr_info) = meta.pr_info {
            pr_info.is_draft = Some(live_pr.is_draft);
            pr_info.state = pr_state.clone();
        }

        // Reconcile PR base with parent: if the live PR base differs from
        // our tracked parent and the live base is a known branch, update.
        if !live_pr.base.is_empty() && live_pr.base != meta.parent_branch_name {
            let base_is_known =
                live_pr.base == stack.trunk || stack.branches.contains_key(&live_pr.base);
            if base_is_known {
                // The new parent's tip is only a valid boundary if `branch_name` has
                // actually moved onto it; a GitHub-side base retarget (e.g. after an
                // intermediate branch is deleted) can flip `live_pr.base` with no local
                // rebase having happened. Verify ancestry before trusting it, else keep
                // the previously recorded (still-valid) boundary (see #830).
                let new_parent_tip = repo
                    .inner()
                    .find_branch(&live_pr.base, git2::BranchType::Local)
                    .ok()
                    .and_then(|r| r.get().peel_to_commit().ok())
                    .map(|c| c.id().to_string());
                meta.parent_branch_revision = repo.resolve_child_parent_boundary(
                    branch_name,
                    &[new_parent_tip.as_deref()],
                    &meta.parent_branch_revision,
                );
                meta.parent_branch_name = live_pr.base.clone();
            }
        }

        let _ = meta.write(repo.inner(), branch_name);
    }

    let _ = CiCache::update_branch_pr(cache_dir, branch_name, Some(pr_state));
}

fn sync_fetch_refs(
    trunk: &str,
    extra_fetch_refs: &[String],
    remote_heads: Option<&HashSet<String>>,
) -> Vec<String> {
    let mut refs = vec![trunk.to_string()];
    for ref_name in extra_fetch_refs {
        if ref_name != trunk
            && remote_heads
                .map(|heads| heads.contains(ref_name))
                .unwrap_or(true)
            && !refs.iter().any(|existing| existing == ref_name)
        {
            refs.push(ref_name.clone());
        }
    }
    refs
}

fn imported_branches_for_remote(
    repo: &GitRepo,
    stack: &Stack,
    remote_name: &str,
) -> Result<Vec<String>> {
    let mut imported = Vec::new();
    for branch in stack.branches.keys() {
        if branch == &stack.trunk {
            continue;
        }

        let Some(meta) = BranchMetadata::read(repo.inner(), branch)? else {
            continue;
        };
        if meta.source_remote.as_deref() == Some(remote_name) {
            imported.push(branch.clone());
        }
    }
    imported.sort();
    Ok(imported)
}

pub(super) fn imported_branches_for_cleanup(
    repo: &GitRepo,
    stack: &Stack,
) -> Result<HashSet<String>> {
    let mut imported = HashSet::new();
    for branch in stack.branches.keys() {
        if branch == &stack.trunk {
            continue;
        }

        let Some(meta) = BranchMetadata::read(repo.inner(), branch)? else {
            continue;
        };
        if meta.source_remote.is_some() {
            imported.insert(branch.clone());
        }
    }
    Ok(imported)
}

fn refresh_imported_branches(
    repo: &GitRepo,
    workdir: &Path,
    remote_name: &str,
    imported_branches: &[String],
    force: bool,
    quiet: bool,
    verbose: bool,
) -> Result<Vec<String>> {
    let mut updated = Vec::new();
    for branch in imported_branches {
        if BranchMetadata::is_frozen(repo.inner(), branch)? {
            if !quiet {
                println!(
                    "  {} skipped frozen imported branch {}",
                    "❄".cyan(),
                    branch.cyan()
                );
            }
            continue;
        }

        let remote_ref = format!("{}/{}", remote_name, branch);
        let Some(remote_oid) = resolve_ref_oid(workdir, &remote_ref) else {
            if !quiet && verbose {
                println!(
                    "  {} skipped imported branch {} (missing {})",
                    "!".yellow(),
                    branch.cyan(),
                    remote_ref.dimmed()
                );
            }
            continue;
        };

        if resolve_ref_oid(workdir, branch).as_deref() == Some(remote_oid.as_str()) {
            continue;
        }

        if let Some(branch_worktree) = repo.branch_worktree_path(branch)? {
            if worktree_dirty(&branch_worktree)? && !force {
                if !quiet {
                    println!(
                        "  {} skipped imported branch {} (checked out with local changes)",
                        "!".yellow(),
                        branch.cyan()
                    );
                }
                continue;
            }

            let output = Command::new("git")
                .args(["reset", "--hard", &remote_ref])
                .current_dir(&branch_worktree)
                .output()
                .with_context(|| format!("Failed to update imported branch '{}'", branch))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!(
                    "Failed to update imported branch '{}' to '{}': {}",
                    branch,
                    remote_ref,
                    stderr.trim()
                );
            }
        } else {
            let output = Command::new("git")
                .args(["update-ref", &format!("refs/heads/{}", branch), &remote_ref])
                .current_dir(workdir)
                .output()
                .with_context(|| format!("Failed to update imported branch '{}'", branch))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!(
                    "Failed to update imported branch '{}' to '{}': {}",
                    branch,
                    remote_ref,
                    stderr.trim()
                );
            }
        }

        if !quiet {
            println!(
                "  {} updated imported branch {} from {}",
                "↓".cyan(),
                branch.cyan(),
                remote_ref.dimmed()
            );
        }
        updated.push(branch.clone());
    }

    Ok(updated)
}

fn worktree_dirty(workdir: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(workdir)
        .output()
        .context("Failed to inspect imported branch worktree")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git status failed: {}", stderr.trim());
    }

    Ok(!output.stdout.is_empty())
}

/// Drop stale `refs/remotes/<remote>/<branch>` for stax-tracked branches that no longer exist on the remote.
fn prune_stale_remote_tracking_refs(
    workdir: &Path,
    remote_name: &str,
    stack: &Stack,
    remote_branches: &HashSet<String>,
) {
    for branch in stack.branches.keys() {
        if branch == &stack.trunk {
            continue;
        }
        if remote_branches.contains(branch.as_str()) {
            continue;
        }
        let refname = format!("refs/remotes/{}/{}", remote_name, branch);
        let _ = Command::new("git")
            .args(["update-ref", "-d", &refname])
            .current_dir(workdir)
            .status();
    }
}

#[derive(Debug, Clone)]
pub(super) enum MergeType {
    Ancestor,    // Detected via git branch --merged
    SquashMerge, // Detected via patch-ID matching
}

#[derive(Debug, Clone)]
pub(super) struct MergedBranchInfo {
    pub(super) branch: String,
    pub(super) merge_type: MergeType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PartialMergeReason {
    PrMerged,
    PrClosed,
    HistoryMerged,
}

#[derive(Debug, Clone)]
pub(super) struct PartiallyMergedNote {
    pub(super) branch: String,
    pub(super) pr_number: Option<u64>,
    pub(super) pr_label: PartialMergeReason,
    pub(super) extra_commits: usize,
}

fn should_spare_empty_never_submitted_branch(
    workdir: &Path,
    stack: &Stack,
    branch: &str,
) -> Result<bool> {
    let Some(info) = stack.branches.get(branch) else {
        return Ok(false);
    };
    if info.pr_number.is_some() {
        return Ok(false);
    }

    // Use the recorded parent revision so branches with real commits that were
    // later merged into trunk are not mistaken for never-started branches.
    let parent = info
        .parent_revision
        .as_deref()
        .or(info.parent.as_deref())
        .unwrap_or(&stack.trunk);
    Ok(!has_unique_commits_since_any_base(
        workdir,
        branch,
        &[parent],
    )?)
}

/// Union planned merged branches with a post-trunk fast re-scan. Planned entries
/// cover squash merges found via patch-id before trunk moved; `fresh` picks up
/// any branch that became an ancestor after the fast-forward.
fn merge_planned_merged_detection(
    planned: Vec<MergedBranchInfo>,
    fresh: Vec<MergedBranchInfo>,
) -> Vec<MergedBranchInfo> {
    let mut by_branch: HashMap<String, MergedBranchInfo> = fresh
        .into_iter()
        .map(|info| (info.branch.clone(), info))
        .collect();
    for info in planned {
        by_branch.entry(info.branch.clone()).or_insert(info);
    }
    by_branch.into_values().collect()
}

pub(super) fn find_merged_branches(
    repo: &GitRepo,
    workdir: &std::path::Path,
    stack: &Stack,
    remote_name: &str,
    remote_branches: &HashSet<String>,
    skip_patch_id_provenance: bool,
) -> Result<Vec<MergedBranchInfo>> {
    let mut merged = Vec::new();
    let remote_trunk_ref = format!("{}/{}", remote_name, stack.trunk);

    // Method 1: git branch --merged (finds local branches merged into trunk)
    let output = Command::new("git")
        .args(["branch", "--merged", &stack.trunk])
        .current_dir(workdir)
        .output()
        .context("Failed to list merged branches")?;

    let merged_output = String::from_utf8_lossy(&output.stdout);

    for line in merged_output.lines() {
        let branch = branch_name_from_merged_output(line);

        // Skip trunk itself and any non-tracked branches
        if branch == stack.trunk || branch.is_empty() {
            continue;
        }

        // Only include branches we're tracking
        if stack.branches.contains_key(branch)
            && !should_spare_empty_never_submitted_branch(workdir, stack, branch)?
        {
            merged.push(MergedBranchInfo {
                branch: branch.to_string(),
                merge_type: MergeType::Ancestor,
            });
        }
    }

    // Method 1b: git branch --merged origin/trunk (handles stale/diverged local trunk)
    let output = Command::new("git")
        .args(["branch", "--merged", &remote_trunk_ref])
        .current_dir(workdir)
        .output();

    if let Ok(output) = output {
        let merged_output = String::from_utf8_lossy(&output.stdout);

        for line in merged_output.lines() {
            let branch = branch_name_from_merged_output(line);

            // Skip trunk itself and any non-tracked branches
            if branch == stack.trunk || branch.is_empty() {
                continue;
            }

            // Only include branches we're tracking (and avoid duplicates)
            if stack.branches.contains_key(branch)
                && !merged.iter().any(|info| info.branch == branch)
                && !should_spare_empty_never_submitted_branch(workdir, stack, branch)?
            {
                merged.push(MergedBranchInfo {
                    branch: branch.to_string(),
                    merge_type: MergeType::Ancestor,
                });
            }
        }
    }

    // Method 2: Check PR state from metadata.
    // Only an explicitly merged PR is a strong enough signal for cleanup here.
    // Closed-but-unmerged PRs must be preserved unless some other merge/deletion
    // heuristic below proves the branch is safe to clean up.
    for (branch, info) in &stack.branches {
        // Skip trunk
        if branch == &stack.trunk {
            continue;
        }

        // Skip if already in merged list
        if merged.iter().any(|m| &m.branch == branch) {
            continue;
        }

        if matches!(
            info.pr_state.as_deref(),
            Some(state) if state.eq_ignore_ascii_case("merged")
        ) {
            merged.push(MergedBranchInfo {
                branch: branch.clone(),
                merge_type: MergeType::Ancestor,
            });
        }
    }

    // Method 4: Check if the tracked remote branch was deleted (GitHub deletes
    // branch after merge). This is cheaper and more robust than enumerating the
    // entire remote ref namespace in very large repos.
    for (branch, info) in &stack.branches {
        // Skip trunk
        if branch == &stack.trunk {
            continue;
        }

        // Skip if already in merged list
        if merged.iter().any(|m| &m.branch == branch) {
            continue;
        }

        // Only consider "remote deleted" if branch had a PR before (was pushed)
        // This prevents false positives for branches that were never pushed
        if info.pr_number.is_none() {
            continue;
        }

        // Check if remote branch was deleted (strong signal it was merged)
        if !remote_branches.contains(branch.as_str()) {
            // Remote branch doesn't exist and had a PR - likely merged and deleted
            merged.push(MergedBranchInfo {
                branch: branch.clone(),
                merge_type: MergeType::Ancestor,
            });
        }
    }

    // Method 5: Find orphaned branches (tracked but no longer exist locally or remotely)
    let local_output = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(workdir)
        .output()
        .context("Failed to list local branches")?;
    let local_branches: std::collections::HashSet<String> =
        String::from_utf8_lossy(&local_output.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .collect();
    for branch in stack.branches.keys() {
        // Skip trunk
        if branch == &stack.trunk {
            continue;
        }

        // Skip if already in merged list
        if merged.iter().any(|m| &m.branch == branch) {
            continue;
        }

        let local_exists = local_branches.contains(branch);
        let remote_exists = remote_branches.contains(branch.as_str());

        // If branch doesn't exist locally AND doesn't exist remotely, it's orphaned
        if !local_exists && !remote_exists {
            merged.push(MergedBranchInfo {
                branch: branch.clone(),
                merge_type: MergeType::Ancestor,
            });
        }
    }

    // Method 3: Patch-id provenance check — detects squash/rebase merges even
    // when trunk has advanced past the merge point (where a simple tree diff
    // would show false negatives). Run this last so cheaper signals resolve
    // most cases before the provenance path touches more refs.
    if skip_patch_id_provenance {
        return Ok(merged);
    }

    let trunk = stack.trunk.as_str();
    let mut need_patch_id: Vec<(String, String)> = Vec::new();

    for branch in stack.branches.keys() {
        if branch == &stack.trunk || merged.iter().any(|m| &m.branch == branch) {
            continue;
        }
        if should_spare_empty_never_submitted_branch(workdir, stack, branch)? {
            continue;
        }
        // Remote still exists -> not merged via squash-delete; skip expensive check.
        if remote_branches.contains(branch.as_str()) {
            continue;
        }
        match repo.is_branch_merged_cheap(branch) {
            Ok(Some(())) => merged.push(MergedBranchInfo {
                branch: branch.clone(),
                merge_type: MergeType::Ancestor,
            }),
            Ok(None) => {
                if let Ok(mb) = repo.merge_base(trunk, branch) {
                    need_patch_id.push((branch.clone(), mb));
                }
            }
            Err(_) => {}
        }
    }

    if !need_patch_id.is_empty() {
        let mut by_merge_base: HashMap<String, Vec<String>> = HashMap::new();
        for (branch, mb) in need_patch_id {
            by_merge_base.entry(mb).or_default().push(branch);
        }

        for (merge_base, branches) in by_merge_base {
            let trunk_range = format!("{}..{}", merge_base, trunk);
            let trunk_count = match repo.rev_list_count(workdir, &trunk_range) {
                Ok(c) => c,
                Err(_) => {
                    for branch in branches {
                        if repo
                            .is_branch_merged_equivalent_to_trunk(&branch)
                            .unwrap_or(false)
                        {
                            merged.push(MergedBranchInfo {
                                branch,
                                merge_type: MergeType::Ancestor,
                            });
                        }
                    }
                    continue;
                }
            };

            if trunk_count > GitRepo::PATCH_ID_TRUNK_COMMIT_CAP {
                for branch in branches {
                    if repo
                        .is_branch_merged_equivalent_to_trunk(&branch)
                        .unwrap_or(false)
                    {
                        merged.push(MergedBranchInfo {
                            branch,
                            merge_type: MergeType::Ancestor,
                        });
                    }
                }
                continue;
            }

            let trunk_patch_ids = match repo.patch_ids_for_range(workdir, &trunk_range) {
                Ok(ids) => ids,
                Err(_) => {
                    for branch in branches {
                        if repo
                            .is_branch_merged_equivalent_to_trunk(&branch)
                            .unwrap_or(false)
                        {
                            merged.push(MergedBranchInfo {
                                branch,
                                merge_type: MergeType::Ancestor,
                            });
                        }
                    }
                    continue;
                }
            };

            for branch in branches {
                let branch_range = format!("{}..{}", merge_base, branch);
                let branch_patch_ids = match repo.patch_ids_for_range(workdir, &branch_range) {
                    Ok(ids) => ids,
                    Err(_) => continue,
                };
                if branch_patch_ids.is_empty() || branch_patch_ids.is_subset(&trunk_patch_ids) {
                    merged.push(MergedBranchInfo {
                        branch,
                        merge_type: MergeType::SquashMerge,
                    });
                }
            }
        }
    }

    Ok(merged)
}

/// Surface tracked branches that sync deliberately spares from deletion
/// despite a merged/closed PR (or already-integrated history) because they
/// carry local commits not present on trunk or any remote — turning what
/// would otherwise be a silent skip into an explicit "not deleting" note.
pub(super) fn find_partially_merged_notes(
    repo: &GitRepo,
    workdir: &Path,
    stack: &Stack,
    remote_name: &str,
    remote_branches: &HashSet<String>,
    merged: &[MergedBranchInfo],
) -> Result<Vec<PartiallyMergedNote>> {
    let mut notes = Vec::new();
    let trunk = stack.trunk.clone();

    for branch in stack.branches.keys() {
        let branch = branch.clone();
        if branch == trunk {
            continue;
        }
        if merged.iter().any(|m| m.branch == branch) {
            continue;
        }
        let Some(info) = stack.branches.get(&branch) else {
            continue;
        };

        let mut bases: Vec<String> = vec![trunk.clone()];
        let remote_trunk_ref = format!("{}/{}", remote_name, trunk);
        if git_ref_exists(workdir, &remote_trunk_ref) {
            bases.push(remote_trunk_ref);
        }
        let remote_branch_ref = format!("{}/{}", remote_name, branch);
        if remote_branches.contains(&branch) || git_ref_exists(workdir, &remote_branch_ref) {
            bases.push(remote_branch_ref);
        }
        let base_refs: Vec<&str> = bases.iter().map(String::as_str).collect();

        let extra = count_extra_commits(workdir, &branch, &base_refs)?;
        if extra == 0 {
            continue;
        }

        let reason = if matches!(
            info.pr_state.as_deref(),
            Some(state) if state.eq_ignore_ascii_case("merged")
        ) {
            PartialMergeReason::PrMerged
        } else if matches!(
            info.pr_state.as_deref(),
            Some(state) if state.eq_ignore_ascii_case("closed")
        ) {
            PartialMergeReason::PrClosed
        } else {
            let Ok(merge_base) = repo.merge_base(&trunk, &branch) else {
                continue;
            };
            let trunk_range = format!("{}..{}", merge_base, trunk);
            let trunk_count = match repo.rev_list_count(workdir, &trunk_range) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if trunk_count == 0 || trunk_count > GitRepo::PATCH_ID_TRUNK_COMMIT_CAP {
                continue;
            }
            let trunk_patch_ids = match repo.patch_ids_for_range(workdir, &trunk_range) {
                Ok(ids) => ids,
                Err(_) => continue,
            };
            let branch_range = format!("{}..{}", merge_base, branch);
            let branch_patch_ids = match repo.patch_ids_for_range(workdir, &branch_range) {
                Ok(ids) => ids,
                Err(_) => continue,
            };
            if branch_patch_ids.is_disjoint(&trunk_patch_ids) {
                continue;
            }
            PartialMergeReason::HistoryMerged
        };

        notes.push(PartiallyMergedNote {
            branch: branch.clone(),
            pr_number: info.pr_number,
            pr_label: reason,
            extra_commits: extra,
        });
    }

    Ok(notes)
}

fn count_extra_commits(workdir: &Path, branch: &str, bases: &[&str]) -> Result<usize> {
    let mut args: Vec<&str> = vec!["rev-list", "--count", branch, "--not"];
    args.extend_from_slice(bases);

    let output = Command::new("git")
        .args(&args)
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to count extra commits for '{}'", branch))?;

    if !output.status.success() {
        return Ok(0);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().parse::<usize>().unwrap_or(0))
}

fn branch_name_from_merged_output(line: &str) -> &str {
    let branch = line.trim();
    branch
        .strip_prefix("* ")
        .or_else(|| branch.strip_prefix("+ "))
        .unwrap_or(branch)
}

pub(super) fn find_upstream_gone_branches(
    workdir: &std::path::Path,
    trunk: &str,
) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname:short)%00%(upstream:short)%00%(upstream:track)",
            "refs/heads",
        ])
        .current_dir(workdir)
        .output()
        .context("Failed to list local branches with upstream tracking info")?;

    let mut branches = std::collections::BTreeSet::new();
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        let mut fields = line.split('\0');
        let branch = fields.next().unwrap_or("").trim();
        let _upstream = fields.next().unwrap_or("").trim();
        let tracking = fields.next().unwrap_or("").trim();

        if branch.is_empty() || branch == trunk {
            continue;
        }

        if tracking.contains("[gone]") {
            branches.insert(branch.to_string());
        }
    }

    Ok(branches.into_iter().collect())
}

pub(super) fn local_branch_exists(workdir: &std::path::Path, branch: &str) -> bool {
    let local_ref = format!("refs/heads/{}", branch);
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet", &local_ref])
        .current_dir(workdir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn git_ref_exists(workdir: &Path, refname: &str) -> bool {
    let commit_ref = format!("{}^{{commit}}", refname);
    Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", &commit_ref])
        .current_dir(workdir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub(super) fn init_forge_client(
    repo: &GitRepo,
    config: &Config,
) -> Option<(tokio::runtime::Runtime, ForgeClient)> {
    let remote_info = RemoteInfo::from_repo(repo, config).ok();
    if let Some(info) = remote_info {
        tokio::runtime::Runtime::new().ok().and_then(|rt| {
            let _enter = rt.enter();
            ForgeClient::new(&info).ok().map(|client| (rt, client))
        })
    } else {
        None
    }
}

fn print_blocked_or_skipped(
    branch: &str,
    blocking_worktree_cleanup: Option<&BlockingWorktreeCleanup>,
    local_worktree_blocked: bool,
) {
    if local_worktree_blocked {
        print_blocked_branch_delete_recovery(branch, blocking_worktree_cleanup);
    } else {
        println!("    {} {}", branch.bright_black(), "skipped".dimmed());
    }
}

fn print_metadata_kept_note() {
    println!(
        "    {} {}",
        "↷".yellow(),
        "metadata kept because local branch still exists".dimmed()
    );
}

pub(super) fn print_cleanup_candidates(kind: &str, branch_names: &[String]) {
    print_cleanup_candidates_with_stack(kind, branch_names, None);
}

pub(super) fn print_cleanup_candidates_with_stack(
    kind: &str,
    branch_names: &[String],
    stack: Option<&Stack>,
) {
    let branch_word = if branch_names.len() == 1 {
        "branch"
    } else {
        "branches"
    };
    println!(
        "    Found {} {} {}:",
        branch_names.len().to_string().cyan(),
        kind,
        branch_word
    );
    for name in branch_names {
        print_cleanup_candidate_branch(name, stack);
    }
    println!();
}

fn print_cleanup_candidate_branch(name: &str, stack: Option<&Stack>) {
    print!("      {} {}", "▸".bright_black(), name);
    if let Some(stack) = stack
        && let Some(info) = stack.branches.get(name)
        && let Some(pr) = info.pr_number
    {
        print!("  {}", format!("PR #{pr}").dimmed());
    }
    println!();
}

pub(super) fn plan_blocking_worktree_cleanup(
    repo: &GitRepo,
    branch: &str,
    force: bool,
) -> Result<Option<BlockingWorktreeCleanup>> {
    let Some(resolution) = repo.branch_delete_resolution(branch)? else {
        return Ok(None);
    };

    if resolution.worktree.is_main {
        return Ok(Some(BlockingWorktreeCleanup {
            resolution,
            blockers: Vec::new(),
        }));
    }

    let details = compute_worktree_details(repo, resolution.worktree.clone())?;
    Ok(Some(BlockingWorktreeCleanup {
        resolution,
        blockers: worktree_removal_blockers_for_cleanup(&details, force),
    }))
}

fn preserve_worktree_for_sync(
    repo: &GitRepo,
    cleanup: &BlockingWorktreeCleanup,
    quiet: bool,
) -> Result<()> {
    let target = repo.switch_worktree_for_branch_delete(&cleanup.resolution)?;
    if !quiet {
        let destination = match target {
            BranchDeleteSwitchTarget::Branch(target) => format!("switched to {}", target),
            BranchDeleteSwitchTarget::Detach => "detached HEAD".to_string(),
        };
        println!(
            "    {} kept linked worktree {} ({})",
            "→".cyan(),
            cleanup.resolution.worktree.name.cyan(),
            destination
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn delete_local_branch_for_sync(
    repo: &GitRepo,
    config: &Config,
    workdir: &std::path::Path,
    branch: &str,
    blocking_worktree_cleanup: Option<&BlockingWorktreeCleanup>,
    force_remove_linked_worktree: bool,
    quiet: bool,
) -> Result<LocalBranchDeleteOutcome> {
    let mut outcome = attempt_local_branch_delete(workdir, branch);
    if outcome.deleted || !outcome.worktree_blocked {
        return Ok(outcome);
    }

    let Some(cleanup) = blocking_worktree_cleanup else {
        return Ok(outcome);
    };

    let force_remove_linked_worktree =
        force_remove_linked_worktree && cleanup.can_force_remove_dirty_worktree_during_sync();
    if !cleanup.can_remove_during_sync() && !force_remove_linked_worktree {
        return Ok(outcome);
    }

    let removed_worktree = remove_worktree_with_hooks(
        repo,
        config,
        &cleanup.resolution.worktree,
        force_remove_linked_worktree,
        crate::commands::worktree::remove::RemovalMode::AllowParking,
    );
    match removed_worktree {
        Ok(display_name) => {
            if !quiet {
                println!(
                    "    {} removed linked worktree {}",
                    "→".cyan(),
                    display_name.cyan()
                );
            }
            outcome = attempt_local_branch_delete(workdir, branch);
            Ok(outcome)
        }
        Err(error) => {
            if !quiet {
                println!(
                    "    {} {}",
                    "↷".yellow(),
                    format!(
                        "couldn't remove linked worktree '{}': {}",
                        cleanup.resolution.worktree.name, error
                    )
                    .dimmed()
                );
            }
            Ok(outcome)
        }
    }
}

fn attempt_local_branch_delete(
    workdir: &std::path::Path,
    branch: &str,
) -> LocalBranchDeleteOutcome {
    let local_output = Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(workdir)
        .output();

    match local_output {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            LocalBranchDeleteOutcome {
                deleted: out.status.success(),
                worktree_blocked: stderr.contains("used by worktree"),
            }
        }
        Err(_) => LocalBranchDeleteOutcome::default(),
    }
}

fn sync_delete_prompt(
    branch: &str,
    checkout_target: Option<&str>,
    reason: Option<&str>,
    blocking_worktree_cleanup: Option<&BlockingWorktreeCleanup>,
) -> String {
    if let Some(target) = checkout_target {
        if let Some(reason) = reason {
            return format!("Delete '{}' ({reason}) and checkout '{}'?", branch, target);
        }

        return format!("Delete '{}' and checkout '{}'?", branch, target);
    }

    if let Some(cleanup) = blocking_worktree_cleanup {
        if cleanup.can_remove_during_sync() {
            if let Some(reason) = reason {
                return format!(
                    "Delete '{}' ({reason}) and remove linked worktree '{}'?",
                    branch, cleanup.resolution.worktree.name
                );
            }

            return format!(
                "Delete '{}' and remove linked worktree '{}'?",
                branch, cleanup.resolution.worktree.name
            );
        }

        if cleanup.can_force_remove_dirty_worktree_during_sync() {
            if let Some(reason) = reason {
                return format!(
                    "Delete '{}' ({reason}) and force-remove dirty linked worktree '{}'?",
                    branch, cleanup.resolution.worktree.name
                );
            }

            return format!(
                "Delete '{}' and force-remove dirty linked worktree '{}'?",
                branch, cleanup.resolution.worktree.name
            );
        }

        if let Some(reason) = reason {
            return format!(
                "Delete '{}' ({reason}; keep linked worktree '{}')?",
                branch, cleanup.resolution.worktree.name
            );
        }

        return format!(
            "Delete '{}' (keep linked worktree '{}')?",
            branch, cleanup.resolution.worktree.name
        );
    }

    if let Some(reason) = reason {
        format!("Delete '{}' ({reason})?", branch)
    } else {
        format!("Delete '{}'?", branch)
    }
}

fn linked_worktree_delete_options(
    cleanup: &BlockingWorktreeCleanup,
) -> Vec<(String, SyncBranchDeleteAction)> {
    let keep_label = match &cleanup.resolution.switch_target {
        BranchDeleteSwitchTarget::Branch(target) => {
            format!(
                "Keep worktree, switch it to '{}', and delete branch",
                target
            )
        }
        BranchDeleteSwitchTarget::Detach => {
            "Keep worktree, detach HEAD, and delete branch".to_string()
        }
    };
    let mut options = vec![(keep_label, SyncBranchDeleteAction::PreserveWorktree)];

    if cleanup.can_remove_during_sync() {
        options.push((
            "Remove worktree and delete branch".to_string(),
            SyncBranchDeleteAction::RemoveWorktree { force: false },
        ));
    } else if cleanup.can_force_remove_dirty_worktree_during_sync() {
        options.push((
            "Force-remove dirty worktree and delete branch".to_string(),
            SyncBranchDeleteAction::RemoveWorktree { force: true },
        ));
    }

    options.push(("Skip".to_string(), SyncBranchDeleteAction::Skip));
    options
}

fn choose_linked_worktree_delete_action(
    branch: &str,
    cleanup: &BlockingWorktreeCleanup,
) -> Result<SyncBranchDeleteAction> {
    let options = linked_worktree_delete_options(cleanup);
    let labels = options
        .iter()
        .map(|(label, _)| label.as_str())
        .collect::<Vec<_>>();
    let selected = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "Branch '{}' is checked out in worktree '{}'. What should stax do?",
            branch, cleanup.resolution.worktree.name
        ))
        .items(&labels)
        .default(0)
        .interact()?;
    Ok(options[selected].1)
}

fn print_blocked_branch_delete_recovery(
    branch: &str,
    blocking_worktree_cleanup: Option<&BlockingWorktreeCleanup>,
) {
    println!(
        "    {} {}",
        branch.bright_black(),
        "not deleted locally (checked out in another worktree)".yellow()
    );
    if let Some(cleanup) = blocking_worktree_cleanup {
        if let Some(reason) = cleanup.blocker_summary() {
            println!(
                "    {} {}",
                "↷".yellow(),
                format!(
                    "sync kept linked worktree '{}' because {}",
                    cleanup.resolution.worktree.name, reason
                )
                .dimmed()
            );
        }

        let resolution = &cleanup.resolution;
        if let Some(remove_cmd) = resolution.remove_worktree_and_branch_cmd() {
            println!(
                "    {} {}",
                "↷".yellow(),
                "Run to remove that worktree and delete the branch:".dimmed()
            );
            println!("      {}", remove_cmd.cyan());
        }
        println!(
            "    {} {}",
            "↷".yellow(),
            if resolution.worktree.is_main {
                "Run to free the branch in the main worktree:".dimmed()
            } else {
                "Or keep the worktree and free the branch:".dimmed()
            }
        );
        println!("      {}", resolution.switch_branch_cmd().cyan());
    }
}

fn checkout_branch_for_cleanup(
    repo: &GitRepo,
    workdir: &std::path::Path,
    branch: &str,
) -> std::result::Result<(), String> {
    if let Ok(Some(other_worktree_path)) = repo.branch_worktree_path(branch) {
        let current_path = std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
        let other_path = std::fs::canonicalize(&other_worktree_path)
            .unwrap_or_else(|_| other_worktree_path.clone());
        if other_path != current_path {
            return Err(format!(
                "'{}' is already checked out in another worktree at '{}'",
                branch,
                other_worktree_path.display()
            ));
        }
    }

    let output = Command::new("git")
        .args(["checkout", branch])
        .current_dir(workdir)
        .output()
        .map_err(|e| format!("git checkout '{}' failed: {}", branch, e))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(format!(
            "git checkout '{}' exited with {}",
            branch, output.status
        ))
    } else {
        Err(stderr)
    }
}

/// Walk the parent chain from `branch` skipping any branch in `doomed` (e.g.
/// branches scheduled for deletion in the same sync pass). Returns the first
/// ancestor that is not doomed and still exists locally, falling back to trunk.
/// This prevents reparenting children onto a branch that is about to be deleted
/// when multiple branches in the same stack have their upstream gone.
pub(super) fn resolve_fallback_parent_skipping_doomed(
    workdir: &std::path::Path,
    stack: &Stack,
    branch: &str,
    doomed: &[String],
) -> (String, Option<String>) {
    let recorded_parent = stack
        .branches
        .get(branch)
        .and_then(|b| b.parent.clone())
        .unwrap_or_else(|| stack.trunk.clone());

    let mut current = recorded_parent.clone();
    let mut visited: std::collections::HashSet<String> =
        std::collections::HashSet::from([branch.to_string()]);

    // Walk up the recorded-parent chain. `visited.insert` doubles as the cycle
    // guard: once we revisit a branch we fall through to the trunk fallback.
    while visited.insert(current.clone()) {
        let is_doomed = doomed.iter().any(|d| d == &current);
        if !is_doomed && local_branch_exists(workdir, &current) {
            let fallback_from = if current == recorded_parent {
                None
            } else {
                Some(recorded_parent.clone())
            };
            return (current, fallback_from);
        }
        // Walk up to the parent of `current`; if none, break to trunk.
        match stack.branches.get(&current).and_then(|b| b.parent.clone()) {
            Some(next) if next != current => current = next,
            _ => break,
        }
    }

    // Fall back to trunk if nothing else worked.
    let fallback_from = if recorded_parent == stack.trunk {
        None
    } else {
        Some(recorded_parent)
    };
    (stack.trunk.clone(), fallback_from)
}

/// Reparent tracked children of `branch` onto `new_parent`, preserving the old
/// parent boundary for later restack (see issue #120 rationale). Best-effort
/// updates the PR base on the forge when a child has a tracked PR.
///
/// Used by both the merged-branch and upstream-gone cleanup paths.
/// Return the names of children that will be reparented when `branch` is deleted.
/// Excludes any child whose name appears in `skipped_deletions` (i.e. also being deleted).
fn children_to_reparent(
    stack_snapshot: &Stack,
    branch: &str,
    skipped_deletions: &HashSet<String>,
) -> Vec<String> {
    stack_snapshot
        .branches
        .iter()
        .filter(|(_, info)| info.parent.as_deref() == Some(branch))
        .map(|(name, _)| name.clone())
        .filter(|name| !skipped_deletions.contains(name))
        .collect()
}

fn reparent_children_for_deletion(
    repo: &GitRepo,
    stack_snapshot: &Stack,
    branch: &str,
    new_parent: &str,
    skipped_children: &HashSet<String>,
    forge_client: Option<&(tokio::runtime::Runtime, ForgeClient)>,
    quiet: bool,
) -> Result<()> {
    let children: Vec<String> = children_to_reparent(stack_snapshot, branch, skipped_children);
    let doomed_tip = repo.branch_commit(branch).ok();

    for child in &children {
        let Some(child_meta) = BranchMetadata::read(repo.inner(), child)? else {
            continue;
        };

        // Preserve the old-parent boundary so restack can run `git rebase
        // --onto <new> <old>` precisely. Only use the deleted branch's tip
        // when it is still in the child's ancestry; otherwise keep the
        // recorded revision (see #120).
        let old_parent_boundary = repo.resolve_child_parent_boundary(
            child,
            &[doomed_tip.as_deref()],
            &child_meta.parent_branch_revision,
        );

        let updated_meta = BranchMetadata {
            parent_branch_name: new_parent.to_string(),
            parent_branch_revision: old_parent_boundary,
            ..child_meta.clone()
        };
        updated_meta.write(repo.inner(), child)?;

        // Best-effort PR base update. Expected to fail when upstream is gone
        // (PR closed) — log and continue.
        if let (Some(pr_info), Some((rt, client))) = (&child_meta.pr_info, forge_client) {
            match rt.block_on(client.update_pr_base(pr_info.number, new_parent)) {
                Ok(()) => {
                    if !quiet {
                        println!(
                            "    {} updated PR #{} base → {}",
                            "↪".cyan(),
                            pr_info.number,
                            new_parent.cyan()
                        );
                    }
                }
                Err(e) => {
                    if !quiet {
                        println!(
                            "    {} couldn't update PR #{} base: {}",
                            "⚠".yellow(),
                            pr_info.number,
                            e
                        );
                    }
                }
            }
        }

        if !quiet {
            println!(
                "    {} reparented {} → {}",
                "↪".cyan(),
                child.cyan(),
                new_parent.cyan()
            );
        }
    }
    Ok(())
}

/// Record CI history for merged branches before they are deleted
fn record_ci_history_for_merged(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    merged_branches: &[String],
    stack: &Stack,
    quiet: bool,
) {
    // Only process branches that still exist locally (can get their commit SHA)
    let branches_to_check: Vec<String> = merged_branches
        .iter()
        .filter(|b| repo.branch_commit(b).is_ok())
        .cloned()
        .collect();

    if branches_to_check.is_empty() {
        return;
    }

    let ci_timer = LiveTimer::maybe_new(!quiet, "Record CI history");

    // Fetch CI statuses for merged branches
    match fetch_ci_statuses(repo, rt, client, stack, &branches_to_check) {
        Ok(statuses) => {
            record_ci_history(repo, &statuses);
            LiveTimer::maybe_finish_timed(ci_timer);
        }
        Err(_) => {
            LiveTimer::maybe_finish_warn(ci_timer, "skipped (couldn't fetch)");
        }
    }
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3}s", duration.as_secs_f64())
}

fn print_restack_branch_timings(restack_branch_timings: &[RestackBranchTiming]) {
    for timing in restack_branch_timings {
        println!(
            "  {}",
            format!("restack branch {}", timing.branch).bright_cyan()
        );
        println!(
            "    {:<31} {}",
            "worktree prep",
            format_duration(timing.rebase_timings.prepare_context).dimmed()
        );
        println!(
            "    {:<31} {}",
            "dirty check",
            format_duration(timing.rebase_timings.dirty_check).dimmed()
        );
        if !timing.rebase_timings.auto_stash_push.is_zero() {
            println!(
                "    {:<31} {}",
                "auto-stash push",
                format_duration(timing.rebase_timings.auto_stash_push).dimmed()
            );
        }
        println!(
            "    {:<31} {}",
            "git rebase",
            format_duration(timing.rebase_timings.git_rebase).dimmed()
        );
        if !timing.rebase_timings.auto_stash_pop.is_zero() {
            println!(
                "    {:<31} {}",
                "auto-stash pop",
                format_duration(timing.rebase_timings.auto_stash_pop).dimmed()
            );
        }
        if !timing.metadata_update.is_zero() {
            println!(
                "    {:<31} {}",
                "metadata update",
                format_duration(timing.metadata_update).dimmed()
            );
        }
        println!(
            "    {:<31} {}",
            "branch total",
            format_duration(timing.total()).dimmed()
        );
    }
}

fn render_sync_footer(stats: &SyncStats, total_duration: Duration) -> String {
    let mut parts = Vec::new();

    if let Some(trunk) = &stats.trunk {
        match trunk {
            TrunkSummary::UpToDate { branch } => {
                parts.push(format!(
                    "{} {}",
                    branch.cyan().bold(),
                    "up to date".dimmed()
                ));
            }
            TrunkSummary::Pulled {
                branch,
                commits,
                files,
                additions,
                deletions,
            } => {
                parts.push(format!(
                    "{} {}",
                    branch.cyan().bold(),
                    format!(
                        "+{} commit{}",
                        commits,
                        if *commits == 1 { "" } else { "s" }
                    )
                    .green()
                ));
                parts.push(format!(
                    "{} {} {}",
                    format!("{} file{}", files, if *files == 1 { "" } else { "s" }).dimmed(),
                    format!("+{}", additions).green(),
                    format!("-{}", deletions).red()
                ));
            }
            TrunkSummary::Updated { branch } => {
                parts.push(format!("{} {}", branch.cyan().bold(), "updated".yellow()));
            }
        }
    }

    if stats.merged_branches_cleaned > 0 {
        parts.push(format!(
            "{} {} {}",
            "cleaned".dimmed(),
            stats.merged_branches_cleaned.to_string().cyan().bold(),
            "merged".dimmed()
        ));
    }

    if !stats.restacked_branches.is_empty() {
        parts.push(format!(
            "{} {}",
            "restacked".dimmed(),
            stats.restacked_branches.len().to_string().cyan().bold()
        ));
    }

    if !stats.imported_branches_updated.is_empty() {
        parts.push(format!(
            "{} {}",
            "updated".dimmed(),
            format!("{} imported", stats.imported_branches_updated.len())
                .cyan()
                .bold()
        ));
    }

    parts.push(format!("{}", format_duration(total_duration).cyan()));
    parts.join(&format!("{}", " | ".dimmed()))
}

fn render_sync_follow_up(stats: &SyncStats) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(trunk) = &stats.trunk_not_updated {
        match trunk.failure {
            TrunkUpdateFailure::Diverged => lines.push(format!(
                "⚠ {} diverged from {}; review the warnings above",
                trunk.branch, trunk.remote_ref
            )),
            TrunkUpdateFailure::Other => lines.push(format!(
                "⚠ {} did not reach {}; review the warnings above",
                trunk.branch, trunk.remote_ref
            )),
        }
    }

    for skip in &stats.cleanup_skips {
        lines.push(format!(
            "⚠ Cleanup skipped for {} ({})",
            skip.branch, skip.reason
        ));
    }

    if let Some(checkout) = &stats.checkout_change {
        lines.push(format!(
            "→ Checked out {} after cleanup (was {})",
            checkout.to, checkout.from
        ));
    }

    if let Some(trunk) = &stats.trunk_not_updated {
        match trunk.failure {
            TrunkUpdateFailure::Diverged => lines.push(format!(
                "Next: inspect and reconcile {} with {}",
                trunk.branch, trunk.remote_ref
            )),
            TrunkUpdateFailure::Other => lines.push("Next: st trunk".to_string()),
        }
    } else if !stats.cleanup_skips.is_empty() {
        lines.push("Next: st sweep".to_string());
    }

    lines
}

fn wait_for_trunk_summary(
    worker: std::thread::JoinHandle<Result<Option<TrunkSummary>>>,
) -> Result<Option<TrunkSummary>> {
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("trunk summary worker panicked"))?
}

fn summarize_trunk_transition(
    workdir: &Path,
    branch: &str,
    local_before: Option<&str>,
    remote_after_fetch: Option<&str>,
) -> Result<Option<TrunkSummary>> {
    if let Some(remote_after_fetch) = remote_after_fetch {
        if let Some(local_before) = local_before {
            if local_before == remote_after_fetch {
                return Ok(Some(TrunkSummary::UpToDate {
                    branch: branch.to_string(),
                }));
            }

            if is_ancestor(workdir, local_before, remote_after_fetch) {
                let commits = count_commits_between(workdir, local_before, remote_after_fetch)?;
                let (files, additions, deletions) =
                    diff_line_stats_between(workdir, local_before, remote_after_fetch)?;
                return Ok(Some(TrunkSummary::Pulled {
                    branch: branch.to_string(),
                    commits,
                    files,
                    additions,
                    deletions,
                }));
            }
        }

        return Ok(Some(TrunkSummary::Updated {
            branch: branch.to_string(),
        }));
    }

    Ok(None)
}

pub(super) fn resolve_ref_oid(workdir: &Path, reference: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", reference])
        .current_dir(workdir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn trunk_reached_remote(workdir: &Path, trunk: &str, remote_oid: Option<&str>) -> bool {
    remote_oid.is_some_and(|remote| resolve_ref_oid(workdir, trunk).as_deref() == Some(remote))
}

fn deleted_tip_suffix(tip: Option<&str>) -> String {
    match tip {
        Some(sha) if !sha.is_empty() => format!(" · {}", &sha[..sha.len().min(7)])
            .dimmed()
            .to_string(),
        _ => String::new(),
    }
}

fn restore_stashed_changes(repo: &GitRepo, stashed: bool, quiet: bool) -> Result<()> {
    if stashed {
        repo.stash_pop()?;
        if !quiet {
            println!("{}", "✓ Restored stashed changes.".green());
        }
    }
    Ok(())
}

pub(super) fn is_ancestor(workdir: &Path, ancestor: &str, descendant: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(workdir)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(super) fn count_commits_between(workdir: &Path, base: &str, head: &str) -> Result<usize> {
    let output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", base, head)])
        .current_dir(workdir)
        .output()
        .context("Failed to count fetched trunk commits")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git rev-list failed while counting trunk commits: {}",
            stderr.trim()
        );
    }

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .context("Failed to parse fetched trunk commit count")
}

pub(super) fn diff_line_stats_between(
    workdir: &Path,
    base: &str,
    head: &str,
) -> Result<(usize, usize, usize)> {
    let range = format!("{}..{}", base, head);
    let output = Command::new("git")
        .args([
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--shortstat",
            &range,
        ])
        .env("LC_ALL", "C")
        .current_dir(workdir)
        .output()
        .context("Failed to calculate fetched trunk line stats")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git diff failed while calculating trunk stats: {}",
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_diff_shortstat(&stdout)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse git diff --shortstat output: {stdout:?}"))
}

fn parse_diff_shortstat(output: &str) -> Option<(usize, usize, usize)> {
    if output.trim().is_empty() {
        return Some((0, 0, 0));
    }

    let mut files = 0;
    let mut additions = 0;
    let mut deletions = 0;
    let mut recognized = false;

    for part in output.trim().split(',').map(str::trim) {
        let value = part.split_whitespace().next()?.parse::<usize>().ok()?;
        if part.contains("file changed") || part.contains("files changed") {
            files = value;
            recognized = true;
        } else if part.contains("insertion") {
            additions = value;
            recognized = true;
        } else if part.contains("deletion") {
            deletions = value;
            recognized = true;
        }
    }

    recognized.then_some((files, additions, deletions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use colored::control;
    use std::path::PathBuf;

    fn linked_worktree_cleanup(blockers: &[&'static str]) -> BlockingWorktreeCleanup {
        BlockingWorktreeCleanup {
            resolution: crate::git::repo::BranchDeleteResolution {
                worktree: crate::git::repo::WorktreeInfo {
                    name: "review-pass".to_string(),
                    path: PathBuf::from("/tmp/review-pass"),
                    branch: Some("cesar/review-pass".to_string()),
                    is_main: false,
                    is_current: false,
                    is_locked: false,
                    lock_reason: None,
                    is_prunable: false,
                    prunable_reason: None,
                },
                remove_worktree_selector: "cesar/review-pass".to_string(),
                switch_target: crate::git::repo::BranchDeleteSwitchTarget::Detach,
            },
            blockers: blockers.to_vec(),
        }
    }

    #[test]
    fn render_sync_footer_is_colored_and_compact() {
        control::set_override(true);

        let footer = render_sync_footer(
            &SyncStats {
                trunk: Some(TrunkSummary::Pulled {
                    branch: "main".to_string(),
                    commits: 1,
                    files: 12,
                    additions: 434,
                    deletions: 22,
                }),
                merged_branches_cleaned: 2,
                restacked_branches: vec!["feat".to_string()],
                imported_branches_updated: vec!["base".to_string()],
                ..SyncStats::default()
            },
            Duration::from_millis(14_022),
        );

        control::unset_override();

        assert!(footer.contains("main"));
        assert!(footer.contains("+1 commit"));
        assert!(footer.contains("12 files"));
        assert!(footer.contains("+434"));
        assert!(footer.contains("-22"));
        assert!(footer.contains("cleaned"));
        assert!(footer.contains("restacked"));
        assert!(footer.contains("updated"));
        assert!(footer.contains("1 imported"));
        assert!(footer.contains("14.022s"));
        assert!(footer.contains('\u{1b}'));
    }

    #[test]
    fn render_sync_footer_handles_up_to_date_branch() {
        control::set_override(true);

        let footer = render_sync_footer(
            &SyncStats {
                trunk: Some(TrunkSummary::UpToDate {
                    branch: "main".to_string(),
                }),
                merged_branches_cleaned: 0,
                restacked_branches: vec![],
                imported_branches_updated: vec![],
                ..SyncStats::default()
            },
            Duration::from_secs(2),
        );

        control::unset_override();

        assert!(footer.contains("main"));
        assert!(footer.contains("up to date"));
        assert!(footer.contains("2.000s"));
        assert!(footer.contains('\u{1b}'));
    }

    #[test]
    fn render_sync_follow_up_is_empty_when_sync_needs_no_attention() {
        assert!(render_sync_follow_up(&SyncStats::default()).is_empty());
    }

    #[test]
    fn render_sync_follow_up_prioritizes_diverged_trunk_recovery() {
        let lines = render_sync_follow_up(&SyncStats {
            trunk_not_updated: Some(TrunkNotUpdated {
                branch: "main".to_string(),
                remote_ref: "origin/main".to_string(),
                failure: TrunkUpdateFailure::Diverged,
            }),
            cleanup_skips: vec![CleanupSkip {
                branch: "old-auth".to_string(),
                reason: "dirty worktree".to_string(),
            }],
            checkout_change: Some(CheckoutChange {
                from: "old-auth".to_string(),
                to: "main".to_string(),
            }),
            ..SyncStats::default()
        });
        let output = lines.join("\n");

        assert!(output.contains("main diverged from origin/main"));
        assert!(output.contains("Cleanup skipped for old-auth (dirty worktree)"));
        assert!(output.contains("Checked out main after cleanup (was old-auth)"));
        assert!(output.contains("Next: inspect and reconcile main with origin/main"));
        assert!(!output.contains("Next: st trunk"));
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with("Next:"))
                .count(),
            1
        );
    }

    #[test]
    fn render_sync_follow_up_suggests_trunk_for_non_divergence_failures() {
        let lines = render_sync_follow_up(&SyncStats {
            trunk_not_updated: Some(TrunkNotUpdated {
                branch: "main".to_string(),
                remote_ref: "origin/main".to_string(),
                failure: TrunkUpdateFailure::Other,
            }),
            ..SyncStats::default()
        });
        let output = lines.join("\n");

        assert!(output.contains("main did not reach origin/main"));
        assert!(output.contains("Next: st trunk"));
    }

    #[test]
    fn render_sync_follow_up_suggests_sweep_for_skipped_cleanup() {
        let lines = render_sync_follow_up(&SyncStats {
            cleanup_skips: vec![CleanupSkip {
                branch: "old-auth".to_string(),
                reason: "dirty worktree".to_string(),
            }],
            ..SyncStats::default()
        });

        let output = lines.join("\n");

        assert!(output.contains("Next: st sweep"));
        assert!(!output.contains("Next: st restack --all"));
    }

    #[test]
    fn waits_for_slow_trunk_summary_instead_of_dropping_it() {
        let worker = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(1_100));
            Ok(Some(TrunkSummary::Pulled {
                branch: "main".to_string(),
                commits: 108,
                files: 752,
                additions: 30_954,
                deletions: 5_422,
            }))
        });

        let summary = wait_for_trunk_summary(worker)
            .expect("trunk summary worker should finish")
            .expect("trunk summary should be retained");

        assert!(matches!(
            summary,
            TrunkSummary::Pulled {
                commits: 108,
                files: 752,
                additions: 30_954,
                deletions: 5_422,
                ..
            }
        ));
    }

    #[test]
    fn reports_trunk_summary_worker_failure() {
        let worker = std::thread::spawn(|| -> Result<Option<TrunkSummary>> {
            panic!("simulated worker failure")
        });

        let error = wait_for_trunk_summary(worker).expect_err("worker failure should be reported");

        assert!(error.to_string().contains("trunk summary worker panicked"));
    }

    #[test]
    fn parses_diff_shortstat_line_counts() {
        assert_eq!(
            parse_diff_shortstat(" 752 files changed, 30954 insertions(+), 5422 deletions(-)\n"),
            Some((752, 30_954, 5_422))
        );
    }

    #[test]
    fn parses_diff_shortstat_with_only_insertions() {
        assert_eq!(
            parse_diff_shortstat(" 1 file changed, 7 insertions(+)\n"),
            Some((1, 7, 0))
        );
    }

    #[test]
    fn parses_empty_diff_shortstat_as_zero_changes() {
        assert_eq!(parse_diff_shortstat(""), Some((0, 0, 0)));
    }

    #[test]
    fn rejects_malformed_diff_shortstat() {
        assert_eq!(parse_diff_shortstat("not git diff output"), None);
    }

    #[test]
    fn linked_worktree_delete_options_default_to_preserve() {
        let options = linked_worktree_delete_options(&linked_worktree_cleanup(&[]));

        assert_eq!(options[0].1, SyncBranchDeleteAction::PreserveWorktree);
        assert!(options[0].0.contains("Keep worktree"));
        assert_eq!(
            options[1].1,
            SyncBranchDeleteAction::RemoveWorktree { force: false }
        );
        assert_eq!(
            options.last().expect("skip option").1,
            SyncBranchDeleteAction::Skip
        );
    }

    #[test]
    fn sync_delete_prompt_prefers_checkout_wording_for_current_branch() {
        let prompt = sync_delete_prompt(
            "cesar/review-pass",
            Some("main"),
            Some("upstream gone"),
            Some(&linked_worktree_cleanup(&[])),
        );

        assert_eq!(
            prompt,
            "Delete 'cesar/review-pass' (upstream gone) and checkout 'main'?"
        );
    }

    #[test]
    fn linked_worktree_delete_options_label_dirty_removal_as_destructive() {
        let options = linked_worktree_delete_options(&linked_worktree_cleanup(&["dirty"]));

        assert_eq!(
            options[1].1,
            SyncBranchDeleteAction::RemoveWorktree { force: true }
        );
        assert!(
            options[1]
                .0
                .contains("Force-remove dirty worktree and delete branch")
        );
    }

    #[test]
    fn linked_worktree_delete_options_omit_remove_for_locked_worktree() {
        let options = linked_worktree_delete_options(&linked_worktree_cleanup(&["locked"]));

        assert_eq!(options.len(), 2);
        assert!(
            options.iter().all(|(_, action)| !matches!(
                action,
                SyncBranchDeleteAction::RemoveWorktree { .. }
            ))
        );
    }

    #[test]
    fn linked_worktree_delete_options_omit_remove_for_main_worktree() {
        let mut cleanup = linked_worktree_cleanup(&[]);
        cleanup.resolution.worktree.is_main = true;
        let options = linked_worktree_delete_options(&cleanup);

        assert_eq!(options.len(), 2);
        assert!(
            options.iter().all(|(_, action)| !matches!(
                action,
                SyncBranchDeleteAction::RemoveWorktree { .. }
            ))
        );
    }

    #[test]
    fn deleted_tip_suffix_renders_seven_char_short_sha() {
        colored::control::set_override(false);
        let sha = "abcdef1234567890";
        let suffix = deleted_tip_suffix(Some(sha));
        assert!(
            suffix.contains("abcdef1"),
            "expected 7-char sha in: {suffix}"
        );
        assert!(suffix.contains(" · "), "expected separator in: {suffix}");
        assert!(
            !suffix.contains("234567"),
            "suffix must not exceed 7 chars of sha: {suffix}"
        );
    }

    #[test]
    fn deleted_tip_suffix_is_empty_without_a_tip() {
        colored::control::set_override(false);
        assert_eq!(deleted_tip_suffix(None), "");
        assert_eq!(deleted_tip_suffix(Some("")), "");
    }

    #[test]
    fn stale_stack_warning_names_consequence_and_error() {
        colored::control::set_override(false);
        let error = anyhow::anyhow!("metadata ref not found");
        let msg = stale_stack_warning("using snapshot from sync start", &error);
        assert!(msg.contains("using snapshot from sync start"));
        assert!(msg.contains("metadata ref not found"));
        assert!(msg.contains("⚠"));
    }

    #[test]
    fn stash_left_behind_warning_tells_user_how_to_recover() {
        colored::control::set_override(false);
        let msg = stash_left_behind_warning();
        assert!(
            msg.contains("stax auto-stash"),
            "must name the stash: {msg}"
        );
        assert!(
            msg.contains("git stash pop"),
            "must tell user how to restore: {msg}"
        );
        assert!(
            msg.contains("git stash list"),
            "must tell user how to inspect: {msg}"
        );
    }

    #[test]
    fn stash_policy_from_flags_returns_never_when_no_stash() {
        assert_eq!(StashPolicy::from_flags(false, true), StashPolicy::Never);
    }

    #[test]
    fn stash_policy_from_flags_returns_always_when_stash() {
        assert_eq!(StashPolicy::from_flags(true, false), StashPolicy::Always);
    }

    #[test]
    fn stash_policy_from_flags_returns_prompt_when_neither() {
        assert_eq!(StashPolicy::from_flags(false, false), StashPolicy::Prompt);
    }

    #[test]
    fn stash_policy_from_flags_no_stash_takes_precedence() {
        // clap enforces conflicts_with, but belt-and-suspenders: Never when no_stash=true
        assert_eq!(StashPolicy::from_flags(false, true), StashPolicy::Never);
    }

    #[test]
    fn merge_planned_merged_detection_unions_planned_and_fresh() {
        let planned = vec![MergedBranchInfo {
            branch: "squash-only".to_string(),
            merge_type: MergeType::SquashMerge,
        }];
        let fresh = vec![MergedBranchInfo {
            branch: "ancestor".to_string(),
            merge_type: MergeType::Ancestor,
        }];
        let merged = super::merge_planned_merged_detection(planned, fresh);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|m| m.branch == "squash-only"));
        assert!(merged.iter().any(|m| m.branch == "ancestor"));
    }

    #[test]
    fn prune_deprecation_warning_names_both_flags() {
        colored::control::set_override(false);
        let msg = prune_deprecation_warning();
        assert!(msg.contains("--prune"), "must name --prune: {msg}");
        assert!(msg.contains("--full"), "must name --full: {msg}");
    }
}
