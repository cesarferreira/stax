use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use regex::Regex;
use serde::Serialize;
use std::env;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

const CHANGELOG_FIND_LIMIT: usize = 25;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_from: Option<String>,
    path: Option<String>,
    commit_count: usize,
    commits: Vec<CommitEntry>,
}

#[derive(Debug, Clone)]
struct ScoredCommitEntry {
    entry: CommitEntry,
    score: i64,
}

/// JSON output structure for changelog commit find mode.
#[derive(Serialize)]
struct ChangelogFindJson {
    query: String,
    from: String,
    to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_from: Option<String>,
    path: Option<String>,
    match_count: usize,
    matches: Vec<CommitEntry>,
}

/// Pick the first tag from `git tag --sort=-creatordate` output that matches
/// the optional prefix. Pure function over the raw stdout string so it can be
/// unit-tested without a real repo.
fn pick_latest_tag(git_tag_output: &str, prefix: Option<&str>) -> Result<String> {
    let tag = git_tag_output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .find(|l| match prefix {
            Some(p) => l.starts_with(p),
            None => true,
        });

    match tag {
        Some(t) => Ok(t.to_string()),
        None => match prefix {
            Some(p) => anyhow::bail!("No tags found matching prefix '{}'", p),
            None => anyhow::bail!("No tags found in this repository"),
        },
    }
}

/// Resolve the latest tag in the repo, optionally filtered by prefix.
fn resolve_latest_tag(workdir: &Path, prefix: Option<&str>) -> Result<String> {
    let output = Command::new("git")
        .args(["tag", "--sort=-creatordate"])
        .current_dir(workdir)
        .output()
        .context("Failed to run git tag")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git tag failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    pick_latest_tag(&stdout, prefix)
}

pub fn run(
    from: Option<String>,
    to: Option<String>,
    find: Option<Option<String>>,
    tag_prefix: Option<String>,
    path: Option<String>,
    json: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?;
    let (from, to, find) = normalize_find_args(from, to, find)?;

    if let Some(query) = find {
        return run_find(workdir, from, to, tag_prefix, path, query, json);
    }

    let (from, to, auto_resolved) =
        resolve_changelog_range(workdir, from, to, tag_prefix.as_deref())?;
    let resolved_path = resolve_path_filter(workdir, path.as_ref())?;
    let commits = load_commits(workdir, &from, &to, &resolved_path)?;

    if json {
        let output = ChangelogJson {
            from: from.clone(),
            to: to.clone(),
            resolved_from: if auto_resolved {
                Some(from.clone())
            } else {
                None
            },
            path: resolved_path.clone(),
            commit_count: commits.len(),
            commits,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    print_changelog(&from, &to, &resolved_path, &commits);

    Ok(())
}

fn resolve_changelog_range(
    workdir: &Path,
    from: Option<String>,
    to: Option<String>,
    tag_prefix: Option<&str>,
) -> Result<(String, String, bool)> {
    let to = to.unwrap_or_else(|| "HEAD".to_string());
    let (from, auto_resolved) = match from {
        Some(f) => (f, false),
        None => {
            let tag = resolve_latest_tag(workdir, tag_prefix)?;
            (tag, true)
        }
    };

    Ok((from, to, auto_resolved))
}

fn resolve_path_filter(workdir: &Path, path: Option<&String>) -> Result<Option<String>> {
    // Resolve path filter relative to current directory and make it relative to repo root
    let resolved_path = if let Some(p) = path {
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

    Ok(resolved_path)
}

fn load_commits(
    workdir: &Path,
    from: &str,
    to: &str,
    resolved_path: &Option<String>,
) -> Result<Vec<CommitEntry>> {
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
    if let Some(p) = &resolved_path {
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
    parse_commits(&stdout)
}

fn normalize_find_args(
    from: Option<String>,
    to: Option<String>,
    find: Option<Option<String>>,
) -> Result<(Option<String>, Option<String>, Option<Option<String>>)> {
    if find.is_some() {
        return Ok((from, to, find));
    }

    let Some(from_value) = from else {
        return Ok((None, to, None));
    };

    match from_value.as_str() {
        "find" | "search" => Ok((None, None, Some(to))),
        "--find" | "--search" => Ok((None, None, Some(to))),
        _ => {
            let separator_query = from_value
                .strip_prefix("--find=")
                .or_else(|| from_value.strip_prefix("--search="));

            if let Some(query) = separator_query {
                if to.is_some() {
                    anyhow::bail!("`stax changelog --find <query>` accepts only one search query");
                }

                Ok((None, None, Some(Some(query.to_string()))))
            } else {
                Ok((Some(from_value), to, None))
            }
        }
    }
}

fn run_find(
    workdir: &Path,
    from: Option<String>,
    to: Option<String>,
    tag_prefix: Option<String>,
    path: Option<String>,
    query: Option<String>,
    json: bool,
) -> Result<()> {
    let (from, to, auto_resolved) =
        resolve_changelog_range(workdir, from, to, tag_prefix.as_deref())?;
    let resolved_path = resolve_path_filter(workdir, path.as_ref())?;
    let commits = load_commits(workdir, &from, &to, &resolved_path)?;

    match query {
        Some(query) => run_find_query(
            &commits,
            &query,
            &from,
            &to,
            auto_resolved,
            &resolved_path,
            json,
        ),
        None => {
            if json {
                anyhow::bail!(
                    "Use `stax changelog find <query> --json` or `stax changelog --find <query> --json` for JSON output"
                );
            }
            run_find_picker(&commits, &from, &to, &resolved_path)
        }
    }
}

fn run_find_query(
    commits: &[CommitEntry],
    query: &str,
    from: &str,
    to: &str,
    auto_resolved: bool,
    path: &Option<String>,
    json: bool,
) -> Result<()> {
    let query = query.trim();
    if query.is_empty() {
        anyhow::bail!("`stax changelog --find <query>` requires a non-empty query");
    }

    let matches = find_commit_entries(commits, query);
    let displayed_matches: Vec<CommitEntry> = matches
        .iter()
        .take(CHANGELOG_FIND_LIMIT)
        .map(|m| m.entry.clone())
        .collect();

    if json {
        let output = ChangelogFindJson {
            query: query.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            resolved_from: if auto_resolved {
                Some(from.to_string())
            } else {
                None
            },
            path: path.clone(),
            match_count: displayed_matches.len(),
            matches: displayed_matches,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    print_changelog_find_matches(query, from, to, path, &matches);
    Ok(())
}

fn run_find_picker(
    commits: &[CommitEntry],
    from: &str,
    to: &str,
    path: &Option<String>,
) -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        anyhow::bail!(
            "`stax changelog find` requires an interactive terminal. Use `stax changelog find <query>` in scripts."
        );
    }

    if commits.is_empty() {
        anyhow::bail!("No commits found in this range.");
    }

    let pr_width = commits
        .iter()
        .filter_map(|entry| entry.pr_number)
        .map(|n| format!("#{}", n).len())
        .max()
        .unwrap_or(1)
        .max(1);

    let items: Vec<String> = commits
        .iter()
        .map(|entry| format_commit_find_picker_item(entry, pr_width))
        .collect();

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "Search changelog commits ({} → {}){}",
            from,
            to,
            path.as_ref()
                .map(|p| format!(", filtered to {p}"))
                .unwrap_or_default()
        ))
        .items(&items)
        .interact()?;

    print_changelog_find_selection(&commits[selection]);
    Ok(())
}

fn find_commit_entries(commits: &[CommitEntry], query: &str) -> Vec<ScoredCommitEntry> {
    let matcher = SkimMatcherV2::default();
    let mut matches: Vec<ScoredCommitEntry> = commits
        .iter()
        .filter_map(|entry| {
            let haystack = commit_find_haystack(entry);
            matcher
                .fuzzy_match(&haystack, query)
                .map(|score| ScoredCommitEntry {
                    entry: entry.clone(),
                    score,
                })
        })
        .collect();

    matches.sort_by(|a, b| b.score.cmp(&a.score));
    matches
}

fn commit_find_haystack(entry: &CommitEntry) -> String {
    format!(
        "{} {} {} {}",
        entry.short_hash,
        entry.pr_number.map(|n| format!("#{n}")).unwrap_or_default(),
        entry.message,
        entry.author
    )
}

fn print_changelog_find_matches(
    query: &str,
    from: &str,
    to: &str,
    path: &Option<String>,
    matches: &[ScoredCommitEntry],
) {
    let match_word = if matches.len() == 1 {
        "match"
    } else {
        "matches"
    };
    println!(
        "{}",
        format!(
            "Changelog commit search: \"{}\" ({} {})",
            query.cyan(),
            matches.len().min(CHANGELOG_FIND_LIMIT),
            match_word
        )
        .bright_white()
        .bold()
    );

    println!("{}", format!("Range: {} → {}", from, to).dimmed());
    if let Some(p) = path {
        println!("{}", format!("Filtered to: {}", p).dimmed());
    }
    println!("{}", "─".repeat(50).dimmed());
    println!();

    if matches.is_empty() {
        println!("{}", "No commits matched.".dimmed());
        return;
    }

    if matches.len() > CHANGELOG_FIND_LIMIT {
        println!(
            "{}",
            format!(
                "Showing top {} of {} matches.",
                CHANGELOG_FIND_LIMIT,
                matches.len()
            )
            .dimmed()
        );
    }

    let displayed: Vec<&CommitEntry> = matches
        .iter()
        .take(CHANGELOG_FIND_LIMIT)
        .map(|m| &m.entry)
        .collect();
    let pr_width = displayed
        .iter()
        .filter_map(|entry| entry.pr_number)
        .map(|n| format!("#{}", n).len())
        .max()
        .unwrap_or(1)
        .max(1);

    for entry in displayed {
        println!(
            "{} {}",
            "  •".bright_black(),
            format_commit_find_item(entry, pr_width)
        );
    }
}

fn print_changelog_find_selection(entry: &CommitEntry) {
    println!("{}", entry.short_hash.bright_yellow().bold());
    println!("{}", entry.message.bright_white());
    println!("{}", format!("Author: {}", entry.author).cyan().dimmed());
    if let Some(pr_number) = entry.pr_number {
        println!("{}", format!("PR: #{}", pr_number).bright_magenta());
    }
}

fn format_commit_find_item(entry: &CommitEntry, pr_width: usize) -> String {
    let pr_col = if let Some(n) = entry.pr_number {
        let raw = format!("{:<width$}", format!("#{}", n), width = pr_width);
        raw.bright_magenta().to_string()
    } else {
        " ".repeat(pr_width).dimmed().to_string()
    };
    let clean_message = remove_pr_suffix(&entry.message);

    format!(
        "{} {} {} {}",
        entry.short_hash.bright_yellow(),
        pr_col,
        clean_message.bright_white(),
        format!("(@{})", entry.author).cyan().dimmed()
    )
}

fn format_commit_find_picker_item(entry: &CommitEntry, pr_width: usize) -> String {
    let pr_col = entry
        .pr_number
        .map(|n| format!("#{}", n))
        .unwrap_or_default();
    let clean_message = remove_pr_suffix(&entry.message);

    format!(
        "{} {:<pr_width$} {} (@{})",
        entry.short_hash, pr_col, clean_message, entry.author
    )
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

        let pr_col = if let Some(n) = commit.pr_number {
            let raw = format!("{:<width$}", format!("#{}", n), width = max_pr_width);
            raw.bright_magenta().to_string()
        } else {
            let raw = " ".repeat(max_pr_width);
            raw.dimmed().to_string()
        };

        let clean_message = remove_pr_suffix(&commit.message);

        println!(
            "    {} {} {} {}",
            hash.bright_yellow(),
            pr_col,
            clean_message,
            format!("(@{})", commit.author).cyan().dimmed(),
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
            resolved_from: None,
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
        assert!(!json.contains("resolved_from"));
    }

    #[test]
    fn test_changelog_json_with_resolved_from() {
        let output = ChangelogJson {
            from: "v2.0.0".to_string(),
            to: "HEAD".to_string(),
            resolved_from: Some("v2.0.0".to_string()),
            path: None,
            commit_count: 0,
            commits: vec![],
        };

        let json = serde_json::to_string_pretty(&output).unwrap();
        assert!(json.contains("\"resolved_from\": \"v2.0.0\""));
    }

    #[test]
    fn test_pick_latest_tag_returns_first() {
        let output = "v3.0.0\nv2.0.0\nv1.0.0\n";
        assert_eq!(pick_latest_tag(output, None).unwrap(), "v3.0.0");
    }

    #[test]
    fn test_pick_latest_tag_with_prefix() {
        let output = "release/android/v2.0.0\nrelease/ios/v1.1.0\nrelease/ios/v1.0.0\nrelease/android/v1.0.0\n";
        assert_eq!(
            pick_latest_tag(output, Some("release/ios")).unwrap(),
            "release/ios/v1.1.0"
        );
    }

    #[test]
    fn test_pick_latest_tag_prefix_no_match() {
        let output = "release/android/v1.0.0\nv1.0.0\n";
        let err = pick_latest_tag(output, Some("release/ios")).unwrap_err();
        assert!(err.to_string().contains("release/ios"));
    }

    #[test]
    fn test_pick_latest_tag_empty_output() {
        let err = pick_latest_tag("", None).unwrap_err();
        assert!(err.to_string().contains("No tags found"));
    }

    #[test]
    fn test_pick_latest_tag_whitespace_only() {
        let err = pick_latest_tag("  \n  \n", None).unwrap_err();
        assert!(err.to_string().contains("No tags found"));
    }
}
