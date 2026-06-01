//! WorkspaceManager — owner of the per-Server storage directory.
//!
//! Source-of-truth: `docs/adr/0019-session-and-workspace-model.md` (D1, D2, D11)
//! and the boot-time migration spec in
//! `docs/adr/0018-canvas-item-data-model.md` D5 / ADR-0006 D15.
//!
//! This manager owns the **Store(C)** directory (ADR-0045 D5 — gtmux's internal
//! control-plane storage: session records + manifest + locks + assets). The
//! `WorkspaceManager` name predates the ADR-0045 vocabulary split and is kept
//! for now (symbol rename → `StoreManager` is a separate, gradual follow-up,
//! ADR-0045 remap table / plan-0020 Stage A-1).
//!
//! Store path resolution precedence (ADR-0045 D5 — supersedes ADR-0044 D-A3):
//!   1. config `workspace_path = "/abs/path"` (→ future `store_path`)
//!   2. default: instance-isolated `${XDG_DATA_HOME}/gtmux/store/<instance>/`
//!      (back-compat: reuse a legacy `…/gtmux/workspaces/<instance>/` (ADR-0044)
//!      or shared `…/gtmux/workspace/` (pre-0044) when it holds data and the new
//!      `store/<instance>` dir does not yet exist — see `resolve`; no data move).
//!
//! NOTE: CLI `--workspace` no longer feeds the Store — under ADR-0045 it
//! designates the **Server Workspace(A)** (the fs sandbox root, resolved in
//! `fs_guard::resolve_server_workspace`). The Store is now instance-derived
//! with no dedicated flag (a `--store-path` override is a possible follow-up).
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

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use atomic_write_file::unix::OpenOptionsExt as AwfOpenOptionsExt;
use atomic_write_file::OpenOptions as AwfOpenOptions;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

use crate::schema::{detect_shape, migrate_v1_to_v2, SchemaShape};

const WORKSPACE_DIR_MODE: u32 = 0o700;
const SESSION_FILE_MODE: u32 = 0o600;
const LOCKS_SUBDIR: &str = ".locks";
const ASSETS_SUBDIR: &str = ".assets";
const WORKSPACE_MANIFEST_FILE: &str = ".gtmux-workspace.json";
const WORKSPACE_MANIFEST_VERSION: u32 = 1;

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
    #[error("invalid workspace manifest: {0}")]
    InvalidManifest(String),
    #[error("workspace io: {0}")]
    Io(#[from] std::io::Error),
}

/// Per-Server workspace handle. Cheap to clone (the wrapped `PathBuf` is the
/// only owned data — share via `Arc` from `AppState`).
#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    path: PathBuf,
    /// `true` when boot fell back to a legacy Store location — either the
    /// ADR-0044 `…/gtmux/workspaces/<instance>/` or the pre-0044 shared
    /// `…/gtmux/workspace/` — instead of the ADR-0045 `…/gtmux/store/<instance>/`
    /// default (back-compat, no data move). Surfaced to the CLI banner so it
    /// can print the one-line deprecation hint.
    legacy_shared: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceManifest {
    pub manifest_version: u32,
    #[serde(default)]
    pub folders: Vec<WorkspaceFolder>,
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionOrg>,
}

impl Default for WorkspaceManifest {
    fn default() -> Self {
        Self {
            manifest_version: WORKSPACE_MANIFEST_VERSION,
            folders: Vec::new(),
            sessions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceFolder {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub order: i64,
    /// Optional display tint (ADR-0044 D-B2 schema). A free-form display
    /// string — `None` when the FE has not assigned a colour. Not validated
    /// (it is not a path or identifier); it simply round-trips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionOrg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub favorite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCountsCacheEntry {
    pub item_count: u32,
    pub terminal_count: u32,
    pub modified_at: SystemTime,
    /// Raw `Layout.workspace_root`(B) captured during the same mtime-keyed
    /// parse the counts use (ADR-0046 D1). `None` for legacy/edge records;
    /// the list handler resolves the effective workspace from it.
    pub workspace_root: Option<String>,
}

impl WorkspaceManager {
    /// Resolve the Store(C) path per ADR-0045 D5 precedence and ensure the
    /// directory tree exists with the correct mode bits.
    ///
    /// `cli_override` is retained for the offline `gtmux session ...` tooling
    /// (which still accepts `--workspace <store-dir>` to read records directly);
    /// the `gtmux start` path now passes `None` here because `--workspace` was
    /// re-purposed to the Server Workspace(A). `config_value` is the TOML
    /// `workspace_path` field (Store override → future `store_path`).
    /// `instance` is the Server Instance name (CLI `--name`).
    ///
    /// Precedence:
    ///   1. `cli_override` → that path.
    ///   2. `config_value` → that path.
    ///   3. both `None` → **instance-isolated** default
    ///      `${XDG_DATA_HOME}/gtmux/store/<instance>/`, *except* the back-compat
    ///      branch (ADR-0045 D5, mirrors ADR-0044 D-A3): when the new
    ///      `store/<instance>` dir does not yet exist but a legacy location
    ///      holds at least one valid `<name>.json` — the ADR-0044
    ///      `…/gtmux/workspaces/<instance>/` first, then the pre-0044 shared
    ///      `…/gtmux/workspace/` — that legacy dir is reused (no data move) and
    ///      [`is_legacy_shared`](Self::is_legacy_shared) returns `true`.
    pub fn resolve(
        cli_override: Option<PathBuf>,
        config_value: Option<PathBuf>,
        instance: &str,
    ) -> Result<Self, WorkspaceError> {
        let (path, legacy_shared) = match (cli_override, config_value) {
            (Some(p), _) => (p, false),
            (None, Some(p)) => (p, false),
            (None, None) => {
                let store_dir = instance_store_path(instance)?;
                let legacy_0044 = instance_workspaces_path(instance)?;
                let legacy_shared = legacy_shared_workspace_path()?;
                // Prefer the ADR-0045 store/<instance> dir. Only when it does
                // not yet exist do we reuse a populated legacy dir (0044
                // workspaces/<instance> first, then pre-0044 shared workspace/)
                // — new installs and already-migrated instances get the store
                // default. No automatic data move (data safety).
                if store_dir.exists() {
                    (store_dir, false)
                } else if dir_has_session_record(&legacy_0044) {
                    (legacy_0044, true)
                } else if dir_has_session_record(&legacy_shared) {
                    (legacy_shared, true)
                } else {
                    (store_dir, false)
                }
            }
        };
        if !path.is_absolute() {
            return Err(WorkspaceError::NonAbsolutePath(path));
        }
        let me = Self {
            path,
            legacy_shared,
        };
        me.ensure_dirs()?;
        if me.legacy_shared {
            warn!(
                store = %me.path.display(),
                "using legacy Store location; set config `workspace_path` to silence \
                 (ADR-0045 D5 back-compat — new instances get …/gtmux/store/<instance>/)"
            );
        }
        Ok(me)
    }

    /// `true` when [`resolve`](Self::resolve) fell back to a legacy Store
    /// location (ADR-0044 `workspaces/<instance>` or pre-0044 shared
    /// `workspace/`) instead of the ADR-0045 `store/<instance>` default. The
    /// CLI banner uses this to print a deprecation hint; always `false` for an
    /// explicit config path and for the `store/<instance>` default.
    pub fn is_legacy_shared(&self) -> bool {
        self.legacy_shared
    }

    /// Construct a manager rooted at `path` *without* running XDG resolution.
    /// Test-only — production callers go through [`resolve`](Self::resolve).
    #[doc(hidden)]
    pub fn from_path(path: PathBuf) -> Result<Self, WorkspaceError> {
        if !path.is_absolute() {
            return Err(WorkspaceError::NonAbsolutePath(path));
        }
        let me = Self {
            path,
            legacy_shared: false,
        };
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

    /// Subdirectory used by the `/api/assets/*` content-addressed store
    /// (`<workspace>/.assets/`). Files are named by their sha256 hex digest;
    /// the directory is mode 0700 and the on-disk layout is owned by
    /// [`crate::assets`] (ADR-0033 D1).
    pub fn assets_dir(&self) -> PathBuf {
        self.path.join(ASSETS_SUBDIR)
    }

    /// Ensure the assets subdir exists with mode 0700. The `assets.rs` upload
    /// path calls this once per request; the cost is a `metadata()` syscall
    /// when the dir is already present.
    pub fn ensure_assets_dir(&self) -> std::io::Result<PathBuf> {
        let dir = self.assets_dir();
        ensure_dir_0700(&dir)?;
        Ok(dir)
    }

    /// Map a validated session name to its on-disk file path.
    pub fn session_path(&self, name: &str) -> Result<PathBuf, WorkspaceError> {
        validate_session_name(name)?;
        Ok(self.path.join(format!("{name}.json")))
    }

    /// Path of the workspace organization manifest.
    pub fn manifest_path(&self) -> PathBuf {
        self.path.join(WORKSPACE_MANIFEST_FILE)
    }

    /// Read the workspace manifest. Missing file means an empty default;
    /// malformed or invalid files are quarantined and also fall back to the
    /// default so a bad org file never prevents Server boot.
    pub fn read_manifest(&self) -> Result<WorkspaceManifest, WorkspaceError> {
        let path = self.manifest_path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(WorkspaceManifest::default());
            }
            Err(e) => return Err(e.into()),
        };
        let manifest: WorkspaceManifest = match serde_json::from_slice(&bytes) {
            Ok(m) => m,
            Err(_) => {
                quarantine(&path, "manifest-parse-fail");
                return Ok(WorkspaceManifest::default());
            }
        };
        let infos = self.enumerate_sessions()?;
        if let Err(e) = self.validate_manifest(&manifest, &infos) {
            warn!(
                path = %path.display(),
                error = %e,
                "workspace: invalid manifest quarantined"
            );
            quarantine(&path, "manifest-invalid");
            return Ok(WorkspaceManifest::default());
        }
        Ok(manifest)
    }

    /// Validate a manifest against the current set of session files.
    pub fn validate_manifest(
        &self,
        manifest: &WorkspaceManifest,
        existing_sessions: &[SessionInfo],
    ) -> Result<(), WorkspaceError> {
        if manifest.manifest_version != WORKSPACE_MANIFEST_VERSION {
            return Err(WorkspaceError::InvalidManifest(format!(
                "manifest_version must be {WORKSPACE_MANIFEST_VERSION}"
            )));
        }

        let existing: BTreeSet<&str> = existing_sessions.iter().map(|s| s.name.as_str()).collect();
        let mut folder_ids = BTreeSet::new();
        for folder in &manifest.folders {
            validate_uuid_shape(&folder.id, "folder.id")?;
            validate_display_name(&folder.name, "folder.name", 64)?;
            if !folder_ids.insert(folder.id.to_ascii_lowercase()) {
                return Err(WorkspaceError::InvalidManifest(format!(
                    "duplicate folder id: {}",
                    folder.id
                )));
            }
        }
        for folder in &manifest.folders {
            if let Some(parent_id) = folder.parent_id.as_deref() {
                let parent_key = parent_id.to_ascii_lowercase();
                if parent_key == folder.id.to_ascii_lowercase() {
                    return Err(WorkspaceError::InvalidManifest(format!(
                        "folder {} cannot be its own parent",
                        folder.id
                    )));
                }
                if !folder_ids.contains(&parent_key) {
                    return Err(WorkspaceError::InvalidManifest(format!(
                        "folder {} has unknown parent_id {}",
                        folder.id, parent_id
                    )));
                }
            }
        }
        validate_folder_acyclic(&manifest.folders)?;

        for (name, org) in &manifest.sessions {
            validate_session_name(name)?;
            if !existing.contains(name.as_str()) {
                return Err(WorkspaceError::InvalidManifest(format!(
                    "session entry has no matching file: {name}"
                )));
            }
            if let Some(folder_id) = org.folder_id.as_deref() {
                if !folder_ids.contains(&folder_id.to_ascii_lowercase()) {
                    return Err(WorkspaceError::InvalidManifest(format!(
                        "session {name} has unknown folder_id {folder_id}"
                    )));
                }
            }
            if org.tags.len() > 16 {
                return Err(WorkspaceError::InvalidManifest(format!(
                    "session {name} has too many tags"
                )));
            }
            let mut tags = BTreeSet::new();
            for tag in &org.tags {
                validate_tag(tag)?;
                if !tags.insert(tag.as_str()) {
                    return Err(WorkspaceError::InvalidManifest(format!(
                        "session {name} has duplicate tag {tag}"
                    )));
                }
            }
        }

        Ok(())
    }

    /// Reconcile manifest drift against session files. Returns `true` when
    /// the manifest was changed and should be persisted by the caller.
    pub fn reconcile_manifest(
        &self,
        manifest: &mut WorkspaceManifest,
        existing_sessions: &[SessionInfo],
    ) -> Result<bool, WorkspaceError> {
        let existing: BTreeSet<String> = existing_sessions.iter().map(|s| s.name.clone()).collect();
        let folder_ids: BTreeSet<String> = manifest
            .folders
            .iter()
            .map(|f| f.id.to_ascii_lowercase())
            .collect();
        let mut changed = false;

        let before_len = manifest.sessions.len();
        manifest.sessions.retain(|name, _| existing.contains(name));
        changed |= manifest.sessions.len() != before_len;

        let mut next_order = manifest
            .sessions
            .values()
            .map(|org| org.order)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        for info in existing_sessions {
            if !manifest.sessions.contains_key(&info.name) {
                manifest.sessions.insert(
                    info.name.clone(),
                    SessionOrg {
                        order: next_order,
                        ..SessionOrg::default()
                    },
                );
                next_order = next_order.saturating_add(1);
                changed = true;
            }
        }

        for (name, org) in manifest.sessions.iter_mut() {
            if org
                .folder_id
                .as_deref()
                .is_some_and(|folder_id| !folder_ids.contains(&folder_id.to_ascii_lowercase()))
            {
                warn!(
                    session = %name,
                    "workspace: manifest session had dangling folder_id; moving to root"
                );
                org.folder_id = None;
                changed = true;
            }
        }

        self.validate_manifest(manifest, existing_sessions)?;
        Ok(changed)
    }

    /// Write the manifest atomically and return its SHA256-128 ETag hex.
    pub fn write_manifest(&self, manifest: &WorkspaceManifest) -> Result<String, WorkspaceError> {
        let bytes = manifest_canonical_bytes(manifest)?;
        let path = self.manifest_path();
        let dir = path.parent().ok_or_else(|| {
            WorkspaceError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "manifest path has no parent",
            ))
        })?;
        ensure_dir_0700(dir)?;
        let mut f = AwfOpenOptions::new()
            .mode(SESSION_FILE_MODE)
            .preserve_mode(false)
            .open(&path)
            .map_err(|e| WorkspaceError::Io(e.into()))?;
        f.write_all(&bytes).map_err(WorkspaceError::Io)?;
        f.commit().map_err(|e| WorkspaceError::Io(e.into()))?;
        manifest_etag_hex_from_bytes(&bytes)
    }

    /// Compute the manifest ETag without writing.
    pub fn manifest_etag_hex(
        &self,
        manifest: &WorkspaceManifest,
    ) -> Result<String, WorkspaceError> {
        let bytes = manifest_canonical_bytes(manifest)?;
        manifest_etag_hex_from_bytes(&bytes)
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

/// `${XDG_DATA_HOME:-~/.local/share}/gtmux/` — the base for both the legacy
/// shared workspace and the per-instance workspaces dir.
fn xdg_gtmux_data_dir() -> Result<PathBuf, WorkspaceError> {
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
    Ok(base.join("gtmux"))
}

/// Instance-isolated Store default (ADR-0045 D5):
/// `${XDG_DATA_HOME}/gtmux/store/<instance>/`.
fn instance_store_path(instance: &str) -> Result<PathBuf, WorkspaceError> {
    Ok(xdg_gtmux_data_dir()?.join("store").join(instance))
}

/// Legacy ADR-0044 Store location: `${XDG_DATA_HOME}/gtmux/workspaces/<instance>/`.
/// Only used by the back-compat branch in [`WorkspaceManager::resolve`].
fn instance_workspaces_path(instance: &str) -> Result<PathBuf, WorkspaceError> {
    Ok(xdg_gtmux_data_dir()?.join("workspaces").join(instance))
}

/// Legacy pre-ADR-0044 *shared* Store location: `${XDG_DATA_HOME}/gtmux/workspace/`.
/// Only used by the back-compat branch in [`WorkspaceManager::resolve`].
fn legacy_shared_workspace_path() -> Result<PathBuf, WorkspaceError> {
    Ok(xdg_gtmux_data_dir()?.join("workspace"))
}

/// `true` when `dir` exists and contains at least one valid `<name>.json`
/// session record. Used to decide whether the legacy shared dir is worth
/// reusing (ADR-0044 D-A3 back-compat). A missing/unreadable dir → `false`.
fn dir_has_session_record(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .flatten()
        .any(|e| session_name_from_path(&e.path()).is_some())
}

fn manifest_canonical_bytes(manifest: &WorkspaceManifest) -> Result<Vec<u8>, WorkspaceError> {
    serde_json::to_vec(manifest)
        .map_err(|e| WorkspaceError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
}

fn manifest_etag_hex_from_bytes(bytes: &[u8]) -> Result<String, WorkspaceError> {
    let d = digest(&SHA256, bytes);
    let full = d.as_ref();
    let mut hex = String::with_capacity(32);
    {
        use std::fmt::Write as _;
        for b in &full[..16] {
            let _ = write!(hex, "{b:02x}");
        }
    }
    Ok(hex)
}

fn validate_display_name(
    name: &str,
    field: &'static str,
    max_len: usize,
) -> Result<(), WorkspaceError> {
    if name.trim() != name || name.is_empty() || name.len() > max_len {
        return Err(WorkspaceError::InvalidManifest(format!(
            "{field} must be trimmed and 1..={max_len} bytes"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(WorkspaceError::InvalidManifest(format!(
            "{field} contains a control character"
        )));
    }
    Ok(())
}

fn validate_tag(tag: &str) -> Result<(), WorkspaceError> {
    let bytes = tag.as_bytes();
    if bytes.is_empty() || bytes.len() > 32 {
        return Err(WorkspaceError::InvalidManifest(
            "tag must be 1..=32 bytes".into(),
        ));
    }
    if !bytes
        .iter()
        .all(|b| matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
    {
        return Err(WorkspaceError::InvalidManifest(format!(
            "invalid tag: {tag}"
        )));
    }
    Ok(())
}

fn validate_uuid_shape(value: &str, field: &'static str) -> Result<(), WorkspaceError> {
    let b = value.as_bytes();
    let ok = b.len() == 36
        && [8, 13, 18, 23].into_iter().all(|i| b[i] == b'-')
        && b.iter().enumerate().all(|(i, ch)| {
            [8, 13, 18, 23].contains(&i) || matches!(ch, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
        });
    if ok {
        Ok(())
    } else {
        Err(WorkspaceError::InvalidManifest(format!(
            "{field} must be a hyphenated UUID"
        )))
    }
}

fn validate_folder_acyclic(folders: &[WorkspaceFolder]) -> Result<(), WorkspaceError> {
    let parent_by_id: BTreeMap<String, Option<String>> = folders
        .iter()
        .map(|folder| {
            (
                folder.id.to_ascii_lowercase(),
                folder.parent_id.as_ref().map(|id| id.to_ascii_lowercase()),
            )
        })
        .collect();
    for folder in folders {
        let mut seen = BTreeSet::new();
        let mut cursor = Some(folder.id.to_ascii_lowercase());
        while let Some(id) = cursor {
            if !seen.insert(id.clone()) {
                return Err(WorkspaceError::InvalidManifest(format!(
                    "folder cycle detected at {id}"
                )));
            }
            cursor = parent_by_id.get(&id).cloned().flatten();
        }
    }
    Ok(())
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
        let mode = std::fs::metadata(wm.locks_dir())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
        let wmode = std::fs::metadata(wm.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(wmode, 0o700);
    }

    #[test]
    fn session_name_validation() {
        for ok in ["a", "demo-build", "build_monitor", "x".repeat(64).as_str()] {
            assert!(validate_session_name(ok).is_ok(), "expected ok: {ok:?}");
        }
        for bad in ["", "../etc", "a/b", "a.b", "a b", "ab\0", &"x".repeat(65)] {
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
    fn manifest_reconcile_appends_missing_and_drops_deleted() {
        let (_dir, wm) = fresh_manager();
        let infos = vec![
            SessionInfo {
                name: "alpha".into(),
                active: false,
            },
            SessionInfo {
                name: "gamma".into(),
                active: false,
            },
        ];
        let mut manifest = WorkspaceManifest::default();
        manifest.sessions.insert(
            "alpha".into(),
            SessionOrg {
                order: 4,
                ..SessionOrg::default()
            },
        );
        manifest.sessions.insert(
            "beta".into(),
            SessionOrg {
                order: 5,
                ..SessionOrg::default()
            },
        );

        assert!(wm.reconcile_manifest(&mut manifest, &infos).unwrap());
        assert!(manifest.sessions.contains_key("alpha"));
        assert!(!manifest.sessions.contains_key("beta"));
        assert_eq!(manifest.sessions["gamma"].folder_id, None);
        assert_eq!(manifest.sessions["gamma"].order, 5);
    }

    #[test]
    fn manifest_validate_rejects_folder_cycle() {
        let (_dir, wm) = fresh_manager();
        let infos = Vec::new();
        let mut manifest = WorkspaceManifest::default();
        manifest.folders = vec![
            WorkspaceFolder {
                id: "11111111-1111-4111-8111-111111111111".into(),
                name: "A".into(),
                parent_id: Some("22222222-2222-4222-8222-222222222222".into()),
                order: 0,
                color: None,
                collapsed: false,
            },
            WorkspaceFolder {
                id: "22222222-2222-4222-8222-222222222222".into(),
                name: "B".into(),
                parent_id: Some("11111111-1111-4111-8111-111111111111".into()),
                order: 1,
                color: None,
                collapsed: false,
            },
        ];

        assert!(matches!(
            wm.validate_manifest(&manifest, &infos),
            Err(WorkspaceError::InvalidManifest(_))
        ));
    }

    #[test]
    fn manifest_write_read_round_trip_with_etag() {
        let (dir, wm) = fresh_manager();
        std::fs::write(dir.path().join("alpha.json"), b"{}").unwrap();
        let infos = wm.enumerate_sessions().unwrap();
        let mut manifest = WorkspaceManifest::default();
        manifest.sessions.insert(
            "alpha".into(),
            SessionOrg {
                folder_id: None,
                order: 7,
                tags: vec!["p0".into()],
                favorite: true,
            },
        );
        wm.validate_manifest(&manifest, &infos).unwrap();

        let etag = wm.write_manifest(&manifest).unwrap();
        assert_eq!(etag.len(), 32);
        let loaded = wm.read_manifest().unwrap();
        assert_eq!(loaded, manifest);
        assert_eq!(wm.manifest_etag_hex(&loaded).unwrap(), etag);
    }

    #[test]
    fn boot_migration_v1_to_v2_rewrites_in_place() {
        let (dir, wm) = fresh_manager();
        let v1 = json!({
            "schema_version": 1,
            "groups": [],
            "panels": [{ "id": "%2", "x": 1, "y": 2 }],
        });
        std::fs::write(
            dir.path().join("alpha.json"),
            serde_json::to_vec(&v1).unwrap(),
        )
        .unwrap();

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
        std::fs::write(
            dir.path().join("rotten.json"),
            serde_json::to_vec(&bad).unwrap(),
        )
        .unwrap();
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
        let err =
            WorkspaceManager::resolve(Some(PathBuf::from("rel/path")), None, "demo").unwrap_err();
        assert!(matches!(err, WorkspaceError::NonAbsolutePath(_)));
    }

    // ──────────────────────────────────────────────────────────────────────
    //  ADR-0044 D-A3 — instance-isolated default + legacy-shared back-compat.
    //  These mutate XDG_DATA_HOME / HOME, so they serialise on a shared lock.
    // ──────────────────────────────────────────────────────────────────────

    use std::sync::Mutex;
    static XDG_LOCK: Mutex<()> = Mutex::new(());

    struct XdgGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev_data: Option<std::ffi::OsString>,
        prev_home: Option<std::ffi::OsString>,
        _dir: TempDir,
    }

    impl XdgGuard {
        fn new() -> (Self, PathBuf) {
            let lock = XDG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev_data = std::env::var_os("XDG_DATA_HOME");
            let prev_home = std::env::var_os("HOME");
            let dir = TempDir::new().unwrap();
            std::env::set_var("XDG_DATA_HOME", dir.path());
            std::env::remove_var("HOME");
            let data_root = dir.path().to_path_buf();
            (
                Self {
                    _lock: lock,
                    prev_data,
                    prev_home,
                    _dir: dir,
                },
                data_root,
            )
        }
    }

    impl Drop for XdgGuard {
        fn drop(&mut self) {
            match &self.prev_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
            match &self.prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn resolve_default_is_store_instance_isolated() {
        let (_g, data_root) = XdgGuard::new();
        let wm = WorkspaceManager::resolve(None, None, "demo").unwrap();
        assert_eq!(wm.path(), data_root.join("gtmux/store/demo"));
        assert!(!wm.is_legacy_shared());
    }

    #[test]
    fn resolve_two_instances_get_separate_dirs() {
        let (_g, _data_root) = XdgGuard::new();
        let a = WorkspaceManager::resolve(None, None, "alpha").unwrap();
        let b = WorkspaceManager::resolve(None, None, "beta").unwrap();
        assert_ne!(a.path(), b.path());
    }

    #[test]
    fn resolve_back_compat_reuses_legacy_shared_when_populated() {
        let (_g, data_root) = XdgGuard::new();
        // Seed the pre-0044 shared dir with a valid record; no store/ or
        // workspaces/ dir present.
        let legacy = data_root.join("gtmux/workspace");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("scratch.json"), b"{}").unwrap();

        let wm = WorkspaceManager::resolve(None, None, "demo").unwrap();
        assert_eq!(wm.path(), legacy, "must reuse pre-0044 shared dir");
        assert!(wm.is_legacy_shared());
    }

    #[test]
    fn resolve_back_compat_reuses_legacy_0044_workspaces_when_populated() {
        let (_g, data_root) = XdgGuard::new();
        // ADR-0044 `workspaces/<instance>` holds a record and store/<instance>
        // is absent → reuse the 0044 dir (preferred over pre-0044 shared).
        let legacy_0044 = data_root.join("gtmux/workspaces/demo");
        std::fs::create_dir_all(&legacy_0044).unwrap();
        std::fs::write(legacy_0044.join("scratch.json"), b"{}").unwrap();
        // pre-0044 shared also populated — 0044 must win.
        let shared = data_root.join("gtmux/workspace");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("other.json"), b"{}").unwrap();

        let wm = WorkspaceManager::resolve(None, None, "demo").unwrap();
        assert_eq!(wm.path(), legacy_0044, "0044 workspaces/<instance> wins");
        assert!(wm.is_legacy_shared());
    }

    #[test]
    fn resolve_back_compat_skipped_when_store_dir_exists() {
        let (_g, data_root) = XdgGuard::new();
        let legacy = data_root.join("gtmux/workspace");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("scratch.json"), b"{}").unwrap();
        // store/<instance> already present → it wins regardless of legacy data.
        let store_dir = data_root.join("gtmux/store/demo");
        std::fs::create_dir_all(&store_dir).unwrap();

        let wm = WorkspaceManager::resolve(None, None, "demo").unwrap();
        assert_eq!(wm.path(), store_dir);
        assert!(!wm.is_legacy_shared());
    }

    #[test]
    fn resolve_back_compat_skipped_when_legacy_empty() {
        let (_g, data_root) = XdgGuard::new();
        // Legacy dirs exist but hold no valid session record.
        let legacy = data_root.join("gtmux/workspace");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("readme.txt"), b"hi").unwrap();

        let wm = WorkspaceManager::resolve(None, None, "demo").unwrap();
        assert_eq!(wm.path(), data_root.join("gtmux/store/demo"));
        assert!(!wm.is_legacy_shared());
    }

    #[test]
    fn resolve_explicit_cli_override_never_legacy() {
        let (_g, _data_root) = XdgGuard::new();
        let dir = TempDir::new().unwrap();
        let wm = WorkspaceManager::resolve(Some(dir.path().to_path_buf()), None, "demo").unwrap();
        assert_eq!(wm.path(), dir.path());
        assert!(!wm.is_legacy_shared());
    }
}
