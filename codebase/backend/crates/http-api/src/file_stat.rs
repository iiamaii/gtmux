//! ADR-0034 / 0060 — `GET /api/file-stat?path=<percent-encoded>`.
//!
//! Returns `{ path, kind, size_bytes, lines, branch }` for files that pass
//! the [`crate::file_open`] allowlist (ADR-0023). The shape is wire-frozen
//! by ADR-0034 D1; the handler is *strictly* a stat query — no caching,
//! no notification, no path mutation. Workspace `state.file_open.allowlist`
//! is the single source of allow/deny truth, so file-stat sits under the
//! same gate as file-open.
//!
//! Implementation notes:
//!   * `lines` = newline byte count, capped at [`LINES_SCAN_MAX_BYTES`]
//!     (ADR-0034 D3). Anything larger reports `lines: null` so a 100 MiB
//!     log doesn't pin a server thread on a scan.
//!   * `branch` reads `.git/HEAD` directly (std-only — ADR-0034 D4 wants
//!     branch shorthand only, so we skip `git2`/`gix` dependencies). The
//!     three shapes accepted are `ref: refs/heads/<name>`, `ref: refs/<...>`,
//!     and a bare 40-char sha (detached HEAD → `"detached: <sha[..7]>"`).
//!   * `worktree` (`.git` as a file pointing at a real gitdir) is *not*
//!     supported in v1 — returns `branch: null`. Follow-up if FilePath
//!     usage actually surfaces worktrees.

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::file_open::AllowlistMatch;
use crate::AppState;

/// ADR-0034 D3 — files larger than this report `lines: null` so the byte
/// scan stays bounded. 64 MiB matches the cap the ADR carved out.
const LINES_SCAN_MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct FileStatQuery {
    pub path: String,
}

#[derive(Debug, Serialize)]
struct FileStatResponse<'a> {
    path: &'a str,
    /// `"file"` or `"directory"` — frozen by ADR-0034 D1.
    kind: &'static str,
    size_bytes: u64,
    /// `None` when the path is a directory or the file exceeds
    /// [`LINES_SCAN_MAX_BYTES`].
    lines: Option<u64>,
    /// `None` when no `.git/HEAD` is reachable from the path's containing
    /// directory.
    branch: Option<String>,
}

/// `GET /api/file-stat?path=<percent-encoded>` — ADR-0034 D1.
pub async fn file_stat_handler(
    State(state): State<AppState>,
    Query(q): Query<FileStatQuery>,
) -> Response {
    // 1. validate + canonicalize via the same path validator the file-open
    //    handler uses. This rejects nul bytes, non-absolute paths, and
    //    missing files at once.
    let canonical = match validate_path(&q.path) {
        Ok(p) => p,
        Err("path_not_exists") => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "path_not_found" })),
            )
                .into_response();
        }
        Err(reason) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_path", "reason": reason })),
            )
                .into_response();
        }
    };

    // 2. ADR-0023 allowlist gate (ADR-0034 D2). Empty allowlist denies all.
    {
        let alist = state.file_open.allowlist.read().await;
        if !matches!(alist.check(&canonical), AllowlistMatch::Allowed(_)) {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "path_not_allowed" })),
            )
                .into_response();
        }
    }

    // 3. stat. `validate_path` already canonicalized, so the entry exists
    //    unless the FS shifted under us between resolve and read — surface
    //    that as 404.
    let meta = match std::fs::metadata(&canonical) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "path_not_found" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "stat_failed", "message": e.to_string() })),
            )
                .into_response();
        }
    };

    let kind = if meta.is_dir() { "directory" } else { "file" };
    let size_bytes = meta.len();
    let lines = if meta.is_file() && size_bytes <= LINES_SCAN_MAX_BYTES {
        count_lines(&canonical)
    } else {
        None
    };
    let branch = git_branch_for(&canonical);

    let canonical_str = canonical.to_string_lossy().into_owned();
    Json(FileStatResponse {
        path: &canonical_str,
        kind,
        size_bytes,
        lines,
        branch,
    })
    .into_response()
}

/// Mirror of `file_open::handlers::validate_path` — kept module-local to
/// avoid widening that module's surface. The contract is identical: only
/// existing, absolute paths survive, the result is the canonicalized form.
fn validate_path(path: &str) -> Result<PathBuf, &'static str> {
    if path.is_empty() {
        return Err("empty_path");
    }
    if path.contains('\0') {
        return Err("nul_byte");
    }
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err("path_not_absolute");
    }
    std::fs::canonicalize(p).map_err(|_| "path_not_exists")
}

/// Newline-byte counter with the [`LINES_SCAN_MAX_BYTES`] guard already
/// applied by the caller. Returns `None` on any read error so the response
/// stays a 200 (the *file existed*, the byte scan is best-effort).
fn count_lines(path: &Path) -> Option<u64> {
    let f = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::with_capacity(64 * 1024, f);
    let mut buf = [0u8; 64 * 1024];
    let mut count: u64 = 0;
    loop {
        let n = reader.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        count += buf[..n].iter().filter(|&&b| b == b'\n').count() as u64;
    }
    Some(count)
}

/// `.git/HEAD` walker — finds the nearest ancestor that *is* a git
/// directory and returns the branch shorthand. Returns `None` when no
/// `.git` directory is reachable upward. Worktree (`.git` as a file
/// pointing elsewhere) is not yet supported — `is_dir()` short-circuits.
fn git_branch_for(target: &Path) -> Option<String> {
    let start = if target.is_dir() {
        target.to_path_buf()
    } else {
        target.parent()?.to_path_buf()
    };
    let git_dir = find_git_dir(&start)?;
    read_git_branch(&git_dir)
}

fn find_git_dir(start: &Path) -> Option<PathBuf> {
    let mut cur: Option<&Path> = Some(start);
    while let Some(dir) = cur {
        let candidate = dir.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        cur = dir.parent();
    }
    None
}

fn read_git_branch(git_dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let trimmed = head.trim();
    if let Some(rest) = trimmed.strip_prefix("ref: refs/heads/") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("ref: refs/") {
        return Some(rest.to_string());
    }
    if trimmed.len() >= 7 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(format!("detached: {}", &trimmed[..7]));
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn validate_path_rejects_relative() {
        assert_eq!(validate_path("relative.txt"), Err("path_not_absolute"));
    }

    #[test]
    fn validate_path_rejects_empty() {
        assert_eq!(validate_path(""), Err("empty_path"));
    }

    #[test]
    fn count_lines_counts_newline_bytes() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, b"line1\nline2\nline3\n").unwrap();
        assert_eq!(count_lines(&p), Some(3));
    }

    #[test]
    fn count_lines_handles_binary_bytes() {
        // newline byte (0x0a) counted regardless of surrounding bytes —
        // ADR-0034 D3: this is "newline byte count", not "logical lines".
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("bin");
        std::fs::write(&p, &[0xff, 0x0a, 0x00, 0x0a, 0xfe]).unwrap();
        assert_eq!(count_lines(&p), Some(2));
    }

    #[test]
    fn git_branch_returns_ref_shorthand() {
        let dir = TempDir::new().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir(&git_dir).unwrap();
        std::fs::write(git_dir.join("HEAD"), b"ref: refs/heads/main\n").unwrap();

        let probe = dir.path().join("src.rs");
        std::fs::File::create(&probe).unwrap();

        assert_eq!(git_branch_for(&probe), Some("main".to_string()));
    }

    #[test]
    fn git_branch_returns_detached_sha7() {
        let dir = TempDir::new().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir(&git_dir).unwrap();
        std::fs::write(
            git_dir.join("HEAD"),
            b"abc1234deadbeefcafe0123456789abcdef012345\n",
        )
        .unwrap();
        let probe = dir.path().join("src.rs");
        std::fs::File::create(&probe).unwrap();
        assert_eq!(
            git_branch_for(&probe),
            Some("detached: abc1234".to_string()),
        );
    }

    #[test]
    fn git_branch_walks_up_to_ancestor() {
        let dir = TempDir::new().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir(&git_dir).unwrap();
        std::fs::write(git_dir.join("HEAD"), b"ref: refs/heads/feature/x\n").unwrap();

        // Probe lives several levels deep.
        let nested = dir.path().join("crates").join("foo").join("src");
        std::fs::create_dir_all(&nested).unwrap();
        let probe = nested.join("lib.rs");
        std::fs::File::create(&probe).unwrap();

        assert_eq!(git_branch_for(&probe), Some("feature/x".to_string()),);
    }

    #[test]
    fn git_branch_returns_none_outside_repo() {
        let dir = TempDir::new().unwrap();
        // No `.git` anywhere.
        let probe = dir.path().join("naked.txt");
        std::fs::File::create(&probe).unwrap();
        assert_eq!(git_branch_for(&probe), None);
    }

    #[test]
    fn count_lines_handles_large_buffer_boundary() {
        // Cross the 64 KiB read buffer to confirm streaming counter works.
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("big.txt");
        let mut f = std::fs::File::create(&p).unwrap();
        // 200_000 newline bytes split across many buffer fills.
        let chunk = vec![b'\n'; 8192];
        for _ in 0..25 {
            f.write_all(&chunk).unwrap();
        }
        drop(f);
        assert_eq!(count_lines(&p), Some(8192 * 25));
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Integration — `GET /api/file-stat` via the live router.
    // ─────────────────────────────────────────────────────────────────────

    use crate::file_open::FileOpenContext;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request as HttpRequest};
    use gtmux_auth::{issue_token, TokenString};
    use gtmux_config::{Config, RuntimeConfig, SecurityConfig, ServerConfig};
    use serde_json::Value;
    use tower::ServiceExt;

    const TEST_HOST: &str = "127.0.0.1:9001";

    fn bearer(token: &TokenString) -> String {
        format!("Bearer {}", token.0)
    }

    fn integration_state() -> (crate::AppState, TokenString, TempDir) {
        let token = issue_token().expect("token");
        let cfg = Config {
            schema_version: 1,
            server: ServerConfig {
                session: "test".to_string(),
                port: 9001,
                bind: "127.0.0.1".to_string(),
            },
            runtime: RuntimeConfig::default(),
            security: SecurityConfig {
                cors_origins: vec!["http://localhost:9001".to_string()],
                host_allowlist: vec![TEST_HOST.to_string()],
            },
            cloud: None,
            frontend_dist: None,
            workspace_path: None,
            auth: gtmux_config::AuthConfig::default(),
        };
        let tmp = TempDir::new().unwrap();
        let mut state = crate::AppState::new(cfg, token.clone());
        state.file_open =
            FileOpenContext::for_tests(tmp.path().join("allowlist.json"), tmp.path().join("audit"));
        (state, token, tmp)
    }

    async fn add_allowlist(
        state: &crate::AppState,
        ext: &str,
        prefix: &str,
        label: Option<String>,
    ) {
        let mut alist = state.file_open.allowlist.write().await;
        alist.add(ext, prefix, label).expect("allowlist add");
    }

    fn fs_request(token: &TokenString, path: &str) -> HttpRequest<Body> {
        let q = urlencoding_minimal(path);
        HttpRequest::builder()
            .uri(format!("/api/file-stat?path={q}"))
            .header(header::HOST, TEST_HOST)
            .header(header::AUTHORIZATION, bearer(token))
            .body(Body::empty())
            .unwrap()
    }

    /// Sufficient query-string escape for path bytes that show up in tests.
    /// The router parses `path` via `serde_urlencoded`, so anything that's a
    /// URL-reserved char (`%`, `&`, `=`, `+`, `#`, ` `) gets percent-encoded;
    /// macOS `/private/tmp/...` is otherwise plain ASCII.
    fn urlencoding_minimal(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                    out.push(b as char);
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }

    /// Gate 0034-1 — happy file inside the allowlist. The lines count
    /// matches the literal newline bytes; the branch is whatever the
    /// fixture's `.git/HEAD` says.
    #[tokio::test]
    async fn file_stat_returns_meta_for_allowed_file() {
        let (state, token, tmp) = integration_state();
        // Layout: tmp/repo/{.git/HEAD, src/lib.rs}
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git").join("HEAD"), b"ref: refs/heads/main\n").unwrap();
        std::fs::create_dir(repo.join("src")).unwrap();
        let probe = repo.join("src").join("lib.rs");
        std::fs::write(&probe, b"line1\nline2\nline3\n").unwrap();

        let canonical_repo = std::fs::canonicalize(&repo).unwrap();
        let allow_prefix = format!("{}/", canonical_repo.display());
        add_allowlist(&state, "rs", &allow_prefix, Some("Rust".into())).await;

        let app = crate::router_with_state(state);
        let canonical_probe = std::fs::canonicalize(&probe).unwrap();
        let resp = app
            .oneshot(fs_request(&token, canonical_probe.to_str().unwrap()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 8192).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["kind"], "file");
        assert_eq!(v["size_bytes"], 18);
        assert_eq!(v["lines"], 3);
        assert_eq!(v["branch"], "main");
    }

    /// Gate 0034-2 — path *outside* every allowlist entry → 403. No body
    /// leaks file existence either way (uniform `path_not_allowed`).
    #[tokio::test]
    async fn file_stat_403_when_path_not_in_allowlist() {
        let (state, token, tmp) = integration_state();
        // Create a file but no allowlist entry pointing at it.
        let probe = tmp.path().join("naked.rs");
        std::fs::write(&probe, b"// nothing").unwrap();
        let app = crate::router_with_state(state);
        let canonical = std::fs::canonicalize(&probe).unwrap();
        let resp = app
            .oneshot(fs_request(&token, canonical.to_str().unwrap()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let v: Value =
            serde_json::from_slice(&to_bytes(resp.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(v["error"], "path_not_allowed");
    }

    /// Gate 0034-3 — `path_not_found`. `canonicalize` fails before the
    /// allowlist check, so the response uses the more-specific 404 code.
    #[tokio::test]
    async fn file_stat_404_when_path_missing() {
        let (_state, token, tmp) = integration_state();
        let app = crate::router_with_state(_state);
        let missing = tmp.path().join("does-not-exist.rs");
        let resp = app
            .oneshot(fs_request(&token, missing.to_str().unwrap()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let v: Value =
            serde_json::from_slice(&to_bytes(resp.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(v["error"], "path_not_found");
    }

    /// Gate 0034-4 — file outside any git working tree → `branch: null`.
    #[tokio::test]
    async fn file_stat_branch_null_when_not_in_repo() {
        let (state, token, tmp) = integration_state();
        // Plain tempdir without .git.
        let probe = tmp.path().join("notes.md");
        std::fs::write(&probe, b"# hi\n").unwrap();
        let canonical_tmp = std::fs::canonicalize(tmp.path()).unwrap();
        let allow_prefix = format!("{}/", canonical_tmp.display());
        add_allowlist(&state, "md", &allow_prefix, None).await;
        let app = crate::router_with_state(state);
        let canonical = std::fs::canonicalize(&probe).unwrap();
        let resp = app
            .oneshot(fs_request(&token, canonical.to_str().unwrap()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v: Value =
            serde_json::from_slice(&to_bytes(resp.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(v["lines"], 1);
        assert_eq!(v["branch"], Value::Null);
    }

    /// Gate 0034-5 — without bearer auth the `/api/*` middleware short-
    /// circuits the request with 401. No body leak.
    #[tokio::test]
    async fn file_stat_401_without_auth() {
        let (state, _token, tmp) = integration_state();
        let probe = tmp.path().join("x.rs");
        std::fs::write(&probe, b"hello").unwrap();
        let canonical = std::fs::canonicalize(&probe).unwrap();
        let q = urlencoding_minimal(canonical.to_str().unwrap());
        let app = crate::router_with_state(state);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/api/file-stat?path={q}"))
                    .header(header::HOST, TEST_HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // NB: ADR-0034 D1 advertises a `kind: "directory"` response value,
    // but the ADR-0023 allowlist is `(ext, prefix)`-shaped and rejects
    // any path whose suffix doesn't carry a `.{ext}` segment. The current
    // behaviour for a directory probe is therefore 403
    // `path_not_allowed`; concrete directory support needs an ADR-0034
    // follow-up that defines either (a) an ext-less allowlist mode or
    // (b) explicit directory carve-out semantics. This gate is parked
    // until that decision lands.
}
