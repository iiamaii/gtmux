# 보고서: A4 정합성 리뷰 — ADR-0001/0002/0003 + SSoT 2종 + R7/R8 cross-check

- 일자: 2026-05-13
- 트랙: A4 (`docs/plans/0002-work-dispatch.md` §1 Task A4)
- 작성: self-review (PM 위임)
- 검토 범위:
  - `docs/adr/0001-tmux-integration-control-mode.md`
  - `docs/adr/0002-transport-websocket.md`
  - `docs/adr/0003-security-defaults.md`
  - `docs/ssot/wire-protocol.md`
  - `docs/ssot/security-defaults.md`
  - `docs/reports/0007-backend-runtime.md` (R7)
  - `docs/reports/0008-frontend-stack.md` (R8)
  - 비교 기준: `docs/adr/0007–0012`, `docs/reports/0010-grill-amendments.md`, `docs/reports/0011-coherence-review.md` (1차/2차), `docs/ssot/canvas-layout-schema.md`, `CONTEXT.md`, `docs/sketch.md`
- 상태: 1차 A4 — Blocking 3건·Advisory 6건·Cosmetic 2건 식별. 배치 C(코드 부트스트랩) 진입 조건은 Blocking 3건 fix commit 후.

## 요약 (3문장)

배치 A1·A2·A3의 산출물 5개(ADR-0001/0002/0003 + SSoT 2종)는 5대 불변식 검증에서 25칸 모두 PASS하고 grill D1~D22·R1/R4/R5의 모든 결정문을 정확히 흡수했지만, **인증 토큰 서브프로토콜 포맷이 ADR-0002와 ADR-0003·SSoT·R7 사이에서 세 가지 다른 형태로 표기**되어 클라이언트·서버 핸드셰이크가 *런타임에 서로 다르게 작동할 가능성*이 발견됐다(B1, Blocking). **WS 0x83 VIEWPORT_CHANGED 페이로드의 엔디언이 SSoT(LE)와 R3/R8(BE) 사이에서 모순**되며 R3는 paneId varint 자체를 누락(B2, Blocking), **R3 보고서 §F7의 zoom-blur risk를 R8 F8이 placeholder 정책으로 closure했으나 ADR-0012 본문/sketch.md §14에 그 결정이 명문화되지 않은 상태**(B3, Blocking)다. R7/R8이 ADR-0011/0012의 Open O1~O7을 모두 closed 상태로 가져왔고, 토큰 파일 경로(`${XDG_STATE_HOME}/gtmux/<session>.token`) · 크립토(`ring` 단독) · MSRV(1.85, "1.80+" 잠정값 안) 등 cross-ADR 키들은 일관된다.

## 조사 범위

A4 spec(plan §1 A4) + 1차/2차 coherence review carry-forward 항목을 12개 specific check로 분해.

1. 인증 토큰 메커니즘 — ADR-0002 D5 / ADR-0003 D5 / SSoT 표 일관성
2. tmux command 발급 경로 — ADR-0001 D5 / ADR-0008 allowlist / wire-protocol 0x01 CTRL argv 배열
3. Backpressure — ADR-0001 D9 `pause-after` + ADR-0002 D7 큐 watermark가 동일 `refresh-client -A` 메커니즘
4. Layout 영속화 — WS notify only / HTTP durable 분리(ADR-0002 D9 + ADR-0010 SSoT)
5. ADR 각각의 R<N>·Grill 인용 + 5대 불변식 검증 강도
6. R7 → ADR-0011 Open O1~O7 closure 매핑
7. R8 → ADR-0012 Open O1~O7 closure 매핑
8. R8 F8 zoom-blur 정책 — R3 risk #1과 sketch.md/ADR 텍스트 정합
9. 토큰 파일 경로 drift — R5 `${XDG_CONFIG_HOME}` ↔ A3/Grill `${XDG_STATE_HOME}` ↔ ADR-0009 D6
10. Crypto crate — ADR-0011 D8 / R7-T2 / ADR-0003 / sketch / CONTEXT 일관성
11. MSRV 1.85 vs ADR-0011 O1 "1.80+"
12. Number/path 충돌

## 핵심 발견

### 1. 인증 토큰 메커니즘 — **세 가지 형식 모순 (Blocking)**

`Sec-WebSocket-Protocol` 서브프로토콜 토큰 전달 포맷이 세 산출물에서 다른 문자열로 표기됨:

| 출처 | 표기 | 의미 |
|---|---|---|
| ADR-0002 D5 + Open O처리 4 | `Sec-WebSocket-Protocol: gtmux.v1, bearer.<base64url-token>` (두 값 콤마 분리) | 클라이언트가 2개 subprotocol을 advertise, 서버는 `gtmux.v1`만 echo |
| ADR-0003 D5 | `gtmux.v1.bearer.<base64url-token>` (단일 subprotocol, 점-구분) | "단일 서브프로토콜로 통일" 명시 |
| SSoT `security-defaults.md` §1.3 + §3 JSON | `"ws_subprotocol_format": "gtmux.v1.bearer.<base64url-token>"` (단일, 점) | ADR-0003과 동일 |
| R7 §5 핸들러 sketch (line 166) | `requested.len() != 2`, `requested[0] != "gtmux.v1"` 검증 + `ws.protocols(["gtmux.v1"])` echo | ADR-0002와 동일 (두 값 콤마) |
| Wire-protocol SSoT line 159 | 인용만, 포맷 미명시 | — |

→ HTTP 헤더 차원에서 `gtmux.v1, bearer.xxx`(콤마 두 값)와 `gtmux.v1.bearer.xxx`(점 한 값)는 **다른 wire 표현**이며, RFC 6455 §11.3.4 `Sec-WebSocket-Protocol` 헤더는 *콤마 구분된 토큰 리스트* 의미이므로 두 의미가 호환되지 않는다 — 서버가 `gtmux.v1.bearer.xxx`를 *단일* 토큰으로 받으면 콤마-분리 클라이언트 advertise를 *2개 토큰*으로 파싱하여 토큰 비교 자체가 실패.

### 2. tmux command 발급 경로 — **PASS**

| 출처 | 표현 | 일관성 |
|---|---|---|
| ADR-0001 D5 | "ADR-0008 §tmux command allowlist 표가 정의하는 발급 가능 명령 집합 안에서만 발급" — 표 인용 | ✓ |
| ADR-0008 §command allowlist 표 (정본) | 9행 ALLOW + 영구 금지 (`split-window`/`resize-pane`/`select-layout`/`-CC`/...) | ✓ |
| Wire-protocol SSoT §2.4 (`0x01 CTRL` JSON) | `{"cmd": string, "args": string[]}` argv 배열 — "ADR-0008 §command allowlist 표 안 값만 허용. `args`는 *문자열 배열만*" | ✓ |
| ADR-0003 D7 | "argv 배열 분리 (Rust = `tokio::process::Command::arg`만 사용, 셸 미경유)" + ADR-0008 allowlist 인용 | ✓ |
| Security-defaults SSoT §1.7 | allowlist 9행 + blocklist 8행 (ADR-0008 정본 인용) | ✓ |

→ argv 배열 컨벤션이 4개 출처에서 *동일 표현*으로 일치. `0x01 CTRL` envelope이 shell 문자열을 운반하지 않도록 wire-protocol §4 "절대 나타나면 안 되는 데이터" 표가 negative space까지 명시. **PASS (강함)**.

### 3. Backpressure — **PASS**

| 출처 | 표현 |
|---|---|
| ADR-0001 D8 | Panel Streaming State 전이 → `refresh-client -A '%<pid>:pause/continue'` (300ms 디바운스) |
| ADR-0001 D9 | `pause-after = 10s` MVP / `5s` stretch, `[runtime].pause_after_sec` config |
| ADR-0002 D7 | 3계층 합성: tmux pause-after(D9) + 서버 큐 watermark(high=512 KiB/low=128 KiB) + 클라이언트 bufferedAmount(256 KiB). "서버 큐의 high 도달 시 *D8과 동일 메커니즘인* `refresh-client -A '%<pid>:pause'` 발급" 명시 |
| Wire-protocol SSoT §2.1 0x05/0x06 | `PANE_PAUSE`/`PANE_RESUME` → `refresh-client -A '%<N>:pause/continue'` (ADR-0001 D8 인용) |

→ ADR-0002 D7가 "ADR-0001 D8과 동일 메커니즘"임을 명시한 후 wire-protocol SSoT가 0x05/0x06 envelope과 1:1 매핑. 정량값(10s, 512 KiB, 128 KiB, 256 KiB)는 R7 §9.6/9.3 측정 대상으로 ADR-0001 O2 + ADR-0002 O1·O3에서 추적. **PASS**.

### 4. Layout 영속화 — **PASS**

| 출처 | 표현 |
|---|---|
| ADR-0002 D9 | "durable Canvas Layout은 HTTP `GET/PUT /api/layout` + `If-Match` ETag로만 (ADR-0006 SSoT). WS는 `0x80 LAYOUT_CHANGED` notify로 *변경 발생*만 알리며, 클라이언트는 그 신호를 받아 `GET /api/layout`을 발급" |
| ADR-0002 D10 | "WS envelope에 절대 흐르지 않는 데이터" 표 — Canvas geometry, Group 트리, Panel label/note 모두 HTTP만 |
| Wire-protocol SSoT §4 | 동일 표 (위 D10 재인용 + raw etag 16B는 WS, JSON hex 32자는 HTTP) |
| ADR-0010 SSoT (canvas-layout-schema.md §2) | ETag 16B raw 정본, HTTP JSON = 32자 lowercase hex (1차/2차 G4 closed) |
| 2차 G3 supersession 명기 | ADR-0002 §맥락에 "R4 §본문의 `POST /layouts` 단정은 grill D12에 의해 supersede" 명문화 (lines 14-16) | ✓

→ Pull-through-notify 패턴(WS notify → HTTP GET)과 ETag 16B raw / 32hex 변환이 4개 출처에서 일관. **PASS (강함)**.

### 5. 각 ADR의 R<N>/Grill 인용 + 5대 불변식 검증 — **PASS**

| ADR | 근거 보고서 인용 | 5대 불변식 |
|---|---|---|
| ADR-0001 | R1 + Grill D8/D15/D16/D19, 매 결정에 § 단위로 인용 | 5/5 PASS, #5 *PASS (강함)* — 본 ADR이 #5의 정본 |
| ADR-0002 | R4 + Grill D12/D13/D14/D17/D19 | 5/5 PASS, #1 *PASS (강함)* — envelope 0x01–0x0F/0x80–0x8F 바이트 수준 분리 |
| ADR-0003 | R5 (12-item) + Grill D17/D20/D21/D22 | 5/5 PASS, #4 *PASS (강함)* — 12 체크리스트 + D13 토큰 정책 + D14 argv guard + D15 CSP |

검증 문구 모두 *근거 있는 PASS*(한 줄 placeholder 없음). 1차/2차 coherence review가 plan §A0.6 DoD에 "PASS (강함) 표현 권장"을 요청했고 ADR-0001/0002/0003은 그 패턴을 직접 채택했다. **PASS**.

### 6. R7 → ADR-0011 Open closure — **PASS (단, MSRV 보강 권고)**

R7 §10 closure 표가 7개 Open을 모두 매핑:

| ADR-0011 Open | R7 결정 | 본 보고서 검증 |
|---|---|---|
| O1 MSRV | **1.85** + `rust-toolchain.toml` pin YES | ✓ (§11 참조) |
| O2 Crypto | `ring 0.17.16000` 단독, binary size < 500 KB 허용폭 (dead-code-elim 추정) | ✓ (§10 참조). 실측은 R7 U5 — 후속 |
| O3 Parser | `winnow 1.0.2` + `bytes` LUT 하이브리드 | ✓ |
| O4 WS token hook | axum extractor 안 + `requested_protocols`/`set_selected_protocol` 수동 | ✓ |
| O5 Schema codegen | `utoipa 5.5.0` + `openapi-typescript` | △ — R8 F2가 `schemars + json-schema-to-typescript` 채택 (A2 Advisory) |
| O6 Config | `figment 0.10.19` (profile + provenance) | ✓ |
| O7 Workspace | 7-crate scaffolding (`mux-router`/`ws-server`/`http-api`/`lifecycle`/`config`/`auth`/`gtmux-cli`) + DAG + 역방향 차단 | ✓ |

→ 7개 모두 closed *카테고리*. ADR-0011 Status는 본 보고서 + R7 채택 후 **Proposed → Accepted** 승격 가능.

### 7. R8 → ADR-0012 Open closure — **PASS (단, schema 도구 불일치 1건)**

R8 §"옵션 비교표 요약"이 7개 Open을 모두 closed:

| ADR-0012 Open | R8 결정 | 본 보고서 검증 |
|---|---|---|
| O1 xterm.js × Svelte 5 wrapper | hand-rolled `$effect` 채택, `BattlefieldDuck/xterm-svelte` 비채택 | ✓ |
| O2 Schema pipeline | `schemars` + `json-schema-to-typescript` | ⚠ A2 — R7과 도구 모순 |
| O3 Reactivity | 분할 class store + SvelteMap, 50 pane × M_CHANGED broadcast 시 boolean derived 50회(<1ms) | ✓ |
| O4 Worker decision | MVP 메인 스레드, P1+ Worker (O4 위반 시 트리거) | ✓ |
| O5 ETag UX | 자동 rebase + 사용자 패널 우선 머지 + 토스트 + 3회 재시도 후 confirm modal | ✓ |
| O6 Reconnect UX | 1s grace + exp backoff 0.5→30s + sticky banner 5종 copy | ✓ |
| O7 Bundle/cold-start | ~145–165 KB gzip (< 200 KB 통과) + cold start < 500ms | ✓ |

→ 7개 모두 closed. ADR-0012 Status는 본 보고서 + R8 채택 후 **Proposed → Accepted** 승격 가능. 단 O2의 schema 도구가 R7 §6과 모순(아래 A2 참조).

### 8. R8 F8 zoom-blur 정책 — **부분 closure (Blocking)**

R3 §F7/O2 risk #1: "xterm.js × CSS-transform scale → blur". R8 F8이 **(b) Placeholder on zoom (ε=0.02)** 정책으로 closed:

- `|zoom-1| < 0.02` 외 구간에 xterm DOM을 placeholder로 대체.
- Streaming State (D16)와 *직교* — 데이터 흐름은 계속 유지.

그러나 이 결정이 **다른 곳에 전파되지 않은 상태**:

| 위치 | 현 상태 | 필요 갱신 |
|---|---|---|
| `docs/adr/0012-frontend-stack-svelte.md` | D4(xterm.js) + D5(캔버스 lib 필터) + O3까지만, *zoom-blur 정책 본문 부재* | D4 또는 D5에 "zoom-blur는 R8 F8 (b) placeholder 정책으로 closed" 한 줄 또는 §결과에 명시 |
| `docs/sketch.md` §14 (기술적 난점) | 8개 난점 enumerate, zoom-blur 미언급 | §14에 "9. xterm.js × CSS scale blur (R8 F8 placeholder 정책으로 closed)" 1행 또는 §6.4 캔버스 절에 placeholder UX 명시 |
| `CONTEXT.md` "Multi-connection 정책" 인근 | zoom 정책 부재 | "Panel Streaming State"와 별개 차원으로 *zoom-mode visibility* 한 줄 명시 |
| ADR-0004 (터미널 렌더링, 미발행) | — | 발행 시 본 결정을 입력 제약으로 인용 |

→ 정책 자체는 결정됐으나 *문서화 surface가 산발적*이라 코드 부트스트랩 시 개발자가 어느 출처를 따를지 모호. **B3 Blocking**.

### 9. 토큰 파일 경로 — **PASS (자동 정렬)**

| 출처 | 경로 |
|---|---|
| R5 §6 (line 173) | `${XDG_CONFIG_HOME}/gtmux/token` (R5 원본) |
| Grill 0010 §3 (line 282) | `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token` (D17 정정) |
| ADR-0009 D6 step 4 | `${XDG_STATE_HOME}/gtmux/<session>.token` (정정 반영) |
| ADR-0003 D4 + D13.3 | `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token` + "R5의 `${XDG_CONFIG_HOME}` 표기는 D20에서 `${XDG_STATE_HOME}`으로 재배치됨" 명문화 (line 33) |
| Security-defaults SSoT §1.3 | `${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token` |
| sketch.md §10.1 line 427 | `${XDG_STATE_HOME}/gtmux/` (token·layout) — 이미 D20 amend |
| sketch.md §13.4 (line 624) | `${XDG_CONFIG_HOME}/gtmux/<session>.token` ⚠ — 옛 경로 잔존 |

→ sketch.md §13.4 단 한 줄이 옛 `${XDG_CONFIG_HOME}` 경로 보유. ADR-0003 D4가 R5의 표기를 명시적으로 supersede하므로 *의미는 정합*이나 *문구는 drift*. **C1 Cosmetic** (sketch는 1차 자료이나 본 결정의 supersession을 §13.4에 1행 반영하면 모호성 해소).

### 10. Crypto crate — **PASS**

| 출처 | 표현 |
|---|---|
| ADR-0011 D8 | "ring 1순위, rustls+rand+subtle 대안 R7-T2에서 라이선스·플랫폼 호환성 비교 후 확정" |
| R7 §3 (T2) | "**ring 0.17.16000 단독**". 라이선스 Apache-2.0 AND ISC, BoringSSL 정적, MSRV 1.85, ADR-0011 D8 1순위 그대로 잠금 |
| ADR-0003 D4 / D13.4 | "`ring::constant_time::verify_slices_are_equal`로 구현" + "ADR-0011 D8 강제" |
| Security-defaults SSoT §1.3 | `"token_csprng": "ring::rand::SystemRandom"` + "1순위. R7-T2 결과로 대안 확정" |
| sketch / CONTEXT | crypto crate 미언급 (sketch는 카테고리 수준만 진술) |

→ R7-T2가 ADR-0011 D8 1순위를 잠갔고 ADR-0003·SSoT 모두 `ring` API를 명시 인용. cloud 모드 TLS 추가 도입(R7 §3 후단 + R7 U6)은 ADR-0003 amend로 후속 — 본 단계 비차단. **PASS**.

### 11. MSRV 1.85 vs ADR-0011 O1 "1.80+" — **PASS (의미 정합)**

ADR-0011 O1: "stable 채널 **1.80+**". R7-T1: "max(rust-version) = **1.85**" (clap·ring·rustls·config·rand 공통 floor).

→ "1.80+"는 *하한*(≥ 1.80) 의미이므로 1.85는 그 안에 포함. *상한* 진술 없음. R7이 ADR-0011 O1을 *정확값으로 좁힘*. 모순 없음. ADR-0011 D1의 "edition 2021 이상"도 1.85가 edition 2024를 지원하여 정합. **PASS**.

### 12. Number/path 충돌 — **PASS (단, 인용 컨벤션 1건)**

| 경로 | 충돌 여부 |
|---|---|
| `reports/0009-adr-coherence-review.md` (본 문서) | 신규 발행 — 충돌 없음. 1차/2차 coherence review의 G11 권고("0013부터 사용")와 별개로 plan A4 spec이 `0009` 슬롯을 명시 지정 |
| `reports/0010-grill-amendments.md` ↔ `adr/0010-group-data-model.md` | 1차 G11 인지, 디렉터리 prefix로 분리 |
| `reports/0011-coherence-review.md` ↔ `adr/0011-backend-stack-rust.md` | 2차 G11 인지 |
| `reports/0007-backend-runtime.md` ↔ `adr/0007-server-session-port-binding.md` | **신규 충돌 발생** — 같은 숫자 "0007"이 reports와 adr 양쪽 점유 |
| `reports/0008-frontend-stack.md` ↔ `adr/0008-single-pane-window-and-group.md` | **신규 충돌 발생** — 같은 숫자 "0008" 양쪽 점유 |

→ 2차 G11의 "다음 보고서는 0013+ 사용" 권고가 R7/R8 작성에 반영되지 않음. 본 ADR R8이 ADR-0011·0012 짝을 의도했으나 reports/adr 양쪽 0007/0008/0011/0012가 동시 점유 → 인용 시 "ADR-NNNN" / "보고서 NNNN" prefix 강제 필요. **C2 Cosmetic**.

## 갭 (Findings)

### B1 (Blocking). 인증 토큰 서브프로토콜 포맷이 세 가지 다른 표기

- **현상**: §1 표 참조. ADR-0002 D5(`gtmux.v1, bearer.<token>` 콤마 두 값) ↔ ADR-0003 D5 + SSoT(`gtmux.v1.bearer.<token>` 단일 점) ↔ R7 §5 핸들러(콤마 두 값) 세 가지.
- **영향**: RFC 6455 §11.3.4 `Sec-WebSocket-Protocol` 헤더는 콤마-구분 토큰 리스트이므로 *클라이언트가 점-표기로 advertise + 서버가 콤마-표기로 split*하면 토큰 비교 자체가 실패. 핸드셰이크 1행이 무작위 단위에서 깨짐. 단일 사용자 환경에서도 *모든 연결 401*.
- **권고**:
  - **옵션 A** — ADR-0003 D5 + SSoT §1.3 + SSoT §3 JSON을 콤마 두 값(`gtmux.v1, bearer.<base64url-token>`)으로 정정. ADR-0002 D5와 R7 §5 sketch가 정본.
  - **옵션 B** — ADR-0002 D5 + R7 §5 sketch를 점 단일(`gtmux.v1.bearer.<base64url-token>`)로 정정. ADR-0003·SSoT가 정본.
  - **추천: 옵션 A**. 근거: (i) RFC 6455의 `Sec-WebSocket-Protocol` semantics에 콤마 두 값이 자연 — 두 token 중 server가 *하나만* echo한다는 Kubernetes PR #47740 패턴(R4 §2 + ADR-0002 D5 인용)이 정본. (ii) 점-단일 표기는 *RFC상 단일 token*이라 서버가 `gtmux.v1`만 echo하기 위해 *문자열 split 추가 코드*가 필요. (iii) ADR-0002의 거절 항목 R6(연결 후 첫 메시지 인증)이 이미 *핸드셰이크 시점*을 가정. 옵션 A가 R4 evidence와 가장 정합.
  - **정정 작업**:
    1. `docs/adr/0003-security-defaults.md` D5 (line 34) — "단일 서브프로토콜로 통일" → "두 값(`gtmux.v1`, `bearer.<base64url-token>`)으로 advertise하고 서버는 `gtmux.v1`만 echo (RFC 6455 + Kubernetes PR #47740 패턴, ADR-0002 D5와 동일)".
    2. `docs/ssot/security-defaults.md` §1.3 (line 47) — `ws_subprotocol_format` 값을 `"gtmux.v1, bearer.<base64url-token>"` (콤마 + 공백) 또는 분리 표현으로 정정.
    3. `docs/ssot/security-defaults.md` §3 JSON (line 253) — 동일.
    4. ADR-0003 변경 이력 1행 추가.

### B2 (Blocking). VIEWPORT_CHANGED 페이로드 엔디언·paneId 모순

- **현상**:
  - SSoT `wire-protocol.md` §2.2 (line 60): `0x83 VIEWPORT_CHANGED` paneId=0, payload = `int32 x (LE) + int32 y (LE) + float32 zoom (IEEE-754 LE)`. 12B.
  - SSoT §5 예제 4 (line 188-191): 14B 총길이 = 1B(type) + 1B(varint paneId=0) + 12B(LE payload). ✓ SSoT 자체 일관.
  - R8 F4 (line 255): `0x83 VIEWPORT_CHANGED: int32 x + int32 y + float32 zoom (BE, 12B)`. *BE 표기*.
  - R8 F4 디코더 sketch (line 561): `# DataView 헬퍼 (varint, int32 BE, float32 BE)`. *BE*.
  - R3 §example (line 243): "`[1B 0x83][int32 x BE][int32 y BE][float32 zoom BE]` 9 바이트 binary". *BE* + **paneId varint 누락** (9B = 1+12 not 1+1+12).
- **영향**: (i) LE/BE 직접 충돌 — 백엔드(Rust `i32::from_le_bytes`, R7이 SSoT 인용으로 LE 채택)와 프런트엔드(R8 sketch BE) 사이 viewport 좌표가 4바이트 순서 다르게 해석 → x = 1024 송신 시 수신측 0x00 0x04 0x00 0x00 (LE) ↔ 0x00 0x00 0x04 0x00 (BE) = 262144배 차이로 viewport 좌표 폭주. (ii) paneId 누락 — R3는 envelope 구조 자체를 잘못 그림. 9B vs 14B 차이로 디코더가 길이 검증에서 reject.
- **권고**:
  - **옵션 A** — SSoT를 BE로 수정. JS `DataView.getInt32(offset)` 기본이 BE이므로 프런트 코드 simpler.
  - **옵션 B** — R8 + R3를 LE로 수정. SSoT 정본 유지. Rust `i32::from_le_bytes` + JS `DataView.getInt32(offset, true)` (littleEndian=true 명시).
  - **추천: 옵션 B**. 근거: (i) SSoT가 wire-protocol *1차 계약*이며 이미 32개 슬롯 전부 정의 완료. (ii) Rust 측 zero-copy `bytes` LUT 패턴은 native endian 가정이 자연 — x86_64/aarch64 모두 LE이므로 *memcpy 1회로 read*. (iii) R3/R8는 *프런트엔드 사이드 추정*이므로 SSoT가 정본. (iv) LEB128 varint도 little-endian 그룹이라 envelope 전체가 LE로 일관.
  - **정정 작업**:
    1. `docs/reports/0008-frontend-stack.md` F4 (line 255) — `(BE, 12B)` → `(LE, 12B)`. line 561 `int32 BE, float32 BE` → `int32 LE, float32 LE`.
    2. `docs/reports/0003-infinite-canvas.md` 예제 (line 243) — 본문 `[int32 x BE]...` → `[varint paneId=0][int32 x LE][int32 y LE][float32 zoom LE]` + "총 14B" 정정.
    3. R7 §9 시나리오에 endian unit test 명시 추가(작은 보강).

### B3 (Blocking). R8 F8 zoom-blur 정책이 ADR/sketch 본문에 미전파

- **현상**: §8 표 참조. R8이 (b) placeholder 정책으로 closed했으나 ADR-0012/sketch.md §14/CONTEXT.md 어디에도 명문화되지 않음. R3 §F7가 "R8 검증 항목"으로 남기고, R8 F8이 결정하지만, *ADR로 격상되지 않음*.
- **영향**: 배치 C(코드 부트스트랩)에서 Panel.svelte 작성자가 "어디 정책을 따라야 하는가?"라는 질문을 R3·R8·ADR-0012 사이에서 무작위 선택. xterm DOM 인스턴스 retain vs unmount 선택이 50 pane 메모리 footprint(R8-O3)에 직접 영향.
- **권고**:
  - **옵션 A** — 신규 ADR-0004(터미널 렌더링)을 즉시 발행하고 zoom 정책을 D-항목으로 포함. plan A4가 ADR-0001/0002/0003 외 추가 ADR 발행을 가정하지 않으므로 **plan 수정 동반**.
  - **옵션 B** — ADR-0012 §결과 또는 §"미해결" 잠금 항목으로 "zoom-blur는 R8 F8 (b) placeholder on zoom (ε=0.02) 정책으로 closed" 한 줄 추가. ADR-0012 Status 승격 시 R8을 *normative reference*로 인용.
  - **추천: 옵션 B**. 근거: (i) plan A4 scope는 ADR-0001~0003 + SSoT 정합만이며 신규 ADR 발행은 trigger 외. (ii) ADR-0012 D4가 이미 xterm.js를 ADR-0004로 위임하므로 *normative reference 1행*이 가장 작은 변경. (iii) sketch.md §14는 1차 자료 amend 정책상 보수적으로 두고, ADR로 충분.
  - **정정 작업**:
    1. `docs/adr/0012-frontend-stack-svelte.md` §결과 또는 §미해결 O1에 "zoom-blur 정책 = R8 F8 (b) placeholder on zoom (ε=0.02, Streaming State와 직교)" 한 줄 추가.
    2. (선택) `CONTEXT.md` "Panel Streaming State" 인근에 "Zoom-mode visibility는 streaming state와 직교, 데이터 흐름 유지" 1행.

### A1 (Advisory). ADR-0011 O7 enum↔allowlist 1:1 검증 (2차 G9 carry-forward)

- **현상**: 2차 G9이 "`mux-router::Command` enum variant 집합 = ADR-0008 allowlist ALLOW 행 9개와 정확히 일치, 정적 매핑 테스트로 강제"를 ADR-0011 O7 측정 기준에 추가 권고. R7 §8 (T7)가 7-crate scaffolding을 잠갔으나 *enum 정적 매핑 테스트 명시*는 아직 ADR-0011 본문에 없음.
- **권고**: ADR-0011 §"미해결" O7 측정 기준 1행 추가 — "`mux-router::Command` enum variant 집합 = `docs/ssot/security-defaults.md` §1.7 allowlist 11행과 정확히 일치. `cargo test`로 정적 매핑 검증". (ADR-0008가 정본이나 SSoT가 코드 readable 형태 — 둘 다 인용 가능)

### A2 (Advisory). Schema 도구체인 R7/R8 모순

- **현상**: R7 §6 (T5)가 `utoipa 5.5.0 + openapi-typescript`(HTTP API 우선 OpenAPI 3.1 → TS) 채택. R8 F2 (T2)가 `schemars + json-schema-to-typescript`(JSON Schema 우선 → TS) 채택. 같은 codegen 파이프라인의 *두 끝*인데 서로 다른 도구 선택.
- **영향**: 코드 부트스트랩 시 backend는 `utoipa` 매크로 + `cargo run --bin gen-openapi` → `docs/ssot/openapi.yaml` 산출, frontend는 `schemars` + `cargo run --bin gen-schema` → `docs/ssot/canvas-layout.schema.json` 산출. **두 산출 파일·두 도구·두 codegen path 동시 유지** → SSoT 정본 *분기* 위험. 1차/2차 coherence가 도구체인을 R7/R8에 위임했으나 *최종 합치 검증*은 본 A4 책임.
- **권고**:
  - **옵션 A** — `utoipa`로 단일 (R7 §6 채택). HTTP API surface는 `GET/PUT /api/layout` 2개라 OpenAPI가 자연 — `utoipa::ToSchema`가 JSON Schema 부분 산출도 가능(`utoipa::openapi::schema` 직접 추출). frontend는 `openapi-typescript` (TS 타입 + `openapi-fetch` 호출 코드) 자동 생성.
  - **옵션 B** — `schemars`로 단일 (R8 F2 채택). HTTP API spec은 *수동 OpenAPI 문서*로 별도 유지 + JSON Schema는 `schemars` 산출. Draft 2020-12 미지원 위험 (2차 G10).
  - **옵션 C** — 둘 다 유지 (R7 + R8 그대로). codegen path 2개.
  - **추천: 옵션 A**. 근거: (i) R7이 R8 이전 결정이고 ADR-0011 D5가 "OpenAPI 우선" 명시. (ii) `openapi-fetch`로 호출 코드까지 자동 — frontend의 `lib/http/layout.ts` 작성 비용 ↓. (iii) Draft 2020-12 호환성 검증을 `utoipa`가 OpenAPI 3.1 spec 내장으로 회피 — 2차 G10 자연 해소.
  - **정정 작업**: ADR-0012 D7 + Open O2를 `utoipa` 단일로 좁히고 R8 F2를 *후속 amend*로 supersede. 또는 ADR-0011/0012 양쪽에 "R7 §6이 정본, R8 F2는 supersede" 1행 추가.

### A3 (Advisory). 보고서 0007/0008/0011/0012 번호 점유 (2차 G11 carry-forward)

- **현상**: `reports/0007` ↔ `adr/0007`, `reports/0008` ↔ `adr/0008`, `reports/0011` ↔ `adr/0011`, `reports/0012` ↔ `adr/0012` 네 쌍이 동시 점유. 2차 G11 권고("0013+ 사용")가 R7/R8 작성에 미반영.
- **권고**: 본 보고서가 새로 도입한 reference도 "ADR-NNNN" / "R<N>" / "보고서 NNNN" prefix 강제. plan 0002에 인용 컨벤션 1행 명시 권고. 이미 발행된 R7/R8 파일명 *변경하지 않음* (외부 인용 깨짐 방지).

### A4 (Advisory). `pause-after` 임계값 잠정 (ADR-0001 D9·O2)

- **현상**: ADR-0001 D9 잠정값 10초, R7-O2 측정 trigger 명기.
- **권고**: 현 상태 유지. R7 §9.6 시나리오 실행 후 ADR-0001 amend.

### A5 (Advisory). Long-suspend buffer disconnect (ADR-0001 D8·O1)

- **현상**: ADR-0001 D8 검증 항목 + O1, R7 U3 carry-forward.
- **권고**: 현 상태 유지. R7 §9.6 + U3 측정 후 ADR-0001 D8 amend (`pause` → `off` 자동 승격 도입 여부).

### A6 (Advisory). 2차 G7/G8/G12/G13 carry-forward

- **현상**: 2차 coherence가 식별한 G7(SSoT `effective lock = OR` 미반영)·G8(grill 0010 D11 미정정)·G12(plan §A0.6 DoD `#5 N/A` 단서)·G13(sketch §11.2.C "window별 grouping")는 본 A4 검토 시점에도 *미해소 상태*.
- **권고**: 본 A4 산출물과 별개로 PM이 일괄 fix commit. 본 A4 Blocking 3건과 동일 PR에 묶을 수 있음.

### C1 (Cosmetic). sketch.md §13.4 line 624 `${XDG_CONFIG_HOME}/gtmux/<session>.token`

- **현상**: §9 표 참조. ADR-0003 D4가 명시적으로 supersede했으나 sketch 원문 1행은 옛 경로 보유.
- **권고**: sketch §13.4 line 624 → `${XDG_STATE_HOME}/gtmux/<session>.token` + "(D17 정정, ADR-0003 D4 참조)" 1행. 1차 자료 amend 정책상 *변경 이력* 절에 자기 갱신 기록.

### C2 (Cosmetic). 인용 컨벤션 plan 명시

- **현상**: A3 참조.
- **권고**: plan 0002 §0 또는 §1·§2 첫 행에 "이 plan 안 모든 인용은 ADR-NNNN / R<N> / 보고서 NNNN prefix" 1행.

## 5대 불변식 매트릭스 정합 (배치 A 누적)

배치 A0(ADR-0007/0008/0009/0010/0011/0012) + 배치 A1·A2·A3(ADR-0001/0002/0003) = ADR 9개 × 5 = 45칸.

| ADR | #1 | #2 | #3 | #4 | #5 |
|---|---|---|---|---|---|
| 0001 | PASS | PASS | PASS | PASS | **PASS (강함)** |
| 0002 | **PASS (강함)** | PASS | PASS | PASS | PASS |
| 0003 | PASS | PASS | PASS (trivially) | **PASS (강함)** | PASS |
| 0007 | PASS | PASS | PASS | PASS | PASS |
| 0008 | PASS | PASS | PASS | PASS | PASS |
| 0009 | PASS | PASS | PASS (trivially) | **PASS (강함)** | PASS |
| 0010 | PASS | PASS | PASS | PASS | PASS |
| 0011 | PASS | PASS | PASS | **PASS (강함)** | PASS |
| 0012 | PASS | PASS | **PASS (강함)** | PASS | N/A |

→ 44 PASS + 1 N/A (ADR-0012 #5, 본문 사유 명시 — 2차 G12 carry-forward). 검증 강도 "PASS (강함)" 8칸 모두 *근거 단락 동반*. *placeholder PASS 없음*.

## 미해결 (배치 C 진입 전 해소 필요)

본 A4 보고서가 식별한 Blocking 3건(B1·B2·B3) + 2차 carry-forward 4건(G7·G8·G12·G13) = 총 **7건 fix commit**이 배치 C 진입 게이트.

| 항목 | 수정 대상 파일 | 작업량 |
|---|---|---|
| B1 토큰 서브프로토콜 | ADR-0003 D5 + SSoT §1.3 + SSoT §3 JSON | 3 곳 1행씩 |
| B2 VIEWPORT_CHANGED endian | R8 F4 (2 곳) + R3 예제 (1 곳) | 3 곳 |
| B3 zoom-blur 정책 | ADR-0012 §결과 또는 O1 | 1행 |
| G7 (2차) `effective lock = OR` | CONTEXT.md line 134 + SSoT canvas-layout-schema.md line 87 + ADR-0010 변경 이력 | 3 곳 |
| G8 (2차) grill 0010 D11 | 옵션 (b) ADR-0010 §맥락 1행 | 1 곳 |
| G12 (2차) plan §A0.6 DoD | plan 0002 line ? | 1 곳 |
| G13 (2차) sketch §11.2.C | sketch.md line 500 | 1 단어 |
| C1 sketch §13.4 token path | sketch.md line 624 | 1행 |

본 A4의 Advisory 6건(A1~A6) + Cosmetic 2건(C1·C2)은 배치 C와 *병렬* 진행 가능 (배치 C 진입 차단 아님).

## 배치 C 진입 권고

**조건부 YES**.

배치 C(코드 부트스트랩 = `codebase/` 안 Rust workspace + Svelte frontend skeleton)는 다음 7건 fix commit 직후 진입 가능:

1. **B1 fix** — ADR-0003 D5 + SSoT 2 곳, 토큰 subprotocol 포맷을 ADR-0002 D5와 동일하게 (옵션 A 추천).
2. **B2 fix** — R8 F4 + R3 예제, VIEWPORT_CHANGED endian을 SSoT(LE)로 통일 + paneId varint 명시.
3. **B3 fix** — ADR-0012에 zoom-blur 정책 1행 추가.
4. **G7 fix** — CONTEXT.md + canvas-layout-schema.md `effective lock = OR` 정정 (2차 carry-forward).
5. **G8 fix** — ADR-0010 §맥락에 grill D11 lock 의미 정정 1행 (2차 carry-forward).
6. **G12 fix** — plan 0002 §A0.6 DoD에 `#5 N/A` 단서 (2차 carry-forward).
7. **G13 fix** — sketch §11.2.C "window별 grouping" → "Group 트리" (2차 carry-forward).

위 7건은 모두 *1-3 행 수정*이며 본 A4 발행 직후 단일 PR로 묶을 수 있다. 7건 fix 후 ADR-0001/0002/0003 + ADR-0011/0012 Status를 **Proposed → Accepted**로 승격 가능.

배치 C 진입 후 *병렬* 처리:
- A1·A2 (R7/R8 후속 보강 — enum 매핑 테스트, schema 도구 단일화)
- A4·A5 (R7 §9 벤치마크 실행 후 ADR-0001 amend)
- C1·C2 (sketch §13.4 token path, plan 인용 컨벤션)

## 변경 이력

- 2026-05-13: 초안 (A4, self-review). Blocking 3건·Advisory 6건·Cosmetic 2건 식별. 5대 불변식 45칸 중 44 PASS + 1 N/A.
