# Resolution — theme 변경 시 xterm contents blank 문제 해소 (2026-05-17)

> 상태: **해결됨**  
> 선행 조사: [`0062-theme-hot-reload-investigation.md`](./0062-theme-hot-reload-investigation.md)  
> 핵심 결론: 서버의 terminal output replay 누락이 아니라, `XtermHost.svelte`의 mount `$effect`가 `themeStore.resolved`를 reactive dependency로 추적하면서 theme 변경 때 xterm 인스턴스를 dispose/recreate한 frontend lifecycle 버그였다.

## 1. 사용자 현상

Settings에서 theme(light/dark/system)를 변경하면 terminal panel의 이전 command/log 영역이 빈 화면처럼 보였다. 새로고침하면 같은 terminal contents가 다시 표시됐다.

이 현상은 다음 사용자 흐름에서만 표면화됐다.

1. Web page가 이미 session에 attach되어 있고 terminal panel이 mount됨.
2. `Settings > Theme`에서 theme을 변경.
3. Chrome UI는 즉시 새 theme으로 바뀜.
4. Terminal panel의 기존 contents가 비어 보임.
5. Page refresh 후에는 contents가 다시 보임.

## 2. 최종 원인

### 2.1 잘못된 1차 가설

초기 조사에서는 xterm v6 DOM renderer가 이미 그려진 cell의 inline color를 새 theme으로 repaint하지 못하는 문제로 의심했다. 이 가설 때문에 다음 우회가 검토됐다.

- `term.refresh(...)`
- xterm DOM reflow 강제
- cell span inline style reset
- xterm remount
- websocket reattach 또는 page reload

하지만 실제 해결 과정에서 핵심은 xterm repaint 자체가 아니라 **Svelte 5 effect dependency**였음이 확인됐다.

### 2.2 실제 원인

[`XtermHost.svelte`](../../codebase/frontend/src/lib/canvas/XtermHost.svelte)의 mount `$effect`는 xterm 인스턴스를 생성하고, cleanup에서 `term.dispose()`를 호출한다.

문제는 mount effect 내부의 `new Terminal({ theme: xtermTheme(themeStore.resolved) })`가 `themeStore.resolved`를 직접 읽고 있었다는 점이다. Svelte 5에서 `$effect` 내부 reactive read는 해당 effect의 dependency가 된다. 따라서 theme 변경 시 mount effect가 재실행되고, 다음 순서가 발생했다.

1. 기존 `Terminal` 인스턴스 cleanup.
2. 기존 xterm 내부 buffer 및 DOM dispose.
3. 새 `Terminal` 인스턴스 생성.
4. 새 인스턴스는 client-side terminal buffer가 비어 있음.
5. 기존 WS connection은 유지되므로 backend handshake catch-up replay가 다시 발생하지 않음.

결과적으로 contents가 “색만 안 보이는 것”처럼 보였지만, 실제로는 **theme 변경이 xterm 인스턴스 재생성을 유발하여 client-side terminal buffer를 날린 것**이었다.

## 3. 왜 새로고침하면 복구됐는가

새로고침은 단순 DOM repaint가 아니라 WS connection을 새로 만든다. backend는 WS handshake 직후 catch-up 단계에서 live pane의 ring buffer를 `PANE_OUT`으로 replay한다.

관련 코드:

- [`ws-server/src/lib.rs`](../../codebase/backend/crates/ws-server/src/lib.rs) — terminal UUID binding을 먼저 replay하고, 이후 live pane별 `PANE_OUT` replay를 수행한다.
- [`pty-backend/src/lib.rs`](../../codebase/backend/crates/pty-backend/src/lib.rs) — `subscribe_output()`이 broadcast receiver와 ring-buffer snapshot을 함께 반환한다.
- [`dispatcher.svelte.ts`](../../codebase/frontend/src/lib/ws/dispatcher.svelte.ts) — frontend late buffer는 handler 등록 전 도착한 `PANE_OUT`만 보관한다. 이미 연결된 상태에서 xterm만 remount된 경우의 historical replay를 담당하지 않는다.

즉 새로고침은 server data가 정상임을 증명했다. 반대로 theme 변경만으로는 WS reconnect가 없으므로 server-side replay가 발생하지 않는 것이 정상이다.

## 4. 수정 접근

수정 원칙은 다음과 같다.

1. xterm mount lifecycle은 pane/container identity에만 묶는다.
2. theme 변경은 기존 xterm 인스턴스의 `options.theme`만 갱신한다.
3. `SettingsOverlay`의 auto-save UX는 유지하고, theme 변경 때문에 modal/page를 reload하지 않는다.

구현:

- [`XtermHost.svelte`](../../codebase/frontend/src/lib/canvas/XtermHost.svelte)
  - `untrack(() => themeStore.resolved)`로 mount effect의 초기 theme read를 dependency에서 제외.
  - 별도 theme `$effect`에서 live `termRef`에 대해 `term.options.theme = { ...xtermTheme(resolved) }` 적용.
  - xterm v6 option setter가 object reference 비교를 하므로 매번 새 theme object를 전달.
  - 적용 후 `term.refresh(0, Math.max(0, term.rows - 1))`로 현재 viewport repaint를 요청.
- [`SettingsOverlay.svelte`](../../codebase/frontend/src/lib/chrome/SettingsOverlay.svelte)
  - theme 변경 시 `themeStore.setMode(mode)`만 수행.
  - 이전의 reload 안내 문구를 제거하고 “Changes apply immediately” 흐름으로 정리.

## 5. 설계 정합

### ADR-0004 — xterm.js v6 DOM renderer

[`ADR-0004`](../adr/0004-terminal-rendering.md)는 terminal rendering을 `@xterm/xterm` v6 DOM renderer로 잠그고, `XtermHost.svelte`를 해당 결정의 reverse-reference로 둔다. 이번 수정은 이 결정과 정합한다.

- xterm instance는 terminal byte stream의 view다.
- `PANE_OUT`은 `terminal.write(Uint8Array)`로 소비한다.
- theme 변경은 web chrome concern이므로 terminal process나 backend state를 건드리지 않는다.

### ADR-0012 — Svelte 5 + runes

[`ADR-0012`](../adr/0012-frontend-stack-svelte.md)는 Svelte 5 runes/store 기반 frontend stack을 결정한다. 이번 문제는 해당 stack에서 `$effect` dependency boundary를 잘못 잡았을 때 발생하는 대표적인 lifecycle 회귀다.

이번 수정은 mount effect와 theme effect를 분리하여 다음 경계를 명확히 했다.

- mount effect: DOM container, pane id, WS handler registration lifecycle
- theme effect: 이미 살아 있는 xterm instance의 visual option 갱신

### ADR-0017 amend ④ — Settings auto-save UX

Settings theme 변경은 auto-save이며 modal을 유지해야 한다. Page reload 또는 silent reattach로 해결하면 terminal contents는 복구될 수 있지만 Settings UX를 깨고, session/web 연결 상태까지 불필요하게 흔든다.

이번 수정은 `SettingsOverlay.svelte`의 `setMode()`를 그대로 auto-save 단일 동작으로 유지한다.

### ADR-0021 — Terminal pool + multi-session mirror

[`ADR-0021`](../adr/0021-terminal-pool-and-mirror.md)는 한 terminal stream을 여러 panel/xterm instance가 mirror할 수 있음을 전제로 한다. 이번 문제는 mirror 모델 자체의 문제가 아니라, 한 xterm instance가 theme 변경으로 불필요하게 subscriber unregister/register + dispose/recreate된 문제였다.

따라서 해결도 terminal pool이나 backend broadcast policy를 바꾸지 않고, frontend xterm instance lifecycle만 바로잡았다.

### ADR-0025 — Session-scoped pane output filter

[`ADR-0025`](../adr/0025-session-scoped-pane-output-filter.md)는 WS catch-up replay와 live filtering의 경계를 정의한다. 새로고침 시 replay가 정상 복구되는 이유는 handshake catch-up이 filter bypass로 동작하기 때문이다. 이번 수정은 그 replay 경로에 의존하지 않고, 정상 사용 중에는 xterm buffer를 보존한다.

## 6. 코드 링크

주요 수정 지점:

- [`codebase/frontend/src/lib/canvas/XtermHost.svelte`](../../codebase/frontend/src/lib/canvas/XtermHost.svelte)
  - mount effect: `new Terminal(...)`, `registerPaneOut(...)`, cleanup `term.dispose()`
  - 초기 theme read: `untrack(() => themeStore.resolved)`
  - live theme effect: `term.options.theme = { ...xtermTheme(resolved) }`, `term.refresh(...)`
- [`codebase/frontend/src/lib/chrome/SettingsOverlay.svelte`](../../codebase/frontend/src/lib/chrome/SettingsOverlay.svelte)
  - `setMode(mode)`는 `themeStore.setMode(mode)`만 수행
  - theme section 안내 문구를 live apply UX로 정리

관련 동작 근거:

- [`codebase/frontend/src/lib/ws/dispatcher.svelte.ts`](../../codebase/frontend/src/lib/ws/dispatcher.svelte.ts)
  - `registerPaneOut()`의 frontend late buffer는 handler 미등록 중 도착한 bytes만 flush한다.
  - 이미 연결된 WS에서 xterm만 재생성되는 경우 historical replay를 담당하지 않는다.
- [`codebase/backend/crates/ws-server/src/lib.rs`](../../codebase/backend/crates/ws-server/src/lib.rs)
  - WS handshake catch-up에서 `TERMINAL_SPAWNED` binding 후 live pane별 ring buffer를 `PANE_OUT`으로 replay한다.
- [`codebase/backend/crates/pty-backend/src/lib.rs`](../../codebase/backend/crates/pty-backend/src/lib.rs)
  - `subscribe_output()`이 broadcast receiver와 ring snapshot을 race-free로 반환한다.

## 7. 검증

정적 검증:

- `pnpm --dir codebase/frontend check` — 통과
- `pnpm --dir codebase/frontend build` — 통과

사용자 검증:

- 사용자가 theme 변경 후 terminal contents blank 문제가 해소됐다고 확인했다.

제약:

- 이 세션에서는 로컬 server/browser runtime 검증은 직접 수행하지 못했다. 이전 server 실행 escalation이 거절되어, 최종 runtime 확인은 사용자 검증으로 대체했다.

## 8. 재발 방지 기준

향후 `XtermHost.svelte` 또는 terminal-like imperative widget을 수정할 때는 다음 기준을 적용한다.

1. `$effect` 안에서 store를 읽으면 lifecycle dependency가 되는지 먼저 확인한다.
2. mount/dispose effect에는 widget identity를 바꾸는 입력만 포함한다.
3. theme, font, visual option, focus style 같은 presentation state는 별도 effect에서 live instance에 적용한다.
4. xterm buffer 보존이 필요한 변경은 `term.dispose()` 또는 component remount를 유발하지 않는다.
5. backend ring replay는 WS attach/reconnect catch-up용이지, frontend widget remount 보정용으로 사용하지 않는다.

## 9. 선행 보고서 정정

[`0062-theme-hot-reload-investigation.md`](./0062-theme-hot-reload-investigation.md)의 “xterm v6 DOM renderer의 cell stale 색” 가설은 최종 원인이 아니었다. 실제 root cause는 Svelte effect dependency로 인한 xterm remount였다. 다만 0062의 실패 시도 기록은 유효한 배제 근거로 보존한다.
