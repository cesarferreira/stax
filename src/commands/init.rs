use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Select};
use std::io::IsTerminal;

/// Run initialization if needed, returns true if initialized (or already was)
pub fn ensure_initialized() -> Result<bool> {
    let repo = match GitRepo::open() {
        Ok(r) => r,
        Err(_) => return Ok(false), // Not in a git repo, skip init
    };

    if repo.is_initialized() {
        return Ok(true);
    }

    // If not interactive (e.g., in tests or scripts), auto-init silently
    if !std::io::stdin().is_terminal() {
        return auto_init(&repo);
    }

    run_init(&repo)
}

/// Auto-initialize without prompts (for non-interactive use)
fn auto_init(repo: &GitRepo) -> Result<bool> {
    if let Ok(trunk) = repo.detect_trunk() {
        repo.set_trunk(&trunk)?;
        return Ok(true);
    }
    Ok(false)
}

/// Run the interactive initialization
fn run_init(repo: &GitRepo) -> Result<bool> {
    println!(
        "{}",
        "stax has not been initialized, setting up now...".dimmed()
    );
    println!();
    println!("{}", "Welcome to stax!".green().bold());
    println!();

    // Detect and confirm trunk branch
    let detected_trunk = repo.detect_trunk().ok();
    let branches = repo.list_branches()?;

    // Handle empty repo (no branches yet)
    if branches.is_empty() {
        if let Some(detected) = detected_trunk {
            // Use detected trunk even if no branches exist yet
            repo.set_trunk(&detected)?;
            println!("Trunk set to {}", detected.cyan());
            println!();
            println!("{}", "Ready to go! Try `stax bc <name>` to create your first branch.".green());
            return Ok(true);
        } else {
            // No branches and can't detect trunk - need at least one commit
            anyhow::bail!(
                "No branches found. Please make an initial commit first:\n  \
                 git add . && git commit -m \"Initial commit\""
            );
        }
    }

    let trunk = if let Some(detected) = &detected_trunk {
        // Show selection with detected as default
        let prompt = format!(
            "Select trunk branch (PRs target this) - detected {}",
            detected.cyan()
        );

        let default_idx = branches.iter().position(|b| b == detected).unwrap_or(0);

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt(&prompt)
            .items(&branches)
            .default(default_idx)
            .interact()?;

        branches[selection].clone()
    } else {
        // No auto-detection, must select
        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select trunk branch (PRs target this)")
            .items(&branches)
            .interact()?;

        branches[selection].clone()
    };

    // Save trunk setting
    repo.set_trunk(&trunk)?;
    println!("Trunk set to {}", trunk.cyan());
    println!();

    // Offer to track existing branches
    let other_branches: Vec<_> = branches.iter().filter(|b| *b != &trunk).collect();

    if !other_branches.is_empty() {
        let track = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Would you like to track existing branches?")
            .default(false)
            .interact()?;

        if track {
            println!(
                "{}",
                "Use `stax branch track` on each branch to set its parent.".dimmed()
            );
        }
    }

    println!();
    println!("{}", "Ready to go! Try `stax bc <name>` to create your first branch.".green());

    Ok(true)
}
