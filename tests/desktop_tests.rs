use crate::common::TestRepo;
use serde_json::Value;

fn terminal_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "desktop stdout was not one JSON object: {error}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    })
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
