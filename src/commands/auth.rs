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
                println!("Enter your GitHub personal access token.");
                println!(
                    "Create one at: {}",
                    "https://github.com/settings/tokens".cyan()
                );
                println!("Required scopes: repo, read:org");
                println!();

                Password::with_theme(&ColorfulTheme::default())
                    .with_prompt("Token")
                    .interact()?
            }
        }
    };

    Config::set_github_token(&token)?;

    println!("{}", "✓ GitHub token saved!".green());
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
