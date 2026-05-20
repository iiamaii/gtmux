# 0036 — Stage 5-D path P1 + ADR-0020 D10 α BE progress

- 일자: 2026-05-15
- 작성자: backend agent (5-A/5-B 커밋 `4fb9ecb` 직후 후속 batch)
- 종류: 진행 snapshot — 0035 §4.3 의 BE-단독 진행 가능 3 항목 중 **#3 (5-D P1) + #2 (D10 α)** 완료. **#1 (smoke 10)** 은 release binary + wscat 필요한 수동 검증 — 본 batch 안 land 후 후속 작업으로.
- 후속 reading order: 본 문서 → `0035-be-fe-coordination-stage-5.md` (FE 결정 요청 표) → `0034-stage-5-ab-ws-envelope-be-progress.md` (5-A/5-B) → `0032-stage-4-...` (Stage 4 진실)

---

## 0. 한 줄 요약

0035 §4.3 의 권장 순서대로 **5-D path P1** (`attach_confirm` → `TERMINAL_LIST_UPDATE` 0x87 to non-trigger sessions) + **D10 α** (WS cookie additive auth) BE-단독 ship. **278 → 292 PASS / 0 FAIL** (+14). 5-D path P2 (MOUNT_CASCADE 0x86) + 5-C (session-scoped routing) 는 0035 §3 의 FE 결정 대기.

---

## 1. 본 batch 의 산출

### 1.1 신규/수정 파일

| 파일 | 변경 | 핵심 |
|---|---|---|
| `crates/ws-server/src/hub.rs` | amend | `CookieValidator` async trait 신규 (D10 α). `terminal_list_change_events` broadcast 채널 + `TerminalListChangeEvent` (5-D P1). `cookie_validator: Arc<Mutex<Option<Arc<dyn CookieValidator>>>>` 필드 + `set_cookie_validator`/`cookie_validator` API. `publish_terminal_list_change`/`subscribe_terminal_list_change` API. 단위 +3 (P1 ) |
| `crates/ws-server/src/lib.rs` | amend | `FrameType::TerminalListUpdate = 0x87` + from_u8 갱신. WS handler 의 `terminal_list_change_rx` arm — per-WS-subscriber cookie 기반 필터 (`hub.session_for_cookie` lookup 후 trigger session 일치 시 skip). WS handshake — *cookie auth additive* path (cookie 또는 bearer 둘 중 하나만 통과해도 accept). 기존 frame_type covers / envelope_decode unknown 테스트 0x88 로 migrate. 새 단위 +5 (D10 α) |
| `crates/ws-server/src/payload.rs` | amend | `encode_terminal_list_update(added, removed)` JSON 인코더 + 단위 +2 |
| `crates/ws-server/Cargo.toml` | amend | `async-trait` 의존 추가 |
| `crates/http-api/src/auth.rs` | amend | `#[async_trait] impl gtmux_ws_server::CookieValidator for SessionTable` — `SessionTable::validate` 호출 후 `.is_some()` flatten. rolling-renewal side effect 의 의도된 동작 doc 명시 |
| `crates/http-api/src/sessions.rs` | amend | `attach_confirm_handler` 의 spawn loop 후 — spawned 가 비어있지 않으면 `hub.publish_terminal_list_change(name, &spawned, &[])` |
| `crates/http-api/src/lib.rs` | amend | 통합 +4: `attach_confirm_publishes_terminal_list_change_when_spawn_succeeds`, `attach_confirm_skips_publish_when_no_spawn_lands`, `session_table_cookie_validator_returns_true_for_live_session`, `session_table_cookie_validator_returns_false_for_unknown` |
| `crates/http-api/Cargo.toml` | amend | `async-trait` 의존 추가 |
| `bin/gtmux-cli/src/main.rs` | amend | `hub.set_cookie_validator(app_state.session_table.clone())` — boot 시 등록 |

### 1.2 테스트 변화

| 시점 | PASS | 증감 | 신규 |
|---|---|---|---|
| 5-A/5-B 종료 (0034, commit `4fb9ecb`) | 278 | — | — |
| 5-D P1 종료 | 285 | +7 | ws-server payload 2 + hub 3 + http-api 통합 2 |
| D10 α 종료 | **292** | +7 | ws-server handshake 5 + http-api 통합 2 |

전체 **+14**.

---

## 2. Stage 5-D path P1 디테일

### 2.1 의도

ADR-0021 D3 의 *trigger-aware auto-mount* 중 P1 분기 (이미 layout 에 존재하는 UUID 의 spawn — `attach_confirm` 경로). 의도: trigger 외 attached webpage 의 *Terminal list sidebar* 가 5-s 폴링 latency 없이 즉시 refresh.

### 2.2 wire (FE 와 일치, 0035 §2.3 확정)

```
0x87 TERMINAL_LIST_UPDATE
inner = varint 0 + UTF-8 JSON { "added": ["<uuid>",...], "removed": ["<uuid>",...] }
```

`added`/`removed` 둘 다 *항상* 배열 (빈 배열 허용) — FE `decodeTerminalListUpdate` 의 `parseStringArray` 가 한 pass 로 검증.

### 2.3 broadcast + per-WS 필터

Hub 의 `terminal_list_change_events: broadcast::Sender<TerminalListChangeEvent>` 가 *server-wide* 으로 publish. 각 WS subscriber 가 자기 cookie 의 `hub.session_for_cookie(cookie)` lookup 후:

```
if cookie_value 없음 → skip
else if cookie 의 session 없음 (attach 안 함) → skip
else if cookie 의 session == event.trigger_session → skip (자기 layout 에 이미 있음)
else → 0x87 emit
```

이 패턴이 0035 §3.1 의 (B) "송신자 제외 fan-out" 과 정합 — 단 5-C 의 다른 frame 들은 connection-id 단위 필터가 필요할 수도. P1 은 session 단위로 충분.

### 2.4 race-cleanup

- `attach_confirm_handler` 가 `spawned.is_empty()` 시 publish skip — 모든 WS subscriber 에 noop wakeup 회피
- `hub.publish_terminal_list_change` 의 `_ = ... .send(event)` — broadcast subscriber 수 0 일 때 silent (LayoutChanged 와 같은 패턴)
- broadcast Lagged 시 WS subscriber warn + continue — FE 가 5-s poll 로 catch-up

### 2.5 미해결 (FE 결정 대기 — 0035 §3.2)

- **P2** (`[New Terminal]` 버튼 — MOUNT_CASCADE 0x86) 미진행. FE 가 0035 §3.2 의 (1) P1-only / (2) P1+P2 / (3) default 좌표 source 결정 후 진행.

0x86 frame ID 는 *예약 only* — `FrameType::from_u8(0x86) = None`. FE decoder `decodeMountCascade` 는 작업트리에 있지만, 본 batch 가 BE 에 인식시키지 않음 (의도). 본 정합은 `envelope_decode_unknown_type` 테스트의 주석에 명시.

---

## 3. ADR-0020 D10 α 디테일

### 3.1 의도

D10 의 (α) 단계 = **additive** cookie auth — 기존 bearer subprotocol 보존 + cookie 도 accept. wire backward compat 안전 (FE 변경 X), 보안 표면 동일 (cookie OR bearer, 둘 중 하나만 valid 해도 accept).

(β) 단계 (FE 가 subprotocol bearer 송신 중단) + (γ) 단계 (BE 가 bearer 검증 폐기) 는 0035 §3.3 의 FE 결정 대기.

### 3.2 trait + 의존 그래프

- `ws-server` 안 `CookieValidator` async trait 정의 — *async_trait* 사용 (이미 workspace dep)
- `http-api::SessionTable` 에 impl — `SessionTable::validate` delegation
- 의존성: `ws-server` 가 trait 정의 + `http-api` 가 impl → 그래프 acyclic (ws-server 는 http-api 모름)

```rust
#[async_trait]
pub trait CookieValidator: Send + Sync {
    async fn validate(&self, cookie_value: &str) -> bool;
}
```

### 3.3 WS handshake refactor

기존 (5-A 까지):
```
parse subprotocol → require gtmux.v1 → require bearer.<token> → verify_token → upgrade
```

D10 α 후:
```
parse subprotocol → require gtmux.v1
                  → extract cookie
                  → cookie_ok = validator?.validate(cookie).await
                  → bearer_ok = verify_token(bearer)
                  → if !cookie_ok && !bearer_ok → 401
                  → upgrade
```

응답 메시지 매트릭스 — 401 의 reason 분기:
- bearer 있으나 invalid (cookie 도 invalid) → `invalid token` (attacker probe 신호)
- bearer 없음, cookie 있으나 invalid → `invalid cookie`
- 둘 다 없음 → `bearer token or cookie required`

### 3.4 rolling-renewal side effect

`SessionTable::validate` 는 hit 시 `expires_at` 을 `max_age` 만큼 bump (ADR-0020 D3 rolling renewal). WS handshake 의 cookie validate 가 이 bump 를 trigger — 의도된 동작 (cookie 의 session 이 active WS upgrade 로 *살아있음* 표시). `auth.rs` 의 trait impl doc 에 명시.

### 3.5 test 매트릭스 (handshake)

5 신규 ws-server 단위:

| 시나리오 | 결과 |
|---|---|
| cookie valid + bearer 없음 + validator 등록 | ✅ accept |
| cookie invalid + bearer 없음 + validator 등록 | ❌ reject |
| cookie 없음 + bearer valid + validator 등록 X | ✅ accept (legacy path) |
| cookie invalid + bearer valid + validator 등록 | ✅ accept (additivity) |
| cookie 없음 + bearer 없음 + marker only | ❌ reject |

기존 4 단위 (require_protocol_header / wrong_token / success / client_origin_layout_changed_closes_1008) 는 그대로 통과 — backward compat 보장.

### 3.6 (β)/(γ) 의 미래 작업

0035 §3.3 의 FE 결정 후:
- (β) BE 는 변경 X — FE 가 subprotocol 송신 중단
- (γ) BE 가 bearer 검증 폐기 — `parse_subprotocol` 의 bearer_token 검사 제거 + 본 batch 의 `bearer_ok` 분기 제거. 단, CLI/automation 의 token 인증 path 의 *대체* (예: HTTP `POST /auth/login` + cookie 획득 후 WS) 가 land 한 *후에만* 안전

---

## 4. 0035 §4 의 BE-단독 액션 진행 매트릭스

| 0035 §4.3 항목 | 상태 |
|---|---|
| #1 smoke 10 (0x85 e2e) | ⏳ 후속 — release binary + wscat 필요 (수동 검증) |
| #2 D10 α | ✅ 완료 (본 batch §3) |
| #3 5-D P1 | ✅ 완료 (본 batch §2) |

| 0035 §4.2 FE 응답 후 | 상태 |
|---|---|
| 5-C outbound 인프라 (§3.1) | 대기 — FE A/B/C 결정 |
| 5-D P2 (§3.2) | 대기 — FE P1-only / P1+P2 결정 |
| D10 (β)/(γ) (§3.3) | 대기 — FE deprecation 일정 |
| Legacy /api/layout cleanup (§3.4) | 대기 — FE migrate 완료 통보 |

---

## 5. 핵심 결정 / 회로 (본 batch 안 굳어진 것)

| 영역 | 결정 | 위치 |
|---|---|---|
| 0x86 frame ID 예약 only | FE decoder 는 작업트리, BE 의 `from_u8` 은 None — *FE 응답 대기 중 wire 변경 없음* | §2.5 |
| TERMINAL_LIST_UPDATE 의 empty 정책 | spawned 가 빈 batch 는 publish skip — broadcast wakeup 노이즈 회피 | §2.4 |
| broadcast Lagged 영향 | warn + continue. 5-s poll 이 fallback | §2.4 |
| CookieValidator trait 위치 | `ws-server` 안 정의 + `http-api` impl. 그래프 acyclic | §3.2 |
| 401 reason 매트릭스 | bearer-invalid > cookie-invalid > marker-only | §3.3 |
| rolling-renewal trigger | WS upgrade 도 trigger 로 카운트 (의도) | §3.4 |

---

## 6. AppState / 라우트 surface (본 batch 변경)

### 6.1 AppState — 무변동

`AppState` 자체는 5-A/5-B 의 15 필드 그대로. 변경은 `Hub` 와 `attach_confirm_handler` 의 *side effect* 만.

### 6.2 Hub — 새 필드 1 + API 4

```rust
pub struct Hub {
    // ... 5-A/5-B 의 11 필드 ...
    cookie_validator: Arc<std::sync::Mutex<Option<Arc<dyn CookieValidator>>>>,  // D10 α
}

// API
pub fn set_cookie_validator(&self, validator: Arc<dyn CookieValidator>);
pub fn cookie_validator(&self) -> Option<Arc<dyn CookieValidator>>;
pub fn publish_terminal_list_change(&self, trigger_session: &str, added: &[String], removed: &[String]);
pub fn subscribe_terminal_list_change(&self) -> broadcast::Receiver<TerminalListChangeEvent>;
```

### 6.3 WS frame 표 (Stage 5-D P1 후)

```
0x85 TERMINAL_DIED      ✅ ship (5-B)
0x86 MOUNT_CASCADE      ❌ reserved (FE decoder ready, BE 미발행 — §2.5)
0x87 TERMINAL_LIST_UPDATE ✅ ship (5-D P1)
```

`is_web_domain` 정합: 3 frame 모두 high-bit set → marker 통과.

---

## 7. 빌드 / 테스트 명령

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo build --workspace
cargo build --release --bin gtmux

cargo test --workspace --color=never  # 292 PASS / 0 FAIL

cargo clippy -p gtmux-ws-server -p gtmux-http-api --no-deps
# pre-existing 2 warnings (unused Duration / dead unlink_stale) 외 무
```

### 7.1 D10 α 빠른 sanity (release binary)

```bash
TOKEN=$(cat ~/.local/state/gtmux/demo.token)

# 1. /auth/login 으로 cookie 획득
COOKIE=$(curl -sS -i \
  -H "Host: 127.0.0.1:9999" \
  -H "Origin: http://127.0.0.1:9999" \
  -X POST http://127.0.0.1:9999/auth/login \
  -d "{\"token\":\"$TOKEN\"}" \
  -H "Content-Type: application/json" \
  | awk -F': ' '/^set-cookie: gtmux_auth=/ { sub(/;.*/,"",$2); print $2 }' \
  | head -1)
echo "$COOKIE"

# 2. cookie-only WS handshake (bearer 없이) — D10 α path
#    subprotocol = "gtmux.v1" 만 송신, bearer.<token> 생략
wscat \
  --header "Cookie: $COOKIE" \
  --subprotocol "gtmux.v1" \
  -c "ws://127.0.0.1:9999/ws"
# → 정상 upgrade + 첫 frame (LAYOUT_CHANGED hello) 수신 확인
```

### 7.2 5-D P1 빠른 sanity

```bash
# 2 session + 2 webpage 시뮬레이션 (cookie A, cookie B)
# A 가 sessA 의 attach + confirm → spawn → B 가 sessB attach 중이면 B 의 WS 가 0x87 수신
# (현재 자동 smoke 없음 — wscat 으로 0x87 envelope 확인)
```

---

## 8. 잔여 / 다음 진입점

### 8.1 본 batch 후의 BE 진입점

순서 우선:

1. **smoke 10** (0x85 e2e) — wscat 으로 release binary 검증. 자동 smoke 스크립트화 검토
2. **5-D P2** (FE 응답 대기) — `[New Terminal]` 새 endpoint + MOUNT_CASCADE publisher
3. **5-C** (FE 응답 대기) — selection/viewport/focus 의 outbound broadcast + session-scoped 라우팅
4. **D10 (β)/(γ)** (FE deprecation 일정 대기)
5. **Legacy /api/layout cleanup** (FE migrate 완료 후)
6. **`--session <name>` flag 제거** (Stage 6+ 큰 refactor)
7. **Settings API** (Stage 7 BE-9)

### 8.2 0035 의 FE 응답 대기 항목 (재게재)

- 0035 §3.1: 5-C broadcast trigger 패턴 (A/B/C)
- 0035 §3.2: 5-D P1 only / P1+P2
- 0035 §3.3: D10 α/β/γ 일정
- 0035 §3.4: layout cleanup 시점

본 batch 후 0035 §1 의 매트릭스 갱신 필요:
- "5-D: ❌ → ✅ ship (P1 only)"
- "D10 α: ❌ → ✅ ship"

---

## 9. cold-pickup 권장 reading order

1. **본 문서 §0 + §1 + §4** — 한 줄 + 산출 + 0035 의 BE-단독 액션 매트릭스
2. **0035 §3** — FE 결정 요청 4 항목 (5-C 패턴, P2 분기, D10 단계, layout migrate)
3. **0034 §3 / §4** — 5-B + 5-C/5-D 의 명세 배경
4. **ADR-0020 D10** — cookie auth 의 단일 진실 채널 설계 의도
5. **ADR-0021 D3** — auto-mount trigger 의 의도 + cascade vs list-update 의 routing 의미

### 9.1 첫 명령

```bash
cd /Users/ws/Desktop/projects/gtmux
cat docs/reports/0036-stage-5-dp1-and-d10-alpha-be-progress.md
cat docs/reports/0035-be-fe-coordination-stage-5.md

cd codebase/backend
git log --oneline -5
cargo test --workspace --color=never 2>&1 | grep "test result:"
# expected: 292 PASS / 0 FAIL (workspace 합산)
```

---

## 10. 변경 이력

- 2026-05-15: 초안 — 5-A/5-B 의 후속 batch (5-D P1 + D10 α) 완료 시점.
