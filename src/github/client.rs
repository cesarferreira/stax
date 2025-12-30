use anyhow::{Context, Result};
use octocrab::Octocrab;
use octocrab::params::repos::Reference;
use serde::Deserialize;

use crate::config::Config;

pub struct GitHubClient {
    pub octocrab: Octocrab,
    pub owner: String,
    pub repo: String,
}

/// Response from the check-runs API
#[derive(Debug, Deserialize)]
struct CheckRunsResponse {
    total_count: usize,
    check_runs: Vec<CheckRun>,
}

#[derive(Debug, Deserialize)]
struct CheckRun {
    status: String,
    conclusion: Option<String>,
}

impl GitHubClient {
    /// Create a new GitHub client from config
    pub fn new(owner: &str, repo: &str, api_base_url: Option<String>) -> Result<Self> {
        let token = Config::github_token()
            .context("GitHub token not set. Run `stax auth` or set GITHUB_TOKEN env var.")?;

        let mut builder = Octocrab::builder().personal_token(token.to_string());
        if let Some(api_base) = api_base_url {
            builder = builder
                .base_uri(api_base)
                .context("Failed to set GitHub API base URL")?;
        }

        let octocrab = builder.build().context("Failed to create GitHub client")?;

        Ok(Self {
            octocrab,
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }

    /// Get combined CI status from both commit statuses AND check runs (GitHub Actions)
    pub async fn combined_status_state(&self, commit_sha: &str) -> Result<Option<String>> {
        // First, check legacy commit statuses
        let commit_status = self
            .octocrab
            .repos(&self.owner, &self.repo)
            .combined_status_for_ref(&Reference::Branch(commit_sha.to_string()))
            .await
            .ok();

        // Then, check GitHub Actions check runs
        let check_runs_status = self.get_check_runs_status(commit_sha).await.ok().flatten();

        // Combine results: prioritize check runs (more common), fall back to commit status
        match (check_runs_status, commit_status) {
            // If we have check runs, use that status
            (Some(cr_status), _) => Ok(Some(cr_status)),
            // Fall back to commit status
            (None, Some(status)) => Ok(Some(format!("{:?}", status.state).to_lowercase())),
            // No CI at all
            (None, None) => Ok(None),
        }
    }

    /// Get status from GitHub Actions check runs
    async fn get_check_runs_status(&self, commit_sha: &str) -> Result<Option<String>> {
        let url = format!(
            "/repos/{}/{}/commits/{}/check-runs",
            self.owner, self.repo, commit_sha
        );

        let response: CheckRunsResponse = self.octocrab.get(&url, None::<&()>).await?;

        if response.total_count == 0 {
            return Ok(None); // No check runs configured
        }

        // Analyze all check runs to determine overall status
        let mut has_pending = false;
        let mut has_failure = false;
        let mut all_success = true;

        for run in &response.check_runs {
            match run.status.as_str() {
                "completed" => {
                    match run.conclusion.as_deref() {
                        Some("success") | Some("skipped") | Some("neutral") => {}
                        Some("failure") | Some("timed_out") | Some("cancelled")
                        | Some("action_required") => {
                            has_failure = true;
                            all_success = false;
                        }
                        _ => {
                            all_success = false;
                        }
                    }
                }
                "queued" | "in_progress" | "waiting" | "requested" | "pending" => {
                    has_pending = true;
                    all_success = false;
                }
                _ => {
                    all_success = false;
                }
            }
        }

        if has_failure {
            Ok(Some("failure".to_string()))
        } else if has_pending {
            Ok(Some("pending".to_string()))
        } else if all_success {
            Ok(Some("success".to_string()))
        } else {
            Ok(Some("pending".to_string())) // Unknown state, treat as pending
        }
    }
}
