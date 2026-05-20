# 0039 — FE 연동 가이드 (Stage 5 BE wire surface)

- 일자: 2026-05-15
- 작성자: backend agent (`d00db66` 직후 시점)
- 종류: 연동 spec — FE agent 가 multi-session terminal 흐름을 BE 와 정합하게 wire 하기 위한 단일 진입 문서
- 대상: frontend 개발 agent
- 후속 reading order: 본 문서 → `0036-frontend-review-action-items.md` (FE 측 review) → `0038-stage-5-next-batch-queue.md` (BE 다음 batch 큐) → `0034`/`0036(BE)` (BE 산출 snapshot)

---

## 0. 한 줄 요약

BE 가 Stage 5-A/5-B/5-D P1 + D10 α + 0x88 TERMINAL_SPAWNED 까지 ship (HEAD `d00db66`). **FE Issue C (multi-session terminal xterm 연결 불가) 의 BE-측 enabler 가 모두 land**. FE 는 (1) 0x88 decoder/handler 추가 + (2) `XtermHost` 의 terminal 모드 활성화 + (3) cross-session leak 임시 가드 — 3 작업으로 Option C1 (실 streaming) 진입 가능.

---

## 1. 현 BE wire surface (HEAD `d00db66`)

### 1.1 WS frame ID 표

| ID | 이름 | 방향 | 라우팅 | 발행 시점 | doc |
|---|---|---|---|---|---|
| `0x01` | CTRL | both | per-conn | client ↔ server | wire SSoT |
| `0x02` | PANE_OUT | server→client | server-wide | pty bytes 흐름 | wire SSoT |
| `0x03` | PANE_IN | client→server | per-conn | 키 입력 | wire SSoT |
| `0x04` | PANE_RESIZE | client→server | per-conn | xterm resize | wire SSoT |
| `0x05` | PANE_PAUSE | client→server | per-conn | pause toggle | wire SSoT |
| `0x06` | PANE_RESUME | client→server | per-conn | resume toggle | wire SSoT |
| `0x07` | NOTIFY_MIRROR | server→client | server-wide | pane-spawned / pane-died / layout-changed / server-ready | wire SSoT |
| `0x80` | LAYOUT_CHANGED | server→client | server-wide | v1 PUT /api/layout | legacy |
| `0x81` | M_CHANGED | client→server (5-C 후 양방향) | inbound-only (현재) | selection 변동 | 0035 §3.1 |
| `0x82` | I_CHANGED | client→server (5-C 후 양방향) | inbound-only (현재) | input target 변동 | 0035 §3.1 |
| `0x83` | VIEWPORT_CHANGED | client→server (5-C 후 양방향) | inbound-only (현재) | viewport 변동 | 0035 §3.1 |
| `0x84` | FOCUS_MODE | client→server (5-C 후 양방향) | inbound-only (현재) | focus 변동 | 0035 §3.1 |
| `0x85` | **TERMINAL_DIED** | server→client | server-wide | pty SIGCHLD / explicit kill | 0034 §3 |
| `0x86` | MOUNT_CASCADE | (BE 미발행) | — | reserved by FE decoder | 0038 next-3 |
| `0x87` | **TERMINAL_LIST_UPDATE** | server→client | session-filtered (trigger 제외) | attach_confirm spawn batch 후 | 0036(BE) §2 |
| `0x88` | **TERMINAL_SPAWNED** | server→client | server-wide | spawn_terminal_with_uuid register 성공 직후 | 본 batch (`d00db66`) |

본 가이드의 핵심 신규 = **0x85, 0x87, 0x88**.

### 1.2 wire shape (binary 정확 형식)

모든 envelope 의 outer codec: `[1B type][4B LE u32 length][payload(length)]`.

**0x85 TERMINAL_DIED** — inner:
```
varint 0 + UTF-8 JSON {
  "terminal_id": "<uuid-v4>",
  "reason": "exit" | "killed"
}
```
- `reason="killed"` ⇔ pty 가 signal 로 종료 (kernel SIGKILL / 명시 SIGTERM)
- `reason="exit"` ⇔ self-exit (shell 의 `exit`, child 의 normal return)

**0x87 TERMINAL_LIST_UPDATE** — inner:
```
varint 0 + UTF-8 JSON {
  "added":   ["<uuid>", ...],   // 빈 배열 허용
  "removed": ["<uuid>", ...]    // 빈 배열 허용
}
```
- 둘 다 *항상* 배열. P1 path 는 항상 `removed: []` (attach_confirm 의 spawn 만 emit).

**0x88 TERMINAL_SPAWNED** ★ NEW — inner:
```
varint 0 + UTF-8 JSON {
  "terminal_id": "<uuid-v4>",
  "pane_id":     <u64-as-JSON-number>
}
```
- `pane_id` 는 JS `Number`. PaneId 가 `1` 부터 증가하므로 안전 정수 범위 (2⁵³-1) 보장됨. FE decoder 가 `Number.isSafeInteger` guard 권장 (단 미래의 long-running server 에서 overflow 0 가능성 대비).

### 1.3 HTTP endpoint 표 (Stage 5 신규 / 변경)

| route | 메서드 | 응답 | 의도 |
|---|---|---|---|
| `/api/sessions` | GET | `[{name, active}, ...]` | 목록 + flock peek |
| `/api/sessions` | POST | `{name}` | 신규 session |
| `/api/sessions/:name` | DELETE | 204 | session 레코드 삭제 |
| `/api/sessions/:name/layout` | GET | `LayoutSnapshot` + ETag | v2 layout |
| `/api/sessions/:name/layout` | PUT | ETag (CAS) | layout 갱신 |
| `/api/sessions/:name/attach` | POST | `{name, attached, server_id, matched, unmatched}` | flock + 분류 |
| `/api/sessions/:name/attach` | DELETE | `{name, released}` | flock 해제 |
| `/api/sessions/:name/attach/confirm` | POST | `{name, spawned, already_present, failed}` | unmatched spawn |
| `/api/sessions/:name/items/:id` | DELETE | 204 + ETag | `?kill_terminal=true` 시 terminal 도 종료 |
| `/api/terminals` | GET | `[{id, alive, label, created_at, attach_count, attached_sessions}]` | pool + meta + cross-ref |
| `/api/terminals/:id` | PATCH | 204 | `{label}` 갱신 (4096 cap) |
| `/api/terminals/:id/kill` | POST | 204 | explicit terminate |
| `/api/terminals/:id/respawn` | POST | `{id}` | same UUID, fresh PaneId |

### 1.4 인증 lifecycle (D10 α 후)

- `POST /auth/login` → `Set-Cookie: gtmux_auth=...; HttpOnly; SameSite=Strict; ...` — 기존
- WS handshake — **cookie 또는 bearer.<token> subprotocol 둘 중 하나만 valid 해도 accept** (additive). FE 가 cookie + subprotocol 둘 다 송신해도 무손
- `/auth/login` 후 cookie 만 있으면 WS handshake 통과 — FE 의 password-only 사용자 의 streaming 진입 unblock
- WS handshake invariant:
  - `Sec-WebSocket-Protocol: gtmux.v1` 필수
  - bearer 은 optional — `gtmux.v1, bearer.<token>` 또는 `gtmux.v1` 모두 OK
  - cookie 헤더 `gtmux_auth=...` 도 optional — *둘 중 하나만* valid 면 통과

---

## 2. FE 작업 매트릭스 (per-feature)

### 2.1 0x85 TERMINAL_DIED — ✅ FE 작업트리 이미 구현

검증:
- `lib/ws/decode.ts:503` `decodeTerminalDied` — payload schema 정확 일치
- `lib/ws/dispatcher.svelte.ts:438` `handleTerminalDied` — `danglingTerminals.mark(terminalId, reason)` + `terminalPool.refresh()`

**조치 의견**: 그대로 둠. 변경 X.

### 2.2 0x87 TERMINAL_LIST_UPDATE — ✅ FE 작업트리 이미 구현

검증:
- `lib/ws/decode.ts:539` `decodeTerminalListUpdate` — `parseStringArray(added/removed)` 정합
- `lib/ws/dispatcher.svelte.ts:511` `handleTerminalListUpdate` — `terminalPool.refresh()` 만

라우팅:
- BE 가 trigger session 제외 fan-out 하므로 FE 는 자기 active session 의 0x87 받지 않음 (정상)
- 다른 session 의 spawn 이 일어나면 sidebar Terminal list 가 즉시 refresh

**조치 의견**: 그대로 둠.

### 2.3 0x88 TERMINAL_SPAWNED — ❌ FE 신규 작업 필요

#### 2.3.1 decoder 추가

`lib/ws/decode.ts` 에 0x88 추가:

```ts
// FRAME_TYPE 표 갱신
TERMINAL_SPAWNED: 0x88,

// 새 payload 인터페이스
export interface TerminalSpawnedPayload {
  readonly terminalId: string;
  readonly paneId: number;
}

// 새 decoder
export function decodeTerminalSpawned(payload: Uint8Array): TerminalSpawnedPayload | null {
  const obj = decodeVarintZeroJsonObject(payload);
  if (!obj) return null;
  const terminalId = obj['terminal_id'];
  const paneId = obj['pane_id'];
  if (typeof terminalId !== 'string' || terminalId.length === 0) return null;
  if (typeof paneId !== 'number' || !Number.isSafeInteger(paneId) || paneId <= 0) return null;
  return { terminalId, paneId };
}
```

#### 2.3.2 dispatcher handler 추가

`lib/ws/dispatcher.svelte.ts`:

```ts
// FRAME_TYPE.TERMINAL_SPAWNED arm 추가
case FRAME_TYPE.TERMINAL_SPAWNED:
  return handleTerminalSpawned(env.payload);

function handleTerminalSpawned(payload: Uint8Array): void {
  const decoded = decodeTerminalSpawned(payload);
  if (!decoded) {
    console.warn('[ws] 0x88 TERMINAL_SPAWNED decode failed');
    return;
  }
  // 핵심: terminalPool 의 UUID → PaneId 매핑 갱신.
  // 본 frame 이 land 하면 XtermHost 가 terminal 모드로 즉시 마운트 가능.
  terminalPool.bindPaneId(decoded.terminalId, decoded.paneId);
}
```

#### 2.3.3 terminalPool 의 매핑 store

```ts
// lib/stores/terminalPool.svelte.ts
class TerminalPoolStore {
  // 기존 list (UUID → row) 외에 PaneId 매핑 store 추가
  private paneIdByUuid = $state(new SvelteMap<string, number>());

  bindPaneId(uuid: string, paneId: number) {
    this.paneIdByUuid.set(uuid, paneId);
  }

  paneIdFor(uuid: string): number | undefined {
    return this.paneIdByUuid.get(uuid);
  }

  // refresh() 안에서도 GET /api/terminals 응답으로 paneIdByUuid 보강 가능 (단,
  // /api/terminals 응답은 PaneId 미포함 — 본 매핑은 0x88 frame 만 source of truth.
  // 또는 BE 가 /api/terminals 응답에 pane_id 추가하도록 amend 요청 가능 — §5.4 참조).
}
```

### 2.4 D10 α — ✅ FE 작업 무변동

FE 는 이미 cookie + subprotocol 둘 다 송신. BE 가 양쪽 허용으로 변경 — FE side 무변동.

password-mode 사용자가 WS 진입 가능해짐.

### 2.5 5-D P1 attach_confirm — ✅ FE 이미 정합

FE 의 `attachSession(name)` flow:
1. `POST /attach` → matched/unmatched 분류
2. unmatched 있으면 `POST /attach/confirm` 호출
3. 응답의 `spawned` 로 진행 (FE 의 작업트리에 이미 구현)

attach_confirm 후:
- *내 webpage* 의 layout 은 그대로 (UUID 가 이미 있음). `terminalPool.refresh()` 만 호출.
- *다른 webpage* 는 0x87 받음 → 자동으로 `terminalPool.refresh()`.
- *모든 webpage* 는 spawn 한 UUID 마다 0x88 받음 → `bindPaneId` 즉시 호출.

---

## 3. Issue C (multi-session xterm) 마이그레이션

FE 의 0036 §4 Issue C — multi-session terminal panel 의 xterm 연결 불가. 현재 FE 가 Option C2 (placeholder) 로 막아둠. 본 batch 의 0x88 frame 이 Option C1 (실 streaming) 전환을 unblock.

### 3.1 현재 상태 (Option C2)

`PanelNode.svelte:386-401`:
```svelte
{#if isLegacyPane && typeof data.pane_id === 'string'}
  <XtermHost paneId={data.pane_id.replace(/^%/, '')} />
{:else if useSessionStore}
  <!-- placeholder: "Terminal stream pending" -->
{/if}
```

`XtermHost` 는 numeric `paneId` 만 받음. multi-session item 의 `data.pane_id` 는 UUID → `Number.parseInt(UUID, 10) = NaN`.

### 3.2 Option C1 으로 전환 — 단계별

#### Step 1: `XtermHost` 의 dual mode

`XtermHost.svelte` props 확장:

```ts
type XtermHostProps =
  | { mode: 'legacy'; paneId: string }
  | { mode: 'terminal'; terminalId: string };

let { mode, paneId, terminalId }: XtermHostProps = $props();

// terminal 모드: terminalPool 의 paneIdFor(terminalId) 로 numeric PaneId 획득
const paneIdNum = $derived.by(() => {
  if (mode === 'legacy') return Number.parseInt(paneId, 10);
  const mapped = terminalPool.paneIdFor(terminalId);
  return mapped ?? null;
});

// paneIdNum 이 null 이면 아직 binding 도착 전 — 명시 pending UI 표시
const ready = $derived(typeof paneIdNum === 'number' && Number.isInteger(paneIdNum));
```

이후 기존 numeric PaneId 기반 input/resize/subscribe 흐름 그대로 동작.

#### Step 2: `PanelNode` 의 mount 분기

```svelte
{#if isLegacyPane && typeof data.pane_id === 'string'}
  <XtermHost mode="legacy" paneId={data.pane_id.replace(/^%/, '')} />
{:else if useSessionStore && data.type === 'terminal'}
  <XtermHost mode="terminal" terminalId={data.id} />
{/if}
```

`XtermHost` 내부에서 binding 미도착 시 spinner / "Terminal stream connecting" 표시.

#### Step 3: 0x88 binding handler

§2.3 의 `handleTerminalSpawned` 가 `terminalPool.bindPaneId` 호출. `XtermHost` 가 reactive 하게 `paneIdNum` 갱신 → 자동 mount.

#### Step 4: cross-session leak 임시 가드 (next-2 land 전)

현재 BE 의 `pane_output` 은 **server-wide broadcast** — session 무관하게 모든 WS 가 모든 pane bytes 받음. session-scoped streaming (0038 next-2) 가 land 하기 전에는:

- *내 layout 의 UUID 가 아닌* PaneId 의 PANE_OUT 은 FE 에서 drop
- 구체: `XtermHost` 가 자기 PaneId 로 `paneOutSubscriber` 등록 (현재 흐름 그대로) — 다른 session 의 pane 도 같은 broadcast 로 오므로 *XtermHost 가 받지만 그건 자기 PaneId 의 데이터* → 자동 격리 (PaneId 매칭 안 하면 drop)
- 따라서 *XtermHost 의 PaneId 기반 subscriber 가 본질적으로 가드 역할* — 명시 추가 가드 불필요

단, *terminalPool.list* 가 cross-session 의 UUID 도 모두 노출하므로 sidebar 의 *attach context* 는 명시 필터 필요:
```ts
const visibleTerminals = $derived(
  terminalPool.list.filter(t =>
    sessionStore.active === null || t.attached_sessions.includes(sessionStore.active.name)
  )
);
```

(이는 BE 의 `/api/terminals` 응답의 `attached_sessions` 필드 기반. 이미 BE 가 제공.)

### 3.3 검증 시나리오 (Option C1 완료 후)

1. 2 browser tab — tab A 가 session α 에 attach, tab B 가 session β 에 attach
2. tab A 의 PUT layout 에 terminal UUID `u1` 추가 → `POST /attach/confirm` → spawn
3. tab A 의 panel `u1` 에 0x88 frame 도착 → `XtermHost` 가 PaneId 매핑 후 mount
4. terminal output 이 tab A 의 xterm 에 표시됨
5. tab B 의 sidebar Terminal list 가 0x87 로 refresh → `u1` 이 보임 (단 tab B 의 layout 에는 안 보임)
6. tab B 의 layout 에 `u1` 추가하면 mirror 표시 (FE-NEW-6 의 multi-xterm subscriber 필요)
7. tab A 가 `u1` kill → 0x85 도착 → `danglingTerminals.mark(u1, "killed")` → tab A/B 둘 다 dangling overlay

---

## 4. 주요 user flow 시퀀스

### 4.1 cookie login → workspace → attach → xterm

```
[FE]                                [BE]
  |                                   |
  |--- GET /?t=<token> --------------->|
  |<-- HTML + sessionStorage          |
  |    (FE 의 0036 Issue A 수정 후:    |
  |     /?t= 발견 시 즉시 login)       |
  |                                   |
  |--- POST /auth/login {token} ------>|
  |<-- 200 + Set-Cookie gtmux_auth     |
  |                                   |
  |--- GET /api/sessions ------------->|
  |<-- [{name, active}, ...]          |
  |                                   |
  |--- WS upgrade /ws ---------------->|
  |    Cookie: gtmux_auth=...         |
  |    Sec-WebSocket-Protocol: gtmux.v1, bearer.<token>
  |<-- 101 Switching (D10 α: 둘 다 통과) |
  |<-- 0x80 LAYOUT_CHANGED hello       |
  |                                   |
  |--- POST /api/sessions/demo/attach >|
  |<-- {matched: [...], unmatched: [...]} |
  |                                   |
  |--- POST /attach/confirm ---------->|
  |<-- {spawned: [u1, u2]}            |
  |    (BE: each spawn → publish_terminal_spawned) |
  |<-- 0x88 {terminal_id: u1, pane_id: 1} |
  |<-- 0x88 {terminal_id: u2, pane_id: 2} |
  |                                   |
  |    [FE] terminalPool.bindPaneId   |
  |    [FE] XtermHost mounts          |
  |<-- 0x07 NOTIFY pane-spawned (1)   |
  |<-- 0x07 NOTIFY pane-spawned (2)   |
  |<-- 0x02 PANE_OUT (1, bytes)       |
  |<-- 0x02 PANE_OUT (2, bytes)       |
  |                                   |
  |--- 0x03 PANE_IN (1, "ls\n") ----->|
  |<-- 0x02 PANE_OUT (1, ls output)   |
```

### 4.2 다른 webpage 가 spawn 한 terminal (session β 의 입장)

```
[FE-A α]      [BE]      [FE-B β]
   |            |           |
   |            |<-- POST /attach/confirm (session α)
   |            |    spawn u3 (PaneId 3)
   |            |
   |<- 0x88 u3 -|-- 0x88 u3 ->|
   |<- 0x87 -- (skipped, A=trigger) -|
   |            |--- 0x87 added:[u3] ->|
   |            |           |
   | (A 의 layout 에 u3 이미 있음)  | (B: terminalPool.refresh)
   |   XtermHost u3 mount         | (B: sidebar 갱신)
   |   PANE_OUT 받기 시작            |
```

### 4.3 terminal kill (kernel SIGCHLD)

```
[OS]             [BE]              [FE-A]   [FE-B]
  |               |                  |        |
  |-- SIGCHLD -->|                  |        |
  |               | reaper:          |        |
  |               | BackendNotify::PaneDied   |
  |               |                  |        |
  |               | handle_pane_died: |        |
  |               | terminal_map.unregister_pane → uuid |
  |               | hub.publish_terminal_died(uuid, "exit") |
  |               |                  |        |
  |               |--- 0x85 u3 ----->|        |
  |               |--- 0x85 u3 -----------------|
  |               |                  |        |
  |               | (terminal_meta.forget X — 보존)|
  |               |                  |        |
  |     [FE] danglingTerminals.mark(u3, "exit") + terminalPool.refresh() |
```

---

## 5. 다음 BE batch 의 FE 정합 항목

### 5.1 next-1: 5-C echo-minus-sender — ✅ BE ship 완료

본 batch (`5-C`) BE 에 land. wire 결정 + FE 측 amend 사양 확정.

#### 5.1.1 wire — binary varint + tail trailer

0x81~0x84 의 outbound payload 는 *기존 inbound payload* 의 끝에 다음 trailer 가 *항상* 붙음:

```
trailer = varint(session_id_len) + UTF-8 session_id_bytes
```

예 (session_id = "alpha", 5 bytes):
```
... [original body 0x83 = varint 0 + int32 x + int32 y + float32 zoom] ...
... 0x05 'a' 'l' 'p' 'h' 'a'                                         <- trailer
```

session_id 가 부재 (이론적으로 발생 X — BE 는 sender 의 session_for_cookie 가 *없으면* publish 자체를 안 함) 면 trailer 도 부재 (현재 BE 가 항상 publish 시 채움).

inbound (FE→BE) 는 trailer 없이 송신해도 OK — BE 가 받는 inbound 는 session_for_cookie lookup 으로 채움. trailer 있게 송신해도 무관 (BE 는 enrich 단계에서 자기 cookie 의 session 으로 덮어씀).

#### 5.1.2 FE 측 amend 사양

`lib/ws/decode.ts` 의 4 decoder 갱신:
- 기존 `if (cursor !== payload.length) return null` strict check 를 *완화* — 잔여 바이트가 있으면 trailer 로 파싱
- 반환 인터페이스에 `sessionId: string | null` 추가

```ts
// 예: decodeViewport
export interface ViewportChangedPayload {
  readonly x: number;
  readonly y: number;
  readonly zoom: number;
  readonly sessionId: string | null;  // 신규
}

export function decodeViewport(payload: Uint8Array): ViewportChangedPayload | null {
  const view = makeView(payload);
  const head = readVarintU(view, 0);
  if (!head || head.value !== 0) return null;
  const off = head.next;
  if (payload.length < off + 12) return null;
  const x = view.getInt32(off, true);
  const y = view.getInt32(off + 4, true);
  const zoom = view.getFloat32(off + 8, true);
  const trailerStart = off + 12;
  let sessionId: string | null = null;
  if (trailerStart < payload.length) {
    const lenRead = readVarintU(view, trailerStart);
    if (!lenRead) return null;
    const strStart = lenRead.next;
    const strEnd = strStart + lenRead.value;
    if (strEnd !== payload.length) return null;
    sessionId = new TextDecoder('utf-8').decode(payload.subarray(strStart, strEnd));
  }
  return { x, y, zoom, sessionId };
}
```

같은 패턴으로 `decodeMChanged` / `decodeIChanged` / `decodeFocusMode` 갱신.

`lib/ws/dispatcher.svelte.ts` 의 각 handler 첫 줄:
```ts
function handleViewportChanged(payload: Uint8Array): void {
  const decoded = decodeViewport(payload);
  if (!decoded) { /* warn */ return; }
  if (!isFrameForActiveSession(decoded.sessionId)) return;  // ← 본 줄 추가
  ephemeralStore.viewport = { x: decoded.x, y: decoded.y, zoom: decoded.zoom };
}
```

#### 5.1.3 outbound 송신 (FE→BE)

FE 가 user gesture (selection 변경, viewport drag, focus toggle) 시 0x81~0x84 송신 시:
- 기존 encoder 그대로 사용 (binary varint, trailer 없음)
- BE 가 받아서 cookie 의 session 으로 enrich → publish → 같은 session 의 다른 webpage 가 frame 받음

BE 는 *연결의 cookie 가 session 에 attach 되지 않은 상태* 의 송신을 silently drop — FE 가 attach 전에 송신해도 무손.

#### 5.1.4 connection_id (BE 내부)

연결마다 monotonic counter `conn-N` 형식. FE 는 connection_id 를 알 필요 X — BE 가 자기 연결의 sender_conn_id 비교로 echo skip.

### 5.2 next-2: session-scoped pane_output (FE-NEW-6 와 동시)

FE 측 핵심 변경:
- `XtermHost` 의 multi-instance — 같은 terminal UUID 가 여러 panel 에 mirror 될 때 broadcast subscriber 패턴 (ADR-0021 D1)
- `paneOut` store 의 key naming — legacy PaneId 와 UUID 분리

BE 가 *session 의 terminal UUID set 외의 PaneId 의 bytes* 를 drop 하면 FE 의 임시 가드 (§3.2 step 4) 도 제거 가능.

### 5.3 next-3: 5-D P2 — ✅ BE ship 완료

본 batch (`5-D P2`) BE 에 land. wire + endpoint 확정.

#### 5.3.1 endpoint

`POST /api/sessions/:name/terminals`
- 인증: cookie 또는 bearer (D10 α)
- cookie 가 session `:name` 의 attach 보유 필요 (403 `not_attached` otherwise — `attach_confirm` 과 같은 정책)
- body: `{}` (MVP — 미래에 `{label, x, y, w, h}` 오버라이드 가능). 본 batch 는 BE 가 default coords 결정
- 응답: 200 `{ terminal_id, pane_id, x, y, w, h }`

default coord 정책:
- 빈 layout (terminal item 없음) → `(80, 80, 720, 420)`
- 기존 terminal items 존재 → `max(item.x)` / `max(item.y)` + `32` cascade, w/h = `720 × 420`

#### 5.3.2 wire frame 0x86 MOUNT_CASCADE

```
inner = varint 0 + UTF-8 JSON {
  "terminal_id": "<uuid-v4>",
  "x":  <number>,
  "y":  <number>,
  "w":  <number>,
  "h":  <number>
}
```

라우팅: trigger session 의 webpage **만** (`hub.session_for_cookie(my) == trigger_session` 인 subscriber 만 받음).

다른 session 의 webpage 는 같은 spawn 으로부터 0x87 TERMINAL_LIST_UPDATE 도 함께 받음 — pool refresh 용.

#### 5.3.3 frame 발행 순서 (per spawn)

1. `0x88 TERMINAL_SPAWNED { terminal_id, pane_id }` — server-wide (FE 의 PaneId 매핑 갱신)
2. `0x86 MOUNT_CASCADE { terminal_id, x, y, w, h }` — trigger session only (FE 가 mutateLayout)
3. `0x87 TERMINAL_LIST_UPDATE { added: [terminal_id], removed: [] }` — other sessions only (sidebar refresh)

#### 5.3.4 FE 측 작업

- `NewPanelButton.svelte` 의 multi-session branch — `POST /api/sessions/:name/terminals` 호출 (현재는 legacy WS CTRL). body `{}` 송신
- `lib/ws/decode.ts` 의 `decodeMountCascade` ✅ 이미 작업트리에 있음
- `lib/ws/dispatcher.svelte.ts` 의 `handleMountCascade` ✅ 이미 작업트리 — `mutateLayout(name, cur => append TerminalItem at server-supplied x/y/w/h)` 호출

#### 5.3.5 persistence 모델

BE 는 layout 에 직접 item 을 persist 하지 **않음**. FE 의 `handleMountCascade` 가 `mutateLayout` → `PUT /api/sessions/:name/layout` 으로 round-trip. 0037 §6.4 의 결정 그대로.

race window: 사용자가 0.1s 이내 연속 클릭 시 두 번의 `POST /terminals` 가 같은 default coords 를 받을 수 있음 — 같은 위치에 두 terminal 이 stack. user 가 한 개 이동하면 해결. correctness 문제 X.

### 5.4 (optional) BE 의 `/api/terminals` 응답에 `pane_id` 추가 요청 권한

현재 응답:
```json
[{ "id": "<uuid>", "alive": true, "label": "...", "created_at": ..., "attach_count": 1, "attached_sessions": [...] }]
```

만일 FE 가 reconnect 후 *0x88 frame 을 받기 전에* 기존 terminal 의 PaneId 가 필요하면, BE 가 응답에 `pane_id: <u64>` 추가하는 게 자연스러움. FE 가 필요하다고 판단 시 BE 에 요청 — 5 분 작업.

---

## 6. 빠른 sanity 명령

### 6.1 BE 측 (release binary)

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo build --release --bin gtmux

env -u TMUX ./target/release/gtmux start --session demo --port 9999 --workspace /tmp/ws-demo
```

### 6.2 FE 측 wscat 으로 0x88 frame 검증

```bash
TOKEN=$(cat ~/.local/state/gtmux/demo.token)

# cookie login
RAW=$(curl -sS -i \
  -H "Host: 127.0.0.1:9999" -H "Origin: http://127.0.0.1:9999" \
  -X POST http://127.0.0.1:9999/auth/login \
  -d "{\"token\":\"$TOKEN\"}" -H "Content-Type: application/json")
COOKIE=$(echo "$RAW" | awk -F': ' '/^set-cookie: gtmux_auth=/ { sub(/;.*/,"",$2); print $2 }' | head -1)

# create session + PUT layout with terminal UUID
curl -sS -H "Authorization: Bearer $TOKEN" -X POST http://127.0.0.1:9999/api/sessions -H "Content-Type: application/json" -d '{"name":"demo"}'
# ... PUT layout (생략) ...

# WS connect (cookie 만)
wscat \
  --header "Cookie: $COOKIE" \
  --subprotocol "gtmux.v1" \
  -c "ws://127.0.0.1:9999/ws"
# → 첫 frame: 0x80 LAYOUT_CHANGED hello
# attach + confirm 후:
# → 0x88 {"terminal_id":"...","pane_id":1}
# → 0x07 NOTIFY pane-spawned
# → 0x02 PANE_OUT (shell prompt)
```

### 6.3 cargo test 매트릭스

```bash
cargo test --workspace --color=never
# 303 PASS / 0 FAIL (commit d00db66)
#   ws-server: 93  (5-A 6 + 5-B 5 + 5-D P1 8 + D10 α 5 + 0x88 + 0x87 routing 5 외)
#   http-api: 142  (Stage 4 + 5-A 3 + 5-B 3 + 5-D P1 2 + D10 α 2 + 0x88 2)
```

---

## 7. 변경 이력

- 2026-05-15: 초안 — BE HEAD `d00db66` 시점 의 wire surface + FE 정합 항목 정리. Issue C 의 Option C1 마이그레이션 path 명시.
