# ADR-0016: Design tokens 정본 + iconography (lucide-svelte)

- 상태: Accepted (2026-05-15, **amend ×1** 2026-05-15)
- 일자: 2026-05-15 (Proposed + Accepted, plan 0004 Phase 0 dispatch 정합) / 2026-05-15 (amend — `ref/frontend-design` Figma adaptation 흡수)
- 결정자: agent (frontend-architect role, plan 0004 마스터 로드맵 진입)
- 근거 plan: `docs/plans/0004-ui-ux-system-design.md` §4 (토큰) + §5 (iconography)
- 관련 ADR: ADR-0012 (Frontend stack — Svelte 5 + Vite), ADR-0005 (Canvas library — @xyflow/svelte), ADR-0004 (Terminal — xterm.js)
- 관련 SSoT: (없음 — 본 ADR이 사실상 design-tokens SSoT 역할)

## 맥락

`docs/plans/0004-ui-ux-system-design.md` 가 UI/UX 마스터 로드맵 10 phase 를 정의하면서, Phase 0 (디자인 시스템 foundation) 의 *진입 prerequisite* 로 본 ADR 을 요구한다. 현 시점 `codebase/frontend/src/styles/tokens.css` 는 **부분 토큰만 보유** — color (5색) + space (4 단계) + z-index (5 layer) + banner/zombie 변종 — 이며 **typography / radius / shadow / motion / semantic color (surface, fg-muted, success, info) 미정의**. PanelNode 와 NewPanelButton 등 핵심 컴포넌트가 hardcoded color 를 사용해 *후속 phase 의 token 정합 refactor* 가 필수 (plan 0004 §1.3).

추가로, 본 plan 의 §5 (component library) 는 `Button`, `IconButton`, `Modal` 등 9 primitive 의 chrome 에 *일관된 아이콘* 을 요구한다. 현재는 텍스트 라벨 (`L M I ×` 등) 위주 — 사용성 측정 시 클릭 타깃 크기 부족 + 시각적 위계 부재. **iconography 셋 결정 + 도입 도구 잠금** 이 본 ADR 의 두 번째 책임.

본 ADR 은 다음 4 차원을 잠근다:
1. **Token 카테고리 정본** — color (semantic), spacing, typography, radius, shadow, motion, layout
2. **Token 명명 규칙** — `--<category>-<role>` (예: `--color-surface-1`, `--space-4`, `--shadow-2`)
3. **Iconography 라이브러리** — lucide-svelte (1300+ SVG, MIT, tree-shakable)
4. **변경 정책** — 새 토큰 추가는 본 ADR amend, 값 변경은 PR + 시각 회귀 캡처

## 결정 (Decisions)

### D1. Token 카테고리 정본 (7 카테고리)

다음 7 카테고리만 `tokens.css` 에 정의한다. 다른 ad-hoc 카테고리 추가는 본 ADR amend 동반.

| 카테고리 | 토큰 prefix | 변종 |
|---|---|---|
| Color (semantic) | `--color-*` | `bg`, `surface-1`, `surface-2`, `surface-3`, `fg`, `fg-muted`, `fg-subtle`, `border-subtle`, `border-strong`, `accent`, `accent-fg`, `success`, `warning`, `danger`, `info` |
| Spacing | `--space-*` | `1` (4px) ~ `7` (48px), 7 단계 |
| Typography | `--font-*`, `--text-*`, `--leading-*`, `--weight-*` | `font-sans`, `font-mono`, `text-xs/sm/base/md/lg/xl`, `leading-tight/normal`, `weight-regular/medium/semibold` |
| Radius | `--radius-*` | `sm` (3px), `md` (4px), `lg` (6px), `xl` (8px) |
| Shadow | `--shadow-*` | `1` (button hover), `2` (panel), `3` (dropdown), `4` (modal) |
| Motion | `--motion-*` | `fast` (80ms), `normal` (160ms), `slow` (240ms), `easing` (cubic-bezier) |
| Layout | `--layout-*` | `toolbar-h`, `sidebar-w`, `banner-h` — 그리드 고정값 |

기존 카테고리 (`--z-*` z-index, `--banner-*` warn/error, `--zombie-*`) 는 D1 의 7 카테고리 *외 보조 정본* 으로 유지. 이들은 *컴포넌트 단위* 토큰이라 D1 의 일반 카테고리에 흡수되지 않는다.

### D2. Token 명명 규칙

- 형식: `--<category>-<role>[-<variant>]` (예: `--color-surface-1`, `--color-fg-muted`, `--shadow-3`).
- **semantic 우선, primitive 회피**: `--color-blue-500` 같은 색 자체 명 사용 금지. 항상 *역할 기반* (`--color-info`, `--color-accent`).
- 영문 소문자 + 하이픈만. 숫자 suffix 는 *위계 깊이* 표현 (surface-1 < surface-2 < surface-3).
- token 값은 `:root` 안에서 한 번만 정의. dark/light 분기는 본 ADR 범위 밖 (P2+).

### D3. 명시 값 (tokens.css 정본)

```css
:root {
  /* ── Color: semantic ───────────────────────────────── */
  --color-bg: #0f172a;
  --color-surface-1: #111827;
  --color-surface-2: #1e293b;
  --color-surface-3: #334155;
  --color-fg: #e2e8f0;
  --color-fg-muted: #94a3b8;
  --color-fg-subtle: #64748b;
  --color-border-subtle: #1f2937;
  --color-border-strong: #334155;

  --color-accent: #38bdf8;
  --color-accent-fg: #052e16;
  --color-success: #22c55e;
  --color-warning: #facc15;
  --color-danger: #ef4444;
  --color-info: #60a5fa;

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
  --radius-sm: 3px;
  --radius-md: 4px;
  --radius-lg: 6px;
  --radius-xl: 8px;

  /* ── Shadow ─────────────────────────────────────────── */
  --shadow-1: 0 1px 2px rgba(0, 0, 0, 0.25);
  --shadow-2: 0 4px 12px rgba(0, 0, 0, 0.35);
  --shadow-3: 0 8px 24px rgba(0, 0, 0, 0.45);
  --shadow-4: 0 16px 48px rgba(0, 0, 0, 0.55);

  /* ── Motion ─────────────────────────────────────────── */
  --motion-fast: 80ms;
  --motion-normal: 160ms;
  --motion-slow: 240ms;
  --motion-easing: cubic-bezier(0.2, 0.8, 0.2, 1);

  /* ── Layout ─────────────────────────────────────────── */
  --layout-toolbar-h: 40px;
  --layout-sidebar-w: 260px;
  --layout-banner-h: 32px;

  /* ── z-index, banner-*, zombie-* (기존 보조 정본) ───── */
  --z-canvas: 0;
  --z-sidebar: 100;
  --z-toolbar: 200;
  --z-banner: 1000;
  --z-modal: 2000;
  --banner-warn-bg: #422006;
  --banner-warn-fg: #fde68a;
  --banner-warn-border: #b45309;
  --banner-error-bg: #450a0a;
  --banner-error-fg: #fecaca;
  --banner-error-border: #b91c1c;
  --zombie-bg: #1f2937;
  --zombie-fg: var(--color-warning);
}
```

### D4. 컴포넌트 정합 정책

- *모든 신규 컴포넌트* 는 토큰만 사용. hardcoded 색/공간/radius/shadow/motion 발견 시 PR review 에서 reject.
- 기존 컴포넌트 (PanelNode / NewPanelButton / Sidebar / ReconnectBanner / PanelPlaceholder) 는 plan 0004 Phase 0 안에서 token refactor (시각 변화 0, 값 보존).
- xterm.js 의 themed color 는 *별도* — xterm 의 `ITheme` API 가 자체 카테고리. 본 ADR 토큰을 xterm theme 에 매핑하는 어댑터는 phase 0 외 별도 task.

### D5. Iconography 라이브러리 = lucide-svelte

- 패키지: `lucide-svelte` (https://lucide.dev/) — MIT, 1300+ tree-shakable SVG icons.
- 도입 위치: `package.json` dependencies. `npm install lucide-svelte`.
- 사용 패턴: `src/lib/ui/Icon.svelte` 가 wrapper — 외부 컴포넌트는 `<Icon name="terminal" size="16" />` 형태로 사용. lucide 의 raw component 를 직접 import 도 허용하되 *공통 size / aria-label / decorative 처리* 가 필요한 경우 wrapper 경유 권장.
- bundle impact: lucide-svelte 는 tree-shaking 기반 — 사용된 아이콘만 번들에 포함. 단일 아이콘 ≈ 200-400 byte gzip. 30 icon 전사용 시 +12 KB gzip 예상.

### D6. Iconography 사용 정책

- **decorative 아이콘**: `aria-hidden="true"` 자동 설정 (Icon wrapper 의 default).
- **interactive 아이콘** (IconButton 안): `aria-label` 필수 — wrapper 가 누락 시 dev warning.
- **size 표준**: 12 (badge), 14 (inline), 16 (button), 20 (toolbar), 24 (modal title). 자유값 허용하되 표준값 권장.
- **stroke-width 표준**: lucide 의 default (2). lucide 의 `strokeWidth` prop 미사용 — 다른 굵기는 다른 카테고리.

### D7. 변경 정책

- 새 토큰 추가 (예: `--motion-very-slow`, `--color-accent-soft`) = 본 ADR amend + tokens.css PR + reviewers 1 + 사용처 별도 PR.
- 토큰 값 변경 (예: `--color-bg` 색 조정) = visual regression 캡처 + reviewers 1.
- 토큰 *제거* = 모든 사용처 grep + migration plan + 최소 1 commit cycle 의 deprecation 단계.
- lucide-svelte 메이저 버전 업 = 본 ADR §D5 amend (호환성 확인 + visual regression).

## 거절된 대안 (Rejected)

- **R1. Tailwind CSS** — atomic class 모델, 1000+ utility class. 본 프로젝트는 *명시 토큰 + Svelte scoped style* 모델 이 더 적합 (R8 §F2: 컴포넌트 캡슐화 정합). Tailwind 도입 시 (a) PostCSS 빌드 step 추가 (b) class 명명 규칙이 토큰과 분리되어 *2-source-of-truth* (c) Svelte 의 scoped style 우위 무효화. 거절.
- **R2. CSS-in-JS (Emotion / Stitches / vanilla-extract)** — Svelte 5 의 scoped style + CSS custom properties 가 *zero-runtime* 이며 동등 표현력. JS runtime overhead 추가 거부. 거절.
- **R3. Material 3 / Bootstrap 같은 완성형 디자인 시스템** — 본 프로젝트의 *터미널 워크스페이스* 미감 (dark, dense, monospace 친화) 과 미스매치. 채택 시 override 비용이 신규 작성보다 큼. 거절.
- **R4. Iconography: heroicons** — lucide 와 유사하나 (a) 아이콘 수가 절반 미만 (300+ vs 1300+) (b) 일부 필요 아이콘 (terminal, kebab, grip 등) 정합도↓ (c) 일관성↓ (filled / outlined 가 별도 set). 거절.
- **R5. Iconography: Phosphor / Tabler / Feather** — lucide 의 사전 fork (feather → lucide), 유지보수성↑ + 활성 커뮤니티 — lucide 채택. 거절.
- **R6. Iconography: inline SVG 수동 작성** — 의존성 0, bundle↓. 단 (a) 30+ 아이콘 시 유지보수 비용 (b) 일관성 (stroke / corner radius) 보장 어려움 (c) 공통 사용처 추상화 비용. 거절. (10 이내 아이콘이라면 채택 가능성↑ — 현 plan 0004 의 §6 매트릭스로는 20-30개 예상.)
- **R7. Iconography: 자체 디자인 SVG (브랜드 일치)** — gtmux 의 브랜드 차별화 우선이 아니므로 (single-user dev tool) 표준 아이콘셋 사용이 효율. 거절.
- **R8. 토큰을 JS 모듈로 정의 (tokens.ts)** — CSS custom property 가 *런타임 변경 가능* (dark/light theme prep) + *DevTools 가시* + *Svelte scoped 와 자연 정합*. JS 모듈은 컴파일 시점 잠금 + 런타임 변경 불가. 거절.

## 결과 (Consequences)

### 긍정

- **컴포넌트 일관성↑** — 모든 UI surface 가 동일 토큰 어휘 사용. 미래 phase 의 디자인 변경 (예: dark → light 추가, 색조 조정) 이 단일 파일 수정으로 전파.
- **bundle 비용↓** — Tailwind 회피 + tree-shakable iconography. lucide-svelte 의 +12 KB gzip 예상 비용 < Tailwind 의 baseline 압축 후 ~30 KB.
- **A11y 기본 흡수** — Icon wrapper 의 `aria-hidden` / `aria-label` 정책이 사용처에 강제. `--motion-*` 토큰이 prefers-reduced-motion 미디어 쿼리 정합.
- **ADR-before-code 정합** — 본 ADR 이 phase 0 의 prerequisite. 토큰 도입 전 발행, 코드는 본 ADR 이후.

### 부정 / 비용

- **신규 의존성 1건** (`lucide-svelte`) — supply chain 표면↑. MIT + 활성 커뮤니티 + tree-shakable 로 위험 완화.
- **기존 컴포넌트 refactor 부담** — PanelNode, NewPanelButton, Sidebar 의 hardcoded 값 교체 ~150 LOC 변경. 시각 변화 0 이지만 PR review 부담.
- **MVP scope 확장** — design tokens 풀세트가 *충분한 풍부함* 인지 검증 못함. 새 토큰 필요 시 본 ADR amend.

### 후속 작업

- **plan 0004 Phase 0 진행**: `tokens.css` 풀세트 + `global.css` 보강 (focus-visible, scrollbar, reduced-motion).
- **plan 0004 Phase 1 진입**: `src/lib/ui/` 9 primitive — Icon wrapper 가 lucide-svelte 의 진입점.
- **xterm theme 어댑터** (별도 task) — xterm 의 `ITheme` 객체를 본 ADR 토큰에서 derive.
- **dark/light 테마 분기** (P2+) — 본 ADR 의 토큰 값이 `[data-theme="light"]` selector 안에서 override 되는 패턴 — 별도 ADR.

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태 / 웹 상태 분리 | **N/A** — 본 ADR 은 frontend chrome 전용 |
| 2 | tmux-native vs web-only 분기 | **N/A** |
| 3 | tmux Layout ≠ Canvas Layout | **N/A** |
| 4 | 보안 기본값 | **PASS** — 본 ADR 은 시각 정본만. lucide-svelte 는 MIT + 활성 + audit 통과. supply chain 모니터링 카테고리 추가 (`docs/ssot/security-defaults.md` 의 dependency review). |
| 5 | control mode 사용 | **N/A** — control mode 는 ADR-0013 으로 폐기 |

## 미해결 항목 (Open)

- **O1.** xterm theme 어댑터의 어느 토큰을 매핑할 지 매트릭스 — xterm 의 `foreground`, `background`, `cursor`, `selection`, 16 ANSI color 와 본 ADR 의 semantic 토큰 매핑은 후속 별도 task.
- **O2.** dark/light 테마 도입 시점 결정 — sketch §15 의 5단계 비범위. 사용자 요구 시 별도 ADR.
- **O3.** lucide-svelte 의 svelte 5 호환 보장 — 현재 호환되나 향후 svelte 6+ 마이그레이션 시 lucide 가 lag 할 가능성. svelte 6+ 도입 시점에 재검토.
- **O4.** A11y color contrast 자동 검증 (WCAG AA 4.5:1) — 본 토큰 값들이 contrast 게이트 통과하는지 일괄 검증 도구 (예: `pa11y`) 도입 여부는 phase 10 (visual polish + a11y) 에서 결정.

## 2026-05-15 Amend ×1 — Figma adaptation 흡수

`docs/reports/0029-frontend-design-ref-analysis.md` 가 `ref/frontend-design/` (Figma-inspired infinity canvas editor 프로토타입) 의 design language 를 분석하면서 본 ADR §D3 의 *명시 값* + §D2 의 일부 규칙이 *재정의* 됨. 본 amend 는 사용자 결정 (Q1=light+dark, Q2=lucide 유지, Q4=ref design language + gtmux 기능 매핑) 4건을 흡수해 다음 D 값을 갱신한다.

### D1 amend — 8번째 카테고리 추가

기존 7 카테고리 (color/spacing/typography/radius/shadow/motion/layout) 에 **canvas** 카테고리 추가. `--canvas-bg` (캔버스 배경) + `--canvas-grid` (24px 도트 그리드) 두 토큰만 보유. SvelteFlow Background 가 본 토큰을 직접 참조하도록 어댑터.

### D2 amend — light/dark 분기 명시

기존 D2 의 "token 값은 `:root` 안에서 한 번만 정의" 룰을 **light = `:root`, dark = `:root.dark`** 두 selector 로 amend. dark/light 토글은 ThemeToggle 컴포넌트가 `<html>` 의 `class="dark"` 를 토글. 기본은 사용자 OS 의 `prefers-color-scheme` 정합 (별도 결정 — *현재 디폴트는 dark*, light 토글은 사용자 명시 동작).

### D3 amend — 명시 값 (정본 supersede)

`tokens.css` 의 정본은 다음으로 supersede. **이전 §D3 의 값은 historical**. (코드 정본은 `codebase/frontend/src/styles/tokens.css` 가 우선.)

```css
:root {
  /* Color */
  --color-bg: #ffffff;
  --color-surface: #ffffff;
  --color-surface-2: #f5f5f5;
  --color-fg: #000000;
  --color-fg-muted: #6b6b6b;
  --color-fg-subtle: #9a9a9a;
  --color-border: rgba(0, 0, 0, 0.10);
  --color-border-strong: rgba(0, 0, 0, 0.18);
  --color-glass-1: rgba(0, 0, 0, 0.06);
  --color-glass-2: rgba(0, 0, 0, 0.10);

  --color-accent: #0d99ff;       /* Figma signature blue */
  --color-accent-fg: #ffffff;
  --color-success: #22c55e;
  --color-warning: #facc15;
  --color-danger: #e5484d;        /* ref delete red */
  --color-info: #0d99ff;          /* M selection — accent 와 정합 */

  /* Canvas */
  --canvas-bg: #f3f3f3;
  --canvas-grid: rgba(0, 0, 0, 0.05);

  /* Spacing (8px base + finer steps) */
  --space-2: 2px;
  --space-4: 4px;
  --space-6: 6px;
  --space-8: 8px;
  --space-10: 10px;
  --space-12: 12px;
  --space-14: 14px;
  --space-16: 16px;
  --space-18: 18px;
  --space-20: 20px;
  --space-24: 24px;
  --space-36: 36px;

  /* Typography */
  --font-sans: 'SF Pro Display', -apple-system, BlinkMacSystemFont,
               'Segoe UI', Roboto, system-ui, sans-serif;
  --font-mono: 'SF Mono', Menlo, 'JetBrains Mono', ui-monospace, monospace;
  --text-xs: 9px;     /* mono section header */
  --text-sm: 10px;    /* mono kbd / small */
  --text-base: 11px;  /* mono input / hint */
  --text-md: 12px;    /* default UI */
  --text-lg: 13px;
  --text-xl: 16px;
  --leading-tight: 1.2;
  --leading-normal: 1.4;
  --weight-regular: 400;
  --weight-medium: 500;
  --weight-semibold: 540;        /* variable weight on canvas display */

  /* Radius */
  --radius-sm: 4px;
  --radius-md: 6px;
  --radius-lg: 8px;
  --radius-pill: 50px;

  /* Shadow — Figma hairline + soft drop */
  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.06),
               0 0 0 0.5px rgba(0, 0, 0, 0.06);
  --shadow-md: 0 8px 24px rgba(0, 0, 0, 0.12),
               0 0 0 0.5px rgba(0, 0, 0, 0.08);
  --shadow-lg: 0 20px 48px rgba(0, 0, 0, 0.18),
               0 0 0 0.5px rgba(0, 0, 0, 0.08);

  /* Motion */
  --motion-fast: 100ms;
  --motion-normal: 200ms;
  --motion-slow: 250ms;
  --motion-easing: cubic-bezier(0.4, 0, 0.2, 1);

  /* Layout */
  --layout-titlebar-h: 44px;
  --layout-toolbar-h: 56px;
  --layout-sidebar-w: 248px;
  --layout-sidebar-right-w: 268px;
  --layout-banner-h: 32px;

  /* z-index (canvas → toolbar → titlebar → banner → modal → toast) */
  --z-canvas: 0;
  --z-canvas-overlay: 18;          /* help-bar, viewport-ctrl */
  --z-rail: 19;
  --z-side-panel: 20;
  --z-toolbar: 25;
  --z-titlebar: 30;
  --z-context-menu: 100;
  --z-banner: 1000;
  --z-modal: 2000;
  --z-toast: 3000;

  /* Legacy banner / zombie tokens (FE-3 D21) */
  --banner-warn-bg: #fef3c7;
  --banner-warn-fg: #78350f;
  --banner-warn-border: #f59e0b;
  --banner-error-bg: #fee2e2;
  --banner-error-fg: #7f1d1d;
  --banner-error-border: #ef4444;
  --zombie-bg: var(--color-surface-2);
  --zombie-fg: var(--color-warning);
}

:root.dark {
  --color-bg: #1e1e1e;
  --color-surface: #2c2c2c;
  --color-surface-2: #383838;
  --color-fg: #f5f5f5;
  --color-fg-muted: #9a9a9a;
  --color-fg-subtle: #6b6b6b;
  --color-border: rgba(255, 255, 255, 0.10);
  --color-border-strong: rgba(255, 255, 255, 0.18);
  --color-glass-1: rgba(255, 255, 255, 0.06);
  --color-glass-2: rgba(255, 255, 255, 0.10);

  /* accent / success / warning / danger / info unchanged
     (Figma accent is theme-agnostic) */

  --canvas-bg: #1a1a1a;
  --canvas-grid: rgba(255, 255, 255, 0.04);

  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.4),
               0 0 0 0.5px rgba(255, 255, 255, 0.06);
  --shadow-md: 0 8px 24px rgba(0, 0, 0, 0.5),
               0 0 0 0.5px rgba(255, 255, 255, 0.08);
  --shadow-lg: 0 20px 48px rgba(0, 0, 0, 0.6),
               0 0 0 0.5px rgba(255, 255, 255, 0.08);

  --banner-warn-bg: #422006;
  --banner-warn-fg: #fde68a;
  --banner-warn-border: #b45309;
  --banner-error-bg: #450a0a;
  --banner-error-fg: #fecaca;
  --banner-error-border: #b91c1c;
  --zombie-bg: #1f2937;
  --zombie-fg: var(--color-warning);
}
```

### D8 신규 — focus ring 정책

기존 `global.css` 의 `:focus-visible { outline: 2px solid var(--color-info); }` 를 **`outline: 2px dashed var(--color-accent); outline-offset: 1px`** 로 supersede. Figma signature dashed 패턴 정합. component-level 별도 focus 스타일 (예: Input 의 `outline-color: accent`) 은 유지.

### D5 reaffirm — lucide-svelte 유지

amend 시점에서 inline SVG 전환 옵션을 검토했으나 (Q2), 사용자 결정으로 lucide-svelte 1.0.1 유지. 신규 chrome 컴포넌트 (Titlebar / Toolbar / RailToggle / ViewportCtrl 등) 도 동일 라이브러리 사용.

### D9 신규 — 폰트 폴백 chain

`--font-sans` / `--font-mono` 는 macOS 의 SF Pro Display / SF Mono 가 1순위. cross-OS 폴백:
- sans: `'SF Pro Display', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, system-ui, sans-serif`
- mono: `'SF Mono', Menlo, 'JetBrains Mono', ui-monospace, monospace`

추가 letter-spacing 정책:
- 본문 (12-13px sans): `-0.14px` (응축, Figma 정합)
- 모노 (10-11px uppercase): `+0.6px` (분산, 가독성)

`global.css` 에서 `body { font-feature-settings: "kern" 1; letter-spacing: -0.14px; }` 적용.

### 영향

- `codebase/frontend/src/styles/tokens.css` — 전면 재작성 (`plan 0005` Stage A)
- `codebase/frontend/src/styles/global.css` — focus + font 보강 (`plan 0005` Stage A)
- 기존 `src/lib/ui/` primitive 9건 — token 참조는 `var(--color-...)` 형태로 동일, *값만* 자동 swap. 단 일부 컴포넌트 (Button.svelte) 의 `--color-surface-1/-2/-3` 참조는 신규 `--color-surface` / `--color-surface-2` / `--color-glass-1/2` 와 매핑 필요 — refactor 1회 (Stage B 점검).
- 기존 컴포넌트 (PanelNode / NewPanelButton / ReconnectBanner / Sidebar / PanelPlaceholder) 도 동일 점검.

## Amend (2026-05-17 ②) — Token-aware ColorPicker preset palette

`ColorPicker.svelte` (Figma-style popover, v4 시안) 의 *Document* swatch grid 가 도입되며, 시안 v4 는 hardcoded preset (Figma brand 색) 을 가정. gtmux 의 *design token* 과 정합하지 않으면 picker 가 *시안 색* 을 노출 → ADR-0016 의 *제한된 팔레트* 정책과 충돌. plan-0010 Task 3 의 미해결 Q ("OKLCH/HSL/hex") 도 본 amend 에서 결정.

### D10. ColorPicker Document preset = token-derived

ColorPicker 의 "Document" row 의 swatch 는 **본 ADR 의 semantic color token 중 사용자 직접 선택이 적합한 token 의 *resolved hex*** 로 구성. 정확한 token list:

| Token | 의미 | 비고 |
|---|---|---|
| `--color-fg` | text / primary | theme 따라 light=black / dark=white |
| `--color-fg-muted` | muted text |  |
| `--color-fg-subtle` | subtle text |  |
| `--color-bg` | background |  |
| `--color-surface-2` | secondary surface |  |
| `--color-accent` | brand / I-target |  |
| `--color-success` | success status |  |
| `--color-warning` | warning status |  |
| `--color-danger` | error status |  |
| `--color-info` | info |  |

- **Alpha-bearing token** (`--color-border`, `--color-glass-1/2` 등 `rgba(_, alpha)` 정의 token) 은 *제외* — swatch grid 의 single-color 의미와 mismatch.
- **Theme-aware**: ColorPicker 의 `resolveCssColor()` 가 *현재 theme 의 computed value* 로 변환 — light → black 계, dark → white 계 자동.
- Recent row 와 별개. Recent 는 사용자 commit 의 LRU, Document 는 token resolve.

### D11. Recent 의 영속 (localStorage)

ColorPicker 의 Recent swatch history (LRU, max 10) 는 module-scope 에서 **localStorage 키 `gtmux:colorpicker:recent`** 에 동기. session-scope 아닌 *device-scope* — 사용자가 새로고침 / 다른 webpage 진입 시에도 recent 유지. private/incognito mode 의 localStorage 차단 시 silent fallback (in-memory only).

### D12. OKLCH format 지원

ColorPicker 의 format toggle 에 **OKLCH** 추가 (기존 HEX / RGB / HSL 외).

- Input 표기: `L%, C, H` (예: `70%, 0.15, 200`) — lightness%/chroma/hue.
- Internal conversion: sRGB → linear RGB → XYZ D65 → OKLab → OKLCH (Björn Ottosson 의 standard formula).
- **Output 은 여전히 hex** — schema (ADR-0018 D3) 의 fill/stroke 는 hex string. OKLCH 는 *display + input* 만, *저장 format 아님*.
- Out-of-gamut color (chroma 가 sRGB 범위 밖) — clip + silent.

### 정합 reference

- `codebase/frontend/src/lib/ui/ColorPicker.svelte` — D10/D11/D12 wire.
- `codebase/frontend/src/styles/tokens.css` — token list 정본 (D10 의 source).

## 변경 이력

- 2026-05-15: 초안 + Accepted — plan 0004 Phase 0 진입 시점. 7 카테고리 토큰 정본 + lucide-svelte iconography + 변경 정책.
- 2026-05-15: amend ① — `ref/frontend-design` Figma adaptation 흡수 (D1 +canvas 카테고리, D2 light/dark 분기, D3 값 supersede, D8 dashed focus 신규, D9 폰트 폴백 신규, D5 lucide 재확인).
- 2026-05-17: amend ② — Token-aware ColorPicker preset palette (D10), Recent localStorage 영속 (D11), OKLCH format (D12). plan-0010 Task 3 의 미해결 Q "OKLCH/HSL/hex" 결정.
