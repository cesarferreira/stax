use super::shared::{
    build_launch_spec, default_create_base, derive_unique_worktree_name, emit_shell_payload,
    ensure_gitignore, ensure_managed_worktrees_root, find_worktree, format_create_message,
    format_go_message, generate_random_lane_slug, managed_worktrees_dir, pick_branch_interactively,
    resolve_branch_name, run_blocking_hook, spawn_background_hook, LaunchOptions,
};
use crate::commands::shell_setup;
use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{bail, Context, Result};
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
    args: Vec<String>,
) -> Result<()> {
    if pick && name.is_some() {
        bail!("Use either a name or --pick, not both.");
    }

    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let launch = build_launch_spec(
        &config,
        &LaunchOptions {
            agent,
            model,
            run,
            args,
        },
    )?;

    if let Some(ref target) = name {
        if let Some(worktree) = find_worktree(&repo, target)? {
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
                    println!("  {}", "stax shell-setup --install".cyan());
                }
            }

            return Ok(());
        }
    }

    let input_name = match (pick, name) {
        (true, _) => pick_branch_interactively(&repo)?,
        (false, Some(name)) => name,
        (false, None) => generate_random_lane_slug(&repo, &config)?,
    };

    let (branch_name, branch_exists) = resolve_branch_name(&repo, &config, &input_name)?;
    if let Some(worktree) = find_worktree(&repo, &branch_name)? {
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
                println!("  {}", "stax shell-setup --install".cyan());
            }
        }
        return Ok(());
    }

    let base_branch = if branch_exists {
        None
    } else {
        let base_branch = from.unwrap_or(default_create_base(&repo)?);
        repo.branch_commit(&base_branch)
            .with_context(|| format!("Base branch '{}' does not exist", base_branch))?;
        Some(base_branch)
    };

    let worktree_name = worktree_name.unwrap_or(derive_unique_worktree_name(&repo, &branch_name)?);
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
    ensure_gitignore(&main_repo_workdir, &config.worktree.root_dir)?;

    if branch_exists {
        repo.worktree_create(&branch_name, &worktree_path)?;
    } else {
        let from_branch = base_branch
            .as_deref()
            .expect("base branch is always set for a new branch");
        repo.worktree_create_new_branch(&branch_name, &worktree_path, &from_branch)?;
        let parent_rev = repo.branch_commit(&from_branch)?;
        let meta = BranchMetadata::new(&from_branch, &parent_rev);
        meta.write(repo.inner(), &branch_name)?;
    }

    let copied_files = repo.tracked_file_count_at(&worktree_path).unwrap_or(0);
    let repo_name = main_repo_workdir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string());
    let from_label = if let Some(base_branch) = base_branch.as_deref() {
        if repo.has_remote(base_branch) {
            format!("origin/{}", base_branch)
        } else {
            base_branch.to_string()
        }
    } else {
        branch_name.clone()
    };
    format_create_message(
        &repo_name,
        &worktree_name,
        &branch_name,
        &from_label,
        copied_files,
        branch_exists,
    );

    if !no_verify {
        run_blocking_hook(
            config.worktree.hooks.post_create.as_deref(),
            &worktree_path,
            "post_create",
        )?;
        spawn_background_hook(
            config.worktree.hooks.post_start.as_deref(),
            &worktree_path,
            "post_start",
        )?;
    }

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
            println!("  {}", "stax shell-setup --install".cyan());
        }
    }

    Ok(())
}
