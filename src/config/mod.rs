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
}

#[derive(Debug, Serialize, Deserialize)]
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

impl Default for BranchConfig {
    fn default() -> Self {
        Self {
            prefix: None,
            date: false,
            replacement: default_replacement(),
        }
    }
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
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.branch.prefix.is_none());
        assert!(!config.branch.date);
        assert_eq!(config.branch.replacement, "-");
        assert_eq!(config.remote.name, "origin");
        assert_eq!(config.remote.base_url, "https://github.com");
        assert!(config.ui.tips);
    }

    #[test]
    fn test_format_branch_name_no_prefix() {
        let config = Config::default();
        assert_eq!(config.format_branch_name("my-feature"), "my-feature");
    }

    #[test]
    fn test_format_branch_name_with_prefix() {
        let mut config = Config::default();
        config.branch.prefix = Some("cesar/".to_string());
        assert_eq!(config.format_branch_name("my-feature"), "cesar/my-feature");
    }

    #[test]
    fn test_format_branch_name_prefix_not_duplicated() {
        let mut config = Config::default();
        config.branch.prefix = Some("cesar/".to_string());
        // If name already has prefix, don't add it again
        assert_eq!(
            config.format_branch_name("cesar/my-feature"),
            "cesar/my-feature"
        );
    }

    #[test]
    fn test_format_branch_name_prefix_override() {
        let mut config = Config::default();
        config.branch.prefix = Some("cesar/".to_string());
        assert_eq!(
            config.format_branch_name_with_prefix_override("auth", Some("feature")),
            "feature/auth"
        );
    }

    #[test]
    fn test_format_branch_name_prefix_override_empty_disables() {
        let mut config = Config::default();
        config.branch.prefix = Some("cesar/".to_string());
        assert_eq!(
            config.format_branch_name_with_prefix_override("auth", Some("")),
            "auth"
        );
    }

    #[test]
    fn test_format_branch_name_spaces_replaced() {
        let config = Config::default();
        assert_eq!(
            config.format_branch_name("my cool feature"),
            "my-cool-feature"
        );
    }

    #[test]
    fn test_format_branch_name_special_chars_replaced() {
        let config = Config::default();
        assert_eq!(
            config.format_branch_name("feat: add stuff!"),
            "feat-add-stuff-"
        );
    }

    #[test]
    fn test_format_branch_name_custom_replacement() {
        let mut config = Config::default();
        config.branch.replacement = "_".to_string();
        assert_eq!(
            config.format_branch_name("my cool feature"),
            "my_cool_feature"
        );
    }

    #[test]
    fn test_format_branch_name_consecutive_replacements_collapsed() {
        let config = Config::default();
        // Multiple spaces should become single dash
        assert_eq!(config.format_branch_name("my   feature"), "my-feature");
    }

    #[test]
    fn test_token_priority_stax_env_first() {
        // Save original values
        let orig_stax = env::var("STAX_GITHUB_TOKEN").ok();
        let orig_github = env::var("GITHUB_TOKEN").ok();

        // Set both env vars
        env::set_var("STAX_GITHUB_TOKEN", "stax-token");
        env::set_var("GITHUB_TOKEN", "github-token");

        // STAX_GITHUB_TOKEN should take priority
        let token = Config::github_token();
        assert_eq!(token, Some("stax-token".to_string()));

        // Restore original values
        match orig_stax {
            Some(v) => env::set_var("STAX_GITHUB_TOKEN", v),
            None => env::remove_var("STAX_GITHUB_TOKEN"),
        }
        match orig_github {
            Some(v) => env::set_var("GITHUB_TOKEN", v),
            None => env::remove_var("GITHUB_TOKEN"),
        }
    }

    #[test]
    fn test_token_fallback_to_github_token() {
        // Save original values
        let orig_stax = env::var("STAX_GITHUB_TOKEN").ok();
        let orig_github = env::var("GITHUB_TOKEN").ok();

        // Only set GITHUB_TOKEN
        env::remove_var("STAX_GITHUB_TOKEN");
        env::set_var("GITHUB_TOKEN", "github-token");

        let token = Config::github_token();
        assert_eq!(token, Some("github-token".to_string()));

        // Restore original values
        match orig_stax {
            Some(v) => env::set_var("STAX_GITHUB_TOKEN", v),
            None => env::remove_var("STAX_GITHUB_TOKEN"),
        }
        match orig_github {
            Some(v) => env::set_var("GITHUB_TOKEN", v),
            None => env::remove_var("GITHUB_TOKEN"),
        }
    }

    #[test]
    fn test_token_empty_string_ignored() {
        // Save original values
        let orig_stax = env::var("STAX_GITHUB_TOKEN").ok();
        let orig_github = env::var("GITHUB_TOKEN").ok();

        // Set empty STAX token, valid GITHUB token
        env::set_var("STAX_GITHUB_TOKEN", "");
        env::set_var("GITHUB_TOKEN", "github-token");

        let token = Config::github_token();
        assert_eq!(token, Some("github-token".to_string()));

        // Restore original values
        match orig_stax {
            Some(v) => env::set_var("STAX_GITHUB_TOKEN", v),
            None => env::remove_var("STAX_GITHUB_TOKEN"),
        }
        match orig_github {
            Some(v) => env::set_var("GITHUB_TOKEN", v),
            None => env::remove_var("GITHUB_TOKEN"),
        }
    }

    #[test]
    fn test_default_ui_config() {
        let ui_config = UiConfig::default();
        assert!(ui_config.tips);
    }

    #[test]
    fn test_ui_tips_serialization() {
        // Test that tips=true serializes correctly
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("[ui]"));
        assert!(toml_str.contains("tips = true"));

        // Test that tips=false deserializes correctly
        let toml_with_tips_false = r#"
[ui]
tips = false
"#;
        let parsed: Config = toml::from_str(toml_with_tips_false).unwrap();
        assert!(!parsed.ui.tips);

        // Test that missing [ui] section defaults tips to true
        let toml_without_ui = r#"
[branch]
prefix = "test/"
"#;
        let parsed: Config = toml::from_str(toml_without_ui).unwrap();
        assert!(parsed.ui.tips);
    }

    #[test]
    fn test_set_github_token_writes_to_file() {
        // Save original HOME
        let orig_home = env::var("HOME").ok();

        // Create temp directory
        let temp_dir = std::env::temp_dir().join(format!("stax-test-{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();

        // Override HOME to use temp directory
        env::set_var("HOME", &temp_dir);

        // Write token
        let test_token = "ghp_test_token_12345";
        let result = Config::set_github_token(test_token);
        assert!(result.is_ok(), "set_github_token should succeed");

        // Verify file was created with correct content
        let creds_path = temp_dir.join(".config").join("stax").join(".credentials");
        assert!(creds_path.exists(), "Credentials file should exist");

        let contents = fs::read_to_string(&creds_path).unwrap();
        assert_eq!(contents, test_token);

        // Verify permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::metadata(&creds_path).unwrap().permissions();
            assert_eq!(
                perms.mode() & 0o777,
                0o600,
                "File should have 600 permissions"
            );
        }

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
        match orig_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
    }

    #[test]
    fn test_github_token_reads_from_credentials_file() {
        // Save original values
        let orig_home = env::var("HOME").ok();
        let orig_stax = env::var("STAX_GITHUB_TOKEN").ok();
        let orig_github = env::var("GITHUB_TOKEN").ok();

        // Create temp directory with credentials file
        let temp_dir = std::env::temp_dir().join(format!("stax-test-read-{}", std::process::id()));
        let config_dir = temp_dir.join(".config").join("stax");
        fs::create_dir_all(&config_dir).unwrap();

        let test_token = "ghp_file_token_67890";
        fs::write(config_dir.join(".credentials"), test_token).unwrap();

        // Override HOME and clear env vars
        env::set_var("HOME", &temp_dir);
        env::remove_var("STAX_GITHUB_TOKEN");
        env::remove_var("GITHUB_TOKEN");

        // Read token - should come from file
        let token = Config::github_token();
        assert_eq!(token, Some(test_token.to_string()));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
        match orig_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
        match orig_stax {
            Some(v) => env::set_var("STAX_GITHUB_TOKEN", v),
            None => env::remove_var("STAX_GITHUB_TOKEN"),
        }
        match orig_github {
            Some(v) => env::set_var("GITHUB_TOKEN", v),
            None => env::remove_var("GITHUB_TOKEN"),
        }
    }

    #[test]
    fn test_github_token_roundtrip() {
        // Save original HOME
        let orig_home = env::var("HOME").ok();

        // Create temp directory with unique name including thread id
        let thread_id = std::thread::current().id();
        let temp_dir = std::env::temp_dir().join(format!(
            "stax-test-roundtrip-{}-{:?}",
            std::process::id(),
            thread_id
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        // Override HOME
        env::set_var("HOME", &temp_dir);

        // Write token
        let test_token = "ghp_roundtrip_token_abcdef";
        Config::set_github_token(test_token).unwrap();

        // Verify by reading file directly (avoids env var race conditions)
        let creds_path = temp_dir.join(".config").join("stax").join(".credentials");
        let contents = fs::read_to_string(&creds_path).unwrap();
        assert_eq!(contents, test_token);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
        match orig_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
    }

    #[test]
    fn test_github_token_env_takes_priority_over_file() {
        // Save original values
        let orig_home = env::var("HOME").ok();
        let orig_stax = env::var("STAX_GITHUB_TOKEN").ok();
        let orig_github = env::var("GITHUB_TOKEN").ok();

        // Create temp directory with credentials file
        let temp_dir =
            std::env::temp_dir().join(format!("stax-test-priority-{}", std::process::id()));
        let config_dir = temp_dir.join(".config").join("stax");
        fs::create_dir_all(&config_dir).unwrap();

        let file_token = "ghp_from_file";
        let env_token = "ghp_from_env";
        fs::write(config_dir.join(".credentials"), file_token).unwrap();

        // Set HOME and env var
        env::set_var("HOME", &temp_dir);
        env::remove_var("STAX_GITHUB_TOKEN");
        env::set_var("GITHUB_TOKEN", env_token);

        // Env var should take priority over file
        let token = Config::github_token();
        assert_eq!(token, Some(env_token.to_string()));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
        match orig_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
        match orig_stax {
            Some(v) => env::set_var("STAX_GITHUB_TOKEN", v),
            None => env::remove_var("STAX_GITHUB_TOKEN"),
        }
        match orig_github {
            Some(v) => env::set_var("GITHUB_TOKEN", v),
            None => env::remove_var("GITHUB_TOKEN"),
        }
    }

    #[test]
    fn test_github_token_trims_whitespace_from_file() {
        // Save original values
        let orig_home = env::var("HOME").ok();
        let orig_stax = env::var("STAX_GITHUB_TOKEN").ok();
        let orig_github = env::var("GITHUB_TOKEN").ok();

        // Create temp directory with credentials file containing whitespace
        let temp_dir = std::env::temp_dir().join(format!("stax-test-trim-{}", std::process::id()));
        let config_dir = temp_dir.join(".config").join("stax");
        fs::create_dir_all(&config_dir).unwrap();

        let token_with_whitespace = "  ghp_token_with_spaces  \n";
        fs::write(config_dir.join(".credentials"), token_with_whitespace).unwrap();

        // Override HOME and clear env vars
        env::set_var("HOME", &temp_dir);
        env::remove_var("STAX_GITHUB_TOKEN");
        env::remove_var("GITHUB_TOKEN");

        // Token should be trimmed
        let token = Config::github_token();
        assert_eq!(token, Some("ghp_token_with_spaces".to_string()));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
        match orig_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
        match orig_stax {
            Some(v) => env::set_var("STAX_GITHUB_TOKEN", v),
            None => env::remove_var("STAX_GITHUB_TOKEN"),
        }
        match orig_github {
            Some(v) => env::set_var("GITHUB_TOKEN", v),
            None => env::remove_var("GITHUB_TOKEN"),
        }
    }
}
