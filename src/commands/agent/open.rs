use crate::config::Config;
use crate::git::GitRepo;
use super::registry::Registry;
use super::util::{open_in_editor, resolve_editor};
use anyhow::{bail, Context, Result};
use colored::Colorize;

pub fn run(name_or_slug: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let git_dir = repo.git_dir()?.to_path_buf();

    let registry = Registry::load(&git_dir)?;

    if registry.entries.is_empty() {
        bail!("No agent worktrees registered. Create one with: stax agent create <title>");
    }

    let entry = match name_or_slug {
        Some(ref name) => registry
            .find_by_name(name)
            .with_context(|| format!("No agent worktree found for '{}'", name))?,
        None => {
            // Interactive fuzzy picker
            let items: Vec<String> = registry
                .entries
                .iter()
                .map(|e| format!("{} ({})", e.name, e.branch))
                .collect();

            let selection = dialoguer::FuzzySelect::with_theme(
                &dialoguer::theme::ColorfulTheme::default(),
            )
            .with_prompt("Select agent worktree")
            .items(&items)
            .default(0)
            .interact()
            .context("Picker cancelled")?;

            &registry.entries[selection]
        }
    };

    if !entry.path.exists() {
        bail!(
            "Worktree path '{}' no longer exists. Run `stax agent prune` to clean up.",
            entry.path.display()
        );
    }

    let editor_cmd = resolve_editor(&config, false, false, true);
    let cmd = editor_cmd.unwrap_or_else(|| "cursor".to_string());

    println!(
        "{}  '{}' in {}",
        "Opening".green().bold(),
        entry.name.cyan(),
        cmd.dimmed()
    );
    println!("  Path: {}", entry.path.display().to_string().dimmed());

    open_in_editor(&cmd, &entry.path)?;

    Ok(())
}
