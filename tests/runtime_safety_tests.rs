use crate::common::{OutputAssertions, TestRepo};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary(name: &str) -> PathBuf {
    let path = match name {
        "stax" => env!("CARGO_BIN_EXE_stax"),
        "st" => env!("CARGO_BIN_EXE_st"),
        _ => panic!("unknown test binary: {name}"),
    };
    PathBuf::from(path)
}

fn run_binary(name: &str, cwd: &Path, home: &Path, args: &[&str]) -> Output {
    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    Command::new(binary(name))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("GIT_CONFIG_GLOBAL", null_path)
        .env("GIT_CONFIG_SYSTEM", null_path)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .env_remove("GITHUB_TOKEN")
        .env_remove("STAX_GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .output()
        .expect("run stax binary")
}

#[test]
fn st_alias_matches_stax_version_output() {
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let stax = run_binary("stax", cwd.path(), home.path(), &["--version"]);
    let st = run_binary("st", cwd.path(), home.path(), &["--version"]);

    assert_eq!(st.status.code(), stax.status.code());
    assert_eq!(st.stdout, stax.stdout);
    assert_eq!(st.stderr, stax.stderr);
}

#[test]
fn st_alias_matches_stax_error_output_and_exit_code() {
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let stax = run_binary("stax", cwd.path(), home.path(), &["status", "--json"]);
    let st = run_binary("st", cwd.path(), home.path(), &["status", "--json"]);

    assert_eq!(st.status.code(), stax.status.code());
    assert_eq!(st.stdout, stax.stdout);
    assert_eq!(st.stderr, stax.stderr);
}

fn conflict_exit_code(name: &str) -> i32 {
    let repo = TestRepo::new();
    repo.create_conflict_scenario();
    let home = tempfile::tempdir().expect("home");
    let output = run_binary(
        name,
        &repo.path(),
        home.path(),
        &["restack", "--yes", "--quiet"],
    );
    repo.abort_rebase();
    output.status.code().expect("process exit code")
}

#[test]
fn both_entrypoints_use_the_conflict_exit_code() {
    assert_eq!(conflict_exit_code("stax"), 2);
    assert_eq!(conflict_exit_code("st"), 2);
}

#[test]
fn read_commands_preserve_orphaned_metadata_until_fix() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();
    repo.create_stack(&["orphan-read"]);
    let branch = repo.current_branch();
    repo.run_stax(&["trunk"]).assert_success();
    repo.git(&["branch", "-D", &branch]).assert_success();

    let metadata_ref = format!("refs/branch-metadata/{branch}");
    repo.git(&["show-ref", "--verify", &metadata_ref])
        .assert_success();

    repo.run_stax(&["status", "--json"]).assert_success();
    repo.git(&["show-ref", "--verify", &metadata_ref])
        .assert_success();

    let validate = repo.run_stax(&["validate"]);
    validate.assert_failure();
    assert!(TestRepo::stdout(&validate).contains("orphaned metadata"));
    repo.git(&["show-ref", "--verify", &metadata_ref])
        .assert_success();

    repo.run_stax(&["fix", "--yes"]).assert_success();
    repo.git(&["show-ref", "--verify", &metadata_ref])
        .assert_failure();
}
