//! CP4 golden tests — the V2 dress rehearsal.
//!
//! Drives the **real** axum handlers (`od_server::router_with_catalog`) over the
//! **vendored** upstream content and asserts each catalog route is byte-identical
//! to the fixtures captured from the pinned Node daemon
//! (`scripts/capture-golden.sh`), modulo array order normalized by `id`.
//!
//! This exercises the entire CP4 seam end to end: route-table dispatch → native
//! exact route → `od-catalog` parse/serialize → HTTP response. It is the gate the
//! contract requires before a `Proxy → Native` flip ships.

use std::net::SocketAddr;
use std::path::PathBuf;

use od_contract::golden::{normalize_by_id, GoldenFixture};
use tokio::net::TcpListener;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixtures")
}

/// The vendored upstream content root (`<repo>/vendor/open-design`).
fn content_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/open-design")
}

/// An empty data dir → empty user roots, matching a fresh daemon capture.
fn empty_data_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("od-golden-empty-data");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn spawn_catalog_router(force_proxy: bool) -> SocketAddr {
    let roots = od_catalog::CatalogRoots::new(content_root(), empty_data_dir());
    // Upstream is unused for the native GET routes under test; point it at a
    // closed port so an accidental proxy fall-through would fail loudly.
    let router = od_server::router_with_catalog("http://127.0.0.1:1", roots, force_proxy);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    addr
}

async fn assert_route_matches_golden(name: &str) {
    let fixture = GoldenFixture::load(fixtures_dir(), name)
        .unwrap_or_else(|e| panic!("load fixture {name}: {e} (run scripts/capture-golden.sh)"));
    assert_eq!(fixture.method, "GET");

    let addr = spawn_catalog_router(false).await;
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}{}", fixture.path))
        .send()
        .await
        .unwrap();

    // Status parity.
    assert_eq!(
        resp.status().as_u16(),
        fixture.status,
        "[{name}] status mismatch"
    );

    // Header-subset parity (e.g. content-type: application/json; charset=utf-8).
    for (key, expected) in &fixture.headers {
        let actual = resp
            .headers()
            .get(key)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| panic!("[{name}] missing header {key}"));
        assert_eq!(actual, expected, "[{name}] header {key} mismatch");
    }

    // Body parity, byte-identical after id-normalization.
    let body = resp.text().await.unwrap();
    let array_key = fixture
        .array_key
        .clone()
        .expect("catalog fixture has arrayKey");
    let actual = normalize_by_id(&body, &array_key);
    let expected = fixture.normalized_body();
    assert_eq!(
        actual, expected,
        "[{name}] body not byte-identical to daemon"
    );
}

#[tokio::test]
async fn skills_route_matches_golden() {
    assert_route_matches_golden("skills").await;
}

#[tokio::test]
async fn design_systems_route_matches_golden() {
    assert_route_matches_golden("design-systems").await;
}

#[tokio::test]
async fn design_templates_route_matches_golden() {
    assert_route_matches_golden("design-templates").await;
}

/// `--force-proxy` reverts the native catalog routes to the proxy. With the
/// upstream pointed at a closed port, the request must fail to connect rather
/// than be served natively — proving the rollback lever bypasses `od-catalog`.
#[tokio::test]
async fn force_proxy_bypasses_native_catalog() {
    let addr = spawn_catalog_router(true).await;
    let result = reqwest::Client::new()
        .get(format!("http://{addr}/api/skills"))
        .send()
        .await;
    // The catch-all proxy tries the closed upstream → 502 (bad gateway), never a
    // native 200. (A native route would have returned the skills JSON.)
    match result {
        Ok(resp) => assert_eq!(
            resp.status().as_u16(),
            502,
            "force_proxy should proxy to the (dead) upstream, not serve natively"
        ),
        Err(_) => { /* connection error to the dead upstream is also acceptable */ }
    }
}
