use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::forge;
use crate::git::{refs, GitRepo};
use crate::remote::{self, RemoteInfo};
use anyhow::Result;
use colored::Colorize;
use std::process::Command;

pub fn run() -> Result<()> {
    println!("{}", "stax doctor".bold());
    println!();

    let repo = match GitRepo::open() {
        Ok(repo) => repo,
        Err(err) => {
            println!("{} {}", "✗".red(), err);
            return Ok(());
        }
    };

    let config = Config::load()?;
    let mut issues = 0;

    if repo.is_initialized() {
        println!("{} {}", "✓".green(), "Repo initialized".dimmed());
    } else {
        println!(
            "{} {}",
            "✗".red(),
            "Repo not initialized (run `stax` once)".yellow()
        );
        issues += 1;
    }

    match repo.trunk_branch() {
        Ok(trunk) => println!("{} {} {}", "✓".green(), "Trunk:".dimmed(), trunk.cyan()),
        Err(err) => {
            println!("{} {} {}", "✗".red(), "Trunk not set:".yellow(), err);
            issues += 1;
        }
    }

    let remote_name = config.remote_name();
    match remote::get_remote_url(repo.workdir()?, remote_name) {
        Ok(url) => println!(
            "{} {} {}",
            "✓".green(),
            "Remote:".dimmed(),
            format!("{} ({})", remote_name, url).cyan()
        ),
        Err(err) => {
            println!("{} {} {}", "✗".red(), "Remote missing:".yellow(), err);
            issues += 1;
        }
    }

    let remote_info = RemoteInfo::from_repo(&repo, &config).ok();
    let forge_label = remote_info
        .as_ref()
        .map(|info| info.forge.to_string())
        .unwrap_or_else(|| "Forge".to_string());

    let has_token = remote_info
        .as_ref()
        .map(|info| forge::forge_token(info.forge).is_some())
        .unwrap_or_else(|| Config::github_token().is_some());

    if has_token {
        println!(
            "{} {}",
            "✓".green(),
            format!("{} API token available", forge_label).dimmed()
        );
    } else {
        println!(
            "{} {}",
            "⚠".yellow(),
            format!(
                "{} API token missing (run `stax auth` — needed for PR/submit against this remote)",
                forge_label
            )
            .yellow()
        );
    }

    if repo.is_dirty()? {
        println!("{} {}", "⚠".yellow(), "Working tree is dirty".yellow());
    } else {
        println!("{} {}", "✓".green(), "Working tree clean".dimmed());
    }

    if repo.rebase_in_progress()? {
        println!(
            "{} {}",
            "⚠".yellow(),
            "Rebase in progress (run `stax continue`)".yellow()
        );
    }

    if let Ok(stack) = Stack::load(&repo) {
        let mut orphaned = Vec::new();
        for (name, info) in &stack.branches {
            if let Some(parent) = &info.parent {
                if repo.branch_commit(parent).is_err() {
                    orphaned.push((name.clone(), parent.clone()));
                }
            }
        }

        if !orphaned.is_empty() {
            issues += 1;
            println!(
                "{} {}",
                "✗".red(),
                "Branches with missing parents:".yellow()
            );
            for (branch, parent) in orphaned {
                println!("  {} → {}", branch, parent);
            }
        }

        let needs_restack = stack.needs_restack();
        if !needs_restack.is_empty() {
            println!(
                "{} {}",
                "⚠".yellow(),
                format!(
                    "{} {} need restack",
                    needs_restack.len(),
                    if needs_restack.len() == 1 {
                        "branch"
                    } else {
                        "branches"
                    }
                )
                .yellow()
            );
        }
    }

    // Check: diverged trunk detection
    if let Ok(trunk) = repo.trunk_branch() {
        let remote_trunk = format!("{}/{}", remote_name, trunk);
        match repo.is_ancestor(&trunk, &remote_trunk) {
            Ok(true) => {
                println!(
                    "{} {}",
                    "✓".green(),
                    "Local trunk is ancestor of remote trunk".dimmed()
                );
            }
            Ok(false) => {
                println!(
                    "{} {}",
                    "⚠".yellow(),
                    format!(
                        "Local {} has diverged from {}/{} (remote may have been force-pushed)",
                        trunk, remote_name, trunk
                    )
                    .yellow()
                );
            }
            Err(_) => {
                // Remote trunk ref may not exist (e.g., never fetched); skip silently
            }
        }
    }

    // Check: git config recommendations for stacked workflows
    {
        let rerere_ok = git_config_is_true(repo.workdir().ok(), "rerere.enabled");
        let autostash_ok = git_config_is_true(repo.workdir().ok(), "rebase.autoStash");

        if rerere_ok && autostash_ok {
            println!(
                "{} {}",
                "✓".green(),
                "Git config: rerere.enabled and rebase.autoStash are set".dimmed()
            );
        } else {
            let mut missing = Vec::new();
            if !rerere_ok {
                missing.push("rerere.enabled");
            }
            if !autostash_ok {
                missing.push("rebase.autoStash");
            }
            println!(
                "{} {}",
                "⚠".yellow(),
                format!(
                    "Recommended git config not set: {}. Run: {}",
                    missing.join(", "),
                    missing
                        .iter()
                        .map(|k| format!("git config --global {} true", k))
                        .collect::<Vec<_>>()
                        .join(" && ")
                )
                .yellow()
            );
        }
    }

    // Check: stale PR metadata (OPEN PR on a branch that no longer exists locally)
    {
        let local_branches: std::collections::HashSet<String> =
            repo.list_branches().unwrap_or_default().into_iter().collect();
        let metadata_branches = refs::list_metadata_branches(repo.inner()).unwrap_or_default();
        let mut stale = Vec::new();

        for branch_name in &metadata_branches {
            if local_branches.contains(branch_name) {
                continue;
            }
            if let Ok(Some(meta)) = BranchMetadata::read(repo.inner(), branch_name) {
                if let Some(pr) = &meta.pr_info {
                    if pr.state == "OPEN" {
                        stale.push((branch_name.clone(), pr.number));
                    }
                }
            }
        }

        if stale.is_empty() {
            println!(
                "{} {}",
                "✓".green(),
                "No stale PR metadata found".dimmed()
            );
        } else {
            println!(
                "{} {}",
                "⚠".yellow(),
                format!(
                    "{} branch(es) have OPEN PR metadata but no local branch:",
                    stale.len()
                )
                .yellow()
            );
            for (branch, pr_num) in &stale {
                println!("  {} (PR #{})", branch, pr_num);
            }
        }
    }

    println!();
    if issues == 0 {
        println!("{}", "✓ Doctor check complete (no critical issues)".green());
    } else {
        println!("{}", format!("✗ Doctor found {} issue(s)", issues).yellow());
    }

    Ok(())
}

/// Check whether a git config key is set to "true".
fn git_config_is_true(workdir: Option<&std::path::Path>, key: &str) -> bool {
    let mut cmd = Command::new("git");
    cmd.args(["config", "--get", key]);
    if let Some(cwd) = workdir {
        cmd.current_dir(cwd);
    }
    match cmd.output() {
        Ok(output) if output.status.success() => {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
            value == "true"
        }
        _ => false,
    }
}
