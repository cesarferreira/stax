use crate::config::Config;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, FuzzySelect};
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

    run_interactive(&repo, false)?;
    Ok(true)
}

/// Initialize or reconfigure stax in the current repo.
pub fn run(trunk: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;

    if let Some(trunk) = trunk {
        set_trunk(repo, &trunk)?;
        return Ok(());
    }

    if !std::io::stdin().is_terminal() {
        if repo.is_initialized() {
            anyhow::bail!(
                "Repository already initialized. Use `stax init --trunk <branch>` to change trunk non-interactively."
            );
        }

        if auto_init(&repo)? {
            return Ok(());
        }

        anyhow::bail!(
            "Could not detect a trunk branch automatically. Use `stax init --trunk <branch>`."
        );
    }

    run_interactive(&repo, repo.is_initialized())
}

/// Auto-initialize without prompts (for non-interactive use)
fn auto_init(repo: &GitRepo) -> Result<bool> {
    if let Ok(trunk) = repo.detect_trunk() {
        repo.set_trunk(&trunk)?;
        return Ok(true);
    }
    Ok(false)
}

fn set_trunk(repo: GitRepo, trunk: &str) -> Result<()> {
    let branches = repo.list_branches()?;

    if branches.is_empty() {
        anyhow::bail!(
            "No branches found. Please make an initial commit first:\n  \
             git add . && git commit -m \"Initial commit\""
        );
    }

    if !branches.iter().any(|branch| branch == trunk) {
        anyhow::bail!(
            "Branch '{}' does not exist. Available branches: {}",
            trunk,
            branches.join(", ")
        );
    }

    let previous_trunk = if repo.is_initialized() {
        repo.trunk_branch().ok()
    } else {
        None
    };
    repo.set_trunk(trunk)?;

    match previous_trunk {
        Some(previous) if previous == trunk => {
            println!("Trunk unchanged: {}", trunk.cyan());
        }
        Some(previous) => {
            println!("Trunk changed: {} -> {}", previous.cyan(), trunk.green());
        }
        None => {
            println!("Trunk set to {}", trunk.cyan());
        }
    }

    // Enable git rerere for conflict resolution memory (if not disabled in config)
    let config = Config::load().ok();
    let should_enable_rerere = config.as_ref().map(|c| c.git.rerere).unwrap_or(true);

    if should_enable_rerere {
        match repo.enable_rerere() {
            Ok(_) => println!("{}  Enabled git rerere", "✓".green().bold()),
            Err(e) => eprintln!(
                "{}  Failed to enable rerere: {}",
                "Warning:".yellow().bold(),
                e
            ),
        }
    }

    Ok(())
}

/// Run interactive initialization or trunk reconfiguration.
fn run_interactive(repo: &GitRepo, reconfigure: bool) -> Result<()> {
    if reconfigure {
        if let Ok(current) = repo.trunk_branch() {
            println!("Current trunk: {}", current.cyan());
            println!();
        }
    } else {
        println!(
            "{}",
            "stax has not been initialized, setting up now...".dimmed()
        );
        println!();
        println!("{}", "Welcome to stax!".green().bold());
        println!();
    }

    let branches = repo.list_branches()?;
    let detected_trunk = repo
        .trunk_branch()
        .ok()
        .or_else(|| repo.detect_trunk().ok());

    if branches.is_empty() {
        if let Some(detected) = detected_trunk {
            repo.set_trunk(&detected)?;
            println!("Trunk set to {}", detected.cyan());
            if !reconfigure {
                println!();
                println!(
                    "{}",
                    "Ready to go! Try `stax bc <name>` to create your first branch.".green()
                );
            }
            return Ok(());
        }

        anyhow::bail!(
            "No branches found. Please make an initial commit first:\n  \
             git add . && git commit -m \"Initial commit\""
        );
    }

    let prompt = if reconfigure {
        "Select trunk branch"
    } else {
        "Select trunk branch (PRs target this)"
    };

    let selection = if let Some(detected) = &detected_trunk {
        let default_idx = branches.iter().position(|b| b == detected).unwrap_or(0);
        FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("{} - detected {}", prompt, detected.cyan()))
            .items(&branches)
            .default(default_idx)
            .interact()?
    } else {
        FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .items(&branches)
            .interact()?
    };

    let trunk = branches[selection].clone();
    let previous_trunk = if repo.is_initialized() {
        repo.trunk_branch().ok()
    } else {
        None
    };
    repo.set_trunk(&trunk)?;

    match previous_trunk {
        Some(previous) if previous == trunk => println!("Trunk unchanged: {}", trunk.cyan()),
        Some(previous) => println!("Trunk changed: {} -> {}", previous.cyan(), trunk.green()),
        None => println!("Trunk set to {}", trunk.cyan()),
    }

    if reconfigure {
        return Ok(());
    }

    println!();

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
    println!(
        "{}",
        "Ready to go! Try `stax bc <name>` to create your first branch.".green()
    );

    Ok(())
}
