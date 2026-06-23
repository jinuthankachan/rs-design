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
mod route_table;

pub use proxy::{proxy_handler, ProxyState};
pub use route_table::{RouteEntry, RouteTable, Target};

use axum::Router;

/// Build the V1 router: the route table with a single `("/", Proxy)` entry, so
/// every request reverse-proxies to the Node sidecar at `upstream` (scheme +
/// host + port). Convenience wrapper over [`RouteTable::proxy_all`].
///
/// V2 migrations add `Native` entries to a [`RouteTable`] instead of calling
/// this; see [`RouteTable::into_router`].
///
/// **SSE invariant (CP2-Task2):** this router applies **no response
/// compression**, because compression buffers Server-Sent Events and breaks
/// live streaming. If a later task adds a `tower_http::CompressionLayer` for the
/// static `out/` assets, it MUST exclude `text/event-stream` (e.g. via
/// `.compress_when(...)`) so the chat/proxy SSE routes stay unbuffered.
pub fn router(upstream: impl Into<String>) -> Router {
    RouteTable::proxy_all(upstream).into_router()
}
