use crate::commands::generate;
use crate::config::Config;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Select};
use std::fs;
use std::io::IsTerminal;

pub fn run(reset_ai: bool, no_prompt: bool, yes: bool, set_ai: bool) -> Result<()> {
    if reset_ai {
        return reset_ai_defaults(no_prompt, yes);
    }
    if set_ai {
        return set_ai_interactive();
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

    println!();
    println!("{}", "CI watch alerts:".blue().bold());
    println!("  [ci]");
    println!(r#"  alert = false"#);
    println!(r#"  # success_alert_sound = "/path/to/ci-success.wav"  # optional"#);
    println!(r#"  # error_alert_sound = "/path/to/ci-error.wav"      # optional"#);

    println!();
    println!("{}", "Per-feature AI overrides:".blue().bold());
    println!(
        r#"  [ai.generate]  # create/PR generation (stax create --ai, stax generate, stax submit --ai)"#
    );
    println!(r#"  [ai.standup]   # standup summaries (stax standup --ai)"#);
    println!(r#"  [ai.resolve]   # conflict resolution (stax resolve)"#);
    println!(r#"  [ai.lane]      # interactive AI lanes (stax lane)"#);
    println!(r#"  # Each accepts optional `agent` and `model` keys."#);
    println!(r#"  # Omitted keys fall back to [ai] global defaults."#);

    Ok(())
}

fn set_ai_interactive() -> Result<()> {
    const FEATURES: &[(&str, &str)] = &[
        (
            "global",
            "Global default  (used when no feature override is set)",
        ),
        (
            "generate",
            "generate        (create/PR details — stax create --ai, stax generate, stax submit --ai)",
        ),
        (
            "standup",
            "standup         (standup summary — stax standup --ai)",
        ),
        (
            "resolve",
            "resolve         (conflict resolution — stax resolve)",
        ),
        (
            "lane",
            "lane            (interactive coding agent — stax lane)",
        ),
    ];

    let items: Vec<&str> = FEATURES.iter().map(|(_, label)| *label).collect();

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Configure AI agent/model for which feature?")
        .items(&items)
        .default(0)
        .interact()?;

    let (feature, _) = FEATURES[selection];
    let mut config = Config::load()?;
    generate::prompt_for_feature_ai(&mut config, feature)?;

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
