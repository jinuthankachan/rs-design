//! DaemonLauncher — how the Node daemon sidecar is started.
//!
//! - [`DevNodeLauncher`] (CP3): runs the daemon from `vendor/open-design` with
//!   the system Node, for the `cargo tauri dev` loop. Node is invoked by
//!   absolute path; the child env is set **explicitly** (env-hygiene rule) so
//!   the daemon never depends on inherited GUI env.
//! - `BundledLauncher` (CP6): runs the pruned daemon bundle with a bundled
//!   Node 24 resolved from Tauri resources. Not yet implemented — the trait
//!   boundary is what lets CP6 add it without touching the supervisor.
//!
//! The launch contract (flags + env) is the one the CP1 daemon-packaging spike
//! locked: `node dist/cli.js daemon start --headless --port <p> --host
//! 127.0.0.1`, with `OD_PORT`/`OD_BIND_HOST`/`OD_DATA_DIR` plus
//! `OD_INSTALLATION_DIR`+`OD_RESOURCE_ROOT` for the content root (the latter is
//! rejected unless it sits under the former — the spike's safe-base gotcha).

use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

/// Abstraction over how the Node daemon sidecar is started. CP3 ships
/// [`DevNodeLauncher`]; CP6 adds a `BundledLauncher`. The supervisor depends
/// only on this trait, so V2's eventual "delete the sidecar" step removes an
/// implementation rather than rewriting the lifecycle code.
pub trait DaemonLauncher: Send + Sync + 'static {
    /// Build a spawnable command for the daemon bound to `port`, persisting data
    /// under `data_dir`. Stdout/stderr are piped (the supervisor reads the
    /// `[od] listening` line and forwards every line to `tracing`); the child is
    /// killed if its `Child` is dropped.
    fn command(&self, port: u16, data_dir: &Path) -> Command;

    /// Human-readable one-liner for logs (e.g. the resolved node + entry paths).
    fn describe(&self) -> String;

    /// Root of the content tree this launcher serves (`skills/`,
    /// `design-systems/`, `design-templates/`, the web `out/`). The supervisor
    /// uses it to build the CP4 native catalog roots. For [`DevNodeLauncher`]
    /// this is the vendored submodule; CP6's `BundledLauncher` will return the
    /// packaged resource root, so the native routes read the same content the
    /// daemon does.
    fn content_root(&self) -> PathBuf;
}

/// Dev-loop launcher: system Node + the `tsc`-built daemon entry living in the
/// `vendor/open-design` submodule. Resolved once at startup so a missing build
/// or missing `node` is surfaced before the window opens.
pub struct DevNodeLauncher {
    node_bin: PathBuf,
    cli_js: PathBuf,
    content_root: PathBuf,
}

impl DevNodeLauncher {
    /// Resolve the dev launch paths, or fail with an actionable error.
    pub fn resolve() -> io::Result<Self> {
        let content_root = dev_content_root();
        let cli_js = content_root.join("apps/daemon/dist/cli.js");
        if !cli_js.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "daemon entry not built at {}: run `pnpm --filter @open-design/daemon build` \
                     (or set OD_CONTENT_ROOT)",
                    cli_js.display()
                ),
            ));
        }
        let node_bin = resolve_node_bin()?;
        Ok(Self {
            node_bin,
            cli_js,
            content_root,
        })
    }
}

impl DaemonLauncher for DevNodeLauncher {
    fn command(&self, port: u16, data_dir: &Path) -> Command {
        let port_str = port.to_string();

        let mut cmd = Command::new(&self.node_bin);
        cmd.arg(&self.cli_js)
            .arg("daemon")
            .arg("start")
            .arg("--headless")
            .arg("--port")
            .arg(&port_str)
            .arg("--host")
            .arg("127.0.0.1");

        // Explicit child env (env-hygiene rule): clear everything, then set only
        // what the daemon needs. CP5 replaces this dev PATH with login-shell
        // reconstruction + explicit `*_BIN` so agent-CLI spawn works under GUI
        // launch; the daemon *core* (HTTP/SSE/SQLite/catalog) needs no PATH.
        cmd.env_clear();
        if let Some(home) = std::env::var_os("HOME") {
            cmd.env("HOME", home);
        }
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        }
        cmd.env("OD_PORT", &port_str);
        cmd.env("OD_BIND_HOST", "127.0.0.1");
        cmd.env("OD_DATA_DIR", data_dir);
        // OD_RESOURCE_ROOT is rejected unless it sits under OD_INSTALLATION_DIR
        // (server.ts safe-base check). Point both at the submodule root so the
        // daemon finds skills/, design-systems/, frames/, and the web `out/`.
        cmd.env("OD_INSTALLATION_DIR", &self.content_root);
        cmd.env("OD_RESOURCE_ROOT", &self.content_root);

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.kill_on_drop(true);
        cmd
    }

    fn describe(&self) -> String {
        format!(
            "DevNodeLauncher(node={}, entry={})",
            self.node_bin.display(),
            self.cli_js.display()
        )
    }

    fn content_root(&self) -> PathBuf {
        self.content_root.clone()
    }
}

/// Root of the vendored upstream monorepo. `OD_CONTENT_ROOT` overrides it;
/// otherwise it is derived from the compile-time crate dir (`src-tauri/..`),
/// which is correct for the dev loop where the binary runs from this checkout.
fn dev_content_root() -> PathBuf {
    if let Some(p) = std::env::var_os("OD_CONTENT_ROOT") {
        return PathBuf::from(p);
    }
    // env!("CARGO_MANIFEST_DIR") = <repo>/src-tauri
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|repo| repo.join("vendor/open-design"))
        .unwrap_or_else(|| PathBuf::from("vendor/open-design"))
}

/// Locate the `node` binary. `OD_NODE_BIN` overrides; otherwise scan `PATH`
/// (inherited in the dev terminal). Returns an absolute path so the daemon is
/// invoked exactly as the packaging spike validated.
fn resolve_node_bin() -> io::Result<PathBuf> {
    if let Some(p) = std::env::var_os("OD_NODE_BIN") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Ok(pb);
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let cand = dir.join("node");
            if cand.is_file() {
                return Ok(cand);
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "could not find `node` on PATH; install Node 24 or set OD_NODE_BIN",
    ))
}

/// Dev data root for the daemon (`OD_DATA_DIR`). Kept out of the repo and the
/// submodule. `OD_DATA_DIR` in the environment wins; otherwise XDG data home.
pub fn dev_data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("OD_DATA_DIR") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("rs-design/dev")
}
