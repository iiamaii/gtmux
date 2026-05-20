# 세션 핸드오프 — 2026-05-14 (Sprint 7 중간 — S7-PTY-BACKEND + S7-WS-PAYLOAD-SIMPLIFY 완료 시점)

본 문서는 `0025-session-resume-handoff.md` + `0026-stage-b-carry-forward.md` 의 후속이지만, **콘텍스트 초기화 후 cold 픽업** 을 위한 *self-contained* 핸드오프다. 0025/0026 은 historical snapshot 이며, 본 문서가 *지금 시점에 새 세션에 합류하는 agent 가 읽어야 할 단일 진실*.

새 세션은 다음 5 문서만 읽으면 작업 재개 가능:
1. `CLAUDE.md` (프로젝트 메타, EN)
2. `CONTEXT.md` (도메인 어휘, 2026-05-14 amend ×2 반영)
3. **본 문서** (`0027-session-resume-handoff.md`)
4. `docs/adr/0013-pty-direct-no-tmux.md` (현 canonical 아키텍처, 2026-05-14 amend ×3 반영)
5. `docs/adr/0014-process-supervisor.md` (라이프사이클, 2026-05-14 amend 1 반영)

추가로 필요 시 §6 의 reading list 참조.

---

## 0. 한 줄 상태

- **현 시점**: Sprint 7 의 backend wholesale swap (Stage A + Stage B) 완료 + frontend wire 정렬 완료. main 브랜치가 *PTY direct backend 위에서* 빌드/타입/단위 테스트 모두 PASS.
- **남은 Sprint 7 작업 3건**: S7-PERSISTENCE-MINIMAL (ADR-0006), S7-FE-SHUTDOWN + S7-FE-CLOSE-GUARD + S7-BE-AUTOMOUNT, S7-DEMO-STAB. demo 안정화는 *PTY backend 위에서* 처음 실행되는 단계 — Sprint 5 의 17건 결함 부류 회귀 가능성 인지 필수.
- **차단성 갭 0건**.

---

## 1. 본 세션 완료 commits (시간순, 7건)

| commit | 작업 |
|---|---|
| `3623aa3` | Stage A — `crates/pty-backend` 신규 (1098 LOC + 9 integration + 11 unit tests 모두 PASS) |
| `552e5c6` | Stage A grilling amend — ADR-0013 D3·D10·R3 / ADR-0014 D10 amend + 코드 정합 (DaemonStarted→ServerReady, SetCwd/SetEnv 폐기, auto-cleanup wait thread, PaneGone dead code 제거) |
| `5eec978` | Stage B step 1 — gtmux-cli `start()` 의 TMUX env startup guard (ADR-0014 D10 amend 의 1차 방어, exit 4) |
| `fbb716a` | 0026 carry-forward 보고서 작성 (Stage B step 2 entry point) |
| `0a7cd65` | Stage B step 2 — wholesale code swap: lifecycle (2467 LOC) + mux-router (897 LOC) 폐기, ws-server CTRL 재배선, gtmux-cli start() 재작성, state_files.rs 신규 |
| `58e2f46` | S7-WS-PAYLOAD-SIMPLIFY — frontend dispatcher / mux store / NewPanelButton / Sidebar 의 wire 정렬 (-152 LOC) |

본 세션 net LOC delta: **3006 insertions / 5047 deletions = -2041 LOC**.

---

## 2. 핵심 결정 요약

### 2.1 Stage A — pty-backend crate 신규

- **D1**: 새 crate `codebase/backend/crates/pty-backend/` 가 ADR-0013 의 PTY direct ownership 정본 구현. `PtyBackend::{new, spawn, kill, resize, send_input, subscribe_output}` + `BackendCommand` (compile-time allowlist enum) + `BackendNotify` (NOTIFY_MIRROR payload enum).
- **D2**: pane 당 3 thread (reader / writer / wait), broadcast cap 512, ring buffer 128 KiB, SIGTERM grace 200ms. 모두 ADR-0013 D3/D7 + ADR-0014 D2/D6/D7 정합.
- **D3**: wait thread = *detached + auto-cleanup* (Weak<Inner> 로 dashmap self-remove). API 계약 *"pane in map ↔ alive"* 가 invariant. `PaneGone` variant 제거 — `PaneNotFound` 가 post-cleanup 자연 표현.

### 2.2 Grilling amend 5건 (ADR-0013/0014 정합)

1. `BackendNotify::DaemonStarted` → `ServerReady` (ADR-0014 D1 daemon 어휘 폐기 정합)
2. `BackendCommand::SetCwd` / `SetEnv` 제거 (YAGNI — no-op variant 폐기)
3. ADR-0013 D3 amend — backend ring 128 KiB vs frontend dispatcher 256 KiB (0022 L-12) layered late-mount 책임 경계 명시
4. ADR-0014 D10 amend — nested-tmux 차단 2-layer 방어 (1차 = gtmux-cli startup TMUX env detect → exit 4 / 2차 = PtyBackend spawn 시 5종 env scrub)
5. ADR-0013 R3 amend — dead pane auto-cleanup (wait thread self-remove)

### 2.3 Stage B — lifecycle / mux-router 영구 폐기

- `crates/lifecycle/` (2467 LOC) + `crates/mux-router/` (897 LOC) 디렉터리 통째 삭제
- ws-server: handle_socket 재배선 (Hub.subscribe_pane_output multiplex + backend.send_input/resize 직접 호출), cmd_router 재작성 (BackendCommand allowlist dispatch), hub 재작성 (PtyBackend wrap + 다중 pane output multiplexer)
- gtmux-cli: TmuxDaemon spawn + run_event_loop + run_command_loop 폐기 → `PtyBackend::new()` 단일 호출. graceful shutdown = `drop(backend)` (ADR-0014 D5 자손 SIGHUP 자동 정리)
- 신규 `bin/gtmux-cli/src/state_files.rs` (409 LOC) — lifecycle 의 OS-level utility (pidfile_*, stop_server, teardown 4단계) 흡수. FFI unsafe 는 module 내 격리

### 2.4 Frontend wire 정렬 (S7-WS-PAYLOAD-SIMPLIFY + S7-MIGRATE)

- CTRL cmd vocabulary: `'new-window'` → `'new-pane'`, args 인자 폐기
- NOTIFY_MIRROR kind 6 변종 (window-add/renamed/close, session-changed, layout-change, pane-mode-changed, subscription-changed, slow-pane) 폐기 → `pane-spawned` / `pane-died` / `layout-changed` / `server-ready` 4종
- muxStore 슬림화: `windows` / `session` surface 폐기, `MirroredPane` 의 window_id / mode 제거. ~150 → ~60 LOC

---

## 3. 현재 ADR 매트릭스 (2026-05-14 기준 정본)

| ADR | 제목 | 상태 |
|---|---|---|
| 0001 | tmux 통합 = 컨트롤 모드 단일 채널 | **Deprecated** (superseded by 0013) |
| 0002 | 전송 = WebSocket + 이진 envelope + HTTP 분리 | Accepted (2026-05-14 amend ×2) |
| 0003 | 보안 디폴트 | Accepted (2026-05-14 amend ×2) |
| 0004 | 터미널 렌더링 = xterm.js v6 | Accepted |
| 0005 | 캔버스 라이브러리 = @xyflow/svelte | Accepted |
| 0006 | 영속화 = plain JSON + atomic write | Accepted (Sprint 7 의 S7-PERSISTENCE-MINIMAL 에서 implement) |
| 0007 | Server : Session : Port 1:1:1 | Accepted (2026-05-14 amend — Session = logical 식별자) |
| 0008 | Single-pane + Group | Accepted (2026-05-14 amend — allowlist 폐기, single-pane-per-process) |
| 0009 | tmux daemon 격리 | **Deprecated** (superseded by 0014) |
| 0010 | Group 데이터 모델 | Accepted |
| 0011 | Backend stack = Rust + axum | Accepted (Sprint 7 에서 portable-pty + dashmap workspace dep 추가, lifecycle/mux-router 폐기) |
| 0012 | Frontend stack = Svelte 5 + Vite | Accepted |
| **0013** | **PTY direct, no tmux** | Accepted (2026-05-14, **amend ×3** — D3 layered buffer / D10 cmd allowlist + envelope kind / R3 auto-cleanup) |
| **0014** | **Process supervisor** | Accepted (2026-05-14, **amend ×1** — D10 2-layer nested-tmux 방어) |

---

## 4. 남은 Sprint 7 작업

### 4.1 S7-PERSISTENCE-MINIMAL (다음 task 1순위, 2-3일)

ADR-0006 의 plain JSON + atomic-write-file + sidecar quarantine 구현. layout snapshot only — process state 보존 비범위 (ADR-0013 D9 + ADR-0014 D5).

**Dispatch prompt**:
```
gtmux Sprint 7 의 S7-PERSISTENCE-MINIMAL — Canvas Layout 영속화 구현.

입력:
- docs/adr/0006-persistence-storage.md
- docs/ssot/canvas-layout-schema.md (Panel + Group schema)
- codebase/backend/crates/http-api/src/lib.rs (현 GET/PUT /api/layout, 현재 메모리 상태만)
- codebase/backend/bin/gtmux-cli/src/state_files.rs (layout_path_for(session))

작업:
1. layout 영속화: GET /api/layout 시 ${XDG_STATE_HOME}/gtmux/<session>.layout.json 읽어 응답 (없으면 빈 layout)
2. PUT /api/layout 시 atomic write — tmp 파일 + fsync + rename + dir fsync
3. ETag = 파일 hash (현 메모리 ETag 와 정합)
4. sidecar quarantine: 손상된 JSON 발견 시 <session>.layout.json.bak 으로 격리 + 빈 layout 으로 fresh start
5. teardown 의 remove_state_files 흐름이 이미 .layout.json 정리하므로 추가 작업 없음
6. tests: atomic-write race, sidecar quarantine, ETag mismatch (412), boot-after-corruption

DoD: cargo test --workspace --tests PASS, http-api 단위 + integration tests 추가, demo 에서 재기동 후 layout 보존 확인
```

### 4.2 S7-FE-SHUTDOWN + S7-FE-CLOSE-GUARD + S7-BE-AUTOMOUNT (2-3일)

CONTEXT.md §"Pane lifecycle invariant" 정합:

- **S7-FE-SHUTDOWN**: 우상단 헤더 메뉴 + Session shutdown action + confirm modal + API `kill-session` 호출 → graceful exit 6
- **S7-FE-CLOSE-GUARD**: panel close 버튼 비활성화 (살아 있는 child=1 일 때) + tooltip
- **S7-BE-AUTOMOUNT**: backend 가 PTY spawn 시 자동 layout PUT + LAYOUT_CHANGED broadcast (0022 L-3 정신)

### 4.3 S7-DEMO-STAB (3-7일, closeout)

`docs/sketch.md` §15 2단계 demo 를 *새 backend 위에서* 재구동. 본 단계가 *PTY backend 의 실 부하 테스트* 첫 회 — 새 부류 결함 발생 가능성:
- $TERM 변종, alt-screen edge, OSC sequence — Sprint 5 의 17건 결함 부류 회귀 검증
- ws-server 의 multiplex pane_output broadcast 의 backpressure 측정 (ADR-0013 O2)
- portable-pty 의 production 운영 시 함정 (sketch §15.3.D wisdom 흡수)

---

## 5. 신규 어휘 (CONTEXT.md 와 정합)

본 세션에서 도입된 / 정착된 어휘 — 이후 코드 / 문서 일관성:

| 어휘 | 정의 |
|---|---|
| **PtyBackend** | gtmux Server 가 직접 관리하는 PTY pair + child process 의 단일 supervisor. `dashmap::DashMap<PaneId, Arc<PaneHandle>>` 기반. Cheap to clone (`Arc<Inner>` 내부). |
| **BackendCommand** | CTRL envelope 의 payload — `serde(tag = "type", rename_all = "kebab-case")` enum. 변형 3개: `NewPane`, `KillPane`, `ResizePane`. 추가는 enum extend + ADR-0013 D10 amend. |
| **BackendNotify** | NOTIFY_MIRROR envelope 의 payload — 변형 4개: `PaneSpawned { id, request_id }`, `PaneDied { id, code, signal }`, `LayoutChanged`, `ServerReady`. |
| **PaneHandle** | 개별 Pane 의 owned 자원 (out_tx broadcast, in_tx mpsc Option, master Arc<Mutex>, child Arc<Mutex>, ring VecDeque<u8>, stall_count, reader/writer join handles). `Drop` 이 SIGTERM → 200ms → SIGKILL + reap. |
| **Hub** | ws-server 가 노출하는 PtyBackend wrapper. multiplexed `(PaneId, Bytes)` broadcast + layout_events broadcast + backend access. `publish_layout_changed(etag)` 시그니처 보존 — http-api 코드 변경 0. |
| **state_files** | bin/gtmux-cli 의 module — pidfile, token, layout, config 파일 경로 + atomic write + 4단계 teardown (ADR-0014 D7) 흡수. FFI unsafe 격리. |
| **server-ready** | NOTIFY_MIRROR kind — Server bootstrap 완료 신호. 구 `daemon-started` 어휘 폐기 (ADR-0014 D1 정합). |

---

## 6. Reading list (우선순위 순)

### 6.1 Tier 1 — 새 세션이 반드시 읽어야

1. `CLAUDE.md` — 프로젝트 메타, EN
2. `CONTEXT.md` — 도메인 어휘 (2026-05-14 amend ×2 반영, tmux 어휘 폐기)
3. **본 문서** (`0027-session-resume-handoff.md`) — 단일 진실 진입점
4. `docs/adr/0013-pty-direct-no-tmux.md` — canonical 아키텍처 (2026-05-14 amend ×3)
5. `docs/adr/0014-process-supervisor.md` — supervisor 라이프사이클 (2026-05-14 amend ×1)

### 6.2 Tier 2 — 작업 시작 전 권장

6. `codebase/backend/crates/pty-backend/src/lib.rs` — 신규 backend 정본 (1098 LOC, 잘 주석화)
7. `codebase/backend/crates/ws-server/src/{lib.rs,hub.rs,cmd_router.rs}` — 새 wire dispatch
8. `codebase/backend/bin/gtmux-cli/src/{main.rs,state_files.rs}` — Server 부트스트랩 + 상태 파일 관리
9. `codebase/frontend/src/lib/ws/dispatcher.svelte.ts` — frontend NOTIFY 매핑 정본
10. `docs/reports/0026-stage-b-carry-forward.md` — Stage B 의 design intent (현재는 historical reference, 본 작업 완료)

### 6.3 Tier 3 — 살아있는 ADR (참조용)

11. `docs/adr/0002-transport-websocket.md` — envelope 의미 (2026-05-14 amend ×2)
12. `docs/adr/0003-security-defaults.md` — 보안 (2026-05-14 amend ×2)
13. `docs/adr/0007-server-session-port-binding.md` — 1:1:1 모델
14. `docs/adr/0008-single-pane-window-and-group.md` — Group + single-pane-per-process
15. `docs/adr/0010-group-data-model.md` — Group 데이터
16. `docs/adr/0004-terminal-rendering.md` / `0005-canvas-library.md` / `0006-persistence-storage.md` / `0011-backend-stack-rust.md` / `0012-frontend-stack-svelte.md`

### 6.4 Tier 4 — Deprecated / Historical

- `docs/adr/0001-tmux-integration-control-mode.md` — Deprecated, *읽지 말 것*
- `docs/adr/0009-tmux-daemon-isolation.md` — Deprecated, *읽지 말 것*
- `docs/reports/0021-session-handoff.md` / `0024-stage-a-closeout-handoff.md` / `0025-session-resume-handoff.md` — historical
- `docs/reports/0026-stage-b-carry-forward.md` — Stage B step 2 entry doc, 본 commit (`0a7cd65`) 으로 *실현 완료* — 이제 historical

---

## 7. 현재 코드 상태 + Git 정보

### 7.1 git 브랜치

```
* main           58e2f46 refactor(frontend): S7-WS-PAYLOAD-SIMPLIFY — Stage-B wire 정렬
  poc/pty-direct c637c39 poc(pty-direct): throwaway PTY-direct experiment, no tmux
```

stage-b-swap branch 는 main 으로 fast-forward merge 후 삭제됨.

### 7.2 main 의 코드 상태 (PTY direct backend, Stage B 완료)

- backend `cargo test --workspace --tests`: **150 PASS / 0 FAIL / 0 ignored**
  - 이전 baseline 212 (tmux backend) → lifecycle 19 + mux-router 28 + ws-server 의 tmux 관련 ~15 tests 폐기 = **150**
  - 신규 추가: pty-backend 단위 11 + integration 9 + ws-server notify mapping + state_files unit + cmd_router unit
- backend `cargo clippy --workspace --all-targets -- -D warnings`: clean
- backend `cargo fmt --all -- --check`: clean
- frontend `svelte-check`: **0 errors / 0 warnings**
- frontend `vite build`: PASS, main bundle **10.93 KB gzip** (baseline 11.41 → -0.48 KB)
- smoke gate (`01_engine_connect.sh`): **아직 새 backend 위에서 안 돌림** — S7-DEMO-STAB 첫 단계

### 7.3 LOC 합계 (Sprint 7 의 본 세션 누적)

```
3006 insertions
5047 deletions
─────
-2041 LOC net
```

세부 (코드 + 문서):
- 신규: `crates/pty-backend/` (1098 LOC + 384 tests) + `bin/gtmux-cli/src/state_files.rs` (409 LOC) + 0026 + 0027 docs
- 폐기: `crates/lifecycle/` (2467 LOC) + `crates/mux-router/` (897 LOC)
- 변경: ws-server, gtmux-cli, frontend dispatcher / mux store

### 7.4 progress vs 0025/0026 표

| Sprint 7 task (0025 §4.1) | 본 세션 상태 |
|---|---|
| S7-PTY-BACKEND | ✅ Stage A + Stage B 완료 |
| S7-WS-PAYLOAD-SIMPLIFY + S7-MIGRATE | ✅ 완료 |
| S7-PERSISTENCE-MINIMAL | ⏸️ 다음 세션 |
| S7-FE-SHUTDOWN + S7-FE-CLOSE-GUARD + S7-BE-AUTOMOUNT | ⏸️ 다음 세션 |
| S7-DEMO-STAB | ⏸️ 다음 세션 (closeout) |

### 7.5 untracked (본 세션 무관, baseline 부터 존재)

- `.agents/skills/{debug-issue,explore-codebase,refactor-safely,review-changes}/`
- `.codex/`, `AGENTS.md`, `docs/demo-guide.md`, `docs/demo/`, `experiments/` (POC, `poc/pty-direct` branch 에서만 tracked)
- `.claude/scheduled_tasks.lock`

---

## 8. Risk register / Carry-forward

### 8.1 Risk (Sprint 7 잔여 작업 안에 발생 가능)

| 카테고리 | risk | 완화 |
|---|---|---|
| Persistence | atomic write 실패 시 sidecar quarantine 동작 검증이 미흡 | S7-PERSISTENCE-MINIMAL 의 test plan 에 명시적 corruption-recovery 시나리오 |
| Demo | PTY backend 위에서 *처음으로 실제 부하* — 새 결함 부류 발견 가능성 ↑ | S7-DEMO-STAB 의 정량 게이트 + Sprint 5 17건 결함 분류 (0020) 회귀 매트릭스 |
| Multiplex | Hub 의 multiplexed pane_output broadcast 가 50 pane × 5 burst 에서 backpressure 측정 미실시 | S7-DEMO-STAB 안에 multi-pane scale gate |
| FE | session 어휘 폐기 후 `muxStore.session?.name` 의존하던 코드가 더 있을 가능성 (사이드바 / banner / 다른 컴포넌트의 hidden 의존) | grep + svelte-check 통과 확인 (본 세션 완료) — 미발견 의존은 demo 단계에서 노출 |
| API | CTRL `new-pane` 성공 ack (success encode) 가 미배선 — 현재는 frontend 가 `pane-spawned` NOTIFY 의 first-sight 매칭으로 우회 | S7-FE-SHUTDOWN 흐름에서 정식 wire 추가 검토 |

### 8.2 Carry-forward (Sprint 8+)

- ADR-0006 의 sqlite vs JSON 결정 (Sprint 7 의 S7-PERSISTENCE-MINIMAL 은 JSON 으로 우선)
- TLS / cloud 모드 helper (sketch §15 4단계)
- Playwright 시각 검증 자동화 (smoke step 8 N/A 해소 prereq)
- 외부 CLI client (ADR-0013 D8 비범위 — 사용자 검증 후 P1+ 재방문)
- xterm 키맵 갭 (Shift / Option) — `SECURE_XTERM_OPTIONS` amend (Sprint 7 backlog 의 S7-XTERM-KEYMAP)
- CI 캐시 도입 (sccache, cargo-cache)
- GitHub push (`iiamaii/gtmux`) — credential 사용자 영역
- ADR-0014 D8 의 lock 파일 (`${XDG_RUNTIME_DIR}/gtmux/*.pid`) — 현 state_files.rs 가 `runtime_dir_for_gtmux` 를 YAGNI 로 dropped, 도입 시점에 재추가
- `connectionStore.slowPaneIds` + `markSlow` / `clearSlow` — 본 세션에서 호출처가 사라졌으나 store API 자체는 잔존 (frontend cleanup task 미루기 — dead surface 이지만 무해)

### 8.3 Open questions

- **O1**: backend 가 CTRL response 의 `result.pane_id` 를 echo 하지 않는 것이 영구 디자인인가, S7-FE-SHUTDOWN 시점에 wire 추가할 일인가? (현재 frontend 는 NOTIFY 의 first-sight 로 우회 — 동작은 OK)
- **O2**: SIGTERM → SIGKILL grace period 200ms (PANE_KILL_GRACE) 가 50 pane 동시 spawn/kill 시 적정한가? — S7-DEMO-STAB 실측 후 조정
- **O3**: `BackendNotify::ServerReady` 의 emit 시점이 미정 — 현재 backend 가 emit 안 함, frontend 는 debug log only. boot UX 신호로 정식 wire 필요한가?
- **O4**: frontend bundle 의 `connectionStore.slowPaneIds` 가 dead surface — 정리 task 별도 분리 vs 다음 작업과 함께 흡수?

---

## 9. 사용자 작업 룰 (메모리 정합)

새 세션 agent 가 사용자와 협업할 때:

1. **기술 디테일 결정** → brief + 진행 (confirm 묻지 않음). 예: 변수명, 파일 위치, 빌드 명령.
2. **도메인 / UX / 정책** → 옵션 비교 + 확인. 예: 새 API 명칭, UI 액션 배치, 보안 정책.
3. **Docs = KO, code = EN**. README/CLAUDE.md/repo-meta = EN.
4. **ADR-before-code** — 새 결정은 ADR 먼저, 그 다음 코드.
5. **Grilling 패턴** — 사용자가 "옵션 비교 + 추천" 을 가장 선호. AskUserQuestion 으로 한 번에 한 결정.
6. **TaskCreate / TaskUpdate** — Sprint 7 잔여 작업 3건 + S7-MIGRATE (S7-WS-PAYLOAD-SIMPLIFY 에 흡수됨) 의 task #2~#5 가 task tracker 에 이미 존재. 본 세션 진입 시 TaskList 로 확인.

---

## 10. 다음 세션 첫 메시지 가이드 (사용자 입력 별 행동)

| 사용자 메시지 | 행동 |
|---|---|
| "S7-PERSISTENCE-MINIMAL 진행" | §4.1 의 dispatch prompt 를 backend-architect agent 에 전달 OR 직접 작업. http-api 가 1 차 영향 surface. |
| "S7-FE-SHUTDOWN 진행" / "Session shutdown UI" | frontend 의 우상단 헤더 메뉴 + confirm modal + CTRL `kill-session` 발사. backend 측 `BackendCommand::KillSession` variant 추가 필요 (ADR-0013 D10 amend). |
| "S7-FE-CLOSE-GUARD 진행" | PanelNode 의 close 버튼 비활성화 로직. 살아 있는 pane 카운트 = `muxStore.panes` 의 `!dead` 필터링. |
| "S7-BE-AUTOMOUNT 진행" | backend 의 PtyBackend::spawn 시점에 자동 layout PUT — 그러나 layout 은 http-api 의 책임이고 backend 가 frontend 의 layout schema 를 모르므로, *frontend* 가 `pane-spawned` NOTIFY 수신 시 layout 에 cascade 자동 추가하는 흐름이 깔끔. ADR / 어휘 결정 필요. |
| "S7-DEMO-STAB 진행" / "demo 안정화" | sketch §15 2단계 demo 절차를 새 backend 위에서 실행 + Sprint 5 17건 결함 회귀 + 새 부류 결함 검증. smoke gate `01_engine_connect.sh` 첫 회 실행. |
| "현 상태 점검" | 본 §7 의 빌드/테스트 명령 재실행. |
| "어떤 결정이 있었어?" / "Stage B 가 뭐였어?" | 본 §1 + §2 + 0026 §1 인용 |
| "ADR 목록" | 본 §3 |
| "신규 어휘" | 본 §5 |

---

## 11. 메모리 정합

다음 메모리 파일이 본 세션의 architectural pivot 후속 변화를 반영:
- `~/.claude/projects/-Users-ws-Desktop-projects-gtmux/memory/project_gtmux.md` — gtmux 의 backend = PTY direct (2026-05-14 amend ×2 — Stage A + Stage B 완료), tmux 어휘 영구 폐기.

다른 메모리 (feedback_language_and_adr, feedback_grill_style) 는 *backend 무관* 이라 변경 없음.

---

## 변경 이력

- 2026-05-14: 초안 — Sprint 7 의 S7-PTY-BACKEND + S7-WS-PAYLOAD-SIMPLIFY 완료 후, S7-PERSISTENCE-MINIMAL / FE-SHUTDOWN / FE-CLOSE-GUARD / BE-AUTOMOUNT / DEMO-STAB 진입 직전 시점의 self-contained handoff.
