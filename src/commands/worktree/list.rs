use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let worktrees = repo.list_worktrees()?;

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

    for wt in &worktrees {
        let marker = if wt.is_current { "*" } else { " " };
        let branch_str = wt.branch.as_deref().unwrap_or("(detached)");

        // Print columns with fixed logical widths (pad manually to avoid ANSI offset issues)
        let name_padded = format!("{:<width$}", wt.name, width = name_width);
        let branch_padded = format!("{:<width$}", branch_str, width = branch_width);

        let name_col = if wt.is_current {
            name_padded.cyan().bold().to_string()
        } else {
            name_padded.cyan().to_string()
        };

        let branch_col = if wt.is_current {
            branch_padded.green().bold().to_string()
        } else {
            branch_padded.green().to_string()
        };

        println!(
            "{}  {}  {}  {}",
            marker.yellow(),
            name_col,
            branch_col,
            wt.path.display().to_string().dimmed(),
        );
    }

    Ok(())
}
