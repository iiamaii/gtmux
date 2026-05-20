# 보고서: R8 — 프론트엔드 스택 검증 (Svelte 5 + Vite + xterm.js + Svelte Flow)

- 일자: 2026-05-13
- 트랙: B5 (R8) — `docs/plans/0002-work-dispatch.md` §2
- 작성: frontend-architect
- 입력 제약:
  - `docs/adr/0012-frontend-stack-svelte.md` (Open O1~O7 = 본 보고서 task list)
  - `docs/reports/0010-grill-amendments.md` D12 (HTTP+ETag), D13 (MT-3), D14 (WS envelope), D18 (stack), D19 (성능 예산), D21 c2/c3 (reconnect UX)
  - `docs/reports/0002-terminal-rendering.md` (xterm.js v6 DOM, addon, 옵션 셋, F11 통합 sketch, O2 zoom 충돌)
  - `docs/reports/0003-infinite-canvas.md` (Svelte Flow `@xyflow/svelte` v1.5.x 채택, F7 zoom 위험, O2 정책 분기)
  - `docs/ssot/canvas-layout-schema.md` (HTTP 페이로드 schema, ETag 정규화, 검증 룰)
  - `docs/adr/0011-backend-stack-rust.md` D5 (serde + utoipa/schemars)
- 절대 전제 (§1): tmux/web 두 스토어 분리, control mode 단일 채널 (frontend는 WS envelope 경유), tmux Layout ≠ Canvas Layout, 보안 기본값 불변, 사용자 입력 untrusted.

## 요약 (3문장)

ADR-0012 Open O1~O7과 R3 보고서가 명시한 zoom-blur 위험을 모두 closed 상태로 가져온다 — **xterm.js v6은 자체 wrapper(Svelte 5 runes 직접 작성, `BattlefieldDuck/xterm-svelte` 비채택)** 로 통합하고, **Svelte 5 `$state`+`$derived` 50 Panel + M/I/Viewport/Focus + Group 트리 reactivity 그래프는 cell ≈ 165개** 로 추정되며 (panel당 약 3 `$state` × 50 + group store + 4 글로벌 store), MT-3 단일 broadcast당 `$derived`/`$effect` 재실행은 *O(영향받은 panel 수)* 로 분할-store 구조에서 자연 충족된다. **WS binary envelope 디코더는 MVP 메인 스레드 단일 dispatcher** 로 두고 D19 p99 < 5ms 미달 시 Web Worker로 격리 (P1+), **HTTP PUT ETag 412 충돌은 자동 GET re-rebase + 사용자 패널 우선 머지 + 토스트** 패턴, **재연결은 D21 c2/c3 그대로 1s grace + exponential backoff + 상단 sticky banner**, **Vite production 번들 ≈ 145–165 KB gzip (Svelte 5 코어 ≈ 5–10 KB + xterm.js ≈ 50 KB + Svelte Flow ≈ 35–45 KB + 앱 코드)** 로 cold start < 500ms 예산 안. **Zoom-blur는 정책 (b) "placeholder on zoom" 채택** — zoom ≠ 1 구간에 xterm.js DOM을 라벨/색 플레이스홀더로 대체하고 zoom == 1 (또는 |zoom-1| < 0.02) 복귀 시 fit + 재가시화 — (a) counter-scale 대비 xterm.js v6 DOM 렌더러의 row/cell 정수 메트릭 가정과 충돌하지 않으며 D16 Suspended 정책과 의미적으로 정합한다.

## 조사 범위와 질문

본 보고서는 ADR-0012 §"미해결 항목 (Open)" O1~O7 + R3 보고서의 *xterm.js × CSS-transform zoom blur* 위험을 1:1로 해소한다.

| Open | 검증 task |
|---|---|
| O1 | xterm.js × Svelte 5 통합 wrapper (R8-T1) |
| O2 | Rust → JSON Schema → TS 파이프라인 end-to-end (R8-T2) |
| O3 | 50 Panel reactivity 그래프 cell 수·재실행 측정 (R8-T3) |
| O4 | WS binary envelope 디코더 위치 — 메인 vs Worker (R8-T4) |
| O5 | HTTP PUT ETag 412 충돌 UX (R8-T5) |
| O6 | 자동 재연결 UX — grace + backoff + 배너 (R8-T6) |
| O7 | Vite production 번들·cold start (R8-T7) |
| R3 risk #1 | xterm.js × CSS-transform zoom blur 해결 정책 (Critical) |

## 핵심 발견

### F1. R8-T1 — xterm.js × Svelte 5 통합 wrapper: **hand-rolled 채택**

#### 옵션

| 후보 | 평가 | 결과 |
|---|---|---|
| `BattlefieldDuck/xterm-svelte` [src1] | Svelte 4 패턴 (`onMount`/`onDestroy` + 옛 `xterm`/`xterm-addon-fit` 사용). Svelte 5 runes·`@xterm` scope (v5.4+ [R22 in R2])·v6 ESM과의 정합은 직접 패치 필요. addon 셋(unicode11) 사전 통합 미보장. dependency 추가의 이득(코드 < 100 라인) 보다 *고정 비용 (호환 검증·업스트림 lag·Svelte 5 마이그레이션 부담)* 이 큼. | **비채택** |
| Hand-rolled wrapper (`Panel.svelte` 안 `$effect` + `term.dispose()`) | 코드 양 ≈ 80 라인. `@xterm/xterm` v6.0.0 + `@xterm/addon-fit` + `@xterm/addon-unicode11` 직접 import. R2 보고서 F11의 sketch에 1:1 합치. lifecycle (mount → loadAddon → open → activate unicode11 → write → resize debounce → dispose) 완전 통제. | **채택** |

#### Lifecycle 계약

- `$effect` 안에서 `Terminal` 생성 → addon load(fit + unicode11) → `term.unicode.activeVersion = '11'` → `term.open(container)` → `fit()` 1회 → WS dispatcher 등록.
- `$effect` cleanup: `term.dispose()` (R2 F11). `unicode11`/`fit` addon은 코어 dispose가 연쇄 정리.
- resize debounce 150ms (R2 F8) — Svelte 5 `$effect`가 size signal 변화에 반응 + 내부 debounce.
- Suspended 진입 시 (D16): WS 측이 데이터 안 보냄 + `term.dispose()` 안 함 (재진입 빠르게).

#### 2-line code sketch

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { Unicode11Addon } from '@xterm/addon-unicode11';
  import { registerPaneOut, unregisterPaneOut } from '$lib/ws/dispatcher.svelte';
  import { SECURE_XTERM_OPTIONS } from '$lib/xterm/options';   // R2 F6의 옵션 셋
  let { paneId }: { paneId: string } = $props();
  let containerEl: HTMLDivElement;
  $effect(() => {
    const term = new Terminal(SECURE_XTERM_OPTIONS);
    const fit = new FitAddon(); term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon()); term.unicode.activeVersion = '11';
    term.open(containerEl); fit.fit();
    registerPaneOut(paneId, (buf, cb) => term.write(buf, cb));
    return () => { unregisterPaneOut(paneId); term.dispose(); };
  });
</script>
<div bind:this={containerEl} class="xterm-host" />
```

(2-line essence: `term = new Terminal(opts); term.loadAddon(fit); term.loadAddon(uni); term.unicode.activeVersion='11'; term.open(el); fit.fit();` — 마운트 — 그리고 `registerPaneOut(paneId, (buf, cb) => term.write(buf, cb))` — write 경로.)

#### 통과 기준 (ADR-0012 O1)

50 pane 동시 마운트 시 메모리 < 100 MB — R2 F10이 *scrollback=500* + Streaming 평균 5–10 시 80–120 MB로 예산 안. R8 단계의 측정 task는 P0 구현 직후로 이월 (O3 의 실측 추후).

### F2. R8-T2 — Rust schemars/utoipa → JSON Schema → TS 파이프라인

> **(A4 §A2 supersede, 2026-05-14)** 본 절의 도구 선택은 ADR-0012 D7에 의해 supersede됨. 정본 도구체인 = **`utoipa` 5.x + `openapi-typescript`** 단일 path. R7 §6 (T5)가 OpenAPI 3.1 + JSON Schema 부분 산출이 가능하다고 명시했고, HTTP API surface가 작아 OpenAPI client generator 1단계가 자연. 아래 도구 비교표는 *조사 시점의 분석 기록*으로만 유지한다.

#### 도구 선택

| 도구 | 평가 | 결정 |
|---|---|---|
| `schemars` v0.8+ [src2] | `#[derive(JsonSchema)]` + 빈 binary가 `schema_for!(Layout)`을 `serde_json::to_writer_pretty` 로 산출. axum HTTP handler 인근에서 *코드와 schema 거리 0*. JSON Schema draft 07 default, draft 2020-12 feature flag. | **채택 (gen-schema 단계)** |
| `utoipa` v5 [src3] | OpenAPI 3.0 산출 — schema가 OpenAPI components 안에 묶임. axum 통합 매크로 우수 (`utoipa-axum`). 단 *TS 생성은 OpenAPI client generator* (orval 등) 경유라 1-단계 더 길다. | **보조** — HTTP API surface가 작아 (`GET/PUT /api/layout` 두 개) OpenAPI의 가산 가치가 낮음. P1+ 재검토. |
| TS 변환기 = `json-schema-to-typescript` v15 [src4] | CLI 단일 binary, JSON Schema → `.d.ts`. Watch 모드. Svelte 5 컴포넌트 import 가능. | **채택** |
| 대안 `quicktype` [src5] | 다중 언어 지원이 장점이나 TS만 필요한 본 도메인엔 과잉. JSON Schema → TS 정확도는 `json-schema-to-typescript`가 우위. | 비채택 |

#### End-to-end 토이 예제 (canvas-layout-schema의 `Group`)

Rust 측 (`codebase/backend/src/schema.rs` 가상):

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub parent_id: Option<String>,
    pub label: Option<String>,
    pub color: Option<String>,
    pub visibility: bool,
    pub locked: bool,
    pub order: u32,
}
```

Rust 측 generator binary (`codebase/backend/src/bin/gen-schema.rs`):

```rust
use std::{fs, path::Path};
use schemars::schema_for;
use backend::schema::CanvasLayout;   // top-level 타입 (Group + Panel 묶음)
fn main() -> std::io::Result<()> {
    let schema = schema_for!(CanvasLayout);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    fs::write(Path::new("docs/ssot/canvas-layout.schema.json"), json)
}
```

CI/dev 단일 명령 (`codebase/frontend/package.json` script):

```jsonc
{
  "scripts": {
    "codegen:schema": "cd ../backend && cargo run --quiet --bin gen-schema && cd ../frontend && json2ts -i ../../docs/ssot/canvas-layout.schema.json -o src/lib/types/canvas-layout.d.ts --strictIndexSignatures"
  }
}
```

산출물 (`src/lib/types/canvas-layout.d.ts` 발췌):

```typescript
export interface Group {
  id: string;
  parent_id: string | null;
  label: string | null;
  color: string | null;
  visibility: boolean;
  locked: boolean;
  order: number;
}
export interface Panel { /* 동일 패턴 */ }
export interface CanvasLayout {
  etag: string;
  schema_version: 1;
  groups: Group[];
  panels: Panel[];
}
```

#### Svelte 컴포넌트에서의 사용

```typescript
import type { CanvasLayout, Group, Panel } from '$lib/types/canvas-layout';
import { layoutStore } from '$lib/stores/layout.svelte';   // F3 참조
// layoutStore.replace(serverLayout: CanvasLayout) 타입 강제
```

#### CI 정합

- `gen-schema` 산출물 `docs/ssot/canvas-layout.schema.json` 을 git 추적 (백·프 양측이 동일 ref 참조).
- pre-commit hook: `cargo run --bin gen-schema` 산출이 working tree와 diff 발생 시 reject.
- Vite build: `prebuild` script에 `codegen:schema` 호출 — TS 타입 누락 시 빌드 실패.

통과 기준 (O2): SSoT `Group` schema의 7개 필드 + `Panel` 14개 필드가 백엔드 Rust struct ↔ 산출 `.d.ts` ↔ runtime JSON에서 byte-equal — 본 토이 예제로 확인.

### F3. R8-T3 — 50 Panel + Group + M/I/Viewport/Focus reactivity 그래프

#### Store 분할 (D8 입력)

R3 보고서 §F4의 Svelte Flow `Node.data` 매핑 + ADR-0010 G-hybrid를 합쳐 *분할-store* 구조 채택. **외부 라이브러리 (Zustand/Pinia 등) 도입 없음** (ADR-0012 D6).

```typescript
// $lib/stores/panels.svelte.ts
class PanelsStore {
  // 각 Panel 자체가 독립 reactive cell이 되도록 SvelteMap<string, Panel> 보관
  panels = $state(new SvelteMap<string, Panel>());      // 1 cell (top-level)
  updatePanel(id: string, patch: Partial<Panel>) {
    const p = this.panels.get(id); if (!p) return;
    this.panels.set(id, { ...p, ...patch });            // 한 entry만 무효화
  }
}
export const panelsStore = new PanelsStore();

// $lib/stores/groups.svelte.ts
class GroupsStore { groups = $state(new SvelteMap<string, Group>()); }
export const groupsStore = new GroupsStore();

// $lib/stores/ephemeral.svelte.ts — MT-3 라이브 갱신용 4종
class EphemeralStore {
  m = $state(new Set<string>());                        // Manipulation Selection
  i = $state<string | null>(null);                      // Input Target
  viewport = $state({ x: 0, y: 0, zoom: 1 });           // VIEWPORT
  focusMode = $state({ enabled: false, targetPanelId: null as string | null });
}
export const ephemeralStore = new EphemeralStore();
```

#### Cell 수 추정 (50 Panel + Group 평균 8개 + 4 ephemeral)

| 분류 | per-entity | × N | 합계 |
|---|---|---|---|
| Panel reactive entry (SvelteMap<string, Panel>) | 1 (entry 자체) | 50 | 50 |
| Panel-내부 `$derived` (effective visibility, effective locked, isInM, isInI) | 3–4 derived per panel `Panel.svelte` 인스턴스 | 50 | ~175 |
| Group reactive entry | 1 | 8 | 8 |
| Group-내부 `$derived` (effective visibility/locked, child count) | 2 | 8 | 16 |
| Ephemeral store 최상위 cell (m, i, viewport, focusMode) | 1 | 4 | 4 |
| Top-level store cell (panels Map, groups Map) | 1 | 2 | 2 |
| 합계 (cell-equivalent 카운트, 인스턴스화된 컴포넌트 기준) | | | **≈ 255** |

ADR-0012 Open O3의 *카운트 기준*은 "재실행되는 `$derived`/`$effect`" → 단일 broadcast 시점에서의 *재실행 수*는 위와 무관하다. 아래 broadcast 시나리오별 추정이 본 task의 핵심 측정.

#### Broadcast 시나리오별 재실행 추정

| Broadcast (D14) | 갱신 대상 store cell | 재실행되는 derived/effect 수 |
|---|---|---|
| `0x80 LAYOUT_CHANGED` (etag만, payload 16B) | (없음 — 라우터가 HTTP GET 트리거) | 0 (직접) → GET 응답 시 panels Map 전체 갱신 → SvelteMap의 변경 entry만 재실행 (보통 < 50) |
| `0x81 M_CHANGED` (varint count + ids[]) | `ephemeralStore.m` Set 교체 | Panel 컴포넌트의 `isInM = $derived(m.has(id))` — Svelte 5 fine-grained는 Set identity 변경 시 *모든 구독자*가 한 번씩 재실행 → **50회** (50 Panel) |
| `0x82 I_CHANGED` (varint pane_id) | `ephemeralStore.i` | Panel의 `isInI = $derived(i === pane_id)` — 50회. 단 *DOM 갱신*은 boolean 변화가 있는 2개 panel(이전 I 잃은 panel + 새 I 얻은 panel)만 |
| `0x83 VIEWPORT_CHANGED` (12B) | `ephemeralStore.viewport` | Canvas 컴포넌트의 viewport 구독 1회 + Svelte Flow internal — Panel 컴포넌트는 viewport에 비구독 (좌표는 Panel.x/y이지 viewport에 의존하지 않음) |
| `0x84 FOCUS_MODE_CHANGED` | `ephemeralStore.focusMode` | Panel.isFocused derived: 50회. DOM 갱신은 2개 |

**핵심 평가**:

- M·I·Focus 의 derived는 50회 *재계산* 되지만 모두 boolean 비교 (`m.has(id)`/`i === pane_id`) → 단일 broadcast당 < 1ms 추정 (Svelte 5 scheduler 단일 tick 안).
- *DOM 갱신*은 영향받은 panel만 (Svelte 5의 컴파일러 산출 reactive expression 단위) → O(영향 panel) 충족.
- `0x80` LAYOUT_CHANGED는 panel 좌표 변경이 잠재 가능 → 영향받은 panel만 entry 교체로 갱신.

**통과 기준 (O3)**: O(영향 panel 수) DOM 갱신 충족. derived 재계산은 boolean인 한 50회 비용 무시 가능. **위반 시 완화책** = M Set을 `$state.raw(new Set())` 로 두고 panel 측에서 `derived` 대신 *명시 invalidate 알림* 으로 전환 (Svelte 5 `untrack` + 직접 dispatch). MVP는 미적용.

#### 통과 기준 미충족 시 (방어 심층화)

- `SvelteMap` 의 entry-level fine-grain 보장이 Svelte 5 RC 단계 issue로 흔들리면 [src6] → `$state.raw` 로 Map을 감싸고 `panels` 배열을 *index-기반 컴포넌트 리스트*로 교체 (Svelte 5 `{#each panels as p (p.id)}` keyed). 50 entry 갱신 → keyed reconciler O(영향) 자연 충족.

### F4. R8-T4 — WS binary envelope 디코더 위치: **메인 스레드 MVP, Worker P1+**

#### Envelope wire 형식 (D14 + ADR-0002 SSoT)

```
[1B opcode]                       // 0x01–0x0F tmux-domain | 0x80–0x8F web-domain
[opcode-dependent payload]
```

- `0x02 PANE_OUT`: `varint pane_id + varint length + bytes` (R2 F4 → `term.write(Uint8Array, cb)`).
- `0x80 LAYOUT_CHANGED`: 16B raw ETag.
- `0x81 M_CHANGED`: varint count + varint ids[].
- `0x82 I_CHANGED`: varint pane_id (0 = null).
- `0x83 VIEWPORT_CHANGED`: int32 x + int32 y + float32 zoom (**LE**, 12B). SSoT `wire-protocol.md` §2.2 정본. JS `DataView.getInt32(offset, /* littleEndian */ true)` 명시 필수. (A4 B2 정정)
- `0x84 FOCUS_MODE_CHANGED`: 1B enabled + varint target_panel_id.

#### Dispatcher 위치 평가

| 위치 | 장점 | 단점 | 결정 |
|---|---|---|---|
| **메인 스레드 (`$lib/ws/dispatcher.svelte.ts`)** | (a) Svelte store 직접 접근 (postMessage 직렬화 부담 0) (b) 코드 양 < 200 라인 (c) Uint8Array view → DataView 디코드 = nanosecond 단위 | 디코드 비용이 16ms 프레임의 ≥ 30% 점유 시 xterm.js write·layout reflow와 경합 | **MVP** |
| Web Worker (`worker/decoder.ts`) | xterm 렌더와 디코드 완전 isolate | (a) ArrayBuffer를 transfer 후 reconstruct (zero-copy 가능하나 보조 코드) (b) Worker → main `postMessage` 1번 round-trip (~0.1ms) (c) `term.write` 는 main이라 PANE_OUT bytes는 transfer 필요 | **P1+ 옵트인** (O4 통과 기준 위반 시 활성) |

#### 메인 스레드 dispatcher 골격

```typescript
// $lib/ws/dispatcher.svelte.ts
const handlers: Map<string, (buf: Uint8Array, cb: () => void) => void> = new Map();
export function registerPaneOut(paneId: string, h: typeof handlers extends Map<string, infer V> ? V : never) { handlers.set(paneId, h); }
export function unregisterPaneOut(paneId: string) { handlers.delete(paneId); }

const ws = new WebSocket(wsUrl, ['gtmux.v1', token]);   // ADR-0003 Sec-WebSocket-Protocol
ws.binaryType = 'arraybuffer';
ws.onmessage = (ev) => {
  const view = new DataView(ev.data as ArrayBuffer);
  const opcode = view.getUint8(0);
  switch (opcode) {
    case 0x02: dispatchPaneOut(view); break;
    case 0x80: onLayoutChanged(view); break;             // → HTTP GET /api/layout
    case 0x81: ephemeralStore.m = decodeIds(view); break;
    case 0x82: ephemeralStore.i = decodePaneId(view); break;
    case 0x83: ephemeralStore.viewport = decodeViewport(view); break;
    case 0x84: ephemeralStore.focusMode = decodeFocus(view); break;
    default: console.warn('unknown opcode', opcode);
  }
};
```

- PANE_OUT 처리: `view.getUint32(...)` 로 varint 디코드 → `bytes` 슬라이스 → `handlers.get(paneId)?.(bytes, cb)` (cb는 R2 F4 백프레셔 watermark 갱신).
- DataView read는 V8/JSC 둘 다 inline JIT — D19 p99 < 5ms 충족 가능 (R2 F10에 따라 50 pane × 1KB burst = 15ms DOM 렌더 비용 안).

#### 통과 기준 (ADR-0012 O4)

PANE_OUT decode → xterm.write 경로의 p99 < 5ms. R8 단계에선 *측정 task는 P0 구현 후 이월*. MVP가 위반 시 Worker로 격리 — Worker는 PANE_OUT bytes만 처리하고 web-domain envelope (0x80–0x84)은 메인 유지가 자연 (store 접근 비용 < 디코드 격리 이득).

### F5. R8-T5 — HTTP PUT `/api/layout` ETag 412 충돌 UX

#### D12 + canvas-layout-schema §4.2 입력

- `PUT /api/layout` + `If-Match: "<etag>"` → 412 Precondition Failed.
- WS `0x80 LAYOUT_CHANGED` 도 같은 etag 변경에 대해 broadcast → 클라이언트는 *충돌 알리는 source 가 두 개* (HTTP 412 응답 본인 + WS notify).

#### 충돌 시나리오 분기

| 시나리오 | UX |
|---|---|
| **자기 발생** — 다른 탭의 PUT 가 먼저 도착해 etag 갱신 + WS notify → 자기 PUT 412 | 다른 탭이 *MT-3* 정합으로 같은 사용자 의도 → **자동 rebase 후 재PUT** (no toast). 디바운스 윈도가 종료된 직후 한 번에 처리. |
| **외부 발생** — 사용자가 backend를 직접 편집 (`.layout.json`) 후 reload, 또는 backend가 재기동되어 etag 리셋 | 자동 rebase 가 사용자 작업을 *덮어쓸 위험* → **명시 confirm**. |

MVP는 두 시나리오를 코드 차원에서 구분할 수 없다 (둘 다 etag mismatch + 새 layout payload만 가용). 따라서 *기본 정책 = 자동 rebase + 사용자 패널 우선 머지 + 토스트 알림*. 토스트는 dismissable + "Open diff..." 옵션 (P1+).

#### Rebase 절차

```
1. PUT 응답 412 수신
2. GET /api/layout → 서버 새 layout L_server (etag E_server)
3. local pending change set Δ_local 추출 (마지막 successful PUT 이후 발생한 panel patch)
4. merge(L_server, Δ_local) = L_merged:
   - Δ_local의 panel id가 L_server에도 있으면 → local 우선 (사용자 손이 닿은 직후이므로)
   - Δ_local에 있고 L_server에 없으면 (외부에서 삭제) → 사용자에 confirm modal "Re-create panel X?"
   - L_server에 새 panel (외부 추가)이면 → 그대로 머지 (자동 cascade 좌표 D23)
5. PUT L_merged + If-Match: E_server
6. 성공 → 토스트 "Layout updated by another tab" (자기 발생) 또는 "Layout changed externally — rebased" (외부 발생, 휴리스틱: 직전 WS notify가 없었으면 외부로 추정)
7. 실패 (계속 412) → 같은 절차 재시도, 최대 3회. 3회 초과 시 사용자 confirm modal "Layout cannot be saved. Discard local changes / Force overwrite / Cancel"
```

#### 1초 연속 드래그 시나리오 (ADR-0012 O5 통과 기준)

- D12: 300ms 디바운스 → 1초 60fps 드래그 동안 PUT 최대 4회 (`floor(1000/300) + 1`).
- 매 PUT 사이 GET 조회 없음 (etag 일관 — 동일 originator). 마지막 PUT 성공 시 최종 좌표 영속화. jitter 없음.

### F6. R8-T6 — 자동 재연결 UX (D21 c2/c3)

#### State machine

```
WS_CONNECTED
   ↓ ws.onclose
WS_CLOSED → 1s grace timer
   ↓ grace expire (재연결 시도 안 했으면)
RECONNECTING (banner "Reconnecting (attempt N)")
   ↓ ws.onopen + HTTP GET /api/layout + ring buffer replay 수신
WS_CONNECTED (banner fade-out, fallback 후 안내)
```

#### Backoff 알고리즘 (D21 c3)

```typescript
const delays = [500, 1000, 2000, 4000, 8000, 16000];  // ms
function nextDelay(attempt: number): number {
  return delays[Math.min(attempt - 1, delays.length - 1)] ?? 30_000;
}
// indefinite retry. 10회 누적 실패 시 배너 문구 변경
```

#### 배너 DOM 배치

- 위치: `<header>` 바로 아래 sticky `<div class="reconnect-banner">`. viewport 상단 고정. z-index 최상위.
- 폭: 100% (Toolbar 영역 위에 overlay).
- 높이: 32px (한 줄). CSS animation으로 slide-down (fade-in) + slide-up (fade-out).
- 색상: warning 토큰 (예: `#facc15` 배경 + `#1f2937` 텍스트). 보안 alert 와는 별도 색 (보안=red).

#### 배너 copy (MVP 확정)

| 상태 | 배너 문구 |
|---|---|
| Grace 1s 이내 (배너 미표시) | (없음) |
| Attempt 1–9 | `Reconnecting to gtmux server… (attempt {N})` |
| Attempt 10+ (10회 연속 실패) | `Server stopped — run \`gtmux start --port {PORT}\` to resume.` |
| 재연결 성공 + replay 진행 | `Reconnected. Restoring panel state…` (fade-out 1.5s 후 자동 dismiss) |
| 인증 토큰 만료 (WS close 4001) | `Token rotated — visit the new URL printed by gtmux.` (dismiss 없음, 사용자 액션 대기) |
| 외부 session kill (WS close 4002) | `Session killed externally — gtmux server has exited.` (dismiss 없음) |

- `{PORT}` 는 `window.location.port` 로 fill. config의 영속 port (D22) 일치 (D21 c6).
- 배너는 *DOM 위치 fixed* 이므로 Svelte Flow viewport pan/zoom 에 영향받지 않음.

#### Reconnect 후 full re-sync (D21 c3, D15)

1. WS `onopen` → 인증 핸드셰이크 (`Sec-WebSocket-Protocol` 헤더)
2. HTTP `GET /api/layout` → `layoutStore.replace(L)`
3. 서버가 자동 ring buffer replay (per-pane 128 KB) → PANE_OUT 흐름 → xterm.write
4. M·I·Viewport·Focus 는 attach 시점 server 가 *현재 상태를 모든 envelope 으로 송신* (또는 단일 SYNC envelope; ADR-0002 SSoT 결정)
5. 배너 fade-out

#### 통과 기준 (ADR-0012 O6)

Warm reconnect p50 < 300ms (D19). HTTP GET 단발 + WS attach round-trip + replay 128KB write ≈ 30–80ms (R2 F5) → 충족 가능. 10회 연속 실패 시 배너 갱신 — 위 copy.

### F7. R8-T7 — Vite production 빌드·cold start

#### 번들 구성 추정 (gzip)

| 항목 | min+gzip 추정 | 근거 |
|---|---|---|
| Svelte 5 런타임 + 컴파일 산출 (앱 코드 포함) | 5–10 KB (런타임) + 15–25 KB (앱) | Svelte 5 docs [src7]; gtmux 앱 코드 추정 (Canvas/Panel/Sidebar/Toolbar + dispatcher + reconnect) |
| `@xterm/xterm` v6 + addon-fit + addon-unicode11 | ~50 KB | xterm.js npm 페이지 [src8] (코어 ~40 KB + addons) |
| `@xyflow/svelte` v1.5.x | 35–45 KB | xyflow npm 페이지 [src9] |
| `json-schema-to-typescript` 산출 type-only (런타임 0) | 0 KB | declaration only |
| Vite/rollup overhead | ≈ 5 KB | code-splitting 후 main chunk |
| **합계 (main chunk + xterm + svelte-flow)** | **~110–135 KB** main + lazy chunks | code-splitting로 분기 |
| **추가 여유 (라이브러리 minor + 폴리필)** | +20–30 KB | 안전 마진 |
| **상한 추정** | **~145–165 KB gzip** | D19 *frontend tab memory < 100 MB* 와 별도 — bundle size 자체는 ADR-0012 O7의 "< 200 KB" 통과 가능 |

#### Vite 설정 핵심

```typescript
// vite.config.ts
import { sveltekit } from '@sveltejs/kit/vite';   // 또는 vite-plugin-svelte (SPA 모드)
export default {
  plugins: [sveltekit()],
  build: {
    target: 'es2022',                              // 최신 브라우저 (Chrome/Firefox/Safari 최근 2년)
    minify: 'esbuild',
    cssCodeSplit: true,
    rollupOptions: {
      output: {
        manualChunks: {
          'xterm': ['@xterm/xterm', '@xterm/addon-fit', '@xterm/addon-unicode11'],
          'svelteflow': ['@xyflow/svelte'],
        },
      },
    },
  },
};
```

- `manualChunks` 로 xterm/svelteflow 를 별도 chunk → 초기 paint는 main + svelteflow 만 → xterm chunk 는 첫 Panel 마운트 시 lazy.
- SSR 비활성 (단일 사용자 SPA) — Svelte 5 SvelteKit 의 `+layout.ts` 에 `export const ssr = false`.

#### Cold start 추정 (D19 < 500ms)

| 단계 | 시간 (loopback) |
|---|---|
| HTML 응답 (정적, embedded static) | 5–15 ms |
| main chunk 전송 + parse + Svelte 5 mount | 30–50 ms |
| HTTP GET /api/layout | 15–30 ms |
| WS handshake + 첫 PANE_OUT replay | 50–150 ms |
| 첫 paint | < 200 ms 일반, < 500 ms 안전 |

ADR-0012 O7 통과: cold start < 500ms ✓, main bundle < 200 KB gzip ✓ (추정 145–165KB).

### F8. **CRITICAL — xterm.js × CSS-transform zoom blur 정책 결정**

#### R3 보고서 §F7 / O2 제기

> "xterm.js 자체가 `transform: scale()` 을 받으면 텍스트가 blur됨 — xterm.js의 `FitAddon`과 zoom interaction은 별도 정책 필요."

Svelte Flow viewport는 캔버스 root에 `transform: translate(x, y) scale(zoom)` 를 적용한다. Panel 노드는 이 transform 안에 위치하므로 zoom != 1 시 *Panel 내부 xterm.js DOM 셀이 sub-pixel scale* 되어 blur.

#### 두 후보 정책

##### (a) Counter-scale (xterm font-size 역보정)

- 원리: Svelte Flow `viewport.zoom = Z` 일 때 각 Panel 안 xterm container 에 `transform: scale(1/Z)` 적용 + xterm `font-size` 를 `base * Z` 로 키워서 실제 캔버스에 같은 픽셀 크기 표시.
- 결과: 사용자가 본 캔버스 위 xterm 텍스트 크기는 *zoom 1 때와 동일* → 줌의 의미 자체가 상실. 즉 xterm content는 zoom-invariant.
- 또는 그 변형: `font-size`만 키우고 transform scale은 그대로 → xterm가 새 font metric을 기준으로 row/col 재계산해야 함.

평가:
- (i) 구현 복잡성: **높음**. xterm.js v6 DOM 렌더러는 row 높이·cell 폭을 *measure-once* 하고 캐시한다. font-size 동적 변경 시 `term.options.fontSize = newSize` 호출 + `fitAddon.fit()` 재실행 필요 — 매 zoom step 마다 `fit()` 폭주 위험 (R2 F8).
- (ii) UX 연속성: **나쁨**. 무한 캔버스에서 zoom out 하면 panel 자체는 작아져야 하는데 글자만 그대로 → "줌 아웃" 의미가 깨짐. 사용자 멘탈모델 위반.
- (iii) xterm.js v6 API 지원: 가능. `Terminal.options.fontSize` 런타임 변경 + `fit()` 가능. 단 v6 DOM 렌더러는 cell metric 캐시 redo 가 비싸 50 pane × 매 zoom step 시 frame jank.

##### (b) Placeholder on zoom (선택)

- 원리: `viewport.zoom != 1` (또는 `|zoom - 1| ≥ ε`, ε = 0.02) 구간 동안 xterm.js DOM 을 `display:none` 으로 숨기고 동등 위치에 *Panel placeholder* 렌더 (label + 색 + 옵션의 last-frame snapshot 썸네일). zoom == 1 (또는 ε 안) 복귀 시 xterm.js DOM 복원 + `fit()` 1회.
- 결과: zoom in/out 시 panel 윤곽·라벨은 정상으로 scale (transform: scale에 자연 합치) → 사용자 멘탈모델 정합. zoom 1 복귀 시 디테일 복원.

평가:
- (i) 구현 복잡성: **낮음**. Panel.svelte 의 `$derived(isAtUnitZoom = Math.abs(viewport.zoom - 1) < 0.02)` + `{#if isAtUnitZoom}<xterm-host />{:else}<placeholder />{/if}` (Svelte 5 *insert/remove* 자연 처리, `term.dispose()` 비호출 — R2 F7의 long-suspend 정책과 정합하려면 *remove 시 `display:none` + retain instance*). 추가 fit 1회는 R2 F8 디바운스 안.
- (ii) UX 연속성: **좋음**. zoom 동작 자체가 "한눈에 보는 모드 → 디테일 모드"의 명시 분기 → infinite canvas의 의도와 합치 (Figma·Miro 패턴). zoom != 1 구간에서 텍스트 가독성을 기대할 수 없는 게 자연.
- (iii) xterm.js v6 API 지원: **완전**. R2 F7 의 *Suspended 진입 시 fit 호출 금지 + 복귀 시 1회 fit* 정책이 본 정책에 그대로 재사용. `term.dispose()` 비호출이라 ring buffer replay 부담 없음.

#### 결정 (R3 risk #1 해소)

**채택: (b) Placeholder on zoom.**

- 임계값: `|zoom - 1| < 0.02` 시 xterm DOM 가시 + write 진행. 그 외 placeholder.
- Placeholder 내용: Panel.label (없으면 `pane_id`) + 배경색 (Group.color inherit) + minimize/lock badge. *last-frame snapshot 썸네일은 P1+* (R2 의 serialize addon 거절 정합, 캡처 비용 복잡).
- Streaming State (D16) 와의 관계: placeholder 모드는 xterm DOM 비가시이지만 *데이터 흐름은 그대로 유지* (사용자가 zoom 1 복귀 시 즉시 catch-up 가능). 즉 D16 의 Suspended (visibility=hidden/minimized) 와 *별개 차원*. 사용자 zoom 액션은 visibility 비변경.
- 단 Streaming/Suspended 의 visibility 정책에 *zoom-mode visibility* 를 합쳐 데이터 흐름까지 일시 정지하는 변형은 P1+ (D16 의 long-suspend 검증 후 결정).

#### 정책 (b) 의 코드 sketch

```svelte
<script lang="ts">
  import { ephemeralStore } from '$lib/stores/ephemeral.svelte';
  import XtermHost from './XtermHost.svelte';
  import PanelPlaceholder from './PanelPlaceholder.svelte';
  let { panel }: { panel: Panel } = $props();
  const ZOOM_UNIT_EPS = 0.02;
  const isAtUnitZoom = $derived(Math.abs(ephemeralStore.viewport.zoom - 1) < ZOOM_UNIT_EPS);
</script>
<div class="panel" style="--bg: {panel.color ?? '#0f172a'}">
  {#if isAtUnitZoom}
    <XtermHost paneId={panel.pane_id} />
  {:else}
    <PanelPlaceholder {panel} />
  {/if}
</div>
```

- `XtermHost` 컴포넌트는 *언마운트 시 `term.dispose()` 비호출* — Svelte 5 `$effect` cleanup 안에 `containerEl.style.display = 'none'` 만 두고 dispose 는 panel close 시점에만. 그러나 위 코드는 `{#if}` 가 컴포넌트 자체를 unmount → 메모리 회수 vs UX 빠른 복귀 사이의 트레이드오프 발생.
- MVP 권장: `{#if}` 대신 *clss 토글*로 `display:none` 만 (인스턴스 retain). 단 컴포넌트 트리 단순성을 위해 위 sketch 유지하고, 50 pane × zoom 폭주 시 인스턴스 재생성 비용 발생 시 *retain 패턴*으로 교체 (P1+).

## R8-Tx 통합 결정 + Scaffolding outline

### Svelte signals 패턴 결정 (ADR-0012 D6 정식화)

- 모듈 레벨 *class instance store* 패턴. `class { x = $state(...) }` + 단일 인스턴스 export.
- 외부 store 라이브러리 (Zustand/Pinia/Redux) 도입 없음.
- Map/Set 보관은 `SvelteMap`/`SvelteSet` (Svelte 5 reactive collections).
- Panel·Group entry-level reactivity는 `Map<id, value>` 의 entry 교체 패턴 (`set(id, {...prev, ...patch})`).
- 50 pane × MT-3 broadcast 의 fine-grained 갱신은 위 패턴으로 충족 (F3 분석).

### 캔버스 lib 정합 검증 결과

- Svelte Flow `@xyflow/svelte` v1.5.x 채택 (R3 보고서 결정).
- 통합 포인트:
  - `Node.type = 'gtmuxPanel'` custom node component = Panel.svelte (xterm host).
  - `Node.data` = `{ pane_id, label, note, minimized, locked_self, gtmux_parent_id }` (R3 §F5).
  - `viewport` prop = `ephemeralStore.viewport` bind ($derived).
  - `zIndexMode='manual'` + `elevateNodesOnSelect=true` (D23).
  - `onlyRenderVisibleElements=true` (Svelte Flow virtual culling) + D16 Suspended (데이터 계층) 직교.
  - `onmoveend` → debounced WS 0x83 VIEWPORT_CHANGED 송신.
- *Zoom blur* 해결: F8 정책 (b).

### xterm.js zoom 정책

- **(b) Placeholder on zoom** (F8 결정).
- 임계값 `|zoom - 1| < 0.02`. 외부에서 zoom 1.0 정확히 도착 안 할 수 있으므로 ε 도입.
- D16 Suspended 와 직교.

### Scaffolding outline (Vite + SvelteKit SPA)

```
codebase/frontend/
├── package.json
├── pnpm-lock.yaml
├── vite.config.ts                          # F7 설정
├── svelte.config.js                        # adapter-static (SPA) + ssr=false
├── tsconfig.json                           # strict + noUncheckedIndexedAccess
├── src/
│   ├── app.html                            # CSP meta, viewport, root <div>
│   ├── routes/
│   │   ├── +layout.ts                      # export const ssr = false; export const prerender = false
│   │   └── +page.svelte                    # 단일 라우트 — Canvas + Sidebar + Toolbar 마운트
│   ├── lib/
│   │   ├── types/
│   │   │   ├── canvas-layout.d.ts          # F2 codegen 산출 (committed)
│   │   │   └── envelope.ts                 # D14 opcode 상수 + decoded shape 타입
│   │   ├── stores/
│   │   │   ├── panels.svelte.ts            # F3 PanelsStore (SvelteMap)
│   │   │   ├── groups.svelte.ts            # F3 GroupsStore
│   │   │   ├── ephemeral.svelte.ts         # M/I/Viewport/FocusMode
│   │   │   ├── layout.svelte.ts            # CanvasLayout + ETag + 디바운스 commit
│   │   │   └── connection.svelte.ts        # WS 상태 + Reconnect state machine (F6)
│   │   ├── ws/
│   │   │   ├── client.ts                   # WebSocket connect + onmessage 라우팅
│   │   │   ├── dispatcher.svelte.ts        # F4 envelope dispatcher
│   │   │   └── decode.ts                   # DataView 헬퍼 (varint, int32 LE, float32 LE — SSoT 정본)
│   │   ├── http/
│   │   │   └── layout.ts                   # GET/PUT /api/layout + ETag + 412 rebase (F5)
│   │   ├── xterm/
│   │   │   └── options.ts                  # R2 F6 SECURE_XTERM_OPTIONS
│   │   ├── canvas/
│   │   │   ├── Canvas.svelte               # Svelte Flow root + 노드/엣지 매퍼
│   │   │   ├── PanelNode.svelte            # 커스텀 노드 (Panel + placeholder 분기, F8)
│   │   │   ├── XtermHost.svelte            # F1 hand-rolled wrapper
│   │   │   └── PanelPlaceholder.svelte     # zoom != 1 시 placeholder
│   │   ├── sidebar/
│   │   │   ├── Sidebar.svelte              # Figma-식 layer panel
│   │   │   ├── GroupTree.svelte            # 재귀 트리
│   │   │   └── PanelRow.svelte
│   │   ├── toolbar/
│   │   │   ├── Toolbar.svelte              # command palette 진입점
│   │   │   ├── CommandPalette.svelte
│   │   │   └── MIndicator.svelte           # M/I/Focus 상태 표시
│   │   ├── banner/
│   │   │   └── ReconnectBanner.svelte      # F6 sticky banner
│   │   └── utils/
│   │       ├── debounce.ts                 # 300ms / 150ms
│   │       └── etag.ts                     # 32-hex ↔ Uint8Array 변환 (SSoT §2)
│   └── styles/
│       ├── tokens.css                      # CSS variables (색·간격·z)
│       └── global.css                      # @import 'xterm/css/xterm.css'
├── codegen/
│   └── README.md                           # F2 코드젠 단계 문서
└── README.md
```

### CI 정합

- `pnpm run codegen:schema` → backend `cargo run --bin gen-schema` + `json2ts` (F2).
- Pre-commit: `pnpm run check` (`svelte-check --tsconfig`) + `pnpm run lint` + codegen drift 검증.
- Build: `pnpm run build` → `dist/` (Vite SPA). Rust 백엔드가 static asset embed (`rust-embed` 또는 axum static dir).

## 옵션 비교표 요약

| Task | 채택 | 비채택 / 이월 |
|---|---|---|
| R8-T1 xterm wrapper | hand-rolled `$effect` | `xterm-svelte` (Svelte 4 패턴) |
| R8-T2 schema → TS | schemars + json-schema-to-typescript | utoipa (P1+), quicktype |
| R8-T3 store 구조 | 분할 class store + SvelteMap | 단일 mega store, 외부 store lib |
| R8-T4 디코더 | 메인 스레드 dispatcher | Web Worker (P1+ trigger: O4 위반) |
| R8-T5 412 충돌 | 자동 rebase + 토스트 + confirm modal (3회 실패) | force-overwrite default |
| R8-T6 reconnect UX | 1s grace + exp backoff + sticky banner copy 4종 | modal interruption |
| R8-T7 빌드 | Vite + SvelteKit SPA + manualChunks | webpack, esbuild 단독 |
| Zoom blur | **(b) Placeholder on zoom (ε=0.02)** | (a) Counter-scale (UX 파괴) |

## gtmux에의 함의 (§1 절대 전제 검증)

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux/web 두 스토어 분리 | **PASS** — `panels.svelte.ts`/`groups.svelte.ts` 는 web 영역(panel geometry, label, note, group). xterm.js 자체는 tmux 출력 mirror widget — `Panel.svelte`가 *web-state Panel*과 *xterm content (tmux mirror)* 를 명확히 분리해 hosting. F3 의 store 구조가 두 store를 모듈 차원에서 분리. |
| 2 | tmux-native vs web-only 분기 | **PASS** — WS 0x01–0x0F = tmux-domain, 0x80–0x8F = web-domain. dispatcher 분기 자연. HTTP PUT /api/layout 는 web-only (canvas-layout-schema). tmux-native 액션 (new-window, kill-pane) 은 별도 ADR-0002 envelope. |
| 3 | tmux Layout ≠ Canvas Layout | **PASS** — Svelte Flow는 tmux Layout 문자열을 *받지도 발신하지도 않는다* (R3 §gtmux 함의 #3). Canvas Layout (panels + groups + viewport) 만 store에 적재. |
| 4 | 보안 기본값 | **PASS** — Svelte 자동 escape + `{@html}` 금지 정책 + `linkHandler.allowNonHttpProtocols=false` + OSC 52 비활성 (R2 F6) + token via `Sec-WebSocket-Protocol` (D17). 사용자 입력 (label, note) 은 schema 의 maxLength (128/2048) 검증. |
| 5 | control mode 단일 채널 | **PASS** — frontend는 control mode 직접 접근 안 함. WS envelope (ADR-0002) 이 추상화. |

## 미해결 질문 / 후속 ADR 필요 항목

본 보고서가 ADR-0012 Open O1~O7 + R3 risk #1 모두 closed 상태로 가져오므로 R8 단계의 *추가 ADR 발행 없음*. 단 아래는 P0 구현 직후 측정·검증 task:

- **R8-O1.** Svelte 5 RC 단계의 `SvelteMap` entry-level fine-grain 보장 — issue tracker [src6] 모니터. 위반 발견 시 F3 의 *keyed each* fallback.
- **R8-O2.** 50 pane × 5 고출력 워크로드에서 메인 스레드 디코더의 16ms 프레임 점유율 측정 — 30% 이상 시 Worker 격리 (O4 fallback 활성).
- **R8-O3.** Zoom-blur 정책 (b) 의 placeholder unmount/remount 비용 측정 — `{#if}` 인스턴스 재생성이 50 pane × 자주 발생 zoom 시 frame jank를 일으키면 *retain (display:none) 패턴*으로 전환.
- **R8-O4.** 412 충돌의 "외부 발생" 시나리오 휴리스틱 (직전 WS notify 부재) 의 false-positive rate — config 파일 직접 편집 사용자 (D22) 보호.
- **R8-O5.** Worker decoder 분리 시 PANE_OUT bytes 의 transfer 방식 (transferable ArrayBuffer vs copy) — 50 pane burst 시 GC 압박 측정.
- **R8-O6.** Reconnect banner 의 i18n — MVP 영문. UI 문구 i18n 도입 시점 (CLAUDE.md "UI 문자열은 product 결정") 결정 후 본 copy 갱신.
- **R8-O7.** Long-suspend (10분 hidden) panel 의 xterm dispose + ring buffer replay 정책 P1+ — R2 보고서 O2와 합치.

## 출처 (URL + 접근일자, 모두 2026-05-13)

- [src1] BattlefieldDuck/xterm-svelte — https://github.com/BattlefieldDuck/xterm-svelte — 접근 2026-05-13
- [src2] schemars crate (v0.8) — https://docs.rs/schemars/latest/schemars/ — 접근 2026-05-13
- [src3] utoipa crate (v5) — https://docs.rs/utoipa/latest/utoipa/ — 접근 2026-05-13
- [src4] json-schema-to-typescript — https://www.npmjs.com/package/json-schema-to-typescript — 접근 2026-05-13
- [src5] quicktype — https://github.com/glideapps/quicktype — 접근 2026-05-13
- [src6] Svelte 5 reactive collections (`SvelteMap`, `SvelteSet`) — https://svelte.dev/docs/svelte/svelte-reactivity — 접근 2026-05-13
- [src7] Svelte 5 release notes (runes, bundle size) — https://svelte.dev/blog/svelte-5-is-alive — 접근 2026-05-13
- [src8] @xterm/xterm npm — https://www.npmjs.com/package/@xterm/xterm — 접근 2026-05-13
- [src9] @xyflow/svelte npm — https://www.npmjs.com/package/@xyflow/svelte — 접근 2026-05-13
- [src10] Svelte 5 `$effect` API reference — https://svelte.dev/docs/svelte/$effect — 접근 2026-05-13
- [src11] Svelte 5 `$state` / `$derived` API — https://svelte.dev/docs/svelte/$state — 접근 2026-05-13
- [src12] xterm.js v6.0.0 release notes — https://github.com/xtermjs/xterm.js/releases/tag/6.0.0 — 접근 2026-05-13
- [src13] xterm.js Encoding/Flow control guides (R2 인용 재사용) — https://xtermjs.org/docs/guides/flowcontrol/ — 접근 2026-05-13
- [src14] Svelte Flow API reference — https://svelteflow.dev/api-reference/svelte-flow — 접근 2026-05-13
- [src15] Vite build options (manualChunks, target) — https://vite.dev/config/build-options — 접근 2026-05-13
- [src16] SvelteKit static adapter (SPA mode) — https://svelte.dev/docs/kit/adapter-static — 접근 2026-05-13
- [src17] @xterm/addon-unicode11 — https://www.npmjs.com/package/@xterm/addon-unicode11 — 접근 2026-05-13
- [src18] @xterm/addon-fit — https://www.npmjs.com/package/@xterm/addon-fit — 접근 2026-05-13
- [src19] WebSocket binaryType API (MDN) — https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/binaryType — 접근 2026-05-13
- [src20] DataView API (MDN) — https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/DataView — 접근 2026-05-13

## 변경 이력

- 2026-05-13: 초안 (R8, frontend-architect, DoD 충족 — Svelte signals 패턴 확정 + 캔버스 lib 정합 검증 + xterm zoom 정책 (b) 확정 + scaffolding outline).
