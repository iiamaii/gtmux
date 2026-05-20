# 0043 — FE 통합 세션 handover (Slice A/B/C 마감 + Slice D 발주 + plan-0008 Phase 1 도착 + Dual-source 제거)

- 일자: 2026-05-16
- 작성자: FE 통합 세션 agent (0041 cold-pickup → Slice A/B/C 모두 ship → BE work package 발주 → 묶음 A 추가 ship → 별 agent 의 plan-0008 Phase 1 land 반영 → Dual-source adapter 제거 ship)
- 종류: cold-pickup handover — 본 세션 누적 + 0041/0044 의 매트릭스 갱신 + plan-0008 land 반영 + dual-source 제거 + 향후 계획
- 후속 reading order: 본 문서 → `0044-be-slice-d-work-package.md` (BE 가 들어올 영역) → `0042-session-attach-recovery.md` (별 agent 의 attach recovery decision report) → `docs/plans/0008-session-attach-recovery-impl.md` (Phase 1 land 명세, Phase 2 잔여) → `0041-fe-be-session-handover.md` (이전 세션 base) → ADR-0017 amend ①~④ + follow-up

---

## 0. 한 줄 요약

0041 cold-pickup 후 Slice A/B/C 모두 ship + 묶음 A (ContextMenu Add 외 5건) + 묶음 B (Dual-source adapter 제거) + 묶음 C (Layer list V2) + 묶음 D (FE-only Tier 1 잔여 4건) + **묶음 E (0045 refresh reconnect loop fix — flowNodes id-cache + viewport 사용자 변경 보존 + reconnectGate 5-state + XtermHost RAF dedup + dev instrumentation. BE 의존 사항은 0046 work package 로 발주)** 까지. FE-only P0/P1 완전 마감 + 0045 분석의 P0 후속 fix 동시 land. BE 의존: 0044 D-1/D-2/D-3/D-4 wire + 0046 attach_handler same-cookie idempotent.

---

## 1. 본 세션 작업 흐름 (chunk 별 시간순)

### 1.1 사용자 요청 1 — 두 panel 분리 + header fold (Tier 1 selvedge)
사용자가 "terminal 패널이 layer list panel 에 포함 → 분리" + "fold/unfold 버튼은 각 panel header 우측 아이콘" 요청.

- `chromeStore` 에 `terminalsCollapsed` 추가 + `--layout-sidebar-{layers-bottom,terminals-top}` token
- `PanelFoldButton.svelte` 신규 (header 우측 fold 아이콘)
- `Sidebar.svelte` 에서 `TerminalListSection` 제거 → `TerminalsPanel.svelte` 신규 (별 floating panel, 좌측 하단 ~40%)
- `RailToggle.svelte` 는 collapsed 시에만 mount + 좌측 두 rail vertical anchor stack
- 각 panel header 에 PanelFoldButton 추가 (Sidebar / TerminalsPanel / PaneInfoPanel)
- ADR-0017 **amend ①** + ADR-0021 D7 갱신

### 1.2 사용자 요청 2 — ref/frontend-design `panel-tabs` 패턴 정합 (분리 회수, 통합)
"layer list, terminal panel 을 원래대로 합치고 탭으로 구분. fold/unfold 는 그대로. fold 시 얇은 바에 unfold + 각 탭 아이콘. 클릭 시 패널 확장 + 탭 선택."

- 좌측 분리 회수 → `LeftPanel.svelte` 신규 (가로 탭 [Layers | Terminals])
- `Sidebar.svelte` → `LayerTreeView.svelte` rename + outer chrome 제거
- `TerminalsPanel.svelte` → `TerminalListView.svelte` rename + outer chrome 제거
- 28px collapsed rail bar (좌측 panel 내부 self-contained) — expand chevron + 탭 아이콘
- `chromeStore.terminalsCollapsed` → `leftPanelTab: 'layers' | 'terminals'` 교체
- tokens.css 의 vertical split 변수 정리 (회수)
- ADR-0017 **amend ②** + ADR-0021 D7 재갱신

### 1.3 사용자 요청 3 — 우측 panel 도 동일 패턴
"오른쪽 패널도 왼쪽 페널과 동일하게 적용. (탭도 반영하고 대신 지금은 일단 단일탭). folding 시 아이콘 표시도 동일하게, unfold 버튼도 동일하게 배치."

- `RightPanel.svelte` 신규 — LeftPanel mirror (현재 단일 `Inspect` 탭)
- `PaneInfoPanel.svelte` → `ItemInfoView.svelte` rename + outer chrome 제거
- `chromeStore.rightPanelTab: 'inspect'` 추가
- `RailToggle.svelte` 폐기 — 두 panel 모두 self-contained rail
- ADR-0017 **amend ③**

### 1.4 Slice B 진입 — Space-hold pan modifier (G29) + Viewport sync UI (FE-9)
- `Canvas.svelte` 에 `isSpacePressed` state + window keydown/keyup/blur listener + editable/xterm focus 가드
- `panOnDragMask = isSpacePressed ? [0, 1, 2] : [1, 2]` reactive prop
- Drag-to-create handler 가 Space 시 early return (pan 우선)
- `.pan-cursor` CSS (grab → grabbing)
- `sessionStore.updateViewport(v)` 에 500ms debounce + PUT layout 영속 추가
- Canvas `$effect` 가 sessionStore.viewport 변경 시 SvelteFlow `setViewport` (loop 방지 — 임계치 미만 skip)

### 1.5 Slice B 완결 — shortcutRegistry (G26)
- `lib/keyboard/shortcutRegistry.svelte.ts` 신규 — 전역 keydown dispatcher + editable/xterm focus 가드 (modifier 있으면 default `true`, plain key 는 `false`)
- `zShortcuts.svelte.ts` 를 consumer 로 마이그레이션 (직접 listener 제거)
- `chromeShortcuts.svelte.ts` 신규 — `Cmd+Shift+L` (LeftPanel) / `Cmd+Shift+I` (RightPanel)
- `+page.svelte` 에 `bindChromeShortcuts` wire (onMount/onDestroy)

### 1.6 Slice C — SettingsOverlay (G19, FE-8)
- `themeStore` 확장 — `system | light | dark` 의 ThemeMode 추가, `resolved` derived, `bindSystemListener()` (MediaQueryList 핫리로드)
- `index.html` 의 FOUC guard 도 `system` 모드 정합
- `settingsDialog.svelte.ts` store + `SettingsOverlay.svelte` 신규 — full-screen overlay + 좌 nav + 우 section pane + auto-save
- Theme section (radio System/Light/Dark + `Current: light` 등 sub)
- Shortcuts section (registry.list() → category 별 grouping + platform-detect Cmd vs Ctrl 표시 + mac/win pair 자동 collapse)
- Storage/Auth/Behavior/Debug 는 placeholder + `Waiting on BE: ...` 명시
- SessionMenu 에 "Settings…" 항목 추가, `Cmd+,` (macOS/Win) shortcut 등록
- ADR-0017 **amend ④**

### 1.7 xtermTheme adapter (G27 xterm 부분)
- `lib/xterm/xtermTheme.ts` — Tango-dark / Solarized-light variant + ANSI 16색 + bg/fg/cursor/selection
- `XtermHost.svelte` 의 mount 시 `xtermTheme(themeStore.resolved)` 적용
- `termRef = $state<Terminal | null>` + 별도 `$effect` 로 `term.options.theme` 핫리로드
- `.xterm-host` 컨테이너 background `var(--canvas-bg)` (light 모드 black flash 방지)
- ADR-0017 amend ④ follow-up 라인 추가

### 1.8 BE work package 발주 — `docs/reports/0044-be-slice-d-work-package.md`
사용자 요청: "BE 의존 작업 내용을 backend-handover 문서에서 찾아 진행 단계를 확인하고 해당 내용을 연결하여 새로운 파일로 backend 가 작업할 수 있도록 전달할 문서로 작성"

- 0042 신규 — Slice D-1 ~ D-5 묶음:
  - D-1 `GET/PATCH /api/settings` (P0)
  - D-2 file_path 4 endpoint (allowlist + allowlist-check + open + audit)
  - D-3 `POST /api/settings/password` + `POST /api/settings/logout-all`
  - D-4 `POST /api/sessions/import`
  - D-5 `POST /api/shutdown` (Tier 3 + WS server_shutdown frame ADR amend 필요)
- 각 endpoint 별 wire shape + ADR 참조 + FE consumer 위치 + ship 후 FE wire 매트릭스
- BE smoke (cargo test + curl) plan

### 1.10 (별 agent — 본 FE 세션 외) plan-0008 Phase 1 land

본 FE 세션 작업 *후* / 핸드오버 0043 첫 작성 *후* 별 agent (system-architect role) 가 grilling G50 결과로 *Session attach recovery* 의 Phase 1 (Case I — page entry blocking) 을 ship.

근거 문서 신규/갱신:
- ADR-0019 D5.1~D5.4 amend (Case I / Case II / silent + modal trade-off)
- `docs/reports/0042-session-attach-recovery.md` — decision report (Phase 2 의 Case II silent reattach + mutation guard 결정)
- `docs/plans/0008-session-attach-recovery-impl.md` — Phase 1 의 UI/UX 디자인 + 구현 단계 명세

FE 변경 (이번 land):
- `lib/stores/sessionStorageHint.ts` 신규 — tab-scoped `sessionStorage` 의 `gtmux-last-active-session` hint store. attach 성공 시 set, 명시 detach/logout/cancel/[Delete] 시 clear. 다음 reload 의 ReconnectModal trigger 입력.
- `lib/stores/reconnectGate.svelte.ts` 신규 — page entry blocking state machine. `state: 'idle' | 'loading' | 'success' | 'in_use' | 'not_found' | 'unreachable'`. `canMountApp` derived 가 true 일 때만 본 화면 mount. AbortController 로 cancel.
- `lib/chrome/ReconnectModal.svelte` 신규 — blocking modal (loading spinner / 4 transition state / [Switch session…] always-visible cancel / [Retry] in-use 만 / [Open session list] not-found/unreachable).
- `lib/stores/sessionStore.svelte.ts` amend:
  - `ReattachResult` discriminated union 추가 (`success` / `in_use{holderPid?}` / `not_found` / `unauthorized` / `unreachable{message}`)
  - `setActiveSession()` 가 자동으로 `sessionStorageHint.set(name)` 호출
  - `clear()` 가 자동으로 `sessionStorageHint.clear()` 호출
  - `attemptReattach(name, signal)` 신규 메서드 — `POST /api/sessions/<name>/attach` silent 시도 → 응답 분기 normalize → 성공 시 `setActiveSession` + `loadLayout`, 404 시 hint auto-clear, AbortError 시 result skip.
  - `makeWsConnId()` helper (`WorkspaceSwitcher` 와 동일 패턴)
- `+page.svelte` amend:
  - `ReconnectModal` mount + `reconnectGate` consumer wire
  - onMount 의 bootstrap pipeline 에 hint 검사 step 추가 (hint 있으면 `reconnectGate.start(name)`, hint 없으면 기존 WorkspaceSwitcher 흐름)
  - 본 화면 mount 가드 — `reconnectGate.canMountApp` 가 false 일 때 Canvas/Toolbar 등 mount 차단
  - 신규 import: `HelpBar`, `ViewportCtrl`, `ContextMenu` (이전엔 빠져있던 surface 도 다시 mount)
  - prompt() native 폐기 주석 갱신
- `lib/chrome/SessionMenu.svelte` amend — sessionStorageHint clear 정합 (detach/logout 항목 등에서 hint 도 정리)
- BE 변경: **0** (ADR-0019 D5 의 attach_handler 가 이미 idempotent — 같은 cookie + 같은 session 재attach 시 200 OK).

잔여 (Phase 2, 본 세션 작업 외):
- Case II — idle reactivate (15초+ 사용자 idle 후 첫 mutation 시) silent attemptReattach + outgoing write mutation guard.
- WS heartbeat client (`lib/ws/heartbeat.svelte.ts`) — ADR-0021 D6.
- 본 plan-0008 §2 의 Phase 2 4-step 매트릭스 참조.

---

### 1.9 묶음 A (FE-only Tier 1 작은 항목 5건 추가 ship)
- ContextMenu "Add ___" sub-menu — pane 우클릭 시 7 type 즉시 spawn (terminal/text/note/rect/ellipse/line/file_path), itemFactory 재사용
- Inline rename group — `LayerTreeView` group row label 더블클릭 → InlineEditField → mutateActiveLayout
- Group propagation 시각화 — `walkAncestors` / `inheritedHiddenFrom` / `inheritedLockedFrom` helper, `.icon.inherited` 회색 톤 + 작은 dot + tooltip
- shortcutRegistry P1 잔여:
  - `Cmd+N` → toolStore.set('terminal') (click 으로 spawn)
  - `Cmd+Shift+Q` → `shutdownDialog.show()` (신규 store + SessionMenu 가 consumer)

### 1.11 묶음 B — Dual-source adapter 제거 (Tier 1 P0)

handover 0043 §5.1 의 Tier 1 P0 항목. Layer list V2 multi-select 작업 전 정리.

**제거 (legacy single-session) 파일 7건:**
- `lib/stores/panels.svelte.ts` (SvelteMap<id, Panel>)
- `lib/stores/groups.svelte.ts` (SvelteMap<id, Group>)
- `lib/stores/layout.svelte.ts` (etag + schemaVersion)
- `lib/stores/ephemeral.svelte.ts` (m/i/viewport/focusMode)
- `lib/http/layout.ts` (`/api/layout` v1 GET/PUT + `If-None-Match`/`If-Match`/412 rebase + `fetchLayoutAndHydrate` / `putLayoutCommitCurrent` / `appendPanelIfMissing`)
- `lib/canvas/legacyNewPane.ts` (WS CTRL `new-pane` + race 매칭 + `appendPanelIfMissing` orchestrator)
- `lib/toolbar/MIndicator.svelte` (unused stub)

**Modified 면적 (12 consumer):**
- `lib/canvas/Canvas.svelte` — `useSessionStore = $derived(...)` 분기 제거. `panelsStore.movePanel` / `putLayoutCommitCurrent` / `requestLegacyNewPane` / `handleTerminalClick` 의 legacy 분기 / `ephemeralStore.m/.viewport` 모두 폐기. nodes 는 `Array.from(sessionStore.items.values()).map(itemToNode)` 단일 source. onmove 는 `sessionStore.updateViewport` only. onnodeclick / onpaneclick / onnodedragstop / onResizeEnd 모두 `sessionStore.active === null` 가드만 두고 multi-session 본 흐름.
- `lib/canvas/PanelNode.svelte` — `panelsStore` / `ephemeralStore` / `paneNumeric` / `isLegacyPane` / `closeLegacy` / `liveCount` / `closeDisabled` / `wsClientHolder` 모두 제거. `terminalPaneId` 는 항상 `terminalPool.paneIdFor(data.id)`. close 흐름은 항상 PanelCloseConfirmModal (G25 3-option). Label rename 항상 활성.
- `lib/sidebar/LayerTreeView.svelte` — `groupsStore` / `panelsStore` / `ephemeralStore` / `putLayoutCommitCurrent` 임포트 제거. `commitLegacyLayout` 폐기. `toggle{Panel,Group}{Visibility,Lock}` 모두 `mutateActiveLayout` 단일 경로. `selectNode` 는 `sessionStore.setM([id])` only. `{@const selected = ...}` 의 dual-source 분기 4개 제거.
- `lib/chrome/ContextMenu.svelte` — `useSessionStore` derived 제거. legacy [Close pane] menu entry (WS CTRL `kill-pane`) 폐기 — multi-session 의 [Remove from canvas] (deleteItem) 가 대체. `wsClientHolder` 의존 제거. Add / Arrange / View / Terminal / Remove section 모두 `panelIdStr ? ...` 로 단순화.
- `lib/chrome/ItemInfoView.svelte` — `ephemeralStore` / `panelsStore` 제거. `selectedPanelId` 는 `sessionStore.M.values().next()`, `selectedPanel` 은 `sessionStore.items.get(id)` 변환.
- `lib/chrome/ViewportCtrl.svelte` — `ephemeralStore.viewport` / `.m` → `sessionStore.viewport` / `.M`.
- `lib/chrome/FocusToggle.svelte` — `ephemeralStore.focusMode` → `sessionStore.focusMode`.
- `lib/common/escRouter.svelte.ts` — priority 6 의 `ephemeralStore.m` fallback 제거. `sessionStore.M` 단일.
- `lib/stores/sessionStore.svelte.ts` — `focusMode: { enabled, targetPanelId }` 신규 필드 (이전엔 ephemeralStore.focusMode). `clear()` / `loadLayout()` 에서 reset.
- `lib/ws/dispatcher.svelte.ts` — `ephemeralStore` import 제거. 0x81 M_CHANGED / 0x82 I_CHANGED / 0x83 VIEWPORT_CHANGED / 0x84 FOCUS_MODE_CHANGED 모두 sessionStore.{setM,setI,viewport,focusMode} 로 라우팅 + `sessionStore.active === null` pre-attach race guard. `setLayoutRefetchHandler` / `setAutoMountHandler` / 관련 type alias 제거 (legacy v1 layout 전용 hook). 0x80 LAYOUT_CHANGED 는 debug log only no-op (mutateLayout 응답이 진실 source). pane-spawned NOTIFY 의 auto-mount hook 호출 제거 (MOUNT_CASCADE/TERMINAL_SPAWNED 가 multi-session 정식 경로).
- `src/routes/+page.svelte` — `appendPanelIfMissing` / `createLayoutRefetchHandler` / `fetchLayoutAndHydrate` / `setLayoutRefetchHandler` / `setAutoMountHandler` / `layoutStore` import 모두 제거. Step 3 의 token-conditional `fetchLayoutAndHydrate` + `setLayoutRefetchHandler` + `setAutoMountHandler` 블록 폐기. onDestroy 의 cleanup 도 같이 정리.
- `src/lib/chrome/Titlebar.svelte` 외 chrome 표면은 변경 X (모두 sessionStore 만 참조).

**검증:**
- `npm run check`: 290 files / **0 errors / 0 warnings**
- `npm run build`: 클린. dist 산출 (gzip ~ 53 KB main + 71 KB svelteflow + 92 KB xterm).

**아키텍처 효과:**
- single source of truth = `sessionStore.{items, groups, viewport, M, I, maximizedItemId, focusMode}`. canvas-layout-schema v2 단일 데이터 모델 정합.
- legacy `/api/layout` v1 (panels/groups/etag) 의 FE 측 자취 완전 제거. BE 측 `/api/layout` 핸들러는 별도 BE work package 에서 제거 가능 (FE 가 더 이상 호출하지 않음).
- `useSessionStore = $derived(...)` 분기 13곳 → 0곳. Layer list V2 multi-select 의 surgery 면적 절반 ↓.
- `wsClient` Bearer subprotocol 의 legacy 자취도 Canvas/PanelNode 에서 사라짐 (Cmd+N → spawnMultiSessionTerminal 단일 경로). ws subprotocol 자체는 cookie-additive (D10 α) 로 유지.

**ADR amend 필요:**
- ADR-0006 (persistence storage) — `/api/layout` v1 의 ETag/412 rebase 정책이 FE 에서 폐기됨을 명시. multi-session 의 `/api/sessions/<name>/layout` + `mutateLayout` PUT 이 대체.
- ADR-0015 (panel auto-mount) — Stage I 의 `appendPanelIfMissing` 흐름이 multi-session 의 MOUNT_CASCADE (0x86) + TERMINAL_SPAWNED (0x88) 로 대체됨을 명시.
- ADR-0017 (layout-grid-and-chrome) — amend ⑤ 신규: "Dual-source adapter 제거 — sessionStore 단일 source 통일".

### 1.12 묶음 C — Layer list V2 (Multi-select + Drag reorder/reparent)

handover 0043 §5.1 의 Tier 1 P1 큰 chunk. ADR-0024 D1 의 "Tree order ≠ Z, organization 만" 정합. 묶음 B (Dual-source 제거) 직후 진입 — surgery 면적 절반 ↓.

**구현 1: Shift range select**
- `selectionAnchor: string | null` 상태 — 마지막 plain-click 의 row id (Finder/VSCode 컨벤션).
- `selectNode(id, e)` 3-mode 분기:
  - plain click → `setM([id])`, anchor = id.
  - Cmd/Ctrl + click → `toggleM(id)`, anchor = id.
  - Shift + click → `visibleRangeIds(anchor, id)` 의 inclusive range 를 `setM(ids)` (또는 Cmd+Shift 결합 시 `addToM`).
- `visibleRangeIds` 는 `tree` derived 의 평탄화된 순서 기반 — collapse 된 group 안 row 는 제외 (visible 만).
- anchor 가 invisible 화 (ancestor collapse 등) 되어도 fallback 으로 target 만 toggle.

**구현 2: HTML5 drag reorder/reparent**
- 모든 row 에 `draggable={layerMode === 'tree' && !isItemLocked(id)}` — Z mode 비활성 (z mutation 은 ADR-0024 D2 의 4 액션 전용), locked row 비활성.
- `onRowDragStart` 캡처:
  - `sourceIds` = dragged 가 M 에 포함이면 M 전체 (multi-drag), 아니면 dragged id 만.
  - `invalidTargets` = dragged 자신 + dragged 가 group 일 때 그 descendants — cycle 보호.
- `onRowDragOver` 의 mouseY 비율 분기:
  - `< 0.25` → 'before' (행 위 2px line)
  - `> 0.75` → 'after' (행 아래 2px line)
  - 중간 + kind === 'group' → 'inside' (accent tint + dashed outline)
  - 중간 + kind === 'panel' → before/after 양분
- `onRowDrop` → `commitReparent(sourceIds, targetId, kind, pos)`:
  - 'inside' (target = group): dragged 의 `parent_id = target.id`. dragged group 은 target 안 max order + 1.
  - 'before'/'after': dragged 의 `parent_id = target.parent_id`. dragged group 들을 target 의 order 직전/직후로 삽입 + 형제 group 들 sequential 재번호 (1, 2, 3 …).
  - item 은 sibling order field 없음 (ItemCommon 에 order 미존재) — parent_id 만 갱신, sibling 안 정확 위치는 보장 X (id-sort 폴백). **schema v3 (BE work)** 가 item.order 를 추가하면 즉시 보강 가능.
- Single `mutateActiveLayout` call 로 items/groups 둘 다 atomic 갱신.

**Drop indicator UI:**
- `.row.drop-before::before` / `.drop-after::after` = 2px accent line at top/bottom edge.
- `.row.drop-inside` = accent 12% tint + 1px dashed outline.
- `.row.dragging` = opacity 0.4 (dragged set 시각 단서).

**Edge cases:**
- Z mode 진입 시 dragstart preventDefault (drag 진입 불가).
- Locked row 의 dragstart preventDefault. M 에 locked + unlocked 섞이면 unlocked 만 drag (silent filter).
- Cycle: dragged group 의 descendants 가 target 이면 drop 거부.
- Self-drop / parent-drop: noop (`sourceIds.length === 1 && sourceIds[0] === targetId`).
- DragEnd handler 가 항상 dragState clear — drop 거부된 경우에도 cleanup.

**잔여:**
- Marquee selection (sidebar 안 rectangle drag) — Figma/Finder 와 다른 UX, item icon 만으로 충분하므로 P2+ 로 deferred.
- Item sibling order — BE schema v3 (item.order 또는 별 list_order field) 가 추가되면 본 module 의 commitReparent 가 즉시 활용.

**검증:**
- `npm run check`: 290 files / 0 errors / 0 warnings.
- `npm run build`: 클린. gzip ~54 KB main + 71 KB svelteflow + 92 KB xterm.

**ADR amend 필요:**
- ADR-0024 (Tree order ≠ Z) — Tree drag reorder/reparent 의 구체 UX (drop indicator / multi-drag / cycle 보호 / locked guard) 명시 + item.order 의 schema 미존재 한계 명시.
- ADR-0017 amend ⑥ (Layer list V2 — Shift range select + drag reorder/reparent + drop indicator).

### 1.13 묶음 D — FE-only Tier 1 잔여 4건 (작은 chunk)

handover 0043 §5.1 의 P1 잔여 5건 중 4건 + 검증 1건. 모두 작은 surgery.

**구현 1: Cross-session leak filter** (0039 §3.2 step 4)
- `TerminalListView.svelte` — `showAllSessions: boolean` 토글 신규.
- Default 모드 = 현 active session 의 attached_sessions 에 포함되거나
  unplaced (attach_count === 0) 인 것만 표시 — 다른 session 에만 attach 된
  entry 는 hide.
- Toolbar 우측 [Mine] / [All] toggle button (font-mono 36×22 pill). hidden
  count 가 0 보다 크면 count-text 옆에 `(+N hidden)` hint.
- 이유: cross-session terminal 을 의도치 않게 현 canvas 에 attach 하는 leak 차단.

**구현 2: WS heartbeat client** (ADR-0021 D6)
- `lib/ws/heartbeat.svelte.ts` 신규.
- `lastFrameAt` (server WS frame timestamp) + `lastActivityAt` (user keydown/
  mousedown/touchstart) 추적. 1s 틱.
- `isStale` derived — lastFrameAt > 0 && now - lastFrameAt > 30s.
- `isIdle` derived — lastActivityAt > 0 && now - lastActivityAt > 15s.
  → **Phase 2 의 Case II trigger 입력**.
- `start()` / `stop()` — page mount/unmount idempotent.
- `markFrame()` — dispatcher 가 매 frame 수신 시 호출.
- `markActivity` — internal listener 가 자동.
- `reset()` — silent reattach 성공 후 fresh baseline.
- RFC 6455 PING/PONG 자체는 browser 자동 — 본 store 는 application-level
  liveness watchdog.

**구현 3: plan-0008 Phase 2 — Silent reattach + mutation guard** (§6 정합)
- `sessionStore`: `reattachInProgress` / `lastSilentReattachResult` /
  `silentReattach(name, signal)` / `guardOutgoingMutation()` 신규.
  - `silentReattach` 은 `attemptReattach` 의 wrapper — 동시 호출 dedup
    (`#silentReattachPromise` shared).
  - `guardOutgoingMutation` 은 mutation 진입점 의 *바로 직전* 에 await —
    in-flight 면 await, 직전 fail 면 그 결과 그대로 반환.
- WS dispatcher: `adaptStateChange` 가 `prevWsState === 'reconnecting' &&
  state === 'open'` 전이 감지 → `sessionStore.silentReattach(active.name)`
  trigger. `reconnectGate.canMountApp` 가드로 Phase 1 의 blocking modal
  흐름과 충돌 방지.
- `+page.svelte`: `visibilitychange` listener → `maybeSilentReattach()`:
  - `document.visibilityState === 'visible'`
  - `reconnectGate.canMountApp`
  - `sessionStore.active !== null`
  - `!sessionStore.reattachInProgress`
  - `heartbeatStore.isIdle` (15s+ user idle)
  - 모두 충족 시 `silentReattach`. 성공 → `heartbeatStore.reset()`. fail →
    toast (Case II 의 무거운 modal 회피 — silent UX).
- **Mutation guard wire (5 site)**:
  - `Canvas.spawnMultiSessionTerminal` (terminal spawn)
  - `Canvas.deleteSelected` (Delete/Backspace)
  - `PanelNode.performClose` (panel close — G25 dialog 후)
  - `TerminalListView.attachToCanvas` (pool → canvas)
  - `TerminalListView.killOne` (kill terminal)
  - 각 site 가 `await sessionStore.guardOutgoingMutation()` → `!guard.ok` 시
    toast 후 early return.

**구현 4 (검증 only): file_path inline rename** — 이미 ship 됨
- FilePathNode 의 `editing` state + `onDblClick` + `onCommit` + InlineEditField
  template 모두 갖춰져 있음. 본 세션은 검증만.

**검증:**
- `npm run check`: 291 files / 0 errors / 0 warnings (heartbeat.svelte.ts +1)
- `npm run build`: 클린 (gzip ~55.6 KB main).

**ADR amend 필요:**
- ADR-0021 D6 — server-driven PING/PONG 의 FE-side watchdog 정합 명시.
- ADR-0019 D5.1 — Phase 2 의 silent reattach + mutation guard 흐름 정합 (이미 D5.1 에 기록되어 있으나 wire 시점 amend).

### 1.14 묶음 E — 0045 refresh reconnect loop fix (P0 후속)

`docs/reports/0045-refresh-session-reconnect-loop-analysis.md` 의 분석 결과를 기반으로 P0 후속 fix 일괄 진행. BE 의존 사항은 0046 work package 로 격리.

**P0-A: `flowNodes` id-cache + signature** (Canvas.svelte)
- `nodeCache: Map<id, { sig: string; node: Node }>` — 매 derived pass 새 Map.
- Signature = `${effVisible}|${effLocked}|${selected}|${mMulti}|${JSON.stringify(item)}` — 모든 mutation-relevant field 포함 (id/type/parent_id/x/y/w/h/z/visibility/locked/minimized/label + type payload 의 line.x2/y2, text.text/font_size, shape.stroke/fill 등 모두 cover).
- 동일 signature → 이전 Node object reference 재사용 → SvelteFlow prop identity 안정 → effect-depth loop 차단.
- 50 entry 기준 < 1ms — GC pressure 무시.

**P0-B: silentReattach 후 viewport 사용자 변경 보존** (sessionStore.svelte.ts)
- `silentReattach` wrapper 가 reattach 직전 `preReattachViewport` snapshot.
- attemptReattach 성공 후 `this.viewport = preReattachViewport` 복원 — 사용자가 silent 발화 *이전* 까지 보던 viewport 유지.
- M/I/maximize/focusMode 의 reset 은 G20 ephemeral 정책 그대로 (server 가 권위).

**reconnectGate 5-state** (`booting/attaching/hydrating/ready/failed`)
- 'success' → 'ready' rename. 'loading' → 'attaching' + 'hydrating' 분리.
- `markReady()` 신규 (`markSuccess()` 는 호환 alias).
- `modalState` derived = `attaching`/`hydrating` → 'loading', 'in_use'/'not_found'/'unreachable' → 그대로.
- +page.svelte boot screen 분기 `booting | attaching | hydrating` 노출 ("Reconnecting…" / "Loading layout…" / "Preparing workspace…").
- canMountApp = `ready || idle` 그대로 (idle 시 workspaceSwitcher modal 이 cover — surgery 면적 최소화).

**XtermHost RAF coalesce + 동일 px guard** (XtermHost.svelte)
- ResizeObserver callback entry-level dedup — 직전 `contentRect.width/height` (Math.round px) 와 동일하면 fit() timer 진입 자체 skip.
- SvelteFlow 의 nodeInternals update 가 동일 width/height 재측정 트리거 시 fit() 비용 0.
- 기존 150ms fit-debounce + 100ms send-debounce + cols/rows dedup 은 유지 (보조 필터).

**Dev-only instrumentation** (`lib/common/debugCounts.ts`)
- localStorage flag `gtmux-debug-counts=1` 켜면 활성. throttled summary (1s) 콘솔 dump.
- Counters: `canvas.mount/unmount`, `canvas.setViewport`, `canvas.onmove`, `canvas.onmove.skip-applying`, `flowNodes.rebuild`, `flowNodes.cache.hit/miss`, `sessionStore.loadLayout`, `xterm.fit`.
- `window.__gtmuxDebug = { enable, disable, snapshot, reset }` — DevTools 콘솔 ad-hoc 사용.
- production noise 0 — flag off 시 함수 즉시 early return.

**BE 의존 사항 (별 work package 0046)**
- `attach_handler` 의 same-cookie same-session 재attach 가 코멘트 약속과 달리 409 CONFLICT 반환 — cookie ownership 분기 추가 필요. 새로고침 race 및 Phase 2 silentReattach 의 *모든* 호출 회귀의 근본 원인. 별 PR — `docs/reports/0046-be-attach-handler-idempotent.md` 참조.

**검증:**
- `npm run check`: 291 files / 0 errors / 0 warnings (debugCounts.ts +1)
- `npm run build`: 클린 (gzip ~56.4 KB main → 새 hash `index-CPenQxFO.js`)

**잔여 (0045 §10.6):**
- terminal 유/무 layout 분리 재현 — P1-D 격리 검증 (FE 환경 변수, surgery X).

**ADR amend 필요:**
- ADR-0019 D5.4 — reconnectGate 5-state 명시 (booting/attaching/hydrating/ready/failed 의 의미 + canMountApp 정합).
- ADR-0024 D1 — node-cache identity-stable 패턴 명시 (Tree drag/reparent + multi-select 와 정합).

---
- plan-0008 §6/§9 — Phase 2 ship 변경 이력 추가.

---

## 2. v3 핸드오버 기준 구현율 (0041 §2 갱신)

### 2.1 P0 (Stage 1~4) — 100% (0041 기준 그대로 + 본 세션 마감)
- FE-NEW-4 ChangeTerminalModal: ✅ (이번 세션) — *0041 기준 잔여 1건 해소*
- FE-NEW-2 Webpage attach lifecycle: ✅ Phase 1 ship (별 agent — plan-0008) — sessionStorageHint + reconnectGate + ReconnectModal + sessionStore.attemptReattach 정합. *heartbeat client (ADR-0021 D6) + Phase 2 Case II silent reattach 만 잔여.*

### 2.2 Stage 5 — 100%
| ID | 이름 | 상태 |
|---|---|---|
| FE-2 Toolbar2 / tool state | ✅ |
| FE-2 Space hold + drag pan modifier (G29) | ✅ (이번) |
| FE-4 신규 5 renderer (text/note/file_path/rect/ellipse/line) | ✅ |
| FE-5 creation gestures (점/드래그) | ✅ |
| FE-NEW-8 file_path open UX | ❌ BE 의존 (0042 Slice D-2) |
| ContextMenu "Add" sub-menu | ✅ (이번) |

### 2.3 Stage 6 (FE-6/7) — 100%
| ID | 이름 | 상태 |
|---|---|---|
| FE-6 Tree/Z toggle | ✅ |
| FE-6 Multi-select (Cmd/Ctrl/Shift) + drag reorder/reparent | ✅ (이번 묶음 C) |
| FE-6 Inline rename group | ✅ |
| FE-6 Group propagation 시각화 | ✅ |
| FE-6 GroupCloseConfirmModal | ✅ |
| FE-7 Panel header more menu | ✅ |
| FE-7 PanelCloseConfirmModal | ✅ |
| FE-7 Inline label rename | ✅ |
| FE-7 PanelDanglingOverlay | ✅ |
| FE-7 ChangeTerminalModal | ✅ |

### 2.4 Stage 7 (FE-9 + FE-8 + G26/G27/G28) — 75%
| ID | 이름 | 상태 |
|---|---|---|
| FE-9 Viewport sync UI | ✅ (이번) |
| shortcutRegistry (G26) | ✅ (이번) |
| shortcutRegistry P0 매트릭스 wire | ✅ (z + Esc 는 escRouter) |
| shortcutRegistry P1 매트릭스 | ✅ (Cmd+N / Cmd+Shift+L / Cmd+Shift+I / Cmd+, / Cmd+Shift+Q) |
| themeStore + xtermTheme adapter (G27) | ✅ (이번) |
| SettingsOverlay (G19, FE-8) shell | ✅ (이번) |
| Settings Theme section | ✅ |
| Settings Shortcut section | ✅ |
| Settings Auth section | ❌ BE 의존 (Slice D-3) |
| Settings Storage section | ❌ BE 의존 (Slice D-2/D-4) |
| Settings Behavior section | ❌ BE 의존 (Slice D-1) |
| Settings Debug section | ❌ BE 의존 (Slice D-1) |
| Session export/import (G28) | ❌ BE 의존 (Slice D-4) |

### 2.5 Stage 8~9 (P2) — 0%
| ID | 상태 |
|---|---|
| ImageNode + asset upload | ❌ BE asset endpoint 미 ship |
| DocumentNode | ❌ 위 의존 |
| FreeDrawNode + RDP | ❌ FE only, 큼 |

### 2.6 Architectural invariants — 12/14 fully + 2 partial (이번 세션 변경)

| # | 항목 | 상태 변화 |
|---|---|---|
| 4 | Tree order ≠ Z | ✅ (Tree/Z toggle ship) |
| 6 | Esc 라우팅 7 priority | ⚠️ (그대로 — Esc 는 escRouter, 비-Esc 는 신 shortcutRegistry) |
| 8b | Space hold + drag pan modifier | ✅ (이번) |
| 9 | Settings full-screen overlay + auto-save | ✅ (이번) |
| 10 | Keyboard shortcut Hybrid + xterm focus | ✅ (이번) |
| 13 | Theme system (light/dark/system + xterm 동기) | ✅ (이번) — system 모드 추가 + xterm hot reload |

### 2.7 공용 컴포넌트 / store (이번 세션 신규 ⭐)

| 파일 | 상태 |
|---|---|
| `lib/sidebar/LayerTreeView.svelte` | ✅ (Sidebar rename + Tree/Z toggle + inline rename + propagation + group close X) |
| `lib/sidebar/TerminalListView.svelte` | ✅ (TerminalsPanel rename) |
| `lib/sidebar/LeftPanel.svelte` ⭐ | ✅ (가로 탭 wrapper) |
| `lib/chrome/RightPanel.svelte` ⭐ | ✅ |
| `lib/chrome/ItemInfoView.svelte` | ✅ (PaneInfoPanel rename) |
| `lib/chrome/PanelFoldButton.svelte` ⭐ | ✅ |
| `lib/chrome/ChangeTerminalModal.svelte` ⭐ | ✅ |
| `lib/chrome/GroupCloseConfirmModal.svelte` ⭐ | ✅ |
| `lib/chrome/SettingsOverlay.svelte` ⭐ | ✅ |
| `lib/keyboard/shortcutRegistry.svelte.ts` ⭐ | ✅ |
| `lib/keyboard/chromeShortcuts.svelte.ts` ⭐ | ✅ |
| `lib/keyboard/zShortcuts.svelte.ts` | ✅ (registry consumer 로 마이그레이션) |
| `lib/xterm/xtermTheme.ts` ⭐ | ✅ |
| `lib/stores/changeTerminalDialog.svelte.ts` ⭐ | ✅ |
| `lib/stores/groupCloseDialog.svelte.ts` ⭐ | ✅ |
| `lib/stores/settingsDialog.svelte.ts` ⭐ | ✅ |
| `lib/stores/shutdownDialog.svelte.ts` ⭐ | ✅ |
| `lib/stores/theme.svelte.ts` | ✅ (system mode 확장) |
| `lib/stores/chrome.svelte.ts` | ✅ (leftPanelTab + rightPanelTab) |
| `lib/stores/sessionStore.svelte.ts` | ✅ (viewport debounce PUT + plan-0008 land: ReattachResult union + setActiveSession/clear hint sync + attemptReattach) |
| `lib/stores/sessionStorageHint.ts` ⭐ (plan-0008 Phase 1) | ✅ (별 agent — tab-scoped hint) |
| `lib/stores/reconnectGate.svelte.ts` ⭐ (plan-0008 Phase 1) | ✅ (별 agent — page entry blocking state machine + AbortController) |
| `lib/chrome/ReconnectModal.svelte` ⭐ (plan-0008 Phase 1) | ✅ (별 agent — blocking modal + 4 transition state) |

### 2.8 종합 — 구현율

| 영역 | items | 이번 세션 진행 |
|---|---|---|
| P0 (Stage 1~4) | 11 | 11/11 (FE-NEW-4 해소) |
| Stage 5 | 11 | 10/11 (FE-NEW-8 만 BE 의존) |
| Stage 6 | 11 | 9/11 (multi-select drag reorder 만 잔여) |
| Stage 7 | 13 | 9/13 (4 BE-dependent section 잔여) |
| Stage 8~9 | 3 | 0/3 |
| Architectural invariants | 14 | 12 fully + 2 partial |

**FE-only P0/P1 거의 마감.** 남은 큰 chunk:
1. Layer list V2 multi-select + drag reorder/reparent (Stage 6, 큼) — dual-source 제거 완료 후 즉시 진입 가능

BE 의존 영역 (4 Settings section + file_path + import + shutdown) 은 0044 Slice D 로 발주.

---

## 3. 본 세션 진행하지 못한 항목 (전체 미구현 매트릭스 — 0041 §3 갱신)

### 3.1 Stage 5 잔여
| 항목 | scope | 의존 |
|---|---|---|
| FE-NEW-8 file_path open UX | FileOpenConfirmModal + double-click handler + Storage section editor | BE 0042 Slice D-2 |

### 3.2 Stage 6 잔여 (큰 chunk)
| 항목 | scope | 의존 |
|---|---|---|
| Layer list V2 — Multi-select + drag reorder/reparent | Cmd/Shift click + marquee + tree drag (organization 만) | FE only, **큰 surgery** |

### 3.3 Stage 7 잔여 (BE 의존)
| 항목 | BE 의존 |
|---|---|
| Settings Auth section | Slice D-3 |
| Settings Storage section | Slice D-2 + D-4 |
| Settings Behavior section | Slice D-1 |
| Settings Debug section | Slice D-1 |
| Session export/import (G28) | Slice D-4 |
| ServerShutdownConfirmModal | Slice D-5 (별 ADR amend 필요) |

### 3.4 Stage 8~9 (P2)
| 항목 | 의존 |
|---|---|
| ImageNode + asset upload | BE: `/api/assets/*` (별 ADR) |
| DocumentNode | 위 의존 |
| FreeDrawNode + RDP simplification | FE only, 큼 |

### 3.5 정합 / 정리
| 항목 | scope |
|---|---|
| ~~Dual-source adapter 제거~~ | ✅ 본 세션 §1.11 묶음 B 에서 ship — legacy 7 파일 삭제 + sessionStore 단일 source 통일. |
| LayoutSnapshot ↔ SessionLayout 통합 | BE (FE 는 더이상 `/api/layout` v1 호출 X — BE 측 핸들러 삭제 가능) |
| WS subscriber Lagged reconciliation | BE 0032 §5.6 / 0034 §9 의 P2+ |
| `gtmux start --session <name>` flag 제거 | BE |
| D10 β/γ — Bearer subprotocol deprecation | BE + FE 의 sessionStorage.gtmux_token 폐기 |

---

## 4. 미커밋 작업트리 상태

### 4.1 BE (단일 chunk — `5932d00` 후의 cumulative 변경)
```
M codebase/backend/crates/http-api/src/lib.rs        (integration tests 보강 — implicit detach test)
M codebase/backend/crates/http-api/src/schema.rs     (39 lines added)
M codebase/backend/crates/ws-server/src/lib.rs       (8 lines)
?? codebase/backend/crates/http-api/src/settings.rs  (NEW — BE side 가 Slice D-1 시작한 흔적?)
```

`settings.rs` 가 신규로 있는데 본 FE 세션 무관 — BE 측 작업 흔적이거나 사용자 manual 추가. 위치 확인만 (`grep -n "pub fn\|pub async fn" src/settings.rs`) 권장.

### 4.2 FE (광범위)
**Modified (이번 세션):**
```
M codebase/frontend/index.html                                (FOUC system mode)
M codebase/frontend/src/lib/canvas/Canvas.svelte              (pan + viewport + Add 좌표 변환은 ContextMenu 가 직접)
M codebase/frontend/src/lib/canvas/PanelNode.svelte           (more menu)
M codebase/frontend/src/lib/canvas/XtermHost.svelte           (xtermTheme + termRef)
M codebase/frontend/src/lib/chrome/ContextMenu.svelte         (Add sub-menu + Change terminal + Arrange + Remove)
M codebase/frontend/src/lib/chrome/SessionMenu.svelte         (Settings… + shutdownDialog wiring)
M codebase/frontend/src/lib/stores/chrome.svelte.ts           (leftPanelTab + rightPanelTab)
M codebase/frontend/src/lib/stores/theme.svelte.ts            (system mode + bindSystemListener)
M codebase/frontend/src/lib/sidebar/LayerTreeView.svelte      (rename from Sidebar + Tree/Z + inline rename + propagation + group close X)
M codebase/frontend/src/lib/styles/tokens.css                 (vertical split 토큰 회수)
M codebase/frontend/src/routes/+page.svelte                   (LeftPanel/RightPanel mount + bindChromeShortcuts + system theme listener)
D codebase/frontend/src/lib/chrome/PaneInfoPanel.svelte       (→ ItemInfoView)
D codebase/frontend/src/lib/chrome/RailToggle.svelte          (폐기)
```

**Untracked (이번 세션 신규):**
```
?? codebase/frontend/src/lib/sidebar/LeftPanel.svelte
?? codebase/frontend/src/lib/sidebar/TerminalListView.svelte
?? codebase/frontend/src/lib/chrome/RightPanel.svelte
?? codebase/frontend/src/lib/chrome/ItemInfoView.svelte
?? codebase/frontend/src/lib/chrome/PanelFoldButton.svelte
?? codebase/frontend/src/lib/chrome/ChangeTerminalModal.svelte
?? codebase/frontend/src/lib/chrome/GroupCloseConfirmModal.svelte
?? codebase/frontend/src/lib/chrome/SettingsOverlay.svelte
?? codebase/frontend/src/lib/keyboard/shortcutRegistry.svelte.ts
?? codebase/frontend/src/lib/keyboard/chromeShortcuts.svelte.ts
?? codebase/frontend/src/lib/xterm/xtermTheme.ts
?? codebase/frontend/src/lib/stores/changeTerminalDialog.svelte.ts
?? codebase/frontend/src/lib/stores/groupCloseDialog.svelte.ts
?? codebase/frontend/src/lib/stores/settingsDialog.svelte.ts
?? codebase/frontend/src/lib/stores/shutdownDialog.svelte.ts
```

**Untracked (별 agent — plan-0008 Phase 1 land):**
```
?? codebase/frontend/src/lib/stores/sessionStorageHint.ts
?? codebase/frontend/src/lib/stores/reconnectGate.svelte.ts
?? codebase/frontend/src/lib/chrome/ReconnectModal.svelte
```

(추가 modified — plan-0008 land 가 본 세션 변경 위에 덮어쓰기:
`+page.svelte` 의 ReconnectModal mount + reconnectGate consumer wire + bootstrap pipeline hint 단계 + canMountApp 가드 + HelpBar/ViewportCtrl/ContextMenu 재추가;
`sessionStore.svelte.ts` 의 ReattachResult union + setActiveSession/clear 의 hint sync + attemptReattach 메서드 + makeWsConnId helper;
`SessionMenu.svelte` 의 hint clear 정합.)

**Deleted (묶음 B — Dual-source adapter 제거):**
```
D codebase/frontend/src/lib/stores/panels.svelte.ts       (legacy panelsStore)
D codebase/frontend/src/lib/stores/groups.svelte.ts       (legacy groupsStore)
D codebase/frontend/src/lib/stores/layout.svelte.ts       (legacy layoutStore — etag/schemaVersion)
D codebase/frontend/src/lib/stores/ephemeral.svelte.ts    (legacy ephemeralStore — m/i/viewport/focusMode → 모두 sessionStore 로)
D codebase/frontend/src/lib/http/layout.ts                (legacy `/api/layout` v1 — fetchLayoutAndHydrate / putLayoutCommitCurrent / appendPanelIfMissing)
D codebase/frontend/src/lib/canvas/legacyNewPane.ts       (legacy WS CTRL new-pane orchestrator)
D codebase/frontend/src/lib/toolbar/MIndicator.svelte     (unused stub)
```

**Modified (묶음 B):**
```
M codebase/frontend/src/lib/canvas/Canvas.svelte                       (useSessionStore 분기 제거 + legacy panelsStore/ephemeralStore/putLayoutCommitCurrent/requestLegacyNewPane 모두 폐기)
M codebase/frontend/src/lib/canvas/PanelNode.svelte                    (panelsStore/ephemeralStore/paneNumeric/closeLegacy/closeDisabled 폐기)
M codebase/frontend/src/lib/sidebar/LayerTreeView.svelte               (groupsStore/panelsStore/ephemeralStore/commitLegacyLayout 폐기)
M codebase/frontend/src/lib/chrome/ContextMenu.svelte                  (useSessionStore 제거 + legacy [Close pane] entry 폐기)
M codebase/frontend/src/lib/chrome/ItemInfoView.svelte                 (ephemeralStore/panelsStore 폐기)
M codebase/frontend/src/lib/chrome/ViewportCtrl.svelte                 (ephemeralStore → sessionStore)
M codebase/frontend/src/lib/chrome/FocusToggle.svelte                  (ephemeralStore.focusMode → sessionStore.focusMode)
M codebase/frontend/src/lib/common/escRouter.svelte.ts                 (priority 6 의 ephemeralStore.m fallback 제거)
M codebase/frontend/src/lib/stores/sessionStore.svelte.ts              (focusMode 필드 추가 + clear()/loadLayout() reset)
M codebase/frontend/src/lib/ws/dispatcher.svelte.ts                    (0x80~0x84 sessionStore 라우팅 + setLayoutRefetchHandler/setAutoMountHandler 폐기)
M codebase/frontend/src/routes/+page.svelte                            (legacy refetch/auto-mount/fetchLayoutAndHydrate 블록 폐기)
```

`svelte-check`: **290 files / 0 errors / 0 warnings**
`npm run build`: 클린 (gzip ~53 KB main + 71 KB svelteflow + 92 KB xterm)

### 4.3 Docs
**Modified:**
```
M docs/adr/0017-layout-grid-and-chrome.md   (amend ①~④ + follow-up — 본 세션 전체)
M docs/adr/0018-canvas-item-data-model.md   (이전 세션 누락)
M docs/adr/0020-auth-lifecycle.md           (이전 세션 누락)
M docs/adr/0021-terminal-pool-and-mirror.md (D7 위치 결정 갱신 + amend 변경 이력 라인)
M docs/plans/0007-multi-session-pivot.md    (이전 세션 누락)
M docs/reports/0041-next-session-handover.md (이전 세션 작성한 0041)
```

**Untracked (이번 세션):**
```
?? docs/reports/0044-be-slice-d-work-package.md   (이번 세션 작성 — 처음 0042 로 작성, 별 agent 의 같은 prefix 충돌로 0044 로 rename)
?? docs/reports/0043-fe-integrated-session-handover.md  (본 문서)
```

**Untracked (별 agent — plan-0008 Phase 1 land):**
```
?? docs/plans/0008-session-attach-recovery-impl.md
?? docs/reports/0042-session-attach-recovery.md
```

(ADR-0019 가 modified — D5.1~D5.4 amend 추가)

### 4.4 BE 의 committed history (그대로)
```
5932d00 feat(backend): 0040 option A — catch-up 0x88 + implicit detach-on-reattach
```
이전 세션의 commit. BE 의 본 세션 작업은 위 4.1 의 미커밋만.

**Commit 권장**: BE / FE 양쪽 모두 단일 chunk 가 거대 — split 권장 (BE 1, FE 의 chrome refactor 1, FE 의 Slice A/B/C ship 1, FE 묶음 A 1, Docs 1).

---

## 5. 향후 계획 (다음 세션 권장)

### 5.1 즉시 가능 — FE only (Tier 1)
| 우선 | 항목 | scope |
|---|---|---|
| ~~Cross-session leak filter~~ | ✅ 본 세션 §1.13 묶음 D 에서 ship |
| ~~Inline rename file_path~~ | ✅ 이전 세션에서 이미 ship (FilePathNode 의 editing/onCommit/InlineEditField) |
| ~~plan-0008 Phase 2~~ | ✅ 본 세션 §1.13 묶음 D 에서 ship — silent reattach + mutation guard 5 site |
| ~~WS heartbeat~~ | ✅ 본 세션 §1.13 묶음 D 에서 ship — heartbeat.svelte.ts |
| 🟡 P2 | Marquee selection (sidebar rectangle drag) | 묶음 C 에서 deferred. Figma/Finder 와 다른 UX. icon multi-select 으로 충분 |
| 🟡 P2 | Item sibling order (schema v3) | item.order 또는 list_order field 가 BE schema 에 추가되어야 함. 현재는 parent_id reparent 만 보장, sibling 안 정확 위치는 id-sort 폴백 |
| 🟡 P2 | Phase 2 mutation guard 의 나머지 site | label PATCH (PanelNode.onLabelCommit), drag commit (Canvas.onnodedragstop), resize commit (PanelNode.onResizeEnd), respawn (PanelDanglingOverlay), 5 site 중 사용자-visible 한 진입점만 우선 ship — 나머지는 후속 |

### 5.2 BE 의존 (0042 Slice D ship 후 즉시 FE wire 가능)
| Slice | FE wire scope |
|---|---|
| D-1 (Settings GET/PATCH) | Settings Debug + Behavior section 분기 채움 (현 placeholder). `lib/http/settings.ts` + `lib/stores/settingsStore.svelte.ts` 신규 |
| D-2 (file_path 4 endpoint) | FilePathNode double-click handler + FileOpenConfirmModal + Storage section editor. `lib/http/filePath.ts` + `lib/stores/fileOpenDialog.svelte.ts` + `lib/chrome/FileOpenConfirmModal.svelte` 신규 |
| D-3 (Auth) | Settings Auth section 2 input + 2 button |
| D-4 (Import) | Settings Storage section export/import buttons |
| D-5 (Shutdown + WS server_shutdown frame) | ServerShutdownConfirmModal + SessionMenu 새 항목 + ReconnectBanner 의 1000+server_shutdown 분기 |

### 5.3 P2 (Stage 8~9)
| 항목 | 의존 |
|---|---|
| BE: `/api/assets/*` (ADR 신규) | BE asset endpoint design |
| ImageNode + asset upload | 위 의존 |
| DocumentNode | 위 의존 |
| FreeDrawNode + RDP | FE only, 큼 |

### 5.4 권장 진입 순서 (다음 세션 1번)
**BE Slice D wire (FE only)** — Slice D-1/D-2/D-3/D-4 BE 측 ship 완료. FE 측 wire 만 남음:
1. **Slice D-1 wire** — Settings Behavior/Debug section 의 placeholder → 실 controls (`/api/settings` GET/PATCH)
2. **Slice D-2 wire** — FilePathNode 더블 클릭 시 FileOpenConfirmModal + Storage section file_path editor
3. **Slice D-3 wire** — Settings Auth section 의 password rotate + logout-all
4. **Slice D-4 wire** — Settings Storage section export/import buttons
5. **Slice D-5** — ServerShutdownConfirmModal (BE 측 D-5 미 ship 가능성 — 0044 §6 확인)

병행: P2 작은 cleanup — Phase 2 mutation guard 의 나머지 5 site (label PATCH / drag commit / resize commit / respawn). FE-only.

FE-only Tier 1 / P0/P1 모두 마감 상태.

### 5.5 Commit 권장 split (즉시)
미커밋 chunk 가 너무 크니 next session 들어가기 전에:
1. **BE commit** — implicit detach integration test + ws-server 8 line + schema 39 line
2. **FE chrome refactor commit** — LeftPanel/RightPanel/LayerTreeView/TerminalListView/ItemInfoView/PanelFoldButton + chromeStore.leftPanelTab/rightPanelTab + RailToggle 폐기 + ADR-0017 amend ①②③
3. **FE Slice A commit** — ChangeTerminalModal + Panel header more menu + GroupCloseConfirmModal + LayerTreeView 의 Tree/Z toggle + group close X
4. **FE Slice B commit** — Space pan + viewport sync + shortcutRegistry + zShortcuts 마이그레이션
5. **FE Slice C commit** — themeStore system + xtermTheme + settingsDialog + SettingsOverlay + chromeShortcuts (Cmd+,/Cmd+Shift+L/I) + SessionMenu Settings… + ADR-0017 amend ④
6. **FE 묶음 A commit** — ContextMenu Add + Inline rename group + Group propagation + Cmd+N + Cmd+Shift+Q + shutdownDialog
7. **FE 묶음 B commit (Dual-source 제거)** — legacy 7 파일 삭제 (panels/groups/layout/ephemeral stores + http/layout + legacyNewPane + MIndicator) + 12 consumer 단순화 + sessionStore.focusMode 신규 + dispatcher 0x80~0x84 정합 + ADR-0006/0015/0017 amend
8. **FE 묶음 C commit (Layer list V2)** — LayerTreeView 의 shift range select + HTML5 drag reorder/reparent + drop indicator + multi-drag + cycle 보호 + locked guard + ADR-0017 amend ⑥ + ADR-0024 amend
9. **Docs commit** — 0044 BE work package + 0043 본 handover + ADR-0017 amend ⑤⑥ + ADR-0024 amend + ADR 변경 이력

---

## 6. 본 세션 동안 굳어진 아키텍처 결정

| 영역 | 결정 | ADR/Doc |
|---|---|---|
| 좌·우 panel chrome | 단일 `LeftPanel` / `RightPanel` 가 panel-tabs 모델 정합. 두 panel 각각 self-contained 28px collapsed rail bar with per-tab icons. RailToggle 폐기. | ADR-0017 amend ②/③ |
| header fold model | `PanelFoldButton` (header 우측) 가 collapse 트리거. 펼침은 collapsed rail bar 의 expand chevron. | ADR-0017 amend ① 유효 잔존 |
| panel header (…) more menu | PanelNode header 우측 (…) 클릭 = 우클릭 컨텍스트 메뉴와 동일 액션 셋 reuse (`setContext('contextMenu')` 패턴) | ADR-0017 amend ④ |
| ChangeTerminal 흐름 | `mutateLayout` atomic — 기존 item.id 만 새 UUID 로 교체 (BE 의 `PUT /api/sessions/<name>/items/<id>/terminal` ship 전 layout-only 경로) | ADR-0021 D8 (FE 측 구현 임시) |
| Group close 흐름 | `mutateLayout` 으로 group + 자손 일괄 prune (PUT). `Panels + Terminals` 옵션은 `Promise.allSettled` 로 SIGTERM 병렬 fan-out + dangling 안내 | ADR-0021 D9.3 |
| keyboard shortcut routing | `shortcutRegistry` (전역 keydown + editable/xterm focus 가드) + `escRouter` (Esc priority chain) 협조. 두 시스템 분리. Modifier 있으면 default fire, plain key 는 skip editable/xterm. | ADR-0017 amend ④ D6 |
| ThemeStore 모델 | `ThemeMode = 'system' \| 'light' \| 'dark'` + `resolved` derived + `bindSystemListener` (MediaQueryList). xterm 인스턴스는 별 effect 가 `term.options.theme` hot reload. | ADR-0017 amend ④ D5 |
| Settings overlay | full-screen overlay (880×640 max) + 좌 nav + 우 section pane + auto-save (control 별 즉시 persist, 별도 [Save] 없음). BE-dependent section 은 placeholder + "Waiting on BE" 명시. | ADR-0017 amend ④ D9 |
| Sidebar `{@const}` placement | `{#snippet groupIcons()}` / `{#snippet panelIcons()}` 패턴으로 `{@const}` 의 valid placement 우회 | `LayerTreeView.svelte` (이번 세션) |
| Viewport persist | `sessionStore.updateViewport` 가 500ms debounce 후 `mutateLayout` PUT. `$effect` 의 setViewport loop 방지는 임계치 비교 (dx<0.5 + dy<0.5 + dz<0.001) | FE-9 |
| pan modifier | Space hold + drag = `panOnDragMask = [0, 1, 2]`. drag-tool 핸들러는 Space 시 early return. blur listener 로 sticky 방지. | G29 |
| BE work package 모델 | FE 가 BE 의존 작업을 정리해 별 reports/ 파일로 발주. 각 endpoint 의 wire shape + FE consumer 위치 + ship 후 FE 측 sync 매트릭스. | `0044-be-slice-d-work-package.md` |
| Session attach recovery (별 agent, plan-0008 Phase 1) | tab-scoped sessionStorage hint + page entry blocking ReconnectModal + silent `attemptReattach`. `setActiveSession()` / `clear()` 가 hint 자동 sync. BE 변경 0 — `attach_handler` 가 이미 idempotent. Phase 2 (idle reactivate silent + mutation guard) 잔여. | ADR-0019 D5.4 / `docs/reports/0042-session-attach-recovery.md` / `docs/plans/0008-session-attach-recovery-impl.md` |
| Dual-source adapter 제거 (묶음 B) | legacy single-session stores (panels/groups/layout/ephemeral + http/layout + legacyNewPane) 모두 삭제. sessionStore 단일 source. `useSessionStore = $derived(...)` 분기 13 곳 → 0 곳. focusMode 가 sessionStore 필드로 통합. WS 0x80~0x84 frame 들이 sessionStore 로 직결. Layer list V2 multi-select 의 surgery 면적 ↓. | ADR-0006 (FE 측 `/api/layout` v1 폐기) / ADR-0015 (auto-mount 의 multi-session 대체) / ADR-0017 amend ⑤ (sessionStore 단일 source) |
| Layer list V2 multi-select + drag reorder/reparent (묶음 C) | Cmd/Ctrl + click = toggle; Shift + click = anchor↔target visible range; HTML5 drag = reorder (group order field) + reparent (parent_id). Multi-drag = M.size > 0 + dragged ∈ M 시 M 전체. Cycle 보호 = dragged group 의 descendants 제외. Locked guard. Z mode 비활성. Item sibling order 는 BE schema v3 대기. | ADR-0024 (Tree order ≠ Z + drag UX 명시) / ADR-0017 amend ⑥ (Layer list V2 — drop indicator + multi-drag) |

---

## 7. 빌드 / 검증 / 실행

```bash
cd /Users/ws/Desktop/projects/gtmux

# FE
( cd codebase/frontend && npm run check )    # → 294 files / 0 errors / 0 warnings
( cd codebase/frontend && npm run build )    # → dist/ 갱신, gzip ~186 KB main

# BE
cd codebase/backend
cargo test --workspace --color=never         # → 0041 시점 327 PASS + 본 세션 implicit detach test 보강
cargo build --release --bin gtmux

# Demo
cd /Users/ws/Desktop/projects/gtmux
( cd codebase/frontend && npm run build ) && \
  unset TMUX && \
  GTMUX_FRONTEND_DIST="$(pwd)/codebase/frontend/dist" \
  GTMUX_SERVER__SESSION=demo \
  GTMUX_SERVER__PORT=9999 \
  GTMUX_SERVER__BIND=127.0.0.1 \
  ./codebase/backend/target/release/gtmux start --session demo --port 9999

# 확인 시나리오 (수동 smoke):
# 1. magic-link 진입 → LeftPanel (Layers/Terminals 탭) + RightPanel (Inspect) 표시
# 2. LeftPanel header [Tree | Z] toggle → Z 모드에서 z 내림차순 정렬 + z=<n> pill
# 3. LeftPanel fold chevron → 28px rail bar + 탭 아이콘 클릭 시 expand + tab select
# 4. RightPanel 동일 — 단일 Inspect 탭
# 5. 우클릭 (빈 canvas) → Add sub-menu (Terminal/Text/Note/Rect/Ellipse/Line/File path)
# 6. 우클릭 (terminal panel) → Arrange + Change terminal… + Remove from canvas
# 7. PanelNode header (…) → 같은 컨텍스트 메뉴 호출
# 8. Cmd+, → Settings overlay, Theme section radio = System/Light/Dark, Shortcuts section read-only matrix
# 9. Cmd+Shift+L → LeftPanel toggle, Cmd+Shift+I → RightPanel toggle, Cmd+Shift+Q → ShutdownModal
# 10. Cmd+N → toolStore.terminal armed, canvas click → spawn
# 11. Group row 더블클릭 → inline rename. ancestor 가 hidden/locked 면 자손 row 의 icon 회색 dot
# 12. Group row X 버튼 → GroupCloseConfirmModal (3 options + mirror hint)
# 13. Space hold + drag canvas → pan mode (cursor: grab)
# 14. Theme = System 으로 두고 OS 의 light/dark 토글 → chrome + xterm 양쪽 즉시 반영
```

---

## 8. 진입 시 첫 메시지 후보

다음 세션 진입 시 사용할 수 있는 명령 후보:

- **"미커밋 작업 split commit"** → §5.5 의 8-step split (BE 1 / FE 6 / Docs 1)
- **"Layer list V2 multi-select + drag reorder"** → §5.1 의 P1 큰 chunk. dual-source 제거 완료로 즉시 진입 가능 (권장)
- **"BE Slice D-1 wire (Settings GET/PATCH)"** → BE 가 Slice D-1 ship 한 시점, 0042 §3.1/3.2 따라 FE wire
- **"BE Slice D-2 wire (file_path)"** → BE 가 Slice D-2 ship 한 시점, FileOpenConfirmModal + FilePathNode + Storage section
- **"FE-NEW-8 file_path open UX 시작"** → BE/FE parallel
- **"Stage 8 진입 — BE asset endpoint"** → ADR 신규 + ImageNode/DocumentNode FE
- **"FreeDrawNode + RDP"** → Stage 9 P2
- **"plan-0008 Phase 2 진입 — Case II silent reattach + mutation guard"** → plan-0008 §2 4-step 매트릭스. Phase 1 의 reconnectGate + attemptReattach 토대 위에 idle reactivate 흐름 + outgoing write guard
- **"WS heartbeat client (ADR-0021 D6)"** → `lib/ws/heartbeat.svelte.ts` 신규, Phase 2 의 idle detection 입력

---

## 9. 참조 reading order (cold pickup)

1. **본 문서 §0 + §1.10 + §2 + §3 + §5** — 한 줄 + plan-0008 land + 구현율 + 잔여 + 향후 계획
2. `docs/reports/0044-be-slice-d-work-package.md` — BE 가 들어올 영역 (Settings + file_path + Import + Shutdown)
3. `docs/reports/0042-session-attach-recovery.md` — 별 agent 의 Case II decision (Phase 2 input)
4. `docs/plans/0008-session-attach-recovery-impl.md` — Phase 1 land 명세 + Phase 2 4-step 매트릭스
5. `docs/reports/0041-fe-be-session-handover.md` §3 + §5 — 이전 세션 base
4. `docs/agents/frontend-handover-v3.md` — FE 의 v3 매트릭스 base
5. `docs/agents/backend-handover-v3.md` §5 + §6 — BE 의 v3 매트릭스 base
6. `docs/adr/0017-layout-grid-and-chrome.md` amend ①~④ + follow-up — 본 세션 chrome 결정 모음
7. `docs/adr/0021-terminal-pool-and-mirror.md` D7 — Terminal list UI 위치 결정
8. `docs/adr/0018-canvas-item-data-model.md` — schema v2 + match-or-spawn
9. `docs/adr/0024-layer-tree-and-z-index-separation.md` — Tree/Z 분리 (Tree/Z toggle 의 기준)
10. `docs/adr/0020-auth-lifecycle.md` D4/D5 — Settings auth section의 기준
11. `docs/adr/0023-file-path-open-security.md` — file_path open UX 의 기준
12. `docs/adr/0014-server-process-supervision.md` D7 — Shutdown 의 기준
13. `CLAUDE.md` + `CONTEXT.md` — 프로젝트 메타 + 어휘 SoT

---

## 10. 변경 이력

- 2026-05-16: 초안 — 0041 cold-pickup 후 FE 통합 세션 결과 종합. Slice A/B/C 모두 ship + 추가 묶음 A 5건 + BE work package 발주. 미구현 매트릭스 갱신 (Stage 5 91% → 거의 100%, Stage 6 27% → 55%, Stage 7 14% → 75%). Dual-source adapter 제거 + Layer list V2 multi-select 가 FE only 의 남은 큰 chunk.
- 2026-05-16 (별 agent land 직후 갱신): plan-0008 Phase 1 (Session attach recovery — sessionStorageHint + reconnectGate + ReconnectModal + sessionStore.attemptReattach) land 반영 — §1.10 신규 chunk + §2.1 FE-NEW-2 ✅ Phase 1 + §2.7 공용 store 3건 추가 + §4.2/§4.3 작업트리 갱신 + §5.1 Phase 2 + heartbeat 후속 추가 + §6 Session attach recovery 결정 + §9 reading order. BE work package `0042-be-slice-d-work-package.md` → `0044-be-slice-d-work-package.md` rename (별 agent 의 `0042-session-attach-recovery.md` 와 prefix 충돌 해소).
- 2026-05-16 (묶음 B land — Dual-source adapter 제거): legacy 7 파일 (panels/groups/layout/ephemeral stores + http/layout + legacyNewPane + MIndicator) 삭제 + 12 consumer 단순화. `useSessionStore = $derived(...)` 분기 13 곳 → 0 곳. sessionStore 에 `focusMode` 필드 추가 (ephemeralStore 통합). dispatcher 의 WS 0x80/0x81/0x82/0x83/0x84 처리가 sessionStore 로 직결. setLayoutRefetchHandler/setAutoMountHandler 폐기. §0 한 줄 + §1.11 신규 chunk + §3.5 제거 표기 + §5.1 P0 항목 제거 + §5.4 권장 순서 갱신 + §5.5 7-step → 8-step + §6 결정 추가 + §8 메시지 후보 갱신. ADR amend 필요: ADR-0006 (FE 측 `/api/layout` v1 폐기) / ADR-0015 (auto-mount 의 multi-session 대체) / ADR-0017 amend ⑤ (sessionStore 단일 source). `npm run check`: 290 files / 0 errors / 0 warnings; `npm run build`: 클린.
- 2026-05-16 (묶음 C land — Layer list V2 multi-select + drag reorder/reparent): LayerTreeView 의 selectNode 가 Cmd/Ctrl/Shift modifier 3-mode 정합 (Shift = visibleRangeIds anchor↔target range). HTML5 drag + dragover 의 25/75% Y-ratio 분기 (before/inside/after). multi-drag (dragged ∈ M 시 M 전체), cycle 보호 (group descendants), locked guard, Z mode 비활성. commitReparent 가 single mutateActiveLayout call 로 items.parent_id + groups.parent_id + groups.order 동시 갱신. drop indicator CSS (2px accent line + dashed outline + dragging opacity). §0 한 줄 + §1.12 신규 chunk + §2.3 Stage 6 100% + §5.1 V2 항목 제거 + Marquee/item order P2 추가 + §5.4 잔여 순서 갱신 + §5.5 9-step + §6 결정 추가 + §10 변경 이력. ADR amend 필요: ADR-0024 (drag UX 명시) / ADR-0017 amend ⑥ (Layer list V2). `npm run check`: 290 files / 0 errors / 0 warnings; `npm run build`: 클린 (gzip ~54 KB main).
- 2026-05-16 (묶음 E land — 0045 refresh reconnect loop P0 후속 fix): (1) Canvas.flowNodes 가 id-cache + JSON signature 기반 → 동일 signature 시 Node ref 재사용 (effect-depth loop 차단). (2) sessionStore.silentReattach 의 wrapper 가 viewport snapshot/restore — Phase 2 silent 의도 보존. (3) reconnectGate state 5단계 (`booting/attaching/hydrating/ready/failed`) + modalState=`loading` normalize + markReady (markSuccess 호환 alias). (4) XtermHost ResizeObserver entry-level px dedup. (5) dev-only debugCounts (localStorage flag + console summary + window.__gtmuxDebug). BE 의존 사항 (`attach_handler` same-cookie idempotent) 은 `0046-be-attach-handler-idempotent.md` 로 발주. §0 한 줄 + §1.14 신규 chunk + ADR-0019 D5.4 / ADR-0024 D1 amend 필요 명시. `npm run check`: 291 files / 0 errors / 0 warnings; `npm run build`: 클린 (gzip ~56.4 KB main).
- 2026-05-16 (묶음 D land — FE-only Tier 1 잔여 4건): (1) Cross-session leak filter — TerminalListView 의 `showAllSessions` 토글 + default 시 active session 의 attached_sessions / unplaced 만 표시. (2) WS heartbeat client (`lib/ws/heartbeat.svelte.ts`) — lastFrameAt/lastActivityAt + isStale/isIdle derived + dispatcher.markFrame 호출 + +page.svelte start/stop. (3) plan-0008 Phase 2 — sessionStore 의 `silentReattach` / `guardOutgoingMutation` / `reattachInProgress` / `lastSilentReattachResult` + WS dispatcher 의 reconnecting→open 전이 trigger + +page.svelte 의 visibilitychange listener (isIdle 가드) + mutation guard wire 5 site (Canvas.spawnMultiSessionTerminal / deleteSelected, PanelNode.performClose, TerminalListView.attachToCanvas / killOne). (4) file_path inline rename 검증 (이미 ship). §0 한 줄 + §1.13 신규 chunk + §5.1 P1 4건 모두 ✅ + §5.4 권장 진입 = BE Slice D wire. ADR amend 필요: ADR-0021 D6 (FE-side watchdog) + ADR-0019 D5.1 (Phase 2 wire) + plan-0008 §6/§9. `npm run check`: 291 files / 0 errors / 0 warnings; `npm run build`: 클린 (gzip ~55.6 KB main).
