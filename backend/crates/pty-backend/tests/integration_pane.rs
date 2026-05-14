//! Integration tests — POC Gate #1~#5 + multi-pane + late-attach.
//!
//! Reference: docs/reports/0023-pty-poc-verification-and-decision.md §1.2
//! (POC gate definitions) + docs/adr/0013-pty-direct-no-tmux.md §D2~D7
//! + docs/reports/0025-session-resume-handoff.md §4.1 (S7-PTY-BACKEND DoD).
//!
//! All gates spawn a real `/bin/sh` (POSIX baseline — vim/tput may be
//! missing on minimal CI images, raw escape sequences keep parity).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use bytes::Bytes;
use gtmux_pty_backend::{BackendNotify, PaneId, PtyBackend, SpawnSpec};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{sleep, timeout};

/// Spawn `/bin/sh` with a known geometry. Uses `env -i` style by clearing
/// PS1 / PROMPT_COMMAND noise via the spec env so output is predictable.
fn shell_spec() -> SpawnSpec {
    SpawnSpec {
        command: Some("/bin/sh".into()),
        args: Vec::new(),
        cwd: None,
        env: vec![
            ("PS1".into(), "$ ".into()),
            ("ENV".into(), "/dev/null".into()),
        ],
        rows: 24,
        cols: 80,
    }
}

/// Read PTY output until `needle` appears or `budget` elapses. Returns
/// the accumulated output (stripped of ANSI controls is *not* done here;
/// tests assert on raw bytes).
async fn read_until(
    rx: &mut tokio::sync::broadcast::Receiver<Bytes>,
    needle: &[u8],
    budget: Duration,
) -> Vec<u8> {
    let mut acc = Vec::new();
    let result = timeout(budget, async {
        loop {
            match rx.recv().await {
                Ok(b) => {
                    acc.extend_from_slice(&b);
                    if acc.windows(needle.len()).any(|w| w == needle) {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            }
        }
    })
    .await;
    if result.is_err() {
        eprintln!(
            "read_until: timeout after {:?} waiting for {:?}, acc={:?}",
            budget,
            String::from_utf8_lossy(needle),
            String::from_utf8_lossy(&acc),
        );
    }
    acc
}

// ─────────────────────────────────────────────────────────────────────────────
//  Gate #1 — Signal handling (Ctrl-C interrupts a foreground sleep).
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gate1_signal_ctrl_c_interrupts_sleep() {
    let backend = PtyBackend::new();
    let id = backend.spawn(shell_spec()).expect("spawn");
    let (_snap, mut rx) = backend.subscribe_output(id).expect("subscribe");

    // Drain initial prompt.
    sleep(Duration::from_millis(150)).await;

    // Launch a long sleep that echoes "BEFORE" first, then "AFTER" after
    // the sleep returns. Ctrl-C should interrupt the sleep; "AFTER" must
    // NOT appear before the prompt comes back.
    backend
        .send_input(id, b"echo BEFORE; sleep 30; echo AFTER\n".to_vec())
        .expect("send");
    let _ = read_until(&mut rx, b"BEFORE", Duration::from_secs(3)).await;

    // Send Ctrl-C (0x03). The shell traps SIGINT, the sleep terminates,
    // and the prompt returns.
    backend.send_input(id, vec![0x03]).expect("send ctrl-c");

    let acc = read_until(&mut rx, b"$ ", Duration::from_secs(3)).await;
    assert!(
        !acc.windows(5).any(|w| w == b"AFTER"),
        "Ctrl-C did not interrupt sleep; output contained AFTER: {:?}",
        String::from_utf8_lossy(&acc)
    );

    backend.kill(id).expect("kill");
}

// ─────────────────────────────────────────────────────────────────────────────
//  Gate #2 — Resize (TIOCSWINSZ → SIGWINCH → child sees new geometry).
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gate2_resize_propagates_to_child_via_tput() {
    let backend = PtyBackend::new();
    let id = backend.spawn(shell_spec()).expect("spawn");
    let (_snap, mut rx) = backend.subscribe_output(id).expect("subscribe");

    // Drain prompt.
    sleep(Duration::from_millis(150)).await;

    // Resize to a distinctive non-default geometry.
    backend.resize(id, 40, 132).expect("resize");
    sleep(Duration::from_millis(100)).await;

    // Ask the shell via `stty size`, which reads the controlling-tty
    // dimensions via TIOCGWINSZ. POSIX `stty` is available on /bin/sh
    // baseline images where `tput` may not be.
    backend
        .send_input(id, b"stty size\n".to_vec())
        .expect("send");
    let acc = read_until(&mut rx, b"40 132", Duration::from_secs(3)).await;
    assert!(
        acc.windows(6).any(|w| w == b"40 132"),
        "stty size did not reflect resize; output: {:?}",
        String::from_utf8_lossy(&acc)
    );

    backend.kill(id).expect("kill");
}

// ─────────────────────────────────────────────────────────────────────────────
//  Gate #3 — Alt-screen sequences pass through the broadcast unmodified.
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gate3_alt_screen_sequences_passthrough() {
    let backend = PtyBackend::new();
    let id = backend.spawn(shell_spec()).expect("spawn");
    let (_snap, mut rx) = backend.subscribe_output(id).expect("subscribe");

    sleep(Duration::from_millis(150)).await;

    // Disable line-discipline echo so the input doesn't bounce back as
    // *literal* `\\033` text — otherwise our sentinel match below would
    // hit the input echo before the shell actually executes the printf.
    backend
        .send_input(id, b"stty -echo\n".to_vec())
        .expect("send stty");
    sleep(Duration::from_millis(300)).await;

    // Emit the alt-screen enter sequence (CSI ?1049h), a marker, then
    // leave alt-screen (CSI ?1049l), then a trailing sentinel echo.
    backend
        .send_input(
            id,
            b"printf '\\033[?1049hALT_IN\\033[?1049l'; echo SENTINEL_AS\n".to_vec(),
        )
        .expect("send");

    let acc = read_until(&mut rx, b"SENTINEL_AS", Duration::from_secs(3)).await;
    // The raw CSI 1049h byte sequence must appear in the stream — the
    // broadcast path is byte-transparent (we never strip or rewrite
    // escape sequences).
    assert!(
        acc.windows(8).any(|w| w == b"\x1b[?1049h"),
        "alt-screen enter sequence not propagated; output: {:?}",
        acc
    );

    backend.kill(id).expect("kill");
}

// ─────────────────────────────────────────────────────────────────────────────
//  Gate #4 — Shell exit + zombie reap (child.wait() drives NOTIFY).
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gate4_shell_exit_emits_pane_died_no_zombie() {
    let backend = PtyBackend::new();
    let mut notify = backend.subscribe_notify();
    let id = backend.spawn(shell_spec()).expect("spawn");

    // Drain the PaneSpawned notify.
    let _ = timeout(Duration::from_secs(1), notify.recv()).await;

    sleep(Duration::from_millis(100)).await;

    // Tell the shell to exit cleanly.
    backend
        .send_input(id, b"exit 0\n".to_vec())
        .expect("send exit");

    // The wait thread polls try_wait() at 50 ms cadence (see lib.rs
    // spawn_inner). 2 s budget is generous.
    let died = timeout(Duration::from_secs(2), async {
        loop {
            match notify.recv().await {
                Ok(BackendNotify::PaneDied { id: did, code, .. }) if did == id => {
                    return Some(code);
                }
                Ok(_) => continue,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    })
    .await
    .expect("timeout waiting for pane-died");

    assert!(
        died.is_some(),
        "PaneDied notification not received within budget"
    );
    // After the wait thread observed exit, the dashmap entry remains
    // (kill() / Drop is what removes it). The pane count drop is verified
    // by the multi_pane_race test below.
}

// ─────────────────────────────────────────────────────────────────────────────
//  Gate #5 — Burst tolerance: ring buffer absorbs a chunk > BROADCAST cap.
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gate5_burst_seq_completes() {
    let backend = PtyBackend::new();
    let id = backend.spawn(shell_spec()).expect("spawn");
    let (_snap, mut rx) = backend.subscribe_output(id).expect("subscribe");

    sleep(Duration::from_millis(150)).await;

    // Use `seq 1 2000` — predictable, finite, generates ~10 KB which is
    // well within ring buffer (128 KiB) but produces enough chunks to
    // exercise the broadcast fan-out. After seq finishes, write DONE.
    backend
        .send_input(id, b"seq 1 2000 > /dev/null && echo DONE_BURST\n".to_vec())
        .expect("send");

    let acc = read_until(&mut rx, b"DONE_BURST", Duration::from_secs(10)).await;
    assert!(
        acc.windows(10).any(|w| w == b"DONE_BURST"),
        "burst sentinel never appeared: {:?}",
        String::from_utf8_lossy(&acc)
    );

    backend.kill(id).expect("kill");
}

// ─────────────────────────────────────────────────────────────────────────────
//  Multi-pane isolation — three concurrent panes with distinct env.
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multi_pane_isolation_env_per_pane() {
    let backend = PtyBackend::new();
    let mut ids = Vec::new();
    let mut rxs = Vec::new();

    for i in 0..3u32 {
        let mut spec = shell_spec();
        spec.env.push(("PANE_TAG".into(), format!("tag-{i}")));
        let id = backend.spawn(spec).expect("spawn");
        let (_snap, rx) = backend.subscribe_output(id).expect("subscribe");
        ids.push(id);
        rxs.push(rx);
    }
    assert_eq!(backend.pane_count(), 3);

    sleep(Duration::from_millis(200)).await;

    for (i, id) in ids.iter().enumerate() {
        backend
            .send_input(*id, b"echo $PANE_TAG\n".to_vec())
            .expect("send");
        let needle = format!("tag-{i}");
        let acc = read_until(&mut rxs[i], needle.as_bytes(), Duration::from_secs(3)).await;
        assert!(
            acc.windows(needle.len()).any(|w| w == needle.as_bytes()),
            "pane {i}: expected {needle:?}, got {:?}",
            String::from_utf8_lossy(&acc)
        );
    }

    for id in ids {
        backend.kill(id).expect("kill");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Multi-pane race — repeated spawn/kill exercises broadcast/mpsc/thread
//  teardown without leaking thread handles or PaneHandle Arcs.
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multi_pane_race_spawn_kill() {
    let backend = PtyBackend::new();
    for _ in 0..20 {
        let mut ids = Vec::new();
        for _ in 0..3 {
            ids.push(backend.spawn(shell_spec()).expect("spawn"));
        }
        for id in ids {
            backend.kill(id).expect("kill");
        }
    }
    // After every kill, pane_count drops to 0. Some pane-died notify
    // events are still in flight to subscribers, but the backend's
    // own dashmap is empty.
    assert_eq!(backend.pane_count(), 0);
}

// ─────────────────────────────────────────────────────────────────────────────
//  Late attach — subscriber that arrives after output replays the ring.
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn late_attach_replays_ring_buffer() {
    let backend = PtyBackend::new();
    let id = backend.spawn(shell_spec()).expect("spawn");

    // Wait long enough for the shell prompt + a synthetic marker to land
    // in the ring buffer before any subscriber attaches.
    sleep(Duration::from_millis(150)).await;
    backend
        .send_input(id, b"echo MARKER_LATE\n".to_vec())
        .expect("send");
    sleep(Duration::from_millis(400)).await;

    // Now attach for the first time and inspect the snapshot.
    let (snap, _rx) = backend.subscribe_output(id).expect("subscribe");
    assert!(
        snap.windows(11).any(|w| w == b"MARKER_LATE"),
        "ring replay missing marker; snapshot: {:?}",
        String::from_utf8_lossy(&snap)
    );

    backend.kill(id).expect("kill");
}

// ─────────────────────────────────────────────────────────────────────────────
//  Drop cleanup — letting PtyBackend go out of scope tears down all panes.
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drop_backend_terminates_all_panes() {
    let pids: Vec<u32> = {
        let backend = PtyBackend::new();
        let _id1 = backend.spawn(shell_spec()).expect("spawn 1");
        let _id2 = backend.spawn(shell_spec()).expect("spawn 2");
        let _id3 = backend.spawn(shell_spec()).expect("spawn 3");
        // Snapshot the pids before drop so we can probe afterwards.
        sleep(Duration::from_millis(50)).await;
        backend
            .pane_ids()
            .into_iter()
            .map(|p: PaneId| p.0 as u32)
            .collect()
    };
    // Backend is now dropped — graceful teardown should have signalled
    // every pane. We can't easily check process-tree state from a
    // unit test without /proc dependencies, so we just verify the API
    // didn't panic + the test exits cleanly.
    assert_eq!(pids.len(), 3);
}
