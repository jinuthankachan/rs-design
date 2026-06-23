//! Catch-all reverse proxy to the Node sidecar (CP2-Task1).
//!
//! Forwards every request that no `Native` route claims to the embedded Node
//! daemon over loopback, byte-for-byte. The request/response bodies are
//! **streamed** (never buffered) so this same handler carries SSE without
//! change — see CP2-Task2 for the SSE-specific guarantees (no compression,
//! framing preserved). Hop-by-hop headers are stripped in both directions per
//! RFC 9110 §7.6.1, since they describe a single transport hop and must not be
//! relayed across the proxy.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderName, StatusCode},
    response::Response,
};

/// Shared state for the proxy fallback: a reusable client plus the upstream
/// base origin (e.g. `http://127.0.0.1:<daemon-port>`).
#[derive(Clone)]
pub struct ProxyState {
    client: reqwest::Client,
    /// Upstream origin without a trailing slash; the incoming path+query is
    /// appended verbatim.
    upstream: String,
}

impl ProxyState {
    /// Build proxy state targeting `upstream` (scheme + host + port).
    ///
    /// Redirects are disabled: the proxy must relay 3xx responses unchanged
    /// (contract parity) rather than chase them itself, which also keeps the
    /// future BYOK SSRF guard honest.
    pub fn new(upstream: impl Into<String>) -> Self {
        let upstream = upstream.into().trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client with default rustls config should build");
        Self { client, upstream }
    }
}

/// Hop-by-hop headers (RFC 9110 §7.6.1) — connection-specific, never forwarded.
/// `host` and `content-length` are stripped from the *request* additionally:
/// `reqwest` derives both from the target URL and the re-streamed body.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn is_hop_by_hop(name: &HeaderName) -> bool {
    HOP_BY_HOP.contains(&name.as_str())
}

/// Tokens named by a `Connection` header are themselves hop-by-hop for this hop
/// (e.g. `Connection: x-custom`) and must also be dropped.
fn connection_tokens(headers: &HeaderMap) -> Vec<String> {
    let mut tokens = Vec::new();
    for value in headers.get_all("connection").iter() {
        if let Ok(v) = value.to_str() {
            for tok in v.split(',') {
                let tok = tok.trim();
                if !tok.is_empty() {
                    tokens.push(tok.to_ascii_lowercase());
                }
            }
        }
    }
    tokens
}

/// Copy `src` into a fresh map, dropping hop-by-hop headers, any header named by
/// a `Connection` token, and anything in `extra_drop` (lowercased names).
fn filter_headers(src: &HeaderMap, extra_drop: &[&str]) -> HeaderMap {
    let drop_tokens = connection_tokens(src);
    let mut out = HeaderMap::with_capacity(src.len());
    for (name, value) in src.iter() {
        let lower = name.as_str().to_ascii_lowercase();
        if is_hop_by_hop(name)
            || drop_tokens.contains(&lower)
            || extra_drop.contains(&lower.as_str())
        {
            continue;
        }
        // `append` preserves duplicate header lines (e.g. multiple Set-Cookie).
        out.append(name.clone(), value.clone());
    }
    out
}

/// axum fallback handler: forward the request to the upstream daemon and relay
/// the response, both with hop-by-hop headers stripped and bodies streamed.
pub async fn proxy_handler(
    State(state): State<ProxyState>,
    req: axum::extract::Request,
) -> Result<Response, StatusCode> {
    let (parts, body) = req.into_parts();

    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let url = format!("{}{}", state.upstream, path_and_query);

    let req_headers = filter_headers(&parts.headers, &["host", "content-length"]);

    // Stream the request body upstream rather than buffering it.
    let upstream_body = reqwest::Body::wrap_stream(body.into_data_stream());

    let upstream_resp = state
        .client
        .request(parts.method, &url)
        .headers(req_headers)
        .body(upstream_body)
        .send()
        .await
        .map_err(|err| {
            tracing::warn!(%url, error = %err, "upstream proxy request failed");
            StatusCode::BAD_GATEWAY
        })?;

    let status = upstream_resp.status();
    let resp_headers = filter_headers(upstream_resp.headers(), &[]);

    // Stream the response body back unbuffered (carries SSE as-is).
    let mut response = Response::new(Body::from_stream(upstream_resp.bytes_stream()));
    *response.status_mut() = status;
    *response.headers_mut() = resp_headers;

    tracing::debug!(%url, %status, "proxied request to upstream daemon");
    Ok(response)
}
