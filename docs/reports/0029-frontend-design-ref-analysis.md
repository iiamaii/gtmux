# Frontend Design Reference Analysis — `ref/frontend-design/`

- 일자: 2026-05-15
- 작성: agent (사용자 요청 "ref/frontend-design 하위 문서를 분석 후 frontend component들 design에 반영")
- 분석 대상: `ref/frontend-design/SPEC.md` (670 lines) + `ref/frontend-design/index.html` (1305 lines)
- 영향: ADR-0016 (Phase 0 design tokens) **major amend / supersede** + plan 0004 (UI/UX 로드맵) 의 §3-§6 구체 다시 작성
- 후속: 본 분석 직후 ADR-0016 v2 + plan 0005 작성 + tokens / primitive 재정의

---

## 0. 한 줄 결론

`ref/frontend-design` 은 **Figma-inspired infinity canvas editor** 의 고밀도 프로토타입 — 본 가이드를 gtmux 의 *터미널 패널 워크스페이스* 에 맞게 adapt 하면 다음 8가지 변경 필요:

1. **Light + Dark 동시 지원** (현재 dark only)
2. **Figma accent `#0d99ff`** (현재 sky `#38bdf8`)
3. **Floating side panels** with shadow + radius (현재 docked sidebar)
4. **44px titlebar + 56px toolbar** 분리 (현재 40px Toolbar 단일)
5. **Bottom-center viewport-ctrl pill** (현재 미존재)
6. **Dashed accent focus ring** — Figma signature (현재 solid info outline)
7. **Inline SVG icons** (현재 lucide-svelte) — 또는 lucide 유지 가능성 별도 결정
8. **8px 베이스 spacing scale + finer steps** (현재 4/8/12/16/24/32/48 — ref 는 2/4/6/8/10/12/14/16/18/20/24/36)

도메인 매핑 차이:
- ref 의 "Panel" 툴 (Figma frame container) ≠ gtmux 의 "Panel" (터미널 패널 시각화) — *어휘 충돌* 주의
- ref 의 Design/Prototype/Inspect 우측 패널 → gtmux 에서는 *Pane Info* 또는 미사용
- ref 의 Rect/Ellipse/Polygon/Pen/Text/Doc 도형 툴 → gtmux 에서는 *New Panel + Session shutdown + Focus* 만
- ref 의 Layers 좌측 패널 → gtmux 의 GroupTree + PanelRow 와 1:1 대응 (이미 부분 구현)

---

## 1. ref 의 design language 요약

### 1.1 토큰

| 카테고리 | ref 값 | 비고 |
|---|---|---|
| 색 | bg/surface/surface-2/fg/muted/border/border-strong/glass-dark/glass-darker/accent | light + dark `:root.dark` 분기 |
| accent | `#0d99ff` (Figma 청록) | 선택 윤곽 / 핸들 / CTA 전용 |
| 그림자 | sm/md/lg = `rgba(0,0,0,.06/.12/.18) + 0.5px hairline` (dark 는 `.4/.5/.6`) | 3 단계 |
| 반경 | sm 4 / md 6 / lg 8 / pill 50 | |
| 폰트 | `SF Pro Display` sans + `SF Mono` mono | font-feature `"kern" 1`, letter-spacing `-0.14px` |
| 모노 | 10–11px uppercase letter-spacing `+0.6px` | 섹션 헤더 / 단축키 / 숫자 |
| 본문 | 12–13px weight 400 | |
| 디스플레이 | variable weight 540 letter-spacing `-2.5px` 까지 | 캔버스 본문 텍스트 — gtmux 는 xterm 이 이미 담당 |
| 간격 | 8px 베이스, 사용 step `2 4 6 8 10 12 14 16 18 20 24 36` | finer + 36 까지 |
| Focus | `outline: 2px dashed var(--accent); outline-offset: 1px` | Figma signature |

### 1.2 레이아웃 (1920×1080 frame, scale-to-fit)

```
+============================================================+
| .titlebar (44px) — hamburger | tabs | center title | …     |
+============================================================+
| .toolbar  (56px) — Page1 | toolgroup | divider | … | Comment|
+----+--------------------------------------------------+----+
|    |  .canvas-stage (dot grid + canvas-transform)     |    |
|    |                                                  |    |
| .left-panel       .help-bar (top-center pill)         | .r |
| (248px floating)                                       |    |
|                                                       |    |
|       .rail.left (펼침/접힘)        .rail.right        |    |
|                                                       |    |
|              .viewport-ctrl (bottom-center pill)      |    |
+----+--------------------------------------------------+----+
```

핵심 차이: panels 가 *floating overlay* (top:8px bottom:8px left:8px width:248px). 컨테이너 docking 아님. 그림자 + radius 로 떠있는 느낌.

### 1.3 인터랙션

- 툴 활성 단일 (`select` 디폴트, `.tool.active` = accent fill + white text)
- Pan: `Space + drag` (cursor `grab` → `grabbing`)
- Zoom: `Cmd/Ctrl + wheel` (pointer-focus 공식 적용)
- 휠 단독: 두 방향 패닝
- 우클릭: contextmenu (좌표 클램프 + 화면 밖 방지)
- 트리 행: 호버 시 lock/vis 아이콘 fade-in, `.on` 시 항상 노출
- 캔버스 marquee: 좌클릭 드래그
- 좌/우 패널 접기 rail: 16×64px 가는 버튼

### 1.4 SVG icon 전략

ref 는 **인라인 SVG**. 외부 폰트/이미지 금지 (§13 "외부 자산 금지"). 모든 아이콘 = 인라인 `<svg>` path. gtmux 가 lucide-svelte 를 도입한 ADR-0016 D5 와 *충돌* — 결정 필요.

옵션:
- (a) **ref 의 인라인 SVG 정신 채택 + lucide 폐기** — bundle 무거움 회피 + 가벼움. ADR-0016 amend.
- (b) **lucide 유지** — 1300+ icon 풀이 미래 feature (CommandPalette, Sidebar tabs 등) 에 유리. 의존성 1건.
- (c) **혼합** — chrome (titlebar/toolbar) 는 인라인 SVG (ref 정합), feature (palette item, status indicator) 는 lucide. 복잡도↑.

---

## 2. gtmux 도메인 매핑

ref 의 *Figma canvas editor* → gtmux *터미널 워크스페이스* 로 어휘 변환:

| ref 항목 | gtmux 매핑 | 비고 |
|---|---|---|
| Title bar — File path "Acme Studio / Onboarding Flow — v3" | "gtmux · `demo` · Saved 2m ago" 또는 "gtmux · `demo` · 127.0.0.1:9999" | session 이름 + 부가 정보 |
| Hamburger menu (5 tabs: File/Edit/View/Object/Help) | dropdown 한 개 — "Session shutdown / Rotate token / About" 만 | gtmux MVP scope 한정 |
| Theme toggle (해 ↔ 달) | 그대로 채택 | light/dark 둘 다 지원 |
| Avatar stack | **미사용** — single-user invariant (sketch §13). 또는 connection status dot 로 대체 | |
| Share (ghost pill) | **미사용** — sharing 비범위 (sketch §13) | |
| Present (검정 pill) | **Focus mode 토글** 로 대체 — 풀-스크린 + 단일 패널 강조 | CONTEXT.md §"Focus mode" |
| Toolbar — Page 1 ▾ | 미사용 또는 future "Sub-canvas" 개념 — MVP 미적용 | |
| Toolbar — Select | Hand | **Select / Pan** — Pan tool 이 명시적이면 Space-drag UX 보조 | |
| Toolbar — Panel tool | **"New Panel" 액션** (현재 NewPanelButton 으로 이미 구현) — 툴 형태로 통합 가능 | |
| Toolbar — Rect/Ellipse/Polygon/Pen/Text/Doc/Caption | **모두 미사용** — gtmux 는 도형 도구 없음 | |
| Toolbar — Comment | **미사용** | |
| Left panel (Layers tab) | **현 Sidebar (GroupTree + PanelRow)** 그대로 — 이미 layer 트리 형태 | |
| Left panel — Assets / Pages 탭 | **미사용** | |
| Right panel (Design/Prototype/Inspect) | **MVP 미구현 / future** — Pane Info 패널 (pane_id / label / locked / visible 속성) 검토 | |
| Right panel — Selection / Layout / Typography / Fill / Effects / Export sections | 미사용 — gtmux 는 도형 속성 없음 | future Group color picker 정도 |
| Canvas dot grid | **그대로 채택** — Sketch dot pattern 정합 | radial-gradient |
| Canvas-transform pan/zoom | **기존 SvelteFlow 가 담당** — 본 ref 의 vanilla 구현은 우리는 사용 X. 단 SvelteFlow 의 외부 UI (viewport-ctrl pill) 는 채택 가능 | |
| Selected outline + 4 handles | SvelteFlow 의 기존 selection 시각 — Figma 스타일 (1.5px accent outline + 4 코너 핸들) 로 매칭 | |
| Viewport ctrl (bottom pill) | **그대로 채택** — 줌 in/out/100%/fit + (gtmux 추가) M count badge / Connection status dot | |
| Help bar (top-center pill, "Space + drag · pan ...") | **그대로 채택** — gtmux 의 단축키 hint | |
| Context menu (우클릭) | **부분 채택** — gtmux 의 패널 관련 동작 (Copy pane_id, Close pane, Hide, Lock 등) 만. 형태는 ref 의 구조 | |

---

## 3. 변경 사항 매트릭스 (vs 현재 코드)

| 영역 | 현재 (Phase 0+1 v1) | ref 기반 목표 | 작업량 |
|---|---|---|---|
| `tokens.css` color | dark only, sky `#38bdf8` | light + dark, Figma `#0d99ff` | **재작성** |
| `tokens.css` shadow | 단색 rgba 4 단계 | 그림자 + 0.5px hairline, light/dark 분기 | 재작성 |
| `tokens.css` typo | system-ui | SF Pro / SF Mono 우선 | minor |
| `tokens.css` spacing | 4/8/12/16/24/32/48 | 2/4/6/8/10/12/14/16/18/20/24/36 | 확장 |
| `global.css` focus | solid info outline | dashed accent outline | swap |
| `+page.svelte` layout | Toolbar 40 + workspace (sidebar+canvas) | titlebar 44 + toolbar 56 + workspace (floating panels + canvas) | **rewrite** |
| `Toolbar.svelte` | 8 LOC placeholder | titlebar (hamburger + tabs + center + theme + Focus) + toolbar (Select/Pan/New Panel) 두 컴포넌트로 분리 | rewrite |
| `Sidebar.svelte` | docked 260px | floating 248px (8px gap) + tabs + collapse rail | minor refactor + 새 chrome |
| New `Titlebar.svelte` | (없음) | 44px header chrome | **신규** |
| New `ViewportCtrl.svelte` | (없음) | bottom-center pill (zoom in/out/100%/fit) | **신규** |
| New `HelpBar.svelte` | (없음) | top-center pill (단축키 hint) | **신규** |
| New `RailToggle.svelte` | (없음) | side panel 접기 버튼 | **신규** |
| New `ContextMenu.svelte` | (없음) | 우클릭 메뉴 | **신규** |
| `PanelNode.svelte` | 부분 토큰화 | Figma selection outline + handles (선택 시) | 시각 정합 |
| `Canvas.svelte` | SvelteFlow + NewPanelButton overlay | + viewport-ctrl + help-bar + rail toggle 마운트 | 추가 |
| `ui/` primitives | dark 만, sky accent, lucide | light/dark, Figma accent, 인라인 SVG (또는 lucide 유지) | 토큰 매핑 + 색만 변경 / 구조 동일 |
| `ui/Icon.svelte` | lucide-svelte wrapper | 인라인 SVG path 매핑 (옵션 a) 또는 유지 (옵션 b) | 결정 필요 |
| ADR-0016 | v1 (sky + lucide + dark only) | v2 (Figma accent + light+dark + 인라인 SVG 또는 lucide) | **amend** |
| 신규 ADR-0017 | (없음) | "Layout overhaul — Figma adaptation" | **신규** |

---

## 4. 핵심 결정 5건 (사용자 확인 필요)

### Q1. Light theme 지원 여부
- **(a)** Light + Dark 둘 다 지원 (ref 정합, theme toggle 버튼 존재) — token 분기 + toggle 컴포넌트 + 모든 컴포넌트 light 테스트
- (b) Dark only — ref 의 light 토큰은 무시, sketch 의 *기본 dark* 만

### Q2. Iconography 전략
- (a) ref 정합 — 인라인 SVG 만 사용, lucide-svelte 제거 → bundle↓, 작성 비용↑
- **(b)** lucide-svelte 유지 (현재 도입 상태) — 일관성 + 풀이 풍부 + tree-shaking
- (c) 혼합 — chrome 인라인 SVG, feature lucide → 복잡도↑

### Q3. Right panel (Design / Pane Info) 처리
- (a) 미구현 — 좌측 sidebar 만, 우측 영역은 캔버스 확장
- **(b)** Pane Info 우측 패널 — pane_id / label / locked / visible 속성 표시 (read-only v0, 편집 P1+)
- (c) Future placeholder — 비어 있는 floating panel 만 렌더 (visual completeness)

### Q4. 레이아웃 정합 수준
- **(a)** ref 의 *디자인 언어* 채택 (floating panel, titlebar+toolbar 분리, viewport-ctrl pill) + gtmux 어휘로 *기능 매핑* (Q3 적용)
- (b) ref 의 1:1 시각 + gtmux 기능 매핑 — 그 외 모든 ref 표현 채택
- (c) ref 는 *영감* 만, 현 docked 구조 유지 + tokens 만 Figma 정합

### Q5. ADR / plan 전개 순서
- **(a)** 분석 doc (본 0029) + ADR-0016 v2 amend + plan 0005 신규 → 코드 변경
- (b) ADR-0016 v2 만 amend + plan 0004 안에서 보완
- (c) ADR / plan 동시 + 즉시 코드

---

## 5. 미해결 (Open questions)

- **O1**: `ref/frontend-design/index.html` 은 데모 sample scene (Onboarding · iPhone artboard / cards / sketch / polygon / doc) 포함 — gtmux 의 빈 캔버스 empty state 가 이 sample 의 어떤 측면을 *시각적으로* 차용할지 (스플래시 일러스트레이션 등).
- **O2**: SF Pro Display 폰트는 macOS 시스템 폰트로만 보장. cross-OS 폴백 정합 (`-apple-system, BlinkMacSystemFont` 등) — 현 토큰 의 `var(--font-sans)` 폴백 chain 검증.
- **O3**: dashed focus ring 이 SvelteFlow 기본 selection 과 충돌하는지 — SvelteFlow 의 `.svelte-flow__node.selected` 스타일 override 정책.
- **O4**: ref 의 1920×1080 fixed frame + scale-to-fit 정책은 *프로토타입 데모* 용. gtmux 는 fluid 레이아웃 — frame 폐기 + viewport-fluid 가 정합.

---

## 6. 다음 액션 (사용자 응답 후)

| Q 응답 | 작업 |
|---|---|
| Q1=a, Q2=b, Q3=b, Q4=a, Q5=a (모두 권장 옵션) | ADR-0016 v2 amend (light+dark tokens, Figma accent, lucide 유지) + plan 0005 (layout overhaul: titlebar+toolbar 분리, floating panels, viewport-ctrl) + tokens.css 재작성 + primitive 토큰 매핑 + +page.svelte rewrite + 신규 chrome 컴포넌트 5건 (Titlebar / Toolbar 분리 / ViewportCtrl / HelpBar / RailToggle / ContextMenu) |
| Q1=b (dark only) | tokens 분기만 제거, 외 동일 |
| Q2=a (인라인 SVG) | lucide-svelte 제거 + 인라인 SVG path 모듈 작성 + Icon.svelte 재작성 |
| Q3=a / c | right panel 컴포넌트 작성 생략 또는 빈 placeholder |
| Q4=c (영감만) | tokens 만 swap + 레이아웃 rewrite 없음 |
| Q5=b / c | 단계 압축 |

---

## 변경 이력

- 2026-05-15: 초안 — `ref/frontend-design/` 분석 + gtmux 도메인 매핑 + 5 결정 surface.
