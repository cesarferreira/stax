use crate::commands::agent::util::ensure_gitignore;
use crate::commands::shell_setup;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, FuzzySelect, Input};
use std::fs;

pub fn run(branch: Option<String>, name: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;

    // Prompt for shell integration if not installed
    shell_setup::prompt_if_missing()?;

    let branch_name = match branch {
        Some(b) => b,
        None => pick_branch_interactively(&repo)?,
    };

    // Check if branch exists; if not, offer to create it
    let branch_exists = repo.branch_commit(&branch_name).is_ok();
    if !branch_exists {
        let create_it = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!(
                "Branch '{}' does not exist. Create it stacked on current branch?",
                branch_name
            ))
            .default(true)
            .interact()?;
        if !create_it {
            println!("{}", "Aborted.".dimmed());
            return Ok(());
        }
    }

    // Derive worktree name
    let worktree_name = match name {
        Some(n) => n,
        None => derive_unique_name(&repo, &branch_name)?,
    };

    let worktrees_dir = repo.worktrees_dir()?;
    let worktree_path = worktrees_dir.join(&worktree_name);

    if worktree_path.exists() {
        bail!(
            "Worktree path '{}' already exists.",
            worktree_path.display()
        );
    }

    fs::create_dir_all(&worktrees_dir)?;

    let main_dir = repo.main_repo_workdir()?;
    ensure_gitignore(&main_dir, ".worktrees")?;

    if branch_exists {
        repo.worktree_create(&branch_name, &worktree_path)?;
    } else {
        let current = repo.current_branch()?;
        let parent_rev = repo.branch_commit(&current)?;
        repo.worktree_create_new_branch(&branch_name, &worktree_path, &current)?;
        let meta = BranchMetadata::new(&current, &parent_rev);
        meta.write(repo.inner(), &branch_name)?;
    }

    println!(
        "{}  worktree '{}' → branch '{}'",
        "Created".green().bold(),
        worktree_name.cyan(),
        branch_name.blue()
    );
    println!(
        "  Path:   {}",
        worktree_path.display().to_string().dimmed()
    );

    if std::env::var("STAX_SHELL_INTEGRATION").is_ok() {
        println!(
            "\n  {}",
            format!("stax worktree go {}", worktree_name).cyan()
        );
    } else {
        println!(
            "\n  {}",
            format!("cd {}", worktree_path.display()).cyan()
        );
    }

    Ok(())
}

fn pick_branch_interactively(repo: &GitRepo) -> Result<String> {
    let branches = repo.list_branches()?;

    if branches.is_empty() {
        bail!("No local branches found.");
    }

    let current = repo.current_branch().unwrap_or_default();
    let default_idx = branches.iter().position(|b| b == &current).unwrap_or(0);

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select branch for worktree")
        .items(&branches)
        .default(default_idx)
        .interact()?;

    Ok(branches[selection].clone())
}

/// Derive a unique short name from the branch, handling path-prefix collisions.
fn derive_unique_name(repo: &GitRepo, branch: &str) -> Result<String> {
    let existing_worktrees = repo.list_worktrees()?;
    let existing_names: std::collections::HashSet<String> = existing_worktrees
        .iter()
        .map(|wt| wt.name.clone())
        .collect();

    // Try last segment first: "feature/auth-api" → "auth-api"
    let segments: Vec<&str> = branch.split('/').collect();
    for start in (0..segments.len()).rev() {
        let candidate = segments[start..].join("-");
        if !existing_names.contains(&candidate) {
            return Ok(candidate);
        }
    }

    // Full branch with slashes replaced by dashes
    let full = branch.replace('/', "-");
    if !existing_names.contains(&full) {
        return Ok(full);
    }

    // Append numeric suffix
    for i in 2..=99u32 {
        let candidate = format!("{}-{}", full, i);
        if !existing_names.contains(&candidate) {
            return Ok(candidate);
        }
    }

    // Last resort: prompt
    let name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Worktree name")
        .interact_text()?;
    Ok(name)
}
