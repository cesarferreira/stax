use crate::common::TestRepo;
use stax::application::{BranchSummary, DiffLineKind, RepositorySession};

fn branch(snapshot: &[BranchSummary], name: &str) -> BranchSummary {
    snapshot
        .iter()
        .find(|branch| branch.name == name)
        .unwrap_or_else(|| panic!("missing branch {name}"))
        .clone()
}

#[test]
fn snapshot_orders_tracked_stacks_before_trunk() {
    let repo = TestRepo::new();
    let created = repo.create_stack(&["first", "second"]);
    assert_eq!(created, vec!["first", "second"]);

    let session = RepositorySession::open(repo.path()).unwrap();
    let snapshot = session.snapshot().unwrap();

    assert_eq!(
        session.repository_root(),
        std::fs::canonicalize(repo.path()).unwrap()
    );
    assert_eq!(snapshot.current_branch, "second");
    assert_eq!(snapshot.trunk, "main");
    assert_eq!(
        snapshot
            .branches
            .iter()
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "first", "main"],
    );
    assert_eq!(
        snapshot
            .branches
            .iter()
            .map(|branch| branch.column)
            .collect::<Vec<_>>(),
        vec![0, 0, 0],
    );
    assert!(snapshot.branches[0].is_current);
    assert!(!snapshot.branches[0].is_trunk);
    assert_eq!(snapshot.branches[0].parent.as_deref(), Some("first"));
    assert!(!snapshot.branches[1].is_current);
    assert_eq!(snapshot.branches[1].parent.as_deref(), Some("main"));
    assert!(snapshot.branches[2].is_trunk);
    assert_eq!(snapshot.branches[2].parent, None);
}

#[test]
fn snapshot_includes_a_trunk_only_repository() {
    let repo = TestRepo::new();

    let snapshot = RepositorySession::open(repo.path())
        .unwrap()
        .snapshot()
        .unwrap();

    assert_eq!(snapshot.current_branch, "main");
    assert_eq!(snapshot.trunk, "main");
    assert_eq!(snapshot.branches.len(), 1);
    assert_eq!(snapshot.branches[0].name, "main");
    assert!(snapshot.branches[0].is_current);
    assert!(snapshot.branches[0].is_trunk);
}

#[test]
fn opening_a_non_repository_reports_the_path() {
    let dir = tempfile::tempdir().unwrap();

    let error = RepositorySession::open(dir.path()).unwrap_err().to_string();

    assert!(error.contains(&dir.path().display().to_string()));
    assert!(error.contains("git repository"));
}

#[test]
fn branch_details_report_ahead_count_and_commit_messages() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    repo.create_file("second.txt", "second feature change\n");
    repo.commit("Second feature commit");

    let session = RepositorySession::open(repo.path()).unwrap();
    let snapshot = session.snapshot().unwrap();
    let details = session
        .branch_details(&branch(&snapshot.branches, "feature"))
        .unwrap();

    assert_eq!(details.ahead, 2);
    assert_eq!(details.behind, 0);
    assert!(!details.has_remote);
    assert_eq!(details.unpushed, 0);
    assert_eq!(details.unpulled, 0);
    assert_eq!(
        details.commits,
        vec!["Second feature commit", "Commit for feature"]
    );
}

#[test]
fn branch_details_limit_commit_messages_to_ten() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    for revision in 1..12 {
        repo.create_file("feature.txt", &format!("revision {revision}\n"));
        repo.commit(&format!("Feature revision {revision}"));
    }

    let session = RepositorySession::open(repo.path()).unwrap();
    let snapshot = session.snapshot().unwrap();
    let details = session
        .branch_details(&branch(&snapshot.branches, "feature"))
        .unwrap();

    assert_eq!(details.ahead, 12);
    assert_eq!(details.commits.len(), 10);
    assert_eq!(
        details.commits.first().map(String::as_str),
        Some("Feature revision 11")
    );
}

#[test]
fn branch_details_for_trunk_have_no_parent_commits() {
    let repo = TestRepo::new();
    let session = RepositorySession::open(repo.path()).unwrap();
    let snapshot = session.snapshot().unwrap();

    let details = session
        .branch_details(&branch(&snapshot.branches, "main"))
        .unwrap();

    assert_eq!(details.ahead, 0);
    assert_eq!(details.behind, 0);
    assert!(details.commits.is_empty());
}

#[test]
fn diff_returns_typed_stat_and_addition_lines() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    let session = RepositorySession::open(repo.path()).unwrap();

    let diff = session.diff("feature", "main").unwrap();

    assert_eq!(diff.stat.len(), 1);
    assert_eq!(diff.stat[0].file, "feature.txt");
    assert_eq!(diff.stat[0].additions, 1);
    assert_eq!(diff.stat[0].deletions, 0);
    assert!(diff.lines.iter().any(|line| {
        line.kind == DiffLineKind::Addition && line.content == "+content for feature"
    }));
}

#[test]
fn second_diff_round_trips_through_the_tui_cache() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    let first = RepositorySession::open(repo.path())
        .unwrap()
        .diff("feature", "main")
        .unwrap();

    let cache_path = repo
        .path()
        .join(".git")
        .join("stax")
        .join("tui-diff-cache.json");
    assert!(cache_path.is_file());

    let second = RepositorySession::open(repo.path())
        .unwrap()
        .diff("feature", "main")
        .unwrap();

    assert_eq!(second, first);
}

#[test]
fn diff_reports_bad_branch_and_parent_names() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    let session = RepositorySession::open(repo.path()).unwrap();

    for (branch, parent, missing) in [
        ("missing-branch", "main", "missing-branch"),
        ("feature", "missing-parent", "missing-parent"),
    ] {
        let error = session.diff(branch, parent).unwrap_err().to_string();
        assert!(error.contains("diff"));
        assert!(error.contains(branch));
        assert!(error.contains(parent));
        assert!(error.contains(missing));
    }
}
