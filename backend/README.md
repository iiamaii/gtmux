# gtmux backend

> **English** · [한국어](README.ko.md)

Rust Cargo workspace — the supervisor process behind gtmux. Holds the
HTTP API, the WebSocket protocol, the PTY backend, config + auth layers,
and the `gtmux` CLI binary.

Five library crates + two binaries (ADR-0011 D10):

```
crates/
  ws-server      WebSocket frame routing + per-cookie session lock.
  http-api       axum router — REST surface (sessions, terminals, file_open, …).
  config         figment-based loader (CLI > env > TOML > defaults).
  auth           Token + Argon2id password modes (ADR-0003 + ADR-0020).
  pty-backend    Direct PTY supervision (ADR-0013, replaces tmux).
bin/
  gtmux-cli      The `gtmux` user-facing CLI.
  gen-openapi    Emits shared/openapi.yaml from utoipa derives.
```

Toolchain pinned to **Rust 1.85** via `rust-toolchain.toml` (clap·ring·
rustls·config·rand common floor; see R7 §2).

---

## Build

```bash
cargo build --workspace             # debug
cargo build --workspace --release   # release — `gtmux` ends up in target/release/
```

## Test

```bash
cargo test --workspace
```

## Run from source (dev)

```bash
cargo run -p gtmux-cli -- start --session dev
```

`-p gtmux-cli` selects the binary; everything after `--` is forwarded to
`gtmux`. See the root `../README.md` for full CLI / config reference.

## Cross-compile

`cargo-zigbuild` (R7 §8 D9). Pre-pinned targets:

```bash
cargo zigbuild --target aarch64-apple-darwin       --release
cargo zigbuild --target x86_64-apple-darwin        --release
cargo zigbuild --target aarch64-unknown-linux-gnu  --release
cargo zigbuild --target x86_64-unknown-linux-gnu   --release
```

## Codegen producer

`bin/gen-openapi` emits the OpenAPI 3.1 document consumed by the
frontend's `openapi-typescript` pipeline:

```bash
cargo run -p gen-openapi -- ../shared/openapi.yaml
```

The top-level `make codegen` wraps this and chains the TS generation.
Never hand-edit `../shared/openapi.yaml`.

## Layout invariants

- **`ws-server` owns the WS protocol; `http-api` owns REST.** Cross-talk
  only through the shared `AppState`.
- **`pty-backend` is the *only* spawner of child processes.** Other
  crates request via the trait.
- **`config` is read-only at runtime** — boot loads it once.
- **`auth` never logs token material.** All `Debug` impls redact.

For deeper rationale see `../../docs/adr/` (0001~0030+).
