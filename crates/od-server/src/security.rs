//! Authoritative security-response headers (CP6).
//!
//! The webview loads the **external axum http origin** (not the Tauri asset
//! protocol), so the `tauri.conf.json` `app.security.csp` — which only governs
//! Tauri-origin content — is *not* the authoritative policy for the app the user
//! actually sees. axum must emit the real `Content-Security-Policy` response
//! header for the HTML it serves. This was deferred from CP2 (see
//! `docs/spikes/cp2-seam.md`) to here.
//!
//! ## Scope: HTML responses only
//!
//! The header is attached only to `text/html` responses (the app shell pages the
//! daemon serves through the proxy). JSON catalog/API responses, static
//! `/_next/*` assets, and — crucially — `text/event-stream` SSE are left
//! untouched, so streaming framing and asset caching are unaffected. Any CSP the
//! upstream daemon already set is **overwritten** (insert, not append) so two
//! policies can't intersect into a broken one — axum's is authoritative.
//!
//! ## Policy shape
//!
//! Hardened where it is free to be, permissive where artifacts need it:
//! - `object-src 'none'` + `base-uri 'self'` + `frame-ancestors 'self'` — close
//!   off plugin embedding, `<base>` injection, and clickjacking.
//! - `frame-src`/`img`/`font`/`style`/`script` allow `blob:`/`data:`/`https:`
//!   because agent-authored **artifacts render in sandboxed `srcdoc` iframes**
//!   that legitimately pull Google Fonts / Three.js / Tailwind from CDNs (online).
//!   `srcdoc` documents inherit the embedder's CSP, so a stricter shell policy
//!   would regress the showcase. The *offline* degradation of those CDN artifacts
//!   (KI-1) is a separate, explicitly-deferred CP6 bundling decision.
//! - `connect-src` allows `'self'` + loopback http/ws (the daemon `/api` + SSE)
//!   and `https:` (artifact fetches; BYOK itself stays server-side via `/api`).

use axum::http::header::{HeaderValue, CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY};
use axum::response::Response;

/// The authoritative CSP for served HTML. Single-line (header value).
const CONTENT_SECURITY_POLICY_VALUE: &str = "default-src 'self'; \
script-src 'self' 'unsafe-inline' 'unsafe-eval' https:; \
style-src 'self' 'unsafe-inline' https:; \
img-src 'self' data: blob: https:; \
font-src 'self' data: https:; \
connect-src 'self' http://127.0.0.1:* http://localhost:* ws://127.0.0.1:* ws://localhost:* https:; \
frame-src 'self' blob: data: https:; \
worker-src 'self' blob:; \
object-src 'none'; \
base-uri 'self'; \
frame-ancestors 'self'";

/// `X-Content-Type-Options` header name (not a typed constant in `http`).
const X_CONTENT_TYPE_OPTIONS: &str = "x-content-type-options";

/// axum `map_response` middleware: stamp the authoritative CSP (+ companion
/// hardening headers) onto `text/html` responses, leaving every other
/// content-type — JSON, assets, and SSE — untouched.
pub async fn set_security_headers(mut resp: Response) -> Response {
    let is_html = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|c| c.starts_with("text/html"))
        .unwrap_or(false);
    if !is_html {
        return resp;
    }
    let headers = resp.headers_mut();
    // insert (overwrite), so any upstream CSP is replaced rather than intersected.
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY_VALUE),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(
        REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    resp
}
