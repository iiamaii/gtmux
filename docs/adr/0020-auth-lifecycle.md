# ADR-0020: Auth lifecycle — token + password (둘 다 MVP, cookie 기반)

- 상태: Accepted (2026-05-15)
- 일자: 2026-05-15 (Proposed + Accepted, plan 0006 의 multi-session pivot grilling 결과)
- 결정자: agent (security-engineer + system-architect role) + user grilling
- 근거 grilling: 2026-05-15 plan 0006 grilling 의 Q17 (Token + password 둘 다 MVP) + Q8 (인증 후 dialog 흐름)
- 근거 plan: `docs/plans/0007-multi-session-pivot.md`
- 관련 ADR: ADR-0003 (Security defaults), ADR-0019 (Session+Workspace Model — 인증 후 dialog flow), ADR-0011 (Backend stack — Rust + axum)
- 관련 SSoT: `docs/ssot/security-defaults.md` (auth section 큰 amend 필요)

## 맥락

이전 정책 (ADR-0003 + 현 구현):
- `?token=<query>` 만 인증 경로
- bootstrap inline script 가 token 을 sessionStorage 에 굽고 즉시 WS 연결
- 새 webpage 마다 token URL 입력 또는 매번 인증 URL 재방문

사용자 요청 (2026-05-15 multi-session pivot):
- **"사용자 인증 (token or password) 은 lifecycle 두고 활성화 (매번 새롭게 인증 X)"**
- 즉 인증을 *cookie 기반 persisted state* 로 변환
- token / password 둘 중 한 mode 선택 가능 (config)
- 인증 lifecycle 만료 또는 명시 logout 까지 자동 통과

본 ADR 은 이 lifecycle 의 **(a) cookie 정책**, **(b) password mode 의 보안 표면**, **(c) 인증 후 dialog 흐름**, **(d) rotation/logout** 4 차원을 잠근다.

## 결정 (Decisions)

### D1. Auth mode = `token | password` (config 로 선택, MVP 둘 다 지원)

```toml
# ~/.config/gtmux/config.toml
[auth]
mode = "token"     # "token" | "password"
                   # default: "token" (현 정책 유지)
```

- `token` mode: 현 정책 + cookie lifecycle. 외부 도구가 token 생성 (CLI `gtmux start` 가 token 발행).
- `password` mode: 사용자가 password 설정 (CLI `gtmux set-password` 또는 UI 의 Settings). Argon2id hash 저장.
- Mode 변경은 config 변경 + server 재기동.

이유: 사용자 명시 "token or password" — 둘 중 선택 가능해야. 동시 운용 (token AND password) 은 보안 표면 + UX 모호 증가로 거부.

### D2. Cookie 정책

| Attribute | Value | 이유 |
|---|---|---|
| Name | `gtmux_auth` | 단일 cookie |
| Value | random opaque token (32 bytes base64url) | session-id 의미 |
| `HttpOnly` | true | XSS 보호 |
| `Secure` | true (HTTPS) / false (localhost HTTP) | TLS 의무 |
| `SameSite` | Strict | CSRF 보호 |
| `Path` | `/` | server-wide |
| `Max-Age` | **7일** (default) | UX 편의 vs 도난 시 노출. config 로 override 가능 (D3) |

#### Server-side session table

```
~/.local/state/gtmux/auth_sessions.db (또는 in-memory map)
  cookie_token → { issued_at, last_seen, expires_at, mode, ... }
```

- Cookie 유효성 검증: server 가 cookie 값 → table lookup → expires_at 검사 → valid 면 OK.
- Logout: table 의 entry 삭제 + Set-Cookie 의 Max-Age=0.
- Server 재기동: table 휘발 (in-memory) 또는 영속 (sqlite, P1+). MVP 는 in-memory — 재기동 시 모든 사용자가 재인증 필요 (보안 측면 안전).

### D3. Cookie lifecycle 의 config 옵션

```toml
[auth]
mode = "token"
cookie_max_age_days = 7   # default 7, range 1~30
```

- 매번 valid request 마다 last_seen 갱신 + expires_at = now + max_age (rolling renewal).
- Browser 의 Max-Age 도 함께 갱신 (Set-Cookie 매 응답).

#### Idle timeout (rolling vs absolute)
- Rolling renewal: 사용자가 30분마다 active 면 expires_at 도 매번 연장 — 일주일 내내 안 끊김.
- Absolute timeout 옵션 (P1+, `[auth] absolute_timeout_days = 30`) — 사용자가 active 여도 30일 후 강제 logout.

MVP 는 rolling 만.

### D4. Token mode (default)

```
첫 진입 → /auth?token=<value>
            ↓ server 가 token 검증 (state file 의 hash 비교)
            ↓ valid → cookie 발행 + redirect to / (Canvas 진입 흐름)
            ↓ invalid → /auth error page
```

- Token 자체: 32 bytes URL-safe. CLI `gtmux start` 가 발행 + console 에 `http://localhost:9999/auth?token=<...>` 인쇄.
- Token rotation: SessionMenu 의 [Rotate token] 액션 — server 가 새 token 발행 + 옛 cookie 즉시 무효화 + 새 token URL 표시 modal.
- Token 은 *URL 에 한 번* — bootstrap 시 cookie 로 변환된 후 URL 에서 제거 (redirect).

### D5. Password mode

#### Setup
1. CLI `gtmux set-password [--workspace <path>]` — 사용자 입력 prompt → Argon2id hash → `${XDG_STATE_HOME}/gtmux/password.argon2` 저장 (mode 0600).
2. 또는 server 첫 부팅 시 password 가 설정 안 됐으면 *제 1회 setup wizard* (브라우저에서 password 입력 → save).

#### Auth flow
```
첫 진입 → /auth (page)
           ┃ form: [ password: _____ ] [Login]
           ↓ POST /auth/login { password }
           ↓ server: rate limit 검사 → argon2_verify → valid → cookie 발행
           ↓                                        → invalid → error
           ↓ redirect to /
```

#### 보안 표면

| 항목 | 정책 |
|---|---|
| Hash 알고리즘 | Argon2id (memory: 64 MiB, iterations: 3, parallelism: 4) |
| Hash file 권한 | 0600 |
| Hash file 위치 | `${XDG_STATE_HOME:-~/.local/state}/gtmux/password.argon2` |
| Rate limit | 5 시도 / 5분 / IP. 초과 시 429 + Retry-After |
| Lock 정책 | 5분 lockout after 5 failed (lockout 도 5분) |
| Password 변경 | UI 의 Settings → [Change password]: 현재 password 검증 + 새 password 2번 입력 + Argon2 rehash |
| 최소 길이 | 8자 + 영문 + 숫자 (zxcvbn 검사 P2+) |
| 평문 logging 금지 | server log 에서 password 필드 redact |

#### Password reset
- 잊은 경우: CLI `gtmux reset-password --workspace <path>` (file system 접근자만 가능 — local-first 정합).
- Remote reset 메커니즘 없음 (single-user, local-trust).

### D6. 인증 후 Dialog Flow (ADR-0019 D8 와 정합)

```
[Auth page]
    ↓ cookie 발행 (D2)
[Dialog: 새 / 기존 session 선택]
    ↓ 새 → [Session name 입력 modal] (ADR-0019 D7)
    ↓ 기존 → [Session list modal] (ADR-0019 D9)
    ↓
[Canvas 진입]
```

- Dialog 자체는 *우회 불가*. URL `?session=<name>` deep link 는 P1+.
- Cookie 가 valid 한 reload/새 탭 → Auth page 건너뛰고 바로 Dialog.

### D7. Token / Password mode 의 동시 운용 거부 ✗

Config 의 `auth.mode` 는 단일 값. 두 mode 동시 운용 (token 이든 password 이든 OK) 는 거부:

이유:
- 보안 표면 증가 (둘 다 보호 필요)
- UX 모호 (Auth page 에 token 입력 칸과 password 입력 칸 둘 다? 어느 게 우선?)
- Single-user gtmux 의 정합 — 사용자 본인이 어느 mode 인지 알면 충분

전환 흐름: 사용자가 mode 변경 원하면 config 수정 → 재기동. 변경 시 cookie 일괄 무효화.

### D8. Bootstrap inline script 의 변경

현 구현: `/auth/bootstrap?token=<...>` 가 inline script 로 sessionStorage 굽기.

새 구현: `/auth?token=<...>` 또는 `/auth?password=...` 의 *server-side* 처리 → cookie 발행 → redirect to `/`. inline script 의 sessionStorage 패턴 폐기.

- 보안 이점: `</` injection 표면 제거 (사용자가 token URL 만 알면 됨, frontend 가 token 손에 안 들고 cookie 만 read).
- SSoT amend: `docs/ssot/security-defaults.md` 의 bootstrap inline script 부분 폐기.

### D9. Logout 액션

- UI: Titlebar 의 SessionMenu → [Logout] → confirm modal → server 가 cookie 무효화 + redirect to /auth.
- 효과: 그 webpage 의 WS close + cookie clear + Auth page 진입. 다른 webpage (다른 탭) 들은 *각자의 cookie 가 같으므로* 모두 logout 됨 (server-side session table entry 단일).

##### D9.1 FE `onLogout` 3-step 흐름 (2026-05-18 amend — 코드 SoT 정합)

`SessionMenu.svelte:32-44` 의 `onLogout` 정확한 3 step (순서 load-bearing):

```typescript
async function onLogout() {
  // (1) sessionStorageHint.clear() — sessionStore.clear() 를 거치지 않고
  //     즉시 page redirect 하므로 hint 명시 제거 (D5.4 reconnectGate 의
  //     hint 기반 silent attempt 가 redirect 후 fresh /auth 진입을 침해
  //     안 하도록).
  sessionStorageHint.clear();

  // (2) POST /auth/logout — Set-Cookie Max-Age=0. 실패해도 (3) 진행:
  //     cookie 가 살아있다면 다음 /auth 진입에서 BE 가 다시 검증 + 만료
  //     처리. silent catch + console.debug 만.
  try { await logout(); } catch (e) { console.debug('[gtmux] logout failed', e); }

  // (3) window.location.href = '/auth' — 명시 reload 로 깨끗한 상태에서
  //     /auth (BE server-rendered HTML, D13) 진입. SPA-내부 navigation
  //     안 함 — 모든 inflight WS / fetch / timer 가 자연 reset.
  window.location.href = '/auth';
}
```

**불변**:
- step (1) 이 step (2) 보다 먼저 — logout fetch 가 시간 걸리면 그 사이
  visibilitychange / WS reconnect 가 hint 기반 silent attempt 트리거할 수
  있어 race 위험. hint clear 가 모든 silent 진입을 차단.
- step (3) 의 `window.location` 사용 — SvelteKit-style internal navigation
  이 아닌 *full reload*. inflight state (sessionStore / connectionStore /
  heartbeat / dispatcher subscriptions) 모두 GC.
- BE 가 자체 cookie Max-Age=0 set 하더라도 FE 가 *cookie 만료 detect 후
  자동 redirect 하지 않음* — step (3) 의 명시 redirect 가 owner.

### D10. WebSocket subprotocol 인증 정합

- WS handshake: cookie 자동 전송 (HttpOnly Secure)
- Server 는 cookie 검증 → invalid 면 close 1008 (옛 정책 그대로)
- WS 의 별도 subprotocol token 폐기 (cookie 가 단일 인증 채널)

이는 ADR-0002 §D5 (현 WS subprotocol) 의 amend — cookie 가 transport 인증의 단일 진실.

### D11. Settings API (Slice D-1 minimal, 2026-05-16 amend)

`docs/reports/0042-be-slice-d-work-package.md` §3.1/§3.2 의 FE-consumer 진실을 ADR 로 승격. FE Settings overlay 의 Debug + Behavior section 의 단일 BE 의존.

#### D11.1 Endpoint

- `GET /api/settings` → 200 + `{ build, server, behavior, auth }` snapshot
- `PATCH /api/settings` → 200 + 갱신된 snapshot (body 가 `{ "behavior": {...} }`)

둘 다 bearer 또는 cookie auth (`/api/*` middleware 와 동일).

#### D11.2 Section 분류

| Section | 내용 | Mutability |
|---|---|---|
| `build` | `sha` (`option_env!("GTMUX_BUILD_SHA")`, 기본 `"unknown"`) + `version` (`env!("CARGO_PKG_VERSION")`) + `rust` (`option_env!("GTMUX_BUILD_RUST_VERSION")`, 기본 `"unknown"`) | boot-immutable |
| `server` | `pid` (`std::process::id()`) + `bind` + `port` (from `Config`) + `log_path` (null — gtmux 는 현재 stderr only) | boot-immutable |
| `behavior` | `auto_kill_terminal_on_panel_close` (default `false`, ADR-0021 G25.1.b) | **mutable** |
| `auth` | `token_present` + `password_set` + `argon2: { m_cost_kib, t_cost, p_cost }` (D5 의 m=64MiB, t=3, p=4) | boot-immutable |

#### D11.3 PATCH 의 validation

- Body 가 `{}` 또는 빈 body → no-op + 현재 snapshot 반환 (idempotent probe)
- Boot-immutable section (`build` / `server` / `auth`) 포함 시 → 400 `{ "error": "boot_immutable", "field": "<section>" }`
- 알 수 없는 top-level key → 400 `{ "error": "unknown_field", "field": "<key>" }`
- `behavior` 안 알 수 없는 nested key → 400 `{ "error": "unknown_field", "field": "behavior.<key>" }`
- `behavior.auto_kill_terminal_on_panel_close` 의 type 불일치 → 400 `{ "error": "type_mismatch", "field": "behavior.auto_kill_terminal_on_panel_close", "expected": "bool" }`
- 성공 → 200 + 전체 snapshot

#### D11.4 Persistence

**In-memory only** (Stage 7 minimal). Server 재기동 시 default 로 reset. Disk 영속화는 follow-up (config file write 또는 `<workspace>/settings.json` — 별 결정).

#### D11.5 구현 위치

- `crates/http-api/src/settings.rs` — handler + types + tests (8 case)
- `crates/http-api/src/lib.rs` — `AppState.behavior_settings: Arc<RwLock<BehaviorSettings>>` 필드 + `/api/settings` route (GET + PATCH)

#### D11.6 후속

- D-3 (Auth Stage 7) 후 `auth.password_set = false` 시 FE 가 `POST /api/settings/password` flow 노출. 본 ADR 의 D11.2 의 `auth` snapshot 의 `password_set` 가 그 분기 입력.
- Disk 영속화 결정 시 별 D 또는 ADR amend.

## 대안 검토

### A1. Token only (cookie 없이 매번 URL token)
**거부.** 사용자 명시 "매번 새롭게 인증 X" 와 정면 반대.

### A2. Password only (token 폐기)
**거부.** 사용자 명시 "token or password". token mode 는 *CLI 도구로 자동화* 측면에서 가치 (스크립트가 token 으로 진입 가능).

### A3. Cookie 영속 storage (sqlite)
**거부 (MVP), P1+ 검토.** in-memory map 으로 시작 — 재기동 시 재인증은 보안 측면에서 오히려 안전.

### A4. JWT (HMAC 또는 RS256)
**거부.** Single-user 환경에서 stateless token 의 이점 (cross-service scale) 낮음. server-side session table 이 단순.

### A5. SSO / OAuth
**거부.** Sketch §3 의 비범위 (multi-user / 외부 IdP 의무 없음).

## 영향

### Code
- **Backend**:
  - Auth handler 신규 (axum middleware)
    - `GET /auth` (Auth page render)
    - `POST /auth/login` (token or password verify, cookie set)
    - `POST /auth/logout` (cookie clear)
  - Server-side session table (in-memory `HashMap<token, AuthSession>`)
  - Argon2id verify (password mode)
  - Rate limiter (token bucket 또는 sliding window)
  - WS handshake 의 cookie 검증
  - CLI `gtmux set-password` 액션
- **Frontend**:
  - `/auth` route (Svelte page)
  - Login form (token mode = 보통 URL query, password mode = input field)
  - Auth dialog (D6, ADR-0019 D8)
  - Logout 액션 (SessionMenu)
  - Settings → Change password (P1+)
  - Bootstrap inline script 폐기 — server-side redirect 흐름

### Config
- `~/.config/gtmux/config.toml`:
  ```toml
  [auth]
  mode = "token"                  # or "password"
  cookie_max_age_days = 7
  rate_limit_per_5min = 5
  ```

### ADR
- ADR-0003 (Security defaults) amend — auth section 폐기 + 본 ADR 으로 redirect
- ADR-0002 §D5 amend — WS subprotocol 인증 폐기, cookie 가 단일

### Docs
- `docs/ssot/security-defaults.md` 큰 amend — auth section 본 ADR 으로
- CLAUDE.md 의 security 정책 항목 amend
- plan-0007 의 Stage 1 (Auth + Dialog) 의 진실

### 보안
- Argon2id 매개변수의 calibration (P1+, machine-specific)
- Rate limit 의 IP 기반이 reverse proxy 뒤에서 무의미할 수 있음 — `X-Forwarded-For` trust 정책 추가 (P1+)
- Cookie domain isolation (localhost 만 / `127.0.0.1` 별 cookie 분리)
- HTTPS 의무화 (production), localhost 만 HTTP 허용
- Token rotation 시 옛 cookie 의 즉시 무효화 (대비 race)

### D13. `/auth` page HTML = FE SPA bundle 단일 source (2026-05-16 amend ③)

#### 맥락

D8 (bootstrap inline script 폐기) + D4/D5 (token/password mode) 의 *page HTML
자체* 는 *암묵* 으로 BE server-rendered (auth.rs:408 `auth_page_handler`) 였다.
한편 FE 는 `routes/auth/+page.svelte` (ref/frontend-design/auth.html 디자인
차용) 를 *demo only* 로 보존 (main.ts 의 `pickPage` 가 `/auth-preview` 만
라우팅). 두 진실이 분기 — 디자인 변경 시 두 곳 sync 부담.

본 amend 는 **FE SPA bundle 의 AuthPage 를 단일 source** 로 승격. BE 는
`/auth` 를 *SPA fallback (index.html + bundle 정적 asset)* 으로만 응답.

#### 결정

- **BE**:
  - `crates/http-api/src/auth.rs` 의 `auth_page_handler` 제거 (또는 `unused`
    유지 후 후속 clean — MVP 는 제거).
  - `crates/http-api/src/lib.rs` 의 `.route("/auth", get(auth::auth_page_handler))`
    제거. `/auth` 는 SPA fallback (정적 asset router) 으로 자연 진입 → index.html
    반환 → FE bundle 의 `pickPage('/auth')` 평가.
  - `is_auth_path` allow-list 의 `/auth` 는 그대로 유지 — *unauthenticated 도달
    허용* (cookie 없이 HTML/JS 정적 asset 받음). 단 그 path 가 *handler 가
    아닌 fallback 으로* 처리됨.
  - `/auth/login` / `/auth/logout` / `/auth/bootstrap` 은 변경 없음 (D4/D5/D8
    의 endpoint 진실 유지).
- **FE**:
  - `codebase/frontend/src/main.ts` 의 `pickPage` 분기에 `/auth` 도 AuthPage
    로 라우팅. 기존 `/auth-preview` alias 는 유지 (디자인 데모 진입).
  - `routes/auth/+page.svelte` 의 *demo only* 주석 정리 — *real auth path*
    명시. `login()` 호출 / `?t=` 자동 처리 / rate-limit countdown 그대로.
- **검증**:
  - cookie 없이 `/auth` GET → 200 + index.html (SPA fallback)
  - FE bundle mount → AuthPage render → form submit → `POST /auth/login` →
    Set-Cookie → `goto('/')` → AppPage 의 auth-gate 통과.
  - `?t=<token>` 자동 처리 → token mode 자동 submit → 동일 흐름.

#### 거절된 대안

- **R1.** BE server-rendered HTML 을 auth-preview 디자인으로 직접 교체 — Rust
  string template 의 inline CSS 비대 + 디자인 변경 마다 두 곳 amend. 거절.
- **R2.** FE 가 *별 path* (`/auth-preview`) 만 유지, BE-rendered 그대로 — 사용자
  요구 "기존 page 가 아닌 디자인" 정면 위반. 거절.

#### 보안 영향

- cookie-less 도달 가능 = BE-rendered 와 동일 (D5 의 *unauthenticated 도달
  허용* 정합).
- FE bundle 의 JS 가 cookie 없이 mount → AuthPage 만 render → fetch (login)
  로만 cookie 발급 trigger. CSRF/CSP 변경 0.
- bootstrap inline script 폐기 (D8) 와 본 amend 의 *SPA fallback* 정합 —
  cookie 가 단일 인증 채널.

#### 후속

- `routes/auth/+page.svelte` 의 design polish — 시안 차용 외 부분의 token
  rotation UI / password reset UI 등은 D12 의 endpoint 와 align.
- ADR-0020 D5 (Password mode) 의 *Password 변경* UI 정확한 wiring 은 본 amend
  와 분리 — Slice D-3 의 SettingsOverlay 진입점이 진실.

### D14. `POST /auth/rotate` — cookie rotation endpoint (2026-05-16 amend ④)

#### 맥락

D4 가 "Token rotation: SessionMenu 의 [Rotate token] 액션 — server 가 새 token
발행 + 옛 cookie 즉시 무효화 + 새 token URL 표시 modal" 로 *server token rotation*
의 사용자 흐름을 약속했으나, 실제 endpoint 명세는 없었음. D12 가 password rotation
+ logout-all 의 *settings 진입점* 만 ship — token-mode 사용자에게도 동등한 "지금
새로 시작" 액션이 필요.

#### 결정

- **`POST /auth/rotate`** (outer router, `/api/*` 의 bearer middleware 밖) — 인증
  은 handler 가 직접 cookie 검증. 인증 자체가 *이 endpoint 의 목적*.
- **MVP scope** = *cookie rotation only*. 서버 token (`state.token`, ?t= / bearer
  paths 에 사용) 은 본 endpoint 가 rotate 하지 않음. 서버 token rotation 은 CLI
  의 `gtmux rotate-token` 작업 (boot 시 새 token 으로 시작) — 본 ADR 의 영역
  밖. *대안 R1 거절 참조*.
- **Flow**:
  1. `extract_session_cookie(req.headers())` — 없으면 401 `session_cookie_required`.
  2. `session_table.validate(&cookie)` — 만료/취소 시 401 `invalid_session`.
     return `Option<AuthMode>` 로 active mode (Token / Password) 보존.
  3. `revoke_others(&old_cookie)` — 다른 device/tab 의 모든 session drop.
     revoked count 반환.
  4. `revoke(&old_cookie)` — 호출자 의 이전 cookie 도 drop. defence in depth —
     이전 cookie 가 captured 되었을 가능성 차단.
  5. `issue(mode)` — 새 opaque session-id mint (validate 가 돌려준 mode 그대로).
  6. `Set-Cookie: gtmux_auth=<new>; HttpOnly; SameSite=Strict; Max-Age=<config>;
     [Secure if Cloud]` + JSON body `{ ok: true, revoked_count: N }` where
     `N = revoked_others + 1` (포함: 호출자의 옛 cookie).
- **Atomic 순서 — defensive**: revoke_others → revoke_self → issue_new. mint 실패
  시에는 호출자가 새 cookie 없이 logged out 상태. FE 는 401 을 받아 `/auth` 로
  redirect — 시안 의 self-explanatory fallback.
- **Origin/Host check 정상 적용** (origin_check_middleware 의 `is_auth_path`
  allowlist 에 추가하지 않음) — `/auth/rotate` 는 authed 호출이므로 CSRF 표면이
  열림. cross-origin POST 는 origin allowlist 외 도메인이라면 거절.
- **Rate limit MVP 안 적용** — authed 호출이라 brute-force 표면 낮음. P1+ 검토.

#### 응답 shape

```json
// 200 OK
{
  "ok": true,
  "revoked_count": 2  // 호출자 옛 cookie + 다른 N-1 sessions
}
// Set-Cookie: gtmux_auth=<new-opaque>; HttpOnly; SameSite=Strict; ...

// 401
{ "error": "session_cookie_required" }  // cookie 없음
{ "error": "invalid_session" }          // cookie 만료/취소
```

#### 거절된 대안

- **R1. 서버 token (`state.token`) 도 함께 rotate** — 변경 surface 가 큼:
  `AppState.token: Arc<TokenString>` → `Arc<RwLock<TokenString>>` 변환 +
  ws-server `RouterState.token` 동기 + gtmux-cli 의 banner / token file 재저장.
  본 amend 의 의도 ("지금 새로 시작" 사용자 액션) 는 cookie rotation 으로 충분
  cover — 서버 token 은 CLI 의 명시 `rotate-token` 작업의 영역. 거절.
- **R2. `/api/auth/rotate` 위치** — `/api/*` 안 두면 bearer middleware 가 자동
  401 처리. 그러나 본 endpoint 의 응답은 *Set-Cookie* 를 emit — bearer-only
  caller (CLI 스크립트) 는 cookie 없이 401 이 자연. /auth/* 안 두는 패턴이
  D4/D5 의 login/logout 과 정합. 거절.
- **R3. revoke_others 선택 (body flag)** — D4 의 "옛 cookie 즉시 무효화" 와
  사용자 mental model ("새로 시작") 는 *전부 revoke* 의도. flag 안 둠. P1+ 에
  필요시 추가. 거절.
- **R4. revoke_all (`session_table.revoke_all()`) + 새 cookie issue 의 2 단계**
  — race window 안 다른 cookie 가 새로 issue 될 수 있음. 본 ADR 의 사용자
  mental model 과 다름. revoke_others + revoke_self + issue 가 안전. 거절.

#### 보안 영향

- caller cookie 의 *교체* 는 cookie pinning 공격 (이전 cookie 가 캡처된 상태)
  의 영향력 차단.
- 다른 device/tab 의 session 도 동시 drop — 사용자가 "분실 디바이스에서 로그아웃"
  의 효과 빠르게 누림.
- origin 체크 정상 적용 → CSRF 차단.
- handler 가 자체 401 처리 — bearer middleware 와 무관 (bearer middleware 가
  /auth/* 를 보호 안 함).

#### 검증

| 테스트 | 검증 |
|---|---|
| `auth_rotate_issues_fresh_cookie_and_revokes_old` | 200 + 새 Set-Cookie + revoked_count=1, 새 cookie 로 /api/sessions 200, 옛 cookie 로 401. |
| `auth_rotate_revokes_other_sessions_too` | 2 cookie mint 후 한쪽 rotate → revoked_count=2, 다른 cookie 도 401. |
| `auth_rotate_401_without_cookie` | cookie 없이 → 401 `session_cookie_required`, Set-Cookie 없음. |
| `auth_rotate_401_with_invalid_cookie` | unknown cookie → 401 `invalid_session`, Set-Cookie 없음. |

#### 후속

- FE SettingsOverlay 의 Auth section 에 [Rotate session] 버튼 wire — 클릭 →
  fetch `POST /auth/rotate, credentials: 'include'` → 응답 body 의 revoked_count
  표시 + 다른 device 로그아웃 toast. plan-0009 §5 design polish 의 후속.
- 서버 token rotation (R1 거절) 은 별 ADR 또는 CLI 측 작업. ADR-0026
  Phase 1/2 진입 시점에 함께 정리.

## 변경 이력

- 2026-05-15: 초안 + Accepted. plan 0006 grilling 의 Q17 합본. ADR-0003 의 auth section supersede, ADR-0002 §D5 amend.
- 2026-05-16: amend ① — D11 (Settings API minimal) 신규. Slice D-1 의 `GET/PATCH /api/settings` endpoint + 4 section 분류 (build/server/behavior/auth) + PATCH validation 정책 + Stage 7 minimal 의 in-memory persistence. `0042` §3.1/§3.2 의 FE-consumer wire 를 ADR 진실로 승격. 구현은 `crates/http-api/src/settings.rs` (337 PASS workspace 안 +8 unit test).
- 2026-05-16: amend ② — D12 (Password rotation + Logout-all API, Slice D-3) 신규:
  - `POST /api/settings/password` — D4 의 password rotation 의 endpoint 명세 (request shape, error codes, side effects, atomic ordering — verify → validate → hash → save 0600 → in-memory swap → revoke_others → caller cookie re-issue). 본 endpoint 가 D5 의 "Password 변경" UI 명세의 BE 진실.
  - `POST /api/settings/logout-all` — D4 의 "logout all" endpoint 명세 (caller cookie 보존 + 다른 cookie 모두 revoke).
  - `SessionTable::revoke_others(except)` — 신규 helper, return revoked count.
  - `AppState.password_hash` 의 type 변경: `Option<Arc<String>>` (immutable) → `Arc<RwLock<Option<String>>>` (runtime-mutable). `with_password_hash` builder 의 sync 호출 패턴 보존 (blocking_write — boot 시점만 사용).
  - `AppState.password_hash_path` 신규 — boot 시점 `default_password_hash_path()` 결과 pin (rotation 시 path 재확인 회피).
  - Password validation MVP: `len >= 8` + 영문자 1+ + 숫자 1+ (D5 의 "8자 + 영문 + 숫자"). zxcvbn 은 P2+.
  - 구현: `crates/http-api/src/settings.rs` (password_handler + logout_all_handler), `crates/http-api/src/auth.rs` (revoke_others), `bin/gtmux-cli/src/main.rs` (with_password_hash_path 등록). 검증: 368 → 375 PASS workspace 안 +7 unit test.
- 2026-05-16: amend ③ — D13 (`/auth` page = FE SPA bundle 단일 source) 신규. BE `auth_page_handler` 제거 + lib.rs route 제거 → SPA fallback 으로 자연 진입. FE main.ts `pickPage` 가 `/auth` 도 AuthPage 로 라우팅. 디자인 source-of-truth 단일화 (ref/frontend-design/auth.html). login/logout/bootstrap endpoint 변경 없음. 구현 plan: `docs/plans/0009-auth-page-fe-pivot.md`.
- 2026-05-16: amend ③ BE side land (plan-0009 §2) — `auth_page_handler` + 동반 helper (`render_auth_page` / `auth_error_html` / `html_escape` / `issue_cookie_and_redirect` / `AuthPageQuery`) 제거. lib.rs 의 `.route("/auth", get(auth::auth_page_handler))` 제거 → fallback_service 가 `/auth` GET catch → index.html. `is_auth_path` 의 `/auth` 매칭 유지 (D13 결정 — cookie 없이 SPA bundle 도달 허용). **bootstrap_handler 정합 amend**: `gtmux start` URL (`/auth/bootstrap?token=…`) 의 303 redirect target 을 `/auth?token=…` → `/auth?t=…` 로 변경 — FE AuthPage 의 magic-link `?t=` 인식 (`routes/auth/+page.svelte:51`) 과 일치. plan-0009 §2.5 의 검증: cargo test workspace 362 PASS / release build PASS. 테스트 정합: 4 obsolete server-rendered `/auth` 테스트 제거 (`auth_page_token_query_issues_cookie_and_redirects` / `auth_page_invalid_token_returns_html_error` / `auth_page_without_token_renders_landing` / `auth_redirect_target_rejects_external`), `bootstrap_legacy_route_redirects_to_auth` → `_to_fe_auth_page` rename + `?t=` 검증, `cookie_auth_works_after_login` + `auth_logout_clears_cookie_and_revokes` 의 cookie 발급 경로를 `GET /auth?token=` → `POST /auth/login` 로 변경 (D4/D5 의 endpoint 진실 정합).
- 2026-05-16: amend ④ — **D14 (`POST /auth/rotate` — cookie rotation endpoint) 신규**. handler `crates/http-api/src/auth.rs::auth_rotate_handler` — cookie validate → revoke_others + revoke_self → mint 새 cookie (validate 가 보존한 AuthMode 그대로) → Set-Cookie. lib.rs outer router 의 `/auth/login` / `/auth/logout` 옆에 `.route("/auth/rotate", post(auth_rotate_handler))` wire. `is_auth_path` 변경 0 (origin 체크 정상 적용 — authed 호출, CSRF 차단). MVP scope = cookie rotation only (서버 token 은 CLI `rotate-token` 영역, R1 거절). Response: `{ ok: true, revoked_count }` (호출자 옛 cookie + 다른 N-1 sessions). 검증: 4 신규 integration test (happy / revoke_others / 401 no-cookie / 401 invalid-cookie) — workspace 364 → 368 PASS. 후속: FE SettingsOverlay Auth section 의 [Rotate session] 버튼 wire (plan-0009 §5 design polish).
- 2026-05-18 (코드 SoT 정합 amend — state-machines.md §7.1): **D9.1 신규** — `SessionMenu.onLogout` 의 3-step FE 흐름 명시 (`SessionMenu.svelte:32-44`). (1) `sessionStorageHint.clear()` — `sessionStore.clear()` 거치지 않고 즉시 redirect 하므로 hint 명시 제거 (D5.4 reconnectGate 의 hint 기반 silent attempt 가 fresh `/auth` 진입 침해 안 하도록 — load-bearing race 차단). (2) `await logout()` (POST `/auth/logout` — Set-Cookie Max-Age=0). 실패해도 (3) 진행, silent catch + console.debug. (3) `window.location.href = '/auth'` — *full reload* (SPA internal navigation X), 모든 inflight state (sessionStore / connectionStore / heartbeat / dispatcher subscriptions) 자연 reset. 불변: step 순서 load-bearing (1 → 2 → 3); FE 가 cookie 만료 detect 후 자동 redirect 하지 *않음* (step 3 의 명시 redirect 가 owner). 짝: `docs/ssot/state-machines.md` §2 mermaid note.
- 2026-05-19: **D15 신규 — Server boot identity (Phase 1, 0074)**. `GET /api/sessions` response 에 `X-Gtmux-Server-Id` header (현 boot 의 `AppState::server_id`, UUID v4) 동봉. FE 가 `sessionStorage.gtmux_observed_server_id` 와 비교 — mismatch (= Server 재시작 후 stale tab) 시 `sessionStore.clear()` + `reconnectGate.cancel()` + `sessionStorageHint.clear()` + `workspaceSwitcher.open()` + warning toast. response body shape 변경 0 (header-only), 옛 FE 와 호환. *Phase 1 = FE detection only* — 인지 부담 차단 + UX 회복. *Phase 2* (BE per-Webpage `webpage_boot_nonce` + write-sensitive guard) 는 별 amend. 검증: `sessions_list_emits_server_id_header` (BE integration) + `pnpm check` 317 files / 0 errors. 정본 보고 = `docs/reports/0074-webpage-auth-epoch-and-stale-tab.md` §7.1 Phase 1.
