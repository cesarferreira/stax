use crate::common::{OutputAssertions, TestRepo};

#[test]
fn submit_plan_json_is_read_only_and_describes_stack_actions() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["plan-parent", "plan-child"]);
    repo.configure_github_like_submit_remote();

    let refs_before = TestRepo::stdout(&repo.git(&["show-ref"]));
    let status_before = TestRepo::stdout(&repo.git(&["status", "--porcelain=v1"]));

    let output = repo.run_stax(&["submit", "--plan", "--json"]);
    output.assert_success();

    let plan: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("submit plan should be valid JSON");
    assert_eq!(plan["schema_version"], 1);
    assert_eq!(plan["read_only"], true);
    assert_eq!(plan["scope"], "stack");
    assert_eq!(plan["remote"], "origin");
    assert_eq!(plan["fetch"]["action"], "fetch");

    let planned = plan["branches"].as_array().expect("branches array");
    assert_eq!(planned.len(), 2);
    assert_eq!(planned[0]["branch"], branches[0]);
    assert_eq!(planned[0]["parent"], "main");
    assert_eq!(planned[0]["push"], "create");
    assert_eq!(planned[0]["pull_request"], "create");
    assert_eq!(planned[1]["branch"], branches[1]);
    assert_eq!(planned[1]["parent"], branches[0]);

    assert_eq!(TestRepo::stdout(&repo.git(&["show-ref"])), refs_before);
    assert_eq!(
        TestRepo::stdout(&repo.git(&["status", "--porcelain=v1"])),
        status_before
    );
}

#[test]
fn submit_plan_reports_invalid_repository_without_mutating_it() {
    let repo = TestRepo::new();
    repo.create_stack(&["local-only"]);
    let refs_before = TestRepo::stdout(&repo.git(&["show-ref"]));

    let output = repo.run_stax(&["submit", "--dry-run", "--json"]);
    output.assert_failure();
    output.assert_stderr_contains("remote");
    assert_eq!(TestRepo::stdout(&repo.git(&["show-ref"])), refs_before);
}

#[test]
fn completions_are_available_without_an_initialized_repository() {
    let repo = TestRepo::new();
    for (shell, marker) in [
        ("bash", "complete"),
        ("zsh", "_st"),
        ("fish", "complete -c st"),
        ("powershell", "Register-ArgumentCompleter"),
        ("elvish", "edit:completion:arg-completer[st]"),
    ] {
        let output = repo.run_stax(&["completions", shell]);
        output.assert_success();
        assert!(
            TestRepo::stdout(&output).contains(marker),
            "{shell} completions should contain {marker:?}"
        );
    }
}

#[test]
fn completions_reject_an_unknown_shell() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["completions", "nushell"]);
    output.assert_failure();
    output.assert_stderr_contains("invalid value");
}
