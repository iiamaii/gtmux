# 세션 핸드오프 — 2026-05-14 (Stage A 마무리 + Sprint 7 진입 직전)

본 문서는 `0021-session-handoff.md` 의 후속이다. 그러나 본 세션의 작업이 **아키텍처 pivot 결정 (ADR-0013 채택, tmux 드롭)** + **Stage A 마무리 (CONTEXT.md + 7 ADR amend + sketch.md 4 sections rewrite)** 로 워낙 큰 폭이라, 0021 은 *historical snapshot* 이 되고 본 문서가 **현 시점 단일 진실** 이다. **CLAUDE.md + CONTEXT.md + 본 문서 + 0023 + 0022 + ADR-0013 + ADR-0014 = 핵심 7개** 만 읽으면 작업이 이어진다.

## TL;DR

- **아키텍처 pivot**: 2026-05-14 동일 일자에 (a) grilling 6 L amend (0022), (b) PTY POC 게이트 통과 (0023), (c) **tmux 드롭 결정** (ADR-0013 채택, ADR-0001 deprecate), (d) Stage A 문서 정렬 (본 문서) 4단계 진행.
- **현 단계**: Sprint 5 의 *demo 가 main 에서 실행 가능* 한 상태는 보존 (tmux backend 위에서). **Sprint 7 (architecture pivot 본격 swap)** 가 다음 작업 단위.
- **회귀 게이트**: main 기준 backend `cargo test --workspace --tests` **192 / 0 / 5**, frontend `svelte-check` **224 files / 0 errors / 0 warnings**, main bundle **11.41 KB gzip**. (POC 는 `experiments/pty-poc/` 별 브랜치, main 영향 0.)
- **차단성 갭 0건**. Sprint 7 의 본격 코드 swap 으로 진입 가능.

## 1. 우선 읽을 문서 (순서대로)

1. `CLAUDE.md` — 프로젝트 메타 (EN)
2. `CONTEXT.md` — 도메인 어휘 + 불변식 (KO, **2026-05-14 amend ×2** — grilling + ADR-0013 채택)
3. **`docs/reports/0023-pty-poc-verification-and-decision.md`** — POC 검증 + tmux 드롭 결정 정본 + 로드맵
4. **본 문서 (`0024-stage-a-closeout-handoff.md`)** — Stage A 마무리 + Sprint 7 진입점
5. `docs/reports/0022-logic-amendment-decisions.md` — grilling 6 L 결정 SSoT (살아남는 항목)
6. **`docs/adr/0013-pty-direct-no-tmux.md`** — 신 canonical 아키텍처 ADR
7. **`docs/adr/0014-process-supervisor.md`** — supervisor 라이프사이클 ADR
8. `docs/adr/0001-tmux-integration-control-mode.md` — **Deprecated** (historical reference 만)
9. `docs/adr/0009-tmux-daemon-isolation.md` — **Deprecated** (historical reference 만)
10. 살아있는 ADR (모두 2026-05-14 amend 반영): 0002 (전송) / 0003 (보안) / 0004 (terminal render) / 0005 (canvas lib) / 0006 (persistence) / 0007 (1:1:1) / 0008 (single-pane + Group) / 0010 (group data) / 0011 (Rust backend) / 0012 (Svelte frontend)
11. `docs/sketch.md` §10.1·§11.2·§13.3.6·§14 — 본 세션 rewrite. §10.3·§15 1단계도 갱신.
12. `docs/reports/0020-debug-classification.md` — Sprint 5 17건 결함 분류 (historical, 그러나 pivot 의 근거).
13. `docs/reports/0021-session-handoff.md` — Sprint 5 closeout snapshot (참조용).

## 2. 본 세션 산출 종합

### 2.1 Phase 1 — Grilling (commit `0f3b1a3`, on main)
- 6개 L 항목 grilling → CONTEXT.md + ADR-0001/0002/0003/0009 amend + 새 보고서 `0022-logic-amendment-decisions.md`.
- 결정 매트릭스: L-2/L-9 (CORS) / L-3 (catch-up) / L-4 (token) / L-7 (argv quoting) / L-12 (late-mount buffer) / L-17 (prevention).

### 2.2 Phase 2 — POC (commit `c637c39`, on `poc/pty-direct`)
- `experiments/pty-poc/` 199 LOC Rust + 75 LOC HTML.
- Day 1·2 게이트 통과 + multi-tab mirror 보너스.

### 2.3 Phase 3 — Architecture pivot 결정 (commit `808935e`, on main)
- 신 ADR-0013 (PTY direct, no tmux) — canonical 결정.
- ADR-0001 → Deprecated.
- 새 보고서 `0023-pty-poc-verification-and-decision.md` — POC 검증 + 결정 정본 + 로드맵.

### 2.4 Phase 4 — Stage A 마무리 (commit 본 세션 종료 시점, on main)
- 신 ADR-0014 (Process supervisor) — daemon 라이프사이클.
- ADR-0009 → Deprecated.
- ADR-0008 amend — allowlist 폐기, single-pane-per-process 의미 단순화.
- ADR-0007 amend — Session 어휘 logical 식별자로 의미 단순화.
- ADR-0002 amend — envelope의 PTY-domain 라벨, D7 backpressure pause-after 폐기, D4 CTRL payload schema enum.
- ADR-0003 amend — D7 (argv 분리) tmux 측 폐기, D8 (식별자 정규식) tmux 측 폐기.
- CONTEXT.md amend — tmux 어휘 (Window / tmux Layout / tmux session) 영구 폐기, Pane 정의 갱신.
- sketch.md §10.1 / §11.2 / §13.3.6 / §14 / §15 1단계 rewrite.
- **본 보고서 0024**.

## 3. ADR 매트릭스 (현재 상태)

| ADR | 제목 | 상태 |
|---|---|---|
| 0001 | tmux 통합 = 컨트롤 모드 단일 채널 | **Deprecated** (2026-05-14, superseded by 0013) |
| 0002 | 전송 = WebSocket + 이진 envelope + HTTP 분리 | Accepted (2026-05-14 amend ×2) |
| 0003 | 보안 디폴트 | Accepted (2026-05-14 amend ×2) |
| 0004 | 터미널 렌더링 = xterm.js v6 | Accepted |
| 0005 | 캔버스 라이브러리 = @xyflow/svelte | Accepted |
| 0006 | 영속화 = plain JSON + atomic write | Accepted |
| 0007 | Server : Session : Port 1:1:1 | Accepted (2026-05-14 amend) |
| 0008 | Single-pane + Group | Accepted (2026-05-14 amend — allowlist 폐기, single-pane-per-process 의미) |
| 0009 | tmux daemon 격리 | **Deprecated** (2026-05-14, superseded by 0014) |
| 0010 | Group 데이터 모델 | Accepted |
| 0011 | Backend stack = Rust + axum | Accepted (Sprint 7 시 portable-pty 추가 amend) |
| 0012 | Frontend stack = Svelte 5 + Vite | Accepted |
| **0013** | **PTY direct, no tmux (신규)** | **Accepted** (2026-05-14) |
| **0014** | **Process supervisor (신규)** | **Accepted** (2026-05-14) |

## 4. Sprint 7 로드맵 (0023 §5.3 정본)

### 4.1 폐기 / 무효화 (0022 Sprint 6 plan 에서)
- `S6-LIFE-AUTOSPAWN` (prevention 으로 흡수 — 0022 §1.3)
- `S6-BE-CTRL-ACK` (tmux CTRL response 자체가 사라짐)
- `S6-ARGV-QUOTE` (tmux 명령을 안 보냄)
- `S6-BE-CLOSE` (KillWindow allowlist — 컨셉 폐기)

### 4.2 재정의 / 유지
- `S6-WS-WINDOW-CATCHUP` → **`S7-BE-AUTOMOUNT`** (backend 가 PTY spawn 시 자동 layout PUT + LAYOUT_CHANGED broadcast)
- `S6-FE-SHUTDOWN` (헤더 메뉴 + Session shutdown action) — 의미만 *Server 종료* 로 단순화
- `S6-FE-CLOSE-GUARD` (마지막 panel close 비활성) — *살아 있는 child process 수 = 1* 일 때 비활성으로 의미 정리

### 4.3 신규 (Sprint 7 핵심)
- **`S7-PTY-BACKEND`**: `crates/pty-backend` 신규. portable-pty + tokio broadcast/mpsc + child supervisor. lifecycle crate 폐기, mux-router 의미 단순화 (`wire-router` rename 또는 합병).
- **`S7-WS-PAYLOAD-SIMPLIFY`**: WS envelope 의 CTRL (0x01) payload schema → 우리 API command enum.
- **`S7-MIGRATE`**: frontend `Canvas` / `PanelNode` / `XtermHost` / `dispatcher` 의 backend 측면 의존성을 새 wire 에 맞춰 재배선.
- **`S7-PERSISTENCE-MINIMAL`**: ADR-0006 implement (layout snapshot only, process state 보존 비범위).
- **`S7-DEMO-STAB`**: sketch §15 2단계 demo 를 새 backend 위에서 재구동, 17건 부류 회귀 확인.

### 4.4 일정 추정
- S7-PTY-BACKEND: 4-7 일
- S7-WS-PAYLOAD-SIMPLIFY + S7-MIGRATE: 2-3 일
- S7-PERSISTENCE-MINIMAL: 2-3 일
- S7-DEMO-STAB: 3-7 일

**총**: 2-3 주.

## 5. 살아남는 0022 결정 (Sprint 7 안에서 그대로 적용)

| L | 결정 | Sprint 7 영향 |
|---|---|---|
| L-2/L-9 | CORS 합성 + loopback alias | Backend `config` crate 그대로 (0f3b1a3 commit 의 effective_cors_origins). 변경 없음. |
| L-3 | Static-state = layout pull-through-notify | 새 `S7-BE-AUTOMOUNT` 의 정신 그대로. backend 가 PTY spawn 시 자동 layout PUT + LAYOUT_CHANGED broadcast. |
| L-4 | HttpOnly cookie 폐기, sessionStorage 단일 | 변경 없음 (frontend 측, backend 무관). |
| L-7 | argv quoting | **무효화** — tmux 명령 발급 안 함. lifecycle crate 폐기로 자동 폐기. |
| L-12 | per-pane 256 KiB FIFO drop-oldest late-mount buffer | 변경 없음 (frontend dispatcher 측). |
| L-17 | prevention 모델 (close 비활성 + auto-mount + Session shutdown) | Sprint 7 의 `S7-FE-CLOSE-GUARD` + `S7-FE-SHUTDOWN` + `S7-BE-AUTOMOUNT` 에 1:1 매핑. 의미는 "tmux invariant" → "우리 child process 라이프사이클 invariant" 로 단순화. |

## 6. 환경·도구

- **Memory files** (`/Users/ws/.claude/projects/-Users-ws-Desktop-projects-gtmux/memory/`): 그대로
  - `MEMORY.md` (index)
  - `project_gtmux.md`, `feedback_language_and_adr.md`, `feedback_grill_style.md`
- **MCP**: `code-review-graph` 활성. Grep 대신 우선 사용.
- **Subagents 가용**: backend-architect, frontend-architect, system-architect, devops-architect, security-engineer, quality-engineer, technical-writer, deep-research, refactoring-expert 등.
- **사용자 피드백 룰**:
  1. 기술 디테일 결정 → brief + 진행 (confirm 묻지 않음)
  2. 도메인/UX/정책 → 옵션 비교 + 확인
  3. KO docs / EN code
  4. ADR-before-code

## 7. 다음 세션 첫 메시지 가이드

사용자가 "Sprint 7 진행" 또는 "본격 swap 시작" 이라 하면 §4.3 순서 권장:

1. **S7-PTY-BACKEND** — `backend-architect` agent 에게 dispatch. POC 코드 (`experiments/pty-poc/src/main.rs` 199 LOC) 를 production-grade 로 — 다중 pane 지원 + ring buffer + 백프레셔 + 테스트 + 별 crate (`crates/pty-backend`). `crates/lifecycle` 폐기는 같은 PR.
2. **S7-WS-PAYLOAD-SIMPLIFY** + **S7-MIGRATE** — `S7-PTY-BACKEND` PR merge 후 진입. WS envelope 의 CTRL payload schema 단순화 + frontend 측 wire 재배선.
3. **S7-PERSISTENCE-MINIMAL** — `S7-MIGRATE` 후. ADR-0006 implement (layout snapshot).
4. **S7-DEMO-STAB** — closeout. Sprint 5 demo 의 17건 부류 회귀 + manual smoke probe.

사용자가 "POC 다시 확인" 또는 "PTY 검증 재현" 이라 하면:
```bash
git checkout poc/pty-direct
cd experiments/pty-poc && cargo run --release
# browse http://127.0.0.1:9100
```

사용자가 "현 상태 점검" 또는 "tmux backend demo" 이라 하면 main 브랜치에서 `0021-session-handoff.md` §11 의 데모 절차 그대로 작동 — Sprint 7 swap 전까지 main 은 tmux backend 로 운영 가능.

사용자가 "Stage A 마무리 진행" 신호가 끝났음을 확인 하려면 본 문서 §2.4 의 7개 amend + sketch rewrite 가 모두 정합한지 git log 로 확인 — commit message + 디렉터리 변동.

## 8. Carry-forward (Sprint 7 이후)

- ADR-0006 의 구체 구현 (sqlite vs JSON file, WAL 정책 등) — Sprint 8.
- TLS / cloud 모드 helper — sketch §15 4단계.
- Playwright 시각 검증 자동화 — smoke step 8 N/A 해소 prereq.
- CI 캐시 도입 (sccache, cargo-cache).
- GitHub push `iiamaii/gtmux` — credential 사용자 영역.
- 외부 attach 가 필요한지 사용자 검증 후 CLI client 도입 결정 (ADR-0013 D8 / 0023 §7.2 O3).
- xterm.js 키맵 갭 (Shift / Option) — `SECURE_XTERM_OPTIONS` amend 로 흡수 (Sprint 7 backlog).

## 9. 안티패턴 / 함정 누적 — 정본은 `0020` + `0022` + `0023`

본 세션 grilling 으로 정리된 함정 부류:
- L-7 (argv quoting / # quirk) — tmux 어휘와 함께 영구 소거
- L-3 (broadcast::Sender late-subscriber 미배달) — auto-mount + layout pull-through-notify 로 우회
- L-12 (mount-vs-emit race) — per-pane FIFO buffer 로 흡수
- L-4 (HttpOnly cookie + sessionStorage 이중 진실) — sessionStorage 단일로 단순화
- L-17 (last-window-close = server-die) — prevention 으로 차단, ADR-0013 채택 후 *Server 가 우리 process owner* 이므로 의미 자체가 단순해짐

본 세션 추가된 디버깅 도구 (Sprint 5 의 `console.debug` 5줄 + `tracing::debug!` 5줄) 는 Sprint 7 의 `S7-PTY-BACKEND` swap 시 *대체* — tmux 측 debug 가 PTY 측 debug 로 변환. retire 시점은 Sprint 7 closeout.

## 변경 이력

- 2026-05-14: 초안 — Stage A 마무리 + Sprint 7 진입점.
