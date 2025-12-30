//! Undo the last stax operation (or a specific one).

use crate::config::Config;
use crate::git::GitRepo;
use crate::ops::receipt::{OpReceipt, OpStatus};
use crate::ops;
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
            .context("No operations to undo. Run a stax command first.")?,
    };
    
    if !receipt.can_undo() {
        anyhow::bail!(
            "Operation {} cannot be undone (no refs with before-OIDs)",
            receipt.op_id
        );
    }
    
    if !quiet {
        println!("{}", "Undoing operation...".bold());
        println!(
            "  {} Operation: {} ({})",
            "▸".dimmed(),
            receipt.op_id.cyan(),
            receipt.kind.display_name()
        );
        println!(
            "  {} Status: {}",
            "▸".dimmed(),
            match receipt.status {
                OpStatus::Success => "success".green(),
                OpStatus::Failed => "failed".red(),
                OpStatus::InProgress => "in_progress".yellow(),
            }
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
            anyhow::bail!("Cannot undo with dirty working tree");
        }
    }
    
    // Restore local refs
    let mut restored_count = 0;
    let head_branch_before = receipt.head_branch_before.clone();
    
    if !quiet {
        println!();
        println!("{}", "Restoring local refs...".bold());
    }
    
    for entry in &receipt.local_refs {
        if let Some(oid_before) = &entry.oid_before {
            if !quiet {
                print!("  {} {} → {}... ", "▸".dimmed(), entry.branch.cyan(), &oid_before[..10]);
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
            
            // Update the ref to the before-OID
            repo.update_ref(&entry.refname, oid_before)?;
            
            if !quiet {
                println!("{}", "done".green());
            }
            restored_count += 1;
        } else if entry.existed_before {
            // Ref existed but we don't have the OID - skip
            if !quiet {
                println!(
                    "  {} {} - skipped (no before-OID recorded)",
                    "▸".dimmed(),
                    entry.branch.yellow()
                );
            }
        }
    }
    
    // If the head branch was modified, reset the working tree
    if receipt.local_refs.iter().any(|r| r.branch == head_branch_before) {
        if !quiet {
            println!("  {} Resetting working tree to {}...", "▸".dimmed(), head_branch_before.cyan());
        }
        
        // Make sure we're on that branch
        repo.checkout(&head_branch_before)?;
        
        // Reset to the restored ref
        if let Some(entry) = receipt.local_refs.iter().find(|r| r.branch == head_branch_before) {
            if let Some(oid_before) = &entry.oid_before {
                repo.reset_hard(oid_before)?;
            }
        }
    }
    
    // Handle remote refs
    if receipt.has_remote_changes() && !no_push {
        let remote_count = receipt.remote_refs.iter()
            .filter(|r| r.oid_before.is_some())
            .count();
        
        if remote_count > 0 {
            if !quiet {
                println!();
                println!(
                    "{}",
                    format!(
                        "This operation force-pushed {} {} to remote.",
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
                restore_remote_refs(&repo, &receipt, quiet)?;
            } else if !quiet {
                println!("  {} Skipping remote restore (local only)", "▸".dimmed());
            }
        }
    }
    
    // Clean up backup refs for this operation
    ops::delete_backup_refs(&repo, &receipt.op_id)?;
    
    if !quiet {
        println!();
        println!(
            "{}",
            format!("✓ Undone! Restored {} {}.", restored_count, if restored_count == 1 { "branch" } else { "branches" }).green().bold()
        );
    }
    
    Ok(())
}

/// Restore remote refs by force-pushing
fn restore_remote_refs(repo: &GitRepo, receipt: &OpReceipt, quiet: bool) -> Result<()> {
    let config = Config::load()?;
    let remote_name = config.remote_name();
    
    if !quiet {
        println!();
        println!("{}", "Restoring remote refs...".bold());
    }
    
    for entry in &receipt.remote_refs {
        if let Some(oid_before) = &entry.oid_before {
            if !quiet {
                print!(
                    "  {} {}/{} → {}... ",
                    "▸".dimmed(),
                    entry.remote,
                    entry.branch.cyan(),
                    &oid_before[..10.min(oid_before.len())]
                );
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
            
            // First, update the local ref to the before-OID, then force push
            let local_refname = format!("refs/heads/{}", entry.branch);
            
            // Temporarily set the branch to the old OID
            repo.update_ref(&local_refname, oid_before)?;
            
            // Force push
            let result = repo.force_push(remote_name, &entry.branch);
            
            // Restore the branch to its current local state (from oid_after or oid_before)
            if let Some(oid_after) = &entry.oid_after {
                let _ = repo.update_ref(&local_refname, oid_after);
            }
            
            match result {
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

