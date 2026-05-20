# ADR-0027 — Inspector multi-select layout + node alignment mutation

- 상태: **Accepted** (2026-05-16)
- 정본 plan: plan-0010 (UI/UX batch 4)
- 관련 ADR: ADR-0017 (chrome layout), ADR-0018 (canvas item v2), ADR-0024 (layer tree + z-index)
- 관련 report: 0049 §4.2 (text-align Inspector ship), 0050 (lasso/selection regression)
- 작성자: agent (system-architect role) — 사용자 요구 5 task batch 의 D2~D5

---

## Context

- ItemInfoView (RightPanel Inspector) 가 *first-selected* item 만 처리 (`M.values().next()` 1건). M.size > 1 시 첫 항목만 노출 — 다른 항목의 시각 단서 0.
- 공통 field (position / size / z / visibility / locked / minimized) 가 type-specific section 안에 흩어져 있어 *Common* abstraction 부재.
- Figma 패턴: 상단 "Transform" (공통) + 하단 "Appearance" / "Fill" / "Stroke" (type-specific) — 다중 선택 시 *공통* 만 노출, 충돌하는 값은 "Mixed" placeholder.
- 사용자 요구: (i) 다중 선택 시 공통 component 중심 layout, (ii) node 간 alignment (left/center/right/top/middle/bottom + distribute).
- 기존 mutation 패턴: `mutateLayout(active.name, (cur) => ({...cur, items: cur.items.map(...)}))` — 한 PUT 으로 multiple item position 갱신 가능 (multi-drag commit `682b584` 가 이미 사용).

---

## Decisions

### D1 — Inspector 의 2-layer 구조

- **Common section** (모든 type 공통): position (x, y), size (w, h), z-index, visibility, locked, minimized, label.
- **Type section** (type-specific): terminal (pane_id / dead / pool entry), text (text_align / vertical_align / color / font_size), shape (fill / stroke / stroke_width), line (stroke / stroke_width / endpoints), note (color), file_path (path), image (url), document (text).
- Common 이 상단, Type 이 하단. 두 section 의 *border* 로 시각 분리.

### D2 — Multi-select 표시 정책

| M.size | type 동질성 | 표시 |
|---|---|---|
| 0 | — | "No selection" placeholder |
| 1 | — | Common + Type ("Item / `{type}`") |
| ≥ 2 | 모두 동일 type | Common + Type (`{N} {type}s selected`) |
| ≥ 2 | 혼합 | Common only (`{N} items selected · multiple types`) + Type section hidden |

- Type section 의 *내용* 도 다중 동일 type 시 mixed value 노출 (D3 참조).

### D3 — Mixed value 표현

- 다중 선택의 어떤 field 가 *모든 selected item 에서 동일* 하면 그 값 직접 노출.
- *다른 값이 1개라도 섞임* 이면 input 의 *value 를 비워* 두고 placeholder 에 `Mixed` 표시 (input 의 색은 muted token).
- 사용자가 비어 있는 input 에 값을 입력하면 *모든 selected item 에 broadcast* (한 mutateLayout PUT).
- boolean field (visibility / locked / minimized) 의 mixed 는 *indeterminate state* (checkbox 의 dash icon).

### D4 — Alignment mode (6 + 2)

- **Align** (6 button — selection 의 BBox 기준):
  - left, center-x, right (horizontal)
  - top, center-y, bottom (vertical)
- **Distribute** (2 button — selection 내 항목 간 균등 간격):
  - horizontal (좌 - 우 BBox edge 기준 균등)
  - vertical (위 - 아래 BBox edge 기준 균등)
- 표시 조건:
  - M.size ≥ 2 → align 6 button 표시
  - M.size ≥ 3 → distribute 2 button 표시
  - M.size < 2 → alignment row 전체 hide

### D5 — Alignment 기준 — Selection BBox

- *Align* 의 기준 = selection 의 union BBox (left = `min(item.x)`, right = `max(item.x + item.w)`, 마찬가지로 top/bottom).
- *Distribute* 의 기준:
  - horizontal: 두 *극단* (leftmost, rightmost) 의 x 는 고정, 중간 항목 들의 *center* 가 균등 간격으로 분포.
  - vertical: 동일 패턴 (top / bottom 고정, 중간 center 균등).
- Figma 의 *공식* 동작과 일치 (center 기준 균등).

### D6 — Alignment mutation 의 batch contract

- 모든 align/distribute 액션 은 **한 mutateLayout PUT** 으로 처리 — 부분 commit 금지 (race 회피).
- mutation function 은 `(cur) => ({...cur, items: cur.items.map(it => M.has(it.id) ? {...it, x: newX, y: newY} : it)})` 패턴.
- BE 측에서 single PUT 의 atomic update — 이미 GET / PUT / PATCH 가 동일 layout file 의 transaction 이라 OK.

### D7 — 제외 / 특수 처리

- **locked** item: alignment 의 *target 에서 제외*. selection BBox 계산엔 포함 (사용자가 "lock 된 항목 기준 정렬" 의도 가능). 단 lock 된 item 의 position 은 갱신 안 함.
- **minimized** item: 일반 항목으로 처리 (시각은 collapsed 지만 BBox 는 item.w/h 유지).
- **line** item: BBox = (min(x, x2), min(y, y2)) ~ (max(x, x2), max(y, y2)). align 시 endpoint *둘 다* 평행 이동 (delta 적용). distribute 도 동일.
- **group child** item: group 자체의 BBox 가 아닌 *child 각각의 BBox* 로 align. group 정책 의 별 amend 필요 시 후속.

### D8 — Distribute 의 최소 N

- N < 3 일 때 distribute 표시 안 함 (정의상 의미 없음). N === 3 일 때 균등 분포 OK (3 항목 중 가운데 1개 의 위치만 변경).

### D9 — Inspector 의 alignment row 위치

- *Common section* 안 (상단). M.size ≥ 2 일 때만 row 표시. row 의 위치는 Common 의 size field 바로 아래.

---

## Consequences

### Positive

- Figma 와 동등한 multi-select Inspector — UX 친숙도 ↑
- *공통 / type-specific* 분리 → Inspector 의 추가 type (image / document 등) 도 일관 패턴 으로 확장 가능
- Alignment 가 mutation 패턴 (multi-drag commit) 재활용 → 새 contract 없음, race 안전

### Negative

- ItemInfoView 의 *대규모 refactor* 필요 — 기존 single-item path 와 multi-item path 통일
- mixed value 의 *input UX* (placeholder vs 빈 input) 정확한 시각 결정 필요 — 본 ADR 의 §D3 가 *행동* 명시, *시각 토큰* 은 plan 의 slice 4C 에서 결정
- alignment.ts 의 pure function 화 + 다양한 type (line / group child 등) 커버 — bug 표면적 증가. plan-0010 §3 의 검증 시나리오 (S5-5A 등) 로 mitigate

### 후속

- ADR-0018 D? amend — rotation / scale 필드 추가 검토 (Task 3 의 "변형" 옵션 (b) 의 확장)
- 본 ADR 의 alignment row 가 *Toolbar 상단 sub-bar* 로 도 migrate 검토 (Figma 의 second-tier 패턴)
- group / inheritance 의 alignment 정책 별 amend (current = child 각각, future = group BBox 옵션)

---

## 변경 이력

- 2026-05-16: Accepted — plan-0010 Task 4/5 의 정본 ADR. 5 task batch 중 D1~D9 의 결정 사항 정합.
