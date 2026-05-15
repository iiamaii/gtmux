//! gtmux CLI — clap derive entrypoint (D20 subcommand set).
//!
//! `start` is the Stage-B PTY-direct bootstrap (ADR-0013 / ADR-0014):
//! TMUX env guard → config → pidfile liveness probe → token issue/load
//! (mode-branched per ADR-0003 D13.1) → PtyBackend construction →
//! Hub + axum router mount → bind + pidfile write → first-run banner →
//! graceful shutdown (drop PtyBackend → all child shells reaped).
//!
//! `teardown` / `rotate-token` / `status` consume the same state-file
//! layout as the pre-Stage-B era (ADR-0014 D7's 4-step variant): pidfile,
//! token, and layout.json under `${XDG_STATE_HOME}/gtmux/`, config TOML
//! under `${XDG_CONFIG_HOME}/gtmux/`. The state-file helpers live in the
//! [`state_files`] module (inlined from the pre-Stage-B `crates/lifecycle`
//! after ADR-0013 made the tmux-specific bulk of that crate obsolete).
//!
//! `stop` writes SIGTERM to the pidfile process and waits up to 5 s for
//! exit (escalating to SIGKILL only when `--force` is passed). Because
//! the Stage-B server owns its child shells directly (no separate tmux
//! daemon), `stop` is now sufficient for clean shutdown — there is no
//! survivor process to clean up afterwards. `teardown` remains the
//! single destructive on-disk cleanup.

// `deny(unsafe_code)` (not `forbid`) so `state_files.rs` can keep its
// libc::kill / geteuid FFI shim — the unsafe is isolated to that module's
// helper functions, all justified inline.
#![deny(unsafe_code)]
#![warn(clippy::all)]

mod process_audit;
mod state_files;

use std::io::IsTerminal;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context};
use axum::Router;
use clap::{Parser, Subcommand};
use gtmux_auth::{issue_token, load_token, rotate_token, save_token, AuthError, TokenString};
use gtmux_config::{derive_mode, load_with_overrides as load_config, Config, Mode};
use gtmux_pty_backend::PtyBackend;
use gtmux_ws_server::Hub;
use state_files::{
    check_pidfile_liveness, layout_path_for, pidfile_path_for, stop_server, write_pidfile,
    PidLiveness, StateFileError, StopOutcome, TeardownOpts, TeardownReport,
};
use tokio::net::TcpListener;
use tokio::signal::unix::{signal, SignalKind};
use tracing::{error, info, warn};

// ────────────────────────────────────────────────────────────────────────────
// Exit codes (grill D20)
// ────────────────────────────────────────────────────────────────────────────

/// Generic failure (unknown / aggregated). clap usage errors map to 2 by clap.
const EXIT_FAILURE: u8 = 1;
/// Session missing (`gtmux start` cannot auto-create — ADR-0007 D3).
const EXIT_SESSION_MISSING: u8 = 3;
/// Port in use or duplicate Server bind (ADR-0007 §결과, grill D20).
const EXIT_PORT_IN_USE: u8 = 4;
/// Permission denied (token perm fail-closed, EUID==0 without --allow-root).
const EXIT_PERMISSION: u8 = 5;
/// tmux daemon communication failure (binary missing, daemon crash).
const EXIT_TMUX: u8 = 6;
/// teardown partial failure — at least one of the five D6 steps surfaced a
/// non-fatal warning. The cleanup still ran to completion; this exit code
/// signals "look at stderr to decide if you need to mop up by hand".
const EXIT_TEARDOWN_PARTIAL: u8 = 7;

// ────────────────────────────────────────────────────────────────────────────
// CLI surface
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Parser)]
#[command(
    name = "gtmux",
    version,
    about = "gtmux — tmux-backed web canvas workspace (CLI)",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Start a gtmux Server (spawns dedicated tmux daemon per ADR-0009).
    Start {
        /// Session name; binds 1:1:1 to Server : tmux session : port.
        #[arg(long)]
        session: String,
        /// HTTP/WS listen port. When omitted the value is taken from the
        /// per-session config file (D21 c6 port lookup). When provided here
        /// the CLI flag wins — ADR-0007 D2 immutable bind still holds because
        /// the override happens *before* the first listener call.
        #[arg(long)]
        port: Option<u16>,
        /// Explicit config path. When omitted figment falls back to defaults
        /// + env-only — useful for first-run / smoke contexts.
        #[arg(long = "config", value_name = "PATH")]
        config_path: Option<PathBuf>,
        /// Workspace storage directory (ADR-0019 D2 / D11, boot-immutable).
        /// Overrides the TOML `workspace_path` field. When neither is set the
        /// default is `${XDG_DATA_HOME:-~/.local/share}/gtmux/workspace/`.
        #[arg(long = "workspace", value_name = "PATH")]
        workspace_path: Option<PathBuf>,
    },
    /// Stop a running gtmux Server (소켓·daemon은 그대로 둔다).
    ///
    /// Reads the pidfile at `${XDG_STATE_HOME}/gtmux/<session>.pid`, sends
    /// SIGTERM, and waits up to 5 s for the process to exit. ADR-0009 D5
    /// is preserved: the tmux daemon (and therefore session / pane state)
    /// survives this call so a subsequent `gtmux start --session <name>`
    /// can re-attach via `new-session -A` (D21 c2).
    Stop {
        #[arg(long)]
        session: String,
        /// On SIGTERM grace timeout, escalate to SIGKILL instead of
        /// returning exit 6. Use sparingly — SIGKILL gives the server no
        /// chance to flush layout state or close WS connections cleanly.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Teardown: ADR-0009 §D6 5-step cleanup (socket·token·layout·pid·config).
    Teardown {
        /// Session name (positional or `--session`). Required.
        #[arg(long)]
        session: String,
        /// Skip the live-daemon refusal — issue `kill-server` outright and
        /// reap the socket after a short settling delay. The user-visible
        /// confirmation prompt that normally precedes this flag lives in
        /// the CLI's TTY branch (Use --force on non-interactive callers).
        #[arg(long, default_value_t = false)]
        force: bool,
        /// Preserve the per-session state files (token / layout / pid).
        /// Useful when capturing post-mortem evidence; the operator can
        /// remove these by hand afterwards.
        #[arg(long = "keep-state", default_value_t = false)]
        keep_state: bool,
        /// Preserve `${XDG_CONFIG_HOME}/gtmux/<session>.config.toml` so the
        /// Server can be brought back up with the same identity (D21 c8).
        #[arg(long = "keep-config", default_value_t = false)]
        keep_config: bool,
    },
    /// Rotate the session token (cloud 모드 전용; local은 매 start 재발급).
    RotateToken {
        #[arg(long)]
        session: String,
    },
    /// Status: running Servers + bound ports + daemon health summary.
    ///
    /// With `--session`, prints a single-row table. Without it, enumerates
    /// every `${XDG_STATE_HOME}/gtmux/*.token` and probes the matching
    /// daemon via `tmux ... has-session`.
    Status {
        /// Restrict the report to one session. When omitted every session
        /// with a token file is listed.
        #[arg(long)]
        session: Option<String>,
    },
    /// Set or replace the password used by ADR-0020 password-mode auth.
    /// Prompts twice on the TTY (or reads from stdin in non-interactive
    /// environments) and writes an Argon2id PHC hash to
    /// `${XDG_STATE_HOME:-~/.local/state}/gtmux/password.argon2` (mode 0600).
    SetPassword,
    /// Remove the password hash file. Equivalent to `rm -f
    /// ${XDG_STATE_HOME}/gtmux/password.argon2` but goes through the same
    /// path resolution as `set-password` so XDG overrides apply.
    ResetPassword,
}

fn main() -> ExitCode {
    // We hand-roll the runtime so `main` can convert anyhow errors into the
    // grill-D20 exit-code matrix without losing context (clap's anyhow path
    // collapses everything to exit 1).
    let cli = Cli::parse();
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("gtmux: failed to start tokio runtime: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };

    match cli.command {
        Cmd::Start {
            session,
            port,
            config_path,
            workspace_path,
        } => match rt.block_on(start(StartArgs {
            session,
            port,
            config_path,
            workspace_override: workspace_path,
        })) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => report_start_error(e),
        },
        Cmd::Stop { session, force } => rt.block_on(stop(&session, force)),
        Cmd::Teardown {
            session,
            force,
            keep_state,
            keep_config,
        } => rt.block_on(teardown_cmd(TeardownArgs {
            session,
            force,
            keep_state,
            keep_config,
        })),
        Cmd::RotateToken { session } => rotate_token_cmd(&session),
        Cmd::Status { session } => rt.block_on(status_cmd(session.as_deref())),
        Cmd::SetPassword => set_password_cmd(),
        Cmd::ResetPassword => reset_password_cmd(),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// `gtmux start`
// ────────────────────────────────────────────────────────────────────────────

struct StartArgs {
    session: String,
    port: Option<u16>,
    config_path: Option<PathBuf>,
    /// `--workspace <path>` override (ADR-0019 D2 / D11). Beats the TOML
    /// `workspace_path` field; both unset → XDG_DATA_HOME default.
    workspace_override: Option<PathBuf>,
}

/// Execute the P0-CLI-1 14-step bootstrap sequence (grill report §3.1 +
/// Sprint 4-D LIFE-3 pidfile addition).
///
/// Step matrix (D20 + bootstrap-smoke §3 P0-CLI-1):
///   1) parse CLI args             — clap above
///   2) load config                — figment (CLI > Env > TOML > defaults)
///      2a) pidfile liveness probe — LIFE-3: refuse duplicate server bind
///   3) derive mode                — bind value → Local / Cloud (D22)
///   4) init tracing               — log_level + log_format (text/json/auto)
///   5) spawn tmux daemon          — ADR-0009 D2/D3 dedicated daemon
///   6) issue / load token         — ADR-0003 D13.1 mode-branched
///   7) build http router          — placeholder mount (real lands in HTTP-1)
///   8) build ws router            — placeholder mount (real lands in WS-1)
///   9) merge into a single app
///  10) bind TCP listener          — `bind` + `port` from config (D2)
///      10a) write server pidfile  — LIFE-3: in-band channel for `gtmux stop`
///  11) print first-run banner     — D21 c1 + ADR-0003 D3 token URL
///  12) install shutdown handlers  — SIGINT + SIGTERM → graceful (D5 daemon ⊥)
///  13) axum::serve(...)           — with_graceful_shutdown
async fn start(args: StartArgs) -> anyhow::Result<()> {
    // 1a) Nested-tmux startup guard — ADR-0014 D10 amend (2026-05-14) 1차 방어.
    //     If the user is running gtmux from inside an existing outer tmux
    //     session, the inherited `TMUX` env makes the child shells we spawn
    //     visible to that outer tmux server. Best to fail fast with a clear
    //     diagnostic — 0022 L-17 "prevention > recovery" principle.
    if let Ok(val) = std::env::var("TMUX") {
        return Err(StartError::NestedTmux(val).into());
    }

    // 2) config — figment chain. CLI `--session` and `--port` are passed in as
    //    figment overrides so they win against TOML / env *before* validation
    //    runs. Mutating `config.server.port` after load is the old path; it
    //    fails when the TOML omits `[server].port` because the sentinel 0 dies
    //    in `validate()` before the CLI ever gets a chance to speak. `bind`
    //    is intentionally not overridable here (security mode D22 flips on it).
    let config = load_config(args.config_path.as_deref(), &args.session, args.port)
        .context("loading gtmux config")?;

    // 2a) pidfile liveness — refuse duplicate server bind on the same session
    //     before we go anywhere near tmux. ADR-0007 D2's 1:1:1 invariant
    //     forbids two Servers per session; the pidfile is the cheap canonical
    //     check. Stale pidfiles (server crashed) downgrade to a warning and
    //     are overwritten in step 10a.
    match check_pidfile_liveness(&config.server.session) {
        Ok(PidLiveness::Alive(pid)) => {
            return Err(StartError::AlreadyRunning {
                session: config.server.session.clone(),
                pid,
            }
            .into());
        }
        Ok(PidLiveness::Stale(pid)) => {
            warn!(
                session = %config.server.session,
                stale_pid = pid,
                "stale gtmux pidfile detected; previous server appears to have crashed — overwriting on bind"
            );
        }
        Ok(PidLiveness::Malformed) => {
            warn!(
                session = %config.server.session,
                "malformed gtmux pidfile detected; overwriting on bind"
            );
        }
        Ok(PidLiveness::Absent) => {
            // First start (or post-teardown) — nothing to check.
        }
        Err(e) => {
            // BadXdg / IO error on the pidfile probe is itself a hard failure
            // because we cannot guarantee uniqueness without it.
            return Err(anyhow::Error::new(e).context("probing pidfile liveness"));
        }
    }

    // 3) mode is *derived* from `bind`; we capture it so subsequent code can
    //    branch (token policy, future TLS/CSP) without re-parsing.
    let mode = derive_mode(&config.server.bind);

    // 4) tracing — explicit, JSON when piped or asked, ANSI text on a tty.
    init_tracing(&config);

    info!(
        session = %config.server.session,
        port = config.server.port,
        bind = %config.server.bind,
        mode = ?mode,
        "gtmux start booting",
    );

    // 4a) Boot-time orphan reap (ADR-0014 D11) — scan all live processes
    //     for `GTMUX_SESSION=<our-session>` from a *previous* gtmux Server
    //     run (`GTMUX_SERVER_PID != our pid`). Common on graceful boot
    //     this returns 0 candidates in milliseconds. After a crash
    //     (SIGKILL / OOM / panic) it cleans up the orphaned shells.
    let audit = process_audit::reap_orphans(&config.server.session);
    if !audit.candidates.is_empty() {
        info!(
            reaped = audit.candidates.len(),
            "process_audit: cleaned up {} orphan(s) from previous session",
            audit.candidates.len()
        );
    }

    // 5) PtyBackend — Stage B (ADR-0013 D1, ADR-0014 D1): we own every
    //    PTY pair + child shell directly. No daemon to spawn — `new()` is
    //    instant + infallible, and the per-pane reader / writer / wait
    //    threads come into existence lazily on the first `spawn()` call.
    //    `with_session(...)` tags every spawned child with
    //    `GTMUX_SESSION=<session>` + `GTMUX_SERVER_PID=<pid>` so the
    //    boot-time orphan scanner (ADR-0014 D11) can identify strays
    //    from a previous crashed Server.
    let backend = PtyBackend::with_session(Some(config.server.session.clone()));
    info!("pty backend ready (ADR-0013 + ADR-0014 supervisor model)");

    // 6) token — ADR-0003 D13.1:
    //    * Local mode: re-issue every start (Jupyter pattern); the banner
    //      below transports it to the user on this run only.
    //    * Cloud mode: persist across restarts; issue on first run only.
    let token = match mode {
        Mode::Local => {
            let t = issue_token().context("issuing local-mode token")?;
            save_token(&config.server.session, &t).context("persisting local-mode token")?;
            t
        }
        Mode::Cloud => match load_token(&config.server.session) {
            Ok(t) => t,
            Err(AuthError::NotFound(_)) => {
                let t = issue_token().context("issuing cloud-mode token")?;
                save_token(&config.server.session, &t).context("persisting cloud-mode token")?;
                t
            }
            Err(e) => return Err(e).context("loading cloud-mode token"),
        },
    };

    // 6a) layout file path — S7-PERSISTENCE-MINIMAL / ADR-0006 (legacy
    //     `/api/layout`). The multi-session storage (ADR-0019) lives in a
    //     *different* directory; the legacy file remains for backwards
    //     compatibility with the singular `/api/layout` route.
    let layout_path = layout_path_for(&config.server.session)
        .context("resolving layout file path under XDG_STATE_HOME")?;

    // 6b) workspace — ADR-0019 D2 / D11 boot-immutable bind. Precedence:
    //     CLI `--workspace` > TOML `workspace_path` > XDG_DATA_HOME default.
    //     The boot-time v1→v2 migration scans the resolved dir for legacy
    //     records (ADR-0018 D5 / ADR-0006 D15) and rewrites them in place.
    let workspace = gtmux_http_api::WorkspaceManager::resolve(
        args.workspace_override.clone(),
        config.workspace_path.clone(),
    )
    .context("resolving workspace directory")?;
    let migration = workspace
        .boot_migration_v1_to_v2()
        .context("workspace boot migration v1→v2")?;
    if migration.migrated > 0 || migration.quarantined > 0 {
        info!(
            migrated = migration.migrated,
            quarantined = migration.quarantined,
            workspace = %workspace.path().display(),
            "workspace: boot migration complete"
        );
    }

    // 6c) password hash — ADR-0020 D5. Loaded eagerly when `auth.mode =
    //     "password"` so a missing file fails fast at boot rather than at
    //     first login attempt. In token mode we leave it unset.
    let password_hash: Option<String> = if config.auth.mode == "password" {
        let path = gtmux_http_api::default_password_hash_path()
            .context("resolving password hash path")?;
        match gtmux_http_api::load_password_hash(&path) {
            Ok(h) => {
                info!(path = %path.display(), "auth: loaded password hash");
                Some(h)
            }
            Err(gtmux_http_api::AuthError::HashFileMissing(p)) => {
                warn!(
                    path = %p.display(),
                    "auth: password mode is configured but no hash file exists yet; \
                     run `gtmux set-password` before any client tries to log in"
                );
                None
            }
            Err(e) => {
                return Err(anyhow!("loading password hash: {e}"));
            }
        }
    } else {
        None
    };

    // 7+8+9) router — HTTP API (layout, bootstrap, healthz) + WebSocket (/ws).
    //   The Hub wraps the PtyBackend (ADR-0013 D11 trivial multi-attach mirror
    //   + the multiplexed `(pane_id, bytes)` broadcast described in
    //   `docs/reports/0026-stage-b-carry-forward.md` §2.4). HTTP / WS share
    //   Origin/Host invariants but have independent middleware stacks.
    let hub = Hub::new(backend.clone());

    // ADR-0019 D6 / ADR-0021 D6: wire two cookie-tagged signal channels
    // from the WS layer to the http-api lock table.
    //   * `disconnect` — emitted on WS close; releases the session lock.
    //   * `heartbeat`  — emitted on every Ping/Pong; refreshes the lease
    //     body so peeking modals see an accurate expected-expiry hint.
    let (disconnect_tx, mut disconnect_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (heartbeat_tx, mut heartbeat_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    hub.set_disconnect_sink(disconnect_tx);
    hub.set_heartbeat_sink(heartbeat_tx);

    let app_state = build_app_state(
        &config,
        &token,
        hub.clone(),
        layout_path,
        workspace,
        password_hash,
    );

    // Stage 5 D10 α: register the cookie validator so the WS handshake
    // accepts cookie auth as an alternative to the subprotocol bearer
    // (ADR-0020 D10 additive). The legacy bearer path stays in place —
    // automation / CLI scripts that pass only the token keep working.
    hub.set_cookie_validator(app_state.session_table.clone());

    // 0040 §5 option A: terminal UUID provider so the WS catch-up replays
    // alive UUID↔PaneId bindings as `0x88 TERMINAL_SPAWNED` frames on every
    // new handshake. Closes the reload / WS reconnect gap where the FE
    // `XtermHost` would otherwise stay on the "connecting" placeholder.
    hub.set_terminal_uuid_provider(app_state.terminal_map.clone());

    let state_for_disconnect = app_state.clone();
    let _disconnect_task = tokio::spawn(async move {
        while let Some(cookie) = disconnect_rx.recv().await {
            state_for_disconnect.release_lock_for_cookie(&cookie).await;
        }
    });
    let state_for_heartbeat = app_state.clone();
    let _heartbeat_task = tokio::spawn(async move {
        while let Some(cookie) = heartbeat_rx.recv().await {
            state_for_heartbeat.refresh_lease_for_cookie(&cookie).await;
        }
    });

    // Stage 4-E hygiene: keep `TerminalMap` consistent with the alive pool
    // by reacting to BackendNotify::PaneDied (a kernel SIGCHLD or an
    // explicit kill). Without this consumer, killed Panes would linger in
    // the map and confuse the match-or-spawn algorithm on the next attach.
    let state_for_pane_died = app_state.clone();
    let mut notify_rx = hub.subscribe_notify();
    let _pane_died_task = tokio::spawn(async move {
        use gtmux_pty_backend::BackendNotify;
        loop {
            match notify_rx.recv().await {
                Ok(BackendNotify::PaneDied { id, signal, .. }) => {
                    state_for_pane_died.handle_pane_died(id, signal).await;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        lagged = n,
                        "pane_died consumer fell behind broadcast; resuming"
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let app = build_router(app_state, &config, &token, hub.clone());

    // 10) bind — TCP only for now (unix socket variant lives behind
    //    `bind = "unix:/..."` and is a planned alt-path; surface a friendly
    //    error rather than half-implementing it).
    if config.server.bind.starts_with("unix:") {
        return Err(anyhow!(
            "unix-socket bind ({}) is not yet wired; use a loopback IP for now",
            config.server.bind
        ));
    }
    let bind_ip: IpAddr = config.server.bind.parse().with_context(|| {
        format!(
            "parsing server.bind={:?} as an IP address",
            config.server.bind
        )
    })?;
    let addr = SocketAddr::from((bind_ip, config.server.port));
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            return Err(BindError::InUse(addr).into());
        }
        Err(e) => {
            return Err(anyhow::Error::new(e).context(format!("binding {addr}")));
        }
    };

    // 10a) pidfile — write *after* the bind succeeds so a duplicate-bind
    //      attempt (caught at step 10) doesn't leave a misleading pidfile
    //      pointing at a process that never actually held the port.
    let pidfile_path = match write_pidfile(&config.server.session) {
        Ok(p) => Some(p),
        Err(e) => {
            warn!(error = %e, "failed to write gtmux pidfile; `gtmux stop` will be unavailable for this run");
            None
        }
    };

    // 11) banner — D21 c1 + ADR-0003 D3. We emit the cleartext token URL
    //    exactly once; subsequent traffic must use Authorization: Bearer
    //    or the WebSocket subprotocol.
    print_banner(&config, mode, &token, listener.local_addr().ok());

    // 12) shutdown — install both SIGINT (Ctrl-C) and SIGTERM listeners.
    //    The graceful shutdown future ends when *either* fires; axum then
    //    drains in-flight requests. ADR-0014 D5: dropping the PtyBackend
    //    sends SIGTERM → 200 ms grace → SIGKILL to every child shell.
    let shutdown_signal = wait_for_shutdown();

    // 13) serve.
    let serve_result = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal)
        .await;

    // Post-shutdown — drop the PtyBackend so its `Drop` impl runs the
    // ADR-0014 D7 teardown step 1 (SIGTERM → grace → SIGKILL fan-out
    // across every pane in parallel). Holding `backend` until here is
    // what keeps the Hub's multiplexer task alive during request
    // draining — releasing it now lets every background thread converge.
    drop(backend);
    drop(hub);

    // Remove the pidfile so `gtmux start` on the next run sees `Absent`
    // instead of `Stale`. Best-effort: a missing pidfile here is fine
    // (operator may have run `gtmux teardown` concurrently).
    if let Some(path) = pidfile_path.as_deref() {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                warn!(path = %path.display(), error = %e, "failed to remove pidfile on shutdown")
            }
        }
    }

    print_farewell(&config.server.session);

    serve_result.context("axum::serve")
}

fn build_app_state(
    config: &Config,
    token: &TokenString,
    hub: Hub,
    layout_path: PathBuf,
    workspace: gtmux_http_api::WorkspaceManager,
    password_hash: Option<String>,
) -> gtmux_http_api::AppState {
    // AppState wires four side-channels into the HTTP router:
    //  * `hub` so `layout_put_handler` broadcasts LAYOUT_CHANGED to WS
    //    subscribers right after a successful PUT.
    //  * `layout_path` so the legacy `/api/layout` endpoint persists per
    //    ADR-0006 (boot-time load + atomic-write swap).
    //  * `workspace` so the multi-session `/api/sessions[/<name>[/layout]]`
    //    routes (ADR-0018 / ADR-0019) start serving — 503 otherwise.
    //  * `password_hash` so password-mode logins (ADR-0020 D5) can verify
    //    against the on-disk Argon2id hash.
    let mut app_state = gtmux_http_api::AppState::with_hub_and_path(
        config.clone(),
        token.clone(),
        hub,
        layout_path,
    )
    .with_workspace(workspace);
    if let Some(h) = password_hash {
        app_state = app_state.with_password_hash(h);
    }
    app_state
}

fn build_router(
    app_state: gtmux_http_api::AppState,
    config: &Config,
    token: &TokenString,
    hub: Hub,
) -> Router {
    let frontend_dist = app_state
        .config
        .frontend_dist
        .as_deref()
        .map(|p| p.to_path_buf());
    let http = gtmux_http_api::router_with_app_state(app_state, frontend_dist.as_deref());
    let ws = gtmux_ws_server::router(config, token, hub);
    http.merge(ws)
}

// ────────────────────────────────────────────────────────────────────────────
// Banner
// ────────────────────────────────────────────────────────────────────────────

/// First-run banner. ADR-0003 D3 + D21 c1. The token is emitted cleartext on
/// stdout *exactly once* — the user is expected to follow the URL, receive an
/// HttpOnly cookie, and bookmark the path-only URL thereafter.
fn print_banner(config: &Config, mode: Mode, token: &TokenString, bound: Option<SocketAddr>) {
    let displayed_addr = bound
        .map(|a| a.to_string())
        .unwrap_or_else(|| format!("{}:{}", config.server.bind, config.server.port));
    let url_host = match bound {
        Some(a) if a.ip().is_unspecified() => format!("127.0.0.1:{}", a.port()),
        Some(a) => a.to_string(),
        None => format!("{}:{}", config.server.bind, config.server.port),
    };
    let token_path = humanise_token_path(&config.server.session);
    let pid_self = std::process::id();

    println!();
    println!("gtmux {} ready", config.server.session);
    println!(
        "  Mode:         {}",
        match mode {
            Mode::Local => "Local",
            Mode::Cloud => "Cloud",
        }
    );
    println!("  Bind:         {}", displayed_addr);
    println!(
        "  Open URL:     http://{}/auth/bootstrap?token={}",
        url_host, token.0
    );
    println!("  Token path:   {} (0600)", token_path);
    println!(
        "  Backend:      PtyBackend (ADR-0013, supervisor pid={})",
        pid_self
    );
    println!();
    println!("Press Ctrl-C to stop. All child shells will be reaped on shutdown.");
    println!();
}

fn print_farewell(session: &str) {
    println!();
    println!(
        "gtmux {session} stopped. All child shells reaped. \
         Run 'gtmux teardown --session {session}' to clean state files."
    );
}

/// `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token`, expanded for
/// display only — we never round-trip the result back into `auth`.
fn humanise_token_path(session: &str) -> String {
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| format!("{h}/.local/state"))
        })
        .unwrap_or_else(|| "$XDG_STATE_HOME".to_string());
    format!("{base}/gtmux/{session}.token")
}

// ────────────────────────────────────────────────────────────────────────────
// Tracing
// ────────────────────────────────────────────────────────────────────────────

fn init_tracing(config: &Config) {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.runtime.log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // log_format = "auto" picks JSON for piped stderr (programmatic
    // consumers — D21 c1) and text + ANSI for an interactive terminal.
    let format = config.runtime.log_format.as_str();
    let want_json = match format {
        "json" => true,
        "text" => false,
        _ => !std::io::stderr().is_terminal(), // "auto" or anything else
    };

    // `try_init` is forgiving when a subscriber is already installed (the
    // integration tests in lifecycle install their own); we never want a
    // double-init panic to take down the CLI.
    let result = if want_json {
        fmt()
            .json()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init()
    } else {
        fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_ansi(std::io::stderr().is_terminal())
            .try_init()
    };
    if let Err(e) = result {
        // Stay silent on debug, surface on error — a subscriber already set
        // up by a test harness is fine. Anything else we want to know about.
        let _ = e;
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Shutdown plumbing
// ────────────────────────────────────────────────────────────────────────────

/// Resolve the future that drives `with_graceful_shutdown`. Returns on either
/// SIGINT (Ctrl-C) or SIGTERM (`kill <pid>`). Each completion path logs the
/// trigger before the daemon-survive guarantee from D21 c5 kicks in.
async fn wait_for_shutdown() {
    // Per-signal handles must be created *before* we race on them. If
    // `signal()` fails we fall back to listening on whichever did succeed.
    let mut term = match signal(SignalKind::terminate()) {
        Ok(s) => Some(s),
        Err(e) => {
            warn!(error = %e, "could not install SIGTERM handler; only SIGINT will trigger shutdown");
            None
        }
    };

    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!(error = %e, "failed to wait for Ctrl-C — shutting down immediately");
        }
    };

    if let Some(term) = term.as_mut() {
        tokio::select! {
            _ = ctrl_c => info!("received SIGINT, shutting down"),
            _ = term.recv() => info!("received SIGTERM, shutting down"),
        }
    } else {
        ctrl_c.await;
        info!("received SIGINT, shutting down");
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Error mapping (anyhow → grill D20 exit code)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum BindError {
    InUse(SocketAddr),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::InUse(addr) => write!(
                f,
                "port {} is already in use on {} — pick another --port or stop the conflicting service",
                addr.port(),
                addr.ip()
            ),
        }
    }
}

impl std::error::Error for BindError {}

/// Friendly start-time failures that don't fit cleanly into `LifecycleError`
/// / `AuthError` / `io::Error`. LIFE-3 adds [`StartError::AlreadyRunning`]
/// so the pidfile liveness gate routes through `EXIT_PORT_IN_USE` with a
/// targeted message ("use `gtmux stop`" rather than the generic "pick
/// another --port").
#[derive(Debug)]
enum StartError {
    AlreadyRunning {
        session: String,
        pid: libc::pid_t,
    },
    /// ADR-0014 D10 amend (2026-05-14) — `TMUX` env detected, refuse to start
    /// inside an outer tmux session. The variant carries the env value so
    /// the diagnostic surfaces the actual offender (e.g. the outer socket
    /// path) and the user can locate / exit it without guessing.
    NestedTmux(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::AlreadyRunning { session, pid } => write!(
                f,
                "gtmux server already running for session '{session}' (pid {pid}). \
                 Use `gtmux stop --session {session}` first, or pick another --session."
            ),
            StartError::NestedTmux(val) => write!(
                f,
                "refusing to start inside an existing tmux session (TMUX env = {val:?}). \
                 Exit the outer tmux first, or run `unset TMUX` then retry. \
                 Spawning child shells under a nested tmux corrupts their environment \
                 (see docs/adr/0014-process-supervisor.md D10)."
            ),
        }
    }
}

impl std::error::Error for StartError {}

/// Print the anyhow chain to stderr and pick the appropriate exit code from
/// the grill D20 matrix. Errors are inspected via `downcast_ref` so callers
/// can attach `.context(...)` freely without breaking the routing.
fn report_start_error(err: anyhow::Error) -> ExitCode {
    // Print full origin chain — first the high-level context, then each
    // wrapped layer. Reducing this to one line would lose the source of the
    // failure on `clippy::all` builds.
    eprintln!("gtmux start: {err:#}");

    if let Some(BindError::InUse(_)) = err.downcast_ref::<BindError>() {
        return ExitCode::from(EXIT_PORT_IN_USE);
    }
    if let Some(start) = err.downcast_ref::<StartError>() {
        return match start {
            StartError::AlreadyRunning { .. } => ExitCode::from(EXIT_PORT_IN_USE),
            // Same family as port-in-use / already-running — the user has
            // *something running* that conflicts and they need to clean it
            // up before retrying. Exit 4 is the matching diagnostic per
            // grill D20 + ADR-0007 D3 + ADR-0014 D10 amend.
            StartError::NestedTmux(_) => ExitCode::from(EXIT_PORT_IN_USE),
        };
    }
    if let Some(sf) = err.downcast_ref::<StateFileError>() {
        return match sf {
            StateFileError::BadXdg(_) => ExitCode::from(EXIT_PERMISSION),
            StateFileError::Io(_) => ExitCode::from(EXIT_FAILURE),
        };
    }
    if let Some(auth) = err.downcast_ref::<AuthError>() {
        return match auth {
            AuthError::BadPerm { .. } => ExitCode::from(EXIT_PERMISSION),
            AuthError::NotFound(_) => ExitCode::from(EXIT_SESSION_MISSING),
            _ => ExitCode::from(EXIT_FAILURE),
        };
    }
    // Also handle the io::Error that often hides under a context layer.
    if let Some(io) = err.downcast_ref::<std::io::Error>() {
        if io.kind() == std::io::ErrorKind::AddrInUse {
            return ExitCode::from(EXIT_PORT_IN_USE);
        }
    }

    ExitCode::from(EXIT_FAILURE)
}

// ────────────────────────────────────────────────────────────────────────────
// `gtmux teardown`  (P0-CLI-3)
// ────────────────────────────────────────────────────────────────────────────

struct TeardownArgs {
    session: String,
    force: bool,
    keep_state: bool,
    keep_config: bool,
}

/// Execute ADR-0014 §D7 four-step cleanup (post-Stage-B). Returns the
/// grill-D20 exit code.
///
/// Confirmation policy: when `force = false` *and* stdin/stderr are a TTY
/// we ask the user to type `yes` before proceeding. On a non-TTY (pipe,
/// CI), we refuse and tell them to re-run with `--force` — there's no
/// other safe option since we can't read from stdin in a non-interactive
/// shell without surprising scripted callers.
async fn teardown_cmd(args: TeardownArgs) -> ExitCode {
    let opts = TeardownOpts {
        force: args.force,
        remove_state_files: !args.keep_state,
        remove_config: !args.keep_config,
    };

    // Pre-flight: is a server still alive (pidfile probe)? When
    // `force = false` we surface the confirmation prompt before SIGTERM
    // touches the process.
    if !opts.force {
        if let Ok(PidLiveness::Alive(pid)) = check_pidfile_liveness(&args.session) {
            if !confirm_teardown(&args.session, pid) {
                return ExitCode::from(EXIT_FAILURE);
            }
        }
        // If the pidfile isn't present / stale, teardown proceeds with
        // stop = NoPidfile / AlreadyDead — confirmation skipped because
        // nothing is at risk of being killed.
    }

    let report = match state_files::teardown(&args.session, opts.clone()).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("gtmux teardown: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };

    print_teardown_report(&args.session, &report, opts.remove_config);

    let unlink_warnings: Vec<&str> = report
        .removed
        .iter()
        .filter_map(|(_, r)| r.as_ref().err().map(String::as_str))
        .collect();
    if unlink_warnings.is_empty() && report.warnings.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_TEARDOWN_PARTIAL)
    }
}

/// Stdin-driven confirmation prompt. Returns `true` when the user typed
/// `yes` (case-insensitive). Non-TTY callers see an instruction line and
/// a `false` return.
fn confirm_teardown(session: &str, pid: libc::pid_t) -> bool {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        eprintln!(
            "gtmux teardown: refusing to proceed without confirmation \
             (server alive at pid {pid}). Re-run with --force."
        );
        return false;
    }
    eprintln!(
        "gtmux teardown will SIGTERM the gtmux Server for session '{session}'\n  \
         pid: {pid}\nContinue? Type 'yes' to confirm: "
    );
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        eprintln!("gtmux teardown: failed to read confirmation; aborting.");
        return false;
    }
    let answer = line.trim().to_ascii_lowercase();
    let ok = answer == "yes" || answer == "y";
    if !ok {
        eprintln!("gtmux teardown: aborted (got {answer:?}, expected 'yes').");
    }
    ok
}

fn print_teardown_report(session: &str, report: &TeardownReport, requested_remove_config: bool) {
    let stop_line = match &report.stop {
        Some(StopOutcome::NoPidfile(_)) => "no pidfile (server was not running)".to_string(),
        Some(StopOutcome::MalformedPidfile(_)) => "malformed pidfile (cleaned)".to_string(),
        Some(StopOutcome::AlreadyDead { pid, .. }) => {
            format!("server already exited (pid {pid})")
        }
        Some(StopOutcome::Stopped { pid, .. }) => format!("server stopped via SIGTERM (pid {pid})"),
        Some(StopOutcome::Killed { pid, .. }) => format!("server killed via SIGKILL (pid {pid})"),
        Some(StopOutcome::TimedOut { pid, .. }) => {
            format!(
                "SIGTERM sent but server did not exit in grace (pid {pid}); re-run with --force"
            )
        }
        None => "(not attempted)".to_string(),
    };

    println!();
    println!("gtmux teardown {session} complete.");
    println!("  Server:              {stop_line}");
    if report.removed.is_empty() {
        println!("  Files removed:       (no state-file unlink attempted)");
    } else {
        println!("  Files:");
        for (kind, result) in &report.removed {
            let line = match result {
                Ok(true) => "removed".to_string(),
                Ok(false) => "(already absent)".to_string(),
                Err(msg) => format!("WARN — {msg}"),
            };
            println!("    {kind:<10} {line}");
        }
    }
    if !requested_remove_config {
        println!("  Config:              (kept — --keep-config)");
    }
    if !report.warnings.is_empty() {
        println!("  Warnings:");
        for w in &report.warnings {
            println!("    - {w}");
        }
    }
    println!();
}

// ────────────────────────────────────────────────────────────────────────────
// `gtmux stop`  (P0-CLI-2 — Sprint 4-D LIFE-3 real wiring)
// ────────────────────────────────────────────────────────────────────────────

/// Stop a running gtmux Server gracefully via the pidfile.
///
/// Exit-code matrix:
///   * `Stopped` / `Killed` / `AlreadyDead` → 0 (success — the operator's
///     mental model is "the server is no longer running", which all three
///     deliver).
///   * `NoPidfile` → 1. The friendly message points at `gtmux teardown`
///     for users who actually wanted to remove tmux state.
///   * `MalformedPidfile` → 1.
///   * `TimedOut` → 6 (lifecycle / tmux-domain failure code in the
///     grill-D20 matrix; the server didn't honour SIGTERM within 5 s).
async fn stop(session: &str, force: bool) -> ExitCode {
    use std::time::Duration;

    // We compute the pidfile path up-front so the friendly error message
    // for `NoPidfile` can mention the exact path operators should look
    // at. Resolution failures here (XDG_STATE_HOME empty, HOME unset)
    // surface as exit 1 with a targeted message.
    let path = match pidfile_path_for(session) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("gtmux stop: cannot resolve pidfile path: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };

    let outcome = match stop_server(session, Duration::from_secs(5), force).await {
        Ok(o) => o,
        Err(e) => {
            eprintln!(
                "gtmux stop: failed to signal server for session '{session}' \
                 (pidfile {}): {e}",
                path.display()
            );
            return ExitCode::from(EXIT_TMUX);
        }
    };

    match outcome {
        StopOutcome::NoPidfile(path) => {
            eprintln!(
                "gtmux stop: no running gtmux server for session '{session}' \
                 (pidfile {} not found).\n\
                 If you intended to remove tmux state, use `gtmux teardown --session {session}`.",
                path.display()
            );
            ExitCode::from(EXIT_FAILURE)
        }
        StopOutcome::MalformedPidfile(path) => {
            eprintln!(
                "gtmux stop: pidfile at {} could not be parsed as a PID; \
                 the corrupt file has been removed.",
                path.display()
            );
            ExitCode::from(EXIT_FAILURE)
        }
        StopOutcome::AlreadyDead { pid, path } => {
            println!(
                "gtmux stop: server (pid {pid}) was already gone; removed stale pidfile {}. \
                 tmux daemon preserved.",
                path.display()
            );
            ExitCode::SUCCESS
        }
        StopOutcome::Stopped { pid, .. } => {
            println!("gtmux stop: server (pid {pid}) stopped gracefully. tmux daemon preserved.");
            ExitCode::SUCCESS
        }
        StopOutcome::Killed { pid, .. } => {
            println!(
                "gtmux stop: server (pid {pid}) did not exit on SIGTERM and was killed (SIGKILL). \
                 tmux daemon preserved, but in-flight layout writes may have been dropped."
            );
            ExitCode::SUCCESS
        }
        StopOutcome::TimedOut { pid, .. } => {
            eprintln!(
                "gtmux stop: server (pid {pid}) didn't exit after SIGTERM (5 s grace). \
                 Re-run with `--force` to escalate to SIGKILL, or `kill -9 {pid}` by hand."
            );
            ExitCode::from(EXIT_TMUX)
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// `gtmux rotate-token`  (P0-CLI-4)
// ────────────────────────────────────────────────────────────────────────────

fn rotate_token_cmd(session: &str) -> ExitCode {
    let fresh = match rotate_token(session) {
        Ok(t) => t,
        Err(AuthError::NotFound(p)) => {
            eprintln!(
                "gtmux rotate-token: no token file for session '{session}' at {}.\n\
                 Has `gtmux start --session {session}` ever run on this host?",
                p.display()
            );
            return ExitCode::from(EXIT_SESSION_MISSING);
        }
        Err(AuthError::BadPerm {
            path,
            expected,
            actual,
        }) => {
            eprintln!(
                "gtmux rotate-token: refusing — {} has perm {:o} (expected {:o}). \
                 Fix the file mode before rotating.",
                path.display(),
                actual,
                expected
            );
            return ExitCode::from(EXIT_PERMISSION);
        }
        Err(e) => {
            eprintln!("gtmux rotate-token: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };

    // Infer host / port from the per-session config file if present — the
    // rotation message is much friendlier with a clickable URL. When the
    // file is missing (first-run after manual file ops, or a non-default
    // config path) we print a generic note instead.
    let url_line = match infer_open_url(session, &fresh) {
        Some(url) => format!("  Open URL:     {url}"),
        None => {
            "  Open URL:     (run `gtmux status --session <name>` for the bound port)".to_string()
        }
    };
    let token_path = humanise_token_path(session);

    println!();
    println!("gtmux {} token rotated.", session);
    println!("  New token:    {}", fresh.0);
    println!("{}", url_line);
    println!("  Token path:   {} (0600)", token_path);
    println!();
    println!(
        "The previous token is now invalid. Any active browser tab will be\n\
         disconnected (close code 4001) and must reconnect using the URL above."
    );
    println!();
    ExitCode::SUCCESS
}

// ────────────────────────────────────────────────────────────────────────────
// `gtmux set-password` / `gtmux reset-password` — ADR-0020 D5
// ────────────────────────────────────────────────────────────────────────────

fn set_password_cmd() -> ExitCode {
    use std::io::IsTerminal;
    let path = match gtmux_http_api::default_password_hash_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("gtmux set-password: cannot resolve hash path: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };

    // Read the password twice. Interactive: rpassword (no echo). Otherwise
    // we read one line from stdin and skip the confirmation (CI pipes / docker
    // exec) — *only* when stdin is genuinely not a TTY so that an operator at
    // a terminal never bypasses the confirmation by accident.
    let (first, confirm) = if std::io::stdin().is_terminal() {
        let p1 = match rpassword::prompt_password("New gtmux password: ") {
            Ok(s) => s,
            Err(e) => {
                eprintln!("gtmux set-password: read failed: {e}");
                return ExitCode::from(EXIT_FAILURE);
            }
        };
        let p2 = match rpassword::prompt_password("Confirm new password: ") {
            Ok(s) => s,
            Err(e) => {
                eprintln!("gtmux set-password: read failed: {e}");
                return ExitCode::from(EXIT_FAILURE);
            }
        };
        (p1, p2)
    } else {
        let mut buf = String::new();
        if let Err(e) = std::io::stdin().read_line(&mut buf) {
            eprintln!("gtmux set-password: stdin read failed: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
        let trimmed = buf.trim_end_matches('\n').to_string();
        (trimmed.clone(), trimmed)
    };

    if first != confirm {
        eprintln!("gtmux set-password: passwords did not match.");
        return ExitCode::from(EXIT_FAILURE);
    }
    if first.len() < 8 {
        eprintln!("gtmux set-password: password must be at least 8 characters (ADR-0020 D5).");
        return ExitCode::from(EXIT_FAILURE);
    }

    let hash = match gtmux_http_api::hash_password(&first) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("gtmux set-password: hash failed: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    if let Err(e) = gtmux_http_api::save_password_hash(&path, &hash) {
        eprintln!("gtmux set-password: write failed: {e}");
        return ExitCode::from(EXIT_FAILURE);
    }
    println!();
    println!("gtmux password saved at {} (mode 0600).", path.display());
    println!("Switch `[auth] mode = \"password\"` in your config and restart `gtmux start`.");
    println!();
    ExitCode::SUCCESS
}

fn reset_password_cmd() -> ExitCode {
    let path = match gtmux_http_api::default_password_hash_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("gtmux reset-password: cannot resolve hash path: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    match std::fs::remove_file(&path) {
        Ok(()) => {
            println!("gtmux: removed password hash at {}.", path.display());
            ExitCode::SUCCESS
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "gtmux reset-password: no password set ({} is absent).",
                path.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!(
                "gtmux reset-password: failed to remove {}: {e}",
                path.display()
            );
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Best-effort: peek into `${XDG_CONFIG_HOME}/gtmux/<session>.config.toml`
/// for the bound host + port and build a clickable URL. We don't go through
/// the full figment chain because rotate-token is offline-tolerant — env
/// overrides and CLI flags don't apply here.
fn infer_open_url(session: &str, token: &TokenString) -> Option<String> {
    let cfg_path = config_dir_for_humanise()?.join(format!("{session}.config.toml"));
    let raw = std::fs::read_to_string(&cfg_path).ok()?;
    // Cheap regex-free parse: walk lines, capture `bind = "..."` and
    // `port = NNNN`. Anything else is ignored.
    let mut bind: Option<String> = None;
    let mut port: Option<u16> = None;
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("bind") {
            let rest = rest.trim_start_matches([' ', '=']).trim();
            let rest = rest.trim_matches('"');
            if !rest.is_empty() {
                bind = Some(rest.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("port") {
            let rest = rest.trim_start_matches([' ', '=']).trim();
            if let Ok(n) = rest.parse::<u16>() {
                port = Some(n);
            }
        }
    }
    let host = match bind.as_deref() {
        Some("0.0.0.0") | Some("::") | None => "127.0.0.1".to_string(),
        Some(other) => other.to_string(),
    };
    let port = port?;
    Some(format!(
        "http://{host}:{port}/auth/bootstrap?token={}",
        token.0
    ))
}

fn config_dir_for_humanise() -> Option<PathBuf> {
    if let Some(s) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(s);
        if !p.as_os_str().is_empty() {
            return Some(p.join("gtmux"));
        }
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config").join("gtmux"))
}

// ────────────────────────────────────────────────────────────────────────────
// `gtmux status`  (P0-CLI-5)
// ────────────────────────────────────────────────────────────────────────────

async fn status_cmd(filter: Option<&str>) -> ExitCode {
    let state_dir = match status_state_dir() {
        Some(d) => d,
        None => {
            eprintln!("gtmux status: cannot resolve XDG_STATE_HOME (and $HOME is unset).");
            return ExitCode::from(EXIT_FAILURE);
        }
    };

    let sessions = match enumerate_sessions(&state_dir, filter) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("gtmux status: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };

    if sessions.is_empty() {
        if let Some(name) = filter {
            println!(
                "gtmux status: no session '{name}' under {}",
                state_dir.display()
            );
        } else {
            println!(
                "gtmux status: no gtmux sessions found under {} \
                 (no .token files).",
                state_dir.display()
            );
        }
        return ExitCode::SUCCESS;
    }

    // Render a fixed-width table. We don't depend on a table crate to keep
    // the CLI binary lean — the column widths cover the common case (short
    // session names; full paths abbreviated on a single line).
    println!(
        "{:<14}{:<28}{:<32}{:<12}{:<10}",
        "SESSION", "SERVER", "PIDFILE", "TOKEN", "CONFIG"
    );
    for s in sessions {
        let row = describe_session(&s).await;
        println!(
            "{:<14}{:<28}{:<32}{:<12}{:<10}",
            truncate(&row.session, 14),
            truncate(&row.server, 28),
            truncate(&row.pidfile, 32),
            row.token,
            row.config,
        );
    }
    ExitCode::SUCCESS
}

fn status_state_dir() -> Option<PathBuf> {
    if let Some(s) = std::env::var_os("XDG_STATE_HOME") {
        let p = PathBuf::from(s);
        if !p.as_os_str().is_empty() {
            return Some(p.join("gtmux"));
        }
    }
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("gtmux"),
    )
}

fn enumerate_sessions(
    state_dir: &std::path::Path,
    filter: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    if !state_dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in
        std::fs::read_dir(state_dir).with_context(|| format!("reading {}", state_dir.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        // We only care about `.token` files — the canonical existence
        // marker for "this session has ever been started".
        let Some(session_name) = name_str.strip_suffix(".token") else {
            continue;
        };
        if let Some(f) = filter {
            if session_name != f {
                continue;
            }
        }
        out.push(session_name.to_string());
    }
    out.sort();
    Ok(out)
}

struct StatusRow {
    session: String,
    server: String,
    pidfile: String,
    token: String,
    config: String,
}

async fn describe_session(session: &str) -> StatusRow {
    // Server liveness — pidfile probe (replaces the pre-Stage-B
    // `tmux has-session` socket probe).
    let (server, pidfile_display) = match check_pidfile_liveness(session) {
        Ok(PidLiveness::Alive(pid)) => (
            format!("running (pid {pid})"),
            match pidfile_path_for(session) {
                Ok(p) => p.display().to_string(),
                Err(_) => "(unresolved)".to_string(),
            },
        ),
        Ok(PidLiveness::Stale(pid)) => (
            format!("stale pidfile (pid {pid}, not alive)"),
            match pidfile_path_for(session) {
                Ok(p) => p.display().to_string(),
                Err(_) => "(unresolved)".to_string(),
            },
        ),
        Ok(PidLiveness::Malformed) => (
            "malformed pidfile".to_string(),
            match pidfile_path_for(session) {
                Ok(p) => p.display().to_string(),
                Err(_) => "(unresolved)".to_string(),
            },
        ),
        Ok(PidLiveness::Absent) => ("stopped".to_string(), "(absent)".to_string()),
        Err(e) => (format!("probe error: {e}"), "(error)".to_string()),
    };

    let token = match check_token_perm(session) {
        TokenStatus::Ok => "ok".to_string(),
        TokenStatus::BadPerm => "bad-perm".to_string(),
        TokenStatus::Missing => "missing".to_string(),
    };

    let config = match config_dir_for_humanise().map(|d| d.join(format!("{session}.config.toml"))) {
        Some(p) if p.exists() => "ok".to_string(),
        Some(_) => "missing".to_string(),
        None => "unknown".to_string(),
    };

    StatusRow {
        session: session.to_string(),
        server,
        pidfile: pidfile_display,
        token,
        config,
    }
}

enum TokenStatus {
    Ok,
    BadPerm,
    Missing,
}

fn check_token_perm(session: &str) -> TokenStatus {
    let Some(state_dir) = status_state_dir() else {
        return TokenStatus::Missing;
    };
    let token_path = state_dir.join(format!("{session}.token"));
    let Ok(meta) = std::fs::metadata(&token_path) else {
        return TokenStatus::Missing;
    };
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode() & 0o777;
    if mode == 0o600 {
        TokenStatus::Ok
    } else {
        TokenStatus::BadPerm
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    // Keep the tail (paths) since the head (XDG prefix) is repeated across
    // rows — operators glance at the *end* to disambiguate.
    let kept: String = s.chars().rev().take(max.saturating_sub(1)).collect();
    let mut out: String = kept.chars().rev().collect();
    out.insert(0, '…');
    out
}

// ────────────────────────────────────────────────────────────────────────────
// Self-tests — argument parsing only. End-to-end (spawn → bind → curl) lives
// in the C4 smoke harness (`codebase/smoke/01_engine_connect.sh`).
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_verifies() {
        // clap's internal sanity check — catches conflicting arg shapes.
        Cli::command().debug_assert();
    }

    #[test]
    fn start_parses_minimum_args() {
        let cli = Cli::parse_from(["gtmux", "start", "--session", "alpha"]);
        match cli.command {
            Cmd::Start {
                session,
                port,
                config_path,
                workspace_path,
            } => {
                assert_eq!(session, "alpha");
                assert!(port.is_none());
                assert!(config_path.is_none());
                assert!(workspace_path.is_none());
            }
            other => panic!("expected Start, got {other:?}"),
        }
    }

    #[test]
    fn start_parses_all_args() {
        let cli = Cli::parse_from([
            "gtmux",
            "start",
            "--session",
            "beta",
            "--port",
            "9999",
            "--config",
            "/tmp/x.toml",
            "--workspace",
            "/tmp/ws",
        ]);
        match cli.command {
            Cmd::Start {
                session,
                port,
                config_path,
                workspace_path,
            } => {
                assert_eq!(session, "beta");
                assert_eq!(port, Some(9999));
                assert_eq!(
                    config_path.as_deref(),
                    Some(std::path::Path::new("/tmp/x.toml"))
                );
                assert_eq!(
                    workspace_path.as_deref(),
                    Some(std::path::Path::new("/tmp/ws"))
                );
            }
            other => panic!("expected Start, got {other:?}"),
        }
    }

    #[test]
    fn humanise_token_path_uses_xdg_state_home() {
        // The function must not panic on common envs; we don't assert exact
        // paths because tests may run with any HOME / XDG_STATE_HOME shape.
        let p = humanise_token_path("smoke");
        assert!(p.ends_with("gtmux/smoke.token"));
    }

    #[test]
    fn bind_error_in_use_message_mentions_port() {
        let e = BindError::InUse(SocketAddr::from(([127, 0, 0, 1], 9001)));
        let msg = e.to_string();
        assert!(msg.contains("9001"));
        assert!(msg.contains("already in use"));
    }

    #[test]
    fn teardown_parses_flags() {
        let cli = Cli::parse_from([
            "gtmux",
            "teardown",
            "--session",
            "smoke",
            "--force",
            "--keep-state",
            "--keep-config",
        ]);
        match cli.command {
            Cmd::Teardown {
                session,
                force,
                keep_state,
                keep_config,
            } => {
                assert_eq!(session, "smoke");
                assert!(force);
                assert!(keep_state);
                assert!(keep_config);
            }
            other => panic!("expected Teardown, got {other:?}"),
        }
    }

    #[test]
    fn teardown_defaults_to_no_force() {
        let cli = Cli::parse_from(["gtmux", "teardown", "--session", "x"]);
        match cli.command {
            Cmd::Teardown {
                force,
                keep_state,
                keep_config,
                ..
            } => {
                assert!(!force);
                assert!(!keep_state);
                assert!(!keep_config);
            }
            other => panic!("expected Teardown, got {other:?}"),
        }
    }

    #[test]
    fn status_accepts_optional_session() {
        let cli = Cli::parse_from(["gtmux", "status"]);
        match cli.command {
            Cmd::Status { session } => assert!(session.is_none()),
            other => panic!("expected Status, got {other:?}"),
        }
        let cli = Cli::parse_from(["gtmux", "status", "--session", "smoke"]);
        match cli.command {
            Cmd::Status { session } => assert_eq!(session.as_deref(), Some("smoke")),
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn rotate_token_requires_session() {
        let cli = Cli::parse_from(["gtmux", "rotate-token", "--session", "smoke"]);
        match cli.command {
            Cmd::RotateToken { session } => assert_eq!(session, "smoke"),
            other => panic!("expected RotateToken, got {other:?}"),
        }
    }

    #[test]
    fn stop_parses() {
        let cli = Cli::parse_from(["gtmux", "stop", "--session", "smoke"]);
        match cli.command {
            Cmd::Stop { session, force } => {
                assert_eq!(session, "smoke");
                assert!(!force, "force defaults to false");
            }
            other => panic!("expected Stop, got {other:?}"),
        }
    }

    #[test]
    fn stop_parses_force_flag() {
        let cli = Cli::parse_from(["gtmux", "stop", "--session", "smoke", "--force"]);
        match cli.command {
            Cmd::Stop { session, force } => {
                assert_eq!(session, "smoke");
                assert!(force, "--force must propagate");
            }
            other => panic!("expected Stop, got {other:?}"),
        }
    }

    #[test]
    fn truncate_keeps_tail() {
        assert_eq!(truncate("abcdef", 10), "abcdef");
        // Longer than max → leading ellipsis + last (max-1) chars.
        let t = truncate("abcdefghij", 5);
        assert_eq!(t.chars().count(), 5);
        assert!(t.starts_with('…'));
        assert!(t.ends_with("ghij"));
    }

    // ────────────────────────────────────────────────────────────────────
    // Sprint 4-D LIFE-3 — `gtmux stop` + start-time pidfile gate.
    //
    // These tests mutate `XDG_STATE_HOME` so they must serialise against
    // each other. The lock is per-binary; concurrent test binaries in
    // `cargo test --workspace` are independent processes and don't share
    // this lock — they each have their own env. The actual cross-crate
    // race surface (gtmux-lifecycle + gtmux-auth + gtmux-cli all touching
    // XDG_STATE_HOME) is mediated by `cargo test` running each crate's
    // tests in its own process by default.
    // ────────────────────────────────────────────────────────────────────

    use std::sync::Mutex;

    static CLI_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct CliXdgGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev_state: Option<std::ffi::OsString>,
        prev_home: Option<std::ffi::OsString>,
        _state_dir: tempfile::TempDir,
        _home_dir: tempfile::TempDir,
    }

    impl CliXdgGuard {
        fn new() -> Self {
            let lock = CLI_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev_state = std::env::var_os("XDG_STATE_HOME");
            let prev_home = std::env::var_os("HOME");
            let state_dir = tempfile::tempdir().expect("state tempdir");
            let home_dir = tempfile::tempdir().expect("home tempdir");
            std::env::set_var("XDG_STATE_HOME", state_dir.path());
            std::env::set_var("HOME", home_dir.path());
            Self {
                _lock: lock,
                prev_state,
                prev_home,
                _state_dir: state_dir,
                _home_dir: home_dir,
            }
        }
    }

    impl Drop for CliXdgGuard {
        fn drop(&mut self) {
            match &self.prev_state {
                Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
            match &self.prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    /// `gtmux stop` against a session that has never been started must
    /// exit 1 with a friendly message that points at `gtmux teardown` as
    /// the alternative path.
    #[tokio::test]
    async fn stop_missing_pidfile_friendly_error() {
        let _g = CliXdgGuard::new();
        let code = stop("never-existed", false).await;
        // `ExitCode` doesn't expose its numeric value through a stable
        // API; reconstruct one with the same byte and compare with the
        // `Debug` output, which prints `ExitCode(unix_exit_status(N))`
        // on Unix. That formatter is the only public observation point.
        let want = format!("{:?}", ExitCode::from(EXIT_FAILURE));
        let got = format!("{code:?}");
        assert_eq!(got, want, "expected EXIT_FAILURE, got {got}");
    }

    /// `gtmux stop` against a stale pidfile (PID guaranteed not to
    /// exist) must succeed (exit 0) and remove the stale file so the
    /// next `gtmux start` sees a clean `Absent` state.
    #[tokio::test]
    async fn stop_stale_pidfile_succeeds() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let _g = CliXdgGuard::new();
        let session = "stale-cli";
        let path = pidfile_path_for(session).expect("path");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::set_permissions(
            path.parent().unwrap(),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{}", libc::pid_t::MAX).unwrap();
        drop(f);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let code = stop(session, false).await;
        let want = format!("{:?}", ExitCode::SUCCESS);
        let got = format!("{code:?}");
        assert_eq!(got, want, "expected EXIT_SUCCESS, got {got}");
        assert!(!path.exists(), "stale pidfile must be cleaned up");
    }

    /// Pidfile liveness gate at start: if a pidfile exists and points at
    /// a live PID (we use our own), `check_pidfile_liveness` reports
    /// `Alive` and `Cmd::Start` would return `StartError::AlreadyRunning`.
    /// We unit-test the gate at the lifecycle boundary because the full
    /// `start` path requires a real tmux daemon + bind, which is the
    /// smoke harness's job.
    #[test]
    fn start_rejects_when_pidfile_alive() {
        let _g = CliXdgGuard::new();
        let session = "alive-cli";
        let path = write_pidfile(session).expect("write_pidfile");
        assert!(path.exists());
        match check_pidfile_liveness(session).expect("liveness") {
            PidLiveness::Alive(pid) => {
                assert_eq!(pid as u32, std::process::id());
                // Round-trip the user-visible error message so the
                // friendly text doesn't drift silently.
                let err = StartError::AlreadyRunning {
                    session: session.to_string(),
                    pid,
                };
                let msg = err.to_string();
                assert!(msg.contains("already running"), "got: {msg}");
                assert!(msg.contains("gtmux stop"), "got: {msg}");
                assert!(msg.contains(session), "got: {msg}");
            }
            other => panic!("expected Alive(self), got {other:?}"),
        }
    }
}
