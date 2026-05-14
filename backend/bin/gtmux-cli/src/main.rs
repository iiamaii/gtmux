//! gtmux CLI — clap derive entrypoint (D20 subcommand set).
//!
//! `start` is wired end-to-end per `docs/reports/0012-bootstrap-smoke.md` §3
//! P0-CLI-1: pidfile liveness probe → config load → daemon spawn → pidfile
//! write → token issue/load (mode-branched per ADR-0003 D13.1) → axum router
//! mount → bind → first-run banner (D21 c1 + ADR-0003 D3 token URL) →
//! graceful shutdown that leaves the tmux daemon alive (ADR-0009 D5 / D21 c5).
//!
//! `teardown` / `rotate-token` / `status` are wired per Sprint 2 P0-CLI-3/4/5
//! (bootstrap-smoke report §3.1): teardown drives `lifecycle::teardown` and
//! reports the five-step outcome; rotate-token calls `auth::rotate_token`
//! and prints a banner that supersedes the prior URL; status enumerates
//! `${XDG_STATE_HOME}/gtmux/*.token` and probes each daemon via `has-session`.
//!
//! Sprint 4-D LIFE-3 turns `stop` from an informational stub into a real
//! graceful-shutdown channel: `gtmux start` writes a pidfile at
//! `${XDG_STATE_HOME}/gtmux/<session>.pid` immediately after binding the
//! TCP listener, and `gtmux stop --session <name>` reads that PID, sends
//! SIGTERM, and waits up to 5 s for the process to exit (escalating to
//! SIGKILL only when `--force` is passed). ADR-0009 D5 invariant is
//! preserved — the tmux daemon survives, `gtmux teardown` remains the
//! single destructive cleanup.

#![forbid(unsafe_code)]
#![warn(clippy::all)]

use std::io::IsTerminal;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use axum::Router;
use clap::{Parser, Subcommand};
use gtmux_auth::{issue_token, load_token, rotate_token, save_token, AuthError, TokenString};
use gtmux_config::{derive_mode, load as load_config, Config, Mode};
use gtmux_lifecycle::{
    check_pidfile_liveness, pidfile_path_for, run_command_loop, run_event_loop, socket_path_for,
    stop_server, write_pidfile, LifecycleError, PidLiveness, SpawnOptions, StopOutcome,
    TeardownOpts, TeardownReport, TmuxDaemon,
};
use gtmux_ws_server::{Hub, TmuxRequest};
use tokio::net::TcpListener;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{mpsc, Mutex};
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
        } => match rt.block_on(start(StartArgs {
            session,
            port,
            config_path,
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
    }
}

// ────────────────────────────────────────────────────────────────────────────
// `gtmux start`
// ────────────────────────────────────────────────────────────────────────────

struct StartArgs {
    session: String,
    port: Option<u16>,
    config_path: Option<PathBuf>,
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
    // 2) config — figment chain. The CLI `session` flag overrides the TOML
    //    `[server].session` (config crate spec). When `--port` was provided on
    //    the CLI we apply it after load so ADR-0007 D2's "immutable after
    //    bind" stays intact: we only mutate `server.port` *before* the first
    //    listener call.
    let mut config =
        load_config(args.config_path.as_deref(), &args.session).context("loading gtmux config")?;
    if let Some(p) = args.port {
        // Honour CLI override exactly once — `bind` itself is not overridable
        // here because the security mode (D22) flips on its value.
        config.server.port = p;
    }

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

    // 5) tmux daemon — spawn (auto-attaches if session already exists via
    //    `new-session -A`, satisfying D21 c2 port-based reattach). Session
    //    is *not* auto-created beyond this argv-driven attach: any error other
    //    than "already exists" propagates and maps to exit 3 / 6 below.
    let daemon = TmuxDaemon::spawn(SpawnOptions {
        session_name: config.server.session.clone(),
        socket_dir: None,
    })
    .await
    .context("spawning dedicated tmux daemon")?;

    let socket_path = daemon.socket_path().to_path_buf();
    info!(socket = %socket_path.display(), "tmux daemon ready");

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
                // First start in cloud mode — generate + persist; explicit
                // rotation is the *only* path that replaces this thereafter.
                let t = issue_token().context("issuing cloud-mode token")?;
                save_token(&config.server.session, &t).context("persisting cloud-mode token")?;
                t
            }
            Err(e) => return Err(e).context("loading cloud-mode token"),
        },
    };

    // 7+8+9) router — HTTP API (layout, bootstrap, healthz) + WebSocket (/ws).
    //   The two routers share Origin/Host invariants but have independent
    //   middleware stacks (cookie/Bearer for HTTP, subprotocol for WS).
    //
    //   Sprint 4-B wiring: a shared [`Hub`] fans tmux events to all WS
    //   subscribers, and a single-writer mpsc channel routes user-origin
    //   commands back to the daemon. Both halves of the daemon
    //   (`read_line` + `write_line`) are guarded by an `Arc<Mutex<…>>`
    //   because the underlying stdio pair can not be split independently.
    //   SAFETY/CORRECTNESS NOTE: the mutex is held only across one read or
    //   one write — never across an `await` that depends on the other half
    //   — so the two background tasks cannot deadlock each other.
    let hub = Hub::new();
    let (cmd_tx, cmd_rx) = mpsc::channel::<TmuxRequest>(64);
    let daemon_arc = Arc::new(Mutex::new(daemon));
    // `config.frontend_dist` opts the bundled SPA in when set; smoke tests
    // set `GTMUX_FRONTEND_DIST` env (figment converts to the config field) so
    // a single port serves both the API and the static UI.
    let app = build_app(
        &config,
        &token,
        hub.clone(),
        cmd_tx,
        config.frontend_dist.as_deref(),
    );
    let event_loop_handle = tokio::spawn({
        let daemon = Arc::clone(&daemon_arc);
        let hub = hub.clone();
        async move {
            if let Err(e) = run_event_loop(daemon, hub).await {
                warn!(error = %e, "tmux event loop exited with error");
            }
        }
    });
    let command_loop_handle = tokio::spawn({
        let daemon = Arc::clone(&daemon_arc);
        async move {
            if let Err(e) = run_command_loop(daemon, cmd_rx).await {
                warn!(error = %e, "tmux command loop exited with error");
            }
        }
    });

    // 10) bind — TCP only for now (unix socket variant is a planned alt-path
    //    that lives behind `bind = "unix:/..."`; we surface a friendly error
    //    rather than half-implementing it).
    if config.server.bind.starts_with("unix:") {
        return Err(anyhow!(
            "unix-socket bind ({}) is not yet wired in P0-CLI-1; \
             use a loopback IP for now",
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
            // Best-effort cleanup of resources we already claimed before
            // surfacing the friendly exit-4 error.
            event_loop_handle.abort();
            command_loop_handle.abort();
            return Err(BindError::InUse(addr).into());
        }
        Err(e) => {
            event_loop_handle.abort();
            command_loop_handle.abort();
            return Err(anyhow::Error::new(e).context(format!("binding {addr}")));
        }
    };

    // 10a) pidfile — write *after* the bind succeeds so a duplicate-bind
    //      attempt (caught at step 10) doesn't leave a misleading pidfile
    //      pointing at a process that never actually held the port. The
    //      pidfile is the in-band channel `gtmux stop` uses to deliver
    //      SIGTERM (LIFE-3); ADR-0009 D5 keeps this as a *server-only*
    //      handle — daemon kill is teardown's job.
    let pidfile_path = match write_pidfile(&config.server.session) {
        Ok(p) => Some(p),
        Err(e) => {
            // Pidfile write failure is non-fatal: the server can still run,
            // but `gtmux stop` will print a friendly "no pidfile" error
            // until the operator's environment is fixed (perm on
            // ${XDG_STATE_HOME}/gtmux/, etc.). We log at WARN so the
            // anomaly surfaces in stderr / journald.
            warn!(error = %e, "failed to write gtmux pidfile; `gtmux stop` will be unavailable for this run");
            None
        }
    };

    // 11) banner — D21 c1 + ADR-0003 D3. We emit the cleartext token URL
    //    exactly once; subsequent traffic must use Authorization: Bearer or
    //    the WebSocket subprotocol (R(rej)2).
    print_banner(
        &config,
        mode,
        &token,
        &socket_path,
        listener.local_addr().ok(),
    );

    // 12) shutdown — install both SIGINT (Ctrl-C) and SIGTERM listeners. The
    //    graceful shutdown future ends when *either* fires; axum then drains
    //    in-flight requests. ADR-0009 D5 / D21 c5: the daemon stays alive.
    let shutdown_signal = wait_for_shutdown();

    // 13) serve.
    let serve_result = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal)
        .await;

    // Post-shutdown — stop the background loops (they hold the daemon mutex
    // and would otherwise prevent `take()`), then drop our control-mode
    // client (daemon survives — ADR-0009 D5 / D21 c5). The farewell banner
    // mirrors the start banner so the user immediately sees that the
    // daemon is *intentionally* still around.
    event_loop_handle.abort();
    command_loop_handle.abort();
    let _ = event_loop_handle.await;
    let _ = command_loop_handle.await;
    {
        let mut guard = daemon_arc.lock().await;
        if let Err(e) = guard.shutdown().await {
            warn!(error = %e, "control-mode client shutdown reported an error");
        }
    }

    // Remove the pidfile so `gtmux start` on the next run sees `Absent`
    // instead of `Stale`. Best-effort: a missing pidfile here is fine
    // (operator may have run `gtmux teardown` concurrently), and any
    // other error is logged but does not affect the exit status.
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

fn build_app(
    config: &Config,
    token: &TokenString,
    hub: Hub,
    cmd_tx: mpsc::Sender<TmuxRequest>,
    frontend_dist: Option<&std::path::Path>,
) -> Router {
    gtmux_http_api::router_with_static(config, token, frontend_dist)
        .merge(gtmux_ws_server::router(config, token, hub, cmd_tx))
}

// ────────────────────────────────────────────────────────────────────────────
// Banner
// ────────────────────────────────────────────────────────────────────────────

/// First-run banner. ADR-0003 D3 + D21 c1. The token is emitted cleartext on
/// stdout *exactly once* — the user is expected to follow the URL, receive an
/// HttpOnly cookie, and bookmark the path-only URL thereafter.
fn print_banner(
    config: &Config,
    mode: Mode,
    token: &TokenString,
    socket_path: &std::path::Path,
    bound: Option<SocketAddr>,
) {
    // Choose the user-facing host: 0.0.0.0 / :: bind to all interfaces but
    // the URL must point somewhere clickable. We prefer the bound address
    // when available and fall back to the configured host name.
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
    println!("  Tmux socket:  {}", socket_path.display());
    println!(
        "  Tmux daemon:  detached (label gtmux-{}), gtmux pid={}",
        config.server.session, pid_self
    );
    println!();
    println!("Press Ctrl-C to stop. tmux daemon will continue running.");
    println!();
}

fn print_farewell(session: &str) {
    println!();
    println!(
        "gtmux stopped. tmux daemon (gtmux-{}) still running. \
         Run 'gtmux teardown --session {}' to remove.",
        session, session
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
    AlreadyRunning { session: String, pid: libc::pid_t },
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::AlreadyRunning { session, pid } => write!(
                f,
                "gtmux server already running for session '{session}' (pid {pid}). \
                 Use `gtmux stop --session {session}` first, or pick another --session."
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
    if let Some(StartError::AlreadyRunning { .. }) = err.downcast_ref::<StartError>() {
        return ExitCode::from(EXIT_PORT_IN_USE);
    }
    if let Some(life) = err.downcast_ref::<LifecycleError>() {
        return match life {
            LifecycleError::TmuxNotFound => {
                eprintln!(
                    "gtmux start: install tmux 3.4+ (https://github.com/tmux/tmux) and retry"
                );
                ExitCode::from(EXIT_TMUX)
            }
            LifecycleError::SessionAlreadyExists(_) => ExitCode::from(EXIT_SESSION_MISSING),
            LifecycleError::SocketBusy(_) | LifecycleError::ProtocolError => {
                ExitCode::from(EXIT_TMUX)
            }
            LifecycleError::TmuxSpawn(_) | LifecycleError::BadXdg(_) | LifecycleError::Io(_) => {
                ExitCode::from(EXIT_TMUX)
            }
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

/// Execute ADR-0009 §D6 five-step cleanup. Returns the grill-D20 exit code.
///
/// Confirmation policy: when `force = false` *and* stderr is a TTY we ask
/// the user to type `yes` before proceeding. On a non-TTY (pipe, CI), we
/// refuse and tell them to re-run with `--force` — there's no other safe
/// option since we can't read from stdin in a non-interactive shell
/// without surprising scripted callers.
async fn teardown_cmd(args: TeardownArgs) -> ExitCode {
    let opts = TeardownOpts {
        force: args.force,
        remove_state_files: !args.keep_state,
        remove_config: !args.keep_config,
    };

    // Cheap pre-flight: is the daemon alive? When `force = false` we want
    // to surface the confirmation banner *before* lifecycle::teardown
    // would refuse with SocketBusy. The probe goes through the public
    // socket helper so the path resolution stays in one place.
    if !opts.force {
        if let Ok(socket_path) = socket_path_for(&args.session) {
            if socket_path.exists() && !confirm_teardown(&args.session, &socket_path) {
                return ExitCode::from(EXIT_FAILURE);
            }
        }
        // If the socket isn't there we still don't enter --force mode —
        // teardown will gracefully proceed with daemon_killed = false.
    }

    // First attempt. When the daemon is alive and force=false this errors
    // with SocketBusy — we surface a clear instruction instead of the raw
    // anyhow chain.
    let report = match gtmux_lifecycle::teardown(&args.session, opts.clone()).await {
        Ok(r) => r,
        Err(LifecycleError::SocketBusy(path)) => {
            eprintln!(
                "gtmux teardown: tmux daemon is still alive on {}.\n\
                 Re-run with --force to kill it, or stop the gtmux Server first.",
                path.display()
            );
            return ExitCode::from(EXIT_TMUX);
        }
        Err(LifecycleError::TmuxNotFound) => {
            eprintln!(
                "gtmux teardown: tmux binary not found on PATH. \
                 Install tmux 3.4+ to run cleanup; state and config files \
                 will only be removed if you re-run after installing."
            );
            return ExitCode::from(EXIT_TMUX);
        }
        Err(e) => {
            eprintln!("gtmux teardown: {e}");
            return ExitCode::from(EXIT_TMUX);
        }
    };

    print_teardown_report(&args.session, &report, opts.remove_config);

    // grill D20 exit-7 maps to "partial failure" — any warning surfaces here
    // so the operator's automation can tell apart "clean teardown" from
    // "some artefacts may need a manual look".
    if report.warnings.is_empty() {
        ExitCode::SUCCESS
    } else if has_only_benign_warnings(&report) {
        // Common case: daemon was already dead + socket was already missing.
        // That's the steady-state outcome of a repeat teardown and not a
        // partial failure. Exit 0 keeps automation idempotent.
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_TEARDOWN_PARTIAL)
    }
}

/// Stdin-driven confirmation prompt. Returns `true` when the user typed a
/// case-insensitive `yes`. Non-TTY callers see an instruction line and a
/// `false` return so the caller can exit 1 without ambiguity.
fn confirm_teardown(session: &str, socket_path: &std::path::Path) -> bool {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        eprintln!(
            "gtmux teardown: refusing to proceed without confirmation \
             (daemon alive at {}). Re-run with --force.",
            socket_path.display()
        );
        return false;
    }
    eprintln!(
        "gtmux teardown will kill tmux daemon for session '{}'\n  \
         socket: {}\nContinue? Type 'yes' to confirm: ",
        session,
        socket_path.display()
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

/// Reportable warnings that don't merit an exit-7 escalation: idempotent
/// re-run of teardown when nothing was alive in the first place. We list
/// the exact strings so the predicate stays grep-able from this file.
fn has_only_benign_warnings(report: &TeardownReport) -> bool {
    const BENIGN: &[&str] = &[
        "tmux daemon already dead",
        "socket already missing",
        "config file not found",
        "config kept",
        "state files kept",
    ];
    report
        .warnings
        .iter()
        .all(|w| BENIGN.iter().any(|b| w.contains(b)) || w.starts_with("pid not present"))
}

fn print_teardown_report(session: &str, report: &TeardownReport, requested_remove_config: bool) {
    let daemon_line = if report.daemon_killed {
        "yes".to_string()
    } else {
        "already dead".to_string()
    };
    let socket_line = if report.socket_removed {
        format!("removed ({})", socket_path_display(session))
    } else {
        "(already absent)".to_string()
    };
    let state_line = if report.state_files_removed.is_empty() {
        "(none removed)".to_string()
    } else {
        report
            .state_files_removed
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let config_line = match (&report.config_removed, requested_remove_config) {
        (Some(p), _) => p.display().to_string(),
        (None, false) => "(kept)".to_string(),
        (None, true) => "(none)".to_string(),
    };

    println!();
    println!("gtmux teardown {} complete.", session);
    println!("  tmux daemon killed:  {}", daemon_line);
    println!("  Socket removed:      {}", socket_line);
    println!("  State files removed: {}", state_line);
    println!("  Config removed:      {}", config_line);
    println!("  Warnings:            {}", report.warnings.len());
    for w in &report.warnings {
        println!("    - {w}");
    }
    println!();
}

fn socket_path_display(session: &str) -> String {
    socket_path_for(session)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| format!("<unresolved>/{session}.sock"))
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
        "{:<14}{:<10}{:<32}{:<12}{:<10}",
        "SESSION", "DAEMON", "SOCKET", "TOKEN", "CONFIG"
    );
    for s in sessions {
        let row = describe_session(&s).await;
        println!(
            "{:<14}{:<10}{:<32}{:<12}{:<10}",
            truncate(&row.session, 14),
            row.daemon,
            truncate(&row.socket, 32),
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
    daemon: String,
    socket: String,
    token: String,
    config: String,
}

async fn describe_session(session: &str) -> StatusRow {
    // Daemon + socket — probe via the lifecycle helper.
    let socket_path = socket_path_for(session).ok();
    let (daemon, socket_display) = match &socket_path {
        Some(path) if path.exists() => match probe_daemon(path).await {
            true => ("running".to_string(), path.display().to_string()),
            false => ("stopped".to_string(), format!("(stale) {}", path.display())),
        },
        Some(path) => (
            "absent".to_string(),
            format!("(missing) {}", path.display()),
        ),
        None => ("unknown".to_string(), "(unresolved)".to_string()),
    };

    // Token — perm gate first, then existence.
    let token = match check_token_perm(session) {
        TokenStatus::Ok => "ok".to_string(),
        TokenStatus::BadPerm => "bad-perm".to_string(),
        TokenStatus::Missing => "missing".to_string(),
    };

    // Config — presence only. We don't validate schema here because a
    // status command must not depend on a working figment chain.
    let config = match config_dir_for_humanise().map(|d| d.join(format!("{session}.config.toml"))) {
        Some(p) if p.exists() => "ok".to_string(),
        Some(_) => "missing".to_string(),
        None => "unknown".to_string(),
    };

    StatusRow {
        session: session.to_string(),
        daemon,
        socket: socket_display,
        token,
        config,
    }
}

async fn probe_daemon(socket_path: &std::path::Path) -> bool {
    // Run `tmux -L gtmux-<session> -S <socket> has-session -t <session>`.
    // We derive the session name from the socket file name to keep the
    // probe self-contained. Exit code 0 means alive.
    let session = socket_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let label = format!("gtmux-{session}");
    let status = tokio::process::Command::new("tmux")
        .arg("-L")
        .arg(&label)
        .arg("-S")
        .arg(socket_path)
        .arg("has-session")
        .arg("-t")
        .arg(session)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .status()
        .await;
    matches!(status, Ok(s) if s.success())
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
            } => {
                assert_eq!(session, "alpha");
                assert!(port.is_none());
                assert!(config_path.is_none());
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
        ]);
        match cli.command {
            Cmd::Start {
                session,
                port,
                config_path,
            } => {
                assert_eq!(session, "beta");
                assert_eq!(port, Some(9999));
                assert_eq!(
                    config_path.as_deref(),
                    Some(std::path::Path::new("/tmp/x.toml"))
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
    fn benign_warnings_predicate() {
        let mut r = TeardownReport {
            warnings: vec![
                "tmux daemon already dead; skipping kill-server".to_string(),
                "socket already missing".to_string(),
                "config kept: /tmp/x".to_string(),
            ],
            ..TeardownReport::default()
        };
        assert!(has_only_benign_warnings(&r));

        r.warnings
            .push("socket unlink failed: permission denied".to_string());
        assert!(!has_only_benign_warnings(&r));
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
