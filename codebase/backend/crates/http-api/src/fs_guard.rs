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
//! A(=[`WorkspaceRoots`]) is a *set* of roots: the primary root (`--workspace` >
//! config `server_workspace` > `$HOME`) plus any config `extra_workspace_roots`
//! — separate mount points such as an external SSD (`/mnt/ssd2`) that cannot be
//! expressed as a subdirectory of the primary root. "⊂ A" means "inside **any**
//! root"; the primary root remains the default open / closed fallback.
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

/// Resolve the optional *extra* Server Workspace(A) roots (config
/// `extra_workspace_roots`) — additional mount points the picker may browse,
/// e.g. an external SSD at `/mnt/ssd2` that is not reachable from `$HOME`.
///
/// Unlike the primary root, an unusable extra root is **not** fatal: an
/// external drive can simply be unplugged at boot, and the Instance is still
/// perfectly usable without it. Returns `(resolved, skipped)` where `skipped`
/// carries a human-readable reason per rejected entry so boot can log it.
pub fn resolve_extra_workspace_roots(raw: &[PathBuf]) -> (Vec<PathBuf>, Vec<(PathBuf, String)>) {
    let mut resolved = Vec::new();
    let mut skipped = Vec::new();
    for entry in raw {
        if !entry.is_absolute() {
            skipped.push((entry.clone(), "not an absolute path".to_string()));
            continue;
        }
        match entry.canonicalize() {
            Ok(canonical) if canonical.is_dir() => resolved.push(canonical),
            Ok(canonical) => skipped.push((
                entry.clone(),
                format!("not a directory: {}", canonical.display()),
            )),
            Err(e) => skipped.push((entry.clone(), e.to_string())),
        }
    }
    (resolved, skipped)
}

/// The Server Workspace(A) sandbox — a primary root plus zero or more extra
/// roots (ADR-0045 D3, extended for multi-mount setups).
///
/// * **primary** — `--workspace` > config `server_workspace` > `$HOME`. It is
///   the picker's default open, the `dir=""` listing target, and the closed
///   fallback of [`effective_workspace`], so it always exists.
/// * **extra** — config `extra_workspace_roots`, each an already-canonical
///   existing directory (see [`resolve_extra_workspace_roots`]). Roots nested
///   inside another root are folded away at construction, so the set is
///   disjoint and `iter()` yields one entry per browsable top level.
///
/// A path is in-sandbox when it is inside *any* root; the denylist still
/// applies on top (ADR-0045 D6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRoots {
    primary: PathBuf,
    extra: Vec<PathBuf>,
}

impl WorkspaceRoots {
    /// Single-root A — the classic shape, and what tests want.
    pub fn single(primary: PathBuf) -> Self {
        Self {
            primary,
            extra: Vec::new(),
        }
    }

    /// A with extra roots. Non-absolute entries, duplicates, and roots already
    /// covered by the primary or by another extra root are dropped (a broader
    /// root supersedes a nested one) so every retained root is a distinct top
    /// level for the picker.
    pub fn new(primary: PathBuf, extra: Vec<PathBuf>) -> Self {
        let mut kept: Vec<PathBuf> = Vec::new();
        for cand in extra {
            if !cand.is_absolute() || cand.starts_with(&primary) {
                continue;
            }
            if kept.iter().any(|k| cand.starts_with(k)) {
                continue;
            }
            kept.retain(|k| !k.starts_with(&cand));
            kept.push(cand);
        }
        Self {
            primary,
            extra: kept,
        }
    }

    /// The primary root — default open, `dir=""` target, closed fallback.
    pub fn primary(&self) -> &Path {
        &self.primary
    }

    /// Extra roots only, in configuration order.
    pub fn extra(&self) -> &[PathBuf] {
        &self.extra
    }

    /// Every root, primary first.
    pub fn iter(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.primary.as_path()).chain(self.extra.iter().map(PathBuf::as_path))
    }

    /// Whether `path` *is* one of the roots (a picker top level — no parent).
    pub fn is_root(&self, path: &Path) -> bool {
        self.iter().any(|root| root == path)
    }

    /// Whether `canonical` lies inside any root. Denylist is checked separately
    /// by [`is_path_allowed`].
    pub fn contains(&self, canonical: &Path) -> bool {
        self.iter().any(|root| canonical.starts_with(root))
    }

    /// Roots as display strings — the `roots` field of the `fs/list` response
    /// the picker renders its root shortcuts from.
    pub fn to_display_strings(&self) -> Vec<String> {
        self.iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect()
    }
}

impl From<PathBuf> for WorkspaceRoots {
    fn from(primary: PathBuf) -> Self {
        Self::single(primary)
    }
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
/// Workspace(A) — i.e. inside any of its roots — *and* outside every denylist
/// entry (ADR-0045 D6). Both the roots and the `denylist` entries are expected
/// canonical.
pub fn is_path_allowed(
    canonical: &Path,
    server_workspace: &WorkspaceRoots,
    denylist: &[PathBuf],
) -> bool {
    server_workspace.contains(canonical)
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
    server_workspace: &WorkspaceRoots,
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
    if !server_workspace.contains(&canonical) {
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
    server_workspace: &WorkspaceRoots,
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
    server_workspace: &WorkspaceRoots,
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
    server_workspace.primary().to_path_buf()
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
        let roots = WorkspaceRoots::single(a_root.clone());

        assert!(is_path_allowed(&project, &roots, &denylist));
        // Store itself and its children are denied.
        assert!(!is_path_allowed(&store, &roots, &denylist));
        assert!(!is_path_allowed(&store.join("demo.json"), &roots, &denylist));
    }

    #[test]
    fn extra_root_is_in_sandbox_and_still_denylisted() {
        let a = TempDir::new().unwrap();
        let a_root = a.path().canonicalize().unwrap();
        let drive = TempDir::new().unwrap();
        let drive_root = drive.path().canonicalize().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_root = outside.path().canonicalize().unwrap();
        let deny_on_drive = drive_root.join("store");
        std::fs::create_dir_all(&deny_on_drive).unwrap();
        let project = drive_root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let denylist = vec![deny_on_drive.clone()];

        let roots = WorkspaceRoots::new(a_root.clone(), vec![drive_root.clone()]);
        // Both roots are tops; a path under the extra root is in-sandbox.
        assert!(roots.is_root(&a_root));
        assert!(roots.is_root(&drive_root));
        assert!(is_path_allowed(&project, &roots, &denylist));
        assert!(is_path_allowed(&a_root, &roots, &denylist));
        // The denylist still wins inside an extra root, and unlisted roots stay out.
        assert!(!is_path_allowed(&deny_on_drive, &roots, &denylist));
        assert!(!is_path_allowed(&outside_root, &roots, &denylist));

        assert_eq!(
            validate_workspace_root(project.to_str().unwrap(), &roots, &denylist).unwrap(),
            project
        );
        assert_eq!(
            validate_workspace_root(outside_root.to_str().unwrap(), &roots, &denylist).unwrap_err(),
            WorkspaceRootError::OutsideServerWorkspace
        );
    }

    #[test]
    fn nested_and_relative_extra_roots_are_folded_away() {
        let primary = PathBuf::from("/home/u");
        let roots = WorkspaceRoots::new(
            primary.clone(),
            vec![
                // Inside the primary root → redundant.
                PathBuf::from("/home/u/ws"),
                PathBuf::from("relative/path"),
                PathBuf::from("/mnt/ssd2/projects"),
                // Broader than the previous entry → supersedes it.
                PathBuf::from("/mnt/ssd2"),
                // Now nested in the broader entry → dropped.
                PathBuf::from("/mnt/ssd2/other"),
            ],
        );
        assert_eq!(roots.extra(), [PathBuf::from("/mnt/ssd2")]);
        assert_eq!(
            roots.to_display_strings(),
            vec!["/home/u".to_string(), "/mnt/ssd2".to_string()]
        );
        assert_eq!(roots.primary(), primary);
    }

    #[test]
    fn resolve_extra_roots_skips_unusable_entries() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let file = root.join("file.txt");
        std::fs::write(&file, b"x").unwrap();

        let missing = root.join("missing");
        let (resolved, skipped) = resolve_extra_workspace_roots(&[
            root.clone(),
            PathBuf::from("rel"),
            missing.clone(),
            file.clone(),
        ]);
        assert_eq!(resolved, vec![root]);
        let skipped: Vec<PathBuf> = skipped.into_iter().map(|(p, _)| p).collect();
        assert_eq!(skipped, vec![PathBuf::from("rel"), missing, file]);
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
        let roots = WorkspaceRoots::single(a_root.clone());

        assert_eq!(
            validate_workspace_root(project.to_str().unwrap(), &roots, &denylist).unwrap(),
            project
        );
        assert_eq!(
            validate_workspace_root("rel/path", &roots, &denylist).unwrap_err(),
            WorkspaceRootError::NotAbsolute
        );
        assert_eq!(
            validate_workspace_root(store.to_str().unwrap(), &roots, &denylist).unwrap_err(),
            WorkspaceRootError::Denied
        );
        assert_eq!(
            validate_workspace_root(
                a_root.join("does-not-exist").to_str().unwrap(),
                &roots,
                &denylist
            )
            .unwrap_err(),
            WorkspaceRootError::NotFound
        );
        // A file (not a dir) inside A → NotADirectory.
        let file = a_root.join("file.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            validate_workspace_root(file.to_str().unwrap(), &roots, &denylist).unwrap_err(),
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
        let roots = WorkspaceRoots::single(a_root.clone());

        // record_root wins when valid.
        assert_eq!(
            effective_workspace_with_home(project.to_str(), None, Some(&home), &roots, &denylist),
            project
        );
        // invalid record_root + no default → $HOME when it is valid inside A.
        assert_eq!(
            effective_workspace_with_home(
                Some("/nonexistent/xyz"),
                None,
                Some(&home),
                &roots,
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
                &roots,
                &denylist
            ),
            project
        );
        // $HOME outside A is rejected by the same guard, then the *primary*
        // A-root is the closed fallback.
        assert_eq!(
            effective_workspace_with_home(None, None, Some(&outside_home_root), &roots, &denylist),
            a_root
        );
        // A record_root on an extra root is honoured as-is (no fallback).
        let drive = TempDir::new().unwrap();
        let drive_root = drive.path().canonicalize().unwrap();
        let multi = WorkspaceRoots::new(a_root.clone(), vec![drive_root.clone()]);
        assert_eq!(
            effective_workspace_with_home(
                drive_root.to_str(),
                None,
                Some(&home),
                &multi,
                &denylist
            ),
            drive_root
        );
    }
}
