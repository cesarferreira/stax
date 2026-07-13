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
use crate::git::{GitRepo, refs};
use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

/// Suffix appended to the entry label for branch-metadata ref backups so
/// they don't collide with the `refs/heads/<branch>` entry for the same
/// branch in the receipt.
pub const METADATA_REF_LABEL_SUFFIX: &str = "@meta";

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

pub(crate) struct ReceiptFinalization {
    pub receipt: OpReceipt,
    pub persistence_error: Option<anyhow::Error>,
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
    #[allow(dead_code)]
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

    /// Plan a branch-metadata ref to be modified (under `refs/branch-metadata/`).
    ///
    /// Used by operations like `fold` that mutate or delete stax metadata. The
    /// metadata blob's OID is captured so that `stax undo` can restore it via
    /// the same `update-ref`-based mechanism it uses for branch heads. Uses
    /// libgit2 (no subprocess) since fold may invoke this in a per-branch loop.
    pub fn plan_metadata_ref(&mut self, repo: &GitRepo, branch: &str) -> Result<()> {
        let oid = refs::metadata_ref_oid(repo.inner(), branch);
        self.receipt.add_metadata_ref(branch, oid.as_deref());
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

    /// Record whether the operation should auto-stash dirty target worktrees.
    pub fn set_auto_stash_pop(&mut self, auto_stash_pop: bool) {
        self.receipt.auto_stash_pop = auto_stash_pop;
    }

    /// Record a branch that completed successfully during this operation.
    pub fn push_completed_branch(&mut self, branch: &str) {
        self.receipt.completed_branches.push(branch.to_string());
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
        let count = self
            .receipt
            .local_refs
            .iter()
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
        self.receipt
            .update_local_ref_after_optional(branch, Some(&oid));
        Ok(())
    }

    /// Record the current after-state for a branch, including an absent ref.
    pub fn record_optional_after(&mut self, repo: &GitRepo, branch: &str) -> Result<()> {
        let oid = match repo.inner().find_branch(branch, git2::BranchType::Local) {
            Ok(reference) => Some(reference.get().peel_to_commit()?.id().to_string()),
            Err(error) if error.code() == git2::ErrorCode::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        self.receipt
            .update_local_ref_after_optional(branch, oid.as_deref());
        Ok(())
    }

    /// Record the branch that should be checked out after redoing the operation.
    pub fn set_head_branch_after(&mut self, branch: &str) {
        self.receipt.head_branch_after = Some(branch.to_string());
    }

    /// Record the after-OID for a branch-metadata ref. Pass `branch` (not the
    /// `@meta` label); the lookup re-derives the label internally. The ref
    /// may be absent (e.g., metadata was deleted) — that's recorded as
    /// `oid_after = None`, which `stax undo` handles by re-creating the ref
    /// from `oid_before`.
    pub fn record_metadata_ref_after(&mut self, repo: &GitRepo, branch: &str) -> Result<()> {
        let oid = refs::metadata_ref_oid(repo.inner(), branch);
        self.receipt
            .update_metadata_ref_after(branch, oid.as_deref());
        Ok(())
    }

    /// Record after-OIDs for all planned branches
    #[allow(dead_code)]
    pub fn record_all_after(&mut self, repo: &GitRepo) -> Result<()> {
        let branches: Vec<String> = self
            .receipt
            .local_refs
            .iter()
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
        self.receipt
            .update_remote_ref_after(remote, branch, local_oid);
    }

    /// Finish the transaction successfully
    pub fn finish_ok(self) -> Result<()> {
        self.finish_ok_with_receipt().map(drop)
    }

    pub(crate) fn finish_ok_preserving_receipt(mut self) -> ReceiptFinalization {
        self.receipt.mark_success();
        let persistence_error = self.receipt.save(&self.git_dir).err();
        self.finished = true;
        ReceiptFinalization {
            receipt: self.receipt.clone(),
            persistence_error,
        }
    }

    pub(crate) fn finish_ok_with_receipt(self) -> Result<OpReceipt> {
        let finalized = self.finish_ok_preserving_receipt();
        match finalized.persistence_error {
            Some(error) => Err(error),
            None => Ok(finalized.receipt),
        }
    }

    /// Finish the transaction with an error
    pub fn finish_err(
        self,
        message: &str,
        failed_step: Option<&str>,
        failed_branch: Option<&str>,
    ) -> Result<()> {
        self.finish_err_with_receipt(message, failed_step, failed_branch)
            .map(drop)
    }

    pub(crate) fn finish_err_with_receipt(
        self,
        message: &str,
        failed_step: Option<&str>,
        failed_branch: Option<&str>,
    ) -> Result<OpReceipt> {
        let finalized = self.finish_err_preserving_receipt(message, failed_step, failed_branch);
        match finalized.persistence_error {
            Some(error) => Err(error),
            None => Ok(finalized.receipt),
        }
    }

    pub(crate) fn finish_err_preserving_receipt(
        mut self,
        message: &str,
        failed_step: Option<&str>,
        failed_branch: Option<&str>,
    ) -> ReceiptFinalization {
        self.receipt
            .mark_failed(message, failed_step, failed_branch);
        let persistence_error = self.receipt.save(&self.git_dir).err();
        self.finished = true;

        if !self.quiet {
            self.print_recovery_hint();
        }

        ReceiptFinalization {
            receipt: self.receipt.clone(),
            persistence_error,
        }
    }

    /// Print the recovery hint after a failure
    fn print_recovery_hint(&self) {
        println!();
        println!("{}", "Your repo is recoverable via:".yellow());
        println!("  {}", "stax undo".cyan());
    }

    /// Get the operation kind
    pub fn kind(&self) -> &OpKind {
        &self.receipt.kind
    }

    /// Check if the transaction has been snapshotted
    #[allow(dead_code)]
    pub fn is_snapshotted(&self) -> bool {
        self.snapshotted
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        // If we snapshotted but didn't finish, mark as failed
        if self.snapshotted && !self.finished {
            self.receipt
                .mark_failed("Transaction dropped without finishing", None, None);
            let _ = self.receipt.save(&self.git_dir);
        }
    }
}

/// Print the plan before executing
pub fn print_plan(_kind: &OpKind, summary: &PlanSummary, quiet: bool) {
    if quiet {
        return;
    }

    if summary.branches_to_rebase > 0 {
        println!(
            "  {} About to rebase {} {}",
            "▸".dimmed(),
            summary.branches_to_rebase.to_string().cyan(),
            if summary.branches_to_rebase == 1 {
                "branch"
            } else {
                "branches"
            }
        );
    }

    if summary.branches_to_push > 0 {
        println!(
            "  {} Will force-push {} {}",
            "▸".dimmed(),
            summary.branches_to_push.to_string().cyan(),
            if summary.branches_to_push == 1 {
                "branch"
            } else {
                "branches"
            }
        );
    }

    for desc in &summary.description {
        println!("  {} {}", "▸".dimmed(), desc);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::ops::receipt::OpStatus;

    fn transaction_for_receipt(
        receipt: OpReceipt,
        git_dir: PathBuf,
        workdir: PathBuf,
    ) -> Transaction {
        Transaction {
            receipt,
            git_dir,
            workdir,
            snapshotted: false,
            finished: false,
            quiet: true,
        }
    }

    #[test]
    fn optional_after_records_an_absent_branch() {
        let temp = tempfile::tempdir().unwrap();
        let repository = git2::Repository::init(temp.path()).unwrap();
        let repo = GitRepo::open_from_path(temp.path()).unwrap();
        let mut receipt = OpReceipt::new(
            "delete-ref".into(),
            OpKind::Delete,
            temp.path().display().to_string(),
            "main".into(),
            "main".into(),
        );
        receipt.add_local_ref("deleted", Some("before"));
        let mut transaction = transaction_for_receipt(
            receipt,
            repository.path().to_path_buf(),
            temp.path().to_path_buf(),
        );

        transaction.record_optional_after(&repo, "deleted").unwrap();
        let finalized = transaction.finish_ok_preserving_receipt();

        let entry = &finalized.receipt.local_refs[0];
        assert!(entry.after_recorded);
        assert_eq!(entry.oid_after, None);
        assert!(finalized.receipt.can_redo());
    }

    #[test]
    fn transaction_records_the_post_operation_checkout_branch() {
        let temp = tempfile::tempdir().unwrap();
        let receipt = OpReceipt::new(
            "rename-head".into(),
            OpKind::Rename,
            temp.path().display().to_string(),
            "main".into(),
            "old".into(),
        );
        let mut transaction = transaction_for_receipt(
            receipt,
            temp.path().to_path_buf(),
            temp.path().to_path_buf(),
        );

        transaction.set_head_branch_after("new");
        let finalized = transaction.finish_ok_preserving_receipt();

        assert_eq!(finalized.receipt.undo_head_branch(), "old");
        assert_eq!(finalized.receipt.redo_head_branch(), "new");
    }

    #[test]
    fn successful_finalization_preserves_receipt_when_persistence_fails() {
        let temp = tempfile::tempdir().unwrap();
        let ops_dir = super::super::ops_dir(temp.path());
        std::fs::create_dir_all(&ops_dir).unwrap();
        let receipt_path = OpReceipt::file_path(temp.path(), "success-save-failure");
        std::fs::create_dir(&receipt_path).unwrap();
        let transaction = Transaction {
            receipt: OpReceipt::new(
                "success-save-failure".into(),
                OpKind::Restack,
                temp.path().display().to_string(),
                "main".into(),
                "feature".into(),
            ),
            git_dir: temp.path().to_path_buf(),
            workdir: temp.path().to_path_buf(),
            snapshotted: true,
            finished: false,
            quiet: true,
        };

        let finalized = transaction.finish_ok_preserving_receipt();

        assert_eq!(finalized.receipt.summary_status(), &OpStatus::Success);
        let persistence_error = finalized.persistence_error.unwrap();
        let io_error = persistence_error.downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(io_error.kind(), std::io::ErrorKind::IsADirectory);
    }

    #[test]
    fn failed_finalization_returns_the_failed_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let transaction = Transaction {
            receipt: OpReceipt::new(
                "failed-receipt".into(),
                OpKind::Restack,
                temp.path().display().to_string(),
                "main".into(),
                "feature".into(),
            ),
            git_dir: temp.path().to_path_buf(),
            workdir: temp.path().to_path_buf(),
            snapshotted: true,
            finished: false,
            quiet: true,
        };

        let receipt = transaction
            .finish_err_with_receipt("conflict", Some("rebase"), Some("feature"))
            .unwrap();

        assert_eq!(receipt.summary_status(), &OpStatus::Failed);
        assert_eq!(receipt.error.unwrap().message, "conflict");
    }
}
