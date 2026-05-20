# ADR-0002: 전송 계층 = WebSocket(RFC 6455) 단일 소켓 + 이진 envelope, 영속화는 HTTP 분리

- 상태: Accepted (2026-05-14, A4 게이트 통과 — `docs/reports/0009-adr-coherence-review.md`. **2026-05-14 amend ×2** — (1) debug session 후속 §D8 sub-clause 2건 (static-state catch-up via Pull-through-notify + frontend late-mount buffer, 0022 §2/§3), (2) ADR-0013 채택 후 envelope 구획의 *tmux-domain* → *PTY-domain* 라벨 재정의 + D7 backpressure 의 pause-after 컨셉 폐기 + D4 의 CTRL payload schema 단순화 (0023 §4 + ADR-0013 §D10).)
- 일자: 2026-05-13 (Proposed) / 2026-05-14 (Accepted, amend 동일)
- 결정자: backend-architect (배치 A2, dispatch 0002 §1 A2)
- 근거 보고서: `docs/reports/0004-transport.md` (이하 *R4*), `docs/reports/0010-grill-amendments.md` (이하 *Grill*) D12·D13·D14·D17·D19
- 관련 ADR: ADR-0001 (tmux 통합 control mode — D7 `%output` → ring buffer → binary frame 파이프라인 상속), ADR-0003 (보안 디폴트 — `Sec-WebSocket-Protocol` 토큰 정책 후속), ADR-0006 (영속화 storage — HTTP `GET/PUT /api/layout` 백킹 후속), ADR-0007 (Server:Session:Port 1:1:1 — 단일 WS endpoint), ADR-0008 (single-pane + command allowlist — argv 배열 라우팅 정본), ADR-0009 (daemon 격리 — long-lived tmux session)
- 부속 SSoT: `docs/ssot/wire-protocol.md` (이진 envelope 코드 표 32개 슬롯 전부 정의)

## 맥락

`docs/sketch.md` §10.1·§11.2 (MVP tmux 연동 + 캔버스 panel)·§13 (보안 모델)이 요구하는 *브라우저 ↔ gtmux Server ↔ tmux daemon* 양방향 저지연 바이트 스트림은 한 전송 계층이 (a) `%output` 라이브 push, (b) `send-keys`형 키 입력 echo, (c) M·I·Viewport·Focus 같은 ephemeral UI 상태의 양방향 broadcast, (d) HTTP 영속화 갱신 알림(LAYOUT_CHANGED notify), 네 종류를 모두 운반해야 한다. R4는 WebSocket·SSE·WebTransport·long-polling·H2/H3-WS 다섯 후보를 비교해 **MVP 단일 후보 = WebSocket(RFC 6455)** 을 evidence로 제시했고 (R4 §1 표 1), Grill D18에서 백엔드 스택을 Rust+axum+tokio+tokio-tungstenite로 확정함으로써 WebSocket이 *언어/런타임 차원의 제약*으로도 굳어졌다.

본 ADR은 그 결정을 단정문으로 격상하고, 동시에 다음 두 차원의 input 제약을 흡수한다 — (1) Grill D12 *T-mixed* 결정: durable한 Canvas Layout은 HTTP가 담당하고 WS는 `0x80 LAYOUT_CHANGED` notify만 운반한다, (2) Grill D14 *web-domain envelope 슬롯 0x80–0x8F 완전 정의*. 이로써 R4 §"미해결 7개" 중 envelope 정확한 코드 표·재연결 리플레이·백프레셔 watermark의 *카테고리적* 부분이 잠긴다 — 정량값(임계 KiB, 측정 후 보정)은 R7 benchmark로 미룬다.

**R4 supersession 명기 (코히런스 리뷰 G3)**: R4 §"gtmux에의 함의" §117의 단정 — *"웹 상태 영속화는 별도 HTTP 엔드포인트(`POST /layouts`)로 분리"* — 는 Grill D12에 의해 supersede되었다. 본 ADR은 `POST /layouts`가 아니라 **`PUT /api/layout` 전체 교체 + `If-Match` ETag optimistic concurrency** 모델을 채택한다. R4의 *분리 정신*(durable HTTP / ephemeral WS)은 그대로 보존되며 메서드·경로·페이로드 모델만 D12로 정정된다.

본 ADR은 ADR-0001 D7의 `%output` decode → per-pane ring buffer → binary frame 파이프라인을 *입력*으로 받고, ADR-0008 §command allowlist 표의 argv 배열 컨벤션을 *envelope의 CTRL 페이로드 형식*으로 인용한다 — 두 ADR을 다시 정의하지 않는다.

## 결정 (Decisions)

- **D1.** [R4 §1·§"옵션 비교표" 표 1] gtmux의 MVP 전송 계층은 **WebSocket(RFC 6455) 단일 소켓**이다. Server당 정확히 1개의 WS endpoint(`/ws`)를 노출한다 — pane당 소켓·subprotocol당 소켓 분리는 채택하지 않는다 (HTTP/1.1 도메인당 6개 동시 연결 제약[R4 §1 인용 RFC 8441 분석] + 다중 TLS 핸드셰이크 비용 회피). 한 Server 안의 모든 pane 출력·입력·UI broadcast는 D2의 envelope으로 다중화되어 이 단일 소켓을 흐른다.
- **D2.** [R4 §4 envelope 비교 + Cockpit/ttyd 절충] 와이어 envelope은 **이진 WebSocket 프레임(opcode 0x2) 위의 다음 고정 구조**다:
    ```
    [1B type] [varint paneId | 0] [payload bytes]
    ```
    - `type` 1바이트는 두 구획으로 분할 — 0x01–0x0F = **tmux-domain**, 0x80–0x8F = **web-domain**. 이 분할 자체가 5대 불변식 #1 (tmux 상태 vs 웹 상태 분리)을 *바이트 수준에서 강제*한다.
    - `paneId`는 *unsigned LEB128 varint*. tmux pane id `%N`에서 정수 `N`만 추출(예: `%37` → varint 37). pane이 무관한 메시지(예: 0x80 LAYOUT_CHANGED, 0x83 VIEWPORT_CHANGED)는 `paneId = 0` (= tmux는 0번 pane을 할당하지 않으므로 sentinel로 안전).
    - 페이로드는 *타입별 정의*로 형식이 다름 — 전체 32개 슬롯 정의는 SSoT `docs/ssot/wire-protocol.md` §2 표가 정본.
- **D3.** [Grill D12·D14, D14 0x80–0x8F 표 그대로] WS web-domain envelope 슬롯 **0x80–0x84는 완전 정의**, **0x85–0x8F는 reserved**.
    - `0x80 LAYOUT_CHANGED`: payload = `etag(16B raw)`. Server → 모든 WS 연결 broadcast (HTTP `PUT /api/layout` 성공 시 발생).
    - `0x81 M_CHANGED`: payload = `varint count + varint panel_ids[]`. 양방향 (클라이언트 갱신 → Server broadcast 모두 연결).
    - `0x82 I_CHANGED`: payload = `varint pane_id (0=null)`. 양방향 broadcast.
    - `0x83 VIEWPORT_CHANGED`: payload = `int32 x (LE) + int32 y (LE) + float32 zoom (IEEE-754 LE)`. 양방향 broadcast.
    - `0x84 FOCUS_MODE_CHANGED`: payload = `1B enabled (0|1) + varint target_panel_id`. 양방향 broadcast.
    - **MT-3 정책** [Grill D13]: 모든 web-domain 메시지는 **originator를 구분하지 않고 broadcast** — 송신한 연결 포함 모든 활성 WS에 동일 메시지가 도달. `client_id`/`origin_id` 같은 connection-level 식별자를 envelope에 두지 않는다. 클라이언트는 자신이 발신한 갱신도 *서버 ack로* 수신해 idempotent하게 적용한다 (MT-3 단일 진실 = Server). 이 정책으로 메시지 순서/충돌 처리 코드가 *분기 0*이 된다.
    - 자세한 페이로드 인코딩과 바이트 다이어그램은 SSoT §2 표 참조.
- **D4.** [R4 §4, ADR-0001 D7] WS tmux-domain envelope 슬롯 **0x01–0x07은 완전 정의**, **0x08–0x0F는 reserved**.
    - `0x01 CTRL`: 명령/응답 envelope. paneId = 0. payload = UTF-8 JSON `{cmd, args}` 또는 `{ok|error, ...}` 응답. **[2026-05-14 amend — ADR-0013 채택]** (구) "tmux control mode 명령 미러 + ADR-0008 allowlist argv 배열" 어휘 폐기. cmd 어휘 = **우리 API command schema** (Rust enum) — `new-pane` / `kill-pane` / `resize-pane` / `set-cwd` / `set-env` 등. enum variant 추가 = 명시적 API 확장, allowlist 자동 강제 (compile-time exhaustive match).
    - `0x02 PANE_OUT`: tmux `%output`/`%extended-output` 디코딩 후 raw bytes. paneId = `%N`의 N. payload = 원바이트(ANSI escape·UTF-8 보존, base64 없음).
    - `0x03 PANE_IN`: 키보드 입력. paneId = I(Input Target). payload = raw bytes (UTF-8/ANSI). 서버는 `send-keys -t %<id> -- <bytes>` argv 분리로 tmux에 전달 (ADR-0001 D12 + ADR-0008 allowlist).
    - `0x04 PANE_RESIZE`: paneId = N. payload = `varint cols + varint rows`. 서버는 ADR-0008 single-pane-window 컨벤션 하에서 *window-size 변경*만 (`resize-window` 또는 외부 attach 측 size 협상 mirror).
    - `0x05 PANE_PAUSE`: paneId = N. payload = 0바이트. 서버 측 *Panel Streaming State Suspended 진입* 신호 — 서버는 `refresh-client -A '%<N>:pause'`로 변환 (ADR-0001 D8, 300ms 디바운스).
    - `0x06 PANE_RESUME`: paneId = N. payload = 0바이트. `refresh-client -A '%<N>:continue'`.
    - `0x07 NOTIFY_MIRROR`: tmux의 비-`%output` 알림(`%window-*`/`%session-*`/`%pane-*`/`%layout-change`/`%subscription-changed` 등) 미러. paneId = 관련 pane이 있으면 그 N, 없으면 0. payload = UTF-8 JSON `{kind, ...}` (kind = `window-add`/`pane-died`/`layout-change`/…). 라우팅 분기는 SSoT §2.3에서 enumeration.
    - 자세한 페이로드 인코딩과 바이트 다이어그램은 SSoT §2 표 참조.
    - **[2026-05-14 amend — ADR-0013 채택, L-13]** D2 의 *구획 의미 재정의* — 0x01–0x0F slot 의 라벨이 (구) "tmux-domain" → **"PTY-domain (직접 PTY ownership)"** 으로 변환. 0x80–0x8F (web-domain) 은 그대로. *바이트 수준 분리* 와 *불변식 #1 강제* 의 구조적 역할은 유지되나 *역사적 동기* (tmux 의 외부 진실 mirror 보호) 가 *우리 측 진실 보호* 로 단순화. 자세한 의미 재정의는 ADR-0013 §D10 정본.
- **D5.** [R4 §2 + Grill D17·D18(R5 흡수)] 인증 토큰은 **`Sec-WebSocket-Protocol` 서브프로토콜 헤더**로만 전달된다 — 핸드셰이크 시 클라이언트가 `Sec-WebSocket-Protocol: gtmux.v1, bearer.<base64url-token>` 두 값을 광고하고, 서버는 토큰을 상수시간 비교한 뒤 `Sec-WebSocket-Protocol: gtmux.v1`만 응답 헤더로 선택한다 (토큰 값은 응답에서 빠짐 — R4 §2 Kubernetes PR #47740 패턴). 토큰 검증 실패 시 즉시 close code **1008 (Policy Violation)** + WS 핸드셰이크 자체 거부(HTTP 401/403도 가능, 구현은 ADR-0003 SSoT가 잠금). 토큰 발급/저장/회전 정책 전체는 ADR-0003 정본 — 본 ADR은 *전달 경로*만 잠근다.
- **D6.** [R4 §3 + Grill D22 `[security].host_allowlist`/`cors_origins`] WS 업그레이드 시점에 **`Origin`과 `Host` 헤더 둘 다 명시 allowlist 동등성 비교** — 디폴트 = config의 `cors_origins` (예: `["http://localhost:9001"]`) + `host_allowlist` (예: `["localhost:9001", "127.0.0.1:9001"]`). 일치하지 않으면 HTTP 403 + 핸드셰이크 거부. **와일드카드(`*`) 사용 영구 금지** (R4 §3 함정). `Host` 헤더 검증은 *DNS rebinding 방어* 목적이며 `Origin` 단독으로 막을 수 없는 공격 표면을 차단한다 (R4 §3).
- **D7.** [R4 §5 + ADR-0013 §D3 + Grill D19] **백프레셔는 세 계층 합성** (2026-05-14 amend — tmux 계층 폐기, *직접 master fd 제어* 로 대체):
    1. **~~tmux 계층~~** **PTY 계층 (2026-05-14 amend)**: ADR-0001 D9 의 `pause-after = 10s`/`5s` 컨셉 폐기 — tmux 가 없으므로 무의미. 대신 ADR-0013 §D3 의 *master fd 의 blocking read 가 broadcast cap 초과 시 자연 stall* + PTY kernel buffer 가 차오르면 line discipline 의 IXON/IXOFF 또는 master fd 의 epoll wait 가 shell 측 backpressure 자동 전파. visibility=hidden Panel 의 PANE_PAUSE/RESUME (0x05/0x06) 은 *broadcast subscribe drop* 으로 변환 — `refresh-client -A pause` 컨셉 폐기.
    2. **서버 큐 계층**: pane별 송신 큐 high/low watermark. **MVP 기본값 high = 512 KiB, low = 128 KiB** (R4 §5 권장). high 도달 시 서버가 broadcast send 를 정지하거나 일부 subscriber 의 lagging Receiver 를 drop (broadcast 의 자체 lag 처리). 임계값은 Sprint 7 의 50 pane × 5 burst 실측 후 재조정 — Grill D19의 p99 < 100ms·메모리 baseline < 30 MB 예산이 정량 게이트.
    3. **클라이언트 계층**: 브라우저 `WebSocket.bufferedAmount` 모니터링은 **입력(0x03 PANE_IN) 큐 관리에만** 사용 — 인간 타이핑 속도 한도라 폭주가 드물고, 출력 백프레셔는 (1)+(2)로 충분히 잡힌다. `bufferedAmount > 256 KiB` 도달 시 클라이언트가 키 입력 일시 차단 + 상단 배너 표시. `WebSocketStream` API는 표준 미정착이라 MVP 비채택 (R4 §5 결론).
- **D8.** [R4 §6 + Grill D15 + ADR-0001 D7 + ADR-0009] **재연결 모델은 "backend long-lived, browser reconnects"**.
    - tmux daemon은 **gtmux Server 종료와 독립적**으로 살아남고 (ADR-0009 D5, Grill D21·c5), tmux 측 control mode 클라이언트는 Server 프로세스 안에 정확히 1개로 *상주* (ADR-0001 D11).
    - 브라우저 WS만 재연결 대상. 클라이언트는 exponential backoff `0.5→1→2→4→8→16→cap 30s` indefinite retry (Grill D21·c3).
    - 재연결 성공 시 *full state re-sync* 절차:
        1. HTTP `GET /api/layout`으로 durable Canvas Layout 가져오기 (ADR-0006 SSoT).
        2. WS attach + 인증 (D5).
        3. 서버가 **per-pane ring buffer (128 KB 기본, Grill D15)를 0x02 PANE_OUT 프레임으로 즉시 replay** — 그 후 live `%output` 스트림으로 자연 전환 (ADR-0001 D7 ring buffer 인용).
        4. 서버가 현재 M/I/Viewport/Focus mode 단일 진실을 0x81–0x84 envelope으로 push (재연결 시점의 단일 broadcast).
    - **Resume token / sequence number 미사용** — D11 참조 (정당화).
    - **[2026-05-14 amend — static-state catch-up via Pull-through-notify, L-3]** tmux 측 *정적 state* (Pane 존재성·이름·dead 여부 등 `%output` 이 아닌 모든 한 번-emit 알림) 의 catch-up 은 **별도 mirror cache 가 아니라 layout 의 Pull-through-notify 사이클로 흡수** 한다:
        1. backend 의 tmux event listener 가 `%window-add` / `%pane-add` / `%window-renamed` 수신.
        2. backend 가 자동 Panel 을 layout 에 append (Pane ↔ Panel 1:1 auto-mount, CONTEXT.md §"Placement principle" 정합).
        3. `PUT /api/layout` 자체-호출로 server-side layout 갱신 (또는 등가 internal commit) + `LAYOUT_CHANGED` 0x80 broadcast 발행.
        4. 새/기존 WS subscriber 는 LAYOUT_CHANGED 수신 시 `GET /api/layout` 으로 canonical state 확보. 첫 attach 시점에도 동일 (D8 step 1 절차).
        - Hub 는 **`last_session` 만 lightweight cache** (catch-up replay 대상 = session_id 1개). windows / panes view 는 *layout 자체* 가 진실. mux-mirror snapshot envelope (`0x07 NOTIFY_MIRROR { kind: state-snapshot }`) 는 도입하지 않는다.
        - 정당화: 두 진실 (mux-mirror + layout) 을 동시에 유지하면 sync 책임이 코드 두 곳으로 분기 → 본 세션의 L-3 결함 (broadcast::Sender 가 late subscriber 미배달) 클래스가 layout 측에 재발할 가능성. Layout 단일 진실 + Pull-through-notify 가 *불변식 #1* (tmux 상태 vs 웹 상태 분리) 의 *기계적 강제* 측면에서도 더 일관.
        - Result: this clause was added retroactively after the debug session 2026-05-14. See `docs/reports/0020-debug-classification.md` §2.2 + `docs/reports/0022-logic-amendment-decisions.md` §2.
    - **[2026-05-14 amend — frontend late-mount buffer, L-12]** 런타임 중 새 Pane 이 추가되면 backend 의 첫 `0x02 PANE_OUT` burst (prompt + welcome 메시지 등 수백 byte ~ 수 KiB) 가 frontend 의 XtermHost 마운트보다 *앞서* 도착할 수 있다 (PUT → LAYOUT_CHANGED → GET → panelsStore → PanelNode → XtermHost 의 4-hop 약 50~200ms race window). dropped 시 첫 prompt 가 검은 화면으로 표면화 (`9268bc6`).
        - **결정**: dispatcher 가 *handler 미등록 pane* 의 PANE_OUT 을 **per-pane buffer** 에 stash. `registerPaneOut(paneId, handler)` 호출 시 flush.
        - **Cap**: 256 KiB per-pane. overflow 시 **FIFO drop-oldest** — 터미널 의 *최근 상태* (cursor·alt-screen 포함 redraw) 가 의미적으로 본질이므로 oldest 부터 폐기.
        - **Lifetime**: first `registerPaneOut` 호출 OR 해당 pane 닫힘 (panelsStore 에서 제거). 후자 시 buffer 즉시 해제.
        - **Hidden panel race 차단**: visibility=hidden Panel 은 ADR-0001 D8 의 Panel Streaming State *Suspended → `refresh-client -A pause`* 로 전이 → tmux 가 %output 자체를 안 보냄 → buffer 영구화 시나리오 차단.
        - Telemetry: overflow count metric (구현 단계에서 추가, UI 노출 X). 일상적 overflow 는 backend bug 의 신호이지 정상 흐름이 아님.
        - **거절안**: (a) cap 축소 (32~64 KiB) — 메모리 baseline 무의미 절약. (b) lifetime timeout (30s) — backend bug 를 silent 하게 흡수하므로 안 한다 (panic / error 로 *드러내는* 게 옳음). (c) backend 측 PANE_OUT 지연 발사 — Sprint 6 backend scope 증가 대비 이득 작음.
        - Result: this clause was added retroactively after the debug session 2026-05-14. See `docs/reports/0020-debug-classification.md` §2.5 + `docs/reports/0022-logic-amendment-decisions.md` §3.
- **D9.** [R4 §1 + Grill D12] **Layout 영속화는 WS 채널을 흐르지 않는다**. durable Canvas Layout은 HTTP `GET/PUT /api/layout` + `If-Match` ETag로만 (ADR-0006 SSoT). WS는 `0x80 LAYOUT_CHANGED` notify로 *변경 발생*만 알리며, 클라이언트는 그 신호를 받아 `GET /api/layout`을 발급해 새 상태를 확보한다 (Pull-through-notify). 이 분리는 *불변식 #1(tmux/웹 상태 분리)의 logical 분리 + 채널 수준 분리의 이중 강제*다.
- **D10.** [R4 §4 envelope 위반 함정] **WS envelope에 다음 데이터는 절대 흐르지 않는다 (불변식 강제 표)**:
    - Canvas geometry 문자열 (panel x/y/w/h/z) — HTTP `PUT /api/layout`만.
    - Shell 명령 문자열, 자유 형식 tmux 명령 문자열 — `0x01 CTRL`은 argv 배열(JSON `{cmd:"new-window", args:["-t", sessionName]}`)만 운반하고, 서버는 ADR-0008 allowlist에 있을 때만 control mode에 발급 (R4 §"가-c" + ADR-0001 D5).
    - tmux Layout 문자열 (`select-layout`이 받는 split layout 표현) — ADR-0008가 `select-layout` 발급 금지 → 운반할 메시지가 *정의되지 않음*.
    - Group 트리, Panel label/note — 모두 durable이므로 HTTP만.
    - 평문 토큰 값 — D5의 핸드셰이크 헤더가 유일 채널. envelope payload 어디에도 토큰을 두지 않는다.
    - 클라이언트 식별자 (`client_id`, `origin_id`) — Grill D13 MT-3에 의해 정의되지 않음.
- **D11.** [Grill D15 + MT-3 + R4 §6] **Sequence number와 resume token은 MVP에서 사용하지 않는다.**
    - 정당화 1 (web-domain): MT-3 broadcast는 *idempotent* — 같은 envelope를 다시 받아도 결과 동일. 순서가 일시적으로 뒤집혀도 *최신 값이 단일 진실*이라는 MT-3 invariant(Grill D13)에 의해 결과 수렴.
    - 정당화 2 (tmux-domain pane output): D8의 재연결 절차에서 ring buffer replay + live 전환이 *틈 없는 바이트 스트림*을 제공한다 — ring buffer가 부족한 경우는 사용자 명시 deep scrollback 회복 액션(`capture-pane`, P1+)에서만 발생하고 MVP는 그 시나리오를 노출하지 않는다 (Grill D15).
    - 정당화 3 (Layout): HTTP `If-Match` ETag가 이미 optimistic concurrency를 제공 — sequence number를 envelope에 두는 것은 *이중 idempotency*이며 복잡성만 증가.
    - P1+ 재방문 트리거: 같은 사용자가 신뢰성 낮은 네트워크(모바일 셀룰러)에서 사용 → resume token 도입 검토. WS-over-HTTP/3 마이그레이션 시 자연 해소 가능 (스트림 ID + QUIC 0-RTT).
- **D12.** [R4 §"미해결" + Grill D19] **`permessage-deflate` 압축 확장은 MVP에서 비활성화**. ANSI escape + UTF-8 + 짧은 시퀀스 패턴에서 deflate 이득이 작고 CPU 비용이 D19의 p99 < 100ms 예산에 침범할 위험이 있다 (R4 §"미해결" 5 — 측정 후 결정). R7 benchmark에서 (50 pane × 5 burst, compression on/off) 측정 후 stretch 단계에서 재검토 — 기본 비활성화는 *유지보다 안전한 디폴트*.
- **D13.** [R4 §1 표 1 + Grill D18] **단일 정적 binary**(ADR-0011 Rust)에서 `tokio-tungstenite`를 WebSocket lib로 채택한다. 본 ADR은 lib 이름만 고정하며 specific crate version은 R7 보고서가 잠근다 (ADR-0011 O1 인용).

## 거절된 대안 (Rejected)

- **R1. SSE (EventSource)** — R4 §1·표 1: 서버→클라 단방향만이라 stdin은 별도 POST 채널 필요 → 동일 pane을 위해 두 개 HTTP 자원, HTTP/1.1 도메인당 6개 동시 연결 제한에 50 pane × 2 = 100 채널이 정면 충돌. 이진 프레임 부재로 ANSI 출력에 base64 ~33% 부풀림 (R4 §1·§"옵션 비교표"). gtmux의 양방향·이진 요구와 *구조적* 비호환. *완전 거절.*
- **R2. WebTransport (HTTP/3 + RFC 9220/9297)** — R4 §1: 2026-05 기준 caniuse 80.46% 글로벌 커버리지 + Safari 26.4+ 진입으로 *기술적으로는 가능*하지만 (a) Node/Rust 외 서버 라이브러리 성숙도 얕음(R4 §1), (b) HTTP/3 인프라(QUIC + TLS 1.3 + 인증서) 운영 부담이 *단일 사용자 localhost 디폴트* 시나리오에서 정당화 불가, (c) ADR-0011 Rust 스택의 `wtransport` crate는 안정성/생태계 평가 별도 필요 — R7 scope에 추가하지 않음. MVP 비채택. **P1+ 마이그레이션 경로는 보존** — D2 envelope를 WebTransport 양방향 스트림 1개에 그대로 실으면 D2 슬롯 표를 변경할 필요 없음 (R4 §"미해결" 6).
- **R3. WebSocket-over-HTTP/2 (RFC 8441) / HTTP/3 (RFC 9220)** — R4 §1: H2/H3 위에서 `:protocol = websocket` Extended CONNECT로 단일 H2/H3 연결 다중화. 그러나 (a) RFC 8441 채택이 패치 상태, CDN/프록시 미지원 다수 (R4 §1 인용), (b) gtmux는 *자체 호스트, CDN 미경유*이므로 도메인당 6개 동시 연결 제한의 해소 가치 무, (c) D1의 단일 WS endpoint + D2 envelope 다중화로 이미 같은 효과 달성. 운영 복잡성 대비 이득 0. 거절.
- **R4. Long-polling** — R4 §1: 지연 가장 큼, 양방향에 별도 POST, 키 입력의 빈번한 작은 메시지마다 TCP 연결 세움/끊음 비용 누적. *완전 거절.* gtmux의 p99 < 100ms 예산(Grill D19) 직접 위배.
- **R5. 쿼리스트링 토큰 (`ws://127.0.0.1:9001/ws?token=...`)** — R4 §2: 액세스 로그·프록시 로그·`location.href` 콘솔에 토큰이 누출. 구현은 가장 간단하나 운영 위험이 누적. **MVP에서도 비권장 — 본 ADR에서 영구 거절.** ADR-0003 SSoT의 `query_string_auth = forbidden` 항목으로 잠금.
- **R6. 연결 후 첫 메시지로 인증** — R4 §2: 핸드셰이크가 이미 성립한 뒤 검증 → 자원 점유 발생, `Origin` 검증과 직교하지 않아 CSRF 표면 미축소. tmux 명령 발급 전 별도 게이트가 필요해 코드 경로 분기 ↑. 거절. D5의 `Sec-WebSocket-Protocol` 핸드셰이크 시점 검증이 단순·안전.
- **R7. 와일드카드 `Origin` 허용** — R4 §3: gorilla/websocket의 기본 안전장치 무력화 함정과 동일 부류. CSRF·DNS rebinding 표면 *전면 개방*. **영구 금지** (D6 정본).
- **R8. JSON envelope (`{type, paneId, payload(base64)}`)** — R4 §4: 디버깅성 최고, 그러나 `%output`의 ANSI 바이트 보존을 위해 base64 인코딩 → 대역폭 ~33% 부풀림. 50 pane × burst에서 D19의 WS write lag < 5s 예산과 메모리 baseline에 압력. 거절. 단, `0x01 CTRL`과 `0x07 NOTIFY_MIRROR`의 *페이로드*는 JSON으로 두어 디버깅성 유지 (D4 — *envelope 자체는 이진, 안쪽 페이로드만 JSON*의 절충).
- **R9. JSON 배열 envelope (terminado 패턴 `["stdin", data]`)** — R4 §4·§"옵션 비교표": 채널 키 부재로 다중 pane 모델에 부적합. 거절.
- **R10. 단일 바이트 opcode + raw payload (ttyd/gotty 패턴)** — R4 §4: paneId 가변 길이 인코딩 자리 없음 → 다중 pane 불가. 거절. (D2가 이 패턴을 *paneId varint 1개 추가*로 확장한 절충안.)
- **R11. pane당 WS 소켓 / subprotocol당 소켓** — R4 §4·§1 표 1: HTTP/1.1 6개 동시 연결 제한 + N개 TLS 핸드셰이크 비용. D1의 단일 endpoint + D2 envelope 다중화로 자연 우회. 거절.
- **R12. Sequence number를 envelope에 포함** — D11 정당화 3개로 거절. MT-3 idempotent broadcast + ring buffer replay + HTTP ETag가 이미 순서/누락 문제를 *카테고리별 해결*. 도입 시 코드 복잡성 ↑, 디버깅 표면 ↑, 이득 0. P1+ 신뢰성 낮은 네트워크 시나리오에서 재방문.
- **R13. WS 위에 Canvas Layout durable write** — R4 §"gtmux에의 함의" §117의 *분리 정신*은 채택하나 R4 본문의 `POST /layouts` 단정은 Grill D12에 의해 supersede. WS에 durable 영속화 메시지를 실으면 reconnect 중 write 손실, 백프레셔 큐 경쟁, optimistic concurrency 직접 구현(서버에 ETag 발급/체크 + WS race) 부담 발생 (Grill D12). HTTP만 영속화 채널. **본 ADR §맥락 supersession 명기.**

## 결과 (Consequences)

- 긍정:
    - **단일 채널 다중화** — Server당 WS endpoint 1개로 N pane × M ephemeral state 모두 운반. ADR-0007의 1:1:1 모델과 자연 합성.
    - **상태 분리 강제** — envelope `type` 1바이트의 0x01–0x0F / 0x80–0x8F 구획이 5대 불변식 #1·#3을 *바이트 수준에서 기계적 강제*. 잘못된 핸들러 라우팅이 *컴파일 타임 분기 위반*으로 드러남 (ADR-0011 Rust enum exhaustive match와 자연 결합).
    - **백프레셔 합성** — tmux pause-after(D7-1) + 서버 큐 watermark(D7-2) + 클라이언트 bufferedAmount(D7-3)가 같은 `refresh-client -A` 명령으로 *수렴 합류* (ADR-0001 D8과 D7-1·D7-2가 동일 메커니즘). 코드 경로 ↓.
    - **재연결 단순성** — backend long-lived + browser-only reconnect (D8) + sequence number 0 (D11). 재연결 코드가 *HTTP GET + WS attach + ring buffer replay + state push 4단계 직선*.
    - **검증 패턴 채택** — Kubernetes API server의 `Sec-WebSocket-Protocol` 토큰 패턴(R4 §2 / D5), Cockpit의 다채널 envelope 절충(R4 §4 / D2). 운영 위험 ↓.
- 부정/비용:
    - **이진 디버깅 부담** — 헥스덤프가 필요하나 SSoT §3의 디코더 의사코드 + dev 모드 로그(envelope 단위 traceable id)로 완화. JSON envelope 대비 디버깅성 ↓는 *대역폭/메모리 baseline*과의 trade-off.
    - **`permessage-deflate` 비활성화** — 텍스트 도미넌트 페이로드에서 잠재 30%+ 절감을 포기 (D12). R7 측정 후 stretch에서 재검토.
    - **resume token 부재** — P1+ 모바일/원격 노출 시 가입자가 짧은 단절을 자주 겪으면 ring buffer가 부족할 수 있음. Grill D15·R4 §"미해결" 7에 후속 검토.
    - **WS 핸드셰이크 시점 토큰 검증** — TLS 미사용 시 헤더 평문 노출 (R4 §2). 디폴트 바인드가 loopback이라 MVP 위험은 작으나 cloud 모드에서 TLS 필수 (ADR-0003 D22 `[cloud].tls_cert/tls_key` 인용).
    - **WebTransport 마이그레이션 경로 미사용** — D2 envelope가 *WebTransport 양방향 스트림과 호환되도록 설계*되었으나 MVP에서 활용 안 됨. 미래 가치 보존 비용 ≈ 0.
- 후속 작업:
    - **ADR-0003** (보안 디폴트) — D5의 토큰 전달 경로를 받아 발급/저장(`${XDG_CONFIG_HOME}/gtmux/<session>.token` 0600)/회전(`gtmux rotate-token`)/`Authorization: Bearer` HTTP 정책을 SSoT(`docs/ssot/security-defaults.md`)로 정본화. WS subprotocol 토큰과 HTTP Bearer 토큰은 *동일 토큰*이어야 함.
    - **ADR-0006** (영속화 storage) — D8·D9의 HTTP `GET/PUT /api/layout`을 sqlite/JSON file 등 storage backend에 매핑. `docs/ssot/canvas-layout-schema.md`의 ETag 정규화 §2 그대로 인용.
    - **ADR-0011** (Rust backend) — `tokio-tungstenite` 정확한 crate version + Origin/Host 미들웨어 + `Sec-WebSocket-Protocol` 검증 위치 + ETag (RFC 7232) middleware 구성을 R7 benchmark + scaffolding으로 잠금. D7-2의 watermark 정량값 R7 측정.
    - **A4 정합성 리뷰** — ADR-0001 + ADR-0002 + ADR-0003 cross-reference (`docs/reports/0009-adr-coherence-review.md`). 특히 (a) 인증 토큰 메커니즘이 ADR-0002와 ADR-0003에 *동일* 기술되는지, (b) `0x01 CTRL` envelope의 argv 배열 컨벤션이 ADR-0008 allowlist 표와 일치하는지, (c) D7의 watermark가 ADR-0001 D8의 `refresh-client -A` 호출과 동일 메커니즘인지 검증.
    - **R7 benchmark DoD 추가** — D7-2의 watermark 임계(high=512 KiB, low=128 KiB) 50 pane × 5 burst 측정, D12의 `permessage-deflate` on/off 측정, D11의 resume token 부재 시 재연결 latency 측정. 결과로 본 ADR amend 가능.

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태/웹 상태 분리 | **PASS (강함)** — D2 envelope의 `type` 1바이트가 0x01–0x0F (tmux-domain) / 0x80–0x8F (web-domain) 두 구획으로 *바이트 수준 분리*. D9가 durable Canvas Layout을 *물리적으로 다른 채널(HTTP)*로 분리. D10이 "WS envelope에 절대 흐르지 않는 데이터" 목록으로 *negative space*까지 정의 — Canvas geometry, Group 트리, Panel label/note 모두 HTTP만. ADR-0011 Rust enum exhaustive match로 라우팅 위반이 컴파일 타임에 드러남. |
| 2 | tmux-native vs web-only 분기 | **PASS** — D2의 두 구획이 그대로 분기 dispatch. `0x04 PANE_RESIZE`는 tmux `resize-window`(single-pane-window 컨벤션 하 = pane resize)를 호출하지만 Panel의 *CSS 크기*는 `0x83 VIEWPORT_CHANGED`와 무관하게 HTTP `PUT /api/layout`의 `panels[].w/h`로 별도 관리. 두 메시지 타입이 *코드 핸들러를 공유하지 않게* envelope 구획 자체가 강제. D4의 `0x01 CTRL` 페이로드는 ADR-0008 allowlist 표 안 명령(argv 배열)만 운반 — 표 밖 명령은 envelope에 *나타나지 않는다*. |
| 3 | tmux Layout ≠ Canvas Layout | **PASS** — D10이 *명시적으로* tmux Layout 문자열을 envelope에 두지 않음을 잠금. ADR-0008가 `select-layout` 발급을 금지하므로 운반할 메시지 타입이 *정의되지 않음*. Canvas geometry(panel x/y/w/h/z)는 HTTP `PUT /api/layout`만 (D9). 두 layout이 *서로 다른 채널에서 흐르고 절대 만나지 않음*. |
| 4 | 보안 기본값 | **PASS** — (a) D5의 `Sec-WebSocket-Protocol` 토큰 패턴이 쿼리스트링·평문 첫-메시지·로그 누출 표면을 *동시 차단*, (b) D6의 Origin + Host 동시 allowlist가 CSRF + DNS rebinding을 *동시 방어*, (c) D10이 평문 토큰·shell 문자열·tmux 자유 명령을 envelope에서 *영구 추방*, (d) D4의 `0x01 CTRL`이 argv 배열만 허용 → ADR-0001 D5·ADR-0008 allowlist와 합성하여 명령 주입 표면 *구조적* 차단, (e) R5·R7이 쿼리스트링 토큰·와일드카드 Origin을 명시 거절. ADR-0003가 12개 체크리스트로 SSoT 정본화. |
| 5 | control mode 사용 | **PASS** — D4의 `0x02 PANE_OUT`은 ADR-0001 D7의 `%output` 디코딩 결과만 운반 (스크린 스크레이핑·셸아웃 폴링 경로 없음). `0x05/0x06 PAUSE/RESUME`은 ADR-0001 D8의 `refresh-client -A` 명령으로 1:1 변환. `0x07 NOTIFY_MIRROR`는 tmux native `%` 알림 14종 mirror. *envelope 자체가 control mode 메시지 카탈로그의 미러* — 본 ADR이 control mode 사용을 추가 강화. |

## 미해결 항목 (Open)

본 ADR이 R4 §"미해결 질문" 7개 중 처리한 것과 미룬 것:

**Resolved (본 ADR이 잠금)**:
- R4 §"미해결" 1 **envelope 정확한 코드 표** → **D2·D3·D4 + SSoT §2가 32개 슬롯 전부 정의**. paneId 인코딩 = unsigned LEB128 varint(D2). 시퀀스 번호 = MVP 미포함(D11). 이진 vs JSON 페이로드 식별 = `type` 바이트의 카테고리(0x01 CTRL·0x07 NOTIFY_MIRROR만 JSON 페이로드, 나머지는 이진 — SSoT §2가 슬롯별 명기).
- R4 §"미해결" 2 **재연결 시 리플레이 정책** → **D8 + ADR-0001 D7(128 KB 기본, Grill D15)**. 클라이언트가 `last-seq`/`since` 토큰을 보내지 않음 (D11 — sequence number 미사용). ring buffer가 부족한 deep scrollback은 사용자 명시 `capture-pane` 액션(P1+)에서만 발생, MVP 노출 안 함.
- R4 §"미해결" 4 **`Sec-WebSocket-Protocol` 다중 값 포맷** → **D5**: `gtmux.v1, bearer.<base64url-token>` 두 값, 서버 응답은 `gtmux.v1`만(Kubernetes PR #47740 패턴). JWT 미사용(단일 사용자 환경에서 over-engineered). CSRF 토큰과 별도 결합은 ADR-0003에서 *secondary `SameSite=Strict` HttpOnly cookie* (Grill D17·D18 인용)로 잠금.

**Deferred to R7 benchmark (정량 boundary)**:
- **O1** [R4 §"미해결" 3] **서버 큐 high/low watermark 임계값** — D7-2의 잠정값 high = 512 KiB / low = 128 KiB가 D19의 p99 < 100ms·메모리 baseline < 30 MB 예산을 50 pane × 5 burst 시나리오에서 만족하는지 R7이 측정. 결과로 본 ADR amend.
- **O2** [R4 §"미해결" 5] **`permessage-deflate` 사용** — D12의 잠정 비활성화. R7이 on/off 측정 후 stretch 단계에서 재검토 (`permessage-deflate = configurable` 도입 가능).
- **O3** [R4 §"미해결" 3] **클라이언트 `bufferedAmount` 임계** — D7-3의 잠정값 256 KiB. R8 (프론트엔드 벤치)이 키 입력 latency 영향 측정 후 보정.

**Deferred to P1+ (시나리오 트리거 후)**:
- **O4** [R4 §"미해결" 6] **WebTransport 마이그레이션 경로** — D2 envelope를 WebTransport 양방향 스트림 1개에 그대로 실을지 vs pane당 스트림 분해. P2 이후 — Safari/Firefox WebTransport 안정 + `wtransport` crate 평가 후 결정. *D2 envelope 형식은 변경 없이 재사용 가능하게 설계됨* (마이그레이션 비용 ≈ transport 어댑터 1개 교체).
- **O5** [R4 §"미해결" 7] **Mosh-style 예측 로컬 에코** — D11의 resume token 검토와 동반. 원격/모바일 노출 시 키 입력 체감 지연 완화 카드.
- **O6** [본 ADR D11 정당화] **Sequence number 재도입** — 모바일 셀룰러 등 신뢰성 낮은 네트워크 시나리오 노출 시. WS-over-HTTP/3 마이그레이션과 동반될 가능성.

**A4 정합성 리뷰 게이트 (O7)** → `docs/reports/0009-adr-coherence-review.md`에서 본 ADR과 ADR-0001·ADR-0003 + 두 SSoT (`docs/ssot/wire-protocol.md`, `docs/ssot/canvas-layout-schema.md`)의 cross-reference 점검 후 Status를 Accepted로 승격. 본 단계에서는 Proposed 유지.
