# gtmux

> **English** · [한국어](README.ko.md)

> Single-user web-canvas workspace with a Rust supervisor and a Svelte 5
> frontend. Spawns PTY-backed shells, lays each one out as a draggable
> panel on an infinite canvas, and serves the whole thing from one
> process behind a per-session token.

The legacy name is *tmux-backed web canvas workspace* — gtmux predates
ADR-0013 and now drives PTYs directly. No tmux daemon is required.

---

## Layout

```
codebase/
  backend/    Rust workspace (axum 0.8 + tokio + tokio-tungstenite).
              Crates per ADR-0011 D10 + two binaries:
                crates/{ws-server, http-api, config, auth, pty-backend}
                bin/{gtmux-cli, gen-openapi}
  frontend/   Svelte 5 + Vite 7 + TypeScript app (ADR-0012).
              Codegen entrypoint: codegen/run.sh.
  shared/     Machine-only handoff between backend and frontend.
              Holds the generated openapi.yaml. See shared/README.md.
  smoke/      Integration smoke scripts.
  Makefile    Top-level orchestrator (codegen, build, test, smoke, clean).
```

---

## Prerequisites

| | Version | Notes |
|---|---|---|
| Rust toolchain | **1.85** | Pinned via `backend/rust-toolchain.toml` — rustup auto-installs on first cargo invocation. |
| Node.js | **≥ 20** | Vite 7 floor. |
| npm | bundled with Node | Or pnpm/yarn — codegen script uses `npm run`. |
| OS | macOS / Linux | x86_64 + aarch64 supported (see `rust-toolchain.toml` targets). Windows untested. |

No tmux, no system PTY library, no global Node packages required. Make
sure `cargo` and `npm` are on `$PATH`.

---

## Quickstart (≈ 5 min)

From the repo root after `git clone …`:

```bash
cd codebase

# 1. Generate the OpenAPI schema and the matching TypeScript types.
#    Required on the first run so frontend type-checks pass.
make codegen

# 2. Install frontend deps. One-time.
cd frontend && npm install && cd ..

# 3. Release build of the CLI + the production frontend bundle.
make build

# 4. Start a server bound to a session name of your choice.
#    Default port 9001, bind 127.0.0.1.
./backend/target/release/gtmux start --session demo
```

The startup banner prints a one-time URL on stdout:

```
gtmux demo ready
  Mode:         Local
  Bind:         127.0.0.1:9001
  Open URL:     http://127.0.0.1:9001/auth/bootstrap?token=<hex>
  Token path:   ~/.local/state/gtmux/demo.token (0600)
  Backend:      PtyBackend (ADR-0013, supervisor pid=<n>)
```

Open the **Open URL** once — the server hands you an HttpOnly cookie and
clears the token from the URL. Bookmark the path-only URL
(`http://127.0.0.1:9001/`) thereafter; the cookie keeps you signed in.

`Ctrl-C` shuts the supervisor down cleanly (all child shells are
reaped). Re-running `gtmux start --session demo` reattaches to the same
workspace.

---

## Development (hot reload)

Two processes side by side:

```bash
# Terminal 1 — backend (debug, auto-rebuild on change via cargo-watch optional)
cd codebase/backend
cargo run -p gtmux-cli -- start --session dev

# Terminal 2 — frontend Vite dev server (proxies API + WS to backend)
cd codebase/frontend
npm run dev
```

Vite serves on `http://localhost:5173/` and proxies `/api/*` + the WS
upgrade to the backend on port 9001. Use the banner URL printed by the
backend (it includes the token) and then switch to `http://localhost:5173/`
once the cookie is set, or set `GTMUX_FRONTEND_DIST` so the backend
serves the built bundle directly.

`npm run check` runs `svelte-check` for type errors without building.
`cargo test --workspace` runs the backend test suite.

---

## CLI reference

```
gtmux start    --session <name> [--port N] [--workspace PATH] [--config PATH]
gtmux stop     --session <name> [--force]
gtmux teardown --session <name> [--force] [--keep-state] [--keep-config]
gtmux status   [--session <name>]
gtmux rotate-token   --session <name>
gtmux set-password
gtmux reset-password
```

| Command | Effect |
|---|---|
| `start` | Bootstraps the supervisor, allocates a token, opens HTTP + WS listeners, prints the banner. Holds the foreground until `Ctrl-C` / `SIGTERM`. |
| `stop` | Sends `SIGTERM` to the pidfile process, waits 5 s, optionally escalates with `--force` (SIGKILL). Workspace + token are preserved. |
| `teardown` | Five-step cleanup (socket / token / layout / pidfile / config). `--keep-state` and `--keep-config` opt parts out. |
| `status` | Lists sessions known to `$XDG_STATE_HOME/gtmux/` with daemon liveness. |
| `rotate-token` | Cloud mode only — local mode reissues the token on every `start`. |
| `set-password` / `reset-password` | Argon2id PHC hash for password-mode auth (ADR-0020). |

Run `gtmux <subcommand> --help` for the full flag list.

---

## Configuration

Precedence: **CLI flag → `GTMUX_*` env var → TOML → built-in defaults**.

### Defaults (built-in)

```toml
[server]
session = "<session>"      # supplied by --session
port    = 9001
bind    = "127.0.0.1"      # loopback ⇒ Local mode; other ⇒ Cloud mode

[runtime]
ring_buffer_size_kb     = 128
layout_debounce_ms      = 300
panel_state_debounce_ms = 300
log_level               = "info"      # trace|debug|info|warn|error|off
log_format              = "auto"      # auto (tty→text, pipe→json) | text | json

[security]
cors_origins   = []        # empty ⇒ synthesised from bind at startup
host_allowlist = []
```

### File locations (XDG)

| Purpose | Path |
|---|---|
| Per-session config | `$XDG_CONFIG_HOME/gtmux/<session>.config.toml` |
| Token | `$XDG_STATE_HOME/gtmux/<session>.token` (mode 0600) |
| Pidfile | `$XDG_STATE_HOME/gtmux/<session>.pid` |
| Password hash | `$XDG_STATE_HOME/gtmux/password.argon2` |
| Workspace | `$XDG_DATA_HOME/gtmux/workspace/` (or `--workspace PATH`) |

`$XDG_CONFIG_HOME` defaults to `~/.config`, `$XDG_STATE_HOME` to
`~/.local/state`, `$XDG_DATA_HOME` to `~/.local/share`.

### Environment variables

TOML keys map to `GTMUX_<SECTION>__<KEY>` (double-underscore section
delimiter):

```bash
export GTMUX_SERVER__PORT=9100
export GTMUX_RUNTIME__LOG_LEVEL=debug
export GTMUX_FRONTEND_DIST=/path/to/built/frontend/dist
```

---

## Make targets

```
make help       List targets.
make codegen    Rust utoipa -> shared/openapi.yaml -> TS types.
make build      cargo build --workspace, then vite build.
make test       cargo test --workspace, then svelte-check.
make smoke      Integration smoke (placeholder until C4).
make clean      Remove target/, node_modules/, dist/, codegen outputs.
```

Run `make codegen` on a fresh clone before `make build` so
`frontend/src/lib/types/api.d.ts` exists.

---

## Codegen path

Single direction (ADR-0011 D5 + ADR-0012 D7):

```
Rust struct + utoipa derive
  → cargo run -p gen-openapi
  → shared/openapi.yaml            (committed)
  → openapi-typescript
  → frontend/src/lib/types/api.d.ts  (committed)
```

Both endpoints are committed; CI's `codegen-verify` job
(`.github/workflows/ci.yml`) rejects PRs that change the source but
forget to regenerate.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `cannot find type Group / Panel in api.d.ts` | Frontend built before codegen | `make codegen` then rebuild |
| `Address already in use (os error 48)` | Another `gtmux` on the same port | `gtmux status` to find it / `gtmux stop --session <name>` |
| `pidfile exists but process is gone` | Crashed prior run | `gtmux teardown --session <name> --force --keep-state` |
| Browser shows `Forbidden` after bookmark | Cookie expired / cleared | Re-open the banner URL from the most recent `gtmux start` |
| `make codegen` fails on `openapi-typescript` | `npm install` skipped | `cd frontend && npm install` |

---

## Project status

Active development — multi-session pivot (plan-0007) on Stage 5+, with
the Session attach-recovery + delete-UI layers landing under ADR-0019.
See `../docs/` for the full picture:

- `../docs/sketch.md` — design spec (Korean), source of truth for scope
- `../docs/adr/` — accepted architectural decisions
- `../docs/plans/` — implementation plans (highest-numbered file = active)
- `../docs/reports/` — investigations and session handovers

---

## License

Dual-licensed under **MIT OR Apache-2.0** (matches the Cargo workspace
metadata). Pick whichever fits your downstream use.
