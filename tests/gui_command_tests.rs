use std::path::{Path, PathBuf};
use std::process::Command;

fn gui_command(cwd: &Path) -> Command {
    let mut command = Command::new(crate::common::stax_bin());
    command
        .current_dir(cwd)
        .env_remove("STAX_GUI_OPEN_EXECUTABLE")
        .env_remove("STAX_CONFIG_DIR")
        .env_remove("STAX_GITHUB_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .env("STAX_DISABLE_UPDATE_CHECK", "1");
    command
}

#[cfg(unix)]
fn recording_launcher(root: &Path, exit_code: i32) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let launcher = root.join("record-open");
    let arguments = root.join("arguments");
    std::fs::write(
        &launcher,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexit {}\n",
            arguments.display(),
            exit_code
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&launcher).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&launcher, permissions).unwrap();
    (launcher, arguments)
}

#[test]
fn gui_help_works_outside_a_repository() {
    let temp = tempfile::tempdir().unwrap();
    let output = gui_command(temp.path())
        .args(["gui", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("[PATH]"));
}

#[test]
fn gui_missing_path_fails_before_platform_launch() {
    let temp = tempfile::tempdir().unwrap();
    let output = gui_command(temp.path())
        .args(["gui", "missing path"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing path"));
}

#[cfg(all(target_os = "macos", unix))]
#[test]
fn gui_bypasses_initialization_in_plain_git_repository() {
    let repo = crate::common::TestRepo::new();
    let recorder = tempfile::tempdir().unwrap();
    let (launcher, arguments) = recording_launcher(recorder.path(), 0);
    let refs_before = crate::common::TestRepo::stdout(&repo.git(&["show-ref"]));
    let status_before = crate::common::TestRepo::stdout(&repo.git(&["status", "--porcelain=v1"]));
    let output = gui_command(&repo.path())
        .env("STAX_GUI_OPEN_EXECUTABLE", &launcher)
        .args(["gui", repo.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(arguments)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        vec![
            "-n",
            "-b",
            "dev.stax.Stax",
            "--args",
            repo.path().canonicalize().unwrap().to_str().unwrap(),
        ],
    );
    assert_eq!(
        crate::common::TestRepo::stdout(&repo.git(&["show-ref"])),
        refs_before
    );
    assert_eq!(
        crate::common::TestRepo::stdout(&repo.git(&["status", "--porcelain=v1"])),
        status_before
    );
}

#[cfg(all(target_os = "macos", unix))]
#[test]
fn launcher_error_wins_over_active_rebase_and_leaves_repository_untouched() {
    let repo = crate::common::TestRepo::new();
    std::fs::create_dir_all(repo.path().join(".git/rebase-merge")).unwrap();
    let recorder = tempfile::tempdir().unwrap();
    let (launcher, _arguments) = recording_launcher(recorder.path(), 1);
    let refs_before = crate::common::TestRepo::stdout(&repo.git(&["show-ref"]));
    let output = gui_command(&repo.path())
        .env("STAX_GUI_OPEN_EXECUTABLE", &launcher)
        .args(["gui", repo.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("make install-gui-app"));
    assert!(stderr.contains("unsigned developer preview"));
    assert!(!stderr.contains("rebase"));
    assert!(!stderr.contains("st init"));
    assert!(repo.path().join(".git/rebase-merge").is_dir());
    assert_eq!(
        crate::common::TestRepo::stdout(&repo.git(&["show-ref"])),
        refs_before
    );
}

#[cfg(all(target_os = "macos", unix))]
#[test]
fn gui_missing_app_result_is_actionable() {
    let repo = crate::common::TestRepo::new();
    let recorder = tempfile::tempdir().unwrap();
    let (launcher, _arguments) = recording_launcher(recorder.path(), 1);
    let output = gui_command(&repo.path())
        .env("STAX_GUI_OPEN_EXECUTABLE", &launcher)
        .args(["gui", repo.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("make install-gui-app"));
    assert!(stderr.contains("$HOME/Applications/Stax.app"));
    assert!(stderr.contains("unsigned developer preview"));
}

#[cfg(target_os = "macos")]
#[test]
fn gui_spawn_error_wins_over_repository_state() {
    let repo = crate::common::TestRepo::new();
    std::fs::create_dir_all(repo.path().join(".git/rebase-merge")).unwrap();
    let missing_launcher = tempfile::tempdir()
        .unwrap()
        .path()
        .join("missing-open")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("/tmp/stax-missing-open"));
    let output = gui_command(&repo.path())
        .env("STAX_GUI_OPEN_EXECUTABLE", &missing_launcher)
        .args(["gui", repo.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("make install-gui-app"));
    assert!(stderr.contains("failed to spawn"));
    assert!(!stderr.contains("rebase"));
    assert!(!stderr.contains("st init"));
    assert!(repo.path().join(".git/rebase-merge").is_dir());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn gui_unsupported_platform_never_runs_override() {
    let temp = tempfile::tempdir().unwrap();
    let recorder = tempfile::tempdir().unwrap();
    let launcher = recorder.path().join("record-open");
    let output = gui_command(temp.path())
        .env("STAX_GUI_OPEN_EXECUTABLE", &launcher)
        .args(["gui", temp.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("only supported on macOS"));
    assert!(!launcher.exists());
}
