use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Main config (safe to commit to dotfiles)
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub branch: BranchConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchConfig {
    /// Prefix for new branches (e.g., "cesar/")
    #[serde(default)]
    pub prefix: Option<String>,
    /// Whether to add date to branch names
    #[serde(default)]
    pub date: bool,
    /// Character to replace spaces and special chars (default: "-")
    #[serde(default = "default_replacement")]
    pub replacement: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UiConfig {
    /// Whether to show tips
    #[serde(default = "default_tips")]
    pub tips: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            branch: BranchConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

impl Default for BranchConfig {
    fn default() -> Self {
        Self {
            prefix: None,
            date: false,
            replacement: default_replacement(),
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

fn default_tips() -> bool {
    true
}

impl Config {
    /// Get the config directory
    pub fn dir() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Could not find config directory")?
            .join("stax");
        Ok(config_dir)
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
    pub fn github_token() -> Option<String> {
        // First try environment variables
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.is_empty() {
                return Some(token);
            }
        }
        if let Ok(token) = std::env::var("STAX_GITHUB_TOKEN") {
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
        let mut result = name.to_string();

        // Replace spaces and special characters
        let replacement = &self.branch.replacement;
        result = result
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

        // Add date if enabled
        if self.branch.date {
            let date = chrono::Local::now().format("%Y-%m-%d").to_string();
            result = format!("{}{}{}", date, replacement, result);
        }

        // Add prefix if set
        if let Some(prefix) = &self.branch.prefix {
            if !result.starts_with(prefix) {
                result = format!("{}{}", prefix, result);
            }
        }

        result
    }
}
