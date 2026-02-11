use super::*;
use std::env;
use std::fs;

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
    // Special chars are replaced with dashes; leading/trailing dashes are trimmed
    assert_eq!(
        config.format_branch_name("feat: add stuff!"),
        "feat-add-stuff"
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

// ========== Format template tests ==========

#[test]
fn test_format_template_message_only() {
    let mut config = Config::default();
    config.branch.format = Some("{message}".to_string());
    assert_eq!(config.format_branch_name("my-feature"), "my-feature");
}

#[test]
fn test_format_template_user_message() {
    let mut config = Config::default();
    config.branch.format = Some("{user}/{message}".to_string());
    config.branch.user = Some("alice".to_string());
    assert_eq!(config.format_branch_name("my-feature"), "alice/my-feature");
}

#[test]
fn test_format_template_user_date_message() {
    let mut config = Config::default();
    config.branch.format = Some("{user}/{date}/{message}".to_string());
    config.branch.user = Some("bob".to_string());
    config.branch.date_format = "%m-%d".to_string();

    let result = config.format_branch_name("add login");

    // Result should be like "bob/01-19/add-login"
    assert!(
        result.starts_with("bob/"),
        "expected bob/ prefix, got: {}",
        result
    );
    assert!(
        result.ends_with("/add-login"),
        "expected /add-login suffix, got: {}",
        result
    );
    let parts: Vec<&str> = result.split('/').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], "bob");
    // Date should match pattern NN-NN
    assert!(
        parts[1].len() == 5 && parts[1].chars().nth(2) == Some('-'),
        "Date should be MM-DD format, got: {}",
        parts[1]
    );
}

#[test]
fn test_format_template_date_message() {
    let mut config = Config::default();
    config.branch.format = Some("{date}/{message}".to_string());
    config.branch.date_format = "%Y-%m-%d".to_string();

    let result = config.format_branch_name("fix bug");

    // Result should be like "2026-01-19/fix-bug"
    assert!(
        result.ends_with("/fix-bug"),
        "expected /fix-bug suffix, got: {}",
        result
    );
    let parts: Vec<&str> = result.split('/').collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts[0].len(),
        10,
        "Date should be YYYY-MM-DD format, got: {}",
        parts[0]
    );
}

#[test]
fn test_format_template_sanitizes_user() {
    let mut config = Config::default();
    config.branch.format = Some("{user}/{message}".to_string());
    config.branch.user = Some("John Doe".to_string());

    let result = config.format_branch_name("feature");
    assert_eq!(result, "John-Doe/feature");
}

#[test]
fn test_format_template_sanitizes_message() {
    let mut config = Config::default();
    config.branch.format = Some("{user}/{message}".to_string());
    config.branch.user = Some("alice".to_string());

    let result = config.format_branch_name("add user login!");
    assert_eq!(result, "alice/add-user-login");
}

#[test]
fn test_format_template_with_prefix_override() {
    let mut config = Config::default();
    config.branch.format = Some("{message}".to_string());

    let result = config.format_branch_name_with_prefix_override("feature", Some("hotfix"));
    assert_eq!(result, "hotfix/feature");
}

#[test]
fn test_format_template_collapses_consecutive_dashes() {
    let mut config = Config::default();
    config.branch.format = Some("{message}".to_string());

    let result = config.format_branch_name("fix   multiple   spaces");
    assert_eq!(result, "fix-multiple-spaces");
}

#[test]
fn test_format_template_empty_user_no_leading_slash() {
    // When {user} resolves to empty, the branch name must not start with "/"
    let mut config = Config::default();
    config.branch.format = Some("{user}/{date}/{message}".to_string());
    config.branch.user = None; // no configured user
    config.branch.date_format = "%m-%d".to_string();

    let result = config.format_branch_name("my-feature");

    // Should not start or end with "/"
    assert!(
        !result.starts_with('/'),
        "branch name must not start with /, got: {}",
        result
    );
    assert!(
        !result.ends_with('/'),
        "branch name must not end with /, got: {}",
        result
    );
    assert!(
        !result.contains("//"),
        "branch name must not contain //, got: {}",
        result
    );
    assert!(
        result.ends_with("/my-feature") || result == "my-feature",
        "branch name should end with message, got: {}",
        result
    );
}

#[test]
fn test_format_template_empty_user_message_only_format() {
    // {user}/{message} with no user should collapse to just message
    let mut config = Config::default();
    config.branch.format = Some("{user}/{message}".to_string());
    config.branch.user = Some("".to_string()); // explicitly empty

    let result = config.format_branch_name("my-feature");
    assert_eq!(
        result, "my-feature",
        "empty user should collapse to just message"
    );
}

#[test]
fn test_legacy_behavior_without_format() {
    // When format is None, should use legacy prefix/date behavior
    let mut config = Config::default();
    config.branch.prefix = Some("legacy/".to_string());
    config.branch.date = false;

    let result = config.format_branch_name("my-feature");
    assert_eq!(result, "legacy/my-feature");
}

#[test]
fn test_format_template_overrides_legacy_prefix() {
    // When format is set, legacy prefix should be ignored
    let mut config = Config::default();
    config.branch.prefix = Some("legacy/".to_string());
    config.branch.format = Some("{message}".to_string());

    let result = config.format_branch_name("my-feature");
    assert_eq!(result, "my-feature");
}

#[test]
fn test_format_template_custom_date_format() {
    let mut config = Config::default();
    config.branch.format = Some("{date}-{message}".to_string());
    config.branch.date_format = "%Y%m%d".to_string();

    let result = config.format_branch_name("feature");

    // Result should be like "20260119-feature"
    assert!(
        result.ends_with("-feature"),
        "expected -feature suffix, got: {}",
        result
    );
    let date_part = result.trim_end_matches("-feature");
    assert_eq!(
        date_part.len(),
        8,
        "Date should be YYYYMMDD format, got: {}",
        date_part
    );
}

#[test]
fn test_legacy_date_uses_original_format() {
    // Legacy date=true must use %Y-%m-%d (the original hardcoded format),
    // NOT the new date_format field, for backward compatibility
    let mut config = Config::default();
    config.branch.date = true;
    config.branch.date_format = "%m-%d".to_string(); // new field, should be ignored in legacy

    let result = config.format_branch_name("feature");

    // Should be like "2026-02-11-feature" (YYYY-MM-DD), not "02-11-feature"
    let parts: Vec<&str> = result.splitn(2, "-feature").collect();
    let date_part = parts[0].trim_end_matches('-');
    assert_eq!(
        date_part.len(),
        10,
        "Legacy date should be YYYY-MM-DD, got: {}",
        date_part
    );
}

#[test]
fn test_format_deserialization() {
    let toml_str = r#"
[branch]
format = "{user}/{date}/{message}"
user = "testuser"
date_format = "%Y-%m-%d"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(
        config.branch.format,
        Some("{user}/{date}/{message}".to_string())
    );
    assert_eq!(config.branch.user, Some("testuser".to_string()));
    assert_eq!(config.branch.date_format, "%Y-%m-%d");
}

#[test]
fn test_format_serialization() {
    let mut config = Config::default();
    config.branch.format = Some("{user}/{message}".to_string());
    config.branch.user = Some("alice".to_string());

    let toml_str = toml::to_string(&config).unwrap();
    assert!(toml_str.contains("format = \"{user}/{message}\""));
    assert!(toml_str.contains("user = \"alice\""));
}

#[test]
fn test_format_backward_compat_missing_fields() {
    // Old configs without format/user/date_format should still parse fine
    let toml_str = r#"
[branch]
prefix = "cesar/"
date = false
replacement = "-"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.branch.prefix, Some("cesar/".to_string()));
    assert!(config.branch.format.is_none());
    assert!(config.branch.user.is_none());
    assert_eq!(config.branch.date_format, "%m-%d");
    // Legacy behavior should still work
    assert_eq!(config.format_branch_name("feature"), "cesar/feature");
}
