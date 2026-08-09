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
    /// Whether snapshot() has been called at least once
    snapshotted: bool,
    /// Whether the transaction has been finished
    finished: bool,
    /// Whether to print status messages
    quiet: bool,
    /// Number of `local_refs` entries that have already been backed up.
    /// Incremental snapshot() backs up only entries at index >= this value.
    backed_up: usize,
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
            backed_up: 0,
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

    /// Create backup refs and write the in-progress receipt.
    ///
    /// Incremental: each call backs up only the `local_refs` entries that were
    /// added since the previous call.  The snapshot message is printed only on
    /// the first call.  Callers may therefore `plan_branch` → `snapshot` →
    /// `plan_branch` → `snapshot` in a loop; each call is a no-op when no new
    /// entries have been added.
    pub fn snapshot(&mut self) -> Result<()> {
        let first_snapshot = !self.snapshotted;

        // Back up any entries that were planned since the last snapshot.
        let entries_to_back_up: Vec<(String, String)> = self.receipt.local_refs[self.backed_up..]
            .iter()
            .filter_map(|entry| {
                entry
                    .oid_before
                    .as_ref()
                    .map(|oid| (entry.branch.clone(), oid.clone()))
            })
            .collect();

        for (branch, oid) in entries_to_back_up {
            super::create_backup_ref(&self.workdir, &self.receipt.op_id, &branch, &oid)?;
        }

        self.backed_up = self.receipt.local_refs.len();

        // Write the in-progress receipt
        self.receipt.save(&self.git_dir)?;

        if first_snapshot {
            self.snapshotted = true;
            if !self.quiet {
                self.print_snapshot_info();
            }
        }

        Ok(())
    }

    /// Plan a branch + its metadata ref, then immediately snapshot both.
    ///
    /// Convenience helper for sync's per-branch lazy snapshot pattern: each
    /// branch is snapshotted right before it is mutated so that a partial sync
    /// (interrupted by an error or conflict) only records the branches it
    /// actually touched.
    pub fn snapshot_branch_with_metadata(&mut self, repo: &GitRepo, branch: &str) -> Result<()> {
        self.plan_branch(repo, branch)?;
        self.plan_metadata_ref(repo, branch)?;
        self.snapshot()
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

    /// Record a known after-OID for a branch ref without querying the repo.
    ///
    /// Sync resolves after-OIDs via a git subprocess (`resolve_ref_oid`) rather
    /// than libgit2, because libgit2's cached refdb may not see refs written by
    /// subprocesses (e.g. `git update-ref`, `git push`, `git branch -D`).
    /// Call this after resolving the OID yourself; pass `None` to record
    /// explicit deletion.
    pub fn record_known_after(&mut self, branch: &str, oid: Option<&str>) {
        self.receipt.update_local_ref_after_optional(branch, oid);
    }

    /// Record a known after-OID for a branch-metadata ref without querying the
    /// repo.  Same subprocess-vs-libgit2 rationale as `record_known_after`.
    pub fn record_known_metadata_after(&mut self, branch: &str, oid: Option<&str>) {
        self.receipt.update_metadata_ref_after(branch, oid);
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
            backed_up: 0,
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
            backed_up: 0,
        };

        let finalized = transaction.finish_ok_preserving_receipt();

        assert_eq!(finalized.receipt.summary_status(), &OpStatus::Success);
        let persistence_error = finalized.persistence_error.unwrap();
        let io_error = persistence_error.downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(io_error.kind(), std::io::ErrorKind::IsADirectory);
    }

    #[test]
    fn snapshot_backs_up_branches_planned_after_the_first_snapshot() {
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // Minimal git repo with two commits so we have real OIDs.
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .expect("git");
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(path.join("f.txt"), "a").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "first"]);
        run(&["branch", "branch-a"]);
        std::fs::write(path.join("f.txt"), "b").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "second"]);
        run(&["branch", "branch-b"]);

        let repo = GitRepo::open_from_path(path).unwrap();
        let ops_dir = path.join(".git/stax/ops");
        std::fs::create_dir_all(&ops_dir).unwrap();

        let mut tx = Transaction::begin(OpKind::Sync, &repo, true).unwrap();
        tx.plan_branch(&repo, "branch-a").unwrap();
        tx.snapshot().unwrap();

        // First snapshot: only branch-a is backed up (plan_branch adds 1 entry).
        assert_eq!(tx.backed_up, 1);
        let backed_up_after_first = tx.backed_up;

        tx.plan_branch(&repo, "branch-b").unwrap();
        assert!(tx.receipt.local_refs.len() > backed_up_after_first);

        tx.snapshot().unwrap();
        // After second snapshot, backed_up catches up.
        assert_eq!(tx.backed_up, tx.receipt.local_refs.len());

        // Both backup refs must exist as git refs.
        let op_id = tx.receipt.op_id.clone();
        let ref_a = format!("refs/stax/backups/{}/branch-a", op_id);
        let ref_b = format!("refs/stax/backups/{}/branch-b", op_id);
        let ok_a = Command::new("git")
            .args(["rev-parse", "--verify", &ref_a])
            .current_dir(path)
            .output()
            .expect("git")
            .status
            .success();
        let ok_b = Command::new("git")
            .args(["rev-parse", "--verify", &ref_b])
            .current_dir(path)
            .output()
            .expect("git")
            .status
            .success();
        assert!(ok_a, "backup ref for branch-a should exist");
        assert!(ok_b, "backup ref for branch-b should exist");
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
            backed_up: 0,
        };

        let receipt = transaction
            .finish_err_with_receipt("conflict", Some("rebase"), Some("feature"))
            .unwrap();

        assert_eq!(receipt.summary_status(), &OpStatus::Failed);
        assert_eq!(receipt.error.unwrap().message, "conflict");
    }

    #[test]
    fn dropping_a_snapshotted_transaction_marks_the_receipt_failed() {
        let temp = tempfile::tempdir().unwrap();
        let op_id = "drop-snapshotted".to_string();
        let receipt = OpReceipt::new(
            op_id.clone(),
            OpKind::Restack,
            temp.path().display().to_string(),
            "main".into(),
            "feature".into(),
        );
        let transaction = Transaction {
            receipt,
            git_dir: temp.path().to_path_buf(),
            workdir: temp.path().to_path_buf(),
            snapshotted: true,
            finished: false,
            quiet: true,
            backed_up: 0,
        };

        drop(transaction);

        let reloaded = OpReceipt::load(temp.path(), &op_id).unwrap();
        assert_eq!(reloaded.status, OpStatus::Failed);
        assert_eq!(
            reloaded.error.unwrap().message,
            "Transaction dropped without finishing"
        );
    }

    #[test]
    fn dropping_an_unsnapshotted_transaction_leaves_no_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let op_id = "drop-unsnapshotted".to_string();
        let receipt = OpReceipt::new(
            op_id.clone(),
            OpKind::Restack,
            temp.path().display().to_string(),
            "main".into(),
            "feature".into(),
        );
        let transaction = transaction_for_receipt(
            receipt,
            temp.path().to_path_buf(),
            temp.path().to_path_buf(),
        );

        drop(transaction);

        assert!(OpReceipt::load(temp.path(), &op_id).is_err());
    }

    #[test]
    fn plan_metadata_ref_and_record_metadata_ref_after_round_trip() {
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .expect("git");
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(path.join("f.txt"), "a").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "first"]);
        run(&["branch", "branch-a"]);

        let repo = GitRepo::open_from_path(path).unwrap();
        refs::write_metadata(repo.inner(), "branch-a", "{\"before\":true}").unwrap();

        let mut tx = Transaction::begin(OpKind::Fold, &repo, true).unwrap();
        tx.plan_metadata_ref(&repo, "branch-a").unwrap();

        let oid_before = tx
            .receipt
            .local_refs
            .iter()
            .find(|e| e.branch == "branch-a@meta")
            .and_then(|e| e.oid_before.clone())
            .expect("metadata ref should have a before-OID");

        refs::write_metadata(repo.inner(), "branch-a", "{\"after\":true}").unwrap();
        tx.record_metadata_ref_after(&repo, "branch-a").unwrap();

        let finalized = tx.finish_ok_preserving_receipt();
        let entry = finalized
            .receipt
            .local_refs
            .iter()
            .find(|e| e.branch == "branch-a@meta")
            .unwrap();

        assert!(entry.after_recorded);
        assert!(entry.oid_after.is_some());
        assert_ne!(entry.oid_after, Some(oid_before));
    }

    #[test]
    fn record_known_after_records_deletion_as_none() {
        let temp = tempfile::tempdir().unwrap();
        let mut receipt = OpReceipt::new(
            "delete-known".into(),
            OpKind::Delete,
            temp.path().display().to_string(),
            "main".into(),
            "main".into(),
        );
        receipt.add_local_ref("feature", Some("before-oid"));
        let mut transaction = transaction_for_receipt(
            receipt,
            temp.path().to_path_buf(),
            temp.path().to_path_buf(),
        );

        transaction.record_known_after("feature", None);
        let finalized = transaction.finish_ok_preserving_receipt();

        let entry = &finalized.receipt.local_refs[0];
        assert!(entry.after_recorded);
        assert_eq!(entry.oid_after, None);
    }

    #[test]
    fn record_known_metadata_after_records_deletion_as_none() {
        let temp = tempfile::tempdir().unwrap();
        let mut receipt = OpReceipt::new(
            "delete-known-meta".into(),
            OpKind::Fold,
            temp.path().display().to_string(),
            "main".into(),
            "main".into(),
        );
        receipt.add_metadata_ref("feature", Some("before-oid"));
        let mut transaction = transaction_for_receipt(
            receipt,
            temp.path().to_path_buf(),
            temp.path().to_path_buf(),
        );

        transaction.record_known_metadata_after("feature", None);
        let finalized = transaction.finish_ok_preserving_receipt();

        let entry = &finalized.receipt.local_refs[0];
        assert_eq!(entry.branch, "feature@meta");
        assert!(entry.after_recorded);
        assert_eq!(entry.oid_after, None);
    }

    #[test]
    fn plan_remote_branch_and_record_remote_after_round_trip() {
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .expect("git");
        };
        let rev_parse = |rev: &str| -> String {
            let output = Command::new("git")
                .args(["rev-parse", rev])
                .current_dir(path)
                .output()
                .expect("git");
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };

        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(path.join("f.txt"), "a").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "first"]);
        run(&["branch", "branch-a"]);
        let oid_before = rev_parse("branch-a");

        run(&["checkout", "branch-a"]);
        std::fs::write(path.join("f.txt"), "b").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "second"]);
        let oid_after = rev_parse("branch-a");
        assert_ne!(oid_before, oid_after);

        run(&["update-ref", "refs/remotes/origin/branch-a", &oid_before]);

        let repo = GitRepo::open_from_path(path).unwrap();
        let mut tx = Transaction::begin(OpKind::Submit, &repo, true).unwrap();
        tx.plan_remote_branch(&repo, "origin", "branch-a").unwrap();
        tx.record_remote_after("origin", "branch-a", &oid_after);

        let finalized = tx.finish_ok_preserving_receipt();
        let entry = &finalized.receipt.remote_refs[0];
        assert_eq!(entry.remote, "origin");
        assert_eq!(entry.branch, "branch-a");
        assert_eq!(entry.oid_before, Some(oid_before));
        assert_eq!(entry.oid_after, Some(oid_after));
    }

    #[test]
    fn record_all_after_updates_every_planned_branch() {
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .expect("git");
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(path.join("f.txt"), "a").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "first"]);
        run(&["branch", "branch-a"]);
        run(&["branch", "branch-b"]);

        let repo = GitRepo::open_from_path(path).unwrap();
        let mut tx = Transaction::begin(OpKind::Restack, &repo, true).unwrap();
        tx.plan_branch(&repo, "branch-a").unwrap();
        tx.plan_branch(&repo, "branch-b").unwrap();

        run(&["checkout", "branch-a"]);
        std::fs::write(path.join("f.txt"), "b").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "advance-a"]);

        run(&["checkout", "branch-b"]);
        std::fs::write(path.join("f.txt"), "c").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "advance-b"]);

        tx.record_all_after(&repo).unwrap();

        let finalized = tx.finish_ok_preserving_receipt();
        let entry_a = finalized
            .receipt
            .local_refs
            .iter()
            .find(|e| e.branch == "branch-a")
            .unwrap();
        let entry_b = finalized
            .receipt
            .local_refs
            .iter()
            .find(|e| e.branch == "branch-b")
            .unwrap();

        assert!(entry_a.after_recorded);
        assert!(entry_b.after_recorded);
        assert_ne!(entry_a.oid_before, entry_a.oid_after);
        assert_ne!(entry_b.oid_before, entry_b.oid_after);
        assert_ne!(entry_a.oid_after, entry_b.oid_after);
    }

    #[test]
    fn snapshot_branch_with_metadata_plans_branch_and_metadata() {
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .expect("git");
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(path.join("f.txt"), "a").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "first"]);
        run(&["branch", "branch-a"]);

        let repo = GitRepo::open_from_path(path).unwrap();
        refs::write_metadata(repo.inner(), "branch-a", "{\"parent\":\"main\"}").unwrap();

        let mut tx = Transaction::begin(OpKind::Fold, &repo, true).unwrap();
        tx.snapshot_branch_with_metadata(&repo, "branch-a").unwrap();

        assert_eq!(tx.receipt.local_refs.len(), 2);
        let branch_entry = tx
            .receipt
            .local_refs
            .iter()
            .find(|e| e.branch == "branch-a")
            .unwrap();
        let meta_entry = tx
            .receipt
            .local_refs
            .iter()
            .find(|e| e.branch == "branch-a@meta")
            .unwrap();
        assert!(branch_entry.oid_before.is_some());
        assert!(meta_entry.oid_before.is_some());

        let op_id = tx.receipt.op_id.clone();
        let git_dir = repo.git_dir().unwrap().to_path_buf();
        let reloaded = OpReceipt::load(&git_dir, &op_id).unwrap();
        assert_eq!(reloaded.local_refs.len(), 2);

        let backup_branch_ref = format!("refs/stax/backups/{}/branch-a", op_id);
        let backup_meta_ref = format!("refs/stax/backups/{}/branch-a@meta", op_id);
        let ok_branch = Command::new("git")
            .args(["rev-parse", "--verify", &backup_branch_ref])
            .current_dir(path)
            .output()
            .expect("git")
            .status
            .success();
        let ok_meta = Command::new("git")
            .args(["rev-parse", "--verify", &backup_meta_ref])
            .current_dir(path)
            .output()
            .expect("git")
            .status
            .success();
        assert!(ok_branch, "backup ref for branch-a should exist");
        assert!(ok_meta, "backup ref for branch-a@meta should exist");
    }

    #[test]
    fn set_plan_summary_and_auto_stash_pop_persist_into_the_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let op_id = "plan-summary-and-stash".to_string();
        let receipt = OpReceipt::new(
            op_id.clone(),
            OpKind::Sync,
            temp.path().display().to_string(),
            "main".into(),
            "feature".into(),
        );
        let mut transaction = transaction_for_receipt(
            receipt,
            temp.path().to_path_buf(),
            temp.path().to_path_buf(),
        );

        transaction.set_plan_summary(PlanSummary {
            branches_to_rebase: 3,
            branches_to_push: 2,
            description: vec!["rebase feature onto main".to_string()],
        });
        transaction.set_auto_stash_pop(true);
        let finalized = transaction.finish_ok_preserving_receipt();
        assert!(finalized.persistence_error.is_none());

        let reloaded = OpReceipt::load(temp.path(), &op_id).unwrap();
        assert_eq!(reloaded.plan_summary.branches_to_rebase, 3);
        assert_eq!(reloaded.plan_summary.branches_to_push, 2);
        assert_eq!(
            reloaded.plan_summary.description,
            vec!["rebase feature onto main".to_string()]
        );
        assert!(reloaded.auto_stash_pop);
    }

    #[test]
    fn push_completed_branch_accumulates_in_order() {
        let temp = tempfile::tempdir().unwrap();
        let op_id = "completed-branches".to_string();
        let receipt = OpReceipt::new(
            op_id.clone(),
            OpKind::Sync,
            temp.path().display().to_string(),
            "main".into(),
            "feature".into(),
        );
        let mut transaction = transaction_for_receipt(
            receipt,
            temp.path().to_path_buf(),
            temp.path().to_path_buf(),
        );

        transaction.push_completed_branch("a");
        transaction.push_completed_branch("b");
        transaction.push_completed_branch("c");
        let finalized = transaction.finish_ok_preserving_receipt();

        assert_eq!(
            finalized.receipt.completed_branches,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );

        let reloaded = OpReceipt::load(temp.path(), &op_id).unwrap();
        assert_eq!(
            reloaded.completed_branches,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }
}
