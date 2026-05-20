# 0034 — Stage 5-A + 5-B BE progress (WS envelope refactor 1/2)

- 일자: 2026-05-15
- 작성자: backend agent (Stage 5 entry, 0033 핸드오버의 후속)
- 종류: 진행 snapshot — Stage 5 의 BE-alone 안전 도착점 (5-A + 5-B 완료, 5-C/5-D 는 FE-NEW-6 의존으로 의도적 deferral)
- 후속 reading order: 본 문서 → `0033-next-session-handover-stage-5-entry.md` → `0032-stage-4-terminal-pool-and-pivot-be-progress.md`

---

## 0. 한 줄 요약

`docs/reports/0033-next-session-handover-stage-5-entry.md` §4 의 4 batch 중 **BE-alone 으로 닫을 수 있는 5-A + 5-B 가 완료**. 5-C (session_id-scoped envelope routing) / 5-D (auto-mount trigger-aware) 는 핸드오버 §4.4 의 명시대로 *FE-NEW-6 dispatcher 의 새 frame 처리 필요* — 본 세션 의도적 deferral. 261 → **278 PASS / 0 FAIL** (+17). 미커밋.

---

## 1. 본 세션 의 산출 (5-A, 5-B)

### 1.1 신규/수정 파일

| 파일 | 종류 | 핵심 변경 |
|---|---|---|
| `crates/ws-server/src/hub.rs` | amend | `session_table: Arc<RwLock<HashMap<String, String>>>` + 4 API (`set/clear_for_cookie`, `session_for_cookie`, `clear_sessions_by_name`). `terminal_died_events` broadcast 채널 + `TerminalDiedEvent` + 2 API (publish/subscribe). 새 unit +10 |
| `crates/ws-server/src/lib.rs` | amend | `FrameType::TerminalDied = 0x85` 추가 + from_u8/as_u8 매트릭스 갱신. WS handler 의 select! 에 `terminal_died_rx` arm 추가 (0x85 envelope 송신). inbound 0x85 는 `policy_violation` close (server-only). 기존 2 단위 (0x85 가 unknown 으로 가정) 0x86 으로 migrate + TerminalDied 의 `is_web_domain` 통과 단위 |
| `crates/ws-server/src/payload.rs` | amend | `encode_terminal_died(uuid, reason)` — varint 0 + UTF-8 JSON `{"terminal_id","reason"}`. 단위 +2 |
| `crates/http-api/src/sessions.rs` | amend | `attach_handler` (success path) → `hub.set_session_for_cookie`. `release_attach` (cleanup) → `hub.clear_session_for_cookie`. `detach_handler` → `hub.clear_sessions_by_name`. cookie ↔ session 의 두-맵을 lock-step 유지 |
| `crates/http-api/src/lib.rs` | amend | `release_lock_for_cookie` → `hub.clear_session_for_cookie`. `handle_pane_died` 시그니처 확장 — `(pane, signal)` → uuid 추출 후 hub publish (`signal.is_some()` ? `"killed"` : `"exit"`). 기존 3 단위 갱신 + 새 통합 +6 (Stage 5-A 3, Stage 5-B 3) |
| `bin/gtmux-cli/src/main.rs` | amend | `BackendNotify::PaneDied { id, signal, .. }` destructure → `state.handle_pane_died(id, signal).await` |

ws-server / http-api / cli 모두 동시에 touched. pty-backend / config / auth crate **변경 없음**.

### 1.2 테스트 수 (workspace 합산)

| 시점 | PASS | 증감 | 신규 |
|---|---|---|---|
| Stage 4 + C 종료 (0033) | 261 | — | — |
| 5-A 종료 | 270 | +9 | ws-server hub session_table 6 + http-api 통합 3 |
| 5-B 종료 | **278** | +8 | ws-server payload 2 + hub terminal_died 3 + http-api 통합 3 |

전체 +17. 모두 ws-server + http-api 안. cli / pty-backend / config 의 테스트 수 무변동.

---

## 2. Stage 5-A 디테일

### 2.1 의도

ADR-0021 D5 의 session-scoped frame routing 의 prereq. WS 연결이 *어느 session 의 attach 인지* 알 수 없으면 5-C 의 session_id 기반 라우팅이 불가. 핸드오버 §4.1.B 의 option (i) — Hub method 직접 호출 — 채택.

### 2.2 API 표면

`Hub` (`crates/ws-server/src/hub.rs`):

```rust
// 4 신규 메서드 + 1 신규 필드 + 1 신규 broadcast 채널 (B 에서 추가)
pub fn set_session_for_cookie(&self, cookie: &str, session_name: &str);
pub fn clear_session_for_cookie(&self, cookie: &str);
pub fn session_for_cookie(&self, cookie: &str) -> Option<String>;
pub fn clear_sessions_by_name(&self, session_name: &str);
```

자료구조: `Arc<std::sync::RwLock<HashMap<String, String>>>`
- `std::sync::RwLock` 선택 이유: 모든 op 가 sub-µs 해시 터치, `.await` 없음. 기존 sink (`disconnect_tx`/`heartbeat_tx`) 의 `std::sync::Mutex<Option<...>>` 패턴과 정합.
- 핸드오버 §8.1 의 option (i) `Arc<RwLock<HashMap>>` 권장안 그대로.

### 2.3 정합 (lock-step) 의 4 부착 지점

| 호출 지점 | 호출 메서드 | 위치 |
|---|---|---|
| `attach_handler` (success, after `by_cookie.insert`) | `set_session_for_cookie(cookie, name)` | `sessions.rs` |
| `release_attach` (failed-attach cleanup) | `clear_session_for_cookie(cookie)` | `sessions.rs` |
| `detach_handler` (after `by_cookie.retain`) | `clear_sessions_by_name(name)` | `sessions.rs` |
| `release_lock_for_cookie` (WS disconnect) | `clear_session_for_cookie(cookie)` | `lib.rs` |

검증: poisoned RwLock 은 warn log + skip — 커널 flock 이 진실 출처이므로 hub 의 stale 항목은 *최대* "session-scoped frame 이 server-wide 처럼 동작" 으로 degrade.

### 2.4 테스트

- ws-server `hub.rs`: 6 단위 (empty default, set/lookup, replace, clear idempotent, by-name 부분 정리, Hub clone share state)
- http-api `lib.rs`: 3 통합
  - `attach_mirrors_cookie_to_hub_session_table` — attach + detach round-trip 의 hub 반영
  - `release_lock_for_cookie_clears_hub_session` — WS-close 시뮬레이션
  - `detach_clears_all_cookies_for_session_in_hub` — by-name 일괄 정리 + 무관 session 보존

---

## 3. Stage 5-B 디테일

### 3.1 의도

ADR-0021 D5 의 UUID-carrying terminal-died — FE 가 `GET /api/terminals` poll 없이 dangling overlay 표시 가능. 핸드오버 §4.1.C + §8.3 option (ii) (hub publish API) 채택.

### 3.2 wire 확장

새 `FrameType::TerminalDied = 0x85`:
- inner: `varint 0 + UTF-8 JSON {"terminal_id":"<uuid>","reason":"exit"|"killed"}`
- 서버 only (inbound 0x85 → `policy_violation` close)
- web-domain marker (`is_web_domain` true)

`reason` 매트릭스:
- `signal.is_some()` → `"killed"` (kill, SIGTERM, SIGKILL 등)
- `signal.is_none()` → `"exit"` (정상 종료, `code` 무관)

### 3.3 broadcast 패턴

```rust
broadcast::Sender<TerminalDiedEvent>
where TerminalDiedEvent { uuid: Arc<str>, reason: &'static str }
```

- 채널 cap: 32 (`TERMINAL_DIED_BROADCAST_CAPACITY`) — pane 사망은 저-빈도, layout_events(16) 과 같은 order-of-magnitude
- `Arc<str>` 선택: broadcast subscriber 당 refcount bump (heap copy X)
- 발행 위치: `AppState::handle_pane_died` 안에서 `unregister_pane` 이 UUID 반환한 직후

### 3.4 시그니처 변경 (breaking)

```rust
// before
pub async fn handle_pane_died(&self, pane: PaneId)

// after
pub async fn handle_pane_died(&self, pane: PaneId, signal: Option<i32>)
```

`code: Option<i32>` 는 의도적 제외 — reason 매트릭스의 "exit"/"killed" 결정에 불필요. 미래에 더 세분화 시 추가.

호출자 3 곳 갱신:
- `bin/gtmux-cli/src/main.rs`: `BackendNotify::PaneDied { id, signal, .. }` destructure → `handle_pane_died(id, signal)`
- 기존 3 test sites: `handle_pane_died(PaneId(_), None)` 으로

### 3.5 server-wide vs session-scoped

terminal-died 는 **server-wide broadcast** — 같은 terminal 이 여러 session 의 panel 일 수 있음 (ADR-0021 D1 mirror). 따라서 5-C 의 session_id 라우팅 정합 X. 본 frame 은 5-A 의 session_table 무관하게 모든 WS subscriber 에 전달.

### 3.6 테스트

- ws-server `payload.rs`: 2 단위 (exit/killed reason)
- ws-server `hub.rs`: 3 단위 (publish silent / single subscriber / multi subscriber)
- http-api `lib.rs`: 3 통합
  - `handle_pane_died_publishes_exit_reason_when_no_signal`
  - `handle_pane_died_publishes_killed_reason_when_signal_set`
  - `handle_pane_died_does_not_publish_for_unknown_pane`

기존 단위 2 (envelope_decode_unknown_type + frame_type_from_u8_covers_all) 의 0x85 unknown 가정 → 0x86 으로 migrate. 새 단위 1 (TerminalDied 의 is_web_domain).

---

## 4. Stage 5-C / 5-D 의 명시 deferral (BE-alone 불가)

### 4.1 5-C — session_id-scoped envelope routing

핸드오버 §4.1.A 의 wire 정의: 기존 4 frame (ManipulationSelection 0x81, InputTarget 0x82, ViewportChanged 0x83, FocusMode 0x84) 에 `session_id: Option<String>` 필드 추가.

**불가 이유**:
1. 현재 4 frame 의 outbound broadcast 인프라 **부재** — WS handler 안에서 이들은 inbound-only placeholder (lib.rs:830-840 의 단순 debug log + drop)
2. 5-C 가 의미를 가지려면 *server 가 frame 을 다른 attached webpage 에 fan-out* 해야 함 — 새 broadcast 채널 + 의존
3. fan-out 의 JSON 키 위치 (`session_id` 가 top-level / nested / wrapper?) 는 FE-NEW-6 와 동시 결정 필요

**5-A 가 깔아둔 인프라**: `hub.session_for_cookie(cookie)` 가 준비됨. 5-C 시작 시 cookie 기반 lookup 만 호출하면 됨.

### 4.2 5-D — Auto-mount trigger-aware

핸드오버 §4.1.D: `attach_confirm` 의 spawn loop 가 `mount-cascade` (trigger session) vs `terminal-list-update` (other sessions) 분기.

**불가 이유**:
1. 새 FrameType ID (mount-cascade, terminal-list-update) 정의 — FE 와 같이 결정
2. JSON payload schema (x/y/w/h 의 형식, added 의 형식) FE 와 같이 결정
3. 두 frame 모두 outbound broadcast — 5-C 와 같은 인프라 필요

**5-A 가 깔아둔 인프라**: 같음 — session_for_cookie lookup 사용 가능.

### 4.3 deferral 의 영향

- 본 BE-only 변경은 **wire backward compat** — FE 가 새 frame 을 모르더라도 5-A 의 hub session_table 은 silent (FE 가 0x85 만 처리하면 됨)
- 5-A 의 cookie ↔ session_id 매핑은 *현재 사용처가 0x85 broadcast 외 없음* — 5-C 가 처음 사용처가 될 때까지 idle. 하지만 5-A 의 hygiene (lock-step 4 부착점) 은 코드 정합 보장.
- 0x85 의 FE 처리 — `terminal_id`(UUID) 로 layout 의 schema item 검색, 해당 panel 에 dangling overlay 표시. 본 frame 만 알면 시작 가능.

---

## 5. 핵심 결정 / 회로 (5-A/5-B 의 기록)

| 영역 | 결정 | 출처 |
|---|---|---|
| session_table 자료구조 | `Arc<std::sync::RwLock<HashMap<String, String>>>` | §2.2 (핸드오버 §8.1 (i) 권장 따름) |
| attach ↔ ws-server 통신 | Hub method 직접 호출 (`state.hub.as_ref()?.set_session_for_cookie`) | §1.1 (핸드오버 §8.2 (i) 권장 따름) |
| terminal-died 발행 위치 | hub `publish_terminal_died` API (handler-driven) | §3.3 (핸드오버 §8.3 (ii) 권장 따름) |
| session_id wire 호환 | (5-C 의 deferral 로 미결정) — 5-C 시작 시 핸드오버 §8.4 (ii) `Option<String>` 권장 | §4.1 |
| terminal-died 라우팅 | server-wide (모든 session 의 webpage) — mirror 정합 | §3.5 |
| poisoned RwLock 정책 | warn log + skip (커널 flock 이 진실, hub 는 라우팅 hint) | §2.3 |
| handle_pane_died 시그니처 | `(pane, signal)` — code 의도적 미포함 | §3.4 |
| FrameType 0x85 inbound 정책 | server-only — `policy_violation` close | §3.2 |

---

## 6. AppState / 라우트 surface 의 본 stage 변경

### 6.1 AppState 의 메서드 변경

```rust
// before (Stage 4 + C)
pub async fn handle_pane_died(&self, pane: PaneId);

// after (5-B)
pub async fn handle_pane_died(&self, pane: PaneId, signal: Option<i32>);
```

`AppState` 의 필드 자체는 무변동. 인프라는 `hub.session_table` + `hub.terminal_died_events` 안에 있음 (ws-server 의 책임).

### 6.2 HTTP / WS 라우트 surface

HTTP 라우트 — **무변동**. attach/detach/release path 의 *side effect* 만 확장 (hub API 호출).

WS frame ID 표:

```
0x01 CTRL              (bidi, 변경 X)
0x02 PANE_OUTPUT       (server-only, 변경 X)
0x03 PANE_INPUT        (client-only, 변경 X)
0x04 PANE_RESIZE       (client-only, 변경 X)
0x05 PANE_PAUSE        (client-only, 변경 X)
0x06 PANE_RESUME       (client-only, 변경 X)
0x07 NOTIFY_MIRROR     (server-only, 변경 X)
0x80 LAYOUT_CHANGED    (server-only, 변경 X)
0x81 MANIPULATION_SEL  (client-inbound placeholder; 5-C 에서 outbound 도)
0x82 INPUT_TARGET      (client-inbound placeholder; 5-C 에서 outbound 도)
0x83 VIEWPORT_CHANGED  (client-inbound placeholder; 5-C 에서 outbound 도)
0x84 FOCUS_MODE        (client-inbound placeholder; 5-C 에서 outbound 도)
0x85 TERMINAL_DIED     ★ 신규 (5-B) — server-only, UUID-carrying
```

---

## 7. 빌드 / 테스트 명령

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend

# 빌드
cargo build --workspace
cargo build --release --bin gtmux

# 테스트 (현재 278 PASS / 0 FAIL)
cargo test --workspace --color=never

# 신규 코드 clippy clean (pre-existing 2 warnings 외 무)
cargo clippy -p gtmux-ws-server --no-deps
cargo clippy -p gtmux-http-api --no-deps

# Stage 5-B 빠른 sanity (release binary, env -u TMUX)
# WS 연결 + kill terminal → 0x85 frame 수신 확인 — FE 미구현 시 wscat 으로:
#   ws://127.0.0.1:9999/ws, subprotocol="gtmux.v1, bearer.<token>"
```

---

## 8. 알려진 잔존 / 다음 진입점

### 8.1 본 stage 진입점

핸드오버 §4.4 의 deferred 5+1 + 본 문서 §4 의 5-C / 5-D:

- **5-C (FE 의존)** — session_id-scoped envelope routing
- **5-D (FE 의존)** — auto-mount trigger-aware
- `--session <name>` flag 제거
- legacy `/api/layout` v1 + `LayoutStore` cleanup
- `LayoutSnapshot` ↔ `SessionLayout` 통합
- WS handshake cookie-only 인증 (ADR-0020 D10)
- Settings API (Stage 7 BE-9)
- Rate limiter X-Forwarded-For 신뢰 정책

### 8.2 5-C 시작 시 의사결정 포인트 (FE 동시 진행 시)

핸드오버 §8.4 권장 (`session_id: Option<String>`) 그대로. JSON envelope 안의 위치:
- option (a) `top-level` field (모든 4 frame 에 직접 노출)
- option (b) `meta: { session_id: ... }` (group)
- option (c) frame-specific (`selection: { session_id, panels: [...] }` 같이)

권장: (a) — 다른 server-routed frame (terminal-died 의 `terminal_id` 같이) 와 정합. dispatcher 가 frame parse 없이 routing 결정 가능.

### 8.3 5-D 시작 시 결정 포인트

새 FrameType ID 권장: 0x86 mount-cascade, 0x87 terminal-list-update. payload:
- `mount-cascade`: `{ terminal_id: "uuid", x, y, w, h }` (서버 결정 좌표 — FE 의 spawn 시 grid 이용)
- `terminal-list-update`: `{ added: ["uuid"], removed: ["uuid"] }` (delta)

권장: spawn 호출 점에서 명시 publish — `spawn_terminal_with_uuid` 자체는 trigger 의식 X (handler 가 trigger session 알고 있음).

---

## 9. 의도적 deferral 외의 미해결

- 5-A 의 `clear_sessions_by_name` 은 detach 가 호출. *cookie 가 같은 session 으로 재attach* 시 hub.set 이 다시 채움 — race 가능성? attach 가 `holders.insert` 후 hub.set 까지 single tokio task 안이므로 race 없음. 다른 task 가 그 사이에 detach 를 call 해도 holders 가 비기 전까지 attach 의 hub.set 이 끝남.
- broadcast Lagged 시 terminal-died 가 일부 subscriber 에 drop 가능. 영향: 해당 webpage 의 panel 이 *다음 GET /api/terminals* poll 까지 dangling 표시 못 함. 0032 §5.6 의 동일 정밀화 (P2+).
- `Arc<str>` event 클로닝 cost — 32-deep broadcast 에서 multi-subscriber 의 Drop 비용은 microsecond 단위. 측정 필요 시 P2.

---

## 10. cold-pickup 권장 reading order (5-A/5-B 이후)

1. **본 문서 §0 + §1 + §4** — 한 줄 + 본 stage 산출 + 5-C/5-D deferral 의 사유
2. **0033 §4 + §8** — Stage 5 의 4 batch 명세 + 의사결정 포인트
3. **0032 §5** — Stage 4 의 회로 (cold-pickup 시 PaneId/UUID/metadata 라이프사이클 인지)
4. **ADR-0021 D5** — session-scoped frame routing 의 설계 의도

### 10.1 첫 명령

```bash
cd /Users/ws/Desktop/projects/gtmux
cat docs/reports/0034-stage-5-ab-ws-envelope-be-progress.md
cat docs/reports/0033-next-session-handover-stage-5-entry.md

cd codebase/backend
git log --oneline -5
cargo test --workspace --color=never 2>&1 | grep "test result:"
# expected: 278 PASS / 0 FAIL (workspace 합산)
```

---

## 11. 변경 이력

- 2026-05-15: 초안 — 본 세션 5-A + 5-B 완료 시점의 snapshot. 5-C/5-D deferral 명시.
