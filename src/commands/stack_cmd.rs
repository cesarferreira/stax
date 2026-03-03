use crate::engine::{BranchMetadata, Stack};
use crate::git::{refs, GitRepo};
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;
use anyhow::Result;
use colored::Colorize;
use git2::BranchType;
use std::collections::HashSet;

// =========================================================================
// validate
// =========================================================================

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
        if let Some(meta) = BranchMetadata::read(repo.inner(), name)? {
            if meta.parent_branch_name != trunk
                && repo.branch_commit(&meta.parent_branch_name).is_err()
            {
                missing_parents.push((name.clone(), meta.parent_branch_name.clone()));
            }
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
        if let Some(json) = refs::read_metadata(repo.inner(), name)? {
            if serde_json::from_str::<BranchMetadata>(&json).is_err() {
                invalid.push(name.clone());
            }
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
        std::process::exit(1);
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

pub fn run_test(cmd: Vec<String>, all: bool, fail_fast: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let trunk = stack.trunk.clone();

    let branches: Vec<String> = if all {
        // All tracked branches
        stack
            .branches
            .keys()
            .filter(|b| **b != trunk)
            .cloned()
            .collect()
    } else {
        stack
            .current_stack(&current)
            .into_iter()
            .filter(|b| *b != trunk)
            .collect()
    };

    if branches.is_empty() {
        println!("{}", "No branches to test.".yellow());
        return Ok(());
    }

    let cmd_str = cmd.join(" ");
    println!(
        "Running '{}' on {} branch(es)...",
        cmd_str.cyan(),
        branches.len()
    );
    println!();

    let mut passed = 0;
    let mut failed = 0;
    let mut failed_branches: Vec<String> = Vec::new();

    for branch in &branches {
        // Checkout branch
        repo.checkout(branch)?;

        print!("  {} ... ", branch.cyan());

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .current_dir(repo.workdir()?)
            .output()?;

        if output.status.success() {
            println!("{}", "PASS".green());
            passed += 1;
        } else {
            println!("{}", "FAIL".red());
            failed += 1;
            failed_branches.push(branch.clone());

            if fail_fast {
                println!();
                println!("{}", "Stopping early (--fail-fast).".yellow());
                break;
            }
        }
    }

    // Return to original branch
    let _ = repo.checkout(&current);

    println!();
    let failed_str = failed.to_string();
    if failed > 0 {
        println!(
            "{} passed, {} failed",
            passed.to_string().green(),
            failed_str.red()
        );
    } else {
        println!(
            "{} passed, {} failed",
            passed.to_string().green(),
            failed_str.green()
        );
    }

    if !failed_branches.is_empty() {
        println!("Failed branches:");
        for b in &failed_branches {
            println!("  {}", b.red());
        }
        std::process::exit(1);
    }

    Ok(())
}
