//! CP4 native catalog routes — the first `Proxy → Native` flips.
//!
//! `GET /api/skills`, `GET /api/design-systems`, and `GET /api/design-templates`
//! are served in Rust by [`od_catalog`], byte-identical to the Node daemon. Each
//! is registered as an **exact-path** native route ([`RouteTable::native_exact`])
//! so that:
//!
//! - only the exact path is intercepted — sibling routes the daemon still owns
//!   (`/api/skills/:id`, `/api/design-systems/:id`, …) fall through to the proxy;
//! - only `GET` is handled — other methods on the same path (e.g.
//!   `POST /api/design-systems`, which creates a user design system) fall through
//!   to the proxy via the method router's fallback.
//!
//! `--force-proxy` ([`RouteTable::force_proxy`]) skips registration entirely, so
//! the whole surface reverts to the Node daemon — the contract's rollback lever.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::Request,
    http::{header, StatusCode},
    response::Response,
    routing::{any, get, MethodRouter},
};

use od_catalog::CatalogRoots;

use crate::proxy::{self, ProxyState};

/// `res.json(...)` parity: a 200 with `application/json; charset=utf-8` (Express's
/// default for `res.json`) and the pre-serialized body.
fn json_response(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(body))
        .expect("static catalog response builds")
}

/// Build a method router that serves `render(roots)` on `GET` and proxies every
/// other method on the path to `upstream` (so the daemon keeps owning, e.g.,
/// `POST /api/design-systems`).
fn catalog_method_router(
    roots: Arc<CatalogRoots>,
    upstream: String,
    render: fn(&CatalogRoots) -> String,
) -> MethodRouter {
    let proxy_state = ProxyState::new(upstream);
    get(move || {
        let roots = roots.clone();
        async move { json_response(render(&roots)) }
    })
    .fallback(any(move |req: Request| {
        let state = proxy_state.clone();
        async move { proxy::forward(state, req).await }
    }))
}

/// The three CP4 catalog routes as `(path, method_router)` pairs, ready for
/// [`RouteTable::native_exact`]. `upstream` is the Node sidecar origin used for
/// the non-`GET` method fallbacks.
pub fn catalog_routes(
    roots: CatalogRoots,
    upstream: impl Into<String>,
) -> Vec<(&'static str, MethodRouter)> {
    let roots = Arc::new(roots);
    let upstream = upstream.into();
    vec![
        (
            "/api/skills",
            catalog_method_router(roots.clone(), upstream.clone(), od_catalog::skills_json),
        ),
        (
            "/api/design-systems",
            catalog_method_router(
                roots.clone(),
                upstream.clone(),
                od_catalog::design_systems_json,
            ),
        ),
        (
            "/api/design-templates",
            catalog_method_router(roots, upstream, od_catalog::design_templates_json),
        ),
    ]
}
