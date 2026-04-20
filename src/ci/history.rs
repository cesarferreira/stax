use super::CheckRunInfo;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::process::Command;

/// Maximum number of historical runs to keep per check
const MAX_HISTORY_RUNS: usize = 20;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_offset_secs: Option<u64>,
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
    add_timing_sample(repo, check_name, duration_secs, completed_at, None)
}

/// Add a completed run to history with an optional run-level end offset
pub fn add_timing_sample(
    repo: &GitRepo,
    check_name: &str,
    duration_secs: u64,
    completed_at: String,
    end_offset_secs: Option<u64>,
) -> Result<()> {
    if duration_secs == 0 {
        return Ok(());
    }

    let mut history = load_check_history(repo, check_name)?;

    history.runs.retain(|run| run.duration_secs > 0);

    // Add new run
    history.runs.push(CiRunRecord {
        duration_secs,
        completed_at,
        end_offset_secs: end_offset_secs.filter(|secs| *secs > 0),
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
    let valid_runs: Vec<&CiRunRecord> = history
        .runs
        .iter()
        .filter(|run| run.duration_secs > 0)
        .collect();

    if valid_runs.is_empty() {
        return None;
    }

    let sum: u64 = valid_runs.iter().map(|run| run.duration_secs).sum();
    Some(sum / valid_runs.len() as u64)
}

/// Calculate average run end offset from history
pub fn calculate_average_end_offset(history: &CiCheckHistory) -> Option<u64> {
    let valid_offsets: Vec<u64> = history
        .runs
        .iter()
        .filter_map(|run| run.end_offset_secs)
        .filter(|secs| *secs > 0)
        .collect();

    if valid_offsets.is_empty() {
        return None;
    }

    let sum: u64 = valid_offsets.iter().sum();
    Some(sum / valid_offsets.len() as u64)
}

/// Estimate total wall-clock runtime for the current run from reusable per-check history.
pub fn estimate_run_average(repo: &GitRepo, checks: &[CheckRunInfo]) -> Option<u64> {
    let run_start = checks
        .iter()
        .filter_map(|check| parse_ci_timestamp(check.started_at.as_deref()))
        .min();

    checks
        .iter()
        .filter_map(|check| {
            let history = load_check_history(repo, &check.name).ok()?;
            calculate_average_end_offset(&history).or_else(|| {
                let avg_duration = calculate_average(&history)?;
                let start_offset =
                    match (run_start, parse_ci_timestamp(check.started_at.as_deref())) {
                        (Some(run_start), Some(check_start)) => check_start
                            .signed_duration_since(run_start)
                            .num_seconds()
                            .max(0)
                            as u64,
                        _ => 0,
                    };
                Some(start_offset + avg_duration)
            })
        })
        .max()
}

fn parse_ci_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    value?.parse::<DateTime<Utc>>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitRepo;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_temp_repo() -> (TempDir, GitRepo) {
        let tempdir = TempDir::new().unwrap();
        let status = Command::new("git")
            .args(["init"])
            .current_dir(tempdir.path())
            .status()
            .unwrap();
        assert!(status.success());
        let repo = GitRepo::open_from_path(tempdir.path()).unwrap();
        (tempdir, repo)
    }

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
            end_offset_secs: None,
        });
        assert_eq!(calculate_average(&history), Some(100));
    }

    #[test]
    fn test_calculate_average_multiple() {
        let mut history = CiCheckHistory::new("test".to_string());
        history.runs.push(CiRunRecord {
            duration_secs: 100,
            completed_at: "2026-01-16T12:00:00Z".to_string(),
            end_offset_secs: None,
        });
        history.runs.push(CiRunRecord {
            duration_secs: 120,
            completed_at: "2026-01-16T12:05:00Z".to_string(),
            end_offset_secs: None,
        });
        history.runs.push(CiRunRecord {
            duration_secs: 140,
            completed_at: "2026-01-16T12:10:00Z".to_string(),
            end_offset_secs: None,
        });
        // Average: (100 + 120 + 140) / 3 = 120
        assert_eq!(calculate_average(&history), Some(120));
    }

    #[test]
    fn test_calculate_average_ignores_zero_duration_runs() {
        let mut history = CiCheckHistory::new("test".to_string());
        history.runs.push(CiRunRecord {
            duration_secs: 0,
            completed_at: "2026-01-16T12:00:00Z".to_string(),
            end_offset_secs: None,
        });
        history.runs.push(CiRunRecord {
            duration_secs: 120,
            completed_at: "2026-01-16T12:05:00Z".to_string(),
            end_offset_secs: None,
        });
        history.runs.push(CiRunRecord {
            duration_secs: 180,
            completed_at: "2026-01-16T12:10:00Z".to_string(),
            end_offset_secs: None,
        });

        assert_eq!(calculate_average(&history), Some(150));
    }

    #[test]
    fn test_calculate_average_all_zero_duration_runs_returns_none() {
        let mut history = CiCheckHistory::new("test".to_string());
        history.runs.push(CiRunRecord {
            duration_secs: 0,
            completed_at: "2026-01-16T12:00:00Z".to_string(),
            end_offset_secs: None,
        });
        history.runs.push(CiRunRecord {
            duration_secs: 0,
            completed_at: "2026-01-16T12:05:00Z".to_string(),
            end_offset_secs: None,
        });

        assert_eq!(calculate_average(&history), None);
    }

    #[test]
    fn test_calculate_average_end_offset() {
        let mut history = CiCheckHistory::new("test".to_string());
        history.runs.push(CiRunRecord {
            duration_secs: 120,
            completed_at: "2026-01-16T12:00:00Z".to_string(),
            end_offset_secs: Some(600),
        });
        history.runs.push(CiRunRecord {
            duration_secs: 180,
            completed_at: "2026-01-16T12:05:00Z".to_string(),
            end_offset_secs: Some(900),
        });

        assert_eq!(calculate_average_end_offset(&history), Some(750));
    }

    #[test]
    fn test_estimate_run_average_prefers_historical_end_offsets() {
        let (_tempdir, repo) = init_temp_repo();
        add_timing_sample(
            &repo,
            "android suite",
            1500,
            "2026-01-16T12:25:00Z".to_string(),
            Some(1500),
        )
        .unwrap();
        add_timing_sample(
            &repo,
            "checklist",
            330,
            "2026-01-16T12:05:30Z".to_string(),
            Some(330),
        )
        .unwrap();

        let checks = vec![
            CheckRunInfo {
                name: "android suite".to_string(),
                status: "in_progress".to_string(),
                conclusion: None,
                url: None,
                started_at: Some("2026-01-16T12:00:00Z".to_string()),
                completed_at: None,
                elapsed_secs: Some(600),
                average_secs: Some(1500),
                completion_percent: Some(40),
            },
            CheckRunInfo {
                name: "checklist".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                url: None,
                started_at: Some("2026-01-16T12:00:00Z".to_string()),
                completed_at: Some("2026-01-16T12:05:11Z".to_string()),
                elapsed_secs: Some(311),
                average_secs: Some(330),
                completion_percent: None,
            },
        ];

        assert_eq!(estimate_run_average(&repo, &checks), Some(1500));
    }

    #[test]
    fn test_estimate_run_average_falls_back_to_duration_plus_start_offset() {
        let (_tempdir, repo) = init_temp_repo();
        add_completion(&repo, "late check", 300, "2026-01-16T12:10:00Z".to_string()).unwrap();

        let checks = vec![
            CheckRunInfo {
                name: "setup".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                url: None,
                started_at: Some("2026-01-16T12:00:00Z".to_string()),
                completed_at: Some("2026-01-16T12:01:00Z".to_string()),
                elapsed_secs: Some(60),
                average_secs: None,
                completion_percent: None,
            },
            CheckRunInfo {
                name: "late check".to_string(),
                status: "in_progress".to_string(),
                conclusion: None,
                url: None,
                started_at: Some("2026-01-16T12:05:00Z".to_string()),
                completed_at: None,
                elapsed_secs: Some(30),
                average_secs: Some(300),
                completion_percent: Some(10),
            },
        ];

        assert_eq!(estimate_run_average(&repo, &checks), Some(600));
    }

    #[test]
    fn test_add_timing_sample_keeps_recent_window() {
        let (_tempdir, repo) = init_temp_repo();

        for idx in 0..(MAX_HISTORY_RUNS + 5) {
            add_timing_sample(
                &repo,
                "android suite",
                100 + idx as u64,
                format!("2026-01-16T12:{idx:02}:00Z"),
                Some(200 + idx as u64),
            )
            .unwrap();
        }

        let history = load_check_history(&repo, "android suite").unwrap();
        assert_eq!(history.runs.len(), MAX_HISTORY_RUNS);
        assert_eq!(history.runs.first().unwrap().duration_secs, 105);
        assert_eq!(
            history.runs.last().unwrap().duration_secs,
            100 + (MAX_HISTORY_RUNS + 4) as u64
        );
    }

    #[test]
    fn test_run_record_serialization() {
        let record = CiRunRecord {
            duration_secs: 150,
            completed_at: "2026-01-16T12:00:00Z".to_string(),
            end_offset_secs: Some(900),
        };

        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("150"));
        assert!(json.contains("2026-01-16T12:00:00Z"));
        assert!(json.contains("900"));

        let deserialized: CiRunRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.duration_secs, 150);
        assert_eq!(deserialized.completed_at, "2026-01-16T12:00:00Z");
        assert_eq!(deserialized.end_offset_secs, Some(900));
    }

    #[test]
    fn test_history_serialization() {
        let mut history = CiCheckHistory::new("build".to_string());
        history.runs.push(CiRunRecord {
            duration_secs: 100,
            completed_at: "2026-01-16T12:00:00Z".to_string(),
            end_offset_secs: Some(120),
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
