# Session Handover — 2026-05-17 — ColorPicker v4 redesign + design unification + theme hot-reload investigation

> 이 문서는 `session-handover` skill 로 생성된 session 인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-17 (저녁)
> - 생성 session 의 마지막 commit: `e006962` (Fix xterm theme buffer preservation — 사용자 명시 fix)
> - 본 session 의 주요 주제: ColorPicker 의 Figma-style popover 전면 재구성 (Phase 1~4), Inspector InspectorField + ColorPicker inline 의 box 디자인 통일, theme hot-reload 의 cell stale 색 회귀 진단 + 7 fix 시도 + 사용자 final fix, ADR draft 3 종 + Settings 의 theme entry 격리, brand favicon + auth page brand layout, 5 minor UX regressions (corner handle / ghost z-index / DocumentNode plain / AuthDialog focus / etc.)
> - 같은 날 다른 worker 의 handover (`2026-05-17-session-handover-canvas-tools-and-file-picker.md`, `2026-05-17-session-handover-maximize-modal-and-ui-batch.md`, `2026-05-17-session-handover-component-design-batch.md`) 와 *별도* — 본 handover 는 ColorPicker 중심 batch + theme investigation 의 후속 *이후* 영역

---

## 1. 프로젝트 개요

- **이름**: gtmux
- **한 줄 정체성**: tmux 를 backend execution engine 으로 쓰는 single-user web canvas workspace — tmux 가 process/session lifecycle 의 진실, FE 가 canvas layout 의 진실
- **현재 phase / 단계**: Stage 7+ (multi-session pivot 완료, UI/UX 시안 정합 batch 마무리, ColorPicker v4 시안 land 완료, theme hot-reload root cause 사용자 fix land, ADR 0030/0031/0032 신규 draft)
- **침범 불가능한 invariants**:
  - **두 state 분리**: tmux state (mirror only) ↔ web state (FE 진실) — `docs/sketch.md` §4 + `CLAUDE.md`
  - **single-attach + no-takeover**: Webpage:Session = 1:1 — `docs/adr/0019-session-and-workspace-model.md` D3/D4
  - **control-mode integration**: tmux CLI shell-out 금지 — `docs/adr/0021-terminal-pool-and-mirror.md`
  - **ADR-before-code hard rule**: 비-trivial 결정은 ADR 우선, ADR amend 시 linked plan/handover 도 갱신 — `CLAUDE.md`
  - **Layout ≠ tmux layout**: 캔버스 free 배치 ≠ tmux split — `docs/sketch.md` §4
  - **Undo/Redo 단일 entry**: 모든 user-driven layout mutation 은 `sessionStore.applyMutation` 통과 (ADR-0028 Phase 3) — 직접 `mutateLayout` 호출 금지
  - **ColorPicker / InspectorField box 디자인 통일**: h22 / bg-bg / 1px border / hover border-strong / focus border-accent — 시안 v2 의 `.input` (h28 / bg-2 / transparent border) 은 supersede

## 2. 현재 session 요약

본 session 의 작업 흐름 (시간순 + 그룹화):

### 2.1 UI 디자인 시안 정합 — brand / auth / favicon

- `cf4bb69` style(frontend/file-path): fp-foot placeholder meta + v3 .sep / .right
- `a9a9e09` style(frontend/ColorPicker): swatch 28 → 26px
- `17d4d62` fix(frontend/toolbar): click 후 button focus retention 차단 (ESC 후 outline 잔류 회피) — toolbar button 의 onclick 에 `e.currentTarget.blur()`. Tab navigation 의 focus-visible 은 유지
- `c6f8ac3` feat(frontend): favicon — b_icon.png 의 16/32/180 PNG 3-set 적용 (sips export, `public/` 신규)
- `28a0be6` fix(frontend/file-path): fp-foot 의 placeholder 항상 표시
- `2f52f0e` style(frontend/ColorPicker): swatch 26 → 22px (4px 추가 축소)
- auth page brand: topbar 좌측 brand image + title 제거 → card heading 위치에 brand-mark (56px) + h1 "gtmux" 가로 배치 + cap-center nudge `translateY(-4px)` + h1 font-size 28→40
- Titlebar brand: brand-mark 22→30→27px, brand-name 15→18→16px, letter-spacing -0.3 → -0.25, .brand-name `translateY(-1px)`, SessionMenu 가 좌측 첫 자리

### 2.2 Tool UX 보강

- ESC fix: `escRouter` 의 listener attach 가 *register 시점* — InlineEdit 미사용 fresh 진입 시 fallback chain 미동작 → module load 시 eager `attach()` 호출 (`escRouter.svelte.ts`)
- Text tool cursor: `isTextTool` derived + `.text-cursor` class + `cursor: text` (I-beam) — drag-tool (crosshair) 와 별도

### 2.3 ColorPicker v4 시안 전면 재구성 (Phase 1~4)

옛 inline (24×22 swatch + hex + alpha 한 줄) → Figma-style popover (`ref/frontend-design/components-v4.html §.shape-colorpicker`).

| Commit | Phase | 내용 |
|---|---|---|
| `fab05b8` | 1 | Visual shell — trigger swatch (22×22) + popover (240px width) head/modes/SV/sliders/value/swatches markup + CSS + token 매핑 |
| `98d7d1e` | — | popover viewport clamp — `position: fixed` + JS getBoundingClientRect + viewport margin. resize/scroll reflow |
| `87b995b` | 2 | SV/hue/alpha drag — pointerdown→move→up, draft preview, drag end 1회 commit. `setPointerCapture`, `clamp01`, `hsvToHex` |
| `2ed7c8f` | 3 | Format toggle (HEX/RGB/HSL) + Eyedropper (window.EyeDropper feature detect) + Recent swatch (module-scope LRU, max 10) |
| `d0ffe2e` | 4 | OKLCH format (Björn Ottosson formula) + Recent localStorage 영속 (`gtmux:colorpicker:recent`) + Token-aware Document palette (10 semantic tokens) + **ADR-0016 amend ② D10/D11/D12** |
| `21971ed` | — | RightPanel 좌측 anchor (`.right-panel` rect.left - popover.width - 8px) + mode tabs 제거 (Solid 외 gradient mode disabled — 별 spec) |
| `bcd06ad` | — | trigger 옆 inline hex + alpha input 복원 (옛 layout 으로 회귀 — popover 닫혀도 색 인지/편집 가능) |
| `58da323` | — | InspectorField 의 box 디자인을 ColorPicker inline 과 통일 (h22 + bg-bg + visible border) |
| `302fadb` | — | inline hex/alpha 에 `.k` prefix label (HEX / A) — InspectorField (X/Y/W/H/Z) 와 동일 패턴 |

### 2.4 5 minor UX regressions

`95b10f9` fix(frontend): 5 minor UX regressions:
1. XtermHost theme refresh — `clearTextureAtlas` + `refresh(0, rows-1)` (단 *실 효과 없음* — §2.5 참조)
2. PanelNode `.panel { overflow: visible }` — corner handle clip 회피
3. Tool guide `.point-spawn-ghost` / `.drag-ghost` z-index 99 → `var(--z-canvas-overlay)` (18) — side-panel (20) 보다 아래
4. DocumentNode 의 InlineEditTextarea `plain={true}` 추가
5. AuthDialog `.choice:focus-visible` outline 제거 + border-color accent — modal autofocus 의 dashed outline 거슬림

후속 `07e737d` fix(frontend/panel): corner resize handle 의 z-index 명시 (xterm-viewport stacking 위) — `.panel-resize-handle { z-index: 10 !important }`.

추가 `7034051` style(frontend): align-btn 아이콘 통일 (12×12 sw1.5 → 14×14 sw1.2 viewBox 16) + tool guide stroke 축소 (1.5/2 → 1).

### 2.5 Theme hot-reload investigation (미해결 → 사용자 final fix)

**현상**: theme toggle 시 chrome (token) 즉시 반영, terminal cell 색만 stale → 새로고침 외 복구 X.

**7 fix 시도** (A~G, 모두 효과 없음 또는 부작용):
- A: clearTextureAtlas + refresh — × stale 그대로
- B: + .xterm root display 토글 reflow — × stale 그대로
- C: cell span inline color reset + refresh — × stale 그대로
- D: PanelNode `{#key themeStore.resolved}` (`092c8e9`) — XtermHost remount 되지만 BE replay 안 옴 + reactive cascade 폭주 (log 빠르게 찍힘)
- E: silentReattach $effect (`6b02f65`) — 무한 loop (sessionStore.reattachInProgress reactive dependency 추적) — 즉시 revert (`40966ab`)
- F: SettingsOverlay setMode auto reload (`e195288`) — modal 자체도 unmount (ADR-0017 D2 Auto-save 정책 위반) — revert (`1213d73`)
- G: ThemeToggle Titlebar 제거 + Settings 만 entry — chrome 만 반영, terminal stale 그대로

**또 부수 작업**:
- TS generic `<HTMLElement>` 표기 (comment 안 raw text 포함) 가 svelte parser 가 HTML tag 로 오인 — `instanceof HTMLElement` + 꺾쇠 자체 제거로 회피 (`084984c` → `e4d0ab8` → `f42fde3` → `7b400c3`)
- `bde370e` style(frontend): Figma-signature dashed accent focus ring 제거 — 버튼 테두리 파란 dashed 효과 전반 제거

**Investigation report**:
- `docs/reports/0062-theme-hot-reload-investigation.md` (`4d25f40`) — 모든 시도 + ADR 정합 + dead code candidate 기록. *정정 note* 추가: 최종 root cause = `XtermHost.svelte` mount `$effect` 의 `themeStore.resolved` reactive dependency 로 인한 xterm remount/buffer loss. 해결 보고서 `0063-xterm-theme-buffer-preservation-resolution.md` 참조 (사용자 작성 예정 또는 별 worker 가 작성).

**사용자 final fix** (`e006962` "Fix xterm theme buffer preservation"):
- XtermHost mount $effect 안 `theme: { ...xtermTheme(untrack(() => themeStore.resolved)) }` — `untrack()` 으로 *mount 시 themeStore reactive 추적 차단*. theme 변경 시 *재 mount 안 되며* live theme effect 가 in-place repaint.
- SettingsOverlay 의 section-hint 의 "If terminal cell colors look stale after switching, reload the page" 안내 제거 (이제 정상 작동).

### 2.6 ADR / 보안 draft 신규 + ADR amend

- `6e43abc` docs(adr+report): ADR-0030 (canvas-item clipboard) + ADR-0031 (figure modifier constraint Shift/Alt) + ADR-0032 (multi-select context menu) 신규 draft + handover 0057 (FE) / 0058 (BE)
- `3644f1b` docs(item-schema): 보완 기능 3종 잔여 등록 — text style / figure pattern / rotation
- ADR-0016 amend ② (commit `d0ffe2e` 안 포함) — D10 token-aware ColorPicker preset palette / D11 Recent localStorage 영속 / D12 OKLCH format 지원

### 2.7 Settings + Titlebar 의 theme entry 정합 (ADR-0017 D2)

- `cf4a5ae` revert: PanelNode `{#key}` 제거
- `40966ab` revert: +page.svelte silentReattach $effect 제거
- `1213d73` fix(frontend/settings): theme setMode 의 auto reload 제거 (ADR-0017 D2 Auto-save 정책 정합) — modal 유지
- Titlebar 의 `<ThemeToggle />` 제거 → theme 변경 entry = SettingsOverlay 안만

### 2.8 결정 사항 (사용자 합의 / 거부 포함)

- **ColorPicker v4 시안 patten** = popover (240px) + trigger swatch (22×22 inline). 옛 inline 한 줄 폐기
- **Document palette = token-aware** (10 semantic tokens, light/dark 자동 변환). v4 시안의 hardcoded brand 색 거부
- **Format toggle** = HEX/RGB/HSL/OKLCH 4 mode. OKLCH 추가 (ADR-0016 D12). 출력 schema 는 항상 hex (ADR-0018 D3 정합)
- **Recent swatch** = module-scope LRU + localStorage 영속 (key `gtmux:colorpicker:recent`, max 10)
- **ColorPicker mode tabs (Solid/Linear/Radial/Angular/Image)** = 시안 보유 단 본 phase 에선 *Solid 외 제거*. gradient fill 은 별 spec
- **ColorPicker popover position** = RightPanel left edge anchor + viewport clamp. `position: fixed` + JS reposition
- **InspectorField box 디자인 = ColorPicker inline 정합** (h22 / bg-bg / 1px border / hover border-strong / focus border-accent). 옛 시안 v2 `.input` (h28 / bg-2 / transparent border) 은 supersede
- **Theme 변경 entry = SettingsOverlay 만**. Titlebar 의 ThemeToggle button 거부 (잦은 toggle 시 회귀 회피). Settings 의 modal 자동 닫힘 거부 (ADR-0017 D2 정합)
- **Theme hot-reload root cause** = XtermHost mount $effect 의 themeStore.resolved reactive dependency. fix = `untrack()` (사용자 final)
- **거부된 fix 접근**: clearTextureAtlas / display 토글 / cell span inline reset / `{#key}` / silentReattach $effect / auto reload — 모두 부작용 또는 효과 없음
- **AuthDialog choice button focus**: `:focus-visible` 의 dashed 2px outline → border-color accent (modal autofocus 의 첫 화면 인디케이터 거슬림)
- **Inline edit plain mode**: PanelNode 의 label / NoteNode 의 title+body / FilePathNode 의 path / MaximizedItemModal 의 note / DocumentNode 의 content — 모두 `plain={true}` (box border 제거)
- **WorkspaceEmptyPlaceholder 제거** + AuthDialog 의 `dismissable` prop (session 없을 때 dismiss 불가능)
- **Cursor mode**: text tool = I-beam (`cursor: text`), drag tool (rect/ellipse/line/free_draw) = crosshair

### 2.9 변경된 파일 (이번 session, commit 단위 누적)

| 파일 | 변경 요약 |
|---|---|
| `codebase/frontend/src/lib/ui/ColorPicker.svelte` | 전면 재구성 — Figma-style popover (Phase 1~4). 1600+ lines |
| `codebase/frontend/src/lib/chrome/InspectorField.svelte` | box 디자인 ColorPicker inline 정합 (h22 / bg-bg / 1px border) |
| `codebase/frontend/src/lib/chrome/SettingsOverlay.svelte` | setMode = themeStore.setMode 만. section-hint 의 manual reload 안내 제거 (사용자 fix 후) |
| `codebase/frontend/src/lib/chrome/Titlebar.svelte` | ThemeToggle import + mount 제거. brand size + nudge 조정 (Session 직전 fix 의 후속) |
| `codebase/frontend/src/lib/chrome/AuthDialog.svelte` | dismissable prop (session 없을 때 dismiss 불가능). choice:focus-visible outline 제거 |
| `codebase/frontend/src/lib/chrome/MaximizedItemModal.svelte` | (다른 worker WIP 잔재) — 사용자 부분 수정 |
| `codebase/frontend/src/lib/chrome/ItemInfoView.svelte` | align-btn icon 통일 (12→14, sw1.5→1.2). ColorPicker swatch markup 갱신 |
| `codebase/frontend/src/lib/canvas/Canvas.svelte` | `.point-spawn-ghost` / `.drag-ghost` z-index → `var(--z-canvas-overlay)`. text-cursor class. stroke 1.5/2 → 1 |
| `codebase/frontend/src/lib/canvas/PanelNode.svelte` | `.panel { overflow: visible }` (corner handle). `.panel-body { overflow: hidden }` (xterm overflow 격리). `.panel-resize-handle { z-index: 10 }`. (직전 themeStore import / `{#key}` 시도 모두 revert) |
| `codebase/frontend/src/lib/canvas/XtermHost.svelte` | 사용자 final fix `e006962` — Terminal init 의 theme 에 `untrack()` (mount $effect 의 themeStore reactive 추적 차단) |
| `codebase/frontend/src/lib/canvas/DocumentNode.svelte` | InlineEditTextarea 에 `plain={true}` 추가 |
| `codebase/frontend/src/lib/canvas/NoteNode.svelte` | 사용자 명시 수정 (직전 session 의 후속) |
| `codebase/frontend/src/lib/common/InlineEditField.svelte` / `InlineEditTextarea.svelte` | `plain` prop 표준 (canvas item 의 inline edit 통일) |
| `codebase/frontend/src/lib/common/escRouter.svelte.ts` | module load 시 eager `attach()` — InlineEdit 미사용 fresh 진입 시 fallback chain 동작 |
| `codebase/frontend/src/lib/toolbar/Toolbar2.svelte` | onclick 후 `e.currentTarget.blur()` (focus retention 차단) |
| `codebase/frontend/src/routes/auth/+page.svelte` | brand layout 변경 — topbar 좌측 brand 제거, card heading 위치에 brand-mark + h1 가로 배치 |
| `codebase/frontend/src/routes/+page.svelte` | (다른 worker 의 ImportSession/ExportSession Modal import 추가 — 직전 session 의 후속) |
| `codebase/frontend/public/` (신규) | favicon-16x16 / favicon-32x32 / apple-touch-icon (180×180) PNG. b_icon.png 의 sips export |
| `codebase/frontend/index.html` | `<link rel="icon">` × 3 추가 |
| `codebase/frontend/src/lib/assets/brand.png` | b_icon.png 사본 (옛 brand-G.png 삭제) |
| `docs/adr/0016-design-tokens-and-iconography.md` | amend ② (D10 token palette / D11 Recent localStorage / D12 OKLCH) |
| `docs/adr/0030-canvas-item-clipboard.md` (신규) | Draft — FE-only clipboard, terminal clone, paste offset (24,24), schema 정합 |
| `docs/adr/0031-figure-input-modifier-constraint.md` (신규) | Draft — Shift constraint (rect/ellipse 1:1, line angle hold), Alt center-anchor P1 |
| `docs/adr/0032-multi-select-context-menu.md` (신규) | Draft — ContextMenu mode 분기, M-replace, batch action matrix |
| `docs/reports/0057-fe-handover-clipboard-shift-rightclick.md` (신규) | FE agent 인계 — 5-phase 진행 권장 |
| `docs/reports/0058-be-handover-clipboard-shift-rightclick.md` (신규) | BE agent 인계 — Slice B (terminal clone POST endpoint) 만 |
| `docs/reports/0062-theme-hot-reload-investigation.md` (신규) | theme hot-reload 7 fix 시도 + 사용자 final 정정 note |

미커밋 변경 (working tree):
- `D docs/src/converted_logo.svg` — deleted (untracked source)
- `?? docs/reports/2026-05-17-session-handover-canvas-tools-and-file-picker.md` — 다른 worker handover, untracked
- `?? ref/frontend-design/components-v3.html` / `components-v4.html` — design ref, untracked (정합 안정 후 commit 권장)

## 3. 주요 참조 자료

| 영역 | 경로 | 왜 읽어야 하는가 |
|---|---|---|
| 프로젝트 instructions | `CLAUDE.md` | 언어 컨벤션 (docs KO / code EN), ADR-before-code, MCP graph 우선, applyMutation 단일 entry |
| 스펙 | `docs/sketch.md` | scope/MVP/우선순위/threat model (KO) |
| 본 session 의 신규 ADR draft | `docs/adr/0030-canvas-item-clipboard.md`, `docs/adr/0031-figure-input-modifier-constraint.md`, `docs/adr/0032-multi-select-context-menu.md` | 3 미설계 기능 — copy/cut/paste, Shift constraint, 다중 right-click menu |
| 본 session 의 ADR amend | `docs/adr/0016-design-tokens-and-iconography.md` (amend ② line 387~436) | D10/D11/D12 — token-aware palette, Recent localStorage, OKLCH |
| ColorPicker 시안 | `ref/frontend-design/components-v4.html` §.shape-colorpicker (line 889~1158, markup ~2455-2530) | popover 정본 |
| Theme investigation | `docs/reports/0062-theme-hot-reload-investigation.md` | 7 fix 시도 + 사용자 final fix note + dead code candidate |
| 활성 plan | `docs/plans/0011-component-design-batch-caption-document.md` | caption / document FE Slice-A2 inline edit wire 완료 (직전 commit `64245ee`), 추가 wire 후속 |
| FE / BE handover | `docs/reports/0057-fe-handover-clipboard-shift-rightclick.md`, `docs/reports/0058-be-handover-clipboard-shift-rightclick.md` | clipboard / Shift constraint / multi-select context menu 의 5-phase 진행 가이드 |
| 다른 worker 의 같은 날 handover | `docs/reports/2026-05-17-session-handover-canvas-tools-and-file-picker.md` (untracked), `…component-design-batch.md`, `…maximize-modal-and-ui-batch.md` | 본 session 의 전 batch — 시간순으로 *직전* 의 context |
| Theme/Settings ADR 정합 | `docs/adr/0017-layout-grid-and-chrome.md` (amend ④ D2 Auto-save, D5 xterm theme hot reload 다음 amend) | Settings 의 modal 정합 + theme hot reload 의 정본 결정 위치 |
| Undo/Redo policy | `docs/adr/0028-undo-redo-policy.md` | applyMutation 단일 entry + history capture 정책 |

## 4. 진행중인 작업

본 session 의 commit 자체는 모두 land. 다만 다음 항목들은 *후속 작업 대상* 으로 남음.

### 4.1 Theme hot-reload resolution report (0063) — 사용자 명시 reference

- **상태**: 미작성 (사용자가 0062 의 정정 note 에 reference 만 추가)
- **관련 문서**: `docs/reports/0062-theme-hot-reload-investigation.md` line 5 의 "정정(2026-05-17): … `0063-xterm-theme-buffer-preservation-resolution.md` 를 참조"
- **관련 commit**: `e006962` "Fix xterm theme buffer preservation"
- **다음 한 step**: `docs/reports/0063-xterm-theme-buffer-preservation-resolution.md` 신규 작성:
  1. Root cause: `XtermHost.svelte` mount `$effect` 가 `themeStore.resolved` 를 read → reactive dependency 가 됨 → theme 변경 시 `$effect` 재발화 → cleanup 호출 → term.dispose → 새 Terminal 생성 → buffer 손실
  2. Fix: `untrack(() => themeStore.resolved)` 으로 mount 시 *값만* read + dependency 추적 차단. 별 G27 hot-reload `$effect` 가 *별도* 로 `themeStore.resolved` 추적 + `term.options.theme` swap → in-place repaint
  3. ADR-0017 amend ④ D5 의 "xterm theme hot reload — 다음 amend" → *resolved* 처리

### 4.2 Dead code 정리 (0062 §8)

- **상태**: 정리 안 됨 (사용자 결정 미)
- **관련 파일**:
  - `codebase/frontend/src/lib/toolbar/Toolbar.svelte` (옛 toolbar — `Toolbar2` 가 active, Toolbar 어디서도 import 없음)
  - `codebase/frontend/src/lib/ui/ThemeToggle.svelte` (Titlebar 제거 후 사용처 없음 — Toolbar 도 dead)
- **다음 한 step**: 두 파일 `rm` + grep 으로 잔존 reference 확인 + commit `chore: dead toolbar / theme-toggle 정리`

### 4.3 untracked refs / source asset

- **상태**: working tree 의 untracked
- **관련 파일**:
  - `ref/frontend-design/components-v3.html` / `components-v4.html` — v4 가 ColorPicker spec 의 정본 (이번 session 의 모든 ColorPicker 작업 의 source). repo 에 commit 권장
  - `docs/reports/2026-05-17-session-handover-canvas-tools-and-file-picker.md` — 다른 worker handover, untracked. commit 또는 그 worker 가 처리할 가능성
  - `D docs/src/converted_logo.svg` — deleted, 사용 흔적 없으면 commit 으로 정리
- **다음 한 step**: `git status` 확인 → 본 영역 미본인 commit 이면 다른 worker 와 협의

### 4.4 plan-0011 (Caption / Document) 후속

- **상태**: FE Slice-A2 (document inline edit wire) 완료 — 직전 commit `64245ee`. 후속 Slice 미land
- **관련 문서**: `docs/plans/0011-component-design-batch-caption-document.md`
- **다음 한 step**: plan-0011 의 다음 Slice 확인. CaptionNode 의 inline edit wire 또는 BE / schema 후속

## 5. 향후 작업

### 5.1 ADR-0030/0031/0032 의 implementation (handover 0057/0058 정합)

- **목표**: 3 미설계 기능 (clipboard / Shift constraint / multi-select context menu) FE/BE 구현
- **관련 문서**: ADR-0030/0031/0032 + handover 0057 (FE 5-phase plan) + handover 0058 (BE Slice B 만)
- **선행 조건**: ADR-0030 의 terminal clone Slice B 만 BE 의존 (`POST /api/terminals` 존재 확인)
- **권장 phase 1**: ADR-0031 (Shift constraint) — schema/BE 영향 0, 시각 효과 즉시. tool drawing + NodeResizer 두 callsite

### 5.2 ColorPicker mode tabs (gradient fill)

- **목표**: 현재 Solid 외 mode (Linear / Radial / Angular / Image) — `21971ed` 에서 *제거* 상태. gradient fill 의 별 spec 필요
- **선행 조건**: ADR-0018 D3 의 `fill: string` 을 *gradient struct* 로 확장? 또는 별 type
- **다음 한 step**: gradient fill 의 schema 결정 (string vs object) + 별 ADR draft

### 5.3 ColorPicker eyedropper FE / BE

- **목표**: 현재 EyeDropper Web API 만 (Chrome/Edge). Safari/Firefox 의 polyfill
- **선행 조건**: 별 API or canvas screenshot — 보안 표면 검토 필요
- **다음 한 step**: P5 — 현 phase 외

### 5.4 Inline edit 의 추가 통일

- **목표**: TextNode 의 inline edit 도 `plain={true}` 통일 (이미 그렇게 land 가능성, 확인 필요)
- **다음 한 step**: TextNode.svelte 의 InlineEditTextarea 호출 확인

### 5.5 brand-icon 최적화 (handover 0057 §5.7 이월)

- **목표**: brand.png (514 KB) → 작은 PNG 또는 SVG. Bundle 줄임
- **다음 한 step**: b_icon.png 의 작은 사이즈 재 export 또는 SVG 변환

### 5.6 PanelNode disk persistence (직전 handover §5.3 이월)

- **목표**: `sessionStore.restoredItemGeoms` in-memory backup 을 schema level 영속
- **선행 조건**: ADR-0018 D11 amend (`ItemCommon.restored_geom?`)

## 6. 주의사항 / Gotchas

- **Theme hot-reload root cause + fix (사용자 final)**: XtermHost mount `$effect` 가 `themeStore.resolved` 를 read 하면 reactive dependency → theme 변경 시 재 mount → buffer 손실. `untrack(() => themeStore.resolved)` 으로 *값만 read* + 별 G27 hot-reload effect 가 theme swap 담당. **새로운 store/derived 를 XtermHost mount effect 안에서 read 시 동일 패턴 적용 필요**.
- **거부된 theme fix 접근법** (반복 회귀 위험):
  - `clearTextureAtlas() + refresh(0, rows-1)` 만 (사용자 보고 stale 그대로)
  - `.xterm` root 의 display 토글 reflow
  - `.xterm-rows > span` 의 inline color reset
  - `PanelNode 의 {#key themeStore.resolved}` (XtermHost remount 트리거 — BE replay 안 옴 + reactive cascade 폭주)
  - `+page.svelte $effect 의 silentReattach` (sessionStore.reattachInProgress reactive dependency 가 cascade → 무한 loop)
  - `SettingsOverlay.setMode 의 window.location.reload()` (modal 자체도 unmount, ADR-0017 D2 Auto-save 정책 위반)
- **TS generic 의 svelte parser 오인**: `querySelectorAll<HTMLElement>(...)` / `as NodeListOf<HTMLElement>` / `as HTMLElement` 모두 svelte parser 가 `<HTMLElement>` 를 HTML tag 로 오인 → `<script>` left open 오류. **comment 안 raw text `<HTMLElement>` 도 동일**. `instanceof HTMLElement` 패턴 + 꺾쇠 자체 회피 필요.
- **ADR-0017 amend ④ D2 Auto-save 정책**: SettingsOverlay 안 control 의 change 가 *즉시 persist + modal 유지*. setMode 안에 reload / close trigger 추가 금지.
- **ColorPicker popover 위치**: `position: fixed` + `.right-panel` left edge anchor + viewport clamp. parent overflow 와 무관. RightPanel 미존재 시 trigger.left fallback.
- **ColorPicker output schema**: 모든 format mode (HEX/RGB/HSL/OKLCH) 의 output 은 *항상 hex string* — schema (ADR-0018 D3) 정합. format 은 *display + input* 만, *저장 format 아님*.
- **Recent swatch localStorage**: key `gtmux:colorpicker:recent`. 6/8-digit hex 만 validate. private/incognito quota → silent in-memory fallback.
- **Token-aware Document palette**: 10 semantic tokens (ADR-0016 D10) — `resolveCssColor()` 로 theme 별 자동 변환. light/dark 전환 시 reactive.
- **InspectorField + ColorPicker inline box 디자인 통일**: 옛 시안 v2 의 `.input` (h28 / bg-2 / transparent border) supersede. **새 inspector control 추가 시 ColorPicker inline 패턴 따라야** (h22 + bg-bg + 1px visible border + hover-strong + focus-accent + `.k` prefix label).
- **`.panel-resize-handle z-index 10 !important`**: xterm-viewport 의 stacking 위로. **다른 canvas item (NoteNode 등) 의 NodeResizer 도 동일 필요 시 z-index 명시**.
- **Tool guide z-index = `var(--z-canvas-overlay)` (18)** — side-panel (20) 보다 아래. ghost 가 LeftPanel/RightPanel 위로 노출 회피.
- **Toolbar button click 후 blur**: `onclick={(e) => { ...; (e.currentTarget as HTMLButtonElement).blur(); }}` — focus retention 차단 (ESC 후 outline 잔류). Tab navigation focus-visible 은 그대로.
- **AuthDialog choice button focus**: `:focus-visible` outline 제거, border-color accent 만 인디케이터.
- **escRouter eager attach**: module load 시 `escRouter.attach()` 자동 호출. **신규 escRouter 사용처는 register 만 — listener attach 신경 X**.
- **Cursor mode**:
  - rect/ellipse/line/free_draw → `.drag-cursor` (crosshair)
  - text → `.text-cursor` (I-beam)
  - hand / space-held → `.pan-cursor` (grab)
  - 다른 tool → default (terminal/note/image/etc.)
- **Inline edit `plain` mode**: canvas item 의 더블 클릭 편집 시 wrapper border 제거. Panel label / Note title+body / FilePath / Document content / MaximizedItemModal note 모두 `plain={true}`. LayerTreeView 의 group rename 은 chrome 영역 — 그대로.
- **본 session 와 동일 날짜 다른 worker handover**: `2026-05-17-session-handover-canvas-tools-and-file-picker.md` 가 *untracked* — 본 handover 와 *별도 영역*. 다른 worker (canvas tools / file picker) 의 작업 이력. 다음 session 진입 시 모두 cross-reference.

## 7. 새 session 시작 방법

이 문서를 받은 session 은 다음 순서로 부트스트랩한다:

1. 이 handover 문서 (`docs/reports/2026-05-17-session-handover-colorpicker-and-design-unification.md`) 를 끝까지 읽는다.
2. §3 의 `CLAUDE.md` + `docs/sketch.md` 를 읽는다 (언어 컨벤션 + invariants + applyMutation 단일 entry).
3. §3 의 **본 session 직전 handover** (같은 날 `2026-05-17-session-handover-canvas-tools-and-file-picker.md` / `…component-design-batch.md` / `…maximize-modal-and-ui-batch.md`) 를 빠르게 훑는다 — 시간순 *직전* 의 context.
4. **§3 의 신규 ADR 0030/0031/0032 + 0062 investigation** 을 읽는다 — 본 session 의 결정 정본.
5. §4 진행 중 작업 / §5 향후 작업 중 우선순위 결정:
   - **§4.1 권장 진행**: `docs/reports/0063-xterm-theme-buffer-preservation-resolution.md` 작성 (root cause + 사용자 fix 의 정합 기록)
   - 또는 **§4.2 dead code 정리** (Toolbar.svelte / ThemeToggle.svelte) — `chore` 1 commit
   - 또는 **§5.1 ADR-0030/0031/0032 implementation** (handover 0057 의 phase 1 부터)
6. **handover 작성 이후 변경 확인**: `git log --oneline e006962..HEAD` — 본 session 종료 후 다른 worker 의 추가 commit 가능성.

만약 §5 의 항목 모두 우선순위 낮다면, §4.2 의 dead code 정리 또는 §5.5 의 brand-icon 최적화 같은 정리 작업 진행.

---

_생성: `session-handover` skill v1_
