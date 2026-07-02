use super::shared::{compute_worktree_details, status_labels, worktree_to_json};
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run(json: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let details = repo
        .list_worktrees()?
        .into_iter()
        .map(|worktree| compute_worktree_details(&repo, worktree))
        .collect::<Result<Vec<_>>>()?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &details.iter().map(worktree_to_json).collect::<Vec<_>>()
            )?
        );
        return Ok(());
    }

    if details.is_empty() {
        println!("{}", "No worktrees found.".dimmed());
        return Ok(());
    }

    let name_width = details
        .iter()
        .map(|d| d.info.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let branch_width = details
        .iter()
        .map(|d| d.branch_label.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let status_strings: Vec<String> = details.iter().map(status_summary).collect();
    let status_width = status_strings
        .iter()
        .map(|status| status.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let base_width = details
        .iter()
        .map(|d| d.stack_parent.as_deref().unwrap_or("—").len())
        .max()
        .unwrap_or(4)
        .max(4);

    println!(
        "  {:<width_n$}  {:<width_b$}  {:<width_s$}  {:<width_base$}  {}",
        "NAME".bold(),
        "BRANCH".bold(),
        "STATUS".bold(),
        "BASE".bold(),
        "PATH".bold(),
        width_n = name_width,
        width_b = branch_width,
        width_s = status_width,
        width_base = base_width,
    );
    println!(
        "  {}",
        "─"
            .repeat(name_width + branch_width + status_width + base_width + 60)
            .dimmed()
    );

    for (detail, status) in details.iter().zip(status_strings.iter()) {
        let marker = if detail.info.is_current { "*" } else { " " };
        let name = format!("{:<width$}", detail.info.name, width = name_width);
        let branch = format!("{:<width$}", detail.branch_label, width = branch_width);
        let status = format!("{:<width$}", status, width = status_width);
        let base = format!(
            "{:<width$}",
            detail.stack_parent.as_deref().unwrap_or("—"),
            width = base_width
        );

        println!(
            "{}  {}  {}  {}  {}  {}",
            marker.yellow(),
            if detail.info.is_current {
                name.cyan().bold().to_string()
            } else {
                name.cyan().to_string()
            },
            branch.green(),
            color_status(detail, &status),
            if detail.is_managed {
                base.blue().to_string()
            } else {
                base.dimmed().to_string()
            },
            detail.info.path.display().to_string().dimmed(),
        );
    }

    Ok(())
}

fn status_summary(details: &super::shared::WorktreeDetails) -> String {
    status_labels(details).join(",")
}

fn color_status(details: &super::shared::WorktreeDetails, status: &str) -> String {
    if details.has_conflicts || details.merge_in_progress || details.rebase_in_progress {
        status.red().bold().to_string()
    } else if details.dirty || details.info.is_prunable {
        status.yellow().to_string()
    } else if details.is_managed {
        status.blue().to_string()
    } else {
        status.dimmed().to_string()
    }
}
