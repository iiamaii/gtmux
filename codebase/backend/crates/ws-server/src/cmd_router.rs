//! 0x01 CTRL command router — translates `CtrlPayload {id, cmd, args}` into
//! the [`gtmux_pty_backend::BackendCommand`] enum and dispatches it. After
//! ADR-0013 the tmux argv allowlist is gone — the closed enum is the new
//! compile-time allowlist (`#[serde(tag = "type")]` on BackendCommand).
//!
//! The wire shape stays `{id, cmd, args}` so the frontend dispatcher does
//! not need to change envelope serialisation (that is the parallel
//! S7-WS-PAYLOAD-SIMPLIFY frontend task). Only the *cmd vocabulary*
//! changes — tmux argv strings → PTY-domain enum tags.
//!
//! Allowlisted commands (the only `cmd` strings that produce a backend call):
//! - `"new-pane"`        → [`BackendCommand::NewPane`]    (request_id = `id`)
//! - `"kill-pane"`       → [`BackendCommand::KillPane`]   (args = `[pane_id]`)
//! - `"resize-pane"`     → [`BackendCommand::ResizePane`] (args = `[pane_id, rows, cols]`)
//! - `"kill-session"`    → [`BackendCommand::KillSession`] (no args)
//!
//! Any other `cmd` produces an `ERR_NOT_ALLOWED` reply (compile-time
//! enforced — a future variant of [`BackendCommand`] would need a new
//! arm here, otherwise the rustc exhaustive-match warning catches the
//! gap before runtime).

use gtmux_pty_backend::{BackendCommand, PaneId, PtyBackend, PtyBackendError, SpawnSpec};

/// Outcome of routing a single CTRL frame. The WS handler uses this to
/// decide what (if any) reply envelope to encode.
#[derive(Debug)]
pub enum CtrlOutcome {
    /// Command accepted and dispatched. For `NewPane` the resulting
    /// `PaneId` will be surfaced via a `PaneSpawned` NOTIFY broadcast —
    /// no immediate envelope reply is generated here.
    Ok,
    /// `KillSession` accepted. WS handler should encode the CTRL `ok`
    /// reply *and then* raise SIGTERM on itself so axum's graceful
    /// shutdown future fires (ADR-0014 D7).
    OkAndExit,
    /// Command rejected — not in the allowlist. WS handler responds with
    /// `ERR_NOT_ALLOWED` keeping the connection alive.
    NotAllowed,
    /// Command well-formed but failed at the backend layer (pane not
    /// found, resize failed, etc.). WS handler responds with
    /// `ERR_BACKEND` keeping the connection alive.
    BackendError(PtyBackendError),
    /// Command syntactically invalid (bad argv shape). WS handler
    /// responds with `ERR_BAD_REQUEST` keeping the connection alive.
    BadRequest,
}

/// Dispatch a parsed CTRL envelope. `cmd` and `args` come from
/// [`crate::payload::decode_ctrl_request`]; `request_id` is the
/// envelope's `id` field (echoed back via NOTIFY_MIRROR when a
/// pane is spawned).
pub fn dispatch_ctrl(
    backend: &PtyBackend,
    request_id: Option<String>,
    cmd: &str,
    args: &[String],
) -> CtrlOutcome {
    match cmd {
        "new-pane" => {
            // Args layout (forward-compat): all optional, positional.
            //   args[0] = command (executable path, default = $SHELL)
            //   args[1..] = arguments
            // The frontend MVP sends an empty args list (= default shell).
            let (command, tail) = match args.split_first() {
                Some((first, rest)) if !first.is_empty() => (Some(first.clone()), rest.to_vec()),
                _ => (None, Vec::new()),
            };
            let spec = SpawnSpec {
                command,
                args: tail,
                ..SpawnSpec::default_shell()
            };
            let result = match request_id {
                Some(rid) => backend.spawn_with_request(spec, rid),
                None => backend.spawn(spec),
            };
            match result {
                Ok(_) => CtrlOutcome::Ok,
                Err(e) => CtrlOutcome::BackendError(e),
            }
        }
        "kill-pane" => {
            let Some(id) = parse_pane_id(args.first().map(String::as_str)) else {
                return CtrlOutcome::BadRequest;
            };
            match backend.dispatch(BackendCommand::KillPane { id }) {
                Ok(_) => CtrlOutcome::Ok,
                Err(e) => CtrlOutcome::BackendError(e),
            }
        }
        "resize-pane" => {
            if args.len() < 3 {
                return CtrlOutcome::BadRequest;
            }
            let Some(id) = parse_pane_id(Some(&args[0])) else {
                return CtrlOutcome::BadRequest;
            };
            let Ok(rows) = args[1].parse::<u16>() else {
                return CtrlOutcome::BadRequest;
            };
            let Ok(cols) = args[2].parse::<u16>() else {
                return CtrlOutcome::BadRequest;
            };
            match backend.dispatch(BackendCommand::ResizePane { id, rows, cols }) {
                Ok(_) => CtrlOutcome::Ok,
                Err(e) => CtrlOutcome::BackendError(e),
            }
        }
        "kill-session" => {
            // No args, no validation surface. `KillSession` dispatch is a
            // no-op at backend level; the OkAndExit signal tells the WS
            // handler to ack + self-SIGTERM (ADR-0013 D10 amend).
            match backend.dispatch(BackendCommand::KillSession) {
                Ok(_) => CtrlOutcome::OkAndExit,
                Err(e) => CtrlOutcome::BackendError(e),
            }
        }
        _ => CtrlOutcome::NotAllowed,
    }
}

/// `true` when `cmd` is one of the three CTRL strings we route. Used by
/// the WS handler's allowlist gate (we surface `ERR_NOT_ALLOWED` *before*
/// calling [`dispatch_ctrl`] so the trace logs are explicit about why a
/// command was refused).
pub fn is_allowed_ctrl_cmd(cmd: &str) -> bool {
    matches!(
        cmd,
        "new-pane" | "kill-pane" | "resize-pane" | "kill-session"
    )
}

/// The full allowlist as a `&[&str]` for tests + future help-text printing.
pub const ALLOWLISTED_CTRL_CMDS: &[&str] =
    &["new-pane", "kill-pane", "resize-pane", "kill-session"];

/// Parse an argv element into a [`PaneId`]. The frontend serialises pane
/// ids as decimal strings (e.g. `"5"`); accept that single shape.
fn parse_pane_id(s: Option<&str>) -> Option<PaneId> {
    s?.parse::<u64>().ok().map(PaneId)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_membership() {
        assert!(is_allowed_ctrl_cmd("new-pane"));
        assert!(is_allowed_ctrl_cmd("kill-pane"));
        assert!(is_allowed_ctrl_cmd("resize-pane"));
        // tmux-era commands are gone.
        assert!(!is_allowed_ctrl_cmd("new-window"));
        assert!(!is_allowed_ctrl_cmd("send-keys"));
        assert!(!is_allowed_ctrl_cmd("kill-window"));
        assert!(!is_allowed_ctrl_cmd("refresh-client-pause"));
        assert!(!is_allowed_ctrl_cmd(""));
    }

    #[test]
    fn unknown_cmd_rejected() {
        let backend = PtyBackend::new();
        let r = dispatch_ctrl(&backend, None, "format-disk", &[]);
        assert!(matches!(r, CtrlOutcome::NotAllowed));
    }

    #[test]
    fn kill_pane_with_no_id_is_bad_request() {
        let backend = PtyBackend::new();
        let r = dispatch_ctrl(&backend, None, "kill-pane", &[]);
        assert!(matches!(r, CtrlOutcome::BadRequest));
    }

    #[test]
    fn kill_pane_with_non_numeric_id_is_bad_request() {
        let backend = PtyBackend::new();
        let r = dispatch_ctrl(&backend, None, "kill-pane", &["not-a-number".into()]);
        assert!(matches!(r, CtrlOutcome::BadRequest));
    }

    #[test]
    fn kill_unknown_pane_surfaces_backend_error() {
        let backend = PtyBackend::new();
        // id 999 was never spawned — backend returns PaneNotFound.
        let r = dispatch_ctrl(&backend, None, "kill-pane", &["999".into()]);
        assert!(matches!(
            r,
            CtrlOutcome::BackendError(PtyBackendError::PaneNotFound(_))
        ));
    }

    #[test]
    fn resize_pane_requires_three_args() {
        let backend = PtyBackend::new();
        let r = dispatch_ctrl(&backend, None, "resize-pane", &["1".into(), "40".into()]);
        assert!(matches!(r, CtrlOutcome::BadRequest));
    }

    #[test]
    fn resize_pane_bad_rows_rejected() {
        let backend = PtyBackend::new();
        let r = dispatch_ctrl(
            &backend,
            None,
            "resize-pane",
            &["1".into(), "not".into(), "132".into()],
        );
        assert!(matches!(r, CtrlOutcome::BadRequest));
    }
}
