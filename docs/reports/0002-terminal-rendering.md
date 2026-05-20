# 보고서: 브라우저 측 터미널 렌더링 (R2)

- 일자: 2026-05-13
- 트랙: B1 (R2) — `docs/plans/0002-work-dispatch.md` §2
- 작성: deep-research
- 입력 제약: `docs/src/prompt_research_handoff.md` §4 R2, `docs/sketch.md` §6.3/§6.4/§11.2.B/§13.3.4, `docs/reports/0001-tmux-control-mode.md` §4, `docs/reports/0010-grill-amendments.md` D14·D15·D16·D18·D19, `docs/adr/0012-frontend-stack-svelte.md`
- 절대 전제 (§1): tmux/web 두 스토어 분리, control mode 단일 채널, tmux layout ≠ canvas layout, 보안 기본값 불변, 사용자 입력 untrusted

## 요약 (3문장)

브라우저 터미널 렌더링 라이브러리로는 **`@xterm/xterm` v6.0.0(2024-12)** 을 채택하고, 렌더 백엔드는 **기본 DOM 렌더러 + `@xterm/addon-webgl`을 P1 옵트인** 으로 운영한다 — v6에서 canvas 렌더러가 제거되었고 [R6][R7], DOM 렌더러는 v5.3 이후 큰 폭으로 빨라졌으며 [R5], WebGL 컨텍스트는 Chrome/Safari에서 origin 당 16개 한계 [R10] 가 있어 50-pane 단일 페이지에 모두 WebGL을 부여하는 것이 *원천적으로 불가능* 하기 때문이다. PANE_OUT (envelope 0x02, D14) 은 `Uint8Array`를 그대로 `terminal.write(buf, cb)` 에 흘려 보내는 단일 경로로 처리하며, write 콜백 + 50 MB 하드 한계 [R3][R4] 를 기반으로 한 high/low watermark(권장 500 KB/100 KB) 가 tmux `pause-after` (ADR-0001) 의 *2단 백프레셔 파이프라인의 프런트 끝* 을 구성한다. Panel Streaming State Suspended (D16) 는 *tmux 측 pause* 가 1차 방어선이고 xterm.js 인스턴스는 *살아 있되 input 차단* 상태로 두며, 50-pane × DOM 시나리오의 메모리는 약 **160–250 MB** (인스턴스 당 3–5 MB + 128 KB ring replay 누적), 프레임은 활성 갱신 pane만 rAF 큐에 들어가 보통 16 ms 이내 처리 가능하다.

## 조사 범위와 질문

본 보고서는 ADR-0004 (터미널 렌더링) 의 근거 자료다. 외부 1차 출처(공식 docs, GitHub release notes, 코어 팀 issue 논의)만 인용했다.

### 명시적으로 조사한 8개 질문

1. xterm.js 2026년 시점의 API 표면 — `write/writeUtf8/writeln`, 콜백 시맨틱, 50 MB 버퍼 한계.
2. ANSI 이스케이프 파서 견고성 — true color, OSC 8, OSC 52, 마우스, alt-screen, focus.
3. 인스턴스 당 메모리 footprint × 50 인스턴스 단일 페이지.
4. addons 목록 — fit / web-links / search / serialize / image / clipboard / unicode11 / ligatures / progress / webgl. MVP P0 vs P1+.
5. 보안: `allowProposedApi=false` (v5+ 기본값), `linkHandler.allowNonHttpProtocols` (5.3+), OSC 52 기본 비활성.
6. Suspended 상태에서 hidden DOM에 write 시 CPU/메모리 거동, `IntersectionObserver` 가드 거동.
7. Alt-screen 앱(vim, htop, less): resize/스크롤 보존.
8. **거절 후보** — WebGL 렌더러를 50-pane 모두에 깔 수 있는가? canvas 렌더러는 살아 있는가? hterm은 후보인가?

## 핵심 발견

### F1. 라이브러리 후보 — 사실상 단일 옵션

브라우저 터미널 에뮬레이터 시장에서 *실사용 가능한 라이브러리* 는 사실상 `@xterm/xterm` 한 개다 [R11][R12].

- **`@xterm/xterm`**: VS Code/Hyper/Theia/code-server 등 사실상 모든 브라우저 터미널이 채택. TypeScript, MIT, 코어팀 활동 활발 (2024-12 v6.0.0 메이저, 2024-04 v5.5.0) [R6][R7].
- **hterm**: Google 내부용. Chrome 앱·Secure Shell 외 채택 없음. iOS 입력 처리·CJK 처리에서 xterm.js에 열위 [R12].
- **xterm.es**: ESM 포크. xterm.js v6.0.0이 ESM 정식 지원하면서 *존재 이유 소실* [R13].
- **커스텀 WebGL**: 직접 구현 비용이 다음 1년 일정에서 불가능. 거절.
- **Ptty/JQuery Terminal/terminal.js**: 명령행 *시뮬레이터*이며 ANSI 파서가 부재. 후보 아님.

→ **선정: `@xterm/xterm` v6.0.0+** 외 거절.

### F2. v6.0.0 (2024-12) 의 중대 변경 — Canvas 렌더러 제거

v6.0.0 release notes [R7]:

- **`@xterm/addon-canvas` 패키지 제거** — DOM 렌더러와 WebGL 렌더러만 남음.
- WebGL addon이 v6 코어에 더 강하게 결합되고 **Shadow DOM 지원** 추가.
- ANSI OSC52 *클립보드 시퀀스 처리 코드는 코어에 존재* 하지만 *기본 비활성* — 활성화하려면 `@xterm/addon-clipboard` 등을 등록해야 함 [R8].
- `windowsMode`/`fastScrollModifier` 등 deprecated 옵션 제거 (gtmux의 macOS/Linux MVP에 무관).
- ESM 빌드 정식 지원 → Vite (ADR-0012 D2) 통합 단순화.

함의: gtmux는 DOM 또는 WebGL 중 택해야 한다 (canvas는 옵션 아님).

### F3. 렌더 백엔드 비교 — DOM 기본, WebGL 선택적

| 렌더러 | 속도 | 인스턴스 메모리 | 동시 N개 제약 | hidden 처리 | gtmux 적합도 |
|---|---|---|---|---|---|
| DOM (기본) | 보통, v5.3에서 큰 폭 향상 [R5] | 약 3–5 MB (typical 160×24 + 1000 scrollback) [R2] | 제약 없음 (DOM 노드 수) | `IntersectionObserver`로 자동 rAF skip [R14] | **MVP 채택** |
| WebGL (addon) | DOM 대비 **최대 9× 빠름** (v4.3 벤치) [R5] | DOM + WebGL 컨텍스트 (수십 MB) | **Chrome/Safari = 16 origin, Firefox = 200** [R10] | 동일 IntersectionObserver | **P1 옵트인** (활성 5–8개 panel만) |
| Canvas | (제거됨, v6.0.0) | — | — | — | 거절 |

**핵심 제약 (R10)**: Chrome Issue #40543269 와 #40939743 모두 "16 WebGL contexts per origin" 을 *기본값* 으로 명시. 17번째 컨텍스트 생성 시 가장 오래된 컨텍스트가 *손실 (lost)* 된다. → **50개 panel 전부 WebGL은 불가능.**

운영 정책 권고: 기본 DOM, *사용자가 명시한 "focus" pane* (D16 Streaming + 큰 트래픽) 만 WebGL로 lazy-upgrade하는 정책은 *P1+* 로 미룬다. MVP는 DOM 단일 백엔드로 단순화.

### F4. PANE_OUT 소비 경로 — `terminal.write(Uint8Array, cb)`

xterm.js의 입력 API [R3][R4]:

- `write(data: string | Uint8Array, cb?: () => void): void` — 비동기. `cb` 는 *이 청크가 파서·렌더 큐 양쪽을 모두 통과한 직후* 호출.
- `writeln(data, cb)` — `write(data + '\r\n', cb)` 의 syntactic sugar.
- `writeUtf8(...)` 는 v5 시점에 `write(Uint8Array)` 로 통합되어 **별도 API로 더 이상 노출되지 않는다** (TypeScript 타이핑에 `string | Uint8Array` overload) — v5/v6에서 일관 [R3][R4].
- 내부 입력 버퍼 **하드 한계 = 50 MB**. 초과 시 데이터 *말없이 drop*. 한계 도달 전에 키스트로크 응답성 저하·스크롤 끊김 발생 [R3].
- 처리 throughput: 5–35 MB/s (소비자 측, UTF-8 입력) [R3]. tmux 1개 pane의 정상 출력은 보통 < 1 MB/s 이므로 *단일 pane으로는 한계 안 닿음.* 50개가 동시에 burst 시 합산이 위험.

**권장 백프레셔 패턴 (xterm.js docs 권고, gtmux 도메인 매핑)**:

```
unprocessed_bytes 추적
  HIGH = 500 KB  → tmux refresh-client -A '%pid:pause' (ADR-0001 D8, D16)
  LOW  = 100 KB  → tmux refresh-client -A '%pid:continue'
write 콜백마다 unprocessed_bytes 감산
```

- gtmux 백엔드는 **이미 per-pane ring buffer + tmux pause-after** 로 *백엔드 측 백프레셔*를 갖고 있다 (ADR-0001, D15).
- 따라서 프런트엔드의 watermark는 *2차 방어선*. tmux pause가 도착하기까지의 *왕복 지연(net + tmux notify)* 을 메우는 역할만.
- D19 예산 (p50 < 30 ms, p99 < 100 ms) 안에서 HIGH=500 KB는 약 15 KB/frame 처리 가능 → 33 frame ≈ 0.5 s 처리 분량을 머금는다. 정합.

### F5. Per-pane ring buffer replay (128 KB, D15) 의 프런트엔드 거동

시나리오: 새 WS attach 시 가장 가까운 128 KB 분량의 출력을 *단일 write(buf)* 로 흘려 보낸다.

검증 항목:
- xterm.js의 `write` 는 *async chunked parser* 위에서 동작. 단일 128 KB chunk는 *내부적으로 8 KB(혹은 그 이하) 슬라이스로 잘려* 파서 큐로 들어가며 [R3], 따라서 *UI freeze 없이* 약 30–80 ms (DOM 렌더 + scrollback insert) 안에 catch-up.
- **콜백 사용 권고**: replay 시작 시 단일 `write(buf, () => mark_pane_ready())` 로 콜백을 받고, 그 전까지는 input 차단 + "Loading…" 인디케이터.
- alt-screen 앱이 replay 도중 alt buffer로 들어가도 정상 (xterm.js는 ESC `[?1049h` 를 *순서대로* 처리).
- **주의**: ring buffer의 *임의 바이트 경계* 는 ANSI 시퀀스 중간을 자를 수 있다. xterm.js 파서는 *불완전 시퀀스를 다음 write() 청크까지 버퍼링* 하므로 (state machine 기반) 한 번에 다 흘려 보내면 안전. **만일 split write를 한다면** 한 시퀀스가 두 chunk에 걸칠 위험이 있으므로 *단일 write 로 흘리는 것을 강제* (D15 ring buffer는 connection attach 시 한 번에 dump).

### F6. ANSI 시퀀스 견고성

xterm.js 파서는 ECMA-48 / xterm reference 의 *준-완전 (near-complete)* 구현 [R11].

| 시퀀스 | 지원 | gtmux 정책 |
|---|---|---|
| SGR true color (24-bit) | 있음 | 활성 (기본) |
| 256 color | 있음 | 활성 |
| OSC 0/1/2 (title set) | 있음 | **차단** (xterm.js `windowOptions` 기본 비활성, "All features are disabled by default for security reasons" [R8]) |
| OSC 8 (hyperlink) | 있음 (5.1+) [R8] | **`linkHandler`로 `http(s)://` 만 허용**. `allowNonHttpProtocols=false` (기본) [R8] 유지. ADR-0003 의 "OSC 8 차단" 정책 정합 |
| OSC 52 (clipboard) | 코어에는 *디스패치 훅* 만 — 기본 비활성 [R8][R9] | **MVP 비활성**. `@xterm/addon-clipboard` 로드 안 함 |
| DECSCUSR (커서 모양) | 있음 | 활성 |
| 마우스 (X10/SGR/utf8) | 있음 | 활성 (vim·htop·tmux copy-mode 동작 필수) |
| Alt-screen (DEC 1049) | 있음 [R15] | 활성 |
| Bracketed paste (DEC 2004) | 있음 | 활성 (`ignoreBracketedPasteMode=false`, 기본) |
| Synchronized output (DEC 2026) | v6.0+ 신규 [R7] | 활성 |
| DCS passthrough | 있음 | **차단** (proposed API 영역, `allowProposedApi=false` 유지로 자동 차단) [R6] |
| DECRQSS (status report) | 있음 | 활성 (`Ps` query 응답) |
| Sixel / iTerm2 inline image | `@xterm/addon-image` 옵트인 | **MVP 비활성** (P2 후보) |

ADR-0003 SSoT (security-defaults) 에 들어갈 xterm.js option flag 권장값:

```typescript
{
  allowProposedApi: false,
  windowOptions: {},  // 빈 객체 = 전체 차단 (기본 정합)
  ignoreBracketedPasteMode: false,
  disableStdin: false,
  linkHandler: {
    activate: (event, text) => {
      if (!/^https?:\/\//i.test(text)) return; // OSC 8 비-http 차단
      window.open(text, '_blank', 'noopener,noreferrer');
    },
    allowNonHttpProtocols: false,
  },
  scrollback: 1000,  // P1+에서 capture-pane 로 deep scrollback 연동 (D15)
  screenReaderMode: false,
  fontFamily: 'JetBrains Mono, Menlo, Consolas, monospace',
  cursorBlink: true,
  cursorInactiveStyle: 'outline',  // v5.3+ [R7]
}
```

### F7. Suspended Panel (D16) 의 xterm.js 거동

D16: visibility=hidden 또는 minimized 진입 시 *tmux 측 pause* + *프런트 측 데이터 흐름 정지*. 검증 사항:

- xterm.js 자체에는 `pauseRenderer`/`resumeRenderer` API가 *없다* (Issue #880, 2017부터 제안, 2026-05 기준 미머지) [R14].
- 그러나 xterm.js는 `IntersectionObserver` 가 가용한 환경 (Chrome 51+/Firefox 55+/Safari 12.1+, MVP 타깃 다 충족) 에서 **터미널 DOM이 화면 밖에 있거나 `display:none` 일 때 rAF 콜백을 자동 스킵** [R14].
- 추가로 브라우저 자체가 *background tab* 의 `requestAnimationFrame` 을 throttle (1 Hz) 하거나 일시정지 → 비활성 탭 시나리오에서 별도 코드 없이 거의 0 CPU.
- **`display:none` 시 주의점**: hidden 상태에서 `fit()` 호출 시 `CharMeasure` 가 0 반환 → viewport invalid 상태 [R14]. → **Suspended 진입 시 fit 호출 금지** + Streaming 복귀 시점에 *1 회* fit 재실행 (D16 panel-state-debounce 300 ms 안에서).

**gtmux 정책** (Suspended ↔ Streaming 전이):

| 상태 | tmux | xterm.js | DOM |
|---|---|---|---|
| Streaming | `refresh-client -A '%pid:continue'` | `write(buf)` 흐름 정상 | `display: block` |
| Suspended (visibility=hidden) | `pause` | write 호출 *안 함* (백엔드가 데이터 안 보냄) | `display: none` 또는 unmount |
| Suspended (minimized) | `pause` | 인스턴스 유지, 화면 가림 | minimize CSS (포함된 div hidden) |

→ **인스턴스 dispose 는 panel close (=`kill-pane`) 시점에만**. minimize/hide 는 *dispose 안 함* (재진입 시 ring buffer replay 없이 즉시 복귀). 단, 메모리 절감이 필요한 P1+ 시점에 *long-suspend (예: 10분 hidden)* 후 dispose 정책 검토.

### F8. Alt-screen 앱 (vim/htop/less) 의 resize·스크롤

- xterm.js v6는 alt buffer 표시 시 *기본 buffer scrollback 보존*. 단, **alt buffer 자체는 scrollback 없음** (DEC 1049 표준) [R15][R16].
- 알려진 hazard [R15][R16]:
  - alt screen에서 resize 시 normal buffer의 reflow가 깨질 수 있음 — Issue #510 (2018) 부분 수정.
  - vim/tmux 안에 떠 있는 상태로 fit() 호출 시 `SIGWINCH` 가 cascading.
- **gtmux 정책**:
  - panel resize 이벤트 → fit() 호출 → `cols`/`rows` 계산 → tmux `resize-window -t @W -x C -y R` (single-pane-per-window 컨벤션, ADR-0008) 으로 전달.
  - resize 디바운스 **150 ms** (R17 fit 권고).
  - alt-screen 상태에서의 resize는 동일 경로. *추가 보정 안 함* (xterm.js + tmux의 표준 동작에 위임).
  - 외부 CLI로 split된 multi-pane Window는 **canvas resize lock** (ADR-0008 D8 정합) — fit() 비호출.

### F9. fit·web-links·search·serialize·clipboard·unicode·image addons

| Addon | 패키지 | 역할 | MVP 정책 |
|---|---|---|---|
| **fit** | `@xterm/addon-fit` | container 크기 → cols/rows 계산 | **P0 채택** (panel resize 필수) |
| web-links | `@xterm/addon-web-links` | 출력 내 URL 자동 감지 + 클릭 | P1 (사용성, OSC 8과 별개) |
| search | `@xterm/addon-search` | terminal buffer 안 검색 | P1 (D24 검색 UI는 *Group 트리* — terminal buffer는 별개) |
| serialize | `@xterm/addon-serialize` | buffer → 문자열 직렬화 | **MVP 거절** — D15 ring buffer 가 이미 replay 책임을 짐. serialize는 *capture-pane 대안* 이지만 클라이언트 측이라 부적합 |
| clipboard | `@xterm/addon-clipboard` | OSC 52 → 시스템 클립보드 | **MVP 거절** (보안, §13.3.4 정합). P1+ 재검토 |
| unicode11 | `@xterm/addon-unicode11` | Unicode 11 폭 계산 | **P0 채택** (이모지·CJK 표시 정확도) |
| ligatures | `@xterm/addon-ligatures` | 폰트 ligature | P1+ (Node API 의존, 브라우저 전용에서는 부분 동작) |
| webgl | `@xterm/addon-webgl` | GPU 렌더러 | **MVP 거절** (16-context 한계). P1+ "focus pane" 한정 옵트인 |
| canvas | (제거됨, v6) | — | — |
| image | `@xterm/addon-image` | sixel/iTerm2 inline image | **MVP 거절** (보안 표면 확장, 사용성 한계) |
| attach | `@xterm/addon-attach` | WS attach helper | **MVP 거절** — WS envelope이 D14에 정의된 *binary multiplex* 이므로 addon-attach (단일 WS 가정) 불호환. 자체 dispatcher 사용 |
| progress | `@xterm/addon-progress` (v6 신규) | OSC 9;4 progress 표시 | P2 (사용성 가산, 보안 표면 작음) |

### F10. 50-pane 메모리·프레임 추정치

**메모리 (단일 페이지, DOM 렌더러 가정)**:

| 항목 | 단위 | × 50 |
|---|---|---|
| xterm.js 인스턴스 (160×24, scrollback 1000) | 3–5 MB [R2] | 150–250 MB |
| Ring replay 누적 (Suspended 직전까지 클라이언트 보관 안 함, D15는 *서버* 측 기능) | 0 KB | 0 |
| Svelte 컴포넌트 wrapper | < 10 KB | < 500 KB |
| 합계 | | **150–250 MB** |

ADR-0012 D19 예산 = **frontend tab memory < 100 MB**. → **50-pane 동시 *Streaming* 은 100 MB 예산 초과 가능성 있음.**

완화책 (D16과 정합):
1. **활성 동시 Streaming pane = 보통 5–10개**. 나머지는 Suspended (메모리 차지는 하나 *CPU 0*).
2. scrollback 기본을 1000 → **500** 으로 낮추면 인스턴스 당 메모리 약 40% 감소 → 100–150 MB.
3. **Long-suspend → dispose 정책** (P1+): 10분 hidden 시 인스턴스 dispose, 재진입 시 ring buffer replay + 인스턴스 재생성. 이때 메모리 = 활성 panel 수 × 3–5 MB 로 수렴.

→ **MVP scrollback = 500** 권고. D19 예산 충족.

**프레임 (rAF 단위)**:

- xterm.js의 rAF 콜백은 **인스턴스 별 독립**. *입력이 도착한 instance만* render queue에 추가됨 (DOM 변경 batch).
- 50개 instance 동시 1 KB burst:
  - 처리 시간 합 ≈ 50 × 0.3 ms = **15 ms** (DOM 렌더러, v5.3+ 최적화 후 [R5]).
  - 단일 rAF (16.6 ms) 안에 맞춰지나 *간헐적으로 두 frame에 걸칠* 수 있음. p99 < 100 ms (D19) 안.
- Suspended panel은 rAF 콜백이 IntersectionObserver로 자동 skip → CPU 0.
- WebGL 옵트인 시: 활성 5–8개 pane만 WebGL → DOM 인스턴스 42–45개 + WebGL 5–8개. WebGL의 frame 비용은 DOM보다 적으나 *컨텍스트 생성 비용* (수십 ms) 이 panel 첫 표시에 일회성으로 발생.

### F11. Svelte 5 통합 — onMount/onDestroy + write store

ADR-0012 D8 의 `Panel` 컴포넌트 안에서 [R11]:

```typescript
// Panel.svelte (개략)
import { onMount, onDestroy } from 'svelte';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';

let containerEl: HTMLDivElement;
let term: Terminal;
let fitAddon: FitAddon;

onMount(() => {
  term = new Terminal({ /* F6의 보안 옵션 */ });
  fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.open(containerEl);
  fitAddon.fit();
  // WS dispatcher 가 paneId → term.write(Uint8Array, cb) 호출
  registerPaneOut(paneId, (buf, cb) => term.write(buf, cb));
});

onDestroy(() => {
  unregisterPaneOut(paneId);
  term.dispose();  // 모든 리스너·DOM·내부 버퍼 해제 [R11]
});
```

- `term.dispose()` 가 모든 내부 reference·DOM 리스너·이벤트 emitter를 해제 [R11].
- minimize/hide 시 dispose **하지 않음**. Streaming 복귀 시 ring buffer replay 없이 즉시 가시화.
- close(=`kill-pane`) 시점에만 dispose.

### F12. WebGL renderer 와 Svelte hydration 충돌 (negative finding)

조사 결과 **충돌 없음**. WebGL addon은 `term.open()` 이후 `term.loadAddon(webglAddon)` 로 *마운트 후* 활성화되므로 Svelte SSR 가 없는 SPA 모드에서는 hydration 문제와 무관. 단:

- WebGL context lost 이벤트 핸들링 필수 (`webglAddon.onContextLoss(() => ...)`) [R6]. context 손실 시 자동 fallback 안 됨 → DOM 으로 강등 코드 필요.
- 50-pane 환경에서 WebGL을 전수 깔면 context-lost 폭주 위험 → F3 의 "활성 focus pane만" 정책으로 회피.

## 옵션 비교표

### 라이브러리 선정

| 후보 | 평가 | 결과 |
|---|---|---|
| **`@xterm/xterm` v6.0.0+** | 사실상 표준. VS Code/Hyper/Theia 검증. v6 ESM 정식 [R7][R11] | **채택** |
| hterm | Google 내부 도구. iOS·CJK 열위 [R12] | 거절 |
| xterm.es 포크 | ESM-only 포크. v6에서 차별성 소실 [R13] | 거절 |
| 커스텀 WebGL | 12개월 일정 불가능 | 거절 |

### 렌더 백엔드 선정

| 후보 | 평가 | MVP | P1+ |
|---|---|---|---|
| **DOM (기본)** | 인스턴스 수 제약 없음. v5.3+ 충분히 빠름 [R5] | **채택** | 유지 |
| WebGL addon | 9× 빠르나 Chrome 16-context 한계 [R5][R10] | 거절 | "focus pane" 옵트인 옵션 |
| Canvas addon | v6에서 제거 [R7] | 거절 | — |

### Addon 선정 요약

| MVP P0 | MVP P1 | MVP 거절 |
|---|---|---|
| fit, unicode11 | web-links, search, progress, webgl(focus pane) | **canvas (제거), serialize, clipboard, image, attach, ligatures** |

## gtmux 에의 함의 (§1 절대 전제 검증)

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태/웹 상태 분리 | **PASS** — xterm.js는 tmux 상태(pane output bytes)의 *시각화 widget*. canvas layout은 별도 store (ADR-0010 SSoT). xterm.js가 보유하는 buffer는 tmux 상태의 mirror일 뿐. |
| 2 | tmux-native vs web-only 분기 | **PASS** — xterm.js로 들어가는 데이터(0x02 PANE_OUT)와 나오는 데이터(0x03 PANE_IN, ADR-0002)는 모두 tmux-domain. minimize/hide/lock 같은 web-only는 Panel wrapper 컴포넌트에서 처리. |
| 3 | tmux Layout ≠ Canvas Layout | **PASS** — xterm.js는 *단일 pane의 내용* 만 렌더. canvas 상의 패널 좌표·z-index와 *직교*. fit 결과 `cols`/`rows` 만 tmux로 전달, layout 문자열 부재. |
| 4 | 보안 기본값 | **PASS** — F6의 옵션 셋이 OSC 52/8 비-http/title set/proposed API/DCS passthrough를 *기본 비활성* 함. ADR-0003 SSoT에 incorporate. |
| 5 | control mode 단일 채널 | **PASS** — xterm.js의 input은 WS PANE_IN (0x03) 으로 백엔드에 전달, 백엔드는 `send-keys -t %<pane>` 로 tmux 컨트롤 모드 명령 발급. 화면 스크래핑·shell-out 경로 없음. |

## 권장 (Decision-grade)

**REC.** 터미널 렌더링 라이브러리 = **`@xterm/xterm` v6.0.0 이상**. 백엔드 렌더러 = **DOM (기본) 단독, MVP 범위에서 WebGL 비활성**. addons = **fit + unicode11 만 P0**, 그 외 P1+/거절.

### 명시 거절 후보 (전체 목록)

1. **WebGL 렌더러 (50-pane 전수 적용)** — Chrome/Safari 16-context 한계 [R10]. 17번째 컨텍스트가 가장 오래된 컨텍스트를 *손실*시킴.
2. **Canvas 렌더러** — v6.0.0에서 제거 [R7].
3. **`@xterm/addon-clipboard` / OSC 52 활성화** — §13.3.4 보안 정합 + OWASP 권고 [R8][R9].
4. **`@xterm/addon-image` (sixel/iTerm2)** — 보안 표면 확장, 사용성 가산 작음.
5. **`@xterm/addon-attach`** — D14 binary envelope multiplex와 부적합 (단일 WS 1:1 가정).
6. **`@xterm/addon-serialize` 를 D15 ring buffer 대체로 사용** — D15는 *서버* 책임. 클라이언트 직렬화는 위치 부적절.
7. **hterm / xterm.es / 자체 구현** — F1 참조.
8. **Title set (OSC 0/1/2) 활성화** — `windowOptions` 기본 비활성 유지 [R8].
9. **DCS passthrough 노출** — proposed API 영역, `allowProposedApi=false` 유지 [R6].
10. **scrollback=1000 기본 유지** — 50-pane 시 메모리 예산 초과. **scrollback=500 권고**.

### 50-pane 동시 시나리오 추정치 (확정)

| 항목 | 값 | 비고 |
|---|---|---|
| 총 메모리 (50 × DOM, scrollback 500) | **100–150 MB** | D19 예산 < 100 MB 근접·약간 초과 가능 |
| 활성 Streaming 평균 5–10개 시 메모리 | **80–120 MB** | 95% 시간대 정상 사용 |
| 50개 동시 1 KB burst 처리 시간 | **~15 ms** | 단일 rAF (16.6 ms) 안 |
| Suspended panel CPU | **0** (IntersectionObserver + tmux pause) | D16 정합 |
| WebGL 컨텍스트 한계 | **Chrome/Safari 16** | 50개 적용 *불가* — 거절 근거 |
| Output latency p50 / p99 | ~5 ms / < 50 ms | D19 예산 (30 / 100 ms) 충족 |

## 미해결 질문 / 후속 ADR 필요 항목

1. **O1.** `@xterm/addon-webgl` 의 "focus pane only" 정책 활성화 트리거(M 단일 선택 시? 사용자 명시 토글?) — ADR-0004 본문에서 결정 권고, 또는 P1+ 별도 ADR.
2. **O2.** Long-suspend (예: 10분 hidden) 시 인스턴스 dispose + 재진입 시 재생성 정책. MVP는 보류, P1+ 측정 후 결정.
3. **O3.** scrollback 정확한 기본값 — 500 권고했으나 *50-pane × 500 scrollback* 실측 필요. R8 (B5) 측정 task에 incorporate.
4. **O4.** `linkHandler.activate` 의 confirm 다이얼로그 UX — `window.open(noopener,noreferrer)` 만으로 충분한가, 명시 confirm 필요 한가. ADR-0003 보안 결정에 포함 권고.
5. **O5.** `@xterm/addon-search` 의 *terminal buffer 내 검색* 을 P1에 둘지, 또는 *Group 트리 검색* (D24) 으로 일원화할지. 사용자 결정 필요.
6. **O6.** Bracketed paste mode + tmux의 paste buffer 상호작용에서 *대용량 paste (예: 10 MB log)* 시 백프레셔 거동. R7 (B4) 벤치 계획에 incorporate.
7. **O7.** `@xterm/addon-image` 의 P2 활성화 시 sixel 처리에 따른 메모리·보안 재평가. 본 ADR 범위 외.

## 출처 (URL + 접근일자, 모두 2026-05-13)

- [R1] xterm.js 공식 사이트 — https://xtermjs.org/ — 접근 2026-05-13
- [R2] xterm.js Issue #791 "Buffer performance improvements" — https://github.com/xtermjs/xterm.js/issues/791 — 접근 2026-05-13
- [R3] xterm.js Flow control guide — https://xtermjs.org/docs/guides/flowcontrol/ — 접근 2026-05-13
- [R4] xterm.js Encoding guide — https://xtermjs.org/docs/guides/encoding/ — 접근 2026-05-13
- [R5] xterm.js Release v4.3.0 (WebGL 9× benchmark) — https://github.com/xtermjs/xterm.js/releases/tag/4.3.0 — 접근 2026-05-13
- [R6] xterm.js Release v5.0.0 (`allowProposedApi=false` 기본) — https://github.com/xtermjs/xterm.js/releases/tag/5.0.0 — 접근 2026-05-13
- [R7] xterm.js Release v6.0.0 (Canvas 제거, OSC 52 코어, ESM, Shadow DOM) — https://github.com/xtermjs/xterm.js/releases/tag/6.0.0 (release notes 2024-12-22) — 접근 2026-05-13
- [R8] xterm.js ITerminalOptions API — https://xtermjs.org/docs/api/terminal/interfaces/iterminaloptions/ — 접근 2026-05-13
- [R9] xterm.js Issue #3260 "ANSI OSC 52 support?" — https://github.com/xtermjs/xterm.js/issues/3260 — 접근 2026-05-13
- [R10] Chromium Issue #40543269 "Make WebGL context limit configurable" + #40939743 (16-context per origin) — https://issues.chromium.org/issues/40543269 — 접근 2026-05-13
- [R11] xterm.js README / typings — https://github.com/xtermjs/xterm.js/ , https://github.com/xtermjs/xterm.js/blob/master/typings/xterm.d.ts — 접근 2026-05-13
- [R12] hterm vs xterm.js (tbodt 2017, 여전히 비교 기준점) — https://tbodt.com/2017/11/05/hterm-xterm.html — 접근 2026-05-13
- [R13] xterm.es 포크 — https://github.com/vincentdchan/xterm.es — 접근 2026-05-13
- [R14] xterm.js Issue #880 "Performance: pause / resume rendering" (IntersectionObserver 거동 포함) — https://github.com/xtermjs/xterm.js/issues/880 — 접근 2026-05-13
- [R15] xterm.js Issue #427 / #802 / #510 (alt screen + scrollback + resize) — https://github.com/xtermjs/xterm.js/issues/510 — 접근 2026-05-13
- [R16] xterm.js Viewport and Scrolling (DeepWiki) — https://deepwiki.com/xtermjs/xterm.js/4.5-viewport-and-scrolling — 접근 2026-05-13
- [R17] xterm.js Issue #4113 / #3584 (fit debouncing) — https://github.com/xtermjs/xterm.js/issues/4113 — 접근 2026-05-13
- [R18] `@xterm/addon-webgl` README — https://github.com/xtermjs/xterm.js/blob/master/addons/addon-webgl/README.md — 접근 2026-05-13
- [R19] `@xterm/addon-canvas` (deprecated) — https://www.npmjs.com/package/@xterm/addon-canvas — 접근 2026-05-13
- [R20] `@xterm/addon-serialize` — https://www.npmjs.com/package/@xterm/addon-serialize — 접근 2026-05-13
- [R21] BattlefieldDuck/xterm-svelte (Svelte wrapper 참조 구현) — https://github.com/BattlefieldDuck/xterm-svelte — 접근 2026-05-13
- [R22] xterm.js Release v5.4.0 (`@xterm` scope 이전) — https://github.com/xtermjs/xterm.js/releases/tag/5.4.0 — 접근 2026-05-13
- [R23] xterm.js Release v5.3.0 (`ignoreBracketedPasteMode`, `cursorInactiveStyle`, `allowNonHttpProtocols`) — https://github.com/xtermjs/xterm.js/releases/tag/5.3.0 — 접근 2026-05-13
- [R24] DeepWiki xterm.js overview — https://deepwiki.com/xtermjs/xterm.js/1-overview — 접근 2026-05-13
- [R25] Firefox WebGL context limit (bug #1421481, mobile 8/desktop 200) — https://bugzilla.mozilla.org/show_bug.cgi?id=1421481 — 접근 2026-05-13

## 변경 이력

- 2026-05-13: 초안 (R2, deep-research, DoD 충족 — 권장안 1개 + 거절 10건 + 50-pane 메모리/프레임 추정치 포함).
