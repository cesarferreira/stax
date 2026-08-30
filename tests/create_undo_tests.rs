//! Issue #839: `stax create` must wrap both its branch-first and commit-first
//! flows in a single `Transaction` so the whole create — branch ref, its
//! metadata ref, `--insert` child reparenting, and `--below` reparenting — is
//! one undoable unit.

use crate::common;

use common::{OutputAssertions, TestRepo};

/// Helper: read the newest op receipt under `.git/stax/ops` as JSON.
fn latest_receipt(repo: &TestRepo) -> serde_json::Value {
    let ops_dir = repo.path().join(".git/stax/ops");
    let mut entries: Vec<_> = std::fs::read_dir(&ops_dir)
        .expect("read ops dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    let newest = entries.last().expect("expected at least one op receipt");
    let content = std::fs::read_to_string(newest.path()).expect("read receipt");
    serde_json::from_str(&content).expect("invalid receipt JSON")
}

#[cfg(unix)]
fn install_failing_pre_commit_hook(repo: &TestRepo) {
    use std::os::unix::fs::PermissionsExt;

    let hooks_dir = repo.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).expect("create hooks dir");
    let hook = hooks_dir.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho hook failed >&2\nexit 1\n").expect("write failing hook");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
        .expect("chmod failing hook");
}

#[test]
fn create_insert_records_child_metadata_in_the_op_receipt() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let branches = repo.create_stack(&["ins-a", "ins-b"]);
    let ins_a = &branches[0];
    let ins_b = &branches[1];

    repo.run_stax(&["checkout", ins_a]).assert_success();
    repo.run_stax(&["create", "ins-mid", "--insert"])
        .assert_success();

    let receipt = latest_receipt(&repo);
    assert_eq!(receipt["kind"], "create");
    assert_eq!(receipt["status"], "success");

    let local_refs = receipt["local_refs"]
        .as_array()
        .expect("local_refs should be an array");

    let branch_entry = local_refs
        .iter()
        .find(|e| e["branch"].as_str() == Some("ins-mid"))
        .expect("expected ins-mid branch entry in receipt");
    assert!(branch_entry["oid_before"].is_null());
    assert!(branch_entry["oid_after"].is_string());

    let meta_entry = local_refs
        .iter()
        .find(|e| e["branch"].as_str() == Some("ins-mid@meta"))
        .expect("expected ins-mid@meta entry in receipt");
    assert!(meta_entry["oid_before"].is_null());
    assert!(meta_entry["oid_after"].is_string());

    let child_meta_entry = local_refs
        .iter()
        .find(|e| e["branch"].as_str() == Some(&format!("{}@meta", ins_b)))
        .unwrap_or_else(|| panic!("expected {}@meta entry in receipt", ins_b));
    assert!(child_meta_entry["oid_before"].is_string());
    assert!(child_meta_entry["oid_after"].is_string());
    assert_ne!(
        child_meta_entry["oid_before"], child_meta_entry["oid_after"],
        "child's metadata ref should have changed"
    );
}

#[test]
fn create_insert_undo_restores_the_childs_original_parent() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let branches = repo.create_stack(&["ins-a", "ins-b"]);
    let ins_a = &branches[0];
    let ins_b = &branches[1];

    repo.run_stax(&["checkout", ins_a]).assert_success();
    repo.run_stax(&["create", "ins-mid", "--insert"])
        .assert_success();

    let undo_output = repo.run_stax(&["undo", "--yes"]);
    undo_output.assert_success();

    let branches_after = repo.list_branches();
    assert!(
        !branches_after.iter().any(|b| b == "ins-mid"),
        "ins-mid should be gone after undo, got branches: {:?}",
        branches_after
    );
    assert_eq!(repo.current_branch(), *ins_a);

    repo.run_stax(&["checkout", ins_b]).assert_success();
    let parent = repo.get_current_parent();
    assert!(
        parent.as_ref().is_some_and(|p| p.contains(ins_a)),
        "expected {}'s parent to be restored to {}, got: {:?}",
        ins_b,
        ins_a,
        parent
    );
}

#[test]
fn create_below_undo_restores_the_original_parent() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let branches = repo.create_stack(&["bel-p", "bel-c"]);
    let bel_p = &branches[0];
    let bel_c = &branches[1];

    repo.run_stax(&["checkout", bel_c]).assert_success();
    repo.run_stax(&["create", "bel-low", "--below"])
        .assert_success();

    let undo_output = repo.run_stax(&["undo", "--yes"]);
    undo_output.assert_success();

    let branches_after = repo.list_branches();
    assert!(
        !branches_after.iter().any(|b| b == "bel-low"),
        "bel-low should be gone after undo, got branches: {:?}",
        branches_after
    );
    assert_eq!(repo.current_branch(), *bel_c);

    let parent = repo.get_current_parent();
    assert!(
        parent.as_ref().is_some_and(|p| p.contains(bel_p)),
        "expected {}'s parent to be restored to {}, got: {:?}",
        bel_c,
        bel_p,
        parent
    );
}

#[test]
fn create_commit_first_undo_removes_the_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.create_file("u.txt", "undo me content\n");
    let main_sha_before = repo.get_commit_sha("main");

    repo.run_stax(&["create", "-a", "-m", "undo me"])
        .assert_success();

    let undo_output = repo.run_stax(&["undo", "--yes"]);
    undo_output.assert_success();

    let branches_after = repo.list_branches();
    assert!(
        !branches_after.iter().any(|b| b.contains("undo-me")),
        "undo-me branch should be gone after undo, got branches: {:?}",
        branches_after
    );
    assert_eq!(repo.current_branch(), "main");
    assert_eq!(repo.get_commit_sha("main"), main_sha_before);
    assert!(
        repo.path().join("u.txt").exists(),
        "undo must not destroy the user's file"
    );
}

#[cfg(unix)]
#[test]
fn create_hook_failure_writes_no_receipt() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    install_failing_pre_commit_hook(&repo);

    repo.create_file("hook.txt", "hook fail content\n");

    let output = repo.run_stax(&["create", "-a", "-m", "hook fail"]);
    output.assert_failure();

    let ops_dir = repo.path().join(".git/stax/ops");
    let receipt_count = std::fs::read_dir(&ops_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                .count()
        })
        .unwrap_or(0);
    assert_eq!(
        receipt_count, 0,
        "a hook failure before the snapshot must not write a receipt"
    );
}
