//! State-file utilities — pidfile + XDG path resolution + teardown.
//!
//! Pre-Stage-B these lived in `crates/lifecycle`; after ADR-0013 (tmux drop)
//! the crate no longer makes sense as its own unit — most of its code was
//! the tmux control-mode client which is gone, and what remains is plain
//! OS-level state-file management that belongs to the CLI binary itself.
//!
//! Path conventions (unchanged across Stage B):
//! - pidfile: `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.pid`
//! - token:   `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token`
//! - layout:  `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.layout.json`
//! - config:  `${XDG_CONFIG_HOME:-~/.config}/gtmux/<session>.config.toml`
//!
//! Perm contract (sketch §13.3.6):
//! - parent dir: 0700
//! - pidfile / token: 0600

// `libc::kill` + `libc::geteuid` require unsafe FFI; we explicitly
// allow inside this module (main.rs sets `deny(unsafe_code)` at crate
// level so anything outside this file still trips the lint).
#![allow(unsafe_code)]

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

/// State-file errors, dispatched to the appropriate CLI exit code by
/// `report_start_error` in main.rs.
#[derive(Debug, Error)]
pub enum StateFileError {
    #[error("XDG path resolution failed: {0}")]
    BadXdg(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, StateFileError>;

const PIDFILE_PERM: u32 = 0o600;
const PIDFILE_DIR_PERM: u32 = 0o700;

/// `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.pid`.
pub fn pidfile_path_for(session: &str) -> Result<PathBuf> {
    Ok(state_dir_for_gtmux()?.join(format!("{session}.pid")))
}

/// `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token`.
pub fn token_path_for(session: &str) -> Result<PathBuf> {
    Ok(state_dir_for_gtmux()?.join(format!("{session}.token")))
}

/// `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.layout.json`.
pub fn layout_path_for(session: &str) -> Result<PathBuf> {
    Ok(state_dir_for_gtmux()?.join(format!("{session}.layout.json")))
}

/// `${XDG_CONFIG_HOME:-~/.config}/gtmux/<session>.config.toml`.
pub fn config_path_for(session: &str) -> Result<PathBuf> {
    Ok(config_dir_for_gtmux()?.join(format!("{session}.config.toml")))
}

/// State-dir base — `${XDG_STATE_HOME:-~/.local/state}/gtmux`.
fn state_dir_for_gtmux() -> Result<PathBuf> {
    if let Some(s) = std::env::var_os("XDG_STATE_HOME") {
        let p = PathBuf::from(s);
        if p.as_os_str().is_empty() {
            return Err(StateFileError::BadXdg(
                "XDG_STATE_HOME is set but empty".to_string(),
            ));
        }
        return Ok(p.join("gtmux"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        StateFileError::BadXdg("$HOME not set; cannot resolve XDG_STATE_HOME default".to_string())
    })?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("gtmux"))
}

fn config_dir_for_gtmux() -> Result<PathBuf> {
    if let Some(s) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(s);
        if p.as_os_str().is_empty() {
            return Err(StateFileError::BadXdg(
                "XDG_CONFIG_HOME is set but empty".to_string(),
            ));
        }
        return Ok(p.join("gtmux"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        StateFileError::BadXdg("$HOME not set; cannot resolve XDG_CONFIG_HOME default".to_string())
    })?;
    Ok(PathBuf::from(home).join(".config").join("gtmux"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PidLiveness {
    Absent,
    Alive(libc::pid_t),
    Stale(libc::pid_t),
    Malformed,
}

pub fn check_pidfile_liveness(session: &str) -> Result<PidLiveness> {
    let path = pidfile_path_for(session)?;
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(PidLiveness::Absent),
        Err(e) => return Err(StateFileError::Io(e)),
    };
    let Some(pid) = parse_pid(&raw) else {
        return Ok(PidLiveness::Malformed);
    };
    if pid_is_alive(pid) {
        Ok(PidLiveness::Alive(pid))
    } else {
        Ok(PidLiveness::Stale(pid))
    }
}

pub fn write_pidfile(session: &str) -> Result<PathBuf> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let final_path = pidfile_path_for(session)?;
    let dir = final_path
        .parent()
        .expect("pidfile_path_for always returns a path with a parent");
    ensure_state_dir(dir)?;

    let tmp_path = dir.join(format!(
        "{}.{}.tmp",
        final_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("pid"),
        std::process::id()
    ));

    let write_result = (|| -> io::Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(PIDFILE_PERM)
            .open(&tmp_path)?;
        let perm = fs::Permissions::from_mode(PIDFILE_PERM);
        f.set_permissions(perm)?;
        writeln!(f, "{}", std::process::id())?;
        f.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(StateFileError::Io(e));
    }

    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(StateFileError::Io(e));
    }

    if let Err(e) = fsync_dir(dir) {
        return Err(StateFileError::Io(e));
    }
    Ok(final_path)
}

fn ensure_state_dir(dir: &Path) -> Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    let perm = fs::Permissions::from_mode(PIDFILE_DIR_PERM);
    fs::set_permissions(dir, perm)?;
    Ok(())
}

fn fsync_dir(dir: &Path) -> io::Result<()> {
    let d = std::fs::File::open(dir)?;
    d.sync_all()
}

fn parse_pid(raw: &str) -> Option<libc::pid_t> {
    let line = raw.lines().find(|l| !l.trim().is_empty())?.trim();
    let n: libc::pid_t = line.parse().ok()?;
    if n <= 0 {
        return None;
    }
    Some(n)
}

fn pid_is_alive(pid: libc::pid_t) -> bool {
    // SAFETY: `libc::kill(pid, 0)` is the canonical "probe" — no signal
    // is delivered. The pid was just parsed; sig=0 is a constant.
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    let err = io::Error::last_os_error();
    err.raw_os_error() == Some(libc::EPERM)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopOutcome {
    NoPidfile(PathBuf),
    MalformedPidfile(PathBuf),
    AlreadyDead { pid: libc::pid_t, path: PathBuf },
    Stopped { pid: libc::pid_t, path: PathBuf },
    Killed { pid: libc::pid_t, path: PathBuf },
    TimedOut { pid: libc::pid_t, path: PathBuf },
}

pub async fn stop_server(session: &str, grace: Duration, force_kill: bool) -> Result<StopOutcome> {
    let path = pidfile_path_for(session)?;

    let raw = match tokio::fs::read_to_string(&path).await {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(StopOutcome::NoPidfile(path)),
        Err(e) => return Err(StateFileError::Io(e)),
    };
    let Some(pid) = parse_pid(&raw) else {
        let _ = tokio::fs::remove_file(&path).await;
        return Ok(StopOutcome::MalformedPidfile(path));
    };

    if !pid_is_alive(pid) {
        let _ = tokio::fs::remove_file(&path).await;
        return Ok(StopOutcome::AlreadyDead { pid, path });
    }

    // SAFETY: identical to `pid_is_alive` — kill with a parsed pid +
    // constant signal number.
    let term_rc = unsafe { libc::kill(pid, libc::SIGTERM) };
    if term_rc != 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            let _ = tokio::fs::remove_file(&path).await;
            return Ok(StopOutcome::AlreadyDead { pid, path });
        }
        return Err(StateFileError::Io(err));
    }

    let poll_interval = Duration::from_millis(200);
    let deadline = tokio::time::Instant::now() + grace;
    loop {
        if !pid_is_alive(pid) {
            let _ = tokio::fs::remove_file(&path).await;
            return Ok(StopOutcome::Stopped { pid, path });
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(poll_interval).await;
    }

    if !force_kill {
        return Ok(StopOutcome::TimedOut { pid, path });
    }

    // SAFETY: same as SIGTERM call above.
    let kill_rc = unsafe { libc::kill(pid, libc::SIGKILL) };
    if kill_rc != 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            let _ = tokio::fs::remove_file(&path).await;
            return Ok(StopOutcome::Killed { pid, path });
        }
        return Err(StateFileError::Io(err));
    }
    let kill_deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while tokio::time::Instant::now() < kill_deadline {
        if !pid_is_alive(pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = tokio::fs::remove_file(&path).await;
    Ok(StopOutcome::Killed { pid, path })
}

// ─── Teardown — ADR-0014 D7 (4-step, no tmux kill-server) ───────────────────

#[derive(Debug, Clone)]
pub struct TeardownOpts {
    /// Skip the interactive confirmation prompt — required when stderr is
    /// not a TTY (CI / pipes).
    pub force: bool,
    /// Remove `<session>.{pid,token,layout.json}` from state dir.
    pub remove_state_files: bool,
    /// Remove `<session>.config.toml` from config dir.
    pub remove_config: bool,
}

impl Default for TeardownOpts {
    fn default() -> Self {
        Self {
            force: false,
            remove_state_files: true,
            remove_config: true,
        }
    }
}

#[derive(Debug, Default)]
pub struct TeardownReport {
    /// Result of step 1 (SIGTERM the pidfile process).
    pub stop: Option<StopOutcome>,
    /// Per-file unlink results: `Ok(true)` = removed, `Ok(false)` =
    /// already absent, `Err(msg)` = best-effort warning.
    pub removed: Vec<(String, std::result::Result<bool, String>)>,
    /// Anything we want to print at WARN but not fail on.
    pub warnings: Vec<String>,
}

pub async fn teardown(session: &str, opts: TeardownOpts) -> Result<TeardownReport> {
    let mut report = TeardownReport::default();

    // Step 1 — graceful SIGTERM to the foreground server. If it is still
    // running, our 4-step is moot until it exits.
    let stop = stop_server(session, Duration::from_secs(5), opts.force).await?;
    report.stop = Some(stop);

    if opts.remove_state_files {
        for (kind, path) in [
            ("pidfile", pidfile_path_for(session)?),
            ("token", token_path_for(session)?),
            ("layout", layout_path_for(session)?),
        ] {
            let result = remove_state_file(&path, kind).await;
            report.removed.push((kind.into(), result));
        }
    }
    if opts.remove_config {
        let path = config_path_for(session)?;
        let result = remove_state_file(&path, "config").await;
        report.removed.push(("config".into(), result));
    }

    Ok(report)
}

async fn remove_state_file(path: &Path, kind: &str) -> std::result::Result<bool, String> {
    use std::os::unix::fs::PermissionsExt;

    let meta = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("{kind} stat failed at {}: {e}", path.display())),
    };
    if kind == "token" {
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o600 {
            return tokio::fs::remove_file(path)
                .await
                .map(|()| true)
                .map_err(|e| {
                    format!(
                        "{kind} unlink failed at {}: {e} (had perm {:o}, expected 0600)",
                        path.display(),
                        mode
                    )
                });
        }
    }
    tokio::fs::remove_file(path)
        .await
        .map(|()| true)
        .map_err(|e| format!("{kind} unlink failed at {}: {e}", path.display()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parse_pid_zero_rejected() {
        assert_eq!(parse_pid("0\n"), None);
    }

    #[test]
    fn parse_pid_negative_rejected() {
        assert_eq!(parse_pid("-1\n"), None);
    }

    #[test]
    fn parse_pid_positive_ok() {
        assert_eq!(parse_pid("42\n"), Some(42));
    }

    #[test]
    fn parse_pid_skips_blank_lines() {
        assert_eq!(parse_pid("\n\n  \n123\n"), Some(123));
    }

    #[test]
    fn pid_is_alive_self() {
        // Our own PID is always alive while this test runs.
        let me = std::process::id() as libc::pid_t;
        assert!(pid_is_alive(me));
    }
}
