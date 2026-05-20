# Plan 0003 — S7 Lifecycle UI 구현 계획

- 일자: 2026-05-15
- 작성: agent (S7-PERSISTENCE-MINIMAL closeout 직후)
- 진입점: `docs/reports/0028-s7-persistence-minimal-closeout.md` §5.1
- 범위: Sprint 7 잔여 **S7-FE-SHUTDOWN + S7-FE-CLOSE-GUARD + S7-FE-AUTOMOUNT** (S7-BE-AUTOMOUNT 재명명) + backend `KillSession` variant 추가
- 후속: S7-DEMO-STAB (본 plan 완료 후 1회 demo 안정화)
- 폐기 조건: 본 plan 의 모든 task 완료 시 `docs/reports/0030-s7-lifecycle-ui-closeout.md` 로 closeout 후 historical

---

## 0. 결정 요약 (UX + 정책)

| 결정 | 선택 | 근거 |
|---|---|---|
| Shutdown UX 진입 | **Toolbar 우상단 dropdown (3-dot)** | 확장 여지 (rotate-token / status 등 추가 가능), destructive 액션 한 단계 멀리 둠. Toolbar.svelte 가 현재 빈 placeholder 라 자연 합류. |
| Confirm modal 정보량 | **활성 pane 수 + session 이름 + layout 보존 안내** | destructive 안전성 + 사용자에게 *지금 어디서 무엇이 죽는지* 가시화. `CONTEXT.md` §"Pane lifecycle invariant" 정신. |
| Panel close action | **Panel 헤더 우측 X 버튼** | 표준 UI 컨벤션 (browser tab close, VSCode tab close). 마지막 1개일 때 disabled + tooltip. |
| Auto-mount 책임 | **Frontend 가 `pane-spawned` NOTIFY 수신 시 자동 cascade PUT** | 0027 §10 권장. backend 가 frontend layout schema 를 모르는 채로 둘 수 있음 (coupling↓). task 이름 `S7-BE-AUTOMOUNT` → **`S7-FE-AUTOMOUNT`** 로 재명명. |
| KillSession 흐름 | **backend ack 후 SIGTERM self** | 기존 `wait_for_shutdown` 의 graceful_shutdown 경로가 그대로 흡수 — 코드 면적 최소. ADR-0014 D7 정합. |
| ADR 발행 | **ADR-0013 D10 amend ×1 (KillSession variant 추가) + ADR-0015 신규 (Auto-mount 책임)** | ADR-before-code. |

---

## 1. 신규 ADR / Amend

### 1.1 ADR-0013 D10 amend (BackendCommand 확장)

```rust
pub enum BackendCommand {
    NewPane { ... },
    KillPane { id: PaneId },
    ResizePane { id: PaneId, rows: u16, cols: u16 },
    // 신규 (S7-FE-SHUTDOWN):
    KillSession,
}
```

- Wire shape: `{ "type": "kill-session" }` — args 없음.
- cmd_router 의 allowlist (`is_allowed_ctrl_cmd` + `ALLOWLISTED_CTRL_CMDS`) 에 `"kill-session"` 추가.
- backend.dispatch 처리: (a) NOTIFY_MIRROR `server-ready` 의 dual인 `server-shutting-down` 추가는 보류 (현재 `pane-died` broadcast 가 자연스럽게 모든 child 정리를 알림) (b) 모든 child SIGTERM (drop PtyBackend 자체가 그 동작) (c) self SIGTERM 으로 graceful_shutdown 트리거.

### 1.2 ADR-0015 — Pane auto-mount 책임 경계 (신규)

- 결정: backend 는 PTY spawn 만 책임. layout 영속화는 frontend 가 `pane-spawned` NOTIFY 수신 시 자동 cascade PUT 으로 처리. 외부 spawn 통로가 추가될 경우에도 동일 dispatcher 흐름이 흡수.
- 근거: `docs/sketch.md` §4 두 도메인 분리, ADR-0002 D9 (durable HTTP / ephemeral WS) 정신, ADR-0013 D8 (외부 attach 비범위) — backend 가 layout schema 를 알 필요 없음.
- 대안 R1 (backend POST /api/spawn): coupling↑, 거절.

---

## 2. 구현 순서 (4 PR 또는 1 PR 분할)

### Phase A — Backend KillSession variant (낮은 risk, 독립)

**Files**:
- `codebase/backend/crates/pty-backend/src/lib.rs` — `BackendCommand::KillSession` enum 항목 + `dispatch` 처리 (no-op at backend level, ws-server 가 SIGTERM 발사)
- `codebase/backend/crates/ws-server/src/cmd_router.rs` — `"kill-session"` arm + allowlist
- `codebase/backend/crates/ws-server/src/lib.rs` — cmd dispatch 후 `libc::raise(SIGTERM)` 또는 `tokio::process::Command` 로 self-signal
- `codebase/backend/crates/pty-backend/src/lib.rs` 의 BackendCommand serde 테스트 갱신

**Tests**:
- pty-backend: `BackendCommand::KillSession` serde round-trip
- ws-server: cmd_router `kill-session` allowlist + dispatch outcome
- ws-server: integration — `kill-session` 발사 후 axum graceful_shutdown 진행 (난이도↑, smoke 로 대체 가능)

**ADR**: ADR-0013 D10 amend 동반.

### Phase B — Frontend auto-mount (S7-FE-AUTOMOUNT, 0027 §10 권장)

**Files**:
- `codebase/frontend/src/lib/ws/dispatcher.svelte.ts` — `handleNotifyMirror` 의 `case 'pane-spawned'` 안에서 layout 에 없는 pane 발견 시 cascade PUT 트리거. NewPanelButton 의 `putLayoutAppendPanel` 을 공통화.
- `codebase/frontend/src/lib/http/layout.ts` — `appendPanelIfMissing(paneId, viewport)` 헬퍼 신규 (`putLayoutAppendPanel` 의 idempotent wrapper)
- `codebase/frontend/src/lib/canvas/NewPanelButton.svelte` — 기존 PUT 경로 유지 (request_id 매칭이 빠르므로 race↓), 보조 자동 mount 가 idempotent 보장

**Tests**:
- vitest 가 backend 에 없으므로 svelte-check + 수동 시연. (현재 frontend test infra 가 미약 — 본 plan 의 별도 task 아님)

**ADR**: ADR-0015 신규 동반.

### Phase C — S7-FE-CLOSE-GUARD (Panel header X 버튼)

**Files**:
- `codebase/frontend/src/lib/canvas/PanelNode.svelte` — `.panel-badges` 우측에 `<button class="panel-close">×</button>` 추가
- `codebase/frontend/src/lib/stores/mux.svelte.ts` — `liveCount` derived 추가 (`[...panes.values()].filter(p => !p.dead).length`)
- 새 핸들러 `closePanel(panelId, paneId)` — (a) CTRL `kill-pane` 발사 (b) layout PUT 으로 panel entry 제거
- disabled 조건: `muxStore.liveCount === 1`. tooltip: `"Use Session shutdown for the last pane"`

**CONTEXT.md 보강 여부**: §"Pane lifecycle invariant 의 UI 측 mirror" 가 이미 close-button 정책을 잠궜으므로 추가 amend 불필요.

### Phase D — S7-FE-SHUTDOWN (Toolbar dropdown + Confirm modal + CTRL kill-session)

**Files**:
- `codebase/frontend/src/lib/toolbar/Toolbar.svelte` — 현재 8 LOC placeholder. 우상단 3-dot dropdown 추가. session 명 표시 + 메뉴.
- `codebase/frontend/src/lib/toolbar/SessionMenu.svelte` 신규 — dropdown body, "Session shutdown" 항목 (rotate-token 등은 추후).
- `codebase/frontend/src/lib/toolbar/ShutdownModal.svelte` 신규 — confirm modal (활성 pane 수 + session 이름 + layout 보존 안내 + [Cancel] [Shutdown] 버튼).
- `codebase/frontend/src/lib/ws/ctrl-registry.ts` — `kill-session` cmd 발사 helper (현재 `sendCtrl` 의 일반화 가능성 검토).
- `codebase/frontend/src/lib/banner/ReconnectBanner.svelte` — backend 가 SIGTERM 자가 발사 후 WS 가 1000/1001 close 로 떨어지면 banner 가 "Session ended" 류 메시지 표시. 분기 추가.

**Tests**: svelte-check + 수동 시연.

**Session 이름 surface 필요**: 현재 frontend 는 session 이름을 모름. 두 옵션:
- (i) GET /api/layout 응답에 `session` 필드 추가 (backend 변경)
- (ii) `/auth/bootstrap` 이 sessionStorage 에 `gtmux_session` 도 같이 주입 (현재 token 만 주입)
- (iii) modal 에서는 "this session" 류 generic 문구 사용

→ **(ii)** 권장 — backend 변경 면적↓, sessionStorage 만 추가. 별도 short 결정.

---

## 3. 파일 변경 매트릭스 (sneak preview, 13 files)

| Phase | 파일 | 신규 / 수정 | LOC 예상 |
|---|---|---|---|
| A | `crates/pty-backend/src/lib.rs` | 수정 | +20 |
| A | `crates/ws-server/src/cmd_router.rs` | 수정 | +30 |
| A | `crates/ws-server/src/lib.rs` | 수정 | +15 (SIGTERM self) |
| A | `docs/adr/0013-pty-direct-no-tmux.md` | amend | +30 |
| B | `frontend/src/lib/ws/dispatcher.svelte.ts` | 수정 | +30 |
| B | `frontend/src/lib/http/layout.ts` | 수정 | +20 |
| B | `frontend/src/lib/canvas/NewPanelButton.svelte` | 수정 (refactor) | +5 |
| B | `docs/adr/0015-pane-auto-mount.md` | 신규 | ~200 |
| C | `frontend/src/lib/canvas/PanelNode.svelte` | 수정 | +60 |
| C | `frontend/src/lib/stores/mux.svelte.ts` | 수정 | +10 |
| D | `frontend/src/lib/toolbar/Toolbar.svelte` | 수정 (rewrite) | +60 |
| D | `frontend/src/lib/toolbar/SessionMenu.svelte` | 신규 | ~80 |
| D | `frontend/src/lib/toolbar/ShutdownModal.svelte` | 신규 | ~120 |
| D | `frontend/src/lib/ws/ctrl-registry.ts` | 수정 | +20 |
| D | `frontend/src/lib/banner/ReconnectBanner.svelte` | 수정 | +20 |
| D | (옵션) `crates/http-api/src/lib.rs` bootstrap landing | 수정 | +5 (session inject) |

LOC 예상 합계: backend ~95 + frontend ~395 + docs ~230 ≈ **+720 LOC**.

---

## 4. 위험 / Open questions

| 카테고리 | risk | 완화 |
|---|---|---|
| KillSession self-SIGTERM | macOS 에서 SIGTERM 자가 발사 후 axum graceful_shutdown 의 in-flight WS close 가 정상 마무리되는지 | Phase A 단위 테스트 + 수동 시연 |
| FE-AUTOMOUNT idempotency | `pane-spawned` NOTIFY 가 NewPanelButton 의 PUT 보다 빨리 도착하면 중복 PUT 가능 | `appendPanelIfMissing` 의 `if layout.has(pane_id) return` 가드 + 412 race auto-rebase |
| Close-guard count race | live count = 1 인 순간 동시 close 클릭 (multi-tab) | UX: disabled 가 일관성↑, 1 명의 single-user 가정 — 정량 무시 |
| Session name surface | bootstrap landing 변경 → 기존 sessionStorage 의 token 만 의존하는 SPA 가 깨지는지 | sessionStorage 추가 키만 진입 — 기존 키 무영향 |
| ReconnectBanner 의 "Session ended" 분기 | close code 1000 (normal) 과 사용자 shutdown 액션 구분 | shutdown 액션 시 frontend 가 `connectionStore.lastAction = 'shutdown'` 마킹 후 modal 표시 |

### 4.1 Open

- **O1**: Phase A 의 self-SIGTERM 외 다른 graceful 종료 시그널 (예: explicit `axum::Server::shutdown` 호출) 비교 — 현재는 SIGTERM 단일.
- **O2**: ShutdownModal 의 "활성 pane 수" 가 muxStore 의 liveCount 와 panelsStore.panels.size 중 어느 쪽을 권위로 — 둘 다 동기되지만 빈 layout + 살아 있는 pane 시나리오 (FE-AUTOMOUNT race) 가 있을 수 있음. **muxStore 권위** 가 backend 진실에 가까우므로 그쪽 채택.
- **O3**: rotate-token / status 등 추가 메뉴 항목의 priority — 본 plan 범위 밖. SessionMenu 의 확장 슬롯만 준비.

---

## 5. 검증 게이트 (각 Phase 완료 시)

| Phase | 게이트 |
|---|---|
| A | `cargo test --workspace --tests` ≥ 168 PASS (+4 신규), clippy / fmt clean |
| B | svelte-check 0/0, `gtmux start --session demo` + browser 에서 (i) NewPanelButton 클릭 후 panel 표시 (ii) muxStore 에만 있고 layout 에 없는 pane 가 발견되면 cascade 자동 mount 확인 |
| C | svelte-check 0/0, browser 에서 (i) 다중 패널 시 X 버튼 정상 동작 (ii) live count=1 시 X 버튼 disabled + tooltip |
| D | svelte-check 0/0, browser 에서 (i) Toolbar dropdown 진입 (ii) Confirm modal 정보 표시 (iii) Shutdown 클릭 → backend exit 6 + browser 의 "Session ended" 배너 |

전체 완료 시: `bash codebase/smoke/01_engine_connect.sh` rewrite (현재 pre-Sprint-0 stale) — 본 plan 의 추가 task 로 보류.

---

## 6. 다음 세션 진입 안내

| 사용자 메시지 | 행동 |
|---|---|
| "Phase A 진행" | ADR-0013 D10 amend 먼저 + BackendCommand::KillSession variant 추가 + 테스트 |
| "Phase B 진행" | ADR-0015 신규 발행 먼저 + dispatcher hook + http/layout helper |
| "Phase C 진행" | PanelNode 의 X 버튼 + muxStore.liveCount + 핸들러 |
| "Phase D 진행" | Toolbar dropdown + SessionMenu + ShutdownModal + ReconnectBanner 분기 |
| "전체 4 Phase 한 번에 진행" | A → B → C → D 순서로 직렬 진행. ADR 2건 (0013 amend + 0015 신규) 가 코드보다 먼저 |
| "Phase 순서 바꾸자" | C 가 A 에 의존하지 않으므로 (close 는 기존 kill-pane 사용) 독립. D 는 A 와 강결합. B 는 독립. C → B → A → D 도 가능 |

---

## 변경 이력

- 2026-05-15: 초안 — S7-PERSISTENCE-MINIMAL 완료 후, S7-FE-SHUTDOWN/CLOSE-GUARD/AUTOMOUNT 진입 직전 시점. UX 4결정 + 기술 2결정 + ADR 2건 (amend 1 + 신규 1) 명시.
