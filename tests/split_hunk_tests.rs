mod common;

use common::{OutputAssertions, TestRepo};
use std::collections::{HashMap, HashSet};

#[test]
fn test_split_hunk_help() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["split", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("--hunk"),
        "Expected --hunk in help output, got: {}",
        stdout
    );
}

// =============================================================================
// Error Case Tests (validation before TUI)
// =============================================================================

#[test]
fn test_split_hunk_on_trunk_fails() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["split", "--hunk"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("trunk") || stderr.contains("Cannot split"),
        "Expected trunk error, got: {}",
        stderr
    );
}

#[test]
fn test_split_hunk_untracked_branch_fails() {
    let repo = TestRepo::new();
    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.create_file("file1.txt", "content");
    repo.commit("commit 1");

    let output = repo.run_stax(&["split", "--hunk"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("not tracked") || stderr.contains("track"),
        "Expected untracked error, got: {}",
        stderr
    );
}

#[test]
fn test_split_commit_mode_single_commit_suggests_hunk() {
    let repo = TestRepo::new();
    repo.create_stack(&["single-commit"]);

    let output = repo.run_stax(&["split"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("--hunk") || stderr.contains("hunk"),
        "Expected hint about --hunk for single commit, got: {}",
        stderr
    );
}

#[test]
fn test_split_hunk_requires_terminal() {
    let repo = TestRepo::new();
    repo.create_stack(&["test-branch"]);
    repo.create_file("file1.txt", "content");
    repo.commit("commit 1");

    let output = repo.run_stax(&["split", "--hunk"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("terminal") || stderr.contains("interactive"),
        "Expected terminal requirement error, got: {}",
        stderr
    );
}

// =============================================================================
// End-to-end success tests (scripted TUI via pseudo-terminal)
// =============================================================================

/// Each round: j(down to hunk), Space(select), Enter(finish round), Enter(accept name).
fn split_hunk_script(rounds: usize) -> String {
    let mut parts = vec!["sleep 1".to_string()];
    for _ in 0..rounds {
        parts.push("printf 'j \\r\\r'".to_string());
        parts.push("sleep 2".to_string());
    }
    parts.join("; ")
}

fn parent_map(repo: &TestRepo) -> HashMap<String, String> {
    let json = repo.get_status_json();
    json["branches"]
        .as_array()
        .map(|branches| {
            branches
                .iter()
                .filter_map(|b| {
                    let name = b["name"].as_str()?;
                    let parent = b["parent"].as_str()?;
                    Some((name.to_string(), parent.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn introduced_files(repo: &TestRepo, base: &str, branch: &str) -> HashSet<String> {
    let output = repo.git(&["diff", "--name-only", base, branch]);
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn run_split_hunk(repo: &TestRepo, rounds: usize) {
    let script = split_hunk_script(rounds);
    let output = common::run_stax_in_script(&repo.path(), &["split", "--hunk"], &script);
    assert!(
        output.status.success(),
        "Split hunk TUI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_split_hunk_two_files_into_two_branches() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-a"]);
    let original = repo.current_branch();
    repo.create_file("extra.txt", "extra content\n");
    repo.commit("add extra file");

    run_split_hunk(&repo, 2);

    let split_1 = format!("{}_split_1", original);
    let branches = repo.list_branches();
    assert!(branches.contains(&split_1), "Missing {split_1}, got: {branches:?}");
    assert!(branches.contains(&original), "Missing {original}, got: {branches:?}");

    let parents = parent_map(&repo);
    assert_eq!(parents.get(&split_1).map(String::as_str), Some("main"));
    assert_eq!(parents.get(&original).map(String::as_str), Some(split_1.as_str()));

    // Each branch should introduce exactly one of the two files
    let s1_files = introduced_files(&repo, "main", &split_1);
    let orig_files = introduced_files(&repo, &split_1, &original);
    assert!(
        (s1_files.contains("extra.txt") && orig_files.contains("feature-a.txt"))
            || (s1_files.contains("feature-a.txt") && orig_files.contains("extra.txt")),
        "Each branch should introduce one file. split_1: {:?}, original: {:?}",
        s1_files, orig_files
    );
}

#[test]
fn test_split_hunk_three_files_three_branches() {
    let repo = TestRepo::new();
    repo.create_stack(&["multi-split"]);
    let original = repo.current_branch();
    repo.create_file("file_b.txt", "content b\n");
    repo.commit("add file b");
    repo.create_file("file_c.txt", "content c\n");
    repo.commit("add file c");

    run_split_hunk(&repo, 3);

    let split_1 = format!("{}_split_1", original);
    let split_2 = format!("{}_split_2", original);
    let branches = repo.list_branches();
    assert!(branches.contains(&split_1), "Missing {split_1}, got: {branches:?}");
    assert!(branches.contains(&split_2), "Missing {split_2}, got: {branches:?}");
    assert!(branches.contains(&original), "Missing {original}, got: {branches:?}");

    let parents = parent_map(&repo);
    assert_eq!(parents.get(&split_1).map(String::as_str), Some("main"));
    assert_eq!(parents.get(&split_2).map(String::as_str), Some(split_1.as_str()));
    assert_eq!(parents.get(&original).map(String::as_str), Some(split_2.as_str()));
}

#[test]
fn test_split_hunk_children_reparented() {
    let repo = TestRepo::new();
    let stack = repo.create_stack(&["parent-branch", "child-branch"]);
    let child = stack[1].clone();

    repo.run_stax(&["checkout", &stack[0]]).assert_success();
    let parent_name = repo.current_branch();
    repo.create_file("second.txt", "second content\n");
    repo.commit("add second file");

    run_split_hunk(&repo, 2);

    let parents = parent_map(&repo);
    assert_eq!(
        parents.get(&child).map(String::as_str),
        Some(parent_name.as_str()),
        "child's parent should be the last split branch (original name)"
    );
}

#[test]
fn test_split_hunk_with_new_file() {
    let repo = TestRepo::new();
    repo.create_stack(&["new-file-test"]);
    let original = repo.current_branch();
    repo.create_file("brand_new.txt", "brand new content\n");
    repo.commit("add brand new file");

    run_split_hunk(&repo, 2);

    let split_1 = format!("{}_split_1", original);
    let branches = repo.list_branches();
    assert!(branches.contains(&split_1));
    assert!(branches.contains(&original));

    let s1_files = introduced_files(&repo, "main", &split_1);
    let orig_files = introduced_files(&repo, &split_1, &original);
    assert!(
        s1_files.contains("brand_new.txt") ^ orig_files.contains("brand_new.txt"),
        "brand_new.txt should be introduced by exactly one branch: split_1={:?}, original={:?}",
        s1_files, orig_files
    );
}

#[test]
fn test_split_hunk_abort_with_dirty_workdir_preserves_changes() {
    let repo = TestRepo::new();
    repo.create_stack(&["dirty-test"]);
    let original = repo.current_branch();

    repo.create_file("tracked.txt", "tracked content\n");
    repo.commit("add tracked file");

    // Create uncommitted changes (dirty workdir)
    repo.create_file("dirty.txt", "dirty content\n");

    // Abort immediately: q(quit) y(confirm)
    let script = "sleep 1; printf 'qy'; sleep 2";
    let output = common::run_stax_in_script(&repo.path(), &["split", "--hunk"], script);

    assert!(
        output.status.success(),
        "Split hunk abort failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Should be back on the original branch
    assert_eq!(repo.current_branch(), original);

    // Dirty file should still exist in the working directory
    let dirty_path = repo.path().join("dirty.txt");
    assert!(
        dirty_path.exists(),
        "dirty.txt should be restored after abort"
    );
    let content = std::fs::read_to_string(&dirty_path).unwrap();
    assert_eq!(content, "dirty content\n");
}
