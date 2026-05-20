# 세션 핸드오프 — 2026-05-14 (compact 직전 캡처)

본 문서는 session 후속(또는 새 세션)이 *어디서 멈췄는지·다음에 무엇을 할지·무엇을 피할지*를 한 화면에 파악할 수 있도록 한 핸드오프 노트다. CLAUDE.md + CONTEXT.md + 본 문서 3개만 읽어도 작업을 이어받을 수 있어야 한다.

## TL;DR

- **프로젝트**: `gtmux` — tmux 백엔드 + 무한 캔버스 웹 UI, 단일 사용자 로컬/개인 서버.
- **현 단계**: pre-implementation 끝. 디자인·ADR·SSoT·코드 부트스트랩·smoke·코히런스 모두 완료. **다음 = sketch §15 1단계(엔진 연결 검증) P0 구현**.
- **즉시 다음 액션**: Sprint 0 dispatch (P0-AUTH-1 + P0-CFG-1 + P0-MUX-1, 3 parallel Agents). 상세 프롬프트는 본 문서 §dispatch-prompts.
- **잔여 제약**: GitHub push는 credential 문제로 보류 (iiamaii/gtmux, macOS keychain이 `SewingRobot` 캐시 → 사용자가 직접 해결 필요).

## 우선 읽을 문서 (순서대로)

1. `CLAUDE.md` — 프로젝트 메타 (English, 영구 룰)
2. `CONTEXT.md` — 도메인 어휘 + 5대 불변식 (KO/EN 혼용, 영구 룰)
3. `docs/sketch.md` — 1차 spec (KO, source of truth)
4. **`docs/reports/0010-grill-amendments.md`** — D1~D23 23개 결정 + sketch 수정 명세 + ADR 발행 큐 (KO, 진행 기록의 단일 진실)
5. **`docs/reports/0012-bootstrap-smoke.md`** — P0 task list (구체 file·function·계약·수용 기준) + Sprint 시퀀스 (KO)
6. **`docs/reports/0013-bootstrap-coherence-review.md`** — C5 게이트 리뷰 결과 (블로커 모두 해소됨)

코드 시작 시 추가로:
7. `docs/adr/0001-0012` (9개 Accepted ADR)
8. `docs/ssot/wire-protocol.md`, `security-defaults.md`, `canvas-layout-schema.md` (3개 SSoT)
9. `docs/reports/0001/0002/0003/0004/0005/0006/0007/0008` (R1~R8, 1차 자료 + B 배치 연구)
10. `docs/plans/0002-work-dispatch.md` — 디스패치 정본

## 진행 매트릭스

| Phase | Status | Artifact |
|---|---|---|
| Grill (D1~D23) | ✅ | `docs/reports/0010-grill-amendments.md` |
| ADR generation (×9) | ✅ Accepted 2026-05-14 | `docs/adr/0001-0003,0007-0012` |
| Research reports (R1~R8) | ✅ | `docs/reports/0001-0008` |
| SSoTs (×3) | ✅ | `docs/ssot/wire-protocol`, `security-defaults`, `canvas-layout-schema` |
| Coherence reviews (1차/2차/A4/C5) | ✅ | `docs/reports/0009`, `0011`, `0013` |
| Code skeleton (Batch C1·C2·C3) | ✅ `make build/codegen/test` PASS | `codebase/` (40 파일, Rust + Svelte + Makefile) |
| Smoke test scaffolding (C4) | ✅ 2 PASS / 6 GATE / 1 manual | `codebase/smoke/01_engine_connect.sh`, `docs/reports/0012` |
| **P0 implementation** | ⏳ **다음 단계** | Sprint 0/1/2/3 (아래 참조) |
| Push to GitHub `iiamaii/gtmux` | ⏳ blocked (credential) | macOS keychain 정리 필요 |

## Commit history (main, 11개)

```
2e152ec C5 Batch C coherence + fix B1/B2/A4 blockers
4e7a0e5 Batch C4 smoke test + ADR-0003 bootstrap-exchange clarification
3af3abe Batch C1 + C2 + C3: code bootstrap skeletons
ba90988 plan 0002 §3 Batch C definition (code bootstrap)
e35fad7 ADR 9개 Proposed → Accepted (α) + A2 utoipa unification (δ)
c0007ad A4 coherence: fix B1/B2/B3 blockers + G12/G13/C1
05e29aa Batch A2 + A3 + B4 + B5: 2 ADRs + 2 SSoTs + R7 + R8
4ef7572 Batch A1 + B1 + B2 + B3: ADR-0001 + R2/R3/R6 reports
270e32c A0.7 2nd-pass coherence: fix G7/G8
2aff743 Grill phase 2: coherence review + tech stack + perf/CLI/config/UX
89a13b5 Initial commit
```

## 핵심 결정 23개 (D1~D23) 요약

| D | 결정 한 줄 | 출처 |
|---|---|---|
| D1 | 1 Canvas : 1 tmux Session (스코프) | grill Q1 |
| D2 | 1 gtmux Server : 1 Session : 1 Port (immutable 바인딩, 부재 시 에러) | grill Q2 |
| D3 | UI 범위 = Pane + Panel + Group 제어만 (Session·Window 외부) | grill Q2 |
| D4 | tmux Window UI 비노출, Group(Figma-식)이 대체 | grill Q3 |
| D5 | tmux active window = M의 default seed mirror only | grill Q3 |
| D6 | Panel active = M (manipulation) + I (input), 직교 | grill Q3 |
| D7 | 신규 Panel 좌표 optional + 미지정 시 cascade (origin+(40,40)) — D23에서 재정의 | grill Q3·Q5 |
| D8 | Single-pane-per-window 컨벤션, allowlist에서 split/resize-pane/select-layout 제외 | grill Q5 |
| D9 | Panel label → `rename-window` 동기화 (외부 attach UX) | grill Q5 |
| D10 | Dedicated tmux daemon per Server (`tmux -L gtmux-<session>`) | grill Q6 |
| D11 | Group 데이터 = G-hybrid (frame 저장 X, 드래그 delta 액션) | grill Q7 |
| D12 | Canvas Layout = HTTP PUT/ETag, WS notify만 (T-mixed) | grill Q8 |
| D13 | MT-3 Live Mirror: 모든 연결 = 단일 사용자 거울 | grill Q9 |
| D14 | WS 0x80–0x84 정의 (LAYOUT_CHANGED/M/I/VIEWPORT/FOCUS_MODE) | grill Q9 |
| D15 | per-pane ring buffer 128KB 기본 (memory only, no disk) | grill Q10 |
| D16 | Panel Streaming State (visibility/minimize → pause/continue) | grill Q10 |
| D17 | 인증 토큰 = 256-bit CSPRNG, local 매시작 재발급 / cloud 영속+회전 | grill Q11 |
| D18 | Stack = Rust(axum+tokio) + Svelte 5 (성능 우선) | grill Q12 |
| D19 | 성능 예산 (50 panel / cold<500ms / p99<100ms 등) | grill Q13 |
| D20 | CLI = `start/stop/teardown/rotate-token/status` + foreground + XDG dirs | grill Q14 |
| D21 | First-run banner + WS reconnect grace 1s + port-based reattach + zombie badge | grill Q15 |
| D22 | Config = TOML per-session, mode 자동 추론 (bind 기반) | grill Q16 |
| D23 | Placement = optional + cascade / z-index = M에 들어오면 최상위 자동 + overlap 허용 | grill Q17 |

## 5대 불변식 (CLAUDE.md §4 + sketch §4.1)

1. **tmux state ↔ web state 분리** — Pane/Window/Session tmux 측, Panel/Group/Canvas Layout web 측.
2. **tmux-native vs web-only 분기** — pane CRUD = tmux 명령, Panel hide/lock/group = web only.
3. **tmux Layout ≠ Canvas Layout** — single-pane-per-window 컨벤션으로 *기계적* 보장.
4. **보안 기본값** — 127.0.0.1 default, 토큰 + Origin + Host 화이트리스트, allowlist 명령.
5. **control mode** — `tmux -C`, 스크린-스크레이핑·`-CC` 금지.

## 코드 그래프 (code-review-graph MCP 활성)

2026-05-14T00:46 빌드. 40 files, 75 nodes (Class 16 / File 40 / Function 19), 210 edges (CALLS 161, CONTAINS 35, IMPORTS_FROM 13, REFERENCES 1), 8 communities. Languages: bash, rust, svelte, typescript, javascript.

**코드 탐색 시 우선 사용** (Grep/Read 대신):
- `mcp__code-review-graph__semantic_search_nodes_tool` — 함수·클래스 검색
- `mcp__code-review-graph__query_graph_tool` (callers_of/callees_of/imports_of/tests_for)
- `mcp__code-review-graph__get_impact_radius_tool` — 변경 blast radius
- `mcp__code-review-graph__detect_changes_tool` + `get_review_context_tool` — 리뷰 시

## 다음 단계 — P0 구현 Sprint 시퀀스

C4 smoke report (`docs/reports/0012`)의 §3 P0 task list가 정본. Sprint 분할:

### Sprint 0 (병렬, 즉시 dispatch 가능)
- **P0-AUTH-1** — `crates/auth/src/lib.rs`: `verify_token`/`issue_token`/`load_token`/`rotate_token`. ring 0.17 + 0600 perm + atomic write. 계약: ADR-0003 D4·D5, ADR-0011 D8, `docs/ssot/security-defaults.md` §1.3·§3.
- **P0-CFG-1** — `crates/config/src/lib.rs`: `Config` struct (D22 schema) + figment loader + mode 자동 추론 (bind → local/cloud). 계약: D22, `docs/ssot/security-defaults.md`.
- **P0-MUX-1** — `crates/mux-router/src/lib.rs`: `Command` enum은 이미 존재 (11 variants per ADR-0008), 추가로 `OutputDecoder` (`%output` 8진수 디코더) + 라인 dispatch parser (winnow 0.6). 실제 tmux IPC는 Sprint 1 LIFE-1. 계약: R1 §3·§4, ADR-0001 D7, D15.

**의존 그래프**: 3개 모두 leaf, 서로 독립. 한 메시지 안에 3 Agent 호출.

### Sprint 1 (critical path)
- **P0-LIFE-1** — `crates/lifecycle/src/lib.rs::spawn_daemon` (+ socket cleanup helper). 계약: ADR-0009 D2·D3·D4 + R7 §lifecycle module.
- **P0-CLI-1** — `bin/gtmux-cli/src/main.rs::Cmd::Start`. lifecycle::spawn_daemon → auth::issue_token → http-api 라우터 mount → ws-server 라우터 mount → axum bind. 계약: ADR-0007 D2, D21 c1 (콘솔 banner).

### Sprint 2 (병렬)
- **P0-HTTP-1** `crates/http-api/src/lib.rs::router` — Origin/Host/Bearer 미들웨어 체인 + `GET/PUT /api/layout` + ETag 32-hex (`canvas-layout-schema.md` §2).
- **P0-HTTP-2** (C5 B1 추가) `http-api/src/lib.rs::bootstrap_handler` — `GET /auth/bootstrap?token=` 1회 cookie 교환 + 302 redirect (ADR-0003 R(rej)2 예외).
- **P0-WS-1** `crates/ws-server/src/lib.rs::router` — `/ws` upgrade + `Sec-WebSocket-Protocol` 콤마 두 값 검증 + envelope codec skeleton (`docs/ssot/wire-protocol.md` §2.1).
- **P0-LIFE-2** + **P0-CLI-3** — `teardown` 5단계 (D6) + `Cmd::Teardown` 핸들러.

### Sprint 3 (frontend)
- **FE-1** WS dispatcher (`src/lib/ws/dispatcher.svelte.ts`) — envelope decode + store fan-out.
- **FE-2** Canvas + 1 Panel (xterm.js mount with R8 F6 옵션).
- **FE-3** Reconnect banner + grace 1s (D21 c2/c3).

### Sprint 0 — Dispatch prompts (즉시 사용 가능)

**Agent #1 (backend-architect / security-engineer) — P0-AUTH-1**
```
구현: codebase/backend/crates/auth/src/lib.rs

계약:
- ADR-0003 D4·D5 (`docs/adr/0003-security-defaults.md`)
- ADR-0011 D8 (ring 0.17)
- docs/ssot/security-defaults.md §1.3·§3
- docs/reports/0010-grill-amendments.md D17 (회전 정책)

API (pub fn):
- issue_token() -> Result<TokenString> — ring::rand::SystemRandom 256-bit base64url
- load_token(session_name) -> Result<TokenString> — XDG_STATE_HOME/gtmux/<session>.token 0600 read + perm 검증
- save_token(session_name, token) -> Result<()> — atomic write (tempfile + rename + fsync file + dir) + 0600
- verify_token(presented, stored) -> bool — ring::constant_time::verify_slices_are_equal
- rotate_token(session_name) -> Result<TokenString> — issue + save + 옛 토큰 무효 (덮어쓰기)

Tests (in #[cfg(test)] mod):
- roundtrip: issue → save → load → verify == true
- verify constant-time property는 측정 불가지만 assertion: ring API 사용 자체로 충족 명시
- 0600 perm 강제 (실제 파일 stat 검증)
- 256-bit 길이 (base64url 디코딩 후 32 bytes)

DoD: cargo test -p auth 통과. clippy clean.
```

**Agent #2 (backend-architect) — P0-CFG-1**
```
구현: codebase/backend/crates/config/src/lib.rs

계약:
- docs/reports/0010-grill-amendments.md D22 (config schema)
- docs/ssot/security-defaults.md §1·§3
- docs/reports/0007-backend-runtime.md §config-loader 결정 (figment 0.10)

API:
- pub struct Config { pub server: ServerConfig, pub runtime: RuntimeConfig, pub security: SecurityConfig, pub cloud: Option<CloudConfig> }
- pub enum Mode { Local, Cloud } — bind 자동 추론 (loopback/unix → Local, else → Cloud)
- pub fn load(path: Option<&Path>, session: &str) -> Result<Config> — CLI flag > GTMUX_* env > TOML file > defaults
- pub fn derive_mode(bind: &str) -> Mode

필드 (D22):
[server] session, port, bind
[runtime] ring_buffer_size_kb=128, layout_debounce_ms=300, panel_state_debounce_ms=300, log_level="info", log_format="auto"
[security] cors_origins, host_allowlist
[cloud] (Option, mode=Cloud일 때만) tls_cert, tls_key, rate_limit_auth_failures_per_minute

Tests:
- load with defaults (모든 필드 채워짐)
- env override (GTMUX_RUNTIME__LOG_LEVEL=debug)
- mode 추론 (127.0.0.1 → Local, 0.0.0.0 → Cloud, unix:/x → Local)
- unknown field 거부 (오타 방지, figment의 strict 모드)

DoD: cargo test -p config 통과.
```

**Agent #3 (backend-architect) — P0-MUX-1**
```
구현: codebase/backend/crates/mux-router/src/lib.rs (Command enum 이미 존재, 부족분 추가)

계약:
- docs/reports/0001-tmux-control-mode.md §3·§4
- docs/adr/0001-tmux-integration-control-mode.md D7
- docs/reports/0010-grill-amendments.md D15·D16
- docs/reports/0007-backend-runtime.md §parser 결정 (winnow 0.6 + 바이트 LUT)

추가 API (pub):
- pub enum Event { Output{pane_id, bytes}, ExtendedOutput{pane_id, age_ms, bytes}, Pause{pane_id}, Continue{pane_id}, SessionChanged{...}, WindowAdd{wid}, WindowClose{wid}, PaneDead{pid}, LayoutChange{wid, ...}, Exit{reason} }
- pub fn parse_line(line: &[u8]) -> Result<Option<Event>> — winnow 파서
- pub fn decode_output_payload(escaped: &[u8]) -> Vec<u8> — 8진수 `\NNN` 디코더 (R1 §4)

Tests:
- 샘플 `%output %1 hello\\nworld` → Event::Output { pane_id: 1, bytes: "hello\nworld" }
- 8진수 디코딩: `\033` → 0x1B, `\134` → 0x5C
- UTF-8 멀티바이트 pass-through (≥0x80)
- 알 수 없는 라인 → Ok(None) (graceful)

NB: 실제 tmux IPC stream attach는 Sprint 1 P0-LIFE-1. 본 task는 parser 라이브러리.

DoD: cargo test -p mux-router 통과.
```

## 안티패턴 / 함정

- **Push 무리하지 말 것** — macOS keychain의 github.com 자격이 `SewingRobot`로 캐시됨. `iiamaii/gtmux`로 push 시 403. 사용자가 keychain 정리 또는 SSH URL 전환 필요. *자동 해결 시도 금지*.
- **Token URL은 *bootstrap* 전용** — `?token=` 쿼리스트링은 ADR-0003 R(rej)2의 예외 절(=`/auth/bootstrap` 일회용 cookie 교환)에서만 합법. ongoing auth는 `Authorization: Bearer` 또는 WS subprotocol.
- **VIEWPORT_CHANGED endian = LE** (Rust native + JS `DataView.getInt32(offset, true)`). R8 sketch 초안의 BE는 정정됨.
- **Lock 전파 = OR (cascade-down), visibility = AND** — 이전 초안의 통합 AND는 정정됨. CONTEXT.md "Group 운영 규칙" + SSoT canvas-layout-schema.md §1.1 + ADR-0010 D6 모두 정합.
- **Codegen toolchain = utoipa 5.x + openapi-typescript 단일** (A4 §A2 통일). schemars + json-schema-to-typescript는 supersede됨 — R8 §F2 본문은 *역사적 분석*으로만 유지.
- **`split-window` · `resize-pane` · `select-layout` 발급 금지** — ADR-0008 allowlist 컴파일 타임 강제. `mux-router::Command` enum에 variant 자체가 없음.
- **단일 사용자 = WS Client identity 구분 없음** (D13 MT-3). client_id 도입 시도 금지.
- **Placement = optional + cascade** (D23). 옛 D7의 "사용자 명시 입력만" + Unplaced Panel 트레이는 폐기.

## 잔여 carry-forward (Batch C 후 처리)

- **ADR-0004 (터미널 렌더링)** — R2(`0002`) 기반, plan §2 B6에서 발행 예정. 3단계 전 P1 작업으로 가능.
- **ADR-0005 (캔버스 라이브러리)** — R3(`0003`) 기반. Svelte Flow @xyflow/svelte v1.5 결정 잠금 ADR.
- **ADR-0006 (영속화 storage)** — R6(`0006`) 기반. 3단계 prereq (1단계 아님 — C5 B2 정정).
- **C5 Advisory** (A1·A2·A3·A5, Cosmetic C1·C2 in `docs/reports/0013`) — Batch C 종료 후 병렬 처리.

## 환경·도구 메모

- **Memory files** (사용자 전역 + 프로젝트):
  - `~/.claude/CLAUDE.md` (graphify skill 참조)
  - `/Users/ws/Desktop/projects/gtmux/CLAUDE.md` (프로젝트 영구 룰)
- **Project memory** (`/Users/ws/.claude/projects/-Users-ws-Desktop-projects-gtmux/memory/`):
  - `MEMORY.md` (index)
  - `project_gtmux.md` (canonical name 등)
  - `feedback_language_and_adr.md` (KO docs / EN code / ADR-before-code)
  - `feedback_grill_style.md` (기술 디테일 brief+진행, 도메인·UX 옵션+확인)
- **MCP**: `code-review-graph` 활성, 그래프 빌드 완료. **코드 탐색 시 Grep 대신 우선 사용**.
- **Skills 활성** (mattpocock 외부 + 프로젝트 + SC 등): `/diagnose`, `/triage`, `/grill-with-docs`, `/tdd`, `/handoff` 등.
- **Subagents 가용**: `backend-architect`, `frontend-architect`, `system-architect`, `devops-architect`, `security-engineer`, `quality-engineer`, `self-review`, `deep-research`, 기타.

## 사용자 피드백 메모리 (영구 룰)

1. **기술 디테일 결정**(숫자·envelope shape·debounce·내부 임계값 등) → 깊은 근거 분석 후 *권장값 + 근거 1-2줄 brief* + 그대로 진행. "동의?" confirm 묻지 않음.
2. **도메인·UX·정책 결정** → 옵션 비교 + 권장 + 확인 받음.
3. **언어 컨벤션**: 코드/식별자/CLAUDE.md = English. docs/sketch/ADR/SSoT/reports = Korean.
4. **ADR-before-code 절대 룰** — 구현 시작 전 ADR 발행 필수 (현재 9 ADRs Accepted, 1단계 진입 자격 충족).

## 변경 이력

- 2026-05-14: 초안 (Batch C5 완료 + Sprint 0 dispatch 직전 상태 캡처)
