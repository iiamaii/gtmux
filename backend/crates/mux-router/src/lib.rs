//! gtmux-mux-router — tmux control mode parser + command argv router.
//!
//! Owns the tmux-side state domain (sessions, windows, panes, output streams).
//! Exposes `Command` enum that mirrors the ADR-0008 allowlist exactly — no
//! variant exists for forbidden commands (`split-window`, `resize-pane`,
//! `select-layout`, `-CC`), so the type system itself enforces invariant #4.
//!
//! Bootstrap scaffold only; real implementation lands in subsequent tasks.

#![forbid(unsafe_code)]

/// tmux command allowlist as a closed enum.
///
/// 출처: `docs/adr/0008-single-pane-window-and-group.md` §"tmux command allowlist 표".
/// 새 명령 추가는 본 enum + ADR-0008 표 동시 갱신 필수.
#[derive(Debug, Clone)]
pub enum Command {
    /// `tmux new-window -t <session>` — Pane 생성 (single-pane-per-window 컨벤션).
    NewWindow,
    /// `tmux kill-pane -t %<pid>` — Panel close / Group close 재귀.
    KillPane,
    /// `tmux kill-window -t @<wid>` — gtmux 내부 정리용 (빈 Window 청소).
    KillWindow,
    /// `tmux rename-window -t @<wid> <label>` — Panel label → window name 동기화 (D5).
    RenameWindow,
    /// `tmux send-keys -t %<pid>` — Input Target(I)로 지정된 Panel 입력 전달.
    SendKeys,
    /// `tmux refresh-client -A '%<pid>:pause/continue'` — Panel Streaming State 전이.
    RefreshClientPause,
    /// `tmux refresh-client -B <subscription>` — 포맷 구독 (tmux 3.2+ 푸시 모델).
    RefreshClientSubscribe,
    /// `tmux capture-pane -p -e -J -S -<lines>` — Deep scrollback 회복 (P1+).
    CapturePane,
    /// `tmux list-sessions -F` — 부트스트랩 1회 스냅샷.
    ListSessions,
    /// `tmux list-windows -a -F` — 부트스트랩 1회 스냅샷.
    ListWindows,
    /// `tmux list-panes -a -F` — 부트스트랩 1회 스냅샷.
    ListPanes,
}

/// 부트스트랩 placeholder — control mode 클라이언트 attach 시그니처.
pub fn connect() -> anyhow::Result<()> {
    todo!("mux-router::connect — control mode attach to be implemented")
}
