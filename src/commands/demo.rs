use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::path::Path;
use std::process::Command;

fn run_git(cwd: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr);
    }
    Ok(())
}

fn run_stax(cwd: &Path, args: &[&str]) -> Result<String> {
    let exe = std::env::current_exe().unwrap_or_else(|_| "stax".into());
    let output = Command::new(exe)
        .args(args)
        .current_dir(cwd)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .with_context(|| format!("Failed to run stax {}", args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if !stdout.is_empty() {
        print!("{}", stdout);
    }
    Ok(stdout)
}

fn pause(message: &str) -> Result<bool> {
    println!();
    let cont = dialoguer::Confirm::new()
        .with_prompt(message)
        .default(true)
        .interact()
        .unwrap_or(false);
    println!();
    Ok(cont)
}

fn step_header(step: usize, total: usize, title: &str) {
    println!(
        "{}  {}",
        format!("[{}/{}]", step, total).bold().blue(),
        title.bold()
    );
    println!();
}

fn print_command(cmd: &str) {
    println!("  {} {}", "$".dimmed(), cmd.cyan());
    println!();
}

/// Set up a fresh temp repo with stax initialized
fn setup_demo_repo() -> Result<(tempfile::TempDir, std::path::PathBuf)> {
    let tmp = tempfile::tempdir().context("Failed to create temp directory")?;
    let dir = tmp.path().to_path_buf();

    run_git(&dir, &["init", "-b", "main"])?;
    run_git(&dir, &["config", "user.email", "demo@stax.dev"])?;
    run_git(&dir, &["config", "user.name", "Stax Demo"])?;

    fs::write(dir.join("README.md"), "# My Project\n")?;
    run_git(&dir, &["add", "-A"])?;
    run_git(&dir, &["commit", "-m", "Initial commit"])?;

    // Initialize stax metadata
    run_stax(&dir, &["doctor"])?;

    Ok((tmp, dir))
}

// ─── Demo 1: Your first pull request ────────────────────────────────────────

fn demo_first_pr() -> Result<()> {
    let total = 6;
    println!();
    println!(
        "{}",
        "Demo: Creating your first pull request".bold().green()
    );
    println!(
        "{}",
        "This demo walks you through creating a branch, making changes,"
            .dimmed()
    );
    println!(
        "{}",
        "and preparing a pull request with stax.".dimmed()
    );
    println!();

    let (_tmp, dir) = setup_demo_repo()?;

    // Step 1: Current state
    step_header(1, total, "Your repo starts on trunk (main)");
    println!("Every stax workflow starts from your trunk branch.");
    println!("Let's see the current state:");
    print_command("stax status");
    run_stax(&dir, &["status"])?;

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 2: Create a branch
    step_header(2, total, "Create a new branch");
    println!("Create a branch stacked on top of main.");
    println!("This is like `git checkout -b`, but stax tracks the parent relationship.");
    print_command("stax create add-login");
    run_stax(&dir, &["create", "add-login"])?;

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 3: Make changes and commit
    step_header(3, total, "Make changes and commit");
    println!("Add some code and commit — just like normal git:");
    println!();
    println!(
        "  {}",
        "echo 'fn login() { ... }' > login.rs".dimmed()
    );
    println!("  {} {}", "$".dimmed(), "git add -A && git commit -m \"Add login function\"".cyan());
    println!();
    fs::write(dir.join("login.rs"), "fn login() {\n    // TODO: implement\n}\n")?;
    run_git(&dir, &["add", "-A"])?;
    run_git(&dir, &["commit", "-m", "Add login function"])?;

    println!("Now let's see the stack:");
    print_command("stax status");
    run_stax(&dir, &["status"])?;

    println!(
        "{}",
        "Your branch is tracked and shows its relationship to main.".dimmed()
    );

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 4: View the log
    step_header(4, total, "View your stack with commit details");
    println!("The log command shows commits for each branch:");
    print_command("stax log");
    run_stax(&dir, &["log"])?;

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 5: Submit (simulated)
    step_header(5, total, "Submit your pull request");
    println!("In a real repo with GitHub configured, running:");
    print_command("stax submit");
    println!("would push your branch and create a PR with the correct base branch.");
    println!();
    println!("For stacks with multiple branches, stax creates one PR per branch,");
    println!("each targeting the correct parent — no manual base-branch fiddling.");

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 6: Wrap up
    step_header(6, total, "You're ready!");
    println!("You've learned the basics:");
    println!();
    println!("  {} - create a stacked branch", "stax create <name>".cyan());
    println!("  {} - see the stack tree", "stax status".cyan());
    println!("  {} - see commits per branch", "stax log".cyan());
    println!("  {} - push and create/update PRs", "stax submit".cyan());
    println!();
    println!(
        "{}",
        "Next: try the \"Creating a stack\" demo to learn multi-branch workflows."
            .dimmed()
    );
    println!();

    Ok(())
}

// ─── Demo 2: Creating a stack of pull requests ──────────────────────────────

fn demo_stacked_prs() -> Result<()> {
    let total = 8;
    println!();
    println!(
        "{}",
        "Demo: Creating a stack of pull requests".bold().green()
    );
    println!(
        "{}",
        "This demo shows the full stacked-branch workflow: create multiple"
            .dimmed()
    );
    println!(
        "{}",
        "branches, navigate between them, and keep them in sync.".dimmed()
    );
    println!();

    let (_tmp, dir) = setup_demo_repo()?;

    // Step 1: Plan
    step_header(1, total, "Plan your stack");
    println!("Imagine you're building a user dashboard. You'll break it into:");
    println!();
    println!("  {} Add the data models", "1.".bold());
    println!("  {} Add the API endpoints", "2.".bold());
    println!("  {} Add the dashboard UI", "3.".bold());
    println!();
    println!("Each becomes a branch stacked on the previous one.");
    println!("Reviewers see small, focused PRs instead of one giant diff.");

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 2: First branch - models
    step_header(2, total, "Create the first branch");
    print_command("stax create add-models");
    run_stax(&dir, &["create", "add-models"])?;

    fs::write(
        dir.join("models.rs"),
        "pub struct User {\n    pub id: u64,\n    pub name: String,\n}\n",
    )?;
    run_git(&dir, &["add", "-A"])?;
    run_git(&dir, &["commit", "-m", "Add User model"])?;
    println!("Added a commit with the data models.");

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 3: Second branch - API
    step_header(3, total, "Stack a second branch on top");
    println!(
        "While still on {}, create the next branch:",
        "add-models".cyan()
    );
    print_command("stax create add-api");
    run_stax(&dir, &["create", "add-api"])?;

    fs::write(
        dir.join("api.rs"),
        "pub fn get_user(id: u64) -> User {\n    // fetch from database\n    todo!()\n}\n",
    )?;
    run_git(&dir, &["add", "-A"])?;
    run_git(&dir, &["commit", "-m", "Add user API endpoint"])?;
    println!("Now we have two branches stacked:");
    print_command("stax status");
    run_stax(&dir, &["status"])?;

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 4: Third branch - UI
    step_header(4, total, "Add the final branch");
    print_command("stax create add-dashboard");
    run_stax(&dir, &["create", "add-dashboard"])?;

    fs::write(
        dir.join("dashboard.rs"),
        "pub fn render_dashboard() {\n    let user = get_user(1);\n    println!(\"Welcome, {}\", user.name);\n}\n",
    )?;
    run_git(&dir, &["add", "-A"])?;
    run_git(&dir, &["commit", "-m", "Add dashboard UI"])?;

    println!("Here's the full stack:");
    print_command("stax log");
    run_stax(&dir, &["log"])?;

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 5: Navigate the stack
    step_header(5, total, "Navigate between branches");
    println!("Move through the stack with {} and {}:", "stax up".cyan(), "stax down".cyan());
    println!();
    println!("Go to the bottom of the stack:");
    print_command("stax bottom");
    run_stax(&dir, &["bottom"])?;
    println!("Go back to the top:");
    print_command("stax top");
    run_stax(&dir, &["top"])?;

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 6: Edit a middle branch
    step_header(6, total, "Edit a branch in the middle");
    println!("Need to fix something in the models branch? Navigate to it:");
    print_command("stax bottom");
    run_stax(&dir, &["bottom"])?;

    println!("Make a change:");
    fs::write(
        dir.join("models.rs"),
        "pub struct User {\n    pub id: u64,\n    pub name: String,\n    pub email: String,\n}\n",
    )?;
    run_git(&dir, &["add", "-A"])?;
    run_git(&dir, &["commit", "-m", "Add email field to User"])?;
    println!("After editing a middle branch, the branches above may need rebasing.");
    println!("stax tracks this automatically:");
    print_command("stax status");
    run_stax(&dir, &["status"])?;

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 7: Restack
    step_header(7, total, "Restack the entire stack");
    println!("Restack rebases each branch onto its updated parent:");
    print_command("stax restack --all");
    run_stax(&dir, &["restack", "--all"])?;
    println!();
    println!("All branches are now up to date:");
    print_command("stax status");
    run_stax(&dir, &["status"])?;

    if !pause("Continue?")? {
        return Ok(());
    }

    // Step 8: Wrap up
    step_header(8, total, "The full workflow");
    println!("Here's the typical stacked-branch cycle:");
    println!();
    println!("  {} Create branches with {}", "1.".bold(), "stax create".cyan());
    println!("  {} Make commits on each branch", "2.".bold());
    println!("  {} Submit all PRs with {}", "3.".bold(), "stax submit".cyan());
    println!("  {} Get reviews, make changes", "4.".bold());
    println!("  {} Rebase the stack with {}", "5.".bold(), "stax restack".cyan());
    println!("  {} Merge from the bottom with {}", "6.".bold(), "stax merge".cyan());
    println!("  {} Clean up with {}", "7.".bold(), "stax sync".cyan());
    println!();
    println!("Useful extras:");
    println!("  {} - restack + push + submit in one step", "stax cascade".cyan());
    println!("  {} - interactive TUI for your stacks", "stax".cyan());
    println!("  {} - undo any risky operation", "stax undo".cyan());
    println!();
    println!(
        "{}",
        "Run `stax --help` to see all available commands.".dimmed()
    );
    println!();
    println!("{}", "Happy stacking!".bold().green());

    Ok(())
}

// ─── Entry point ────────────────────────────────────────────────────────────

pub fn run() -> Result<()> {
    println!();
    println!("{}", "Welcome to the stax interactive demo!".bold().green());
    println!();
    println!(
        "{}",
        "Pick a demo to learn how stax works. A temporary repo will be"
            .dimmed()
    );
    println!(
        "{}",
        "created — nothing in your real projects will be changed.".dimmed()
    );
    println!();

    let demos = &[
        "Creating a pull request          (Time to completion: ~2 min)",
        "Creating a stack of pull requests (Time to completion: ~5 min)",
    ];

    let selection = dialoguer::Select::new()
        .with_prompt("What demo would you like to run?")
        .items(demos)
        .default(0)
        .interact_opt()
        .unwrap_or(None);

    match selection {
        Some(0) => demo_first_pr()?,
        Some(1) => demo_stacked_prs()?,
        _ => {
            println!();
            println!("No demo selected. Run {} anytime to try again.", "stax demo".cyan());
        }
    }

    Ok(())
}
