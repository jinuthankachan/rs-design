//! od-proxy — BYOK provider proxy with SSRF protection.
//!
//! Owns `/api/proxy/{provider}/stream`. Forwards to any OpenAI-compatible
//! endpoint (`reqwest`) with SSE normalization and an SSRF guard reimplemented
//! with `ipnet` (block private / link-local / CGNAT / multicast / reserved
//! ranges; disable upstream redirects). Self-contained and testable in isolation.
//
// TODO(V2 step 3): reqwest streaming + SSE normalization + ipnet SSRF guard.
