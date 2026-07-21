//! `gtmux skill` — self-bootstrap for the embedded Agent Skill (ADR-0055 D2).
//!
//! The canonical `SKILL.md` (Agent Skills standard: YAML frontmatter
//! `name`+`description` + markdown body) lives at the crate root and is
//! embedded into the `gtmux` binary at compile time via `include_str!`.
//! Embedding — rather than reading `docs/` at runtime — is deliberate: the
//! skill is a *product artefact* shipped with the binary, and the origin
//! push scope excludes `docs/`, so the codebase build must not depend on it
//! (ADR-0055 D2 "정본 위치" amend).
//!
//! This command is fully offline: it never contacts the server and pulls in
//! no HTTP client. Three modes:
//!   * `gtmux skill`              — print the whole embedded SKILL.md.
//!   * `gtmux skill --section <n>`— print only the `## <n>.` section (body
//!     only; the frontmatter block is excluded).
//!   * `gtmux skill install …`    — write the embedded copy into user-level
//!     skill directories so agents pick it up from any project CWD.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};

/// The canonical skill document, embedded at compile time from the crate
/// root. `CARGO_MANIFEST_DIR` resolves to `…/bin/gtmux-cli`, so this reads
/// `…/bin/gtmux-cli/SKILL.md` — no dependency on `docs/`.
pub const SKILL_MD: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/SKILL.md"));

// ────────────────────────────────────────────────────────────────────────────
// CLI surface
// ────────────────────────────────────────────────────────────────────────────

/// `gtmux skill [--section <n>] [install …]`.
#[derive(Debug, Args)]
pub struct SkillArgs {
    /// Print only the section whose heading is `## <n>.` (e.g. `--section 2`).
    /// The YAML frontmatter block is excluded from section output.
    #[arg(long, value_name = "N")]
    pub section: Option<String>,
    #[command(subcommand)]
    pub command: Option<SkillCmd>,
}

#[derive(Debug, Subcommand)]
pub enum SkillCmd {
    /// Write the embedded SKILL.md into user-level skill directories.
    ///
    /// With neither `--claude` nor `--codex`, installs to *both*. Missing
    /// directories are created. An existing file is skipped (with a stderr
    /// notice) unless `--force` overwrites it.
    Install {
        /// Install to `~/.claude/skills/gtmux-cli/SKILL.md`.
        #[arg(long)]
        claude: bool,
        /// Install to `~/.agents/skills/gtmux-cli/SKILL.md`.
        #[arg(long)]
        codex: bool,
        /// Overwrite an existing SKILL.md instead of skipping it.
        #[arg(long)]
        force: bool,
    },
}

/// Dispatch entrypoint called from `main`.
pub fn run(args: SkillArgs) -> ExitCode {
    match args.command {
        Some(SkillCmd::Install {
            claude,
            codex,
            force,
        }) => {
            if args.section.is_some() {
                eprintln!("gtmux skill: --section cannot be combined with `install`.");
                return ExitCode::from(crate::EXIT_FAILURE);
            }
            run_install(claude, codex, force)
        }
        None => match args.section.as_deref() {
            Some(section) => run_section(section),
            None => run_full(),
        },
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Output modes
// ────────────────────────────────────────────────────────────────────────────

fn run_full() -> ExitCode {
    let mut stdout = std::io::stdout();
    // Ignore write errors (broken pipe on `gtmux skill | head` is benign) —
    // the same tolerance the offline `session export` path uses.
    let _ = stdout.write_all(SKILL_MD.as_bytes());
    if !SKILL_MD.ends_with('\n') {
        let _ = stdout.write_all(b"\n");
    }
    ExitCode::SUCCESS
}

fn run_section(section: &str) -> ExitCode {
    match extract_section(SKILL_MD, section) {
        Some(text) => {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(text.as_bytes());
            let _ = stdout.write_all(b"\n");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!(
                "gtmux skill: no section '## {section}.' in SKILL.md \
                 (run `gtmux skill` to see the section headings)."
            );
            ExitCode::from(crate::EXIT_FAILURE)
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Section extraction (pure — unit-tested)
// ────────────────────────────────────────────────────────────────────────────

/// Byte offset at which the markdown body begins — just past the closing
/// `---` of a leading YAML frontmatter block. Returns 0 when there is no
/// frontmatter (or it is unterminated), so the whole string is the body.
fn body_start(md: &str) -> usize {
    let mut lines = md.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return 0;
    };
    if first.trim_end() != "---" {
        return 0;
    }
    let mut offset = first.len();
    for line in lines {
        let closes = line.trim_end() == "---";
        offset += line.len();
        if closes {
            return offset;
        }
    }
    // Unterminated frontmatter — treat the whole document as body.
    0
}

/// Does `heading_line` (already trimmed of its trailing newline, and known to
/// start with `## `) open the requested numeric `section`? Matches `## 2.`
/// for `section = "2"` but not `## 20.` — the section token must be followed
/// by a literal `.`.
fn heading_matches(heading_line: &str, section: &str) -> bool {
    heading_line
        .strip_prefix("## ")
        .and_then(|rest| rest.strip_prefix(section))
        .is_some_and(|after| after.starts_with('.'))
}

/// Extract the `## <section>.` section from the markdown body (frontmatter
/// excluded): everything from the matching heading line up to — but not
/// including — the next `## ` heading (or EOF). Fenced code blocks are
/// skipped so a `## ` inside a ``` fence never counts as a heading. Returns
/// `None` when no heading matches.
fn extract_section(md: &str, section: &str) -> Option<String> {
    let body = &md[body_start(md)..];
    let mut in_fence = false;
    let mut collecting = false;
    let mut out = String::new();
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let is_fence_marker = trimmed.trim_start().starts_with("```");
        // A level-2 heading only counts outside a code fence.
        let is_h2 = !in_fence && trimmed.starts_with("## ");
        if is_fence_marker {
            in_fence = !in_fence;
        }
        if is_h2 {
            if collecting {
                break; // reached the next section
            }
            if heading_matches(trimmed, section) {
                collecting = true;
            }
        }
        if collecting {
            out.push_str(line);
        }
    }
    if !collecting {
        return None;
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Install (pure decision + injectable base dir — unit-tested)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallTarget {
    Claude,
    Codex,
}

impl InstallTarget {
    /// Path segments under `$HOME` for this target's SKILL.md.
    fn segments(self) -> &'static [&'static str] {
        match self {
            // Claude Code user-level skill directory.
            InstallTarget::Claude => &[".claude", "skills", "gtmux-cli", "SKILL.md"],
            // Codex uses the Agent Skills standard `~/.agents/skills`. NOTE:
            // some Codex versions read `~/.codex/skills` instead — that skew
            // is documented in docs/agents/skills/README.md and ADR-0055
            // §미해결 2; we install only the standard `.agents` path here.
            InstallTarget::Codex => &[".agents", "skills", "gtmux-cli", "SKILL.md"],
        }
    }

    fn label(self) -> &'static str {
        match self {
            InstallTarget::Claude => "claude",
            InstallTarget::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallAction {
    Created,
    Updated,
    Skipped,
}

impl InstallAction {
    fn verb(self) -> &'static str {
        match self {
            InstallAction::Created => "created",
            InstallAction::Updated => "updated",
            InstallAction::Skipped => "skipped",
        }
    }
}

/// Pure write-action decision from filesystem facts: `exists` = target file
/// already present, `force` = `--force` given.
fn decide_action(exists: bool, force: bool) -> InstallAction {
    match (exists, force) {
        (false, _) => InstallAction::Created,
        (true, true) => InstallAction::Updated,
        (true, false) => InstallAction::Skipped,
    }
}

/// Resolve a target's SKILL.md path under an injected home directory. Pure
/// w.r.t. `$HOME` so it is unit-testable against a tempdir.
fn target_path(base_home: &Path, target: InstallTarget) -> PathBuf {
    let mut path = base_home.to_path_buf();
    for seg in target.segments() {
        path.push(seg);
    }
    path
}

struct InstallOutcome {
    path: PathBuf,
    action: InstallAction,
}

/// Install one target under `base_home`. Creates parent directories as
/// needed and writes the embedded skill unless the decision is `Skipped`.
/// Base dir is injected so this is testable without touching the real HOME.
fn install_one(
    base_home: &Path,
    target: InstallTarget,
    force: bool,
) -> std::io::Result<InstallOutcome> {
    let path = target_path(base_home, target);
    let action = decide_action(path.exists(), force);
    if action != InstallAction::Skipped {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, SKILL_MD)?;
    }
    Ok(InstallOutcome { path, action })
}

fn run_install(claude: bool, codex: bool, force: bool) -> ExitCode {
    // `$HOME` is required — user-level skill dirs hang off it (ADR-0055 D2).
    let base_home = match std::env::var_os("HOME") {
        Some(h) if !h.is_empty() => PathBuf::from(h),
        _ => {
            eprintln!(
                "gtmux skill install: $HOME is not set; cannot resolve the \
                 user-level skill directory."
            );
            return ExitCode::from(crate::EXIT_FAILURE);
        }
    };

    // Neither flag → install to both (the default).
    let (do_claude, do_codex) = if !claude && !codex {
        (true, true)
    } else {
        (claude, codex)
    };
    let mut targets = Vec::with_capacity(2);
    if do_claude {
        targets.push(InstallTarget::Claude);
    }
    if do_codex {
        targets.push(InstallTarget::Codex);
    }

    let mut any_error = false;
    for target in targets {
        match install_one(&base_home, target, force) {
            Ok(outcome) => {
                // Per-target result (path + created/updated/skipped) → stdout.
                println!(
                    "gtmux skill install [{}]: {} {}",
                    target.label(),
                    outcome.action.verb(),
                    outcome.path.display()
                );
                if outcome.action == InstallAction::Skipped {
                    eprintln!(
                        "gtmux skill install [{}]: {} already exists; pass --force to overwrite.",
                        target.label(),
                        outcome.path.display()
                    );
                }
            }
            Err(e) => {
                any_error = true;
                eprintln!("gtmux skill install [{}]: {}", target.label(), e);
            }
        }
    }

    if any_error {
        ExitCode::from(crate::EXIT_FAILURE)
    } else {
        ExitCode::SUCCESS
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_skill_is_nonempty_and_named() {
        // Length check (not `is_empty`) — `SKILL_MD` is a const, so clippy's
        // `const_is_empty` would flag a compile-time-constant `is_empty()`.
        // A substantial byte count also guards against a truncated embed.
        assert!(
            SKILL_MD.len() > 200,
            "embedded SKILL.md looks empty/truncated ({} bytes)",
            SKILL_MD.len()
        );
        assert!(
            SKILL_MD.contains("name: gtmux-cli"),
            "frontmatter must declare name: gtmux-cli"
        );
    }

    #[test]
    fn body_start_skips_frontmatter() {
        let start = body_start(SKILL_MD);
        assert!(start > 0, "SKILL.md has a frontmatter block");
        let body = &SKILL_MD[start..];
        // The frontmatter `name:` field must NOT survive into the body.
        assert!(
            !body.contains("name: gtmux-cli"),
            "body must exclude the frontmatter"
        );
        // First real content after the frontmatter is the H1 title.
        assert!(body.trim_start().starts_with("# gtmux CLI"));
    }

    #[test]
    fn body_start_zero_without_frontmatter() {
        assert_eq!(body_start("# just a title\nbody\n"), 0);
        assert_eq!(body_start(""), 0);
    }

    #[test]
    fn extract_section_pulls_one_section() {
        let sec = extract_section(SKILL_MD, "2").expect("section 2 exists");
        assert!(sec.starts_with("## 2."), "starts at the heading: {sec:?}");
        assert!(sec.contains("명령 표면"), "section 2 title present");
        // The next section must NOT bleed in.
        assert!(!sec.contains("## 3."), "stops before section 3");
        // Frontmatter must be excluded.
        assert!(!sec.contains("name: gtmux-cli"));
    }

    #[test]
    fn extract_section_handles_zero() {
        let sec = extract_section(SKILL_MD, "0").expect("section 0 exists");
        assert!(sec.starts_with("## 0."));
        assert!(sec.contains("전제"));
    }

    #[test]
    fn extract_section_missing_returns_none() {
        assert!(extract_section(SKILL_MD, "99").is_none());
        assert!(extract_section(SKILL_MD, "abc").is_none());
    }

    #[test]
    fn extract_section_does_not_overmatch_numeric_prefix() {
        let md = "---\nname: x\n---\n## 2. two\nbody-two\n## 20. twenty\nbody-twenty\n";
        let s2 = extract_section(md, "2").expect("section 2");
        assert!(s2.contains("two"));
        assert!(!s2.contains("twenty"), "must not swallow section 20");
        let s20 = extract_section(md, "20").expect("section 20");
        assert!(s20.contains("twenty"));
        assert!(s20.starts_with("## 20."));
    }

    #[test]
    fn extract_section_ignores_headings_inside_code_fence() {
        let md = "---\nname: x\n---\n## 1. real\n```\n## 2. fake-in-fence\n```\ntail\n## 2. actual\nafter\n";
        let s1 = extract_section(md, "1").expect("section 1");
        // The fenced `## 2.` must not terminate section 1 early.
        assert!(s1.contains("fake-in-fence"));
        assert!(s1.contains("tail"));
        // The real `## 2.` after the fence terminates it.
        assert!(!s1.contains("actual"));
        let s2 = extract_section(md, "2").expect("real section 2");
        assert!(s2.contains("after"));
    }

    #[test]
    fn decide_action_matrix() {
        assert_eq!(decide_action(false, false), InstallAction::Created);
        assert_eq!(decide_action(false, true), InstallAction::Created);
        assert_eq!(decide_action(true, false), InstallAction::Skipped);
        assert_eq!(decide_action(true, true), InstallAction::Updated);
    }

    #[test]
    fn target_paths_are_user_level() {
        let home = Path::new("/home/x");
        assert_eq!(
            target_path(home, InstallTarget::Claude),
            Path::new("/home/x/.claude/skills/gtmux-cli/SKILL.md")
        );
        assert_eq!(
            target_path(home, InstallTarget::Codex),
            Path::new("/home/x/.agents/skills/gtmux-cli/SKILL.md")
        );
    }

    #[test]
    fn install_one_created_then_skipped_then_forced() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path();

        // 1) fresh install → Created, content == embedded skill.
        let o1 = install_one(base, InstallTarget::Claude, false).expect("install");
        assert_eq!(o1.action, InstallAction::Created);
        assert!(o1.path.exists());
        assert_eq!(std::fs::read_to_string(&o1.path).unwrap(), SKILL_MD);

        // 2) mutate the file, re-install without --force → Skipped, unchanged.
        std::fs::write(&o1.path, "JUNK").unwrap();
        let o2 = install_one(base, InstallTarget::Claude, false).expect("install");
        assert_eq!(o2.action, InstallAction::Skipped);
        assert_eq!(std::fs::read_to_string(&o2.path).unwrap(), "JUNK");

        // 3) re-install with --force → Updated, content restored.
        let o3 = install_one(base, InstallTarget::Claude, true).expect("install");
        assert_eq!(o3.action, InstallAction::Updated);
        assert_eq!(std::fs::read_to_string(&o3.path).unwrap(), SKILL_MD);
    }

    #[test]
    fn install_one_creates_missing_parents() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Nested, non-existent home → create_dir_all must build the chain.
        let base = tmp.path().join("nonexistent").join("home");
        let outcome = install_one(&base, InstallTarget::Codex, false).expect("install");
        assert_eq!(outcome.action, InstallAction::Created);
        assert!(outcome.path.exists());
        assert!(outcome.path.ends_with(".agents/skills/gtmux-cli/SKILL.md"));
    }
}
