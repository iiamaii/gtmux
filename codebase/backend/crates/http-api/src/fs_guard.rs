//! fs_guard — Server Workspace(A) sandbox + Store/config/state denylist.
//!
//! Source-of-truth: ADR-0045 (D3/D4/D6 — storage 3분리, A 상한 + M2 denylist)
//! and ADR-0046 (D1/D3 — effective workspace chain, fs picker re-root).
//!
//! Every filesystem access the user can steer — the file picker (`fs_list`),
//! `mkdir`/`rmdir`, a session's `workspace_root`(B), file-path open, and the
//! terminal cwd — must pass a *single* guard:
//!
//!   canonicalize(path)  AND  path ⊂ Server Workspace(A)  AND  path ∉ denylist
//!
//! where the denylist is `{ Store dir, gtmux config dir, gtmux state dir }`
//! (ADR-0045 D6 / M2). The guard applies even with `show_hidden = true`: the
//! danger is the user pointing a terminal / mkdir / rmdir at gtmux's own
//! control-plane storage (session records / locks), which would be a
//! self-corruption / data-loss vector.

// The error-enum variants are self-describing via their `#[error(...)]`
// strings (and the type-level docs above); suppress per-variant `missing_docs`
// to match the sibling `workspace.rs` / `schema.rs` convention.
#![allow(missing_docs)]

use std::path::{Path, PathBuf};

use thiserror::Error;

/// `${XDG_CONFIG_HOME:-~/.config}/gtmux/` — the gtmux config dir (TOML, the
/// file-open allowlist, …). Used only for the denylist. `None` when neither
/// `$XDG_CONFIG_HOME` nor `$HOME` resolves (the dir is then simply not added —
/// a non-existent dir contains nothing, so the guard stays sound).
pub fn gtmux_config_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(s) if !s.is_empty() => PathBuf::from(s),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("gtmux"))
}

/// `${XDG_STATE_HOME:-~/.local/state}/gtmux/` — the gtmux state dir (pidfile,
/// token, password hash, audit logs). Used only for the denylist.
pub fn gtmux_state_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(s) if !s.is_empty() => PathBuf::from(s),
        _ => PathBuf::from(std::env::var_os("HOME")?)
            .join(".local")
            .join("state"),
    };
    Some(base.join("gtmux"))
}

/// Build the M2 denylist (ADR-0045 D6) from the Store directory. Entries are
/// canonicalized when they exist (so the canonical-candidate `starts_with`
/// check in [`is_path_allowed`] lines up); a not-yet-created config/state dir
/// is kept lexically — nothing can be *inside* a path that does not exist, so
/// either form denies correctly.
pub fn build_denylist(store_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(3);
    for dir in [
        Some(store_dir.to_path_buf()),
        gtmux_config_dir(),
        gtmux_state_dir(),
    ]
    .into_iter()
    .flatten()
    {
        out.push(dir.canonicalize().unwrap_or(dir));
    }
    out
}

/// Resolve the Server Workspace(A) root (ADR-0045 D3 / ADR-0046 D7):
/// `cli (--workspace) > config.server_workspace > $HOME`. The resolved path is
/// canonicalized and must already exist as a directory — A is a pre-existing
/// fs region, never created by gtmux (boot fails otherwise: there is no
/// sandbox to enforce).
pub fn resolve_server_workspace(
    cli: Option<PathBuf>,
    config: Option<PathBuf>,
) -> Result<PathBuf, ServerWorkspaceError> {
    let raw = match (cli, config) {
        (Some(p), _) => p,
        (None, Some(p)) => p,
        (None, None) => {
            let home = std::env::var_os("HOME")
                .ok_or_else(|| ServerWorkspaceError::Unresolved("$HOME not set".into()))?;
            PathBuf::from(home)
        }
    };
    if !raw.is_absolute() {
        return Err(ServerWorkspaceError::NotAbsolute(raw));
    }
    let canonical = raw
        .canonicalize()
        .map_err(|e| ServerWorkspaceError::Missing(raw.clone(), e.to_string()))?;
    if !canonical.is_dir() {
        return Err(ServerWorkspaceError::NotADirectory(canonical));
    }
    Ok(canonical)
}

/// Boot-time Server Workspace(A) resolution failure. Each maps to a fatal
/// `gtmux start` error — A is the sandbox boundary, so an unusable A means the
/// server cannot safely enforce file access at all.
#[derive(Debug, Error)]
pub enum ServerWorkspaceError {
    #[error("server workspace path is not absolute: {0}")]
    NotAbsolute(PathBuf),
    #[error("server workspace does not exist: {0} ({1})")]
    Missing(PathBuf, String),
    #[error("server workspace is not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error("server workspace could not be resolved: {0}")]
    Unresolved(String),
}

/// Whether `canonical` (an already-canonicalized path) is inside the Server
/// Workspace(A) *and* outside every denylist entry (ADR-0045 D6). Both
/// `server_workspace` and `denylist` entries are expected canonical.
pub fn is_path_allowed(canonical: &Path, server_workspace: &Path, denylist: &[PathBuf]) -> bool {
    canonical.starts_with(server_workspace)
        && !denylist.iter().any(|deny| canonical.starts_with(deny))
}

/// Reason a candidate `workspace_root`(B) / picker path was rejected. The
/// `code` is surfaced to the FE so it can show a precise hint; the create /
/// change-workspace handlers map every variant to `400 invalid_workspace`
/// except [`NotFound`](Self::NotFound) which is still 400 (the dir must exist
/// or be created via picker mkdir first — ADR-0046 D1(e)).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkspaceRootError {
    #[error("workspace_root must be an absolute path")]
    NotAbsolute,
    #[error("workspace_root is outside the server workspace")]
    OutsideServerWorkspace,
    #[error("workspace_root resolves into a gtmux internal directory")]
    Denied,
    #[error("workspace_root does not exist")]
    NotFound,
    #[error("workspace_root is not a directory")]
    NotADirectory,
}

impl WorkspaceRootError {
    /// Stable machine-readable reason for the `400 invalid_workspace` body.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::NotAbsolute => "not_absolute",
            Self::OutsideServerWorkspace => "outside_server_workspace",
            Self::Denied => "denied",
            Self::NotFound => "not_found",
            Self::NotADirectory => "not_a_directory",
        }
    }
}

/// Validate a candidate `workspace_root`(B) against ADR-0045 D4 / D6:
/// (a) absolute, (b) canonicalize → inside A, (c) outside the denylist,
/// (d) a directory, (e) exists. Returns the canonical absolute path on success.
pub fn validate_workspace_root(
    raw: &str,
    server_workspace: &Path,
    denylist: &[PathBuf],
) -> Result<PathBuf, WorkspaceRootError> {
    let candidate = PathBuf::from(raw);
    if !candidate.is_absolute() {
        return Err(WorkspaceRootError::NotAbsolute);
    }
    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(WorkspaceRootError::NotFound);
        }
        // Any other canonicalize error (permission, ELOOP, …) is treated as
        // "outside" — fail closed rather than leak why.
        Err(_) => return Err(WorkspaceRootError::OutsideServerWorkspace),
    };
    if !canonical.starts_with(server_workspace) {
        return Err(WorkspaceRootError::OutsideServerWorkspace);
    }
    if denylist.iter().any(|deny| canonical.starts_with(deny)) {
        return Err(WorkspaceRootError::Denied);
    }
    if !canonical.is_dir() {
        return Err(WorkspaceRootError::NotADirectory);
    }
    Ok(canonical)
}

/// Resolve a session's *effective* workspace (ADR-0046 D1):
/// `record_root ?? config_default ?? $HOME`. The chosen value is only used
/// when it passes the same A-scope + denylist + dir + exists guard; otherwise
/// the chain falls through and, as a safety backstop, the Server Workspace(A)
/// root is returned (which is always a valid directory by boot invariant).
/// This keeps legacy / out-of-A records working without a prompt while making
/// the user-facing default `$HOME` whenever it is within A.
pub fn effective_workspace(
    record_root: Option<&str>,
    config_default: Option<&Path>,
    server_workspace: &Path,
    denylist: &[PathBuf],
) -> PathBuf {
    effective_workspace_with_home(
        record_root,
        config_default,
        std::env::var_os("HOME").as_deref().map(Path::new),
        server_workspace,
        denylist,
    )
}

fn effective_workspace_with_home(
    record_root: Option<&str>,
    config_default: Option<&Path>,
    home: Option<&Path>,
    server_workspace: &Path,
    denylist: &[PathBuf],
) -> PathBuf {
    if let Some(root) = record_root {
        if let Ok(p) = validate_workspace_root(root, server_workspace, denylist) {
            return p;
        }
        tracing::warn!(
            workspace_root = %root,
            "effective_workspace: session workspace_root invalid/out-of-A; falling back"
        );
    }
    if let Some(default) = config_default {
        if let Some(s) = default.to_str() {
            if let Ok(p) = validate_workspace_root(s, server_workspace, denylist) {
                return p;
            }
        }
        tracing::warn!(
            default = %default.display(),
            "effective_workspace: config default_session_workspace invalid/out-of-A; falling back"
        );
    }
    if let Some(home) = home {
        if let Some(s) = home.to_str() {
            if let Ok(p) = validate_workspace_root(s, server_workspace, denylist) {
                return p;
            }
        }
        tracing::warn!(
            home = %home.display(),
            "effective_workspace: $HOME invalid/out-of-A; using A-root"
        );
    }
    server_workspace.to_path_buf()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn allowed_inside_a_and_outside_denylist() {
        let a = TempDir::new().unwrap();
        let a_root = a.path().canonicalize().unwrap();
        let store = a_root.join("store");
        std::fs::create_dir_all(&store).unwrap();
        let project = a_root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let denylist = vec![store.clone()];

        assert!(is_path_allowed(&project, &a_root, &denylist));
        // Store itself and its children are denied.
        assert!(!is_path_allowed(&store, &a_root, &denylist));
        assert!(!is_path_allowed(
            &store.join("demo.json"),
            &a_root,
            &denylist
        ));
    }

    #[test]
    fn validate_workspace_root_happy_and_rejections() {
        let a = TempDir::new().unwrap();
        let a_root = a.path().canonicalize().unwrap();
        let store = a_root.join("store");
        std::fs::create_dir_all(&store).unwrap();
        let project = a_root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let denylist = vec![store.clone()];

        assert_eq!(
            validate_workspace_root(project.to_str().unwrap(), &a_root, &denylist).unwrap(),
            project
        );
        assert_eq!(
            validate_workspace_root("rel/path", &a_root, &denylist).unwrap_err(),
            WorkspaceRootError::NotAbsolute
        );
        assert_eq!(
            validate_workspace_root(store.to_str().unwrap(), &a_root, &denylist).unwrap_err(),
            WorkspaceRootError::Denied
        );
        assert_eq!(
            validate_workspace_root(
                a_root.join("does-not-exist").to_str().unwrap(),
                &a_root,
                &denylist
            )
            .unwrap_err(),
            WorkspaceRootError::NotFound
        );
        // A file (not a dir) inside A → NotADirectory.
        let file = a_root.join("file.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            validate_workspace_root(file.to_str().unwrap(), &a_root, &denylist).unwrap_err(),
            WorkspaceRootError::NotADirectory
        );
    }

    #[test]
    fn effective_falls_back_through_chain_to_home_then_a_root() {
        let a = TempDir::new().unwrap();
        let a_root = a.path().canonicalize().unwrap();
        let store = a_root.join("store");
        std::fs::create_dir_all(&store).unwrap();
        let denylist = vec![store.clone()];
        let project = a_root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let home = a_root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let outside_home = TempDir::new().unwrap();
        let outside_home_root = outside_home.path().canonicalize().unwrap();

        // record_root wins when valid.
        assert_eq!(
            effective_workspace_with_home(project.to_str(), None, Some(&home), &a_root, &denylist),
            project
        );
        // invalid record_root + no default → $HOME when it is valid inside A.
        assert_eq!(
            effective_workspace_with_home(
                Some("/nonexistent/xyz"),
                None,
                Some(&home),
                &a_root,
                &denylist
            ),
            home
        );
        // out-of-A record_root falls through to config default when valid.
        assert_eq!(
            effective_workspace_with_home(
                Some("/etc"),
                Some(&project),
                Some(&home),
                &a_root,
                &denylist
            ),
            project
        );
        // $HOME outside A is rejected by the same guard, then A-root is the
        // closed fallback.
        assert_eq!(
            effective_workspace_with_home(None, None, Some(&outside_home_root), &a_root, &denylist),
            a_root
        );
    }
}
