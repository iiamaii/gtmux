# ADR-0005: 캔버스 라이브러리 — `@xyflow/svelte` (Svelte Flow) v1.5.x

- 상태: Accepted (2026-05-14)
- 일자: 2026-05-14
- 결정자: frontend-architect
- 근거 보고서: `docs/reports/0003-infinite-canvas.md` (R3 정본), `docs/reports/0008-frontend-stack.md` (R8 §F8 zoom-blur, §F4 store 매핑, §F12 번들 크기)
- 관련 ADR: ADR-0012 (frontend stack, D5에서 본 ADR로 위임), ADR-0010 (Group 데이터 모델, G-hybrid), ADR-0008 (single-pane-per-window + Group), ADR-0004 (터미널 렌더링, xterm.js v6 DOM 호스트 입력 제약), ADR-0002 (WS wire-protocol, 0x83 VIEWPORT_CHANGED)
- 동반 SSoT: `docs/ssot/canvas-layout-schema.md` (Panel/Group 페이로드 정본 — 본 ADR이 라이브러리 매핑을 잠그는 대상)

## 맥락

ADR-0012 D5가 *DOM-host 호환만이 후보 필터*임을 명시한 채 캔버스 라이브러리 결정을 본 ADR로 위임했다. R3 보고서가 DOM-host cut-off (xterm.js의 `<div>` 서브트리를 노드 내용으로 호스트하면서 viewport pan/zoom에 자동 참여 가능) + Svelte 5 정합 + 라이선스 + 50-Panel 성능 기준으로 후보를 평가하고 단일 추천(`@xyflow/svelte` 1.5.x)을 산출했다. R8 보고서가 그 추천을 Svelte 5 runes·xterm.js v6·zoom-blur·번들 크기 차원에서 재검증해 모두 closed로 가져왔다.

본 ADR은 라이브러리 자체뿐 아니라 *gtmux에서의 사용 패턴*(pan 트리거·custom node·Group 모델 매핑·viewport broadcast 경로)을 함께 잠근다. 그 이유는 `@xyflow/svelte`의 일부 native API(특히 `parentNode` sub-flow 모델)가 ADR-0010 G-hybrid의 Group 운영 규칙과 의미가 어긋나므로, *채택하지 않는* API를 ADR 차원에서 명시해 추후 우발 도입을 방지하기 위함이다.

`docs/sketch.md` §10.2 (프런트엔드 구성: infinite canvas renderer / panel manager / panel viewport) 및 §15 1단계 prereq의 일부로서 본 ADR이 발행되며, `docs/sketch.md` §12 P0 (캔버스 panel placement / layout persistence) 진행의 라이브러리 기반이 된다.

## 결정 (Decisions)

- **D1.** 캔버스 라이브러리 = **`@xyflow/svelte` v1.5.x** (Svelte Flow). 실측 버전은 `codebase/frontend/package.json` 의 `"@xyflow/svelte": "^1.5.0"` 으로 잠금. minor·patch 갱신은 본 ADR의 D1 한도(v1.x) 안에서 자유, v2.x 이행은 본 ADR 갱신 또는 supersede 트리거.
- **D2. 렌더링 모드 = DOM-host 노드.** xterm.js v6이 마운트하는 `<div class="xterm">` 서브트리를 custom node 내부에 직접 mount한다. Svelte Flow의 viewport transform(`translate` + `scale`)이 wrapper 레벨에 적용되어 노드 컨텐츠가 자동 pan/zoom에 참여한다. canvas/WebGL-only 렌더링 라이브러리(노드 내용을 GPU 평면에 그리는 라이브러리)는 cut-off 필터로 본 ADR 후보에서 제외된다 (ADR-0012 D5 정합).
- **D3. Pan 트리거 = `panOnDrag={[1, 2]}`** (middle-click 또는 right-click drag만). left-click drag는 *노드 선택·드래그 + 사이드바 reparent UX*에 양보한다 (ADR-0010 D5 G-hybrid drag-reparent). R3 §F4의 `panOnDrag` boolean 또는 `MouseButton[]` 시그니처에 부합. 본 결정은 `Canvas.svelte`의 현행 `panOnDrag={[1, 2]}` 값과 일치.
  - 부속 zoom 정책: `zoomOnScroll=true` (default 유지), `zoomOnDoubleClick=false` (P0에서 더블클릭은 노드 액션에 양보). `selectionOnDrag=false` — left-click drag가 box-select 모드로 전환되면 노드 drag와 충돌하므로 비활성.
  - left-click 한 번 = 노드 선택(M 토글) / 빈 캔버스 클릭 = M clear (Figma 컨벤션). 현행 `Canvas.svelte`의 `onnodeclick`/`onpaneclick` 핸들러가 이 정책의 reference 구현.
- **D4. Custom node 단일 타입 = `nodeTypes={{ panel: PanelNode }}`.** MVP는 `'panel'` 단일 타입만 등록한다. PanelNode 내부 구조:
  - 헤더 (`<header class="panel-header">`)가 drag handle. label·M/lock/minimize/I 배지 표시. Svelte Flow `Node.dragHandle` prop으로 `.panel-header` 셀렉터를 지정하여 *헤더만* drag 트리거. 본문 영역 클릭/drag는 노드 이동을 발생시키지 않는다.
  - 본문 (`<div class="panel-body">`)에 xterm.js host 또는 placeholder 분기. 본문에 Svelte Flow의 `nodrag` / `nowheel` 클래스 또는 동등 속성을 부여하여 본문 내 마우스 이벤트가 *노드 drag*과 *viewport zoom-on-scroll*을 트리거하지 않게 한다 (xterm input·자체 wheel 스크롤에 양보). 현행 `PanelNode.svelte` 가 zoom-blur 분기를 구현하며 `nodrag` / `nowheel` / `dragHandle` 명시 wiring은 향후 FE-2/3 단계에서 마무리.
  - Resize handle: Svelte Flow의 `<NodeResizer>` 별도 컴포넌트로 P1+에서 도입. MVP는 사용자가 드래그로 크기 조정하지 않음 — 신규 Panel 디폴트 480×320 (`PanelNode.svelte`의 `panelW`/`panelH` derived 폴백). 본 결정은 sketch.md §12 P0 범위 정합.
- **D5. Group 모델 매핑 — Svelte Flow native group container 비채택.** Group은 `groups[]` 별도 store(`groupsStore`)로 관리하고, Group 드래그 시 사용자 마우스 delta를 자손 Panel 좌표에 일괄 적용한다 (ADR-0010 D8). Svelte Flow의 `Node.parentNode` / `Node.extent: 'parent'` (sub-flow API)는 *사용하지 않는다*. 그 이유는:
  1. ADR-0010 G-hybrid에서 Group은 frame을 1차 상태로 저장하지 않는다 — `parentNode` 모델은 자식이 부모 상대 좌표를 가지는 sub-flow 의미이며, frame 미저장 모델과 충돌한다.
  2. effective locked 자손은 Group drag delta 적용에서 제외되어야 한다 (ADR-0010 D8). `parentNode` 자동 동기화는 자손별 선택적 제외를 표현하지 못한다.
  3. Group bounding box는 매 렌더마다 *derived* 값이며 (R3 §F6), Svelte Flow의 group container는 별도 1급 노드로 그려져 derived 모델과 의미가 다르다.
  Group의 *사이드바 표현*(Figma-식 layer panel)은 Svelte Flow 영역 밖의 별도 컴포넌트로 구현한다 (R3 §F6, ADR-0010 D5).
- **D6. Viewport broadcast 경로.** Svelte Flow의 `onmove`/`onmoveend` 콜백 + 클라이언트 `ephemeralStore.viewport` 갱신 → **300ms 디바운스** → WS `0x83 VIEWPORT_CHANGED` 송신 (MT-3 D13 broadcast, ADR-0002 SSoT wire-protocol). 매핑 규칙은 R3 §F5: `x = Math.round(viewport.x)` (int32), `y = Math.round(viewport.y)` (int32), `zoom = Math.fround(viewport.zoom)` (float32). 디바운스 값(300ms)은 HTTP `PUT /api/layout` 디바운스(ADR-0012 D12, 300ms)와 동일 값으로 통일하되, 본 ADR 결정 사항은 *디바운스 적용 그 자체*이며 정확한 ms 값은 D19 측정 후 조정 가능(`docs/ssot/wire-protocol.md` MT-3 절 참조).
  - 로컬 viewport(현재 클라이언트의 pan/zoom 상태)는 디바운스 *없이* 즉시 `ephemeralStore.viewport`에 commit한다. PanelNode의 `isAtUnitZoom = |zoom - 1| < 0.02` derived가 매 frame placeholder 분기에 사용되므로 디바운스를 적용하면 zoom-blur 정책(D2 호환성 조건)의 토글이 늦어진다. 디바운스는 *원격 broadcast 송신*에만 적용.
  - 수신 측: 다른 연결의 0x83 broadcast 도착 시 `ephemeralStore.viewport` 덮어쓰기 → Svelte Flow `viewport={...}` controlled prop으로 반영. *self-echo 방지*는 본 ADR 범위 밖 (WS dispatcher의 책임, ADR-0002).
- **D7. 라이브러리 prop 잠금 (현행 wiring 일치 확인).** 다음 prop은 본 ADR이 잠그며 wiring 변경 시 ADR 갱신을 동반한다.
  - `elevateNodesOnSelect={true}` — M 진입 시 z-index 자동 상승 (CONTEXT.md "Z-index 정책", ADR-0010 D11).
  - `onlyRenderVisibleElements={true}` — viewport culling. 50 Panel 평균 동시 렌더 < 50 노드 (R3 §F7, R8 §F11).
  - `minZoom={0.05}`, `maxZoom={3}` — zoom 범위 한도. D5 zoom-blur 정책의 placeholder 분기와 함께 작동.
  - `proOptions={{ hideAttribution: true }}` — MIT 라이선스 하 attribution 숨김 허용 (Svelte Flow 1차 source).
  - `fitView={false}` — 초기 viewport는 SSoT 또는 ephemeralStore에서 복원 (P1+에서 ADR-0006 영속화 시 fitView 활용 검토).
- **D8. Custom node prop 매핑 (R3 §F4 + R8 §F4 store 매핑).** `nodes` derived의 각 entry는 다음 매핑을 유지한다.
  | Svelte Flow `Node` 필드 | SSoT `Panel` 또는 ephemeral 출처 |
  |---|---|
  | `id` | `Panel.id` |
  | `type` | 상수 `'panel'` |
  | `position: {x, y}` | `Panel.x`, `Panel.y` |
  | `width`, `height` | `Panel.w`, `Panel.h` |
  | `zIndex` | `Panel.z` (`zIndexMode`는 default — `elevateNodesOnSelect`로 충분) |
  | `draggable` | `!Panel.locked` (self만; effective locked 누적은 PanelNode 내부 계산) |
  | `selected` | `ephemeralStore.m.has(Panel.id)` |
  | `hidden` | (현재 미사용 — PanelNode 자체에서 `isVisible` 분기로 처리) |
  | `data` | `PanelData` 전체 (label·note·minimized·pane_id·locked·visibility) |

## SSoT 정렬 — `canvas-layout-schema.md` ↔ Svelte Flow `Node`

본 ADR의 라이브러리 사용은 SSoT의 Panel/Group 정의에 1:1 대응한다. 변환 규칙은 R3 §F4 + R8 §F4의 분할-store 패턴을 기준으로 한다.

- **Panel → `Node<PanelData>`**: D8 표의 매핑을 단일 `panelToFlowNode(p: Panel): Node<PanelData>` 함수로 캡슐화한다 (R3 §F4 pseudo-code). 현행 `Canvas.svelte`의 `nodes = $derived(...)` 가 이 함수의 inline 구현. 추후 codegen된 `$lib/types/canvas-layout.d.ts` 정본 도착(R8 F2 도구체인, ADR-0012 D7)과 함께 별도 모듈로 추출 — 본 ADR의 결정은 *매핑 규칙*이며 추출 시점은 wiring 디테일.
- **Group → 별도 store**: D5에 따라 `groups[]`는 Svelte Flow에 nodes로 등록하지 않는다. 사이드바 컴포넌트(`Sidebar.svelte`, ADR-0012 D8)가 `groupsStore` 직접 구독. 캔버스 위 Group bounding box overlay(P1+)는 Svelte Flow의 `<Panel>` 자손 컴포넌트(라이브러리 namespace, gtmux Panel과 명칭 충돌 주의)나 별도 SVG 오버레이로 viewport transform 동기화하여 그린다. ADR-0010 O1과 합류.
- **Viewport ↔ `Viewport`**: Svelte Flow `Viewport = {x, y, zoom}` 타입이 SSoT `ephemeralStore.viewport`와 byte-equal. WS 0x83 broadcast 시 D6 매핑 규칙으로 int32/float32 정규화. HTTP `PUT /api/layout` 페이로드에는 viewport 미포함(durable 아님, ADR-0010 §SSoT 정렬 envelope).
- **ID 컨벤션**: `Panel.id` (`^p[0-9a-zA-Z]{1,32}$`)와 `Group.id` (`^g[0-9a-zA-Z]{1,32}$`)는 prefix로 자연 분리되므로 Svelte Flow `Node.id` (Panel만 등록)는 자동으로 prefix `p`만 가진다. Group의 client-side ID는 라이브러리에 들어가지 않으므로 충돌 표면 없음.

## 거절된 대안 (Rejected)

- **R1. Konva / react-konva** — 캔버스 2D 렌더링 강제. `Html` 컴포넌트는 canvas 위 별도 absolute-positioned DOM이며 viewport transform이 자동 적용되지 않는다. xterm.js의 DOM 트리 + 50 pane × 이벤트 라우팅 이중화 비용. R3 §F2 cut-off, §F1 표.
- **R2. fabric.js / PixiJS + pixi-viewport / Excalidraw core** — canvas/WebGL-only 렌더링. 노드 내용을 GPU 평면에 그리거나 iframe embed로만 외부 컨텐츠 수용. xterm.js DOM 서브트리 호스트 불가. R3 §F1·§F2.
- **R3. vanilla DOM + custom CSS transform 직접 구현** (panzoom + sortablejs 조합) — DOM-host는 자유롭게 가능하나 pan/zoom·hit-test·multi-select·z-index 정책·resize handle을 모두 직접 구현. R3 §self-build cost 추정으로 drag 50줄·resize 200줄·z 100줄 + 회귀 비용. 본 단계 채택 부적합 (백업 옵션 — Svelte Flow가 차단되는 시나리오에서만 재평가, R8-O3 트리거).
- **R4. Svelvet** — Svelte 4 기반. Svelte 5 runes 호환 미검증. `@xyflow/svelte`가 *같은 도메인·같은 모델·Svelte 5 native runes*로 존재하는 이상 우위 부재. R3 §F3.
- **R5. tldraw SDK 4.0+** — DOM-host는 가능하나 Hobby/Commercial 라이선스 키 모델. gtmux의 단일 바이너리 self-hosted 배포(ADR-0011)와 정합 불가. R3 §F2 라이선스 cut-off.
- **R6. React Flow `@xyflow/react` + Svelte wrapper** — 같은 xyflow 팀의 React 변형. 채택 시 React 런타임 dual-runtime 도입 + VDOM diff 비용. ADR-0012 R1과 동일 거절 사유. R3 §F3.
- **R7. maxGraph (Apache-2.0) / Drawflow / tela** — Svelte 5 정합 미문서·multi-select·resize·active maintenance 어느 차원에서도 `@xyflow/svelte`에 미치지 못함. R3 §F1·§F2·옵션 비교표.
  - maxGraph: 프레임워크-agnostic이며 Svelte 통합 사례 0. imperative API를 Svelte action으로 binding하는 비용 + SVG/HTML 혼합 50 pane 측정 자료 부재.
  - Drawflow: vanilla JS, Svelte 5 wrapper 직접 작성. multi-select·z-index·Group 모델 API 미흡.
  - tela: DOM-host ✅·MIT지만 resize handle "todo"·multi-select 미문서. 150 commits/66 stars로 활성 메인테넌스 부족. Svelte Flow가 차단되는 시나리오의 백업 후보로만 보존 (R3 §F1 [13], 본 ADR Open O1의 fallback 트리거 일부).

## 결과 (Consequences)

- **긍정**:
  - pan·zoom·노드 drag·multi-select·z-index·`Background`·`Controls`·`MiniMap` 부속 컴포넌트 ready-made — sketch.md §12 P0/P1/P2 다수 기능이 라이브러리 차원에서 충족.
  - DOM-host 검증 완료 (R3 §F1, R8 §F8) — xterm.js v6 + Svelte Flow custom node 정합.
  - Svelte 5 runes 위에서 ground-up 재작성 — VDOM diff·memoization 부담 없음. MT-3 D13 50 Panel broadcast 시 영향받은 노드만 갱신 (R8 §F3).
  - MIT 라이선스 — ADR-0011 단일 바이너리 배포와 충돌 없음.
  - 번들 크기 35–45 KB gzip (R8 §F12) — cold start < 500ms 예산 안.
- **부정/비용**:
  - `@xyflow/svelte` v1.x → v2.x 이행 시 semver 변동 가능 (xyflow 1.0이 2025 출시, semver 인지). minor bump마다 회귀 테스트 필요 — D7 prop 잠금이 회귀 테스트의 기준점.
  - native group container (D5에서 비채택)를 우회한 결과, Group 시각 힌트(P1+ 캔버스 위 bounding box overlay)는 *별도 SVG 오버레이 + viewport transform 동기화*로 직접 구현 — R3 §F6, ADR-0010 O1.
  - zoom != 1 구간의 xterm.js blur 위험은 ADR-0012 O1 (R8 §F8) placeholder 정책으로 우회됨 — Svelte Flow 자체의 비용이 아니라 *DOM-scaling의 본질적 한계*. 본 ADR은 그 정책을 D2의 일부 호환성 조건으로 인용.
  - middle-click pan 트리거 (D3)는 일부 트랙패드 환경에서 자연스럽지 않음 — D3 한도 안에서 keyboard modifier 보조 트리거를 P1+에서 검토 (Open O4).
- **후속 작업**:
  - Sprint 4-C FE wiring (S4-C 태스크)이 D3·D4·D6·D7을 실코드에 적용. 현행 `codebase/frontend/src/lib/canvas/Canvas.svelte` + `PanelNode.svelte`는 D2·D3·D7·D8을 이미 반영한 *잠정 구현*이며, D4의 `nodrag`/`nowheel` 명시 wiring + D6의 디바운스 적용은 *향후 wiring*으로 남아있다 (코드 비변경, 본 ADR은 결정만 잠금).
  - 50 panel × pan/zoom 60fps 측정 (Open O1) — R8-O3와 합류.
  - ADR-0006 (Canvas Layout 영속화) 작성 시 `fitView` 활용 + 초기 viewport 복원 패턴 결정.

## 불변식 검증

| # | 불변식 | 검증 |
|---|--------|------|
| 1 | tmux 상태 / 웹 상태 분리 | PASS — Svelte Flow는 viewport·노드 좌표·z-index·selection·drag 등 *web-only* 상태만 다룬다. tmux mirror 필드(`pane_id`, output bytes, dead flag)는 `Node.data`에 *읽기 전용*으로 실릴 뿐 라이브러리가 tmux command 발급 경로를 가지지 않는다. SSoT `canvas-layout-schema.md`의 web-domain 필드만 송수신. |
| 2 | tmux-native vs web-only 분기 | PASS — Svelte Flow의 모든 node action(pan/zoom/drag/select/resize)은 web-only이다. tmux-native 액션(new-window/kill-pane)은 별도 Toolbar/CommandPalette 컴포넌트(ADR-0012 D8)에서 발급되며 본 라이브러리 경로에 들어오지 않는다. |
| 3 | **tmux Layout ≠ Canvas Layout (강한 보장)** | **PASS** — `@xyflow/svelte`는 tmux Layout 문자열을 *받지도 발신하지도 않는다*. tmux-domain WS envelope(0x01–0x0F)에 layout 문자열 슬롯이 없고(ADR-0002 allowlist + ADR-0001 control mode), HTTP `PUT /api/layout` 페이로드는 Canvas Layout(`groups`+`panels`)만 schema 강제(ADR-0010 R1, SSoT). 프런트엔드 코드 차원에서 두 layout이 같은 store나 같은 컴포넌트 prop으로 흐를 경로가 부재. ADR-0008 single-pane-per-window 컨벤션에 의해 tmux Layout 자체가 trivial. |
| 4 | 보안 기본값 | PASS — Svelte의 자동 escape가 `Node.data.label`·`note`·`pane_id` 보간을 HTML-escape하므로 §13.3.4 XSS 1차 방어선이 언어 기본값. `{@html}` 사용 정책상 금지(ADR-0012 D6 정합). `@xyflow/svelte` 라이브러리 자체는 외부 입력 fetch나 임의 코드 실행 경로를 노출하지 않으며, CSS-in-JS는 라이브러리 내부 상수값만 사용. `proOptions.hideAttribution`은 MIT 라이선스 하 명시 허용 옵션. |
| 5 | control mode 사용 | N/A — 프런트엔드는 tmux control mode 채널에 직접 접근하지 않는다. WS envelope(ADR-0002)이 추상화 레이어이며 본 ADR의 라이브러리는 그 추상화의 *프런트 측 소비자*. |

## 현행 코드 정합 점검

본 ADR 발행 시점의 `codebase/frontend/src/lib/canvas/` 잠정 구현이 본 결정과 일치하는지 점검한다. 코드는 본 ADR 발행과 함께 *건드리지 않는다* — wiring gap이 있으면 Sprint 4-C FE 태스크의 작업 항목으로 남긴다.

| 결정 | 현행 코드 (`Canvas.svelte` / `PanelNode.svelte`) | 상태 |
|---|---|---|
| D1 (v1.5.x) | `package.json`: `"@xyflow/svelte": "^1.5.0"` | 일치 |
| D2 (DOM-host) | `import { SvelteFlow, Background } from '@xyflow/svelte'` + `PanelNode`가 xterm `<div>` 호스트 | 일치 |
| D3 (`panOnDrag={[1, 2]}`) | `Canvas.svelte` line 115 동일값 | 일치 |
| D4 (nodeTypes / drag handle / nodrag) | `nodeTypes = { panel: PanelNode }` 등록 ✅. `dragHandle`/`nodrag`/`nowheel` 명시 wiring **미적용** | 향후 wiring (Sprint 4-C FE-2) |
| D5 (Group native container 비채택) | 코드에 `parentNode` / `extent: 'parent'` 미사용 | 일치 (negative compliance) |
| D6 (viewport 디바운스 broadcast) | `onmove` 핸들러가 `ephemeralStore.viewport` 즉시 commit. WS 0x83 송신은 **미배선** (dispatcher 트랙) | 향후 wiring (Sprint 4-C FE-3/4) |
| D7 (prop 잠금: `elevateNodesOnSelect=true` / `onlyRenderVisibleElements=true` / `minZoom`/`maxZoom`/`fitView=false`/`hideAttribution`) | 모두 일치 | 일치 |
| D8 (Node 필드 매핑) | `nodes = $derived` 매핑 표 그대로 (id·type·position·width·height·zIndex·draggable·selected·data) | 일치 |

향후 wiring 두 항목(D4 명시 속성, D6 WS 송신)은 본 ADR 결정에 따라 추후 코드 변경으로 채워지며, 본 ADR 발행 자체는 코드 변경을 동반하지 않는다.

## 미해결 항목 (Open)

- **O1. 50 panel × pan/zoom 60fps 유지 측정.** R8-O3와 합류. `onlyRenderVisibleElements=true` + Svelte 5 signals 조합에서 pan 60fps·zoom 60fps의 frame jank가 발생하지 않는지 D19 워크로드(50 pane × 5 고출력)에서 측정. 위반 시 (a) Web Worker 디코더 분리 (ADR-0012 O4 합류), (b) placeholder retain 패턴(`display:none` + xterm 인스턴스 유지) 적용. → 측정 결과로 R8-O3 closed 시 본 ADR도 closed 갱신.
- **O2. MiniMap 도입 시점.** sketch.md §12 P2 항. `@xyflow/svelte`는 `<MiniMap>` 부속 컴포넌트를 제공하며 본 ADR D7 prop 잠금 밖. P2 진입 시점에 ADR 보강 또는 후속 ADR.
- **O3. Group 시각 힌트(P1+)의 native group container 재평가.** D5에서 비채택한 `parentNode` API를 P1+의 캔버스 위 Group bounding box overlay 구현 시 *부분적으로* 재고할지 — drag-delta 모델과 충돌하지 않는 *시각 전용 컨테이너*로 한정 사용 가능한지. ADR-0010 O1·O3과 합류.
- **O4. middle-click pan 보조 트리거.** 트랙패드 환경에서 middle-click 부재 시 `panActivationKey` (Space 등)로 키보드 보조 트리거 도입 검토. P1+ UX 항.
- **O5. D6 viewport broadcast 디바운스 ms 값 확정.** 300ms를 잠정값으로 두되 R8 §F11 viewport sync 측정 후 50–500ms 사이에서 조정 가능. `docs/ssot/wire-protocol.md` MT-3 절에 정본 기록.

## 변경 이력

- 2026-05-14: 초안 → Accepted. ADR-0012 D5에서 위임된 캔버스 라이브러리 결정 잠금 (Sprint 4-A S4-A2 산출).
