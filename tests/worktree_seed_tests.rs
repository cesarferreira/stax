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

fn write_seed_config_raw(home: &str, contents: &str) {
    let config_dir = PathBuf::from(home).join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.toml"), contents).expect("write config.toml");
}

#[test]
fn wt_create_auto_seeds_detected_node_modules_without_config() {
    let repo = TestRepo::new();
    let home = repo.clean_home();

    repo.create_file("package.json", "{}");
    repo.create_file("package-lock.json", "{}");
    repo.create_file("node_modules/dep.txt", "cached-dep");

    let out = repo.run_stax_with_env(&["wt", "c", "auto-js"], &[("HOME", home.as_str())]);
    out.assert_success();

    let worktree = single_worktree(&repo, &home);
    let seeded = worktree.join("node_modules").join("dep.txt");
    assert!(
        seeded.exists(),
        "expected auto-seeded node_modules/dep.txt in {}",
        worktree.display()
    );
    assert_eq!(fs::read_to_string(&seeded).unwrap(), "cached-dep");
}

#[test]
fn wt_create_does_not_auto_seed_env_files() {
    let repo = TestRepo::new();
    let home = repo.clean_home();

    repo.create_file("package.json", "{}");
    repo.create_file(".env", "SECRET=1");

    let out = repo.run_stax_with_env(&["wt", "c", "no-env"], &[("HOME", home.as_str())]);
    out.assert_success();

    let worktree = single_worktree(&repo, &home);
    assert!(
        !worktree.join(".env").exists(),
        ".env should never be auto-seeded"
    );
}

#[test]
fn wt_create_explicit_seed_paths_override_auto_detection() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    write_seed_config(&home, &[".cache/tool"]);

    repo.create_file("package.json", "{}");
    repo.create_file("node_modules/dep.txt", "cached-dep");
    repo.create_file(".cache/tool/cache.txt", "tool-cache");

    let out = repo.run_stax_with_env(&["wt", "c", "override"], &[("HOME", home.as_str())]);
    out.assert_success();

    let worktree = single_worktree(&repo, &home);
    assert!(
        !worktree.join("node_modules").exists(),
        "explicit seed_paths should replace auto-detected paths"
    );
    assert!(worktree.join(".cache/tool/cache.txt").exists());
}

#[test]
fn wt_create_auto_seed_false_disables_default_detection() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    write_seed_config_raw(&home, "[worktree]\nauto_seed = false\n");

    repo.create_file("package.json", "{}");
    repo.create_file("node_modules/dep.txt", "cached-dep");

    let out = repo.run_stax_with_env(&["wt", "c", "disabled"], &[("HOME", home.as_str())]);
    out.assert_success();

    let worktree = single_worktree(&repo, &home);
    assert!(
        !worktree.join("node_modules").exists(),
        "auto_seed = false should skip default dependency detection"
    );
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
