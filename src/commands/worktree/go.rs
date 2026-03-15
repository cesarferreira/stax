use super::shared::{
    build_launch_spec, emit_shell_payload, find_worktree, format_go_message,
    pick_worktree_interactively, run_blocking_hook, spawn_background_hook, LaunchOptions,
};
use crate::commands::shell_setup;
use crate::config::Config;
use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;

pub fn run_path(name: &str) -> Result<()> {
    let repo = GitRepo::open()?;
    let worktree = find_worktree(&repo, name)?
        .ok_or_else(|| anyhow::anyhow!("No worktree named '{}'", name))?;
    println!("{}", worktree.path.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_go(
    name: Option<String>,
    no_verify: bool,
    shell_output: bool,
    agent: Option<String>,
    model: Option<String>,
    run: Option<String>,
    tmux: bool,
    tmux_session: Option<String>,
    args: Vec<String>,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let worktree = match name {
        Some(name) => find_worktree(&repo, &name)?
            .ok_or_else(|| anyhow::anyhow!("No worktree named '{}'", name))?,
        None => pick_worktree_interactively(&repo)?,
    };

    let launch = build_launch_spec(
        &config,
        &LaunchOptions {
            agent,
            model,
            run,
            tmux,
            tmux_session,
            args,
        },
        &worktree.name,
    )?;

    if !worktree.path.exists() {
        bail!(
            "Worktree path '{}' does not exist. Run `stax worktree prune`.",
            worktree.path.display()
        );
    }

    format_go_message(&worktree);

    if !no_verify {
        run_blocking_hook(None, &worktree.path, "pre_go")?;
        spawn_background_hook(
            config.worktree.hooks.post_go.as_deref(),
            &worktree.path,
            "post_go",
        )?;
    }

    if shell_output {
        emit_shell_payload(&worktree.path, launch.as_ref());
    } else if let Some(launch) = launch.as_ref() {
        launch.execute_in(&worktree.path)?;
    } else {
        println!();
        println!("{}", "Current shell did not move automatically.".yellow());
        println!("  {}", format!("cd {}", worktree.path.display()).cyan());

        if !shell_setup::is_installed() {
            println!();
            println!(
                "{}",
                "Tip: add shell integration for automatic cd:".dimmed()
            );
            println!("  {}", "stax shell-setup --install".cyan());
        }
    }

    Ok(())
}
