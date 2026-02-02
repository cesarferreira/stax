use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use regex::Regex;
use serde::Serialize;
use std::env;
use std::path::PathBuf;
use std::process::Command;

/// A single commit entry in the changelog
#[derive(Debug, Clone, Serialize)]
struct CommitEntry {
    hash: String,
    short_hash: String,
    message: String,
    author: String,
    pr_number: Option<u64>,
}

/// JSON output structure for changelog
#[derive(Serialize)]
struct ChangelogJson {
    from: String,
    to: String,
    path: Option<String>,
    commit_count: usize,
    commits: Vec<CommitEntry>,
}

pub fn run(from: String, to: String, path: Option<String>, json: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?;

    // Resolve path filter relative to current directory and make it relative to repo root
    let resolved_path = if let Some(p) = path.as_ref() {
        let current_dir = env::current_dir().context("Failed to get current directory")?;
        let path_buf = PathBuf::from(p);

        // Make path absolute if it's relative
        let abs_path = if path_buf.is_absolute() {
            path_buf
        } else {
            current_dir.join(path_buf)
        };

        // Make it relative to the repo root
        let rel_path = abs_path
            .strip_prefix(workdir)
            .context("Path is outside repository")?;

        Some(rel_path.to_string_lossy().to_string())
    } else {
        None
    };

    // Build git log command
    // Use %x00 (NULL byte) as delimiter to handle messages with special characters
    // %aN gives us the author name from git config (user.name)
    let range = format!("{}..{}", from, to);
    let mut args = vec![
        "log".to_string(),
        "--format=%H%x00%s%x00%aN".to_string(),
        range.clone(),
    ];

    // Add path filter if specified
    if let Some(ref p) = resolved_path {
        args.push("--".to_string());
        args.push(p.clone());
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(workdir)
        .output()
        .context("Failed to run git log")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git log failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let commits = parse_commits(&stdout)?;

    if json {
        let output = ChangelogJson {
            from: from.clone(),
            to: to.clone(),
            path: resolved_path.clone(),
            commit_count: commits.len(),
            commits,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Pretty output
    print_changelog(&from, &to, &resolved_path, &commits);

    Ok(())
}

/// Parse git log output into CommitEntry structs
/// Uses NULL byte (\0) as delimiter to handle messages with special characters
fn parse_commits(output: &str) -> Result<Vec<CommitEntry>> {
    let pr_regex = Regex::new(r"\(#(\d+)\)").unwrap();
    let mut commits = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, '\0').collect();
        if parts.len() < 3 {
            continue;
        }

        let hash = parts[0].to_string();
        let short_hash = hash.chars().take(7).collect();
        let message = parts[1].to_string();
        let author = parts[2].to_string();

        // Extract PR number from message (e.g., "feat: add thing (#123)")
        let pr_number = pr_regex
            .captures(&message)
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse::<u64>().ok());

        commits.push(CommitEntry {
            hash,
            short_hash,
            message,
            author,
            pr_number,
        });
    }

    Ok(commits)
}

/// Print a pretty, colorful changelog
fn print_changelog(from: &str, to: &str, path: &Option<String>, commits: &[CommitEntry]) {
    let commit_word = if commits.len() == 1 {
        "commit"
    } else {
        "commits"
    };

    // Header
    println!(
        "{}",
        format!(
            "Changelog: {} → {} ({} {})",
            from,
            to,
            commits.len(),
            commit_word
        )
        .bold()
    );

    // Path filter indicator
    if let Some(p) = path {
        println!("{}", format!("Filtered to: {}", p).dimmed());
    }

    println!("{}", "─".repeat(50).dimmed());
    println!();

    if commits.is_empty() {
        println!("{}", "No commits found in this range.".dimmed());
        return;
    }

    // Calculate column width for PR number alignment
    let max_pr_width = commits
        .iter()
        .filter_map(|c| c.pr_number)
        .map(|n| format!("#{}", n).len())
        .max()
        .unwrap_or(1)
        .max(1);

    for commit in commits {
        let hash = &commit.short_hash;
        let pr_str = commit
            .pr_number
            .map(|n| format!("#{}", n))
            .unwrap_or_else(|| "     ".to_string()); // Empty space if no PR

        // Clean message (remove PR number suffix for cleaner display)
        let clean_message = remove_pr_suffix(&commit.message);

        // Format: hash  pr_number  message (author)
        println!(
            "  {} {:width$} {} {}",
            hash.bright_yellow(),
            if commit.pr_number.is_some() {
                pr_str.bright_magenta().to_string()
            } else {
                pr_str.dimmed().to_string()
            },
            clean_message,
            format!("(@{})", commit.author).cyan().dimmed(),
            width = max_pr_width,
        );
    }

    println!();
}

/// Remove the PR number suffix from a commit message for cleaner display
fn remove_pr_suffix(message: &str) -> String {
    let pr_regex = Regex::new(r"\s*\(#\d+\)\s*$").unwrap();
    pr_regex.replace(message, "").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_commits_basic() {
        // Uses NULL byte as delimiter (matches git log --format=%H%x00%s%x00%aN)
        // %aN gives author name from git config (user.name)
        let output =
            "abc1234567890\0feat: add feature\0John Doe\ndef9876543210\0fix: bug fix\0Jane Smith";
        let commits = parse_commits(output).unwrap();

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].short_hash, "abc1234");
        assert_eq!(commits[0].message, "feat: add feature");
        assert_eq!(commits[0].author, "John Doe");
        assert_eq!(commits[0].pr_number, None);
    }

    #[test]
    fn test_parse_commits_with_pr() {
        let output = "abc1234567890\0feat: add feature (#42)\0John Doe";
        let commits = parse_commits(output).unwrap();

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].pr_number, Some(42));
    }

    #[test]
    fn test_parse_commits_empty() {
        let output = "";
        let commits = parse_commits(output).unwrap();
        assert_eq!(commits.len(), 0);
    }

    #[test]
    fn test_parse_commits_with_special_chars_in_message() {
        // NULL byte delimiter allows pipes and other special chars in message
        let output = "abc1234567890\0feat: add foo|bar feature\0John Doe";
        let commits = parse_commits(output).unwrap();

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "feat: add foo|bar feature");
    }

    #[test]
    fn test_remove_pr_suffix() {
        assert_eq!(
            remove_pr_suffix("feat: add thing (#123)"),
            "feat: add thing"
        );
        assert_eq!(remove_pr_suffix("fix: bug (#42)  "), "fix: bug");
        assert_eq!(remove_pr_suffix("no pr here"), "no pr here");
        assert_eq!(
            remove_pr_suffix("mentions #42 but not at end"),
            "mentions #42 but not at end"
        );
    }

    #[test]
    fn test_commit_entry_serialization() {
        let entry = CommitEntry {
            hash: "abc1234567890".to_string(),
            short_hash: "abc1234".to_string(),
            message: "feat: test".to_string(),
            author: "Test Author".to_string(),
            pr_number: Some(42),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"pr_number\":42"));
        assert!(json.contains("\"short_hash\":\"abc1234\""));
    }

    #[test]
    fn test_changelog_json_serialization() {
        let output = ChangelogJson {
            from: "v1.0.0".to_string(),
            to: "HEAD".to_string(),
            path: Some("src/".to_string()),
            commit_count: 1,
            commits: vec![CommitEntry {
                hash: "abc123".to_string(),
                short_hash: "abc123".to_string(),
                message: "test".to_string(),
                author: "Test User".to_string(),
                pr_number: None,
            }],
        };

        let json = serde_json::to_string_pretty(&output).unwrap();
        assert!(json.contains("\"from\": \"v1.0.0\""));
        assert!(json.contains("\"path\": \"src/\""));
        assert!(json.contains("\"commit_count\": 1"));
    }
}
