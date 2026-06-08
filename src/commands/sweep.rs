use crate::config::Config;
use crate::engine::branch_detect::{
    MergeType, StaleBranchInfo, find_merged_branches_all, find_stale_branches,
    find_upstream_gone_branches,
};
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, theme::ColorfulTheme};
use serde::Serialize;
use std::collections::HashSet;
use std::process::Command;

const DEFAULT_STALE_DAYS: u64 = 30;

pub fn run(
    delete: bool,
    include_stale: bool,
    force: bool,
    stale_days: Option<u64>,
    json: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let trunk = stack.trunk.clone();
    let workdir = repo.workdir()?.to_path_buf();
    let config = Config::load()?;

    let remote_name = config.remote_name();
    let remote_trunk_ref = format!("{}/{}", remote_name, trunk);
    let effective_stale_days = stale_days.unwrap_or(config.branch.stale_days);

    // --- Classify all local branches ---

    // 1. Merged (ancestor or squash-merged)
    let merged_infos: Vec<_> =
        find_merged_branches_all(&workdir, &trunk, Some(remote_trunk_ref.as_str()))?
            .into_iter()
            .filter(|m| m.branch != trunk && m.branch != current)
            .collect();
    let merged_set: HashSet<String> = merged_infos.iter().map(|m| m.branch.clone()).collect();

    // 2. Upstream-gone
    let gone_branches = find_upstream_gone_branches(&workdir, &trunk)?;
    // Merged takes precedence over upstream-gone; trunk/current are never candidates.
    let gone_set: HashSet<String> = gone_branches
        .into_iter()
        .filter(|b| b != &trunk && b != &current && !merged_set.contains(b))
        .collect();

    // 3. Stale (old commits, not merged or gone)
    let already_classified: HashSet<String> =
        merged_set.iter().chain(gone_set.iter()).cloned().collect();
    let stale_infos = find_stale_branches(
        &workdir,
        &trunk,
        &current,
        effective_stale_days,
        &already_classified,
    )?;
    let stale_set: HashSet<String> = stale_infos.iter().map(|s| s.branch.clone()).collect();

    // 4. Active = everything else except trunk + current
    let all_branches = repo.list_branches()?;
    let active_branches: Vec<String> = all_branches
        .into_iter()
        .filter(|b| {
            b != &trunk
                && b != &current
                && !merged_set.contains(b)
                && !gone_set.contains(b)
                && !stale_set.contains(b)
        })
        .collect();

    let total_classified =
        merged_set.len() + gone_set.len() + stale_set.len() + active_branches.len();

    // --- JSON output ---
    if json {
        return print_json(
            &merged_infos
                .iter()
                .map(|m| m.branch.clone())
                .collect::<Vec<_>>(),
            &gone_set.iter().cloned().collect::<Vec<_>>(),
            &stale_infos,
            &active_branches,
            &stack,
        );
    }

    // --- Human-readable output ---

    if total_classified == 0 {
        println!(
            "{}",
            "No local branches found (other than trunk and current).".dimmed()
        );
        return Ok(());
    }

    println!(
        "{} {} {}",
        "Branch sweep".bold(),
        "—".dimmed(),
        format!("stale threshold: {} days", effective_stale_days).dimmed()
    );
    println!();

    // Merged branches
    if !merged_set.is_empty() {
        let mut sorted: Vec<&String> = merged_set.iter().collect();
        sorted.sort();
        println!(
            "{} {}",
            format!("  merged  ({})", sorted.len()).green().bold(),
            "— safe to delete".dimmed()
        );
        for b in &sorted {
            let tracked_marker = if stack.branches.contains_key(*b) {
                " tracked".dimmed()
            } else {
                "".normal()
            };
            let merge_label = merged_infos
                .iter()
                .find(|m| &m.branch == *b)
                .map(|m| match m.merge_type {
                    MergeType::Ancestor => "",
                    MergeType::SquashMerge => " squash",
                })
                .unwrap_or("");
            println!(
                "    {} {}{}{}",
                "✓".green(),
                b.green(),
                merge_label.dimmed(),
                tracked_marker,
            );
        }
        println!();
    }

    // Upstream-gone branches
    if !gone_set.is_empty() {
        let mut sorted: Vec<&String> = gone_set.iter().collect();
        sorted.sort();
        println!(
            "{} {}",
            format!("  upstream-gone  ({})", sorted.len())
                .yellow()
                .bold(),
            "— remote deleted, safe to delete".dimmed()
        );
        for b in &sorted {
            let tracked_marker = if stack.branches.contains_key(*b) {
                " tracked".dimmed()
            } else {
                "".normal()
            };
            println!("    {} {}{}", "⚑".yellow(), b.yellow(), tracked_marker);
        }
        println!();
    }

    // Stale branches
    if !stale_infos.is_empty() {
        let mut sorted = stale_infos.clone();
        sorted.sort_by_key(|s| std::cmp::Reverse(s.days_old));
        println!(
            "{} {}",
            format!("  stale  ({})", sorted.len()).bright_black().bold(),
            format!("— no commits in {}+ days", effective_stale_days).dimmed()
        );
        for info in &sorted {
            let tracked_marker = if stack.branches.contains_key(&info.branch) {
                " tracked".dimmed()
            } else {
                "".normal()
            };
            let age = format_age(info.days_old);
            println!(
                "    {} {} {}{}",
                "○".bright_black(),
                info.branch.bright_black(),
                age.dimmed(),
                tracked_marker,
            );
        }
        println!();
    }

    // Active branches
    if !active_branches.is_empty() {
        let mut sorted = active_branches.clone();
        sorted.sort();
        println!("{}", format!("  active  ({})", sorted.len()).cyan().bold());
        for b in &sorted {
            let tracked_marker = if stack.branches.contains_key(b) {
                " tracked".dimmed()
            } else {
                "".normal()
            };
            println!("    {} {}{}", "○".cyan(), b.cyan(), tracked_marker);
        }
        println!();
    }

    // Summary / hints
    print_summary(
        &merged_set,
        &gone_set,
        &stale_set,
        effective_stale_days,
        delete,
    );

    // --- Deletion ---
    if delete {
        let mut to_delete: Vec<String> =
            merged_set.iter().chain(gone_set.iter()).cloned().collect();
        if include_stale {
            to_delete.extend(stale_set.iter().cloned());
        }
        to_delete.retain(|b| b != &current && b != &trunk);
        to_delete.sort();

        if to_delete.is_empty() {
            println!("{}", "Nothing to delete.".dimmed());
            return Ok(());
        }

        if !force {
            println!();
            println!(
                "Will delete {} branch{}:",
                to_delete.len().to_string().bold(),
                if to_delete.len() == 1 { "" } else { "es" }
            );
            for b in &to_delete {
                println!("  {} {}", "▸".bright_black(), b.red());
            }
            println!();
            let confirm = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Proceed with deletion?")
                .default(false)
                .interact()?;
            if !confirm {
                println!("{}", "Cancelled.".dimmed());
                return Ok(());
            }
        }

        let mut deleted = 0usize;
        let mut skipped = 0usize;

        for branch in &to_delete {
            let is_tracked = stack.branches.contains_key(branch);

            if is_tracked {
                let _ = reparent_children_to_trunk(&repo, &stack, branch, &to_delete);
            }

            match delete_branch_subprocess(&workdir, branch) {
                Ok(()) => {
                    if is_tracked {
                        let _ = BranchMetadata::delete(repo.inner(), branch);
                    }
                    println!("  {} {}", "✓".green(), branch.red());
                    deleted += 1;
                }
                Err(e) => {
                    println!(
                        "  {} skipped {} — {}",
                        "⚠".yellow(),
                        branch.yellow(),
                        e.to_string().dimmed()
                    );
                    skipped += 1;
                }
            }
        }

        println!();
        if deleted > 0 {
            println!(
                "Deleted {} branch{}{}.",
                deleted.to_string().bold(),
                if deleted == 1 { "" } else { "es" },
                if skipped > 0 {
                    format!(", {} skipped", skipped)
                } else {
                    String::new()
                }
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// JSON output
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SweepJsonBranch {
    name: String,
    status: &'static str,
    tracked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    days_old: Option<u64>,
}

#[derive(Serialize)]
struct SweepJson {
    branches: Vec<SweepJsonBranch>,
}

fn print_json(
    merged: &[String],
    gone: &[String],
    stale: &[StaleBranchInfo],
    active: &[String],
    stack: &Stack,
) -> Result<()> {
    let mut branches: Vec<SweepJsonBranch> = Vec::new();

    for b in merged {
        branches.push(SweepJsonBranch {
            name: b.clone(),
            status: "merged",
            tracked: stack.branches.contains_key(b),
            days_old: None,
        });
    }
    for b in gone {
        branches.push(SweepJsonBranch {
            name: b.clone(),
            status: "upstream-gone",
            tracked: stack.branches.contains_key(b),
            days_old: None,
        });
    }
    for s in stale {
        branches.push(SweepJsonBranch {
            name: s.branch.clone(),
            status: "stale",
            tracked: stack.branches.contains_key(&s.branch),
            days_old: Some(s.days_old),
        });
    }
    for b in active {
        branches.push(SweepJsonBranch {
            name: b.clone(),
            status: "active",
            tracked: stack.branches.contains_key(b),
            days_old: None,
        });
    }

    branches.sort_by(|a, b| a.name.cmp(&b.name));

    let out = SweepJson { branches };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn print_summary(
    merged: &HashSet<String>,
    gone: &HashSet<String>,
    stale: &HashSet<String>,
    stale_days: u64,
    delete_mode: bool,
) {
    let deletable = merged.len() + gone.len();
    if deletable == 0 && stale.is_empty() {
        println!("{}", "All branches are active.".green());
        return;
    }

    if delete_mode {
        return;
    }

    let mut hints: Vec<String> = Vec::new();

    if deletable > 0 {
        hints.push(format!(
            "run {} to delete {} merged/gone branch{}",
            "stax sweep --delete".bold(),
            deletable.to_string().bold(),
            if deletable == 1 { "" } else { "es" },
        ));
    }
    if !stale.is_empty() {
        hints.push(format!(
            "add {} to also delete {} stale branch{}",
            "--include-stale".bold(),
            stale.len().to_string().bold(),
            if stale.len() == 1 { "" } else { "es" },
        ));
    }
    if !stale.is_empty() && stale_days == DEFAULT_STALE_DAYS {
        hints.push(format!(
            "set {} in {} to change the stale threshold",
            "branch.stale_days".bold(),
            "~/.config/stax/config.toml".dimmed()
        ));
    }

    for (i, hint) in hints.iter().enumerate() {
        if i == 0 {
            println!("Tip: {}", hint);
        } else {
            println!("     {}", hint);
        }
    }
}

fn format_age(days: u64) -> String {
    if days < 7 {
        format!("({} day{})", days, if days == 1 { "" } else { "s" })
    } else if days < 31 {
        let weeks = days / 7;
        format!("({} week{})", weeks, if weeks == 1 { "" } else { "s" })
    } else if days < 365 {
        let months = days / 30;
        format!("({} month{})", months, if months == 1 { "" } else { "s" })
    } else {
        let years = days / 365;
        format!("({} year{})", years, if years == 1 { "" } else { "s" })
    }
}

/// Reparent stax-tracked children of `branch` to trunk before deleting it.
fn reparent_children_to_trunk(
    repo: &GitRepo,
    stack: &Stack,
    branch: &str,
    doomed_set: &[String],
) -> Result<()> {
    let trunk = &stack.trunk;
    let doomed: HashSet<&str> = doomed_set.iter().map(|s| s.as_str()).collect();

    let children: Vec<String> = stack
        .branches
        .iter()
        .filter(|(_, info)| info.parent.as_deref() == Some(branch))
        .map(|(name, _)| name.clone())
        .filter(|name| !doomed.contains(name.as_str()))
        .collect();

    if children.is_empty() {
        return Ok(());
    }

    let branch_tip = repo.branch_commit(branch).ok();

    for child in &children {
        let Some(child_meta) = BranchMetadata::read(repo.inner(), child)? else {
            continue;
        };

        let old_parent_boundary = branch_tip
            .clone()
            .filter(|tip| repo.is_ancestor(tip, child).unwrap_or(false))
            .unwrap_or_else(|| child_meta.parent_branch_revision.clone());

        let updated_meta = BranchMetadata {
            parent_branch_name: trunk.clone(),
            parent_branch_revision: old_parent_boundary,
            ..child_meta
        };
        updated_meta.write(repo.inner(), child)?;
    }

    Ok(())
}

fn delete_branch_subprocess(workdir: &std::path::Path, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to delete branch '{}'", branch))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{}", stderr.trim());
    }
    Ok(())
}
