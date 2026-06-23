//! Supervisor — owns the embedded backend lifecycle.
//!
//! - **CP2 (this file):** pick two ephemeral loopback ports, start the
//!   `od-server` axum app on one (the webview-facing endpoint) with a route
//!   table proxying 100% to the daemon, and *reserve* the second port for the
//!   daemon. The webview is pointed at the axum URL. Graceful shutdown is wired
//!   to window close.
//! - **CP3:** spawn + health-check the Node daemon on the reserved port (wait
//!   for the `[od] listening` stdout line AND poll `GET /api/skills`); restart
//!   policy; kill on exit. Ports the known-good logic from upstream
//!   `vendor/open-design/apps/desktop/src/main/runtime.ts`.
//!
//! ## Two ephemeral loopback ports
//!
//! Both ports are OS-assigned (`bind(127.0.0.1:0)`) so two instances never
//! collide and nothing leaks off-host. The axum listener is bound *synchronously*
//! up front so the port is known the instant we build the webview window
//! (connections queue in the listen backlog until the async `serve` accepts
//! them). The daemon port is reserved by binding then immediately dropping the
//! listener — CP3's spawn binds it for real.

use std::net::{Ipv4Addr, TcpListener as StdTcpListener};
use std::sync::Arc;

use tokio::sync::Notify;

/// Handle to the running backend. Held in Tauri state so the window-close
/// handler can trigger graceful shutdown.
pub struct Supervisor {
    /// Webview-facing axum origin, e.g. `http://127.0.0.1:54321`.
    pub axum_url: String,
    /// Port the embedded daemon will bind in CP3 (reserved now; unused until
    /// the daemon spawn lands).
    #[allow(dead_code)]
    pub daemon_port: u16,
    shutdown: Arc<Notify>,
}

impl Supervisor {
    /// Signal the axum server to shut down gracefully (idempotent).
    pub fn shutdown(&self) {
        tracing::info!("supervisor: graceful shutdown requested");
        self.shutdown.notify_waiters();
    }
}

/// Start the axum server on an ephemeral loopback port, proxying to the reserved
/// daemon port, and return a [`Supervisor`] handle. Does not block.
pub fn start() -> std::io::Result<Supervisor> {
    // Reserve the daemon's future port (CP3 spawns the daemon here).
    let daemon_port = reserve_ephemeral_port()?;
    let daemon_url = format!("http://{}:{}", Ipv4Addr::LOCALHOST, daemon_port);

    // Bind the webview-facing listener now so the port is known immediately.
    let std_listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    std_listener.set_nonblocking(true)?;
    let axum_port = std_listener.local_addr()?.port();
    let axum_url = format!("http://{}:{}", Ipv4Addr::LOCALHOST, axum_port);

    // V1 route table: everything proxies to the daemon (every prefix `Proxy`).
    let router = od_server::router(daemon_url.clone());

    let shutdown = Arc::new(Notify::new());
    let shutdown_signal = shutdown.clone();
    let serve_url = axum_url.clone();

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(listener) => listener,
            Err(err) => {
                tracing::error!(error = %err, "od-server: failed to adopt listener");
                return;
            }
        };
        tracing::info!(axum = %serve_url, daemon = %daemon_url, "od-server listening (CP2: proxy → daemon)");
        let server = axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown_signal.notified().await });
        if let Err(err) = server.await {
            tracing::error!(error = %err, "od-server exited with error");
        } else {
            tracing::info!("od-server stopped");
        }
    });

    Ok(Supervisor {
        axum_url,
        daemon_port,
        shutdown,
    })
}

/// Bind an ephemeral loopback port, read its number, and release it. The number
/// is reused by the daemon spawn in CP3. (A brief TOCTOU window is acceptable on
/// loopback for dev; CP3 owns robust daemon binding.)
fn reserve_ephemeral_port() -> std::io::Result<u16> {
    let listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}
