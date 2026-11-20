//! `stax open` browser-launch behaviour with a forced launcher.
#![cfg(unix)]

use crate::common::TestRepo;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const REPO_URL: &str = "https://github.com/test-owner/test-repo";
const FRIENDLY: &str = "Couldn't open a browser. Open this link manually: ";

fn repo_with_origin() -> TestRepo {
    let repo = TestRepo::new();
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/test-owner/test-repo.git",
    ]);
    repo
}

fn launcher(dir: &TempDir, body: &str) -> PathBuf {
    let path = dir.path().join("launcher.sh");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write launcher");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod launcher");
    path
}

#[test]
fn open_succeeds_quietly_when_launcher_exits_zero() {
    let repo = repo_with_origin();
    let dir = TempDir::new().unwrap();
    let script = launcher(&dir, "exit 0");
    let output = repo.run_stax_with_env(
        &["open"],
        &[("STAX_BROWSER_COMMAND", script.to_str().unwrap())],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stdout.contains("Opening"), "{stdout}");
    assert!(!stderr.contains("Couldn't open a browser"), "{stderr}");
}

#[test]
fn open_reports_friendly_message_when_launcher_fails() {
    let repo = repo_with_origin();
    let dir = TempDir::new().unwrap();
    let script = launcher(
        &dir,
        "echo 'links2: not found' >&2\n\
         echo \"xdg-open: no method available for opening 'x'\" >&2\n\
         echo stdout-noise\n\
         exit 3",
    );
    let output = repo.run_stax_with_env(
        &["open"],
        &[("STAX_BROWSER_COMMAND", script.to_str().unwrap())],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!("{FRIENDLY}{REPO_URL}")),
        "{stderr}"
    );
    for noise in ["links2", "no method available", "stdout-noise"] {
        assert!(!stdout.contains(noise), "stdout: {stdout}");
        assert!(!stderr.contains(noise), "stderr: {stderr}");
    }
    assert_eq!(
        stderr
            .lines()
            .filter(|l| l.contains("Couldn't open a browser"))
            .count(),
        1,
        "{stderr}"
    );
}

#[test]
fn open_reports_friendly_message_when_launcher_missing() {
    let repo = repo_with_origin();
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("no-such-browser");
    let output = repo.run_stax_with_env(
        &["open"],
        &[("STAX_BROWSER_COMMAND", missing.to_str().unwrap())],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!("{FRIENDLY}{REPO_URL}")),
        "{stderr}"
    );
    assert!(!stderr.contains("No such file"), "{stderr}");
}

#[test]
fn open_treats_long_running_launcher_as_success() {
    let repo = repo_with_origin();
    let dir = TempDir::new().unwrap();
    let script = launcher(&dir, "sleep 10");
    let started = Instant::now();
    let output = repo.run_stax_with_env(
        &["open"],
        &[("STAX_BROWSER_COMMAND", script.to_str().unwrap())],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!stderr.contains("Couldn't open a browser"), "{stderr}");
    assert!(started.elapsed() < Duration::from_secs(5));
}
