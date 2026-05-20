//! Slice D-2 — `/api/file-path/*` endpoint group (BE-NEW-12).
//!
//! Spec source: ADR-0023 (file_path Item 의 OS-level Open 보안 정책,
//! Accepted 2026-05-15 + amend ① 2026-05-16) +
//! `docs/reports/0044-be-slice-d-work-package.md` §3.5-§3.8.
//!
//! ## Wire surface
//!
//! ```text
//! GET    /api/file-path/allowlist                       → 200 { entries: [...] }
//! POST   /api/file-path/allowlist                       → 201 { ext, prefix, added_at, label? }
//! DELETE /api/file-path/allowlist?ext=&prefix=          → 204 | 404
//! GET    /api/file-path/allowlist-check?path=<p>        → 200 { allowed, matched_entry | reason }
//! POST   /api/file-path/open { path, user_confirmed? }  → 200 | 400 | 403 | 500
//! ```
//!
//! ## Storage
//!
//! `${XDG_CONFIG_HOME:-~/.config}/gtmux/file-open-allowlist.json` — JSON
//! array of `{ ext, prefix, added_at, label? }` rows. Atomic write via
//! `atomic-write-file` to avoid torn writes.
//!
//! ## Match rule (ADR-0023 D2)
//!
//! ```text
//! allow(path) := ∃ entry ∈ allowlist :
//!     path.starts_with(entry.prefix)
//!     ∧ path.to_lower().ends_with("." + entry.ext.to_lower())
//! ```
//!
//! - `ext` is case-insensitive (`md` ≡ `MD`) — stored lowercased.
//! - `prefix` is case-sensitive POSIX; must be an absolute, canonical
//!   directory with a trailing `/`.
//! - Recursive subdir match — `entry.prefix == "/a/b/"` matches `/a/b/c/x.md`.
//!
//! ## Open flow (ADR-0023 D5/D6)
//!
//! ```text
//! POST /open { path, user_confirmed }:
//!   1. path absolute? → no: 400 path_not_absolute
//!   2. NUL byte?      → yes: 400 nul_byte
//!   3. canonicalize(path) → fail: 400 path_not_exists
//!   4. allowlist match canonical(path)?
//!      yes → spawn OS open, audit `allowed_via: "allowlist"`, 200
//!      no:
//!        user_confirmed == true → spawn, audit "one_time", 200
//!        user_confirmed == false → 403 user_confirmation_required, audit "denied"
//!   5. spawn fail → 500 spawn_failed
//! ```
//!
//! The `user_confirmed=true` bypass is gated by ADR-0020's cookie
//! SameSite=Strict + ADR-0003's Origin/Host allowlist — CSRF surface is
//! small. ADR-0023 D6's pre-issued nonce is **deferred to P1+**
//! defense-in-depth (per ADR-0023 amend ①).
//!
//! ## Audit log (ADR-0023 D9)
//!
//! Every `POST /open` call writes one NDJSON line to
//! `${XDG_STATE_HOME:-~/.local/state}/gtmux/audit/file-open-YYYYMMDD.log`
//! including denied attempts. Fields: `ts` (epoch), `path`, `allowed_via`
//! (`"allowlist"|"one_time"|"denied"`), `reason` (on deny), `cookie_prefix`
//! (first 8 chars of session cookie for correlation without exposing it).

mod allowlist;
mod audit;
mod handlers;
mod spawn;

pub use allowlist::{Allowlist, AllowlistEntry, AllowlistError, AllowlistMatch};
pub use audit::AuditLog;
pub use handlers::{
    allowlist_check_handler, allowlist_delete_handler, allowlist_get_handler,
    allowlist_post_handler, open_handler,
};

use std::path::PathBuf;
use std::sync::Arc;

/// Resolve the on-disk allowlist path, honouring `XDG_CONFIG_HOME` per
/// ADR-0023 D3.
pub fn default_allowlist_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join(".config")
        });
    base.join("gtmux").join("file-open-allowlist.json")
}

/// Resolve the audit log directory under `XDG_STATE_HOME` per
/// ADR-0023 D9.
pub fn default_audit_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join(".local").join("state")
        });
    base.join("gtmux").join("audit")
}

/// Shared per-AppState bundle wiring the persistent allowlist file +
/// audit log writer. Construct once in `AppState::new` so the handler
/// path can pull both from `State<AppState>` without re-reading the
/// allowlist file on every request.
#[derive(Clone)]
pub struct FileOpenContext {
    /// Persistent `(ext, prefix)` allowlist. Guarded by `RwLock` so the
    /// hot path (GET / check) is shared-read and the slow path
    /// (POST/DELETE → disk write) is serialised.
    pub allowlist: Arc<tokio::sync::RwLock<Allowlist>>,
    /// NDJSON audit log writer. The `Arc` is shared across handlers
    /// so every record lands in the same daily file.
    pub audit: Arc<AuditLog>,
}

impl FileOpenContext {
    /// Build the production context: load (or create) the allowlist at
    /// `default_allowlist_path()` and pin the audit log under
    /// `default_audit_dir()`. Errors during initial load degrade to an
    /// empty allowlist so the server still boots (the failure is logged
    /// at warn).
    pub fn production() -> Self {
        let path = default_allowlist_path();
        let allowlist = match Allowlist::load(&path) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    path = %path.display(),
                    "file_open: failed to load allowlist; starting empty"
                );
                Allowlist::empty(path.clone())
            }
        };
        let audit = AuditLog::new(default_audit_dir());
        Self {
            allowlist: Arc::new(tokio::sync::RwLock::new(allowlist)),
            audit: Arc::new(audit),
        }
    }

    /// Construct a context for tests with an in-memory allowlist and a
    /// caller-supplied audit dir. The allowlist is rooted at
    /// `tmp_path/file-open-allowlist.json` (must not yet exist or be
    /// readable — `Allowlist::empty` is the seed).
    #[cfg(test)]
    pub fn for_tests(allowlist_path: PathBuf, audit_dir: PathBuf) -> Self {
        let allowlist = Allowlist::empty(allowlist_path);
        let audit = AuditLog::new(audit_dir);
        Self {
            allowlist: Arc::new(tokio::sync::RwLock::new(allowlist)),
            audit: Arc::new(audit),
        }
    }
}
