# gtmux

> **English** · [한국어](README.ko.md)
>
> Single-user web-canvas workspace. A Rust supervisor spawns PTY-backed
> shells and lays each one out as a draggable panel on an infinite
> canvas served from one process behind a per-session cookie.

```
You → browser → gtmux server (Rust, axum + tokio) → PTY pool → your shells
                            ↓
                  canvas with Terminal panels,
                  shapes, notes, snippets,
                  documents, images, file refs.
```

---

## What it is

A web app that turns one PTY-backed shell session into a Figma-style
infinite canvas. You drop Terminal panels, sticky notes, shapes,
images, snippet collections, and documents anywhere on the canvas;
groups behave like layer-tree containers; auth and persistence are
scoped to a named *session* that lives at
`${XDG_STATE_HOME}/gtmux/<session>.json`.

There is no tmux at runtime (despite the name). The PTY supervisor
lives inside the gtmux binary itself — see [`docs/adr/0013-pty-direct-no-tmux.md`](docs/adr/0013-pty-direct-no-tmux.md).

---

## Quick install

Detailed flow (Local + Cloud, auth, first session) is in
[QUICKSTART.md](QUICKSTART.md). The 30-second version:

```bash
git clone https://github.com/iiamaii/gtmux.git
cd gtmux/codebase

make codegen                      # OpenAPI → TS types
( cd frontend && npm install --no-audit --no-fund && npm run build )
( cd backend  && cargo build --workspace --release )

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
./backend/target/release/gtmux start --session demo
```

Open the `Open URL: …token=…` line printed on stdout once. After that,
bookmark `http://127.0.0.1:9001/`.

---

## What's on screen

[USAGE.md](USAGE.md) is the full walkthrough. The short version:

- **Toolbar** — 12 tools in 4 semantic groups:
  - Mode: **Select (V)**, **Hand (H)**.
  - Terminal: **Terminal (T)** — spawn a PTY-backed panel.
  - Figures: **Rectangle (R)**, **Ellipse (O)**, **Line (L)**,
    **Free draw (P)**, **Text (T)**.
  - Content: **Note (N)**, **Snippets**, **Document (D)**,
    **Image (I)**, **File path (F)**.
  - Plus **Undo (⌘Z)** / **Redo (⇧⌘Z)** and the **Q-lock** indicator.
- **Session management** — active-session dropdown + titlebar Session
  menu (New / List / Import / Export / Rotate token / Settings /
  Shutdown / Logout).
- **Group feature** — Figma-style layer tree, drag-reparent,
  AND-visibility / OR-lock propagation, sub-tree clipboard, z-index
  separated from tree order.
- **Architecture** — single gtmux process hosting (a) HTTP/WS server,
  (b) terminal-server PTY supervisor with one broadcast channel per
  Terminal, (c) the Svelte 5 web app. Multiple Terminal *panels* can
  mirror one *Terminal* (1 PTY ↔ N panels).

---

## CLI reference

```
gtmux start    --session <name> [--port N] [--workspace PATH] [--config PATH]
gtmux stop     --session <name> [--force]
gtmux teardown --session <name> [--force] [--keep-state] [--keep-config]
gtmux status   [--session <name>]
gtmux rotate-token --session <name>
gtmux set-password / gtmux reset-password
```

Run `gtmux <subcommand> --help` for full flags.

---

## Repository layout

```
codebase/
  backend/     Rust workspace (axum 0.8 + tokio).
               crates/{ws-server, http-api, config, auth, pty-backend}
               bin/{gtmux-cli, gen-openapi}
  frontend/    Svelte 5 + Vite 7 + TypeScript app (ADR-0012).
  shared/      Machine-only handoff (openapi.yaml + generated TS types).
  smoke/       Integration smoke scripts.
  Makefile     codegen / build / test / smoke / clean.
```

---

## Project status

Active development — multi-session pivot (plan-0007) on Stage 5+, with
the Session attach-recovery + delete-UI layers landing under
[ADR-0019](docs/adr/0019-session-and-workspace-model.md).

---

## License

Dual-licensed under **MIT OR Apache-2.0**, matching the Cargo workspace
metadata. Pick whichever fits downstream use.
