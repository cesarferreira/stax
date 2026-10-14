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

/// Width of the left-hand label column shared by the header and counter rows.
const LABEL_WIDTH: usize = 11;
const RULE_MIN: usize = 44;
const RULE_MAX: usize = 74;

fn rule_width() -> usize {
    (console::Term::stdout().size().1 as usize).clamp(RULE_MIN, RULE_MAX)
}

/// Join value chunks with a dimmed separator so the values stay the bright part
/// of each row.
fn join_parts(parts: &[String]) -> String {
    parts.join(&format!("{}", "  ·  ".dimmed()))
}

/// A count and its unit, e.g. a bright `3` followed by a dimmed `open`.
fn metric(value: &str, label: &str, color: colored::Color) -> String {
    format!("{} {}", value.color(color).bold(), label.dimmed())
}

/// The inverse of [`metric`], for values that read better after their name
/// ("deepest 6" rather than "6 deepest").
fn labeled(label: &str, value: &str) -> String {
    format!("{} {}", label.dimmed(), value.bold())
}

/// Truncate to a visible width, appending an ellipsis. Only safe for strings
/// that carry no ANSI codes (attention details are built plain by the engine).
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push('…');
    out
}

/// Pad to a fixed width, truncating anything longer so the column never
/// shifts. ANSI-free input only.
fn cell(text: &str, width: usize) -> String {
    format!("{:<width$}", truncate(text, width), width = width)
}

fn label_cell(label: &str) -> colored::ColoredString {
    format!("{:<LABEL_WIDTH$}", label).dimmed()
}

fn heading(text: &str) -> colored::ColoredString {
    text.bold().underline()
}

fn divergence_arrows(ahead: usize, behind: usize) -> String {
    let mut parts = Vec::new();
    if ahead > 0 {
        parts.push(format!("{}", format!("{}↑", ahead).green().bold()));
    }
    if behind > 0 {
        parts.push(format!("{}", format!("{}↓", behind).red().bold()));
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

fn attention_color(kind: &str) -> colored::Color {
    match kind {
        "missing_parent" => colored::Color::Red,
        "restack" | "no_pr" => colored::Color::Yellow,
        _ => colored::Color::Cyan,
    }
}

/// `stats::bar` emits filled cells followed by empty ones; color the two runs
/// separately so the empty track recedes.
fn colored_bar(value: usize, max: usize, cells: usize, color: colored::Color) -> String {
    let bar = stats::bar(value, max, cells);
    let filled: String = bar.chars().filter(|c| *c == '█').collect();
    let empty: String = bar.chars().filter(|c| *c != '█').collect();
    format!("{}{}", filled.color(color), empty.dimmed())
}

fn render_human(stats: &RepoStats) {
    println!();
    render_header(stats);
    render_counters(stats);
    render_attention(stats);
    render_biggest_stacks(stats);
    render_pr_mix(stats);
    render_hygiene(stats);
    render_next(stats);
    println!();
}

fn render_header(stats: &RepoStats) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(repo) = &stats.repo {
        parts.push(format!("{}", repo.bold()));
    }
    parts.push(format!("{}", stats.trunk.cyan().bold()));
    if let (Some(ahead), Some(behind)) = (stats.trunk_ahead, stats.trunk_behind) {
        if ahead == 0 && behind == 0 {
            parts.push(format!("{}", "in sync".green()));
        } else {
            parts.push(divergence_arrows(ahead, behind));
        }
    }
    if stats.scope == "current" {
        parts.push(format!("{}", "current stack only".dimmed()));
    }

    println!("{}{}", label_cell("Repo"), join_parts(&parts));
    println!(
        "{}{}",
        label_cell("You"),
        stats.current.bright_cyan().bold()
    );
    println!("{}", "─".repeat(rule_width()).dimmed());
}

fn render_counters(stats: &RepoStats) {
    use colored::Color::{Blue, Cyan, Green, Red, White, Yellow};

    let mut rows: Vec<(&str, String)> = Vec::new();

    let shape = &stats.stack_shape;
    if shape.tracked > 0 {
        rows.push((
            "Stacks",
            join_parts(&[
                metric(
                    &shape.independent.to_string(),
                    if shape.independent == 1 {
                        "stack"
                    } else {
                        "stacks"
                    },
                    White,
                ),
                metric(&shape.tracked.to_string(), "tracked", White),
                labeled("deepest", &shape.deepest.to_string()),
                labeled("avg", &format!("{:.1}", shape.avg_height)),
            ]),
        ));
    }

    let pr = &stats.pr_mix;
    let mut pr_parts: Vec<String> = Vec::new();
    if pr.ready > 0 {
        pr_parts.push(metric(&pr.ready.to_string(), "ready", Green));
    }
    if pr.draft > 0 {
        pr_parts.push(metric(&pr.draft.to_string(), "draft", Yellow));
    }
    if pr.no_pr > 0 {
        pr_parts.push(metric(&pr.no_pr.to_string(), "no PR", Yellow));
    }
    if pr.frozen > 0 {
        pr_parts.push(metric(&pr.frozen.to_string(), "frozen", Blue));
    }
    if !pr_parts.is_empty() {
        rows.push(("PRs", join_parts(&pr_parts)));
    }

    let health = &stats.health;
    let mut health_parts: Vec<String> = Vec::new();
    if health.need_restack > 0 {
        health_parts.push(metric(
            &health.need_restack.to_string(),
            "need restack",
            Yellow,
        ));
    }
    if health.missing_parent > 0 {
        health_parts.push(metric(
            &health.missing_parent.to_string(),
            "missing parent",
            Red,
        ));
    }
    if let Some(dirty) = health.dirty_worktrees
        && dirty > 0
    {
        health_parts.push(metric(&dirty.to_string(), "dirty", Yellow));
    }
    if health_parts.is_empty() && shape.tracked > 0 {
        health_parts.push(format!("{} {}", "✓".green(), "all clear".dimmed()));
    }
    if !health_parts.is_empty() {
        rows.push(("Health", join_parts(&health_parts)));
    }

    let wt = &stats.worktrees;
    let mut wt_parts: Vec<String> = Vec::new();
    if wt.linked > 0 {
        wt_parts.push(metric(&wt.linked.to_string(), "linked", Cyan));
    }
    if wt.idle_slots > 0 {
        wt_parts.push(metric(&wt.idle_slots.to_string(), "idle", White));
    }
    if !wt_parts.is_empty() {
        rows.push(("Worktrees", join_parts(&wt_parts)));
    }

    if let Some(ci) = &stats.ci {
        let mut ci_parts: Vec<String> = Vec::new();
        if ci.failing > 0 {
            ci_parts.push(metric(&ci.failing.to_string(), "failing", Red));
        }
        if ci.pending > 0 {
            ci_parts.push(metric(&ci.pending.to_string(), "pending", Yellow));
        }
        if ci.passing > 0 {
            ci_parts.push(metric(&ci.passing.to_string(), "passing", Green));
        }
        if !ci_parts.is_empty() {
            rows.push(("CI", join_parts(&ci_parts)));
        }
    } else if let Some(reason) = &stats.ci_unavailable_reason {
        rows.push(("CI", format!("{}", reason.dimmed())));
    }

    if rows.is_empty() {
        let hint = if stats.scope == "current" {
            "No tracked branches in the current stack — run `st create` to start one."
        } else {
            "No tracked branches yet — run `st create` to start a stack."
        };
        println!("{}", hint.dimmed());
        return;
    }

    for (label, value) in &rows {
        println!("{}{}", label_cell(label), value);
    }
}

fn render_attention(stats: &RepoStats) {
    if stats.attention.is_empty() {
        return;
    }
    println!();
    println!("{}", heading("Attention"));
    let detail_width = rule_width().saturating_sub(14);
    for item in &stats.attention {
        let color = attention_color(item.kind);
        println!(
            "  {} {} {}",
            item.glyph.color(color).bold(),
            format!("{:<9}", attention_label(item.kind)).color(color),
            truncate(&item.detail, detail_width)
        );
    }
}

fn render_biggest_stacks(stats: &RepoStats) {
    // With a single stack the table repeats the counters above, so it only
    // earns its space once there is something to compare.
    if stats.biggest_stacks.len() < 2 {
        return;
    }
    println!();
    println!("{}", heading("Biggest stacks"));
    for (index, summary) in stats.biggest_stacks.iter().enumerate() {
        let lane = crate::commands::stack_palette::lane_color(index);
        let pr_range = match (summary.pr_low, summary.pr_high) {
            (Some(low), Some(high)) if low == high => format!("#{}", low),
            (Some(low), Some(high)) => format!("#{}\u{2013}#{}", low, high),
            _ => "—".to_string(),
        };
        println!(
            "  {} {} {} {} {}",
            format!("{:>2}", summary.height).color(lane).bold(),
            cell(&summary.root, 30).color(lane),
            format!("{:>5}", format!("{}↑", summary.commits_ahead)).green(),
            cell(&summary.label, 9).dimmed(),
            pr_range.bright_magenta()
        );
    }
}

fn render_pr_mix(stats: &RepoStats) {
    let pr = &stats.pr_mix;
    let categories = [
        ("ready", pr.ready, colored::Color::Green),
        ("draft", pr.draft, colored::Color::Yellow),
        ("none", pr.no_pr, colored::Color::BrightBlack),
    ];
    // A chart of one non-zero bar says nothing the PRs row did not.
    if categories.iter().filter(|(_, value, _)| *value > 0).count() < 2 {
        return;
    }
    let max = categories
        .iter()
        .map(|(_, value, _)| *value)
        .max()
        .unwrap_or(0);

    println!();
    println!("{}", heading("PR mix"));
    for (label, value, color) in categories {
        println!(
            "  {} {}  {}",
            format!("{:<5}", label).dimmed(),
            colored_bar(value, max, 12, color),
            value.to_string().color(color).bold()
        );
    }
}

fn render_hygiene(stats: &RepoStats) {
    let hygiene = &stats.hygiene;
    let rows = [
        (
            "merged-but-local".to_string(),
            hygiene.merged_but_local,
            "st sweep --delete",
        ),
        ("upstream-gone".to_string(), hygiene.upstream_gone, ""),
        (format!("stale {}d+", hygiene.stale_days), hygiene.stale, ""),
    ];
    if rows.iter().all(|(_, count, _)| *count == 0) {
        return;
    }

    println!();
    println!("{}", heading("Hygiene"));
    for (label, count, hint) in rows.iter().filter(|(_, count, _)| *count > 0) {
        let mut line = format!(
            "  {} {}",
            format!("{:<17}", label).dimmed(),
            format!("{:>2}", count).yellow().bold()
        );
        if !hint.is_empty() {
            line.push_str(&format!("   {}", hint.dimmed()));
        }
        println!("{}", line);
    }
}

fn render_next(stats: &RepoStats) {
    if stats.next_actions.is_empty() {
        return;
    }
    let actions: Vec<String> = stats
        .next_actions
        .iter()
        .map(|action| format!("{}", action.cyan().bold()))
        .collect();
    println!();
    println!("{}{}", label_cell("Next"), join_parts(&actions));
}
