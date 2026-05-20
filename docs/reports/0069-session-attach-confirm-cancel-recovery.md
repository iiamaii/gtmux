# 0069 — Session attach confirm cancel 회귀 분석 및 정리

- 작성일: 2026-05-18
- 범위: Frontend session 연결 UX, attach confirm dialog, FE-BE detach API 계약
- 관련 ADR: `docs/adr/0019-session-and-workspace-model.md` D3/D5.5, `docs/adr/0018-canvas-item-data-model.md` D6
- 관련 코드:
  - `codebase/frontend/src/lib/chrome/WorkspaceSwitcher.svelte`
  - `codebase/frontend/src/lib/stores/workspaceSwitcher.svelte.ts`
  - `codebase/frontend/src/lib/http/sessions.ts`
  - `codebase/frontend/src/lib/types/sessions.ts`
  - `codebase/frontend/src/lib/chrome/AttachConfirmModal.svelte`

## 1. 사용자 증상

인증 후 첫 main page 진입에서 session 이 없는 상태로 기존 session 을 선택하면, layout 의 terminal item 과 server terminal pool 이 맞지 않는 경우 `AttachConfirmModal` 이 뜬다. 이 상태에서 `Cancel` 을 누르면 session list 로 돌아가야 하지만 다음 문제가 발생했다.

- no-session guard 는 유지되어 main page 로는 빠지지 않지만,
- toast 로 `Attach cancelled, but previous session could not be restored: POST detach returned 405` 가 표시됨.
- session list 에서 방금 선택했던 session 이 attached/in-use 처럼 비활성화되어 보임.

## 2. 원인

### 2.1 active session 전환 시점이 빨랐음

기존 `WorkspaceSwitcher.tryAttach()` 는 `POST /api/sessions/:name/attach` 응답이 `confirm_required` 인 시점에 `sessionStore.setActiveSession({ name })` 을 먼저 호출했다.

하지만 `confirm_required` 는 아직 사용자가 spawn 을 승인하지 않았고, layout 도 load 되지 않은 중간 상태다. 이 상태를 active session 으로 올리면 `AuthDialog` 의 `dismissable={sessionStore.active !== null}` 조건이 true 가 되어 no-session guard 와 충돌한다.

정확한 의미는 다음과 같다.

- `POST /attach` 성공: BE lock 은 잡힘.
- `confirm_required`: terminal spawn 여부에 대한 사용자 결정이 남아 있음.
- `Confirm attach`: `POST /attach/confirm` + layout fetch 성공 후에야 FE active session 으로 간주 가능.
- `Cancel`: tentative attach lock 을 해제하고 session 선택 흐름으로 돌아가야 함.

### 2.2 detach API 클라이언트가 BE 계약과 불일치

FE 의 `detachSession()` 은 `POST /api/sessions/:name/detach` 를 호출하고 있었다. 그러나 BE 실제 계약은 `DELETE /api/sessions/:name/attach` 이다.

BE route 는 다음 계약을 가진다.

| 동작 | Endpoint |
|---|---|
| attach lock 획득 및 match 분류 | `POST /api/sessions/:name/attach` |
| attach lock 해제 | `DELETE /api/sessions/:name/attach` |
| unmatched terminal spawn 승인 | `POST /api/sessions/:name/attach/confirm` |

따라서 Cancel 시 tentative lock 을 풀려고 해도 `405 Method Not Allowed` 가 발생했고, 서버에는 선택했던 session 의 attach lock 이 남았다. session list 가 그 session 을 attached/in-use 로 표시한 것은 서버 상태 기준으로는 맞았지만, 사용자의 Cancel 의도와는 반대 결과였다.

## 3. 적용한 해결

### 3.1 confirm_required 를 tentative attach 로 분리

`WorkspaceSwitcher.svelte` 에서 `confirm_required` 분기에서는 더 이상 `sessionStore.active` 를 변경하지 않는다. 대신 다음 상태만 별도로 보관한다.

- `pendingAttachPreviousSession`: session switch 중이었다면 이전 active session 이름
- `pendingAttachHasTentativeLock`: BE attach lock 이 이미 잡혔는지 여부

`Confirm attach` 성공 후에만 다음 순서로 FE active session 을 전환한다.

1. `attachConfirm(name)`
2. `getLayout(name)`
3. `sessionStore.setActiveSession({ name })`
4. `sessionStore.loadLayout(layout)`
5. `workspaceSwitcher.close()`

### 3.2 Cancel 시 tentative lock 해제

`AttachConfirmModal` 의 Cancel handler 를 `workspaceSwitcher.goList()` 직접 호출에서 `cancelAttachConfirm()` 으로 바꿨다.

Cancel 흐름:

1. pending session 이 tentative lock 을 가진 상태면 `detachSession(pending)` 호출
2. 이전 active session 이 있던 switch 흐름이면 이전 session 으로 재attach 시도
3. 이전 session 이 없는 첫 진입이면 `sessionStore.clear()` 상태 유지
4. `workspaceSwitcher.goList()` 로 session list 복귀

이로써 no-session 상태에서는 session 없이 main page 로 진입하지 않고, 선택했던 session 도 attached 상태로 남지 않는다.

### 3.3 FE detach API 계약 수정

`detachSession()` 을 다음과 같이 BE 계약에 맞췄다.

- 변경 전: `POST /api/sessions/:name/detach`
- 변경 후: `DELETE /api/sessions/:name/attach`

응답 body 는 현재 FE 에서 사용하지 않으므로 2xx 성공 시 `{ kind: 'ok' }` 로 normalize 한다. 관련 타입 주석도 동일 계약으로 수정했다.

### 3.4 pending state cleanup

`workspaceSwitcher.goList()` 진입 시 `pendingSession` / `pendingSummary` 를 clear 하도록 보강했다. attach confirm 에서 list 로 되돌아온 뒤 stale summary 가 남아 다음 attach 흐름에 섞이는 것을 막는다.

## 4. 검증

실행한 검증:

```bash
pnpm --dir codebase/frontend check
pnpm --dir codebase/frontend build
```

결과:

- `svelte-check`: 0 errors, 0 warnings
- `vite build`: 성공

브라우저 수동 E2E 는 별도 환경 의존성이 있어 이 문서 작성 시점에는 수행하지 않았다. 다음 시나리오는 회귀 테스트 후보로 남긴다.

1. cookie 인증 후 no-session 상태로 main page 진입
2. 기존 session 선택
3. unmatched terminal item 이 있어 `AttachConfirmModal` 노출
4. Cancel 클릭
5. session list 로 복귀
6. 선택했던 session row 가 in-use/disabled 로 남지 않는지 확인
7. choice modal 로 돌아가도 no-session 상태에서는 Cancel 버튼이 없는지 확인

## 5. 남은 리스크와 후속

- FE 컴포넌트 단위 테스트가 없어 session switch modal state machine 을 자동 회귀로 고정하지 못했다. `WorkspaceSwitcher` 의 attach/cancel 흐름은 테스트 seam 을 만들 가치가 있다.
- `ImportSessionModal` 도 `detachSession()` 을 사용하므로 이번 API 계약 수정의 영향을 받는다. 현재 의도상으로는 이쪽도 올바른 방향의 수정이지만, imported session open 중 confirm_required Cancel 시나리오는 별도 수동 확인이 필요하다.
- ADR-0019 에 D5.5 로 “confirm_required 는 tentative attach 이며 FE active 전환 금지” 규칙을 추가했다.

