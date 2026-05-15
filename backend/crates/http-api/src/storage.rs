//! Canvas Layout 영속화 (S7-PERSISTENCE-MINIMAL, ADR-0006).
//!
//! Storage backend = plain JSON + atomic-write-file. 단일 파일
//! `${XDG_STATE_HOME}/gtmux/<session>.layout.json` 에 LayoutSnapshot 의
//! body 가 그대로 직렬화된다 — `etag` 는 부팅 시 디스크 페이로드 hash 로
//! 재계산되므로 디스크에 별도 슬롯이 없다 (ADR-0006 D10 표 두 번째 행).
//!
//! 본 모듈은 두 표면만 노출한다:
//! - [`LayoutStore::load`] — 부팅 시 호출. ADR-0006 D10 7-state 표를 그대로
//!   구현해 손상 파일은 sidecar quarantine (`<file>.corrupt-<unix_ts>`) 으로
//!   격리하고 빈 layout 으로 cold start. 절대 panic 하지 않는다.
//! - [`LayoutStore::save`] — `PUT /api/layout` 의 (c) 단계에서 호출. 검증
//!   통과 + 새 ETag 계산 *후* in-memory snapshot 교체 *전* 에 실행되어
//!   disk-first invariant 를 보장한다 (ADR-0006 D13).
//!
//! Cross-reference:
//! - `docs/adr/0006-persistence-storage.md` D1-D13
//! - `docs/ssot/canvas-layout-schema.md` §1 (페이로드 schema)
//! - `docs/reports/0027-session-resume-handoff.md` §4.1 (S7-PERSISTENCE-MINIMAL spec)

use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use atomic_write_file::unix::OpenOptionsExt as AwfOpenOptionsExt;
use atomic_write_file::OpenOptions as AwfOpenOptions;
use serde_json::Value;
use thiserror::Error;
use tracing::warn;

use crate::LayoutSnapshot;

/// Required mode bits on the persisted layout file (ADR-0006 D11 / ADR-0003
/// SSoT §1.10 — same 0600 contract as the token file).
const LAYOUT_FILE_MODE: u32 = 0o600;

/// Errors produced by the on-disk layout store. `save()` returns these so
/// callers can decide between 500 (disk write failed mid-PUT) and other
/// dispositions. `load()` never returns — it absorbs all failure modes into
/// the ADR-0006 D10 quarantine path and returns a valid empty snapshot.
#[derive(Debug, Error)]
pub enum StorageError {
    /// I/O failure while writing or reading the layout file.
    #[error("layout store io: {0}")]
    Io(#[from] io::Error),
}

/// File-backed layout store wrapping a single absolute path. Cheap to clone
/// (the wrapped `PathBuf` is the only owned data).
#[derive(Debug, Clone)]
pub struct LayoutStore {
    path: PathBuf,
}

impl LayoutStore {
    /// Construct a store rooted at `path`. The directory does not need to
    /// exist yet — `save()` creates the parent (mode 0700) if missing.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Path to the backing file. Test-only public surface; production code
    /// goes through [`load`](Self::load) / [`save`](Self::save).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Boot-time loader. Implements ADR-0006 D10 7-state table exactly:
    ///
    /// | State | Action | Notify |
    /// |---|---|---|
    /// | Absent | empty layout | log `layout: cold start (no file)` |
    /// | Valid JSON + valid schema | load, recompute ETag from body | (silent) |
    /// | 0 bytes | quarantine + empty | WARN |
    /// | JSON parse fail | quarantine + empty | WARN |
    /// | Missing or unknown `schema_version` | quarantine + empty | WARN |
    /// | Minimal schema rule violation | quarantine + empty | WARN |
    /// | Mode != 0600 | chmod 0600 + WARN, then proceed | WARN |
    ///
    /// This function *never* returns an error — every failure path falls
    /// back to a fresh empty snapshot so the Server can always boot.
    pub fn load(&self) -> LayoutSnapshot {
        // Read raw bytes first. Anything other than NotFound is treated as
        // corruption (e.g. transient IO errors during boot are surfaced as
        // a quarantine rather than blocking the whole Server).
        let raw = match std::fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                tracing::info!(
                    path = %self.path.display(),
                    "layout: cold start (no file)"
                );
                return LayoutSnapshot::empty();
            }
            Err(e) => {
                warn!(
                    path = %self.path.display(),
                    error = %e,
                    "layout: read failed; treating as corrupt and starting empty"
                );
                self.quarantine_corrupt("io-error");
                return LayoutSnapshot::empty();
            }
        };

        // Perm audit — fix up before parsing so a recovered boot still
        // leaves the file at 0600 on disk.
        self.audit_perm();

        if raw.is_empty() {
            warn!(
                path = %self.path.display(),
                "layout: file is 0 bytes (D10 row 3) — quarantining and starting empty"
            );
            self.quarantine_corrupt("zero-bytes");
            return LayoutSnapshot::empty();
        }

        let mut body: Value = match serde_json::from_slice(&raw) {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    path = %self.path.display(),
                    error = %e,
                    "layout: JSON parse failed (D10 row 4); quarantining"
                );
                self.quarantine_corrupt("parse-fail");
                return LayoutSnapshot::empty();
            }
        };

        // schema_version: required, must equal 1. Missing or any other
        // integer triggers quarantine (D10 row 5). We are stricter here
        // than the `minimal_layout_check` used on the PUT path because
        // boot-time validation is the only sentinel against an
        // out-of-band file edit producing an ambiguous state.
        match body.get("schema_version").and_then(Value::as_u64) {
            Some(1) => {}
            Some(other) => {
                warn!(
                    path = %self.path.display(),
                    schema_version = other,
                    "layout: unsupported schema_version (D10 row 5); quarantining"
                );
                self.quarantine_corrupt("unsupported-schema-version");
                return LayoutSnapshot::empty();
            }
            None => {
                warn!(
                    path = %self.path.display(),
                    "layout: schema_version missing (D10 row 5); quarantining"
                );
                self.quarantine_corrupt("missing-schema-version");
                return LayoutSnapshot::empty();
            }
        }

        if let Err(msg) = crate::minimal_layout_check(&body) {
            warn!(
                path = %self.path.display(),
                reason = %msg,
                "layout: schema rule violation (D10 row 6); quarantining"
            );
            self.quarantine_corrupt("schema-rule-violation");
            return LayoutSnapshot::empty();
        }

        // ADR-0006 D14 (2026-05-15 amend) — *strip panels[] on boot*.
        //
        // PTY-direct era (ADR-0013): every Pane is a child process and
        // dies with the Server (ADR-0014 D5). On Server restart the new
        // PtyBackend allocates fresh PaneIds starting at 1 — the
        // persisted `Panel.pane_id` strings (`%2`, `%3`, ...) reference
        // *stale* panes that no longer exist. Rather than ship orphan
        // Panels with disconnected xterms (jarring UX), we drop the
        // panels[] array on load. groups[] + schema_version + future
        // canvas viewport state survive.
        //
        // Trade-off: sketch §6.7 promised Panel-coord persistence; that
        // promise belongs to the tmux era. The PTY-direct era trades
        // Panel persistence for process-lifecycle clarity. User-facing
        // recovery is the explicit "New Panel" action.
        let panels_before = body
            .get("panels")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0);
        if panels_before > 0 {
            if let Some(panels) = body.get_mut("panels") {
                *panels = Value::Array(Vec::new());
            }
            tracing::info!(
                path = %self.path.display(),
                stripped = panels_before,
                "layout: stripped {panels_before} stale Panel(s) on boot (ADR-0006 D14, PTY-direct era)"
            );
        }

        // ETag is recomputed from the canonical serialisation of the loaded
        // body — D10 row 2: "ETag 는 디스크 페이로드 hash 재계산으로 메모리
        // 셋업". This naturally absorbs any user edits made while the
        // Server was stopped (ADR-0006 R12).
        LayoutSnapshot::from_body(body)
    }

    /// Atomically replace the layout file with `body_bytes` (the canonical
    /// JSON serialisation of a validated [`LayoutSnapshot::body`]).
    ///
    /// Order of operations (ADR-0006 D3):
    /// 1. Ensure parent directory exists, mode 0700.
    /// 2. Open `<dir>/.<file>.<rand>` via `atomic-write-file` with mode 0600
    ///    + `preserve_mode(false)` so umask cannot relax it.
    /// 3. Write all bytes, `commit()` (fsync + rename + dir fsync).
    /// 4. `commit()` consumes the handle; no temp file remains on the
    ///    filesystem regardless of success or panic.
    pub fn save(&self, body_bytes: &[u8]) -> Result<(), StorageError> {
        let dir = self.path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "layout path has no parent")
        })?;
        ensure_dir_0700(dir)?;

        // mode(0o600) + preserve_mode(false) forces 0600 even when an
        // earlier file at the same path had broader perms. The std OpenOptionsExt
        // `mode` setter is on the `atomic_write_file::OpenOptions` thanks to
        // the upstream trait impl in `unix.rs`.
        let mut f = AwfOpenOptions::new()
            .mode(LAYOUT_FILE_MODE)
            .preserve_mode(false)
            .open(&self.path)?;
        f.write_all(body_bytes)?;
        f.commit()?;
        Ok(())
    }

    /// Move the current file (if any) aside to `<path>.corrupt-<unix_ts>`
    /// per ADR-0006 D10. Failures are logged but never propagated — the
    /// caller is mid-boot and a fresh empty layout always wins.
    fn quarantine_corrupt(&self, reason: &'static str) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let sidecar = sidecar_path(&self.path, ts);
        match std::fs::rename(&self.path, &sidecar) {
            Ok(()) => {
                tracing::error!(
                    original = %self.path.display(),
                    quarantine = %sidecar.display(),
                    reason = reason,
                    "layout: corrupt file quarantined; starting with empty layout"
                );
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Race or missing file — nothing to do.
            }
            Err(e) => {
                warn!(
                    original = %self.path.display(),
                    quarantine = %sidecar.display(),
                    error = %e,
                    reason = reason,
                    "layout: quarantine rename failed; sidecar may be incomplete"
                );
            }
        }
    }

    /// chmod 0600 if the mode bits diverge. Failure surfaces as WARN only
    /// — we do not refuse to boot over a permission audit.
    fn audit_perm(&self) {
        let meta = match std::fs::metadata(&self.path) {
            Ok(m) => m,
            Err(_) => return,
        };
        let mode = meta.permissions().mode() & 0o777;
        if mode == LAYOUT_FILE_MODE {
            return;
        }
        warn!(
            path = %self.path.display(),
            actual = format!("{:o}", mode),
            expected = format!("{:o}", LAYOUT_FILE_MODE),
            "layout: file mode != 0600 (D10 row 7); applying chmod 0600"
        );
        let perm = std::fs::Permissions::from_mode(LAYOUT_FILE_MODE);
        if let Err(e) = std::fs::set_permissions(&self.path, perm) {
            warn!(
                path = %self.path.display(),
                error = %e,
                "layout: chmod 0600 failed; continuing with current mode"
            );
        }
    }
}

/// Build `<dir>/<filename>.corrupt-<unix_ts>`. Inlining a helper keeps the
/// load() call site free of path-arithmetic noise.
fn sidecar_path(path: &Path, unix_ts: u64) -> PathBuf {
    let mut filename = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    filename.push(format!(".corrupt-{}", unix_ts));
    match path.parent() {
        Some(dir) => dir.join(filename),
        None => PathBuf::from(filename),
    }
}

/// Ensure the parent directory exists and is mode 0700 (ADR-0006 D11).
/// Re-applied on every save so a manual chmod cannot weaken the dir.
fn ensure_dir_0700(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    let perm = std::fs::Permissions::from_mode(0o700);
    std::fs::set_permissions(dir, perm)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn store_in(dir: &TempDir) -> LayoutStore {
        LayoutStore::new(dir.path().join("smoke.layout.json"))
    }

    #[test]
    fn load_absent_returns_empty() {
        let dir = TempDir::new().unwrap();
        let snap = store_in(&dir).load();
        let empty = LayoutSnapshot::empty();
        assert_eq!(snap.etag, empty.etag);
        assert_eq!(snap.body, empty.body);
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        let body = json!({
            "schema_version": 1,
            "groups": [{
                "id": "ga1",
                "parent_id": null,
                "label": "main",
                "color": "#abcdef",
                "visibility": true,
                "locked": false,
                "order": 0,
            }],
            "panels": [],
        });
        let original = LayoutSnapshot::from_body(body);
        let bytes = serde_json::to_vec(&original.body).unwrap();
        store.save(&bytes).expect("save ok");
        let loaded = store.load();
        assert_eq!(loaded.etag, original.etag);
        assert_eq!(loaded.body, original.body);
        // Persisted file is 0600.
        let mode = std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn load_zero_bytes_quarantines() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        std::fs::write(store.path(), b"").unwrap();
        let snap = store.load();
        assert_eq!(snap.body, LayoutSnapshot::empty().body);
        assert!(!store.path().exists(), "original must be moved aside");
        let sidecar = find_sidecar(dir.path());
        assert!(sidecar.is_some(), "corrupt sidecar must exist");
    }

    #[test]
    fn load_parse_fail_quarantines() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        std::fs::write(store.path(), b"{ this is not json").unwrap();
        let snap = store.load();
        assert_eq!(snap.body, LayoutSnapshot::empty().body);
        assert!(!store.path().exists());
        assert!(find_sidecar(dir.path()).is_some());
    }

    #[test]
    fn load_unsupported_schema_version_quarantines() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        let bad = json!({ "schema_version": 99, "groups": [], "panels": [] });
        std::fs::write(store.path(), serde_json::to_vec(&bad).unwrap()).unwrap();
        let snap = store.load();
        assert_eq!(snap.body, LayoutSnapshot::empty().body);
        assert!(!store.path().exists());
        assert!(find_sidecar(dir.path()).is_some());
    }

    #[test]
    fn load_missing_schema_version_quarantines() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        let bad = json!({ "groups": [], "panels": [] });
        std::fs::write(store.path(), serde_json::to_vec(&bad).unwrap()).unwrap();
        let snap = store.load();
        assert_eq!(snap.body, LayoutSnapshot::empty().body);
        assert!(!store.path().exists());
        assert!(find_sidecar(dir.path()).is_some());
    }

    #[test]
    fn load_schema_rule_violation_quarantines() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        // schema_version valid but `groups` is the wrong type — caught by
        // minimal_layout_check.
        let bad = json!({
            "schema_version": 1,
            "groups": "not an array",
            "panels": [],
        });
        std::fs::write(store.path(), serde_json::to_vec(&bad).unwrap()).unwrap();
        let snap = store.load();
        assert_eq!(snap.body, LayoutSnapshot::empty().body);
        assert!(!store.path().exists());
        assert!(find_sidecar(dir.path()).is_some());
    }

    #[test]
    fn load_bad_perm_chmod_then_loads() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        // Write a valid layout but with mode 0644 on the file.
        let body = json!({ "schema_version": 1, "groups": [], "panels": [] });
        std::fs::write(store.path(), serde_json::to_vec(&body).unwrap()).unwrap();
        std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        let snap = store.load();
        assert_eq!(snap.body, LayoutSnapshot::empty().body);
        // Mode must have been corrected to 0600.
        let mode = std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        // No quarantine sidecar — the file content was valid.
        assert!(store.path().exists());
        assert!(find_sidecar(dir.path()).is_none());
    }

    #[test]
    fn save_creates_parent_dir_0700() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("nested").join("smoke.layout.json");
        let store = LayoutStore::new(nested.clone());
        let body = json!({ "schema_version": 1, "groups": [], "panels": [] });
        let bytes = serde_json::to_vec(&body).unwrap();
        store.save(&bytes).expect("save ok");
        let parent_mode = std::fs::metadata(nested.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(parent_mode, 0o700);
    }

    #[test]
    fn save_is_atomic_no_tmp_left_behind() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        let body = json!({ "schema_version": 1, "groups": [], "panels": [] });
        let bytes = serde_json::to_vec(&body).unwrap();
        store.save(&bytes).expect("save ok");
        // The committed file should be the only entry in the directory —
        // atomic-write-file's tmp sidecar must have been renamed away.
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        assert_eq!(entries.len(), 1, "leftover entries: {entries:?}");
        assert_eq!(entries[0].to_string_lossy(), "smoke.layout.json");
    }

    fn find_sidecar(dir: &Path) -> Option<PathBuf> {
        std::fs::read_dir(dir).ok()?.find_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains(".corrupt-") {
                Some(entry.path())
            } else {
                None
            }
        })
    }
}
