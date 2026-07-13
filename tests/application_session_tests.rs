use crate::common::TestRepo;
use stax::application::{BranchSummary, DiffLineKind, RepositorySession};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn cwd_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

struct CurrentDirGuard(PathBuf);

impl CurrentDirGuard {
    fn change_to(path: &Path) -> Self {
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self(original)
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

fn branch(snapshot: &[BranchSummary], name: &str) -> BranchSummary {
    snapshot
        .iter()
        .find(|branch| branch.name == name)
        .unwrap_or_else(|| panic!("missing branch {name}"))
        .clone()
}

fn write_stack_parent(repo: &TestRepo, branch: &str, parent: &str) {
    let metadata_file = format!(".metadata-{branch}.json");
    let metadata = format!(
        r#"{{"parentBranchName":"{parent}","parentBranchRevision":"{}"}}"#,
        repo.get_commit_sha(parent)
    );
    repo.create_file(&metadata_file, &metadata);
    let hash = repo.git(&["hash-object", "-w", &metadata_file]);
    assert!(
        hash.status.success(),
        "hash metadata failed: {}",
        TestRepo::stderr(&hash)
    );
    let oid = TestRepo::stdout(&hash).trim().to_string();
    let refname = format!("refs/branch-metadata/{branch}");
    let update = repo.git(&["update-ref", &refname, &oid]);
    assert!(
        update.status.success(),
        "update metadata failed: {}",
        TestRepo::stderr(&update)
    );
    std::fs::remove_file(repo.path().join(metadata_file)).unwrap();
}

fn diff_cache_entries(repo: &TestRepo) -> Vec<PathBuf> {
    let dir = repo
        .path()
        .join(".git")
        .join("stax")
        .join("diff-cache")
        .join("v1");
    let mut entries = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| {
                    let path = entry.ok()?.path();
                    (path.extension().and_then(|extension| extension.to_str()) == Some("json"))
                        .then_some(path)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

fn only_diff_cache_entry(repo: &TestRepo) -> PathBuf {
    let entries = diff_cache_entries(repo);
    assert_eq!(entries.len(), 1, "expected one persisted diff entry");
    entries.into_iter().next().unwrap()
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
fn snapshot_sorts_forked_siblings_and_assigns_deterministic_columns() {
    let repo = TestRepo::new();
    repo.create_stack(&["zeta"]);
    let checkout = repo.run_stax(&["checkout", "main"]);
    assert!(
        checkout.status.success(),
        "checkout main failed: {}",
        TestRepo::stderr(&checkout)
    );
    repo.create_stack(&["alpha"]);

    let snapshot = RepositorySession::open(repo.path())
        .unwrap()
        .snapshot()
        .unwrap();

    assert_eq!(
        snapshot
            .branches
            .iter()
            .map(|branch| (branch.name.as_str(), branch.column))
            .collect::<Vec<_>>(),
        vec![("alpha", 0), ("zeta", 1), ("main", 0)]
    );
}

#[test]
fn snapshot_rejects_unreachable_parent_cycles() {
    let repo = TestRepo::new();
    repo.create_stack(&["first", "second"]);
    write_stack_parent(&repo, "first", "second");
    write_stack_parent(&repo, "second", "first");
    let session = RepositorySession::open(repo.path()).unwrap();

    let error = session.snapshot().unwrap_err().to_string();

    assert!(error.contains("Invalid stack topology"));
    assert!(error.contains("first"));
    assert!(error.contains("second"));
    assert!(error.contains("exactly once"));
}

#[test]
fn opening_a_linked_worktree_uses_its_canonical_root_and_branch() {
    let repo = TestRepo::new();
    let create_branch = repo.git(&["branch", "linked"]);
    assert!(
        create_branch.status.success(),
        "create linked branch failed: {}",
        TestRepo::stderr(&create_branch)
    );
    let linked_parent = tempfile::tempdir().unwrap();
    let linked_root = linked_parent.path().join("linked-worktree");
    let linked_root_text = linked_root.to_string_lossy().into_owned();
    let add_worktree = repo.git(&["worktree", "add", &linked_root_text, "linked"]);
    assert!(
        add_worktree.status.success(),
        "add linked worktree failed: {}",
        TestRepo::stderr(&add_worktree)
    );

    let session = RepositorySession::open(&linked_root).unwrap();
    let snapshot = session.snapshot().unwrap();

    assert_eq!(
        session.repository_root(),
        std::fs::canonicalize(&linked_root).unwrap()
    );
    assert_eq!(snapshot.repository_root, session.repository_root());
    assert_eq!(snapshot.current_branch, "linked");
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
fn branch_details_use_configured_remote_outside_process_cwd() {
    let _cwd_lock = cwd_lock();
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    std::fs::write(
        repo.path().join("stax.toml"),
        "[remote]\nname = \"upstream\"\n",
    )
    .unwrap();
    let add_remote = repo.git(&["remote", "add", "upstream", repo.path().to_str().unwrap()]);
    assert!(
        add_remote.status.success(),
        "add upstream failed: {}",
        TestRepo::stderr(&add_remote)
    );
    let update_ref = repo.git(&["update-ref", "refs/remotes/upstream/feature", "main"]);
    assert!(
        update_ref.status.success(),
        "create upstream tracking ref failed: {}",
        TestRepo::stderr(&update_ref)
    );
    let session = RepositorySession::open(repo.path()).unwrap();
    let snapshot = session.snapshot().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let _cwd = CurrentDirGuard::change_to(elsewhere.path());

    let details = session
        .branch_details(&branch(&snapshot.branches, "feature"))
        .unwrap();

    assert!(details.has_remote);
    assert_eq!(details.unpushed, 1);
    assert_eq!(details.unpulled, 0);
}

#[test]
fn branch_details_do_not_fall_back_to_origin_for_unknown_configured_remote() {
    let _cwd_lock = cwd_lock();
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    std::fs::write(
        repo.path().join("stax.toml"),
        "[remote]\nname = \"missing\"\n",
    )
    .unwrap();
    let origin_ref = repo.git(&["update-ref", "refs/remotes/origin/feature", "main"]);
    assert!(
        origin_ref.status.success(),
        "create origin tracking ref failed: {}",
        TestRepo::stderr(&origin_ref)
    );
    let session = RepositorySession::open(repo.path()).unwrap();
    let snapshot = session.snapshot().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let _cwd = CurrentDirGuard::change_to(elsewhere.path());

    let details = session
        .branch_details(&branch(&snapshot.branches, "feature"))
        .unwrap();

    assert!(!details.has_remote);
    assert_eq!(details.unpushed, 0);
    assert_eq!(details.unpulled, 0);
}

#[test]
fn branch_details_report_repository_config_errors() {
    let _cwd_lock = cwd_lock();
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    std::fs::write(repo.path().join("stax.toml"), "[remote\nname = broken\n").unwrap();
    let session = RepositorySession::open(repo.path()).unwrap();
    let snapshot = session.snapshot().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let _cwd = CurrentDirGuard::change_to(elsewhere.path());

    let error = session
        .branch_details(&branch(&snapshot.branches, "feature"))
        .unwrap_err();
    let message = format!("{error:#}");

    assert!(message.contains("Failed to load stax config"));
    assert!(message.contains(&repo.path().display().to_string()));
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

    assert!(only_diff_cache_entry(&repo).is_file());

    let second = RepositorySession::open(repo.path())
        .unwrap()
        .diff("feature", "main")
        .unwrap();

    assert_eq!(second, first);
}

#[test]
fn cached_diff_returns_the_same_entry_without_recalculating() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    let session = RepositorySession::open(repo.path()).unwrap();
    let loaded = session.diff("feature", "main").unwrap();

    let cached = RepositorySession::open(repo.path())
        .unwrap()
        .cached_diff("feature", "main")
        .unwrap();

    assert_eq!(cached, Some(loaded));
}

#[test]
fn refresh_diff_bypasses_matching_stale_cache_and_replaces_it() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    let session = RepositorySession::open(repo.path()).unwrap();
    let actual = session.diff("feature", "main").unwrap();
    let cache_path = only_diff_cache_entry(&repo);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache_path).unwrap()).unwrap();
    stored["stat"] = serde_json::json!([]);
    stored["lines"] = serde_json::json!([
        {"content": "deliberately incorrect cached patch", "line_type": "context"}
    ]);
    std::fs::write(&cache_path, serde_json::to_vec_pretty(&stored).unwrap()).unwrap();

    let cached = session.cached_diff("feature", "main").unwrap().unwrap();
    assert_eq!(
        cached.lines[0].content,
        "deliberately incorrect cached patch"
    );

    let refreshed = session.refresh_diff("feature", "main").unwrap();

    assert_eq!(refreshed, actual);
    assert_eq!(
        session.cached_diff("feature", "main").unwrap(),
        Some(actual)
    );
}

#[test]
fn refresh_diff_returns_live_result_and_replaces_malformed_cache() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    let session = RepositorySession::open(repo.path()).unwrap();
    let expected = session.diff("feature", "main").unwrap();
    let cache_path = only_diff_cache_entry(&repo);
    let malformed = b"{";
    std::fs::write(&cache_path, malformed).unwrap();

    let refreshed = session.refresh_diff("feature", "main").unwrap();

    assert_eq!(refreshed, expected);
    assert_eq!(
        session.cached_diff("feature", "main").unwrap(),
        Some(expected)
    );
    assert_ne!(std::fs::read(cache_path).unwrap(), malformed);
}

#[test]
fn cached_diff_miss_does_not_calculate_a_patch() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    let blob_oid = repo.get_commit_sha("feature:feature.txt");
    let object_path = repo
        .path()
        .join(".git")
        .join("objects")
        .join(&blob_oid[..2])
        .join(&blob_oid[2..]);
    std::fs::remove_file(object_path).unwrap();
    let session = RepositorySession::open(repo.path()).unwrap();

    let cached = session.cached_diff("feature", "main").unwrap();

    assert_eq!(cached, None);
    assert!(diff_cache_entries(&repo).is_empty());
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

#[test]
fn diff_failure_after_ref_validation_is_not_cached_as_empty() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    let blob_oid = repo.get_commit_sha("feature:feature.txt");
    let object_path = repo
        .path()
        .join(".git")
        .join("objects")
        .join(&blob_oid[..2])
        .join(&blob_oid[2..]);
    std::fs::remove_file(&object_path).unwrap();
    assert!(diff_cache_entries(&repo).is_empty());
    let session = RepositorySession::open(repo.path()).unwrap();

    let error = session.diff("feature", "main").unwrap_err();
    let message = format!("{error:#}");

    assert!(message.contains("diff stat"));
    assert!(message.contains("feature"));
    assert!(message.contains("main"));
    assert!(message.contains("exit status"));
    assert!(message.contains("fatal:"));
    assert!(diff_cache_entries(&repo).is_empty());
}
