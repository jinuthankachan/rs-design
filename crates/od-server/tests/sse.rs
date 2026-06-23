//! CP2-Task2 integration tests: SSE-safe streaming proxy.
//!
//! Proves the catch-all proxy carries Server-Sent Events without buffering or
//! compressing them: `text/event-stream` + cache headers survive, frames arrive
//! incrementally (first frame before the upstream emits the second), and
//! `accept-encoding` never reaches the loopback daemon.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::{
    body::{Body, Bytes},
    extract::Request,
    response::Response,
    routing::any,
    Router,
};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

/// Mock SSE upstream: emits one frame, waits, emits a second, then closes — so a
/// non-buffering proxy must deliver frame 1 before frame 2 is even produced.
async fn sse_upstream(req: Request) -> Response {
    // Echo back whether the proxy forwarded a compression preference.
    let saw_accept_encoding = req.headers().contains_key("accept-encoding");

    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(1);
    tokio::spawn(async move {
        let _ = tx
            .send(Ok(Bytes::from(format!(
                "data: 1 (accept-encoding={saw_accept_encoding})\n\n"
            ))))
            .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = tx.send(Ok(Bytes::from("data: 2\n\n"))).await;
    });

    Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        // Hop-by-hop — must be stripped before reaching the client.
        .header("connection", "keep-alive")
        .body(Body::from_stream(ReceiverStream::new(rx)))
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

async fn setup() -> String {
    let upstream = spawn(Router::new().fallback(any(sse_upstream))).await;
    let proxy = spawn(od_server::router(format!("http://{upstream}"))).await;
    format!("http://{proxy}")
}

#[tokio::test]
async fn preserves_event_stream_content_type_and_cache_headers() {
    let base = setup().await;
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/chat"))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream"),
    );
    assert_eq!(
        resp.headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-cache"),
    );
    // Hop-by-hop response header must not leak to the client.
    assert!(!resp.headers().contains_key("connection"));
}

#[tokio::test]
async fn streams_frames_incrementally_without_buffering() {
    let base = setup().await;
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/chat"))
        .send()
        .await
        .unwrap();

    let mut stream = resp.bytes_stream();
    let t0 = Instant::now();

    let first = stream.next().await.expect("first frame").unwrap();
    let first_at = t0.elapsed();

    let mut rest = Vec::new();
    while let Some(chunk) = stream.next().await {
        rest.extend_from_slice(&chunk.unwrap());
    }
    let total_at = t0.elapsed();

    assert!(
        first.starts_with(b"data: 1"),
        "unexpected first frame: {first:?}"
    );
    // The first frame arrives well before the upstream's 300ms gap elapses —
    // impossible if the proxy buffered the whole body.
    assert!(
        first_at < Duration::from_millis(250),
        "first frame arrived too late ({first_at:?}) — body was buffered"
    );
    // The full body only completes after the upstream's delay.
    assert!(
        total_at >= Duration::from_millis(280),
        "stream finished implausibly fast ({total_at:?})"
    );
    assert!(
        String::from_utf8_lossy(&rest).contains("data: 2"),
        "second frame missing"
    );
}

#[tokio::test]
async fn drops_accept_encoding_so_upstream_never_compresses() {
    let base = setup().await;
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/chat"))
        // The webview would send this; the proxy must not relay it to loopback.
        .header("accept-encoding", "gzip, br")
        .send()
        .await
        .unwrap();

    let first = resp.bytes_stream().next().await.expect("frame").unwrap();
    assert!(
        String::from_utf8_lossy(&first).contains("accept-encoding=false"),
        "accept-encoding leaked to upstream: {first:?}"
    );
}
