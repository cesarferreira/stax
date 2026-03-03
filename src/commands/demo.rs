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
        "{}",
        format!("--- Step {}/{}: {} ---", step, total, title)
            .bold()
            .blue()
    );
    println!();
}

pub fn run() -> Result<()> {
    let total_steps = 7;

    println!();
    println!("{}", "Welcome to the stax interactive demo!".bold().green());
    println!();
    println!("This tutorial will walk you through the basics of stacked branches.");
    println!("A temporary repository will be created for the demo.");
    println!();

    if !pause("Start the demo?")? {
        println!("Demo cancelled.");
        return Ok(());
    }

    // Create temp directory
    let tmp = tempfile::tempdir().context("Failed to create temp directory")?;
    let dir = tmp.path();

    // Initialize repo
    run_git(dir, &["init", "-b", "main"])?;
    run_git(dir, &["config", "user.email", "demo@stax.dev"])?;
    run_git(dir, &["config", "user.name", "Stax Demo"])?;

    // Create initial commit
    fs::write(dir.join("README.md"), "# My Project\n")?;
    run_git(dir, &["add", "-A"])?;
    run_git(dir, &["commit", "-m", "Initial commit"])?;

    // Initialize stax
    run_stax(dir, &["doctor"])?;

    // Step 1: Explain stacks
    step_header(1, total_steps, "What are stacked branches?");
    println!("Stacked branches are a series of branches where each one builds");
    println!("on top of the previous. They help you break large changes into");
    println!("small, reviewable PRs.");
    println!();
    println!("Let's see the current state:");
    run_stax(dir, &["status"])?;

    if !pause("Continue to create your first branch?")? {
        println!("Demo ended.");
        return Ok(());
    }

    // Step 2: Create first branch
    step_header(2, total_steps, "Create a stacked branch");
    println!("Creating a new branch 'add-login' stacked on main...");
    println!();
    run_stax(dir, &["create", "add-login"])?;
    fs::write(dir.join("login.rs"), "fn login() { /* TODO */ }\n")?;
    run_git(dir, &["add", "-A"])?;
    run_git(dir, &["commit", "-m", "Add login function"])?;
    println!();
    println!("We've created a branch and added a commit. Let's see the status:");
    run_stax(dir, &["status"])?;

    if !pause("Continue to add a second stacked branch?")? {
        println!("Demo ended.");
        return Ok(());
    }

    // Step 3: Create second branch
    step_header(3, total_steps, "Stack another branch");
    println!("Creating 'add-auth' on top of 'add-login'...");
    println!();
    run_stax(dir, &["create", "add-auth"])?;
    fs::write(
        dir.join("auth.rs"),
        "fn authenticate() { login(); /* verify */ }\n",
    )?;
    run_git(dir, &["add", "-A"])?;
    run_git(dir, &["commit", "-m", "Add authentication layer"])?;

    if !pause("Continue to see the full stack?")? {
        println!("Demo ended.");
        return Ok(());
    }

    // Step 4: Show the full stack
    step_header(4, total_steps, "View the full stack");
    println!("Here's the full stack with commit details:");
    println!();
    run_stax(dir, &["log"])?;

    if !pause("Continue to learn about navigation?")? {
        println!("Demo ended.");
        return Ok(());
    }

    // Step 5: Navigate
    step_header(5, total_steps, "Navigate the stack");
    println!("Move down to the parent branch:");
    run_stax(dir, &["down"])?;
    println!();
    println!("Now move back up:");
    run_stax(dir, &["up"])?;
    println!();
    println!("Jump to the bottom of the stack:");
    run_stax(dir, &["bottom"])?;
    println!();
    println!("Jump to the top:");
    run_stax(dir, &["top"])?;

    if !pause("Continue to create a third branch?")? {
        println!("Demo ended.");
        return Ok(());
    }

    // Step 6: Third branch
    step_header(6, total_steps, "Growing the stack");
    run_stax(dir, &["create", "add-dashboard"])?;
    fs::write(
        dir.join("dashboard.rs"),
        "fn dashboard() { /* show user data */ }\n",
    )?;
    run_git(dir, &["add", "-A"])?;
    run_git(dir, &["commit", "-m", "Add dashboard view"])?;
    println!();
    println!("Three branches stacked:");
    run_stax(dir, &["log"])?;

    if !pause("Continue to the final step?")? {
        println!("Demo ended.");
        return Ok(());
    }

    // Step 7: Wrap up
    step_header(7, total_steps, "What's next?");
    println!("You've learned the basics of stax!");
    println!();
    println!("Key commands:");
    println!("  {} - create a new stacked branch", "stax create <name>".cyan());
    println!("  {} - view all stacks", "stax status".cyan());
    println!("  {} - view stack with commits", "stax log".cyan());
    println!("  {} - push & create/update PRs", "stax submit".cyan());
    println!("  {} - rebase onto updated parent", "stax restack".cyan());
    println!("  {} - pull trunk, clean up merged", "stax sync".cyan());
    println!("  {} / {} - navigate the stack", "stax up".cyan(), "stax down".cyan());
    println!();
    println!(
        "{}",
        "Run `stax --help` to see all available commands.".dimmed()
    );
    println!();
    println!("{}", "Happy stacking!".bold().green());

    Ok(())
}
