use crate::common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

/// Overwrite a tracked branch's metadata ref with an arbitrary parent name,
/// mirroring `sweep_tests.rs`'s metadata helper.
fn write_branch_parent_metadata(repo: &TestRepo, branch: &str, parent_branch: &str) {
    let metadata = serde_json::json!({
        "parentBranchName": parent_branch,
        "parentBranchRevision": repo.get_commit_sha(branch),
    });

    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(repo.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to hash metadata blob");
    child
        .stdin
        .as_mut()
        .expect("metadata hash stdin")
        .write_all(metadata.to_string().as_bytes())
        .expect("Failed to write metadata JSON");
    let output = child.wait_with_output().expect("Failed to hash metadata");
    assert!(
        output.status.success(),
        "git hash-object failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let blob_hash = String::from_utf8(output.stdout)
        .expect("metadata hash UTF-8")
        .trim()
        .to_string();

    let update_ref = repo.git(&[
        "update-ref",
        &format!("refs/branch-metadata/{}", branch),
        &blob_hash,
    ]);
    assert!(
        update_ref.status.success(),
        "git update-ref failed: {}",
        TestRepo::stderr(&update_ref)
    );
}

fn run_stats_json(repo: &TestRepo, extra_args: &[&str]) -> Value {
    let mut args = vec!["stats", "--json"];
    args.extend_from_slice(extra_args);
    let out = repo.run_stax(&args);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stats --json should output valid JSON: {}\n{}", e, stdout))
}

#[test]
fn stats_on_repo_with_no_tracked_branches_exits_zero() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["stats"]).assert_success();

    let json = run_stats_json(&repo, &[]);
    assert_eq!(json["stack_shape"]["tracked"], 0);
    assert_eq!(json["stack_shape"]["independent"], 0);
}

#[test]
fn stats_reports_two_independent_stacks_and_deepest_height() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.create_stack(&["stack-a-1", "stack-a-2"]);
    repo.run_stax(&["t"]).assert_success();
    repo.create_stack(&["stack-b-1"]);

    let json = run_stats_json(&repo, &[]);
    assert_eq!(json["stack_shape"]["tracked"], 3);
    assert_eq!(json["stack_shape"]["independent"], 2);
    assert_eq!(json["stack_shape"]["deepest"], 2);
}

#[test]
fn stats_current_scopes_to_current_stack() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.create_stack(&["stack-a-1", "stack-a-2"]);
    repo.run_stax(&["t"]).assert_success();
    repo.create_stack(&["stack-b-1"]);

    // Current branch is stack-b-1 after the second create_stack call.
    let json = run_stats_json(&repo, &["--current"]);
    assert_eq!(json["scope"], "current");
    assert_eq!(json["stack_shape"]["tracked"], 1);
    assert_eq!(json["stack_shape"]["independent"], 1);
}

#[test]
fn stats_current_on_untracked_branch_is_graceful() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.git(&["checkout", "-b", "untracked-branch"]);

    let out = repo.run_stax(&["stats", "--current"]);
    out.assert_success();

    let json = run_stats_json(&repo, &["--current"]);
    assert_eq!(json["stack_shape"]["tracked"], 0);
    assert_eq!(json["stack_shape"]["independent"], 0);
    assert_eq!(json["stack_shape"]["deepest"], 0);
}

#[test]
fn stats_reports_needs_restack_in_attention() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let branches = repo.create_stack(&["restack-parent", "restack-child"]);
    let parent = &branches[0];

    // Advance the parent branch without restacking the child.
    repo.git(&["checkout", parent]);
    repo.create_file("extra.txt", "extra parent work");
    repo.commit("extra parent commit");

    let json = run_stats_json(&repo, &[]);
    assert_eq!(json["health"]["need_restack"], 1);

    let attention = json["attention"]
        .as_array()
        .expect("attention should be an array");
    assert!(
        attention.iter().any(|item| item["kind"] == "restack"),
        "expected a restack attention item:\n{}",
        json
    );
}

#[test]
fn stats_reports_missing_parent_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.create_stack(&["orphan-child"]);
    write_branch_parent_metadata(&repo, "orphan-child", "ghost-parent");

    let json = run_stats_json(&repo, &[]);
    assert_eq!(json["health"]["missing_parent"], 1);

    let attention = json["attention"]
        .as_array()
        .expect("attention should be an array");
    assert!(
        attention.iter().any(|item| item["kind"] == "missing_parent"
            && item["detail"]
                .as_str()
                .unwrap_or("")
                .contains("ghost-parent")),
        "expected a missing-parent attention item referencing ghost-parent:\n{}",
        json
    );
}

#[test]
fn stats_counts_frozen_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.create_stack(&["frozen-branch"]);
    repo.run_stax(&["freeze"]).assert_success();

    let json = run_stats_json(&repo, &[]);
    assert_eq!(json["pr_mix"]["frozen"], 1);
}

#[test]
fn stats_counts_branches_without_prs() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.create_stack(&["no-pr-branch"]);

    let json = run_stats_json(&repo, &[]);
    assert_eq!(json["pr_mix"]["no_pr"], 1);
    assert_eq!(json["pr_mix"]["open"], 0);
}

#[test]
fn stats_json_has_expected_top_level_keys() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_stack(&["some-branch"]);

    let json = run_stats_json(&repo, &[]);
    let obj = json.as_object().expect("stats JSON should be an object");
    for key in [
        "scope",
        "trunk",
        "current",
        "stack_shape",
        "pr_mix",
        "health",
        "worktrees",
        "attention",
        "biggest_stacks",
        "hygiene",
        "next_actions",
    ] {
        assert!(
            obj.contains_key(key),
            "expected top-level key '{}':\n{}",
            key,
            json
        );
    }
}

#[test]
fn stats_json_counts_match_human_output() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_stack(&["human-check-branch"]);

    let json = run_stats_json(&repo, &[]);
    let no_pr = json["pr_mix"]["no_pr"].as_u64().expect("no_pr count");

    let human_out = repo.run_stax(&["stats"]);
    human_out.assert_success();
    let human_stdout = TestRepo::stdout(&human_out);

    assert!(
        human_stdout.contains(&format!("{} no PR", no_pr)),
        "expected human output to mention '{} no PR':\n{}",
        no_pr,
        human_stdout
    );
}

#[test]
fn stats_omits_zero_valued_counters() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_stack(&["zero-noise-branch"]);

    let out = repo.run_stax(&["stats"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);

    for noise in ["0 draft", "0 missing parent", "0 open", "0 frozen"] {
        assert!(
            !stdout.contains(noise),
            "expected '{}' to be omitted:\n{}",
            noise,
            stdout
        );
    }
}

#[test]
fn stats_reports_all_clear_health_when_nothing_is_broken() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_stack(&["healthy-branch"]);

    let out = repo.run_stax(&["stats"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);

    assert!(
        stdout.contains("all clear"),
        "expected a clean health line:\n{}",
        stdout
    );
}

#[test]
fn stats_hides_single_bar_pr_mix_chart() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_stack(&["only-one-category"]);

    let out = repo.run_stax(&["stats"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);

    // One non-zero category repeats the PRs row, so the chart is suppressed.
    assert!(
        !stdout.contains("PR mix"),
        "expected the PR mix chart to be hidden for a single category:\n{}",
        stdout
    );
}

#[test]
fn stats_ci_without_forge_auth_reports_unavailable_and_exits_zero() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let json = run_stats_json(&repo, &["--ci"]);
    assert!(
        json.get("ci").is_none() || json["ci"].is_null(),
        "expected ci to be absent/null without forge auth:\n{}",
        json
    );
    let reason = json["ci_unavailable_reason"]
        .as_str()
        .expect("ci_unavailable_reason should be a string");
    assert!(
        reason.contains("forge") || reason.contains("remote"),
        "unexpected ci_unavailable_reason: {}",
        reason
    );
}

#[test]
fn stats_help_lists_current_json_and_ci_flags() {
    let repo = TestRepo::new();
    let out = repo.run_stax(&["stats", "--help"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    assert!(stdout.contains("--current"));
    assert!(stdout.contains("--json"));
    assert!(stdout.contains("--ci"));
}
