//! CP2-Task3 integration tests: the `Proxy | Native` route table + dispatcher.
//!
//! Verifies that Native prefixes are served in Rust, everything else falls
//! through to the proxy catch-all, and `force_proxy` reverts Native prefixes to
//! the proxy without changing the table's contents.

use std::net::SocketAddr;

use axum::{
    extract::Request,
    response::Response,
    routing::{any, get, MethodRouter},
    Router,
};
use od_server::{RouteTable, Target};
use tokio::net::TcpListener;

/// Mock upstream that reflects the path it received, so a proxied request is
/// distinguishable from a native one.
async fn upstream_reflect(req: Request) -> Response {
    let path = req.uri().path().to_string();
    Response::new(axum::body::Body::from(format!("UPSTREAM:{path}")))
}

async fn spawn(router: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    addr
}

async fn upstream() -> String {
    let addr = spawn(Router::new().fallback(any(upstream_reflect))).await;
    format!("http://{addr}")
}

fn native_skills() -> Router {
    Router::new().route("/", get(|| async { "NATIVE-SKILLS" }))
}

#[tokio::test]
async fn native_prefix_served_by_rust_rest_proxied() {
    let up = upstream().await;
    let table = RouteTable::proxy_all(&up).native("/api/skills", native_skills());
    let base = format!("http://{}", spawn(table.into_router()).await);
    let client = reqwest::Client::new();

    // Native prefix → handled in Rust.
    let native = client
        .get(format!("{base}/api/skills"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(native, "NATIVE-SKILLS");

    // Anything else → proxy catch-all.
    let proxied = client
        .get(format!("{base}/api/design-systems"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(proxied, "UPSTREAM:/api/design-systems");
}

#[tokio::test]
async fn force_proxy_reverts_native_to_proxy() {
    let up = upstream().await;
    let table = RouteTable::proxy_all(&up)
        .native("/api/skills", native_skills())
        .force_proxy(true);
    let base = format!("http://{}", spawn(table.into_router()).await);

    // With force_proxy, the native prefix is NOT registered → proxied instead.
    let body = reqwest::Client::new()
        .get(format!("{base}/api/skills"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, "UPSTREAM:/api/skills");
}

#[tokio::test]
async fn v1_default_proxies_everything() {
    let up = upstream().await;
    let base = format!(
        "http://{}",
        spawn(RouteTable::proxy_all(&up).into_router()).await
    );

    let body = reqwest::Client::new()
        .get(format!("{base}/anything/at/all"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, "UPSTREAM:/anything/at/all");
}

#[tokio::test]
async fn non_root_proxy_prefix_forwards_prefix_and_subtree() {
    let up = upstream().await;
    // A native catch-all-ish app where /legacy proxies to a (here, same) upstream
    // and the rest is served natively, proving non-root Proxy entries work.
    let table = RouteTable::new().proxy("/legacy", &up).native(
        "/",
        Router::new().route("/", get(|| async { "ROOT-NATIVE" })),
    );
    let base = format!("http://{}", spawn(table.into_router()).await);
    let client = reqwest::Client::new();

    let exact = client
        .get(format!("{base}/legacy"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(exact, "UPSTREAM:/legacy");

    let subtree = client
        .get(format!("{base}/legacy/deep/path"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(subtree, "UPSTREAM:/legacy/deep/path");
}

/// A CP4-style exact-path native route: `GET` is handled in Rust, every other
/// method falls through to the method router's fallback (wired to the proxy in
/// production via `catalog::catalog_routes`).
fn exact_skills() -> MethodRouter {
    get(|| async { "NATIVE-EXACT" }).fallback(any(|| async { "METHOD-FALLBACK" }))
}

#[tokio::test]
async fn native_exact_serves_path_but_siblings_and_methods_fall_through() {
    let up = upstream().await;
    let table = RouteTable::proxy_all(&up).native_exact("/api/skills", exact_skills());
    let base = format!("http://{}", spawn(table.into_router()).await);
    let client = reqwest::Client::new();

    // Exact path + GET → native.
    let native = client
        .get(format!("{base}/api/skills"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(native, "NATIVE-EXACT");

    // Sibling path → proxied (the daemon still owns `/api/skills/:id`).
    let sibling = client
        .get(format!("{base}/api/skills/some-id"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(sibling, "UPSTREAM:/api/skills/some-id");

    // Non-GET on the exact path → method fallback (proxy in production).
    let posted = client
        .post(format!("{base}/api/skills"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(posted, "METHOD-FALLBACK");
}

#[tokio::test]
async fn force_proxy_reverts_native_exact_to_proxy() {
    let up = upstream().await;
    let table = RouteTable::proxy_all(&up)
        .native_exact("/api/skills", exact_skills())
        .force_proxy(true);
    let base = format!("http://{}", spawn(table.into_router()).await);

    let body = reqwest::Client::new()
        .get(format!("{base}/api/skills"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, "UPSTREAM:/api/skills");
}

/// `Target` is a real public enum (the exit criterion: "route table is a real
/// `Proxy | Native` type"), constructible and inspectable.
#[test]
fn target_is_a_real_typed_enum() {
    let table = RouteTable::new()
        .proxy("/", "http://127.0.0.1:9")
        .native("/api/skills", Router::new())
        .native_exact("/api/design-systems", exact_skills());
    let kinds: Vec<&str> = table
        .entries()
        .iter()
        .map(|e| match e.target {
            Target::Proxy { .. } => "proxy",
            Target::Native { .. } => "native",
            Target::NativeExact { .. } => "native-exact",
        })
        .collect();
    assert_eq!(kinds, ["proxy", "native", "native-exact"]);
}
