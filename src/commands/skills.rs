use anyhow::{Context, Result};
use colored::Colorize;
use std::path::PathBuf;

const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const REMOTE_URL: &str =
    "https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md";

/// Known agent skill file locations (relative to `$HOME`).
struct SkillLocation {
    /// Display name shown in output.
    name: &'static str,
    /// Path relative to the user's home directory.
    relative_path: &'static str,
    /// Whether this file uses YAML frontmatter (SKILL.md format).
    has_frontmatter: bool,
}

const SKILL_LOCATIONS: &[SkillLocation] = &[
    SkillLocation {
        name: "Codex",
        relative_path: ".codex/skills/stax/SKILL.md",
        has_frontmatter: true,
    },
    SkillLocation {
        name: "OpenCode",
        relative_path: ".config/opencode/skills/stax/SKILL.md",
        has_frontmatter: true,
    },
    SkillLocation {
        name: "Claude Code (global)",
        relative_path: ".claude/skills/stax/SKILL.md",
        has_frontmatter: true,
    },
    SkillLocation {
        name: "Cursor",
        relative_path: ".cursor/skills/stax/SKILL.md",
        has_frontmatter: true,
    },
];

/// Parse `<!-- stax-skills-version: X.Y.Z -->` or `stax_version: "X.Y.Z"` from the
/// first 40 lines of a skill file's content.
pub fn extract_skills_version(content: &str) -> Option<String> {
    for line in content.lines().take(40) {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("<!-- stax-skills-version:") {
            let v = rest.trim_end_matches("-->").trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }

        if let Some(rest) = trimmed.strip_prefix("stax_version:") {
            let v = rest
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Build the full path for a skill location from `$HOME`.
fn skill_path(loc: &SkillLocation) -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(loc.relative_path))
}

/// Generate the content to write for a given location given the remote body.
///
/// For `has_frontmatter = true` files we prepend a minimal YAML front-matter so
/// agent skill runners can load them.  For plain markdown files we write the body
/// as-is (it already contains the `<!-- stax-skills-version: … -->` marker).
fn build_content(body: &str, loc: &SkillLocation) -> String {
    if loc.has_frontmatter {
        format!(
            "---\nname: stax\ndescription: Use stax to manage stacked Git branches and PRs. Covers all commands, flags, workflows, and best practices for AI coding agents.\nstax_version: \"{PKG_VERSION}\"\nmetadata:\n  short-description: Stax stacked-branch and PR management commands\n---\n\n{body}",
        )
    } else {
        body.to_string()
    }
}

/// Download the latest `skills.md` from GitHub and return its content.
fn fetch_remote_skills() -> Result<String> {
    let runtime = tokio::runtime::Runtime::new()?;
    let body = runtime.block_on(async {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(10))
            .build()?
            .get(REMOTE_URL)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
    })
    .context("Failed to download skills from GitHub")?;
    Ok(body)
}

pub fn run_list() -> Result<()> {
    println!("{}", "stax skills".bold());
    println!();

    let mut any_found = false;

    for loc in SKILL_LOCATIONS {
        let Some(path) = skill_path(loc) else {
            continue;
        };

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                any_found = true;
                let installed = extract_skills_version(&content);
                let label = loc.name.cyan();
                let path_str = path.display().to_string().dimmed();

                match &installed {
                    Some(v) if v == PKG_VERSION => {
                        println!(
                            "{}  {} {}  {}",
                            "✓".green(),
                            label,
                            format!("(v{v})").dimmed(),
                            path_str,
                        );
                    }
                    Some(v) => {
                        println!(
                            "{}  {} {}  {}",
                            "⚠".yellow(),
                            label,
                            format!("(v{v} → v{PKG_VERSION} available)").yellow(),
                            path_str,
                        );
                    }
                    None => {
                        println!(
                            "{}  {} {}  {}",
                            "⚠".yellow(),
                            label,
                            "(no version marker — may be out of date)".yellow(),
                            path_str,
                        );
                    }
                }
            }
            Err(_) => {
                // File doesn't exist — show it as "not installed".
                let path_str = path.display().to_string().dimmed();
                println!(
                    "{}  {}  {}",
                    "–".dimmed(),
                    loc.name.dimmed(),
                    path_str,
                );
            }
        }
    }

    println!();
    if !any_found {
        println!(
            "{}",
            "No skill files found. Run `stax skills update` to install them.".yellow()
        );
    } else {
        println!("Run {} to bring all skill files up to date.", "`stax skills update`".cyan());
    }

    Ok(())
}

pub fn run_update(dry_run: bool) -> Result<()> {
    if dry_run {
        println!("{}", "stax skills update --dry-run".bold());
    } else {
        println!("{}", "stax skills update".bold());
    }
    println!();

    println!("Fetching latest skills from GitHub…");
    let remote_body = fetch_remote_skills()?;

    let remote_version = extract_skills_version(&remote_body)
        .unwrap_or_else(|| PKG_VERSION.to_string());

    println!(
        "Remote version: {}",
        format!("v{remote_version}").green()
    );
    println!();

    let mut updated = 0usize;
    let mut skipped = 0usize;

    for loc in SKILL_LOCATIONS {
        let Some(path) = skill_path(loc) else {
            continue;
        };

        let installed_version = std::fs::read_to_string(&path)
            .ok()
            .and_then(|c| extract_skills_version(&c));

        let needs_update = installed_version
            .as_deref()
            .map(|v| v != remote_version)
            .unwrap_or(true);

        let file_exists = path.exists();

        if !needs_update && file_exists {
            println!(
                "{}  {} {}",
                "✓".green(),
                loc.name.cyan(),
                "already up to date".dimmed(),
            );
            skipped += 1;
            continue;
        }

        let action = if file_exists { "update" } else { "install" };
        let content = build_content(&remote_body, loc);

        if dry_run {
            println!(
                "{}  {} {}",
                "→".cyan(),
                loc.name.cyan(),
                format!("would {action}: {}", path.display()).dimmed(),
            );
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create directory {}", parent.display())
                })?;
            }
            std::fs::write(&path, &content)
                .with_context(|| format!("Failed to write {}", path.display()))?;

            println!(
                "{}  {} {}",
                "✓".green(),
                loc.name.cyan(),
                format!("{action}d: {}", path.display()).dimmed(),
            );
            updated += 1;
        }
    }

    println!();
    if dry_run {
        println!("{}", "Dry run complete — no files were written.".dimmed());
    } else if updated == 0 {
        println!("{}", "All skill files are already up to date.".green());
    } else {
        println!(
            "{}",
            format!(
                "Updated {} skill file(s){}.",
                updated,
                if skipped > 0 {
                    format!(", {skipped} already current")
                } else {
                    String::new()
                }
            )
            .green()
        );
    }

    Ok(())
}

/// Check installed skill files and return a list of (name, installed_version) pairs
/// that are out of date relative to PKG_VERSION.  Used by `stax doctor`.
pub fn stale_skill_files() -> Vec<(String, Option<String>)> {
    let mut stale = Vec::new();

    for loc in SKILL_LOCATIONS {
        let Some(path) = skill_path(loc) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };

        let installed = extract_skills_version(&content);
        let is_current = installed.as_deref() == Some(PKG_VERSION);

        if !is_current {
            stale.push((loc.name.to_string(), installed));
        }
    }

    stale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_html_comment_version() {
        let content = "<!-- stax-skills-version: 1.2.3 -->\n# Stax Skills\n";
        assert_eq!(
            extract_skills_version(content),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn test_extract_yaml_frontmatter_version() {
        let content = "---\nname: stax\nstax_version: \"0.50.2\"\n---\n# Stax\n";
        assert_eq!(
            extract_skills_version(content),
            Some("0.50.2".to_string())
        );
    }

    #[test]
    fn test_extract_yaml_single_quotes() {
        let content = "---\nstax_version: '1.0.0'\n---\n";
        assert_eq!(
            extract_skills_version(content),
            Some("1.0.0".to_string())
        );
    }

    #[test]
    fn test_extract_missing_returns_none() {
        let content = "# Stax Skills\nNo version here.\n";
        assert_eq!(extract_skills_version(content), None);
    }

    #[test]
    fn test_build_content_with_frontmatter() {
        let loc = &SKILL_LOCATIONS[0]; // Codex — has_frontmatter = true
        let body = "<!-- stax-skills-version: 0.50.2 -->\n# Skills\n";
        let content = build_content(body, loc);
        assert!(content.starts_with("---\n"));
        assert!(content.contains("stax_version:"));
        assert!(content.contains("# Skills"));
    }

    #[test]
    fn test_stale_files_skips_missing() {
        // Should not panic when no files are installed.
        let _ = stale_skill_files();
    }
}
