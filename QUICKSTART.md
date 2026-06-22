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
built-in defaults (`bind = "127.0.0.1"`, `port = 9001`) are already the
Local mode. A ready-made template — only needed if you want to tune the
port / logging — lives at `codebase/config.local.sample.toml`.

```bash
cd codebase

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
gtmux start --name local
```

Startup banner (printed once on stdout):

```text
gtmux local ready (instance)
  Mode:         Local
  Bind:         127.0.0.1:9001
  Open URL:     http://127.0.0.1:9001/auth/bootstrap?token=<hex>
  Token path:   ~/.local/state/gtmux/local.token (0600)
  Workspace(A): /Users/you
  Store(C):     ~/.local/state/gtmux/store/local
  Backend:      PtyBackend (ADR-0013, supervisor pid=<n>)
```

Local-mode characteristics:

| | |
|---|---|
| `bind` | `127.0.0.1` |
| Mode | Local |
| Auth | token bootstrap (add an optional password — see §4) |
| TLS | not required |
| `[cloud]` block | not required |
| External access | not possible |

`Ctrl-C` shuts the supervisor down cleanly (every child shell is
reaped).

Stop from another terminal:

```bash
gtmux stop --name local
```

---

## 3) Run mode B — Cloud (`bind = 0.0.0.0`, with config file)

Use this when a second device (laptop, phone, another box) on a trusted
network needs to reach the server.

### 3.1 Write a config file

A ready-made template lives at `codebase/config.cloud.sample.toml`. Copy
it and replace the `PUBLIC_HOST` placeholders with your server's IP or
domain (also swap `9001` if you change the port).

```bash
mkdir -p ~/.config/gtmux
mkdir -p ~/.local/state/gtmux && chmod 700 ~/.local/state/gtmux

cp codebase/config.cloud.sample.toml ~/.config/gtmux/prod.config.toml
$EDITOR    ~/.config/gtmux/prod.config.toml
```

Key fields:

| Key | Value | Why |
|---|---|---|
| `[server].session` | `"prod"` | Server Instance name — must match `--name` |
| `[server].port` | `9001` | Listen port |
| `[server].bind` | `"0.0.0.0"` | Listen on every interface → cloud mode auto-enables |
| `[security].cors_origins` | `["https://PUBLIC_HOST"]` | Exact match, no wildcards |
| `[security].host_allowlist` | `["PUBLIC_HOST"]` | DNS-rebind defence |
| `[cloud].rate_limit_auth_failures_per_minute` | `10` | Required `[cloud]` key — no default; per-minute auth-failure ceiling |
| `[cloud].tls_required` | `true` | Default. gtmux terminates TLS (supply `tls_cert`/`tls_key`). Set `false` only when a reverse proxy fronts HTTPS for trusted-network validation. |
| `[cloud].trusted_proxy_ips` | `["10.0.0.2/32"]` | Reverse-proxy IP/CIDR whose `X-Forwarded-For` is trusted for per-client rate limiting (proxy mode). See note below. |
| `[assets].max_size_bytes` | `52428800` | 50 MiB per uploaded asset (image / document) |

> **Server Instance vs. session.** A **Server Instance** is one running
> gtmux server, named by `--name` (and `[server].session` in the TOML —
> the key keeps its old name but must match `--name`). A **session** is a
> saved workspace/layout record you switch between inside the UI. They are
> different concepts — one Server Instance holds many sessions.

Every key is documented inline in the sample.

> **Trusted proxy / `X-Forwarded-For`.** When gtmux sits behind a reverse
> proxy, set `[cloud].trusted_proxy_ips` to the proxy's IP/CIDR so the
> auth rate limiter keys on the real client IP. gtmux trusts `X-Forwarded-For`
> **only** when the request's socket peer IP matches that list (spoofed
> XFF is ignored). If it is unset/empty, XFF is ignored entirely and every
> client behind the proxy shares **one** rate-limit bucket (one user's
> failed logins can throttle everyone) — gtmux prints a stderr warning at
> boot unless you set `trusted_proxy_ips_required = false`. Only
> `X-Forwarded-For` is consulted (`-Proto`/`-Host` are not). Local mode is
> unaffected. Full detail: `docs/deploy.md` §3.6.3.

### 3.2 Start the server

```bash
cd codebase

GTMUX_FRONTEND_DIST="$PWD/frontend/dist" \
gtmux start \
  --name prod \
  --config ~/.config/gtmux/prod.config.toml
```

Open the host firewall if any (`sudo ufw allow 9001/tcp`, etc.). The
banner still shows `http://127.0.0.1:9001/auth/bootstrap?token=...` —
replace the host part with your `PUBLIC_HOST` to open it from another
device:

```text
http://PUBLIC_HOST:9001/auth/bootstrap?token=<hex>
```

---

## 4) Auth — get the cookie

Both modes use the same flow.

1. Open the **bootstrap URL** (the magic link) from the banner once in
   your browser.
2. The server validates the token and issues the `gtmux_auth` cookie
   (`HttpOnly` + `SameSite=Strict` + `Path=/`, `Secure` on Cloud/HTTPS,
   7-day rolling renewal).
3. After that, bookmark the path-only URL — `http://127.0.0.1:9001/` in
   Local mode, `http://PUBLIC_HOST:9001/` in Cloud mode. The cookie keeps
   you signed in until you sign out or it expires.

### Adding a password (optional)

The **token** is always valid — the magic link above works on every
`gtmux start`. You may **additionally** set a **password**; both
credentials stay valid at once, so you can log in with either. There is
**no auth "mode" to choose** — `[auth].mode` is deprecated and ignored,
and adding a password needs **no config edit and no restart**.

Set a password either way:

```bash
gtmux set-password           # prompts twice, writes an Argon2id hash
                              # → ~/.local/state/gtmux/password.argon2 (0600)
                              # active from the next `gtmux start`
```

or in the UI — **Settings → Auth** — which applies it live, no restart
(5 attempts / 5 minutes throttle on the password login form).

**Remove the password** (back to token-only — this is the lost-password
recovery path):

```bash
gtmux reset-password         # deletes the hash file → token-only login
```

or **Settings → Auth → "Delete password"** (re-authenticate with the
token or the current password first → token-only).

**Rotate a leaked token.** **Settings → Auth → "Rotate token"** reissues
the server token, signs out **every** session (all active tabs
disconnect), and shows a fresh login link; it asks you to re-enter your
current credential first. The password is left unchanged. The CLI
equivalent (for a stopped server) is:

```bash
gtmux rotate-token --name prod   # offline reissue; local mode reissues
                                 # on every start anyway
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
nohup gtmux start --name local \
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
gtmux stop --name <name>            # SIGTERM, 5 s grace
gtmux stop --name <name> --force    # then SIGKILL
```

This honours the pidfile and reaps every child shell. Killing the shell
job directly leaves orphaned PTYs.

---

## 7) Lifecycle reference

```bash
gtmux status                            # all known instances + liveness
gtmux status   --name prod              # one Server Instance
gtmux stop     --name prod [--force]    # graceful / forced shutdown
gtmux teardown --name prod --force      # 5-step cleanup
                                        # (socket / token / layout / pidfile / config)
                                        # opt-out parts with --keep-state / --keep-config
gtmux set-password / reset-password     # add / remove the optional password credential
gtmux rotate-token --name prod          # reissue the server token (cloud / offline)
```

(`--session` is still accepted as a deprecated alias for `--name`; it
prints a deprecation warning.)

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
| `Address already in use (os error 48)` | Port taken | `gtmux status` → `gtmux stop --name <name>` |
| `pidfile exists but process is gone` | Previous crash | `gtmux teardown --name <name> --force --keep-state` |
| Bootstrap URL says “Forbidden” on re-open | Cookie expired / cleared | Re-open the most recent banner URL |
| `gtmux` refuses to start | Running as root (`EUID == 0`) | Switch to a regular user |

## Next

- [`USAGE.md`](USAGE.md) — main canvas walkthrough (session management,
  architecture, every toolbar tool, Group feature).
- [`README.md`](README.md) — project overview and document index.
