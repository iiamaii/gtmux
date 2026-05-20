# Plan 0009 — `/auth` page FE-bundle pivot 구현

- 일자: 2026-05-16
- 작성자: agent (system-architect role) — 사용자 요구 "auth-preview 디자인 연동"
- 종류: **implementation plan + BE work-package**
- 정본 ADR: **ADR-0020 D13** (FE SPA bundle 단일 source 결정)
- 관련 ADR: ADR-0020 D4 / D5 / D8 (endpoint 동작 정의 — 변경 없음)
- 우선: BE 작업이 *blocking* — BE auth.rs / lib.rs 변경 land 전까지 FE pickPage 분기만 해도 `/auth` 접근 시 BE-rendered HTML 이 우선 → FE bundle 미동작

---

## 0. 한 줄 요약

`BE 의 /auth server-rendered handler 를 제거하면 SPA fallback (정적 asset) 으로 자연 진입한다. FE main.ts 의 pickPage 가 /auth 도 AuthPage (routes/auth/+page.svelte) 로 라우팅하면, 사용자가 이미 production-ready 인 FE AuthPage (login fetch / ?t= magic-link / rate-limit countdown / theme apply 모두 구현) 의 디자인 + 동작을 그대로 받는다.`

---

## 1. 현 상태 분석

### 1.1 BE 측 (변경 대상)

| 파일 | line / item | 현 동작 | 변경 후 |
|---|---|---|---|
| `crates/http-api/src/auth.rs` | `pub async fn auth_page_handler(...)` (line ~408) | `/auth` GET 요청 → `Html<String>` 으로 inline template (token form / password form / `?t=` 자동 처리 inline script) 반환 | **제거** — 또는 `#[allow(dead_code)]` 후 후속 clean |
| `crates/http-api/src/lib.rs` | `.route("/auth", get(auth::auth_page_handler))` (line ~631) | 위 handler 를 `/auth` GET 에 mount | **route 정의 제거** — fallback handler (정적 asset / index.html) 가 `/auth` 캐치 |
| `crates/http-api/src/lib.rs` | `is_auth_path` allow-list (line ~679) 의 `path == "/auth"` | auth middleware bypass — cookie 없이 도달 허용 | **유지** — SPA fallback 도 같은 정책 (cookie 없이 정적 asset 도달 허용) |

### 1.2 FE 측 (이미 ready)

| 파일 | 현 상태 |
|---|---|
| `codebase/frontend/src/routes/auth/+page.svelte` | **production-ready** — `lib/http/auth.ts` 의 `login()` 호출, `?t=<token>` 자동 처리 (URL 의 magic-link), rate-limit countdown, theme apply. `ref/frontend-design/auth.html` 디자인 차용. 543 line. *주석에 "demo only" 표현은 stale — D13 amend 로 production path 가 됨* |
| `codebase/frontend/src/main.ts` `pickPage` | `/auth-preview` → AuthPage, 그 외 → AppPage. `/auth` 분기 *추가 필요* |
| `codebase/frontend/src/lib/http/auth.ts` | `POST /auth/login` / `POST /auth/logout` / `POST /auth/rotate` 모두 `credentials: 'include'` + 응답 분기 (ok / invalid / rate_limited / bad_request / unavailable). 변경 없음 |

### 1.3 SPA fallback 검증 필요

BE 의 router 구성에서 *정적 asset / index.html* fallback 이 *어떤 path* 를 catch 하는지 확인. 일반적인 axum 패턴:
- `Router::nest_service("/", ServeDir::new("dist"))` 또는
- `Router::fallback_service(ServeDir::new(...))` 또는
- `Router::route("/*path", get(spa_fallback))`

`/auth` 가 *기존 server-rendered route* 에 가려져 있던 상태에서 route 제거 후 fallback 이 자연 catch 해야 정상. `lib.rs:541` 의 *router 구성 끝* 부분 검증 필요.

---

## 2. BE 작업 inventory (Slice-D-A1 work-package)

### 2.1 `crates/http-api/src/auth.rs`

```rust
// 제거 (또는 #[cfg(test)] 로 격리 — 단 unit test 가 본 handler 를 참조하면 함께 제거):
pub async fn auth_page_handler(
    State(state): State<AppState>,
    ...
) -> Html<String> {
    // ~200 line 의 inline HTML template
}
```

- 함수 본체 + 의존 helper (HTML escape, template builder 등) 가 *오직 본 handler 에서만 사용* 되는지 확인 후 동반 제거.
- 의존 imports (askama / minijinja / format! 등 inline template macro) 정리.

### 2.2 `crates/http-api/src/lib.rs`

```diff
- .route("/auth", get(auth::auth_page_handler))
  .route("/auth/login", axum::routing::post(auth::auth_login_handler))
  .route("/auth/logout", axum::routing::post(auth::auth_logout_handler))
  .route("/auth/bootstrap", get(bootstrap_handler))
```

- `/auth` route 만 제거. 나머지 endpoint 는 유지.
- `is_auth_path` 의 `path == "/auth"` 매칭은 유지 (D13 결정 — cookie 없이 도달 허용).

### 2.3 SPA fallback 동작 검증

router 의 끝부분에서 *fallback / nest_service / route("/*...")* 가 `/auth` GET 요청을 catch 하여 index.html 반환하는지 확인. 만약 fallback 이 `/auth` 를 catch 못 하면 (예: `/api/*` 만 fallback) 명시 추가 필요:

```rust
// dist 의 정적 asset 으로 fallback. /auth 가 정적 file 매칭 실패 시
// index.html 으로 fall through (Axum 의 ServeDir + handle_error).
.fallback_service(
    ServeDir::new("dist")
        .fallback(ServeFile::new("dist/index.html")),
)
```

(현 구성은 lib.rs 의 router builder 안에 이미 존재할 가능성 — 검증 후 결정.)

### 2.4 Test 영향

- `auth_page_handler` 의 unit test (예: `tests/auth_page.rs` 등) 가 있으면 *제거 또는 SPA fallback test 로 변경*. SPA fallback test 패턴:
  ```rust
  let res = app.oneshot(Request::get("/auth").body(Body::empty()).unwrap()).await?;
  assert_eq!(res.status(), 200);
  let body = body_to_string(res).await;
  assert!(body.contains(r#"<div id="app">"#));  // index.html 의 marker
  ```
- `auth_login_handler` / `auth_logout_handler` / `bootstrap_handler` test 는 *영향 없음* — 본 amend 와 무관.

### 2.5 검증 순서

1. `cargo test -p http-api` — 변경 후 PASS 유지 (또는 *handler 제거에 따른* test 1~2 개 update)
2. `cargo build --release` — workspace 전체 build PASS
3. 수동 — 서버 start 후 `curl -i http://localhost:9527/auth` →
   - status 200
   - Content-Type: text/html
   - body 가 *FE bundle index.html* (예: `<script type="module" src="/assets/index-...js">`)

---

## 3. FE 작업 inventory (BE land 후)

### 3.1 `codebase/frontend/src/main.ts`

```diff
 function pickPage(pathname: string): typeof AppPage {
-  if (pathname === '/auth-preview' || pathname.startsWith('/auth-preview/')) {
+  if (
+    pathname === '/auth' ||
+    pathname.startsWith('/auth/') ||
+    pathname === '/auth-preview' ||
+    pathname.startsWith('/auth-preview/')
+  ) {
     return AuthPage;
   }
   return AppPage;
 }
```

**중요**: `/auth/login` / `/auth/logout` / `/auth/bootstrap` 은 BE 가 직접 handle — FE bundle 의 fetch 호출만 통과. `/auth/*` 의 *GET request* 가 brower URL 로 들어오는 경우는 `/auth` 본체뿐. 위 startsWith 가 *fetch path* 와 충돌하지 않는다 (fetch 는 browser routing 와 무관).

### 3.2 `codebase/frontend/src/routes/auth/+page.svelte`

주석 정리 (line 23~26):

```diff
- // ⚠️ 본 SPA 페이지는 BE 의 server-rendered `/auth` (auth.rs:408) 에 의해
- // shadowed 되어 일반 흐름에서는 도달하지 않는다 (BE 가 JS bundle 무관하게
- // auth 게이트를 책임지는 의도, auth.rs:405). main.ts 의 path dispatch 에서도
- // /auth 분기를 제거했다. 본 파일은 *별 path* (예: 디자인 preview) 로
- // 마운트될 때 사용. 디자인 ref/frontend-design/auth.html 의 fancy 버전.
+ // ADR-0020 D13 — `/auth` page 의 단일 source. BE 는 SPA fallback (index.html)
+ // 만 응답하므로 본 컴포넌트가 production path. `/auth-preview` 는 디자인 demo
+ // alias 로 유지 (동일 컴포넌트 mount). 디자인 ref/frontend-design/auth.html.
```

본문 로직은 *변경 없음* — 이미 production-ready.

### 3.3 검증 시나리오

| # | 시나리오 | 기대 |
|---|---|---|
| F-1 | brower `/auth` GET (cookie 없음) | SPA fallback → index.html → AuthPage mount → token form (또는 server mode 에 따라 password form) |
| F-2 | brower `/auth?t=<valid>` | AuthPage onMount 의 `?t=` 자동 처리 → `login({token})` fetch → 200 → `goto('/')` → AppPage 의 auth-gate 통과 |
| F-3 | brower `/auth?t=<invalid>` | login 응답 invalid → form error 표시. 사용자 재시도 가능 |
| F-4 | brower `/auth-preview` | AuthPage mount (현 흐름 유지) — 디자인 demo 동일 |
| F-5 | 사용자 logout 후 `/auth` redirect | SessionMenu 의 onLogout → BE Set-Cookie Max-Age=0 → `window.location.href = '/auth'` → SPA fallback → AuthPage |
| F-6 | `?t=<token>` 의 rate-limit (5회/5분) | login 응답 `rate_limited` → form 비활성 + countdown → expire 후 자동 enable |

---

## 4. 진행 순서

1. **BE Slice-D-A1** (이 plan 의 §2) — auth.rs / lib.rs 변경 + cargo test PASS → commit `feat(backend): D13 — auth page server-rendered 제거, SPA fallback 으로 위임`
2. **FE Slice** (이 plan 의 §3) — main.ts pickPage + AuthPage 주석 정리 → svelte-check + build PASS → commit `feat(frontend): D13 — AuthPage 를 /auth path 의 단일 source 로 mount`
3. **수동 검증** (이 plan 의 §3.3 F-1~F-6) — 사용자 brower 확인
4. **ADR-0020 변경 이력** (이미 D13 amend 와 함께 update) — 별 commit 불필요

---

## 5. Risk / 후속

| Risk | 완화 |
|---|---|
| SPA fallback 이 `/auth` 를 catch 안 함 (router 구성 누락) | §2.3 의 명시 fallback_service 추가. 또는 `Router::route("/auth", get(spa_fallback))` 으로 명시 |
| 옛 `auth_page_handler` 의 inline script (`?t=` 자동 처리) 가 *서버측 redirect* 흐름에 의존 | FE AuthPage 의 `?t=` onMount 처리가 이미 동일 흐름 cover — F-2/F-3 검증 |
| FE bundle 크기 증가 — AuthPage 의 추가 코드가 main bundle 에 포함 | 현재 main bundle 192kb (gzip 56kb). AuthPage 추가 분 (~15kb) 무시 가능. 후속 *code-split* 검토 (dynamic import). |
| `/auth/bootstrap` 의 inline script (D8) 와 충돌 | D8 의 inline script 는 *server-side* 처리로 cookie 발급 후 redirect. 본 amend 와 독립 — 영향 없음 |
| Cookie 없이 정적 asset 도달 = 정보 노출 우려 | bundle 자체는 *공개 source* (production frontend) — 추가 표면 0. server-rendered 도 동일 |

### 후속 작업

- BE Slice-D-A2 (별 plan): SPA fallback 의 명시 catch-all 정책 정리 (D17 또는 새 ADR)
- AuthPage 디자인 polish — `ref/frontend-design/auth.html` 와 1:1 비교 후 시안 차이 정리

---

## 6. 변경 이력

- 2026-05-16: 초안 — 사용자 요구 "auth-preview design 을 실제 /auth 와 연동" 의 BE work-package. ADR-0020 D13 amend 와 짝.
- 2026-05-16: amend ① — BE Slice-D-A1 land. `auth_page_handler` 제거 + lib.rs route 제거 → `fallback_service(ServeDir::new(dist).not_found_service(ServeFile::new(index)))` 가 자연 catch (lib.rs:637-645, 명시 추가 불필요). `is_auth_path` `/auth` 매칭 유지. **§2 의 cold-pickup 시 발견된 정합 gap 한 가지 추가 amend**: `gtmux start` 가 출력하는 URL (`/auth/bootstrap?token=…`) → bootstrap_handler 의 303 redirect target 이 기존 `/auth?token=…` 이었으나 FE AuthPage 는 `?t=` 만 인식 (`routes/auth/+page.svelte:51`) → bootstrap_handler 의 redirect target 을 `/auth?t=…` 로 변경. 검증: cargo test workspace **362 PASS / 0 FAIL** (4 obsolete server-rendered 테스트 제거 + 2 cookie 테스트 의 발급 경로 `POST /auth/login` 로 정합 + 1 bootstrap assertion `?t=` 검증). cargo build --release PASS. 후속 FE side 작업 (§3 main.ts pickPage 분기 + AuthPage 주석 정리) 은 별 commit 으로 진행.
