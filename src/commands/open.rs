use crate::config::Config;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Longest we wait for the launcher to report an early failure before assuming it worked.
const BROWSER_LAUNCH_WAIT: Duration = Duration::from_millis(1500);
const BROWSER_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Internal override for integration tests: a program path run with the URL as its only
/// argument. Not a supported user setting.
const BROWSER_COMMAND_ENV: &str = "STAX_BROWSER_COMMAND";

fn launcher_command(url: &str) -> Command {
    if let Some(program) = std::env::var_os(BROWSER_COMMAND_ENV).filter(|v| !v.is_empty()) {
        let mut cmd = Command::new(program);
        cmd.arg(url);
        return cmd;
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(url);
        cmd
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/c", "start", url]);
        cmd
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(url);
        cmd
    }
}

fn run_launcher(mut cmd: Command, wait: Duration) -> std::io::Result<()> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(std::io::Error::other(format!(
                    "browser launcher exited with {status}"
                )));
            }
            None if started.elapsed() >= wait => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            None => std::thread::sleep(BROWSER_POLL_INTERVAL),
        }
    }
}

/// Open `url` in the platform default browser without letting launcher output reach the
/// terminal. Waits up to 1.5s for an early failure; a launcher still running after that
/// counts as success.
pub fn try_open_url_in_browser(url: &str) -> std::io::Result<()> {
    run_launcher(launcher_command(url), BROWSER_LAUNCH_WAIT)
}

pub fn browser_open_failed_message(url: &str) -> String {
    format!("Couldn't open a browser. Open this link manually: {url}")
}

/// Open `url` in the default browser. On failure prints a single warning with the link.
pub fn open_url_in_browser(url: &str) {
    if try_open_url_in_browser(url).is_err() {
        eprintln!("{} {}", "⚠".yellow(), browser_open_failed_message(url));
    }
}

/// Open the repository in the default browser
pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    let repo_url = remote_info.repo_url();

    println!("Opening {} in browser...", repo_url.cyan());

    open_url_in_browser(&repo_url);

    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn run_launcher_ok_when_launcher_exits_zero() {
        assert!(run_launcher(Command::new("true"), Duration::from_secs(2)).is_ok());
    }

    #[test]
    fn run_launcher_err_when_launcher_exits_nonzero() {
        assert!(run_launcher(Command::new("false"), Duration::from_secs(2)).is_err());
    }

    #[test]
    fn run_launcher_err_when_launcher_missing() {
        let cmd = Command::new("/nonexistent/stax-browser");
        assert!(run_launcher(cmd, Duration::from_secs(2)).is_err());
    }

    #[test]
    fn run_launcher_ok_when_launcher_still_running_after_window() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 5"]);
        let started = Instant::now();
        assert!(run_launcher(cmd, Duration::from_millis(100)).is_ok());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn browser_open_failed_message_contains_url() {
        assert_eq!(
            browser_open_failed_message("https://x.test/a"),
            "Couldn't open a browser. Open this link manually: https://x.test/a"
        );
    }
}
