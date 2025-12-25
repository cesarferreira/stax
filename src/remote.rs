use crate::config::Config;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    GitHub,
    GitLab,
    Gitea,
}

impl Provider {
    pub fn from_str(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "gitlab" => Provider::GitLab,
            "gitea" => Provider::Gitea,
            _ => Provider::GitHub,
        }
    }

    pub fn pr_label(&self) -> &'static str {
        match self {
            Provider::GitLab => "MR",
            _ => "PR",
        }
    }

    pub fn pr_path(&self, number: u64) -> String {
        match self {
            Provider::GitLab => format!("-/merge_requests/{}", number),
            Provider::Gitea => format!("pulls/{}", number),
            Provider::GitHub => format!("pull/{}", number),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub namespace: String,
    pub repo: String,
    pub provider: Provider,
    pub base_url: String,
    pub api_base_url: Option<String>,
}

impl RemoteInfo {
    pub fn from_repo(repo: &GitRepo, config: &Config) -> Result<Self> {
        let name = config.remote_name().to_string();
        let url = get_remote_url(repo.workdir()?, &name)?;
        let (host, path) = parse_remote_url(&url)?;
        let (namespace, repo_name) = split_namespace_repo(&path)?;

        let provider = Provider::from_str(config.remote_provider());
        let configured_base = config.remote_base_url().trim_end_matches('/');
        let base_url = if configured_base.is_empty()
            || (configured_base == "https://github.com" && host != "github.com")
        {
            format!("https://{}", host)
        } else {
            configured_base.to_string()
        };

        let api_base_url = if let Some(api) = &config.remote.api_base_url {
            Some(api.clone())
        } else if provider == Provider::GitHub {
            if base_url == "https://github.com" {
                Some("https://api.github.com".to_string())
            } else {
                Some(format!("{}/api/v3", base_url))
            }
        } else {
            None
        };

        Ok(Self {
            name,
            namespace,
            repo: repo_name,
            provider,
            base_url,
            api_base_url,
        })
    }

    pub fn owner(&self) -> &str {
        self.namespace.as_str()
    }

    pub fn repo_url(&self) -> String {
        format!("{}/{}/{}", self.base_url, self.namespace, self.repo)
    }

    pub fn pr_url(&self, number: u64) -> String {
        format!("{}/{}", self.repo_url(), self.provider.pr_path(number))
    }
}

pub fn get_remote_url(workdir: &Path, remote: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", remote])
        .current_dir(workdir)
        .output()
        .context("Failed to get remote URL")?;

    if !output.status.success() {
        anyhow::bail!(
            "No git remote '{}' found.\n\n\
             To fix this, add a remote:\n\n  \
             git remote add {} <url>",
            remote,
            remote
        );
    }

    let url = String::from_utf8(output.stdout)?.trim().to_string();

    if url.is_empty() {
        anyhow::bail!(
            "Git remote '{}' has no URL configured.\n\n\
             To fix this, set the remote URL:\n\n  \
             git remote set-url {} <url>",
            remote,
            remote
        );
    }

    Ok(url)
}

pub fn get_remote_branches(workdir: &Path, remote: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["branch", "-r", "--format=%(refname:short)"])
        .current_dir(workdir)
        .output()
        .context("Failed to list remote branches")?;

    let prefix = format!("{}/", remote);
    let branches: Vec<String> = String::from_utf8(output.stdout)?
        .lines()
        .map(|s| s.trim().strip_prefix(&prefix).unwrap_or(s).to_string())
        .collect();

    Ok(branches)
}

pub fn fetch_remote(workdir: &Path, remote: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["fetch", remote])
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to fetch from remote")?;

    if !status.success() {
        anyhow::bail!("Failed to fetch from {}", remote);
    }
    Ok(())
}

fn parse_remote_url(url: &str) -> Result<(String, String)> {
    if let Some(stripped) = url.strip_prefix("git@") {
        let mut parts = stripped.splitn(2, ':');
        let host = parts.next().unwrap_or("").to_string();
        let path = parts
            .next()
            .context("Invalid SSH remote URL")?
            .trim_end_matches(".git")
            .to_string();
        return Ok((host, path));
    }

    if let Some(stripped) = url.strip_prefix("ssh://") {
        let without_scheme = stripped;
        let mut host_and_path = without_scheme.splitn(2, '/');
        let host_part = host_and_path.next().unwrap_or("");
        let path = host_and_path
            .next()
            .context("Invalid SSH remote URL")?
            .trim_end_matches(".git")
            .to_string();

        let host = host_part
            .split('@')
            .nth(1)
            .unwrap_or(host_part)
            .to_string();
        return Ok((host, path));
    }

    if let Some(stripped) = url.strip_prefix("https://") {
        return parse_http_remote(stripped);
    }

    if let Some(stripped) = url.strip_prefix("http://") {
        return parse_http_remote(stripped);
    }

    anyhow::bail!("Unsupported remote URL format: {}", url)
}

fn parse_http_remote(stripped: &str) -> Result<(String, String)> {
    let mut parts = stripped.splitn(2, '/');
    let host = parts.next().unwrap_or("").to_string();
    let path = parts
        .next()
        .context("Invalid HTTP remote URL")?
        .trim_end_matches(".git")
        .to_string();
    Ok((host, path))
}

fn split_namespace_repo(path: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();

    if parts.len() < 2 {
        anyhow::bail!("Remote URL path '{}' is missing owner/repo", path);
    }

    let repo = parts.last().unwrap().to_string();
    let namespace = parts[..parts.len() - 1].join("/");

    Ok((namespace, repo))
}
