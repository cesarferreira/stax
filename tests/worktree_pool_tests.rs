use crate::common;

use common::{OutputAssertions, TestRepo};
use std::fs;
use std::path::PathBuf;

fn default_worktree_root(repo: &TestRepo, home: &str) -> PathBuf {
    let repo_name = repo
        .path()
        .file_name()
        .expect("repo dir name")
        .to_string_lossy()
        .into_owned();
    PathBuf::from(home)
        .join(".stax")
        .join("worktrees")
        .join(repo_name)
}

fn linked_worktree_dirs(repo: &TestRepo, home: &str) -> Vec<PathBuf> {
    let root = default_worktree_root(repo, home);
    if !root.exists() {
        return Vec::new();
    }
    fs::read_dir(&root)
        .expect("read worktree root")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.is_dir())
        .collect()
}

fn manifest_path(repo: &TestRepo, home: &str) -> PathBuf {
    default_worktree_root(repo, home).join(".stax-pool.json")
}

/// Count idle slots recorded in the pool manifest.
fn idle_slot_count(repo: &TestRepo, home: &str) -> usize {
    let path = manifest_path(repo, home);
    if !path.exists() {
        return 0;
    }
    fs::read_to_string(&path)
        .expect("read pool manifest")
        .matches("\"idle\"")
        .count()
}

fn write_config(home: &str, contents: &str) {
    let config_dir = PathBuf::from(home).join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.toml"), contents).expect("write config.toml");
}

/// Prepare a repo with a committed .gitignore so gitignored deps survive
/// `git clean -fd` inside a recycled slot.
fn repo_with_gitignore() -> (TestRepo, String) {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    repo.create_file(".gitignore", "node_modules/\n");
    repo.commit("add gitignore");
    (repo, home)
}

#[test]
fn adopt_reuses_parked_slot_and_gitignored_deps_survive() {
    let (repo, home) = repo_with_gitignore();

    // Create the first lane and drop a gitignored dependency into it.
    repo.run_stax_with_env(
        &["wt", "c", "lane-a", "--no-verify"],
        &[("HOME", home.as_str())],
    )
    .assert_success();
    let lane_a = default_worktree_root(&repo, &home).join("lane-a");
    fs::create_dir_all(lane_a.join("node_modules")).unwrap();
    fs::write(lane_a.join("node_modules").join("dep.txt"), "cached-dep").unwrap();

    // Removing a clean, merged-equivalent lane parks it (directory kept).
    repo.run_stax_with_env(&["wt", "rm", "lane-a"], &[("HOME", home.as_str())])
        .assert_success();
    assert!(lane_a.exists(), "parked slot directory should be kept");
    assert!(
        manifest_path(&repo, &home).exists(),
        "parking should write a pool manifest"
    );

    // A new lane adopts the parked slot: same directory, deps intact.
    repo.run_stax_with_env(
        &["wt", "c", "lane-b", "--no-verify"],
        &[("HOME", home.as_str())],
    )
    .assert_success();

    let dirs = linked_worktree_dirs(&repo, &home);
    assert_eq!(
        dirs.len(),
        1,
        "adopt should recycle the parked dir, not create a new one: {:?}",
        dirs
    );
    assert_eq!(
        dirs[0], lane_a,
        "adopted worktree must reuse the parked path"
    );
    let seeded = lane_a.join("node_modules").join("dep.txt");
    assert!(
        seeded.exists(),
        "gitignored deps must survive slot recycling"
    );
    assert_eq!(fs::read_to_string(&seeded).unwrap(), "cached-dep");
}

#[test]
fn park_preserves_gitignored_deps_but_discards_tracked_changes() {
    let (repo, home) = repo_with_gitignore();
    // Commit a tracked file so a working change can be introduced in the lane.
    repo.create_file("tracked.txt", "original\n");
    repo.commit("add tracked file");

    repo.run_stax_with_env(
        &["wt", "c", "lane-a", "--no-verify"],
        &[("HOME", home.as_str())],
    )
    .assert_success();
    let lane_a = default_worktree_root(&repo, &home).join("lane-a");

    // Gitignored dep + an untracked file (both survive clean -fd? untracked is
    // removed by clean; gitignored is kept). Assert the gitignored one survives.
    fs::create_dir_all(lane_a.join("node_modules")).unwrap();
    fs::write(lane_a.join("node_modules").join("dep.txt"), "cached-dep").unwrap();

    repo.run_stax_with_env(&["wt", "rm", "lane-a"], &[("HOME", home.as_str())])
        .assert_success();

    assert!(lane_a.exists(), "slot should be parked");
    assert!(
        lane_a.join("node_modules").join("dep.txt").exists(),
        "gitignored dep must be preserved across park"
    );
    // Tracked file is reset to trunk content (parking resets --hard trunk).
    assert_eq!(
        fs::read_to_string(lane_a.join("tracked.txt")).unwrap(),
        "original\n"
    );
}

#[test]
fn force_dirty_removal_never_parks() {
    let (repo, home) = repo_with_gitignore();
    repo.create_file("tracked.txt", "original\n");
    repo.commit("add tracked file");

    repo.run_stax_with_env(
        &["wt", "c", "lane-a", "--no-verify"],
        &[("HOME", home.as_str())],
    )
    .assert_success();
    let lane_a = default_worktree_root(&repo, &home).join("lane-a");

    // Make the lane dirty (tracked working change).
    fs::write(lane_a.join("tracked.txt"), "modified\n").unwrap();

    // --force dirty removal must really remove the directory, never park it.
    repo.run_stax_with_env(
        &["wt", "rm", "lane-a", "--force"],
        &[("HOME", home.as_str())],
    )
    .assert_success();

    assert!(
        !lane_a.exists(),
        "a --force dirty removal must delete the worktree, not park it"
    );
    assert!(
        !manifest_path(&repo, &home).exists()
            || fs::read_to_string(manifest_path(&repo, &home))
                .unwrap()
                .contains("\"slots\": []"),
        "forced removal should not leave a parked slot behind"
    );
}

#[test]
fn reuse_slots_false_uses_cold_create_and_real_remove() {
    let (repo, home) = repo_with_gitignore();
    write_config(&home, "[worktree]\nreuse_slots = false\n");

    repo.run_stax_with_env(
        &["wt", "c", "lane-a", "--no-verify"],
        &[("HOME", home.as_str())],
    )
    .assert_success();
    let lane_a = default_worktree_root(&repo, &home).join("lane-a");

    repo.run_stax_with_env(&["wt", "rm", "lane-a"], &[("HOME", home.as_str())])
        .assert_success();

    assert!(
        !lane_a.exists(),
        "reuse_slots = false must really remove the worktree"
    );
    assert!(
        !manifest_path(&repo, &home).exists(),
        "reuse_slots = false must not create a pool manifest"
    );
}

#[test]
fn max_idle_slots_cap_forces_real_remove_beyond_cap() {
    let (repo, home) = repo_with_gitignore();
    write_config(&home, "[worktree]\nmax_idle_slots = 1\n");

    // Create TWO lanes that exist simultaneously, each with a gitignored dep so
    // they are real recyclable slots.
    repo.run_stax_with_env(
        &["wt", "c", "lane-a", "--no-verify"],
        &[("HOME", home.as_str())],
    )
    .assert_success();
    let lane_a = default_worktree_root(&repo, &home).join("lane-a");
    fs::create_dir_all(lane_a.join("node_modules")).unwrap();
    fs::write(lane_a.join("node_modules").join("dep.txt"), "cached-dep").unwrap();

    repo.run_stax_with_env(
        &["wt", "c", "lane-b", "--no-verify"],
        &[("HOME", home.as_str())],
    )
    .assert_success();
    let lane_b = default_worktree_root(&repo, &home).join("lane-b");
    fs::create_dir_all(lane_b.join("node_modules")).unwrap();
    fs::write(lane_b.join("node_modules").join("dep.txt"), "cached-dep").unwrap();

    // Removing lane-a parks it: idle count becomes 1, exactly at the cap.
    repo.run_stax_with_env(&["wt", "rm", "lane-a"], &[("HOME", home.as_str())])
        .assert_success();
    assert!(lane_a.exists(), "first park should keep the slot on disk");
    assert_eq!(
        idle_slot_count(&repo, &home),
        1,
        "parking lane-a should leave exactly one idle slot at the cap"
    );

    // Removing lane-b hits the cap (already 1 idle slot), so it must be really
    // removed instead of parked.
    repo.run_stax_with_env(&["wt", "rm", "lane-b"], &[("HOME", home.as_str())])
        .assert_success();
    assert!(
        !lane_b.exists(),
        "parking beyond max_idle_slots must fall back to a real remove"
    );
    assert_eq!(
        idle_slot_count(&repo, &home),
        1,
        "idle slot count must stay capped at 1 (only lane-a parked)"
    );
}

#[test]
fn adopt_skips_slot_leased_by_live_pid() {
    let (repo, home) = repo_with_gitignore();

    // Park a slot.
    repo.run_stax_with_env(
        &["wt", "c", "lane-a", "--no-verify"],
        &[("HOME", home.as_str())],
    )
    .assert_success();
    let lane_a = default_worktree_root(&repo, &home).join("lane-a");
    repo.run_stax_with_env(&["wt", "rm", "lane-a"], &[("HOME", home.as_str())])
        .assert_success();

    // Rewrite the manifest so the slot is leased by a live pid (our own pid).
    let manifest = manifest_path(&repo, &home);
    let leased = format!(
        "{{\n  \"slots\": [\n    {{\n      \"path\": {:?},\n      \"state\": \"leased\",\n      \"branch\": null,\n      \"lease_owner_pid\": {},\n      \"last_used\": 1\n    }}\n  ]\n}}\n",
        lane_a.to_string_lossy(),
        std::process::id()
    );
    fs::write(&manifest, leased).unwrap();

    // A new lane cannot adopt the live-leased slot, so it is created cold.
    repo.run_stax_with_env(
        &["wt", "c", "lane-b", "--no-verify"],
        &[("HOME", home.as_str())],
    )
    .assert_success();
    let lane_b = default_worktree_root(&repo, &home).join("lane-b");
    assert!(
        lane_b.exists(),
        "a slot leased by a live pid must not be adopted"
    );
}

#[test]
fn reconcile_hook_runs_on_adopt_and_is_non_fatal() {
    let (repo, home) = repo_with_gitignore();
    let marker = default_worktree_root(&repo, &home).join("reconcile-marker.txt");
    // Reconcile writes a marker, then exits non-zero: a failing hook must not
    // fail create.
    write_config(
        &home,
        &format!(
            "[worktree]\nreconcile = \"touch {} && exit 3\"\n",
            marker.to_string_lossy()
        ),
    );

    // Park a slot to adopt.
    repo.run_stax_with_env(
        &["wt", "c", "lane-a", "--no-verify"],
        &[("HOME", home.as_str())],
    )
    .assert_success();
    repo.run_stax_with_env(&["wt", "rm", "lane-a"], &[("HOME", home.as_str())])
        .assert_success();

    // Adopt the slot: reconcile should run (marker created) and its non-zero
    // exit must not fail the command.
    repo.run_stax_with_env(&["wt", "c", "lane-b"], &[("HOME", home.as_str())])
        .assert_success();
    assert!(
        marker.exists(),
        "reconcile hook should have run on adopt and created its marker"
    );
}

#[test]
fn manifest_consistent_across_create_remove_create_cycles() {
    let (repo, home) = repo_with_gitignore();

    for _ in 0..3 {
        repo.run_stax_with_env(
            &["wt", "c", "cycle", "--no-verify"],
            &[("HOME", home.as_str())],
        )
        .assert_success();
        repo.run_stax_with_env(&["wt", "rm", "cycle"], &[("HOME", home.as_str())])
            .assert_success();
    }

    // After each remove parks the slot and each create adopts it, exactly one
    // idle slot should remain and exactly one worktree directory should exist.
    let dirs = linked_worktree_dirs(&repo, &home);
    assert_eq!(
        dirs.len(),
        1,
        "create/remove cycles should converge on a single recycled slot: {:?}",
        dirs
    );
}
