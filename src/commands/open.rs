use crate::config::Config;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;

/// Open `url` in the platform default browser. Logs a warning if the launcher fails.
pub fn open_url_in_browser(url: &str) {
    let spawn_result: std::io::Result<()> = (|| {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open").arg(url).spawn()?;
        }
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open").arg(url).spawn()?;
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/c", "start", url])
                .spawn()?;
        }
        Ok(())
    })();

    if let Err(e) = spawn_result {
        eprintln!(
            "{} Could not open browser ({}). URL: {}",
            "⚠".yellow(),
            e,
            url
        );
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
