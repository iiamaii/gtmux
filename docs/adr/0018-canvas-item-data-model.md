# ADR-0018: Canvas Item Data Model — schema v2 (unified items[])

- 상태: Accepted (2026-05-15)
- 일자: 2026-05-15 (Proposed + Accepted, plan 0006 의 multi-session pivot grilling 결과)
- 결정자: agent (system-architect role) + user grilling
- 근거 grilling: 2026-05-15 plan 0006 grilling 의 Q1 / Q2 / Q6 / Q7
- 근거 plan: `docs/plans/0007-multi-session-pivot.md`
- 관련 ADR: ADR-0019 (Session+Workspace Model — 본 ADR 이 정의한 schema 가 session file 의 본체), ADR-0006 (Persistence storage — D15 amend), ADR-0010 (Group data model), ADR-0021 (Terminal Pool + Mirror — terminal item id 의 매칭 흐름)
- 관련 SSoT: `docs/ssot/canvas-layout-schema.md` (v1 → v2 갱신 필요, 본 ADR 가 schema 의 단일 진실)
- Amends: ADR-0006 D14 (`panels[]` strip 정책 폐기 → D15 hard cutover)

## 맥락

ADR-0006 D14 (2026-05-15 1차 amend) 는 schema v1 (`groups[] + panels[]`) 의 *PTY-direct 시대 정합* — boot 시 `panels[]` 는 stale 로 간주해 strip, `groups[]` 만 보존. 사용자의 좌표·라벨·그룹 작업이 server 재기동 시마다 손실되는 부작용.

multi-session pivot (ADR-0019) + plan 0006 의 Canvas Item 확장 (text/note/shape/image/document/file_path) 요구 + 사용자 명시 "**구동 환경에 의존적인 panel 들에 할당되어있던 terminal id 나 document path 등은 동일하게 연동할 수 있으면 자동 연동, 없으면 fresh spawn / 알람**" → schema 의 *큰 변경* 이 불가피.

본 ADR 은 schema v2 의 **(a) unified items[] 구조**, **(b) v1 → v2 migration 정책**, **(c) terminal item id 의 source**, **(d) Session attach 시 match-or-spawn 알고리즘** 4 차원을 잠근다.

## 결정 (Decisions)

### D1. Unified `items[]` (discriminated union)

Schema v2 의 *최상위 도메인 단위* 는 **Canvas Item**. terminal Panel 과 non-terminal Canvas Item 을 한 array `items[]` 에 담고 `type` discriminant 로 분기.

```json
{
  "schema_version": 2,
  "groups": [
    { "id": "g1", "parent_id": null, "label": "build cluster",
      "color": "#0d99ff", "visibility": "visible", "locked": false, "order": 0 }
  ],
  "items": [
    { "id": "7f3a-b9e2-…", "type": "terminal",
      "parent_id": "g1",
      "x": 200, "y": 150, "w": 640, "h": 400, "z": 0,
      "visibility": "visible", "locked": false,
      "label": "build watch", "description": "cargo watch on workspace",
      "minimized": false },
    { "id": "8a4c-c7f1-…", "type": "text",
      "parent_id": null,
      "x": 900, "y": 150, "w": 240, "h": 80, "z": 1,
      "visibility": "visible", "locked": false,
      "label": "Build instructions",
      "minimized": false,
      "text": "Use cargo nextest for parallel runs",
      "font_size": 14, "text_align": "center",
      "text_vertical_align": "middle", "color": "#333" }
  ],
  "viewport": { "x": 0, "y": 0, "zoom": 1.0 }
}
```

### D2. Terminal item.id = backend Terminal.id (직접 동일성)

Schema 의 `type:"terminal"` 인 item 의 `id` 필드 = backend Terminal 의 id. 즉 같은 값을 둘이 동시에 사용 — workspace 가 *backend Terminal id 의 영속 저장소* 역할.

- ID 형식: Auto UUID (server 가 spawn 시 부여). 사용자 입력 부담 0.
- 사용자 친화 라벨은 별도 `label` 필드 (자유, 중복 OK, 빈 OK).
- Session attach 시 `id` 가 server-pool 의 alive Terminal id 와 매칭되면 reconnect, 없으면 같은 id 로 fresh spawn (D4 알고리즘).

이유: 단일 식별자가 *workspace 영속 데이터* 와 *backend ephemeral data* 의 join key 역할을 함. 별도 *terminal_id ↔ pane_id* 매핑 테이블 불필요 — schema 단순화.

대안:
- 사용자 부여 stable name (`build-watch`) — 거부. 새 terminal 마다 이름 입력 부담, 중복 충돌 방지 부담.
- UUID + 별도 stable name 둘 다 — 거부 (P1+). MVP 는 UUID + label 만으로 충분.

### D3. Common field 매트릭스

| Field | 적용 type | 의미 |
|---|---|---|
| `id` | all | UUID. terminal 은 backend Terminal id 와 동일 (D2). |
| `type` | all | discriminant. `terminal/text/note/rect/ellipse/line/free_draw/image/document/file_path/caption/connector` (D10 amend 2026-05-16, **D12 amend 2026-05-19** — connector 추가, ADR-0036 정본). |
| `parent_id` | all | Group id (자식) 또는 null (Canvas 루트). |
| `x, y, w, h` | all | 좌표/크기. SvelteFlow coordinate. |
| `z` | all | z-index. 신규 item z = max(z) + 1. **Tree order 와 무관 mutate** (ADR-0024) — drag reorder 는 organization 만 바꿈, z 는 ADR-0024 D2 의 4 액션 (Bring/Send) 으로만. Group 은 z field 없음 — 모든 items 가 flat global z 공간 공유. |
| `visibility` | all | "visible" \| "hidden". |
| `locked` | all | boolean. |
| `label` | all | 사용자 자유 라벨, optional. |
| `description` | all | 사용자 자유 메모, optional, multiline. |
| `minimized` | all | boolean. header bar 만 표시. **영속** (schema field). |
| `restored_geom` | all (effective when `minimized=true`) | optional `{ x, y, w, h }`. minimize 직전 의 geometry backup. **D11 amend (2026-05-17, draft)** — page reload 후에도 옛 size 복원 가능. |

> ⚠️ **2026-05-15 G20 grilling amend**: 옛 `maximized` schema field 는 **제거**됨 — FE-only ephemeral state 로 강등 (다음 attach 시 자동 unmaximize). Maximize 의 fill 영역 = *Canvas viewport area* (Titlebar / Toolbar / Status bar 유지). 한 시점에 1 panel 만 maximize 가능 (toggle 시 다른 max 자동 해제). Unmaximize trigger = Esc / 헤더 toggle 버튼 / panel header double-click. Esc 우선순위는 *modal stack top 우선* (Settings overlay / dialog 가 위면 그것이 Esc 흡수). 자세한 BE/FE 명세는 plan-0007 §14.6.

### D4. Type-specific payload

| Type | Payload 필드 |
|---|---|
| `terminal` | **G35 amend (2026-05-15)**: optional `terminal_overrides?: { font_size?: number, wrap?: bool, scrollback?: number, cursor_style?: "block"\|"underline"\|"bar", cursor_blink?: bool, bell?: "none"\|"sound"\|"visual" }` — 모든 필드 optional (없으면 Settings 의 global default 사용). |
| `text` | `text: string`, `font_size: number` (**D4 amend ② — 2026-05-20 batch 5**: 1≤≤96, validation `TextFontSizeOutOfRange`), `text_align?: "left" \| "center" \| "right"` (**G39 amend, 누락 시 `"center"`), `text_vertical_align?: "top" \| "middle" \| "bottom"` (**G40 amend, 누락 시 `"middle"`), `color: string`, **D4 amend ② (2026-05-20 batch 5)** 신규: `font_weight?: "light" \| "normal" \| "bold"` (default `"normal"`, register 의 100~900 numeric 은 P1 로 미루고 3-bucket 으로 결정 — Grill #6), `italic?: bool` (default `false`), `underline?: bool` (default `false`), `strikethrough?: bool` (default `false`). 4 boolean 결정 — Grill #15 (register 의 단일 `text_decoration` enum 폐기, underline+strikethrough 동시 표현 위해 boolean array 가 아닌 4 boolean). 모든 신규 필드 `#[serde(default)]` — 옛 record 자연 round-trip. |
| `note` | `title: string`, `body: string`, `color: string` |
| `rect` | `stroke: string`, `fill: string`, `stroke_width: number` (**D4 amend ① — 2026-05-20 batch 5**: 1≤≤32 — Inspector slider 와 정합, validation `StrokeWidthOutOfRange`). **D4 amend ① (2026-05-20 batch 5)** 신규: `fill_enabled?: bool` (default `true`; legacy `fill: "transparent"` 의 알파 0 의미는 그대로 보존, 본 boolean 은 hit-test 도 제외하는 "off" — Grill #3), `stroke_enabled?: bool` (default `true`; border 시각 + hit-target band 모두 제거), `corner_rounded?: bool` (default `false`; 자동 radius `clamp(min(w,h)*0.15, 4, 16)` 는 FE 계산 — Grill #5, 수치 input 폐기), `stroke_dash?: "solid" \| "dash" \| "dot" \| "dash_dot"` (default `null`=solid). 모든 필드 옵셔널 + `#[serde(default)]`. |
| `ellipse` | `stroke: string`, `fill: string`, `stroke_width: number` (1≤≤32, **D4 amend ①**). **D4 amend ① 신규**: `fill_enabled?: bool` (default `true`), `stroke_enabled?: bool` (default `true`), `stroke_dash?: "solid" \| "dash" \| "dot" \| "dash_dot"` (default `null`=solid). `corner_rounded` 는 *rect 전용* 이라 ellipse 에는 없음. |
| `line` | `stroke: string`, `stroke_width: number` (1≤≤32, **D4 amend ①**), `x2: number`, `y2: number`. **D4 amend ① 신규**: `stroke_dash?: "solid" \| "dash" \| "dot" \| "dash_dot"` (default `null`=solid). `fill_enabled` / `stroke_enabled` 는 line 에 의미 없음 (fill 영역 무, stroke 가 line 의 본질) — 추가 안 함. |
| `free_draw` | `stroke: string`, `stroke_width: number`, `points: [{x, y}]` (point cap 적용, P2+) |
| `image` | `asset_id: string` (sha256 hash), `mime: string`, optional `original_w/h` |
| `document` | **D10 amend (2026-05-16)**: 두 mode 지원. (a) asset-based — `asset_id: string`, `mime: string`, `file_name: string`, `size_bytes: number`. (b) inline-stored — `content: string` (UTF-8 markdown, cap 64 KB), `file_name: string` (display 용 — 시안의 doc-head). 두 mode 는 *상호 배타*: `asset_id` 가 있으면 (a), 없으면 (b). 본 amend 는 시안 (`ref/frontend-design/components.html §02`) 의 inline-editable document 를 cover. asset_id 는 *optional* 로 amend. **BE schema 정합 ship (amend ② — 2026-05-17, 0056 work package)**: `crates/http-api/src/schema.rs` 의 `Item::Document` 가 `asset_id: Option<String>` + `content: Option<String>` 신규. `DOCUMENT_INLINE_MAX_BYTES` 상수 (64 KB) + 3 신규 ValidationError variant (`DocumentMissingSource` / `DocumentBothSources` / `DocumentInlineTooLong`). validate() 의 match arm 이 (None,None) / (Some,Some) / (None,Some > cap) 분기 거절. asset_id 의 sha256 형식 regex 는 `/api/assets/*` ship (Stage 2, ADR-0030 to-be) 시 추가. |
| `file_path` | `path: string` (UTF-8 string), `kind: "directory" \| "file"` (optional cache, P2+). **OS-level open 은 ADR-0023 정책**: double-click → confirm modal → ext+prefix allowlist → backend `xdg-open`/`open` argv spawn. |
| `caption` | **D10 amend (2026-05-16, 신규)**: `head: string` (mono uppercase label — pattern `Fig. NN · Topic`), `body: string` (multi-line note text, cap 4 KB), optional `meta?: string` (trailing meta — e.g. author/time). 시안 (`ref/frontend-design/components.html §01`) 의 *pinned annotation block* — accent rail + head + body 구조. |
| `connector` | **D12 amend (2026-05-19, 신규 — ADR-0036 정본)**: `from_id: UUID`, `to_id: UUID` (다른 item 의 id), `from_anchor: "N"\|"NE"\|"E"\|"SE"\|"S"\|"SW"\|"W"\|"NW"\|"center"`, `to_anchor`: 동일, `direction: "uni"\|"bi"\|"none"`, `head_from: "arrow"\|"circle"\|"diamond"\|"none"`, `head_to`: 동일, `routing: "straight"\|"orthogonal"\|"bezier"` (MVP 는 straight 만 wire), `stroke: string`, `stroke_width: number`, optional `stroke_dash?: "dash"\|"dot"\|null`, optional `waypoints?: [{x,y}]` (P1), optional `label_offset?: {dx,dy}` (P1). `x/y/w/h` 는 *endpoint-bound BBox cache* — BE 가 PUT 직전 자동 재계산. |

비고: `image`/`document` 의 asset storage 정책은 **ADR-0033 (Draft, 2026-05-17)** 가 정본 — `/api/assets/*` binary endpoint + sha256 hash + workspace `.assets/` storage + Settings-driven MIME/cap + boot-lazy GC. 본 deferred 영역의 후속 결정 완료. `file_path` 의 fp-foot meta (lines / size / branch) wire 는 **ADR-0034 (Draft, 2026-05-17)** 의 `GET /api/file-stat` endpoint.

### D5. v1 → v2 hard cutover migration

Boot 시 layout file 의 `schema_version` 검사:

| 케이스 | 동작 |
|---|---|
| `schema_version: 2` | 그대로 사용 |
| `schema_version: 1` | hard cutover: groups[] 보존, panels[] 통째 폐기 (어차피 ADR-0006 D14 가 이미 strip 정책), items[] = [], schema_version = 2 로 atomic write. info log `layout: migrated v1→v2` |
| `schema_version` missing or unknown | corrupt 분류, sidecar quarantine (ADR-0006 §7-state table) |

이로써 ADR-0006 D14 의 strip 정책은 **무의미해진다** — v2 에는 panels[] 가 없고 items[] 는 매 attach 마다 match-or-spawn 으로 재구성 가능. ADR-0006 의 amend 마커로 *D15: schema v2 hard cutover, D14 obsolete* 추가.

대안:
- Co-existence (v1/v2 양 reader) — 거부. 코드 분기 영구화.
- Side-by-side file (legacy + new 양 file) — 거부. file 관리 부담.

### D6. Session attach 시 match-or-spawn 알고리즘

새 webpage 가 session X 에 attach 할 때 (D3 의 single-attach 정합):

```
load session X file → items[]
                       ↓
For each item.type == "terminal":
  if item.id ∈ server-pool alive Terminal ids:
    bind panel ↔ existing terminal  (= reconnect)
  else:
    spawn new Terminal with id = item.id  (= fresh spawn)
                       ↓
Non-terminal items: 그대로 render

또한 *server-pool 에 있는 alive Terminal 중 items[] 에 없는 것* 들 (다른 session 의 attach 일 수도 있음, ADR-0021 D2 의 multi-session terminal 정합):
  이 session 의 Canvas 에는 표시 안 함 (다른 session 의 작업).
```

이는 사용자가 명시한 정합:

| 매칭 case | 동작 |
|---|---|
| item.id ↔ server-pool alive Terminal: 매칭 | keep + reconnect |
| item.id, server-pool 에 없음 | 새 Terminal spawn with same id |
| server-pool 에 alive Terminal 있으나 다른 session 의 layout 의 일부 | 이 session 은 touch 안 함 — 다른 session 의 attached webpage 가 자기 view |

#### Unmatched warning dialog

사용자 명시 *"matching 되지 않는 panel 이 있으면, 그대로 진행하시겠냐고 dialog 표시"*. 새 모델에서 두 경우:

1. **current canvas ✓ / session record ✗** — 이 시나리오는 *session 간 switch* 일 때만 발생. attach 흐름은 *항상 fresh attach (= current canvas 가 빈 상태)* 이므로 발생 X. 다만 *같은 session 재 attach* (current = session X 이미 attached) 인데 disk 의 session X 가 외부에서 수정된 경우는 P2+ (ADR-0018 후속).
2. **current ✗ / session record ✓** — 첫 attach 시 모든 terminal items 가 spawn 분기. 새 terminal 수 가 1 이상이면 "Attach session 'X'? (will spawn N new terminals)" confirm modal. 사용자 [Confirm] → spawn 진행. [Cancel] → attach 취소, dialog 로 회귀.

#### Concurrency

- Match-or-spawn 은 backend 가 spawn lock + ETag 로 atomic 처리 (ADR-0006 §7-state table 정합).
- 동일 session 의 두 webpage 가 동시 attach 시도 시 ADR-0019 D3 single-attach 정책으로 한 webpage 만 성공.

### D7. Z-index 정책

- 신규 item z = max(현재 items 의 z) + 1.
- Manipulation Selection (M) 들어오면 z = max + 1 (자동 최상위, ADR-0010 의 기존 Z 정책 그대로).
- Explicit [Bring to front / Send to back / Up / Down] 액션은 P1+.

### D8. Schema validation (backend Rust)

```rust
#[derive(Deserialize, Serialize)]
struct Layout {
    schema_version: u32,
    groups: Vec<Group>,
    items: Vec<Item>,
    viewport: Viewport,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Item {
    Terminal { #[serde(flatten)] common: ItemCommon },
    Text     { #[serde(flatten)] common: ItemCommon, text: String, font_size: u32, text_align: TextAlign, text_vertical_align: TextVerticalAlign, color: String },
    Note     { #[serde(flatten)] common: ItemCommon, title: String, body: String, color: String },
    Rect     { #[serde(flatten)] common: ItemCommon, stroke: String, fill: String, stroke_width: u32 },
    Ellipse  { #[serde(flatten)] common: ItemCommon, stroke: String, fill: String, stroke_width: u32 },
    Line     { #[serde(flatten)] common: ItemCommon, stroke: String, stroke_width: u32, x2: f64, y2: f64 },
    FreeDraw { #[serde(flatten)] common: ItemCommon, stroke: String, stroke_width: u32, points: Vec<Point> },
    Image    { #[serde(flatten)] common: ItemCommon, asset_id: String, mime: String, original_w: Option<u32>, original_h: Option<u32> },
    Document { #[serde(flatten)] common: ItemCommon, asset_id: String, mime: String, file_name: String, size_bytes: u64 },
    FilePath { #[serde(flatten)] common: ItemCommon, path: String, kind: Option<String> },
    // D12 amend (2026-05-19, ADR-0036 정본) — component connector.
    Connector {
        #[serde(flatten)] common: ItemCommon,
        from_id: String,       // UUID of source item
        to_id:   String,       // UUID of target item
        from_anchor: Anchor,   // N/NE/E/SE/S/SW/W/NW/center
        to_anchor:   Anchor,
        direction: Direction,  // uni / bi / none
        stroke: String,
        stroke_width: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stroke_dash: Option<StrokeDash>,
        head_from: Head,       // arrow / circle / diamond / none
        head_to:   Head,
        routing: Routing,      // straight / orthogonal / bezier (MVP: straight 만 wire)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        waypoints: Option<Vec<Point>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label_offset: Option<Point>,
    },
}
```

Validation:
- `id` UUID format
- `parent_id` 는 groups[].id 중 하나 또는 null (refer 무결성)
- terminal item 만 backend Terminal 와 join
- string field cap: label/description 4 KB, text 64 KB, free_draw points 5000 (P2+ point simplification)
- 전체 file size cap: 16 MB (P0)
- **Connector** (D12 amend, ADR-0036 D6) — `from_id` / `to_id` 가 items[] 안 다른 item.id 가리켜야 (refer 무결성). 둘 다 *connector type 이 아닌* item 가리켜야 (self / connector chain 금지 — `ConnectorInvalidEndpoint`). `from_id !== to_id` (self-loop reject, MVP — O2). PUT 직전 BE 가 `x/y/w/h` 를 endpoint anchor 의 BBox 로 재계산 (사용자가 보낸 값 무시).

### D9. Canvas Item 편집 시각 정책 (G39 amend)

Canvas Item 의 선택/resize 시각은 **SvelteFlow wrapper bbox 와 item renderer 자체를 분리하지 않는다**. 모든 node renderer 는 SvelteFlow 가 resize 중 갱신하는 wrapper 크기를 실제 렌더 root 가 `width:100%; height:100%` 로 따라가야 한다. 따라서 사용자는 rect/ellipse/panel/text/note/file_path resize 중에 선택 박스만 커지는 것이 아니라 실제 객체가 즉시 변형되는 것을 본다.

선택 표시의 정본은 **NodeResizer handle/line 1벌**이다. renderer 내부의 별도 accent outline 이나 SvelteFlow 기본 selected border/box-shadow 는 제거한다. 이는 Figma/Excalidraw 처럼 객체 자체의 stroke/fill 과 선택 affordance 를 구분하기 위함이다.

**2026-05-23 보강 — bbox scaler 와 live paint geometry.**

- NodeResizer 의 실제 handle DOM geometry 는 resize 계산 표면이므로 viewport zoom 보정을 직접 적용하지 않는다. scaler 의 보이는 크기는 `pointer-events:none` visual layer 로만 보정하고, 실제 handle 은 xyflow stable geometry 를 유지한다.
- Canvas capture handler 는 `.svelte-flow__resize-control` / `.nodrag` / line `.endpoint` 에서 시작한 pointer sequence 를 selection/lasso/group-drag 로 해석하지 않는다. control surface 는 library-owned gesture 또는 component-owned endpoint gesture 로 격리한다.
- rect/ellipse renderer 는 persisted `data.w/h` 만으로 paint geometry 를 만들지 않는다. SvelteFlow 가 전달하는 live `width/height` 가 있으면 이를 우선 사용해 SVG `viewBox`, visible shape, transparent hit-target, corner radius 를 계산한다. 그래야 drag-resize 중 filled shape 와 wrapper bbox 가 같은 frame 에서 동기화된다.
- padding/border 가 있는 visual node 는 NodeResizer 기준 좌표계를 왜곡하지 않도록, 필요 시 `*-shell` wrapper 를 두고 Resizer 를 shell 기준으로 배치한다. Note 는 이 패턴의 reference implementation 이다.

Text item 의 inline edit 은 새 카드/입력 박스를 띄우지 않는다. 더블클릭 시 동일한 text node content box 안에서 chrome-less textarea 로 전환하며, padding/font-size/line-height/text-align/vertical-align 을 표시 상태와 맞춘다. 기본 horizontal alignment 는 `"center"`, 기본 vertical alignment 는 `"middle"` 이다. 사용자는 horizontal `"left" | "center" | "right"`, vertical `"top" | "middle" | "bottom"` 으로 변경할 수 있다. 정렬 상태는 `text_align`, `text_vertical_align` payload 로 영속화하며, 기존 layout 에 필드가 없으면 각각 `"center"`, `"middle"` 로 해석한다.

### D11. Minimize 의 옛 geometry 영속화 — `restored_geom` (amend, 2026-05-17, draft)

#### 배경

`PanelNode.onMinimizeClick` (PanelNode.svelte:294-325) + `NoteNode.onMinimizeClick` (NoteNode.svelte:137 동일 패턴) 는 schema item 의 geometry (x, y, w, h) 를 변경 + `sessionStore.restoredItemGeoms` (in-memory `SvelteMap`, `sessionStore.svelte.ts:123`) 에 옛 값을 backup. restore 시 backup 에서 복원, 미존재 시 default `h = 220` fallback.

backup map 이 reactive state 라 **page reload (또는 session 전환) 시 소실**. 사용자가 minimize 한 상태로 새로고침하면 옛 size 정보 없어 default 로 복원 — 사용자가 직접 set 했던 height 손실. 직전 batch handover (`docs/reports/2026-05-17-session-handover-component-design-batch.md` §5.3, `...maximize-modal-and-ui-batch.md` §5.3) 가 이 trade-off 를 인지 + 본 amend 를 follow-up 으로 명시.

#### 결정

`ItemCommon` 에 optional field `restored_geom?: { x, y, w, h }` 추가 — minimize 직전 의 geometry 를 schema-level 로 영속화.

```rust
// BE schema.rs::ItemCommon (추가 field)
#[serde(default, skip_serializing_if = "Option::is_none")]
pub restored_geom: Option<RestoredGeom>,

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RestoredGeom { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }
```

```ts
// FE canvas.ts::ItemCommon (추가 field)
restored_geom?: { x: number; y: number; w: number; h: number };
```

#### Invariant

- `restored_geom !== undefined ⟺ minimized === true` 이 정합 상태. 둘 다 false 인데 `restored_geom` 만 set 인 layout 은 stale (`schema::validate` 의 warning 대상 — strict error 아님 — restore 시 자동 정리).
- `minimized: false → true` 전이 시 직전 `(x, y, w, h)` 가 `restored_geom` 에 저장, geometry 는 collapse 값 (terminal: `h = MIN_HEADER_H = 32` / note: `w = h = 32` chip) 으로 set.
- `minimized: true → false` 전이 시 `restored_geom` 의 값을 `(x, y, w, h)` 에 복원 후 `restored_geom = undefined` 로 unset.

#### Maximize 는 본 amend 대상 아님

G20 amend (2026-05-15) 로 maximize 는 schema field 가 아니라 **FE-only ephemeral state** (`sessionStore.maximizedItemId`). reload 시 자동 unmaximize 라 disk persistence 불필요. **maximize-side 의 in-memory backup** (`sessionStore.restoredItemGeoms` 의 maximize 항목) 은 본 amend 후에도 그대로 유지 — 별 store (e.g. `maximizeBackupMap`) 로 분리하거나 단일 map 의 contract 를 doc 화하는 건 implementation step 의 결정 사항.

#### Cap / Validation

- 각 number 는 layout 의 다른 geometry 와 동일 (`f64`, 음수 허용). 별도 cap 도입 안 함 — `RestoredGeom` 의 4 number 가 layout 의 16 MB cap 안 충분.
- Validation (D8): `restored_geom.is_some() && !minimized` 는 warn — strict reject 안 함 (legacy import / round-trip 시 자연 정리).

#### Migration

- field 가 optional + `#[serde(default)]` 라 **backward-compatible**. 옛 layout 의 record 들은 자동 `restored_geom = None` 으로 deserialize. v2 schema_version 자체 변경 없음 (ADR-0018 D5 의 hard cutover 대상 아님 — additive field).
- 옛 minimize-된 record 들이 disk 에 있으면 `restored_geom = None` + `minimized = true` 상태 — restore 시 옛 default fallback (`h = 220`) 적용. 사용자가 한 번 unminimize → 다시 minimize 하면 정합.

#### ETag / History impact

- `restored_geom` 변경은 layout structure 변경 — ADR-0006 D5 의 일반 ETag rebase 흐름. minimize 의 atomic write 안에 함께 commit.
- History (ADR-0028) 의 PRE snapshot 에 `restored_geom` 도 포함 → undo / redo 시 자동 정합 (별 처리 불필요, `applyMutation` 의 layout-level snapshot 이 cover).

#### Status

**Draft** — 본 amend 는 doc-only. Accepted 전 grilling/review + 별 plan (e.g. plan-00XX) 으로 구현 step 분리:

1. BE schema.rs::ItemCommon + RestoredGeom + serde round-trip test (Gate 0018-D11-1 ~ -3).
2. FE canvas.ts::ItemCommon field 추가.
3. PanelNode/NoteNode 의 onMinimizeClick 변경 — `backupItemGeom` 대신 `applyMutation` 안에서 `it.restored_geom` 함께 set + minimize=false 시 unset.
4. sessionStore.restoredItemGeoms 의 contract 명확화 — maximize-only backup 으로 격하 (또는 `maximizeBackupMap` 으로 rename).
5. E2E: minimize → reload → restore 시 옛 size 복원 검증.

## 어휘 매트릭스 (CONTEXT.md 정합)

- **Canvas Item** = 본 ADR 의 1차 entity, items[] 의 element
- **Panel** = `type:"terminal"` 인 Canvas Item (어휘 호환을 위해)
- **Terminal** = backend execution unit (ADR-0021), schema 안 reference 만

## 대안 검토

### A1. Separated `panels[] + canvas_items[]`
**거부.** layer tree / multi-select / drag-reparent 의 cross-cutting 부담. SvelteFlow nodes 가 한 array 이므로 frontend 정합도 약함.

### A2. Unified items[] + 별도 runtime_panels[] (runtime/persisted 분리)
**거부.** 두 source join 의 복잡성. 본 ADR 의 D6 match-or-spawn 이 동일 효과 (runtime 정보는 server memory 의 alive Terminal pool 에서 read).

### A3. Terminal id 분리 (separate terminal_id + pane_id 매핑 테이블)
**거부.** schema 단순화의 가치가 큼. session attach 의 join key 가 *직접* 일치.

### A4. v1/v2 dual reader 영구 유지
**거부.** 코드 분기 영구화 비용 > migration 이점 (어차피 ADR-0006 D14 가 데이터 손실 인정).

### A5. 사용자 부여 stable name 매칭 (UUID 대신 'build-watch')
**거부.** 새 terminal 마다 이름 입력 부담 + 중복 충돌. UUID 가 매칭 안전성에서 우월.

## 영향

### Code
- **Backend**:
  - `LayoutStore` 의 schema 정의 (v1 enum + v2 enum, Item discriminated)
  - v1 → v2 migration 코드 (boot 시 1회)
  - Schema validation (serde + 별 cap 검사)
  - Match-or-spawn 알고리즘 (session attach 흐름)
  - HTTP API: `GET /api/sessions/<name>/layout`, `PUT ... /layout` (v2 schema, ETag)
- **Frontend**:
  - `CanvasItem` discriminated union 도입 (TS)
  - 각 type 의 Node renderer (TextNode / NoteNode / ShapeNode / LineNode / FreeDrawNode / ImageNode / DocumentNode / FilePathNode)
  - Match-or-spawn confirm dialog UI (D6)
  - SvelteFlow nodes 의 unified array 매핑

### ADR
- ADR-0006 D15 amend (schema v2 hard cutover, D14 obsolete 명시)
- ADR-0010 (Group) 그대로 — groups[] 구조 변경 없음

### Docs
- `docs/ssot/canvas-layout-schema.md` v2 갱신 (본 ADR 이 schema source-of-truth, SSoT 는 reference)
- plan-0007 의 Stage 4 (schema v2) 가 본 ADR 의 코드 진행

### 보안
- File size cap (16 MB)
- String field cap (label/description, free_draw points)
- Path traversal: file_path item 의 path 는 *string metadata 만*. 단 **ADR-0023 (G21 grilling) 에 의해 OS-level open 을 explicit opt-in** — double-click → confirm modal → ext+prefix allowlist → backend argv spawn (no shell, canonicalize, NUL block). ADR-0019 D7 D11 와 정합.
- Asset MIME sniffing (P2+, image/document 의 별 ADR)

## 변경 이력

- 2026-05-15: 초안 + Accepted. plan 0006 grilling 의 Q1/Q2/Q6/Q7 합본. ADR-0006 D14 폐기 + D15 신규 amend.
- 2026-05-15 (G20 grilling amend): D3 의 `maximized` schema field 제거 (FE-only ephemeral 로 강등). minimize 만 영속. 본 ADR 의 D1 예시 JSON 도 정합.
- 2026-05-15 (G24 grilling amend, by ADR-0024): D3 의 `z` field 의 mutation 규칙 정리 — Tree drag reorder 는 z 영향 X, z 는 4 액션 (Bring/Send) 으로만. Group 은 z 없음. ADR-0024 reference.
- 2026-05-15 (G35 grilling amend): D4 의 `terminal` payload 에 optional `terminal_overrides?: {...}` 추가 (font_size / wrap / scrollback / cursor_style / cursor_blink / bell). 모든 field optional — global default fallback. Settings 의 Terminal section 신규.
- 2026-05-16 (G39 amend): D4 의 `text` payload 에 optional `text_align` 추가. D9 로 Canvas Item resize/selection/inline text edit 시각 정책을 명시.
- 2026-05-16 (G40 amend): D4 의 `text` payload 에 optional `text_vertical_align` 추가. Text placeholder/input/final text 의 content box 위치 기준을 동일화.
- 2026-05-16 (D10 amend — components batch): D3 type discriminant 에 `caption` 추가. D4 payload 에 (a) `caption` 신규 (head/body/meta) 와 (b) `document` 의 inline-stored mode 추가 (content/file_name, asset_id optional). 정본 시안 = `ref/frontend-design/components.html §01 / §02`. 구현 plan = plan-0011.
- 2026-05-17 (**D11 amend, draft**): `ItemCommon` 에 optional `restored_geom?: { x, y, w, h }` 필드 추가 — minimize 직전 의 geometry 를 schema-level 영속화. 배경: 기존 `sessionStore.restoredItemGeoms` (in-memory `SvelteMap`) 가 page reload 시 소실 → 사용자가 set 했던 옛 size 손실. 본 amend 후: minimize 의 atomic write 안에 `restored_geom` 함께 set, restore 시 복원 + unset. invariant = `restored_geom.is_some() ⟺ minimized=true`. maximize 는 G20 amend 후 ephemeral 이라 본 amend 대상 아님 (FE-only backup map 으로 유지). Migration = additive optional field 라 backward-compatible (옛 record 는 `restored_geom = None`). 정본 trade-off 출처 = 직전 batch handover (`docs/reports/2026-05-17-session-handover-component-design-batch.md` §5.3, `...maximize-modal-and-ui-batch.md` §5.3). Status = **Draft** — Accepted 전 grilling/review + 별 plan (BE schema + FE handler + E2E) 으로 구현 step 분리.
- 2026-05-19 (**D12 amend — connector variant 추가, ADR-0036 정본**): D3 type discriminant 에 `connector` 추가. D4 payload 에 `connector` row 신규 (from_id/to_id/from_anchor/to_anchor/direction/head_from/head_to/routing/stroke/stroke_width + optional stroke_dash/waypoints/label_offset, x/y/w/h 는 BBox cache). D8 의 `Item::Connector` Rust variant + Validation 의 connector refer-무결성 + self-loop reject 명시. 본 amend 는 **register only** — BE schema.rs + openapi + FE renderer + creation gesture 는 별 batch (Phase D BE handover `docs/reports/0078-be-handover-connector.md` + FE handover `0079-fe-handover-connector.md` 로 분담). Migration = additive variant 라 backward-compatible (옛 record 에 connector 없음 = 자연).
- 2026-05-17 (**D10 amend ② — Document BE schema 정합 ship**): D10 amend ① (2026-05-16) 의 *inline-stored mode* 가 schema 와 drift 였음 (`asset_id: String` required, `content` field 부재) — 본 amend 가 해소. `crates/http-api/src/schema.rs` 의 `Item::Document` 를 `asset_id: Option<String>` + `content: Option<String>` 으로 수정 + `DOCUMENT_INLINE_MAX_BYTES = 64 * 1024` 상수 도입. `ValidationError` 에 3 신규 variant (`DocumentMissingSource` / `DocumentBothSources` / `DocumentInlineTooLong`) + `code()` mapping. `validate()` 의 Document match arm 이 (asset_id, content) tuple 의 4 case 중 (None,None) / (Some,Some) 거절 + (None, Some>cap) 거절 + 나머지 OK. backward-compatible (Option + skip_serializing_if 가 옛 layout 의 `"asset_id": "..."` 자연 흡수). 5 신규 schema unit test (`document_inline_stored_validates` / `document_asset_based_validates` / `document_missing_source_rejected` / `document_both_sources_rejected` / `document_inline_cap_enforced`) 추가. work package = `docs/reports/0056-be-document-inline-mode-and-assets.md`. 본 amend 는 1-stage of 2 — `/api/assets/*` binary endpoint (asset-based mode 의 실제 binary 전송) 는 별 ADR (0030 to-be) 의 영역. 검증: workspace 376 → **381 PASS / 0 FAIL** (+5 신규 document validation tests). release build PASS. asset_id 의 sha256 형식 regex 검증은 Stage 2 ship 시 추가.
- 2026-05-20 (**D4 amend ① — Figure (rect/ellipse/line) fill·stroke·corner·dash, batch-5 R1+R2 / Grill #3+#5+#14**): D4 의 `rect` / `ellipse` / `line` payload 표 row 갱신. `Item::Rect` 에 `fill_enabled: bool` (default true) + `stroke_enabled: bool` (default true) + `corner_rounded: bool` (default false) + `stroke_dash: Option<FigureStrokeDash>` 신규. `Item::Ellipse` 에 `fill_enabled` + `stroke_enabled` + `stroke_dash`. `Item::Line` 에 `stroke_dash`. 신규 enum `FigureStrokeDash: Solid | Dash | Dot | DashDot` (snake_case wire — connector 의 `StrokeDash` 와 의미·default 가 달라 별 enum). 신규 `ValidationError::StrokeWidthOutOfRange { width }` — Rect/Ellipse/Line 의 `stroke_width` 1..=32 강제 (Grill #14, Inspector slider 정합). `corner_radius` 수치 입력은 *register entry 와 다르게* `corner_rounded: bool` 로 단순화 (Grill #5 — 자동 radius `clamp(min(w,h)*0.15, 4, 16)` 는 FE 계산, BE 는 토글만 저장). 이에 따라 `RectCornerRadiusExceedsBox` ValidationError 는 *신설 안 함*. backward-compat = 모든 필드 `#[serde(default)]` 또는 `default = "default_true"` 로 옛 record 자연 round-trip. 8 신규 schema unit test (rect_fill_stroke_enabled_round_trip / rect_old_layout_defaults_fill_stroke_enabled_true / ellipse_fill_stroke_enabled_round_trip / line_stroke_dash_round_trip / figure_stroke_dash_snake_case_wire / figure_stroke_width_zero_rejected / figure_stroke_width_over_32_rejected / figure_stroke_width_boundary_accepted). 정본 cross-link = `docs/reports/2026-05-20-ui-ux-batch-5-analysis.md` §R1+§R2 + `docs/reports/2026-05-20-be-handover-ui-ux-batch-5.md` §B1+§B3. 본 amend 는 2026-05-17 register entry (b) 의 *promote* — register 의 `fill_pattern?` 후보는 본 batch scope 외 (P1+ 별 ADR).
- 2026-05-20 (**D4 amend ② — Text 풀-style, batch-5 R3 / Grill #6+#7+#15**): D4 의 `text` payload 표 row 갱신. `Item::Text` 에 `font_weight: FontWeight` (Light/Normal/Bold 3-variant — Grill #6, register 의 100~900 numeric 폐기) + `italic: bool` + `underline: bool` + `strikethrough: bool` 신규. 4 boolean 결정 — Grill #15 (register 의 단일 `text_decoration` enum 폐기, underline+strikethrough 동시 표현). 신규 enum `FontWeight: Light | Normal | Bold` (lowercase wire). 신규 `ValidationError::TextFontSizeOutOfRange { font_size }` — `font_size` 8..=96 강제 (Grill #7, Inspector slider 정합). `FontStyle` enum 은 *신설 안 함* — italic 도 `bool` 로 단순화 (Grill #15). backward-compat = 모든 신규 필드 `#[serde(default)]` (FontWeight 의 Default = Normal, bool 의 Default = false) 로 옛 record 자연 round-trip. 4 신규 schema unit test (text_full_style_round_trip / text_old_layout_no_decorations / text_font_size_under_8_rejected / text_font_size_over_96_rejected). 정본 cross-link = `docs/reports/2026-05-20-ui-ux-batch-5-analysis.md` §R3 + `docs/reports/2026-05-20-be-handover-ui-ux-batch-5.md` §B2. 본 amend 는 2026-05-17 register entry (a) 의 *promote* — register 의 `font_family?` / `line_height?` 후보는 본 batch scope 외 (Inspector UI 미노출 결정).
- 2026-05-20 (**D9 amend — Text spawn auto-edit + label-empty derive, batch-5 R7 / Grill #18**): Text item 의 inline-edit lifecycle 보강. (i) **spawn 직후 auto-edit**: `itemFactory.commitNewItem` 의 성공 path 에서 text type 이면 `sessionStore.justSpawnedTextId = id` 신호 set, TextNode 의 mount `$effect` 가 본 값과 self id 가 일치하면 `editing = true` + flag clear (`untrack` 으로 read/write 분리). 옛 path = 사용자가 spawn 후 별도 dblclick 필요 — 본 amend 가 *Figma 패턴* 으로 정합. (ii) **label-empty trigger derive**: `TextNode.onCommit(next)` 에서 `data.label === '' && next.length > 0` 일 때만 label 을 `text.split('\n', 1)[0].trim().slice(0, 4000)` 로 자동 갱신 — 이후 사용자가 Inspector 에서 label 을 따로 설정하면 자동 derive 비활성 (사용자 자율성). label cap 은 기존 D8 의 4 KB byte cap 자연 활용 (char-cap 4000 은 conservative UI guard). (iii) **empty cancel = placeholder 보존**: 빈 text 의 ESC / blur → editing=false 만, item 은 layout 에 남고 "Double-click to edit" placeholder 표시 — 옛 동작 그대로, *delete-on-empty 패턴 거절* (Grill 결정). Schema 변경 0. Settings 신설 0 (`text_label_max_chars` 폐기 — Grill #18). cross-link: `2026-05-20-ui-ux-batch-5-analysis.md` §R7 / `2026-05-20-fe-handover-ui-ux-batch-5.md` §B5/B9/B10.
- 2026-05-20 (**D9 amend — Note body dblclick zone 확장, batch-5 R6 / Grill #13**): NoteNode 의 dblclick inline-edit 진입 zone 을 *root `.note-node` container 전체* 로 확장. 종전 동작 = `.note-label` (title) 과 `.note-body-wrap` (body) 2 분리 zone 만 dblclick 반응 → padding (8px / 12px) + head/body gap (6px) + head row 의 *비라벨* 영역 (glyph, label-이후 빈 공간) 이 dead zone. 본 amend 후 = root container 가 단일 dblclick handler 보유, target 이 button / button-자손 이면 skip (자체 click handler 우선) + .note-label 의 자체 ondblclick (title editing) 은 `stopPropagation` 으로 흡수, 그 외 (head 의 비라벨 영역 / body / padding / gap) 모두 → body editing 진입. *title 영역 별 처리 없음* (Grill #13 — "body 영역만, title 영역 별 처리 X"). MaximizedItemModal 의 `.note-body-host` 도 동일 정책 — host 전체 dblclick → body editing. cross-link: `2026-05-20-ui-ux-batch-5-analysis.md` §R6 / `2026-05-20-fe-handover-ui-ux-batch-5.md` §B8. Schema 변경 0 (UI 정책만).
- 2026-05-23 (**D9 amend — bbox scaler viewport-invariant visual + live resize geometry 정합**): 사용자 보고 3종 (bbox scaler drag 과증폭, rect resize 중 filled shape/bbox 불일치, note scaler corner misalignment) 을 D9 의 편집 시각 정책에 회수. NodeResizer 실제 handle geometry 는 xyflow resize 계산 표면이라 zoom 보정 대상에서 제외하고, visible scaler 는 pseudo visual layer 로만 보정. Canvas capture path 는 resize control / nodrag / endpoint control surface 를 selection 로직에서 제외. ShapeNode 는 live `width/height` prop 을 SVG viewBox/geometry 에 사용해 drag 중 fill/stroke/hit-target 과 bbox 를 동기화. NoteNode 는 Resizer 를 visual node 내부가 아닌 shell 기준으로 배치해 padding/border 가 corner control 기준점을 밀지 않게 함. Schema 변경 0 (runtime/rendering invariant).
- 2026-05-17 (**schema 확장 후보 등록 — plan-0007 §14.4 + handover-v3 §5/§6 정합, land 별 batch**): 3 보완 기능의 schema amend 후보를 plan / handover 에 register. **본 entry 는 register 만** — D2 / D4 본문 표 갱신 + BE serde struct + openapi 재발행은 별 batch (코드 land 시점에 같이) 로 진행.
  - **(a) Text 풀-style** — D4 `text` payload 에 옵셔널 5 필드 후보: `font_family?: string`, `font_weight?: 100~900 | "normal" | "bold"`, `font_style?: "normal" | "italic"`, `text_decoration?: "none" | "underline" | "line-through"`, `line_height?: number` (0.8~2.0). Default fallback (Inspector 노출 안 함): family = system stack / weight = 400 / style = normal / decoration = none / line-height = 1.4.
  - **(b) Figure stroke/fill 패턴** — D4 `rect / ellipse / line` payload 에 옵셔널 2 필드 후보: `stroke_dash?: "solid" | "dash" | "dot" | "dashdot"` (SVG `stroke-dasharray` 매핑, `rect/ellipse/line` 공통) + `fill_pattern?: "solid" | "none" | "hatch"` (`rect/ellipse` 만 — `line` 은 stroke only). Gradient / image fill 같은 복잡 패턴은 P2+ 별 ADR 후보 (본 amend scope 외).
  - **(c) Item rotation (cross-cut)** — D2 `ItemCommon` 에 옵셔널 1 필드 후보: `rotation?: number` (degree, 0~360, default 0). 모든 visual item type 에 영향 (Panel / Text / Note / Rect / Ellipse / Line / FreeDraw / Image / Document / FilePath / Caption). BBox 계산 = 회전 후 axis-aligned bbox 로 재정의 — Multi-item bbox resize (G40) + Alignment (plan-0010 §1) + Layer tree drag reorder 의 bbox 의존 부분과 정합 필요. Snap 정책 = 15° 단위 (Shift hold = 자유 회전).
  - 본 register entry 의 land 시점에 정합 작업 4종 동시 (a/b/c 별 batch 가능):
    1. 본 ADR 의 D2 / D4 표 row 갱신 + 새 enum (TextDecoration, FontStyle, StrokeDash, FillPattern 등) 정의
    2. BE `crates/http-api/src/schema.rs` 의 `Item::*` + `ItemCommon` 의 serde struct 정합 + Option/skip_serializing_if 로 backward-compat
    3. `bin/gen-openapi` 재실행 → `shared/openapi.yaml` + `frontend/src/lib/types/api.d.ts` 재생성 (commit 포함)
    4. FE renderer (TextNode / ShapeNode / LineNode / + transform wrapper) + Inspector v2 의 신규 control row + smoke-7c/7d/7e gate
  - Backward-compat: 모든 필드 옵셔널 + default 정합 → 옛 layout record 도 자연 동작.
  - 본 amend 는 plan-0011 / 0012 후속 batch 후보로 분리 — 본 register 는 cold-pickup 의 next-step 가시화 목적.


- 2026-05-21 (**D10 amend ③ — Document inline + asset preview 의 marked + DOMPurify markdown rendering**): D10 amend ① 의 *inline-stored mode* 와 amend ② 의 asset-based markdown preview 양쪽 모두 *옛 paragraphs.slice(0, 3) + heading 분리* 의 "잘림" 표시 폐기. plan-0011 의 follow-up ("후속에서 marked.js 등 도입") 으로 명시되어있던 step land. `marked` (v18) + `DOMPurify` (v3) 의존성 신규 — `marked.parse(content)` → `DOMPurify.sanitize(html, { USE_PROFILES: { html: true } })` 통과 → `{@html}` 렌더. Security: script / iframe / object / embed / on* event handler 모두 제거. user-content 가 single-user 자체 input 이라 threat surface 낮지만 보안 default 유지. DocumentNode 의 `.doc-body` 가 이미 `overflow:auto + .nowheel` wired (line 525-526) — normal 상태에서 full markdown content scroll 가능. `.doc-md` :global selector 로 marked output 의 h1~h6 / p / ul / ol / li / blockquote / code / pre / a / table / hr / img 모두 token-driven styling. HTML viewer (text/html mime) 는 별 sprint — sandboxed iframe + security model 별 ADR 필요. 정본 cross-link: `docs/plans/0011-component-design-batch-caption-document.md` follow-up.

- 2026-05-21 (**D10 amend ④ — Document viewer 의 normal/maximize sync + HTML source/rendered toggle**): D10 amend ③ 의 markdown rendering 이 `DocumentNode` (normal) 에만 적용되었고 `MaximizedItemModal` (maximize) 는 옛 `parseDocumentText + paragraph slice` 로 그대로 → maximize 시 markdown 미렌더링 (paragraph 분리만) 의 회귀. 본 amend 가: (i) `lib/canvas/documentRender.ts` helper 외부화 (`renderMarkdown` / `renderHtml` / `isToggleableFileType` / `DocumentViewMode` type) — DocumentNode + MaximizedItemModal 양쪽이 *같은 helper* 사용으로 rendering 동기화 강제. (ii) HTML file type (mime: text/html, ext: html/htm) 의 별 분기 — markdown parse 우회하고 `DOMPurify.sanitize` 만 — HTML 의 native rendering (`<p>`, `<h1>`, `<table>`, `<a>` 등 그대로). (iii) **Source / Rendered toggle** — header 의 toggle button (`</>` ↔ eye icon). `viewMode: 'rendered' | 'source'` 상태. source 시 raw text `<pre>` 노출 (사용자가 markdown source 확인 또는 HTML source 확인). markdown / html 양쪽 fileType 에만 button 노출 (다른 type 은 의미 없음). DocumentNode (normal) 와 MaximizedItemModal (maximize) 모두 같은 toggle. Security: helper 의 `renderHtml` 도 DOMPurify USE_PROFILES.html — script/iframe/object/embed/on* 모두 제거. 미해결: viewMode 가 component-local 이라 normal ↔ maximize 전환 시 reset. 후속 (P1) 으로 sessionStore 의 ephemeral 또는 itemUI store 에 persist 고려.

- 2026-05-21 (**D10 amend ⑤ — HTML viewer 의 interactive (sandboxed iframe) + SVG/MathML/media allowlist 확장**): amend ④ 의 미해결 "HTML viewer 는 별 sprint — sandboxed iframe + security model 별 ADR 필요" close. **정본은 [ADR-0037](./0037-document-html-viewer-interactive.md)**. 본 amend 는 cross-link only. 요약: (i) `DocumentViewMode` 가 3-mode (`'rendered' | 'interactive' | 'source'`) 로 확장 (markdown 은 2-mode 유지, html 은 cyclic 3-mode). (ii) `rendered` mode 의 DOMPurify allowlist 확장 — SVG / MathML / `<video>` / `<audio>` / `<source>` / `<track>` / `<picture>` 추가 (B 부분). (iii) `interactive` mode 의 sandboxed iframe — `srcdoc` + `sandbox="allow-scripts allow-popups"` (no `allow-same-origin` → parent storage / cookie 격리). (iv) `documentRender.ts` 의 `getNextViewMode(current, fileType)` helper 추가 — DocumentNode + MaximizedItemModal 의 mode 전이 single source of truth. (v) 보안 default = `rendered` — `interactive` 진입은 사용자 명시 토글 후. amend ④ 의 *viewMode persist* 미해결은 본 amend 에서도 그대로 — 3-mode 확장이 그 follow-up 의 시급성 증대.

- 2026-05-26 (**D10 amend ⑨ — HTML viewer interactive 제거 + rendered iframe 정본화**): 2026-05-26 사용자 재현 `eggroll_visual_summary.html` 로 standalone HTML 의 parent DOM 직접 mount 설계 문제가 확정됐다. 원인: DOMPurify 는 XSS sanitizer 이며 CSS scope isolator 가 아니므로, HTML 안의 `:root` / `body` / universal selector / `.shell` / `.toc` style 이 gtmux chrome/canvas 에 누수될 수 있다. 정본은 [ADR-0037](./0037-document-html-viewer-interactive.md) 의 2026-05-26 대체 amend. 요약: (i) `DocumentViewMode` 는 다시 2-mode (`'rendered' | 'source'`) 로 축소. (ii) HTML `rendered` 는 parent DOM `{@html}` 이 아니라 `iframe srcdoc sandbox="allow-scripts"` 로 격리 — MathJax 같은 script-rendered static output 허용, `allow-same-origin` / `allow-popups` / `allow-top-navigation` / `allow-forms` 는 제외. (iii) `interactive` mode 와 height-probe helper 는 제거 — raw HTML 을 "실행 가능한 앱" 으로 다루는 UX 는 별도 ADR 전까지 비채택. (iv) rendered HTML 에 `<base target="_blank">` 를 주입하지 않아 `href="#..."` 내부 routing link 는 iframe 내부 이동으로 유지. (v) iframe drag isolation 대상은 PDF + rendered HTML iframe (`.doc-pdf`, `.doc-html-frame`) 으로 정리.

- 2026-05-22 (**D10 amend ⑧ 보강 — reactive bypass fast path via pointerdown-capture (self + cross-component)**): amend ⑧ 의 *reactive timing* 한계 회귀 (`class:drag-isolated={dragging}` 가 Svelte 5 reactive 흐름 — `dragging=true` → effect → DOM attr → repaint — 의 frame 갭에서, drag 가 *빠르게* iframe 위를 지나가면 reactive 미적용 frame 에 iframe 이 mouse capture) + *cross-component* 회귀 (다른 component 인 NoteNode / PanelNode / ShapeNode 등 을 drag 중 mouse 가 본 DocumentNode 의 PDF 위 통과하면 PDF plugin 이 그 다른 panel 의 drag 마저 capture 해버림). **Fix 두 trigger path**: (i) `onRootPointerDownCapture` — 자체 panel root 의 pointerdown 시점에 *즉시* 모든 자체 iframe (`.doc-iframe` / `.doc-pdf`) 의 inline `pointer-events: none` direct DOM mutate. self drag (header / resize handle 등) 의 fast path. (ii) **window-level capture-phase `pointerdown` listener** (DocumentNode 의 `$effect` 안에 등록) — *어떤 source* 의 pointerdown 이든 catch 해 자체 iframe 차단. 다른 component 의 drag 시작도 cover. self panel 안 pointerdown 은 `root.contains(e.target)` 으로 early return (자체 capture listener 가 처리, 중복 회피). 두 path 모두 `pointerup`/`pointercancel` (window capture) once 시 복원. **사용자 interact 보호**: `e.target instanceof HTMLIFrameElement` → 사용자가 그 iframe 안 click/scroll 의도, 차단 안 함. 기존 `class:drag-isolated` 도 *fallback layer* 로 유지 — programmatic drag 등 pointerdown 미발사 source 보호. light (코드 ~50줄 + window listener 1개 per DocumentNode; 50 panel 기준 mousedown 마다 50 callbacks × O(1) early-return = 무시 가능).

- 2026-05-22 (**D10 amend ⑧ — Document iframe (PDF + interactive HTML) 의 drag-time pointer-events isolation**): 사용자 보고 (#drag-iframe-capture, PDF 만 처음 보고됨): document item 의 header 를 drag 로 이동 중에 mouse 가 body 의 iframe 영역으로 들어가면 drag 가 멈춰버림. **원인**: iframe 은 자체 browsing context (PDF plugin 또는 sandbox 안의 interactive HTML) 라 부모의 mouse event 와 분리 — drag 중 mouse 가 iframe 위로 들어가면 iframe 안의 plugin/document 가 mouse event 를 capture → xyflow 의 drag mousemove/mouseup 미도달 → drag 멈춤. **Fix**: Svelte Flow custom node 의 `dragging` prop (drag 중 true) 을 reactive 신호로 받아 iframe 의 `pointer-events:none` 을 drag 동안만 적용. drag 종료 시 자동 복원. CSS `.doc-pdf.drag-isolated, .doc-iframe.drag-isolated { pointer-events: none; }`. PDF + interactive HTML 양쪽 동일 fix (interactive HTML 도 같은 잠재 위험 — sandbox 안 document 가 event capture). 적용 site: `DocumentNode.svelte` 의 `dragging` prop 사용 + iframe 3개 (PDF, inline interactive, asset interactive) 모두 `class:drag-isolated`. MaximizedItemModal 은 적용 X — modal 안 panel 은 drag 안 됨. 부수: 본 invariant 는 향후 Document item 에 다른 iframe (예: 미래의 video player iframe) 추가 시에도 동일하게 적용해야 — `dragging` 신호 + `pointer-events:none` 패턴.

- 2026-05-22 (**D10 amend ⑦ — Document PDF viewer via browser-native iframe**): 사용자 요청 — Document component 에 markdown / html 외 **PDF viewer** 도 추가. asset-based mode 만 (inline PDF 불가능 — binary + 64 KB inline cap). 정책: `<iframe src="/api/assets/${asset_id}">` — browser-internal PDF plugin (Chrome PDF Viewer / Safari Preview / Firefox PDF.js 등) 이 PDF rendering / scroll / multi-page 책임. `sandbox` 미지정 — PDF plugin 의 same-origin context 요구 (`sandbox="allow-scripts"` 만 주면 PDF viewer 미작동). same-origin endpoint + single-user trust 라 XSS 위험 최소. ADR-0037 의 *interactive HTML* 의 sandbox-격리 모델과는 별 mental model — PDF 는 사용자 script 실행이 아닌 binary document rendering. `referrerpolicy="no-referrer"` + `loading="lazy"` 적용. viewMode toggle 미지원 (`isToggleableFileType` 그대로 markdown/html 만) — PDF 는 source/interactive 의미 X. CSS: `.doc-pdf` / `.document-pdf` + `:has()` rule — host padding 0, overflow hidden, eyebrow 숨김 (PDF plugin 자체 internal scroll). DocumentNode (normal) + MaximizedItemModal (maximize) 양쪽 동일 패턴. fileTypeLabel = 'pdf' 의 detect 는 기존 helper 가 이미 ext (.pdf) + mime (application/pdf) 모두 인식 — 변경 없음. BE 정합: `crates/http-api/src/assets.rs` 가 PDF magic (`%PDF-`) sniff + `application/pdf` Content-Type serve — 이미 ship. 적용 파일: `DocumentNode.svelte` (`isPdfAsset` / `pdfAssetSrc` derived + 새 template branch + `.doc-pdf` CSS), `MaximizedItemModal.svelte` (`isDocumentPdf` / `documentPdfSrc` derived + 새 template branch + `.document-pdf` CSS). 미해결: 사용자가 PDF 의 *raw download* (source mode 대체) 원하면 별 UI (예: 우 클릭 menu 의 "Download") — 본 amend scope 외. 부수: ADR-0037 §미해결 의 "iframe 안 keyboard isolation" 는 PDF 에도 동일 — Cmd+Z 등이 PDF iframe focus 시 parent 미수신 가능, 사용자 시연 후 별 amend.

- 2026-05-22 (**D10 amend ⑥ — viewMode persist via per-itemId store**): amend ④ 의 미해결 *"viewMode 가 component-local 이라 normal ↔ maximize 전환 시 reset"* close + ADR-0037 D7 ("Maximize ↔ normal viewMode persist — 본 ADR scope 외") close. 신설 store = `lib/stores/documentViewMode.svelte.ts` — `SvelteMap<itemId, DocumentViewMode>` 의 reactive store. DocumentNode (normal) + MaximizedItemModal (maximize) 양쪽이 *같은 store* 의 *같은 itemId* 구독 → normal ↔ maximize 전환 + unmount/remount 시 viewMode reset 회피. 정책: default `'rendered'` 는 storage skip (Map 의 absent entry 가 default 의미 — memory 절약 + size = non-default item 수). session-local ephemeral — durable 정렬 아님. item delete 시 cleanup 은 caller 책임 (optional — dead entry 가 남아도 다른 id 와 충돌 없음). 컴포넌트의 `let viewMode = $state(...)` → `const viewMode = $derived(store.get(data.id))` 로 변경. setter 는 `store.set(id, next)`. interactive→rendered 의 자동 reset effect (file type 변경 시) 도 store 통과. 동기: 3-mode 확장 (amend ⑤) 의 시급성 — interactive 진입 후 maximize 시 sanitized rendered 로 떨어지면 사용자 혼란.
