# 보고서: 실시간 전송 계층 (WebSocket vs SSE vs WebTransport)

## 요약 (3문장)

gtmux의 기본 전송은 **WebSocket(RFC 6455) 단일 소켓**으로 두되, 인증 토큰은 `Sec-WebSocket-Protocol` 서브프로토콜 헤더로 전달하고 `Origin` 헤더는 서버에서 동등성 검사로 강제하여 R5(보안)와 정렬한다[1][6][10][20]. 와이어 포맷은 **이진 프레임 위의 길이 접두 + 작은 헤더 envelope**(`[1바이트 채널종류][가변길이 paneId 또는 채널ID][페이로드]`)을 권장하며, tmux 제어 채널과 패널 데이터 채널을 명확히 분리하여 불변식 #1(상태 도메인 분리)·#3(레이아웃 비혼동)을 유지한다 — Cockpit의 `<length>\n<channel-id>\n<payload>` 패턴, ttyd/gotty의 단일 바이트 opcode가 검증된 선례다[7][12][13][14]. WebTransport(HTTP/3)는 2026년 5월 기준 Safari 26.4+ 진입으로 caniuse 80% 커버리지에 도달했으나[5][16] 서버 라이브러리 성숙도와 Node 생태계가 여전히 얕고 단순 reliable 스트림이면 충분한 단일 사용자 워크로드 특성상 **MVP 비채택**, 향후 다중 동시 패널·미디어/대용량 출력 시 옵션으로 ADR에 보존한다.

## 조사 범위와 질문

R4 트랙은 gtmux가 `tmux -C`(R1 트랙 결론)와 브라우저 사이의 실시간 양방향 바이트 스트림을 어떻게 운반할지 결정하기 위한 사전 조사다. 구체적으로 다음 6개 하위 질문을 다룬다.

1. WebSocket / SSE / WebTransport / long-polling 중 어떤 것이 gtmux의 양방향·저지연·바이트 지향 요구에 적합한가.
2. WebSocket 핸드셰이크에서 인증 토큰을 어떻게 안전하게 전달하는가 (브라우저 `WebSocket` 생성자가 임의 헤더를 허용하지 않는 제약 하에서).
3. 서버에서 `Origin` 검증은 어떤 패턴으로 구현하며 흔한 함정은 무엇인가 (R5 보안 트랙과 교차).
4. 다수의 동시 pane 스트림과 tmux 명령 채널을 **하나의 소켓 위에서 어떻게 멀티플렉싱**할 것인가 (envelope 스키마 선택지).
5. tmux 측의 `refresh-client -A`/`pause-after`와 브라우저 측의 `bufferedAmount`를 어떻게 결합하여 백프레셔를 구성하는가.
6. 짧은 연결 단절 후 pane 스트림을 어떻게 재개하는가 (resume 토큰, 리플레이 버퍼, Mosh의 SSP가 주는 시사점).

소스 우선순위: RFC > WHATWG/MDN > 참조 구현(xterm.js, ttyd, gotty, terminado, Cockpit) > 라이브러리 가이드 > 블로그.

## 핵심 발견

### 1. 전송 후보 비교

**WebSocket (RFC 6455)** 은 양방향, 메시지 지향, 텍스트(opcode 0x1)와 이진(opcode 0x2) 프레임을 단일 TCP 상에서 전달한다. 프레임당 페이로드 길이는 7/16/64비트로 인코딩되므로 사실상 임의 크기 메시지를 지원하지만 실제 한도는 구현 정책에 위임된다[1]. 브라우저 지원은 2015년 7월부터 "Baseline Widely available"로 표시되어 사실상 보편적이다[2].

**SSE(EventSource)** 는 서버→클라이언트 단방향 텍스트 스트림이다. 단말 stdin은 별도 `fetch`/POST로 보내야 하므로 (a) 한 패널당 두 개의 HTTP 자원이 필요하고 (b) HTTP/1.1 동시 연결 6개 제한에 빠르게 부딪힌다[3]. 브라우저가 자동 재연결과 `Last-Event-ID`로 재시작점을 보내주는 장점은 있으나, **이진 프레임 부재**(텍스트만, base64로 ANSI 출력 부풀림)와 **양방향 결손**이 gtmux의 핵심 요구와 정면 충돌한다[3].

**WebTransport (HTTP/3 위, RFC 9220 / RFC 9297)** 는 한 세션 위에 다수의 양방향·단방향 스트림과 UDP 유사 datagram을 제공하여 멀티플렉싱·헤드오브라인 차단 회피·신뢰성/비신뢰성 혼합을 네이티브로 지원한다[5][8][9]. 2026년 5월 caniuse 기준 Chrome 97+/Edge 97+/Firefox 114+/Safari 26.4+가 지원하며 전 세계 사용량 약 80.46%다[16]. 서버 라이브러리는 Rust(`wtransport`, Salvo), 부분적으로 Go가 있으나 Node 측 서버 구현은 여전히 라이브러리화가 얕고, 브라우저 측 API 또한 비교적 새로워 운영 경험이 적다[15][17]. gtmux MVP의 단일 사용자·localhost 디폴트 시나리오에서 HTTP/3 인프라(QUIC, TLS 1.3, 인증서) 운영 부담은 정당화하기 어렵다.

**WebSocket-over-HTTP/2 (RFC 8441)** 와 **WebSocket-over-HTTP/3 (RFC 9220)** 는 `:protocol = websocket` Extended CONNECT로 단일 H2/H3 연결 위에 다수의 WS 스트림을 실어 HTTP/1.1의 "도메인당 동시 6개" 제약을 푼다[18][19]. 그러나 RFC 8441 채택은 패치 상태이며 많은 CDN/프록시가 여전히 HTTP/1.1로 WS를 터널링한다[18]. gtmux는 단일 백엔드(자체 호스트, CDN 미경유)이므로 H2/H3 WS의 이점은 작다.

**Long-polling** 은 모든 후보 중 지연이 가장 크고 양방향성을 위해 별도 POST 채널이 필요하며, 단말 키 입력 같은 빈번한 작은 메시지에는 TCP 연결 세움/끊음 비용이 누적된다. 평가 대상에서 사실상 제외된다.

결론: **MVP 기본 전송은 WebSocket(HTTP/1.1 업그레이드, RFC 6455)** 이 가장 균형 잡힌 선택이며, 코드 경계가 잘 잡혔다면 향후 같은 메시지 envelope를 WebTransport 스트림이나 H2/H3 WS로 옮기는 비용은 작다.

### 2. 인증 토큰 전달 (Sec-WebSocket-Protocol)

브라우저 `WebSocket` 생성자는 `(url, protocols)` 두 인자만 받고 `Authorization` 등 임의 헤더를 직접 설정할 방법이 없다 — WHATWG 사양은 `Upgrade`, `Connection`, `Sec-WebSocket-Key/Version/Protocol/Extensions` 모두 UA가 결정하도록 못 박았다[10][20]. 알려진 우회 옵션은 세 가지다.

1. **`Sec-WebSocket-Protocol` 서브프로토콜에 토큰을 실어 보내기** — Kubernetes API server가 채택한 패턴으로, 클라이언트는 `["gtmux.v1", "auth.bearer.<token>"]`처럼 정상 서브프로토콜과 토큰을 함께 광고하고, 서버는 검증 후 정상 서브프로토콜만 응답 헤더로 선택한다[20][21]. 헤더는 TLS로만 보호되며 평문 HTTP에서는 노출되므로 로컬호스트 외부 노출 시 TLS 필수.
2. **쿼리스트링 토큰** (`ws://127.0.0.1:.../?token=...`) — 구현은 가장 간단하지만 액세스 로그·프록시 로그에 토큰이 남고, 브라우저 referrer 정책의 영향을 받지 않더라도 운영 위험이 누적된다[20]. **MVP에서도 비권장**.
3. **연결 후 첫 메시지로 인증** — 구현 단순하지만 인증 전 핸드셰이크가 이미 성립되어 자원 점유가 발생하고, `Origin` 검증과 직교하지 않아 CSRF 표면이 줄지 않는다. tmux 명령을 실행하기 전 명시적 게이트가 필요하다[20].

쿠키 기반 인증은 동일 출처라면 자동 전송되지만, gtmux가 단일 사용자 데스크톱 도구이며 외부 노출 시 `SameSite=Lax/Strict` 만으로는 WS 핸드셰이크 CSRF를 완전히 막지 못하는 경우가 있어 `Origin` 검증과 반드시 결합되어야 한다[6][10].

**권장**: gtmux는 (a) 옵션 1을 1차 사용 — 서브프로토콜 `gtmux.v1` + `bearer.<one-shot-token>`, (b) 토큰은 메인 프로세스(혹은 CLI)가 부팅 시 OS keyring/임시 파일로 발급, (c) 첫 프레임을 받기 전 토큰 검증 실패 시 즉시 1008 `Policy Violation` 코드로 종료.

### 3. Origin 검증

RFC 6455는 "If the server does not wish to accept this connection, it MUST return an appropriate HTTP error code (e.g., 403 Forbidden)"라고 명시한다[1]. 핵심 함정은 두 가지다.

- **CheckOrigin을 비활성화하거나 와일드카드로 두는 것**. gorilla/websocket의 기본 `Upgrader.CheckOrigin == nil`은 `Origin` 호스트와 `Host` 헤더가 다르면 거부하는 "안전 디폴트"를 적용하지만, 사용자가 명시적으로 `func(r) bool { return true }`를 넣어 무력화하는 사례가 빈번하다[6]. gtmux는 **반드시 명시적 allowlist**(MVP에서는 정확히 `http://127.0.0.1:<port>`/`http://localhost:<port>` 두 값) 비교 함수를 등록한다.
- **DNS rebinding**. 공격자 통제 도메인이 짧은 TTL로 `127.0.0.1`로 재해석되면 `Origin: http://attacker.example`이 아니라 `Origin: http://127.0.0.1:<port>`가 올 수도 있다. **`Host` 헤더 화이트리스트**(정확히 `127.0.0.1:<port>` 또는 unix 소켓 경유)와 **CSRF 토큰**(서브프로토콜에 실리는 일회용 토큰)이 함께 있으면 DNS rebinding으로는 토큰을 얻을 수 없다.

WebSocket은 same-origin 정책 밖에서 동작하므로 CORS preflight가 없고, 따라서 `Origin` 검증은 서버가 직접 해야 한다 — preflight 부재가 가장 큰 보안적 함정이다[6].

### 4. 멀티플렉싱 와이어 포맷

참조 구현들이 실전에서 사용하는 스키마를 정리한다.

- **xterm.js `AttachAddon`**: 와이어 envelope 없이 WS 페이로드를 그대로 단말에 쓰고, 단말 입력을 그대로 WS로 보낸다. 단일 pane 가정. gtmux의 다중 pane 시나리오에는 부적합하다[11].
- **ttyd**: `server.h`에 정의된 단일 바이트 opcode를 메시지 첫 바이트로 둔다 — 서버→클라이언트 `OUTPUT '0'`, `SET_WINDOW_TITLE '1'`, `SET_PREFERENCES '2'`; 클라이언트→서버 `INPUT '0'`, `RESIZE_TERMINAL '1'`, `PAUSE '2'`, `RESUME '3'`, 초기 핸드셰이크는 `JSON_DATA '{'` (즉 페이로드가 `{`로 시작하면 JSON 문서로 파싱)[12]. 단일 pane 모델로 channel id는 없다.
- **gotty (webtty)**: 비슷한 패턴으로 클라→서버 `Input '1'`, `Ping '2'`, `ResizeTerminal '3'`, 서버→클라 `Output '1'`, `Pong '2'`, `SetWindowTitle '3'`, `SetPreferences '4'`[13]. 역시 단일 pane.
- **terminado (JupyterLab 터미널)**: `["stdin", "ls\n"]` / `["stdout", "..."]` / `["set_size", rows, cols]` / `["setup", ...]` / `["disconnect"]` 형태의 **JSON 배열 envelope**[14]. 가독성과 디버깅 친화성이 최고지만 단말 출력이 텍스트로 인코딩되어 base64 또는 UTF-8 escape 비용이 든다.
- **Cockpit**: `<length>\n<channel-id>\n<payload>` 라인 접두 프레임. 빈/없는 channel-id는 제어 채널을 의미하며 제어 메시지는 JSON, 페이로드 채널은 텍스트 또는 이진[7]. **다채널 멀티플렉싱이 핵심 설계 목표**라는 점에서 gtmux 요구와 가장 유사하다.

**평가**: gtmux는 (1) 다수 pane 데이터 채널, (2) 1개의 tmux 제어 채널, (3) 가능하면 1개의 웹 상태 ack 채널을 한 소켓에서 운반해야 한다. 순수 JSON envelope는 디버깅에 좋지만 단말 출력의 ANSI 시퀀스 바이트 보존을 위해 base64 또는 텍스트 escape가 필요해 대역폭이 30-40% 늘어난다. 순수 단일 바이트 opcode(ttyd/gotty)는 pane id를 가변 길이로 인코딩하기 어렵다. **권장 절충안**: 이진 WS 프레임 위에 `[1바이트 type][varint paneId 또는 0][페이로드 바이트]` envelope. 제어 메시지(`type=0x01 CTRL`)의 페이로드는 JSON으로 두어 디버깅성을 유지하고, 데이터 메시지(`type=0x02 PANE_OUT`, `0x03 PANE_IN`, `0x04 PANE_RESIZE`, `0x05 PAUSE`, `0x06 RESUME`)는 paneId만 헤더에 두고 페이로드는 ANSI 바이트 원본을 보존한다. 이 스키마는 Cockpit의 채널 식별자 패턴과 ttyd의 opcode 패턴을 절충한다[7][12][13].

WS 단일 소켓을 유지하는 이유: HTTP/1.1 위에서 pane당 소켓을 열면 도메인당 6개 동시 연결 제한[18]과 N개의 TLS 핸드셰이크 비용이 누적된다. RFC 8441 H2-WS 다중화로 회피할 수 있으나 구현·운영 복잡성 대비 이득이 작다.

### 5. 백프레셔

`yes` 또는 `cat /var/log/...`를 패널에 흘리는 시나리오에서 데이터가 폭주하면 (a) tmux의 출력 큐, (b) gtmux 서버의 사용자 공간 버퍼, (c) 커널 TCP send 버퍼, (d) 브라우저 수신 버퍼, (e) xterm.js 파서 백로그가 차례로 가득 찬다. 세 계층에서 동시에 대응한다.

- **tmux 계층**: tmux는 `pause-after=seconds` 클라이언트 플래그를 두면 pane이 `seconds`만큼 뒤처질 때 자동으로 출력을 일시중단하고 `%pause` 통지를 보낸다. 클라이언트는 `refresh-client -A '%<id>:continue'`로 재개하고, 명시적 일시중단·재개도 `-A '%<id>:pause'`/`':continue'`로 가능하다 — `off`를 보내면 그 pane은 모든 클라이언트에서 닫힐 때 tmux가 PTY 읽기를 멈춘다[4]. 화면에서 숨기거나 최소화한 패널은 `off`, 표시 중이지만 백그라운드인 패널은 `pause-after`를 사용한다.
- **전송 계층**: 브라우저 `WebSocket.send()`는 동기적이고 백프레셔 신호를 제공하지 않는다. 유일한 신호는 `bufferedAmount`로, 임계치(예: 1 MiB)를 넘으면 서버가 일시적으로 RST되거나 클라이언트가 OOM 위험에 처한다[10][22]. 권장 패턴은 (1) 서버 측에서 pane별 송신 큐에 high-watermark(예: 512 KiB)를 두어 `refresh-client -A '%id:pause'`로 tmux를 멈추고, (2) low-watermark에서 `continue`로 풀고, (3) 클라이언트 측 `bufferedAmount`는 입력(키 입력) 큐 관리에만 사용한다(단말 입력은 본질적으로 인간 속도라 폭주가 드물다). 향후 표준화되는 `WebSocketStream` API는 `WritableStream.write()`가 Promise로 백프레셔를 표현해 깔끔하지만 2026년 5월 기준 비표준·실험적 단계이므로 MVP 채택은 보류[23].
- **렌더 계층**: xterm.js에 도착한 바이트는 `requestAnimationFrame` 청크로 flush하여 메인 스레드 점유를 분산. 캔버스 밖/최소화 패널은 (a) 짧은 ring buffer(예: 64 KiB)에만 누적하고 그 이상은 drop-with-marker(`…[N bytes dropped]…`)로 표시, (b) tmux 측에서는 `refresh-client -A '%id:off'`로 PTY 읽기 자체를 멈춘다. 이 정책은 불변식 #1의 "tmux 상태 vs 웹 상태" 분리에 자연스럽게 부합한다 — "캔버스에서 숨김"은 웹 상태 결정이고, 그 결정의 부산물로 "tmux 출력 구독 해제"라는 명령이 발행된다.

Cockpit과 같은 다채널 시스템의 일반 패턴: **소스 → 페이싱**(token bucket) → **버퍼링**(bounded) → **드롭/응급**(load shedding). gtmux는 페이싱(tmux pause-after), 버퍼링(서버 큐), 드롭(off-canvas pane)을 세 도구로 갖춘다[22].

### 6. 재연결과 메시지 순서

WS는 단일 TCP 위에서 **소켓 내 순서 보존**만 보장한다 — 멀티플렉싱 시 paneA의 메시지 1이 paneB의 메시지 1보다 먼저 갔다면 그 순서는 유지되나, paneA의 메시지가 paneB보다 먼저 가야 한다는 의미는 아니다(같은 소켓이므로 자연스레 발신 순서대로 도착). 재연결 후에는 새 소켓이므로 순서는 다시 시작된다.

짧은 단절(<수 초) 후 재개 시나리오에서 **단순한 패턴**은 tmux의 `capture-pane -p -S -<N>` 또는 `display-message` 기반 스크롤백 다시 읽기로 최근 N 줄을 받아오는 것이다. 그러나 control mode 클라이언트가 끊어졌다 다시 붙으면 tmux는 해당 클라이언트의 세션 attach를 잃었기 때문에 새 클라이언트로 인식한다. 따라서 gtmux 서버 프로세스는 tmux 제어 채널을 **상주 프로세스로 유지**하고, **브라우저 WS만 재연결** 대상이 되어야 한다. 이 분리가 핵심이다.

브라우저 재연결 시 서버는 (a) 직전 시퀀스 ID를 받고, (b) pane별 **최근 64-256 KiB ring buffer**를 가지고 있다면 격차를 채워 보낸 뒤 라이브 스트림으로 전환한다. ring buffer가 부족하면 `[truncated: scrollback resume via capture-pane]` 마커와 함께 `capture-pane`으로 스크롤백을 끌어와 한 번에 보낸다.

Mosh의 SSP는 비신뢰성 UDP 위에서 **화면 상태 동기화**(byte stream이 아니라 terminal emulator state diff)와 **예측 로컬 에코**를 결합해 29% 손실 환경에서 SSH 대비 50배 빠른 응답을 시연한다[24]. gtmux는 TCP 기반 WS를 쓰고 단일 사용자·로컬 시나리오라 손실율이 0에 가까우므로 SSP의 비신뢰 UDP 이점은 사라진다. 단, **예측 로컬 에코 아이디어**는 향후 P1/P2에서 원격 노출이나 모바일 사용 시 고려할 만하다 — ADR 미해결 항목으로 보존.

## 옵션 비교표

### 표 1. 전송 후보 비교

| 후보 | 양방향성 | 이진 지원 | 멀티플렉싱 | 브라우저 지원(2026-05) | 서버 라이브러리 성숙도 | gtmux 적합성 |
|------|---------|----------|-----------|----------------------|---------------------|-------------|
| **WebSocket / RFC 6455** | O (full-duplex) | O (opcode 0x2) | 앱 계층 | Baseline widely available (2015-07+)[2] | 매우 높음 (모든 주요 언어) | **권장(MVP)** |
| **SSE (EventSource)** | X (서버→클라만) | X (텍스트 전용) | 다중 EventSource (HTTP/1.1 6개 제한) | 모든 모던 브라우저[3] | 높음 | 부적합(stdin 별도 채널 필요) |
| **WebTransport / RFC 9220+9297** | O (스트림+datagram) | O | 네이티브(스트림별) | Chrome 97+/Firefox 114+/Safari 26.4+, 글로벌 80.46%[16] | Rust 양호, Go 중간, **Node 얕음**[15] | 미래 옵션(MVP 비채택) |
| **WS-over-HTTP/2 / RFC 8441** | O | O | HTTP/2 스트림 다중화 | 패치 상태(CDN 미지원 다수)[18] | 중간 | 단일 호스트 gtmux에 이점 작음 |
| **WS-over-HTTP/3 / RFC 9220** | O | O | HTTP/3 스트림 다중화 | Chrome/Edge/Firefox 일부 | 낮음 | MVP 비채택 |
| **Long-polling** | 반(POST 별도) | 인코딩 필요 | 어려움 | 보편적 | 모두 | 지연·복잡성 모두 열위 |

### 표 2. 멀티플렉싱 envelope 비교

| 스키마 | 이진 효율 | 디버깅성 | 멀티 채널 | 참조 구현 | gtmux 적합성 |
|--------|---------|---------|----------|----------|-------------|
| **JSON envelope** (`{type,paneId,seq,payload(base64)}`) | 낮음(base64로 ~33% 부풀림) | 높음 | 자연스러움 | terminado/JupyterLab[14] | 보조(제어 채널) |
| **JSON 배열** (`["stdin", data]`) | 낮음 | 매우 높음 | 약함(채널 키 부재) | terminado[14] | 단일 채널 한정 |
| **단일 바이트 opcode + raw** (`[opcode][payload]`) | 매우 높음 | 낮음 | **없음(채널 키 부재)** | ttyd[12], gotty[13] | 단일 pane 모델에만 적합 |
| **길이 접두 + 채널 id 라인** (`<len>\n<chan>\n<payload>`) | 높음 | 중간 | **네이티브** | Cockpit[7] | 강한 후보 |
| **이진 envelope** (`[1B type][varint paneId][bytes]`) | 매우 높음 | 중간(헥스덤프) | **네이티브** | (gtmux 권장 절충) | **권장(MVP)** |
| **subprotocol/소켓 per pane** | 높음 | 중간 | 자연스러움 | (학술적, ttyd/gotty 미사용) | HTTP/1.1 6개 제한[18]으로 비권장 |

## gtmux에의 함의 (불변식 검증 포함)

**불변식 #1 (tmux 상태 vs 웹 상태 분리)**. 권장 envelope의 `type` 코드 공간을 두 구획으로 나눈다 — `0x01..0x0F`는 tmux 도메인 메시지(`CTRL`=tmux 명령 입출력, `PANE_OUT`/`PANE_IN`=pane I/O, `PANE_RESIZE`, `PAUSE`/`RESUME`, tmux 알림 미러), `0x80..0x8F`는 웹 도메인 메시지(layout ack, viewport snapshot, focus change). 두 구획은 서로 다른 코드 경로와 핸들러로 라우팅되고, **데이터 측 채널에서는 절대로 layout/geometry 같은 웹 상태가 흐르지 않으며**, 제어 측 채널에서는 tmux 명령이 흐르지 않는다. 웹 상태 영속화는 별도 HTTP 엔드포인트(`POST /layouts`)로 분리하여 WS 채널 오염을 추가로 방지한다.

**불변식 #2 (tmux-native vs web-only 기능 분기)**. envelope의 `type` 코드 자체가 분기 컴파일러 역할을 한다. 예컨대 `PANE_RESIZE`는 tmux `resize-pane`을 호출하지만 panel의 **CSS 크기**는 변하지 않는다 — 패널 시각 크기는 별도 `0x82 PANEL_GEOMETRY` 웹 메시지가 처리한다. 두 메시지 타입이 동일 핸들러를 공유하지 않게 강제함으로써 코드 수준에서 혼동을 차단한다.

**불변식 #3 (tmux 레이아웃 ≠ 캔버스 레이아웃)**. WS 스키마에 tmux의 split layout 문자열을 보내는 메시지 타입을 정의하지 않는다. 캔버스 패널 좌표는 웹 도메인 `0x82 PANEL_GEOMETRY`로만, tmux의 split은 tmux 명령(`select-layout` 등)으로만 변경한다. 메시지 타입 카탈로그를 SSOT 문서에 두어 양쪽이 같은 enum을 공유하지 못하도록 한다.

**불변식 #4 (보안 디폴트 강제)**. 본 보고서의 권장이 가장 직접적으로 검증해야 하는 항목이다. (a) **토큰 전달**: `Sec-WebSocket-Protocol: gtmux.v1, bearer.<token>` 패턴으로 쿼리스트링·평문 첫-메시지 방식을 회피한다 — 토큰은 액세스 로그에 남지 않고 브라우저 `console.log(location.href)`로도 노출되지 않는다[20][21]. (b) **Origin 검증**: 서버는 `Origin`을 `http://127.0.0.1:<port>`/`http://localhost:<port>`로 정확 비교하고 일치하지 않으면 403, gorilla/websocket을 쓴다면 `Upgrader.CheckOrigin`을 명시 등록한다[6]. (c) **메시지 스키마가 셸 문자열을 운반하지 않는다**: tmux 명령은 envelope의 `CTRL` 메시지 JSON에서 `{cmd:"new-window", args:[...]}` 같은 **argv 배열**로만 전달되며 서버는 allowlist(`new-window`, `kill-pane`, `select-pane`, `resize-pane`, `display-message`, `capture-pane` 등)에 있을 때만 tmux control mode에 명령을 발행한다 — 문자열 인터폴레이션이 메시지 경로 어디에도 없다. (d) **Pane label/note**: 사용자 입력은 웹 도메인 `0x82` 메시지로만 수신되고 tmux로 절대 흐르지 않으므로 셸 컨텍스트로 새는 경로가 원천 차단된다. (e) **localhost 기본 바인드**: WS 업그레이드 자체가 unix 소켓 또는 127.0.0.1에서만 수신되며, 외부 노출은 명시적 플래그+TLS+서브프로토콜 토큰 조합으로만 활성화된다.

**불변식 #5 (control mode 사용)**. 본 보고서는 R1 결론(control mode)을 전제하고, envelope의 `PANE_OUT`은 tmux `%output`/`%extended-output` 알림에서 1:1로 매핑된 결과를 운반한다. 화면 스크래핑·반복 shell-out 없음. `pause-after`와 `refresh-client -A`로 백프레셔를 구현하므로 P0-Stage의 핵심 코드 경로가 control mode 사용을 그대로 강제한다.

## 미해결 질문 / 후속 ADR 필요 항목

1. **envelope의 정확한 코드 표**: `type` 바이트 값과 의미를 ADR-0001(또는 SSOT 문서)에서 확정. paneId 인코딩(varint vs 고정 4바이트 uint32), 시퀀스 번호 포함 여부, 이진 vs JSON 페이로드 식별 비트.
2. **재연결 시 리플레이 정책**: pane별 ring buffer 크기(기본 64 KiB?), 미스 시 `capture-pane -p -S -<N>` fallback의 N 값, 그리고 재연결 시 클라이언트가 보내는 `last-seq`/`since` 토큰 형식.
3. **백프레셔 watermark 수치**: 서버 측 pane 송신 큐의 high/low watermark, `bufferedAmount` 모니터링 임계, off-canvas pane의 ring buffer 한도. 측정 기반 보정 필요.
4. **`Sec-WebSocket-Protocol` 다중 값 포맷 표준화**: `"gtmux.v1"`만 정상 서브프로토콜로 응답하고 토큰 값은 응답에서 빼는 패턴 vs JWT 형식. CSRF 토큰과의 결합 방식.
5. **`permessage-deflate` 사용 여부**: ANSI escape + UTF-8은 텍스트와 이진의 혼합으로, 이미 짧은 시퀀스가 많아 deflate 이득이 적고 CPU 비용은 들어간다[24]. 측정 후 결정.
6. **장래 WebTransport 마이그레이션 경로**: 동일 envelope를 WebTransport 양방향 스트림 1개에 그대로 실어 호환을 유지할 수 있는가, 아니면 pane당 스트림으로 분해할 것인가. P2 이후 결정.
7. **Mosh-style 예측 로컬 에코** 도입 여부: 단일 사용자 localhost에서는 가치가 거의 없으나 향후 원격/모바일 노출 시 키 입력 체감 지연 완화를 위해 검토.

## 출처 (URL + 접근일자)

[1] RFC 6455: The WebSocket Protocol — https://datatracker.ietf.org/doc/html/rfc6455 (접근: 2026-05-13)
[2] WebSocket — Web APIs | MDN — https://developer.mozilla.org/en-US/docs/Web/API/WebSocket (접근: 2026-05-13)
[3] Server-sent events — Web APIs | MDN — https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events (접근: 2026-05-13)
[4] tmux(1) manual page — https://man7.org/linux/man-pages/man1/tmux.1.html (접근: 2026-05-13)
[5] WebTransport — Web APIs | MDN — https://developer.mozilla.org/en-US/docs/Web/API/WebTransport (접근: 2026-05-13)
[6] gorilla/websocket package documentation (Upgrader.CheckOrigin) — https://pkg.go.dev/github.com/gorilla/websocket (접근: 2026-05-13)
[7] Cockpit Developer Guide (channels, transport) — https://cockpit-project.org/guide/latest/development.html (접근: 2026-05-13)
[8] RFC 9220: Bootstrapping WebSockets with HTTP/3 — https://datatracker.ietf.org/doc/html/rfc9220 (접근: 2026-05-13)
[9] RFC 9297: HTTP Datagrams and the Capsule Protocol — https://datatracker.ietf.org/doc/html/rfc9297 (접근: 2026-05-13)
[10] WHATWG WebSockets Living Standard — https://websockets.spec.whatwg.org/ (접근: 2026-05-13)
[11] xterm.js AttachAddon source (`addon-attach/src/AttachAddon.ts`) — https://github.com/xtermjs/xterm.js/blob/master/addons/addon-attach/src/AttachAddon.ts (접근: 2026-05-13)
[12] ttyd protocol constants (`src/server.h`, `INPUT '0'`, `OUTPUT '0'`, `JSON_DATA '{'`, etc.) — https://github.com/tsl0922/ttyd/blob/main/src/server.h (접근: 2026-05-13)
[13] gotty webtty protocol message types (`Input '1'`, `Output '1'`, `ResizeTerminal '3'`, ...) — https://github.com/yudai/gotty/blob/master/webtty/webtty.go (접근: 2026-05-13)
[14] terminado WebSocket handler (`["stdin", ...]`, `["set_size", ...]`, `["stdout", ...]`) — https://github.com/jupyter/terminado/blob/main/terminado/websocket.py (접근: 2026-05-13)
[15] wtransport — async WebTransport in Rust — https://github.com/BiagioFesta/wtransport (접근: 2026-05-13)
[16] Can I use… WebTransport API — https://caniuse.com/mdn-api_webtransport (접근: 2026-05-13)
[17] WebTransport in libp2p — https://docs.libp2p.io/concepts/transports/webtransport/ (접근: 2026-05-13)
[18] RFC 8441: Bootstrapping WebSockets with HTTP/2 — https://datatracker.ietf.org/doc/html/rfc8441 (접근: 2026-05-13)
[19] Future of WebSockets: HTTP/3, WebTransport & Beyond — https://websocket.org/guides/future-of-websockets/ (접근: 2026-05-13)
[20] Sec-WebSocket-Protocol header — HTTP | MDN — https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Sec-WebSocket-Protocol (접근: 2026-05-13)
[21] Kubernetes PR #47740: Add token authentication method for websocket browser clients — https://github.com/kubernetes/kubernetes/pull/47740 (접근: 2026-05-13)
[22] WebSocket: bufferedAmount property — Web APIs | MDN — https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/bufferedAmount (접근: 2026-05-13)
[23] WebSocketStream API — Web APIs | MDN — https://developer.mozilla.org/en-US/docs/Web/API/WebSocketStream (접근: 2026-05-13)
[24] RFC 7692: Compression Extensions for WebSocket (permessage-deflate) — https://datatracker.ietf.org/doc/html/rfc7692 (접근: 2026-05-13)
[25] Mosh (software) — Wikipedia (SSP, predictive echo, roaming) — https://en.wikipedia.org/wiki/Mosh_(software) (접근: 2026-05-13)
