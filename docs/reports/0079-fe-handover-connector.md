# 0079 — FE Handover: Component connector (ADR-0036) renderer + creation + Inspector

- 작성일: 2026-05-19
- 작성 주체: agent (frontend-architect role, 본 batch dispatch)
- 정본 cross-link:
  - **결정 출처**: [ADR-0036](../adr/0036-canvas-component-connector.md) — Canvas component connector (Accepted 2026-05-19)
  - **schema amend**: [ADR-0018 D12 amend](../adr/0018-canvas-item-data-model.md) — connector variant
  - **BE 짝 (prerequisite)**: [0078-be-handover-connector.md](./0078-be-handover-connector.md) — schema.rs + validate + BBox 재계산. **본 FE 작업은 BE land 후 시작**
  - **prior land 정합**: ADR-0030 D11 + ADR-0017 D6 amend ⑧⑨ wire (Phase A/B/C, 2026-05-19) — clipboardStore / editingShortcuts / ContextMenu 의 정합 패턴
- 관련 ADR: ADR-0017 (Chrome — Toolbar2/ContextMenu/Inspector), ADR-0024 (Z-index), ADR-0027 (Multi-select Inspector), ADR-0028 (Undo/Redo), ADR-0030 (Clipboard — D9 connector-aware paste)

## 핵심 원칙 — 거짓 ship 방지

[`0073` §0 의 5 원칙](./0073-fe-handover-from-0071-audit.md#핵심-원칙---거짓-ship-방지) 그대로. 특히 본 batch 는 *7 영역* 의 cross-cutting (renderer / gesture / Inspector / ContextMenu / clipboard / cascade delete / z-order) — 각 영역의 acceptance 가 *독립 검증* 가능해야.

---

## 0. Self-grilling 결정 사항

### Q1. Connector 의 SvelteFlow 표현 — Node 또는 Edge?

**결정**: ✅ **Edge** (SvelteFlow 의 native edge layer). 이유:
- *Node 아래에 그려짐* (SvelteFlow default) — ADR-0036 D5 의 z-order sub-space 자연 충족.
- Edge layer 가 단일 SVG `<g>` — 모든 connector 가 한 layer 에 그려져 rendering 효율.
- SvelteFlow 의 *custom edge component* 패턴 (e.g. `BezierEdge`, `StraightEdge`) 재사용 가능.

**의외 처리**:
- SvelteFlow 의 edge 는 `source`/`target` 의 *node id* 와 `sourceHandle`/`targetHandle` 의 *handle id* 로 binding. ADR-0036 D1 의 `from_id`/`to_id` + `from_anchor`/`to_anchor` 와 1:1 매핑.
- 단 connector *도 layout 의 item* 이라 sessionStore.items 에는 일반 item 처럼 들어감. *SvelteFlow nodes 배열 build 시* connector 만 제외하고 *edges 배열* 로 분리.

### Q2. Anchor handle 의 DOM 구조 — Node 안 또는 별 layer?

**결정**: ✅ **각 Node 의 자체 markup 안**. PanelNode/TextNode/NoteNode 등 모든 Node component 가 동일한 `<AnchorHandles />` 자식 (별 svelte component) 를 가짐. SvelteFlow 의 `<Handle type="source" position={Position.Top} id="N" />` API 활용.

이유: SvelteFlow 가 handle 의 position 자동 계산 (node 의 좌표 + handle 의 relative position). 별 layer 로 그리면 handle 좌표 직접 계산 부담.

5 vs 9 anchor:
- ADR-0036 D12 — *Default* hover 시 5 point (4 edge midpoint + center). *Connector tool 활성* (P1, D7-(b)) 시 9 point.
- MVP 는 **항상 5 point** 로 단순화. 9 point 는 P1 (toolbar 도구 land 시 동시).

### Q3. Creation gesture — drag-out 의 임시 wire 표현?

**결정**: ✅ **SvelteFlow 의 `onConnect` / `onConnectStart` / `onConnectEnd` callback 활용**. drag 중 임시 wire = SvelteFlow native `connectionLine` rendering.

```svelte
<SvelteFlow
  oncconnectstart={onConnectStart}
  onconnect={onConnect}
  onconnectend={onConnectEnd}
  connectionLineType="straight"
  connectionMode="loose"
>
```

`onConnect({ source, target, sourceHandle, targetHandle })` 안에서:
- `source/target` = item UUID (Q1 의 node id 매핑)
- `sourceHandle/targetHandle` = anchor name (N/E/S/W/center)
- `commitNewItem` 으로 connector item append (createConnectorItem helper)

### Q4. Z-order — 본 MVP 에서 별 처리?

**결정**: ✅ **별 처리 0** — Q1 의 *SvelteFlow edge layer* 가 자연스럽게 node 아래. zStore 의 4 액션은 connector 에 *no-op* (selection 안에 connector 있으면 skip).

P1 검토: connector 끼리의 z 순서 (e.g. 여러 wire 가 겹칠 때 위/아래) — *SvelteFlow edges 배열의 순서* = render 순서. 본 MVP 는 *items[] 의 z field 정렬* 그대로 — connector 끼리는 z 순서로 정렬.

### Q5. Cascade delete — sessionStore.applyDeletion 의 transform?

**결정**: ✅ **`applyDeletion` 의 transform 안에서 cascade**.

```ts
applyDeletion(ids: readonly string[], ...) {
  // ... existing logic ...
  const before = this.layoutSnapshot();
  // BE/FE handover (0079) — connector cascade: endpoint 가 ids 안 connector 도 cascade.
  // 단 applyDeletion 은 1개씩 delete endpoint 호출하는 구조 — 한 PUT 으로 묶는 별 path 필요.
}
```

문제: 현 `applyDeletion` 은 `DELETE /api/sessions/:name/items/:id` 의 *개별 endpoint* 사용. cascade 를 *atomic* 으로 처리하려면 *layout PUT* 으로 변경 필요 — 큰 refactor.

**MVP 타협**: cascade 도 *순차 DELETE* 로 처리 — 먼저 일반 item 들 delete, 그 다음 *그 item 을 endpoint 로 가지는 connector 들* delete. BE 의 validate 가 *orphan connector* 를 reject 하므로 *역순* 위험 — connector 부터 먼저 delete.

```ts
async applyDeletion(ids: readonly string[], options) {
  // Step 1 — connector cascade scan. 삭제 대상 ids 의 어떤 item 을 endpoint 로 가지는 connector 의 id 수집.
  const cascadeIds: string[] = [];
  const idSet = new Set(ids);
  for (const it of this.items.values()) {
    if (it.type !== 'connector') continue;
    if (idSet.has(it.from_id) || idSet.has(it.to_id)) cascadeIds.push(it.id);
  }
  // Step 2 — connector 먼저 delete (BE validate 의 orphan reject 회피).
  const ordered = [...cascadeIds, ...ids.filter((id) => !cascadeIds.includes(id))];
  // ... existing for loop with ordered ...
}
```

근거: BE 가 validate 시 orphan reject (Q2 of BE handover) — connector 가 사라진 endpoint 가리키면 reject. FE 가 *connector 먼저* 처리 필수.

### Q6. Clipboard / Duplicate — ADR-0030 D9 정합

**결정**: ✅ **clipboardOps.pasteItems 의 connector 분기**. selection 의 item 들 안에 connector 가 있을 때:

- *양 endpoint (from_id + to_id) 모두 selection 안* → connector 도 paste, 단 endpoint id 도 새 UUID 로 *remap* (UUID 매핑 table 사용)
- *한 endpoint 만 selection 안* → connector *제외 from paste* (D9 정합)
- *둘 다 selection 밖* → connector 자체가 selection 에 없을 수도 (= source 에 없으면 제외 자연)

```ts
function cloneWithOffset(src, bboxX, bboxY, dx, dy, idMap) {
  const newId = crypto.randomUUID();
  idMap.set(src.id, newId);
  // ... existing offset logic ...
  if (out.type === 'connector') {
    // remap endpoints — only if BOTH endpoints are in the same paste batch.
    out.from_id = idMap.get(out.from_id) ?? null;
    out.to_id = idMap.get(out.to_id) ?? null;
    if (out.from_id === null || out.to_id === null) {
      // dangling — exclude from paste.
      return null;
    }
  }
  return out;
}
```

`pasteItems` 의 `fresh = sources.map(...)` 에서 `null` filter out. 단 *order* 가 중요 — endpoint item 들이 *connector 보다 먼저 cloneWithOffset 호출* 되어 `idMap` 에 등록되어야. → `sources.sort((a, b) => (a.type === 'connector' ? 1 : 0) - (b.type === 'connector' ? 1 : 0))` 로 connector 를 *마지막* 으로.

### Q7. ContextMenu / Inspector — connector 의 specific entry?

**결정**: ✅ **ContextMenu 는 *공통 entry 만*** (Copy/Cut/Paste/Delete/Hide/Lock — 일반 item 과 동일). connector-specific 액션 (direction toggle / anchor change) 은 **Inspector 의 connector section** 에만 노출.

Inspector connector section (RightPanel 의 InspectView 안):
- Direction picker — `[Uni A→B] [Bi A↔B] [None A—B]` 3-button segmented
- Endpoint picker — `from_id` / `to_id` 의 *현재 item label* 표시 + `[Reassign]` 버튼 (P1, target item 클릭 모드)
- Anchor picker — `from_anchor` / `to_anchor` 의 9-point dropdown (또는 grid picker)
- Style — stroke color / width / dash
- Head per-end — `head_from` / `head_to` 4-option dropdown
- Routing — MVP 는 read-only (`straight` 고정 표시). P1 의 picker.

### Q8. Anchor handle 의 hover detection?

**결정**: ✅ **SvelteFlow 의 `connectableStart={false}` 기본 + 사용자 hover 시 dynamic enable**. Default 는 handle 의 connection initiation 봉인 — *Node 의 hover 300ms+* 가 trigger.

```ts
// Node 안의 자체 state
let isHovering = $state(false);
let hoverTimer: ReturnType<typeof setTimeout> | null = null;
// pointerenter → 300ms grace → isHovering = true → handles visible + connectableStart
// pointerleave → cancel timer + isHovering = false (단 drag 중이면 유지 — onConnectStart 가 active 시 무시)
```

근거: ADR-0036 D12 의 hover 300ms grace 정합. 너무 빠르면 mouse pass-through 마다 handle flicker.

---

## §A. Task 목록

| Task | 영역 | 출처 | 예상 소요 |
|---|---|---|---|
| **FE-A** | `lib/canvas/edges/ConnectorEdge.svelte` 신규 — SvelteFlow custom edge, straight routing, direction-aware head rendering | ADR-0036 D3/D4 | 1 commit |
| **FE-B** | `lib/canvas/AnchorHandles.svelte` 신규 + 모든 Node component 에 mount — 5 anchor (N/E/S/W/center) hover-revealed handles | ADR-0036 D12 | 1 commit |
| **FE-C** | Canvas.svelte 의 onConnect/onConnectStart/onConnectEnd 핸들러 + createConnectorItem helper (`lib/canvas/itemFactory.ts`) | ADR-0036 D7 (a) | 1 commit |
| **FE-D** | `clipboardOps.svelte.ts` 의 connector-aware paste (Q6) + sort + idMap remap + null filter | ADR-0030 D9 (D12 amend) | 1 commit |
| **FE-E** | `sessionStore.applyDeletion` 의 connector cascade scan + 순서 조정 (Q5) | ADR-0036 D6 | 1 commit |
| **FE-F** | `lib/chrome/InspectView.svelte` (또는 ItemInfoView) 의 connector section — direction segmented + anchor dropdowns + style/head pickers | ADR-0036 D8 + ADR-0027 정합 | 1 commit |
| **FE-G** | `nodeAdapter` / `edgeAdapter` — sessionStore.items 의 connector 분리 (edge 배열) + 나머지 node 배열 | Q1 | 1 commit |

총 7 commit. 또는 *FE-A + FE-B + FE-G + FE-C* (renderer + gesture) 한 commit + *FE-D + FE-E* (clipboard + cascade) 한 commit + *FE-F* (Inspector) 한 commit = 3 commit 묶음 가능.

---

## §B. Anchor — 변경 대상

| # | 파일 | 변경 |
|---|---|---|
| B1 | `codebase/frontend/src/lib/types/canvas.ts` | `ConnectorItem` interface 신규 (ADR-0018 D12 payload) + `isConnector` type guard 추가. `CanvasItem` discriminated union 확장. |
| B2 | `codebase/frontend/src/lib/canvas/edges/ConnectorEdge.svelte` (신규) | SvelteFlow custom edge. straight path + head rendering (arrow/circle/diamond/none, per-end). stroke / stroke_width / stroke_dash 적용. label midpoint 표시. |
| B3 | `codebase/frontend/src/lib/canvas/AnchorHandles.svelte` (신규) | 5 SvelteFlow Handle (N/E/S/W/center). hover 300ms grace 로 visible. `connectableStart`/`connectableEnd` dynamic. |
| B4 | `codebase/frontend/src/lib/canvas/PanelNode.svelte`, `TextNode.svelte`, `NoteNode.svelte`, ... (모든 Node) | `<AnchorHandles ... />` mount (slot 또는 직접). pointerenter/leave 핸들러 추가. |
| B5 | `codebase/frontend/src/lib/canvas/Canvas.svelte` | `onConnect` callback wire — `createConnectorItem` 호출 + commitNewItem. `onConnectStart`/`onConnectEnd` 의 임시 wire 표시 (SvelteFlow default 활용). |
| B6 | `codebase/frontend/src/lib/canvas/itemFactory.ts` | `createConnectorItem({from_id, to_id, from_anchor, to_anchor})` helper 추가. defaults: `direction="uni"` / `head_to="arrow"` / `routing="straight"` / `stroke="#0d99ff"` / `stroke_width=2`. BBox 는 `{x:0,y:0,w:0,h:0}` placeholder (BE 가 재계산). |
| B7 | `codebase/frontend/src/lib/canvas/clipboardOps.svelte.ts` | Q6 의 sort + idMap + connector remap + null filter. |
| B8 | `codebase/frontend/src/lib/stores/sessionStore.svelte.ts` 의 `applyDeletion` | Q5 의 connector cascade scan + order. |
| B9 | `codebase/frontend/src/lib/chrome/ItemInfoView.svelte` (또는 connector-specific InspectView) | Connector section — 7 sub-control (direction / endpoints / anchors / stroke / stroke_width / heads / routing read-only). M.size===1 + selected item.type==='connector' 시 노출. |
| B10 | `codebase/frontend/src/lib/canvas/nodeAdapter.ts` (or equivalent) | sessionStore.items 를 nodes / edges 두 array 로 분리 — connector 만 edges. |

---

## §C. Acceptance criteria

### C1. Connector 생성 (drag-out)
- Item A hover 300ms → 5 anchor handle 표시
- handle 에서 drag → 임시 wire 표시 (pointer 따라)
- Item B 위에서 release → connector item commit
- 사용자가 두 panel 을 연결한 wire 가 *node 아래 layer* 에 표시

### C2. Direction toggle
- Connector 선택 → Inspector 의 direction picker 3-segment
- `[Uni]` 클릭 → wire 가 `A → B` (target end 만 arrow)
- `[Bi]` 클릭 → wire 가 `A ↔ B` (양 끝 arrow)
- `[None]` 클릭 → wire 가 `A — B` (양 끝 head 없음)

### C3. Endpoint 자동 follow
- Item A 를 drag → connector 의 source endpoint 가 따라 움직임 (BBox 자동 재계산은 BE 가 PUT 직전 처리)
- 즉 *FE 의 SvelteFlow 가 source/target node id 기반으로 자동 update* — connector.x/y/w/h 직접 mutate 0
- PUT 후 응답 layout 의 connector BBox 가 endpoint 좌표 기준

### C4. Cascade delete
- Item A 선택 → Delete (Backspace / Cmd+X / ContextMenu)
- A 를 endpoint 로 가지는 connector 들이 *동일 mutation* 으로 cascade delete
- Cmd+Z → A + connector 둘 다 복원 (ADR-0028 의 1 history entry)

### C5. Clipboard / Duplicate
- Item A + B + (A↔B connector) 모두 selection → Cmd+C → Cmd+V
- 새 A' + B' + (A'↔B' connector) 가 (24,24) offset 으로 paste, 새 UUID + endpoint remap
- 검증: paste 직후 selection 이 새 3 item 으로 교체, 새 connector 가 새 endpoint A'/B' 를 가리킴
- Item A 만 selection (B 와 connector 제외) → Cmd+V → A' 만 paste, connector 미생성

### C6. Inspector connector section
- M.size === 1 + type === 'connector' 시 connector section 노출
- 7 sub-control 각각 동작 (Q7 의 매트릭스)
- `from_id` / `to_id` 의 endpoint label 표시 (item.label 또는 첫 4 char of UUID)
- Reassign 버튼은 *P1 deferred* — `Coming soon` placeholder OK

### C7. ADR-0027 align/distribute — connector 제외
- selection 안에 panel + connector 동시 → align 6-button + distribute 2-button 표시되지만 *connector 는 target 에서 제외* (geometry 변경 없음)

### C8. Z-action — connector no-op
- selection 에 connector 만 있음 + zStore.bringToFront → no-op (또는 toast "Z-order not applicable to connectors"). 단 다른 item 도 같이 있으면 그 item 들만 적용.

### C9. Type-check
- `npm run check` — 0 errors / 0 warnings

### C10. Build
- `npm run build` — clean

---

## §D. Self-check 표 — 회귀 검증

| 영역 | 검증 시나리오 |
|---|---|
| 기존 4 z 액션 | connector 가 없는 layout 에서 panel 의 `[`/`]` 액션 정상 |
| 기존 Cmd+C/X/V | connector 가 없는 selection 의 clipboard 정상 |
| 기존 Cmd+D Duplicate | 동일 |
| 기존 Arrow nudge | connector 는 selection 에서 자연 제외 (locked check 와 동일) |
| 기존 Cmd+L Lock / Cmd+Shift+H Hide | connector 의 locked/visibility 토글도 정상 (단 visual 효과는 ADR-0036 의 group propagation 정합) |
| 기존 ContextMenu | connector 의 right-click 시 일반 entry (Copy/Cut/Paste/Hide/Lock/Delete) 동작, connector-specific 액션은 Inspector 만 |
| ADR-0028 Undo/Redo | connector add/delete/edit/cascade 모두 1 entry capture, Cmd+Z 복원 |
| ADR-0030 Multi-paste | endpoint+endpoint+connector 3개 paste 시 새 UUID 매핑 정확 |

---

## §E. Anti-pattern — *하지 말 것*

1. **`connector` 를 nodes 배열에 포함** — Q1 정합 위반. nodeAdapter 가 분리 필수.
2. **Connector 의 x/y/w/h 를 FE 가 직접 mutate** — BE 가 진실 source (BE handover Q4). FE 는 *empty placeholder* 로 commit, PUT 응답의 BBox 신뢰.
3. **Cascade delete 의 *역순 처리*** — connector 보다 endpoint 를 먼저 delete 하면 BE 가 orphan reject. Q5 정합 — connector 먼저.
4. **AnchorHandles 의 *항상 보임*** — visual noise. Q8 정합 — hover 300ms grace.
5. **Inspector connector section 을 M.size>1 시도 노출** — direction toggle / endpoints 의 mixed value 표현 부담. M.size===1 일 때만 (기존 ItemInfoView 패턴 정합).
6. **`onConnect` 의 *bidirectional 자동 생성*** — direction 은 default `"uni"`. 사용자가 Inspector 에서 변경.

---

## §F. Behavior change 정확성

본 batch 이후 FE 의 *canvas 표현* 변경:

- 모든 Node component 가 hover 시 5 anchor handle (visual disrupt 없는 미세 dot).
- Drag-out 시 임시 wire 표시.
- Connector 가 *edges layer* 에서 panel 아래로 그려짐.
- Item drag 시 dependent connector 가 *자동 follow* (SvelteFlow native).
- 일반 item 의 cascade delete 시 connector 도 함께 삭제 (one history entry).

기존 사용자 workflow 영향 0 — connector 는 *opt-in feature* (drag-out gesture). connector 를 안 만든 사용자는 별 차이 없음.

---

## §G. Verification 순서

1. **BE handover (0078) commit 후** — types 재생성 (`npm run gen-types` 또는 동등) → `ConnectorItem` type 가용
2. `npm run check` — 0 errors. Phase 1: B1 (types) → B2 (ConnectorEdge) → B10 (nodeAdapter) → B6 (createConnectorItem)
3. Phase 2: B3 (AnchorHandles) → B4 (Node mount) → B5 (Canvas onConnect wire)
4. Manual smoke — drag-out + connector commit + endpoint follow 동작 확인
5. Phase 3: B7 (clipboardOps) → B8 (applyDeletion) — cascade + clipboard 회귀
6. Phase 4: B9 (Inspector connector section)
7. `npm run check` + `npm run build` — clean

---

## §H. 의존성

- **Prerequisite**: BE handover (0078) 의 `Item::Connector` schema land + openapi 재발행 → FE typings 재생성. 본 prerequisite 없이는 FE-A 의 type 정의가 BE 와 drift.
- **Independent**: ADR-0036 의 P1 deferred (auto anchor / orthogonal routing / Toolbar2 도구 / Connector tool overlay) 는 본 batch 외. P1 별 ADR amend + 별 batch.

---

## 변경 이력
- 2026-05-19: 초안 land. ADR-0036 Accepted 직후 발주. 7 task (FE-A ~ FE-G) — 3 commit 묶음 권장 (renderer+gesture / cascade+clipboard / Inspector). BE handover 0078 의 prerequisite 후 시작.
