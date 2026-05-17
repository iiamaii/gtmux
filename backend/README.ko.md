# gtmux backend

> [English](README.md) · **한국어**

Rust Cargo workspace — gtmux 의 supervisor 프로세스. HTTP API, WebSocket
프로토콜, PTY backend, config + auth 레이어, `gtmux` CLI binary 보유.

5 library crate + 2 binary (ADR-0011 D10):

```
crates/
  ws-server      WebSocket frame 라우팅 + per-cookie session lock.
  http-api       axum router — REST 표면 (sessions, terminals, file_open, …).
  config         figment 기반 로더 (CLI > env > TOML > defaults).
  auth           Token + Argon2id password 모드 (ADR-0003 + ADR-0020).
  pty-backend    PTY 직접 supervision (ADR-0013, tmux 대체).
bin/
  gtmux-cli      사용자 facing `gtmux` CLI.
  gen-openapi    utoipa derive 로부터 shared/openapi.yaml 발행.
```

Toolchain 은 `rust-toolchain.toml` 로 **Rust 1.85** pin (clap·ring·
rustls·config·rand 공통 floor, R7 §2).

---

## 빌드

```bash
cargo build --workspace             # debug
cargo build --workspace --release   # release — `gtmux` 가 target/release/ 에 생성
```

## 테스트

```bash
cargo test --workspace
```

## 소스에서 실행 (dev)

```bash
cargo run -p gtmux-cli -- start --session dev
```

`-p gtmux-cli` 가 binary 선택, `--` 이후는 `gtmux` 로 그대로 전달.
CLI / config 전체 레퍼런스는 root `../README.md`.

## 교차 컴파일

`cargo-zigbuild` (R7 §8 D9). Pre-pin 타겟:

```bash
cargo zigbuild --target aarch64-apple-darwin       --release
cargo zigbuild --target x86_64-apple-darwin        --release
cargo zigbuild --target aarch64-unknown-linux-gnu  --release
cargo zigbuild --target x86_64-unknown-linux-gnu   --release
```

## Codegen producer

`bin/gen-openapi` 가 OpenAPI 3.1 문서를 발행 — frontend 의
`openapi-typescript` 파이프라인이 consume:

```bash
cargo run -p gen-openapi -- ../shared/openapi.yaml
```

최상위 `make codegen` 이 이를 감싸고 TS 생성까지 chain. `../shared/openapi.yaml` 손대지 말 것.

## Layout 불변

- **`ws-server` 는 WS 프로토콜 owner, `http-api` 는 REST owner.**
  Cross-talk 는 공유 `AppState` 통해서만.
- **`pty-backend` 가 child process 의 *유일* spawner.** 다른 crate 는
  trait 으로 요청.
- **`config` 는 runtime 에 read-only** — boot 시 1회 로드.
- **`auth` 는 token material 을 절대 log 하지 않음.** 모든 `Debug` impl
  이 redact.

더 깊은 근거는 `../../docs/adr/` (0001~0030+).
