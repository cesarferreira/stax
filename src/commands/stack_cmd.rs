use crate::commands::worktree::shared::platform_shell;
use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, refs};
use crate::github::gh_stack::{self, ExtensionStatus, LinkOutcome};
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;
use crate::remote::{ForgeType, RemoteInfo};
use anyhow::Result;
use colored::Colorize;
use git2::BranchType;
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

// =========================================================================
// validate
// =========================================================================

pub fn run_link() -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    if remote_info.forge != ForgeType::GitHub {
        anyhow::bail!(
            "`stax stack link` is only supported for GitHub remotes (found {})",
            remote_info.forge
        );
    }
    ensure_gh_stack_extension()?;

    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let pr_numbers = current_stack_pr_numbers(&stack, &current)?;
    // `gh stack link` requires at least two PRs — a native stack is inherently
    // multi-PR. Fail early with a clear message instead of surfacing gh-stack's
    // raw "requires at least 2 arg(s)" error.
    if pr_numbers.len() < 2 {
        anyhow::bail!(
            "Native GitHub Stacks require at least 2 PRs in the current stack (found {}). \
             Submit another branch in this stack, then run `stax stack link` again.",
            pr_numbers.len()
        );
    }

    match gh_stack::link_stack(&pr_numbers, &stack.trunk, &remote_info.name) {
        LinkOutcome::Linked => {
            gh_stack::set_feature_enabled(repo.workdir()?, true)?;
            println!(
                "{} {}",
                "✓".green(),
                format!("Linked {} PRs as a native GitHub Stack", pr_numbers.len()).dimmed()
            );
            Ok(())
        }
        LinkOutcome::FeatureDisabled { message } => {
            gh_stack::set_feature_enabled(repo.workdir()?, false)?;
            anyhow::bail!("GitHub native Stacked PRs are not enabled for this repo: {message}");
        }
        LinkOutcome::AuthTokenUnsupported { message } => {
            anyhow::bail!(
                "GitHub rejected the native Stack link: {message}\n\n\
                 stax already ignores GH_TOKEN/GITHUB_TOKEN when talking to `gh stack`, but no \
                 OAuth-authenticated `gh` account was found. Run `gh auth login` (or `gh auth \
                 switch` if you already have one) to add an OAuth-authenticated account, then \
                 retry."
            );
        }
        LinkOutcome::SinglePrValidationRejected { message } => {
            anyhow::bail!("GitHub rejected the native Stack link: {message}");
        }
        LinkOutcome::Failed { message } => {
            if gh_stack::is_stack_fork_conflict(&message) {
                anyhow::bail!(
                    "Cannot link this stack natively: it shares ancestor PRs with another \
                     branch that's already registered as a native GitHub Stack. GitHub's native \
                     Stack feature only supports one linear chain at a time — unlink the other \
                     branch's stack first (run `stax stack unlink` from that branch, or remove \
                     the stack from the GitHub PR UI) if you want this one linked instead.\n\n\
                     gh-stack said: {message}"
                );
            }
            anyhow::bail!("Failed to link native GitHub Stack: {message}");
        }
    }
}

pub fn run_unlink(stack_number: Option<u64>) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    if remote_info.forge != ForgeType::GitHub {
        anyhow::bail!(
            "`stax stack unlink` is only supported for GitHub remotes (found {})",
            remote_info.forge
        );
    }
    ensure_gh_stack_extension()?;

    match gh_stack::unlink_stack(stack_number) {
        LinkOutcome::Linked => {
            println!("{}", "✓ Native GitHub Stack removed".green());
            Ok(())
        }
        LinkOutcome::FeatureDisabled { message } => {
            anyhow::bail!("GitHub native Stacked PRs are not enabled for this repo: {message}");
        }
        LinkOutcome::AuthTokenUnsupported { message } => {
            anyhow::bail!(
                "GitHub rejected the native Stack unlink: {message}\n\n\
                 stax already ignores GH_TOKEN/GITHUB_TOKEN when talking to `gh stack`, but no \
                 OAuth-authenticated `gh` account was found. Run `gh auth login` (or `gh auth \
                 switch` if you already have one) to add an OAuth-authenticated account, then \
                 retry."
            );
        }
        LinkOutcome::SinglePrValidationRejected { message } => {
            anyhow::bail!("GitHub rejected the native Stack unlink: {message}");
        }
        LinkOutcome::Failed { message } => {
            // `gh stack unstack` operates on a locally-tracked stack, but stacks
            // registered via `gh stack link` (the seam stax uses) carry no local
            // tracking — so unstack reports the current branch is not part of a
            // stack. Explain the limitation rather than the raw gh-stack error.
            if message.to_lowercase().contains("not part of a stack") {
                anyhow::bail!(
                    "No locally-tracked native stack for the current branch. Native stacks that \
                     stax registers via `gh stack link` are not tracked locally. Run `st stack \
                     unlink <stack-number>` to remove one remotely, or remove it from the GitHub \
                     PR UI."
                );
            }
            anyhow::bail!("Failed to remove native GitHub Stack: {message}");
        }
    }
}

fn ensure_gh_stack_extension() -> Result<()> {
    match gh_stack::extension_status() {
        ExtensionStatus::Installed => Ok(()),
        ExtensionStatus::Outdated => {
            anyhow::bail!(
                "`gh-stack` extension is outdated and lacks `gh stack link`. Run `gh extension upgrade stack`."
            )
        }
        ExtensionStatus::NoExtension => {
            anyhow::bail!(
                "`gh-stack` extension is not installed. Run `gh extension install github/gh-stack`."
            )
        }
        ExtensionStatus::NoGh => {
            anyhow::bail!("GitHub CLI `gh` is not installed or not available on PATH.")
        }
    }
}

fn current_stack_pr_numbers(stack: &Stack, current: &str) -> Result<Vec<u64>> {
    let mut pr_numbers = Vec::new();
    let mut missing = Vec::new();

    for branch in stack.current_stack(current) {
        if branch == stack.trunk {
            continue;
        }
        match stack.branches.get(&branch).and_then(|info| info.pr_number) {
            Some(number) => pr_numbers.push(number),
            None => missing.push(branch),
        }
    }

    if !missing.is_empty() {
        anyhow::bail!(
            "These branches have no PR yet, so they cannot be linked into a native stack:\n  {}\n\n\
             Run `stax submit` to create PRs for the stack, then `stax stack link` again.",
            missing.join("\n  ")
        );
    }

    Ok(pr_numbers)
}

pub fn run_validate() -> Result<()> {
    let repo = GitRepo::open()?;
    let trunk = repo.trunk_branch()?;
    let tracked = refs::list_metadata_branches(repo.inner())?;

    let mut issues = 0;

    println!("{}", "Stack validation".bold());
    println!();

    // 1. Orphaned metadata - refs exist for deleted branches
    let mut orphaned: Vec<String> = Vec::new();
    for name in &tracked {
        if repo.inner().find_branch(name, BranchType::Local).is_err() {
            orphaned.push(name.clone());
        }
    }
    if orphaned.is_empty() {
        println!("{} No orphaned metadata", "PASS".green());
    } else {
        issues += 1;
        println!(
            "{} {} orphaned metadata ref(s):",
            "FAIL".red(),
            orphaned.len()
        );
        for name in &orphaned {
            println!("  {} (branch deleted, metadata remains)", name.yellow());
        }
    }

    // 2. Missing parents - metadata points to non-existent parent
    let mut missing_parents: Vec<(String, String)> = Vec::new();
    for name in &tracked {
        if orphaned.contains(name) {
            continue;
        }
        if let Some(meta) = BranchMetadata::read(repo.inner(), name)?
            && meta.parent_branch_name != trunk
            && repo.branch_commit(&meta.parent_branch_name).is_err()
        {
            missing_parents.push((name.clone(), meta.parent_branch_name.clone()));
        }
    }
    if missing_parents.is_empty() {
        println!("{} All parents exist", "PASS".green());
    } else {
        issues += 1;
        println!(
            "{} {} branch(es) with missing parent:",
            "FAIL".red(),
            missing_parents.len()
        );
        for (branch, parent) in &missing_parents {
            println!("  {} → {} (not found)", branch.yellow(), parent.red());
        }
    }

    // 3. Cycle detection - walk parent chains
    let mut has_cycle = false;
    for name in &tracked {
        if orphaned.contains(name) {
            continue;
        }
        let mut visited = HashSet::new();
        let mut current = name.clone();
        visited.insert(current.clone());

        while let Some(meta) = BranchMetadata::read(repo.inner(), &current)? {
            if meta.parent_branch_name == trunk {
                break;
            }
            if !visited.insert(meta.parent_branch_name.clone()) {
                if !has_cycle {
                    issues += 1;
                }
                has_cycle = true;
                println!(
                    "{} Cycle detected involving '{}'",
                    "FAIL".red(),
                    name.yellow()
                );
                break;
            }
            current = meta.parent_branch_name;
        }
    }
    if !has_cycle {
        println!("{} No cycles detected", "PASS".green());
    }

    // 4. Invalid metadata - unparseable JSON
    let mut invalid: Vec<String> = Vec::new();
    for name in &tracked {
        if orphaned.contains(name) {
            continue;
        }
        if let Some(json) = refs::read_metadata(repo.inner(), name)?
            && serde_json::from_str::<BranchMetadata>(&json).is_err()
        {
            invalid.push(name.clone());
        }
    }
    if invalid.is_empty() {
        println!("{} All metadata is valid JSON", "PASS".green());
    } else {
        issues += 1;
        println!(
            "{} {} branch(es) with invalid metadata:",
            "FAIL".red(),
            invalid.len()
        );
        for name in &invalid {
            println!("  {}", name.yellow());
        }
    }

    // 5. Stale parent revision - needs restack
    let stack = Stack::load(&repo)?;
    let needs_restack = stack.needs_restack();
    if needs_restack.is_empty() {
        println!("{} All branches up to date", "PASS".green());
    } else {
        issues += 1;
        println!(
            "{} {} branch(es) need restack:",
            "WARN".yellow(),
            needs_restack.len()
        );
        for name in &needs_restack {
            println!("  {}", name.yellow());
        }
    }

    println!();
    if issues == 0 {
        println!("{}", "All checks passed.".green());
    } else {
        println!(
            "{}",
            format!("{} issue(s) found. Run `stax fix` to repair.", issues).yellow()
        );
        return Err(crate::errors::SilentExit(crate::errors::exit_codes::GENERAL).into());
    }

    Ok(())
}

// =========================================================================
// fix
// =========================================================================

pub fn run_fix(dry_run: bool, yes: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let trunk = repo.trunk_branch()?;
    let tracked = refs::list_metadata_branches(repo.inner())?;

    let mut fixes = 0;

    println!(
        "{}",
        if dry_run {
            "Stack fix (dry run)".bold()
        } else {
            "Stack fix".bold()
        }
    );
    println!();

    // Collect issues
    let mut orphaned: Vec<String> = Vec::new();
    let mut missing_parents: Vec<(String, String)> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();

    for name in &tracked {
        let branch_exists = repo.inner().find_branch(name, BranchType::Local).is_ok();

        if !branch_exists {
            orphaned.push(name.clone());
            continue;
        }

        if let Some(json) = refs::read_metadata(repo.inner(), name)? {
            if let Ok(meta) = serde_json::from_str::<BranchMetadata>(&json) {
                if meta.parent_branch_name != trunk
                    && repo.branch_commit(&meta.parent_branch_name).is_err()
                {
                    missing_parents.push((name.clone(), meta.parent_branch_name.clone()));
                }
            } else {
                invalid.push(name.clone());
            }
        }
    }

    // Fix orphaned metadata
    if !orphaned.is_empty() {
        println!(
            "Orphaned metadata ({} ref(s)):",
            orphaned.len().to_string().yellow()
        );
        for name in &orphaned {
            println!("  Delete metadata for '{}'", name.yellow());
        }
        if !dry_run {
            for name in &orphaned {
                BranchMetadata::delete(repo.inner(), name)?;
                fixes += 1;
            }
        }
    }

    // Fix invalid metadata
    if !invalid.is_empty() {
        println!(
            "Invalid metadata ({} ref(s)):",
            invalid.len().to_string().yellow()
        );
        for name in &invalid {
            println!("  Delete invalid metadata for '{}'", name.yellow());
        }
        if !dry_run {
            for name in &invalid {
                BranchMetadata::delete(repo.inner(), name)?;
                fixes += 1;
            }
        }
    }

    // Fix missing parents
    if !missing_parents.is_empty() {
        println!(
            "Missing parents ({} branch(es)):",
            missing_parents.len().to_string().yellow()
        );
        for (branch, parent) in &missing_parents {
            println!(
                "  Reparent '{}' to '{}' (was '{}')",
                branch.cyan(),
                trunk.blue(),
                parent.red()
            );
        }
        let should_fix = dry_run
            || yes
            || dialoguer::Confirm::new()
                .with_prompt("Reparent orphaned branches to trunk?")
                .default(true)
                .interact()?;

        if should_fix && !dry_run {
            let mut tx = Transaction::begin(OpKind::Fix, &repo, false)?;
            for (branch, _) in &missing_parents {
                tx.plan_branch(&repo, branch)?;
            }
            tx.snapshot()?;

            for (branch, _) in &missing_parents {
                let trunk_rev = repo.branch_commit(&trunk)?;
                let merge_base = repo
                    .merge_base(&trunk, branch)
                    .unwrap_or_else(|_| trunk_rev.clone());
                let existing = BranchMetadata::read(repo.inner(), branch)?;
                let updated = if let Some(meta) = existing {
                    BranchMetadata {
                        parent_branch_name: trunk.clone(),
                        parent_branch_revision: merge_base,
                        ..meta
                    }
                } else {
                    BranchMetadata::new(&trunk, &merge_base)
                };
                updated.write(repo.inner(), branch)?;
                tx.record_after(&repo, branch)?;
                fixes += 1;
            }
            tx.finish_ok()?;
        }
    }

    // Report stale branches
    let stack = Stack::load(&repo)?;
    let needs_restack = stack.needs_restack();
    if !needs_restack.is_empty() {
        println!();
        println!(
            "{} branch(es) need restack:",
            needs_restack.len().to_string().yellow()
        );
        for name in &needs_restack {
            println!("  {}", name.yellow());
        }
        println!("{}", "Run `stax restack --all` to update them.".dimmed());
    }

    println!();
    if dry_run {
        let total = orphaned.len() + invalid.len() + missing_parents.len();
        if total == 0 {
            println!("{}", "No issues found.".green());
        } else {
            println!(
                "{}",
                format!(
                    "{} issue(s) would be fixed. Run without --dry-run to apply.",
                    total
                )
                .yellow()
            );
        }
    } else if fixes == 0 && orphaned.is_empty() && invalid.is_empty() && missing_parents.is_empty()
    {
        println!("{}", "No issues found.".green());
    } else {
        println!("{}", format!("Fixed {} issue(s).", fixes).green());
    }

    Ok(())
}

// =========================================================================
// test
// =========================================================================

pub fn run_test(
    cmd: Vec<String>,
    all: bool,
    stack_filter: Option<Option<String>>,
    fail_fast: bool,
    parallel: bool,
    jobs: usize,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;

    let branches: Vec<String> = if all {
        // All tracked non-trunk branches (stable order for deterministic output).
        let mut list: Vec<String> = stack
            .branches
            .values()
            .filter(|b| b.parent.is_some())
            .map(|b| b.name.clone())
            .collect();
        list.sort();
        list
    } else if let Some(stack_arg) = stack_filter {
        let target = stack_arg.unwrap_or_else(|| current.clone());
        if !stack.branches.contains_key(&target) {
            anyhow::bail!("Branch '{}' is not tracked in the stack.", target);
        }
        stack
            .current_stack(&target)
            .into_iter()
            .filter(|b| b != &stack.trunk)
            .collect()
    } else {
        stack
            .current_stack(&current)
            .into_iter()
            .filter(|b| b != &stack.trunk)
            .collect()
    };

    if branches.is_empty() {
        println!("{}", "No branches to run command on.".yellow());
        return Ok(());
    }

    let cmd_str = cmd.join(" ");
    println!(
        "Running '{}' on {} branch(es)...",
        cmd_str.cyan(),
        branches.len()
    );
    println!();

    if parallel {
        return run_test_parallel(&repo, &branches, &cmd_str, jobs);
    }

    let mut succeeded = 0;
    let mut failed = 0;
    let mut failed_branches: Vec<String> = Vec::new();

    for branch in &branches {
        // Checkout branch
        repo.checkout(branch)?;

        println!("  {}:", branch.cyan());
        io::stdout().flush()?;

        let status = platform_shell(&cmd_str)
            .current_dir(repo.workdir()?)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()?;

        if status.success() {
            println!("  {} {}", "Result:".dimmed(), "SUCCESS".green());
            succeeded += 1;
        } else {
            println!("  {} {}", "Result:".dimmed(), "FAIL".red());
            failed += 1;
            failed_branches.push(branch.clone());

            if fail_fast {
                println!();
                println!("{}", "Stopping early (--fail-fast).".yellow());
                break;
            }
        }

        println!();
    }

    // Return to original branch
    let _ = repo.checkout(&current);

    println!();
    let failed_str = failed.to_string();
    if failed > 0 {
        println!(
            "{} succeeded, {} failed",
            succeeded.to_string().green(),
            failed_str.red()
        );
    } else {
        println!(
            "{} succeeded, {} failed",
            succeeded.to_string().green(),
            failed_str.green()
        );
    }

    if !failed_branches.is_empty() {
        println!("Failed branches:");
        for b in &failed_branches {
            println!("  {}", b.red());
        }
        return Err(crate::errors::SilentExit(crate::errors::exit_codes::GENERAL).into());
    }

    Ok(())
}

#[derive(Debug)]
struct RunWorktree {
    branch: String,
    path: PathBuf,
}

#[derive(Debug)]
struct ParallelRunResult {
    branch: String,
    path: PathBuf,
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    dirty: bool,
}

fn run_test_parallel(repo: &GitRepo, branches: &[String], cmd: &str, jobs: usize) -> Result<()> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("stax-run-{}-{timestamp}", std::process::id()));
    fs::create_dir_all(&root)?;

    let mut worktrees = Vec::with_capacity(branches.len());
    for (index, branch) in branches.iter().enumerate() {
        let path = root.join(format!("{index:03}-{}", safe_path_component(branch)));
        let output = Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&path)
            .arg(branch)
            .current_dir(repo.workdir()?)
            .output()?;
        if !output.status.success() {
            cleanup_run_worktrees(repo.workdir()?, &worktrees);
            let _ = fs::remove_dir_all(&root);
            anyhow::bail!(
                "Failed to create parallel worktree for '{}': {}",
                branch,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        worktrees.push(RunWorktree {
            branch: branch.clone(),
            path,
        });
    }

    let results = crate::parallel::map_ordered_with_limit(&worktrees, jobs, |worktree| {
        let output = platform_shell(cmd)
            .current_dir(&worktree.path)
            .env("STAX_RUN_BRANCH", &worktree.branch)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        let dirty = worktree_is_dirty(&worktree.path);
        match output {
            Ok(output) => ParallelRunResult {
                branch: worktree.branch.clone(),
                path: worktree.path.clone(),
                success: output.status.success() && !dirty,
                stdout: output.stdout,
                stderr: output.stderr,
                dirty,
            },
            Err(error) => ParallelRunResult {
                branch: worktree.branch.clone(),
                path: worktree.path.clone(),
                success: false,
                stdout: Vec::new(),
                stderr: error.to_string().into_bytes(),
                dirty,
            },
        }
    });

    let mut failed_branches = Vec::new();
    let mut preserved_paths = Vec::new();
    for result in &results {
        println!(
            "  {} (parallel worktree {}):",
            result.branch.cyan(),
            result.path.display()
        );
        print_captured(&result.stdout);
        print_captured(&result.stderr);
        if result.dirty {
            println!(
                "  {} Command left tracked changes; preserved for recovery at {}",
                "WARNING".yellow(),
                result.path.display()
            );
            preserved_paths.push(result.path.clone());
        }
        println!(
            "  {} {}\n",
            "Result:".dimmed(),
            if result.success {
                "SUCCESS".green()
            } else {
                "FAIL".red()
            }
        );
        if !result.success {
            failed_branches.push(result.branch.clone());
        }
    }

    for worktree in &worktrees {
        if preserved_paths.contains(&worktree.path) {
            continue;
        }
        remove_run_worktree(repo.workdir()?, &worktree.path);
    }
    let _ = Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo.workdir()?)
        .status();
    if preserved_paths.is_empty() {
        let _ = fs::remove_dir_all(&root);
    }

    let succeeded = results.len() - failed_branches.len();
    println!(
        "{} succeeded, {} failed",
        succeeded.to_string().green(),
        if failed_branches.is_empty() {
            "0".green()
        } else {
            failed_branches.len().to_string().red()
        }
    );
    if !failed_branches.is_empty() {
        println!("Failed branches:");
        for branch in failed_branches {
            println!("  {}", branch.red());
        }
        return Err(crate::errors::SilentExit(crate::errors::exit_codes::GENERAL).into());
    }
    Ok(())
}

fn print_captured(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(bytes);
    print!("{text}");
    if !text.ends_with('\n') {
        println!();
    }
}

fn worktree_is_dirty(path: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(path)
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(true)
}

fn cleanup_run_worktrees(repo: &Path, worktrees: &[RunWorktree]) {
    for worktree in worktrees {
        remove_run_worktree(repo, &worktree.path);
    }
}

fn remove_run_worktree(repo: &Path, path: &Path) {
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(path)
        .current_dir(repo)
        .status();
}

fn safe_path_component(branch: &str) -> String {
    branch
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
