use crate::cache::CiCache;
use crate::ci::{history, CheckRunInfo};
use crate::config::Config;
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::github::GitHubClient;
use crate::notifications::{self, BuiltInSound, Sound};
use crate::remote::RemoteInfo;
use anyhow::Result;
use futures_util::future::join_all;
use chrono::{DateTime, Utc};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// CI status for a branch
#[derive(Debug, Clone, Serialize)]
pub struct BranchCiStatus {
    pub branch: String,
    pub sha: String,
    pub sha_short: String,
    pub overall_status: Option<String>,
    pub check_runs: Vec<CheckRunInfo>,
    pub pr_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_is_draft: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiAlertSoundArg {
    DefaultSound,
    Path(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CiAlertSounds {
    success: CiAlertSound,
    error: CiAlertSound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CiAlertSound {
    BuiltIn(BuiltInSound),
    CustomPath(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CiAlertOutcome {
    Success,
    Error,
}

/// Raw timing data returned by calculate_branch_timing
struct BranchTiming {
    elapsed_secs: u64,
    average_secs: Option<u64>,
    is_complete: bool,
    /// Completion percentage (0-99) when in progress with history
    pct: Option<u8>,
}

/// Response from the check-runs API (detailed version)
#[derive(Debug, Deserialize)]
struct CheckRunsResponse {
    total_count: usize,
    check_runs: Vec<CheckRunDetail>,
}

#[derive(Debug, Deserialize)]
struct CheckRunDetail {
    name: String,
    status: String,
    conclusion: Option<String>,
    html_url: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
}

/// Response from the commit statuses API
#[derive(Debug, Deserialize)]
struct CommitStatus {
    context: String,
    state: String,
    target_url: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

/// Deduplicate check runs by name, keeping only the most recent for each
fn dedup_check_runs(check_runs: Vec<CheckRunInfo>) -> Vec<CheckRunInfo> {
    let mut unique_checks: HashMap<String, CheckRunInfo> = HashMap::new();
    for check in check_runs {
        let should_replace = if let Some(existing) = unique_checks.get(&check.name) {
            match (&check.started_at, &existing.started_at) {
                (Some(new_start), Some(existing_start)) => {
                    if let (Ok(new_time), Ok(existing_time)) = (
                        new_start.parse::<DateTime<Utc>>(),
                        existing_start.parse::<DateTime<Utc>>(),
                    ) {
                        new_time > existing_time
                    } else {
                        false
                    }
                }
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => true,
            }
        } else {
            true
        };

        if should_replace {
            unique_checks.insert(check.name.clone(), check);
        }
    }

    let mut result: Vec<CheckRunInfo> = unique_checks.into_values().collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

/// Calculate overall timing for the entire branch CI run
fn calculate_branch_timing(repo: &GitRepo, checks: &[CheckRunInfo]) -> Option<BranchTiming> {
    if checks.is_empty() {
        return None;
    }

    let is_complete = checks.iter().all(|c| c.status == "completed");

    // Try to derive elapsed from real timestamps first (most accurate)
    let elapsed_secs = try_elapsed_from_timestamps(checks, is_complete).or_else(|| {
        // Fallback: use the max elapsed_secs across all checks as a proxy
        // for how long the slowest check has been running
        checks.iter().filter_map(|c| c.elapsed_secs).max()
    })?;

    let average_secs = history::estimate_run_average(repo, checks)
        .or_else(|| checks.iter().filter_map(|c| c.average_secs).max());

    let pct = if !is_complete {
        average_secs.map(|avg| {
            if avg == 0 || elapsed_secs >= avg {
                99u8
            } else {
                ((elapsed_secs * 100) / avg).min(99) as u8
            }
        })
    } else {
        None
    };

    Some(BranchTiming {
        elapsed_secs,
        average_secs,
        is_complete,
        pct,
    })
}

/// Try to compute elapsed time from started_at / completed_at timestamps
fn try_elapsed_from_timestamps(checks: &[CheckRunInfo], is_complete: bool) -> Option<u64> {
    let earliest_start = checks
        .iter()
        .filter_map(|c| c.started_at.as_ref())
        .filter_map(|s| s.parse::<DateTime<Utc>>().ok())
        .min()?;

    let now = Utc::now();

    let elapsed_secs = if is_complete {
        let latest_complete = checks
            .iter()
            .filter_map(|c| c.completed_at.as_ref())
            .filter_map(|s| s.parse::<DateTime<Utc>>().ok())
            .max()?;
        let duration = latest_complete.signed_duration_since(earliest_start);
        duration.num_seconds().max(0) as u64
    } else {
        let duration = now.signed_duration_since(earliest_start);
        duration.num_seconds().max(0) as u64
    };

    Some(elapsed_secs)
}

/// Render a Unicode block progress bar. Width is number of block chars.
fn render_progress_bar(pct: u8, width: usize) -> String {
    let filled = ((pct as usize * width) / 100).min(width);
    let empty = width - filled;
    format!("{}{}", "▰".repeat(filled), "▱".repeat(empty))
}

/// Format the timing footer line for compact and verbose displays
fn format_timing_footer(timing: &BranchTiming, overall_status: Option<&str>) -> String {
    let elapsed_str = format_duration(timing.elapsed_secs);

    if timing.is_complete {
        let avg_str = timing
            .average_secs
            .map(|avg| format!("  (avg: {})", format_duration(avg)))
            .unwrap_or_default();
        match overall_status {
            Some("success") => format!("{}  ⏱ {}{}", "passed".green().bold(), elapsed_str, avg_str),
            Some("failure") => format!("{}  ⏱ {}{}", "failed".red().bold(), elapsed_str, avg_str),
            _ => format!("done  ⏱ {}{}", elapsed_str, avg_str),
        }
    } else {
        match (timing.average_secs, timing.pct) {
            (Some(avg), Some(pct)) => {
                let bar = render_progress_bar(pct, 10);
                let eta = if timing.elapsed_secs >= avg {
                    "overdue".yellow().to_string()
                } else {
                    format!("~{} left", format_duration(avg - timing.elapsed_secs))
                };
                format!(
                    "{}  {}  {}%  ⏱ {}  elapsed  {}  (avg: {})",
                    "running".yellow().bold(),
                    bar,
                    pct,
                    elapsed_str,
                    eta,
                    format_duration(avg)
                )
            }
            _ => format!("{}  ⏱ {} elapsed", "running".yellow().bold(), elapsed_str),
        }
    }
}

fn check_timing_text(check: &CheckRunInfo) -> String {
    match check.status.as_str() {
        "completed" => match (check.elapsed_secs, check.average_secs) {
            (Some(elapsed), Some(avg)) => {
                format!(
                    "{}  (avg: {})",
                    format_duration(elapsed),
                    format_duration(avg)
                )
            }
            (Some(elapsed), None) => format_duration(elapsed),
            (None, Some(avg)) => format!("avg: {}", format_duration(avg)),
            (None, None) => String::new(),
        },
        "in_progress" | "pending" | "queued" | "waiting" | "requested" => {
            match (check.elapsed_secs, check.average_secs) {
                (Some(elapsed), Some(avg)) => {
                    if elapsed >= avg {
                        format!(
                            "{}  overdue (avg: {})",
                            format_duration(elapsed),
                            format_duration(avg)
                        )
                    } else {
                        format!(
                            "{}  ~{} left (avg: {})",
                            format_duration(elapsed),
                            format_duration(avg - elapsed),
                            format_duration(avg)
                        )
                    }
                }
                (Some(elapsed), None) => format!("{} elapsed", format_duration(elapsed)),
                (None, Some(avg)) => format!("avg: {}", format_duration(avg)),
                (None, None) => String::new(),
            }
        }
        _ => String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    all: bool,
    stack: bool,
    json: bool,
    _refresh: bool,
    watch: bool,
    alert_arg: Option<CiAlertSoundArg>,
    no_alert: bool,
    strict: bool,
    interval: u64,
    verbose: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack_data = Stack::load(&repo)?;
    let config = Config::load()?;

    let remote_info = RemoteInfo::from_repo(&repo, &config).ok();

    // Get branches to check
    let branches_to_check: Vec<String> = if all {
        stack_data
            .branches
            .keys()
            .filter(|b| *b != &stack_data.trunk)
            .cloned()
            .collect()
    } else if stack {
        stack_data
            .current_stack(&current)
            .into_iter()
            .filter(|b| b != &stack_data.trunk)
            .collect()
    } else {
        // Default: current branch only
        vec![current.clone()]
    };

    if branches_to_check.is_empty() {
        println!("{}", "No tracked branches found.".dimmed());
        return Ok(());
    }

    let Some(remote) = remote_info else {
        anyhow::bail!("Could not determine remote info. Check that a git remote is configured.");
    };

    if crate::forge::forge_token(remote.forge).is_none() {
        anyhow::bail!(
            "{} auth not configured.\n\
             Set the appropriate token for your forge:\n  \
             - GitHub: `stax auth`, `stax auth --from-gh`, or set `STAX_GITHUB_TOKEN`\n  \
             - GitLab: `stax auth`, or set `STAX_GITLAB_TOKEN`, `GITLAB_TOKEN`, or `STAX_FORGE_TOKEN`\n  \
             - Gitea:  `stax auth`, or set `STAX_GITEA_TOKEN`, `GITEA_TOKEN`, or `STAX_FORGE_TOKEN`",
            remote.forge
        );
    }

    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();

    let client = ForgeClient::new(&remote)?;

    if watch {
        let alert = resolve_ci_alert_sounds(&config, alert_arg, no_alert);
        return run_watch_mode(
            &repo,
            &rt,
            &client,
            &stack_data,
            &branches_to_check,
            &current,
            interval,
            json,
            verbose,
            alert,
            strict,
        );
    }

    let statuses = fetch_ci_statuses(&repo, &rt, &client, &stack_data, &branches_to_check)?;
    update_ci_cache(&repo, &stack_data, &statuses);

    if json {
        println!("{}", serde_json::to_string_pretty(&statuses)?);
        return Ok(());
    }

    let multi = statuses.len() > 1;
    if verbose {
        // --verbose: compact cards view
        display_ci_compact(&repo, &statuses, &current, multi);
    } else {
        // default: full per-check table
        display_ci_verbose(&repo, &statuses, &current, multi);
    }
    record_ci_history(&repo, &statuses);

    Ok(())
}

/// Fetch CI statuses for all branches (async; use from an existing runtime or tests).
pub(crate) async fn fetch_ci_statuses_async(
    repo: &GitRepo,
    client: &ForgeClient,
    stack: &Stack,
    branches_to_check: &[String],
) -> Result<Vec<BranchCiStatus>> {
    let prepared: Vec<(String, String, String, Option<u64>)> = branches_to_check
        .iter()
        .filter_map(|branch| {
            let sha = repo.branch_commit(branch).ok()?;
            let sha_short = sha.chars().take(7).collect::<String>();
            let pr_number = stack.branches.get(branch).and_then(|b| b.pr_number);
            Some((branch.clone(), sha, sha_short, pr_number))
        })
        .collect();

    let mut statuses = join_all(
        prepared
            .iter()
            .map(|(branch, sha, sha_short, pr_number)| async {
                let check_runs_result = client.fetch_checks(repo, sha).await;
                let (overall_status, check_runs) = match check_runs_result {
                    Ok((status, runs)) => (status, runs),
                    Err(_) => (None, Vec::new()),
                };

                let pr_live = match pr_number {
                    Some(n) => client.get_pr(*n).await.ok(),
                    None => None,
                };
                let pr_is_draft = pr_live.as_ref().map(|p| p.is_draft);

                BranchCiStatus {
                    branch: branch.clone(),
                    sha: sha.clone(),
                    sha_short: sha_short.clone(),
                    overall_status,
                    check_runs,
                    pr_number: *pr_number,
                    pr_is_draft,
                }
            }),
    )
    .await;

    statuses.sort_by(|a, b| a.branch.cmp(&b.branch));

    Ok(statuses)
}

/// Fetch CI statuses for all branches
pub fn fetch_ci_statuses(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    stack: &Stack,
    branches_to_check: &[String],
) -> Result<Vec<BranchCiStatus>> {
    rt.block_on(fetch_ci_statuses_async(
        repo,
        client,
        stack,
        branches_to_check,
    ))
}

/// Compact single-branch display block
fn display_branch_compact(repo: &GitRepo, status: &BranchCiStatus, is_current: bool) {
    if status.check_runs.is_empty() {
        // No CI: single line
        let marker = if is_current { "◉" } else { "○" };
        println!(
            "{}  {}  {}  {}",
            marker,
            status.branch.dimmed(),
            format!("({})", status.sha_short).dimmed(),
            "no CI".dimmed()
        );
        return;
    }

    // --- Header ---
    let overall_icon = match status.overall_status.as_deref() {
        Some("success") => "✓".green().bold().to_string(),
        Some("failure") => "✗".red().bold().to_string(),
        Some("pending") => "●".yellow().bold().to_string(),
        _ => "○".dimmed().to_string(),
    };

    let pr_info = status
        .pr_number
        .map(|n| format!("  PR #{}", n).bright_magenta().to_string())
        .unwrap_or_default();

    let branch_display = if is_current {
        status.branch.bold().to_string()
    } else {
        status.branch.normal().to_string()
    };

    let header = format!(
        "{}  {}{}  {}",
        overall_icon,
        branch_display,
        pr_info,
        format!("({})", status.sha_short).dimmed()
    );

    // Measure visible header width (strip ANSI for length calculation)
    let visible_len = strip_ansi_len(&format!(
        "{}  {}{}  ({})",
        overall_icon_plain(status),
        status.branch,
        status
            .pr_number
            .map(|n| format!("  PR #{}", n))
            .unwrap_or_default(),
        status.sha_short
    ));
    let separator = "━".repeat(visible_len.min(72));

    println!("{}", header);
    println!("{}", separator.dimmed());
    println!();

    // Partition checks
    let failed: Vec<&CheckRunInfo> = status
        .check_runs
        .iter()
        .filter(|c| {
            c.status == "completed"
                && matches!(
                    c.conclusion.as_deref(),
                    Some("failure") | Some("timed_out") | Some("action_required")
                )
        })
        .collect();

    let running: Vec<&CheckRunInfo> = status
        .check_runs
        .iter()
        .filter(|c| {
            matches!(
                c.status.as_str(),
                "in_progress" | "queued" | "waiting" | "requested" | "pending"
            )
        })
        .collect();

    let passed: Vec<&CheckRunInfo> = status
        .check_runs
        .iter()
        .filter(|c| c.status == "completed" && matches!(c.conclusion.as_deref(), Some("success")))
        .collect();

    let skipped: Vec<&CheckRunInfo> = status
        .check_runs
        .iter()
        .filter(|c| {
            c.status == "completed"
                && matches!(
                    c.conclusion.as_deref(),
                    Some("skipped") | Some("neutral") | Some("cancelled")
                )
        })
        .collect();

    // Print failed checks (each on own line)
    if !failed.is_empty() {
        for check in &failed {
            println!("  {} {}", "✗".red().bold(), check.name.red());
        }
        println!();
    }

    // Print running checks (comma-separated)
    if !running.is_empty() {
        let names: Vec<String> = running.iter().map(|c| c.name.clone()).collect();
        println!("  {} {}", "●".yellow().bold(), names.join(", ").yellow());
        println!();
    }

    // Print passed summary
    if !passed.is_empty() {
        // Sort slowest first for the snippet
        let mut sorted_passed = passed.clone();
        sorted_passed.sort_by(|a, b| b.elapsed_secs.cmp(&a.elapsed_secs));

        let show_n = 3.min(sorted_passed.len());
        let snippets: Vec<String> = sorted_passed[..show_n]
            .iter()
            .map(|c| {
                if let Some(secs) = c.elapsed_secs {
                    format!("{} {}", c.name, format_duration(secs))
                } else {
                    c.name.clone()
                }
            })
            .collect();
        let remaining = passed.len().saturating_sub(show_n);
        let detail = if remaining > 0 {
            format!("{}, +{} more", snippets.join(", "), remaining)
        } else {
            snippets.join(", ")
        };

        println!(
            "  {} {}  {}",
            "✓".green(),
            format!("{} passed", passed.len()).green(),
            format!("({})", detail).dimmed()
        );
    }

    // Print skipped summary
    if !skipped.is_empty() {
        println!(
            "  {} {}",
            "⊘".dimmed(),
            format!("{} skipped", skipped.len()).dimmed()
        );
    }

    // Timing / ETA footer
    if let Some(timing) = calculate_branch_timing(repo, &status.check_runs) {
        println!();
        println!(
            "  {}",
            format_timing_footer(&timing, status.overall_status.as_deref())
        );
    }

    println!();
}

/// Verbose single-branch display block (one check per line, aligned)
fn display_branch_verbose(repo: &GitRepo, status: &BranchCiStatus, is_current: bool) {
    if status.check_runs.is_empty() {
        let marker = if is_current { "◉" } else { "○" };
        println!(
            "{}  {}  {}  {}",
            marker,
            status.branch.dimmed(),
            format!("({})", status.sha_short).dimmed(),
            "no CI".dimmed()
        );
        return;
    }

    // --- Header (same as compact) ---
    let overall_icon = match status.overall_status.as_deref() {
        Some("success") => "✓".green().bold().to_string(),
        Some("failure") => "✗".red().bold().to_string(),
        Some("pending") => "●".yellow().bold().to_string(),
        _ => "○".dimmed().to_string(),
    };

    let pr_info = status
        .pr_number
        .map(|n| format!("  PR #{}", n).bright_magenta().to_string())
        .unwrap_or_default();

    let branch_display = if is_current {
        status.branch.bold().to_string()
    } else {
        status.branch.normal().to_string()
    };

    let header = format!(
        "{}  {}{}  {}",
        overall_icon,
        branch_display,
        pr_info,
        format!("({})", status.sha_short).dimmed()
    );

    let visible_len = strip_ansi_len(&format!(
        "{}  {}{}  ({})",
        overall_icon_plain(status),
        status.branch,
        status
            .pr_number
            .map(|n| format!("  PR #{}", n))
            .unwrap_or_default(),
        status.sha_short
    ));
    let separator = "━".repeat(visible_len.min(72));

    println!("{}", header);
    println!("{}", separator.dimmed());
    println!();

    // Compute column widths
    let max_name = status
        .check_runs
        .iter()
        .map(|c| c.name.len())
        .max()
        .unwrap_or(0);

    // Sort: failures first, then running, then passed, then skipped
    let mut sorted = status.check_runs.clone();
    sorted.sort_by_key(check_sort_key);

    // Pre-compute timing strings so we can measure max width for right-alignment
    let timing_cols: Vec<String> = sorted.iter().map(check_timing_text).collect();

    let max_timing = timing_cols.iter().map(|s| s.len()).max().unwrap_or(0);

    for (check, timing_col) in sorted.iter().zip(timing_cols.iter()) {
        let (icon, label) = check_icon_label(check);

        let name_padded = format!("{:<width$}", check.name, width = max_name);
        let timing_padded = format!("{:<width$}", timing_col, width = max_timing);

        if timing_col.is_empty() {
            println!(
                "  {}  {}  {}  {}",
                icon,
                name_padded,
                label,
                " ".repeat(max_timing)
            );
        } else {
            println!(
                "  {}  {}  {}  {}",
                icon,
                name_padded,
                label,
                timing_padded.dimmed()
            );
        }
    }

    // Timing / ETA footer
    if let Some(timing) = calculate_branch_timing(repo, &status.check_runs) {
        println!();
        println!(
            "  {}",
            format_timing_footer(&timing, status.overall_status.as_deref())
        );
    }

    println!();
}

/// Compact display for one or more branches
fn display_ci_compact(repo: &GitRepo, statuses: &[BranchCiStatus], current: &str, multi: bool) {
    if multi {
        print_multi_branch_header(statuses);
        println!();
    }

    for status in statuses {
        let is_current = status.branch == current;
        display_branch_compact(repo, status, is_current);
    }
}

/// Verbose display for one or more branches
fn display_ci_verbose(repo: &GitRepo, statuses: &[BranchCiStatus], current: &str, multi: bool) {
    if multi {
        print_multi_branch_header(statuses);
        println!();
    }

    for status in statuses {
        let is_current = status.branch == current;
        display_branch_verbose(repo, status, is_current);
    }
}

/// One-line dashboard header for multi-branch views
fn print_multi_branch_header(statuses: &[BranchCiStatus]) {
    let total = statuses.len();
    let success = statuses
        .iter()
        .filter(|s| s.overall_status.as_deref() == Some("success"))
        .count();
    let failure = statuses
        .iter()
        .filter(|s| s.overall_status.as_deref() == Some("failure"))
        .count();
    let pending = statuses
        .iter()
        .filter(|s| s.overall_status.as_deref() == Some("pending"))
        .count();
    let no_ci = statuses.iter().filter(|s| s.check_runs.is_empty()).count();

    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("{} branches", total).bold().to_string());
    if success > 0 {
        parts.push(
            format!("{} {}", "✓".green(), format!("{} passing", success).green()).to_string(),
        );
    }
    if failure > 0 {
        parts.push(format!("{} {}", "✗".red(), format!("{} failing", failure).red()).to_string());
    }
    if pending > 0 {
        parts.push(
            format!(
                "{} {}",
                "●".yellow(),
                format!("{} running", pending).yellow()
            )
            .to_string(),
        );
    }
    if no_ci > 0 {
        parts.push(format!("○ {} no CI", no_ci).dimmed().to_string());
    }

    println!("CI  {}", parts.join("  "));
}

/// Record CI history for completed successful checks
pub fn record_ci_history(repo: &GitRepo, statuses: &[BranchCiStatus]) {
    for status in statuses {
        let earliest_start = status
            .check_runs
            .iter()
            .filter_map(|c| c.started_at.as_ref())
            .filter_map(|s| s.parse::<DateTime<Utc>>().ok())
            .min();

        for check in &status.check_runs {
            if check.status == "completed" && check.conclusion.as_deref() == Some("success") {
                if let (Some(elapsed), Some(completed_at)) =
                    (check.elapsed_secs, check.completed_at.as_ref())
                {
                    let end_offset_secs = earliest_start.and_then(|earliest| {
                        completed_at.parse::<DateTime<Utc>>().ok().map(|completed| {
                            completed
                                .signed_duration_since(earliest)
                                .num_seconds()
                                .max(0) as u64
                        })
                    });
                    let _ = history::add_timing_sample(
                        repo,
                        &check.name,
                        elapsed,
                        completed_at.clone(),
                        end_offset_secs,
                    );
                }
            }
        }
    }
}

/// Check if all CI checks are complete (not pending)
fn all_checks_complete(statuses: &[BranchCiStatus]) -> bool {
    statuses.iter().all(|s| {
        // Branches with no CI configured are considered "done" (nothing to wait for)
        s.check_runs.is_empty()
            || s.check_runs
                .iter()
                .all(|check| check_run_is_terminal(check.status.as_str()))
    })
}

fn check_run_is_terminal(status: &str) -> bool {
    !matches!(
        status,
        "queued" | "in_progress" | "waiting" | "requested" | "pending"
    )
}

fn ci_watch_should_exit(statuses: &[BranchCiStatus], strict: bool) -> bool {
    (strict && has_ci_failure(statuses)) || all_checks_complete(statuses)
}

fn has_ci_failure(statuses: &[BranchCiStatus]) -> bool {
    statuses
        .iter()
        .any(|s| s.overall_status.as_deref() == Some("failure"))
}

fn resolve_ci_alert_sounds(
    config: &Config,
    alert_arg: Option<CiAlertSoundArg>,
    no_alert: bool,
) -> Option<CiAlertSounds> {
    if no_alert {
        return None;
    }

    match alert_arg {
        Some(CiAlertSoundArg::DefaultSound) => Some(CiAlertSounds::built_in()),
        Some(CiAlertSoundArg::Path(path)) => Some(CiAlertSounds {
            success: CiAlertSound::CustomPath(path.clone()),
            error: CiAlertSound::CustomPath(path),
        }),
        None if config.ci.alert => Some(CiAlertSounds {
            success: config_alert_sound(&config.ci.success_alert_sound, BuiltInSound::Success),
            error: config_alert_sound(&config.ci.error_alert_sound, BuiltInSound::Error),
        }),
        None => None,
    }
}

impl CiAlertSounds {
    fn built_in() -> Self {
        Self {
            success: CiAlertSound::BuiltIn(BuiltInSound::Success),
            error: CiAlertSound::BuiltIn(BuiltInSound::Error),
        }
    }

    fn for_outcome(&self, outcome: CiAlertOutcome) -> &CiAlertSound {
        match outcome {
            CiAlertOutcome::Success => &self.success,
            CiAlertOutcome::Error => &self.error,
        }
    }
}

fn config_alert_sound(path: &Option<String>, built_in: BuiltInSound) -> CiAlertSound {
    path.as_ref()
        .map(|path| CiAlertSound::CustomPath(PathBuf::from(path)))
        .unwrap_or(CiAlertSound::BuiltIn(built_in))
}

fn play_ci_alert(alert: Option<&CiAlertSounds>, outcome: CiAlertOutcome) {
    let Some(alert) = alert else {
        return;
    };

    let sound = match alert.for_outcome(outcome) {
        CiAlertSound::BuiltIn(kind) => Sound::BuiltIn(*kind),
        CiAlertSound::CustomPath(path) => Sound::Path(path.clone()),
    };

    if let Err(err) = notifications::play_sound(&sound) {
        eprintln!(
            "{} Could not play CI alert sound: {}",
            "warning:".yellow().bold(),
            err
        );
    }
}

/// Run watch mode - poll CI status until all checks complete
#[allow(clippy::too_many_arguments)]
fn run_watch_mode(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    stack: &Stack,
    branches_to_check: &[String],
    current: &str,
    interval: u64,
    json: bool,
    verbose: bool,
    alert: Option<CiAlertSounds>,
    strict: bool,
) -> Result<()> {
    let poll_duration = Duration::from_secs(interval);
    let mut iteration = 0;

    println!("{}", "Watching CI status (Ctrl+C to stop)...".cyan().bold());
    println!();

    loop {
        // Safety valve: if there are no branches to watch, exit immediately
        if branches_to_check.is_empty() {
            println!("{}", "No tracked branches to watch.".dimmed());
            return Ok(());
        }
        iteration += 1;

        let statuses = fetch_ci_statuses(repo, rt, client, stack, branches_to_check)?;
        update_ci_cache(repo, stack, &statuses);

        if iteration > 1 {
            print!("\x1B[2J\x1B[H");
            let _ = std::io::stdout().flush();
            println!("{}", "Watching CI status (Ctrl+C to stop)...".cyan().bold());
            println!();
        }

        if json {
            println!("{}", serde_json::to_string_pretty(&statuses)?);
        } else {
            let multi = statuses.len() > 1;
            if verbose {
                display_ci_compact(repo, &statuses, current, multi);
            } else {
                display_ci_verbose(repo, &statuses, current, multi);
            }
        }

        let failed = has_ci_failure(&statuses);
        let complete = ci_watch_should_exit(&statuses, strict);

        if complete {
            println!();
            let width = 50;
            let line = "═".repeat(width);
            if failed {
                let failed_branch = statuses
                    .iter()
                    .find(|s| s.overall_status.as_deref() == Some("failure"))
                    .map(|s| s.branch.as_str())
                    .unwrap_or("a branch");
                println!("{}", line.red());
                if iteration == 1 {
                    println!(
                        "{}",
                        format!(" ✗  CI already finished — failed on {}", failed_branch)
                            .red()
                            .bold()
                    );
                } else {
                    println!(
                        "{}",
                        format!(" ✗  CI failed on {}", failed_branch).red().bold()
                    );
                }
                println!("{}", line.red());
            } else {
                println!("{}", line.green());
                if iteration == 1 {
                    println!(
                        "{}",
                        " ✓  CI already finished — all checks passed".green().bold()
                    );
                } else {
                    println!("{}", " ✓  All CI checks passed".green().bold());
                }
                println!("{}", line.green());
            }

            record_ci_history(repo, &statuses);
            let alert_outcome = if failed {
                CiAlertOutcome::Error
            } else {
                CiAlertOutcome::Success
            };
            play_ci_alert(alert.as_ref(), alert_outcome);
            return Ok(());
        }

        if !json {
            println!(
                "{}",
                format!("Refreshing in {}s... (iteration #{})", interval, iteration).dimmed()
            );
        }

        std::thread::sleep(poll_duration);
    }
}

/// Format duration in seconds to human-readable string
fn format_duration(secs: u64) -> String {
    match secs {
        0..60 => format!("{}s", secs),
        60..3600 => {
            let mins = secs / 60;
            let secs_remainder = secs % 60;
            if secs_remainder == 0 {
                format!("{}m", mins)
            } else {
                format!("{}m {}s", mins, secs_remainder)
            }
        }
        _ => {
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            if mins == 0 {
                format!("{}h", hours)
            } else {
                format!("{}h {}m", hours, mins)
            }
        }
    }
}

pub(crate) fn update_ci_cache(repo: &GitRepo, stack: &Stack, statuses: &[BranchCiStatus]) {
    let git_dir = match repo.git_dir() {
        Ok(path) => path,
        Err(_) => return,
    };

    let mut cache = CiCache::load(git_dir);
    for status in statuses {
        let pr_state = status.pr_is_draft.map(|is_draft| {
            if is_draft { "DRAFT".to_string() } else { "OPEN".to_string() }
        });
        cache.update(&status.branch, status.overall_status.clone(), pr_state);
    }

    let valid_branches: Vec<String> = stack.branches.keys().cloned().collect();
    cache.cleanup(&valid_branches);
    cache.mark_refreshed();
    let _ = cache.save(git_dir);
}

/// Fetch all checks (both check runs and commit statuses), deduplicated
pub async fn fetch_github_checks(
    repo: &GitRepo,
    client: &crate::github::GitHubClient,
    commit_sha: &str,
) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
    let (check_runs_overall, mut all_checks) = fetch_check_runs(repo, client, commit_sha).await?;
    let (statuses_overall, status_checks) = fetch_commit_statuses(repo, client, commit_sha).await?;

    all_checks.extend(status_checks);

    // Deduplicate across both sources, keeping most recent per name
    all_checks = dedup_check_runs(all_checks);

    let combined_overall = match (check_runs_overall, statuses_overall) {
        (Some(ref a), Some(ref b)) if a == "failure" || b == "failure" => {
            Some("failure".to_string())
        }
        (Some(ref a), Some(ref b)) if a == "pending" || b == "pending" => {
            Some("pending".to_string())
        }
        (Some(a), Some(_)) => Some(a),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    Ok((combined_overall, all_checks))
}

/// Fetch commit statuses (older CI systems like Buildkite, CircleCI, etc.)
async fn fetch_commit_statuses(
    repo: &GitRepo,
    client: &GitHubClient,
    commit_sha: &str,
) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
    let url = format!(
        "/repos/{}/{}/commits/{}/statuses",
        client.owner, client.repo, commit_sha
    );

    let statuses: Vec<CommitStatus> = match client.octocrab.get(&url, None::<&()>).await {
        Ok(s) => s,
        Err(_) => return Ok((None, Vec::new())),
    };

    if statuses.is_empty() {
        return Ok((None, Vec::new()));
    }

    let check_runs = normalize_commit_statuses(repo, statuses, Utc::now());

    let mut has_pending = false;
    let mut has_failure = false;
    let mut all_success = true;

    for run in &check_runs {
        match run.status.as_str() {
            "completed" => match run.conclusion.as_deref() {
                Some("success") => {}
                Some("failure") | Some("error") => {
                    has_failure = true;
                    all_success = false;
                }
                _ => {
                    all_success = false;
                }
            },
            "in_progress" | "queued" | "pending" => {
                has_pending = true;
                all_success = false;
            }
            _ => {
                all_success = false;
            }
        }
    }

    let overall = if has_failure {
        Some("failure".to_string())
    } else if has_pending {
        Some("pending".to_string())
    } else if all_success && !check_runs.is_empty() {
        Some("success".to_string())
    } else {
        None
    };

    Ok((overall, check_runs))
}

fn normalize_commit_statuses(
    repo: &GitRepo,
    statuses: Vec<CommitStatus>,
    now: DateTime<Utc>,
) -> Vec<CheckRunInfo> {
    let mut by_context: HashMap<String, Vec<CommitStatus>> = HashMap::new();
    for status in statuses {
        by_context
            .entry(status.context.clone())
            .or_default()
            .push(status);
    }

    let mut check_runs = Vec::new();
    for (context, events) in by_context {
        if let Some(check_run) = normalize_commit_status_context(repo, &context, &events, now) {
            check_runs.push(check_run);
        }
    }

    check_runs.sort_by(|a, b| a.name.cmp(&b.name));
    check_runs
}

fn normalize_commit_status_context(
    repo: &GitRepo,
    context: &str,
    events: &[CommitStatus],
    now: DateTime<Utc>,
) -> Option<CheckRunInfo> {
    let latest = events
        .iter()
        .max_by_key(|status| commit_status_event_time(status))?;
    let latest_time = commit_status_event_time(latest)?;

    let average_secs = match history::load_check_history(repo, context) {
        Ok(hist) => history::calculate_average(&hist),
        Err(_) => None,
    };

    let pending_start = events
        .iter()
        .filter(|status| status.state == "pending")
        .filter_map(commit_status_event_time)
        .filter(|time| *time <= latest_time)
        .max();

    let (status, conclusion, started_at, completed_at, elapsed_secs) = match latest.state.as_str() {
        "success" => (
            "completed".to_string(),
            Some("success".to_string()),
            pending_start.map(|time| time.to_rfc3339()),
            Some(latest_time.to_rfc3339()),
            pending_start
                .map(|time| latest_time.signed_duration_since(time).num_seconds().max(0) as u64),
        ),
        "failure" | "error" => (
            "completed".to_string(),
            Some("failure".to_string()),
            pending_start.map(|time| time.to_rfc3339()),
            Some(latest_time.to_rfc3339()),
            pending_start
                .map(|time| latest_time.signed_duration_since(time).num_seconds().max(0) as u64),
        ),
        "pending" => (
            "in_progress".to_string(),
            None,
            Some(latest_time.to_rfc3339()),
            None,
            Some(now.signed_duration_since(latest_time).num_seconds().max(0) as u64),
        ),
        _ => (
            "queued".to_string(),
            None,
            latest.created_at.clone(),
            latest.updated_at.clone(),
            None,
        ),
    };

    let completion_percent = if status == "in_progress" {
        if let (Some(elapsed), Some(avg)) = (elapsed_secs, average_secs) {
            if avg > 0 {
                Some(((elapsed * 100) / avg).min(99) as u8)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    Some(CheckRunInfo {
        name: context.to_string(),
        status,
        conclusion,
        url: latest.target_url.clone(),
        started_at,
        completed_at,
        elapsed_secs,
        average_secs,
        completion_percent,
    })
}

fn commit_status_event_time(status: &CommitStatus) -> Option<DateTime<Utc>> {
    status
        .created_at
        .as_deref()
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        .or_else(|| {
            status
                .updated_at
                .as_deref()
                .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        })
}

async fn fetch_check_runs(
    repo: &GitRepo,
    client: &GitHubClient,
    commit_sha: &str,
) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
    let url = format!(
        "/repos/{}/{}/commits/{}/check-runs",
        client.owner, client.repo, commit_sha
    );

    let response: CheckRunsResponse = client.octocrab.get(&url, None::<&()>).await?;

    if response.total_count == 0 {
        return Ok((None, Vec::new()));
    }

    let now = Utc::now();
    let mut check_runs: Vec<CheckRunInfo> = Vec::new();

    for r in response.check_runs {
        let (elapsed_secs, completed_at_str) = if let Some(completed) = &r.completed_at {
            if let (Some(started), Ok(completed_time)) = (
                r.started_at
                    .as_ref()
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok()),
                completed.parse::<DateTime<Utc>>(),
            ) {
                let duration = completed_time.signed_duration_since(started);
                let secs = duration.num_seconds();
                if secs >= 0 {
                    (Some(secs as u64), Some(completed.clone()))
                } else {
                    (None, Some(completed.clone()))
                }
            } else {
                (None, Some(completed.clone()))
            }
        } else if let Some(started) = &r.started_at {
            if let Ok(started_time) = started.parse::<DateTime<Utc>>() {
                let duration = now.signed_duration_since(started_time);
                let secs = duration.num_seconds();
                if secs >= 0 {
                    (Some(secs as u64), None)
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let average_secs = match history::load_check_history(repo, &r.name) {
            Ok(hist) => history::calculate_average(&hist),
            Err(_) => None,
        };

        let completion_percent = if r.status == "in_progress" {
            if let (Some(elapsed), Some(avg)) = (elapsed_secs, average_secs) {
                if avg > 0 {
                    let pct: u64 = ((elapsed * 100) / avg).min(99);
                    Some(pct as u8)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        check_runs.push(CheckRunInfo {
            name: r.name,
            status: r.status,
            conclusion: r.conclusion,
            url: r.html_url,
            started_at: r.started_at,
            completed_at: completed_at_str,
            elapsed_secs,
            average_secs,
            completion_percent,
        });
    }

    // Deduplicate within check runs
    check_runs = dedup_check_runs(check_runs);

    let mut has_pending = false;
    let mut has_failure = false;
    let mut all_success = true;

    for run in &check_runs {
        match run.status.as_str() {
            "completed" => match run.conclusion.as_deref() {
                Some("success") | Some("skipped") | Some("neutral") | Some("cancelled") => {}
                Some("failure") | Some("timed_out") | Some("action_required") => {
                    has_failure = true;
                    all_success = false;
                }
                _ => {
                    all_success = false;
                }
            },
            "queued" | "in_progress" | "waiting" | "requested" | "pending" => {
                has_pending = true;
                all_success = false;
            }
            _ => {
                all_success = false;
            }
        }
    }

    let overall = if has_failure {
        Some("failure".to_string())
    } else if has_pending {
        Some("pending".to_string())
    } else if all_success {
        Some("success".to_string())
    } else {
        Some("pending".to_string())
    };

    Ok((overall, check_runs))
}

// --- Small helpers ---

fn overall_icon_plain(status: &BranchCiStatus) -> &'static str {
    match status.overall_status.as_deref() {
        Some("success") => "✓",
        Some("failure") => "✗",
        Some("pending") => "●",
        _ => "○",
    }
}

/// Approximate visible character width (strips ANSI escapes)
fn strip_ansi_len(s: &str) -> usize {
    // Simple state machine: skip ESC[ ... m sequences
    let mut len = 0;
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            if ch == 'm' {
                in_escape = false;
            }
        } else if ch == '\x1B' {
            in_escape = true;
        } else {
            len += 1;
        }
    }
    len
}

/// Sort key: failures first (0), running (1), passed (2), skipped (3)
fn check_sort_key(c: &CheckRunInfo) -> u8 {
    match c.status.as_str() {
        "completed" => match c.conclusion.as_deref() {
            Some("failure") | Some("timed_out") | Some("action_required") => 0,
            Some("success") => 2,
            _ => 3,
        },
        "in_progress" | "queued" | "waiting" | "requested" | "pending" => 1,
        _ => 3,
    }
}

/// Icon and label string for a check in verbose mode
fn check_icon_label(check: &CheckRunInfo) -> (String, String) {
    match check.status.as_str() {
        "completed" => match check.conclusion.as_deref() {
            Some("success") => ("✓".green().to_string(), "passed".green().to_string()),
            Some("failure") => (
                "✗".red().bold().to_string(),
                "failed".red().bold().to_string(),
            ),
            Some("skipped") => ("⊘".dimmed().to_string(), "skipped".dimmed().to_string()),
            Some("neutral") => ("○".dimmed().to_string(), "neutral".dimmed().to_string()),
            Some("cancelled") => ("⊘".yellow().to_string(), "cancelled".yellow().to_string()),
            Some("timed_out") => ("⏱".red().to_string(), "timed out".red().to_string()),
            Some("action_required") => (
                "!".yellow().to_string(),
                "action required".yellow().to_string(),
            ),
            Some(other) => ("?".dimmed().to_string(), other.dimmed().to_string()),
            None => ("?".dimmed().to_string(), "unknown".dimmed().to_string()),
        },
        "queued" | "waiting" | "requested" => ("◎".cyan().to_string(), "queued".cyan().to_string()),
        "in_progress" => ("●".yellow().to_string(), "running".yellow().to_string()),
        "pending" => ("●".yellow().to_string(), "pending".yellow().to_string()),
        _ => ("?".dimmed().to_string(), check.status.dimmed().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::stack::StackBranch;
    use crate::engine::Stack;
    use crate::forge::ForgeClient;
    use crate::git::GitRepo;
    use crate::github::GitHubClient;
    use chrono::TimeZone;
    use octocrab::Octocrab;
    use std::collections::HashMap;
    use std::process::Command;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn init_temp_repo() -> (TempDir, GitRepo) {
        let tempdir = TempDir::new().unwrap();
        let status = Command::new("git")
            .args(["init"])
            .current_dir(tempdir.path())
            .status()
            .unwrap();
        assert!(status.success());

        let repo = GitRepo::open_from_path(tempdir.path()).unwrap();
        (tempdir, repo)
    }

    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    /// Two local branches `b1` and `b2` (each one commit ahead of `main`) with distinct SHAs.
    fn git_repo_with_two_branches() -> (TempDir, GitRepo, String, String) {
        let tempdir = TempDir::new().unwrap();
        let dir = tempdir.path();
        let run = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(dir)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "ci-test@stax.local"]);
        run(&["config", "user.name", "ci-test"]);
        std::fs::write(dir.join("marker"), "0").unwrap();
        run(&["add", "marker"]);
        run(&["commit", "-m", "init"]);
        run(&["checkout", "-b", "b1"]);
        std::fs::write(dir.join("marker"), "1").unwrap();
        run(&["add", "marker"]);
        run(&["commit", "-m", "on-b1"]);
        run(&["checkout", "main"]);
        run(&["checkout", "-b", "b2"]);
        std::fs::write(dir.join("marker"), "2").unwrap();
        run(&["add", "marker"]);
        run(&["commit", "-m", "on-b2"]);
        let repo = GitRepo::open_from_path(dir).unwrap();
        let sha_b1 = repo.branch_commit("b1").unwrap();
        let sha_b2 = repo.branch_commit("b2").unwrap();
        (tempdir, repo, sha_b1, sha_b2)
    }

    fn test_stack_for_ci_fetch(pr_a: u64, pr_b: u64) -> Stack {
        let mut branches = HashMap::new();
        branches.insert(
            "b1".to_string(),
            StackBranch {
                name: "b1".to_string(),
                parent: Some("main".to_string()),
                parent_revision: None,
                children: Vec::new(),
                needs_restack: false,
                pr_number: Some(pr_a),
                pr_state: None,
                pr_is_draft: None,
            },
        );
        branches.insert(
            "b2".to_string(),
            StackBranch {
                name: "b2".to_string(),
                parent: Some("main".to_string()),
                parent_revision: None,
                children: Vec::new(),
                needs_restack: false,
                pr_number: Some(pr_b),
                pr_state: None,
                pr_is_draft: None,
            },
        );
        Stack {
            branches,
            trunk: "main".to_string(),
        }
    }

    fn pr_json(number: u64, is_draft: bool) -> serde_json::Value {
        serde_json::json!({
            "url": format!("https://api.github.com/repos/test-owner/test-repo/pulls/{number}"),
            "id": number,
            "number": number,
            "state": "open",
            "draft": is_draft,
            "head": { "ref": "head", "sha": "aaa", "label": "test-owner:head" },
            "base": { "ref": "main", "sha": "bbb" }
        })
    }

    fn check_runs_body(check_name: &str) -> serde_json::Value {
        serde_json::json!({
            "total_count": 1,
            "check_runs": [
                {
                    "id": 1,
                    "name": check_name,
                    "status": "completed",
                    "conclusion": "success",
                    "html_url": null,
                    "started_at": "2026-01-01T00:00:00Z",
                    "completed_at": "2026-01-01T00:01:00Z"
                }
            ]
        })
    }

    async fn mount_github_ci_mocks(server: &MockServer, sha_b1: &str, sha_b2: &str) {
        let path_b1 = format!("/repos/test-owner/test-repo/commits/{sha_b1}/check-runs");
        let path_b2 = format!("/repos/test-owner/test-repo/commits/{sha_b2}/check-runs");

        Mock::given(method("GET"))
            .and(path(path_b1.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(check_runs_body("ci-b1")))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(path_b2.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(check_runs_body("ci-b2")))
            .mount(server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/201"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pr_json(201, false)))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/202"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pr_json(202, true)))
            .mount(server)
            .await;
    }

    #[test]
    fn test_render_progress_bar_empty() {
        assert_eq!(render_progress_bar(0, 10), "▱▱▱▱▱▱▱▱▱▱");
    }

    #[test]
    fn test_render_progress_bar_half() {
        assert_eq!(render_progress_bar(50, 10), "▰▰▰▰▰▱▱▱▱▱");
    }

    #[test]
    fn test_render_progress_bar_full() {
        // 99% of 10 blocks = floor(9.9) = 9 filled, 1 empty
        assert_eq!(render_progress_bar(99, 10), "▰▰▰▰▰▰▰▰▰▱");
        // 100% fills all blocks
        assert_eq!(render_progress_bar(100, 10), "▰▰▰▰▰▰▰▰▰▰");
    }

    fn test_check(name: &str, status: &str, conclusion: Option<&str>) -> CheckRunInfo {
        CheckRunInfo {
            name: name.to_string(),
            status: status.to_string(),
            conclusion: conclusion.map(str::to_string),
            url: None,
            started_at: None,
            completed_at: None,
            elapsed_secs: None,
            average_secs: None,
            completion_percent: None,
        }
    }

    fn test_branch_status(overall_status: &str, check_runs: Vec<CheckRunInfo>) -> BranchCiStatus {
        BranchCiStatus {
            branch: "feature".to_string(),
            sha: "0123456789abcdef".to_string(),
            sha_short: "0123456".to_string(),
            overall_status: Some(overall_status.to_string()),
            check_runs,
            pr_number: Some(123),
            pr_is_draft: None,
        }
    }

    #[test]
    fn ci_watch_waits_for_running_checks_after_failure() {
        let statuses = vec![test_branch_status(
            "failure",
            vec![
                test_check("codeowners", "completed", Some("action_required")),
                test_check("integration", "in_progress", None),
            ],
        )];

        assert!(!all_checks_complete(&statuses));
    }

    #[test]
    fn ci_watch_strict_exits_on_failure_before_running_checks_complete() {
        let statuses = vec![test_branch_status(
            "failure",
            vec![
                test_check("codeowners", "completed", Some("action_required")),
                test_check("integration", "in_progress", None),
            ],
        )];

        assert!(ci_watch_should_exit(&statuses, true));
    }

    #[test]
    fn ci_watch_exits_after_all_checks_are_terminal_with_failure() {
        let statuses = vec![test_branch_status(
            "failure",
            vec![
                test_check("codeowners", "completed", Some("action_required")),
                test_check("integration", "completed", Some("success")),
            ],
        )];

        assert!(ci_watch_should_exit(&statuses, false));
    }

    #[test]
    fn test_dedup_check_runs_keeps_most_recent() {
        let older = CheckRunInfo {
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            url: None,
            started_at: Some("2026-01-16T12:00:00Z".to_string()),
            completed_at: Some("2026-01-16T12:02:00Z".to_string()),
            elapsed_secs: Some(120),
            average_secs: None,
            completion_percent: None,
        };
        let newer = CheckRunInfo {
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
            url: None,
            started_at: Some("2026-01-16T13:00:00Z".to_string()),
            completed_at: Some("2026-01-16T13:02:00Z".to_string()),
            elapsed_secs: Some(120),
            average_secs: None,
            completion_percent: None,
        };

        let result = dedup_check_runs(vec![older, newer]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].conclusion, Some("failure".to_string()));
    }

    #[test]
    fn test_dedup_check_runs_different_names() {
        let build = CheckRunInfo {
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            url: None,
            started_at: None,
            completed_at: None,
            elapsed_secs: None,
            average_secs: None,
            completion_percent: None,
        };
        let test = CheckRunInfo {
            name: "test".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            url: None,
            started_at: None,
            completed_at: None,
            elapsed_secs: None,
            average_secs: None,
            completion_percent: None,
        };

        let result = dedup_check_runs(vec![build, test]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn ci_alert_resolution_prefers_cli_path_over_config() {
        let mut config = Config::default();
        config.ci.alert = true;
        config.ci.success_alert_sound = Some("/tmp/config-success.wav".to_string());
        config.ci.error_alert_sound = Some("/tmp/config-error.wav".to_string());

        let alert = resolve_ci_alert_sounds(
            &config,
            Some(CiAlertSoundArg::Path("/tmp/cli.wav".into())),
            false,
        );

        assert_eq!(
            alert,
            Some(CiAlertSounds {
                success: CiAlertSound::CustomPath("/tmp/cli.wav".into()),
                error: CiAlertSound::CustomPath("/tmp/cli.wav".into()),
            })
        );
    }

    #[test]
    fn ci_alert_resolution_uses_built_in_defaults() {
        let mut config = Config::default();
        config.ci.alert = true;

        let alert = resolve_ci_alert_sounds(&config, None, false);

        assert_eq!(
            alert,
            Some(CiAlertSounds {
                success: CiAlertSound::BuiltIn(BuiltInSound::Success),
                error: CiAlertSound::BuiltIn(BuiltInSound::Error),
            })
        );
    }

    #[test]
    fn ci_alert_resolution_uses_config_success_and_error_sounds() {
        let mut config = Config::default();
        config.ci.alert = true;
        config.ci.success_alert_sound = Some("/tmp/success.wav".to_string());
        config.ci.error_alert_sound = Some("/tmp/error.wav".to_string());

        let alert = resolve_ci_alert_sounds(&config, None, false);

        assert_eq!(
            alert,
            Some(CiAlertSounds {
                success: CiAlertSound::CustomPath("/tmp/success.wav".into()),
                error: CiAlertSound::CustomPath("/tmp/error.wav".into()),
            })
        );
    }

    #[test]
    fn ci_alert_resolution_defaults_missing_error_sound_only() {
        let mut config = Config::default();
        config.ci.alert = true;
        config.ci.success_alert_sound = Some("/tmp/success.wav".to_string());

        let alert = resolve_ci_alert_sounds(&config, None, false);

        assert_eq!(
            alert,
            Some(CiAlertSounds {
                success: CiAlertSound::CustomPath("/tmp/success.wav".into()),
                error: CiAlertSound::BuiltIn(BuiltInSound::Error),
            })
        );
    }

    #[test]
    fn ci_alert_resolution_no_alert_disables_config() {
        let mut config = Config::default();
        config.ci.alert = true;
        config.ci.success_alert_sound = Some("/tmp/config-success.wav".to_string());
        config.ci.error_alert_sound = Some("/tmp/config-error.wav".to_string());

        let alert = resolve_ci_alert_sounds(&config, None, true);

        assert_eq!(alert, None);
    }

    #[test]
    fn test_check_run_info_serialization() {
        let info = CheckRunInfo {
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            url: Some("https://github.com/test/test/runs/123".to_string()),
            started_at: Some("2026-01-16T12:00:00Z".to_string()),
            completed_at: Some("2026-01-16T12:02:30Z".to_string()),
            elapsed_secs: Some(150),
            average_secs: Some(160),
            completion_percent: None,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("build"));
        assert!(json.contains("completed"));
        assert!(json.contains("success"));

        let deserialized: CheckRunInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "build");
        assert_eq!(deserialized.status, "completed");
        assert_eq!(deserialized.conclusion, Some("success".to_string()));
        assert_eq!(deserialized.elapsed_secs, Some(150));
    }

    #[test]
    fn test_check_run_info_without_url() {
        let info = CheckRunInfo {
            name: "test".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            url: None,
            started_at: Some("2026-01-16T12:00:00Z".to_string()),
            completed_at: None,
            elapsed_secs: Some(120),
            average_secs: Some(180),
            completion_percent: Some(66),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("url"));
        assert!(json.contains("test"));
        assert!(json.contains("in_progress"));
        assert!(json.contains("120"));
        assert!(json.contains("66"));
    }

    #[test]
    fn test_branch_ci_status_serialization() {
        let status = BranchCiStatus {
            branch: "feature-branch".to_string(),
            sha: "abc123def456".to_string(),
            sha_short: "abc123d".to_string(),
            overall_status: Some("success".to_string()),
            check_runs: vec![CheckRunInfo {
                name: "build".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                url: None,
                started_at: None,
                completed_at: None,
                elapsed_secs: None,
                average_secs: None,
                completion_percent: None,
            }],
            pr_number: Some(42),
            pr_is_draft: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("feature-branch"));
        assert!(json.contains("abc123def456"));
        assert!(json.contains("abc123d"));
        assert!(json.contains("success"));
        assert!(json.contains("build"));
        assert!(json.contains("42"));
    }

    #[test]
    fn test_branch_ci_status_without_pr() {
        let status = BranchCiStatus {
            branch: "no-pr-branch".to_string(),
            sha: "xyz789".to_string(),
            sha_short: "xyz789".to_string(),
            overall_status: None,
            check_runs: vec![],
            pr_number: None,
            pr_is_draft: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("no-pr-branch"));
        assert!(json.contains("null"));
    }

    #[test]
    fn test_check_runs_response_deserialization() {
        let json = r#"{
            "total_count": 2,
            "check_runs": [
                {"name": "build", "status": "completed", "conclusion": "success", "html_url": "https://example.com/1"},
                {"name": "test", "status": "in_progress", "conclusion": null, "html_url": null}
            ]
        }"#;

        let response: CheckRunsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total_count, 2);
        assert_eq!(response.check_runs.len(), 2);
        assert_eq!(response.check_runs[0].name, "build");
        assert_eq!(
            response.check_runs[0].conclusion,
            Some("success".to_string())
        );
        assert_eq!(response.check_runs[1].name, "test");
        assert_eq!(response.check_runs[1].conclusion, None);
    }

    #[test]
    fn test_check_run_detail_deserialization() {
        let json = r#"{"name": "lint", "status": "queued", "conclusion": null, "html_url": "https://example.com", "started_at": "2026-01-16T12:00:00Z", "completed_at": null}"#;

        let detail: CheckRunDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.name, "lint");
        assert_eq!(detail.status, "queued");
        assert_eq!(detail.conclusion, None);
        assert_eq!(detail.html_url, Some("https://example.com".to_string()));
        assert_eq!(detail.started_at, Some("2026-01-16T12:00:00Z".to_string()));
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(150), "2m 30s");
        assert_eq!(format_duration(3599), "59m 59s");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3660), "1h 1m");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(7320), "2h 2m");
    }

    #[test]
    fn test_check_run_info_with_timing() {
        let info = CheckRunInfo {
            name: "build".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            url: None,
            started_at: Some("2026-01-16T12:00:00Z".to_string()),
            completed_at: None,
            elapsed_secs: Some(90),
            average_secs: Some(120),
            completion_percent: Some(75),
        };

        assert_eq!(info.elapsed_secs, Some(90));
        assert_eq!(info.average_secs, Some(120));
        assert_eq!(info.completion_percent, Some(75));

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("90"));
        assert!(json.contains("120"));
        assert!(json.contains("75"));
    }

    #[test]
    fn test_check_timing_text_running_without_elapsed_still_shows_average() {
        let info = CheckRunInfo {
            name: "android suite".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            url: None,
            started_at: None,
            completed_at: None,
            elapsed_secs: None,
            average_secs: Some(1500),
            completion_percent: None,
        };

        assert_eq!(check_timing_text(&info), "avg: 25m");
    }

    #[test]
    fn test_format_timing_footer_running_includes_average() {
        let timing = BranchTiming {
            elapsed_secs: 1800,
            average_secs: Some(1500),
            is_complete: false,
            pct: Some(99),
        };

        let footer = format_timing_footer(&timing, Some("pending"));
        assert!(footer.contains("avg: 25m"), "footer was: {footer}");
        assert!(footer.contains("overdue"), "footer was: {footer}");
    }

    #[test]
    fn test_normalize_commit_status_context_pending_tracks_elapsed_from_created_at() {
        let (_tempdir, repo) = init_temp_repo();
        history::add_timing_sample(
            &repo,
            "android suite",
            1500,
            "2026-01-16T11:00:00Z".to_string(),
            None,
        )
        .unwrap();

        let now = Utc.with_ymd_and_hms(2026, 1, 16, 12, 25, 0).unwrap();
        let events = vec![CommitStatus {
            context: "android suite".to_string(),
            state: "pending".to_string(),
            target_url: None,
            created_at: Some("2026-01-16T12:00:00Z".to_string()),
            updated_at: Some("2026-01-16T12:00:00Z".to_string()),
        }];

        let run = normalize_commit_status_context(&repo, "android suite", &events, now).unwrap();
        assert_eq!(run.status, "in_progress");
        assert_eq!(run.elapsed_secs, Some(1500));
        assert_eq!(run.average_secs, Some(1500));
        assert_eq!(run.completion_percent, Some(99));
    }

    #[test]
    fn test_normalize_commit_status_context_uses_pending_to_success_duration() {
        let (_tempdir, repo) = init_temp_repo();
        let now = Utc.with_ymd_and_hms(2026, 1, 16, 12, 30, 0).unwrap();
        let events = vec![
            CommitStatus {
                context: "android suite".to_string(),
                state: "pending".to_string(),
                target_url: Some("https://example.com/pending".to_string()),
                created_at: Some("2026-01-16T12:00:00Z".to_string()),
                updated_at: Some("2026-01-16T12:00:00Z".to_string()),
            },
            CommitStatus {
                context: "android suite".to_string(),
                state: "success".to_string(),
                target_url: Some("https://example.com/success".to_string()),
                created_at: Some("2026-01-16T12:25:00Z".to_string()),
                updated_at: Some("2026-01-16T12:25:00Z".to_string()),
            },
        ];

        let run = normalize_commit_status_context(&repo, "android suite", &events, now).unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(run.conclusion.as_deref(), Some("success"));
        assert_eq!(run.started_at.as_deref(), Some("2026-01-16T12:00:00+00:00"));
        assert_eq!(
            run.completed_at.as_deref(),
            Some("2026-01-16T12:25:00+00:00")
        );
        assert_eq!(run.elapsed_secs, Some(1500));
        assert_eq!(run.url.as_deref(), Some("https://example.com/success"));
    }

    #[test]
    fn test_check_run_info_completed_with_timing() {
        let info = CheckRunInfo {
            name: "test".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            url: None,
            started_at: Some("2026-01-16T12:00:00Z".to_string()),
            completed_at: Some("2026-01-16T12:02:00Z".to_string()),
            elapsed_secs: Some(120),
            average_secs: Some(110),
            completion_percent: None,
        };

        assert_eq!(info.elapsed_secs, Some(120));
        assert_eq!(info.average_secs, Some(110));
        assert_eq!(info.completion_percent, None);

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("120"));
        assert!(json.contains("110"));
        assert!(!json.contains("completion_percent"));
    }

    #[test]
    fn test_check_sort_key_ordering() {
        let failed = CheckRunInfo {
            name: "a".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
            url: None,
            started_at: None,
            completed_at: None,
            elapsed_secs: None,
            average_secs: None,
            completion_percent: None,
        };
        let running = CheckRunInfo {
            name: "b".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            url: None,
            started_at: None,
            completed_at: None,
            elapsed_secs: None,
            average_secs: None,
            completion_percent: None,
        };
        let passed = CheckRunInfo {
            name: "c".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            url: None,
            started_at: None,
            completed_at: None,
            elapsed_secs: None,
            average_secs: None,
            completion_percent: None,
        };

        assert!(check_sort_key(&failed) < check_sort_key(&running));
        assert!(check_sort_key(&running) < check_sort_key(&passed));
    }

    #[test]
    fn fetch_ci_statuses_merges_github_data_per_branch_sorted() {
        ensure_crypto_provider();
        let (_td, repo, sha_b1, sha_b2) = git_repo_with_two_branches();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mock_server = MockServer::start().await;
            mount_github_ci_mocks(&mock_server, &sha_b1, &sha_b2).await;

            let octocrab = Octocrab::builder()
                .base_uri(mock_server.uri())
                .unwrap()
                .personal_token("test-token".to_string())
                .build()
                .unwrap();
            let gh = GitHubClient::with_octocrab(octocrab, "test-owner", "test-repo");
            let client = ForgeClient::GitHub(gh);
            let stack = test_stack_for_ci_fetch(201, 202);

            // Input order is intentionally not alphabetical; output is sorted by branch name.
            // `missing-branch` is skipped (no local ref).
            let branches = vec![
                "b2".to_string(),
                "b1".to_string(),
                "missing-branch".to_string(),
            ];
            let statuses = fetch_ci_statuses_async(&repo, &client, &stack, &branches)
                .await
                .unwrap();

            assert_eq!(statuses.len(), 2);
            assert_eq!(statuses[0].branch, "b1");
            assert_eq!(statuses[1].branch, "b2");
            assert_eq!(statuses[0].sha, sha_b1);
            assert_eq!(statuses[1].sha, sha_b2);
            assert_eq!(statuses[0].check_runs[0].name, "ci-b1");
            assert_eq!(statuses[1].check_runs[0].name, "ci-b2");
            assert_eq!(statuses[0].pr_number, Some(201));
            assert_eq!(statuses[1].pr_number, Some(202));
            assert_eq!(statuses[0].pr_is_draft, Some(false));
            assert_eq!(statuses[1].pr_is_draft, Some(true));
            assert_eq!(statuses[0].overall_status.as_deref(), Some("success"));
            assert_eq!(statuses[1].overall_status.as_deref(), Some("success"));
        });
    }

    #[test]
    fn fetch_ci_statuses_branch_without_pr_skips_pull_request_fetch() {
        ensure_crypto_provider();
        let (_td, repo, sha_b1, _sha_b2) = git_repo_with_two_branches();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mock_server = MockServer::start().await;
            let path_b1 = format!("/repos/test-owner/test-repo/commits/{sha_b1}/check-runs");
            Mock::given(method("GET"))
                .and(path(path_b1.as_str()))
                .respond_with(ResponseTemplate::new(200).set_body_json(check_runs_body("solo")))
                .mount(&mock_server)
                .await;

            let octocrab = Octocrab::builder()
                .base_uri(mock_server.uri())
                .unwrap()
                .personal_token("test-token".to_string())
                .build()
                .unwrap();
            let gh = GitHubClient::with_octocrab(octocrab, "test-owner", "test-repo");
            let client = ForgeClient::GitHub(gh);

            let mut branches = HashMap::new();
            branches.insert(
                "b1".to_string(),
                StackBranch {
                    name: "b1".to_string(),
                    parent: Some("main".to_string()),
                    parent_revision: None,
                    children: Vec::new(),
                    needs_restack: false,
                    pr_number: None,
                    pr_state: None,
                    pr_is_draft: None,
                },
            );
            let stack = Stack {
                branches,
                trunk: "main".to_string(),
            };

            let statuses = fetch_ci_statuses_async(&repo, &client, &stack, &["b1".to_string()])
                .await
                .unwrap();
            assert_eq!(statuses.len(), 1);
            assert_eq!(statuses[0].branch, "b1");
            assert_eq!(statuses[0].check_runs[0].name, "solo");
            assert_eq!(statuses[0].pr_number, None);
            assert_eq!(statuses[0].pr_is_draft, None);
        });
    }
}
