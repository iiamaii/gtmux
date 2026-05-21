# ADR-0017: Layout grid + chrome 컴포넌트 책임 경계

- 상태: Accepted (2026-05-15)
- 일자: 2026-05-15 (Proposed + Accepted, plan 0005 Stage C dispatch 정합)
- 결정자: agent (frontend-architect role)
- 근거 plan: `docs/plans/0005-figma-layout-overhaul.md` §3 (레이아웃 그리드) + §6 (Feature surface 설계)
- 근거 분석: `docs/reports/0029-frontend-design-ref-analysis.md` (ref/frontend-design 흡수)
- 관련 ADR: ADR-0012 (Frontend stack — Svelte 5 + Vite), ADR-0016 (Design tokens + lucide-svelte), ADR-0013 (PTY direct, no tmux — KillSession variant 정의)
- 관련 SSoT: (없음 — 본 ADR 이 사실상 layout-grid SSoT 역할)

## 맥락

ADR-0016 이 *디자인 시스템 foundation* (tokens, light/dark, iconography) 을 잠갔다. plan 0005 가 Figma adaptation 의 11 stage 로드맵을 정의했다. 본 ADR 은 그 위에서 **(a) 6 영역 grid 의 정형 spec**, **(b) 각 chrome 컴포넌트의 책임 경계**, **(c) Session lifecycle UI flow** 세 차원을 잠근다.

현 시점 (plan 0005 Stage C 진입) frontend 의 layout 은:
- `+page.svelte` 가 `app > {ReconnectBanner, Toolbar, workspace > {Sidebar, canvas-pane}}` flexbox
- Toolbar 는 40px stopgap chrome (brand + ThemeToggle 만)
- Sidebar 는 248px docked (left)
- canvas-pane 는 fluid (SvelteFlow + NewPanelButton overlay)

본 ADR 진입 후 목표:
- Banner (32px, conditional) + **Titlebar (44px)** + workspace
- workspace 안에 **floating Sidebar (248px @ left:8 top:8 bottom:8)** + Canvas + **floating PaneInfoPanel (268px @ right:8 top:8 bottom:8)** + **bottom-center ViewportCtrl pill** + **top-center HelpBar pill** + **right-click ContextMenu**

## 결정 (Decisions)

### D1. Grid 영역 (6 levels)

`+page.svelte` 는 다음 grid:

```css
.app {
  display: grid;
  grid-template-rows: auto var(--layout-titlebar-h) 1fr;
  /* row 1 = ReconnectBanner (conditional, 32px or 0)
   * row 2 = Titlebar (44px fixed)
   * row 3 = workspace (1fr remaining) */
  width: 100vw;
  height: 100vh;
  overflow: hidden;
}

.workspace {
  position: relative;
  overflow: hidden;
}
```

workspace 안의 모든 chrome (Sidebar / PaneInfoPanel / ViewportCtrl / HelpBar / Canvas) 은 `position: absolute` 기반 — Figma 의 floating panel 모델 정합 (`ref/frontend-design/SPEC.md` §1.3).

### D2. 컴포넌트 책임 매트릭스

| 컴포넌트 | 위치 | 책임 |
|---|---|---|
| `ReconnectBanner.svelte` | row 1 (conditional) | WS 끊김 grace 1s 후 close code 분기 메시지 (D21 c2) |
| **`Titlebar.svelte`** | row 2 (44px) | 좌: SessionMenu 트리거 (≡ kebab) + "Workspace" 탭. 중앙: "gtmux · `<session>` · `<bind>:<port>` · `<mode>`". 우: ThemeToggle + FocusToggle |
| **`SessionMenu.svelte`** | Titlebar 의 dropdown | Session shutdown / Rotate token (P1+) / About (Modal) |
| **`ShutdownModal.svelte`** | overlay (z=2000) | confirm modal — 활성 pane 수 + session 이름 + layout 보존 안내 + [Cancel] [Shutdown] |
| **`FocusToggle.svelte`** | Titlebar 우측 | `ephemeralStore.focusMode` 토글 (P1+ 실제 wire) |
| `Sidebar.svelte` | workspace absolute left | Layer tree (현 구현 유지, Stage E 에서 floating chrome 추가) |
| **`PaneInfoPanel.svelte`** | workspace absolute right (Stage E) | 선택된 Panel 의 pane_id / label / locked / visibility 표시 |
| **`ViewportCtrl.svelte`** | workspace bottom-center pill (Stage F) | zoom in/out/100%/fit + M count badge |
| **`HelpBar.svelte`** | workspace top-center pill (Stage F) | 단축키 hint (`⌘N` `space+drag` `⌘+wheel`) |
| **`ContextMenu.svelte`** | workspace fixed (right-click, Stage F) | Copy pane_id / Close pane / Hide / Lock |
| `Canvas.svelte` | workspace fill | SvelteFlow + NewPanelButton overlay (현 구현, NewPanelButton 은 Stage D 의 Toolbar2 흡수 검토) |

### D3. Session lifecycle UI flow (KillSession)

1. **사용자가 Titlebar 의 ≡ 클릭** → SessionMenu dropdown 열림
2. **"Session shutdown" 선택** → ShutdownModal 표시 (focus trap, Esc / backdrop 닫힘)
3. **사용자가 "Shutdown" 클릭** → frontend 가 CTRL `{cmd:"kill-session"}` 발사
4. **backend 가 ack** (CTRL `ok=true`) **+ self-SIGTERM** (ADR-0013 D10 amend)
5. **axum graceful_shutdown** → WS close (code 1000 normal)
6. **frontend 의 ReconnectBanner** 가 1000 normal 을 *재연결 시도 없는* 분기로 처리 (sketch §7.4 정합). banner 내부에 "Session ended" 메시지 (Stage C 의 banner 분기 추가).
7. **사용자 새 `gtmux start --session demo`** 로 재진입

### D4. Session 이름 surface

backend 의 `bootstrap_handler` 의 inline JS 가 `sessionStorage.gtmux_session` 에 session 이름 주입. frontend 는 `sessionStorage.getItem('gtmux_session')` 으로 읽음 (token 과 동일 패턴, ADR-0003 D13.1 정합). 별도 GET /api/session 등 API 추가 없음 — 단일 진실 (bootstrap landing) 유지.

### D5. ThemeToggle / FocusToggle 위치

둘 다 **Titlebar 우측 actions 영역**. SessionMenu (좌측 kebab) 와 좌우 분리 — destructive 액션 (Shutdown) 과 cosmetic 토글 (Theme / Focus) 의 *시각 거리* 확보 (UX 안전성).

### D6. 키보드 shortcut

본 ADR 은 Stage C 도입 chrome 의 shortcut 만 잠근다. CommandPalette + 전역 shortcut 등록 시스템은 별도 ADR (P1+):

- `Cmd-Shift-Q` / `Ctrl-Shift-Q` → SessionMenu 가 ShutdownModal 직접 트리거 (단축키 — confirm 단계는 여전히 modal)
- `Esc` → 열린 Modal / Dropdown / ContextMenu 닫음 (현 Modal / Dropdown 가 이미 내부 처리)
- `Cmd-Shift-L` / `Ctrl-Shift-L` → ThemeToggle.toggle() (theme.svelte.ts 의 toggle 호출)

### D7. Floating panel chrome (Stage E 잠금)

본 ADR 은 Titlebar / SessionMenu / ShutdownModal / FocusToggle (Stage C) 까지만 코드 진입. Stage E (Sidebar floating + PaneInfoPanel) 와 Stage F (ViewportCtrl / HelpBar / ContextMenu) 의 구현은 별도 PR — 본 ADR 의 §D2 매트릭스가 spec.

floating panel 공통 chrome:
- `position: absolute` + `top: 8px; bottom: 8px`
- `background: var(--color-surface); border-radius: var(--radius-lg); box-shadow: var(--shadow-md);`
- transition: `transform 0.25s cubic-bezier(.4,0,.2,1), opacity 0.2s` — collapse rail 정합
- `z-index: var(--z-side-panel)` (=20)

### D8. 반응형 (breakpoint)

본 ADR 은 fluid 레이아웃 채택 (ref §1.1 의 1920×1080 fixed frame *비채택*). breakpoint:

- viewport `< 800px`: Sidebar 폭 248 → 180px
- viewport `< 600px`: Sidebar `display: none` (사용자가 RailToggle 로 복귀 — Stage E)
- viewport `< 400px`: Titlebar 의 중앙 session info 가 ellipsis 또는 hidden (action 우선)

PaneInfoPanel (Stage E) 도 동일 정책.

## 거절된 대안 (Rejected)

- **R1. ref 의 1920×1080 fixed frame + scale-to-fit** — 데모 프로토타입 용. gtmux 는 데스크탑 워크스페이스 — viewport 의 모든 픽셀이 *실제 작업 면적*. fixed-frame 의 scale 결과 미니어처 효과는 부적합. 거절.
- **R2. Titlebar + 별도 Toolbar 2-row 헤더** (ref §1.2) — gtmux 의 도구 셋이 ref 와 다름 (도형 도구 없음). Toolbar 의 별도 56px row 가 *빈 공간 90%* 가 되어 비효율. Titlebar 단일 row 에 Workspace 탭 + 액션 통합. Stage D 가 Toolbar2 도입 시 본 결정 재방문 (현재는 거절).
- **R3. SessionMenu = 우상단 빨강 "Shutdown" 버튼** — destructive 액션이 *항상 가시* 라 실수 클릭 위험. kebab dropdown 한 단계 멀리. 거절.
- **R4. 타이핑 confirm ('demo' 입력 강제)** — single-user 빠른 워크플로 마찰. 활성 pane 수 + session 이름 표시로 인지 보조 충분. 거절.
- **R5. backend POST /api/shutdown HTTP endpoint** — durable/ephemeral 분리 (ADR-0002 D9) 정합. 그러나 (a) WS 가 닫혀 있어도 동작해야 한다는 시나리오 부재 (b) Session shutdown 은 WS subprotocol 이 통과한 같은 인증 surface 에서 동작. WS CTRL 채택. 거절.
- **R6. Session 이름을 GET /api/session 별도 endpoint** — bootstrap 단계에서 이미 server-side 가 알고 있으므로 inline 주입이 자연. 추가 round-trip 회피. 거절.

## 결과 (Consequences)

### 긍정

- **Figma adaptation 의 시각 정합** — ref 의 design language (floating panels, 44px titlebar, dashed focus) 를 gtmux 의 도메인 (단일 사용자 + 터미널 워크스페이스) 에 매핑.
- **chrome 컴포넌트 분리** — 각 영역이 자체 책임 (Titlebar / Sidebar / Canvas / Modal). Stage C ~ F 가 PR 단위로 독립.
- **Session lifecycle 의 UI/Backend 정합** — D3 의 7-step flow 가 ADR-0013 D10 amend 의 KillSession variant 와 1:1 매핑.
- **반응형 fluid layout** — desktop 워크스페이스의 *모든 픽셀 활용*, 좁은 viewport 도 자연 적응.

### 부정 / 비용

- **신규 컴포넌트 7건** (Stage C ~ F) — 코드 면적 +1,000 LOC 예상.
- **bootstrap 인라인 JS 의 sessionStorage 키 2개** — `gtmux_token` + `gtmux_session`. 키 추가 시 본 ADR amend.
- **Stage C 의 KillSession 흐름이 backend 의 graceful_shutdown 에 의존** — 만약 backend 가 SIGTERM 후 5초 grace 내에 마무리 못하면 사용자가 "stuck shutdown" 경험. ADR-0014 D2 의 200ms × N pane 의 *상한 보장* 으로 5초 내 충분 (50 pane × 200ms = 10s 가 worst, 단 병렬 처리되므로 실제 ≤ 200ms total).

### 후속 작업

- **Stage C 진입**: ADR-0013 D10 amend (✅ 완료) + ADR-0017 (본 ADR ✅) + 4 신규 컴포넌트 + +page.svelte rewire
- **Stage E**: Sidebar floating chrome refactor + PaneInfoPanel 신규
- **Stage F**: ViewportCtrl / HelpBar / ContextMenu
- **Keyboard shortcut**: 전역 등록 시스템 별도 ADR (P1+)

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태 / 웹 상태 분리 | **N/A** — chrome layout 전용 |
| 2 | tmux-native vs web-only 분기 | **N/A** |
| 3 | tmux Layout ≠ Canvas Layout | **N/A** |
| 4 | 보안 기본값 | **PASS** — Session 이름은 인증 통과 후 (HttpOnly cookie / Authorization Bearer) 에만 noindow inject. KillSession 은 WS subprotocol 인증 통과 후만 도달. |
| 5 | control mode 사용 | **N/A** — control mode 는 ADR-0013 으로 폐기 |

## 미해결 항목 (Open)

- **O1.** Titlebar 의 중앙 session info 의 "Saved 2m ago" 류 metadata 미정 — layout PUT 의 마지막 timestamp 를 표시? P1+ 검토.
- **O2.** FocusToggle 의 실제 wire — ephemeralStore.focusMode 가 이미 정의되어 있으나 backend broadcast 흐름이 미배선. Stage C 는 *UI 만* 도입, 실제 effect 는 별도 phase.
- **O3.** 다중 viewport (window resize) 시 Modal / Dropdown 의 위치 재계산 — Modal 은 viewport center 고정, Dropdown 은 anchor 상대 위치라 자연 정합. window resize event 별도 처리 불필요 *예상* — Stage C 의 시연에서 검증.
- **O4.** Locale — Titlebar / Modal 의 텍스트는 영문 ("Shutdown", "Cancel", "Workspace"). KO/EN 토글은 별도 ADR (현재 KO 1언어 기본, sketch §"Language conventions").

## Amend (2026-05-16 ①) — ~~Layers / Terminals 분리 + header fold 모델~~

> **Note (2026-05-16 ②):** 본 amend 의 vertical split 결정은 같은 날 ②번 amend (tab merge) 로 회수되었다. 다음 항목 중 **유효 잔존**: header fold (`PanelFoldButton`) + `RailToggle` 의 *expand-only* 역할 격하. **회수**: 두 좌측 panel 의 vertical split (60/40) + `chromeStore.terminalsCollapsed` 필드 + 좌측 RailToggle 의 2 anchor stack. ②번 amend 본문 참조.

### 맥락 (당시)

§D2 의 매트릭스에서 `Sidebar.svelte` 의 책임은 "Layer tree" 로 못 박혀 있었다. ADR-0021 D7 (server-wide Terminal pool) 이 land 한 뒤 `TerminalListSection` 이라는 sub-section 형태로 Sidebar 의 *하단* 에 흡수되어 한 floating panel 안에 두 개의 서로 다른 도메인 (layer tree + terminal pool list) 이 동거하게 되었다. 본 amend ① 은 이를 *별 floating panel 2개* 로 분리하고 fold UX 를 panel header 내부로 일원화했다.

### 결정 (① 시점)

- ~~**D2 amend.** Sidebar 는 다시 "Layer tree only". Terminal pool list 는 별도 floating panel `TerminalsPanel.svelte` 로 분리.~~ (②번 amend 로 회수)
- ~~**D7 amend (좌측 column vertical split).** 두 좌측 panel 의 위치:~~
  - ~~`Sidebar` — `top: 8, bottom: var(--layout-sidebar-layers-bottom), left: 8, width: 248px` (workspace 상단 ~60%).~~
  - ~~`TerminalsPanel` — `top: var(--layout-sidebar-terminals-top), bottom: 8, left: 8, width: 248px` (workspace 하단 ~40%, 8px gap).~~
- **D7 amend (header fold + edge rail 의 책임 분리)** — 유효 잔존:
  - `PanelFoldButton.svelte` — panel 의 header 우측에 위치하는 fold 아이콘 (▶/◀ chevron). panel 이 *펼쳐진* 상태에서만 보이며 collapse 트리거.
  - `RailToggle.svelte` — viewport edge 의 thin (16×64) 버튼. panel 이 *접힌* 상태에서만 mount (`{#if collapsed}` gate). expand 트리거 전용. (좌측은 ②번 amend 의 LeftPanel 자체 rail bar 가 흡수, 우측 PaneInfoPanel 만 사용)
- ~~**chromeStore amend.** `terminalsCollapsed` 필드 추가.~~ (②번 amend 로 회수 — `leftPanelTab: 'layers' | 'terminals'` 로 교체)

## Amend (2026-05-16 ②) — Layers / Terminals tab merge + collapsed rail with tab icons

### 맥락

①번 amend 직후 사용자 검토 결과: 좌측을 *별 panel 2개로 분리* 한 형태가 ref/frontend-design 의 `panel-tabs` 패턴과 어긋난다는 피드백. ref 의 `.side-panel.left` 는 한 panel 안에 `Layers / Assets / Pages` 같은 가로 탭으로 컨텐츠를 전환한다. Vertical split 60/40 은 (a) 두 도메인의 디스플레이 영역을 동시에 고정 점유해 *비활성 도메인 영역이 낭비* 되며 (b) 좁은 viewport 에서 두 list 가 모두 cramped 한다.

②번 amend 는 ①번 amend 의 *분리* 결정을 회수하고 ref 패턴 정합으로 통합한다. 단, ①번 amend 의 header fold 결정 (PanelFoldButton + RailToggle expand-only) 은 *유효 잔존*.

### 결정

- **D2 amend ②.** 좌측은 단일 floating panel `LeftPanel.svelte`. 내부에 가로 탭 2개 (`[Layers | Terminals]`) — ref/frontend-design `panel-tabs` 패턴 정합. 컨텐츠는 view 컴포넌트로 분리:
  - `LayerTreeView.svelte` — 기존 `Sidebar.svelte` 의 layer tree 컨텐츠. outer chrome (aside / fold / floating absolute) 제거.
  - `TerminalListView.svelte` — 기존 `TerminalsPanel.svelte` 의 terminal pool list. 동일하게 outer chrome 제거.
  - 두 view 는 stateless container `<div>` 만 — 외곽 chrome 은 `LeftPanel` 단독 owner.
- **D7 amend ② (collapsed rail bar with tab icons).** Fold 시 28px wide vertical rail 가 panel 자리를 차지:
  - 최상단: expand chevron 버튼 (▶) — 클릭 시 `chromeStore.toggleSidebar()` (현재 active tab 유지).
  - 그 아래 separator + 각 탭 아이콘 (Layers · layers stack icon / Terminals · terminal prompt icon).
  - 탭 아이콘 클릭 = `chromeStore.setLeftPanelTab(tab)` 호출 — panel 확장 + 해당 탭 선택 (1 click UX, 2-step 회피).
  - 활성 탭 아이콘은 accent 색 + tint 배경으로 강조 — collapsed 상태에서도 "어느 탭이 다음에 열릴지" 시각 단서.
- **chromeStore amend ②.** `terminalsCollapsed` 필드 *회수*, `leftPanelTab: 'layers' | 'terminals'` 필드 추가. `setLeftPanelTab(tab)` action 추가 — tab 변경 시 자동으로 `sidebarCollapsed=false` 도 set. localStorage schema 변경, 누락/이전 schema 는 default (`'layers'` + `sidebarCollapsed=false`) 로 graceful.

### 거절된 대안 (②번 시점)

- **R10.** ①번 amend 유지 (vertical split 60/40) — viewport 활용 비효율 + ref 패턴 일탈. 거절.
- **R11.** 두 탭 + 세로 split 동시 지원 (`mode: 'split' | 'tabs'`) — toggle UI 가 chrome 의 메인 책임이 아닌데도 *2가지 모드* 를 사용자에게 노출하면 인지 부담 증가. 거절.
- **R12.** Collapsed rail 에서 탭 아이콘 클릭 시 *expand 만* 하고 탭은 그대로 — 사용자가 의도한 탭이 별도라면 2-step (expand → tab) 이 필요. 그 의도 없이 expand 만 원할 땐 chevron 버튼이 따로 있으므로 *탭 아이콘 = expand + select* 가 명확. 거절.
- **R13.** Collapsed rail 을 RailToggle.svelte 의 vertical anchor stack 으로 재사용 — RailToggle 은 *단일 button* 의 자세히 정의되어 있어 multi-icon rail bar 구현 시 책임 경계 흐려짐. `LeftPanel` 안의 self-contained rail markup 으로 처리. RailToggle 은 우측 PaneInfoPanel 용으로 단순화 유지. 거절.

### 결과

- 긍정: ref 의 `panel-tabs` 패턴 정합 → 시각 일관성. 좁은 viewport 에서도 두 list 가 cramped 하지 않음. Collapsed rail 의 탭 아이콘이 1-click UX 로 expand + select 를 합쳐 마찰 ↓.
- 부정: ①번 amend 의 코드 변경 (`TerminalsPanel.svelte`, `--layout-sidebar-{layers-bottom,terminals-top}` 토큰) 을 같은 날 회수 — 단기 churn. `Sidebar.svelte` 의 파일명을 `LayerTreeView.svelte` 로 rename — import 경로 1회 갱신.
- 후속:
  - 탭 추가 가능성 — Assets / Pages 등 ref 의 추가 탭이 필요하면 `LeftPanelTab` union 확장 + view 컴포넌트 추가.
  - 키보드 shortcut — G26 shortcutRegistry 일원화에서 `Cmd+Shift+L` (left toggle), `Cmd+1` / `Cmd+2` (탭 선택) 같은 단축키 wire.

## Amend (2026-05-16 ③) — RightPanel parity (우측 panel 도 동일 패턴)

### 맥락

②번 amend 에서 좌측만 `panel-tabs` + collapsed rail 로 통일했고 우측 `PaneInfoPanel` 은 ①번 amend 의 *기존 header + RailToggle* 모델을 그대로 유지하고 있었다. 좌·우 chrome 패턴이 비대칭이라 시각 일관성이 깨지고, 향후 ref 의 `Design / Prototype / Inspect` 같은 우측 탭 확장을 도입하려면 구조 재작업이 필요했다. 본 amend ③ 은 우측을 좌측과 동일한 모양으로 통일한다 (지금은 단일 탭, chrome 만 정합).

### 결정

- **D2 amend ③.** 우측은 `RightPanel.svelte` 단일 floating panel. 내부 컨텐츠는 `ItemInfoView.svelte` (기존 `PaneInfoPanel.svelte` 의 컨텐츠) 가 첫 (그리고 현재 유일한) 탭 `Inspect`. ref/frontend-design 의 우측 `Design / Prototype / Inspect` 3 탭 자리는 *기능 도입 시* 추가 — 단, chrome 은 그 자리를 이미 마련.
- **D7 amend ③ (우측 collapsed rail).** 우측 panel 도 28px wide vertical rail. `RightPanel.svelte` 내부에 self-contained — 좌측 `LeftPanel.svelte` 의 `.left-rail` 과 mirror:
  - 최상단: expand chevron 버튼 (◀, viewport interior 방향) — 클릭 시 `chromeStore.togglePaneInfo()`.
  - 그 아래 separator + 탭 아이콘 1개 (Inspect — info circle icon).
  - 탭 아이콘 클릭 = `chromeStore.setRightPanelTab(tab)` — panel 확장 + 탭 선택 (좌측과 동일 UX).
- **chromeStore amend ③.** `rightPanelTab: 'inspect'` union (현재 단일 값) + `setRightPanelTab(tab)` action 추가. localStorage schema 확장 — 이전 schema 누락 시 default (`'inspect'` + `paneInfoCollapsed=false`) graceful.
- **`RailToggle.svelte` 폐기.** 더 이상 사용처 없음. 두 panel 모두 자체 collapsed rail bar 를 가짐. 파일 삭제.
- **파일 rename.** `PaneInfoPanel.svelte` → `ItemInfoView.svelte` (컨텐츠 view 책임만), `RightPanel.svelte` 신규 (chrome owner).

### 거절된 대안

- **R14.** 우측은 단일 탭이므로 탭 chrome 생략 — 좌·우 비대칭 그대로 두면 시각 인지 부담 + 향후 탭 추가 시 chrome 재작업 비용. 거절.
- **R15.** RailToggle 을 유지하되 우측만 사용 — 두 panel 의 collapsed UX 가 다른 형태가 됨 (좌측은 28px rail with tab icons, 우측은 16×64 thin chevron-only button). 일관성 손실. 거절.

### 결과

- 긍정: 좌·우 chrome 완전 대칭 — 학습 부담 없음. 미래 우측 탭 확장이 *additive* (markup 변경 0, store union 확장만). `RailToggle` 폐기로 컴포넌트 면적 감소.
- 부정: ①번 amend 의 `--layout-sidebar-{layers-bottom,terminals-top}` 토큰은 이미 ②번에서 정리되었으나, 그 외에도 ①번 amend 가 만든 단명 코드 (RailToggle 의 multi-anchor stack) 가 ③번 시점에 완전히 제거 — 어느 시점부터의 git history 가 churn 으로 보임.
- 후속: 우측 탭 확장 (Design / Prototype / Inspect 등) 은 별도 ADR — 본 amend 는 chrome 만, 새 컨텐츠는 P1+.

## Amend (2026-05-16 ④) — SettingsOverlay (G19) + shortcutRegistry + themeStore system mode

### 맥락

frontend-handover-v2 의 Stage 7 P1 매트릭스 (FE-8 / G19 / G26 / G27) — Settings overlay + shortcut registry + theme system mode — 가 미진행. §D6 의 키보드 단축키 결정도 그 시점엔 컴포넌트 외부에 산재된 `addEventListener` 였다. 본 amend ④ 는 이 셋을 chrome 차원에서 정합 ship 한다.

### 결정

- **D6 amend ④ (keyboard shortcut routing).** 전역 keydown 매트릭스를 `shortcutRegistry.svelte.ts` 라는 단일 dispatcher 로 일원화:
  - `register({ key, meta?, ctrl?, alt?, shift?, handler, description, category, allowInEditable?, allowInXterm? })` API.
  - Editable focus (`INPUT/TEXTAREA/SELECT/contenteditable`) + xterm focus (`xterm-helper-textarea`) 가드 자동 — modifier 가 있으면 default `true`, plain key 는 default `false`. 호출자가 override 가능.
  - `escRouter` 는 그대로 유지 — Esc 의 *priority chain* (inline-edit > modal > unmaximize > tool > select) 이 flat keycombo table 로 매핑되지 않음. 두 시스템은 협조 (Esc → escRouter / 그 외 → shortcutRegistry).
  - 기존 `zShortcuts` 가 첫 consumer — 4 z 액션을 registry register 로 마이그레이션 (직접 listener 폐기).
  - 신규 `chromeShortcuts` — `Cmd+Shift+L` (LeftPanel toggle) / `Cmd+Shift+I` (RightPanel toggle) / `Cmd+,` (Settings overlay). 각각 macOS / Windows 변형 동시 등록.
  - `Cmd-Shift-L = ThemeToggle` (원안 §D6) 는 *회수* — handover-v2 의 P1 매트릭스가 같은 키를 sidebar toggle 로 지정. ThemeToggle 은 Settings overlay 의 Theme section 안으로 흡수, 단축키 없음.
- **D2 amend ④ + 신 D9 (Settings overlay).** `SettingsOverlay.svelte` 신규 — full-screen overlay (880×640 max, viewport responsive). 왼쪽 section nav + 오른쪽 section pane:
  - `theme` (G27, ✅ 이번 amend)
  - `shortcuts` (G26 read-only matrix, ✅ 이번 amend)
  - `storage` / `auth` / `behavior` / `debug` — placeholder + BE endpoint 명시 ("Waiting on BE: ..."). BE wire 시 본 ADR 또는 별 ADR amend.
  - Auto-save 정책 — 각 control 의 change 가 즉시 persist (별도 [Save] 버튼 없음). Theme 은 `themeStore.setMode()` → localStorage. Shortcuts 는 read-only 라 save 없음.
  - 진입점 3개: SessionMenu 의 "Settings…" 항목 / `Cmd+,` (shortcutRegistry) / `settingsDialog.show()` 직접 호출 (다른 컴포넌트가 deep-link 가능).
  - `settingsDialog.svelte.ts` (store) — `open: boolean`, `section: SettingsSection`, `show(section?)`, `close()`, `toggle()`, `setSection()`.
- **D5 amend ④ (themeStore G27 system mode).** `themeStore` 의 user-choice 가 `Theme = 'light' | 'dark'` 에서 `ThemeMode = 'system' | 'light' | 'dark'` 로 확장:
  - `mode` = user choice, `resolved` = 실제 적용 theme (`mode==='system'` 시 `prefers-color-scheme` 따름).
  - `bindSystemListener()` — MediaQueryList 변경 시 `system` 모드일 때만 hot reload. `+page.svelte` onMount 에서 호출, onDestroy 에서 cleanup.
  - localStorage `gtmux-theme` schema = `'system' | 'light' | 'dark'`. 이전 schema (`light/dark`) 와 호환 — `resolveInitialMode` 가 graceful fallback.
  - `index.html` 의 FOUC guard 도 같은 schema 정합 — `system` 이면 inline 에서 matchMedia 평가.
  - 기존 `themeStore.theme` getter / `set(theme)` / `toggle()` 시그니처 유지 — backwards-compat 보존.
  - xterm 의 theme hot reload (G27 xtermTheme adapter) 는 *다음 amend* — 본 amend 는 chrome theme 만.

### 거절된 대안

- **R16.** Esc 도 shortcutRegistry 로 통합 — priority chain 의 7 단계는 keycombo table 로 자연 매핑되지 않음. escRouter 가 더 적합. 거절.
- **R17.** Shortcuts 섹션에 in-place rebind 지원 — 사용자가 단축키를 직접 변경 가능. P1+ 의 영역, 본 amend 는 read-only matrix 만. 거절.
- **R18.** Settings 의 BE-dependent section 을 hide 또는 disabled — placeholder 가 *BE endpoint 명시* 로 작업 항목을 가시화하는 게 디버깅 / handover 에 더 가치 있음. 거절.
- **R19.** themeStore 를 그대로 두고 SettingsOverlay 에서 `system` 모드 시뮬레이션 — `system` 은 OS preference 의 live tracking 이 본질이라 store 가 책임지는 게 자연. 거절.

### 결과

- 긍정: G19 / G26 / G27 의 chrome 부분이 한 amend 로 ship → Stage 7 의 Settings 영역 진입점 확보. shortcutRegistry 가 단일 source 라 향후 Settings · Shortcuts section + 디버그 surface 단일. ThemeToggle 의 단축키 충돌 해소.
- 부정: BE-dependent section (Storage / Auth / Behavior / Debug) 가 placeholder — 사용자가 wire 미완료를 인지해야 함. 본 amend 는 의도적으로 *명시 표기* 로 처리.
- 후속:
  - xtermTheme adapter (G27) — xterm 인스턴스 별 theme 객체 hot reload.
  - BE: `GET/PATCH /api/settings`, `/api/file-path/*`, `/auth/rotate`, `/auth/set-password`, `/api/sessions/import` — Slice D 의 작업 항목.
  - Shortcut rebind UI (P1+) — registry 가 이미 `description` / `category` metadata 보유, override layer 추가만.

## Amend (2026-05-16 ⑤) — Dual-source adapter 제거 (sessionStore 단일 source)

### 결정

Stage 5 (multi-session) 이전의 legacy single-session store stack 을 FE 에서 완전 폐기. 본 amend 후 모든 chrome / canvas / sidebar / inspect surface 는 `sessionStore` 만 참조.

**제거 (legacy) 7 파일:**
- `lib/stores/panels.svelte.ts` — `panelsStore.panels: SvelteMap<id, Panel>` + `movePanel`/`resizePanel`/`removePanel`.
- `lib/stores/groups.svelte.ts` — `groupsStore.groups: SvelteMap<id, Group>`.
- `lib/stores/layout.svelte.ts` — `layoutStore.etag` + `schemaVersion` (legacy `/api/layout` v1 의 ETag 추적).
- `lib/stores/ephemeral.svelte.ts` — `m: SvelteSet` / `i: number|null` / `viewport` / `focusMode`. 모두 `sessionStore.{M, I, viewport, focusMode}` 로 통합.
- `lib/http/layout.ts` — `/api/layout` v1 GET/PUT + `If-None-Match`/`If-Match`/412 rebase + `fetchLayoutAndHydrate` / `putLayoutCommitCurrent` / `appendPanelIfMissing`.
- `lib/canvas/legacyNewPane.ts` — WS CTRL `new-pane` + race 매칭 + `appendPanelIfMissing` orchestrator.
- `lib/toolbar/MIndicator.svelte` — unused stub.

**Consumer 단순화 12 파일:**
- 모든 surface 의 `useSessionStore = $derived(sessionStore.active !== null)` 분기 13 곳 → 0 곳.
- WS dispatcher 의 0x80 LAYOUT_CHANGED / 0x81 M_CHANGED / 0x82 I_CHANGED / 0x83 VIEWPORT_CHANGED / 0x84 FOCUS_MODE_CHANGED 모두 sessionStore 직결 + `sessionStore.active === null` pre-attach race guard.
- `setLayoutRefetchHandler` / `setAutoMountHandler` export 폐기. `0x80 LAYOUT_CHANGED` 는 `mutateLayout()` 응답이 진실 source 이므로 debug log only no-op. `pane-spawned` NOTIFY 의 auto-mount hook 도 제거 (0x86 MOUNT_CASCADE / 0x88 TERMINAL_SPAWNED 가 multi-session 정식 경로).

**sessionStore 확장:**
- `focusMode: { enabled, targetPanelId }` 신규 필드 — ephemeralStore.focusMode 와 정합.
- `clear()` / `loadLayout()` 가 focusMode 도 reset.

### 이유

1. **Single source of truth**: canvas-layout-schema v2 단일 데이터 모델 정합. legacy v1 `panels[]` + `groups[]` 와 v2 `items[]` + `groups[]` 의 두 데이터 모델 공존이 정신적 부담 + bug source.
2. **Surgery 면적 ↓**: Layer list V2 multi-select / drag reorder/reparent 작업 (sessionStore 만 만지면 됨) 의 면적이 절반 ↓.
3. **Legacy `/api/layout` v1 의 FE 자취 폐기**: BE 측 핸들러도 다음 BE work package 에서 안전하게 제거 가능.
4. **WS 라우팅 정합**: 0x80~0x84 frame 들이 session-scoped 의도와 맞물려 sessionStore 로 직결 — ephemeralStore 의 server-wide 의미와 충돌하던 문제 해소.

### 검증

- `npm run check`: 290 files / 0 errors / 0 warnings.
- `npm run build`: 클린. dist gzip ~53 KB main + 71 KB svelteflow + 92 KB xterm.

### 후속

- ADR-0006 amend (FE 측 `/api/layout` v1 폐기 + BE 측 핸들러 제거 권고).
- ADR-0015 amend (Stage I auto-mount 의 `appendPanelIfMissing` 가 multi-session 의 0x86 MOUNT_CASCADE 로 대체됨을 명시).
- Layer list V2 multi-select + drag reorder (Stage 6 P1, 권장 다음 진입).

## Amend (2026-05-16 ⑥) — Layer list V2 (Multi-select + Drag reorder/reparent)

### 결정

LayerTreeView 에 Figma/Finder 식 다중 선택 + HTML5 drag reorder/reparent 추가. ADR-0024 amend 와 짝.

**Multi-select (Cmd/Ctrl/Shift):**
- Plain → setM([id]), anchor = id.
- Cmd/Ctrl → toggleM(id), anchor = id.
- Shift → visibleRangeIds(anchor, id) 의 inclusive range 일괄 setM (또는 Cmd+Shift 시 addToM). anchor 는 유지.

**Drag UX:**
- 모든 row draggable (Z mode 비활성 + locked 비활성).
- Drop position = mouseY ratio:
  - `< 0.25` → before (2px accent line top)
  - `> 0.75` → after (2px accent line bottom)
  - 중간 + group → inside (accent tint + dashed outline)
  - 중간 + panel → before/after 양분
- Multi-drag = dragged ∈ M 시 M 전체.
- Cycle 보호 = dragged group 의 descendants 제외.
- DragEnd 가 항상 dragState clear.

**Mutation:**
- Single `mutateActiveLayout` call 로 items.parent_id + groups.parent_id + groups.order atomic 갱신.
- Item sibling order 는 BE schema v3 (item.order 추가) 까지 id-sort 폴백.

### 이유

1. **Stage 6 마감**: §D2 D6 chrome 매트릭스의 "Layer list V2" 가치 (다중 선택 + 조직화) 완결. 묶음 B (Dual-source 제거) 직후 진입 — surgery 면적 절반 ↓.
2. **Z 분리 가시화**: drag UX 의 Z mode 비활성 정책이 ADR-0024 D1 의 "Tree order ≠ Z" 를 사용자에게 가시화.
3. **Bulk action 1차 가치**: M 에 묶은 set 을 한 번에 reparent — multi-select 의 실질 표현.

### 영향

- LayerTreeView 의 selectionAnchor / visibleRangeIds / drag handler 6개 신규.
- `.row.drop-before::before` / `.drop-after::after` / `.drop-inside` / `.dragging` CSS.

### 후속

- ADR-0024 amend (Tree order ≠ Z 의 drag UX 구체화) — 같은 PR.
- BE schema v3 — item.order field — `commitReparent` 의 정확 위치 적용 (현재 group 만 정확).
- Marquee selection — deferred (P2).

## Amend (2026-05-17 ⑦) — Basic editing shortcut matrix (D6 amend ⑤)

### 맥락

D6 amend ④ (2026-05-16) 가 `shortcutRegistry` 인프라 + chrome 단축키
(`Cmd+,` / `Cmd+Shift+L` / `Cmd+Shift+I`) 를 ship 했지만 *기본 편집
단축키* 는 산재된 상태:

- **Copy / Cut / Paste** — ADR-0030 D5 가 매트릭스 spec, 그러나 D6 hub
  + plan/handover 매트릭스에 미반영
- **Undo / Redo** — ADR-0028 가 spec, 같은 미반영
- **Select all** (Cmd+A) — OS-표준, *어디에도 spec 없음*

사용자 보고 "기본 단축키 (copy/paste/cut/전체 선택 등) 누락" 도 본
공백을 가시화. 본 amend 가 D6 hub 에서 cross-link + 신규 Cmd+A
결정으로 한 곳에 모은다.

### 결정 — D6 amend ⑤

**P0 매트릭스 추가 6 row** — 정책은 기존 ADR (0028 / 0030) 를 SoT 로
cross-link, 신규 결정은 (a) 만:

**(a) `Cmd/Ctrl+A` — Select all (신규)**
- **canvas focus** = `sessionStore.M` 에 active session 의 모든 *visible*
  item (locked 포함, hidden 제외 — Layer tree 의 visibility toggle 과
  정합).
- **LayerTreeView focus** = LayerTreeView 의 모든 row (group + item) M
  set.
- **xterm focus** = shell 의 select-all (xterm v6 default) 로 routing
  (`document.activeElement` 분기, G26 단축키 정책 정합).
- **editable focus** (`InlineEditField` / textarea / input) = editable
  의 OS-default select-all 로 routing.

**(b) `Cmd/Ctrl+C` / `Cmd/Ctrl+X` / `Cmd/Ctrl+V` — Copy / Cut / Paste**
- ADR-0030 D5 정본. `bindClipboardShortcuts` 가 registry consumer.
- Focus 분기 (canvas vs xterm vs editable) = ADR-0030 D5 의 xterm-routing
  정책 그대로.

**(c) `Cmd/Ctrl+Z` / `Cmd/Ctrl+Shift+Z` — Undo / Redo**
- ADR-0028 정본 (history stack capacity 50, session 별 독립).
- editable focus 시 editable 의 OS-default undo/redo 로 routing
  (브라우저 default 우선 — 본 결정 의 거절된 대안 R20 참조).

### 비범위 (P3) — OS-standard, 의도적 제외

| Shortcut | 사유 |
|---|---|
| `Cmd/Ctrl+S` | Auto-save 정합 (G19). Layout/Settings 모두 mutation → debounced PUT. 별도 명시 안 함. |
| `Cmd/Ctrl+P` | Print — 비범위. |
| `Cmd/Ctrl+W` | Tab close — browser default 우선. |
| `Cmd/Ctrl+R` | Reload — Session attach recovery (ADR-0019 D5.1 / D5.4) 가 자연 처리. |
| `Cmd/Ctrl+Tab` | App / tab switch — OS / browser 영역. |

### Find / search (`Cmd/Ctrl+F`) — P2 deferred

- 별 ADR 후보 — Cmd+K command palette (`plan-0007 §14.20.5.3` 의 TBD)
  와 분기 검토 필요.
- 본 amend scope 외 — handover-v3 §10.5.4 비범위 표에 명시.

### 거절된 대안

- **R18.** 별 ADR (Basic editing shortcut matrix) 신규 — D6 가 이미
  shortcut 의 SoT 라 D6 amend ⑤ 가 자연. 별 ADR 분리는 ADR 수의 과도
  증가. 거절.
- **R19.** Cmd+A 의 *canvas focus 일 때 hidden item 도 포함* — Layer
  tree 의 visibility toggle 의 의미 (= 시각적 hide) 와 충돌. 거절.
- **R20.** Cmd+Z / Cmd+Shift+Z 가 *editable focus 시에도 application
  undo* 발동 — InlineEditField 내부 typing 의 자연 undo (브라우저
  default) 우선이 사용자 mental model 정합. 거절.

### 결과

- ADR-0030 / ADR-0028 의 spec 이 D6 hub 에서 cross-link 형태로
  가시화 — 외부 agent 가 단축키 매트릭스를 한 곳에서 발견.
- Cmd+A 의 신규 결정 — `bindEditingShortcuts` (별 wire) 또는 기존
  `chromeShortcuts` 안 register 가 implementation 진입점.
- P3 비범위가 명시 → 향후 "왜 Cmd+S 안 됨?" 같은 사용자 질문에 ADR
  근거 직접 답.

### 산출물 / 정합 작업

- 본 amend = SoT register. 코드 land 는 별 batch:
  - `lib/keyboard/editingShortcuts.svelte.ts` (신규) 또는 기존 wire 의
    register 추가 — Cmd+A handler (focus 4 모드 분기)
  - `bindClipboardShortcuts` ship 확인 (ADR-0030 D5 의 wire)
  - `bindUndoShortcuts` ship 확인 (ADR-0028 의 wire)
- doc 정합 (본 amend 와 같은 batch):
  - `plan-0007 §14.20.5.2` P0 매트릭스 + `§14.20.5.4` 비범위 amend
  - `handover-v3 §10.5.2` P0 매트릭스 + `§10.5.4` 비범위 amend + `§13`
    변경 이력

## Amend (2026-05-19 ⑧) — Arrow nudge + Cmd+D Duplicate (D6 amend ⑥)

### 맥락

D6 amend ⑦ (2026-05-17, D6 amend ⑤) 가 *기본 편집 단축키* (Cmd+A/C/X/V/Z) 의
P0 매트릭스를 한 곳에 정합 register 했다. 그 batch 의 의도된 후속:

- **Arrow nudge** — 사용자가 선택된 item 을 keyboard 로 미세 이동.
  Figma / Sketch / Miro 의 표준 UX. 본 ADR 매트릭스에 미정의.
- **Duplicate (Cmd+D)** — Copy → Paste 두 단계의 1-key 단축. ADR-0030
  본문에 D11 신규로 등록 (2026-05-19 amend ②) — 본 hub 에서 매트릭스
  cross-link.

본 amend ⑧ 가 두 결정을 D6 hub 의 매트릭스에 추가한다.

### 결정 — D6 amend ⑥

**P0 매트릭스 추가** (D6 amend ⑤ 의 후속):

| Shortcut | 액션 | 조건 / 정본 |
|---|---|---|
| `↑` / `↓` / `←` / `→` | nudge 1px | M.size ≥ 1, locked item 은 source 에서 제외, editable / xterm focus 시 OS default |
| `Shift + ↑↓←→` | nudge 8px (grid unit) | 동일 |
| `Cmd/Ctrl + ↑↓←→` | nudge 64px (large step, 8 × grid) | 동일 |
| `Cmd/Ctrl + D` | Duplicate | ADR-0030 D11 cross-link, M.size ≥ 1, locked 제외 |

### Nudge 의 history grain — debounce 250ms

연속 nudge 의 *historyStore capture* 정책:

- 매 keydown 은 sessionStore 의 items 좌표를 *optimistic* 갱신 (DOM 즉시 반영).
- **250ms idle** (마지막 nudge 후) 시 단일 `applyMutation` PUT — historyStore
  가 1 entry capture (PRE-state = 첫 nudge 직전 layout snapshot).
- 실패 시 priorSnapshot 으로 store rollback (ADR-0028 D11.1 의 *optimistic
  failure rollback* 계약 정합).
- 사용자가 250ms 안에 다시 nudge 하면 timer reset — *한 번의 위치 조정 = 한
  entry* 의 사용자 mental model 정합.

근거: Figma / Sketch 의 batch 패턴. 매 keydown = 1 entry 면 50-cap 빠르게
소진 + BE PUT 부하. 250ms idle 의 batch 가 BE 부하 + history 효율 둘 다 충족.

### Focus 분기

- Arrow / Shift+Arrow / Cmd+Arrow / Cmd+D 모두 `allowInEditable: false` +
  `allowInXterm: false` 명시. xterm focus 의 Arrow 는 shell readline / cursor
  navigation 으로 자연 routing. editable focus 의 Arrow 는 input cursor 이동
  으로 routing. Cmd+D 의 browser default (Add bookmark on Chrome / Firefox)
  는 *consumed* — registry handler 가 `event.preventDefault` 호출 (browser
  default suppress).

### 거절된 대안

- **R21.** Arrow nudge 의 grain = 매 keydown 1 entry — stack 효율 낮음, Figma
  정합 일탈. 거절.
- **R22.** Cmd+Arrow = 1 viewport step (= zoom 의존 dynamic) — 사용자 인지
  부담. 64px 고정 (= 8 grid units) 채택. 거절.
- **R23.** Cmd+D 가 clipboard 도 갱신 (Sketch 패턴) — Figma 패턴이 사용자
  mental model 더 자연 (이전 copy 보존). 거절. ADR-0030 D11 의 정합 그대로.
- **R24.** Arrow 의 *모든 4 modifier 조합* (Alt, Shift+Alt 등) 매트릭스 확장
  — Alt 의 의미 미정 (Figma 는 Alt+Arrow = nudge with align). P1 후속.

### 결과

- Cmd+D 의 단일 shortcut 으로 in-place duplicate — workflow 마찰 ↓
- Arrow nudge 의 250ms debounce 로 history stack 효율 + Figma 정합 + BE PUT
  부하 ↓

### 산출물 / 정합 작업

- 본 amend = SoT register. 코드 land (별 commit):
  - `lib/keyboard/editingShortcuts.svelte.ts` 확장 — Cmd+D + 12 Arrow
    register (4 방향 × 3 modifier 조합)
  - `lib/keyboard/nudgeBuffer.svelte.ts` (또는 동일 모듈 내 helper) —
    debounce class + priorSnapshot 보관
- ADR-0030 D11 (2026-05-19 amend ②) — Duplicate 정본
- doc 정합 (deferred — 별 doc 작업 batch): `plan-0007 §14.20.5.X` + `handover-v3 §10.5.X`

## Amend (2026-05-19 ⑨) — Lock / Hide toggle + Group/Ungroup 매트릭스 cross-link (D6 amend ⑦)

### 결정 — D6 amend ⑦

**P0 매트릭스 추가**:

| Shortcut | 액션 | 정본 / 조건 |
|---|---|---|
| `Cmd/Ctrl + L` | Lock toggle (batch) | ADR-0018 D3 `locked` field. M.size ≥ 1. 모두 locked → 모두 unlock, 그 외 → 모두 lock (Figma 패턴). |
| `Cmd/Ctrl + Shift + H` | Hide toggle (batch) | ADR-0018 D3 `visibility` field. M.size ≥ 1. 모두 hidden → 모두 visible, 그 외 → 모두 hide. |

**P1 매트릭스 — wire deferred (ADR-0010 Group helper 미land)**:

| Shortcut | 액션 | 정본 / 비고 |
|---|---|---|
| `Cmd/Ctrl + G` | Group selection (M.size ≥ 1) | ADR-0010 D4. Group helper (groups[] 추가 + parent_id reparent) 미land — 본 amend 는 *매트릭스 등록* 만, wire 는 ADR-0010 helper land 시 별 batch. |
| `Cmd/Ctrl + Shift + G` | Ungroup (group 단일 선택) | ADR-0010 D12 (비파괴 — 자손 grandparent 로 승격). 동일 deferred. |

### Focus 분기

- 4 단축키 모두 modifier 보유 → registry 의 `allowInEditable=true` default 가
  *editable / xterm focus 시에도 fire 가능* 이지만, *ADR-0017 D6 amend ⑦ (a)*
  의 정책 정합 위해 명시 `allowInEditable: false` + `allowInXterm: false` — OS
  default 우선 (e.g., browser Cmd+L = URL bar focus 는 *canvas 사용 시*
  봉인되지만 editable focus 시 OS default 가 진입). canvas focus 시 사용자가
  의도한 Lock/Hide 만 fire.

### Batch 동작의 동일 분기

Lock / Hide 토글은 *batch 일관 상태* 정책:

- 모두 같은 상태 (all locked / all hidden) → 반대 상태로 토글
- 부분 일치 (mixed) → *모두 활성 상태* (Lock 의 경우 lock, Hide 의 경우 hide) — Figma 패턴

근거: mixed 상태에서 사용자 의도는 "다 적용해" — 부분 unlock/show 는 별 입력 (single item 의 Inspector toggle).

### ContextMenu wire

- ADR-0017 §D2 의 `[Hide / Show]` / `[Lock / Unlock]` placeholder → 본 amend 와 같은 batch 로 real wire. 같은 batch toggle 호출.

### 거절된 대안

- **R25.** Hide / Lock 의 *mixed → 모두 비활성* 분기 (Sketch 패턴) — 부분 활성 상태 보존 의도와 충돌. 거절.
- **R26.** Group / Ungroup 의 단축키 *wire 도 본 amend 에 포함* — ADR-0010 의 group helper 가 미land 라 scope 큼. 매트릭스 register 만, wire 는 별 batch.
- **R27.** `Cmd+H` 로 Hide (Cmd+Shift+H 대신) — macOS 의 *hide window* 와 충돌 (OS 가 먼저 양보 안 함). Cmd+Shift+H 채택.

### 결과

- Lock / Hide 의 batch 토글 단축키 — canvas mutation 효율 ↑
- ADR-0010 Group helper 의 미land 영역이 매트릭스 hub 에서 *deferred 표시* 로 가시화

### 산출물 / 정합 작업

- `lib/keyboard/editingShortcuts.svelte.ts` — Cmd+L / Cmd+Shift+H handler 추가
- `lib/chrome/ContextMenu.svelte` — `onHide` / `onLock` placeholder → real toggle wire (selection batch)
- ADR-0010 group helper land 시 본 amend cross-link 갱신

## Amend (2026-05-20 ⑩) — Tool-active node click forward to onpaneclick (R4)

### 맥락

UI/UX batch-5 (`docs/reports/2026-05-20-ui-ux-batch-5-analysis.md` §R4 / FE handover §B6) — point-spawn 도구 (text/note/file_path/terminal) active 인 동안 사용자가 *기존 panel 위* 를 click 했을 때, 기존 동작은 `onnodeclick` 의 early return (`if (!isSelectMode) return;`) 으로 *아무 일도 안 일어남*. 사용자 의도는 "도구가 active 인 동안은 위치 상관없이 새 item 생성" — 기존 동작과 충돌.

### 결정 — Canvas.svelte::onnodeclick forward

```
function onnodeclick({ node, event }) {
  if (isSelectMode) {
    // 기존 동작 — single / meta-toggle.
    ...
    return;
  }
  // tool active — onpaneclick 의 spawn 로직 forward (같은 좌표).
  if (event instanceof MouseEvent) onpaneclick({ event });
}
```

- **select 모드**: 기존 single / meta-toggle 동작 보존 (회귀 0).
- **point-spawn tool active (text/note/file_path/terminal)**: node hit 도 pane hit 처럼 처리 → 새 item 그 좌표 spawn.
- **drag-spawn tool (rect/ellipse/line/free_draw)**: 별 pointer handler (`pointerdown/move/up` capture) 가 처리 — `onnodeclick` 까지 도달 안 함, 별도 분기 불요.
- **hand tool**: `onpaneclick` 의 첫 줄 `if (isHandTool) return;` 가 자연 흡수 — node 위 click 도 no-op.

### 거절된 대안

- **R28.** Tool active 시에도 node click 으로 *기존 item 선택*. — 도구 의도와 충돌 (도구가 켜져 있는데 selection 이 바뀜).
- **R29.** Tool active 시 onnodeclick 을 SvelteFlow 의 `nodesFocusable=false` 로 차단. — global flag 변경은 회귀 risk + 도구 ↔ select 전환 시 flap.

### 결과

- 도구 활성 중 *어디든 click* 으로 새 item 생성 — Figma/Sketch 의 도구 사용 mental model 정합.
- Canvas.svelte 의 변경 범위 = onnodeclick 함수 한 곳 + forward 한 줄. 회귀 위험 작음.

### 산출물 / 정합 작업

- `lib/canvas/Canvas.svelte::onnodeclick` — forward 분기 land.
- AC-D3 (FE handover §D-3) — manual E2E: text/note tool active + 기존 panel 위 click → 새 item 생성 + 기존 panel 미선택.

## Amend (2026-05-21 ⑪) — Hand tool mode (component event 절대 격리)

### 맥락

`toolStore.current === 'hand'` 는 Figma 의 **H 키 = pan mode** 컨벤션 — 캔버스 viewport 만 조작 (왼-드래그 pan), canvas component 자체에는 *어떤 상호작용도 발생하지 않음*. 직전까지 구현은:
- Pan: ✅ `panOnDragMask = isHandTool ? [0, 1, 2] : [1, 2]` (Canvas.svelte:307)
- Selection / drag: ✅ `elementsSelectable / nodesDraggable / selectionOnDrag = isSelectMode` 가 자연 차단 (hand 모드 = !select)
- **Click**: ✅ `onpaneclick / onnodeclick` 의 `if (isHandTool) return;` 으로 차단
- **Right-click**: ❌ `onpanecontextmenu / onnodecontextmenu` 가 hand 모드 무시 → ContextMenu 열림. 사용자 보고 결함.

### 결정

Hand tool active 인 동안 canvas component / pane 의 *모든 mouse interaction* 차단:

| Event | hand 모드 동작 |
|---|---|
| Left click (`onpaneclick / onnodeclick`) | early return (현 동작 유지) |
| Right click (`onpanecontextmenu / onnodecontextmenu`) | **early return — ContextMenu 열지 않음 (본 amend 신규)** |
| Left-drag | pan (`panOnDragMask` 0 포함) |
| Selection box | 차단 (`selectionOnDrag={isSelectMode && !isSpacePressed && !isMaximizedActive}` 가 자연 차단) |
| Node drag | 차단 (`nodesDraggable={isSelectMode && !isMaximizedActive}`) |
| Resize handle | 차단 (NodeResizer 의 `isVisible={isInM && !isLocked}` — hand 모드는 M 갱신 안 되므로 자연 차단) |
| Hover / cursor | grab cursor (`pan-cursor` class) |
| Keyboard shortcut (Cmd+C/V/Z 등) | 그대로 동작 (도구와 직교) |

**원칙**: hand 모드는 *canvas 와 component 의 wall* — 사용자는 viewport 만 본다. 어떤 component-level event 도 fire 하지 않는다.

### 거절된 대안

- **R30.** Right-click 은 ContextMenu 열되 액션은 차단 (visual feedback) — 사용자가 "왜 안 되지" 혼란. 거절: 아예 안 여는 게 mental model 일관.
- **R31.** Hand 모드 자체를 폐기 (Space 만으로 충분) — Space-hold 는 momentary pan, Hand 는 sustained mode. 둘은 mutually distinct UX (Figma 컨벤션).

### 결과

- Pan 외 component interaction = 0. 사용자가 layout 만 navigate.
- Selection 보존 (hand 모드 진입 후 사라지지 않음) — select 모드 복귀 시 직전 selection 그대로.

### 산출물 / 정합 작업

- `lib/canvas/Canvas.svelte::onpanecontextmenu / onnodecontextmenu` — `if (isHandTool) return;` 추가.
- 후속: `lib/keyboard/chromeShortcuts.svelte.ts` 의 도구 shortcut (V/H 등) 회귀 확인 — 도구 전환은 항상 정상.

## 변경 이력

- 2026-05-15: 초안 + Accepted — plan 0005 Stage C 진입 시점. 6 영역 grid + 7 chrome 컴포넌트 책임 매트릭스 + Session lifecycle 7-step flow + Session 이름 surface.
- 2026-05-16 ①: Amend — Layers/Terminals 분리 (Sidebar = layer tree only / TerminalsPanel 신규) + header fold 모델 (PanelFoldButton 신규, RailToggle 은 collapsed 시에만 mount) + chromeStore.terminalsCollapsed 추가. *(②번 amend 로 vertical split + terminalsCollapsed 회수, header fold + RailToggle expand-only 는 유효 잔존)*
- 2026-05-16 ②: Amend — Layers / Terminals 통합 to `LeftPanel.svelte` + ref/frontend-design `panel-tabs` 패턴 (가로 탭) + collapsed 28px rail bar with per-tab icons (1-click expand + select). chromeStore.terminalsCollapsed 회수, leftPanelTab 추가. Sidebar.svelte → LayerTreeView.svelte rename, TerminalsPanel.svelte → TerminalListView.svelte rename.
- 2026-05-16 ③: Amend — RightPanel parity. `RightPanel.svelte` 신규 + `PaneInfoPanel.svelte` → `ItemInfoView.svelte` rename. 우측도 `panel-tabs` (현재 단일 `Inspect` 탭) + 28px collapsed rail with tab icon. `chromeStore.rightPanelTab` 추가. `RailToggle.svelte` 폐기 — 두 panel 모두 self-contained rail 보유.
- 2026-05-16 ④: Amend — Settings overlay (G19) + shortcutRegistry (G26) + themeStore system mode (G27 chrome). `SettingsOverlay.svelte`, `settingsDialog` store, `shortcutRegistry.svelte.ts`, `chromeShortcuts.svelte.ts` 신규. `zShortcuts` 를 registry consumer 로 마이그레이션. `Cmd+,` / `Cmd+Shift+L` / `Cmd+Shift+I` shortcut wire. `ThemeToggle` 의 §D6 단축키 회수 (Settings overlay Theme section 으로 흡수).
- 2026-05-16 ④ follow-up: G27 xtermTheme adapter ship — `lib/xterm/xtermTheme.ts` (ANSI 16 + bg/fg/cursor/selection, light/dark variants). XtermHost mount 시 `xtermTheme(themeStore.resolved)` 적용 + 별도 $effect 로 hot reload (theme flip 시 live `term.options.theme` 교체). xterm-host 컨테이너 background 도 `--canvas-bg` 로 변경 (light 모드 black flash 방지).
- 2026-05-16 ⑤: Amend — Dual-source adapter 제거. legacy 7 파일 (panels/groups/layout/ephemeral stores + http/layout v1 + legacyNewPane + MIndicator) 삭제. `useSessionStore = $derived(...)` 분기 13 곳 → 0 곳. sessionStore 에 `focusMode` 필드 추가 (ephemeralStore 통합). WS dispatcher 0x80~0x84 모두 sessionStore 직결 + `setLayoutRefetchHandler`/`setAutoMountHandler` 폐기. ADR-0006 / ADR-0015 후속 amend 필요 (FE 측 `/api/layout` v1 자취 + auto-mount 대체).
- 2026-05-16 ⑥: Amend — Layer list V2 (Multi-select + Drag reorder/reparent). LayerTreeView 의 selectNode 가 Cmd/Ctrl/Shift modifier 3-mode 정합 (Shift = visibleRangeIds anchor↔target range). HTML5 drag + dragover Y-ratio 분기 (before/inside/after). Multi-drag (dragged ∈ M 시 M 전체) + cycle 보호 + locked guard + Z mode 비활성. commitReparent 가 single mutateActiveLayout call 로 items.parent_id + groups.parent_id + groups.order atomic 갱신. Drop indicator CSS (2px accent line + dashed outline + dragging opacity). Item sibling order 는 BE schema v3 대기. ADR-0024 amend 와 짝.
- 2026-05-17 ⑦ (D6 amend ⑤): Basic editing shortcut matrix register. (a) `Cmd/Ctrl+A` (Select all — 신규 결정, focus 4 모드 분기: canvas / LayerTreeView / xterm / editable), (b) `Cmd/Ctrl+C` / `X` / `V` (ADR-0030 D5 cross-link), (c) `Cmd/Ctrl+Z` / `Shift+Z` (ADR-0028 cross-link). 비범위 OS-standard 5종 (Cmd+S auto-save 정합 / Cmd+P print / Cmd+W tab close browser / Cmd+R reload — D5.1/D5.4 자연 처리 / Cmd+Tab OS) 명시. Find (Cmd+F) 는 P2 deferred — 별 ADR 후보 (Cmd+K palette 와 분기). 거절: R18 별 ADR 신규 / R19 canvas Cmd+A 의 hidden 포함 / R20 editable Cmd+Z 의 app undo. `plan-0007 §14.20.5.2 / .4` + `handover-v3 §10.5.2 / .4` + 각 변경 이력 동시 amend.
- 2026-05-19 ⑧ (D6 amend ⑥): Arrow nudge + Cmd+D 매트릭스. (a) Plain `↑↓←→` = 1px / `Shift+↑↓←→` = 8px / `Cmd/Ctrl+↑↓←→` = 64px nudge — 모두 M.size ≥ 1, locked 제외, editable/xterm focus 시 OS default. (b) `Cmd/Ctrl+D` Duplicate — ADR-0030 D11 cross-link (clipboard 미오염 1-step in-place clone). Nudge 의 history grain = **250ms idle debounce = 1 entry** (Figma 패턴, ADR-0028 D11.1 의 optimistic failure rollback 정합). 거절: R21 매 keydown 1 entry / R22 Cmd+Arrow = viewport step dynamic / R23 Cmd+D clipboard 갱신 (Sketch 패턴) / R24 Alt+Arrow 확장 (P1).
- 2026-05-19 ⑨ (D6 amend ⑦): Lock/Hide toggle 매트릭스 + Group/Ungroup deferred. (a) `Cmd/Ctrl+L` Lock toggle (batch — all locked → unlock, 그 외 → lock; Figma 패턴), (b) `Cmd/Ctrl+Shift+H` Hide toggle (batch — all hidden → visible, 그 외 → hide). 둘 다 ADR-0018 D3 cross-link, M.size ≥ 1, editable/xterm focus 시 OS default. ContextMenu 의 `[Hide / Show]` + `[Lock / Unlock]` placeholder → real wire 같은 batch. (c) `Cmd/Ctrl+G` Group + `Cmd/Ctrl+Shift+G` Ungroup — *매트릭스 register only, wire deferred* (ADR-0010 group helper 미land). 거절: R25 mixed→비활성 (Sketch) / R26 Group wire 본 batch / R27 Cmd+H (OS hide 충돌).
- 2026-05-20 ⑩: UI/UX batch-5 R4 — Canvas.svelte::onnodeclick 의 tool-active forward. select 모드는 기존 single/meta-toggle 보존. point-spawn tool (text/note/file_path/terminal) active 시 node 위 click 도 onpaneclick 의 spawn 로직 forward — 사용자가 어디든 click 으로 새 item 생성 가능 (Figma/Sketch 정합). drag-spawn / hand 는 별 분기 불요 (자연 흡수). 거절: R28 tool-active 시 기존 item 선택 / R29 SvelteFlow nodesFocusable=false. cross-link: `2026-05-20-ui-ux-batch-5-analysis.md` §R4 / `2026-05-20-fe-handover-ui-ux-batch-5.md` §B6.
- 2026-05-21 ⑪: Hand tool mode 의 component event 절대 격리. 직전엔 left click 만 차단, right click (ContextMenu) 은 hand 모드에서도 열림. 본 amend 가 `onpanecontextmenu / onnodecontextmenu` 도 `if (isHandTool) return;` 으로 차단. 원칙: hand 모드 = canvas/component 간 wall, 사용자는 viewport pan 만. 거절: R30 ContextMenu 열되 액션만 차단 (mental model 혼란) / R31 hand 모드 폐기 (Space momentary vs sustained 의 distinct UX).
