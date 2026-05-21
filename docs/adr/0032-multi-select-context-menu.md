# ADR-0032: Multi-select context menu — batch 액션

- 상태: **Draft** (2026-05-17 신규)
- 관련 ADR: ADR-0017 (chrome — ContextMenu), ADR-0024 (z-index 분리 — 4 z 액션), ADR-0027 (multi-select + alignment), ADR-0028 (Undo/Redo), ADR-0030 (clipboard), ADR-0010 (Group)
- 근거 문서: 본 ADR 이전, `ContextMenu.svelte` 는 *single panel/pane id* 만 input — 다중 선택 (M.size > 1) 상태에서 우 클릭 시 *클릭된 단일 item* 만 대상이 됨. 사용자가 다중 선택 후 batch 액션 (Delete, Hide, Lock, Group, ...) 을 수행할 entry point 가 keyboard shortcut 또는 Inspector 만 — *우 클릭의 자연스러운 mental model 불충족*.

## 결정

### D1. ContextMenu 의 input 확장

ContextMenu 가 받는 input 을 *single item id* + **M (selection set)** 의 *둘 다* 로:

- 클릭 위치의 item 이 *M 안에 있음* → batch mode (M 전체 대상)
- 클릭 위치의 item 이 *M 밖* → M 무시, 클릭된 single item 만 대상 (그리고 M 은 해제 — Figma 컨벤션)
- 클릭 위치가 *빈 area* → empty-area mode (paste / fit-to-view 같은 *non-selection* 액션만)

### D2. Mode 별 액션 매트릭스

| Mode | 액션 |
|---|---|
| **Single-item** (M.size === 1 or empty M + item 클릭) | Copy / Cut / Paste · Z 액션 4 (Bring/Send) · Hide · Lock · Group · Rename · Change terminal · Delete |
| **Multi-item batch** (M.size ≥ 2, clicked-item ∈ M) | Copy (multi) / Cut (multi) / Paste · **Z 액션 4 (batch)** · **Hide all** · **Lock all** · **Group** · **Align (sub-menu)** · **Distribute (sub-menu)** · **Delete all** |
| **Empty area** | Paste · Fit to view · Add (sub-menu — toolbar 와 동일 entries) |

### D3. Per-item 액션의 multi mode 처리

- **Rename**: M.size ≥ 2 시 *hide* (mass rename 의미 모호. P1 의 batch rename 별도 검토).
- **Change terminal** (ADR-0021 D8): M.size ≥ 2 시 *hide* — 다중 terminal item 의 일괄 교체 의도 불명확.
- **Copy pane_id** (ADR-0017 §D2): M.size ≥ 2 시 *hide* — pane_id 다중 복사 의미 모호. *Copy* (item) 와 mental 충돌 회피.

### D4. Z 액션의 batch 동작 (ADR-0024 정합)

ADR-0024 의 4 액션 (Bring to front / Send to back / Bring forward / Send backward) 은 이미 *M-aware* 로 정의되어 있음 (`zStore.bringForward` 등). 본 ADR 은 *ContextMenu entry 노출* 의 결정만 — 동작은 ADR-0024 그대로.

### D5. Group / Ungroup 진입 (ADR-0010 정합)

- **Group**: M.size ≥ 2 (또는 ≥ 1 + parent_id 동일 그룹화) 시 ContextMenu 의 `[Group]` 액션 노출. ADR-0010 D4 의 `Group` 액션 발동.
- **Ungroup**: M 의 single member 가 *Group* type 일 때 `[Ungroup]` 노출 (ADR-0010 D12).

### D6. Align / Distribute (ADR-0027 정합)

ADR-0027 의 multi-select alignment 액션 — Inspector 안에서만 존재. 본 ADR 이 **ContextMenu 의 sub-menu** 로 second entry 제공:

- `[Align ▸]` — Left / Center / Right / Top / Middle / Bottom
- `[Distribute ▸]` — Horizontally / Vertically

M.size ≥ 2 시만 노출. M.size === 2 면 distribute 의 의미 약함 → enable but no-op visually.

### D7. Trigger 패턴

- **Canvas right-click**: `Canvas.svelte` 의 `oncontextmenu` 가 좌표 + clicked-item (또는 null = 빈 area) 을 `contextMenuRef.openAt(...)` 에 전달.
- **Panel/Note header right-click**: PanelNode header 의 `(…)` more button + native right-click 양쪽 모두 같은 ContextMenu 호출. ADR-0017 §D2 의 정합.
- **Layer tree row right-click** (P1): LayerTreeView 의 row right-click 도 같은 ContextMenu 진입 — sidebar 에서 batch 액션 — P1.

### D8. Undo/Redo (ADR-0028 정합)

- batch 액션 (Hide all / Lock all / Delete all / Z batch / Align batch) 은 **단일 `applyMutation` call** 로 표현 → historyStore 가 1 entry capture.
- 사용자는 Cmd+Z 한 번으로 batch 액션 전체 되돌리기.

### D9. M (selection) 의 *click-to-replace* 정합

D1 의 "클릭 위치의 item 이 M 밖 → M 무시" 는 *Figma 컨벤션*. 본 결정의 부작용: 사용자가 *우 클릭 자체를 selection 변경 의도로 활용* 가능 — 좌 클릭 selection 과 정합.

- 클릭된 item 이 M ∈ : M 유지
- 클릭된 item 이 M ∉ : M = {clicked-item} 으로 *replace*

이 동작은 ADR-0027 의 multi-select 좌 클릭 변형 (Cmd/Shift toggle) 과 직교 — 우 클릭은 *항상 single-replace 또는 batch* 둘 중 하나.

## 비채택 대안

- **다중 선택 후 우 클릭 = M 보존 + 클릭된 item 만 대상** (M.size > 1 인데 클릭된 single item 만 작동) — 사용자 의도 mismatch. 거부.
- **다중 선택 후 우 클릭 → 별도 "Multi" 메뉴 root** — sub-menu 분리. 액션 위치 학습 비용. 거부 — 본 ADR 의 평면 매트릭스 (D2) 가 일관.
- **우 클릭 자체로 M 변경 안 함 (M 외 클릭 시 우 클릭 무시)** — 사용자가 우 클릭 한 item 의 액션을 *기대* 한 흐름과 mismatch. 거부.

## 미해결

- **O1.** Empty area 의 `[Add ▸]` sub-menu — toolbar 와 액션 중복. Sub-menu 위치 결정 (toolbar 가 항상 보이므로 ContextMenu 항목 중복은 redundant). P1 의 user research.
- **O2.** Locked item batch 처리 — `[Delete all]` 시 locked 자손 처리는 ADR-0010 D10 의 group close 정합 (Cancel / Delete unlocked only / Delete all force) 패턴 차용? — P1.
- **O3.** Touch 환경의 long-press = 우 클릭 equivalent — mobile P2.

## Amend (2026-05-21 ①) — Mixed-type intersection + batch Delete + Align/Distribute wire

### 맥락

ADR-0032 의 D1-D9 가 multi-select ContextMenu 의 *구조* 를 정의했으나 직전 구현 (`ContextMenu.svelte`) 은 다음 갭이 있다:

1. **Mixed-type filter 미구현**: M 안에 다른 type 의 item 들 (예: rect + terminal + text) 이 섞여있을 때, 일부 액션 (e.g. `[Change terminal]`) 이 그 type 의 single-item 에만 의미. 사용자가 "공통 속성만" 보길 원함 (사용자 요구 2026-05-21).
2. **Align / Distribute sub-menu 미wire**: D6 에서 spec 만, ContextMenu 의 sub-menu entry 미land.
3. **Group / Ungroup 미wire**: D5 에서 spec 만.
4. **Batch Delete 미구현**: `onDeleteItem` 이 단일 `panelIdStr` 만 적용 — multi-mode 에서 batch 의도 mismatch.
5. **Z 4 액션 batch 미wire**: D4 가 *ADR-0024 가 M-aware* 라고 했으나, ContextMenu 의 `onBringToFront(panelIdStr)` 은 단일 id 만 인자로 받음 — `zStore` 의 batch path 우회.

### 결정

#### D10 — Mode 산정 + type intersection

```ts
const isMultiMode = panelIdStr !== null
  && sessionStore.M.has(panelIdStr)
  && sessionStore.M.size >= 2;
const selectedItems = isMultiMode
  ? [...sessionStore.M].map((id) => sessionStore.items.get(id)).filter(Boolean)
  : (panelIdStr !== null ? [sessionStore.items.get(panelIdStr)].filter(Boolean) : []);
const commonType = selectedItems.length > 0
  && selectedItems.every((it) => it!.type === selectedItems[0]!.type)
  ? selectedItems[0]!.type
  : null;  // null = mixed
const anyLocked = selectedItems.some((it) => it!.locked);
const allLocked = selectedItems.length > 0 && selectedItems.every((it) => it!.locked);
```

**액션 가시성 매트릭스 (mode + commonType)**:

| 액션 | single | multi (common) | multi (mixed) |
|---|---|---|---|
| Copy / Cut / Paste | ✅ | ✅ | ✅ |
| Bring/Send 4 z 액션 | ✅ (단일 id) | ✅ (batch 모든 id) | ✅ (batch) |
| Hide / Lock | ✅ | ✅ | ✅ |
| Group | ≥2 시 ✅ | ✅ | ✅ |
| Ungroup | group 단일 시 ✅ | — | — |
| Align ▸ | — | ≥2 ✅ | ≥2 ✅ |
| Distribute ▸ | — | ≥3 ✅ | ≥3 ✅ |
| Change terminal | terminal ✅ | terminal-only common ✅ | ❌ hide |
| Copy pane_id | terminal ✅ | ❌ hide | ❌ hide |
| Rename | text 등 ✅ | ❌ hide | ❌ hide |
| Delete (batch) | ✅ (single) | ✅ (batch all ids) | ✅ (batch) |

원칙: **type-specific 액션은 commonType 일치 시만 노출**. mixed 시 *공통 속성 만* — 위치/visibility/lock/clipboard/z/group 같은 type-agnostic 액션만 보임.

#### D11 — Z 4 액션의 batch 인자

`zStore.bringToFront / sendToBack / bringForward / sendBackward` 는 *이미 selection-aware* (모든 ID array 받음). 본 amend 가 ContextMenu 의 호출자 패턴을 변경:

```ts
// 옛
zStore.bringToFront(panelIdStr);
// 새
const ids = effectiveTargetIds();  // multi 면 [...M], single 면 [panelIdStr]
zStore.bringToFront(ids);
```

(zStore API 가 단일 id만 받는다면 별 helper 로 wrap — `for (const id of ids) zStore.bringToFront(id)` 는 z 충돌 위험 → zStore 의 batch 진입 필수.)

#### D12 — Batch Delete

`onDeleteItem` 가 `effectiveTargetIds()` 의 모든 id 를 `sessionStore.applyDeletion(ids, ...)` 한 번에 전달. ADR-0028 D9 정합 — single PUT = single history entry, Cmd+Z 한 번으로 복원.

#### D13 — Align / Distribute sub-menu

ContextMenu 안에 hover-popout sub-menu (또는 inline button group). entries:

- `[Align ▸] Left | Center | Right | Top | Middle | Bottom`
- `[Distribute ▸] Horizontally | Vertically`

각 entry click → ADR-0027 의 `applyAlign(...)` / `applyDistribute(...)` helper 호출. M.size ≥ 2 (Distribute 는 ≥ 3) 시만 노출.

#### D14 — Group / Ungroup

- `[Group]`: M.size ≥ 2 시 노출. ADR-0010 D4 발동.
- `[Ungroup]`: M 의 단일 member 가 *Group* type 시 노출 (ADR-0010 D12). multi mode 에서는 미노출.

ADR-0010 의 group helper 가 미land 면 entry 자체 disable (tooltip "Coming soon").

### 거절된 대안

- **R5.** Mixed-type 시 *각 type 별 sub-section* 분리 표시 — UI 복잡 + scan 비용. 거절: 공통 only 가 명확.
- **R6.** Mixed-type 시 단일 "Multi" disclaimer + 액션 회피 — 사용자가 batch 의도를 했으므로 *적용 가능한 액션* 은 노출하는 게 맞음.

### 산출물 / 정합 작업

- `lib/chrome/ContextMenu.svelte` — D10 의 mode/commonType 계산, D11 batch z, D12 batch delete, D13 Align/Distribute sub-menu, D14 Group/Ungroup entry.
- `lib/stores/zStore.svelte.ts` — batch API 노출 검증 (이미 있으면 wire only).
- `lib/chrome/ItemInfoView.svelte` 의 align/distribute helper — ContextMenu 에서도 재사용 (function export 또는 store helper).
- ADR-0010 group helper 미land 면 D14 entry 의 disabled state 명시.

## Amend (2026-05-21 ②) — Selection-wrapper right-click + 잘못된 진단 정정

### 맥락

직전 commit `883c21f` (D9 snapshot) + `b02deac` (global capture fallback) 의 시도 후에도 사용자 보고 회귀 잔존:

> "drag 후 오른쪽 클릭하면 browser native menu 는 안 보임. 그런데 lasso 내부영역을 오른쪽클릭해도 canvas 오른쪽 클릭 menu (empty-area menu) 가 나옴. cmd multi selection 은 정상 동작." (2026-05-21)

증상 정정:
- (이전 추정) drag-lasso 후 right-click → browser native menu 노출 → *틀린 진단*. `b02deac` 의 fallback 으로 native 는 차단됨.
- (실제) drag-lasso 후 *lasso bounding box 안 어디든* right-click → **우리 empty-area menu (Paste/Add)** 노출. M 은 유지되는데 menu 만 잘못된 mode.

### 진짜 root cause

xyflow `@xyflow/svelte` 1.5.2 의 `NodeSelection` 컴포넌트 — drag-lasso 종료 시 `store.selectionRectMode === 'nodes'` 가 되어 selected nodes 의 bounding box 를 덮는 overlay div 생성:

```html
<div
  class="svelte-flow__selection-wrapper"
  style="z-index: 2000; pointer-events: all;"
  oncontextmenu={oncontextmenu_internal}  <!-- onselectioncontextmenu 호출 -->
>
  <Selection ... />
</div>
```

- `pointer-events: all` + `z-index: 2000` → bbox 안 모든 right-click 의 target 이 wrapper 가 됨.
- 우리 capture handler 의 `target.closest('.svelte-flow__node')` = **null** (wrapper 는 node 의 ancestor 가 아님).
- SvelteFlow 도 `onpanecontextmenu` / `onnodecontextmenu` 를 fire 하지 않고 **`onselectioncontextmenu`** 라는 별도 callback 으로 routing — 우리는 그걸 wire 하지 않음 → fall-through 으로 우리 capture 가 처리하지만 panelId=null 로 흘러 empty-area menu.

Cmd-click multi-select 은 `selectionRectMode` 를 변경하지 않아 wrapper 가 미생성 → 정상.

ADR-0032 Amend ① (2026-05-21 ①) 의 D9 snapshot 가설 ("SvelteFlow 가 button=2 pointerdown 에서 M 을 reset") 도 본 회귀의 직접 원인이 *아니었음* — Cmd-click 이 정상 동작했다는 사실이 반증. snapshot 은 다른 edge case 대응으로 부수적 가치는 있으나 본 회귀의 fix 아님.

### 결정

#### D15 — Selection-wrapper right-click 은 multi mode menu

xyflow 의 `onselectioncontextmenu` callback 을 wire 하여 wrapper 위 우 클릭을 multi mode 진입으로 처리:

```ts
function onselectioncontextmenu({ event, nodes }) {
  if (isHandTool) return;
  event.preventDefault();
  const anyId = nodes[0]?.id ?? [...sessionStore.M][0];
  if (anyId === undefined) return;
  onContextMenuRequest?.({
    clientX: event.clientX,
    clientY: event.clientY,
    paneId: null,
    panelId: anyId,   // ContextMenu 의 isMultiMode 가 panelId ∈ M && M.size>=2
  });
}
```

defense-in-depth — 우리 capture handler `onCanvasContextMenu` 도 `target.closest('.svelte-flow__selection-wrapper')` 검사를 추가하여 redundant 경로로 같은 mode 진입. ContextMenu.openAt 가 idempotent 라 양쪽 fire 시 무해.

#### D16 — Scope-A: lasso bbox 바깥 빈 canvas 는 empty-area menu 유지

drag-lasso 다중 선택 (M.size ≥ 2) 활성 상태에서 wrapper bbox **밖**의 빈 canvas 우 클릭 — Figma 관습 따라 empty-area menu (Paste / Add) 노출, M 은 유지 (clear 안 함).

- 채택: Scope-A — bbox 안만 multi menu. bbox 밖은 empty-area menu + M 보존.
- 거절: Scope-B — M.size≥2 이면 위치 무관 multi menu. 사용자 ergonomics 적으로는 매력적이나 Figma/macOS 컨벤션과 어긋남. 추후 사용자 요구 시 재검토.
- 거절: Scope-A + bbox 밖 우 클릭 시 M clear — Figma 정통 동작이지만 좌클릭 deselect 와 의미 중복. 우 클릭의 deselect 효과는 학습 비용.

#### D17 — `rightClickMSnapshot` 의 평가

Amend ① 의 D9 (button=2 pointerdown 에서 M snapshot) 은 본 회귀의 fix 가 아니었다. 다만 일부 edge case (예: SvelteFlow 가 single-id click-to-select 동작을 right-click 에 적용하는 경우 — 본 ADR 작성 시점 미검증) 의 방어막으로 부수적 가치. *제거 보류* — 본 amend 의 wrapper detection 만 추가하고 기존 snapshot 코드는 그대로 (회귀 위험 회피).

후속 검증 항목: Cmd-click multi-select 후 selected node 위 right-click 시 SvelteFlow 의 reset 동작 — 실측으로 snapshot 코드 dead/load-bearing 판정. 결과에 따라 후속 amend 에서 정리.

### 거절된 대안

- **R7. ContextMenu 의 openAt 인자에 명시적 mode hint (`mode: 'single' | 'multi' | 'empty'`)**: openAt 확장 → 호출자 모두 mode 명시 → ContextMenu derived 단순화. 더 명료하지만 호출 site 다수 변경 + 본 회귀의 fix 와 결합도 약함. *후속 cleanup* 으로 미루기.
- **R8. `.svelte-flow__selection-wrapper` 의 `pointer-events: none` 으로 강제 override** — 우리 CSS 로 xyflow internal class 의 pointer behavior 변경. 깨지면 lasso-bbox-drag (이동) 도 무력. 거절: xyflow 내부 동작 의존성 증가.
- **R9. capture handler 만으로 처리, SvelteFlow callback wire 안 함**: capture 가 항상 *먼저* fire 하므로 충분하다는 가정. xyflow upgrade 시 capture syntax 변경 가능성 → defense-in-depth 손실. 거절: 양쪽 다 wire 하여 안정성 확보.

### 산출물 / 정합 작업

- `lib/canvas/Canvas.svelte` — `onselectioncontextmenu` 함수 + SvelteFlow markup 의 prop wire. `onCanvasContextMenu` 의 else branch 에 wrapper detection 추가.
- (후속 검증 필요) `rightClickMSnapshot` 의 dead 여부 판단 + cleanup.
- (후속 cleanup 후보) ContextMenu.openAt 의 명시적 mode hint (R7 의 R reject 와 별개로 polish 단계에서).

## Amend (2026-05-21 ③) — ContextMenu lifecycle (close-then-proceed) + Empty-area entry 확장

### 맥락

Amend ② 의 wrapper fix 후 사용자 추가 보완 요청 (2026-05-21):

1. menu card 가 열린 상태에서 *어떤 user input event 가 발생하든* 기존 menu 는 즉시 close + 새 event 그대로 진행. (예: 우 클릭 → 다른 item 좌 클릭 → menu close + 좌클릭 selection 정상; 우 클릭 → 다른 위치 우 클릭 → 기존 close + 새 위치에 menu open; lasso 후 우 클릭 menu open → cmd+좌 클릭 추가 선택 → menu close + 추가 선택 정상).
2. Empty-area (canvas 빈 공간) menu 의 entry 확장: Paste (clipboard 활성 시만 enabled) + Select all + Add ▸ (hover sub-menu) + Clear all + Switch session.

### 결정

#### D18 — Close-then-proceed lifecycle

ContextMenu 가 open 상태에서 *어떤 user input event 가 발생하든* 다음을 따른다:

- **`pointerdown`** (mouse / touch / pen — left·right·middle 모두): menu 내부 클릭이 *아니면* close. window-level **capture phase** 로 listener 등록 → underlying element 의 handler 보다 *먼저* close 가 일어남. 단 `preventDefault` / `stopPropagation` 모두 호출하지 않음 → 새 event 는 그대로 propagate 되어 element 의 click / context / drag 가 정상 수행.
- **`Escape` keydown**: 명시적 dismiss. 다른 keyboard shortcut (⌘C 등) 은 close 안 함 — close 가 trigger 한 event 와 shortcut 의 충돌 회피 (P1 검토).
- **`blur` window**: 탭 전환 / focus 손실 시 close.

Right-click → close → 새 right-click 의 contextmenu → openAt 의 sequence 가 자연 동작 (browser 가 pointerdown → mouseup → contextmenu 순서 보장). close 와 reopen 사이에 1-2 frame 의 flash 가 시각적으로 보일 수 있으나 — 채택. *별 freeze-frame 또는 in-place mutate 는 complexity 대비 이득 낮음*.

#### D19 — Empty-area menu entry 매트릭스

`panelIdStr === null` (빈 area / pane 우 클릭) 시 ContextMenu 의 entry:

| Entry | 가시성 / enable | 동작 | 단축키 |
|---|---|---|---|
| **Paste** | 항상 노출, `clipboardStore.hasItems` 일 때만 enable | `pasteItems(clipboardStore.entries, default offset)` | ⌘V |
| **Select all** | 항상 노출 (no-op 가능) | visible 한 모든 item id 를 M 으로. ⌘A 단축키와 동일 (`editingShortcuts.selectAllVisible`) | ⌘A |
| **Add ▸** | 항상 노출. hover 시 right-popout sub-menu | `Terminal / Text / Note / Rectangle / Ellipse / Line / File path` 7 entry. 클릭 시 anchor = ContextMenu pos 의 flow 좌표 | — |
| **Clear all** (danger styled) | `sessionStore.items.size > 0` 일 때만 enable | `applyDeletion(allIds, { killTerminal: false })`. Terminal pool 유지. Cmd+Z 복원 가능. *confirm dialog 없음* | — |
| **Switch session** | 항상 노출 | `workspaceSwitcher.open()` (SessionMenu 의 entry 와 동일 진입) | — |

`Add ▸` 의 hover submenu 는 `onmouseenter` / `onmouseleave` state binding — `addSubmenuOpen` boolean 으로 toggling. CSS 로 `left: 100%` 위치 (parent menu 의 우측 인접). viewport 우측 clipping 은 P1 (현 viewport 보통 sufficient — clamp 보강은 후속).

### 거절된 대안

- **R10.** Close listener 를 `mousedown` 만 listen — touch / pen 미cover, drag 시작 시 close 누락. *pointerdown* 으로 일반화 채택.
- **R11.** Close 와 reopen 사이의 freeze frame (close 보류 + state 만 mutate) — flash 회피용. complexity 대비 이득 낮음. 거절.
- **R12.** `Add` entry 를 hover 없이 click 으로만 expand — 빈도 높은 진입에 추가 click. UX 비용 거절.
- **R13.** Clear all 의 confirm dialog — Cmd+Z 복원 + Terminal pool 유지로 복구 비용 낮음. 확인 friction 거절. *후속 사용자 요구 시 재검토*.
- **R14.** Empty-area 의 `Add` sub-menu 폐기 (Toolbar 의 도구와 중복) — Amend 의 O1 미해결. 사용자 요구로 hover 형태로 *유지* 결정.

### 산출물 / 정합 작업

- `lib/chrome/ContextMenu.svelte`:
  - `onWindowPointerDown` (capture) + `onWindowBlur` listener. `mousedown` listener 폐기.
  - `onSelectAll` / `onClearAll` / `onSwitchSession` helper 추가.
  - empty-area branch 의 markup 재구성. `Add ▸` sub-menu 의 hover state + popout div.
  - `.ctx-item-with-sub` / `.ctx-submenu` CSS.
- 정합: keyboard 단축키 ⌘A (editingShortcuts) 와 menu entry 의 동작 일치 검증 — visible filter, M setM 패턴.
- 후속: `Switch session` entry 의 sessionStore.active === null 시 가시성 — 본 ADR 의 `if (sessionStore.active === null) return;` (Canvas.svelte 의 onCanvasContextMenu) 가 menu open 자체를 차단하므로 entry 도달 불가. no-session 진입 path 는 SessionMenu / WorkspaceSwitcher 의 상시 entry 가 cover.

## Amend (2026-05-21 ④) — Paste at click anchor + Terminal-included batch delete dialog

### 맥락

Amend ③ 후 사용자 추가 보완 (2026-05-21):

1. **Paste anchor 가 click 위치**: right-click → Paste 시 clipboard items 의 bbox top-left 이 *클릭 위치* 가 되도록. 기존엔 ⌘V 와 동일한 default offset (bbox + (24,24)*pasteCount) — ADR-0030 O2 의 deferred 항목.
2. **Terminal 포함 batch 제거 시 confirm dialog**: ContextMenu 의 `[Remove]` / `[Cut]` / `[Clear all]` 가 terminal item 을 포함하면 단일 panel close 와 같은 `PanelCloseConfirmModal` 노출 — `[Panel only]` / `[Panel + Terminal]` 선택. 기존엔 항상 `killTerminal: false` 로 일괄 처리 → 다중 terminal 의 SIGTERM 의도를 표현 못 함.

### 결정

#### D20 — Paste anchor = click flow position

ContextMenu 의 paste handler (`onPaste`):

```ts
const flow = screenToFlowPosition({ x: clickPos.x, y: clickPos.y });  // pre-clamp 원본 click
const sources = clipboardStore.entries;
const bboxX = Math.min(...sources.map((s) => s.x));
const bboxY = Math.min(...sources.map((s) => s.y));
const offset = { dx: flow.x - bboxX, dy: flow.y - bboxY };
await pasteItems(sources, { offset });
```

- `clickPos` 는 menu open 시점에 *clamp 이전* 의 원본 viewport 좌표로 별 저장 (ContextMenu state). `clampPos` 가 menu 자체를 viewport 안으로 옮겨도 paste anchor 는 사용자가 실제 클릭한 곳.
- `screenToFlowPosition` 으로 viewport → flow 좌표 변환.
- `pasteItems` 의 D4 offset 규약 그대로 — bbox top-left + (dx, dy). 모든 source item 의 상대 위치 보존.
- ⌘V 의 default (bbox + (24,24)*pasteCount) 는 *유지* — 두 path 의 의도가 다름 (click = 위치 명시, ⌘V = 복제 누적).
- `Add ▸` sub-menu 의 `onAddItem` 도 동일하게 `clickPos` anchor — *원본 클릭 위치에 spawn*.

ADR-0030 의 O2 (deferred) 가 본 결정으로 closed.

#### D21 — Terminal-included batch 제거 시 `PanelCloseConfirmModal` 경유

ContextMenu 의 3 destructive entry (`Remove from canvas` / `Cut` / `Clear all`) 가 *terminal item* 을 포함하면, 단일 panel close 와 동일한 `PanelCloseConfirmModal` 을 모달로 노출:

| Trigger | 동작 |
|---|---|
| Items 에 terminal 0개 | dialog 우회, `applyDeletion(ids, { killTerminal: false })` 즉시 |
| Items 에 terminal ≥ 1개 + `auto_kill_terminal_on_panel_close=true` | dialog 우회, `applyDeletion(ids, { killTerminal: true })` 즉시 |
| Items 에 terminal ≥ 1개 + 기본 settings | Dialog 노출. `[Cancel]` / `[Panel(s) only]` / `[Panel(s) + Terminal(s)]` 3-옵션 |

Dialog 의 attachCount / otherSessions 는 **selection 안 terminal 들의 합/union** — 어떤 terminal 이라도 다른 session 에서 mirror 되어 있으면 `[Panels + Terminals]` 비활성 (kill 시 다른 session 의 panel 들이 dangling). 단일 panel close 의 mirror 가드 (ADR-0010 G25 amend) 와 동일 정책.

신규 store: `lib/stores/panelCloseDialog.svelte.ts`. 호출자 API:
```ts
panelCloseDialog.show({
  items: CanvasItem[],
  onConfirm: (killTerminal: boolean) => Promise<void> | void,
});
```

`PanelCloseConfirmModal.svelte` 에 `count?: number` prop 추가:
- `count === 1` → 기존 title (`Close panel ‘X’?`) + 기존 단수 라벨 ("Panel only" / "Panel + Terminal").
- `count > 1` → batch title (`Remove N items from canvas?`) + 복수 라벨 ("Panels only" / "Panels + Terminals").

### 거절된 대안

- **R15.** Paste anchor 를 *menu pos* 로 사용 — `clampPos` 가 menu 를 viewport 안으로 옮긴 경우 사용자가 실제 우 클릭한 곳과 paste 위치가 어긋남. *clickPos* 별 보관으로 anchor 정확성 보장.
- **R16.** ⌘V 와 right-click paste 의 offset 통일 (둘 다 click anchor or 둘 다 default offset) — 사용자 의도 mismatch. right-click 은 *위치 명시*, ⌘V 는 *복제 누적*. 분기 유지.
- **R17.** Terminal batch 제거를 *항상 silent kill=false* — 사용자가 "session 끝낸다" 의도를 표현 못 함. dialog 경유 채택.
- **R18.** Terminal batch 제거를 *항상 silent kill=true* — 단일 close 와 mental model 불일치 + 사고로 인한 SIGTERM 위험. 거절.
- **R19.** Batch confirm 을 별 modal (`BatchPanelCloseConfirmModal` 신규) — 코드 중복. `PanelCloseConfirmModal` 의 `count` prop 으로 분기하는 게 변경 폭 작음.

### 산출물 / 정합 작업

- `lib/chrome/ContextMenu.svelte`:
  - `clickPos` state — pre-clamp 원본 viewport 좌표. `openAt` 에서 set.
  - `onPaste` / `onAddItem` 이 `clickPos` 기준으로 flow 좌표 변환.
  - `onCut` / `onClearAll` / `onDeleteItem` 가 `panelCloseDialog.show(...)` 경유.
- `lib/stores/panelCloseDialog.svelte.ts` — NEW. terminal 포함 여부 + `auto_kill_terminal_on_panel_close` 분기.
- `lib/chrome/PanelCloseConfirmModal.svelte` — `count` prop 추가. 단/복수 라벨 분기.
- `routes/+page.svelte` — `PanelCloseConfirmModal` 의 두 번째 instance 를 store 와 bind (panel 단일 close 는 PanelNode 안에서 직접 mount, batch 는 본 global instance).
- ADR-0030 O2 — closed (D20 으로 resolved).
- *후속 정합*: keyboard 단축키 (Cmd+Backspace 등) 의 batch delete 도 같은 dialog 경유 — P1 (현 ADR 미land).

## Amend (2026-05-21 ⑤) — Outside-wrapper click 의 Figma-precise deselect

### 맥락

Amend ② 의 D16 (Scope-A — bbox 바깥 빈 canvas 우 클릭 시 M 유지) 를 사용자 추가 명세로 update:

> "drag로 multi-selection 후 오른쪽 클릭하고나서 lasso 영역 외부를 클릭(오른쪽 클릭 포함)하면 선택은 해제되고 오른쪽 클릭이 되도록. + lasso 내부 영역도 단일 클릭은 component가 없는 영역이면 선택 해제로." (2026-05-21)

요구 사항:
1. **Outside wrapper bbox 우 클릭**: empty menu 노출 + M clear (이전 Scope-A 는 M 유지).
2. **Wrapper 내부 빈 영역 좌·우 클릭** (visual 상 node 없는 영역): selection 해제. wrapper 의 `pointer-events:all` 이 underlying node 의 click 을 가로채므로 *시각상 node 가 있는지* 판정하는 hit-test 필요.

`onpaneclick` 은 이미 끝에서 `sessionStore.clearM()` — outside 좌 클릭은 자연 deselect. 본 amend 는 *우 클릭* 과 *wrapper 내부 빈 영역 click* 의 정합 보강.

### 결정

#### D22 — Outside-wrapper 우 클릭 = empty menu + clearM (Scope-A 의 D16 supersede)

`onpanecontextmenu` 와 `onCanvasContextMenu` (capture handler) 의 else / pane 분기:
```ts
event.preventDefault();
sessionStore.clearM();   // NEW — Figma deselect
onContextMenuRequest?.({ ..., panelId: null });
```

D16 의 *M 유지* 결정은 본 amend 로 supersede. 좌 클릭 `onpaneclick` 의 `clearM()` 과 정합 — 좌·우 모두 outside = deselect.

#### D23 — Hit-test based wrapper interior dispatch

`nodeIdAtPoint(clientX, clientY)` helper — `document.elementsFromPoint` 의 stack 을 순회하며 `.svelte-flow__selection-wrapper` ancestor 인 element 는 skip. 첫 `.svelte-flow__node` ancestor 의 id 를 반환 (없으면 null).

wrapper 위 click 분기:
- **`onselectioncontextmenu` (우 클릭)**:
  - `nodeUnder !== null` → 기존 multi menu (panelId = M 의 임의 멤버)
  - `nodeUnder === null` (빈 공간) → empty menu + `clearM()`
- **`onselectionclick` (좌 클릭, 신규 wire)**:
  - `nodeUnder !== null` → no-op (현 시점 정책 미정 — Figma 정통은 single-replace 지만 사용자 명시 요구 없음)
  - `nodeUnder === null` → `clearM()`
- **`onCanvasContextMenu` capture handler 의 wrapper branch**: 동일 hit-test 분기 — SvelteFlow callback 과 idempotent 정합.

### 거절된 대안

- **R20.** Wrapper interior 의 모든 click 에 deselect (node under 무관) — Figma 의 "selection rect 위 클릭 = 그 안 한 item single select" 컨벤션과 어긋남. node under 케이스는 현 미설정 (no-op) 으로 두고 후속 결정.
- **R21.** `pointer-events:none` 강제 (wrapper 의 click 가로채기 무력화) — xyflow internal 의 wrapper drag (selected 전체 이동) 기능 손상. 거절.
- **R22.** Outside-wrapper 우 클릭 시 M 유지 + 'Deselect' menu entry 추가 — 사용자가 자연스러운 Figma 동작 요구. 한 step 더 = friction. 거절.

### 산출물 / 정합 작업

- `lib/canvas/Canvas.svelte`:
  - `nodeIdAtPoint(clientX, clientY)` helper — elementsFromPoint hit-test.
  - `onpanecontextmenu` — `sessionStore.clearM()` 추가.
  - `onCanvasContextMenu` (capture) — wrapper branch 의 hit-test 분기 + else branch 의 `clearM()`.
  - `onselectioncontextmenu` — hit-test 분기 (node under → multi menu, empty → empty + clearM).
  - `onselectionclick` (신규) — empty under 시 `clearM()`.
  - SvelteFlow markup 에 `{onselectionclick}` wire.
- Amend ② 의 D16 (Scope-A — M 유지) → D22 로 *superseded*. 변경 이력 entry.
- *후속 정합*: wrapper 위 좌 클릭 + node under 케이스 (Figma single-replace vs no-op) — 사용자 요구 없으면 보류.

## 변경 이력

- 2026-05-17: 신규 draft. ADR-0017 §D2 의 ContextMenu spec 을 batch 시나리오로 확장.
- 2026-05-21 ①: D10 (mixed-type intersection — common 속성 only) + D11 (Z 4 액션 batch wire) + D12 (batch Delete) + D13 (Align/Distribute sub-menu wire) + D14 (Group/Ungroup entry) amend. ContextMenu 의 구현 갭 (mixed-type filter / batch delete / sub-menu / group) 을 spec 으로 정형화. 거절: R5 type 별 sub-section / R6 mixed-type 단일 disclaimer.
- 2026-05-21 ②: D15 (selection-wrapper right-click 의 multi mode 진입 — xyflow `onselectioncontextmenu` wire + capture handler 의 wrapper detection) + D16 (Scope-A: bbox 바깥 빈 canvas 는 empty-area menu 유지) + D17 (`rightClickMSnapshot` 의 평가 — 본 회귀의 fix 아님, snapshot 은 다른 edge case 방어로 보류). 거절: R7 openAt mode hint 확장 / R8 wrapper pointer-events 강제 override / R9 capture-only path. 회귀 진단 정정 — Amend ① 의 D9 가설 (button=2 가 M reset) 은 본 회귀의 직접 원인이 아니었다 (Cmd-click 정상 동작이 반증).
- 2026-05-21 ③: D18 (close-then-proceed lifecycle — pointerdown capture + Escape + blur 로 menu 닫고 새 event 는 그대로 propagate) + D19 (Empty-area entry 확장 — Paste(conditional) / Select all / Add ▸ hover submenu / Clear all(danger) / Switch session). 거절: R10 mousedown only / R11 freeze frame / R12 Add click-only / R13 Clear all confirm / R14 Add submenu 폐기.
- 2026-05-21 ④: D20 (Paste / Add anchor = click flow position — clickPos 별 보관으로 clamp 후 anchor 정확성 유지. ADR-0030 O2 resolved) + D21 (Terminal-included batch 제거 시 PanelCloseConfirmModal 경유 — `panelCloseDialog` store + modal `count` prop 분기). 거절: R15 menu pos anchor / R16 ⌘V·right-click offset 통일 / R17 항상 kill=false / R18 항상 kill=true / R19 별 batch modal.
- 2026-05-21 ⑤: D22 (Outside-wrapper 우 클릭 = empty menu + clearM — D16 의 Scope-A "M 유지" 정책 supersede) + D23 (Hit-test based wrapper interior dispatch — `nodeIdAtPoint` 로 시각상 node 유무 판정. wrapper 안 빈 영역 좌·우 클릭 → clearM. `onselectionclick` wire 신규.). 거절: R20 wrapper 안 모든 click 에 deselect / R21 wrapper pointer-events:none / R22 outside 우 클릭에 별 Deselect entry. D16 (Scope-A) *superseded by D22*.
