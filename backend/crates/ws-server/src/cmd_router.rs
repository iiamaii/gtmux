//! Client-origin frame → outbound tmux command message routing.
//!
//! The WS handler decodes each incoming envelope, parses its inner payload,
//! and emits a [`TmuxRequest`] over a [`tokio::sync::mpsc`] channel to the
//! single-writer tmux command loop in `gtmux_lifecycle`. Centralising the
//! mapping here keeps the WS handler free of tmux-domain knowledge.
//!
//! [`gtmux_mux_router::Command`] is the *type-system level* allowlist (a
//! closed enum with one variant per allowlisted command); this module adds
//! the run-time payload (pane id, argv strings, raw input bytes) that the
//! command loop needs to actually serialise a tmux control-mode line.
//!
//! 정본:
//! - `docs/ssot/wire-protocol.md` §2.1 (0x03 PANE_IN ↔ `send-keys`, 0x04
//!   PANE_RESIZE ↔ `resize-window`, 0x05/0x06 ↔ `refresh-client -A pause/continue`).
//! - `docs/adr/0008-single-pane-window-and-group.md` §command allowlist —
//!   11 variants. `split-window` / `resize-pane` / `select-layout` / `-CC`
//!   are not encodable by [`gtmux_mux_router::Command`] (the enum is closed),
//!   so disallowed commands surface as `ERR_NOT_ALLOWED` here without ever
//!   reaching the tmux daemon.

use gtmux_mux_router::Command;

/// One outbound message destined for the single-writer tmux command loop.
///
/// The `tmux_id` (`Some("uuid-…")`) is propagated back when the loop emits a
/// CTRL response so the client can match `%begin`/`%end` to the originating
/// request (SSoT §2.4 commentary, ADR-0001 D4).
#[derive(Debug, Clone)]
pub struct TmuxRequest {
    /// CTRL `id` field — opaque echo for client-side correlation.
    pub id: Option<String>,
    /// The command's closed-enum discriminator. Future maintenance: adding a
    /// new outbound command means extending [`gtmux_mux_router::Command`]
    /// *and* [`build_request`] simultaneously.
    pub command: Command,
    /// argv strings to splice after the command keyword. Per ADR-0001 D12
    /// the lifecycle writer asserts every line is non-empty *after* joining
    /// with the command keyword — empty bursts are rejected before they
    /// reach the tmux daemon (a bare `\n` is a detach trigger).
    pub args: Vec<String>,
}

/// The 11 allowlisted CTRL `cmd` strings (ADR-0008 §command allowlist).
///
/// Stored as `&'static str` so the lookup is a slice scan with zero
/// allocations on the hot path. Kept here (rather than in `mux-router`) to
/// avoid coupling the parser crate to the CTRL JSON shape.
pub const ALLOWLISTED_CTRL_CMDS: &[&str] = &[
    "new-window",
    "kill-pane",
    "kill-window",
    "rename-window",
    "send-keys",
    "refresh-client-pause",
    "refresh-client-continue",
    "refresh-client-subscribe",
    "capture-pane",
    "list-sessions",
    "list-windows",
    "list-panes",
];

/// `true` when `cmd` is in the allowlist.
pub fn is_allowed_ctrl_cmd(cmd: &str) -> bool {
    ALLOWLISTED_CTRL_CMDS.iter().any(|c| *c == cmd)
}

/// Build a [`TmuxRequest`] for an allowlisted CTRL `cmd` JSON request.
///
/// Returns `None` when:
///   * `cmd` is not in [`ALLOWLISTED_CTRL_CMDS`] — caller surfaces this as
///     `ERR_NOT_ALLOWED` per SSoT §2.4.
///
/// Callers MUST gate on [`is_allowed_ctrl_cmd`] beforehand; this function
/// matches on the same set defensively.
pub fn build_ctrl_request(id: Option<String>, cmd: &str, args: Vec<String>) -> Option<TmuxRequest> {
    let command = match cmd {
        "new-window" => Command::NewWindow,
        "kill-pane" => Command::KillPane,
        "kill-window" => Command::KillWindow,
        "rename-window" => Command::RenameWindow,
        "send-keys" => Command::SendKeys,
        "refresh-client-pause" | "refresh-client-continue" => Command::RefreshClientPause,
        "refresh-client-subscribe" => Command::RefreshClientSubscribe,
        "capture-pane" => Command::CapturePane,
        "list-sessions" => Command::ListSessions,
        "list-windows" => Command::ListWindows,
        "list-panes" => Command::ListPanes,
        _ => return None,
    };
    Some(TmuxRequest { id, command, args })
}

/// Build the [`TmuxRequest`] for a `0x03 PANE_IN` envelope — translates to
/// `tmux send-keys -t %<pane_id> -- <bytes-as-ascii-best-effort>`.
///
/// Input bytes are passed through verbatim as a single argv string; the
/// lifecycle writer is responsible for handling the line framing. Empty
/// `bytes` are dropped at the WS handler before reaching this function.
pub fn build_pane_in_request(pane_id: u32, bytes: &[u8]) -> TmuxRequest {
    // `-l` is `send-keys -l` (literal — disable name-to-key translation).
    // We pass bytes as a single arg using lossy UTF-8 since send-keys is
    // documented to accept arbitrary literal text and tmux propagates byte
    // sequences as-is over the pty. Non-UTF8 input is rare in practice
    // (xterm sequences are 7-bit ASCII); if it shows up we lossy-replace
    // rather than dropping the keystroke, preserving the typing flow.
    let literal = String::from_utf8_lossy(bytes).into_owned();
    TmuxRequest {
        id: None,
        command: Command::SendKeys,
        args: vec![
            "-l".to_string(),
            "-t".to_string(),
            format!("%{pane_id}"),
            literal,
        ],
    }
}

/// Build the [`TmuxRequest`] for a `0x04 PANE_RESIZE` envelope.
///
/// SSoT §2.1 + ADR-0008 D2: single-pane-per-window convention → use
/// `resize-window` (NOT `resize-pane`). Emits [`Command::ResizeWindow`]
/// directly; the lifecycle writer renders the canonical argv from the
/// variant fields, so `args` is empty here.
///
/// `pane_id` from the wire frame is treated as the window target under the
/// single-pane convention (ADR-0008 D1) — every gtmux-created Panel maps 1:1
/// to a tmux Window, so a pane-scoped resize trigger is also a window-scoped
/// resize trigger. `cols`/`rows` are u32 on the wire but tmux cell dimensions
/// fit comfortably in u16; we saturate at the upper bound to keep the variant
/// type-safe rather than reject the frame outright.
pub fn build_pane_resize_request(pane_id: u32, cols: u32, rows: u32) -> TmuxRequest {
    let cols = u16::try_from(cols).unwrap_or(u16::MAX);
    let rows = u16::try_from(rows).unwrap_or(u16::MAX);
    TmuxRequest {
        id: None,
        command: Command::ResizeWindow {
            window_id: pane_id,
            cols,
            rows,
        },
        args: Vec::new(),
    }
}

/// Build the [`TmuxRequest`] for a `0x05 PANE_PAUSE` envelope.
///
/// SSoT §2.1 + ADR-0001 D8: emit `refresh-client -A '%<pane_id>:pause'`.
/// The 300 ms debounce per ADR-0001 D8 is enforced at the WS handler level
/// (a tokio timer per pane_id collapses duplicate frames).
pub fn build_pane_pause_request(pane_id: u32) -> TmuxRequest {
    TmuxRequest {
        id: None,
        command: Command::RefreshClientPause,
        args: vec!["-A".to_string(), format!("%{pane_id}:pause")],
    }
}

/// Build the [`TmuxRequest`] for a `0x06 PANE_RESUME` envelope.
/// Same shape as pause, with a `:continue` suffix.
pub fn build_pane_resume_request(pane_id: u32) -> TmuxRequest {
    TmuxRequest {
        id: None,
        command: Command::RefreshClientPause,
        args: vec!["-A".to_string(), format!("%{pane_id}:continue")],
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_membership() {
        assert!(is_allowed_ctrl_cmd("new-window"));
        assert!(is_allowed_ctrl_cmd("kill-pane"));
        assert!(!is_allowed_ctrl_cmd("split-window"));
        assert!(!is_allowed_ctrl_cmd("resize-pane"));
        assert!(!is_allowed_ctrl_cmd("select-layout"));
        assert!(!is_allowed_ctrl_cmd(""));
    }

    #[test]
    fn build_ctrl_for_each_allowed_cmd() {
        for cmd in ALLOWLISTED_CTRL_CMDS {
            let req = build_ctrl_request(None, cmd, vec![]);
            assert!(req.is_some(), "allowlisted cmd '{cmd}' must build");
        }
    }

    #[test]
    fn build_ctrl_rejects_unknown() {
        assert!(build_ctrl_request(None, "split-window", vec![]).is_none());
        assert!(build_ctrl_request(None, "", vec![]).is_none());
    }

    #[test]
    fn pane_in_args_shape() {
        let req = build_pane_in_request(37, b"ls\n");
        assert_eq!(req.args, vec!["-l", "-t", "%37", "ls\n"]);
    }

    #[test]
    fn pane_resize_routes_to_resize_window() {
        // S5-MUX-1: PANE_RESIZE → Command::ResizeWindow direct emission.
        // No more keyword-override via Command::ListWindows.
        let req = build_pane_resize_request(37, 120, 40);
        assert!(
            req.args.is_empty(),
            "args is empty — variant carries fields"
        );
        match req.command {
            Command::ResizeWindow {
                window_id,
                cols,
                rows,
            } => {
                assert_eq!(window_id, 37);
                assert_eq!(cols, 120);
                assert_eq!(rows, 40);
            }
            other => panic!("expected ResizeWindow, got {other:?}"),
        }
    }

    #[test]
    fn pane_resize_saturates_oversized_dimensions() {
        // Wire u32 → variant u16. Anything beyond u16::MAX saturates so the
        // frame is never silently dropped at the boundary.
        let req = build_pane_resize_request(1, 70_000, 80_000);
        match req.command {
            Command::ResizeWindow { cols, rows, .. } => {
                assert_eq!(cols, u16::MAX);
                assert_eq!(rows, u16::MAX);
            }
            other => panic!("expected ResizeWindow, got {other:?}"),
        }
    }

    #[test]
    fn pane_pause_resume_distinct_args() {
        let p = build_pane_pause_request(7);
        let r = build_pane_resume_request(7);
        assert_eq!(p.args, vec!["-A", "%7:pause"]);
        assert_eq!(r.args, vec!["-A", "%7:continue"]);
    }
}
