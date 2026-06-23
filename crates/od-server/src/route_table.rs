//! The `Proxy | Native` route table and its dispatcher (CP2-Task3).
//!
//! Each path prefix is owned by exactly one [`Target`]:
//!
//! - [`Target::Proxy`] — forward to the Node sidecar (V1 default for everything).
//! - [`Target::Native`] — handled in Rust by an [`axum::Router`].
//!
//! V1 has a single entry — `("/", Proxy)` — so every request proxies. Each V2
//! migration flips one prefix `Proxy → Native` (CP4 `od-catalog` first). When
//! the last prefix flips, the sidecar is deleted and this *is* the daemon.
//!
//! ## Dispatcher
//!
//! [`RouteTable::into_router`] compiles the declarative table into an axum
//! `Router` (the dispatcher): Native prefixes are `nest`ed, and the `/` Proxy
//! entry becomes the catch-all fallback. axum resolves most-specific-match, so a
//! Native prefix wins over the fallback automatically.
//!
//! ## `force_proxy` (CP4 rollback lever)
//!
//! With [`RouteTable::force_proxy`] set, Native entries are *not* registered, so
//! their prefixes fall through to the proxy fallback — i.e. the whole app
//! reverts to pure-proxy behaviour without touching the table's contents. This
//! is the `--force-proxy` escape hatch the contract requires until each native
//! route passes its golden tests.

use axum::{extract::Request, routing::any, routing::MethodRouter, Router};
use tower_http::trace::TraceLayer;

use crate::proxy::{self, ProxyState};

/// What handles a matched prefix.
pub enum Target {
    /// Forward to the Node sidecar at this origin (`scheme://host:port`).
    Proxy { upstream: String },
    /// Handled in Rust by this router (registered at the entry's prefix).
    Native { router: Router },
}

/// One `(prefix, target)` row of the route table.
pub struct RouteEntry {
    pub prefix: String,
    pub target: Target,
}

/// The ordered set of route entries plus the `force_proxy` override.
#[derive(Default)]
pub struct RouteTable {
    entries: Vec<RouteEntry>,
    force_proxy: bool,
}

impl RouteTable {
    /// An empty table (no fallback → 404 until a `/` proxy or native route is
    /// added). Prefer [`RouteTable::proxy_all`] for the V1 default.
    pub fn new() -> Self {
        Self::default()
    }

    /// The V1 default: `("/", Proxy)` — every request proxies to `upstream`.
    pub fn proxy_all(upstream: impl Into<String>) -> Self {
        Self::new().proxy("/", upstream)
    }

    /// Add a `Proxy` entry. Use `"/"` for the catch-all (the V1 case).
    pub fn proxy(mut self, prefix: impl Into<String>, upstream: impl Into<String>) -> Self {
        self.entries.push(RouteEntry {
            prefix: prefix.into(),
            target: Target::Proxy {
                upstream: upstream.into(),
            },
        });
        self
    }

    /// Add a `Native` entry: `router` serves everything under `prefix`.
    pub fn native(mut self, prefix: impl Into<String>, router: Router) -> Self {
        self.entries.push(RouteEntry {
            prefix: prefix.into(),
            target: Target::Native { router },
        });
        self
    }

    /// Force every `Native` prefix back to `Proxy` (the CP4 `--force-proxy`
    /// rollback). Returns `self` for chaining.
    pub fn force_proxy(mut self, force: bool) -> Self {
        self.force_proxy = force;
        self
    }

    /// The configured entries, in insertion order (for diagnostics/logging).
    pub fn entries(&self) -> &[RouteEntry] {
        &self.entries
    }

    /// Whether the `--force-proxy` override is active.
    pub fn is_force_proxy(&self) -> bool {
        self.force_proxy
    }

    /// Compile the table into the axum dispatcher.
    pub fn into_router(self) -> Router {
        let force_proxy = self.force_proxy;
        let mut app = Router::new();
        // Defer the `/` proxy to the very end so it becomes the fallback.
        let mut catch_all: Option<String> = None;

        for entry in self.entries {
            match entry.target {
                Target::Native { router } => {
                    if force_proxy {
                        tracing::debug!(prefix = %entry.prefix, "route: Native disabled by force_proxy → proxied");
                        continue;
                    }
                    tracing::debug!(prefix = %entry.prefix, "route: Native");
                    app = app.nest(&entry.prefix, router);
                }
                Target::Proxy { upstream } => {
                    if entry.prefix == "/" {
                        tracing::debug!(%upstream, "route: Proxy catch-all (\"/\")");
                        catch_all = Some(upstream);
                    } else {
                        // A non-root proxy prefix: register an explicit subtree
                        // (a nested router's own fallback would be ignored by
                        // axum, so use wildcard routes at the top level).
                        tracing::debug!(prefix = %entry.prefix, %upstream, "route: Proxy");
                        let exact = entry.prefix.clone();
                        let subtree = format!("{}/*rest", entry.prefix.trim_end_matches('/'));
                        app = app
                            .route(&exact, proxy_method_router(upstream.clone()))
                            .route(&subtree, proxy_method_router(upstream));
                    }
                }
            }
        }

        if let Some(upstream) = catch_all {
            app = app.fallback_service(proxy_method_router(upstream));
        }

        // Per-request route-resolution logging (method/path/status/latency).
        // `TraceLayer` wraps but never buffers the body, so SSE stays unbuffered.
        app.layer(TraceLayer::new_for_http())
    }
}

/// An `any`-method handler that forwards every request to `upstream` via the
/// streaming, hop-by-hop-stripping proxy. Used for both the catch-all fallback
/// and non-root proxy subtrees.
fn proxy_method_router(upstream: String) -> MethodRouter {
    let state = ProxyState::new(upstream);
    any(move |req: Request| {
        let state = state.clone();
        async move { proxy::forward(state, req).await }
    })
}
