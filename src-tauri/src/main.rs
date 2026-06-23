//! rs-design — Tauri v2 native shell entry point.
//!
//! Owns the window, the system WebKitGTK webview, and the supervisor that (CP2)
//! starts the `od-server` axum app on an ephemeral loopback port and (CP3)
//! spawns + health-checks the embedded Node daemon sidecar behind the route
//! table.
//!
//! The webview is pointed at the **axum http origin** (not the Tauri asset
//! protocol). This is required by the CP1 api-base spike: the frontend reads its
//! API base as `window.location.origin` and calls relative `/api/*`, so the
//! origin must *be* axum for those calls to reach the route table. axum serves
//! the app and proxies `/api` from one origin; SSE flows straight through.

mod diagnostics;
mod env_inject;
mod launcher;
mod supervisor;

use std::time::Duration;

use supervisor::{DaemonStatus, Supervisor};
use tauri::Manager;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            // Start axum + spawn and health-check the embedded daemon.
            let supervisor = supervisor::start()?;
            let axum_url = supervisor.axum_url.clone();

            // Gate the window on a reachable backend, but never block the dev
            // loop forever: on failure we still open the window (devtools/logs
            // stay available; the proxy will surface 502s) after surfacing the
            // error.
            match tauri::async_runtime::block_on(supervisor.wait_ready(Duration::from_secs(30))) {
                DaemonStatus::Ready => {
                    tracing::info!("embedded daemon ready; catalog reachable via axum");
                    // CP5 acceptance diagnostics: probe /api/agents through the
                    // seam and log which CLIs the env_inject levers surfaced.
                    // Off the critical path (detection spawns per-CLI probes), so
                    // spawn it rather than block the window.
                    let diag_url = axum_url.clone();
                    tauri::async_runtime::spawn(async move {
                        diagnostics::log_acceptance_summary(&diag_url).await;
                    });
                }
                DaemonStatus::Failed(err) => tracing::error!(
                    error = %err,
                    "embedded daemon failed to become ready; opening window anyway"
                ),
                DaemonStatus::Starting => {}
            }

            tracing::info!(url = %axum_url, "pointing webview at od-server");

            // Build the main window on the axum origin (same-origin API/SSE).
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(axum_url.parse()?),
            )
            .title("rs-design")
            .inner_size(1280.0, 800.0)
            .resizable(true)
            .build()?;

            // Hold the handle so window-close can shut axum down gracefully.
            app.manage(supervisor);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if let Some(supervisor) = window.app_handle().try_state::<Supervisor>() {
                    supervisor.shutdown();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running rs-design");
}
