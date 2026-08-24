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
use anyhow::Result;
use session::{WebSession, generate_token, make_shared};
use std::path::PathBuf;

/// Public entry point: bind the server and return the URL.
///
/// The server runs until Ctrl-C or the process exits.
pub async fn run_server(repo_root: PathBuf, port: u16, no_open: bool) -> Result<()> {
    let session_token = generate_token();
    let csrf_token = generate_token();

    // Validate the repository before starting the server.
    let root_clone = repo_root.clone();
    tokio::task::spawn_blocking(move || RepositorySession::open(&root_clone))
        .await
        .map_err(|e| anyhow::anyhow!("spawn error: {e}"))??;

    let mut web_session = WebSession::new(repo_root.clone(), session_token, csrf_token);

    // Seed the selection with the current branch.
    {
        let root_clone = repo_root.clone();
        if let Ok(Ok(snap)) =
            tokio::task::spawn_blocking(move || RepositorySession::open(&root_clone)?.snapshot())
                .await
        {
            web_session.selected_branch = Some(snap.current_branch.clone());
        }
    }

    let shared = make_shared(web_session);
    let bound = server::bind(port, shared).await?;

    println!("stax web workspace running at:\n  {}", bound.url);

    if !no_open {
        crate::commands::open::open_url_in_browser(&bound.url);
    }

    // The serve task shuts down on Ctrl-C via its own graceful shutdown.
    let _ = bound.join_handle.await;
    println!("\nServer stopped.");

    Ok(())
}

/// Test helper: start a server bound to port 0 and return the base URL with token.
///
/// Exported for integration tests in `tests/`. Dropping the handle aborts the server.
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
