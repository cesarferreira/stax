use crate::engine::Stack;
use crate::git::GitRepo;
use crate::tui;
use anyhow::Result;
use colored::Colorize;
use std::io::IsTerminal;

/// Split the current branch into multiple stacked branches
pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;

    // Validate: not on trunk
    if current == stack.trunk {
        anyhow::bail!(
            "Cannot split trunk branch. Create a branch first with {}",
            "stax create".cyan()
        );
    }

    // Validate: branch is tracked
    let branch_info = stack.branches.get(&current);
    if branch_info.is_none() {
        anyhow::bail!(
            "Branch '{}' is not tracked. Use {} to track it first.",
            current,
            "stax branch track".cyan()
        );
    }

    // Validate: has parent
    let parent = branch_info.and_then(|b| b.parent.as_ref());
    if parent.is_none() {
        anyhow::bail!("Branch '{}' has no parent to split from.", current);
    }

    // Validate: has commits
    let parent = parent.unwrap();
    let commits = repo.commits_between(parent, &current)?;
    if commits.is_empty() {
        anyhow::bail!(
            "No commits to split. Branch '{}' has no commits above '{}'.",
            current,
            parent
        );
    }

    if commits.len() == 1 {
        anyhow::bail!(
            "Only 1 commit on branch '{}'. Need at least 2 commits to split.",
            current
        );
    }

    // Validate: interactive terminal required for TUI
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("Split requires an interactive terminal.");
    }

    // Launch split TUI
    tui::split::run()
}
