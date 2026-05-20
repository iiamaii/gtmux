# 0070 — Webpage owner 적용 후 session list active 판정 회귀 분석

- 작성일: 2026-05-18
- 범위: Frontend `SessionListModal`, `GET /api/sessions`, Webpage-scoped attach owner
- 수정 ADR 확인: `docs/adr/0019-session-and-workspace-model.md` D5.6
- 관련 코드:
  - `codebase/frontend/src/lib/http/sessions.ts`
  - `codebase/backend/crates/http-api/src/sessions.rs`
  - `codebase/backend/crates/http-api/src/lib.rs`

## 1. 사용자 증상

Webpage identity 변경 후 session list 의 `active` 의미를 잘못 해석하면서, 이미 다른 Webpage 에서 열려 있는 session 이 선택 가능해지는 치명적 회귀가 발생했다.

정확한 UX 요구:

- **웹페이지에 열려 있는 모든 session 은 선택할 수 없어야 한다.**
- 현재 Webpage 가 보유한 자기 session 도 session picker 에서는 선택 대상이 아니다.
- Webpage owner identity 는 attach/WS/mutation 권한 구분에 쓰이고, session list 의 선택 가능 여부를 "자기 session 이므로 available" 로 바꾸는 근거가 아니다.

## 2. 원인

직전 수정에서 attach owner 를 auth cookie 단독에서 `auth cookie + tab-scoped webpage id` 로 바꿨다. 이 변경 자체는 D5.6 의 핵심 방향과 맞다. 문제는 그 후 `GET /api/sessions.active` 의 의미까지 owner-relative conflict flag 로 바꿔버린 점이다.

잘못 적용한 의미:

- 현재 Webpage 가 보유한 lock 이면 `active:false`
- 다른 Webpage 가 보유한 lock 이면 `active:true`

이 해석은 "현재 Webpage 의 자기 session 은 선택 가능" 이라는 결과를 만들며, session picker 의 목적과 충돌한다. session picker 는 현재 session 을 다시 고르는 UI가 아니라 새/기존 session 으로 진입하거나 switch 하는 UI이므로, 이미 열려 있는 session 은 owner 와 무관하게 선택 불가여야 한다.

최종 의미:

- lock 이 있으면 `active:true`
- lock 이 없거나 stale 이면 `active:false`
- owner-scoped 구분은 `POST /attach`, `DELETE /attach`, layout-changing mutation, WS routing 에서만 사용한다.

## 3. 적용한 해결

### 3.1 BE list active 판정 복구

`sessions.rs::list_handler` 를 raw lock 판정으로 되돌렸다.

최종 판정:

| lock state | 응답 `active` |
|---|---|---|
| Vacant/Stale | `false` |
| InUse | `true` |
| InUseRaceyBody | `true` |

### 3.2 Webpage owner 적용 범위 재정의

Webpage owner key 는 계속 유지한다. 다만 적용 범위를 명확히 분리했다.

- list 표시: raw session lock 기준
- attach 충돌: owner key 기준 same-owner reattach 만 idempotent, 다른 owner 는 409
- detach: owner-scoped release
- layout-changing mutation: owner key 가 해당 session attach 를 보유해야 통과
- WS routing: owner key 기준 session binding

### 3.3 회귀 테스트 추가

`session_list_disables_any_open_webpage_session` 테스트로 수정했다.

검증하는 조건:

- `page-a` 가 `alpha` 를 attach 한 상태에서 `page-a` 의 session list 도 `alpha.active == true`
- 같은 cookie 의 `page-b` session list 는 `alpha.active == true`

## 4. ADR 반영

`docs/adr/0019-session-and-workspace-model.md` D5.6 을 보완했다.

반영 내용:

- `GET /api/sessions.active` 는 "어느 Webpage 에서든 이미 열려 있어 session picker 에서 선택 불가" 라는 UI-facing flag 로 정의
- 현재 Webpage 가 보유한 session 도 lock 이 있으면 `active:true`
- 다른 Webpage 가 보유한 session 도 `active:true`
- owner-scoped 구분은 list 가 아니라 attach/detach/mutation/WS routing 에서만 사용

## 5. 검증

실행한 검증:

```bash
cargo test --manifest-path codebase/backend/Cargo.toml webpage -- --nocapture
pnpm --dir codebase/frontend check
pnpm --dir codebase/frontend build
```

결과:

- BE webpage owner 관련 테스트 3개 통과
  - `same_cookie_different_webpage_cannot_attach_same_session`
  - `detach_is_webpage_scoped`
  - `session_list_disables_any_open_webpage_session`
- `svelte-check`: 0 errors, 0 warnings
- `vite build`: 성공

## 6. 남은 리스크

- 실제 브라우저에서 old bundle 이 열려 있으면 `webpage_id` 를 보내지 않으므로 hard reload 가 필요하다.
- `active` 필드 이름은 기존 API 호환을 위해 유지한다. 의미는 raw lock 기반 "이미 열려 있어 session picker 에서 선택 불가" 이다. 후속 API 정리 시 `open` 또는 `selectable` 처럼 UI 의미가 더 명확한 필드로 분리할 수 있다.
