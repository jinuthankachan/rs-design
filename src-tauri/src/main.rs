//! rs-design — Tauri v2 native shell entry point.
//!
//! Owns the window, the system WebKitGTK webview, and the supervisor that will
//! (CP2) start the `od-server` axum app on a loopback port and (CP3) spawn +
//! health-check the embedded Node daemon sidecar behind the route table. The
//! webview is then pointed at the supervisor's loopback endpoint.
//!
//! This V1 CP0 version is a placeholder shell: it opens a window on the
//! placeholder UI. See TODO.md (CP2, CP3, CP5).

mod env_inject;
mod launcher;
mod supervisor;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .setup(|_app| {
            // TODO(CP2/CP3): start the axum supervisor + spawn the daemon sidecar,
            // then point the main window's webview at the supervisor loopback port.
            tracing::info!("rs-design starting (V1 CP0 placeholder shell)");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running rs-design");
}
