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
