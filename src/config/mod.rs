use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub github: GitHubConfig,
    #[serde(default)]
    pub branch: BranchConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GitHubConfig {
    pub token: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UiConfig {
    /// Whether to show tips
    #[serde(default = "default_tips")]
    pub tips: bool,
}

fn default_replacement() -> String {
    "-".to_string()
}

fn default_tips() -> bool {
    true
}

impl Config {
    /// Get the config file path
    pub fn path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Could not find config directory")?
            .join("stax");
        Ok(config_dir.join("config.toml"))
    }

    /// Load config from file, or return default
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

    /// Get GitHub token
    pub fn github_token(&self) -> Option<&str> {
        self.github.token.as_deref()
    }

    /// Set GitHub token
    pub fn set_github_token(&mut self, token: &str) {
        self.github.token = Some(token.to_string());
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
