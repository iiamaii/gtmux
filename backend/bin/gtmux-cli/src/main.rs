//! gtmux CLI — clap derive entrypoint (D20 subcommand set).
//!
//! `start` / `stop` / `teardown` / `rotate-token` / `status` — bodies are
//! `todo!()` placeholders. Real wiring lands in subsequent C2/C3 tasks.

#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};

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
        /// HTTP/WS listen port.
        #[arg(long)]
        port: Option<u16>,
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Start { .. } => todo!("gtmux start — wire lifecycle::spawn_daemon + http/ws routers"),
        Cmd::Stop { .. } => todo!("gtmux stop — graceful shutdown of Server process"),
        Cmd::Teardown { .. } => todo!("gtmux teardown — ADR-0009 D6 5-step cleanup"),
        Cmd::RotateToken { .. } => todo!("gtmux rotate-token — auth::rotate"),
        Cmd::Status => todo!("gtmux status — directory scan + health probe"),
    }
}
