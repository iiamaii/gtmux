# 보고서: 무한 캔버스 라이브러리 (R3)

- 일자: 2026-05-13
- 담당: deep-research (Batch B2)
- 입력 핸드오프: `docs/src/prompt_research_handoff.md` §4 R3
- 입력 제약: `docs/plans/0002-work-dispatch.md` Task B2 (DOM-host 필터), `docs/sketch.md` §4.1.3·§6.4·§14.4, `docs/reports/0010-grill-amendments.md` D11·D18·D19·D23, `docs/adr/0010-group-data-model.md`, `docs/adr/0012-frontend-stack-svelte.md`, `docs/ssot/canvas-layout-schema.md`

## 요약 (3문장)

DOM-host 컷오프(xterm.js의 `<div>` 서브트리를 노드로 호스트하면서 pan/zoom에 참여 가능)와 Svelte 5 정합 기준으로 평가한 결과, **Svelte Flow (`@xyflow/svelte` v1.5.x)** 가 단일 추천이다. Svelte Flow는 Svelte 5 runes 위에서 ground-up 재작성되어 fine-grained reactivity와 자연 호환되며, custom node가 임의의 Svelte 컴포넌트(곧 임의 DOM 서브트리)를 hosting할 수 있고, MIT 라이선스로 단일 바이너리 배포(ADR-0011)와 충돌이 없다. tldraw·Konva·Pixi.js·Excalidraw 등 1차 후보 다수는 (a) DOM-host 비호환(canvas/WebGL only) 또는 (b) 라이선스/프레임워크 제약으로 본 단계에서 제거되며, tela·custom 직접 구현은 백업 옵션이다.

## 조사 범위와 질문

핵심 결정 질문: gtmux Canvas(50 Panel 동시, 각 Panel = xterm.js 호스팅 DOM 노드, pan/zoom + drag/resize + z-index + 다중 선택 + Group 트리 사이드바)를 구현할 때 어떤 라이브러리를 채택하는가?

DoD 출력:
1. 후보 라이브러리 비교 + 결정적 기준 + 랭크.
2. DOM-host 컷오프로 *제거된 후보 set* 명시.
3. Svelte 5 wrapper 가능성 분석 (React-first 라이브러리 한정).
4. 시리얼라이즈 포맷 샘플(Panel + Group + viewport).
5. 0x83 VIEWPORT_CHANGED 페이로드(`int32 x, int32 y, float32 zoom`)와의 정합 검증.

조사 대상 후보 (핸드오프 §4 R3 + PM 입력):
- (A) Svelte Flow / xyflow `@xyflow/svelte`
- (B) React Flow `@xyflow/react`
- (C) tldraw SDK
- (D) Konva / react-konva
- (E) Pixi.js + pixi-viewport
- (F) Excalidraw core
- (G) mxGraph / maxGraph
- (H) Drawflow
- (I) tela (deta)
- (J) Custom CSS transform 직접 구현 (panzoom, sortablejs 조합)

## 핵심 발견

### F1. DOM-host 컷오프 (`xterm.js <div>` 호스팅 + pan/zoom 참여)

xterm.js는 `terminal.open(element: HTMLElement)`로 자체 DOM(`<div class="xterm">...`)을 mount한다 (ADR-0012 D4, ADR-0004 미발행). 따라서 캔버스 노드의 *내용*이 canvas/WebGL로 그려지는 라이브러리는 불가능하다 — pixel 평면에 그릴 수 있을 뿐 xterm.js의 DOM·이벤트·접근성·CSS 트리를 옮겨 담을 수 없다. 일부 라이브러리는 "HTML overlay" 우회를 제공하지만, 노드가 *canvas 안 객체가 아니라 canvas 위의 별도 DOM*이 되며 pan/zoom과 hit-testing이 분리되어 사용자 멘탈모델·동기화·접근성이 모두 깨진다.

평가 결과:

| 후보 | DOM-host? | 근거 |
|---|---|---|
| (A) Svelte Flow | ✅ | Custom node = 임의 Svelte 컴포넌트. 노드 내부에 xterm.js mount해도 viewport transform이 wrapper에 적용되어 자동 pan/zoom. [1][7] |
| (B) React Flow | ✅ | 동일 모델. 노드는 React 컴포넌트 = 임의 DOM. [3][4] |
| (C) tldraw | ◯ (조건부) | Custom shape는 React 컴포넌트 + HTML/SVG 출력. 임의 DOM 호스팅 가능. *단 라이선스 컷오프로 별도 제거 (F2).* [8][9] |
| (D) Konva / react-konva | ❌ | "Konva is a canvas library and can't render DOM elements directly. The `Html` component will just create a div element and put it on top of the canvas with absolute positioning." 노드가 canvas 위 별도 DOM이 되어 pan/zoom 분리. [10] |
| (E) Pixi.js + pixi-viewport | ❌ | DOMContainer가 있으나 "screen space" 좌표로 동작 — pixi scene graph 안의 진짜 호스팅이 아니라 overlay. 50 pane 동시 표시 시 viewport sync 부담 + DOM event ↔ pixi event 이중 처리 비용. [11] |
| (F) Excalidraw core | ❌ | 자체 캔버스 기반 화이트보드 엔진. embed는 iframe URL 기반으로만 가능하며 임의 DOM subtree 직접 mount API 없음. (1차 출처 부재 — 후속 ADR-0005 시점에 검증 필요한 부분이지만 본 후보군에서 가장 약함) |
| (G) maxGraph | △ (이론적) | "SVG + HTML 혼합 렌더". HTMLLabel을 노드로 사용 가능하지만 1차 시민은 SVG 셰이프. xterm.js 같은 *복합 DOM 서브트리 + 자체 lifecycle*과의 정합 검증 자료 부족. 프레임워크-agnostic이지만 Svelte 통합 사례 0. [12] |
| (H) Drawflow | △ | 노드는 HTML 기반이며 임의 컨텐츠 가능. 그러나 Svelte 5 정합·active maintenance·z-index/Group/multi-select 등 advanced UX API 미흡. |
| (I) tela | ✅ | "Native DOM elements ... so that you can use any existing HTML, CSS, JS component inside a canvas." 그러나 resize handle "todo", multi-selection 미문서화, 메인테넌스 약함. [13] |
| (J) Custom 직접 구현 | ✅ | CSS `transform: translate()`+`scale()` wrapper + 각 Panel `position: absolute` 자체 컴포넌트. 자유도 100%, 라이브러리 부담 0, AI agent generated code 표면적 ↑. |

### F2. 제거된 후보 set (eliminated)

본 단계에서 *후보 단계 컷오프*로 명시 제거:

- **Konva / react-konva** — 캔버스 렌더링 강제. `Html` 컴포넌트는 overlay이며 viewport transform 자동 적용이 아님. xterm.js + 50 pane 시나리오에서 hit-test/접근성/이벤트 라우팅 이중화 불가피.
- **Pixi.js + pixi-viewport** — WebGL 렌더링 + DOMContainer overlay 동일 문제. 추가로 WebGL 컨텍스트 + xterm.js의 WebGL/canvas addon이 동시 활성화될 경우 GPU 리소스 경쟁.
- **Excalidraw core** — 임의 DOM subtree 노드 hosting API 부재. iframe embed 모델만 제공.
- **tldraw SDK 4.0+** — DOM-host는 ✅이지만 **상용/Hobby 라이선스 모델**로 production 사용 시 license key 필수, 100일 trial 후 commercial 또는 hobby license 필요. gtmux는 단일 사용자 self-hosted FOSS 단일 바이너리 배포(ADR-0011 결과)와 정합 불가 → 라이선스 컷오프. [9][14]
- **mxGraph (EOL 2020-11-09)** — 종료. maxGraph fork만 active. mxGraph 자체는 사용 불가.

### F3. Svelte 5 정합

- **Svelte Flow `@xyflow/svelte` 1.0+** 는 Svelte 5 runes 위에서 ground-up 재작성. xyflow 팀이 "fully embraced signals by converting all stores to runes" — `$state` 적용, Svelte 팀의 직접 협업으로 RC 단계에서 패치. v1.5.2 (2026-03-27) 시점 활성 메인테넌스. MIT. [2][6][15]
- React Flow → Svelte wrapper: 가능하지만 (a) VDOM diff 비용을 그대로 가져옴(ADR-0012 R1 거절 사유와 동일), (b) React 18+ 런타임 dependency, (c) Svelte 5 reactivity와의 통합 점이 적은 dual-runtime이 됨. Svelte Flow가 *같은 팀의 같은 API*로 존재하는 이상 React Flow→Svelte wrapper는 부정 선택.
- tldraw → Svelte: 공식 wrapper 없음. React-only SDK. 커뮤니티 wrapper도 부재 (검색 결과 0).
- maxGraph: 프레임워크-agnostic. Svelte 통합 가이드 없음 (자체 imperative API를 Svelte action에 binding 필요).
- tela: Svelte 5 명시 호환 미문서화. 본 보고서 시점 검증 필요.
- Drawflow: vanilla JS lib. Svelte wrapper 직접 작성 필요.

### F4. drag/resize/z-index/multi-select API (Svelte Flow 기준)

Svelte Flow의 `SvelteFlow` 컴포넌트 props 발췌 (1차 출처 [5]):

| 카테고리 | prop | 타입 |
|---|---|---|
| Pan/Zoom | `panOnDrag`, `panOnScroll`, `zoomOnScroll`, `zoomOnPinch`, `zoomOnDoubleClick`, `panActivationKey`, `zoomActivationKey` | `boolean` 또는 `KeyDefinition` |
| Viewport | `viewport`, `initialViewport`, `fitView`, `fitViewOptions`, `minZoom`, `maxZoom`, `translateExtent` | `Viewport`(= `{x, y, zoom}`), … |
| 노드 드래그 | `nodesDraggable`, `nodeDragThreshold`, `nodeClickDistance`, `autoPanOnNodeDrag` | `boolean`, `number` |
| 다중 선택 | `selectionMode`, `selectionOnDrag`, `selectNodesOnDrag`, `selectionKey` | `SelectionMode`, `boolean`, `KeyDefinition` |
| z-index | `elevateNodesOnSelect`, `elevateEdgesOnSelect`, `zIndexMode` | `boolean`, `ZIndexMode` (`auto`·`basic`·`manual`) |
| Grid | `snapGrid` | `SnapGrid` |
| 성능 | `onlyRenderVisibleElements` | `boolean` |

특히:
- **`zIndexMode: 'manual'`** + `Node.zIndex: number`로 D23의 *명시적 정수 z* 정책을 그대로 반영 가능.
- **`elevateNodesOnSelect: true`** = D23의 "M에 들어오는 Panel은 자동 최상위" 정책을 라이브러리 차원에서 자동 충족 (제거하고 직접 핸들 안 되면 false로 두고 직접 처리 가능).
- **`onlyRenderVisibleElements: true`** = §14.4 "비가시 panel 렌더링 최적화"의 라이브러리 측 zero-cost 구현. D16의 Panel Streaming State Suspended는 *데이터 계층*에서 따로 적용 (둘은 직교).

Node 타입 발췌 [16]: `id, position: XYPosition, data: NodeData(generic), type, width, height, hidden, zIndex, selected, dragging, draggable, selectable, deletable, dragHandle, parentId, extent, class, style, domAttributes`. `data: NodeData`가 generic → gtmux의 Panel state(visibility/minimized/locked/label/note 등)를 `data`에 그대로 실어 보낼 수 있다.

**Resize handle**: Svelte Flow는 `<NodeResizer>` 컴포넌트(별도 import)를 통해 노드 안에서 resize handle을 노출. 본 보고서 시점 1차 source 직접 페치 미수행 — ADR-0005 작성 시 API contract 확인 필요 (Open O1).

### F5. Viewport 시리얼라이즈 정합 (D14 0x83)

Svelte Flow `Viewport` 타입 = `{ x: number, y: number, zoom: number }` [17].

D14 0x83 VIEWPORT_CHANGED 페이로드 = `int32 x, int32 y, float32 zoom`.

매핑 규칙:
- 클라이언트는 Svelte Flow의 `viewport` reactive value를 받아 `x = Math.round(viewport.x)`, `y = Math.round(viewport.y)` (int32로 좁힘), `zoom = Math.fround(viewport.zoom)` (float32로 좁힘) 변환 후 WS envelope에 송신.
- 수신 시 역변환은 직접 대입 (int → number, float32 표현 그대로). zoom의 IEEE-754 round-trip은 float32 한도 안에서 lossless. x/y는 픽셀 단위이고 ±2^31 ≈ ±2.1B px 범위로 캔버스 좌표계에 충분.
- 디바운스: VIEWPORT_CHANGED broadcast(D14 MT-3)는 *모든 연결 sync* 필요. Svelte Flow의 `onmoveend` 핸들러로 pan/zoom 종료 시점에 전송 + 16ms throttle로 빈도 제어 권장 (R8 보고서 검증 항목).

### F6. Group overlay 렌더링

ADR-0010 G-hybrid: Group은 spatial frame을 1차 상태로 저장하지 않는다 → "Group bounding box"는 *클라이언트가 자손 Panel 좌표로 매 렌더마다 계산*하는 derived 값이다 (ADR-0010 §결과 부정/비용).

Svelte Flow 정합:
- **Sidebar (Figma-식 layer panel)**: Svelte Flow의 영역 *밖*. 별도 Svelte 컴포넌트로 직접 구현. `groups` 트리(SSoT)에 대한 reactive store + drag-reparent UX (ADR-0010 D5 사이드바 한정).
- **캔버스 위 Group 시각 힌트(P1+)**: Svelte Flow는 `<Background>`·`<Controls>` 등 부속 컴포넌트와 `panelPosition` API로 viewport 위에 정적 오버레이 가능. Group bounding box는 *별도 SVG 오버레이 레이어*를 Svelte Flow viewport transform과 동기화하는 패턴으로 구현. MVP는 사이드바 layer panel만이라 본 단계 비범위.
- Svelte Flow `parentId` 노드 prop은 *sub-flow* 의미(Group inside Group). gtmux Group과 의미가 다르므로 직접 사용하지 않음 — gtmux Group은 `Panel.data.gtmux_parent_id` 또는 별도 store로 관리.

### F7. 성능 (50 DOM 노드 + xterm.js)

1차 출처:
- React Flow 공식 성능 가이드 [4]: "100 default 노드 drag 시 첫 1초 50 FPS → 안정 60 FPS. memo 없이는 default 10 FPS, heavy 노드 2 FPS." → memoization과 store 분리가 결정적. Svelte Flow는 동일 코어이지만 *VDOM 부재 + signals*로 memo 부담 자체가 없음.
- xterm.js 50 인스턴스의 메모리/프레임은 R2 보고서(미작성)의 책임 영역. 본 보고서는 *캔버스 라이브러리 자체의 50-DOM-노드 오버헤드*만 평가.
- Svelte Flow `onlyRenderVisibleElements`로 viewport 밖 노드는 mount 자체 skip 가능. 50 노드는 거의 항상 viewport 안에 들어오지 않을 것 (pan/zoom 자유) → 평균 *동시 렌더 노드 수 < 50*.

추정:
- Pan 중 transform CSS 갱신: GPU compositor 경로 (Panzoom 패턴 [18])로 60 FPS 유지 가능.
- Zoom 중: xterm.js 자체가 `transform: scale()`을 받으면 텍스트가 blur됨 — xterm.js의 `FitAddon`과 zoom interaction은 별도 정책 필요 (R8 검증 항목). 단순히 *Svelte Flow viewport scale을 Panel 컨테이너의 CSS transform으로 적용*하는 기본 동작은 안전.

### F8. 라이선스

| 후보 | 라이선스 | 단일 바이너리 배포 정합 |
|---|---|---|
| Svelte Flow | MIT | ✅ |
| React Flow | MIT | ✅ (Svelte 사용 시 비채택) |
| tldraw 4.0+ | Hobby / Commercial 라이선스 키 필요 (production) | ❌ 컷오프 |
| Konva | MIT | (DOM 컷오프) |
| Pixi.js | MIT | (DOM 컷오프) |
| Excalidraw | MIT (`@excalidraw/excalidraw`) | (DOM 컷오프) |
| maxGraph | Apache-2.0 | ✅ |
| Drawflow | MIT | ✅ |
| tela | MIT | ✅ |
| Custom 구현 | N/A (자작) | ✅ |

## 옵션 비교표 (DOM-host 통과 후보만 랭크)

| Rank | 후보 | DOM-host | Svelte 5 | drag/resize/z/멀티선택 API | 성능 (50 DOM) | 라이선스 | 유지보수 | 종합 |
|---|---|---|---|---|---|---|---|---|
| **1** | **Svelte Flow `@xyflow/svelte`** | ✅ | ✅ Native | ✅ 모두 prop으로 제공 [5] | ✅ `onlyRenderVisibleElements` + signals | ✅ MIT | ✅ v1.5.2 (2026-03), xyflow team | **추천** |
| 2 | Custom 구현 (CSS transform + Svelte action) | ✅ | ✅ Native | △ 자작 부담 (50 lines drag, 200 lines resize, 100 lines z) | ✅ 최저 오버헤드 가능 | ✅ N/A | △ 자체 유지 | **Fallback** — Svelte Flow가 어떤 차원에서 막힐 때 |
| 3 | tela | ✅ | △ Svelte 5 미문서 | ❌ resize "todo", multi-select 미 | △ 미측정 | ✅ MIT | ⚠ 150 commits/66 stars, slow | 본 단계 채택 부적합 |
| 4 | maxGraph | △ HTML 호환 미검증 | ❌ wrapper 직접 작성 | ✅ 풍부하지만 imperative | △ SVG/HTML 혼합 50 pane 미측정 | ✅ Apache-2.0 | ✅ v0.23 (2026-03) | Svelte 통합 비용 과대 |
| 5 | Drawflow | △ | ❌ wrapper 직접 작성 | △ 단순 모델 | △ | ✅ MIT | △ vanilla JS | 도메인 부합도 약 |
| 6 | React Flow → Svelte wrapper | ✅ | ❌ dual runtime | ✅ | △ VDOM 비용 | ✅ MIT | ✅ | Svelte Flow 우선 |

## gtmux에의 함의 (§1 전제 검증 포함)

### §1 전제 매핑

1. **tmux 상태 / 웹 상태 분리** — Svelte Flow는 *web 상태*(Panel position·z·visibility·label·note)만 다룬다. tmux mirror 필드(`Panel.pane_id`, `Panel.data.pane_dead` 등)는 `Node.data`에 *읽기 전용으로* 실어 둘 뿐, 라이브러리가 발신하는 액션은 *web-only 갱신*만이다 (HTTP `PUT /api/layout` 또는 WS 0x80–0x8F). 라이브러리가 tmux 명령을 생성할 경로 부재 → PASS.
2. **tmux-native vs web-only 분기** — Svelte Flow의 node action(drag/select/resize)은 모두 web-only. tmux-native 액션(`new-window`, `kill-pane`)은 별도 toolbar/command palette 컴포넌트에서 발급. 분기 자연 PASS.
3. **tmux 레이아웃 ≠ 캔버스 레이아웃** — Svelte Flow는 tmux Layout 문자열을 *받지도 발신하지도 않는다*. Canvas Layout(`groups`+`panels`+`viewport`)만 다룸. ADR-0012 D8 불변식 #3 PASS와 정합.
4. **보안 디폴트** — Svelte의 자동 escape가 `Node.data.label`·`note` 렌더에 적용. `{@html}` 금지 정책(ADR-0012 D6) 유지. Svelte Flow 자체는 외부 입력 fetch 안 함. PASS.
5. **control mode 사용** — Svelte Flow는 tmux와 무관. N/A.

### 채택 시 영향

- ADR-0005 (canvas lib, 미발행) 본 보고서 결과로 발행 가능.
- ADR-0012 D5 Open closed (DOM-host 후보 = Svelte Flow).
- `Node.data` 안에 SSoT `Panel`의 web-only 필드(visibility, minimized, locked, label, note)를 직접 매핑 → Svelte Flow의 `Node` ↔ SSoT `Panel` 변환 layer 매우 얇음 (양방향 < 30 lines).
- D23 z-index 정책: `zIndexMode: 'manual'` + `Node.zIndex` 직접 제어 + `elevateNodesOnSelect: true`로 자동 최상위 만족.
- D19 50 panel: `onlyRenderVisibleElements` + Svelte 5 signals fine-grained reactivity → memoization 부담 없이 통과 가능 (R8 측정 항목).
- D14 0x83 VIEWPORT_CHANGED: `Viewport = {x, y, zoom}` 직접 매핑 + 클라이언트측 int32/float32 정규화.

### 시리얼라이즈 포맷 샘플 (Panel + Group + Viewport subset)

HTTP `PUT /api/layout` 페이로드 (SSoT `canvas-layout-schema.md` §1 합치, 본 보고서가 Svelte Flow Node 매핑 예시 추가):

```json
{
  "etag": "0123456789abcdef0123456789abcdef",
  "schema_version": 1,
  "groups": [
    {
      "id": "gA1",
      "parent_id": null,
      "label": "logs",
      "color": "#3b82f6",
      "visibility": true,
      "locked": false,
      "order": 0
    },
    {
      "id": "gA2",
      "parent_id": "gA1",
      "label": "tail",
      "color": null,
      "visibility": true,
      "locked": false,
      "order": 0
    }
  ],
  "panels": [
    {
      "id": "p001",
      "parent_id": "gA1",
      "pane_id": "%3",
      "x": 120.0,
      "y": 200.0,
      "w": 640.0,
      "h": 360.0,
      "z": 5,
      "visibility": true,
      "minimized": false,
      "locked": false,
      "label": "server log",
      "note": null
    },
    {
      "id": "p002",
      "parent_id": "gA2",
      "pane_id": "%7",
      "x": 800.0,
      "y": 200.0,
      "w": 640.0,
      "h": 360.0,
      "z": 6,
      "visibility": true,
      "minimized": true,
      "locked": false,
      "label": "tail -f /var/log/app.log",
      "note": "P1 incident watch"
    }
  ]
}
```

Viewport는 SSoT의 `groups`/`panels`와 *별도 페이로드 (WS 0x83 envelope ephemeral)*. HTTP `/api/layout`은 durable durable만 다루므로 viewport 미포함이 정합. WS 0x83 페이로드(논리 표현):

```json
{ "type": "0x83 VIEWPORT_CHANGED", "x": -240, "y": 60, "zoom": 0.875 }
```

(실제 wire는 `[1B 0x83][1B varint paneId=0][int32 x LE][int32 y LE][float32 zoom LE]` **총 14 바이트** binary — ADR-0002 SSoT `wire-protocol.md` §2.2 정의 따름. **LE** 명시 + paneId varint 포함. A4 B2 정정.)

Svelte Flow Node ↔ SSoT Panel 매핑 (TS 타입 수준 의사 코드):

```ts
// gtmux Panel (SSoT) → Svelte Flow Node
function panelToFlowNode(p: Panel): Node<PanelData> {
  return {
    id: p.id,
    type: 'gtmuxPanel',
    position: { x: p.x, y: p.y },
    width: p.w,
    height: p.h,
    zIndex: p.z,
    hidden: !effectiveVisibility(p),       // ADR-0010 D6 AND 전파
    selectable: !effectiveLocked(p),       // OR 전파
    draggable: !effectiveLocked(p),
    deletable: false,                      // 삭제는 toolbar에서 confirm modal 거쳐
    data: {
      pane_id: p.pane_id,
      label: p.label,
      note: p.note,
      minimized: p.minimized,
      locked_self: p.locked,
      gtmux_parent_id: p.parent_id,        // Group 트리 (Svelte Flow parentId와 별도)
    },
  };
}
```

## 미해결 질문 / 후속 ADR 필요 항목

- **O1. `<NodeResizer>` API 직접 검증** — 본 보고서는 props 표를 1차 source로 인용했으나 resize handle 컴포넌트의 정확한 API contract(`onResize`/`onResizeEnd`/`isVisible` 등)와 `effective locked` 자손의 resize 비활성 처리 패턴은 ADR-0005 작성 시점에 1차 source(`svelteflow.dev/api-reference/components/node-resizer`)로 확정.
- **O2. zoom과 xterm.js의 텍스트 렌더링 충돌** — Svelte Flow viewport `zoom`이 Panel 컨테이너 CSS `transform: scale()`로 전달되면 xterm.js의 텍스트(특히 canvas/WebGL renderer)가 blur되거나 좌표 매핑이 어긋남. 정책 옵션:
  - (a) 캔버스 zoom 시 xterm.js 컨테이너는 `transform: scale(1)` + 자체 font-size 조정 (역scale 보정).
  - (b) zoom 중에는 xterm.js를 가리고 placeholder 렌더, zoom 종료 시 복원 (D16의 Suspended 정신과 정합).
  - 결정은 R2 보고서(터미널 렌더링) + R8 보고서(Svelte 5 통합)에서.
- **O3. Group overlay 시각 힌트 (P1+)** — ADR-0010 G-hybrid는 Group spatial frame을 저장하지 않으므로 캔버스 위 "Group 영역 표시"는 *derived bounding box*. Svelte Flow의 sub-flow `parentId` 모델을 *재사용하지 않고* 별도 SVG 오버레이 레이어로 구현하는 패턴 — ADR-0005 또는 후속 ADR에서.
- **O4. `tela` 백업 옵션 maturity 재평가 시점** — Svelte Flow가 차단되는 시나리오(예: Svelte 6 호환 깨짐, MT-3 broadcast 성능 미달)에서 tela/custom 구현 검토. R8 benchmark 결과에 따라 결정.
- **O5. 50 panel × 5 고출력 워크로드 실측** — D19 benchmark 환경에서 Svelte Flow `onlyRenderVisibleElements` ON·OFF, `elevateNodesOnSelect` ON·OFF, `zIndexMode: 'manual'` 시나리오의 메모리·프레임. ADR-0012 Open O3·O4와 합치.
- **O6. tldraw 라이선스 재검토 트리거** — 향후 사용자가 tldraw를 *반드시* 원하면 hobby/commercial 라이선스 비용 + 코드 의존성을 재평가. 현 시점 명시 컷오프.

## 출처 (URL + 접근일자)

모두 2026-05-13 접근.

1. https://xyflow.com/blog/svelte-flow-launch — "Svelte Flow – a library for rendering interactive node-based UIs" (xyflow 공식 블로그).
2. https://xyflow.com/blog/svelte-flow-release — Svelte Flow 1.0 release announcement (runes 마이그레이션 언급).
3. https://reactflow.dev/examples/nodes/custom-node — React Flow 공식 custom node 예제.
4. https://reactflow.dev/learn/advanced-use/performance — React Flow 공식 성능 가이드 (50/100 노드 FPS, memoization 패턴).
5. https://svelteflow.dev/api-reference/svelte-flow — `SvelteFlow` 컴포넌트 props 1차 reference.
6. https://github.com/xyflow/xyflow — xyflow 모노레포 (라이선스 MIT, v1.5.2 2026-03-27).
7. https://svelteflow.dev/examples/nodes/custom-node — Svelte Flow custom node 예제.
8. https://tldraw.dev/features/customization/custom-shapes-and-tools — tldraw custom shape 문서.
9. https://tldraw.dev/blog/tldraw-sdk-4-0 — tldraw SDK 4.0 announcement + 라이선스 모델.
10. https://konvajs.org/docs/react/DOM_Portal.html — Konva `Html` 컴포넌트 (DOM portal overlay 한계).
11. https://pixijs.download/dev/docs/scene.DOMContainer.html — Pixi.js DOMContainer.
12. https://github.com/maxGraph/maxGraph — maxGraph repo (Apache-2.0, v0.23 2026-03-30).
13. https://github.com/deta/tela — tela 라이브러리 (MIT, 150 commits, resize "todo").
14. https://tldraw.dev/community/license — tldraw license 페이지.
15. https://svelteflow.dev/ — Svelte Flow 공식 홈 (MIT 명시).
16. https://svelteflow.dev/api-reference/types/node — `Node` 타입 reference.
17. https://svelteflow.dev/api-reference/types/viewport — `Viewport` 타입 reference (`{x, y, zoom}`).
18. https://timmywil.com/panzoom/ — Panzoom 라이브러리 (CSS transform GPU 가속 패턴 근거).
19. https://medium.com/@Fjonan/performant-drag-and-zoom-using-fabric-js-3f320492f24b — CSS transform vs canvas API 성능 비교 (Custom 구현 fallback 근거).
20. https://github.com/xyflow/xyflow/discussions/4975 — xyflow 공식 discussion: 다수 노드 렌더링 성능 최적화.

## 변경 이력

- 2026-05-13: 초안 (Batch B2 산출).
