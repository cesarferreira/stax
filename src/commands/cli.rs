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
            command.args(["install", "stax", "--locked"]);
            command
        }
        InstallMethod::CargoBinstall => {
            let mut command = Command::new("cargo");
            command.args(["binstall", "stax", "--force"]);
            command
        }
        InstallMethod::Homebrew => {
            let mut command = Command::new("brew");
            command.args(["upgrade", "stax"]);
            command
        }
        InstallMethod::Unknown => {
            bail!(
                "Unknown stax installation method. Reinstall or upgrade manually using one of the documented methods: `brew upgrade stax`, `cargo binstall stax --force`, or `cargo install stax --locked`."
            );
        }
    };

    command
        .status()
        .with_context(|| format!("Failed to run `{}`", install_method.upgrade_command()))
}
