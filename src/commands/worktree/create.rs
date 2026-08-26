use super::shared::{
    LaunchOptions, adopt_or_create_worktree, build_launch_spec, default_create_base,
    derive_unique_worktree_name, emit_shell_payload, ensure_gitignore,
    ensure_managed_worktrees_root, find_worktree, format_create_message, format_go_message,
    generate_random_lane_slug, managed_worktrees_dir, pick_branch_interactively,
    resolve_branch_name, run_post_create_setup, spawn_background_hook,
};
use crate::commands::shell_setup;
use crate::config::Config;
use crate::git::GitRepo;
use anyhow::{Context, Result, bail};
use colored::Colorize;
use std::fs;

#[allow(clippy::too_many_arguments)]
pub fn run(
    name: Option<String>,
    from: Option<String>,
    pick: bool,
    worktree_name: Option<String>,
    no_verify: bool,
    shell_output: bool,
    agent: Option<String>,
    model: Option<String>,
    run: Option<String>,
    tmux: bool,
    tmux_session: Option<String>,
    args: Vec<String>,
    yolo: bool,
    agent_args: Vec<String>,
) -> Result<()> {
    if pick && name.is_some() {
        bail!("Use either a name or --pick, not both.");
    }

    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let launch_options = LaunchOptions {
        agent,
        model,
        run,
        tmux,
        tmux_session,
        args,
        yolo,
        agent_args,
    };

    if let Some(ref target) = name
        && let Some(worktree) = find_worktree(&repo, target)?
    {
        let launch = build_launch_spec(&config, &launch_options, &worktree.name)?;
        format_go_message(&worktree);
        if !no_verify {
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
                println!("  {}", "stax setup".cyan());
            }
        }

        return Ok(());
    }

    let input_name = match (pick, name) {
        (true, _) => pick_branch_interactively(&repo)?,
        (false, Some(name)) => name,
        (false, None) => generate_random_lane_slug(&repo, &config)?,
    };

    let resolved_branch = resolve_branch_name(&repo, &config, &input_name)?;
    let branch_name = resolved_branch.name.clone();
    if let Some(worktree) = find_worktree(&repo, &branch_name)? {
        let launch = build_launch_spec(&config, &launch_options, &worktree.name)?;
        format_go_message(&worktree);
        if !no_verify {
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
                println!("  {}", "stax setup".cyan());
            }
        }
        return Ok(());
    }

    let base_branch = if resolved_branch.needs_base_branch() {
        let base_branch = from.unwrap_or(default_create_base(&repo)?);
        repo.branch_commit(&base_branch)
            .with_context(|| format!("Base branch '{}' does not exist", base_branch))?;
        Some(base_branch)
    } else {
        None
    };

    let worktree_name = worktree_name.unwrap_or(derive_unique_worktree_name(&repo, &branch_name)?);
    let launch = build_launch_spec(&config, &launch_options, &worktree_name)?;
    let worktrees_dir = managed_worktrees_dir(&repo, &config)?;
    let worktree_path = worktrees_dir.join(&worktree_name);
    if worktree_path.exists() {
        bail!(
            "Worktree path '{}' already exists.",
            worktree_path.display()
        );
    }

    fs::create_dir_all(&worktrees_dir)?;
    ensure_managed_worktrees_root(&repo, &config, &worktrees_dir)?;
    let main_repo_workdir = repo.main_repo_workdir()?;
    ensure_gitignore(&main_repo_workdir, &worktrees_dir)?;

    let outcome = adopt_or_create_worktree(
        &repo,
        &config,
        &resolved_branch,
        &worktree_path,
        base_branch.as_deref(),
        &worktrees_dir,
    )?;
    let worktree_path = outcome.path().to_path_buf();

    let repo_name = main_repo_workdir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string());
    let from_label = resolved_branch.source_label(&repo, base_branch.as_deref());
    format_create_message(
        &repo_name,
        &worktree_name,
        &branch_name,
        &from_label,
        resolved_branch.is_existing(),
    );

    run_post_create_setup(&config, &main_repo_workdir, &worktree_path, no_verify)?;

    if shell_output {
        emit_shell_payload(&worktree_path, launch.as_ref());
    } else if let Some(launch) = launch.as_ref() {
        launch.execute_in(&worktree_path)?;
    } else {
        println!();
        println!("{}", "Current shell did not move automatically.".yellow());
        println!("  {}", format!("cd {}", worktree_path.display()).cyan());
        if !shell_setup::is_installed() {
            println!();
            println!(
                "{}",
                "Tip: add shell integration for automatic cd:".dimmed()
            );
            println!("  {}", "stax setup".cyan());
        }
    }

    Ok(())
}
