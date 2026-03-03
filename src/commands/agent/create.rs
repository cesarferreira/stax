use super::registry::{AgentWorktree, Registry};
use super::util::{ensure_gitignore, open_in_editor, resolve_editor, slugify};
use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::fs;
use std::process::Command;

pub fn run(
    title: String,
    base: Option<String>,
    stack_on: Option<String>,
    open_editor: bool,
    open_cursor: bool,
    open_codex: bool,
    no_hook: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let workdir = repo.workdir()?.to_path_buf();
    let git_dir = repo.git_dir()?.to_path_buf();
    let current = repo.current_branch()?;

    let parent_branch = stack_on.or(base).unwrap_or_else(|| current.clone());

    if repo.branch_commit(&parent_branch).is_err() {
        bail!("Branch '{}' does not exist", parent_branch);
    }

    let slug = slugify(&title);
    if slug.is_empty() {
        bail!(
            "Title '{}' produces an empty slug — use alphanumeric characters",
            title
        );
    }

    let branch_name = config.format_branch_name(&title);

    if repo.branch_commit(&branch_name).is_ok() {
        bail!(
            "Branch '{}' already exists. Use a different title.",
            branch_name
        );
    }

    let worktrees_dir = workdir.join(&config.agent.worktrees_dir);
    let worktree_path = worktrees_dir.join(&slug);

    if worktree_path.exists() {
        bail!(
            "Worktree path '{}' already exists.",
            worktree_path.display()
        );
    }

    fs::create_dir_all(&worktrees_dir).with_context(|| {
        format!(
            "Failed to create worktrees directory: {}",
            worktrees_dir.display()
        )
    })?;

    ensure_gitignore(&workdir, &config.agent.worktrees_dir)?;

    let parent_rev = repo.branch_commit(&parent_branch)?;

    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            &branch_name,
            worktree_path.to_str().context("Non-UTF-8 worktree path")?,
            &parent_branch,
        ])
        .current_dir(&workdir)
        .output()
        .context("Failed to run git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("git worktree add failed: {}", stderr);
    }

    let meta = BranchMetadata::new(&parent_branch, &parent_rev);
    meta.write(repo.inner(), &branch_name)?;

    let mut registry = Registry::load(&git_dir)?;
    registry.add(AgentWorktree {
        name: slug.clone(),
        branch: branch_name.clone(),
        path: worktree_path.clone(),
        created_at: chrono::Local::now().to_rfc3339(),
    });
    registry.save()?;

    println!(
        "{}  worktree '{}' → branch '{}'",
        "Created".green().bold(),
        slug.cyan(),
        branch_name.blue()
    );
    println!("  Path:   {}", worktree_path.display().to_string().dimmed());
    println!("  Parent: {}", parent_branch.dimmed());

    if !no_hook {
        if let Some(hook) = &config.agent.post_create_hook {
            if !hook.is_empty() {
                println!("\nRunning post-create hook...");
                let status = Command::new("sh")
                    .args(["-c", hook])
                    .current_dir(&worktree_path)
                    .status()
                    .context("Failed to run post_create_hook")?;
                if !status.success() {
                    eprintln!("{}", "  Post-create hook failed (continuing)".yellow());
                }
            }
        }
    }

    let editor_cmd = resolve_editor(&config, open_cursor, open_codex, open_editor);
    if let Some(cmd) = editor_cmd {
        open_in_editor(&cmd, &worktree_path)?;
    } else {
        println!("\n  Tip: {}", format!("stax agent open {}", slug).cyan());
    }

    Ok(())
}
