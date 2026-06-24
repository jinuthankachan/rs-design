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

mod catalog;
mod proxy;
mod route_table;
mod security;

pub use proxy::{proxy_handler, ProxyState};
pub use route_table::{RouteEntry, RouteTable, Target};
pub use security::set_security_headers;

use axum::Router;
use od_catalog::CatalogRoots;

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
    with_security_headers(RouteTable::proxy_all(upstream).into_router())
}

/// Attach the authoritative CSP (+ hardening) response headers to HTML responses
/// (CP6). Applied by the public router builders so every webview-facing surface
/// — proxied app shell HTML included — carries the policy, while SSE/JSON/asset
/// responses pass through untouched. See [`security`].
fn with_security_headers(router: Router) -> Router {
    router.layer(axum::middleware::map_response(
        security::set_security_headers,
    ))
}

/// Build the CP4 router: the three `od-catalog` catalog reads served natively
/// (`/api/skills`, `/api/design-systems`, `/api/design-templates`), with every
/// other path — and every non-`GET` method on those paths — proxied to the Node
/// sidecar at `upstream`.
///
/// `force_proxy` is the contract's rollback lever: when `true`, the native
/// catalog routes are *not* registered and the whole surface reverts to the
/// daemon (`--force-proxy`), without changing the table's contents.
///
/// The same **no-response-compression SSE invariant** documented on [`router`]
/// applies here — the catalog responses are small JSON, but the proxied chat/SSE
/// routes still flow through this router uncompressed.
pub fn router_with_catalog(
    upstream: impl Into<String>,
    roots: CatalogRoots,
    force_proxy: bool,
) -> Router {
    let upstream = upstream.into();
    let mut table = RouteTable::new();
    for (path, handler) in catalog::catalog_routes(roots, upstream.clone()) {
        table = table.native_exact(path, handler);
    }
    with_security_headers(
        table
            .proxy("/", upstream)
            .force_proxy(force_proxy)
            .into_router(),
    )
}
