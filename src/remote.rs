use crate::config::Config;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use git2::{ConfigLevel, Repository};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeType {
    GitHub,
    GitLab,
    #[serde(alias = "forgejo")]
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

/// A remote whose Git host, provider, and API destination were validated before
/// noninteractive credential lookup.
#[derive(Debug, Clone)]
pub(crate) struct TrustedRemoteInfo {
    remote: RemoteInfo,
}

impl TrustedRemoteInfo {
    pub(crate) fn from_repo(repo: &GitRepo, global_config: &Config) -> Result<Self> {
        let remote = RemoteInfo::from_repo(repo, global_config)?;
        validate_trusted_network_remote(&remote, global_config)?;
        Ok(Self { remote })
    }

    pub(crate) fn remote(&self) -> &RemoteInfo {
        &self.remote
    }
}

impl RemoteInfo {
    pub fn from_repo(repo: &GitRepo, config: &Config) -> Result<Self> {
        let name = config.remote_name().to_string();
        let url = get_remote_url(repo.workdir()?, &name)?;
        let (host, path) = parse_remote_url(&url)?;
        let forge = detect_forge(
            &host,
            config.remote_base_url(),
            config.remote_forge_override(),
        );
        let (namespace, repo_name) = split_namespace_repo(&path)?;

        let configured_base = config.remote_base_url().trim_end_matches('/');
        let base_url = if configured_base.is_empty()
            || (configured_base == "https://github.com" && host != "github.com")
            || (configured_base == "https://gitlab.com" && host != "gitlab.com")
            || (configured_base == "https://gitea.com" && host != "gitea.com")
        {
            format!("https://{}", url_authority_host(&host))
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

fn detect_forge(
    host: &str,
    configured_base_url: &str,
    forge_override: Option<ForgeType>,
) -> ForgeType {
    if let Some(forge) = forge_override {
        return forge;
    }

    let host = host.to_ascii_lowercase();
    let configured_base_url = configured_base_url.to_ascii_lowercase();

    if host.contains("gitlab") || configured_base_url.contains("gitlab") {
        ForgeType::GitLab
    } else if host.contains("gitea")
        || host.contains("forgejo")
        || configured_base_url.contains("gitea")
        || configured_base_url.contains("forgejo")
    {
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

fn validate_trusted_network_remote(remote: &RemoteInfo, global_config: &Config) -> Result<()> {
    let remote_host = remote.host.to_ascii_lowercase();
    let base_host = network_url_host(&remote.base_url, "provider base URL")?;
    if base_host != remote_host {
        anyhow::bail!(
            "Noninteractive repository network access blocked a provider base URL that does not match the Git \
             remote hostname; configure matching global remote.base_url settings"
        );
    }

    let official_forge = official_forge_for_host(&remote_host);
    if let Some(expected) = official_forge {
        if remote.forge != expected {
            anyhow::bail!(
                "Noninteractive repository network access blocked a provider mismatch for an official forge host; \
                 remove or correct the global remote.forge override"
            );
        }
    } else {
        let configured_base_host =
            network_url_host(&global_config.remote.base_url, "global provider base URL")?;
        if configured_base_host != remote_host {
            anyhow::bail!(
                "Noninteractive repository network access blocked an untrusted Git remote hostname; configure \
                 matching global remote.base_url and remote.forge settings to trust this host"
            );
        }
    }

    let api_url = remote.api_base_url.as_deref().context(
        "Noninteractive repository network access requires a resolved provider API URL in global configuration",
    )?;
    let api_host = network_url_host(api_url, "provider API URL")?;
    let built_in_relationship = matches!(
        (remote.forge, remote_host.as_str(), api_host.as_str()),
        (ForgeType::GitHub, "github.com", "api.github.com")
            | (ForgeType::GitLab, "gitlab.com", "gitlab.com")
            | (ForgeType::Gitea, "gitea.com", "gitea.com")
    );
    if api_host != remote_host
        && !built_in_relationship
        && global_config.remote.api_base_url.is_none()
    {
        anyhow::bail!(
            "Noninteractive repository network access blocked an untrusted provider API hostname; configure the \
             remote/API relationship explicitly in global remote.api_base_url"
        );
    }

    Ok(())
}

fn official_forge_for_host(host: &str) -> Option<ForgeType> {
    match host {
        "github.com" => Some(ForgeType::GitHub),
        "gitlab.com" => Some(ForgeType::GitLab),
        "gitea.com" => Some(ForgeType::Gitea),
        _ => None,
    }
}

fn network_url_host(url: &str, label: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url).with_context(|| {
        format!("Noninteractive repository network access has an invalid {label}")
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("Noninteractive repository network access requires an HTTP(S) {label}");
    }
    parsed_url_host(&parsed).with_context(|| {
        format!("Noninteractive repository network access has an invalid {label} hostname")
    })
}

fn normalize_url_host(host: &str) -> String {
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .to_ascii_lowercase()
}

fn parsed_url_host(parsed: &reqwest::Url) -> Result<String> {
    let host = normalize_url_host(
        parsed
            .host_str()
            .context("Remote URL does not contain a hostname")?,
    );
    if host.chars().any(|character| {
        matches!(character, '@' | '%' | '/' | '\\' | '?' | '#') || character.is_whitespace()
    }) {
        anyhow::bail!("Remote URL contains an ambiguous hostname");
    }
    Ok(host)
}

fn url_authority_host(host: &str) -> String {
    if host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{host}]")
    } else {
        host.to_string()
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

pub fn get_existing_remote_branches_from_repo(
    repo: &Repository,
    remote: &str,
    branches: &[String],
) -> HashSet<String> {
    branches
        .iter()
        .filter(|branch| {
            repo.find_reference(&format!("refs/remotes/{}/{}", remote, branch))
                .is_ok()
        })
        .cloned()
        .collect()
}

/// Remote branch names from `git ls-remote --heads` (no object transfer).
pub fn ls_remote_heads(workdir: &Path, remote: &str) -> Result<HashSet<String>> {
    Ok(ls_remote_head_oids(workdir, remote)?.into_keys().collect())
}

/// Remote branch names and object IDs from `git ls-remote --heads` (no object transfer).
pub fn ls_remote_head_oids(workdir: &Path, remote: &str) -> Result<HashMap<String, String>> {
    let output = Command::new("git")
        .args(["ls-remote", "--heads", remote])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to run git ls-remote --heads {}", remote))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git ls-remote --heads failed ({}): {}",
            output.status,
            stderr.trim()
        );
    }

    let prefix = "refs/heads/";
    let mut heads = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((oid, refpart)) = line.split_once('\t') {
            if let Some(name) = refpart.strip_prefix(prefix) {
                heads.insert(name.to_string(), oid.to_string());
            }
        }
    }
    Ok(heads)
}

/// Fetch only the given branch tips from `remote` (plus any objects reachable from them).
pub fn fetch_remote_refs(workdir: &Path, remote: &str, branches: &[String]) -> Result<()> {
    if branches.is_empty() {
        anyhow::bail!("fetch_remote_refs: no refs to fetch");
    }

    let output = Command::new("git")
        .arg("fetch")
        .arg("--no-tags")
        .arg(remote)
        .args(branches.iter().map(|s| s.as_str()))
        .current_dir(workdir)
        .output()
        .context("Failed to run git fetch")?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    anyhow::bail!(
        "Failed to fetch refs from {}.\n\ngit stdout:\n{}\n\ngit stderr:\n{}",
        remote,
        stdout.trim(),
        stderr.trim()
    );
}

fn parse_remote_url(url: &str) -> Result<(String, String)> {
    if url.contains("://") {
        let parsed = reqwest::Url::parse(url).context("Invalid remote URL")?;
        if !matches!(parsed.scheme(), "ssh" | "http" | "https") {
            anyhow::bail!("Unsupported remote URL scheme");
        }
        return remote_host_and_path(&parsed);
    }

    parse_scp_like_remote(url)
}

fn remote_host_and_path(parsed: &reqwest::Url) -> Result<(String, String)> {
    let host = parsed_url_host(parsed)?;
    let path = parsed
        .path()
        .trim_start_matches('/')
        .trim_end_matches(".git")
        .to_string();
    if path.is_empty() {
        anyhow::bail!("Invalid remote URL: missing repository path");
    }
    Ok((host, path))
}

fn parse_scp_like_remote(url: &str) -> Result<(String, String)> {
    let mut inside_ipv6_literal = false;
    let mut separator = None;
    for (index, character) in url.char_indices() {
        match character {
            '[' if !inside_ipv6_literal => inside_ipv6_literal = true,
            ']' if inside_ipv6_literal => inside_ipv6_literal = false,
            ':' if !inside_ipv6_literal => {
                separator = Some(index);
                break;
            }
            _ => {}
        }
    }
    if inside_ipv6_literal {
        anyhow::bail!("Invalid SCP-like remote URL: unterminated IPv6 hostname");
    }

    let separator = separator.context("Unsupported remote URL format")?;
    let authority = &url[..separator];
    let path = url[separator + 1..].trim_end_matches(".git");
    if authority.is_empty() || path.is_empty() {
        anyhow::bail!("Invalid SCP-like remote URL");
    }
    if authority.matches('@').count() > 1 {
        anyhow::bail!("Invalid SCP-like remote URL: ambiguous user information");
    }
    if let Some((user, _)) = authority.split_once('@')
        && user.is_empty()
    {
        anyhow::bail!("Invalid SCP-like remote URL: empty user information");
    }

    let authority_url = reqwest::Url::parse(&format!("ssh://{authority}/"))
        .context("Invalid SCP-like remote URL authority")?;
    let host = parsed_url_host(&authority_url)?;
    Ok((host, path.to_string()))
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
    fn test_parse_ssh_scheme_url_with_explicit_port() {
        let (host, path) =
            parse_remote_url("ssh://git@gitlab.example.com:2222/org/project.git").unwrap();
        assert_eq!(host, "gitlab.example.com");
        assert_eq!(path, "org/project");
    }

    #[test]
    fn test_parse_ssh_scheme_uses_the_actual_authority_host() {
        let (host, path) =
            parse_remote_url("ssh://git@github.com@attacker.example/org/project.git").unwrap();
        assert_eq!(host, "attacker.example");
        assert_eq!(path, "org/project");
    }

    #[test]
    fn test_parse_ssh_scheme_ignores_userinfo_that_looks_like_a_host_and_port() {
        let (host, path) =
            parse_remote_url("ssh://github.com:22@attacker.example/org/project.git").unwrap();
        assert_eq!(host, "attacker.example");
        assert_eq!(path, "org/project");
    }

    #[test]
    fn test_parse_ssh_scheme_rejects_an_encoded_authority_delimiter() {
        assert!(
            parse_remote_url("ssh://git@github.com%40attacker.example/org/project.git").is_err()
        );
    }

    #[test]
    fn test_parse_ssh_scheme_ipv6_host_and_port() {
        let (host, path) =
            parse_remote_url("ssh://git@[2001:db8::1]:2222/org/project.git").unwrap();
        assert_eq!(host, "2001:db8::1");
        assert_eq!(path, "org/project");
    }

    #[test]
    fn test_parse_scp_like_ipv6_remote() {
        let (host, path) = parse_remote_url("git@[2001:db8::1]:org/project.git").unwrap();
        assert_eq!(host, "2001:db8::1");
        assert_eq!(path, "org/project");
    }

    #[test]
    fn test_parse_scp_like_remote_with_custom_user() {
        let (host, path) = parse_remote_url("deploy@git.example.com:org/project.git").unwrap();
        assert_eq!(host, "git.example.com");
        assert_eq!(path, "org/project");
    }

    #[test]
    fn test_parse_scp_like_remote_rejects_ambiguous_userinfo() {
        let error =
            parse_remote_url("git@github.com@attacker.example:org/project.git").unwrap_err();
        assert!(error.to_string().contains("ambiguous"));
    }

    #[test]
    fn trusted_network_remote_does_not_trust_a_hostname_hidden_in_ssh_userinfo() {
        let (_dir, repo) =
            repo_with_remote("ssh://git@github.com@attacker.example/owner/private-repo.git");

        let error = TrustedRemoteInfo::from_repo(&repo, &Config::default()).unwrap_err();
        let message = error.to_string();

        assert!(message.contains("untrusted"));
        assert!(message.contains("global"));
        assert!(!message.contains("private-repo"));
        assert!(!message.contains("github.com@attacker.example"));
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
    fn test_get_existing_remote_branches_from_repo_checks_only_requested_branches() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = dir.path();

        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(path)
            .output()
            .expect("Failed to init git repo");
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .output()
            .expect("Failed to set email");
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .output()
            .expect("Failed to set name");

        std::fs::write(path.join("README.md"), "base\n").expect("Failed to write file");
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(path)
            .output()
            .expect("Failed to add file");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(path)
            .output()
            .expect("Failed to commit");
        Command::new("git")
            .args(["update-ref", "refs/remotes/origin/main", "HEAD"])
            .current_dir(path)
            .output()
            .expect("Failed to create remote main ref");
        Command::new("git")
            .args([
                "update-ref",
                "refs/remotes/origin/feature/with-slash",
                "HEAD",
            ])
            .current_dir(path)
            .output()
            .expect("Failed to create remote feature ref");

        let repo = Repository::open(path).expect("Failed to open repo");
        let branches = vec![
            "main".to_string(),
            "feature/with-slash".to_string(),
            "missing".to_string(),
        ];

        let existing = get_existing_remote_branches_from_repo(&repo, "origin", &branches);

        assert!(existing.contains("main"));
        assert!(existing.contains("feature/with-slash"));
        assert!(!existing.contains("missing"));
        assert_eq!(existing.len(), 2);
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

    fn repo_with_remote(url: &str) -> (TempDir, GitRepo) {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.remote("origin", url).unwrap();
        drop(repo);
        let repo = GitRepo::open_from_path(dir.path()).unwrap();
        (dir, repo)
    }

    #[test]
    fn trusted_network_remote_trusts_official_host_resolutions() {
        let cases = [
            (
                "https://github.com/owner/repo.git",
                "github.com",
                ForgeType::GitHub,
                "https://github.com",
                "https://api.github.com",
            ),
            (
                "https://gitlab.com/owner/repo.git",
                "gitlab.com",
                ForgeType::GitLab,
                "https://gitlab.com",
                "https://gitlab.com/api/v4",
            ),
            (
                "https://gitea.com/owner/repo.git",
                "gitea.com",
                ForgeType::Gitea,
                "https://gitea.com",
                "https://gitea.com/api/v1",
            ),
        ];

        for (url, host, forge, base_url, api_base_url) in cases {
            let (_dir, repo) = repo_with_remote(url);
            let trusted = TrustedRemoteInfo::from_repo(&repo, &Config::default()).unwrap();
            let remote = trusted.remote();

            assert_eq!(remote.host, host);
            assert_eq!(remote.forge, forge);
            assert_eq!(remote.base_url, base_url);
            assert_eq!(remote.api_base_url.as_deref(), Some(api_base_url));
        }
    }

    #[test]
    fn trusted_network_remote_accepts_globally_configured_custom_relationship() {
        let (_dir, repo) = repo_with_remote("git@git.corp.example:platform/service.git");
        let mut config = Config::default();
        config.remote.base_url = "https://git.corp.example".to_string();
        config.remote.api_base_url = Some("https://api.corp.example/v3".to_string());
        config.remote.forge = Some(ForgeType::GitHub);
        config.auth.gh_hostname = Some("git.corp.example".to_string());

        let trusted = TrustedRemoteInfo::from_repo(&repo, &config).unwrap();
        let remote = trusted.remote();

        assert_eq!(remote.host, "git.corp.example");
        assert_eq!(remote.forge, ForgeType::GitHub);
        assert_eq!(
            remote.api_base_url.as_deref(),
            Some("https://api.corp.example/v3")
        );
    }

    #[test]
    fn trusted_network_remote_accepts_a_globally_configured_ipv6_host() {
        let (_dir, repo) = repo_with_remote("ssh://git@[2001:db8::1]:2222/platform/service.git");
        let mut config = Config::default();
        config.remote.base_url = "https://[2001:db8::1]".to_string();
        config.remote.api_base_url = Some("https://[2001:db8::1]/api/v3".to_string());
        config.remote.forge = Some(ForgeType::GitHub);

        let trusted = TrustedRemoteInfo::from_repo(&repo, &config).unwrap();
        let remote = trusted.remote();

        assert_eq!(remote.host, "2001:db8::1");
        assert_eq!(remote.base_url, "https://[2001:db8::1]");
        assert_eq!(
            remote.api_base_url.as_deref(),
            Some("https://[2001:db8::1]/api/v3")
        );
    }

    #[test]
    fn trusted_network_remote_rejects_unconfigured_unknown_host() {
        let (_dir, repo) = repo_with_remote("https://untrusted.example/owner/private-repo.git");

        let error = TrustedRemoteInfo::from_repo(&repo, &Config::default()).unwrap_err();
        let message = error.to_string();

        assert!(message.contains("untrusted"));
        assert!(message.contains("global"));
        assert!(!message.contains("private-repo"));
    }

    #[test]
    fn trusted_network_remote_rejects_official_provider_mismatch() {
        let (_dir, repo) = repo_with_remote("https://github.com/owner/repo.git");
        let mut config = Config::default();
        config.remote.forge = Some(ForgeType::GitLab);

        let error = TrustedRemoteInfo::from_repo(&repo, &config).unwrap_err();

        assert!(error.to_string().contains("provider"));
    }

    #[test]
    fn trusted_network_remote_rejects_implicit_api_host_mismatch() {
        let mut config = Config::default();
        config.remote.base_url = "https://git.corp.example".to_string();
        config.remote.forge = Some(ForgeType::GitHub);
        let remote = RemoteInfo {
            name: "origin".to_string(),
            forge: ForgeType::GitHub,
            host: "git.corp.example".to_string(),
            namespace: "platform".to_string(),
            repo: "service".to_string(),
            base_url: "https://git.corp.example".to_string(),
            api_base_url: Some("https://api.other.example/v3".to_string()),
        };

        let error = validate_trusted_network_remote(&remote, &config).unwrap_err();

        assert!(error.to_string().contains("API hostname"));
        assert!(error.to_string().contains("global"));
    }

    #[test]
    fn test_detect_forge_prefers_host() {
        assert_eq!(
            detect_forge("gitlab.com", "https://github.com", None),
            ForgeType::GitLab
        );
        assert_eq!(
            detect_forge("gitea.example.com", "https://github.com", None),
            ForgeType::Gitea
        );
        assert_eq!(
            detect_forge("github.example.com", "https://github.com", None),
            ForgeType::GitHub
        );
    }

    #[test]
    fn test_detect_forge_recognizes_forgejo() {
        assert_eq!(
            detect_forge("forgejo.example.com", "https://github.com", None),
            ForgeType::Gitea
        );
        assert_eq!(
            detect_forge("git.example.com", "https://forgejo.example.com", None),
            ForgeType::Gitea
        );
    }

    #[test]
    fn test_detect_forge_explicit_override() {
        // Override should win over hostname-based detection
        assert_eq!(
            detect_forge("github.com", "https://github.com", Some(ForgeType::GitLab)),
            ForgeType::GitLab
        );
        assert_eq!(
            detect_forge(
                "git.mycompany.com",
                "https://git.mycompany.com",
                Some(ForgeType::GitLab)
            ),
            ForgeType::GitLab
        );
        assert_eq!(
            detect_forge("gitlab.com", "https://gitlab.com", Some(ForgeType::GitHub)),
            ForgeType::GitHub
        );
        assert_eq!(
            detect_forge(
                "git.example.com",
                "https://git.example.com",
                Some(ForgeType::Gitea)
            ),
            ForgeType::Gitea
        );
    }

    #[test]
    fn test_forge_type_serde_roundtrip() {
        // Lowercase names deserialize correctly
        assert_eq!(
            serde_json::from_str::<ForgeType>(r#""github""#).unwrap(),
            ForgeType::GitHub
        );
        assert_eq!(
            serde_json::from_str::<ForgeType>(r#""gitlab""#).unwrap(),
            ForgeType::GitLab
        );
        assert_eq!(
            serde_json::from_str::<ForgeType>(r#""gitea""#).unwrap(),
            ForgeType::Gitea
        );
        // "forgejo" alias maps to Gitea
        assert_eq!(
            serde_json::from_str::<ForgeType>(r#""forgejo""#).unwrap(),
            ForgeType::Gitea
        );
    }
}
