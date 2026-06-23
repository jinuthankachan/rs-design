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

mod proxy;

pub use proxy::{proxy_handler, ProxyState};

use axum::Router;

/// Build the V1 router: a single catch-all fallback that reverse-proxies every
/// request to the Node sidecar at `upstream` (scheme + host + port).
///
/// CP2-Task3 replaces this with a real `Proxy | Native` route table; until then
/// the fallback *is* the route table (every prefix proxies).
pub fn router(upstream: impl Into<String>) -> Router {
    Router::new()
        .fallback(proxy_handler)
        .with_state(ProxyState::new(upstream))
}
