use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;

pub fn run(branch: Option<String>, frozen: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let branch = branch.unwrap_or(repo.current_branch()?);
    let mut metadata = BranchMetadata::read(repo.inner(), &branch)?
        .with_context(|| format!("'{branch}' is not a tracked branch"))?;

    if metadata.frozen == frozen {
        println!(
            "Branch '{}' is already {}.",
            branch.cyan(),
            if frozen { "frozen" } else { "unfrozen" }
        );
        return Ok(());
    }

    metadata.frozen = frozen;
    metadata.write(repo.inner(), &branch)?;
    println!(
        "{} Branch '{}' {}.",
        "✓".green(),
        branch.cyan(),
        if frozen { "frozen" } else { "unfrozen" }
    );
    Ok(())
}
