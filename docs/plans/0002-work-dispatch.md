# 업무 분배 (PM 디스패치)

> 본 문서는 PM이 분야별 전문가에게 업무를 할당하기 위한 작업 정의서다. 각 업무는 **담당 / 입력 / 산출물 / 프롬프트 / 양식 / 완료 기준(DoD)** 6단으로 정의한다. 본 문서는 작업을 직접 수행하지 않는다.

## 공통 규칙 (모든 담당자)

- 산출물 언어: **한국어**. 코드/식별자는 영어.
- 인용: 1차 출처 우선, URL + 접근일자 필수.
- 모든 결정은 `docs/sketch.md` §4·§8·§13의 5대 불변식과 정합해야 한다.
- 산출물 경로는 본 문서가 지정한 그대로 사용. 임의 변경 금지.
- 완료 시 PM에게 산출물 경로 + 핵심 결정 3줄 요약 반환.

## 0. 배치 A0 — Foundational ADRs (Grill 산출물, 배치 A·B 선행)

**근거**: `docs/reports/0010-grill-amendments.md` §3. Grill 결과로 도메인·UX 4대 결정이 굳어졌고, 배치 A·B의 ADR들이 이 결정을 *입력 제약*으로 참조해야 한다. 따라서 다음 4개 ADR을 **배치 A·B보다 먼저** 발행한다.

### Task A0.1. ADR-0007 — Server : Session : Port 1:1:1 바인딩 모델
- **담당**: `system-architect`
- **입력**: `docs/reports/0010-grill-amendments.md` D1·D2·D3
- **산출물**: `docs/adr/0007-server-session-port-binding.md`
- **프롬프트**: 보고서 §3 ADR-0007 (Proposed) 항목을 ADR 템플릿(§3)에 맞춰 한국어로 정식 작성. Decisions / Rejected / Consequences / 불변식 검증 / Open 절 채움. 불변식 5개 전부 PASS 확인.
- **DoD**: 5대 불변식 PASS 명시. "Open" 절에 잔여 항목 0개 또는 명시적 후속 ADR 참조.

### Task A0.2. ADR-0008 — Single-pane-per-tmux-window + Group 기반 UI 계층
- **담당**: `system-architect`
- **입력**: `docs/reports/0010-grill-amendments.md` D4·D8·D9. R1 보고서 §9 (iTerm2 한계).
- **산출물**: `docs/adr/0008-single-pane-window-and-group.md`
- **프롬프트**: 보고서 §3 ADR-0008 (Proposed)를 정식 ADR로. 거절 대안 명시(P-γ multi-pane, flat window 노출). gtmux backend의 tmux command allowlist 표(허용/금지) 포함. 외부 multi-pane mirror 정책 명시.
- **DoD**: command allowlist 표가 §10.1 백엔드 구성과 정합. 외부 multi-pane resize lock UX 정책 명시.

### Task A0.3. ADR-0009 — tmux daemon 격리 모델 (Dedicated daemon per Server)
- **담당**: `system-architect` (또는 `devops-architect`)
- **입력**: `docs/reports/0010-grill-amendments.md` D10. 실측 footprint 데이터(보고서 D10 참조).
- **산출물**: `docs/adr/0009-tmux-daemon-isolation.md`
- **프롬프트**: 보고서 §3 ADR-0009 (Proposed)를 정식 ADR로. 소켓 경로 컨벤션(`-L gtmux-<session>`)과 부팅·종료·teardown CLI 절차 명시. 50 Server 시나리오 footprint 추정 인용.
- **DoD**: `gtmux teardown` CLI의 socket 파일 명시 rm 단계 절차 명시.

### Task A0.4. ADR-0010 — Group 데이터 모델 (G-hybrid)
- **담당**: `frontend-architect` (또는 `system-architect`)
- **입력**: `docs/reports/0010-grill-amendments.md` D11. CONTEXT.md "Group 운영 규칙" 절.
- **산출물**: `docs/adr/0010-group-data-model.md` + `docs/ssot/canvas-layout-schema.md` (영속화 스키마 SSoT)
- **프롬프트**: 보고서 §3 ADR-0010 (Proposed)를 정식 ADR로. G-hybrid의 자체 상태(label/color/visibility/locked/order/parent_id) 명시, AND 전파 규칙, multi-select group/ungroup UX, Group 이동 = drag delta 일괄 적용. SSoT는 `groups: [...]` + `panels: [...]` JSON Schema 형태로.
- **DoD**: SSoT의 JSON Schema가 HTTP `PUT /api/layout` 페이로드와 정확히 매칭. ADR-0006(영속화)에서 이 schema를 그대로 참조.

### Task A0.5. ADR-0011 — Backend stack (Rust + axum + tokio)
- **담당**: `backend-architect`
- **입력**: `docs/reports/0010-grill-amendments.md` D18. 본 grill의 후보 비교 표 그대로.
- **산출물**: `docs/adr/0011-backend-stack-rust.md`
- **프롬프트**: 보고서 §3 ADR-0011 (Proposed) 항목을 ADR 템플릿(§3)에 맞춰 한국어 정식 작성. Decisions / Rejected / Consequences / 불변식 검증 / Open. 특히 거절 대안 (Bun, Node, Go, Python)을 D18 표 그대로 인용.
- **DoD**: 5대 불변식 PASS. Open 절에 R7 보고서로 검증할 항목(specific crate version, benchmark target) 명시.

### Task A0.6. ADR-0012 — Frontend stack (Svelte 5 + Vite + TS)
- **담당**: `frontend-architect`
- **입력**: `docs/reports/0010-grill-amendments.md` D18.
- **산출물**: `docs/adr/0012-frontend-stack-svelte.md`
- **프롬프트**: 보고서 §3 ADR-0012 (Proposed) 항목을 ADR 템플릿(§3)에 맞춰 한국어 정식 작성. 거절 대안 (React, Vue, Solid.js, Vanilla TS) D18 표 그대로 인용. Canvas 라이브러리는 R3 결과에 따라 결정 — 본 ADR은 *프레임워크 선택*만 잠그고 lib은 Open으로 남김.
- **DoD**: 5대 불변식 PASS 또는 N/A(사유 본문 명시 필수 — 예: #5 control mode 사용은 프론트엔드와 무관할 수 있음). Open 절에 R8 보고서로 검증할 항목(signals 패턴, 캔버스 lib 정합) 명시.

### Task A0.7. 배치 A0 정합성 리뷰 (PM 게이트)
- **담당**: `self-review` (또는 PM 직접 수행)
- **입력**: A0.1·A0.2·A0.3·A0.4 산출물 + A0.5·A0.6 산출물
- **산출물**: `docs/reports/0011-coherence-review.md` (1차 작성 완료, 2026-05-13) + A0.5·A0.6 산출물에 대한 후속 정합 점검
- **상태**: 1차 (A0.1~A0.4) ✅ **완료** — 갭 6건 발견·해소 적용:
  - G1 (Blocking): `docs/ssot/canvas-layout-schema.md` 초안 작성 → ADR-0010 Open O2 closed
  - G2: dispatch §A1·A2·A3 **입력** line에 `0010-grill-amendments.md` 명시
  - G3: A2 프롬프트에 "R4 supersession 명기" 지시 추가
  - G4: SSoT §2 ETag 정규화 규칙 + A2 프롬프트 인용
  - G5: sketch.md §4.3 amend 완료 검증 (12개 어휘 확인)
  - G6: dispatch `## 0. 공통 규칙` → `## 공통 규칙` 재번호
- 2차 (A0.5·A0.6 ADR-0011/0012): A0.5·A0.6 산출 후 추가 점검 필요. 본 보고서에 §7 (또는 추가 보고서)로 보강.
- **DoD**: D-결정·ADR 본문 모순 0. 배치 A·B 진입 허용.

---

## 1. 배치 A — 1차 ADR 묶음 (배치 A0 후 착수)

**선행조건**: `docs/reports/0001-tmux-control-mode.md`, `0004-transport.md`, `0005-security-model.md` 존재 (충족) + **배치 A0 완료** + `docs/reports/0010-grill-amendments.md` 참조 가능.

세 ADR은 서로 참조하므로 **동일 사이클에서 함께 작성**, 작성 후 PM이 정합성 리뷰.

### Task A1. ADR-0001 tmux 통합 = 컨트롤 모드

- **담당**: `system-architect`
- **입력**: `docs/reports/0001-tmux-control-mode.md`, **`docs/reports/0010-grill-amendments.md` D8·D15·D16** (single-pane allowlist, ring buffer, Panel Streaming State), `docs/sketch.md` §10·§11.2
- **산출물**: `docs/adr/0001-tmux-integration-control-mode.md`
- **프롬프트**:
  ```
  Write ADR-0001 fixing gtmux's tmux integration strategy. Read
  docs/reports/0001-tmux-control-mode.md as the evidence base; do not re-research.
  The decision is already implied by report §"구체 권장" — your job is to
  translate it into an ADR with crisp Decisions, Consequences, and a Rejected
  Alternatives section. Korean. Use the ADR template in §3 of this dispatch doc.

  Decisions must cover: (1) `tmux -C` as the sole channel, (2) minimum tmux
  version 3.2 / recommended 3.4+, (3) bootstrap order (list-* snapshot →
  pause-after enable → -B subscriptions → live notifications), (4) single FIFO
  command queue with command-number matching, (5) `-CC` forbidden in backend,
  (6) `refresh-client -C` not called in MVP, (7) `%output` decode → per-pane
  ring buffer → binary WS frame, (8) `%pause` UX policy.

  Reject explicitly: screen-scraping, repeated shell-outs of `tmux list-*`,
  `tmux -CC` for the backend.

  Additional input constraints from grill report
  (`docs/reports/0010-grill-amendments.md`):
  - D8 (single-pane-per-window): tmux command allowlist excludes
    `split-window`, `resize-pane`, `select-layout`.
  - D15 (ring buffer): per-pane ring buffer is 128 KB default,
    user-configurable. Server-memory only, no disk persistence.
  - D16 (Panel Streaming State): pane visibility/minimize transitions trigger
    `refresh-client -A '%pid:pause/continue'` (debounce 300ms).
    Verify tmux long-pause buffer behavior + CONTROL_BUFFER_HIGH/disconnect
    interaction in manual-pause case; document result in ADR.
  ```
- **DoD**: ADR이 5대 불변식 검증을 포함한다. "Decisions"의 모든 항목이 보고서 인용 번호를 단다.

### Task A2. ADR-0002 전송 계층 = WebSocket + 이진 envelope

- **담당**: `backend-architect`
- **입력**: `docs/reports/0004-transport.md`, **`docs/reports/0010-grill-amendments.md` D12·D13·D14** (T-mixed 영속화 전송, MT-3, WS 0x80–0x8F 슬롯), ADR-0001 (Task A1 완료 후)
- **산출물**:
  - `docs/adr/0002-transport-websocket.md`
  - `docs/ssot/wire-protocol.md` (envelope type 코드 표 SSoT)
- **프롬프트**:
  ```
  Write ADR-0002 + the wire-protocol SSOT. Read docs/reports/0004-transport.md
  and docs/adr/0001-* as evidence. Korean.

  ADR Decisions:
  - WebSocket (RFC 6455) over a single socket as the MVP transport.
  - Reject SSE, WebTransport (MVP), HTTP/2-WS, long-polling — with the report's
    reasoning condensed.
  - Auth token via `Sec-WebSocket-Protocol` subprotocol (align with ADR-0003).
  - Origin and Host whitelist enforced at the upgrade.
  - Binary frame envelope; full code table lives in docs/ssot/wire-protocol.md.
  - Backpressure: server-side per-pane queue high/low watermark → tmux
    `refresh-client -A '%id:pause|continue'` per ADR-0001.
  - Reconnect: backend tmux session is long-lived; only the browser WS reconnects.

  SSOT (wire-protocol.md) must specify:
  - Frame format: `[1B type][varint paneId|0][payload bytes]`.
  - Type code table with two regions: 0x01–0x0F tmux-domain (CTRL, PANE_OUT,
    PANE_IN, PANE_RESIZE, PAUSE, RESUME, NOTIFY_MIRROR), 0x80–0x8F web-domain
    (PANEL_GEOMETRY, LAYOUT_ACK, VIEWPORT, FOCUS).
  - Sequence number policy and reconnect resume token format (note as TBD if
    persistence ADR not yet written).
  - Explicit "must never appear in this protocol" list — layout strings as
    canvas geometry, shell command strings, free-form tmux command strings
    (only argv arrays).

  Layout persistence transport: **resolved by grill D12 — HTTP** (`GET/PUT
  /api/layout` + ETag). WS web-domain envelope carries notify only, not
  durable writes. See `docs/reports/0010-grill-amendments.md` D12.

  Note in §맥락 of the ADR: "R4 보고서 §본문의 `POST /layouts` 단정은
  grill D12에 의해 supersede되었다. 본 ADR은 PUT 전체 교체 + ETag 모델을
  채택한다." 코히런스 리뷰 G3 참조.

  ETag normalization (G4 해소): 16-byte raw가 정본. HTTP JSON에서는
  lowercase hex 32자, WS envelope에서는 raw bytes. 자세한 규칙은
  `docs/ssot/canvas-layout-schema.md` §2를 참조하고, wire-protocol SSoT의
  0x80 LAYOUT_CHANGED payload 정의에 같은 규칙을 명기.

  Wire-protocol SSOT web-domain region (0x80–0x8F) is now fully defined by
  grill D14:
  - 0x80 LAYOUT_CHANGED: `etag(16B)` — durable HTTP layout updated notify
  - 0x81 M_CHANGED: `varint count + varint panel_ids[]` (MT-3 broadcast)
  - 0x82 I_CHANGED: `varint pane_id (0=null)` (MT-3 broadcast)
  - 0x83 VIEWPORT_CHANGED: `int32 x, int32 y, float32 zoom` (MT-3 broadcast)
  - 0x84 FOCUS_MODE_CHANGED: `1B enabled, varint target_panel_id` (MT-3)
  - 0x85–0x8F reserved
  All web-domain messages are bidirectional and broadcast to all WS
  connections including originator (MT-3 D13: single-user, no client
  identity distinction).

  Reconnect resume token: not needed for MVP. Server holds ephemeral
  state in memory; new WS attach gets full state push (HTTP GET for
  durable, WS replay for ring buffer per D15).
  ```
- **DoD**: SSoT의 type 코드 표가 32개 슬롯을 모두 정의(예약 슬롯 포함). ADR이 R4 보고서의 "미해결 7개" 중 어떤 것을 정하고 어떤 것을 미루는지 명시.

### Task A3. ADR-0003 보안 디폴트

- **담당**: `security-engineer`
- **입력**: `docs/reports/0005-security-model.md`, **`docs/reports/0010-grill-amendments.md` D17** (인증 모델 확정 사항), ADR-0002 (Task A2 완료 후 — 인증 패턴 정합)
- **산출물**:
  - `docs/adr/0003-security-defaults.md`
  - `docs/ssot/security-defaults.md` (12개 체크리스트 SSoT)
- **프롬프트**:
  ```
  Write ADR-0003 + security-defaults SSOT. Read docs/reports/0005-security-model.md
  as evidence; do not re-research. Korean.

  ADR Decisions must include all 12 items of report §"안전한 기본값 체크리스트"
  verbatim, plus two additions the reviewer flagged:
  - Verify whether tmux honors `--` end-of-options consistently across the
    allowlisted subcommands; if not, fall back to strict per-command argv
    schemas. Document the verification result inline.
  - `connect-src` policy in CSP for both local (`'self'`) and cloud
    (`wss://<configured-host>`) modes.

  Reject: shell invocation of tmux, query-string tokens, wildcard Origin,
  `dangerouslySetInnerHTML`, OSC 52 auto-enable, running as root without
  explicit opt-in.

  Token policy: **resolved by grill D17 — local mode = regenerate on every
  Server start (Jupyter style); cloud mode = persistent + explicit rotation
  command** (`gtmux rotate-token --session <name>`). Token = 256-bit CSPRNG,
  base64url, stored at `${XDG_STATE_HOME}/gtmux/<session>.token` (perm 0600,
  D20 디렉터리 컨벤션 — CONFIG는 사용자 편집 가능, STATE는 머신 발급 자료),
  constant-time comparison. WS handshake transports token via
  `Sec-WebSocket-Protocol` subprotocol; HTTP uses `Authorization: Bearer
  <token>` with secondary `SameSite=Strict` HttpOnly cookie. See
  `docs/reports/0010-grill-amendments.md` D17.

  OS auth delegation (PAM/SSH): out of MVP scope (single-user assumption);
  revisit in P1+ for cloud mode if needed.

  Other open items still deferred:
  - Unix socket as a first-class mode vs supplementary.
  - Default tmux command palette whitelist (exact list — constrained by
    ADR-0008 to exclude split-window/resize-pane/select-layout).

  SSOT (security-defaults.md) is a flat key→value config table the
  implementation must read at startup: bind target, port range, socket path,
  token file path, header allowlist values, CSP string template, xterm.js
  option flags, tmux blocklist, identifier regex set.
  ```
- **DoD**: ADR이 ADR-0002의 인증 메시지 흐름과 모순 없음. SSoT가 구현이 직접 `require()` 할 수 있는 구조(JSON 또는 표).

### Task A4. 정합성 리뷰 (PM 게이트)

- **담당**: `self-review` (PM이 호출)
- **입력**: A1·A2·A3 산출물 전체
- **산출물**: `docs/reports/0009-adr-coherence-review.md`
- **프롬프트**:
  ```
  Cross-check ADR-0001, ADR-0002, ADR-0003 and the two SSOT files for
  consistency. Korean. Verify specifically:
  - Auth token mechanism described identically in ADR-0002 and ADR-0003.
  - tmux command invocation: ADR-0001 says "control mode commands", ADR-0003
    says "argv array + allowlist" — confirm the wire envelope in
    wire-protocol.md carries argv arrays, not strings.
  - Backpressure: ADR-0001's pause-after and ADR-0002's queue watermarks
    reference the same tmux `refresh-client -A` calls.
  - Layout persistence surface is not double-defined.
  - Each ADR cites its source report and lists the 5 invariants with verdicts.

  Report format: a checklist of cross-references with PASS/FAIL/AMBIGUOUS and
  the file:line citation. No new decisions.
  ```
- **DoD**: 모든 항목 PASS, 또는 AMBIGUOUS 항목에 대해 후속 task 발행.

---

## 2. 배치 B — 2차 리서치 (배치 A와 병렬 가능)

핸드오프 문서는 이미 `docs/src/prompt_research_handoff.md`에 있다. 보고서 양식은 동 문서 §3을 사용.

### Task B1. R2 — 브라우저 터미널 렌더링

- **담당**: `deep-research` (또는 `frontend-architect` 위임)
- **입력**: `docs/src/prompt_research_handoff.md` §4 R2 블록
- **산출물**: `docs/reports/0002-terminal-rendering.md`
- **프롬프트**: 핸드오프 문서 §4 R2 그대로 사용. 추가 제약:
  ```
  Constraint from PM: the recommendation must be compatible with the binary
  wire envelope defined in docs/ssot/wire-protocol.md (if it exists at read
  time) — pane output arrives as raw bytes, not JSON. Validate that xterm.js
  `write(Uint8Array)` is the consumption path.
  ```
- **DoD**: 권장안 1개 + 거절 후보 명시. 50-pane 동시 시나리오의 메모리/프레임 추정치 포함.

### Task B2. R3 — 무한 캔버스

- **담당**: `deep-research` (또는 `frontend-architect` 위임)
- **입력**: `docs/src/prompt_research_handoff.md` §4 R3 블록
- **산출물**: `docs/reports/0003-infinite-canvas.md`
- **프롬프트**: 핸드오프 문서 §4 R3 그대로 사용. 추가 제약:
  ```
  Constraint from PM: the cut-off filter is "can host an arbitrary DOM subtree
  (xterm.js mounts a <div>) as a node while participating in pan/zoom".
  Libraries that force canvas/WebGL rendering of node contents are eliminated
  early. State the eliminated set explicitly.
  ```
- **DoD**: 랭크 + 결정적 기준 명시. 시리얼라이즈 포맷 샘플 JSON 포함.

### Task B3. R6 — 레이아웃 영속화

- **담당**: `deep-research` (또는 `backend-architect` 위임)
- **입력**: `docs/src/prompt_research_handoff.md` §4 R6 블록, **`docs/reports/0010-grill-amendments.md` D11·D12** (Group 데이터 모델, T-mixed HTTP), `docs/ssot/canvas-layout-schema.md` (ADR-0010 부속), ADR-0002 (가능하면)
- **산출물**: `docs/reports/0006-layout-persistence.md`
- **프롬프트**: 핸드오프 문서 §4 R6 그대로 사용. 추가 제약:
  ```
  Transport: **resolved by grill D12 — HTTP** (`GET/PUT /api/layout` +
  ETag). WS only carries `LAYOUT_CHANGED` notify (envelope 0x80 per D14).
  Report should justify storage backend (sqlite vs file json vs other),
  schema (must match grill D11 G-hybrid: groups + panels tree), migration
  policy (single-version MVP, no migration needed), backup strategy.

  Schema input from grill D11:
  - `groups: [{id, parent_id|null, label, color|null, visibility, locked, order}]`
  - `panels: [{id, parent_id|null, x, y, w, h, z, visibility, locked, label, note, ...}]`

  Tree integrity (no multi-parenting, no cycles) must be enforced at PUT
  validation time.

  See `docs/reports/0010-grill-amendments.md` D11·D12.
  ```
- **DoD**: 권장 스키마(SQL DDL 또는 JSON Schema) + 마이그레이션 정책 + WS vs HTTP 결론.

### Task B4. R7 — 백엔드 런타임 검증 (Rust crate set + benchmark + scaffolding)
- **담당**: `deep-research` (또는 `backend-architect`)
- **입력**: `docs/reports/0010-grill-amendments.md` D18, ADR-0011 (A0.5 산출)
- **산출물**: `docs/reports/0007-backend-runtime.md`
- **프롬프트**:
  ```
  Verify ADR-0011's Rust stack: tokio + axum + tokio-tungstenite +
  tower-http + serde + clap + tracing + ring/rustls. Korean.

  Scope (narrowed by D18 — 후보 비교 단계 종결):
  1. Specific crate versions (compatible set, MVP target: rust 1.80+).
  2. 50-pane simulation benchmark: 메모리/CPU/latency 측정 시나리오 설계.
     실제 코드 작성·실행은 별도 task. 본 보고서는 *측정 계획*만.
  3. tmux control mode 파서 + per-pane ring buffer (D15)의 Rust 구현 패턴
     (Bytes/BytesMut, channels, backpressure).
  4. axum + tower-http 미들웨어 체인: Origin/Host/CSRF/Authorization Bearer
     검증 위치 + ETag (RFC 7232) middleware 구성.
  5. tokio-tungstenite의 binary frame 처리 + WebSocket subprotocol
     (Sec-WebSocket-Protocol) 토큰 검증 위치.
  6. Cross-compile: cargo-zigbuild로 macOS/Linux 단일 바이너리.
  7. Scaffolding: 디렉터리 구조 + Cargo.toml workspace 안 추천 모듈
     분리 (mux-router, ws-server, http-api, lifecycle, config, auth).

  Reject explicitly: 새 언어/런타임 비교 (D18 결정 supersede).
  ```
- **DoD**: crate set 확정 + benchmark 설계 + scaffolding outline. ADR-0011 Open 항목 closed.

### Task B5. R8 — 프론트엔드 스택 검증 (Svelte 5 signals + canvas lib + xterm.js)
- **담당**: `deep-research` (또는 `frontend-architect`)
- **입력**: `docs/reports/0010-grill-amendments.md` D18, ADR-0012 (A0.6 산출), R3 보고서 (`docs/reports/0003-infinite-canvas.md`, B2 산출)
- **산출물**: `docs/reports/0008-frontend-stack.md`
- **프롬프트**:
  ```
  Verify ADR-0012's Svelte 5 + Vite + TS stack with R3's canvas library
  result. Korean.

  Scope (narrowed by D18 — 후보 비교 단계 종결):
  1. Svelte 5 signals 사용 패턴: M·I·viewport·focus·N개 Panel state를
     동시에 라이브 갱신할 때의 store/runes 설계.
  2. xterm.js + Svelte 통합 (writable wrapper, lifecycle, addon).
  3. 캔버스 lib (R3 결과)과 Svelte의 정합 — DOM-host 요구사항 만족 여부.
  4. Vite + TS + Svelte 5 빌드 파이프라인. Rust 백엔드 schema → TS
     타입 자동 생성 (utoipa/schemars JSON Schema → TS).
  5. WS binary frame 수신 (ArrayBuffer) → envelope 디코딩 → 각 envelope
     처리기 (PANE_OUT → xterm, LAYOUT_CHANGED → HTTP GET, M/I/Viewport
     /FOCUS → store 갱신).
  6. HTTP `PUT /api/layout` 디바운스 (300ms 기본, configurable D12).
  7. Scaffolding: Vite project 구조 + Svelte component 추천 분리
     (Canvas/Panel/Sidebar/Toolbar 등).

  Reject explicitly: 다른 프론트엔드 프레임워크 비교 (D18 결정 supersede).
  ```
- **DoD**: scaffolding outline + Svelte signals 패턴 결정 + 캔버스 lib 정합 검증. ADR-0012 Open 항목 closed.

### Task B6. 배치 B → ADR 매핑 (배치 B 종료 후)

배치 B 완료 시 PM이 다음 ADR을 추가 발행한다 (현 시점에서는 정의만):

- ADR-0004 터미널 렌더링 (R2 기반) → `frontend-architect`
- ADR-0005 무한 캔버스 라이브러리 (R3 기반) → `frontend-architect`
- ADR-0006 영속화 스토리지·스키마 (R6 기반) → `backend-architect`
- ADR-0011 Accepted 승격 (R7 결과로) — Proposed였던 백엔드 stack 결정 정식화 ✅ **완료 2026-05-14 (commit `e35fad7`)**
- ADR-0012 Accepted 승격 (R8 결과로) — Proposed였던 프론트엔드 stack 결정 정식화 ✅ **완료 2026-05-14 (commit `e35fad7`)**

---

## 3. 배치 C — 코드 부트스트랩 (sketch §15 1단계 엔진 연결 검증)

**선행조건**: 9개 ADR 모두 Accepted (배치 A0/A 완료) + R2/R3/R6/R7/R8 모두 발행 (배치 B 완료) + A4 게이트 통과 (`docs/reports/0009-adr-coherence-review.md`).

이번 배치는 `codebase/` 디렉터리 안에 *실행 가능한 minimal skeleton* 을 생성한다. 비즈니스 로직은 거의 없고, **빌드 시스템·디렉터리·toolchain 고정·통합 smoke** 검증이 목표.

### Task C1. Rust backend workspace skeleton
- **담당**: `backend-architect`
- **입력**: ADR-0011 (Rust stack), R7 §8 (T7 7-crate workspace outline), ADR-0009 (daemon 격리 → `lifecycle` crate), ADR-0008 (allowlist 표 → `mux-router::Command` enum), CONTEXT.md
- **산출물**:
  - `codebase/backend/Cargo.toml` (workspace root)
  - `codebase/backend/rust-toolchain.toml` (1.85 pin, R7 §1)
  - `codebase/backend/.cargo/config.toml` (cargo-zigbuild target 설정)
  - 7 crate 디렉터리 (`crates/mux-router/`, `crates/ws-server/`, `crates/http-api/`, `crates/lifecycle/`, `crates/config/`, `crates/auth/`, `bin/gtmux-cli/`)
  - 각 crate `Cargo.toml` + `src/lib.rs` (또는 `src/main.rs` for cli) — *minimal: pub use 한 줄, 빈 함수 시그니처 1~3개*
  - 워크스페이스 `[workspace.dependencies]` (R7 §주요 crate version 표 그대로)
  - `.gitignore`(target/) + `README.md` (build 1줄 명령)
- **DoD**: `cargo build --workspace` 통과. `cargo test --workspace` 통과 (테스트 없어도 OK, 컴파일만). `cargo zigbuild --target aarch64-apple-darwin --release` 명령 dry-run으로 toolchain 검증.

### Task C2. Svelte 5 + Vite frontend skeleton
- **담당**: `frontend-architect`
- **입력**: ADR-0012 (Svelte stack), R8 §Scaffolding outline (디렉터리 트리), CONTEXT.md
- **산출물**:
  - `codebase/frontend/package.json` (svelte 5 / vite / typescript / xterm / @xyflow/svelte / openapi-typescript / openapi-fetch — R7 §6/A2 도구체인)
  - `codebase/frontend/vite.config.ts` (manualChunks: xterm, svelteflow)
  - `codebase/frontend/svelte.config.js`, `tsconfig.json` (strict)
  - R8 디렉터리 트리 그대로: `src/routes/`, `src/lib/{types,stores,ws,http,xterm,canvas,sidebar,toolbar,banner}/`, `src/styles/`
  - 각 디렉터리에 **placeholder 파일 1개씩** (`Canvas.svelte` 빈 컴포넌트, `panels.svelte.ts` 빈 store 등)
  - `codebase/frontend/codegen/README.md` (utoipa → openapi-typescript 흐름 설명, 실행은 C3)
  - `.gitignore`(node_modules/, .svelte-kit/, dist/) + `README.md` (dev/build 1줄씩)
- **DoD**: `npm install` + `npm run build` 통과 (빈 컴포넌트라도). `npm run check` (svelte-check) 통과.

### Task C3. Codegen 파이프라인 + Makefile
- **담당**: `devops-architect`
- **입력**: ADR-0011 D5 (utoipa), ADR-0012 D7 (openapi-typescript 통일 — A4 §A2), `docs/ssot/canvas-layout-schema.md`, `docs/ssot/wire-protocol.md`, `docs/ssot/security-defaults.md`
- **산출물**:
  - `codebase/Makefile` (또는 `justfile`) — `make build` / `make test` / `make codegen` / `make smoke` / `make clean`
  - `codebase/backend/bin/gen-openapi/` 또는 동등 — utoipa-derived 토이 schema 산출 binary (Group/Panel 빈 struct → OpenAPI 3.1 YAML 산출)
  - `codebase/shared/openapi.yaml` (codegen 산출 위치 placeholder)
  - `codebase/frontend/codegen/run.sh` — `openapi-typescript shared/openapi.yaml → src/lib/types/api.d.ts`
  - GitHub Actions placeholder `.github/workflows/ci.yml` (build·test·codegen 단계, 실제 PR 없으니 노드만 작성)
- **DoD**: `make codegen` 한 번 실행하면 backend → openapi.yaml → frontend types.d.ts까지 *한 사이클* 자동 진행. 실제 schema가 비어도 OK, 파이프라인 자체가 작동.

### Task C4. 통합 smoke test (1단계 종료 기준)
- **담당**: `quality-engineer` (또는 PM 직접 수행)
- **입력**: C1·C2·C3 완료
- **산출물**: `docs/reports/0012-bootstrap-smoke.md` (실측 결과 기록) + `codebase/smoke/01_engine_connect.sh` (재현 스크립트)
- **검증 시나리오**:
  1. `make build` 통과
  2. `make codegen` 통과
  3. `gtmux start --session smoke --port 9999` (foreground) 띄움 — daemon 자동 spawn 확인 (소켓 파일 존재)
  4. 별도 셸에서 `tmux -L gtmux-smoke a -t smoke` 정상 attach (외부 진입 가능 검증)
  5. 브라우저로 `http://localhost:9999/auth/bootstrap?token=<token>` (콘솔 출력 URL) 1회 접속 — `SameSite=Strict` HttpOnly Secure cookie 발급 + `/` 리다이렉트 확인. 이후 모든 요청은 cookie + `Authorization: Bearer` 헤더 (ADR-0003 D5/D6/R(rej)2 예외 절 참조).
  6. WS handshake 성공 (Sec-WebSocket-Protocol echo `gtmux.v1`)
  7. `GET /api/layout` → 빈 layout JSON 반환, ETag 헤더 포함
  8. xterm.js 1개 인스턴스 화면 표시 (실제 pane attach는 P0 후속)
  9. `gtmux teardown --session smoke` 5단계 정상 완료 (socket·token·layout·pid·config 모두 정리)
- **DoD**: 위 9 단계 모두 PASS. 실패 단계가 있으면 그 단계의 fix를 P0 작업으로 발행.

### Task C5. 배치 C 정합성 리뷰 (PM 게이트)
- **담당**: `self-review`
- **입력**: C1·C2·C3·C4 산출물 + sketch §15 1단계 success criteria
- **산출물**: `docs/reports/0013-bootstrap-coherence-review.md`
- **DoD**: 1단계 진입 조건 충족 확인 또는 후속 task 발행.

---

## 4. ADR 템플릿 (배치 A·B6 공용)

```markdown
# ADR-NNNN: <결정 한 줄>

- 상태: Proposed | Accepted | Superseded by ADR-XXXX
- 일자: YYYY-MM-DD
- 결정자: <역할>
- 근거 보고서: docs/reports/NNNN-*.md

## 맥락
<왜 이 결정이 지금 필요한가. 1~3문단. `docs/sketch.md` 어느 절과 연결되는지 인용.>

## 결정 (Decisions)
- D1. <단정문 1개. "권장" 같은 약한 표현 금지.>
- D2. ...

## 거절된 대안 (Rejected)
- R1. <후보> — <거절 이유 + 근거 보고서 인용 번호>
- R2. ...

## 결과 (Consequences)
- 긍정: ...
- 부정/비용: ...
- 후속 작업: <트리거되는 ADR/Task ID>

## 불변식 검증
| # | 불변식 | 검증 |
|---|--------|------|
| 1 | tmux 상태/웹 상태 분리 | PASS — <근거> |
| 2 | tmux-native vs web-only 분기 | ... |
| 3 | tmux 레이아웃 ≠ 캔버스 레이아웃 | ... |
| 4 | 보안 기본값 | ... |
| 5 | control mode 사용 | ... |

## 미해결 항목 (Open)
- O1. ... → ADR-XXXX 에서 결정
```

## 5. SSoT 문서 양식

```markdown
# SSoT: <도메인>

- 일자: YYYY-MM-DD
- 정의 ADR: ADR-NNNN
- 변경 정책: PR + ADR 갱신 동반. 코드는 본 문서를 참조해 구현해야 한다.

## <섹션 1: 표 또는 키-값>
| 키 | 값 | 비고 |
|----|----|------|

## <섹션 2>
...

## 변경 이력
- YYYY-MM-DD: 초안 (ADR-NNNN)
```

## 6. 디스패치 우선순위와 의존 그래프

```
A0.1 (ADR-0007) ──┐
A0.2 (ADR-0008) ──┤
A0.3 (ADR-0009) ──┤
A0.4 (ADR-0010) ──┼──→ A0.7 (정합 리뷰) ──┐
A0.5 (ADR-0011) ──┤  ✅                  │
A0.6 (ADR-0012) ──┘                       │
                                          ▼
                                  A1 (ADR-0001) ───┐
                                                    ├──→ A4 (정합 리뷰) ──┐
                                  A2 (ADR-0002) ───┤  ✅                 │
                                  A3 (ADR-0003) ───┘                     │
                                                                          ▼
                                                                  C1 (Rust skeleton) ──┐
                                                                  C2 (Svelte skeleton) ─┤
                                                                  C3 (codegen pipe)  ───┼──→ C4 smoke ──→ C5 (배치 C 정합) ──→ sketch §15 1단계
                                                                  

B1 (R2)  ──→ ADR-0004 ────┐
B2 (R3)  ──→ ADR-0005 ────┼─→ 프론트엔드 후속 기능 ADR (배치 D — 추후)
B3 (R6)  ──→ ADR-0006 ────┤
B4 (R7)  ──→ ADR-0011 Accepted  ✅
B5 (R8)  ──→ ADR-0012 Accepted  ✅ ─┘
```

병렬화:
- 배치 A0 내부: A0.1∥A0.2∥A0.3∥A0.4∥A0.5∥A0.6 (6개 모두 독립, 동시 진행) → A0.7 정합 리뷰. ✅ **완료**
- 배치 A0 → 배치 A·B. ✅ **완료**
- 배치 A와 배치 B는 서로 독립, 동시 진행. 배치 A 내부는 A1 → (A2∥A3) → A4. ✅ **완료**
- 배치 B 내부: B1∥B2 (독립). B3는 ADR-0002 입력 가능 시. B4∥B5는 ADR-0011/0012 발행 후. ✅ **완료**
- **배치 C 내부**: C1∥C2∥C3 (3개 독립, 동시 진행) → C4 통합 smoke → C5 정합 리뷰. **본 시점에서 dispatch 가능**.

## 7. PM 호출 인터페이스 (참고)

PM이 각 task를 실행시킬 때 사용하는 호출 패턴(예시, 실행 아님):

- A0.1 → `Agent(subagent_type="system-architect", prompt=Task A0.1 프롬프트)`
- A0.2 → `Agent(subagent_type="system-architect", prompt=Task A0.2 프롬프트)`
- A0.3 → `Agent(subagent_type="system-architect", prompt=Task A0.3 프롬프트)`
- A0.4 → `Agent(subagent_type="frontend-architect", prompt=Task A0.4 프롬프트)`
- A0.5 → `Agent(subagent_type="self-review", prompt=Task A0.5 프롬프트)`
- A1 → `Agent(subagent_type="system-architect", prompt=Task A1 프롬프트)`
- A2 → `Agent(subagent_type="backend-architect", prompt=Task A2 프롬프트)`
- A3 → `Agent(subagent_type="security-engineer", prompt=Task A3 프롬프트)`
- A4 → `Agent(subagent_type="self-review", prompt=Task A4 프롬프트)`
- B1·B2·B3 → `Agent(subagent_type="deep-research", prompt=핸드오프 §4 + PM 추가 제약)`

A0.1·A0.2·A0.3·A0.4 4개는 단일 메시지에 다중 Agent 호출로 병렬 실행. A0.5 정합 리뷰는 4개 결과 모인 후. 그 다음 배치 A·B 진행. A2·A3, B1·B2·B3은 단일 메시지에 다중 Agent 호출로 병렬 실행 가능.
