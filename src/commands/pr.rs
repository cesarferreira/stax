use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;

/// Open the PR for the current branch in the default browser
pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;

    // Check if branch is tracked
    let branch_info = stack.branches.get(&current);
    if branch_info.is_none() {
        anyhow::bail!(
            "Branch '{}' is not tracked. Use {} to track it first.",
            current,
            "stax branch track".cyan()
        );
    }

    // Check if branch has a PR
    let pr_number = branch_info.and_then(|b| b.pr_number);
    if pr_number.is_none() {
        anyhow::bail!(
            "No PR found for branch '{}'. Use {} to create one.",
            current,
            "stax submit".cyan()
        );
    }

    let pr_number = pr_number.unwrap();
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    let pr_url = remote_info.pr_url(pr_number);

    println!("Opening {} in browser...", pr_url.cyan());

    // Open in default browser
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&pr_url)
            .spawn()
            .ok();
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&pr_url)
            .spawn()
            .ok();
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", &pr_url])
            .spawn()
            .ok();
    }

    Ok(())
}
