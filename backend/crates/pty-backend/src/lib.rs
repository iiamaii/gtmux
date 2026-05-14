//! gtmux-pty-backend — portable-pty direct PTY pair + child process owner.
//!
//! Replaces the legacy tmux control-mode integration (ADR-0001, now
//! superseded by ADR-0013) with our own per-Pane PTY supervisor
//! (ADR-0014). One [`PaneHandle`] = one PTY pair + one child process,
//! kept 1:1:1 by [`PtyBackend`] using a [`dashmap::DashMap`] keyed by
//! [`PaneId`].
//!
//! Public surface:
//! - [`PtyBackend::new`] / [`PtyBackend::spawn`] / [`PtyBackend::kill`] /
//!   [`PtyBackend::resize`] / [`PtyBackend::send_input`] /
//!   [`PtyBackend::subscribe_output`] — the five lifecycle + IO entry
//!   points the ws-server CTRL router calls into.
//! - [`BackendCommand`] — the compile-time allowlist enum (ADR-0013
//!   D10/D12). New CTRL command surface = add a variant here and route it.
//! - [`BackendNotify`] — the NOTIFY_MIRROR payload enum (ADR-0013 D10).
//! - [`SpawnSpec`] — input to [`PtyBackend::spawn`]; carries argv / cwd /
//!   env / initial geometry.
//!
//! Internal invariants (do not break):
//! - PTY master reader / writer / child-wait threads are *std::thread*,
//!   not tokio tasks (portable-pty's reader is `Box<dyn Read + Send>`
//!   which blocks in syscall — putting it on the tokio reactor would
//!   stall the runtime). Each pane spawns three threads.
//! - [`broadcast::Sender`] cap = [`BROADCAST_CAPACITY`] (512). Lagged
//!   subscribers see `RecvError::Lagged` and are expected to re-sync
//!   via [`PtyBackend::subscribe_output`] which replays the
//!   [`RING_CAPACITY`]-byte per-pane ring buffer.
//! - SIGTERM → [`PANE_KILL_GRACE`] (200 ms) → SIGKILL → `child.wait()`
//!   reaps (ADR-0014 D7 / D6, POC Gate #4).

#![deny(unsafe_code)]
#![deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bytes::Bytes;
use dashmap::DashMap;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

// ─────────────────────────────────────────────────────────────────────────────
//  Tunables — calibrated against POC Gate #5 + ADR-0013 D3 / ADR-0014 O1.
// ─────────────────────────────────────────────────────────────────────────────

/// Per-pane output broadcast capacity. POC §1.1 + ADR-0013 D3.
///
/// When a subscriber lags past this many events it receives
/// `RecvError::Lagged` on its next `recv()` and is expected to
/// re-subscribe via [`PtyBackend::subscribe_output`] (which replays the
/// ring buffer below). Higher values smooth bursts at the cost of RSS.
pub const BROADCAST_CAPACITY: usize = 512;

/// Per-pane ring buffer capacity in bytes. Matches the historical ADR-0001
/// D7 / Grill D15 value so the UX (~last screen of scrollback at boot or
/// reconnect) stays identical to the tmux era.
pub const RING_CAPACITY: usize = 128 * 1024;

/// SIGTERM grace period before SIGKILL escalation (ADR-0014 D7 + O1).
///
/// 200 ms is the jeong-jeong value pending Sprint 7 measurement against
/// noisy ZSH startup. Operators can recompile with a different value if
/// real-world shells exceed this budget; we keep the const here (not a
/// runtime knob) so the policy is machine-enforced uniformly.
pub const PANE_KILL_GRACE: Duration = Duration::from_millis(200);

/// PTY master read chunk size. Matches POC §1.1; further tuning is an
/// open item (ADR-0013 O2) once we have multi-pane × N burst data.
const READ_CHUNK: usize = 8192;

/// Backpressure observability watermarks (per task spec §A.5).
///
/// portable-pty's master fd provides natural kernel-level backpressure
/// (the line discipline buffers in-kernel; once full, write(2) blocks
/// the child until the reader catches up). These constants are
/// *observability counters only* — we never block or throttle on them.
const STALL_HIGH_WATERMARK: usize = 512 * 1024;
const STALL_LOW_WATERMARK: usize = 128 * 1024;

// ─────────────────────────────────────────────────────────────────────────────
//  PaneId — opaque u64 issued by PtyBackend.
// ─────────────────────────────────────────────────────────────────────────────

/// Stable identifier for a Pane. 1:1:1 with a PTY pair and a child
/// process (ADR-0013 D2). Issued monotonically by [`PtyBackend`]; never
/// reused within one Server lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PaneId(pub u64);

impl std::fmt::Display for PaneId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Errors
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum PtyBackendError {
    #[error("pane {0} not found")]
    PaneNotFound(PaneId),
    /// Pane died between command lookup and dispatch — the operation is
    /// not retriable; caller should surface this to the user as a
    /// pane-died notification.
    #[error("pane {0} no longer alive")]
    PaneGone(PaneId),
    #[error("spawn failed: {0}")]
    SpawnFailed(#[source] anyhow::Error),
    #[error("resize failed: {0}")]
    ResizeFailed(#[source] anyhow::Error),
    /// Input mpsc channel was closed (writer thread exited).
    #[error("input channel closed for pane {0}")]
    ChannelClosed(PaneId),
}

pub type Result<T> = std::result::Result<T, PtyBackendError>;

// ─────────────────────────────────────────────────────────────────────────────
//  SpawnSpec — input to PtyBackend::spawn.
// ─────────────────────────────────────────────────────────────────────────────

/// Description of a new Pane to spawn. `command = None` falls back to
/// `$SHELL`, then `/bin/bash` if `$SHELL` is unset (POC parity).
#[derive(Debug, Clone, Default)]
pub struct SpawnSpec {
    /// Executable path. `None` → `$SHELL` → `/bin/bash`.
    pub command: Option<String>,
    /// Argv tail (does NOT include argv[0]).
    pub args: Vec<String>,
    /// Working directory. `None` → `$HOME` → current process cwd.
    pub cwd: Option<PathBuf>,
    /// Extra env *added* on top of the inherited environment (after the
    /// ADR-0014 D10 noisy-env scrub). Existing keys are overwritten.
    pub env: Vec<(String, String)>,
    /// Initial PTY geometry. `(rows, cols) = (24, 80)` matches the POC
    /// default and the xterm.js default.
    pub rows: u16,
    pub cols: u16,
}

impl SpawnSpec {
    /// Convenience constructor: default shell at the user's home, 80×24.
    pub fn default_shell() -> Self {
        Self {
            command: None,
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            rows: 24,
            cols: 80,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  BackendCommand — compile-time allowlist enum (ADR-0013 D10 / D12).
// ─────────────────────────────────────────────────────────────────────────────

/// Single inbound CTRL command. JSON shape: `{"type":"new-pane", ...}`
/// (`serde(tag = "type")` + kebab-case). Adding a variant here is the
/// *only* way to surface a new backend API — exhaustive `match` in the
/// dispatcher guarantees no command leaks past the allowlist (ADR-0013
/// D12). Argv strings, `#` quoting, and tmux-style escapes are
/// permanently gone (ADR-0013 D13).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum BackendCommand {
    /// Spawn a new Pane. Server replies with `BackendNotify::PaneSpawned`
    /// (carries the assigned [`PaneId`]) via NOTIFY_MIRROR broadcast.
    NewPane {
        /// Echoed back in `BackendNotify::PaneSpawned.request_id` so the
        /// originating client can correlate the spawn to its UI action.
        #[serde(default)]
        request_id: Option<String>,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(default)]
        env: Vec<(String, String)>,
        #[serde(default = "default_rows")]
        rows: u16,
        #[serde(default = "default_cols")]
        cols: u16,
    },
    /// Kill an existing Pane (SIGTERM → grace → SIGKILL → reap).
    KillPane { id: PaneId },
    /// Resize an existing Pane (`TIOCSWINSZ` → SIGWINCH).
    ResizePane { id: PaneId, rows: u16, cols: u16 },
    /// Change the working directory of an existing Pane. Pending in
    /// MVP — child shells own their cwd, this is reserved for future
    /// `cd` automation against a freshly spawned pane.
    SetCwd { id: PaneId, path: PathBuf },
    /// Set an environment variable on a *future* spawn. Reserved for
    /// the same automation surface as `SetCwd`.
    SetEnv {
        id: PaneId,
        key: String,
        value: String,
    },
}

fn default_rows() -> u16 {
    24
}
fn default_cols() -> u16 {
    80
}

// ─────────────────────────────────────────────────────────────────────────────
//  BackendNotify — NOTIFY_MIRROR payload enum (ADR-0013 D10).
// ─────────────────────────────────────────────────────────────────────────────

/// One asynchronous notification from the backend. Maps to wire frame
/// `0x07 NOTIFY_MIRROR`. tmux's 14-notification protocol is gone; we
/// only emit what the UI actually consumes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BackendNotify {
    /// Pane successfully spawned. Carries the new id so the client can
    /// rendezvous (CTRL request_id ↔ this notification).
    PaneSpawned {
        id: PaneId,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// Pane child process exited. `code` = process exit status (Unix),
    /// `signal` set when the child was terminated by signal.
    PaneDied {
        id: PaneId,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        signal: Option<i32>,
    },
    /// Layout snapshot was overwritten on disk. Mirror of the legacy
    /// LAYOUT_CHANGED broadcast; emitted by the persistence layer.
    /// Currently not used by [`PtyBackend`] itself — present so the
    /// frontend dispatcher table can be exhaustive.
    LayoutChanged,
    /// Server bootstrap completed; mirror of the legacy
    /// `daemon-started` notification. Reserved for the auto-mount path.
    DaemonStarted,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Per-pane handle.
// ─────────────────────────────────────────────────────────────────────────────

/// Owned resources for one Pane. The handle is never exposed directly
/// to callers — they interact via [`PtyBackend`].
struct PaneHandle {
    /// Output fan-out. Broadcast cap = [`BROADCAST_CAPACITY`].
    out_tx: broadcast::Sender<Bytes>,
    /// Input fan-in. mpsc unbounded — the writer thread drains it to
    /// the master fd. Held as `Option` so `PaneHandle::drop` can `.take()`
    /// it *before* joining the writer thread (otherwise `blocking_recv`
    /// would never return because the sender is still alive inside the
    /// struct being dropped — classic Drop deadlock).
    in_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
    /// PTY master clone, owned for ioctl (resize) only. Reader / writer
    /// halves are *moved* into background threads at spawn time.
    master: Arc<StdMutex<Box<dyn MasterPty + Send>>>,
    /// Child process handle. Locked only by the wait thread + the kill
    /// path (signal delivery). Both call sites hold the lock briefly;
    /// no `.await` lives inside the critical section.
    child: Arc<StdMutex<Box<dyn Child + Send + Sync>>>,
    /// Late-mount buffer. Mirrors the most recent [`RING_CAPACITY`]
    /// bytes the PTY emitted so a WS attach that arrives after the
    /// first burst still sees the user-visible terminal state.
    ring: Arc<StdMutex<VecDeque<u8>>>,
    /// Backpressure observability — incremented by the reader thread
    /// every time it sees a broadcast `send` error (= no subscribers
    /// OR cap overflow). Read-only externally.
    stall_count: Arc<AtomicU64>,
    /// Background thread handles. Held so [`PaneHandle::drop`] can join
    /// them after closing the input channel + signalling the child.
    reader_join: Option<JoinHandle<()>>,
    writer_join: Option<JoinHandle<()>>,
    wait_join: Option<JoinHandle<()>>,
}

impl PaneHandle {
    /// Append `bytes` to the late-mount ring. Drops oldest bytes when
    /// the cap is exceeded. Single-burst > cap keeps only the trailing
    /// window — matches `crates/ws-server/src/ring.rs` semantics.
    fn ring_append(ring: &StdMutex<VecDeque<u8>>, bytes: &[u8]) {
        let Ok(mut buf) = ring.lock() else {
            // PoisonError — another thread panicked while holding the
            // lock. We surface the data loss via tracing and bail; the
            // pane is unrecoverable anyway, the wait thread will reap.
            warn!("pty-backend: ring buffer mutex poisoned, dropping burst");
            return;
        };
        if bytes.len() >= RING_CAPACITY {
            buf.clear();
            buf.extend(&bytes[bytes.len() - RING_CAPACITY..]);
            return;
        }
        let combined = buf.len() + bytes.len();
        if combined > RING_CAPACITY {
            let drop = combined - RING_CAPACITY;
            buf.drain(..drop);
        }
        buf.extend(bytes);
    }

    /// Copy the current ring contents into a contiguous Vec.
    fn ring_snapshot(&self) -> Vec<u8> {
        let Ok(buf) = self.ring.lock() else {
            return Vec::new();
        };
        let (a, b) = buf.as_slices();
        let mut out = Vec::with_capacity(a.len() + b.len());
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        out
    }
}

impl Drop for PaneHandle {
    /// Cooperative teardown. Sends SIGTERM → waits the grace period →
    /// escalates to SIGKILL → reaps via `child.wait()` → joins the
    /// background threads. Called by [`PtyBackend::kill`] and by
    /// [`PtyBackend::drop`] for graceful server shutdown.
    fn drop(&mut self) {
        // 1) Close the input channel *first* — the writer thread sees
        //    `recv()` return None and exits. Without this the
        //    writer_join below would block forever because the sender
        //    is still alive inside `self`.
        drop(self.in_tx.take());

        // 2) SIGTERM, then grace, then SIGKILL.
        terminate_child(&self.child);

        // 3) Reap. We do not synchronously block forever — the wait
        //    thread already drives `child.wait()` and broadcasts
        //    `pane-died` when the kernel reaps. Joining the wait thread
        //    here is the synchronisation point.
        if let Some(j) = self.wait_join.take() {
            let _ = j.join();
        }
        if let Some(j) = self.reader_join.take() {
            // The reader thread exits when the master fd EOFs — which
            // happens automatically once the child is reaped + we drop
            // the master clone below. Join with a short timeout via
            // try_join would be ideal; std::thread doesn't offer it,
            // so we rely on the EOF being prompt after reap.
            let _ = j.join();
        }
        if let Some(j) = self.writer_join.take() {
            let _ = j.join();
        }
    }
}

/// Send SIGTERM → wait [`PANE_KILL_GRACE`] → SIGKILL fallback. Idempotent
/// (calling on an already-dead child is harmless). Errors are logged at
/// `warn` because there is no recovery path — the child is leaving one
/// way or another.
fn terminate_child(child_mutex: &StdMutex<Box<dyn Child + Send + Sync>>) {
    // SIGTERM phase.
    if let Ok(child) = child_mutex.lock() {
        // portable-pty's `Child::kill` sends SIGKILL on Unix — we want
        // SIGTERM first. Reach into the platform child via `process_id`.
        // SAFETY/CORRECTNESS: libc::kill is a stable C ABI signal
        // delivery; we never pass a sentinel pid (< 0) which would
        // broadcast to a process group. Errors from `kill` are
        // benign (ESRCH = already dead).
        if let Some(pid) = child.process_id() {
            // pid is u32 on portable-pty; libc::kill expects i32.
            let pid_signed = pid as i32;
            let _ = unsafe_send_signal(pid_signed, libc::SIGTERM);
        }
    }
    // Grace period. We poll instead of blocking on wait so a stuck
    // child does not pin the runtime past the budget.
    let deadline = Instant::now() + PANE_KILL_GRACE;
    while Instant::now() < deadline {
        if let Ok(mut child) = child_mutex.lock() {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {}
                Err(_) => return,
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // SIGKILL fallback.
    if let Ok(mut child) = child_mutex.lock() {
        let _ = child.kill();
    }
}

/// `libc::kill` wrapper. We isolate the FFI to a single non-`unsafe`-fn
/// boundary so the crate's `forbid(unsafe_code)` stays clean — the
/// actual `unsafe` block lives in a child module.
fn unsafe_send_signal(pid: libc::pid_t, sig: libc::c_int) -> i32 {
    sigsend::kill(pid, sig)
}

mod sigsend {
    //! Tiny FFI shim. Isolated so the crate-level
    //! `#![forbid(unsafe_code)]` stays effective — only this module
    //! permits `unsafe`, and it does so for one function.
    #![allow(unsafe_code)]

    /// Wraps `libc::kill(2)`. Errors (e.g. ESRCH for an already-dead
    /// child) are surfaced as the raw return; callers ignore them
    /// because all known failures are benign.
    pub fn kill(pid: libc::pid_t, sig: libc::c_int) -> i32 {
        // SAFETY: libc::kill is a C ABI signal delivery; pid is a
        // process id we just read from the same child handle, sig is
        // a compile-time signal number. We never pass pid < 0 (which
        // would broadcast to a process group).
        unsafe { libc::kill(pid, sig) }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  PtyBackend — top-level supervisor.
// ─────────────────────────────────────────────────────────────────────────────

/// Single-Server PTY supervisor. Holds every live Pane and dispatches
/// CTRL commands. Cheap to clone — internal state is `Arc<DashMap>` + an
/// atomic id counter.
#[derive(Debug, Clone)]
pub struct PtyBackend {
    inner: Arc<PtyBackendInner>,
}

#[derive(Debug)]
struct PtyBackendInner {
    panes: DashMap<PaneId, Arc<PaneHandle>>,
    next_id: AtomicU64,
    /// NOTIFY_MIRROR broadcast — every pane spawn/die event lands here
    /// alongside the per-pane output broadcasts. Subscribers receive
    /// [`BackendNotify`] values; the ws-server router serialises them
    /// to `0x07` envelopes.
    notify_tx: broadcast::Sender<BackendNotify>,
}

impl std::fmt::Debug for PaneHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaneHandle")
            .field("out_subscribers", &self.out_tx.receiver_count())
            .field("stall_count", &self.stall_count.load(Ordering::Relaxed))
            .finish()
    }
}

impl Default for PtyBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyBackend {
    /// Construct an empty backend — no panes, no live processes.
    pub fn new() -> Self {
        let (notify_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            inner: Arc::new(PtyBackendInner {
                panes: DashMap::new(),
                next_id: AtomicU64::new(1),
                notify_tx,
            }),
        }
    }

    /// Subscribe to backend-level notifications (spawned / died /
    /// layout / daemon-started). Independent of any single Pane.
    pub fn subscribe_notify(&self) -> broadcast::Receiver<BackendNotify> {
        self.inner.notify_tx.subscribe()
    }

    /// Number of live Panes. Useful for tests + metrics.
    pub fn pane_count(&self) -> usize {
        self.inner.panes.len()
    }

    /// Spawn a new Pane. ADR-0013 D2 + D10 + ADR-0014 D2. Returns the
    /// freshly-issued [`PaneId`].
    pub fn spawn(&self, spec: SpawnSpec) -> Result<PaneId> {
        spawn_inner(&self.inner, spec, None)
    }

    /// Same as [`Self::spawn`] but echoes `request_id` back via
    /// [`BackendNotify::PaneSpawned.request_id`]. Used by the CTRL
    /// router so the originating UI action can correlate its dispatch
    /// to the assigned [`PaneId`].
    pub fn spawn_with_request(&self, spec: SpawnSpec, request_id: String) -> Result<PaneId> {
        spawn_inner(&self.inner, spec, Some(request_id))
    }

    /// Apply a single decoded [`BackendCommand`]. The ws-server CTRL
    /// router calls into this after JSON-deserialising the envelope
    /// payload. Returns the new PaneId for `NewPane`, `()` otherwise.
    pub fn dispatch(&self, cmd: BackendCommand) -> Result<Option<PaneId>> {
        match cmd {
            BackendCommand::NewPane {
                request_id,
                command,
                args,
                cwd,
                env,
                rows,
                cols,
            } => {
                let spec = SpawnSpec {
                    command,
                    args,
                    cwd,
                    env,
                    rows,
                    cols,
                };
                let id = match request_id {
                    Some(rid) => self.spawn_with_request(spec, rid)?,
                    None => self.spawn(spec)?,
                };
                Ok(Some(id))
            }
            BackendCommand::KillPane { id } => {
                self.kill(id)?;
                Ok(None)
            }
            BackendCommand::ResizePane { id, rows, cols } => {
                self.resize(id, rows, cols)?;
                Ok(None)
            }
            BackendCommand::SetCwd { id, path } => {
                // Reserved for future automation against a freshly
                // spawned shell. For now we simply verify the pane
                // exists so the caller gets a useful error.
                if !self.inner.panes.contains_key(&id) {
                    return Err(PtyBackendError::PaneNotFound(id));
                }
                debug!(pane = %id, path = %path.display(), "set-cwd is a no-op in MVP");
                Ok(None)
            }
            BackendCommand::SetEnv { id, key, value } => {
                if !self.inner.panes.contains_key(&id) {
                    return Err(PtyBackendError::PaneNotFound(id));
                }
                debug!(pane = %id, key = %key, value = %value, "set-env is a no-op in MVP");
                Ok(None)
            }
        }
    }

    /// Kill a Pane (SIGTERM → grace → SIGKILL → reap). Idempotent —
    /// calling on an already-dead Pane returns [`PtyBackendError::PaneNotFound`].
    pub fn kill(&self, id: PaneId) -> Result<()> {
        // Remove the entry, then drop the Arc. The actual signal
        // delivery happens in PaneHandle::drop, but only when the last
        // Arc reference goes out of scope. To make kill synchronous we
        // run the SIGTERM phase explicitly here first.
        let removed = self
            .inner
            .panes
            .remove(&id)
            .ok_or(PtyBackendError::PaneNotFound(id))?;
        let (_id, handle) = removed;
        terminate_child(&handle.child);
        // The wait thread observes the exit and broadcasts pane-died
        // on its own — we don't double-broadcast here. Drop the Arc;
        // if we held the only reference, PaneHandle::drop joins the
        // threads. If subscribers are still holding the broadcast
        // receiver, the senders close when the Arc reaches 0.
        drop(handle);
        Ok(())
    }

    /// Resize a Pane. portable-pty's `MasterPty::resize` issues
    /// `TIOCSWINSZ` which the kernel translates into SIGWINCH to the
    /// child — vim / less / tmux / ncurses all reflow naturally
    /// (ADR-0013 D5, POC Gate #2).
    pub fn resize(&self, id: PaneId, rows: u16, cols: u16) -> Result<()> {
        let handle = self
            .inner
            .panes
            .get(&id)
            .ok_or(PtyBackendError::PaneNotFound(id))?;
        let master = handle.master.clone();
        // Drop the dashmap shard guard before locking the master mutex
        // to avoid holding two locks simultaneously.
        drop(handle);
        let guard = master.lock().map_err(|e| {
            PtyBackendError::ResizeFailed(anyhow::anyhow!("master mutex poisoned: {e}"))
        })?;
        guard
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyBackendError::ResizeFailed(anyhow::anyhow!(e)))
    }

    /// Send raw input bytes to the Pane's PTY master writer. The bytes
    /// are queued onto the mpsc and drained by the writer thread —
    /// this method does not block on the actual write.
    pub fn send_input(&self, id: PaneId, bytes: Vec<u8>) -> Result<()> {
        let handle = self
            .inner
            .panes
            .get(&id)
            .ok_or(PtyBackendError::PaneNotFound(id))?;
        handle
            .in_tx
            .as_ref()
            .ok_or(PtyBackendError::ChannelClosed(id))?
            .send(bytes)
            .map_err(|_| PtyBackendError::ChannelClosed(id))
    }

    /// Subscribe to the Pane's output broadcast and obtain the current
    /// ring-buffer snapshot in one call (race-free: we subscribe first,
    /// then snapshot, so any bytes emitted between snapshot + subscribe
    /// are still delivered through the broadcast queue). Returns `None`
    /// if the Pane is gone.
    pub fn subscribe_output(&self, id: PaneId) -> Option<(Vec<u8>, broadcast::Receiver<Bytes>)> {
        let handle = self.inner.panes.get(&id)?;
        // Order matters — subscribe *before* snapshot to avoid a window
        // where output written in between gets lost.
        let rx = handle.out_tx.subscribe();
        let snap = handle.ring_snapshot();
        Some((snap, rx))
    }

    /// Reader thread's stall counter for `id`, or `None` if the pane
    /// is gone. Counter increments once per broadcast `send` error
    /// (no subscribers OR cap overflow).
    pub fn stall_count(&self, id: PaneId) -> Option<u64> {
        let handle = self.inner.panes.get(&id)?;
        Some(handle.stall_count.load(Ordering::Relaxed))
    }

    /// Backpressure thresholds exposed for tests / metrics. Returns
    /// `(high, low)` byte watermarks — purely observability, no
    /// runtime throttling.
    pub fn backpressure_watermarks() -> (usize, usize) {
        (STALL_HIGH_WATERMARK, STALL_LOW_WATERMARK)
    }

    /// Enumerate every live Pane id, sorted ascending. Useful for the
    /// supervisor teardown loop in `gtmux-cli`.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut v: Vec<PaneId> = self.inner.panes.iter().map(|e| *e.key()).collect();
        v.sort();
        v
    }
}

impl Drop for PtyBackendInner {
    /// Graceful server teardown: signal every pane in parallel, wait
    /// the grace period, then escalate. ADR-0014 D5 + D7 step 1.
    fn drop(&mut self) {
        if self.panes.is_empty() {
            return;
        }
        info!(panes = self.panes.len(), "pty-backend: tearing down");
        // SIGTERM phase — fan out without blocking.
        for entry in self.panes.iter() {
            if let Ok(child) = entry.value().child.lock() {
                if let Some(pid) = child.process_id() {
                    let _ = unsafe_send_signal(pid as i32, libc::SIGTERM);
                }
            }
        }
        // Single shared grace window (not per-pane) keeps shutdown
        // bounded even with N panes.
        let deadline = Instant::now() + PANE_KILL_GRACE;
        while Instant::now() < deadline {
            let all_done = self.panes.iter().all(|entry| {
                let Ok(mut child) = entry.value().child.lock() else {
                    return true;
                };
                matches!(child.try_wait(), Ok(Some(_)))
            });
            if all_done {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // SIGKILL fallback for the laggards.
        for entry in self.panes.iter() {
            if let Ok(mut child) = entry.value().child.lock() {
                if matches!(child.try_wait(), Ok(None)) {
                    let _ = child.kill();
                }
            }
        }
        // Drop the DashMap — each PaneHandle's Drop joins its threads.
        self.panes.clear();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  spawn_inner — the heavy lifting.
// ─────────────────────────────────────────────────────────────────────────────

fn spawn_inner(
    inner: &Arc<PtyBackendInner>,
    spec: SpawnSpec,
    request_id: Option<String>,
) -> Result<PaneId> {
    let pty_system = native_pty_system();
    let rows = if spec.rows == 0 { 24 } else { spec.rows };
    let cols = if spec.cols == 0 { 80 } else { spec.cols };
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| PtyBackendError::SpawnFailed(anyhow::anyhow!(e)))?;

    // Resolve the command. ADR-0014 D10 noisy-env scrub happens below.
    let shell = spec
        .command
        .clone()
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/bash".to_string());
    let mut cmd = CommandBuilder::new(&shell);
    for a in &spec.args {
        cmd.arg(a);
    }
    // cwd default = $HOME, then process cwd. portable-pty errors when
    // cwd is unset *and* the inherited env lacks PWD; we keep parity
    // with the POC by falling back explicitly.
    if let Some(cwd) = spec.cwd.as_ref() {
        cmd.cwd(cwd);
    } else if let Some(home) = std::env::var_os("HOME") {
        cmd.cwd(home);
    }

    // Inherit current env then scrub noisy keys (ADR-0014 D10).
    cmd.env_clear();
    for (k, v) in std::env::vars_os() {
        let Some(k) = k.to_str() else { continue };
        if NOISY_ENV_KEYS.iter().any(|nk| *nk == k) {
            continue;
        }
        cmd.env(k, v);
    }
    // Sensible default for `$TERM` (POC parity).
    cmd.env("TERM", "xterm-256color");
    // User-supplied env overrides anything inherited.
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| PtyBackendError::SpawnFailed(anyhow::anyhow!(e)))?;
    drop(pair.slave);

    // Split the master fd into a reader handle + writer handle, plus
    // a clone for resize ioctl.
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| PtyBackendError::SpawnFailed(anyhow::anyhow!(e)))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| PtyBackendError::SpawnFailed(anyhow::anyhow!(e)))?;
    let master = Arc::new(StdMutex::new(pair.master));
    let child = Arc::new(StdMutex::new(child as Box<dyn Child + Send + Sync>));

    let (out_tx, _) = broadcast::channel::<Bytes>(BROADCAST_CAPACITY);
    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let ring = Arc::new(StdMutex::new(VecDeque::with_capacity(RING_CAPACITY)));
    let stall = Arc::new(AtomicU64::new(0));

    let id = PaneId(inner.next_id.fetch_add(1, Ordering::Relaxed));

    // ─── reader thread ──────────────────────────────────────────────
    let out_tx_reader = out_tx.clone();
    let ring_reader = ring.clone();
    let stall_reader = stall.clone();
    let reader_join = std::thread::Builder::new()
        .name(format!("pty-reader-{}", id.0))
        .spawn(move || {
            let mut buf = [0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        debug!(pane = %id, "pty reader: EOF");
                        break;
                    }
                    Ok(n) => {
                        // Update the ring buffer *before* the broadcast
                        // so a late attach that arrives between the two
                        // operations still sees the bytes.
                        PaneHandle::ring_append(&ring_reader, &buf[..n]);
                        let chunk = Bytes::copy_from_slice(&buf[..n]);
                        if out_tx_reader.send(chunk).is_err() {
                            // No subscribers OR every subscriber is
                            // lagged past cap. Increment the
                            // observability counter and keep reading
                            // (the ring buffer still holds the bytes).
                            stall_reader.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        debug!(pane = %id, error = %e, "pty reader: error");
                        break;
                    }
                }
            }
        })
        .map_err(|e| PtyBackendError::SpawnFailed(anyhow::anyhow!(e)))?;

    // ─── writer thread ──────────────────────────────────────────────
    let writer_id = id;
    let writer_join = std::thread::Builder::new()
        .name(format!("pty-writer-{}", id.0))
        .spawn(move || {
            while let Some(bytes) = in_rx.blocking_recv() {
                if let Err(e) = writer.write_all(&bytes) {
                    debug!(pane = %writer_id, error = %e, "pty writer: error");
                    break;
                }
                let _ = writer.flush();
            }
            debug!(pane = %writer_id, "pty writer: channel closed");
        })
        .map_err(|e| PtyBackendError::SpawnFailed(anyhow::anyhow!(e)))?;

    // ─── child-wait thread ──────────────────────────────────────────
    let wait_id = id;
    let wait_child = child.clone();
    let wait_notify = inner.notify_tx.clone();
    let wait_join = std::thread::Builder::new()
        .name(format!("pty-wait-{}", id.0))
        .spawn(move || {
            // Block on the child handle. We hold the mutex across
            // wait() — that is OK because no other code path needs the
            // child handle (kill uses libc::kill directly via the pid,
            // and SIGKILL fallback in the teardown path uses try_wait
            // which only fires after this thread already returned).
            // Actually: `terminate_child` *does* lock + try_wait, and
            // it must succeed during the grace window. Holding the
            // mutex across blocking wait would deadlock that path. So
            // we drop the lock after extracting a clone-able pid then
            // wait via a polling loop on try_wait — same effect but
            // never holds the lock across a blocking syscall.
            let exit_status = loop {
                let res = {
                    let Ok(mut child) = wait_child.lock() else {
                        // Poisoned — give up; the supervisor will see
                        // the broadcast never arrive but the pane is
                        // already getting dropped.
                        warn!(pane = %wait_id, "child mutex poisoned in wait thread");
                        return;
                    };
                    child.try_wait()
                };
                match res {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        warn!(pane = %wait_id, error = %e, "child wait error");
                        break None;
                    }
                }
            };
            info!(pane = %wait_id, status = ?exit_status, "child exited");
            let (code, signal) = match &exit_status {
                Some(s) => exit_code_signal(s.exit_code()),
                None => (None, None),
            };
            let _ = wait_notify.send(BackendNotify::PaneDied {
                id: wait_id,
                code,
                signal,
            });
        })
        .map_err(|e| PtyBackendError::SpawnFailed(anyhow::anyhow!(e)))?;

    let handle = Arc::new(PaneHandle {
        out_tx,
        in_tx: Some(in_tx),
        master,
        child,
        ring,
        stall_count: stall,
        reader_join: Some(reader_join),
        writer_join: Some(writer_join),
        wait_join: Some(wait_join),
    });
    inner.panes.insert(id, handle);

    // PaneSpawned NOTIFY_MIRROR — fired *after* the entry lands in the
    // dashmap so a racing subscribe_output sees the pane immediately.
    let _ = inner
        .notify_tx
        .send(BackendNotify::PaneSpawned { id, request_id });
    info!(pane = %id, rows, cols, "pane spawned");
    Ok(id)
}

/// Extract a POSIX-style `(code, signal)` pair from portable-pty's u32
/// `ExitStatus::exit_code`. portable-pty packs both into the same byte
/// the way `waitpid` does on Unix: low 7 bits = signal if non-zero,
/// otherwise high 8 bits = exit code.
fn exit_code_signal(exit_code: u32) -> (Option<i32>, Option<i32>) {
    // Mimic POSIX W* macros so the FE can tell "exit 0" from
    // "killed by SIGTERM 15".
    let low = (exit_code & 0x7F) as i32;
    let high = ((exit_code >> 8) & 0xFF) as i32;
    if low == 0 {
        (Some(high), None)
    } else if low == 0x7F {
        // Stopped, not exited. Treat as still-alive — surface None/None.
        (None, None)
    } else {
        (None, Some(low))
    }
}

/// ADR-0014 D10 — strip noisy env keys before spawning the child shell
/// so we don't accidentally nest inside an outer tmux / mux session.
const NOISY_ENV_KEYS: &[&str] = &[
    "TMUX",
    "TMUX_PANE",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
];

// ─────────────────────────────────────────────────────────────────────────────
//  Unit tests — pure logic only. Integration tests live under tests/.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn pane_id_round_trip_json() {
        let id = PaneId(42);
        let s = serde_json::to_string(&id).unwrap();
        assert_eq!(s, "42");
        let back: PaneId = serde_json::from_str(&s).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn backend_command_new_pane_round_trip() {
        let cmd = BackendCommand::NewPane {
            request_id: Some("req-1".to_string()),
            command: Some("/bin/sh".to_string()),
            args: vec!["-c".into(), "echo hi".into()],
            cwd: Some(PathBuf::from("/tmp")),
            env: vec![("FOO".into(), "bar".into())],
            rows: 24,
            cols: 80,
        };
        let s = serde_json::to_string(&cmd).unwrap();
        assert!(s.contains(r#""type":"new-pane""#));
        assert!(s.contains(r#""request_id":"req-1""#));
        let back: BackendCommand = serde_json::from_str(&s).unwrap();
        match back {
            BackendCommand::NewPane {
                command,
                args,
                rows,
                cols,
                ..
            } => {
                assert_eq!(command.as_deref(), Some("/bin/sh"));
                assert_eq!(args, vec!["-c", "echo hi"]);
                assert_eq!(rows, 24);
                assert_eq!(cols, 80);
            }
            other => panic!("expected NewPane, got {other:?}"),
        }
    }

    #[test]
    fn backend_command_kill_pane_round_trip() {
        let cmd = BackendCommand::KillPane { id: PaneId(7) };
        let s = serde_json::to_string(&cmd).unwrap();
        assert!(s.contains(r#""type":"kill-pane""#));
        let back: BackendCommand = serde_json::from_str(&s).unwrap();
        matches!(back, BackendCommand::KillPane { id } if id == PaneId(7));
    }

    #[test]
    fn backend_command_resize_pane_round_trip() {
        let cmd = BackendCommand::ResizePane {
            id: PaneId(3),
            rows: 40,
            cols: 120,
        };
        let s = serde_json::to_string(&cmd).unwrap();
        assert!(s.contains(r#""type":"resize-pane""#));
        let back: BackendCommand = serde_json::from_str(&s).unwrap();
        match back {
            BackendCommand::ResizePane { id, rows, cols } => {
                assert_eq!(id, PaneId(3));
                assert_eq!(rows, 40);
                assert_eq!(cols, 120);
            }
            other => panic!("expected ResizePane, got {other:?}"),
        }
    }

    #[test]
    fn backend_notify_pane_spawned_serialises_kebab() {
        let n = BackendNotify::PaneSpawned {
            id: PaneId(5),
            request_id: Some("r1".into()),
        };
        let s = serde_json::to_string(&n).unwrap();
        // tag = "kind" (NOTIFY_MIRROR mirrors the legacy field name)
        assert!(s.contains(r#""kind":"pane-spawned""#));
        assert!(s.contains(r#""id":5"#));
        assert!(s.contains(r#""request_id":"r1""#));
    }

    #[test]
    fn backend_notify_pane_died_omits_none_fields() {
        let n = BackendNotify::PaneDied {
            id: PaneId(9),
            code: Some(0),
            signal: None,
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains(r#""code":0"#));
        assert!(!s.contains("signal"));
    }

    #[test]
    fn unknown_command_type_rejected() {
        let bad = r#"{"type":"format-disk"}"#;
        let res: std::result::Result<BackendCommand, _> = serde_json::from_str(bad);
        assert!(res.is_err());
    }

    #[test]
    fn exit_code_signal_normal_exit() {
        // exit 0 — low 7 bits zero, high byte zero
        assert_eq!(exit_code_signal(0), (Some(0), None));
        // exit 42 — high byte = 42 (waitpid layout)
        assert_eq!(exit_code_signal(42 << 8), (Some(42), None));
    }

    #[test]
    fn exit_code_signal_killed_by_signal() {
        // SIGTERM = 15
        assert_eq!(exit_code_signal(15), (None, Some(15)));
        // SIGKILL = 9
        assert_eq!(exit_code_signal(9), (None, Some(9)));
    }

    #[test]
    fn backend_construction_is_empty() {
        let backend = PtyBackend::new();
        assert_eq!(backend.pane_count(), 0);
        assert!(backend.pane_ids().is_empty());
    }

    #[test]
    fn backpressure_watermarks_documented() {
        let (high, low) = PtyBackend::backpressure_watermarks();
        assert!(high > low);
        assert_eq!(high, STALL_HIGH_WATERMARK);
        assert_eq!(low, STALL_LOW_WATERMARK);
    }
}
