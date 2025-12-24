use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    // Get ALL branches in display order (leaves first, trunk last)
    let stack_branches = stack.all_branches_display_order();

    if stack_branches.len() <= 1 {
        println!("{}", "No tracked branches in stack.".dimmed());
        println!(
            "Use {} to start tracking branches.",
            "stax branch track".cyan()
        );
        return Ok(());
    }

    let total = stack_branches.len();
    for (i, branch_name) in stack_branches.iter().enumerate() {
        let branch_info = stack.branches.get(branch_name);
        let is_current = branch_name == &current;
        let is_trunk = branch_name == &stack.trunk;
        let is_last = i == total - 1;

        // Branch indicator
        let indicator = if is_current { "◉" } else { "○" };
        let indicator_colored = if is_current {
            indicator.green()
        } else {
            indicator.cyan()
        };

        // Branch name with status
        let name_colored = if is_current {
            branch_name.green().bold()
        } else if is_trunk {
            branch_name.blue()
        } else {
            branch_name.cyan()
        };

        // Build first line: indicator + branch name + status
        let mut first_line = format!("{} {}", indicator_colored, name_colored);

        if is_current {
            first_line.push_str(&" (current)".dimmed().to_string());
        }

        if let Some(info) = branch_info {
            if info.needs_restack {
                first_line.push_str(&" (needs restack)".yellow().to_string());
            }
        }

        println!("{}", first_line);

        // Second line: age
        if let Ok(age) = repo.branch_age(branch_name) {
            let prefix = if is_last { " " } else { "│" };
            println!("{} {}", prefix.dimmed(), age.dimmed());
        }

        // PR info (if available)
        if let Some(info) = branch_info {
            if let Some(pr_num) = info.pr_number {
                let prefix = if is_last { " " } else { "│" };
                // TODO: fetch actual PR title from GitHub
                let pr_line = format!(
                    "PR #{} {}",
                    pr_num.to_string().magenta(),
                    format!("https://github.com/.../pull/{}", pr_num).dimmed()
                );
                println!("{}", prefix.dimmed());
                println!("{} {}", prefix.dimmed(), pr_line);
            }
        }

        // Commits unique to this branch
        let parent = branch_info.and_then(|b| b.parent.as_deref());
        if let Ok(commits) = repo.branch_commits(branch_name, parent) {
            if !commits.is_empty() {
                let prefix = if is_last { " " } else { "│" };
                println!("{}", prefix.dimmed());
                for commit in commits {
                    println!(
                        "{} {} - {}",
                        prefix.dimmed(),
                        commit.short_hash.yellow(),
                        commit.message.dimmed()
                    );
                }
            }
        }

        // Spacing between branches
        if !is_last {
            println!("{}", "│".dimmed());
        }
    }

    println!();

    Ok(())
}
