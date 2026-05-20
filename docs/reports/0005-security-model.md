# 보고서: 로컬 단일 사용자 웹앱 보안 모델

작성일: 2026-05-13
연구 트랙: R5 (single-user local web-app security model)
대상 산출물: gtmux 보안 ADR 작성에 필요한 기술적 근거

## 요약 (3문장)

**로컬 모드 기본 자세**로 gtmux는 `127.0.0.1` 또는 `$XDG_RUNTIME_DIR` 하의 유닉스 도메인 소켓에 바인드하고, 매 시작 시 재발급되는 고엔트로피 토큰을 `Sec-WebSocket-Protocol` 서브프로토콜로 전달하며, 모든 HTTP/WebSocket 핸드셰이크에서 `Host` 및 `Origin` 화이트리스트 검증과 strict CSP를 강제하는 “fail-closed” 정책을 채택해야 한다.[1][2][3][8] **클라우드(개인 서버) 모드**에서는 위 로컬 기본값에 더해 Caddy/nginx 리버스 프록시로 ACME 자동 TLS 종료, `SameSite=Strict` 세션 쿠키, 인증 실패 레이트 리밋, 그리고 신뢰 가능한 `X-Forwarded-*` 헤더 명시적 허용 리스트가 필수이다.[6][14][15] 두 모드 공통으로 tmux 호출은 절대 셸을 경유하지 않고 argv 배열 + 명령별 인자 스키마 화이트리스트로 실행되어야 하며, 출력 측 xterm.js는 `allowProposedApi=false`(기본) 상태에서 명시적 `linkHandler`를 등록해 OSC 8 하이퍼링크와 비-http 프로토콜을 차단해야 한다.[7][10][11]

## 조사 범위와 질문

본 보고서는 `docs/sketch.md` §13의 위협 모델과 불변식 #4(“보안 기본값은 단일 사용자라 해도 선택 사항이 아니다”)를 코드로 옮길 때 ADR 작성자가 필요로 하는 기술적 선택지를 정리한다. 조사 질문은 다음 6개 군으로 묶었다.

1. **네트워크 바인딩**: `127.0.0.1` / 유닉스 소켓 / Tailscale / 리버스 프록시 + TLS 비교, DNS rebinding 대응, 루프백 주소의 함정.
2. **WebSocket 인증**: 핸드셰이크 시 토큰 전달 방식(쿼리스트링/서브프로토콜/쿠키), Origin 검증, CSWSH/CSRF.
3. **입력 안전성**: 셸 회피, tmux argument injection, ID/이름/포맷 문자열 검증, 명령별 화이트리스트.
4. **출력 안전성**: xterm.js OSC 시퀀스 처리, OSC 8 하이퍼링크, 비-터미널 UI(라벨/노트/팔레트)에서의 XSS, CSP.
5. **저장과 프로세스**: XDG 경로/권한, tmux 전용 소켓(`-L`), 루트 회피.
6. **헤더와 메타**: 보안 응답 헤더, 리버스 프록시 신뢰 모델, 감사 로그 최소 범위.

또한 비교 대상으로 Jupyter Server, Syncthing, Cockpit, code-server, ttyd/gotty의 보안 자세를 조사해 “단일 사용자 로컬 웹앱” 카테고리의 모범과 안티패턴을 추출했다.

## 핵심 발견

### A. 네트워크 바인딩

**A1. `127.0.0.1` 바인드는 안전한 기본값이지만 DNS rebinding 방어가 별도로 필요하다.** 공격자 도메인이 짧은 TTL로 로컬 IP로 “재바인딩”되면 브라우저는 동일 출처로 간주해 인증되지 않은 로컬 서비스에 접근한다. Deluge, plug.dj 부류 사고가 대표적이며, 방어는 (a) 서버가 모든 요청의 `Host` 헤더를 `127.0.0.1:<port>` / `localhost:<port>` 같은 명시 화이트리스트로 검증, (b) 모든 상태 변경 엔드포인트에 인증 토큰 강제, (c) 가능하면 HTTPS(공격자의 도메인 인증서가 일치하지 않아 재바인딩이 깨짐) 세 가지를 결합한다.[1][13] Syncthing은 동일한 위협을 인지해 GUI가 로컬에 바인드된 경우 `Host` 헤더가 `localhost` 모양인지 강제 검증하고, 이를 우회하려면 `insecureSkipHostcheck`라는 의도적 “안전 해제” 플래그를 켜야 한다.[5]

**A2. 유닉스 도메인 소켓은 브라우저가 직접 말하지 못한다.** AF_UNIX 소켓은 파일시스템 권한(0600)으로 동일 UID만 접근 가능하지만, 브라우저의 `WebSocket`/`fetch`는 TCP만 이해한다. 따라서 (i) `socat`/`nginx`/Caddy로 로컬 TCP→UNIX 프록시를 두거나, (ii) 어플리케이션 자체에서 `127.0.0.1` 루프백과 UNIX 소켓을 동시에 노출해야 한다.[12] 소켓 경로는 XDG 규약에 따라 `$XDG_RUNTIME_DIR/gtmux/control.sock`이 자연스럽다 — XDG는 해당 디렉터리에 0700, 사용자 전용 소유를 명시한다.[12]

**A3. Tailscale 등 오버레이는 “인증된 L3”를 추가로 제공한다.** ACME나 자가서명 TLS 없이도 디바이스 간 mTLS와 ACL을 활용할 수 있어 “개인 클라우드” 모드의 대안이 되지만, gtmux 자체의 인증/Origin/Host 검증을 면제해 주지는 않는다(심층 방어 유지).

**A4. 리버스 프록시 + ACME TLS는 클라우드 모드의 표준이다.** code-server는 자체 가이드에서 “SSH 포트포워딩이 최선, 인터넷 노출 시 Caddy/nginx + Let’s Encrypt”를 권장하며, 인증 없이 인터넷에 노출하면 “누군가 터미널로 머신을 탈취할 수 있다”고 명시한다.[6] Cockpit도 동일한 패턴(별도 `cockpit-tls` 종단 프록시, 또는 외부 리버스 프록시 + 명시적 신뢰 헤더 정책)을 강제한다.[15]

**A5. 루프백 주소 표기 차이.** `localhost`는 OS 리졸버에 따라 `127.0.0.1` 외의 항목(예: IPv6 `::1`, 또는 `/etc/hosts` 오버라이드)으로 해석될 수 있고, 127.0.0.0/8 전체가 루프백이라는 점을 악용한 회피(`127.0.0.1` 대신 `127.0.0.2` 등)도 존재한다. `Host` 화이트리스트는 IP 리터럴과 `localhost` 둘 다 명시하고, 그 외는 거부하는 것이 안전하다.[1]

### B. WebSocket 인증

**B1. 브라우저의 `WebSocket` 생성자는 `Authorization` 헤더를 설정할 수 없다.** `Sec-` 접두 헤더는 “forbidden header”로 분류되며 `Sec-WebSocket-Protocol`만 `WebSocket(url, protocols)` 두 번째 인자로 간접 설정 가능하다.[4] 이 때문에 사실상 토큰 전송 옵션은 세 가지로 좁혀진다.

- **(i) 쿼리스트링** (`wss://host/ws?token=...`): 구현은 단순하지만 토큰이 액세스 로그/Referer/프록시 로그에 남는다. OWASP는 “access log에 남고 redaction이 필요”하다고 명시.[2]
- **(ii) `Sec-WebSocket-Protocol`로 토큰 운반**: 가장 널리 쓰이는 우회. 다만 (a) 서버가 응답에 동일 서브프로토콜을 echo해야 Chrome이 핸드셰이크를 수락하므로 더미 서브프로토콜 한 개를 함께 보내야 하고(Jupyter의 사전 제안이 이 패턴), (b) MDN은 “이 헤더는 인증용이 아니지만 브라우저에서 유일하게 설정 가능한 헤더”라는 한계를 인정한다.[4][16]
- **(iii) 쿠키**: 핸드셰이크가 HTTP 업그레이드이므로 동작은 하지만 CSWSH(Cross-Site WebSocket Hijacking) 표면을 직접 노출한다. OWASP는 “쿠키에만 의존하지 말 것, `SameSite=Lax|Strict`와 `Origin` 검증, 핸드셰이크 CSRF 토큰을 결합”하도록 권고.[2]

**B2. Origin 검증은 fail-closed 화이트리스트가 정답이다.** OWASP의 코드 예시는 핸드셰이크의 `info.origin`이 명시 배열에 포함되지 않으면 즉시 거부하도록 한다. 와일드카드/부분 일치는 “bypass 사고가 잦다”고 경고.[2] `Origin: null`(파일 URL, sandboxed iframe)도 거부 대상이다.

**B3. CSWSH 방어 = `Origin` 화이트리스트 + 토큰.** WebSocket은 동일 출처 정책의 보호를 받지 않으므로, 인증을 쿠키에만 의존하면 악성 사이트가 사용자의 브라우저로 `new WebSocket(...)`을 호출해 인증된 세션을 가로챌 수 있다. 토큰을 메시지/서브프로토콜로 요구하면 공격자 페이지는 토큰을 모르므로 핸드셰이크 후의 첫 메시지에서 거절된다.[2][17]

**B4. 비-WS 엔드포인트의 CSRF.** REST/정적 자원도 (a) `SameSite=Strict` 세션 쿠키, (b) 상태 변경에 커스텀 헤더(예: `X-Gtmux-Token`) 요구(브라우저는 cross-site에서 커스텀 헤더를 자동 부착하지 못함), (c) `Sec-Fetch-Site: same-origin` 검증의 3중 방어를 권장.[17]

**B5. 토큰 형상과 비교.** 256비트 이상의 CSPRNG 난수 토큰을 base64url로 인코딩, `~/.config/gtmux/token` 파일에 0600으로 저장. 비교는 반드시 상수시간(`crypto/subtle` 또는 `subtle.timingSafeEqual`)으로 수행. **회전 정책은 “매 시작 시 재발급”이 단일 사용자 로컬에는 충분**(Jupyter도 동일)이며, 클라우드 모드에서는 영속 + 명시적 회전 명령(`gtmux token rotate`) 조합이 합리적이다.[8][18]

### C. 입력 안전성 (명령 / 인자 주입)

**C1. 셸 없는 실행이 1차 방어.** `/bin/sh -c "tmux ..."` 절대 금지. Node에서는 `child_process.spawn(cmd, args, {shell: false})`, Go에서는 `exec.Command("tmux", "send-keys", "-t", id, text)`처럼 argv 배열을 직접 전달해야 한다. 셸 메타문자(`; | & $ \``)가 활성화될 표면 자체를 없앤다.

**C2. argv 분리만으로는 부족 — “argument injection”이 남는다.** 사용자 입력이 `-`/`--`로 시작하면 tmux의 다른 옵션으로 해석될 수 있다. tmux의 `-F`(format), `-c`(target client), `-t`(target pane), `if-shell`/`run-shell`(임의 명령 실행) 등은 특히 위험하다.[9][19] Staaldraad의 사례 연구는 GNU tar에서 `Shellwords.escape`가 `-`를 이스케이프하지 않아 `--checkpoint-action=exec=` 코드 실행으로 이어졌음을 보였다.[19] 따라서 **`--` 구분자 + 명령별 인자 스키마 화이트리스트 + 식별자 정규식 검증**의 3중 정책이 필요하다.

**C3. tmux 식별자/이름 정규식.**

- 세션 ID: `^\$[0-9]+$`
- 윈도우 ID: `^@[0-9]+$`
- 페인 ID: `^%[0-9]+$`
- 이름(세션/윈도우): 인쇄 가능 문자, 제어문자 없음, `[^\x00-\x1f\x7f]{1,64}` 정도로 제한.
- **포맷 문자열 `-F`는 절대 사용자 입력을 받지 않는다** — 서버 측 상수만 허용.[9]

**C4. 명령 팔레트 = 가장 위험한 표면.** “tmux 명령을 자유 입력”하게 두면 모든 화이트리스트가 무력해진다. 권장 패턴은 `cmd → {positional, flags, validators}` 테이블 기반의 명령 디스패처(예: `new-window`, `kill-pane`, `select-window`, `rename-pane`만 노출). `if-shell`, `run-shell`, `source-file`, `pipe-pane`은 기본 비허용 목록에 포함.

### D. 출력 안전성 (렌더링 공격)

**D1. xterm.js v5+는 `allowProposedApi=false`가 기본**이다 — 실험적 API 사용을 차단한다. gtmux는 이 기본을 그대로 유지해야 한다.[7] `scrollback`은 1000(기본) 또는 메모리 압박 시 더 작게.

**D2. OSC 8 하이퍼링크.** `linkHandler`를 설정하지 않으면 xterm.js는 클릭 시 “strongly worded warning”과 함께 `window.confirm`을 띄운다. **명시적 핸들러를 등록해 (a) `http(s)`만 허용(`allowNonHttpProtocols=false`), (b) hover 시 전체 URL 표시, (c) Ctrl/Cmd 모디파이어를 요구**하는 것이 공식 권고 사항이다.[10][11]

**D3. OSC 52 (클립보드 쓰기).** 원격 프로그램이 사용자 클립보드를 임의로 덮어쓰면 “페이스트 → 셸 명령 실행” 공격이 가능하다. xterm.js는 기본적으로 OSC 52를 클립보드에 쓰지 않으며, 처리하려면 별도 애드온/패스스루 설정이 필요하다.[7] gtmux는 기본 비활성을 유지하고, 명시적 동의 시에만 활성화한다.

**D4. 터미널 외 UI의 XSS.** 페인 라벨, 노트, 세션/윈도우 이름은 모두 untrusted. React를 쓴다면 기본 이스케이프로 충분하지만 `dangerouslySetInnerHTML`는 전면 금지. 마크다운 렌더링이 필요하면 DOMPurify + `javascript:`/`data:` 스킴 차단.

**D5. Strict CSP.** OWASP/web.dev 합의된 골격은 다음과 같다.[3][20]

```
Content-Security-Policy:
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

`unsafe-inline`은 nonce/hash가 있을 때 모던 브라우저가 무시하므로 “구형 브라우저 폴백”으로만 두는 것이 안전.[3] `strict-dynamic`은 번들된 SPA에서 동적 로딩 스크립트를 신뢰 전파한다. **인라인 핸들러 금지, 모든 스크립트는 `addEventListener`로.**

### E. 저장과 프로세스

**E1. 경로와 권한.**

- 설정 디렉터리: `${XDG_CONFIG_HOME:-~/.config}/gtmux` (0700).
- 토큰: 위 디렉터리 하 `token` (0600).
- 레이아웃/상태: `${XDG_DATA_HOME:-~/.local/share}/gtmux` (0700 / 파일 0600).
- 런타임 소켓: `${XDG_RUNTIME_DIR}/gtmux/control.sock` (0600, 디렉터리 0700). XDG 명세상 `$XDG_RUNTIME_DIR` 자체가 0700 사용자 소유여야 한다.[12]

기동 시 위 권한이 더 넓으면 경고를 띄우고, 토큰 파일은 더 넓을 때 거부(fail-closed)하는 것을 권장.

**E2. 루트 회피.** EUID==0이면 부팅 시 경고 + 명시적 `--allow-root` 없이는 거부. tmux 서버를 루트로 띄우는 것은 사용자의 다른 tmux 세션과 권한이 섞이는 위험이 있다.

**E3. tmux 전용 소켓.** `tmux -L gtmux`로 별도 서버를 띄우면 `~/.tmux` 또는 `/tmp/tmux-<uid>/gtmux` 경로에 0700 디렉터리 하의 소켓 파일이 생성된다. 사용자의 기존 tmux 세션과 격리되어, gtmux의 화이트리스트 실수가 사용자 운영 환경에 침범하는 폭발 반경을 줄인다.

### F. 헤더와 메타

**F1. 응답 헤더 기본값.**

| 헤더 | 값(권장) | 비고 |
|---|---|---|
| `Content-Security-Policy` | D5의 strict CSP | nonce는 응답마다 새로 발급 |
| `X-Content-Type-Options` | `nosniff` | MIME sniff 방지 |
| `Referrer-Policy` | `no-referrer` | 토큰/세션 ID 누설 차단 |
| `Cross-Origin-Opener-Policy` | `same-origin` | window.opener 격리 |
| `Cross-Origin-Embedder-Policy` | `require-corp` | SharedArrayBuffer 필요 시만 |
| `Cross-Origin-Resource-Policy` | `same-origin` | 정적 자원 차단 |
| `Permissions-Policy` | `camera=(), microphone=(), geolocation=(), interest-cohort=()` | 최소 권한 |
| `Strict-Transport-Security` | (TLS 모드만) `max-age=31536000; includeSubDomains` | 로컬 HTTP에는 부착 금지 |

**F2. 리버스 프록시 신뢰.** 클라우드 모드에서 Caddy/nginx 종단 TLS를 채택하면 `X-Forwarded-For`/`-Proto`/`-Host`는 **신뢰 가능한 프록시에서 온 경우에만** 신뢰하도록 명시적 IP 화이트리스트가 필요하다. Cockpit 문서가 동일한 경고를 명시.[15] 자동 TLS는 Caddy(`auto_https` 기본) 또는 certbot + nginx가 표준.

**F3. 로깅과 레드액션.** 사용자가 셸에 타이핑하는 stdin에는 비밀번호/환경변수/경로가 통과한다. **stdin 페이로드는 절대 로깅하지 않음**을 정책으로 못박고, 토큰/쿠키/Authorization 헤더는 미들웨어에서 `***REDACTED***`로 치환. `docs/sketch.md` §13.5의 “세밀한 감사 로그는 범위 밖” 결정은 유지하되, **최소 감사**(인증 실패, 명령 팔레트 실행, 외부 바인드 활성화)는 남기는 것이 합리적.

**F4. 레이트 리밋.** 로컬 모드는 불필요. 클라우드 모드에서는 인증 실패에 한해 코드서버 수준(분당 2회, 시간당 14회)이 합리적 시작점.[6]

## 옵션 비교표

### 표 1. 네트워크 바인딩 옵션

| 옵션 | 공격 표면 | 브라우저 직접 접근 | DNS rebinding 노출 | 추가 인프라 | gtmux 적합도 |
|---|---|---|---|---|---|
| `127.0.0.1` (IPv4 루프백) | 동일 호스트의 다른 로컬 프로세스/탭 | 가능 | **있음** — `Host`/`Origin`/토큰 필수[1] | 없음 | **로컬 기본** |
| `[::1]` (IPv6 루프백) | 위와 동일 | 가능 | 있음, IPv6 환경에서만 | 없음 | IPv6 단독 시 |
| Unix 도메인 소켓 (`$XDG_RUNTIME_DIR/...sock`, 0600) | 동일 UID로 한정 | **불가** — 로컬 TCP→UNIX 프록시 필요 | 없음 (TCP 아님) | `socat`/Caddy 1개 추가 | 가장 강한 로컬 격리 |
| Tailscale/WireGuard 오버레이 | 동일 tailnet 디바이스 | 가능(MagicDNS) | 매우 낮음 | Tailscale 클라이언트 | 다중 디바이스 사용자 |
| 리버스 프록시 + ACME TLS | 인터넷 + 인증서 자동화 | 가능(HTTPS) | **없음** (TLS가 무력화)[1] | Caddy/nginx + ACME | **클라우드 권장** |
| `0.0.0.0`(직접 노출) | 인터넷/LAN 전체 | 가능 | 있음 | 없음 | **금지(=명시적 opt-in만)** |

### 표 2. WebSocket 핸드셰이크 토큰 전송 방식

| 방식 | 브라우저 가능성 | 로그/Referer 누설 | CSWSH 내성 | 구현 복잡도 | gtmux 권고 |
|---|---|---|---|---|---|
| 쿠키(`HttpOnly`, `SameSite=Strict`) | 가능 | 낮음 | **약함** — Origin 검증 필수, 핸드셰이크 CSRF 토큰 동반 권장[2][17] | 낮음 | 비-WS REST 세션 보조에만 |
| 쿼리스트링 `?token=...` | 가능 | **높음** — access log/Referer에 남음[2] | 보통 | 매우 낮음 | 피할 것 |
| `Sec-WebSocket-Protocol` 서브프로토콜 | 가능(브라우저에서 유일하게 설정 가능한 헤더)[4] | 낮음 (헤더 로깅 정책에 좌우) | 보통 — Origin 검증과 결합 시 강함 | 낮음 (서버가 echo 필요) | **기본 권장** |
| 첫 메시지로 토큰 송신 | 가능 | 낮음 | 강함 — 토큰 도착 전까지 모든 메시지 거부 | 보통 (상태 머신) | 클라우드 모드 보조 |
| HTTP `Authorization: Bearer` | **불가** — 브라우저 WS API가 헤더 설정 불허[4] | — | — | — | 비브라우저 클라이언트 한정 |
| TLS 클라이언트 인증서 (mTLS) | 가능(브라우저 프롬프트) | 없음 | 강함 | 높음 (인증서 발급/배포) | 클라우드 모드 옵션 |

## gtmux에의 함의 (불변식 검증 포함)

### 불변식 #4 검증

1. **“기본 바인드는 `127.0.0.1` 또는 유닉스 소켓”** — A1, A2가 직접 지지. 외부 노출은 `--bind 0.0.0.0` 같은 명시 옵션 + 기동 시 stderr 경고 + 로그 라인으로만 가능. Syncthing의 `insecureAdminAccess`/`insecureSkipHostcheck` 패턴(이름 자체에 “insecure”)을 차용 권장.[5]
2. **“WebSocket 핸드셰이크는 인증 토큰과 Origin 검사를 요구”** — B2, B3가 OWASP의 두 축(Origin 화이트리스트 + 토큰)을 그대로 옮긴 것임을 확인. 토큰은 `Sec-WebSocket-Protocol`로 전달(B1).
3. **“모든 사용자 입력은 untrusted, `dangerouslySetInnerHTML` 금지, 셸 문자열 연결 금지, allowlist된 tmux 명령만 분리된 argv로”** — C1–C4, D4가 그대로 매핑. tmux argument injection(C2)는 sketch.md가 명시하지 않은 새로운 위험으로, **ADR에 “명령별 인자 스키마” 요건 추가 필수**.
4. **“tmux 통합은 control mode”** — 본 보고서 범위 외(R5)지만, 셸 회피와 별개로 control mode 단일 채널이 화이트리스트 정책의 강제력을 보장한다는 보조 근거.

### 안전한 기본값 체크리스트 (ADR에 그대로 채택 가능)

1. 기본 바인드 = `127.0.0.1:<random-high-port>` (또는 `--socket` 시 `$XDG_RUNTIME_DIR/gtmux/control.sock`, 0600).
2. **모든** HTTP 요청에서 `Host` 헤더 화이트리스트(`127.0.0.1:<port>`, `localhost:<port>`, `[::1]:<port>`) 검증, 불일치 즉시 403.
3. **모든** WebSocket 핸드셰이크에서 `Origin` 화이트리스트(정확 일치, `null` 거부) 검증.
4. 세션 토큰은 256비트 CSPRNG, 매 서버 시작 시 재발급, `${XDG_CONFIG_HOME}/gtmux/token` 0600에 저장, 상수시간 비교.
5. WebSocket 토큰 전송은 `Sec-WebSocket-Protocol` 서브프로토콜 사용(쿼리스트링 금지).
6. 비-WS 상태 변경은 커스텀 헤더(`X-Gtmux-Token`) + `SameSite=Strict` 쿠키 이중 점검.
7. tmux 호출은 항상 `spawn(cmd, args, {shell: false})` 등 argv 배열, `tmux -L gtmux` 전용 소켓, 명령별 인자 스키마 화이트리스트(`if-shell`/`run-shell`/`source-file`/`pipe-pane` 차단).
8. 식별자/이름 정규식 검증(`^\$[0-9]+$`, `^@[0-9]+$`, `^%[0-9]+$`, 인쇄 가능 1–64자), 포맷 문자열 `-F`는 사용자 입력 금지.
9. xterm.js 옵션: `allowProposedApi=false`(기본 유지), 명시적 `linkHandler`로 OSC 8을 http(s)만 + Ctrl/Cmd 모디파이어 요구, OSC 52 클립보드 쓰기 비활성.
10. React 트리에서 `dangerouslySetInnerHTML` ESLint 차단, 마크다운은 DOMPurify로 sanitize.
11. Strict CSP (nonce + `strict-dynamic`, `object-src 'none'`, `base-uri 'none'`, `frame-ancestors 'none'`) + `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `COOP=same-origin`.
12. 클라우드 모드 활성화 시 추가: Caddy/nginx + ACME TLS, HSTS, 신뢰 프록시 IP 화이트리스트로만 `X-Forwarded-*` 인정, 인증 실패 레이트 리밋, 토큰 영속 + 명시적 회전 명령.

## 미해결 질문 / 후속 ADR 필요 항목

1. **토큰 회전 정책**: 매 시작 시 재발급(Jupyter 방식) vs 영속 + 명시적 회전. 단일 사용자 경험(브라우저 즐겨찾기 깨짐)과 보안을 어떻게 trade-off할지 결정 필요.
2. **유닉스 소켓 모드를 1차 시민으로 둘지** — 별도 로컬 프록시(socat/Caddy) 의존을 수용하는지, 아니면 항상 `127.0.0.1` 보조 리스너를 함께 노출하는지.
3. **명령 팔레트의 디폴트 화이트리스트** — “사용자에게 노출되는 tmux 명령의 정확한 목록”을 ADR에서 고정해야 한다. `if-shell`/`run-shell`/`source-file` 부재 시 어떤 사용자 시나리오가 깨지는지 검증 필요.
4. **클라우드 모드 OS 인증 통합** — code-server는 단일 비밀번호, Cockpit은 PAM/SSH 위임. gtmux가 OS 사용자 인증을 위임할지, 자체 토큰만 유지할지.
5. **OSC 52 정책** — 완전 차단 vs 사용자 명시 동의로 활성화. tmux + 원격 셸 사용자 경험을 의식.
6. **레이아웃/노트의 동기화 표면** — 향후 다중 브라우저 탭에서 동일 사용자 세션을 공유할 때 CSRF/원본 신뢰 모델이 추가로 필요한지.
7. **윈도우(MS Windows) 지원 범위** — XDG 디렉터리 명세는 *nix 중심. Windows 지원 시 `%LOCALAPPDATA%\gtmux\` + ACL 설정이 별도 결정 필요(스코프 제외일 가능성 큼).

## 출처

[1] DNS Rebinding Attacks Explained — GitHub Security Blog — https://github.blog/security/application-security/dns-rebinding-attacks-explained-the-lookup-is-coming-from-inside-the-house/ (접근: 2026-05-13)
[2] WebSocket Security Cheat Sheet — OWASP Cheat Sheet Series — https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html (접근: 2026-05-13)
[3] Content Security Policy Cheat Sheet — OWASP Cheat Sheet Series — https://cheatsheetseries.owasp.org/cheatsheets/Content_Security_Policy_Cheat_Sheet.html (접근: 2026-05-13)
[4] Sec-WebSocket-Protocol header — MDN Web Docs — https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Sec-WebSocket-Protocol (접근: 2026-05-13)
[5] Syncthing Configuration: GUI / API — Syncthing 공식 문서 — https://docs.syncthing.net/users/config.html (접근: 2026-05-13)
[6] Securely Access & Expose code-server — Coder 공식 가이드 — https://coder.com/docs/code-server/guide (접근: 2026-05-13)
[7] xterm.js ITerminalOptions API 레퍼런스 — https://xtermjs.org/docs/api/terminal/interfaces/iterminaloptions/ (접근: 2026-05-13)
[8] Security in the Jupyter Server — Jupyter Server 공식 문서 — https://jupyter-server.readthedocs.io/en/latest/operators/security.html (접근: 2026-05-13)
[9] tmux Formats wiki — tmux GitHub Wiki — https://github.com/tmux/tmux/wiki/Formats (접근: 2026-05-13)
[10] xterm.js Link Handling 가이드 — https://xtermjs.org/docs/guides/link-handling/ (접근: 2026-05-13)
[11] xterm.js Releases (v5+ allowProposedApi 기본 false) — https://github.com/xtermjs/xterm.js/releases (접근: 2026-05-13)
[12] XDG Base Directory Specification (XDG_RUNTIME_DIR 0700 요구) — https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html (접근: 2026-05-13)
[13] Singularity of Origin: DNS Rebinding Attack Framework — NCC Group — https://github.com/nccgroup/singularity (접근: 2026-05-13)
[14] Caddy Automatic HTTPS 문서 — https://caddyserver.com/docs/automatic-https (접근: 2026-05-13)
[15] cockpit-ws(8) — Cockpit Project 공식 매뉴얼 — https://cockpit-project.org/guide/latest/cockpit-ws.8 (접근: 2026-05-13)
[16] Pre-proposal: WebSocket token authentication with subprotocols — Jupyter Enhancement Proposals #119 — https://github.com/jupyter/enhancement-proposals/issues/119 (접근: 2026-05-13)
[17] Cross-Site Request Forgery Prevention Cheat Sheet — OWASP — https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html (접근: 2026-05-13)
[18] Disallow all unauthenticated web GUI access — Syncthing Issue #3357 — https://github.com/syncthing/syncthing/issues/3357 (접근: 2026-05-13)
[19] Argument injection and getting past shellwords.escape — Staaldraad — https://staaldraad.github.io/post/2019-11-24-argument-injection/ (접근: 2026-05-13)
[20] Mitigate XSS with a Strict Content Security Policy — web.dev — https://web.dev/articles/strict-csp (접근: 2026-05-13)
