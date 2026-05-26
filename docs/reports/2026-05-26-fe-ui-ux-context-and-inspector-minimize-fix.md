# 2026-05-26 FE UI/UX 컨텍스트 및 Inspector minimize 복원 버그 수정 보고서

- 일자: 2026-05-26
- 종류: FE UI/UX 분석 + 버그 수정 보고
- 대상: `codebase/frontend`
- 관련 파일:
  - `codebase/frontend/src/lib/chrome/ItemInfoView.svelte`
  - `codebase/frontend/src/lib/canvas/PanelNode.svelte`
  - `codebase/frontend/src/lib/canvas/DocumentNode.svelte`
  - `codebase/frontend/src/lib/canvas/SnippetsNode.svelte`
  - `codebase/frontend/src/lib/canvas/Canvas.svelte`
  - `codebase/frontend/src/lib/canvas/documentRender.ts`

## 0. 요약

현재 FE 는 Svelte 5 + SvelteFlow 기반의 canvas workspace 구현이 상당히 진행된 상태다. 최근 작업 축은 group/drill-in, tree order = z, bbox/resize 안정화, snippets canvas item, document iframe/PDF 렌더링이다.

이번 조사에서 사용자 보고 이슈인 **Inspector 의 minimize toggle 로 item 을 minimize 한 뒤 다시 normal 로 복원하면 기존 사용자가 설정한 크기가 아니라 fallback 임의 크기로 복원되는 문제**를 수정했다. Header 버튼에서 minimize/restore 할 때는 정상 동작했는데, Inspector 경로만 `optimisticMutation` transform 재실행과 restore backup clear 시점이 충돌했다.

정적 검증:

- `pnpm --dir codebase/frontend check` — PASS (`0 errors / 0 warnings`)

## 1. 수정한 문제 — Inspector minimize restore geometry 손실

### 1.1 증상

- Header 의 minimize 버튼으로 접었다가 복원하면 기존 panel/document/snippets/note 크기가 유지된다.
- Inspector 의 State 섹션에서 minimize toggle 을 눌러 접었다가 다시 복원하면 기존 크기가 아니라 type 별 fallback 크기로 복원된다.
  - 예: snippets 는 `320x150`
  - document 는 `360x220`
  - note 는 `240x96`
  - terminal 은 height `220`

### 1.2 원인

`ItemInfoView.svelte` 의 Inspector 경로는 `sessionStore.optimisticMutation()` 을 사용한다. 이 helper 는 같은 transform 을 두 번 실행한다.

1. optimistic UI 반영을 위해 local `priorSnapshot` 에 transform 실행
2. 서버 `mutateLayout()` 에 넘기기 위해 같은 transform 재실행

기존 `applyMinimizeGeom()` 은 transform 안에서 다음 side effect 를 수행했다.

- minimize 진입: `sessionStore.backupItemGeom(...)`
- restore 진입: `sessionStore.getRestoredGeom(...)` 후 `sessionStore.clearRestoredGeom(...)`

따라서 Inspector restore 시 첫 번째 transform 이 backup 을 읽고 즉시 clear 했다. 두 번째 transform 은 같은 restore 계산을 다시 수행하지만 backup 이 이미 없어져 fallback 크기로 복원했다.

Header 버튼 경로는 transform 밖에서 `nextW/nextH` 를 미리 계산하고 `applyMutation()` 에 순수 transform 을 넘기므로 이 문제가 없었다.

### 1.3 변경 내용

`ItemInfoView.svelte` 를 다음 구조로 수정했다.

- `applyCommonBool('minimized', next)` 를 별도 `applyCommonMinimized(next)` 경로로 분리.
- minimize 진입 시 backup 저장은 transform 밖에서 한 번만 수행.
- restore 진입 시 backup snapshot 을 `restoreGeoms` map 에 미리 캡처.
- `applyMinimizeGeom()` 은 `restoreGeoms` 를 입력으로 받는 순수 geometry 계산 함수로 변경.
- restore backup clear 는 `optimisticMutation()` 성공 후에만 수행.
- PUT 실패 또는 reattach guard 실패 시 backup 을 보존해 다음 restore 재시도에서 기존 크기를 유지할 수 있게 했다.
- Document minimized strip height 를 `DocumentNode.svelte` 의 header 버튼 경로와 동일한 `35px` 로 맞췄다. Inspector 경로만 `30px` 를 쓰면 normal 상태는 정상이어도 minimize 상태에서 header strip 이 너무 작게 projection 되어 잠깐 보인 뒤 사라지는 것처럼 보일 수 있다.

### 1.5 추가 수정 — HTML document routing 이 parent workspace 를 이동시키는 문제

사용자 추가 보고: normal 상태에서도 HTML file 안에서 routing 을 시도하면 같은 “보였다가 사라짐” 계열 문제가 발생했다.

확인 결과 rendered mode 의 HTML 은 `{@html}` 로 gtmux parent DOM 안에 직접 삽입된다. 따라서 HTML 내부 `<a href>` 클릭이 gtmux SPA 의 top-level navigation 으로 전파될 수 있고, 상대 route 는 현재 workspace route 를 덮어 document 가 사라진 것처럼 보일 수 있다.

변경:

- `DocumentNode.svelte` 의 normal rendered HTML (`.doc-md`) 에 DOM action 기반 link interceptor 를 추가했다.
- `MaximizedItemModal.svelte` 의 maximized rendered HTML (`.document-md`) 에도 같은 interceptor 를 추가했다.
- rendered HTML 내부 링크 클릭은 `preventDefault()` 후 새 탭으로 열어 parent SPA route 를 변경하지 않게 했다.
- 이후 rendered HTML 이 iframe 으로 격리되면서 `.doc-md` interceptor 는 markdown rendered link 보호 용도로 남고, HTML link 는 iframe 내부 browser context 가 처리한다.

### 1.6 추가 수정 — standalone HTML rendered mode 격리

사용자 재보고 기준 실제 workspace asset 의 문제 파일은 `eggroll_visual_summary.html` 로 확인했다. 이 파일은 fragment 가 아니라 `<!doctype html>` 로 시작하는 standalone HTML 이고, `<style>` 안에 `:root`, `body`, universal selector, `.shell`, `.toc` 등 전역 selector 를 포함한다.

기존 normal/rendered mode 는 `DOMPurify.sanitize(raw)` 결과를 parent DOM 에 `{@html}` 로 직접 주입했다. 따라서 fetch 완료 직후 문서의 전역 CSS 가 gtmux app chrome 에 적용되어 “보이다가 갑자기 사라지는” 것처럼 보일 수 있었다. DOMPurify 는 XSS sanitizer 이며 CSS scope isolator 가 아니므로, standalone HTML 은 parent DOM 에 직접 삽입하면 안 된다.

변경:

- `documentRender.ts`
  - `buildRenderedHtmlSrcdoc(raw)` 추가.
  - rendered HTML 은 `sandbox="allow-scripts"` iframe 으로 격리.
  - MathJax 같은 script-rendered static output 은 허용하되 `allow-same-origin` / `allow-popups` / `allow-top-navigation` 은 주지 않는다.
  - `interactive` mode 와 height probe helper 는 제거. HTML view mode 는 `rendered ↔ source` 두 상태로 축소.
  - rendered HTML 에 `<base target="_blank">` 를 주입하지 않아 `href="#..."` 내부 routing link 가 iframe 내부 이동으로 동작한다.
- `DocumentNode.svelte`
  - normal rendered HTML branch 를 `div.doc-md {@html ...}` 에서 `iframe.doc-html-frame srcdoc=... sandbox="allow-scripts"` 로 변경.
  - drag 중 iframe pointer capture 차단 대상에 `.doc-html-frame` 추가.
  - `.doc-iframe` interactive branch 제거.
- `MaximizedItemModal.svelte`
  - maximize rendered HTML 도 동일하게 `iframe.document-html-frame` 으로 격리.
  - `.document-iframe` interactive branch 제거.

검증:

- `pnpm --dir codebase/frontend check` — 0 errors / 0 warnings.
- `pnpm --dir codebase/frontend build` — 성공. 기존 chunk size warning 만 유지.
- 브라우저 격리 smoke:
  - sandbox iframe 내부 HTML 에 parent 를 오염시키는 `#sentinel`, `.shell`, `body` style 을 넣고 확인.
  - parent DOM 결과: `parentHasInjectedShell=false`, `sentinelColor="rgb(1, 2, 3)"`, `iframeSandbox=""`.
- MathJax 비교 smoke:
  - 원본 HTML 을 직접 열면 MathJax output `mjx-container` 502개 생성.
  - `sandbox=""` rendered iframe 에서는 수식이 raw TeX 로 남음.
  - `sandbox="allow-scripts"` rendered iframe 에서는 Chrome 직접 표시와 같은 수식 렌더가 확인됨.
  - 원본 HTML 의 최종 `scrollHeight` 는 약 34,236px 로 매우 크므로 iframe 자체를 panel viewport 에 채우고 내부 scroll 을 사용한다.
- 내부 routing link:
  - rendered HTML iframe 에 `<base target="_blank">` 를 주입하지 않도록 정리했다.
  - `href="#..."` anchor 는 parent workspace route 를 바꾸지 않고 iframe 내부 hash/scroll 로 처리되는 것이 정본이다.
  - 외부 link navigation / popup / form / download 은 rendered mode 의 scope 가 아니며, 필요 시 별도 live-app/open-external 설계가 필요하다.

남은 한계:

- 해당 session 은 다른 webpage 가 attach 중이라 본 agent 브라우저로 같은 session 을 열어 item-level click flow 까지는 확인하지 못했다.
- 대신 같은 asset HTML 을 로컬 HTTP 진단 서버에서 raw/rendered sandbox 조건으로 비교했다.

### 1.4 영향 범위

- Inspector 의 visible/locked toggle 경로는 기존 `broadcastMutation()` 유지.
- Header minimize 버튼 경로는 변경하지 않았다.
- terminal/note/document/snippets 의 Inspector minimize/restore 경로만 영향.
- figure/text/image/file_path/free_draw/line 은 기존처럼 minimize 미지원이다.

## 2. 현재 FE 작업 컨텍스트

### 2.1 주요 구조

- Canvas 핵심: `Canvas.svelte`
  - SvelteFlow node projection, selection/drill scope, lasso, drag, context menu, viewport 저장 담당.
- Canvas item renderer:
  - `PanelNode.svelte` — terminal panel + xterm host
  - `DocumentNode.svelte` — markdown/html/pdf/source 렌더
  - `SnippetsNode.svelte` — snippets collection
  - `NoteNode.svelte`, `TextNode.svelte`, `ShapeNode.svelte`, `LineNode.svelte`, `ImageNode.svelte`, `FilePathNode.svelte`
- Chrome:
  - `LeftPanel.svelte`, `LayerTreeView.svelte`, `RightPanel.svelte`, `ItemInfoView.svelte`, `Toolbar2.svelte`, `SettingsOverlay.svelte`, modal 계열
- State:
  - `sessionStore.svelte.ts` — layout/items/groups/M/I/viewport/history mutation entry
  - `terminalPool.svelte.ts` — terminal snapshot + UUID→PaneId binding
  - `historyStore`, `toolStore`, dialog stores

### 2.2 최근 반영된 보정

이미 코드에 반영된 중요 개선 사항:

- drag commit 실패 시 prior snapshot rollback
- viewport debounce 와 session switch race 방지
- terminal pool `byId()` O(1) map lookup
- PANE_OUT late buffer running total 적용
- Line endpoint drag 중 unmount listener cleanup
- bbox scaler actual/visual layer 분리
- SvelteFlow resize control surface 를 canvas pointer capture 에서 제외
- Document iframe/PDF drag-time pointer-events isolation

## 3. 남은 UI/UX 보완 후보

### 3.1 Snippets body overflow

`SnippetsNode.svelte` 의 `.snip-body` 는 현재 `overflow: hidden` 이다. entries 가 많거나 node 가 작으면 뒤쪽 badge 또는 add button 접근성이 떨어질 수 있다.

권장:

- compact body 에 `overflow: auto` 또는 내부 scroll region 적용.
- scroll이 생겨도 header/action chrome 과 NodeResizer handle 이 가려지지 않는지 브라우저 확인.

### 3.2 Snippet body 64KB cap 사전 검증

BE 는 snippet body 를 64KB 로 제한하지만 FE edit panel 은 body textarea 에 byte cap / counter / 사전 error 가 없다. 큰 paste 는 BE 400 + generic toast 로 끝날 수 있다.

권장:

- `TextEncoder().encode(body).length` 기반 검증.
- Save disable + error text + byte counter 추가.
- ADR-0038 의 body 길이 정책과 BE cap 문서 정합.

### 3.3 Focus-visible 누락 후보

다음 control 은 `outline: none` 후 명확한 대체 focus ring 이 약하다.

- `Toggle.svelte`
- `InlineEditField.svelte` 의 plain variant
- `InlineEditTextarea.svelte` 의 plain variant

권장:

- keyboard 사용자에게 보이는 focus-visible style 복구.
- canvas 내부 plain edit 은 component chrome 을 깨지 않는 inset/underline focus ring 검토.

### 3.4 Auth page form 접근성

auth page 의 token/password 입력은 시각 label 은 있지만 실제 `<label for>` 연결이 없다. token input 은 `name` / `autocomplete` 속성도 보강 대상이다.

권장:

- token/password 모두 명시 label 연결.
- `name="token"` / `name="password"` 추가.
- token field 는 정책에 맞는 `autocomplete` 값 결정.

### 3.5 Selection elevation 부작용

현재 selected node 는 `.svelte-flow__node.m-selected { z-index: 9999 !important }` 로 전체 node 를 올린다. 선택 bbox 와 resize handle 이 가려지는 문제는 해결하지만, 선택된 item 자체도 시각적으로 앞으로 올라와 tree order = z 와 순간적으로 다르게 보일 수 있다.

권장:

- 장기적으로 ring/handle-only overlay 분리.
- 단기적으로 selection elevation 이 context menu, group overlay, drag reorder 와 충돌하지 않는지 e2e smoke 추가.

### 3.6 SnippetEditPanel Esc 우선순위

`SnippetEditPanel` 은 자체 window keydown 으로 Esc 를 처리하지만, 프로젝트 공통 `escRouter` 우선순위 체인에는 통합되어 있지 않다. modal / settings overlay / inline edit 와 겹칠 때 우선순위 회귀 가능성이 있다.

권장:

- `escRouter` 등록 패턴으로 통합.
- 현재 자체 keydown 은 fallback 으로 유지하거나 제거.

## 4. 보안 / 취약점 관점 메모

### 4.1 Document rendered mode

`documentRender.ts` 는 markdown/html rendered mode 에서 `marked + DOMPurify` 를 사용한다. `{@html}` 사용 지점은 sanitize 된 HTML 렌더에 한정되어 있어 즉시 위험은 낮다.

확인된 정책:

- `<script>`, `on*`, `javascript:`, `<iframe>`, `<object>`, `<embed>` 는 DOMPurify default 금지 유지.
- SVG/MathML/media 는 의도적으로 allowlist 확장.

### 4.2 Document HTML rendered iframe

HTML rendered mode 는 raw HTML 을 sandbox iframe `srcdoc` 으로 넣는다. sandbox 는 `allow-scripts` only 이며 `allow-same-origin`, `allow-popups`, `allow-top-navigation`, `allow-forms` 는 제외되어 있다.

정책:

- MathJax 같은 script-rendered static output 은 허용한다.
- parent DOM / storage / cookie / route navigation 은 browser sandbox 로 격리한다.
- rendered iframe 에 `<base target="_blank">` 를 주입하지 않는다. 내부 `#hash` routing link 는 iframe 안에서 동작해야 한다.
- 이전 `interactive` mode 는 제거했다. raw HTML 을 "실행 가능한 앱" 으로 다루는 기능이 필요하면 별도 설계가 필요하다.

## 5. 테스트 공백

현재 FE 는 `svelte-check` 정적 검증은 통과하지만, Playwright/Vitest 기반의 FE 자동 회귀 테스트가 보이지 않는다.

우선 추가할 smoke:

1. Inspector minimize → restore 가 기존 geometry 를 복원한다.
2. Header minimize → Inspector restore, Inspector minimize → Header restore 교차 동작이 동일하다.
3. bbox resize handle pointerdown 이 selection/lasso/group drag 를 시작하지 않는다.
4. group-selected descendant 의 resize handle / line endpoint 가 숨겨진다.
5. snippets create/edit/delete/reorder/overflow.
6. auth token/password form keyboard 접근.
7. document iframe 위를 지나며 drag 해도 drag 가 끊기지 않는다.

## 6. 다음 작업 권장 순서

1. Snippets overflow + body byte cap.
2. focus-visible / auth form 접근성 보강.
3. Inspector/header minimize 교차 e2e smoke.
4. selection elevation 의 overlay 분리 설계.
5. SnippetEditPanel Esc router 통합.
