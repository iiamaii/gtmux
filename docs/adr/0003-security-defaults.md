# ADR-0003: 보안 디폴트 (single-user web app, local + cloud 모드)

- 상태: Accepted (2026-05-14, A4 게이트 통과 — `docs/reports/0009-adr-coherence-review.md`. B1 정정 (token subprotocol RFC 6455 콤마 두 값) 반영 완료. **2026-05-14 amend ×2** — (1) debug session 후속 §D3 sub-clause (CORS 합성 + loopback alias) + §D6 본문 갱신 (HttpOnly cookie 폐기, 3축 → 2축), 0022 §4/§6. (2) ADR-0013 채택 후 §D7 (argv 분리 정책의 tmux 측) 폐기 + §D8 (식별자 정규식의 tmux 측) 폐기, Session 정규식 추가, 0023 §4 + ADR-0013.)
- 일자: 2026-05-13 (Proposed) / 2026-05-14 (Accepted, amend 동일)
- 결정자: security-engineer (grill D17·D20·D21·D22 + R5 보고서 산출)
- 근거 보고서:
  - `docs/reports/0005-security-model.md` (R5, 12-item 안전한 기본값 체크리스트 정본)
  - `docs/reports/0010-grill-amendments.md` D17 (토큰 정책 closed), D20 (CLI), D21 c1/c6/c7 (first-run·port lookup·token-rotate-on-mismatch), D22 (`[security]` config + mode 자동 추론)
  - `docs/sketch.md` §13 (전체 위협 모델 + 8 위험 항목)
- 관련 ADR:
  - ADR-0007 (Server : Session : Port 1:1:1) — 토큰 스코프가 Server 단위 = Session 단위
  - ADR-0008 (single-pane + Group) — tmux command allowlist의 도메인 측 기반 (`split-window`·`resize-pane`·`select-layout` 제외)
  - ADR-0009 (tmux daemon 격리) — 소켓 perm·dir perm tmux 표준 자동 강제
  - ADR-0011 (Backend stack = Rust) — D7 redaction · D8 `ring` CSPRNG·상수시간 비교 crate 구현 입력
  - ADR-0002 (전송 계층, 작성 예정) — 본 ADR의 WS handshake 토큰 검증 흐름이 그쪽 SSoT(`wire-protocol.md`)와 정합해야 함
  - ADR-0001 (tmux 통합, 작성 예정) — 본 ADR의 argv 정책과 명령 allowlist를 입력 제약으로 상속

## 맥락

`docs/sketch.md` §13.1은 본 프로젝트의 위협 모델을 (i) 로컬 머신 브라우저 접속, (ii) 개인 클라우드 서버 본인 접속, (iii) **동일 네트워크/외부 노출 환경에서 잘못된 설정으로 접근 가능해지는 상황**으로 명시한다. §13.2는 5대 핵심 보안 원칙을, §13.3은 8개 카테고리(무단 접근·WebSocket 하이재킹·명령 주입·XSS·저장 데이터 노출·tmux socket 노출·DoS·운영 설정 실수)의 대응 방향을 비-규범적 수준으로 나열한다. 본 ADR은 이 8개 카테고리를 **구현이 startup에 읽는 단일 SSoT(`docs/ssot/security-defaults.md`) + 코드 레벨 강제 규칙**으로 변환한다. CLAUDE.md 불변식 #4(*"보안 디폴트는 단일 사용자라 해도 선택 사항이 아니다"*)와 grill 결정 사항을 binding으로 잠근다.

R5 보고서(`docs/reports/0005-security-model.md`)가 5개월 분량의 외부 출처(OWASP, MDN, Jupyter, code-server, Cockpit, Syncthing, Caddy, web.dev, xterm.js, XDG, NCC Group)를 종합해 **12-item 안전한 기본값 체크리스트**를 도출했다. 본 ADR은 그 12개 항목을 *결정문으로 직접 인용*하여 ADR-규모로 잠그고, 본 grill에서 closed된 D17 토큰 정책과 reviewer flag 2개(tmux `--` end-of-options 일관성·CSP `connect-src` 모드별 정책)를 결정 (D13~D15) 형태로 추가한다.

Mode는 별도 필드가 아니라 `bind` 값에서 자동 추론된다 (D22): `bind ∈ {127.0.0.1, ::1, unix:/...}` → **local**, 그 외 → **cloud**. local 모드는 토큰 매시작 재발급(Jupyter 패턴), cloud 모드는 영속 토큰 + 명시 회전(`gtmux rotate-token --session <name>`, D20)이 기본이다. OS 인증 위임(PAM/SSH)은 단일 사용자 전제(sketch §1.3) 하에서 MVP 미적용, P1+에서 재방문 여지만 남긴다 (D17).

## 결정 (Decisions)

### D1~D12. R5 12-item 안전한 기본값 체크리스트 (정본 인용, 보고서 §"안전한 기본값 체크리스트" verbatim)

- **D1.** 기본 바인드 = `127.0.0.1:<random-high-port>` (또는 `--socket` 시 `$XDG_RUNTIME_DIR/gtmux/control.sock`, 0600). (R5 #1)
- **D2.** **모든** HTTP 요청에서 `Host` 헤더 화이트리스트(`127.0.0.1:<port>`, `localhost:<port>`, `[::1]:<port>`) 검증, 불일치 즉시 **403**. (R5 #2 — DNS rebinding 방어, Syncthing 패턴 차용)
- **D3.** **모든** WebSocket 핸드셰이크에서 `Origin` 화이트리스트(정확 일치, `null` 거부) 검증. 와일드카드/부분 일치 금지. (R5 #3 — CSWSH 방어, OWASP)
    - **[2026-05-14 amend — 디폴트 합성 + loopback alias, L-2/L-9]** `cors_origins` 가 **빈 셋** 이고 `bind` ∈ {`127.0.0.1`, `localhost`, `::1`, `unix:/...`} 이면 다음 3 origin 을 *자동 합성* 한다 (scheme = `http` 고정):
        - `http://127.0.0.1:<port>`
        - `http://localhost:<port>`
        - `http://[::1]:<port>`
        - 정당화: 사용자가 banner URL 의 `127.0.0.1` 대신 `localhost` 로 접속하는 운영 현실 + same-origin SPA fetch 자체가 빈 셋 디폴트로 차단되는 L-9 결함을 동시 해소. ADR-0003 §O3 의 "명시 우선, 미설정 시 자동 합성" 권고 정합.
        - **Cloud 정신 유지**: bind 가 위 loopback 집합 *밖* (예: `0.0.0.0`, public IP, hostname) 이면 사용자가 `cors_origins` 를 명시할 의무. 빈 셋 + non-loopback = startup fail-closed (exit 5). 0.0.0.0 은 *모든 인터페이스* catch-all 이지만 ADR-0003 §D22 mode 자동 추론 (loopback → local, 그 외 → cloud) 정합 하 *cloud 의도* 로 분류 — *2중 정체성* (loopback + cloud) 을 허용해 fail-closed 정신을 흐리지 않는다.
        - 사용자가 `cors_origins` 를 *일부* 명시한 경우엔 합성 안 함 (명시가 의도이므로 그대로 존중).
        - **현 구현**: `Config::effective_cors_origins()` 헬퍼 (`crates/config/src/lib.rs`) + `origin_check_middleware` (`crates/http-api/src/lib.rs`) — `da5c221`, `50bad9c`.
        - Result: this clause was added retroactively after the debug session 2026-05-14. See `docs/reports/0020-debug-classification.md` §2.1 + `docs/reports/0022-logic-amendment-decisions.md` §6.
- **D4.** 세션 토큰 = **256-bit CSPRNG**, base64url 인코딩, 매 서버 시작 시 재발급(local) 또는 영속(cloud), `${XDG_STATE_HOME}/gtmux/<session>.token` 파일에 0600으로 저장, **상수시간 비교**. (R5 #4, D17 통합 — ADR-0011 D8 `ring::rand::SystemRandom` + `ring::constant_time::verify_slices_are_equal`로 구현. 파일 위치는 grill D20 디렉터리 레이아웃에 정합 — R5의 `${XDG_CONFIG_HOME}` 표기는 D20에서 `${XDG_STATE_HOME}`으로 재배치됨. CONFIG는 사용자 편집 가능, STATE는 머신 발급 자료.)
- **D5.** WebSocket 토큰 전송은 **`Sec-WebSocket-Protocol` 서브프로토콜** 사용. 쿼리스트링 토큰 금지 (access log/Referer 누설, OWASP). 클라이언트는 **두 값 (`gtmux.v1`, `bearer.<base64url-token>`)** 으로 advertise하고, 서버는 `gtmux.v1`만 echo한다 (RFC 6455 §11.3.4 콤마-구분 토큰 리스트 semantics, Kubernetes PR #47740 패턴). ADR-0002 D5·R7 §5 핸들러와 동일. (R5 #5, A4 B1 정정)
- **D6.** 비-WS 상태 변경(HTTP `PUT /api/layout` 등) = **`Authorization: Bearer <token>` 헤더**(1차) + 추가로 **`Sec-Fetch-Site: same-origin`** + **Origin/Host allowlist**(2차) 검증. 두 축 모두 fail-closed. (R5 #6 + D17 통합. **2026-05-14 amend — HttpOnly cookie 항 폐기, L-4**)
    - **[2026-05-14 amend — 본문 갱신, L-4]** *(구) HttpOnly Secure cookie (2차 축)* 은 본 amend 로 **폐기**. 정당화 — gtmux 의 SPA 가 동일 token 을 `sessionStorage` 에 *반드시* 보관해야 한다 (WS `Sec-WebSocket-Protocol: gtmux.v1, bearer.<token>` 헤더는 JS 가 조립해야 하고, HttpOnly cookie 는 JS 가 읽지 못한다 — ADR-0002 D5 와 정면 충돌). 결과적으로 같은 token 이 *JS 가 읽을 수 있는* sessionStorage 에 이미 있으므로, HttpOnly cookie 의 XSS 방어 (cookie 값 은닉) 는 *circumvented* — XSS 가 발생하면 attacker 는 sessionStorage 에서 token 을 직접 읽을 수 있다. Cookie 의 실제 기여 = "존재 확인 (CSRF 방어)" 였으나, Authorization: Bearer + Sec-Fetch-Site + Origin allowlist 도 동등한 CSRF/CSWSH 방어를 제공하므로 cookie 는 *redundant defense-in-depth* 였음. 단순화 = 코드 표면 ↓ + 이중-진실 sync 책임 제거.
    - **Bootstrap landing inline-script 패턴** (L-4 동반): `/auth/bootstrap?token=<one-shot-token>` 응답은 minimal HTML 을 반환하여 inline-script 가 token 을 `sessionStorage.setItem('gtmux.token', '<token>')` 한 뒤 `/` 로 redirect. 응답 헤더는 `Cache-Control: no-store` 강제. token 문자열 내부의 `</` 시퀀스는 `<\/` 로 JS escape 하여 inline-script termination 보호. 첫 부팅 banner URL 의 query token 노출 표면은 *one-shot 즉시 sessionStorage transit + path redirect* 로 제거. 사용자는 직후 *path 만* 북마크.
    - **현 구현**: `crates/http-api/src/lib.rs` 의 bootstrap 핸들러 (`dea7c13`) — Set-Cookie 헤더 제거 의무 (본 amend 로 후속 cleanup).
    - **Authorization 검증 흐름** (요약):
      1. SPA 가 sessionStorage 의 token 을 모든 보호 라우트 요청에 `Authorization: Bearer <token>` 헤더로 첨가.
      2. 서버 미들웨어가 token 을 `ring::constant_time::verify_slices_are_equal` 로 상수시간 비교 (D13.4).
      3. `Sec-Fetch-Site: same-origin` 헤더 부재/불일치 시 403.
      4. `Origin` / `Host` 헤더가 §D3 의 allowlist (자동 합성 또는 명시) 와 정확 일치하지 않으면 403.
    - Result: this clause body was rewritten retroactively after the debug session 2026-05-14. See `docs/reports/0020-debug-classification.md` §2.3 + `docs/reports/0022-logic-amendment-decisions.md` §4.
- **D7.** **[2026-05-14 amend — ADR-0013 채택]** tmux 호출 자체가 사라짐 — 본 결정의 *tmux 측 부분* 폐기. 대체 모델: child process spawn 은 `portable_pty::CommandBuilder::new(&shell)` + `pair.slave.spawn_command(cmd)` 의 단일 경로 (ADR-0013 D2). argv 배열 분리 정신은 그대로 유지 (셸 미경유, `CommandBuilder::arg` 만 사용). (구) `tmux -L gtmux-<session>` 전용 소켓 + ADR-0008 allowlist 표 + `split-window`/`resize-pane`/`select-layout`/`if-shell`/`run-shell`/`source-file`/`pipe-pane` 발급 금지는 *moot* — 우리가 그런 명령 어휘를 발급하지 않음 (ADR-0013 D12: API command schema enum 이 allowlist 역할 자동). (R5 #7 + ADR-0008 amend 통합)
- **D8.** **[2026-05-14 amend — ADR-0013 채택]** tmux 식별자 정규식 (세션/윈도우/페인 ID, 세션/윈도우 이름) 폐기 — tmux 가 없으므로 무의미. *살아남는 정규식* :
  - **Pane ID (web-측)**: `^p[0-9a-zA-Z]{1,32}$` (canvas-layout-schema §1 정합) — 우리 측 Panel 식별자
  - **Group ID**: `^g[0-9a-zA-Z]{1,32}$` (canvas-layout-schema §1 정합)
  - **Session 식별자 (logical, ADR-0007 D1 amend)**: `^[a-zA-Z0-9][a-zA-Z0-9._-]{0,62}$` — 파일명 안전 문자만 (XDG 디렉터리의 `<session>.token` / `<session>.layout.json` 파일명 키로 사용되므로 path traversal / control char 방지)
  - Panel label / note 같은 free-form 사용자 입력은 *server-side 저장 전 escape* (HTML entity encode + length cap 1024) — 렌더 시 `dangerouslySetInnerHTML` / `{@html}` 금지 (D10) 와 합성 defense.
  - (구) `-F` 포맷 문자열 정책은 *moot* — `-F` 가 tmux 명령 인자였음, 우리가 그런 명령을 발급하지 않음. (R5 #8 일부 폐기)
- **D9.** xterm.js 옵션: `allowProposedApi=false`(기본 유지), 명시적 `linkHandler` 등록으로 OSC 8 하이퍼링크는 **`http(s)`만 허용**(`allowNonHttpProtocols=false`) + Ctrl/Cmd 모디파이어 요구 + hover 시 전체 URL 표시. **OSC 52 클립보드 쓰기는 비활성** (auto-enable 금지 — 명시 사용자 동의 시에만 활성). `scrollback=1000` 기본. (R5 #9)
- **D10.** React/Svelte 트리 어디서도 **`dangerouslySetInnerHTML` / `{@html ...}` 사용 금지** (ESLint 규칙 + 컴파일 타임 차단). 마크다운 렌더링이 필요하면 DOMPurify로 sanitize + `javascript:`/`data:` 스킴 차단. (R5 #10)
- **D11.** Strict CSP 강제. 응답 헤더 기본값 표:

  | 헤더 | 값 | 비고 |
  |---|---|---|
  | `Content-Security-Policy` | 아래 템플릿 | nonce는 응답마다 새로 발급, `connect-src`는 모드별 |
  | `X-Content-Type-Options` | `nosniff` | MIME sniff 방지 |
  | `Referrer-Policy` | `no-referrer` | 토큰/세션 ID 누설 차단 |
  | `Cross-Origin-Opener-Policy` | `same-origin` | `window.opener` 격리 |
  | `Cross-Origin-Resource-Policy` | `same-origin` | 정적 자원 cross-origin 차단 |
  | `Permissions-Policy` | `camera=(), microphone=(), geolocation=(), interest-cohort=()` | 최소 권한 |
  | `Strict-Transport-Security` | (cloud 모드만) `max-age=31536000; includeSubDomains` | local HTTP에는 부착 금지 |

  CSP 템플릿(local):

  ```
  default-src 'none';
  script-src 'nonce-{RANDOM}' 'strict-dynamic';
  style-src 'self' 'nonce-{RANDOM}';
  img-src 'self' data:;
  font-src 'self';
  connect-src 'self';
  worker-src 'self';
  base-uri 'none';
  object-src 'none';
  frame-ancestors 'none';
  form-action 'none';
  ```

  CSP 템플릿(cloud): 위와 동일하되 `connect-src 'self' wss://<configured-host>;` (D15 참조). (R5 #11)
- **D12.** **cloud 모드 활성화 시 추가**: Caddy/nginx + ACME TLS 리버스 프록시, HSTS 부착, 신뢰 프록시 IP 화이트리스트로만 `X-Forwarded-For`/`-Proto`/`-Host` 인정, 인증 실패 레이트 리밋(분당 2회, 시간당 14회 — code-server 수준), 토큰 영속 + 명시 회전 명령(`gtmux rotate-token --session <name>`). (R5 #12 + D17 통합)

### D13. 토큰 정책 (grill D17 closed)

- **D13.1 회전 정책**:
  - **local 모드**(bind ∈ {127.0.0.1, ::1, unix:/...}): 매 서버 시작 시 token 재발급(Jupyter 패턴). 사용자 UX 완화 — 부팅 콘솔에 token이 포함된 전체 URL을 banner로 출력(D21 c1). 즐겨찾기는 path만, 토큰은 1회성 cookie 발급(D6)으로 transport한 뒤 path 북마크로 재진입(D21 c6 port-based lookup과 정합).
  - **cloud 모드**(그 외 bind): token **영속**. 회전은 명시적 CLI `gtmux rotate-token --session <name>` (D20)으로만. 회전 즉시 활성 WS·HTTP 연결은 RFC 6455 + custom **close code 4001 (token revoked)**으로 즉시 끊김, 새 URL을 stdout 출력 (D21 c1·c7).
- **D13.2 형식**: 32 byte random (`ring::rand::SystemRandom::fill`) → base64url 인코딩 → 길이 43 문자(패딩 제외).
- **D13.3 저장**: `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token`, 권한 0600. 디렉터리 권한 0700. 기동 시 파일이 더 넓은 권한이면 **거부**(fail-closed, R5 §E.1) — 사용자에게 권한 조정 안내 후 exit 5.
- **D13.4 비교**: 항상 `ring::constant_time::verify_slices_are_equal` (또는 동등 상수시간). 일반 `==` 비교 경로는 컴파일 타임 lint로 금지 (ADR-0011 D8 강제).
- **D13.5 OS 인증 위임 (PAM/SSH) — MVP 미적용**: 단일 사용자 전제(sketch §1.3)에서 자체 토큰이 충분. P1+ cloud 옵션 재방문 여지만 남김. R5 §"미해결 질문" #1·#4 본 결정으로 closed.

### D14. tmux `--` end-of-options 일관성 검증 (reviewer flag #1)

- **D14.1** ADR-0008 D2 allowlist의 발급 가능 명령 9종(`new-window`/`kill-pane`/`kill-window`/`rename-window`/`send-keys`/`refresh-client`/`capture-pane`/`list-sessions`/`list-windows`/`list-panes`)에 대해, tmux가 `--` end-of-options 구분자를 일관되게 honor하는지 **실측 검증을 R7(backend runtime) verification task에 포함**한다. 결과 표를 ADR-0001 (tmux 통합) 또는 ADR-0011 부속 절에 기록한다.
- **D14.2** **TBD — 실측 미수행 (2026-05-13 시점).** R7-T4 또는 별도 R5-followup task가 macOS/Linux × tmux 3.4+ 환경에서 9종 명령 각각에 `--` 직후 `-`-시작 위치 인자를 전달했을 때의 동작을 측정해 표로 산출해야 한다. 측정 항목:
  - 각 명령이 `--`를 옵션 종결자로 인식하는가 (위치 인자가 `-foo`여도 옵션 파싱이 멈추는가)
  - 인식하지 않는 명령이 있다면 그 명령에 한해 **strict per-command argv schema fallback** 적용 — 사용자 입력이 `-`/`--`로 시작하면 거부 또는 prefix strip (명령별로 결정).
- **D14.3 fallback 정책**: D14.2 검증이 완료될 때까지는 **방어적으로 모든 사용자 유래 위치 인자에 strict schema 적용** — `-`로 시작하는 입력은 명령별 인자 위치에 들어가기 전에 reject. 식별자 (`%<pid>`, `@<wid>`, `$<sid>`)는 D8 정규식이 이미 `-`-시작을 차단하므로 별도 처리 불요. 이름 입력(`rename-window <name>`)에서 사용자가 `-`-시작 이름을 원하면 D14.2 검증 결과에 따라 허용/거부 결정. (Staaldraad의 GNU tar `Shellwords.escape` 사례 R5 §C.2 참조 — `--` 단독 의존은 위험)
- **D14.4** SSoT(`security-defaults.md` §"argv injection guard")가 이 fallback 정책을 코드 readable form으로 보유한다.

### D15. CSP `connect-src` 모드별 정책 (reviewer flag #2)

- **D15.1 local 모드**: `connect-src 'self'`. 같은 출처(`http://127.0.0.1:<port>` 또는 `http://localhost:<port>`)의 HTTP·WebSocket만 허용. 외부 도메인 fetch/WS 전부 차단.
- **D15.2 cloud 모드**: `connect-src 'self' wss://<configured-host>`. `<configured-host>`는 `[security].host_allowlist` 첫 항목 또는 `[cloud].public_host` 명시 값(D22 향후 확장). `'self'`만으로는 TLS 종단 프록시가 다른 호스트명을 사용할 때 WS 업그레이드가 깨질 수 있으므로 *명시적으로* `wss://` 스킴 + 호스트를 허용.
- **D15.3 동적 생성**: CSP 응답 헤더는 미들웨어가 startup config(`[security]`)를 읽어 *서버 부팅 시 1회 빌드*하여 매 응답에 부착. nonce만 응답마다 새로 발급 (`ring::rand` 16 byte → base64). 사용자 입력에 의한 동적 부분은 없음.
- **D15.4 검증**: SSoT의 `csp_template_local` / `csp_template_cloud` 두 키가 정본. 구현은 `<configured-host>` placeholder를 startup에 1회 치환한 결과를 캐싱.

### 명시적 거부 항목 (Reject)

- **R(rej)1. tmux의 셸 경유 호출** — `/bin/sh -c "tmux ..."` 형태 절대 금지. argv 분리(D7)만 허용. (R5 §C.1)
- **R(rej)2. 쿼리스트링 토큰** — `wss://host/ws?token=...`, `http://host/api/...?token=...` 등 *ongoing auth*에서 쿼리스트링 토큰 거부 (access log·Referer 누설, OWASP). 토큰은 `Sec-WebSocket-Protocol`(D5) 또는 `Authorization: Bearer`(D6)로만.

  **예외 (D17 c1 bootstrap exchange, 2026-05-14 amend — L-4)**: 첫 부팅 콘솔 URL `http://localhost:<port>/auth/bootstrap?token=<token>`은 *일회용 sessionStorage transit* 전용 엔드포인트. 서버는 이 URL을 받으면 (i) token 검증 → (ii) minimal HTML 응답 (`Cache-Control: no-store`, `</` → `<\/` JS escape) 반환 → (iii) inline-script 가 `sessionStorage.setItem('gtmux.token', '<token>')` 수행 → (iv) 즉시 `/` 리다이렉트하고 access log redaction 미들웨어(D9)가 query를 `***REDACTED***`로 마스킹. 이후 모든 통신은 sessionStorage 의 token 을 SPA 가 `Authorization: Bearer`(HTTP) 또는 `Sec-WebSocket-Protocol`(WS) 헤더로 명시 송신. (구) HttpOnly cookie 발급은 **본 amend 로 폐기** (D6 amend 참조). 사용자는 *path 만* 북마크하므로 URL 재사용 표면이 없음. Jupyter `/login?token=...` 패턴 정신은 유지하되 cookie 대신 sessionStorage 매개. (R5 §B.1 + D17, A4 C4 surface)
- **R(rej)3. 와일드카드 Origin** — `Access-Control-Allow-Origin: *`, `Origin: null` 모두 거부. fail-closed 정확 일치 화이트리스트(D3).
- **R(rej)4. `dangerouslySetInnerHTML` / `{@html ...}`** — 어느 라이브러리에서도 금지(D10). 마크다운은 DOMPurify로만.
- **R(rej)5. OSC 52 자동 활성화** — 원격 프로그램의 클립보드 쓰기는 "paste → 셸 명령" 공격 표면이므로 auto-enable 금지. 명시 사용자 동의 시에만 활성(D9, 향후 옵션). (R5 §D.3)
- **R(rej)6. 루트 실행** — EUID==0이면 부팅 시 stderr 경고 후 **명시적 `--allow-root` 플래그 없이는 거부** (exit 5). 사용자의 다른 tmux 세션과 권한이 섞이는 위험. (R5 §E.2)

## 거절된 대안 (Rejected)

- **R1. 쿠키 단독 인증 (`HttpOnly` + `SameSite=Strict`만으로 충분)** — CSWSH가 직접 표면화됨. WebSocket은 same-origin policy 보호 밖이라 악성 사이트가 사용자 브라우저로 `new WebSocket(...)`을 호출해 쿠키-인증 세션을 가로챌 수 있음 (R5 §B.3, OWASP). 토큰 + Origin 검증의 이중 방어를 유지한다.
- **R2. 매 요청 토큰 회전 (rolling token)** — Jupyter도 single-user는 매 *서버 시작* 회전으로 충분하며, 매 요청 회전은 멀티 탭(MT-3 D13)과 충돌(한 탭이 새 토큰을 받으면 다른 탭이 즉시 끊김). 단일 사용자 + 단일 Session UX에 과도. (R5 §B.5)
- **R3. PAM/SSH 위임 (OS 인증)** — 단일 사용자 전제에서 자체 토큰이 충분(D13.5). cloud 모드에서 *언젠가* 필요할 수 있으나 MVP 범위 밖. P1+ 재방문. (D17, R5 §"미해결 질문" #4)
- **R4. Tailscale/WireGuard 오버레이 의무화** — 추가 인증된 L3로 매력적이나 사용자에게 의존성을 강제. local 모드는 `127.0.0.1`로 충분하고, cloud 모드는 Caddy/nginx + ACME가 표준(R5 §A.3·A.4). 사용자가 *원하면* 사용 가능하지만 gtmux가 강제하지 않음.
- **R5. 유닉스 소켓 1차 모드 (TCP 보조 노출 없음)** — 브라우저가 AF_UNIX에 직접 말하지 못해 (R5 §A.2) socat/Caddy 의존이 필수가 됨. MVP는 `127.0.0.1` TCP를 1차로, unix socket을 보조 옵션으로(D1). 유닉스 소켓 단독 모드는 P1+로 deferred.
- **R6. CSP의 `unsafe-inline`/`unsafe-eval` 허용** — 모던 브라우저는 nonce/hash가 있으면 `unsafe-inline`을 무시(R5 §D.5) — 형식적으로만 포함된 fallback일 뿐. gtmux는 `'strict-dynamic'`로 충분하므로 둘 다 제거.

## 결과 (Consequences)

- **긍정**
  - 12개 체크리스트 항목 + 2개 reviewer flag + D17 토큰 정책이 단일 ADR + 단일 SSoT(`docs/ssot/security-defaults.md`)로 통합되어, 구현체가 한 번에 로딩 가능.
  - `bind` 값 1개로 mode가 자동 추론(D22)되어 사용자가 별도 `--mode local|cloud` 플래그를 기억할 필요 없음. 잘못 설정해도 외부 bind는 cloud 정책(레이트 리밋·TLS·HSTS)이 자동 활성화.
  - argv 분리 + allowlist + `--` fallback(D14) + 식별자 정규식(D8)의 4중 방어로 명령 주입 표면이 *코드 어휘* 수준에서 제거됨. ADR-0011 D4 Rust enum allowlist가 컴파일 타임에 강제.
  - 토큰 정책이 mode별로 명확 — local UX(매시작 새 URL banner)와 cloud UX(영속 토큰 + 명시 회전)가 충돌하지 않음.
  - CSP `connect-src` 모드별 분기(D15)가 cloud 모드 TLS 종단 프록시 시나리오에서 WS 업그레이드 실패를 방지.
- **부정/비용**
  - tmux `--` end-of-options 검증(D14.2)이 **TBD** — R7 또는 R5-followup task가 산출해야 본 ADR의 D14가 *Accepted*로 승격 가능. 그때까지는 fallback 정책(D14.3)이 보수적으로 동작 → `-`-시작 이름 입력은 거부.
  - cloud 모드에서 ACME TLS 리버스 프록시 설정·신뢰 프록시 IP 화이트리스트가 사용자 책임 — gtmux 본 프로젝트가 자동화하지 않음 (배포 가이드 문서로 분리).
  - local 모드 매시작 토큰 재발급은 사용자가 URL을 *path만* 북마크하고 cookie 또는 first-run banner로 토큰 transport해야 함. cookie가 만료/소거되면 `gtmux start` 콘솔에서 토큰 재확인 필요.
- **후속 작업**
  - **SSoT** `docs/ssot/security-defaults.md` 발행 (본 ADR 동반) — Rust `auth` + `config` crate가 startup에 `serde_json::from_str` 또는 `toml::from_str`로 직접 로딩.
  - **R7-T4** (`docs/reports/0007-backend-runtime.md`) — D14.2 실측 + WebSocket subprotocol 토큰 검증 hook 위치 결정.
  - **ADR-0001 (tmux 통합)** — D14 fallback 정책과 ADR-0008 allowlist 표를 입력 제약으로 본 ADR 인용.
  - **ADR-0002 (전송 계층)** — D5 `Sec-WebSocket-Protocol` echo, D6 `Authorization: Bearer` + cookie 흐름이 wire-protocol SSoT의 핸드셰이크 절과 정합 필요.
  - **`sketch.md` §13.3.2** 본문이 D17 결정문으로 갱신됨(0010 grill amendments §2 amendment list 표 — 동반 PR에 흡수).
  - **ADR-0011 D8** — `ring` 채택이 본 ADR의 D13.4·D4 상수시간 비교의 구현 입력. R7-T2가 `ring` vs `rustls`+`rand`+`subtle` 비교 후 최종 확정.

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태 / 웹 상태 분리 | **PASS** — 본 ADR이 정의하는 보안 표면은 *전송 계층 + 인증 + 입출력 검증* 으로 한정. tmux 상태(pane id, window id, session name)는 mirror-only로 정규식 검증(D8)만 거치고 web 상태(panel id `^p...`, group id `^g...`)와 별도 정규식 네임스페이스로 갈라져 있어 두 도메인 어휘가 코드 경로에서 교차하지 않는다. |
| 2 | tmux-native vs web-only 분기 | **PASS** — tmux 측 보안 = argv 분리(D7) + 명령 allowlist(ADR-0008 인용) + 식별자 정규식(D8). web 측 보안 = Origin/Host(D2·D3) + CSP(D11·D15) + xterm.js linkHandler(D9) + sanitize(D10). 두 카테고리가 서로 다른 미들웨어 chain에서 강제되며 한 카테고리의 실수가 다른 카테고리로 전파되지 않는다 (예: Origin 검증 실패는 tmux 명령 발급 전에 차단됨). |
| 3 | tmux Layout ≠ Canvas Layout | **PASS (trivially)** — 본 ADR은 보안 차원이며 layout 차원과 무관. ADR-0008(single-pane-per-window)이 layout 분리를 기계적으로 보장. 단, 본 ADR D7이 `select-layout`을 allowlist에서 영구 제외함으로써 *layout 변환 코드 경로 자체가 존재하지 않음*을 컴파일 타임에 강제하므로 불변식 #3을 우회할 코드 표면도 동시에 제거된다. |
| 4 | 보안 기본값 | **PASS (강함)** — 본 ADR이 직접 정의하는 축. 12-item 체크리스트(D1~D12) + 토큰 정책(D13) + argv injection guard(D14) + CSP `connect-src`(D15) + 6개 명시 거부(R(rej)1~6)가 모두 fail-closed로 잠금. `bind` 값에 의한 mode 자동 추론(D22)이 사용자 실수로 외부 노출되었을 때도 cloud 정책(레이트 리밋·HSTS)을 자동 활성화. ADR-0009 daemon 격리 + ADR-0011 Rust 정적 타입 enum allowlist와 다층 결합. |
| 5 | control mode 사용 | **PASS** — 본 ADR의 D7 argv 분리 정책은 ADR-0009 D2의 `tmux -L gtmux-<session> -C attach -t <session>` control mode 진입을 입력 제약으로 받으며, 셸 경유·screen scraping·반복 shell-out 경로를 R(rej)1에서 명시적으로 거부. control mode 단일 채널이 본 ADR의 명령 allowlist 강제력을 보장. |

## 미해결 항목 (Open)

- **O1. tmux `--` end-of-options 9-command 실측 표** → **R7-T4 또는 R5-followup task**. 본 ADR D14.2 TBD 표를 채우고 D14.3 fallback 정책이 어느 명령에 강제 유지되는지 확정. (현 상태: 보수적 fallback 모든 명령 적용)
- **O2. CSP nonce 발급 미들웨어의 nonce 캐싱 정책** → **R7-T4** (axum + tower-http 미들웨어 구성 절). nonce는 응답당 1회 생성이 표준이나, 부분 응답(streaming)에서의 nonce 일관성과 정적 자산(JS/CSS)의 nonce 부착 흐름은 구현 시 결정.
- **O3. `[security].host_allowlist` 자동 합성** → **ADR-0011 D6 (config crate, `figment`)**. `bind` + `port`로부터 host_allowlist 기본값(`127.0.0.1:<port>` + `localhost:<port>` + `[::1]:<port>`)을 자동 생성할지, 사용자가 명시할지 결정. 본 ADR은 *명시 우선, 미설정 시 자동 합성*을 권장(SSoT §"host_allowlist" 기본 셋 참조).
- **O4. cloud 모드 `public_host` 명시 필드** — 현재 `host_allowlist[0]`을 cloud `wss://` connect-src로 차용하나, ACME 발급 호스트와 `host_allowlist`가 분리되는 케이스(예: behind multi-host 프록시)가 발생하면 `[cloud].public_host` 명시 필드 추가 필요. → **D22 config 스키마 확장 (P1+)** 또는 본 ADR 후속 amend.
- **O5. PAM/SSH OS 인증 위임** → **P1+ cloud 옵션 재방문**. 본 MVP 범위 밖 (D13.5). 사용자가 cloud 모드를 다중 디바이스로 확장하면 재진입.
- **O6. 감사 로그 최소 범위** — `docs/sketch.md` §13.5는 "세밀한 감사 로그는 범위 밖"으로 두지만 R5 §F.3는 **최소 감사**(인증 실패, 명령 팔레트 실행, 외부 bind 활성화)를 권고. 본 ADR은 *원칙*만 ADR-0011 D7 (tracing) 미들웨어에 인계 — 구체 이벤트 set은 R7에서 확정.
- **O7. A4 정합성 리뷰 게이트** → `docs/reports/0009-adr-coherence-review.md` (또는 0011 후속)에서 본 ADR이 ADR-0002 인증 메시지 흐름과 모순 없는지 검증한 후 Accepted 승격. 본 단계는 Proposed 유지.
