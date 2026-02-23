use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Main config (safe to commit to dotfiles)
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub branch: BranchConfig,
    #[serde(default)]
    pub remote: RemoteConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub auth: AuthConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BranchConfig {
    /// Prefix for new branches (e.g., "cesar/")
    /// DEPRECATED: Use `format` instead. Kept for backward compatibility.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Whether to add date to branch names
    /// DEPRECATED: Use `format` instead. Kept for backward compatibility.
    #[serde(default)]
    pub date: bool,
    /// Date format string (default: "%m-%d", e.g., "01-19")
    /// Use chrono strftime format: %Y=year, %m=month, %d=day
    #[serde(default = "default_date_format")]
    pub date_format: String,
    /// Character to replace spaces and special chars (default: "-")
    #[serde(default = "default_replacement")]
    pub replacement: String,
    /// Branch name format template. Placeholders:
    /// - {user}: Git username (from config.branch.user or git user.name)
    /// - {date}: Current date (formatted by date_format)
    /// - {message}: The branch name/message input
    ///
    /// Examples: "{message}", "{user}/{message}", "{user}/{date}/{message}"
    #[serde(default)]
    pub format: Option<String>,
    /// Username for branch naming. If not set, uses git config user.name
    #[serde(default)]
    pub user: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// Git remote name (default: "origin")
    #[serde(default = "default_remote_name")]
    pub name: String,
    /// Base web URL for GitHub (e.g., https://github.com or GitHub Enterprise URL)
    #[serde(default = "default_remote_base_url")]
    pub base_url: String,
    /// API base URL (GitHub Enterprise), e.g., https://github.company.com/api/v3
    #[serde(default)]
    pub api_base_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UiConfig {
    /// Whether to show contextual tips/suggestions (default: true)
    #[serde(default = "default_tips")]
    pub tips: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AiConfig {
    /// AI agent to use: "claude", "codex", "gemini", or "opencode" (default: auto-detect)
    #[serde(default)]
    pub agent: Option<String>,
    /// Model to use with the AI agent (default: agent's own default)
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Whether to use `gh auth token` as a fallback auth source (default: true)
    #[serde(default = "default_use_gh_cli")]
    pub use_gh_cli: bool,
    /// Whether to allow ambient GITHUB_TOKEN env var (default: false)
    #[serde(default = "default_allow_github_token_env")]
    pub allow_github_token_env: bool,
    /// Optional GitHub hostname for `gh auth token --hostname` (enterprise)
    #[serde(default)]
    pub gh_hostname: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubAuthSource {
    StaxGithubTokenEnv,
    CredentialsFile,
    GhCli,
    GithubTokenEnv,
}

impl GitHubAuthSource {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::StaxGithubTokenEnv => "STAX_GITHUB_TOKEN",
            Self::CredentialsFile => "credentials file (~/.config/stax/.credentials)",
            Self::GhCli => "gh auth token",
            Self::GithubTokenEnv => "GITHUB_TOKEN",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GitHubAuthStatus {
    pub active_source: Option<GitHubAuthSource>,
    pub stax_env_available: bool,
    pub credentials_file_available: bool,
    pub gh_cli_available: bool,
    pub github_env_available: bool,
    pub use_gh_cli: bool,
    pub allow_github_token_env: bool,
    pub gh_hostname: Option<String>,
}

impl Default for BranchConfig {
    fn default() -> Self {
        Self {
            prefix: None,
            date: false,
            date_format: default_date_format(),
            replacement: default_replacement(),
            format: None,
            user: None,
        }
    }
}

fn default_date_format() -> String {
    "%m-%d".to_string()
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            name: default_remote_name(),
            base_url: default_remote_base_url(),
            api_base_url: None,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            tips: default_tips(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            use_gh_cli: default_use_gh_cli(),
            allow_github_token_env: default_allow_github_token_env(),
            gh_hostname: None,
        }
    }
}

fn default_replacement() -> String {
    "-".to_string()
}

fn default_remote_name() -> String {
    "origin".to_string()
}

fn default_remote_base_url() -> String {
    "https://github.com".to_string()
}

fn default_tips() -> bool {
    true
}

fn default_use_gh_cli() -> bool {
    true
}

fn default_allow_github_token_env() -> bool {
    false
}

impl Config {
    /// Get the config directory (~/.config/stax on all platforms)
    pub fn dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not find home directory")?;
        Ok(home.join(".config").join("stax"))
    }

    /// Get the config file path
    pub fn path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("config.toml"))
    }

    /// Get the credentials file path (separate from config, not for dotfiles)
    fn credentials_path() -> Result<PathBuf> {
        Ok(Self::dir()?.join(".credentials"))
    }

    /// Ensure config exists, creating default if needed
    /// Call this once at startup
    pub fn ensure_exists() -> Result<()> {
        let path = Self::path()?;
        if !path.exists() {
            let config = Config::default();
            config.save()?;
        }
        Ok(())
    }

    /// Load config from file
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    /// Save config to file
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// Get GitHub token (from env var, credentials file, or gh cli)
    /// Priority:
    /// 1. STAX_GITHUB_TOKEN
    /// 2. credentials file (~/.config/stax/.credentials)
    /// 3. gh auth token (if auth.use_gh_cli = true)
    /// 4. GITHUB_TOKEN (if auth.allow_github_token_env = true)
    pub fn github_token() -> Option<String> {
        let auth_config = Self::load().map(|c| c.auth).unwrap_or_default();
        Self::resolve_github_auth_with_config(&auth_config).map(|(_, token)| token)
    }

    pub fn github_auth_status() -> GitHubAuthStatus {
        let auth_config = Self::load().map(|c| c.auth).unwrap_or_default();

        let stax_env_available = Self::read_env_token("STAX_GITHUB_TOKEN").is_some();
        let credentials_file_available = Self::token_from_credentials_file().is_some();
        let gh_cli_available = if auth_config.use_gh_cli {
            Self::token_from_gh_cli(auth_config.gh_hostname.as_deref())
                .ok()
                .flatten()
                .is_some()
        } else {
            false
        };
        let github_env_available = Self::read_env_token("GITHUB_TOKEN").is_some();

        let active_source = if stax_env_available {
            Some(GitHubAuthSource::StaxGithubTokenEnv)
        } else if credentials_file_available {
            Some(GitHubAuthSource::CredentialsFile)
        } else if auth_config.use_gh_cli && gh_cli_available {
            Some(GitHubAuthSource::GhCli)
        } else if auth_config.allow_github_token_env && github_env_available {
            Some(GitHubAuthSource::GithubTokenEnv)
        } else {
            None
        };

        GitHubAuthStatus {
            active_source,
            stax_env_available,
            credentials_file_available,
            gh_cli_available,
            github_env_available,
            use_gh_cli: auth_config.use_gh_cli,
            allow_github_token_env: auth_config.allow_github_token_env,
            gh_hostname: auth_config.gh_hostname,
        }
    }

    /// Set GitHub token (to credentials file)
    pub fn set_github_token(token: &str) -> Result<()> {
        let path = Self::credentials_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, token)?;

        // Set restrictive permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    /// Read token from gh CLI for explicit import (`stax auth --from-gh`).
    pub fn gh_cli_token_for_import() -> Result<String> {
        let auth_config = Self::load().map(|c| c.auth).unwrap_or_default();

        Self::token_from_gh_cli(auth_config.gh_hostname.as_deref())?.context(
            "Could not read token from `gh auth token`.\n\
             Ensure GitHub CLI is installed and authenticated (`gh auth login`).",
        )
    }

    fn read_env_token(var_name: &str) -> Option<String> {
        std::env::var(var_name)
            .ok()
            .and_then(|value| Self::normalize_token(value.as_str()))
    }

    fn token_from_credentials_file() -> Option<String> {
        let path = Self::credentials_path().ok()?;
        let token = fs::read_to_string(path).ok()?;
        Self::normalize_token(token.as_str())
    }

    fn token_from_gh_cli(hostname: Option<&str>) -> Result<Option<String>> {
        let mut command = Command::new("gh");
        command.args(["auth", "token"]);
        if let Some(host) = hostname.and_then(Self::normalize_token) {
            command.args(["--hostname", host.as_str()]);
        }

        let output = match command.output() {
            Ok(output) => output,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err).context("Failed to execute `gh auth token`"),
        };

        if !output.status.success() {
            return Ok(None);
        }

        let token = String::from_utf8_lossy(&output.stdout);
        Ok(Self::normalize_token(token.as_ref()))
    }

    fn normalize_token(token: &str) -> Option<String> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn resolve_github_auth_with_config(
        auth_config: &AuthConfig,
    ) -> Option<(GitHubAuthSource, String)> {
        if let Some(token) = Self::read_env_token("STAX_GITHUB_TOKEN") {
            return Some((GitHubAuthSource::StaxGithubTokenEnv, token));
        }

        if let Some(token) = Self::token_from_credentials_file() {
            return Some((GitHubAuthSource::CredentialsFile, token));
        }

        if auth_config.use_gh_cli {
            if let Ok(Some(token)) = Self::token_from_gh_cli(auth_config.gh_hostname.as_deref()) {
                return Some((GitHubAuthSource::GhCli, token));
            }
        }

        if auth_config.allow_github_token_env {
            if let Some(token) = Self::read_env_token("GITHUB_TOKEN") {
                return Some((GitHubAuthSource::GithubTokenEnv, token));
            }
        }

        None
    }

    /// Format a branch name according to config settings
    pub fn format_branch_name(&self, name: &str) -> String {
        self.format_branch_name_with_prefix_override(name, None)
    }

    /// Format a branch name, optionally overriding the configured prefix
    pub fn format_branch_name_with_prefix_override(
        &self,
        name: &str,
        prefix_override: Option<&str>,
    ) -> String {
        // Sanitize the message/name first
        let sanitized_name = self.sanitize_branch_segment(name);

        // If format template is set, use it (new behavior)
        if let Some(ref format_template) = self.branch.format {
            if !format_template.contains("{message}") {
                eprintln!(
                    "Warning: branch.format template is missing {{message}} placeholder. \
                     The branch name input will not appear in the generated name."
                );
            }
            return self.apply_format_template(format_template, &sanitized_name, prefix_override);
        }

        // Legacy behavior: use prefix/date fields for backward compatibility
        let replacement = &self.branch.replacement;
        let mut result = sanitized_name;

        // Add date if enabled (legacy, preserves original %Y-%m-%d format)
        if self.branch.date {
            let date = chrono::Local::now().format("%Y-%m-%d").to_string();
            result = format!("{}{}{}", date, replacement, result);
        }

        let prefix = if let Some(override_prefix) = prefix_override {
            let trimmed = override_prefix.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(Self::normalize_prefix_override(trimmed))
            }
        } else {
            self.branch.prefix.clone()
        };

        if let Some(prefix) = prefix {
            if !result.starts_with(&prefix) {
                result = format!("{}{}", prefix, result);
            }
        }

        result
    }

    /// Apply the format template to create a branch name
    fn apply_format_template(
        &self,
        template: &str,
        message: &str,
        prefix_override: Option<&str>,
    ) -> String {
        let mut result = template.to_string();

        // Replace {message} placeholder
        result = result.replace("{message}", message);

        // Replace {date} placeholder if present
        if result.contains("{date}") {
            let date = chrono::Local::now()
                .format(&self.branch.date_format)
                .to_string();
            result = result.replace("{date}", &date);
        }

        // Replace {user} placeholder if present
        if result.contains("{user}") {
            let user = self.get_user_for_branch();
            result = result.replace("{user}", &user);
        }

        // Clean up empty segments: collapse repeated separators and trim leading/trailing ones
        // This handles cases where {user} resolves to "" (e.g., "/02-11/msg" -> "02-11/msg")
        while result.contains("//") {
            result = result.replace("//", "/");
        }
        result = result.trim_matches('/').to_string();

        // Handle prefix override (for -p flag compatibility)
        if let Some(override_prefix) = prefix_override {
            let trimmed = override_prefix.trim();
            if !trimmed.is_empty() {
                let normalized = Self::normalize_prefix_override(trimmed);
                if !result.starts_with(&normalized) {
                    result = format!("{}{}", normalized, result);
                }
            }
        }

        result
    }

    /// Sanitize a segment of the branch name (replace special chars, collapse duplicates)
    fn sanitize_branch_segment(&self, segment: &str) -> String {
        let replacement = &self.branch.replacement;

        let mut result: String = segment
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == '/' {
                    c
                } else {
                    replacement.chars().next().unwrap_or('-')
                }
            })
            .collect();

        // Replace multiple consecutive replacements with single one
        while result.contains(&format!("{}{}", replacement, replacement)) {
            result = result.replace(&format!("{}{}", replacement, replacement), replacement);
        }

        // Trim leading/trailing replacement chars
        let replacement_char = replacement.chars().next().unwrap_or('-');
        result = result
            .trim_start_matches(replacement_char)
            .trim_end_matches(replacement_char)
            .to_string();

        result
    }

    /// Get the username for branch naming
    /// Priority: 1. config.branch.user (explicit empty disables fallback),
    /// 2. git config user.name, 3. empty string
    fn get_user_for_branch(&self) -> String {
        // First check config
        if let Some(ref user) = self.branch.user {
            if user.is_empty() {
                return String::new();
            }
            return self.sanitize_branch_segment(user);
        }

        // Then try git config user.name
        if let Ok(output) = std::process::Command::new("git")
            .args(["config", "user.name"])
            .output()
        {
            if output.status.success() {
                let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !name.is_empty() {
                    return self.sanitize_branch_segment(&name);
                }
            }
        }

        // Fallback to empty
        String::new()
    }

    fn normalize_prefix_override(prefix: &str) -> String {
        if prefix.ends_with('/') || prefix.ends_with('-') || prefix.ends_with('_') {
            prefix.to_string()
        } else {
            format!("{}/", prefix)
        }
    }

    pub fn remote_name(&self) -> &str {
        self.remote.name.as_str()
    }

    pub fn remote_base_url(&self) -> &str {
        self.remote.base_url.as_str()
    }
}

#[cfg(test)]
mod tests;
