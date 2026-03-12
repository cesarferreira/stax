use crate::git::GitRepo;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

/// Maximum number of historical runs to keep per check
const MAX_HISTORY_RUNS: usize = 5;
const HISTORY_REF_PREFIX: &str = "refs/stax/ci-history/";

/// CI check history stored in git refs
#[derive(Debug, Serialize, Deserialize)]
pub struct CiCheckHistory {
    pub check_name: String,
    pub runs: Vec<CiRunRecord>,
}

/// Individual CI run record
#[derive(Debug, Serialize, Deserialize)]
pub struct CiRunRecord {
    pub duration_secs: u64,
    pub completed_at: String, // ISO 8601 timestamp
}

impl CiCheckHistory {
    pub fn new(check_name: String) -> Self {
        Self {
            check_name,
            runs: Vec::new(),
        }
    }
}

fn history_ref_name(check_name: &str) -> String {
    let encoded = check_name
        .as_bytes()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>();
    format!("{HISTORY_REF_PREFIX}{encoded}")
}

/// Load CI history for a specific check name from git refs
pub fn load_check_history(repo: &GitRepo, check_name: &str) -> Result<CiCheckHistory> {
    let ref_name = history_ref_name(check_name);
    let inner_repo = repo.inner();

    match inner_repo.find_reference(&ref_name) {
        Ok(reference) => {
            let oid = reference.target().context("Reference has no target")?;
            let blob = inner_repo.find_blob(oid)?;
            let content = std::str::from_utf8(blob.content())?;
            let history: CiCheckHistory = serde_json::from_str(content)?;
            Ok(history)
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            // No history exists yet, return empty
            Ok(CiCheckHistory::new(check_name.to_string()))
        }
        Err(e) => Err(e.into()),
    }
}

/// Save CI history for a specific check name to git refs
pub fn save_check_history(repo: &GitRepo, history: &CiCheckHistory) -> Result<()> {
    let ref_name = history_ref_name(&history.check_name);
    let workdir = repo.workdir()?;
    let json = serde_json::to_string(history)?;

    // Create blob with json content
    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(workdir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(json.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        anyhow::bail!("git hash-object -w --stdin failed");
    }

    let hash = String::from_utf8(output.stdout)?.trim().to_string();

    // Update the ref to point to the blob
    let status = Command::new("git")
        .args(["update-ref", &ref_name, &hash])
        .current_dir(workdir)
        .status()
        .context("Failed to update CI history ref")?;

    if !status.success() {
        anyhow::bail!("git update-ref {} {} failed", ref_name, hash);
    }

    Ok(())
}

/// Add a completed run to history (keeps only last MAX_HISTORY_RUNS)
pub fn add_completion(
    repo: &GitRepo,
    check_name: &str,
    duration_secs: u64,
    completed_at: String,
) -> Result<()> {
    let mut history = load_check_history(repo, check_name)?;

    // Add new run
    history.runs.push(CiRunRecord {
        duration_secs,
        completed_at,
    });

    // Keep only last MAX_HISTORY_RUNS (FIFO queue)
    if history.runs.len() > MAX_HISTORY_RUNS {
        history
            .runs
            .drain(0..(history.runs.len() - MAX_HISTORY_RUNS));
    }

    save_check_history(repo, &history)?;
    Ok(())
}

/// Calculate average duration from history
pub fn calculate_average(history: &CiCheckHistory) -> Option<u64> {
    if history.runs.is_empty() {
        return None;
    }

    let sum: u64 = history.runs.iter().map(|r| r.duration_secs).sum();
    Some(sum / history.runs.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn test_new_history() {
        let history = CiCheckHistory::new("build".to_string());
        assert_eq!(history.check_name, "build");
        assert_eq!(history.runs.len(), 0);
    }

    #[test]
    fn test_history_ref_name_encodes_invalid_ref_chars() {
        let ref_name = history_ref_name("branch-overall:feature/foo CI (Ubuntu)");
        assert!(ref_name.starts_with(HISTORY_REF_PREFIX));
        assert_eq!(
            ref_name,
            "refs/stax/ci-history/6272616e63682d6f766572616c6c3a666561747572652f666f6f20434920285562756e747529"
        );

        let status = Command::new("git")
            .args(["check-ref-format", &ref_name])
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_calculate_average_empty() {
        let history = CiCheckHistory::new("test".to_string());
        assert_eq!(calculate_average(&history), None);
    }

    #[test]
    fn test_calculate_average_single() {
        let mut history = CiCheckHistory::new("test".to_string());
        history.runs.push(CiRunRecord {
            duration_secs: 100,
            completed_at: "2026-01-16T12:00:00Z".to_string(),
        });
        assert_eq!(calculate_average(&history), Some(100));
    }

    #[test]
    fn test_calculate_average_multiple() {
        let mut history = CiCheckHistory::new("test".to_string());
        history.runs.push(CiRunRecord {
            duration_secs: 100,
            completed_at: "2026-01-16T12:00:00Z".to_string(),
        });
        history.runs.push(CiRunRecord {
            duration_secs: 120,
            completed_at: "2026-01-16T12:05:00Z".to_string(),
        });
        history.runs.push(CiRunRecord {
            duration_secs: 140,
            completed_at: "2026-01-16T12:10:00Z".to_string(),
        });
        // Average: (100 + 120 + 140) / 3 = 120
        assert_eq!(calculate_average(&history), Some(120));
    }

    #[test]
    fn test_run_record_serialization() {
        let record = CiRunRecord {
            duration_secs: 150,
            completed_at: "2026-01-16T12:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("150"));
        assert!(json.contains("2026-01-16T12:00:00Z"));

        let deserialized: CiRunRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.duration_secs, 150);
        assert_eq!(deserialized.completed_at, "2026-01-16T12:00:00Z");
    }

    #[test]
    fn test_history_serialization() {
        let mut history = CiCheckHistory::new("build".to_string());
        history.runs.push(CiRunRecord {
            duration_secs: 100,
            completed_at: "2026-01-16T12:00:00Z".to_string(),
        });

        let json = serde_json::to_string(&history).unwrap();
        assert!(json.contains("build"));
        assert!(json.contains("100"));

        let deserialized: CiCheckHistory = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.check_name, "build");
        assert_eq!(deserialized.runs.len(), 1);
        assert_eq!(deserialized.runs[0].duration_secs, 100);
    }
}
