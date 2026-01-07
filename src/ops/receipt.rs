//! Operation receipt persistence.
//!
//! Receipts are stored as JSON files under `.git/stax/ops/<op-id>.json`

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Status of an operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OpStatus {
    InProgress,
    Success,
    Failed,
}

/// Kind of operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Restack,
    UpstackRestack,
    SyncRestack,
    Submit,
    Reorder,
    Split,
}

impl OpKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            OpKind::Restack => "restack",
            OpKind::UpstackRestack => "upstack restack",
            OpKind::SyncRestack => "sync --restack",
            OpKind::Submit => "submit",
            OpKind::Reorder => "reorder",
            OpKind::Split => "split",
        }
    }
}

/// Information about a local ref that was modified
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRefEntry {
    /// Branch name (without refs/heads/)
    pub branch: String,
    /// Full ref name (e.g., refs/heads/feature/foo)
    pub refname: String,
    /// Whether the ref existed before the operation
    pub existed_before: bool,
    /// OID before the operation (None if didn't exist)
    pub oid_before: Option<String>,
    /// OID after the operation (filled in on success)
    pub oid_after: Option<String>,
}

/// Information about a remote ref that was modified (for submit)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRefEntry {
    /// Remote name (e.g., "origin")
    pub remote: String,
    /// Branch name
    pub branch: String,
    /// Full remote ref name (e.g., refs/remotes/origin/feature/foo)
    pub remote_refname: String,
    /// OID on remote before push (None if didn't exist)
    pub oid_before: Option<String>,
    /// OID pushed (the local OID that was force-pushed)
    pub oid_after: Option<String>,
}

/// Error information for failed operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpError {
    pub message: String,
    pub failed_step: Option<String>,
    pub failed_branch: Option<String>,
}

/// Plan summary for display
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanSummary {
    /// Number of branches to rebase
    pub branches_to_rebase: usize,
    /// Number of branches to force-push
    pub branches_to_push: usize,
    /// Human-readable description bullets
    pub description: Vec<String>,
}

/// Operation receipt - persisted to `.git/stax/ops/<op-id>.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpReceipt {
    /// Unique operation ID
    pub op_id: String,
    /// Kind of operation
    pub kind: OpKind,
    /// When operation started (ISO 8601)
    pub started_at: String,
    /// When operation finished (ISO 8601), None if still in progress
    pub finished_at: Option<String>,
    /// Current status
    pub status: OpStatus,
    /// Repository working directory (for verification)
    pub repo_workdir: String,
    /// Trunk branch name
    pub trunk: String,
    /// Branch that was checked out when operation started
    pub head_branch_before: String,
    /// Local refs that were/will be modified
    pub local_refs: Vec<LocalRefEntry>,
    /// Remote refs that were/will be modified (for submit)
    pub remote_refs: Vec<RemoteRefEntry>,
    /// Plan summary for display
    pub plan_summary: PlanSummary,
    /// Error information if failed
    pub error: Option<OpError>,
}

impl OpReceipt {
    /// Create a new receipt for an operation that's about to start
    pub fn new(
        op_id: String,
        kind: OpKind,
        repo_workdir: String,
        trunk: String,
        head_branch_before: String,
    ) -> Self {
        let started_at = chrono::Utc::now().to_rfc3339();
        
        Self {
            op_id,
            kind,
            started_at,
            finished_at: None,
            status: OpStatus::InProgress,
            repo_workdir,
            trunk,
            head_branch_before,
            local_refs: Vec::new(),
            remote_refs: Vec::new(),
            plan_summary: PlanSummary::default(),
            error: None,
        }
    }
    
    /// Add a local ref to track
    pub fn add_local_ref(&mut self, branch: &str, oid_before: Option<&str>) {
        self.local_refs.push(LocalRefEntry {
            branch: branch.to_string(),
            refname: format!("refs/heads/{}", branch),
            existed_before: oid_before.is_some(),
            oid_before: oid_before.map(|s| s.to_string()),
            oid_after: None,
        });
    }
    
    /// Add a remote ref to track
    pub fn add_remote_ref(&mut self, remote: &str, branch: &str, oid_before: Option<&str>) {
        self.remote_refs.push(RemoteRefEntry {
            remote: remote.to_string(),
            branch: branch.to_string(),
            remote_refname: format!("refs/remotes/{}/{}", remote, branch),
            oid_before: oid_before.map(|s| s.to_string()),
            oid_after: None,
        });
    }
    
    /// Update the after-OID for a local ref
    pub fn update_local_ref_after(&mut self, branch: &str, oid_after: &str) {
        if let Some(entry) = self.local_refs.iter_mut().find(|e| e.branch == branch) {
            entry.oid_after = Some(oid_after.to_string());
        }
    }
    
    /// Update the after-OID for a remote ref
    pub fn update_remote_ref_after(&mut self, remote: &str, branch: &str, oid_after: &str) {
        if let Some(entry) = self.remote_refs.iter_mut().find(|e| e.remote == remote && e.branch == branch) {
            entry.oid_after = Some(oid_after.to_string());
        }
    }
    
    /// Mark operation as successful
    pub fn mark_success(&mut self) {
        self.status = OpStatus::Success;
        self.finished_at = Some(chrono::Utc::now().to_rfc3339());
    }
    
    /// Mark operation as failed
    pub fn mark_failed(&mut self, message: &str, failed_step: Option<&str>, failed_branch: Option<&str>) {
        self.status = OpStatus::Failed;
        self.finished_at = Some(chrono::Utc::now().to_rfc3339());
        self.error = Some(OpError {
            message: message.to_string(),
            failed_step: failed_step.map(|s| s.to_string()),
            failed_branch: failed_branch.map(|s| s.to_string()),
        });
    }
    
    /// Get the receipt file path
    pub fn file_path(git_dir: &Path, op_id: &str) -> std::path::PathBuf {
        super::ops_dir(git_dir).join(format!("{}.json", op_id))
    }
    
    /// Save receipt to disk
    pub fn save(&self, git_dir: &Path) -> Result<()> {
        super::ensure_ops_dir(git_dir)?;
        let path = Self::file_path(git_dir, &self.op_id);
        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize receipt")?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write receipt: {}", path.display()))?;
        Ok(())
    }
    
    /// Load receipt from disk
    pub fn load(git_dir: &Path, op_id: &str) -> Result<Self> {
        let path = Self::file_path(git_dir, op_id);
        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read receipt: {}", path.display()))?;
        let receipt: Self = serde_json::from_str(&json)
            .with_context(|| format!("Failed to parse receipt: {}", path.display()))?;
        Ok(receipt)
    }
    
    /// Load the latest receipt
    pub fn load_latest(git_dir: &Path) -> Result<Option<Self>> {
        match super::latest_op_id(git_dir)? {
            Some(op_id) => Ok(Some(Self::load(git_dir, &op_id)?)),
            None => Ok(None),
        }
    }
    
    /// Check if this receipt can be undone
    pub fn can_undo(&self) -> bool {
        // Can undo if we have local refs with before-OIDs
        self.local_refs.iter().any(|r| r.oid_before.is_some())
    }
    
    /// Check if this receipt can be redone
    pub fn can_redo(&self) -> bool {
        // Can redo if we have local refs with after-OIDs
        self.local_refs.iter().any(|r| r.oid_after.is_some())
    }
    
    /// Check if this receipt has remote changes
    pub fn has_remote_changes(&self) -> bool {
        !self.remote_refs.is_empty()
    }
    
    /// Count branches that were actually modified
    #[allow(dead_code)]
    pub fn modified_branch_count(&self) -> usize {
        self.local_refs.iter()
            .filter(|r| r.oid_before != r.oid_after)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_receipt_roundtrip() {
        let mut receipt = OpReceipt::new(
            "20251229T120500Z-abc123".to_string(),
            OpKind::Restack,
            "/tmp/repo".to_string(),
            "main".to_string(),
            "feature/foo".to_string(),
        );

        receipt.add_local_ref("feature/foo", Some("abc123"));
        receipt.update_local_ref_after("feature/foo", "def456");
        receipt.mark_success();

        let json = serde_json::to_string(&receipt).unwrap();
        let loaded: OpReceipt = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.op_id, receipt.op_id);
        assert_eq!(loaded.status, OpStatus::Success);
        assert_eq!(loaded.local_refs.len(), 1);
        assert_eq!(loaded.local_refs[0].oid_before, Some("abc123".to_string()));
        assert_eq!(loaded.local_refs[0].oid_after, Some("def456".to_string()));
    }

    #[test]
    fn test_op_kind_display_name() {
        assert_eq!(OpKind::Restack.display_name(), "restack");
        assert_eq!(OpKind::UpstackRestack.display_name(), "upstack restack");
        assert_eq!(OpKind::SyncRestack.display_name(), "sync --restack");
        assert_eq!(OpKind::Submit.display_name(), "submit");
        assert_eq!(OpKind::Reorder.display_name(), "reorder");
    }

    #[test]
    fn test_op_status_serialization() {
        let status = OpStatus::InProgress;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"in_progress\"");

        let status = OpStatus::Success;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"success\"");

        let status = OpStatus::Failed;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"failed\"");
    }

    #[test]
    fn test_op_kind_serialization() {
        let kind = OpKind::Restack;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"restack\"");

        let kind = OpKind::UpstackRestack;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"upstack_restack\"");
    }

    #[test]
    fn test_receipt_new() {
        let receipt = OpReceipt::new(
            "test-op-id".to_string(),
            OpKind::Submit,
            "/tmp/repo".to_string(),
            "main".to_string(),
            "feature".to_string(),
        );

        assert_eq!(receipt.op_id, "test-op-id");
        assert!(matches!(receipt.kind, OpKind::Submit));
        assert!(matches!(receipt.status, OpStatus::InProgress));
        assert!(receipt.finished_at.is_none());
        assert!(receipt.local_refs.is_empty());
        assert!(receipt.remote_refs.is_empty());
        assert!(receipt.error.is_none());
    }

    #[test]
    fn test_add_local_ref() {
        let mut receipt = OpReceipt::new(
            "test".to_string(),
            OpKind::Restack,
            "/tmp".to_string(),
            "main".to_string(),
            "feature".to_string(),
        );

        receipt.add_local_ref("feature/foo", Some("abc123"));

        assert_eq!(receipt.local_refs.len(), 1);
        assert_eq!(receipt.local_refs[0].branch, "feature/foo");
        assert_eq!(receipt.local_refs[0].refname, "refs/heads/feature/foo");
        assert!(receipt.local_refs[0].existed_before);
        assert_eq!(receipt.local_refs[0].oid_before, Some("abc123".to_string()));
    }

    #[test]
    fn test_add_local_ref_new_branch() {
        let mut receipt = OpReceipt::new(
            "test".to_string(),
            OpKind::Restack,
            "/tmp".to_string(),
            "main".to_string(),
            "feature".to_string(),
        );

        receipt.add_local_ref("new-branch", None);

        assert_eq!(receipt.local_refs.len(), 1);
        assert!(!receipt.local_refs[0].existed_before);
        assert!(receipt.local_refs[0].oid_before.is_none());
    }

    #[test]
    fn test_add_remote_ref() {
        let mut receipt = OpReceipt::new(
            "test".to_string(),
            OpKind::Submit,
            "/tmp".to_string(),
            "main".to_string(),
            "feature".to_string(),
        );

        receipt.add_remote_ref("origin", "feature/foo", Some("abc123"));

        assert_eq!(receipt.remote_refs.len(), 1);
        assert_eq!(receipt.remote_refs[0].remote, "origin");
        assert_eq!(receipt.remote_refs[0].branch, "feature/foo");
        assert_eq!(receipt.remote_refs[0].remote_refname, "refs/remotes/origin/feature/foo");
    }

    #[test]
    fn test_update_remote_ref_after() {
        let mut receipt = OpReceipt::new(
            "test".to_string(),
            OpKind::Submit,
            "/tmp".to_string(),
            "main".to_string(),
            "feature".to_string(),
        );

        receipt.add_remote_ref("origin", "feature", Some("abc123"));
        receipt.update_remote_ref_after("origin", "feature", "def456");

        assert_eq!(receipt.remote_refs[0].oid_after, Some("def456".to_string()));
    }

    #[test]
    fn test_mark_failed() {
        let mut receipt = OpReceipt::new(
            "test".to_string(),
            OpKind::Restack,
            "/tmp".to_string(),
            "main".to_string(),
            "feature".to_string(),
        );

        receipt.mark_failed("Conflict detected", Some("rebase"), Some("feature/foo"));

        assert!(matches!(receipt.status, OpStatus::Failed));
        assert!(receipt.finished_at.is_some());
        assert!(receipt.error.is_some());

        let error = receipt.error.unwrap();
        assert_eq!(error.message, "Conflict detected");
        assert_eq!(error.failed_step, Some("rebase".to_string()));
        assert_eq!(error.failed_branch, Some("feature/foo".to_string()));
    }

    #[test]
    fn test_can_undo() {
        let mut receipt = OpReceipt::new(
            "test".to_string(),
            OpKind::Restack,
            "/tmp".to_string(),
            "main".to_string(),
            "feature".to_string(),
        );

        // Initially no refs, can't undo
        assert!(!receipt.can_undo());

        // Add ref with before OID
        receipt.add_local_ref("feature", Some("abc123"));
        assert!(receipt.can_undo());

        // Clear and add ref without before OID (new branch)
        receipt.local_refs.clear();
        receipt.add_local_ref("new-branch", None);
        assert!(!receipt.can_undo());
    }

    #[test]
    fn test_can_redo() {
        let mut receipt = OpReceipt::new(
            "test".to_string(),
            OpKind::Restack,
            "/tmp".to_string(),
            "main".to_string(),
            "feature".to_string(),
        );

        receipt.add_local_ref("feature", Some("abc123"));

        // No after OID yet
        assert!(!receipt.can_redo());

        // Add after OID
        receipt.update_local_ref_after("feature", "def456");
        assert!(receipt.can_redo());
    }

    #[test]
    fn test_has_remote_changes() {
        let mut receipt = OpReceipt::new(
            "test".to_string(),
            OpKind::Submit,
            "/tmp".to_string(),
            "main".to_string(),
            "feature".to_string(),
        );

        assert!(!receipt.has_remote_changes());

        receipt.add_remote_ref("origin", "feature", Some("abc123"));
        assert!(receipt.has_remote_changes());
    }

    #[test]
    fn test_modified_branch_count() {
        let mut receipt = OpReceipt::new(
            "test".to_string(),
            OpKind::Restack,
            "/tmp".to_string(),
            "main".to_string(),
            "feature".to_string(),
        );

        receipt.add_local_ref("feature-a", Some("abc123"));
        receipt.add_local_ref("feature-b", Some("abc123"));
        receipt.add_local_ref("feature-c", Some("xyz789"));

        // Before any updates, all modified (before != after which is None)
        assert_eq!(receipt.modified_branch_count(), 3);

        // Update one to same value (not modified)
        receipt.update_local_ref_after("feature-a", "abc123");
        assert_eq!(receipt.modified_branch_count(), 2);

        // Update one to different value (still modified)
        receipt.update_local_ref_after("feature-b", "def456");
        assert_eq!(receipt.modified_branch_count(), 2);
    }

    #[test]
    fn test_plan_summary_default() {
        let summary = PlanSummary::default();
        assert_eq!(summary.branches_to_rebase, 0);
        assert_eq!(summary.branches_to_push, 0);
        assert!(summary.description.is_empty());
    }

    #[test]
    fn test_local_ref_entry_clone() {
        let entry = LocalRefEntry {
            branch: "feature".to_string(),
            refname: "refs/heads/feature".to_string(),
            existed_before: true,
            oid_before: Some("abc123".to_string()),
            oid_after: Some("def456".to_string()),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.branch, "feature");
        assert_eq!(cloned.oid_before, entry.oid_before);
    }

    #[test]
    fn test_remote_ref_entry_clone() {
        let entry = RemoteRefEntry {
            remote: "origin".to_string(),
            branch: "feature".to_string(),
            remote_refname: "refs/remotes/origin/feature".to_string(),
            oid_before: Some("abc123".to_string()),
            oid_after: Some("def456".to_string()),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.remote, "origin");
        assert_eq!(cloned.branch, "feature");
    }

    #[test]
    fn test_op_error_clone() {
        let error = OpError {
            message: "Test error".to_string(),
            failed_step: Some("rebase".to_string()),
            failed_branch: Some("feature".to_string()),
        };
        let cloned = error.clone();
        assert_eq!(cloned.message, "Test error");
    }
}

