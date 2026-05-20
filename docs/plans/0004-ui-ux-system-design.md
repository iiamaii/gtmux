# Plan 0004 — UI/UX 총체적 설계 + 구현 로드맵

- 일자: 2026-05-15
- 작성: agent (S7-PERSISTENCE-MINIMAL closeout + 0003 lifecycle UI plan 후속)
- 진입점: 사용자 요청 "UI/UX, 화면 레이아웃 총체적 설계 및 구현"
- 흡수: `docs/plans/0003-s7-lifecycle-ui-implementation.md` 의 4 phase (FE-AUTOMOUNT / FE-CLOSE-GUARD / FE-SHUTDOWN / backend KillSession) 가 본 plan 의 §6.x feature surface 안에 흡수
- 후속: 본 plan 은 *마스터 로드맵*. 각 phase 별 구현 시 별도 short plan 또는 inline ADR
- 범위 제외: 모바일/터치 UX (sketch §13 비범위), 멀티 사용자/공유, i18n 다국어 (현재 KO 1언어)

---

## 0. 한 줄 의도

현재 frontend 는 *기능 골격* 은 갖췄으나 (Canvas + Panel + Sidebar + WS dispatcher + 영속화 + ReconnectBanner), **chrome (Toolbar / CommandPalette / MIndicator) 는 빈 placeholder + design tokens 가 일부 hardcoded color 와 미정합**. 본 plan 은 (a) **design system foundation 정립** (b) **layout grid + chrome 채움** (c) **component library 표준화** (d) **잔여 feature surface 구현** 의 4 축을 단일 로드맵으로 정렬.

---

## 1. 현 상태 인벤토리

### 1.1 구현된 / 구현 부분 / placeholder

| 컴포넌트 | LOC | 상태 | 비고 |
|---|---|---|---|
| `routes/+page.svelte` | 163 | ✅ 구현 | Toolbar / Sidebar / Canvas / Banner 마운트 |
| `lib/toolbar/Toolbar.svelte` | 8 | ⏳ **placeholder** | 빈 `<header>` |
| `lib/toolbar/CommandPalette.svelte` | 8 | ⏳ placeholder | |
| `lib/toolbar/MIndicator.svelte` | 8 | ⏳ placeholder | M/I/Focus 상태 표시 |
| `lib/sidebar/Sidebar.svelte` | 370 | ✅ 구현 | Figma 식 layer panel (Group/Panel 트리 인라인) |
| `lib/sidebar/GroupTree.svelte` | 8 | 🟡 미사용 placeholder | Sidebar 안에 인라인 — 본 파일 dead |
| `lib/sidebar/PanelRow.svelte` | 8 | 🟡 미사용 placeholder | 동일 사유 |
| `lib/canvas/Canvas.svelte` | 176 | ✅ 구현 | SvelteFlow + NewPanelButton overlay |
| `lib/canvas/NewPanelButton.svelte` | 266 | ✅ 구현 | CTRL new-pane + PUT cascade |
| `lib/canvas/PanelNode.svelte` | 218 | ✅ 부분 구현 | header(label+badges) + body. **close 버튼 없음**, **hardcoded color** |
| `lib/canvas/PanelPlaceholder.svelte` | 66 | ✅ 구현 | zoom/suspended placeholder |
| `lib/canvas/XtermHost.svelte` | (미조사) | ✅ 구현 (전제) | xterm.js 마운트 |
| `lib/banner/ReconnectBanner.svelte` | 167 | ✅ 구현 | grace 1s + close code 분기 + role=status |
| `styles/tokens.css` | ~50 | ✅ 부분 | color/space/z-index/banner/zombie. **typography/radius/shadow/motion 없음** |
| `styles/global.css` | ~15 | ✅ 부분 | html/body 기본만 |

### 1.2 미구현 surface (UI 측)

- **Session shutdown UX** (S7-FE-SHUTDOWN, 0003 plan)
- **Panel close 버튼 + last-pane guard** (S7-FE-CLOSE-GUARD, 0003 plan)
- **Frontend pane-spawned auto-mount** (S7-FE-AUTOMOUNT, 0003 plan)
- **Command palette MVP** (현재 placeholder)
- **MIndicator wire** (현재 placeholder — M count / I target / Focus 상태)
- **Empty-state UI** (캔버스에 패널 0개일 때 안내 문구)
- **Loading-state UI** (WS connecting / 첫 hydrate 중)
- **Toast / non-blocking 알림** (PUT 412 rebase, slow-pane 경고 등)
- **Group label 편집 + color picker** (CONTEXT.md Group 운영 규칙 §"Group → M 확장")
- **Focus mode 시각 처리** (CONTEXT.md §"M·I·Viewport·Focus mode")
- **Keyboard shortcut 시스템** (Cmd-K palette, Cmd-Shift-Q shutdown 등)
- **Iconography** — 현재 텍스트 라벨 + `L/M/I/×` 문자. 아이콘 셋 미선정

### 1.3 Design token 갭

| 카테고리 | 현재 | 갭 |
|---|---|---|
| Color | 5색 (bg/fg/accent/warning/danger) + banner/zombie 변종 | **semantic tokens** (surface-1~3, border-subtle/strong, text-muted, success, info) 없음. PanelNode 가 토큰 미사용 hardcode |
| Space | 4/8/12/16 | OK, 추가로 24/32/48 가 큰 컨테이너에 필요할 수 있음 |
| Typography | font-family 만 (sans, monospace) | size scale (xs/sm/base/lg/xl), weight (regular/medium/semibold), line-height 없음 |
| Radius | 없음 (개별 `border-radius: 4px / 6px` hardcode) | `--radius-sm/md/lg` 토큰화 필요 |
| Shadow | 없음 | `--shadow-1/2/3` panel/modal/dropdown 용 |
| Motion | 없음 | `--motion-duration-fast/normal`, `--motion-easing-standard` (modal 진입, dropdown 펼침) |
| z-index | canvas/sidebar/toolbar/banner/modal | OK |

---

## 2. 디자인 원칙 (sketch.md / CONTEXT.md 인입)

1. **두 상태 도메인 분리의 시각화** (sketch §4) — backend mirror (pane 살아 있음/dead, slow) vs web-only (visibility/locked/minimized) 는 *시각적으로 다른 channel* 로 표시. 예: dead/slow = 색조 (warning), locked/minimized = 아이콘 + opacity.
2. **destructive 액션은 한 단계 멀리** — Shutdown / Panel close (last) / Group close 모두 confirm modal 또는 disabled.
3. **single-user지만 보안 가시화** (sketch §13) — Toolbar 의 session 명 + auth 상태가 항상 visible. token 노출 영역은 banner 의 transient 안내.
4. **MT-3 broadcast 인지** (CONTEXT.md §MT-3) — multi-tab 의 M/I/viewport 변경은 같은 사용자의 다른 탭에서도 즉시 보임. UI 가 *"this tab"* / *"all tabs"* 구분 노출 안 함 (Server = 단일 진실).
5. **Empty / loading / error 의 명시적 디자인** — 빈 캔버스, 첫 hydrate, WS 실패 등 모든 경계 상태에 *명시적 일러스트레이션 + CTA*.
6. **키보드 우선** — palette (Cmd-K), shortcut (Cmd-Shift-Q shutdown) 가 모든 메뉴 액션의 alias.
7. **Figma / VSCode 친화적 모델** — Sidebar = layer panel, Panel = window-like chrome, Toolbar = menu bar. 사용자 멘탈 모델 차용으로 학습 비용↓.

---

## 3. 레이아웃 그리드 (target)

```
+============================================================+
| ReconnectBanner   (z=1000, conditional, role=status)        |   ← banner layer
+============================================================+
| Toolbar (height 40px, z=200)                                |
|  ┌─ branding ─┐  ┌─ session name ─┐ ┌─ MIndicator ─┐ [ ⋮ ] |
+----+--------------------------------------------------+----+
|    |                                                  |    |
|    |             Canvas (z=0)                         |    |
|    |    ┌─ canvas-toolbar overlay top-left ─┐         |    |
|    |    │  [ New Panel ]                    │         |    |
|    |    └───────────────────────────────────┘         |    |
| Sb |                                                  | Sb |
| id |        ┌────────────────┐                        | id |
| eb |        │ %5  L M I  [×] │                        | eb |
| ar |        │                │ ← PanelNode             | ar |
| L  |        │ $ ls -la       │                        | R  |
| 26 |        │ ...            │                        | (- |
| 0p |        └────────────────┘                        | )  |
| x  |                                                  |    |
|    |  ┌──── empty-state if no panels ────┐            |    |
|    |  │ "No panels yet — click New Panel" │            |    |
|    |  └────────────────────────────────────┘           |    |
|    |                                                  |    |
+----+--------------------------------------------------+----+
| (status bar 검토 — P1) port / mode / connection / latency |   ← optional bottom
+============================================================+
```

- Toolbar: **40px** 고정 (compact, VSCode 패턴)
- Sidebar: **260px** 기본 (사용자 resize 는 P1+, MVP 고정)
- Canvas: fill remaining
- Banner: Toolbar 위 절대 위치 (conditional, banner 표시 시 Toolbar 가 banner 아래로 밀림)
- 하단 status bar 는 P1+ 보류 — 현재는 Toolbar 안에 통합

---

## 4. 디자인 시스템 토큰 (확장 제안)

`styles/tokens.css` 를 다음 구조로 확장:

```css
:root {
  /* ── Color: semantic ───────────────────────────────── */
  --color-bg: #0f172a;            /* app background */
  --color-surface-1: #111827;     /* sidebar / toolbar surface */
  --color-surface-2: #1e293b;     /* panel header / dropdown */
  --color-surface-3: #334155;     /* panel body / divider strong */
  --color-fg: #e2e8f0;            /* primary text */
  --color-fg-muted: #94a3b8;      /* secondary text */
  --color-fg-subtle: #64748b;     /* placeholder / disabled */
  --color-border-subtle: #1f2937;
  --color-border-strong: #334155;

  --color-accent: #38bdf8;        /* I (input target), primary button */
  --color-accent-fg: #052e16;     /* text on accent */
  --color-success: #22c55e;       /* connected, pane live */
  --color-warning: #facc15;       /* slow-pane, transient */
  --color-danger: #ef4444;        /* destructive (shutdown, close last) */
  --color-info: #60a5fa;          /* M selection, focus mode */

  /* ── Spacing ────────────────────────────────────────── */
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 24px;
  --space-6: 32px;
  --space-7: 48px;

  /* ── Typography ─────────────────────────────────────── */
  --font-sans: ui-sans-serif, system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
  --font-mono: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  --text-xs: 10px;
  --text-sm: 11px;
  --text-base: 12px;
  --text-md: 13px;
  --text-lg: 14px;
  --text-xl: 16px;
  --leading-tight: 1.2;
  --leading-normal: 1.4;
  --weight-regular: 400;
  --weight-medium: 500;
  --weight-semibold: 600;

  /* ── Radius ─────────────────────────────────────────── */
  --radius-sm: 3px;     /* badge */
  --radius-md: 4px;     /* button, input */
  --radius-lg: 6px;     /* panel, dropdown */
  --radius-xl: 8px;     /* modal */

  /* ── Shadow ─────────────────────────────────────────── */
  --shadow-1: 0 1px 2px rgba(0, 0, 0, 0.25);          /* button hover */
  --shadow-2: 0 4px 12px rgba(0, 0, 0, 0.35);         /* panel (현 PanelNode 정합) */
  --shadow-3: 0 8px 24px rgba(0, 0, 0, 0.45);         /* dropdown, modal */
  --shadow-4: 0 16px 48px rgba(0, 0, 0, 0.55);        /* modal overlay */

  /* ── Motion ─────────────────────────────────────────── */
  --motion-fast: 80ms;     /* button hover, focus ring */
  --motion-normal: 160ms;  /* dropdown, banner */
  --motion-slow: 240ms;    /* modal enter */
  --motion-easing: cubic-bezier(0.2, 0.8, 0.2, 1);    /* standard */

  /* ── Layout ─────────────────────────────────────────── */
  --layout-toolbar-h: 40px;
  --layout-sidebar-w: 260px;
  --layout-banner-h: 32px;

  /* (기존) z-index, banner-*, zombie-* 유지 */
}
```

**규칙**: 모든 컴포넌트 색/spacing 은 토큰만 사용. PanelNode 의 hardcoded `#0f172a #1e293b #334155 ...` 등은 token 으로 교체 (refactor task).

---

## 5. 컴포넌트 라이브러리 (신규 표준)

`src/lib/ui/` 디렉터리 신규 — primitive 컴포넌트 모음.

| 컴포넌트 | 역할 | 사용처 |
|---|---|---|
| `Button.svelte` | primary / secondary / danger / ghost variant, sm/md size | Toolbar, Modal, NewPanelButton |
| `IconButton.svelte` | 정사각 + icon only + aria-label | Panel close, Toolbar kebab |
| `Dropdown.svelte` | anchored menu (Toolbar kebab, future Sidebar context) | SessionMenu |
| `Modal.svelte` | overlay + focus trap + Esc close + role=dialog | ShutdownModal, Group rename |
| `Toast.svelte` | non-blocking 알림 (PUT 412 rebase 자동 시 등) | layout helper |
| `Banner.svelte` | sticky 알림 (Reconnect, AuthExpired) | ReconnectBanner refactor |
| `Input.svelte` | text input + label + error | future Group label edit |
| `Tooltip.svelte` | hover 안내 (close 버튼 disabled tooltip) | Panel close, MIndicator |
| `Icon.svelte` | SVG inline 아이콘 wrapper | 전체 |

**Iconography 전략**:
- 옵션 A (권장): **lucide-svelte** — 1300+ tree-shakable SVG, MIT, 9KB gzip 추가 분리. 후속 phase 에서 명시 ADR.
- 옵션 B: 인라인 SVG 직접 작성 (의존성 0, 유지보수↑)
- 옵션 C: Heroicons / Phosphor — 비교만.

본 plan 은 옵션 A 를 default 로 가정, ADR-0016 에서 정식.

---

## 6. Feature surface 설계 (구현 단위)

### 6.1 Toolbar 재설계

레이아웃: `[branding] [session info] [spacer] [M-Indicator] [palette trigger] [kebab menu]`

- **branding**: `gtmux` text + small logo (lucide `terminal` 아이콘?). 좌측.
- **session info**: `demo · 127.0.0.1:9999 · Local` 형식. `--text-sm` `--color-fg-muted`. 클릭 → 상세 popover (rotate-token, port, supervisor pid).
- **M-Indicator**: `M:3 / I:%5 / Focus` 형태. 0 일 때 `--color-fg-subtle`.
- **palette trigger**: `Cmd-K` 핫키 hint 가 visible 한 button. P1+ 에서 진짜 palette modal.
- **kebab menu (⋮)**: SessionMenu — items: "Session shutdown", "Rotate token", "Copy bootstrap URL", "About".

Session info 의 session 이름 출처: bootstrap landing 에서 `sessionStorage.gtmux_session` 주입 (0003 plan §2 Phase D 와 정합).

### 6.2 Sidebar 채움

현재 Sidebar 는 layer tree 만 구현. 추가:

- **header**: Sidebar 상단에 `Workspace` 라벨 + (P1+) search input. 현재는 라벨만.
- **footer**: 하단에 connection 상태 mini-dot (`● open` / `● reconnecting` / `● closed`) + WS attempt 번호 표시 (banner 정보의 mini mirror).
- **tree 행**: 기존 구현 유지하되 토큰 미정합 부분 정리 (color, space).

### 6.3 Canvas chrome

- **canvas-toolbar overlay**: 현재 NewPanelButton만. 추가: 우상단에 `viewport mini-info` (`zoom: 100%` 등) — 사용자가 zoom 상태를 인지 (R8 §F8 placeholder 진입 점 가시화).
- **empty state**: panelsStore.panels.size === 0 일 때 캔버스 중앙에 illustrative 안내. `+ New Panel` 큰 버튼 + "Press the button above or use the command palette" 부텍스트.
- **loading state**: connectionStore.state === 'connecting' 또는 layoutStore 첫 hydrate 직전 → 캔버스 위에 subtle spinner + "Connecting…" 텍스트.

### 6.4 PanelNode chrome 보강

- **헤더**: 현재 `label + badges (L/M/I)`. 변경:
  - 좌측: drag handle icon (lucide `grip-vertical`) + label
  - 중앙: badges (L/M/Min/I) — 토큰화
  - 우측: `IconButton(×, ariaLabel='Close panel')` — disabled when `muxStore.liveCount === 1` + Tooltip
- **dead/slow 시각**: `dead === true` → header opacity 0.6 + label strikethrough + dead badge. `slow` → header 우측에 warning chip.

### 6.5 ShutdownModal (0003 Phase D)

`Modal.svelte` 위에 build:

- 제목: `Shutdown session 'demo'?`
- 본문 (3-bullet):
  - `· 3 active panes will be reaped`
  - `· Layout will be preserved`
  - `· Server process will exit (6)`
- 액션: `[Cancel]` (ghost) + `[Shutdown]` (danger)
- Esc / overlay 클릭 → cancel. 포커스 트랩.

### 6.6 SessionMenu (Toolbar kebab dropdown)

`Dropdown.svelte` 위에:
- `Session shutdown` (danger 색조) → ShutdownModal
- `Rotate token` (warning 색조) → Toast 안내 (실제 rotate-token 은 CLI 측 — P1+ 에서 endpoint 추가 시 wire)
- `Copy bootstrap URL` → clipboard write (Toast 확인)
- `About` → small modal with version, ADR refs

### 6.7 CommandPalette MVP

- `Cmd-K` 트리거. 입력창 + 명령 리스트.
- 명령 V1: `new-pane`, `shutdown`, `focus mode toggle`, `goto pane %N`.
- 본 MVP 는 0003 Phase D 외 별도 phase (P1+) — 본 plan 의 §7 에서 분리 phase.

### 6.8 Empty / loading / error 상태

| 상태 | 트리거 | 표현 |
|---|---|---|
| 첫 WS connecting | `connectionStore.state === 'connecting'` 첫 1초 | Canvas 중앙 spinner + "Connecting to gtmux Server…" |
| 빈 layout | `panelsStore.panels.size === 0` | Canvas 중앙 illustration + CTA "New Panel" |
| 사이드바 빈 트리 | `panelsStore + groupsStore = empty` | Sidebar 본문에 `"No panels yet"` muted text |
| 인증 실패 (close 4001) | banner 가 이미 처리 | banner 안에 "Re-authenticate" 링크 |
| Layout PUT 412 race | http/layout 의 auto-rebase | Toast `"Layout out of sync — refreshed"` (silent UX 권장, P1+에서 결정) |

### 6.9 Keyboard shortcut 표

| 단축키 | 동작 | 비고 |
|---|---|---|
| `Cmd-K` / `Ctrl-K` | Command palette | MVP |
| `Cmd-Shift-Q` | Shutdown modal | 0003 Phase D |
| `Cmd-N` | New panel | Canvas 활성 시 |
| `Esc` | modal close / palette close / focus mode exit | global |
| `Cmd-/` | toggle Sidebar | P1+ |
| `Cmd-F` | toggle Focus mode | P1+ |
| `Cmd-1..9` | panel 번호로 jump (M 설정) | P1+ |

---

## 7. 구현 phase 매핑 (master roadmap)

본 plan 은 *마스터*. 각 phase 는 PR 1건 분량.

### Phase 0 — Design system foundation (선행 필수)

**Files**:
- `styles/tokens.css` — §4 토큰 풀세트 확장
- `styles/global.css` — typography 기본, scrollbar styling
- 모든 컴포넌트 — hardcoded color/space 를 token 으로 교체 (PanelNode, Canvas, NewPanelButton 우선)

**LOC**: ~100 (tokens 확장) + ~150 (refactor)

**ADR**: ADR-0016 — "Design tokens 정본" (lucide-svelte 도입 포함)

### Phase 1 — UI primitive 라이브러리 (선행)

**Files**: `src/lib/ui/` 신규 9 컴포넌트
- Button, IconButton, Dropdown, Modal, Toast, Banner, Input, Tooltip, Icon

**LOC**: ~800

**ADR**: 별도 ADR 없이 본 plan §5 가 reference

### Phase 2 — Toolbar 재설계 + SessionMenu + Session info

**Files**: `Toolbar.svelte` rewrite, `SessionMenu.svelte` 신규, `MIndicator.svelte` wire-up
- bootstrap landing 의 sessionStorage 에 `gtmux_session` 추가 (http-api 측 +5 LOC)

**LOC**: ~250 (FE) + 10 (BE)

### Phase 3 — Empty / Loading state + Canvas chrome 보강

**Files**: `Canvas.svelte` (empty-state insert), 신규 `EmptyState.svelte`, `LoadingOverlay.svelte`

**LOC**: ~150

### Phase 4 — PanelNode chrome 보강 + Close button (0003 Phase C 흡수)

**Files**: `PanelNode.svelte` (header redesign, close button, tooltip), `mux.svelte.ts` (liveCount)

**LOC**: ~150

### Phase 5 — Backend KillSession + FE-AUTOMOUNT + ShutdownModal (0003 Phase A+B+D 흡수)

**Files**:
- backend (Phase A from 0003): pty-backend / cmd_router / ws-server + ADR-0013 D10 amend
- frontend (Phase B from 0003): dispatcher hook + http/layout helper + ADR-0015 신규
- frontend (Phase D from 0003): SessionMenu의 Shutdown item wire + ShutdownModal + ReconnectBanner "Session ended" 분기

**LOC**: backend ~80 + frontend ~400 + docs ~250

### Phase 6 — Sidebar header/footer + 토큰 정합 refactor

**Files**: `Sidebar.svelte` (header + footer 추가, 토큰 정합)

**LOC**: ~100

### Phase 7 — CommandPalette MVP

**Files**: `CommandPalette.svelte` (rewrite), keyboard global handler 신규

**LOC**: ~300

### Phase 8 — Keyboard shortcut 표 정식 wire

**Files**: `lib/shortcuts/` 신규 — global handler + 등록 헬퍼

**LOC**: ~150

### Phase 9 — Toast 시스템 + 412 rebase 알림

**Files**: `lib/ui/Toast.svelte` 마운트 host + 호출 API

**LOC**: ~80

### Phase 10 — Visual polish + accessibility audit

- ARIA 라벨 보강 (Sidebar 트리 role, Toolbar role)
- focus ring 일관성 (`--color-info` outline 2px)
- reduced-motion media query
- color contrast (WCAG AA ≥ 4.5:1) 검증

**LOC**: ~150

### Phase total

- backend: ~80 LOC + ADR amend 1
- frontend: ~2,500 LOC
- docs: ~600 LOC (ADR-0015, ADR-0016, plan updates)

---

## 8. 우선순위 옵션 (사용자 선택)

본 plan 의 10 phase 는 직렬 의존이 강하지 않음. 다음 3 옵션 중 선택:

### 옵션 A — Foundation-first (권장)
**순서**: Phase 0 → 1 → 2 → 3 → 4 → 5 → ...
- 디자인 시스템 + primitive 부터. 이후 feature 가 token / primitive 위에 쌓이므로 retrofit 비용 0.
- Phase 5 (S7-FE-SHUTDOWN 등 0003 의 핵심) 까지 도달 시간↑ (Phase 0-4 가 prerequisite).

### 옵션 B — Feature-first
**순서**: Phase 5 → 4 → 0 → 1 → 2 → ...
- 0003 plan 의 S7 잔여 feature 부터 진입. 디자인 시스템 미정합 상태로 코드 작성 → 후속 phase 0/1 에서 refactor.
- 시연 가능 기능 빨리. 단 retrofit 비용 발생.

### 옵션 C — 병행 (foundation + feature 인터리빙)
**순서**: Phase 0 → 5 → 1 → 4 → 2 → 3 → ...
- foundation 의 핵심 (tokens) 만 먼저, 그다음 feature, 다음 primitive 라이브러리, 등 인터리빙.
- 가장 유연하나 phase 경계가 모호 — agent 가 한 PR 안에서 여러 phase 의 일부씩 처리.

---

## 9. ADR 매트릭스 (본 plan 으로 발행되는 결정)

| ADR | 제목 | 본 plan 의 phase | 상태 |
|---|---|---|---|
| **0013 D10 amend** | BackendCommand 에 KillSession variant | Phase 5 | 0003 plan 에서 정의 |
| **0015 신규** | Pane auto-mount 책임 = frontend (`pane-spawned` cascade PUT) | Phase 5 | 0003 plan 에서 정의 |
| **0016 신규** | Design tokens 정본 + iconography (lucide-svelte) | Phase 0 + 1 | 본 plan 에서 정의 — phase 0 진입 시 발행 |
| **0017 신규 (옵션)** | Keyboard shortcut 등록 시스템 + global handler 경계 | Phase 8 | phase 8 진입 시 발행 |
| **0018 신규 (옵션)** | Modal / Toast / Banner overlay 책임 분리 | Phase 1 | 잠재 — primitive 설계 시 결정 |

---

## 10. Open questions

- **O1**: lucide-svelte (옵션 A) 대신 인라인 SVG (옵션 B) 채택 가능성 — bundle size +9KB vs 의존성 0. 의도적 결정 필요 — ADR-0016 에서 잠금.
- **O2**: 단축키 충돌 정책 — Cmd-K 는 브라우저 기본 (구글 검색 등) 과 충돌 가능. preventDefault 정책 필요.
- **O3**: Sidebar resize (drag) — P1+ 보류했으나 사용자 요구 빈도에 따라 phase 6 흡수 가능.
- **O4**: dark/light 테마 분기 — 현재 dark 단일. light 도입은 별도 phase (본 plan 외).
- **O5**: i18n — KO/EN 토글. 본 plan 범위 밖. CONTEXT.md 의 "UI 문자열은 i18n 별도 결정" 정신 유지.

---

## 11. 다음 행동 (사용자 선택)

| 사용자 메시지 | 행동 |
|---|---|
| "옵션 A 진행" / "Foundation 먼저" | Phase 0 ADR-0016 draft → tokens 확장 → primitive 라이브러리 |
| "옵션 B 진행" / "Feature 먼저 (S7 lifecycle UI)" | 0003 plan §2 Phase A 진입 — backend KillSession ADR amend 부터 |
| "옵션 C 진행" / "병행" | Phase 0 (tokens) + Phase 5 (lifecycle UI) 를 한 사이클로 묶음 |
| "Phase 0 + 1 (디자인 시스템) 단독 PR" | 본 plan 의 §4 + §5 만 분리 PR — UI/UX 시각 변화 없이 refactor + 인프라 |
| "옵션 비교만 더 자세히" | 본 plan §8 의 phase 별 trade-off 표 작성 |
| "이 plan 자체에 수정 필요" | 본 plan 의 §3 레이아웃 그리드 / §4 토큰 / §5 컴포넌트 별 조정 |

---

## 변경 이력

- 2026-05-15: 초안 — S7-PERSISTENCE-MINIMAL closeout + 0003 lifecycle UI plan 후속, "UI/UX 총체적 설계 및 구현" 요청 흡수. 10 phase 마스터 로드맵 + design system foundation + 우선순위 3 옵션.
