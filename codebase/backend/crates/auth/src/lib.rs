//! gtmux-auth — 256-bit CSPRNG 토큰 발급 / 영속화 / 회전 / 상수시간 비교.
//!
//! ADR-0003 D4·D13, `docs/ssot/security-defaults.md` §1.3, ADR-0011 D8을
//! 단일 SSoT로 잠금 구현한다.
//!
//! - CSPRNG: `ring::rand::SystemRandom`, raw 32 bytes
//! - Encoding: base64url no-pad (43 chars)
//! - Storage: `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token`
//! - File perm 0600, parent dir perm 0700 (fail-closed on load)
//! - Compare: `ring::constant_time::verify_slices_are_equal` (raw bytes)
//! - Atomic write: tempfile → chmod 0600 → fsync → rename → fsync(dir)

#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::rand::{SecureRandom, SystemRandom};
use thiserror::Error;

/// CSPRNG output length (D13.2 — 256-bit).
const TOKEN_BYTE_LEN: usize = 32;
/// Required file permission (D13.3, octal 0o600 == decimal 384).
const TOKEN_FILE_PERM: u32 = 0o600;
/// Required parent directory permission (D13.3, octal 0o700 == decimal 448).
const TOKEN_DIR_PERM: u32 = 0o700;
/// Permission bits we care about — sticky/setuid bits are ignored to match
/// the SSoT comparison semantics (`mode & 0o777`).
const PERM_MASK: u32 = 0o777;

/// A base64url-encoded 32-byte token. The inner `String` is the *encoded*
/// form; comparisons must always go through [`verify_token`] which decodes
/// and runs constant-time over raw bytes.
#[derive(Clone, Debug)]
pub struct TokenString(pub String);

impl TokenString {
    /// Decode to raw bytes. Returns `BadEncoding` if the inner string is not
    /// valid base64url-no-pad or does not decode to exactly 32 bytes.
    fn decode_raw(&self) -> Result<[u8; TOKEN_BYTE_LEN]> {
        let bytes = URL_SAFE_NO_PAD
            .decode(self.0.as_bytes())
            .map_err(|_| AuthError::BadEncoding)?;
        if bytes.len() != TOKEN_BYTE_LEN {
            return Err(AuthError::BadEncoding);
        }
        let mut out = [0u8; TOKEN_BYTE_LEN];
        out.copy_from_slice(&bytes);
        Ok(out)
    }
}

/// Errors surfaced by the auth crate. All variants are fail-closed: any
/// non-`Ok` path must abort the operation without producing a token.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// File or parent directory permission mismatch — D13.3 fail-closed.
    #[error("permission mismatch on {path}: expected {expected:#o}, got {actual:#o}")]
    BadPerm {
        path: PathBuf,
        expected: u32,
        actual: u32,
    },

    /// Token bytes were not valid base64url-no-pad or wrong length.
    #[error("token encoding invalid (must be base64url, 32 bytes decoded)")]
    BadEncoding,

    /// Token file missing — caller must distinguish "first run" from "load".
    #[error("token file not found: {0}")]
    NotFound(PathBuf),

    /// CSPRNG fill failure (extremely rare — surfaces `ring::error::Unspecified`).
    #[error("csprng failure")]
    Csprng,

    /// `$HOME` not set when defaulting `$XDG_STATE_HOME` (`~/.local/state`).
    #[error("$HOME not set; cannot resolve $XDG_STATE_HOME default")]
    HomeUnset,
}

pub type Result<T> = std::result::Result<T, AuthError>;

/// Generate a fresh 256-bit token using the OS CSPRNG (`ring::rand::SystemRandom`).
///
/// Returns the token as base64url no-pad (43 chars). The raw bytes never leave
/// this function as a `Vec` — only the encoded form is exposed.
pub fn issue_token() -> Result<TokenString> {
    let rng = SystemRandom::new();
    let mut raw = [0u8; TOKEN_BYTE_LEN];
    rng.fill(&mut raw).map_err(|_| AuthError::Csprng)?;
    Ok(TokenString(URL_SAFE_NO_PAD.encode(raw)))
}

/// Persist a token to `$XDG_STATE_HOME/gtmux/<session>.token` with mode 0600.
///
/// Atomic semantics (D13.3 + R5 §E.1 fail-closed):
/// 1. Ensure parent directory exists with mode 0700.
/// 2. Create temp file in the *same* directory with `O_CREAT | O_EXCL`, mode 0600.
/// 3. Write token bytes, `fsync` the file.
/// 4. `rename` temp → final (atomic on POSIX, same filesystem guaranteed).
/// 5. `fsync` the parent directory so the rename is durable.
///
/// The temp file is removed on any error path before the rename succeeds, so
/// no half-written `.tmp` artifacts survive a crash mid-write.
pub fn save_token(session_name: &str, token: &TokenString) -> Result<()> {
    let final_path = token_path(session_name)?;
    let dir = final_path
        .parent()
        .expect("token_path always returns a path with a parent");
    ensure_state_dir(dir)?;

    // Temp file lives in the same directory so `rename` is atomic on POSIX.
    // We embed the PID to keep concurrent rotations from colliding on the
    // same temp name; the file is created with O_EXCL as a safety net.
    let tmp_path = dir.join(format!(
        "{}.{}.tmp",
        final_path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("token"),
        std::process::id()
    ));

    // Scope the file handle so it closes before rename — required on some
    // filesystems (and avoids holding an fd through the rename point).
    let write_result = (|| -> Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(TOKEN_FILE_PERM)
            .open(&tmp_path)?;
        // Some umasks may override the creation mode on older kernels — set
        // permissions explicitly post-open to guarantee 0600.
        let perm = fs::Permissions::from_mode(TOKEN_FILE_PERM);
        f.set_permissions(perm)?;
        f.write_all(token.0.as_bytes())?;
        f.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        // Best-effort cleanup; ignore failure (file may not exist).
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }

    // Durability fence on the directory. Failure here means the rename may
    // not survive a power loss, but the in-memory state is consistent.
    fsync_dir(dir)?;
    Ok(())
}

/// Load and validate the token for `session_name`.
///
/// Fail-closed checks (D13.3, SSoT §5 startup checklist step 4–5):
/// - Parent directory must exist with mode 0700.
/// - File must exist with mode exactly 0600.
/// - File contents must decode to exactly 32 base64url bytes.
pub fn load_token(session_name: &str) -> Result<TokenString> {
    let path = token_path(session_name)?;
    let dir = path
        .parent()
        .expect("token_path always returns a path with a parent");

    check_perm(dir, TOKEN_DIR_PERM)?;

    if !path.exists() {
        return Err(AuthError::NotFound(path));
    }
    check_perm(&path, TOKEN_FILE_PERM)?;

    let mut f = File::open(&path)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    let token = TokenString(buf.trim().to_string());
    // Validate encoding eagerly so callers never hold an undecodeable token.
    let _ = token.decode_raw()?;
    Ok(token)
}

/// Constant-time compare of a presented token (raw or encoded) against a
/// stored token. Length mismatch returns `false` without invoking the
/// comparator (ring rejects length-mismatched slices anyway, but doing it
/// here keeps the contract explicit and panic-free).
///
/// Compare happens on the *decoded raw bytes*: this avoids any base64
/// canonicalisation pitfalls (e.g. trailing whitespace, alternate padding)
/// influencing the timing path.
pub fn verify_token(presented: &str, stored: &TokenString) -> bool {
    let Ok(stored_raw) = stored.decode_raw() else {
        return false;
    };
    let Ok(presented_decoded) = URL_SAFE_NO_PAD.decode(presented.as_bytes()) else {
        return false;
    };
    if presented_decoded.len() != stored_raw.len() {
        return false;
    }
    // ring 0.17 marks `verify_slices_are_equal` as deprecated but still
    // re-exports it from `constant_time`. ADR-0011 D8 + SSoT §1.3
    // (`token_compare = "constant_time"`) name this exact function as the
    // canonical comparator. R7-T2 may switch to `subtle::ConstantTimeEq`
    // pending the licence/binary-size review — until then, the allow is
    // load-bearing on the SSoT and is *not* a correctness compromise
    // (the implementation is unchanged, only the symbol is renamed).
    #[allow(deprecated)]
    let eq = ring::constant_time::verify_slices_are_equal(&presented_decoded, &stored_raw);
    eq.is_ok()
}

/// Issue a new token and overwrite the existing file atomically. After this
/// returns successfully, any previously-issued token for the session is
/// immediately invalid (D13.1 cloud rotation semantics; D17 c7 also requires
/// the WS layer to close 4001 — that lives in `ws-server`, not here).
pub fn rotate_token(session_name: &str) -> Result<TokenString> {
    let fresh = issue_token()?;
    save_token(session_name, &fresh)?;
    Ok(fresh)
}

// -------- internal helpers --------

fn token_path(session_name: &str) -> Result<PathBuf> {
    let dir = state_home()?.join("gtmux");
    Ok(dir.join(format!("{session_name}.token")))
}

fn state_home() -> Result<PathBuf> {
    if let Some(s) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(s));
    }
    // Spec default: `~/.local/state`. `$HOME` is required when XDG is unset.
    let home = std::env::var_os("HOME").ok_or(AuthError::HomeUnset)?;
    Ok(PathBuf::from(home).join(".local").join("state"))
}

/// Create the state directory if missing and tighten its mode to 0700.
/// An existing directory with broader perms is fixed in place (matches
/// SSoT §1.10 `process.umask = 0o077` intent — we own this directory).
fn ensure_state_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    // Force 0700 regardless of umask; same-user only.
    let perm = fs::Permissions::from_mode(TOKEN_DIR_PERM);
    fs::set_permissions(dir, perm)?;
    Ok(())
}

fn check_perm(path: &Path, expected: u32) -> Result<()> {
    let meta = fs::metadata(path)?;
    let actual = meta.permissions().mode() & PERM_MASK;
    if actual != expected {
        return Err(AuthError::BadPerm {
            path: path.to_path_buf(),
            expected,
            actual,
        });
    }
    Ok(())
}

/// `fsync` a directory by opening it read-only and calling `sync_all`.
/// Required for rename durability on POSIX (rename metadata lives in the
/// directory inode, not the file).
fn fsync_dir(dir: &Path) -> Result<()> {
    let d = File::open(dir)?;
    d.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // `XDG_STATE_HOME` is process-global; serialise tests that mutate it.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev: Option<std::ffi::OsString>,
        _tmp: TempDir,
    }

    impl EnvGuard {
        fn new() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev = std::env::var_os("XDG_STATE_HOME");
            let tmp = tempfile::tempdir().expect("tempdir");
            // SAFETY note: we hold ENV_LOCK so no other test mutates env.
            std::env::set_var("XDG_STATE_HOME", tmp.path());
            Self {
                _lock: lock,
                prev,
                _tmp: tmp,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
        }
    }

    fn gtmux_dir() -> PathBuf {
        state_home().unwrap().join("gtmux")
    }

    #[test]
    fn roundtrip() {
        let _g = EnvGuard::new();
        let issued = issue_token().unwrap();
        save_token("alpha", &issued).unwrap();
        let loaded = load_token("alpha").unwrap();
        assert_eq!(issued.0, loaded.0);
        assert!(verify_token(&issued.0, &loaded));
    }

    #[test]
    fn verify_rejects_wrong() {
        let _g = EnvGuard::new();
        let a = issue_token().unwrap();
        let b = issue_token().unwrap();
        assert_ne!(a.0, b.0);
        assert!(!verify_token(&a.0, &b));
    }

    #[test]
    fn verify_length_mismatch() {
        let _g = EnvGuard::new();
        let stored = issue_token().unwrap();
        // Short input — decodes to <32 bytes.
        assert!(!verify_token("AAAA", &stored));
        // Long input — decodes to >32 bytes.
        let long = "A".repeat(64);
        assert!(!verify_token(&long, &stored));
        // Empty input.
        assert!(!verify_token("", &stored));
        // Invalid base64.
        assert!(!verify_token("!!!not_base64!!!", &stored));
    }

    #[test]
    fn perm_0600_enforced() {
        let _g = EnvGuard::new();
        let t = issue_token().unwrap();
        save_token("perm-check", &t).unwrap();
        let path = gtmux_dir().join("perm-check.token");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "file mode must be 0600, got {:o}", mode);
        let dir_mode = fs::metadata(gtmux_dir()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "dir mode must be 0700, got {:o}", dir_mode);
    }

    #[test]
    fn perm_rejection_on_load() {
        let _g = EnvGuard::new();
        let t = issue_token().unwrap();
        save_token("loose", &t).unwrap();
        let path = gtmux_dir().join("loose.token");
        // Loosen to 0644 → load must reject.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let err = load_token("loose").unwrap_err();
        assert!(
            matches!(
                err,
                AuthError::BadPerm {
                    actual: 0o644,
                    expected: 0o600,
                    ..
                }
            ),
            "expected BadPerm{{0o644→0o600}}, got {err:?}"
        );
    }

    #[test]
    fn perm_rejection_on_dir() {
        let _g = EnvGuard::new();
        let t = issue_token().unwrap();
        save_token("dirloose", &t).unwrap();
        // Loosen the gtmux dir to 0755 → load must reject before reading file.
        fs::set_permissions(gtmux_dir(), fs::Permissions::from_mode(0o755)).unwrap();
        let err = load_token("dirloose").unwrap_err();
        assert!(
            matches!(
                err,
                AuthError::BadPerm {
                    actual: 0o755,
                    expected: 0o700,
                    ..
                }
            ),
            "expected BadPerm on directory, got {err:?}"
        );
    }

    #[test]
    fn token_length_32_bytes() {
        let _g = EnvGuard::new();
        let t = issue_token().unwrap();
        let raw = URL_SAFE_NO_PAD.decode(t.0.as_bytes()).unwrap();
        assert_eq!(raw.len(), 32);
        // Encoded length is 43 chars for 32 raw bytes (no padding).
        assert_eq!(t.0.len(), 43);
    }

    #[test]
    fn rotate_invalidates_old() {
        let _g = EnvGuard::new();
        let old = issue_token().unwrap();
        save_token("rot", &old).unwrap();
        let new = rotate_token("rot").unwrap();
        assert_ne!(old.0, new.0);
        let loaded = load_token("rot").unwrap();
        assert_eq!(loaded.0, new.0);
        // Old token must not verify against the rotated stored value.
        assert!(!verify_token(&old.0, &loaded));
        assert!(verify_token(&new.0, &loaded));
    }

    #[test]
    fn atomic_write_no_partial() {
        let _g = EnvGuard::new();
        let t = issue_token().unwrap();
        save_token("atomic", &t).unwrap();
        // After a successful save, no `.tmp` artifacts should remain in
        // the gtmux directory.
        let entries: Vec<_> = fs::read_dir(gtmux_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap_or_default())
            .collect();
        let stragglers: Vec<_> = entries
            .iter()
            .filter(|n| n.ends_with(".tmp"))
            .cloned()
            .collect();
        assert!(
            stragglers.is_empty(),
            "found temp-file residue: {stragglers:?}"
        );
        // And the canonical file must exist.
        assert!(entries.contains(&"atomic.token".to_string()));
    }

    #[test]
    fn load_missing_returns_not_found() {
        let _g = EnvGuard::new();
        // Touch the gtmux dir with correct perms so the perm gate passes
        // and we actually exercise the NotFound branch.
        let dir = gtmux_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let err = load_token("never-issued").unwrap_err();
        assert!(matches!(err, AuthError::NotFound(_)), "got {err:?}");
    }
}
