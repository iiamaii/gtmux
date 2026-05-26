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
- 2026-05-21: **Follow-up land — Markdown viewer (marked + DOMPurify)**. plan 본문 §171 의 "후속에서 marked.js 등 도입" 항목 land. `marked` 18.x + `DOMPurify` 3.x install. DocumentNode 의 옛 paragraph slice(0, 3) + heading 분리 폐기 — full markdown rendering. `.doc-body` 의 overflow:auto + .nowheel 가 normal 상태 scroll wire. 표준 markdown element (heading / list / blockquote / code block / table / link / hr / img) token-driven styling. HTML viewer 는 별 sprint (security model 결정 필요). ADR-0018 D10 amend ③ 짝.
- 2026-05-22: **Follow-up land — HTML viewer 의 interactive (sandboxed iframe) + SVG/MathML/media allowlist 확장**. 본 follow-up 의 "HTML viewer 는 별 sprint" 미해결 close. 정본 = **ADR-0037** (신규). 변경 요약: (i) `documentRender.ts` 의 `PURIFY_OPTIONS` 확장 — `USE_PROFILES.svg/svgFilters/mathMl` + `<video>/<audio>/<source>/<track>/<picture>` allowlist. (ii) `DocumentViewMode` 3-mode 확장 (`'rendered' | 'interactive' | 'source'`) — markdown 은 2-mode 유지, html 은 cyclic 3-mode. (iii) `interactive` mode = `<iframe srcdoc sandbox="allow-scripts allow-popups">` — script 실행 + `<a target=_blank>` 새 탭 동작 + parent origin storage/cookie 격리 (no `allow-same-origin`). (iv) `documentRender.ts` 에 `getNextViewMode` + `getNextViewModeLabel` + `INTERACTIVE_IFRAME_SANDBOX` 신규 — DocumentNode + MaximizedItemModal 의 toggle 동작 single source. (v) `:has(.doc-iframe)` / `:has(.document-iframe)` CSS — iframe mode 일 때 padding 0 + eyebrow 숨김 (markup churn 없음). 보안 default = `rendered`, `interactive` 진입은 사용자 명시 토글 후. **미해결 (별 follow-up)**: viewMode persist (normal↔maximize 전환 시 reset), iframe height auto-fit, Blob URL 별 origin (외부 CSS / nested iframe), CSP `frame-src` 정합. ADR-0018 D10 amend ⑤ 짝.
- 2026-05-22: **Follow-up land — drag-iframe-capture 의 cross-component coverage (window-level pointerdown listener)**. 사용자 보고 (#drag-cross-component-capture): 직전 fix (f4b22fb) 의 `onpointerdowncapture` 가 *자체 panel root* 에만 등록되어, 다른 component (NoteNode / PanelNode / ShapeNode 등) 를 drag 중 mouse 가 PDF 위 통과 시 capture 회귀 그대로. Fix: `DocumentNode.svelte` 의 `$effect` 안에 **window-level capture-phase `pointerdown` listener** 추가 — 어떤 source 의 pointerdown 이든 catch 해 자체 iframe 차단. self panel pointerdown 은 `root.contains(e.target)` early return (자체 capture listener 가 이미 처리). iframe 자체 target 도 early return (interact 의도). `isolateLocalIframes(root)` helper 로 두 trigger path 의 로직 통합. `bind:this={rootEl}` 추가. ADR-0018 D10 amend ⑧ 보강 짝.
- 2026-05-22: **Follow-up land — drag-iframe-capture 의 reactive bypass fast path**. 사용자 보고 (#drag-iframe-capture-fast): 직전 follow-up 의 `class:drag-isolated={dragging}` 가 Svelte 5 reactive timing 갭 (dragging=true → effect → DOM attr → repaint) 에서 빠른 mousemove 가 iframe 위 지나가면 여전히 capture. Fix: `DocumentNode.svelte` 의 root `onpointerdowncapture` listener — pointerdown 시점에 *즉시* (reactive 거치지 않고) iframe 의 inline `pointer-events: none` direct mutate, `pointerup`/`pointercancel` (window capture) 시 복원. `e.target` 이 iframe 자체면 차단 안 함 (사용자 interact 의도). 기존 `class:drag-isolated` 는 fallback layer 그대로. light (단일 listener + ~20줄). ADR-0018 D10 amend ⑧ 의 보강 짝.
- 2026-05-22: **Follow-up land — Document iframe (PDF + interactive HTML) 의 drag-time pointer-events isolation**. 사용자 보고 (#drag-iframe-capture): document item header drag 중 mouse 가 iframe body 영역으로 들어가면 drag 가 멈춤 — iframe (PDF plugin / sandbox HTML) 이 mouse event 를 capture 해 부모 xyflow 의 drag mousemove/up 미도달. Fix: `DocumentNode.svelte` 의 `dragging` prop (Svelte Flow 의 drag-state 신호) 받아 iframe 에 `class:drag-isolated={dragging}` 적용 + CSS `.doc-pdf.drag-isolated, .doc-iframe.drag-isolated { pointer-events: none }`. drag 동안만 차단, 종료 즉시 복원. PDF + interactive HTML 양쪽 동일 적용 (사용자가 PDF 만 보고했지만 interactive HTML 도 잠재 위험). ADR-0018 D10 amend ⑧ 짝.
- 2026-05-22: **Follow-up land — upload commit-after layout refetch (BE/FE desync workaround)**. 사용자 보고 (#upload-canvas-desync): 파일 선택 후 picker 창 닫혔는데 canvas 에 신규 item 등록 *안 됨* (재현 어려움 — intermittent). browser refresh 하면 보임 → BE 에는 commit, **FE store 가 desync**. 직전 follow-up 의 *cursor 즉시 복귀* 는 tool 활성의 시각 단서 손실로 revert + (i) `sessionStore.reloadActiveLayout()` 신규 — `/api/sessions/{name}/layout` GET → loadLayout 으로 store 동기. silent best-effort (실패해도 functional). (ii) `Canvas.svelte` 의 image / document / file_path 의 commit-after callback 에서 `commitNewItem` 성공 후 `toolStore.consume()` + `void reloadActiveLayout()` 호출 (fire-and-forget). 즉 사용자가 manually 한 *browser refresh* 의 **narrow equivalent** — WS 연결 / maximize / M selection / viewport / 다른 in-flight state 모두 보존, layout 만 fresh sync. 추가 비용 = GET 1회 (수 KB). root cause (applyMutation 의 PUT 응답 + loadLayout 의 race) 의 별 trace 는 후속.
- 2026-05-22: **Follow-up land — async creation 의 tool 즉시 idle 복귀 + pickLocalFile reentrant guard**. 사용자 보고: PDF 같은 무거운 파일 upload 중 tool 이 'document' 로 유지되어 또 클릭하면 native picker 가 중복 열려 중복 입력 가능. fix 두 layer: (i) `Canvas.svelte` 의 image / document / file_path tool 분기에서 `toolStore.consume()` 호출 위치를 await 완료 *후* → picker 열기 *전* 으로 이동. unlocked tool 은 즉시 'select' 복귀 → 추가 click 은 빈 영역 clearM 로 가 picker 중복 호출 X. locked tool 은 consume no-op (lock 의도 유지). (ii) `lib/files/localFilePicker.ts` 에 module-level `pendingPicker` flag — pending picker 가 있으면 새 호출 즉시 null resolve (silent block, native picker 자체가 시각 단서). locked tool + DocumentNode / ImageNode / ItemInfoView 의 onLoadFileClick 의 동시 호출도 자연 보호. `toolStore.svelte.ts` 의 `consume()` doc-comment 에 *async creation 의 timing* contract 명시 (trigger 직후 consume, await 완료 후 X).
- 2026-05-22: **Follow-up land — PDF viewer (browser-native iframe) + figure SVG stroke 의 viewport-independent paint**. 사용자 요청: (1) Document component 에 PDF viewer 추가, (2) 도형 drag-resize 중 stroke 가 비정상 scale 되는 회귀 fix. (i) PDF: `DocumentNode` + `MaximizedItemModal` 에 `isPdfAsset` / `isDocumentPdf` derived + `<iframe src="/api/assets/${asset_id}">` 새 branch. sandbox 미지정 (PDF plugin same-origin 요구). CSS `:has(.doc-pdf)` / `:has(.document-pdf)` — host padding 0 + overflow hidden + eyebrow 숨김 (PDF plugin 자체 scroll). viewMode toggle 미지원. fileTypeLabel = 'pdf' detect 는 기존 helper 활용. BE 정합 = `crates/http-api/src/assets.rs` 의 PDF magic sniff + `application/pdf` Content-Type — 변경 없음. ADR-0018 D10 amend ⑦ 짝. (ii) Stroke fix: `ShapeNode` (rect/ellipse) + `LineNode` + `FreeDrawNode` 의 모든 stroke-bearing SVG element 에 `vector-effect="non-scaling-stroke"` 추가. root cause = drag-resize 중 `data.w/h` 가 `onResizeEnd` 까지 갱신 X → viewBox stale + SVG element 는 wrapper 의 새 크기 + `preserveAspectRatio="none"` → 내용 stretch + stroke 비례 scale. fix = browser-native paint-stage attribute, JS/reactive cost 0. dash pattern + hit-target band 두께 stretch 도 동시 해소. ADR-0005 D10 짝.
- 2026-05-22: **Follow-up land — HTML viewer 2단계 (iframe height auto-fit) + viewMode persist**. ADR-0037 R4 채택 + D7 close. (i) `buildInteractiveSrcdoc(raw)` + `IFRAME_HEIGHT_MESSAGE_TAG` 신규 — raw HTML 끝에 inline probe `<script>` (ResizeObserver + parent.postMessage) inject, parent 가 height 받아 iframe `style.height` 반영. host (`.doc-body:has(.doc-iframe)`) overflow:auto + iframe flex:0 0 auto — content 작으면 fit, 크면 host scroll. (ii) `lib/stores/documentViewMode.svelte.ts` 신설 — `SvelteMap<itemId, DocumentViewMode>` reactive store. DocumentNode + MaximizedItemModal 가 같은 store 구독 → normal↔maximize 전환 시 reset 회피. (iii) probe 의 `contextmenu` preventDefault (canvas 의 ContextMenu 와 mental model 정합). R3 (Blob URL) 은 sandbox + srcdoc 와 격리 모델 동일 — skip. ADR-0018 D10 amend ⑥ + ADR-0037 R4/D7 짝.
- 2026-05-26: **Follow-up amend — HTML viewer interactive 제거 + rendered iframe 정본화**. 사용자 재현 `eggroll_visual_summary.html` 의 root cause = standalone HTML 을 parent DOM 에 직접 mount 하면서 문서의 전역 CSS (`:root`, `body`, universal selector 등) 가 gtmux chrome/canvas 를 오염. 정본 = **ADR-0037 2026-05-26 amend + ADR-0018 D10 amend ⑨**. 변경 요약: (i) `DocumentViewMode` 를 2-mode (`'rendered' | 'source'`) 로 축소. (ii) HTML `rendered` 는 `iframe srcdoc sandbox="allow-scripts"` 로 격리 — MathJax 수식은 동작, parent origin/storage/top navigation/popups/forms 는 차단. (iii) `interactive` mode / height-probe helper / `.doc-iframe` / `.document-iframe` 제거. (iv) rendered HTML 에 `<base target="_blank">` 를 주입하지 않아 내부 `href="#..."` routing link 는 iframe 내부 이동으로 유지. (v) drag-time iframe isolation 대상은 PDF + rendered HTML iframe 으로 정리.
