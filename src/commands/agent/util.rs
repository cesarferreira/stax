use crate::config::Config;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Convert a human title into a URL/filesystem-safe slug.
///
/// "Add dark mode system" → "add-dark-mode-system"
pub fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Ensure the worktrees dir is listed in the repo's .gitignore.
pub fn ensure_gitignore(repo_root: &Path, worktrees_dir: &str) -> Result<()> {
    let gitignore = repo_root.join(".gitignore");
    let entry = format!("{}/", worktrees_dir.trim_end_matches('/'));

    if gitignore.exists() {
        let content = fs::read_to_string(&gitignore)?;
        if content
            .lines()
            .any(|l| l.trim() == entry.trim_end_matches('/') || l.trim() == entry)
        {
            return Ok(());
        }
        let updated = if content.ends_with('\n') {
            format!("{}{}\n", content, entry)
        } else {
            format!("{}\n{}\n", content, entry)
        };
        fs::write(&gitignore, updated)?;
    } else {
        fs::write(&gitignore, format!("{}\n", entry))?;
    }

    Ok(())
}

/// Decide which editor command to use.
///
/// Priority: explicit flag > config > auto-detect.
pub fn resolve_editor(
    config: &Config,
    open_cursor: bool,
    open_codex: bool,
    open_any: bool,
) -> Option<String> {
    if open_cursor {
        return Some("cursor".to_string());
    }
    if open_codex {
        return Some("codex".to_string());
    }
    if !open_any {
        return None;
    }

    // Check config
    match config.agent.default_editor.as_str() {
        "cursor" => return Some("cursor".to_string()),
        "codex" => return Some("codex".to_string()),
        "code" => return Some("code".to_string()),
        _ => {}
    }

    // Auto-detect: prefer cursor, then code
    for candidate in &["cursor", "code"] {
        if which_exists(candidate) {
            return Some(candidate.to_string());
        }
    }

    // Nothing found — just print path
    None
}

/// Open a path in the given editor.
pub fn open_in_editor(editor: &str, path: &Path) -> Result<()> {
    let path_str = path.to_str().context("Non-UTF-8 path")?;

    let args: Vec<&str> = match editor {
        "cursor" => vec!["-n", path_str],
        "code" => vec!["-n", path_str],
        _ => vec![path_str],
    };

    Command::new(editor).args(&args).spawn().with_context(|| {
        format!(
            "Failed to launch '{}'. Is it installed and on PATH?",
            editor
        )
    })?;

    Ok(())
}

fn which_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Add dark mode system"), "add-dark-mode-system");
        assert_eq!(slugify("Fix: user auth bug!"), "fix-user-auth-bug");
        assert_eq!(slugify("  spaces  "), "spaces");
        assert_eq!(slugify("already-a-slug"), "already-a-slug");
    }
}
