# ADR-0021: Terminal pool + multi-session mirror (ADR-0015 amend)

- 상태: Accepted (2026-05-15)
- 일자: 2026-05-15 (Proposed + Accepted, plan 0006 의 multi-session pivot grilling 결과)
- 결정자: agent (system-architect role) + user grilling
- 근거 grilling: 2026-05-15 plan 0006 grilling 의 Q9 (terminal multi-attach + 입력 공유) / Q10 (auto-mount trigger session 만) / Q11 (session lifecycle) / Q13 (heartbeat) / Q14 (활성 UI) / Q15 (takeover 금지)
- 근거 plan: `docs/plans/0007-multi-session-pivot.md`
- Amends: ADR-0015 (Pane auto-mount — cascade 정책 의 trigger-session 한정), ADR-0002 D3 (MT-3 server-wide → session-scoped + server-wide 2-layer)
- 관련 ADR: ADR-0019 (Session+Workspace Model), ADR-0018 (Canvas Item Data Model — match-or-spawn), ADR-0013 (PTY direct + tokio::broadcast — N:N 멀티 attach 의 자연 구현), ADR-0014 (Process supervisor)
- 관련 SSoT: `docs/ssot/wire-protocol.md` (WS frame 의 session-scoped 분리, heartbeat ping/pong amend)

## 맥락

ADR-0015 (Pane auto-mount) 는 single-session 시대의 정합 — *server 가 새 pane spawn 하면 모든 frontend 가 dispatcher hook 으로 cascade PUT*. multi-session pivot (ADR-0019) + 사용자 명시 *"webpage 별 독립적인 layout"* 으로 이 정책이 깨진다 — *어느 webpage 의 layout 에* mount 할지의 분기.

또 사용자 명시:
- "**1 terminal 이 여러 webpage 의 panel 에 동시 attach 가능 + 입력도 공유 (full mirror)**" (Q9)
- "**Trigger session 에만 auto-mount**" (Q10)
- "**Session 의 active 는 webpage attach 만, file record 는 영속**" (Q11)

PTY 자체는 (사용자 명시) 본래 multi-attach 자연 — kernel 의 master FD multiplex + tokio::broadcast subscriber 의 N:N 패턴 (ADR-0013 D11). 즉 *기술 layer 는 추가 처리 X*, *application semantic* 만 잠그면 됨.

본 ADR 은 그 application semantic 을 잠근다.

## 결정 (Decisions)

### D1. Terminal : Panel = 1:N (multi-session mirror)

- 한 Terminal 은 0 개 이상의 Panel 에 attach 가능.
- attach 된 모든 Panel 은 같은 PTY output stream 받음 (server-wide broadcast, ADR-0013 D11 의 tokio::broadcast).
- attach 된 모든 Panel 은 같은 shell 로 input forward (어디서 타이핑하든 같은 echo).
- Panel 들은 같은 Session 의 둘일 수도, 다른 Session 들일 수도, 또는 둘 다 (mix) 가능.

### D2. Terminal output / input 의 server-scoped broadcast

```
PTY master FD
    ↓ byte stream
backend reader task
    ↓ tokio::broadcast.send(bytes)
    ↓ N subscribers (= terminal 을 attach 한 모든 Panel 들의 WS 전송 채널)
    ↓
Panel A (Session 1) ←─ broadcast ─┐
Panel B (Session 1) ←─ broadcast ─┤  같은 stream, N 갈래
Panel C (Session 2) ←─ broadcast ─┘
```

- broadcast cap (기존 ADR-0002 D7: 512) 도달 시 자연 backpressure (Suspended subscriber drop).
- 입력 (`PTY_INPUT` frame) 은 *어느 webpage 에서 와도* 같은 PTY master FD 로 write — multi-source merge OK (kernel level).

### D3. Auto-mount 는 trigger session 의 layout 에만 (ADR-0015 amend)

옛 ADR-0015: server 가 새 pane spawn → 모든 frontend dispatcher hook → 모든 active webpage 의 layout 에 cascade PUT.

새 정책:
- 사용자가 webpage W 의 [New Terminal] 누름 → backend 가 새 Terminal spawn → **그 webpage W 가 attach 중인 Session 의 layout 에만** cascade PUT (= auto-mount).
- 다른 active webpage 들 (다른 session 의) 은 *Terminal list 갱신만* 받음 (새 terminal 이 server-pool 에 추가됐다는 알림).
- 그 webpage 들의 active session layout 에는 *건드리지 않음*.

#### Auto-mount 의 server-side 알고리즘

```
on Webpage W in Session S clicks [New Terminal]:
    1. server spawn new Terminal T (ADR-0014, GTMUX env marker)
    2. server emit NOTIFY pane-spawned { terminal_id, trigger_session: S }
    3. dispatcher hook (server-side, NEW):
         for each active webpage W' in session S':
             if S' == S:
                 send WS_FRAME mount-cascade
                   { terminal_id, x, y, w, h }
                   (frontend 의 appendItemIfMissing 로 layout PUT)
             else:
                 send WS_FRAME terminal-list-update
                   { added: [terminal_id] }
                   (frontend 의 Terminal list UI 만 갱신)
```

이는 ADR-0015 의 dispatcher hook 의 *cascade target 분기* — 한 단계 정밀화.

#### D3 amend ② — `0x86 MOUNT_CASCADE` wire 의 `trigger_session` (2026-05-18, 0072 BE follow-up §1)

위 알고리즘 의 step 3 에서 *server-side 필터* 만 의존하면 **session-switch
race** 가 생긴다:

```
T0  webpage W (owner=K) 가 session A 에서 [New Terminal] click
T1  BE spawn + publish MountCascade(trigger_session=A)
T2  WS dispatcher: hub.session_for_owner(K) = A → match → frame 전송
T3  frame 비행 중
T4  사용자가 session B 로 switch (attach_handler 가 owner K → B 갱신)
T5  frame 도착, FE 가 sessionStore.active=B 로 add → wrong session
T6  결과: terminal 이 trigger 의도 session 이 아닌 B 의 layout 에 등록
```

`hub.session_for_owner` 는 T2 시점 상태 — 그 후 owner 가 switch 해도 frame
은 이미 sink 에 들어갔다. FE 가 검증할 수단이 필요.

**결정**: `0x86 MOUNT_CASCADE` 의 JSON body 에 `trigger_session: string`
필드 추가. FE `handleMountCascade` 가 `decoded.triggerSession !==
sessionStore.active?.name` 이면 drop (console.warn). 옛 body
`{terminal_id, x, y, w, h}` 는 `{trigger_session, terminal_id, x, y, w, h}`
로 확장.

```
새 wire shape:
  varint 0 + UTF-8 JSON {
    "trigger_session": "<session-name>",   // ← 신규
    "terminal_id": "<uuid>",
    "x": <num>, "y": <num>, "w": <num>, "h": <num>
  }
```

**호환성**: 신규 필드 *추가*만 — 옛 FE 가 새 BE 와 만나도 body 의 다른 field
모두 그대로 읽고 새 field 만 무시. 단, 옛 FE 는 race 검출 못 함 — paired
ship 권장. 본 ADR amend 의 land commit 이 BE encoder + FE decoder/handler
를 동시 변경.

### D4. Session attach 시 layout 의 Terminal binding (ADR-0018 D6 와 정합)

ADR-0018 D6 의 match-or-spawn 흐름. 본 ADR 의 D2 (terminal multi-attach) 와 정합:
- 한 Terminal 이 이미 다른 Session 의 Panel 에 attach 중일 수 있음 — 새 attach 는 *그저 subscriber 하나 추가* (kernel + broadcast 의 자연).
- Frontend 의 xterm 인스턴스는 *(session, panel)* 쌍 단위로 mount. 한 사용자가 여러 탭으로 같은 terminal 의 다른 view 를 볼 수 있음.

### D5. Session-scoped state (M / I / Viewport / Focus) — server-wide 폐기

이전 (ADR-0002 D3 MT-3): M/I/Viewport/Focus 가 server-wide 단일. 모든 WS 연결이 mirror.

새 정책 (본 ADR 의 D5):
- M / I / Viewport / Focus 모두 **Session-scoped**.
- 각 session 의 attached webpage (1 개, ADR-0019 D3) 와만 양방향 sync.
- 다른 session 들은 자기 별도 M/I/Viewport.

#### WS frame 의 분리

```
구 (ADR-0002 §D3):
  WS frame: { type: "selection-changed", panels: [...] }  // server-wide

새:
  WS frame: { type: "selection-changed", session_id: "...", panels: [...] }
  // server 가 frame 을 *그 session 의 attached webpage* 에만 send.
  // 다른 webpage (다른 session) 는 못 받음.
```

#### Terminal-wide frame 은 그대로 broadcast

```
WS frame: { type: "pane-output", terminal_id: "...", bytes: "..." }
// server-wide broadcast — 그 terminal 을 attach 한 모든 webpage (모든 session) 에 send.
```

### D6. Webpage heartbeat (15s ping / 30s timeout)

- WS handshake 직후 server 가 15초마다 PING frame (RFC 6455 0x9).
- Client 의 axum/tungstenite/Svelte 가 자동 PONG (RFC 6455 0xA) — 별 application 코드 X.
- 2 cycles 연속 PONG 부재 (= 30초 무응답) 시 server 가 WS close + 그 session 의 active=false.
- 정상 close (브라우저 탭 닫기, 명시 logout) 는 close frame 으로 즉시 감지.
- 보조: `beforeunload` 의 `navigator.sendBeacon('/api/leave')` (best-effort) — server 가 owner-key 기반 attach 즉시 해제. wire shape 는 §D6 amend ② 참조.

#### Heartbeat 부하

| 차원 | 비용 |
|---|---|
| Per connection memory | ~수 KB |
| Per second traffic | 100 conn 기준 6.6 pps (15s 간격) |
| 100 sessions broadcast | < 100 μs CPU |

→ 부담 0. 수십~수백 connection 안전.

#### D6.1 FE-side liveness watchdog (2026-05-16 amend, 묶음 D ship)

server-driven PING/PONG 은 browser 의 WebSocket implementation 이 자동 처리하므로 FE 측 application 코드 0 — 그러나 **(a) page-level liveness perception** + **(b) plan-0008 Phase 2 silent reattach 의 idle detection trigger** 를 위해 FE 가 별 application-level watchdog 을 운영한다.

**`lib/ws/heartbeat.svelte.ts` 신규** (commit `7703b19` 묶음 D):

| 필드 | 의미 |
|---|---|
| `lastFrameAt: number` | 마지막 server WS frame (0x02 PANE_OUT / 0x07 NOTIFY_MIRROR / 0x80~0x88 등) 수신 timestamp. 0 = mount 직후 아직 미수신. |
| `lastActivityAt: number` | 마지막 user 입력 timestamp (window keydown / mousedown / touchstart). 0 = mount 직후 아직 미입력. |
| `isStale = $derived(lastFrameAt > 0 && now - lastFrameAt > 30_000)` | server liveness watchdog — 30s+ application frame 없음. RFC 6455 PING/PONG 와 별 차원 (browser auto-PONG 은 사용자 perception 없음). |
| `isIdle = $derived(lastActivityAt > 0 && now - lastActivityAt > 15_000)` | **Phase 2 의 Case II trigger 입력**. visibility 변경 + isIdle 합집합 충족 시 silent reattach. |
| `start() / stop()` | page mount/unmount. 1s 틱 + activity listener bind/unbind. idempotent. |
| `markFrame()` | dispatcher 가 매 WS frame 수신 시 호출. |
| `markActivity` (internal) | listener 가 자동. |
| `reset()` | silent reattach 성공 후 fresh baseline (idle counter 초기화). |

`window.__gtmuxDebug.snapshot()` 의 dev-only instrumentation 으로 카운터 노출 (`xterm.fit`, `canvas.setViewport`, `flowNodes.rebuild` 등 — 0048 검증 체크리스트 §1).

#### D6.2 BE 측 D6 ping/pong 구현 상태 (2026-05-16 amend ② — ship 정합)

**현 상태: ✅ ship**. BE end-to-end wire 가 모두 완료:

| Layer | 위치 | 동작 |
|---|---|---|
| WS server | `crates/ws-server/src/lib.rs` `handle_socket` | `Hub::heartbeat_timings()` snapshot (production default 15s ping / 30s pong-timeout, `HeartbeatTimings::default()`). `tokio::time::interval(ping_interval)` 매 tick 마다 `last_pong.elapsed() > pong_timeout` 검사 — 초과 시 `Close(1011 INTERNAL "heartbeat timeout")` send + `return` (자연스럽게 disconnect_sink fire). 정상 tick 은 `Message::Ping(Bytes::new())` send. `Message::Pong` 수신 시 `last_pong = Instant::now()` + `emit_heartbeat(sink, cookie)`. `Message::Ping` 수신 시 자동 Pong 회신 + emit_heartbeat. |
| Hub | `crates/ws-server/src/hub.rs` | `heartbeat_tx` slot + `set_heartbeat_sink` setter. **`heartbeat_timings` slot 신규** (2026-05-16 amend ②) — production default `HeartbeatTimings::default()`, 테스트 `set_heartbeat_timings(...)` 으로 ms 단위 override (per-WS-upgrade snapshot). |
| CLI wiring | `bin/gtmux-cli/src/main.rs:432-475` | `(heartbeat_tx, heartbeat_rx)` mpsc + `hub.set_heartbeat_sink(heartbeat_tx)` + `tokio::spawn` consumer task → `app_state.refresh_lease_for_cookie(&cookie).await`. 동일 패턴으로 `disconnect_tx` → `release_lock_for_cookie`. |
| http-api | `crates/http-api/src/lib.rs:237-249` `AppState::refresh_lease_for_cookie` | reverse-map `session_locks_by_cookie` 에서 cookie ↔ name 찾아 `session_lock` guard 의 `refresh_lease(ws_conn_id)` 호출. lease body 의 `lease_until_unix` 갱신. |
| session_lock | `crates/http-api/src/session_lock.rs:118` `refresh_lease` | flock body rewrite — peek 시 modal 의 "expected expiry" hint 정확성 유지. |

**테스트 cover** (2026-05-16 amend ②):
- `heartbeat_timeout_closes_and_emits_disconnect` (ws-server) — `set_heartbeat_timings(100ms ping / 300ms pong-timeout)` 으로 disconnect_sink emit 검증 (load-bearing, *strict*). graceful close frame (1011 / `CloseCode::Error`) 은 best-effort — parallel cargo-test 의 runtime 경합에서 `sink.send(close_frame)` 이 socket teardown 과 race 가능, client 가 `None` 보이는 케이스 허용 (`matches!(close_code, None | Some(CloseCode::Error))`). 본 test 의 contract = "timeout → disconnect", graceful 1011 frame 은 hint.
- `heartbeat_pong_reply_emits_heartbeat_sink` (ws-server) — auto-pong 흐름이 heartbeat_sink 에 cookie emit 검증.
- `refresh_lease_for_cookie_bumps_lease_until` (http-api) — http-api 측 lease-body refresh unit-level.

**효과**:
- abrupt close (browser crash, OS kill) 시 cookie 가 보유한 lock 이 **30s 내 자동 release** — 0046 의 same-cookie idempotent 와 짝으로, 다른 webpage 의 takeover-style attach 도 ≤30s 안 가능.
- D6.1 의 FE-side watchdog 과 별 차원으로 동작 — BE 가 transport-level 진실, FE 는 application-level perception.

#### D6 amend ② — `POST /api/leave` sendBeacon endpoint (2026-05-18, 0071 §D-5)

D6 본문은 `navigator.sendBeacon('/api/leave')` 를 *"보조: best-effort"* 회수
경로로 잠궜으나 BE 측 endpoint 가 미구현이었다. 정상 탭 close 도 30s heartbeat
timeout 까지 lock 잔존 — 다른 webpage 가 같은 session 진입 시 회수 지연. 본
amend 가 endpoint shape 를 잠근다.

| 항목 | 값 |
|---|---|
| Method / Path | `POST /api/leave?webpage_id=<id>` |
| Auth | `/api/*` middleware (bearer 또는 cookie) — 통과 못 하면 401 |
| Body | 없음 (Content-Type: text/plain — sendBeacon default) |
| 응답 | `204 No Content` (성공, idempotent) / `401 Unauthorized` |
| 동작 | `release_lock_for_owner(owner_key)`. 보유 lock 없으면 no-op. |

`webpage_id` 는 URL **query** 로 전달 — `sendBeacon` 이 custom header 를 못 보내기
때문 (`X-Gtmux-Webpage-Id` 헤더 대안 불가). WS handshake 의
`?webpage_id=<id>` (ADR-0019 D5.6) 와 동일 채널 정합. server-side 의
owner_key 형성은 cookie + 0x1f + webpage_id 로 다른 path 와 똑같음.

**`DELETE /api/sessions/{name}/attach` 와의 의미 차이**: wire 는 다르지만
처리 함수는 같음. `/api/leave` = *page-unload best-effort* (URL 만 들고
오는 sendBeacon 채널, body 없음); `DELETE /attach` = *명시 user action* 의
reliable channel (Path 에 session name 명시). 둘 다 같은
`AppState::release_lock_for_owner` 를 호출.

**Anti-rate-limit**: 단일 사용자 환경 + cookie auth 통과 + page-unload 시점
1회 호출 — brute-force 표면 0. P1+ 에서 재검토.

**검증 테스트** (`crates/http-api/src/lib.rs::tests`):
- `leave_releases_lock_for_owner` — happy path, 후속 GET /sessions 의
  `active=false` 확인.
- `leave_idempotent_when_no_lock` — 보유 lock 없는 owner 호출도 204.
- `leave_requires_auth` — bearer/cookie 없으면 401.
- `leave_releases_only_matching_owner` — 같은 cookie 의 두 webpage 가 각자
  session 보유 시 한쪽 `/api/leave` 가 다른 쪽 lock 영향 없음.

### D7. Terminal list UI 의 server-wide 노출

각 webpage (session attach 후) 는 *server-pool 의 모든 alive Terminal 목록* 을 볼 수 있음:
- ID, label, status (alive/idle/dead)
- 현재 attach 중인 panel 의 수 + session 이름들 (사용자 mental model 도움)
- 버튼: [Attach to this session] (현재 attach 중인 webpage 의 session 에 panel 로 추가)

#### D7 amend ③ (2026-05-18, 0066 §BE-2 / 0067 Phase 4 / 0068 work package): `attach_count` / `attached_sessions` 의 attach reverse index

`GET /api/terminals` 의 응답 행 `attach_count` + `attached_sessions` 은 *모든 session file 의 layout 안 terminal_id 의 cross-reference*. 옛 구현 (`crates/http-api/src/terminals.rs::scan_session_terminal_refs`) 은 매 GET 마다 `enumerate_sessions` + 동기 `std::fs::read` + `serde_json::from_slice` per file — multi-session × 잦은 polling × 큰 layout 시 latency + tokio worker 점유 비례 증가 (0066 §BE-2).

amend ③ 의 implementation note:

- `AppState::attach_index: Arc<AttachIndex>` 신설 — in-memory `RwLock<HashMap<TerminalUuid, BTreeSet<SessionName>>>` 한 개. boot 시 workspace scan 으로 rebuild, 이후 4 mutation hook (layout PUT / delete_item / session import / session delete) 가 diff 적용.
- `GET /api/terminals` 는 `attach_index.read_all_attach_refs()` 로 in-memory read — disk scan O(N_sessions × file_size) → in-memory O(N_uuids) 으로 단축.
- **consistency model = strong** — index 갱신은 *disk write 성공 후* 같은 critical section (write lock 보유) 안에서 수행. disk write 실패 시 index 미갱신 → disk-of-truth invariant 보존.
- boot rebuild 의 disk parse 실패 session 은 silently skip (옛 scan 의 정합) — index 미등록, 다음 successful PUT 에 자연 등록.
- (선택, follow-up) `session_pane_set.rs` 의 layout-read 도 attach_index 의 *session→uuids* 역방향 (별 map) 으로 단축 가능 — 본 amend 의 *우선 외*, 필요성 측정 후 결정.

본 amend 의 정본 work package = `docs/reports/0068-be-attach-index-work-package.md`.

#### D7 amend ④ (2026-05-18, 0077 follow-up): attach 시점 self-heal hook — invariant 의 *최후 안전망*

amend ③ 는 *boot rebuild + 4 mutation hook* 의 strong consistency 를 명시. 그러나 실 production 에서 어떤 source 의 silent miss (boot rebuild parse failure / schema drift / 미보고 race) 든 *영속 desync* 의 가능성이 남는다 — 본 보고 `0077-terminal-pool-attach-index-desync-resolution.md` 의 사용자 시연이 그 증례.

amend ④ 가 *attach 흐름 self-heal hook* 을 invariant 로 격상:

- **호출 site**: 모든 attach 흐름이 통과하는 단일 지점 두 곳 —
  - `classify_layout_terminals` (sessions.rs) — `attach_handler` / `reuse_existing_attach_response` 양 path 의 분류 단계
  - `attach_confirm_handler` (sessions.rs) — confirm 흐름의 spawn 직전
- **동작**: `state.attach_index.apply_full_session(name, &load_terminal_uuids(state, wm, name))` — 그 session 의 layout 의 모든 UUID 를 reinsert.
- **set semantics**: 이미 boot rebuild 가 add 했으면 변경 0; miss 였으면 자동 회복.
- **격리**: `apply_full_session` 는 *해당 session 의 contribution 만* replace — 다른 session 의 mirror entry 보존.
- **비용**: layout scan 1회 + set update — microsecond 대 (100 panel 미만 일반 session 의 polling tick 비용 무시 가능).
- **진단**: `apply_full_session` 의 trace 가 *prior count vs new count* 차이 시 **WARN** surface (`attach_index: apply_full_session — count drift detected (self-heal recovered missing entries?)`). 본질적 결함 path 의 추가 추적 보조.
- **boot rebuild trace 보완**: `rebuild_from_disk` 가 `sessions_skipped > 0` 시 **WARN** 로 surface — 영속 desync 의 가장 직접적 source 가 boot rebuild 의 silent skip 이라는 진단 가설 검증.

**의미**: amend ③ 의 strong consistency 가 *증명 차원* 이라면, amend ④ 는 *production 회복 가능성* 의 보장. session 연결 시점 = *attach_index 의 정합이 사용자 인지에 영향* 의 마지막 boundary. 이 boundary 에서 reset 하면 어떤 source 의 stale 이든 *영속화 차단*.

본 amend 의 정본 보고 = `docs/reports/0077-terminal-pool-attach-index-desync-resolution.md`.

#### Terminal list UI 위치

- ~~Sidebar 의 별 section (Layer tree 와 동등 부분)~~ — MVP 초기 형태. 2026-05-16 amend 로 *별 floating panel* 로 격상.
- 또는 Toolbar 의 [Terminals] 메뉴 → drawer
- (P1+) 모달 형태로 큰 picker

**현(2026-05-16 amend ②)**: `TerminalListView.svelte` — `LeftPanel.svelte` 안의 두 번째 탭 (`[Layers | Terminals]`). 같은 날 ①번 amend 의 *별 floating panel* (`TerminalsPanel.svelte`) 결정은 ref/frontend-design `panel-tabs` 정합을 위해 회수, 통합 tab 모델로 진화. Collapsed 시 28px rail 의 terminal 아이콘 클릭으로 1-click expand + tab select. 분리/통합 history 및 chrome 의 책임 분리는 ADR-0017 의 2026-05-16 ①/② amend 참조. Stage 별 결정은 plan-0007 참조.

### D8. Terminal binding UI (panel 의 terminal id 변경)

기존 panel 이 가리킨 Terminal 을 *다른 Terminal 로 rebind* 하는 흐름:
- Panel 의 header context menu → [Change terminal...]
- → Terminal list 의 modal popup
- → 선택 → server 의 `PUT /api/sessions/<name>/items/<item-id> { terminal_id: <new-id> }`
- → 그 panel 의 xterm 인스턴스가 새 terminal 의 stream 으로 subscriber 교체
- → 옛 terminal 은 *그 session 에서 detach* (다른 session 의 attach 가 있으면 그대로 살아있음)

#### Detach / Attach 정합

| 액션 | 동작 |
|---|---|
| Detach panel | Item 의 terminal_id 를 null 로? — 거부. Schema v2 의 terminal item 은 항상 terminal_id 가짐 (D2). 대신 panel 을 layout 에서 제거 (= delete item) 또는 다른 terminal 로 rebind 만. |
| Attach existing terminal | Terminal list 에서 [Attach to canvas] → 새 panel 을 cascade 좌표로 mount, terminal_id 는 그 terminal 의 |
| Spawn fresh + attach | [New Terminal] (D3) |

#### D8 amend ② — [Attach to this session] / [Change terminal] 시 ring buffer replay (2026-05-18, 0075/0076/0077)

[Attach to this session] (D7 의 [Terminals] tab) 또는 [Change terminal] (D8
본문) 으로 *기존 alive UUID* 가 layout 에 added 되면, BE 가 그 시점의 ring
buffer 를 *그 session 의 WS 에* 1회 broadcast (PANE_OUT envelope). 사용자
mental model "이 terminal 의 history 가 그대로 보임" 보장.

**문제의 본질** (0076 §2): catch-up replay 는 WS handshake 시 1회만 발화
(`ws-server/lib.rs:524-540` doc). layout PUT 의 add 시점에는 ring buffer
emit 발화 0 — 그 결과 [Attach to this session] 직후 새 panel 의 xterm 이
빈 화면. catch-up 의 자연 회복은 강제 WS reconnect / page reload 까지 영구
지연.

**결정** (옵션 a-1, session-aware envelope):

| 항목 | 값 |
|---|---|
| Broadcast 채널 | 신규 `attach_replay_events: broadcast::Sender<AttachReplayEvent>` (cap 16) |
| Envelope | `AttachReplayEvent { session: Arc<str>, pane_id: u64, bytes: Bytes }` — owner-scope 매칭의 진실 |
| Trigger | `put_layout_handler` 의 `attach_index.apply_diff(name, removed, added)` 직후, `added` UUIDs 중 `terminal_map.lookup_pane(uuid).await` 가 `Some` 인 것 |
| Forward 조건 | WS handler 의 `select!` arm 이 `hub.session_for_owner(owner) == ev.session` 매칭 시 PANE_OUT envelope 으로 forward. **`session_pane_set` filter 우회** — envelope 안 session 동봉이 routing 의 진실 (ADR-0025 set hot-update timing 면역) |
| Idempotency | `apply_diff` 의 added 가 *진짜 신규* 인 경우만 emit. drag 의 net-zero (added=[]) → emit 0. same-UUID re-add → emit 1회 (사용자 정합) |
| Fault tolerance | hub 없음 / pane lookup 실패 / ring 비어있음 → continue (skip). broadcast cap hit → Lagged warn + 사용자 perception 1회 누락. send 실패는 `let _ =` ignore — PUT 응답에 영향 0 (disk-of-truth invariant 보존) |

**Race 매트릭스** (7 시나리오 검증, 0076 §8.5): data corruption 0, lock
leak 0. 최악 perception loss = history 1회 누락 (cap hit, 사용자 명시 click
sub-Hz 라 사실상 불가) 또는 1회 duplicate (catch-up reconnect window 의
edge race).

**거절된 대안** (0076 §4):
- 옵션 (b) HTTP endpoint `POST /api/terminals/<uuid>/replay`: round-trip
  1회 추가 + FE 의 *언제 호출할지* state 필요. broadcast piggy-back 보다
  검증 표면 큼.
- 옵션 (c) FE 가 layout PUT 직후 WS force-reconnect: UX 거슬림 (잠시
  disconnect).
- 옛 옵션 (a) plain `(pane_id, bytes)` envelope + `session_pane_set`
  filter: set hot-update ordering race 발생 가능 (envelope 도착 시점에
  set 가 아직 stale). (a-1) 의 envelope.session 동봉이 race 면역.

**검증 test** (3 신규):
- `attach_existing_terminal_replays_ring_buffer_to_session_ws` —
  owning session 의 WS 가 PANE_OUT envelope 으로 replay 수신
- `attach_existing_terminal_replay_owner_scoped` — sibling sessions 의
  WS 는 envelope.session 매칭 실패로 차단
- `attach_existing_terminal_replay_idempotent_for_drag_layout` — drag
  의 net-zero (added=[]) PUT 은 emit 0

본 amend 가 없으면 [Attach to this session] 직후 β 측 xterm 빈 화면 → 강제
WS reconnect / page reload 까지 영구 지연 (0076 §2.3 의 *catch-up 의 자연
회복도 안 됨*).

### D9. Close 의 semantic (Panel close 의 정확한 행동) — 2026-05-15 G25.1 grilling amend

> ⚠️ **G25.1 amend (2026-05-15)**: 기존 *"attach 점 = 0 → 자동 SIGTERM (single-session legacy)"* 는 **폐기**. Panel = 그 session 의 layout entry, Terminal = backend process 의 *별 개념* 분리. Panel close 와 Terminal kill 의 의도는 *각 매번 사용자 선택*.

#### D9.1 Session reload / switch 시 (자동, snapshot 변화)
- *현재 canvas 에는 있는데 load snapshot 에는 없는 panel* → 제거 + terminal SIGTERM (사용자가 ADR-0018 D6 의 confirm 통과 후)

#### D9.2 명시 [Close] 클릭 시 (사용자 액션)

**Default UX (Settings.behavior.auto_kill_terminal_on_panel_close = false)**:

```
panel close 클릭
  ↓
Confirm dialog:
┌ Close panel? ──────────────┐
│ Terminal 'build-watch'      │
│ is mirrored in:             │
│  • session 'monitor'         │
│  • session 'demo-build'      │
│  (otherwise: 'Only here')   │
│                              │
│ [Cancel]                    │
│ [Panel only]                │
│ [Panel + Terminal] ⚠        │
└────────────────────────────┘
```

옵션 의미:
- **[Cancel]**: 아무 동작 없음.
- **[Panel only]**: 그 session 의 layout 에서 panel item 제거. Terminal pool 유지 — 다른 session 의 attach 는 그대로, 또는 0 이면 server-pool 의 *idle terminal* 로 남음 (다음 attach 또는 explicit [Kill terminal] 까지 alive).
- **[Panel + Terminal]**: panel 제거 + terminal SIGTERM. 다른 session 의 mirror panel 은 *dangling* 상태 (D10 의 c2 처리).

**Settings 자동화 (Settings.behavior.auto_kill_terminal_on_panel_close = true)**:
- Dialog 생략 + [Panel + Terminal] 즉시 발동.
- Default = `false` (G25.1.c grilling) — 안전 default.

#### D9.3 Group close 의 bulk dialog (ADR-0010 D10 amend)

Group close (Layer list group [Delete]) → 자손 panel 모두 *bulk* close:

```
┌ Delete group 'build-cluster'? ──┐
│ This group contains:             │
│  • 3 terminal panels:            │
│      build-watch ⚠ (mirror 2x)  │
│      monitor-1                   │
│      log-tail ⚠ (mirror 1x)     │
│  • 2 notes                       │
│                                  │
│ [Cancel]                         │
│ [Panels only] (all terminals     │
│   stay in pool)                  │
│ [Panels + Terminals] ⚠ (kill    │
│   all 3 terminals)               │
└──────────────────────────────────┘
```

- 자손 마다 dialog 발동 안 함 (bulk 1 dialog).
- ADR-0010 D10 (기존 *자손 kill-pane 재귀 + confirm modal*) 의 amend — *[Panels only] 옵션 추가* + mirror hint.
- Settings auto-toggle = on 이면 dialog 없이 [Panels + Terminals] 즉시.

#### D9.4 사용자 명시 [Kill terminal] 액션 (별 액션, 기존 유지)

TerminalsPanel 의 row → [Kill terminal]:
- 현재 session panel 과 연결된 terminal 은 row action 을 숨긴다. 종료는 해당 panel close 의 [Panel + Terminal] 로 수행한다.
- 다른 session panel 과만 연결된 terminal 은 [Attach] 만 표시한다. 현재 session 으로 가져오는 것은 허용하지만, Terminal tab 에서 직접 kill 하지는 않는다.
- `attach_count == 0` 인 unplaced terminal 은 [Attach] + [Kill terminal] 을 모두 표시한다.
- 차이: Panel close 의 [Panel + Terminal] 는 *그 session 의 panel* 도 제거한다. Terminal tab 의 [Kill terminal] 은 panel 과 연결되지 않은 orphan/live pool entry 정리용이다.

### D10. Cross-session terminal lifecycle 알림 + dangling 의 lazy fresh spawn (G25.1.b amend)

```
Terminal T 가 dead (child process exit OR 다른 session 의 [Panel + Terminal] 액션 OR [Kill terminal]):
    ↓
server 가 TERMINAL_DIED { terminal_id: T, reason: "exit" | "killed" } broadcast
    ↓
모든 active webpage 에 broadcast (T 를 attach 한 session 들 모두)
    ↓
각 webpage 의 frontend:
    - terminal_id == T 인 panel 의 xterm 인스턴스 detach
    - reason == "exit": 기존 자동 복구 정책에 따라 POST /api/terminals/:id/respawn
    - reason == "killed": 사용자 명시 종료 의도를 보존해 자동 respawn 금지
    - 또는 사용자 명시 [Remove panel] (panel header more menu) → 그 session 만 panel 제거
```

#### D10.1 Lazy vs Eager fresh spawn (G25.1.b 결정)

| 정책 | 선택 |
|---|---|
| c1 Eager (terminal die 즉시 모든 mirror auto-spawn) | **부분 거부** — `reason:"killed"` 에서는 "끝낸 줄 알았는데 자동 부활" 로 보임 |
| **c2 Auto on natural exit only** | **선택** — `reason:"exit"` 은 흐름 복구, `reason:"killed"` 는 사용자 종료 의도 보존 |
| **c3 명시 [Restart terminal] CTA (killed only)** | **부분 채택 (2026-05-18 amend)** — components-v5 §04 "Not connected" empty-state 패턴 차용. 자동 respawn 은 여전히 금지 (사용자 의도 보존) 이지만, 빈 panel-body 가 *dead-end* 로 남는 회복 부담을 해소하기 위해 panel 안 명시 CTA 1개만 노출. [Remove panel] 은 별 panel close 흐름이 이미 책임지므로 본 CTA 에서는 제외. |

#### D10.2 Attach 시 vs Live dangling 시의 dialog 차별화

| 시점 | 사용자 의도 | UX |
|---|---|---|
| **Session attach 시 (snapshot 안 unmatched panel)** | 큰 변화 (여러 panel 일 수 있음) | ADR-0018 D6 의 *확인 dialog* — "Will spawn N new terminal(s). Continue?" |
| **Live session dangling 시: natural exit** | 작은 변화 (한 terminal) | 자동 respawn |
| **Live session dangling 시: explicit kill/SIGTERM** | 사용자가 종료를 명시 | 자동 respawn 금지 + panel-body 안 명시 [Restart terminal] CTA (components-v5 §04 empty-state) |

이 차별화 이유: attach 는 *큰 transition* 의 일부이고, natural exit 는 흐름 복구가 자연스럽다. 반면 explicit kill 은 종료 자체가 사용자 의도이므로 자동 복구가 action 을 되돌리는 결과가 된다.

#### D10.3 Respawn endpoint 의 동시-호출 정책 (2026-05-17 amend ③)

**맥락**: D10.1 의 lazy c2 spawn 은 FE `PanelDanglingOverlay` 가 mount 시 자동
`POST /api/terminals/:id/respawn` 호출하는 흐름으로 ship. 두 webpage 가 같은
terminal UUID 의 panel 을 mirror (ADR-0021 D1) 하고 *동시에* dead 감지 시,
두 respawn 요청이 거의 동시에 도착 가능. 0053 §3.4 follow-up.

**결정**: `respawn_handler` (`crates/http-api/src/terminals.rs:283`) 는
[`AppState::respawn_locks`] 의 *per-UUID* mutex 로 kill→spawn 쌍을 직렬화.

```
요청 A    요청 B
  ↓        ↓
  outer lock_for(uuid)   ← 같은 Arc<Mutex<()>> 공유
  ↓        ↓
  A 가 inner lock 먼저 획득
  ↓        ↓ (대기)
  lookup_pane(uuid) → None (dangling)
  kill_and_unregister (no-op)
  spawn_terminal_with_uuid → PaneId_X 등록
  → 0x88 broadcast
  release
            ↓
            inner lock 획득
            lookup_pane(uuid) → Some(PaneId_X)
            ← idempotent {reused: true}
```

**응답 shape** (amend ③ 후):
- `200 OK + { id, reused: false }` — kill+spawn 실행, 새 PaneId 바인딩.
- `200 OK + { id, reused: true }` — lock 진입 후 이미 alive 발견 → no-op.
- `503 hub_not_configured` — hub 미 wire.
- `500 respawn_failed + message` — spawn 실패.

**불변**:
- terminal_map 에는 *언제든* UUID 당 최대 1 alive PaneId 바인딩 (D1 의 multi-mirror 는 panel-수준이지 PaneId-수준이 아님).
- PTY leak 0 — 모든 race 분기 (this lock + `terminal_map.register` 의 inner serialization) 가 loser PaneId 의 `hub.backend().kill(pane)` 보장 (lib.rs:349-361).
- 응답 body 의 `reused` field 가 client 에 *어느 분기를 탔는지* 정직 보고 — FE 측 metric / debug 에 활용 가능.

**FE 측 영향**: 변경 0 — `PanelDanglingOverlay` 가 success 응답만 보면 충분 (이미 ship). multi-webpage 시 둘 다 success, 한쪽이 빈 시도라는 점은 BE 의 부담으로 흡수됨.

**검증 테스트**: `respawn_concurrent_same_uuid_yields_single_alive_binding`
(`lib.rs:2573~`) — `tokio::join!` 으로 동시 2개 호출, sorted `reused` flags ==
`[false, true]` + terminal_map 에 alive 바인딩 1 검증. workspace +1 PASS.

**거절된 대안**:
- **R1**: 전체 respawn 직렬화 (단일 mutex) — 다른 UUID 끼리도 직렬화되어 concurrency 손실. 거부.
- **R2**: lock 없이 `terminal_map.register` 의 race 결과 (UuidAlreadyBound 분기) 만으로 충분 — spawn 측 race 만 cover, *kill 측 race* 의 brief output-stream orphan 은 cover 안 됨. 거부.
- **R3**: respawn_locks map GC (Weak<Mutex<()>>) — 단일 사용자 환경에서 UUID 수가 작아 leak 영향 ~60 byte/UUID. 복잡도 증가 부담 > 절약. P3 follow-up.

## 어휘 매트릭스 (CONTEXT.md 정합)

- **Terminal** = ADR-0013 의 PTY pair + child process (= 옛 *Pane*, multi-session pivot 으로 어휘 통일)
- **Panel** = `type:"terminal"` 인 Canvas Item (ADR-0018 D1)
- **Mirror** = 한 Terminal 이 여러 Panel 에 동시 attach 된 상태 — 입출력 공유 (D1+D2)

## 대안 검토

### A1. Terminal 1:1 exclusive (= 다른 session 이 쓰려면 detach 후 attach)
**거부.** 사용자 명시 "공유 허용 + 입력 공유" (Q9 옵션 A). multi-monitor / 협업 / 모니터링 use case 의 자연 표현 X.

### A2. Terminal mirror 이지만 input 은 owner 한 session 만
**거부.** Q9 grilling 에서 사용자 명시 거부. *Single-user 환경* 이라 input 공유의 race 위험 낮음.

### A3. Auto-mount 를 모든 session broadcast (옛 ADR-0015 그대로)
**거부.** 사용자 명시 *"webpage 별 독립적인 layout"* 정면 반대. 다른 session 에서 작업 중 panel 이 무관하게 추가되는 UX 손상.

### A4. Auto-mount 폐지 (모든 mount = 사용자 명시)
**거부.** [New Terminal] 누른 사용자가 자기 layout 에 panel 안 보이면 의아함. trigger session 만 auto-mount 가 자연.

### A5. Heartbeat 더 짧게 (5s/10s)
**검토 후 거부.** Q13 grilling 에서 15s/30s 의 SaaS 표준값 채택. 짧게 잡아도 부담은 작지만 *UX 측정 가능한 차이는 없음*.

### A6. Takeover 허용 (활성 session 도 click + confirm 으로 가져가기)
**거부.** Q15 grilling. 단일 사용자라 takeover 의 효용 낮음. 다른 session 만들면 됨.

## 영향

### Code
- **Backend**:
  - dispatcher hook 의 *trigger-session aware* 분기 (D3)
  - Session-scoped state store (M/I/Viewport/Focus 가 session 단위 dict)
  - WS frame 의 session_id 라우팅 (D5)
  - Heartbeat ping/pong (axum/tokio-tungstenite 의 built-in 활용, D6)
  - Terminal list API: `GET /api/terminals` (server-wide)
  - Panel rebind API: `PUT /api/sessions/<name>/items/<id>/terminal { terminal_id }`
- **Frontend**:
  - `terminalsStore` (server-wide Terminal pool 의 frontend mirror)
  - Terminal list UI (`TerminalListView.svelte` — `LeftPanel` 의 두 번째 탭; ADR-0017 §D2 amend ② 정합)
  - Panel context menu 의 [Change terminal] (D8)
  - Heartbeat 의 server-side는 자동, frontend 코드 변경 거의 없음
  - xterm 인스턴스의 subscriber pattern (multi-panel for 1 terminal)

### ADR
- ADR-0015 amend (cascade target 분기 — trigger session 만)
- ADR-0002 D3 amend (MT-3 → session-scoped + server-wide 2-layer)

### Docs
- `docs/ssot/wire-protocol.md` 의 WS frame envelope 에 session_id 추가, heartbeat 명세 추가
- plan-0007 Stage 의 진실

### 보안
- WS frame 의 session_id 검증 (다른 session 의 frame 을 spoofing 시도 차단)
- Terminal list 의 *모든 alive Terminal 노출* 정합 — 단일 사용자라 OK, multi-user 면 ACL 필요 (비범위)

## 변경 이력

- 2026-05-15: 초안 + Accepted. plan 0006 grilling 의 Q9/Q10/Q11/Q13/Q14/Q15 합본. ADR-0015 amend, ADR-0002 D3 amend.
- 2026-05-15 (G25.1 grilling amend): D9 의 *"attach 점 = 0 → 자동 SIGTERM"* 폐기. D9.1~D9.4 신규 (session reload/switch 자동 / 명시 close dialog 3 옵션 / Group close bulk 1 dialog / [Kill terminal] 별 액션). D10 의 dangling 처리에 D10.1 (lazy c2 spawn) + D10.2 (attach vs live 차별화) 추가. Settings.behavior.auto_kill_terminal_on_panel_close (default false) 신규.
- 2026-05-16 ①: D7 amend — Terminal list UI 가 Sidebar 의 sub-section 에서 *별 floating panel* (`TerminalsPanel.svelte`) 로 분리. ADR-0017 의 동일자 ①번 amend 와 짝. *(같은 날 ②번 amend 로 통합 tab 모델로 진화.)*
- 2026-05-16 ②: D7 amend — Terminal list UI 가 `LeftPanel.svelte` 안의 `[Layers | Terminals]` 탭 두 번째로 통합 (`TerminalListView.svelte`). ref/frontend-design `panel-tabs` 패턴 정합. ADR-0017 의 동일자 ②번 amend 와 짝.
- 2026-05-16 (묶음 D + 0045/0046 정합): **D6.1 FE-side liveness watchdog amend** — `lib/ws/heartbeat.svelte.ts` 신규 (`lastFrameAt`/`lastActivityAt` + `isStale`/`isIdle` derived + dispatcher.markFrame wire + window activity listener). plan-0008 Phase 2 silent reattach 의 idle detection trigger 입력. RFC 6455 PING/PONG 의 browser-auto 처리와 별 application-level perception. **D6.2** BE 측 ping/pong 미 ship 상태 + `0047-be-next-session-handover.md` §3.2 로 발주 명시 — abrupt close (browser crash/OS kill) 시 lock leak 의 30s 대기 회피를 위한 enabler.
- 2026-05-16: **D6.2 amend ② — BE ship 정합**. 이전 entry 의 "BE 측 ping/pong 미 ship" claim 이 stale 으로 확인 — 실제 wire 는 이미 ship (ws-server `handle_socket` 의 `ping_timer` + `last_pong` tracking + `emit_heartbeat`, hub `heartbeat_tx` slot + setter, cli `_heartbeat_task` consumer → `refresh_lease_for_cookie`, http-api `AppState::refresh_lease_for_cookie` + `session_lock::refresh_lease`). 본 amend 는 두 가지 작업의 짝: (1) `HeartbeatTimings` 신규 struct + `Hub::set_heartbeat_timings` setter 도입 — production default 변경 0, 테스트가 ms 단위로 override 가능 (per-WS-upgrade snapshot 패턴, disconnect/heartbeat sink 와 동일). 이전의 `PING_INTERVAL`/`PONG_TIMEOUT` const 는 제거 (defaults 가 `HeartbeatTimings::default()` 안 흡수). (2) integration test 2개 추가 — `heartbeat_timeout_closes_1011_and_emits_disconnect` (close 1011 + disconnect_sink) + `heartbeat_pong_reply_emits_heartbeat_sink` (정상 pong 흐름이 heartbeat_sink 에 cookie emit). D6.2 § 본문 도 ship 상태로 amend (5-row layer 표 + 테스트 cover 명시). 검증: workspace 362 → 364 PASS (+2 신규).
- 2026-05-17: **D6.2 amend ③ — heartbeat timeout test contract 정합 (0051 §5.3 flaky fix)**. amend ② 의 `heartbeat_timeout_closes_1011_and_emits_disconnect` 가 `cargo test --workspace` parallel 실행 시 flaky 로 보고 (0051 handover §5.3) — 단독 실행 PASS, 8-thread 병렬 압박 시 `Close(1011)` frame 이 `None` 으로 떨어짐 (client 가 stream end 만 봄). **진단**: server-side `let _ = sink.send(close_frame).await` 가 tokio runtime scheduling 압박 + socket teardown 과 race — graceful close payload 전달은 best-effort 였음. **fix**: (a) 테스트 timing 4x 확장 — `ping_interval 50→100ms`, `pong_timeout 150→300ms`, client wait `400→700ms`. (b) test contract 정합 — close_code assertion 을 `matches!(None | Some(CloseCode::Error))` 로 완화. `disconnect_sink` emit 은 strict 유지 (production `release_lock_for_cookie` 트리거의 load-bearing signal). (c) test 이름 rename `..._1011_...` → `..._and_...` (1011 close code 는 hint, 진짜 contract 는 "timeout → disconnect"). 검증: `cargo test --workspace` 5회 연속 실행, flake 0, 본 test 매번 PASS.
- 2026-05-18: **D7 amend ③ — `attach_count` / `attached_sessions` 의 in-memory reverse index** (0066 §BE-2 / 0067 Phase 4). 옛 `terminals.rs::scan_session_terminal_refs` 의 매 GET 시 동기 file scan 을 in-memory `AttachIndex` (UUID → BTreeSet<session>) 로 교체. boot rebuild + 4 mutation hook (layout PUT diff / delete_item / import / session delete). consistency = strong (lock 안 atomic 갱신, disk write 성공 후). 정본 work package = `0068-be-attach-index-work-package.md`. 검증: 신규 unit 6 + integration 5 → 405 → 416 PASS 목표.
- 2026-05-18: **D7 amend ④ — attach 시점 self-heal hook** (0077 follow-up). amend ③ 의 strong consistency 가 *증명 차원* 이라면, amend ④ 는 *production 회복 가능성* 의 보장. `classify_layout_terminals` + `attach_confirm_handler` 의 200 응답 직전에 `attach_index.apply_full_session(name, &uuids)` 호출 — boot rebuild miss / schema drift / 미보고 race 어떤 source 의 stale 이든 *session 연결 시점에 자동 회복*. set semantics 라 boot rebuild 정상 시 변경 0 (microsecond), miss 시 자동 회복. 다른 session 의 mirror entry 영향 0. `apply_full_session` 의 trace 가 prior/new count drift 시 WARN surface. `rebuild_from_disk` 의 `sessions_skipped > 0` 도 WARN surface. 정본 보고 = `0077-terminal-pool-attach-index-desync-resolution.md` (commit series: `72278b1` / `605d8d8` / `5ea3dc3` / `c63be0c` / `a276058` + 외부 `abc5931`). 사용자 시연 환경의 end-to-end 검증 완료.
- 2026-05-17: **D10 amend ④ — D10.3 respawn 동시-호출 정책 신규 (0053 §3.4 closed)**. FE `PanelDanglingOverlay` 가 mount 시 자동 respawn 흐름 ship 후 multi-webpage 환경에서 같은 UUID 의 dead 감지 race 가능성 (0053 §3.4 발주). **결정**: `respawn_handler` 가 `AppState::respawn_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>` 의 per-UUID mutex 로 kill→spawn 쌍을 직렬화. lock 후 `terminal_map.lookup_pane(uuid)` 가 alive 면 idempotent `{reused: true}` short-circuit — 두 번째 caller 는 winner 의 새 PaneId 를 그대로 reuse, kill 안 함. **응답 shape 확장**: `{ id, reused: bool }` — kill+spawn 분기는 `reused: false`, 이미 alive lookup 분기는 `reused: true`. unbreaking change (FE 는 `reused` 무시 가능). **PTY leak 검증**: spawn 측 race 는 이미 `terminal_map.register` 의 `UuidAlreadyBound` 분기 (lib.rs:349-361) 가 loser kill 보장 (D6.2 가 D10.3 와 직교). 검증: `respawn_concurrent_same_uuid_yields_single_alive_binding` test 추가 — `tokio::join!` 으로 동시 2호출, sorted `reused` flags `[false, true]` + terminal_map alive 1 검증. workspace 375 → 376 PASS. 0053 §3.4 closed.
- 2026-05-18: **D6 amend ② — `POST /api/leave` sendBeacon endpoint (0071 §D-5 land)**. D6 본문이 `navigator.sendBeacon('/api/leave')` 를 *"보조: best-effort"* 회수 경로로 잠궜으나 BE endpoint 가 미구현이었음. **결정**: `POST /api/leave?webpage_id=<id>` 신규, body 없음, 응답 `204 No Content`, 동작 = `AppState::release_lock_for_owner(owner_key)` (idempotent). `webpage_id` 는 URL **query** — sendBeacon 의 custom-header 제한 우회. 처리 함수는 `DELETE /attach` 와 동일 (`release_lock_for_owner`) — wire 만 다른 ingress. 정상 탭 close 의 30s heartbeat 대기 회피 → 다른 webpage 의 같은-session 진입 즉시 가능. 검증: 4 integration test (happy / idempotent / 401 / different-webpage-isolation), workspace 419 → 423 PASS. 짝: `docs/reports/0072-be-handover-from-0071-audit.md` §C BE-B.
- 2026-05-18: **D8 amend ② — [Attach to this session] / [Change terminal] 시 ring buffer replay (0075/0076/0077, 옵션 a-1 채택)**. catch-up replay 가 WS handshake 1회만 발화 (`ws-server/lib.rs:524-540` doc) — layout PUT 의 add 시점 emit 발화 0 → [Attach to this session] 직후 새 panel 의 xterm 빈 화면 (`0076 §2` 확정). **결정**: 신규 `attach_replay_events: broadcast::Sender<AttachReplayEvent>` (cap 16) + `AttachReplayEvent { session, pane_id, bytes }` envelope. `put_layout_handler` 의 `apply_diff` 직후 `added` UUIDs 중 alive PaneId 인 것의 ring buffer 를 publish. WS handler 가 `hub.session_for_owner(owner) == ev.session` 매칭 시 PANE_OUT envelope forward — `session_pane_set` filter 우회 (envelope.session 동봉이 race-immune routing 의 진실, ADR-0025 set hot-update timing 면역). idempotent (drag 의 net-zero → emit 0) + race 매트릭스 검증 (0076 §8.5 의 7 시나리오, data corruption 0). 검증: 3 신규 integration test (F-1 forwarding / F-2 owner-scoped / F-3 drag idempotency), workspace 425 → 428 PASS. 정본 보고 = `0075-be-handover-rebind-history-replay.md` + `0076-rebind-history-replay-missing.md`.
- 2026-05-18: **D3 amend ② — `0x86 MOUNT_CASCADE` wire 의 `trigger_session` 필드 (0072 BE follow-up §1, FE 72278b1 desync trace 짝)**. server-side `hub.session_for_owner(K)` 필터는 frame send 시점만 검증 — frame 비행 중 owner 가 session 을 switch 하면 옛 wire body `{terminal_id, x, y, w, h}` 로는 FE 가 mismatch 인지 불가능, terminal 이 wrong session 의 layout 에 등록되는 desync 발생. **결정**: encode_mount_cascade 가 `trigger_session` 을 JSON body 에 동봉, FE `decodeMountCascade` 가 검증, `dispatcher.handleMountCascade` 가 `triggerSession !== sessionStore.active?.name` 면 console.warn + drop. BE+FE paired land. 추가: `AttachIndex::rebuild_from_disk` 의 parse failure 가 silent 였던 가시성 결함 (§2) 도 tracing::warn 동봉. workspace test 영향 0 (기존 test 갱신만, 425 PASS 유지). 짝: `docs/reports/0072-be-handover-from-0071-audit.md` 추가 follow-up.
