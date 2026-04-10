use crate::engine::Stack;
use crate::git::GitRepo;
use crate::tui;
use anyhow::Result;
use colored::Colorize;
use std::io::IsTerminal;

pub fn run(hunk_mode: bool, no_verify: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;

    if current == stack.trunk {
        anyhow::bail!(
            "Cannot split trunk branch. Create a branch first with {}",
            "stax create".cyan()
        );
    }

    let branch_info = stack.branches.get(&current);
    if branch_info.is_none() {
        anyhow::bail!(
            "Branch '{}' is not tracked. Use {} to track it first.",
            current,
            "stax branch track".cyan()
        );
    }

    let parent = branch_info.and_then(|b| b.parent.as_ref());
    if parent.is_none() {
        anyhow::bail!("Branch '{}' has no parent to split from.", current);
    }

    if !hunk_mode {
        let parent_ref = parent.unwrap();
        let commits = repo.commits_between(parent_ref, &current)?;
        if commits.is_empty() {
            anyhow::bail!(
                "No commits to split. Branch '{}' has no commits above '{}'.",
                current,
                parent_ref
            );
        }

        if commits.len() == 1 {
            anyhow::bail!(
                "Only 1 commit on branch '{}'. Need at least 2 commits to split.\n\
                 Tip: Use {} to split by hunk instead.",
                current,
                "stax split --hunk".cyan()
            );
        }
    }

    if !std::io::stdin().is_terminal() {
        anyhow::bail!("Split requires an interactive terminal.");
    }

    if hunk_mode {
        drop(repo);
        return tui::split_hunk::run(no_verify);
    }

    tui::split::run()
}
