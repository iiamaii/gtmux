# Session Handover — 2026-05-17 — Maximize modal + UX polish batch

> 이 문서는 `session-handover` skill 로 생성된 session 인수인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-17 (저녁)
> - 생성 session 의 마지막 커밋: `8b2e2be` (feat(frontend): Maximize 의 xterm DOM portal — single instance, state 100% 보존)
> - 이번 session 의 주요 주제: Maximize 의 modal overlay + xterm DOM portal (single-instance content preservation), Focus 의 ViewportCtrl 이동 + sidebar-aware union BBox, ColorPicker var resolve + alpha 채널, Note minimize/maximize, Inspector state row 보강, cursor mode 일관성 (select-only selection), Brand image 적용
> - 같은 날 이전 handover (`2026-05-17-session-handover-component-design-batch.md`) 와 시간순서로 *이후* 위치 — 본 문서는 그 batch 이후의 작업 + 후속 iteration 의 정합 결과

---

## 1. 프로젝트 개요

- **이름**: gtmux
- **한 줄 정체성**: tmux 를 backend execution engine 으로 쓰는 single-user 의 web canvas workspace — tmux 가 process/session lifecycle 의 진실, FE 가 *canvas layout 의 진실*
- **현재 phase / 단계**: Stage 7+ (multi-session pivot 완료, UX/디자인 polish + 시안 정합 batch + Undo/Redo Phase 1~3 land). plan-0010 완료 / plan-0011 BE Slice-A1 대기.
- **침범 불가능한 invariants**:
  - **두 state 분리**: tmux state (mirror only) ↔ web state (FE 진실) — `docs/sketch.md` §4 + `CLAUDE.md`
  - **single-attach + no-takeover**: Webpage:Session = 1:1 — `docs/adr/0019-session-and-workspace-model.md` D3/D4
  - **control-mode integration**: tmux CLI shell-out 금지 — `docs/adr/0021-terminal-pool-and-mirror.md`
  - **ADR-before-code hard rule**: 비-trivial 결정은 ADR 우선, ADR amend 시 linked plan/handover 도 갱신 — `CLAUDE.md`
  - **Layout ≠ tmux layout**: 캔버스 free 배치 ≠ tmux split — `docs/sketch.md` §4
  - **Undo/Redo 단일 entry**: 모든 user-driven layout mutation 은 `sessionStore.applyMutation` 통과 (ADR-0028 Phase 3) — 직접 `mutateLayout` 호출 금지

## 2. 현재 session 요약

본 session 의 작업은 *세 갈래* — (A) v2 시안 정합 후속, (B) maximize 의 안정화 (여러 iteration), (C) UX 일관성 (cursor mode / focus / state row / brand image). Maximize 는 특히 사용자와의 합의 과정에서 3 번 redesign 됨: schema 변경 → in-flow geom override → modal + DOM portal (최종).

본 session 의 커밋 흐름 (시간순):

- `e062c27` feat(frontend): NoteNode redesign — v2 §01 정합 + minimize chip
- `f1a3ed1` docs(handover): 0051 (다른 worker)
- `48fc1b4`/`a747dac`/`861fb20`/`52310f6`/`a7911a5`/`8b5393d` Inspector edit / input 시안 / readonly muted / FilePath height / .k label / InspectorField .k width (다른 worker)
- `0c4fd21` feat(frontend): Titlebar brand 정리 (conic-gradient logo) + LayerTreeView full-width hover/gap
- `1efbb17` feat(frontend): LeftPanel item style 통일 + Inspector state-row full width + z-mode reorder (up/down) + ActiveSessionDropdown height 36
- `64ef296` fix(frontend): lasso 회귀 — onselectionchange wire 복구 (다른 worker)
- `ff6374c` docs(adr): 0028 Undo/Redo Draft
- `c36ea28` feat(frontend): Maximize modal v1 + cursor mode 일관성 (select-only selection) + Inspector/Note UX 보강 (minimize 제대로 연동 + figure 타입 hide + focus 버튼 + Note label=title 매핑) + ColorPicker transparent + brand image (docs/src/G)
- `44753eb` feat(frontend): ColorPicker — CSS var resolve + alpha 채널 지원
- `1481ef1` feat(frontend): Focus 기능을 ViewportCtrl 로 이동 + multi-select union BBox
- `1a24eb4` feat(frontend): Focus 가 sidebar (LeftPanel/RightPanel) 고려 + Note minimize chip (square box) 복원 + Note maximize 버튼 추가
- `128cc53` feat(frontend): Undo/Redo Phase 1+2 (다른 worker — historyStore + applyMutation + Cmd+Z)
- `b77bd4f` refactor(frontend): mutateLayout callers → applyMutation 단일 entry (다른 worker)
- `8b2e2be` feat(frontend): Maximize 의 xterm DOM portal — single XtermHost instance, scroll/buffer/state 100% 보존

### 결정사항 (사용자 합의 / 거부 포함)

- **Maximize 의 최종 패턴 = modal overlay + DOM portal** (사용자가 schema 변경 / in-flow override 모두 거부 후 합의). 단일 XtermHost 인스턴스가 in-flow PanelNode 의 `[data-portal-id]` host ↔ MaximizedItemModal 의 portal-slot 사이를 DOM `appendChild` 로 reparent. xterm 인스턴스 dispose 없이 scroll/buffer/focus 보존.
- **거부된 maximize 접근**:
  - (a) Schema panel geom 을 viewport-fill 로 변경 (`c36ea28` 직전 시도) — 사용자가 "canvas 상 panel 크기 변경하는건 되돌리기" 로 거부.
  - (b) In-flow geom override + viewport (0,0,1) lock — 사용자가 "구현이 불안정 — maximize 가 바로 동작 안 하고 canvas 누르면 동작" 으로 거부.
  - (c) Modal + 별개 XtermHost mount (두 인스턴스) — buffer 분량 넘는 history 손실. 사용자가 "매번 fresh 연결로 보임" 지적 → DOM portal 로 수렴.
- **Note 의 minimize/maximize 동작**:
  - minimize → **w=h=32 chip (square icon button)** — 직전 header-strip 시도를 사용자 요구로 chip 으로 복원. Inspector minimize 버튼 SVG 토글 (line ↔ square) 은 Panel 과 동일.
  - maximize → **Panel 과 동일** — `sessionStore.toggleMaximize(id)` 토글. MaximizedItemModal 이 type 분기로 note body 의 InlineEdit 까지 wire.
- **Note: Label == Title (Inspector 매핑)** — `noteAwareLabel(it)` / `commonNoteAwareLabel()` helper. Inspector 의 common label field 가 note item 의 `title` 을 read/write. Type section 의 중복 title row 제거.
- **Cursor mode 일관성** — 사용자 요구: "다른 mode 일때도 canvas 위 component 선택은 막아야해. 입력하다가 선택이 되어버려." → `isSelectMode = toolStore.current === 'select'`, SvelteFlow props `elementsSelectable / nodesDraggable / selectionOnDrag` 모두 isSelectMode gate.
- **Focus 의 ViewportCtrl 이동** — 사용자 요구: Layer / Inspector 의 focus 버튼 제거 → ViewportCtrl 의 target reticle 버튼 단일 entry. `sessionStore.zoomToSelection()` + union BBox 계산. sidebar (LeftPanel/RightPanel/rail) 의 가시 width 보정 → `computeVisibleCanvas()` 의 `visibleX/visibleW` 기준으로 viewport center.
- **Session button badge (ActiveSessionDropdown)** — 사용자 지적: 전체 pool 수 → **현 session 의 terminal 수만**. `sessionStore.items.filter(type==='terminal').length` 카운트.
- **Brand image** — 사용자가 `docs/src/G.png` 제공 → `codebase/frontend/src/lib/assets/brand-G.png` 로 복사 + Titlebar + auth page 의 `.brand-mark` 가 `<img>` 로 교체. conic-gradient 폐기.
- **ColorPicker**:
  - `var(--color-fg)` / `transparent` 같은 schema 값을 hex 로 resolve (hidden DOM probe + `getComputedStyle`) — hex-input 에 항상 hex 표시.
  - **alpha 채널** — rect/ellipse 의 stroke/fill 과 line stroke 에 `allowAlpha={true}`. note color / text color 는 미적용 (단색 identity / 가독성).
- **LeftPanel item style 통일** — Layer 와 Terminal row 의 font-size (12px 통일), action button (18×18 통일), hover opacity (0→1 통일). Mono/sans 의미적 차이는 유지.
- **Note bbox 회귀 fix**: `.note-node { overflow: visible }` (NodeResizer corner handle 의 negative offset 이 clip 되지 않게) + `:global(.panel-resize-line) { border-color: transparent }` (wrapper box-shadow 와 시각 중복 제거).

### 변경된 파일 (이번 session, commit 단위 누적)

| 파일 | 변경 요약 |
|---|---|
| `codebase/frontend/src/lib/chrome/MaximizedItemModal.svelte` (신규) | Workspace-level modal overlay. terminal 분기 = xterm DOM portal slot. note 분기 = InlineEditField (title) + InlineEditTextarea (body), `sessionStore.applyMutation` 으로 commit. backdrop click + Esc 로 restore. `z-index: var(--z-modal)` (2000) |
| `codebase/frontend/src/lib/canvas/PanelNode.svelte` | `.xterm-portal-host` wrapper 도입 (`<div data-portal-id={data.id}>`). isStreaming 가드에 maximize 무관. NodeResizer `isVisible` 에 `!isMaximized` 추가. onMaximizeClick 단순 토글. `:global(.panel-resize-line) { border-color: transparent }` |
| `codebase/frontend/src/lib/canvas/NoteNode.svelte` | minimize → chip (w=h=32), maximize 버튼 추가, NodeResizer `!isMaximized` 가드, `overflow: visible` (corner handle clip fix). Inspector label = title 매핑과 정합 (header 의 b 텍스트 = title) |
| `codebase/frontend/src/lib/chrome/ItemInfoView.svelte` | state-row flex full-width, minimize 버튼 figure 타입 hide (`selectionSupportsMinimize`), `applyMinimizeGeom` (terminal h=32 / note w=h=32 / 그 외 flag-only), `noteAwareLabel` + `commonNoteAwareLabel` 으로 note=title 매핑, focus 버튼 제거 (ViewportCtrl 로 이동) |
| `codebase/frontend/src/lib/chrome/ViewportCtrl.svelte` | Focus selection 버튼 신규 (target reticle, mCount===0 시 disabled). `onFocusSelection` → `sessionStore.zoomToSelection()` |
| `codebase/frontend/src/lib/canvas/Canvas.svelte` | `isSelectMode` / `isMaximizedActive` derived. SvelteFlow props 가 isSelectMode 기반 (elementsSelectable / nodesDraggable / selectionOnDrag). `computeVisibleCanvas()` (sidebar 고려) + union BBox $effect (pendingZoomToIds 처리) |
| `codebase/frontend/src/lib/stores/sessionStore.svelte.ts` | `pendingZoomToIds: string[] \| null` (단일/다중 통합) + `zoomToIds / zoomToSelection / clearPendingZoom`. `maximizedItemId` 만 유지 (`maximizedRestoreViewport` / `maximizedGeom` 폐기 — modal 패턴이라 불필요) |
| `codebase/frontend/src/lib/chrome/ActiveSessionDropdown.svelte` | height 28 → 36 (toolbar tool 정합), badge = `sessionTerminalCount` (session 내 terminal 수만) |
| `codebase/frontend/src/lib/sidebar/LayerTreeView.svelte` | row hover full-width (.row 가 li + padding-left → row-inner 로 이동), 행 간 2px gap, font-size text-md, z-mode up/down reorder (`zStore.bringForward/sendBackward`), focus 버튼 제거, note icon SVG inline (speech-bubble) |
| `codebase/frontend/src/lib/sidebar/TerminalListView.svelte` | font-size text-md 통일, action button 18×18, kill hover opacity 1 (attach 와 일관) |
| `codebase/frontend/src/lib/chrome/Titlebar.svelte` | brand-mark `<img>` 로 교체 (`brand-G.png`), workspace tab 제거 |
| `codebase/frontend/src/routes/auth/+page.svelte` | brand-mark `<img>` 동일 적용 |
| `codebase/frontend/src/lib/canvas/itemFactory.ts` | Note default color `--color-warning` → `--color-accent`, size 280×160 → 300×96 (v2 §01 정합) |
| `codebase/frontend/src/lib/ui/ColorPicker.svelte` | `resolveCssColor(s)` (var/named → hex), `allowAlpha` prop (8-digit hex), `allowTransparent` toggle, swatch 의 checker pattern, alpha numeric input + `%` suffix |
| `codebase/frontend/src/routes/+page.svelte` | `<MaximizedItemModal />` workspace 안 mount |
| `codebase/frontend/src/lib/assets/brand-G.png` (신규) | docs/src/G.png 사본 (Vite asset) |
| `codebase/frontend/src/vite-env.d.ts` (신규) | `/// <reference types="vite/client" />` — `.png` import 타입 지원 |
| `codebase/frontend/src/lib/canvas/MaximizedPanelModal.svelte` (삭제) | chrome/MaximizedItemModal 으로 통합 |

미커밋 변경 (다른 worker 의 WIP — 본 session 영역 아님):
- `codebase/frontend/src/lib/canvas/Canvas.svelte` — drag commit 의 `priorSnapshot` 옵션 (ADR-0028 D7 정합, Undo/Redo Phase 후속)
- `codebase/frontend/src/lib/stores/sessionStore.svelte.ts` — `applyMutation` 에 `priorSnapshot` 옵션 추가
- `codebase/frontend/src/lib/stores/danglingTerminals.svelte.ts` — store 보강
- `codebase/frontend/src/lib/ws/dispatcher.svelte.ts` — dispatcher 변경

## 3. 주요 참조 자료

| 영역 | 경로 | 왜 읽어야 하는가 |
|---|---|---|
| 프로젝트 instructions | `CLAUDE.md` | 언어 컨벤션 (docs KO / code EN), ADR-before-code hard rule, MCP graph 우선, applyMutation 단일 entry |
| 스펙 | `docs/sketch.md` | scope/MVP/우선순위/threat model (KO) |
| 직전 handover | `docs/reports/2026-05-17-session-handover-component-design-batch.md` | 같은 날의 *전반부* — v2 시안 정합 batch + plan-0010 완료 |
| 본 session 직전 handover | `docs/reports/0051-session-migration-handover.md` | cold-pickup brief (다른 worker 가 인계받기용) |
| 활성 plan #1 (BE 대기) | `docs/plans/0011-component-design-batch-caption-document.md` | caption / document inline-stored 신규 — BE Slice-A1 land 대기 |
| 활성 plan #2 (완료) | `docs/plans/0010-ui-ux-batch-4-inspector-and-layer-actions.md` | 5 task 모두 land |
| 본 session 정본 ADR | `docs/adr/0027-inspector-multi-select-and-alignment.md`, `docs/adr/0018-canvas-item-data-model.md` D10 amend, `docs/adr/0028-undo-redo-policy.md` (Phase 1~3 진행) | Inspector / schema / Undo-Redo 결정 |
| 시안 (frontend design) | `ref/frontend-design/components-v2.html` §01~§05 | Note / Document / FilePath / Panel / Shared rules 정본 visual spec |
| 시안 (inspector) | `ref/frontend-design/index-v2.html` `.prop-row` / `.input` | Inspector input field 의 시안 |
| Undo/Redo 후속 작업 doc | `docs/adr/0028-undo-redo-policy.md` | Phase 3 직후 — drag commit 의 priorSnapshot 패턴 (미커밋 다른 worker WIP) |
| 회귀 시나리오 | `docs/reports/0050-lasso-selection-regression-scenarios.md` | lasso/selection sync 회귀 검출 절차 (S1~S9) |

## 4. 진행중인 작업

본 session 의 *자체* 작업은 모두 commit 완료 (마지막 commit `8b2e2be`). 진행 중인 항목은 **다른 worker 의 미커밋 WIP** — 본 session 영역 아니지만 다음 session 이 인지해야 함.

### 4.1 Undo/Redo Phase 3+ — drag commit 의 priorSnapshot 패턴 (다른 worker)

- **상태**: 미커밋. 현재 working tree 에 `Canvas.svelte` + `sessionStore.svelte.ts` 의 `priorSnapshot` 옵션 추가됨
- **관련 문서**: `docs/adr/0028-undo-redo-policy.md` D7 (history capture 의 PRE-state 정의)
- **관련 파일**:
  - `codebase/frontend/src/lib/canvas/Canvas.svelte` (drag 의 onnodedragstop 부근 — `sessionStore.layoutSnapshot()` 으로 PRE 잡고 `applyMutation(..., { priorSnapshot })` 호출)
  - `codebase/frontend/src/lib/stores/sessionStore.svelte.ts` (`applyMutation` 의 옵션 시그니처 + 사용 로직)
- **다음 한 step**: 다른 worker 가 본 patch 를 review/commit 할 가능성. 본 session 은 *touch 하지 말 것* — drag-and-drop 의 history snapshot 의도가 정확히 그 worker 의 도메인.

### 4.2 danglingTerminals / dispatcher 변경 (다른 worker, 미커밋)

- **상태**: 미커밋 — `codebase/frontend/src/lib/stores/danglingTerminals.svelte.ts` + `codebase/frontend/src/lib/ws/dispatcher.svelte.ts` 의 작은 변경
- **다음 한 step**: 본 session 영역 외, 다른 worker 의 commit 대기

## 5. 향후 작업

### 5.1 Caption / Document inline-stored — BE Slice-A1 (plan-0011 §2)

- **목표**: ADR-0018 D10 amend 의 schema 를 BE 측 land — `canvas-layout.schema.json` + Rust `CanvasItem` enum + validation cap + serde round-trip test.
- **관련 문서**: `docs/plans/0011-component-design-batch-caption-document.md` §2, `docs/adr/0018-canvas-item-data-model.md` D10
- **선행 조건**: 없음 (BE worker 영역)
- **다음 한 step**: `codebase/backend/crates/.../canvas_item.rs` 의 `CanvasItem` enum 에 `Caption` variant 추가 + `Document` 의 (asset_id / content) optional 두 mode 검증

### 5.2 Caption / Document FE wire — Slice-A2 (plan-0011 §3)

- **목표**: BE land 후 FE wire — `CaptionNode.svelte` / `DocumentNode.svelte` 신규 + itemFactory + Canvas nodeTypes + Toolbar tool + LayerTreeView icon + ItemInfoView Type section.
- **선행 조건**: §5.1 (BE) land
- **다음 한 step**: 본 session 의 v2 §01/§02 시안을 정합한 CaptionNode/DocumentNode 컴포넌트 신규

### 5.3 PanelNode min 의 disk persistence (handover 직전 batch 의 §5.3 이월)

- **목표**: `sessionStore.restoredItemGeoms` 의 in-memory backup 을 schema-level 영속 — page reload 시 옛 geom 살아남도록.
- **선행 조건**: ~~ADR-0018 D11 amend (`ItemCommon.restored_geom?: { x, y, w, h }`)~~ → **D11 amend draft 작성 ✅ 2026-05-17** (`docs/adr/0018-canvas-item-data-model.md` §D11). Accepted 전 grilling/review + 별 plan 분리 필요.
- **다음 한 step**: ADR D11 grilling/review → plan 작성 (BE schema + FE handler + E2E) → 구현.
- **명시 결정 (D11 draft §Maximize)**: maximize 는 G20 ephemeral 이라 amend 대상 아님 — title 의 "max/min" 을 "min" 으로 좁힘.

### 5.4 PanelNode status LED 의 실 source

- ✅ **closed (2026-05-17)** — commit `e192397`. 4-state derived (running / connecting / dangling / offline) ship 완료.

### 5.5 FilePathNode foot row 실 데이터 (lines / KB / branch)

- **목표**: 시안 §03 의 `124 lines · 4.2 KB · main` 표시
- **선행 조건**: 별 ADR — BE file stat + git lookup endpoint, ADR-0023 의 allowlist 정책과 정합

### 5.6 Inspector Type section multi-select edit broadcast (직전 batch §5.7 이월)

- **목표**: 다중 동일 type 선택 시 shape stroke/fill / text alignment 등의 broadcast edit
- **다음 한 step**: `ItemInfoView.svelte` 의 `isMultiHomogeneous && commonType === 'rect'|'ellipse'|'line'` 분기 활성

### 5.7 brand-G.png 최적화

- **목표**: 현 사본 (347 KB) → 작은 PNG (64×64 px) 또는 SVG 로 교체. Bundle size 줄임.
- **다음 한 step**: docs/src/G.png 를 작은 사이즈로 재export → frontend assets/brand-G.png 갱신

### 5.8 Stale CSS cleanup — `.m-single { outline: none }` (직전 batch §5.9 이월)

- **목표**: §05 audit 후 cosmetic-only no-op 정리 — PanelNode (line 503-518) / FilePathNode (235) / ShapeNode (127) / TextNode (199) / NoteNode (200) / LineNode (245)

## 6. 주의사항 / Gotchas

- **Maximize 의 xterm DOM portal — fragile path**: 본 session 의 핵심 변경. PanelNode 의 `<div data-portal-id={data.id}>` 안의 XtermHost 의 DOM (containerEl) 이 modal active 시 `appendChild` 로 modal slot 으로 이동. cleanup 시 home 으로 복귀. **edge case 처리 — home 이 사라진 case (session switch 등) 는 noop**. 새 type 의 maximize 추가 시 동일 portal 패턴 따를 것.
- **maximize 직후 ResizeObserver fit() 가 1~2 frame 지연** 가능 — xterm 의 cell 크기 재계산이 즉시 끝나지 않을 수 있음. 사용자 시각 영향 미미하나 인지 필요.
- **MaximizedItemModal 의 note 편집 commit 은 applyMutation 통과** (`b77bd4f` 의 ADR-0028 Phase 3 정합). 이제 다른 worker 의 Undo/Redo 도 maximize 모드의 편집 history 를 capture.
- **Cursor mode 일관성 — select 외에는 selection 자체가 차단**: `elementsSelectable={isSelectMode}` SvelteFlow prop. 새 도구 추가 시 onpaneclick 의 spawn 로직만 신경, selection 로직 건드릴 필요 X.
- **Focus viewport 의 sidebar 보정**: `computeVisibleCanvas()` 가 `.left-panel, .left-rail, .right-panel, .right-rail` selector 로 sidebar 의 가시 경계 측정. **새 floating panel/rail 추가 시 본 selector 확장 필요**.
- **`pendingZoomToIds` 의 array shape**: 단일 항목도 `[id]` 로 set. 기존 `pendingZoomToItemId: string` 시그니처는 폐기 — 호출자 모두 `zoomToIds(ids)` / `zoomToSelection()` 두 entry 만 사용.
- **ColorPicker 의 `resolveCssColor` 는 DOM probe 사용**: SSR 환경에서는 `typeof document === 'undefined'` 가드. 첫 render 직후 (DOM 마운트 후) 만 동작.
- **Note bbox 시각 fix — `overflow: visible`**: `.note-node` 의 corner-radius clip 이 약해질 수 있으나 실 content 는 거의 overflow 하지 않음. 새 child 추가 시 명시적 overflow:hidden 필요 시 inner wrapper 추가.
- **`:global(.panel-resize-line) { border-color: transparent }` 은 PanelNode + NoteNode 모두 영향**: edge resize 의 시각 line 비활성 — corner handle 만 보이지만 edge resize cursor 는 그대로 작동.
- **다른 worker 의 parallel commit 가능성** — 본 session 진행 중 Inspector / Undo-Redo 시리즈가 *다른 worker* 에 의해 동시 land (`48fc1b4` ~ `b77bd4f` 시리즈). 다음 session 도 동일 가능성 — `git log --oneline 2026-05-17..HEAD` 로 확인 필수.
- **사용자가 거부한 접근법 (반복 회귀 위험)**:
  - Maximize: schema panel geom 변경 (사용자 거부)
  - Maximize: in-flow geom override + viewport (0,0,1) lock (사용자 거부 — "구현 불안정")
  - Maximize: 두 XtermHost mount + dispatcher mirror (사용자 거부 — "매번 fresh 연결")
  - Note minimize: header-strip (h=32, w 유지) (사용자 거부 — "chip 으로 복원")
- **사용자가 명시 결정한 정책**:
  - Maximize 후 in-flow 의 xterm state 는 *반드시* 보존 (single instance + DOM portal 패턴 합의)
  - Focus 는 ViewportCtrl 단일 entry (Layer/Inspector 의 focus 버튼 제거)
  - Cursor mode 가 select 외에는 캔버스 element 선택 차단
  - ColorPicker hex-input 에 `var(...)` 같은 raw CSS 값이 보이면 안 됨 (resolve 후 hex 표시)
  - Session button badge 는 server-wide pool 이 아닌 **현 session 의 terminal 수**

## 7. 새 session 시작 방법

이 문서를 받은 session 은 다음 순서로 부트스트랩한다:

1. 이 handover 문서 (`docs/reports/2026-05-17-session-handover-maximize-modal-and-ui-batch.md`) 를 끝까지 읽는다.
2. §3 의 `CLAUDE.md` + `docs/sketch.md` 를 읽는다 (언어 컨벤션 + invariants + applyMutation 단일 entry).
3. §3 의 **직전 handover** (`2026-05-17-session-handover-component-design-batch.md`) 를 빠르게 훑는다 — 같은 날 *전반부* 의 v2 시안 정합 결과.
4. §3 의 **활성 plan**:
   - **§5.1 진행 권장**: `docs/plans/0011-component-design-batch-caption-document.md` — BE Slice-A1 부터.
   - 또는 §5.3 (PanelNode max/min disk persistence) — ADR draft 부터.
   - 또는 §5.4 / §5.5 / §5.6 / §5.7 / §5.8 의 FE-only 짧은 항목 중 선택.
5. **다른 worker 의 미커밋 WIP 인지** (§4.1, §4.2): `git status` 로 `Canvas.svelte / sessionStore.svelte.ts / danglingTerminals / dispatcher` 의 변경 확인. **본 영역 touch 금지** — 그 worker 가 commit 할 때까지 대기 또는 협의.
6. **handover 작성 이후 변경 확인**: `git log --oneline 8b2e2be..HEAD` — 본 session 종료 후 다른 worker 가 commit 했을 가능성 있음. 특히 ADR-0028 Phase 3+ 후속 / plan-0011 BE land 여부.

만약 §5 의 항목 모두 우선순위 낮다면, `docs/reports/0050-lasso-selection-regression-scenarios.md` 의 §3 monitoring 항목 / brand-G.png 최적화 (§5.7) / stale CSS cleanup (§5.8) 같은 정리 작업 진행.

---

_생성: `session-handover` skill v1_
