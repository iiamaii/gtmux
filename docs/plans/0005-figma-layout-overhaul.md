# Plan 0005 — Figma-Adapted Layout Overhaul

- 일자: 2026-05-15
- 작성: agent (사용자 결정 Q1=light+dark, Q2=lucide 유지, Q3=Pane Info, Q4=ref design language 흡수 후)
- 진입점: `docs/reports/0029-frontend-design-ref-analysis.md` (분석 + 결정 surface)
- ADR: `docs/adr/0016-design-tokens-and-iconography.md` (v2 amend 완료)
- supersede 관계: plan 0004 §3 (레이아웃 그리드) 와 §6 (Feature surface) 를 본 plan 이 *대체*. plan 0004 의 §1 (현 상태), §2 (디자인 원칙), §5 (primitive 라이브러리) 는 유효 — 본 plan 은 그 위에 layout overhaul 만 추가
- 범위 제외: S7-FE-SHUTDOWN feature (0003 plan) 는 본 plan 의 §6 SessionMenu / ShutdownModal 안에 *완전 흡수*

---

## 0. 한 줄 목표

`+page.svelte` 의 단일 `Toolbar + Sidebar + Canvas` 구조를 **Figma-inspired 6 영역** (Banner → Titlebar 44px → Toolbar 56px → workspace (floating Sidebar + Canvas + floating PaneInfo + ViewportCtrl + HelpBar + ContextMenu + RailToggle ×2)) 로 재배치. Phase 0+1 의 디자인 시스템 (tokens v2 + primitive 9건) 위에 신규 chrome 컴포넌트 7건 마운트.

---

## 1. 목표 레이아웃 (gtmux adaptation, ref §1.2)

```
+========================================================================+
| ReconnectBanner   (conditional, z=1000)                                |
+========================================================================+
| Titlebar (44px, z=30)                                                   |
| [≡] [Workspace]              gtmux · demo · 127.0.0.1:9999  [☀/☾][Focus]|
+========================================================================+
| Toolbar  (56px, z=25)                                                   |
|        [ Select | Hand ]   [ + New Panel ]   [ ... ]                    |
+----+--------------------------------------------------------------+----+
|    |                                                              |    |
|    | .help-bar  ⌘+wheel · zoom | space+drag · pan | ⌘N · new panel |    |
|    | .canvas-stage (dot grid)                                     |    |
|    |                                                              |    |
| Sb |  ┌─ Panel ─────────┐    ┌─ Panel ─────────┐                  | Pi |
| id |  │ %5  L M I  [×] │    │ %7  L M I  [×]  │                  | nf |
| eb |  │ $ ls -la       │    │ $ vim file.md   │                  | o  |
| ar |  │ ...            │    │ ...             │                  | (2 |
| (2 |  └────────────────┘    └─────────────────┘                  | 68 |
| 48 |                                                              | px)|
| px)|                                                              |    |
|    |         ┌─────── viewport-ctrl ───────┐                      |    |
|    |  rail   │ −  ⃦100% ⃦  +  ·  fit  100  ⃦│  rail                |    |
|    |  ([◀])  │                              │  ([▶])              |    |
|    |         └──────────────────────────────┘                      |    |
|    |                                                              |    |
+----+--------------------------------------------------------------+----+
```

치수:
- Titlebar 44px (`--layout-titlebar-h`)
- Toolbar 56px (`--layout-toolbar-h`)
- Sidebar 248px floating @ left:8 top:8 bottom:8 (`--layout-sidebar-w`)
- PaneInfo 268px floating @ right:8 top:8 bottom:8 (`--layout-sidebar-right-w`)
- Canvas: workspace 전체, floating panels 가 위에 떠 있음
- Banner: 32px (`--layout-banner-h`), conditional 표시 시 Titlebar 위

z 순서: canvas(0) < canvas-overlay(18: help-bar/viewport-ctrl) < rail(19) < side-panel(20) < toolbar(25) < titlebar(30) < context-menu(100) < banner(1000) < modal(2000) < toast(3000).

---

## 2. 도메인 매핑 정리 (ref → gtmux)

| ref 어휘 | gtmux 어휘 / 동작 |
|---|---|
| **Title bar 좌측** — 햄버거(≡) + File/Edit/View/Object/Help 탭 | 햄버거 1개만 (SessionMenu — Session shutdown / Rotate token / About). 텍스트 탭은 1개 ("Workspace") — 단일 view 라 미래 multi-canvas 자리 |
| **Title bar 중앙** — "Acme Studio / Onboarding — v3 · Saved 2m ago" | "gtmux · `demo` · 127.0.0.1:9999 · Local" — session 이름 + 바인드 + 모드 |
| **Title bar 우측** — 테마 토글 + 아바타 + Share + Present | 테마 토글 (☀/☾) + Focus mode 토글 (Eye 아이콘). 아바타/Share/Present 폐기 (single-user) |
| **Toolbar — Page 1 ▾** | 폐기 (multi-canvas 비범위) |
| **Toolbar — Select / Hand** | 유지 — Pan tool 명시화 (현재 SvelteFlow 의 Space-drag 만) |
| **Toolbar — Panel tool** | "New Panel" 액션 통합 (현재 NewPanelButton 을 toolbar arm 으로 이동) |
| **Toolbar — Rect / Ellipse / Polygon / Pen / Text / Doc / Caption / Comment** | 전부 폐기 (도형 도구 없음) |
| **Toolbar — More (...)** | 미사용 |
| **Left panel** — Layers / Assets / Pages 탭 | Layers 1개만 (현 Sidebar). Assets / Pages 탭은 시각 정합용 disabled tab 으로 표시 또는 제거 — *제거* 선택 |
| **Right panel** — Design / Prototype / Inspect | **Pane Info** 1개. 선택된 Panel 의 `pane_id` / `label` / `locked` / `visibility` 표시. 미선택 시 "No selection" empty state |
| **Viewport ctrl** | 그대로 (Zoom in/out/100%/fit + history). M count badge 추가 — `M:3` 같은 mini-indicator |
| **Help bar (top-center pill)** | 그대로 (단축키 hint). gtmux 의 `⌘N`, `space+drag`, `⌘+wheel` 명시 |
| **Context menu (우클릭)** | 부분 — Copy pane_id / Close pane / Hide / Lock / Group selection / Send to back. gtmux 어휘로 |
| **Rail toggle** | 그대로 — Sidebar / PaneInfo 접기 |
| **Dot grid** | 그대로 (SvelteFlow 의 Background 가 이미 dot pattern 지원 — variant="dots" 옵션) |
| **Selection outline + 4 handles** | SvelteFlow 의 기본 selection 위에 우리 CSS 로 *Figma-style* outline (1.5px accent + handle 4개) 오버라이드 |
| **Marquee selection** | SvelteFlow `selectionOnDrag` 또는 우리 구현 — P1+ |

---

## 3. 신규 컴포넌트 인벤토리

`src/lib/ui/` (primitives, Phase 0+1 v1 — 완료):
- ✅ Icon, Button, IconButton, Tooltip, Dropdown, Modal, Banner, Toast, Input

`src/lib/chrome/` (신규 디렉터리 — chrome 전용):
- 🆕 **Titlebar.svelte** — 44px header. SessionMenu trigger + Workspace tab + 중앙 session info + ThemeToggle + FocusToggle. ~150 LOC.
- 🆕 **SessionMenu.svelte** — Dropdown 의 wrapper. items: Session shutdown / Rotate token / About. ~80 LOC.
- 🆕 **ShutdownModal.svelte** — Modal 의 wrapper. 활성 pane 수 + session 이름 + layout 보존 안내. ~100 LOC.
- 🆕 **ThemeToggle.svelte** — IconButton + lucide Sun/Moon. html.dark 클래스 토글 + localStorage 영속. ~40 LOC.
- 🆕 **FocusToggle.svelte** — IconButton + lucide Maximize2. ephemeralStore.focusMode 토글 broadcast. ~40 LOC.
- 🆕 **Toolbar2.svelte** — 56px 두 번째 toolbar. ToolGroup [Select | Hand] | divider | [New Panel] | More. ~120 LOC. (현 `Toolbar.svelte` rename → `Titlebar.svelte` 흡수 또는 폐기)
- 🆕 **HelpBar.svelte** — top-center pill. 단축키 hint. ~60 LOC.
- 🆕 **ViewportCtrl.svelte** — bottom-center pill. Zoom in/out/100%/fit + M count badge. ~120 LOC.
- 🆕 **RailToggle.svelte** — 16×64 가는 collapse button. Sidebar / PaneInfo 양쪽 사용. ~50 LOC.
- 🆕 **ContextMenu.svelte** — 우클릭 메뉴. Copy pane_id / Close pane / Hide / Lock / Group / Send to back. ~150 LOC.

`src/lib/sidebar/` (Layers panel):
- 🔄 **Sidebar.svelte** — 기존 layer tree 유지, **chrome 만 floating panel 로 wrap** (8px gap + radius-lg + shadow-md). 토큰 정합 refactor. ~+30 LOC.
- 🔄 GroupTree / PanelRow — 미사용 placeholder (Sidebar 가 인라인 렌더링)

`src/lib/inspector/` (신규 디렉터리 — 우측 PaneInfo 패널):
- 🆕 **PaneInfoPanel.svelte** — 268px floating panel. M 의 첫 번째 Panel 에 대한 속성 표시. ~200 LOC.

`src/lib/canvas/`:
- 🔄 **Canvas.svelte** — SvelteFlow 위에 ViewportCtrl + HelpBar overlay. 기존 NewPanelButton 은 Toolbar 로 이전 → 제거.
- 🔄 **PanelNode.svelte** — header 에 X (close) IconButton 추가 + Figma selection 시각 (1.5px accent + 4 핸들). 토큰 정합.
- 🔄 **NewPanelButton.svelte** — Toolbar 의 "New Panel" 툴 arm 으로 흡수, 별도 컴포넌트 제거 또는 toolbar tool 로 변환.
- 🔄 **PanelPlaceholder.svelte** — 토큰 정합 (시각 변화 없음).

`src/routes/+page.svelte`:
- 🔄 **rewrite** — 새 grid layout, 신규 chrome 컴포넌트 마운트 + Toast 호스트 + ContextMenu 호스트.

`src/lib/stores/`:
- 🔄 **theme.svelte.ts** (신규) — light/dark 상태 + localStorage 영속.

LOC 추정: 신규 chrome ~1,100 + refactor ~250 + +page rewrite ~150 = **+1,500 LOC**.

---

## 4. 구현 stage

### Stage A — 토큰 + global 마이그레이션 ✅ 완료
- ADR-0016 amend ×1
- tokens.css 재작성 (light + dark)
- global.css focus dashed + font + scroll
- primitive 9건 토큰 rename (`--space-1` → `--space-4` 등)

### Stage B — Stores + ThemeToggle 인프라
- `src/lib/stores/theme.svelte.ts` 신규 — `theme: 'light' | 'dark'` + persist (localStorage `gtmux-theme`) + html.dark 클래스 자동 sync
- `+page.svelte` 의 OnMount 에 theme 복원 hook

### Stage C — Titlebar + SessionMenu + ShutdownModal (S7-FE-SHUTDOWN 흡수)
- `Titlebar.svelte` (44px 그리드 — 좌 햄버거+탭 / 중앙 session info / 우 토글)
- `SessionMenu.svelte` (Dropdown wrapper, 3 items)
- `ShutdownModal.svelte` (Modal wrapper, 활성 pane 수 + session 이름)
- `ThemeToggle.svelte` (☀/☾)
- `FocusToggle.svelte` (Eye)
- 기존 `Toolbar.svelte` (placeholder) 폐기
- bootstrap landing 의 `sessionStorage.gtmux_session` 주입 (backend +5 LOC)

### Stage D — Toolbar 2nd (56px)
- `Toolbar2.svelte` — Tool group [Select | Hand] + [New Panel] + divider + More
- Active tool state — ephemeralStore 에 `currentTool: 'select'|'hand'` 추가 (or component-local)
- 기존 NewPanelButton 의 onclick 로직을 Toolbar 의 New Panel arm 으로 이전 (또는 `<NewPanelButton />` 을 toolbar 안에 inline)

### Stage E — Sidebar floating refactor + PaneInfoPanel
- Sidebar — chrome 만 변경 (position: absolute, radius-lg, shadow-md, collapse rail). 트리 내용 무변경.
- `RailToggle.svelte` — 좌/우 양쪽 재사용
- `PaneInfoPanel.svelte` — M selection 변경 시 첫 번째 Panel 의 속성 표시. read-only v0

### Stage F — Canvas chrome (HelpBar + ViewportCtrl + ContextMenu)
- `HelpBar.svelte` — top-center pill, kbd hints (`⌘N` `⌘+wheel` `space+drag`)
- `ViewportCtrl.svelte` — bottom-center pill, zoom buttons + M count badge
- `ContextMenu.svelte` — 우클릭 시 (좌표 클램프) Copy pane_id / Close / Hide / Lock / Group
- Canvas.svelte 에 마운트 + SvelteFlow 의 viewport API 와 wire

### Stage G — PanelNode 마감 + close button (S7-FE-CLOSE-GUARD 흡수)
- PanelNode header 에 `IconButton(X, aria='Close panel')` 추가
- disabled when `muxStore.liveCount === 1` + Tooltip "Use Session shutdown for the last pane"
- Figma-style selection (1.5px accent outline + 4 corner handle)
- 토큰 정합 (hardcoded color 제거)

### Stage H — +page.svelte rewrite
- 새 grid 적용: `grid-template-rows: var(--layout-banner-h, 0) var(--layout-titlebar-h) var(--layout-toolbar-h) 1fr`
- Workspace 안에 Canvas + 좌 Sidebar + 우 PaneInfoPanel + RailToggle ×2 마운트
- Toast 호스트 + ContextMenu 호스트 추가

### Stage I — S7-FE-AUTOMOUNT (frontend pane-spawned cascade PUT) — 0003 plan Phase B 흡수
- dispatcher 의 `handleNotifyMirror` 'pane-spawned' 안에서 layout 에 없는 pane 발견 시 cascade PUT
- ADR-0015 신규 발행 (auto-mount 책임 = frontend)

### Stage J — Backend KillSession (0003 plan Phase A 흡수)
- BackendCommand::KillSession variant + cmd_router + ws-server SIGTERM self
- ADR-0013 D10 amend

### Stage K — 검증 + 시연
- `cargo test --workspace --tests` + `cargo clippy` + `cargo fmt`
- `svelte-check` + `npm run build`
- 브라우저 시연 (port 9999) — light/dark 토글 + 모든 chrome 컴포넌트 + Shutdown flow

LOC 누적: Stage A ~250 (완료) + B ~80 + C ~600 + D ~200 + E ~400 + F ~350 + G ~150 + H ~150 + I ~50 (frontend) + J ~100 (backend) = **+2,300 LOC** (frontend +2,150, backend +100, docs +60).

---

## 5. ADR 매트릭스

| ADR | 제목 | Stage | 상태 |
|---|---|---|---|
| ADR-0013 D10 amend | BackendCommand 에 KillSession variant | Stage J | 0003 plan 에서 정의, 코드 진입 시 amend |
| ADR-0015 신규 | Pane auto-mount 책임 = frontend | Stage I | 0003 plan 에서 정의, 코드 진입 시 발행 |
| **ADR-0016 v2 amend** | Figma tokens + light/dark + dashed focus | Stage A ✅ | 완료 |
| **ADR-0017 신규** | Layout overhaul — 6 영역 grid (titlebar/toolbar/floating panels/viewport-ctrl/help-bar) | Stage C 진입 시 | **본 plan 의 prerequisite** |
| **ADR-0018 신규 (옵션)** | Keyboard shortcut + global handler | Stage K 이후 별도 PR | P1+ |

ADR-0017 의 책임: `+page.svelte` 의 grid 구조 + floating panel 정책 + collapse rail 정책 + theme toggle 위치 + Session shutdown UX placement 를 잠근다. plan 0005 의 §1-2 결정을 ADR 형식으로 정형화.

---

## 6. Open questions

- **O1**: Sidebar / PaneInfo 의 collapse 상태 영속화 — localStorage 또는 layout schema 의 부가 필드? 본 plan 은 localStorage 권장 (frontend-only, web state).
- **O2**: theme 의 디폴트 — sketch 의 *기본 dark* vs 브라우저 `prefers-color-scheme` 우선? 본 plan 은 후자 (`prefers-color-scheme: light` 면 light 디폴트). 사용자 명시 토글이 항상 우선.
- **O3**: SvelteFlow 의 selection style override — `.svelte-flow__node.selected` 의 default outline 을 우리 Figma-style 로 교체할 때 SvelteFlow 의 `selectionKeyCode` / `multiSelectionKeyCode` 와 정합 검증.
- **O4**: dot grid — SvelteFlow `<Background variant="dots" gap={24} size={1} color="var(--canvas-grid)" />` 가 ref 의 radial-gradient 와 동등한지 시각 비교.
- **O5**: light theme 에서 xterm.js theme — 현재 xterm 은 dark hardcoded. light 적용 시 별도 xterm theme 어댑터 phase 필요 (Stage K 이후 별도 task).

---

## 7. 검증 게이트 (각 stage)

| Stage | 게이트 |
|---|---|
| A | svelte-check 0/0, 기존 primitive 시각 정합 (light + dark 둘 다) |
| B | localStorage persist 정상 (browser refresh 후 theme 유지) |
| C | Titlebar / SessionMenu / ShutdownModal 동작 + Esc/backdrop 닫힘 + focus trap |
| D | Toolbar Select/Hand 활성 토글 + New Panel 동작 (기존 흐름 보존) |
| E | Sidebar / PaneInfo 둘 다 floating + RailToggle 으로 접힘/펼침 |
| F | HelpBar pill 표시 + ViewportCtrl 의 zoom in/out/100%/fit 모두 SvelteFlow viewport 와 sync + ContextMenu 좌표 클램프 |
| G | PanelNode X 버튼 표시 + 마지막 1개일 때 disabled |
| H | +page.svelte rewrite 후 모든 chrome 컴포넌트 정상 마운트 |
| I | 외부 spawn 시 (or NewPanelButton 의 PUT 경로 외) frontend 가 자동 cascade PUT |
| J | gtmux 의 SessionMenu → Shutdown → backend exit 6 graceful |
| K | full smoke — browser 에서 모든 chrome / Modal / banner / toast / shortcut 검증 |

---

## 8. 다음 행동

| 사용자 메시지 | 행동 |
|---|---|
| "Stage B 진행" / "ThemeToggle 시작" | theme.svelte.ts + +page.svelte hook + Sun/Moon icon test |
| "Stage C 진행" / "Titlebar 시작" | ADR-0017 draft 먼저 → Titlebar / SessionMenu / ShutdownModal / FocusToggle |
| "Stage E 진행" / "Sidebar floating" | Sidebar chrome refactor + RailToggle + PaneInfoPanel |
| "Stage A 커밋 먼저" | 토큰 v2 commit 한 후 stages 진행 |
| "전체 한 번에" | A → B → C → D → E → F → G → H → I → J → K 순서로. 각 stage 별 PR 또는 1 PR bundling. |
| "ADR-0017 먼저 작성" | Stage A 커밋 + ADR-0017 draft + Stage B 부터 |

---

## 9. 부분 진행 (Stage 우선순위 끌어올림)

사용자 피드백 5건 (2026-05-15) 으로 다음 Stage 항목이 *조기 진입* 됨:

| Stage | 항목 | 적용 |
|---|---|---|
| Stage A 후속 | Canvas 색 테마 (--canvas-bg / --canvas-grid 진입) | Canvas.svelte 의 `<Background bgColor patternColor>` + `.canvas-root { background }` + xyflow CSS 변수 override |
| Stage G | Panel 선택 모드 분기 (single solid vs multi dashed) | PanelNode 의 `.m-single` / `.m-multi` 클래스 + `m_multi` data prop (Canvas 가 `ephemeralStore.m.size > 1` 계산) |
| Stage G | Panel resize | `@xyflow/svelte` 의 `NodeResizer` 도입 — `isVisible={isInM && !isLocked}`, min 240×140, onResizeEnd → `panelsStore.resizePanel` + `putLayoutCommitCurrent` |
| Canvas 측 | Selection logic — plain=single / Cmd-Ctrl-Shift=multi-toggle | `onnodeclick({ node, event })` 에서 modifier key 분기. `event.metaKey \|\| event.ctrlKey \|\| event.shiftKey` |
| Stage E (부분) | Layer panel row 시각 | Sidebar.svelte 의 row hover-fade icons + border-left accent indicator on selected + accent text + glass-1 hover + transition |

남은 Stage G / E 잔여:
- PanelNode 의 X (close) 버튼 (S7-FE-CLOSE-GUARD)
- Sidebar 의 floating panel chrome (radius-lg + shadow-md + 8px gap)
- PaneInfoPanel (우측 268px floating panel)

### Stage C + J — Titlebar / SessionMenu / ShutdownModal / FocusToggle + Backend KillSession (완료)

ADR-0017 신규 + ADR-0013 D10 amend ×4 동반.

**Frontend (Stage C, src/lib/chrome/ 신규)**:
- `Titlebar.svelte` (44px) — 3-col grid (SessionMenu+Workspace tab / session info / ThemeToggle+FocusToggle)
- `SessionMenu.svelte` — Dropdown 위 kebab. Items: Session shutdown / Rotate token / About
- `ShutdownModal.svelte` — Modal 위. 활성 pane 수 + session 이름 + bullet 3건 (pane 정리 / layout 보존 / exit 6) + Cancel/Shutdown buttons
- `FocusToggle.svelte` — IconButton + Maximize2/Minimize2 SVG. ephemeralStore.focusMode 토글 (실제 visual effect 는 P1+ wire)
- 임시 `Toolbar.svelte` 대신 `Titlebar.svelte` 마운트 in `+page.svelte`
- `Toast.svelte` 호스트 마운트 추가
- `ReconnectBanner.svelte` — close code 1000 분기 amend: `client-stop` reason 외엔 "Session ended" banner

**Backend (Stage J)**:
- `BackendCommand::KillSession` variant + `dispatch` 의 no-op arm (ADR-0013 D10 amend ×4)
- `cmd_router::dispatch_ctrl` 의 `"kill-session"` arm → `CtrlOutcome::OkAndExit`
- ws-server `handle_socket` 의 `OkAndExit` 처리 — `encode_ctrl_success` ack + `libc::raise(SIGTERM)` self → axum graceful_shutdown
- `payload::encode_ctrl_success` 신규
- `forbid(unsafe_code)` → `deny(unsafe_code)` (libc::raise inline allow)
- `http-api::render_bootstrap_landing` 에 session 이름 인자 추가 → inline JS 가 `sessionStorage.gtmux_session` 도 주입

**검증**: cargo test --workspace --tests 164 PASS. clippy clean (pre-existing manual_contains 도 동시 fix). svelte-check 0/0. vite build OK (CSS +7.78 KB, JS +13.38 KB — Modal + Dropdown + 4 chrome).

**알려진 잔여**:
- FocusToggle 의 실제 visual effect (canvas darken / single-panel highlight) 는 미배선 — Stage K 또는 별도 phase
- Session shutdown 단축키 (Cmd-Shift-Q) 는 ADR-0017 §D6 에 spec, 별도 keyboard shortcut 시스템 phase
- ReconnectBanner 의 "Session ended" 메시지가 close reason 빈 문자열 시 표시 — backend 가 reason 을 명시하면 (P1+) 정밀 분기 가능

### Stage E — Sidebar floating + PaneInfoPanel + RailToggle ×2 (완료)

ref/frontend-design §6 (Left panel) + §7 (Right panel) + §8 (Rail toggles) adaptation.

**신규 컴포넌트**:
- `$lib/stores/chrome.svelte.ts` — `chromeStore` (sidebarCollapsed + paneInfoCollapsed) + localStorage 영속 (`gtmux-chrome` 키). plan 0005 §10 O1 정합.
- `$lib/chrome/RailToggle.svelte` — 16×64 thin collapse button. `side: 'left' | 'right'` prop. 패널 외측 anchor (open 시 left/right 264/284px) ↔ viewport edge (collapsed 시 left/right 8px) 전환. chevron 회전 transition.
- `$lib/chrome/PaneInfoPanel.svelte` — 우측 268px floating. M 의 첫 Panel 의 *Identity* (pane_id / label / id) + *Geometry* (x/y/w/h/z) + *State* (visible/locked/minimized/alive) 표시. read-only v0. 미선택 시 "No selection" empty state. dead 표시 시 warning 색조.

**Sidebar refactor**:
- `<aside class="sidebar" class:collapsed>` 추가 — `collapsed: boolean` prop.
- CSS: `flex` 컬럼 sibling → `position: absolute; top: 8px; bottom: 8px; left: 8px`. radius-lg + shadow-md + z-side-panel.
- `.collapsed` 변종: `translateX(-(width+12px))` + opacity 0 + pointer-events none. transition `var(--motion-slow)`.

**+page.svelte rewrite**:
- workspace = `position: relative` overlay host.
- Canvas = `position: absolute; inset: 0` fill (캔버스가 가능한 최대 면적).
- Sidebar + PaneInfoPanel + RailToggle ×2 = absolute overlays.
- chromeStore.state.{sidebarCollapsed,paneInfoCollapsed} prop pass.
- 반응형 media query 재정의: `< 900px` 시 PaneInfoPanel auto-hide, `< 700px` 시 Sidebar auto-hide. 사용자 RailToggle 토글은 보존.

**검증**:
- svelte-check 0/0 (243 files — 신규 3 컴포넌트)
- vite build OK. CSS +4.07 KB (사이드 패널 chrome / shadow / transition). JS +6.13 KB (chrome store + 3 컴포넌트).

**시연**: 새 release binary 가 9999 listening. 강제 새로고침 후:
1. 좌측 248px Sidebar floating (Layer tree) + 좌측 가장자리 옆 RailToggle (◀)
2. 우측 268px PaneInfoPanel floating ("No selection" empty state) + 우측 가장자리 옆 RailToggle (▶)
3. Canvas 가 두 패널 *밑* 으로 fill — 패널 위치에서도 캔버스 panel 인터랙션 가능 (z-order)
4. 패널 선택 → PaneInfoPanel 에 속성 즉시 표시
5. RailToggle 클릭 → 패널 슬라이드 아웃 + RailToggle 이 viewport 가장자리로 이동 + chevron 180° 회전. 영속 (localStorage)

**알려진 잔여**:
- PaneInfoPanel 의 편집 컨트롤 (rename / lock toggle / visibility toggle) — 별도 phase
- Sidebar / PaneInfoPanel 의 *내용* (Layers 트리, Pane Info 섹션) 은 기능적 정합 — 시각 chrome 만 Figma adaptation
- 다중 선택 (M.size > 1) 시 PaneInfoPanel 의 표시 정책 미정 — 현재 *첫 선택만* 표시. ref §7.1 의 "shared properties" 처리 방식 별도 phase

### Stage F — Canvas chrome: HelpBar + ViewportCtrl + ContextMenu (완료)

ref/frontend-design §9 (Viewport ctrl) + §10 (Context menu) + §5.5 (Help bar) adaptation.

**신규 컴포넌트**:
- `$lib/chrome/HelpBar.svelte` — top-center pill (pointer-events:none). mono uppercase 11px kbd hints (`Space + drag` · pan / `⌘ + scroll` · zoom / `right-click` · menu). 720px 이하 hide.
- `$lib/chrome/ViewportCtrl.svelte` — bottom-center pill. `useSvelteFlow()` hook 으로 zoomIn / zoomOut / setViewport(zoom=1) / fitView 호출. 라이브 zoom % 라벨 + 100% 클릭 시 reset. ✕✕✕ divider 후 `M:N` 카운트 badge (accent 색).
- `$lib/chrome/ContextMenu.svelte` — right-click menu. fixed 위치 (좌표 클램프 — viewport bounds 침범 시 left/top 조정). outside-click / Esc 닫힘. items: Copy pane_id (clipboard write + toast 확인) / Close pane (CTRL kill-pane 발사, danger 색조) / 구분선 / Hide / Lock (placeholders, P1+ wire). `openAt({clientX, clientY, paneId?, panelId?})` 외부 트리거 메서드 export.

**Canvas wire**:
- `Canvas.svelte` 에 `onContextMenuRequest` prop 추가 — `+page.svelte` 가 ContextMenu ref 의 `openAt` 으로 wire.
- SvelteFlow 의 `onpanecontextmenu` + `onnodecontextmenu` 핸들러 추가 — `event.preventDefault()` 로 native 컨텍스트 메뉴 차단 + onContextMenuRequest 호출.
- node-context 시 `node.data.pane_id` 추출 → ContextMenu 의 Copy / Close 액션 wire.

**+page.svelte 변경**:
- `<SvelteFlowProvider>` 로 workspace wrap — ViewportCtrl 의 `useSvelteFlow()` 가 sibling 컴포넌트에서도 resolvable.
- HelpBar / ViewportCtrl / ContextMenu 3 컴포넌트 마운트 + ContextMenu 의 ref 를 Canvas 의 onContextMenuRequest 콜백에 wire.

**검증**:
- svelte-check: 0 errors / 0 warnings (246 files)
- vite build: CSS +3.92 KB (HelpBar pill + ViewportCtrl + ContextMenu styles). JS +6.14 KB (3 신규 컴포넌트 + SvelteFlowProvider context).

**시연**:
1. 캔버스 상단 중앙 — 'Space + drag · pan | ⌘ + scroll · zoom | right-click · menu' pill
2. 캔버스 하단 중앙 — `[−] [100%] [+] · [⊟] · M:0` 픽토그램 + 모노 zoom %
3. `+` / `−` 클릭 → 0.15s transition zoom
4. `100%` 클릭 → zoom 1 reset (pan 위치 유지)
5. `⊟` (Fit) → fitView 호출
6. 패널 선택 → `M:N` badge 라이브 업데이트
7. 캔버스 빈 영역 우클릭 → ContextMenu (Copy pane_id disabled, Close pane disabled, Hide/Lock toasts) + 좌표 클램프
8. 패널 위 우클릭 → 같은 메뉴이지만 Copy pane_id active (clipboard 쓰기) + Close pane active (CTRL kill-pane)

**알려진 잔여**:
- Hide / Lock 액션 wire — Stage G/E 의 PanelNode chrome 보강 + layout PUT 정책 결정 후
- `Space + drag` pan 명시 지원 — 현재 SvelteFlow 기본 panOnDrag=[1,2] (middle/right click pan). space-key activation 은 SvelteFlow API `panActivationKey="Space"` 로 도입 가능 — 별도 phase
- 키보드 단축키 (Cmd-N / Cmd-Shift-Q / Cmd-Shift-L) — ADR-0017 §D6 에 spec, 별도 phase

### Stage G — PanelNode close button + last-pane guard (S7-FE-CLOSE-GUARD, 완료)

CONTEXT.md §"Pane lifecycle invariant 의 UI 측 mirror" 정신 — *사후 recovery 가 아닌 사전 prevention*.

**panelsStore.removePanel(id)** 신규 — 시각 Panel 만 layout 에서 제거 (idempotent). 호출자가 후속 `putLayoutCommitCurrent` 로 disk 영속화 책임.

**PanelNode 변경**:
- header 의 `.panel-actions` 컨테이너로 badges + close 묶음
- `.panel-close` IconButton (16×16) — 인라인 SVG X. hover 시 danger bg + white fg.
- `liveCount = muxStore.panes.values().filter(p => p.dead !== true).length` derived
- `closeDisabled = liveCount <= 1 || paneNumeric === null` — 마지막 살아 있는 child 일 때 비활성화
- `closeTooltip` = liveCount<=1 시 *"Last live pane — use Session shutdown"*, 외 *"Close panel"*
- `onmousedown` 에서 e.stopPropagation() — 드래그 hijack 방지
- `onClose`:
  1. `panelsStore.removePanel(data.id)` — 즉시 시각 제거 (반응성 UX)
  2. `putLayoutCommitCurrent(token)` — disk 영속화 + LAYOUT_CHANGED broadcast
  3. `sendCtrl(client, 'kill-pane', [paneNumeric])` — backend child shell SIGTERM
  4. response.ok false 시 toast (error). 실패해도 시각 제거는 유지 (사용자 의도)

**검증**:
- svelte-check: 0 errors / 0 warnings (246 files)
- vite build OK. CSS +0.55 KB, JS +1.5 KB

**시연**:
1. 패널 1개 — close 버튼이 disabled (회색, opacity 0.35). hover 시 tooltip "Last live pane — use Session shutdown"
2. New Panel 로 추가 → 두 패널 모두 close 버튼 enabled
3. X 클릭 → 패널 즉시 시각 제거 + backend child 종료 + (다른 탭이 있다면) LAYOUT_CHANGED broadcast 로 동기화
4. 마지막 1개 남으면 다시 disabled 상태로 복귀

**알려진 잔여**:
- xterm 정리 — 패널 제거 시 `unregisterPaneOut` 호출 timing 검토 (현재 컴포넌트 unmount 시 자동, 영향 없음 예상)
- close 도중 race — 사용자가 매우 빠르게 같은 패널 X 두 번 클릭 시 `closing` flag 가 보호 (single 액션)
- multi-select 시 일괄 close — P1+. 현재는 단일 패널만

### Stage I — S7-FE-AUTOMOUNT (frontend pane-spawned cascade PUT) — 완료

ADR-0015 신규 동반. dispatcher 의 `pane-spawned` NOTIFY hook 이 layout 에 없는 pane 발견 시 자동 cascade PUT.

**ADR-0015 정본**: backend 는 PTY spawn 만 책임, frontend dispatcher hook 이 layout PUT 책임. backend 의 layout schema 무지 유지. cascade 좌표 = origin + N×40px (N = `panelsStore.panels.size`). NewPanelButton 의 viewport-center 좌표 path 는 명시 클릭 mental model 보존 — `appendPanelIfMissing(paneId, { coords })` helper 가 두 path 의 idempotent 진입점.

**신규 코드**:
- `$lib/http/layout.ts` — `appendPanelIfMissing(paneId, { token, coords? })` 신규. layout 에 같은 `pane_id` panel 존재 시 *no-op return*. 없으면 cascade or 명시 coords 로 `putLayoutAppendPanel`. 412 rebase 후 재가드 (다른 탭이 먼저 추가 시 흡수).
- `$lib/ws/dispatcher.svelte.ts` — `AutoMountHandler` 타입 + `setAutoMountHandler` 신규 hook. `handleNotifyMirror` 의 `pane-spawned` case 가 호출.
- `+page.svelte` 의 `onMount` — `setAutoMountHandler((paneId) => appendPanelIfMissing(paneId, { token }))`. `onDestroy` 에서 `setAutoMountHandler(null)` cleanup.
- `NewPanelButton.svelte` — 기존 `putLayoutAppendPanel` 직접 호출 → `appendPanelIfMissing(paneId, { token, coords: viewportCenter })` 교체. 동일 idempotent 가드 통과.

**race 시나리오**:
1. 사용자 New Panel 클릭 → CTRL new-pane 송신
2. backend 가 child spawn + `pane-spawned` NOTIFY broadcast
3. dispatcher 의 `pane-spawned` hook → `appendPanelIfMissing(paneId)` 호출 (cascade 좌표)
4. *동시에* NewPanelButton 의 `onclick` 도 `appendPanelIfMissing(paneId, { coords: center })` 호출
5. 둘 중 *먼저 도착한 쪽* 이 PUT 성공 → 좌표가 그쪽 값으로 잠김
6. 나중 도착 쪽 — `panelsStore` 가 (broadcast 후 hydrate 되어) 이미 같은 pane_id panel 보유 → idempotent 가드 통과 *no-op return*

**검증**:
- svelte-check: 0 errors / 0 warnings (246 files)
- vite build OK. CSS 변동 없음, JS +0.22 KB

**시연 (다중 탭)**:
1. 브라우저 탭 A 에서 New Panel 클릭 → panel 추가 + LAYOUT_CHANGED broadcast
2. 브라우저 탭 B 가 broadcast 수신 → fetchLayoutAndHydrate → 같은 panel 표시
3. *외부 spawn* (현재 비범위) 시점 도래 시 — 별도 코드 없이 dispatcher hook 이 자연 흡수

**알려진 잔여**:
- D3 의 race 좌표 우선순위 *실 빈도 측정* — Stage K 시연
- SSoT `canvas-layout-schema.md` 의 *Panel.pane_id 유일성* 명시화 (현재 frontend 가드만) — 별도 SSoT PR

## 변경 이력

- 2026-05-15: 초안 — ref/frontend-design 분석 (0029) + ADR-0016 v2 amend 후. plan 0004 의 §3, §6 supersede.
- 2026-05-15: §9 추가 — 사용자 피드백 5건의 조기 진입 매핑 (canvas theme / selection logic / single-multi visual / NodeResizer / Sidebar row).
