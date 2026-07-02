use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Output};

const FEATURE_ENABLED_KEY: &str = "stax.nativeStack.enabled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionStatus {
    NoGh,
    NoExtension,
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
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeStackEntry {
    pub branch: String,
    pub pr_number: Option<u64>,
    pub base: Option<String>,
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
                ExtensionStatus::Installed
            } else {
                ExtensionStatus::NoExtension
            }
        }
        _ => ExtensionStatus::NoExtension,
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

pub fn view_stack(target: &str) -> Result<Vec<NativeStackEntry>> {
    view_stack_with_env(target, &[])
}

pub fn view_stack_with_env(target: &str, env: &[(&str, &str)]) -> Result<Vec<NativeStackEntry>> {
    let output = gh_command(env)
        .args(["stack", "view", "--json", target])
        .output()
        .context("Failed to execute `gh stack view --json`")?;

    if !output.status.success() {
        bail!("`gh stack view --json` failed: {}", output_details(&output));
    }

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse `gh stack view --json`")?;
    Ok(parse_native_stack_entries(&value))
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

fn parse_native_stack_entries(value: &serde_json::Value) -> Vec<NativeStackEntry> {
    let Some(entries) = native_stack_array(value) else {
        return Vec::new();
    };

    entries
        .iter()
        .filter_map(|entry| {
            let branch = string_field(entry, &["branch", "name", "headRefName", "head_ref", "ref"])
                .or_else(|| nested_string_field(entry, "head", &["ref", "name", "branch"]))?;
            let pr_number = number_field(entry, &["pr_number", "prNumber", "number"])
                .or_else(|| nested_number_field(entry, "pull_request", &["number"]))
                .or_else(|| nested_number_field(entry, "pullRequest", &["number"]))
                .or_else(|| nested_number_field(entry, "pr", &["number"]));
            let base = string_field(entry, &["base", "baseRefName", "target", "parent"])
                .or_else(|| nested_string_field(entry, "base", &["ref", "name", "branch"]));

            Some(NativeStackEntry {
                branch,
                pr_number,
                base,
            })
        })
        .collect()
}

fn native_stack_array(value: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    if let Some(array) = value.as_array() {
        return Some(array);
    }

    for key in ["branches", "pullRequests", "pull_requests", "prs", "stack"] {
        if let Some(array) = value.get(key).and_then(|v| v.as_array()) {
            return Some(array);
        }
    }

    None
}

fn string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(|v| v.as_str()))
        .map(ToString::to_string)
}

fn nested_string_field(value: &serde_json::Value, parent: &str, keys: &[&str]) -> Option<String> {
    value
        .get(parent)
        .and_then(|nested| string_field(nested, keys))
}

fn number_field(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(|v| v.as_u64()))
}

fn nested_number_field(value: &serde_json::Value, parent: &str, keys: &[&str]) -> Option<u64> {
    value
        .get(parent)
        .and_then(|nested| number_field(nested, keys))
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
