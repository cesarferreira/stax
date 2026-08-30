use crate::git::refs;
use anyhow::Result;
use git2::Repository;
use serde::{Deserialize, Serialize};

/// Metadata stored for each tracked branch
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchMetadata {
    /// Name of the parent branch
    #[serde(default)]
    pub parent_branch_name: String,
    /// Commit SHA of parent when this branch was last rebased
    #[serde(default)]
    pub parent_branch_revision: String,
    /// Remote this branch was imported from with `stax get`.
    ///
    /// Imported branches are tracked so local branches can stack on top of
    /// them, but submit should not push or update their PRs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_remote: Option<String>,
    /// Protect this branch from history-rewriting bulk operations.
    #[serde(default)]
    pub frozen: bool,
    /// PR information (if submitted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_info: Option<PrInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
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
            source_remote: None,
            frozen: false,
            pr_info: None,
        }
    }

    /// Read metadata for a branch from git refs
    pub fn read(repo: &Repository, branch: &str) -> Result<Option<Self>> {
        match refs::read_metadata(repo, branch)? {
            Some(json) => {
                let mut meta: Self = serde_json::from_str(&json)?;

                // Backward/partial-compatibility guard:
                // Some historical/broken metadata records may miss parent fields.
                if meta.parent_branch_name.trim().is_empty() {
                    meta.parent_branch_name =
                        refs::read_trunk(repo)?.unwrap_or_else(|| "main".to_string());
                }

                if meta.parent_branch_revision.trim().is_empty()
                    && let Ok(parent_ref) =
                        repo.find_branch(&meta.parent_branch_name, git2::BranchType::Local)
                    && let Ok(commit) = parent_ref.get().peel_to_commit()
                {
                    meta.parent_branch_revision = commit.id().to_string();
                }

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

    /// Whether a tracked branch is protected from restack operations.
    pub fn is_frozen(repo: &Repository, branch: &str) -> Result<bool> {
        Ok(Self::read(repo, branch)?.is_some_and(|metadata| metadata.frozen))
    }

    /// Check if the branch needs restacking (parent has moved).
    ///
    /// The recorded `parent_branch_revision` can be a lie: an interrupted restack may have
    /// written the new base before being rolled back, leaving a SHA that no longer sits under
    /// the branch's tip. A bare SHA comparison would then report the branch clean forever, so
    /// when the SHAs match we additionally verify the recorded base is still reachable from the
    /// branch head.
    pub fn needs_restack(&self, repo: &Repository, branch: &str) -> Result<bool> {
        if self.source_remote.is_some() {
            return Ok(false);
        }

        let parent_ref = repo.find_branch(&self.parent_branch_name, git2::BranchType::Local)?;
        let current_parent_rev = parent_ref.get().peel_to_commit()?.id().to_string();
        if current_parent_rev != self.parent_branch_revision {
            return Ok(true);
        }

        Ok(!Self::recorded_base_is_reachable(
            repo,
            branch,
            &self.parent_branch_revision,
        ))
    }

    /// Whether `revision` is reachable from `branch`'s tip.
    ///
    /// Conservative: any lookup failure (unknown branch, unparsable or missing revision)
    /// returns `true` so unrelated repo problems never surface as a spurious "needs restack".
    fn recorded_base_is_reachable(repo: &Repository, branch: &str, revision: &str) -> bool {
        let Ok(base_oid) = git2::Oid::from_str(revision) else {
            return true;
        };
        let Ok(branch_ref) = repo.find_branch(branch, git2::BranchType::Local) else {
            return true;
        };
        let Ok(branch_oid) = branch_ref.get().peel_to_commit().map(|commit| commit.id()) else {
            return true;
        };
        if branch_oid == base_oid {
            return true;
        }
        if repo
            .graph_descendant_of(base_oid, branch_oid)
            .unwrap_or(false)
        {
            return true;
        }
        repo.graph_descendant_of(branch_oid, base_oid)
            .unwrap_or(true)
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
        assert!(meta.source_remote.is_none());
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
    fn test_metadata_deserialization_missing_parent_fields_uses_defaults() {
        let json = r#"{
            "prInfo": {
                "number": 99,
                "state": "OPEN"
            }
        }"#;
        let meta: BranchMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.parent_branch_name, "");
        assert_eq!(meta.parent_branch_revision, "");
        assert!(meta.pr_info.is_some());
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

#[cfg(all(test, unix))]
mod git_tests {
    use super::*;
    use std::process::Command;

    /// Sets up a repo with `main` at c1, `feature` branched off c1 with its own
    /// commit, then advances `main` to c2. Returns `(tempdir, repo, c2)`; the
    /// tempdir must be kept alive for the duration of the test.
    fn repo_with_feature_branch_and_advanced_main() -> (tempfile::TempDir, git2::Repository, String)
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .expect("git");
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };

        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(path.join("f.txt"), "c1").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "c1"]);
        run(&["branch", "feature"]);

        run(&["checkout", "feature"]);
        std::fs::write(path.join("feature.txt"), "on feature").unwrap();
        run(&["add", "feature.txt"]);
        run(&["commit", "-m", "feature commit"]);

        run(&["checkout", "main"]);
        std::fs::write(path.join("f.txt"), "c2").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-m", "c2"]);

        let output = Command::new("git")
            .args(["rev-parse", "main"])
            .current_dir(path)
            .output()
            .expect("git");
        let c2 = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let repo = git2::Repository::open(path).unwrap();
        (dir, repo, c2)
    }

    #[test]
    fn needs_restack_detects_a_recorded_parent_revision_that_is_not_an_ancestor() {
        let (_dir, repo, c2) = repo_with_feature_branch_and_advanced_main();

        // The lie: SHAs match the parent tip, but c2 is not reachable from feature.
        let meta = BranchMetadata::new("main", &c2);

        assert!(meta.needs_restack(&repo, "feature").unwrap());
    }

    #[test]
    fn needs_restack_is_false_when_the_branch_sits_on_the_recorded_parent_tip() {
        let (_dir, repo, c2) = repo_with_feature_branch_and_advanced_main();
        let path = repo.workdir().unwrap().to_path_buf();

        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(&path)
                .output()
                .expect("git");
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["checkout", "feature"]);
        run(&["rebase", "main"]);

        let meta = BranchMetadata::new("main", &c2);

        assert!(!meta.needs_restack(&repo, "feature").unwrap());
    }
}
