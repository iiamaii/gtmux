# Session Handover — 2026-05-17 — Component design batch + panel min/max + UX refinement

> 이 문서는 `session-handover` skill 로 생성된 session 인수인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-17 01:10+09:00
> - 생성 session 의 마지막 커밋: `f80ecc1` (feat(frontend): WorkspaceEmptyPlaceholder — modal cancel 후 인지 단서 + 진입점)
> - 이번 session 의 주요 주제: plan-0010 (Inspector v2 + Alignment + Layer actions) 완료, ref/frontend-design/components.html §03/§04/§05 시안 정합 (FilePathNode/PanelNode/Wrapper outline), PanelNode min/max 실제 시각 적용 (schema geometry 변경), 진입 UX 회귀 fix (SessionListModal cancel + WorkspaceEmptyPlaceholder)

---

## 1. 프로젝트 개요

- **이름**: gtmux
- **한 줄 정체성**: tmux 를 backend execution engine 으로 쓰는 single-user 의 web canvas workspace — tmux 가 process/session lifecycle 의 진실, FE 가 *canvas layout 의 진실*
- **현재 phase / 단계**: Stage 6~7 (multi-session pivot 완료, UX/디자인 폴리시 batch + 시안 정합 batch). plan-0010 (P0/P1 5 task) 모두 land. plan-0011 (caption / document 신규 type) BE-handoff 대기.
- **침범 불가능한 invariants**:
  - **두 state 분리**: tmux state (mirror only) ↔ web state (FE 진실) — `docs/sketch.md` §4 + `CLAUDE.md`
  - **single-attach + no-takeover**: Webpage:Session = 1:1, 활성 session 강제 takeover 없음 — `docs/adr/0019-session-and-workspace-model.md` D3/D4
  - **control-mode integration**: tmux CLI shell-out 금지 — `docs/adr/0021-terminal-pool-and-mirror.md`
  - **ADR-before-code hard rule**: 비-trivial 결정은 ADR 우선, ADR amend 시 linked plan/handover 도 갱신 — `CLAUDE.md`
  - **Layout ≠ tmux layout**: 캔버스 free 배치 ≠ tmux split — `docs/sketch.md` §4

## 2. 현재 session 요약

본 session 은 두 가지 큰 batch 가 주축:
1. **plan-0010 — UI/UX batch 4** (5 task P0+P1 모두 land + ADR-0027 신규)
2. **ref/frontend-design/components.html 시안 정합** (§04 Panel header / §03 FilePath / §05 Shared rules / §01-§02 BE handoff)
중간에 회귀 fix (panel title null / line bbox / SessionListModal cancel) 와 진입 UX 강화 (WorkspaceEmptyPlaceholder) 도 land.

본 session 의 커밋 흐름 (시간순, 20+ commit):

- `7622e6c` plan-0010 작성 (5 task scope + 우선순위)
- `759ec05` Task 2 — terminal_meta label 우선 source (panel title null 회귀 fix)
- `91e6c1c` Task 1 — LayerTreeView min/max/focus + zoom-to-item
- `f63c1e1` ADR-0027 — Inspector multi-select + alignment (D1~D9)
- `ac1962f` Task 4 — ItemInfoView v2 (Common+Type split + mixed value)
- `1acd2c3` Task 5 — alignment.ts + Inspector 6 align + 2 distribute
- `48e26df` Task 1 fix — min/max 는 terminal panel 행 에만 (사용자 요구)
- `8d11298` Task 3 — ColorPicker + shape fill/stroke
- `1ea1d83` ADR-0018 D10 amend + plan-0011 (caption / document inline-stored 신규 — BE handoff)
- `c1b980b` PanelNode header redesign — 시안 §04 정합 (32px head + glyph + status LED + 4 actions)
- `30ef6fe` LayerTreeView min/max 제거 + row hover container 통합
- `e6e658b` FilePathNode redesign — 시안 §03 정합 (two-row card + lang badge)
- `05e3f4b` §05 Shared rules audit — wrapper 단일 source selection/hover (`.svelte-flow__node` box-shadow ring)
- `543f0ad` PanelNode minimize/maximize 시각 적용 (schema geometry 변경 + in-memory backup)
- `b52529f` line item wrapper bbox ring 회귀 fix — type-based selector 제외
- `42d8089` SessionListModal cancel 회귀 fix — closeList() 분기 복원
- `f80ecc1` WorkspaceEmptyPlaceholder — modal cancel 후 인지 단서 + 진입점

### 결정사항

- **plan-0010 의 5 task 우선순위 = (P0) Task 2 진단+fix → Task 1 / (P1) ADR-0027 → Task 4 → Task 3 → Task 5**. 이유: Task 4 가 3/5 의 공통 layout foundation. 사용자 확인: 진행 컨펌 후 5 task 모두 land.
- **Task 2 의 panel title null 회귀 Root cause = BE `terminal_meta` (per-UUID, server-wide, `terminals.rs:46-48`) ↔ layout file `items[].label` 의 source 분리**. FE 가 layout-side label 만 읽어 stale. **Fix Option F4**: display 시점에 `terminalPool.byId(id).label` 우선. 거절된 옵션: BE 측 GET layout 의 응답에서 join (별 ADR-0021 amend — out of scope).
- **min/max 는 PanelNode header 의 단일 entry — Layer list 의 row 에는 추가 안 함**. 사용자 명시 요구: "minimize, maximize는 panel header에만. 다른 element는 해당 기능 필요 없음." Layer row 에는 focus 만 유지 (모든 type 적용 가능).
- **§04 Panel header redesign 의 시안 정합 + 기존 chrome 보존**: 시안의 4-action cluster (min · max · ⋯ · close) 와 status LED 정합. 단 *inline label rename* (더블 클릭) + *I / L badge* 는 시안에 없으나 functional info 라 유지.
- **§05 Shared rules B/C — wrapper `.svelte-flow__node` 가 selection/hover outline 의 단일 source**. `box-shadow` ring 패턴 (border-radius 따라감) — outline 보다 자연. 각 Node 의 `.m-single`/`.m-multi` 는 *비-outline* 시각 단서 (PanelNode 의 header 색조 등) 만 유지.
- **PanelNode minimize/maximize = schema item geometry 변경 패턴 + in-memory backup** (`sessionStore.restoredItemGeoms`). 거절된 옵션: (a) CSS-only visual override — SvelteFlow controlled bounds 와 어긋남. (b) portal-based fullscreen overlay — XtermHost DOM 이동 큰 scope. **Trade-off 인지**: maximize 동안 pan/zoom 시 panel 함께 이동 (sustainable 우선). page reload 시 backup 손실 (default 220 으로 복원).
- **SessionListModal cancel 의 entry-point 분기 = `workspaceSwitcher.closeList()` 사용**. `Toolbar2.svelte:167` 의 `goList('closed')` 진입 시 cancel = canvas 로 닫기 / SessionMenu 의 switch 진입 시 cancel = AuthDialog choice 회귀. 회귀 발견: `WorkspaceSwitcher.svelte:215` 가 *항상* `open()` 호출하던 stale code.
- **WorkspaceEmptyPlaceholder — modal cancel 후 빈 dot grid 만 보이는 혼란 해소**. canvas workspace 안 centered card overlay, `sessionStore.active === null && workspaceSwitcher.stage === 'closed'` 조건. Cancel 자체 없음 — New / Open 두 button 만, 사실상 session 선택 강제.

### 변경된 파일 (commit 단위)

| 파일 | 변경 요약 |
|---|---|
| `codebase/frontend/src/lib/stores/sessionStore.svelte.ts:88-150` | `pendingZoomToItemId` (focus) + `zoomToItem()` + `restoredItemGeoms` Map + `backupItemGeom`/`getRestoredGeom`/`clearRestoredGeom` helpers |
| `codebase/frontend/src/lib/canvas/Canvas.svelte:534-560` (line case) | line node 의 type-class 활용 (`.svelte-flow__node-line` 의 selection/hover ring 제외) |
| `codebase/frontend/src/lib/canvas/Canvas.svelte:1073-1101` | `§05 Shared rules` — `.svelte-flow__node.selected` box-shadow 1.5px accent + `:hover` 1px border-strong + `.svelte-flow__node-line` 제외 |
| `codebase/frontend/src/lib/canvas/Canvas.svelte` (zoom effect) | `$effect` watch `pendingZoomToItemId` → item BBox 의 viewport 중앙 + 88% padding 적용 |
| `codebase/frontend/src/lib/canvas/PanelNode.svelte` | header 32px (glyph + title mono 11px/540/0.2px + status LED green + 4 panel-btn 22×22 cluster) / minimized state body hide + header bottom border 제거 / `onMinimizeClick` `onMaximizeClick` (schema geom 변경 + backup) / `terminalPool.byId(id).label` 우선 |
| `codebase/frontend/src/lib/canvas/FilePathNode.svelte` | two-row card (fp-main: 24×24 glass icon + path/name split, fp-foot: lang badge per-ext) / `splitPath` + `langBadge` derived / mono throughout |
| `codebase/frontend/src/lib/canvas/alignment.ts` (신규) | `alignItems(items, mode)` 6 mode + `distributeItems(items, mode)` 2 mode pure function. line endpoint 둘 다 평행 이동, locked 제외, distribute N≥3 |
| `codebase/frontend/src/lib/chrome/ItemInfoView.svelte` | Common+Type split: `selectedIds` / `commonType` / `commonField<K>` / `isMultiMixed` / `isMultiHomogeneous` derived. Multi-header. Mixed value display. alignment row (M.size ≥ 2). ColorPicker for shape stroke/fill. |
| `codebase/frontend/src/lib/ui/ColorPicker.svelte` (신규) | hex input + native picker swatch + mixed placeholder (diagonal-line swatch + 'Mixed' text). `oncommit(hex)` callback |
| `codebase/frontend/src/lib/sidebar/LayerTreeView.svelte` | min/max 제거 (focus 만 유지). row hover container 통합 (`.row:hover .row-inner` / `.row.selected .row-inner` — 별 영역 hover 제거). `terminalPool.byId` label 우선 |
| `codebase/frontend/src/lib/chrome/WorkspaceSwitcher.svelte:215` | SessionListModal `onClose` → `workspaceSwitcher.closeList()` (entry-point 분기) |
| `codebase/frontend/src/lib/chrome/WorkspaceEmptyPlaceholder.svelte` (신규) | canvas centered card — heading + deck + "New session" / "Open existing" 2 button |
| `codebase/frontend/src/routes/+page.svelte` | WorkspaceEmptyPlaceholder import + workspace div 안 conditional mount |
| `docs/adr/0018-canvas-item-data-model.md` | **D10 amend** — D3 type discriminant 에 `caption` 추가, D4 payload 에 caption (head/body/meta) + document inline-stored mode (content/file_name, asset_id optional) |
| `docs/adr/0027-inspector-multi-select-and-alignment.md` (신규) | Inspector D1~D9 — Common+Type 구조 / multi-select 정책 / Mixed value / alignment 6+2 / batch atomic mutation contract |
| `docs/plans/0010-ui-ux-batch-4-inspector-and-layer-actions.md` (신규) | 5 task scope + 우선순위 + ADR amend 인벤토리 |
| `docs/plans/0011-component-design-batch-caption-document.md` (신규) | caption / document 구현 plan + BE handoff (§2 Slice-A1) + FE wire (§3 Slice-A2) + §05 audit (§4) |

미커밋 변경: 없음 (모두 commit 완료).

## 3. 주요 참조 자료

| 영역 | 경로 | 왜 읽어야 하는가 |
|---|---|---|
| 프로젝트 instructions | `CLAUDE.md` | 언어 컨벤션 (docs KO / code EN), ADR-before-code hard rule, MCP graph 우선 |
| 스펙 | `docs/sketch.md` | scope/MVP/우선순위/threat model (KO) |
| 활성 plan #1 | `docs/plans/0010-ui-ux-batch-4-inspector-and-layer-actions.md` | **완료 상태** — 5 task 모두 land. 후속 cut 항목 §6 에 명시 |
| 활성 plan #2 | `docs/plans/0011-component-design-batch-caption-document.md` | **BE-handoff 대기** — caption / document 신규 type 의 schema + Rust + FE wire |
| 본 session 의 정본 ADR | `docs/adr/0018-canvas-item-data-model.md` (D10 amend), `docs/adr/0027-inspector-multi-select-and-alignment.md` (신규) | schema / Inspector / alignment 결정 |
| 시안 (frontend design) | `ref/frontend-design/components.html` §01~§05 | Caption / Document / FilePath / Panel / Shared rules 의 정본 visual spec |
| 시안 (auth page) | `ref/frontend-design/auth.html` | AuthPage 의 시안 — 이전 session 의 §05 정합 한 결과 (focus outline 만 a11y 후속) |
| 직전 handover | `docs/reports/0049-session-handover-ui-ux-and-auth-pivot.md` | 본 session 직전 (UI/UX batch 1~3 + ADR-0020 D13 auth pivot) |
| BE Phase 2 idempotent attach | `docs/reports/0046-be-attach-handler-idempotent.md`, `docs/reports/0047-be-next-session-handover.md` | BE 측 land 완료 (`e9eb9a6 / 9ee5679`) — F-1~F-6 brower 검증도 완료. 본 session 내 reference 만 |
| 회귀 시나리오 정리 | `docs/reports/0050-lasso-selection-regression-scenarios.md` | lasso/selection sync 회귀 검출 절차 (S1~S9) |

## 4. 진행중인 작업

본 session 자체의 *진행 중* 작업은 없음 — 모든 plan-0010 task + 시안 정합 batch (FE-only) 모두 land 완료. *다음 session 이 이어야 할* 후속 항목들은 §5 향후 작업 으로 분류.

(진행중 작업 없음 — §5 의 첫 항목부터 진행)

## 5. 향후 작업

### 5.1 BE Slice-A1 — caption / document schema 신규 (plan-0011 §2)

- **목표**: ADR-0018 D10 amend 의 schema 를 BE 측 land — `canvas-layout.schema.json` + Rust `CanvasItem` enum + validation cap + serde round-trip test.
- **관련 문서**:
  - `docs/plans/0011-component-design-batch-caption-document.md` §2 (BE work-package)
  - `docs/adr/0018-canvas-item-data-model.md` D10 (D4 payload table)
- **선행 조건**: 없음 (별 BE 작업자 또는 FE 가 직접 진행 가능 — Rust 작업)
- **예상 진입 지점**:
  - `codebase/backend/crates/.../canvas_item.rs` 의 `CanvasItem` enum 에 `Caption` variant 추가 + `Document` variant 의 (asset_id, content) optional + 두 mode 상호 배타 검증
  - `docs/ssot/canvas-layout.schema.json` 의 type enum 에 `caption` 추가 + payload schema 의 if/then 두 mode
  - cap: head 256B / body 4KB / meta 128B / content 64KB / file_name 256B

### 5.2 FE Slice-A2 — CaptionNode + DocumentNode + Toolbar tool + LayerTreeView icon (plan-0011 §3)

- **목표**: BE schema land 후 FE 측 wire — TS type 추가 + 2 Node component 신규 + itemFactory + Canvas nodeTypes + Toolbar tool button + LayerTreeView panelTypeIcon amend + ItemInfoView Type section payload.
- **관련 문서**: `docs/plans/0011-component-design-batch-caption-document.md` §3
- **선행 조건**: §5.1 (BE Slice-A1) land 후
- **예상 진입 지점**:
  - `codebase/frontend/src/lib/types/canvas.ts` 의 `CanvasItem` union 에 `CaptionItem` + `isCaption` 추가
  - `codebase/frontend/src/lib/canvas/CaptionNode.svelte` (신규) — flex column, 2px accent left rail, head/body
  - `codebase/frontend/src/lib/canvas/DocumentNode.svelte` (신규) — grid 30/1fr/26 (head/body/foot)
  - `codebase/frontend/src/lib/canvas/itemFactory.ts` 의 default payload
  - `codebase/frontend/src/lib/canvas/Canvas.svelte` 의 `nodeTypes`
  - `codebase/frontend/src/lib/toolbar/Toolbar2.svelte` 의 도구 (speech-bubble / file icon)
  - `codebase/frontend/src/lib/sidebar/LayerTreeView.svelte:239 panelTypeIcon` amend
  - `codebase/frontend/src/lib/chrome/ItemInfoView.svelte` Item Payload section (sessionItem.type === 'caption' / 'document') 분기

### 5.3 PanelNode min 의 disk persistence

- **목표**: `sessionStore.restoredItemGeoms` 의 in-memory backup 을 schema-level 영속화 — page reload 시 옛 geom 살아남도록.
- **관련 문서**: `docs/adr/0018-canvas-item-data-model.md` (**D11 amend draft 작성 ✅ 2026-05-17 — Accepted 전 grilling/review + 별 plan 필요**)
- **선행 조건**: ~~ADR-0018 D11 amend~~ → **D11 draft 작성됨**. 다음 step = grilling/review + plan 분리 (BE schema + FE handler + E2E).
- **예상 진입 지점** (D11 draft §"구현 step" 정합):
  1. BE `schema.rs::ItemCommon` 에 `restored_geom: Option<RestoredGeom>` 필드 + 신규 `RestoredGeom { x, y, w, h }` struct + serde round-trip test.
  2. FE `canvas.ts::ItemCommon` 에 `restored_geom?: { x: number; y: number; w: number; h: number }` 추가.
  3. `PanelNode.svelte` / `NoteNode.svelte` 의 `onMinimizeClick` 변경 — `backupItemGeom` 호출을 `applyMutation` 안 `it.restored_geom = {...}` 함께 set 으로 교체. restore 시 `it.restored_geom = undefined`.
  4. `sessionStore.restoredItemGeoms` 의 contract 명확화 — maximize-only backup 으로 격하 (또는 `maximizeBackupMap` 으로 rename, G20 amend 정합).
  5. E2E: minimize → reload → restore 시 옛 size 복원 검증.
- **명시 결정 (D11 draft §Maximize)**: maximize 는 schema field 아니므로 **본 amend 대상 아님**. G20 amend 후 ephemeral, reload 시 자동 unmaximize. maximize-side backup map 은 유지.

### 5.4 PanelNode maximized 동안 viewport interaction 비활성

- **목표**: maximize 동안 사용자가 pan/zoom 하면 panel 이 함께 이동하는 trade-off 회피 — `sessionStore.maximizedItemId !== null` 시 SvelteFlow 의 `panOnDrag` / `zoomOnScroll` 비활성.
- **관련 문서**: `docs/plans/0010-ui-ux-batch-4-inspector-and-layer-actions.md` §6 (Task 1 보류)
- **선행 조건**: 없음
- **예상 진입 지점**: `codebase/frontend/src/lib/canvas/Canvas.svelte` 의 SvelteFlow props — `panOnDrag={maximizedActive ? [] : panOnDragMask}` 패턴

### 5.5 PanelNode status LED 의 실 source

- **목표**: 현재 `running` 고정 표시 → `terminalPool.alive` / `muxStore.dead` 결합 으로 실 상태 반영 (running / idle / err).
- **관련 문서**: `c1b980b` 의 commit message 후속 항목
- **선행 조건**: 없음
- **예상 진입 지점**: `codebase/frontend/src/lib/canvas/PanelNode.svelte:headerLabel` 인근에 `statusKind` derived 추가 (terminal 의 alive/dead → running/dangling, idle 은 별 신호 필요)

### 5.6 FilePathNode foot row 의 실 데이터 (lines / KB / branch)

- **목표**: 시안 §03 의 `124 lines · 4.2 KB · main` 표시 — BE 가 file stat (size + line count) + git lookup (branch) 제공.
- **관련 문서**: `e6e658b` 의 commit message 후속
- **선행 조건**: 별 ADR — BE-side file stat / git endpoint + schema의 derived field. `ADR-0023 file-path-open-security` 의 allowlist 정책과 정합 필요.
- **예상 진입 지점**: 별 plan (file_path-server-stat) 우선

### 5.7 Inspector Type section 의 multi-select edit broadcast

- **목표**: Task 4 의 scope cut — 현재 Item Payload section 이 *selectionCount === 1* 시만 표시. 다중 동일 type 선택 시 shape stroke/fill / text alignment 등 의 broadcast edit.
- **관련 문서**: `docs/plans/0010-ui-ux-batch-4-inspector-and-layer-actions.md` Task 3/4 후속, `docs/adr/0027-inspector-multi-select-and-alignment.md` D3
- **선행 조건**: 없음
- **예상 진입 지점**: `codebase/frontend/src/lib/chrome/ItemInfoView.svelte` 의 `isMultiHomogeneous && commonType === 'rect'|'ellipse'|'line'` 분기 활성 + ColorPicker `mixed` prop 활용

### 5.8 Caption / Document / FilePath 의 §05 Shared rules 정합 audit (재검토)

- **목표**: 새 type (caption/document) 도 wrapper outline 단일 source 정합 (자체 outline 금지) + overflow:hidden + theme-reactive.
- **관련 문서**: `docs/plans/0011-component-design-batch-caption-document.md` §4
- **선행 조건**: §5.2 (FE Slice-A2) land 후
- **예상 진입 지점**: CaptionNode / DocumentNode 신규 시 동일 정책 적용

### 5.9 각 Node 의 stale `.m-single { outline: none }` cleanup

- **목표**: §05 audit 후 `outline: none` 정의가 cosmetic-only (no-op) — 가독성 위해 제거.
- **관련 문서**: `05e3f4b` 후속
- **선행 조건**: 없음 (cleanup-only)
- **예상 진입 지점**: PanelNode (line 503-518) / FilePathNode (235) / ShapeNode (127) / TextNode (199) / NoteNode (200) / LineNode (245). PanelNode 의 `.m-multi .panel-header` 의 header 색조 변화 는 *유지*.

## 6. 주의사항 / Gotchas

- **`.svelte-flow__node-line` 의 wrapper ring 제외** — `05e3f4b` 의 §05 audit 적용 후, line 의 bounding-box 에 ring 이 그대로 적용되어 *대각선 line 외 사각형 outline* 으로 회귀. `b52529f` 에서 type-based selector (`svelte-flow__node-{type}` — SvelteFlow 가 자동 부여, `node_modules/@xyflow/svelte/dist/lib/components/NodeWrapper/NodeWrapper.svelte:263`) 로 제외. **새 type (caption/document) 추가 시 line-art 유사 시각이면 동일 제외 필요**.
- **`workspaceSwitcher.closeList()` 의 entry-point 분기** — `listCloseTarget` ('closed' | 'choice') 따라 cancel target 달라짐. *ActiveSessionDropdown 진입* = `goList('closed')` → cancel = canvas (close). *SessionMenu switch 진입* = `goList()` → cancel = AuthDialog choice (open). **새 진입점 추가 시 어느 target 적용할지 명시**.
- **PanelNode min/max 의 in-memory backup 손실** — `sessionStore.restoredItemGeoms` 가 reactive state 라 page reload 시 사라짐. 사용자가 minimize 후 reload 하면 `h = 32` 상태로 disk persist, restore 시 *default 220* 으로 복원 (옛 사용자 size 손실). §5.3 의 ADR amend 후 fix.
- **PanelNode maximize 동안 pan/zoom = panel 함께 이동** — *schema-driven* 패턴이 가진 의도된 trade-off. portal-based fullscreen overlay (XtermHost DOM 이동) 가 큰 scope 라 보류. §5.4 에서 viewport interaction 비활성 으로 부분 완화.
- **terminal_meta label 의 BE side 단일 source** — `terminals.rs:46-48` 의 `terminal_meta.label` 이 server-wide pool. layout file `items[].label` 은 *별 source* 라 stale 가능. FE 가 display 시점에 `terminalPool.byId(id).label` 우선 (`PanelNode.svelte:88-99`, `LayerTreeView.svelte:panelDisplayLabel`). **새 display surface 추가 시 동일 패턴 적용 필수** (예: TerminalListView, ActiveSessionDropdown 등은 이미 적용).
- **다른 worker 의 parallel commit 가능성** — 본 session 진행 중 BE 측 (Phase 2 idempotent attach `e9eb9a6` / `9ee5679`) + LeftPanel/RightPanel resizable (`cbc277c`) + listCloseTarget 도입 (`514b15d`) + 0042 정합 (`741be5b`) 가 *별 worker* 에 의해 동시 land. 다음 session 도 동일 가능성 — `git log --oneline {handover-date}..HEAD` 로 항상 확인.
- **ItemInfoView v2 의 Type section 은 단일 선택만** — Task 4 의 scope cut. 다중 선택 시 Common section 만, Type section hide. **이 정책 변경하려면 §5.7 진행 필수** (broadcast edit 패턴 신규).
- **사용자가 거부한 접근법**:
  - LayerTreeView 의 min/max button 을 모든 type 에 — *거절*. "minimize, maximize는 panel header에만". 본 session 에서 layer row 의 min/max 제거 (`30ef6fe`).
  - 별도 영역 (label / icons) 의 hover effect — *거절*. row-inner 단일 hover 컨테이너 패턴. (`30ef6fe`)
- **사용자가 명시 결정한 정책**:
  - 첫 진입 modal cancel 후 사용자 인지 단서 필요 → `WorkspaceEmptyPlaceholder` 신규 (`f80ecc1`). "사실상 session 을 추가하거나 선택할 수 밖에 없도록" — Cancel 자체 없음, 두 액션 중 하나 강제.

## 7. 새 session 시작 방법

이 문서를 받은 session 은 다음 순서로 부트스트랩한다:

1. 이 handover 문서 (`docs/reports/2026-05-17-session-handover-component-design-batch.md`) 를 끝까지 읽는다.
2. §3 의 `CLAUDE.md` + `docs/sketch.md` 를 읽는다 (언어 컨벤션 + invariants).
3. §3 의 **활성 plan #2** = `docs/plans/0011-component-design-batch-caption-document.md` 를 읽는다 (plan-0010 은 완료).
4. §5.1 의 **다음 한 step** = "BE Slice-A1 (caption / document schema land)" 부터 진행하거나, BE 가 다른 worker 영역이면 §5.3~§5.5 의 FE-only 항목 (PanelNode max/min disk persistence / viewport interaction 비활성 / status LED 실 source) 중 선택.
5. handover 작성 이후의 변경 확인: `git log --oneline 2026-05-17..HEAD` — 본 session 종료 후 다른 worker 가 commit 했을 가능성 있음.

만약 §5 의 항목 모두 우선순위 낮다면, `docs/reports/0050-lasso-selection-regression-scenarios.md` 의 §3 S6 (lasso drag 중 시각 sync) / S7 (z-order race) 등 monitoring 항목으로 회귀 검증 진행.

---

_생성: `session-handover` skill v1_
