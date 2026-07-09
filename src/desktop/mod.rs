use std::path::PathBuf;

use anyhow::Result;

mod diff;
pub(crate) mod protocol;
mod snapshot;

use protocol::{DesktopAction, DesktopError, DiffSnapshot, RepositorySnapshot, TerminalEvent};

const NATIVE_SDK_TRANSPORT_BYTES: usize = 512 * 1024;

pub(crate) fn run_snapshot(repo: PathBuf, schema_version: u32, request_id: String) -> Result<()> {
    validate_schema(schema_version, &request_id)?;
    match snapshot::build(&repo) {
        Ok(snapshot) => emit_terminal(&TerminalEvent::success(&request_id, snapshot)),
        Err(error) => fail::<RepositorySnapshot>(&request_id, error),
    }
}

pub(crate) fn run_diff(
    repo: PathBuf,
    schema_version: u32,
    request_id: String,
    branch: String,
) -> Result<()> {
    validate_schema(schema_version, &request_id)?;
    match diff::build(&repo, &branch) {
        Ok(snapshot) => {
            let event = TerminalEvent::success(&request_id, snapshot);
            emit_terminal_bounded(&event, &request_id)
        }
        Err(error) => fail::<DiffSnapshot>(&request_id, error),
    }
}

pub(crate) fn run_action(
    repo: PathBuf,
    schema_version: u32,
    request_id: String,
    action: DesktopAction,
    branch: Option<String>,
) -> Result<()> {
    validate_schema(schema_version, &request_id)?;
    let _ = (repo, action, branch);
    not_implemented(&request_id, "action")
}

fn validate_schema(schema_version: u32, request_id: &str) -> Result<()> {
    if schema_version != protocol::SCHEMA_VERSION {
        return fail::<serde_json::Value>(
            request_id,
            DesktopError::unsupported_schema(schema_version),
        );
    }
    Ok(())
}

fn not_implemented(request_id: &str, operation: &str) -> Result<()> {
    fail::<serde_json::Value>(
        request_id,
        DesktopError::operation(
            "not_implemented",
            format!("Desktop {operation} is not implemented."),
            "The desktop engine protocol is still being initialized.",
            "retry",
        ),
    )
}

fn fail<T: serde::Serialize>(request_id: &str, error: DesktopError) -> Result<()> {
    let code = error.code;
    emit_terminal(&TerminalEvent::<T>::failure(request_id, error))?;
    anyhow::bail!("desktop request failed: {code}")
}

fn emit_terminal<T: serde::Serialize>(event: &protocol::TerminalEvent<'_, T>) -> Result<()> {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, event)?;
    writeln!(stdout)?;
    stdout.flush()?;
    Ok(())
}

fn emit_terminal_bounded<T: serde::Serialize>(
    event: &protocol::TerminalEvent<'_, T>,
    request_id: &str,
) -> Result<()> {
    use std::io::Write;

    let mut payload = serde_json::to_vec(event)?;
    payload.push(b'\n');
    if payload.len() > NATIVE_SDK_TRANSPORT_BYTES {
        return fail::<T>(
            request_id,
            DesktopError::operation(
                "bridge_payload_too_large",
                "The desktop response exceeded the bridge transport limit.",
                format!(
                    "Serialized response was {} bytes; the limit is {} bytes.",
                    payload.len(),
                    NATIVE_SDK_TRANSPORT_BYTES
                ),
                "reinstall_app",
            ),
        );
    }

    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&payload)?;
    stdout.flush()?;
    Ok(())
}
