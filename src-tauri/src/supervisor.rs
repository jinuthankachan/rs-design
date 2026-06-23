//! Supervisor — owns the embedded backend lifecycle.
//!
//! - **CP2:** pick two ephemeral loopback ports and start the `od-server` axum
//!   app on one (the webview-facing endpoint), proxying 100% to the daemon.
//! - **CP3 (this file):** spawn + health-check the Node daemon on the reserved
//!   port (wait for the stdout `[od] listening` line **and** poll
//!   `GET /api/skills`), pipe its stdout/stderr to `tracing`, restart it on
//!   crash (bounded), kill it on shutdown, and seed a first-run config with
//!   telemetry disabled. Ports the known-good launch contract from upstream
//!   `vendor/open-design/apps/desktop/src/main/runtime.ts` + the CP1
//!   daemon-packaging spike.
//!
//! ## Two ephemeral loopback ports
//!
//! Both ports are OS-assigned (`bind(127.0.0.1:0)`) so two instances never
//! collide and nothing leaks off-host. The axum listener is bound *synchronously*
//! up front so the port is known the instant we build the webview window. The
//! daemon port is reserved by binding then immediately dropping the listener; the
//! daemon spawn (which `--port`-binds it for real) is kicked off promptly after,
//! minimizing the bind-then-drop TOCTOU window.

use std::io;
use std::net::{Ipv4Addr, TcpListener as StdTcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStderr, ChildStdout};
use tokio::sync::{oneshot, watch};
use tokio::time::sleep;

use crate::launcher::{self, DaemonLauncher, DevNodeLauncher};

/// Overall ceiling on becoming ready (listening line + first `/api/skills` 200).
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to keep polling `/api/skills` after the listening line appears.
const SKILLS_POLL_TIMEOUT: Duration = Duration::from_secs(10);
/// Bounded restart-on-crash: give up after this many consecutive crashes.
const MAX_RESTARTS: u32 = 5;

/// Readiness of the embedded daemon, published over a watch channel.
#[derive(Clone, Debug)]
pub enum DaemonStatus {
    /// Spawned (or about to be) but not yet serving.
    Starting,
    /// `[od] listening` seen **and** `GET /api/skills` returned 200.
    Ready,
    /// Failed to come up (spawn error, readiness timeout, or too many crashes).
    Failed(String),
}

/// Handle to the running backend. Held in Tauri state so the window-close
/// handler can trigger graceful shutdown.
pub struct Supervisor {
    /// Webview-facing axum origin, e.g. `http://127.0.0.1:54321`.
    pub axum_url: String,
    /// Port the embedded daemon binds (kept for diagnostics; CP5 surfaces it).
    #[allow(dead_code)]
    pub daemon_port: u16,
    shutdown_tx: watch::Sender<bool>,
    ready_rx: watch::Receiver<DaemonStatus>,
}

impl Supervisor {
    /// Signal both the daemon lifecycle task and the axum server to shut down
    /// gracefully (idempotent).
    pub fn shutdown(&self) {
        tracing::info!("supervisor: graceful shutdown requested");
        let _ = self.shutdown_tx.send(true);
    }

    /// Block until the daemon is [`DaemonStatus::Ready`] / [`DaemonStatus::Failed`],
    /// or `timeout` elapses (returns `Failed` then). Used by `setup` to gate the
    /// window on a reachable backend while still surfacing failures.
    pub async fn wait_ready(&self, timeout: Duration) -> DaemonStatus {
        let mut rx = self.ready_rx.clone();
        let wait = async {
            loop {
                let status = rx.borrow_and_update().clone();
                match status {
                    DaemonStatus::Starting => {}
                    ready_or_failed => return ready_or_failed,
                }
                if rx.changed().await.is_err() {
                    return DaemonStatus::Failed("supervisor task dropped".into());
                }
            }
        };
        match tokio::time::timeout(timeout, wait).await {
            Ok(status) => status,
            Err(_) => DaemonStatus::Failed("daemon readiness timed out".into()),
        }
    }
}

/// Start the axum server on an ephemeral loopback port and spawn + health-check
/// the embedded Node daemon on the reserved daemon port. Returns immediately with
/// a [`Supervisor`]; call [`Supervisor::wait_ready`] to await the backend.
pub fn start() -> io::Result<Supervisor> {
    let data_dir = launcher::dev_data_dir();
    ensure_first_run_config(&data_dir)?;
    let launcher: Arc<dyn DaemonLauncher> = Arc::new(DevNodeLauncher::resolve()?);
    tracing::info!(launcher = %launcher.describe(), data_dir = %data_dir.display(), "daemon launcher resolved");

    // Reserve the daemon's port (the spawn below binds it for real).
    let daemon_port = reserve_ephemeral_port()?;
    let daemon_url = format!("http://{}:{}", Ipv4Addr::LOCALHOST, daemon_port);

    // Bind the webview-facing listener now so the port is known immediately.
    let std_listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    std_listener.set_nonblocking(true)?;
    let axum_port = std_listener.local_addr()?.port();
    let axum_url = format!("http://{}:{}", Ipv4Addr::LOCALHOST, axum_port);

    // CP4 route table: the three `od-catalog` reads are served natively from the
    // same content the daemon reads (launcher's content root + data dir);
    // everything else — and every non-GET method on those paths — proxies to the
    // daemon. `OD_FORCE_PROXY` is the contract's rollback lever: set it to revert
    // the catalog routes to the daemon without changing anything else.
    let catalog_roots = od_catalog::CatalogRoots::new(launcher.content_root(), &data_dir);
    let force_proxy = force_proxy_requested();
    if force_proxy {
        tracing::warn!("OD_FORCE_PROXY set: native catalog routes disabled, proxying to daemon");
    }
    let router = od_server::router_with_catalog(daemon_url.clone(), catalog_roots, force_proxy);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (ready_tx, ready_rx) = watch::channel(DaemonStatus::Starting);

    // Daemon lifecycle task: spawn promptly to minimize the reserve→bind window.
    {
        let launcher = launcher.clone();
        let data_dir = data_dir.clone();
        let shutdown_rx = shutdown_rx.clone();
        let daemon_url = daemon_url.clone();
        tauri::async_runtime::spawn(async move {
            supervise(
                launcher,
                daemon_port,
                daemon_url,
                data_dir,
                ready_tx,
                shutdown_rx,
            )
            .await;
        });
    }

    // axum server task.
    {
        let mut shutdown_rx = shutdown_rx.clone();
        let serve_url = axum_url.clone();
        tauri::async_runtime::spawn(async move {
            let listener = match tokio::net::TcpListener::from_std(std_listener) {
                Ok(listener) => listener,
                Err(err) => {
                    tracing::error!(error = %err, "od-server: failed to adopt listener");
                    return;
                }
            };
            tracing::info!(axum = %serve_url, daemon = %daemon_url, "od-server listening (CP3: proxy → embedded daemon)");
            let server = axum::serve(listener, router).with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
            });
            if let Err(err) = server.await {
                tracing::error!(error = %err, "od-server exited with error");
            } else {
                tracing::info!("od-server stopped");
            }
        });
    }

    Ok(Supervisor {
        axum_url,
        daemon_port,
        shutdown_tx,
        ready_rx,
    })
}

/// Outcome of running a single daemon process instance to completion.
enum RunOutcome {
    /// Shutdown was requested; the child was killed.
    Shutdown,
    /// The process exited on its own (crash, or never became ready).
    Ended,
}

/// Supervise the daemon: (re)spawn, health-check, restart on crash (bounded),
/// kill on shutdown.
async fn supervise(
    launcher: Arc<dyn DaemonLauncher>,
    port: u16,
    daemon_url: String,
    data_dir: PathBuf,
    ready_tx: watch::Sender<DaemonStatus>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut restarts: u32 = 0;
    loop {
        if *shutdown_rx.borrow() {
            return;
        }
        match run_once(
            &*launcher,
            port,
            &daemon_url,
            &data_dir,
            &ready_tx,
            &mut shutdown_rx,
        )
        .await
        {
            RunOutcome::Shutdown => {
                tracing::info!("daemon: stopped (shutdown)");
                return;
            }
            RunOutcome::Ended => {
                restarts += 1;
                if restarts > MAX_RESTARTS {
                    let msg = format!("daemon exited {restarts} times; giving up");
                    tracing::error!("{msg}");
                    let _ = ready_tx.send(DaemonStatus::Failed(msg));
                    return;
                }
                let backoff = Duration::from_millis(500 * u64::from(restarts));
                tracing::warn!(restarts, ?backoff, "daemon: restarting after exit");
                let _ = ready_tx.send(DaemonStatus::Starting);
                tokio::select! {
                    _ = sleep(backoff) => {}
                    _ = shutdown_rx.changed() => return,
                }
            }
        }
    }
}

/// Spawn one daemon instance and drive it until it exits or shutdown is
/// requested. Publishes [`DaemonStatus::Ready`]/`Failed` on the readiness
/// channel as soon as the outcome is known.
async fn run_once(
    launcher: &dyn DaemonLauncher,
    port: u16,
    daemon_url: &str,
    data_dir: &Path,
    ready_tx: &watch::Sender<DaemonStatus>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> RunOutcome {
    let mut child = match launcher.command(port, data_dir).spawn() {
        Ok(child) => child,
        Err(err) => {
            let msg = format!("failed to spawn daemon: {err}");
            tracing::error!("{msg}");
            let _ = ready_tx.send(DaemonStatus::Failed(msg));
            // Treat as an ended instance so the restart backoff applies.
            return RunOutcome::Ended;
        }
    };
    tracing::info!(pid = child.id(), port, "daemon: spawned");

    // Pipe stdout/stderr to tracing; signal when the listening line appears.
    let (listen_tx, listen_rx) = oneshot::channel::<()>();
    if let Some(stdout) = child.stdout.take() {
        pipe_stdout(stdout, listen_tx);
    }
    if let Some(stderr) = child.stderr.take() {
        pipe_stderr(stderr);
    }

    let readiness = readiness(daemon_url, listen_rx);
    tokio::pin!(readiness);
    let mut settled = false;

    loop {
        tokio::select! {
            // Readiness resolves once: listening line + `/api/skills` 200.
            result = &mut readiness, if !settled => {
                settled = true;
                match result {
                    Ok(()) => {
                        tracing::info!("daemon: ready (catalog reachable)");
                        let _ = ready_tx.send(DaemonStatus::Ready);
                    }
                    Err(err) => {
                        tracing::error!(error = %err, "daemon: readiness check failed");
                        let _ = ready_tx.send(DaemonStatus::Failed(err));
                    }
                }
            }
            // Hard ceiling on readiness so a hung boot can't strand the splash.
            _ = sleep(READY_TIMEOUT), if !settled => {
                settled = true;
                let msg = "daemon did not become ready within timeout".to_string();
                tracing::error!("{msg}");
                let _ = ready_tx.send(DaemonStatus::Failed(msg));
            }
            status = child.wait() => {
                match status {
                    Ok(code) => tracing::error!(?code, "daemon: process exited"),
                    Err(err) => tracing::error!(error = %err, "daemon: wait failed"),
                }
                return RunOutcome::Ended;
            }
            _ = shutdown_rx.changed() => {
                tracing::info!("daemon: killing on shutdown");
                let _ = child.kill().await;
                return RunOutcome::Shutdown;
            }
        }
    }
}

/// Await readiness: first the stdout `[od] listening` line (or the process dies,
/// dropping the sender), then poll `GET /api/skills` until it returns 200.
async fn readiness(daemon_url: &str, listen_rx: oneshot::Receiver<()>) -> Result<(), String> {
    listen_rx
        .await
        .map_err(|_| "daemon exited before it started listening".to_string())?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("http client build failed: {e}"))?;
    let url = format!("{daemon_url}/api/skills");
    let deadline = Instant::now() + SKILLS_POLL_TIMEOUT;
    loop {
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(
                "`GET /api/skills` did not return 200 after the listening line".to_string(),
            );
        }
        sleep(Duration::from_millis(150)).await;
    }
}

/// Forward daemon stdout to `tracing` (target `daemon`) and fire `listen_tx`
/// once, on the first `[od] listening` line.
fn pipe_stdout(stdout: ChildStdout, listen_tx: oneshot::Sender<()>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut listen_tx = Some(listen_tx);
        while let Ok(Some(line)) = lines.next_line().await {
            if listen_tx.is_some() && line.contains("[od] listening on") {
                if let Some(tx) = listen_tx.take() {
                    let _ = tx.send(());
                }
            }
            tracing::info!(target: "daemon", "{line}");
        }
    });
}

/// Forward daemon stderr to `tracing` (target `daemon`) at warn level.
fn pipe_stderr(stderr: ChildStderr) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::warn!(target: "daemon", "{line}");
        }
    });
}

/// Seed a first-run `app-config.json` with telemetry **disabled**. The daemon
/// defaults telemetry to opt-in (`metrics: true`) when the `telemetry` key is
/// absent, so writing it explicitly is what makes the embedded app private by
/// default. No-op if the file already exists (never clobbers user choices).
fn ensure_first_run_config(data_dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let config_path = data_dir.join("app-config.json");
    if config_path.exists() {
        return Ok(());
    }
    let config = serde_json::json!({
        "telemetry": { "metrics": false, "content": false, "artifactManifest": false }
    });
    let body = serde_json::to_vec_pretty(&config)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&config_path, body)?;
    tracing::info!(path = %config_path.display(), "wrote first-run config (telemetry disabled)");
    Ok(())
}

/// Whether the `--force-proxy` rollback lever is engaged, read from the
/// `OD_FORCE_PROXY` env var (`1`/`true`, case-insensitive). The app is a GUI
/// process with no CLI surface, so the contract's `--force-proxy` is exposed as
/// an env toggle here.
fn force_proxy_requested() -> bool {
    std::env::var("OD_FORCE_PROXY")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true"
        })
        .unwrap_or(false)
}

/// Bind an ephemeral loopback port, read its number, and release it so the
/// daemon spawn can `--port`-bind it.
fn reserve_ephemeral_port() -> io::Result<u16> {
    let listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}
