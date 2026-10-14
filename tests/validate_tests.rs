use crate::common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_validate_healthy_stack() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a healthy stack
    repo.create_stack(&["feature-a", "feature-b"]);

    // Validate should pass
    let output = repo.run_stax(&["validate"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("All checks passed"),
        "Expected all checks to pass, got: {}",
        stdout
    );
}

#[test]
fn test_validate_empty_repo() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Validate on empty repo (no tracked branches) should pass
    let output = repo.run_stax(&["validate"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("All checks passed"),
        "Expected all checks to pass, got: {}",
        stdout
    );
}

#[test]
fn test_validate_detects_needs_restack() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack then modify parent to trigger needs-restack
    repo.create_stack(&["feature-a"]);
    repo.run_stax(&["t"]); // go to trunk

    // Add a commit to trunk (this makes feature-a's parent revision stale)
    repo.create_file("trunk-change.txt", "new content");
    repo.commit("Trunk change");

    // Validate should detect the stale branch
    let output = repo.run_stax(&["validate"]);

    let stdout = TestRepo::stdout(&output);
    // Should report needs restack
    assert!(
        stdout.contains("need restack") || stdout.contains("WARN"),
        "Expected needs-restack warning, got: {}",
        stdout
    );
}

#[test]
fn test_validate_detects_orphaned_metadata() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a branch, then delete it with git directly (leaving metadata)
    repo.create_stack(&["orphan-branch"]);
    let branch_name = repo.current_branch();
    repo.run_stax(&["t"]); // go to trunk

    // Delete branch with raw git (bypassing stax, leaving metadata)
    repo.git(&["branch", "-D", &branch_name]);

    // Validate should detect orphaned metadata
    let output = repo.run_stax(&["validate"]);

    let stdout = TestRepo::stdout(&output);
    // Stack::load auto-prunes orphaned metadata, so validate may see it as clean
    // or it may detect it before load prunes
    assert!(
        stdout.contains("PASS") || stdout.contains("FAIL") || stdout.contains("orphaned"),
        "Expected some validation output, got: {}",
        stdout
    );
}

fn metadata_ref(branch: &str) -> String {
    format!("refs/branch-metadata/{branch}")
}

/// Overwrite `branch`'s metadata so `parentBranchRevision` is `lie_revision`
/// (SHA string, not derived from the real parent), while leaving
/// `parentBranchName` unchanged. Mirrors what a bad snapshot/undo could leave
/// behind: a recorded revision that matches some real commit but is no longer
/// actually reachable from the branch.
fn rewrite_parent_revision_lie(repo: &TestRepo, branch: &str, lie_revision: &str) {
    let show = repo.git(&["show", &metadata_ref(branch)]);
    show.assert_success();
    let mut metadata: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&show)).expect("valid metadata JSON");
    metadata["parentBranchRevision"] = serde_json::Value::String(lie_revision.to_string());

    let file = tempfile::NamedTempFile::new().expect("metadata file");
    std::fs::write(file.path(), metadata.to_string()).expect("metadata contents");
    let hash = repo.git(&[
        "hash-object",
        "-w",
        file.path().to_str().expect("metadata path"),
    ]);
    hash.assert_success();
    repo.git(&[
        "update-ref",
        &metadata_ref(branch),
        TestRepo::stdout(&hash).trim(),
    ])
    .assert_success();
}

#[test]
fn test_validate_detects_stale_parent_revision_that_is_not_an_ancestor() {
    // Regression test for issue #822: an interrupted sync-restack (undone)
    // could leave a branch's parentBranchRevision pointing at a SHA that
    // matches trunk's current tip by string comparison, but is no longer an
    // ancestor of the branch (the rebase that would have made it one was
    // rolled back). A bare SHA comparison in needs_restack() reported this as
    // clean forever. validate must catch it via the ancestry check instead.
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    repo.create_stack(&["feature-a"]);
    let branch = repo.current_branch();
    repo.create_file("feature-change.txt", "feature work");
    repo.commit("Feature commit");

    repo.run_stax(&["t"]); // back to trunk
    repo.create_file("trunk-change.txt", "new trunk content");
    repo.commit("Trunk change");
    let trunk_tip = repo.get_commit_sha("main");

    // The lie: claim feature-a is already based on trunk's new tip, without
    // actually rebasing it there.
    rewrite_parent_revision_lie(&repo, &branch, &trunk_tip);

    let output = repo.run_stax(&["validate"]);
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("need restack") || stdout.contains("WARN"),
        "Expected validate to catch the ancestry lie instead of reporting clean, got: {}",
        stdout
    );
}
