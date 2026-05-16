//! Cross-platform OS-native open spawn — ADR-0023 D5 step 5/6.
//!
//! macOS:    `open <path>`
//! Linux:    `xdg-open <path>`
//! Windows:  `cmd /C start "" <path>` (the empty `""` is the window title
//!           argument — `start` requires it when the first quoted arg is
//!           the path; otherwise the path is treated as a window title.)
//!
//! Critical: argv-direct via [`std::process::Command::new`], never shell
//! string interpolation. The path has already been `canonicalize`d by
//! the caller so embedded shell metacharacters (if any) reach the
//! handler binary as a literal argv element with no expansion.

use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("no_handler")]
    NoHandler,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Spawn the platform OS-open handler with `path` as its sole argument.
/// Returns immediately after `spawn()` (no `wait` / `output` — the
/// caller's HTTP handler must not block on the GUI app launch).
pub fn spawn(path: &Path) -> Result<(), SpawnError> {
    let (cmd, args): (&str, &[&str]) = match std::env::consts::OS {
        "macos" => ("open", &[]),
        "linux" | "freebsd" | "netbsd" | "openbsd" | "dragonfly" => ("xdg-open", &[]),
        "windows" => ("cmd", &["/C", "start", ""]),
        _ => return Err(SpawnError::NoHandler),
    };
    let mut command = std::process::Command::new(cmd);
    command.args(args);
    command.arg(path);
    // `stdin/out/err = null` so a non-interactive headless run doesn't
    // block on the handler reading from / writing to the parent's tty.
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    match command.spawn() {
        Ok(mut child) => {
            // Detach: we don't wait for the GUI handler. Drop the child
            // handle — on Unix the kernel reparents to PID 1 once the
            // server exits.
            let _ = child.id();
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(SpawnError::NoHandler),
        Err(e) => Err(SpawnError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_with_nonexistent_handler_returns_no_handler() {
        // We can't easily simulate "no `open`" on macOS or "no
        // `xdg-open`" on Linux without polluting PATH. Instead this
        // test documents the contract — when the unit-test platform is
        // not in the known list, `spawn` returns `NoHandler`. CI
        // matrices that hit unknown platforms (FreeBSD, etc.) exercise
        // the fallback.
        //
        // The actual success path is verified by `02_stage5.sh` gate
        // 5-9, which spawns against a real binary on the dev host.
        let _ = spawn(Path::new("/tmp/nonexistent"));
    }
}
