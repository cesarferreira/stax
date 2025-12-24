use crate::config::Config;
use anyhow::Result;
use colored::Colorize;
use std::fs;

pub fn run() -> Result<()> {
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

    Ok(())
}
