use crate::commands::sync::{SyncStats, TrunkNotUpdated, TrunkSummary, TrunkUpdateFailure};
use crate::commands::sync_plan::TrunkPlan;
use crate::errors::{ConflictStopped, DirtyWorkingTree};
use anyhow::Error;
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize)]
pub struct SyncOutput {
    pub schema_version: u8,
    pub kind: &'static str,
    pub success: bool,
    pub dry_run: bool,
    pub duration_ms: u64,
    pub trunk: TrunkJson,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deleted_branches: Vec<DeletedBranchJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped_branches: Vec<SkippedBranchJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub protected_branches: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub partially_merged: Vec<PartiallyMergedJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub restacked_branches: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub imported_branches_updated: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkout_change: Option<CheckoutChangeJson>,
    pub stash: StashJson,
    // Plan-mode fields (sync_plan kind only; absent in sync kind via skip_serializing_if)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub merged_candidates: Vec<MergedCandidateJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub upstream_gone_protected: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub upstream_gone_deletable: Vec<UpstreamGoneDeleteJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub frozen_branches: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub branches_to_restack: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub predicted_conflicts: Vec<PredictedConflictJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub would_stash: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorJson>,
}

#[derive(Serialize)]
pub struct TrunkJson {
    pub branch: String,
    pub remote_ref: String,
    pub action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additions: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletions: Option<usize>,
}

#[derive(Serialize)]
pub struct DeletedBranchJson {
    pub name: String,
    pub category: &'static str,
    pub scope: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
    pub metadata_deleted: bool,
}

#[derive(Serialize)]
pub struct SkippedBranchJson {
    pub name: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct PartiallyMergedJson {
    pub name: String,
    pub reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
    pub extra_commits: usize,
}

#[derive(Serialize)]
pub struct CheckoutChangeJson {
    pub from: String,
    pub to: String,
}

#[derive(Serialize)]
pub struct StashJson {
    pub stashed: bool,
    pub restored: bool,
    pub left_stashed: bool,
}

/// Per-branch disposition in a dry-run merged-branch plan.
#[derive(Serialize)]
pub struct MergedCandidateJson {
    pub name: String,
    /// `would_delete` · `would_prompt_then_delete` · `would_keep_worktree` · `would_skip` ·
    /// `would_rebase_children`
    pub disposition: &'static str,
    /// `both` · `local` — present when disposition is `would_delete` or
    /// `would_prompt_then_delete`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<&'static str>,
    /// Human-readable reason — present when disposition is `would_keep_worktree` or
    /// `would_skip`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_reason: Option<String>,
    /// Child branch names — present when disposition is `would_rebase_children`
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
}

/// Per-branch disposition for an upstream-gone branch in a dry-run plan.
#[derive(Serialize)]
pub struct UpstreamGoneDeleteJson {
    pub name: String,
    /// `would_delete` (with `--force`) or `would_prompt_then_delete` (without)
    pub disposition: &'static str,
}

/// Predicted merge conflict for a single branch restack.
#[derive(Serialize)]
pub struct PredictedConflictJson {
    pub branch: String,
    pub onto: String,
    pub files: Vec<String>,
}

/// All plan-specific data accumulated during a `--dry-run --json` run.
/// Passed from `sync_plan::run()` to `build_plan()`.
pub(super) struct SyncPlanData {
    pub merged_candidates: Vec<MergedCandidateJson>,
    pub partially_merged: Vec<PartiallyMergedJson>,
    pub upstream_gone_protected: Vec<String>,
    pub upstream_gone_deletable: Vec<UpstreamGoneDeleteJson>,
    pub frozen_branches: Vec<String>,
    pub branches_to_restack: Vec<String>,
    pub predicted_conflicts: Vec<PredictedConflictJson>,
    /// Whether the working tree is dirty (real sync would auto-stash).
    pub would_stash: bool,
}

#[derive(Serialize)]
pub struct ErrorJson {
    pub kind: &'static str,
    pub message: String,
}

/// Map TrunkSummary / TrunkNotUpdated to a stable JSON action string.
/// Action strings are extensible — consumers should treat unknown values as "unknown".
fn trunk_action(
    trunk_summary: Option<&TrunkSummary>,
    trunk_not_updated: Option<&TrunkNotUpdated>,
) -> &'static str {
    if let Some(summary) = trunk_summary {
        return match summary {
            TrunkSummary::UpToDate { .. } => "up_to_date",
            TrunkSummary::Pulled { .. } => "fast_forwarded",
            TrunkSummary::Updated { .. } => "reset",
        };
    }
    if let Some(not_updated) = trunk_not_updated {
        return match not_updated.failure {
            TrunkUpdateFailure::Diverged => "diverged",
            TrunkUpdateFailure::Other => "failed",
        };
    }
    "unknown"
}

/// Map a TrunkPlan variant (from --dry-run) to the same action vocab as run-mode.
pub(super) fn trunk_plan_to_action(plan: &TrunkPlan) -> &'static str {
    match plan {
        TrunkPlan::UpToDate => "up_to_date",
        TrunkPlan::FastForward { .. } => "fast_forwarded",
        TrunkPlan::ResetToRemote { .. } => "reset",
        TrunkPlan::SkippedSafeMode { .. } => "failed",
        TrunkPlan::Diverged { .. } => "diverged",
        TrunkPlan::RemoteUnknown | TrunkPlan::ObjectsAbsent { .. } => "failed",
    }
}

/// Build a SyncOutput from the sync stats collected during a run.
pub(super) fn build(
    trunk_branch: &str,
    remote_trunk_ref: &str,
    stats: &SyncStats,
    dry_run: bool,
    duration: Duration,
    error: Option<ErrorJson>,
) -> SyncOutput {
    let action = trunk_action(stats.trunk.as_ref(), stats.trunk_not_updated.as_ref());

    let (commits, files, additions, deletions) = match &stats.trunk {
        Some(TrunkSummary::Pulled {
            commits,
            files,
            additions,
            deletions,
            ..
        }) => (
            Some(*commits),
            Some(*files),
            Some(*additions),
            Some(*deletions),
        ),
        _ => (None, None, None, None),
    };

    let trunk = TrunkJson {
        branch: trunk_branch.to_string(),
        remote_ref: remote_trunk_ref.to_string(),
        action,
        commits,
        files,
        additions,
        deletions,
    };

    let deleted_branches = stats
        .deleted_branches
        .iter()
        .map(|r| DeletedBranchJson {
            name: r.branch.clone(),
            category: r.category,
            scope: r.scope,
            tip: r.tip.clone(),
            metadata_deleted: r.metadata_deleted,
        })
        .collect();

    let skipped_branches = stats
        .cleanup_skips
        .iter()
        .map(|s| SkippedBranchJson {
            name: s.branch.clone(),
            reason: s.reason.clone(),
        })
        .collect();

    let partially_merged = stats
        .partially_merged
        .iter()
        .map(|r| PartiallyMergedJson {
            name: r.branch.clone(),
            reason: r.reason,
            pr_number: r.pr_number,
            extra_commits: r.extra_commits,
        })
        .collect();

    let checkout_change = stats.checkout_change.as_ref().map(|c| CheckoutChangeJson {
        from: c.from.clone(),
        to: c.to.clone(),
    });

    let stash = StashJson {
        stashed: stats.stash.stashed,
        restored: stats.stash.restored,
        left_stashed: stats.stash.stashed && !stats.stash.restored,
    };

    let success = error.is_none();

    SyncOutput {
        schema_version: 1,
        kind: "sync",
        success,
        dry_run,
        duration_ms: duration.as_millis() as u64,
        trunk,
        deleted_branches,
        skipped_branches,
        protected_branches: stats.protected_branches.clone(),
        partially_merged,
        restacked_branches: stats.restacked_branches.clone(),
        imported_branches_updated: stats.imported_branches_updated.clone(),
        checkout_change,
        stash,
        // Plan-mode fields are absent in run-mode output.
        merged_candidates: vec![],
        upstream_gone_protected: vec![],
        upstream_gone_deletable: vec![],
        frozen_branches: vec![],
        branches_to_restack: vec![],
        predicted_conflicts: vec![],
        would_stash: None,
        error,
    }
}

/// Build a SyncOutput for --dry-run --json from a sync_plan run.
pub(super) fn build_plan(
    trunk_branch: &str,
    remote_trunk_ref: &str,
    trunk_plan: &TrunkPlan,
    plan_data: SyncPlanData,
    duration: Duration,
) -> SyncOutput {
    let action = trunk_plan_to_action(trunk_plan);

    let (commits, files, additions, deletions) = match trunk_plan {
        TrunkPlan::FastForward {
            commits,
            files,
            additions,
            deletions,
        } => (
            Some(*commits),
            Some(*files),
            Some(*additions),
            Some(*deletions),
        ),
        _ => (None, None, None, None),
    };

    let trunk = TrunkJson {
        branch: trunk_branch.to_string(),
        remote_ref: remote_trunk_ref.to_string(),
        action,
        commits,
        files,
        additions,
        deletions,
    };

    SyncOutput {
        schema_version: 1,
        kind: "sync_plan",
        success: true,
        dry_run: true,
        duration_ms: duration.as_millis() as u64,
        trunk,
        deleted_branches: vec![],
        skipped_branches: vec![],
        protected_branches: vec![],
        partially_merged: plan_data.partially_merged,
        restacked_branches: vec![],
        imported_branches_updated: vec![],
        checkout_change: None,
        stash: StashJson {
            stashed: false,
            restored: false,
            left_stashed: false,
        },
        merged_candidates: plan_data.merged_candidates,
        upstream_gone_protected: plan_data.upstream_gone_protected,
        upstream_gone_deletable: plan_data.upstream_gone_deletable,
        frozen_branches: plan_data.frozen_branches,
        branches_to_restack: plan_data.branches_to_restack,
        predicted_conflicts: plan_data.predicted_conflicts,
        would_stash: Some(plan_data.would_stash),
        error: None,
    }
}

/// Serialize a SyncOutput to a pretty-printed JSON string.
pub(super) fn emit(output: &SyncOutput) -> String {
    serde_json::to_string_pretty(output).unwrap_or_else(|_| "{}".to_string())
}

/// Classify an anyhow error into a JSON error envelope.
pub(super) fn classify_error(e: &Error) -> ErrorJson {
    if e.downcast_ref::<DirtyWorkingTree>().is_some() {
        ErrorJson {
            kind: "dirty_working_tree",
            message: e.to_string(),
        }
    } else if e.downcast_ref::<ConflictStopped>().is_some() {
        ErrorJson {
            kind: "restack_conflict",
            message: e.to_string(),
        }
    } else {
        ErrorJson {
            kind: "error",
            message: e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::sync::{StashOutcome, SyncStats, TrunkNotUpdated, TrunkUpdateFailure};
    use std::time::Duration;

    fn make_default_stats() -> SyncStats {
        SyncStats::default()
    }

    #[test]
    fn build_success_round_trips_to_valid_json() {
        let stats = make_default_stats();
        let output = build(
            "main",
            "origin/main",
            &stats,
            false,
            Duration::from_secs(1),
            None,
        );
        let json_str = emit(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["kind"], "sync");
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["dry_run"], false);
        assert_eq!(parsed["trunk"]["branch"], "main");
        assert_eq!(parsed["trunk"]["remote_ref"], "origin/main");
        assert_eq!(parsed["trunk"]["action"], "unknown");
    }

    #[test]
    fn build_error_sets_success_false_and_error_field() {
        let stats = make_default_stats();
        let err = ErrorJson {
            kind: "dirty_working_tree",
            message: "Working tree is dirty. Commit or stash your changes, or re-run with --stash to stash them automatically.".to_string(),
        };
        let output = build(
            "main",
            "origin/main",
            &stats,
            false,
            Duration::from_millis(500),
            Some(err),
        );
        let json_str = emit(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"]["kind"], "dirty_working_tree");
        assert!(
            parsed["error"]["message"]
                .as_str()
                .unwrap()
                .contains("dirty")
        );
    }

    #[test]
    fn diverged_trunk_action_maps_correctly() {
        let stats = SyncStats {
            trunk_not_updated: Some(TrunkNotUpdated {
                branch: "main".to_string(),
                remote_ref: "origin/main".to_string(),
                failure: TrunkUpdateFailure::Diverged,
            }),
            ..SyncStats::default()
        };
        let output = build(
            "main",
            "origin/main",
            &stats,
            false,
            Duration::from_secs(1),
            None,
        );
        let json_str = emit(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
        assert_eq!(parsed["trunk"]["action"], "diverged");
    }

    #[test]
    fn failed_trunk_action_maps_correctly() {
        let stats = SyncStats {
            trunk_not_updated: Some(TrunkNotUpdated {
                branch: "main".to_string(),
                remote_ref: "origin/main".to_string(),
                failure: TrunkUpdateFailure::Other,
            }),
            ..SyncStats::default()
        };
        let output = build(
            "main",
            "origin/main",
            &stats,
            false,
            Duration::from_secs(1),
            None,
        );
        let json_str = emit(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
        assert_eq!(parsed["trunk"]["action"], "failed");
    }

    #[test]
    fn default_stats_omits_optional_vec_keys() {
        let stats = make_default_stats();
        let output = build(
            "main",
            "origin/main",
            &stats,
            false,
            Duration::from_secs(1),
            None,
        );
        let json_str = emit(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
        assert!(
            parsed.get("deleted_branches").is_none(),
            "absent when empty"
        );
        assert!(
            parsed.get("skipped_branches").is_none(),
            "absent when empty"
        );
        assert!(
            parsed.get("restacked_branches").is_none(),
            "absent when empty"
        );
        assert!(parsed.get("checkout_change").is_none(), "absent when None");
    }

    #[test]
    fn stash_left_stashed_is_computed() {
        let stats = SyncStats {
            stash: StashOutcome {
                stashed: true,
                restored: false,
            },
            ..SyncStats::default()
        };
        let output = build(
            "main",
            "origin/main",
            &stats,
            false,
            Duration::from_secs(1),
            None,
        );
        let json_str = emit(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
        assert_eq!(parsed["stash"]["stashed"], true);
        assert_eq!(parsed["stash"]["restored"], false);
        assert_eq!(parsed["stash"]["left_stashed"], true);
    }

    #[test]
    fn classify_error_dirty_working_tree() {
        let err: anyhow::Error = DirtyWorkingTree.into();
        let classified = classify_error(&err);
        assert_eq!(classified.kind, "dirty_working_tree");
        assert!(classified.message.contains("dirty"));
    }

    #[test]
    fn classify_error_conflict_stopped() {
        let err: anyhow::Error = ConflictStopped.into();
        let classified = classify_error(&err);
        assert_eq!(classified.kind, "restack_conflict");
    }

    #[test]
    fn classify_error_generic_error() {
        let err: anyhow::Error = anyhow::anyhow!("something went wrong");
        let classified = classify_error(&err);
        assert_eq!(classified.kind, "error");
        assert!(classified.message.contains("went wrong"));
    }

    #[test]
    fn trunk_plan_to_action_maps_all_variants() {
        use crate::commands::sync_plan::TrunkPlan;
        assert_eq!(trunk_plan_to_action(&TrunkPlan::UpToDate), "up_to_date");
        assert_eq!(
            trunk_plan_to_action(&TrunkPlan::FastForward {
                commits: 1,
                files: 1,
                additions: 1,
                deletions: 0
            }),
            "fast_forwarded"
        );
        assert_eq!(
            trunk_plan_to_action(&TrunkPlan::ResetToRemote {
                target: "abc".to_string()
            }),
            "reset"
        );
        assert_eq!(
            trunk_plan_to_action(&TrunkPlan::Diverged {
                ahead: 1,
                behind: 1
            }),
            "diverged"
        );
        assert_eq!(trunk_plan_to_action(&TrunkPlan::RemoteUnknown), "failed");
    }

    #[test]
    fn build_plan_includes_merged_candidates_and_partially_merged() {
        use crate::commands::sync_plan::TrunkPlan;

        let plan_data = SyncPlanData {
            merged_candidates: vec![
                MergedCandidateJson {
                    name: "feat-a".to_string(),
                    disposition: "would_prompt_then_delete",
                    scope: Some("both"),
                    keep_reason: None,
                    children: vec![],
                },
                MergedCandidateJson {
                    name: "feat-b".to_string(),
                    disposition: "would_keep_worktree",
                    scope: None,
                    keep_reason: Some("it has uncommitted changes".to_string()),
                    children: vec![],
                },
            ],
            partially_merged: vec![PartiallyMergedJson {
                name: "feat-c".to_string(),
                reason: "pr_merged",
                pr_number: Some(42),
                extra_commits: 2,
            }],
            upstream_gone_protected: vec!["gone-safe".to_string()],
            upstream_gone_deletable: vec![UpstreamGoneDeleteJson {
                name: "gone-del".to_string(),
                disposition: "would_prompt_then_delete",
            }],
            frozen_branches: vec!["frozen-x".to_string()],
            branches_to_restack: vec!["feat-d".to_string()],
            predicted_conflicts: vec![PredictedConflictJson {
                branch: "feat-d".to_string(),
                onto: "main".to_string(),
                files: vec!["src/lib.rs".to_string()],
            }],
            would_stash: false,
        };

        let output = build_plan(
            "main",
            "origin/main",
            &TrunkPlan::UpToDate,
            plan_data,
            Duration::from_secs(1),
        );
        let json_str = emit(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");

        assert_eq!(parsed["kind"], "sync_plan");
        assert_eq!(parsed["dry_run"], true);

        // merged_candidates
        let candidates = parsed["merged_candidates"]
            .as_array()
            .expect("merged_candidates present");
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0]["name"], "feat-a");
        assert_eq!(candidates[0]["disposition"], "would_prompt_then_delete");
        assert_eq!(candidates[0]["scope"], "both");
        assert_eq!(candidates[1]["disposition"], "would_keep_worktree");
        assert!(candidates[1]["keep_reason"].is_string());

        // partially_merged
        let pm = parsed["partially_merged"]
            .as_array()
            .expect("partially_merged present");
        assert_eq!(pm.len(), 1);
        assert_eq!(pm[0]["name"], "feat-c");
        assert_eq!(pm[0]["reason"], "pr_merged");
        assert_eq!(pm[0]["pr_number"], 42);
        assert_eq!(pm[0]["extra_commits"], 2);

        // upstream_gone
        assert_eq!(
            parsed["upstream_gone_protected"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            parsed["upstream_gone_deletable"].as_array().unwrap()[0]["name"],
            "gone-del"
        );

        // frozen + restack + conflicts
        assert_eq!(parsed["frozen_branches"].as_array().unwrap()[0], "frozen-x");
        assert_eq!(
            parsed["branches_to_restack"].as_array().unwrap()[0],
            "feat-d"
        );
        let conflicts = parsed["predicted_conflicts"].as_array().unwrap();
        assert_eq!(conflicts[0]["branch"], "feat-d");
        assert_eq!(conflicts[0]["files"].as_array().unwrap()[0], "src/lib.rs");

        // would_stash present in plan mode
        assert_eq!(parsed["would_stash"], false);
    }

    #[test]
    fn build_plan_would_stash_true_when_dirty() {
        use crate::commands::sync_plan::TrunkPlan;

        let plan_data = SyncPlanData {
            merged_candidates: vec![],
            partially_merged: vec![],
            upstream_gone_protected: vec![],
            upstream_gone_deletable: vec![],
            frozen_branches: vec![],
            branches_to_restack: vec![],
            predicted_conflicts: vec![],
            would_stash: true,
        };

        let output = build_plan(
            "main",
            "origin/main",
            &TrunkPlan::UpToDate,
            plan_data,
            Duration::from_millis(200),
        );
        let json_str = emit(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");

        assert_eq!(parsed["would_stash"], true);
        // plan-only fields that are empty/None must be absent
        assert!(parsed.get("merged_candidates").is_none());
        assert!(parsed.get("frozen_branches").is_none());
    }

    #[test]
    fn build_run_mode_omits_plan_only_fields() {
        let stats = make_default_stats();
        let output = build(
            "main",
            "origin/main",
            &stats,
            false,
            Duration::from_secs(1),
            None,
        );
        let json_str = emit(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");

        // Plan-only fields must be absent in run mode
        assert!(
            parsed.get("merged_candidates").is_none(),
            "merged_candidates must be absent in run mode"
        );
        assert!(
            parsed.get("would_stash").is_none(),
            "would_stash must be absent in run mode"
        );
        assert!(
            parsed.get("branches_to_restack").is_none(),
            "branches_to_restack must be absent in run mode"
        );
    }
}
