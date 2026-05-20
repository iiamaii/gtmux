# ADR-0036: Canvas component connector — directed / undirected / bidirectional

- 상태: **Accepted** (2026-05-19 신규)
- 관련 ADR: ADR-0018 (Canvas Item Data Model — connector variant 추가 amend), ADR-0024 (Z-index 분리), ADR-0027 (Multi-select Inspector + alignment), ADR-0028 (Undo/Redo), ADR-0030 (Clipboard), ADR-0032 (Multi-select context menu), ADR-0017 (Chrome — Toolbar2 / ContextMenu / Inspector)
- 관련 plan: plan-0007 §14 (Layout 편집 UX 고도화 batch)
- 근거 grilling: 2026-05-19 conversation §4 "component 간 연결 기능" — 사용자 명시 *"두 component 를 연결하는 선을 canvas 상에 표시. directional, indirectional, bidirectional 설정 가능 하도록"*
- 작성자: agent (system-architect role) — Phase D 의 결정 영역 12개 + 권장값 정합

## 맥락

ADR-0018 D4 의 type matrix 에 `terminal/text/note/rect/ellipse/line/free_draw/image/document/file_path/caption` 는 land, 그러나 **두 item 의 endpoint 가 *item id* 로 binding 되는 도메인** (= component connector) 은 미정의.

기존 `line` type 은 *geometric primitive* — endpoint 가 absolute coordinate, item 이동 시 별개. *connector* 는 *두 item 의 위치를 따라 다니는 wire* — endpoint 가 *item id + anchor point*. 두 도메인 분리 필요.

본 ADR 은 connector 의 **(a) data model**, **(b) 시각 표현 (3 direction mode)**, **(c) anchor 정책**, **(d) routing**, **(e) z-order**, **(f) cascade delete**, **(g) 생성 UX**, **(h) label**, **(i) multi-select 정합**, **(j) Undo/Redo 정합**, **(k) anchor display**, **(l) 단축키** 12 차원을 잠근다.

## 결정 (Decisions)

### D1. Data model — `items[]` 안 `type: "connector"` variant

ADR-0018 D1 의 discriminated union 자연 확장 — `Item::Connector` enum variant. 별도 `connectors[]` array 신설 안 함.

**Payload**:

```json
{
  "id": "uuid",
  "type": "connector",
  "parent_id": "g1" | null,
  "x": 0, "y": 0, "w": 0, "h": 0,
  "z": 12,
  "visibility": "visible",
  "locked": false,
  "label": "data flow",
  "minimized": false,
  "from_id": "uuid-of-item-A",
  "to_id":   "uuid-of-item-B",
  "from_anchor": "E",
  "to_anchor":   "W",
  "direction": "uni",
  "stroke": "#0d99ff",
  "stroke_width": 2,
  "stroke_dash": null,
  "head_from": "none",
  "head_to":   "arrow",
  "routing": "straight",
  "waypoints": null,
  "label_offset": null
}
```

- **`from_id` / `to_id`** — 다른 item 의 `id` (UUID). connector 도 item 이라 *자기 자신* 또는 *다른 connector* endpoint 는 무효 (D6 validate).
- **`from_anchor` / `to_anchor`** — D2 의 9-point keyword (`N/NE/E/SE/S/SW/W/NW/center`).
- **`direction`** — `"uni" | "bi" | "none"` (D4).
- **`head_from` / `head_to`** — `"arrow" | "circle" | "diamond" | "none"`. `direction` 의 default 매핑보다 우선 (per-end override).
- **`routing`** — `"straight" | "orthogonal" | "bezier"` (D3). MVP 는 `straight` 만 wire.
- **`waypoints`** — orthogonal/bezier 의 사용자 조정 중간점 (P1+, optional `[{x, y}]`).
- **`label_offset`** — label midpoint 기준 vector `{dx, dy}` (P1+, optional).
- **`stroke_dash`** — `null` (solid) | `"dash"` | `"dot"` (P1+).
- **`x / y / w / h`** — connector 는 *layout-bound 이 아니라 endpoint-bound* 이므로 본 4 field 는 **bounding box cache**. ItemCommon 정합을 위해 보존 (alignment / hit-test / SvelteFlow 의 bbox 계산용). BE 가 PUT 직전 자동 재계산 — 사용자가 직접 mutate 하지 않음.

근거: ADR-0018 의 single discriminated union 모델 정합. groups[] / 별 connectors[] 분리는 schema 복잡도 ↑ + frontend 의 SvelteFlow 정합 약화.

### D2. Anchor — Fixed 9-point (MVP), Auto routing P1

**MVP**: 8 cardinal/diagonal + center = `"N" | "NE" | "E" | "SE" | "S" | "SW" | "W" | "NW" | "center"`. 사용자가 명시 선택 (생성 UX D7 의 hover edge drag-out 시 시작 anchor 결정).

**P1 deferred** — `"auto"` anchor — endpoint 의 *가장 가까운 edge* 동적 계산 (Figma 패턴). MVP 의 fixed anchor 가 사용자 학습 부담 ↓.

좌표 계산:
- `N` = (item.x + w/2, item.y)
- `E` = (item.x + w, item.y + h/2)
- 등 BBox 의 8 edge midpoint + center
- `line` type 의 endpoint 1/2 도 같은 BBox 계산 (line 의 lineBoxFromEndpoints 정합)

### D3. Routing — Straight (MVP), Orthogonal/Bezier P1

**MVP**: `routing: "straight"` — 두 anchor 를 직선으로 연결. SVG `<line>` 또는 `<path d="M ... L ...">` 1 segment.

**P1 deferred**:
- `orthogonal` — 90° corner routing (Lucidchart 패턴). 알고리즘 = endpoint 의 anchor 방향 따라 corner ≥ 1 의 path 생성. `waypoints` 사용자 조정 가능.
- `bezier` — 2 control point 의 cubic bezier (Figma/Miro 패턴). control point = anchor 방향의 (offset, 0) tangent.

근거: MVP 의 *straight* 가 가장 직관적. orthogonal/bezier 는 *flow chart* 같은 특정 use case 에서 가치 — gtmux 의 *general layout edit* 영역에서 별 P1.

### D4. Direction 의 시각 표현 (3 mode)

| `direction` | `head_from` default | `head_to` default | 시각 |
|---|---|---|---|
| `"uni"` | `"none"` | `"arrow"` | A → B (단방향) |
| `"bi"` | `"arrow"` | `"arrow"` | A ↔ B (양방향) |
| `"none"` | `"none"` | `"none"` | A — B (방향 없음) |

**Per-end override**: 사용자가 Inspector 에서 `head_from` / `head_to` 명시 시 default 무시. e.g., `direction:"uni" + head_from:"circle"` = A ●→ B.

근거: 사용자 명시 *"directional, indirectional, bidirectional 설정 가능"* 정합. `none` 이 사용자의 "indirectional" 의 영문 표준 매칭 (= undirected / non-directional).

### D5. Z-order — Connector 는 connected items 보다 *아래*

Connector 의 `z` 는 일반 item 의 z 공간과 *분리된 sub-space* — *connected items 의 min(z) - 0.5* 로 동적 계산 (또는 별 z-layer rendering). 시각 효과 = panel 들 위에 wire 가 *겹치지 않음*.

**Implementation hint**:
- SvelteFlow 의 *edge layer* 사용 — node 들 위에 별도 SVG layer 가 edge 들을 그림. 자연스럽게 *node 아래에 connector wire* 가 표시 (SvelteFlow default).
- 단 *connector 도 item* 이라 layer-tree 정합 위해 z 도 유지. *z-action 액션 (Bring to front 등) 은 connector 끼리* 만 — 다른 type 의 z 와 분리 (`zStore` amend 필요).

근거: 연결선이 panel 의 *위* 에 그려지면 panel content (terminal output / note text) 를 가림. Lucidchart / draw.io 패턴.

**거부**: connector 도 일반 z 공간 공유 — wire 가 terminal output 위에 그려지는 시각 disruption. 거절.

### D6. Cascade delete — auto

`from_id` 또는 `to_id` 의 item 이 삭제될 때 그 connector 도 자동 삭제. 같은 `applyMutation` entry 안에서 처리 → ADR-0028 의 1 history entry 로 undo 시 자동 복원.

**구현**: `sessionStore.applyDeletion(ids)` 의 transform 안에서 cascade — `cur.items.filter` 시 *연결된 connector 도 함께 제외*.

**거부**:
- *dangling* 모드 (Figma 패턴) — endpoint marker visible, 사용자가 명시 delete — wire 가 *떠도는* visual chaos. MVP 거절. P1 검토.
- *cascade prompt* (확인 modal) — Cmd+Z 한 번에 복원 가능하므로 prompt 불필요.

### D7. 생성 UX

**MVP**:
- **(a) Hover edge drag-out** — item 의 9 anchor 가 hover 시 표시 (300ms grace). drag-out 시 *connector 도구 활성* — pointer move 따라 임시 wire 그림. drop target item 에 release 시 connector commit.
- 좌표 = source.anchor → target.anchor. release target 이 *빈 area* 면 connector 미생성 (사용자 cancel 의도).

**P1 deferred**:
- **(b) Toolbar2 "Connector" 도구** — toolbar 에서 도구 선택 후 source → target 두 클릭 (Lucidchart 패턴). 사용자가 도구 mode 의도 명시.
- **(c) ContextMenu `[Connect to…]`** — right-click 후 target picker overlay (Miro 패턴, P2).

### D8. Connector label

- `label` 필드 (ItemCommon) 사용. midpoint 에 inline text.
- `label_offset` 가 사용자 조정한 label 위치 — null 이면 wire 의 *geometric midpoint*.
- Font / color = 별 sub-decision — MVP 는 stroke color + 기본 font_size (12px). Inspector 에서 amend P1.

### D9. Multi-select / alignment 정합 (ADR-0027)

- ADR-0027 의 align/distribute = connector *제외 from target* — geometry 가 endpoint 종속이라 *위치 직접 조정* 의미 모호.
- selection BBox 계산은 connector 의 `x/y/w/h` cache 사용 (D1) — 시각 BBox 정확.

**Clipboard / Duplicate (ADR-0030)**:
- selection 안에 connector + 양 endpoint 모두 있음 → connector 도 함께 paste (endpoint 도 new UUID 로 mapping).
- selection 안에 connector 만 있고 endpoint 가 selection 밖 → **connector 제외 from paste** (Figma 패턴). 사용자가 두 endpoint 가 모두 source 안에 있을 때만 의도 명확.

### D10. 단축키 — 본 ADR scope 외

- 별도 connector-specific 단축키 없음 (P1+). 진입은 D7 의 hover drag-out 으로 충분.
- ADR-0017 D6 amend 의 *현존 매트릭스* (Delete / Undo / Copy / Cut / Paste / Nudge / Lock / Hide) 가 connector 에도 자동 적용 (item 의 일종이라).

### D11. Undo / Redo (ADR-0028 정합)

- Connector add/delete/edit 모두 `applyMutation` 통과 → historyStore 1 entry capture (별 처리 0).
- D6 의 cascade delete 도 같은 entry — undo 시 endpoint item + connector 둘 다 복원.

### D12. Anchor display 의 hover/focus 정책

- **Default**: 9 anchor 모두 hidden — item 의 시각 noise ↓.
- **Item hover 300ms+**: 4 edge midpoint (`N/E/S/W`) + center 만 표시 (5 point). drag-out 가능 affordance.
- **Connector tool 활성** (P1, D7-(b)): 모든 item 의 9 anchor 가 항상 표시. drag target 가시화.
- **Selected item + 다른 item 위 hover**: 다른 item 의 anchor 도 표시 — *어디에 attach 할지* 미리 보여주는 affordance (Miro 패턴).

## 비채택 대안

### R1. `connectors[]` 별도 top-level array
거부 — schema 의 union 모델 일관성 손실. layer tree / multi-select / clipboard / undo 등 모든 cross-cutting 로직이 *items + connectors 둘 다* 처리해야 — 코드 면적 ↑.

### R2. Auto closest-edge anchor 만 (fixed 없음)
거부 — 학습 부담 ↓ 의도이지만 anchor *명시* 가 불가능 → 사용자가 "여기서 시작해" 의도 표현 못함. MVP 의 fixed 9-point + P1 의 auto 추가가 더 유연.

### R3. Orthogonal routing 을 default
거부 — flow chart 시 자연 이지만 *general layout edit* 시 사용자가 manual 조정 부담. MVP straight 가 단순.

### R4. Connector 도 *일반 z space* 공유
거부 — wire 가 panel content 위에 그려지면 시각 disruption. 거절 — D5 의 sub-space 채택.

### R5. Dangling connector (cascade delete 없음)
거부 — *떠도는 wire* 의 visual chaos + 사용자 의도 추정 어려움. cascade delete 가 단순 + Cmd+Z 로 복원.

### R6. Connector 의 *별 store* (`connectorStore`)
거부 — sessionStore.items 의 SvelteMap 정합. 별 store = duplicated lifecycle / persistence 부담.

## 미해결 (Open)

- **O1.** P1 의 *auto anchor* 의 정확한 알고리즘 — closest edge vs straight-line intersection — 결정 필요.
- **O2.** *Self-loop connector* (from_id === to_id) 허용 여부 — flow chart 의 recursion 표현 가능. MVP 는 *validate reject* — P1 검토.
- **O3.** *Multi-edge between same pair* — A → B 가 이미 있는데 또 만들면? duplicate OK / merge dialog / dedupe — MVP 는 *허용* (duplicate). P1 의 UX.
- **O4.** *Connector label 의 typography* — Inspector 의 sub-section. P1.
- **O5.** *Routing avoid items* (orthogonal 의 obstacle avoidance) — Lucidchart 패턴, 복잡. P2.
- **O6.** *Connector 의 group 영향* — connector 가 group ∈ 일 때 group hide 시 connector 도 hide? endpoint 가 group ∉ 면? — D3 의 group propagation 정합 검증 필요. *MVP*: connector 의 parent_id 는 *기본 null*, 사용자가 명시 group 안에 넣으면 일반 item 의 group rule 적용 (ADR-0010 D6).

## 영향 (Consequences)

### Positive
- 두 item 의 *관계* 시각 표현 — gtmux 의 workflow 표현 가치 ↑ (terminal A → text note "build output" 같은 annotation)
- ADR-0018 의 union 모델 자연 확장 — schema 신규 영역 0
- ADR-0028 / ADR-0030 의 모든 정책 자동 적용

### Negative
- BE schema.rs 의 `Item::Connector` variant 추가 (P0)
- FE 의 SvelteFlow edge renderer 신규 + anchor overlay renderer 신규 (P0)
- ADR-0027 의 alignment / ADR-0030 의 clipboard 의 *connector-aware 정합* 필요 (D9)

### 후속
- 본 ADR Accepted 직후 **별 BE handover + FE handover** 로 작업 분담 (Phase D dispatch).
- P1 — auto anchor + orthogonal/bezier routing + Toolbar2 도구 + Inspector connector section.

## 변경 이력
- 2026-05-19: 신규 Accepted. ADR-0018 D1/D4/D8 amend 와 짝 (connector variant 등록). 사용자 권장값 (D1-(a) items 안 / D2-(a) 9-point / D3-(a) straight / D5 sub-space / D6 auto cascade / D7-(a) hover drag-out) 전부 채택. BE/FE handover 는 `docs/reports/0078-be-handover-connector.md` / `0079-fe-handover-connector.md`.
- 2026-05-20: **BE land** (0078 BE-A). `crates/http-api/src/schema.rs` 에 `Item::Connector` variant + `Anchor/Direction/Head/Routing/StrokeDash` 5 enum + `ValidationError::{ConnectorEndpointMissing, ConnectorInvalidEndpoint, ConnectorSelfLoop}` 3 신규 + `validate()` 의 connector arm (id_index O(1) lookup) + `recompute_connector_bboxes()` 2-pass helper + `put_layout_handler` 의 validate 직전 hook. 신규 unit test 6종 (valid endpoints / endpoint missing / self-loop / invalid endpoint / bbox recomputed / serde roundtrip). 447 → 453 PASS / 0 FAIL. FE side (0079) 의 typings 재생성 + renderer / creation gesture / Inspector 는 별 cycle.
