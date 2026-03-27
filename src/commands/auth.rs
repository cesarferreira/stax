use crate::config::Config;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Password};

pub fn run(token: Option<String>, from_gh: bool) -> Result<()> {
    let token = if from_gh {
        Config::gh_cli_token_for_import()?
    } else {
        match token {
            Some(t) => t,
            None => {
                println!(
                    "Enter a personal access token for your forge (GitHub, GitLab, or Gitea)."
                );
                println!(
                    "It is stored once and reused for whichever host your `origin` remote uses."
                );
                println!(
                    "Examples: {} (GitHub), or your GitLab/Gitea instance token settings.",
                    "https://github.com/settings/tokens".cyan()
                );
                println!("Use a token with API scopes that allow reading/writing merge requests or pull requests.");
                println!();

                Password::with_theme(&ColorfulTheme::default())
                    .with_prompt("Token")
                    .interact()?
            }
        }
    };

    Config::set_github_token(&token)?;

    println!("{}", "✓ API token saved!".green());
    if from_gh {
        println!("{}", "Imported from `gh auth token`.".dimmed());
    }
    println!(
        "Credentials stored at: {}",
        Config::dir()?
            .join(".credentials")
            .display()
            .to_string()
            .dimmed()
    );
    println!();
    println!(
        "{}",
        "Note: Token is stored separately from config (safe to commit config to dotfiles)".dimmed()
    );

    Ok(())
}

pub fn status() -> Result<()> {
    let status = Config::github_auth_status();

    println!("{}", "Auth status".bold());
    println!(
        "{}",
        "(One saved token is used for GitHub, GitLab, and Gitea API calls.)".dimmed()
    );
    if let Some(source) = status.active_source {
        println!(
            "{} {}",
            "✓ Active source:".green(),
            source.display_name().cyan()
        );
    } else {
        println!("{}", "⚠ No GitHub auth source resolved.".yellow());
    }
    println!();
    println!("{}", "Resolution order:".bold());
    print_source_line("1. STAX_GITHUB_TOKEN", status.stax_env_available, true, "");
    print_source_line(
        "2. credentials file (~/.config/stax/.credentials)",
        status.credentials_file_available,
        true,
        "",
    );

    let gh_note = if let Some(hostname) = status.gh_hostname.as_deref() {
        format!(" (hostname: {})", hostname)
    } else {
        String::new()
    };
    print_source_line(
        "3. gh auth token",
        status.gh_cli_available,
        status.use_gh_cli,
        gh_note.as_str(),
    );
    print_source_line(
        "4. GITHUB_TOKEN",
        status.github_env_available,
        status.allow_github_token_env,
        " (disabled by default; enable with [auth].allow_github_token_env = true)",
    );

    if status.active_source.is_none() {
        println!();
        println!(
            "{}",
            "Run `stax auth`, `stax auth --from-gh`, or `gh auth login`.".dimmed()
        );
    }

    Ok(())
}

fn print_source_line(label: &str, available: bool, enabled: bool, note: &str) {
    let availability = if available {
        "available".green()
    } else {
        "not found".yellow()
    };
    let enabled_state = if enabled {
        "enabled".dimmed()
    } else {
        "disabled".yellow()
    };

    println!(
        "  {}: {} ({}){}",
        label,
        availability,
        enabled_state,
        if note.is_empty() { "" } else { note }
    );
}
