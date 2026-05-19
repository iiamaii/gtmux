# 0076 — Rebind history replay 부재 (0071 §B-2.4 confirmed)

- 작성일: 2026-05-18
- 작성 주체: agent (system-architect role, 0071→0072/0073 land review 시 발주)
- 정본 cross-link:
  - **상위 감사 보고**: [`0071-...audit.md`](./0071-session-terminal-panel-lifecycle-audit.md) §B-2.4 (가설 출처)
  - **review 시 verify**: 본 보고서가 0073 §F (FE-E) 의 code-level verify 결과 — *(b) replay 미표시 확정*
  - **관련 ADR**: ADR-0021 D7/D8 (Terminal list UI + [Attach to this session]), ADR-0025 (PANE_OUT filter, hot-update path)
  - **paired prior handover**: [`0072-be-handover-...md`](./0072-be-handover-from-0071-audit.md), [`0073-fe-handover-...md`](./0073-fe-handover-from-0071-audit.md)

## 1. 증상

[Attach to this session] 흐름 ([Terminals] tab → alive 인 terminal T 의 row → 현 session β 로 mount):

1. β 측 layout 에 panel item 추가됨 (T 가 이미 alive process)
2. β 측 xterm 이 새 panel 에 mount
3. **xterm 이 빈 화면을 표시** — T 의 기존 ring buffer 내용 (e.g., 사용자가 α 측에서 친 `echo hello && ls`) 이 *β 측에 보이지 않음*
4. 이후 새 PANE_OUT 만 정상 mirror (사용자가 어디서든 typing 하면 양쪽 xterm 에 표시)

사용자 mental model: "이 terminal 에 history 가 있으니 mount 하면 그 history 도 보여야지" → 어긋남.

## 2. 원인 — code-level 확정

### 2.1 catch-up replay 의 단일 발화 boundary

`codebase/backend/crates/ws-server/src/lib.rs:524-540` doc comment:

```rust
/// Per-connection loop. Performs catch-up replay on attach (every alive
/// pane's ring buffer is flushed as a 0x02 PANE_OUT envelope, followed
/// by the matching `pane-spawned` NOTIFY so the frontend knows the id
/// is live), then enters the live fan-out: backend notifications +
/// multiplexed pane outputs + layout broadcasts.
async fn handle_socket(...) { ... }
```

즉 catch-up replay 는 **WS handshake 시 1회만** 발화. 이후 `select!` loop (`lib.rs:710+`) 의 각 분기 (`output_rx`, `notify_rx`, `layout_rx`, `terminal_died_rx`, `terminal_list_change_rx`, `terminal_spawned_rx`, `manipulation_rx`, `mount_cascade_rx`, `session_change_rx`) 어디에도 `backend.subscribe_output(id).0` (= ring buffer replay tuple) emit 없음.

### 2.2 [Attach to this session] 의 wire 흐름

1. FE: `PUT /api/sessions/β/layout` (β layout 의 items 에 `{type:"terminal", terminal_id: T}` 추가)
2. BE: `put_layout_handler` (sessions.rs)
   - ETag CAS → disk write
   - `attach_index.apply_diff(β, [T], [])` — *added* T
   - `hub.publish_layout_changed(β)` → `layout_events` broadcast
3. β 측 WS connection 의 `layout_rx.recv()` 분기:
   - LAYOUT_CHANGED envelope 만 forward — payload 는 etag 알림 only
   - **ring buffer 의 replay 발화 0**
4. β 의 `session_pane_set` hot-update:
   - `terminal_spawned_rx.recv()` 또는 `layout_rx.recv()` 에서 set 에 T 의 PaneId 추가 (ADR-0025 D3 의 hot-update)
5. FE: LAYOUT_CHANGED envelope 받고 `GET /api/sessions/β/layout` 으로 refetch (또는 optimistic)
6. β 측 PanelNode mount + xterm.write 호출 — **ring buffer 없음 → 빈 화면**
7. 이후 live PANE_OUT 만 mirror (set 에 PaneId 가 있으므로 filter 통과)

### 2.3 catch-up 의 자연 회복도 안 됨

`ADR-0025 amend ③` 의 catch-up replay 는 *cookie 의 session 이 바뀐 reconnect* 시 cold-load 후 1회 발화 — *layout 의 추가는 reconnect 와 별개*. β 측 사용자가 강제 WS reconnect 안 하면 영구히 빈 화면.

→ FE 가 명시 reconnect 또는 page reload 하면 catch-up replay 가 fresh 로 발화하면서 그 시점의 ring buffer 가 들어와 history 가 *그제서야* 표시되긴 함. 그러나 사용자 직관과 mismatch — *mount 한 직후* 보여야 하는데 *reconnect 해야* 보임.

## 3. 영향도

- **사용자 perception**: ADR-0021 D7 (Terminal list + [Attach] action) 의 *핵심 가치* 가 절반 손상 — "다른 session 의 alive terminal 을 현 canvas 에 표시" 의 *시각적 history* 가 결락
- **빈도**: 다른 webpage 가 같은 cookie 의 다른 session 으로 [Attach to this session] 클릭 — single-user multi-tab 의 자주 발생할 시나리오
- **자가 회복**: page reload 또는 WS force reconnect 시만 회복

## 4. 권장 fix — 3 옵션

### 옵션 (a) — Layout PUT 의 *added* UUIDs 에 대해 ring buffer 1회 broadcast (P0 권장)

**위치**: `codebase/backend/crates/http-api/src/sessions.rs::put_layout_handler` 의 `attach_index.apply_diff(name, &added, &removed)` 직후

**의사 코드**:
```rust
// added UUIDs 의 alive PaneId 의 ring buffer 를 그 session 의 WS 로 1회 emit.
// hot-update path 의 *replay piggy-back*. owner-scoped — 다른 session 의 WS 에는 안 보냄.
for uuid in &added {
    if let Some(pane_id) = state.terminal_map.lookup_pane(uuid).await {
        if let Some(hub) = state.hub.as_ref() {
            if let Some(backend) = hub.backend_handle() {  // 또는 hub 가 적당한 access 제공
                if let Some((replay, _rx)) = backend.subscribe_output(pane_id) {
                    if !replay.is_empty() {
                        hub.publish_attach_replay(&name, pane_id.0, replay);
                    }
                }
            }
        }
    }
}
```

- 신규 broadcast 채널 `attach_replay_events: broadcast::Sender<AttachReplayEvent>` 추가 (또는 기존 layout_events 채널에 piggy-back)
- WS handler 의 `select!` 에 새 arm 추가 — `session_pane_set.contains(pane_id)` 통과 시 PANE_OUT envelope 으로 emit

**장점**:
- 사용자 mental model 그대로 — mount 즉시 history 표시
- 비용 낮음 — *added UUID 마다* 1회 ring read + emit
- 기존 PANE_OUT envelope 재사용 — FE 변경 0

**단점**:
- 모든 layout PUT 마다 added 검사 (대부분은 empty added — drag 시 added=[]) — 의외 비용 ~0
- broadcast 채널 1개 추가

### 옵션 (b) — 별 endpoint `POST /api/terminals/<uuid>/replay` (P1, FE 명시 호출)

**위치**: `codebase/backend/crates/http-api/src/terminals.rs`

```rust
/// Returns the current ring-buffer contents of an alive terminal as a binary
/// blob. Caller-driven, idempotent. FE 가 [Attach to this session] 직후
/// 새 panel 의 xterm 에 prepend.
pub async fn replay_handler(...) -> Response {
    // 1. validate caller owns the session that just added this UUID
    // 2. terminal_map.lookup_pane(uuid) → PaneId
    // 3. backend.subscribe_output(pane_id) → (replay, _rx)
    // 4. return Bytes (Content-Type: application/octet-stream)
}
```

FE 측: `PanelNode.svelte` mount 시 `if isNewlyAttachedFromList(item)` 면 fetch + xterm.write.

**장점**: 명시 호출 — control flow 명확. WS broadcast 의 부수 효과 없음
**단점**: round-trip 1회 추가 + FE 의 *언제 호출할지* state 관리 (newly-added flag) 필요

### 옵션 (c) — FE 에서 layout PUT 응답 직후 WS force-reconnect (P2, UX hack)

작업 단순하지만 UX 거슬림 (잠시 disconnect 후 reconnect). 권장 X.

---

## 5. ADR amend 후보

**ADR-0021 D8 amend** (Terminal binding UI) — 옵션 (a) 채택 시:

```markdown
### D8 amend ② — [Attach to this session] 흐름의 ring buffer replay (2026-05-18, 0076)

[Attach to this session] (D7 의 [Terminals] tab) 또는 [Change terminal] (D8 본문) 으로 layout 에 기존 alive terminal 의 UUID 가 added 되면, BE 가 *그 session 의 WS 에* 그 시점의 ring buffer 를 1회 broadcast (PANE_OUT envelope). 의도: 사용자 mental model "이 terminal 의 history 가 그대로 보임" 보장.

- Trigger: `put_layout_handler` 의 attach_index.apply_diff 의 `added` UUIDs 중 alive PaneId 보유한 것
- Channel: 신규 `attach_replay_events` broadcast (또는 layout_events piggy-back)
- Scope: owner-scoped — 그 session 의 WS 만 받음 (session_pane_set filter)
- Idempotency: same-PaneId 의 reattach 는 ring buffer 의 *그 시점 snapshot* 을 1회 emit. 다음 byte 부터는 normal live broadcast
- FE: 변경 0 — 기존 PANE_OUT routing (paneOutHandlers) 이 자연 처리

본 amend 가 없으면 [Attach to this session] 직후 β 측 xterm 이 빈 화면. catch-up replay 는 WS handshake 시 1회만 발화 (lib.rs:524-540 doc) — layout PUT 의 add 시점에는 발화 0 (0076 §2.2).
```

---

## 6. Test plan

### 6.1 BE integration test

```rust
#[tokio::test]
async fn attach_existing_terminal_emits_replay_to_session_ws() {
    // 1. Setup: 2 session (α, β). α attach + spawn T + write some output → ring buffer 에 N bytes
    // 2. β attach via owner_key_2. WS handshake → catch-up replay (β 의 layout 은 빈 layout)
    // 3. PUT /api/sessions/β/layout (β 에 T 의 panel item 추가)
    // 4. β 측 WS 에서 PANE_OUT envelope (paneId=T.0) + bytes==그 시점 ring buffer 수신 확인
    // 5. 추가 byte (α 에서 새 명령) → 정상 mirror
}

#[tokio::test]
async fn attach_existing_terminal_replay_owner_scoped() {
    // 1. session α + β. α 에 T attach + output. γ (3번째 session) attach.
    // 2. β layout PUT 으로 T 추가
    // 3. β WS 에 PANE_OUT 도착 / γ WS 에는 안 도착 (session_pane_set filter)
}

#[tokio::test]
async fn attach_existing_terminal_replay_idempotent_for_dragged_layout() {
    // 1. β layout 에 T 가 이미 있는 상태에서, drag 으로 위치 변경 PUT
    // 2. apply_diff 의 added == [] → replay 발화 0
    // 3. β WS 에 추가 PANE_OUT 없음
}
```

### 6.2 Manual E2E

0073 §F 의 시나리오 그대로 — 단 step 6 의 *기대* 가 (a) replay OK 로 변경.

---

## 7. 우선순위 + 발주 권장

- **P0** (옵션 a 선택 시): BE+FE pair commit. ADR-0021 D8 amend ② + put_layout_handler 의 added replay broadcast + 3 신규 integration test
- **P1** (옵션 b 선택 시): BE replay endpoint + FE PanelNode 의 newly-added trigger
- 결정 권한: **사용자**. 옵션 (a) vs (b) 의 trade-off — broadcast piggy-back vs FE 명시 호출

본 report 는 0071 §B-2.4 의 verify 결과 + follow-up work package 발주. 다음 BE/FE worker handover 에 통합 가능.

---

## 8. 최종 결정 (2026-05-18 amend ①) — **옵션 (a-1) session-aware `AttachReplayEvent`**

원안 (a) plain broadcast 의 set hot-update *ordering race* 를 제거한 변형. 사용자 결정 — 0071 의 "예외 상황에도 강건한 설계" 요구와 정합.

### 8.1 핵심 차이 vs 원안 (a)

| 차원 | 원안 (a) plain | **(a-1) session-aware** |
|---|---|---|
| Envelope | `(pane_id, bytes)` only | `(session, pane_id, bytes)` |
| WS forward 조건 | `session_pane_set.contains(pane_id)` | `hub.session_for_owner(owner) == ev.session` |
| Set hot-update race | ⚠️ ordering 의존 | ✅ filter 우회 — race 면역 |

### 8.2 신규 broadcast 채널

```rust
// ws-server/src/hub.rs
pub struct AttachReplayEvent {
    pub session: String,   // owner-scope 매칭의 진실
    pub pane_id: u64,
    pub bytes: bytes::Bytes,
}

attach_replay_events: broadcast::Sender<AttachReplayEvent>,   // cap 16
pub fn subscribe_attach_replay(&self) -> broadcast::Receiver<AttachReplayEvent>;
pub fn publish_attach_replay(&self, session: String, pane_id: u64, bytes: Bytes);
```

cap 16: 사용자 명시 [Attach to this session] action 빈도가 sub-Hz 라 충분.

### 8.3 emit 시점 — `put_layout_handler`

```rust
// http-api/src/sessions.rs::put_layout_handler
// (기존 disk write + attach_index.apply_diff 직후)
for uuid in &added {
    let pane = match state.terminal_map.lookup_pane(uuid).await {
        Some(p) => p,
        None => continue,  // unmatched UUID — confirm flow 영역, replay 없음
    };
    let backend = match state.hub.as_ref().and_then(|h| h.backend_handle()) {
        Some(b) => b,
        None => continue,  // hub 없음 (test path 등)
    };
    let Some((replay, _rx)) = backend.subscribe_output(pane) else { continue };
    if replay.is_empty() { continue }
    state.hub.as_ref().unwrap()
        .publish_attach_replay(name.clone(), pane.0, replay);
}
```

`backend_handle()` 추가 필요 시 hub 에 별 method — 또는 hub 가 backend Arc 보유 중이면 직접 access.

### 8.4 WS handler 의 select! arm

```rust
// ws-server/src/lib.rs::handle_socket 의 select! loop
ev = attach_replay_rx.recv() => {
    match ev {
        Ok(ev) => {
            let owner_session = owner_key.as_deref()
                .and_then(|o| hub.session_for_owner(o));
            if owner_session.as_deref() != Some(ev.session.as_str()) {
                continue;
            }
            // owner-scoped 매칭 — set filter 우회.
            // envelope 안 session 동봉이 다른 session 의 WS 차단 보장.
            let env = Envelope::new(
                FrameType::PaneOutput,
                Bytes::from(payload::encode_pane_out(
                    u32::try_from(ev.pane_id).unwrap_or(0),
                    &ev.bytes,
                )),
            );
            if let Ok(buf) = env.encode() {
                if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                    break;
                }
            }
            // session_pane_set hot-update 는 layout_rx 분기가 별도 처리.
            // 본 arm 은 forward 만.
        }
        Err(broadcast::error::RecvError::Lagged(n)) => {
            warn!(skipped = n, "ws attach-replay lagged");
        }
        Err(broadcast::error::RecvError::Closed) => break,
    }
}
```

### 8.5 Race 매트릭스 — (a-1) 의 강건성

| Race | (a) 영향 | (a-1) 영향 | 회복 |
|---|---|---|---|
| layout_events 와 attach_replay 도착 순서 비결정 | set filter 가 race 의존 | ✅ filter 우회 | N/A — race 자체 없음 |
| catch-up 진행 중 PUT 도착, replay envelope 가 buffer 에 쌓임 | 같음 | 같음 — 단 자기 session 매칭만 검사 | duplicate 위험: catch-up 의 ring buffer + replay 의 ring buffer 가 같은 PaneId 면 history 2번. 단 *명시 [Attach] action 후 reconnect* 의 좁은 window 만 발생 |
| alive PaneId 가 PUT 와 replay broadcast 사이에 die | stale bytes emit | 같음 — 단 의미 정합 ("die 직전의 마지막 출력") | terminal_died broadcast 가 곧 Dangling overlay |
| 다른 session 의 WS 도 added UUID 매칭 (cross-session mirror, ADR-0021 D1) | set filter 가 차단 | envelope.session 매칭이 차단 | 정합 보존 |
| broadcast cap 16 초과 | drop — duplicate 더 큼 | Lagged warn — 사용자 perception: history 누락 1회. 다음 PANE_OUT 부터 정상 | sub-Hz frequency 라 cap hit 사실상 불가 |
| auth 만료 / WS close 직전 emit | dead subscriber drop | 같음 | 자연 정합 |
| Server SIGTERM 중 PUT | 5xx 또는 broadcast 미emit | 같음 | by-design — 사용자 retry 또는 다음 BE start 의 AttachConfirmModal |

→ 단일 race 도 **data corruption 0, lock leak 0**. 최악 perception 손실 = history 1번 누락 (cap hit, 사실상 불가) 또는 1번 duplicate (catch-up reconnect race, edge).

### 8.6 Idempotency

`apply_diff(name, added, removed)` 의 `added` 정의 = "본 PUT 의 prev layout 에 없었던 UUID". 따라서:
- Drag (위치 변경) → added=[] → emit 0
- Same UUID 재추가 (remove → re-add, 사용자 명시) → emit 1회 — 사용자 mental model 정합 ("다시 mount 하니 history 다시 보임")

자연 idempotent.

### 8.7 ADR amend 짝 — ADR-0021 D8 amend ② (draft)

```markdown
### D8 amend ② — [Attach to this session] / [Change terminal] 시 ring buffer replay (2026-05-18, 0076)

D7 의 [Attach to this session] (TerminalListView row action) 또는 D8 본문의 [Change terminal] 으로 *기존 alive UUID* 가 layout 에 added 되면, BE 가 그 시점의 ring buffer 를 *그 session 의 WS 에* 1회 broadcast (PANE_OUT envelope). 사용자 mental model "이 terminal 의 history 가 그대로 보임" 보장.

- Channel: 신규 `attach_replay_events: broadcast::Sender<AttachReplayEvent>` (cap 16)
- Envelope: `AttachReplayEvent { session: String, pane_id: u64, bytes: Bytes }` — owner-scope 매칭의 진실
- Trigger: `put_layout_handler` 의 `attach_index.apply_diff(name, added, removed)` 의 *added* UUIDs 중 `terminal_map.lookup_pane(uuid).await` 가 Some 인 것
- Forward 조건: WS handler 의 select! arm 이 `hub.session_for_owner(owner) == ev.session` 매칭 시 forward. **`session_pane_set` filter 우회** — envelope 의 session 동봉이 충분 검증 (ADR-0025 의 race-1 ordering 면역)
- Idempotency: `apply_diff` 의 added 가 *진짜 신규* 인 경우만 emit. drag 의 net-zero (added=[]) → emit 0. same-UUID re-add → emit 1회 (사용자 정합)
- Fault tolerance: hub 없음 / pane lookup 실패 / ring 비어있음 → continue (skip). broadcast cap hit → Lagged warn + 사용자 perception 1회 누락 (다음 PANE_OUT 부터 정상)
- 거절된 대안 (옵션 b — HTTP endpoint): replay 와 live PANE_OUT 의 ordering race + FE buffer state 필요. (a-1) 의 broadcast piggy-back 보다 검증 표면 큼

본 amend 가 없으면 [Attach to this session] 직후 β 측 xterm 이 빈 화면 — catch-up replay 가 WS handshake 1회만 발화 (lib.rs:524-540 doc) 라 layout PUT 의 add 시점 emit 발화 0 (0076 §2.2).
```

### 8.8 발주

본 결정으로 후속 BE work package handover `0075-be-handover-rebind-history-replay.md` 발주.

---

## 변경 이력

- 2026-05-18: 초안. 0073 §F (FE-E) manual verify (code-level trace) 결과 = (b) replay 미표시 확정. 옵션 (a)/(b)/(c) + ADR amend 후보 + 3 BE integration test 명세.
- 2026-05-18: amend ① — **옵션 (a-1) session-aware `AttachReplayEvent` 채택**. envelope 에 `session: String` 동봉 + WS handler 가 `session_pane_set` filter 우회. set hot-update ordering race 면역. 7 race 시나리오 매트릭스 + idempotency 분석 + ADR-0021 D8 amend ② draft + 발주 (0075).
