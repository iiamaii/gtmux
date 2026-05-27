# gtmux Quickstart — Install · Config · Auth · First Session

> [English] · [한국어](QUICKSTART.ko.md)
>
> Get a gtmux server running on localhost (or a private cloud host) and
> open the canvas in your browser. Covers install, the two run modes
> (Local / Cloud), config, the auth handshake, and creating your first
> session inside the UI.
>
---

## ⚠ Security baseline

- gtmux does **not** terminate TLS itself. Binding `0.0.0.0` and exposing
  port 9001 directly to the public internet sends tokens + cookies as
  plaintext HTTP. Use a trusted network (LAN / VPN / Tailscale) for the
  flow in this document, or front gtmux with a proper HTTPS reverse
  proxy.
- Single user only — one human → one gtmux instance.
- Localhost-only execution (`bind = "127.0.0.1"`, the default) requires
  no cloud config and no TLS.

---

## 0) Prerequisites

| Item | Version | Notes |
|---|---|---|
| Rust | 1.85 | Pinned by `backend/rust-toolchain.toml`. `curl https://sh.rustup.rs -sSf \| sh` and the right toolchain auto-installs on first `cargo` invocation. |
| Node.js | ≥ 20 (22 LTS recommended) | Vite 7 floor. `brew install node` / `nvm install --lts`. |
| OS | macOS / Linux (x86_64 · aarch64) | Windows untested. |

`cargo` and `npm` on `$PATH` is enough — Vite, Svelte, xterm.js, etc. are
pulled in via `npm install`.

---

## 1) Install

### 1.1 Build from source (local or cloud, same procedure)

```bash
git clone https://github.com/iiamaii/gtmux.git
cd gtmux/codebase

# OpenAPI → TypeScript types (frontend build prerequisite).
make codegen

# Frontend dependencies (Vite / Svelte / xterm).
( cd frontend && npm install --no-audit --no-fund )

# Release build: Rust workspace + production frontend bundle.
( cd backend  && cargo build --workspace --release )
( cd frontend && npm run build )

# Outputs:
#   backend/target/release/gtmux   (binary)
#   frontend/dist/                  (static bundle the binary will serve)
```

### 1.2 (Optional) Install the binary system-wide

```bash
sudo install -m 755 backend/target/release/gtmux /usr/local/bin/gtmux
```

Without this step, call the binary by full path
(`./backend/target/release/gtmux …`). Throughout this document the
short form `gtmux` is used.

> `gtmux` refuses to run as root (`EUID == 0`). Always launch as your
> own user.

---

## 2) Run mode A — Local (`bind = 127.0.0.1`, no config file)

If only this machine will connect, skip the config file entirely. The
built-in defaults (`bind = "127.0.0.1"`, `port = 9001`, `mode = token`)
are already the Local mode.

```bash
cd codebase

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
gtmux start --session local
```

Startup banner (printed once on stdout):

```text
gtmux local ready
  Mode:         Local
  Bind:         127.0.0.1:9001
  Open URL:     http://127.0.0.1:9001/auth/bootstrap?token=<hex>
  Token path:   ~/.local/state/gtmux/local.token (0600)
  Backend:      PtyBackend (supervisor pid=<n>)
```

Local-mode characteristics:

| | |
|---|---|
| `bind` | `127.0.0.1` |
| Mode | Local |
| Auth | token bootstrap |
| TLS | not required |
| `[cloud]` block | not required |
| External access | not possible |

`Ctrl-C` shuts the supervisor down cleanly (every child shell is
reaped).

Stop from another terminal:

```bash
gtmux stop --session local
```

---

## 3) Run mode B — Cloud (`bind = 0.0.0.0`, with config file)

Use this when a second device (laptop, phone, another box) on a trusted
network needs to reach the server.

### 3.1 Write a config file

A ready-made template lives at `codebase/config.sample.toml`. Copy it
and replace the two `PUBLIC_IP` placeholders with your server's IP or
domain (also swap `9001` if you change the port).

```bash
mkdir -p ~/.config/gtmux
mkdir -p ~/.local/state/gtmux && chmod 700 ~/.local/state/gtmux

cp codebase/config.sample.toml ~/.config/gtmux/prod.config.toml
$EDITOR    ~/.config/gtmux/prod.config.toml
```

Key fields:

| Key | Value | Why |
|---|---|---|
| `[server].session` | `"prod"` | Must match `--session` |
| `[server].port` | `9001` | Listen port |
| `[server].bind` | `"0.0.0.0"` | Listen on every interface → cloud mode auto-enables |
| `[security].cors_origins` | `["http://PUBLIC_IP:9001"]` | Exact match, no wildcards |
| `[security].host_allowlist` | `["PUBLIC_IP:9001"]` | DNS-rebind defence |
| `[auth].mode` | `"token"` | Bootstrap URL per `gtmux start` (default validated path) |
| `[cloud].tls_required` | `false` | Plaintext HTTP for trusted-network validation. Set `true` once you front the server with HTTPS. |
| `[assets].max_size_bytes` | `104857600` | 100 MiB per uploaded asset (image / document) |

Every key is documented inline in the sample.

### 3.2 Start the server

```bash
cd codebase

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
gtmux start \
  --session prod \
  --config ~/.config/gtmux/prod.config.toml
```

Open the host firewall if any (`sudo ufw allow 9001/tcp`, etc.). The
banner still shows `http://127.0.0.1:9001/auth/bootstrap?token=...` —
replace the host part with your `PUBLIC_IP` to open it from another
device:

```text
http://PUBLIC_IP:9001/auth/bootstrap?token=<hex>
```

---

## 4) Auth — get the cookie

Both modes use the same flow.

1. Open the **bootstrap URL** from the banner once in your browser.
2. The server validates the token, sets an `HttpOnly` + `SameSite=Strict`
   session cookie (7-day rolling renewal), and strips the token from the
   URL.
3. After that, bookmark the path-only URL — `http://127.0.0.1:9001/` in
   Local mode, `http://PUBLIC_IP:9001/` in Cloud mode. The cookie keeps
   you signed in until you log out or it expires.

### Switching to password mode (optional)

Token mode is the default and reissues a new bootstrap URL on every
`gtmux start`. If you want a stable password instead:

```bash
gtmux set-password           # prompts twice, writes Argon2id hash
                              # → ~/.local/state/gtmux/password.argon2 (0600)
```

Then set `[auth].mode = "password"` in the config and restart. The
login page will ask for the password instead of consuming a token (5
attempts / 5 minutes throttle).

To rotate a forgotten password:

```bash
gtmux reset-password
```

To rotate a leaked token (cloud only — local mode rotates on every
start):

```bash
gtmux rotate-token --session prod
```

---

## 5) Create your first session in the UI

When the browser lands on the canvas after step 4, an **Auth dialog**
appears asking you to pick or create a session.

1. Pick **[New session]**.
2. Enter a session name (letters, digits, `-`, `_`).
3. The server creates an empty workspace file at
   `${XDG_STATE_HOME:-~/.local/state}/gtmux/<name>.json` and the canvas
   loads, blank.
4. From now on the **Active session dropdown** in the toolbar (top-left)
   switches between sessions inside the same server. Use the kebab menu
   in the titlebar (`Session menu`) for [New session], [Session list],
   [Import session], [Export session], [Rotate token], [Settings],
   [Shutdown], [Logout].

You can now drop a Terminal panel onto the canvas:

- Click the **Terminal** tool in the toolbar (or press **T**).
- Click somewhere on the canvas — a new PTY is spawned and a Terminal
  panel mounts at that position.
- The shell follows `$SHELL` (default `/bin/zsh` on macOS, `/bin/bash`
  on most Linux). It survives WebSocket disconnects; closing the panel
  via the **×** button kills the shell (with a confirm modal).

Full feature walkthrough — toolbar tools, groups, layer tree, clipboard,
shortcuts — lives in [`USAGE.md`](USAGE.md).

---

## 6) Background / long-running operation

`gtmux start` is a foreground process by default. Two idiomatic ways
to keep it alive after closing the terminal.

### 6.1 `nohup` (simplest)

```bash
cd codebase
mkdir -p ~/.local/state/gtmux

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
nohup gtmux start --session local \
  > ~/.local/state/gtmux/local.log 2>&1 &

tail -f ~/.local/state/gtmux/local.log   # read the bootstrap URL
```

Cloud variant — same `nohup …` invocation with
`--config ~/.config/gtmux/prod.config.toml`.

### 6.2 systemd (recommended for long-lived deployments)

Use a user-level service with auto-restart and journal logging for
long-lived deployments.

Regardless of how you launched it, always stop the server with:

```bash
gtmux stop --session <name>            # SIGTERM, 5 s grace
gtmux stop --session <name> --force    # then SIGKILL
```

This honours the pidfile and reaps every child shell. Killing the shell
job directly leaves orphaned PTYs.

---

## 7) Lifecycle reference

```bash
gtmux status                            # all known sessions + liveness
gtmux status   --session prod           # one session
gtmux stop     --session prod [--force] # graceful / forced shutdown
gtmux teardown --session prod --force   # 5-step cleanup
                                        # (token / layout / pidfile / socket / config)
                                        # opt-out parts with --keep-state / --keep-config
gtmux set-password / reset-password     # password-mode credential
gtmux rotate-token --session prod       # cloud-mode token rotation
```

`gtmux <subcommand> --help` for the complete flag list.

---

## 8) Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `bind=... is cloud-mode but [cloud] section is missing` | Cloud mode without a `[cloud]` block | Add one to the config (`tls_required = false` for the Quickstart path) |
| `[cloud].tls_cert and tls_key must be set when cloud.tls_required=true` | TLS enforced without cert markers | Set `tls_required = false` for plain HTTP, or supply cert/key |
| Browser shows `Forbidden` | `cors_origins` / `host_allowlist` mismatch | Origin must match exactly — scheme + host + port |
| `/` returns `{"error":"not_found"}` | Started server without `GTMUX_FRONTEND_DIST` | Set the env var or install the bundle |
| `cannot find type Group / Panel in api.d.ts` | Skipped `make codegen` | Run `make codegen`, rebuild |
| `Address already in use (os error 48)` | Port taken | `gtmux status` → `gtmux stop --session <name>` |
| `pidfile exists but process is gone` | Previous crash | `gtmux teardown --session <name> --force --keep-state` |
| Bootstrap URL says “Forbidden” on re-open | Cookie expired / cleared | Re-open the most recent banner URL |
| `gtmux` refuses to start | Running as root (`EUID == 0`) | Switch to a regular user |

## Next

- [`USAGE.md`](USAGE.md) — main canvas walkthrough (session management,
  architecture, every toolbar tool, Group feature).
- [`README.md`](README.md) — project overview and document index.
