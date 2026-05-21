# 2026-05-20 — UI/UX Batch 5 — 요구 분석 + 위험·해결안 정리 (FE/BE handover 의 input)

- 작성일: 2026-05-20
- 작성 주체: agent (system-architect role, requirements-analyst pass)
- 범위: 사용자가 명시한 8 UI/UX 요구를 ADR/SSoT/reports/code 와 매칭. 위험·결정·구현 step 정리. FE/BE handover (`docs/reports/2026-05-20-fe-handover-ui-ux-batch-5.md` / `...-be-handover-...`) 가 본 doc 의 자식.
- 정본 cross-link:
  - ADR-0018 (canvas item data model) — D4 schema-amend candidates 절 (a/b/c 등록만 land)
  - ADR-0017 (chrome + Toolbar2 + ContextMenu + Settings auto-save)
  - ADR-0027 (Inspector multi-select + alignment)
  - ADR-0028 (Undo/Redo + D11.1 priorSnapshot)
  - ADR-0030 (Clipboard — D3 terminal clone-spawn)
  - ADR-0031 (Figure modifier constraint — Shift hold)
  - ADR-0032 (Multi-select context menu)
  - ADR-0021 (Terminal pool + mirror)
  - SSoT `canvas-layout-schema.md` (v1 doc — schema 본체는 ADR-0018 이 진실)
  - 직전 회고 리포트: `docs/reports/2026-05-20-fe-context-and-error-fix-recap.md`

---

> ⚠️ **2026-05-20 Grill #1-19 amend 후** — §1 권장 절은 *grill 결과* 반영. R5 폐기 / R2 corner_radius→corner_rounded / R7 Settings 폐기 / R8 full clone preserve 등 *큰 reversal*. *변경 이력* 절 참조.

## 0. 8 요구 한 줄 매핑

| # | 사용자 요구 | 본질 | 관련 ADR | 분류 |
|---|---|---|---|---|
| **R1** | rect/ellipse — fill / stroke on/off + on 일때만 inspector 노출 + off 면 그 영역 selection event 차단 (≠ alpha) | schema 신규 boolean + hit-test 분기 | ADR-0018 D4(b register), ADR-0027 | BE+FE 양 |
| **R2** | stroke style (dash 패턴) + 두께 + rect 만 corner round | schema 신규 enum/scalar | ADR-0018 D4(b register) | BE+FE 양 |
| **R3** | text font 크기·색·bold·italic·밑줄·취소선 | schema 신규 enum + 기존 color 활용 | ADR-0018 D4(a register) | BE+FE 양 |
| **R4** | 도구 활성 중 canvas 입력 → 기존 selection event 차단 | tool-active state 의 click handler 정합 | ADR-0017 §Toolbar2, Canvas.svelte | FE-only |
| **R5** | terminal 의 OS clipboard 복사/붙여넣기 금지 | xterm v6 옵션 / event 차단 | ADR-0004 (xterm), ADR-0030 D7 (focus 분기) | FE-only |
| **R6** | note content 영역 *전체* (padding 포함) 더블클릭으로 입력 활성 | dblclick zone 확장 | ADR-0018 D9 (inline edit), NoteNode.svelte | FE-only |
| **R7** | text 추가 즉시 입력 모드 + empty 취소시 placeholder 그대로 + 입력 시 앞 N자 label (Settings.max) | itemFactory → TextNode 의 auto-edit prop + label derive + Settings 신규 키 | ADR-0017 §Settings (auto-save), ADR-0018 D4 text | FE-only + Settings (BE도 키 등록만) |
| **R8** | terminal panel 복붙 = 크기만 재사용, 새 terminal 생성·연동 | 이미 ADR-0030 D3 default — 실제 구현 정합 확인 | ADR-0030 D3, clipboardOps.svelte.ts, ADR-0021 D6 match-or-spawn | FE-only (BE 신규 endpoint 0)  |

---

## 1. 요구 별 상세 분석

### R1. Fill/Stroke on-off + on 일 때만 inspector + off 면 hit-test 차단

#### 1.1 현재 상태

- Schema (`schema.rs::Item::Rect` / `Item::Ellipse`): `stroke: String`, `fill: String`, `stroke_width: u32` — *on/off 개념 없음*. `fill: "transparent"` 가 *시각적 fill 없음* 의 현 표현 (`itemFactory.createShapeItem` default).
- FE renderer (`ShapeNode.svelte`): `border-color: {stroke}` / `border-width: {stroke_width}px` / `background: {fill}` 그대로 inline-style 만 적용. *DOM 의 `.shape-node` div 가 늘 pointer event capture* — fill 이 transparent 여도 div 자체가 pointer-events 받음.
- Inspector (`ItemInfoView.svelte` `applyShapeColor`): rect/ellipse 의 `stroke` + `fill` ColorPicker 2 row. on/off toggle 없음. `fill: "transparent"` 가 pseudo-off 지만 stroke 도 off 할 수 없음.

#### 1.2 본질·위험

> ⚠️ 사용자 verbatim: *"fill이 off되어있으면 내부 영역은 선택 event 발생하지 않도록. stroke도 마찬가지. 투명도와 다름."*

→ **(투명도 ≠ off)** 의 의미: `fill_enabled: false` 면 *fill 색 자체*는 layout 에 보존 (재 on 했을 때 옛 색 복원) + *hit-test 에서도 제외*. 단순히 `fill: "rgba(0,0,0,0)"` 으로는 hit-test 가 div 전체로 잡혀 안 됨 — DOM 의 `pointer-events` 분기 필요.

위험들:
- **W1**: stroke off 시 border 두께 0 으로 만들면 corner-radius 와 layout shift 발생.
- **W2**: fill off 시 div 내부 hit-test 가 빠지면 *resize handle* (NodeResizer) 도 못 닿음 — handle 은 별 layer 라 자연 보존. 단 panel 자체 drag 가능 영역은 *border 위*만 됨.
- **W3**: 둘 다 off (`fill_enabled=false && stroke_enabled=false`) 면 *완전 invisible* — 사용자가 잃어버릴 가능성. visibility=hidden 과 사실상 동치. **결정**: 둘 다 off 허용하되 *M 안에 있을 때는 NodeResizer outline* 로 인지 가능 + Inspector 노출 + Layer Tree 의 row 에서 발견 가능. 추가 적극 fallback (예: ghost outline) 은 P1 검토.
- **W4**: backward-compat: 옛 layout 의 `fill: "transparent"` 가 *of-off* 의 의도였는지 *알파 0* 의 의도였는지 모호. **결정**: 옛 record 의 `fill === "transparent"` 는 *알파 0* 으로 해석 (현 코드 정합), 새 `fill_enabled` field 의 default 는 `true`. 사용자가 명시 off 한 record 만 false.

#### 1.3 결정·스펙

- Schema 신규 field (ADR-0018 D4 amend, register-only → land):
  - `Rect` / `Ellipse`: `fill_enabled: bool` (default `true`, `#[serde(default = "default_true")]`), `stroke_enabled: bool` (default `true`).
- FE `ShapeNode.svelte` rendering:
  - `fill_enabled=false` → `background: transparent` + container `pointer-events: none` (하지만 NodeResizer + CanvasCloseButton 은 별 layer 로 호스팅 → 직접 영향 0). 단 `.shape-node` 의 child 영역 도 pointer-events:none 이라 *fill 영역 click* 차단.
  - `stroke_enabled=false` → `border-style: none` (or 0 width with `box-sizing` 보정).
- Hit-test 분기:
  - `fill_enabled=true && stroke_enabled=true` → div 전체 pointer-events 받음 (현 동작).
  - `fill_enabled=false && stroke_enabled=true` → div pointer-events:none + 별 *border-only hit-target* layer 추가 (SVG `<rect>` stroke 두께만 hit, 또는 CSS clip-path 로 ring 형태만 hit). **MVP 권장**: SVG layer 한 겹 — `pointer-events: stroke` 로 *stroke 만* hit (SVG painted area 만 hit, 내부 fill area 는 transparent + pointer-events:none).
  - `fill_enabled=true && stroke_enabled=false` → div 전체 hit (현 동작에서 border 만 시각적으로 제거).
  - `fill_enabled=false && stroke_enabled=false` → 둘 다 pointer-events:none — *완전 invisible + non-hittable*. 단 M 에 강제 들어가면 NodeResizer outline 으로 인지.
- Inspector (`ItemInfoView.svelte` rect/ellipse section):
  - 2 row 추가: `[Fill on/off toggle] [color picker disabled when off]` / `[Stroke on/off toggle] [color picker disabled when off]`.
  - `stroke_enabled=true` 일 때만 stroke_width / stroke_dash row 노출.
  - `fill_enabled=true` 일 때만 fill 의 alpha 채널 노출 (지금 `allowAlpha={true}` 그대로).
- ADR amend: ADR-0018 D4 의 `rect/ellipse` row + `Item::Rect/Ellipse` Rust variant 갱신 + D4(b) "Figure stroke/fill 패턴" register 의 amend ① 로 promote (`fill_enabled` / `stroke_enabled` 신규).

### R2. Stroke style (dash) + 두께 + corner round (rect 만)

#### 2.1 현재 상태

- Schema: `stroke_width: u32` 만. `stroke_dash` 는 connector 의 `Option<StrokeDash>` (ADR-0036) 에만 존재 — rect/ellipse/line 에는 없음.
- ADR-0018 D4(b) register 가 정확히 이걸 cover: *"rect / ellipse / line 의 `stroke_dash`: "solid" | "dash" | "dot" | "dashdot""*. 본 batch 가 promote.
- corner round: schema 도, 코드도 *없음*. ShapeNode 는 ellipse 시 `border-radius: 50%`, rect 는 *기본 0*.

#### 2.2 본질·위험

> ⚠️ 사용자 명시: *"rectangle만 모서리 round 설정기능 추가."*

→ rect 한정. ellipse 는 의미 없음 (이미 full round), line 도 무관.

위험들:
- **W5**: `corner_radius` 값이 `min(w, h) / 2` 를 초과하면 stadium 모양. 시각적으로 ellipse 처럼 보임 → 사용자 혼동 가능. **결정**: max cap = `min(w, h) / 2` — Inspector slider 가 cap 적용, BE 도 validate.
- **W6**: connector 의 `StrokeDash` enum 이 `Dash | Dot` 2 variant 만 정의. rect/ellipse/line 의 register 는 4 variant (`solid|dash|dot|dashdot`). **결정**: connector 의 enum 을 *재사용* 하지 말고 *별 enum* 신규 (`FigureStrokeDash: Solid|Dash|Dot|DashDot`) — connector 와 의미·default 다름 (connector 는 nullable for solid, figure 는 Solid variant 명시).
- **W7**: line 의 dash 표현은 SVG `stroke-dasharray` 매핑 자연. rect/ellipse 의 dash 는 *border-style: dashed/dotted* 로 CSS 가능하나 *dashdot* 은 CSS 미지원 → SVG `<rect>` / `<ellipse>` 변환 필요. **결정**: ShapeNode 를 *SVG-based* 로 점진 마이그 (성능 이슈 없음, item 당 SVG 1 element).

#### 2.3 결정·스펙

- Schema 신규 (ADR-0018 D4 amend):
  - `Rect`: `corner_radius: Option<u32>` (default `0`), `stroke_dash: Option<FigureStrokeDash>` (default `None` → "solid").
  - `Ellipse` / `Line`: `stroke_dash: Option<FigureStrokeDash>`.
- 새 enum `FigureStrokeDash` (snake_case): `Solid | Dash | Dot | DashDot`.
- Validation: `Rect.corner_radius` ≤ `min(w, h) / 2` — over 면 `RectCornerRadiusExceedsBox` 신규 ValidationError variant.
- FE: ShapeNode 를 inline SVG 로 리팩토링 — `<svg width="100%" height="100%"><rect ... rx={corner_radius} stroke-dasharray={pattern} ...></svg>` 또는 ellipse `<ellipse cx="50%" cy="50%" rx="50%-stroke/2" ry="..."/>` + `<line>` for LineNode.

### R3. Text font 풀-style

#### 3.1 현재 상태

- Schema: `Text { ..., text: String, font_size: u32, text_align, text_vertical_align, color: String }`.
- ADR-0018 D4(a) register: `font_family?` / `font_weight?` / `font_style?` / `text_decoration?` / `line_height?` 5 옵셔널 후보 — 본 batch 가 promote.
- 사용자 요구는 5 중 *bold/italic/밑줄/취소선* 만 명시 — `font_family` / `line_height` 는 본 batch 에서 *register 유지, 노출 안 함*. `font_size` 와 `color` 는 *이미* 있음.

#### 3.2 본질·위험

- **W8**: 밑줄+취소선 동시는 CSS 가 `text-decoration: underline line-through` 로 표현. enum `"underline" | "line-through" | "none"` 만으론 둘 다 불가. **결정**: schema 의 `text_decoration` 을 *array* 또는 *bit-flag string* 으로 — `text_decorations: Option<Vec<TextDecoration>>` 가 가장 명확. 단 wire 부피 약간 늘어남. 대안: 별 4 boolean field — `bold/italic/underline/strikethrough`. **선택**: 4 boolean (간단하고 inspector 의 4 토글과 1:1). register 의 `text_decoration` enum 은 *별 amend* 로 폐기 (단 본 amend 가 register 변경 명시).
- **W9**: backward-compat — 모두 옵셔널 (`#[serde(default)]` 로 옛 record 자동 `false`).

#### 3.3 결정·스펙

- Schema `Text` 변형:
  - `font_weight: Option<FontWeight>` — `FontWeight: Normal | Bold` (MVP. 100~900 numeric 은 P1).
  - `font_style: Option<FontStyle>` — `FontStyle: Normal | Italic`.
  - `underline: Option<bool>` (default false).
  - `strikethrough: Option<bool>` (default false).
- 단 register 의 `text_decoration` 단일 enum 을 *4 boolean* 로 갈아끼우는 결정은 ADR-0018 D4 amend ⑤ (변경 이력) 에 명시 — 옛 register entry 무효화.
- FE TextNode rendering — inline style 추가: `font-weight: {bold ? '700' : 'normal'}; font-style: {italic ? 'italic' : 'normal'}; text-decoration: {underline ? 'underline ' : ''}{strikethrough ? 'line-through' : ''};`.
- Inspector 의 Text section 에 4 toggle (segmented control 4 button: B / I / U / S).

### R4. 도구 활성 중 canvas 입력 → selection event 차단

#### 4.1 현재 상태

- `Canvas.svelte:303` `isSelectMode = $derived(toolStore.current === 'select')`. 다음 4 gate 가 *이미* select 모드 외 차단:
  - `onnodeclick` (918) — `if (!isSelectMode) return;`
  - `onselectionchange` (1244) — `if (!isSelectMode) return;`
  - SvelteFlow props: `nodesDraggable={isSelectMode && !isMaximizedActive}`, `elementsSelectable={isSelectMode}`, `selectionOnDrag={isSelectMode && !isSpacePressed && !isMaximizedActive}`.
- 그러나 *create gesture 후 `sessionStore.setM([fresh.id])`* 가 fire — 새 item 이 자동 선택 (Canvas:1100, 1259, itemFactory:361).

#### 4.2 본질·위험

> ⚠️ 사용자 verbatim: *"도구 선택 후 canvas 입력 시 canvas 선택과 같은 event 제한."*

→ "canvas 선택과 같은 event 제한" 의 의미는 (1) 도구 입력으로 인해 *기존 item 선택* 이 발생하지 않음 + (2) drag-create 중 marquee selection 비활성 + (3) 도구 활성 중 onnodeclick 의 selection 자체 차단. (1)(3) 은 이미 가드됨. (2) 도 `selectionOnDrag` 조건으로 가드됨. **남은 결함**: drag-create 흐름의 down/move/up 동안 *SvelteFlow 의 marquee selection rectangle 잔여* 가 잠시 보임 (drag-tool active 때도 `selectionOnDrag=isSelectMode` 라 false 일 텐데 잔여 시각 의심) — 실 동작 confirm 필요.

위험들:
- **W10**: text/note tool 활성 중 *node 위* click 시 `onpaneclick` 이 fire 안 됨 (SvelteFlow 가 node hit → onnodeclick 으로 routing) → tool 미발화. 사용자가 *위에 새 item 만들고 싶어도* 안 됨. **결정**: tool active 일 때 onnodeclick 은 *event 흡수만* (early return) + onpaneclick 도 안 옴 → tool action 실패. **결정 fix**: tool active 일 때 *node hit 도 onpaneclick 처럼 동작* — onnodeclick handler 가 tool 분기에서 onpaneclick 의 spawn 로직 forward 호출.
- **W11**: 새 item 생성 후 자동 setM 이 *도구 활성 상태에서도* 새 item 을 선택 상태로 만듦. 사용자 의도는 "도구는 계속 active 일 수도 있고, 한 번 사용 후 select 로 회귀 (one-shot default — toolStore.consume) 일 수도 있음". 자동 setM 자체는 유지 (Figma 패턴). 단 *consume 후 select 회귀가 일어났을 때만* setM 효과가 사용자 가시. 본 batch 는 *추가 변경 없음* — toolStore.consume 후 자연 정합.
- **W12**: dragstop 시 `onnodedragstop` 이 node move 처리 — node drag 자체가 발생하려면 `nodesDraggable` 가 true. 즉 select 모드 외에는 node drag 안 됨. 정합.

#### 4.3 결정·스펙

- Canvas.svelte 의 `onnodeclick` 의 tool 분기:
  - 현재: `if (!isSelectMode) return;`
  - 변경: tool active + point-spawn tool 이면 *onpaneclick 의 spawn 로직 동일 호출* — 즉 node 위 클릭도 새 item 을 그 좌표에 생성.
  - drag-tool (rect/ellipse/line/free_draw) 는 onnodeclick 이 발화될 일 없음 (down/move/up 의 capture 처리) — 분기 불요.
- 회귀 가드: select 모드일 때 onnodeclick 의 기존 behavior (single / meta-toggle) 그대로.
- **결정**: `marquee selection rectangle` 의 잔여 시각 확인을 manual E2E 항목으로 추가 (코드 변경 없음 가능성 큼).

### R5. Terminal 의 OS clipboard 복사·붙여넣기 금지

#### 5.1 현재 상태

- `SECURE_XTERM_OPTIONS` (`lib/xterm/options.ts`): 비어있는 placeholder — *보안 옵션은 P0 구현 시 채움* 주석만. OSC 52 도 비활성 미명시.
- xterm v6 기본 동작: helper-textarea (`.xterm-helper-textarea`) 가 OS clipboard 의 paste/copy 를 받음. Ctrl/Cmd+V → paste, Ctrl/Cmd+C (selection 있을 때) → copy.
- `clipboardShortcuts.svelte.ts`: FE clipboard 의 C/X/V 가 `allowInXterm: false` — xterm focus 시 *FE clipboard 도 skip* → xterm 의 native 가 흡수.
- 결과: 현재는 *xterm 의 native clipboard 동작이 살아있음*.

#### 5.2 본질·위험

> ⚠️ 사용자 verbatim: *"terminal 복사 붙여넣기 금지."*

위험들:
- **W13**: terminal 의 paste 차단 후 사용자가 *명령어 빈번 paste* 의 workflow 가 깨짐. → 의도된 결과 (사용자가 명시 요구). 단 *왜 차단하는지* 의 이유 (R8 의 panel 복붙 동작과 의미 분리) 가 release note 에 명시되어야.
- **W14**: middle-click 의 *primary selection* paste (Linux/X11) — 별도 차단 필요. xterm v6 의 `rightClickSelectsWord` / `customRightClickHandler` 등은 다 별. `paste` DOM event 직접 차단이 가장 단순.
- **W15**: drag-and-drop text 도 잠재 paste 표면. `dragover` / `drop` 차단 필요.

#### 5.3 결정·스펙

- `XtermHost.svelte` 마운트 시:
  - `term.attachCustomKeyEventHandler(e => { ... })` — Ctrl/Cmd+V / Ctrl/Cmd+C / Ctrl/Cmd+X 시 `return false;` (xterm 의 default 차단). 단 Ctrl+C 의 *SIGINT* 는 keyboard onData 로 통과해야 — 즉 *selection 이 있는 경우* 만 차단, *selection 없는 Ctrl+C* 는 통과. → 그러나 simpler: *모두 차단* (사용자 요구의 명확성 우선). 사용자는 SIGINT 를 GUI 외 도구 (e.g. ContextMenu `[Send SIGINT]`) 로 보낼 수도 있고, 또는 *Cmd+. (mac) 같은 별 path*. **MVP 결정**: 모두 차단 + ADR amend 에 "SIGINT 의 별 entry P1" 명시.
  - container 의 `paste` DOM event capture-phase 차단: `containerEl.addEventListener('paste', e => e.preventDefault(), { capture: true })`.
  - container 의 `copy` / `cut` 도 차단: `addEventListener('copy', e => e.preventDefault(), ...)`.
  - container 의 `drop` / `dragover` 차단.
- `SECURE_XTERM_OPTIONS` 에 명시:
  - `disableStdin: false` (기본 유지 — 키 입력 자체는 필요).
  - 옵션 직접 차단 불가 (xterm v6 의 paste/copy 는 별 옵션 없음) — 이벤트 차단으로만.
- `clipboardShortcuts` 의 `allowInXterm: false` 정책 유지 — FE clipboard 도 xterm focus 시 발화 X. 단 새 *FE-canvas* 영역에서 Cmd+C/V 는 FE clipboard 가 정상 발화.

### R6. Note content 전체 영역 더블클릭 입력 활성

#### 6.1 현재 상태

- NoteNode.svelte: `ondblclick` 은 `.note-label` (title) 과 `.note-body-wrap` (body) 두 *분리된 zone* 에만 부착.
- `.note-node` 컨테이너의 padding (`padding: 8px 6px 12px 12px`) + head/body 사이 gap (`gap: 6px`) 영역의 dblclick 은 *어떤 listener 도 없음* → 무동작.

#### 6.2 본질·위험

> ⚠️ 사용자 verbatim: *"note의 content 영역 모두 더블클릭으로 입력 활성화 하도록. (padding은 적용)"*

= padding 안의 *모든* 영역을 dblclick zone 으로. *어느 field* 가 active 되어야 하는가가 모호 — head 위 zone 더블클릭 → title, body 위 zone → body, padding 영역 → 어느 쪽?

위험들:
- **W16**: padding gap 영역의 dblclick 이 *어떤 field* 를 활성할지 선택. **결정**: vertical center (`y < headBottom + halfGap`) → title, 그 외 → body. 또는 단순화: *padding 자체는 body 로 routing* (head 자체 row 더블클릭은 title) — 사용자가 head 외 영역 더블클릭 = body 의도.
- **W17**: 현재 dblclick listener 가 `.note-label` 에만 — 즉 head 의 *glyph (svg)* / *button 영역* 의 dblclick 은 title 활성 안 됨. **결정**: head 의 buttons 외 영역 (label + glyph) 모두 title 활성. 단 *buttons* 는 자체 click 우선 (자식의 stopPropagation).

#### 6.3 결정·스펙

- NoteNode `.note-node` 컨테이너 자체에 `ondblclick={onContentDblClick}` 부착.
- `onContentDblClick(e)` 분기:
  - `e.target` 이 button 또는 button 자식 (svg) 이면 return.
  - `e.target` 이 head row 또는 그 자식 (label / glyph) 이면 → titleEditing = true.
  - 그 외 (body, padding) → bodyEditing = true.
- Padding 은 그대로 유지 (CSS 변경 없음) — *dblclick zone 만 확장*.
- `isLocked || isMinimized` 가드 그대로.

### R7. Text 추가 즉시 입력 모드 + 앞 N자 label 동기화

#### 7.1 현재 상태

- `Canvas.svelte::onpaneclick` `tool === 'text'` 분기: `createCanvasItem('text', {x,y})` + `commitNewItem`. 새 item 은 layout 에 들어가지만 *editing 모드 자동 진입 안 함* — 사용자가 *다시 더블클릭* 해야 입력.
- `TextNode.svelte::editing` 은 *내부 state* — 외부에서 set 할 수 없음.
- Label: 현재 `ItemCommon.label` field 가 있고 Inspector 가 표시. text 의 *body* 가 label 과 별. 사용자가 label 을 직접 수정해야 함.

#### 7.2 본질·위험

> ⚠️ 사용자 verbatim: *"text 는 추가하자마자 입력 상태로 전환. empty 상태에서 입력취소(esc나 빈공간 클릭)하면 지금처럼 표시. 그리고 입력이 있는 경우에는 text 가장 앞 내용 일부를(반영되는 최대 글자수는 설정) label로 설정."*

위험들:
- **W18**: 새 text item 의 auto-edit 진입을 *어떻게 trigger* 할 것인가. TextNode 의 editing state 는 internal. **결정**: TextNode 에 `autoEdit?: boolean` prop 추가 → mount 시 true 면 editing = true 로 초기화. 또는 `sessionStore.justSpawnedTextId: string | null` 신호 → TextNode 가 mount 시 `data.id === justSpawnedTextId` 면 auto-edit 진입 후 store flag clear. **선택**: store-level 신호 (prop 전달 chain 복잡 차단, 단일 책임).
- **W19**: empty 상태에서 ESC / blank click → 현재 *editing=false* 만. 그러나 item 은 layout 에 남음 → 빈 text 의 placeholder ("Double-click to edit") 가 보임. *사용자가 의도하지 않은 item 생성* 위험. **결정 후보**:
  - (a) empty + ESC → 자동 delete (Figma 패턴). 단 빈 text 가 *의도된 placeholder* 일 수도 있음.
  - (b) empty + ESC → 그대로 placeholder 표시. 사용자가 명시 delete (Backspace) 필요.
  - 사용자 verbatim: *"empty 상태에서 입력취소... 하면 지금처럼 표시"* → (b) 가 직접 매치. **선택**: (b) 그대로 placeholder.
- **W20**: label 의 derive 시점 — *typing 중* 매 keystroke 마다 label update 는 PUT 폭주. **결정**: text *commit* 시점 (사용자가 Enter 또는 blur 로 확정) 에 한 번 derive — applyMutation 한 번에 `text` + `label` 둘 다 갱신.
- **W21**: 최대 글자수 setting — Settings 의 어디에 두는가. 현 settingsStore 가 BE 의 `/api/settings` 통과. **결정**: `text_label_max_chars: u32` (default 24). BE 의 `Settings` struct 에 신규 field + `/api/settings` JSON 의 신규 키.
- **W22**: label 의 길이 제한 — 사용자가 직접 label 입력 시 4 KB cap 그대로 (ADR-0018 D8). text-derived label 은 max_chars 가 *그보다 작음* — 자연 cap.

#### 7.3 결정·스펙

- Settings: 신규 키 `text_label_max_chars: u32`. default = 24. BE `Settings` struct + Settings UI section ("Canvas — Text label max chars").
- itemFactory `createCanvasItem('text', ...)` → commitNewItem 의 *성공 path* 에서 `sessionStore.justSpawnedTextId = fresh.id`.
- TextNode `$effect` 마운트 시: `if (sessionStore.justSpawnedTextId === data.id) { editing = true; sessionStore.justSpawnedTextId = null; }`.
- TextNode `onCommit(next)` body:
  - 기존 `text` mutation + `label` 도 `next.slice(0, settings.text_label_max_chars).trim()` 으로 동시 갱신.
  - 단 사용자가 직접 label 을 *Inspector 에서 변경* 한 후에는 *override 보존* — 추가 flag 필요?
    - **결정**: 자동 derive 는 *unconditional* — 사용자가 label 을 다르게 두고 싶으면 *Inspector 에서 text 변경 없이 label 변경*. text 변경 시는 label 도 새로 자동 derive (Figma 의 frame 자동 이름 패턴). 단순.
- onCancel (empty + ESC):
  - 현재 `editing = false` 만 — 그대로. 사용자 요구 정합.

### R8. Terminal panel 복붙 = 크기 재사용 + 새 terminal spawn

#### 8.1 현재 상태

- ADR-0030 D3: *terminal paste = clone default*. 새 UUID + BE 의 unmatched-spawn 분기 자연 활용.
- `clipboardOps.svelte.ts::cloneWithOffset`: `id: crypto.randomUUID()` — 모든 type 의 ID 갈아끼움. terminal 의 `terminal_id` 는 *없음* (ADR-0018 D2 의 *item.id = backend Terminal.id*).
- `pasteItems` 의 흐름:
  1. `applyMutation` 로 layout 에 새 UUID 의 terminal item 추가.
  2. BE 는 unmatched UUID → match-or-spawn 의 spawn 분기 → 새 terminal 생성 (D6).
  3. WS `0x88 TERMINAL_SPAWNED` → terminalPool.bindPaneId → XtermHost mount.

#### 8.2 본질·위험

> ⚠️ 사용자 verbatim: *"terminal panel은 복사 붙여넣기 했을때 크기만 재사용하고 새로운 terminal을 만들고 연동하는 형태로 구현."*

위험들:
- **W23**: `cloneWithOffset` 의 spread `{ ...clone, id: crypto.randomUUID(), x: ..., y: ... }` 가 *terminal 의 모든 payload* (label / description / minimized / restored_geom 등) 도 복제. **사용자 의도는 "크기 (w, h) 만 재사용"** → label / description / minimized 등은 fresh state. **결정**: terminal type 일 때 `cloneWithOffset` 분기 — `id / x / y / w / h / z / parent_id / visibility / locked / type` 만 보존, 나머지 field 는 fresh default. 즉:
  - `label = ""`
  - `description = ""`
  - `minimized = false`
  - `restored_geom = undefined`
- **W24**: 그러나 *attach 흐름* (D6 match-or-spawn) 은 이미 동작 중 — 본 batch 는 *clone 의 페이로드 filter* 만 변경. attach flow 변경 0.
- **W25**: 다른 type (text/note/rect 등) 의 복붙 시에는 *모든 payload 복제* 가 정합 (사용자 의도 = 시각 동일 복제). terminal 만 *특별 처리*.

#### 8.3 결정·스펙

- `clipboardOps.svelte.ts::cloneWithOffset` 의 `out.type === 'terminal'` 분기 추가 — fresh state 만 보존:
  ```ts
  if (out.type === 'terminal') {
    out.label = '';
    out.description = '';
    out.minimized = false;
    // restored_geom 은 옵셔널이라 undefined 가 default
    delete (out as any).restored_geom;
  }
  ```
- ADR-0030 D3 amend: *"Terminal paste 시 (w, h, parent_id, visibility, locked, z) 만 source 에서 보존. label / description / minimized 는 fresh default."* 명시.
- BE 변경: *없음* — 기존 match-or-spawn 의 unmatched 분기 그대로 활용.

---

## 2. 우선순위 / 의존 / 묶음

| 묶음 | 요구 | 의존 | 우선 |
|---|---|---|---|
| **A. Schema amend (BE)** | R1 + R2 + R3 + R7 (Settings) | 없음 — backward-compat additive | **P0** (FE 작업 전 prerequisite) |
| **B. Shape inspector + renderer** | R1 + R2 | A | P0 |
| **C. Text inspector + renderer + auto-edit + label sync** | R3 + R7 | A | P0 |
| **D. Canvas tool gating** | R4 | 없음 | P1 |
| **E. Xterm clipboard 차단** | R5 | 없음 | P1 |
| **F. Note dblclick zone 확장** | R6 | 없음 | P1 |
| **G. Terminal paste payload filter** | R8 | 없음 (ADR-0030 D3 amend ① 만) | P1 |

→ BE 작업 = A 한 batch 만. FE 작업 = B/C/D/E/F/G 6 batch. A 이 land 후 FE 의 typings 재생성 (`pnpm gen-types` 또는 openapi 재발행) → FE batch 진입.

---

## 3. ADR amend 매트릭스

| ADR | 본 batch 의 amend |
|---|---|
| **ADR-0018 D4** | (b) register promote to amend ①: `fill_enabled` / `stroke_enabled` (rect/ellipse) / `corner_radius` (rect) / `stroke_dash: FigureStrokeDash` (rect/ellipse/line). (a) register promote to amend ②: text 의 `font_weight` / `font_style` / `underline` / `strikethrough` (4 boolean 결정 명시 — 옛 `text_decoration` enum 폐기). |
| **ADR-0018 D8** | `RectCornerRadiusExceedsBox` 신규 ValidationError variant. |
| **ADR-0018 D9** | Note dblclick zone 확장 — *content 전체 (padding 포함)* 가 inline edit 진입 source. head vs body 분기 명시. |
| **ADR-0030 D3** | Terminal paste 시 *size + 좌표 + visibility + locked + z + parent_id* 만 source 에서 보존, 나머지 fresh default — amend ①. |
| **ADR-0017 §Settings** | `text_label_max_chars: u32` 신규 (default 24). amend (Settings section). |
| **ADR-0017 §Toolbar2** | tool active 시 node click 도 onpaneclick 의 spawn 로직 forward — amend. |
| **ADR-0004 (xterm)** | xterm v6 의 clipboard event 차단 정책 (paste/copy/cut/drop/dragover + attachCustomKeyEventHandler). amend. |

---

## 4. 본 분석의 *open question* (사용자 결정 권장)

| Q | 옵션 | 권장 |
|---|---|---|
| Q1. rect/ellipse 둘 다 off 시 *visual fallback* | (a) 완전 invisible / (b) ghost outline / (c) 금지 | (a) — 단 M 진입 시 NodeResizer outline + Inspector + LayerTree 로 인지 가능 |
| Q2. text 의 ESC empty cancel = delete 또는 placeholder 유지 | (a) delete / (b) placeholder | (b) — 사용자 verbatim 매치 |
| Q3. terminal Ctrl+C 의 SIGINT 도 차단 | (a) selection 있을 때만 / (b) 모두 차단 + 별 entry P1 | (b) — MVP 명확성. P1 에서 ContextMenu `[Send SIGINT]` |
| Q4. text label auto-derive 가 사용자 label override 도 덮어쓰는가 | (a) 항상 override / (b) 사용자 override 시 보존 | (a) — Figma frame 자동 이름 패턴. 단순 |
| Q5. ShapeNode SVG migration 의 범위 | (a) 본 batch 에서 rect/ellipse/line 모두 SVG / (b) rect 만 (corner_radius 요구), ellipse/line 은 다음 batch | (a) — dashdot 표현 위해 모두 SVG 필요 |

→ 본 doc 의 §1 의 결정 절은 *권장* 선택. 사용자 grilling 후 변경 가능.

---

## 변경 이력

- 2026-05-20: 초안. 사용자 8 요구 → ADR/SSoT/reports/code 매핑 + 위험 분석 + 결정 권장 + ADR amend 매트릭스. FE/BE 별 handover (sister docs) 의 input.
- 2026-05-20 (Grill #1-19 amend): `grill-with-docs` skill session 으로 19 분기 결정 pin-down. 본 amend 가 §1 의 권장 절을 *최종 결정* 으로 갱신. 주요 reversal / 단순화:
  - **R1 factory default** = `fill_enabled=false` (stored gray `#D9D9D9`) + `stroke_enabled=true` + outline-only 시각 (Grill #3).
  - **R2 corner_radius 폐기 → `corner_rounded: bool`** + 자동 radius `clamp(min(w,h)*0.15, 4, 16)`. ValidationError `RectCornerRadiusExceedsBox` 폐기 (Grill #5).
  - **R2 stroke_width cap** 1~32 + ValidationError `StrokeWidthOutOfRange` (Grill #14).
  - **R3 FontWeight** = Light/Normal/Bold 3 variant (Grill #6).
  - **R3 italic/underline/strikethrough** = `bool` + `#[serde(default)]`. FontStyle enum 폐기 (Grill #15).
  - **R3 font_size cap** 8~96 + ValidationError `TextFontSizeOutOfRange` (Grill #7).
  - **R5 폐기** — terminal 의 xterm clipboard 차단 안 함 (Grill #8).
  - **R6 body 영역만** — title 영역 별 처리 X. MaximizedItemModal 안 동일 (Grill #13).
  - **R7 label-empty trigger** — `cur.label === '' && next !== ''` 만 derive. 이후 독립 (Grill #10).
  - **R7 derive 알고리즘** = `text.split('\n', 1)[0].trim().slice(0, 4000)` — 첫 줄만, 기존 label cap (4 KB) 자연 활용 (Grill #18).
  - **R7 `text_label_max_chars` Settings 폐기** — 별 setting 신설 안 함 (Grill #18).
  - **R8 terminal paste = full clone + id 만 fresh** — layout/label/description/visibility/locked/minimized/restored_geom 모두 preserve. 기존 cloneWithOffset 그대로 사용 — *추가 코드 변경 0* (Grill #17).
  - **Q1~Q5 resolved**: Q1 = (a) invisible. Q2 = (b) placeholder 보존. Q3 = R5 폐기로 무관. Q4 = (B'3) label-empty trigger. Q5 = (a) SVG migration.
  - 본 grill session 의 자세한 결정 trace 는 `grill-with-docs` 의 conversation history. *반영 patch* 가 sister docs (BE/FE handover) 에도 적용.
