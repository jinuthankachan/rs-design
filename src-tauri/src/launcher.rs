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

use std::ffi::OsString;
use std::io;
use std::os::unix::fs::PermissionsExt;
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
    /// PATH injected into the daemon child (CP5 env_inject: login-shell
    /// reconstruction ∪ GUI PATH) so the daemon's PATH-scan finds the agent CLIs.
    daemon_path: OsString,
}

impl DevNodeLauncher {
    /// Resolve the dev launch paths, or fail with an actionable error.
    /// `daemon_path` is the CP5-injected PATH the daemon child runs with; it is
    /// also what we scan for `node` so resolution and spawn stay symmetric.
    pub fn resolve(daemon_path: OsString) -> io::Result<Self> {
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
        let node_bin = resolve_node_bin(&daemon_path)?;
        Ok(Self {
            node_bin,
            cli_js,
            content_root,
            daemon_path,
        })
    }
}

impl DaemonLauncher for DevNodeLauncher {
    fn command(&self, port: u16, data_dir: &Path) -> Command {
        // Explicit child env (env-hygiene rule) via the shared builder: clear
        // everything, then set only HOME + the CP5-injected `daemon_path` (login
        // -shell reconstruction ∪ GUI PATH, so the daemon's PATH-scan finds agent
        // CLIs under GUI launch) + the `OD_*` vars. The daemon *core*
        // (HTTP/SSE/SQLite/catalog) needs no PATH; explicit `*_BIN` overrides ride
        // in via the seeded `agentCliEnv` (env_inject::seed_agent_cli_env).
        configure_daemon_command(
            &self.node_bin,
            &self.cli_js,
            &self.content_root,
            &self.daemon_path,
            port,
            data_dir,
        )
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

/// Packaged launcher (CP6): the bundled Node 24 + the pruned daemon bundle laid
/// out by `scripts/build-daemon-bundle.sh` under the Tauri resource dir
/// (`<resource>/runtime/`). The launch contract is identical to
/// [`DevNodeLauncher`] — only the resolved paths differ — so the supervisor's
/// lifecycle code is unchanged.
///
/// The runtime layout (see the build script) makes the daemon's own path
/// resolution land on one root: `runtime/apps/daemon/dist` → `PROJECT_ROOT =
/// runtime` → `STATIC_DIR = runtime/apps/web/out`, with `OD_RESOURCE_ROOT =
/// OD_INSTALLATION_DIR = runtime` for the catalog content.
pub struct BundledLauncher {
    /// Bundled Node, **materialized into the (writable) data dir** — see
    /// [`materialize_node`]. Resources are mounted read-only in an AppImage and
    /// Tauri's resource copy can drop the exec bit, so we copy `node` out and
    /// `chmod +x` it where we control the filesystem.
    node_bin: PathBuf,
    cli_js: PathBuf,
    content_root: PathBuf,
    /// PATH injected into the daemon child (CP5 env_inject), identical contract
    /// to [`DevNodeLauncher`].
    daemon_path: OsString,
}

impl BundledLauncher {
    /// Resolve the packaged launch paths from the Tauri resource dir, or fail if
    /// the bundle isn't present (the caller then falls back to [`DevNodeLauncher`]
    /// for `cargo tauri dev`). `runtime` is `<resource_dir>/runtime`.
    pub fn resolve(runtime: &Path, data_dir: &Path, daemon_path: OsString) -> io::Result<Self> {
        let cli_js = runtime.join("apps/daemon/dist/cli.js");
        if !cli_js.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("bundled daemon entry not found at {}", cli_js.display()),
            ));
        }
        let src_node = runtime.join("node");
        if !src_node.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("bundled node not found at {}", src_node.display()),
            ));
        }
        let node_bin = materialize_node(&src_node, data_dir)?;
        Ok(Self {
            node_bin,
            cli_js,
            content_root: runtime.to_path_buf(),
            daemon_path,
        })
    }
}

impl DaemonLauncher for BundledLauncher {
    fn command(&self, port: u16, data_dir: &Path) -> Command {
        configure_daemon_command(
            &self.node_bin,
            &self.cli_js,
            &self.content_root,
            &self.daemon_path,
            port,
            data_dir,
        )
    }

    fn describe(&self) -> String {
        format!(
            "BundledLauncher(node={}, entry={})",
            self.node_bin.display(),
            self.cli_js.display()
        )
    }

    fn content_root(&self) -> PathBuf {
        self.content_root.clone()
    }
}

/// Copy the bundled `node` into the writable data dir and mark it executable.
/// Idempotent: re-copies only when the destination is missing or its size differs
/// from the source (a cheap proxy for a Node/version bump — avoids a ~101 M copy
/// on every launch). Returns the path to the materialized, executable binary.
fn materialize_node(src: &Path, data_dir: &Path) -> io::Result<PathBuf> {
    let bin_dir = data_dir.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let dst = bin_dir.join("node");

    let needs_copy = match (std::fs::metadata(&dst), std::fs::metadata(src)) {
        (Ok(d), Ok(s)) => d.len() != s.len(),
        _ => true,
    };
    if needs_copy {
        std::fs::copy(src, &dst)?;
        tracing::info!(src = %src.display(), dst = %dst.display(), "materialized bundled node");
    }
    // Always ensure the exec bit (cheap; corrects a perm-stripped prior copy).
    let mut perms = std::fs::metadata(&dst)?.permissions();
    if perms.mode() & 0o111 != 0o111 {
        perms.set_mode(0o755);
        std::fs::set_permissions(&dst, perms)?;
    }
    Ok(dst)
}

/// Shared command builder for both launchers — the launch contract the CP1
/// daemon-packaging spike locked. Clears the env (env-hygiene rule), then sets
/// only HOME + the CP5-injected PATH + the `OD_*` vars the daemon needs.
fn configure_daemon_command(
    node_bin: &Path,
    cli_js: &Path,
    content_root: &Path,
    daemon_path: &OsString,
    port: u16,
    data_dir: &Path,
) -> Command {
    let port_str = port.to_string();
    let mut cmd = Command::new(node_bin);
    cmd.arg(cli_js)
        .arg("daemon")
        .arg("start")
        .arg("--headless")
        .arg("--port")
        .arg(&port_str)
        .arg("--host")
        .arg("127.0.0.1");

    cmd.env_clear();
    if let Some(home) = std::env::var_os("HOME") {
        cmd.env("HOME", home);
    }
    cmd.env("PATH", daemon_path);
    cmd.env("OD_PORT", &port_str);
    cmd.env("OD_BIND_HOST", "127.0.0.1");
    cmd.env("OD_DATA_DIR", data_dir);
    // OD_RESOURCE_ROOT is rejected unless it sits under OD_INSTALLATION_DIR
    // (server.ts safe-base check); point both at the content root.
    cmd.env("OD_INSTALLATION_DIR", content_root);
    cmd.env("OD_RESOURCE_ROOT", content_root);

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    cmd
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

/// Locate the `node` binary. `OD_NODE_BIN` overrides; otherwise scan the
/// CP5-injected daemon PATH (login-shell reconstruction ∪ GUI PATH), so node is
/// found even under GUI launch where the bare process PATH is minimal. Returns an
/// absolute path so the daemon is invoked exactly as the packaging spike validated.
fn resolve_node_bin(daemon_path: &std::ffi::OsStr) -> io::Result<PathBuf> {
    if let Some(p) = std::env::var_os("OD_NODE_BIN") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Ok(pb);
        }
    }
    for dir in std::env::split_paths(daemon_path) {
        let cand = dir.join("node");
        if cand.is_file() {
            return Ok(cand);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "could not find `node` on PATH; install Node 24 or set OD_NODE_BIN",
    ))
}

/// Data root for the daemon (`OD_DATA_DIR`). Kept out of the repo and the
/// submodule. `OD_DATA_DIR` in the environment wins; otherwise XDG data home
/// (`$XDG_DATA_HOME` or `~/.local/share`). The packaged app uses
/// `rs-design/` directly; the dev loop uses `rs-design/dev` so a packaged install
/// and `cargo tauri dev` never share a SQLite store.
pub fn data_dir(packaged: bool) -> PathBuf {
    if let Some(p) = std::env::var_os("OD_DATA_DIR") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    if packaged {
        base.join("rs-design")
    } else {
        base.join("rs-design/dev")
    }
}
