//! gtmux-lifecycle — dedicated tmux daemon spawn / teardown / socket cleanup.
//!
//! Implements ADR-0009 (tmux daemon isolation). 본 crate가 `tmux -L
//! gtmux-<session> -C` 프로세스 spawn + ADR-0009 §D6 5단계 teardown 절차의
//! 단일 책임자.
//!
//! Bootstrap scaffold only.

#![forbid(unsafe_code)]

/// 부트스트랩 placeholder — ADR-0009 §D3 dedicated tmux daemon spawn.
///
/// 실제 구현은 `tmux -L gtmux-<session> start-server` 호출 + 소켓 파일
/// 존재 검증 + PID file 작성 단계로 구성된다.
pub fn spawn_daemon() -> anyhow::Result<()> {
    todo!("lifecycle::spawn_daemon — ADR-0009 D3 spawn sequence")
}

/// 부트스트랩 placeholder — ADR-0009 §D6 5단계 teardown 절차.
///
/// 5단계: (1) 모든 panes kill, (2) tmux server kill, (3) 소켓 파일 rm,
/// (4) token file rm, (5) layout/PID/config 정리.
pub fn teardown() -> anyhow::Result<()> {
    todo!("lifecycle::teardown — ADR-0009 D6 5-step teardown")
}
