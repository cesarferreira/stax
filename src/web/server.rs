//! Binds the Axum HTTP server for `st web`.

use crate::web::routes::build_router;
use crate::web::session::SharedSession;
use anyhow::Result;
use std::net::SocketAddr;
use tokio::task::JoinHandle;

/// Bound server information returned to callers.
pub struct BoundServer {
    /// The actual local socket address the server is listening on.
    pub addr: SocketAddr,
    /// The full workspace URL including the session token.
    pub url: String,
    /// The port number originally requested by the caller.
    ///
    /// When `fell_back` is true, `addr.port()` differs from this value.
    pub requested_port: u16,
    /// Whether the server is listening on a different port than requested.
    ///
    /// Set to `true` when the requested port was busy and the OS chose a free
    /// substitute. Always `false` when the caller requested port `0`.
    pub fell_back: bool,
    /// Background serve task — abort it to shut the server down (tests).
    pub join_handle: JoinHandle<()>,
}

/// Bind the server on `127.0.0.1:{port}` (or an ephemeral port when `port == 0`).
///
/// When `port != 0` and the address is already in use, retries with
/// `127.0.0.1:0` so the OS picks a free port and sets `fell_back = true` on
/// the returned `BoundServer`. All other bind failures are returned as errors.
pub async fn bind(port: u16, session: SharedSession) -> Result<BoundServer> {
    let token = session.lock().unwrap().session_token.clone();
    let (listener, fell_back) = bind_listener(port).await?;
    let bound_addr = listener.local_addr()?;
    let allowed_origin = format!("http://127.0.0.1:{}", bound_addr.port());
    let router = build_router(session, allowed_origin);
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
        requested_port: port,
        fell_back,
        join_handle,
    })
}

/// Attempt to bind `127.0.0.1:{port}`.
///
/// Returns `(listener, fell_back)`. Only `AddrInUse` on a non-zero requested
/// port triggers an automatic retry on port `0`; every other error is hard.
async fn bind_listener(port: u16) -> Result<(tokio::net::TcpListener, bool)> {
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => Ok((listener, false)),
        Err(e) if port != 0 && e.kind() == std::io::ErrorKind::AddrInUse => {
            let fallback: SocketAddr = "127.0.0.1:0".parse()?;
            let listener = tokio::net::TcpListener::bind(fallback)
                .await
                .map_err(|fe| {
                    anyhow::anyhow!(
                        "Port {port} was busy and the fallback bind on port 0 also failed: {fe}"
                    )
                })?;
            Ok((listener, true))
        }
        Err(e) => Err(if port != 0 {
            anyhow::anyhow!("Failed to bind port {port}: {e}")
        } else {
            anyhow::anyhow!("Failed to bind a local port: {e}")
        }),
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
