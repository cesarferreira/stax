//! Binds the Axum HTTP server for `st web`.

use crate::web::routes::build_router;
use crate::web::session::SharedSession;
use anyhow::Result;
use std::net::SocketAddr;
use tokio::task::JoinHandle;

/// Bound server information returned to callers.
pub struct BoundServer {
    pub addr: SocketAddr,
    pub url: String,
    /// Background serve task — abort it to shut the server down (tests).
    pub join_handle: JoinHandle<()>,
}

/// Bind the server on 127.0.0.1:`port` (or an ephemeral port when `port == 0`).
///
/// Returns the bound address and the full workspace URL including the session token.
pub async fn bind(port: u16, session: SharedSession) -> Result<BoundServer> {
    let token = session.lock().unwrap().session_token.clone();
    let router = build_router(session);

    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;

    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        if port != 0 {
            anyhow::anyhow!(
                "Port {port} is already in use — try --port 0 for an ephemeral port: {e}"
            )
        } else {
            anyhow::anyhow!("Failed to bind a local port: {e}")
        }
    })?;
    let bound_addr = listener.local_addr()?;
    let url = format!("http://127.0.0.1:{}/s/{}/", bound_addr.port(), token);

    let join_handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .ok();
    });

    Ok(BoundServer {
        addr: bound_addr,
        url,
        join_handle,
    })
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
