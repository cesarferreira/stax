use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Output};

const FEATURE_ENABLED_KEY: &str = "stax.nativeStack.enabled";

/// Below this version, `gh-stack` reports Personal Access Token rejections
/// with the same ambiguous "Stacked PRs are not enabled" message it uses for
/// a genuinely feature-disabled repo (fixed in v0.0.6's "PAT auth warning"
/// change, which introduced the distinct "Personal access tokens are not
/// supported" message `auth_token_unsupported_output` matches on). Below
/// this version, an auth-token issue can still be misclassified as
/// `FeatureDisabled` and incorrectly cached. This is purely a `doctor`
/// diagnostic — `gh stack link` itself still works on any version that
/// passes `link_command_supported` (added after v0.0.1).
const RECOMMENDED_GH_STACK_VERSION: (u32, u32, u32) = (0, 0, 6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionStatus {
    NoGh,
    NoExtension,
    /// `github/gh-stack` is installed but predates the `gh stack link` command
    /// (added after v0.0.1), so stax cannot register native stacks with it.
    Outdated,
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionStatus {
    /// Version string on the `gh extension list` line couldn't be parsed
    /// (unreleased/dev build, unexpected format, etc.) — not treated as an
    /// error since `link_command_supported` already gates real capability.
    Unknown,
    BelowRecommended {
        installed: String,
    },
    MeetsRecommended {
        installed: String,
    },
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
    FeatureDisabled {
        message: String,
    },
    /// GitHub's native Stacked PRs API is in private preview and rejects
    /// Personal Access Tokens outright — only OAuth app tokens (the ones
    /// stored via `gh auth login`) are accepted. This is distinct from
    /// `FeatureDisabled` (the repo/org not having the feature at all) and
    /// must never be cached as such, since it depends on which `gh` account
    /// is active rather than any durable repo-level fact.
    AuthTokenUnsupported {
        message: String,
    },
    SinglePrValidationRejected {
        message: String,
    },
    Failed {
        message: String,
    },
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

pub fn version_status() -> VersionStatus {
    version_status_with_env(&[])
}

pub fn version_status_with_path(path: &str) -> VersionStatus {
    version_status_with_env(&[("PATH", path)])
}

/// Checks the installed `github/gh-stack` version against
/// `RECOMMENDED_GH_STACK_VERSION`. Informational only — never blocks `gh
/// stack link` from being attempted; surfaced by `stax doctor` so users on
/// an old version know to upgrade for reliable auth-error diagnostics.
pub fn version_status_with_env(env: &[(&str, &str)]) -> VersionStatus {
    let Some(installed) = installed_gh_stack_version(env) else {
        return VersionStatus::Unknown;
    };

    if parse_semver(&installed).is_some_and(|v| v >= RECOMMENDED_GH_STACK_VERSION) {
        VersionStatus::MeetsRecommended { installed }
    } else {
        VersionStatus::BelowRecommended { installed }
    }
}

/// Extracts the raw version string (e.g. `"0.0.7"`) from the `gh extension
/// list` line for `github/gh-stack`, if present and parseable.
fn installed_gh_stack_version(env: &[(&str, &str)]) -> Option<String> {
    let output = gh_command(env).args(["extension", "list"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find(|line| line.contains("github/gh-stack"))
        .and_then(|line| {
            line.split_whitespace()
                .find_map(|token| {
                    token
                        .strip_prefix('v')
                        .filter(|v| parse_semver(v).is_some())
                })
                .map(str::to_string)
        })
}

fn parse_semver(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
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
        // Must be checked before `feature_disabled_output`: gh-stack's PAT
        // rejection message also contains "private preview", which would
        // otherwise be misclassified as the repo/org lacking the feature.
        Ok(output) if auth_token_unsupported_output(&output) => LinkOutcome::AuthTokenUnsupported {
            message: command_message(&output),
        },
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
        Ok(output) if auth_token_unsupported_output(&output) => LinkOutcome::AuthTokenUnsupported {
            message: command_message(&output),
        },
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

/// `gh` env vars that override the CLI's stored (OAuth) credentials.
/// GitHub's native Stacked PRs API is in private preview and rejects
/// Personal Access Tokens, so stax strips these before shelling out to
/// `gh stack`, letting `gh` fall back to a keyring-stored OAuth account
/// even when a PAT is exported for other tooling (CI scripts, other CLIs).
const AUTH_OVERRIDE_ENV_VARS: &[&str] = &["GH_TOKEN", "GITHUB_TOKEN"];

fn gh_command(env: &[(&str, &str)]) -> Command {
    let mut command = Command::new("gh");
    for (key, value) in env {
        command.env(key, value);
    }
    for var in AUTH_OVERRIDE_ENV_VARS {
        command.env_remove(var);
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

fn auth_token_unsupported_output(output: &Output) -> bool {
    command_message(output)
        .to_lowercase()
        .contains("personal access token")
}

/// True when `gh stack link` rejected the request because the requested PR
/// chain shares ancestor PRs with another branch that's already registered
/// as a native GitHub Stack. GitHub's native Stack feature is inherently
/// linear — a PR can only anchor one native-stack "tip" at a time — so this
/// fires whenever a *local* stack forks (two branches created off the same
/// ancestor branch each try to register their own native Stack). gh-stack
/// surfaces this in a couple of different shapes depending on whether it
/// detects the conflict up front or only after attempting a reorder:
///   - `"Cannot update stack: this would remove #123 from the stack"`
///   - `"Failed to update stack (HTTP 409): Stack contents have changed"`
///     (seen alongside a `422 PullRequest.base is invalid` on the branch
///     gh-stack tried to reparent to fit its assumed linear order)
///
/// Both are surfaced as the same plain-language note instead of the raw
/// multi-line CLI dump.
pub(crate) fn is_stack_fork_conflict(message: &str) -> bool {
    let lower = message.to_lowercase();
    (lower.contains("would remove") && lower.contains("from the stack"))
        || lower.contains("stack contents have changed")
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
