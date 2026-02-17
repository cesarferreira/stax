use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
    /// AI agent to use: "claude" or "codex" (default: auto-detect)
    #[serde(default)]
    pub agent: Option<String>,
    /// Model to use with the AI agent (default: agent's own default)
    #[serde(default)]
    pub model: Option<String>,
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

    /// Get GitHub token (from env var or credentials file)
    /// Priority: 1. STAX_GITHUB_TOKEN, 2. GITHUB_TOKEN, 3. credentials file
    pub fn github_token() -> Option<String> {
        // First try stax-specific env var
        if let Ok(token) = std::env::var("STAX_GITHUB_TOKEN") {
            if !token.is_empty() {
                return Some(token);
            }
        }
        // Then try generic GitHub token
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.is_empty() {
                return Some(token);
            }
        }

        // Then try credentials file
        if let Ok(path) = Self::credentials_path() {
            if let Ok(token) = fs::read_to_string(path) {
                let token = token.trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }

        None
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
    /// Priority: 1. config.branch.user, 2. git config user.name, 3. empty string
    fn get_user_for_branch(&self) -> String {
        // First check config
        if let Some(ref user) = self.branch.user {
            if !user.is_empty() {
                return self.sanitize_branch_segment(user);
            }
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
