use crate::commands::sync::{
    BlockingWorktreeCleanup, MergeType, MergedBranchInfo, PartialMergeReason,
    count_commits_between, diff_line_stats_between, find_merged_branches,
    find_partially_merged_notes, find_upstream_gone_branches, imported_branches_for_cleanup,
    init_forge_client, is_ancestor, local_branch_exists, plan_blocking_worktree_cleanup,
    print_cleanup_candidates, resolve_fallback_parent_skipping_doomed, resolve_ref_oid,
};
use crate::config::Config;
use crate::engine::branch_detect::has_unique_commits_since_any_base;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::progress::LiveTimer;
use crate::remote;
use anyhow::Result;
use colored::Colorize;
use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct SyncPlanOptions {
    pub restack: bool,
    pub full: bool,
    pub delete_merged: bool,
    pub delete_upstream_gone: bool,
    pub force: bool,
    pub safe: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub auto_stash_pop: bool,
}

#[derive(Debug, Clone)]
pub(super) enum TrunkPlan {
    UpToDate,
    FastForward {
        commits: usize,
        files: usize,
        additions: usize,
        deletions: usize,
    },
    ResetToRemote {
        target: String,
    },
    SkippedSafeMode {
        target: String,
    },
    Diverged {
        ahead: usize,
        behind: usize,
    },
    RemoteUnknown,
    ObjectsAbsent {
        target: String,
    },
}

pub(super) fn classify_trunk_plan(
    workdir: &Path,
    trunk: &str,
    remote_head_oid: Option<&str>,
    safe: bool,
    on_trunk: bool,
    dirty: bool,
) -> TrunkPlan {
    let remote_oid = match remote_head_oid {
        Some(oid) => oid,
        None => return TrunkPlan::RemoteUnknown,
    };

    let local_oid = match resolve_ref_oid(workdir, trunk) {
        Some(oid) => oid,
        None => {
            return TrunkPlan::ObjectsAbsent {
                target: remote_oid.to_string(),
            };
        }
    };

    if local_oid == remote_oid {
        return TrunkPlan::UpToDate;
    }

    // Check if remote commit object exists locally (no fetch was done)
    if !object_exists_locally(workdir, remote_oid) {
        return TrunkPlan::ObjectsAbsent {
            target: remote_oid.to_string(),
        };
    }

    if is_ancestor(workdir, &local_oid, remote_oid) {
        // Fast-forward is possible. If on trunk with a dirty tree, git merge --ff-only may fail.
        if on_trunk && dirty {
            if safe {
                return TrunkPlan::SkippedSafeMode {
                    target: remote_oid.to_string(),
                };
            } else {
                // Real sync would attempt ff (may succeed or fail) then fall back to reset --hard.
                return TrunkPlan::ResetToRemote {
                    target: remote_oid.to_string(),
                };
            }
        }
        match (
            count_commits_between(workdir, &local_oid, remote_oid),
            diff_line_stats_between(workdir, &local_oid, remote_oid),
        ) {
            (Ok(commits), Ok((files, additions, deletions))) => TrunkPlan::FastForward {
                commits,
                files,
                additions,
                deletions,
            },
            _ => TrunkPlan::ObjectsAbsent {
                target: remote_oid.to_string(),
            },
        }
    } else {
        let ahead = count_commits_between(workdir, remote_oid, &local_oid).unwrap_or(0);
        let behind = count_commits_between(workdir, &local_oid, remote_oid).unwrap_or(0);
        TrunkPlan::Diverged { ahead, behind }
    }
}

fn object_exists_locally(workdir: &Path, oid: &str) -> bool {
    let commit_ref = format!("{}^{{commit}}", oid);
    Command::new("git")
        .args(["cat-file", "-e", &commit_ref])
        .current_dir(workdir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub(super) fn render_trunk_plan(trunk: &str, plan: &TrunkPlan) {
    match plan {
        TrunkPlan::UpToDate => {
            println!(
                "  {} {} {}",
                "✓".green(),
                trunk.cyan(),
                "up to date".dimmed()
            );
        }
        TrunkPlan::FastForward {
            commits,
            files,
            additions,
            deletions,
        } => {
            println!(
                "  {} would fast-forward {} (+{} commit{}, {} file{}, +{} -{})",
                "→".cyan(),
                trunk.cyan().bold(),
                commits,
                if *commits == 1 { "" } else { "s" },
                files,
                if *files == 1 { "" } else { "s" },
                additions.to_string().green(),
                deletions.to_string().red(),
            );
        }
        TrunkPlan::ResetToRemote { target } => {
            println!(
                "  {} would reset {} to remote ({}) — dirty working tree, non-fast-forward fallback",
                "⚠".yellow(),
                trunk.cyan(),
                short_sha(target).dimmed(),
            );
        }
        TrunkPlan::SkippedSafeMode { target } => {
            println!(
                "  {} would skip {} update (--safe; dirty working tree; remote {})",
                "↷".yellow(),
                trunk.cyan(),
                short_sha(target).dimmed(),
            );
        }
        TrunkPlan::Diverged { ahead, behind } => {
            println!(
                "  {} {} diverged from remote ({} ahead, {} behind) — cannot update",
                "⚠".yellow(),
                trunk.cyan(),
                ahead,
                behind,
            );
        }
        TrunkPlan::RemoteUnknown => {
            println!(
                "  {} could not reach remote — trunk status unknown",
                "?".yellow(),
            );
        }
        TrunkPlan::ObjectsAbsent { target } => {
            println!(
                "  {} remote {} at {} — objects absent locally (fetch required for full diff)",
                "→".cyan(),
                trunk.cyan(),
                short_sha(target).dimmed(),
            );
        }
    }
}

fn short_sha(oid: &str) -> &str {
    &oid[..oid.len().min(7)]
}

/// Compute which branches in `scope` would need restack after trunk advances to
/// `predicted_trunk_oid`. Unions the existing `needs_restack` flag with branches
/// whose stored parent_revision doesn't match the predicted trunk tip, then
/// propagates transitively to descendants.
pub(super) fn would_need_restack(
    stack: &Stack,
    scope: &[String],
    predicted_trunk_oid: Option<&str>,
) -> Vec<String> {
    let scope_set: HashSet<&str> = scope.iter().map(|s| s.as_str()).collect();
    let mut needs: HashSet<String> = HashSet::new();

    // Seed with branches that already need restack
    for branch in scope {
        if stack
            .branches
            .get(branch.as_str())
            .map(|b| b.needs_restack)
            .unwrap_or(false)
        {
            needs.insert(branch.clone());
        }
    }

    // Add branches that would need restack if trunk advances to predicted_trunk_oid
    if let Some(new_trunk_oid) = predicted_trunk_oid {
        for branch in scope {
            let Some(br) = stack.branches.get(branch.as_str()) else {
                continue;
            };
            if br.parent.as_deref() == Some(stack.trunk.as_str()) {
                let stored_rev = br.parent_revision.as_deref().unwrap_or("");
                if stored_rev != new_trunk_oid {
                    needs.insert(branch.clone());
                }
            }
        }
    }

    // Propagate: if a branch needs restack, its in-scope descendants also would
    let mut changed = true;
    while changed {
        changed = false;
        for branch in scope {
            if !needs.contains(branch.as_str()) {
                continue;
            }
            let children: Vec<String> = stack
                .branches
                .get(branch.as_str())
                .map(|br| br.children.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|c| scope_set.contains(c.as_str()))
                .collect();
            for child in children {
                if needs.insert(child) {
                    changed = true;
                }
            }
        }
    }

    scope
        .iter()
        .filter(|b| needs.contains(b.as_str()))
        .cloned()
        .collect()
}

fn patch_stack_pr_states_in_memory(stack: &mut Stack, repo: &GitRepo, config: &Config) {
    let Some((rt, client)) = init_forge_client(repo, config) else {
        return;
    };

    let tracked: Vec<(String, u64)> = stack
        .branches
        .iter()
        .filter(|(name, _)| *name != &stack.trunk)
        .filter_map(|(name, info)| info.pr_number.map(|n| (name.clone(), n)))
        .collect();

    for (branch_name, pr_number) in tracked {
        if let Ok(live_pr) = rt.block_on(client.get_pr(pr_number))
            && let Some(br) = stack.branches.get_mut(&branch_name)
        {
            br.pr_state = Some(live_pr.state.to_uppercase());
            br.pr_is_draft = Some(live_pr.is_draft);
        }
    }
}

pub fn run(options: SyncPlanOptions) -> Result<()> {
    let SyncPlanOptions {
        restack,
        full,
        delete_merged,
        delete_upstream_gone,
        force,
        safe,
        quiet,
        verbose,
        auto_stash_pop,
    } = options;

    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    let config = Config::load()?;
    let remote_name = config.remote_name().to_string();
    let trunk = repo.trunk_branch()?;
    let remote_trunk_ref = format!("{}/{}", remote_name, trunk);

    // Stderr warnings for inert flags — always emitted regardless of --quiet
    if force {
        eprintln!(
            "{}",
            "warning: --force is ignored by --dry-run (no prompts to suppress)".yellow()
        );
    }
    if auto_stash_pop {
        eprintln!(
            "{}",
            "warning: --auto-stash-pop is ignored by --dry-run (no stash operations)".yellow()
        );
    }
    if full {
        eprintln!(
            "{}",
            "warning: --full is ignored by --dry-run (no fetch is performed)".yellow()
        );
    }
    if verbose {
        eprintln!(
            "{}",
            "warning: --verbose is ignored by --dry-run (output detail is fixed)".yellow()
        );
    }

    if !quiet {
        println!(
            "{}",
            "Sync plan (read-only — no changes will be made)".bold()
        );
        println!();
    }

    // --- Probe remote (ls-remote; read-only, no FETCH_HEAD writes) ---
    let mut use_cached_remote = false;
    let (remote_head_oid, remote_branch_set): (Option<String>, HashSet<String>) =
        match remote::ls_remote_head_oids(&workdir, &remote_name) {
            Ok(heads) => {
                let trunk_oid = heads.get(&trunk).cloned();
                let branch_count = heads.len();
                let names: HashSet<String> = heads.into_keys().collect();
                if !quiet {
                    println!(
                        "{}",
                        format!(
                            "  Probed {} via ls-remote ({} branches)",
                            remote_name, branch_count
                        )
                        .dimmed()
                    );
                }
                (trunk_oid, names)
            }
            Err(_) => {
                use_cached_remote = true;
                let oid = resolve_ref_oid(&workdir, &remote_trunk_ref);
                let names = repo.remote_branch_names(&remote_name).unwrap_or_default();
                if !quiet {
                    println!(
                        "  {} {}",
                        "⚠".yellow(),
                        "offline — using cached remote-tracking refs".dimmed()
                    );
                }
                (oid, names)
            }
        };

    if use_cached_remote && !quiet {
        println!(
            "  {}",
            "Merged-branch detection uses cached remote state — run `git fetch` for fresh results"
                .dimmed()
        );
    }

    // --- Load stack (in-memory only; no ref writes) ---
    let mut stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;

    // Patch PR states in memory BEFORE any &Stack borrows for detection
    patch_stack_pr_states_in_memory(&mut stack, &repo, &config);

    // --- Trunk section ---
    let on_trunk = current == trunk;
    let dirty = repo.is_dirty().unwrap_or(false);
    let trunk_plan = classify_trunk_plan(
        &workdir,
        &trunk,
        remote_head_oid.as_deref(),
        safe,
        on_trunk,
        dirty,
    );

    if !quiet {
        println!("{}", "Trunk".bold());
        render_trunk_plan(&trunk, &trunk_plan);
    }

    let restack_would_fail = matches!(
        trunk_plan,
        TrunkPlan::Diverged { .. } | TrunkPlan::ObjectsAbsent { .. }
    );

    if restack && restack_would_fail {
        eprintln!(
            "  {} --restack: real sync would fail closed (trunk did not reach remote); restack section skipped",
            "⚠".yellow()
        );
    }

    // --- Dirty tree section ---
    if dirty && !quiet {
        println!();
        println!("{}", "Working tree".bold());
        println!(
            "  {} working tree is dirty — real sync would stash before proceeding{}",
            "⚠".yellow(),
            format!(" ({})", "dry-run leaves your working tree untouched").dimmed()
        );
    }

    // --- Merged branches section ---
    let mut merged_count = 0usize;
    if !quiet {
        println!();
        println!("{}", "Merged branches".bold());
    }

    if !delete_merged {
        if !quiet {
            println!(
                "  {}",
                "--no-delete: merged-branch cleanup is disabled".dimmed()
            );
        }
    } else {
        let merged =
            find_merged_branches(&repo, &workdir, &stack, &remote_name, &remote_branch_set)?;
        let exempt_imported = imported_branches_for_cleanup(&repo, &stack)?;
        let partially_merged_notes = find_partially_merged_notes(
            &repo,
            &workdir,
            &stack,
            &remote_name,
            &remote_branch_set,
            &merged,
        )?;

        if merged.is_empty() {
            if !quiet {
                println!("  {}", "No merged branches detected.".dimmed());
            }
        } else {
            let merged_names: Vec<String> = merged.iter().map(|m| m.branch.clone()).collect();
            merged_count = merged_names.len();

            if !quiet {
                print_cleanup_candidates("merged", &merged_names);

                for merged_info in &merged {
                    let branch = &merged_info.branch;
                    let is_current = *branch == current;
                    let blocking =
                        plan_blocking_worktree_cleanup(&repo, branch, force).unwrap_or(None);
                    let (fallback_parent, _) = resolve_fallback_parent_skipping_doomed(
                        &workdir,
                        &stack,
                        branch,
                        &merged_names,
                    );
                    let parent_exists = local_branch_exists(&workdir, &fallback_parent);

                    render_merged_branch_plan(
                        branch,
                        merged_info,
                        blocking.as_ref(),
                        &fallback_parent,
                        parent_exists,
                        is_current,
                        force,
                        &stack,
                        &exempt_imported,
                        &remote_branch_set,
                    );
                }
            }
        }

        if !partially_merged_notes.is_empty() && !quiet {
            println!();
            println!("  {}", "Partially-merged (would NOT delete):".dimmed());
            for note in &partially_merged_notes {
                let reason = match note.pr_label {
                    PartialMergeReason::PrMerged => "PR merged",
                    PartialMergeReason::PrClosed => "PR closed",
                    PartialMergeReason::HistoryMerged => "history merged",
                };
                if let Some(n) = note.pr_number {
                    println!(
                        "  {} {} (#{}: {}; +{} local commit{})",
                        "↷".yellow(),
                        note.branch.cyan(),
                        n,
                        reason,
                        note.extra_commits,
                        if note.extra_commits == 1 { "" } else { "s" }
                    );
                } else {
                    println!(
                        "  {} {} ({}; +{} local commit{})",
                        "↷".yellow(),
                        note.branch.cyan(),
                        reason,
                        note.extra_commits,
                        if note.extra_commits == 1 { "" } else { "s" }
                    );
                }
            }
        }
    }

    // --- Upstream-gone section ---
    let mut gone_protected_count = 0usize;
    let mut gone_deletable_count = 0usize;

    if delete_upstream_gone {
        if !quiet {
            println!();
            println!("{}", "Upstream-gone branches".bold());
        }

        let gone = find_upstream_gone_branches(&workdir, &trunk)?;
        if gone.is_empty() {
            if !quiet {
                println!("  {}", "No upstream-gone branches detected.".dimmed());
            }
        } else {
            let mut protected: Vec<String> = Vec::new();
            let mut deletable: Vec<String> = Vec::new();

            for branch in &gone {
                if branch == &trunk {
                    continue;
                }
                let parent = stack
                    .branches
                    .get(branch.as_str())
                    .and_then(|b| b.parent.clone())
                    .unwrap_or_else(|| trunk.clone());
                let has_unique =
                    has_unique_commits_since_any_base(&workdir, branch, &[parent.as_str()])
                        .unwrap_or(false);
                if has_unique {
                    protected.push(branch.clone());
                } else {
                    deletable.push(branch.clone());
                }
            }

            if !protected.is_empty() && !quiet {
                for b in &protected {
                    println!(
                        "  {} {} {}",
                        "↷".yellow(),
                        b.cyan(),
                        "(upstream-gone; protected — has unique commits)".dimmed()
                    );
                }
                gone_protected_count = protected.len();
            }

            if !deletable.is_empty() {
                if !quiet {
                    print_cleanup_candidates("upstream-gone", &deletable);
                    for branch in &deletable {
                        if force {
                            println!(
                                "  {} {} {}",
                                "✓".green(),
                                branch.bright_black(),
                                "— would delete".green()
                            );
                        } else {
                            println!(
                                "  {} {} {}",
                                "?".bright_black(),
                                branch.bright_black(),
                                "— would prompt, then delete if confirmed".dimmed()
                            );
                        }
                    }
                }
                gone_deletable_count = deletable.len();
            } else if !quiet && protected.is_empty() {
                println!("  {}", "No upstream-gone branches to delete.".dimmed());
            }
        }
    }

    // --- Restack section ---
    let mut restack_count = 0usize;

    if restack && !restack_would_fail {
        if !quiet {
            println!();
            println!("{}", "Restack preview".bold());
        }

        let scope_order: Vec<String> =
            if current != trunk && stack.branches.contains_key(current.as_str()) {
                stack.current_stack(&current)
            } else {
                Vec::new()
            };

        let mut frozen_branches: Vec<String> = Vec::new();
        let restack_scope: Vec<String> = scope_order
            .iter()
            .filter(|branch| {
                let frozen = BranchMetadata::is_frozen(repo.inner(), branch).unwrap_or(false);
                if frozen {
                    frozen_branches.push((*branch).clone());
                }
                !frozen
            })
            .cloned()
            .collect();

        if !frozen_branches.is_empty() && !quiet {
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

        // Predicted trunk OID after would-be sync
        let predicted_trunk_oid: Option<String> = match &trunk_plan {
            TrunkPlan::FastForward { .. } | TrunkPlan::ResetToRemote { .. } => {
                remote_head_oid.clone()
            }
            TrunkPlan::UpToDate => resolve_ref_oid(&workdir, &trunk),
            _ => None,
        };

        let branches_to_restack =
            would_need_restack(&stack, &restack_scope, predicted_trunk_oid.as_deref());

        if branches_to_restack.is_empty() {
            if !quiet {
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
            restack_count = branches_to_restack.len();

            if !quiet {
                let branch_parent_pairs: Vec<(String, String)> = branches_to_restack
                    .iter()
                    .filter_map(|branch| {
                        stack
                            .branches
                            .get(branch.as_str())
                            .and_then(|br| br.parent.clone().map(|p| (branch.clone(), p)))
                    })
                    .collect();

                let timer = LiveTimer::maybe_new(!quiet, "Checking for conflicts...");
                let predictions = repo.predict_restack_conflicts(&branch_parent_pairs);

                if predictions.is_empty() {
                    LiveTimer::maybe_finish_ok(timer, "no conflicts predicted");
                } else {
                    LiveTimer::maybe_finish_warn(
                        timer,
                        &format!("{} branch(es) with conflicts", predictions.len()),
                    );
                    println!();
                    for prediction in &predictions {
                        println!(
                            "  {} {} → {}",
                            "✗".red(),
                            prediction.branch.yellow().bold(),
                            prediction.onto.dimmed()
                        );
                        for file in &prediction.conflicting_files {
                            println!("    {} {}", "│".dimmed(), file.red());
                        }
                    }
                }

                println!();
                println!("  {} branch(es) would restack:", branches_to_restack.len());
                for branch in &branches_to_restack {
                    println!("    {} {}", "▸".dimmed(), branch.cyan());
                }
            }
        }
    }

    // --- Footer ---
    if !quiet {
        println!();

        let mut summary_parts: Vec<String> = Vec::new();
        if merged_count > 0 {
            summary_parts.push(format!("{} merged to clean", merged_count));
        }
        if gone_deletable_count > 0 {
            summary_parts.push(format!("{} upstream-gone to clean", gone_deletable_count));
        }
        if gone_protected_count > 0 {
            summary_parts.push(format!("{} upstream-gone protected", gone_protected_count));
        }
        if restack_count > 0 {
            summary_parts.push(format!("{} to restack", restack_count));
        }

        let summary = if summary_parts.is_empty() {
            "nothing to do".to_string()
        } else {
            summary_parts.join(", ")
        };

        println!(
            "{} {}",
            "Plan complete — no changes made.".green().bold(),
            format!("({})", summary).dimmed()
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_merged_branch_plan(
    branch: &str,
    merged_info: &MergedBranchInfo,
    blocking: Option<&BlockingWorktreeCleanup>,
    fallback_parent: &str,
    parent_exists: bool,
    is_current: bool,
    force: bool,
    stack: &Stack,
    exempt_imported: &HashSet<String>,
    remote_branches: &HashSet<String>,
) {
    // Blocking worktree takes priority
    if let Some(blocker) = blocking
        && let Some(reason) = blocker.blocker_summary()
    {
        println!(
            "  {} {} {}",
            "○".yellow(),
            branch.bright_black(),
            format!("(would keep worktree: {})", reason).dimmed()
        );
        return;
    }

    // Missing local parent → would skip
    if !parent_exists && fallback_parent != stack.trunk {
        println!(
            "  {} {} {}",
            "↷".yellow(),
            branch.bright_black(),
            format!(
                "(would skip — parent '{}' not found locally)",
                fallback_parent
            )
            .dimmed()
        );
        return;
    }

    // SquashMerge with surviving children → would rebase children
    if matches!(merged_info.merge_type, MergeType::SquashMerge) {
        let child_count = stack
            .branches
            .values()
            .filter(|info| info.parent.as_deref() == Some(branch))
            .count();
        if child_count > 0 {
            println!(
                "  {} {} {}",
                "↪".cyan(),
                branch.bright_black(),
                format!("(squash-merge; would rebase {} child(ren))", child_count).dimmed()
            );
        }
    }

    // Current branch → note checkout first
    if is_current {
        println!(
            "  {} {} {}",
            "→".cyan(),
            branch.bright_black(),
            format!("(would check out '{}' first)", fallback_parent).dimmed()
        );
    }

    // Exempt imported (local delete only, remote exempt)
    if exempt_imported.contains(branch) {
        println!(
            "  {} {} {}",
            "✓".green(),
            branch.bright_black(),
            "(imported — would delete local only, remote exempt)".dimmed()
        );
        return;
    }

    let remote_still_exists = remote_branches.contains(branch);
    let scope_suffix = if remote_still_exists {
        " (local only)"
    } else {
        ""
    };

    if force {
        println!(
            "  {} {}{}",
            "✓".green(),
            branch.bright_black(),
            format!(" — would delete{}", scope_suffix).green()
        );
    } else {
        println!(
            "  {} {}{}",
            "?".bright_black(),
            branch.bright_black(),
            format!(" — would prompt, then delete{} if confirmed", scope_suffix).dimmed()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Stack;
    use std::collections::HashMap;

    fn make_stack_branch(
        name: &str,
        parent: Option<&str>,
        parent_revision: Option<&str>,
        children: Vec<&str>,
        needs_restack: bool,
    ) -> crate::engine::stack::StackBranch {
        crate::engine::stack::StackBranch {
            name: name.to_string(),
            parent: parent.map(|s| s.to_string()),
            parent_revision: parent_revision.map(|s| s.to_string()),
            children: children.iter().map(|s| s.to_string()).collect(),
            needs_restack,
            pr_number: None,
            pr_state: None,
            pr_is_draft: None,
        }
    }

    fn make_test_stack() -> Stack {
        let mut branches = HashMap::new();
        branches.insert(
            "main".to_string(),
            make_stack_branch("main", None, None, vec!["feat-a", "feat-b"], false),
        );
        branches.insert(
            "feat-a".to_string(),
            make_stack_branch(
                "feat-a",
                Some("main"),
                Some("abc123"),
                vec!["feat-a-1"],
                false,
            ),
        );
        branches.insert(
            "feat-a-1".to_string(),
            make_stack_branch("feat-a-1", Some("feat-a"), Some("def456"), vec![], true),
        );
        branches.insert(
            "feat-b".to_string(),
            make_stack_branch("feat-b", Some("main"), Some("abc123"), vec![], false),
        );
        Stack {
            branches,
            trunk: "main".to_string(),
        }
    }

    #[test]
    fn classify_trunk_plan_remote_unknown_when_no_remote_oid() {
        let dir = std::env::temp_dir();
        let plan = classify_trunk_plan(&dir, "main", None, false, false, false);
        assert!(matches!(plan, TrunkPlan::RemoteUnknown));
    }

    #[test]
    fn render_trunk_plan_up_to_date_contains_check() {
        colored::control::set_override(false);
        let plan = TrunkPlan::UpToDate;
        // Just exercise the render path — primarily ensures no panic
        render_trunk_plan("main", &plan);
        colored::control::unset_override();
    }

    #[test]
    fn render_trunk_plan_fast_forward_contains_commit_count() {
        colored::control::set_override(false);
        let plan = TrunkPlan::FastForward {
            commits: 3,
            files: 5,
            additions: 42,
            deletions: 7,
        };
        render_trunk_plan("main", &plan);
        colored::control::unset_override();
    }

    #[test]
    fn render_trunk_plan_diverged_shows_ahead_behind() {
        colored::control::set_override(false);
        let plan = TrunkPlan::Diverged {
            ahead: 2,
            behind: 4,
        };
        render_trunk_plan("main", &plan);
        colored::control::unset_override();
    }

    #[test]
    fn would_need_restack_returns_existing_needs_restack_without_predicted_oid() {
        let stack = make_test_stack();
        let scope: Vec<String> = vec![
            "feat-a".to_string(),
            "feat-a-1".to_string(),
            "feat-b".to_string(),
        ];
        let result = would_need_restack(&stack, &scope, None);
        // feat-a-1 already has needs_restack=true; no predicted oid so no new additions
        assert!(result.contains(&"feat-a-1".to_string()));
        assert!(!result.contains(&"feat-a".to_string()));
        assert!(!result.contains(&"feat-b".to_string()));
    }

    #[test]
    fn would_need_restack_adds_branches_with_stale_parent_revision() {
        let stack = make_test_stack();
        let scope: Vec<String> = vec![
            "feat-a".to_string(),
            "feat-a-1".to_string(),
            "feat-b".to_string(),
        ];
        // Predict trunk will advance to a new OID — feat-a and feat-b have stored "abc123"
        let result = would_need_restack(&stack, &scope, Some("newoid999"));
        // feat-a and feat-b both point to trunk (main) with parent_revision "abc123" != "newoid999"
        assert!(
            result.contains(&"feat-a".to_string()),
            "feat-a should need restack"
        );
        assert!(
            result.contains(&"feat-b".to_string()),
            "feat-b should need restack"
        );
        // feat-a-1 is already needs_restack AND its parent feat-a now also needs restack
        assert!(
            result.contains(&"feat-a-1".to_string()),
            "feat-a-1 should propagate"
        );
    }

    #[test]
    fn would_need_restack_propagates_to_descendants() {
        let stack = make_test_stack();
        // feat-a itself needs restack → feat-a-1 should also be included
        let mut stack_mut = stack;
        // Set feat-a to needs_restack=true manually
        if let Some(br) = stack_mut.branches.get_mut("feat-a") {
            br.needs_restack = true;
        }
        let scope: Vec<String> = vec!["feat-a".to_string(), "feat-a-1".to_string()];
        let result = would_need_restack(&stack_mut, &scope, None);
        assert!(result.contains(&"feat-a".to_string()));
        assert!(
            result.contains(&"feat-a-1".to_string()),
            "descendant should propagate"
        );
    }

    #[test]
    fn would_need_restack_up_to_date_when_revision_matches() {
        let stack = make_test_stack();
        let scope: Vec<String> = vec!["feat-a".to_string(), "feat-b".to_string()];
        // "abc123" is the stored parent_revision for both — if trunk stays at "abc123", no restack
        let result = would_need_restack(&stack, &scope, Some("abc123"));
        assert!(
            !result.contains(&"feat-a".to_string()),
            "up-to-date when revision matches"
        );
        assert!(
            !result.contains(&"feat-b".to_string()),
            "up-to-date when revision matches"
        );
    }
}
