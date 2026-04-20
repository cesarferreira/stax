//! Tests for `stax submit` behaviour when `git fetch` cannot reach the remote.
//!
//! These pin the contract for issue #222: a failed fetch must be surfaced to
//! the user with the underlying error and an actionable `--no-fetch` hint, and
//! must NOT silently fall through to a `--force-with-lease` push against
//! stale remote-tracking refs (which the remote rejects as `(stale info)`).

mod common;

use common::TestRepo;

/// Point `origin` at a parseable-but-unreachable URL so that:
///   1. `RemoteInfo::from_repo` happily parses it (https://host/owner/repo).
///   2. `git fetch origin` deterministically fails with connection refused on
///      127.0.0.1:1 — fast and offline.
fn break_origin_fetch(repo: &TestRepo) {
    let out = repo.git(&[
        "remote",
        "set-url",
        "origin",
        "https://127.0.0.1:1/test-owner/test-repo.git",
    ]);
    assert!(
        out.status.success(),
        "set-url failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The bug: today the fetch error is swallowed and submit prints
/// `"skipped (continuing with local refs)"`, then proceeds to a
/// `--force-with-lease` push against potentially-stale refs.
///
/// After the fix submit must abort with the underlying fetch error and never
/// reach the push step.
#[test]
fn submit_bails_when_git_fetch_fails_and_does_not_attempt_push() {
    let repo = TestRepo::new_with_remote();

    // Build: main -> feat-a (one commit on top of trunk).
    let bc = repo.run_stax(&["bc", "feat-a"]);
    assert!(
        bc.status.success(),
        "bc failed: {}",
        String::from_utf8_lossy(&bc.stderr)
    );
    repo.create_file("a.txt", "a");
    repo.commit("Add a");

    break_origin_fetch(&repo);

    let out = repo.run_stax(&["ss", "--no-pr", "--no-prompt", "--yes"]);

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let combined = format!("{stdout}\n---STDERR---\n{stderr}");

    // 1. Submit must exit non-zero when fetch fails.
    assert!(
        !out.status.success(),
        "submit must abort when fetch fails, but it exited 0.\n{combined}"
    );

    // 2. The user must see the underlying remote error, not just "skipped".
    //    The fetch and ls-remote both run in parallel; whichever the user sees
    //    first must mention the underlying git error and the failing remote.
    assert!(
        (combined.contains("git fetch") || combined.contains("git ls-remote"))
            && combined.contains("failed"),
        "expected surfaced fetch/ls-remote error mentioning 'failed', got:\n{combined}"
    );

    // 3. The misleading legacy label must be gone.
    assert!(
        !combined.contains("skipped (continuing with local refs)"),
        "the misleading `skipped (continuing with local refs)` label must be \
         gone after the fix, got:\n{combined}"
    );

    // 4. The error must point the user at the documented escape hatch.
    assert!(
        combined.contains("--no-fetch"),
        "expected `--no-fetch` hint in the bail message, got:\n{combined}"
    );

    // 5. Smoking gun: pre-fix submit silently falls through to planning and
    //    the push step ("Will force-push N branch", "Pushing branches...",
    //    "Failed to push branch X"). Post-fix submit must bail before any of
    //    those phases run.
    for phase in [
        "Planning PR operations",
        "Will force-push",
        "Pushing branches",
        "Failed to push branch",
    ] {
        assert!(
            !combined.contains(phase),
            "submit must bail before the `{phase}` phase when fetch fails, \
             got:\n{combined}"
        );
    }
}

/// Regression guard: the documented `--no-fetch` escape hatch must keep
/// working when the remote is unreachable (cached refs already exist for
/// trunk because `TestRepo::new_with_remote` pushes main during setup).
#[test]
fn submit_with_no_fetch_does_not_invoke_fetch_when_remote_unreachable() {
    let repo = TestRepo::new_with_remote();

    let bc = repo.run_stax(&["bc", "feat-a"]);
    assert!(bc.status.success());
    repo.create_file("a.txt", "a");
    repo.commit("Add a");

    break_origin_fetch(&repo);

    let out = repo.run_stax(&["ss", "--no-fetch", "--no-pr", "--no-prompt", "--yes"]);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let combined = format!("{stdout}\n---STDERR---\n{stderr}");

    // Either submit explicitly skipped the fetch, or it did not surface a
    // fetch failure (the only scenario we'd reject is the post-fix bail
    // accidentally firing inside the `--no-fetch` branch).
    assert!(
        combined.contains("Skipping fetch") || combined.contains("skipped (--no-fetch)"),
        "expected `--no-fetch` to skip the fetch step, got:\n{combined}"
    );
    assert!(
        !(combined.contains("git fetch") && combined.contains("failed")),
        "submit with --no-fetch must not surface a fetch failure, got:\n{combined}"
    );
}
