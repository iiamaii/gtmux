# 세션 핸드오프 — 2026-05-14 (Sprint 5 완료 + 데모 안정화 직후, sketch §15 2단계 핵심 박스 통과)

본 문서는 `0018-session-handoff.md` 의 후속이다. Sprint 5 의 3 가닥 (A / B / B-sidebar) 이 main 에 합류했고, *실제 브라우저에서 [New Panel] 클릭 → tmux pane 생성 → xterm 렌더 → 드래그 영속* 까지 시연 가능한 상태이다. **CLAUDE.md + CONTEXT.md + `0019-progress-status.md` + `0020-debug-classification.md` + 본 문서 5개**만 읽어도 작업이 이어진다.

## TL;DR

- **현 단계**: sketch §15 2단계 *create + paint + drag* 박스 통과. backend `cargo test --workspace --tests` **192 / 0 / 5**, frontend `svelte-check` **224 files / 0 errors / 0 warnings**, main bundle **11.41 KB gzip** (cap 200 KB의 5.7 %).
- **다음 = Sprint 6** — select / close UX, group reparent, mux mirror visualisation, 그리고 §3 의 ADR amend 6건.
- **차단성 갭 0건**. 운영 갭은 sidebar UX 토글, drag-commit 디바운스, 마지막 window 닫힘 시 recovery 등 P1+ 분류.
- **본 세션 디버깅 17건의 root-cause 분류는 `0020-debug-classification.md` 정본**이며 본 문서는 *진행 산출* 에 집중한다.

## 1. 우선 읽을 문서 (순서대로)

1. `CLAUDE.md` — 프로젝트 메타 (EN)
2. `CONTEXT.md` — 도메인 어휘 + 5대 불변식 (KO)
3. **`docs/reports/0019-progress-status.md`** — Sprint 5 직후 데이터-가능 능력 매트릭스 + 데모 5-step 절차
4. **`docs/reports/0020-debug-classification.md`** — 데모 안정화 17건 분류 (Logic 7 / 오구현 7 / 미구현 3). Sprint 6 의 ADR amend 항목 6건이 §4.1 표에 정렬돼 있다.
5. **본 문서 (`0021-session-handoff.md`)** — Sprint 6 진입점 + dispatch prompts
6. `docs/reports/0018-session-handoff.md` — Sprint 5 직전 상태 (재참조 불필요 시 skip 가능)
7. `docs/sketch.md` §12·§15 — 우선순위·단계 정의
8. `docs/adr/0001~0012` — 12개 Accepted (§3 에 amend 권고 6건)
9. `docs/ssot/{wire-protocol,security-defaults,canvas-layout-schema}.md` — 3개 SSoT
10. `codebase/smoke/01_engine_connect.sh` — 9-step 회귀 게이트 (8 PASS / 1 N/A)

## 2. 진행 매트릭스

| Phase | 상태 | 산출 commit |
|---|---|---|
| Grill + ADR ×9 + SSoT ×3 + Bootstrap | ✅ | …`894ab69` |
| Sprint 0 (AUTH + CFG + MUX) | ✅ | `4a010a0` |
| Sprint 1 (LIFE-1 + CLI-1) | ✅ | `951750b` |
| Sprint 2 (HTTP + WS + Teardown) | ✅ | `da19003` |
| Sprint 3 (FE-1+2+3) | ✅ | `80675d6` |
| Sprint 4-A (ADR-0004/0005/0006 publish) | ✅ | `c0710b1` |
| Sprint 4-B (Hub broadcaster + reverse routing) | ✅ | `de73005` |
| Sprint 4-C (Frontend FE-1~5) | ✅ | `04b4ec0` |
| Sprint 4-D (LIFE-3 + SPA + smoke 9/9) | ✅ | `b4900ad` |
| Sprint 4 closeout (0017 + 0018) | ✅ | `6c81b26` |
| **Sprint 5-A** (cors default + ResizeWindow variant) | ✅ | `50bad9c` |
| **Sprint 5-B** (FE NewPanel + ctrl-registry + mux mirror) | ✅ | `50bad9c` (5-A와 합본) |
| **Sprint 5-B 추가** (Sidebar V0 read-only) | ✅ | `7a5c873` |
| Sprint 5 hotfix #1 (`--port` + cors loopback alias) | ✅ | `da5c221` |
| Sprint 5 closeout 보고 (0019) | ✅ | `929433e` |
| **데모 안정화 #1** (session catch-up) | ✅ | `11778fb` |
| **데모 안정화 #2** (bootstrap landing) | ✅ | `dea7c13` |
| **데모 안정화 #3** (`-d` 제거) | ✅ | `3a36f0f` |
| **데모 안정화 #4** (mutex deadlock 해소) | ✅ | `4a5faf0` |
| **데모 안정화 #5** (`#{pane_id}` parse error 우회) | ✅ | `8cbadee` |
| **데모 안정화 #6** (LAYOUT_CHANGED broadcast) | ✅ | `c2af73a` |
| **데모 안정화 #7** (PUT 204 + drag commit + late buffer) | ✅ | `9268bc6` |
| **데모 안정화 #8** (etag race + paneId contract) | ✅ | `21a1fe2` |
| **데모 안정화 #9** (xterm.css import) | ✅ | `bcb37a8` |
| Debug classification report (0020) | ✅ | `633f743` |
| sketch §15 **2단계 핵심 박스 (create+paint+drag) 통과** | ✅ | `bcb37a8` 시점 |
| sketch §15 2단계 마무리 (select/close/group/mirror UX) | ⏳ **다음** | — |
| sketch §15 3단계 (영속화 정착, ADR-0006 implement) | ⏳ 선택 | — |

## 3. Sprint 5 산출 상세 — *본 문서의 중점*

### 3.1 Sprint 5-A — backend quick wins (`50bad9c` 의 일부)

| 파일 | 변경 |
|---|---|
| `crates/config/src/lib.rs` | `effective_cors_origins()` 헬퍼. 빈 셋이면 bind 호스트로 합성 |
| `crates/http-api/src/lib.rs` | `origin_check_middleware` 가 위 헬퍼 사용 |
| `crates/mux-router/src/lib.rs` | `Command::ResizeWindow { window_id, cols, rows }` 정식 variant |
| `crates/ws-server/src/cmd_router.rs` | `build_pane_resize_request` 가 `ResizeWindow` 직접 발급 (legacy keyword override 제거) |
| `crates/lifecycle/src/lib.rs` | `serialise_command` 가 `ResizeWindow → "resize-window -t @<id> -x <cols> -y <rows>"` 직렬화 |

테스트: 184 → 188 PASS (+4 — effective_cors_origins ×2, ResizeWindow serialisation, pane_resize routing).

### 3.2 Sprint 5-B — frontend New-Panel UX + mux mirror (`50bad9c` 의 일부)

| 파일 | 역할 |
|---|---|
| `lib/stores/mux.svelte.ts` (신규) | windows / panes / session 미러 store. 8 메서드 (addWindow / renameWindow / closeWindow / setSession / setLayout / setPaneMode / killPane / addPane) |
| `lib/ws/ctrl-registry.ts` (신규) | CTRL request/response 상관기. UUID-v4 id + per-request timeout |
| `lib/canvas/NewPanelButton.svelte` (신규) | 캔버스 좌상단 툴바 버튼. 클릭 → `encodeCtrl(new-window)` → `Promise.race(ctrl-registry, waitForNewPane)` → `PUT /api/layout` (If-Match + 412 자동 rebase) |
| `lib/canvas/Canvas.svelte` | 좌상단 툴바 overlay (pointer-events 분리), drag-commit (5단계 데모 안정화 후 store + PUT 모두 호출) |
| `lib/http/layout.ts` | `putLayoutAppendPanel()`, `putLayoutCommitCurrent()` 헬퍼 |
| `lib/ws/decode.ts` + `lib/types/envelope.ts` | `encodeCtrl` / `decodeCtrl` + `CtrlDecoded` re-export |
| `lib/ws/dispatcher.svelte.ts` | NOTIFY_MIRROR 7 kind → muxStore routing, CTRL response → ctrl-registry, 첫 PANE_OUT → `mux.addPane` |

svelte-check 221 → 224 files 0/0. main 번들 7.21 → 9.71 KB gzip.

### 3.3 Sprint 5-B 추가 — Sidebar V0 (`7a5c873`)

| 파일 | 역할 |
|---|---|
| `lib/sidebar/Sidebar.svelte` | 좌측 280 px 고정 칼럼. groupsStore + panelsStore 트리 (depth indent), unicode 가시성/잠금 아이콘, dead pane 취소선, 단일 선택 (`ephemeralStore.m.clear()+add()`). component-local `SvelteSet<string>` 으로 펼침 상태. 디자인 시스템 없음 (inline CSS variables). |

main 번들 9.71 → 10.94 KB gzip.

### 3.4 데모 안정화 — Sprint 5 외 추가 9건

`0020-debug-classification.md` §3 참조 (원인 + 해결 접근 정본). 본 문서는 *commit 단위 매핑*만 표시:

| commit | 한 줄 요약 | 분류 |
|---|---|---|
| `da5c221` | `--port` figment provider 합류 + cors loopback alias | I 오구현 + L |
| `11778fb` | Hub.last_session catch-up + 새 subscriber replay | L |
| `dea7c13` | `/auth/bootstrap` → HTML landing + sessionStorage inline-script mirror | L |
| `3a36f0f` | `TmuxDaemon::spawn` argv 에서 `-d` 제거 → control-mode client long-lived | I 오구현 |
| `4a5faf0` | `TmuxDaemon` 내부 stdin/stdout 독립 mutex → deadlock 해소 | I 오구현 |
| `8cbadee` | NewPanelButton args 에서 `-P -F #{pane_id}` 제거 (tmux `#` quirk 우회) | L |
| `c2af73a` | `Hub.publish_layout_changed` + `layout_put_handler` broadcast | I 미구현 |
| `9268bc6` | PUT 204 + Canvas drag commit + PANE_OUT late-mount buffer | I 오구현 + I 미구현 + L |
| `21a1fe2` | `handleLayoutChanged` setEtag 제거 + PanelNode 의 `%` strip | I 오구현 ×2 |
| `bcb37a8` | `@xterm/xterm/css/xterm.css` import | I 미구현 |

## 4. Backend / Frontend 누적 상태

### 4.1 Backend (Rust) crates

| Crate | LOC | 테스트 | 본 세션 변경 |
|---|---:|---|---|
| `auth` | 478 | 10 PASS | — |
| `config` | ~750 | 12 PASS | `effective_cors_origins` loopback alias, `load_with_overrides` |
| `mux-router` | ~890 | 28 PASS | `Command::ResizeWindow` variant |
| `lifecycle` | ~1820 | 29 + 5 ignored | spawn `-d` 제거, internal mutex split, command/event loop trace |
| `http-api` | ~870 | 18 PASS | bootstrap landing HTML, 204 응답, `AppState::with_hub`, `Hub.publish_layout_changed` 트리거 |
| `ws-server` | ~1400 | 72 PASS | `Hub.last_session` + `layout_events`, catch-up replay 2 종, CTRL/PANE_OUT trace |
| `bin/gtmux-cli` | ~1380 | 16 PASS | `load_with_overrides` 호출, `AppState::with_hub` wiring |

**합계**: 184 → **192** PASS (+8) / 5 ignored. clippy `-D warnings` clean, fmt clean.

### 4.2 Frontend (Svelte 5 + Vite 7)

| 모듈 | 본 세션 변경 |
|---|---|
| `lib/ws/{dispatcher,decode,client,ctrl-registry}.ts` + `lib/types/envelope.ts` | encodeCtrl/decodeCtrl, ctrl-registry, NOTIFY_MIRROR 7 kind → muxStore routing, PANE_OUT late buffer, handleLayoutChanged etag 비-mutation |
| `lib/canvas/{Canvas,PanelNode,XtermHost,NewPanelButton}.svelte` + `PanelPlaceholder` | NewPanelButton 신규, toolbar overlay, drag-commit, PanelNode 의 `%` strip, XtermHost mount/post-fit trace, **xterm.css import** |
| `lib/stores/{panels,mux,layout}.svelte.ts` | `panelsStore.movePanel`, `muxStore` 신규 |
| `lib/http/layout.ts` | `putLayoutAppendPanel`, `putLayoutCommitCurrent`, etag 단일 책임화 |
| `lib/sidebar/Sidebar.svelte` | read-only 트리 + 단일 선택 |
| `routes/+page.svelte` | (구조 그대로) |

**합계**: 221 → **224** files, svelte-check 0/0. `npm run build` main bundle **11.41 KB gzip** + index.css **25.45 KB**(xterm.css 포함).

## 5. Code graph (code-review-graph MCP)

- **2026-05-14 빌드** (최신 commit `633f743` 기준): 632 nodes / 5233 edges / 175 flows / 10 communities. 본 세션 동안 graph auto-update 17회.
- 탐색 시 `mcp__code-review-graph__*` 우선 사용. 본 세션 안티패턴 학습:
  - graph 의 *"untested"* 보고는 parser annotation 한계라 실측 `cargo test` PASS 와 어긋날 수 있음. 실측이 정본.
  - `detect_changes_tool` 가 boot 시 17개 변경 리포트 → 0020 분류표 작성의 출발점이었음.

## 6. Sprint 6 — 다음 단계 task 분해

본 절은 **`0020-debug-classification.md` §4 의 권고 + `0019-progress-status.md` §6 잔여 갭** 을 통합 정렬한 것이다. ADR amend 6건은 *S6-A 박스* 로, 코드 보강은 *S6-B/C/D* 로 묶었다.

### 6.1 S6-A. ADR amend 6건 (코드 변경 0 — 정공 documentation 단계)

| Task | ADR / SSoT | 추가/수정 절 |
|---|---|---|
| **S6-ADR-0001** | `docs/adr/0001-tmux-integration-control-mode.md` §D11 | argv 안전 quoting (`#` / 공백 / 따옴표) |
| **S6-ADR-0002** | `docs/adr/0002-transport-websocket.md` §D8 | static-state cache + frontend late-mount buffer |
| **S6-ADR-0003** | `docs/adr/0003-security-defaults.md` §D3 | cors_origins 디폴트 합성 + loopback alias |
| **S6-ADR-0003b** | `docs/adr/0003-security-defaults.md` §D6 | bootstrap landing inline-script sessionStorage 미러 |
| **S6-ADR-0004** | `docs/adr/0004-terminal-rendering.md` | Required imports — `@xterm/xterm/css/xterm.css` |
| **S6-ADR-0009** | `docs/adr/0009-tmux-daemon-isolation.md` §D5 | 마지막 window 종료 시 server graceful recovery |

DoD: ADR 본문에 절 추가 + "Result: this clause was added retroactively after debug session 2026-05-14, see report 0020" 정도의 cross-link.

### 6.2 S6-B. Frontend UX (sketch §15 2단계 마무리)

| Task | 작업 | DoD |
|---|---|---|
| **S6-FE-SELECT** | tmux `select-window` 발사 (사이드바 클릭 또는 캔버스 dblclick) + NOTIFY_MIRROR `session-changed` 반영 | manual probe |
| **S6-FE-CLOSE** | 패널 컨텍스트 메뉴/사이드바 우클릭 → close. confirm dialog | svelte-check / bundle |
| **S6-FE-GROUP-REPARENT** | 사이드바 drag → group drop. ADR-0010 G-hybrid drag-delta 액션 (PUT `/api/layout` `panels[].parent_id`) | bundle, manual probe |
| **S6-FE-MUX-VIS** | muxStore.windows / panes 를 사이드바에 *Available* 섹션으로 노출 | bundle |
| **S6-FE-SIDEBAR-TOGGLE** | 가시성(👁) / 잠금(🔒) 아이콘 click handler (현재 read-only) | bundle |

### 6.3 S6-C. 영속화 정착 (sketch §15 3단계 entry)

| Task | 작업 |
|---|---|
| **S6-LAYOUT-STORAGE** | `0018-handoff` §8 Agent #4 prompt 그대로. ADR-0006 implement. |

### 6.4 S6-D. Backend 보강

| Task | 작업 |
|---|---|
| **S6-BE-CTRL-ACK** | 0x01 CTRL response 정식 wire. `new-window` 응답에 `result.pane_id`. frontend `ctrl-registry` fallback 자동 deprecated. *S6-ARGV-QUOTE 와 같이* `-F #{pane_id}` 복귀 가능. |
| **S6-BE-CLOSE** | `Command::KillWindow { window_id }` allowlist 정식화 + serialise_command. `Command::KillPane` 도 같이. |
| **S6-ARGV-QUOTE** | `lifecycle::serialise_command` 가 argv 토큰의 `#`/공백/따옴표 escape. S6-ADR-0001 amend 와 동반. |
| **S6-LIFE-AUTOSPAWN** | control-mode pipe broken 감지 시 `TmuxDaemon::spawn` 재실행 + `daemon-restarted` NOTIFY_MIRROR. S6-ADR-0009 와 동반. |
| **S6-WS-WINDOW-CATCHUP** | Hub 에 windows mirror cache 추가 → 새 subscriber 에게 `window-add` replay. S6-ADR-0002 와 동반. |

### 6.5 S6-E. 정합 / 위생

| Task | 작업 |
|---|---|
| **S6-FE-ETAG-SYM** | `attemptAppend` 성공 시 `setEtag` 호출 (대칭성). |
| **S6-FE-DRAG-DEBOUNCE** | 연속 드래그 시 in-flight PUT 직렬화. |
| **S6-SMOKE-CONFORMANCE** | smoke 9-step 에 SSoT-conformance probe 추가 (예: PUT → 204 + ETag + WS 0x80 broadcast 모두 검증). |
| **S6-CI** | GitHub Actions smoke 9-step gate. |
| **S6-DOC-CLEANUP** | 본 세션 추가된 console.debug 5줄 + lifecycle `debug!` 5줄 retire 시점 결정 (polish 단계 후). |

## 7. Sprint 6 dispatch prompts (즉시 사용 가능)

### Agent #1 (technical-writer) — S6-A ADR amend 6건

```
gtmux Sprint 6-A 작업. 데모 안정화 후 발견된 logic 결함 6건을 관련 ADR 본문에 반영.

입력 (반드시 읽을 것):
- docs/reports/0020-debug-classification.md §2 (logic 7건 상세)
- docs/reports/0020-debug-classification.md §4.1 (amend 표)
- docs/adr/0001-tmux-integration-control-mode.md (§D11)
- docs/adr/0002-transport-websocket.md (§D8)
- docs/adr/0003-security-defaults.md (§D3, §D6)
- docs/adr/0004-terminal-rendering.md
- docs/adr/0009-tmux-daemon-isolation.md (§D5)

작업: 위 6개 ADR 각각에 0020 §2/§4.1 의 amend 방향 본문 절 추가. 각 절 끝에 "Result: clause added retroactively after debug session 2026-05-14, see docs/reports/0020-debug-classification.md" cross-link. KO docs.

DoD: 6개 ADR 모두 절 추가. amend 후에도 기존 결정 본문 보존 (덮어쓰기 금지). PM 에게 amend 내용 1-line summary × 6 보고.
```

### Agent #2 (backend-architect) — S6-D 백엔드 보강 (CTRL-ACK + CLOSE + ARGV-QUOTE)

```
gtmux Sprint 6-D 작업. backend CTRL response 정식 wire + KillWindow allowlist + argv quoting.

입력:
- docs/reports/0020-debug-classification.md §3.1.2 / §3.1.7 (오구현 사례)
- docs/adr/0008-single-pane-window-and-group.md (allowlist)
- docs/ssot/wire-protocol.md §2.4 (CTRL response)
- codebase/backend/crates/ws-server/src/lib.rs::handle_client_envelope (Ctrl arm)
- codebase/backend/crates/mux-router/src/lib.rs (Command enum)
- codebase/backend/crates/lifecycle/src/lib.rs::serialise_command

작업:
1. S6-BE-CTRL-ACK: ws-server 가 CTRL request 처리 후 `%begin/%end` 응답 파싱 → 결과를 `0x01 CTRL` response envelope 으로 발급 (`{id, ok: true, result: {pane_id: "%N"}}` 또는 `{id, ok: false, code, error}`).
2. S6-BE-CLOSE: Command::KillWindow / KillPane variant 정식 + allowlist 추가 + serialise_command + 단위 테스트 ×2.
3. S6-ARGV-QUOTE: serialise_command 의 argv 토큰 단계에서 `#`, 공백, 작은따옴표가 있으면 `'...'` quoting (단일따옴표 안의 `'` 는 `'\''` escape). NewPanelButton args 에 `-P -F #{pane_id}` 복귀 후 정상 동작 확인.
4. 본 세션 추가된 debug! 5줄은 보존 (polish 후 일괄 retire).

DoD: cargo test PASS, clippy/fmt clean, smoke 9-step PASS. PM 에게 변경 파일 + 새 테스트 이름 + smoke 결과 보고.
```

### Agent #3 (frontend-architect) — S6-B select / close / mirror visualisation

```
gtmux Sprint 6-B 작업. sketch §15 2단계 마무리 — select / close / mirror 시각화.

입력:
- docs/sketch.md §10.2 + §12 P0
- docs/reports/0020-debug-classification.md §4.3
- docs/adr/0008 + 0010 + 0011
- codebase/frontend/src/lib/{ws/dispatcher.svelte.ts, stores/mux.svelte.ts, sidebar/Sidebar.svelte, canvas/*}

작업:
1. S6-FE-SELECT: 사이드바 row 또는 캔버스 패널 dblclick → CTRL `select-window` 발사.
2. S6-FE-CLOSE: 패널 헤더에 close × 버튼 + confirm dialog. CTRL `kill-window` 발사 (S6-BE-CLOSE 와 동반).
3. S6-FE-MUX-VIS: 사이드바에 "Available windows" 섹션 — muxStore.windows 중 panelsStore 에 mount 안 된 것 표시. 클릭 → New Panel 동등 동작 (CTRL kill-window 대신 PUT layout append).

DoD: npm run check / build PASS. bundle gzip < 200 KB. manual probe checklist.
```

## 8. 안티패턴 / 함정 누적 — 정본은 `0020`

본 절은 0018 §9 + 본 세션 신규 누적의 1-line index. 상세는 `0020-debug-classification.md` §3 참조.

| 함정 | 정본 위치 |
|---|---|
| figment chain 검증 순서 (CLI override 는 figment provider 로) | 0020 §3.1.1 |
| `tmux -C ... -d` 가 control-mode client 즉시 종료 | 0020 §3.1.2 |
| 단일 Mutex 가 `read_line.await` 로 writer starve | 0020 §3.1.3 |
| PUT 응답 status 가 SSoT 명시값과 불일치하면 silent fail | 0020 §3.1.4 |
| Pull-through-notify 의 setEtag 위치 | 0020 §3.1.5 |
| PanelNode→XtermHost paneId contract | 0020 §3.1.6 |
| xterm v6 의 `xterm.css` import 의무 | 0020 §3.2.4 |
| `broadcast::Sender` 는 late subscriber 에게 과거 미배달 → static-state cache | 0020 §2.2 |
| HttpOnly cookie 와 SPA JS 호환성 | 0020 §2.3 |
| tmux control-mode 의 `#` line-comment quirk | 0020 §2.4 |
| 마지막 window 종료 시 daemon broken pipe | 0020 §2.6 |
| Frontend mount-vs-emit race (late buffer) | 0020 §2.5 |
| loopback alias 운영 현실 | 0020 §2.1 |

또한 본 세션 누적된 *디버깅 도구* 도 정본:
- `console.debug` 5줄 (dispatcher PANE_OUT/registerPaneOut, XtermHost mount/post-fit) — 데모 안정화 #9 의 발견 트레일이 그대로 보존됨.
- `tracing::debug!` 5줄 (ws-server Ctrl arm, lifecycle run_command/event_loop) — backend wire 측 결함 시 즉시 가시화.
- 외부 python WS probe (handshake → catch-up frame parsing → CTRL 발사 → 응답 카운트) — 7회 재사용. 향후 smoke 확장 시 통합 권고.

## 9. 잔여 carry-forward (Sprint 6 이후)

- ADR-0006 implement (디스크 영속) — 1-session 데모에는 무관, 서버 재시작 시 layout 휘발.
- TLS / cloud 모드 helper — sketch §15 4단계 진입 시.
- Playwright 시각 검증 자동화 — smoke step 8 N/A 해소 prereq.
- `Arc<Mutex<TmuxDaemon>>` → `tokio::io::split` (현재는 내부 mutex split — 충분히 동작 중). 성능 측정 후 결정.
- CI 캐시 도입 (sccache, cargo-cache).
- GitHub push `iiamaii/gtmux` — credential 사용자 영역.

## 10. 환경·도구 (변경 없음)

`0018-handoff` §11 그대로. 핵심만 재요약:

- **Memory files** (`/Users/ws/.claude/projects/-Users-ws-Desktop-projects-gtmux/memory/`):
  - `MEMORY.md` (index)
  - `project_gtmux.md`, `feedback_language_and_adr.md`, `feedback_grill_style.md`
- **MCP**: `code-review-graph` 활성. Grep 대신 우선 사용.
- **Subagents 가용**: backend-architect, frontend-architect, system-architect, devops-architect, security-engineer, quality-engineer, technical-writer, deep-research, refactoring-expert 등.
- **사용자 피드백 룰**:
  1. 기술 디테일 결정 → brief + 진행 (confirm 묻지 않음)
  2. 도메인/UX/정책 → 옵션 비교 + 확인
  3. KO docs / EN code
  4. ADR-before-code

## 11. 다음 세션 첫 메시지 가이드

사용자가 "Sprint 6 진행" 또는 "다음 단계 진행"이라 하면 본 문서 §6~§7 을 그대로 따른다. 순서 권장:

1. **S6-A (ADR amend ×6)** → 코드 변경 0, 정공 documentation. Agent #1 (technical-writer).
2. **S6-D (CTRL-ACK + CLOSE + ARGV-QUOTE)** → S6-ADR-0001 amend 와 동반. Agent #2 (backend-architect).
3. **S6-B (SELECT + CLOSE + MIRROR-VIS)** → S6-D 완료 후 (CLOSE 가 backend 의존). Agent #3 (frontend-architect).
4. **S6-C (LAYOUT-STORAGE)** → 선택. `0018-handoff` §8 Agent #4 prompt 재사용.
5. **S6-E (정합 / 위생)** → Sprint closeout 묶음.

사용자가 "회귀 검증" 또는 "smoke 재실행"이라 하면:
```bash
cd /Users/ws/Desktop/projects/gtmux
SMOKE_GATE_RUNTIME=0 ./codebase/smoke/01_engine_connect.sh
```
8 PASS / 1 N/A / 0 GATE / 0 FAIL 유지 확인. Port 9999 conflict 시 leftover daemon 정리:
```bash
./codebase/backend/target/debug/gtmux teardown --session smoke --force
```

사용자가 "데모 시연"이라 하면 `0019-progress-status.md` §5 의 5-step 절차를 따른다.

사용자가 "본 세션 결함을 더 깊이 보겠다" 또는 "어떤 결함이 있었는지"라 하면 `0020-debug-classification.md` 가 정본.

## 변경 이력

- 2026-05-14: 초안 (Sprint 5 완료 + 데모 안정화 9건 직후, sketch §15 2단계 핵심 박스 통과 직후 PM 핸드오프).
