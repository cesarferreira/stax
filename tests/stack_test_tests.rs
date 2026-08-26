use crate::common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_stack_test_true_passes() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack with multiple branches
    repo.create_stack(&["test-a", "test-b"]);

    // Run `true` on all branches - should pass
    let output = repo.run_stax(&["test", "true"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("SUCCESS"),
        "Expected SUCCESS in output, got: {}",
        stdout
    );
    assert!(
        stdout.contains("succeeded"),
        "Expected 'succeeded' summary, got: {}",
        stdout
    );
}

#[test]
fn test_stack_test_false_fails() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    repo.create_stack(&["fail-a", "fail-b"]);

    // Run `false` on all branches - should fail
    let output = repo.run_stax(&["test", "false"]);
    output.assert_failure();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("FAIL"),
        "Expected FAIL in output, got: {}",
        stdout
    );
    assert!(
        stdout.contains("failed"),
        "Expected 'failed' summary, got: {}",
        stdout
    );
}

#[test]
fn test_stack_test_returns_to_original_branch() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    let branches = repo.create_stack(&["orig-a", "orig-b"]);

    // Go to the first branch
    repo.run_stax(&["checkout", &branches[0]]);
    let original = repo.current_branch();

    // Run test
    let _ = repo.run_stax(&["test", "true"]);

    // Should be back on original branch
    assert_eq!(
        repo.current_branch(),
        original,
        "Should return to original branch after stack test"
    );
}

#[test]
fn test_stack_test_fail_fast() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack with multiple branches
    repo.create_stack(&["ff-a", "ff-b", "ff-c"]);

    // Run `false` with --fail-fast
    let output = repo.run_stax(&["test", "--fail-fast", "false"]);
    output.assert_failure();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("fail-fast") || stdout.contains("Stopping"),
        "Expected fail-fast message, got: {}",
        stdout
    );
}

#[test]
fn test_stack_test_with_command() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    repo.create_stack(&["cmd-a"]);

    // Run a command that checks for a file
    let output = repo.run_stax(&["test", "test", "-f", "README.md"]);
    output.assert_success();
}

#[test]
fn test_stack_test_displays_command_output() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    repo.create_stack(&["out-a"]);

    // Command output should be forwarded to stax output
    let output = repo.run_stax(&["test", "echo", "hello-from-command"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("hello-from-command"),
        "Expected command output to be visible, got: {}",
        stdout
    );
}

#[test]
fn test_stack_run_alias_works() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    repo.create_stack(&["run-a"]);

    // Run the same command through the neutral alias
    let output = repo.run_stax(&["run", "true"]);
    output.assert_success();
}

#[test]
fn test_stack_run_stack_filter_limits_scope() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create first stack
    let stack_a = repo.create_stack(&["scope-a1", "scope-a2"]);

    // Create second, separate stack
    repo.run_stax(&["trunk"]).assert_success();
    let stack_b = repo.create_stack(&["scope-b1"]);

    // Run only stack A
    let output = repo.run_stax(&["run", &format!("--stack={}", stack_a[0]), "true"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains(&format!("{}:", stack_a[0])),
        "Expected stack root '{}' to run, got: {}",
        stack_a[0],
        stdout
    );
    assert!(
        stdout.contains(&format!("{}:", stack_a[1])),
        "Expected stack descendant '{}' to run, got: {}",
        stack_a[1],
        stdout
    );
    assert!(
        !stdout.contains(&format!("{}:", stack_b[0])),
        "Did not expect other stack branch '{}' to run, got: {}",
        stack_b[0],
        stdout
    );
}

#[test]
fn test_stack_run_stack_filter_unknown_branch_fails() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack so metadata exists
    repo.create_stack(&["unknown-a"]);

    let output = repo.run_stax(&["run", "--stack=does-not-exist", "true"]);
    output
        .assert_failure()
        .assert_stderr_contains("not tracked in the stack");
}

#[test]
fn test_stack_run_stack_flag_without_value_uses_current_stack() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create stack A and move to its tip
    let stack_a = repo.create_stack(&["current-a1", "current-a2"]);

    // Create stack B from trunk
    repo.run_stax(&["trunk"]).assert_success();
    let stack_b = repo.create_stack(&["current-b1"]);

    // Move back to stack A tip
    repo.run_stax(&["checkout", &stack_a[1]]).assert_success();

    // --stack with no value should resolve to current branch's stack
    let output = repo.run_stax(&["run", "--stack", "true"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains(&format!("{}:", stack_a[0])),
        "Expected current stack branch '{}' to run, got: {}",
        stack_a[0],
        stdout
    );
    assert!(
        stdout.contains(&format!("{}:", stack_a[1])),
        "Expected current stack branch '{}' to run, got: {}",
        stack_a[1],
        stdout
    );
    assert!(
        !stdout.contains(&format!("{}:", stack_b[0])),
        "Did not expect different stack branch '{}' to run, got: {}",
        stack_b[0],
        stdout
    );
}

#[test]
fn test_stack_run_all_excludes_trunk() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack above trunk
    let branches = repo.create_stack(&["all-a"]);

    // --all should include tracked branches but never trunk
    let output = repo.run_stax(&["run", "--all", "true"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains(&format!("{}:", branches[0])),
        "Expected tracked branch '{}' to run, got: {}",
        branches[0],
        stdout
    );
    assert!(
        !stdout.contains("main:"),
        "Trunk branch should not be executed under --all, got: {}",
        stdout
    );
}

#[test]
fn test_stack_run_parallel_uses_isolated_worktrees_and_restores_current_branch() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["parallel-a", "parallel-b"]);
    let original = repo.current_branch();

    let output = repo.run_stax(&["run", "--parallel", "--jobs", "2", "pwd"]);
    output.assert_success();
    output.assert_stdout_contains("parallel worktree");
    assert_eq!(repo.current_branch(), original);
    for branch in branches {
        output.assert_stdout_contains(&branch);
    }

    let worktrees = TestRepo::stdout(&repo.git(&["worktree", "list", "--porcelain"]));
    assert_eq!(worktrees.matches("worktree ").count(), 1);
}

#[test]
fn test_stack_run_parallel_exposes_original_branch_name() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["parallel-context-a", "parallel-context-b"]);

    let output = repo.run_stax(&[
        "run",
        "--parallel",
        "--jobs",
        "2",
        "printf 'context=%s\\n' \"$STAX_RUN_BRANCH\"",
    ]);

    output.assert_success();
    for branch in branches {
        output.assert_stdout_contains(&format!("context={branch}"));
    }
}

#[test]
fn test_stack_run_parallel_propagates_failures_and_cleans_worktrees() {
    let repo = TestRepo::new();
    repo.create_stack(&["parallel-fail-a", "parallel-fail-b"]);

    let output = repo.run_stax(&["run", "--parallel", "false"]);
    output.assert_failure();
    output.assert_stdout_contains("2 failed");
    let worktrees = TestRepo::stdout(&repo.git(&["worktree", "list", "--porcelain"]));
    assert_eq!(worktrees.matches("worktree ").count(), 1);
}

#[test]
fn test_stack_run_parallel_preserves_untracked_changes_for_recovery() {
    let repo = TestRepo::new();
    repo.create_stack(&["parallel-untracked-a", "parallel-untracked-b"]);

    let output = repo.run_stax(&[
        "run",
        "--parallel",
        "--jobs",
        "2",
        "printf report > untracked-report",
    ]);

    output.assert_failure();
    output.assert_stdout_contains("Command left uncommitted changes; preserved for recovery at");

    let worktrees = TestRepo::stdout(&repo.git(&["worktree", "list", "--porcelain"]));
    let preserved_paths: Vec<_> = worktrees
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .filter(|path| *path != repo.path().to_string_lossy())
        .collect();
    assert_eq!(
        preserved_paths.len(),
        2,
        "Expected both dirty worktrees to be preserved"
    );
    assert!(
        preserved_paths
            .iter()
            .all(|path| path.contains("stax-run-")),
        "Expected preserved worktrees to use stax-run recovery paths: {worktrees}"
    );
    let recovery_root = std::path::Path::new(preserved_paths[0])
        .parent()
        .expect("Preserved worktree should have a recovery root")
        .to_path_buf();

    for path in preserved_paths {
        repo.git(&["worktree", "remove", "--force", path])
            .assert_success();
    }
    std::fs::remove_dir_all(&recovery_root).expect("Failed to remove recovery root");
    let remaining = TestRepo::stdout(&repo.git(&["worktree", "list", "--porcelain"]));
    assert_eq!(remaining.matches("worktree ").count(), 1);
}

#[test]
fn test_stack_run_parallel_preserves_tracked_changes_for_recovery() {
    let repo = TestRepo::new();
    repo.create_stack(&["parallel-tracked-a", "parallel-tracked-b"]);

    let output = repo.run_stax(&[
        "run",
        "--parallel",
        "--jobs",
        "2",
        "printf report > tracked-report && git add tracked-report",
    ]);

    output.assert_failure();
    output.assert_stdout_contains("Command left uncommitted changes; preserved for recovery at");

    let worktrees = TestRepo::stdout(&repo.git(&["worktree", "list", "--porcelain"]));
    let preserved_paths: Vec<_> = worktrees
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .filter(|path| *path != repo.path().to_string_lossy())
        .collect();
    assert_eq!(
        preserved_paths.len(),
        2,
        "Expected both dirty worktrees to be preserved"
    );
    let recovery_root = std::path::Path::new(preserved_paths[0])
        .parent()
        .expect("Preserved worktree should have a recovery root")
        .to_path_buf();

    for path in preserved_paths {
        repo.git(&["worktree", "remove", "--force", path])
            .assert_success();
    }
    std::fs::remove_dir_all(&recovery_root).expect("Failed to remove recovery root");
    let remaining = TestRepo::stdout(&repo.git(&["worktree", "list", "--porcelain"]));
    assert_eq!(remaining.matches("worktree ").count(), 1);
}

#[test]
fn test_stack_run_parallel_cleans_ignored_artifacts() {
    let repo = TestRepo::new();
    repo.create_file(".gitignore", "ignored-report\n");
    repo.commit("Add ignored parallel-run artifact");
    repo.create_stack(&["parallel-ignored-a", "parallel-ignored-b"]);

    let output = repo.run_stax(&[
        "run",
        "--parallel",
        "--jobs",
        "2",
        "printf report > ignored-report",
    ]);

    output.assert_success();
    output.assert_stdout_not_contains("preserved for recovery");
    let worktrees = TestRepo::stdout(&repo.git(&["worktree", "list", "--porcelain"]));
    assert_eq!(worktrees.matches("worktree ").count(), 1);
}

#[test]
fn test_stack_run_parallel_rejects_zero_jobs() {
    let repo = TestRepo::new();
    repo.create_stack(&["parallel-jobs"]);
    let output = repo.run_stax(&["run", "--parallel", "--jobs", "0", "true"]);
    output.assert_failure();
    output.assert_stderr_contains("invalid value");
}
