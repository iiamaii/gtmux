# 0033 — Frontend Stage 1~5 (partial) multi-session pivot progress

- 일자: 2026-05-15
- 작성자: frontend agent (Claude session)
- 종류: 진행 snapshot — multi-session pivot 의 FE 측 전체 작업 종료 시점
- 후속 reading order:
  1. **본 문서 §0 + §1 + §3 + §8** — 한 줄 요약 + 현 상태 + 파일 인벤토리 + 잔여 작업
  2. `docs/agents/frontend-handover.md` §4 — 누적 ✅/❌ 매트릭스 (라이브 진행 추적)
  3. `docs/reports/0032-stage-4-terminal-pool-and-pivot-be-progress.md` — BE Phase 4 산출
  4. `docs/reports/0031-stage-1-3-multi-session-be-progress.md` — BE Stage 1~3 산출
  5. `docs/adr/0018-canvas-item-data-model.md`, `0019-session-and-workspace-model.md`, `0020-auth-lifecycle.md`, `0021-terminal-pool-and-mirror.md`, `0024-layer-tree-and-z-index-separation.md` — schema/lifecycle 정본

---

## 0. 한 줄 요약

`docs/agents/frontend-handover.md` 의 Stage 1~3 + Stage 5 부분 + BE Phase 4-B/C/D 의 FE 연동분이 모두 완료. 사용자는 `/auth/bootstrap?token=…` 한 URL 로 진입해 cookie 인증 → AuthDialog → New/Open existing → match-or-spawn confirm → Canvas 의 SvelteFlow 노드로 sessionStore.items 가 표시되는 상태까지 도달. **xterm streaming 만 Stage 5 의 WS cookie + multi-xterm subscriber (BE-NEW-4) 의존**으로 후속 — 그 외 multi-session HTTP 흐름 (auth/sessions/terminals/layout/items) 은 모두 FE-BE 정합 동작.

---

## 1. 현 코드 상태 스냅샷

- 작업 트리: `codebase/frontend/`, 미커밋 (`src/main.ts`, `src/routes/+page.svelte`, `src/lib/...` 다수 신규/수정)
- `svelte-check`: **268 files / 0 errors / 0 warnings**
- `vite build`: 성공. dist 최신 hash 예: `index-D5DLNGtB.js` (gzip 31.21 KB)
- TypeScript strict 통과. Svelte 5 runes 모드.

### 1.1 새 dist 적용

backend 재기동 불필요 — `ServeDir + ServeFile(index.html)` 가 매 요청마다 read.

```bash
( cd codebase/frontend && npm run build ) && \
  unset TMUX && \
  GTMUX_FRONTEND_DIST="$(pwd)/codebase/frontend/dist" \
  GTMUX_SERVER__SESSION=demo \
  GTMUX_SERVER__PORT=9999 \
  GTMUX_SERVER__BIND=127.0.0.1 \
  ./codebase/backend/target/release/gtmux start --session demo --port 9999
```

---

## 2. 본 세션 완료 작업 (chunk 별 시간순)

### 2.1 Stage 1 — FE Foundation (BE 무의존)

| 산출 | 정본 |
|---|---|
| `lib/types/canvas.ts` | ADR-0018 D1/D3/D4. `CanvasItem` discriminated union 10 type (terminal/text/note/rect/ellipse/line/free_draw/image/document/file_path) + `ItemCommon` (maximized 제외 G20) + `Viewport` + `CanvasLayout` envelope + 10 type guard |
| `lib/types/group.ts` | ADR-0010 D6 + ADR-0024 D3. `Group` (z field 없음) + propagation helpers: `effectiveVisibility` (AND), `effectiveLocked` (OR), `inheritedLabel/Color`, `directChild*`, `descendant*`, `getAncestors` |
| `lib/stores/sessionStore.svelte.ts` | ADR-0019 D5. session-scoped layout/M/I/viewport/maximize. `active`/`items`/`groups`/`viewport`/`M`/`I`/`maximizedItemId` + `loadLayout`/`switchSession`/`clear` + M/I/maximize 조작 |

### 2.2 Stage 2 — Auth + Sessions UI (BE Stage 2 의존)

| 산출 | 정본 |
|---|---|
| `lib/types/auth.ts` | ADR-0020 + BE actual `auth.rs:436`. `LoginRequest { token? \| password?, redirect? }` (mode 는 server-set), `LoginResponse` 5분기 (ok/invalid/rate_limited/unavailable/bad_request) |
| `lib/types/sessions.ts` | `SessionInfo`/`AttachResponse` (ok\|confirm_required\|conflict)/`CreateSession*`/`AttachConfirmSummary`/`AttachConfirmResponse` |
| `lib/http/auth.ts` | `login`/`logout`/`rotateToken`. 429 → rate_limited (BE field `retry_after_secs`), 401 → invalid, 503 → unavailable, 400 → bad_request |
| `lib/http/sessions.ts` | `listSessions` (bare array 정규화)/`createSession` (flat `{name}` → SessionInfo synthesize)/`deleteSession`/`attachSession` (200/409/404)/`detachSession`/`getLayout` (ETag)/`putLayout`/`mutateLayout` (412 자동 1회 rebase)/`attachConfirm`/`deleteItem` + `UnauthorizedError`/`EtagMismatchError` |
| `src/routes/auth/+page.svelte` | ref/frontend-design/auth.html 디자인 SPA 포팅. Token/Password tab, `?t=` 자동 처리, eye toggle, rate-limit countdown, theme toggle. **BE 가 `/auth` 를 server-rendered HTML 로 직접 처리하므로 SPA path 는 `/auth-preview` 로 이전 (main.ts 정합)** |
| `src/routes/auth/+layout.ts` | `ssr=false` |
| `lib/chrome/AuthDialog.svelte` | switchboard (New / Open existing 2-choice grid icon card) |
| `lib/chrome/NewSessionModal.svelte` | name regex validate `^[A-Za-z0-9_-]{1,64}$` + duplicate pre-check + `POST /api/sessions` |
| `lib/chrome/SessionListModal.svelte` | Available/In-use 2-section + 1s polling + active row disabled + pid badge + chevron |

### 2.3 Stage 3 부분 — 라우팅 + AttachConfirm + ActiveSessionDropdown

| 산출 | 비고 |
|---|---|
| `src/main.ts` | pathname-based mount: `/auth-preview*` → AuthPage, 그 외 AppPage. `/auth` 는 BE-rendered → SPA 도달 X |
| `lib/chrome/AttachConfirmModal.svelte` | spawn/keep badge UI. BE 4-C 정합 (matched_item_ids keep + spawn_count) |
| `lib/chrome/ActiveSessionDropdown.svelte` | Toolbar2 우측. 현 session 이름 + accent pool size 배지 + green status dot. 클릭 시 WorkspaceSwitcher.open() |
| `lib/chrome/WorkspaceSwitcher.svelte` | AuthDialog → New/SessionList → AttachConfirm 흐름 통합. `tryAttach`/`confirmAttach` (`POST attach/confirm`) wiring |
| `lib/stores/workspaceSwitcher.svelte.ts` | stage 머신 (closed/choice/create/list/attach_confirm) + `pendingSession`/`pendingSummary` |
| `routes/+page.svelte` amend | (a) auth-gate: `GET /api/sessions` 401 → `/auth` redirect / 200 + active 없음 → `workspaceSwitcher.open()` 자동 (b) `?t=<token>` 자동 sessionStorage 캡처 + URL clean (c) `acquireToken()` 의 `window.prompt()` fallback 제거 (cookie 인증 사용자 prompt 혼란 해소) |
| `lib/chrome/SessionMenu.svelte` amend | [Switch workspace session…] / [Sign out] item. Sign out 은 `POST /auth/logout` + `/auth` redirect |

### 2.4 BE-FE contract 정합 작업 (BE Stage 2 완료 후 발견된 mismatch)

- `POST /api/sessions` → flat `{name}` (FE 가정 `{session: SessionInfo}` 였음). FE 가 SessionInfo synthesize
- `GET /api/sessions` → bare array (FE 가정 `{sessions: [...]}`). FE 측에서 정규화
- `POST attach` 200 → `{name, attached, server_id}` (no layout). FE 가 `getLayout()` 별도 호출
- `POST attach` 409 → `{holder: {pid}}` (FE 가정 top-level `active_server_pid`). FE 가 nested 파싱
- 모든 fix `lib/http/sessions.ts` 안에 반영

### 2.5 Stage 5 부분 — Toolbar2 12-도구 (BE 무의존)

| 산출 | 비고 |
|---|---|
| `lib/stores/toolStore.svelte.ts` | G22 one-shot + Q lock + Esc chain. STICKY_MODES = {select, hand}. `set/consume/toggleLock/handleEsc` |
| `lib/toolbar/Toolbar2.svelte` | 12 도구 (Select/Hand/Terminal/Rect/Ellipse/Line/FreeDraw/Text/Note/Image/Document/FilePath) + group dividers + tooltip + Q-lock visual ring. ADR-0018 D4 ↔ ItemType 1:1 |
| `routes/+page.svelte` 마운트 | Titlebar 44px 아래 Toolbar2 56px |

⚠️ **toolStore 의 consume() 호출이 아직 없음** — Stage 5 의 creation gestures (click-to-create / drag-to-create) 를 Canvas 에 wire 하면 자동 Select 복귀 동작.

### 2.6 BE Phase 4-B 연동 — Terminal pool 읽기 + 쓰기

| 산출 | 비고 |
|---|---|
| `lib/types/terminals.ts` | `TerminalInfo { id, alive, label, created_at, attach_count, attached_sessions[] }` |
| `lib/http/terminals.ts` | `listTerminals` (503→empty graceful), `killTerminal` (4-D), `respawnTerminal` (4-D) |
| `lib/stores/terminalPool.svelte.ts` | 공유 폴링 캐시. ref-count `subscribe()` — 1+ 활성 시 5s poll, 0 떨어지면 stop. `refresh()` 즉시 fetch |
| `lib/sidebar/TerminalListSection.svelte` | 신규 section. alive dot + label/short-id + attach_count badge + `unplaced` placeholder + `Xm ago` meta. Hover-revealed [+] attach + [×] kill. Mirror-affected count toast |
| `lib/sidebar/Sidebar.svelte` 마운트 | Layer-list section 아래 |

### 2.7 BE Phase 4-C/D 연동 — Attach confirm + kill/respawn/deleteItem

| 산출 | 비고 |
|---|---|
| `attachSession` 응답 분기 갱신 | BE 4-C: 200 `{matched, unmatched}` → unmatched.length>0 → confirm_required 활성 |
| `attachConfirm(name)` http helper | BE 4-C `POST attach/confirm` → `{spawned, already_present, failed}` |
| `deleteItem(name, id, killTerminal?)` http helper | BE 4-D `DELETE /api/sessions/<name>/items/<id>?kill_terminal=bool` (현재 unmounted — Stage 5 PanelCloseConfirmModal 와 함께 wire) |
| `killTerminal(id)` http helper | BE 4-D — TerminalListSection [×] 버튼에서 wire |
| `respawnTerminal(id)` http helper | BE 4-D — 현재 unmounted (Stage 5 의 dangling overlay 와 함께 wire) |
| WorkspaceSwitcher `confirmAttach` | per-UUID failed toast + spawned count toast + getLayout + loadLayout + terminalPool.refresh + close |

### 2.8 FE-only big surgery — Canvas/Sidebar/PaneInfoPanel sessionStore migrate

**dual-source 패턴**: `useSessionStore = $derived(sessionStore.active !== null)`. multi-session 사용자는 sessionStore 가 source, 그 외 (Sprint 7 legacy demo) 는 panelsStore/groupsStore.

| Surface | 변경 |
|---|---|
| `Canvas.svelte` | nodes 매핑 — multi-session 시 sessionStore.items.filter(isTerminal).map(itemToNode). itemToNode: visibility 문자열→bool, UUID 를 pane_id 슬롯에 노출 (Stage 5 multi-xterm migrate 호환). 클릭 → sessionStore.toggleM/setM/clearM. drag commit → `mutateLayout(activeSession, mutator)` → loadLayout |
| `Sidebar.svelte` | tree 빌더 dual-source. sessionStore.groups + items.filter(isTerminal) 을 legacy GroupData/PanelData shape 으로 어댑트. selectNode 도 dual |
| `PaneInfoPanel.svelte` | selectedPanelId / selectedPanel 어댑터. multi-session 시 sessionStore.items.get(id) 의 v2 → legacy shape 변환 + Terminal · Pool section (attach_count + attached_sessions chips + alive/dangling) |

⚠️ 한계: Stage 5 의 multi-xterm subscriber + WS cookie migrate 가 들어와야 xterm streaming 동작. 현재는 layout/select/drag/pane info 까지만 multi-session 흐름. Legacy WS dispatcher 는 여전히 작동 중 (dual channel).

### 2.9 Demo 편의 / contract fix 자잘한 항목

- `NewPanelButton` 의 "WebSocket not ready" cryptic msg → 안내문으로 교체 (cookie 사용자에게 sessionStorage.gtmux_token + Stage 4 안내)
- `acquireToken()` 의 `window.prompt()` 제거 — 사용자 진입 시 native dialog 혼란 해소
- `/?t=<token>` 자동 캡처 + URL clean — magic-link 통합 시 Bearer 도 함께 활성
- WorkspaceSwitcher 의 attach 404 fallback (Stage 3 시점 가짜 attach) 제거 — BE Phase 4-C/D 후 실 attach 작동

---

## 3. 파일 인벤토리 (신규/amend)

### 3.1 신규 (이번 multi-session pivot 에서 추가)

```
src/lib/types/canvas.ts                          ★ Stage 1
src/lib/types/group.ts                           ★ Stage 1
src/lib/types/sessions.ts                        ★ Stage 2
src/lib/types/auth.ts                            ★ Stage 2
src/lib/types/terminals.ts                       ★ Phase 4-B

src/lib/stores/sessionStore.svelte.ts            ★ Stage 1
src/lib/stores/workspaceSwitcher.svelte.ts       ★ Stage 3
src/lib/stores/toolStore.svelte.ts               ★ Stage 5
src/lib/stores/terminalPool.svelte.ts            ★ Phase 4-B

src/lib/http/auth.ts                             ★ Stage 2
src/lib/http/sessions.ts                         ★ Stage 2/4
src/lib/http/terminals.ts                        ★ Phase 4-B

src/lib/chrome/AuthDialog.svelte                 ★ Stage 2
src/lib/chrome/NewSessionModal.svelte            ★ Stage 2
src/lib/chrome/SessionListModal.svelte           ★ Stage 2
src/lib/chrome/AttachConfirmModal.svelte         ★ Stage 3
src/lib/chrome/ActiveSessionDropdown.svelte      ★ Stage 3
src/lib/chrome/WorkspaceSwitcher.svelte          ★ Stage 3

src/lib/sidebar/TerminalListSection.svelte       ★ Phase 4-B

src/lib/toolbar/Toolbar2.svelte                  ★ Stage 5

src/routes/auth/+page.svelte                     ★ Stage 2 (BE 가 /auth shadow — /auth-preview)
src/routes/auth/+layout.ts                       ★ Stage 2
```

### 3.2 amend (기존 파일 수정)

```
src/main.ts                                      pathname routing (auth-preview only)
src/routes/+page.svelte                          auth-gate, ?t 자동 캡처, WorkspaceSwitcher 마운트, Toolbar2 마운트, prompt 제거
src/lib/chrome/SessionMenu.svelte                Switch session / Sign out items
src/lib/chrome/PaneInfoPanel.svelte              dual-source + Terminal · Pool section
src/lib/canvas/Canvas.svelte                     dual-source nodes / click / drag
src/lib/canvas/NewPanelButton.svelte             정보 메시지 개선
src/lib/sidebar/Sidebar.svelte                   dual-source tree + TerminalListSection 마운트
```

### 3.3 unmounted (헬퍼만 작성, 후속 wiring 필요)

```
src/lib/http/sessions.ts:
  - deleteItem(name, id, killTerminal?)  → Stage 5 PanelCloseConfirmModal 와 함께
src/lib/http/terminals.ts:
  - respawnTerminal(id)                  → Stage 5 dangling overlay 와 함께
```

---

## 4. HTTP route surface — FE 가 호출하는 endpoint

```
GET    /auth                                     (BE server-rendered HTML)
POST   /auth/login                               body { token? | password?, redirect? }
POST   /auth/logout
GET    /auth/bootstrap?token=…                   (legacy redirect)

GET    /api/sessions                             bare array [{name, active}]
POST   /api/sessions                             body { name } → 201 {name}
DELETE /api/sessions/:name
GET    /api/sessions/:name/layout                v2 schema + ETag
PUT    /api/sessions/:name/layout                v2 + If-Match
POST   /api/sessions/:name/attach                200 {name, attached, server_id, matched, unmatched}
DELETE /api/sessions/:name/attach                detach
POST   /api/sessions/:name/attach/confirm        body {} → 200 {name, spawned, already_present, failed}
DELETE /api/sessions/:name/items/:id?kill_terminal=bool  (helper present, unmounted)

GET    /api/terminals                            bare array TerminalInfo[]
POST   /api/terminals/:id/kill                   204
POST   /api/terminals/:id/respawn                200 {id}                (helper present, unmounted)
```

---

## 5. Architectural notes (다음 agent 가 기억해야 할 회로)

### 5.1 Dual-source 패턴

Canvas / Sidebar / PaneInfoPanel 가 `useSessionStore = $derived(sessionStore.active !== null)` 으로 분기. multi-session 진입 후엔 sessionStore, 그 외 legacy panelsStore/groupsStore/ephemeralStore.m. Stage 5 의 BE-NEW-4 통합 시 legacy 폐기 + 단일 채널.

### 5.2 schema v2 ↔ legacy shape adapter

sessionStore 의 v2 type (`CanvasItem`, `Group`) 을 *legacy* (`PanelData`, `GroupData`) shape 으로 down-convert 하는 어댑터 패턴. 핵심 변환:
- `visibility: "visible"|"hidden"` → `boolean`
- terminal item.id (UUID) → legacy `pane_id` 슬롯 (Stage 5 multi-xterm 까지 호환)

후속에서 모든 consumer 가 v2 직접 read 로 통일 (legacy 폐기) 하면 어댑터 제거.

### 5.3 BE Phase 4-C 의 attach 분리 흐름

BE 가 *attach* (lock 잡고 분류만) 과 *attach/confirm* (spawn) 을 별 endpoint 로 분리. FE 가:
1. `attach` → 200 `{matched, unmatched}`
2. unmatched.length>0 → AttachConfirmModal → 사용자 [Confirm attach]
3. `attach/confirm` → spawn 결과 → getLayout → loadLayout

`attach` 만 성공한 상태에서 close switcher 하면 session 이 "lock 잡힘 + spawn 안됨" 상태 — 사용자가 다시 unsaved confirm 진입 가능 (cookie 가 holder).

### 5.4 mutateLayout 412 rebase

`mutateLayout(name, mutator)` 은 GET → mutate → PUT 시 412 시 한 번 자동 GET + retry. 두 번째 412 throw. caller closure 가 idempotent 가 핵심 (예: 같은 terminal item 두 번 append 방지).

### 5.5 terminalPool 의 ref-count subscribe

`terminalPool.subscribe()` 호출 시 ref count 증가, 1 → polling start, 0 → stop. 여러 consumer (TerminalListSection, PaneInfoPanel, ActiveSessionDropdown) 가 한 채널 공유. 사용자 액션 직후 `terminalPool.refresh()` 로 즉시 fetch (latency 단축).

### 5.6 cookie 인증 + Bearer token 의 dual-channel

- HTTP `/api/*` 와 modal stack: cookie 인증 (credentials:'include')
- WS subprotocol (xterm streaming, legacy NewPanel): Bearer token (sessionStorage)
- `/?t=<token>` 자동 캡처가 두 채널 통합 — Bearer 와 cookie 모두 활성

BE-NEW-4 의 WS cookie 통합 후 sessionStorage Bearer 완전 폐기.

### 5.7 `/auth` 는 BE 가 owns

`GET /auth` 의 server-rendered HTML 은 BE 가 직접 — JS bundle 무관 의도 (auth.rs:405 코멘트). FE `routes/auth/+page.svelte` 는 design preview 용 으로 `/auth-preview` 에 격리.

### 5.8 SvelteFlow node type 단일

현재 `nodeTypes = { panel: PanelNode }` — terminal item 만 표면. Stage 5 의 non-terminal renderer (TextNode/NoteNode/ShapeNode/LineNode/FilePathNode) 가 들어오면 type 추가 + `nodes` derivation 에서 filter 제거.

---

## 6. Demo E2E walkthrough

```
1. /auth/bootstrap?token=X     → BE 303 → /auth?token=X
2. /auth?token=X               → BE token verify + Set-Cookie + 303 → /
3. /                           → SPA mount → /api/sessions ping
   ├ 401                       → /auth redirect (BE-rendered)
   └ 200 + active 없음         → WorkspaceSwitcher.open() 자동
4. AuthDialog                  → [New session]
5. NewSessionModal             → "demo" 입력 → POST /api/sessions → onCreated → tryAttach
6. POST attach                 → 200 {matched=[], unmatched=[]}
                                → getLayout → sessionStore.loadLayout
7. Canvas + Sidebar            → 비어있음 (empty layout)
8. Sidebar > Terminals         → GET /api/terminals (empty pool)
9. (사용자가 별 webpage 에서 [New Terminal] 또는 attach/confirm 으로 spawn 가정)
10. Sidebar Terminals 의 [+]  → mutateLayout → terminal item 추가 → Canvas SvelteFlow node 노출
11. Sidebar Terminals 의 [×]  → POST kill → toast (dangling)
12. 패널 drag                  → mutateLayout PUT → loadLayout
13. 패널 클릭                  → sessionStore.M → PaneInfoPanel 갱신 (Terminal · Pool section)
14. SessionMenu (≡) → [Sign out] → POST /auth/logout + /auth redirect
```

xterm streaming 은 현재 작동 X — Stage 5 BE-NEW-4 후 정상.

---

## 7. Known issues / gotchas

| # | 항목 | 후속 |
|---|---|---|
| G1 | xterm streaming 미작동 (cookie 인증 사용자) | BE-NEW-4 (WS cookie + session_id frame routing) + FE multi-xterm subscriber (FE-NEW-6) |
| G2 | Legacy WS dispatcher (pane-spawned/LAYOUT_CHANGED) 가 multi-session 흐름과 dual channel — 충돌 없으나 비효율 | Stage 5 의 WS migrate 시 단일화 |
| G3 | `NewPanelButton` 의 legacy WS 의존 — multi-session 사용자에게는 작동 X | Stage 5 의 Toolbar Terminal 도구 + WS routing 후 정합 |
| G4 | `PanelNode` 의 close 버튼이 legacy `sendCtrl('kill-pane')` 호출 — multi-session 정합 X | `deleteItem(sessionName, panelId, killTerminal?)` 으로 amend + PanelCloseConfirmModal (G25, 3 옵션) wire |
| G5 | `ContextMenu` 의 "Close pane" 도 legacy WS — same as G4 | Stage 6 의 panel header V2 amend 와 함께 |
| G6 | sessionStore.active 가 in-memory — reload 시 초기화. WorkspaceSwitcher 가 다시 열림 | sessionStore.active 를 sessionStorage 영속 또는 BE 의 `GET /api/sessions/me` 같은 endpoint 후속 검토 |
| G7 | toolStore.consume() 호출하는 caller 없음 — Q lock 토글은 작동, one-shot 자동 복귀는 inactive | Stage 5 의 creation gesture 통합 시 활성 |
| G8 | AttachConfirmModal 의 BE Phase 4-C 실 시나리오 검증 안 됨 — 단위 테스트 없음 | smoke gate 추가 (handover §10.1 의 smoke-7 ~ 9) |
| G9 | `respawnTerminal` / `deleteItem` 헬퍼 unmounted | Stage 5/6 의 dangling overlay + Panel close UX 와 함께 wire |
| G10 | 멀티-session 동시 attach (cross-server flock) 의 FE 측 conflict UX | SessionListModal 의 active row 가 이미 disabled — pid badge 노출 (BE 4-C `lock_conflict_response` 의 `holder.pid` 파싱). 401/409 외 status 의 toast 메시지 폴리시 검토 |

---

## 8. 잔여 작업 (next session 권고 순서)

### 8.1 BE Stage 5 의존 (FE-NEW-6 multi-xterm subscriber)

Stage 5 batch 의 BE 작업 (handover-be §6 Stage 5 / 0032 §9.1):
1. **WS frame 의 session_id field** — selection/viewport/focus
2. **server-side per-session WS routing** — connection table refactor
3. **`terminal-died` UUID-carrying WS frame** — dangling overlay 트리거
4. **Auto-mount trigger-aware** — `[New Terminal]` 의 trigger session 만 cascade

FE 측 작업:
- `lib/ws/dispatcher.svelte.ts` amend — session_id field 처리 + dispatch table 확장
- `lib/canvas/PanelNode.svelte` amend — *per-panel xterm 인스턴스* + broadcast subscriber 패턴 (ADR-0021 D1)
- `lib/canvas/PanelDanglingOverlay.svelte` 신규 — terminal_died 수신 → 그 UUID 의 모든 panel 에 overlay → click → respawnTerminal(id)
- WS handshake 시 cookie + session_id query param (BE Stage 5-A 의 ADR-0020 D10 정합)

### 8.2 FE-only 후속

| 우선순위 | 항목 | 비고 |
|---|---|---|
| ✅ DONE | **PanelNode close 의 deleteItem 정합** | `deleteItem(sessionName, panelId, killTerminal?)` wire. `PanelCloseConfirmModal.svelte` (G25, 3 옵션 [Cancel/Panel only/Panel + Terminal]) 신규. dual-source — multi-session 시 modal, legacy 시 sendCtrl |
| ✅ DONE | **ContextMenu 의 ARRANGE section** | 4 z 액션 (Bring to front/Bring forward/Send backward/Send to back) + Remove from canvas (deleteItem killTerminal=false). useSessionStore 분기 |
| ✅ DONE | **Inline edit (G23) 공용 컴포넌트** | `lib/common/InlineEditField.svelte` (single) + `InlineEditTextarea.svelte` (multi). 실 consumer wire 는 후속 (Panel header label rename, Group label edit, Note title/body 등) |
| ✅ DONE | **Esc 라우터** | `lib/common/escRouter.svelte.ts` — 7-priority chain + default fallback. 실 register 는 후속 — 현재 InlineEdit* 만 priority 1 등록 |
| ✅ DONE | **zStore** | ADR-0024 D2 의 4 z 액션 + mutateLayout 자동 commit |
| 🟢 P1 | **Stage 5 creation gestures** | Toolbar 의 12 도구 → Canvas 의 click-to-create (Text/Note/FilePath) + drag-to-create (Rect/Ellipse/Line). 완료 시 `toolStore.consume()` 호출 → one-shot Select 복귀. ADR-0018 D4 의 payload field 정합 |
| 🟢 P1 | **Stage 5 non-terminal Node renderers** | TextNode/NoteNode/ShapeNode/LineNode/FilePathNode (FreeDraw/Image/Document 는 P3). Canvas 의 `nodeTypes` registry 확장 + filter 제거 |
| 🟢 P1 | **InlineEditField consumer wire** | PanelNode header 의 label rename, Sidebar Layer-list group label inline edit, NoteNode title/body 등에서 사용. 모달 안에서도 사용 가능 |
| 🟢 P1 | **Z keyboard shortcuts** | `[`/`]`/`⇧[`/`⇧]` 키 binding → `zStore.bring*/send*`. G26 의 keyboard registry P1+ 와 함께 |
| 🟡 P2 | **Layer list V2 (Stage 6)** | Tree/Z toggle + group propagation 시각화 + multi-select (Cmd/Shift click + marquee) + drag reorder/reparent + GroupCloseConfirmModal. ADR-0024 + ADR-0010 G25 정합 |
| 🟡 P2 | **Panel header V2 (Stage 6)** | 4 z 액션 (ContextMenu 만 wire, header more menu 도 추가) + Kill/Remove + dangling overlay + ChangeTerminalModal |
| 🟡 P2 | **file_path open UX (Stage 5)** | ADR-0023. `FileOpenConfirmModal.svelte` 신규 + SettingsOverlay 의 Storage section allowlist editor. BE-NEW-12 의존 |
| 🟡 P2 | **Viewport sync (Stage 7)** | session-scoped viewport ↔ SvelteFlow pan/zoom 양방향 sync (debounce). sessionStore.viewport 와 SvelteFlow useSvelteFlow viewport 연결 |
| 🟡 P2 | **Settings overlay G19 (Stage 7)** | full-screen overlay + Section nav + auto-save + allowlist editor |
| 🟢 P1 | **PaneInfoPanel — Design panel section 폴리시** | ref/frontend-design/SPEC.md §7. Selection chip + tabs (Design/Prototype/Inspect placeholder) |
| 🟢 P1 | **Titlebar tabs polish** | File/Edit/View placeholder (1-tab → 5-tab). ref §3 정합 |

### 8.3 정합 / 정리

- Legacy `/api/layout` v1 endpoint + `LayoutStore` / `panelsStore` / `groupsStore` 폐기 (Stage 5 후)
- WS subscriber Lagged reconciliation (BE 0032 §5.6)
- `gtmux start --session <name>` flag 제거 (workspace+session 모델에서 무의미, BE 0031 잔존)

---

## 9. Required reading order (cold pickup)

1. **본 문서** §0 + §1 + §2 + §8 — context + 산출 + 잔여
2. `docs/agents/frontend-handover.md` — 라이브 진행 매트릭스 + Stage 명세
3. `docs/reports/0032-stage-4-terminal-pool-and-pivot-be-progress.md` §4 + §9.1 — BE Phase 4 산출 + Stage 5 batch brief
4. `docs/reports/0031-stage-1-3-multi-session-be-progress.md` — BE 진실 source
5. `docs/adr/0018-canvas-item-data-model.md` D1/D2/D3/D4/D6 — schema v2 + match-or-spawn
6. `docs/adr/0019-session-and-workspace-model.md` D3/D5/D7 — single-attach + dialog + session-scoped store
7. `docs/adr/0020-auth-lifecycle.md` D1/D2/D4/D5/D8 — auth modes + cookie + rate limit
8. `docs/adr/0021-terminal-pool-and-mirror.md` D1/D2/D3/D6/D7/D10 — multi-xterm + dangling + heartbeat
9. `docs/adr/0024-layer-tree-and-z-index-separation.md` — Tree/Z 분리
10. `docs/agents/frontend-handover.md` §6 Stage 5/6/7 — 잔여 작업의 명세

### 9.1 첫 명령

```bash
cd /Users/ws/Desktop/projects/gtmux
cat docs/reports/0033-fe-stage-1-to-5-partial-progress.md   # 본 문서
cat docs/agents/frontend-handover.md                         # 라이브 진행
cat docs/reports/0032-stage-4-terminal-pool-and-pivot-be-progress.md

cd codebase/frontend
npm run check    # 268 files / 0 errors / 0 warnings 확인
npm run build    # 빌드 정합

# 다음 작업 시작 위치 — Stage 5 BE-NEW-4 후라면:
grep -n "ws/dispatcher\|subscribe_pane_output" src/lib/ws/dispatcher.svelte.ts

# FE-only 정리 시작 위치:
grep -n "sendCtrl\|kill-pane" src/lib/canvas/PanelNode.svelte
grep -n "deleteItem" src/lib/http/sessions.ts  # 헬퍼 위치 확인
```

---

## 10. 빌드 / 실행 명령

```bash
# Frontend
cd codebase/frontend
npm run check         # svelte-check
npm run build         # vite production → dist/

# Demo launch (full)
cd /Users/ws/Desktop/projects/gtmux
( cd codebase/frontend && npm run build ) && \
  unset TMUX && \
  GTMUX_FRONTEND_DIST="$(pwd)/codebase/frontend/dist" \
  GTMUX_SERVER__SESSION=demo \
  GTMUX_SERVER__PORT=9999 \
  GTMUX_SERVER__BIND=127.0.0.1 \
  ./codebase/backend/target/release/gtmux start --session demo --port 9999
```

브라우저: `gtmux start` 출력 magic-link 또는 `http://127.0.0.1:9999/auth/bootstrap?token=<TOKEN>&redirect=/?t=<TOKEN>` (cookie + Bearer 통합).

---

## 11. 변경 이력

- 2026-05-15: 초안 — multi-session pivot 의 FE Stage 1~5 부분 + BE Phase 4-B/C/D 연동 완료 시점 snapshot. Stage 5 multi-xterm + FE-only 정리 plan 포함.
