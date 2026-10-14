use crate::common::TestRepo;

fn tag_exists_locally(repo: &TestRepo, tag: &str) -> bool {
    repo.git(&["rev-parse", "--verify", &format!("refs/tags/{tag}")])
        .status
        .success()
}

#[test]
fn sync_full_fetches_remote_only_tags_while_default_sync_does_not() {
    let repo = TestRepo::new_with_remote();
    let tag = "remote-only-sync-tag";

    // Publish a tag without leaving its local ref behind. This makes the tag
    // observable only through origin, just like a release published elsewhere.
    repo.git(&["tag", tag, "main"]);
    repo.git(&["push", "origin", &format!("refs/tags/{tag}")]);
    repo.git(&["tag", "--delete", tag]);
    assert!(
        !tag_exists_locally(&repo, tag),
        "test setup must leave the tag on origin only"
    );

    let default_sync = repo.run_stax(&["sync", "--force"]);
    assert!(
        default_sync.status.success(),
        "default sync failed: {}",
        TestRepo::stderr(&default_sync)
    );
    assert!(
        !tag_exists_locally(&repo, tag),
        "default sync must retain its --no-tags fast path"
    );

    let full_sync = repo.run_stax(&["sync", "--full", "--force"]);
    assert!(
        full_sync.status.success(),
        "sync --full failed: {}",
        TestRepo::stderr(&full_sync)
    );
    assert!(
        tag_exists_locally(&repo, tag),
        "sync --full must fetch a tag that exists only on origin"
    );
}
