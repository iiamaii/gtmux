# 세션 핸드오프 — 2026-05-14 (Sprint 0~4 완료 + sketch §15 1단계 통과 + 2단계 진입 직전)

본 문서는 `0016-session-handoff.md`의 후속이다. P0 구현이 backend (Sprint 0~2) + frontend (Sprint 3) + 정합/SPA 정착 (Sprint 4) 모두 마무리되어 **sketch §15 1단계가 자동화 smoke 9/9 PASS로 통과**한 직후의 상태를 캡처한다. **CLAUDE.md + CONTEXT.md + `0017-progress-status.md` + 본 문서 4개**만 읽어도 작업이 이어진다.

## TL;DR

- **현 단계**: sketch §15 1단계 **정식 통과** (`b4900ad`). backend 184 + 5 ignored / frontend 221 svelte-check 0/0 / 12 WS frame round-trip byte-equal / smoke 9/9.
- **다음 = Sprint 5 — cors 디폴트 합성 + `Command::ResizeWindow` 정식 변형 + "New Panel" UX + mux mirror store + (선택) ADR-0006 implement**.
- **현재 실패하는 invariant 없음**. 차단성 갭 0건. 데모 운영 갭 G1~G6 6건(0017 §3.2).
- **GitHub push** (`iiamaii/gtmux`) credential 사용자 영역, 본 세션 무관.

## 1. 우선 읽을 문서 (순서대로)

1. `CLAUDE.md` — 프로젝트 메타 (EN)
2. `CONTEXT.md` — 도메인 어휘 + 5대 불변식 (KO)
3. **`docs/reports/0017-progress-status.md`** — 현재 진행 스냅샷 + 데모 시연 절차 + G1~G6 갭 표 + 메트릭 (본 핸드오프의 직접 짝)
4. **본 문서 (`0018-session-handoff.md`)** — Sprint 5 진입점 + dispatch prompts
5. `docs/reports/0016-session-handoff.md` — Sprint 4 직전 상태 + 환경 도구 메모 (재참조 불필요 시 skip 가능)
6. `docs/sketch.md` §12·§15 — 우선순위 + 단계 정의 (§15 1단계 통과 표시 commit `b4900ad`)
7. `docs/adr/0001~0012` — 12개 Accepted
8. `docs/ssot/{wire-protocol,security-defaults,canvas-layout-schema}.md` — 3개 SSoT
9. `codebase/smoke/01_engine_connect.sh` — 9-step 정본 (회귀 검증 entry)

## 2. 진행 매트릭스

| Phase | 상태 | 산출 commit |
|---|---|---|
| Grill D1~D23 + ADR ×9 + SSoT ×3 + Code skeleton | ✅ | `894ab69`까지 |
| Sprint 0 (AUTH + CFG + MUX) | ✅ | `4a010a0` |
| Sprint 1 (LIFE-1 + CLI-1) | ✅ | `951750b` |
| Sprint 2 (HTTP + WS + Teardown) | ✅ | `da19003` |
| Sprint 3 (FE-1+2+3) | ✅ | `80675d6` |
| **Sprint 4-A** (ADR-0004/0005/0006) | ✅ | `c0710b1` |
| **Sprint 4-B** (Hub broadcaster + reverse routing) | ✅ | `de73005` |
| **Sprint 4-C** (Frontend FE-1~5) | ✅ | `04b4ec0` |
| **Sprint 4-D LIFE-3** (pidfile + gtmux stop) | ✅ | `e5dd63f` |
| **Sprint 4-D SMOKE + SPA** (9/9 PASS) | ✅ | `b4900ad` |
| sketch §15 **1단계 정식 통과** | ✅ | `b4900ad` |
| sketch §15 **2단계 진입** (basic UI workspace) | ⏳ **다음** | — |
| GitHub push iiamaii/gtmux | ⏳ blocked (credential, 사용자 영역) | — |

## 3. Commit history (main, 누적 21개)

```
b4900ad Sprint 4-D smoke: bundled SPA serving + 9-step harness now 0 GATE
04b4ec0 Sprint 4-C: Frontend WS bidirectional wiring (FE-1 ~ FE-5)
e5dd63f Sprint 4-D LIFE-3: server pidfile + gtmux stop graceful shutdown
de73005 Sprint 4-B: mux→WS Hub broadcaster + reverse-direction routing
c0710b1 Sprint 4-A: Publish ADR-0004 / ADR-0005 / ADR-0006
972578d docs: archive 0015 progress snapshot + rename handoff to 0016
0dc6c94 Session handoff 0015
80675d6 Sprint 3: FE-1 + FE-2 + FE-3
da19003 Sprint 2: HTTP + WS + Teardown
951750b Sprint 1: P0-LIFE-1 + P0-CLI-1
4a010a0 Sprint 0: AUTH + CFG + MUX
894ab69 Session handoff doc + code-review-graph build
2e152ec C5 Batch C coherence
4e7a0e5 Batch C4 smoke test + bootstrap-exchange
3af3abe Batch C1+C2+C3 code bootstrap skeletons
ba90988 plan 0002 §3 Batch C definition
e35fad7 ADR 9개 Proposed → Accepted (α) + A2 utoipa unification
c0007ad A4 coherence: fix B1/B2/B3 + G12/G13/C1
05e29aa Batch A2 + A3 + B4 + B5
4ef7572 Batch A1 + B1 + B2 + B3
270e32c A0.7 2nd-pass coherence: fix G7/G8
2aff743 Grill phase 2
89a13b5 Initial commit
```

## 4. Backend (Rust) — 누적 구현 상태

| Crate | LOC | 테스트 | 핵심 |
|---|---|---|---|
| `auth` | 478 | 10 PASS | `issue/save/load/verify/rotate_token`, ring 0.17 CSPRNG, atomic 0600 |
| `config` | 685 | 14 PASS | `Config{server,runtime,security,cloud,frontend_dist}`, figment, `derive_mode` |
| `mux-router` | 885 | 28 PASS | `Command` enum 11 variants (ADR-0008 allowlist), `Event` enum 14, `parse_line`, `decode_output_payload` |
| `lifecycle` | ~1760 | 28 + 5 ignored | `TmuxDaemon::{spawn,attach,read_line,write_line,shutdown}` + `teardown` 5단계 + **pidfile_path_for / write_pidfile / check_pidfile_liveness / stop_server**(LIFE-3) + **run_event_loop / run_command_loop**(Hub wiring) |
| `http-api` | ~785 | 18 PASS | `router/router_with_static/router_with_state_and_spa`, ETag SHA256-128, `/auth/bootstrap`, Origin+Host+Bearer 미들웨어 + **ServeDir+ServeFile SPA fallback**(S4-D SMOKE) |
| `ws-server` | ~1330 (+ hub.rs ring.rs varint.rs payload.rs cmd_router.rs) | 70 PASS | `Envelope` codec 12 frame, `Hub<Event>` broadcast 256-deep, `RingBuffer` 128 KiB, catch-up replay, allowlist gate, 300ms pause/resume debounce |
| `bin/gtmux-cli` | ~1380 | 16 PASS | `start/stop/teardown/rotate-token/status`, `--force`, pidfile gate, build_app(config, token, hub, cmd_tx, frontend_dist) |
| `bin/gen-openapi` | small | — | codegen 그대로 |

**합계**: 184 unit + 5 ignored PASS. clippy `-D warnings` clean, fmt clean.

## 5. Frontend (Svelte 5 + Vite 7) — 누적 구현 상태

| 모듈 | 핵심 파일 |
|---|---|
| WS pipeline | `lib/ws/{decode.ts, client.ts, dispatcher.svelte.ts}` + `lib/types/envelope.ts` (12 frame helpers, varint LEB128, encodePaneInput/Resize/Pause/Resume, decode → frame-type 분기) |
| Canvas | `lib/canvas/{Canvas, PanelNode, XtermHost, PanelPlaceholder}.svelte` (panOnDrag={[1,2]}, ZOOM_UNIT_EPS=0.02, term.onData→WS PANE_IN, ResizeObserver+150ms fit→100ms send debounce → PANE_RESIZE) |
| Banner | `lib/banner/ReconnectBanner.svelte` (close-code 분기, D21 c2 1s grace, c3 zombie 배지, slow-pane 배지) |
| Stores | `lib/stores/{connection,ephemeral,groups,layout,panels}.svelte.ts` (svelte 5 runes, MT-3 라이브 갱신, slowPaneIds 추가) |
| HTTP | `lib/http/layout.ts` (fetchLayoutAndHydrate, Authorization Bearer, If-None-Match 304) |
| Page bootstrap | `routes/+page.svelte` (createDispatcher + WsClientHolder context + setLayoutRefetchHandler + onDestroy stop) |

**합계**: 221 files, svelte-check 0/0. `npm run build` → 6 chunks, **166.48 KB gzip total** (main **7.21 KB** << 200 KB cap, R8 §F7 PASS).

## 6. Code graph (code-review-graph MCP)

- **2026-05-14 빌드**: 522 nodes / 4805 edges / 162 flows / 10 communities. Languages: bash, rust, typescript, svelte, javascript, css.
- 탐색 시 `mcp__code-review-graph__*` 우선 사용. 핵심:
  - `semantic_search_nodes_tool` — 함수·클래스 검색
  - `query_graph_tool` (callers_of/callees_of/imports_of/tests_for)
  - `get_impact_radius_tool` — 변경 blast radius
  - `detect_changes_tool` + `get_review_context_tool` — 리뷰 시
- *주의*: code graph의 "untested" 보고는 graph parser annotation 한계로 실측 `cargo test` PASS 와 다를 수 있음. 실측이 정본.

## 7. Sprint 5 — 다음 단계 task 분해

`0017-progress-status.md` §3.2의 G1~G6 + §6 권고를 정리. ADR 추가 *없이* 진행 가능한 항목이 대부분 (코드+SSoT 정합 작업).

### S5-A. Quick wins (코드 변경 ≤ 50 LOC, 각 1 PR 단위)

| Task | 작업 | 산출 |
|---|---|---|
| **S5-CFG-1** | `gtmux-config`에 `effective_cors_origins(&self) -> Vec<String>` 헬퍼 추가. `cors_origins`가 빈 셋이면 `server.bind + server.port`로 same-origin 자동 합성. host_allowlist도 동일 패턴. | http-api middleware가 이 헬퍼 사용. ADR-0003 D3 정합 노트 추가 (변경 없으면 ADR amend 불필요). |
| **S5-MUX-1** | `mux-router::Command`에 `ResizeWindow { window_id: u32, cols: u16, rows: u16 }` 정식 변형. | cmd_router::build_pane_resize_request + lifecycle::serialise_command 갱신. 기존 임시 park 코드 (Command::ListWindows + args[0] override) 제거. 1~2 tests. |
| **S5-MUX-2** | (선택) `Event::Continue` → SSoT §2.3에 `"slow-pane-resumed"` kind 추가. ws-server `event_to_envelope`에서 매핑 처리. | SSoT amend + payload encoder 1줄. |

### S5-B. Frontend UX (sketch §15 2단계 진입)

| Task | 작업 | 산출 |
|---|---|---|
| **S5-FE-NEW-PANEL** | `routes/+page.svelte` 또는 `Canvas.svelte`에 "New Panel" 액션 (툴바 버튼 or 우클릭 메뉴). 클릭 시 `mux_router::Command::NewWindow`(tmux) + PUT `/api/layout` (panel append) sequence. pane_id는 NOTIFY_MIRROR `kind: "window-add"` 도착 후 mapping. | sketch §12 P0 "create/close/select" + "canvas panel placement". |
| **S5-FE-MUX-MIRROR** | `lib/stores/mux.svelte.ts` 신설 — windows/panes/session metadata 미러 store. dispatcher의 NOTIFY_MIRROR 7개 kind (window-add/renamed/close, session-changed, layout-change, subscription-changed, pane-mode-changed)를 console.debug → store routing 으로 승격. | 외부 attach 변경 인지 → 사이드바 표시 prereq. |
| **S5-FE-SIDEBAR-V0** | Figma-식 layer panel skeleton — Group 트리 + Panel 리스트. read-only 표시. drag/drop reparent는 P2. | sketch §10.2 + ADR-0010 D5. |
| **S5-FE-PALETTE** | command palette (cmd-K) skeleton — 검색 + 명명된 액션 등록 슬롯. ADR-0008 allowlist에 한해 실 명령 발급. | sketch §10.2 "command palette". |

### S5-C. 영속화 정착 (sketch §15 3단계 prereq) — *선택, Sprint 5 후반 또는 별도 Sprint*

| Task | 작업 | 산출 |
|---|---|---|
| **P0-LAYOUT-STORAGE-1** | ADR-0006 implement. `lifecycle::layout_path_for(session)` + `layout::{load,save}` (atomic-write-file). http-api `layout_put_handler` 가 RwLock<LayoutSnapshot>에서 → 디스크 atomic write 추가. `LayoutSnapshot::load_or_empty(path)` 부팅 시 호출. | sketch §15 3단계 진입 prereq. |
| **P0-LAYOUT-STORAGE-2** | server start 시 stale lock 검출 (D8 advisory lock pid-file) + 손상 시 sidecar 격리 (ADR-0006 D10). | crash-safe. |

### S5-D. 정합/위생 (Sprint 5 closeout 묶음)

| Task | 작업 |
|---|---|
| **S5-WIRE-3** | tokio::io::split 도입 (TmuxDaemon stdio split — `Arc<Mutex<...>>` 제거). 성능 측정 후 결정. |
| **S5-DOC** | 0017-progress-status.md 의 §3.2 G1~G6를 본 sprint 결과로 갱신 또는 폐기. CONTEXT.md에 *변화 없으면 amend 불필요*. |
| **S5-CI** | GitHub Actions CI에 smoke 9-step PASS 게이트 추가 (현재 `make build` + `make codegen` 만 자동). |
| **S5-VISUAL** | Playwright skeleton (선택, sketch §15 5단계 진입 시 필요) — smoke step 8 N/A 해소 첫 발걸음. |

## 8. Sprint 5 dispatch prompts (즉시 사용 가능)

다음 세션이 "Sprint 5 시작" 또는 "2단계 진행"이라 하면 본 §의 프롬프트를 그대로 Agent 호출에 복붙.

### Agent #1 (backend-architect) — S5-CFG-1 + S5-MUX-1

```
gtmux Sprint 5-A 작업. 두 가지 quick win — same-origin cors 디폴트 합성 + Command::ResizeWindow 정식 변형.

입력 (반드시 읽을 것):
- docs/reports/0017-progress-status.md §3.2 G1 / G4
- docs/adr/0003-security-defaults.md D3 (Origin/Host)
- docs/adr/0008-single-pane-window-and-group.md (allowlist)
- codebase/backend/crates/config/src/lib.rs (effective_host_allowlist 패턴 참고)
- codebase/backend/crates/ws-server/src/cmd_router.rs::build_pane_resize_request
- codebase/backend/crates/lifecycle/src/lib.rs::serialise_command

작업 A — S5-CFG-1:
1. config crate에 `effective_cors_origins(&self) -> Vec<String>` 추가. `self.security.cors_origins`가 비면 `vec![format!("http://{}:{}", server.bind, server.port)]` (http only — TLS는 cloud 모드 별도). 이미 있으면 그대로.
2. http-api `origin_check_middleware` 가 `state.config.effective_cors_origins()` 호출하도록 변경.
3. 동일 패턴이 `effective_host_allowlist`에 이미 있는지 확인 — 있으면 동일 스타일로.
4. 단위 테스트 2개: empty → bind+port 합성 / 사용자 명시 → 명시값 그대로.

작업 B — S5-MUX-1:
1. mux-router::Command에 `ResizeWindow { window_id: u32, cols: u16, rows: u16 }` variant 추가.
2. cmd_router::build_pane_resize_request 를 ResizeWindow 직접 발급으로 갱신 (`Command::ListWindows` park 제거).
3. lifecycle::serialise_command 의 ResizeWindow → `resize-window -t @<id> -x <cols> -y <rows>` 직렬화.
4. 기존 keyword override 코드 제거.
5. unit test 2개: resize_window_serialisation + pane_resize_routes_to_resize_window.

DoD: cargo test --workspace --tests PASS. clippy/fmt clean. PM에게 변경 파일 리스트 + 테스트 이름 보고.
```

### Agent #2 (frontend-architect) — S5-FE-NEW-PANEL + S5-FE-MUX-MIRROR

```
gtmux Sprint 5-B 작업. sketch §15 2단계 진입의 첫 두 task — "New Panel" UX + mux mirror store.

입력 (반드시 읽을 것):
- docs/sketch.md §12 P0 (create/close/select + canvas panel placement) + §10.2
- docs/adr/0008 + 0010 + 0011 D10 + 0012 D8
- docs/ssot/wire-protocol.md §2.1 (0x01 CTRL JSON) + §2.3 (NOTIFY_MIRROR kind enum)
- docs/ssot/canvas-layout-schema.md (panels[] 정합)
- codebase/frontend/src/lib/{ws/decode.ts, ws/dispatcher.svelte.ts, stores/*.svelte.ts, canvas/Canvas.svelte}
- codebase/frontend/src/lib/http/layout.ts (fetchLayoutAndHydrate 재사용)

작업 A — S5-FE-NEW-PANEL:
1. Canvas.svelte 또는 +page.svelte에 툴바 1개 + "New Panel" 버튼.
2. 클릭 시: (a) WsClient.send(encodeCtrl({id, cmd: "new-window", args: ["-t", session, "-P", "-F", "#{pane_id}"]})) — pane_id 캡처는 응답에서.
   대안: 새 window 생성 후 dispatcher의 NOTIFY_MIRROR `window-add` 도착 후 거기서 pane_id mapping.
3. pane_id 확보 후 PUT /api/layout — panels[]에 `{id: pane_id, x: <viewport center>, y, w: 480, h: 320, z: <max+1>, visibility: true, locked: false, label: null, note: null}` append + If-Match 현 ETag.
4. 412 시 fetchLayoutAndHydrate 후 재시도 1회.

작업 B — S5-FE-MUX-MIRROR:
1. lib/stores/mux.svelte.ts 신설:
   - `windows: SvelteMap<string, { name: string, layout: string }>` (key = "@N")
   - `panes: SvelteMap<number, { window_id: string, dead: boolean, mode: string|null }>` (key = %N의 N)
   - `session: { id: string, name: string } | null`
   - 메서드: addWindow / renameWindow / closeWindow / setSession / setLayout / setPaneMode / killPane / addPane
2. dispatcher.svelte.ts NOTIFY_MIRROR 0x07 핸들러 갱신 — 현 console.debug 7개 kind를 mux store 메서드로 routing.
3. unit test 또는 type-check: store 메서드 시그니처 + envelope decode → store side-effect.

DoD: npm run check PASS. npm run build PASS. main bundle gzip < 200KB. PM에게 변경 파일 + 새 store API + 빌드 사이즈 보고.
```

### Agent #3 (frontend-architect) — S5-FE-SIDEBAR-V0

```
gtmux Sprint 5-B 추가 — read-only sidebar v0 (Figma-식 layer panel).

입력:
- docs/sketch.md §10.2 ("sidebar")
- docs/adr/0010 (Group 데이터 모델 G-hybrid)
- docs/ssot/canvas-layout-schema.md (groups + panels 트리)
- codebase/frontend/src/lib/stores/{groups, panels, mux}.svelte.ts

작업:
1. lib/sidebar/Sidebar.svelte 신설. left-edge 280px wide column.
2. 트리 렌더 — Group을 펼침 가능 노드, Panel을 leaf로. depth 들여쓰기.
3. 각 행에 label + 가시성/잠금 아이콘 (현 상태 표시만, 토글은 P1).
4. Panel 행 클릭 시 ephemeralStore.m.set([pane_id]) — selection 동기화.
5. 사이드바 폭은 고정 280px (P0). resize handle은 P1+.

DoD: npm run check PASS. 빌드 사이즈 main < 200 KB gzip 유지. read-only이므로 store mutation 없음. PM에게 사이드바 스크린샷 description (자동 캡처 불가시 manual probe checklist) + 변경 파일.
```

### Agent #4 (backend-architect) — P0-LAYOUT-STORAGE-1 *(선택, Sprint 5 후반 또는 별도)*

```
gtmux ADR-0006 implement — Canvas Layout 영속화 정착. sketch §15 3단계 prereq.

입력 (반드시 읽을 것):
- docs/adr/0006-persistence-storage.md (D1~D13 결정 정본 — 그대로 implement)
- docs/ssot/canvas-layout-schema.md (schema 정본)
- codebase/backend/crates/http-api/src/lib.rs::{LayoutSnapshot, layout_put_handler}
- codebase/backend/crates/lifecycle/src/lib.rs (atomic write 패턴 — write_pidfile 참고)
- docs/ssot/security-defaults.md (XDG_STATE_HOME 컨벤션)

작업:
1. lifecycle (또는 별도 새 crate `gtmux-layout-store`)에 layout_path_for(session) + load_or_empty(path) -> LayoutSnapshot + save(path, &snapshot) (atomic write).
2. http-api::AppState에 layout_path 추가. layout_put_handler 성공 시 save() 호출. AppState::new가 부팅 시 load_or_empty() 호출하도록 변경.
3. 손상된 JSON 검출 시 ADR-0006 D10 (sidecar quarantine) — `<path>.corrupted.<timestamp>` 로 이동 + 빈 layout으로 폴백.
4. 단위 테스트:
   - save_then_load_round_trip
   - load_or_empty_missing_returns_empty
   - load_or_empty_corrupted_quarantines_and_falls_back
   - atomic_write_survives_kill_mid_write (어려우면 ignored)
5. CLI Cmd::Start의 D20 sequence에 layout_path 주입 (AppState 생성 시점).

DoD: cargo test PASS. smoke 9-step 재실행 후 PASS 유지. ADR-0006 §결과 절에 "implement task complete: commit <hash>" 추가.
```

## 9. 안티패턴 / 함정 (Sprint 0~4 누적)

핸드오프 0016 §9 목록 모두 유효. 본 세션에서 추가:

- **`cors_origins` 빈 셋 디폴트는 same-origin fetch도 차단** — browser navigation은 Origin 헤더 부재로 통과하나, SPA의 `fetch('/api/layout')`은 차단됨. 데모 시 `GTMUX_SECURITY__CORS_ORIGINS` env 명시 또는 S5-CFG-1 적용 필요.
- **figment env prefix `GTMUX_` 충돌** — `frontend_dist`를 환경변수로 받으려면 Config 필드로 정식 등록해야 함 (Sprint 4-D 학습). 새 옵션 추가 시 *항상* Config 스키마 + DefaultsSeed 양쪽 갱신.
- **`tower-http` features `fs` 미포함** — workspace Cargo.toml 에 ServeDir/ServeFile 쓰려면 추가 필수 (Sprint 4-D 학습). 빌드 에러 메시지가 명확해서 빨리 잡히지만 인지.
- **`Cmd::Stop` 은 server-only graceful, daemon 보존** — ADR-0009 D5. `gtmux teardown`만이 daemon kill 책임. `gtmux teardown` 은 daemon-alive 시 `--force` 없이는 거절 (의도된 안전망).
- **smoke 스크립트 경로는 bootstrap-draft가 아닌 lifecycle::*_path_for 함수가 정본** — 회귀 시 두 쪽이 어긋날 수 있으므로 smoke 변경 시 lifecycle 함수도 함께 검토.
- **WS subprotocol 응답은 lowercase header** — axum/hyper가 RFC 7230 §3.2 case-insensitive를 따르고 wire는 lowercase 사용. 클라이언트 검증 시 case-insensitive 비교 (smoke step 6 python 패치).
- **mux-router::Command::ResizeWindow 부재** — cmd_router의 build_pane_resize_request가 임시 park 상태 (`Command::ListWindows` + args[0] override). S5-MUX-1 에서 정식화.
- **NOTIFY_MIRROR 7개 kind는 console.debug 만** — window-add/renamed/close, session-changed, layout-change, subscription-changed, pane-mode-changed. S5-FE-MUX-MIRROR가 store routing 추가.
- **code-graph "untested" 보고 ≠ 실 test 부재** — graph annotation 한계. 실측 cargo test/svelte-check가 정본.

## 10. 잔여 carry-forward (Sprint 5 이후)

- **G3 영속화 storage 정착** (ADR-0006 implement) — Sprint 5 후반 또는 별도 Sprint
- **G6 TLS / cloud 모드 helper** — sketch §15 4단계 진입 시
- **`Arc<Mutex<TmuxDaemon>>` → tokio::io::split** — 성능 측정 후 결정 (P1+)
- **시각 검증 자동화 (Playwright/Cypress)** — sketch §15 5단계 prereq
- **CI 캐시 도입** (sccache, cargo-cache, node_modules cache) — CI 시간 cap 시
- **GitHub push iiamaii/gtmux** — credential 사용자 영역
- **단위 테스트 graph annotation 정합** — code-graph parser tuning (낮은 우선순위)

## 11. 환경·도구 (변경 없음, 0016 §환경·도구 메모 참조)

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

사용자가 "Sprint 5 진행" 또는 "다음 단계 진행"이라 하면 본 문서 §7~§8을 그대로 따른다. 순서:

1. **S5-A (CFG-1 + MUX-1)** → 코드 변경 ≤ 50 LOC, 1 PR. Agent #1.
2. **S5-B (FE-NEW-PANEL + FE-MUX-MIRROR)** → sketch §15 2단계 첫 진입. Agent #2.
3. **S5-B 추가 (FE-SIDEBAR-V0)** → read-only. Agent #3.
4. **S5-C (LAYOUT-STORAGE-1)** → 선택. Agent #4.

S5-A는 단독 진행 후 S5-B 병렬 (서로 독립 — backend↔frontend). S5-C는 sprint 후반 또는 별도 sprint.

사용자가 "회귀 검증" 또는 "smoke 재실행"이라 하면:
```bash
SMOKE_GATE_RUNTIME=0 ./codebase/smoke/01_engine_connect.sh
```
9/9 PASS 유지 확인.

사용자가 "데모 시연"이라 하면 `docs/reports/0017-progress-status.md` §2.2 5-step 절차를 따른다.

## 변경 이력

- 2026-05-14: 초안 (Sprint 0~4 완료 + sketch §15 1단계 통과 직후, 2단계 진입 직전 PM 핸드오프)
