use crate::common::{OutputAssertions, TestRepo};

#[test]
fn trace_reports_git_subprocess_count_and_total_time() {
    let repo = TestRepo::new();
    repo.create_stack(&["trace-a", "trace-b"]);

    let output = repo.run_stax(&["--trace", "status", "--json"]);
    output.assert_success();

    let stderr = TestRepo::stderr(&output);
    assert!(stderr.contains("[trace] git #"), "stderr:\n{stderr}");
    assert!(stderr.contains("git commands in"), "stderr:\n{stderr}");
}
