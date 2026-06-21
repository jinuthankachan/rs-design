//! DaemonLauncher — how the Node daemon sidecar is started.
//!
//! - `DevNodeLauncher` (CP3): runs the daemon from `vendor/open-design` with the
//!   system Node, for the dev loop.
//! - `BundledLauncher` (CP6): runs the esbuild'd daemon with a bundled Node 24
//!   runtime resolved from Tauri resources.
//!
//! The trait boundary makes the eventual V2 "delete the sidecar" step a matter
//! of removing one implementation, not a rewrite.
//
// TODO(CP3/CP6): DaemonLauncher trait + Dev/Bundled implementations.
