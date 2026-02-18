use crate::commands;
use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;
use std::process::Command;

pub fn run(no_pr: bool, no_submit: bool, auto_stash_pop: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let original = repo.current_branch()?;

    println!("{}", "Cascading stack...".bold());

    // Warn if local trunk is behind its remote-tracking ref. This uses the
    // cached remote refs (no network call) so it's instant. The user can run
    // `stax rs` to fetch and sync trunk before cascading.
    warn_if_trunk_stale(&repo);

    commands::navigate::bottom()?;
    commands::restack::run(false, false, true, auto_stash_pop)?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    commands::upstack::restack::run(auto_stash_pop)?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    if no_submit {
        println!("{}", "Skipping push and PRs (--no-submit)".dimmed());
    } else {
        commands::submit::run(
            commands::submit::SubmitScope::Stack,
            false,  // draft
            no_pr,  // no_pr (push but skip PR creation/updates)
            false,  // force
            true,   // yes
            true,   // no_prompt
            vec![], // reviewers
            vec![], // labels
            vec![], // assignees
            false,  // quiet
            false,  // verbose
            None,   // template
            false,  // no_template
            false,  // edit
            false,  // ai_body
        )?;
    }

    if !repo.rebase_in_progress()? && repo.current_branch()? != original {
        repo.checkout(&original)?;
    }

    Ok(())
}

/// Check whether local trunk is behind its remote-tracking ref and print a
/// warning if so. Uses the cached remote refs — no network call. Non-fatal:
/// the user may intentionally be working offline or not ready to sync yet.
fn warn_if_trunk_stale(repo: &GitRepo) {
    let Ok(config) = Config::load() else { return };
    let Ok(stack) = Stack::load(repo) else { return };
    let Ok(workdir) = repo.workdir() else { return };

    let remote_ref = format!("{}/{}", config.remote_name(), stack.trunk);

    // Count commits on remote that aren't in local trunk.
    // git rev-list --count <local>..<remote> — uses only local git objects.
    let output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", stack.trunk, remote_ref)])
        .current_dir(workdir)
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let count_str = String::from_utf8_lossy(&out.stdout);
            let count: u64 = count_str.trim().parse().unwrap_or(0);
            if count > 0 {
                println!(
                    "  {} {} is {} commit{} behind {} — run {} to sync first",
                    "warning:".yellow().bold(),
                    stack.trunk.cyan(),
                    count.to_string().yellow(),
                    if count == 1 { "" } else { "s" },
                    remote_ref.cyan(),
                    "stax rs".bold(),
                );
            }
        }
        // If rev-list fails (e.g. remote ref doesn't exist yet), silently skip.
    }
}
