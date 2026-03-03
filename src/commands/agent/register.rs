use super::registry::{AgentWorktree, Registry};
use crate::git::GitRepo;
use anyhow::{bail, Context, Result};
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let git_dir = repo.git_dir()?.to_path_buf();
    let workdir = repo.workdir()?.to_path_buf();
    let current = repo.current_branch()?;

    // Check if this looks like a worktree (workdir != main repo workdir)
    // We detect by checking if the worktree path is inside the configured worktrees dir.
    // Even without that, just register the current branch/path.

    let slug = current
        .split('/')
        .next_back()
        .unwrap_or(&current)
        .to_string();

    let mut registry = Registry::load(&git_dir)?;

    if registry.find_by_name(&slug).is_some() {
        bail!(
            "A worktree named '{}' is already registered. \
             Use `stax agent list` to see all registered worktrees.",
            slug
        );
    }

    let parent_rev = repo
        .branch_commit(&current)
        .context("Could not read current branch commit")?;
    let _ = parent_rev; // used for validation only

    registry.add(AgentWorktree {
        name: slug.clone(),
        branch: current.clone(),
        path: workdir.clone(),
        created_at: chrono::Local::now().to_rfc3339(),
    });
    registry.save()?;

    println!(
        "{}  '{}' → branch '{}' at {}",
        "Registered".green().bold(),
        slug.cyan(),
        current.blue(),
        workdir.display().to_string().dimmed()
    );

    Ok(())
}
