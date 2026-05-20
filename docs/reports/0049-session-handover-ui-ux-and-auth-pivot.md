# 0049 — Session handover (UI/UX 폴리시 + auth FE pivot 진행 중)

- 일자: 2026-05-16
- 작성자: agent (이 session 종료 직전)
- 종류: handover (cold-pickup 가능한 전체 context + 진행 상태)
- 후속 정본: ADR-0019 / ADR-0020 / plan-0007 / plan-0008 / plan-0009 / 본 handover

---

## 0. 한 줄 요약

`UI/UX batch 1~3 + ToolbarSubbar Figma/Excalidraw 전환 + ADR-0020 D13 (auth page FE-bundle pivot) 완료. /auth page 의 BE server-rendered handler 제거는 plan-0009 §2 의 BE Slice-A1 으로 분리되어 별 작업자에게 전달 — FE 측 (Slice-A2) 는 land 완료 (commit 22060e1). BE land 시점에 brower 의 /auth 가 자동으로 FE AuthPage 로 전환.`

---

## 1. 프로젝트 mental model (1 분 요약)

**gtmux** = tmux-backed web canvas workspace. *single-user* SPA. tmux 가 process lifecycle 의 진실, FE 가 *canvas layout 의 진실*.

### 1.1 어휘 (`CONTEXT.md` + ADR-0019 정합)

| 어휘 | 정의 |
|---|---|
| Server | gtmux process, 1 port owner, 1 workspace dir 바인딩 |
| Workspace | server 와 1:1, `<XDG_DATA_HOME>/gtmux/workspace/` 의 dir |
| Session | workspace 안 named file record (`<name>.json`). canvas layout + viewport |
| Webpage | brower 탭, 1 WS 연결, 0/1 session attach |
| Terminal | server-pool, multi-session 공유 가능 (mirror) |
| Canvas | 한 session 의 무한 작업 공간 |
| Canvas Item | canvas 위 시각 객체 (`type: terminal | rect | ellipse | line | text | note | file_path | document | image`) |
| Panel | type:"terminal" 인 Canvas Item |
| Group | session 안 item 의 묶음 (parent_id 트리, ADR-0010) |

### 1.2 핵심 invariant (절대 깨면 안 됨)

1. **두 state 분리**: tmux state (mirror only) ↔ web state (FE 진실)
2. **layout ≠ tmux layout**: 캔버스 free 배치 ≠ tmux split
3. **single-attach**: Webpage : Session = 1:1 (ADR-0019 D3 — multi-attach 거부)
4. **takeover 금지**: 활성 session 강제 takeover 없음 (ADR-0019 D4)
5. **control-mode integration**: tmux CLI shell-out 금지 (ADR-0021)

### 1.3 우선 (sketch §12, §15)

- P0: control-mode 연결 / session·window·pane 리스팅 / pane terminal render + input / canvas panel placement / 생성·종료·선택 / layout persistence
- P1: search, custom labels, focus/highlight, auto-reconnect, fit-to-view, keyboard shortcuts, destructive-action confirms
- 현재 단계 = Stage 6~7 (multi-session pivot 완료, UX 폴리시 + auth FE pivot)

---

## 2. 디렉토리 / 빌드

```
gtmux/
├─ CLAUDE.md                       # 프로젝트 instruction (한 번 읽으세요)
├─ docs/
│  ├─ sketch.md                    # 원본 design spec (KO)
│  ├─ adr/0001-0026.md             # 결정 기록
│  ├─ plans/0001-0009.md           # 구현 계획
│  ├─ reports/0001-0049.md         # 조사/handover/work-package
│  └─ ssot/                        # JSON Schema / canvas-layout 등
├─ codebase/
│  ├─ frontend/                    # Svelte 5 + @xyflow/svelte + xterm.js
│  │  ├─ src/main.ts
│  │  ├─ src/routes/+page.svelte   # AppPage (canvas)
│  │  ├─ src/routes/auth/+page.svelte  # AuthPage (D13 신규 단일 source)
│  │  ├─ src/lib/canvas/           # Canvas + *Node.svelte
│  │  ├─ src/lib/chrome/           # Titlebar / RightPanel / *Modal / ContextMenu
│  │  ├─ src/lib/sidebar/          # LeftPanel / LayerTreeView / TerminalListView
│  │  ├─ src/lib/toolbar/          # Toolbar2
│  │  ├─ src/lib/stores/           # sessionStore / reconnectGate / toolStore 등
│  │  ├─ src/lib/http/             # auth / sessions / terminals client
│  │  ├─ src/lib/ws/               # client / dispatcher / heartbeat / decode
│  │  └─ src/lib/xterm/            # xterm.js theme + binding
│  └─ backend/                     # Rust workspace (axum + tmux control mode)
│     ├─ crates/http-api/          # REST + WS endpoint + auth + settings
│     ├─ crates/ws-server/         # WS frame protocol (0x80-0x89)
│     └─ bin/gtmux-cli/            # entry point
```

### 2.1 빌드 / 검증 명령

```bash
# FE
cd codebase/frontend
npm run build                      # vite build → dist/
npx svelte-check --threshold error # type check
npm run dev                        # local dev server (vite, port 5173)

# BE
cd codebase/backend
cargo test -p http-api             # unit + handler tests
cargo build --release
cargo run --bin gtmux-cli -- start  # boot server (port 9527 default)
```

본 session 의 마지막 빌드: FE 509 modules / 1.46s, `dist/assets/index-DhUHqmOD.js`.

---

## 3. 본 session 의 commit 목록 (cold-pickup chronology)

| commit | 메시지 | 핵심 변경 |
|---|---|---|
| `edd642c` | foundation surfaces — multi-session chrome + auth + canvas v2 | 이전 session — multi-session 기반 |
| `7e4b955` | dual-source adapter 제거 + Layer list V2 (multi-select + drag reorder) | 이전 session — layer panel V2 |
| `9a3e2af` | handover reports + ADR amends (Stage 5~7 누적) | 이전 session — 정합 amend 묶음 |
| `7703b19` | 묶음 D — FE Tier 1 잔여 (cross-session filter + heartbeat + Phase 2 + mutation guard) | 이전 session — Phase 2 wire |
| `51f3a86` | Slice D-5 graceful shutdown + ADR-0014 D12 + WS 0x89 | BE 측 |
| `6b5fb2e` | ToolbarSubbar — Excalidraw-style floating panel | **본 session** — toolbar row → floating |
| `92a507b` | next-2 session-scoped PANE_OUT filter + ADR-0025 Accepted | BE 측 |
| `752f7c1` | UI/UX batch — inspector text-align, panel ghost, subbar 폐기 외 | **본 session** — 5종 (text-align Inspector / panel border-radius 1/2 / panel ghost preview / viewport radius / ActiveSessionDropdown single-button) |
| `21ea4ea` | retire legacy /api/layout v1 + LayoutStore | BE 측 (Stage 6 cleanup) |
| `682b584` | UI/UX batch 3 — multi-drag commit, selection persist, lasso, layout | **본 session** — 5종 (빈여백 clearM 검증 / loadLayout M.clear 제거 / shift-free lasso / Page pill 제거 → session button left / ContextMenu wire 검증) |
| `53f11cf` | ADR-0026 server identity (workspace-derived) | BE 측 |
| `da7663b` | 묶음 E — 0045 refresh reconnect loop P0 후속 | **본 session** 외 |
| `c84cae4` | ADR-0020 D13 + plan-0009 — /auth page FE-bundle pivot | **본 session** — 문서 |
| `7e52410` | BE next-session handoff 0047 + ADR-0019/0024 amend | BE 측 |
| `22060e1` | D13 Slice-A2 — AuthPage 를 /auth path 의 단일 source | **본 session** — FE pivot |

### 3.1 본 session 의 핵심 5 commit 흐름

```
ToolbarSubbar Figma/Excalidraw 전환 (6b5fb2e)
   ↓
UI/UX batch 1 (752f7c1) — text-align inspector / ghost / radius / 우측 session btn
   ↓
UI/UX batch 3 (682b584) — multi-drag fix / selection persist / lasso / layout
   ↓
ADR-0020 D13 + plan-0009 (c84cae4) — auth page pivot 결정 + work-package
   ↓
FE Slice-A2 (22060e1) — main.ts pickPage `/auth` 라우팅
```

---

## 4. 본 session 의 결정 / 변경 사항 정밀 정리

### 4.1 ToolbarSubbar Figma/Excalidraw 전환 (commit `6b5fb2e`)
- 기존: toolbar 아래 44px sibling row 항상 점유
- 변경: `position: fixed` floating panel. anchor = source tool button center-x + bottom-y + 8px gap. context 없으면 unmount → canvas 영역 100% 회복.
- **후속 폐기**: 본 컴포넌트는 commit `752f7c1` 에서 *완전 삭제*. text-align UI 는 ItemInfoView (Inspector) 로 이전. *진실*: ToolbarSubbar = 폐기.

### 4.2 UI/UX batch 1 (commit `752f7c1`, 5종)
1. **Text align Inspector (Figma segmented)** — `ItemInfoView.svelte` 의 text item payload section 에 6 button group (horizontal 3 + vertical 3). `applyTextAlign` / `applyTextVerticalAlign` 로직 단일화. `ToolbarSubbar.svelte` 삭제.
2. **Left/Right Panel border-radius 1/2** — `--radius-lg (8px)` → `--radius-sm (4px)`. expanded + collapsed rail.
3. **Canvas panel ghost preview** — `hoverScreen` $state + `terminalGhost` $derived. terminal tool 활성 시 cursor 위치에 480×320 (× zoom) dashed accent outline. drag 중 hide.
4. **ViewportCtrl roundness** — `--radius-pill (50px)` → `--radius-md (6px)`.
5. **ActiveSessionDropdown 단일 버튼** — chevron 제거. `workspaceSwitcher.open()` (AuthDialog choice) → `goList()` (SessionListModal 직접 진입). session *생성* 은 SessionMenu 의 "Switch workspace session…" 에서만.

### 4.3 UI/UX batch 3 (commit `682b584`, 5종)
1. **빈 여백 click → clearM** — 이미 동작 (Canvas onpaneclick 의 select tool 분기 끝). 검증만.
2. **자동 selection 해제 제거** — `sessionStore.loadLayout` 에서 M/I/maximizedItemId/focusMode clear 제거. 외부 source (session 진입) 의 ephemeral reset 은 `setActiveSession` 으로 이동. 결과: drag/resize/align/text PUT 응답마다 selection 유지.
3. **Shift-free lasso + Cmd/Ctrl 동등 sync** — `selectionOnDrag={!isSpacePressed && !isHandTool && !isDragTool}` + `onselectionchange` handler (fast no-op 으로 동일 set skip). Layer panel 자동 동기화.
4. **Toolbar 좌측 정리** — `.page-pill` (Page 1 ▾) 제거 + `ActiveSessionDropdown` 을 `.right` → `.left` 이동.
5. **Canvas 우클릭 ContextMenu** — 이미 wire (변경 없음 — `<ContextMenu bind:this={contextMenuRef} />` + Canvas `onpanecontextmenu`/`onnodecontextmenu`).

### 4.4 multi-drag commit 회귀 fix (commit `682b584` 안 포함)
- **Root cause**: xyflow 가 *NodeWrapper* (단일) 와 *NodeSelection* (group) 두 컴포넌트에서 `onnodedragstop` 호출. group drag 시 `targetNode: null, nodes: dragged`. 기존 코드 `if (!targetNode) return` 가드가 group drag 전체를 skip → BE PUT 없음 → loadLayout 응답이 옛 position 으로 회귀.
- **Fix**: targetNode 가드 제거. nodes.length 만 검사. nodes array iterate → 모든 dragged items 의 new position 일괄 mutateLayout commit (line endpoint delta 처리). 단일/다중 동일 path.

### 4.5 ADR-0020 D13 + plan-0009 (commit `c84cae4`, `22060e1`)
- **결정**: `/auth` page HTML 의 단일 source = FE SPA bundle 의 AuthPage (`routes/auth/+page.svelte`). BE 는 SPA fallback (index.html) 만 응답.
- **거절된 대안**: BE server-rendered HTML 을 시안 디자인으로 직접 교체 (Rust template inline CSS 비대 + 디자인 sync 두 곳 부담).
- **FE (commit `22060e1`)**: `main.ts pickPage` 가 `/auth`, `/auth/*` 도 AuthPage 라우팅. `routes/auth/+page.svelte` 의 stale "demo only" 주석 정리.
- **BE (in-flight)**: plan-0009 §2 의 inventory — auth.rs `auth_page_handler` 제거 + lib.rs route 제거 + SPA fallback 동작 검증. `/auth/login` / `/auth/logout` / `/auth/bootstrap` 변경 없음.

---

## 5. **진행 중 (in-flight) 작업 — 다음 작업자 step**

### 5.1 ⚠️ BE Slice-A1 (plan-0009 §2) — *blocking*
**상태**: 사용자가 별 BE 작업자에게 전달함. FE Slice-A2 는 land 완료, BE 완료 전까지 brower 동작 변화 0.

**Step**:
1. `crates/http-api/src/auth.rs` 의 `auth_page_handler` 제거 (의존 helper 함께)
2. `crates/http-api/src/lib.rs` 의 `.route("/auth", get(auth::auth_page_handler))` 제거
3. `is_auth_path` allow-list 의 `/auth` 는 유지 (cookie-less 도달 허용)
4. SPA fallback 이 `/auth` 를 catch 하는지 검증 (router 끝 `fallback_service` 또는 nest_service)
5. unit test 정리 — `auth_page_handler` 의 test 가 있으면 SPA fallback test 로 변경
6. cargo test PASS → commit `feat(backend): D13 — auth page server-rendered 제거, SPA fallback 으로 위임`

**검증 (BE land 후 사용자가 brower 로)**: plan-0009 §3.3 의 F-1~F-6

### 5.2 합동 검증 시점
1. F-1: cookie-less `/auth` GET → AuthPage 시안 form
2. F-2: `/auth?t=<valid>` magic-link 자동 처리
3. F-3: `/auth?t=<invalid>` → form error
4. F-4: `/auth-preview` 디자인 demo alias 동일
5. F-5: logout → `/auth` redirect → SPA fallback → AuthPage
6. F-6: rate-limit 5회/5분 countdown

---

## 6. 핵심 ADR / plan 인덱스 (cold-pickup 시 우선 읽기)

| 문서 | 범위 | 본 session 에서 amend 했나? |
|---|---|---|
| `docs/sketch.md` | 원본 spec (KO) — scope/우선 | — |
| `CLAUDE.md` | 프로젝트 instruction (ADR-before-code hard rule 포함) | — |
| ADR-0010 | Group data model (parent_id 트리) | — |
| ADR-0014 | Server lifecycle (D12 = graceful shutdown) | BE land (51f3a86) |
| ADR-0017 | Layout grid + chrome (D2 = LeftPanel 의 tab 통합) | — |
| ADR-0018 | Canvas Item data model v2 | — |
| ADR-0019 | Multi-session pivot (D5 / D5.1 / D5.4 / D6 / D9 ...) | 본 session 외 (0045 후속에서 amend) |
| ADR-0020 | Auth lifecycle (D4/D5 mode, D8 inline script, D11 settings, D12 password rotation, **D13 신규 — FE bundle 단일 source**) | ✅ **D13 본 session** |
| ADR-0021 | Terminal pool + multi-attach mirror | — |
| ADR-0024 | Layer list / z-index separation (다중 선택 + bulk action 1차 가치) | — |
| ADR-0025 | (BE) session-scoped PANE_OUT filter | BE land (92a507b) |
| ADR-0026 | (BE) Server identity (workspace-derived) | BE land (53f11cf) |
| plan-0007 | Multi-session pivot 의 Stage / FE-NEW-1~9 + G22/G25/G27/G29 | — |
| plan-0008 | Session attach recovery (Phase 1 ship + Phase 2 wire) | — |
| plan-0009 | **Auth page FE pivot — BE work-package** | ✅ **신규 본 session** |

---

## 7. 미해결 known issue / risk

| Issue | 영향 | 상태 |
|---|---|---|
| BE Slice-A1 (plan-0009) 미land | `/auth` brower 진입 시 BE-rendered HTML 우선 — FE bundle 미동작 | 별 BE 작업자에게 전달됨 |
| `routes/auth/+page.svelte` 의 디자인 polish | 시안 (`ref/frontend-design/auth.html`) 과 1:1 비교 안 됨 | 후속 (D13 plan-0009 §5) |
| Lasso (`selectionOnDrag`) 의 *external 갱신* path 검증 | external (Layer click) → SvelteFlow internal 의 시각 sync — bind:nodes 폐기 후 derived one-way 패턴으로 회귀. 이전 effect_update_depth 회귀 시 controlled binding 필요 가능성 | 시각 회귀 시 검토 |
| `effect_update_depth_exceeded` (잠재) | xyflow `nodes` 가 `$bindable` 인데 우리는 derived one-way pass. 이전에 cycle 발생 후 본 session 에서 *selected 필드 제거* + *callback 패턴 제거* 등으로 회피 | 회귀 모니터링 |
| BE-NEW-4 (Stage 3 WS routing cookie 통합) | sessionStorage 의 Bearer token 보존 필요 — BE land 후 폐기 가능 | 후속 |
| `routes/auth/+page.svelte` 의 mode 자동 감지 | 현재 클라이언트가 token/password 모두 표시. BE config.auth.mode 에 따라 active 모드만 활성 — *서버 mode 추출 API* 가 필요할 수 있음 | follow-up |

---

## 8. 작업 컨벤션 (CLAUDE.md 압축)

- **Code**: English (identifiers / comments / commits / error strings)
- **Docs**: Korean (`sketch.md` 정합)
- **ADR-before-code hard rule**: 비-trivial 결정은 ADR 우선
- **ADR ↔ plan coherence hard rule**: ADR amend 시 linked plan/handover 도 갱신
- **MCP code-review-graph**: 코드 탐색 시 Grep/Glob 전에 graph tool 우선
- **Active plan = `docs/plans/` 의 max-number** — plan-0009

### 8.1 Commit 메시지 패턴 (본 session 사용)
- `feat(frontend): D13 Slice-A2 — …`
- `docs(adr+plan): ADR-0020 D13 + plan-0009 — …`
- `fix(frontend): … — root cause + fix 단락`

### 8.2 Build 확인 패턴
```bash
npx svelte-check --threshold error  # type check (0 errors 통과)
npm run build                       # vite build (modules / time / hash)
```

---

## 9. 자주 헷갈리는 부분 (gotcha)

### 9.1 SvelteFlow nodes 의 source
- 현재: `flowNodes = $derived(...)` (one-way). 이전엔 *bind:nodes={$state}* 시도했으나 effect_update_depth 해결 위해 회귀.
- `selected` 필드: itemToNode 에서 `sessionStore.M.has(item.id)` 로 carry. xyflow 가 *prop merge* — multi-source 위험 시 `bind:` 패턴 재검토.
- `onnodedragstop` callback: **targetNode null = group drag** (NodeSelection 패턴). 절대 가드 X.

### 9.2 sessionStore.loadLayout 의 의도
- M / I / maximizedItemId / focusMode **clear 안 함** (PUT 응답마다 selection 유지). 외부 source 의 reset 책임은 `setActiveSession` 안에 명시.

### 9.3 ContextMenu wire
- `<ContextMenu bind:this={contextMenuRef} />` 가 `+page.svelte` 의 `.workspace` div 안에 mount. Canvas → onContextMenuRequest → openAt.
- 동작 안 한다고 보고되면 z-index / mount 위치 / SvelteFlowProvider 의 context propagation 점검.

### 9.4 ToolbarSubbar = 폐기
- 본 session 에서 *완전 삭제*. text-align UI 는 ItemInfoView (RightPanel) 에 통합. Excalidraw floating 패턴은 *후속 다른 도구* (예: shape stroke) 에서 다시 활용 가능 — git log 의 6b5fb2e 참고.

### 9.5 Auth page 의 두 진입
- `/auth` (production) — BE land 후 SPA fallback → AuthPage
- `/auth-preview` (디자인 demo alias) — 동일 컴포넌트 mount, 단 사용자가 디자인 시안 직접 비교용

---

## 10. 본 handover 의 검증 체크리스트 (다음 작업자가 verify)

- [ ] `git log --oneline -15` 가 본 §3 의 commit 흐름과 일치
- [ ] `docs/plans/0009-auth-page-fe-pivot.md` 의 §2 BE work-package 가 명확
- [ ] `docs/adr/0020-auth-lifecycle.md` 의 D13 amend 가 `:269` 부근에 존재
- [ ] FE build PASS — `dist/assets/index-DhUHqmOD.js`
- [ ] `cargo test -p http-api` (BE) PASS — BE land 후
- [ ] brower `/auth` GET 시 AuthPage 시안 form 표시 — BE land 후 plan-0009 §3.3 F-1~F-6

---

## 11. 변경 이력

- 2026-05-16: 초안 — 본 session 종료 직전 handover. UI/UX batch 1~3 + ADR-0020 D13 + plan-0009 + FE Slice-A2 의 cold-pickup 정리. BE Slice-A1 는 별 작업자에게 전달 — 진행 상태 §5 참조.
