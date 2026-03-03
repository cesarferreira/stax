use anyhow::{Context, Result};
use colored::Colorize;
use console;
use dialoguer::theme::ColorfulTheme;
use dialoguer::Select;
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

fn stax_exe() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| "stax".into())
}

/// Run stax and print its output (for demo steps the user should see)
fn run_stax(cwd: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new(stax_exe())
        .args(args)
        .current_dir(cwd)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .with_context(|| format!("Failed to run stax {}", args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        print!("{}", stdout);
    }
    Ok(())
}

/// Run stax silently (for setup scaffolding)
fn run_stax_quiet(cwd: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new(stax_exe())
        .args(args)
        .current_dir(cwd)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .with_context(|| format!("Failed to run stax {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("stax {} failed: {}", args.join(" "), stderr);
    }
    Ok(())
}

fn pause() -> Result<bool> {
    println!();
    let cont = dialoguer::Confirm::new()
        .with_prompt("Continue?")
        .default(true)
        .interact()
        .unwrap_or(false);
    println!();
    Ok(cont)
}

fn step(n: usize, total: usize, title: &str) {
    println!(
        "{}  {}",
        format!("[{}/{}]", n, total).bold().blue(),
        title.bold()
    );
    println!();
}

fn cmd(text: &str) {
    println!("  {} {}", "$".dimmed(), text.cyan());
    println!();
}

/// Initialize a temp repo with stax trunk set — no noisy doctor output
fn setup_repo() -> Result<(tempfile::TempDir, std::path::PathBuf)> {
    let tmp = tempfile::tempdir().context("Failed to create temp directory")?;
    let dir = tmp.path().to_path_buf();
    run_git(&dir, &["init", "-b", "main"])?;
    run_git(&dir, &["config", "user.email", "demo@stax.dev"])?;
    run_git(&dir, &["config", "user.name", "Stax Demo"])?;
    fs::write(dir.join("README.md"), "# My Project\n")?;
    run_git(&dir, &["add", "-A"])?;
    run_git(&dir, &["commit", "-m", "Initial commit"])?;

    // Write the trunk ref directly (same as stax init) to avoid doctor output
    let child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    use std::io::Write;
    let mut child = child;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(b"main")?;
    }
    let output = child.wait_with_output()?;
    let blob_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    run_git(&dir, &["update-ref", "refs/stax/trunk", &blob_hash])?;

    Ok((tmp, dir))
}

fn commit(dir: &Path, file: &str, content: &str, msg: &str) -> Result<()> {
    fs::write(dir.join(file), content)?;
    run_git(dir, &["add", "-A"])?;
    run_git(dir, &["commit", "-m", msg])
}

/// Silently create a branch and commit (for scaffolding stacks before a demo step)
fn scaffold_branch(dir: &Path, name: &str, file: &str, code: &str, msg: &str) -> Result<()> {
    run_stax_quiet(dir, &["create", name])?;
    commit(dir, file, code, msg)
}

// ─── Demo 1: First PR ───────────────────────────────────────────────────────

fn demo_first_pr() -> Result<()> {
    let t = 4;
    println!();
    println!("{}", "Demo: Your first pull request".bold().green());
    println!("{}", "Create a branch, commit, and see how stax tracks it.".dimmed());
    println!();

    let (_tmp, dir) = setup_repo()?;

    step(1, t, "Start from trunk");
    cmd("stax status");
    run_stax(&dir, &["status"])?;
    if !pause()? { return Ok(()); }

    step(2, t, "Create a branch and add a commit");
    cmd("stax create add-login");
    run_stax(&dir, &["create", "add-login"])?;
    commit(&dir, "login.rs", "pub fn login(user: &str, pass: &str) -> bool { true }\n", "Add login function")?;
    cmd("stax status");
    run_stax(&dir, &["status"])?;
    println!("{}", "stax tracks the parent automatically — no manual base branches.".dimmed());
    if !pause()? { return Ok(()); }

    step(3, t, "See commits per branch");
    cmd("stax log");
    run_stax(&dir, &["log"])?;
    if !pause()? { return Ok(()); }

    step(4, t, "Submit your PR");
    println!("With GitHub configured, {} pushes and creates a PR.", "stax submit".cyan());
    println!("The PR targets the correct parent branch automatically.");
    println!();
    println!("{}", "Done! You can now create branches and submit PRs with stax.".bold().green());
    println!();
    Ok(())
}

// ─── Demo 2: Stacking PRs ───────────────────────────────────────────────────

fn demo_stacking() -> Result<()> {
    let t = 5;
    println!();
    println!("{}", "Demo: Stacking multiple PRs".bold().green());
    println!("{}", "Break a big feature into small, reviewable PRs.".dimmed());
    println!();

    let (_tmp, dir) = setup_repo()?;

    step(1, t, "Build a 3-branch stack");
    cmd("stax create add-models");
    run_stax(&dir, &["create", "add-models"])?;
    commit(&dir, "models.rs", "pub struct User { pub id: u64, pub name: String }\n", "Add User model")?;

    cmd("stax create add-api");
    run_stax(&dir, &["create", "add-api"])?;
    commit(&dir, "api.rs", "pub fn get_user(id: u64) -> User { todo!() }\n", "Add user API")?;

    cmd("stax create add-ui");
    run_stax(&dir, &["create", "add-ui"])?;
    commit(&dir, "ui.rs", "pub fn render(user: &User) { println!(\"{}\", user.name); }\n", "Add user UI")?;

    cmd("stax log");
    run_stax(&dir, &["log"])?;
    println!("{}", "3 branches, each building on the last. Each becomes its own PR.".dimmed());
    if !pause()? { return Ok(()); }

    step(2, t, "Navigate the stack");
    cmd("stax bottom");
    run_stax(&dir, &["bottom"])?;
    cmd("stax top");
    run_stax(&dir, &["top"])?;
    if !pause()? { return Ok(()); }

    step(3, t, "Edit a middle branch");
    run_stax_quiet(&dir, &["bottom"])?;
    commit(&dir, "models.rs", "pub struct User { pub id: u64, pub name: String, pub email: String }\n", "Add email to User")?;
    cmd("stax status");
    run_stax(&dir, &["status"])?;
    println!("{}", "Branches above are marked as needing rebase.".dimmed());
    if !pause()? { return Ok(()); }

    step(4, t, "Restack everything");
    cmd("stax restack --all");
    run_stax(&dir, &["restack", "--all"])?;
    cmd("stax status");
    run_stax(&dir, &["status"])?;
    println!("{}", "All branches rebased onto their updated parents.".dimmed());
    if !pause()? { return Ok(()); }

    step(5, t, "Submit the whole stack");
    println!("{} pushes every branch and creates/updates all PRs at once.", "stax submit".cyan());
    println!("Each PR targets the correct parent — reviewers see small diffs.");
    println!();
    println!("{}", "Done! You can build, restack, and submit entire stacks.".bold().green());
    println!();
    Ok(())
}

// ─── Demo 3: Navigating stacks ──────────────────────────────────────────────

fn demo_navigation() -> Result<()> {
    let t = 4;
    println!();
    println!("{}", "Demo: Navigating your stack".bold().green());
    println!("{}", "Move between branches without remembering names.".dimmed());
    println!();

    let (_tmp, dir) = setup_repo()?;

    // Build a 4-branch stack silently
    scaffold_branch(&dir, "feat-auth", "auth.rs", "pub fn auth() {}\n", "Add auth")?;
    scaffold_branch(&dir, "feat-session", "session.rs", "pub fn session() {}\n", "Add session")?;
    scaffold_branch(&dir, "feat-profile", "profile.rs", "pub fn profile() {}\n", "Add profile")?;
    scaffold_branch(&dir, "feat-settings", "settings.rs", "pub fn settings() {}\n", "Add settings")?;

    step(1, t, "See where you are");
    cmd("stax status");
    run_stax(&dir, &["status"])?;
    println!("{}", "You're at the top of a 4-branch stack.".dimmed());
    if !pause()? { return Ok(()); }

    step(2, t, "Move down and up");
    cmd("stax down");
    run_stax(&dir, &["down"])?;
    cmd("stax down 2");
    run_stax(&dir, &["down", "2"])?;
    cmd("stax up");
    run_stax(&dir, &["up"])?;
    println!("{}", "down/up accept a count — jump multiple levels at once.".dimmed());
    if !pause()? { return Ok(()); }

    step(3, t, "Jump to top and bottom");
    cmd("stax bottom");
    run_stax(&dir, &["bottom"])?;
    cmd("stax top");
    run_stax(&dir, &["top"])?;
    if !pause()? { return Ok(()); }

    step(4, t, "Switch to trunk and back");
    cmd("stax trunk");
    run_stax(&dir, &["trunk"])?;
    cmd("stax prev");
    run_stax(&dir, &["prev"])?;
    println!("{}", "prev returns to whatever branch you were on before.".dimmed());
    println!();
    println!("{}", "Done! Navigate any stack without typing branch names.".bold().green());
    println!();
    Ok(())
}

// ─── Demo 4: Undo risky operations ──────────────────────────────────────────

fn demo_undo() -> Result<()> {
    let t = 3;
    println!();
    println!("{}", "Demo: Undo and safety net".bold().green());
    println!("{}", "Every risky operation can be reversed with stax undo.".dimmed());
    println!();

    let (_tmp, dir) = setup_repo()?;

    step(1, t, "Create a stack");
    scaffold_branch(&dir, "feat-payments", "pay.rs", "pub fn charge(amount: u64) {}\n", "Add payments")?;
    scaffold_branch(&dir, "feat-receipts", "receipt.rs", "pub fn receipt() {}\n", "Add receipts")?;
    cmd("stax log");
    run_stax(&dir, &["log"])?;
    if !pause()? { return Ok(()); }

    step(2, t, "Detach a branch (risky operation)");
    println!("Remove {} from the stack:", "feat-payments".cyan());
    run_stax_quiet(&dir, &["down"])?;
    cmd("stax detach --yes");
    run_stax(&dir, &["detach", "--yes"])?;
    cmd("stax status");
    run_stax(&dir, &["status"])?;
    println!("{}", "feat-receipts was reparented to main automatically.".dimmed());
    if !pause()? { return Ok(()); }

    step(3, t, "Undo it");
    cmd("stax undo --yes");
    run_stax(&dir, &["undo", "--yes"])?;
    cmd("stax log");
    run_stax(&dir, &["log"])?;
    println!("{}", "The stack is restored to its original shape.".dimmed());
    println!();
    println!("{}", "Done! Experiment freely — stax undo has your back.".bold().green());
    println!();
    Ok(())
}

// ─── Demo 5: Validate and fix ───────────────────────────────────────────────

fn demo_health() -> Result<()> {
    let t = 3;
    println!();
    println!("{}", "Demo: Stack health checks".bold().green());
    println!("{}", "Detect and fix broken metadata before it causes problems.".dimmed());
    println!();

    let (_tmp, dir) = setup_repo()?;

    step(1, t, "Build a stack");
    scaffold_branch(&dir, "feat-cache", "cache.rs", "pub fn cache() {}\n", "Add caching")?;
    scaffold_branch(&dir, "feat-ttl", "ttl.rs", "pub fn ttl() {}\n", "Add TTL support")?;
    cmd("stax status");
    run_stax(&dir, &["status"])?;
    if !pause()? { return Ok(()); }

    step(2, t, "Run a health check");
    cmd("stax validate");
    run_stax(&dir, &["validate"])?;
    println!("{}", "All checks passed — no orphaned refs, no cycles, no stale parents.".dimmed());
    if !pause()? { return Ok(()); }

    step(3, t, "Auto-fix problems");
    println!("If validate finds issues, {} repairs them automatically:", "stax fix".cyan());
    println!();
    println!("  {} Deletes metadata for branches that no longer exist", "-".dimmed());
    println!("  {} Reparents orphans to trunk", "-".dimmed());
    println!("  {} Cleans up invalid JSON refs", "-".dimmed());
    println!();
    println!("Use {} to preview without changing anything.", "stax fix --dry-run".cyan());
    println!();
    println!("{}", "Done! Keep your stacks healthy with validate and fix.".bold().green());
    println!();
    Ok(())
}

// ─── Entry point ────────────────────────────────────────────────────────────

pub fn run() -> Result<()> {
    println!();
    println!("{}", "Welcome to the stax interactive demo!".bold().green());
    println!(
        "{}",
        "A temporary repo is created for each demo — your projects are untouched.".dimmed()
    );
    println!();

    let demos = &[
        format!(
            "{}  {}",
            "Your first pull request".bold(),
            "(~1 min)".dimmed()
        ),
        format!(
            "{}  {}",
            "Stacking multiple PRs".bold(),
            "(~3 min)".dimmed()
        ),
        format!(
            "{}  {}",
            "Navigating your stack".bold(),
            "(~2 min)".dimmed()
        ),
        format!(
            "{}  {}",
            "Undo and safety net".bold(),
            "(~2 min)".dimmed()
        ),
        format!(
            "{}  {}",
            "Stack health checks".bold(),
            "(~1 min)".dimmed()
        ),
    ];

    let theme = ColorfulTheme {
        active_item_style: console::Style::new()
            .for_stderr()
            .green()
            .bold(),
        active_item_prefix: console::style("▸ ".to_string())
            .for_stderr()
            .green()
            .bold(),
        inactive_item_prefix: console::style("  ".to_string()).for_stderr(),
        prompt_style: console::Style::new().for_stderr().bold().cyan(),
        prompt_prefix: console::style("?".to_string())
            .for_stderr()
            .green()
            .bold(),
        ..ColorfulTheme::default()
    };

    let selection = Select::with_theme(&theme)
        .with_prompt("What demo would you like to run?")
        .items(demos)
        .default(0)
        .interact_opt()
        .unwrap_or(None);

    match selection {
        Some(0) => demo_first_pr()?,
        Some(1) => demo_stacking()?,
        Some(2) => demo_navigation()?,
        Some(3) => demo_undo()?,
        Some(4) => demo_health()?,
        _ => {
            println!();
            println!("No demo selected. Run {} anytime.", "stax demo".cyan());
        }
    }

    Ok(())
}
