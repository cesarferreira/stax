use colored::Colorize;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use update_informer::{registry, Check};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

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

/// Spawn a background thread to check for updates.
/// This is non-blocking and won't affect CLI performance.
/// Results are cached by update-informer for 24 hours.
pub fn check_in_background() {
    if update_checks_disabled() {
        return;
    }

    thread::spawn(|| {
        let informer = update_informer::new(registry::Crates, PKG_NAME, PKG_VERSION)
            .timeout(Duration::from_secs(3))
            .interval(Duration::from_secs(60 * 60 * 24)); // 24 hours

        // This will either use cached result or make a network request
        // The result is cached for the next run
        let _ = informer.check_version();
    });
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
    let path = path.to_string_lossy();
    if path.contains("/homebrew/") || path.contains("/Cellar/") {
        InstallMethod::Homebrew
    } else if path.contains(".cargo/bin") {
        if binstall_metadata_contains_stax(cargo_home) {
            InstallMethod::CargoBinstall
        } else {
            InstallMethod::Cargo
        }
    } else {
        InstallMethod::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let temp = tempfile::tempdir().expect("temp cargo home");
        let metadata_dir = temp.path().join("binstall");
        fs::create_dir_all(&metadata_dir).expect("metadata dir");
        fs::write(
            metadata_dir.join("crates-v1.json"),
            r#"{"name":"other"}{"name":"stax"}"#,
        )
        .expect("metadata");

        let path = "/home/user/.cargo/bin/stax";
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
