use super::{remove, shared::compute_worktree_details};
use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
enum CleanupKind {
    Detached,
    ManagedMerged,
}

impl CleanupKind {
    fn summary(self) -> &'static str {
        match self {
            Self::Detached => "detached",
            Self::ManagedMerged => "managed + merged",
        }
    }
}

#[derive(Debug, Clone)]
struct CleanupCandidate {
    kind: CleanupKind,
    name: String,
    branch_label: String,
    path: PathBuf,
    dirty: bool,
}

#[derive(Debug, Clone)]
struct BlockedCandidate {
    candidate: CleanupCandidate,
    blockers: Vec<&'static str>,
}

#[derive(Debug, Clone)]
struct CleanupPlan {
    prune_candidates: Vec<StaleEntry>,
    pruned: usize,
    prune_skipped: usize,
    candidates: Vec<CleanupCandidate>,
    blocked: Vec<BlockedCandidate>,
    ignored: usize,
}

#[derive(Debug, Clone)]
struct StaleEntry {
    name: String,
    branch_label: String,
    path: PathBuf,
}

pub fn run(force: bool, yes: bool, dry_run: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let plan = build_plan(&repo, force, dry_run)?;

    print_prune_summary(&plan, dry_run);

    if plan.candidates.is_empty() {
        print_blocked(&plan.blocked);
        print_ignored(plan.ignored);

        if plan.prune_candidates.is_empty() && plan.pruned == 0 && plan.prune_skipped == 0 {
            println!("{}", "Nothing to clean up.".dimmed());
        } else if dry_run {
            println!(
                "{}",
                "No additional live worktrees would be removed.".dimmed()
            );
        } else {
            println!(
                "{}",
                "No additional safe worktrees matched cleanup.".dimmed()
            );
        }

        if dry_run {
            println!();
            println!("{}", "Dry run only. No changes made.".dimmed());
        }
        return Ok(());
    }

    println!();
    println!(
        "{}",
        format!(
            "Found {} cleanup candidate{}:",
            plan.candidates.len(),
            if plan.candidates.len() == 1 { "" } else { "s" }
        )
        .dimmed()
    );
    for candidate in &plan.candidates {
        print_candidate(candidate);
    }

    print_blocked(&plan.blocked);
    print_ignored(plan.ignored);

    if dry_run {
        println!();
        println!("{}", "Dry run only. No changes made.".dimmed());
        return Ok(());
    }

    if !yes {
        if !std::io::stdin().is_terminal() {
            bail!(
                "`st wt cleanup` needs confirmation in non-interactive mode. Re-run with `--yes`."
            );
        }

        let confirmed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!(
                "Remove {} worktree{}?",
                plan.candidates.len(),
                if plan.candidates.len() == 1 { "" } else { "s" }
            ))
            .default(true)
            .interact()?;

        if !confirmed {
            println!("{}", "Cancelled.".yellow());
            return Ok(());
        }
    }

    println!();

    let mut removed = 0usize;
    let mut failed = 0usize;
    for candidate in &plan.candidates {
        match remove::run(Some(candidate.path.display().to_string()), force, false) {
            Ok(()) => removed += 1,
            Err(error) => {
                failed += 1;
                eprintln!(
                    "{}",
                    format!(
                        "Warning: could not remove worktree '{}': {}",
                        candidate.name, error
                    )
                    .yellow()
                );
            }
        }
    }

    println!();
    println!(
        "{}",
        format!(
            "Cleanup removed {} worktree{}.",
            removed,
            if removed == 1 { "" } else { "s" }
        )
        .green()
        .bold()
    );

    if failed > 0 {
        bail!(
            "Cleanup left {} worktree{} behind due to removal errors.",
            failed,
            if failed == 1 { "" } else { "s" }
        );
    }

    Ok(())
}

fn build_plan(repo: &GitRepo, force: bool, dry_run: bool) -> Result<CleanupPlan> {
    let before = repo.list_worktrees()?;
    let prune_candidates = before
        .iter()
        .filter(|worktree| worktree.is_prunable)
        .map(stale_entry_from)
        .collect::<Vec<_>>();

    let (pruned, prune_skipped, active_worktrees) = if dry_run || prune_candidates.is_empty() {
        (0, 0, before)
    } else {
        repo.worktree_prune()?;

        let after = repo.list_worktrees()?;
        let remaining_prunable: HashSet<_> = after
            .iter()
            .filter(|worktree| worktree.is_prunable)
            .map(|worktree| worktree.path.clone())
            .collect();

        let pruned = prune_candidates
            .iter()
            .filter(|entry| !remaining_prunable.contains(&entry.path))
            .count();
        let prune_skipped = prune_candidates.len().saturating_sub(pruned);
        (pruned, prune_skipped, after)
    };

    let details = active_worktrees
        .into_iter()
        .map(|worktree| compute_worktree_details(repo, worktree))
        .collect::<Result<Vec<_>>>()?;

    let mut candidates = Vec::new();
    let mut blocked = Vec::new();
    let mut ignored = 0usize;

    for detail in details {
        if detail.info.is_prunable {
            continue;
        }

        let Some(kind) = classify_candidate(repo, &detail)? else {
            ignored += 1;
            continue;
        };

        let candidate = CleanupCandidate {
            kind,
            name: detail.info.name.clone(),
            branch_label: detail.branch_label.clone(),
            path: detail.info.path.clone(),
            dirty: detail.dirty,
        };
        let blockers = candidate_blockers(&detail, force);

        if blockers.is_empty() {
            candidates.push(candidate);
        } else {
            blocked.push(BlockedCandidate {
                candidate,
                blockers,
            });
        }
    }

    Ok(CleanupPlan {
        prune_candidates,
        pruned,
        prune_skipped,
        candidates,
        blocked,
        ignored,
    })
}

fn stale_entry_from(worktree: &crate::git::repo::WorktreeInfo) -> StaleEntry {
    StaleEntry {
        name: worktree.name.clone(),
        branch_label: worktree
            .branch
            .clone()
            .unwrap_or_else(|| "(detached)".to_string()),
        path: worktree.path.clone(),
    }
}

fn classify_candidate(
    repo: &GitRepo,
    detail: &crate::commands::worktree::shared::WorktreeDetails,
) -> Result<Option<CleanupKind>> {
    if detail.info.is_main || detail.info.is_prunable {
        return Ok(None);
    }

    if detail.info.branch.is_none() {
        return Ok(Some(CleanupKind::Detached));
    }

    if !detail.is_managed {
        return Ok(None);
    }

    let Some(branch) = detail.info.branch.as_deref() else {
        return Ok(None);
    };

    if repo.is_branch_merged_equivalent_to_trunk(branch)? {
        Ok(Some(CleanupKind::ManagedMerged))
    } else {
        Ok(None)
    }
}

fn candidate_blockers(
    detail: &crate::commands::worktree::shared::WorktreeDetails,
    force: bool,
) -> Vec<&'static str> {
    let mut blockers = Vec::new();

    if detail.info.is_current {
        blockers.push("current");
    }
    if detail.info.is_locked {
        blockers.push("locked");
    }
    if detail.rebase_in_progress {
        blockers.push("rebase");
    }
    if detail.merge_in_progress {
        blockers.push("merge");
    }
    if detail.has_conflicts {
        blockers.push("conflicts");
    }
    if detail.dirty && !force {
        blockers.push("dirty");
    }

    blockers
}

fn print_candidate(candidate: &CleanupCandidate) {
    let mut summary = candidate.kind.summary().to_string();
    if candidate.dirty {
        summary.push_str(", dirty");
    }

    println!(
        "  {} {}  {}  {}",
        "▸".bright_black(),
        candidate.name.cyan(),
        format!("({})", candidate.branch_label).dimmed(),
        format!("{}  {}", summary, candidate.path.display()).dimmed(),
    );
}

fn print_prune_summary(plan: &CleanupPlan, dry_run: bool) {
    if dry_run {
        if plan.prune_candidates.is_empty() {
            return;
        }

        println!(
            "{}",
            format!(
                "Would prune {} stale {}:",
                plan.prune_candidates.len(),
                if plan.prune_candidates.len() == 1 {
                    "entry"
                } else {
                    "entries"
                }
            )
            .dimmed()
        );
        for entry in &plan.prune_candidates {
            println!(
                "  {} {}  {}  {}",
                "▸".bright_black(),
                entry.name.cyan(),
                format!("({})", entry.branch_label).dimmed(),
                entry.path.display().to_string().dimmed(),
            );
        }
        return;
    }

    if plan.pruned > 0 {
        println!(
            "{}  {} stale {} pruned",
            "Pruned".green().bold(),
            plan.pruned.to_string().cyan(),
            if plan.pruned == 1 { "entry" } else { "entries" }
        );
    }
    if plan.prune_skipped > 0 {
        println!(
            "  {} {} {} still marked prunable",
            "Skipped".yellow().bold(),
            plan.prune_skipped.to_string().yellow(),
            if plan.prune_skipped == 1 {
                "entry"
            } else {
                "entries"
            }
        );
    }
}

fn print_blocked(blocked: &[BlockedCandidate]) {
    if blocked.is_empty() {
        return;
    }

    println!();
    println!(
        "{}",
        format!(
            "Skipping {} unsafe candidate{}:",
            blocked.len(),
            if blocked.len() == 1 { "" } else { "s" }
        )
        .dimmed()
    );
    for blocked_candidate in blocked {
        println!(
            "  {} {}  {}",
            "▸".bright_black(),
            blocked_candidate.candidate.name.yellow(),
            format!(
                "{} [{}]",
                blocked_candidate.candidate.kind.summary(),
                blocked_candidate.blockers.join(", ")
            )
            .dimmed(),
        );
    }
}

fn print_ignored(ignored: usize) {
    if ignored == 0 {
        return;
    }

    println!();
    println!(
        "{}",
        format!(
            "Ignored {} active worktree{} that do not match cleanup rules.",
            ignored,
            if ignored == 1 { "" } else { "s" }
        )
        .dimmed()
    );
}
