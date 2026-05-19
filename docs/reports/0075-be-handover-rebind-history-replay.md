# 0075 — BE Handover: Rebind history replay (`AttachReplayEvent` 옵션 a-1)

- 작성일: 2026-05-18
- 작성 주체: agent (system-architect role, 0076 결정 후 발주)
- 정본 cross-link:
  - **결정 출처**: [`0076-rebind-history-replay-missing.md`](./0076-rebind-history-replay-missing.md) §8 의 옵션 (a-1) 채택
  - **상위 감사**: [`0071-...audit.md`](./0071-session-terminal-panel-lifecycle-audit.md) §B-2.4
  - **prior land 정합**: [`0072-be-handover-from-0071-audit.md`](./0072-be-handover-from-0071-audit.md) (C-1 naming 의 owner_key 패턴 정합 의존)
  - **관련 ADR**: ADR-0021 D7/D8 + amend ② (draft 본 handover §D) / ADR-0025 (race-immune 정합)

## 핵심 원칙 — 거짓 ship 방지

[`0072` §0 의 5 원칙](./0072-be-handover-from-0071-audit.md#핵심-원칙---거짓-ship-방지) 그대로 적용 — anchor / acceptance criteria / anti-pattern / behavior change 정확성 / self-check 표.

특히 본 task 는 *broadcast race* 영역이라 ordering invariant 검증이 가장 중요. integration test 의 *concurrent emit* 시나리오가 필수.

---

## 0. Self-grilling 결정 사항 (0076 §8 보강)

본 handover land 전 resolve 한 추가 결정:

### Q1. `backend_handle()` 의 신규 method vs 기존 `backend()` access?

**결정**: ✅ **기존 hub.backend() 사용 + AppState 가 hub 의 backend Arc 보유 여부 확인**. 신규 method 추가 회피. 이유: hub 가 이미 backend Arc 공유 중 (ws-server lib.rs:536 `backend = hub.backend().clone()`). put_layout_handler 가 같은 hub 보유 — `state.hub.as_ref().map(|h| h.backend().clone())` 패턴 사용.

### Q2. `subscribe_output(pane_id)` 의 부수 효과?

`subscribe_output` 은 *새 broadcast subscriber 등록* + *현재 ring buffer 의 replay snapshot 반환*. 본 task 는 replay 만 필요, subscriber 등록은 *원치 않음* (broadcast cap 의 dead subscriber 누적).

**결정**: ✅ **반환된 `_rx: broadcast::Receiver` 를 즉시 drop**. Rust 의 drop 으로 subscriber 자동 unregister — broadcast 의 subscriber count 자연 감소. 코드 패턴: `let (replay, _rx) = backend.subscribe_output(pane)?;` 의 `_rx` 가 scope 끝에 drop.

→ 단, `_rx` 가 같은 expression 안 evaluation 끝나면 곧 drop 되어 subscriber 가 *replay 반환 시점 직전에만* 활성. ring buffer snapshot 의 atomic 성은 PTY backend 의 mutex 가 보장.

### Q3. `bytes::Bytes` 의 clone 비용?

`Bytes` 는 Arc-based — clone 이 ref count 증가만. broadcast::send 가 N subscriber 마다 clone 호출. cap 16 의 typical case (subscriber 수 = 활성 WS connection ~1-10) 에서 clone 비용 무시.

**결정**: ✅ replay 의 `Vec<u8>` → `Bytes::from(vec)` 으로 변환 후 broadcast. WS handler 가 envelope encoding 에 same Bytes 재사용.

### Q4. cap=16 vs 더 큼?

사용자 명시 [Attach to this session] action 빈도: ~0.01/s (사용자 클릭 빈도). 또는 PUT layout 의 added=non-empty 빈도. 둘 다 sub-Hz.

**결정**: ✅ **cap=16** 유지. 1초당 16 added events 면 cap hit — 사실상 불가. 너무 크면 메모리 낭비 (Bytes Arc 누적).

### Q5. WS handler 의 select! arm 의 priority?

select! `biased` loop 의 arm 순서가 ordering 영향. attach_replay_rx 를 layout_rx 보다 *위* 에 두면 layout_events 보다 replay 가 우선 처리 — set hot-update 가 아직 안 됐어도 (a-1) 의 envelope.session 매칭 OK.

**결정**: ✅ **layout_rx 직후, output_rx 직전**. layout 의 LAYOUT_CHANGED notify 가 먼저 도착 → FE 가 GET /layout 으로 새 panel mount → attach_replay 의 PANE_OUT 이 그 직후 도착해 xterm 에 history 적용. ordering 직관적.

### Q6. 동일 session 의 multi-Webpage (cross-tab) — 둘 다 replay 받나?

(a-1): envelope.session 매칭 — 같은 cookie 의 다른 webpage_id (= 다른 owner_key) 가 같은 session 에 attach 가능? **불가능** (ADR-0019 D3 single-attach). 한 시점에 한 owner_key 만 보유. 따라서 한 WS 만 매칭 → replay 1번만 forward.

**결정**: ✅ 정합. multi-Webpage 의 cross-session mirror (다른 session 의 layout 에 같은 UUID) 는 envelope.session 이 *PUT 의 session* 만 동봉하므로 다른 session 의 WS 는 자연 차단.

### Q7. emit 실패 (cap hit 또는 channel closed) 시 disk-of-truth invariant 손상?

`hub.publish_attach_replay` 의 `broadcast::Sender::send` 가 0 subscriber 또는 cap hit 시 Err. **PUT 응답 자체는 200** — disk write + attach_index update 가 우선 진실. replay 누락은 perception 1회 손실 (ADR-0006 D13 invariant 와 별 영역).

**결정**: ✅ **broadcast send 결과 ignore**. `let _ = hub.publish_attach_replay(...)`. PUT 응답에 영향 0. tracing::debug 로만 기록.

---

## §A. Task 목록

| Task | 영역 | 출처 | 예상 소요 |
|---|---|---|---|
| **RB-A** | `AttachReplayEvent` broadcast + put_layout_handler emit + WS handler arm + 3 integration test + ADR-0021 D8 amend ② | 0076 §8 (a-1) | 1 commit |

단일 task. BE-only, FE 영향 0.

---

## §B. Anchor — 변경 대상

| # | 파일 | 변경 |
|---|---|---|
| 1 | `codebase/backend/crates/ws-server/src/hub.rs` | 신규 `pub struct AttachReplayEvent { session: String, pane_id: u64, bytes: bytes::Bytes }` + `Hub::attach_replay_events: broadcast::Sender<AttachReplayEvent>` field + `subscribe_attach_replay()` / `publish_attach_replay(session, pane_id, bytes)` public method. cap = 16 |
| 2 | `codebase/backend/crates/http-api/src/sessions.rs::put_layout_handler` | `attach_index.apply_diff(name, &added, &removed)` 직후, *added* 의 alive PaneId 의 ring buffer 를 `hub.publish_attach_replay` 로 emit (의사 코드는 0076 §8.3) |
| 3 | `codebase/backend/crates/ws-server/src/lib.rs::handle_socket` select! | `attach_replay_rx.recv()` arm 추가 — layout_rx 직후, output_rx 직전. envelope.session 매칭 시 PANE_OUT envelope forward (set filter 우회). Lagged warn / Closed break (의사 코드는 0076 §8.4) |
| 4 | `docs/adr/0021-terminal-pool-and-mirror.md` D8 | amend ② 추가 (draft 는 0076 §8.7) |
| 5 | `docs/ssot/state-machines.md` (선택) §4 또는 §6 | rebind 시 ring buffer replay 의 invariant 1줄 추가 |

---

## §C. 의사 코드 (정본)

### C-1. hub.rs

```rust
/// Payload of an [`AttachReplayEvent`] broadcast. Emitted by
/// `put_layout_handler` when a session's layout newly attaches an alive
/// `terminal_id` (e.g., via [Attach to this session]). The receiving
/// session's WS handler forwards the bytes as a single `PANE_OUT` envelope
/// so the xterm panel renders the existing ring buffer immediately
/// on mount.
///
/// `session` is the owner-scope match key — WS handlers compare it with
/// `hub.session_for_owner(self.owner)` and forward only on match. This
/// makes the broadcast race-immune against `session_pane_set` hot-update
/// timing (ADR-0025 amend ③) — the envelope itself carries the routing
/// decision.
#[derive(Clone, Debug)]
pub struct AttachReplayEvent {
    pub session: String,
    pub pane_id: u64,
    pub bytes: bytes::Bytes,
}

const ATTACH_REPLAY_BROADCAST_CAPACITY: usize = 16;

impl Hub {
    // ... 기존 fields 옆에
    attach_replay_events: broadcast::Sender<AttachReplayEvent>,

    // ... 기존 new() 안에
    let (attach_replay_events, _) = broadcast::channel(ATTACH_REPLAY_BROADCAST_CAPACITY);

    /// Subscribe to ring-buffer replay events for newly-attached terminals
    /// (ADR-0021 D8 amend ②, 0076 §8). One envelope per *added* terminal_id
    /// that resolves to an alive `PaneId`.
    pub fn subscribe_attach_replay(&self) -> broadcast::Receiver<AttachReplayEvent> {
        self.attach_replay_events.subscribe()
    }

    /// Publish a ring-buffer replay for a session's newly-attached terminal.
    /// `session` is the routing key — only WS connections whose owner-key
    /// resolves to this session forward the envelope.
    ///
    /// Failure (no subscriber, cap hit) is silent — `put_layout_handler`'s
    /// disk-of-truth invariant (ADR-0006 D13) is not affected. At most a
    /// single replay event is lost; the next live `PANE_OUT` resumes normally.
    pub fn publish_attach_replay(&self, session: String, pane_id: u64, bytes: bytes::Bytes) {
        let _ = self.attach_replay_events.send(AttachReplayEvent {
            session,
            pane_id,
            bytes,
        });
    }
}
```

### C-2. sessions.rs::put_layout_handler

기존 `attach_index.apply_diff(name, &added, &removed)` 의 *직후* (broadcast layout_events 호출 *이전 또는 직후* — Q5 의 ordering 의도 따라):

```rust
// 0076 §8 / 본 commit (RB-A) — added UUIDs 의 ring buffer 를 그 session 의 WS 에
// 1회 replay broadcast. session_pane_set hot-update timing 과 무관 (envelope
// 안 session 동봉이 routing 의 진실).
if !added.is_empty() {
    if let Some(hub) = state.hub.as_ref() {
        let backend = hub.backend().clone();
        for uuid in &added {
            let pane = match state.terminal_map.lookup_pane(uuid).await {
                Some(p) => p,
                None => continue,  // unmatched — confirm flow 영역, replay 무
            };
            let Some((replay, _rx)) = backend.subscribe_output(pane) else { continue };
            // _rx 는 scope 끝에 drop — broadcast subscriber 자동 unregister.
            // replay snapshot 의 atomic 성은 backend 의 mutex 가 보장.
            if replay.is_empty() { continue }
            hub.publish_attach_replay(
                name.clone(),
                pane.0,
                bytes::Bytes::from(replay),
            );
        }
    }
}
```

위치: `attach_index.apply_diff` 의 *직후*, layout_events 의 publish *이전* (사용자가 LAYOUT_CHANGED 받기 전에 history 가 channel 에 들어가 있도록).

### C-3. ws-server lib.rs::handle_socket select!

기존 `output_rx.recv()` arm 직전에 추가:

```rust
// 본 task 의 신규 subscriber — handle_socket 초입에 subscribe
let mut attach_replay_rx = hub.subscribe_attach_replay();
```

select! 안 (layout_rx arm 직후, output_rx arm 직전):

```rust
ev = attach_replay_rx.recv() => {
    match ev {
        Ok(ev) => {
            // owner-scope 매칭. envelope 안 session 동봉이 race-immune
            // 한 routing 의 진실 — session_pane_set filter 우회 (ADR-0025
            // 의 set hot-update timing 의존 회피, 0076 §8.5).
            let owner_session = owner_key.as_deref()
                .and_then(|o| hub.session_for_owner(o));
            if owner_session.as_deref() != Some(ev.session.as_str()) {
                continue;
            }
            let env = Envelope::new(
                FrameType::PaneOutput,
                Bytes::from(payload::encode_pane_out(
                    u32::try_from(ev.pane_id).unwrap_or(0),
                    &ev.bytes,
                )),
            );
            if let Ok(buf) = env.encode() {
                if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                    debug!("ws attach-replay send failed; peer hung up");
                    break;
                }
            }
        }
        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
            warn!(skipped = n, "ws attach-replay subscriber lagged");
        }
        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
            info!("attach-replay channel closed; ending connection");
            break;
        }
    }
}
```

---

## §D. ADR amend 짝

0076 §8.7 의 draft 그대로 `docs/adr/0021-terminal-pool-and-mirror.md` 의 D8 amend ② 로 ship. 변경 이력 entry 동봉.

추가: state-machines.md 의 §4 (Terminal connection lifecycle) 또는 §6.2 (Server-side 예외) 에 1줄 — *"`PUT /layout` 의 added UUID alive → 그 session 의 WS 에 ring buffer 1회 replay (D8 amend ②)"*.

---

## §E. Acceptance criteria

| # | 검증 명령 | 기대 결과 |
|---|---|---|
| AC-RB-A1 | `cd codebase/backend && cargo build --color=never` | clean build, 0 error/warn |
| AC-RB-A2 | `cargo test --workspace -- attach_replay\|rebind` | **3 신규 test PASS** |
| AC-RB-A3 | `cargo test --workspace --no-fail-fast 2>&1 \| tail -3` | **428 PASS / 0 FAIL** (직전 baseline 425 + 3) |
| AC-RB-A4 | `grep -n "publish_attach_replay\|AttachReplayEvent" codebase/backend/crates/ws-server/src/hub.rs` | struct 1 + method 2 (publish + subscribe) |
| AC-RB-A5 | `grep -n "subscribe_attach_replay" codebase/backend/crates/ws-server/src/lib.rs codebase/backend/crates/http-api/src/sessions.rs` | ws-server 1 hit (subscribe) + sessions.rs 직접 publish 호출 또는 hub 경유 |
| AC-RB-A6 | `git diff --stat HEAD~1 HEAD` | 변경 파일: hub.rs / lib.rs (ws-server) / sessions.rs / ADR-0021 / (선택 state-machines) — 다른 파일 0 |

---

## §F. 필수 Integration Test

3 신규 test. 이름은 *handover 명시 그대로 사용* — 다른 이름은 false ship 표지.

### F-1. `attach_existing_terminal_replays_ring_buffer_to_session_ws`

```rust
// codebase/backend/crates/http-api/src/lib.rs (#[cfg(test)] 블록)
#[tokio::test]
async fn attach_existing_terminal_replays_ring_buffer_to_session_ws() {
    // Setup: 2 session (α, β). cookie C + webpage_id W_alpha 로 α attach.
    // α 의 layout 에 terminal T 추가 + spawn + write 'echo HELLO\n' (output 캡처).
    // T 의 ring buffer 에 HELLO\n 포함.

    // Action: 같은 cookie C + W_beta 로 β attach. WS subscribe (β).
    // β 의 layout PUT 으로 T 의 panel item 추가.

    // Verify: β 의 WS 가 PANE_OUT envelope (pane_id=T.0, bytes contains "HELLO")
    //         을 *layout_events 직후* 받음 (timeout 1s).
    //         envelope encoding 의 ordering: LAYOUT_CHANGED → PANE_OUT (replay).
}
```

### F-2. `attach_existing_terminal_replay_owner_scoped`

```rust
#[tokio::test]
async fn attach_existing_terminal_replay_owner_scoped() {
    // Setup: 3 session (α, β, γ). α + T attach + output. β + γ 도 attach.
    // 3 WS connection 활성 (W_alpha, W_beta, W_gamma).

    // Action: β layout PUT 으로 T 추가.

    // Verify:
    //   β WS — PANE_OUT envelope 1회 받음 (replay)
    //   α WS — replay 안 받음 (α 는 envelope.session = "β" 매칭 실패)
    //   γ WS — replay 안 받음
    //
    // → envelope.session 동봉의 owner-scope 매칭이 cross-session 차단 보장.
}
```

### F-3. `attach_existing_terminal_replay_idempotent_for_drag_layout`

```rust
#[tokio::test]
async fn attach_existing_terminal_replay_idempotent_for_drag_layout() {
    // Setup: α attach + T 의 panel item 이 이미 layout 에 있음.
    // (T 의 ring buffer 에 history 있음).

    // Action: α layout PUT 으로 T 의 panel item 의 *위치만 변경*
    //         (terminal_id 동일, x/y 변경).

    // Verify: apply_diff 의 added = [] (drag 의 net-zero) →
    //         attach_replay broadcast emit 0회.
    //         α 의 WS 에 추가 PANE_OUT 도착 안 함 (timeout 200ms).
    //
    // → drag 의 net-zero idempotency 보장.
}
```

---

## §G. Anti-pattern (false ship 표지)

❌ **이런 fix 는 ship 거부**:

1. **`session_pane_set` filter 적용**: WS handler 의 새 arm 이 `session_pane_set.contains(pane_id)` 로 검사 → ordering race 그대로. 0076 §8.5 의 race-1 미해결. **envelope.session 매칭이 전부**.
2. **broadcast cap 무한대 또는 너무 큼**: 메모리 누수. cap=16 고정.
3. **`subscribe_output` 의 `_rx` 를 keep 함**: broadcast subscriber 누적. `_rx` 가 expression scope 안에서만 살아야 함.
4. **emit 실패 시 PUT 응답 5xx**: disk-of-truth invariant 위반. broadcast send 결과는 `let _ =` 로 ignore.
5. **session_for_owner == None 일 때 forward**: legacy demo path (cookie 없음) 의 WS 에 다른 session 의 replay 흘러감 — ADR-0025 D5 의 legacy demo path 보존과 충돌. **None ≠ Some 매칭이라 자연 차단** — 단 코드에서 `as_deref()` 의 None 처리 명확히.
6. **integration test 이름 변경**: handover 명시 3 이름 (F-1/F-2/F-3) 그대로 사용. naming creep 은 다음 reviewer 가 *어느 test 가 어느 invariant 검증* 모름.
7. **`put_layout_handler` 의 emit 위치가 disk write 이전**: disk write 실패 시 broadcast 만 발화 = stale state. **반드시 disk write + attach_index.apply_diff 성공 후**.
8. **LAYOUT_CHANGED broadcast 와 attach_replay broadcast 의 순서 reverse**: FE 가 replay 받기 전에 layout fetch 안 한 상태라 새 panel mount 안 됨 → 도착한 replay 의 PaneId 매칭 panel 없음 → 누락. **LAYOUT_CHANGED 가 먼저, attach_replay 가 다음**.

→ Q5 결정: select! 의 arm 순서가 *receive side* 의 ordering 보장. send side 도 LAYOUT_CHANGED → attach_replay 순서로 publish.

---

## §H. Self-check (commit 전 ☑)

- [ ] `hub.rs` 의 `AttachReplayEvent` struct + `attach_replay_events` field + 2 public method 추가
- [ ] `sessions.rs::put_layout_handler` 의 `apply_diff` 직후, added UUIDs 의 ring buffer publish (의사 코드 §C-2 매치)
- [ ] `ws-server lib.rs::handle_socket` 의 select! 에 `attach_replay_rx` arm 추가 (의사 코드 §C-3 매치)
- [ ] envelope.session 매칭 — `session_pane_set` filter 우회 확인 (anti-pattern #1 회피)
- [ ] AC-RB-A1 ~ AC-RB-A6 모두 PASS
- [ ] 3 integration test (F-1, F-2, F-3) 추가 + 모두 PASS — **이름 정확 매치**
- [ ] ADR-0021 D8 amend ② 동봉 commit + 변경 이력 entry
- [ ] state-machines.md (선택) §4 또는 §6.2 갱신
- [ ] commit message: `feat(be): RB-A AttachReplayEvent broadcast (0076, ADR-0021 D8 amend ②)` + baseline test 수 명시 (`425 PASS → 428 PASS`)

---

## §I. 통합 검증 — land 후

```bash
cd codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | grep "^test result" | head -20
# 기대: PASS 합 428 / FAIL 0

cargo build --release --bin gtmux --color=never
# 기대: PASS
```

---

## §J. 본 handover 가 *완전 검증* 안 한 영역

- **Manual E2E 의 시각 evidence**: BE+FE 띄워 실제 [Attach to this session] 후 β 측 xterm 의 history 표시 확인. integration test 로 wire 검증되면 사용자 perception 도 자연 정합 — 다만 *시각 확인* 은 작업자가 dev 환경에서 1회 진행 권장
- **multi-Webpage ordering**: 본 handover 의 F-1/F-2/F-3 가 ordering 도 검증하지만, `tokio::join!` 으로 동시 PUT 의 *cap hit* edge 는 미검증. 별 follow-up 필요 없음 — cap 16 + sub-Hz frequency 에서 사실상 불가능
- **FE 측 변경 0 확인**: dispatcher.svelte.ts 의 PANE_OUT routing 이 *이미* paneId 매칭이라 본 replay 의 추가 byte 가 자연 처리. FE 변경 없으나 시각 시연 시 확인 권장 (handover 0073 §F-5 의 시나리오 그대로)

---

## 변경 이력

- 2026-05-18: 초안. 0076 §8 (a-1) 결정 반영. Self-grilling 7 Q resolve. anchor + 의사 코드 + 3 integration test + ADR amend draft + anti-pattern 8 + self-check 표.
