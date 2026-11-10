//! A tracked branch whose stored `parentBranchRevision` is behind the real
//! merge-base (it was rebased outside stax, or a stax rebase was finished with
//! plain `git rebase --continue`). `stax validate` must flag it and `stax fix`
//! must repair it.

use crate::common;

use common::{OutputAssertions, TestRepo};

/// `main` gets a commit, `feat` is created on it, trunk moves, then `feat` is
/// rebased with plain git. Returns the stale boundary stored for `feat`.
fn externally_rebased_branch(repo: &TestRepo, trunk_moves_again: bool) -> String {
    repo.create_file("shared.txt", "a\n");
    repo.commit("add shared");

    let bc = repo.run_stax(&["bc", "feat"]);
    assert!(bc.status.success(), "bc feat: {}", TestRepo::stderr(&bc));
    let stale = stored_revision(repo, "feat");
    repo.create_file("feat.txt", "feat\n");
    repo.commit("feat commit");

    assert!(repo.git(&["checkout", "main"]).status.success());
    repo.create_file("shared.txt", "b\n");
    repo.commit("trunk edits shared");

    assert!(repo.git(&["checkout", "feat"]).status.success());
    let rebase = repo.git(&["rebase", "main"]);
    assert!(rebase.status.success(), "external rebase failed");

    if trunk_moves_again {
        assert!(repo.git(&["checkout", "main"]).status.success());
        repo.create_file("shared.txt", "c\n");
        repo.commit("trunk edits shared again");
        assert!(repo.git(&["checkout", "feat"]).status.success());
    }
    stale
}

fn stored_revision(repo: &TestRepo, branch: &str) -> String {
    let out = repo.git(&["cat-file", "-p", &format!("refs/branch-metadata/{branch}")]);
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&out)).expect("metadata is JSON");
    json["parentBranchRevision"]
        .as_str()
        .expect("parentBranchRevision")
        .to_string()
}

#[test]
fn fix_dry_run_reports_stale_boundary_without_changing_it() {
    let repo = TestRepo::new();
    let stale = externally_rebased_branch(&repo, true);

    let out = repo.run_stax(&["fix", "--dry-run"]);
    out.assert_success();
    let text = TestRepo::stdout(&out);
    assert!(text.contains("Stale parent boundary"), "got:\n{text}");
    assert!(text.contains("feat"), "got:\n{text}");
    assert_eq!(
        stored_revision(&repo, "feat"),
        stale,
        "dry run is read-only"
    );
}

#[test]
fn fix_moves_stale_boundary_to_merge_base_and_is_idempotent() {
    let repo = TestRepo::new();
    let stale = externally_rebased_branch(&repo, true);
    let merge_base = TestRepo::stdout(&repo.git(&["merge-base", "main", "feat"]));

    repo.run_stax(&["fix", "--yes"]).assert_success();
    let repaired = stored_revision(&repo, "feat");
    assert_ne!(repaired, stale);
    assert_eq!(repaired, merge_base.trim());

    let again = repo.run_stax(&["fix", "--yes"]);
    again.assert_success();
    assert!(
        !TestRepo::stdout(&again).contains("Stale parent boundary"),
        "second run must find nothing to repair"
    );

    // The repaired branch restacks by replaying only its own commit.
    repo.run_stax(&["restack", "--yes", "--quiet"])
        .assert_success();
    let own = TestRepo::stdout(&repo.git(&["rev-list", "--count", "main..feat"]));
    assert_eq!(own.trim(), "1");
}

#[test]
fn validate_flags_stale_boundary() {
    let repo = TestRepo::new();
    externally_rebased_branch(&repo, false);

    let out = repo.run_stax(&["validate"]);
    assert!(
        TestRepo::stdout(&out).contains("stale parent boundary"),
        "got:\n{}",
        TestRepo::stdout(&out)
    );
}
