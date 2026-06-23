//! CP2-Task1 integration tests: catch-all forwarding + hop-by-hop stripping.
//!
//! Spins up a mock "daemon" and the od-server proxy on ephemeral loopback
//! ports, then drives requests through the proxy and asserts parity.

use std::net::SocketAddr;

use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use tokio::net::TcpListener;

/// Mock upstream: reflects what it received and plants headers to probe
/// response-side stripping.
async fn upstream_reflect(req: Request) -> Response {
    let method = req.method().to_string();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_default();

    // Did a hop-by-hop request header survive the proxy? (It must not.)
    let saw_connection = req.headers().contains_key("connection");
    let saw_forwarded = req
        .headers()
        .get("x-forwarded-marker")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let body_text = String::from_utf8_lossy(&body).to_string();

    let payload = format!(
        "method={method};path={path_and_query};saw_connection={saw_connection};forwarded={saw_forwarded};body={body_text}"
    );

    Response::builder()
        .status(StatusCode::OK)
        .header("x-end-to-end", "kept")
        // A hop-by-hop response header that must be stripped before the client.
        .header("keep-alive", "timeout=5")
        .body(axum::body::Body::from(payload))
        .unwrap()
}

async fn spawn(router: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    addr
}

/// Start mock upstream + proxy; return the proxy's base URL.
async fn setup() -> String {
    let upstream_addr = spawn(Router::new().fallback(any(upstream_reflect))).await;
    let proxy_addr = spawn(od_server::router(format!("http://{upstream_addr}"))).await;
    format!("http://{proxy_addr}")
}

#[tokio::test]
async fn forwards_method_path_and_query() {
    let base = setup().await;
    let client = reqwest::Client::new();

    let body = client
        .get(format!("{base}/api/skills?limit=2"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("method=GET"), "got: {body}");
    assert!(body.contains("path=/api/skills?limit=2"), "got: {body}");
}

#[tokio::test]
async fn forwards_request_body_unbuffered() {
    let base = setup().await;
    let client = reqwest::Client::new();

    let body = client
        .post(format!("{base}/api/chat"))
        .body("hello-upstream")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("method=POST"), "got: {body}");
    assert!(body.contains("body=hello-upstream"), "got: {body}");
}

#[tokio::test]
async fn strips_hop_by_hop_request_headers_but_keeps_end_to_end() {
    let base = setup().await;
    let client = reqwest::Client::new();

    let body = client
        .get(format!("{base}/api/anything"))
        // `connection` is hop-by-hop → must not reach upstream.
        .header("connection", "keep-alive")
        // An ordinary end-to-end header → must reach upstream.
        .header("x-forwarded-marker", "present")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("saw_connection=false"), "got: {body}");
    assert!(body.contains("forwarded=present"), "got: {body}");
}

#[tokio::test]
async fn strips_hop_by_hop_response_headers_but_keeps_end_to_end() {
    let base = setup().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/api/anything"))
        .send()
        .await
        .unwrap();

    let headers: &HeaderMap = resp.headers();
    assert!(
        !headers.contains_key("keep-alive"),
        "hop-by-hop response header leaked: {headers:?}"
    );
    assert_eq!(
        headers.get("x-end-to-end").and_then(|v| v.to_str().ok()),
        Some("kept"),
        "end-to-end response header dropped: {headers:?}"
    );
}

#[tokio::test]
async fn bad_gateway_when_upstream_unreachable() {
    // Point the proxy at a port nothing is listening on.
    let proxy_addr = spawn(od_server::router("http://127.0.0.1:1")).await;
    let client = reqwest::Client::new();

    let status = client
        .get(format!("http://{proxy_addr}/api/skills"))
        .send()
        .await
        .unwrap()
        .status();

    assert_eq!(status, StatusCode::BAD_GATEWAY);
}
