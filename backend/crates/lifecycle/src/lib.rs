//! gtmux-lifecycle — dedicated tmux daemon spawn / attach / shutdown +
//! stale socket cleanup helper.
//!
//! Implements ADR-0009 (tmux daemon isolation) D2/D3 (spawn + socket path)
//! and the first three steps of D4 (cleanup helper: stat → ping → unlink).
//! D4 steps 4 and 5 — token / layout / pid / config file removal — belong to
//! the `teardown` CLI subcommand, not this crate. ADR-0001 D1·D11 commit the
//! control-mode entrypoint argv (`tmux -L gtmux-<session> -S <socket> -C ...`)
//! that this module reifies into [`TmuxDaemon::spawn`] / [`TmuxDaemon::attach`].
//!
//! D21 c2 (Grill report `0010-grill-amendments.md`) requires port-based
//! reattach to succeed against an already-running daemon. We split that into
//! two surfaces:
//!   * [`TmuxDaemon::spawn`] — uses `new-session -A` so an existing session is
//!     re-attached transparently while still owning a child handle (only the
//!     first concurrent spawner does); a second spawner gets a child that
//!     exits cleanly after detaching and is dropped.
//!   * [`TmuxDaemon::attach`] — pure attach path with `child = None` for the
//!     CLI `start --port <N>` lookup branch where the daemon was started in a
//!     prior run.

// SAFETY policy: this crate calls `libc::kill` and `libc::geteuid` for signal
// delivery and uid lookup. Both are wrapped in narrow `unsafe` blocks with
// inline SAFETY notes. We intentionally do *not* `forbid(unsafe_code)` — the
// FFI is load-bearing for clean SIGTERM-then-SIGKILL shutdown.
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all)]

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bytes::Bytes;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tracing::{debug, warn};

/// Errors surfaced by the lifecycle crate. Every variant is fail-closed:
/// callers must not retry without re-running the precondition gates
/// (perm check, socket existence, etc.).
#[derive(Debug, Error)]
pub enum LifecycleError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// `tmux` executable not on `$PATH` or not executable. Surfaced when
    /// `Command::spawn` returns `ErrorKind::NotFound`.
    #[error("tmux binary not found on PATH (install tmux >= 3.2)")]
    TmuxNotFound,

    /// `tmux` spawn failed for a reason other than missing binary — usually
    /// permission, exec format, or the kernel refusing the fork.
    #[error("tmux spawn failed: {0}")]
    TmuxSpawn(String),

    /// `new-session` without `-A` would fail because the session already
    /// exists. Currently unused (we always pass `-A`) but reserved so the
    /// CLI can switch strategies in future without an enum-shape break.
    #[error("tmux session '{0}' already exists")]
    SessionAlreadyExists(String),

    /// Cleanup target's daemon is still alive (`has-session` returned 0).
    /// The caller must explicitly tear the server down before unlinking.
    #[error("socket {0} is in use by a live tmux daemon")]
    SocketBusy(PathBuf),

    /// Either stdin or stdout of the spawned tmux child was not piped —
    /// this is a programmer bug in [`TmuxDaemon::spawn`] / [`attach`].
    #[error("control-mode stdio missing (stdin or stdout was not piped)")]
    ProtocolError,

    /// `XDG_RUNTIME_DIR` was set but did not resolve to an absolute path,
    /// or the fallback `/tmp/gtmux-<uid>` could not be derived (uid lookup
    /// failure is reported via this variant for callers' simplicity).
    #[error("XDG_RUNTIME_DIR resolution failed: {0}")]
    BadXdg(String),
}

pub type Result<T> = std::result::Result<T, LifecycleError>;

/// Caller-supplied parameters for [`TmuxDaemon::spawn`] / [`attach`].
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// tmux session name. Must match the `-s` argv slot exactly — argv
    /// separation prevents shell-level injection (ADR-0001 D12 + sketch
    /// §13.3.3).
    pub session_name: String,

    /// Optional override for the socket parent directory. `None` selects
    /// `${XDG_RUNTIME_DIR}/gtmux` with `/tmp/gtmux-<uid>` fallback. Tests
    /// inject a tempdir here to avoid clobbering live developer sockets.
    pub socket_dir: Option<PathBuf>,
}

/// 1 server : 1 tmux daemon binding. Owns the child handle (when this
/// process spawned the daemon) and the control-mode line stream.
pub struct TmuxDaemon {
    session_name: String,
    socket_path: PathBuf,
    /// `Some` when we spawned the daemon ourselves and therefore have a
    /// child handle to signal. `None` when we attached to a daemon owned by
    /// a prior run — shutting that down is the teardown subcommand's job,
    /// not ours (ADR-0009 D5: Server lifecycle ⊥ tmux daemon lifecycle).
    child: Option<Child>,
    /// Control-mode stdout reader. `Option` so [`Self::read_line`] can take
    /// ownership during `next_line()` without forcing the whole struct
    /// through `&mut Pin<...>` gymnastics — we re-attach on every call.
    control_stdout: Option<Lines<BufReader<ChildStdout>>>,
    control_stdin: Option<ChildStdin>,
}

impl TmuxDaemon {
    /// Spawn (or re-attach to) the dedicated tmux daemon for `opts.session_name`.
    ///
    /// argv = `["tmux", "-L", "gtmux-<session>", "-S", <socket>, "-C",
    /// "new-session", "-A", "-s", <session>, "-d"]` per ADR-0009 D2 + ADR-0001
    /// D11. The `-A` flag turns this into a re-attach when the session
    /// already exists, which is the D21 c2 idempotency contract.
    pub async fn spawn(opts: SpawnOptions) -> Result<Self> {
        let socket_path = resolve_socket_path(&opts)?;
        ensure_socket_dir(socket_path.parent().expect("socket has a parent"))?;

        let label = label_for(&opts.session_name);
        let mut cmd = Command::new("tmux");
        cmd.arg("-L")
            .arg(&label)
            .arg("-S")
            .arg(&socket_path)
            .arg("-C")
            .arg("new-session")
            .arg("-A")
            .arg("-s")
            .arg(&opts.session_name)
            .arg("-d")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            // Prevent zombie daemons if the Rust process panics or exits
            // before reaching [`Self::shutdown`].
            .kill_on_drop(true);
        // ADR-0009 O1: strip `TMUX` env so a developer running gtmux from
        // inside their own tmux session doesn't get a "nested" warning that
        // corrupts our control-mode line parser.
        cmd.env_remove("TMUX");

        let mut child = cmd.spawn().map_err(map_spawn_err)?;
        let stdin = child.stdin.take().ok_or(LifecycleError::ProtocolError)?;
        let stdout = child.stdout.take().ok_or(LifecycleError::ProtocolError)?;

        Ok(Self {
            session_name: opts.session_name,
            socket_path,
            child: Some(child),
            control_stdout: Some(BufReader::new(stdout).lines()),
            control_stdin: Some(stdin),
        })
    }

    /// Re-attach to an existing daemon without claiming ownership of it.
    ///
    /// argv = `["tmux", "-L", <label>, "-S", <socket>, "-C", "attach",
    /// "-t", <session>]`. The control-mode stream is opened on a fresh
    /// short-lived `tmux` client; we *do not* keep the [`Child`] handle
    /// because the daemon (the long-lived process) is not our child.
    pub async fn attach(opts: SpawnOptions) -> Result<Self> {
        let socket_path = resolve_socket_path(&opts)?;
        if !socket_exists(&socket_path) {
            // We cannot fabricate a daemon out of nothing — attach mode is
            // strictly for ports the user is reconnecting to.
            return Err(LifecycleError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("socket not present: {}", socket_path.display()),
            )));
        }

        let label = label_for(&opts.session_name);
        let mut cmd = Command::new("tmux");
        cmd.arg("-L")
            .arg(&label)
            .arg("-S")
            .arg(&socket_path)
            .arg("-C")
            .arg("attach")
            .arg("-t")
            .arg(&opts.session_name)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);
        cmd.env_remove("TMUX");

        let mut client = cmd.spawn().map_err(map_spawn_err)?;
        let stdin = client.stdin.take().ok_or(LifecycleError::ProtocolError)?;
        let stdout = client.stdout.take().ok_or(LifecycleError::ProtocolError)?;

        // `child = None` even though we have a client process: the *daemon*
        // is what `shutdown` would target, and the daemon was not spawned by
        // us. We deliberately drop `client` here — but kill_on_drop would
        // tear our own control-mode channel down at the end of scope, so
        // we keep the handle alive by *not* dropping it. Park it in a
        // detached task that lives as long as the stream does.
        // The simplest representation: stash the client as `child` but
        // model "attach mode" by never invoking [`Self::shutdown`] on it
        // semantically. To preserve the contract that `child = None` means
        // attach mode, we move the client into a tokio::spawn that
        // owns it for the lifetime of the stdio handles. When the
        // stdio handles drop (Self drops), the spawned task observes EOF
        // and the client exits.
        tokio::spawn(async move {
            // The client must not be reaped here — its stdin/stdout move out
            // above. We just hold the Child so kill_on_drop fires when this
            // task is dropped. The task itself parks on `wait()` so it does
            // not consume CPU.
            let _ = client.wait().await;
        });

        Ok(Self {
            session_name: opts.session_name,
            socket_path,
            child: None,
            control_stdout: Some(BufReader::new(stdout).lines()),
            control_stdin: Some(stdin),
        })
    }

    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Read one LF-terminated line from the control-mode channel. Trailing
    /// `\r` is stripped (ADR-0001 D12 CRLF normalisation). Returns `Ok(None)`
    /// at EOF — callers treat that as orderly shutdown.
    pub async fn read_line(&mut self) -> Result<Option<Bytes>> {
        let stream = self
            .control_stdout
            .as_mut()
            .ok_or(LifecycleError::ProtocolError)?;
        match stream.next_line().await? {
            Some(mut s) => {
                if s.ends_with('\r') {
                    s.pop();
                }
                Ok(Some(Bytes::from(s.into_bytes())))
            }
            None => Ok(None),
        }
    }

    /// Write one line plus LF and flush. ADR-0001 D12 requires non-empty
    /// outbound lines (tmux treats a bare `\n` as a detach trigger); we
    /// assert that here instead of letting the daemon misinterpret it.
    pub async fn write_line(&mut self, line: &[u8]) -> Result<()> {
        if line.is_empty() {
            // Caller bug — bare newlines would trigger tmux detach. Refuse
            // explicitly so the failure surfaces in test rather than as a
            // mysterious disconnect at runtime.
            return Err(LifecycleError::ProtocolError);
        }
        let stdin = self
            .control_stdin
            .as_mut()
            .ok_or(LifecycleError::ProtocolError)?;
        stdin.write_all(line).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Graceful control-mode client shutdown: SIGTERM → 5s grace → SIGKILL
    /// fallback.
    ///
    /// Important: this terminates the *control-mode client process we
    /// spawned*, not the tmux daemon. ADR-0009 D5 mandates that the daemon
    /// survives gtmux Server termination so that session / pane state is
    /// preserved across reconnects (D21 c5). The actual daemon shutdown
    /// belongs to the `teardown` CLI (ADR-0009 D6 step 2:
    /// `tmux -L ... kill-server`), not to this lifecycle handle.
    ///
    /// No-op on attach-mode handles (`child = None`) — the control client
    /// in that case is owned by the detached task spawned in [`Self::attach`].
    pub async fn shutdown(&mut self) -> Result<()> {
        // Close stdio first so tmux observes EOF on the control channel —
        // this is the polite signal for tmux to exit its client loop. We
        // still follow up with SIGTERM in case the daemon ignored it.
        drop(self.control_stdin.take());
        drop(self.control_stdout.take());

        let Some(mut child) = self.child.take() else {
            debug!(
                session = %self.session_name,
                "shutdown skipped (attach mode — daemon not owned)"
            );
            return Ok(());
        };

        let Some(pid) = child.id() else {
            // Already reaped — nothing to signal.
            return Ok(());
        };
        let pid = pid as libc::pid_t;

        // SAFETY: `libc::kill` is async-signal-safe and the only inputs are
        // a pid we just observed from the child handle and a constant
        // signal number. Failure (ESRCH) is acceptable — it means the child
        // already exited between `id()` and `kill()`.
        let _ = unsafe { libc::kill(pid, libc::SIGTERM) };

        match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(_status)) => Ok(()),
            Ok(Err(e)) => Err(LifecycleError::Io(e)),
            Err(_elapsed) => {
                warn!(
                    session = %self.session_name,
                    pid,
                    "tmux daemon did not exit within 5s of SIGTERM; sending SIGKILL"
                );
                // `Child::kill` translates to SIGKILL on Unix. We *await*
                // wait() afterwards so the kernel actually reaps the
                // process and we don't leave a zombie behind.
                if let Err(e) = child.kill().await {
                    // ESRCH again — race with natural exit. Acceptable.
                    if e.kind() != io::ErrorKind::InvalidInput {
                        return Err(LifecycleError::Io(e));
                    }
                }
                let _ = child.wait().await;
                Ok(())
            }
        }
    }
}

/// Stale socket cleanup helper. ADR-0009 D4 step 1–3:
///   1. stat the socket — absent ⇒ Ok immediately (nothing to clean)
///   2. probe the daemon with `tmux -L ... -S ... has-session` — alive
///      daemon ⇒ `SocketBusy` (caller must teardown explicitly first)
///   3. unlink + fsync the parent directory for durability
///
/// Steps 4 (token/layout/pid file removal) and 5 (config file removal) are
/// the teardown CLI's responsibility — not this helper's — so callers can
/// reuse this function from any cleanup path without dragging in XDG_STATE
/// semantics.
pub async fn cleanup_stale_socket(socket_path: &Path) -> Result<()> {
    // Step 1: absence is a no-op success.
    if !socket_exists(socket_path) {
        return Ok(());
    }

    // Step 2: ping the daemon. If `has-session` returns 0 we'd be deleting
    // a live socket out from under an active server — refuse.
    if daemon_is_alive(socket_path).await? {
        return Err(LifecycleError::SocketBusy(socket_path.to_path_buf()));
    }

    // Step 3: unlink and fsync the parent so the removal survives a power
    // loss — the rename/unlink metadata lives in the directory inode.
    tokio::fs::remove_file(socket_path).await?;
    if let Some(parent) = socket_path.parent() {
        // Best-effort: fsync may not be supported on all filesystems but a
        // failure here is far less serious than skipping it entirely.
        let parent = parent.to_path_buf();
        let _ = tokio::task::spawn_blocking(move || {
            let dir = std::fs::File::open(&parent)?;
            dir.sync_all()
        })
        .await;
    }
    Ok(())
}

/// Compute the canonical socket path for a session.
///
/// Resolution order:
///   1. `${XDG_RUNTIME_DIR}/gtmux/<session>.sock` — preferred when XDG is
///      set (systemd / launchd places this on a tmpfs that survives login
///      sessions but not reboots — the right semantic for ephemeral state).
///   2. `/tmp/gtmux-<uid>/<session>.sock` — fallback. ADR-0009 O2: on
///      systemd-tmpfiles-managed Linux this directory may be aged out, so
///      operators are nudged toward setting `XDG_RUNTIME_DIR` explicitly.
pub fn socket_path_for(session: &str) -> Result<PathBuf> {
    let dir = socket_dir()?;
    Ok(dir.join(format!("{session}.sock")))
}

// ---- internal helpers -------------------------------------------------------

fn resolve_socket_path(opts: &SpawnOptions) -> Result<PathBuf> {
    match &opts.socket_dir {
        Some(dir) => Ok(dir.join(format!("{}.sock", opts.session_name))),
        None => socket_path_for(&opts.session_name),
    }
}

fn socket_dir() -> Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
        let p = PathBuf::from(xdg);
        if p.is_absolute() {
            return Ok(p.join("gtmux"));
        }
        return Err(LifecycleError::BadXdg(format!(
            "XDG_RUNTIME_DIR is not absolute: {}",
            p.display()
        )));
    }
    // Fallback — `/tmp/gtmux-<uid>`. We use the *effective* uid so suid
    // binaries don't write into another user's tmpdir.
    Ok(PathBuf::from("/tmp").join(format!("gtmux-{}", current_uid())))
}

/// SAFETY-justified `geteuid` wrapper. `libc::geteuid` is documented to
/// always succeed and never set `errno`; the unsafe block is required only
/// because it is FFI.
fn current_uid() -> u32 {
    // SAFETY: `geteuid` takes no arguments, has no preconditions, and is
    // guaranteed by POSIX to succeed. The cast preserves bit-width since
    // `uid_t` is `u32` on all platforms we target (macOS / Linux).
    unsafe { libc::geteuid() }
}

fn ensure_socket_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    // 0700 — sketch §13.3.6 + ADR-0009 §D2. We force the mode even when the
    // directory pre-existed, matching the auth crate's `ensure_state_dir`
    // contract (same-user only).
    use std::os::unix::fs::PermissionsExt;
    let perm = std::fs::Permissions::from_mode(0o700);
    std::fs::set_permissions(dir, perm)?;
    Ok(())
}

fn label_for(session: &str) -> String {
    format!("gtmux-{session}")
}

fn socket_exists(path: &Path) -> bool {
    // `try_exists` follows symlinks; the socket itself is a real inode so
    // a plain `metadata` check would also work. We accept either presence
    // signal as "something is there".
    matches!(path.try_exists(), Ok(true))
}

async fn daemon_is_alive(socket_path: &Path) -> Result<bool> {
    // We re-derive the label from the filename so this helper does not
    // require the caller to pass the session name twice. Stripping the
    // `.sock` suffix is sufficient because we control the naming scheme
    // via `socket_path_for`.
    let session = socket_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let label = label_for(session);

    let status = Command::new("tmux")
        .arg("-L")
        .arg(&label)
        .arg("-S")
        .arg(socket_path)
        .arg("has-session")
        .arg("-t")
        .arg(session)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .status()
        .await;

    match status {
        Ok(s) => Ok(s.success()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(LifecycleError::TmuxNotFound),
        Err(e) => Err(LifecycleError::Io(e)),
    }
}

fn map_spawn_err(e: io::Error) -> LifecycleError {
    if e.kind() == io::ErrorKind::NotFound {
        LifecycleError::TmuxNotFound
    } else {
        LifecycleError::TmuxSpawn(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// `XDG_RUNTIME_DIR` is process-global; serialise tests that mutate it.
    /// We mirror the pattern from gtmux-auth so the two crates do not race
    /// each other during a workspace-wide `cargo test`.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct XdgGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev: Option<std::ffi::OsString>,
        _tmp: TempDir,
    }

    impl XdgGuard {
        fn new() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev = std::env::var_os("XDG_RUNTIME_DIR");
            let tmp = tempfile::tempdir().expect("tempdir");
            std::env::set_var("XDG_RUNTIME_DIR", tmp.path());
            Self {
                _lock: lock,
                prev,
                _tmp: tmp,
            }
        }
    }

    impl Drop for XdgGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
        }
    }

    /// A guard that *clears* `XDG_RUNTIME_DIR` to exercise the fallback
    /// branch. Required because CI hosts often set XDG_RUNTIME_DIR but we
    /// still need to assert the `/tmp/gtmux-<uid>` shape.
    struct NoXdgGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev: Option<std::ffi::OsString>,
    }

    impl NoXdgGuard {
        fn new() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev = std::env::var_os("XDG_RUNTIME_DIR");
            std::env::remove_var("XDG_RUNTIME_DIR");
            Self { _lock: lock, prev }
        }
    }

    impl Drop for NoXdgGuard {
        fn drop(&mut self) {
            if let Some(v) = &self.prev {
                std::env::set_var("XDG_RUNTIME_DIR", v);
            }
        }
    }

    #[test]
    fn socket_path_for_xdg() {
        let g = XdgGuard::new();
        let p = socket_path_for("alpha").unwrap();
        let xdg = g._tmp.path();
        assert_eq!(p, xdg.join("gtmux").join("alpha.sock"));
    }

    #[test]
    fn socket_path_fallback() {
        let _g = NoXdgGuard::new();
        let p = socket_path_for("beta").unwrap();
        let s = p.to_string_lossy();
        // Pattern: `/tmp/gtmux-<uid>/beta.sock`.
        assert!(
            s.starts_with("/tmp/gtmux-"),
            "fallback should live under /tmp/gtmux-<uid>, got {s}"
        );
        assert!(s.ends_with("/beta.sock"), "should end with session.sock");
    }

    #[test]
    fn socket_path_rejects_relative_xdg() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os("XDG_RUNTIME_DIR");
        std::env::set_var("XDG_RUNTIME_DIR", "relative/path");
        let err = socket_path_for("x").unwrap_err();
        match &prev {
            Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
        assert!(
            matches!(err, LifecycleError::BadXdg(_)),
            "expected BadXdg, got {err:?}"
        );
    }

    #[tokio::test]
    async fn cleanup_stale_socket_missing_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        // A path that has never been touched — Ok(()) immediately.
        let p = tmp.path().join("ghost.sock");
        cleanup_stale_socket(&p).await.unwrap();
    }

    #[tokio::test]
    async fn cleanup_stale_socket_dead_daemon() {
        // tmux is available on the dev host; we drop an empty file into a
        // unique directory and expect cleanup to unlink it. `has-session`
        // against a nonexistent daemon returns non-zero, satisfying the
        // "dead" branch.
        if !tmux_available().await {
            eprintln!("skipping: tmux not available");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("never-was.sock");
        std::fs::write(&path, b"").unwrap();
        assert!(path.exists());
        cleanup_stale_socket(&path).await.unwrap();
        assert!(!path.exists(), "stale socket should be unlinked");
    }

    #[tokio::test]
    #[ignore = "requires tmux binary on PATH"]
    async fn spawn_and_shutdown() {
        let tmp = tempfile::tempdir().unwrap();
        let session = format!("test-spawn-{}", std::process::id());
        let opts = SpawnOptions {
            session_name: session.clone(),
            socket_dir: Some(tmp.path().to_path_buf()),
        };
        let mut d = TmuxDaemon::spawn(opts.clone()).await.expect("spawn");
        // After spawn, the socket file should exist and `has-session`
        // should succeed against it.
        // Give tmux a moment to bind the socket — spawn returns once we have
        // the child handle, but socket creation is async on the daemon side.
        for _ in 0..50 {
            if d.socket_path().exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(d.socket_path().exists(), "socket should exist post-spawn");
        assert!(
            daemon_is_alive(d.socket_path()).await.unwrap(),
            "daemon must respond to has-session"
        );

        // shutdown terminates the control-mode client we own — ADR-0009 D5
        // mandates the daemon survives so session state is preserved.
        d.shutdown().await.expect("shutdown");
        assert!(
            daemon_is_alive(d.socket_path()).await.unwrap(),
            "daemon must survive client shutdown (ADR-0009 D5)"
        );

        // Explicit teardown matches the CLI subcommand semantics (D6 step 2).
        // We invoke it here directly so the test cleans up after itself.
        let socket_path = d.socket_path().to_path_buf();
        let label = label_for(&session);
        let killed = Command::new("tmux")
            .arg("-L")
            .arg(&label)
            .arg("-S")
            .arg(&socket_path)
            .arg("kill-server")
            .status()
            .await
            .expect("kill-server")
            .success();
        assert!(killed, "kill-server should succeed");
        // After kill-server the socket file may persist (D6 step 3 confirms
        // this is observed behaviour). cleanup_stale_socket handles it.
        if socket_path.exists() {
            cleanup_stale_socket(&socket_path)
                .await
                .expect("cleanup after kill-server");
        }
    }

    #[tokio::test]
    #[ignore = "requires tmux binary on PATH"]
    async fn attach_to_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let session = format!("test-attach-{}", std::process::id());
        let opts = SpawnOptions {
            session_name: session.clone(),
            socket_dir: Some(tmp.path().to_path_buf()),
        };
        let mut first = TmuxDaemon::spawn(opts.clone()).await.expect("spawn");
        for _ in 0..50 {
            if first.socket_path().exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(first.socket_path().exists());

        // attach must succeed and report no child ownership.
        let mut second = TmuxDaemon::attach(opts.clone()).await.expect("attach");
        assert!(
            second.child.is_none(),
            "attach mode must leave child = None"
        );
        // attach-mode shutdown is a no-op — daemon must survive.
        second.shutdown().await.expect("attach shutdown is no-op");
        assert!(
            daemon_is_alive(first.socket_path()).await.unwrap(),
            "daemon should still be alive after attach-mode shutdown"
        );

        // Client shutdown on the spawning handle — daemon still survives.
        first.shutdown().await.expect("client shutdown");
        assert!(
            daemon_is_alive(first.socket_path()).await.unwrap(),
            "daemon survives client shutdown on the owning handle too"
        );

        // Explicit teardown to keep the test sandbox clean.
        let socket_path = first.socket_path().to_path_buf();
        let label = label_for(&session);
        let _ = Command::new("tmux")
            .arg("-L")
            .arg(&label)
            .arg("-S")
            .arg(&socket_path)
            .arg("kill-server")
            .status()
            .await;
        if socket_path.exists() {
            let _ = cleanup_stale_socket(&socket_path).await;
        }
    }

    #[tokio::test]
    #[ignore = "requires tmux binary on PATH"]
    async fn cleanup_stale_socket_busy_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        let session = format!("test-busy-{}", std::process::id());
        let opts = SpawnOptions {
            session_name: session.clone(),
            socket_dir: Some(tmp.path().to_path_buf()),
        };
        let mut d = TmuxDaemon::spawn(opts.clone()).await.expect("spawn");
        for _ in 0..50 {
            if d.socket_path().exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(d.socket_path().exists());

        // Live daemon must refuse cleanup.
        let err = cleanup_stale_socket(d.socket_path()).await.unwrap_err();
        assert!(
            matches!(err, LifecycleError::SocketBusy(_)),
            "expected SocketBusy, got {err:?}"
        );
        // Socket must still be present after the refused cleanup.
        assert!(d.socket_path().exists());

        // Client shutdown — daemon still alive after this.
        d.shutdown().await.expect("client shutdown");

        // Explicit teardown to keep the test sandbox clean.
        let socket_path = d.socket_path().to_path_buf();
        let label = label_for(&session);
        let _ = Command::new("tmux")
            .arg("-L")
            .arg(&label)
            .arg("-S")
            .arg(&socket_path)
            .arg("kill-server")
            .status()
            .await;
        if socket_path.exists() {
            let _ = cleanup_stale_socket(&socket_path).await;
        }
    }

    async fn tmux_available() -> bool {
        Command::new("tmux")
            .arg("-V")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
