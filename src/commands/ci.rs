use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::github::GitHubClient;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Individual check run info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckRunInfo {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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
}

pub fn run(all: bool, json: bool, refresh: bool) -> Result<()> {
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
        anyhow::bail!("GitHub token not set. Run `stax auth` or set GITHUB_TOKEN env var.");
    }

    let Some(remote) = remote_info else {
        anyhow::bail!("Could not determine GitHub remote info.");
    };

    // Create tokio runtime for async GitHub API calls
    let rt = tokio::runtime::Runtime::new()?;

    let client = rt.block_on(async {
        GitHubClient::new(remote.owner(), &remote.repo, remote.api_base_url.clone())
    })?;

    // Collect CI status for all branches
    let mut statuses: Vec<BranchCiStatus> = Vec::new();

    for branch in &branches_to_check {
        let sha = match repo.branch_commit(branch) {
            Ok(sha) => sha,
            Err(_) => continue,
        };

        let sha_short = sha.chars().take(7).collect::<String>();
        let pr_number = stack.branches.get(branch).and_then(|b| b.pr_number);

        // Fetch detailed check runs
        let check_runs_result = rt.block_on(async {
            fetch_check_runs(&client, &sha).await
        });

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

    if json {
        println!("{}", serde_json::to_string_pretty(&statuses)?);
        return Ok(());
    }

    // Display in a nice format
    for status in &statuses {
        let is_current = status.branch == current;

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

                println!("    {} {} {}", icon, check.name, status_str);
            }
        }

        println!(); // Blank line between branches
    }

    // Summary
    let success_count = statuses.iter().filter(|s| s.overall_status.as_deref() == Some("success")).count();
    let failure_count = statuses.iter().filter(|s| s.overall_status.as_deref() == Some("failure")).count();
    let pending_count = statuses.iter().filter(|s| s.overall_status.as_deref() == Some("pending")).count();
    let no_ci_count = statuses.iter().filter(|s| s.overall_status.is_none()).count();

    let mut summary_parts: Vec<String> = Vec::new();
    if success_count > 0 {
        summary_parts.push(format!("{} passed", success_count).green().to_string());
    }
    if failure_count > 0 {
        summary_parts.push(format!("{} failed", failure_count).red().to_string());
    }
    if pending_count > 0 {
        summary_parts.push(format!("{} pending", pending_count).yellow().to_string());
    }
    if no_ci_count > 0 {
        summary_parts.push(format!("{} no CI", no_ci_count).dimmed().to_string());
    }

    if !summary_parts.is_empty() {
        println!("{}", summary_parts.join(" · "));
    }

    Ok(())
}

async fn fetch_check_runs(
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

    let mut check_runs: Vec<CheckRunInfo> = response
        .check_runs
        .into_iter()
        .map(|r| CheckRunInfo {
            name: r.name,
            status: r.status,
            conclusion: r.conclusion,
            url: r.html_url,
        })
        .collect();

    // Sort by name for consistent ordering
    check_runs.sort_by(|a, b| a.name.cmp(&b.name));

    // Calculate overall status
    let mut has_pending = false;
    let mut has_failure = false;
    let mut all_success = true;

    for run in &check_runs {
        match run.status.as_str() {
            "completed" => match run.conclusion.as_deref() {
                Some("success") | Some("skipped") | Some("neutral") => {}
                Some("failure") | Some("timed_out") | Some("cancelled") | Some("action_required") => {
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
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("build"));
        assert!(json.contains("completed"));
        assert!(json.contains("success"));

        let deserialized: CheckRunInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "build");
        assert_eq!(deserialized.status, "completed");
        assert_eq!(deserialized.conclusion, Some("success".to_string()));
    }

    #[test]
    fn test_check_run_info_without_url() {
        let info = CheckRunInfo {
            name: "test".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            url: None,
        };

        let json = serde_json::to_string(&info).unwrap();
        // url should be skipped when None due to skip_serializing_if
        assert!(!json.contains("url"));
        assert!(json.contains("test"));
        assert!(json.contains("in_progress"));
    }

    #[test]
    fn test_branch_ci_status_serialization() {
        let status = BranchCiStatus {
            branch: "feature-branch".to_string(),
            sha: "abc123def456".to_string(),
            sha_short: "abc123d".to_string(),
            overall_status: Some("success".to_string()),
            check_runs: vec![
                CheckRunInfo {
                    name: "build".to_string(),
                    status: "completed".to_string(),
                    conclusion: Some("success".to_string()),
                    url: None,
                },
            ],
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
        assert_eq!(response.check_runs[0].conclusion, Some("success".to_string()));
        assert_eq!(response.check_runs[1].name, "test");
        assert_eq!(response.check_runs[1].conclusion, None);
    }

    #[test]
    fn test_check_run_detail_deserialization() {
        let json = r#"{"name": "lint", "status": "queued", "conclusion": null, "html_url": "https://example.com"}"#;

        let detail: CheckRunDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.name, "lint");
        assert_eq!(detail.status, "queued");
        assert_eq!(detail.conclusion, None);
        assert_eq!(detail.html_url, Some("https://example.com".to_string()));
    }
}
