# 0036 — Frontend Review Action Items

- 일자: 2026-05-15
- 작성자: Codex review
- 대상: frontend 개발 agent
- 종류: 리뷰 후속 작업 문서
- 기준 상태: multi-session pivot 진행 중인 작업트리
- 관련 문서:
  - `docs/reports/0033-fe-stage-1-to-5-partial-progress.md`
  - `docs/reports/0035-be-fe-coordination-stage-5.md`
  - `docs/agents/frontend-handover-v3.md`
  - `docs/plans/0007-multi-session-pivot.md`

---

## 0. 목적

본 문서는 현재 frontend 작업트리 리뷰에서 발견된 데모 차단/사용성 저하 항목을 frontend 개발 agent 에게 전달하기 위한 실행 문서다.

리뷰 범위는 실제 사용자 데모 가능성을 기준으로 한다. 즉, "코드가 존재한다"보다 "사용자가 인증 후 session 을 열고 canvas 에서 terminal panel 을 보고 조작할 수 있는가"를 우선한다.

우선 해결해야 할 핵심은 다음 3개다.

1. `/?t=<token>` 진입이 인증 쿠키를 만들지 못해 `/auth` 로 되돌아가는 문제
2. multi-session terminal panel 이 닫히지 않는 문제
3. multi-session terminal panel 이 `XtermHost` 와 연결되지 않아 terminal body 가 blank/non-interactive 가 되는 문제

4번째 항목인 legacy `pane-spawned` auto-mount 충돌은 위 3개보다 낮은 우선순위지만, Stage 5 WS/session routing 과 같이 정리해야 한다.

---

## 1. 우선순위 요약

| 우선순위 | 항목 | 사용자 영향 | 권장 처리 |
|---|---|---|---|
| P0 | `/?t=<token>` 인증 쿠키 교환 누락 | magic link / token link 로 앱 진입 불가 | 즉시 수정 |
| P0 | v2 terminal panel close disabled | 사용자가 terminal panel 을 제거/kill 할 수 없음 | 즉시 수정 |
| P0 | v2 terminal panel xterm 연결 불가 | terminal panel 이 실제 terminal 로 동작하지 않음 | BE/FE 상태에 따라 즉시 수정 또는 명시적 pending UI |
| P1 | legacy `pane-spawned` auto-mount 충돌 | multi-session layout 에 legacy panel 이 섞일 수 있음 | Stage 5 WS routing 작업과 함께 정리 |

---

## 2. 이슈 A — `/?t=<token>` 진입 인증 쿠키 교환 누락

### 2.1 증상

사용자가 다음 형태로 앱에 접근한다.

```text
http://127.0.0.1:9999/?t=<token>
```

현재 frontend 는 URL 의 `t` 값을 `sessionStorage.gtmux_token` 에 저장하고 URL 에서 제거한다. 그러나 이후 auth gate 는 cookie 기반 `GET /api/sessions` 만 호출한다. 사용자가 아직 `/auth/login` 을 통과하지 않았으면 cookie 가 없으므로 401 이 발생하고 `/auth` 로 redirect 된다.

이 시점에는 이미 URL query 에서 token 이 제거되어, `/auth` 쪽이 token 을 다시 사용할 수 없다.

### 2.2 관련 코드

| 파일 | 위치 | 내용 |
|---|---:|---|
| `codebase/frontend/src/routes/+page.svelte` | `captureTokenFromUrl()` | `?t=` 를 `sessionStorage` 에 저장하고 URL clean |
| `codebase/frontend/src/routes/+page.svelte` | `bootstrap()` auth gate | `GET /api/sessions` 를 cookie-only 로 호출 |
| `codebase/frontend/src/lib/http/auth.ts` | `login()` | token/password login helper 존재 |
| `codebase/frontend/src/routes/auth/+page.svelte` | auth preview | token tab / password tab / `?t=` 처리 흐름 존재 |

### 2.3 원인

현재 root page 의 token 처리와 cookie auth lifecycle 이 분리되어 있다.

```text
?t=token
  -> sessionStorage 저장
  -> URL clean
  -> /api/sessions cookie auth
  -> cookie 없음
  -> /auth redirect
```

`sessionStorage.gtmux_token` 은 legacy bearer WebSocket 을 위한 값으로만 쓰이고, HTTP API 인증 쿠키 발급에는 쓰이지 않는다.

### 2.4 수정 방향

권장안은 root page 에서 `?t=` 를 발견하면 즉시 cookie login 교환을 수행하는 것이다.

```text
?t=token
  -> POST /auth/login { token, redirect: false }
  -> Set-Cookie 수신
  -> URL clean
  -> GET /api/sessions
  -> workspace 진입
```

구현 지침:

- `captureTokenFromUrl()` 은 token 저장만 하지 말고, bootstrap 단계에서 `login({ token, redirect: false })` 를 호출할 수 있도록 token 값을 반환해야 한다.
- URL clean 은 login 성공 후 수행하는 편이 안전하다. 실패 시 사용자가 auth page 로 이동해도 token 을 복구할 수 있어야 한다.
- token 을 계속 query string 에 노출하는 것이 우려되면, 실패 케이스에서도 `/auth/bootstrap?token=...` 또는 `/auth-preview?t=...` 로 명시 전달하는 대안을 선택한다.
- root page 에서 auth helper 를 호출할 경우 `credentials: 'include'` 가 유지되어야 한다.
- legacy bearer WS 가 완전히 제거되기 전까지는 `sessionStorage.gtmux_token` 저장을 유지해도 된다. 단, HTTP cookie login 과 별개로 생각하면 안 된다.

### 2.5 수용 기준

- 새 브라우저 세션에서 cookie 가 없는 상태로 `/?t=<valid-token>` 접근 시 `/auth` 로 튕기지 않고 workspace switcher 또는 active session 으로 진입한다.
- `GET /api/sessions` 가 200 으로 통과한다.
- invalid token 접근 시 URL token 이 무조건 유실되어 사용자가 복구 불가 상태가 되지 않는다.
- password-only 모드와 token-only 모드 모두 기존 auth page 동작이 깨지지 않는다.

### 2.6 검증 시나리오

1. 브라우저 storage/cookie 를 비운다.
2. `/?t=<valid-token>` 로 접근한다.
3. 네트워크에서 `POST /auth/login` 후 `GET /api/sessions` 순서가 보이는지 확인한다.
4. `/auth` 로 redirect 되지 않는지 확인한다.
5. invalid token 으로 접근해 실패 UI/redirect 가 복구 가능한지 확인한다.

---

## 3. 이슈 B — multi-session terminal panel close disabled

### 3.1 증상

multi-session layout 에서 terminal item 이 canvas node 로 표시되지만 panel close 버튼이 비활성화되거나, 클릭해도 close flow 로 진입하지 못한다.

사용자는 terminal panel 을 canvas 에서 제거하거나 terminal 을 kill 하는 close dialog 로 접근할 수 없다.

### 3.2 관련 코드

| 파일 | 위치 | 내용 |
|---|---:|---|
| `codebase/frontend/src/lib/canvas/Canvas.svelte` | `itemToNode()` | v2 terminal item 의 UUID 를 `data.pane_id` 에 넣음 |
| `codebase/frontend/src/lib/canvas/PanelNode.svelte` | `paneNumeric` | `%N` 형식만 numeric pane id 로 인정 |
| `codebase/frontend/src/lib/canvas/PanelNode.svelte` | `closeDisabled` | `paneNumeric === null` 이면 close disabled |
| `codebase/frontend/src/lib/canvas/PanelNode.svelte` | `onClose()` | disabled 이면 multi-session branch 전에 return |
| `codebase/frontend/src/lib/chrome/PanelCloseConfirmModal.svelte` | 신규 파일 | v2 close UX 로 보이나 현재 진입 불가 |
| `codebase/frontend/src/lib/http/sessions.ts` | `deleteItem()` | v2 delete API helper 존재 |

### 3.3 원인

legacy single-session 시대의 close 조건이 multi-session terminal item 에 그대로 적용되고 있다.

legacy panel:

```text
pane_id = "%1" 같은 tmux/pty numeric id
close 조건 = liveCount > 1 && paneNumeric !== null
close command = legacy pane close
```

multi-session terminal item:

```text
item.id = terminal UUID
pane_id 슬롯에도 UUID 를 넣음
close 조건 = paneNumeric === null
결과 = 항상 disabled
```

즉, multi-session 에서는 UUID 가 정상 identifier 인데 legacy numeric pane 조건 때문에 close 가 차단된다.

### 3.4 수정 방향

`PanelNode` 의 close 가능 여부와 close handler 를 legacy/v2 로 분리해야 한다.

권장 구조:

```ts
const isSessionItem = Boolean(data.session_item_id 또는 sessionStore.active)
const canCloseLegacyPane = !isSessionItem && liveCount > 1 && paneNumeric !== null
const canCloseSessionItem = isSessionItem && Boolean(data.item_id 또는 data.pane_id)
```

주의점:

- v2 terminal close 는 `liveCount` 에 의존하면 안 된다.
- v2 terminal close 는 `paneNumeric` 에 의존하면 안 된다.
- v2 close 버튼은 `PanelCloseConfirmModal` 로 진입해야 한다.
- modal 의 action 은 최소 2개를 구분해야 한다.
  - canvas item 만 제거: `DELETE /api/sessions/:name/items/:id?kill_terminal=false`
  - terminal 도 kill: `DELETE /api/sessions/:name/items/:id?kill_terminal=true`
- terminal mirror 가 여러 panel 에 연결될 수 있으므로, "remove panel" 과 "kill terminal" 의 의미를 UI 에서 분명히 분리해야 한다.

### 3.5 수용 기준

- multi-session terminal panel 에서 close 버튼이 활성화된다.
- close 버튼 클릭 시 `PanelCloseConfirmModal` 이 열린다.
- "panel 만 제거" 선택 시 layout item 이 사라지고 terminal pool 은 유지된다.
- "terminal 도 종료" 선택 시 item 제거와 terminal kill 이 함께 수행되며, terminal pool/dangling state 가 갱신된다.
- legacy single-session panel close 동작은 기존과 동일하게 유지된다.

### 3.6 검증 시나리오

1. session 을 attach 하고 terminal item 이 있는 layout 을 연다.
2. terminal panel close 버튼을 클릭한다.
3. close confirm modal 이 열리는지 확인한다.
4. `kill_terminal=false` 경로로 item 제거 후 새로고침해도 item 이 복구되지 않는지 확인한다.
5. `kill_terminal=true` 경로에서 terminal pool/dangling 표시가 의도대로 갱신되는지 확인한다.

---

## 4. 이슈 C — multi-session terminal panel xterm 연결 불가

### 4.1 증상

multi-session layout 의 terminal item 이 canvas 에 표시되지만 terminal body 가 실제 terminal 로 동작하지 않는다.

예상되는 사용자 증상:

- xterm 영역이 비어 있거나 과거 output 을 받지 못한다.
- 키 입력이 backend 로 전송되지 않는다.
- panel resize 시 terminal resize 가 backend 로 전달되지 않는다.

### 4.2 관련 코드

| 파일 | 위치 | 내용 |
|---|---:|---|
| `codebase/frontend/src/lib/canvas/PanelNode.svelte` | terminal body render | `XtermHost paneId={data.pane_id.replace(/^%/, '')}` |
| `codebase/frontend/src/lib/canvas/Canvas.svelte` | `itemToNode()` | v2 UUID 를 `data.pane_id` 에 넣음 |
| `codebase/frontend/src/lib/canvas/XtermHost.svelte` | `parsedPaneId` | `Number.parseInt(paneId, 10)` 으로 numeric id 만 valid |
| `codebase/frontend/src/lib/canvas/XtermHost.svelte` | pane output subscribe | raw `paneId` key 로 `paneOut` subscriber 등록 |
| `codebase/frontend/src/lib/canvas/XtermHost.svelte` | input/resize send | numeric valid 조건일 때만 `PANE_IN`, resize 송신 |
| `codebase/frontend/src/lib/ws/dispatcher.svelte.ts` | `paneOut` 처리 | 현재 legacy pane id 기반 stream 처리 |

### 4.3 원인

현재 `XtermHost` 는 legacy numeric pane id 모델에 묶여 있다. 그러나 multi-session terminal item 의 canonical id 는 terminal UUID 다.

```text
PanelNode
  data.pane_id = "<terminal-uuid>"
  -> XtermHost paneId="<terminal-uuid>"

XtermHost
  Number.parseInt("<terminal-uuid>", 10) -> NaN 또는 invalid
  -> canSendPaneInput = false
  -> resize 송신 안 함
  -> paneOut subscriber key 도 backend stream key 와 불일치
```

### 4.4 수정 방향

선택지는 두 가지다. 현재 BE Stage 5 진행 상태에 맞춰 하나를 명시적으로 선택해야 한다.

#### Option C1 — multi-xterm subscriber 를 지금 연결

권장되는 최종 방향이다.

필요 작업:

- `XtermHost` props 를 legacy `paneId` 중심에서 terminal identity 중심으로 확장한다.

```ts
type XtermHostMode =
  | { mode: 'legacy'; paneId: string }
  | { mode: 'terminal'; terminalId: string; sessionName: string; panelId: string }
```

- backend 가 terminal UUID 기준 output/input/resize routing 을 제공하는지 확인한다.
- `paneOut` store/key naming 을 legacy pane id 와 terminal UUID 로 분리한다.
- terminal item 이 alive/dangling 상태일 때 UI 를 다르게 표시한다.
- 같은 terminal UUID 를 여러 panel 이 mirror 할 수 있으므로, subscriber 는 panel instance 별로 독립 xterm buffer 를 관리해야 한다.

#### Option C2 — xterm streaming 이 아직 BE 의존이면 pending UI 로 막기

BE 5-C/5-D 또는 multi-xterm subscriber 가 아직 준비되지 않았다면, blank xterm 을 보여주면 안 된다. 사용자는 terminal 이 고장난 것으로 이해한다.

필요 작업:

- `PanelNode` 에서 v2 terminal item 인 경우 `XtermHost` 를 바로 mount 하지 않는다.
- 대신 명시 상태를 표시한다.

```text
Terminal stream pending
Session layout is attached, but live terminal streaming is not connected yet.
```

- dangling terminal 인 경우 `PanelDanglingOverlay` 를 우선 표시한다.
- legacy panel 은 기존 `XtermHost` 를 그대로 사용한다.

### 4.5 권장 결정

데모 시연 기준이면 C1 이 가장 좋다. 단, backend 가 아직 UUID 기반 terminal stream 을 제공하지 않는다면 C2 로 사용자 혼란을 줄이고, terminal pool/list/layout 조작 중심 데모로 범위를 제한해야 한다.

중요한 것은 blank/non-interactive xterm 을 active terminal 처럼 보여주지 않는 것이다.

### 4.6 수용 기준

C1 선택 시:

- multi-session terminal panel 에서 output 이 표시된다.
- 키 입력이 해당 terminal 로 전달된다.
- panel resize 시 terminal resize 가 전달된다.
- 같은 terminal 을 여러 panel 에 mirror 해도 각 panel 의 xterm host 가 깨지지 않는다.

C2 선택 시:

- multi-session terminal panel 이 blank xterm 으로 보이지 않는다.
- streaming 미연결 상태가 명시적으로 표시된다.
- legacy single-session xterm 은 기존처럼 동작한다.

### 4.7 검증 시나리오

1. session attach 후 terminal item 이 있는 layout 을 연다.
2. terminal body 에 output 이 표시되는지 확인한다.
3. 키 입력 후 backend terminal 에 전달되는지 확인한다.
4. panel resize 후 terminal cols/rows 가 갱신되는지 확인한다.
5. 같은 terminal UUID 를 두 panel 에 mirror 한 뒤 둘 다 정상 렌더링되는지 확인한다.

---

## 5. 이슈 D — legacy `pane-spawned` auto-mount 와 multi-session 충돌

### 5.1 증상

multi-session 경로가 활성화된 상태에서도 legacy WebSocket `pane-spawned` notify 가 들어오면 v1 `appendPanelIfMissing` 가 실행될 수 있다.

가능한 문제:

- active session layout 이 아닌 legacy panel layout 이 변경된다.
- backend 의 server-wide catch-up notify 가 현재 session 과 무관한 pane 을 canvas 에 추가할 수 있다.
- Stage 5 의 `MOUNT_CASCADE` / `TERMINAL_LIST_UPDATE` 경로와 중복된다.

### 5.2 관련 코드

| 파일 | 위치 | 내용 |
|---|---:|---|
| `codebase/frontend/src/lib/ws/dispatcher.svelte.ts` | `handleNotifyMirror()` | `pane-spawned` notify 수신 시 auto-mount handler 호출 |
| `codebase/frontend/src/routes/+page.svelte` | `setAutoMountHandler()` | v1 `appendPanelIfMissing(paneId, { token })` 로 연결 |
| `codebase/backend/crates/ws-server/src/lib.rs` | socket catch-up | alive pane 에 대해 `pane-spawned` notify / output catch-up 송신 |
| `codebase/frontend/src/lib/ws/decode.ts` | 0x86/0x87 decoder | Stage 5 신규 frame decoder 는 준비됨 |

### 5.3 원인

dual-source migration 중 legacy WS dispatcher 와 multi-session sessionStore 가 동시에 살아 있다.

legacy 흐름:

```text
WS NOTIFY pane-spawned
  -> dispatcher.handleNotifyMirror
  -> autoMountHandler
  -> appendPanelIfMissing
  -> legacy panelsStore/layout mutate
```

multi-session 목표 흐름:

```text
session-scoped event
  -> active session 확인
  -> sessionStore/mutateLayout
  -> v2 CanvasItem 추가 또는 terminalPool refresh
```

현재는 두 흐름이 명확히 격리되지 않았다.

### 5.4 수정 방향

권장안:

- `sessionStore.active !== null` 인 동안 legacy `pane-spawned` auto-mount 를 비활성화한다.
- 또는 `setAutoMountHandler()` 내부에서 active session 여부를 확인해 early return 한다.
- Stage 5 이후 새 terminal auto mount 는 0x86 `MOUNT_CASCADE` 만 사용한다.
- terminal pool 갱신은 0x87 `TERMINAL_LIST_UPDATE` 또는 `GET /api/terminals` refresh 로 처리한다.
- legacy single-session demo path 는 `sessionStore.active === null` 일 때만 유지한다.

예상 guard:

```ts
setAutoMountHandler(async (paneId) => {
  if (sessionStore.active) return;
  await appendPanelIfMissing(paneId, { token });
});
```

단, 이 guard 는 임시 안전장치다. 최종적으로는 legacy bearer WS 의 lifecycle 자체를 cookie/session-scoped WS 로 교체해야 한다.

### 5.5 수용 기준

- active session 이 있는 상태에서 legacy `pane-spawned` notify 가 들어와도 v1 panelsStore 가 mutate 되지 않는다.
- legacy single-session path 에서는 기존 auto-mount 가 유지된다.
- 0x86 `MOUNT_CASCADE` 가 들어오면 active session layout 에만 item 이 추가된다.
- 0x87 `TERMINAL_LIST_UPDATE` 는 terminalPool refresh 만 유발한다.

### 5.6 검증 시나리오

1. multi-session attach 상태에서 backend 가 legacy `pane-spawned` notify 를 보내는 상황을 만든다.
2. canvas 에 v1 panel 이 추가되지 않는지 확인한다.
3. legacy mode 에서 새 pane 생성 시 기존 auto-mount 가 동작하는지 확인한다.
4. 0x86/0x87 mock frame 또는 실제 frame 으로 sessionStore/terminalPool 동작을 확인한다.

---

## 6. 권장 작업 순서

### 6.1 Batch 1 — demo blocker 제거

1. 이슈 A 수정: `/?t=` token 을 cookie login 으로 교환
2. 이슈 B 수정: v2 terminal close flow 를 legacy close 조건에서 분리
3. 이슈 C 결정: UUID 기반 terminal streaming 연결 또는 pending UI 로 명시

Batch 1 완료 후 사용자는 최소한 다음 흐름을 수행할 수 있어야 한다.

```text
token link 진입
  -> cookie auth 완료
  -> session 선택/attach
  -> terminal item canvas 표시
  -> terminal panel close/remove 가능
  -> terminal body 상태가 정상 또는 pending 으로 명확히 표시
```

### 6.2 Batch 2 — WS/session 경계 정리

1. legacy `pane-spawned` auto-mount guard 추가
2. 0x86 `MOUNT_CASCADE` 를 active session layout append 로 확정
3. 0x87 `TERMINAL_LIST_UPDATE` 를 terminalPool refresh 로 확정
4. cookie-only 또는 cookie-additive WS auth 전환 계획과 맞춘다

### 6.3 Batch 3 — 사용자 사용성 보완

1. `PanelCloseConfirmModal` 문구와 action semantics 정리
2. dangling terminal overlay 와 respawn path 연결
3. terminal pool item 의 attach/kill/respawn 후 toast 및 refresh 일관화
4. failure state 를 blank UI 대신 명시 UI 로 표시

---

## 7. 구현 시 주의할 불변식

### 7.1 Pane 과 Panel 을 혼동하지 말 것

- `Terminal` / backend process identity 는 terminal UUID 중심으로 간다.
- `Panel` / canvas item identity 는 layout item id 중심이다.
- 같은 terminal UUID 는 여러 panel 에 mirror 될 수 있다.
- close action 은 항상 "panel 제거"와 "terminal kill"을 분리해야 한다.

### 7.2 tmux/pty state 와 web layout state 를 섞지 말 것

- terminal alive/dead/output/input 은 backend/terminal domain 이다.
- item 위치/크기/visibility/z/group 은 web layout domain 이다.
- `DELETE /api/sessions/:name/items/:id?kill_terminal=false` 는 web layout mutation 이다.
- `kill_terminal=true` 는 layout mutation 과 terminal lifecycle mutation 을 함께 수행하는 명시 action 이다.

### 7.3 legacy path 는 보존하되 active session 과 격리할 것

현재 migration 중이므로 legacy single-session path 가 남아 있다. 단, multi-session active 상태에서는 legacy auto-mount/layout mutation 이 실행되지 않도록 해야 한다.

### 7.4 blank terminal 을 정상 UI 로 보여주지 말 것

streaming 이 연결되지 않은 terminal panel 은 pending/dangling/unsupported 상태를 보여줘야 한다. blank xterm 은 사용자가 입력이 먹히지 않는 버그로 인식한다.

---

## 8. 최소 QA 체크리스트

### 8.1 Auth

- [ ] cookie 없는 브라우저에서 `/?t=<valid-token>` 로 진입 가능
- [ ] invalid token 실패가 복구 가능
- [ ] `/auth` server-rendered page 와 `/auth-preview` SPA preview 의 역할이 충돌하지 않음
- [ ] logout 후 cookie 제거 및 `/auth` redirect 정상

### 8.2 Session attach

- [ ] `GET /api/sessions` 200 후 workspace switcher 표시
- [ ] existing session attach 성공
- [ ] attach conflict / confirm_required 상태 UI 정상
- [ ] attach 후 layout load 및 `sessionStore.active` 설정 정상

### 8.3 Canvas terminal panel

- [ ] v2 terminal item 이 canvas node 로 표시
- [ ] v2 terminal close 버튼이 활성화
- [ ] panel-only remove 정상
- [ ] kill terminal 포함 close 정상
- [ ] legacy panel close 기존 동작 유지

### 8.4 Terminal body

- [ ] UUID terminal streaming 연결 또는 pending UI 표시 중 하나가 명확히 적용
- [ ] blank/non-interactive xterm 이 active terminal 처럼 보이지 않음
- [ ] resize/input/output 상태가 선택한 구현 방향에 맞게 검증됨

### 8.5 WS/session boundary

- [ ] active session 상태에서 legacy `pane-spawned` auto-mount 가 v1 layout 을 변경하지 않음
- [ ] legacy mode 에서는 기존 auto-mount 유지
- [ ] 0x85 terminal died 수신 시 dangling/terminalPool 갱신
- [ ] 0x86/0x87 처리 시 active session filtering 이 적용됨

---

## 9. 완료 정의

이 문서의 후속 작업은 다음 상태가 되면 완료로 본다.

1. token link 로 앱에 진입해 session attach 까지 끊기지 않는다.
2. multi-session terminal panel 에서 close/remove/kill 의 사용자 선택이 가능하다.
3. terminal body 는 실제 streaming 이 되거나, 아직 미지원이면 명확한 pending UI 를 표시한다.
4. multi-session active 상태에서 legacy `pane-spawned` auto-mount 가 session layout 을 오염시키지 않는다.
5. 위 항목에 대한 수동 QA 결과가 `docs/reports/` 에 짧게 기록된다.

