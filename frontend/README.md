# gtmux frontend

> **English** · [한국어](README.ko.md)

Svelte 5 + Vite 7 + TypeScript single-page app. Talks to the Rust
backend over HTTP (`/api/*`) and a binary WebSocket protocol. xterm.js
renders terminal output; `@xyflow/svelte` powers the infinite canvas.

Stack:

| | |
|---|---|
| UI framework | Svelte 5 (runes) |
| Build | Vite 7 |
| Type checker | svelte-check + TypeScript 5.9 |
| Canvas | `@xyflow/svelte` 1.5 |
| Terminal renderer | `@xterm/xterm` 6 + addons (fit, unicode11) |
| HTTP client | `openapi-fetch` (typed against generated `api.d.ts`) |
| Icons | `lucide-svelte` |

## Prerequisites

- Node.js **≥ 20** (Vite 7 floor).
- A built `../shared/openapi.yaml` — run `make codegen` from
  `../` (the `codebase/` directory) on a fresh clone or whenever the
  backend schema changes.
- A running backend on port 9001 if you want live data (`cargo run -p
  gtmux-cli -- start --session dev` from `../backend`).

## Install

```bash
npm install
```

## Scripts

```bash
npm run dev        # Vite dev server on http://localhost:5173 with /api proxy
npm run build      # Production bundle into dist/
npm run preview    # Serve dist/ on http://localhost:4173 for smoke testing
npm run check      # svelte-check — type errors without building
npm run codegen    # Rerun ./codegen/run.sh (openapi.yaml → src/lib/types/api.d.ts)
```

## Dev loop

1. Start the backend: `cargo run -p gtmux-cli -- start --session dev`
   (from `../backend`).
2. Open the banner URL from the backend stdout in a browser **once** to
   pick up the auth cookie.
3. `npm run dev` here — Vite proxies `/api/*` and the WS upgrade to
   `127.0.0.1:9001`, so the cookie set in step 2 is reused.
4. Edit. Vite hot-reloads.

## Bundling for the backend to serve

```bash
npm run build
GTMUX_FRONTEND_DIST="$(pwd)/dist" cargo run -p gtmux-cli -- start --session dev
```

The backend mounts `dist/` as the static root, so one process serves
both the API and the UI.

## Layout

```
src/
  routes/        Top-level pages (+page.svelte under SvelteKit-style layout).
  lib/
    canvas/      PanelNode / NoteNode / FilePathNode / LineNode renderers,
                 Canvas.svelte itself.
    chrome/      Modals, dialogs, the SessionMenu kebab, etc.
    sidebar/     LayerTreeView + TerminalListView (LeftPanel content).
    toolbar/     Toolbar2 + tool state.
    stores/      session / workspace / theme / reconnect-gate Svelte 5 stores.
    ws/          client.ts + heartbeat.svelte.ts + dispatcher.svelte.ts.
    http/        Typed REST wrappers built on openapi-fetch.
    keyboard/    shortcutRegistry + chrome / z shortcuts.
    types/       api.d.ts (generated) + hand-rolled domain types.
codegen/         openapi-typescript orchestration (run.sh).
```

For UX rules, ADR map, and the active stage matrix see
`../../docs/agents/frontend-handover-v3.md`.
