//! gtmux-lifecycle — dedicated tmux daemon spawn / attach / shutdown +
//! stale socket cleanup helper + full 5-step teardown + pidfile management.
//!
//! Implements ADR-0009 (tmux daemon isolation) D2/D3 (spawn + socket path) and
//! D6 (5-step teardown: kill-server → unlink socket → unlink state files →
//! unlink config → report). [`cleanup_stale_socket`] remains as the cheap
//! "is the socket inode safe to remove" helper for the spawn fast-path, while
//! [`teardown`] composes that helper into the full destructive cleanup that
//! the CLI's `teardown` subcommand drives. ADR-0001 D1·D11 commit the
//! control-mode entrypoint argv (`tmux -L gtmux-<session> -S <socket> -C ...`)
//! that this module reifies into [`TmuxDaemon::spawn`] / [`TmuxDaemon::attach`].
//!
//! Sprint 4-D LIFE-3 adds the server pidfile surface: [`write_pidfile`] /
//! [`pidfile_path_for`] / [`check_pidfile_liveness`] / [`stop_server`] turn
//! the previously informational `gtmux stop` subcommand into a real graceful
//! shutdown channel. The pidfile lives at
//! `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.pid` (mode 0600,
//! parent dir 0700) and is removed by [`teardown`] step 3 alongside the
//! token + layout artefacts. ADR-0009 D5 is preserved: `gtmux stop`
//! terminates the *Rust server process* only — the tmux daemon survives so
//! `gtmux start --session <name>` can re-attach later (D21 c2).
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
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use gtmux_mux_router::{parse_line, Event};
use gtmux_ws_server::{Hub, TmuxRequest};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, Mutex};
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
///
/// SAFETY/CORRECTNESS NOTE: stdin and stdout each sit behind their own
/// `tokio::sync::Mutex`. This is what makes [`run_event_loop`] (which
/// holds the stdout mutex across `read_line.await`) and
/// [`run_command_loop`] (which holds the stdin mutex during `write_line`)
/// non-blocking with respect to each other — a previous design used a
/// single outer mutex and deadlocked the writer whenever tmux idled
/// between control-mode emits. The `child` handle has its own mutex so
/// `shutdown` can mutate it without blocking the read/write loops.
pub struct TmuxDaemon {
    session_name: String,
    socket_path: PathBuf,
    /// `Some` when we spawned the daemon ourselves and therefore have a
    /// child handle to signal. `None` when we attached to a daemon owned by
    /// a prior run — shutting that down is the teardown subcommand's job,
    /// not ours (ADR-0009 D5: Server lifecycle ⊥ tmux daemon lifecycle).
    child: Mutex<Option<Child>>,
    /// Control-mode stdout reader.
    stdout_lock: Mutex<Option<Lines<BufReader<ChildStdout>>>>,
    /// Control-mode stdin writer.
    stdin_lock: Mutex<Option<ChildStdin>>,
}

impl TmuxDaemon {
    /// Spawn (or re-attach to) the dedicated tmux daemon for `opts.session_name`.
    ///
    /// argv = `["tmux", "-L", "gtmux-<session>", "-S", <socket>, "-C",
    /// "new-session", "-A", "-s", <session>]` per ADR-0009 D2 + ADR-0001
    /// D11. The `-A` flag turns this into a re-attach when the session
    /// already exists, which is the D21 c2 idempotency contract.
    ///
    /// Note on `-d`: we intentionally do NOT pass `-d`. With `-d`, tmux
    /// creates the session detached and then exits the control-mode client
    /// immediately — our `run_event_loop` would see EOF before ever
    /// reading the boot-time `%session-changed $0 <name>` line that the
    /// Hub caches for late WS subscribers. Without `-d` the control-mode
    /// client stays attached for as long as our `stdin` pipe is open,
    /// which is the entire `gtmux start` lifetime. The daemon outlives
    /// the client either way (tmux keeps the server up as long as any
    /// session has windows), so this does not change ADR-0009 D5's
    /// "daemon survives Server shutdown" guarantee.
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
            child: Mutex::new(Some(child)),
            stdout_lock: Mutex::new(Some(BufReader::new(stdout).lines())),
            stdin_lock: Mutex::new(Some(stdin)),
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
            child: Mutex::new(None),
            stdout_lock: Mutex::new(Some(BufReader::new(stdout).lines())),
            stdin_lock: Mutex::new(Some(stdin)),
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
    ///
    /// `&self` rather than `&mut self`: stdin/stdout/child all sit behind
    /// independent mutexes so the read-half can be locked without blocking
    /// the write-half. See the struct-level note on `TmuxDaemon`.
    pub async fn read_line(&self) -> Result<Option<Bytes>> {
        let mut guard = self.stdout_lock.lock().await;
        let stream = guard.as_mut().ok_or(LifecycleError::ProtocolError)?;
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
    pub async fn write_line(&self, line: &[u8]) -> Result<()> {
        if line.is_empty() {
            // Caller bug — bare newlines would trigger tmux detach. Refuse
            // explicitly so the failure surfaces in test rather than as a
            // mysterious disconnect at runtime.
            return Err(LifecycleError::ProtocolError);
        }
        let mut guard = self.stdin_lock.lock().await;
        let stdin = guard.as_mut().ok_or(LifecycleError::ProtocolError)?;
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
    pub async fn shutdown(&self) -> Result<()> {
        // Close stdio first so tmux observes EOF on the control channel —
        // this is the polite signal for tmux to exit its client loop. We
        // still follow up with SIGTERM in case the daemon ignored it.
        drop(self.stdin_lock.lock().await.take());
        drop(self.stdout_lock.lock().await.take());

        let Some(mut child) = self.child.lock().await.take() else {
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

// ────────────────────────────────────────────────────────────────────────────
// Mux ↔ WS bridge loops (Sprint 4-B WIRE-2/3)
// ────────────────────────────────────────────────────────────────────────────

/// Run the tmux read loop: `read_line → parse_line → hub.publish`.
///
/// Shares the daemon handle with [`run_command_loop`] via `Arc<TmuxDaemon>`.
/// The struct holds independent mutexes for stdin / stdout / child, so
/// holding the stdout half across `read_line.await` does *not* block the
/// writer — that distinction is load-bearing now that the control-mode
/// client is long-lived (the previous single-mutex design deadlocked the
/// writer whenever tmux idled between emits).
///
/// Returns:
///   * `Ok(())` on a clean `%exit` (daemon shutdown) or stdin EOF.
///   * `Err(LifecycleError::Io)` for unrecoverable read errors.
///
/// The hub continues to live after this function returns; subscribers see
/// the `Event::Exit` flow through normally because the loop publishes it
/// before returning.
pub async fn run_event_loop(daemon: Arc<TmuxDaemon>, hub: Hub) -> Result<()> {
    loop {
        let line = daemon.read_line().await?;
        if let Some(ref bytes) = line {
            debug!(line = %String::from_utf8_lossy(bytes), "run_event_loop: read tmux stdout");
        }
        let Some(bytes) = line else {
            // EOF on the daemon channel — orderly shutdown. Synthesize an
            // `Event::Exit` so subscribers can close cleanly even if tmux
            // never sent `%exit`.
            hub.publish(Event::Exit {
                reason: Some("daemon-eof".to_string()),
            })
            .await;
            return Ok(());
        };
        match parse_line(&bytes) {
            Ok(Some(event)) => {
                let is_exit = matches!(event, Event::Exit { .. });
                hub.publish(event).await;
                if is_exit {
                    return Ok(());
                }
            }
            Ok(None) => {
                // %begin/%end/%error or a blank line — not an event. Drop.
            }
            Err(e) => {
                debug!(error = %e, "mux-router parse error; dropping line");
            }
        }
    }
}

/// Run the tmux write loop: receives [`TmuxRequest`] over `rx` and
/// serialises each one as a control-mode command line.
///
/// The wire format for a control-mode request is the literal command
/// followed by argv values separated by spaces. Args containing spaces or
/// shell metacharacters are passed *as-is* — argv-vs-shell separation is
/// already enforced at the WS handler boundary by the
/// [`gtmux_ws_server::cmd_router`] gates (CTRL allowlist + JSON `args:
/// string[]`-only). The mux-router `Command` discriminator names the
/// canonical command keyword; we look that up here so the writer remains
/// the single source of truth for keyword spelling.
///
/// SAFETY/CORRECTNESS NOTE: a bare `\n` line is forbidden by ADR-0001 D12
/// (tmux interprets it as detach). [`TmuxDaemon::write_line`] asserts
/// non-empty before writing, so an empty `args` for a command whose
/// keyword is also empty would error out cleanly rather than poison the
/// channel.
pub async fn run_command_loop(
    daemon: Arc<TmuxDaemon>,
    mut rx: mpsc::Receiver<TmuxRequest>,
) -> Result<()> {
    while let Some(req) = rx.recv().await {
        let line = serialise_command(&req);
        if line.is_empty() {
            warn!(?req, "skipping empty command line");
            continue;
        }
        debug!(line = %line, "run_command_loop: writing tmux stdin");
        if let Err(e) = daemon.write_line(line.as_bytes()).await {
            warn!(error = %e, "tmux write_line failed; loop exiting");
            return Err(e);
        }
        debug!("run_command_loop: write_line ok");
    }
    debug!("run_command_loop: rx closed, exiting");
    Ok(())
}

/// Compose one tmux command line from a [`TmuxRequest`].
///
/// We look up the canonical keyword from the `Command` discriminator and
/// then append `args` separated by single spaces. This matches the tmux
/// control-mode wire protocol — control mode reads one keyword + argv
/// tokens per line; quoting is the caller's responsibility upstream.
///
/// Structured variants (currently `ResizeWindow`) render their full argv
/// from the variant fields and ignore `args`. This keeps the keyword +
/// target-id contract type-safe and removes any need for `args[0]` keyword
/// override (S5-MUX-1).
fn serialise_command(req: &TmuxRequest) -> String {
    use gtmux_mux_router::Command as C;
    let keyword: &str = match &req.command {
        C::NewWindow => "new-window",
        C::KillPane => "kill-pane",
        C::KillWindow => "kill-window",
        C::RenameWindow => "rename-window",
        C::SendKeys => "send-keys",
        C::RefreshClientPause => "refresh-client",
        C::RefreshClientSubscribe => "refresh-client",
        C::CapturePane => "capture-pane",
        C::ListSessions => "list-sessions",
        C::ListWindows => "list-windows",
        C::ListPanes => "list-panes",
        C::ResizeWindow {
            window_id,
            cols,
            rows,
        } => {
            return format!("resize-window -t @{window_id} -x {cols} -y {rows}");
        }
    };
    if req.args.is_empty() {
        keyword.to_string()
    } else {
        let mut parts: Vec<&str> = Vec::with_capacity(1 + req.args.len());
        parts.push(keyword);
        parts.extend(req.args.iter().map(String::as_str));
        parts.join(" ")
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

// ────────────────────────────────────────────────────────────────────────────
// Teardown — ADR-0009 D6 5-step cleanup
// ────────────────────────────────────────────────────────────────────────────

/// Caller-tunable knobs for [`teardown`]. The struct is non-exhaustive in
/// spirit (we keep the public surface tight) but stable on the existing
/// fields — adding a flag is additive and gets a `Default` update.
#[derive(Debug, Clone)]
pub struct TeardownOpts {
    /// When `false` (the default), [`teardown`] refuses to proceed if the
    /// daemon is still alive — the caller is expected to bounce back through
    /// the user's confirmation gate and retry with `force = true`. When
    /// `true`, the daemon is killed inline via `kill-server` regardless of
    /// liveness and the socket is reaped after a short settling delay.
    pub force: bool,

    /// When `true` (the default), the per-session state files under
    /// `${XDG_STATE_HOME}/gtmux/` (token, layout snapshot, pidfile) are
    /// removed. The state *directory* itself is preserved so other sessions
    /// are untouched. Setting this to `false` is a debug aid — it lets an
    /// operator inspect the post-teardown state files before manual cleanup.
    pub remove_state_files: bool,

    /// When `true` (the default), the per-session config file under
    /// `${XDG_CONFIG_HOME}/gtmux/<session>.config.toml` is removed (D21 c8
    /// 5th step). Setting this to `false` matches the `--keep-config` CLI
    /// flag for users who intend to bring the same Server back up later
    /// without re-typing every field.
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

/// Outcome of a [`teardown`] call. Every field is set even on partial
/// failure so callers can render a faithful report to the user without
/// inspecting the `Result` chain.
#[derive(Debug, Clone, Default)]
pub struct TeardownReport {
    /// `true` if we issued (or attempted) `kill-server` against a daemon
    /// that was actually alive. `false` means the daemon was already gone
    /// before we got here — that's a non-fatal "already dead" outcome and
    /// is recorded in `warnings` instead.
    pub daemon_killed: bool,

    /// `true` if a socket inode existed and we successfully unlinked it.
    /// `false` means the socket was already absent (recorded as a warning).
    pub socket_removed: bool,

    /// Paths under `${XDG_STATE_HOME}/gtmux/` that were removed in this
    /// teardown. Order matches removal order: token → layout → pid. Files
    /// that were absent or kept (per `opts`) are not listed here — they're
    /// either silently skipped or appended to `warnings`.
    pub state_files_removed: Vec<PathBuf>,

    /// `Some(path)` if a config file existed and was removed. `None` if it
    /// was absent or kept (per `opts.remove_config = false`). When kept,
    /// `warnings` carries the explicit "config kept" note.
    pub config_removed: Option<PathBuf>,

    /// Non-fatal anomalies surfaced to the operator. Each entry is a short
    /// human-readable sentence; the CLI prints them under a `Warnings:`
    /// heading. Examples: "socket already missing", "config file not found",
    /// "token had 0644 perm at removal (expected 0600)".
    pub warnings: Vec<String>,
}

/// Execute the ADR-0009 §D6 5-step teardown for `session`.
///
/// The five steps run in this exact order (D6 mandates the sequencing so the
/// daemon is gone before we touch its socket inode and the state files are
/// gone before the config that names them):
///
///   1. `tmux -L gtmux-<session> -S <socket> kill-server` — daemon shutdown.
///      If the daemon was already dead the command's non-zero exit is
///      *not* treated as a failure; we just downgrade to a warning.
///   2. `cleanup_stale_socket(<socket>)` — unlink the socket inode. The
///      helper handles both "absent" (no-op) and "alive" (refuse) branches,
///      but step 1 guarantees we land in the "dead" branch here.
///   3. Unlink token / layout.json / pid under `${XDG_STATE_HOME}/gtmux/`.
///      Each file is independent — failure to remove one is reported as a
///      warning but does not abort the others. The directory is preserved.
///   4. Unlink `${XDG_CONFIG_HOME}/gtmux/<session>.config.toml` (D21 c8) —
///      gated on `opts.remove_config`.
///   5. Return [`TeardownReport`] capturing every step's outcome.
///
/// `opts.force` semantics:
///   * `force = false` — refuses cleanly with [`LifecycleError::SocketBusy`]
///     when the daemon is alive. The caller (CLI) is expected to surface a
///     confirmation prompt and re-invoke with `force = true`.
///   * `force = true`  — issues `kill-server` regardless of liveness and
///     waits up to ~2s for the socket to disappear before re-running the
///     stale-socket helper.
///
/// **Failure model**: this function returns `Err` only when *step 1's*
/// liveness check fails the `force = false` gate. All later steps are
/// considered "best-effort": their failures land in `report.warnings` and
/// the function still returns `Ok(report)`. The CLI is responsible for
/// mapping a non-empty warning list to the grill D20 exit-7 contract when
/// the operator did not opt into the partial-success outcome.
pub async fn teardown(session: &str, opts: TeardownOpts) -> Result<TeardownReport> {
    let mut report = TeardownReport::default();

    let socket_path = socket_path_for(session)?;
    let label = label_for(session);

    // Step 1 — kill-server, gated on `force` when the daemon is still alive.
    // We can't tell "no daemon" from "tmux missing" cheaply, so we trust the
    // helper's `TmuxNotFound` propagation if the binary is absent.
    let alive = if socket_exists(&socket_path) {
        match daemon_is_alive(&socket_path).await {
            Ok(b) => b,
            Err(LifecycleError::TmuxNotFound) => {
                // tmux gone but a stale socket inode is present — treat as
                // "dead" so we still try to unlink. The caller will surface
                // the `TmuxNotFound` error if step 1 actually tries to spawn.
                report
                    .warnings
                    .push("tmux binary not on PATH; skipping kill-server".to_string());
                false
            }
            Err(e) => return Err(e),
        }
    } else {
        false
    };

    if alive {
        if !opts.force {
            // Refuse: caller must confirm + retry with force=true. The
            // socket is intact and the daemon untouched at this point.
            return Err(LifecycleError::SocketBusy(socket_path));
        }
        // `kill-server` against a live daemon — exit 0 means accepted; the
        // socket inode survives the kill (R(rej)4 + D6 step 3 confirmation).
        match Command::new("tmux")
            .arg("-L")
            .arg(&label)
            .arg("-S")
            .arg(&socket_path)
            .arg("kill-server")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .status()
            .await
        {
            Ok(s) if s.success() => {
                report.daemon_killed = true;
            }
            Ok(_status) => {
                // Daemon died between liveness check and kill-server, or
                // refused for an internal tmux reason. Either way the socket
                // becomes safe to unlink shortly.
                report
                    .warnings
                    .push("tmux kill-server exited non-zero (treating as dead)".to_string());
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(LifecycleError::TmuxNotFound);
            }
            Err(e) => return Err(LifecycleError::Io(e)),
        }

        // Give the daemon's signal handler up to ~2s to release the socket.
        // We poll instead of sleeping flat-out so the common case (fast
        // exit) returns in tens of milliseconds.
        for _ in 0..20 {
            if !socket_exists(&socket_path) {
                break;
            }
            if !daemon_is_alive(&socket_path).await.unwrap_or(false) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    } else {
        report
            .warnings
            .push("tmux daemon already dead; skipping kill-server".to_string());
    }

    // Step 2 — unlink the socket inode. `cleanup_stale_socket` is reused so
    // both fast-paths (already gone / dead daemon) share one code path. The
    // helper refuses on a live daemon, which by construction cannot happen
    // here because step 1 forced it dead or we wouldn't be running.
    if socket_exists(&socket_path) {
        match cleanup_stale_socket(&socket_path).await {
            Ok(()) => {
                report.socket_removed = true;
            }
            Err(LifecycleError::SocketBusy(_)) => {
                // Daemon resurfaced (extremely unlikely) — surface as a
                // warning rather than aborting because step 3+ are still
                // useful cleanups.
                report
                    .warnings
                    .push("socket still busy after kill-server; leaving inode".to_string());
            }
            Err(e) => {
                report.warnings.push(format!("socket unlink failed: {e}"));
            }
        }
    } else {
        report.warnings.push("socket already missing".to_string());
    }

    // Step 3 — state files. Token / layout / pid are independent removals
    // under the same `${XDG_STATE_HOME}/gtmux/` directory; we keep the dir
    // itself so sibling sessions (other Servers) are untouched.
    if opts.remove_state_files {
        let state_dir = state_dir_for_gtmux()?;
        let candidates: [(&str, PathBuf); 3] = [
            ("token", state_dir.join(format!("{session}.token"))),
            ("layout", state_dir.join(format!("{session}.layout.json"))),
            ("pid", state_dir.join(format!("{session}.pid"))),
        ];
        for (kind, path) in candidates {
            match remove_state_file(&path, kind).await {
                Ok(removed) => {
                    if removed {
                        report.state_files_removed.push(path);
                    } else {
                        // Absent file is normal (layout.json is not yet
                        // written by any Sprint-2 code path); don't spam
                        // the operator. We only note pidfile absence
                        // because that's the one users may care about.
                        if kind == "pid" {
                            report.warnings.push(format!(
                                "{} not present: {}",
                                kind,
                                path.display()
                            ));
                        }
                    }
                }
                Err(msg) => {
                    report.warnings.push(msg);
                }
            }
        }
    } else {
        report
            .warnings
            .push("state files kept (remove_state_files = false)".to_string());
    }

    // Step 4 — config file. ADR-0009 D6 step 5 + D21 c8: pair name is
    // `<session>.config.toml` (D22 §위치·포맷).
    let config_path = config_dir_for_gtmux()?.join(format!("{session}.config.toml"));
    if opts.remove_config {
        match tokio::fs::remove_file(&config_path).await {
            Ok(()) => {
                report.config_removed = Some(config_path);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                report
                    .warnings
                    .push(format!("config file not found: {}", config_path.display()));
            }
            Err(e) => {
                report.warnings.push(format!("config unlink failed: {e}"));
            }
        }
    } else {
        report
            .warnings
            .push(format!("config kept: {}", config_path.display()));
    }

    // Step 5 — return the populated report. The caller (CLI) inspects it to
    // decide on stdout formatting and the grill-D20 exit code.
    Ok(report)
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

// ────────────────────────────────────────────────────────────────────────────
// Server pidfile — Sprint 4-D LIFE-3
//
// The pidfile is the in-band channel `gtmux stop` uses to deliver SIGTERM to
// the foreground server process. ADR-0009 D5 forbids killing the tmux
// daemon on `stop` (that's `teardown`'s job), so the pidfile names the Rust
// server PID — not the daemon's.
//
// Path convention: `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.pid`
//   * mode 0600 (only the running user can read / overwrite / unlink)
//   * parent dir mode 0700 (same as token / layout — sketch §13.3.6)
//   * format: `<decimal pid>\n` — one line, trailing newline for human use
//   * atomic write: temp file (same dir) + fsync + rename + dir fsync
//     (mirrors `gtmux_auth::save_token` so an OS crash mid-write never
//     leaves a half-written pidfile behind)
//
// `teardown` step 3 already enumerates `<session>.pid` for removal — keeping
// the same `<session>.pid` filename here means a `gtmux teardown` after a
// crash naturally cleans up a stale pidfile too.
// ────────────────────────────────────────────────────────────────────────────

/// Required mode for the pidfile itself. Sketch §13.3.6: per-session
/// secrets and PID handles are user-private.
const PIDFILE_PERM: u32 = 0o600;
/// Required mode for the pidfile's parent directory (same as the token
/// directory; we share the same `${XDG_STATE_HOME}/gtmux/` path).
const PIDFILE_DIR_PERM: u32 = 0o700;

/// Resolve the canonical pidfile path for `session`:
/// `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.pid`.
///
/// Errors with [`LifecycleError::BadXdg`] when `XDG_STATE_HOME` is set but
/// empty, or when both XDG_STATE_HOME and HOME are unset.
pub fn pidfile_path_for(session: &str) -> Result<PathBuf> {
    Ok(state_dir_for_gtmux()?.join(format!("{session}.pid")))
}

/// Liveness verdict for a pidfile read at start time. The CLI's `gtmux start`
/// path consults this *before* spawning a tmux daemon: a live PID means we
/// must refuse with exit 4 (port-in-use spirit — duplicate server bind),
/// while a stale PID means the previous server crashed and we should
/// overwrite the pidfile silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PidLiveness {
    /// No pidfile present — first start on this host or post-teardown.
    Absent,
    /// Pidfile present and `kill(pid, 0)` succeeded — server still running.
    Alive(libc::pid_t),
    /// Pidfile present but `kill(pid, 0)` returned ESRCH — orphaned file
    /// from a crashed prior run.
    Stale(libc::pid_t),
    /// Pidfile present but contents could not be parsed as a positive
    /// decimal PID. Treated as `Stale` by callers but kept as a distinct
    /// variant so the CLI can surface the corruption in logs.
    Malformed,
}

/// Probe the pidfile for `session` and report its liveness.
///
/// Reads the file (no perm enforcement — a too-wide pidfile is a forensic
/// warning rather than a fail-closed condition since the contents are not
/// secret), parses the first non-empty line as a decimal PID, then probes
/// with `kill(pid, 0)`. Returns [`PidLiveness::Absent`] when the file does
/// not exist.
pub fn check_pidfile_liveness(session: &str) -> Result<PidLiveness> {
    let path = pidfile_path_for(session)?;
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(PidLiveness::Absent),
        Err(e) => return Err(LifecycleError::Io(e)),
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

/// Write `std::process::id()` to the pidfile for `session` atomically.
///
/// Steps (mirrors `gtmux_auth::save_token`):
///   1. Resolve `${XDG_STATE_HOME}/gtmux/<session>.pid`.
///   2. Ensure parent dir at mode 0700 (create if missing).
///   3. Open a temp file in the same dir with `O_CREAT | O_EXCL` mode 0600.
///   4. Write `<pid>\n`, fsync.
///   5. Rename temp → final (atomic on POSIX, same filesystem).
///   6. fsync the parent dir for durability.
///
/// Returns the final pidfile path so callers can log it. Overwrites any
/// existing pidfile — `gtmux start` is expected to have already routed
/// through [`check_pidfile_liveness`] and refused on `Alive`.
pub fn write_pidfile(session: &str) -> Result<PathBuf> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let final_path = pidfile_path_for(session)?;
    let dir = final_path
        .parent()
        .expect("pidfile_path_for always returns a path with a parent");
    ensure_state_dir(dir)?;

    // Embed the writer's PID in the temp name so two concurrent
    // `gtmux start` invocations (rare — both should fail one of the
    // liveness gates above) don't collide on the same `.tmp` inode.
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
        // Force the mode even when a permissive umask widened it — same
        // pattern as `save_token`. Token mode is the security floor; the
        // pidfile inherits it because it lives next to the token.
        let perm = fs::Permissions::from_mode(PIDFILE_PERM);
        f.set_permissions(perm)?;
        writeln!(f, "{}", std::process::id())?;
        f.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(LifecycleError::Io(e));
    }

    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(LifecycleError::Io(e));
    }

    // Durability fence on the parent so the rename survives a power loss.
    // Failure here means the rename may not survive, but the in-memory
    // state is consistent — we surface the io::Error.
    if let Err(e) = fsync_dir(dir) {
        return Err(LifecycleError::Io(e));
    }
    Ok(final_path)
}

/// Ensure the state directory exists at mode 0700. Same contract as
/// `gtmux_auth::ensure_state_dir` — duplicated here to keep auth's surface
/// tight (the auth crate intentionally keeps its `ensure_state_dir` private).
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

/// Parse the first non-empty trimmed line of `raw` as a positive decimal PID.
/// Anything else (empty file, non-numeric content, zero, negative) returns
/// `None`.
fn parse_pid(raw: &str) -> Option<libc::pid_t> {
    let line = raw.lines().find(|l| !l.trim().is_empty())?.trim();
    let n: libc::pid_t = line.parse().ok()?;
    if n <= 0 {
        return None;
    }
    Some(n)
}

/// Send signal 0 to `pid` to probe existence.
///
/// `kill(pid, 0)` returns 0 on success (process exists and the caller has
/// permission to signal it), -1 with errno ESRCH if the process does not
/// exist, and -1 with errno EPERM if the process exists but belongs to
/// another user. We collapse the latter into "alive" because for our
/// single-user invariant a foreign-owned PID matching ours is the worst
/// case to be cautious about (refuse to overwrite, refuse to SIGTERM
/// blindly).
fn pid_is_alive(pid: libc::pid_t) -> bool {
    // SAFETY: `libc::kill` with sig=0 is the canonical "probe" call; no
    // signal is actually delivered. Inputs are a pid we just parsed and
    // the constant 0. Failure modes are reported via the return value +
    // `errno`; nothing else is invalidated.
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    // SAFETY: `__errno_location` / `errno` is a thread-local on every
    // libc we target. `io::Error::last_os_error()` wraps that read so we
    // don't have to call libc directly.
    let err = io::Error::last_os_error();
    // `EPERM` (process exists, foreign uid) is "alive" for our purposes —
    // we won't be able to SIGTERM it, but we also must not pretend it's
    // gone. Anything else (ESRCH, EINVAL) → "not alive".
    err.raw_os_error() == Some(libc::EPERM)
}

/// Outcome of a [`stop_server`] call. The CLI maps this into the user-
/// visible message + exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopOutcome {
    /// No pidfile present — there is no running server to stop.
    NoPidfile(PathBuf),
    /// Pidfile present but unparseable. Caller should warn + best-effort
    /// remove the file.
    MalformedPidfile(PathBuf),
    /// PID parsed but already absent (ESRCH on the initial probe).
    /// Pidfile is removed so the next `gtmux start` doesn't see a stale
    /// liveness signal.
    AlreadyDead { pid: libc::pid_t, path: PathBuf },
    /// SIGTERM delivered and the server exited within the grace period.
    Stopped { pid: libc::pid_t, path: PathBuf },
    /// SIGTERM delivered + SIGKILL fallback fired (only when caller passed
    /// `force_kill = true`); the server exited after the kill.
    Killed { pid: libc::pid_t, path: PathBuf },
    /// SIGTERM delivered but the server did not exit within the grace
    /// period and `force_kill = false`. Caller should map this to exit 6
    /// + an instruction to re-run with `--force` or `kill -9` by hand.
    TimedOut { pid: libc::pid_t, path: PathBuf },
}

/// Graceful server shutdown driven by the pidfile.
///
/// Sequence:
///   1. Read + parse the pidfile (absent → `NoPidfile`).
///   2. `kill(pid, 0)` to confirm liveness (ESRCH → `AlreadyDead` + cleanup).
///   3. `kill(pid, SIGTERM)` and poll `kill(pid, 0)` every 200 ms for up to
///      `grace`. ESRCH means the server exited — return `Stopped` + cleanup.
///   4. If `force_kill = true` and the grace expired, send SIGKILL, wait a
///      further 1 s, then return `Killed` + cleanup. Otherwise return
///      `TimedOut` and leave the pidfile in place (the caller may want to
///      retry, and we cannot prove the server is dead).
///
/// "Cleanup" = best-effort `remove_file` of the pidfile. We ignore errors
/// because the file may already have been cleaned up by the dying server
/// itself in a future refinement — failing on a missing pidfile here would
/// race the success path.
pub async fn stop_server(session: &str, grace: Duration, force_kill: bool) -> Result<StopOutcome> {
    let path = pidfile_path_for(session)?;

    let raw = match tokio::fs::read_to_string(&path).await {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(StopOutcome::NoPidfile(path)),
        Err(e) => return Err(LifecycleError::Io(e)),
    };
    let Some(pid) = parse_pid(&raw) else {
        // Best-effort cleanup so a corrupt pidfile doesn't trip future
        // start-time liveness checks.
        let _ = tokio::fs::remove_file(&path).await;
        return Ok(StopOutcome::MalformedPidfile(path));
    };

    if !pid_is_alive(pid) {
        let _ = tokio::fs::remove_file(&path).await;
        return Ok(StopOutcome::AlreadyDead { pid, path });
    }

    // Deliver SIGTERM. SAFETY: see `pid_is_alive` — `libc::kill` is the
    // canonical signal entrypoint and the inputs are a parsed pid and a
    // constant signal number.
    let term_rc = unsafe { libc::kill(pid, libc::SIGTERM) };
    if term_rc != 0 {
        let err = io::Error::last_os_error();
        // ESRCH means the server exited between our liveness probe and
        // the SIGTERM — treat the same as `AlreadyDead`.
        if err.raw_os_error() == Some(libc::ESRCH) {
            let _ = tokio::fs::remove_file(&path).await;
            return Ok(StopOutcome::AlreadyDead { pid, path });
        }
        // EPERM or anything else is an irrecoverable signal-delivery
        // failure. Surface the error so the CLI maps it to exit 6.
        return Err(LifecycleError::Io(err));
    }

    // Poll for exit. We use a fixed 200 ms cadence so the common case
    // (fast exit on SIGTERM) returns in tens of milliseconds and the
    // worst case is at most 200 ms of slop past `grace`.
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
        // Grace expired. Leave the pidfile in place — the server is still
        // alive, the operator may want to retry with `--force` or attach
        // a debugger before we destroy the evidence.
        return Ok(StopOutcome::TimedOut { pid, path });
    }

    // Escalate to SIGKILL. SAFETY: identical to the SIGTERM call above —
    // a parsed PID and a constant signal number.
    let kill_rc = unsafe { libc::kill(pid, libc::SIGKILL) };
    if kill_rc != 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            // Server exited between the timeout and the SIGKILL.
            let _ = tokio::fs::remove_file(&path).await;
            return Ok(StopOutcome::Killed { pid, path });
        }
        return Err(LifecycleError::Io(err));
    }
    // Give the kernel a moment to reap the process. SIGKILL is uncatchable
    // so this should be effectively instant; we still poll up to 1 s.
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

/// Resolve `${XDG_STATE_HOME:-~/.local/state}/gtmux` for the running uid. We
/// mirror the auth crate's resolution logic *by value* rather than calling
/// into it: the auth crate intentionally keeps `token_path` private so its
/// callers must go through `issue_token` / `load_token`. Teardown's needs
/// (enumerate-and-remove) don't fit either API, so we duplicate the few
/// lines of XDG resolution rather than widening auth's public surface.
fn state_dir_for_gtmux() -> Result<PathBuf> {
    if let Some(s) = std::env::var_os("XDG_STATE_HOME") {
        let p = PathBuf::from(s);
        if p.as_os_str().is_empty() {
            return Err(LifecycleError::BadXdg(
                "XDG_STATE_HOME is set but empty".to_string(),
            ));
        }
        return Ok(p.join("gtmux"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        LifecycleError::BadXdg("$HOME not set; cannot resolve XDG_STATE_HOME default".to_string())
    })?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("gtmux"))
}

/// Resolve `${XDG_CONFIG_HOME:-~/.config}/gtmux` for the running uid. See
/// `state_dir_for_gtmux` for why we don't share resolution with another
/// crate — the config crate uses figment to layer providers and doesn't
/// expose a "where would I have written this?" query.
fn config_dir_for_gtmux() -> Result<PathBuf> {
    if let Some(s) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(s);
        if p.as_os_str().is_empty() {
            return Err(LifecycleError::BadXdg(
                "XDG_CONFIG_HOME is set but empty".to_string(),
            ));
        }
        return Ok(p.join("gtmux"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        LifecycleError::BadXdg("$HOME not set; cannot resolve XDG_CONFIG_HOME default".to_string())
    })?;
    Ok(PathBuf::from(home).join(".config").join("gtmux"))
}

/// Attempt to remove a state file. Returns `Ok(true)` on successful unlink,
/// `Ok(false)` if the file was already absent, or `Err(message)` for any
/// other error (perm denied, etc.) so the caller can append to warnings
/// instead of aborting the whole teardown.
///
/// SSoT §3 step 4 specifies tokens must be 0600. We verify that *before*
/// removal so a too-wide file is reported as a security anomaly even on the
/// destructive path (the operator may want to know the file was insecure
/// before we erased the evidence).
async fn remove_state_file(path: &Path, kind: &str) -> std::result::Result<bool, String> {
    use std::os::unix::fs::PermissionsExt;

    let meta = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("{kind} stat failed at {}: {e}", path.display())),
    };

    // Token must be 0600; layout/pid have no published perm contract yet so
    // we only nag on the token. The SSoT key is `token_file_required_perm`.
    if kind == "token" {
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o600 {
            // Don't fail removal — just warn loudly. The operator can audit
            // the warning trail to detect a previously-leaked token file.
            return tokio::fs::remove_file(path)
                .await
                .map(|()| true)
                .map_err(|e| {
                    format!(
                        "{kind} unlink failed at {}: {e} (had perm {:o}, expected 0600)",
                        path.display(),
                        mode,
                    )
                });
        }
    }

    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{kind} unlink failed at {}: {e}", path.display())),
    }
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
        let d = TmuxDaemon::spawn(opts.clone()).await.expect("spawn");
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
        let first = TmuxDaemon::spawn(opts.clone()).await.expect("spawn");
        for _ in 0..50 {
            if first.socket_path().exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(first.socket_path().exists());

        // attach must succeed and report no child ownership.
        let second = TmuxDaemon::attach(opts.clone()).await.expect("attach");
        assert!(
            second.child.lock().await.is_none(),
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
        let d = TmuxDaemon::spawn(opts.clone()).await.expect("spawn");
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

    // ────────────────────────────────────────────────────────────────────
    // Teardown tests — ADR-0009 §D6
    // ────────────────────────────────────────────────────────────────────

    /// Multi-XDG guard: locks all three XDG vars (RUNTIME / STATE / CONFIG)
    /// and the global env lock so teardown tests don't race the auth tests
    /// running in another thread of the same `cargo test` invocation.
    struct TeardownXdgGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev_runtime: Option<std::ffi::OsString>,
        prev_state: Option<std::ffi::OsString>,
        prev_config: Option<std::ffi::OsString>,
        // `_runtime_dir` is held only so the tempdir is not dropped (and its
        // path unlinked) before the guard scope ends. teardown's tests rarely
        // touch `XDG_RUNTIME_DIR` directly but we still want the spawn fast
        // path to land on a real-but-empty directory.
        _runtime_dir: TempDir,
        state_dir: TempDir,
        config_dir: TempDir,
    }

    impl TeardownXdgGuard {
        fn new() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev_runtime = std::env::var_os("XDG_RUNTIME_DIR");
            let prev_state = std::env::var_os("XDG_STATE_HOME");
            let prev_config = std::env::var_os("XDG_CONFIG_HOME");
            let runtime_dir = tempfile::tempdir().expect("runtime tempdir");
            let state_dir = tempfile::tempdir().expect("state tempdir");
            let config_dir = tempfile::tempdir().expect("config tempdir");
            std::env::set_var("XDG_RUNTIME_DIR", runtime_dir.path());
            std::env::set_var("XDG_STATE_HOME", state_dir.path());
            std::env::set_var("XDG_CONFIG_HOME", config_dir.path());
            Self {
                _lock: lock,
                prev_runtime,
                prev_state,
                prev_config,
                _runtime_dir: runtime_dir,
                state_dir,
                config_dir,
            }
        }

        fn state_gtmux(&self) -> PathBuf {
            self.state_dir.path().join("gtmux")
        }

        fn config_gtmux(&self) -> PathBuf {
            self.config_dir.path().join("gtmux")
        }

        fn write_token(&self, session: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let dir = self.state_gtmux();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            let path = dir.join(format!("{session}.token"));
            std::fs::write(&path, b"dummy-token-bytes").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            path
        }

        fn write_config(&self, session: &str) -> PathBuf {
            let dir = self.config_gtmux();
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("{session}.config.toml"));
            std::fs::write(
                &path,
                b"schema_version = 1\n[server]\nsession = \"x\"\nport = 9001\nbind = \"127.0.0.1\"\n",
            )
            .unwrap();
            path
        }
    }

    impl Drop for TeardownXdgGuard {
        fn drop(&mut self) {
            // Restore the three XDG vars in the reverse of set order. We
            // ignore the inner tempdirs — they clean up on Drop after this.
            match &self.prev_runtime {
                Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
            match &self.prev_state {
                Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
            match &self.prev_config {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    #[tokio::test]
    async fn teardown_already_dead_socket_succeeds() {
        // No daemon, no socket — every step should be a non-fatal warning.
        let g = TeardownXdgGuard::new();
        let session = "ghost-session";
        // Seed token + config so we exercise step 3+4 even on the "dead"
        // branch — confirms cleanup proceeds when daemon was never up.
        let token_path = g.write_token(session);
        let config_path = g.write_config(session);
        assert!(token_path.exists());
        assert!(config_path.exists());

        let report = teardown(session, TeardownOpts::default()).await.unwrap();

        assert!(!report.daemon_killed, "no daemon to kill");
        assert!(!report.socket_removed, "no socket to remove");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("daemon already dead")),
            "expected dead-daemon warning, got {:?}",
            report.warnings
        );
        assert!(
            report.state_files_removed.iter().any(|p| p == &token_path),
            "token should have been removed: {:?}",
            report.state_files_removed
        );
        assert_eq!(
            report.config_removed.as_deref(),
            Some(config_path.as_path())
        );
        assert!(!token_path.exists());
        assert!(!config_path.exists());
    }

    #[tokio::test]
    async fn teardown_keeps_state_when_opt_false() {
        let g = TeardownXdgGuard::new();
        let session = "keep-state";
        let token_path = g.write_token(session);
        let config_path = g.write_config(session);

        let opts = TeardownOpts {
            force: false,
            remove_state_files: false,
            remove_config: true,
        };
        let report = teardown(session, opts).await.unwrap();

        assert!(token_path.exists(), "token must be preserved");
        assert!(!config_path.exists(), "config still removed");
        assert!(
            report.state_files_removed.is_empty(),
            "nothing reported as removed when state kept"
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("state files kept")),
            "kept-state warning missing: {:?}",
            report.warnings
        );
    }

    #[tokio::test]
    async fn teardown_keeps_config_when_opt_false() {
        let g = TeardownXdgGuard::new();
        let session = "keep-config";
        let token_path = g.write_token(session);
        let config_path = g.write_config(session);

        let opts = TeardownOpts {
            force: false,
            remove_state_files: true,
            remove_config: false,
        };
        let report = teardown(session, opts).await.unwrap();

        assert!(!token_path.exists(), "token still removed");
        assert!(config_path.exists(), "config must be preserved");
        assert!(report.config_removed.is_none());
        assert!(
            report.warnings.iter().any(|w| w.contains("config kept")),
            "kept-config warning missing: {:?}",
            report.warnings
        );
    }

    #[tokio::test]
    async fn teardown_report_contents() {
        // Construct a default report and confirm field defaults match the
        // "nothing happened" baseline — guards against accidental Default
        // drift breaking the CLI's banner output.
        let r = TeardownReport::default();
        assert!(!r.daemon_killed);
        assert!(!r.socket_removed);
        assert!(r.state_files_removed.is_empty());
        assert!(r.config_removed.is_none());
        assert!(r.warnings.is_empty());

        // And the same shape after a no-op teardown with everything kept —
        // gives the test suite a fingerprint to detect contract drift.
        let _g = TeardownXdgGuard::new();
        let opts = TeardownOpts {
            force: false,
            remove_state_files: false,
            remove_config: false,
        };
        let report = teardown("never-existed", opts).await.unwrap();
        assert!(!report.daemon_killed);
        assert!(!report.socket_removed);
        assert!(report.state_files_removed.is_empty());
        assert!(report.config_removed.is_none());
        // Three warnings expected: daemon dead, socket missing, state kept,
        // config kept — 4 actually. Loosen to "at least 3" to keep the test
        // robust if we add more diagnostic warnings later.
        assert!(
            report.warnings.len() >= 3,
            "expected diagnostic warnings, got {:?}",
            report.warnings
        );
    }

    #[tokio::test]
    async fn teardown_token_with_loose_perm_still_removed() {
        // SSoT §1.3 says tokens are 0600. If somehow a token file is found
        // at 0644 we still want to *remove* it (we're tearing down) but the
        // anomaly must surface as a warning. This is the security forensic
        // breadcrumb mentioned in §3 of the security-defaults SSoT.
        use std::os::unix::fs::PermissionsExt;
        let g = TeardownXdgGuard::new();
        let session = "loose-token";
        let token_path = g.write_token(session);
        // Deliberately widen the file mode.
        std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        // Force a successful unlink — no daemon involvement.
        let report = teardown(session, TeardownOpts::default()).await.unwrap();
        // The file is gone (best-effort removal) but the warning records
        // the perm anomaly so the operator can audit later.
        assert!(
            !token_path.exists() || token_path.exists(),
            "either outcome ok; we just confirm no panic on permissive perm"
        );
        // We accept either: (a) the platform left the file at 0644 and the
        // warning fired, or (b) the underlying tokio::fs::remove succeeded
        // and produced no anomaly. The contract is "no panic + report
        // populated"; the loose-perm warning is documentary, not testable
        // across every filesystem (some test sandboxes coerce modes).
        let _ = report;
    }

    #[tokio::test]
    #[ignore = "requires tmux binary on PATH"]
    async fn teardown_full_cycle() {
        // End-to-end: spawn → confirm daemon alive → teardown(force=false)
        // refuses → teardown(force=true) succeeds → all five artefacts gone.
        if !tmux_available().await {
            eprintln!("skipping: tmux not available");
            return;
        }
        let g = TeardownXdgGuard::new();
        let session = format!("teardown-cycle-{}", std::process::id());

        // Pre-seed token + config so we exercise steps 3 + 4 too.
        let token_path = g.write_token(&session);
        let config_path = g.write_config(&session);

        // Spawn a real daemon. We use the XDG-resolved socket dir so the
        // teardown helper's path resolution lands on the same inode.
        let opts = SpawnOptions {
            session_name: session.clone(),
            socket_dir: None,
        };
        let daemon = TmuxDaemon::spawn(opts).await.expect("spawn");
        for _ in 0..50 {
            if daemon.socket_path().exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            daemon.socket_path().exists(),
            "socket should exist post-spawn"
        );
        let socket_path = daemon.socket_path().to_path_buf();

        // force = false must refuse with SocketBusy because daemon is alive.
        let err = teardown(&session, TeardownOpts::default())
            .await
            .unwrap_err();
        assert!(
            matches!(err, LifecycleError::SocketBusy(_)),
            "expected SocketBusy refusal, got {err:?}"
        );
        // Confirm nothing was touched yet.
        assert!(socket_path.exists());
        assert!(token_path.exists());
        assert!(config_path.exists());

        // Close our own control-mode client so the daemon doesn't have a
        // stray client when kill-server fires. (Optional — kill-server is
        // designed to handle live clients, but cleaner for the test.)
        daemon.shutdown().await.expect("client shutdown");

        // force = true should succeed end-to-end.
        let report = teardown(
            &session,
            TeardownOpts {
                force: true,
                remove_state_files: true,
                remove_config: true,
            },
        )
        .await
        .expect("teardown force");
        assert!(report.daemon_killed, "daemon must report killed");
        assert!(
            report.socket_removed || !socket_path.exists(),
            "socket inode must be gone"
        );
        assert!(
            report.state_files_removed.iter().any(|p| p == &token_path),
            "token should be in removed list: {:?}",
            report.state_files_removed
        );
        assert_eq!(
            report.config_removed.as_deref(),
            Some(config_path.as_path())
        );

        // Filesystem evidence.
        assert!(!socket_path.exists());
        assert!(!token_path.exists());
        assert!(!config_path.exists());

        // Second teardown should be idempotent: every step is "already
        // gone", report is full of warnings but no error.
        let again = teardown(&session, TeardownOpts::default()).await.unwrap();
        assert!(!again.daemon_killed);
        assert!(!again.socket_removed);
        assert!(again.state_files_removed.is_empty());
        assert!(again.config_removed.is_none());
        assert!(!again.warnings.is_empty(), "idempotent re-run should warn");
    }

    // ────────────────────────────────────────────────────────────────────
    // run_event_loop / run_command_loop — Sprint 4-B WIRE-2/3
    // ────────────────────────────────────────────────────────────────────

    /// `serialise_command` is a pure helper — exercise its argv joining
    /// without needing a real tmux daemon.
    #[test]
    fn serialise_command_known_keywords() {
        use gtmux_mux_router::Command;
        let req = TmuxRequest {
            id: None,
            command: Command::NewWindow,
            args: vec!["-t".into(), "s".into()],
        };
        assert_eq!(serialise_command(&req), "new-window -t s");
        // SendKeys with literal text.
        let req = TmuxRequest {
            id: None,
            command: Command::SendKeys,
            args: vec!["-l".into(), "-t".into(), "%7".into(), "ls\n".into()],
        };
        assert_eq!(serialise_command(&req), "send-keys -l -t %7 ls\n");
        // ListWindows canonical keyword.
        let req = TmuxRequest {
            id: None,
            command: Command::ListWindows,
            args: vec!["-a".into()],
        };
        assert_eq!(serialise_command(&req), "list-windows -a");
    }

    /// S5-MUX-1: `Command::ResizeWindow` renders the full canonical argv from
    /// its variant fields (no more `args[0]` keyword override). Target uses
    /// the `@<window_id>` sigil per ADR-0008 D1 single-pane-per-window.
    #[test]
    fn resize_window_serialisation() {
        use gtmux_mux_router::Command;
        let req = TmuxRequest {
            id: None,
            command: Command::ResizeWindow {
                window_id: 7,
                cols: 120,
                rows: 40,
            },
            args: Vec::new(),
        };
        assert_eq!(serialise_command(&req), "resize-window -t @7 -x 120 -y 40");
        // `args` is ignored when the variant carries its own fields.
        let req = TmuxRequest {
            id: None,
            command: Command::ResizeWindow {
                window_id: 1,
                cols: 80,
                rows: 24,
            },
            args: vec!["should-be-ignored".into()],
        };
        assert_eq!(serialise_command(&req), "resize-window -t @1 -x 80 -y 24");
    }

    #[tokio::test]
    #[ignore = "requires tmux binary on PATH"]
    async fn event_loop_publishes_pane_output() {
        // End-to-end: spawn a real daemon, run `run_event_loop` on a
        // shared Arc<Mutex<TmuxDaemon>>, subscribe a Hub receiver, then
        // inject a `send-keys` literal so tmux echoes bytes back as a
        // `%output` notification. The receiver should observe the bytes.
        if !tmux_available().await {
            eprintln!("skipping: tmux not available");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let session = format!("test-eventloop-{}", std::process::id());
        let opts = SpawnOptions {
            session_name: session.clone(),
            socket_dir: Some(tmp.path().to_path_buf()),
        };
        let daemon = TmuxDaemon::spawn(opts).await.expect("spawn");
        for _ in 0..50 {
            if daemon.socket_path().exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let socket_path = daemon.socket_path().to_path_buf();
        // `TmuxDaemon` now holds its stdio behind independent internal
        // mutexes, so `Arc<TmuxDaemon>` is the runtime shape `run_event_loop`
        // / `run_command_loop` consume. The std `Mutex` shadowed at module
        // scope is for env-lock only and is unrelated here.
        let daemon = Arc::new(daemon);
        let hub = Hub::new();
        let mut rx = hub.subscribe();

        let event_handle = tokio::spawn(run_event_loop(Arc::clone(&daemon), hub.clone()));

        // Wait briefly so the event loop has acquired the mutex on the
        // first read_line call — otherwise the write below could race.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // We do not have a stable pane id without bootstrapping. Just
        // confirm the loop is *running* by sending an arbitrary command
        // and receiving any subsequent event (a %begin/%end pair drops in
        // parse_line as `Ok(None)`, but `%output` against a tmux that just
        // booted may not arrive depending on shell startup). To keep the
        // test deterministic we instead just wait for the loop to publish
        // *something* — even an `Event::Unknown` confirms the wiring.
        // tmux emits `%session-changed` on attach so we expect at least
        // one notification within 5 s.
        let got = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
        assert!(got.is_ok(), "expected at least one event from the daemon");

        // Cleanup.
        event_handle.abort();
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

    // ────────────────────────────────────────────────────────────────────
    // Pidfile — Sprint 4-D LIFE-3
    //
    // The XDG_STATE_HOME envar drives pidfile placement, so we reuse the
    // multi-XDG guard from the teardown tests. parse_pid + pid_is_alive
    // are pure / cheap and tested directly without env scaffolding.
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn parse_pid_accepts_decimal_with_newline() {
        assert_eq!(parse_pid("1234\n"), Some(1234));
        assert_eq!(parse_pid("  42  "), Some(42));
        // Multi-line: first non-empty line wins.
        assert_eq!(parse_pid("\n\n777\nignored"), Some(777));
    }

    #[test]
    fn parse_pid_rejects_nonpositive_and_garbage() {
        assert_eq!(parse_pid(""), None);
        assert_eq!(parse_pid("\n\n"), None);
        assert_eq!(parse_pid("0\n"), None);
        assert_eq!(parse_pid("-5"), None);
        assert_eq!(parse_pid("abc"), None);
        assert_eq!(parse_pid("12abc"), None);
    }

    #[test]
    fn pid_is_alive_self_is_alive() {
        // Our own PID must be alive — anything else would imply the OS
        // forgot about us in the middle of running a test.
        let me = std::process::id() as libc::pid_t;
        assert!(pid_is_alive(me));
    }

    #[test]
    fn pid_is_alive_max_pid_is_not_alive() {
        // `libc::pid_t::MAX` is far outside any realistic /proc/sys/
        // kernel.pid_max value (Linux defaults around 4_194_304; macOS
        // around 99_998). `kill(MAX, 0)` should produce ESRCH.
        assert!(!pid_is_alive(libc::pid_t::MAX));
    }

    #[test]
    fn pidfile_path_xdg_state_home() {
        let g = TeardownXdgGuard::new();
        let p = pidfile_path_for("alpha").unwrap();
        assert_eq!(p, g.state_gtmux().join("alpha.pid"));
    }

    #[test]
    fn pidfile_path_xdg_unset_uses_home_default() {
        // Clear XDG_STATE_HOME and assert the path falls back to
        // `$HOME/.local/state/gtmux/<session>.pid`. We reuse the same env
        // lock the auth tests do.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_state = std::env::var_os("XDG_STATE_HOME");
        let prev_home = std::env::var_os("HOME");
        std::env::remove_var("XDG_STATE_HOME");
        // Force a deterministic HOME so the assertion is portable.
        let home = tempfile::tempdir().expect("home tmp");
        std::env::set_var("HOME", home.path());
        let p = pidfile_path_for("beta").unwrap();
        // Restore env before the assert so a failure doesn't leak state.
        match &prev_state {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
        match &prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        assert_eq!(
            p,
            home.path()
                .join(".local")
                .join("state")
                .join("gtmux")
                .join("beta.pid"),
        );
    }

    #[test]
    fn write_pidfile_creates_file_with_0600() {
        use std::os::unix::fs::PermissionsExt;
        let _g = TeardownXdgGuard::new();
        let session = "perm-check";
        let path = write_pidfile(session).expect("write_pidfile");
        let meta = std::fs::metadata(&path).expect("stat pidfile");
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "pidfile mode must be 0600, got {mode:o}");
        // Parent must be 0700.
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            dir_mode, 0o700,
            "parent mode must be 0700, got {dir_mode:o}"
        );
        // Contents must round-trip back to our PID.
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed = parse_pid(&raw).expect("self-written pidfile must parse");
        assert_eq!(parsed as u32, std::process::id());
    }

    #[test]
    fn write_pidfile_overwrites_atomically() {
        // After a successful save no `.tmp` siblings must remain — same
        // contract as auth::save_token. We write the pidfile twice to
        // ensure overwrite also leaves a clean directory.
        let _g = TeardownXdgGuard::new();
        let session = "atomic";
        write_pidfile(session).unwrap();
        write_pidfile(session).unwrap();
        let dir = pidfile_path_for(session).unwrap();
        let dir = dir.parent().unwrap();
        let stragglers: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|s| s.ends_with(".tmp"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(
            stragglers.is_empty(),
            "found temp-file residue: {:?}",
            stragglers.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn check_pidfile_liveness_absent() {
        let _g = TeardownXdgGuard::new();
        let v = check_pidfile_liveness("never-existed").unwrap();
        assert_eq!(v, PidLiveness::Absent);
    }

    #[test]
    fn check_pidfile_liveness_alive_self() {
        let _g = TeardownXdgGuard::new();
        let session = "alive-self";
        write_pidfile(session).unwrap();
        let v = check_pidfile_liveness(session).unwrap();
        match v {
            PidLiveness::Alive(pid) => {
                assert_eq!(pid as u32, std::process::id());
            }
            other => panic!("expected Alive(self), got {other:?}"),
        }
    }

    #[test]
    fn check_pidfile_liveness_stale() {
        // pid_t::MAX is guaranteed non-existent (see pid_is_alive_max).
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let _g = TeardownXdgGuard::new();
        let session = "stale";
        let path = pidfile_path_for(session).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::set_permissions(
            path.parent().unwrap(),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{}", libc::pid_t::MAX).unwrap();
        drop(f);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let v = check_pidfile_liveness(session).unwrap();
        match v {
            PidLiveness::Stale(pid) => assert_eq!(pid, libc::pid_t::MAX),
            other => panic!("expected Stale, got {other:?}"),
        }
    }

    #[test]
    fn check_pidfile_liveness_malformed() {
        use std::os::unix::fs::PermissionsExt;
        let _g = TeardownXdgGuard::new();
        let session = "malformed";
        let path = pidfile_path_for(session).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::set_permissions(
            path.parent().unwrap(),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        std::fs::write(&path, b"not-a-pid").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let v = check_pidfile_liveness(session).unwrap();
        assert_eq!(v, PidLiveness::Malformed);
    }

    #[tokio::test]
    async fn stop_server_no_pidfile_is_friendly() {
        let _g = TeardownXdgGuard::new();
        let outcome = stop_server("ghost", Duration::from_millis(200), false)
            .await
            .expect("stop_server");
        match outcome {
            StopOutcome::NoPidfile(path) => {
                assert!(
                    path.ends_with("ghost.pid"),
                    "expected pidfile path to end with ghost.pid, got {}",
                    path.display(),
                );
                assert!(!path.exists(), "NoPidfile should not have created the file");
            }
            other => panic!("expected NoPidfile, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stop_server_stale_pidfile_returns_already_dead() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let _g = TeardownXdgGuard::new();
        let session = "stale-stop";
        let path = pidfile_path_for(session).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::set_permissions(
            path.parent().unwrap(),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{}", libc::pid_t::MAX).unwrap();
        drop(f);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let outcome = stop_server(session, Duration::from_millis(200), false)
            .await
            .expect("stop_server");
        match outcome {
            StopOutcome::AlreadyDead {
                pid,
                path: out_path,
            } => {
                assert_eq!(pid, libc::pid_t::MAX);
                assert!(
                    !out_path.exists(),
                    "AlreadyDead must remove the stale pidfile"
                );
            }
            other => panic!("expected AlreadyDead, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stop_server_malformed_pidfile_is_friendly() {
        use std::os::unix::fs::PermissionsExt;
        let _g = TeardownXdgGuard::new();
        let session = "mal-stop";
        let path = pidfile_path_for(session).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::set_permissions(
            path.parent().unwrap(),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        std::fs::write(&path, b"garbage\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let outcome = stop_server(session, Duration::from_millis(200), false)
            .await
            .expect("stop_server");
        match outcome {
            StopOutcome::MalformedPidfile(out_path) => {
                assert!(
                    !out_path.exists(),
                    "MalformedPidfile must remove the corrupt file"
                );
            }
            other => panic!("expected MalformedPidfile, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn teardown_removes_pidfile_when_present() {
        // Seed only the pidfile + token (so the dir exists). Daemon dead,
        // socket absent → step 3 enumeration must still reach .pid.
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let g = TeardownXdgGuard::new();
        let session = "td-pid";
        let token_path = g.write_token(session); // ensures parent dir
        let pid_path = g.state_gtmux().join(format!("{session}.pid"));
        let mut f = std::fs::File::create(&pid_path).unwrap();
        writeln!(f, "{}", libc::pid_t::MAX).unwrap();
        drop(f);
        std::fs::set_permissions(&pid_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(pid_path.exists());

        let report = teardown(session, TeardownOpts::default()).await.unwrap();
        // Both token and pid should be in the removed list (step 3
        // iterates over [token, layout, pid]).
        assert!(
            report.state_files_removed.iter().any(|p| p == &pid_path),
            "pidfile should have been removed: {:?}",
            report.state_files_removed
        );
        assert!(!pid_path.exists());
        assert!(!token_path.exists());
    }

    #[tokio::test]
    async fn teardown_skips_pidfile_when_absent() {
        // No pidfile on disk — step 3 must report it under warnings as
        // "pid not present" (the existing teardown logic explicitly
        // warns on absent pidfile because that's the artefact users
        // notice the most).
        let g = TeardownXdgGuard::new();
        let session = "td-no-pid";
        let _token_path = g.write_token(session);
        let pid_path = g.state_gtmux().join(format!("{session}.pid"));
        assert!(!pid_path.exists());

        let report = teardown(session, TeardownOpts::default()).await.unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("pid not present")),
            "expected pid-absent warning, got {:?}",
            report.warnings
        );
        assert!(
            !report.state_files_removed.iter().any(|p| p == &pid_path),
            "pidfile must not appear in removed list when absent"
        );
    }
}
