//! od-agents — coding-agent transport + per-CLI adapters (the long pole).
//!
//! Owns `/api/chat` (SSE) and spawns coding-agent CLIs via `tokio::process` with
//! line-delimited JSON. Do one adapter end to end first (Claude Code
//! `stream-json`), then ACP (one parser covers six CLIs: Devin/Hermes/Kimi/
//! Kiro/Kilo/Vibe), then the stragglers. Centralized env control (PATH + `*_BIN`)
//! is set up in V1's supervisor (CP5), so spawning CLIs directly in V2 inherits a
//! clean environment.
//
// TODO(V2 step 5): tokio::process transport + per-CLI stream parsers.
