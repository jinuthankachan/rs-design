//! CP3 verification harness (temporary): serve the real `od_server::router`
//! against a live embedded daemon so curl can exercise the axum→daemon seam the
//! webview depends on (R1 SPA fallback, R2 origin-root assets, catalog reads).
//!
//! Run: `OD_DAEMON_URL=http://127.0.0.1:<port> cargo run -p od-server --example cp3_proxy_verify`
//! Prints `AXUM_URL=http://127.0.0.1:<port>` then serves until killed.

#[tokio::main]
async fn main() {
    let daemon_url = std::env::var("OD_DAEMON_URL").expect("set OD_DAEMON_URL");
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind axum");
    let port = listener.local_addr().unwrap().port();
    println!("AXUM_URL=http://127.0.0.1:{port}");
    let router = od_server::router(daemon_url);
    axum::serve(listener, router).await.expect("serve");
}
