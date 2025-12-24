use crate::config::Config;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Password};

pub fn run(token: Option<String>) -> Result<()> {
    let token = match token {
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
    };

    let mut config = Config::load()?;
    config.set_github_token(&token);
    config.save()?;

    println!("{}", "✓ GitHub token saved!".green());
    println!(
        "Config stored at: {}",
        Config::path()?.display().to_string().dimmed()
    );

    Ok(())
}
