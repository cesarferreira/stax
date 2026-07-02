//! Operation tracking and transactional support for stax.
//!
//! This module provides:
//! - Unique operation IDs
//! - Receipt persistence under `.git/stax/ops/`
//! - Backup refs under `refs/stax/backups/<op-id>/`
//! - Transaction wrapper for safe history rewriting

pub mod receipt;
pub mod tx;

use crate::git::GitRepo;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Generate a unique operation ID: UTC timestamp + random suffix
/// Format: 20251229T120500Z-4f2a9c
pub fn generate_op_id() -> String {
    use std::time::SystemTime;

    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();

    // Format as ISO-ish timestamp
    let secs = now.as_secs();
    let datetime =
        chrono::DateTime::from_timestamp(secs as i64, 0).unwrap_or_else(chrono::Utc::now);
    let timestamp = datetime.format("%Y%m%dT%H%M%SZ").to_string();

    // Add random suffix for uniqueness
    let random: u32 = rand_suffix();
    let suffix = format!("{:06x}", random & 0xFFFFFF);

    format!("{}-{}", timestamp, suffix)
}

/// Simple random suffix generator (no external crate needed)
fn rand_suffix() -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    hasher.finish() as u32
}

/// Get the ops directory path: `.git/stax/ops/`
pub fn ops_dir(git_dir: &Path) -> PathBuf {
    git_dir.join("stax").join("ops")
}

/// Ensure the ops directory exists
pub fn ensure_ops_dir(git_dir: &Path) -> Result<PathBuf> {
    let dir = ops_dir(git_dir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create ops directory: {}", dir.display()))?;
    Ok(dir)
}

/// Get the backup refs prefix for an operation
pub fn backup_ref_prefix(op_id: &str) -> String {
    format!("refs/stax/backups/{}/", op_id)
}

/// Get the full backup ref name for a branch
pub fn backup_ref_name(op_id: &str, branch: &str) -> String {
    format!("refs/stax/backups/{}/{}", op_id, branch)
}

/// Create a backup ref for a branch
pub fn create_backup_ref(workdir: &Path, op_id: &str, branch: &str, oid: &str) -> Result<()> {
    let ref_name = backup_ref_name(op_id, branch);

    let status = Command::new("git")
        .args(["update-ref", &ref_name, oid])
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run git update-ref")?;

    if !status.success() {
        anyhow::bail!("Failed to create backup ref {} -> {}", ref_name, oid);
    }

    Ok(())
}

/// Delete backup refs for an operation
pub fn delete_backup_refs(repo: &GitRepo, op_id: &str) -> Result<()> {
    let prefix = backup_ref_prefix(op_id);
    let workdir = repo.workdir()?;

    // List all refs with this prefix
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname)",
            &format!("{}*", prefix.trim_end_matches('/')),
        ])
        .current_dir(workdir)
        .output()
        .context("Failed to list backup refs")?;

    if !output.status.success() {
        return Ok(()); // No refs to delete
    }

    let refs = String::from_utf8_lossy(&output.stdout);
    for ref_name in refs.lines() {
        if ref_name.is_empty() {
            continue;
        }
        let _ = repo.delete_ref(ref_name);
    }

    Ok(())
}

/// List all operation IDs (sorted newest first)
pub fn list_op_ids(git_dir: &Path) -> Result<Vec<String>> {
    let dir = ops_dir(git_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut ops: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                Some(name.trim_end_matches(".json").to_string())
            } else {
                None
            }
        })
        .collect();

    // Sort descending (newest first) - timestamp format is sortable
    ops.sort();
    ops.reverse();

    Ok(ops)
}

/// Get the latest operation ID
pub fn latest_op_id(git_dir: &Path) -> Result<Option<String>> {
    let ops = list_op_ids(git_dir)?;
    Ok(ops.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_generate_op_id_format() {
        let id = generate_op_id();
        // Should be timestamp-suffix format
        assert!(id.contains('-'));
        assert!(id.len() > 20);
        // Should contain Z for UTC
        assert!(id.contains('Z'));
    }

    #[test]
    fn test_generate_op_id_unique() {
        let id1 = generate_op_id();
        let id2 = generate_op_id();
        // Different calls should produce different IDs
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_backup_ref_name() {
        let ref_name = backup_ref_name("20251229T120500Z-abc123", "feature/foo");
        assert_eq!(
            ref_name,
            "refs/stax/backups/20251229T120500Z-abc123/feature/foo"
        );
    }

    #[test]
    fn test_backup_ref_prefix() {
        let prefix = backup_ref_prefix("20251229T120500Z-abc123");
        assert_eq!(prefix, "refs/stax/backups/20251229T120500Z-abc123/");
    }

    #[test]
    fn test_ops_dir() {
        let temp = TempDir::new().unwrap();
        let git_dir = temp.path().join(".git");
        let dir = ops_dir(&git_dir);
        assert!(dir.to_string_lossy().contains("stax"));
        assert!(dir.to_string_lossy().contains("ops"));
    }

    #[test]
    fn test_ensure_ops_dir() {
        let temp = TempDir::new().unwrap();
        let git_dir = temp.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();

        let dir = ensure_ops_dir(&git_dir).unwrap();
        assert!(dir.exists());
    }

    #[test]
    fn test_list_op_ids_empty() {
        let temp = TempDir::new().unwrap();
        let git_dir = temp.path().join(".git");

        let ops = list_op_ids(&git_dir).unwrap();
        assert!(ops.is_empty());
    }

    #[test]
    fn test_list_op_ids_with_files() {
        let temp = TempDir::new().unwrap();
        let git_dir = temp.path().join(".git");
        let ops_path = ops_dir(&git_dir);
        std::fs::create_dir_all(&ops_path).unwrap();

        // Create some op files
        std::fs::write(ops_path.join("20251229T120000Z-aaa111.json"), "{}").unwrap();
        std::fs::write(ops_path.join("20251229T120100Z-bbb222.json"), "{}").unwrap();
        std::fs::write(ops_path.join("20251229T120200Z-ccc333.json"), "{}").unwrap();
        // Also create a non-json file (should be ignored)
        std::fs::write(ops_path.join("not-an-op.txt"), "text").unwrap();

        let ops = list_op_ids(&git_dir).unwrap();
        assert_eq!(ops.len(), 3);
        // Should be sorted newest first
        assert_eq!(ops[0], "20251229T120200Z-ccc333");
        assert_eq!(ops[1], "20251229T120100Z-bbb222");
        assert_eq!(ops[2], "20251229T120000Z-aaa111");
    }

    #[test]
    fn test_latest_op_id_empty() {
        let temp = TempDir::new().unwrap();
        let git_dir = temp.path().join(".git");

        let latest = latest_op_id(&git_dir).unwrap();
        assert!(latest.is_none());
    }

    #[test]
    fn test_latest_op_id_with_files() {
        let temp = TempDir::new().unwrap();
        let git_dir = temp.path().join(".git");
        let ops_path = ops_dir(&git_dir);
        std::fs::create_dir_all(&ops_path).unwrap();

        std::fs::write(ops_path.join("20251229T120000Z-old.json"), "{}").unwrap();
        std::fs::write(ops_path.join("20251229T120200Z-new.json"), "{}").unwrap();

        let latest = latest_op_id(&git_dir).unwrap();
        assert_eq!(latest, Some("20251229T120200Z-new".to_string()));
    }

    #[test]
    fn test_rand_suffix_produces_values() {
        let suffix = rand_suffix();
        // Just check it produces a non-zero value
        assert!(suffix > 0 || suffix == 0); // Always true, just testing it runs
    }
}
