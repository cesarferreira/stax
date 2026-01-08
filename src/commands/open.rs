use crate::config::Config;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;

/// Open the repository in the default browser
pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    let repo_url = remote_info.repo_url();

    println!("Opening {} in browser...", repo_url.cyan());

    // Open in default browser
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&repo_url)
            .spawn()
            .ok();
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&repo_url)
            .spawn()
            .ok();
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", &repo_url])
            .spawn()
            .ok();
    }

    Ok(())
}
