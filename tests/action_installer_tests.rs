use crate::common::OutputAssertions;
use std::process::Command;

fn resolve_artifact(os: &str, arch: &str, version: &str) -> std::process::Output {
    Command::new("bash")
        .arg("scripts/install-action.sh")
        .env("RUNNER_OS", os)
        .env("STAX_INSTALL_ARCH", arch)
        .env("INPUT_VERSION", version)
        .env("STAX_INSTALL_DRY_RUN", "1")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run action installer resolver")
}

#[test]
fn action_installer_maps_every_release_target() {
    for (os, arch, artifact) in [
        ("macOS", "arm64", "stax-aarch64-apple-darwin.tar.gz"),
        ("macOS", "x86_64", "stax-x86_64-apple-darwin.tar.gz"),
        ("Linux", "aarch64", "stax-aarch64-unknown-linux-gnu.tar.gz"),
        ("Linux", "x86_64", "stax-x86_64-unknown-linux-gnu.tar.gz"),
        ("Windows", "x86_64", "stax-x86_64-pc-windows-msvc.zip"),
    ] {
        let output = resolve_artifact(os, arch, "v0.94.0");
        output.assert_success();
        output.assert_stdout_contains(artifact);
        output.assert_stdout_contains("/download/v0.94.0/");
    }
}

#[test]
fn action_installer_resolves_latest_release_url() {
    let output = resolve_artifact("Linux", "x86_64", "latest");
    output.assert_success();
    output.assert_stdout_contains("/releases/latest/download/");
}

#[test]
fn action_installer_rejects_unsupported_platform() {
    let output = resolve_artifact("Plan9", "mips", "latest");
    output.assert_failure();
    output.assert_stderr_contains("Unsupported runner platform");
}
