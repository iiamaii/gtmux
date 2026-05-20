# Sprint 7 Stage B — Carry-forward 보고서

본 문서는 `0025-session-resume-handoff.md` 의 후속. Stage A 완료 + Stage B 의 step 1 (TMUX env startup guard) 까지 진행된 시점의 *남은 작업 정본* 이다. 다음 세션 (또는 backend-architect agent) 이 즉시 이어서 진행 가능한 self-contained reference 로 작성.

---

## 0. 한 줄 상태

- **현 시점 (2026-05-14 21:30 경)**: Stage A 완료 (`3623aa3`, `552e5c6`) + Stage B step 1 완료 (`5eec978`). 다음 = Stage B step 2 (lifecycle 폐기 + ws-server CTRL 재배선 + gtmux-cli wiring + 테스트 갱신).
- **main 브랜치**: 여전히 tmux backend 위에서 demo 실행 가능 (workspace 212/212 tests PASS — pty-backend crate 는 standalone 으로만 존재).
- **차단성 갭 0건**.

---

## 1. 본 세션 완료 작업 (commits)

| commit | 작업 |
|---|---|
| `3623aa3` | Stage A — gtmux-pty-backend crate 신규 (1098 LOC + 9 integration tests + 11 단위 tests, 모두 PASS) |
| `552e5c6` | Stage A grilling amend — ADR-0013 D3·D10·R3 / ADR-0014 D10 amend + 코드 정합 (`DaemonStarted→ServerReady`, `SetCwd/SetEnv` 폐기, auto-cleanup wait thread, `PaneGone` dead code 제거) |
| `5eec978` | Stage B step 1 — `gtmux-cli start` 의 TMUX env startup guard (ADR-0014 D10 amend 의 1차 방어) |

본 세션의 모든 변경은 *additive* — 기존 192 tests 회귀 0 + 신규 20 추가 = workspace 212/212 PASS.

---

## 2. 남은 Stage B 작업 (S7-PTY-BACKEND §B + §C)

### 2.1 핵심 surgery 5건 (강한 결합으로 단일 commit 권장)

| 작업 | 대상 파일 | 예상 변경 |
|---|---|---|
| (a) lifecycle crate 완전 삭제 | `codebase/backend/crates/lifecycle/` (2467 LOC) | 디렉터리 통째로 `git rm -r` |
| (b) mux-router crate 완전 삭제 | `codebase/backend/crates/mux-router/` (897 LOC) | 디렉터리 통째로 `git rm -r` |
| (c) ws-server CTRL 재배선 | `codebase/backend/crates/ws-server/src/{lib.rs, cmd_router.rs, hub.rs}` (~2000 LOC) | 다음 §3.1 참조 |
| (d) gtmux-cli start() 재배선 | `codebase/backend/bin/gtmux-cli/src/main.rs` (~150 LOC of `start()`) | 다음 §3.2 참조 |
| (e) http-api Hub 정합 | `codebase/backend/crates/http-api/src/lib.rs` line 122·144·664 | Hub 의 publish_layout_changed 인터페이스 보존 (1줄) |
| (f) workspace Cargo.toml 정리 | `codebase/backend/Cargo.toml` members | lifecycle / mux-router 제거 |
| (g) ws-server 테스트 재정렬 | `codebase/backend/crates/ws-server/src/lib.rs` tests + 다른 파일 | 다음 §3.3 참조 |

### 2.2 작업 순서 (위→아래 의존)

1. ws-server 의 새 cmd_router 작성 (`gtmux_pty_backend::BackendCommand` 디스패치)
2. ws-server 의 새 hub 작성 (PtyBackend 의 broadcast 를 attach 시점 replay + layout_events 만 보존)
3. ws-server lib.rs 의 `handle_socket` / `event_to_envelope` 재배선
4. http-api Hub 인터페이스 정합 (`hub.publish_layout_changed(etag)` 1줄 — 인터페이스 변화 없으면 noop)
5. gtmux-cli `start()` 재배선:
   - `TmuxDaemon::spawn` → `PtyBackend::new()`
   - `run_event_loop` / `run_command_loop` 두 spawn 폐기 (PtyBackend 가 내부 thread 로 흡수)
   - import 줄에서 `gtmux_lifecycle::*` 제거
   - graceful shutdown 의 `daemon_arc.shutdown()` → `drop(pty_backend)` (PtyBackend::Drop 이 graceful teardown)
6. gtmux-cli 의 `teardown_cmd` / `stop` 함수 재배선:
   - `gtmux_lifecycle::stop_server` / `teardown` 폐기 → ADR-0014 D7 의 4단계 (1단계 + 4단계만 pid/lock/token/layout/config 파일 정리, 2·3단계는 자손 SIGHUP 자동)
7. workspace `Cargo.toml` 의 `members` 에서 `crates/lifecycle`, `crates/mux-router` 제거
8. `git rm -r crates/lifecycle crates/mux-router`
9. ws-server 의 tmux-specific tests 폐기 (~50 tests), framing-level tests (envelope/varint/ring/subprotocol parse, ~20 tests) 보존
10. PtyBackend 와 통합한 새 tests 작성 (~15 tests 권장 — backend_dispatch / replay_on_attach / notify_mirror_serde / layout_etag_broadcast 등)

### 2.3 새 ws-server cmd_router 의 인터페이스 (drop-in 대체)

```rust
// codebase/backend/crates/ws-server/src/cmd_router.rs (new)
use gtmux_pty_backend::{BackendCommand, PaneId, PtyBackend};

/// Single outbound request — replaces the old TmuxRequest.
pub enum BackendRequest {
    /// Spawn a Pane. Reply = PaneSpawned NOTIFY_MIRROR.
    NewPane { request_id: Option<String>, /* spec fields ... */ },
    /// Kill a Pane. Reply = PaneDied NOTIFY_MIRROR (from PtyBackend's wait thread).
    Kill(PaneId),
    /// Resize. No reply (TIOCSWINSZ is fire-and-forget).
    Resize(PaneId, u16, u16),
    /// Input bytes. No reply.
    Input(PaneId, Vec<u8>),
    /// Streaming State pause/resume — handled in ws-server level by
    /// dropping / re-subscribing the broadcast::Receiver, NO backend call.
    PauseStream(PaneId),
    ResumeStream(PaneId),
}

pub fn dispatch(backend: &PtyBackend, req: BackendRequest) -> Result<(), ...> {
    match req {
        BackendRequest::NewPane { request_id, .. } => {
            let spec = SpawnSpec::default_shell();
            if let Some(rid) = request_id {
                backend.spawn_with_request(spec, rid)?;
            } else {
                backend.spawn(spec)?;
            }
        }
        BackendRequest::Kill(id) => backend.kill(id)?,
        BackendRequest::Resize(id, rows, cols) => backend.resize(id, rows, cols)?,
        BackendRequest::Input(id, bytes) => backend.send_input(id, bytes)?,
        BackendRequest::PauseStream(_) | BackendRequest::ResumeStream(_) => {
            // No backend call — ws-server's per-connection loop manages
            // its own broadcast::Receiver lifetime.
        }
    }
    Ok(())
}
```

### 2.4 새 hub 의 인터페이스 (slimmed)

```rust
// codebase/backend/crates/ws-server/src/hub.rs (new, ~80 LOC)
use gtmux_pty_backend::PtyBackend;

#[derive(Clone)]
pub struct Hub {
    backend: PtyBackend,
    layout_events: broadcast::Sender<[u8; 16]>,
}

impl Hub {
    pub fn new(backend: PtyBackend) -> Self { ... }

    /// API compat (http-api uses this).
    pub fn publish_layout_changed(&self, etag: [u8; 16]) { ... }

    pub fn subscribe_layout(&self) -> broadcast::Receiver<[u8; 16]> { ... }

    /// New API — replaces the old `subscribe()` (Event stream) and
    /// `snapshot()` (ring buffer). Subscribers attach to a specific
    /// Pane's broadcast + replay.
    pub fn subscribe_pane(&self, id: PaneId) -> Option<(Vec<u8>, broadcast::Receiver<Bytes>)> {
        self.backend.subscribe_output(id)
    }

    /// Backend notify stream (PaneSpawned/PaneDied/LayoutChanged/ServerReady).
    pub fn subscribe_notify(&self) -> broadcast::Receiver<BackendNotify> {
        self.backend.subscribe_notify()
    }
}
```

### 2.5 새 envelope ↔ NOTIFY_MIRROR 매핑

(ws-server lib.rs 의 `event_to_envelope` 대체)

| BackendNotify variant | NOTIFY_MIRROR JSON payload | paneId varint |
|---|---|---|
| `PaneSpawned { id, request_id }` | `{"kind":"pane-spawned","request_id":...}` | id |
| `PaneDied { id, code, signal }` | `{"kind":"pane-died","code":...,"signal":...}` | id |
| `LayoutChanged` | `{"kind":"layout-changed"}` | 0 |
| `ServerReady` | `{"kind":"server-ready"}` | 0 |

PaneOut 의 *바이트 스트림* 은 `Hub::subscribe_pane(id)` 의 `broadcast::Receiver<Bytes>` 로부터 직접 — 별 dispatch 없음 (PTY master fd → broadcast → WS sink, byte-transparent, ADR-0013 D10).

---

## 3. 코드 위치별 변경 디테일

### 3.1 ws-server/src/lib.rs

**제거**:
- `use gtmux_mux_router::Event;` (line 42)
- `pub use cmd_router::{build_ctrl_request, ..., TmuxRequest, ALLOWLISTED_CTRL_CMDS}` (line 54)
- `fn event_to_envelope` (line 283~367) — 전체 교체
- `Event::Output / Pause / SessionChanged / WindowAdd / ...` 의 매핑들 — 전부 BackendNotify 로 교체

**유지** (framing-level, 그대로):
- `FrameType`, `Envelope`, `CodecError` (line 107~270)
- `MAX_PAYLOAD`, `HEADER_LEN`, `PING_INTERVAL`, `PONG_TIMEOUT`, close_codes (line 60~104)
- `parse_subprotocol`, `ParsedSubprotocol` (line 398~456)
- 모듈 `payload`, `ring`, `varint`

**수정**:
- `router(...)` 시그니처: `cmd_tx: mpsc::Sender<TmuxRequest>` → `backend: PtyBackend`
- `WsState`: `cmd_tx` → `backend: PtyBackend`
- `handle_socket(...)`: hub.subscribe() 대신 hub.subscribe_notify() + 클라이언트의 active pane 별 subscribe_pane() 합성

### 3.2 ws-server/src/hub.rs

전체 재작성 — 위 §2.4 의 스켈레톤대로 80~120 LOC. 기존 `Event` 스트림 / `snapshot_all` / `snapshot_session` 폐기 (PaneSpawned NOTIFY 가 spawn 시점 1회 fan-out 이어서 catch-up 불필요).

### 3.3 ws-server tests

**보존 (~20 tests)** — framing-level:
- `varint::tests::*` (5 tests)
- `ring::tests::*` (3 tests)
- `tests::parse_subprotocol_*` (4 tests)
- `tests::envelope_*` (encoding/decoding, 5 tests)
- `tests::ws_upgrade_*` (3 tests)

**삭제 (~50 tests)** — tmux-specific:
- `cmd_router::tests::*` (12 tests — 모두 tmux Command 의 argv 변환)
- `tests::allowed_ctrl_cmd_routed_to_command` (tmux allowlist 기반)
- `tests::client_pane_in_routed_to_command` (TmuxRequest 발행)
- `event_to_envelope` 기반 tests (15+ tests)
- `hub::tests::*` (5 tests — Event stream + snapshot)
- 기타 tmux-domain 통합

**신규 추가 (~15 tests)** — PTY direct 통합:
- `tests::backend_pane_spawned_notify_routed` (BackendNotify::PaneSpawned → 0x07 NOTIFY_MIRROR)
- `tests::backend_pane_died_notify_routed`
- `tests::pane_input_envelope_routed_to_send_input`
- `tests::pane_resize_envelope_routed_to_resize`
- `tests::late_attach_replays_ring`
- `tests::pause_resume_drops_and_reattaches_subscriber`
- 기타 새 envelope 의미 검증

### 3.4 gtmux-cli/src/main.rs `start()` 함수

**제거** (line 38~43):
```rust
use gtmux_lifecycle::{
    check_pidfile_liveness, pidfile_path_for, run_command_loop, run_event_loop, socket_path_for,
    stop_server, write_pidfile, LifecycleError, PidLiveness, SpawnOptions, StopOutcome,
    TeardownOpts, TeardownReport, TmuxDaemon,
};
use gtmux_ws_server::{Hub, TmuxRequest};
```

**대체**:
```rust
use gtmux_pty_backend::PtyBackend;
use gtmux_ws_server::Hub;
// pidfile + state file logic: gtmux-cli 가 직접 구현 (ADR-0014 D7 4단계).
// pidfile_path_for, write_pidfile, check_pidfile_liveness 의 본문을 gtmux-cli
// 안으로 내재화 (lifecycle 폐기로 해당 함수들이 사라짐).
```

**`start()` 핵심 변경** (line 297~371):
```rust
// 폐기:
let daemon = TmuxDaemon::spawn(SpawnOptions { ... }).await?;
let event_loop_handle = tokio::spawn({ ... run_event_loop ... });
let command_loop_handle = tokio::spawn({ ... run_command_loop ... });

// 대체:
let backend = PtyBackend::new();
let hub = Hub::new(backend.clone()); // Hub 가 PtyBackend wrapper
// Background event/command loops 가 사라짐 — PtyBackend 의 std::thread
// per-pane reader/writer/wait 가 모든 IO 흡수
```

**shutdown 경로** (line 446~457):
```rust
// 폐기:
event_loop_handle.abort();
command_loop_handle.abort();
daemon_arc.shutdown().await?;

// 대체:
drop(backend); // PtyBackend::Drop 이 ADR-0014 D7 의 step 1 (모든 pane SIGTERM → 200ms → SIGKILL) 자동
```

### 3.5 gtmux-cli 의 `teardown_cmd` / `stop`

**`stop(session, force)`**: pidfile 의 PID 에 SIGTERM 발사 + 5s wait + (force) SIGKILL — 현 lifecycle::stop_server 의 본문을 gtmux-cli 안으로 내재화 (~50 LOC).

**`teardown_cmd(...)`**: ADR-0014 D7 의 4단계 정확 구현. 현 lifecycle::teardown 의 5단계 (tmux kill-server / socket 정리 2단계 폐기) 를 4단계 (pid + lock + token + layout + config 파일 정리) 로 단순화. ~100 LOC.

### 3.6 http-api/src/lib.rs

**변경 없음** (인터페이스 보존). line 122·144·664 의 `gtmux_ws_server::Hub` 가 그대로 — 새 Hub 의 `publish_layout_changed(etag)` API 가 동일 시그니처.

---

## 4. 시간 추정 + Risk

### 4.1 시간 추정

- §3.1 + §3.2 ws-server 코어 (cmd_router + hub + lib.rs): **3~4시간**
- §3.4 + §3.5 gtmux-cli start/teardown/stop: **2~3시간**
- §3.3 tests: **1~2시간**
- §3.6 http-api: **15분** (인터페이스 보존, noop)
- 빌드 ↔ 테스트 cycle 디버깅: **1~2시간**
- 폐기 commit + 클린업: **30분**

총 합: **7~12시간** (1~2 세션, backend-architect agent 와 함께 권장).

### 4.2 Risk

| 카테고리 | risk | 완화 |
|---|---|---|
| Code | ws-server 의 `Event`-기반 dispatch 가 너무 깊어 단순 mechanical replace 가 안 됨 (multi-layered semantics) | backend-architect agent 가 BackendNotify 매핑 매트릭스 §2.5 를 input 으로 받아 system-level refactor |
| Test | 50+ tests 폐기 시 *frontend 측 contract* 가 깨질 가능성 (어떤 envelope shape 가 변하는지 frontend 의 dispatcher 가 모름) | §3.3 의 new tests + Sprint 7 의 S7-WS-PAYLOAD-SIMPLIFY task 가 frontend 측을 함께 갱신 |
| Build | 단일 commit 으로 wholesale swap 시도 → 빌드 깨진 채로 중간에 멈추면 main 브랜치 더러워짐 | `worktree` 또는 별 branch `stage-b-swap` 에서 작업 후 머지 |
| Test | ws-server tests 가 mock TmuxDaemon 에 의존 — 새 tests 는 real PtyBackend 가 필요 (느림, /bin/sh 의존성) | pty-backend 의 integration test 패턴 (Gate #1~#5) 그대로 재사용 |

---

## 5. 다음 세션 시작 지시 (dispatch prompt 정본)

다음 세션 (backend-architect agent 또는 사용자 직접) 의 작업 진입점.

```
gtmux Sprint 7 Stage B step 2 — wholesale code swap.

입력 (반드시 읽을 것):
- docs/reports/0026-stage-b-carry-forward.md (본 문서) — 전체 작업 매트릭스
- docs/reports/0025-session-resume-handoff.md §4.1 — Stage B task 표
- docs/adr/0013-pty-direct-no-tmux.md (2026-05-14 amend ×3 — D3·D10·R3)
- docs/adr/0014-process-supervisor.md (2026-05-14 amend — D10 2-layer 방어)
- codebase/backend/crates/pty-backend/src/lib.rs (canonical reference)

작업:
- 0026 §3.1~§3.6 의 surgery 6건 단일 logical commit 으로 실행
- 0026 §2.2 의 순서 따라 (위→아래 의존)
- 빌드/clippy/fmt clean + 새 tests PASS 까지 진행
- 별 branch (예: stage-b-swap) 에서 작업 후 main 으로 머지 권장

DoD:
- cargo test --workspace --tests: 새 tests + 보존 tests 모두 PASS
- cargo clippy --workspace --all-targets -- -D warnings: clean
- cargo fmt --all -- --check: clean
- workspace 의 lifecycle / mux-router crate 완전 삭제 확인
- gtmux-cli 가 PtyBackend 만 사용 (gtmux_lifecycle import 0건)
- http-api 의 Hub 인터페이스 변경 없음 (publish_layout_changed 시그니처 보존)

회수 보고:
- 변경 파일 + 변경 LOC 표
- 폐기 LOC 합 (~3500 LOC 예상: lifecycle 2467 + mux-router 897 + ws-server tests ~150)
- 신규 LOC 합 (~1500 LOC 예상: 새 ws-server core + cmd_router + hub + 새 tests)
- net LOC delta (~-2000 LOC 예상)
```

---

## 6. 살아남는 carry-forward (Sprint 7 후)

본 보고서 §2.3·§2.4·§2.5 의 인터페이스 설계는 *Sprint 7 후* 에도 그대로 유효 — 본 세션의 grilling 작업이 *throw* 가 되지 않음:

- ADR-0013 D3 amend (backend ring vs frontend buffer 경계) — Stage B 의 새 hub 가 본 경계 그대로 구현
- ADR-0013 D10 amend (BackendNotify 의 ServerReady) — §2.5 매핑 표가 본 amend 반영
- ADR-0013 R3 amend (auto-cleanup) — Stage B 의 새 dispatcher 가 PaneNotFound 처리 단순화 (PaneGone 별 분기 불필요)
- ADR-0014 D10 amend (2-layer 방어) — Stage B step 1 (TMUX guard, 본 세션 완료) + Stage A 의 env scrub 이 본 amend 의 2 layer 모두 코드로 실현

---

## 변경 이력

- 2026-05-14 21:30 — 초안 — Stage B step 1 완료 시점, 다음 세션 단일 진입점으로 작성.
