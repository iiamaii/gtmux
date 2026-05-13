//! gtmux CLI — clap derive entrypoint (D20 subcommand set).
//!
//! `start` is wired end-to-end per `docs/reports/0012-bootstrap-smoke.md` §3
//! P0-CLI-1: config load → daemon spawn → token issue/load (mode-branched per
//! ADR-0003 D13.1) → axum router mount → bind → first-run banner (D21 c1 +
//! ADR-0003 D3 token URL) → graceful shutdown that leaves the tmux daemon
//! alive (ADR-0009 D5 / D21 c5).
//!
//! `stop` / `teardown` / `rotate-token` / `status` remain placeholders — they
//! are split out as P0-CLI-2..5 (Sprint 2+) per the bootstrap-smoke report.

#![forbid(unsafe_code)]
#![warn(clippy::all)]

use std::io::IsTerminal;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use axum::http::StatusCode;
use axum::routing::any;
use axum::Router;
use clap::{Parser, Subcommand};
use gtmux_auth::{issue_token, load_token, save_token, AuthError, TokenString};
use gtmux_config::{derive_mode, load as load_config, Config, Mode};
use gtmux_lifecycle::{LifecycleError, SpawnOptions, TmuxDaemon};
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
    Stop {
        #[arg(long)]
        session: String,
    },
    /// Teardown: ADR-0009 §D6 5-step cleanup (socket·token·layout·pid·config).
    Teardown {
        #[arg(long)]
        session: String,
    },
    /// Rotate the session token (cloud 모드 전용; local은 매 start 재발급).
    RotateToken {
        #[arg(long)]
        session: String,
    },
    /// Status: running Servers + bound ports + daemon health summary.
    Status,
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
        // Sprint 2 placeholders — surface a clear "not yet wired" error rather
        // than a panic. Exit 1 (generic) until the dedicated wiring lands.
        Cmd::Stop { .. } => {
            eprintln!("gtmux stop: not yet implemented (Sprint 2 / P0-CLI-2)");
            ExitCode::from(EXIT_FAILURE)
        }
        Cmd::Teardown { .. } => {
            eprintln!("gtmux teardown: not yet implemented (Sprint 2 / P0-CLI-3)");
            ExitCode::from(EXIT_FAILURE)
        }
        Cmd::RotateToken { .. } => {
            eprintln!("gtmux rotate-token: not yet implemented (Sprint 2+ / P0-CLI-4)");
            ExitCode::from(EXIT_FAILURE)
        }
        Cmd::Status => {
            eprintln!("gtmux status: not yet implemented (Sprint 2+ / P0-CLI-5)");
            ExitCode::from(EXIT_FAILURE)
        }
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

/// Execute the P0-CLI-1 13-step bootstrap sequence (grill report §3.1).
///
/// Step matrix (D20 + bootstrap-smoke §3 P0-CLI-1):
///   1) parse CLI args            — clap above
///   2) load config               — figment (CLI > Env > TOML > defaults)
///   3) derive mode               — bind value → Local / Cloud (D22)
///   4) init tracing              — log_level + log_format (text/json/auto)
///   5) spawn tmux daemon         — ADR-0009 D2/D3 dedicated daemon
///   6) issue / load token        — ADR-0003 D13.1 mode-branched
///   7) build http router         — placeholder mount (real lands in HTTP-1)
///   8) build ws router           — placeholder mount (real lands in WS-1)
///   9) merge into a single app
///  10) bind TCP listener         — `bind` + `port` from config (D2)
///  11) print first-run banner    — D21 c1 + ADR-0003 D3 token URL
///  12) install shutdown handlers — SIGINT + SIGTERM → graceful (D5 daemon ⊥)
///  13) axum::serve(...)          — with_graceful_shutdown
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

    // 7+8+9) router — build placeholder routes locally. The real routers live
    //   in `gtmux-http-api::router` / `gtmux-ws-server::router` (Sprint 2
    //   P0-HTTP-1 / P0-WS-1) and will be merged in *here* once their public
    //   signatures stabilise. Until then we mount catch-all 404s on the same
    //   listener so end-to-end smoke can exercise bind+banner without
    //   panicking on `todo!()` inside the placeholder routers.
    let app = build_app();

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
            drop(daemon);
            return Err(BindError::InUse(addr).into());
        }
        Err(e) => {
            drop(daemon);
            return Err(anyhow::Error::new(e).context(format!("binding {addr}")));
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
    let daemon = Arc::new(tokio::sync::Mutex::new(Some(daemon)));

    // 13) serve.
    let serve_result = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal)
        .await;

    // Post-shutdown — drop our control-mode client (daemon survives). The
    // farewell banner mirrors the start banner so the user immediately sees
    // that the daemon is *intentionally* still around.
    if let Some(mut d) = daemon.lock().await.take() {
        if let Err(e) = d.shutdown().await {
            warn!(error = %e, "control-mode client shutdown reported an error");
        }
    }
    print_farewell(&config.server.session);

    serve_result.context("axum::serve")
}

/// Build the in-CLI placeholder app. Every request returns 404 with a banner
/// in the body so callers can detect that the real handlers are not yet
/// mounted. When P0-HTTP-1 / P0-WS-1 land, this body becomes
/// `gtmux_http_api::router(&config, &token).merge(gtmux_ws_server::router(...))`.
fn build_app() -> Router {
    Router::new().fallback(any(|| async {
        (
            StatusCode::NOT_FOUND,
            "gtmux: HTTP/WS routers not yet wired (Sprint 2 — P0-HTTP-1 / P0-WS-1)\n",
        )
    }))
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
}
