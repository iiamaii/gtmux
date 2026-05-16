//! NDJSON audit log per ADR-0023 D9. Every `POST /api/file-path/open`
//! call appends one line — both successful opens and denied attempts.
//!
//! File pattern: `<audit_dir>/file-open-YYYYMMDD.log` (UTC date). New
//! day → new file, old files left in place. No automatic rotation /
//! retention in MVP (deferred to P1+).
//!
//! Fields:
//! - `ts`            : Unix epoch seconds (u64)
//! - `path`          : the user-supplied path (post-canonicalize)
//! - `allowed_via`   : `"allowlist" | "one_time" | "denied"`
//! - `reason`        : on `denied`, the deny `reason` string
//! - `cookie_prefix` : first 8 chars of session cookie (correlation
//!                     without exposing the full secret)

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// File-system NDJSON audit log. Cheap to clone — the `Mutex` is internal.
#[derive(Debug)]
pub struct AuditLog {
    dir: PathBuf,
    // Serialise the write so concurrent handler invocations don't
    // interleave bytes within a single line. Lock-and-write is fast
    // enough; file_open volume is low (user clicks, not stream).
    write_lock: Mutex<()>,
}

#[derive(Debug, Serialize)]
struct AuditRecord<'a> {
    ts: u64,
    path: &'a str,
    allowed_via: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
    #[serde(skip_serializing_if = "str::is_empty")]
    cookie_prefix: &'a str,
}

impl AuditLog {
    /// Create an `AuditLog` rooted at `dir`. The directory is created
    /// on demand on the first write — no I/O happens in this
    /// constructor, so it's safe to call before the user's HOME exists
    /// (containers, ephemeral CI).
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            write_lock: Mutex::new(()),
        }
    }

    /// Record a successful open against the allowlist.
    pub fn record_allowed(&self, path: &str, cookie_prefix: &str) {
        self.record(path, "allowlist", None, cookie_prefix);
    }

    /// Record a successful one-time open (user_confirmed=true, no
    /// allowlist match).
    pub fn record_one_time(&self, path: &str, cookie_prefix: &str) {
        self.record(path, "one_time", None, cookie_prefix);
    }

    /// Record a denied attempt. `reason` is one of the wire error
    /// strings (`"user_confirmation_required"`, `"path_not_absolute"`, …).
    pub fn record_denied(&self, path: &str, reason: &str, cookie_prefix: &str) {
        self.record(path, "denied", Some(reason), cookie_prefix);
    }

    fn record(
        &self,
        path: &str,
        allowed_via: &'static str,
        reason: Option<&str>,
        cookie_prefix: &str,
    ) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let record = AuditRecord {
            ts,
            path,
            allowed_via,
            reason,
            cookie_prefix,
        };
        let line = match serde_json::to_string(&record) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "audit: failed to serialise record");
                return;
            }
        };
        let log_path = self.dir.join(daily_filename(ts));
        if let Err(e) = self.append_line(&log_path, &line) {
            // An audit-log failure shouldn't break the user's open;
            // surface it at warn so ops can notice.
            tracing::warn!(
                error = %e,
                path = %log_path.display(),
                "audit: failed to append"
            );
        }
    }

    fn append_line(&self, log_path: &Path, line: &str) -> std::io::Result<()> {
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(log_path)?;
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
        Ok(())
    }
}

/// `file-open-YYYYMMDD.log` filename for the given epoch second. Uses
/// UTC to avoid daylight-saving boundary discontinuities. Hand-rolled
/// to avoid pulling `chrono` / `time` just for this.
fn daily_filename(ts: u64) -> String {
    let (y, m, d) = epoch_seconds_to_ymd(ts);
    format!("file-open-{y:04}{m:02}{d:02}.log")
}

/// Convert Unix epoch seconds (UTC) → `(year, month, day)`.
///
/// Algorithm: civil-from-days from Howard Hinnant's date paper
/// (https://howardhinnant.github.io/date_algorithms.html#civil_from_days).
/// Public-domain reference algorithm — embedded directly so we don't
/// pull a date crate.
fn epoch_seconds_to_ymd(ts: u64) -> (i32, u32, u32) {
    let days = (ts / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146_096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_to_ymd_matches_known_dates() {
        // 2026-05-16 00:00:00 UTC = 1779638400
        assert_eq!(epoch_seconds_to_ymd(1_778_889_600), (2026, 5, 16));
        // 1970-01-01 00:00:00 UTC = 0
        assert_eq!(epoch_seconds_to_ymd(0), (1970, 1, 1));
        // 2000-02-29 00:00:00 UTC = 951782400 (leap year)
        assert_eq!(epoch_seconds_to_ymd(951_782_400), (2000, 2, 29));
    }

    #[test]
    fn daily_filename_format() {
        assert_eq!(daily_filename(1_778_889_600), "file-open-20260516.log");
        assert_eq!(daily_filename(0), "file-open-19700101.log");
    }

    #[test]
    fn record_allowed_writes_ndjson_line() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = AuditLog::new(dir.path().to_path_buf());
        log.record_allowed("/tmp/notes/spec.md", "B5Rc3qtn");
        // Locate today's file.
        let files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("file-open-")
            })
            .collect();
        assert_eq!(files.len(), 1);
        let body = std::fs::read_to_string(files[0].path()).unwrap();
        assert!(body.ends_with('\n'));
        let line = body.trim();
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["path"], "/tmp/notes/spec.md");
        assert_eq!(v["allowed_via"], "allowlist");
        assert_eq!(v["cookie_prefix"], "B5Rc3qtn");
        assert!(v["ts"].as_u64().unwrap() > 0);
        assert!(v.get("reason").is_none(), "no reason on allowed");
    }

    #[test]
    fn record_denied_writes_reason() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = AuditLog::new(dir.path().to_path_buf());
        log.record_denied("/etc/shadow", "user_confirmation_required", "");
        let entry = std::fs::read_dir(dir.path()).unwrap().next().unwrap().unwrap();
        let line = std::fs::read_to_string(entry.path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["allowed_via"], "denied");
        assert_eq!(v["reason"], "user_confirmation_required");
        assert!(v.get("cookie_prefix").is_none(), "empty cookie_prefix elided");
    }

    #[test]
    fn multiple_records_append_lines() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = AuditLog::new(dir.path().to_path_buf());
        log.record_allowed("/a", "");
        log.record_one_time("/b", "");
        log.record_denied("/c", "nul_byte", "");
        let entry = std::fs::read_dir(dir.path()).unwrap().next().unwrap().unwrap();
        let body = std::fs::read_to_string(entry.path()).unwrap();
        let lines: Vec<_> = body.lines().collect();
        assert_eq!(lines.len(), 3);
        let allowed_vias: Vec<_> = lines
            .iter()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                v["allowed_via"].as_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(allowed_vias, vec!["allowlist", "one_time", "denied"]);
    }
}
