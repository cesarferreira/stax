use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;
use arboard::Clipboard;
use colored::Colorize;

/// What to copy to clipboard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CopyTarget {
    /// Copy branch name (default)
    #[default]
    Branch,
    /// Copy PR URL
    Pr,
}

impl CopyTarget {
    /// Human-readable label for the resolved value.
    fn label(self) -> &'static str {
        match self {
            CopyTarget::Branch => "Branch name",
            CopyTarget::Pr => "PR URL",
        }
    }
}

/// Resolve the value to copy (branch name or PR URL) without touching the
/// clipboard. Kept separate from clipboard writing so that value resolution
/// fails (or succeeds) independently of clipboard availability.
fn resolve_value(target: CopyTarget) -> Result<String> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;

    let text = match target {
        CopyTarget::Branch => current,
        CopyTarget::Pr => {
            let stack = Stack::load(&repo)?;
            let config = Config::load()?;

            // Check if branch is tracked
            let branch_info = stack.branches.get(&current);
            if branch_info.is_none() {
                anyhow::bail!(
                    "Branch '{}' is not tracked. Use {} to track it first.",
                    current,
                    "stax branch track".cyan()
                );
            }

            // Resolve PR number (local metadata or forge fallback)
            let pr_number = super::resolve_pr::resolve_pr_number(&repo, &stack, &current, &config)?;
            if pr_number.is_none() {
                anyhow::bail!(
                    "No PR found for branch '{}'. Use {} to create one.",
                    current,
                    "stax submit".cyan()
                );
            }

            let pr_number = pr_number.unwrap();
            let remote_info = RemoteInfo::from_repo(&repo, &config)?;
            remote_info.pr_url(pr_number)
        }
    };

    Ok(text)
}

/// Attempt to write `text` to the system clipboard.
///
/// Returns an error describing why the clipboard could not be used (e.g. no
/// desktop clipboard in a headless/SSH/CI session).
fn write_to_clipboard(text: &str) -> Result<()> {
    if std::env::var_os("STAX_TEST_FORCE_CLIPBOARD_UNAVAILABLE").is_some() {
        anyhow::bail!("clipboard unavailable by test request");
    }

    let mut clipboard =
        Clipboard::new().map_err(|e| anyhow::anyhow!("Failed to access clipboard: {}", e))?;
    clipboard
        .set_text(text)
        .map_err(|e| anyhow::anyhow!("Failed to copy to clipboard: {}", e))?;
    Ok(())
}

/// Copy branch name or PR URL to clipboard.
///
/// In headless/SSH/CI environments where no desktop clipboard is available the
/// resolved value is still printed (with a warning on stderr) and the command
/// exits successfully, so callers always get the value instead of only a
/// clipboard backend error.
pub fn run(target: CopyTarget) -> Result<()> {
    // Resolve the value first; this is what the user actually wants and must
    // not be gated behind clipboard availability.
    let text = resolve_value(target)?;
    let label = target.label();

    match write_to_clipboard(&text) {
        Ok(()) => {
            println!("{} copied to clipboard: {}", label, text.cyan());
        }
        Err(e) => {
            // No clipboard available (headless/SSH/CI). Still give the user the
            // value and exit successfully instead of hard-failing.
            eprintln!(
                "{} Clipboard unavailable ({}). {} below:",
                "warning:".yellow().bold(),
                e,
                label
            );
            println!("{}", text);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_target_default() {
        let target = CopyTarget::default();
        assert_eq!(target, CopyTarget::Branch);
    }

    #[test]
    fn test_copy_target_variants() {
        assert_ne!(CopyTarget::Branch, CopyTarget::Pr);
    }

    #[test]
    fn test_copy_target_clone() {
        let target = CopyTarget::Pr;
        let cloned = target.clone();
        assert_eq!(target, cloned);
    }

    #[test]
    fn test_copy_target_debug() {
        let target = CopyTarget::Branch;
        let debug_str = format!("{:?}", target);
        assert_eq!(debug_str, "Branch");

        let target_pr = CopyTarget::Pr;
        let debug_str_pr = format!("{:?}", target_pr);
        assert_eq!(debug_str_pr, "Pr");
    }

    #[test]
    fn test_copy_target_copy_trait() {
        let target = CopyTarget::Branch;
        let copied = target; // Copy, not move
        assert_eq!(target, copied); // Original still accessible
    }

    #[test]
    fn test_copy_target_label() {
        assert_eq!(CopyTarget::Branch.label(), "Branch name");
        assert_eq!(CopyTarget::Pr.label(), "PR URL");
    }
}
