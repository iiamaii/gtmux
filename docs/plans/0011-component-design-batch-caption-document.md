# Plan 0011 — Canvas component design batch (Caption / Document / shared rules)

- 일자: 2026-05-16
- 종류: **implementation plan + BE handoff** — 시안 `ref/frontend-design/components.html` 의 §01 / §02 / §05 정합
- 정본 ADR: **ADR-0018 D10** (2026-05-16 amend — caption 신규 + document inline-stored mode)
- 관련 ADR: ADR-0023 (file_path open security — caption / document 무관)
- 시안 ref: `ref/frontend-design/components.html` §01 (Caption) / §02 (Document) / §05 (Shared rules)
- 관련 commit: `c1b980b` (PanelNode §04), `e6e658b` (FilePathNode §03), `30ef6fe` (LayerTreeView row hover 통합)

---

## 0. 한 줄 요약

`시안 §01 Caption + §02 Document 를 schema (ADR-0018 D10) + FE Node component 로 구현. caption 은 신규 type (head/body/meta), document 는 inline-stored mode 추가 (content/file_name + asset_id optional). BE Rust serde model + canvas-layout.schema.json 의 amend 후 FE wire.`

---

## 1. 현재 상태 (본 plan 직전)

| Component | Schema | FE Node | Toolbar | 상태 |
|---|---|---|---|---|
| §04 Panel (Terminal) | ✅ `terminal` 기존 | ✅ `PanelNode.svelte` (시안 정합 — `c1b980b`) | ✅ Terminal tool | **완료** |
| §03 File path | ✅ `file_path` 기존 | ✅ `FilePathNode.svelte` (시안 정합 — `e6e658b`) | ✅ File path tool | **완료** (foot meta 의 lines/KB/branch 는 BE stat 후속) |
| §01 Caption | ❌ — 부재 | ❌ — 부재 | ❌ — 부재 | **본 plan 대상** |
| §02 Document | ⚠️ asset-based — inline-stored 모드 amend 필요 | ❌ — `DocumentNode.svelte` 부재 | ❌ — 부재 | **본 plan 대상** |
| §05 Shared rules | — | — | — | 정합 검증만 — Canvas wrapper 의 selection/hover 정책 (B/C) 검토 후 일관화 |

---

## 2. BE work-package (Slice-A1)

### 2.1 `docs/ssot/canvas-layout.schema.json`

**Caption 추가**:
```jsonc
{
  "if": { "properties": { "type": { "const": "caption" } } },
  "then": {
    "required": ["head", "body"],
    "properties": {
      "head": { "type": "string", "maxLength": 256 },
      "body": { "type": "string", "maxLength": 4096 },
      "meta": { "type": "string", "maxLength": 128 }
    }
  }
}
```

`type` enum 에 `caption` 추가.

**Document amend** — asset_id optional + content/file_name 신규:
```jsonc
{
  "if": { "properties": { "type": { "const": "document" } } },
  "then": {
    "anyOf": [
      { "required": ["asset_id", "mime", "file_name", "size_bytes"] },
      { "required": ["content", "file_name"] }
    ],
    "properties": {
      "asset_id": { "type": "string" },
      "mime": { "type": "string" },
      "file_name": { "type": "string", "maxLength": 256 },
      "size_bytes": { "type": "number" },
      "content": { "type": "string", "maxLength": 65536 }
    }
  }
}
```

두 mode 상호 배타: `asset_id` present → asset mode, absent → inline mode (content 필수).

### 2.2 BE Rust model (`codebase/backend/crates/.../canvas_item.rs`)

`CanvasItem` enum 에 `Caption` variant 추가:
```rust
#[serde(rename = "caption")]
Caption {
    #[serde(flatten)]
    common: ItemCommon,
    head: String,
    body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    meta: Option<String>,
},
```

`Document` variant 의 필드 amend (asset_id optional + content 신규):
```rust
#[serde(rename = "document")]
Document {
    #[serde(flatten)]
    common: ItemCommon,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mime: Option<String>,
    file_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content: Option<String>,
},
```

검증 함수에 두 mode 의 *상호 배타* 검사 추가 (deserialize 후 `validate()` 에서 `asset_id.is_some() ^ content.is_some()` 확인 — 둘 다 none/some 시 error).

### 2.3 BE validation cap

- head: 256 bytes
- body: 4096 bytes
- meta: 128 bytes
- content: 65536 bytes (64 KiB — markdown 기준)
- file_name: 256 bytes

### 2.4 BE Test

- caption round-trip serde test
- document inline-only round-trip (asset_id 없이 content 만)
- document asset-only round-trip (기존 동작 회귀 검증)
- document 의 (asset_id + content 둘 다) → reject
- document 의 (asset_id + content 둘 다 없음) → reject
- caption 의 head/body cap 초과 → reject

### 2.5 Commit
`feat(backend): D10 — caption 신규 + document inline-stored mode (ADR-0018 amend)`

---

## 3. FE work-package (Slice-A2 — BE land 후)

### 3.1 `codebase/frontend/src/lib/types/canvas.ts`

```ts
export interface CaptionItem extends ItemCommon {
  type: 'caption';
  head: string;
  body: string;
  meta?: string;
}

export interface DocumentItem extends ItemCommon {
  type: 'document';
  file_name: string;
  // inline mode
  content?: string;
  // asset mode (기존)
  asset_id?: string;
  mime?: string;
  size_bytes?: number;
}
```

`CanvasItem` union 에 `CaptionItem` 추가. `isCaption` type guard 신규.

### 3.2 `codebase/frontend/src/lib/canvas/CaptionNode.svelte` (신규)

- `<NodeResizer>` (Panel/Note/Shape 와 동일 패턴)
- Layout: flex column, gap 6, padding 10 12 12
- Frame: `--color-surface` + 1px border + radius 4 + 2px accent left rail (`border-left: 2px solid var(--color-accent)`)
- cap-head row: dot (6px accent) + bold figure label + meta (auto-margin right)
- cap-body: sans 12px / 1.4 / fg
- Edit: double-click → 두 InlineEditField (head + body separately)
- min size: 220 × 60

### 3.3 `codebase/frontend/src/lib/canvas/DocumentNode.svelte` (신규)

- `<NodeResizer>`
- Layout: grid 30px / 1fr / 26px
- doc-head: surface-2 strip, file SVG + filename mono + size + edited time
- doc-body: padding 28 36 24, eyebrow + h2 + p (multi-line markdown rendering — 최소 paragraph 분리만, 후속에서 marked.js 등 도입)
- doc-foot: surface-2 strip, page-dots + count + format meta
- Edit: double-click → InlineEditField (file_name 또는 content multi-line)
- min size: 320 × 220

### 3.4 `codebase/frontend/src/lib/canvas/itemFactory.ts`

`createCanvasItem` 의 type-별 default payload 에 caption / document inline mode 추가.

### 3.5 `codebase/frontend/src/lib/canvas/Canvas.svelte` (nodeTypes)

`nodeTypes` 객체 에 `caption: CaptionNode, document: DocumentNode` 추가.

### 3.6 Toolbar 의 tool button 추가

- `toolStore` 의 tool enum 에 `caption` / `document` 추가
- `Toolbar2.svelte` 에 두 도구 button (speech-bubble icon / file icon)

### 3.7 LayerTreeView icon map

`panelTypeIcon` 에 caption / document 의 icon 추가 (speech-bubble / file).

### 3.8 ItemInfoView Type section

multi-select 시에는 Common only — 단일 선택 시 Item Payload section 에 caption (head/body/meta) / document (file_name / content) 표시.

### 3.9 Commit
`feat(frontend): D10 Slice-A2 — CaptionNode + DocumentNode + toolbar + layer icon`

---

## 4. Shared rules (§05) 정합 검증

| Rule | 현 상태 | 액션 |
|---|---|---|
| A. `.canvas-item` wrapper | ✅ SvelteFlow Node 가 wrapper 책임 | — |
| B. Selection = `.canvas-item` outline | ⚠️ 일부 shape (Panel/FilePath) 가 자체 m-single outline | shape 의 outline 제거 + SvelteFlow selection 시 wrapper 가 accent outline (NodeResizer 가 이미 제공) |
| C. Hover = wrapper slight outline | ⚠️ 미적용 | wrapper 에 hover outline 신규 (SvelteFlow node 의 hover state — `.svelte-flow__node:hover`) |
| D. Theme-reactive | ✅ tokens.css 기반 | 단 fp-badge 의 lang color 는 hardcoded — 의도적 |
| E. Layer tree icon map | 부분 (caption / document 미정) | §3.7 의 panelTypeIcon amend |
| F. overflow: hidden | ✅ shape 의 .file-path-node / panel 이미 적용 | caption / document 신규 시도 동일 |

본 plan 후 §5 정합 audit 진행 — 별 commit 또는 plan 후속에 포함.

---

## 5. 우선순위 / 진행 순서

```
BE Slice-A1 (§2) — schema + Rust model + cap + test
   ↓
FE Slice-A2 (§3) — types + 2 Node component + toolbar + nodeTypes + ItemInfoView
   ↓
Shared rules audit (§4) — selection/hover outline 일관화
```

각 slice 한 commit. 전체 ~3-4 commit.

---

## 6. Risk / 후속

| Risk | 완화 |
|---|---|
| document 의 두 mode (asset / inline) → ItemInfoView 표시 분기 복잡 | 본 plan §3.8 에서 `content !== undefined` 가드로 mode 결정 |
| Caption 의 head/body cap 초과 시 BE reject — FE 가 사용자 input 단계에서 미리 검증 안 함 | FE InlineEditField 의 validate 함수에 length check (4 KB 등) — 본 plan §3.2 의 InlineEditField wire 단계에서 |
| FE marked.js 미도입 — document body 가 plain text 만 | 초기 ship 은 paragraph 분리만. marked.js 도입은 후속 ADR |
| BE schema land 전 FE 진행 시 — type guard 실패 | Slice 순서 엄수 (BE 먼저). 단 FE TS type 은 미리 push 가능 (BE 가 *동일 shape* 인 한 unused) |
| Shared rules (§4 B/C) 의 selection/hover outline 변경 — 기존 Panel 의 m-single outline 과 회귀 | 별 commit 으로 분리 + 본 plan 외 (FE-only audit) |

### 후속

- caption / document 의 LayerTreeView display name pattern (`Caption · Fig NN` / `Doc · filename`) — §3.7 amend 시점
- file_path 의 foot row 데이터 (lines / KB / branch) — BE stat / git lookup ADR + schema amend (별 plan)
- §05 의 selection / hover outline 일관화 — FE-only audit + 별 commit

---

## 7. 변경 이력

- 2026-05-16: 초안 — ref/frontend-design/components.html §01 §02 §05 정합. ADR-0018 D10 amend 와 짝. PanelNode (§04) + FilePathNode (§03) 는 본 plan 직전 land.
