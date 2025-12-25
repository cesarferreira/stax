use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::remote;
use anyhow::Result;
use colored::Colorize;

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
        println!("{} {}", "✗".red(), "Repo not initialized (run `stax` once)".yellow());
        issues += 1;
    }

    match repo.trunk_branch() {
        Ok(trunk) => println!(
            "{} {} {}",
            "✓".green(),
            "Trunk:".dimmed(),
            trunk.cyan()
        ),
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
            println!(
                "{} {} {}",
                "✗".red(),
                "Remote missing:".yellow(),
                err
            );
            issues += 1;
        }
    }

    let provider = config.remote_provider();
    println!(
        "{} {} {}",
        "✓".green(),
        "Provider:".dimmed(),
        provider.cyan()
    );

    if provider.eq_ignore_ascii_case("github") {
        if Config::github_token().is_some() {
            println!("{} {}", "✓".green(), "GitHub token set".dimmed());
        } else {
            println!(
                "{} {}",
                "⚠".yellow(),
                "GitHub token missing (PR creation disabled)".yellow()
            );
        }
    } else {
        println!(
            "{} {}",
            "⚠".yellow(),
            "Non-GitHub provider: PR creation is disabled".yellow()
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
                format!("{} branch(es) need restack", needs_restack.len()).yellow()
            );
        }
    }

    println!();
    if issues == 0 {
        println!("{}", "✓ Doctor check complete (no critical issues)".green());
    } else {
        println!(
            "{}",
            format!("✗ Doctor found {} issue(s)", issues).yellow()
        );
    }

    Ok(())
}
