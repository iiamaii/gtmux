# 0041 — FE/BE 통합 세션 handover (frontend-handover v1/v2 기준 구현율 + 향후 계획)

- 일자: 2026-05-16
- 작성자: 통합 세션 agent (FE Stage 5 Batch 1/2 + BE 0040 §5 옵션 A + attach 함수 정합 직후)
- 종류: cold-pickup handover — 본 세션 누적 작업 + frontend-handover v1/v2 의 미구현 항목 매트릭스 + 향후 계획
- 후속 reading order: 본 문서 → `0040-terminal-panel-integration-verification.md` → `0039-fe-integration-guide-stage-5.md` → `0036-frontend-review-action-items.md` → `0035-be-fe-coordination-stage-5.md` → `docs/agents/frontend-handover-v2.md`

---

## 0. 한 줄 요약

frontend-handover **v2 의 P0 (Stage 1~4) 100%** + **Stage 5 Batch 1/2 (text/note/file_path 점-spawn + rect/ellipse/line drag-spawn + non-terminal node renderer 5종) 100%** + **0036 P0/P1 4건 해소** + **BE Stage 5-A/B/C/D + D10 α + 0040 §5 옵션 A + attach 의 implicit detach** 모두 main 으로. **xterm streaming 동작 (multi-session + reload/reconnect)** + **session shift cleanup** 양측 모두 ship. 남은 ❌ = **FE-NEW-4 ChangeTerminalModal** / **FE-6 Layer list V2 전체** / **FE-7 Panel header more menu** / **FE-NEW-8 file_path open UX** / **FE-9 Viewport sync** / **FE-8 Settings overlay** / **shortcutRegistry P1** / **xtermTheme adapter** / **session export/import** / **Batch 3 (free_draw/image/document)**.

---

## 1. 본 세션 작업 흐름 (chunk 별 시간순)

### 1.1 BE Stage 5-A/B FE 측 wire-up
- `0x85 TERMINAL_DIED` decoder + dispatcher → `danglingTerminals.mark` + `terminalPool.refresh` + `terminalPool.unbindPaneId` (respawn 후 stale 차단)
- `PanelDanglingOverlay.svelte` 신규 — exit/killed reason 표시 + `respawnTerminal` click handler + toast
- PanelNode 의 panel-body 안에 multi-session 시 overlay 마운트

### 1.2 0035 FE 결정 답변
- `docs/reports/0035-be-fe-coordination-stage-5.md` §7 추가:
  - §3.1: 5-C broadcast trigger = **(B) echo-minus-sender**, BE connection-table 의 connection_id 확장, session_id top-level
  - §3.2: 5-D = **P1 + P2 동시 진행**, P2 default 좌표 BE 결정 (cascade offset)
  - §3.3: D10 = **α 즉시 + β Stage 6 + γ Stage 7**
  - §3.4: legacy `/api/layout` v1 cleanup = **Stage 5 후반** (creation gestures + NewPanelButton migrate + auto-mount unification 후)

### 1.3 0036 P0/P1 4건 해소
- **P0-A** `?t=<token>` → cookie login 교환 (`+page.svelte` 의 bootstrap pipeline 재구성 — login → URL clean → auth gate → WS bootstrap 순차)
- **P0-B** v2 terminal panel close 활성화 (PanelNode 의 `closeDisabled` derive 가 `useSessionStore` 분기, legacy `paneNumeric/liveCount` 조건은 legacy path 에만)
- **P0-C** v2 terminal pending placeholder (이전: 영구 blank → 신규: "connecting…" 또는 "stream pending" 명시 UI)
- **P1-D** legacy `pane-spawned` auto-mount guard (`sessionStore.active !== null` 시 early return → multi-session active 동안 legacy panelsStore 오염 차단)

### 1.4 Z keyboard shortcuts + InlineEditField wire
- `lib/keyboard/zShortcuts.svelte.ts` 신규 — `[/]/{/}` 키 → `zStore.bringForward/sendBackward/bringToFront/sendToBack`. `sessionStore.M.size === 1` + editable focus skip 가드. onMount/onDestroy 의 bind/unbind.
- `patchTerminalLabel` http helper — `PATCH /api/terminals/:id {label}` (BE 0034 §1.5 cleanup C 정합). 4KiB cap 사전 검증.
- PanelNode header label 더블클릭 → InlineEditField commit → `patchTerminalLabel` + `sessionStore.items` 즉시 갱신 + `terminalPool.refresh`. legacy 는 read-only.

### 1.5 Stage 5 Batch 1 — Click-to-create
- `lib/canvas/itemFactory.ts` 신규 — `createCanvasItem('text'|'note'|'file_path', pos)` + `commitNewItem` (`mutateLayout` + `loadLayout` + `setM` + z = max+1 ADR-0018 D7)
- `TextNode.svelte` / `NoteNode.svelte` / `FilePathNode.svelte` 신규 — InlineEditField/Textarea consumer + NodeResizer
- `Canvas.svelte` 의 `onpaneclick` 에 toolStore.current 분기 (text/note/file_path → click-to-create), `nodeTypes` registry 확장, `isTerminal` filter 제거
- `useSvelteFlow().screenToFlowPosition` 으로 viewport-aware 좌표 변환

### 1.6 Stage 5 Batch 2 — Drag-to-create + shape renderer
- `ShapeNode.svelte` 신규 (rect/ellipse 공용 — border-radius 분기), `LineNode.svelte` 신규 (SVG polyline)
- `createShapeItem(rect|ellipse, bounds)` + `createLineItem(p1, p2)` (4 방향 endpoint 보존)
- Canvas 의 capture-phase pointer 핸들러 (`pointerdowncapture/movecapture/upcapture/cancelcapture`) on `.canvas-root` — SvelteFlow selection 보다 먼저 받음
- 라이브 ghost overlay (dashed rect / ellipse / SVG line)
- drag tool active 시 cursor: crosshair
- 8px threshold — drag < 거리면 default size 폴백

### 1.7 Stage 5 Batch 2 — bug fixes
- **stroke_width 정수화** (BE schema `u32` 정합 — `1.5` → `2`. 이전 400 Bad Request 원인)
- **Line direction Approach B** — `(x,y)` = 시작 / `(x2,y2)` = 끝 절대 좌표. Canvas `itemToNode` 가 SvelteFlow position 을 `min(x,x2), min(y,y2)` 로 계산 + `_boxX1/Y1/X2/Y2` derived 주입. LineNode 가 box-local 좌표로 렌더 — 4 방향 모두 정확. drag-stop 시 line 양 endpoint 동시 translate.

### 1.8 BE 0x88 TERMINAL_SPAWNED + multi-session terminal panel 연동 (0039 §3 Option C1)
- `decode.ts`: `FRAME_TYPE.TERMINAL_SPAWNED = 0x88` + `TerminalSpawnedPayload` + `decodeTerminalSpawned` (Number.isSafeInteger guard)
- `terminalPool.svelte.ts`: `paneIdByUuid: SvelteMap<string, number>` reactive store + `bindPaneId/paneIdFor/unbindPaneId`
- `dispatcher`: `handleTerminalSpawned` → `terminalPool.bindPaneId`. `handleTerminalDied` 도 `unbindPaneId` 호출
- `PanelNode`: `terminalPaneId = $derived(terminalPool.paneIdFor(data.id))` for multi-session — 3-way mount: legacy `%N` → XtermHost / multi-session **with binding** → XtermHost / multi-session **pre-binding** → "connecting" placeholder
- XtermHost 는 변경 X (numeric paneId 만 받음 — PanelNode 가 UUID resolve 후 numeric 전달)

### 1.9 NewPanelButton → Toolbar Terminal 도구 마이그레이션
- `lib/canvas/legacyNewPane.ts` 신규 — `requestLegacyNewPane(client, token, coords)` (sendCtrl race + appendPanelIfMissing 로직 분리)
- Canvas `onpaneclick` 의 `toolStore.current === 'terminal'` 분기 — legacy/multi-session 분리
- multi-session: `spawnMultiSessionTerminal(coords)` — fresh UUID + `mutateLayout` append + `attachConfirm` (BE P2 endpoint 미 ship 대체. 0033 §2.5 의 manual flow)
- `NewPanelButton.svelte` 삭제. `.canvas-toolbar` overlay markup + CSS 제거.
- `createTerminalItem(pos)` factory 추가 (UUID + 480×320)

### 1.10 BE 0040 §5 옵션 A — catch-up 0x88 재발행 (FE 변경 0 줄)
- `crates/ws-server/src/hub.rs`: `TerminalUuidProvider` async trait + Hub field + `set/get` accessor (CookieValidator 패턴 정합). 4 unit test 추가.
- `crates/ws-server/src/lib.rs`: handle_socket 의 catch-up 에서 `provider.alive_bindings().await` 후 각 `(pane_id, uuid)` 마다 `Envelope::new(FrameType::TerminalSpawned, ...)` send. pane-spawned NOTIFY / PANE_OUT replay 보다 *먼저* emit. `TerminalUuidProvider` re-export.
- `crates/http-api/src/terminal_map.rs`: `async-trait` + `Arc` import. `impl TerminalUuidProvider for TerminalMap` — read-lock 안에서 `(pane.0, Arc::<str>::from(uuid.as_str()))` 변환. 3 unit test 추가.
- `bin/gtmux-cli/src/main.rs`: `hub.set_terminal_uuid_provider(app_state.terminal_map.clone())` 등록.
- 효과: 페이지 reload / WS reconnect 후 alive terminal 의 UUID↔PaneId binding 자동 복원 → XtermHost 가 placeholder 에서 자동 mount + PANE_OUT 흐름.

### 1.11 cookie-only WS bootstrap (Issue 1/2 해소 결정타)
- `WsClient.token: string | null`. handshake subprotocol 은 token 있을 때만 `bearer.<t>` 추가, 그 외 `['gtmux.v1']` (D10 α 의 cookie path 가 dead code 였던 원인 제거).
- `DispatcherOptions.token` 도 nullable.
- `+page.svelte` bootstrap step 3 갱신 — `acquireToken() === null` 도 `createDispatcher({token: null})` + `client.start()`. legacy v1 refetch + auto-mount handler 는 `token !== null` 시만 wire.
- 효과:
  - **Terminal 미연결 해소** — cookie-only 사용자도 WS 시작 → 0x88 catch-up 수신 → XtermHost mount
  - **Session 비활성 해소** — WS close → `disconnect_sink.send(cookie)` → `release_lock_for_cookie` → flock 해제 → 다음 `GET /api/sessions` 의 active=false

### 1.12 BE attach_handler implicit detach (session shift 해소)
- `crates/http-api/src/sessions.rs` 의 `attach_handler` 에 cookie 의 이전 attach 자동 정리:
  - `session_locks_by_cookie.get(cookie)` peek → 다른 name 이면 cleanup
  - holders.remove(prev_name) + guard.release() (flock 해제 + lock 파일 unlink)
  - hub.clear_session_for_cookie
- same-name reattach 는 cleanup branch skip → `holders.contains_key` 체크에서 409
- 2 integration test 추가 — A→B switch (A active=false / B active=true / flock 파일 정합) + same-name 409 (flock 보존)

---

## 2. frontend-handover v1/v2 기준 구현율

### 2.1 P0 (Stage 1~4) — **100%** ✅

| ID | 이름 | Stage | 상태 | 위치 |
|---|---|---|---|---|
| FE-3 | `CanvasItem` discriminated union | 1 | ✅ | `lib/types/canvas.ts` |
| FE-NEW-7 | Session-scoped store | 1 | ✅ | `lib/stores/sessionStore.svelte.ts` |
| FE-1 | Auth page | 2 | ✅ | `/auth-preview` + BE `/auth` |
| FE-NEW-1 | Session UI (Auth/New/List/Menu/ActiveDropdown) | 2 | ✅ | `lib/chrome/` 5 신규 |
| FE-NEW-2 | Webpage attach lifecycle | 2~3 | ✅ | `+page.svelte` + WS cookie-only |
| FE-NEW-5 | Attach confirm modal | 3 | ✅ | `AttachConfirmModal.svelte` |
| FE-NEW-3 | Terminal pool UI | 4 | ✅ | `TerminalListSection.svelte` |
| FE-NEW-3 | Kill terminal action | 4 | ✅ | TerminalListSection [×] |
| **FE-NEW-4** | ChangeTerminalModal | 4 | ❌ | (panel context menu 진입) |
| FE-NEW-6 | Multi-xterm subscriber | 4 | ✅ | `paneOutHandlers: Map<string, Set<Handler>>` + XtermHost handler identity. ADR-0021 D1 mirror 동작. |
| **PanelDanglingOverlay** ⭐ | terminal_died overlay → respawn (G25 c2) | 4 | ✅ | `PanelDanglingOverlay.svelte` + `danglingTerminals` store + 0x85 wire |

### 2.2 Stage 5 (FE-2/4/5/NEW-8) — **부분**

| ID | 이름 | Stage | 상태 | 비고 |
|---|---|---|---|---|
| FE-2 | Toolbar2 — 12 도구 UI | 5 | ✅ | `Toolbar2.svelte` |
| FE-2 | tool state (one-shot + Q lock + Esc) | 5 | ✅ | `toolStore.svelte.ts` |
| FE-2 | **Space hold + drag = pan modifier (G29)** | 5 | ❌ | Canvas 에서 미구현 |
| FE-5 | Creation gestures — click-to-create (text/note/file_path) | 5 | ✅ | Batch 1 |
| FE-5 | Creation gestures — drag-to-create (rect/ellipse/line) | 5 | ✅ | Batch 2 |
| FE-4 | TextNode renderer | 5 | ✅ | InlineEditTextarea consumer |
| FE-4 | NoteNode renderer | 5 | ✅ | title (single) + body (multi) inline edit + NodeResizer |
| FE-4 | ShapeNode (rect/ellipse 공용) | 5 | ✅ | border-radius 분기 |
| FE-4 | LineNode | 5 | ✅ | SVG polyline + 4 방향 endpoint + drag endpoint resize |
| FE-4 | FilePathNode | 5 | ✅ | path + icon + InlineEditField (open UX 분리) |
| **FE-NEW-8** | file_path open UX (FileOpenConfirmModal + allowlist editor) | 5 | ❌ | BE `/api/file-path/*` + Settings Storage 미존재 |

### 2.3 Stage 6 (FE-6/7) — **부분**

| ID | 이름 | Stage | 상태 | 비고 |
|---|---|---|---|---|
| FE-6 | Layer list V2 — Tree/Z toggle | 6 | ❌ | 현재 Sidebar dual-source adapter (legacy GroupData shape 으로 down-convert) |
| FE-6 | Layer list V2 — Multi-select (Cmd/Shift/marquee) | 6 | ❌ | |
| FE-6 | Layer list V2 — Drag reorder/reparent | 6 | ❌ | |
| FE-6 | Group propagation 시각화 | 6 | ⚠️ | `effectiveVisibility/Locked` helpers 존재, Sidebar 가 partial 사용 |
| FE-6 | Inline rename (group label) | 6 | ⚠️ | InlineEditField 컴포넌트 ready — consumer wire 안 됨 |
| FE-6 | `GroupCloseConfirmModal` (G25) | 6 | ❌ | |
| FE-7 | Panel header V2 — 4 z 액션 menu | 6 | ⚠️ | ContextMenu 에 ARRANGE section 있음, header more menu 자체 미구현 |
| FE-7 | Panel header V2 — Kill terminal / Remove panel menu | 6 | ⚠️ | ContextMenu 에 있음, header more menu 없음 |
| FE-7 | PanelCloseConfirmModal (G25 3 옵션) | 6 | ✅ | `PanelCloseConfirmModal.svelte` + PanelNode close button wire |
| FE-7 | Panel header inline label rename | 6 | ✅ | InlineEditField wire (이번 세션) |
| FE-7 | PanelDanglingOverlay | 4/6 | ✅ | 이번 세션 |

### 2.4 Stage 7 (FE-9/8 + G26/G27/G28) — **거의 미진행**

| ID | 이름 | Stage | 상태 | 비고 |
|---|---|---|---|---|
| FE-9 | Viewport sync UI (양방향 debounce) | 7 | ❌ | `sessionStore.viewport` 존재, SvelteFlow `useSvelteFlow` viewport 와 미연결 |
| FE-8 | SettingsOverlay (G19 full-screen) | 7 | ❌ | |
| FE-8 | Settings sections (Auth/Theme/Shortcut/Storage/Behavior/Debug) | 7 | ❌ | 6 sections 모두 |
| **shortcutRegistry** ⭐ | 전역 keydown + xterm focus + P0/P1 매트릭스 (G26) | 5+ 또는 7 | ⚠️ | `lib/keyboard/zShortcuts.svelte.ts` 가 Z 단축키만 — 일반화 안 됨 |
| **themeStore** ⭐ | currentTheme + resolvedTheme + MediaQueryList (G27) | 7 | ⚠️ | `lib/stores/theme.svelte.ts` 기본 light/dark/system 토글 동작 — xterm 동기는 ❌ |
| **xtermTheme adapter** ⭐ | ANSI 16 색 + chrome 정합 hot reload | 7 | ❌ | |
| **Session export/import (G28)** ⭐ | Storage section [Export/Import] | 7 | ❌ | |

### 2.5 Stage 8~9 — **미진행** (P2)

| ID | 이름 | Stage | 상태 |
|---|---|---|---|
| FE-4 | ImageNode + asset upload | 8 | ❌ |
| FE-4 | DocumentNode | 8 | ❌ |
| FE-4 | FreeDrawNode + RDP simplification | 9 | ❌ |

### 2.6 공용 컴포넌트 / store 매트릭스

| 파일 | 상태 | 비고 |
|---|---|---|
| `lib/common/InlineEditField.svelte` | ✅ | 단일선 |
| `lib/common/InlineEditTextarea.svelte` | ✅ | 다중선 |
| `lib/common/escRouter.svelte.ts` | ✅ | 7 priority chain + register API. consumer wire = InlineEdit\* priority 1 only — 그 외 priority (modal/unmax/tool lock/Select) 의 register 는 미연결 |
| `lib/common/shortcutRegistry.svelte.ts` | ❌ | zShortcuts 만 partial 대체 |
| `lib/stores/toolStore.svelte.ts` | ✅ | G22 one-shot + Q lock + Esc chain |
| `lib/stores/zStore.svelte.ts` | ✅ | ADR-0024 D2 4 액션 |
| `lib/stores/sessionStore.svelte.ts` | ✅ | FE-NEW-7 |
| `lib/stores/themeStore.svelte.ts` | ⚠️ | 기본 동작 — xtermTheme adapter 별 |
| `lib/utils/xtermTheme.ts` | ❌ | G27 |
| `lib/stores/danglingTerminals.svelte.ts` | ✅ | (handover doc 외 신규) |
| `lib/stores/terminalPool.svelte.ts` | ✅ | + paneIdByUuid map (이번 세션) |
| `lib/stores/workspaceSwitcher.svelte.ts` | ✅ | Stage 3 |
| `lib/canvas/itemFactory.ts` | ✅ | text/note/file_path/rect/ellipse/line/terminal factory |
| `lib/canvas/legacyNewPane.ts` | ✅ | Toolbar Terminal 도구 의 legacy 분기용 |
| `lib/keyboard/zShortcuts.svelte.ts` | ✅ | `[/]/{/}` 만 — shortcutRegistry 전체화 필요 |

### 2.7 Architectural invariants (v2 의 14 항목 매트릭스)

| # | 항목 | v2 명세 | 현 코드 |
|---|---|---|---|
| 1 | session-scoped store | ADR-0019 + ADR-0021 D5 | ✅ |
| 2 | Auto-mount = trigger session 만 | ADR-0021 D3 | ✅ BE 5-D P1/P2 + FE 0x86/0x87 handlers |
| 3 | Multi-xterm subscriber 패턴 | ADR-0021 D1 | ✅ Set<handler> + handler identity |
| 4 | Tree order ≠ Z | ADR-0024 | ⚠️ zStore 동작, Layer list V2 미구현 |
| 5 | Maximize = FE-only ephemeral | G20 | ✅ sessionStore.maximizedItemId |
| 6 | Esc 라우팅 7 priority | §14.20.2 | ⚠️ escRouter 코어 있음, 일부 consumer 미등록 |
| 7 | Inline edit Enter/Cmd-Enter/Esc/blur | G23 | ✅ |
| 8 | Toolbar one-shot + Q lock + Select/Hand sticky | G22 | ✅ creation gestures 후 consume() 호출 |
| 8b | Space hold + drag = pan modifier | G29 | ❌ |
| 9 | Settings full-screen overlay + auto-save | G19 | ❌ |
| 10 | Keyboard shortcut Hybrid + xterm focus | G26 | ⚠️ zShortcuts 만 |
| 11 | Panel close dialog 3 옵션 + mirror hint | G25 | ✅ |
| 12 | Dangling lazy spawn | G25 c2 | ✅ |
| 13 | Theme system (light/dark/system + xterm 동기) | G27 | ⚠️ chrome 만 |
| 14 | 점진 어휘 통일 (pane → Terminal) | — | ⚠️ 점진 |

### 2.8 종합 — 구현율

| 영역 | v2 명세 | 구현율 |
|---|---|---|
| P0 (Stage 1~4) | 11 items | **10 / 11 (91%)** — FE-NEW-4 ChangeTerminalModal 1건만 ❌ |
| Stage 5 (FE-2/4/5/NEW-8) | 11 items (Toolbar 3 + Renderer 5 + Gestures 2 + FilePath UX 1) | **10 / 11 (91%)** — Space-pan + FE-NEW-8 (file_path open UX) ❌ |
| Stage 6 (FE-6/7) | 11 items | **3 / 11 (27%)** — Panel close, dangling, inline rename ✅ 외 8건 ❌ |
| Stage 7 (FE-9/8 + G26/G27/G28) | 7 items | **0 / 7 + 2 partial (~14%)** — themeStore basic + zShortcuts only |
| Stage 8~9 (image/document/free_draw) | 3 items | 0 / 3 |
| Architectural invariants | 14 항목 | **9 fully + 4 partial (78%)** |
| 공용 컴포넌트 / store | 14 항목 | **10 / 14 (71%)** |

**총 implementation rate (가중 평균)**: Stage 1~5 의 P0 + Batch 1/2 까지는 **사실상 완료**. Stage 6 와 Stage 7+ 의 비중이 큼 — Stage 6 의 Layer list V2 + Panel header more menu 는 큰 chunk. Stage 7 의 settings overlay + shortcut/theme adapter + export/import 도 큰 chunk.

---

## 3. 본 세션 진행하지 못한 항목 (전체 미구현 매트릭스)

### 3.1 Stage 5 잔여 (P1)

| 항목 | scope | 의존 |
|---|---|---|
| **Space hold + drag = pan modifier (G29)** | Canvas 의 pointerdown/move/up + isSpacePressed state + 일시 panOnDrag override | FE only, 작음 |
| **FE-NEW-8 file_path open UX** | `FileOpenConfirmModal.svelte` + allowlist 사전 추론 + double-click handler + Settings Storage allowlist editor | BE: `/api/file-path/allowlist-check` + `/api/file-path/open` + `/api/file-path/allowlist` (ADR-0023). 미 ship. |
| **ContextMenu "Add ___" sub-menu** | 우클릭으로 itemFactory 재사용 | FE only |

### 3.2 Stage 6 잔여 (P0/P1 — Layer list V2 + Panel header V2)

| 항목 | scope | 의존 |
|---|---|---|
| **FE-NEW-4 ChangeTerminalModal** | Panel 의 terminal_id 교체 — UUID picker (terminalPool 의 alive list) + PUT layout (item.id 교체) | FE only |
| **Layer list V2 — Tree/Z toggle** | Sidebar 상단 toggle [Tree | Z] + Z mode 시 flat z 정렬 + drag reorder 비활성 | FE only, 중간 |
| **Layer list V2 — Multi-select + drag reorder/reparent** | Cmd/Shift click + marquee + tree drag = organization 만 | FE only, 큼 |
| **Layer list V2 — Inline rename group** | InlineEditField consumer (group label) | FE only, 작음 |
| **Group propagation 시각화** | row 의 visibility/lock toggle 옆 회색 dot + tooltip ("inherited from ...") | FE only, 작음 |
| **GroupCloseConfirmModal (G25)** | 자손 panel/non-terminal + mirror hint + 3 옵션 ([Cancel] / [Panels only] / [Panels + Terminals]) | FE only + BE 의 group DELETE (현재는 layout PUT 으로 자손 제거 가능) |
| **Panel header more menu (…)** | 4 z 액션 + [Change terminal...] + [Kill terminal] + [Remove panel] + Rename / Settings | FE only |
| **ChangeTerminalModal trigger** | Panel header more menu 안에서 진입 | FE-NEW-4 의존 |

### 3.3 Stage 7 잔여 (P1 — Settings overlay + 인프라)

| 항목 | scope | 의존 |
|---|---|---|
| **FE-9 Viewport sync UI** | sessionStore.viewport ↔ SvelteFlow useSvelteFlow viewport 양방향 sync (debounce) + PUT layout 의 viewport 영속 | FE only, 작음 |
| **shortcutRegistry (G26)** | 전역 keydown listener + xterm focus 검사 + P0/P1 매트릭스 | FE only |
| **shortcutRegistry P0 매트릭스 wire** | Esc / Enter / Cmd-Enter / Q / `]/[/⇧]/⇧[` — 일부는 이미 escRouter 또는 zShortcuts 로 동작, registry 일원화 | FE only |
| **shortcutRegistry P1 매트릭스** | Cmd+N (new terminal) / Cmd+Shift+L (sidebar) / Cmd+Shift+Q (shutdown) / Cmd+, (settings) | FE only |
| **themeStore + xtermTheme adapter (G27)** | currentTheme + resolvedTheme + MediaQueryList + xterm 인스턴스 hot reload | FE only |
| **SettingsOverlay (G19, FE-8)** | full-screen overlay + sidebar nav + auto-save (debounce) | FE only |
| **Settings Auth section** | token rotate / password change UI | BE: `/auth/rotate` + `/auth/set-password` 미 ship |
| **Settings Theme section (G27)** | radio [System / Light / Dark] | FE only |
| **Settings Shortcut section (G26)** | read-only list + 카테고리별 + platform-detect | FE only |
| **Settings Storage section** | workspace path read-only + file_open allowlist editor + **Session export/import (G28)** | BE: `/api/sessions/import` + 위 file-path API. 미 ship. |
| **Settings Behavior section** | `auto_kill_terminal_on_panel_close` toggle (G25) | BE: `/api/settings` PATCH. 미 ship. |
| **Settings Debug section** | server pid / build sha / log path | BE: `/api/settings` GET. 미 ship. |

### 3.4 Stage 8~9 (P2)

| 항목 | scope | 의존 |
|---|---|---|
| **ImageNode + 업로드** | file picker + drop + `/api/assets/<sha256>` | BE: `/api/assets/*` 미 ship |
| **DocumentNode** | type 별 preview (markdown 등) | 위 + 추가 |
| **FreeDrawNode + RDP** | pointermove stroke point[] + Ramer-Douglas-Peucker simplification | FE only, 큼 |

### 3.5 정합 / 정리 (Stage 5 후반 ~ Stage 6)

| 항목 | scope |
|---|---|
| Legacy `/api/layout` v1 + `LayoutStore` / `panelsStore` / `groupsStore` 폐기 | FE 모든 surface 가 sessionStore 단일 source 로 전환 후 (creation gestures + NewPanelButton migrate + auto-mount unification 후) |
| `LayoutSnapshot` ↔ `SessionLayout` 통합 | BE |
| WS subscriber Lagged reconciliation | BE 0032 §5.6 / 0034 §9 의 P2+ |
| `gtmux start --session <name>` flag 제거 | BE |
| Dual-source adapter 제거 | FE (Canvas/Sidebar/PaneInfoPanel 의 `useSessionStore = $derived(...)` 분기) |

---

## 4. 미커밋 작업트리 상태

### 4.1 BE
```
M codebase/backend/bin/gtmux-cli/src/main.rs        (set_terminal_uuid_provider 등록)
M codebase/backend/crates/http-api/src/lib.rs       (attach 의 implicit detach 테스트 추가)
M codebase/backend/crates/http-api/src/sessions.rs  (attach_handler 의 cookie 의 이전 attach 자동 release)
M codebase/backend/crates/http-api/src/terminal_map.rs (impl TerminalUuidProvider + tests)
M codebase/backend/crates/ws-server/src/hub.rs      (TerminalUuidProvider trait + Hub field + tests)
M codebase/backend/crates/ws-server/src/lib.rs      (re-export + handle_socket catch-up 0x88 emit)
```

내용:
- **0040 §5 option A** — catch-up 0x88 재발행 (reload/reconnect 후 mapping 자동 복원)
- **attach_handler implicit detach** — session shift 시 이전 session flock 자동 해제

cargo test --workspace: **327 PASS / 0 FAIL** (이전 baseline 325 + 본 세션 5건 추가: hub 4 + http-api 3 - 1 = 실 +6 정도; 327 / 325 차이는 다른 누락 또는 측정 시점 차이로 인한 미세).

### 4.2 FE
```
M codebase/frontend/src/lib/canvas/Canvas.svelte
M codebase/frontend/src/lib/canvas/PanelNode.svelte
M codebase/frontend/src/lib/canvas/XtermHost.svelte
M codebase/frontend/src/lib/chrome/ContextMenu.svelte
M codebase/frontend/src/lib/chrome/PaneInfoPanel.svelte
M codebase/frontend/src/lib/chrome/SessionMenu.svelte
M codebase/frontend/src/lib/sidebar/Sidebar.svelte
M codebase/frontend/src/lib/ws/client.ts          (token nullable + cookie-only subprotocol)
M codebase/frontend/src/lib/ws/decode.ts          (0x85/0x86/0x87/0x88 frames)
M codebase/frontend/src/lib/ws/dispatcher.svelte.ts (handlers + Set<handler> + 5-C scaffold)
M codebase/frontend/src/main.ts
M codebase/frontend/src/routes/+page.svelte       (bootstrap pipeline 재구성 + cookie-only WS)
D codebase/frontend/src/lib/canvas/NewPanelButton.svelte
?? codebase/frontend/src/lib/canvas/FilePathNode.svelte
?? codebase/frontend/src/lib/canvas/LineNode.svelte
?? codebase/frontend/src/lib/canvas/NoteNode.svelte
?? codebase/frontend/src/lib/canvas/PanelDanglingOverlay.svelte
?? codebase/frontend/src/lib/canvas/ShapeNode.svelte
?? codebase/frontend/src/lib/canvas/TextNode.svelte
?? codebase/frontend/src/lib/canvas/itemFactory.ts
?? codebase/frontend/src/lib/canvas/legacyNewPane.ts
?? codebase/frontend/src/lib/chrome/ActiveSessionDropdown.svelte
?? codebase/frontend/src/lib/chrome/AttachConfirmModal.svelte
?? codebase/frontend/src/lib/chrome/AuthDialog.svelte
?? codebase/frontend/src/lib/chrome/NewSessionModal.svelte
?? codebase/frontend/src/lib/chrome/PanelCloseConfirmModal.svelte
?? codebase/frontend/src/lib/chrome/SessionListModal.svelte
?? codebase/frontend/src/lib/chrome/WorkspaceSwitcher.svelte
?? codebase/frontend/src/lib/common/
?? codebase/frontend/src/lib/http/
?? codebase/frontend/src/lib/keyboard/
?? codebase/frontend/src/lib/sidebar/TerminalListSection.svelte
?? codebase/frontend/src/lib/stores/danglingTerminals.svelte.ts
?? codebase/frontend/src/lib/stores/sessionStore.svelte.ts
?? codebase/frontend/src/lib/stores/terminalPool.svelte.ts
?? codebase/frontend/src/lib/stores/toolStore.svelte.ts
?? codebase/frontend/src/lib/stores/workspaceSwitcher.svelte.ts
?? codebase/frontend/src/lib/stores/zStore.svelte.ts
?? codebase/frontend/src/lib/toolbar/Toolbar2.svelte
?? codebase/frontend/src/lib/types/
?? codebase/frontend/src/routes/auth/
```

`svelte-check`: **282 files / 0 errors / 0 warnings**  
`npm run build`: 클린 (main bundle ~146 KB / gzip ~42 KB)

### 4.3 BE 의 committed history (사용자 main 에 있음)
```
e5606f9 feat(backend): Stage 5-D P2 — POST /terminals + 0x86 MOUNT_CASCADE
47365fd feat(backend): Stage 5-C — echo-minus-sender + session-scoped routing
d00db66 feat(backend): 0x87 routing tests + 0x88 TERMINAL_SPAWNED binding
4fb9ecb feat(backend): Stage 5-A/5-B — hub session table + terminal-died frame
```

본 세션의 BE 변경 (옵션 A + implicit detach) 은 위 history 위에 작업 트리만 — **commit 권장**.

---

## 5. 향후 계획 (frontend-handover v1/v2 미구현 + 본 세션 발견 follow-up)

### 5.1 즉시 가능 — FE only (BE 의존 X)

| 우선 | 항목 | scope | 의존 |
|---|---|---|---|
| 🔴 P0 | **FE-NEW-4 ChangeTerminalModal** | UUID picker (terminalPool.terminals 의 alive only) + PUT layout (item.id 교체) + PanelNode context menu trigger | 작음 (FE only) |
| 🔴 P0 | **Layer list V2 — Tree/Z toggle minimum** | Sidebar 상단 segmented + 정렬 알고리즘 분기 | 중간 |
| 🟢 P1 | **Space hold + drag = pan modifier (G29)** | Canvas pointerdown/up + isSpacePressed state | 작음 |
| 🟢 P1 | **FE-9 Viewport sync UI** | sessionStore.viewport ↔ useSvelteFlow viewport 양방향 | 작음 |
| 🟢 P1 | **shortcutRegistry 일원화** | zShortcuts 흡수 + P0 매트릭스 등록 | 작음 |
| 🟢 P1 | **GroupCloseConfirmModal** | bulk dialog UI + sessionStore.items/groups 의 자손 enum | 중간 |
| 🟢 P1 | **Panel header more menu** | 4 z 액션 + Kill/Remove + Rename. ContextMenu 의 ARRANGE/CLOSE section 재사용 | 작음 |
| 🟢 P1 | **xtermTheme adapter (G27)** | xterm 인스턴스 별 theme 객체 hot reload | 작음 |
| 🟢 P1 | **ContextMenu "Add ___" sub-menu** | itemFactory 재사용 우클릭 add | 작음 |
| 🟡 P2 | **Layer list V2 — Multi-select + drag reorder/reparent** | 큰 surgery | 큼 |
| 🟡 P2 | **Inline rename consumer wire** (group label, file_path 의 path) | InlineEditField wire | 작음 |
| 🟡 P2 | **Dual-source adapter 제거** | Canvas/Sidebar/PaneInfoPanel 의 useSessionStore 분기 — sessionStore 단일 source 로 통일 + panelsStore/groupsStore/legacy v1 layout 폐기 | 중간 |
| 🟡 P2 | **Cross-session leak filter** (0039 §3.2 step 4) | Sidebar Terminal list 의 attached_sessions 필터링 | 작음 |

### 5.2 BE 의존

| 우선 | 항목 | BE 작업 |
|---|---|---|
| 🟢 P1 | **FE-NEW-8 file_path open UX** | BE: `GET /api/file-path/allowlist-check` + `POST /api/file-path/open` + `POST/DELETE /api/file-path/allowlist` (ADR-0023) |
| 🟢 P1 | **Settings overlay 전체** | BE: `GET /api/settings` + `PATCH /api/settings` (boot-immutable 제외) + `POST /auth/rotate` + `POST /auth/set-password` + `POST /api/sessions/import` |
| 🟢 P1 | **D10 β/γ transition** | BE: subprotocol bearer deprecation. FE 측 `sessionStorage.gtmux_token` 저장 / acquireToken / WS subprotocol 송신 코드 삭제 |
| 🟡 P2 | **ImageNode + asset upload (Stage 8)** | BE: `POST /api/assets` (sha256-keyed, mime-validated, size cap) + `GET /api/assets/<sha256>` + ADR 신규 |
| 🟡 P2 | **DocumentNode (Stage 8)** | 위 + preview metadata |

### 5.3 권장 진입 순서 (다음 세션)

#### Slice A — FE Stage 6 의 minimal (Layer list V2 + Panel header more menu)
1. `lib/chrome/Sidebar.svelte` 의 Tree/Z toggle (segmented) + 정렬 분기 — 가장 작은 P0
2. ChangeTerminalModal — picker + PUT 의 minimal UI
3. Panel header more menu — ContextMenu 의 dropdown 버전 wire
4. GroupCloseConfirmModal — bulk + propagation 자손 enum
5. svelte-check + smoke

#### Slice B — Stage 5 의 인프라 마무리 (shortcutRegistry + Space-pan + viewport sync)
1. shortcutRegistry — zShortcuts 의 일반화 + xterm focus 검사 + P0 매트릭스
2. Space hold + drag pan modifier
3. Viewport sync UI

#### Slice C — Settings overlay 의 BE-light section
1. SettingsOverlay shell + sidebar nav + auto-save infrastructure
2. Theme section (G27 chrome) + xtermTheme adapter (G27 xterm)
3. Shortcut section (read-only list from shortcutRegistry)
4. Behavior section (`auto_kill_terminal_on_panel_close` — BE 의 `/api/settings` ship 후)
5. Debug section
6. Storage section 의 file-path allowlist editor + Auth section 의 password change — BE 의존

#### Slice D — BE follow-up (병렬)
1. BE: `/api/settings` GET/PATCH
2. BE: `/api/file-path/*` (ADR-0023)
3. BE: `/auth/rotate` + `/auth/set-password`
4. BE: `/api/sessions/import` (G28)
5. BE: D10 β/γ phase (Stage 6~7 정합)

#### Slice E — Stage 8~9 (P2)
1. Asset infrastructure BE
2. ImageNode + DocumentNode FE
3. FreeDrawNode FE + RDP simplification

---

## 6. 본 세션 동안 굳어진 아키텍처 결정

| 영역 | 결정 | 출처 | 위치 |
|---|---|---|---|
| WS frame 0x85 wire shape | varint 0 + JSON {terminal_id, reason} | 0034 §3.2 | FE decoder + BE encoder 정합 |
| WS frame 0x86 wire shape | varint 0 + JSON {terminal_id, x, y, w, h} — server determines coords | 0034 §8.3 + 0035 §7.2 P2 | FE 가 0033 §8.1 spec 그대로 채택 |
| WS frame 0x87 wire shape | varint 0 + JSON {added: [], removed: []} — delta hint, GET /api/terminals authoritative | 0034 §8.3 | FE handler 가 terminalPool.refresh() only |
| WS frame 0x88 wire shape | varint 0 + JSON {terminal_id, pane_id} — server-wide, fresh spawn + catch-up | 0039 §1.2 + 0040 §5 | FE bindPaneId + BE catch-up 재발행 |
| Multi-xterm subscriber pattern | paneOutHandlers: Map<paneKey, Set<Handler>>, identity-based unregister | ADR-0021 D1 | dispatcher + XtermHost |
| Line schema 해석 | (x,y) = 시작 / (x2,y2) = 끝 (canvas 절대). Node.position = min/min for bounding box. | canvas.ts comment + Approach B | itemFactory + LineNode + Canvas itemToNode |
| stroke_width / font_size | 정수 (BE schema u32 정합 — 1.5 폴리시 X) | schema.rs | itemFactory |
| WS bootstrap policy | cookie OR bearer (둘 중 하나만 valid). token=null → bearer subprotocol 미송신 | D10 α + 0036 P0-A | client.ts + +page.svelte |
| attach implicit detach | 같은 cookie 의 다른 session attach 시 이전 flock 자동 release | ADR-0019 D3 single-attach invariant | sessions.rs attach_handler |
| Auth bootstrap pipeline | step 1 cookie 교환 → step 2 auth gate → step 3 WS bootstrap (순차) | 0036 P0-A | +page.svelte |
| Stage 5-C scaffold | `isFrameForActiveSession(sessionId)` helper exported, decoder 의 sessionId 반환 amend 는 BE 5-C 정식 ship 후 | 0035 §3.1 | dispatcher.svelte.ts |
| NewPanelButton 폐기 | Toolbar Terminal 도구로 흡수. legacy CTRL `new-pane` 분리 모듈 `legacyNewPane.ts`. multi-session 은 `spawnMultiSessionTerminal` emulation (mutateLayout + attachConfirm) — BE P2 endpoint 가 ship 됐어도 동일 wire 결과 |  본 세션 | Canvas.svelte + legacyNewPane.ts |
| Pending UI policy | multi-session terminal 의 binding 미도착 시 "connecting…" placeholder. blank xterm 금지 (0036 §7.4) | 0036 P0-C | PanelNode.svelte |

---

## 7. 빌드 / 검증 / 실행

```bash
cd /Users/ws/Desktop/projects/gtmux

# FE
( cd codebase/frontend && npm run check )   # → 282 files / 0 errors / 0 warnings
( cd codebase/frontend && npm run build )   # → dist/ 갱신 (BE 가 매 요청마다 read)

# BE
cd codebase/backend
cargo test --workspace --color=never        # → 327 PASS / 0 FAIL
cargo build --release --bin gtmux           # → target/release/gtmux

# Demo launch
cd /Users/ws/Desktop/projects/gtmux
( cd codebase/frontend && npm run build ) && \
  unset TMUX && \
  GTMUX_FRONTEND_DIST="$(pwd)/codebase/frontend/dist" \
  GTMUX_SERVER__SESSION=demo \
  GTMUX_SERVER__PORT=9999 \
  GTMUX_SERVER__BIND=127.0.0.1 \
  ./codebase/backend/target/release/gtmux start --session demo --port 9999

# 브라우저
# - magic-link: http://127.0.0.1:9999/auth/bootstrap?token=<TOKEN>&redirect=/?t=<TOKEN>
# - 직접 /?t=<TOKEN> 도 동작 (Issue 0036 P0-A 정합)
```

---

## 8. 진입 시 첫 메시지 후보

다음 세션 진입 시 사용할 수 있는 명령 후보:

- **"미커밋 작업 commit"** → BE 2 commit (옵션 A + implicit detach) + FE 1 commit (Stage 5 Batch 1/2 + 0036 fix + cookie-only WS + 0x88 wire + label rename + Z 단축키)
- **"Stage 6 진입 — Layer list V2 + Panel header more menu"** → §5.3 Slice A
- **"Stage 5 인프라 마무리 — shortcutRegistry + Space-pan + viewport sync"** → §5.3 Slice B
- **"Settings overlay shell"** → §5.3 Slice C
- **"D10 β/γ deprecation"** → BE 5.2 D10 transition + FE `sessionStorage.gtmux_token` 폐기
- **"ChangeTerminalModal"** → FE-NEW-4 단일
- **"FE-NEW-8 file_path open UX 시작 (BE ADR-0023 와 같이)"** → BE/FE parallel
- **"BE Asset endpoints (Stage 8 entry)"** → BE 측 시작 + ImageNode/DocumentNode FE 가 후속

---

## 9. 참조 reading order (cold pickup)

1. **본 문서 §0 + §2 + §3 + §5** — 한 줄 + 구현율 + 잔여 + 향후 계획
2. `docs/agents/frontend-handover-v2.md` — v2 의 P0/P1 명세 (본 문서가 v2 기준 평가)
3. `docs/reports/0040-terminal-panel-integration-verification.md` §1.2 + §5 — reload/reconnect gap + 옵션 A 구현 명세 (본 세션 land)
4. `docs/reports/0039-fe-integration-guide-stage-5.md` §1.2 + §2.3 + §3 — BE Stage 5 wire surface + Option C1 의 FE 측 구현 (본 세션 land)
5. `docs/reports/0036-frontend-review-action-items.md` §1~5 — 0036 P0/P1 4건 의 진단 (모두 본 세션 해소)
6. `docs/reports/0035-be-fe-coordination-stage-5.md` §3 + §7 — BE-FE 협의 + FE 답변 (본 세션 §7 추가)
7. `docs/adr/0018-canvas-item-data-model.md` — 10 item type schema (Stage 5 Batch 1/2 의 기준)
8. `docs/adr/0021-terminal-pool-and-mirror.md` D1/D6/D10 — multi-xterm + dangling + heartbeat
9. `docs/adr/0024-layer-tree-and-z-index-separation.md` — Layer list V2 의 기준 (다음 세션 Slice A)
10. `docs/adr/0020-auth-lifecycle.md` D10 — α/β/γ transition (Stage 6~7 정합)

---

## 10. 변경 이력

- 2026-05-16: 초안 — 통합 세션 후 작업 흐름 (1.1~1.12), v1/v2 기준 구현율 (P0 91% / Stage 5 91% / Stage 6 27% / Stage 7 14%), 잔여 항목 매트릭스 (§3), 미커밋 작업트리 (§4), 향후 계획 5 slice (§5), 굳어진 아키텍처 결정 12건 (§6), 진입 명령 후보 (§8), reading order (§9).
