use colored::Colorize;
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

/// Detect how stax was installed based on binary path
fn detect_install_method() -> InstallMethod {
    match std::env::current_exe() {
        Ok(path) => install_method_from_path(&path.to_string_lossy()),
        Err(_) => InstallMethod::Cargo, // Default fallback
    }
}

enum InstallMethod {
    Cargo,
    Homebrew,
    Unknown,
}

impl InstallMethod {
    fn upgrade_command(&self) -> &'static str {
        match self {
            InstallMethod::Cargo => "cargo install stax",
            InstallMethod::Homebrew => "brew upgrade stax",
            InstallMethod::Unknown => "upgrade stax",
        }
    }
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
fn install_method_from_path(path: &str) -> InstallMethod {
    if path.contains("/homebrew/") || path.contains("/Cellar/") {
        InstallMethod::Homebrew
    } else if path.contains(".cargo/bin") {
        InstallMethod::Cargo
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
            install_method_from_path(path),
            InstallMethod::Homebrew
        ));
    }

    #[test]
    fn test_detect_homebrew_cellar_arm() {
        let path = "/opt/homebrew/Cellar/stax/0.5.0/bin/stax";
        assert!(matches!(
            install_method_from_path(path),
            InstallMethod::Homebrew
        ));
    }

    #[test]
    fn test_detect_homebrew_intel() {
        let path = "/usr/local/Cellar/stax/0.5.0/bin/stax";
        assert!(matches!(
            install_method_from_path(path),
            InstallMethod::Homebrew
        ));
    }

    #[test]
    fn test_detect_cargo() {
        let path = "/Users/cesar/.cargo/bin/stax";
        assert!(matches!(
            install_method_from_path(path),
            InstallMethod::Cargo
        ));
    }

    #[test]
    fn test_detect_cargo_linux() {
        let path = "/home/user/.cargo/bin/stax";
        assert!(matches!(
            install_method_from_path(path),
            InstallMethod::Cargo
        ));
    }

    #[test]
    fn test_detect_unknown_usr_local_bin() {
        let path = "/usr/local/bin/stax";
        assert!(matches!(
            install_method_from_path(path),
            InstallMethod::Unknown
        ));
    }

    #[test]
    fn test_detect_unknown_custom_path() {
        let path = "/opt/mytools/stax";
        assert!(matches!(
            install_method_from_path(path),
            InstallMethod::Unknown
        ));
    }

    #[test]
    fn test_upgrade_command_cargo() {
        assert_eq!(InstallMethod::Cargo.upgrade_command(), "cargo install stax");
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
        assert_eq!(InstallMethod::Unknown.upgrade_command(), "upgrade stax");
    }
}
