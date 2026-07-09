use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use crate::commands::resolve_pr::resolve_pr_number;
use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;

use super::protocol::{ActionResult, DesktopAction, DesktopError, ProgressEvent, SCHEMA_VERSION};

#[derive(Debug, PartialEq, Eq)]
struct ChildCommand {
    phase: &'static str,
    args: Vec<String>,
}

const MAX_CHILD_STREAM_BYTES: usize = 16 * 1024;

struct ChildOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

struct CapturedStream {
    bytes: Vec<u8>,
    truncated: bool,
}

pub(super) fn run(
    repo_path: &Path,
    action: DesktopAction,
    branch: Option<&str>,
    request_id: &str,
) -> Result<ActionResult, DesktopError> {
    emit_progress(request_id, "validating", "Validating repository state")?;

    let repo = GitRepo::open_from_path(repo_path).map_err(|error| {
        DesktopError::operation(
            "invalid_repository",
            "The selected folder is not a Git repository.",
            error.to_string(),
            "choose_repository",
        )
    })?;
    let stack = Stack::load(&repo).map_err(operation_error)?;
    let current_branch = repo.current_branch().map_err(operation_error)?;
    let target_branch = branch.unwrap_or(&current_branch).to_string();

    if !stack.branches.contains_key(&target_branch) {
        return Err(DesktopError::operation(
            "branch_not_found",
            format!("Branch '{target_branch}' is not part of this stack."),
            "Refresh the repository and choose an available branch.",
            "refresh",
        ));
    }

    if action != DesktopAction::OpenPr {
        let conflicted_files = repo.conflicted_files().map_err(operation_error)?;
        if !conflicted_files.is_empty() {
            return Err(DesktopError::operation(
                "conflict_in_progress",
                "The repository has unresolved conflicts.",
                format!("Conflicted files: {}", conflicted_files.join(", ")),
                "refresh",
            ));
        }
        if repo.rebase_in_progress().map_err(operation_error)? {
            return Err(DesktopError::operation(
                "rebase_in_progress",
                "A rebase is already in progress.",
                "Resolve or abort the rebase before starting another desktop action.",
                "refresh",
            ));
        }
    }

    if matches!(action, DesktopAction::Restack | DesktopAction::SubmitStack)
        && repo.is_dirty().map_err(operation_error)?
    {
        return Err(DesktopError::operation(
            "dirty_repository",
            "The repository has uncommitted changes.",
            "Commit or stash the changes before restacking or submitting.",
            "refresh",
        ));
    }

    if action == DesktopAction::OpenPr {
        return open_pull_request(repo_path, &repo, &stack, &target_branch, request_id);
    }

    for command in command_plan(action, &current_branch, &target_branch) {
        emit_progress(
            request_id,
            command.phase,
            &progress_message(command.phase, &target_branch),
        )?;
        let args = command.args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = run_stax_child(repo_path, &args)?;
        ensure_child_success(&args, output)?;
    }

    Ok(ActionResult {
        action: action_name(action),
        branch: Some(target_branch.clone()),
        summary: match action {
            DesktopAction::Checkout => format!("Checked out {target_branch}."),
            DesktopAction::Restack => format!("Restacked {target_branch}."),
            DesktopAction::SubmitStack => format!("Submitted the stack for {target_branch}."),
            DesktopAction::OpenPr => unreachable!("open-pr returns above"),
        },
    })
}

fn open_pull_request(
    repo_path: &Path,
    repo: &GitRepo,
    stack: &Stack,
    branch: &str,
    request_id: &str,
) -> Result<ActionResult, DesktopError> {
    emit_progress(
        request_id,
        "opening_pr",
        &format!("Opening the pull request for {branch}"),
    )?;
    let config = Config::load().map_err(operation_error)?;
    let pr_number = resolve_pr_number(repo, stack, branch, &config)
        .map_err(operation_error)?
        .ok_or_else(|| {
            no_pull_request(branch, "No pull-request metadata or remote PR was found.")
        })?;
    let remote = RemoteInfo::from_repo(repo, &config)
        .map_err(|error| no_pull_request(branch, error.to_string()))?;
    let url = remote.pr_url(pr_number);

    if std::env::var("STAX_DESKTOP_NO_OPEN").as_deref() != Ok("1") {
        let output = Command::new("/usr/bin/open")
            .arg(&url)
            .current_dir(repo_path)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| {
                DesktopError::operation(
                    "operation_failed",
                    "The pull request could not be opened.",
                    error.to_string(),
                    "retry",
                )
            })?;
        if !output.status.success() {
            return Err(child_failure(
                &["/usr/bin/open", &url],
                ChildOutput {
                    status: output.status,
                    stdout: output.stdout,
                    stderr: output.stderr,
                    stdout_truncated: false,
                    stderr_truncated: false,
                },
            ));
        }
    }

    Ok(ActionResult {
        action: "open_pr",
        branch: Some(branch.to_string()),
        summary: format!("Pull request #{pr_number}: {url}"),
    })
}

fn no_pull_request(branch: &str, details: impl Into<String>) -> DesktopError {
    DesktopError::operation(
        "no_pull_request",
        format!("No pull request was found for '{branch}'."),
        details,
        "refresh",
    )
}

fn command_plan(action: DesktopAction, current: &str, target: &str) -> Vec<ChildCommand> {
    let mut commands = Vec::new();
    if action == DesktopAction::Checkout
        || (current != target
            && matches!(action, DesktopAction::Restack | DesktopAction::SubmitStack))
    {
        commands.push(ChildCommand {
            phase: "checking_out",
            args: vec!["checkout".to_string(), target.to_string()],
        });
    }

    match action {
        DesktopAction::Restack => commands.push(ChildCommand {
            phase: "restacking",
            args: vec!["restack".to_string(), "--quiet".to_string()],
        }),
        DesktopAction::SubmitStack => commands.push(ChildCommand {
            phase: "submitting",
            args: vec![
                "submit".to_string(),
                "--no-prompt".to_string(),
                "--yes".to_string(),
                "--quiet".to_string(),
            ],
        }),
        DesktopAction::Checkout | DesktopAction::OpenPr => {}
    }
    commands
}

fn progress_message(phase: &str, branch: &str) -> String {
    match phase {
        "checking_out" => format!("Checking out {branch}"),
        "restacking" => format!("Restacking {branch}"),
        "submitting" => format!("Submitting the stack for {branch}"),
        _ => format!("Running {phase}"),
    }
}

fn action_name(action: DesktopAction) -> &'static str {
    match action {
        DesktopAction::Checkout => "checkout",
        DesktopAction::Restack => "restack",
        DesktopAction::SubmitStack => "submit_stack",
        DesktopAction::OpenPr => "open_pr",
    }
}

fn emit_progress(request_id: &str, phase: &str, message: &str) -> Result<(), DesktopError> {
    let event = ProgressEvent {
        schema_version: SCHEMA_VERSION,
        request_id,
        event_type: "progress",
        phase,
        message,
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &event).map_err(bridge_error)?;
    writeln!(stdout).map_err(bridge_error)?;
    stdout.flush().map_err(bridge_error)
}

fn run_stax_child(repo_path: &Path, args: &[&str]) -> Result<ChildOutput, DesktopError> {
    let executable = std::env::current_exe().map_err(|error| {
        DesktopError::operation(
            "engine_unavailable",
            "The bundled stax engine could not locate itself.",
            error.to_string(),
            "reinstall_app",
        )
    })?;
    let mut child = Command::new(executable)
        .args(args)
        .current_dir(repo_path)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            DesktopError::operation(
                "operation_failed",
                "The stax operation could not start.",
                error.to_string(),
                "retry",
            )
        })?;
    let stdout = child.stdout.take().ok_or_else(capture_error)?;
    let stderr = child.stderr.take().ok_or_else(capture_error)?;
    let stdout_thread = std::thread::Builder::new()
        .name("stax-desktop-stdout".to_string())
        .spawn(move || read_bounded(stdout))
        .map_err(capture_io_error)?;
    let stderr_thread = std::thread::Builder::new()
        .name("stax-desktop-stderr".to_string())
        .spawn(move || read_bounded(stderr))
        .map_err(capture_io_error)?;
    let status = child.wait().map_err(capture_io_error)?;
    let stdout = stdout_thread
        .join()
        .map_err(|_| capture_error())?
        .map_err(capture_io_error)?;
    let stderr = stderr_thread
        .join()
        .map_err(|_| capture_error())?
        .map_err(capture_io_error)?;
    Ok(ChildOutput {
        status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    })
}

fn ensure_child_success(args: &[&str], output: ChildOutput) -> Result<(), DesktopError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(child_failure(args, output))
    }
}

fn child_failure(args: &[&str], output: ChildOutput) -> DesktopError {
    let stdout_marker = if output.stdout_truncated {
        "\n...[stdout truncated]"
    } else {
        ""
    };
    let stderr_marker = if output.stderr_truncated {
        "\n...[stderr truncated]"
    } else {
        ""
    };
    DesktopError::operation(
        "operation_failed",
        "The stax operation failed.",
        format!(
            "command: st {}\nstatus: {}\nstdout:\n{}{}\nstderr:\n{}{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            stdout_marker,
            String::from_utf8_lossy(&output.stderr),
            stderr_marker,
        ),
        "retry",
    )
}

fn read_bounded(mut reader: impl Read) -> std::io::Result<CapturedStream> {
    let mut bytes = Vec::with_capacity(MAX_CHILD_STREAM_BYTES);
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = MAX_CHILD_STREAM_BYTES.saturating_sub(bytes.len());
        let keep = remaining.min(read);
        bytes.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
    }
    Ok(CapturedStream { bytes, truncated })
}

fn capture_error() -> DesktopError {
    DesktopError::operation(
        "operation_failed",
        "The stax operation output could not be captured.",
        "A bundled engine output stream was unavailable.",
        "retry",
    )
}

fn capture_io_error(error: std::io::Error) -> DesktopError {
    DesktopError::operation(
        "operation_failed",
        "The stax operation output could not be captured.",
        error.to_string(),
        "retry",
    )
}

fn operation_error(error: anyhow::Error) -> DesktopError {
    DesktopError::operation(
        "operation_failed",
        "The stax operation failed.",
        format!("{error:#}"),
        "retry",
    )
}

fn bridge_error(error: impl std::fmt::Display) -> DesktopError {
    DesktopError::operation(
        "bridge_failure",
        "The desktop engine could not write a progress event.",
        error.to_string(),
        "reinstall_app",
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn restack_plan_checks_out_target_then_runs_quiet_restack() {
        let plan = command_plan(DesktopAction::Restack, "main", "feature/demo");
        assert_eq!(
            plan,
            vec![
                ChildCommand {
                    phase: "checking_out",
                    args: vec!["checkout".to_string(), "feature/demo".to_string()],
                },
                ChildCommand {
                    phase: "restacking",
                    args: vec!["restack".to_string(), "--quiet".to_string()],
                },
            ]
        );
    }

    #[test]
    fn child_output_capture_is_bounded_and_marks_truncation() {
        let input = vec![b'x'; MAX_CHILD_STREAM_BYTES * 3];
        let captured = read_bounded(Cursor::new(input)).unwrap();

        assert_eq!(captured.bytes.len(), MAX_CHILD_STREAM_BYTES);
        assert!(captured.truncated);
    }

    #[test]
    fn submit_plan_checks_out_target_then_runs_noninteractive_quiet_submit() {
        let plan = command_plan(DesktopAction::SubmitStack, "main", "feature/demo");
        assert_eq!(
            plan,
            vec![
                ChildCommand {
                    phase: "checking_out",
                    args: vec!["checkout".to_string(), "feature/demo".to_string()],
                },
                ChildCommand {
                    phase: "submitting",
                    args: vec![
                        "submit".to_string(),
                        "--no-prompt".to_string(),
                        "--yes".to_string(),
                        "--quiet".to_string(),
                    ],
                },
            ]
        );
    }
}
