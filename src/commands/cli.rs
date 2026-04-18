use crate::commands::shell_setup;
use crate::update::{self, InstallMethod};
use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::process::Command;

pub fn run_upgrade() -> Result<()> {
    let install_method = update::detect_install_method();
    println!(
        "{} {}",
        "Upgrading stax with".green().bold(),
        install_method.upgrade_command().cyan()
    );

    let status = run_upgrade_command(install_method)?;
    if !status.success() {
        bail!(
            "Upgrade command failed: {}",
            install_method.upgrade_command()
        );
    }

    if let Err(err) = shell_setup::refresh_installed_snippets() {
        eprintln!(
            "{}  Upgraded stax, but failed to refresh shell integration: {}",
            "Warning:".yellow().bold(),
            err
        );
    }

    println!("{}", "stax upgrade complete.".green().bold());
    Ok(())
}

fn run_upgrade_command(install_method: InstallMethod) -> Result<std::process::ExitStatus> {
    let mut command = match install_method {
        InstallMethod::Cargo => {
            let mut command = Command::new("cargo");
            command.args(["install", "stax"]);
            command
        }
        InstallMethod::Homebrew => {
            let mut command = Command::new("brew");
            command.args(["upgrade", "stax"]);
            command
        }
        InstallMethod::Unknown => {
            #[cfg(windows)]
            let mut command = {
                let mut command = Command::new("cmd");
                command.args(["/C", "upgrade stax"]);
                command
            };

            #[cfg(not(windows))]
            let command = {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                let mut command = Command::new(shell);
                command.args(["-c", "upgrade stax"]);
                command
            };

            command
        }
    };

    command
        .status()
        .with_context(|| format!("Failed to run `{}`", install_method.upgrade_command()))
}
