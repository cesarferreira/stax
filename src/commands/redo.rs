//! Redo the last undone stax operation.

use crate::config::Config;
use crate::git::GitRepo;
use crate::ops::receipt::{OpReceipt, OpStatus};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};

pub fn run(op_id: Option<String>, yes: bool, no_push: bool, quiet: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let git_dir = repo.git_dir()?;
    
    // Load the receipt
    let receipt = match op_id {
        Some(id) => OpReceipt::load(git_dir, &id)?,
        None => OpReceipt::load_latest(git_dir)?
            .context("No operations to redo. Run a stax command first.")?,
    };
    
    if !receipt.can_redo() {
        anyhow::bail!(
            "Operation {} cannot be redone (no refs with after-OIDs)",
            receipt.op_id
        );
    }
    
    if receipt.status != OpStatus::Success {
        anyhow::bail!(
            "Operation {} was not successful, cannot redo",
            receipt.op_id
        );
    }
    
    if !quiet {
        println!("{}", "Redoing operation...".bold());
        println!(
            "  {} Operation: {} ({})",
            "▸".dimmed(),
            receipt.op_id.cyan(),
            receipt.kind.display_name()
        );
    }
    
    // Check for rebase in progress
    if repo.rebase_in_progress()? {
        if !quiet {
            println!("  {} Aborting in-progress rebase...", "▸".dimmed());
        }
        repo.rebase_abort()?;
    }
    
    // Check for dirty working tree
    if repo.is_dirty()? {
        if quiet {
            anyhow::bail!("Working tree is dirty. Please stash or commit changes first.");
        }
        
        let stash = if yes {
            true
        } else {
            Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Working tree has uncommitted changes. Stash them?")
                .default(true)
                .interact()?
        };
        
        if stash {
            repo.stash_push()?;
            if !quiet {
                println!("  {} Stashed working tree changes.", "✓".green());
            }
        } else {
            anyhow::bail!("Cannot redo with dirty working tree");
        }
    }
    
    // Restore local refs to after-OIDs
    let mut restored_count = 0;
    let head_branch_before = receipt.head_branch_before.clone();
    
    if !quiet {
        println!();
        println!("{}", "Restoring refs to after-state...".bold());
    }
    
    for entry in &receipt.local_refs {
        if let Some(oid_after) = &entry.oid_after {
            if !quiet {
                print!("  {} {} → {}... ", "▸".dimmed(), entry.branch.cyan(), &oid_after[..10]);
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
            
            // Update the ref to the after-OID
            repo.update_ref(&entry.refname, oid_after)?;
            
            if !quiet {
                println!("{}", "done".green());
            }
            restored_count += 1;
        }
    }
    
    // If the head branch was modified, reset the working tree
    if receipt.local_refs.iter().any(|r| r.branch == head_branch_before) {
        if !quiet {
            println!("  {} Resetting working tree to {}...", "▸".dimmed(), head_branch_before.cyan());
        }
        
        repo.checkout(&head_branch_before)?;
        
        if let Some(entry) = receipt.local_refs.iter().find(|r| r.branch == head_branch_before) {
            if let Some(oid_after) = &entry.oid_after {
                repo.reset_hard(oid_after)?;
            }
        }
    }
    
    // Handle remote refs
    if receipt.has_remote_changes() && !no_push {
        let remote_count = receipt.remote_refs.iter()
            .filter(|r| r.oid_after.is_some())
            .count();
        
        if remote_count > 0 {
            if !quiet {
                println!();
                println!(
                    "{}",
                    format!(
                        "This operation had force-pushed {} {} to remote.",
                        remote_count,
                        if remote_count == 1 { "branch" } else { "branches" }
                    ).yellow()
                );
            }
            
            let push = if yes {
                true
            } else if quiet {
                false
            } else {
                Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt("Force-push to restore remote branches too?")
                    .default(false)
                    .interact()?
            };
            
            if push {
                restore_remote_refs_after(&repo, &receipt, quiet)?;
            } else if !quiet {
                println!("  {} Skipping remote restore (local only)", "▸".dimmed());
            }
        }
    }
    
    if !quiet {
        println!();
        println!(
            "{}",
            format!("✓ Redone! Restored {} {} to after-state.", restored_count, if restored_count == 1 { "branch" } else { "branches" }).green().bold()
        );
    }
    
    Ok(())
}

/// Restore remote refs to after-state by force-pushing
fn restore_remote_refs_after(repo: &GitRepo, receipt: &OpReceipt, quiet: bool) -> Result<()> {
    let config = Config::load()?;
    let remote_name = config.remote_name();
    
    if !quiet {
        println!();
        println!("{}", "Restoring remote refs to after-state...".bold());
    }
    
    for entry in &receipt.remote_refs {
        if let Some(oid_after) = &entry.oid_after {
            if !quiet {
                print!(
                    "  {} {}/{} → {}... ",
                    "▸".dimmed(),
                    entry.remote,
                    entry.branch.cyan(),
                    &oid_after[..10.min(oid_after.len())]
                );
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
            
            // The local ref should already be at oid_after, just force push
            match repo.force_push(remote_name, &entry.branch) {
                Ok(()) => {
                    if !quiet {
                        println!("{}", "done".green());
                    }
                }
                Err(e) => {
                    if !quiet {
                        println!("{}", format!("failed: {}", e).red());
                    }
                }
            }
        }
    }
    
    Ok(())
}

