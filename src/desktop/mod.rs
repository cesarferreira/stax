use std::path::PathBuf;

use anyhow::Result;

pub(crate) mod protocol;

use protocol::{DesktopAction, DesktopError, TerminalEvent};

pub(crate) fn run_snapshot(repo: PathBuf, schema_version: u32, request_id: String) -> Result<()> {
    validate_schema(schema_version, &request_id)?;
    let _ = repo;
    not_implemented(&request_id, "snapshot")
}

pub(crate) fn run_diff(
    repo: PathBuf,
    schema_version: u32,
    request_id: String,
    branch: String,
) -> Result<()> {
    validate_schema(schema_version, &request_id)?;
    let _ = (repo, branch);
    not_implemented(&request_id, "diff")
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
        return fail(request_id, DesktopError::unsupported_schema(schema_version));
    }
    Ok(())
}

fn not_implemented(request_id: &str, operation: &str) -> Result<()> {
    fail(
        request_id,
        DesktopError::operation(
            "not_implemented",
            format!("Desktop {operation} is not implemented."),
            "The desktop engine protocol is still being initialized.",
            "retry",
        ),
    )
}

fn fail(request_id: &str, error: DesktopError) -> Result<()> {
    let code = error.code;
    emit_terminal(&TerminalEvent::<serde_json::Value>::failure(
        request_id, error,
    ))?;
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
