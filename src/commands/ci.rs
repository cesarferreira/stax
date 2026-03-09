use crate::cache::CiCache;
use crate::ci::{history, CheckRunInfo};
use crate::config::Config;
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::github::GitHubClient;
use crate::remote::RemoteInfo;
use anyhow::Result;
use chrono::{DateTime, Utc};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
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
fn calculate_branch_timing(
    repo: &GitRepo,
    branch_name: &str,
    checks: &[CheckRunInfo],
) -> Option<BranchTiming> {
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

    // Try branch-level history first (most accurate: wall-clock from first to last check)
    let history_key = format!("branch-overall:{}", branch_name);
    let average_secs = match history::load_check_history(repo, &history_key) {
        Ok(hist) => history::calculate_average(&hist),
        Err(_) => None,
    }
    // Fall back to max(individual check averages) -- the slowest check is the critical path
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
                    "{}  {}  {}%  ⏱ {}  elapsed  {}",
                    "running".yellow().bold(),
                    bar,
                    pct,
                    elapsed_str,
                    eta
                )
            }
            _ => format!("{}  ⏱ {} elapsed", "running".yellow().bold(), elapsed_str),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    all: bool,
    stack: bool,
    json: bool,
    _refresh: bool,
    watch: bool,
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

    // Check for GitHub token
    if Config::github_token().is_none() {
        anyhow::bail!(
            "GitHub auth not configured. Use one of: `stax auth`, `stax auth --from-gh`, `gh auth login`, or set `STAX_GITHUB_TOKEN`."
        );
    }

    let Some(remote) = remote_info else {
        anyhow::bail!("Could not determine GitHub remote info.");
    };

    let rt = tokio::runtime::Runtime::new()?;

    let client = ForgeClient::new(&remote)?;

    if watch {
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

/// Fetch CI statuses for all branches
pub fn fetch_ci_statuses(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    stack: &Stack,
    branches_to_check: &[String],
) -> Result<Vec<BranchCiStatus>> {
    let mut statuses: Vec<BranchCiStatus> = Vec::new();

    for branch in branches_to_check {
        let sha = match repo.branch_commit(branch) {
            Ok(sha) => sha,
            Err(_) => continue,
        };

        let sha_short = sha.chars().take(7).collect::<String>();
        let pr_number = stack.branches.get(branch).and_then(|b| b.pr_number);

        let check_runs_result = rt.block_on(async { client.fetch_checks(repo, &sha).await });

        let (overall_status, check_runs) = match check_runs_result {
            Ok((status, runs)) => (status, runs),
            Err(_) => (None, Vec::new()),
        };

        statuses.push(BranchCiStatus {
            branch: branch.clone(),
            sha,
            sha_short,
            overall_status,
            check_runs,
            pr_number,
        });
    }

    // Sort by branch name for consistent output
    statuses.sort_by(|a, b| a.branch.cmp(&b.branch));

    Ok(statuses)
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
    if let Some(timing) = calculate_branch_timing(repo, &status.branch, &status.check_runs) {
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
    let timing_cols: Vec<String> = sorted
        .iter()
        .map(|check| match check.status.as_str() {
            "completed" => {
                if let Some(elapsed) = check.elapsed_secs {
                    match check.average_secs {
                        Some(avg) => format!(
                            "{}  (avg: {})",
                            format_duration(elapsed),
                            format_duration(avg)
                        ),
                        None => format_duration(elapsed),
                    }
                } else {
                    String::new()
                }
            }
            "in_progress" | "pending" | "queued" | "waiting" | "requested" => {
                if let (Some(elapsed), Some(avg)) = (check.elapsed_secs, check.average_secs) {
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
                } else if let Some(elapsed) = check.elapsed_secs {
                    format!("{} elapsed", format_duration(elapsed))
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        })
        .collect();

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
    if let Some(timing) = calculate_branch_timing(repo, &status.branch, &status.check_runs) {
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
        for check in &status.check_runs {
            if check.status == "completed" && check.conclusion.as_deref() == Some("success") {
                if let (Some(elapsed), Some(completed_at)) =
                    (check.elapsed_secs, check.completed_at.as_ref())
                {
                    let _ =
                        history::add_completion(repo, &check.name, elapsed, completed_at.clone());
                }
            }
        }

        let all_completed = !status.check_runs.is_empty()
            && status.check_runs.iter().all(|c| c.status == "completed");
        let all_success = status.check_runs.iter().all(|c| {
            c.conclusion.as_deref() == Some("success")
                || c.conclusion.as_deref() == Some("skipped")
                || c.conclusion.as_deref() == Some("neutral")
        });

        if all_completed && all_success {
            if let (Some(earliest), Some(latest)) = (
                status
                    .check_runs
                    .iter()
                    .filter_map(|c| c.started_at.as_ref())
                    .filter_map(|s| s.parse::<DateTime<Utc>>().ok())
                    .min(),
                status
                    .check_runs
                    .iter()
                    .filter_map(|c| c.completed_at.as_ref())
                    .filter_map(|s| s.parse::<DateTime<Utc>>().ok())
                    .max(),
            ) {
                let duration = latest.signed_duration_since(earliest);
                let elapsed_secs = duration.num_seconds().max(0) as u64;
                let completed_at = latest.to_rfc3339();
                let history_key = format!("branch-overall:{}", status.branch);
                let _ = history::add_completion(repo, &history_key, elapsed_secs, completed_at);
            }
        }
    }
}

/// Check if all CI checks are complete (not pending)
fn all_checks_complete(statuses: &[BranchCiStatus]) -> bool {
    statuses.iter().all(|s| {
        // Branches with no CI configured are considered "done" (nothing to wait for)
        s.check_runs.is_empty() || s.overall_status.as_deref() != Some("pending")
    })
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

        let complete = all_checks_complete(&statuses);

        if complete {
            let has_failure = statuses
                .iter()
                .any(|s| s.overall_status.as_deref() == Some("failure"));
            println!();
            let width = 50;
            let line = "═".repeat(width);
            if has_failure {
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

fn update_ci_cache(repo: &GitRepo, stack: &Stack, statuses: &[BranchCiStatus]) {
    let git_dir = match repo.git_dir() {
        Ok(path) => path,
        Err(_) => return,
    };

    let mut cache = CiCache::load(git_dir);
    for status in statuses {
        cache.update(&status.branch, status.overall_status.clone(), None);
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

    let mut check_runs: Vec<CheckRunInfo> = Vec::new();

    for status in statuses {
        let (status_str, conclusion, elapsed_secs) = match status.state.as_str() {
            "success" => {
                let elapsed = if let (Some(created), Some(updated)) =
                    (&status.created_at, &status.updated_at)
                {
                    if let (Ok(created_time), Ok(updated_time)) = (
                        created.parse::<DateTime<Utc>>(),
                        updated.parse::<DateTime<Utc>>(),
                    ) {
                        let duration = updated_time.signed_duration_since(created_time);
                        Some(duration.num_seconds().max(0) as u64)
                    } else {
                        None
                    }
                } else {
                    None
                };
                (
                    "completed".to_string(),
                    Some("success".to_string()),
                    elapsed,
                )
            }
            "failure" | "error" => ("completed".to_string(), Some("failure".to_string()), None),
            "pending" => ("in_progress".to_string(), None, None),
            _ => ("queued".to_string(), None, None),
        };

        let average_secs = match history::load_check_history(repo, &status.context) {
            Ok(hist) => history::calculate_average(&hist),
            Err(_) => None,
        };

        let completion_percent = if status_str == "in_progress" {
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
            name: status.context,
            status: status_str,
            conclusion,
            url: status.target_url,
            started_at: status.created_at,
            completed_at: status.updated_at.clone(),
            elapsed_secs,
            average_secs,
            completion_percent,
        });
    }

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
}
