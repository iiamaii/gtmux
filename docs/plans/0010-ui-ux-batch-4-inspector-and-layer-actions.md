# Plan 0010 — UI/UX batch 4 (Layer actions + Inspector v2 + Alignment)

- 일자: 2026-05-16
- 종류: **implementation plan** — 5 task overview + scope + ADR-amend 인벤토리
- 정본 ADR: ADR-0017 (chrome layout), ADR-0018 (canvas item data v2), ADR-0024 (layer tree + z-index separation)
- 신규 ADR 후보: ADR-0027 (Inspector multi-select layout + alignment mutation), ADR-0018 D? amend (shape transform — 후속 결정)
- 관련 plan: plan-0007 §14.6 (maximize UI spec — task 1 의 기존 base)
- 관련 report: 0049 §4.2 (text-align Inspector ship), 0049 §7 known issue

---

## 0. 한 줄 요약

`Layer list item 의 minimize/maximize/focus 액션 + Inspector 의 multi-select 공통 layout + shape fill/stroke 색 picker + node 간 alignment (left/center/right/top/middle/bottom + distribute) 까지 한 묶음 UI/UX batch 4. 진단 필요한 bug (작업 2 — session 진입 시 panel title null 회귀) 가 P0.`

---

## 1. Task 인벤토리

### Task 1 — Layer list item 의 minimize / maximize / focus 버튼

- **state foundation (이미 ready)**:
  - `sessionStore.maximizedItemId: string | null` (line 93)
  - `sessionStore.focusMode: { enabled, targetPanelId }` (line 100)
  - mutator: `toggleMaximize(id)` (line 231)
  - `item.minimized: boolean` (canvas-layout schema)
- **UI gap**:
  - LayerTreeView row 에 action button (minimize/maximize/focus) 없음
  - focus = "해당 컴포넌트를 가득 채우는 viewport 이동" — Canvas viewport setter API 활용 (현 `sessionStore.updateViewport` + `Canvas.svelte` 의 `applyingStoreViewport` race-guard) — 단 *focus mode flag* 와 *viewport zoom-to-fit* 의 구분 필요. 사용자 정의는 후자.
- **결정 필요**:
  - focus button = (a) focusMode flag toggle (다른 panel 흐림), (b) viewport zoom-to-fit panel, (c) 둘 다.
  - 사용자 요구: "해당 컴포넌트를 가득 채우는 viewport 이동" = (b) 가 명시. (a) 는 *별 action* 또는 *함께 묶음*.
- **slice**:
  - 1A: LayerTreeView row 에 3 button (min/max/focus) 추가
  - 1B: `sessionStore` 에 `zoomToItem(itemId)` API — Canvas viewport 의 x/y/zoom 을 panel BBox + margin 으로 계산
  - 1C: 기존 toggleMaximize + minimized field 의 PUT mutation wire (PanelNode 의 minimize 진입과 정합)
- **ADR**: ADR-0017 D? amend (Layer list action surface) — *trivial* 일 수 있어 본 plan 만으로 진행 가능.

### Task 2 — Session 진입 시 panel title 사라짐 bug

- **현 상태**:
  - PanelNode.svelte:88 `headerLabel = data.label ?? data.pane_id ?? data.id`
  - LayerTreeView 의 panelDisplayLabel 도 동일 우선
- **가설** (정확한 진단 필요):
  - A. 새 session 의 layout 안 `items[].label` 이 *null* — BE 가 session 간 label 공유 안 함 → 정상 동작인데 *UX 의도* 와 불일치 (사용자는 terminal pool 의 label 이 carry 되길 기대했을 수 있음)
  - B. terminal pool 의 label cache 가 별 path 로 도착 — race 로 처음 paint 에 빈 label, 나중에 pool 응답으로 채움 (PATCH `/api/terminals/:id { label }` 의 broadcast 가 새 session attach 후 도착?)
  - C. terminalPool.refresh 의 trigger 가 session switch 후 누락
- **진단 절차**:
  1. session A 에서 terminal label "Build" 로 설정 → BE 의 어디에 저장되는지 (per-terminal vs per-item)
  2. session B 진입 후 동일 terminal id 가 *items 에 있을 때* (cross-session share) headerLabel 의 raw value 확인
  3. 동일 terminal id 가 *없을 때* 의 의도 (label 자체가 session-local 인가)
- **slice**:
  - 2A: 재현 + raw value 진단 (15분)
  - 2B: 원인별 fix:
    - A 면 → UX 의도 결정 (cross-session label 정책 — ADR-0021 `terminal pool + mirror` amend?)
    - B/C 면 → race fix (sessionStore.setActiveSession 안에서 terminalPool refresh 후 mount, 또는 PanelNode 의 label fallback path 보강)
- **ADR**: B/C 면 fix 만, A 면 ADR-0021 amend.

### Task 3 — Inspector Figma 스타일 shape (fill / stroke / 변형)

- **schema (ADR-0018)**:
  - rect/ellipse: `fill?`, `stroke?`, `stroke_width?`, `corner_radius?` (rect)
  - line: `stroke`, `stroke_width`, `dash?`
  - rotation/scale 필드 *없음* — "변형" 의 의미 결정 필요:
    - (a) width/height edit 만 (현 schema 만으로 가능)
    - (b) rotation 추가 (schema amend → ADR-0018 amend)
- **UI gap**:
  - ItemInfoView.svelte:272-277 stroke/fill 읽기 표시만
  - color picker 컴포넌트 부재 (TextNode 의 color 도 동일)
  - shape-specific Inspector section 부재
- **결정 필요**:
  - 변형 = (a)만 (rotation 안 함) — *권장* — schema amend 회피
  - color picker = 신규 component (`<ColorPicker bind:value />` + hex input + preset swatch). 또는 native `<input type="color">` first 후 폴리시.
- **slice**:
  - 3A: `<ColorPicker>` 컴포넌트 신규 (`lib/ui/ColorPicker.svelte`)
  - 3B: ItemInfoView 의 shape (rect/ellipse) section 확장 — fill / stroke / stroke_width 편집 + mutation path (mutateLayout)
  - 3C: line 의 stroke / stroke_width / dash 편집 (LineNode 도 동일)
  - 3D: TextNode color 의 picker 도 동일 component 로 통일
- **ADR**: ColorPicker 의 token 정책 (preset palette 정의) — *trivial*, 신규 ADR 불요.

### Task 4 — Inspector multi-select 공통 component layout

- **현 상태**:
  - ItemInfoView.svelte 가 *first-selected* item 만 처리 (M.size check 없음 — 추정)
  - 공통 field abstraction 없음 (position / size / z 가 type 별 section 안에 흩어짐)
- **Figma 패턴**:
  - 상단 "Transform" section (position / size / rotation / opacity) — 다중 선택 시 *공통*. 다른 값이면 placeholder "Mixed" 표시.
  - 하단 "Appearance" / "Fill" / "Stroke" section — 다중 선택 시 *공통* 일 때만 표시. type 다르면 hide 또는 "Multiple types" 표시.
- **slice**:
  - 4A: ItemInfoView 를 *2 layer* 구조로 split:
    - Layer A: `<CommonSection>` (transform + appearance — 모든 type 공통)
    - Layer B: `<TypeSection>` (type-specific — terminal / text / shape / line / note / file_path / image / document)
  - 4B: multi-select 시 동작 정책:
    - M.size === 0 → "No selection" placeholder
    - M.size === 1 → Common + Type
    - M.size > 1, type 동일 → Common + Type ("N selected" header)
    - M.size > 1, type 혼합 → Common only ("N selected, multiple types")
  - 4C: "Mixed value" UI — input placeholder + empty value. edit 시 모든 selected 에 broadcast.
- **ADR**: **신규 ADR-0027** (Inspector multi-select layout + mixed-value 정책 + batch mutation 패턴).

### Task 5 — 다중 선택 node 간 alignment

- **현 상태**:
  - text-align 은 *text item 내부* (TextNode horizontal/vertical align) — 다른 개념
  - **node 간 alignment** (selection 내 항목들의 좌/우/상/하/center 정렬, distribute) — 전무
- **Figma 패턴**:
  - 6 alignment button (align left / center / right / top / middle / bottom)
  - 2 distribute button (horizontal / vertical)
  - 다중 선택 시 자동 표시. *2개 이상 선택* 시 alignment, *3개 이상* 시 distribute.
  - alignment 기준: selection 의 BBox (또는 첫 항목 / 마지막 항목 / 가장 큰 항목 — Figma default = BBox)
- **slice**:
  - 5A: `lib/canvas/alignment.ts` — pure function (selectedItems, mode) → 각 item 의 new position 계산
  - 5B: ItemInfoView 의 Common section 안 *Align row* (6 + 2 = 8 button group). M.size ≥ 2 일 때만 표시.
  - 5C: sessionStore 의 batch mutation 패턴 (한 PUT 으로 multiple item position 갱신) — 이미 multi-drag 에서 사용 중 (`682b584`) — 재활용
- **ADR**: ADR-0027 (Task 4 와 합쳐 신규) — alignment mutation 의 batch contract 정의.

---

## 2. 의존 / 우선순위 / 진행 순서

```
                   ┌─ Task 2 (bug fix, isolated) ─── P0
                   │
                   ├─ Task 1 (Layer list buttons) ── P0
                   │   (state foundation 이미 ready)
                   │
ADR-0027 신규 ─────┼─ Task 4 (Inspector multi-select layout) ── P1
  │                │
  │                ├─ Task 3 (color picker + shape fill/stroke) ── P1
  │                │   (Task 4 의 layer A/B 구조 위에)
  │                │
  │                └─ Task 5 (node alignment) ── P1
  │                    (Task 4 의 multi-select layout 안 align row)
```

추천 진행 순서:
1. **Task 2** 진단 + fix (P0, root cause 미확정 — 진단부터)
2. **Task 1** Layer list buttons (P0, state ready, plan-0007 §14.6 의 기존 spec 실행)
3. **ADR-0027 작성** (Inspector multi-select + alignment 정책)
4. **Task 4** Inspector v2 구조 (Common + Type 분리)
5. **Task 3** color picker + shape Inspector section (Task 4 의 Type section 안에)
6. **Task 5** alignment row (Task 4 의 Common section 안에)

각 task 의 commit:
- Task 2: `fix(frontend): session 진입 시 panel title null 회귀 — root cause 분석 + fix`
- Task 1: `feat(frontend): LayerTreeView — minimize/maximize/focus row action + zoom-to-item`
- ADR-0027: `docs(adr): ADR-0027 — Inspector multi-select layout + alignment mutation`
- Task 4: `feat(frontend): Inspector v2 — Common+Type section split + mixed-value 정책`
- Task 3: `feat(frontend): ColorPicker + shape Inspector (fill/stroke/stroke_width 편집)`
- Task 5: `feat(frontend): multi-select alignment row (6 align + 2 distribute) + alignment.ts`

---

## 3. Slice 단위 검증

| Task | 단위 검증 |
|---|---|
| Task 2 | 시나리오: session A 에서 label 변경 → session B 진입 → headerLabel 의 raw value 가 fix 전후 비교 |
| Task 1 | Layer row 에 3 button visible, minimize click → PanelNode 의 minimize 동작, maximize click → toggleMaximize, focus click → viewport zoom to item BBox |
| Task 4 | Multi-select 시 Common section 만 표시, type 혼합 시 Type section hide. Mixed value 의 input placeholder 동작. |
| Task 3 | ColorPicker 의 hex input + preset 동작. shape fill 변경 → mutateLayout PUT → 응답으로 시각 갱신. |
| Task 5 | 2 선택 align-left → 두 항목 left edge 일치. 3 선택 distribute-horizontal → 균등 간격. |

각 PR build: `svelte-check 0 errors` + `npm run build` modules 변화 < 10.

---

## 4. Risk / 후속

| Risk | 완화 |
|---|---|
| Task 2 의 root cause 가 BE 측 (label cache 분리) — FE-only fix 불가 | 진단 결과를 별 report (0051) 로 분리. BE 작업자에게 handoff. |
| Task 4 의 multi-select layout 이 sessionStore mutator 의 batch API 부재 시 race | mutateLayout 의 호출당 한 PUT 패턴 유지 — multi-mutate 도 한 fn 호출 안에서 처리 |
| Task 5 의 alignment.ts 가 line / group / minimized item 의 BBox 정책 모호 | (a) line 은 BBox = endpoints + stroke_width / 2, (b) minimized 는 alignment 에서 제외, (c) group child 는 group 의 BBox 로 — 본 plan 의 후속 명시 |
| Task 3 의 color picker 가 OKLCH / HSL / hex 중 어느 입력 — token color 와 정합 | 일단 hex 만. token-aware 는 후속 ADR-0016 amend |

### 후속 (본 plan scope 밖)

- ADR-0018 D? amend — rotation 필드 추가 검토 (Task 3 의 "변형" 의 (b) 옵션이 채택될 시)
- ADR-0017 D? amend — Inspector 의 chrome 위치 (RightPanel 의 width 등)
- alignment.ts 의 distribute 변형 (BBox center / edge 기준 옵션)

---

## 5. 변경 이력

- 2026-05-16: 초안 — 사용자 요구 5 task batch (Layer actions + Inspector v2 + shape Inspector + multi-select layout + node alignment) 의 plan. P0=2/1, P1=4→3+5, 신규 ADR-0027 명시.
