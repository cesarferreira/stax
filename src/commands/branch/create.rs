use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;

pub fn run(name: Option<String>, message: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let current = repo.current_branch()?;
    let current_rev = repo.branch_commit(&current)?;

    // Get the branch name from either name or message
    let input = match (name, message) {
        (Some(n), _) => n,
        (None, Some(m)) => m,
        (None, None) => bail!("Branch name required. Use: stax bc <name> or stax bc -m \"message\""),
    };

    // Format the branch name according to config
    let branch_name = config.format_branch_name(&input);

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
