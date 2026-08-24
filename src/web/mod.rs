//! `st web` — localhost HTMX web workspace for stax.
//!
//! Starts an Axum server on 127.0.0.1, opens the browser, and provides a
//! three-pane workspace (stack / changes / inspector) with HTMX-powered
//! mutations through `execute_repository_operation`.
//!
//! Safety model:
//! - Binds 127.0.0.1 only; no `--host` flag.
//! - Every URL contains an unguessable session token: `/s/<token>/...`.
//! - Every mutating POST requires a matching CSRF token.
//! - Non-local Host/Origin headers are rejected with 403.
//! - One mutation at a time (flag in `WebSession`).

pub mod routes;
pub mod server;
pub mod session;
pub mod static_assets;
pub mod templates;

use crate::application::RepositorySession;
use crate::progress::LiveTimer;
use anyhow::Result;
use colored::Colorize;
use session::{WebSession, generate_token, make_shared};
use std::path::PathBuf;

/// Format a single startup-timeline row with consistent column alignment.
///
/// Produces:  `  {icon} {label:<36}{detail}`
///
/// The label column is right-padded to 36 characters so detail values line up
/// across rows regardless of label length.
fn format_step_row(icon: &str, label: &str, detail: &str) -> String {
    format!("  {} {:<36}{}", icon, label, detail)
}

/// Public entry point: bind the server and return the URL.
///
/// The server runs until Ctrl-C or the process exits.
pub async fn run_server(repo_root: PathBuf, port: u16, no_open: bool) -> Result<()> {
    println!("\n  {}", "stax web".bold());
    println!();

    // Open and validate the repository, then seed the initial branch selection.
    // Explicit failure rows keep startup errors visible; LiveTimer's Drop impl
    // still guarantees cancellation-safe worker cleanup.
    let timer = LiveTimer::new("Opening repository...");

    let root_clone = repo_root.clone();
    let load_result = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
        let sess = RepositorySession::open(&root_clone)?;
        let current_branch = sess.snapshot().ok().map(|snap| snap.current_branch.clone());
        Ok(current_branch)
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn error: {e}"));

    let current_branch = match load_result {
        Ok(Ok(branch)) => {
            timer.finish_timed();
            branch
        }
        Ok(Err(e)) => {
            timer.finish_err("failed");
            return Err(e);
        }
        Err(e) => {
            timer.finish_err("failed");
            return Err(e);
        }
    };

    let session_token = generate_token();
    let csrf_token = generate_token();
    let mut web_session = WebSession::new(repo_root.clone(), session_token, csrf_token);
    web_session.selected_branch = current_branch;

    let shared = make_shared(web_session);
    let bound = server::bind(port, shared).await?;

    if bound.fell_back {
        let detail = format!(":{} → :{}", bound.requested_port, bound.addr.port());
        println!(
            "{}",
            format_step_row("⚠", "Port busy, using free port", &detail).yellow()
        );
    }

    println!(
        "{}",
        format_step_row("✓", "Server started", &format!(":{}", bound.addr.port())).green()
    );

    if no_open {
        println!(
            "{}",
            format_step_row("○", "Browser", "skipped (--no-open)").dimmed()
        );
    } else {
        crate::commands::open::open_url_in_browser(&bound.url);
        println!("{}", format_step_row("→", "Browser", "requested"));
    }

    println!("\n  {}  {}", "Workspace".bold(), bound.url.cyan());
    println!("  {}", "Press Ctrl-C to stop.".dimmed());

    let _ = bound.join_handle.await;
    println!("\n  {} Server stopped.", "✓".green());

    Ok(())
}

/// Test helper: start a server bound to port 0 and return the base URL with token.
///
/// Exported for integration tests in `tests/`. Dropping the handle aborts the server.
/// This function is intentionally output-free.
#[doc(hidden)]
pub struct WebServerHandle {
    pub base_url: String,
    join_handle: tokio::task::JoinHandle<()>,
}

impl Drop for WebServerHandle {
    fn drop(&mut self) {
        self.join_handle.abort();
    }
}

#[doc(hidden)]
pub async fn start_test_server(repo: PathBuf) -> Result<WebServerHandle> {
    let session_token = generate_token();
    let csrf_token = generate_token();

    let mut web_session = WebSession::new(repo.clone(), session_token.clone(), csrf_token.clone());

    let root_clone = repo.clone();
    if let Ok(Ok(snap)) =
        tokio::task::spawn_blocking(move || RepositorySession::open(&root_clone)?.snapshot()).await
    {
        web_session.selected_branch = Some(snap.current_branch.clone());
    }

    let shared = make_shared(web_session);
    let bound = server::bind(0, shared).await?;

    Ok(WebServerHandle {
        base_url: bound.url,
        join_handle: bound.join_handle,
    })
}

#[cfg(test)]
mod tests {
    use super::format_step_row;

    #[test]
    fn startup_step_row_aligns_details() {
        assert_eq!(
            format_step_row("✓", "Starting local server", ":53142"),
            "  ✓ Starting local server               :53142"
        );
    }
}
