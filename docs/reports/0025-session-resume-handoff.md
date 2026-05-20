# 세션 핸드오프 — 2026-05-14 (session-clear → 재개용 전용)

본 문서는 `0024-stage-a-closeout-handoff.md` 의 후속이지만, **콘텍스트 초기화 후 cold 픽업** 을 위한 *self-contained* 핸드오프다. 0021 / 0024 가 진행 흐름의 historical snapshot 이라면, 본 문서는 *지금 시점에 새 세션에 합류하는 agent 가 읽어야 할 단일 진실*이다.

새 세션은 다음 4 개 문서만 읽으면 작업 재개 가능:
1. `CLAUDE.md` (프로젝트 메타, EN)
2. `CONTEXT.md` (도메인 어휘, 2026-05-14 amend ×2 반영)
3. **본 문서** (`0025-session-resume-handoff.md`)
4. `docs/adr/0013-pty-direct-no-tmux.md` (현 canonical 아키텍처)

추가로 필요 시 §6 의 reading list 참조.

---

## 0. 한 줄 상태

- **현 시점**: tmux 드롭 결정 채택 (2026-05-14), 모든 문서 정렬 완료 (Stage A 마무리), 본격 코드 swap (Sprint 7 = Stage B) 직전.
- **main 브랜치**: 여전히 *tmux backend 위에서 실행 가능한* Sprint 5 demo 상태. 코드 변경 0.
- **`poc/pty-direct` 브랜치**: throwaway POC (`experiments/pty-poc/`, 199 LOC). Day 1+2 게이트 통과 — *tmux 없는 PTY 직접 backend* 가 작동 가능함의 증거.
- **차단성 갭 0건**. 다음 세션 = "Sprint 7 진입" 신호 한 줄이면 즉시 작업 시작.

---

## 1. 핵심 결정 요약 (시간순)

### 1.1 Phase 1 — Grilling (commit `0f3b1a3`)
Sprint 5 demo 안정화 17건 결함 (0020 분류) 중 7건 L-class (planning ambiguity) 를 grilling 으로 해결:

| L | code | 채택 결정 |
|---|---|---|
| L-2 / L-9 | CORS | `cors_origins` 빈 셋 + loopback bind → `127.0.0.1`/`localhost`/`[::1]` 3 origin 자동 합성. 0.0.0.0 = cloud = 명시 의무 |
| L-3 | static-state catch-up | 별도 mirror cache 폐기. backend auto-mount → layout PUT → LAYOUT_CHANGED → GET 으로 흡수 |
| L-4 | token 채널 | HttpOnly cookie 폐기. sessionStorage 단일. D6 3축 → 2축 |
| L-7 | argv quoting | selective single-quote wrap (`#`/공백/`'`/`"`/`\`). **Sprint 7 진입 후 moot** — tmux 명령 발급 안 함 |
| L-12 | late-mount buffer | per-pane 256 KiB FIFO drop-oldest, lifetime = first registerPaneOut OR pane close |
| L-17 | last-window invariant | recovery → prevention 전환. tmux window=1 일 때 close 비활성, 항상 auto-mount, Session shutdown UI 액션. LIFE-AUTOSPAWN 취소 |

SSoT: `docs/reports/0022-logic-amendment-decisions.md`.

### 1.2 Phase 2 — PTY POC (commit `c637c39`, on `poc/pty-direct`)
`experiments/pty-poc/`. portable-pty 0.9 + tokio broadcast + axum WS + xterm.js. **Day 1·2 게이트 100% 통과 + multi-tab mirror 보너스** (tokio::broadcast 만으로 tmux 의 multi-attach mirror 동등 달성).

게이트:
- ✅ #1 Signal (Ctrl-C/D/Z + fg)
- ✅ #2 Resize (SIGWINCH → vim/tput reflow)
- ✅ #3 Alt-screen (vim/less)
- ✅ #4 Shell exit + zombie reap
- ✅ #5 Burst throughput (yes / /dev/urandom)
- ⚠️ Shift/Option 모디파이어 = xterm.js config 레이어, *backend 무관* — Sprint 7 backlog 분리

### 1.3 Phase 3 — Architecture pivot (commit `808935e`)
ADR-0013 (PTY direct, no tmux) 신규 채택. ADR-0001 (tmux control mode) → Deprecated.

핵심 결정: **tmux 드롭**. portable-pty 직접 ownership + 우리 API command schema enum + 단일 wire envelope (PTY-domain + web-domain). 새 코드 ~1500 LOC, 폐기 코드 ~2700 LOC (lifecycle + mux-router).

SSoT: `docs/reports/0023-pty-poc-verification-and-decision.md`.

### 1.4 Phase 4 — Stage A 마무리 (commit `fa354a0`)
- 신 ADR-0014 (Process supervisor) — ADR-0009 supersede
- ADR-0009 → Deprecated
- ADR-0007 / 0008 / 0002 / 0003 amend
- CONTEXT.md 전면 amend (tmux 어휘 영구 폐기)
- sketch.md §10.1·§10.3·§11.1·§11.2·§13.3.6·§14·§15.1단계 rewrite
- 보고서 0024 (Stage A closeout handoff)

---

## 2. 현재 ADR 매트릭스 (2026-05-14 기준 정본)

| ADR | 제목 | 상태 |
|---|---|---|
| 0001 | tmux 통합 = 컨트롤 모드 단일 채널 | **Deprecated** (superseded by 0013) |
| 0002 | 전송 = WebSocket + 이진 envelope + HTTP 분리 | Accepted (2026-05-14 amend ×2) |
| 0003 | 보안 디폴트 | Accepted (2026-05-14 amend ×2) |
| 0004 | 터미널 렌더링 = xterm.js v6 | Accepted |
| 0005 | 캔버스 라이브러리 = @xyflow/svelte | Accepted |
| 0006 | 영속화 = plain JSON + atomic write | Accepted (Sprint 7 시 implement 진행) |
| 0007 | Server : Session : Port 1:1:1 | Accepted (2026-05-14 amend — Session = logical 식별자) |
| 0008 | Single-pane + Group | Accepted (2026-05-14 amend — allowlist 폐기, single-pane-per-process) |
| 0009 | tmux daemon 격리 | **Deprecated** (superseded by 0014) |
| 0010 | Group 데이터 모델 | Accepted |
| 0011 | Backend stack = Rust + axum | Accepted (Sprint 7 에서 portable-pty crate 추가 amend) |
| 0012 | Frontend stack = Svelte 5 + Vite | Accepted |
| **0013** | **PTY direct, no tmux** | **Accepted** (신규, 2026-05-14) |
| **0014** | **Process supervisor** | **Accepted** (신규, 2026-05-14) |

---

## 3. 도메인 어휘 변경 (CONTEXT.md amend 핵심)

새 세션 agent 가 *옛 어휘* 를 쓰지 않게 명확히:

| 폐기 어휘 | 대체 어휘 / 처리 |
|---|---|
| tmux Window | **영구 폐기**. UI 측 묶음은 Group 이 담당 |
| tmux Layout | **영구 폐기**. Canvas Layout 만 |
| tmux session | **logical Session** (사용자 부여 식별자) |
| tmux control mode client | **Process supervisor** (portable-pty + tokio broadcast) |
| tmux command allowlist | **API command schema enum** (Rust enum exhaustive match) |
| `refresh-client -A pause/continue` | **broadcast subscribe drop/재등록** (Panel Streaming State 구현) |
| `%output` / `%session-changed` / `%window-add` | **PTY master fd raw bytes** + **우리 API NOTIFY_MIRROR** |
| daemon outlives Server | **Server 가 owner — 종료 시 모든 child 자손 정리** |
| `tmux -L gtmux-<session>` 소켓 | `${XDG_STATE_HOME}/gtmux/<session>.lock` 파일 |

핵심 정의 (현 CONTEXT.md):
- **Pane** = gtmux Server 가 직접 관리하는 PTY pair (master + slave) + child process (shell) 의 1:1 묶음
- **Session** = 한 Server 가 부팅 시 CLI 인자로 부여받는 logical 식별자
- **Server** = logical Session 1:1 + 단일 포트 + 모든 PTY/child 의 owner
- **Pane ↔ Panel** = 1:1 auto-mount (Sprint 5 에서 0개 이상이었던 cardinality 가 정확히 1개로 단순화)

---

## 4. Sprint 7 (Stage B) — 본격 코드 swap

다음 세션의 핵심 작업. 2-3 주 예상.

### 4.1 Task 순서 (직렬 권장)

#### S7-PTY-BACKEND (4-7일) — 시작점
`crates/pty-backend` 신규 (workspace 안). POC 코드를 production-grade 로:
- 다중 pane 지원 (pane_id ↔ PTY pair + child process 매핑)
- per-pane ring buffer (128 KiB 기본, ADR-0001 D7 정신 계승)
- 백프레셔 watermark (ADR-0002 §D7 amend — broadcast cap + master fd 자연 backpressure)
- spawn/kill/wait/reap + SIGWINCH 처리
- `tokio::process` + `portable-pty` 통합
- 단위 테스트 (POC Gate #1~#5 + multi-pane variants)
- `crates/lifecycle` 폐기, `crates/mux-router` 의미 단순화 (`crates/wire-router` rename 또는 합병)

**Dispatch prompt** (backend-architect agent 용 — 새 세션에서 즉시 사용 가능):
```
gtmux Sprint 7-A 작업. PTY direct backend 본격 구현.

입력 (반드시 읽을 것):
- docs/adr/0013-pty-direct-no-tmux.md (canonical 아키텍처)
- docs/adr/0014-process-supervisor.md (라이프사이클)
- experiments/pty-poc/src/main.rs (POC 199 LOC, throwaway 참조)
- docs/reports/0023-pty-poc-verification-and-decision.md §5.3 (S7-PTY-BACKEND scope)
- docs/reports/0025-session-resume-handoff.md §4.1
- docs/adr/0002-transport-websocket.md (envelope, 2026-05-14 amend ×2)
- codebase/backend/Cargo.toml (workspace dependencies)

작업:
1. codebase/backend/crates/pty-backend 신규 crate. portable-pty 0.9 + tokio
   broadcast/mpsc. 다중 pane 지원, per-pane ring buffer (128 KiB), 백프레셔
   watermark (high 512 KiB / low 128 KiB).
2. ADR-0013 D2/D7 + ADR-0014 D2/D6/D7 의 라이프사이클 정확 반영.
3. crates/lifecycle 폐기 (TmuxDaemon 등 lifecycle 모듈 전부).
4. crates/mux-router 의미 단순화: API command schema enum (new-pane, kill-pane,
   resize-pane, set-cwd, set-env). serialise_command 폐기. argv quoting 폐기.
   이름은 wire-router 로 rename 또는 ws-server 안에 흡수 — 판단 후 결정.
5. ws-server 의 CTRL (0x01) 라우터 재배선 — 새 API enum dispatch.
6. 단위 테스트: POC Gate #1~#5 자동 재현 + multi-pane race + zombie reap.
7. cargo test --workspace --tests, clippy -D warnings, fmt clean 유지.
8. bin/gtmux-cli 의 wiring 갱신 (TmuxDaemon spawn 호출 폐기, pty-backend 로 대체).

DoD: workspace 빌드 통과, 신 crate 의 단위 테스트 PASS, clippy/fmt clean.
변경 파일 + 새 테스트 이름 + LOC 변동 보고.
```

#### S7-WS-PAYLOAD-SIMPLIFY + S7-MIGRATE (2-3일)
- WS envelope 의 CTRL (0x01) payload schema 단순화 — tmux argv → 우리 API enum.
- frontend `lib/ws/dispatcher.svelte.ts` / `lib/canvas/{Canvas,PanelNode,XtermHost,NewPanelButton}.svelte` 의 backend 측면 의존성을 새 wire 에 맞춰 재배선.
- 0022 의 L-3/L-12/L-17 정신 그대로 유지 — auto-mount loop / late-mount buffer / close-guard.

#### S7-PERSISTENCE-MINIMAL (2-3일)
- ADR-0006 implement — layout snapshot only (plain JSON file + atomic-write-file + sidecar quarantine, ADR-0006 그대로).
- process state 보존 비범위 (ADR-0013 D9 + ADR-0014 D5).

#### S7-DEMO-STAB (3-7일, closeout)
- sketch §15 2단계 demo 를 새 backend 위에서 재구동.
- Sprint 5 의 17건 결함 부류 회귀 확인.
- 새 부류 결함 발생 가능성 인지 (PTY edge case — alt-screen / OSC / $TERM 변종).

### 4.2 신규 추가 작업 (Sprint 7 에 포함)
- **S7-FE-SHUTDOWN** (CONTEXT.md §"Pane lifecycle invariant" 정합): 우상단 헤더 메뉴 + Session shutdown action + confirm modal + API `kill-session` 호출 → graceful exit 6
- **S7-FE-CLOSE-GUARD**: panel close 버튼 비활성화 (살아 있는 child process 수 = 1 일 때) + tooltip
- **S7-BE-AUTOMOUNT**: backend 가 PTY spawn 시 자동 layout PUT + LAYOUT_CHANGED broadcast (L-3 의 정신)
- **S7-XTERM-KEYMAP** (orthogonal): `SECURE_XTERM_OPTIONS` 에 `macOptionIsMeta: true` 등 추가 (POC 에서 발견된 Shift/Option 갭)

---

## 5. 살아남는 0022 결정 매트릭스

본 amend 작업의 일부는 *Sprint 7 후* 에도 그대로 유효함 — 본 세션의 grilling 작업이 *throw* 가 되지 않음:

| L | 결정 | Sprint 7 이후 |
|---|---|---|
| L-2 / L-9 | CORS 합성 + loopback alias | 그대로 (config crate, backend 무관) |
| L-3 | static-state via layout pull-through-notify | **S7-BE-AUTOMOUNT** 가 본 정신 구현 |
| L-4 | sessionStorage 단일 채널 | 그대로 (frontend, backend 무관) |
| L-7 | argv quoting | **무효화** — tmux 명령 발급 없음 |
| L-12 | per-pane 256 KiB FIFO drop-oldest | 그대로 (frontend dispatcher) |
| L-17 | prevention 모델 | **S7-FE-CLOSE-GUARD + S7-FE-SHUTDOWN + S7-BE-AUTOMOUNT** 가 본 정신 구현. *invariant 의 주체* 가 "tmux invariant" → "우리 child process lifecycle invariant" 로 단순화 |

---

## 6. Reading list (우선순위 순)

### 6.1 Tier 1 — 새 세션이 반드시 읽어야 (cold pickup 필수)
1. `CLAUDE.md` — 프로젝트 메타, EN
2. `CONTEXT.md` — 도메인 어휘 (2026-05-14 amend ×2 반영, tmux 어휘 폐기)
3. **본 문서 (`0025-session-resume-handoff.md`)** — 단일 진실 진입점
4. `docs/adr/0013-pty-direct-no-tmux.md` — canonical 아키텍처

### 6.2 Tier 2 — 작업 시작 전 권장
5. `docs/adr/0014-process-supervisor.md` — supervisor 라이프사이클
6. `docs/reports/0023-pty-poc-verification-and-decision.md` — POC 검증 + 로드맵
7. `docs/reports/0022-logic-amendment-decisions.md` — grilling 6 L 결정 (살아남는 정책)
8. `docs/adr/0002-transport-websocket.md` — envelope 의미 (2026-05-14 amend ×2)
9. `docs/sketch.md` §10.1·§11.2·§13.3.6·§14 — backend 아키텍처 재정의

### 6.3 Tier 3 — 살아있는 ADR (참조용)
10. `docs/adr/0003-security-defaults.md` — 보안 (2026-05-14 amend ×2)
11. `docs/adr/0007-server-session-port-binding.md` — 1:1:1 모델
12. `docs/adr/0008-single-pane-window-and-group.md` — Group + (구) single-pane
13. `docs/adr/0010-group-data-model.md` — Group 데이터
14. `docs/adr/0004-terminal-rendering.md` — xterm.js
15. `docs/adr/0005-canvas-library.md` — @xyflow/svelte
16. `docs/adr/0006-persistence-storage.md` — 영속화 (Sprint 7 시 implement)
17. `docs/adr/0011-backend-stack-rust.md` — Rust workspace
18. `docs/adr/0012-frontend-stack-svelte.md` — Svelte

### 6.4 Tier 4 — Deprecated / Historical
- `docs/adr/0001-tmux-integration-control-mode.md` — Deprecated, *읽지 말 것*
- `docs/adr/0009-tmux-daemon-isolation.md` — Deprecated, *읽지 말 것*
- `docs/reports/0021-session-handoff.md` — Sprint 5 closeout, historical
- `docs/reports/0024-stage-a-closeout-handoff.md` — Stage A closeout, 본 0025 가 supersede
- `docs/reports/0020-debug-classification.md` — Sprint 5 결함 분류 (historical 근거)

---

## 7. 현재 코드 상태 + Git 정보

### 7.1 git 브랜치
```
* main           fa354a0 docs: Stage A 마무리 — ADR-0014 신규 + 0009 deprecate + amend ×5 + sketch rewrite
  poc/pty-direct c637c39 poc(pty-direct): throwaway PTY-direct experiment, no tmux
```

### 7.2 main 의 코드 상태 (tmux backend 위에서 demo 실행 가능)
- backend `cargo test --workspace --tests`: **192 PASS / 0 FAIL / 5 ignored**
- frontend `svelte-check`: **224 files / 0 errors / 0 warnings**
- main bundle: **11.41 KB gzip**
- smoke gate `01_engine_connect.sh`: 8 PASS / 1 N/A / 0 GATE / 0 FAIL

### 7.3 진행 경로
- Sprint 5 (tmux backend, sketch §15 2단계 demo) = main 의 현재
- Sprint 7 (PTY direct backend) = 다음 세션
- **Sprint 6 ↔ Sprint 7**: 0022 의 Sprint 6 task 표는 0023 + 0025 로 supersede. Sprint 6 는 *명칭 자체로 폐기* — Sprint 7 가 곧바로 architecture pivot 시작.

### 7.4 untracked (본 세션 무관, baseline 부터 존재)
- `.agents/skills/{debug-issue,explore-codebase,refactor-safely,review-changes}/`
- `.codex/`
- `AGENTS.md`
- `docs/demo-guide.md`, `docs/demo/`
- `experiments/` (디렉터리 자체는 untracked — `experiments/pty-poc/` 는 `poc/pty-direct` 브랜치에서만 tracked)

### 7.5 본 세션의 commit 3건 (main)
- `fa354a0` — Stage A 마무리 (Stage A 본 세션의 최종)
- `808935e` — POC 검증 + ADR-0013 + ADR-0001 deprecate (Phase 3)
- `0f3b1a3` — grilling 6 L amend (Phase 1)

### 7.6 POC 브랜치의 commit 1건
- `c637c39` (on `poc/pty-direct`) — POC 코드. Stage B 시작 시 swap 의 reference.

---

## 8. Risk register / Carry-forward

### 8.1 Risk (Sprint 7 안에 발생 가능)
| 카테고리 | risk | 완화 |
|---|---|---|
| Backend | portable-pty 의 production 사용 시 미발견 함정 (alt-screen edge, OSC, $TERM 변종) | wezterm/alacritty 가 같은 crate 사용 → wisdom 흡수. demo 안정화 cycle 흡수 |
| Backend | 50 pane × 5 burst 스케일에서 backpressure 측정 미실시 | S7-DEMO-STAB 의 정량 게이트 |
| Frontend | Migration 시 dispatcher / Canvas / XtermHost 의 backend 의존성 race | 0022 의 L-3/L-12/L-17 정신 그대로 — pull-through-notify + late-mount buffer + close-guard |
| Persistence | Server 재기동 후 process state 미복원 의 UX 마찰 | ADR-0014 D5 명시 — sketch.md §11.2 D 도 명시. Sprint 8 의 ADR-0006 implement 시 UX 설명 강화 |
| Project framing | "gtmux" 의 g 가 graphical-tmux 의미 → 명칭 모순 | README/sketch 의 framing note 만 갱신, rename 비범위 (0023 §7.2 O3) |

### 8.2 Carry-forward (Sprint 8+)
- ADR-0006 의 sqlite vs JSON 결정 (Sprint 7 의 S7-PERSISTENCE-MINIMAL 은 JSON 으로 우선)
- TLS / cloud 모드 helper (sketch §15 4단계)
- Playwright 시각 검증 자동화 (smoke step 8 N/A 해소 prereq)
- 외부 CLI client (ADR-0013 D8 비범위 결정 — 사용자 검증 후 P1+ 재방문)
- xterm 키맵 갭 (Shift/Option) — `SECURE_XTERM_OPTIONS` amend
- CI 캐시 도입 (sccache, cargo-cache)
- GitHub push (`iiamaii/gtmux`) — credential 사용자 영역

### 8.3 Open questions (다음 세션 시작 시 확인 권고)
- **O1**: `crates/mux-router` 폐기 vs `crates/wire-router` rename. cosmetic 결정 — S7-PTY-BACKEND 안에서 backend-architect agent 가 판단.
- **O2**: SIGTERM → SIGKILL grace period (ADR-0014 D7 의 200ms 잠정값). 50 pane 스케일에서 실측 후 조정.
- **O3**: `${XDG_RUNTIME_DIR}` 부재 시 fallback (ADR-0014 O3) — macOS sandboxed 환경 대응. ADR-0011 D6 (config crate) 와 통합.
- **O4**: Multi-pane × N 스케일 backpressure 측정 (ADR-0013 O2 + ADR-0014 O4) — S7-DEMO-STAB.

---

## 9. 사용자 작업 룰 (메모리 정합)

새 세션 agent 가 사용자와 협업할 때:

1. **기술 디테일 결정** → brief + 진행 (confirm 묻지 않음). 예: 변수명, 파일 위치, 빌드 명령.
2. **도메인 / UX / 정책** → 옵션 비교 + 확인. 예: 새 API 명칭, UI 액션 배치, 보안 정책.
3. **Docs = KO, code = EN**. README/CLAUDE.md/repo-meta = EN.
4. **ADR-before-code** — 새 결정은 ADR 먼저, 그 다음 코드.
5. **Grilling 패턴** — 사용자가 "옵션 비교 + 추천" 을 가장 선호. AskUserQuestion 으로 한 번에 한 결정.

---

## 10. 다음 세션 첫 메시지 가이드 (사용자 입력 별 행동)

| 사용자 메시지 | 행동 |
|---|---|
| "Sprint 7 진행" / "본격 swap 시작" | §4.1 의 S7-PTY-BACKEND dispatch prompt 를 `backend-architect` agent 에게 전달. 본격 작업 진입. |
| "S7-PTY-BACKEND dispatch" | 동상 — agent dispatch. |
| "PTY POC 다시 검증" / "POC 재현" | `git checkout poc/pty-direct && cd experiments/pty-poc && cargo run --release` → http://127.0.0.1:9100 |
| "현 상태 점검" / "tmux backend demo" | main 브랜치에서 0021 §11 demo 절차 그대로 (Sprint 7 swap 전까지 main 은 tmux backend 로 운영) |
| "어떤 결정이 있었어?" / "왜 tmux 드롭?" | 본 문서 §1 + §3 + 0023 §2.2 (근거 6점) 인용 |
| "Stage A 가 뭐였어?" | 본 문서 §1.4 + 0024 §2.4 |
| "ADR 목록" / "현 ADR 상태" | 본 문서 §2 |
| "어휘 변경 정리" | 본 문서 §3 + CONTEXT.md §"Flagged ambiguities" |

---

## 11. 메모리 정합

다음 메모리 파일이 본 세션의 architectural pivot 을 반영하도록 갱신됨:
- `~/.claude/projects/-Users-ws-Desktop-projects-gtmux/memory/project_gtmux.md` — gtmux 의 backend = PTY direct (2026-05-14 amend), tmux 어휘 폐기.

다른 메모리 (feedback_language_and_adr, feedback_grill_style) 는 *backend 무관* 이라 변경 없음.

---

## 변경 이력

- 2026-05-14: 초안 — 콘텍스트 초기화 직전, cold pickup 용 self-contained 핸드오프. 0024 의 *Stage A 흐름* 을 *현 시점 단일 진실* 로 재정렬.
