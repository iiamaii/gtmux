//! WorkspaceManager — owner of the per-Server storage directory.
//!
//! Source-of-truth: `docs/adr/0019-session-and-workspace-model.md` (D1, D2, D11)
//! and the boot-time migration spec in
//! `docs/adr/0018-canvas-item-data-model.md` D5 / ADR-0006 D15.
//!
//! Path resolution precedence (ADR-0019 D2):
//!   1. CLI `--workspace <path>` (boot-time, immutable)
//!   2. config `workspace_path = "/abs/path"`
//!   3. default: `${XDG_DATA_HOME:-~/.local/share}/gtmux/workspace/`
//!
//! Responsibilities:
//!   * Ensure the workspace dir + `.locks/` subdir exist with mode 0700.
//!   * Map a session name to its on-disk file path.
//!   * Validate session names against the ADR-0019 D7 regex `^[A-Za-z0-9_-]{1,64}$`.
//!   * Enumerate session records on disk (no body read — handlers fetch on
//!     demand).
//!   * Boot-time v1→v2 hard cutover for every record present in the workspace
//!     dir (ADR-0018 D5).
//!
//! What this module does *not* own:
//!   * Session file body I/O — that lives in `sessions.rs`.
//!   * Cross-server `.lock` file semantics — that lives in `session_lock.rs`
//!     (Stage 3 work, not yet present). This module only *creates* the
//!     `.locks/` dir so the future module can drop files into it without
//!     racing on directory creation.

// Public data fields on `SessionInfo` / `BootMigrationReport` / `WorkspaceError`
// variants are intentionally self-describing — see ADR-0019 D2/D6 for the
// authoritative semantics. We suppress the per-field lint to keep this file
// readable instead of restating those decisions on every accessor.
#![allow(missing_docs)]

use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use atomic_write_file::unix::OpenOptionsExt as AwfOpenOptionsExt;
use atomic_write_file::OpenOptions as AwfOpenOptions;
use thiserror::Error;
use tracing::{info, warn};

use crate::schema::{detect_shape, migrate_v1_to_v2, SchemaShape};

const WORKSPACE_DIR_MODE: u32 = 0o700;
const SESSION_FILE_MODE: u32 = 0o600;
const LOCKS_SUBDIR: &str = ".locks";

/// Errors produced by [`WorkspaceManager`]. Variants map 1:1 to handler
/// responses: invalid name → 400, IO → 500, not found → 404.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("invalid session name: {0:?} (allowed: ^[A-Za-z0-9_-]{{1,64}}$)")]
    InvalidSessionName(String),
    #[error("workspace path is not absolute: {0}")]
    NonAbsolutePath(PathBuf),
    #[error("xdg resolution failed: {0}")]
    BadXdg(String),
    #[error("workspace io: {0}")]
    Io(#[from] std::io::Error),
}

/// Per-Server workspace handle. Cheap to clone (the wrapped `PathBuf` is the
/// only owned data — share via `Arc` from `AppState`).
#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    path: PathBuf,
}

/// Lightweight enumeration result returned by [`WorkspaceManager::enumerate_sessions`].
/// The `name` field is the file stem (no `.json` extension); the active flag
/// is left to Stage 3 (cross-server lock peek) — for now it is always `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub name: String,
    /// `true` once Stage 3's `session_lock` module reports an EX-held flock.
    pub active: bool,
}

impl WorkspaceManager {
    /// Resolve the workspace path per ADR-0019 D2 precedence and ensure the
    /// directory tree exists with the correct mode bits.
    ///
    /// `cli_override` corresponds to `gtmux start --workspace <path>` and
    /// wins over everything else. `config_value` is the TOML `workspace_path`
    /// field. When both are `None` the default `${XDG_DATA_HOME}/gtmux/workspace/`
    /// applies (with the standard XDG fallback to `~/.local/share`).
    pub fn resolve(
        cli_override: Option<PathBuf>,
        config_value: Option<PathBuf>,
    ) -> Result<Self, WorkspaceError> {
        let path = match (cli_override, config_value) {
            (Some(p), _) => p,
            (None, Some(p)) => p,
            (None, None) => default_workspace_path()?,
        };
        if !path.is_absolute() {
            return Err(WorkspaceError::NonAbsolutePath(path));
        }
        let me = Self { path };
        me.ensure_dirs()?;
        Ok(me)
    }

    /// Construct a manager rooted at `path` *without* running XDG resolution.
    /// Test-only — production callers go through [`resolve`](Self::resolve).
    #[doc(hidden)]
    pub fn from_path(path: PathBuf) -> Result<Self, WorkspaceError> {
        if !path.is_absolute() {
            return Err(WorkspaceError::NonAbsolutePath(path));
        }
        let me = Self { path };
        me.ensure_dirs()?;
        Ok(me)
    }

    /// Absolute workspace directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Subdirectory used by the future cross-server `.lock` module
    /// (`<workspace>/.locks/`). Created at boot so Stage 3 callers don't
    /// have to worry about a missing parent.
    pub fn locks_dir(&self) -> PathBuf {
        self.path.join(LOCKS_SUBDIR)
    }

    /// Map a validated session name to its on-disk file path.
    pub fn session_path(&self, name: &str) -> Result<PathBuf, WorkspaceError> {
        validate_session_name(name)?;
        Ok(self.path.join(format!("{name}.json")))
    }

    /// Enumerate session records on disk. The `.locks/` subdirectory and
    /// `.corrupt-*` sidecars are skipped.
    pub fn enumerate_sessions(&self) -> Result<Vec<SessionInfo>, WorkspaceError> {
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(&self.path) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, "workspace: skipping unreadable dir entry");
                    continue;
                }
            };
            let path = entry.path();
            let Some(name) = session_name_from_path(&path) else {
                continue;
            };
            out.push(SessionInfo {
                name,
                active: false,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Walk every session file in the workspace and, if it is shape-v1,
    /// rewrite it as v2 atomically. Quarantine via `.corrupt-<ts>` sidecar on
    /// JSON parse failure — same policy as ADR-0006 D10 row 4.
    ///
    /// Returns the number of records migrated (for the boot log) and the
    /// number quarantined.
    pub fn boot_migration_v1_to_v2(&self) -> Result<BootMigrationReport, WorkspaceError> {
        let mut report = BootMigrationReport::default();
        let entries = match std::fs::read_dir(&self.path) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(report),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if session_name_from_path(&path).is_none() {
                continue;
            }
            match self.migrate_one(&path) {
                Ok(MigrateOutcome::AlreadyV2) => {}
                Ok(MigrateOutcome::Migrated) => report.migrated += 1,
                Ok(MigrateOutcome::Quarantined { reason }) => {
                    report.quarantined += 1;
                    warn!(
                        path = %path.display(),
                        reason,
                        "workspace: quarantined unsupported session record"
                    );
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "workspace: migration failed; skipping");
                }
            }
        }
        if report.migrated > 0 || report.quarantined > 0 {
            info!(
                migrated = report.migrated,
                quarantined = report.quarantined,
                "workspace: boot migration complete"
            );
        }
        Ok(report)
    }

    fn ensure_dirs(&self) -> Result<(), WorkspaceError> {
        ensure_dir_0700(&self.path)?;
        ensure_dir_0700(&self.locks_dir())?;
        Ok(())
    }

    fn migrate_one(&self, path: &Path) -> Result<MigrateOutcome, WorkspaceError> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(MigrateOutcome::AlreadyV2);
            }
            Err(e) => return Err(e.into()),
        };
        if bytes.is_empty() {
            quarantine(path, "zero-bytes");
            return Ok(MigrateOutcome::Quarantined {
                reason: "zero-bytes",
            });
        }
        let mut body: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                quarantine(path, "parse-fail");
                return Ok(MigrateOutcome::Quarantined {
                    reason: "parse-fail",
                });
            }
        };
        match detect_shape(&body) {
            SchemaShape::V2 => Ok(MigrateOutcome::AlreadyV2),
            SchemaShape::V1 => {
                migrate_v1_to_v2(&mut body);
                let bytes = serde_json::to_vec(&body).map_err(|e| {
                    WorkspaceError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                })?;
                atomic_write_session(path, &bytes)?;
                info!(
                    path = %path.display(),
                    "workspace: migrated session record v1→v2"
                );
                Ok(MigrateOutcome::Migrated)
            }
            SchemaShape::Unknown => {
                quarantine(path, "unknown-schema-version");
                Ok(MigrateOutcome::Quarantined {
                    reason: "unknown-schema-version",
                })
            }
        }
    }
}

/// Boot migration summary.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BootMigrationReport {
    pub migrated: usize,
    pub quarantined: usize,
}

enum MigrateOutcome {
    AlreadyV2,
    Migrated,
    Quarantined { reason: &'static str },
}

/// Write `bytes` to `path` atomically, mode 0600. Re-exported for `sessions.rs`
/// so both modules go through the same atomic-write primitive.
pub(crate) fn atomic_write_session(path: &Path, bytes: &[u8]) -> Result<(), WorkspaceError> {
    let dir = path.parent().ok_or_else(|| {
        WorkspaceError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "session path has no parent",
        ))
    })?;
    ensure_dir_0700(dir)?;
    let mut f = AwfOpenOptions::new()
        .mode(SESSION_FILE_MODE)
        .preserve_mode(false)
        .open(path)
        .map_err(|e| WorkspaceError::Io(e.into()))?;
    f.write_all(bytes).map_err(WorkspaceError::Io)?;
    f.commit().map_err(|e| WorkspaceError::Io(e.into()))?;
    Ok(())
}

/// Sidecar-quarantine the file at `path` (`.corrupt-<unix_ts>`). Best-effort —
/// caller is mid-boot and a missing record is preferable to a failed boot.
fn quarantine(path: &Path, reason: &'static str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut filename = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    filename.push(format!(".corrupt-{ts}"));
    let sidecar = match path.parent() {
        Some(d) => d.join(filename),
        None => PathBuf::from(filename),
    };
    match std::fs::rename(path, &sidecar) {
        Ok(()) => warn!(
            original = %path.display(),
            quarantine = %sidecar.display(),
            reason,
            "workspace: corrupt session record quarantined"
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!(
            original = %path.display(),
            error = %e,
            "workspace: quarantine rename failed"
        ),
    }
}

/// Validate a session name against ADR-0019 D7. ASCII letters/digits/`-`/`_`,
/// 1–64 bytes. Rejects path-traversal vectors (`/`, `.`, `\0`) by construction.
pub fn validate_session_name(name: &str) -> Result<(), WorkspaceError> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return Err(WorkspaceError::InvalidSessionName(name.to_string()));
    }
    let ok = bytes
        .iter()
        .all(|b| matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'));
    if !ok {
        return Err(WorkspaceError::InvalidSessionName(name.to_string()));
    }
    Ok(())
}

/// Resolve the default workspace path: `${XDG_DATA_HOME}/gtmux/workspace/`
/// (with the standard XDG fallback to `$HOME/.local/share`).
fn default_workspace_path() -> Result<PathBuf, WorkspaceError> {
    let base = if let Some(s) = std::env::var_os("XDG_DATA_HOME") {
        let p = PathBuf::from(s);
        if p.as_os_str().is_empty() {
            return Err(WorkspaceError::BadXdg(
                "XDG_DATA_HOME is set but empty".into(),
            ));
        }
        p
    } else {
        let home = std::env::var_os("HOME").ok_or_else(|| {
            WorkspaceError::BadXdg("$HOME not set; cannot resolve XDG_DATA_HOME default".into())
        })?;
        PathBuf::from(home).join(".local").join("share")
    };
    Ok(base.join("gtmux").join("workspace"))
}

/// Recursively create `dir` (if missing) and chmod 0700.
fn ensure_dir_0700(dir: &Path) -> std::io::Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    let perm = std::fs::Permissions::from_mode(WORKSPACE_DIR_MODE);
    std::fs::set_permissions(dir, perm)
}

/// Recover the session name from a path within the workspace dir. Returns
/// `None` for anything that is not a regular `<valid-name>.json` file — that
/// covers `.locks/`, `.corrupt-*` sidecars, dotfiles, and accidental
/// non-session files the user may have dropped into the workspace dir.
fn session_name_from_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?;
    let s = file_name.to_str()?;
    if s.starts_with('.') {
        return None;
    }
    let stem = path.file_stem().and_then(OsStr::to_str)?;
    if path.extension().and_then(OsStr::to_str) != Some("json") {
        return None;
    }
    if validate_session_name(stem).is_err() {
        return None;
    }
    if !path.is_file() {
        return None;
    }
    Some(stem.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn fresh_manager() -> (TempDir, WorkspaceManager) {
        let dir = TempDir::new().unwrap();
        let wm = WorkspaceManager::from_path(dir.path().to_path_buf()).unwrap();
        (dir, wm)
    }

    #[test]
    fn ensure_dirs_creates_locks_subdir_0700() {
        let (_dir, wm) = fresh_manager();
        assert!(wm.locks_dir().exists());
        let mode = std::fs::metadata(wm.locks_dir()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let wmode = std::fs::metadata(wm.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(wmode, 0o700);
    }

    #[test]
    fn session_name_validation() {
        for ok in ["a", "demo-build", "build_monitor", "x".repeat(64).as_str()] {
            assert!(validate_session_name(ok).is_ok(), "expected ok: {ok:?}");
        }
        for bad in [
            "", "../etc", "a/b", "a.b", "a b", "ab\0", &"x".repeat(65),
        ] {
            assert!(
                validate_session_name(bad).is_err(),
                "expected reject: {bad:?}"
            );
        }
    }

    #[test]
    fn session_path_rejects_traversal() {
        let (_dir, wm) = fresh_manager();
        assert!(wm.session_path("../etc/passwd").is_err());
        assert!(wm.session_path("a/b").is_err());
    }

    #[test]
    fn enumerate_lists_only_valid_session_files() {
        let (dir, wm) = fresh_manager();
        // Real session record
        std::fs::write(dir.path().join("demo.json"), b"{}").unwrap();
        // Quarantined sidecar — must be skipped
        std::fs::write(dir.path().join("demo.json.corrupt-100"), b"{}").unwrap();
        // Hidden dotfile — must be skipped
        std::fs::write(dir.path().join(".hidden.json"), b"{}").unwrap();
        // Random non-JSON — must be skipped
        std::fs::write(dir.path().join("readme.txt"), b"hi").unwrap();
        // Invalid stem — must be skipped
        std::fs::write(dir.path().join("bad name.json"), b"{}").unwrap();

        let mut found = wm.enumerate_sessions().unwrap();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "demo");
        assert!(!found[0].active);
    }

    #[test]
    fn boot_migration_v1_to_v2_rewrites_in_place() {
        let (dir, wm) = fresh_manager();
        let v1 = json!({
            "schema_version": 1,
            "groups": [],
            "panels": [{ "id": "%2", "x": 1, "y": 2 }],
        });
        std::fs::write(dir.path().join("alpha.json"), serde_json::to_vec(&v1).unwrap()).unwrap();

        let report = wm.boot_migration_v1_to_v2().unwrap();
        assert_eq!(report.migrated, 1);
        assert_eq!(report.quarantined, 0);

        let after: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("alpha.json")).unwrap()).unwrap();
        assert_eq!(after["schema_version"], 2);
        assert!(after.get("panels").is_none());
        assert_eq!(after["items"].as_array().unwrap().len(), 0);
        // Persisted file is 0600 after rewrite.
        let mode = std::fs::metadata(dir.path().join("alpha.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn boot_migration_quarantines_unknown_schema_version() {
        let (dir, wm) = fresh_manager();
        let bad = json!({ "schema_version": 99, "groups": [], "panels": [] });
        std::fs::write(dir.path().join("rotten.json"), serde_json::to_vec(&bad).unwrap()).unwrap();
        let report = wm.boot_migration_v1_to_v2().unwrap();
        assert_eq!(report.migrated, 0);
        assert_eq!(report.quarantined, 1);
        // Original moved aside.
        assert!(!dir.path().join("rotten.json").exists());
        let any_sidecar = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(any_sidecar);
    }

    #[test]
    fn boot_migration_leaves_v2_files_untouched() {
        let (dir, wm) = fresh_manager();
        let v2 = json!({
            "schema_version": 2,
            "groups": [],
            "items": [],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let bytes = serde_json::to_vec(&v2).unwrap();
        std::fs::write(dir.path().join("clean.json"), &bytes).unwrap();
        // Stamp a known mtime by reading it before.
        let mtime_before = std::fs::metadata(dir.path().join("clean.json"))
            .unwrap()
            .modified()
            .ok();
        let report = wm.boot_migration_v1_to_v2().unwrap();
        assert_eq!(report.migrated, 0);
        assert_eq!(report.quarantined, 0);
        let mtime_after = std::fs::metadata(dir.path().join("clean.json"))
            .unwrap()
            .modified()
            .ok();
        // The file should not have been rewritten — mtime preserved if reads
        // don't touch it. (We do not assert equality across systems where
        // mtime is unsupported.)
        if let (Some(a), Some(b)) = (mtime_before, mtime_after) {
            assert_eq!(a, b, "v2 file must not be rewritten");
        }
    }

    #[test]
    fn boot_migration_handles_empty_workspace() {
        let (_dir, wm) = fresh_manager();
        let report = wm.boot_migration_v1_to_v2().unwrap();
        assert_eq!(report, BootMigrationReport::default());
    }

    #[test]
    fn resolve_rejects_relative_path() {
        let err = WorkspaceManager::resolve(Some(PathBuf::from("rel/path")), None).unwrap_err();
        assert!(matches!(err, WorkspaceError::NonAbsolutePath(_)));
    }
}
