# gtmux frontend

> [English](README.md) · **한국어**

Svelte 5 + Vite 7 + TypeScript SPA. Rust backend 와 HTTP (`/api/*`) +
binary WebSocket 프로토콜로 통신. 터미널 출력은 xterm.js, 무한 캔버스
는 `@xyflow/svelte`.

스택:

| | |
|---|---|
| UI framework | Svelte 5 (runes) |
| Build | Vite 7 |
| 타입체커 | svelte-check + TypeScript 5.9 |
| Canvas | `@xyflow/svelte` 1.5 |
| Terminal renderer | `@xterm/xterm` 6 + addons (fit, unicode11) |
| HTTP client | `openapi-fetch` (생성된 `api.d.ts` 로 typed) |
| Icons | `lucide-svelte` |

## 요구사항

- Node.js **≥ 20** (Vite 7 floor).
- `../shared/openapi.yaml` 가 빌드되어 있어야 함 — fresh clone 에서는
  `../` (= `codebase/` 디렉토리) 에서 `make codegen` 1회, 그 다음 backend
  스키마 변경 때마다.
- 실 데이터 작업 시 backend 가 port 9001 에서 실행 중이어야 함
  (`../backend` 에서 `cargo run -p gtmux-cli -- start --session dev`).

## 설치

```bash
npm install
```

## 스크립트

```bash
npm run dev        # http://localhost:5173 Vite dev 서버 (/api proxy 포함)
npm run build      # dist/ 로 프로덕션 번들
npm run preview    # http://localhost:4173 으로 dist/ 서빙 (smoke 테스트)
npm run check      # svelte-check — 빌드 없이 타입 오류만
npm run codegen    # ./codegen/run.sh 재실행 (openapi.yaml → src/lib/types/api.d.ts)
```

## Dev 루프

1. Backend 시작: `cargo run -p gtmux-cli -- start --session dev`
   (`../backend` 에서).
2. Backend 의 banner URL 을 브라우저에서 **한 번** 열어 auth 쿠키 받기.
3. 본 디렉토리에서 `npm run dev` — Vite 가 `/api/*` + WS upgrade 를
   `127.0.0.1:9001` 로 proxy. step 2 의 쿠키 그대로 재사용.
4. 편집. Vite 가 hot-reload.

## Backend 가 dist 를 서빙하도록 번들링

```bash
npm run build
GTMUX_FRONTEND_DIST="$(pwd)/dist" cargo run -p gtmux-cli -- start --session dev
```

Backend 가 `dist/` 를 static root 로 마운트 — 한 프로세스가 API + UI
모두 서빙.

## 디렉토리 구조

```
src/
  routes/        최상위 페이지 (SvelteKit 스타일 layout 아래 +page.svelte).
  lib/
    canvas/      PanelNode / NoteNode / FilePathNode / LineNode renderer,
                 Canvas.svelte 본체.
    chrome/      Modal, dialog, SessionMenu kebab 등.
    sidebar/     LayerTreeView + TerminalListView (LeftPanel content).
    toolbar/     Toolbar2 + tool state.
    stores/      session / workspace / theme / reconnect-gate Svelte 5 store.
    ws/          client.ts + heartbeat.svelte.ts + dispatcher.svelte.ts.
    http/        openapi-fetch 기반 typed REST wrapper.
    keyboard/    shortcutRegistry + chrome / z shortcuts.
    types/       api.d.ts (생성) + 직접 작성한 도메인 타입.
codegen/         openapi-typescript orchestration (run.sh).
```

UX 규칙, ADR 맵, 활성 stage 매트릭스는
`../../docs/agents/frontend-handover-v3.md`.
