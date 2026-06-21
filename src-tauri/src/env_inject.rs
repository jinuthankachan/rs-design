//! Environment injection for the daemon child process (V1 gotcha #1).
//!
//! Linux GUI apps don't inherit the login-shell PATH, so the embedded daemon
//! can't find `claude`/`codex`/etc. Two independent levers (CP5):
//!   1. reconstruct the login-shell PATH (e.g. `$SHELL -lic 'env'`), and
//!   2. honor explicit `*_BIN` settings (`CLAUDE_BIN`, `CODEX_BIN`, …).
//!
//! The supervisor sets the child env *explicitly* (PATH, `OD_PORT`,
//! `OD_BIND_HOST`, `OD_DATA_DIR`, provider config) rather than inheriting the
//! GUI env (env-hygiene rule). This centralization is exactly what V2's direct
//! CLI-spawn (`od-agents`) reuses.
//
// TODO(CP5): login-shell PATH reconstruction + *_BIN injection.
