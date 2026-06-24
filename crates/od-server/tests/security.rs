//! CP6 integration tests: authoritative CSP response header.
//!
//! The webview loads the external axum origin, so axum must emit the real CSP for
//! served HTML (the `tauri.conf.json` config CSP only governs Tauri-origin
//! content). These assert the header lands on `text/html` and on nothing else —
//! crucially not on `text/event-stream`, where extra headers are harmless but the
//! point is to prove SSE responses are untouched by the middleware.

use std::net::SocketAddr;

use axum::{extract::Request, http::StatusCode, response::Response, routing::any, Router};
use tokio::net::TcpListener;

/// Mock upstream that serves HTML at `/`, JSON at `/api/x`, and SSE at `/sse` —
/// and plants a bogus CSP on the HTML so we can prove axum *overwrites* it.
async fn upstream(req: Request) -> Response {
    let path = req.uri().path().to_string();
    let (ct, body) = match path.as_str() {
        "/api/x" => ("application/json", "{\"ok\":true}"),
        "/sse" => ("text/event-stream", "event: ready\ndata: 1\n\n"),
        _ => (
            "text/html; charset=utf-8",
            "<!doctype html><title>app</title>",
        ),
    };
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", ct);
    if ct.starts_with("text/html") {
        builder = builder.header("content-security-policy", "default-src 'none'");
    }
    builder.body(axum::body::Body::from(body)).unwrap()
}

async fn spawn(router: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    addr
}

/// Bring up the mock daemon + the real od-server proxy router in front of it.
async fn harness() -> SocketAddr {
    let upstream_addr = spawn(Router::new().fallback(any(upstream))).await;
    let proxy = od_server::router(format!("http://{upstream_addr}"));
    spawn(proxy).await
}

#[tokio::test]
async fn html_gets_authoritative_csp_overwriting_upstream() {
    let proxy = harness().await;
    let resp = reqwest::get(format!("http://{proxy}/")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let csp = resp
        .headers()
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // Upstream's "default-src 'none'" was overwritten by ours.
    assert!(csp.starts_with("default-src 'self'"), "csp = {csp:?}");
    assert!(csp.contains("object-src 'none'"), "csp = {csp:?}");
    assert!(csp.contains("frame-ancestors 'self'"), "csp = {csp:?}");
    // Companion hardening headers present on HTML.
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert!(resp.headers().get("referrer-policy").is_some());
}

#[tokio::test]
async fn json_response_has_no_csp() {
    let proxy = harness().await;
    let resp = reqwest::get(format!("http://{proxy}/api/x")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("content-security-policy").is_none());
    assert!(resp.headers().get("x-content-type-options").is_none());
}

#[tokio::test]
async fn sse_response_is_untouched() {
    let proxy = harness().await;
    let resp = reqwest::get(format!("http://{proxy}/sse")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
    // No CSP / nosniff injected onto the event stream.
    assert!(resp.headers().get("content-security-policy").is_none());
    assert!(resp.headers().get("x-content-type-options").is_none());
}
