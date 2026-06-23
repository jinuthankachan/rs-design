# CP5 — Acceptance integrations (CLI/PATH, SSE chat, BYOK)

CP5 makes the three behavioral done-criteria from the CP1 acceptance-scope
decision real: (a) Claude Code detected via PATH, (b) ≥1 other CLI detected,
(c) BYOK works with **no** CLI present, (d) SSE chat + todo card stream live in
WebKitGTK. The first three are verified headlessly here through the real axum
seam; (d) is GUI-gated (clean desktop session) and documented at the end.

## The shape of the problem

In V1 the **daemon** owns CLI detection, chat/SSE, and the BYOK proxy. It detects
an agent CLI by resolving its binary from **its own `process.env.PATH`** (plus a
fixed set of well-known user toolchain dirs — Homebrew, `~/.local/bin`,
`~/.bun/bin`, version-manager dirs) — see
`vendor/open-design/apps/daemon/src/runtimes/executables.ts`. So the Rust port
must never reimplement detection; its job is to hand the daemon a *good*
environment. That is exactly **V1 gotcha #1**: a Linux GUI app doesn't inherit
the login-shell PATH, so a GUI-launched daemon can't find `claude`/`codex`/etc.

## Two independent levers (`src-tauri/src/env_inject.rs`)

### Lever 1 — login-shell PATH reconstruction
Run the user's `$SHELL` as a login+interactive shell and read back its `PATH`:

```
$SHELL -lic 'printf "__OD_PATH_START__%s__OD_PATH_END__" "$PATH"'
```

`-lic` sources both the login profile (`.zprofile`/`.bash_profile`/`.profile`)
and the interactive rc (`.zshrc`/`.bashrc`) where users usually extend PATH. The
markers isolate the PATH from any banner/job-control noise an interactive shell
prints; stderr is discarded; a 5 s timeout + kill keeps a hung rc file from
stranding startup. The result is **merged** with the GUI process PATH
(login entries first, de-duplicated) and injected as the daemon child's `PATH`
(`launcher.rs`). `resolve_node_bin` scans the same merged PATH so node itself is
found under GUI launch. Failure degrades gracefully to the GUI PATH;
`OD_SKIP_LOGIN_PATH=1` opts out (CI/tests).

### Lever 2 — explicit `*_BIN` overrides
The daemon honors `CLAUDE_BIN`, `CODEX_BIN`, … **only** via its
`app-config.json` `agentCliEnv` map (`configuredExecutableOverride` in
`executables.ts`), **not** from `process.env`. So `env_inject` collects any
`*_BIN` present in the app's own environment and **seeds** them into
`agentCliEnv`, never clobbering a value the user already set there. This lever is
fully **independent of PATH**: it works even when shell reconstruction yields
nothing (locked-down shell, exotic `$SHELL`). The `*_BIN` key list mirrors the
daemon's `AGENT_BIN_ENV_KEYS` exactly so the keys we write are the keys it reads.

> Why seed the file rather than pass env vars? The daemon's binary *resolution*
> (which decides `available`) reads `*_BIN` from `agentCliEnv`, not from
> `process.env`. Writing the file is the faithful way to make an explicit path
> take effect as a hard override.

Both levers are resolved once at supervisor start; `seed_agent_cli_env` runs
**after** the first-run config write so it merges into the same file.

## Startup diagnostics (`src-tauri/src/diagnostics.rs`)

After readiness, the app probes `GET /api/agents` **through axum** (off the window
critical path) and logs the acceptance summary: each available CLI
(name/version/auth), whether Claude Code + ≥1 other are present, graceful no-CLI
degradation, and a BYOK-always-available note. The *user-facing* surface stays the
daemon's Settings UI, which is already served through axum (proxied) — we don't
rebuild it.

## Verification (headless, through the real seam)

`scripts/cp5-acceptance.sh` boots throwaway daemons and serves the real
`router_with_catalog` (`crates/od-server/examples/cp5_seam_verify.rs`), then
drives curl against the axum origin — the exact path the webview uses.

| # | Check | Through axum | Result |
|---|---|---|---|
| 1 | CLI detection (full PATH) | `GET /api/agents` | ✅ 200; `claude 2.1.137` + `antigravity 1.0.10` available → **Claude Code + ≥1 other** |
| 2 | No-CLI graceful degradation | `GET /api/agents` (minimal PATH + empty `OD_AGENT_HOME`) | ✅ 200, **0 available** — daemon still serves |
| 3 | SSRF guard intact | `POST /api/proxy/anthropic/stream` `baseUrl=http://169.254.169.254/v1` | ✅ **403** (link-local blocked), no CLI required |
| 3 | BYOK reachable w/o CLI | `POST /api/proxy/anthropic/stream` (incomplete body) | ✅ **400** (route present + validates, not 404) |

`4 passed, 0 failed` on this machine. Mode 2 forces a deterministically CLI-free
env via `OD_AGENT_HOME=<empty>` (scopes toolchain search to the empty home and
skips system bins), so the "0 available" assertion holds regardless of what's
installed locally.

Unit tests (`cargo test -p rs-design --bins env_inject`, 7 passing) cover the
pure helpers: marker parsing, login-first de-duplicated PATH merge, empty-segment
handling, and the non-clobbering `agentCliEnv` seed.

## GUI-gated remainder (manual, clean desktop session)

These need a real display + the sanitized-env launch the CP1 spikes documented
(the VS Code snap shell crashes GTK on `GLIBC_PRIVATE`):

- **Live `EventSource` chat + todo card in WebKitGTK** — done-criterion (d). The
  SSE *framing* is verified end-to-end in CP2/CP3 (real daemon SSE through axum,
  no buffering/compression, immediate first frame); what remains is observing a
  real chat stream tokens + the pinned TodoCard render live in the window. Drive
  via `cargo tauri dev` with an agent CLI installed (or a BYOK key).
- **BYOK happy-path token stream** — the SSRF guard intentionally *allows*
  loopback (local Ollama) but a real public provider can't be reached offline, so
  the successful token stream needs a live API key. The guard, route presence,
  and CLI-independence are all verified above; only the live upstream round-trip
  is manual.

The login-shell PATH probe runs at supervisor start and logs
`login_path_recovered=true|false`; on a clean desktop session this is where the
GUI-PATH-loss fix is observable (the daemon detects the same CLIs the user's
terminal sees).
