use super::shared::{compute_worktree_details, worktree_to_json};
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run(json: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let worktrees = repo.list_worktrees()?;

    if json {
        let details = worktrees
            .into_iter()
            .map(|worktree| compute_worktree_details(&repo, worktree))
            .collect::<Result<Vec<_>>>()?;
        println!(
            "{}",
            serde_json::to_string_pretty(
                &details.iter().map(worktree_to_json).collect::<Vec<_>>()
            )?
        );
        return Ok(());
    }

    if worktrees.is_empty() {
        println!("{}", "No worktrees found.".dimmed());
        return Ok(());
    }

    let name_width = worktrees
        .iter()
        .map(|w| w.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let branch_width = worktrees
        .iter()
        .map(|w| w.branch.as_deref().unwrap_or("(detached)").len())
        .max()
        .unwrap_or(6)
        .max(6);

    println!(
        "  {:<width_n$}  {:<width_b$}  {}",
        "NAME".bold(),
        "BRANCH".bold(),
        "PATH".bold(),
        width_n = name_width,
        width_b = branch_width,
    );
    println!("  {}", "─".repeat(name_width + branch_width + 50).dimmed());

    for worktree in &worktrees {
        let marker = if worktree.is_current { "*" } else { " " };
        let branch_str = worktree.branch.as_deref().unwrap_or("(detached)");
        let name_padded = format!("{:<width$}", worktree.name, width = name_width);
        let branch_padded = format!("{:<width$}", branch_str, width = branch_width);

        let name_col = if worktree.is_current {
            name_padded.cyan().bold().to_string()
        } else {
            name_padded.cyan().to_string()
        };
        let branch_col = if worktree.is_current {
            branch_padded.green().bold().to_string()
        } else {
            branch_padded.green().to_string()
        };

        println!(
            "{}  {}  {}  {}",
            marker.yellow(),
            name_col,
            branch_col,
            worktree.path.display().to_string().dimmed(),
        );
    }

    Ok(())
}
