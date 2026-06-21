//! Supervisor — owns the embedded backend lifecycle.
//!
//! - CP2: start the `od-server` axum app on an ephemeral loopback port (the
//!   webview-facing endpoint) with a route table proxying 100% to the daemon.
//! - CP3: spawn + health-check the Node daemon on a second internal port (wait
//!   for the `[od] listening` stdout line AND poll `GET /api/skills`); restart
//!   policy; kill on exit (no orphan daemon). Ports the known-good logic from
//!   upstream `vendor/open-design/apps/desktop/src/main/runtime.ts`.
//
// TODO(CP2/CP3): axum supervisor + daemon health-check + lifecycle.
