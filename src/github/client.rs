use anyhow::{Context, Result};
use octocrab::Octocrab;

use crate::config::Config;

pub struct GitHubClient {
    pub octocrab: Octocrab,
    pub owner: String,
    pub repo: String,
}

impl GitHubClient {
    /// Create a new GitHub client from config
    pub fn new(owner: &str, repo: &str) -> Result<Self> {
        let config = Config::load()?;
        let token = config
            .github_token()
            .context("GitHub token not set. Run `gt auth` first.")?;

        let octocrab = Octocrab::builder()
            .personal_token(token.to_string())
            .build()
            .context("Failed to create GitHub client")?;

        Ok(Self {
            octocrab,
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }

    /// Parse owner/repo from git remote URL
    pub fn from_remote(remote_url: &str) -> Result<(String, String)> {
        // Handle SSH format: git@github.com:owner/repo.git
        if remote_url.starts_with("git@github.com:") {
            let path = remote_url
                .strip_prefix("git@github.com:")
                .unwrap()
                .trim_end_matches(".git");
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 2 {
                return Ok((parts[0].to_string(), parts[1].to_string()));
            }
        }

        // Handle HTTPS format: https://github.com/owner/repo.git
        if remote_url.contains("github.com/") {
            let path = remote_url
                .split("github.com/")
                .nth(1)
                .context("Invalid GitHub URL")?
                .trim_end_matches(".git");
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 2 {
                return Ok((parts[0].to_string(), parts[1].to_string()));
            }
        }

        anyhow::bail!("Could not parse GitHub remote URL: {}", remote_url)
    }
}
