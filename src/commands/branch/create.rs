use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run(name: &str) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let current = repo.current_branch()?;
    let current_rev = repo.branch_commit(&current)?;

    // Format the branch name according to config
    let branch_name = config.format_branch_name(name);

    // Create the branch
    repo.create_branch(&branch_name)?;

    // Track it with current branch as parent
    let meta = BranchMetadata::new(&current, &current_rev);
    meta.write(repo.inner(), &branch_name)?;

    // Checkout the new branch
    repo.checkout(&branch_name)?;

    println!(
        "Created and switched to branch '{}' (stacked on {})",
        branch_name.green(),
        current.blue()
    );

    Ok(())
}
