# 0035 — BE ↔ FE 협의 정리 (Stage 5 잔여 / FE 의존 항목)

- 일자: 2026-05-15
- 작성자: backend agent (Stage 5-A/5-B 커밋 `4fb9ecb` 직후)
- 종류: 협의 doc — BE 다음 진입 전 FE 측 결정 / 확인이 필요한 항목을 한 문서에 합본
- 후속 reading order: 본 문서 → `0034-stage-5-ab-ws-envelope-be-progress.md` (BE 산출) → `0033-fe-stage-1-to-5-partial-progress.md` (FE 산출) → `0033-next-session-handover-stage-5-entry.md` (Stage 5 전체 명세)

---

## 0. 한 줄 요약

BE 가 Stage 5-A + 5-B 를 `4fb9ecb` 로 main 에 ship. **FE 는 이미 0x85 / 0x86 / 0x87 의 decoder + handler + 5-C scaffold (`isFrameForActiveSession`) 까지 미커밋 작업트리에 구현됨** — BE 0034 의 §8.3 권장 wire shape 그대로 따름. 따라서 *FE 의존* 으로 deferral 된 BE 5-C/5-D 중 **5-D 는 사실상 BE-단독으로 ship 가능** (FE 가 이미 준비). **5-C 는 outbound broadcast trigger 패턴 1 결정** + ADR-0020 D10 (WS cookie auth) 의 *전환 시점* 1 결정만 받으면 진행 가능.

---

## 1. 현황 매트릭스 (BE / FE)

| 항목 | BE 상태 | FE 상태 | gap |
|---|---|---|---|
| 5-A: Hub cookie ↔ session_name table | ✅ ship `4fb9ecb` (`Hub::set/clear/clear_by_name/session_for_cookie`) | (소비 코드 없음 — 5-C 의 라우팅에서만 사용) | 없음 |
| 5-B: 0x85 TERMINAL_DIED frame | ✅ ship `4fb9ecb` (`encode_terminal_died` + `publish_terminal_died`) | ✅ decoder `decodeTerminalDied` + handler `handleTerminalDied` 작업트리 (`lib/ws/decode.ts:503` / `dispatcher.svelte.ts:438`). server-wide 정합 인지 (`mirror 정합`) | **없음** — BE ship + FE 가 이미 소비 — 정합 검증만 필요 (smoke gate) |
| 5-C: session_id-scoped envelope routing (0x81~0x84 + session_id field) | ❌ 미진행 (4 frame 현재 inbound-only placeholder, 0x80 LAYOUT_CHANGED 만 outbound) | ⚠️ scaffold (`isFrameForActiveSession(frameSessionId)` 헬퍼만 있음, decoder 의 `sessionId` 반환 amend 는 BE wire 확정 후) | **broadcast trigger 패턴 1 결정** (§3.1) + **outbound 인프라 BE 구축** |
| 5-D: 0x86 MOUNT_CASCADE / 0x87 TERMINAL_LIST_UPDATE | ❌ 미진행 (publisher 없음) | ✅ decoder + handler 완성 (`decodeMountCascade`/`decodeTerminalListUpdate` + `handleMountCascade`/`handleTerminalListUpdate`). payload shape 는 0034 §8.3 권장과 정확 일치 | **trigger session 식별 패턴 1 결정** (§3.2). 그 외 BE-단독 ship 가능 |
| ADR-0020 D10: WS cookie-only auth | ❌ 미진행 (현재 `bearer.<token>` subprotocol *필수*) | (FE 가 cookie + subprotocol 둘 다 송신 — 둘 중 하나만 valid 해도 통과 가능한 *additive* path 가 BE 에 land 하면 FE 작업 없이 자동 정합) | **전환 시점 1 결정** (§3.3) |
| Legacy `/api/layout` v1 cleanup | ❌ 미진행 | ⚠️ FE 가 `$lib/http/layout.ts` 에서 활발히 사용 | FE 의 *migrate 완료 시점* 통보 필요 — 그 전까지 BE 보존 (§3.4) |

---

## 2. FE 가 이미 정한 사실 (BE 0034 권장과 일치)

FE 작업트리 안 (미커밋) 의 `lib/ws/decode.ts` + `dispatcher.svelte.ts` 가 *0034 §8.3 권장* 을 그대로 채택. 핵심:

### 2.1 0x85 TERMINAL_DIED (BE ship 완료)

```
inner = varint 0 + UTF-8 JSON { "terminal_id": "<uuid>", "reason": "exit" | "killed" }
```

- 라우팅: server-wide (mirror 정합)
- FE 처리: `danglingTerminals.mark(terminalId, reason)` + `terminalPool.refresh()`
- BE 정본: `crates/ws-server/src/payload.rs::encode_terminal_died` (0034 §3.2)

### 2.2 0x86 MOUNT_CASCADE (BE 미진행, FE 준비됨)

```
inner = varint 0 + UTF-8 JSON { "terminal_id": "<uuid>", "x": <num>, "y": <num>, "w": <num>, "h": <num> }
```

- 라우팅: trigger session 의 webpage **만** (5-A 의 `hub.session_for_cookie` 사용)
- 정합 invariants (FE decoder 가 거부 조건):
  - `terminal_id` ≠ 빈 문자열
  - `x`, `y` finite (음수 허용)
  - `w > 0`, `h > 0` (finite)
- FE 처리: `mutateLayout(name, cur => append TerminalItem)` (idempotent — 이미 존재 시 no-op)
- z 결정: FE 가 `max(z) + 1` 로 결정 (BE 는 결정 X)

### 2.3 0x87 TERMINAL_LIST_UPDATE (BE 미진행, FE 준비됨)

```
inner = varint 0 + UTF-8 JSON { "added": ["<uuid>", ...], "removed": ["<uuid>", ...] }
```

- 라우팅: non-trigger session 의 webpage (= trigger 이외 모든 attached webpage)
- 정합 invariants (FE decoder 가 거부 조건):
  - `added`, `removed` 모두 string-array (빈 배열 허용)
- FE 처리: `terminalPool.refresh()` (hint delta — authoritative source 는 여전히 GET /api/terminals)

### 2.4 5-C 의 session_id field (BE/FE 모두 미진행)

FE scaffold (`isFrameForActiveSession`) 가 가정하는 *0034 §8.4 (ii) + §8.2 (a)* — *optional top-level `session_id` field*:

```
inner = varint 0 + UTF-8 JSON {
  "session_id": "<name>",     // optional — 부재 시 server-wide 로 해석
  "panels": [...]               // 0x81 의 경우
  // 또는 frame 별 다른 keys
}
```

FE 의 drop 정책: `frameSessionId === active.name` 일 때만 처리. session_id 가 null/undefined 면 server-wide 로 통과 (정상 경로). 본 추가 필터는 *BE 의 fan-out 오라우팅* + *FE 의 active 변경 race* 의 두 safety net.

---

## 3. FE 결정이 필요한 3 항목

### 3.1 5-C — selection/viewport/focus 의 broadcast trigger 패턴 ★

**문제**: 현재 4 frame (0x81 M_CHANGED, 0x82 I_CHANGED, 0x83 VIEWPORT_CHANGED, 0x84 FOCUS_MODE_CHANGED) 은 *FE → BE inbound* placeholder. server 가 어떻게 fan-out 할지의 model 미결정.

**option**:

- **(A) Echo broadcast** — FE A 가 0x81 송신 → BE 가 *같은 session 의 모든 webpage* (송신자 포함) 에 fan-out. FE 가 자기 echo 도 처리 (idempotent — 자기 store 갱신이 노옵).
- **(B) Echo broadcast minus sender** — BE 가 송신 connection 제외하고 fan-out. FE 는 echo 처리 필요 X.
- **(C) Server-authoritative** — BE 가 selection/viewport/focus 의 진실 출처. FE 는 BE 에 mutation 요청 (e.g. CTRL `set-selection`) → BE 가 *모든 webpage* (송신자 포함) 에 broadcast → FE 가 received frame 으로만 store 갱신 (optimistic UI X).

**BE 권장: (B)** — 송신자 제외 fan-out. 이유:
- (A) 는 FE 가 자기 송신을 echo 로 다시 받음 — store 가 이미 갱신된 상태에서 같은 값 다시 set → idempotent 이지만 redundant
- (C) 는 latency 증가 (raindrop animation 같은 viewport pan 의 즉시 반응 X) + FE 의 optimistic UI 패턴 차단
- (B) 는 multi-tab 동기화 + 송신자 즉시 반응 + redundant write 회피의 sweet spot

**FE 결정 요청**:
1. (A)/(B)/(C) 중 어느 것?
2. (B) 채택 시 BE 가 송신자를 어떻게 식별? — 현재 cookie 단위. 같은 cookie 의 *동일 WS connection* 은 connection-table 의 id 로 가능. BE 의 connection-table 확장 (5-A 의 cookie ↔ session 외에 *connection id* 추가) 검토 필요
3. session_id field 의 위치 — top-level (§2.4 권장) vs nested?

답변이 (A) 면 BE 는 connection-table 확장 X (cookie-only routing). (B) 면 connection-table 확장 필요. (C) 면 client→server 의 CTRL command 추가.

### 3.2 5-D — `[New Terminal]` 의 trigger session 식별 패턴

**문제**: FE 가 `MOUNT_CASCADE` 를 받기 위해서는 "어느 session 이 trigger 인지" 가 BE 측에 있어야 함. 현재 BE 의 spawn 진입점 (`spawn_terminal_with_uuid`) 은 caller 의 session 정보 모름.

**상황 분석**:

| spawn 경로 | trigger session 의 출처 | 5-D fan-out 의미 |
|---|---|---|
| `attach_confirm` (unmatched UUID spawn) | 이미 layout 에 있는 UUID — *layout 안에 좌표/위치 있음* → MOUNT_CASCADE 불필요. 같은 session 안 FE 는 layout 그대로 보존 + match 후 alive 상태로만 변환 | **MOUNT_CASCADE X** — TERMINAL_LIST_UPDATE 만 다른 session 에 broadcast |
| `[New Terminal]` 버튼 (Stage 5-C/5-D 미구현 FE flow) | FE 가 CTRL `new-terminal` 송신 + cookie + active session | **MOUNT_CASCADE** 를 trigger session 에 + TERMINAL_LIST_UPDATE 를 others 에 |

**핵심 갈림**: `[New Terminal]` 의 wire flow 미정의. 현재 FE 의 `NewPanelButton` 은 legacy WS 의 CTRL `new-pane` 호출 (G3 의 "multi-session 사용자에게는 작동 X"). multi-session 에서 새 terminal 생성 path 가 *PUT layout (UUID 직접 추가) + /attach/confirm* 이라면, MOUNT_CASCADE 는 attach_confirm path 에서 trigger 식별 가능 (cookie → session 매핑 사용).

**BE 권장**: 두 경로 분리.
- **경로 P1 (attach_confirm spawn — 이미 layout 에 있음)**: TERMINAL_LIST_UPDATE 만 publish (added: [uuid], removed: []) — 다른 session 의 pool 갱신용. MOUNT_CASCADE 발행 X.
- **경로 P2 (`[New Terminal]` 버튼 — FE 가 BE 에 명시 요청)**: BE 가 새 endpoint `POST /api/sessions/:name/terminals` 추가 (또는 CTRL `new-terminal` over WS) → BE 가 default 좌표로 layout 의 새 item 추가 + MOUNT_CASCADE (trigger) + TERMINAL_LIST_UPDATE (others) publish.

**FE 결정 요청**:
1. 경로 P1 만 우선 (`[New Terminal]` 은 P3 으로 deferral) — Stage 5-D 의 ship 단순화
2. 또는 P2 도 동시 진행 — 새 endpoint `POST /api/sessions/:name/terminals` 의 default 좌표 정책 (cascade offset? grid-fit?) 결정
3. P2 의 경우 default 좌표 source: BE 가 결정 (e.g., max+32 offset cascade) vs FE 가 미리 보내고 BE 는 그대로

답변에 따라 BE 5-D 의 scope 가 결정 — P1 만이면 1 batch, P2 까지면 2 batch.

### 3.3 ADR-0020 D10 — WS cookie-only auth 의 전환 시점

**문제**: 현재 WS handshake 는 `bearer.<token>` subprotocol **필수** + cookie 로 disconnect 라우팅. D10 은 cookie 가 *단일 인증 채널* 이라고 명시. 전환 path 옵션:

**option**:

- **(α) Additive (즉시 ship 가능)** — BE 가 cookie 검증을 *추가* path 로 둠. 우선순위: subprotocol token > cookie. 둘 다 invalid 면 401. FE 변경 X (subprotocol 송신 유지). **위험: 0. 보상: cookie-only client 가능, 기존 client 무손**.
- **(β) Cookie-first (FE 동시 작업)** — BE 가 cookie 우선 검증. subprotocol bearer 는 *deprecated fallback*. FE 가 subprotocol 송신 중단 시점 정함.
- **(γ) Cookie-only (deprecation 완료)** — subprotocol bearer 송신/검증 모두 폐기. FE 가 subprotocol 안 보내야 함. **breaking change**.

**BE 권장: (α) → (β) → (γ)** 3-step. (α) 는 BE 단독 land — wire backward compat 안전. (β) 는 FE 의 deprecation timeline 동기. (γ) 는 deprecation 완료 후.

**FE 결정 요청**:
1. (α) 즉시 진행 — BE 가 cookie auth additive land. FE 변경 없음. ✅ 권장
2. (β) 의 transition 기간 (1 release? 2 release?)
3. (γ) 진행 여부 — 또는 (β) 영구 유지

### 3.4 Legacy `/api/layout` v1 cleanup — FE 의 migrate 완료 통보

**문제**: BE 는 v1 endpoint (`GET/PUT /api/layout`) + `LayoutStore` + `LayoutSnapshot` 보존 중. FE 가 v2 (`/api/sessions/:name/layout`) 로 *완전* migrate 하면 BE 는 cleanup 가능.

**현재 FE 사용처** (grep 결과):
- `lib/http/layout.ts` — GET / PUT 양쪽 활발
- `lib/canvas/Canvas.svelte` — 주석 안 만
- `lib/stores/layout.svelte.ts` — store base
- `lib/ws/dispatcher.svelte.ts` — 0x80 LAYOUT_CHANGED 후 GET /api/layout

**FE 결정 요청**:
1. v2 로 완전 migrate 시점 — Stage 5? 6? 7?
2. migrate 완료 통보 시점 BE 가 v1 endpoint 제거 + `LayoutStore` 제거 + `LayoutSnapshot` ↔ `SessionLayout` 통합

---

## 4. BE 의 진행 가능 액션 (FE 응답 받기 전/후)

### 4.1 FE 응답 받기 전에 가능

**즉시 (5-B smoke)**:
- 0x85 TERMINAL_DIED 의 end-to-end smoke gate — release binary + curl spawn + curl kill + wscat 으로 0x85 frame 수신 확인 + JSON 매트릭스 (exit/killed) 확인. FE 의 `handleTerminalDied` 가 같은 wire 를 소비하므로 BE 측 검증만으로 충분.

**점진**:
- 5-D 경로 P1 (attach_confirm path 의 TERMINAL_LIST_UPDATE publisher) — *attach_confirm 의 spawn loop 안에서 다른 session 의 webpage 에 broadcast*. trigger session 식별 = cookie 의 session 이므로 다른 session = *그 cookie 외의 모든 attached cookie*. 본 path 는 §3.2 의 답 (P1-only or P1+P2) 에 무관 — 항상 land 가능.
- ADR-0020 D10 의 (α) — cookie additive auth — §3.3 의 답 (α 채택 시) BE 단독 land.

### 4.2 FE 응답 받은 후 가능

- 5-C 의 outbound 인프라 (§3.1 결정 후)
- 5-D 경로 P2 (`[New Terminal]` 새 endpoint, §3.2 결정 후)
- D10 (β)/(γ) 단계 (§3.3 결정 후)
- Legacy `/api/layout` cleanup (§3.4 결정 후)

### 4.3 권장 진행 순서 (FE 응답 무관)

1. **smoke 10** — 5-B 의 0x85 검증 (BE 단독, FE 응답 X)
2. **§3.3 의 (α)** — D10 cookie additive auth (BE 단독, FE 변경 X)
3. **§3.2 의 P1** — TERMINAL_LIST_UPDATE publisher in attach_confirm (BE 단독, FE 가 이미 0x87 소비 준비)
4. **§3.1/§3.2 P2/§3.3 (β,γ)/§3.4** — FE 응답 후 진행

### 4.4 본 협의 doc 가 처리 안 한 잔존

- `--session <name>` flag 제거 — Stage 6+ refactor (CLI 의 token/pid/config 명명 종속, 단일 doc 으로 정리 어려움). 별 doc 필요 시 0036 으로 분리.
- Settings API (Stage 7 BE-9) — out-of-stage. ADR-0020 D11 에 spec 있음.
- Rate limiter X-Forwarded-For — Cloud mode only, edge.
- WS subscriber Lagged reconciliation — 0032 §5.6 / 0034 §9 의 P2+.

---

## 5. FE 응답 양식 (제안)

각 항목별로 짧게 답변하면 BE 가 다음 batch 진입 가능. 답변 위치: 본 doc 의 §3 표 갱신 (또는 별 reply doc).

```
§3.1 5-C broadcast trigger: [A / B / C], 송신자 식별 방식: [...], session_id 위치: [top-level / nested]
§3.2 5-D 경로: [P1 only / P1+P2], P2 의 default 좌표 source: [BE / FE / ...]
§3.3 D10: [α 즉시 / α+β 동시 / α+β+γ 동시], (β) 기간: [...]
§3.4 layout cleanup: [Stage 5 / 6 / 7]
```

---

## 6. 현 빌드 / 테스트 / git 상태

```
HEAD → 4fb9ecb  feat(backend): Stage 5-A/5-B — hub session table + terminal-died frame
       03056bb  fix(backend): preserve terminal metadata across kill+respawn cycle
       bcd54de  feat(backend): multi-session pivot stages 1-4

cargo test --workspace --color=never  → 278 PASS / 0 FAIL
미커밋: FE 작업트리 + agent/system files (BE 책임 밖)
```

본 doc 자체는 미커밋 — FE 응답을 본 doc 안에 갱신 후 commit 권장 (round-trip 보존).

---

## 7. FE 응답 (2026-05-15, frontend agent)

§3 의 4 항목에 대한 FE 측 결정. BE 가 본 답을 기준으로 §4.2 / §4.3 의 다음 batch 진입 가능.

### 7.1 §3.1 답 — 5-C broadcast trigger: **(B) Echo broadcast minus sender**

- **선택**: (B) 송신자 제외 fan-out
- **사유**:
  1. (A) 의 self-echo 는 redundant — FE 가 send 시점에 *이미* `ephemeralStore` / `sessionStore.viewport` 를 갱신한 상태에서 같은 값을 다시 set 받는 것은 reactive 알림을 무의미하게 트리거. (B) 가 *FE optimistic UI + BE multi-tab sync* 의 sweet spot.
  2. (C) server-authoritative 는 viewport pan/zoom 같은 high-frequency interaction 에 round-trip latency 추가 — UX 저하. 또한 FE 의 `ephemeralStore` mutation 흐름이 모두 inbound 만 받는 구조가 되어 *optimistic UI 패턴 자체* 가 사라짐.
- **송신자 식별 방식**: BE 의 connection-table 확장 (5-A 의 `cookie ↔ session_name` 외에 `connection_id: Uuid` 추가). FE 는 변경 X — 송신 frame 에 connection_id 를 *FE 가 명시 송신할 필요 없음* (server 가 receive 시점에 own connection 의 id 를 알고 있음, 같은 id 의 connection 만 제외하고 fan-out).
- **session_id 위치**: top-level (§2.4 + 0034 §8.2 option (a)). FE 의 `isFrameForActiveSession()` scaffold 가 이 가정으로 작성됨. 다른 routing 정보 (terminal_id 등) 와 정합.

### 7.2 §3.2 답 — 5-D 경로: **P1 + P2 (동시 진행)**

- **선택**: P1 (attach_confirm path 의 TERMINAL_LIST_UPDATE) **+** P2 (`POST /api/sessions/:name/terminals` 새 endpoint)
- **사유**:
  1. P1 only 면 fresh session (empty layout) 에서 사용자가 *새 terminal 을 만들 수 없음* — demo blocker. attach_confirm 는 *layout 에 이미 있는 UUID* 의 spawn 경로일 뿐.
  2. multi-session 의 user-facing "[New Terminal]" 동작이 정식 wire flow 없이는 FE 가 항상 *manual UUID 생성 + mutateLayout PUT + attach_confirm POST* 를 emulate 해야 함 (3-step round-trip). 신규 endpoint 1 번 호출이 훨씬 깔끔.
  3. P2 의 endpoint 가 있어야 ADR-0015 의 "trigger session 만 cascade" invariant 가 BE 의 단일 진실 source 로 강제됨.
- **P2 의 default 좌표 source**: **BE 결정**. cascade offset (예: 기존 max(x), max(y) + 32) 또는 fixed origin (empty layout 시). FE 가 viewport-aware 좌표를 추후 hint 로 보낼 수도 있으나 (request body 에 `x?, y?` optional), MVP 는 BE-only. FE 의 `handleMountCascade` 가 server-supplied x/y/w/h 그대로 사용.
- **응답 shape 권장** (FE 가 호출 시 사용할 표면):
  ```
  POST /api/sessions/:name/terminals
    body: {} (또는 optional { x?: number, y?: number, w?: number, h?: number, label?: string })
    200: { id: "<uuid>", x, y, w, h }    // FE 가 idempotent guard 용으로 사용
    409: lock conflict (다른 cookie 가 attach 중)
    403: forbidden (cookie 의 session 이 :name 과 불일치)
  ```
  - FE 는 200 응답을 *추가 read 로 사용하지 않음* — 같은 정보가 MOUNT_CASCADE 로 도착하여 layout 에 반영. 응답은 client error path (e.g., toast) 용. BE 의 endpoint 실행 직후 MOUNT_CASCADE publish 시점이 응답 reply 보다 빠를 수도 늦을 수도 있음 — handler 의 idempotent guard (이미 items 에 있음) 가 두 path 모두 안전.

### 7.3 §3.3 답 — D10 transition: **(α) 즉시 + (β) Stage 6 + (γ) Stage 7**

- **선택**:
  - **(α) BE 단독 land — 즉시 OK** (FE 변경 X, 기존 subprotocol bearer 송신 유지). 위험 0.
  - **(β) cookie-first, subprotocol bearer = deprecated fallback — Stage 6 동시 작업.** FE 가 subprotocol bearer 송신을 *log only* (deprecation warning) 로 보존하다 Stage 7 에서 send 코드 제거.
  - **(γ) cookie-only, subprotocol bearer 검증/송신 완전 폐기 — Stage 7 cleanup batch.** FE 의 `sessionStorage.gtmux_token` 저장 / `acquireToken()` / WS subprotocol 송신 코드 삭제. BE 의 subprotocol 검증 path 제거.
- **(β) transition 기간**: Stage 6 ~ Stage 7 사이 (1 release 정도 — Stage 6 의 panel header V2 / layer list V2 작업과 함께 묶기). FE-only 의 P0/P1 (0036) 처리가 더 시급하므로 Stage 5 의 끝자락 ~ Stage 6 의 진입점.
- **(α) 즉시 진행 의 즉시 효과**: FE 는 무변경. 미래의 cookie-only client (예: WS 만 cookie 로 인증하는 별 SPA) 가 가능해짐 — multi-tab 안전성 + cookie httpOnly 의 XSS 안전성. legacy bearer path 는 backward-compat 그대로.

### 7.4 §3.4 답 — Legacy `/api/layout` v1 cleanup: **Stage 5 후반**

- **선택**: Stage 5 후반 (BE Stage 5-C/5-D land + FE 의 0036 P0/P1 fix + Stage 5 creation gestures land 후) 부터 BE 가 v1 endpoint 제거 가능.
- **사유**:
  1. 현재 FE 의 `lib/http/layout.ts` (v1 GET/PUT) 가 *legacy single-session demo path* + `NewPanelButton` (G3) + `appendPanelIfMissing` 의 3 곳에서 사용 중.
  2. 0036 P1-D 의 auto-mount guard (`sessionStore.active !== null` 일 때 v1 mutation 차단) 는 *isolation* 만 — 실 사용 path 자체는 legacy demo 용으로 살아 있음.
  3. **마이그레이션 완료 시점 통보 trigger**: FE 가 다음 3 항목 완료 시 BE 에 시그널.
     - (a) Stage 5 creation gestures + non-terminal Node renderers land — multi-session 에서 사용자가 모든 도구를 사용 가능 (legacy demo path 무필요).
     - (b) NewPanelButton (legacy WS CTRL `new-pane`) → multi-session `POST /api/sessions/:name/terminals` (위 §7.2 P2) 로 migrate.
     - (c) WS auto-mount handler 의 legacy `appendPanelIfMissing` call → 0x86 MOUNT_CASCADE 의 dispatcher.handleMountCascade 로 단일화.
  4. 완료 통보 path: 본 0035 doc 의 §7.4 갱신 또는 별 doc (예: `0040-legacy-layout-v1-deprecation-ready.md`) 로 BE 에 알림.

### 7.5 응답 요약 (한 줄 양식)

```
§3.1 5-C broadcast trigger: (B) echo minus sender,
                            송신자 식별: BE connection-table 의 connection_id 확장,
                            session_id 위치: top-level
§3.2 5-D 경로: P1 + P2 (동시),
              P2 default 좌표 source: BE (cascade offset),
              endpoint: POST /api/sessions/:name/terminals
§3.3 D10: α 즉시 (BE 단독), β Stage 6, γ Stage 7
§3.4 layout cleanup: Stage 5 후반 (creation gestures + NewPanelButton migrate + auto-mount unification 후)
```

### 7.6 FE 측 follow-up 작업 (응답 이후 진행 예정)

- Stage 5 의 Creation gestures (Toolbar 12 도구 → Canvas click/drag-to-create) — `toolStore.consume()` 정합
- Stage 5 의 Non-terminal Node renderers (TextNode/NoteNode/ShapeNode/LineNode/FilePathNode)
- 0036 P0-A/B/C, P1-D — 본 응답 commit 직전에 land
- BE Stage 5-C ship 시 `isFrameForActiveSession()` scaffold 의 실 wire-up (decoder 의 sessionId 반환 amend + 4 handler 의 가드 추가)
- BE P2 endpoint ship 시 `NewPanelButton` 의 multi-session path wire (legacy WS CTRL → `POST /api/sessions/:name/terminals`)

---

## 8. 변경 이력

- 2026-05-15: 초안 — BE 5-A/5-B ship 직후, FE 의 0x85/0x86/0x87 + 5-C scaffold 발견 후 작성. §3 의 3+1 결정 요청 항목, §4 의 BE 진행 가능 단독 액션 목록 포함.
- 2026-05-15: FE 응답 (§7) 추가 — 4 결정 모두 답변. BE 가 §4.3 의 권장 순서 + §7.2 의 P2 endpoint scope 로 다음 batch 진입 가능.
