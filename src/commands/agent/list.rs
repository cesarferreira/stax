use crate::git::GitRepo;
use super::registry::Registry;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let git_dir = repo.git_dir()?.to_path_buf();

    let registry = Registry::load(&git_dir)?;

    if registry.entries.is_empty() {
        println!("{}", "No agent worktrees registered.".dimmed());
        println!(
            "Create one with: {}",
            "stax agent create <title>".cyan()
        );
        return Ok(());
    }

    println!(
        "{:<20}  {:<35}  {:<6}  {}",
        "NAME".bold(),
        "BRANCH".bold(),
        "EXISTS".bold(),
        "OPEN".bold()
    );
    println!("{}", "─".repeat(90).dimmed());

    for entry in &registry.entries {
        let exists = entry.path.exists();
        let exists_str = if exists {
            "yes".green().to_string()
        } else {
            "no".red().to_string()
        };

        let open_cmd = format!("stax agent open {}", entry.name);

        println!(
            "{:<20}  {:<35}  {:<6}  {}",
            entry.name.cyan(),
            entry.branch.blue(),
            exists_str,
            open_cmd.dimmed()
        );
    }

    Ok(())
}
