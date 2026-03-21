use crate::commands::generate;
use crate::config::Config;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::fs;
use std::io::IsTerminal;

pub fn run(reset_ai: bool, no_prompt: bool, yes: bool) -> Result<()> {
    if reset_ai {
        return reset_ai_defaults(no_prompt, yes);
    }

    let path = Config::path()?;

    println!("{}", "Config path:".blue().bold());
    println!("  {}\n", path.display());

    if path.exists() {
        let content = fs::read_to_string(&path)?;
        println!("{}", "Contents:".blue().bold());
        println!("{}", content);
    } else {
        println!("{}", "Config file does not exist yet.".yellow());
        println!("Run any stax command to create a default config.");
    }

    println!();
    println!("{}", "Submit stack links setting:".blue().bold());
    println!("  [submit]");
    println!(r#"  stack_links = "comment"  # "comment" | "body" | "both" | "off""#);
    println!(r#"  # Example: stack_links = "body""#);

    Ok(())
}

fn reset_ai_defaults(no_prompt: bool, yes: bool) -> Result<()> {
    let path = Config::path()?;
    let mut config = Config::load()?;

    if !yes && std::io::stdin().is_terminal() {
        let confirmed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Clear saved AI agent/model defaults from config?")
            .default(true)
            .interact()?;

        if !confirmed {
            println!("{}", "Cancelled.".yellow());
            return Ok(());
        }
    }

    let had_saved_defaults = config.clear_ai_defaults();
    config.save()?;

    if had_saved_defaults {
        println!(
            "  {} Cleared saved AI defaults in {}",
            "✓".green().bold(),
            path.display()
        );
    } else {
        println!(
            "  {} No saved AI defaults were set in {}",
            "✓".green().bold(),
            path.display()
        );
    }

    if no_prompt {
        println!(
            "  {} Skipped reconfiguration because --no-prompt was set.",
            "Tip:".dimmed()
        );
        return Ok(());
    }

    if !std::io::stdin().is_terminal() {
        println!(
            "  {} Not running in an interactive terminal, so stax did not re-prompt.",
            "Tip:".dimmed()
        );
        return Ok(());
    }

    println!(
        "  {} Re-select the AI agent/model to save new defaults.",
        "Next:".dimmed()
    );
    let _ = generate::prompt_for_agent_and_model(&mut config, false)?;

    println!(
        "  {} Updated defaults will be used by future interactive AI flows.",
        "Tip:".dimmed()
    );

    Ok(())
}
