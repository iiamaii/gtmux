//! `Allowlist` — persistent set of `(ext, prefix)` entries. JSON on
//! disk, in-memory `Vec` for O(N) match. N is expected to be small
//! (<100 entries even for power users) so linear scan is fine.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Single allowlist entry — `(ext, prefix)` tuple per ADR-0023 D2.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AllowlistEntry {
    /// File extension *without* the leading dot. Stored lowercased; the
    /// match path also lowercases its tail so the compare is
    /// case-insensitive (per ADR-0023 D2).
    pub ext: String,
    /// Absolute, canonical directory path with a trailing `/`. Compared
    /// case-sensitively (POSIX semantics).
    pub prefix: String,
    /// Unix epoch seconds at which the entry was first added.
    pub added_at: u64,
    /// Optional human-readable label shown in the Settings Storage UI.
    /// `None` is serialised as `null`.
    pub label: Option<String>,
}

/// Outcome of an `allowlist.check(path)` call. The matched entry (if
/// any) is exposed so the handler can echo it back to the FE
/// (`matched_entry` in the wire shape).
#[derive(Debug, Clone)]
pub enum AllowlistMatch<'a> {
    /// `path` matched the carried `AllowlistEntry` — modal bypass.
    Allowed(&'a AllowlistEntry),
    /// `path` did not match any entry — FE must surface the confirm modal.
    Denied,
}

/// In-memory allowlist with its on-disk path. The path is held so the
/// handler can persist after `add` / `remove` without re-plumbing.
#[derive(Debug)]
pub struct Allowlist {
    path: PathBuf,
    entries: Vec<AllowlistEntry>,
}

#[derive(Debug, Error)]
pub enum AllowlistError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("validation: {0}")]
    Validation(&'static str),
}

#[derive(Debug, Serialize, Deserialize)]
struct OnDiskFile {
    entries: Vec<AllowlistEntry>,
}

impl Allowlist {
    /// Seed an empty allowlist anchored at `path` (no read, no write).
    /// Useful for tests and for the production fallback when the on-disk
    /// file is unreadable.
    pub fn empty(path: PathBuf) -> Self {
        Self {
            path,
            entries: Vec::new(),
        }
    }

    /// Load from disk. Missing file → empty (this is the cold-boot
    /// case; user has never added an entry). Corrupt JSON returns
    /// `Err(Parse)` so the caller can decide between failing the boot
    /// vs degrading.
    pub fn load(path: &Path) -> Result<Self, AllowlistError> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                return Ok(Self::empty(path.to_path_buf()))
            }
            Err(e) => return Err(AllowlistError::Io(e)),
        };
        let parsed: OnDiskFile = serde_json::from_slice(&bytes)?;
        Ok(Self {
            path: path.to_path_buf(),
            entries: parsed.entries,
        })
    }

    /// Atomically persist the current entries to disk. Parent dir is
    /// created on demand so first-write doesn't error on a fresh
    /// `~/.config/gtmux`.
    pub fn save(&self) -> Result<(), AllowlistError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_vec_pretty(&OnDiskFile {
            entries: self.entries.clone(),
        })?;
        let mut f = atomic_write_file::AtomicWriteFile::open(&self.path)?;
        use std::io::Write;
        f.write_all(&body)?;
        f.commit()?;
        Ok(())
    }

    /// Borrow the current entries in arbitrary order (insertion order
    /// today, but callers must not rely on it — the file may be
    /// re-sorted in a future amend without changing the wire contract).
    pub fn entries(&self) -> &[AllowlistEntry] {
        &self.entries
    }

    /// Returns `true` if any existing entry has the same `(ext, prefix)`
    /// tuple — the compound key is unique per ADR-0023 D2.
    pub fn contains(&self, ext: &str, prefix: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.ext == ext && e.prefix == prefix)
    }

    /// Add a fresh entry. Validates and normalises inputs:
    ///
    /// - `ext`: non-empty, no leading dot, no path separator (`/`, `\`).
    ///   Lowercased.
    /// - `prefix`: absolute, canonical, ends with `/`. The caller is
    ///   expected to have `canonicalize`d already (handler responsibility).
    /// - Duplicate `(ext, prefix)` returns `Err(Validation("duplicate"))`.
    pub fn add(
        &mut self,
        ext: &str,
        prefix: &str,
        label: Option<String>,
    ) -> Result<&AllowlistEntry, AllowlistError> {
        let ext = normalise_ext(ext)?;
        let prefix = normalise_prefix(prefix)?;
        if self.contains(&ext, &prefix) {
            return Err(AllowlistError::Validation("duplicate"));
        }
        let entry = AllowlistEntry {
            ext,
            prefix,
            added_at: now_epoch_seconds(),
            label,
        };
        self.entries.push(entry);
        self.save()?;
        Ok(self.entries.last().expect("just pushed"))
    }

    /// Remove a `(ext, prefix)` entry. Returns `Ok(true)` if removed,
    /// `Ok(false)` if not found.
    pub fn remove(&mut self, ext: &str, prefix: &str) -> Result<bool, AllowlistError> {
        let before = self.entries.len();
        self.entries
            .retain(|e| !(e.ext == ext && e.prefix == prefix));
        let removed = before != self.entries.len();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// Check whether `path` matches any entry. ADR-0023 D2 algorithm.
    pub fn check<'a>(&'a self, path: &Path) -> AllowlistMatch<'a> {
        let path_str = path.to_string_lossy();
        let lower_tail = path_str.to_lowercase();
        for entry in &self.entries {
            if !path_str.starts_with(&entry.prefix) {
                continue;
            }
            let needle = format!(".{}", entry.ext);
            if lower_tail.ends_with(&needle) {
                return AllowlistMatch::Allowed(entry);
            }
        }
        AllowlistMatch::Denied
    }
}

fn now_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Validate + lowercase the `ext` portion of an allowlist entry. The
/// returned `String` is safe to persist.
pub(crate) fn normalise_ext(raw: &str) -> Result<String, AllowlistError> {
    if raw.is_empty() {
        return Err(AllowlistError::Validation("ext_empty"));
    }
    if raw.starts_with('.') {
        return Err(AllowlistError::Validation("ext_contains_dot"));
    }
    if raw
        .chars()
        .any(|c| c == '/' || c == '\\' || c.is_whitespace() || c.is_control())
    {
        return Err(AllowlistError::Validation("ext_invalid"));
    }
    Ok(raw.to_ascii_lowercase())
}

/// Validate the `prefix` portion. The caller must have already run
/// `std::fs::canonicalize` on a user-supplied prefix; this function
/// just enforces the trailing `/` and absoluteness.
pub(crate) fn normalise_prefix(raw: &str) -> Result<String, AllowlistError> {
    if raw.is_empty() {
        return Err(AllowlistError::Validation("prefix_empty"));
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(AllowlistError::Validation("prefix_not_absolute"));
    }
    if !raw.ends_with('/') {
        return Err(AllowlistError::Validation("prefix_must_end_slash"));
    }
    Ok(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_check_match() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        a.add("md", "/tmp/notes/", None).unwrap();
        let m = a.check(Path::new("/tmp/notes/spec.md"));
        assert!(matches!(m, AllowlistMatch::Allowed(_)));
    }

    #[test]
    fn ext_case_insensitive() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        a.add("MD", "/tmp/notes/", None).unwrap();
        let m = a.check(Path::new("/tmp/notes/UPPER.MD"));
        assert!(matches!(m, AllowlistMatch::Allowed(_)));
    }

    #[test]
    fn prefix_case_sensitive() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        a.add("md", "/tmp/Notes/", None).unwrap();
        let m = a.check(Path::new("/tmp/notes/spec.md"));
        assert!(matches!(m, AllowlistMatch::Denied), "lowercase != Notes");
    }

    #[test]
    fn recursive_subdir_match() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        a.add("pdf", "/tmp/docs/", None).unwrap();
        let m = a.check(Path::new("/tmp/docs/2026/q1/report.pdf"));
        assert!(matches!(m, AllowlistMatch::Allowed(_)));
    }

    #[test]
    fn shell_script_not_auto_matched_under_md_prefix() {
        // The very attack vector ADR-0023 D2 cites: a user adds an `md`
        // prefix for /tmp/proj/, and a `.sh` file under the same prefix
        // must still hit confirm modal (the FE-NEW-8 path).
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        a.add("md", "/tmp/proj/", None).unwrap();
        let m = a.check(Path::new("/tmp/proj/payload.sh"));
        assert!(matches!(m, AllowlistMatch::Denied));
    }

    #[test]
    fn add_rejects_duplicate() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        a.add("md", "/tmp/notes/", None).unwrap();
        let err = a.add("md", "/tmp/notes/", None).unwrap_err();
        assert!(matches!(err, AllowlistError::Validation("duplicate")));
    }

    #[test]
    fn add_normalises_ext_lowercase() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        let entry = a.add("MD", "/tmp/notes/", None).unwrap().clone();
        assert_eq!(entry.ext, "md");
    }

    #[test]
    fn add_rejects_leading_dot_ext() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        let err = a.add(".md", "/tmp/notes/", None).unwrap_err();
        assert!(matches!(
            err,
            AllowlistError::Validation("ext_contains_dot")
        ));
    }

    #[test]
    fn add_rejects_prefix_without_slash() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        let err = a.add("md", "/tmp/notes", None).unwrap_err();
        assert!(matches!(
            err,
            AllowlistError::Validation("prefix_must_end_slash")
        ));
    }

    #[test]
    fn add_rejects_relative_prefix() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        let err = a.add("md", "notes/", None).unwrap_err();
        assert!(matches!(
            err,
            AllowlistError::Validation("prefix_not_absolute")
        ));
    }

    #[test]
    fn remove_returns_true_when_present() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut a = Allowlist::empty(dir.path().join("alist.json"));
        a.add("md", "/tmp/notes/", None).unwrap();
        assert!(a.remove("md", "/tmp/notes/").unwrap());
        assert!(!a.remove("md", "/tmp/notes/").unwrap());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("alist.json");
        let mut a = Allowlist::empty(path.clone());
        a.add("md", "/tmp/notes/", Some("Notes".to_string()))
            .unwrap();
        a.add("pdf", "/tmp/docs/", None).unwrap();
        // Reload from disk.
        let b = Allowlist::load(&path).unwrap();
        assert_eq!(b.entries().len(), 2);
        assert_eq!(b.entries()[0].ext, "md");
        assert_eq!(b.entries()[0].label.as_deref(), Some("Notes"));
        assert_eq!(b.entries()[1].label, None);
    }

    #[test]
    fn load_missing_returns_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = Allowlist::load(&dir.path().join("nonexistent.json")).unwrap();
        assert!(a.entries().is_empty());
    }
}
