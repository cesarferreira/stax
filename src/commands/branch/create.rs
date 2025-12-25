use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use crate::remote;
use anyhow::{bail, Result};
use colored::Colorize;

pub fn run(
    name: Option<String>,
    message: Option<String>,
    from: Option<String>,
    prefix: Option<String>,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let current = repo.current_branch()?;
    let parent_branch = from.unwrap_or_else(|| current.clone());

    if repo.branch_commit(&parent_branch).is_err() {
        anyhow::bail!("Branch '{}' does not exist", parent_branch);
    }

    // Get the branch name from either name or message
    let input = match (name, message) {
        (Some(n), _) => n,
        (None, Some(m)) => m,
        (None, None) => bail!("Branch name required. Use: stax bc <name> or stax bc -m \"message\""),
    };

    // Format the branch name according to config
    let branch_name = match prefix.as_deref() {
        Some(_) => config.format_branch_name_with_prefix_override(&input, prefix.as_deref()),
        None => config.format_branch_name(&input),
    };

    // Create the branch
    if parent_branch == current {
        repo.create_branch(&branch_name)?;
    } else {
        repo.create_branch_at(&branch_name, &parent_branch)?;
    }

    // Track it with current branch as parent
    let parent_rev = repo.branch_commit(&parent_branch)?;
    let meta = BranchMetadata::new(&parent_branch, &parent_rev);
    meta.write(repo.inner(), &branch_name)?;

    // Checkout the new branch
    repo.checkout(&branch_name)?;

    if let Ok(remote_branches) = remote::get_remote_branches(repo.workdir()?, config.remote_name()) {
        if !remote_branches.contains(&parent_branch) {
            println!(
                "{}",
                format!(
                    "Warning: parent '{}' is not on remote '{}'.",
                    parent_branch,
                    config.remote_name()
                )
                .yellow()
            );
        }
    }

    println!(
        "Created and switched to branch '{}' (stacked on {})",
        branch_name.green(),
        parent_branch.blue()
    );

    Ok(())
}
