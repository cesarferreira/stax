use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;

/// Print the absolute path of the named worktree (used by the shell function for `cd`).
pub fn run_path(name: &str) -> Result<()> {
    let path = resolve_path(name)?;
    // Raw path to stdout — the shell wrapper reads this and does `cd`
    println!("{}", path.display());
    Ok(())
}

/// Print a human-readable message with a tip about shell integration.
pub fn run_go(name: &str) -> Result<()> {
    let path = resolve_path(name)?;

    if std::env::var("STAX_SHELL_INTEGRATION").is_ok() {
        // Shell wrapper handles the actual cd; this branch is only hit if the
        // user called `stax worktree go` directly instead of via the wrapper.
        println!("{}", path.display());
    } else {
        println!(
            "{} {}",
            "Worktree path:".dimmed(),
            path.display().to_string().cyan()
        );
        println!();
        println!(
            "{}",
            "Tip: add shell integration for transparent cd:".dimmed()
        );
        println!("  {}", "stax shell-setup --install".cyan());
    }

    Ok(())
}

fn resolve_path(name: &str) -> Result<std::path::PathBuf> {
    let repo = GitRepo::open()?;
    let worktrees = repo.list_worktrees()?;

    // Match by name, branch suffix, or exact branch
    let found = worktrees.iter().find(|wt| {
        wt.name == name
            || wt.branch.as_deref() == Some(name)
            || wt.branch
                .as_deref()
                .map(|b| b.ends_with(&format!("/{}", name)))
                .unwrap_or(false)
    });

    match found {
        Some(wt) => Ok(wt.path.clone()),
        None => bail!("No worktree named '{}'", name),
    }
}
