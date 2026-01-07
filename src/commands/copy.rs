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

/// Copy branch name or PR URL to clipboard
pub fn run(target: CopyTarget) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;

    let text = match target {
        CopyTarget::Branch => current.clone(),
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

            // Check if branch has a PR
            let pr_number = branch_info.and_then(|b| b.pr_number);
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

    // Copy to clipboard
    let mut clipboard = Clipboard::new().map_err(|e| anyhow::anyhow!("Failed to access clipboard: {}", e))?;
    clipboard
        .set_text(&text)
        .map_err(|e| anyhow::anyhow!("Failed to copy to clipboard: {}", e))?;

    let label = match target {
        CopyTarget::Branch => "Branch name",
        CopyTarget::Pr => "PR URL",
    };

    println!("{} copied to clipboard: {}", label, text.cyan());

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
}
