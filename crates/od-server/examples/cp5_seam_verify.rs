//! CP5 verification harness (temporary): serve the real CP4+CP5 router
//! (`router_with_catalog`) against a live embedded daemon so curl can exercise
//! the acceptance integrations *through the exact axum seam the webview uses*:
//!
//!   - **CLI detection** — `GET /api/agents` (proxied) lists the agent CLIs the
//!     daemon detected from the env_inject-injected PATH / seeded `*_BIN`.
//!   - **BYOK + SSRF** — `POST /api/proxy/{provider}/stream` (proxied) enforces
//!     the daemon's server-side SSRF guard (403 for private/link-local base
//!     URLs) and is reachable independent of any CLI.
//!
//! Run (the daemon must already be up with the CP5 env contract; see
//! `scripts/cp5-acceptance.sh`, which boots both and drives the assertions):
//!   OD_DAEMON_URL=http://127.0.0.1:<port> OD_CONTENT_ROOT=vendor/open-design \
//!   OD_DATA_DIR=<dir> cargo run -p od-server --example cp5_seam_verify
//! Prints `AXUM_URL=http://127.0.0.1:<port>` then serves until killed.

#[tokio::main]
async fn main() {
    let daemon_url = std::env::var("OD_DAEMON_URL").expect("set OD_DAEMON_URL");
    let content_root = std::env::var("OD_CONTENT_ROOT").expect("set OD_CONTENT_ROOT");
    let data_dir = std::env::var("OD_DATA_DIR").expect("set OD_DATA_DIR");
    let roots = od_catalog::CatalogRoots::new(content_root, data_dir);
    let force_proxy = std::env::var("OD_FORCE_PROXY").is_ok();

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind axum");
    let port = listener.local_addr().unwrap().port();
    println!("AXUM_URL=http://127.0.0.1:{port}");
    let router = od_server::router_with_catalog(daemon_url, roots, force_proxy);
    axum::serve(listener, router).await.expect("serve");
}
