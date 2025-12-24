use crate::git::refs;
use anyhow::Result;
use git2::Repository;
use serde::{Deserialize, Serialize};

/// Metadata stored for each tracked branch
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchMetadata {
    /// Name of the parent branch
    pub parent_branch_name: String,
    /// Commit SHA of parent when this branch was last rebased
    pub parent_branch_revision: String,
    /// PR information (if submitted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_info: Option<PrInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrInfo {
    #[serde(default)]
    pub number: u64,
    #[serde(default)]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,
}

impl BranchMetadata {
    /// Create new metadata for a branch
    pub fn new(parent_name: &str, parent_revision: &str) -> Self {
        Self {
            parent_branch_name: parent_name.to_string(),
            parent_branch_revision: parent_revision.to_string(),
            pr_info: None,
        }
    }

    /// Read metadata for a branch from git refs
    pub fn read(repo: &Repository, branch: &str) -> Result<Option<Self>> {
        match refs::read_metadata(repo, branch)? {
            Some(json) => {
                let meta: Self = serde_json::from_str(&json)?;
                Ok(Some(meta))
            }
            None => Ok(None),
        }
    }

    /// Write metadata for a branch to git refs
    pub fn write(&self, repo: &Repository, branch: &str) -> Result<()> {
        let json = serde_json::to_string(self)?;
        refs::write_metadata(repo, branch, &json)
    }

    /// Delete metadata for a branch
    pub fn delete(repo: &Repository, branch: &str) -> Result<()> {
        refs::delete_metadata(repo, branch)
    }

    /// Check if the branch needs restacking (parent has moved)
    pub fn needs_restack(&self, repo: &Repository) -> Result<bool> {
        let parent_ref = repo.find_branch(&self.parent_branch_name, git2::BranchType::Local)?;
        let current_parent_rev = parent_ref.get().peel_to_commit()?.id().to_string();
        Ok(current_parent_rev != self.parent_branch_revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_new() {
        let meta = BranchMetadata::new("main", "abc123");
        assert_eq!(meta.parent_branch_name, "main");
        assert_eq!(meta.parent_branch_revision, "abc123");
        assert!(meta.pr_info.is_none());
    }

    #[test]
    fn test_metadata_serialization() {
        let meta = BranchMetadata::new("main", "abc123");
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("parentBranchName"));
        assert!(json.contains("main"));
    }

    #[test]
    fn test_metadata_deserialization() {
        let json = r#"{"parentBranchName":"main","parentBranchRevision":"abc123"}"#;
        let meta: BranchMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.parent_branch_name, "main");
        assert_eq!(meta.parent_branch_revision, "abc123");
    }

    #[test]
    fn test_metadata_with_pr_info() {
        let json = r#"{
            "parentBranchName": "main",
            "parentBranchRevision": "abc123",
            "prInfo": {
                "number": 42,
                "state": "OPEN",
                "isDraft": false
            }
        }"#;
        let meta: BranchMetadata = serde_json::from_str(json).unwrap();
        assert!(meta.pr_info.is_some());
        let pr = meta.pr_info.unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.state, "OPEN");
    }

    #[test]
    fn test_freephite_compatibility() {
        // This JSON format matches freephite's metadata format
        let freephite_json = r#"{
            "parentBranchName": "main",
            "parentBranchRevision": "deadbeef1234567890",
            "prInfo": {
                "number": 123,
                "state": "OPEN",
                "isDraft": true
            }
        }"#;
        let meta: BranchMetadata = serde_json::from_str(freephite_json).unwrap();
        assert_eq!(meta.parent_branch_name, "main");
        assert_eq!(meta.parent_branch_revision, "deadbeef1234567890");
    }
}
