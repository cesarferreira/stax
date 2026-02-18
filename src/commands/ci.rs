use crate::ci::history;
use crate::config::Config;
use crate::engine::Stack;
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

/// Individual check run info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckRunInfo {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    // Timing fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_percent: Option<u8>,
}

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
    context: String, // This is like the "name" for statuses
    state: String,   // success, pending, failure, error
    target_url: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

/// Calculate overall timing for the entire branch CI run
fn calculate_branch_timing(
    repo: &GitRepo,
    branch_name: &str,
    checks: &[CheckRunInfo],
) -> Option<String> {
    if checks.is_empty() {
        return None;
    }

    // Find the earliest started_at time (when CI run began)
    let earliest_start = checks
        .iter()
        .filter_map(|c| c.started_at.as_ref())
        .filter_map(|s| s.parse::<DateTime<Utc>>().ok())
        .min()?;

    // Calculate elapsed time
    let now = Utc::now();
    let is_complete = checks.iter().all(|c| c.status == "completed");

    let elapsed_secs = if is_complete {
        // Find the latest completed_at time
        let latest_complete = checks
            .iter()
            .filter_map(|c| c.completed_at.as_ref())
            .filter_map(|s| s.parse::<DateTime<Utc>>().ok())
            .max()?;

        let duration = latest_complete.signed_duration_since(earliest_start);
        duration.num_seconds().max(0) as u64
    } else {
        // Still running, calculate from start to now
        let duration = now.signed_duration_since(earliest_start);
        duration.num_seconds().max(0) as u64
    };

    let elapsed_str = format_duration(elapsed_secs);

    // Load historical average for this branch
    let history_key = format!("branch-overall:{}", branch_name);
    let average_secs = match history::load_check_history(repo, &history_key) {
        Ok(hist) => history::calculate_average(&hist),
        Err(_) => None,
    };

    // Format according to user requirements
    if let Some(avg) = average_secs {
        if is_complete {
            // Completed with history: elapsed | avg
            Some(format!(
                "Build time: {} | avg: {}",
                elapsed_str,
                format_duration(avg)
            ))
        } else {
            // In progress with history: elapsed | avg, ETA, percentage
            let (eta_str, pct) = if elapsed_secs >= avg {
                ("overdue".to_string(), 99)
            } else {
                let remaining = avg - elapsed_secs;
                let pct = ((elapsed_secs * 100) / avg).min(99) as u8;
                (format_duration(remaining), pct)
            };
            Some(format!(
                "Build time: {} | avg: {}, ETA: {} ({}%)",
                elapsed_str,
                format_duration(avg),
                eta_str,
                pct
            ))
        }
    } else {
        // No history, just show elapsed
        Some(format!("Build time: {}", elapsed_str))
    }
}

pub fn run(all: bool, json: bool, _refresh: bool, watch: bool, interval: u64) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;

    let remote_info = RemoteInfo::from_repo(&repo, &config).ok();

    // Get branches to check
    let branches_to_check: Vec<String> = if all {
        stack
            .branches
            .keys()
            .filter(|b| *b != &stack.trunk)
            .cloned()
            .collect()
    } else {
        // Get current stack (excluding trunk)
        stack
            .current_stack(&current)
            .into_iter()
            .filter(|b| b != &stack.trunk)
            .collect()
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

    // Create tokio runtime for async GitHub API calls
    let rt = tokio::runtime::Runtime::new()?;

    let client = rt.block_on(async {
        GitHubClient::new(remote.owner(), &remote.repo, remote.api_base_url.clone())
    })?;

    // Watch mode: loop until all CI checks complete
    if watch {
        return run_watch_mode(
            &repo,
            &rt,
            &client,
            &stack,
            &branches_to_check,
            &current,
            interval,
            json,
        );
    }

    // Single run mode (original behavior)
    let statuses = fetch_ci_statuses(&repo, &rt, &client, &stack, &branches_to_check)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&statuses)?);
        return Ok(());
    }

    display_ci_statuses(&repo, &statuses, &current);
    record_ci_history(&repo, &statuses);

    Ok(())
}

/// Fetch CI statuses for all branches
pub fn fetch_ci_statuses(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &GitHubClient,
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

        // Fetch both check runs and commit statuses
        let check_runs_result = rt.block_on(async { fetch_all_checks(repo, client, &sha).await });

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

/// Display CI statuses in a nice format
fn display_ci_statuses(repo: &GitRepo, statuses: &[BranchCiStatus], current: &str) {
    for status in statuses {
        let is_current = status.branch == current;

        // Calculate overall branch timing
        let branch_timing = calculate_branch_timing(repo, &status.branch, &status.check_runs);

        // Branch header
        let branch_display = if is_current {
            format!("◉ {}", status.branch).bold()
        } else {
            format!("○ {}", status.branch).normal()
        };

        let overall_icon = match status.overall_status.as_deref() {
            Some("success") => "✓".green().bold(),
            Some("failure") => "✗".red().bold(),
            Some("pending") => "●".yellow().bold(),
            None => "○".dimmed(),
            _ => "?".dimmed(),
        };

        let pr_info = status
            .pr_number
            .map(|n| format!(" PR #{}", n).bright_magenta().to_string())
            .unwrap_or_default();

        println!(
            "{} {} {}{}",
            overall_icon,
            branch_display,
            format!("({})", status.sha_short).dimmed(),
            pr_info
        );

        // Show individual check runs
        if status.check_runs.is_empty() {
            println!("    {}", "No CI checks configured".dimmed());
        } else {
            for check in &status.check_runs {
                let (icon, status_str) = match check.status.as_str() {
                    "completed" => match check.conclusion.as_deref() {
                        Some("success") => ("✓".green(), "passed".green()),
                        Some("failure") => ("✗".red(), "failed".red()),
                        Some("skipped") => ("⊘".dimmed(), "skipped".dimmed()),
                        Some("neutral") => ("○".dimmed(), "neutral".dimmed()),
                        Some("cancelled") => ("⊘".yellow(), "cancelled".yellow()),
                        Some("timed_out") => ("⏱".red(), "timed out".red()),
                        Some("action_required") => ("!".yellow(), "action required".yellow()),
                        Some(other) => ("?".dimmed(), other.dimmed()),
                        None => ("?".dimmed(), "unknown".dimmed()),
                    },
                    "queued" => ("◎".cyan(), "queued".cyan()),
                    "in_progress" => ("●".yellow(), "running".yellow()),
                    "waiting" => ("◎".cyan(), "waiting".cyan()),
                    "requested" => ("◎".cyan(), "requested".cyan()),
                    "pending" => ("●".yellow(), "pending".yellow()),
                    _ => ("?".dimmed(), check.status.as_str().dimmed()),
                };

                // Build timing information if available
                let timing_info = if let Some(elapsed) = check.elapsed_secs {
                    let elapsed_str = format!("[{}]", format_duration(elapsed)).cyan();

                    let timing = if let Some(avg) = check.average_secs {
                        if let Some(pct) = check.completion_percent {
                            // In progress with prediction
                            format!(
                                " {} (avg: {}, {}%)",
                                elapsed_str,
                                format_duration(avg).dimmed(),
                                pct
                            )
                        } else {
                            // Completed, show comparison to average
                            format!(" {} (avg: {})", elapsed_str, format_duration(avg).dimmed())
                        }
                    } else {
                        // No history, just elapsed
                        format!(" {}", elapsed_str)
                    };
                    timing
                } else {
                    String::new()
                };

                println!("    {} {} {}{}", icon, check.name, status_str, timing_info);
            }
        }

        // Display overall branch timing at the bottom
        if let Some(timing_str) = branch_timing {
            println!("    {}", timing_str.dimmed());
        }

        println!(); // Blank line between branches
    }

    // Summary
    let success_count = statuses
        .iter()
        .filter(|s| s.overall_status.as_deref() == Some("success"))
        .count();
    let failure_count = statuses
        .iter()
        .filter(|s| s.overall_status.as_deref() == Some("failure"))
        .count();
    let pending_count = statuses
        .iter()
        .filter(|s| s.overall_status.as_deref() == Some("pending"))
        .count();
    let _no_ci_count = statuses
        .iter()
        .filter(|s| s.overall_status.is_none())
        .count();

    if !statuses.is_empty() {
        let total = statuses.len();
        let branch_word = if total == 1 { "branch" } else { "branches" };
        println!("{}", "─".repeat(40).dimmed());
        println!("{}", format!("Summary: {} {}", total, branch_word).bold());
        println!(
            "  {} passing: {}, {} failing: {}, {} pending: {}",
            "✓".green(),
            success_count,
            "✗".red(),
            failure_count,
            "●".yellow(),
            pending_count
        );
    }
}

/// Record CI history for completed successful checks
pub fn record_ci_history(repo: &GitRepo, statuses: &[BranchCiStatus]) {
    for status in statuses {
        for check in &status.check_runs {
            // Only record successful completions with valid timing data
            if check.status == "completed"
                && check.conclusion.as_deref() == Some("success")
                && check.elapsed_secs.is_some()
                && check.completed_at.is_some()
            {
                let elapsed = check.elapsed_secs.unwrap();
                let completed_at = check.completed_at.as_ref().unwrap().clone();

                // Silently ignore errors - don't fail the command if history update fails
                let _ = history::add_completion(repo, &check.name, elapsed, completed_at);
            }
        }

        // Also save branch-level overall timing if all checks are completed successfully
        let all_completed = !status.check_runs.is_empty()
            && status.check_runs.iter().all(|c| c.status == "completed");
        let all_success = status.check_runs.iter().all(|c| {
            c.conclusion.as_deref() == Some("success")
                || c.conclusion.as_deref() == Some("skipped")
                || c.conclusion.as_deref() == Some("neutral")
        });

        if all_completed && all_success {
            // Calculate branch-level elapsed time
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

                // Silently ignore errors
                let _ = history::add_completion(repo, &history_key, elapsed_secs, completed_at);
            }
        }
    }
}

/// Check if all CI checks are complete (not pending)
fn all_checks_complete(statuses: &[BranchCiStatus]) -> bool {
    statuses
        .iter()
        .all(|s| s.overall_status.as_deref() != Some("pending") && !s.check_runs.is_empty())
}

/// Run watch mode - poll CI status until all checks complete
#[allow(clippy::too_many_arguments)]
fn run_watch_mode(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &GitHubClient,
    stack: &Stack,
    branches_to_check: &[String],
    current: &str,
    interval: u64,
    json: bool,
) -> Result<()> {
    let poll_duration = Duration::from_secs(interval);
    let mut iteration = 0;

    println!("{}", "Watching CI status (Ctrl+C to stop)...".cyan().bold());
    println!();

    loop {
        iteration += 1;

        // Fetch current CI statuses
        let statuses = fetch_ci_statuses(repo, rt, client, stack, branches_to_check)?;

        // Clear screen for clean output (except first iteration)
        if iteration > 1 {
            // Move cursor up and clear previous output
            // Using ANSI escape codes for portability
            print!("\x1B[2J\x1B[H"); // Clear screen and move to home
            let _ = std::io::stdout().flush();
            println!("{}", "Watching CI status (Ctrl+C to stop)...".cyan().bold());
            println!();
        }

        if json {
            println!("{}", serde_json::to_string_pretty(&statuses)?);
        } else {
            display_ci_statuses(repo, &statuses, current);
        }

        // Check if all complete
        let complete = all_checks_complete(&statuses);

        if complete {
            println!();
            println!("{}", "All CI checks complete!".green().bold());

            // Record history now that everything is done
            record_ci_history(repo, &statuses);

            return Ok(());
        }

        // Show next refresh time
        if !json {
            println!();
            println!(
                "{}",
                format!("Refreshing in {}s... (iteration #{})", interval, iteration).dimmed()
            );
        }

        // Wait before next poll
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

/// Fetch all checks (both check runs and commit statuses)
async fn fetch_all_checks(
    repo: &GitRepo,
    client: &GitHubClient,
    commit_sha: &str,
) -> Result<(Option<String>, Vec<CheckRunInfo>)> {
    // Fetch check runs (newer GitHub Actions-style checks)
    let (check_runs_overall, mut all_checks) = fetch_check_runs(repo, client, commit_sha).await?;

    // Fetch commit statuses (older external CI systems)
    let (statuses_overall, status_checks) = fetch_commit_statuses(repo, client, commit_sha).await?;

    // Combine both types of checks
    all_checks.extend(status_checks);

    // Combine overall statuses (failure > pending > success)
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
        // Convert commit status to CheckRunInfo format
        // Commit statuses don't have detailed timing, so we estimate based on created/updated
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

        // Load history and calculate average
        let average_secs = match history::load_check_history(repo, &status.context) {
            Ok(hist) => history::calculate_average(&hist),
            Err(_) => None,
        };

        // Calculate completion percentage (only for in_progress checks)
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

    // Calculate overall status
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
        // Calculate elapsed time
        let (elapsed_secs, completed_at_str) = if let Some(completed) = &r.completed_at {
            // Check is completed
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
            // Check is running
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

        // Load history and calculate average
        let average_secs = match history::load_check_history(repo, &r.name) {
            Ok(hist) => history::calculate_average(&hist),
            Err(_) => None,
        };

        // Calculate completion percentage (only for in_progress checks)
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

    // Deduplicate check runs by name, keeping only the most recent for each
    let mut unique_checks: HashMap<String, CheckRunInfo> = HashMap::new();
    for check in check_runs {
        let should_replace = if let Some(existing) = unique_checks.get(&check.name) {
            // Keep the one with the most recent started_at timestamp
            match (&check.started_at, &existing.started_at) {
                (Some(new_start), Some(existing_start)) => {
                    // Parse and compare timestamps
                    if let (Ok(new_time), Ok(existing_time)) = (
                        new_start.parse::<DateTime<Utc>>(),
                        existing_start.parse::<DateTime<Utc>>(),
                    ) {
                        new_time > existing_time
                    } else {
                        false // Keep existing if we can't parse
                    }
                }
                (Some(_), None) => true, // New has timestamp, existing doesn't
                (None, Some(_)) => false, // Existing has timestamp, new doesn't
                (None, None) => true,    // Neither has timestamp, keep new one
            }
        } else {
            true // No existing check with this name
        };

        if should_replace {
            unique_checks.insert(check.name.clone(), check);
        }
    }

    // Convert back to vector and sort by name for consistent ordering
    let mut check_runs: Vec<CheckRunInfo> = unique_checks.into_values().collect();
    check_runs.sort_by(|a, b| a.name.cmp(&b.name));

    // Calculate overall status
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

#[cfg(test)]
mod tests {
    use super::*;

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
        // url should be skipped when None due to skip_serializing_if
        assert!(!json.contains("url"));
        assert!(json.contains("test"));
        assert!(json.contains("in_progress"));
        assert!(json.contains("120")); // elapsed_secs
        assert!(json.contains("66")); // completion_percent
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
        assert!(json.contains("null")); // pr_number is null
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
            completion_percent: None, // No percentage for completed checks
        };

        assert_eq!(info.elapsed_secs, Some(120));
        assert_eq!(info.average_secs, Some(110));
        assert_eq!(info.completion_percent, None);

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("120"));
        assert!(json.contains("110"));
        assert!(!json.contains("completion_percent"));
    }
}
