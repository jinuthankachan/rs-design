//! od-server — the axum application and route table.
//!
//! Owns the webview-facing HTTP/SSE surface. The route table maps each path
//! prefix to either `Proxy { upstream }` (forward to the Node sidecar) or
//! `Native { service }` (handled in Rust). In V1 every prefix is `Proxy`; each
//! V2 migration flips one prefix to `Native`. When the last prefix flips, this
//! crate *is* the daemon and the sidecar is deleted.
//!
//! - CP2: catch-all SSE-safe reverse proxy + route-table abstraction.
//! - CP4: first `Native` routes (`od-catalog`).
//
// TODO(CP2): axum app, route table (Proxy|Native), SSE-safe streaming proxy.
