//! CP2 spike harness — SSE through a real `EventSource`, end to end via axum.
//!
//! The automated proof (`tests/sse.rs`) asserts incremental delivery with a
//! streaming HTTP client. This harness closes the *real-`EventSource`* half of
//! the CP2 spike: it serves a page whose `EventSource` connects back through the
//! od-server proxy to a mock SSE source, so you can open it in the actual
//! WebKitGTK engine (the same one Tauri uses) and watch frames tick in live.
//!
//! Topology (mirrors the V1 seam — webview → axum route table → daemon):
//!
//! ```text
//!   browser/webview  ──/app──▶  od-server  (Native: serves the HTML page)
//!                    ──/sse──▶  od-server  (Proxy catch-all) ──▶  mock SSE upstream
//! ```
//!
//! Run it:
//!
//! ```sh
//! cargo run -p od-server --example sse_spike
//! # → prints a URL; open it in a browser, or in WebKitGTK:
//! #   GDK_BACKEND=x11 webkit2gtk-... / or just paste into any Chromium/Firefox
//! ```
//!
//! Pass: the `<pre>` log fills with `tick 1, tick 2, …` one line every ~500ms
//! (NOT all at once after a delay — that would mean buffering). The header shows
//! `content-type: text/event-stream` preserved.

use std::net::Ipv4Addr;
use std::time::Duration;

use axum::{
    body::{Body, Bytes},
    response::{Html, Response},
    routing::get,
    Router,
};
use od_server::RouteTable;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[tokio::main]
async fn main() {
    // 1. Mock SSE upstream: emits one frame every 500ms, forever.
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    tokio::spawn(async move {
        let app = Router::new().route("/sse", get(sse_source));
        axum::serve(upstream, app).await.unwrap();
    });

    // 2. od-server: Native page at /app, everything else proxied to the upstream.
    let table = RouteTable::proxy_all(format!("http://{upstream_addr}"))
        .native("/app", Router::new().route("/", get(page)));

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    println!("\n  CP2 SSE spike ready — open:  http://{addr}/app\n");
    axum::serve(listener, table.into_router()).await.unwrap();
}

/// Infinite SSE stream, one `tick N` frame every 500ms.
async fn sse_source() -> Response {
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(1);
    tokio::spawn(async move {
        let mut n = 0u64;
        loop {
            n += 1;
            if tx
                .send(Ok(Bytes::from(format!("data: tick {n}\n\n"))))
                .await
                .is_err()
            {
                break; // client disconnected
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(Body::from_stream(ReceiverStream::new(rx)))
        .unwrap()
}

/// Minimal page whose `EventSource` reconnects through the proxy.
async fn page() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<meta charset="utf-8">
<title>CP2 SSE EventSource spike</title>
<h1>CP2 — SSE via real EventSource</h1>
<p>Frames should appear one per ~500ms (live), not all at once.</p>
<pre id="log" style="font:14px monospace;background:#111;color:#0f0;padding:1rem"></pre>
<script>
  const log = document.getElementById("log");
  const es = new EventSource("/sse");
  es.onmessage = (e) => { log.textContent += e.data + " @ " + new Date().toISOString() + "\n"; };
  es.onerror = () => { log.textContent += "[error/closed]\n"; };
</script>
"#,
    )
}
