use crate::common::{OutputAssertions, TestRepo};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn write_submit_config(
    home: &Path,
    native_stack: &str,
    stack_links_when_native: &str,
    single_stack: &str,
    forge: Option<&str>,
) {
    let config_dir = home.join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("config directory");
    fs::write(
        config_dir.join("config.toml"),
        format!(
            "{}[submit]\nstack_links = \"body\"\nsingle_stack = \"{single_stack}\"\nnative_stack = \"{native_stack}\"\nstack_links_when_native = \"{stack_links_when_native}\"\n",
            forge
                .map(|forge| format!("[remote]\nforge = \"{forge}\"\n\n"))
                .unwrap_or_default()
        ),
    )
    .expect("test config");
}

fn write_branch_pr_metadata(repo: &TestRepo, branch: &str, parent: &str, pr_number: u64) {
    let parent_revision = TestRepo::stdout(&repo.git(&["rev-parse", parent]))
        .trim()
        .to_string();
    let json = serde_json::json!({
        "parentBranchName": parent,
        "parentBranchRevision": parent_revision,
        "prInfo": {
            "number": pr_number,
            "state": "OPEN",
            "isDraft": false
        }
    })
    .to_string();

    let mut hash_child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(repo.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn git hash-object");
    hash_child
        .stdin
        .as_mut()
        .expect("hash-object stdin")
        .write_all(json.as_bytes())
        .expect("write metadata json");
    let hash_output = hash_child.wait_with_output().expect("wait hash-object");
    assert!(hash_output.status.success(), "git hash-object failed");
    let hash = String::from_utf8_lossy(&hash_output.stdout)
        .trim()
        .to_string();
    repo.git(&[
        "update-ref",
        &format!("refs/branch-metadata/{branch}"),
        &hash,
    ])
    .assert_success();
}

fn branch_plan<'a>(plan: &'a serde_json::Value, branch: &str) -> &'a serde_json::Value {
    plan["branches"]
        .as_array()
        .expect("branches array")
        .iter()
        .find(|entry| entry["branch"] == branch)
        .unwrap_or_else(|| panic!("missing plan for {branch}"))
}

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
    assert_eq!(plan["schema_version"], 2);
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
    assert_eq!(plan["native_stack"]["action"], "attempt");

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
fn submit_plan_uses_live_remote_heads_without_writing_tracking_refs() {
    let repo = TestRepo::new_with_remote();
    let branch = repo.create_stack(&["live-remote"]).remove(0);
    repo.configure_github_like_submit_remote();
    repo.git(&["push", "origin", &branch]).assert_success();
    repo.git(&["update-ref", "-d", &format!("refs/remotes/origin/{branch}")])
        .assert_success();
    let refs_before = TestRepo::stdout(&repo.git(&["show-ref"]));

    let output = repo.run_stax(&["submit", "--plan", "--json"]);
    output.assert_success();
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");

    assert_eq!(branch_plan(&plan, &branch)["push"], "none");
    assert_eq!(TestRepo::stdout(&repo.git(&["show-ref"])), refs_before);
}

#[test]
fn submit_plan_marks_temporary_restack_push_as_runtime_evaluated() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["plan-parent", "plan-child"]);
    repo.configure_github_like_submit_remote();
    repo.git(&["push", "origin", &branches[0], &branches[1]])
        .assert_success();
    repo.run_stax(&["checkout", &branches[0]]).assert_success();
    repo.create_file("parent-update.txt", "parent update\n");
    repo.commit("advance plan parent");
    repo.run_stax(&["checkout", &branches[1]]).assert_success();

    let output = repo.run_stax(&["submit", "--plan", "--json", "--no-fetch"]);
    output.assert_success();
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");
    let child = branch_plan(&plan, &branches[1]);

    assert_eq!(child["publish_source"], "temporary_restack");
    assert_eq!(child["push"], "evaluate_after_temporary_restack");
}

#[test]
fn submit_plan_propagates_temporary_restack_to_current_descendants() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["plan-parent", "plan-child", "plan-grandchild"]);
    repo.configure_github_like_submit_remote();
    repo.git(&["push", "origin", &branches[0], &branches[1], &branches[2]])
        .assert_success();
    repo.run_stax(&["checkout", &branches[0]]).assert_success();
    repo.create_file("parent-update.txt", "parent update\n");
    repo.commit("advance plan parent");
    repo.run_stax(&["checkout", &branches[2]]).assert_success();

    let output = repo.run_stax(&["submit", "--plan", "--json", "--no-fetch"]);
    output.assert_success();
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");
    let grandchild = branch_plan(&plan, &branches[2]);

    assert_eq!(grandchild["needs_restack"], false);
    assert_eq!(grandchild["publish_source"], "temporary_restack");
    assert_eq!(grandchild["push"], "evaluate_after_temporary_restack");
}

#[test]
fn submit_plan_honors_no_native_stack_when_deciding_stack_links() {
    let repo = TestRepo::new_with_remote();
    repo.create_stack(&["no-native-plan"]);
    repo.configure_github_like_submit_remote();
    let home = repo.clean_home();
    write_submit_config(Path::new(&home), "auto", "off", "on", None);

    let output = repo.run_stax(&["submit", "--plan", "--json", "--no-native-stack"]);
    output.assert_success();
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");

    assert_eq!(plan["native_stack"]["action"], "skip");
    assert_eq!(plan["stack_links"]["action"], "update");
}

#[test]
fn branch_submit_plan_counts_known_prs_in_the_stack_for_link_sync() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["plan-parent", "plan-child"]);
    repo.configure_github_like_submit_remote();
    write_branch_pr_metadata(&repo, &branches[0], "main", 101);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 102);
    let home = repo.clean_home();
    write_submit_config(Path::new(&home), "off", "off", "off", None);

    let output = repo.run_stax(&["branch", "submit", "--plan", "--json"]);
    output.assert_success();
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");

    assert_eq!(plan["branches"].as_array().expect("branches").len(), 1);
    assert_eq!(plan["stack_links"]["action"], "update");
}

#[test]
fn branch_submit_plan_defers_stack_link_decision_for_unknown_context_prs() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["known-pr", "unknown-pr"]);
    repo.configure_github_like_submit_remote();
    write_branch_pr_metadata(&repo, &branches[0], "main", 201);
    repo.run_stax(&["checkout", &branches[0]]).assert_success();
    let home = repo.clean_home();
    write_submit_config(Path::new(&home), "off", "off", "off", None);

    let output = repo.run_stax(&["branch", "submit", "--plan", "--json"]);
    output.assert_success();
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");

    assert_eq!(plan["stack_links"]["action"], "evaluate_after_pr_discovery");
}

#[test]
fn native_stack_plan_defers_fork_decision_until_unknown_prs_are_discovered() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["fork-root", "known-fork-child"]);
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["checkout", &branches[0]]).assert_success();
    repo.run_stax(&["create", "unknown-fork-child"])
        .assert_success();
    repo.create_file("unknown-fork-child.txt", "unknown child\n");
    repo.commit("Create unknown fork child");
    write_branch_pr_metadata(&repo, &branches[0], "main", 301);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 302);
    repo.run_stax(&["checkout", &branches[0]]).assert_success();
    let home = repo.clean_home();
    write_submit_config(Path::new(&home), "auto", "off", "off", None);

    let output = repo.run_stax(&["branch", "submit", "--plan", "--json"]);
    output.assert_success();
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");

    assert_eq!(
        plan["native_stack"]["action"],
        "evaluate_after_pr_discovery"
    );
    assert_eq!(
        plan["stack_links"]["action"],
        "update_unless_native_link_succeeds"
    );
}

#[test]
fn submit_plan_skips_native_stack_for_no_pr_single_pr_and_non_github() {
    let no_pr_repo = TestRepo::new_with_remote();
    no_pr_repo.create_stack(&["no-pr-parent", "no-pr-child"]);
    no_pr_repo.configure_github_like_submit_remote();
    let output = no_pr_repo.run_stax(&["submit", "--plan", "--json", "--no-pr"]);
    output.assert_success();
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");
    assert_eq!(plan["native_stack"]["action"], "skip");

    let single_repo = TestRepo::new_with_remote();
    single_repo.create_stack(&["single-pr"]);
    single_repo.configure_github_like_submit_remote();
    let output = single_repo.run_stax(&["submit", "--plan", "--json"]);
    output.assert_success();
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");
    assert_eq!(plan["native_stack"]["action"], "skip");

    let gitlab_repo = TestRepo::new_with_remote();
    gitlab_repo.create_stack(&["gitlab-parent", "gitlab-child"]);
    gitlab_repo.configure_github_like_submit_remote();
    let home = gitlab_repo.clean_home();
    write_submit_config(Path::new(&home), "auto", "off", "on", Some("gitlab"));
    let output = gitlab_repo.run_stax(&["submit", "--plan", "--json"]);
    output.assert_success();
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");
    assert_eq!(plan["native_stack"]["action"], "skip");
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
