use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Output};

const FEATURE_ENABLED_KEY: &str = "stax.nativeStack.enabled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionStatus {
    NoGh,
    NoExtension,
    /// `github/gh-stack` is installed but predates the `gh stack link` command
    /// (added after v0.0.1), so stax cannot register native stacks with it.
    Outdated,
    Installed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureState {
    Unknown,
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkOutcome {
    Linked,
    FeatureDisabled { message: String },
    SinglePrValidationRejected { message: String },
    Failed { message: String },
}

pub fn extension_status() -> ExtensionStatus {
    extension_status_with_env(&[])
}

pub fn extension_status_with_path(path: &str) -> ExtensionStatus {
    extension_status_with_env(&[("PATH", path)])
}

fn extension_status_with_env(env: &[(&str, &str)]) -> ExtensionStatus {
    let version = gh_command(env).arg("--version").output();
    match version {
        Ok(output) if output.status.success() => {}
        Ok(_) => return ExtensionStatus::NoGh,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return ExtensionStatus::NoGh,
        Err(_) => return ExtensionStatus::NoGh,
    }

    let extensions = gh_command(env).args(["extension", "list"]).output();
    match extensions {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.lines().any(|line| line.contains("github/gh-stack")) {
                if link_command_supported(env) {
                    ExtensionStatus::Installed
                } else {
                    ExtensionStatus::Outdated
                }
            } else {
                ExtensionStatus::NoExtension
            }
        }
        _ => ExtensionStatus::NoExtension,
    }
}

/// Probe whether the installed `gh-stack` exposes the `link` subcommand that
/// stax relies on. Parses `gh stack --help` output rather than exit codes so it
/// stays robust across the extension's cobra help behavior.
fn link_command_supported(env: &[(&str, &str)]) -> bool {
    match gh_command(env).args(["stack", "--help"]).output() {
        Ok(output) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            text.lines()
                .any(|line| line.trim_start().starts_with("link"))
        }
        Err(_) => false,
    }
}

pub fn feature_enabled(repo_path: impl AsRef<Path>) -> FeatureState {
    let output = Command::new("git")
        .args(["config", "--get", FEATURE_ENABLED_KEY])
        .current_dir(repo_path.as_ref())
        .output();

    match output {
        Ok(output) if output.status.success() => match String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_lowercase()
            .as_str()
        {
            "true" => FeatureState::Enabled,
            "false" => FeatureState::Disabled,
            _ => FeatureState::Unknown,
        },
        _ => FeatureState::Unknown,
    }
}

pub fn set_feature_enabled(repo_path: impl AsRef<Path>, enabled: bool) -> Result<()> {
    let value = if enabled { "true" } else { "false" };
    let output = Command::new("git")
        .args(["config", FEATURE_ENABLED_KEY, value])
        .current_dir(repo_path.as_ref())
        .output()
        .context("Failed to run git config for native stack feature cache")?;

    if !output.status.success() {
        bail!(
            "failed to set native stack feature cache: {}",
            output_details(&output)
        );
    }

    Ok(())
}

pub fn link_stack(pr_numbers: &[u64], base: &str, remote: &str) -> LinkOutcome {
    link_stack_with_env(pr_numbers, base, remote, &[])
}

pub fn link_stack_with_path(
    pr_numbers: &[u64],
    base: &str,
    remote: &str,
    path: &str,
) -> LinkOutcome {
    link_stack_with_env(pr_numbers, base, remote, &[("PATH", path)])
}

pub fn link_stack_with_env(
    pr_numbers: &[u64],
    base: &str,
    remote: &str,
    env: &[(&str, &str)],
) -> LinkOutcome {
    let mut command = gh_command(env);
    command.args(["stack", "link"]);
    for number in pr_numbers {
        command.arg(number.to_string());
    }
    command.args(["--base", base, "--remote", remote]);

    match command.output() {
        Ok(output) if output.status.success() => LinkOutcome::Linked,
        Ok(output) if feature_disabled_output(&output) => LinkOutcome::FeatureDisabled {
            message: command_message(&output),
        },
        Ok(output) if single_pr_validation_output(pr_numbers, &output) => {
            LinkOutcome::SinglePrValidationRejected {
                message: command_message(&output),
            }
        }
        Ok(output) => LinkOutcome::Failed {
            message: command_message(&output),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => LinkOutcome::Failed {
            message: "`gh` executable not found".to_string(),
        },
        Err(err) => LinkOutcome::Failed {
            message: err.to_string(),
        },
    }
}

pub fn install_extension() -> Result<()> {
    install_extension_with_env(&[])
}

pub fn unlink_stack() -> LinkOutcome {
    unlink_stack_with_env(&[])
}

pub fn unlink_stack_with_env(env: &[(&str, &str)]) -> LinkOutcome {
    match gh_command(env).args(["stack", "unstack"]).output() {
        Ok(output) if output.status.success() => LinkOutcome::Linked,
        Ok(output) if feature_disabled_output(&output) => LinkOutcome::FeatureDisabled {
            message: command_message(&output),
        },
        Ok(output) => LinkOutcome::Failed {
            message: command_message(&output),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => LinkOutcome::Failed {
            message: "`gh` executable not found".to_string(),
        },
        Err(err) => LinkOutcome::Failed {
            message: err.to_string(),
        },
    }
}

pub fn install_extension_with_env(env: &[(&str, &str)]) -> Result<()> {
    let output = gh_command(env)
        .args(["extension", "install", "github/gh-stack"])
        .output()
        .context("Failed to execute `gh extension install github/gh-stack`")?;

    if !output.status.success() {
        bail!(
            "`gh extension install github/gh-stack` failed: {}",
            output_details(&output)
        );
    }

    Ok(())
}

pub fn upgrade_extension() -> Result<()> {
    upgrade_extension_with_env(&[])
}

pub fn upgrade_extension_with_env(env: &[(&str, &str)]) -> Result<()> {
    let output = gh_command(env)
        .args(["extension", "upgrade", "gh-stack"])
        .output()
        .context("Failed to execute `gh extension upgrade gh-stack`")?;

    if !output.status.success() {
        bail!(
            "`gh extension upgrade gh-stack` failed: {}",
            output_details(&output)
        );
    }

    Ok(())
}

fn gh_command(env: &[(&str, &str)]) -> Command {
    let mut command = Command::new("gh");
    for (key, value) in env {
        command.env(key, value);
    }
    command
}

fn feature_disabled_output(output: &Output) -> bool {
    let message = command_message(output).to_lowercase();
    message.contains("private preview")
        || message.contains("not enabled")
        || message.contains("not been enabled")
        || message.contains("feature has been enabled")
}

fn single_pr_validation_output(pr_numbers: &[u64], output: &Output) -> bool {
    if pr_numbers.len() != 1 {
        return false;
    }

    let message = command_message(output).to_lowercase();
    (message.contains("require") || message.contains("requires") || message.contains("need"))
        && (message.contains("at least two")
            || message.contains("at least 2")
            || message.contains("multiple pr")
            || message.contains("more than one")
            || message.contains("minimum of two"))
}

fn command_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn output_details(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => format!("exit status {}", output.status),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}
