//! Environment injection for the daemon child process (V1 gotcha #1, CP5).
//!
//! Linux GUI apps don't inherit the login-shell PATH, so a GUI-launched daemon
//! can't find `claude`/`codex`/etc. The daemon detects an agent CLI by resolving
//! its binary from **its own `process.env.PATH`** (plus a fixed set of
//! well-known user toolchain dirs) — see the upstream
//! `apps/daemon/src/runtimes/executables.ts`. So the port's job here is purely to
//! hand the daemon a *good* environment; we never reimplement detection.
//!
//! Two **independent** levers, exactly as CLAUDE.md gotcha #1 calls for:
//!
//!   1. **Login-shell PATH reconstruction.** Run the user's login+interactive
//!      shell (`$SHELL -lic 'printf … "$PATH"'`) to recover the PATH their
//!      terminal sees (where `claude` etc. live), then merge it with the GUI
//!      process PATH. Injected as the daemon's `PATH`.
//!   2. **Explicit `*_BIN` overrides.** Honor `CLAUDE_BIN`, `CODEX_BIN`, … taken
//!      from the app's own environment by seeding the daemon's `app-config.json`
//!      `agentCliEnv`, which the daemon consults as a hard override that is
//!      *independent* of PATH (`configuredExecutableOverride`). This works even
//!      when shell reconstruction yields nothing (locked-down shell, exotic
//!      `$SHELL`, CI).
//!
//! The supervisor sets the child env *explicitly* (env-hygiene rule) rather than
//! inheriting the GUI env. This centralization is exactly what V2's direct
//! CLI-spawn (`od-agents`) will reuse.
//!
//! Pure helpers (`parse_marked_path`, `merge_paths`, `collect_bin_overrides`)
//! are unit-tested; the shell exec and the file seed are integration-level.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Markers wrapped around the PATH in the shell probe's stdout so we can pull it
/// out cleanly even when an interactive shell prints prompts/MOTD/job-control
/// noise around it.
const PATH_MARKER_START: &str = "__OD_PATH_START__";
const PATH_MARKER_END: &str = "__OD_PATH_END__";

/// Hard ceiling on the login-shell probe; a hung rc file must not strand startup.
const SHELL_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Agent id → the `*_BIN` env var that overrides PATH detection for it. Mirrors
/// `AGENT_BIN_ENV_KEYS` in the upstream daemon's `executables.ts` so the keys we
/// seed match exactly the keys the daemon reads.
const AGENT_BIN_ENV_KEYS: &[(&str, &str)] = &[
    ("amr", "VELA_BIN"),
    ("aider", "AIDER_BIN"),
    ("claude", "CLAUDE_BIN"),
    ("codebuddy", "CODEBUDDY_BIN"),
    ("codex", "CODEX_BIN"),
    ("copilot", "COPILOT_BIN"),
    ("cursor-agent", "CURSOR_AGENT_BIN"),
    ("deepseek", "DEEPSEEK_BIN"),
    ("devin", "DEVIN_BIN"),
    ("gemini", "GEMINI_BIN"),
    ("hermes", "HERMES_BIN"),
    ("kimi", "KIMI_BIN"),
    ("kiro", "KIRO_BIN"),
    ("kilo", "KILO_BIN"),
    ("opencode", "OPENCODE_BIN"),
    ("pi", "PI_BIN"),
    ("qoder", "QODER_BIN"),
    ("qwen", "QWEN_BIN"),
    ("reasonix", "REASONIX_BIN"),
    ("trae-cli", "TRAE_CLI_BIN"),
    ("vibe", "VIBE_BIN"),
];

/// The environment the daemon child should be launched with, resolved once at
/// supervisor start. `path` is injected as `PATH`; `agent_cli_env` is merged into
/// `app-config.json` before spawn (see [`seed_agent_cli_env`]).
#[derive(Debug, Clone)]
pub struct DaemonEnv {
    /// Merged PATH (login-shell reconstruction ∪ GUI process PATH).
    pub path: OsString,
    /// `agentId -> { ENV_KEY -> value }` collected from explicit `*_BIN` settings.
    pub agent_cli_env: BTreeMap<String, BTreeMap<String, String>>,
    /// Whether login-shell reconstruction actually contributed anything (for logs).
    pub login_path_recovered: bool,
}

/// Resolve the daemon child environment: reconstruct + merge PATH and collect the
/// explicit `*_BIN` overrides. Never fails — a failed shell probe degrades to the
/// GUI process PATH (graceful, as gotcha #1 requires).
pub fn resolve() -> DaemonEnv {
    let gui_path = std::env::var_os("PATH").unwrap_or_default();

    let login_path = if skip_login_shell_probe() {
        None
    } else {
        login_shell_path()
    };

    let (path, login_path_recovered) = match login_path.as_deref() {
        Some(login) if !login.trim().is_empty() => (merge_paths(login, &gui_path), true),
        _ => (gui_path.clone(), false),
    };

    let agent_cli_env = collect_bin_overrides();

    DaemonEnv {
        path,
        agent_cli_env,
        login_path_recovered,
    }
}

/// Honor `OD_SKIP_LOGIN_PATH=1`/`true` to skip the shell probe (CI/tests, or a
/// dev terminal that already carries the full PATH).
fn skip_login_shell_probe() -> bool {
    std::env::var("OD_SKIP_LOGIN_PATH")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true"
        })
        .unwrap_or(false)
}

/// Reconstruct the user's login-shell PATH by running their `$SHELL` as a
/// login+interactive shell. Returns `None` on any failure (no `$SHELL`, exotic
/// shell that doesn't speak `-lic`/`printf`, timeout, marker not found) — callers
/// fall back to the GUI PATH.
fn login_shell_path() -> Option<String> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/bash"));
    run_shell_path_probe(&shell, SHELL_PROBE_TIMEOUT)
}

/// Run `<shell> -lic 'printf <marker>%s<marker> "$PATH"'` with a hard timeout and
/// return the parsed PATH. stderr is discarded (interactive shells without a tty
/// emit job-control warnings that are not errors here).
fn run_shell_path_probe(shell: &OsStr, timeout: Duration) -> Option<String> {
    let script = format!("printf '{PATH_MARKER_START}%s{PATH_MARKER_END}' \"$PATH\"");
    let mut child = Command::new(shell)
        .arg("-lic")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Drain stdout on a thread so a chatty rc file can't deadlock on a full pipe
    // while we poll for exit.
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    let output = reader.join().ok()?;
    parse_marked_path(&output)
}

/// Extract the PATH from the marker-wrapped probe stdout. Returns `None` if the
/// markers are absent (probe failed / shell didn't run our script).
fn parse_marked_path(output: &str) -> Option<String> {
    let start = output.find(PATH_MARKER_START)? + PATH_MARKER_START.len();
    let rest = &output[start..];
    let end = rest.find(PATH_MARKER_END)?;
    let path = &rest[..end];
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// Merge two `:`-separated PATH strings into one, login-shell entries first
/// (they reflect the user's real toolchain), then GUI entries, de-duplicated with
/// order preserved. Empty segments are dropped.
fn merge_paths(login: &str, gui: &OsStr) -> OsString {
    let gui = gui.to_string_lossy();
    let mut seen = std::collections::HashSet::new();
    let mut merged: Vec<&str> = Vec::new();
    for entry in login
        .split(':')
        .chain(gui.split(':'))
        .filter(|e| !e.is_empty())
    {
        if seen.insert(entry) {
            merged.push(entry);
        }
    }
    OsString::from(merged.join(":"))
}

/// Collect explicit `*_BIN` overrides present in the app's own environment into
/// the `agentId -> { ENV_KEY -> value }` shape the daemon's `agentCliEnv` uses.
/// Only non-empty values are taken; the daemon validates that the path is an
/// executable file, so we don't gate on existence here.
fn collect_bin_overrides() -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (agent_id, env_key) in AGENT_BIN_ENV_KEYS {
        if let Some(val) = std::env::var_os(env_key) {
            let val = val.to_string_lossy();
            let val = val.trim();
            if !val.is_empty() {
                out.entry((*agent_id).to_string())
                    .or_default()
                    .insert((*env_key).to_string(), val.to_string());
            }
        }
    }
    out
}

/// Merge the collected `*_BIN` overrides into the daemon's `app-config.json`
/// `agentCliEnv`, **without clobbering** any value the user already set there
/// (Settings UI / a previous run win). No-op when there's nothing to seed.
///
/// `app-config.json` is expected to already exist (the supervisor writes a
/// first-run config before calling this); if it's missing or unparsable we start
/// from an empty object so the override still lands.
pub fn seed_agent_cli_env(
    data_dir: &Path,
    overrides: &BTreeMap<String, BTreeMap<String, String>>,
) -> io::Result<()> {
    if overrides.is_empty() {
        return Ok(());
    }
    let config_path = data_dir.join("app-config.json");
    let mut root: serde_json::Value = std::fs::read(&config_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }

    let obj = root.as_object_mut().expect("root is an object");
    let agent_cli_env = obj
        .entry("agentCliEnv")
        .or_insert_with(|| serde_json::json!({}));
    if !agent_cli_env.is_object() {
        *agent_cli_env = serde_json::json!({});
    }
    let agent_cli_env = agent_cli_env
        .as_object_mut()
        .expect("agentCliEnv is an object");

    let mut seeded: Vec<String> = Vec::new();
    for (agent_id, keys) in overrides {
        let entry = agent_cli_env
            .entry(agent_id.clone())
            .or_insert_with(|| serde_json::json!({}));
        if !entry.is_object() {
            *entry = serde_json::json!({});
        }
        let entry = entry.as_object_mut().expect("agent entry is an object");
        for (env_key, value) in keys {
            // Never clobber an existing user-set value.
            if entry.contains_key(env_key) {
                continue;
            }
            entry.insert(env_key.clone(), serde_json::Value::String(value.clone()));
            seeded.push(format!("{agent_id}.{env_key}"));
        }
    }

    if seeded.is_empty() {
        return Ok(());
    }

    let body = serde_json::to_vec_pretty(&root)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&config_path, body)?;
    tracing::info!(
        path = %config_path.display(),
        seeded = ?seeded,
        "seeded explicit CLI-path overrides into agentCliEnv"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_path_between_markers() {
        let out = format!(
            "some login banner\n{PATH_MARKER_START}/usr/bin:/home/u/.bun/bin{PATH_MARKER_END}\n"
        );
        assert_eq!(
            parse_marked_path(&out).as_deref(),
            Some("/usr/bin:/home/u/.bun/bin")
        );
    }

    #[test]
    fn parse_returns_none_without_markers() {
        assert_eq!(parse_marked_path("bash: no job control\n"), None);
        assert_eq!(parse_marked_path(""), None);
    }

    #[test]
    fn parse_returns_none_for_empty_path() {
        let out = format!("{PATH_MARKER_START}{PATH_MARKER_END}");
        assert_eq!(parse_marked_path(&out), None);
    }

    #[test]
    fn merge_puts_login_first_and_dedups() {
        let login = "/home/u/.local/bin:/usr/bin";
        let gui = OsString::from("/usr/bin:/usr/sbin");
        let merged = merge_paths(login, &gui);
        assert_eq!(
            merged.to_string_lossy(),
            "/home/u/.local/bin:/usr/bin:/usr/sbin"
        );
    }

    #[test]
    fn merge_drops_empty_segments() {
        let login = "/a::/b:";
        let gui = OsString::from(":/c:");
        let merged = merge_paths(login, &gui);
        assert_eq!(merged.to_string_lossy(), "/a:/b:/c");
    }

    #[test]
    fn seed_writes_override_without_clobbering_user_value() {
        let dir = std::env::temp_dir().join(format!("od-cp5-seed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("app-config.json");
        // Pre-existing user choice for claude + telemetry block from first-run.
        std::fs::write(
            &config,
            serde_json::to_vec_pretty(&serde_json::json!({
                "telemetry": { "metrics": false },
                "agentCliEnv": { "claude": { "CLAUDE_BIN": "/user/set/claude" } }
            }))
            .unwrap(),
        )
        .unwrap();

        let mut overrides: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        overrides
            .entry("claude".into())
            .or_default()
            .insert("CLAUDE_BIN".into(), "/seeded/claude".into());
        overrides
            .entry("codex".into())
            .or_default()
            .insert("CODEX_BIN".into(), "/seeded/codex".into());

        seed_agent_cli_env(&dir, &overrides).unwrap();

        let root: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
        // Existing claude value preserved (not clobbered).
        assert_eq!(
            root["agentCliEnv"]["claude"]["CLAUDE_BIN"],
            "/user/set/claude"
        );
        // New codex override seeded.
        assert_eq!(root["agentCliEnv"]["codex"]["CODEX_BIN"], "/seeded/codex");
        // Telemetry block untouched.
        assert_eq!(root["telemetry"]["metrics"], false);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn seed_is_noop_when_no_overrides() {
        let dir = std::env::temp_dir().join(format!("od-cp5-noop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("app-config.json");
        std::fs::write(&config, b"{\"telemetry\":{}}").unwrap();
        let before = std::fs::read(&config).unwrap();

        seed_agent_cli_env(&dir, &BTreeMap::new()).unwrap();

        assert_eq!(std::fs::read(&config).unwrap(), before);
        std::fs::remove_dir_all(&dir).ok();
    }
}
