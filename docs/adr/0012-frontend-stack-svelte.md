# ADR-0012: Frontend stack — Svelte 5 + Vite + TypeScript

- 상태: Accepted (2026-05-14, R8 보고서로 Open O1~O7 모두 closed, A4 B3 zoom-blur 정책 inline + A2 codegen 도구체인 utoipa 통일 반영)
- 일자: 2026-05-13 (Proposed) / 2026-05-14 (Accepted)
- 결정자: frontend-architect (grill 산출)
- 근거 보고서: `docs/reports/0010-grill-amendments.md` (D18 tech stack, D19 성능 예산)
- 관련 ADR: ADR-0011 (백엔드 stack — schema 공유 짝), ADR-0005 (캔버스 라이브러리, 미발행), ADR-0004 (터미널 렌더링, 미발행)

## 맥락

본 ADR은 gtmux frontend의 *프레임워크·빌드 도구·언어* 3종을 결정한다. 캔버스 라이브러리(ADR-0005)와 터미널 렌더링 라이브러리(ADR-0004)는 각각 R3·R2 보고서 산출 후 별도 ADR에서 결정한다 — 본 ADR의 범위는 **프레임워크 잠금**까지다.

사용자 입력 제약 (grill §D18):
- **AI agent로 구현 진행**. dev velocity 페널티는 *예측 가능한 wall-clock 비용*이며 코드 품질 손실은 없음.
- **성능 우선** — 단, 프런트엔드는 사용자 인지 latency가 기준. D19에서 정량화된 예산 적용 (cold start < 500ms, pane output p50 < 30ms / p99 < 100ms, 50 concurrent panels MVP, frontend tab memory < 100 MB).

도메인 제약 (D6·D11·D13·D14·D15·D16·D21 누적):
- **MT-3 Live Mirror (D13)**: M·I·Viewport·Focus + N개 Panel 상태가 모두 서버에서 broadcast됨 → *50+ 컴포넌트가 동시에 라이브 갱신*. fine-grained reactivity가 도메인 정합.
- **xterm.js (D4 후속, ADR-0004)**: DOM widget이므로 캔버스 노드 호스트가 DOM-host를 지원해야 함.
- **무한 캔버스 + pan/zoom + Figma-식 사이드바 + drag-reparent** (D11 G-hybrid).
- **WS 0x80–0x8F web-domain envelope (D14)**: binary frame을 ArrayBuffer로 수신·디코딩하여 store에 라우팅.
- **HTTP `PUT /api/layout` + ETag (D12)**: durable layout은 HTTP, WS는 notify만. 클라이언트측 디바운스 300ms.
- **자동 재연결 (D21 c2/c3)**: 1s grace + exponential backoff 0.5→30s cap + indefinite retry + full re-sync (HTTP GET + WS attach + ring buffer replay).
- **wire-protocol·schema 공유 (ADR-0011 D10 정합)**: Rust 백엔드의 HTTP payload schema를 TS 타입으로 자동 생성.

`docs/sketch.md` §10.2 "프런트엔드 구성"(canvas renderer / panel viewport / pane list / command palette / WS client / state mirror / config UI)이 본 ADR의 컴포넌트 분리(D8) 입력.

## 결정 (Decisions)

- **D1.** UI 프레임워크 = **Svelte 5** (signals/runes 기반 fine-grained reactivity). 50+ Panel + MT-3 라이브 갱신에 VDOM diff 비용 없이 *독립 컴포넌트 갱신*.
- **D2.** 빌드 도구 = **Vite** + **`@sveltejs/vite-plugin-svelte`** (Svelte 5 공식 plugin). HMR 활용으로 AI agent iteration 속도 직접 가산.
- **D3.** 언어 = **TypeScript** (`strict: true`, `noUncheckedIndexedAccess: true`). Svelte 컴포넌트는 `<script lang="ts">`.
- **D4.** 터미널 렌더링 widget = **xterm.js** (`@xterm/xterm`) — DOM widget. binary frame 소비 경로는 `terminal.write(Uint8Array)`이며 WS PANE_OUT envelope(ADR-0002)을 직접 받음. **본 ADR-0004(미발행)에서 정식화**, 본 ADR은 *호환성 제약*으로 인용만.
- **D5.** **무한 캔버스 라이브러리 — 본 ADR에서 결정하지 않음.** R3 보고서(B2 산출) 결과로 **ADR-0005에서 잠금**. 본 ADR은 *DOM-host 호환만이 후보 필터*임을 명시 — 즉 *xterm.js가 마운트하는 `<div>` 서브트리를 노드로 호스트하면서 pan/zoom에 참여 가능한* 라이브러리만 후보. canvas/WebGL-only 렌더링(노드 내용까지 GPU로 그리는 라이브러리)은 *후보 단계에서 제외*.
- **D6.** State 관리 = Svelte 5 **runes** (`$state`, `$derived`, `$effect`) + **모듈 레벨 store** (`*.svelte.ts`). Redux/Zustand/MobX 등 외부 store 도입 안 함 — signals로 MT-3 라이브 갱신을 충분히 표현.
- **D7.** Backend schema → TS 타입 = **Rust `utoipa` 5.x** (OpenAPI 3.1 산출, axum 통합 매크로 + JSON Schema draft 2020-12 내장) **→ `openapi-typescript`** (TS 타입 + `openapi-fetch` 호출 코드 자동 생성) → 프런트 import. **R7 §6 (T5) 정본**, A4 §A2 권고에 따라 통일. R8 F2의 `schemars + json-schema-to-typescript` 변형은 supersede됨 (HTTP API surface가 작고 OpenAPI 3.1이 JSON Schema 부분 산출 가능하므로 단일 codegen path가 정합). ADR-0011 D5(serde + utoipa) 산출물과 paired.
- **D8.** 컴포넌트 분리 outline (R8에서 정식 scaffolding 산출, 본 ADR은 *역할 분리*까지 잠금):
  - `Canvas` — viewport·pan·zoom·캔버스 lib 호스팅
  - `Panel` — xterm wrapper + header (label/badge)
  - `Sidebar` — Figma-식 layer panel (Group 트리)
  - `Toolbar` — command palette·M·I·Focus 표시
  - `WSClient` — binary envelope 디코더·dispatcher (0x01–0x0F tmux-domain, 0x80–0x8F web-domain)
  - `HTTPClient` — `GET/PUT /api/layout` + ETag + 디바운스 (D12)
  - `ReconnectController` — D21 c2/c3의 grace + backoff + full re-sync

## 거절된 대안 (Rejected)

| 후보 | 거절 이유 (D18 표 + 본 ADR 보강) |
|---|---|
| **R1. React + TypeScript** | VDOM diff + 수동 memoization 부담. MT-3 D13 broadcast로 50+ Panel이 동시 갱신되면 `useMemo`/`useCallback`/`React.memo`의 *최적화 부담이 도메인 본질이 아닌데도* 코드 전반에 침투. fine-grained reactivity가 더 자연. 작동은 가능하지만 우리 도메인에 가산 가치 없음. |
| **R2. Vue 3 + TypeScript** | Composition API + reactivity proxy로 Svelte와 가까운 모델이나, *결정적 이점 부재*. 컴파일러 산출물 크기·런타임 footprint 모두 Svelte 5가 약간 우위. ecosystem은 React에 비해 React 우위가 강한 도구(xterm 등)에 의존하지 않는 영역이므로 차이 미미. |
| **R3. Solid.js + TypeScript** | fine-grained reactivity 모델은 Svelte와 동급으로 우수. 단점은 **생태계가 Svelte보다 niche** — UI 라이브러리·통합 가이드·LLM 학습 데이터 모두 Svelte가 우위. 같은 reactivity 모델이면 ecosystem 큰 쪽 선택. |
| **R4. Vanilla TypeScript + 미니 프레임워크** | wheel 재발명. 50+ Panel 독립 갱신, MT-3 broadcast 라우팅, drag-reparent, 캔버스 lib 통합을 모두 직접 구현하는 비용이 *Svelte 의존성 비용*을 명백히 초과. 도메인 외부에 인지 부하 집중. |
| **R5. Angular / Lit / Qwik / 기타** | 생태·학습곡선·도메인 정합 어느 차원에서도 D1~D4의 후보를 넘지 못함. 거절. |

## 결과 (Consequences)

- 긍정:
  - Svelte 5 signals가 *50+ Panel의 독립 갱신*을 자연 처리 — D13 broadcast 도착 시 영향받는 panel만 갱신, 무관 panel 재실행 없음.
  - 컴파일 산출물 사이즈 작음 (Svelte ≈ React/Vue 대비 30–50%) → D19 frontend tab memory < 100 MB 예산 여유.
  - TypeScript 단일 언어로 백엔드 schema → 프런트 타입 single source (D7).
  - Vite HMR이 AI agent iteration 속도에 직접 기여. cold reload < 1s 일반.
  - Svelte 자동 escape가 §13.3.4 XSS 방어를 *언어 기본값으로* 제공 (불변식 #4 정합).
- 부정/비용:
  - Svelte 5 (signals/runes) 생태가 Svelte 4 대비 변동 진행 중. 일부 third-party 라이브러리(특히 캔버스/drag-drop 영역)는 호환 검증 필요 → R8.
  - 일부 캔버스/drag-drop 라이브러리는 React 우선 — 필요 시 Svelte wrapper 직접 작성. 작성 비용은 한정적.
  - LLM 학습 데이터는 Svelte 5 (runes)가 Svelte 4보다 적음 — AI agent가 옛 패턴(`writable()`) 회귀 가능성. R8 scaffolding이 *정식 runes 패턴*을 코드로 고정하여 완화.
- 후속:
  - R8 보고서가 signals 패턴·캔버스 lib 정합·xterm.js 통합·schema codegen 도구체인·scaffolding 산출 → 본 ADR Accepted 승격 (`docs/plans/0002-work-dispatch.md` B5).
  - R3 보고서가 캔버스 lib 후보 평가 — DOM-host 가능 후보 중에서 선택 (ADR-0005).
  - ADR-0011 D5(serde + utoipa/schemars) 산출과 본 ADR D7이 *동일 codegen 파이프라인의 두 끝*. R8에서 end-to-end 검증.

## 불변식 검증

| # | 불변식 | 검증 |
|---|--------|------|
| 1 | tmux 상태/웹 상태 분리 | PASS — 프런트엔드는 web 측 author. tmux 상태는 WS envelope을 통해 *불변 mirror*로만 수신 (PANE_OUT 등 0x01–0x0F). store 차원에서 tmux-mirror store와 web-state store 별도 모듈로 분리 (D8). |
| 2 | tmux-native vs web-only 분기 | PASS — `Panel` 컴포넌트가 *tmux mirror 필드*(pane_id, output bytes, dead flag)와 *web-only 필드*(x/y/w/h/z, visibility, locked, label, note)를 TypeScript 타입 차원에서 분리. 단방향 액션 라우팅: tmux-native 액션 → WSClient → 백엔드 tmux command, web-only 액션 → HTTPClient `PUT /api/layout`. |
| 3 | **tmux Layout ≠ Canvas Layout** | **PASS (강한 보장) — 프런트엔드는 tmux Layout 문자열을 *받지도 발신하지도 않는다*.** WS envelope의 web-domain 슬롯(D14 0x80–0x8F)에 tmux Layout 페이로드가 정의되어 있지 않고, tmux-domain 슬롯(0x01–0x0F)에도 layout 문자열 슬롯이 없다(ADR-0001/0002 allowlist 결과). HTTP `PUT /api/layout` payload는 Canvas Layout(`groups`+`panels` 트리, D11) 전용으로 schema 강제. 따라서 *frontend code 차원에서 두 layout을 혼동할 수 있는 경로가 존재하지 않는다.* Svelte store 또한 Canvas Layout 단일 종류만 관리. |
| 4 | **보안 기본값** | **PASS — Svelte의 자동 escape가 모든 `{expr}` 보간을 HTML-escape한다.** 사용자 입력(panel label, note, group label, command palette 입력)은 Svelte가 *기본값으로* escape하므로 §13.3.4 XSS 1차 방어선이 *언어 기본값*. `{@html ...}` 사용은 코드 리뷰 시 정책으로 금지(React `dangerouslySetInnerHTML` 대응 API 부재가 *덜 위험한 기본값*). `connect-src` CSP는 백엔드 ADR-0003 책임. xterm.js 내 OSC 시퀀스(특히 OSC 52)는 ADR-0003 SSoT의 xterm option flag로 제어. |
| 5 | control mode 사용 | N/A — 프런트엔드는 tmux control mode 채널에 직접 접근하지 않음. WS envelope(ADR-0002)이 추상화 레이어. |

## 미해결 항목 (Open) — R8 보고서에서 검증·결정

각 항목은 *측정 가능한 검증 기준*으로 정의한다. R8 보고서 DoD는 본 목록의 *모든 항목이 closed* 상태.

- **O1. xterm.js + Svelte 5 통합 wrapper 작동 검증.**
  - 측정: 기존 `xterm-svelte` 등 wrapper의 Svelte 5 runes 호환 여부 확인. 호환 불가 시 직접 wrapper 작성 (lifecycle: `$effect` 마운트/`onDestroy` dispose, addon 등록 순서).
  - 통과 기준: 50 pane 동시 마운트 시 메모리 < 100 MB (D19), p50 paint < 30ms.
  - **Zoom-blur 정책**: R8 F8 (b) **placeholder on zoom** (ε = 0.02) — Svelte Flow의 `transform: scale()`이 xterm.js 텍스트 렌더를 blur시키므로, `|zoom - 1| ≥ 0.02` 범위에서 xterm DOM을 숨기고 placeholder(라벨 + 배경색)로 대체, zoom이 정상 범위로 복귀하면 xterm DOM 복원 + `fit()` 1회 호출. Streaming State(D16)와 직교 (데이터 흐름 유지, DOM만 토글). 거절: (a) counter-scale font-size — 50 pane × zoom step마다 cell metrics 재계산 비용 + infinite-canvas 멘탈모델 파괴. (A4 B3 정정, R8 F8 정본)

- ~~**O2. Rust schemars/utoipa → JSON Schema → TypeScript 파이프라인 end-to-end 검증.**~~ → **해소** (A4 §A2): **`utoipa` 5.x → `openapi-typescript`** 단일 path 채택. R7 §6 정본, R8 F2 `schemars` 변형은 supersede. 통과 기준 (`groups`/`panels` 타입 byte-equal)은 그대로 적용되며, 도구체인은 본 D7에서 잠금.

- **O3. Svelte 5 runes + 모듈 store의 50-Panel reactivity 그래프 측정.**
  - 측정: 50 Panel 마운트 상태에서 단일 `M_CHANGED` (D14 0x81) broadcast 도착 시 *재실행되는 `$derived`/`$effect`* 수를 dev-tool로 카운트.
  - 통과 기준: O(M의 panel 수) 재실행, O(N) 아님. 위반 시 store 분할 또는 `$state.raw()` 적용.

- **O4. WS binary envelope 디코더 위치 (메인 스레드 vs Web Worker).**
  - 측정: 50 pane × 5 고출력(D19 워크로드) 시 디코더의 메인 스레드 점유율. 16ms 프레임 예산의 ≥ 30% 점유 시 Web Worker 분리.
  - 통과 기준: PANE_OUT decode → xterm.write 경로의 p99 < 5ms.

- **O5. HTTP PUT `/api/layout` 디바운스 + ETag 충돌 처리 패턴.**
  - 측정: D12의 300ms 디바운스 + `If-Match` → 412 시 GET 재조회·재send 흐름이 *드래그 연속 작업*에서 패널 위치 jitter 없이 작동.
  - 통과 기준: 1초 연속 드래그(60fps) 동안 PUT 호출 ≤ 4회, 최종 상태 일관.

- **O6. 자동 재연결 (D21 c2/c3) UX 검증.**
  - 측정: 백엔드 강제 종료 → 재기동 시 클라이언트가 1s grace, 그 후 exponential backoff(0.5→30s), 재연결 시 HTTP GET layout + WS attach + ring buffer replay(D15) 흐름 완수.
  - 통과 기준: warm reconnect p50 < 300ms (D19), 10회 연속 실패 시 배너 문구 갱신.

- **O7. Vite production 빌드 산출물 크기·로드 latency.**
  - 측정: `vite build`의 main bundle gzip 크기, cold start (gtmux start → 첫 paint).
  - 통과 기준: cold start < 500ms (D19), main bundle < 200 KB gzip.
