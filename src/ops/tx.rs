//! Transaction wrapper for safe history-rewriting operations.
//!
//! Provides a builder-style API:
//! ```ignore
//! let mut tx = Transaction::begin(OpKind::Restack, &repo)?;
//! tx.plan_branch("feature/foo")?;
//! tx.plan_branch("feature/bar")?;
//! tx.snapshot()?;  // Creates backup refs and writes in-progress receipt
//! 
//! // ... do the actual work ...
//! 
//! tx.record_after("feature/foo", new_oid)?;
//! tx.record_after("feature/bar", new_oid)?;
//! tx.finish_ok()?;  // Or tx.finish_err("message")?;
//! ```

use super::receipt::{OpKind, OpReceipt, PlanSummary};
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

/// A transaction wrapper for history-rewriting operations
pub struct Transaction {
    receipt: OpReceipt,
    git_dir: PathBuf,
    workdir: PathBuf,
    /// Whether snapshot() has been called
    snapshotted: bool,
    /// Whether the transaction has been finished
    finished: bool,
    /// Whether to print status messages
    quiet: bool,
}

impl Transaction {
    /// Begin a new transaction
    pub fn begin(kind: OpKind, repo: &GitRepo, quiet: bool) -> Result<Self> {
        let op_id = super::generate_op_id();
        let git_dir = repo.git_dir()?.to_path_buf();
        let workdir = repo.workdir()?.to_path_buf();
        let trunk = repo.trunk_branch()?;
        let head_branch = repo.current_branch()?;
        
        let receipt = OpReceipt::new(
            op_id,
            kind,
            workdir.to_string_lossy().to_string(),
            trunk,
            head_branch,
        );
        
        Ok(Self {
            receipt,
            git_dir,
            workdir,
            snapshotted: false,
            finished: false,
            quiet,
        })
    }
    
    /// Get the operation ID
    pub fn op_id(&self) -> &str {
        &self.receipt.op_id
    }
    
    /// Plan a local branch to be modified
    pub fn plan_branch(&mut self, repo: &GitRepo, branch: &str) -> Result<()> {
        let oid = repo.branch_commit(branch).ok();
        self.receipt.add_local_ref(branch, oid.as_deref());
        Ok(())
    }
    
    /// Plan multiple local branches to be modified
    pub fn plan_branches(&mut self, repo: &GitRepo, branches: &[String]) -> Result<()> {
        for branch in branches {
            self.plan_branch(repo, branch)?;
        }
        Ok(())
    }
    
    /// Plan a remote ref to be modified (for submit)
    pub fn plan_remote_branch(&mut self, repo: &GitRepo, remote: &str, branch: &str) -> Result<()> {
        // Get current remote ref OID
        let remote_ref = format!("{}/{}", remote, branch);
        let oid = repo.rev_parse(&remote_ref).ok();
        self.receipt.add_remote_ref(remote, branch, oid.as_deref());
        Ok(())
    }
    
    /// Set the plan summary
    pub fn set_plan_summary(&mut self, summary: PlanSummary) {
        self.receipt.plan_summary = summary;
    }
    
    /// Create backup refs and write the in-progress receipt
    pub fn snapshot(&mut self) -> Result<()> {
        if self.snapshotted {
            return Ok(());
        }
        
        // Create backup refs for all planned branches
        for entry in &self.receipt.local_refs {
            if let Some(oid) = &entry.oid_before {
                super::create_backup_ref(&self.workdir, &self.receipt.op_id, &entry.branch, oid)?;
            }
        }
        
        // Write the in-progress receipt
        self.receipt.save(&self.git_dir)?;
        
        self.snapshotted = true;
        
        if !self.quiet {
            self.print_snapshot_info();
        }
        
        Ok(())
    }
    
    /// Print snapshot information
    fn print_snapshot_info(&self) {
        let count = self.receipt.local_refs.iter()
            .filter(|r| r.oid_before.is_some())
            .count();
        
        if count > 0 {
            println!(
                "  {} Backup refs created: {}",
                "▸".dimmed(),
                format!("refs/stax/backups/{}/*", self.receipt.op_id).dimmed()
            );
        }
    }
    
    /// Record the after-OID for a branch
    pub fn record_after(&mut self, repo: &GitRepo, branch: &str) -> Result<()> {
        let oid = repo.branch_commit(branch)?;
        self.receipt.update_local_ref_after(branch, &oid);
        Ok(())
    }
    
    /// Record after-OIDs for all planned branches
    pub fn record_all_after(&mut self, repo: &GitRepo) -> Result<()> {
        let branches: Vec<String> = self.receipt.local_refs.iter()
            .map(|r| r.branch.clone())
            .collect();
        
        for branch in branches {
            if let Ok(oid) = repo.branch_commit(&branch) {
                self.receipt.update_local_ref_after(&branch, &oid);
            }
        }
        Ok(())
    }
    
    /// Record the after-OID for a remote branch (the local OID that was pushed)
    pub fn record_remote_after(&mut self, remote: &str, branch: &str, local_oid: &str) {
        self.receipt.update_remote_ref_after(remote, branch, local_oid);
    }
    
    /// Finish the transaction successfully
    pub fn finish_ok(mut self) -> Result<()> {
        self.receipt.mark_success();
        self.receipt.save(&self.git_dir)?;
        self.finished = true;
        Ok(())
    }
    
    /// Finish the transaction with an error
    pub fn finish_err(
        mut self,
        message: &str,
        failed_step: Option<&str>,
        failed_branch: Option<&str>,
    ) -> Result<()> {
        self.receipt.mark_failed(message, failed_step, failed_branch);
        self.receipt.save(&self.git_dir)?;
        self.finished = true;
        
        if !self.quiet {
            self.print_recovery_hint();
        }
        
        Ok(())
    }
    
    /// Print the recovery hint after a failure
    fn print_recovery_hint(&self) {
        println!();
        println!(
            "{}",
            "Your repo is recoverable via:".yellow()
        );
        println!("  {}", "stax undo".cyan());
    }
    
    /// Get the operation kind
    pub fn kind(&self) -> &OpKind {
        &self.receipt.kind
    }
    
    /// Check if the transaction has been snapshotted
    pub fn is_snapshotted(&self) -> bool {
        self.snapshotted
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        // If we snapshotted but didn't finish, mark as failed
        if self.snapshotted && !self.finished {
            self.receipt.mark_failed(
                "Transaction dropped without finishing",
                None,
                None,
            );
            let _ = self.receipt.save(&self.git_dir);
        }
    }
}

/// Print the plan before executing
pub fn print_plan(kind: &OpKind, summary: &PlanSummary, quiet: bool) {
    if quiet {
        return;
    }
    
    if summary.branches_to_rebase > 0 {
        println!(
            "  {} About to rebase {} {}",
            "▸".dimmed(),
            summary.branches_to_rebase.to_string().cyan(),
            if summary.branches_to_rebase == 1 { "branch" } else { "branches" }
        );
    }

    if summary.branches_to_push > 0 {
        println!(
            "  {} Will force-push {} {}",
            "▸".dimmed(),
            summary.branches_to_push.to_string().cyan(),
            if summary.branches_to_push == 1 { "branch" } else { "branches" }
        );
    }
    
    for desc in &summary.description {
        println!("  {} {}", "▸".dimmed(), desc);
    }
}

