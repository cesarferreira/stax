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

fn single_worktree(repo: &TestRepo, home: &str) -> PathBuf {
    let root = default_worktree_root(repo, home);
    let mut dirs: Vec<PathBuf> = fs::read_dir(&root)
        .expect("read worktree root")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.is_dir())
        .collect();
    assert_eq!(
        dirs.len(),
        1,
        "expected exactly one linked worktree, got {:?}",
        dirs
    );
    dirs.remove(0)
}

fn write_seed_config(home: &str, seed_paths: &[&str]) {
    let config_dir = PathBuf::from(home).join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("create config dir");
    let joined = seed_paths
        .iter()
        .map(|p| format!("\"{}\"", p))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        config_dir.join("config.toml"),
        format!("[worktree]\nseed_paths = [{}]\n", joined),
    )
    .expect("write config.toml");
}

#[test]
fn wt_create_seeds_configured_dependency_paths() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    write_seed_config(&home, &["node_modules"]);

    // A gitignored dependency dir that only exists in the main checkout; it is
    // never materialized by `git worktree add`, so seeding is what carries it over.
    repo.create_file("node_modules/dep.txt", "cached-dep");

    let out = repo.run_stax_with_env(&["wt", "c", "seedy"], &[("HOME", home.as_str())]);
    out.assert_success();

    let worktree = single_worktree(&repo, &home);
    let seeded = worktree.join("node_modules").join("dep.txt");
    assert!(
        seeded.exists(),
        "expected seeded node_modules/dep.txt in {}",
        worktree.display()
    );
    assert_eq!(fs::read_to_string(&seeded).unwrap(), "cached-dep");
}

#[test]
fn wt_create_seeds_multiple_paths_and_skips_missing_sources() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    write_seed_config(&home, &["node_modules", ".venv", "missing-dir"]);

    repo.create_file("node_modules/a.txt", "a");
    repo.create_file(".venv/b.txt", "b");
    // `missing-dir` intentionally does not exist in the main checkout.

    let out = repo.run_stax_with_env(&["wt", "c", "multi"], &[("HOME", home.as_str())]);
    out.assert_success();

    let worktree = single_worktree(&repo, &home);
    assert!(worktree.join("node_modules").join("a.txt").exists());
    assert!(worktree.join(".venv").join("b.txt").exists());
    assert!(
        !worktree.join("missing-dir").exists(),
        "missing source should not create a destination"
    );
}

#[test]
fn wt_create_rejects_seed_paths_with_parent_traversal() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    write_seed_config(&home, &["../escape"]);

    let out = repo.run_stax_with_env(
        &["wt", "c", "bad", "--no-verify"],
        &[("HOME", home.as_str())],
    );
    // `--no-verify` skips seeding entirely, so the bad path is only rejected on a
    // real create. Run again without it to trigger validation.
    out.assert_success();

    let out = repo.run_stax_with_env(&["wt", "c", "bad2"], &[("HOME", home.as_str())]);
    out.assert_failure();
    out.assert_stderr_contains("must not contain '..'");
}

#[test]
fn wt_create_no_verify_skips_seeding() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    write_seed_config(&home, &["node_modules"]);
    repo.create_file("node_modules/dep.txt", "cached-dep");

    let out = repo.run_stax_with_env(
        &["wt", "c", "skipped", "--no-verify"],
        &[("HOME", home.as_str())],
    );
    out.assert_success();

    let worktree = single_worktree(&repo, &home);
    assert!(
        !worktree.join("node_modules").exists(),
        "--no-verify should skip dependency seeding"
    );
}
