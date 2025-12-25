use anyhow::{Context, Result};
use octocrab::Octocrab;
use octocrab::params::repos::Reference;

use crate::config::Config;

pub struct GitHubClient {
    pub octocrab: Octocrab,
    pub owner: String,
    pub repo: String,
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

    pub async fn combined_status_state(&self, commit_sha: &str) -> Result<Option<String>> {
        let status = self
            .octocrab
            .repos(&self.owner, &self.repo)
            .combined_status_for_ref(&Reference::Branch(commit_sha.to_string()))
            .await?;

        Ok(Some(format!("{:?}", status.state).to_lowercase()))
    }
}
