# Investigation — xterm v6 theme hot-reload 의 cell stale 색 회귀 (2026-05-17)

> 상태: **미해결** — 모든 in-place fix 시도 실패. *page manual reload* 가 유일 작동 path.
> 후속: ADR-0017 amend ④ 의 D5 "xterm theme hot reload — 다음 amend" 영역 — 본 issue 의 *근본 원인* 으로 그 amend 의 input.
> 정정(2026-05-17): 최종 원인은 xterm cell stale 색이 아니라 `XtermHost.svelte` mount `$effect` 의 `themeStore.resolved` reactive dependency 로 인한 xterm remount/buffer loss 였다. 해결 보고서는 [`0063-xterm-theme-buffer-preservation-resolution.md`](./0063-xterm-theme-buffer-preservation-resolution.md)를 참조.

## 1. 현상

- 사용자가 *theme 변경* (light ↔ dark, Settings overlay 또는 옛 Titlebar `ThemeToggle`) 시:
  - **chrome** (token swap, `<html>.classList`) 은 즉시 새 theme 으로 반영 (titlebar / panel border / button 색 등) — OK.
  - **terminal panel** 의 cell 색만 *stale*. 글자 + bg 가 *옛 theme 색* 그대로 잔존 또는 *fg=bg 같은 색* 으로 *글자 안 보임* (빈 영역).
- **새로고침** (`Cmd+R` / page reload) 시에만 정상 복구.
- 사용자 진단 보조 단서:
  - DOM 측면에서 `class=xterm-viewport` 가 *상위 container 의 style 을 침범* (관찰).
  - 새로고침 후엔 `[ws] registerPaneOut pane=N (no buffered bytes)` log 안 찍힘 — *새 connection 의 ring buffer replay* 가 정상 시 *buffered bytes 가 있어야* 함을 시사.

## 2. 추정 원인 (확정 안 됨)

xterm v6 의 *DOM renderer* (default) 가 cell 의 inline color (`<span style="color: rgb(...); background-color: rgb(...)">`) 를 *cell write 시점에 fix*. `term.options.theme = newTheme` 은 *theme 객체만* swap — *이미 그려진 cell* 의 inline color 는 갱신 안 됨.

`term.refresh(0, rows-1)` 는 cell 의 fg/bg 재 paint 호출이지만 — v6 의 *cached span fragment* 가 *recycle* 안 됨 (또는 *atlas cache* 만 refresh, *DOM span 자체* 의 inline 색 stale).

`!important` background override (XtermHost.svelte 의 `.xterm-host :global(.xterm-viewport)`) 도 *cell span 의 inline 색* 보다 selector specificity 낮음.

## 3. 시도한 fix 와 결과

| 시도 | Commit | 효과 |
|---|---|---|
| **A. `clearTextureAtlas() + refresh(0, rows-1)`** | `95b10f9` | × stale 그대로 |
| **B. A + `.xterm` root display 토글 reflow** | `569dab4` | × stale 그대로 |
| **C. cell span 의 inline `style.color` / `backgroundColor` reset + refresh** | `16945d1` → `7b400c3` | × stale 그대로 |
| **D. `PanelNode` 의 `{#key themeStore.resolved}` — XtermHost 강제 remount** | `092c8e9` | △ XtermHost 는 remount, 단 BE 가 *replay 안 보냄* → cell 빈 채. + reactive cascade 로 *log 빠르게 찍힘* 회귀 |
| **E. `+page.svelte` $effect — silentReattach on theme change** | `6b02f65` | × 무한 loop (sessionStore.reattachInProgress 같은 reactive read 가 effect dependency 가 되어 cascade) — 즉시 revert (`40966ab`) |
| **F. SettingsOverlay setMode 의 auto reload** | `e195288` | △ chrome / xterm 둘 다 fix — 단 modal 자체도 unmount (ADR-0017 D2 Auto-save 정책 위반) — revert (`1213d73`) |
| **G. ThemeToggle Titlebar 제거 + Settings setMode 의 reload 없음** | `cf4a5ae` / `1213d73` | × chrome 만 반영, xterm cell stale 그대로 |

→ **모든 in-place path 실패**. 현 상태 = G (변경 가능 entry 만 Settings, reload 없음).

## 4. 진단 보조 정보

- `[ws] registerPaneOut pane=N (no buffered bytes)`: dispatcher 의 register 시점 *cached PANE_OUT 0*. 새로고침 시엔 안 찍힘 → *새 WS connection 의 ring buffer replay* 가 BE 에서 흘러옴.
- `silentReattach` 의 동작 (`sessionStore.svelte.ts:525~`) — *attach API + WS 의 layout reload* — 단 *cell 의 history replay* 까지는 trigger 안 됨 가능성 (ADR-0021 D8 의 mirror 패턴 + reattach 의 정확한 chain 의 상세 분석 미완).
- xterm v6 의 *DOM renderer* default 가 *canvas renderer 대신* 사용되는지 확인 필요 (`SECURE_XTERM_OPTIONS` 의 renderer 옵션 없음 — v6 default = dom).

## 5. 권장 follow-up (별 plan / ADR)

### A. xterm.js 의 official theme hot-reload pattern
- xterm github 의 *theme hot swap* issue 검토. v6 의 알려진 workaround / fix PR.
- 옵션: renderer 명시 reset (예: `term.options.windowsMode = ...` 같은 unrelated option swap 후 refresh trick).

### B. `@xterm/addon-serialize` 도입
- ADR-0004 D3 의 "addon 미채택" 정책 amend 필요.
- buffer dump → `term.reset()` → 재 write — cell 들 새 theme 으로 paint.
- 단 cell 의 *scrollback 위치 보존* + *cursor state* 정확.

### C. WS connection 명시 재시작
- `wsClient.stop() + wsClient.start()` 또는 BE 의 *replay request* endpoint 신설.
- BE 의 attach handler chain 정합 검증 (silentReattach 와 별도).

### D. 사용자 경험 만 일단 정리 (현재 path)
- *Theme 변경 = drone event*. Titlebar 의 toggle 제거 (완료).
- Settings overlay 안 theme section + manual reload 안내 hint (완료).
- 사용자가 stale 색 보면 *Cmd+R* — *acceptable* 단 *근본 X*.

## 6. ADR 정합

- **ADR-0017 amend ④ D2** (Auto-save 정책: change 즉시 persist + modal 유지) — 현 코드 정합. F 시도의 reload 가 위반.
- **ADR-0017 amend ④ D5** (xterm theme hot reload는 다음 amend) — 본 issue 가 정확히 그 amend 영역. 본 report 가 *그 amend 의 input* 역할.

## 7. 현 코드 상태 (`1213d73` 기준)

- `Titlebar.svelte`: ThemeToggle import + mount + doc 라인 모두 제거. titlebar-right 빈 슬롯.
- `SettingsOverlay.svelte`: `setMode()` = `themeStore.setMode(mode)` 만. section-hint 에 "If terminal cell colors look stale after switching, reload the page" 안내.
- `XtermHost.svelte` G27 effect = `term.options.theme = xtermTheme(resolved)` 1줄 (in-place hack 모두 제거).
- `PanelNode.svelte` = `{#key}` 없음, themeStore 의존성 없음.
- 기능적 결과: chrome 색 즉시 반영, terminal cell 색 새로고침 전까지 stale.

## 8. Dead code (별 정리 candidate)

본 investigation 중 발견 — *theme issue 와 무관* 한 dead code:
- `codebase/frontend/src/lib/toolbar/Toolbar.svelte` (옛 toolbar — `Toolbar2` 가 active, Toolbar 어디서도 import 없음)
- `codebase/frontend/src/lib/ui/ThemeToggle.svelte` (Titlebar 제거 후 Toolbar 외 사용처 없음 — Toolbar 도 dead)

→ 별 commit 으로 정리 권장.
