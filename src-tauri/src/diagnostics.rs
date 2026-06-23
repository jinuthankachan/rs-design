//! Startup acceptance diagnostics (CP5).
//!
//! Once the backend is ready, probe `GET /api/agents` *through the axum seam*
//! (the exact path the webview's Settings UI uses) and log a concise summary of
//! which agent CLIs the daemon detected — the env_inject levers' observable
//! payoff. This is the app's diagnostic surface in the dev loop; the
//! *user-facing* surface is the daemon's Settings UI, which is already served
//! through axum (proxied), so we deliberately don't rebuild it here.
//!
//! The two CP5 acceptance levers this summarizes:
//!   - **CLI detection** — `claude` + ≥1 other CLI available (PATH reconstruction
//!     and/or explicit `*_BIN`); a no-CLI machine degrades gracefully (the daemon
//!     still serves, agents just report `available: false`).
//!   - **BYOK** — works with **no** CLI present: the frontend POSTs
//!     `/api/proxy/{provider}/stream` with `baseUrl`/`apiKey` in the body and the
//!     daemon applies the SSRF guard server-side. Nothing to detect at startup;
//!     noted here so the diagnostic reads as a complete acceptance picture.

use std::time::Duration;

use serde::Deserialize;

/// One agent entry from `GET /api/agents` (subset of the daemon's `DetectedAgent`
/// we care about for the summary). Unknown fields are ignored.
#[derive(Debug, Deserialize)]
struct DetectedAgent {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    available: bool,
    #[serde(default)]
    version: Option<String>,
    #[serde(rename = "authStatus", default)]
    auth_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentsResponse {
    #[serde(default)]
    agents: Vec<DetectedAgent>,
}

/// Probe `/api/agents` through axum and log the acceptance summary. Best-effort:
/// a failure logs a warning and returns, never blocking startup.
pub async fn log_acceptance_summary(axum_url: &str) {
    let url = format!("{axum_url}/api/agents");
    let client = match reqwest::Client::builder()
        // Detection spawns `--version` probes per CLI; give it generous headroom.
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!(error = %err, "diagnostics: http client build failed");
            return;
        }
    };

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!(error = %err, "diagnostics: GET /api/agents failed");
            return;
        }
    };
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "diagnostics: /api/agents non-200");
        return;
    }
    let body = match resp.json::<AgentsResponse>().await {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!(error = %err, "diagnostics: /api/agents parse failed");
            return;
        }
    };

    let available: Vec<&DetectedAgent> = body.agents.iter().filter(|a| a.available).collect();
    for a in &available {
        let name = a.name.as_deref().unwrap_or(&a.id);
        tracing::info!(
            id = %a.id,
            name = %name,
            version = a.version.as_deref().unwrap_or("?"),
            auth = a.auth_status.as_deref().unwrap_or("?"),
            "agent CLI detected"
        );
    }

    let claude_ok = available.iter().any(|a| a.id == "claude");
    let other_ok = available.iter().any(|a| a.id != "claude");

    tracing::info!(
        total = body.agents.len(),
        available = available.len(),
        claude_detected = claude_ok,
        second_cli_detected = other_ok,
        "CP5 CLI-detection summary"
    );

    if claude_ok && other_ok {
        tracing::info!("CP5 acceptance: Claude Code + ≥1 other CLI detected ✓");
    } else if available.is_empty() {
        tracing::info!(
            "CP5 acceptance: no agent CLI detected — degrading gracefully (BYOK still available; \
             set CLAUDE_BIN/CODEX_BIN or fix login-shell PATH to enable CLI agents)"
        );
    } else {
        tracing::info!(
            claude_detected = claude_ok,
            second_cli_detected = other_ok,
            "CP5 acceptance: partial CLI detection — see per-agent diagnostics in Settings"
        );
    }

    // BYOK is request-driven (no startup probe); state it so the diagnostic is
    // a complete acceptance picture.
    tracing::info!(
        "CP5 acceptance: BYOK available regardless of CLI presence \
         (POST /api/proxy/{{provider}}/stream, SSRF-guarded server-side)"
    );
}
