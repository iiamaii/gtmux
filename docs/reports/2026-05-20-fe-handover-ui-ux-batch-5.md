# 2026-05-20 — FE Handover: UI/UX Batch 5 — Shape/Text inspector + Tool gating + Note dblclick + Text auto-edit

> ⚠️ **2026-05-20 Grill #1-19 amend 적용 후** — 본 doc 의 *§A Task* / *§B Anchor* / *§D AC* 가 *grill 결과* 반영. 주요 변경:
> - **FE-G (XtermHost clipboard 차단) 완전 폐기** — R5 폐기로 unrelated.
> - **FE-J (Settings UI text_label_max_chars row) 완전 폐기** — Settings 신설 안 함.
> - **FE-K (terminal paste filter) 사실상 코드 변경 0** — 기존 cloneWithOffset 의 full clone + id 새로 가 *정확히 사용자 의도 매칭*. test-only 회귀 가드.
> - **FE-D (Inspector rect)** — corner_radius slider → `corner_rounded` **Toggle** 한 row. 수치 input X.
> - **FE-B (ShapeNode SVG)** — `cornerRadius = data.corner_rounded ? clamp(min(w,h)*0.15, 4, 16) : 0` 자동 계산.
> - **FE-E (Text)** — font_weight `[L|N|B]` 3-segment + I/U/S 3 toggle (각 boolean). font_size InspectorField 신규. color ColorPicker 신규.
> - **FE-H (Note dblclick)** — **body 영역만**. title 영역 별 처리 X. MaximizedItemModal 안 동일.
> - **FE-I (text auto-edit + derive)** — derive = `text.split('\n', 1)[0].trim().slice(0, 4000)`. label-empty trigger (`cur.label === '' && next !== ''`). justSpawnedTextId 의 `untrack` 안 write.
>
> **commit 총수: 9 → 5~6 commit** (FE-G / FE-J 제거 + FE-K 통합).
>
> 자세한 결정 trace 는 `2026-05-20-ui-ux-batch-5-analysis.md` 의 변경 이력 절. 새 컴포넌트 design 정합은 sister doc `2026-05-20-design-handover-ui-ux-batch-5.md`.

- 작성일: 2026-05-20
- 작성 주체: agent (frontend-architect role, FE/BE 짝)
- 정본 cross-link:
  - **상위 분석**: [`2026-05-20-ui-ux-batch-5-analysis.md`](./2026-05-20-ui-ux-batch-5-analysis.md) (8 요구·위험·결정)
  - **BE 짝 (prerequisite)**: [`2026-05-20-be-handover-ui-ux-batch-5.md`](./2026-05-20-be-handover-ui-ux-batch-5.md) — schema amend + Settings 키. **본 FE 작업은 BE-F (typings regenerate) 후 시작**
  - **결정 출처 ADR**: ADR-0018 D4 (rect/ellipse/line/text payload amend), ADR-0017 §Settings/§Toolbar2, ADR-0030 D3 (clipboard terminal clone), ADR-0004 (xterm), ADR-0027 (Inspector), ADR-0028 D11 (applyMutation), D11.1 (priorSnapshot)
  - 코드 anchor: `lib/canvas/ShapeNode.svelte`, `TextNode.svelte`, `NoteNode.svelte`, `XtermHost.svelte`, `Canvas.svelte`, `itemFactory.ts`, `clipboardOps.svelte.ts`, `chrome/ItemInfoView.svelte`, `chrome/SettingsOverlay.svelte`, `stores/sessionStore.svelte.ts`, `stores/settings.svelte.ts`, `xterm/options.ts`

## 핵심 원칙 — 거짓 ship 방지

- **Anchor 명시** (file:line).
- **Acceptance 가 검증 가능** — `pnpm check` / `pnpm build` / `grep` / browse E2E.
- **applyMutation 단일 entry 보존** (ADR-0028 D11) — 모든 user-driven layout mutation 통과.
- **D11.1 `priorSnapshot` 패턴 유지** — optimistic update 의 failure rollback 자동.
- **신규 schema field 의 default-aware** — BE 가 옛 record 를 default 로 deserialize. FE 도 `??` fallback 강제.
- **0079 invariant**: PanelNode 의 minimize 시 xterm 인스턴스 보존 (unstaged 영역, 본 batch 와 정합).
- **No-session UI gating** (ADR-0017 신규 invariant) — Toolbar/Inspector/Panel 모두 `sessionStore.active === null` 시 inert.

---

## 0. Self-grilling 결정

### Q1. ShapeNode 를 SVG 로 migration — 본 batch?

**결정**: ✅ **본 batch 에서 rect/ellipse/line 모두 SVG migration**. 이유:
- corner_radius 가 rect 에 추가 — CSS border-radius 로 처리 가능하나 dashdot stroke 와 *동시* 표현 불가 (CSS `border-style: dashed/dotted` 만 지원).
- ellipse 의 dashdot 도 CSS 불가.
- line 의 stroke_dash 는 이미 SVG `<line>` 만 가능.
- 세 type 의 일관성 — *모두 inline SVG*.
- 성능 영향 0 — item 당 SVG 1 element. SvelteFlow 의 virtualization 도 영향 없음 (한 wrapper div 안의 SVG).

**거절**: CSS border 유지 + line 만 SVG — 의도 분기 + dashdot 표현 불가.

### Q2. fill/stroke off 시 hit-test 구현 방법?

**결정**: ✅ **SVG `<rect>`/`<ellipse>` 의 pointer-events attribute 분기**. 이유:
- `fill_enabled=false` → `<rect ... fill="transparent" pointer-events="visibleStroke"/>` — stroke 만 hit.
- `fill_enabled=true && stroke_enabled=false` → `<rect ... stroke="none" pointer-events="visibleFill"/>` — fill 만 hit.
- 둘 다 true → `pointer-events="visiblePainted"` (또는 생략) — 둘 다 hit.
- 둘 다 false → `pointer-events="none"` — hit X. M 진입 시 NodeResizer outline 으로 인지.

대안 (거절): DOM div + 별 pointer-events:none child — SVG 의 native pointer-events attribute 가 더 단순.

### Q3. Text auto-edit 신호 — store flag 또는 prop chain?

**결정**: ✅ **`sessionStore.justSpawnedTextId: string | null`** store flag. 이유:
- prop chain (Canvas → nodeData → TextNode prop) 은 SvelteFlow 의 reactive prop 갱신을 거쳐야 — *mount 시점* 의 prop 가 늦게 도착.
- store flag 가 mount $effect 안에서 즉시 read 가능.
- TextNode 가 mount 시 *읽고 → editing=true 설정 → flag clear* — 한 번만 발화.

**거절**: SvelteFlow node 의 `data.autoEdit` — node update 가 SvelteFlow 의 nodes array 재계산을 trigger 해 mount cycle 다시 발생 위험.

### Q4. 도구 활성 중 node click → spawn forward?

**결정**: ✅ **onnodeclick 의 tool 분기 추가**. 이유:
- 사용자가 *기존 node 위에 새 item 만들고 싶음* 의도 빈번.
- 현 코드: tool active 시 `onnodeclick` 이 early return — *spawn 안 됨*.
- forward 구현: tool active + point-spawn tool 이면 `onpaneclick` 의 spawn 로직 *같은 좌표* 로 호출.

**거절**: tool active 시에도 node click 으로 선택 → 도구 의도와 충돌.

### Q5. Xterm clipboard 차단 — `attachCustomKeyEventHandler` + DOM event?

**결정**: ✅ **두 layer 모두**. 이유:
- `attachCustomKeyEventHandler` — keyboard shortcut path 차단 (Ctrl/Cmd+C/V/X).
- DOM `paste` / `copy` / `cut` event capture-phase 차단 — *programmatic* paste / right-click menu / middle-click primary selection 차단.
- `drop` / `dragover` 차단 — drag-and-drop paste 차단.

**거절**: `attachCustomKeyEventHandler` 만 — primary selection / right-click 의 OS paste 가 살아남음.

### Q6. Note dblclick zone — 어디서 어디까지?

**결정**: ✅ **`.note-node` 컨테이너 자체에 onContentDblClick**. event.target 분기:
- button 또는 그 자식 (SVG path) → return (자식 button 의 native handler 우선).
- `.note-head` 또는 그 자식 (label/glyph) → titleEditing = true.
- 그 외 (body / padding / gap) → bodyEditing = true.

**거절**: padding 안 invisible spacer div 추가 — DOM 복잡.

### Q7. Text label auto-derive 의 트리거 — typing 중 또는 commit 시점?

**결정**: ✅ **commit 시점만**. 이유:
- typing 중 매 keystroke 마다 layout PUT → BE 부하 + ETag conflict 위험.
- 사용자가 commit (Enter / blur) 할 때 *text + label 둘 다 한 PUT* — applyMutation 1 entry.

**거절**: typing 중 throttled label update — 복잡 + ADR-0028 의 history capture 다중 entry.

### Q8. Terminal paste 시 보존 field 의 정확한 list?

**결정**: ✅ **`type / x / y / w / h / z / parent_id / visibility / locked`** 만 source 에서 보존. 나머지 (label / description / minimized / restored_geom) = fresh default. 이유:
- 사용자 verbatim: *"크기만 재사용"* — 시각·위치·visibility/locked/parent 도 자연 의미 (Figma 패턴).
- label / description = 사용자 메타데이터 — fresh terminal 의 의미와 무관 (혼동 차단).
- minimized = ephemeral chrome state — fresh.

---

## §A. Task 목록 (Grill #1-19 amend)

| Task | 영역 | 출처 | 예상 소요 |
|---|---|---|---|
| **FE-A** | `lib/types/canvas.ts` 의 RectItem / EllipseItem / LineItem / TextItem field 확장 (`fill_enabled?` / `stroke_enabled?` / **`corner_rounded?`** / `stroke_dash?` / `font_weight?: 'light'\|'normal'\|'bold'` / `italic?: boolean` / `underline?: boolean` / `strikethrough?: boolean`) | R1 + R2 + R3 | 1 commit |
| **FE-B** | `lib/canvas/ShapeNode.svelte` 의 SVG migration — fill/stroke 의 `pointer-events` 분기 + **자동 corner radius (`corner_rounded ? clamp(min(w,h)*0.15, 4, 16) : 0`)** + stroke_dash (옵션 A `w*4 w*2` 등) | R1 + R2 | 1 commit |
| **FE-C** | `lib/canvas/LineNode.svelte` 의 stroke_dash 적용 (공통 helper `lib/canvas/strokeDash.ts` 권장) | R2 | FE-B 와 같은 commit |
| **FE-D** | `lib/chrome/ItemInfoView.svelte` 의 rect/ellipse section — fill on/off Toggle + stroke on/off Toggle + **corner_rounded Toggle (rect only, 수치 X)** + stroke_dash dropdown + stroke_width number input (1~32) | R1 + R2 | 1 commit |
| **FE-E** | `lib/canvas/TextNode.svelte` 의 font_weight (3 variant) / italic / underline / strikethrough inline style + Inspector **3-segment (L/N/B) + 3 toggle (I/U/S) + font_size number (8~96) + color ColorPicker** | R3 | 1 commit |
| **FE-F** | `lib/canvas/Canvas.svelte::onnodeclick` 의 tool active 시 spawn forward + marquee selection 잔여 시각 확인 (manual E2E only — 코드 변경 0 회귀 가드) | R4 | 1 commit |
| ~~**FE-G**~~ | ~~XtermHost clipboard 차단~~ | ~~R5~~ | **폐기 (Grill #8)** — terminal native clipboard 그대로 |
| **FE-H** | `lib/canvas/NoteNode.svelte` + `lib/chrome/MaximizedItemModal.svelte` — `.note-node` root onContentDblClick. **body 영역만** (title 영역 별 처리 X). MaximizedItemModal 안의 NoteNode 동일 적용 | R6 | 1 commit |
| **FE-I** | `lib/canvas/itemFactory.ts::commitNewItem` 후속 `sessionStore.justSpawnedTextId = id` + `TextNode` 의 mount $effect 의 auto-edit 진입 (`untrack` 안 write) + `onCommit` 의 **label-empty trigger derive** (`text.split('\n', 1)[0].trim().slice(0, 4000)`, `cur.label === ''` 시만) | R7 | 1 commit |
| ~~**FE-J**~~ | ~~Settings text_label_max_chars row~~ | ~~R7~~ | **폐기 (Grill #18)** — Settings 신설 안 함 |
| **FE-K** | `lib/canvas/clipboardOps.svelte.ts::cloneWithOffset` 의 terminal 분기 — **사실상 코드 변경 0**. 기존 generic clone (`...src + id 새로`) 이 *정확히 사용자 의도 매칭*. **test-only 회귀 가드** (label/description/visibility/locked/minimized/restored_geom/parent_id 모두 preserve 확인) | R8 | test-only commit |

**총 5~6 commit** — (FE-A) + (FE-B+C+D) + (FE-E) + (FE-F) + (FE-H) + (FE-I) + (FE-K test).

---

## §B. Anchor 변경 매트릭스

### B1. `lib/types/canvas.ts`

위치: 기존 RectItem (90-95), EllipseItem (97-102), LineItem (104-111), TextItem (71-81) interfaces.

```ts
// 변경 — RectItem
export interface RectItem extends ItemCommon {
  type: 'rect';
  stroke: string;
  fill: string;
  stroke_width: number;
  /** ADR-0018 D4 amend ① — fill on/off (≠ alpha). default true. */
  fill_enabled?: boolean;
  /** ADR-0018 D4 amend ① — stroke on/off. default true. */
  stroke_enabled?: boolean;
  /** ADR-0018 D4 amend ① — corner radius (rect only). 0 = sharp. */
  corner_radius?: number;
  /** ADR-0018 D4 amend ① — stroke dash pattern. undefined = solid. */
  stroke_dash?: FigureStrokeDash;
}

export type FigureStrokeDash = 'solid' | 'dash' | 'dot' | 'dash_dot';

// 변경 — EllipseItem (rect 와 동일 + corner_radius 없음)
export interface EllipseItem extends ItemCommon {
  type: 'ellipse';
  stroke: string;
  fill: string;
  stroke_width: number;
  fill_enabled?: boolean;
  stroke_enabled?: boolean;
  stroke_dash?: FigureStrokeDash;
}

// 변경 — LineItem (stroke_dash 추가)
export interface LineItem extends ItemCommon {
  type: 'line';
  stroke: string;
  stroke_width: number;
  x2: number;
  y2: number;
  stroke_dash?: FigureStrokeDash;
}

// 변경 — TextItem (4 옵셔널 추가)
export interface TextItem extends ItemCommon {
  type: 'text';
  text: string;
  font_size: number;
  text_align?: TextAlign;
  text_vertical_align?: TextVerticalAlign;
  color: string;
  /** ADR-0018 D4 amend ② — bold? */
  font_weight?: 'normal' | 'bold';
  /** ADR-0018 D4 amend ② — italic? */
  font_style?: 'normal' | 'italic';
  /** ADR-0018 D4 amend ② — underline? */
  underline?: boolean;
  /** ADR-0018 D4 amend ② — strikethrough? */
  strikethrough?: boolean;
}
```

(BE typings regenerate 시 `api.d.ts` 가 자동 생성 — 본 `canvas.ts` 의 type 은 별 hand-written. *둘 사이 drift 가드*를 위해 *FE-A commit* 에서 양 type 의 *snapshot 비교* test (vitest) 또는 grep 검증 권장.)

### B2. `lib/canvas/ShapeNode.svelte` SVG migration

위치: 전체 파일 (127 lines, line 78-106).

```svelte
<script lang="ts">
  // ShapeNode — rect / ellipse SVG renderer (ADR-0018 D4 amend ①).
  // SVG migration: dashdot stroke + corner_radius 표현 위해 CSS → SVG 전환.

  import { NodeResizer } from '@xyflow/svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import type { CanvasItem, EllipseItem, RectItem, FigureStrokeDash } from '$lib/types/canvas';
  import CanvasCloseButton from './CanvasCloseButton.svelte';

  // ... props 동일 ...

  const isVisible = $derived(data.visibility !== false);
  const isLocked = $derived(data.locked === true);
  const isInM = $derived(selected || sessionStore.M.has(data.id));
  const isEllipse = $derived(data.type === 'ellipse');

  // ADR-0018 D4 amend ① — fill / stroke on-off
  const fillEnabled = $derived(data.fill_enabled !== false);
  const strokeEnabled = $derived(data.stroke_enabled !== false);
  const cornerRadius = $derived(data.corner_radius ?? 0);
  const strokeDash = $derived(data.stroke_dash ?? 'solid');

  // SVG stroke-dasharray 매핑 (FigureStrokeDash → SVG attr)
  const dashArray = $derived.by(() => {
    const w = data.stroke_width;
    switch (strokeDash) {
      case 'dash':     return `${w * 4} ${w * 2}`;
      case 'dot':      return `${w} ${w * 2}`;
      case 'dash_dot': return `${w * 4} ${w * 2} ${w} ${w * 2}`;
      case 'solid':    return 'none';
    }
  });

  // pointer-events 분기 (R1 hit-test)
  const pointerEvents = $derived.by(() => {
    if (!fillEnabled && !strokeEnabled) return 'none';
    if (!fillEnabled) return 'visibleStroke';
    if (!strokeEnabled) return 'visibleFill';
    return 'visiblePainted';
  });

  // resize handler — 동일 (corner_radius 가 새 cap 위반 시 BE 가 reject — FE 도 clamp)
  // ...
</script>

{#if isVisible}
  <div class="shape-node" class:m-single={isInM} class:locked={isLocked}>
    <NodeResizer ... />
    <CanvasCloseButton id={data.id} disabled={isLocked} />
    <svg width="100%" height="100%" style="display:block; overflow:visible;">
      {#if !isEllipse}
        <rect
          x={data.stroke_width / 2}
          y={data.stroke_width / 2}
          width="calc(100% - {data.stroke_width}px)"
          height="calc(100% - {data.stroke_width}px)"
          rx={cornerRadius}
          ry={cornerRadius}
          fill={fillEnabled ? data.fill : 'none'}
          stroke={strokeEnabled ? data.stroke : 'none'}
          stroke-width={strokeEnabled ? data.stroke_width : 0}
          stroke-dasharray={dashArray}
          pointer-events={pointerEvents}
        />
      {:else}
        <ellipse
          cx="50%" cy="50%"
          rx="calc(50% - {data.stroke_width / 2}px)"
          ry="calc(50% - {data.stroke_width / 2}px)"
          fill={fillEnabled ? data.fill : 'none'}
          stroke={strokeEnabled ? data.stroke : 'none'}
          stroke-width={strokeEnabled ? data.stroke_width : 0}
          stroke-dasharray={dashArray}
          pointer-events={pointerEvents}
        />
      {/if}
    </svg>
  </div>
{/if}
```

(주의: SVG 의 `calc(...)` 는 attribute 가 아닌 *style* 안에서만. 위 예시는 의사 코드 — 실제 구현 시 `viewBox + preserveAspectRatio="none"` 또는 inline `style="..."` 로 표현 필요. 별도 measure 필요시 SvelteFlow 의 width/height prop 으로 직접 계산.)

### B3. `lib/canvas/LineNode.svelte`

`<line stroke-dasharray={dashArray} />` 추가 — 동일 dashArray 함수 재사용 (공통 helper `lib/canvas/strokeDash.ts` 권장).

### B4. `lib/chrome/ItemInfoView.svelte` rect/ellipse section

위치: 768-789 (현 stroke / fill ColorPicker 영역).

```svelte
{:else if sessionItem.type === 'rect' || sessionItem.type === 'ellipse'}
  <!-- ADR-0018 D4 amend ① — fill on/off + color -->
  <div class="prop-row full">
    <div class="display-row picker">
      <span class="k">fill</span>
      <Toggle
        checked={sessionItem.fill_enabled !== false}
        onchange={(on) => void applyShapeBoolean('fill_enabled', on)}
        aria-label="Enable fill"
      />
      {#if sessionItem.fill_enabled !== false}
        <ColorPicker
          value={sessionItem.fill}
          allowAlpha={true}
          allowTransparent={true}
          oncommit={(hex) => void applyShapeColor('fill', hex)}
        />
      {/if}
    </div>
  </div>
  <!-- ADR-0018 D4 amend ① — stroke on/off + color + width + dash -->
  <div class="prop-row full">
    <div class="display-row picker">
      <span class="k">stroke</span>
      <Toggle
        checked={sessionItem.stroke_enabled !== false}
        onchange={(on) => void applyShapeBoolean('stroke_enabled', on)}
        aria-label="Enable stroke"
      />
      {#if sessionItem.stroke_enabled !== false}
        <ColorPicker
          value={sessionItem.stroke}
          allowAlpha={true}
          oncommit={(hex) => void applyShapeColor('stroke', hex)}
        />
      {/if}
    </div>
  </div>
  {#if sessionItem.stroke_enabled !== false}
    <div class="prop-row">
      <InspectorField
        type="number" k="width"
        value={String(sessionItem.stroke_width)}
        ariaLabel="Stroke width"
        oncommit={(s) => void applyShapeNumber('stroke_width', Math.max(0, Math.min(64, Number(s))))}
      />
      <div class="dash-picker">
        <span class="k">dash</span>
        <select
          value={sessionItem.stroke_dash ?? 'solid'}
          onchange={(e) => void applyShapeDash(e.currentTarget.value as FigureStrokeDash)}
        >
          <option value="solid">Solid</option>
          <option value="dash">Dash</option>
          <option value="dot">Dot</option>
          <option value="dash_dot">Dash-Dot</option>
        </select>
      </div>
    </div>
  {/if}
  <!-- ADR-0018 D4 amend ① — corner_radius (rect only) -->
  {#if sessionItem.type === 'rect'}
    {@const cap = Math.floor(Math.min(sessionItem.w, sessionItem.h) / 2)}
    <div class="prop-row full">
      <InspectorField
        type="number" k="radius"
        value={String(sessionItem.corner_radius ?? 0)}
        ariaLabel="Corner radius"
        oncommit={(s) => {
          const next = Math.max(0, Math.min(cap, Math.round(Number(s))));
          void applyShapeNumber('corner_radius', next);
        }}
      />
    </div>
  {/if}
{/if}
```

### B5. `lib/canvas/TextNode.svelte` font 추가

위치: 60-94 (현 inline style + InlineEditTextarea).

```svelte
<script>
  // ... 기존 ...
  const fontWeight = $derived(data.font_weight ?? 'normal');
  const fontStyle = $derived(data.font_style ?? 'normal');
  const underline = $derived(data.underline === true);
  const strikethrough = $derived(data.strikethrough === true);

  const textDecoration = $derived.by(() => {
    const parts: string[] = [];
    if (underline) parts.push('underline');
    if (strikethrough) parts.push('line-through');
    return parts.length > 0 ? parts.join(' ') : 'none';
  });

  // mount $effect — R7 auto-edit
  $effect(() => {
    if (sessionStore.justSpawnedTextId === data.id) {
      editing = true;
      sessionStore.justSpawnedTextId = null;
    }
  });

  // onCommit body — label auto-derive (R7)
  async function onCommit(next: string): Promise<void> {
    if (next === data.text) {
      editing = false;
      return;
    }
    if (sessionStore.active === null) {
      editing = false;
      return;
    }
    const maxChars = settingsStore.text_label_max_chars;  // R7
    const derivedLabel = next.slice(0, maxChars).trim();
    const result = await sessionStore.applyMutation(
      (cur) => ({
        ...cur,
        items: cur.items.map((it: CanvasItem) =>
          it.id === data.id && it.type === 'text'
            ? ({ ...it, text: next, label: derivedLabel } as TextItem)
            : it,
        ),
      }),
      { failMessage: 'Text commit failed' },
    );
    if (result.ok) editing = false;
  }
</script>

<div
  class="text-node ..."
  style="
    font-size: {data.font_size}px;
    color: {data.color};
    text-align: {textAlign};
    font-weight: {fontWeight === 'bold' ? 700 : 400};
    font-style: {fontStyle};
    text-decoration: {textDecoration};
  "
  ...
>
```

### B6. `lib/canvas/Canvas.svelte::onnodeclick` 의 tool forward

위치: 918-930.

```ts
function onnodeclick({ node, event }: { node: Node; event: MouseEvent | TouchEvent }) {
  // Select mode 의 기존 동작 — single / meta toggle.
  if (isSelectMode) {
    const id = node.id;
    const isModifierClick =
      event instanceof MouseEvent &&
      event.metaKey;
    if (isModifierClick) {
      sessionStore.toggleM(id);
    } else {
      sessionStore.setM([id]);
    }
    return;
  }
  // R4 — tool active 시 node click 도 onpaneclick 의 spawn 로직 forward.
  // 단 drag-tool 은 별 pointer handler 가 처리하므로 onnodeclick 까지 안 옴 —
  // point-spawn tool 만 분기.
  if (event instanceof MouseEvent) {
    onpaneclick({ event });
  }
}
```

### B7. `lib/canvas/XtermHost.svelte` clipboard 차단

위치: 88-103 (현 mount effect 의 `new Terminal` 직후).

```ts
// R5 — terminal clipboard 차단 (ADR-0004 amend, 2026-05-20).
term.attachCustomKeyEventHandler((e: KeyboardEvent) => {
  // Cmd/Ctrl+C / X / V 모두 차단 (selection 유무 무관). SIGINT 의 별 entry P1.
  const mod = e.metaKey || e.ctrlKey;
  if (mod && !e.altKey) {
    const k = e.key.toLowerCase();
    if (k === 'c' || k === 'x' || k === 'v') return false;
  }
  return true;
});

if (containerEl) {
  const blockEvent = (e: Event) => { e.preventDefault(); e.stopPropagation(); };
  // capture-phase 차단 — xterm helper-textarea 의 native paste 이벤트보다 먼저 받음
  containerEl.addEventListener('paste',    blockEvent, { capture: true });
  containerEl.addEventListener('copy',     blockEvent, { capture: true });
  containerEl.addEventListener('cut',      blockEvent, { capture: true });
  containerEl.addEventListener('drop',     blockEvent, { capture: true });
  containerEl.addEventListener('dragover', blockEvent, { capture: true });
}

// cleanup ($effect return) 에 unsubscribe 추가:
return () => {
  // ... 기존 ...
  if (containerEl) {
    containerEl.removeEventListener('paste', blockEvent, { capture: true });
    // ... 4 더 ...
  }
};
```

(주의: `blockEvent` 가 closure 변수라 listener 제거 시 *동일 reference* 가 필요. `const blockEvent` 를 `$effect` 안 localvar 로 두고 cleanup 에서 같은 reference 로 제거.)

### B8. `lib/canvas/NoteNode.svelte::onContentDblClick`

위치: 65-77 (현 onTitleDblClick + onBodyDblClick).

```ts
function onContentDblClick(e: MouseEvent): void {
  if (isLocked || isMinimized) return;
  // 자식 button 또는 그 svg 는 자체 click handler 가 처리 — skip
  const target = e.target as HTMLElement | null;
  if (target === null) return;
  let cursor: HTMLElement | null = target;
  while (cursor !== null && cursor !== e.currentTarget) {
    if (cursor.tagName === 'BUTTON') return;
    cursor = cursor.parentElement;
  }
  e.stopPropagation();
  // head row 안인지 확인 (note-head class 의 부모 체인 안)
  cursor = target;
  while (cursor !== null && cursor !== e.currentTarget) {
    if (cursor.classList?.contains('note-head')) {
      titleEditing = true;
      return;
    }
    cursor = cursor.parentElement;
  }
  // 그 외 (body / padding / gap) → bodyEditing
  bodyEditing = true;
}
```

`.note-node` div 의 `ondblclick={onContentDblClick}` 추가. 기존 `.note-label` 의 `ondblclick={onTitleDblClick}` 과 `.note-body-wrap` 의 `ondblclick={onBodyDblClick}` 은 *유지* (자체 zone 의 일관성) — 또는 *제거* (root 가 다 받음). **권장**: 자식 listener 제거 — root 1곳만. event.stopPropagation 정합 유지.

### B9. `lib/stores/sessionStore.svelte.ts` — `justSpawnedTextId`

위치: store class 의 새 field.

```ts
class SessionStore {
  // ... 기존 ...
  /** R7 — 새로 spawn 된 text item id. TextNode 가 mount 시 자동 edit 진입 후 clear. */
  justSpawnedTextId = $state<string | null>(null);

  clear(): void {
    // ... 기존 ...
    this.justSpawnedTextId = null;
  }
}
```

### B10. `lib/canvas/itemFactory.ts::commitNewItem` 의 text 분기

위치: 346-363.

```ts
export async function commitNewItem(item: CanvasItem): Promise<CanvasItem | null> {
  if (sessionStore.active === null) return null;
  let committed: CanvasItem = item;
  const result = await sessionStore.applyMutation(...);
  if (!result.ok) return null;
  sessionStore.setM([committed.id]);
  // R7 — text item 의 auto-edit 진입 signal.
  if (committed.type === 'text') {
    sessionStore.justSpawnedTextId = committed.id;
  }
  return committed;
}
```

### B11. `lib/stores/settings.svelte.ts` — `text_label_max_chars`

위치: settingsStore 의 field 추가.

```ts
class SettingsStore {
  // ... 기존 ...
  text_label_max_chars = $state<number>(24);  // BE 의 default 24 와 정합

  // GET /api/settings 의 응답 hydrate 시:
  applyFromServer(s: ServerSettings): void {
    // ... 기존 ...
    this.text_label_max_chars = s.text_label_max_chars ?? 24;
  }

  // PUT /api/settings 의 body build 시:
  toRequest(): ServerSettings {
    return {
      // ... 기존 ...
      text_label_max_chars: this.text_label_max_chars,
    };
  }
}
```

### B12. `lib/chrome/SettingsOverlay.svelte` — Canvas section 의 신규 row

위치: Settings overlay 의 "Canvas" 또는 "Editor" section.

```svelte
<section class="settings-section">
  <h3>Text</h3>
  <div class="settings-row">
    <label for="text-label-max">Auto-derived label max chars</label>
    <input
      id="text-label-max"
      type="number"
      min="1" max="128"
      value={settingsStore.text_label_max_chars}
      onchange={(e) => {
        const v = Math.max(1, Math.min(128, Math.round(Number(e.currentTarget.value))));
        settingsStore.text_label_max_chars = v;
        void settingsStore.saveToServer();
      }}
    />
    <span class="hint">First N chars of text are used as item label.</span>
  </div>
</section>
```

### B13. `lib/canvas/clipboardOps.svelte.ts::cloneWithOffset` terminal 분기

위치: 53-74 (현 cloneWithOffset).

```ts
function cloneWithOffset(src: CanvasItem, bboxX, bboxY, dx, dy): CanvasItem {
  const clone = structuredClone($state.snapshot(src)) as CanvasItem;
  const out = {
    ...clone,
    id: crypto.randomUUID(),
    x: bboxX + dx + (src.x - bboxX),
    y: bboxY + dy + (src.y - bboxY),
  } as CanvasItem;
  if (out.type === 'line') {
    const lineSrc = src as LineItem;
    const lineOut = out as LineItem;
    lineOut.x2 = lineSrc.x2 + dx;
    lineOut.y2 = lineSrc.y2 + dy;
  }
  // ADR-0030 D3 amend ① (2026-05-20 batch-5, R8) — terminal paste = clone-spawn,
  // 크기·좌표·visibility·locked·z·parent_id 만 source 보존. 나머지는 fresh.
  if (out.type === 'terminal') {
    out.label = '';
    out.description = '';
    out.minimized = false;
    // restored_geom 은 옵셔널 — 본 batch 의 schema 에 아직 없으면 무시
    // (delete (out as any).restored_geom; — Type 시그너처 보존 위해 narrow cast)
    if ('restored_geom' in out) {
      // @ts-expect-error — optional field; ADR-0018 D11 draft
      out.restored_geom = undefined;
    }
  }
  return out;
}
```

---

## §C. API contract — FE → BE

본 batch 의 *FE 측 API 호출* 은 모두 **기존 endpoint 의 신규 field 추가**. 새 endpoint 0.

### C1. `PUT /api/sessions/:name/layout`

- 신규 field (`fill_enabled`, `stroke_enabled`, `corner_radius`, `stroke_dash`, `font_weight`, `font_style`, `underline`, `strikethrough`) 포함 가능.
- ETag 정합 (`If-Match`) 동일.
- 응답 400 — `rect_corner_radius_exceeds_box` 신규 ValidationError code.

### C2. `GET/PUT /api/settings`

- `text_label_max_chars: number` (1 ≤ value ≤ 128) 신규 키.
- 응답 GET: 옛 settings 도 default 24 로 hydrate (BE serde default).
- PUT 의 range 위반 → 400.

### C3. FE 신규 store flag (server 무관)

- `sessionStore.justSpawnedTextId: string | null` — page lifetime 안. 새로고침 시 자동 null (이미 store reset).

---

## §D. Acceptance criteria

### D-1. ShapeNode SVG migration (FE-B)

| # | 검증 | 기대 |
|---|---|---|
| AC-FB1 | `pnpm check` | 0 errors |
| AC-FB2 | `pnpm build` | OK |
| AC-FB3 | `grep -c "<rect" codebase/frontend/src/lib/canvas/ShapeNode.svelte` | ≥1 |
| AC-FB4 | manual E2E — rect 생성 → Inspector 에서 corner_radius 8 입력 → 둥근 모서리 표시 | screenshot |
| AC-FB5 | manual E2E — rect 의 fill toggle off → 내부 영역 클릭 시 *뒤 panel 이 selectable* (rect 미선택) | manual |
| AC-FB6 | manual E2E — rect 의 stroke toggle off → border 사라짐 + fill 만 hit | manual |
| AC-FB7 | manual E2E — rect/ellipse/line 모두 stroke_dash="dash_dot" → dashdot 표시 | screenshot |

### D-2. Text full style (FE-E)

| # | 검증 | 기대 |
|---|---|---|
| AC-FE1 | Inspector 의 B/I/U/S 4 button click → TextNode inline style 변경 | manual |
| AC-FE2 | underline + strikethrough 동시 ON → CSS `text-decoration: underline line-through` 적용 | DevTools |
| AC-FE3 | layout PUT 후 GET → 4 field 모두 정합 | curl |

### D-3. Tool gating (FE-F)

| # | 검증 | 기대 |
|---|---|---|
| AC-FF1 | text tool active → 기존 panel 위 click → 새 text 생성 (위에 + selection 변경 없음) | manual |
| AC-FF2 | rect tool drag → 기존 panel 위 시작 → drag 정상 종료 → 새 rect 생성 + 기존 panel 선택 안 됨 | manual |
| AC-FF3 | hand tool active → 어디 click 해도 marquee selection 안 됨 | manual |
| AC-FF4 | select tool 의 기존 single / meta-toggle 동작 그대로 | manual |

### D-4. Xterm clipboard (FE-G)

| # | 검증 | 기대 |
|---|---|---|
| AC-FG1 | terminal focus + Cmd/Ctrl+V → OS clipboard 안 paste, terminal 에 아무것도 안 들어감 | manual |
| AC-FG2 | terminal focus + 글자 selection + Cmd/Ctrl+C → OS clipboard 비변경 (DevTools 의 clipboardData 확인) | manual |
| AC-FG3 | terminal area right-click → context menu paste 안 나타나거나 paste 안 됨 | manual |
| AC-FG4 | terminal area drag-drop text → 안 paste | manual |
| AC-FG5 | terminal 영역 *밖* 의 canvas 에서 Cmd+V → FE clipboard 의 panel paste 정상 동작 (R5 가 R8 fail 시키지 않음) | manual |

### D-5. Note dblclick (FE-H)

| # | 검증 | 기대 |
|---|---|---|
| AC-FH1 | note 의 head row 더블클릭 → title editing | manual |
| AC-FH2 | note 의 body 영역 더블클릭 → body editing | manual |
| AC-FH3 | note 의 padding/gap 영역 더블클릭 → body editing (default) | manual |
| AC-FH4 | note 의 close/minimize button 더블클릭 → 자체 click handler 발화, edit 진입 X | manual |

### D-6. Text auto-edit + label sync (FE-I, FE-J)

| # | 검증 | 기대 |
|---|---|---|
| AC-FI1 | text tool 클릭 → canvas click → 즉시 InlineEditTextarea focus (cursor 깜빡) | manual |
| AC-FI2 | empty 상태에서 ESC → editing=false, placeholder 표시, item 보존 | manual |
| AC-FI3 | 빈 공간 click → editing=false (blur), placeholder 표시 | manual |
| AC-FI4 | "Hello world" 입력 + Enter commit → Inspector 의 label = "Hello world" (slice ≤ max_chars) | manual |
| AC-FI5 | Settings 의 text_label_max_chars = 5 로 변경 → 새 text 입력 "Hello world" → label = "Hello" | manual |
| AC-FI6 | Settings text_label_max_chars 입력 130 → input 자동 clamp 128 + PUT 400 | manual |

### D-7. Terminal paste filter (FE-K)

| # | 검증 | 기대 |
|---|---|---|
| AC-FK1 | terminal panel 의 label = "build watch" + Cmd+C → Cmd+V → 새 terminal panel 의 label = "" (fresh) | manual |
| AC-FK2 | minimize 한 terminal 을 copy → paste → 새 terminal 은 minimized=false (full size) | manual |
| AC-FK3 | description 가 있는 terminal copy → paste → 새 terminal description="" | manual |
| AC-FK4 | size (w, h) 는 source 와 동일 | manual |
| AC-FK5 | 새 terminal 의 backend spawn 정상 (X session active 시 attachConfirm 의 unmatched-spawn) | manual |

### D-8. 통합

| # | 검증 | 기대 |
|---|---|---|
| AC-G1 | `pnpm check` | 0 errors / 0 warnings |
| AC-G2 | `pnpm build` | OK |
| AC-G3 | BE 의 release build + FE dev server + browser E2E 시나리오 — 8 요구 모두 작동 | manual + screenshot |
| AC-G4 | 옛 layout (신규 field 모두 없음) load → 모든 item default state — 시각 차이 0 | manual |

---

## §E. Anti-pattern — *하지 말 것*

1. **`fill_enabled` 의 default false**: 옛 record 의 의미 변경 — 모든 사용자가 새로고침하면 shape 가 invisible. **default true 필수**.
2. **`pointer-events: none` 을 `.shape-node` div 에 적용**: NodeResizer 도 안에 있어 resize 핸들도 안 됨. **SVG element 의 pointer-events attribute 만**.
3. **CSS `border-style: dashed` 를 SVG 와 혼용**: dashdot 표현 불가 + CSS border-radius 와 stroke-dasharray 결합 시 corner rendering 깨짐. **SVG 단독**.
4. **Inspector 의 toggle 이 ColorPicker 와 *별 mutation***: 2 PUT — ETag conflict 위험 + history 2 entry. *하나의 applyMutation* 안에 묶음.
5. **text label auto-derive 가 사용자 typing 마다 발화**: PUT 폭주. **commit 시점에만** + applyMutation 1 entry.
6. **xterm `attachCustomKeyEventHandler` 만 사용**: right-click paste / middle-click primary selection 등 OS-level paste 가 살아남음. **DOM event capture 도 필수**.
7. **`drop` / `dragover` 차단 누락**: drag-drop text 가 xterm 으로 paste 됨. **둘 다 차단**.
8. **Note dblclick 이 `pointerdown` event 의 onclick chain 흡수**: 일반 click 으로 selection 변경 + dblclick 으로 edit — 두 event 분리. dblclick 만 stopPropagation, click 은 자연 전파.
9. **TextNode 의 mount $effect 안 store flag *read 만*, clear 안 함**: 한 번 spawn 후 다른 text item mount 시 잘못 발화. **read + 즉시 clear**.
10. **Terminal paste 시 source 의 `terminal_id` (= item.id) 도 보존**: BE 의 match-or-spawn 이 *기존 terminal 과 mirror* — 사용자 의도와 다름. **id 는 새 UUID 강제** (cloneWithOffset 가 이미 적용 — 주의 유지).
11. **`SECURE_XTERM_OPTIONS` 의 `disableStdin: true`**: 키 입력 자체 차단 → 사용자가 terminal 사용 불가. **clipboard 만 차단, stdin 은 유지**.
12. **No-session 상태에서 settings PUT**: ADR-0017 의 no-session gating 위반. **Settings overlay 의 input 은 disabled gating + 직접 store mutation 차단**.

---

## §F. Test plan

### F-1. 개별 단위 test (vitest)

| Test | 영역 | 검증 |
|---|---|---|
| `clipboardOps.cloneWithOffset_terminal_filters_metadata` | clipboardOps | R8 — label / description / minimized fresh |
| `clipboardOps.cloneWithOffset_terminal_preserves_size` | clipboardOps | R8 — w / h / x / y / z / parent_id / visibility / locked |
| `clipboardOps.cloneWithOffset_other_types_preserves_all` | clipboardOps | non-terminal 의 full clone |
| `itemFactory.createCanvasItem_text_initial_label_empty` | itemFactory | R7 prerequisite — fresh text 의 label "" |
| `settingsStore.text_label_max_chars_clamp_1_to_128` | settings | R7 — input 의 clamp |
| `xtermClipboard.helper_event_blocked` (mocked) | xtermHost | R5 — addEventListener spy + dispatchEvent → preventDefault 호출 확인 |

총 신규 6 unit test.

### F-2. Component test (vitest + @testing-library/svelte)

| Test | 영역 | 검증 |
|---|---|---|
| `ShapeNode_renders_svg_rect_with_corner_radius` | ShapeNode | DOM 의 `<rect rx="8">` 확인 |
| `ShapeNode_fill_disabled_means_pointer_events_visibleStroke` | ShapeNode | hit-test attribute 정합 |
| `ShapeNode_stroke_dash_dashdot_maps_to_stroke_dasharray` | ShapeNode | SVG attribute 매핑 |
| `TextNode_renders_bold_italic_underline` | TextNode | inline style 의 font-weight/font-style/text-decoration 정합 |
| `TextNode_auto_edit_on_just_spawned` | TextNode | store flag set → mount → editing=true |
| `NoteNode_padding_dblclick_enters_body_edit` | NoteNode | padding 영역 dblclick → bodyEditing=true |
| `NoteNode_head_dblclick_enters_title_edit` | NoteNode | head 영역 dblclick → titleEditing=true |

총 신규 7 component test.

### F-3. Integration test (Playwright via browse CLI — manual baseline)

각 AC 의 manual E2E 시나리오를 browse 의 reproducible 스크립트로 자동화 (별 sprint — 본 batch 는 manual baseline 만 capture).

### F-4. 통합 검증

```bash
cd codebase/frontend
pnpm check       # 0 errors / 0 warnings
pnpm build       # OK
pnpm test        # 신규 13 test PASS

# Manual E2E (BE demo running)
/Users/ws/Desktop/projects/termcanvas/dist-cli/browse goto http://localhost:9998/auth?t=<token>
# → 8 AC 시나리오 순차 실행
```

---

## §G. Verification 순서

1. **BE-A/B/C/D/E land + BE-F (typings regenerate)** 후 진입.
2. `pnpm gen-types` (또는 동등 — `lib/types/api.d.ts` 가 새 field 포함되어 있어야).
3. FE-A (types.ts amend) → `pnpm check` (0 errors).
4. FE-B + FE-C (ShapeNode SVG + Line dash) → AC-D1.
5. FE-D (Inspector rect/ellipse section) → AC-D1 의 Inspector 부분.
6. FE-E (TextNode font + Inspector 4 toggle) → AC-D2.
7. FE-F (Canvas onnodeclick forward) → AC-D3.
8. FE-G (XtermHost clipboard 차단) → AC-D4.
9. FE-H (NoteNode dblclick zone) → AC-D5.
10. FE-I + FE-J (TextNode auto-edit + Settings) → AC-D6.
11. FE-K (clipboardOps terminal filter) → AC-D7.
12. AC-D8 통합 검증.

---

## §H. Commit 분리 권장

| Commit | 내용 |
|---|---|
| `feat(fe/types): batch-5 rect/ellipse/line/text payload field extensions (ADR-0018 D4 amend ①/②)` | FE-A — types.ts 의 신규 field + FigureStrokeDash enum |
| `feat(fe/canvas): batch-5 ShapeNode SVG renderer + fill/stroke enabled + corner_radius + dash (R1+R2)` | FE-B + FE-C — ShapeNode SVG + LineNode dash + 공통 strokeDash helper |
| `feat(fe/chrome): batch-5 Inspector shape on/off toggle + corner_radius + stroke_dash (R1+R2)` | FE-D — ItemInfoView rect/ellipse section |
| `feat(fe/canvas+chrome): batch-5 TextNode font_weight/font_style/underline/strikethrough + Inspector 4 toggle (R3)` | FE-E |
| `feat(fe/canvas): batch-5 tool active node click forward to spawn (R4)` | FE-F |
| `feat(fe/xterm): batch-5 clipboard 차단 — paste/copy/cut/drop + key handler (R5, ADR-0004 amend)` | FE-G |
| `feat(fe/canvas): batch-5 NoteNode content area dblclick zone 확장 (R6, ADR-0018 D9 amend)` | FE-H |
| `feat(fe/text+settings): batch-5 text auto-edit on spawn + label auto-derive + Settings text_label_max_chars (R7)` | FE-I + FE-J |
| `feat(fe/clipboard): batch-5 terminal paste payload filter — size only re-use (R8, ADR-0030 D3 amend ①)` | FE-K |

총 9 commit. 또는 R1+R2 묶음 / R3 / R4 / R5 / R6 / R7 / R8 의 7 logical batch 로 묶음 가능. **권장 묶음**: 7 commit (logical), 각 commit 에 관련 ADR amend 동봉.

---

## §I. 의존성

- **Prerequisite**:
  1. BE handover (`2026-05-20-be-handover-ui-ux-batch-5.md`) 의 BE-A~F 모두 land + release binary 갱신 (mtime check).
  2. `pnpm install` 후 typings regenerate (`lib/types/api.d.ts` 의 새 field 확인).
- **Independent**:
  - 0079 connector FE batch 와 file 충돌 0 (Connector 는 별 ShapeNode 영역 + ConnectorEdge 신규).
  - 0080 asset upload FE batch (image/document) 와 file 충돌 0.
- **현재 unstaged 영역** (XtermHost minimize buffer 보존, `0079-be-handover-connector.md` 미발화) — 본 batch 의 FE-G (XtermHost clipboard 차단) 와 *같은 파일*. **commit 순서**: unstaged 의 minimize fix 를 먼저 commit + ADR-0021 D16 amend → 그 후 FE-G 진입. 또는 *두 fix 묶음 한 commit* 도 가능하나 logical 영역이 다름 (minimize lifecycle vs clipboard 차단) — 분리 권장.

---

## §J. Self-check 표

- [ ] BE-A~F 모두 land + release binary mtime 갱신
- [ ] `lib/types/api.d.ts` 의 새 field 확인 (FigureStrokeDash / FontWeight / FontStyle / fill_enabled 등)
- [ ] FE-A — canvas.ts 의 type 확장 + type guard 추가
- [ ] FE-B/C — ShapeNode SVG migration + Line stroke_dash
- [ ] FE-D — Inspector rect/ellipse fill/stroke on/off + corner_radius + dash
- [ ] FE-E — TextNode 4 boolean inline style + Inspector 4 toggle
- [ ] FE-F — Canvas onnodeclick 의 tool forward (spawn-on-node)
- [ ] FE-G — XtermHost paste/copy/cut/drop/dragover 차단 + attachCustomKeyEventHandler
- [ ] FE-H — NoteNode root dblclick zone + target 분기
- [ ] FE-I — TextNode auto-edit 진입 + label auto-derive (commit 시점)
- [ ] FE-J — Settings text_label_max_chars row + saveToServer wire
- [ ] FE-K — clipboardOps cloneWithOffset terminal 분기 (size only re-use)
- [ ] `pnpm check` 0 errors
- [ ] `pnpm build` OK
- [ ] 13 신규 unit + component test PASS
- [ ] 모든 8 AC manual E2E PASS
- [ ] ADR amend 동봉 — ADR-0018 D4/D8/D9, ADR-0017 §Settings/§Toolbar2, ADR-0030 D3, ADR-0004

---

## 변경 이력

- 2026-05-20: 초안. 8 UI/UX 요구의 FE-side 구현 (R1~R8). BE 짝 (`2026-05-20-be-handover-ui-ux-batch-5.md`) 의 prerequisite 후 진입. 7 logical batch 묶음 권장 (Shape/Text/Tool/Xterm/Note/Text-auto-edit/Terminal-paste). 13 신규 test + 8 AC.
- 2026-05-20 (Grill #1-19 amend): 본 doc 상단의 ⚠️ amend 표 적용. **FE-G 폐기** (XtermHost clipboard 차단 — R5 폐기로 무관). **FE-J 폐기** (Settings UI — R7 의 Settings 신설 안 함). **FE-K test-only** (기존 cloneWithOffset 이 사용자 의도 매칭). `corner_radius` 의 *수치 input* → `corner_rounded` **Toggle 한 row** + FE 자동 radius 계산. FontWeight 3-segment + I/U/S toggle. font_size InspectorField + color ColorPicker 신규. NoteNode dblclick zone = *body 영역만*, MaximizedItemModal 안 동일. text derive = `text.split('\n', 1)[0].trim().slice(0, 4000)` + label-empty trigger. justSpawnedTextId 의 `untrack` 안 write. 총 5~6 commit. sister doc `2026-05-20-design-handover-ui-ux-batch-5.md` 가 신규 컴포넌트 design 정합.
