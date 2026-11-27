use crate::common::TestRepo;

fn has_timed_trace(stderr: &str, command: &str) -> bool {
    stderr.lines().any(|line| {
        line.contains("[trace] git #") && line.contains("ms @") && line.ends_with(command)
    })
}

#[test]
fn sync_trace_reports_fetch_and_remote_probe() {
    let repo = TestRepo::new_with_remote();
    let old_trunk = TestRepo::stdout(&repo.git(&["rev-parse", "main"]));
    repo.simulate_remote_commit("remote-update.txt", "updated", "Advance remote trunk");
    let remote = TestRepo::stdout(&repo.git(&["ls-remote", "origin", "refs/heads/main"]));
    let remote_oid = remote.split_whitespace().next().expect("remote main OID");
    assert_ne!(old_trunk.trim(), remote_oid, "remote must advance for test");

    let output = repo.run_stax(&["--trace", "rs", "--force"]);
    assert!(
        output.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&output)
    );

    let stderr = TestRepo::stderr(&output);
    for command in [
        "git fetch --no-tags origin main",
        "git ls-remote --heads origin",
    ] {
        assert!(
            has_timed_trace(&stderr, command),
            "missing timed {command} trace:\n{stderr}"
        );
    }

    let local = TestRepo::stdout(&repo.git(&["rev-parse", "main"]));
    assert_eq!(local.trim(), remote_oid, "sync must update trunk");
}

#[test]
fn sync_trace_reports_failed_fetch_and_remote_probe() {
    let repo = TestRepo::new_with_remote();
    let missing_remote = repo.path().join("missing-origin");
    let set_url = repo.git(&[
        "remote",
        "set-url",
        "origin",
        missing_remote.to_str().expect("UTF-8 test path"),
    ]);
    assert!(set_url.status.success(), "failed to set up missing remote");

    let output = repo.run_stax(&["--trace", "sync", "--restack", "--force"]);
    assert!(!output.status.success(), "sync should fail with no remote");
    let stderr = TestRepo::stderr(&output);
    assert!(
        has_timed_trace(&stderr, "git fetch --no-tags origin main"),
        "missing failed fetch trace:\n{stderr}"
    );
    assert!(
        has_timed_trace(&stderr, "git ls-remote --heads origin"),
        "missing failed probe trace:\n{stderr}"
    );
    assert!(
        stderr.contains("git ls-remote --heads failed")
            || stderr.contains("Cannot restack because fetching origin did not succeed"),
        "missing useful fetch/probe failure:\n{stderr}"
    );
}

#[test]
fn sync_trace_no_delete_skips_remote_probe() {
    let repo = TestRepo::new_with_remote();
    let output = repo.run_stax(&["--trace", "sync", "--no-delete", "--force"]);
    assert!(
        output.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&output)
    );

    let stderr = TestRepo::stderr(&output);
    assert!(
        has_timed_trace(&stderr, "git fetch --no-tags origin main"),
        "missing fetch trace:\n{stderr}"
    );
    assert!(
        !stderr.contains("git ls-remote --heads origin"),
        "no-delete should skip remote-head probe:\n{stderr}"
    );
}

#[test]
fn sync_trace_full_fetch_uses_prune() {
    let repo = TestRepo::new_with_remote();
    let output = repo.run_stax(&["--trace", "sync", "--full", "--force"]);
    assert!(
        output.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&output)
    );

    let stderr = TestRepo::stderr(&output);
    assert!(
        has_timed_trace(&stderr, "git fetch --prune origin"),
        "missing full-fetch trace:\n{stderr}"
    );
    assert!(
        !stderr.contains("git ls-remote --heads origin"),
        "full fetch should skip remote-head probe:\n{stderr}"
    );
}

#[test]
fn sync_trace_dry_run_only_probes_remote() {
    let repo = TestRepo::new_with_remote();
    let fetch_head_path = repo.path().join(".git/FETCH_HEAD");
    let fetch_head_before = std::fs::read(&fetch_head_path).ok();
    let tracking_before = TestRepo::stdout(&repo.git(&["rev-parse", "origin/main"]));
    repo.simulate_remote_commit("remote-only.txt", "updated", "Advance remote trunk");

    let output = repo.run_stax(&["--trace", "sync", "--dry-run"]);
    assert!(
        output.status.success(),
        "dry-run failed: {}",
        TestRepo::stderr(&output)
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        has_timed_trace(&stderr, "git ls-remote --heads origin"),
        "missing probe trace:\n{stderr}"
    );
    assert!(!stderr.contains("git fetch"), "dry-run fetched:\n{stderr}");
    assert_eq!(
        std::fs::read(&fetch_head_path).ok(),
        fetch_head_before,
        "dry-run must not write FETCH_HEAD"
    );
    assert_eq!(
        TestRepo::stdout(&repo.git(&["rev-parse", "origin/main"])),
        tracking_before,
        "dry-run must not update remote-tracking refs"
    );
}

#[test]
fn sync_trace_keeps_json_stdout_valid() {
    let repo = TestRepo::new_with_remote();
    let output = repo.run_stax(&["--trace", "sync", "--json", "--force"]);
    assert!(
        output.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("invalid JSON stdout: {error}\n{stdout}"));
    assert_eq!(json["kind"], "sync");
    let stderr = TestRepo::stderr(&output);
    assert!(
        has_timed_trace(&stderr, "git fetch --no-tags origin main"),
        "missing fetch trace:\n{stderr}"
    );
    assert!(
        has_timed_trace(&stderr, "git ls-remote --heads origin"),
        "missing probe trace:\n{stderr}"
    );
}

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

/// Push a tag on a commit that no remote branch reaches, then remove every
/// local trace of it. Only an explicit `--tags` fetch can bring it back;
/// tag auto-following from fetched branches cannot.
fn publish_unreachable_remote_tag(repo: &TestRepo, tag: &str) {
    repo.git(&["checkout", "-q", "-b", "tag-source"]);
    repo.create_file("tag-source.txt", "tagged");
    repo.commit("Commit only reachable from a tag");
    repo.git(&["tag", tag, "HEAD"]);
    repo.git(&["push", "origin", &format!("refs/tags/{tag}")]);
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&["branch", "-D", "tag-source"]);
    repo.git(&["tag", "--delete", tag]);
    assert!(
        !tag_exists_locally(repo, tag),
        "test setup must leave the tag on origin only"
    );
}

#[test]
fn sync_tags_fetches_all_remote_tags() {
    for extra_args in [&[][..], &["--no-delete"][..], &["--full"][..]] {
        let repo = TestRepo::new_with_remote();
        let tag = "unreachable-release-tag";
        publish_unreachable_remote_tag(&repo, tag);

        let mut args = vec!["--trace", "rs", "--tags", "--force"];
        args.extend_from_slice(extra_args);
        let output = repo.run_stax(&args);
        assert!(
            output.status.success(),
            "rs {extra_args:?} --tags failed: {}",
            TestRepo::stderr(&output)
        );
        assert!(
            tag_exists_locally(&repo, tag),
            "rs {extra_args:?} --tags must fetch a tag that only origin has"
        );
        let stderr = TestRepo::stderr(&output);
        assert!(
            stderr.lines().any(|line| line.contains("git fetch")
                && line.contains("--tags")
                && !line.contains("--no-tags")),
            "rs {extra_args:?} --tags must pass --tags to git fetch:\n{stderr}"
        );
    }
}

#[test]
fn sync_full_tags_keeps_local_only_tags() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["tag", "local-only-tag", "main"]);

    let output = repo.run_stax(&["rs", "--full", "--tags", "--force"]);
    assert!(
        output.status.success(),
        "rs --full --tags failed: {}",
        TestRepo::stderr(&output)
    );
    assert!(
        tag_exists_locally(&repo, "local-only-tag"),
        "--prune with --tags must not delete local-only tags"
    );
}

#[test]
fn sync_dry_run_warns_that_tags_is_ignored() {
    let repo = TestRepo::new_with_remote();
    let output = repo.run_stax(&["rs", "--dry-run", "--tags"]);
    assert!(
        output.status.success(),
        "rs --dry-run --tags failed: {}",
        TestRepo::stderr(&output)
    );
    assert!(
        TestRepo::stderr(&output).contains("--tags is ignored by --dry-run"),
        "missing --tags dry-run warning:\n{}",
        TestRepo::stderr(&output)
    );
}

#[test]
fn sync_tags_keeps_diverged_local_tag_and_still_restacks() {
    let repo = TestRepo::new_with_remote();
    let tag = "diverged-tag";
    publish_unreachable_remote_tag(&repo, tag);
    repo.git(&["tag", tag, "main"]);
    let local_tag = TestRepo::stdout(&repo.git(&["rev-parse", tag]));

    let output = repo.run_stax(&["rs", "--tags", "--restack", "--force"]);
    let stderr = TestRepo::stderr(&output);
    assert!(
        output.status.success(),
        "rs --tags --restack must not stop on a tag conflict:\n{stderr}"
    );
    assert!(
        stderr.contains("kept local tag(s)") && stderr.contains(tag),
        "missing warning naming the conflicting tag:\n{stderr}"
    );
    assert_eq!(
        TestRepo::stdout(&repo.git(&["rev-parse", tag])),
        local_tag,
        "sync must not overwrite the local tag"
    );
}
