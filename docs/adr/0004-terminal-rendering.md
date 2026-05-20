# ADR-0004: 터미널 렌더링 — xterm.js v6 (DOM) + fit + unicode11 + placeholder-on-zoom

- 상태: Accepted (2026-05-14)
- 일자: 2026-05-13 (Proposed, R2 결론에 따라 draft) / 2026-05-14 (Accepted, R8 §F8 zoom 정책 inline + ADR-0012 D4 호환성 제약 reverse-lock)
- 결정자: frontend-architect
- 근거 보고서: `docs/reports/0002-terminal-rendering.md` (R2 — 라이브러리·렌더러·addon·옵션), `docs/reports/0008-frontend-stack.md` §F8 (R8 — zoom-blur 정책 (b) 확정)
- 관련 ADR: ADR-0012 D4 (Svelte 5 stack 안에서 본 ADR을 *호환성 제약*으로만 언급 — 본 ADR-0004가 정식 잠금), ADR-0002 (WS envelope — PANE_OUT 0x02·PANE_IN 0x03·PANE_RESIZE 0x04 정의), ADR-0003 D9 (xterm 보안 옵션 셋), ADR-0005 (캔버스 라이브러리 — viewport zoom 소비자)
- 관련 SSoT: `docs/ssot/wire-protocol.md` §2.1 (0x02 PANE_OUT / 0x03 PANE_IN / 0x04 PANE_RESIZE 페이로드 규격)
- 관련 코드: `codebase/frontend/src/lib/canvas/XtermHost.svelte` (본 ADR 결정의 reverse-reference), `codebase/frontend/src/lib/xterm/options.ts` (ADR-0003 D9 SECURE_XTERM_OPTIONS 정본)

## 맥락

본 ADR은 gtmux 프런트엔드가 *한 Pane의 ANSI/UTF-8 출력*을 캔버스 위 Panel 안에서 어떻게 시각화·입력 라우팅할지 — 즉 **터미널 widget 라이브러리·렌더 백엔드·필수 addon·zoom 거동**을 결정한다. ADR-0012 D4는 본 결정을 *호환성 제약*("xterm.js DOM widget"으로 캔버스 lib 후보를 필터링)으로만 사용했고 실제 라이브러리·렌더러·addon은 본 ADR이 책임진다.

도메인 입력:
- `docs/sketch.md` §10.2: 프런트엔드 구성표에 "pane 터미널 렌더(xterm.js 등)"가 명시.
- `docs/sketch.md` §11.2.B + R2 §F4: PANE_OUT 소비 경로 = `terminal.write(Uint8Array)` 단일 파이프. base64/UTF-8 디코드 우회.
- `docs/sketch.md` §13.3.4: OSC 시퀀스(특히 OSC 52 클립보드, OSC 8 비-http) 차단을 *기본값*으로 강제.
- `CONTEXT.md` "Panel Streaming State" + "Input Target (I)" + "tmux Layout ≠ Canvas Layout": Panel은 *Pane의 시각 객체*이며 한 Panel은 한 Pane을 1:1 widget으로 표현, layout 문자열은 widget 입력에 도달하지 않는다.

R2(2026-05-13)는 후보 라이브러리·렌더러·addon·옵션 셋·메모리 추정·alt-screen 거동·suspended 동작을 *Decision-grade*로 산출했다. R8(2026-05-13) §F8은 ADR-0012 O1로 식별되었던 **xterm.js × Svelte Flow `transform: scale()` zoom blur** 위험을 (a) counter-scale font-size / (b) placeholder on zoom 두 후보로 비교해 (b)를 채택. 본 ADR은 R2의 핵심 결정 + R8 §F8을 한 결정 본문에 통합한다.

## 결정 (Decisions)

- **D1.** 터미널 widget 라이브러리 = **`@xterm/xterm` v6.x** (실측 `package.json` 록 = `^6.0.0`). VS Code/Hyper/Theia/code-server가 공유하는 사실상 표준이며, v6에서 ESM 정식·Shadow DOM·DEC 2026 synchronized output을 갖춤. (R2 §F1·§F2)
- **D2.** 렌더 백엔드 = **DOM 단독** (MVP). `@xterm/addon-canvas`는 v6에서 제거됐고 `@xterm/addon-webgl`은 Chrome/Safari의 origin당 WebGL 컨텍스트 16개 한계로 50-pane 전수 적용이 *원천적으로 불가능*하므로 거절. WebGL의 "focus pane 한정 옵트인" 정책은 P1+ 별도 결정(O 영역). (R2 §F3, R10)
- **D3.** 필수 addon = **`@xterm/addon-fit` 0.11.x** + **`@xterm/addon-unicode11` 0.9.x** 두 개만 MVP P0. fit은 Panel 가시 영역 → `cols/rows` 계산, unicode11은 `term.unicode.activeVersion = "11"` 강제로 CJK·이모지 폭 정합. 그 외 addon(web-links·search·serialize·clipboard·image·attach·ligatures·progress·webgl)은 MVP 비채택. (R2 §F9)
- **D4.** 입출력 경로 — **양방향 모두 WS envelope 직결, JSON·UTF-8 디코드 우회**.
  - 출력: WS `PANE_OUT (0x02)` payload(ANSI/UTF-8 raw bytes, SSoT §2.1) → 디스패처가 `terminal.write(Uint8Array, cb)` 직접 호출. `cb`는 R2 §F4의 백프레셔 watermark 갱신 훅(2차 방어선 — 1차는 백엔드 tmux `pause-after`, ADR-0001).
  - 입력: `term.onData(handler)` → handler가 WS `PANE_IN (0x03)`로 전송. `paneId`는 SSoT §2.1 정의대로 *현재 I (Input Target)*. PANE_IN 송신 와이어링은 Sprint 4-C FE-1/FE-2 task에서 적용 예정.
- **D5.** Resize 경로 = 컨테이너 `ResizeObserver` → **150ms 디바운스** → `fitAddon.fit()` 1회 → 변경된 `(cols, rows)`를 WS `PANE_RESIZE (0x04)`로 송신. 150ms 디바운스 값은 R2 §F8/§R17(xterm.js Issue #4113) 권고. 백엔드는 SSoT §2.1 정의에 따라 `resize-window -x <cols> -y <rows>`로 전달(single-pane-per-window 컨벤션, ADR-0008).
- **D6.** **Zoom-blur 정책 = (b) Placeholder on zoom (ε = 0.02).** (R8 §F8) Svelte Flow viewport의 `transform: translate(x,y) scale(zoom)`이 Panel 안 xterm DOM 셀을 sub-pixel scale시켜 blur를 유발하므로:
  - `|viewport.zoom - 1| < 0.02` (= unit-zoom 구간): xterm DOM 가시 + write 진행.
  - 그 외: xterm DOM 비가시 + Panel-level placeholder(라벨 + 색) 렌더. 데이터 흐름(WS PANE_OUT 수신·write 호출)은 *유지*. zoom 복귀 시 `fitAddon.fit()` 1회 호출 후 가시화.
  - MVP 구현 패턴은 `{#if isAtUnitZoom}<XtermHost />{:else}<Placeholder />{/if}` (R8 §F8 sketch); 단 *instance retain*(`display:none`) 패턴으로의 전환 트리거는 O2.
  - 본 정책은 D16 Panel Streaming State와 **직교**(visibility=hidden/minimized는 별개 차원; zoom 액션은 visibility를 건드리지 않는다).
  - 거절 (a) counter-scale font-size + `fit()`: 매 zoom step에서 xterm v6 DOM cell-metric 캐시 redo + 50 pane × frame jank, 그리고 "줌 아웃 시 글자만 그대로"라는 무한-캔버스 멘탈모델 파괴.
- **D7.** OSC 시퀀스·security option flag 정책은 ADR-0003 D9에 위임 (`SECURE_XTERM_OPTIONS` 정본, `codebase/frontend/src/lib/xterm/options.ts`). 본 ADR은 *위임 사실*과 *adoption 의무*만 결정 — 새 `Terminal(...)` 호출은 반드시 `SECURE_XTERM_OPTIONS`로 시작해야 하며 ad-hoc 옵션 직접 작성 금지.

## 거절된 대안 (Rejected)

- **R1. HyperTerm (`@hyperterm/...`)** — Electron 종속, 브라우저 DOM widget으로 호스팅 불가. Svelte mount 경로 부재. (R2 §F1)
- **R2. react-terminal류(`react-console-emulator`, `xterm-for-react` 등)** — React 종속. ADR-0012 R1(React 거절)과 동일 이유 — Svelte 5 stack 안에서 React 트리 mount는 가산 가치 없이 도메인 외 인지 부하만 가산.
- **R3. Custom canvas/WebGL renderer 자체 구현** — ANSI ECMA-48 + SGR + OSC + DEC + Unicode 11 폭 + alt-screen + bracketed paste + 마우스 + DECSCUSR을 전부 자체 구현하는 비용이 12개월 일정 안에 불가능. wheel 재발명. (R2 §F1)
- **R4. node-pty + 브라우저 내 native terminal subprocess** — 브라우저 환경에서 비현실(Node API 부재). 백엔드 PTY는 *tmux daemon 영역*(ADR-0009)이며, 브라우저는 *시각화 widget*만 책임진다 — 두 역할의 혼동.
- **R5. `@xterm/addon-canvas`** — v6.0.0에서 패키지 제거됨. 옵션 아님. (R2 §F2, R7)
- **R6. `@xterm/addon-webgl` 50-pane 전수 적용** — Chrome/Safari origin당 16 WebGL context 한계, 17번째 컨텍스트가 가장 오래된 컨텍스트를 *손실*. 50 pane × WebGL은 *원천적으로 불가*. P1+ "focus pane 한정 옵트인"으로만 재방문 가능(O 영역). (R2 §F3, R10)
- **R7. `@xterm/addon-attach`** — WS attach helper지만 *단일 WS 1:1 가정*. gtmux는 ADR-0002 binary envelope multiplex(0x01–0x84) 위에서 동작하므로 부적합 — `WSClient` 디스패처가 자체 라우팅. (R2 §F9)
- **R8. `@xterm/addon-clipboard` (OSC 52 활성)** — `docs/sketch.md` §13.3.4 + ADR-0003 D9·R(rej)5 보안 정합 위반. MVP 비채택. (R2 §F6·§F9)
- **R9. `@xterm/addon-serialize`로 D15 ring buffer 대체** — D15 ring buffer는 *서버 책임*(ADR-0001 D7 후속). 클라이언트 측 직렬화는 위치 부적절. (R2 §F9)
- **R10. `@xterm/addon-image` (sixel/iTerm2 inline image)** — 보안 표면 확장 + 사용성 가산 작음 + MVP 비범위. P2 후보로 기록만. (R2 §F9)
- **R11. scrollback=1000 (xterm 기본값) 그대로 사용** — 50 pane × 1000 scrollback이 D19 frontend tab memory < 100 MB 예산을 초과할 수 있음. ADR-0003 D9의 SECURE_XTERM_OPTIONS에 *scrollback=500*으로 잠금됨 (위임). (R2 §F10)

## 결과 (Consequences)

- 긍정:
  - ANSI(SGR true color, OSC 8 hyperlink with linkHandler 제약, DEC 1049 alt-screen, DEC 2004 bracketed paste, DEC 2026 synchronized output, DECSCUSR, X10/SGR/utf8 mouse) + Unicode 11 폭 처리가 *라이브러리 기본값*으로 완비.
  - Svelte 5 wrapper(`XtermHost.svelte`)가 단순 — xterm은 DOM widget이라 lifecycle을 `$effect` 마운트 + 클린업 함수 반환으로 처리, `onDestroy`/`onMount` 별도 import 불필요.
  - PANE_OUT 0x02가 raw Uint8Array로 도착 → `term.write` 직결 → *zero-copy 파이프라인*(JSON parse·UTF-8 decode·base64 변환 모두 없음). p50 < 30ms / p99 < 100ms (D19) 예산 안.
  - Placeholder-on-zoom이 infinite-canvas 멘탈모델(Figma/Miro 패턴)과 정합 — zoom out = 한눈에 보는 모드, zoom 1 = 디테일 모드의 명시 분기.
- 부정/비용:
  - xterm v6.x DOM 렌더가 CSS `transform: scale()` 환경에서 sub-pixel blur 발생 → D6 placeholder 정책으로 회피하나 zoom 전이 순간 *재마운트 비용*이 50 pane × frequent zoom 시 frame jank 유발 가능 → O2에서 measure → 필요 시 `display:none` retain 패턴으로 전환.
  - addon 버전 lockstep 필요: **xterm v6.x ↔ addon-fit 0.11.x ↔ addon-unicode11 0.9.x** (실측 `package.json`: `^6.0.0` / `^0.11.0` / `^0.9.0`). 메이저 bump 시 동시 갱신, npm `peerDependencies` 경고 모니터.
  - WebGL 렌더러 거절로 GPU 가속 부재 — DOM 렌더러 단독은 v5.3+에서 충분히 빠르나(R2 §F3) burst 시 단일 rAF 내 처리 한계가 50 × 1 KB ≈ 15 ms ≈ 1 frame. 50 pane 동시 burst는 *간헐적 2-frame 분할* 가능. D19 p99 < 100ms 안.
  - 입력 송신(PANE_IN)·resize 송신(PANE_RESIZE)은 본 ADR 결정대로지만 *실 송신 wiring*은 `XtermHost.svelte`가 현재 `console.debug` stub 상태 — Sprint 4-C FE-1/FE-2 task에서 적용 예정.
- 후속:
  - **Sprint 4-C FE-1·FE-2** wiring task: D4 입력 경로(`term.onData` → `sendPaneInput(paneId, bytes)`) + D5 resize 경로(`fitAddon.fit()` 결과 → `sendPaneResize(paneId, cols, rows)`) 실 송신 함수와 배선. 현 `XtermHost.svelte`의 stub 라인 둘을 교체.
  - **ADR-0005 (캔버스 라이브러리)**의 Svelte Flow viewport store가 본 ADR D6의 `isAtUnitZoom` 입력. `viewport.zoom` shape이 R8 §F8 sketch와 일치하도록 reverse-reference 명시.
  - **R8 §O3** (zoom placeholder remount 비용 측정) 결과에 따라 D6 구현 패턴을 `{#if}` → retain(`display:none`)으로 전환 가능 — 본 ADR 갱신 없이 구현 패턴 차원의 변경.
  - **ADR-0003 D9**(SECURE_XTERM_OPTIONS) 변경 시 본 ADR D7 위임 본문 검토 — security 옵션이 옮겨오거나 새 옵션이 도입되면 D7 인용 갱신.

## 불변식 검증

| # | 불변식 | 검증 |
|---|--------|------|
| 1 | tmux 상태/웹 상태 분리 | **PASS** — xterm.js는 tmux 상태(PANE_OUT 바이트 스트림)의 *시각화 widget이자 read-only mirror*. xterm 내부 buffer(scrollback 500)는 tmux 상태의 derived view이며 web 측이 author하지 않는다. web-only 상태(Panel x/y/w/h/z/visibility/locked/label/note)는 xterm widget 외부의 PanelNode wrapper가 보유 — 두 상태가 widget 경계로 자연 분리. |
| 2 | tmux-native vs web-only 분기 | **PASS** — xterm으로 들어오는(PANE_OUT 0x02)·나가는(PANE_IN 0x03, PANE_RESIZE 0x04) 데이터는 모두 *tmux-native* 액션(tmux 채널을 통해 PTY에 도달). web-only 액션(minimize/hide/lock/label/group reparent/z-index)은 PanelNode wrapper·Sidebar 컴포넌트가 처리하고 xterm widget을 통과하지 않는다. zoom-blur placeholder 토글(D6)은 *DOM 가시성 web-only 분기*로, tmux로 어떤 명령도 발급하지 않는다. |
| 3 | tmux Layout ≠ Canvas Layout | **PASS (강한 보장)** — xterm widget은 *단일 Pane의 출력 1:1*만 렌더. tmux Layout 문자열(SSoT §2.3 `0x07` NOTIFY_MIRROR `kind=layout-change`의 `layout` 필드)은 *xterm.write에 도달하지 않는다* — 디스패처가 PANE_OUT(0x02)과 NOTIFY_MIRROR(0x07)을 다른 핸들러로 라우팅하고, NOTIFY_MIRROR의 layout 문자열은 SSoT §4의 "절대 나타나지 않는 데이터" 정합으로 *trigger 식별자로만 취급*. 본 widget은 Canvas Layout 좌표/그룹/visibility를 *입력으로 받지 않는다* — Panel wrapper가 외부에서 위치·가시성을 결정하고 widget은 자신의 `cols/rows`만 안다. |
| 4 | 보안 기본값 | **PASS** — (a) Svelte 자동 escape가 label/note 보간을 HTML-escape (ADR-0012 #4). (b) xterm option flag(ADR-0003 D9 SECURE_XTERM_OPTIONS, D7로 위임)이 OSC 0/1/2(title set, windowOptions={})·OSC 8 비-http(linkHandler.allowNonHttpProtocols=false)·OSC 52(addon-clipboard 비로드)·proposed API(allowProposedApi=false)·DCS passthrough를 *기본 비활성*. (c) PANE_IN 0x03 송신은 SSoT §2.1에 따라 백엔드가 `send-keys -t %<N> -- <bytes>` argv 분리로 전달하므로 shell 인젝션 표면 없음. (d) `term.write(Uint8Array)`의 ANSI 파서는 xterm v6 내부 책임 — 불완전 시퀀스는 state machine으로 다음 chunk까지 버퍼링(R2 §F5), 부분 시퀀스로 인한 파서 혼란 없음. |
| 5 | control mode 사용 | **N/A** — xterm은 tmux 채널에 직접 접근하지 않는다. WS envelope(ADR-0002)이 추상화 레이어이며 백엔드만 tmux `-C` control mode 클라이언트(ADR-0001)를 사용. 본 widget의 입출력은 모두 *envelope 경계 안*. |

## 미해결 항목 (Open)

- **O1.** 50 pane × 5 stream(D19 워크로드)에서의 메모리·p99 latency 실측. R2 §F10 추정치(150–250 MB → scrollback=500 시 100–150 MB)와 R8 §O3/§O4(remount 비용·Web Worker 분리 트리거)를 합류해 측정. 통과 기준: D19 예산(frontend tab memory < 100 MB, pane output p99 < 100 ms) 충족. 미달 시 (a) scrollback 추가 하향, (b) long-suspend dispose 정책(R2 §F7 P1+), (c) WebGL focus-pane 옵트인(R2 §F3 P1+) 중 1택.
- **O2.** Zoom-blur placeholder 구현 패턴 — `{#if}` 인스턴스 재생성 vs `display:none` retain. 50 pane × frequent zoom 시 frame jank가 측정 가능한 수준이면 retain으로 전환. R8 §O3 measure task 후 결정 (본 ADR 갱신 없이 구현 차원에서 교체 가능 — `XtermHost.svelte` 패턴 변경만).
- **O3.** `@xterm/addon-serialize` 도입 여부 — zoom 복귀 시 last-frame snapshot 재구성 또는 ephemeral state 보존 목적의 클라이언트 측 serialize. R2 §F9는 D15 ring buffer 대체로는 거절했으나 *zoom placeholder 썸네일* 용도는 별도 차원 — measure(O2) 결과 placeholder가 "라벨 + 색"만으로 UX 부족할 경우 P1+ 재방문.
- **O4.** PANE_IN scancode/IME 처리 — `term.onData`는 *조합 완성된 문자열*만 emit. IME 중간 상태(composition event)·OS 단축키 패스스루는 별도 검증 필요. Sprint 4-C FE-1 wiring 시 실측 — gtmux MVP는 macOS/Linux 타깃이므로 IME 시나리오 우선 확인.
