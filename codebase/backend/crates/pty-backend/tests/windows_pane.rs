#![cfg(windows)]
//! Native ConPTY smoke test. Runs on Windows CI, never through WSL.
use gtmux_pty_backend::{PtyBackend, SpawnSpec};
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_shell_output_and_shutdown() {
    let backend = PtyBackend::new();
    let spec = SpawnSpec { command: Some("cmd.exe".into()), args: vec!["/D".into(), "/Q".into(), "/K".into(), "echo GTMUX_CONPTY_READY".into()], ..Default::default() };
    let id = backend.spawn(spec).expect("ConPTY shell must spawn");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let (snapshot, _) = backend.subscribe_output(id).expect("pane exists");
        if String::from_utf8_lossy(&snapshot).contains("GTMUX_CONPTY_READY") { break; }
        assert!(tokio::time::Instant::now() < deadline, "ConPTY output was not received");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    backend.kill(id).expect("owned shell must stop");
    assert!(backend.pane_ids().is_empty());
}
