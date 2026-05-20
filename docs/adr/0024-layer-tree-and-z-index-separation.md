# ADR-0024: Layer Tree 와 Z-Index 분리 — 작업공간 canvas 모델

- 상태: Accepted (2026-05-15)
- 일자: 2026-05-15 (Proposed + Accepted, G24 grilling)
- 결정자: agent (frontend-architect role) + user grilling G24
- 근거 grilling: G24 / G24 재제안
- 관련 ADR: ADR-0010 (Group tree 정합 — z 무관 명시 amend), ADR-0018 (Canvas Item Data Model — z field 의 의미)
- 관련 SSoT: `docs/ssot/canvas-layout-schema.md` (z 의 mutation 규칙), plan-0007 §14 (Layer list 컴포넌트)

## 맥락

Figma / Sketch / Photoshop / Illustrator 의 디자인 도구는 *Layer tree order = canvas z-index* 의 완전 동기화 패턴 (드래그 reorder → z 재책정). 이 동기화의 자연성은 *시각 결과물 canvas* — z-index 가 디자인 의도 그 자체 — 라는 모델 위에서 성립한다.

gtmux 의 canvas 는 다른 모델 — **작업공간 (workspace)**:
- Element 는 작업 도구 (Terminal Panel, Note, file_path bookmark, …) 의 *시각적 representation*.
- Group 의 tree 는 *논리적 조직* (예: "build cluster", "monitoring") 이지 *시각 stacking* 이 아님.
- Z-index 는 *우연한 overlap 의 해소 수단* — 디자인 의도 자체가 아님.
- Layer list 의 1차 가치 = element 의 *목록 + 다중 선택 + 가시화/lock 토글 + bulk action* — *시각 stacking 조정* 은 부차.

이 모델 위에서 Tree=Z 동기화는 *인위적 결합* — 사용자가 *조직 의도* 와 *시각 stacking 의도* 를 동일 액션으로 표현하도록 강제. Figma 사용자에게는 친숙해도 *작업공간 canvas* 사용자에게는 의도 외 부작용 (예: organization 만 바꾸려 drag 했는데 z 도 함께 변경) 발생.

## 결정 (Decisions)

### D1. Layer tree order 와 canvas z-index 의 완전 분리

- **Tree order** = pure organization (그룹 안 자식 순서, 그룹 sibling 순서). Drag reorder 는 *organization* 만 변경 — z 에 영향 없음.
- **Z-index** = canvas 위 시각 stacking. Tree 와 무관하게 별 mutation path.
- 두 값은 schema 의 *독립 field* — `parent_id + order_in_parent` (tree) 와 `z` (canvas stacking) 가 별 영속 데이터.

### D2. Z mutation 의 정책 (사용자 액션)

새 item 생성 시:
- `z = max(z) + 1` — 즉시 canvas 최전면 (ADR-0018 D3 정합).

기존 item 의 z 변경:
- Panel header more menu (…) / canvas right-click context menu 에 4 액션:
  | 액션 | 효과 | 키보드 단축키 |
  |---|---|---|
  | Bring to front | `z = max(z) + 1` | Shift + `]` |
  | Send to back | `z = min(z) - 1` | Shift + `[` |
  | Bring forward | `z++` (다음 큰 값과 swap) | `]` |
  | Send backward | `z--` (다음 작은 값과 swap) | `[` |
- 다른 z mutation 경로 없음 (Tree drag 는 z 안 바꿈, Layer list 의 Z 모드 정렬도 read-only).

### D3. Group 은 z-index 없음

- Group = pure organization (자식 묶음 + label/color 메타).
- Group 자체의 z 값 없음 — schema 에서 `groups[]` 에 z field 없음.
- 자식들의 z 는 group 의 z 와 무관 — *flat global z 공간* (모든 items 가 한 z 공간 공유).

이유: Group 이 z 를 가지면 *Figma 의 z 컨테이너* 모델로 부분 동기화가 들어오며 사용자 의도 외 결합 발생. 작업공간 canvas 의 group 은 *논리 묶음 only*.

### D4. Layer list 의 표시

- **Row 표시 (always)**: type icon + label + z badge (small text, ex: `z:10`) + visibility/lock toggle + state indicators (hidden/locked/minimized/dead).
- **상단 toggle [Tree | Z]**:
  - **Tree 모드 (default)**: group 계층 tree 표시 (drag reorder/reparent, group collapse/expand). 각 row 에 z badge.
  - **Z 모드**: flat 정렬 (z 내림차순, 위가 z 최대 = canvas 최전면). 그룹 구조는 *시각적으로 표시 안 됨* — z 비교만의 모드. *Read-only 정렬* (이 모드에서 drag reorder 비활성). z badge 옆에 그룹 label 작은 hint.
- Z 모드의 가치 = "어느 element 가 정확히 위에 있는지" 확인. 변경은 여전히 context menu / 단축키.

### D5. Z 충돌 처리

- 동일 z 값을 가진 item 두 개가 우연히 생기는 경우 (예: 사용자 명시 z 변경 시 race) — *insertion order 가 tie-break*. Schema 영속 시 (`items[]` 의 array order) 보존 → reload 후에도 같은 시각 stacking.
- Bring forward / Send backward 가 swap 인 이유 = z 의 *연속 정수* 가정 안 하기 위함. 어떤 z 값이든 OK, swap 으로 *상대 순서만* 보존.
- Z 의 정수 범위: i32 충분 (실 사용 시 1000 단위 미만). 음수 허용 (Send to back 시 발생).

### D6. ADR-0010 (Group tree) 정합

- ADR-0010 의 *group propagation* 규칙 (visible AND, lock OR — ADR-0010 의 디테일은 별도) 은 그대로 유지.
- 본 ADR 은 *group 의 z field 없음* 을 명시 — ADR-0010 의 group 정의 amend 필요.
- Group 안 자식들의 z 는 *group 의 자식* 이지만 *z 공간은 global* 이므로 group sibling 의 자식들과 직접 z 비교 가능.

### D7. ADR-0018 schema 정합

- ADR-0018 D3 의 `z` field 는 그대로 (item-level, integer).
- ADR-0018 D3 의 *새 item z = max(z) + 1* 그대로.
- ADR-0018 의 `groups[]` 에 z field 추가 *안 함* (확인). 본 ADR D3 의 명시.

### D8. 단축키의 P1+ 처리

- 단축키 ]/[ + Shift 변형은 *키보드 등록 시스템 (G26, P1)* 의 일부.
- MVP 에서는 *context menu + Panel header more menu* 만 제공. 단축키는 G26 grilling 이후.

## 영향

### Code
- **Frontend**:
  - `lib/sidebar/LayerList.svelte` (큰 amend) — Tree/Z toggle, z badge 표시, Tree 모드의 drag reorder = organization 만
  - `lib/canvas/items/PanelNode.svelte` 의 header more menu — 4 z 액션 추가
  - `lib/canvas/ContextMenu.svelte` (신규 또는 amend) — canvas right-click 의 4 z 액션
  - `lib/stores/zStore.svelte.ts` (신규) 또는 panel store amend — Bring/Send 로직
  - 단축키 등록 (G26 후 P1+)

- **Backend**:
  - 변경 없음 (z 는 schema 의 일부, 영속만).

### ADR
- **ADR-0010 amend** — group 은 z field 없음 명시 (header amend).
- **ADR-0018 amend** — D3 의 z field 가 *Tree 와 무관하게 mutate* 됨을 명시.

### Docs
- `CONTEXT.md` 의 *Canvas Item* / *Group* 어휘 영역에 본 ADR 한 줄 reference.
- plan-0007 §14.6 (Layer list V2) amend.
- plan-0007 §14.5 (Panel header more menu) amend.

### 보안
- 변경 없음.

## 대안 검토

### A1. Figma 패턴 — tree=z 완전 동기화
**거부.** 작업공간 canvas 의 모델 (조직 != 시각 stacking) 위에서 인위적 결합.

### A2. PowerPoint 패턴 — tree 위 = canvas 뒤 (배경)
**거부.** Figma 사용자에게 의외 + 시각 stacking 의 mental model 도 약화.

### A3. C+ — Layer row 에 z badge 만, Z 모드 toggle 없음
**검토 후 거부** (G24 재제안). 사용자가 element 의 정확한 z 비교 위해 Z 모드 보기 가치 있음 — flat 정렬 모드 제공이 자연.

### A4. C- — z 정보 layer list 에 표시 안 함 (visual canvas 만)
**거부.** 우연한 겹침의 진단 어려움.

### A5. Group container z (Figma 정합)
**거부.** D3 의 이유 — 사용자 의도 외 결합 발생.

## Amend (2026-05-16) — Layer list V2 (Multi-select + Drag reorder/reparent) UX 구체화

### 결정

D1 (Tree order ≠ Z) 의 organization 측면을 FE 가 사용자에게 어떻게 노출하는지 구체화. ADR-0017 amend ⑥ 와 짝.

**Multi-select 정책 (LayerTreeView.selectNode):**
- Plain click → `setM([id])` + `selectionAnchor = id`.
- Cmd / Ctrl + click → `toggleM(id)` + anchor 갱신.
- Shift + click → `visibleRangeIds(anchor, id)` 의 inclusive range 일괄 `setM` (또는 Cmd+Shift 결합 시 `addToM`). anchor 는 유지 — 동일 anchor 에서 연속 shift-click 가능 (Finder/VSCode 컨벤션).
- anchor 가 invisible 화 (ancestor collapse) 되어도 fallback 으로 target 만 toggle.

**Drag reorder/reparent (HTML5 native drag):**
- 모든 row 에 `draggable={layerMode === 'tree' && !isItemLocked(id)}`.
- Z mode 비활성 — z 변경은 D2 의 4 액션 전용. drag 가 z 를 만지지 않음 (Tree order ≠ Z 의 사용자 가시성).
- Multi-drag: dragged 가 M 에 포함되어 있으면 M 전체. 아니면 dragged id 만.
- Cycle 보호: dragged group 의 descendants 가 target 이면 drop 거부.
- Locked guard: locked row 는 dragstart preventDefault. M 안 locked + unlocked 섞이면 unlocked 만 drag.
- mouseY ratio (target row 높이 기준):
  - `< 0.25` → 'before' (행 위 2px accent line)
  - `> 0.75` → 'after' (행 아래 2px accent line)
  - 중간 + group row → 'inside' (accent 12% tint + dashed outline = reparent into group)
  - 중간 + panel row → before/after 양분 (item 은 inside 컨테이너 아님)

**Mutation 모델:**
- 'inside' (target = group): dragged.parent_id = target.id. dragged group 은 target 안 max(order) + 1.
- 'before'/'after': dragged.parent_id = target.parent_id. dragged group 들을 target 의 order 직전/직후로 삽입 + 형제 group 들 sequential 재번호.
- Single `mutateActiveLayout` call 로 items.parent_id + groups.parent_id + groups.order 동시 atomic 갱신.

**Item sibling order 한계:**
- `ItemCommon` 에 `order` field 미존재 (현 schema v2). 따라서 item 의 sibling 안 *정확 위치* 는 보장 X — parent_id reparent 만 보장하고 sibling 안 순서는 id-sort 폴백.
- BE schema v3 (item.order 또는 list_order field) 가 추가되면 본 LayerTreeView.commitReparent 가 즉시 활용 가능 (코드 stub 이 이미 동작 — order 필드를 그대로 쓰면 됨).

### 이유

1. **Tree organization 의 1차 가치**: drag reorder/reparent 가 ADR-0024 D1 의 "사용자가 *조직* 한다" 의 실질 표현. 본 amend 없이는 D1 이 "tree mode 가 분리되어 있다" 까지만 보장하고 mutation UX 부재.
2. **Z mode 와 명확 분리**: drag UX 가 Z mode 에서는 비활성 — 사용자가 "Z mode 에서 drag 하면 z 가 바뀐다" 를 기대하지 않도록 차단. D2 의 4 액션 정합.
3. **Multi-select + drag 결합**: M 에 다중 선택한 set 을 한 번에 reparent — bulk action 1차 가치 (D2 영향 영역 동일).
4. **Cycle / locked guard**: ADR-0010 D2 (tree 사이클 차단) + effectiveLocked 정책 정합. drag 차단으로 invalid state 발생 source 자체를 봉쇄.

### 영향

- LayerTreeView 의 `selectionAnchor` 상태 + `visibleRangeIds` helper + drag handler 6개 (onRowDragStart / Over / Leave / Drop / DragEnd) 신규.
- `commitReparent` mutation 이 items / groups 양쪽을 동시 갱신.
- Drop indicator CSS — `.row.drop-before::before` / `.drop-after::after` / `.drop-inside` / `.dragging`.

### 후속

- BE schema v3 — item 에 `order: number` 필드 추가 → `commitReparent` 의 'before'/'after' 가 item 에도 정확 위치 적용.
- Marquee selection (sidebar rectangle drag) 은 deferred (Figma/Finder 다름, icon multi-select 으로 충분).

## 변경 이력

- 2026-05-15: 초안 + Accepted. G24 grilling (재제안 후 C++ 선택) 합본.
- 2026-05-16: Amend — Layer list V2 multi-select (Cmd/Ctrl/Shift) + HTML5 drag reorder/reparent (before/inside/after with drop indicator). multi-drag / cycle 보호 / locked guard / Z mode 비활성. Item sibling order 의 BE schema v3 의존 명시.
- 2026-05-16 (0045 P0 후속): **Identity-stable node adapter 패턴 명시**. `Canvas.svelte` 의 `flowNodes` derived 가 `items` 의 매 변경마다 새 Node array+object 를 만들면 SvelteFlow 가 prop identity churn 으로 판단 → 내부 측정/정렬 effect → parent rebuild loop → `effect_update_depth_exceeded`. 본 D1 (Tree order ≠ Z) 정합을 *implementation 측* 에서 보장하려면 id-cache + signature 패턴 필요: ① per-derived-pass `Map<id, { sig: string; node: Node }>` 새 Map. ② signature = `${effVisible}|${effLocked}|${selected}|${mMulti}|${JSON.stringify(item)}` — common (id/type/parent_id/x/y/w/h/z/visibility/locked/minimized/label) + derived (effective visible/locked, M.has, M.size>1) + type-specific payload (line.x2/y2, text.text/font_size/color/align, shape.stroke/fill, etc.) 모두 cover. ③ 동일 signature 시 이전 Node ref 재사용 — SvelteFlow prop unchanged. 본 패턴은 V2 drag reorder (parent_id + groups.order 변경) / multi-select (M 변경) / Z mutation (z 변경) / visibility/locked toggle 모두 정합 — 모든 mutable field 가 signature 에 포함되어야 stale render 방지 (signature 누락 = critical bug). 50 entry 기준 < 1ms 비용, GC pressure 무시. 본 amend 의 motivation 은 `docs/reports/0045-refresh-session-reconnect-loop-analysis.md` §6 P0-A 의 정통 fix.
