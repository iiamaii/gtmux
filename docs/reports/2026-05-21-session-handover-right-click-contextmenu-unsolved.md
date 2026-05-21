# Session Handover — 2026-05-21 — Right-click ContextMenu 미해결 (drag-lasso 후 browser native menu)

> 이 문서는 `session-handover` skill 로 생성된 session 인수인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-21
> - 생성 session 의 마지막 커밋: `b02deac` (fix(fe/canvas): drag-lasso 후 right-click 의 native menu 회귀 — global oncontextmenu fallback)
> - 이번 session 의 주요 주제: **drag-lasso 다중 선택 → right-click 시 browser native menu 가 노출되는 회귀를 새 session 에서 깨끗하게 재진입하기 위한 handover**. 직전 2 commit (`883c21f` D9 snapshot + `b02deac` capture-phase fallback) 으로 해결을 시도했으나 사용자 보고 여전히 재현됨. 본 session 의 전체 작업 (batch-5 / Inspector design 규칙 / ContextMenu 확장 / Hand mode 격리) 도 context 로 포함.

---

## 1. 프로젝트 개요

- **이름**: gtmux
- **한 줄 정체성**: tmux 를 backend execution engine 으로 쓰는 single-user 의 web canvas workspace — tmux 가 process/session 의 진실, FE 가 canvas layout 의 진실.
- **현재 phase / 단계**: **Stage 7+ polish** — multi-session pivot 완료 + canvas tool 셋 확장 + batch-5 (figure / text payload + 디자인 규칙) + ContextMenu D10-D14 확장 + Hand mode 격리.
- **침범 불가능한 invariants**:
  - **두 state 분리** — tmux state (mirror only) ↔ web state (FE 진실). `docs/sketch.md` §4 + `CLAUDE.md`.
  - **single-attach + no-takeover** — Webpage:Session = 1:1, 활성 session 강제 takeover 없음. `docs/adr/0019-session-and-workspace-model.md` D3/D4.
  - **applyMutation 단일 entry + D11.1 priorSnapshot rollback + D11.2 optimisticMutation wrapper** — Inspector hot-path 가 commit 시 즉시 UI 반영. `docs/adr/0028-undo-redo-policy.md` D11.1/D11.2/D11.3.
  - **No-session UI gating** — `sessionStore.active === null` 시 Toolbar 12 도구 + LeftPanel/RightPanel + SessionMenu shutdown 등 비활성. `docs/adr/0017-layout-grid-and-chrome.md` Amend ⑨ 와 §D6 amend ⑦.
  - **Hand 모드 = canvas/component 의 wall** (본 session 신규, ADR-0017 Amend ⑪) — pan 만 허용. left/right click + drag + resize 등 component event 모두 차단.
  - **ContextMenu multi-select D9 — clicked ∈ M 이면 M 유지 (batch), clicked ∉ M 이면 M = {clicked} replace** (ADR-0032 D9). 본 session Amend ① 에서 D10 (mixed-type intersection — 공통 속성만 노출) / D11 (Z batch wire) / D12 (batch Delete) / D13 (Align/Distribute) / D14 (Group/Ungroup deferred) 로 spec 확장.
  - **Inspector design 규칙 (사용자 2026-05-21 정의)** — component 24px height 통일 / width full / 라벨 좌측 내부 / Figma-style fig-group container. 본 session 에서 시각 통일 진행.

---

## 2. 현재 session 요약

본 session 은 크게 5 phase 로 작업:

### 2.1 Inspector design 규칙 적용 — Figma-style fig-group (commit `7e84d6d` ~ `375d019`)

사용자 정의 7-rule design 규칙 (`375d019` commit message 본문 참조):
- 모든 component (input/dropdown/picker) 좌측 내부 label
- Group (toggle expand 필요한 fill/stroke 등) 은 state-row 참조의 border container
- Component height 24px 통일
- Width 항상 full
- ColorPicker 외부 "color" label 제거, 내부 "HEX" → "C"
- Figure rect/ellipse 의 fill/stroke/rounded 3 group + line 의 stroke group + text 의 typography group

관련 ADR: ADR-0028 D11.2 (`optimisticMutation`) / D11.3 (`makeSignature` 완전성 invariant).

### 2.2 Figure ContextMenu 확장 + Hand mode 격리 (commit `9bd7b7f`)

- ADR-0017 Amend ⑪ 신규 — Hand tool 의 component event 절대 격리. 옛 동작은 left click 만 차단, right click 은 hand 모드에서도 열림. 본 amend 가 onpanecontextmenu / onnodecontextmenu 도 `if (isHandTool) return;` 으로 차단.
- ADR-0032 Amend ① 신규 — D10 (mixed-type intersection — 공통 속성만 노출) / D11 (Z 4 batch wire) / D12 (batch Delete) / D13 (Align/Distribute sub-menu) / D14 (Group/Ungroup entry deferred).
- ContextMenu.svelte 의 multi-aware 재설계 — `isMultiMode`, `effectiveItems`, `commonType`, `targetIds` derived 신규.

### 2.3 Terminal change-entry 일원화 (commit `83c949c`)

- PanelNode header 의 "panel actions" 3-dot kebab 폐기 → Change terminal button (DocumentNode change document 와 같은 link icon) 로 교체.
- Inspector Identity ID row 옆에도 Change terminal entry — terminal type only.
- State section 의 중복 `alive` row 제거.
- 후속 polish (`883c21f`): Change terminal button 을 header 의 leftmost (badges 다음 첫 번째) 로 이동. ContextMenu Align/Distribute 를 text-only line-by-line 으로 (icon grid 폐기).

### 2.4 Right-click M 보존 시도 #1 (commit `883c21f`)

직전 회귀 진단: drag-lasso 후 right-click 시 ContextMenu 가 single mode 로 열림.
- 원인 추정: SvelteFlow 의 click-to-select internal logic 이 mousedown(button=2) 시점에 M 을 단일 clicked id 로 reset → onnodecontextmenu fire 시점엔 이미 M.size === 1.
- 시도한 수정: capture-phase `onCanvasPointerDown` 에서 right-click on selected node 시 `rightClickMSnapshot = new Set(sessionStore.M)` 으로 snapshot. `onnodecontextmenu` 가 clicked node ∈ snapshot 이면 `sessionStore.setM([...snapshot])` 로 복원.

### 2.5 Right-click M 보존 시도 #2 — global fallback (commit `b02deac`)

사용자 추가 보고: "drag multi-selection 상태에서 오른쪽 클릭 시 여전히 browser native menu 만 나옴".
- 원인 추정: SvelteFlow 의 `onPaneContextMenu` / `onNodeContextMenu` prop 이 lasso overlay (`.svelte-flow__selection`) 등 edge case 에서 fire 안 됨 → `event.preventDefault()` 도 호출 안 됨 → browser native menu 노출.
- 시도한 수정: `.canvas-root` div 에 capture-phase `oncontextmenucapture={onCanvasContextMenu}` 직접 부착. SvelteFlow internal routing 과 무관하게 모든 right-click 의 가장 처음에 fire. target.closest('.svelte-flow__node') 로 node 판정 후 직접 ContextMenu.openAt 호출.

### 2.6 사용자 최종 보고 (handover trigger)

> "지금 문제 해결이 안되고 있는데, 깔끔하게 다시 우리 오른쪽 클릭에 대한 문제만 풀도록 하자."

→ 두 번의 시도에도 미해결. 새 session 에서 처음부터 다시 진단 + 깨끗하게 풀기.

### 결정사항

- **Hand 모드 = component event 절대 격리** (사용자 명시) — 사용자 verbatim "hand는 viewport 제어만 되어야해". ADR-0017 Amend ⑪ 로 spec lock.
- **ContextMenu D10 mixed-type intersection** — 사용자 verbatim "각각 설정이 다른 component들을 포함되어있을때에는 공통적인 속성만 표시". 거절된 대안: type 별 sub-section 분리 / single "Multi" disclaimer.
- **ContextMenu 모든 entry 는 text-only line-by-line** (사용자 verbatim "menu card에는 모두 text로만 기능을 표시해야해") — Align/Distribute 의 icon grid 폐기.
- **Change terminal button leftmost** (사용자 verbatim "왼쪽으로 위치하도록") — 가장 빈번한 액션이라 시각 우선.
- **Drag-lasso 와 Cmd-click multi-select 가 다르게 동작할 논리적 이유 없음** — 본 session 분석 결과. 둘 다 right-click 시 multi-mode 진입해야 함. 통일이 맞음.

### 변경된 파일 (본 session)

| 파일 | 변경 요약 |
|---|---|
| `codebase/frontend/src/lib/canvas/Canvas.svelte` | (1) `rightClickMSnapshot` snapshot/restore (line 383~ + onnodecontextmenu 1376~). (2) `onCanvasContextMenu` global fallback (line 1303~) + markup `oncontextmenucapture` (line 1418). |
| `codebase/frontend/src/lib/canvas/PanelNode.svelte` | header kebab → Change terminal button (leftmost). `changeTerminalDialog` import. `getContext` / `contextMenuHolder` 제거. |
| `codebase/frontend/src/lib/chrome/ContextMenu.svelte` | multi-aware 확장: `isMultiMode` / `effectiveItems` / `commonType` / `targetIds` derived. Z batch / batch Delete / Align·Distribute text entries. `canCopyPaneId` / `canChangeTerminal` / `canAlign` / `canDistribute` 가시성 derived. icon grid → text-only line-by-line. |
| `codebase/frontend/src/lib/chrome/ItemInfoView.svelte` | Identity ID row 옆 Change terminal button. State section 의 alive row 제거. `changeTerminalDialog` import. Inspector design 규칙 (Figma fig-group / text typography group / 24px / width full / "color" label 제거) 전면 적용. |
| `codebase/frontend/src/lib/chrome/DashSegments.svelte` | 24px 통일 + 사용자 요구로 dash → "style" rename, custom popover dropdown 으로 재설계, "style" label inside left. |
| `codebase/frontend/src/lib/ui/ColorPicker.svelte` | HEX → C label (k prefix). swatch 24→20px. inline-hex / inline-alpha 24px. |
| `codebase/frontend/src/lib/chrome/InspectorField.svelte` | height 22→24, disabled prop. |
| `docs/adr/0017-layout-grid-and-chrome.md` | Amend ⑪ — Hand tool component event 절대 격리 + 변경 이력 entry. |
| `docs/adr/0032-multi-select-context-menu.md` | Amend ① — D10/D11/D12/D13/D14 + 변경 이력. |
| `docs/adr/0028-undo-redo-policy.md` | (이전 session) D11.2 / D11.3 amend. |

미커밋: **없음 (본 session 범위)**. `git status` 의 dirty file 들 (`assets.rs` 등 BE 15 파일 — rustfmt only / `DocumentNode.svelte` / `XtermHost.svelte` — 0079 minimize buffer 보존 / `Toggle.svelte` untracked / `ref/frontend-design/components-v5` deleted) 은 모두 **다른 worker batch**.

---

## 3. 주요 참조 자료

| 영역 | 경로 | 왜 읽어야 하는가 |
|---|---|---|
| 프로젝트 instructions | `/Users/ws/Desktop/projects/gtmux/CLAUDE.md` | 언어 (docs KO / code EN), ADR-before-code, applyMutation 단일 entry, No-session UI gating |
| **본 session 의 미해결 정본** | `/Users/ws/Desktop/projects/gtmux/docs/adr/0032-multi-select-context-menu.md` (Amend ① 의 D9~D14) | ContextMenu multi-select 의 spec 및 본 회귀의 핵심 contract |
| **Hand mode 정본** | `/Users/ws/Desktop/projects/gtmux/docs/adr/0017-layout-grid-and-chrome.md` (Amend ⑪) | Hand tool component event 격리 contract |
| 직전 시도 코드 | `/Users/ws/Desktop/projects/gtmux/codebase/frontend/src/lib/canvas/Canvas.svelte:383-410, 1276-1340, 1376-1400, 1418` | 본 session 의 right-click 처리 시도들. 새 session 은 이걸 *기준선* 으로 읽고 디버깅 진입. |
| ContextMenu 구현 | `/Users/ws/Desktop/projects/gtmux/codebase/frontend/src/lib/chrome/ContextMenu.svelte` | multi-aware derived + entry 가시성 분기. 회귀가 ContextMenu 안에 있다면 여기서 검증. |
| 직전 commit 의 본문 진단 | `git show 883c21f` / `git show b02deac` — commit body 의 "원인" / "해결" 절 | 본 session 의 진단 추정 (확정 아님) — 새 session 이 검증 시작점. |
| 이전 session handover (이번 session 의 시작 시점) | `/Users/ws/Desktop/projects/gtmux/docs/reports/2026-05-18-session-handover-0065-fe-remediation-and-no-session-gating.md` | 본 session 이전 의 invariants + 작업 컨텍스트. batch-5 / 0079 connector / 0080 asset upload 의 병행 worker 영역 명시. |
| 직전 batch context | `/Users/ws/Desktop/projects/gtmux/docs/reports/2026-05-20-ui-ux-batch-5-analysis.md` + `2026-05-20-fe-handover-ui-ux-batch-5.md` | batch-5 의 R1-R8 결정 spec. 본 session 의 figure/text inspector 변경의 출발점. |
| 디버깅 도구 | `~/.local/state/gtmux/demo.token` (BE demo token), `browse` CLI (`/Users/ws/Desktop/projects/termcanvas/dist-cli/browse`) | E2E 검증 시 필요. browse `goto / fill / press / screenshot` 등으로 right-click 재현 가능. **단 browse 의 contextmenu 트리거 방법 검증 필요 — playwright 의 `page.click({ button: 'right' })` 매핑 여부 확인.** |

---

## 4. 진행중인 작업

### 4.1 ★ 최우선 — drag-lasso (또는 모든 right-click 경로) 의 ContextMenu 안정 동작 디버깅

- **상태**: 두 번의 수정 (commit `883c21f` D9 snapshot + `b02deac` global capture fallback) 후에도 사용자 보고 **여전히 browser native menu** 노출. 즉, 우리 `oncontextmenucapture` 가 *fire 안 되거나*, fire 됐는데 *preventDefault 가 효과 없는* 상황.
- **관련 문서**: `docs/adr/0032-multi-select-context-menu.md` (Amend ① 의 D9 / D10) + `docs/adr/0017-layout-grid-and-chrome.md` (Amend ⑪ Hand mode).
- **관련 파일·코드**:
  - `codebase/frontend/src/lib/canvas/Canvas.svelte`:
    - line 383~410 — `onCanvasPointerDown` 안의 `rightClickMSnapshot` snapshot 로직 (capture phase).
    - line 1276~1290 — SvelteFlow callback `onpanecontextmenu`.
    - line 1303~1340 — `onCanvasContextMenu` global capture handler (현 시도 #2 의 본체).
    - line 1376~1400 — SvelteFlow callback `onnodecontextmenu`.
    - line 1418 — markup `oncontextmenucapture={onCanvasContextMenu}` binding.
  - `codebase/frontend/src/lib/chrome/ContextMenu.svelte` — `openAt` (line 50~) + `isMultiMode` 등 derived 가 정상이라고 가정 (별 회귀 미관찰).
- **다음 한 step (제안 진단 사다리, 순서대로)**:
  1. **DevTools 로 실제 event flow 확인** — dev server 실행 후 (`cd codebase/frontend && pnpm dev`), 브라우저에서 drag-lasso 한 뒤 우 클릭. F12 → Event Listeners 탭에서 `.canvas-root` 의 contextmenu listener 등록 여부 확인. 또는 `monitorEvents(document.querySelector('.canvas-root'), 'contextmenu')` 콘솔 명령으로 fire 여부 직접 관찰.
  2. **만약 `onCanvasContextMenu` 가 fire 함**: preventDefault 가 호출되는지 확인. browser 가 native menu 를 그래도 노출한다면 → 다른 element 의 `oncontextmenu={e => {}}` (preventDefault 없이 stopPropagation 하는 것) 이 capture 보다 먼저 동작했을 가능성. e.g., SvelteFlow 가 자체적으로 .svelte-flow 또는 .svelte-flow__pane 에 contextmenu listener 를 capture phase 로 부착해서 *우리 capture 보다 더 먼저* fire → 우리 핸들러가 아예 호출 안 되는 회귀.
  3. **만약 `onCanvasContextMenu` 가 fire 안 함**: capture-phase 의 listener 등록 자체가 안 되었거나, Svelte 의 `oncontextmenucapture` 속성 syntax 가 v5 에서 capture 로 변환되지 않는 회귀 가능성. fallback 으로 `$effect(() => { canvasRootEl.addEventListener('contextmenu', handler, { capture: true }); return () => ...; });` 로 명시적 등록 시도.
  4. **만약 capture/bubble 둘 다 fire 안 함**: SvelteFlow 가 자체 `.svelte-flow__pane` 에서 `contextmenu` event 를 `stopImmediatePropagation` 으로 차단할 가능성. 이 경우 우리는 *SvelteFlow 의 callback prop 만* 사용 가능 — 즉 `onpanecontextmenu` / `onnodecontextmenu` 가 fire 되는 케이스로 한정. lasso overlay 위 우 클릭은 어떤 callback 도 안 받는 게 SvelteFlow 의 의도된 동작일 수 있음.
  5. **궁극 fallback** — `window.addEventListener('contextmenu', handler, { capture: true })` 로 *문서 root* 에 등록 후 target 이 `.canvas-root` 자손인지 직접 검사. browser 가 가장 먼저 받는 위치라 SvelteFlow internal stop 도 우회. 단, 다른 UI (Inspector, modal 등) 의 contextmenu 와 충돌하지 않도록 target scope 필터 필수.
- **사용자 가설 검증 필요**: "browser native menu 가 나옴" 이 정확히 어느 경우 발생하는지 사용자 시연 영상/스크린샷 요청 또는 browse 로 재현. 가능 시나리오:
  - (A) lasso 후 lasso overlay 위 우 클릭 → native (현 추정)
  - (B) lasso 후 selected node 위 우 클릭 → native (또 다른 회귀)
  - (C) lasso 후 *unselected* node 위 우 클릭 → native
  - (D) lasso 후 *empty area* 위 우 클릭 → native
  - 각 케이스가 다른 원인 (SvelteFlow internal 의 stopPropagation 시점) 일 수 있음. 새 session 은 *어느 케이스에서 회귀인지* 부터 사용자 확인.

### 4.2 drag-lasso 와 Cmd-click multi-select 의 동작 일관성 검증

- **상태**: 본 session 에서 *논리적 동등성* 만 문서화 (commit `883c21f` body). 실제 코드 동작 동등성 검증 미수행.
- **관련 문서**: `docs/adr/0032-multi-select-context-menu.md` Amend ① D9.
- **관련 파일·코드**: `Canvas.svelte:onnodeclick` (Cmd-click path) vs `onselectionchange` (lasso sync path). 둘 다 결국 `sessionStore.setM(...)` 호출.
- **다음 한 step**: §4.1 의 진단 결과에 따라, Cmd-click multi-select 후 right-click 도 같이 회귀했는지 확인. 같은 회귀라면 §4.1 의 fix 가 양쪽 다 해결. 다른 회귀라면 별 분기 필요.

### 4.3 (참고) Inspector design 규칙 후속 — Geometry 의 group 화 여부

- **상태**: 사용자 통일성 요구에 따라 Inspector 의 figure / text 가 fig-group container 안에. **Geometry (X/Y/H/W/Z) 는 group 외부 prop-row** — 본질적 row 라 group 신설 미적용. 사용자 후속 피드백 가능.
- **관련 문서**: 본 session 의 commit `8481bbe` (text fig-group), `37038cf` (figure fig-group), `375d019` (Inspector design 규칙 7 rule).
- **다음 한 step**: 사용자 명시 요구 없으면 보류. 만약 요구 발생 시 Geometry section 도 `fig-group is-on` wrapper 안에.

---

## 5. 향후 작업

### 5.1 ContextMenu D14 — Group / Ungroup 의 실제 wire

- **목표**: ContextMenu 의 `[Group]` / `[Ungroup]` entry 가 실 동작 (ADR-0010 D4/D12 의 group helper 호출).
- **관련 문서**: `docs/adr/0032-multi-select-context-menu.md` Amend ① D14 + `docs/adr/0010-group-data-model.md`.
- **선행 조건**: ADR-0010 의 group helper (`sessionStore.createGroup(...)`, `ungroup(...)`) 가 미land. helper 신설 → ContextMenu entry wire.
- **예상 진입 지점**: `codebase/frontend/src/lib/stores/sessionStore.svelte.ts` 의 group/ungroup helper 신설 + ContextMenu.svelte 의 `[Group]` entry enable.

### 5.2 Browse 기반 multi-select / hand mode E2E 회귀 가드

- **목표**: drag-lasso → right-click, Cmd-click → right-click, hand mode → right-click 의 3 시나리오 각각 browse 시나리오 자동화.
- **관련 문서**: `docs/reports/2026-05-20-fe-handover-ui-ux-batch-5.md` §D (AC table — context 잠재).
- **선행 조건**: browse 의 right-click 트리거 방법 확인 (`browse press button=2`? `browse contextmenu @e1`?). 안 되면 playwright 직접 호출 필요.
- **예상 진입 지점**: 별 sprint — 본 회귀 해결 후.

### 5.3 (other worker) 0079 XtermHost minimize buffer 보존 + 0080 asset upload + BE rustfmt

- **목표**: 다른 worker 의 in-progress batch. 본 session 의 right-click 영역과 무관.
- **관련 문서**: `docs/reports/0079-fe-handover-connector.md` 등.
- **선행 조건**: 본 session 미터치. `git status` 의 dirty file 들이 그 worker 의 작업.

---

## 6. 주의사항 / Gotchas

- **★ 사용자 명시 거부**: "지금 문제 해결이 안되고 있는데, 깔끔하게 다시 우리 오른쪽 클릭에 대한 문제만 풀도록 하자." — 즉 직전 두 commit (`883c21f`, `b02deac`) 의 접근이 **불완전 / 잘못된 진단**일 가능성 큼. 새 session 은 **그 commit 의 가설을 기준선이 아닌 디버깅 출발점** 으로 다뤄야 함. 필요시 *revert 후 처음부터* 도 고려.
- **`oncontextmenucapture` Svelte 5 syntax**: Svelte 5 에서 capture modifier 의 정식 표기가 변경됐을 가능성 (`onContextMenu` 가 listener 객체 syntax 만 capture 지원 등). 검증 필요 — 만약 capture 안 걸리면 fire 안 되는 게 자연스러움.
- **SvelteFlow `selectionOnDrag` 의 right-click 회귀**: SvelteFlow 가 lasso 중 / 직후 의 contextmenu 이벤트를 자체적으로 swallow 할 가능성. SvelteFlow GitHub issues 검색 키워드: `selectionOnDrag context menu`, `onPaneContextMenu lasso`, `selection rect right click`.
- **ContextMenu `openAt` 의 idempotent 가정**: 본 session 의 fallback 설계는 SvelteFlow callback 과 capture handler 가 둘 다 fire 시 두 번 호출되어도 안전하다고 가정. 실제로는 두 번째 호출의 position 이 정확히 같지 않을 수 있음 (`.openAt` 안의 clamp 가 그 사이 layout 변화로 다른 결과). 디버깅 시 *menu 가 두 번 깜빡이는* 시각 효과로 확인 가능.
- **Hand mode 와 ContextMenu 의 격리는 *다른 경로***: Hand mode 차단은 `if (isHandTool) return;` 으로 명시 — 본 회귀 와 무관. Hand mode 회귀가 보고된다면 별 가설.
- **drag-lasso 와 Cmd-click 의 동작 일관성**: 본 session 에서 *논리적으로 동등* 이라고 결론. 실제 회귀가 *둘 다* 발생하는지 *lasso 만* 발생하는지 사용자 시연으로 확인 필요. 만약 lasso 만 → SvelteFlow internal 의 lasso-specific 동작이 원인. 둘 다 → 우리의 capture handler 자체 문제.
- **다른 worker batch 동시 진행** — `git status` 의 dirty file 들 (`Toggle.svelte` 새 component / `XtermHost.svelte` minimize 보존 / BE fmt) 본 session 의 right-click 작업과 무관. 새 session 의 right-click 디버깅 시 *그 파일 영역 미터치* — commit 충돌 회피.
- **이번 session 의 batch-5 / Inspector design / Hand mode / ContextMenu 확장은 모두 land 완료** — `git log --oneline | head -25` 의 `2598578` ~ `b02deac` 범위. 본 회귀는 그 위에서 별도로 진단해야 함.

---

## 7. 새 session 시작 방법

이 문서를 받은 session 은 다음 순서로 부트스트랩한다:

1. **이 handover 문서 (§1~§6) 끝까지 읽는다**. 특히 §4.1 의 진단 사다리.
2. **`CLAUDE.md`** 읽기 — 언어 컨벤션, ADR-before-code, MCP graph 우선, **No-session UI gating**, **D11.1 / D11.2 / D11.3** (본 session 에 ship 한 invariants).
3. **§3 의 핵심 ADR 2건** 읽기:
   - `docs/adr/0032-multi-select-context-menu.md` (Amend ① 의 D9/D10 — 본 회귀의 contract)
   - `docs/adr/0017-layout-grid-and-chrome.md` (Amend ⑪ — Hand mode 격리)
4. **§4.1 의 다음 한 step 진단 사다리** 시작:
   - 우선 dev server 띄우고 (`cd codebase/frontend && pnpm dev`) **browser DevTools 로 실제 event flow 관찰** — 사용자가 보고하는 회귀 케이스 (A/B/C/D) 중 어디서 native menu 가 노출되는지 *시각 확인 + console.log 추적*.
   - 사용자에게 *어느 케이스인지* 추가 정보 요청 (스크린샷/시연) 검토.
5. **handover 작성 이후 변경 확인**: `git log --oneline b02deac..HEAD` — 본 session 종료 시점 HEAD = `b02deac`. 그 이후 다른 worker 의 commit 이 있는지 + 본 회귀 영역 파일 (`Canvas.svelte`, `ContextMenu.svelte`) 에 다른 변경이 있는지.
6. **(옵션) 직전 2 commit 의 가설 revert 검토** — `git revert b02deac 883c21f` 로 깨끗한 baseline 에서 재진단할지 결정. revert 시 ContextMenu 의 multi-mode 진입 자체가 회귀되니, *진단 끝난 후* 새 fix 와 함께 single commit 으로 정합.

만약 §4.1 진단 결과 새로운 ADR 결정이 필요하면:
- ADR-0017 Amend ⑫ 또는 ADR-0032 Amend ② 로 amend.
- *원인 + 해결 + 거절된 대안* 3 절 필수 (본 session 의 Amend ⑪ / Amend ① 패턴 참조).

---

_생성: `session-handover` skill v1_
