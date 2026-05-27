# gtmux

> **English** · [한국어](README.ko.md)

**gtmux is a single-user web canvas for terminal-centered work.**
It runs a local or private-cloud Rust server, spawns PTY-backed shells,
and lets you arrange terminals, notes, snippets, documents, images,
shapes, and file references on an infinite browser canvas.

It is designed for people who live in terminals but need more spatial
context than a tab list: operators, developers, SREs, researchers, and
anyone who keeps several command lines, notes, runbooks, and references
open while working through a task.

```
Browser canvas
  ├─ Terminal panels      live PTY shells rendered with xterm.js
  ├─ Snippets             one-click reusable commands/text blocks
  ├─ Notes & documents    markdown, PDFs, file references, images
  ├─ Shapes & text        visual grouping and lightweight diagrams
  └─ Groups & layers      structure, visibility, locking, z-order

          HTTP + WebSocket
                │
                ▼
gtmux server: Rust · axum · tokio · portable-pty
```

---

## Why It Exists

Terminal work is rarely just one terminal. A real task often has:

- a running server, a database shell, a log tail, and a deploy command;
- notes about the current incident or experiment;
- commands that should be copied accurately, not retyped;
- files, screenshots, diagrams, and references that explain what is
  happening;
- multiple related work areas that should stay visually separate.

gtmux turns that into a persistent workspace. Instead of remembering
which terminal tab was which, you place panels where they make sense,
group related work, attach notes and snippets near the relevant shell,
and return later to the same layout.

---

## What You Can Do

- **Run real shells in the browser.** Terminal panels are backed by PTYs
  managed by the gtmux server. They survive browser reloads and
  WebSocket reconnects while the server process is alive.
- **Work spatially.** Drag, resize, group, hide, lock, minimize,
  maximize, and reorder items on an infinite canvas.
- **Keep commands close.** Snippet collections store reusable command or
  text blocks as badges. Click a badge to copy its body.
- **Document as you go.** Add notes, markdown documents, PDFs, images,
  file paths, shapes, free-draw marks, and text labels next to the
  terminals they explain.
- **Organize complex tasks.** Use groups and the layer tree to keep
  workflows tidy without mixing visual layout with terminal process
  lifecycle.
- **Recover from normal interruptions.** Reconnect banners, attach
  recovery, terminal ring buffers, and persistent layout files make
  browser refreshes and short network drops less disruptive.
- **Move layouts around.** Import/export session JSON for backups or
  templates. Live terminal output and uploaded asset bytes are not
  bundled in exports.

---

## Convenience And Expected Benefits

gtmux is not trying to replace your shell. It gives your shell work a
workspace.

- **Less context switching:** terminal, notes, snippets, and references
  stay in one visual surface.
- **Fewer command mistakes:** frequently used snippets can be copied
  from named badges instead of being retyped from memory.
- **Better task recall:** spatial layout, labels, notes, and groups make
  it easier to remember what each terminal was doing.
- **Cleaner handoff to yourself:** export layouts, keep runbooks near
  command panels, and return to long-running work without reconstructing
  the screen from scratch.
- **Lower local setup overhead:** one Rust process serves the frontend,
  HTTP API, WebSocket stream, auth, layout persistence, and PTY
  supervisor.

---

## Technology Stack

### Backend

- **Rust 1.85**
- **axum 0.8** and **tower/tower-http** for HTTP, static serving,
  middleware, CORS, Host validation, and API routing
- **tokio 1.52** for async runtime, process handling, IO, signals, and
  timers
- **tokio-tungstenite** for WebSocket transport
- **portable-pty** for cross-platform PTY-backed child shells
- **serde / serde_json** for layout and API data
- **figment + TOML** for configuration
- **argon2** for password-mode credential storage
- **utoipa + openapi-typescript** for OpenAPI-driven frontend types

### Frontend

- **Svelte 5**, **TypeScript 5.9**, **Vite 7**
- **@xyflow/svelte** for the canvas/node interaction foundation
- **xterm.js 6** with fit and Unicode 11 addons for terminal rendering
- **marked + DOMPurify** for sanitized markdown document rendering
- **lucide-svelte** for UI icons
- OpenAPI-generated API types shared from the backend contract

---

## Quick Start

Full setup instructions are in [QUICKSTART.md](QUICKSTART.md). The
short version:

```bash
git clone https://github.com/iiamaii/gtmux.git
cd gtmux/codebase

make codegen
( cd frontend && npm install --no-audit --no-fund && npm run build )
( cd backend  && cargo build --workspace --release )

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
./backend/target/release/gtmux start --session demo
```

Open the `Open URL: .../auth/bootstrap?token=...` line printed by the
server once. After the cookie is issued, use the normal root URL such as
`http://127.0.0.1:9001/`.

---

## Local And Cloud Modes

gtmux is single-user software. It is intended for:

- **Local mode:** bind to `127.0.0.1`, run on your own machine, no TLS
  required.
- **Private cloud mode:** bind to a trusted LAN/VPN/Tailscale interface
  with explicit CORS and Host allowlists.
- **Public internet exposure:** put gtmux behind a proper HTTPS reverse
  proxy. Do not expose plaintext HTTP with tokens and cookies to the
  public internet.

See [QUICKSTART.md](QUICKSTART.md) for the local/cloud setup flow.

---

## Documentation Map

- [QUICKSTART.md](QUICKSTART.md) — install, config, auth, first session
- [USAGE.md](USAGE.md) — full UI walkthrough after sign-in

---

## Repository Layout

```
codebase/
  backend/     Rust workspace
               crates/{http-api, ws-server, auth, config, pty-backend}
               bin/{gtmux-cli, gen-openapi}
  frontend/    Svelte 5 + Vite + TypeScript browser app
  shared/      Generated OpenAPI handoff files
  smoke/       Integration smoke scripts
  Makefile     codegen / build / test / smoke / clean
```

---

## Project Status

gtmux is under active development. Core terminal panels, session
management, canvas layout, groups, snippets, documents, assets,
import/export, auth, reconnect handling, and local/cloud startup paths
are implemented, but the project should still be treated as evolving
software rather than a stable production platform.

---

## License

Dual-licensed under **MIT OR Apache-2.0**, matching the Rust workspace
metadata.
