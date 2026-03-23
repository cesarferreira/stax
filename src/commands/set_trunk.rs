use crate::git::{refs, GitRepo};
use anyhow::Result;
use git2::BranchType;

pub fn run(branch: &str) -> Result<()> {
    let repo = GitRepo::open()?;

    // Validate the branch exists locally
    if repo.inner().find_branch(branch, BranchType::Local).is_err() {
        anyhow::bail!(
            "Branch '{}' does not exist locally. Create it first or check the name.",
            branch
        );
    }

    refs::write_trunk(repo.inner(), branch)?;
    println!("Trunk branch set to '{}'", branch);
    Ok(())
}
