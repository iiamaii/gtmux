# 2026-05-20 — Design Handover: UI/UX Batch 5 — 신규 컴포넌트 시각 + 위치 + 동작

- 작성일: 2026-05-20
- 작성 주체: agent (frontend-architect role, UI design specialist hat)
- 정본 cross-link:
  - **상위 분석**: [`2026-05-20-ui-ux-batch-5-analysis.md`](./2026-05-20-ui-ux-batch-5-analysis.md) (요구·결정)
  - **FE handover 짝**: [`2026-05-20-fe-handover-ui-ux-batch-5.md`](./2026-05-20-fe-handover-ui-ux-batch-5.md) (구현 anchor + AC)
  - **BE handover 짝**: [`2026-05-20-be-handover-ui-ux-batch-5.md`](./2026-05-20-be-handover-ui-ux-batch-5.md) (schema)
  - **결정 출처 ADR**: ADR-0016 (design tokens + iconography), ADR-0017 (layout grid + chrome + Toolbar2 + Inspector), ADR-0027 (multi-select inspector + alignment)
  - **참조 reference**: `ref/frontend-design/components-v5.html` + `components-v6.html`
  - **현 Inspector**: `codebase/frontend/src/lib/chrome/ItemInfoView.svelte` (1481 라인)

## 핵심 원칙 — Design 일관성 + 사용자 인지 부담 최소화

- **ADR-0016 design tokens 정합** — `--color-fg`, `--color-fg-muted`, `--color-accent`, `--color-border`, `--color-surface`, `--color-surface-2`, `--radius-sm`, `--radius-md`, `--space-*`, `--font-mono`, `--font-sans` 만 사용. 직접 색 값 hardcoding 금지 (단 design token 정의의 *값* 부분만).
- **ADR-0017 §D6 (Inspector v2 패턴)** — `.prop-section` (h4 head + rows) / `.prop-row` (full or 2-col) / `.display-row` (k + val) / `.picker` 의 기존 grid 정합.
- **No-session UI gating** — `sessionStore.active === null` 시 모든 신규 control disabled (ADR-0017 신규 invariant).
- **Locked item gating** — `it.locked === true` 시 *값 변경 control* disabled (Toggle / ColorPicker / dropdown / input). 단 *읽기* 는 가능.
- **Multi-select 정합** (ADR-0027) — `M.size > 1` 시 *동일 type* 의 *동일 값* 만 노출, mixed value 는 placeholder. 본 batch 의 신규 control 도 동일 패턴.
- **Visual disrupt 최소** — 신규 row 는 *기존 row 패턴* 그대로 (h4 + .prop-row). 새 widget type (segmented control / toggle) 만 기존 패턴 확장.

---

## §0. 신규 컴포넌트 매트릭스

본 batch 가 *신규* 도입하는 UI primitive + 사용 위치:

| Primitive | 이름 | 사용 위치 | 의도 |
|---|---|---|---|
| **Toggle (switch)** | `Toggle.svelte` | Inspector — `fill_enabled` / `stroke_enabled` / `corner_rounded` / `italic` / `underline` / `strikethrough` (Inspector text-section 4개 + shape-section 3개) | binary on/off |
| **3-segment control** | `SegmentedControl.svelte` 또는 inline buttons | Inspector text-section `font_weight` (L/N/B) | mutually-exclusive 3-state |
| **Dropdown select** | 기존 `<select>` 또는 `Dropdown.svelte` (이미 있음) | Inspector shape-section `stroke_dash` (4 option) | mutually-exclusive 4-state |
| **Number InspectorField** | 기존 `InspectorField.svelte` | text-section `font_size`, shape-section `stroke_width` | bounded number input |

⚠️ *별 신규 component 0* — Toggle 만 신규. 나머지는 *기존 primitive 활용* 또는 *inline DOM 으로 충분*.

---

## §1. Toggle Component — 신규 (`Toggle.svelte`)

### 1.1 위치

`codebase/frontend/src/lib/ui/Toggle.svelte` (신규).

### 1.2 시각 / 형태

```
[OFF]                 [ON]
┌────────────┐        ┌────────────┐
│  ○         │        │         ○  │
└────────────┘        └────────────┘
  border-muted          accent
  surface bg            accent bg
```

- **크기**: width 28px × height 16px (compact, Inspector row 의 *24px row height* 안 정합).
- **knob**: 12×12 circle. 좌끝 OFF, 우끝 ON. 4px transition (CSS `transition: transform 120ms ease`).
- **OFF state**:
  - background: `var(--color-surface-2)` (light theme: pale gray)
  - border: 1px `var(--color-border)`
  - knob fill: `var(--color-fg-muted)`
- **ON state**:
  - background: `var(--color-accent)` (Figma signature blue `#0d99ff`)
  - border: 1px `var(--color-accent)`
  - knob fill: `#FFFFFF` (high contrast on accent bg)
- **Disabled state**: `opacity: 0.4` + `cursor: not-allowed`. ON/OFF 모두 동일 dim.
- **Focus**: 사라진 dashed accent ring 정책 정합 — `:focus-visible { outline: none }` (현 *No focus-ring* 정책 그대로).

### 1.3 Props 인터페이스

```svelte
<script lang="ts">
  let {
    checked = false,
    disabled = false,
    ariaLabel,
    onchange,
  }: {
    checked: boolean;
    disabled?: boolean;
    ariaLabel: string;
    onchange?: (next: boolean) => void;
  } = $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={ariaLabel}
  {disabled}
  onclick={() => !disabled && onchange?.(!checked)}
  class="toggle"
  class:on={checked}
>
  <span class="knob" />
</button>
```

### 1.4 Mixed-value state (multi-select)

`M.size > 1` 의 mixed 시 *3rd visual state* — half ON / half OFF 의 striped knob:
- knob 의 fill 이 `var(--color-fg-muted)` (dimmed)
- knob 위치는 *중앙* (translate(-50%, 0))
- aria: `aria-checked="mixed"`
- click 시 *모두 ON* 으로 통일 (Figma 패턴).

---

## §2. 3-Segment Control — Inline pattern

### 2.1 위치

신규 component 미생성 — Inspector 의 text-section 안 *inline group* 으로 직접 구현. 또는 기존 align-group 패턴 재활용.

### 2.2 시각 / 형태

```
[ L ][ N ][ B ]
```

- 3 button 의 *segmented* — 인접 border share, 좌/우 끝만 rounded.
- 각 button: width 28px × height 22px, 텍스트 (L / N / B) 가 center.
- **Active state** (selected): background `var(--color-accent)` + text `#FFFFFF` + bold (segmented control 자체의 *기능 표시*, *bold 자체* 와는 분리).
- **Inactive**: background `var(--color-surface-2)` + text `var(--color-fg)` (regular weight).
- **Disabled**: `opacity: 0.4`.
- **Hover (inactive)**: background `var(--color-glass-1)`.
- **Mixed value** (multi-select): 3 button 모두 *inactive* + tooltip "Mixed".
- font: `var(--font-mono)` (consistency with mono labels in head). letter-spacing 0.5px.

### 2.3 사용 위치

ItemInfoView.svelte 의 text-section 의 *font-weight row*:

```
[L|N|B]   weight
```

- 좌측 segmented control (28px×3 = 84px width).
- 우측 *label* "weight" (font-size 11px, font-muted, mono).

### 2.4 동작

- 클릭 → `applyTextEnum('font_weight', 'light'|'normal'|'bold')` mutation.
- Multi-select 의 mixed → click 으로 *그 value* 로 통일.

---

## §3. Inspector — Rect / Ellipse section (R1 + R2)

### 3.1 위치

`codebase/frontend/src/lib/chrome/ItemInfoView.svelte:768-789` 의 *기존 stroke / fill section* 위치를 *확장*.

### 3.2 Layout — 새 row 매트릭스

```
┌────────────────────────────────────────┐
│ ▾ Item Payload                         │  ← 기존 h4 head
├────────────────────────────────────────┤
│ fill        [Toggle]  [ColorPicker]    │  ← Row 1 (rect+ellipse)
├────────────────────────────────────────┤
│ stroke      [Toggle]  [ColorPicker]    │  ← Row 2 (rect+ellipse+line)
├────────────────────────────────────────┤
│ width  [  2  ]   dash  [Solid ▾]       │  ← Row 3 (rect+ellipse+line, 2-col)
├────────────────────────────────────────┤
│ rounded     [Toggle]                   │  ← Row 4 (rect-only)
└────────────────────────────────────────┘
```

### 3.3 각 row 의 시각 + 동작

#### Row 1 — fill on/off + color

```
fill         [○─────] [▢ #D9D9D9 ▾]
             OFF       ColorPicker
```

- `fill` label: `font-mono`, `font-size: 11px`, `color: var(--color-fg-muted)`, `letter-spacing: 0.4px`.
- Toggle (§1): default ON state false, click → `applyShapeBoolean('fill_enabled', !cur)`.
- ColorPicker: 기존 component 그대로. `disabled={!fill_enabled}` 시 *opacity 0.4 + click no-op* (또는 row 전체 *render 하지 않음* — 사용자 verbatim "off 면 inspector 노출 X").
- **결정**: Toggle 만 행 안 *항상 노출*. ColorPicker 는 *fill_enabled=true 일 때만 render* (사용자 verbatim 정합).

#### Row 2 — stroke on/off + color (rect/ellipse/line 공통)

```
stroke       [─────○] [▢ #FFFFFF ▾]
             ON        ColorPicker
```

- Toggle: default ON. click → `applyShapeBoolean('stroke_enabled', !cur)`.
- ColorPicker: `stroke_enabled=true` 일 때만 render.

#### Row 3 — stroke width + dash (rect/ellipse/line, 2-col)

```
width  [    2 ]      dash  [Dash ▾]
       number input        dropdown
```

- 2-col grid (`.prop-row` default 2-col 패턴).
- *width* `InspectorField` (number, k="width", min=1, max=32, step=1).
  - Out-of-range 시 자동 clamp + BE strict reject 시 toast "Stroke width must be 1~32".
- *dash* `<select>` 4 option:
  - `<option value="solid">Solid</option>`
  - `<option value="dash">— — —</option>` (visual hint)
  - `<option value="dot">• • •</option>` (visual hint)
  - `<option value="dash_dot">— • —</option>` (visual hint)
  - Default option visual hint 없이 *text label* 만으로도 OK (단순 우선).
- *조건부 render*: `stroke_enabled=true` 일 때만. off 시 row 전체 hide.

#### Row 4 — corner rounded (rect-only)

```
rounded      [───○─]
             Toggle only — 수치 input 없음
```

- *rect 만* render (sessionItem.type === 'rect').
- Toggle: default OFF. click → `applyShapeBoolean('corner_rounded', !cur)`.
- *수치 input 없음* (Grill #5). 자동 radius = `clamp(min(w,h)*0.15, 4, 16)`.

### 3.4 Multi-select 정합

- `M.size > 1` 이고 *type 동일* (rect/rect, ellipse/ellipse 등):
  - Toggle 의 mixed state 표시 (§1.4).
  - ColorPicker 의 mixed (기존 패턴 — *Mixed* placeholder).
  - dash 의 mixed → dropdown 의 *empty selected* + placeholder "Mixed".
- `M.size > 1` 이고 *type mixed* (rect + ellipse + ...):
  - rect-only 의 `corner_rounded` row 는 *숨김*.
  - 공통 row (fill on/off, stroke on/off, width, dash) 만 표시. *공통 type group* 의 *공통 field* 만.

---

## §4. Inspector — Text section (R3 + R7)

### 4.1 위치

`codebase/frontend/src/lib/chrome/ItemInfoView.svelte:820-933` 의 기존 *text section* (chars / align / v-align) 위 확장.

### 4.2 Layout — 새 row 매트릭스

```
┌────────────────────────────────────────┐
│ ▾ Item Payload (text)                  │
├────────────────────────────────────────┤
│ chars       12                         │  ← 기존
├────────────────────────────────────────┤
│ size   [   16 ]    color [▢ #333 ▾]    │  ← Row A (신규, 2-col)
├────────────────────────────────────────┤
│ [L|N|B]   weight                       │  ← Row B (신규)
├────────────────────────────────────────┤
│ [I][U][S]   style                      │  ← Row C (신규)
├────────────────────────────────────────┤
│ align    [L|C|R]                       │  ← 기존
├────────────────────────────────────────┤
│ v-align  [T|M|B]                       │  ← 기존
└────────────────────────────────────────┘
```

### 4.3 각 신규 row 의 시각

#### Row A — font size + color (2-col)

```
size  [    16 ]      color  [▢ #333 ▾]
      number input          ColorPicker
```

- `size` InspectorField (k="size", number, min=8, max=96, step=1).
- `color` ColorPicker (기존, allowAlpha={true}, allowTransparent={false} — text 는 transparent 불가).
- *commit*: number 의 blur 또는 Enter 시 `applyTextNumber('font_size', value)`.

#### Row B — font weight 3-segment

```
[L|N|B]   weight
84px×22px  font-mono 11px label
```

- §2 의 segmented control inline.
- click → `applyTextEnum('font_weight', 'light'|'normal'|'bold')`.

#### Row C — italic / underline / strikethrough 3 toggle

```
[I][U][S]   style
3×28px×22px  font-mono 11px label
```

- 3 button group (segmented-style이지만 각 button 이 *독립* — multi-toggle, not mutually-exclusive).
- 각 button: width 28px × height 22px. 활성 시 *accent background + white text*. 비활성 시 *surface-2 + muted text*.
- 각 button 의 text:
  - **I**: italic *그 자체 style* (Italic font 로 표시).
  - **U**: underlined "U".
  - **S**: line-through "S".
- 클릭 → 각자 `applyTextBoolean('italic'|'underline'|'strikethrough', !cur)`.
- Mixed: dimmed (§1.4 의 toggle mixed 와 동일 패턴).

### 4.4 Inspector 의 label row 의 hint

- 기존 label row 의 *tooltip* / *hint text* (UI subtle):
  - *"Derived from first line of text when empty"*.
  - 표시: tooltip on hover 또는 *작은 hint icon* 옆.
- 권장: *tooltip* — 시각 노이즈 최소.

---

## §5. NoteNode dblclick — R6 (시각 변경 0)

### 5.1 위치

`codebase/frontend/src/lib/canvas/NoteNode.svelte`.

### 5.2 변경

- DOM 구조 / CSS / 시각 모두 **변경 없음**.
- 동작만 변경 — `.note-node` root 에 `ondblclick` 부착 + body 영역 default routing.

### 5.3 사용자 가시 차이

- *padding / gap / rail* 영역 dblclick → 이전엔 *무동작*, 이후엔 *body 입력 진입*.
- title 영역 dblclick → 이전과 동일 (title 입력 진입).
- button 영역 dblclick → 이전과 동일 (button click — 자체 handler).

### 5.4 MaximizedItemModal 안 정합

- `MaximizedItemModal.svelte` 의 NoteNode 도 동일 dblclick zone behavior.
- modal 의 *추가 padding* 도 *body 영역* 으로 routing.
- modal 의 *외곽 click* 은 modal close — *NoteNode dblclick zone 외*.

---

## §6. TextNode auto-edit + placeholder — R7 (시각 변경 0)

### 6.1 위치

`codebase/frontend/src/lib/canvas/TextNode.svelte`.

### 6.2 변경

- DOM / CSS 변경 0.
- mount 시 *editing=true 자동 진입* — 사용자 click 후 *cursor 가 즉시 input 안*.
- 기존 placeholder visual ("Double-click to edit") *그대로 유지* — 사용자 입력 안 한 채 ESC 시 그 placeholder 표시.

### 6.3 사용자 가시 차이

- text 도구 선택 후 canvas click → *이전엔 placeholder 표시 후 사용자가 더블 클릭* 해야 입력. *이후엔 즉시 cursor 입력 모드*.
- ESC / blank click → placeholder 표시. 이전과 동일.

---

## §7. Tool active 시 cursor — R4 (시각 강화)

### 7.1 위치

`codebase/frontend/src/lib/canvas/Canvas.svelte` 의 `.canvas-root` CSS.

### 7.2 변경 — 현 코드 확장

기존:
- `drag-cursor` (rect/ellipse/line/free_draw): `cursor: crosshair`.
- `text-cursor` (text): `cursor: text`.
- `pan-cursor` (Space hold / Hand tool): `cursor: grab` (drag 중 `grabbing`).

본 batch 의 *추가 cursor* — point-spawn tool 의 *spawn cursor*:

- `note` / `file_path` / `image` / `document` / `terminal`: `cursor: copy` 또는 `cursor: cell` — *click 으로 만들어진다는 hint*.
- 또는 *기존 default cursor* 그대로 — *ghost preview 가 이미 시각 hint* 제공.

**권장**: 기존 ghost preview (Canvas.svelte:130-160) 가 *충분한 시각 feedback*. 추가 cursor 변경 *없음*.

### 7.3 Marquee selection 잔여 검증

- *코드 변경 0* — `selectionOnDrag` 가 이미 `isSelectMode` 조건.
- *manual E2E* 만 — rect tool active + drag → marquee box 안 나타남 확인.

---

## §8. ShapeNode SVG 시각 (R1 + R2)

### 8.1 위치

`codebase/frontend/src/lib/canvas/ShapeNode.svelte` (현 CSS-based → SVG).

### 8.2 시각 — Default state (Grill #3 D2)

```
┌─────────────────────┐
│                     │  ← stroke (border 2px, var(--color-fg))
│                     │     fill_enabled=false → SVG fill="none"
│                     │     stroke_enabled=true → visible
└─────────────────────┘
```

- 기존 visual (outline-only) 와 *동일* — 사용자가 신규 record 보고도 *기존 record* 와 같은 시각.

### 8.3 시각 — fill on + stroke on

```
┌─────────────────────┐
│#########################│  ← fill (사용자 picker 색)
│#########################│
└─────────────────────┘     ← stroke (사용자 picker 색)
```

### 8.4 시각 — fill on + stroke off

```
░░░░░░░░░░░░░░░░░░░░░░░░░░  ← fill 만, border 없음
░░░░░░░░░░░░░░░░░░░░░░░░░░
```

### 8.5 시각 — fill off + stroke on (default D2)

```
┌─────────────────────┐
│                     │  ← stroke 만, 내부 hit X
│                     │
└─────────────────────┘
```

### 8.6 시각 — fill off + stroke off

```
( 완전 invisible — NodeResizer 의 outline 만 M 진입 시 보임 )
```

### 8.7 corner_rounded=true (rect 만)

```
╭─────────────────────╮  ← corner_rounded=true
│                     │     radius = clamp(min(w,h)*0.15, 4, 16)
╰─────────────────────╯
```

### 8.8 stroke_dash 시각

```
─────────────  Solid
─ ─ ─ ─ ─ ─    Dash
· · · · · · ·  Dot
─ · ─ · ─ · ─  Dash-Dot
```

stroke_width 비례 — 굵기에 따라 dash 도 크기 조정.

---

## §9. 색상 / 기타 design token 매핑

본 batch 가 *직접 사용하는* design token:

| Token | 값 (light theme) | 값 (dark theme) | 사용처 |
|---|---|---|---|
| `--color-accent` | `#0d99ff` | `#1B9CFC` | Toggle ON / segmented active / focus 표시 |
| `--color-surface-2` | `#F4F4F4` | `#1E1E1E` | Toggle OFF bg / segmented inactive bg |
| `--color-border` | `#D6D6D6` | `#383838` | Toggle border / segmented border |
| `--color-fg` | `#222` | `#EEE` | Inspector value text / segmented inactive text |
| `--color-fg-muted` | `#888` | `#999` | Inspector label / hint text |
| `--font-mono` | `ui-monospace, ...` | (동일) | segmented L/N/B + dash dropdown text |
| `--font-sans` | `Inter, ...` | (동일) | Inspector hint |
| `--radius-sm` | 4px | (동일) | Toggle / segmented corner |

→ 모든 신규 control 이 *기존 token만* 사용. *별 token 신설 0*.

---

## §10. Accessibility (a11y)

### 10.1 Toggle

- `role="switch"` + `aria-checked={checked|'mixed'}` + `aria-label`.
- Keyboard: Space / Enter 로 toggle.
- Focus visible: 기존 *No focus-ring* 정책 정합 — 단 사용자가 keyboard 전용이면 *별 visible* 필요? **결정**: outline 0 + *focus 시 *subtle inner highlight** (`box-shadow: inset 0 0 0 2px var(--color-accent)`) — minimal noise + keyboard 인지 가능.

### 10.2 Segmented control

- 각 button `aria-pressed={active}` + `aria-label="Font weight: Light"` 등.
- Group 의 `role="radiogroup"` (단 multi-state segmented 의 *radio semantics* 정합 검토). 또는 `role="group"` + 각 button `role="radio"`.

### 10.3 Inspector — color picker disabled

- `fill_enabled=false` 일 때 ColorPicker 가 *시각 hide* 또는 *disabled visible*. 사용자 mental model: *hide* 가 단순.
- **결정**: hide. *disabled visible* 은 *왜 못 누르는지* 추가 인지 부담.

### 10.4 Tooltip / hint

- 신규 Toggle / button 의 *tooltip 1줄* — *어떤 동작* + *현재 상태*.
- 예: Toggle 의 `title={checked ? 'Fill enabled — click to disable' : 'Fill disabled — click to enable'}`.

---

## §11. 신규 component 의 reference (시안 출처)

- `ref/frontend-design/components-v5.html` (line ~ Toggle / segmented section 검토 필요).
- `ref/frontend-design/components-v6.html` (untracked, parallel worker 가 작업 중).
- Figma 의 *Inspector* + *Toggle* 패턴 — *general convention* 으로 참조.

→ **본 doc 의 시각 명세** 가 ref/frontend-design 의 *구체 component* 와 충돌 시 *ref 우선*. 단 *본 doc 의 위치 / 동작 invariant* 는 변경 없음.

---

## §12. 구현 순서 (FE handover §G Verification 순서 정합)

1. **Toggle.svelte 신규** — `lib/ui/Toggle.svelte` 작성. unit test (vitest + @testing-library/svelte) — *click toggle / disabled no-op / mixed state* 3 test.
2. **Inspector rect/ellipse section** (FE-D) — Toggle 사용. fill / stroke / width / dash / rounded row 추가.
3. **Inspector text section** (FE-E) — 3-segment (inline) + 3 Toggle + size + color row 추가.
4. **ShapeNode SVG migration** (FE-B/C) — Inspector 변경 land 후 *시각 변화* manual E2E.
5. **TextNode font + auto-edit** (FE-E + FE-I) — Inspector 의 control 과 *시각 연동* 확인.
6. **NoteNode dblclick** (FE-H) — *시각 변경 0* — manual E2E 만.
7. **Tool active forward** (FE-F) — *시각 변경 0* — manual E2E 만.
8. **MaximizedItemModal 의 dblclick zone 확인** — *시각 변경 0*.

---

## §13. Acceptance — design 기준

| # | 검증 | 기대 |
|---|---|---|
| AD-1 | Toggle 의 OFF/ON 시각 — design token 정합 | screenshot |
| AD-2 | Toggle 의 mixed state — 사용자 *반직관* 가 0 | screenshot |
| AD-3 | Segmented control 의 L/N/B 활성 시각 | screenshot |
| AD-4 | Inspector rect section — 4 row layout 의 grid 정합 | screenshot |
| AD-5 | Inspector text section — 5 row (chars + size+color + weight + style + align + v-align) 의 layout grid 정합 | screenshot |
| AD-6 | fill_enabled=false 시 ColorPicker hide | screenshot |
| AD-7 | corner_rounded=true 시 4px 이상 radius 적용 시각 | screenshot |
| AD-8 | stroke_dash 의 4 variant — 시각 차이 명확 | screenshot |
| AD-9 | No-session 시 Inspector 의 모든 신규 control disabled | screenshot |
| AD-10 | Locked item 시 신규 control disabled | screenshot |
| AD-11 | Light + Dark theme 양쪽 toggle/segmented control 의 시각 자연 | screenshot ×2 |

---

## §14. 본 doc 이 *완전 검증 안 한* 영역

- **ref/frontend-design/components-v6.html 의 구체 component 시안** — parallel worker 가 작업 중. 본 doc land 시점에 *시안 정합* 필요. *충돌 시 시안 우선*.
- **Mobile / touch UX** — 본 doc 은 desktop 가정. mobile 의 touch target size (44×44 권장) 는 *본 batch 외* (gtmux 의 sketch.md §"제외" — mobile polish 제외).
- **Animation / transition** — Toggle 의 knob slide 120ms, segmented active 의 *background flash* 등. *기본 ease-out* — 별 motion design 없음.

---

## §15. 다음 batch — Document feature 강화 (사용자 명시)

본 batch 종료 후 *다음 작업* 의 scope (별 ADR / 별 batch):

> *"document 기능 강화인데 지금은 local 문서 upload 하는 것이라면 선택적으로 server의 문서 파일을 불러올 수도 있도록 추가."*

→ 별 design / FE / BE handover 의 sister batch:
- BE: `/api/documents` 또는 file_open allowlist 기반 *server-side document fetch* endpoint (ADR-0023 allowlist 정합).
- FE: DocumentNode 의 *source mode selector* — `[Local upload]` / `[Server file]` 의 분기 UI.
- ADR amend: ADR-0033 (Asset storage) 의 *document source mode* 확장 + ADR-0023 (file path open) 정합.

본 doc 은 *현 batch 5 의 범위 외* — 다음 session 에서 *별 grill + handover*.

---

## 변경 이력

- 2026-05-20: 초안. Grill #1-19 결정 반영 후 *신규 UI 컴포넌트 의 위치 + 시각 + 동작* 정합. Toggle (신규) + Segmented (inline) + Inspector 의 rect/text section 확장 + ShapeNode SVG visual + NoteNode/TextNode 시각 invariant. 다음 batch (document 강화) 예고.
