use crate::config::Config;
use crate::engine::branch_detect::{
    find_ancestor_merged_branches, find_stale_branches, find_upstream_gone_branches,
    has_unique_commits_since_any_base,
};
use crate::engine::stats::{self, CiFacts, HygieneFacts, RepoStats, StatsFacts, WorktreeFact};
use crate::engine::{Stack, StackSnapshot, TrackedFacts, collect_tracked_facts};
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::progress::LiveTimer;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;
use std::collections::{HashMap, HashSet};

/// Above this many non-prunable worktrees, skip the per-worktree dirty check
/// (each check is a `git status` subprocess) and report `dirty_worktrees: None`.
const DIRTY_CHECK_LIMIT: usize = 16;

/// Above this many upstream-gone candidates, skip the unique-commits refine
/// pass (each check is a `git rev-list` subprocess).
const HYGIENE_GONE_REFINE_LIMIT: usize = 32;

pub fn run(json: bool, current: bool, ci: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let snapshot = StackSnapshot::load(&repo)?;
    let stack = snapshot.stack;
    let current_branch = snapshot.current_branch;
    let config = Config::load()?;

    let scope = compute_scope(&stack, &current_branch, current);
    let mut scope_vec: Vec<String> = scope.iter().cloned().collect();
    scope_vec.sort();

    let remote_info = RemoteInfo::from_repo(&repo, &config).ok();
    let repo_slug = remote_info.as_ref().map(|r| r.project_path());
    let trunk_divergence = repo.commits_vs_remote_named(config.remote_name(), &stack.trunk);
    let tracked: TrackedFacts = collect_tracked_facts(&repo, &stack);

    let ahead_behind = gather_ahead_behind(&repo, &stack, &scope_vec);
    let (worktrees, dirty_worktrees) = gather_worktree_facts(&repo)?;
    let idle_slots = gather_idle_slots(&repo, &config);
    let hygiene = gather_hygiene(&repo, &stack, &current_branch, &config)?;

    let (ci_facts, ci_unavailable_reason) =
        gather_ci_facts(ci, json, &repo, &stack, &scope_vec, &remote_info);

    let facts = StatsFacts {
        stack,
        current: current_branch,
        current_only: current,
        repo_slug,
        trunk_divergence,
        tracked,
        ahead_behind,
        worktrees,
        dirty_worktrees,
        idle_slots,
        hygiene,
        ci: ci_facts,
        ci_unavailable_reason,
    };

    let result = stats::compute(&facts);

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    render_human(&result);
    Ok(())
}

fn compute_scope(stack: &Stack, current: &str, current_only: bool) -> HashSet<String> {
    if current_only {
        stack
            .current_stack(current)
            .into_iter()
            .filter(|b| b != &stack.trunk)
            .collect()
    } else {
        stack
            .branches
            .keys()
            .filter(|b| *b != &stack.trunk)
            .cloned()
            .collect()
    }
}

fn gather_ahead_behind(
    repo: &GitRepo,
    stack: &Stack,
    scope: &[String],
) -> HashMap<String, (usize, usize)> {
    let pairs: Vec<(String, String)> = scope
        .iter()
        .map(|name| {
            let base = stack
                .branches
                .get(name)
                .and_then(|b| b.parent.clone())
                .unwrap_or_else(|| stack.trunk.clone());
            (base, name.clone())
        })
        .collect();
    let results = repo.commits_ahead_behind_many(&pairs);
    scope
        .iter()
        .cloned()
        .zip(results.into_iter().map(|r| r.unwrap_or((0, 0))))
        .collect()
}

fn gather_worktree_facts(repo: &GitRepo) -> Result<(Vec<WorktreeFact>, Option<usize>)> {
    let infos = repo.list_worktrees()?;
    let non_prunable_paths: Vec<std::path::PathBuf> = infos
        .iter()
        .filter(|w| !w.is_prunable)
        .map(|w| w.path.clone())
        .collect();

    let dirty_by_path: Option<HashMap<std::path::PathBuf, bool>> =
        if non_prunable_paths.len() <= DIRTY_CHECK_LIMIT {
            let results =
                crate::parallel::map_ordered(&non_prunable_paths, |path| worktree_is_dirty(path));
            Some(non_prunable_paths.iter().cloned().zip(results).collect())
        } else {
            None
        };

    let facts: Vec<WorktreeFact> = infos
        .iter()
        .map(|w| {
            let is_dirty = if w.is_prunable {
                None
            } else {
                dirty_by_path.as_ref().and_then(|m| m.get(&w.path).copied())
            };
            WorktreeFact {
                name: w.name.clone(),
                is_main: w.is_main,
                is_prunable: w.is_prunable,
                is_dirty,
            }
        })
        .collect();

    let dirty_worktrees = dirty_by_path.map(|m| m.values().filter(|d| **d).count());

    Ok((facts, dirty_worktrees))
}

/// Thread-safe dirty check (subprocess-based, unlike `GitRepo::is_dirty_at`
/// which holds a non-`Sync` `git2::Repository`).
fn worktree_is_dirty(path: &std::path::Path) -> bool {
    crate::git::command::output(path, &["status", "--porcelain"])
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}

fn gather_idle_slots(repo: &GitRepo, config: &Config) -> usize {
    crate::commands::worktree::shared::managed_worktrees_dir(repo, config)
        .ok()
        .and_then(|dir| crate::commands::worktree::pool::load(&dir).ok())
        .map(|pool| pool.idle_count())
        .unwrap_or(0)
}

fn gather_hygiene(
    repo: &GitRepo,
    stack: &Stack,
    current: &str,
    config: &Config,
) -> Result<HygieneFacts> {
    let workdir = repo.workdir()?.to_path_buf();
    let trunk = &stack.trunk;
    let remote_trunk_ref = format!("{}/{}", config.remote_name(), trunk);
    let stale_days = config.branch.stale_days;

    let merged: HashSet<String> =
        find_ancestor_merged_branches(&workdir, trunk, Some(remote_trunk_ref.as_str()))?
            .into_iter()
            .filter(|b| b != current)
            .collect();

    let gone_candidates: Vec<String> = find_upstream_gone_branches(&workdir, trunk)?
        .into_iter()
        .filter(|b| b != trunk && b != current && !merged.contains(b))
        .collect();

    let gone: HashSet<String> = if gone_candidates.len() <= HYGIENE_GONE_REFINE_LIMIT {
        gone_candidates
            .into_iter()
            .filter(|b| {
                !has_unique_commits_since_any_base(&workdir, b, &[trunk, remote_trunk_ref.as_str()])
                    .unwrap_or(false)
            })
            .collect()
    } else {
        gone_candidates.into_iter().collect()
    };

    let already_classified: HashSet<String> = merged.iter().chain(gone.iter()).cloned().collect();
    let stale = find_stale_branches(&workdir, trunk, current, stale_days, &already_classified)?;

    Ok(HygieneFacts {
        merged_but_local: merged.len(),
        upstream_gone: gone.len(),
        stale: stale.len(),
        stale_days,
    })
}

fn gather_ci_facts(
    requested: bool,
    json: bool,
    repo: &GitRepo,
    stack: &Stack,
    scope: &[String],
    remote_info: &Option<RemoteInfo>,
) -> (Option<CiFacts>, Option<String>) {
    if !requested {
        return (None, None);
    }

    let Some(remote) = remote_info else {
        return (
            None,
            Some("no supported forge remote configured".to_string()),
        );
    };
    if crate::forge::forge_token(remote.forge).is_none() {
        return (
            None,
            Some("forge authentication is not configured".to_string()),
        );
    }

    let timer = LiveTimer::maybe_new_stderr(!json, "Checking CI...");
    let result = (|| -> Result<CiFacts> {
        let rt = tokio::runtime::Runtime::new()?;
        let statuses = rt.block_on(async {
            let client = ForgeClient::new(remote)?;
            crate::commands::ci::fetch_ci_statuses_async(repo, &client, stack, scope).await
        })?;
        let failing = statuses
            .iter()
            .filter(|s| s.overall_status.as_deref() == Some("failure"))
            .count();
        let pending = statuses
            .iter()
            .filter(|s| s.overall_status.as_deref() == Some("pending"))
            .count();
        let passing = statuses
            .iter()
            .filter(|s| s.overall_status.as_deref() == Some("success"))
            .count();
        Ok(CiFacts {
            failing,
            pending,
            passing,
        })
    })();

    match result {
        Ok(facts) => {
            LiveTimer::maybe_finish_ok(timer, "done");
            (Some(facts), None)
        }
        Err(err) => {
            LiveTimer::maybe_finish_warn(timer, "unavailable");
            (None, Some(format!("{err:#}")))
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn divergence_arrows(ahead: usize, behind: usize) -> String {
    let mut parts = Vec::new();
    if ahead > 0 {
        parts.push(format!("{}", format!("{}↑", ahead).green()));
    }
    if behind > 0 {
        parts.push(format!("{}", format!("{}↓", behind).red()));
    }
    parts.join(" ")
}

fn attention_label(kind: &str) -> &'static str {
    match kind {
        "restack" => "restack",
        "no_pr" => "no PR",
        "missing_parent" => "parent",
        "lanes" => "lanes",
        _ => "item",
    }
}

fn render_human(stats: &RepoStats) {
    // --- Header ---
    let mut header_parts: Vec<String> = Vec::new();
    if let Some(repo) = &stats.repo {
        header_parts.push(repo.clone());
    }
    header_parts.push(stats.trunk.clone());
    if let (Some(ahead), Some(behind)) = (stats.trunk_ahead, stats.trunk_behind) {
        if ahead == 0 && behind == 0 {
            header_parts.push("in sync".to_string());
        } else {
            header_parts.push(divergence_arrows(ahead, behind));
        }
    }
    println!(
        "{}{}",
        format!("{:<6}", "Stats").bold(),
        header_parts.join(" · ")
    );
    println!("{}{}", format!("{:<6}", "You").bold(), stats.current.cyan());
    println!();

    // --- Counters ---
    let shape = &stats.stack_shape;
    let mut counter_lines: Vec<(&str, String)> = Vec::new();

    if shape.tracked > 0 || shape.independent > 0 || shape.deepest > 0 {
        counter_lines.push((
            "Stacks",
            format!(
                "{} tracked  ·  {} independent  ·  deepest {}  ·  avg {:.1}",
                shape.tracked, shape.independent, shape.deepest, shape.avg_height
            ),
        ));
    }

    let pr = &stats.pr_mix;
    if pr.open > 0 || pr.no_pr > 0 || pr.frozen > 0 {
        let mut parts = vec![
            format!("{} open", pr.open),
            format!("{} draft", pr.draft),
            format!("{} no PR", pr.no_pr),
        ];
        if pr.frozen > 0 {
            parts.push(format!("{} frozen", pr.frozen));
        }
        counter_lines.push(("PRs", parts.join("  ·  ")));
    }

    let health = &stats.health;
    if health.need_restack > 0
        || health.missing_parent > 0
        || health.dirty_worktrees.unwrap_or(0) > 0
    {
        let mut parts = vec![
            format!("{} need restack", health.need_restack),
            format!("{} missing parent", health.missing_parent),
        ];
        if let Some(dirty) = health.dirty_worktrees {
            parts.push(format!("{} dirty", dirty));
        }
        counter_lines.push(("Health", parts.join("  ·  ")));
    }

    let wt = &stats.worktrees;
    if wt.linked > 0 || wt.idle_slots > 0 {
        counter_lines.push((
            "Worktrees",
            format!("{} linked  ·  {} idle", wt.linked, wt.idle_slots),
        ));
    }

    if let Some(ci) = &stats.ci {
        if ci.failing > 0 || ci.pending > 0 || ci.passing > 0 {
            counter_lines.push((
                "CI",
                format!(
                    "{} failing  ·  {} pending  ·  {} passing",
                    ci.failing, ci.pending, ci.passing
                ),
            ));
        }
    } else if let Some(reason) = &stats.ci_unavailable_reason {
        counter_lines.push(("CI", format!("{}", reason.dimmed())));
    }

    if !counter_lines.is_empty() {
        for (label, value) in &counter_lines {
            println!("{:<11}{}", label, value);
        }
        println!();
    }

    // --- Attention ---
    if !stats.attention.is_empty() {
        println!("{}", "Attention".bold());
        for item in &stats.attention {
            println!(
                "   {}  {:<10}{}",
                item.glyph,
                attention_label(item.kind),
                item.detail
            );
        }
        println!();
    }

    // --- Biggest stacks ---
    if !stats.biggest_stacks.is_empty() {
        println!("{}", "Biggest stacks".bold());
        for summary in &stats.biggest_stacks {
            let pr_range = match (summary.pr_low, summary.pr_high) {
                (Some(low), Some(high)) if low == high => format!("#{}", low),
                (Some(low), Some(high)) => format!("#{}\u{2013}#{}", low, high),
                _ => "–".to_string(),
            };
            println!(
                "   {:>2}  {:<20} {:>4}  {:<8}  {}",
                summary.height,
                summary.root,
                format!("{}↑", summary.commits_ahead),
                summary.label,
                pr_range
            );
        }
        println!();
    }

    // --- PR mix bar chart ---
    let max = pr.ready.max(pr.draft).max(pr.no_pr);
    if max > 0 {
        println!("{}", "PR mix (open)".bold());
        println!(
            "   {:<5} {}  {}",
            "ready",
            stats::bar(pr.ready, max, 10),
            pr.ready
        );
        println!(
            "   {:<5} {}  {}",
            "draft",
            stats::bar(pr.draft, max, 10),
            pr.draft
        );
        println!(
            "   {:<5} {}  {}",
            "none",
            stats::bar(pr.no_pr, max, 10),
            pr.no_pr
        );
        println!();
    }

    // --- Hygiene ---
    let hygiene = &stats.hygiene;
    if hygiene.merged_but_local > 0 || hygiene.upstream_gone > 0 || hygiene.stale > 0 {
        println!("{}", "Hygiene".bold());
        if hygiene.merged_but_local > 0 {
            println!(
                "   {:<17}  {:<2}  {}",
                "merged-but-local",
                hygiene.merged_but_local,
                "st sweep --delete".dimmed()
            );
        }
        if hygiene.upstream_gone > 0 {
            println!("   {:<17}  {:<2}", "upstream-gone", hygiene.upstream_gone);
        }
        if hygiene.stale > 0 {
            println!(
                "   {:<17}  {:<2}",
                format!("stale ({}d+)", hygiene.stale_days),
                hygiene.stale
            );
        }
        println!();
    }

    // --- Next actions ---
    if !stats.next_actions.is_empty() {
        println!("{}", "Next".bold());
        for action in &stats.next_actions {
            println!("   {}", action.cyan());
        }
    }
}
