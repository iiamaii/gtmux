# 0065. Frontend 성능/로직 충돌 리뷰

작성일: 2026-05-17  
범위: 현재 구현된 frontend 코드의 성능 위험, 예외 사용 흐름, 상태 충돌 가능성  
관련 코드: `codebase/frontend/src/lib/canvas/`, `codebase/frontend/src/lib/stores/`, `codebase/frontend/src/lib/ws/`, `codebase/frontend/src/lib/sidebar/`

## 요약

현재 데모 가능한 동작은 유지되지만, 긴 free draw stroke, 다수 terminal/panel, session 전환 중 viewport 저장, 네트워크 실패 중 drag commit 같은 사용 흐름에서 성능 저하 또는 상태 불일치가 발생할 수 있다.

우선순위는 다음 순서가 적절하다.

1. free draw 입력/렌더링 최적화
2. drag commit 실패 rollback
3. terminal pool 조회 구조 개선
4. viewport debounce와 session switch 충돌 해소
5. hot-path 로그/late buffer 비용 축소
6. line endpoint drag listener cleanup

## FE-1. free draw 입력 경로의 누적 비용

### 증상

`free_draw` 도구로 긴 stroke를 그릴 때 pointer move마다 points 배열 전체가 복사되고, preview path도 전체 재계산된다. 5000 point cap이 있어도 입력 중 프레임 비용이 점점 커진다.

### 근거

- `codebase/frontend/src/lib/canvas/Canvas.svelte`
  - `onCanvasPointerMove`: points/pointsLocal을 spread로 매번 복사
  - `ghostPreview`: pointsLocal 전체 순회로 bbox/path 재계산
- `codebase/frontend/src/lib/canvas/FreeDrawNode.svelte`
  - `localPath`: points 전체를 매번 SVG path string으로 변환

### 위험 흐름

- 사용자가 free draw를 길게 그릴수록 입력 지연 증가
- canvas zoom/pan 중 preview가 끊김
- 저사양 환경에서 main thread long task 발생 가능
- 저장된 free draw item이 많아질수록 렌더 비용 증가

### 보완 방향

- 입력 중 points buffer는 non-reactive mutable array로 유지한다.
- preview 갱신은 `requestAnimationFrame` 단위로 제한한다.
- commit 시 Douglas-Peucker 또는 거리 기반 point simplification을 적용한다.
- 저장 schema의 point cap은 유지하되, UI 입력 cap과 저장 cap을 분리해 UX를 부드럽게 만든다.

## FE-2. drag commit 실패 시 UI와 서버 layout 불일치

### 증상

node drag stop에서 frontend store를 먼저 optimistic update하고, 이후 `applyMutation`을 fire-and-forget으로 호출한다. mutation 실패 시 rollback이 없다.

### 근거

- `codebase/frontend/src/lib/canvas/Canvas.svelte`
  - drag stop에서 `sessionStore.items.set(id, next)` 선반영
  - `void sessionStore.applyMutation(...)`
- `codebase/frontend/src/lib/stores/sessionStore.svelte.ts`
  - `applyMutation` 실패 시 toast만 표시하고 이전 snapshot 복원 없음

### 위험 흐름

- 네트워크 단절
- ETag conflict
- auth 만료
- session reattach 실패

이 경우 사용자는 이동된 상태를 보지만, 서버 layout은 이전 상태로 남는다. 새로고침 또는 재진입 시 위치가 되돌아가며 “저장 실패 후 조용한 회귀”처럼 보일 수 있다.

### 보완 방향

- drag commit은 `applyMutation` 결과를 await한다.
- `{ ok: false }`이면 `priorSnapshot`으로 `loadLayout` rollback한다.
- 실패 toast에는 “저장 실패로 이전 위치로 복원됨”처럼 실제 상태 변화를 명시한다.

## FE-3. terminal pool 조회의 O(panel × terminal) 반복

### 증상

`terminalPool.byId(id)`가 배열 선형 검색이다. PanelNode 하나가 여러 derived에서 `byId`를 호출하고, LayerTreeView도 row 표시마다 호출한다.

### 근거

- `codebase/frontend/src/lib/stores/terminalPool.svelte.ts`
  - `byId`: `this.terminals.find(...)`
- `codebase/frontend/src/lib/canvas/PanelNode.svelte`
  - label/status/otherSessions/attachCount에서 반복 조회
- `codebase/frontend/src/lib/sidebar/LayerTreeView.svelte`
  - terminal label 표시 시 조회

### 위험 흐름

- terminal 수와 panel 수가 모두 증가
- `/api/terminals` polling 또는 WS event로 pool refresh 발생
- refresh마다 다수 derived가 재계산되며 선형 검색 반복

### 보완 방향

- `TerminalPoolStore`에서 `terminalsById: SvelteMap<string, TerminalInfo>`를 유지한다.
- `refresh()` 시 array와 map을 같이 갱신한다.
- `byId`는 map lookup으로 변경한다.

## FE-4. viewport debounce와 session switch 충돌 가능성

### 증상

viewport 저장은 500ms debounce로 지연된다. timer가 살아있는 상태에서 session switch/clear가 발생하면, flush 시점의 현재 active session과 현재 viewport를 읽어 저장한다.

### 근거

- `codebase/frontend/src/lib/stores/sessionStore.svelte.ts`
  - `updateViewport`: timer 예약
  - `#flushViewport`: flush 시점의 `this.active`, `this.viewport` 사용
  - `clear`: viewport timer 취소 없음

### 위험 흐름

1. session A에서 pan/zoom
2. 500ms 이내 session B로 switch
3. A의 pending timer가 B의 active/viewport 상태로 실행

상황에 따라 B의 viewport가 잘못 저장될 수 있다.

### 보완 방향

- timer 예약 시 `{ sessionName, viewport }` snapshot을 캡처한다.
- flush 시 active session이 snapshot session과 다르면 폐기한다.
- `clear()`와 명시적 session switch 시작 시 pending timer를 취소한다.

## FE-5. PANE_OUT late buffer와 hot-path debug log 비용

### 증상

subscriber가 없는 pane output은 late buffer에 저장된다. buffer trimming에서 매번 `reduce`를 반복하고, `PANE_OUT`마다 `console.debug`가 호출된다.

### 근거

- `codebase/frontend/src/lib/ws/dispatcher.svelte.ts`
  - `appendLateBuffer`: while 조건 내부에서 `queued.reduce(...)` 반복
  - `handlePaneOut`: no-subscriber/subscriber 양쪽 모두 hot-path debug log

### 위험 흐름

- backend catch-up replay가 관련 없는 pane output까지 전송
- terminal output이 burst 형태로 발생
- subscriber 없는 pane output이 late buffer에 누적

### 보완 방향

- pane별 queued byte total을 별도 유지한다.
- trimming은 누적 total을 증감시키며 O(k) 이하로 처리한다.
- `PANE_OUT` hot-path log는 dev flag, sampling, 또는 counters로 대체한다.

## FE-6. line endpoint drag 중 unmount cleanup 누락

### 증상

Line endpoint drag는 window-level pointer listener를 직접 등록한다. pointerup/cancel에서는 제거하지만, drag 중 component가 unmount되는 흐름에는 cleanup이 없다.

### 근거

- `codebase/frontend/src/lib/canvas/LineNode.svelte`
  - `window.addEventListener('pointermove' | 'pointerup' | 'pointercancel', ...)`
  - `onDestroy(removeWindowListeners)` 없음

### 위험 흐름

- endpoint drag 중 session switch
- endpoint drag 중 item delete
- endpoint drag 중 layout reload

### 보완 방향

- `onDestroy(removeWindowListeners)`를 추가한다.
- destroy 시 `activeEndpoint`, `draft`, `pendingCommit`도 정리한다.

## 검증 메모

- `pnpm --dir codebase/frontend check` 통과.
- 본 문서는 타입 오류가 아니라 부하/예외 흐름에서 나타날 수 있는 구조적 위험을 정리한 것이다.
