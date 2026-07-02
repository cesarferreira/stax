use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

/// Remove stax metadata for a branch, but keep the git branch intact.
pub fn run(branch: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;
    let target = branch.unwrap_or(repo.current_branch()?);

    if BranchMetadata::read(repo.inner(), &target)?.is_none() {
        println!("Branch '{}' is not tracked.", target.yellow());
        return Ok(());
    }

    BranchMetadata::delete(repo.inner(), &target)?;
    println!(
        "✓ Untracked '{}' (removed stax metadata, kept git branch)",
        target.green()
    );

    Ok(())
}
