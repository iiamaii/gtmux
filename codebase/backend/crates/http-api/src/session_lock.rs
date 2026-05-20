//! Cross-server session lock — `<workspace>/.locks/<name>.lock` (ADR-0019 D6).
//!
//! Two layers operate together:
//!   * **OS-level `flock(2)`** (LOCK_EX|LOCK_NB on acquire; LOCK_SH|LOCK_NB on
//!     peek). The kernel auto-releases on process death so a SIGKILL-ed server
//!     can never leave a stale exclusive lock behind.
//!   * **Lease JSON in the file body** for diagnostic display only — modal
//!     rows show the holder server_id / pid / expected expiry. The body is
//!     truthful at the moment of write but is *not* the source of truth for
//!     whether the lock is held; flock state is.
//!
//! Lifetime:
//!   * `acquire` blocks the calling task on a `spawn_blocking` to do the
//!     synchronous flock without stalling the runtime, then writes the lease
//!     JSON under the held lock.
//!   * `peek` is non-blocking; it tries LOCK_SH|LOCK_NB. EWOULDBLOCK ⇒
//!     in-use; success ⇒ stale (caller may unlink + re-acquire).
//!   * Dropping a [`LockGuard`] releases the flock and unlinks the file.
//!
//! Single-server invariant (D6.6): callers must serialise concurrent attach
//! requests on the same session name *before* hitting this module. The
//! attach handler in `sessions.rs` does this with a per-name `tokio::Mutex`.

#![allow(missing_docs)]

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};

/// Default lease window — must match the WS heartbeat (ADR-0021 D6) so a
/// missed PONG leaves a *short* uncertainty zone in the diagnostic body
/// before the OS flock release reflects reality.
pub const DEFAULT_LEASE_SECS: u64 = 30;

const LOCK_FILE_MODE: u32 = 0o600;

#[derive(Debug, Error)]
pub enum LockError {
    #[error("session lock io: {0}")]
    Io(#[from] std::io::Error),
    #[error("session is in use by another webpage")]
    Contended,
    #[error("session lock json: {0}")]
    Serde(String),
}

/// Lease body — written to the lock file under the exclusive flock so other
/// servers can `peek` it for the modal UI hint. Truthful only at the moment
/// of write — kernel flock state is the authoritative answer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lease {
    /// UUID minted once per Server boot. Lets a peeker disambiguate two
    /// gtmux processes that happen to share the same PID after a wrap.
    pub server_id: String,
    /// OS PID of the holding gtmux process. Displayed verbatim
    /// ("in use by server-pid 12345").
    pub pid: u32,
    /// Cookie / connection identifier of the WebSocket connection holding
    /// the attach. The same `gtmux_auth` cookie used by HTTP — kept opaque
    /// at the lock layer.
    pub ws_conn_id: String,
    /// Expected lease expiry as Unix seconds. Refreshed on every heartbeat
    /// ping; the *actual* release is bounded by the OS flock.
    pub lease_until_unix: u64,
}

/// Outcome of a `peek` against a `.lock` file.
#[derive(Debug, Clone, PartialEq)]
pub enum LockState {
    /// No `.lock` file present.
    Vacant,
    /// `.lock` file exists but no flock is held (stale file from a crashed
    /// server). The peeker may unlink it and re-acquire.
    Stale,
    /// Exclusive flock is held; the body is the holder's diagnostic.
    InUse(Lease),
    /// Exclusive flock is held but the body could not be parsed yet (the
    /// holder is between LOCK_EX and the JSON write). Modal renders this as
    /// "acquiring…" and re-polls.
    InUseRaceyBody,
}

/// RAII guard returned by [`acquire`]. Releasing it unlocks the flock and
/// unlinks the file — by `Drop` if you forget to call [`release`] yourself.
pub struct LockGuard {
    path: PathBuf,
    server_id: Arc<str>,
    file: Option<File>,
    released: bool,
}

impl std::fmt::Debug for LockGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockGuard")
            .field("path", &self.path)
            .field("server_id", &self.server_id)
            .field("released", &self.released)
            .finish()
    }
}

impl LockGuard {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Re-write the lease body with a fresh `lease_until` (called on each
    /// heartbeat ping per ADR-0019 D6.2). Cheap — keeps the file open and
    /// truncates in place.
    pub fn refresh_lease(&mut self, ws_conn_id: &str) -> Result<(), LockError> {
        let lease = Lease {
            server_id: self.server_id.to_string(),
            pid: std::process::id(),
            ws_conn_id: ws_conn_id.to_string(),
            lease_until_unix: now_unix() + DEFAULT_LEASE_SECS,
        };
        let Some(file) = self.file.as_mut() else {
            return Err(LockError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock guard already released",
            )));
        };
        write_lease_body(file, &lease)
    }

    /// Manually release. Idempotent — called automatically from `Drop` if
    /// the caller doesn't reach this. Failures during release are logged
    /// (warn-level) but never returned, mirroring the bootstrap-time
    /// quarantine policy: a missing-on-shutdown lock file is preferable to
    /// a panic.
    pub fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        if let Some(file) = self.file.take() {
            // unlock_safely() is infallible on Drop — see `fs2` semantics:
            // FileExt::unlock returns io::Result but a panic-during-drop is
            // worse than a leaked descriptor for a process that's exiting.
            if let Err(e) = FileExt::unlock(&file) {
                warn!(error = %e, path = %self.path.display(), "session_lock: flock release failed");
            }
            drop(file);
        }
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                warn!(error = %e, path = %self.path.display(), "session_lock: unlink failed");
            }
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        self.release();
    }
}

/// Try to acquire the cross-server lock for `<locks_dir>/<name>.lock`.
///
/// Returns `Err(LockError::Contended)` if another server / webpage already
/// holds the EX flock. The caller (the attach handler) maps this to a 409.
pub fn acquire(
    locks_dir: &Path,
    name: &str,
    server_id: Arc<str>,
    ws_conn_id: &str,
) -> Result<LockGuard, LockError> {
    if !locks_dir.exists() {
        std::fs::create_dir_all(locks_dir)?;
    }
    let path = locks_dir.join(format!("{name}.lock"));
    let mut opts = OpenOptions::new();
    opts.read(true)
        .write(true)
        .create(true)
        .mode(LOCK_FILE_MODE);
    let mut file = opts.open(&path)?;

    match FileExt::try_lock_exclusive(&file) {
        Ok(_) => {}
        Err(e) => {
            // fs2 returns std::io::Error with kind WouldBlock for contention.
            if e.kind() == std::io::ErrorKind::WouldBlock {
                return Err(LockError::Contended);
            }
            return Err(LockError::Io(e));
        }
    }

    // Reset truncation so a stale body left by a previously SIGKILL-ed
    // server doesn't bleed into our diagnostic.
    file.set_len(0)?;
    let lease = Lease {
        server_id: server_id.to_string(),
        pid: std::process::id(),
        ws_conn_id: ws_conn_id.to_string(),
        lease_until_unix: now_unix() + DEFAULT_LEASE_SECS,
    };
    if let Err(e) = write_lease_body(&mut file, &lease) {
        // Body write failed — release the flock and propagate so the caller
        // doesn't end up with a held lock and a confused modal hint.
        let _ = FileExt::unlock(&file);
        return Err(e);
    }
    debug!(
        path = %path.display(),
        pid = lease.pid,
        ws_conn_id = %ws_conn_id,
        "session_lock: acquired"
    );
    Ok(LockGuard {
        path,
        server_id,
        file: Some(file),
        released: false,
    })
}

/// Non-destructive inspection of a lock file. Used by `enumerate_sessions`
/// to mark `active=true` on rows currently held.
pub fn peek(locks_dir: &Path, name: &str) -> LockState {
    let path = locks_dir.join(format!("{name}.lock"));
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return LockState::Vacant,
        Err(e) => {
            warn!(error = %e, path = %path.display(), "session_lock: peek open failed");
            return LockState::Vacant;
        }
    };
    match FileExt::try_lock_shared(&file) {
        Ok(_) => {
            // SH succeeded → no exclusive holder. Treat as stale (the holder
            // crashed). Caller may unlink + re-acquire.
            let _ = FileExt::unlock(&file);
            LockState::Stale
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            // Someone holds EX. Try to parse the body for the diagnostic.
            match read_lease_body(&path) {
                Ok(lease) => LockState::InUse(lease),
                Err(_) => LockState::InUseRaceyBody,
            }
        }
        Err(e) => {
            warn!(error = %e, path = %path.display(), "session_lock: peek flock failed");
            LockState::Vacant
        }
    }
}

fn write_lease_body(file: &mut File, lease: &Lease) -> Result<(), LockError> {
    let bytes = serde_json::to_vec(lease).map_err(|e| LockError::Serde(e.to_string()))?;
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(())
}

fn read_lease_body(path: &Path) -> Result<Lease, LockError> {
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        // Holder is between LOCK_EX and the JSON write.
        return Err(LockError::Serde("empty body".into()));
    }
    serde_json::from_slice(&bytes).map_err(|e| LockError::Serde(e.to_string()))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Generate a fresh server_id (UUID-shaped). Used by the boot wiring so
/// every `gtmux start` mints exactly one and shares it across all lock
/// acquisitions for the lifetime of the process.
pub fn fresh_server_id() -> String {
    // 16 random bytes → UUID v4 canonical shape via simple format. We avoid
    // pulling in the `uuid` crate just for this.
    use ring::rand::{SecureRandom, SystemRandom};
    let mut b = [0u8; 16];
    SystemRandom::new()
        .fill(&mut b)
        .expect("ring SystemRandom is infallible for small fills");
    // Set version (4) and variant (RFC 4122) bits.
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15],
    )
}

/// Unlink a stale lock file. Used by callers that detected
/// [`LockState::Stale`] before retrying [`acquire`]. Safe to call on a
/// missing path — returns Ok in that case.
pub fn unlink_stale(locks_dir: &Path, name: &str) -> std::io::Result<()> {
    let path = locks_dir.join(format!("{name}.lock"));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Boot-time housekeeping: walk every session record in `wm` and peek its
/// `.lock` file. When a peek returns [`LockState::Stale`] — the body
/// survived but the kernel-level flock is gone (typical SIGKILL trail) —
/// remove the file so the directory does not grow unboundedly across many
/// abrupt restarts.
///
/// **Non-functional**: peek already recognises `Stale` and the next
/// [`acquire`] would overwrite the body anyway (`set_len(0)` in §acquire);
/// this helper exists only for `.locks/` hygiene. It is therefore
/// `warn`-on-error rather than fail-fast — a single unreadable entry
/// must never block boot (0072 handover Q6).
///
/// **`InUse` is never touched** — that would silently break a live
/// Webpage. Only entries the kernel has already released get pruned.
///
/// Returns the number of stale files unlinked (boot log uses this to
/// emit a single info-level line; zero is silent).
pub fn scan_and_cleanup_stale_locks(wm: &crate::workspace::WorkspaceManager) -> u32 {
    let infos = match wm.enumerate_sessions() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "session_lock: stale scan enumerate failed; skipping cleanup"
            );
            return 0;
        }
    };
    let locks_dir = wm.locks_dir();
    let mut cleaned = 0u32;
    for info in &infos {
        if !matches!(peek(&locks_dir, &info.name), LockState::Stale) {
            continue;
        }
        match unlink_stale(&locks_dir, &info.name) {
            Ok(()) => {
                cleaned += 1;
            }
            Err(e) => {
                tracing::warn!(
                    session = %info.name,
                    error = %e,
                    "session_lock: stale unlink failed; continuing"
                );
            }
        }
    }
    if cleaned > 0 {
        tracing::info!(count = cleaned, "session_lock: boot-time stale cleanup");
    }
    cleaned
}

// Suppress dead_code on the unused-after-Drop sentinel field on platforms
// that elide the trait dispatch.
const _: fn(&LockGuard) = |_| {};

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn dir() -> TempDir {
        let d = TempDir::new().unwrap();
        std::fs::create_dir_all(d.path().join(".locks")).unwrap();
        d
    }

    fn locks(d: &TempDir) -> PathBuf {
        d.path().join(".locks")
    }

    #[test]
    fn acquire_then_release_creates_and_removes() {
        let d = dir();
        let server_id: Arc<str> = fresh_server_id().into();
        let mut guard = acquire(&locks(&d), "alpha", server_id, "conn-A").unwrap();
        assert!(guard.path().exists());
        // Body parses back as a valid lease.
        let lease = read_lease_body(guard.path()).unwrap();
        assert_eq!(lease.pid, std::process::id());
        assert_eq!(lease.ws_conn_id, "conn-A");
        assert!(lease.lease_until_unix > now_unix());
        guard.release();
        assert!(!locks(&d).join("alpha.lock").exists());
    }

    #[test]
    fn concurrent_acquire_returns_contended() {
        let d = dir();
        let server_id: Arc<str> = fresh_server_id().into();
        let _g = acquire(&locks(&d), "beta", server_id.clone(), "conn-A").unwrap();
        let err = acquire(&locks(&d), "beta", server_id, "conn-B").unwrap_err();
        assert!(matches!(err, LockError::Contended));
    }

    #[test]
    fn peek_vacant_for_missing_file() {
        let d = dir();
        assert_eq!(peek(&locks(&d), "ghost"), LockState::Vacant);
    }

    #[test]
    fn peek_inuse_while_held_then_stale_after_drop_no_release() {
        let d = dir();
        let server_id: Arc<str> = fresh_server_id().into();
        let guard = acquire(&locks(&d), "gamma", server_id, "conn-X").unwrap();
        match peek(&locks(&d), "gamma") {
            LockState::InUse(lease) => {
                assert_eq!(lease.ws_conn_id, "conn-X");
                assert_eq!(lease.pid, std::process::id());
            }
            other => panic!("expected InUse, got {other:?}"),
        }
        drop(guard); // releases flock + unlinks.
        assert_eq!(peek(&locks(&d), "gamma"), LockState::Vacant);
    }

    #[test]
    fn peek_stale_when_file_exists_but_no_holder() {
        let d = dir();
        // Simulate a SIGKILL-survivor file: write a body but don't lock it.
        std::fs::write(
            locks(&d).join("delta.lock"),
            br#"{"server_id":"x","pid":1,"ws_conn_id":"y","lease_until_unix":0}"#,
        )
        .unwrap();
        assert_eq!(peek(&locks(&d), "delta"), LockState::Stale);
        // Caller may unlink and retry.
        unlink_stale(&locks(&d), "delta").unwrap();
        assert_eq!(peek(&locks(&d), "delta"), LockState::Vacant);
    }

    #[test]
    fn refresh_lease_extends_expiry_in_body() {
        let d = dir();
        let server_id: Arc<str> = fresh_server_id().into();
        let mut guard = acquire(&locks(&d), "eps", server_id, "conn-1").unwrap();
        let before = read_lease_body(guard.path()).unwrap().lease_until_unix;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        guard.refresh_lease("conn-2").unwrap();
        let after = read_lease_body(guard.path()).unwrap();
        assert!(after.lease_until_unix > before, "lease must extend");
        assert_eq!(after.ws_conn_id, "conn-2");
    }

    #[test]
    fn server_id_is_uuid_shaped() {
        let id = fresh_server_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.as_bytes()[8], b'-');
        assert_eq!(id.as_bytes()[13], b'-');
        assert_eq!(id.as_bytes()[14], b'4', "uuid version 4 nibble");
        assert_eq!(id.as_bytes()[18], b'-');
        assert_eq!(id.as_bytes()[23], b'-');
    }

    // Quiet unused import on builds where Duration is constants-only.
    const _: Duration = Duration::from_secs(0);

    /// Build a `WorkspaceManager` whose workspace dir contains the named
    /// session files plus the matching stale `.lock` body for each.
    /// Used by the `scan_and_cleanup_stale_locks` tests to set up a
    /// SIGKILL-aftermath scenario without an actual server crash.
    fn workspace_with_stale_locks(names: &[&str]) -> (TempDir, crate::workspace::WorkspaceManager) {
        let dir = TempDir::new().unwrap();
        let wm = crate::workspace::WorkspaceManager::from_path(dir.path().to_path_buf()).unwrap();
        std::fs::create_dir_all(wm.locks_dir()).unwrap();
        for name in names {
            // empty v2 layout JSON — enumerate_sessions just needs the file.
            std::fs::write(
                dir.path().join(format!("{name}.json")),
                br#"{"schema_version":2,"groups":[],"items":[],"viewport":{"x":0,"y":0,"zoom":1}}"#,
            )
            .unwrap();
            // stale lock body — same shape acquire writes, but no live flock.
            std::fs::write(
                wm.locks_dir().join(format!("{name}.lock")),
                format!(
                    r#"{{"server_id":"x","pid":1,"ws_conn_id":"y","lease_until_unix":0,"_for":"{name}"}}"#
                ),
            )
            .unwrap();
        }
        (dir, wm)
    }

    /// 0071 §D-1 / 0072 BE-C happy path: a workspace inherited from a
    /// SIGKILL'd run has stale `.lock` bodies for every session. The
    /// boot sweep must unlink each one.
    #[test]
    fn stale_lock_scan_unlinks_stale_files() {
        let (_dir, wm) = workspace_with_stale_locks(&["alpha", "beta"]);
        assert!(wm.locks_dir().join("alpha.lock").exists());
        assert!(wm.locks_dir().join("beta.lock").exists());
        let cleaned = scan_and_cleanup_stale_locks(&wm);
        assert_eq!(cleaned, 2, "both stale files must be unlinked");
        assert!(!wm.locks_dir().join("alpha.lock").exists());
        assert!(!wm.locks_dir().join("beta.lock").exists());
    }

    /// A lock that is **actively held** by another acquirer (e.g. a
    /// surviving Webpage on a previous server reboot path) must survive
    /// the sweep — `LockState::InUse` is never unlinked. The held flock
    /// also remains valid after the sweep returns.
    #[test]
    fn stale_lock_scan_preserves_held_locks() {
        let dir = TempDir::new().unwrap();
        let wm = crate::workspace::WorkspaceManager::from_path(dir.path().to_path_buf()).unwrap();
        std::fs::create_dir_all(wm.locks_dir()).unwrap();
        // Session record for "alpha" plus a real, held flock.
        std::fs::write(
            dir.path().join("alpha.json"),
            br#"{"schema_version":2,"groups":[],"items":[],"viewport":{"x":0,"y":0,"zoom":1}}"#,
        )
        .unwrap();
        let server_id: Arc<str> = fresh_server_id().into();
        let mut guard = acquire(&wm.locks_dir(), "alpha", server_id, "conn-live").unwrap();
        assert!(matches!(
            peek(&wm.locks_dir(), "alpha"),
            LockState::InUse(_)
        ));
        let cleaned = scan_and_cleanup_stale_locks(&wm);
        assert_eq!(cleaned, 0, "InUse must never be unlinked");
        assert!(
            wm.locks_dir().join("alpha.lock").exists(),
            "held lock file must survive the sweep"
        );
        // Guard is still valid — release explicitly to verify.
        guard.release();
        assert!(!wm.locks_dir().join("alpha.lock").exists());
    }
}
