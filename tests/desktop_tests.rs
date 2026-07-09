use crate::common::TestRepo;
use serde_json::Value;
use std::process::Output;

fn terminal_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "desktop stdout was not one JSON object: {error}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    })
}

fn desktop_diff(repo: &TestRepo, branch: &str, request_id: &str) -> Output {
    let repo_path = repo.path().to_string_lossy().into_owned();
    repo.run_stax(&[
        "desktop",
        "diff",
        "--repo",
        &repo_path,
        "--schema-version",
        "1",
        "--request-id",
        request_id,
        "--branch",
        branch,
    ])
}

fn desktop_action(
    repo: &TestRepo,
    action: &str,
    branch: Option<&str>,
    request_id: &str,
    env: &[(&str, &str)],
) -> Output {
    let repo_path = repo.path().to_string_lossy().into_owned();
    let mut args = vec![
        "desktop",
        "action",
        "--repo",
        &repo_path,
        "--schema-version",
        "1",
        "--request-id",
        request_id,
        "--action",
        action,
    ];
    if let Some(branch) = branch {
        args.extend(["--branch", branch]);
    }
    repo.run_stax_with_env(&args, env)
}

fn json_lines(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!(
                    "desktop stdout line was not JSON: {error}\nline={line}\nstderr={}",
                    String::from_utf8_lossy(&output.stderr)
                )
            })
        })
        .collect()
}

#[test]
fn rejects_unsupported_desktop_schema_with_machine_error() {
    let repo = TestRepo::new();
    let repo_path = repo.path().to_string_lossy().into_owned();
    let output = repo.run_stax(&[
        "desktop",
        "snapshot",
        "--repo",
        &repo_path,
        "--schema-version",
        "9",
        "--request-id",
        "req-schema",
    ]);

    assert!(!output.status.success());
    let event = terminal_json(&output);
    assert_eq!(event["schema_version"], 1);
    assert_eq!(event["request_id"], "req-schema");
    assert_eq!(event["type"], "result");
    assert_eq!(event["ok"], false);
    assert_eq!(event["error"]["code"], "unsupported_schema");
    assert_eq!(event["error"]["recovery"], "reinstall_app");
}

#[test]
fn snapshot_reports_stacked_branches_in_display_order() {
    let repo = TestRepo::new();

    let create_base = repo.run_stax(&["create", "feature/base"]);
    assert!(
        create_base.status.success(),
        "failed to create base branch: {}",
        String::from_utf8_lossy(&create_base.stderr)
    );
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    assert!(repo.git(&["add", "base.txt"]).status.success());
    assert!(repo.git(&["commit", "-m", "Add base"]).status.success());

    let create_ui = repo.run_stax(&["create", "feature/ui"]);
    assert!(
        create_ui.status.success(),
        "failed to create ui branch: {}",
        String::from_utf8_lossy(&create_ui.stderr)
    );
    std::fs::write(repo.path().join("ui.txt"), "ui\n").unwrap();
    assert!(repo.git(&["add", "ui.txt"]).status.success());
    assert!(repo.git(&["commit", "-m", "Add UI"]).status.success());

    let repo_path = repo.path().to_string_lossy().into_owned();
    let output = repo.run_stax(&[
        "desktop",
        "snapshot",
        "--repo",
        &repo_path,
        "--schema-version",
        "1",
        "--request-id",
        "req-snapshot",
    ]);

    assert!(
        output.status.success(),
        "snapshot failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let event = terminal_json(&output);
    assert_eq!(event["ok"], true);
    assert_eq!(event["data"]["trunk"], "main");
    assert_eq!(event["data"]["current_branch"], "feature/ui");
    assert_eq!(event["data"]["repository_state"], "normal");
    let names = event["data"]["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|branch| branch["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["feature/ui", "feature/base", "main"]);
    assert_eq!(event["data"]["branches"][0]["parent"], "feature/base");
    assert_eq!(
        event["data"]["branches"][0]["recommended_action"],
        "submit_stack"
    );
    let generation = event["data"]["generation"].as_str().unwrap();
    assert_eq!(generation.len(), 16);
    assert!(
        generation
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );

    let second_output = repo.run_stax(&[
        "desktop",
        "snapshot",
        "--repo",
        &repo_path,
        "--schema-version",
        "1",
        "--request-id",
        "req-snapshot-2",
    ]);
    assert!(second_output.status.success());
    let second_event = terminal_json(&second_output);
    assert_eq!(second_event["data"]["generation"], generation);
}

#[test]
fn snapshot_rejects_a_non_repository_folder() {
    let runner = TestRepo::new();
    let invalid_repo = tempfile::TempDir::new().unwrap();
    let invalid_path = invalid_repo.path().to_string_lossy().into_owned();
    let output = runner.run_stax(&[
        "desktop",
        "snapshot",
        "--repo",
        &invalid_path,
        "--schema-version",
        "1",
        "--request-id",
        "req-invalid-repo",
    ]);

    assert!(!output.status.success());
    let event = terminal_json(&output);
    assert_eq!(event["ok"], false);
    assert_eq!(event["error"]["code"], "invalid_repository");
    assert_eq!(event["error"]["recovery"], "choose_repository");
}

#[test]
fn diff_reports_structured_text_changes() {
    let repo = TestRepo::new();
    std::fs::create_dir(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/example.txt"), "old\nkeep\n").unwrap();
    assert!(repo.git(&["add", "src/example.txt"]).status.success());
    assert!(repo.git(&["commit", "-m", "Add example"]).status.success());
    assert!(repo.run_stax(&["create", "feature/diff"]).status.success());
    std::fs::write(repo.path().join("src/example.txt"), "new\nkeep\n").unwrap();
    assert!(repo.git(&["add", "src/example.txt"]).status.success());
    assert!(
        repo.git(&["commit", "-m", "Update example"])
            .status
            .success()
    );

    let output = desktop_diff(&repo, "feature/diff", "req-diff");

    assert!(
        output.status.success(),
        "diff failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let event = terminal_json(&output);
    assert_eq!(event["data"]["branch"], "feature/diff");
    assert_eq!(event["data"]["parent"], "main");
    assert_eq!(event["data"]["files"][0]["path"], "src/example.txt");
    let lines = event["data"]["lines"].as_array().unwrap();
    assert!(lines.iter().any(|line| line["kind"] == "addition"));
    assert!(lines.iter().any(|line| line["kind"] == "deletion"));
    assert_eq!(event["data"]["truncated"], false);
}

#[test]
fn diff_reports_an_empty_tracked_branch() {
    let repo = TestRepo::new();
    assert!(repo.run_stax(&["create", "feature/empty"]).status.success());

    let output = desktop_diff(&repo, "feature/empty", "req-empty-diff");

    assert!(output.status.success());
    let event = terminal_json(&output);
    assert_eq!(event["data"]["files"], serde_json::json!([]));
    assert_eq!(event["data"]["lines"], serde_json::json!([]));
    assert_eq!(event["data"]["truncated"], false);
}

#[test]
fn diff_rejects_an_unknown_branch() {
    let repo = TestRepo::new();

    let output = desktop_diff(&repo, "missing/branch", "req-missing-diff");

    assert!(!output.status.success());
    let event = terminal_json(&output);
    assert_eq!(event["error"]["code"], "branch_not_found");
    assert_eq!(event["error"]["recovery"], "refresh");
}

#[test]
fn diff_truncates_oversized_patch_before_transport_limit() {
    let repo = TestRepo::new();
    assert!(
        repo.run_stax(&["create", "feature/large-diff"])
            .status
            .success()
    );
    let large_line = format!("{}\n", "x".repeat(470 * 1024));
    std::fs::write(repo.path().join("large.txt"), large_line).unwrap();
    assert!(repo.git(&["add", "large.txt"]).status.success());
    assert!(
        repo.git(&["commit", "-m", "Add large file"])
            .status
            .success()
    );

    let output = desktop_diff(&repo, "feature/large-diff", "req-large-diff");

    assert!(output.status.success());
    assert!(output.stdout.len() < 512 * 1024);
    let event = terminal_json(&output);
    assert_eq!(event["data"]["truncated"], true);
}

#[test]
fn action_checkout_switches_to_the_selected_branch() {
    let repo = TestRepo::new();
    assert!(
        repo.run_stax(&["create", "feature/action-checkout"])
            .status
            .success()
    );
    assert!(repo.git(&["checkout", "main"]).status.success());

    let output = desktop_action(
        &repo,
        "checkout",
        Some("feature/action-checkout"),
        "req-action-checkout",
        &[],
    );

    assert!(
        output.status.success(),
        "checkout failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(repo.current_branch(), "feature/action-checkout");
    let events = json_lines(&output);
    assert!(events.iter().any(|event| event["phase"] == "checking_out"));
    let terminal = events.last().unwrap();
    assert_eq!(terminal["type"], "result");
    assert_eq!(terminal["ok"], true);
    assert_eq!(terminal["data"]["action"], "checkout");
}

#[test]
fn action_restack_rejects_a_dirty_repository() {
    let repo = TestRepo::new();
    assert!(
        repo.run_stax(&["create", "feature/dirty-restack"])
            .status
            .success()
    );
    std::fs::write(repo.path().join("dirty.txt"), "dirty\n").unwrap();

    let output = desktop_action(
        &repo,
        "restack",
        Some("feature/dirty-restack"),
        "req-action-dirty",
        &[],
    );

    assert!(!output.status.success());
    let events = json_lines(&output);
    assert_eq!(events.last().unwrap()["error"]["code"], "dirty_repository");
}

#[test]
fn action_rejects_an_unknown_branch() {
    let repo = TestRepo::new();

    let output = desktop_action(
        &repo,
        "checkout",
        Some("missing/action-branch"),
        "req-action-missing",
        &[],
    );

    assert!(!output.status.success());
    let events = json_lines(&output);
    assert_eq!(events.last().unwrap()["error"]["code"], "branch_not_found");
}

#[test]
fn action_open_pr_without_metadata_preserves_current_branch() {
    let repo = TestRepo::new();
    assert!(repo.run_stax(&["create", "feature/no-pr"]).status.success());
    assert!(repo.git(&["checkout", "main"]).status.success());

    let output = desktop_action(
        &repo,
        "open-pr",
        Some("feature/no-pr"),
        "req-action-no-pr",
        &[("STAX_DESKTOP_NO_OPEN", "1")],
    );

    assert!(!output.status.success());
    assert_eq!(repo.current_branch(), "main");
    let events = json_lines(&output);
    assert_eq!(events.last().unwrap()["error"]["code"], "no_pull_request");
}

#[test]
fn action_submit_captures_child_output_as_machine_json() {
    let repo = TestRepo::new();
    assert!(
        repo.run_stax(&["create", "feature/submit-no-remote"])
            .status
            .success()
    );
    std::fs::write(repo.path().join("submit.txt"), "submit\n").unwrap();
    assert!(repo.git(&["add", "submit.txt"]).status.success());
    assert!(
        repo.git(&["commit", "-m", "Add submit change"])
            .status
            .success()
    );

    let output = desktop_action(
        &repo,
        "submit-stack",
        Some("feature/submit-no-remote"),
        "req-action-submit",
        &[],
    );

    assert!(!output.status.success());
    let events = json_lines(&output);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "result")
            .count(),
        1
    );
    let terminal = events.last().unwrap();
    assert_eq!(terminal["type"], "result");
    assert_eq!(terminal["ok"], false);
    assert_eq!(terminal["error"]["code"], "operation_failed");
}
