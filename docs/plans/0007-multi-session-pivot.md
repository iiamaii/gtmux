# Plan 0007 — Multi-session pivot 구현 로드맵 (plan-0006 supersede)

- 일자: 2026-05-15
- 작성: agent (system-architect role) + user grilling 17 결정
- 상태: Draft
- 목적: 2026-05-15 multi-session pivot 의 17 결정을 backend / frontend 의 병렬 진행 Stage 로 분해하고, 각 Stage 의 integration gate · 상호 의존성 · 잔여 항목을 명시한다.
- Supersedes: `docs/plans/0006-canvas-workspace-feature-roadmap.md` (single-session 가정 위에서 작성됐고, multi-session pivot 으로 §1, §2 P1, §3 Flow A/F, §6 BE-2/BE-6, §7 ADR-0018~0021 모두 outdated)
- 입력:
  - 2026-05-15 plan-0006 grilling 의 17 결정 (Q1~Q17)
  - `CONTEXT.md` (큰 amend 완료 — 2026-05-15 multi-session pivot section)
  - ADR-0019 (Session+Workspace Model, ADR-0007 supersede)
  - ADR-0018 (Canvas Item Data Model v2)
  - ADR-0020 (Auth Lifecycle)
  - ADR-0021 (Terminal Pool + Mirror, ADR-0015 amend)

## 0. 한 줄 목표

multi-session 가능한 gtmux — *한 server / 한 workspace / N session / 1:1 webpage attach / N:N terminal mirror* — 의 architectural 전환을 완료하고, plan-0006 의 Canvas Item 확장 (text/note/shape/image/document/file_path) 을 그 위에 얹는다.

## 1. 현재 기준점 (2026-05-15)

- backend: PTY-direct (ADR-0013), single-session, server lifetime layout, 164 cargo test PASS, 9999 listening
- frontend: Sprint 7 Stage A~K 완료 (Figma adaptation, chrome 컴포넌트 9건, single-session UX)
- ADR: 0018/0019/0020/0021 신규 + 0006/0007/0015 amend (본 plan 작성 후 진실)
- CONTEXT.md: 2026-05-15 multi-session pivot 큰 amend 완료

## 2. 결정 매트릭스 (17 결정 vs ADR/구현)

| # | 결정 | ADR | 구현 영역 |
|---|---|---|---|
| 1 | Schema unified `items[]` (discriminated union) | ADR-0018 D1 | BE schema + FE node renderer |
| 2 | v1 → v2 hard cutover | ADR-0018 D5, ADR-0006 D15 | BE migration code |
| 3 | Workspace = storage dir, Session = named record (어휘 재정의) | ADR-0019 D1, CONTEXT.md | 전체 어휘 |
| 4 | 3 layer mental model (Server / Workspace / Session) | ADR-0019, CONTEXT.md | 어휘 + UI 표시 |
| 5 | Config=XDG_CONFIG, Workspace path=XDG_DATA_HOME | ADR-0019 D2 | BE config + path |
| 6 | Session attach 시 match-or-spawn + unmatched dialog | ADR-0018 D6 | BE 알고리즘 + FE dialog |
| 7 | Terminal id = Auto UUID + 자유 label | ADR-0018 D2 | BE spawn + schema |
| 8 | Session = single-attach reciprocal | ADR-0019 D3, D6 | BE lock + FE modal |
| 9 | Terminal multi-attach + 입력 공유 (mirror) | ADR-0021 D1, D2 | BE broadcast (이미 ADR-0013 D11) + FE multi-xterm |
| 10 | Auto-mount = trigger session 만 | ADR-0021 D3 | BE dispatcher hook 분기 |
| 11 | Session lifecycle = file 영속, active flag ephemeral | ADR-0019 D5 | BE state |
| 12 | Snapshot shift = simple 1:1 attach | ADR-0019 (자명) | BE attach handler |
| 13 | Heartbeat 15s ping / 30s timeout | ADR-0021 D6 | BE (axum/tungstenite 내장) |
| 14 | 활성 session UI = Disabled + "in use" badge | ADR-0019 D9 | FE modal |
| 15 | Takeover 금지 | ADR-0019 D4 | FE click disable |
| 16 | 새 session = 이름 입력 modal + Create | ADR-0019 D7 | FE modal + BE POST /api/sessions |
| 17 | Auth = Token + password 둘 다 MVP | ADR-0020 D1, D5 | BE auth middleware + FE Auth page |
| **G18** | **Cross-server lock = flock+lease hybrid / 15s ping·30s lease / 1s polling** | ADR-0019 D6.1~D6.7 | BE flock + FE poll |
| **G19** | **Settings = full-screen overlay + 좌측 sidebar nav + auto-save** | plan-0007 §14.8 + §13.9 | FE overlay + BE PATCH |
| **G20** | **Maximize = canvas viewport area fill, FE-only ephemeral** | ADR-0018 D3 amend, plan-0007 §14.6 / §14.20.4 | FE only (schema 영속 제거) |
| **G21** | **file_path open = double-click + confirm + ext+prefix allowlist + argv spawn** | ADR-0023 (신규) D1~D9 | BE argv + audit log + FE confirm modal |
| **G22** | **Toolbar 도구 = one-shot default + Q lock + Esc 해제** | plan-0007 §14.2 / §14.20.3 | FE toolStore |
| **G23** | **Inline edit = Enter commit / Esc cancel / blur commit (modern)** | plan-0007 §14.20.1 | FE 공용 컴포넌트 |
| **G24** | **Layer tree 와 z-index 완전 분리, group z 없음** | ADR-0024 (신규) D1~D8, ADR-0010 amend, ADR-0018 D3 amend | FE Layer list + z store |
| **G25** | **Panel close 와 Terminal kill 분리, dialog 3 옵션, dangling = lazy c2 spawn** | ADR-0021 D9 / D10 amend, ADR-0010 D10/D12/D13 amend | BE close API + FE PanelCloseConfirmModal + DanglingOverlay |
| **G26** | **Keyboard shortcut Hybrid + xterm Esc shell 전달 + P0/P1 매트릭스** | plan-0007 §14.20.5 | FE `shortcutRegistry.svelte.ts` + Settings Shortcut section |
| **G27** | **Theme = light + dark 2 fixed + system detect, chrome/xterm 동기** | plan-0007 §14.10 | FE `themeStore.svelte.ts` + `xtermTheme.ts` + Settings Theme section |
| **G28** | **Session export/import = 1 session 1 JSON file + meta, asset/allowlist 미포함** | plan-0007 §14.8 Storage + §13.9 | BE `POST /api/sessions/import` + FE Settings 의 [Export/Import] 버튼 |
| **G29** | **Select + Hand 둘 다 도구 유지 + Space hold + drag = pan modifier** | plan-0007 §14.2 + §14.20.5.2 | FE Toolbar2 + Space-pan handler |
| **G32** | **Item duplicate = terminal 은 mirror (same terminal_id), Cmd+D (P0) + context/Layer menu + multi-select 일괄 + (20,20) cascade offset** | plan-0007 §14.6 / §14.7 / §14.20.5.2 | FE duplicate handler + ContextMenu / Layer row menu |
| **G37~G40** | **Resize semantic = Figma 표준: Line=endpoint per handle, Free draw=scale path, Image=Shift+drag aspect lock, Multi=bbox scale. Terminal Panel 은 Sprint 7 NodeResizer + xterm fit/winsize 이미 구현, plan SoT 명시.** | plan-0007 §14.20.6 신규 | FE LineNode / FreeDrawNode / ImageNode + Multi-resize handler |
| **G33** | **Viewport zoom = 25%~200%, Cmd+0/1/2 (P0) + Cmd+./=/- (P1), statusbar 우측 cluster, mouse wheel + trackpad native pinch/pan** | plan-0007 §14.9 + §14.20.5.2/.3 | FE ViewportCtrl 확장 |
| **G34** | **Focus mode = M 외 dim (50% opacity), Streaming 영향 X, ephemeral, Statusbar [🎯] toggle + Cmd+Shift+F (P1)** | plan-0007 §14.20.7 신규 | FE focusStore + statusbar toggle |
| **G35** | **Terminal settings = Global default (Settings → Terminal section) + Per-panel override (Panel Settings modal). Default: 13px / wrap / 1000 lines / block cursor / visual bell.** | plan-0007 §14.7 / §14.8, ADR-0018 D4 amend (terminal_overrides) | FE Settings Terminal section + Panel Settings Terminal Override sub-section + BE schema field |
| **G36** | **Shell template = Settings → Templates CRUD (4 default preset: bash/zsh/python/htop), Toolbar [Terminal▾] dropdown, `${XDG_CONFIG_HOME}/gtmux/terminal-templates.toml`** | plan-0007 §14.2 / §14.8 / §13.20 amend | FE Toolbar dropdown + BE template CRUD + POST /api/terminals { template_id } |
| **Tier 3 amend** | **Server shutdown confirm modal (Cmd+Shift+Q + SessionMenu) + WS reconnect backoff (1s grace + 1/2/4/8/16/cap 30s, 10회 후 banner)** | plan-0007 §14.10 + §13.9 amend | FE `ServerShutdownConfirmModal.svelte` + `reconnect.svelte.ts` + BE `POST /api/shutdown` |

## 3. 어휘 재정의 요약 (CONTEXT.md 정합)

| 옛 어휘 | 새 어휘 |
|---|---|
| Pane | **Terminal** (어휘 통일) |
| Panel | Panel (그대로, `type:"terminal"` 인 Canvas Item) |
| Session (= Server logical 이름) | (의미 폐기, ADR-0007 supersede) |
| 사용자가 사용한 "workspace" 단위 | **Session** (workspace 안 named record) |
| (없음) | **Workspace** (server 와 1:1 storage dir) |
| (없음) | **Webpage** (WS 연결 = session 의 편집 채널) |
| MT-3 server-wide broadcast | session-scoped state + server-wide terminal stream (ADR-0021 D5) |

## 4. Stage 분해 (BE / FE parallel + integration gate)

### Stage 0 — Foundation (ADR + Docs)  ✅ **완료**

| 작업 | 산출물 |
|---|---|
| ADR-0019 / 0018 / 0020 / 0021 작성 | `docs/adr/0018~0021.md` |
| CONTEXT.md amend | `CONTEXT.md` §2026-05-15 amend (multi-session pivot) |
| ADR-0006 / 0007 / 0015 amend | header amend |
| Plan 0007 (본 문서) | `docs/plans/0007-multi-session-pivot.md` |

후속 모든 Stage 의 진실. 본 단계의 변경 후 코드 작업 시작.

### Stage 1 — Workspace + Session storage (BE leading, FE 따라옴)

**Critical path. 모든 후속의 prereq.**

#### Stage 1-BE
- BE-1.1: `WorkspaceManager` 모듈 — config 의 workspace_path read + default XDG_DATA_HOME, dir 자동 생성 (ADR-0019 D2)
- BE-1.2: Schema v2 정의 (serde, `Item` enum + `Group` 그대로) — ADR-0018 D1, D3, D4
- BE-1.3: v1 → v2 migration 코드 (boot 1회, atomic write) — ADR-0018 D5
- BE-1.4: `SessionRecord` CRUD — file read/write (atomic, ADR-0006 정합), validation (D8)
- BE-1.5: HTTP API 신규:
  - `GET /api/sessions` — workspace 의 모든 session 목록 + active flag
  - `POST /api/sessions { name }` — 새 session 생성
  - `DELETE /api/sessions/:name`
  - `GET /api/sessions/:name/layout`
  - `PUT /api/sessions/:name/layout` (ETag, ADR-0006 정합)
- BE-1.6: Server config (`config.toml`) load + default 자동 생성 if 미존재

#### Stage 1-FE
- FE-1.1: `workspaceStore` (server-wide, session 목록 + active flags)
- FE-1.2: `sessionStore` (active session 의 layout state) — 기존 panels store / mux store 의 session-scoped 화
- FE-1.3: HTTP fetch wrapper amend (`/api/sessions/:name/layout`)
- FE-1.4: Schema v2 TS type 정의 (discriminated union `CanvasItem`)

#### Stage 1-integration gate
- smoke-1: `gtmux start` → workspace dir 자동 생성 → `POST /api/sessions { name: "test1" }` → `GET /api/sessions` 에 "test1" available 표시 → `PUT` layout → `GET` 으로 동일 layout

### Stage 2 — Auth + Dialog + Session list modal (BE/FE parallel)

#### Stage 2-BE
- BE-2.1: Auth middleware (axum) — cookie 검증
- BE-2.2: `GET /auth` — auth page (mode 에 따라 token query 처리 또는 password form)
- BE-2.3: `POST /auth/login { password? }` — Argon2id verify + rate limit + cookie 발행
- BE-2.4: `POST /auth/logout`
- BE-2.5: Server-side session table (in-memory `HashMap<cookie, AuthSession>`)
- BE-2.6: CLI `gtmux set-password` 액션
- BE-2.7: Bootstrap inline script 폐기 — server-side redirect 흐름 (ADR-0020 D8)

#### Stage 2-FE
- FE-2.1: `/auth` route 신규 (Svelte page)
- FE-2.2: Auth page UI (token mode = URL query 처리 only, password mode = form)
- FE-2.3: Cookie 기반 인증 흐름 (axios/fetch 의 credentials: include)
- FE-2.4: `AuthDialog.svelte` (인증 후 [새 / 기존] 선택, ADR-0019 D8)
- FE-2.5: `NewSessionModal.svelte` (이름 입력 + Create, ADR-0019 D7)
- FE-2.6: `SessionListModal.svelte` (목록 + 활성 "in use" disabled, ADR-0019 D9)
- FE-2.7: Logout 액션 (SessionMenu)

#### Stage 2-integration gate
- smoke-2: `gtmux start` (default token mode) → 브라우저 진입 → Auth page (URL 의 token) → Dialog → [새 session] → 이름 입력 → 빈 Canvas 진입. Reload → Auth page 건너뛰고 (cookie 유효) 바로 Dialog.
- smoke-3: `gtmux set-password` → server 재기동 password mode → Auth page → password 입력 → Dialog 흐름 동일.

### Stage 3 — Session attach + Match-or-spawn + WS frame routing (BE leading)

#### Stage 3-BE
- BE-3.1: Session attach handler — `POST /api/sessions/:name/attach { cookie }` (또는 WS handshake 내 attach intent)
- BE-3.2: WS server 의 session_id routing — 각 connection 의 attached session 추적
- BE-3.3: Match-or-spawn 알고리즘 (ADR-0018 D6):
  - layout 의 terminal items[].id 들 ↔ server-pool alive Terminal ids 매칭
  - 매칭 → 그대로 (panel ↔ existing terminal subscribe)
  - 미매칭 → 같은 id 로 spawn (PtyBackend, ADR-0014 정합)
  - unmatched dialog 정보 (count) 응답 또는 NOTIFY frame
- BE-3.4: Session 의 single-attach lock (ADR-0019 D3, D6)
  - workspace/.locks/<name>.lock 파일
  - lock acquire 시점에 fail-fast (다른 webpage 이미 attach 중이면 409 Conflict)
  - WS close 시 lock release
- BE-3.5: WS frame 의 session-scoped 분리 (ADR-0021 D5)
  - selection-changed / viewport-changed / etc 는 `session_id` 필드 + 그 session 의 attached webpage 에만 send
  - pane-output / pane-died / pane-spawned 는 그대로 server-wide broadcast

#### Stage 3-FE
- FE-3.1: Session list modal 의 [Attach] 클릭 → POST attach → match-or-spawn 결과 받기
- FE-3.2: Unmatched confirm dialog (ADR-0018 D6, ADR-0021 D9)
  - "이 session 을 attach 하면 N 개의 새 terminal 이 spawn 됩니다. 계속?"
  - [Cancel] / [Confirm]
- FE-3.3: 활성 session 의 layout 만 받기 (session-scoped 라우팅)
- FE-3.4: Single-attach 충돌 시 (409 응답) — modal 의 그 session 을 disabled 로 갱신 + toast

#### Stage 3-integration gate
- smoke-4: 한 탭에서 session1 만들고 terminal 추가 → 탭 close → 다른 탭 reload → Dialog → 기존 session 연동 → modal 의 session1 selectable (heartbeat timeout 후) → 클릭 → unmatched confirm (terminal 1 개 spawn) → Canvas 에 panel 표시.
- smoke-5: 두 탭이 같은 session 동시 attach 시도 → 한 탭만 성공, 다른 탭은 disabled 갱신.

### Stage 4 — Terminal pool + Multi-attach mirror UI (BE/FE parallel)

#### Stage 4-BE
- BE-4.1: Terminal list API — `GET /api/terminals` (server-wide alive terminals + 각 terminal 의 attach 점 정보)
- BE-4.2: Panel rebind API — `PUT /api/sessions/:name/items/:id/terminal { terminal_id }`
- BE-4.3: Auto-mount trigger session 분기 (ADR-0021 D3) — dispatcher hook 의 cascade target 좁히기
- BE-4.4: WS heartbeat (15s ping / 30s timeout) — axum/tokio-tungstenite 의 PING 활성화
- BE-4.5: 비정상 종료 감지 → session active=false → workspace/.locks/<name>.lock 해제
- **BE-4.6: Panel/Terminal close 분리 API (G25, BE-NEW-12.5)** — `DELETE /api/sessions/<name>/items/<id>?kill_terminal=<bool>` + `POST /api/terminals/<id>/respawn` + `POST /api/terminals/<id>/kill` + WS `terminal_died` broadcast (ADR-0021 D9/D10)

#### Stage 4-FE
- FE-4.1: LeftPanel 의 Terminals tab — `TerminalListView.svelte` (ADR-0017 2026-05-16 ② amend: LeftPanel 의 두 번째 가로 탭)
- FE-4.2: Panel context menu 의 [Change terminal...] — list modal
- FE-4.3: Toolbar 우측 활성 session 드롭다운 → SessionListModal
- FE-4.4: 다중 xterm 인스턴스 (한 terminal_id 의 panel 이 여러 session 의 view 에서 동시 render)
- FE-4.5: Multi-attach mirror 시각 표시 (Terminal list 의 attach 수 + 어느 session 들이 attach 중인지)
- **FE-4.6: Dangling overlay (G25, ADR-0021 D10.1 c2)** — terminal_died WS frame 수신 시 그 id 의 모든 panel 에 `[exit] — Click to restart` overlay → focus / click / input 시 respawn API 호출 + xterm 재attach

#### Stage 4-integration gate
- smoke-6: 한 탭에서 [New Terminal] → 다른 탭의 Terminal list 갱신 (그 탭 layout 에는 mount 안 됨) → 다른 탭의 사용자가 그 terminal 을 [Attach to canvas] → 두 탭이 같은 terminal 의 다른 panel 들. 한 쪽에서 `echo $$` 타이핑 → 두 쪽 모두 동일 PID 출력.

### Stage 5 — Canvas Item (text/note/rect/ellipse/line/file_path) (FE leading)

ADR-0018 D4 의 non-terminal type 중 asset 비의존 type 들. (image/document = Stage 8, free_draw = Stage 9 — asset 의존)

#### Stage 5-FE
- FE-5.1: Toolbar2 도입 — Select/Hand/Text/Note/Rect/Ellipse/Line/**File Path** 도구. G22 one-shot + Q lock 정책 정합 (§14.20.3)
- FE-5.2: `toolStore` (current tool + locked, §14.2)
- FE-5.3: Per-type Node renderer (TextNode, NoteNode, ShapeNode, LineNode, **FilePathNode**)
- FE-5.4: Creation gestures (click-to-create, drag-to-create, pointer capture, cancel on Esc — §14.20.2 Esc 라우팅 정합)
- FE-5.5: Inline edit (text/note 의 placeholder + Enter commit / Esc cancel — G23 정책 정합, §14.20.1 공용 컴포넌트 사용)
- FE-5.6: Item rename inline (label field) — *공용 `InlineEditField.svelte` 사용 (G23)*
- **FE-5.7: file_path open UX (G21, FE-NEW-8)** — double-click → confirm modal → allowlist check → backend POST. Settings → Storage 의 allowlist editor 도 같이.

#### Stage 5-BE
- BE-5.1: Schema v2 의 non-terminal validation (text/note/rect/ellipse/line/**file_path** 의 payload field)
- BE-5.2: payload size cap (label/description 4 KB, text 64 KB)
- **BE-5.3: file_path OS-level open + allowlist (G21, BE-NEW-12, ADR-0023)** — `POST /api/file-path/open` + allowlist CRUD + argv spawn + audit log

#### Stage 5-integration gate
- smoke-7: Toolbar 의 Text 도구 클릭 → Canvas 위 클릭 → text item 생성 → 인라인 입력 "hello" → Enter commit → layer list 에 표시 → PUT layout 영속 → reload 시 같은 위치/내용 복원.
- **smoke-7b (G21 정합)**: File Path 도구 클릭 → Canvas 위 클릭 → path 입력 (`/home/me/notes/spec.md`) → file_path item 생성 → double-click → confirm modal → [✓ Always for *.md within /home/me/notes/] + [Open] → toast "Opened externally" + allowlist 영속. 다시 double-click → confirm 생략 + 즉시 open.

### Stage 6 — Layer list V2 + Panel header redesign (FE leading)

#### Stage 6-FE
- FE-6.1: Layer list V2 — terminal Panel + non-terminal Canvas Item 통합 트리
- FE-6.2: Multi-select (Cmd+click, Shift+click, drag marquee)
- FE-6.3: Group tree (drag reorder, drag reparent, expand/collapse)
- FE-6.4: Per-row toggles (visibility, lock) + propagation 표시 (ADR-0010 정합)
- FE-6.5: Panel header redesign — id / label / status / Input Target marker / minimize / maximize / invisible / close + more menu
- FE-6.6: Panel footer description (collapsible)
- FE-6.7: Minimize state (header bar 만 + bottom radius) — *영속* (schema field)
- FE-6.8: Maximize state (G20 grilling) — *Canvas viewport area fill* (Titlebar/Toolbar/Status bar 유지). **FE-only ephemeral** (schema 영속 안 함, 다음 attach 시 fresh). 한 시점 1 panel 만 maximize (toggle 시 다른 max 자동 해제). Unmaximize trigger = Esc / 헤더 toggle 버튼 / panel header double-click. Esc 우선순위 = modal stack top 우선 (Settings overlay / dialog 가 위면 그것이 Esc 흡수).

#### Stage 6-integration gate
- smoke-8: 여러 item 다중 선택 → Group 액션 → Group row 생성 → Group visibility 토글 → 자손 모두 dim. Panel minimize → header bar 만 표시.

### Stage 7 — Viewport sync + Settings page (BE/FE parallel)

#### Stage 7-BE
- BE-7.1: Viewport state — session 의 layout 의 일부로 영속 (PUT layout 의 일부)
- BE-7.2: Settings API — `GET /api/settings` (config read-only surface), `PUT /api/settings { ... }` (mutable 만)

#### Stage 7-FE
- FE-7.1: ViewportCtrl 확장 (go to selection, fit selected/all, sync indicator)
- FE-7.2: Settings page (full-screen overlay, Svelte 5 simple route 변형 또는 modal)
- FE-7.3: Panel settings modal (per-panel 설정)

#### Stage 7-integration gate
- smoke-9: 같은 session 의 두 탭 (불가, single-attach) 대신 *layout 의 viewport 영속* 검증 — pan/zoom 후 detach + reattach → 같은 viewport 복원.

### Stage 8 — Asset items (image/document/file_path) — P2

- 별 ADR (asset storage 정책) 작성 필요. `${XDG_DATA_HOME}/gtmux/workspace/assets/`, content hash file name, MIME sniffing, size cap.
- UI: drop target, file picker, upload progress toast.

### Stage 9 — Free draw + perf — P2

- Point simplification (Douglas-Peucker)
- Batching debounce
- Backpressure 측정 (50 pane × 5 burst)

### Stage 10 — Testing + stabilization

- vitest + @testing-library/svelte
- Store unit tests
- Chrome 컴포넌트 통합 tests
- Playwright E2E (Stage 1~7 의 smoke-1~smoke-9 자동화)
- Multi-tab race tests (Stage 3 의 smoke-5)
- Light/dark visual regression
- Multi-session lifecycle (long-running, leak 검증)

## 5. Integration gate 운영 정책

각 Stage 의 *smoke* 가 통과해야 다음 Stage 진입. BE 가 leading 인 Stage 는 BE-only smoke (cargo test + curl) 로 FE 가 따라옴.

| Stage | Gate type | 검증 도구 |
|---|---|---|
| 0 | docs review | manual + git log |
| 1 | smoke-1 | cargo test + curl POST/GET /api/sessions |
| 2 | smoke-2, smoke-3 | playwright + manual browser |
| 3 | smoke-4, smoke-5 | playwright multi-tab |
| 4 | smoke-6 | playwright + xterm echo check |
| 5 | smoke-7 | playwright + manual |
| 6 | smoke-8 | playwright + visual screenshot |
| 7 | smoke-9 | playwright |
| 8~9 | (P2) | Stage 10 의 E2E 안에 |

## 6. BE / FE 의존성 매트릭스

| BE 산출물 | FE 가 기다림 |
|---|---|
| Schema v2 정의 + TS type 합의 | Stage 1-FE 모두 |
| `/api/sessions` API | Stage 2-FE (Session list modal) |
| Cookie 인증 middleware | Stage 2-FE (Auth + Dialog) |
| Match-or-spawn 알고리즘 결과 응답 | Stage 3-FE (Unmatched dialog) |
| Session-scoped WS frame routing | Stage 3-FE (session-scoped store) |
| Terminal list API | Stage 4-FE (Terminal list UI) |
| Panel rebind API | Stage 4-FE (Change terminal) |

| FE 산출물 | BE 가 기다림 |
|---|---|
| (없음 — BE 가 cycle 시작) | |

→ **BE 가 critical path**. FE 는 BE 의 schema + API 합의를 기다림. BE leading 의 Stage 가 끝나면 FE 의 stage 가 병렬 가능.

## 7. ADR / CONTEXT / SSoT 참조표

| 산출물 | 의존성 |
|---|---|
| `CONTEXT.md` | 본 plan 진입 prereq (이미 amend 완료) |
| `docs/adr/0018-canvas-item-data-model.md` | Stage 1~4 의 BE 진실 |
| `docs/adr/0019-session-and-workspace-model.md` | 본 plan 전체의 architectural 진실 |
| `docs/adr/0020-auth-lifecycle.md` | Stage 2 의 BE 진실 |
| `docs/adr/0021-terminal-pool-and-mirror.md` | Stage 3~4 의 BE 진실 |
| `docs/adr/0007-server-session-port-binding.md` | Superseded by ADR-0019 |
| `docs/adr/0006-persistence-storage.md` | D15 amend (schema v2 hard cutover) |
| `docs/adr/0015-pane-auto-mount.md` | ADR-0021 D3 amend (cascade target) |
| `docs/adr/0002-transport-websocket.md` | D3 amend (MT-3 → 2-layer) |
| `docs/ssot/canvas-layout-schema.md` | **갱신 필요** — schema v2 가 본 SSoT 의 진실 |
| `docs/ssot/wire-protocol.md` | **갱신 필요** — session_id field + heartbeat |
| `docs/ssot/security-defaults.md` | **갱신 필요** — auth section, password mode, cookie |

## 8. P0 / P1 / P2 우선순위

### P0 (이번 pivot 의 본체)
- Stage 0 (ADR/CONTEXT/plan) ✅
- Stage 1 (Workspace + Session storage)
- Stage 2 (Auth + Dialog + Session list modal)
- Stage 3 (Session attach + Match-or-spawn + WS routing)
- Stage 4 (Terminal pool + Multi-attach mirror UI)
- Stage 6 (Layer list V2 + Panel header redesign) — 사용자 직관 회복

### P1 (Canvas Item 확장)
- Stage 5 (Text/Note/Rect/Ellipse/Line)
- Stage 7 (Viewport sync + Settings)

### P2 (asset / free-draw / 안정화)
- Stage 8 (Asset items)
- Stage 9 (Free draw + perf)
- Stage 10 (Testing + stabilization) — 본 pivot 의 모든 layer 에 대한 회귀 보장

## 9. 주요 리스크

| 리스크 | 설명 | 대응 |
|---|---|---|
| Schema v2 churn | 본 pivot 의 schema 변경 큼, 후속 P1+ 에서 다시 churn 가능 | ADR-0018 §D8 의 validation 보수적, type 추가는 새 ADR (asset/freedraw) 에서 분리 |
| Session lock race | 두 webpage 동시 attach 의 lock acquire | file lock + ETag + 명확한 409 응답 (ADR-0019 D6) |
| Single-attach UX 마찰 | 사용자가 같은 session 두 탭에서 보고 싶을 때 못 함 | Terminal multi-attach 가 별 layer 에서 mirror 욕구 해소 (ADR-0021), 그래도 UX 안내 명확화 |
| Heartbeat false-positive | 느린 네트워크에서 false inactive | 30s timeout 으로 보수적, P1+ 에서 측정 후 조정 |
| Cookie 도난 | XSS / network sniff | HttpOnly Secure SameSite=Strict + lifecycle 짧음 (7일) + rotation (ADR-0020) |
| Password mode 의 보안 표면 | Argon2 calibration + rate limit 정합 | ADR-0020 D5 의 매개변수 + P1+ 의 machine-specific 보정 |
| Auto-mount trigger 식별 정합 | 어느 webpage 가 trigger 인지 BE 가 알아야 | WS connection 의 attached session 정보로 routing (Stage 3-BE) |
| BE-FE schema 동기화 | TS type 과 Rust schema 의 sync | TS type 수동 합의 + 양쪽 fixture (P1+ ts-rs 검토) |

## 10. 미해결 잔여 (P2+ ADR 후보)

### 10.1 Resolved (G18~G40 + Tier 3 grilling 으로 처리됨)

- ~~**Cross-server session lock**~~ → **Resolved 2026-05-15 G18**. ADR-0019 D6.1~D6.7.
- ~~**file_path OS-level open 보안 정책**~~ → **Resolved 2026-05-15 G21**. ADR-0023 신규.
- ~~**xterm theme adapter**~~ → **Resolved 2026-05-15 G27**. plan-0007 §14.10 amend.
- ~~**Keyboard shortcut global registry**~~ → **Resolved 2026-05-15 G26**. plan-0007 §14.20.5.
- ~~**Session export/import**~~ → **Resolved 2026-05-15 G28**. plan-0007 §14.8 + §13.9.
- ~~**Toolbar Select/Hand 필요성**~~ → **Resolved 2026-05-15 G29**. plan-0007 §14.2 + Space-pan modifier.
- ~~**Item duplicate**~~ → **Resolved 2026-05-15 G32**. plan-0007 §14.6/§14.7/§14.20.5.2.
- ~~**Viewport zoom controls**~~ → **Resolved 2026-05-15 G33**. plan-0007 §14.9 + §14.20.5.2/.3.
- ~~**Focus mode (M 외 dim)**~~ → **Resolved 2026-05-15 G34**. plan-0007 §14.20.7.
- ~~**Terminal sub-settings**~~ → **Resolved 2026-05-15 G35**. plan-0007 §14.7/§14.8 + ADR-0018 D4 amend.
- ~~**Shell template (pane 생성 템플릿)**~~ → **Resolved 2026-05-15 G36**. plan-0007 §14.2/§14.8/§13.20.
- ~~**Resize semantic (Line/Free draw/Image/Multi)**~~ → **Resolved 2026-05-15 G37~G40**. plan-0007 §14.20.6.
- ~~**Server shutdown confirm + WS reconnect backoff**~~ → **Resolved 2026-05-15 Tier 3**. plan-0007 §14.10/§13.9.

### 10.2 Tier 2 P2 deferred (Stage 8+ 진입 전 grilling)

각 항목 — *design hint* + *future grilling anchor*. 결정은 Stage 8+ 직전.

- **Asset storage (P2, ADR-0022 후보)** — image/document 의 storage path / content hash (sha256) / MIME sniffing (libmagic 또는 ext-based) / size cap / garbage collection (orphan asset 정리). file_path 는 ADR-0023 으로 분리됨.
- **Snap to grid (P2, design hint)** — grid size (8px / 16px) 의 default, snap 활성 toggle (Toolbar 또는 Settings), Shift hold 시 임시 disable, group / line endpoint 의 snap 규칙.
- **Align / Distribute (P2, design hint)** — M (multi-selection) 2 개 이상 시 toolbar/menu 에 정렬 액션 (left/center/right/top/middle/bottom + horizontal/vertical distribute). Figma 표준.
- **Mini-map (P2, design hint)** — Statusbar 또는 LeftPanel 의 작은 viewport 미니뷰. 현 viewport rect 표시 + click 으로 pan. SvelteFlow 의 `<MiniMap />` 컴포넌트 활용 가능.
- **Undo / Redo (P2, design hint)** — Command pattern 의 client-side history stack. 모든 mutation (move, resize, rename, group, ungroup, delete, duplicate) 의 *역연산* 기록. WS sync 와의 정합 — undo 가 server PUT 또는 단순 client revert 후 다음 commit. Cmd+Z / Cmd+Shift+Z 단축키 (P1+).
- **Layout preset (P2, design hint)** — workspace 안에서 *named layout copy* (session 의 alternative form). G28 의 session export/import 와 부분 정합 (preset = 내부 import). Settings → Storage 의 [Save current as preset] / [Load preset].
- **Command palette (P2, ADR-0026 후보)** — `Cmd+K` quick action (sketch §7.3 의 quick jump). 사용자가 입력 → fuzzy match → 액션 list 보여줌 (예: "switch to session X", "open settings", "kill terminal", "new terminal"). Spotlight / VS Code 표준. G26 P1+ 후보 그대로.
- **최근 활성 panel 기록 + 즐겨찾기 (P3, design hint)** — LeftPanel 의 추가 탭 또는 Sessions/Templates 같은 그룹. 사용자 즐겨찾기 toggle. P3 (즐거움 항목이라 후순위).

### 10.3 비범위 (P3)
- **Mobile / responsive** — sketch §3 비범위
- **Multi-user / ACL** — sketch §3 비범위
- **고대비 모드** — accessibility, P3 또는 별 ADR
- **Customization shortcut rebind** — G26 의 P3

## 13. Backend 기능 명세 (plan-0006 §6 amend + 신규)

각 항목: 1~3줄 요약 + 의존 ADR + 산출물 위치.

### 13.1 BE-1 Auth (큰 amend by ADR-0020)
Token + password 둘 다 지원, cookie 기반 lifecycle (7일 default, rolling renew), Argon2id hash + rate limit, rotate UI 지원. bootstrap inline script 폐기 → server-side redirect.
- ADR-0020 D1~D10 정합
- 산출물: `codebase/backend/crates/http-api/src/auth/` (신규)
- HTTP: `GET /auth`, `POST /auth/login`, `POST /auth/logout`, `POST /auth/rotate`

### 13.2 BE-2 Canvas Layout schema v2 (amend by ADR-0018)
`{ schema_version: 2, groups: [...], items: [...], viewport: {...} }` — unified discriminated union. 옛 `panels[]` 폐기.
- ADR-0018 D1~D4
- 산출물: `codebase/backend/crates/http-api/src/storage/schema.rs` (기존 amend)

### 13.3 BE-3 Schema validation v2 (amend by ADR-0018 D8)
Rust serde `Item` enum + `tag = "type"`. id UUID format, parent_id 무결성, type-specific payload validation, cap (label/description 4KB, text 64KB, points 5000, file 16MB).
- ADR-0018 D8
- 산출물: 같은 storage crate, validation 모듈

### 13.4 BE-4 Asset storage (P2+, 별 ADR 후보)
image/document item 의 file storage. `${workspace_path}/assets/<sha256>` + metadata json. 본 plan 의 P2.
- 별 ADR (0022 후보) 필요
- 산출물: `codebase/backend/crates/http-api/src/assets/` (Stage 8)

### 13.5 BE-5 File path policy (그대로 + ADR-0018 D4 정합)
file_path item 의 path 는 string-only. backend 가 자동 read/open 안 함. open 액션은 P1+ explicit opt-in (사용자 명시 호출).
- ADR-0018 D4, ADR-0019 D11
- 산출물: schema validation 안

### 13.6 BE-6 WebSocket sync extension (**재정의** by ADR-0021 D5)
**옛 server-wide MT-3 broadcast 폐기**. 새 2-layer 모델:
- Session-scoped state (M/I/Viewport/Focus) — `session_id` 라벨 부착, 그 session 의 attached webpage 에만 send
- Server-wide terminal stream (pane-output/input/lifecycle) — 그대로 tokio::broadcast N:N
- ADR-0021 D2, D5
- 산출물: `codebase/backend/crates/ws-server/src/router.rs` (재작성)

### 13.7 BE-7 Conflict handling + cross-server lock (amend by ADR-0019 D6)
기존 ETag PUT 유지 + 새 cross-server session lock (`workspace/.locks/<name>.lock` 파일 + PID + lease). stale lease 자가 정리.
- ADR-0019 D6
- 산출물: `codebase/backend/crates/http-api/src/session_lock.rs` (신규)

### 13.8 BE-8 Terminal metadata (amend by ADR-0021 D7)
기존 (id, label, alive/dead status) + 신규 (attach 점 — 각 terminal 의 attached panels 의 (session_id, panel_id) 목록).
- ADR-0021 D7
- 산출물: `Terminal` 구조체 amend, `GET /api/terminals` response

### 13.9 BE-9 Settings API (amend by ADR-0020 D5, G19 grilling)
- `GET /api/settings` — config 의 read-only surface + mutable field 의 현재 값
- `PATCH /api/settings { field: value }` — field 단위 즉시 commit (FE 의 auto-save 정합, G19.1)
- `POST /api/settings/password { current, new }` — Argon2id rehash + lockout 검사 (form-group 단위)
- `POST /api/settings/logout-all` — 모든 active session 의 cookie revoke + WS disconnect (destructive, confirm 후 호출)
- **`POST /api/shutdown` (Tier 3 amend)** — graceful server shutdown trigger. ADR-0014 D7 정합: WS close (모든 webpage) + 모든 child SIGHUP 정리 + 모든 session record sync flush + state/lock dir 정리 → exit 6. Confirm modal 통과 후 호출 (G26 의 Cmd+Shift+Q 단축키 + SessionMenu).
- Boot-immutable field 의 PATCH 시도는 403 "Boot-immutable; restart required"
- **`POST /api/sessions/import { content, on_conflict: "rename"|"override"|"reject" } { new_name? }` (G28)** — content 의 schema validate (gtmux_export_version, schema_version) → 이름 conflict 처리 → 새 session record 생성. Response: `{ created_name }`.
- ADR-0020 D5, G28
- 산출물: settings endpoint amend + `crates/http-api/src/sessions/import.rs` (신규)

### 13.10 BE-10 Performance / safety (그대로)
Payload size cap + drawing point simplification (P2+) + asset lazy load (P2+) + upload cap + audit logs.
- 그대로
- 산출물: 각 영역별 분산

### 13.11 BE-NEW-1 WorkspaceManager (신규, ADR-0019 D2)
Config 의 `workspace_path` read + default `${XDG_DATA_HOME}/gtmux/workspace/` + boot 시 dir 자동 생성. immutable 바인딩 (D11).
- ADR-0019 D2, D11
- 산출물: `codebase/backend/crates/http-api/src/workspace/manager.rs` (신규)

### 13.12 BE-NEW-2 SessionRecord CRUD (신규, ADR-0018 + ADR-0019)
File read/write (atomic via ADR-0006 정합), schema v2 직렬화, validation. enumeration + active flag join.
- ADR-0018 D1~D8, ADR-0019 D5
- 산출물: `codebase/backend/crates/http-api/src/workspace/sessions.rs` (신규)
- HTTP: `GET /api/sessions`, `POST /api/sessions { name }`, `DELETE /api/sessions/:name`, `GET/PUT /api/sessions/:name/layout`

### 13.13 BE-NEW-3 Session attach + match-or-spawn 알고리즘 + single-attach lock (신규, ADR-0018 D6 + ADR-0019 D3/D6)
Attach handler 가:
1. Single-attach lock acquire (D6 의 file lock)
2. Layout 의 terminal items[].id ↔ server-pool alive Terminal ids 매칭
3. Match → reconnect (panel ↔ existing terminal subscribe)
4. No match → 같은 id 로 fresh spawn (PtyBackend, ADR-0014)
5. Unmatched count 응답 (FE 의 confirm dialog 용)

- ADR-0018 D6, ADR-0019 D3/D6
- 산출물: `codebase/backend/crates/http-api/src/workspace/attach.rs` (신규)
- HTTP: `POST /api/sessions/:name/attach` (또는 WS handshake intent)

### 13.14 BE-NEW-4 WS frame routing (신규, ADR-0021 D5)
WS server 가 각 connection 의 attached session 추적 → frame envelope 의 `session_id` 검사 → broadcast target 분기:
- selection-changed / viewport-changed / focus-changed → 그 session 의 attached webpage 만
- pane-output / pane-died / pane-spawned → server-wide
- ADR-0021 D5
- 산출물: `codebase/backend/crates/ws-server/src/router.rs` (BE-6 와 정합)

### 13.15 BE-NEW-5 Heartbeat (신규, ADR-0021 D6)
axum/tokio-tungstenite 의 PING/PONG 활성화 — 15s interval, 30s timeout. timeout 시 그 session 의 active=false + lock release.
- ADR-0021 D6
- 산출물: WS connection 초기화 코드 amend

### 13.16 BE-NEW-6 Auto-mount trigger-aware (신규, ADR-0021 D3 = ADR-0015 amend)
Dispatcher hook 의 NOTIFY pane-spawned 가 `trigger_session_id` 포함. cascade target 분기:
- trigger_session == attached session → 그 webpage 에 mount-cascade frame
- 다른 session → terminal-list-update frame 만
- ADR-0021 D3
- 산출물: `codebase/backend/crates/ws-server/src/dispatcher.rs` (amend)

### 13.17 BE-NEW-7 Cookie 기반 인증 lifecycle (신규, ADR-0020 D2/D3)
Server-side session table (in-memory `HashMap<cookie_token, AuthSession>`). HttpOnly Secure SameSite=Strict, Max-Age=7일, rolling renew on every valid request.
- ADR-0020 D2, D3
- 산출물: `codebase/backend/crates/http-api/src/auth/cookie.rs` (신규)

### 13.18 BE-NEW-8 Token + Password mode dispatch (신규, ADR-0020 D1, D4/D5)
Config 의 `auth.mode` 에 따라 분기. token mode = URL query verify, password mode = Argon2id verify + rate limit + lockout.
- ADR-0020 D1, D4, D5
- 산출물: auth handler 내부 분기 + Argon2 verify + rate limiter

### 13.19 BE-NEW-9 Cross-server session lock (신규, ADR-0019 D6, G18 grilling 결과)
`${workspace_path}/.locks/<session-name>.lock` 파일 — **OS flock(2) + lease 내용 hybrid** (ADR-0019 D6.1).
- Acquire: `open + flock(LOCK_EX|LOCK_NB)` → 성공 시 JSON write `{ server_id, pid, ws_conn_id, lease_until }`.
- Peek (다른 server 의 modal row 판정): `flock(LOCK_SH|LOCK_NB)` → EWOULDBLOCK 면 in-use, 내용 read 해서 holder 표시. LOCK_SH 성공 시 stale → unlink 후 acquire 가능.
- Lease 갱신: WS heartbeat ping 15s 주기 → lease_until = now+30s 재기록 (ADR-0021 D6 정합).
- Release: 정상 close / heartbeat timeout / shutdown hook 모두 `LOCK_UN + unlink`. SIGKILL 은 kernel auto-release + 다음 acquirer 가 내용 덮어쓰기.
- Server-internal mutex: 같은 server 안 동시 attach 시도는 attach handler critical section 으로 직렬화.
- ADR-0019 D6.1~D6.7
- 산출물: `codebase/backend/crates/http-api/src/session_lock.rs` (Rust `fs2::FileExt`)

### 13.20 BE-NEW-10 Terminal pool list API + multi-attach 추적 + template spawn (신규, ADR-0021 D7 + G36 grilling)
- `GET /api/terminals` — server-wide alive terminals + 각 terminal 의 attach 점 (sessions × panels). Terminal 마다 attach count + attaching session names.
- **`POST /api/terminals { template_id?: string, command?: string[], cwd?: string, env?: { [k]: v }, fresh_spawn?: bool, id?: string }` (G36)**
  - `template_id` 가 있으면 server-side `terminal-templates.toml` 에서 조회 + spawn
  - `template_id` 없고 `command` 있으면 raw spawn (argv direct, no shell)
  - `fresh_spawn=true` + `id` 있으면 *same id 로 새 child* (G25 dangling spawn)
  - Auto-mount: trigger session 의 layout 에만 cascade (ADR-0021 D3)
- **`GET /api/terminal-templates` / `POST /api/terminal-templates` / `PUT /api/terminal-templates/<name>` / `DELETE /api/terminal-templates/<name>` (G36)** — Settings UI 의 CRUD
- ADR-0021 D7, G36
- 산출물: `Terminal` 메타 amend, list endpoint 신규, `crates/http-api/src/terminals/templates.rs` (신규)

### 13.21 BE-NEW-11 v1 → v2 hard cutover migration (신규, ADR-0018 D5)
Boot 시 layout file 의 `schema_version` 검사. v1 발견 시 groups[] 보존 + panels[] 폐기 + items[] = [] + atomic write. info log.
- ADR-0018 D5, ADR-0006 D15
- 산출물: BE-NEW-2 의 boot 흐름 안

### 13.22.5 BE-NEW-12.5 Panel/Terminal close 분리 + same-id fresh spawn (신규, ADR-0021 D9/D10 G25.1 amend)
- `DELETE /api/sessions/<name>/items/<id>?kill_terminal=<bool>` — panel 제거. `kill_terminal=true` 면 terminal 도 SIGTERM (multi-mirror 영향 broadcast).
- `POST /api/terminals/<id>/respawn` (또는 `POST /api/terminals { id, fresh_spawn: true }`) — same id 로 새 child spawn (ADR-0021 D10.1 c2). 기존 id 가 server-pool 에 alive 면 400 reject (idempotency).
- `POST /api/terminals/<id>/kill` — terminal SIGTERM 만 (panel item 들 유지, dangling broadcast).
- WS broadcast: `{ kind: "terminal_died", terminal_id, reason: "exit"|"killed_by_panel_close"|"killed_explicit" }` — 모든 attached webpage 가 그 id 의 panel 들에 dangling overlay 표시.
- ADR-0021 D9/D10
- 산출물: `codebase/backend/crates/http-api/src/sessions/items.rs` (DELETE handler amend) + `codebase/backend/crates/http-api/src/terminals/{respawn,kill}.rs` (신규)

### 13.22 BE-NEW-12 file_path OS-level open + allowlist (신규, ADR-0023, G21 grilling)
- `GET /api/file-path/allowlist-check?path=<abs>` → `{ allowed: bool }`
- `POST /api/file-path/open { path }` — server-side allowlist 매칭 OR one-time nonce 검증 → `Command::new("open"|"xdg-open").arg(canonical_path).spawn()`. shell 비경유, NUL byte 차단, absolute path 강제, canonicalize 적용.
- `POST /api/file-path/allowlist { ext, prefix }` — entry 추가 (confirm modal 의 [✓ Always for] 통과 시).
- `DELETE /api/file-path/allowlist/<id>` — Settings UI 의 entry 삭제.
- Audit log: 모든 open / 거부 시도 NDJSON 으로 `${XDG_STATE_HOME}/gtmux/audit/file-open-YYYYMMDD.log`.
- ADR-0023 D1~D9
- 산출물: `codebase/backend/crates/http-api/src/file_open/{handler,allowlist,spawn,audit}.rs`

## 14. Frontend 기능 명세 (plan-0006 §5 amend + 신규)

### 14.1 FE-1 Auth page (큰 amend by ADR-0020)
- 옛 bootstrap inline script 폐기.
- 새 `/auth` route — token mode (URL query 자동 처리) 또는 password mode (form). 인증 통과 시 cookie 발행 + redirect to `/`.
- ADR-0020 D4/D5/D8
- 산출물: `codebase/frontend/src/routes/auth/*.svelte`

### 14.2 FE-2 Toolbar2 + Tool state (그대로 + plan-0006 §4.2, G22 + G29 + G36 grilling 결과)
- 도구: Select / Hand / **Terminal▾** / Text / Note / Rect / Ellipse / Line / Free draw / Image / Document / File Path (총 12 도구)
- **Terminal 도구의 dropdown (G36)**:
  - click = default template 으로 새 terminal spawn (`POST /api/terminals { template_id }`)
  - icon 우측 `▾` click 또는 long-press → dropdown 의 template list 표시 → 선택 → spawn
  - dropdown 의 [+ Manage templates...] → Settings → Terminal → Templates section 이동
  - 다른 11 도구는 dropdown 없음
- `toolStore`:
  - `currentTool: ToolId`
  - `locked: boolean` — Q toggle 로 켜고 끄기 (G22)
- **One-shot default + Q lock 정책 (G22)**:
  - **Select / Hand 는 *mode* — 항상 sticky** (one-shot 개념 무관, G29 정합).
  - 그 외 10 도구: 1회 사용 후 자동 Select 복귀 (`locked === false` 인 경우).
  - **Q 단축키** 또는 *toolbar 아이콘 long-press* → `locked = true` + 시각 표시 (아이콘 outline 또는 `*`).
  - **Esc**: `locked` 해제 + Select 복귀. modal stack top 우선 (G20.2 정합).
  - Locked 도구는 사용 후에도 그대로 유지 (반복 사용).
- **Space hold + drag = pan modifier (G29)**:
  - 어느 mode 에서든 Space hold + drag = viewport pan. 즉 Select mode 의 사용자도 빠른 pan 가능.
  - Hand mode 와 동일 효과 — 명시 mode 전환 부담 0.
  - Space 해제 시 원래 mode 복귀.
  - Trackpad 2-finger drag 는 native pan (browser/OS 처리, 도구 무관).
- 키보드 단축키 (P1+, G26 grilling)
- 산출물: `codebase/frontend/src/lib/chrome/Toolbar2.svelte` (신규) + `lib/stores/toolStore.svelte.ts` (신규) + Space-pan handler (Canvas 컴포넌트 안)

### 14.3 FE-3 Canvas Item model (amend by ADR-0018)
- TS `CanvasItem` discriminated union — `terminal/text/note/rect/ellipse/line/free_draw/image/document/file_path`
- TypeGuard + 공통 field 헬퍼
- ADR-0018 D1~D4
- 산출물: `codebase/frontend/src/lib/canvas/items/types.ts` (신규)

### 14.4 FE-4 Item Renderers (그대로 + 2026-05-17 amend: style/pattern/rotation 확장 후보 등록)
- `TextNode.svelte`, `NoteNode.svelte`, `ShapeNode.svelte` (rect/ellipse), `LineNode.svelte`, `FreeDrawNode.svelte`, `ImageNode.svelte`, `DocumentNode.svelte`, `FilePathNode.svelte`
- SvelteFlow custom node 패턴
- 산출물: `codebase/frontend/src/lib/canvas/nodes/` (Stage 5)
- **2026-05-17 amend — 보완 기능 확장 후보 (ADR-0018 D2/D4 schema amend 필요, 별 batch 로 land)**:
  - **(a) Text style 풀** — `text` payload 에 옵셔널 5 필드 추가 후보: `font_family?: string` (system stack / mono / serif / sans + free token), `font_weight?: 100~900 | "normal" | "bold"`, `font_style?: "normal" | "italic"`, `text_decoration?: "none" | "underline" | "line-through"`, `line_height?: number` (0.8~2.0). TextNode renderer 가 CSS 매핑 + Inspector v2 의 text section 에 control row. Inline edit (`InlineEditTextarea`) 도 표시 상태와 동일한 font CSS 상속. Default: family = system stack / weight = 400 / style = normal / decoration = none / line-height = 1.4.
  - **(b) Figure stroke/fill 패턴** — `rect / ellipse / line` payload 에 옵셔널 2 필드 추가 후보: `stroke_dash?: "solid" | "dash" | "dot" | "dashdot"` (SVG `stroke-dasharray` 매핑) + `fill_pattern?: "solid" | "none" | "hatch"`. `line` 은 `fill_pattern` 없음 (stroke 만). ShapeNode + LineNode renderer 가 SVG 분기. Inspector v2 의 shape section 에 stroke pattern dropdown + fill pattern dropdown. 별 grad / image fill 같은 복잡 패턴은 P2+ (별 ADR).
  - **(c) Item rotation** — `ItemCommon` (cross-cut) 에 `rotation?: number` (degree, 0~360, default 0) 추가 후보. 모든 visual renderer (PanelNode / TextNode / NoteNode / ShapeNode / LineNode / FreeDrawNode / ImageNode / DocumentNode / FilePathNode / CaptionNode) 의 transform 에 `rotate(${rotation}deg)` 적용 (center 기준). Resize handle 의 외부 +20px 위치에 추가 *rotate grip* (Figma 컨벤션). Snap: 15° 단위 (Shift hold = 자유 회전). Inspector geometry row 에 rotation slider (0~360) + 수치 입력. BBox 계산은 회전 후 axis-aligned bbox 로 — Multi-item bbox resize (G40) 와 정합 필요. SvelteFlow 의 connection point / hover 영역도 회전 정합 (별 sub-test).
- 본 amend (a/b/c) 는 plan-0011 / 0012 후속 batch 후보 — handover-v3 §5 P1 매트릭스 3 신규 row + §6 Stage 5 §7~§9 와 짝. ADR-0018 D2 (ItemCommon) + D4 (text/rect/ellipse/line) schema row 갱신은 본 batch land 시점에 같이 진행 (코드 + serde struct + openapi 정합).

### 14.5 FE-5 Creation gestures (그대로)
- Click-to-create (text, note)
- Drag-to-create (rect/ellipse/line)
- Pointer capture (free draw)
- Cancel on Esc, min size threshold
- Inline edit start
- 산출물: 각 도구 핸들러

### 14.6 FE-6 Layer list V2 (amend + ADR-0021 D7 + ADR-0024 + G25 grilling)
- 기존: Group tree + Terminal Panel + Canvas Item unified
- 신규: **Terminal pool section** (server-wide alive terminals, 각 attach 점 표시)
- **상단 toggle [Tree | Z] (G24, ADR-0024 D4)**:
  - Tree 모드 (default): group 계층 tree + 각 row 에 z badge (small, `z:10`) + drag reorder/reparent = *organization 만* 변경 (z 영향 X)
  - Z 모드: flat 정렬 (z 내림차순, 위가 canvas 최전면). read-only — drag reorder 비활성. group label hint 만 표시.
- Reorder/reparent drag (Tree 모드만), group collapse, invisible/lock toggle, rename (G23 의 `InlineEditField`), type icon, z badge (always 표시)
- **Group row context menu (G25)**:
  - [Ungroup] (비파괴, ADR-0010 D12 — confirm 없음) — group 만 제거, 자손은 parent_id 승격
  - [Delete group] (파괴, ADR-0010 D10 amend, ADR-0021 D9.3) — confirm modal `GroupCloseConfirmModal.svelte` (신규) bulk 1 dialog:
    - 옵션: [Cancel] / [Panels only] (terminal pool 유지) / [Panels + Terminals] (자손 terminal 모두 SIGTERM)
    - 자손 list (terminal panel + non-terminal item 분리, mirror 횟수 hint)
    - `Settings.behavior.auto_kill_terminal_on_panel_close = true` 시 dialog 없이 [Panels + Terminals] 즉시
- 산출물 (ADR-0017 2026-05-16 ②/③ amend 후 — chrome 어휘 재정의): `codebase/frontend/src/lib/sidebar/LeftPanel.svelte` (chrome owner, 신규 — 가로 탭 `[Layers | Terminals]` + 28px collapsed rail with tab icons) + `codebase/frontend/src/lib/sidebar/LayerTreeView.svelte` (구 `Sidebar.svelte` rename, layer tree content) + `codebase/frontend/src/lib/sidebar/TerminalListView.svelte` (Terminals tab content, 신규) + `codebase/frontend/src/lib/chrome/GroupCloseConfirmModal.svelte` (신규). `chromeStore.leftPanelTab: 'layers' | 'terminals'` 추가, `setLeftPanelTab(tab)` action 으로 panel 확장 + 탭 선택.

### 14.7 FE-7 Panel header/footer V2 (amend + ADR-0021 D8 + ADR-0024 D2 + G25.1 grilling)
- 기존 header redesign (title/id/status/Input Target marker, minimize/maximize/invisible/close)
- 신규 header more menu (…):
  - **Z mutation 4 액션 (ADR-0024 D2)**:
    - ▲ Bring to front (Shift + `]`, P1+ 단축키)
    - ▼ Send to back (Shift + `[`, P1+)
    - ↑ Bring forward (`]`, P1+)
    - ↓ Send backward (`[`, P1+)
  - **[Change terminal...]** → Terminal pool modal
  - **[Kill terminal]** (별 액션, ADR-0021 D9.4) — terminal SIGTERM 만, panel item 들은 모두 dangling 상태로 (모든 attached session)
  - **[Remove panel]** (multi-mirror dangling 의 alternative) — 그 session 의 panel 만 제거 (terminal 영향 X)
  - Rename / Settings
- **Panel close 버튼 (header 의 X)** — G25.1 grilling 결과:
  - 클릭 시 confirm dialog `PanelCloseConfirmModal.svelte` (신규):
    - 옵션: [Cancel] / [Panel only] / [Panel + Terminal]
    - Hint: 그 terminal 이 mirror 된 다른 session 이름 목록 (또는 "Only here")
    - [Panel + Terminal] 옵션 옆 ⚠ 아이콘 (multi-mirror 경우 강조)
  - `Settings.behavior.auto_kill_terminal_on_panel_close = true` 시 dialog 생략 + [Panel + Terminal] 즉시
- **Dangling overlay (ADR-0021 D10.1, G25.1.b c2)**: terminal SIGTERM 시 panel 위에 `[exit code N] — Click to restart` overlay. 사용자 *focus / click / input* → `POST /api/terminals` (same id, fresh_spawn=true) → xterm 재attach + toast "Terminal restarted".
- Canvas right-click context menu 도 동일 4 z 액션 + [Kill terminal] + [Remove panel] + [Close panel...] 노출.
- Footer description collapsible
- **Panel Settings modal (per-panel, G35)**:
  - General: title (= label), description, visibility, lock, size (numeric input)
  - **Terminal Override section (terminal type 만)**:
    - [✓ Override global font size] : 13 (slider 10~24)
    - [✓ Override line wrapping] : ✓/✗
    - [✓ Override scrollback] : 1000 (dropdown)
    - [✓ Override cursor style] : block/underline/bar
    - (각 항목 체크 안 하면 global default 사용)
  - Danger zone: [Close panel...] (PanelCloseConfirmModal 동등)
- 산출물: `codebase/frontend/src/lib/canvas/PanelNode.svelte` (큰 amend) + `lib/canvas/ContextMenu.svelte` (신규 또는 amend) + `lib/canvas/PanelCloseConfirmModal.svelte` (신규) + `lib/canvas/PanelDanglingOverlay.svelte` (신규) + `lib/stores/zStore.svelte.ts` (신규)

### 14.8 FE-8 Settings UI (amend by ADR-0020 D5, G19 grilling 결과)
- **Form factor**: full-screen overlay (route 아님, URL 변경 없음) + 좌측 sidebar nav (Auth / Theme / Shortcut / Storage / Debug section)
- **Open**: Titlebar 의 SessionMenu (≡) → "Settings...", canvas state 그대로 유지
- **Close**: Esc / X 버튼 / Cancel — *outside click 은 닫지 않음* (실수 방지). dirty state 처리는 *auto-save* 이므로 confirm 불필요.
- **Commit 정책 (G19.1)**: 즉시 자동 저장 (no global Save/Cancel)
  - Toggle / select / number: 0.5s debounce → 자동 PATCH → toast "saved"
  - Free text input: 1s debounce + blur 시 commit
  - Destructive action (Reset config / Logout all sessions): 즉시 PATCH X — [버튼 + confirm modal]
  - Multi-field form (password change: current + new + confirm): 별 panel + [Change password] 버튼 (form-group 단위 commit)
  - Boot-immutable (workspace_path, port): read-only + "다음 재기동에 적용" hint
- **Section**:
  - **Auth**: token rotate, password change/setup
  - **Theme** (G27): `[System | Light | Dark]` radio, chrome+xterm 동기
  - **Shortcut** (G26): read-only list, 카테고리별 (Editing/Layer-Z/Tool/Application), P3 customization 비범위
  - **Terminal (G35) ⭐**: global default (모든 terminal 적용, per-panel override 가능)
    - Font size: 13px (slider 10~24)
    - Line wrapping: ✓ (default on)
    - Scrollback: 1000 lines (dropdown 100/500/1000/5000/10000)
    - Cursor: block / underline / bar (radio) + blink ✓
    - Bell: visual / none / sound
    - Copy on select: ✗ (default off)
    - Right-click 동작: context menu (default) / paste
  - **Templates (G36) ⭐**: Shell preset CRUD
    - Default 4 preset (boot 시 자동 생성, 사용자 삭제 가능):
      - `bash` (`/bin/bash` 또는 `$SHELL` system default)
      - `zsh` (`/bin/zsh`)
      - `python` (`python3 -i`)
      - `htop` (`htop`)
    - 사용자 추가 entry: `{ name, command: [...], cwd?, env?, description? }`
    - Default template 지정 가능 (radio) — Toolbar [Terminal] click 시 사용
    - Storage: `${XDG_CONFIG_HOME}/gtmux/terminal-templates.toml`
  - **Storage**:
    - workspace path (read-only)
    - file_open allowlist editor (ADR-0023, entry list + [Delete])
    - **Session export/import (G28)**:
      - [Export this session...] → 활성 session 의 JSON + meta → Blob download (`<name>-YYYYMMDD-HHmmss.gtmux.json`)
      - [Import session...] → file picker (`.gtmux.json` accept) → schema validate → 이름 conflict 시 dialog [Rename / Override / Cancel] → 새 session record 생성
  - **Behavior** (G25.1): `auto_kill_terminal_on_panel_close: bool` default `false`
  - **Debug**: server pid, build sha, log path
- 산출물 (ADR-0017 2026-05-16 ④ amend 으로 chrome 부분 ship): `codebase/frontend/src/lib/chrome/SettingsOverlay.svelte` (신규) + `codebase/frontend/src/lib/stores/settingsDialog.svelte.ts` (store: `open` / `section` / `show(section?)` / `close()` / `toggle()` / `setSection()`). 진입점 3개 — SessionMenu "Settings…" / `Cmd+,` (shortcutRegistry) / `settingsDialog.show()` 직접 호출. Theme · Shortcuts section 은 ④ amend 으로 wire 완료, Storage / Auth / Behavior / Debug / Terminal / Templates section 은 BE wire 시 별 amend (현재 placeholder + "Waiting on BE: ..." 명시).

### 14.9 FE-9 Viewport sync UI (**재정의** by ADR-0019, ADR-0021 D5 + G33 grilling)
- 옛 server-authoritative viewport 폐기. 새: viewport = session layout 의 일부, attached webpage 와 양방향 sync.
- ViewportCtrl 확장: zoom in/out, reset 100%, fit all, fit selected, go to selected, selection count, sync indicator
- **UI 위치 (G33)**: Statusbar 우측 cluster — `[Selection count | zoom level / 100% | -| +| fit all | fit selected | go to selection | sync indicator]`
- **Zoom 범위 (G33)**: 25% ~ 200% (Figma 표준). 25% 미만 / 200% 초과 시도 시 clamp.
- **Mouse/Trackpad 입력**:
  - mouse wheel = vertical pan / Shift+wheel = horizontal pan / Cmd+wheel = zoom step ±10%
  - trackpad 2-finger drag = native pan / pinch = native zoom (browser/OS 처리)
- **단축키 (P0/P1, G26+G33 정합)**:
  - P0: `Cmd+0` reset 100% / `Cmd+1` fit all / `Cmd+2` fit selected
  - P1 (Stage 7+): `Cmd+.` go to selection / `Cmd+=` zoom +10% / `Cmd+-` zoom -10%
- **Edge cases**:
  - `fit all` + empty layout → reset 100% 동작 (no-op effect)
  - `fit selected` + 0 selection → toast "No selection"
  - `go to selection` + multi-selection → bounding box centering
- 산출물: `codebase/frontend/src/lib/chrome/ViewportCtrl.svelte` (amend)

### 14.10 FE-10 UX polish (그대로 + G27 grilling + Tier 3 amend + 2026-05-16 attach recovery layer 분리 amend)
- Shutdown / new panel 위치 재정렬
- Left/right unfold rail gap 동일화
- Responsive behavior
- **Server shutdown confirm modal (Tier 3 amend; ✅ ship — `ShutdownModal.svelte` 으로 rename)** — Cmd+Shift+Q (G26 P1) 또는 SessionMenu (≡) 의 [Shutdown server...] 클릭 시 confirm modal `lib/chrome/ShutdownModal.svelte` (구 `ServerShutdownConfirmModal` rename — 어휘 단순화):
  - "Shut down gtmux server? All sessions will be detached. Layouts are saved (workspace dir) — restart to reattach. N terminal(s) will be killed."
  - [Cancel] / [Shut down]
  - Confirm → `POST /api/shutdown` (또는 BE 의 graceful endpoint) → ADR-0014 D7 / D12 의 graceful teardown
- **WS auto-reconnect (Tier 3 amend, sketch §7.4 D21 c2·c3; ✅ ship — `lib/ws/client.ts` 안 통합)** — heartbeat (15s ping / 30s timeout) 후 WS close 감지 시 transport-level 자동 reconnect:
  - 1s grace → exponential backoff: 1s / 2s / 4s / 8s / 16s / cap 30s (`BACKOFF_MS` array 기반)
  - 누적 attempt 카운터 → banner 가 "(attempt N)" 표기 (`connectionStore` 의 state 노출)
  - 인증 실패 (1008/1011/4001 token rotated) 도 일단 재시도 — backoff 가 늘어날 뿐, 영구화 시 `ConnectionStore` 의 카운터로 UI 가 안내
  - `lib/ws/client.ts` 의 `ConnectionState` 머신 (`connecting / open / closing / closed / reconnecting`) 으로 정합
  - **별 file 분리 안 함** — 옛 계획의 `lib/ws/reconnect.svelte.ts` 는 채택 X. `client.ts` 의 `#scheduleReconnect()` 내부 메서드로 통합
  - 별 store: `lib/ws/heartbeat.svelte.ts` — frame timestamp (`lastFrameAt`) + activity timestamp (`lastActivityAt`) + `isStale` / `isIdle` derived (Phase 2 silent reattach 의 보조 신호)
- **Session attach recovery (별 layer, 2026-05-16 amend — ADR-0019 D5.1 + D5.4)** — *transport-level* WS reconnect 와 *application-level* attach 복구 의 분리:
  - 본 §14.10 의 WS auto-reconnect 는 *transport* 만 — TCP/WS 재연결만 보장. BE 의 `session_locks_by_cookie` 가 heartbeat 30s timeout 으로 release 됐다면 *attach 상태 복구는 별 layer* 가 처리.
  - 그 별 layer = **`plan-0008-session-attach-recovery-impl.md`** + ADR-0019 D5.1 (Case II silent reattach + mutation guard) + D5.4 (Case I initial entry blocking ReconnectModal). 구현 = `reconnectGate.svelte.ts` (8-state) + `ReconnectModal.svelte` (4 mode) + `sessionStorageHint.ts` + `sessionStore.{attemptReattach, silentReattach, ensureMutationOk}`.
  - Trigger 합집합 (Phase 2 = Case II): `dispatcher.svelte.ts` 의 WS `reconnecting → open` 전이 + `+page.svelte` 의 `visibilitychange === 'visible'` — 본 §14.10 의 transport reconnect 가 *완료된 후* 별 fire.
  - 재연결 후 자동 cookie 검증 + Dialog 진입 → ~~"이미 attach 됐다면 그 session 으로 자동 복귀"~~ 의 옛 정책은 plan-0008 의 *blocking ReconnectModal* (Case I) + *silent reattach + mutation guard* (Case II) 의 두 path 로 정합 — 단순 "자동 복귀" 가 아닌 *결과 분기 (200/409/404/401/5xx) + UX 명시* 의 형태.
  - 산출물: ~~`lib/ws/reconnect.svelte.ts` (신규)~~ → 본 §14.10 의 transport 는 `client.ts` 통합 / attach recovery 의 별 layer 산출물은 plan-0008 §4 inventory 참조.
- **Theme adapter (G27, P1; ADR-0017 2026-05-16 ④ amend + ④ follow-up 으로 chrome+xterm ship)** — light + dark 2 fixed theme + system detect + chrome/xterm 동기:
  - `lib/stores/theme.svelte.ts` (신규/amend, 파일명 `theme.svelte.ts` — `themeStore` 의 store 이름은 보존) — `mode: ThemeMode = "system"|"light"|"dark"` ($state, user choice) + `resolved: Theme = "light"|"dark"` ($derived). System 모드는 `window.matchMedia('(prefers-color-scheme: dark)')` listener (`bindSystemListener()` — `+page.svelte` onMount 에서 호출, onDestroy cleanup). localStorage `gtmux-theme` schema = `'system'|'light'|'dark'` (이전 `'light'|'dark'` 와 graceful fallback).
  - `lib/xterm/xtermTheme.ts` (신규, ADR-0017 ④ follow-up — 경로 `lib/xterm/` 정합) — light/dark 2 fixed xterm theme 객체 (ANSI 16 + bg/fg/cursor/selection, light/dark variants). XtermHost mount 시 `xtermTheme(themeStore.resolved)` 적용 + 별도 $effect 로 hot reload (theme flip 시 live `term.options.theme` 교체). xterm-host 컨테이너 background 도 `--canvas-bg` 로 변경 (light 모드 black flash 방지).
  - CSS variable: `:root[data-theme="light"]` / `[data-theme="dark"]` — chrome 컴포넌트 모두 이 var 참조
  - Settings → **Theme section**: `[System | Light | Dark]` radio. 변경 즉시 `data-theme` attr + 모든 xterm 인스턴스의 `terminal.options.theme = newTheme` (xterm.js 5.x API)
  - System 모드는 OS dark mode 변경 자동 반영 (MediaQueryList change event)
- Visual regression target 정리
- 산출물: 분산 + 위 신규 store / util

### 14.11 FE-11 Tests (그대로)
- Vitest + @testing-library/svelte
- Store / primitive / chrome / panel state tests
- Playwright E2E (Stage 10)
- 산출물: `codebase/frontend/tests/` (신규)

### 14.12 FE-NEW-1 Session UI (신규, ADR-0019 D7/D8/D9, G18 grilling)
- `AuthDialog.svelte` (인증 후 [새 / 기존] 선택)
- `NewSessionModal.svelte` (이름 입력 + Create + 중복 reject)
- `SessionListModal.svelte` (Available / In use 섹션, "in use by server-pid X" badge + disabled)
  - **Lock state polling (ADR-0019 D6.4)**: modal open 동안 1s 주기로 `GET /api/sessions` 재호출 → row state 갱신. modal close 시 polling 중단. 다른 webpage close 후 ~1s 내에 row 자동 enable.
- `SessionMenu.svelte` (Titlebar 의 ≡ 드롭다운 amend — Switch session... / Logout)
- `ActiveSessionDropdown.svelte` (Toolbar 우측 현재 session 표시 → 클릭 시 SessionListModal)
- **Delete session 액션 (ADR-0019 D10/D10.1, 2026-05-17 amend, G51)**:
  - SessionListModal 의 `Available` row 우측 hover-kebab [⋯] → [Delete session…] popover → ConfirmModal ("Delete session '<name>'? (Terminal 들은 server-pool 에 남음)") → 승인 시 `deleteSession(name)` (`lib/http/sessions.ts:115` 기존 wrapper). *In use* row 와 본 webpage 의 active row (= `sessionStore.activeName`) 는 kebab 표시 X (각각 다른 webpage 의 작업 침해 차단 + 본 entry 는 SessionMenu 가 own).
  - SessionMenu 의 "Delete current session…" item (Logout 아래 / Shutdown server… 위) → 같은 ConfirmModal → 승인 시 `deleteSession(activeName)` → `sessionStore.clear()` + `reconnectGate.cancel()` + `sessionStorageHint.clear()` + `workspaceSwitcher.open()` (D5.4 cancel 흐름 + D10 의 "현 attached 였으면 dialog 회귀" 정합).
  - BE 변경 0 — `DELETE /api/sessions/<name>` (BE-NEW-2) 이미 ship.
- 산출물: `codebase/frontend/src/lib/chrome/` 안 신규 5건 + Delete UI 의 SessionListModal/SessionMenu amend (D10.1)

### 14.13 FE-NEW-2 Webpage attach lifecycle (신규, ADR-0019 D3 + ADR-0021 D6)
- Cookie 기반 자동 재인증 (reload 시 Auth page 건너뛰고 Dialog)
- Heartbeat 의 client-side 는 자동 (브라우저 WS API 의 PONG)
- Single-attach 충돌 시 (409) modal 의 그 session 을 즉시 disabled 로 + toast
- `beforeunload` 시 `navigator.sendBeacon('/api/leave')` (best-effort)
- 산출물: `codebase/frontend/src/lib/auth/lifecycle.ts` (신규)

### 14.14 FE-NEW-3 Terminal pool UI (신규, ADR-0021 D7 + 2026-05-16 ② amend)
- LeftPanel 의 두 번째 탭 "Terminals" (ADR-0021 D7 amend ②, ADR-0017 ②와 짝) — server-wide alive terminals
- 각 row: id (short), label, status, attach 수 + attached session names
- 액션: [Attach to canvas] (현 session 에 panel 추가), [Kill terminal]
- 산출물: `lib/sidebar/TerminalListView.svelte` (FE-6 §14.6 의 LeftPanel 안 두 번째 탭 content view — outer chrome 은 LeftPanel 단독 owner)

### 14.15 FE-NEW-4 Terminal binding UI (신규, ADR-0021 D8)
- Panel context menu (header 의 more 버튼 또는 right-click) 의 [Change terminal...]
- Modal popup — server-wide terminal list (filter, search)
- 선택 → `PUT /api/sessions/<name>/items/<id>/terminal { terminal_id }` → xterm subscriber 교체
- 산출물: `PanelNode.svelte` (amend) + `ChangeTerminalModal.svelte` (신규)

### 14.16 FE-NEW-5 Match-or-spawn confirm dialog (신규, ADR-0018 D6)
- Session attach 시 backend 의 attach 응답에 unmatched count 포함
- count > 0 시 modal: "Attach session 'X'? Will spawn N new terminal(s). Continue?" + [Cancel] [Confirm]
- Confirm → backend 가 spawn 진행 + layout 받음
- 산출물: `AttachConfirmModal.svelte` (신규)

### 14.17 FE-NEW-6 Multi-xterm subscriber (신규, ADR-0021 D1)
- 한 terminal_id 가 여러 panel 에 attach 됐을 때 각 panel 의 xterm 인스턴스가 같은 broadcast stream subscribe
- xterm 인스턴스 lifecycle 은 panel 단위. 한 panel 의 dispose 가 다른 panel 의 xterm 에 영향 안 줘야.
- 산출물: `codebase/frontend/src/lib/canvas/PanelNode.svelte` (xterm subscriber 패턴 amend)

### 14.18 FE-NEW-7 Session-scoped store 분리 (신규, ADR-0019 + ADR-0021 D5)
- 현 `panels.svelte.ts` / `mux.svelte.ts` 의 server-wide single state → session-scoped (active session 의 layout state 만 보유)
- Session switch 시 store reset + new layout load
- M / I / Viewport / Focus 도 session 단위
- 산출물: `codebase/frontend/src/lib/stores/sessionStore.svelte.ts` (신규) + 기존 store 들 amend

### 14.19 FE-NEW-8 file_path open UX + Settings allowlist editor (신규, ADR-0023, G21 grilling)
- `FilePathItem.svelte` (신규) — Canvas Item 의 file_path type 컴포넌트
  - Double-click handler → `GET /api/file-path/allowlist-check` → allowed 면 즉시 `POST /api/file-path/open`, 아니면 confirm modal
  - Single-click = item selection (다른 item 과 일관)
- `FileOpenConfirmModal.svelte` (신규) — path + [✓ Always for *.{ext} within {prefix}/] 자동 추론 체크박스 + [Cancel] [Open]
  - Always 체크 → `POST /api/file-path/allowlist` + `POST /api/file-path/open`
  - 미체크 → `POST /api/file-path/open?one_time=1`
- `SettingsOverlay` 의 **Storage section** 안 allowlist editor — entry 표 + [Delete] (G19.1 auto-save)
- Failure UX: spawn 실패 시 toast, path stale (canonicalize 실패) 시 visual indicator
- 산출물: `lib/canvas/items/FilePathItem.svelte`, `lib/canvas/items/FileOpenConfirmModal.svelte`, `lib/chrome/SettingsOverlay/StorageSection.svelte`

### 14.20 FE-UX-Common — 공용 UX 운영 규칙 (G20 / G22 / G23 grilling 합본)

이 sub-section 은 *공용 컴포넌트와 정책의 single-source-of-truth*. 모든 FE 컴포넌트가 이 규칙을 따른다.

#### 14.20.1 Inline edit (G23)
- **공용 컴포넌트**:
  - `lib/common/InlineEditField.svelte` — single-line (Panel label / Group label / Layer row / Note title)
  - `lib/common/InlineEditTextarea.svelte` — multi-line (Note body, Text content)
- **키 매핑 (single-line)**: Enter → commit, Esc → cancel (원래 값 복원), blur (outside click / Tab) → commit
- **키 매핑 (multi-line)**: Cmd/Ctrl-Enter → commit, Enter → newline, Esc → cancel, blur → commit
- **Validation 실패** 시 빨간 hint + 키 비활성. Empty single-line 은 cancel 효과 (원래 값 복원). Empty multi-line 은 빈 string 허용.

#### 14.20.2 Esc 키 라우팅 우선순위 (G20 + G22 + G23 합본)
Esc 입력 시 *위에서 아래로* 검사하고 첫 매치만 발동:

1. **Inline edit 활성** → 그 inline edit 의 cancel (다른 layer 영향 X)
2. **Modal stack top** (Settings overlay / SessionListModal / NewSessionModal / FileOpenConfirmModal / AttachConfirmModal / Confirm dialog) → 그 modal close
3. **Panel maximize 활성** → unmaximize (G20)
4. **Tool locked** (G22) → lock 해제 + Select 복귀
5. **Tool 비-Select 인 상태** (one-shot 미사용 중) → Select 복귀
6. **Selection 있음** → selection clear
7. (그 외) — no-op

이 우선순위는 *focus 위치* + *modal stack* 의 자연 추론. 라우터 구현은 `lib/common/escRouter.svelte.ts` (신규).

#### 14.20.3 Toolbar 도구 정책 (G22)
- Default: 모든 도구 one-shot — 사용 후 자동 Select 복귀.
- Q 단축키 (또는 toolbar 아이콘 long-press) = 현 도구 lock sticky.
- Esc = lock 해제 + Select 복귀 (위 §14.20.2 의 4번).
- Select / Hand 는 mode 이므로 always sticky.

#### 14.20.4 Maximize (G20)
- Canvas viewport area fill (Titlebar/Toolbar/Status bar 유지).
- FE-only ephemeral (schema 영속 안 함).
- 한 시점 1 panel 만 maximize — toggle 시 다른 max 자동 해제.
- Unmaximize: Esc (위 §14.20.2 의 3번) / 헤더 toggle 버튼 / panel header double-click.

#### 14.20.5 Keyboard shortcut 정책 (G26 grilling)

##### 14.20.5.1 Layer 우선순위 (G26.1 — Hybrid)
- **Modifier shortcut** (Cmd/Ctrl/Shift + key): xterm focus 포함 *어디서든* 발동.
- **Single-key shortcut** (`]` `[` `Q` `Enter` 등): **xterm focus 외에만** 발동. xterm focus 시 shell typing 으로 전달.
- **Esc 의 특수 처리 (G26.2 — Option i)**:
  - xterm focus 시 Esc → 항상 shell 의 `\x1b` 로 전달 (vim/less/htop 자연).
  - 그 외 focus 시 Esc → `escRouter` (§14.20.2 의 7 우선순위).
  - Modal stack 활성 시는 focus trap 으로 *xterm focus 자체 없음* — 자연 escRouter 진입.

##### 14.20.5.2 P0 shortcut 매트릭스 (Stage 1~6 필수, G26.3)

| Shortcut | 동작 | 출처 |
|---|---|---|
| `Esc` | escRouter (§14.20.2 의 7 우선순위) | G20+G22+G23 합본 |
| `Enter` | Inline edit commit (single-line) | G23 |
| `Cmd/Ctrl+Enter` | Inline edit commit (multi-line) | G23 |
| `Q` | Tool lock toggle | G22 |
| `]` | Bring forward (z++) | ADR-0024 D2 |
| `[` | Send backward (z--) | ADR-0024 D2 |
| `Shift+]` | Bring to front (z = max+1) | ADR-0024 D2 |
| `Shift+[` | Send to back (z = min-1) | ADR-0024 D2 |
| `Space hold + drag` | Viewport pan (Select mode 의 사용자도 빠른 pan, Hand mode 와 동일 효과) | G29 |
| `Delete` / `Backspace` | Remove selected items (`sessionStore.M` 일괄, BE `DELETE /items/:id?kill_terminal=false`). terminal item 은 panel 만 제거 — pool 유지 (G25 default). xterm/editable focus 시 무시 → shell 입력. SvelteFlow builtin `deleteKey` 는 `null` 로 비활성 — store-derived `nodes` 와 미동기 회귀 회피. | Figma 컨벤션 + G25 |
| `Cmd/Ctrl+D` | Duplicate selected items (terminal = mirror, non-terminal = deep copy, (20,20) offset cascade) | G32 |
| `Cmd/Ctrl+0` | Viewport reset 100% | G33 |
| `Cmd/Ctrl+1` | Viewport fit all | G33 |
| `Cmd/Ctrl+2` | Viewport fit selected | G33 |
| **`Cmd/Ctrl+A`** ⭐ | **Select all — focus 4 모드 분기**: canvas = `sessionStore.M` 에 active session 의 *visible* item 일괄 (locked 포함, hidden 제외 — Layer tree visibility 정합) / LayerTreeView = 모든 row (group + item) M / xterm = shell select-all (xterm v6 default) / editable (`InlineEditField` / textarea / input) = OS-default select-all | **ADR-0017 D6 amend ⑤ (2026-05-17)** |
| **`Cmd/Ctrl+C`** ⭐ | Copy selected items (multi-select 시 array). canvas focus + M.size ≥ 1. xterm/editable focus 시 OS-default copy 로 routing. | **ADR-0030 D5 cross-link** |
| **`Cmd/Ctrl+X`** ⭐ | Cut (locked item 제외). undo 시 cut 된 item 복귀 + clipboard 변경 없음 (Figma). | **ADR-0030 D5 cross-link** |
| **`Cmd/Ctrl+V`** ⭐ | Paste — (24,24) offset cascade (Figma), 다중은 bounding-box top-left 기준 상대 위치 보존. Terminal item 은 *Clone (default) — 새 terminal spawn*. | **ADR-0030 D5 cross-link** |
| **`Cmd/Ctrl+Z`** ⭐ | Undo (active session 의 mutation stack, capacity 50). editable focus 시 브라우저 OS-default undo 로 routing (앱 undo 발동 안 함). | **ADR-0028 cross-link** |
| **`Cmd/Ctrl+Shift+Z`** ⭐ | Redo (active session 의 redo stack, capacity 50). editable focus 시 OS-default redo 로 routing. | **ADR-0028 cross-link** |

##### 14.20.5.3 P1 shortcut 매트릭스 (Stage 7+, G26.3)

| Shortcut | 동작 | 출처 |
|---|---|---|
| `Cmd/Ctrl+N` | New Terminal (trigger session 의 layout 에 cascade mount) | plan-0006 §6 |
| `Cmd/Ctrl+Shift+L` | LeftPanel toggle (Layers / Terminals 탭 보유, ADR-0017 2026-05-16 ② amend) | plan-0006 §6 + ADR-0017 ④ |
| `Cmd/Ctrl+Shift+I` | RightPanel toggle (현재 단일 Inspect 탭, ADR-0017 2026-05-16 ③ amend) | ADR-0017 ④ |
| `Cmd/Ctrl+Shift+Q` | Server shutdown (confirm modal) | plan-0006 §6 |
| `Cmd/Ctrl+,` | Settings overlay open | G19 표준 + ADR-0017 ④ |
| `Cmd/Ctrl+.` | Viewport go to selection | G33 |
| `Cmd/Ctrl+=` / `Cmd/Ctrl+-` | Viewport zoom ±10% | G33 |
| `Cmd/Ctrl+Shift+F` | Focus mode toggle (M 외 dim, ephemeral) | G34 |
| `Cmd/Ctrl+K` | Command palette (P1+ 후보) | TBD |

##### 14.20.5.4 비범위 (P3, G26.3 + 2026-05-17 ADR-0017 D6 amend ⑤ 합본)
- 사용자 customization (rebind UI) — 비범위.
- Chord shortcut (Cmd+K → Cmd+S 같은 2단계) — 비범위 (Cmd+K command palette 가 P1+ 후보 시 검토).
- **OS-standard 의도적 제외 (ADR-0017 D6 amend ⑤)**:
  - `Cmd/Ctrl+S` — Auto-save 정합 (G19). Layout / Settings mutation 모두 debounced PUT — 사용자 액션 불필요.
  - `Cmd/Ctrl+P` — Print 비범위.
  - `Cmd/Ctrl+W` — Tab close, browser default 우선.
  - `Cmd/Ctrl+R` — Reload, Session attach recovery (ADR-0019 D5.1 / D5.4) 가 자연 처리.
  - `Cmd/Ctrl+Tab` — App / tab switch, OS / browser 영역.
- **Find / search (`Cmd/Ctrl+F`) — P2 deferred** — 별 ADR 후보, `Cmd+K` command palette (§14.20.5.3 TBD) 와 분기 검토. 본 amend scope 외.

##### 14.20.5.5 Discoverability
- **Settings overlay (G19) 의 Shortcut section** — read-only list, 카테고리별 (Editing / Layer/Z / Tool / Application).
- **Tooltip 옆 단축키 표시** — 메뉴 항목, 헤더 more menu, context menu 등 모두 단축키 hint 부착 (예: "Bring to front  ⌘⇧]").
- **OS 차이 처리**: Mac → `⌘` / `⇧` / `⌥` glyph 표시, Win-Linux → `Ctrl` / `Shift` / `Alt`. Browser `navigator.platform` 으로 detect. 라우팅은 *같은 key code* (Mac Cmd / Win Ctrl 의 modifier 매핑은 platform-agnostic 으로).

##### 14.20.5.6 구현 디테일
- **공용 컴포넌트** (ADR-0017 2026-05-16 ④ amend 으로 ship): `lib/keyboard/shortcutRegistry.svelte.ts` (신규 — 경로 `lib/keyboard/` 정합)
  - 전역 `keydown` listener (window-level)
  - xterm focus 검사 (`document.activeElement` 가 xterm container 안인지) + editable focus 가드 자동 (modifier 있으면 default `true`, plain key 는 default `false`, 호출자 override 가능)
  - Modifier + key code 매칭
  - Modal stack 정합 (modal 활성 시 *focus trap* 이 자동 동작 — 별 라우팅 분기 필요 없음)
  - 첫 consumer: `lib/keyboard/chromeShortcuts.svelte.ts` (`Cmd+Shift+L` / `Cmd+Shift+I` / `Cmd+,`). `zShortcuts` 도 registry consumer 로 마이그레이션 완료 (직접 listener 폐기)
  - **Esc 는 `escRouter`** (lib/common/escRouter.svelte.ts) 가 별도 — Esc 의 priority chain (inline-edit > modal > unmaximize > tool > select) 이 flat keycombo table 로 자연 매핑되지 않아 두 dispatcher 가 협조 (Esc → escRouter / 그 외 → shortcutRegistry)
- Stage 6 까지: P0 shortcut 만 등록. P1 은 Stage 7 의 Settings UI 와 같이 추가.
- 테스트: xterm focus 시 `]` 가 shell ESC 로 전달 되는지 + xterm 외 focus 시 `]` 가 Bring forward 발동되는지 unit test.

#### 14.20.6 Resize 정책 (G37~G40 grilling, panel resize 의 type 별 semantic SoT)

##### 14.20.6.1 Terminal Panel resize (Sprint 7 이미 구현, plan SoT 명시)
- SvelteFlow **`NodeResizer`** — corner + edge handles
- `onResizeEnd(event, params: { x, y, width, height })` → `panelsStore.resizePanel(id, x, y, w, h)` (top-/left-anchored handle 이 x/y 도 갱신)
- 내부 xterm 의 **`ResizeObserver` + 150ms debounce → `fitAddon.fit()` → `encodePaneResize(paneId, cols, rows)` WS frame** (FRAME_TYPE 0x04) → backend `PANE_RESIZE` → PTY `winsize` ioctl
- 산출물 (이미 있음): `PanelNode.svelte`, `XtermHost.svelte`, `panels.svelte.ts`

##### 14.20.6.2 Non-terminal Item resize semantic (G37~G40)

| Item type | Resize 방법 | 비고 |
|---|---|---|
| `text` | NodeResizer 표준 frame resize | width 변경 시 text wrap, font_size 는 별 설정 |
| `note` | NodeResizer 표준 | title + body, wrap 적용 |
| `rect`, `ellipse` | NodeResizer 표준 | stroke/fill 그대로, frame 만 |
| `document`, `file_path` | NodeResizer 표준 | string metadata, frame 만 |
| **`line` (G37)** | **별 LineNode — 양 endpoint 별 handle, 각 점 독립 drag (Figma 표준)** | NodeResizer 미사용. payload `(x, y, x2, y2)` 직접 갱신 |
| **`free_draw` (G38)** | **NodeResizer + bbox corner drag = 모든 points 비례 scale (Figma 표준)** | drawing visual 크기 변경 |
| **`image` (G39)** | **NodeResizer + Shift+drag = aspect ratio lock (Figma 표준)** | default 자유, Shift modifier 로 lock |

##### 14.20.6.3 Multi-item resize (G40)

- M (multi-selection) 의 2개 이상 items 선택 시: 통합 bounding box 의 corner drag handle 표시
- Drag 시 **bounding box scale** — 모든 selected items 의 위치 + 크기 *비례 scale* (Figma / Sketch / Illustrator 표준)
- relative resize (각 item 좌표 유지, 크기만 변경) 는 **비채택** — 직관 어색
- 단일 selection 시는 type 별 resize (§14.20.6.2)

##### 14.20.6.4 Group resize
- **MVP 미지원** (ADR-0010 D9 그대로). Group 자체는 frame 없음 — pure organization.
- Group 의 자손 일괄 이동은 D8 (drag delta 모든 자손에 적용, effective locked 제외) 그대로.

##### 14.20.6.5 Minimum size constraints

| Item type | Minimum (w, h) |
|---|---|
| terminal | 120 × 60 (header bar + xterm 최소 가시) |
| text, note | 80 × 40 (텍스트 최소 가시) |
| rect, ellipse | 20 × 20 |
| line | 길이 5 (endpoint 사이) |
| free_draw | bounding box 의 longest axis ≥ 20 |
| image, document, file_path | 60 × 40 |

Minimum 도달 시 resize handle 의 drag 가 멈춤 (constraint enforcement, NodeResizer 의 `minWidth` / `minHeight` 옵션).

##### 14.20.6.6 Resize 와 schema 영속

- Resize 완료 시 (`onResizeEnd`): `PUT /api/sessions/<name>/layout` (debounce 300ms)
- xterm winsize: 별 WS frame `PANE_RESIZE` (실시간, no debounce, PTY 가 즉시 알아야)
- Resize 중 (`onResize` 중간): client-side state 만 변경, server 통신 없음 (resize 완료 시점만)

#### 14.20.7 Focus mode (G34 grilling)

##### 14.20.7.1 정의
- **M (selection) 외 dim** — M items 정상 (opacity 1), 나머지 panel/item 50% opacity.
- **Streaming State 부수 영향 X** — dim 된 panel 도 *Streaming* 그대로 (Suspended 아님). xterm output 계속 흐름.
- **Click/interaction 자유** — dim 된 item 도 click → M 에 추가 → 그 item 자동 정상화.
- **Maximize (G20) 와 별 개념** — Maximize 는 area fill (다른 panel 가림), Focus 는 dim only (다른 panel 보임).

##### 14.20.7.2 Lifecycle (Ephemeral)
- **FE-only ephemeral**. Schema 영속 X.
- Attach 마다 fresh (Focus off).
- Session switch 시 reset.

##### 14.20.7.3 Toggle
- **Statusbar [🎯 Focus mode] toggle 버튼** (statusbar cluster 안, ViewportCtrl 옆 또는 별 위치)
- **단축키 `Cmd/Ctrl+Shift+F` (P1, Stage 7)**.
- **M 변경 시 dim 자동 갱신** — M 에 추가/제거 따라 그 item 의 opacity 적용.
- **M 이 empty 일 때 Focus mode on** → 모든 item dim (또는 toggle 자동 off — UX 선택, default = 모두 dim 으로 표시 + 명시 click 으로 회복).

##### 14.20.7.4 Visual indicator
- Focus mode ON 시 statusbar toggle 버튼이 *active* 표시 (filled icon 또는 색 강조).
- Canvas viewport 상단 toast/badge "Focus mode" (선택적 — 명시 사용자 인지).

---

## 15. BE / FE 의존성 cross-matrix (디테일)

행 = FE 항목, 열 = unblock 에 필요한 BE 항목. ✓ = 블로킹 의존. (2026-05-15 G18~G25 grilling 후 BE-NEW-12 / BE-NEW-12.5 / FE-NEW-8 추가)

| FE 항목 | BE-1 Auth | BE-2 Schema | BE-NEW-1 WM | BE-NEW-2 Session | BE-NEW-3 Attach | BE-NEW-4 WS routing | BE-NEW-7 Cookie | BE-NEW-10 Term pool | BE-NEW-12 file_open | BE-NEW-12.5 close API |
|---|---|---|---|---|---|---|---|---|---|---|
| FE-1 Auth page | ✓ |  |  |  |  |  | ✓ |  |  |  |
| FE-NEW-1 Session UI | ✓ |  | ✓ | ✓ |  |  | ✓ |  |  |  |
| FE-NEW-2 Attach lifecycle | ✓ |  |  |  | ✓ |  | ✓ |  |  |  |
| FE-NEW-5 Attach confirm |  |  |  |  | ✓ |  |  |  |  |  |
| FE-3 Item model |  | ✓ |  |  |  |  |  |  |  |  |
| FE-4 Item renderers |  | ✓ |  |  |  |  |  |  |  |  |
| FE-NEW-6 Multi-xterm |  |  |  |  |  | ✓ |  | ✓ |  |  |
| FE-NEW-7 Session store |  | ✓ |  | ✓ |  | ✓ |  |  |  |  |
| FE-NEW-3 Terminal pool |  |  |  |  |  |  |  | ✓ |  | ✓ |
| FE-NEW-4 Terminal binding |  |  |  | ✓ |  |  |  | ✓ |  |  |
| FE-6 Layer list V2 |  | ✓ |  | ✓ |  |  |  | ✓ |  | ✓ |
| FE-7 Panel header V2 |  |  |  | ✓ |  |  |  | ✓ |  | ✓ |
| FE-8 Settings UI | ✓ |  |  |  |  |  | ✓ |  | ✓ |  |
| FE-9 Viewport sync |  | ✓ |  | ✓ |  | ✓ |  |  |  |  |
| FE-NEW-8 file_path open UX (G21) | ✓ | ✓ |  |  |  |  | ✓ |  | ✓ |  |

→ **BE-1, BE-2, BE-NEW-1/2/3/7 이 critical path** (FE 의 절반 이상이 의존). 우선 완료해야 FE 병렬 진입 가능.
→ **BE-NEW-12.5 (close API)** 는 Stage 4 후반~Stage 6 진입 전에 — FE-NEW-3 / FE-6 / FE-7 의 close UX 가 의존.
→ **BE-NEW-12 (file_open)** 는 Stage 5 의 file_path renderer (FE-4) 와 함께 — FE-NEW-8 의존.

---

## 16. 우선순위 매핑 (각 BE / FE 항목별 P0/P1/P2)

### Backend

| 항목 | 우선순위 | Stage |
|---|---|---|
| BE-NEW-1 WorkspaceManager | **P0** | Stage 1 |
| BE-NEW-2 SessionRecord CRUD | **P0** | Stage 1 |
| BE-NEW-11 v1→v2 migration | **P0** | Stage 1 |
| BE-2 Schema v2 | **P0** | Stage 1 |
| BE-3 Schema validation v2 | **P0** | Stage 1 |
| BE-1 Auth | **P0** | Stage 2 |
| BE-NEW-7 Cookie lifecycle | **P0** | Stage 2 |
| BE-NEW-8 Token + Password mode | **P0** | Stage 2 |
| BE-NEW-3 Session attach + match-or-spawn | **P0** | Stage 3 |
| BE-NEW-4 WS frame routing | **P0** | Stage 3 |
| BE-NEW-9 Cross-server session lock | **P0** | Stage 3 |
| BE-6 WS sync extension | **P0** | Stage 3 |
| BE-NEW-5 Heartbeat | **P0** | Stage 3 |
| BE-NEW-6 Auto-mount trigger-aware | **P0** | Stage 4 |
| BE-NEW-10 Terminal pool list API | **P0** | Stage 4 |
| BE-8 Terminal metadata | **P0** | Stage 4 |
| BE-7 Conflict + lock | **P0** | Stage 3-7 |
| BE-NEW-12.5 Panel/Terminal close 분리 + respawn (G25) | **P0** | Stage 4 |
| BE-9 Settings API | **P1** | Stage 7 |
| BE-5 File path policy | **P1** | Stage 5 |
| BE-NEW-12 file_path open + allowlist (G21) | **P1** | Stage 5 |
| BE-4 Asset storage | **P2** | Stage 8 |
| BE-10 Performance/Safety | **P2** (지속) | Stage 9-10 |

### Frontend

| 항목 | 우선순위 | Stage |
|---|---|---|
| FE-NEW-7 Session store 분리 | **P0** | Stage 1 |
| FE-3 Item model | **P0** | Stage 1 |
| FE-1 Auth page | **P0** | Stage 2 |
| FE-NEW-1 Session UI | **P0** | Stage 2 |
| FE-NEW-2 Webpage attach lifecycle | **P0** | Stage 2-3 |
| FE-NEW-5 Match-or-spawn dialog | **P0** | Stage 3 |
| FE-NEW-3 Terminal pool UI | **P0** | Stage 4 |
| FE-NEW-4 Terminal binding UI | **P0** | Stage 4 |
| FE-NEW-6 Multi-xterm subscriber | **P0** | Stage 4 |
| FE-6 Layer list V2 | **P0** | Stage 6 |
| FE-7 Panel header/footer V2 | **P0** | Stage 6 |
| FE-9 Viewport sync UI | **P1** | Stage 7 |
| FE-2 Toolbar2 + Tool state | **P1** | Stage 5 |
| FE-4 Item Renderers (text/note/rect/ellipse/line/**file_path**) | **P1** | Stage 5 |
| FE-NEW-8 file_path open UX (G21) | **P1** | Stage 5 |
| FE-5 Creation gestures | **P1** | Stage 5 |
| FE-8 Settings UI (+ G19 overlay + G21 allowlist editor) | **P1** | Stage 7 |
| FE-4 Item Renderers (image/document) | **P2** | Stage 8 |
| FE-4 Item Renderers (free_draw) | **P2** | Stage 9 |
| FE-10 UX polish | **P2** (지속) | Stage 7-10 |
| FE-11 Tests | **P0** (지속) | Stage 10 |

### Critical path (P0 의 최단 경로, 2026-05-15 G18~G25 grilling 후 갱신)

```
Stage 1: BE-NEW-1, BE-NEW-2, BE-NEW-11, BE-2, BE-3
       │
       ├─ FE-3 (TS type), FE-NEW-7 (store 분리)
       │
Stage 2: BE-1, BE-NEW-7, BE-NEW-8 ─→ FE-1, FE-NEW-1
       │
Stage 3: BE-NEW-3, BE-NEW-4, BE-NEW-9, BE-6, BE-NEW-5 ─→ FE-NEW-2, FE-NEW-5
       │
Stage 4: BE-NEW-6, BE-NEW-10, BE-8, BE-NEW-12.5 ─→ FE-NEW-3, FE-NEW-4, FE-NEW-6
       │   (BE-NEW-12.5 = close API + respawn — G25 의 panel/terminal 분리)
       │
Stage 6: (BE 의존 적음) ─→ FE-6, FE-7
       │   (FE-6 Layer list V2 = ADR-0024 Tree/Z 분리 정합, BE-NEW-12.5 group close API 의존)
       │   (FE-7 Panel header V2 = ADR-0021 D9 close dialog + dangling overlay, BE-NEW-12.5 의존)
       │
P0 완료
```

P1 (Stage 5/7) 와 P2 (Stage 8/9) 는 P0 완료 후 또는 일부 병렬 가능. Stage 5 의 file_path renderer (FE-4) 와 BE-NEW-12 / FE-NEW-8 (file_path open security) 는 같이.

---

## 17. 첫 진입 권장 (다음 세션)

다음 세션 첫 메시지 가이드:

| 사용자 메시지 | 진입 |
|---|---|
| "Stage 1 시작" / "BE Workspace + Session storage" | Stage 1-BE.1 부터, BE-1.1 ~ BE-1.6 순차 |
| "Stage 2 Auth 시작" | Stage 1 완료 확인 후 Stage 2-BE |
| "Schema v2 migration" | ADR-0018 D5 + Stage 1-BE.3 |
| "Auth page UI" | Stage 2-FE — 단 BE-2.1/2.2 의 endpoint 가 mock 라도 있어야 |
| "Session list modal" | Stage 2-FE.6 — BE-1.5 의 `GET /api/sessions` 필요 |
| "Terminal multi-attach" | Stage 4 — Stage 3 완료 후 |
| "현재 결정 검토" | 본 plan §2 |
| "ADR 새로 추가" | ADR-0022 부터 (asset storage 등) |

## 18. 변경 이력

- 2026-05-17: §14.20.5.2 P0 매트릭스 + §14.20.5.4 비범위 amend — **ADR-0017 D6 amend ⑤ (Basic editing shortcut matrix) 정합**. P0 6 row 추가: Cmd/Ctrl+A (Select all — 신규, focus 4 모드 분기) / Cmd/Ctrl+C/X/V (ADR-0030 D5 cross-link, Copy/Cut/Paste) / Cmd/Ctrl+Z, Shift+Z (ADR-0028 cross-link, Undo/Redo). §14.20.5.4 비범위에 OS-standard 5종 (Cmd+S/P/W/R/Tab) 의도적 제외 + Cmd+F 의 P2 deferred (별 ADR / Cmd+K palette 분기) 명시. 짝: ADR-0017 변경 이력 + handover-v3 §10.5.2 / .4 / §13.
- 2026-05-15: 초안. plan-0006 supersede. 17 결정의 BE/FE 병렬 Stage 분해 + integration gate + 의존성 매트릭스 + 잔여 P2+ 큐.
- 2026-05-15: §13 (BE 기능 명세) / §14 (FE 기능 명세) / §15 (BE/FE cross-matrix) / §16 (P0/P1/P2 매핑) 신규 — plan-0006 §5/§6 의 후속 정리. 기존 §11/§12 → §17/§18 으로 이동.
- 2026-05-15 (G18~G25 grilling 합본):
  - §2 결정 매트릭스에 G18~G25 8 row 추가
  - §10 미해결 잔여: G18 / G21 resolved 표시
  - §13.19 (BE-NEW-9 cross-server lock) → flock+lease hybrid 디테일 (G18)
  - §13.22 (BE-NEW-12 file_path open + allowlist) 신규 (G21, ADR-0023)
  - §13.22.5 (BE-NEW-12.5 Panel/Terminal close 분리 + respawn) 신규 (G25, ADR-0021 D9 amend)
  - §13.9 (BE-9 Settings API) PATCH/POST 정합 (G19)
  - §14.2 (Toolbar2 one-shot + Q lock) amend (G22)
  - §14.6 (Layer list V2) [Tree | Z] toggle + group close confirm modal (G24, G25)
  - §14.7 (Panel header V2) 4 z 액션 + [Kill terminal] + [Remove panel] + PanelCloseConfirmModal + DanglingOverlay (G24, G25)
  - §14.8 (Settings UI) overlay + auto-save 정책 (G19)
  - §14.12 (Session UI) 1s polling (G18)
  - §14.19 (FE-NEW-8 file_path open UX + Settings allowlist editor) 신규 (G21)
  - §14.20 FE-UX-Common 공용 UX 운영 규칙 신규 — Inline edit (G23), Esc 라우팅 (G20+G22+G23), Toolbar 정책 (G22), Maximize (G20)
- 2026-05-15 (D 검증 amend): §15 cross-matrix 에 BE-NEW-12 / BE-NEW-12.5 컬럼 + FE-NEW-8 행 추가. §16 우선순위 매핑에 BE-NEW-12 / BE-NEW-12.5 + FE-NEW-8 추가. Critical path diagram 의 Stage 4 에 BE-NEW-12.5 명시. Stage 5 정의 갱신 — file_path 가 Stage 5 (string-only, asset 비의존) 로 이동, image/document 만 Stage 8 잔여. Stage 4 / Stage 5 의 sub-items amend.
- 2026-05-15 (G26~G29 P1 grilling 합본):
  - §2 결정 매트릭스에 G26~G29 4 row 추가
  - §10 미해결 잔여: G26 / G27 / G28 / G29 (Toolbar2 Select/Hand) resolved 표시
  - §14.20.5 Keyboard shortcut 정책 신규 (G26 — Hybrid layer + xterm Esc shell 전달 + P0/P1 매트릭스 + Discoverability)
  - §14.10 FE-10 UX polish amend (G27 — light+dark 2 fixed + system detect + chrome/xterm 동기 + ANSI 16 색 보존)
  - §14.8 FE-8 Settings UI amend (G27 Theme section + G26 Shortcut section + G28 export/import 버튼)
  - §13.9 BE-9 Settings API amend (`POST /api/sessions/import` G28)
  - §14.2 FE-2 Toolbar2 amend (G29 — Select+Hand mode + Space-pan modifier)
  - §14.20.5.2 매트릭스에 Space+drag pan 추가 (G29)
- 2026-05-15 (초기 기획 누락 점검 + Tier 1 + Tier 3 amend grilling 합본):
  - §2 결정 매트릭스에 G32 / G37~G40 / G33 / G34 / G35 / G36 / Tier 3 row 추가
  - §14.20.5.2 P0 단축키에 Cmd+D (G32) + Cmd+0/1/2 (G33) 추가
  - §14.20.5.3 P1 단축키에 Cmd+./=/- (G33) + Cmd+Shift+F (G34) 추가
  - §14.20.6 Resize 정책 신규 (G37~G40 + Sprint 7 SoT 명시 + Minimum size + Group resize 미지원 + 영속 정책)
  - §14.9 FE-9 Viewport sync UI amend (G33 — UI 위치 + zoom 범위 + mouse/trackpad + 단축키 + edge cases)
  - §14.20.7 Focus mode 신규 (G34 — M 외 dim, ephemeral, Statusbar toggle + Cmd+Shift+F)
  - §14.8 Settings 의 Terminal section + Templates section 신규 (G35 + G36)
  - §14.7 Panel Settings modal 의 Terminal Override sub-section 신규 (G35)
  - §14.2 FE-2 Toolbar2 의 Terminal dropdown (G36)
  - §13.20 BE-NEW-10 amend (`POST /api/terminals { template_id }` + template CRUD endpoint, G36)
  - §13.9 BE-9 amend (`POST /api/shutdown`, Tier 3)
  - §14.10 FE-10 amend (`ServerShutdownConfirmModal.svelte` + `reconnect.svelte.ts` Tier 3)
  - ADR-0018 D4 의 `terminal` payload 에 `terminal_overrides` field 추가 (G35)
- 2026-05-16 (chrome amend 정합 — ADR-0017 ②/③/④ + ④ follow-up + ADR-0021 D7 amend ② + ADR-0018 G39/G40 정합):
  - §10.2 의 Mini-map / 즐겨찾기 hint: Sidebar → LeftPanel 어휘 정합
  - §11 critical path 의 Stage 5 FE 항목명 정합 (FE-4.1: Sidebar 의 Terminal list section → LeftPanel 의 Terminals tab / `TerminalListView`)
  - §14.6 FE-6 산출물 amend — chrome SoT 재정의: `lib/sidebar/LeftPanel.svelte` (chrome owner) + `lib/sidebar/LayerTreeView.svelte` (구 `Sidebar.svelte` rename) + `lib/sidebar/TerminalListView.svelte` (Terminals tab content). LeftPanel 안 `[Layers | Terminals]` 가로 탭 + 28px collapsed rail with per-tab icons + `chromeStore.leftPanelTab`
  - §14.8 FE-8 산출물 amend — `SettingsOverlay.svelte` 가 ADR-0017 ④ 으로 chrome 부분 ship (Theme · Shortcuts section + Cmd+, 진입 + auto-save). Storage/Auth/Behavior/Debug/Terminal/Templates section 은 BE wire 시 별 amend (현재 placeholder)
  - §14.10 FE-10 theme adapter amend — 실제 ship 명: `lib/stores/theme.svelte.ts` (`mode` user choice / `resolved` $derived) + `lib/xterm/xtermTheme.ts` (경로 `lib/xterm/` 정합) + XtermHost 의 hot reload $effect + `--canvas-bg` light flash 방지
  - §14.14 FE-NEW-3 amend — Terminal pool UI 가 LeftPanel 의 Terminals 탭 (`TerminalListView.svelte`) 으로 통합 (구 별 section / `TerminalListSection.svelte` 회수)
  - §14.20.5.3 P1 매트릭스 — `Cmd/Ctrl+Shift+L` 의 동작 의미 정합 (LeftPanel toggle) + `Cmd/Ctrl+Shift+I` (RightPanel toggle, ADR-0017 ③) row 추가
  - §14.20.5.6 shortcutRegistry path: `lib/common/` → `lib/keyboard/` 정합 + `chromeShortcuts.svelte.ts` 첫 consumer 명시 + Esc 는 별 dispatcher (`escRouter`) 유지 명시
  - ADR-0017 ① (TerminalsPanel.svelte 별 floating panel 분리) 는 같은 날 ② amend 으로 회수 — 본 plan 의 ① 직접 참조 없음. RailToggle.svelte 폐기 (ADR-0017 ③) — 두 panel 모두 self-contained 28px rail
  - PaneInfoPanel.svelte → ItemInfoView.svelte rename + RightPanel.svelte 신규 (ADR-0017 ③)
  - text item field `text_align`/`text_vertical_align` (G39/G40) 은 ADR-0018 D4 정본이 SoT — plan 의 §14.5 는 "TextNode" 만 명시 (field 디테일 중복 회피)
- 2026-05-16 (§14.10 Tier 3 + attach recovery layer 분리 amend — ADR-0019 D5.1 + D5.4, plan-0008):
  - §14.10 의 "WS reconnect backoff" → **"WS auto-reconnect"** rename + 실제 ship 정합. 별 `lib/ws/reconnect.svelte.ts` 채택 X → `lib/ws/client.ts` 안 `#scheduleReconnect()` 통합 (`ConnectionState` 머신 + `BACKOFF_MS` array + 누적 attempt 카운터 `connectionStore` 노출). 별 store `lib/ws/heartbeat.svelte.ts` (frame/activity timestamp + `isStale`/`isIdle` derived) 명시.
  - Server shutdown confirm modal — `ServerShutdownConfirmModal.svelte` → **`ShutdownModal.svelte`** rename (어휘 단순화). graceful teardown cross-link 을 ADR-0014 D7 + D12 둘 다 명시.
  - **신규 항목**: *Session attach recovery 는 별 layer (D5.1/D5.4)* 임 명시 — §14.10 의 WS auto-reconnect 는 *transport* 만 보장하고, BE 의 `session_locks_by_cookie` 가 timeout-release 됐을 때의 attach 상태 복구는 plan-0008 (`reconnectGate` + `ReconnectModal` + `sessionStorageHint` + `sessionStore.{attemptReattach, silentReattach, ensureMutationOk}`) 으로 처리. Trigger 합집합 (WS reconnect 완료 + visibilitychange) + 결과 분기 (200/409/404/401/5xx) + UX 명시 (Case I blocking modal / Case II silent + mutation guard) 의 형태.
  - 옛 "재연결 성공 시 자동 복귀" 단순 정책 → strikethrough + plan-0008 의 두 path 로 대체 명시.
