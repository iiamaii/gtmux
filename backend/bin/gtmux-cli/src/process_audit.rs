//! Boot-time orphan process audit (ADR-0014 D11, 2026-05-15 amend).
//!
//! Every child shell spawned by PtyBackend has `GTMUX_SESSION=<session>`
//! and `GTMUX_SERVER_PID=<pid>` injected into its environment. Normally
//! these processes die with the Server (ADR-0014 D5 — Drop of
//! PtyBackend sends SIGTERM fan-out). But if the Server crashes (SIGKILL,
//! kernel OOM, hardware lockup), the child shells are orphaned and
//! re-parented to init (PID 1). The next Server boot finds them via
//! this audit and reaps them before spawning new panes.
//!
//! Cross-platform via `sysinfo` — Linux reads `/proc/<pid>/environ`,
//! macOS uses `proc_pidinfo` and `KERN_PROCARGS2`.

use std::time::Duration;

use sysinfo::{Pid, Signal, System};
use tracing::{info, warn};

/// Result of one audit pass. Fields are *for human reporting* — the
/// CLI banner can log "Reaped N stale gtmux processes from previous
/// session" before booting.
#[derive(Debug, Default)]
pub struct OrphanAuditReport {
    /// Total candidate processes detected (GTMUX_SESSION match + not our
    /// own PID). Subset of these are *signalled*.
    pub candidates: Vec<OrphanProcess>,
    /// Processes that were successfully signalled. Subset of candidates.
    pub signalled: Vec<libc::pid_t>,
    /// Processes that resisted SIGTERM and got SIGKILL.
    pub force_killed: Vec<libc::pid_t>,
    /// Errors encountered (does not block boot — best-effort).
    pub warnings: Vec<String>,
}

/// Single candidate orphan — surfaced for logging / debugging.
#[derive(Debug, Clone)]
pub struct OrphanProcess {
    pub pid: libc::pid_t,
    /// Recorded `GTMUX_SERVER_PID` env value (the prior Server's PID).
    pub prior_server_pid: Option<libc::pid_t>,
    /// Command line of the orphan (e.g. `/bin/zsh`).
    pub command: String,
}

/// Scan all live processes for ones tagged with `GTMUX_SESSION=<session>`
/// and `GTMUX_SERVER_PID != current_pid`. Signal each with SIGTERM,
/// wait briefly, then escalate to SIGKILL if needed.
///
/// Errors are accumulated into `report.warnings` — the function never
/// blocks boot. A clean Server (no prior crash) returns an empty report
/// in milliseconds (sysinfo enumerates ≈ 500 processes on macOS).
pub fn reap_orphans(session_marker: &str) -> OrphanAuditReport {
    let mut report = OrphanAuditReport::default();

    let mut sys = System::new();
    // We only need processes for this audit — skip CPU/mem refresh.
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::new().with_environ(sysinfo::UpdateKind::Always),
    );

    let our_pid_u32: u32 = std::process::id();
    let our_pid = our_pid_u32 as libc::pid_t;

    for (sys_pid, proc) in sys.processes() {
        let pid_u32: u32 = sys_pid.as_u32();
        let pid = pid_u32 as libc::pid_t;
        // Skip ourselves *and* our direct ancestors / children. The
        // tracked invariant is GTMUX_SERVER_PID — any matching child
        // whose prior_server_pid != our_pid is a *previous* Server's
        // orphan.
        if pid == our_pid {
            continue;
        }

        let environ = proc.environ();
        let mut session_match = false;
        let mut prior_server_pid: Option<libc::pid_t> = None;

        for entry in environ {
            let s = entry.to_string_lossy();
            if let Some(value) = s.strip_prefix("GTMUX_SESSION=") {
                if value == session_marker {
                    session_match = true;
                }
            } else if let Some(value) = s.strip_prefix("GTMUX_SERVER_PID=") {
                if let Ok(n) = value.parse::<libc::pid_t>() {
                    prior_server_pid = Some(n);
                }
            }
        }

        if !session_match {
            continue;
        }
        if prior_server_pid == Some(our_pid) {
            // This is one of *our own* children spawned this Server run
            // (during the same process lifetime). Don't touch.
            continue;
        }

        let command = proc
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");

        report.candidates.push(OrphanProcess {
            pid,
            prior_server_pid,
            command: command.clone(),
        });

        // SIGTERM first.
        if proc.kill_with(Signal::Term).unwrap_or(false) {
            report.signalled.push(pid);
        } else {
            report
                .warnings
                .push(format!("SIGTERM dispatch failed for pid {pid}"));
        }
    }

    // Wait for the SIGTERMs to land — 250ms is typically enough for a
    // shell to exit when its parent is already gone. The grace is bounded
    // because we don't want boot to stall.
    if !report.signalled.is_empty() {
        std::thread::sleep(Duration::from_millis(250));
    }

    // Escalate any survivors to SIGKILL.
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::new(),
    );
    for pid in report.signalled.clone() {
        if sys.process(Pid::from_u32(pid as u32)).is_some() {
            // Still alive — escalate.
            // SAFETY: libc::kill with a parsed pid + constant signal is sound.
            #[allow(unsafe_code)]
            unsafe {
                if libc::kill(pid, libc::SIGKILL) == 0 {
                    report.force_killed.push(pid);
                } else {
                    let err = std::io::Error::last_os_error();
                    // ESRCH = process already gone between refresh and kill
                    // — fine.
                    if err.raw_os_error() != Some(libc::ESRCH) {
                        report
                            .warnings
                            .push(format!("SIGKILL dispatch failed for pid {pid}: {err}"));
                    }
                }
            }
        }
    }

    let total = report.candidates.len();
    if total > 0 {
        info!(
            session = %session_marker,
            candidates = total,
            sigtermed = report.signalled.len(),
            sigkilled = report.force_killed.len(),
            "process_audit: reaped {} orphan child(ren) from previous gtmux session",
            total
        );
        for c in &report.candidates {
            info!(pid = c.pid, prior_pid = ?c.prior_server_pid, cmd = %c.command, "orphan reaped");
        }
    } else {
        // Silent log — boot path stays quiet on the common case.
        tracing::debug!(session = %session_marker, "process_audit: no orphans");
    }
    for w in &report.warnings {
        warn!("process_audit: {w}");
    }

    report
}
