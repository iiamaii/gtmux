# 세션 핸드오프 — 2026-05-14 (Sprint 0~3 완료 + Sprint 4 진입 직전)

본 문서는 `0014-session-handoff.md`의 후속이다. P0 구현이 backend (Sprint 0~2) + frontend (Sprint 3) 모두 완료된 시점에서 다음 세션이 어디서 이어받을지를 한 화면으로 캡처한다. **CLAUDE.md + CONTEXT.md + 본 문서 3개**만 읽어도 작업이 이어진다.

## TL;DR

- **현 단계**: sketch §15 1단계(엔진 연결 검증) 핵심 P0 구현 끝. backend 7 crates 모두 골격 완성, frontend 캔버스+xterm+WS 파이프라인 통과.
- **다음 = Sprint 4 — byte-level alignment + frontend WS wiring + ADR-0004/0005/0006 발행 + smoke 재실행**.
- **현재 실패하는 invariant 없음**. 단, backend ws-server에서 mux-router Event → WS envelope 변환 broadcaster가 미작성이라 E2E 데이터 흐름은 아직 비활성.
- **GitHub push**는 여전히 보류 (`iiamaii/gtmux` keychain 문제, 사용자 수동 해결 영역).

## 1. 우선 읽을 문서 (순서대로)

1. `CLAUDE.md` — 프로젝트 메타 (English)
2. `CONTEXT.md` — 도메인 어휘 + 5대 불변식
3. **본 문서 (`0015-session-handoff.md`)** — Sprint 0~3 결과 + Sprint 4 진입점
4. `docs/reports/0014-session-handoff.md` — Sprint 0 직전 상태 (D1~D23 결정 요약 + 환경 도구 메모)
5. `docs/sketch.md` — spec 정본
6. `docs/reports/0010-grill-amendments.md` — D1~D23 상세 결정 (0014에 요약본 있음)
7. `docs/reports/0012-bootstrap-smoke.md` — P0 task list (정본)
8. `docs/adr/0001-0003, 0007-0012` — 9 ADRs Accepted
9. `docs/ssot/wire-protocol.md`, `security-defaults.md`, `canvas-layout-schema.md` — 3 SSoTs

## 2. 진행 매트릭스

| Phase | Status | 산출물 |
|---|---|---|
| Grill D1~D23 + ADR ×9 + SSoT ×3 + Code skeleton | ✅ | `894ab69` 이전 commits |
| **Sprint 0** (AUTH + CFG + MUX) | ✅ | `4a010a0` — 52 unit tests |
| **Sprint 1** (LIFE-1 daemon + CLI-1 start) | ✅ | `951750b` — +13 tests |
| **Sprint 2** (HTTP-1+2 / WS-1 / LIFE-2 + CLI-3/4/5) | ✅ | `da19003` — +49 tests |
| **Sprint 3** (FE-1 dispatcher / FE-2 canvas+xterm / FE-3 banner) | ✅ | `80675d6` — svelte-check 0/221 |
| **Sprint 4** (alignment + wiring + ADR-0004/5/6 + smoke) | ⏳ **다음 단계** | 본 문서 §5 |
| ADR-0004/0005/0006 발행 | ⏳ B6 mapping 후 P1 작업 가능 |
| GitHub push iiamaii/gtmux | ⏳ blocked (credential, 사용자 영역) |

## 3. Commit history (main, 누적 16개)

```
80675d6 Sprint 3: FE-1 (WS dispatcher) + FE-2 (Canvas+xterm) + FE-3 (Reconnect banner)
da19003 Sprint 2: http-api (HTTP-1+2), ws-server (WS-1), teardown (LIFE-2 + CLI-3/4/5)
951750b Sprint 1: P0-LIFE-1 (tmux daemon) + P0-CLI-1 (gtmux start)
4a010a0 Sprint 0: P0-AUTH-1 + P0-CFG-1 + P0-MUX-1 implementations
894ab69 Session handoff doc + code-review-graph build for compact
2e152ec C5 Batch C coherence + fix B1/B2/A4 blockers
4e7a0e5 Batch C4 smoke test + ADR-0003 bootstrap-exchange clarification
3af3abe Batch C1 + C2 + C3: code bootstrap skeletons
ba90988 plan 0002 §3 Batch C definition
e35fad7 ADR 9개 Proposed → Accepted (α) + A2 utoipa unification (δ)
c0007ad A4 coherence: fix B1/B2/B3 blockers + G12/G13/C1
05e29aa Batch A2 + A3 + B4 + B5
4ef7572 Batch A1 + B1 + B2 + B3
270e32c A0.7 2nd-pass coherence: fix G7/G8
2aff743 Grill phase 2
89a13b5 Initial commit
```

## 4. Backend (Rust) — 구현 상태

| Crate | LOC | 테스트 | 주요 API |
|---|---|---|---|
| `auth` | 478 | 10 PASS | `issue_token`/`save_token`/`load_token`/`verify_token`/`rotate_token`, ring 0.17 CSPRNG, atomic write, 0600/0700 perm enforced |
| `config` | 678 | 14 PASS | `Config{server,runtime,security,cloud}`, `Mode::{Local,Cloud}` derived from bind, figment strict, deny_unknown_fields |
| `mux-router` | 885 | 28 PASS | `Command` enum (11 variants, allowlist per ADR-0008), `Event` enum (14), `parse_line`, `decode_output_payload` (256-LUT octal) |
| `lifecycle` | 1502 | 10 + 4 ignored | `TmuxDaemon::{spawn,attach,read_line,write_line,shutdown}` + `teardown` (D6 5단계) + `cleanup_stale_socket` + `socket_path_for` |
| `http-api` | 738 | 18 PASS | `router(&Config, &TokenString)`: /healthz, /auth/bootstrap (R(rej)2 예외), GET/PUT /api/layout (32-hex SHA256-128 ETag, If-Match, 256 KiB cap), Origin+Host+Bearer 미들웨어 |
| `ws-server` | ~700 | 19 PASS | `router`: /ws upgrade with subprotocol "gtmux.v1, bearer.<token>" (RFC 6455 colon-list), envelope codec ([1B type][4B LE u32 len][payload]), 4 MiB cap, 30s ping/60s pong heartbeat, close codes |
| `bin/gtmux-cli` | 1305 | 12 PASS | clap derive: start (D20 1~13단계 wiring) / teardown / rotate-token / status / stop (informational stub, P0-LIFE-3 pending) |
| `bin/gen-openapi` | small | — | codegen stub |

**총 backend**: 114 unit tests + 4 ignored (tmux 바이너리 필요) PASS. clippy `-D warnings` clean.

## 5. Frontend (Svelte 5 + Vite 7) — 구현 상태

| 모듈 | 핵심 파일 | LOC |
|---|---|---|
| WS pipeline | `lib/ws/{decode.ts, client.ts, dispatcher.svelte.ts}` + `lib/types/envelope.ts` | 443+292+260+27 |
| Canvas | `lib/canvas/{Canvas, PanelNode, XtermHost, PanelPlaceholder}.svelte` | 133+214+112+66 |
| Banner | `lib/banner/ReconnectBanner.svelte` | 168 |
| Stores | `lib/stores/{connection,ephemeral,groups,layout,panels}.svelte.ts` | 5 stores |
| Page | `routes/+page.svelte` | 128 |

**총 frontend**: 221 files, svelte-check 0 errors / 0 warnings. `npm run build` → 6 chunks, 167 KB gzip (main 4 + svelteflow 67 + xterm 91), ADR-0012 O7 cap (200 KB main) 통과.

## 6. Code graph (code-review-graph MCP)

2026-05-14T01:0X 빌드. **379 nodes / 3445 edges / 105 flows / 10 communities**. Languages: bash + rust + svelte + typescript + javascript + css.

탐색 시 `mcp__code-review-graph__*` 우선 사용. 특히:
- `semantic_search_nodes_tool` — 함수·클래스 검색
- `query_graph_tool` (callers_of/callees_of/imports_of/tests_for)
- `get_impact_radius_tool` — 변경 blast radius
- `detect_changes_tool` + `get_review_context_tool` — 리뷰 시

## 7. Sprint 4 — 다음 단계 task 분해

Sprint 0~3에서 의도적으로 잘라둔 carry-forward를 정리한다. **ADR 발행은 코드보다 우선** (CLAUDE.md ADR-before-code 룰).

### S4-A. ADR 발행 (B6 mapping) — 코드 의존 없음, 즉시 가능

| Task | 입력 | 산출 |
|---|---|---|
| **S4-ADR-0004** 터미널 렌더링 | `docs/reports/0002-terminal-rendering.md` (R2) | `docs/adr/0004-terminal-rendering.md` — xterm.js + addon-fit + addon-unicode11 잠금 |
| **S4-ADR-0005** 캔버스 라이브러리 | `docs/reports/0003-canvas-library.md` (R3) | `docs/adr/0005-canvas-library.md` — @xyflow/svelte v1.5 잠금 |
| **S4-ADR-0006** 영속화 storage | `docs/reports/0006-persistence.md` (R6) | `docs/adr/0006-persistence-storage.md` — 영속화 결정 (3단계 prereq) |

각 ADR 형식은 `docs/plans/0002-work-dispatch.md` §4 템플릿. 발행 후 sketch §15 prereq 표 갱신.

### S4-B. Byte-level alignment (backend ↔ frontend)

**문제**: FE-1이 SSoT §2.1을 따라 `PANE_OUTPUT` payload를 `varint paneId + raw bytes`로 디코드 가정. R8 §F4는 `varint pane_id + varint length + bytes` 변형을 시사. backend ws-server는 hello envelope만 보내고 mux-router Event → envelope broadcaster가 미작성이라 정합 검증이 못 됐다.

| Task | 작업 |
|---|---|
| **S4-WIRE-1** envelope payload SSoT 확정 | wire-protocol.md §2 표를 *바이트별 정본*으로 잠금. 12개 frame 모두 ABI fix. FE-1 dispatcher 코멘트에 적힌 가정과 차이 있으면 dispatcher 또는 SSoT 한쪽 조정 후 commit |
| **S4-WIRE-2** mux → WS broadcaster | `ws-server` crate에 `Hub` 구조 추가: mux-router의 `Event` 채널 (`tokio::sync::broadcast`) + WS handler가 subscribe + envelope 인코딩 후 송신. CLI-1 start 시점에 `TmuxDaemon::read_line` 루프 → `parse_line` → `Hub.publish`. **per-pane ring buffer 128 KB (D15)** 도입 시점 |
| **S4-WIRE-3** 양방향 입력 | client → server frames (0x03 PANE_INPUT / 0x04 PANE_RESIZE / 0x05 PANE_PAUSE / 0x06 PANE_RESUME) backend handler. mux-router `Command` enum (`send-keys`, `resize-window`)으로 변환 후 tmux daemon `write_line` |
| **S4-WIRE-4** 통합 테스트 | backend ws-server + 가짜 mux 채널로 round-trip test (envelope 인코딩 → 디코딩 byte-equal). FE-1 decode.ts test fixture로도 동일 byte stream 검증 |

### S4-C. Frontend WS wiring

| Task | 작업 |
|---|---|
| **S4-FE-1** XtermHost.onData → WsClient.send | term.onData 콜백에서 PANE_INPUT envelope encode + send. paneId는 `data` prop의 `pane_id` |
| **S4-FE-2** FitAddon resize → PANE_RESIZE | onResize 이벤트에서 cols/rows + paneId → PANE_RESIZE envelope. debounce 100ms |
| **S4-FE-3** NOTIFY_MIRROR → markZombie | dispatcher가 `0x07 NOTIFY_MIRROR`의 `pane-died` kind 수신 시 `connectionStore.markZombie([paneIds])` 호출 |
| **S4-FE-4** HTTP layoutRefetchHandler 등록 | dispatcher가 LAYOUT_CHANGED 수신 시 `fetchLayout()` 재실행 + `panelsStore.hydrate(snapshot)` |
| **S4-FE-5** WsClient state propagation | client.ts에서 close 시 `setCloseInfo(code, reason)` 호출 wiring |

### S4-D. P0 잔여 + smoke

| Task | 작업 |
|---|---|
| **S4-LIFE-3** pidfile + gtmux stop | `lifecycle::spawn_daemon`에서 `$XDG_STATE_HOME/gtmux/<session>.pid` 작성 (atomic). `Cmd::Stop`이 pid 읽어 SIGTERM 전송 + 5s wait → graceful. teardown은 pid 파일도 cleanup 대상 |
| **S4-SMOKE** 9-step smoke 재실행 | `codebase/smoke/01_engine_connect.sh` 실행. Sprint 4-C 직후 시점에서 6 gates → 0 gates 목표 |
| **S4-FE-TOKEN** server-side token injection | bootstrap response가 SPA HTML inline script로 `sessionStorage.gtmux_token`을 set. 현재 prompt() dev fallback 제거 |

## 8. Sprint 4 dispatch prompts (즉시 사용 가능)

다음 세션이 "Sprint 4 시작"이라고 하면 아래를 그대로 Agent 호출에 복붙.

### Agent #1 (technical-writer 또는 backend-architect) — S4-ADR-0004/0005/0006

```
ADR 3개 발행. 모두 docs/plans/0002-work-dispatch.md §4 템플릿 (맥락/결정/거절/결과/불변식 검증/미해결).

ADR-0004 터미널 렌더링:
  입력: docs/reports/0002-terminal-rendering.md (R2)
  결정 잠금: xterm.js (6.x) + @xterm/addon-fit + @xterm/addon-unicode11
  거절: HyperTerm, react-terminal, custom canvas renderer
  불변식: 5대 invariant 중 #4(security 입력 escaping), CLAUDE.md ADR-before-code

ADR-0005 캔버스 라이브러리:
  입력: docs/reports/0003-canvas-library.md (R3)
  결정 잠금: @xyflow/svelte v1.5 (Svelte Flow)
  거절: Konva, fabric.js, vanilla DOM transforms, custom WebGL
  불변식: tmux Layout ≠ Canvas Layout (invariant #3)

ADR-0006 영속화 storage (sketch §15 3단계 prereq):
  입력: docs/reports/0006-persistence.md (R6)
  결정 잠금: 채택 storage (sqlite vs JSON file vs key-value)
  거절: 분산 DB, cloud-only
  불변식: state separation (invariant #1), single-user scope

각 ADR 발행 후 sketch §15 prereq 표 + docs/reports/0010-grill-amendments.md ADR 발행 큐 업데이트.
DoD: 3 ADR 파일 Accepted 2026-05-14, sketch §15 amended.
```

### Agent #2 (backend-architect) — S4-WIRE-1 + WIRE-2

```
ws-server crate에 mux → WS broadcaster Hub 도입.

계약:
- docs/ssot/wire-protocol.md §2 (12 frame ids 바이트별 정본 — 본 task에서 SSoT를 정본화)
- docs/reports/0010-grill-amendments.md D14 (web-domain frames), D15 (per-pane ring buffer 128 KB), D16 (pause/continue)
- docs/adr/0001 D7 (tmux notification list)
- codebase/backend/crates/mux-router/src/lib.rs::Event (14 variants)
- codebase/backend/crates/ws-server/src/lib.rs::Envelope (현재 hello envelope만)
- codebase/frontend/src/lib/ws/decode.ts의 per-frame helpers (정합 검증 target)

작업:
1. Hub<Event> 구조 추가 — tokio::sync::broadcast capacity 256
2. lifecycle::TmuxDaemon::read_line 루프 → mux_router::parse_line → Hub.publish
3. WS handler: subscribe → loop { recv() → Envelope encode → ws.send(Binary) }
4. per-pane ring buffer (D15): pane_id 별 VecDeque<u8> capacity 128*1024 (메모리 only, no disk). 새 WS attach 시 catch-up replay
5. SSoT §2 표를 *바이트 정본*으로 확정 (FE-1 dispatcher 가정과의 차이 명시 + 한쪽 조정)

테스트:
- envelope encode/decode byte-equal (12 frames × 다양한 payload)
- Hub broadcast 다중 subscriber 정합
- ring buffer 128 KB 정확 관리 (oldest 폐기)
- frontend decode.ts fixture와 동일 byte stream로 round-trip
DoD: cargo test -p gtmux-ws-server PASS. SSoT byte-spec commit.
```

### Agent #3 (frontend-architect) — S4-FE-1~5

```
frontend wiring 완성. 5개 sub-task:

1. XtermHost.svelte의 term.onData console.debug stub → WsClient.send(encodePaneInput(paneId, bytes))
2. FitAddon resize 콜백 → debounce 100ms → WsClient.send(encodePaneResize(paneId, cols, rows))
3. dispatcher.svelte.ts의 NOTIFY_MIRROR 0x07 핸들러 — JSON kind "pane-died" → connectionStore.markZombie([paneIds]) (append)
4. dispatcher.svelte.ts의 LAYOUT_CHANGED 0x80 핸들러 추가: setEtag 외 + setLayoutRefetchHandler 콜 (HTTPClient의 fetchLayout 재실행 후 panelsStore.hydrate)
5. WsClient의 onclose → connectionStore.setCloseInfo(code, reason) + setState 이미 호출 중인지 확인 후 보강

계약: SSoT wire-protocol.md §2 (S4-WIRE-1에서 정본화 완료된 버전 참조)

DoD: npm run check + npm run build PASS. 4 MiB cap, 200 KB main-bundle cap 유지.
```

### Agent #4 (backend-architect) — S4-LIFE-3

```
gtmux stop 실 wiring.

계약: 0014 핸드오프 §carry-forward "Stop=stub 유지 결정" + sketch §6 (graceful shutdown)

작업:
1. lifecycle::spawn_daemon 또는 CLI Cmd::Start가 $XDG_STATE_HOME/gtmux/<session>.pid에 std::process::id() atomic 작성 (write_atomic 패턴 재사용)
2. CLI Cmd::Stop 핸들러:
   - pid 파일 read → SIGTERM 송신 → tokio::time::timeout(5s, wait_pid) → 실패 시 SIGKILL fallback (또는 사용자 안내)
3. teardown 5단계의 step 3 state files 목록에 .pid 추가
4. tests: spawn_writes_pidfile, stop_kills_pid, stop_missing_pidfile_friendly_error

DoD: cargo test -p gtmux-lifecycle + -p gtmux-cli PASS.
```

## 9. 안티패턴 / 함정 (Sprint 0~3 누적)

핸드오프 0014의 목록은 모두 유효. 본 세션에서 추가:

- **Backend ws-server는 envelope codec만 있고 broadcaster는 없음** — `/ws` 연결 시 0x80 LAYOUT_CHANGED hello만 1회 보내고 침묵. mux-router의 Event를 WS로 흘리는 채널이 미작성. Sprint 4-B의 S4-WIRE-2가 이를 담당.
- **PANE_OUTPUT inner shape 양면 가정** — FE-1 decode.ts는 `varint paneId + raw bytes` (SSoT §2.1)를 채택. R8 §F4의 `varint paneId + varint length + bytes` 변형은 *비채택*. backend가 인코딩하기 전 SSoT를 *바이트 정본*으로 잠가둘 것 (S4-WIRE-1).
- **vite.config.ts에 `$lib` alias 필수** — tsconfig.json `paths`만으로는 Rollup이 인식 못 함. 추가 시 svelte-check와 vite build 둘 다 통과. Sprint 3에서 학습.
- **HttpOnly cookie는 WS subprotocol 토큰으로 사용 불가** — `Sec-WebSocket-Protocol`은 JS API로 토큰을 *전달*해야 하는데 HttpOnly cookie는 JS에서 못 읽음. 현재 sessionStorage 경로 (S4-FE-TOKEN에서 server-side inline script로 정식화 예정). prompt() dev fallback은 명백한 임시 우회.
- **WS close code 1000은 banner 미표시** — 의도적 종료를 noise로 표면화하지 않음. 1001/1006/1008/1011/4001만 사용자에게 노출.
- **shutdown 의미는 control-mode client 종료, 데몬은 살림** — ADR-0009 D5. lifecycle::TmuxDaemon::shutdown은 daemon kill이 아니다. teardown만이 daemon kill 책임. 이 invariant는 Sprint 1에서 LIFE-1 agent가 자체 보정해서 코드에 정확히 반영됨.
- **gen-openapi의 rustfmt 자동 정렬** — Sprint 2 작업 중 cargo fmt가 `[openapi(info(...))]` 콤마 위치를 자동 정렬해 diff에 포함됨. cosmetic, 의도 외 변경 아님 — 다음 세션에서 발견 시 정상.
- **envelope.ts `verbatimModuleSyntax` re-export 회귀** — FE-3 agent가 보고에서 언급. 그러나 FE-1이 동시 진행에서 envelope.ts를 전면 재작성하여 type-only re-export로 해소됨. 현재 svelte-check 0 errors. 다음 세션에서 본 문제가 재발하면 `export type` 명시 확인.

## 10. 잔여 carry-forward (Sprint 4 이후)

- **ADR-0004/0005/0006** — Sprint 4-A에서 발행 예정
- **C5 Advisory** (A1·A2·A3·A5, Cosmetic C1·C2 in `docs/reports/0013`) — Sprint 4 병렬 처리 가능
- **GitHub push** — credential 사용자 영역
- **시각 검증 (Playwright/Cypress)** — Sprint 5에서 도입
- **CI 캐시 도입** (sccache, cargo-cache, node_modules cache) — 본 cap이 안 잡힐 때
- **TLS 인증서 생성/관리 helper** — Cloud 모드 활성화 시점 (sketch §15 4단계)
- **단위 테스트 커버리지** — code-graph가 "test gap 205"로 표시하나 실제 unit test는 존재. graph가 test annotation을 못 잡는 것이 원인 — graph parser tuning 필요 (낮은 우선순위)

## 11. 환경·도구 (변경 없음, 0014 §환경·도구 메모 참조)

핵심만 재요약:
- **Memory files** (`/Users/ws/.claude/projects/-Users-ws-Desktop-projects-gtmux/memory/`):
  - `MEMORY.md` (index)
  - `project_gtmux.md`, `feedback_language_and_adr.md`, `feedback_grill_style.md`
- **MCP**: `code-review-graph` 활성. Grep 대신 우선 사용.
- **Subagents 가용**: backend-architect, frontend-architect, system-architect, devops-architect, security-engineer, quality-engineer, technical-writer, deep-research 등.
- **사용자 피드백 룰**:
  1. 기술 디테일 결정 → brief + 진행 (confirm 묻지 않음)
  2. 도메인/UX/정책 → 옵션 비교 + 확인
  3. KO docs / EN code
  4. ADR-before-code

## 12. 다음 세션 첫 메시지 가이드

사용자가 "Sprint 4 진행" 또는 "다음 단계 진행"이라 하면 본 문서 §7~§8을 그대로 따른다. 순서:

1. **S4-A (ADR ×3)** → 코드 의존 없이 발행 후 commit
2. **S4-B (WIRE-1+2)** → SSoT byte-spec 확정 후 backend broadcaster
3. **S4-C (FE wiring ×5)** → frontend bidirectional flow
4. **S4-D (smoke 재실행 + LIFE-3)** → P0 종료 + 1단계 통과 선언

S4-A는 단독 진행 (ADR-before-code 룰). S4-B 끝난 후 S4-C/D 병렬 가능.

## 변경 이력

- 2026-05-14: 초안 (Sprint 0~3 완료 직후)
