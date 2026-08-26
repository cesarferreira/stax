use colored::Colorize;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use update_informer::{Check, registry};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const UPDATE_WORKER_ENV: &str = "STAX_UPDATE_CHECK_WORKER";

fn update_checks_disabled() -> bool {
    std::env::var("STAX_DISABLE_UPDATE_CHECK")
        .ok()
        .map(|v| {
            let value = v.trim().to_ascii_lowercase();
            matches!(value.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

/// Detect how stax was installed based on binary path and installer metadata.
pub(crate) fn detect_install_method() -> InstallMethod {
    match std::env::current_exe() {
        Ok(path) => {
            let cargo_home = cargo_home_from_binary_path(&path).or_else(default_cargo_home);
            install_method_from_path(&path, cargo_home.as_deref())
        }
        Err(_) => InstallMethod::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallMethod {
    Cargo,
    CargoBinstall,
    Homebrew,
    Unknown,
}

impl InstallMethod {
    pub(crate) fn upgrade_command(&self) -> &'static str {
        match self {
            InstallMethod::Cargo => "cargo install stax --locked",
            InstallMethod::CargoBinstall => "cargo binstall stax --force",
            InstallMethod::Homebrew => "brew upgrade stax",
            InstallMethod::Unknown => "manual upgrade required",
        }
    }
}

#[derive(Deserialize)]
struct BinstallRecord {
    name: String,
}

fn cargo_home_from_binary_path(path: &Path) -> Option<PathBuf> {
    let bin_dir = path.parent()?;
    if bin_dir.file_name()? == "bin" && bin_dir.parent()?.file_name()? == ".cargo" {
        bin_dir.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

fn default_cargo_home() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CARGO_HOME") {
        return Some(PathBuf::from(path));
    }

    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".cargo"))
}

fn binstall_metadata_contains_stax(cargo_home: Option<&Path>) -> bool {
    let Some(cargo_home) = cargo_home else {
        return false;
    };
    let metadata_path = cargo_home.join("binstall").join("crates-v1.json");
    let Ok(metadata) = fs::read_to_string(metadata_path) else {
        return false;
    };

    // cargo-binstall stores concatenated JSON objects rather than a JSON array.
    let stream = serde_json::Deserializer::from_str(&metadata).into_iter::<BinstallRecord>();
    stream
        .filter_map(Result::ok)
        .any(|record| record.name == PKG_NAME)
}

fn should_spawn_background_check(disabled: bool, is_worker: bool) -> bool {
    !disabled && !is_worker
}

/// Spawn a detached worker process that refreshes the update cache.
///
/// The command never waits for the worker, so fast local commands do not inherit
/// network latency. The worker uses the same executable and exits after one check.
pub fn spawn_background_check() {
    let is_worker = std::env::var_os(UPDATE_WORKER_ENV).is_some();
    if !should_spawn_background_check(update_checks_disabled(), is_worker) {
        return;
    }

    let Ok(executable) = std::env::current_exe() else {
        return;
    };

    let _ = Command::new(executable)
        .arg("__update-check")
        .env(UPDATE_WORKER_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Refresh the update cache from the hidden worker command.
pub fn run_background_check() {
    if update_checks_disabled() {
        return;
    }

    let informer = update_informer::new(registry::Crates, PKG_NAME, PKG_VERSION)
        .timeout(Duration::from_secs(1))
        .interval(Duration::from_secs(60 * 60 * 24));

    let _ = informer.check_version();
}

/// Check for cached update info and display if a new version is available.
/// This reads from cache only - it won't make network requests or block.
pub fn show_update_notification() {
    if update_checks_disabled() {
        return;
    }

    // Use a very short timeout so this never blocks
    // If there's no cached result, this returns quickly
    let informer = update_informer::new(registry::Crates, PKG_NAME, PKG_VERSION)
        .timeout(Duration::from_millis(1))
        .interval(Duration::from_secs(60 * 60 * 24));

    if let Ok(Some(new_version)) = informer.check_version() {
        let install_method = detect_install_method();
        eprintln!();
        eprintln!(
            "{} {} → {} {}",
            "A new version of stax is available:".yellow(),
            PKG_VERSION.dimmed(),
            new_version.to_string().green().bold(),
            format!("({})", install_method.upgrade_command()).dimmed()
        );
    }
}

/// Parse install method from a given path (for testing)
fn install_method_from_path(path: &Path, cargo_home: Option<&Path>) -> InstallMethod {
    if is_homebrew_path(path) {
        InstallMethod::Homebrew
    } else if is_cargo_bin_path(path) {
        if binstall_metadata_contains_stax(cargo_home) {
            InstallMethod::CargoBinstall
        } else {
            InstallMethod::Cargo
        }
    } else {
        InstallMethod::Unknown
    }
}

fn is_homebrew_path(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name == "homebrew" || name == "Cellar"
    })
}

fn is_cargo_bin_path(path: &Path) -> bool {
    let path = path.to_string_lossy();
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();

    matches!(
        parts.as_slice(),
        [.., ".cargo", "bin", binary] if !binary.is_empty()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cargo_home_with_binstall_metadata() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("temp cargo home");
        let metadata_dir = temp.path().join("binstall");
        fs::create_dir_all(&metadata_dir).expect("metadata dir");
        fs::write(
            metadata_dir.join("crates-v1.json"),
            r#"{"name":"other"}{"name":"stax"}"#,
        )
        .expect("metadata");
        temp
    }

    #[test]
    fn background_check_spawns_only_for_normal_enabled_commands() {
        assert!(should_spawn_background_check(false, false));
        assert!(!should_spawn_background_check(true, false));
        assert!(!should_spawn_background_check(false, true));
        assert!(!should_spawn_background_check(true, true));
    }

    #[test]
    fn test_detect_homebrew_arm() {
        let path = "/opt/homebrew/bin/stax";
        assert!(matches!(
            install_method_from_path(Path::new(path), None),
            InstallMethod::Homebrew
        ));
    }

    #[test]
    fn test_detect_homebrew_cellar_arm() {
        let path = "/opt/homebrew/Cellar/stax/0.5.0/bin/stax";
        assert!(matches!(
            install_method_from_path(Path::new(path), None),
            InstallMethod::Homebrew
        ));
    }

    #[test]
    fn test_detect_homebrew_intel() {
        let path = "/usr/local/Cellar/stax/0.5.0/bin/stax";
        assert!(matches!(
            install_method_from_path(Path::new(path), None),
            InstallMethod::Homebrew
        ));
    }

    #[test]
    fn test_detect_cargo() {
        let path = "/Users/cesar/.cargo/bin/stax";
        assert!(matches!(
            install_method_from_path(Path::new(path), None),
            InstallMethod::Cargo
        ));
    }

    #[test]
    fn test_detect_cargo_linux() {
        let path = "/home/user/.cargo/bin/stax";
        assert!(matches!(
            install_method_from_path(Path::new(path), None),
            InstallMethod::Cargo
        ));
    }

    #[test]
    fn test_detect_cargo_windows() {
        let path = r"C:\Users\user\.cargo\bin\stax.exe";
        assert!(matches!(
            install_method_from_path(Path::new(path), None),
            InstallMethod::Cargo
        ));
    }

    #[test]
    fn test_detect_unknown_usr_local_bin() {
        let path = "/usr/local/bin/stax";
        assert!(matches!(
            install_method_from_path(Path::new(path), None),
            InstallMethod::Unknown
        ));
    }

    #[test]
    fn test_detect_unknown_custom_path() {
        let path = "/opt/mytools/stax";
        assert!(matches!(
            install_method_from_path(Path::new(path), None),
            InstallMethod::Unknown
        ));
    }

    #[test]
    fn test_upgrade_command_cargo() {
        assert_eq!(
            InstallMethod::Cargo.upgrade_command(),
            "cargo install stax --locked"
        );
    }

    #[test]
    fn test_detect_cargo_binstall() {
        let temp = cargo_home_with_binstall_metadata();

        let path = "/home/user/.cargo/bin/stax";
        assert!(matches!(
            install_method_from_path(Path::new(path), Some(temp.path())),
            InstallMethod::CargoBinstall
        ));
    }

    #[test]
    fn test_detect_cargo_binstall_windows() {
        let temp = cargo_home_with_binstall_metadata();

        let path = r"C:\Users\user\.cargo\bin\stax.exe";
        assert!(matches!(
            install_method_from_path(Path::new(path), Some(temp.path())),
            InstallMethod::CargoBinstall
        ));
    }

    #[test]
    fn test_upgrade_command_cargo_binstall() {
        assert_eq!(
            InstallMethod::CargoBinstall.upgrade_command(),
            "cargo binstall stax --force"
        );
    }

    #[test]
    fn test_upgrade_command_homebrew() {
        assert_eq!(
            InstallMethod::Homebrew.upgrade_command(),
            "brew upgrade stax"
        );
    }

    #[test]
    fn test_upgrade_command_unknown() {
        assert_eq!(
            InstallMethod::Unknown.upgrade_command(),
            "manual upgrade required"
        );
    }
}
