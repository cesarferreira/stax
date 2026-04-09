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

fn file_content(repo: &TestRepo, branch: &str, path: &str) -> String {
    let output = repo.git(&["show", &format!("{}:{}", branch, path)]);
    String::from_utf8_lossy(&output.stdout).to_string()
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
    assert!(
        branches.contains(&split_1),
        "Missing {split_1}, got: {branches:?}"
    );
    assert!(
        branches.contains(&original),
        "Missing {original}, got: {branches:?}"
    );

    let parents = parent_map(&repo);
    assert_eq!(parents.get(&split_1).map(String::as_str), Some("main"));
    assert_eq!(
        parents.get(&original).map(String::as_str),
        Some(split_1.as_str())
    );

    // Each branch should introduce exactly one of the two files
    let s1_files = introduced_files(&repo, "main", &split_1);
    let orig_files = introduced_files(&repo, &split_1, &original);
    assert!(
        (s1_files.contains("extra.txt") && orig_files.contains("feature-a.txt"))
            || (s1_files.contains("feature-a.txt") && orig_files.contains("extra.txt")),
        "Each branch should introduce one file. split_1: {:?}, original: {:?}",
        s1_files,
        orig_files
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
    assert!(
        branches.contains(&split_1),
        "Missing {split_1}, got: {branches:?}"
    );
    assert!(
        branches.contains(&split_2),
        "Missing {split_2}, got: {branches:?}"
    );
    assert!(
        branches.contains(&original),
        "Missing {original}, got: {branches:?}"
    );

    let parents = parent_map(&repo);
    assert_eq!(parents.get(&split_1).map(String::as_str), Some("main"));
    assert_eq!(
        parents.get(&split_2).map(String::as_str),
        Some(split_1.as_str())
    );
    assert_eq!(
        parents.get(&original).map(String::as_str),
        Some(split_2.as_str())
    );
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
        s1_files,
        orig_files
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

// =============================================================================
// Regression tests: same-file multi-hunk splits (stale offset bug)
// =============================================================================

#[test]
fn test_split_hunk_same_file_two_hunks() {
    let repo = TestRepo::new();

    let base_content: String = (1..=30).map(|i| format!("line {}\n", i)).collect();
    repo.create_file("shared.txt", &base_content);
    repo.commit("add shared file");

    let output = repo.run_stax(&["bc", "same-file-split"]);
    assert!(
        output.status.success(),
        "bc failed: {}",
        TestRepo::stderr(&output)
    );
    let original = repo.current_branch();

    let modified: String = (1..=30)
        .map(|i| match i {
            3 => "line 3 MODIFIED\n".to_string(),
            25 => "line 25 MODIFIED\n".to_string(),
            _ => format!("line {}\n", i),
        })
        .collect();
    repo.create_file("shared.txt", &modified);
    repo.commit("modify shared file in two places");

    run_split_hunk(&repo, 2);

    let split_1 = format!("{}_split_1", original);
    let branches = repo.list_branches();
    assert!(
        branches.contains(&split_1),
        "Missing {split_1}, got: {branches:?}"
    );
    assert!(
        branches.contains(&original),
        "Missing {original}, got: {branches:?}"
    );

    let parents = parent_map(&repo);
    assert_eq!(parents.get(&split_1).map(String::as_str), Some("main"));
    assert_eq!(
        parents.get(&original).map(String::as_str),
        Some(split_1.as_str())
    );

    let s1_content = file_content(&repo, &split_1, "shared.txt");
    assert!(
        s1_content.contains("line 3 MODIFIED"),
        "split_1 should have line 3 modification"
    );
    assert!(
        !s1_content.contains("line 25 MODIFIED"),
        "split_1 should NOT have line 25 modification"
    );

    let final_content = file_content(&repo, &original, "shared.txt");
    assert!(
        final_content.contains("line 3 MODIFIED"),
        "final branch should have line 3 modification"
    );
    assert!(
        final_content.contains("line 25 MODIFIED"),
        "final branch should have line 25 modification"
    );
}

#[test]
fn test_split_hunk_same_file_three_hunks() {
    let repo = TestRepo::new();

    let base_content: String = (1..=40).map(|i| format!("line {}\n", i)).collect();
    repo.create_file("shared.txt", &base_content);
    repo.commit("add shared file");

    let output = repo.run_stax(&["bc", "three-way-split"]);
    assert!(
        output.status.success(),
        "bc failed: {}",
        TestRepo::stderr(&output)
    );
    let original = repo.current_branch();

    let modified: String = (1..=40)
        .map(|i| match i {
            3 => "line 3 MODIFIED\n".to_string(),
            20 => "line 20 MODIFIED\n".to_string(),
            37 => "line 37 MODIFIED\n".to_string(),
            _ => format!("line {}\n", i),
        })
        .collect();
    repo.create_file("shared.txt", &modified);
    repo.commit("modify shared file in three places");

    run_split_hunk(&repo, 3);

    let split_1 = format!("{}_split_1", original);
    let split_2 = format!("{}_split_2", original);
    let branches = repo.list_branches();
    assert!(
        branches.contains(&split_1),
        "Missing {split_1}, got: {branches:?}"
    );
    assert!(
        branches.contains(&split_2),
        "Missing {split_2}, got: {branches:?}"
    );
    assert!(
        branches.contains(&original),
        "Missing {original}, got: {branches:?}"
    );

    let parents = parent_map(&repo);
    assert_eq!(parents.get(&split_1).map(String::as_str), Some("main"));
    assert_eq!(
        parents.get(&split_2).map(String::as_str),
        Some(split_1.as_str())
    );
    assert_eq!(
        parents.get(&original).map(String::as_str),
        Some(split_2.as_str())
    );

    let s1_content = file_content(&repo, &split_1, "shared.txt");
    assert!(s1_content.contains("line 3 MODIFIED"));
    assert!(!s1_content.contains("line 20 MODIFIED"));
    assert!(!s1_content.contains("line 37 MODIFIED"));

    let s2_content = file_content(&repo, &split_2, "shared.txt");
    assert!(s2_content.contains("line 3 MODIFIED"));
    assert!(s2_content.contains("line 20 MODIFIED"));
    assert!(!s2_content.contains("line 37 MODIFIED"));

    let final_content = file_content(&repo, &original, "shared.txt");
    assert!(final_content.contains("line 3 MODIFIED"));
    assert!(final_content.contains("line 20 MODIFIED"));
    assert!(final_content.contains("line 37 MODIFIED"));
}

#[test]
fn test_split_hunk_line_additions_partial_select() {
    let repo = TestRepo::new();

    // Create base file with enough lines for 4 hunks (lines 8 apart minimum)
    let base: String = (1..=60).map(|i| format!("line {}\n", i)).collect();
    repo.create_file("main.txt", &base);
    repo.commit("add base file");

    let output = repo.run_stax(&["bc", "add-lines-split"]);
    assert!(
        output.status.success(),
        "bc failed: {}",
        TestRepo::stderr(&output)
    );
    let original = repo.current_branch();

    // ADD new lines (not modify) at 4 locations — this shifts offsets between hunks
    let modified: String = (1..=60)
        .flat_map(|i| {
            let mut lines = vec![format!("line {}\n", i)];
            match i {
                5 => {
                    lines.push("ADDED AFTER 5a\n".to_string());
                    lines.push("ADDED AFTER 5b\n".to_string());
                }
                18 => {
                    lines.push("ADDED AFTER 18a\n".to_string());
                    lines.push("ADDED AFTER 18b\n".to_string());
                    lines.push("ADDED AFTER 18c\n".to_string());
                }
                35 => {
                    lines.push("ADDED AFTER 35a\n".to_string());
                }
                50 => {
                    lines.push("ADDED AFTER 50a\n".to_string());
                    lines.push("ADDED AFTER 50b\n".to_string());
                }
                _ => {}
            }
            lines
        })
        .collect();
    repo.create_file("main.txt", &modified);
    repo.commit("add lines at 4 locations");

    // Flat list:
    //   [0] FileHeader(main.txt)
    //   [1] Hunk 0 (adds 2 lines after line 5)
    //   [2] Hunk 1 (adds 3 lines after line 18)
    //   [3] Hunk 2 (adds 1 line after line 35)
    //   [4] Hunk 3 (adds 2 lines after line 50)
    //
    // Round 1: select hunk 0 only (j, space, Enter, Enter)
    // Round 2: select hunk 1 only (j, space, Enter, Enter)
    // Round 3: select hunks 2 and 3 (j, space, j, space, Enter, Enter)

    let script = [
        "sleep 1",
        "printf 'j \\r\\r'",
        "sleep 3",
        "printf 'j \\r\\r'",
        "sleep 3",
        "printf 'j j \\r\\r'",
        "sleep 2",
    ]
    .join("; ");
    let output = common::run_stax_in_script(&repo.path(), &["split", "--hunk"], &script);
    assert!(
        output.status.success(),
        "Split TUI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let split_1 = format!("{}_split_1", original);
    let split_2 = format!("{}_split_2", original);
    let branches = repo.list_branches();
    assert!(
        branches.contains(&split_1),
        "Missing {split_1}, got: {branches:?}"
    );
    assert!(
        branches.contains(&split_2),
        "Missing {split_2}, got: {branches:?}"
    );
    assert!(
        branches.contains(&original),
        "Missing {original}, got: {branches:?}"
    );

    // Verify split_1 only has hunk 0
    let s1 = file_content(&repo, &split_1, "main.txt");
    assert!(s1.contains("ADDED AFTER 5a"), "split_1 should have hunk 0");
    assert!(
        !s1.contains("ADDED AFTER 18a"),
        "split_1 should NOT have hunk 1"
    );

    // Verify split_2 has hunks 0+1
    let s2 = file_content(&repo, &split_2, "main.txt");
    assert!(s2.contains("ADDED AFTER 5a"));
    assert!(s2.contains("ADDED AFTER 18a"));
    assert!(!s2.contains("ADDED AFTER 35a"));

    // Final branch has everything
    let fin = file_content(&repo, &original, "main.txt");
    assert!(fin.contains("ADDED AFTER 5a"));
    assert!(fin.contains("ADDED AFTER 18a"));
    assert!(fin.contains("ADDED AFTER 35a"));
    assert!(fin.contains("ADDED AFTER 50a"));
}

#[test]
fn test_split_hunk_partial_file_selection_gets_second_round() {
    let repo = TestRepo::new();

    // Create a base file on main with enough lines for 4 well-separated hunks
    let base_a: String = (1..=50).map(|i| format!("a line {}\n", i)).collect();
    repo.create_file("file_a.txt", &base_a);
    let base_b: String = (1..=20).map(|i| format!("b line {}\n", i)).collect();
    repo.create_file("file_b.txt", &base_b);
    repo.commit("add base files");

    let output = repo.run_stax(&["bc", "partial-select"]);
    assert!(
        output.status.success(),
        "bc failed: {}",
        TestRepo::stderr(&output)
    );
    let original = repo.current_branch();

    // Modify file_a in 4 well-separated locations (4 hunks)
    let mod_a: String = (1..=50)
        .map(|i| match i {
            3 => "a line 3 MODIFIED\n".to_string(),
            15 => "a line 15 MODIFIED\n".to_string(),
            30 => "a line 30 MODIFIED\n".to_string(),
            45 => "a line 45 MODIFIED\n".to_string(),
            _ => format!("a line {}\n", i),
        })
        .collect();
    repo.create_file("file_a.txt", &mod_a);

    // Modify file_b in 2 well-separated locations (2 hunks)
    let mod_b: String = (1..=20)
        .map(|i| match i {
            3 => "b line 3 MODIFIED\n".to_string(),
            15 => "b line 15 MODIFIED\n".to_string(),
            _ => format!("b line {}\n", i),
        })
        .collect();
    repo.create_file("file_b.txt", &mod_b);
    repo.commit("modify both files");

    // Flat list:
    //   [0] FileHeader(file_a.txt)
    //   [1] Hunk(A, 0) - line 3
    //   [2] Hunk(A, 1) - line 15
    //   [3] Hunk(A, 2) - line 30
    //   [4] Hunk(A, 3) - line 45
    //   [5] FileHeader(file_b.txt)
    //   [6] Hunk(B, 0) - line 3
    //   [7] Hunk(B, 1) - line 15
    //
    // Round 1: select A:0, A:1, and all of B (skip A:2, A:3)
    //   j=A:0, space=select, j=A:1, space=select, j=A:2, j=A:3, j=FileHeader(B), a=select-all-B
    //   Enter=commit, Enter=accept name
    //
    // Round 2: should have A:2, A:3 remaining
    //   j=A:2(now idx 0), space=select, j=A:3(now idx 1), space=select
    //   Enter=commit, Enter=accept name

    let script = "sleep 1; printf 'j j jjja\\r\\r'; sleep 3; printf 'j j \\r\\r'; sleep 2";
    let output = common::run_stax_in_script(&repo.path(), &["split", "--hunk"], &script);
    assert!(
        output.status.success(),
        "Split hunk TUI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let split_1 = format!("{}_split_1", original);
    let branches = repo.list_branches();
    assert!(
        branches.contains(&split_1),
        "Missing {split_1}, got: {branches:?}"
    );
    assert!(
        branches.contains(&original),
        "Missing {original}, got: {branches:?}"
    );

    // split_1 should have A:0, A:1 + all of B
    let s1_a = file_content(&repo, &split_1, "file_a.txt");
    assert!(s1_a.contains("a line 3 MODIFIED"));
    assert!(s1_a.contains("a line 15 MODIFIED"));
    assert!(
        !s1_a.contains("a line 30 MODIFIED"),
        "split_1 should NOT have A hunk 2"
    );
    assert!(
        !s1_a.contains("a line 45 MODIFIED"),
        "split_1 should NOT have A hunk 3"
    );

    // original should have all modifications
    let final_a = file_content(&repo, &original, "file_a.txt");
    assert!(final_a.contains("a line 3 MODIFIED"));
    assert!(final_a.contains("a line 15 MODIFIED"));
    assert!(
        final_a.contains("a line 30 MODIFIED"),
        "final branch should have A hunk 2"
    );
    assert!(
        final_a.contains("a line 45 MODIFIED"),
        "final branch should have A hunk 3"
    );
}
