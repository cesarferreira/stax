use crate::config::Config;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use git2::{ConfigLevel, Repository};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeType {
    GitHub,
    GitLab,
    Gitea,
}

impl std::fmt::Display for ForgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GitHub => write!(f, "GitHub"),
            Self::GitLab => write!(f, "GitLab"),
            Self::Gitea => write!(f, "Gitea"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub forge: ForgeType,
    pub host: String,
    pub namespace: String,
    pub repo: String,
    pub base_url: String,
    pub api_base_url: Option<String>,
}

impl RemoteInfo {
    pub fn from_repo(repo: &GitRepo, config: &Config) -> Result<Self> {
        let name = config.remote_name().to_string();
        let url = get_remote_url(repo.workdir()?, &name)?;
        let (host, path) = parse_remote_url(&url)?;
        let forge = detect_forge(&host, config.remote_base_url());
        let (namespace, repo_name) = split_namespace_repo(&path)?;

        let configured_base = config.remote_base_url().trim_end_matches('/');
        let base_url = if configured_base.is_empty()
            || (configured_base == "https://github.com" && host != "github.com")
            || (configured_base == "https://gitlab.com" && host != "gitlab.com")
            || (configured_base == "https://gitea.com" && host != "gitea.com")
        {
            format!("https://{}", host)
        } else {
            configured_base.to_string()
        };

        let api_base_url = if let Some(api) = &config.remote.api_base_url {
            Some(api.clone())
        } else {
            Some(default_api_base_url(forge, &base_url))
        };

        Ok(Self {
            name,
            forge,
            host,
            namespace,
            repo: repo_name,
            base_url,
            api_base_url,
        })
    }

    /// Returns the GitHub API owner (first path component only).
    /// For repos like `wayve/frontends/robot-android`, namespace is `wayve/frontends`
    /// but the GitHub API owner is just `wayve`.
    pub fn owner(&self) -> &str {
        self.namespace.split('/').next().unwrap_or(&self.namespace)
    }

    pub fn project_path(&self) -> String {
        format!("{}/{}", self.namespace, self.repo)
    }

    pub fn encoded_project_path(&self) -> String {
        self.project_path().replace('/', "%2F")
    }

    pub fn repo_url(&self) -> String {
        format!("{}/{}/{}", self.base_url, self.namespace, self.repo)
    }

    pub fn pr_url(&self, number: u64) -> String {
        match self.forge {
            ForgeType::GitHub => format!("{}/pull/{}", self.repo_url(), number),
            ForgeType::GitLab => format!("{}/-/merge_requests/{}", self.repo_url(), number),
            ForgeType::Gitea => format!("{}/pulls/{}", self.repo_url(), number),
        }
    }
}

fn detect_forge(host: &str, configured_base_url: &str) -> ForgeType {
    let host = host.to_ascii_lowercase();
    let configured_base_url = configured_base_url.to_ascii_lowercase();

    if host.contains("gitlab") || configured_base_url.contains("gitlab") {
        ForgeType::GitLab
    } else if host.contains("gitea") || configured_base_url.contains("gitea") {
        ForgeType::Gitea
    } else {
        ForgeType::GitHub
    }
}

fn default_api_base_url(forge: ForgeType, base_url: &str) -> String {
    match forge {
        ForgeType::GitHub => {
            if base_url == "https://github.com" {
                "https://api.github.com".to_string()
            } else {
                format!("{}/api/v3", base_url)
            }
        }
        ForgeType::GitLab => format!("{}/api/v4", base_url),
        ForgeType::Gitea => format!("{}/api/v1", base_url),
    }
}

pub fn get_remote_url(workdir: &Path, remote: &str) -> Result<String> {
    if let Ok(repo) = Repository::discover(workdir) {
        if let Ok(config) = repo.config() {
            if let Ok(local) = config.open_level(ConfigLevel::Local) {
                if let Ok(url) = local.get_string(&format!("remote.{}.url", remote)) {
                    if !url.trim().is_empty() {
                        return Ok(url);
                    }
                }
            }
        }
    }

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
        .args(["branch", "-r", "--format=%(refname)"])
        .current_dir(workdir)
        .output()
        .context("Failed to list remote branches")?;

    let prefix = format!("refs/remotes/{}/", remote);
    let branches: Vec<String> = String::from_utf8(output.stdout)?
        .lines()
        .filter_map(|s| s.trim().strip_prefix(&prefix))
        .map(|s| s.to_string())
        .collect();

    Ok(branches)
}

pub fn fetch_remote(workdir: &Path, remote: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["fetch", remote])
        .current_dir(workdir)
        .output()
        .context("Failed to run git fetch")?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // git typically reports meaningful diagnostics on stderr (auth, network, DNS, etc.).
    // Include both streams so users can self-diagnose without re-running manually.
    anyhow::bail!(
        "Failed to fetch from {}.\n\ngit stdout:\n{}\n\ngit stderr:\n{}",
        remote,
        stdout.trim(),
        stderr.trim()
    );
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

        let host = host_part.split('@').nth(1).unwrap_or(host_part).to_string();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn test_parse_ssh_git_url() {
        let (host, path) = parse_remote_url("git@github.com:owner/repo.git").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "owner/repo");
    }

    #[test]
    fn test_parse_ssh_git_url_without_extension() {
        let (host, path) = parse_remote_url("git@github.com:owner/repo").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "owner/repo");
    }

    #[test]
    fn test_parse_https_url() {
        let (host, path) = parse_remote_url("https://github.com/owner/repo.git").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "owner/repo");
    }

    #[test]
    fn test_parse_https_url_without_extension() {
        let (host, path) = parse_remote_url("https://github.com/owner/repo").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "owner/repo");
    }

    #[test]
    fn test_parse_http_url() {
        let (host, path) = parse_remote_url("http://github.com/owner/repo.git").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "owner/repo");
    }

    #[test]
    fn test_parse_ssh_scheme_url() {
        let (host, path) = parse_remote_url("ssh://git@github.com/owner/repo.git").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "owner/repo");
    }

    #[test]
    fn test_parse_github_enterprise_ssh() {
        let (host, path) = parse_remote_url("git@github.example.com:org/project.git").unwrap();
        assert_eq!(host, "github.example.com");
        assert_eq!(path, "org/project");
    }

    #[test]
    fn test_parse_github_enterprise_https() {
        let (host, path) = parse_remote_url("https://github.example.com/org/project.git").unwrap();
        assert_eq!(host, "github.example.com");
        assert_eq!(path, "org/project");
    }

    #[test]
    fn test_get_remote_url_ignores_insteadof_rewrite() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = dir.path();

        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(path)
            .output()
            .expect("Failed to init git repo");

        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/test/repo.git",
            ])
            .current_dir(path)
            .output()
            .expect("Failed to add remote");

        let base = format!("file://{}/", path.display());
        Command::new("git")
            .args([
                "config",
                &format!("url.{}.insteadOf", base),
                "https://github.com/",
            ])
            .current_dir(path)
            .output()
            .expect("Failed to set insteadOf");

        let url = get_remote_url(path, "origin").unwrap();
        assert_eq!(url, "https://github.com/test/repo.git");
    }

    #[test]
    fn test_parse_nested_namespace() {
        let (host, path) =
            parse_remote_url("https://gitlab.com/group/subgroup/project.git").unwrap();
        assert_eq!(host, "gitlab.com");
        assert_eq!(path, "group/subgroup/project");
    }

    #[test]
    fn test_parse_unsupported_url_format() {
        let result = parse_remote_url("ftp://example.com/repo");
        assert!(result.is_err());
    }

    #[test]
    fn test_split_namespace_repo_simple() {
        let (namespace, repo) = split_namespace_repo("owner/repo").unwrap();
        assert_eq!(namespace, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_split_namespace_repo_nested() {
        let (namespace, repo) = split_namespace_repo("org/team/project").unwrap();
        assert_eq!(namespace, "org/team");
        assert_eq!(repo, "project");
    }

    #[test]
    fn test_split_namespace_repo_with_slashes() {
        let (namespace, repo) = split_namespace_repo("/owner/repo/").unwrap();
        assert_eq!(namespace, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_split_namespace_repo_missing_parts() {
        let result = split_namespace_repo("onlyrepo");
        assert!(result.is_err());
    }

    #[test]
    fn test_split_namespace_repo_empty() {
        let result = split_namespace_repo("");
        assert!(result.is_err());
    }

    #[test]
    fn test_remote_info_owner() {
        let info = RemoteInfo {
            name: "origin".to_string(),
            forge: ForgeType::GitHub,
            host: "github.com".to_string(),
            namespace: "myorg".to_string(),
            repo: "myrepo".to_string(),
            base_url: "https://github.com".to_string(),
            api_base_url: Some("https://api.github.com".to_string()),
        };
        assert_eq!(info.owner(), "myorg");
    }

    #[test]
    fn test_remote_info_repo_url() {
        let info = RemoteInfo {
            name: "origin".to_string(),
            forge: ForgeType::GitHub,
            host: "github.com".to_string(),
            namespace: "myorg".to_string(),
            repo: "myrepo".to_string(),
            base_url: "https://github.com".to_string(),
            api_base_url: Some("https://api.github.com".to_string()),
        };
        assert_eq!(info.repo_url(), "https://github.com/myorg/myrepo");
    }

    #[test]
    fn test_remote_info_pr_url() {
        let info = RemoteInfo {
            name: "origin".to_string(),
            forge: ForgeType::GitHub,
            host: "github.com".to_string(),
            namespace: "myorg".to_string(),
            repo: "myrepo".to_string(),
            base_url: "https://github.com".to_string(),
            api_base_url: Some("https://api.github.com".to_string()),
        };
        assert_eq!(info.pr_url(42), "https://github.com/myorg/myrepo/pull/42");
    }

    #[test]
    fn test_remote_info_nested_namespace() {
        let info = RemoteInfo {
            name: "origin".to_string(),
            forge: ForgeType::GitLab,
            host: "gitlab.com".to_string(),
            namespace: "org/team".to_string(),
            repo: "project".to_string(),
            base_url: "https://gitlab.com".to_string(),
            api_base_url: None,
        };
        assert_eq!(info.repo_url(), "https://gitlab.com/org/team/project");
    }

    #[test]
    fn test_remote_info_gitlab_pr_url() {
        let info = RemoteInfo {
            name: "origin".to_string(),
            forge: ForgeType::GitLab,
            host: "gitlab.com".to_string(),
            namespace: "org/team".to_string(),
            repo: "project".to_string(),
            base_url: "https://gitlab.com".to_string(),
            api_base_url: Some("https://gitlab.com/api/v4".to_string()),
        };
        assert_eq!(
            info.pr_url(42),
            "https://gitlab.com/org/team/project/-/merge_requests/42"
        );
    }

    #[test]
    fn test_remote_info_gitea_pr_url() {
        let info = RemoteInfo {
            name: "origin".to_string(),
            forge: ForgeType::Gitea,
            host: "gitea.example.com".to_string(),
            namespace: "org".to_string(),
            repo: "project".to_string(),
            base_url: "https://gitea.example.com".to_string(),
            api_base_url: Some("https://gitea.example.com/api/v1".to_string()),
        };
        assert_eq!(
            info.pr_url(42),
            "https://gitea.example.com/org/project/pulls/42"
        );
    }

    #[test]
    fn test_detect_forge_prefers_host() {
        assert_eq!(
            detect_forge("gitlab.com", "https://github.com"),
            ForgeType::GitLab
        );
        assert_eq!(
            detect_forge("gitea.example.com", "https://github.com"),
            ForgeType::Gitea
        );
        assert_eq!(
            detect_forge("github.example.com", "https://github.com"),
            ForgeType::GitHub
        );
    }

    #[test]
    fn test_parse_http_remote_simple() {
        let (host, path) = parse_http_remote("github.com/owner/repo").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "owner/repo");
    }

    #[test]
    fn test_parse_http_remote_with_git_extension() {
        let (host, path) = parse_http_remote("github.com/owner/repo.git").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "owner/repo");
    }
}
