use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    if stack.branches.len() <= 1 {
        println!("{}", "No tracked branches in stack.".dimmed());
        println!(
            "Use {} to start tracking branches.",
            "stax branch track".cyan()
        );
        return Ok(());
    }

    println!();

    // Render tree starting from trunk (bottom-up: trunk at bottom)
    render_branch_tree(&repo, &stack, &stack.trunk, &current, &mut Vec::new());

    println!();

    Ok(())
}

fn render_branch_tree(
    repo: &GitRepo,
    stack: &Stack,
    branch: &str,
    current: &str,
    pipes: &mut Vec<bool>, // true = draw pipe at this level, false = space
) {
    let branch_info = stack.branches.get(branch);
    let is_current = branch == current;
    let is_trunk = branch == &stack.trunk;

    // Get children
    let children: Vec<String> = branch_info
        .map(|b| b.children.clone())
        .unwrap_or_default();

    // Render children first (so leaves are at top)
    // Process in reverse so first child ends up at bottom (closest to parent)
    for (i, child) in children.iter().rev().enumerate() {
        let is_last_child = i == children.len() - 1;
        // Add pipe for this level - true if there are more siblings after this one
        pipes.push(!is_last_child);
        render_branch_tree(repo, stack, child, current, pipes);
        pipes.pop();
    }

    // Build prefix from pipes
    let prefix: String = pipes.iter().map(|&has_pipe| if has_pipe { "|   " } else { "    " }).collect();

    // Branch indicator
    let indicator = if is_current { "*" } else { "o" };
    let indicator_colored = if is_current {
        indicator.bright_green().bold()
    } else if is_trunk {
        indicator.bright_blue()
    } else {
        indicator.bright_cyan()
    };

    // Branch name with bright colors
    let name_colored = if is_current {
        branch.bright_green().bold()
    } else if is_trunk {
        branch.bright_blue().bold()
    } else {
        branch.bright_cyan()
    };

    // Build status badges
    let mut badges = String::new();

    if is_current {
        badges.push_str(&" <".bright_green().to_string());
    }

    if let Some(info) = branch_info {
        if info.needs_restack {
            badges.push_str(&" [needs restack]".bright_yellow().to_string());
        }
        if let Some(pr_num) = info.pr_number {
            badges.push_str(&format!(" PR #{}", pr_num).bright_magenta().to_string());
        }
    }

    println!(
        "{}{} {}{}",
        prefix.bright_black(),
        indicator_colored,
        name_colored,
        badges
    );

    // Details prefix (continues the tree line)
    let details_prefix: String = pipes.iter().map(|&has_pipe| if has_pipe { "|   " } else { "    " }).collect();
    let details_prefix = format!("{}|   ", details_prefix);

    // Age
    if let Ok(age) = repo.branch_age(branch) {
        println!("{}{}", details_prefix.bright_black(), age.dimmed());
    }

    // Commits unique to this branch
    let parent = branch_info.and_then(|b| b.parent.as_deref());
    if let Ok(commits) = repo.branch_commits(branch, parent) {
        if !commits.is_empty() {
            for commit in commits {
                println!(
                    "{}{} {}",
                    details_prefix.bright_black(),
                    commit.short_hash.bright_yellow(),
                    commit.message.white()
                );
            }
        }
    }

    // Spacing line (only if not trunk)
    if !pipes.is_empty() {
        let spacer: String = pipes.iter().map(|&has_pipe| if has_pipe { "|   " } else { "    " }).collect();
        println!("{}{}", spacer.bright_black(), "|".bright_black());
    }
}
