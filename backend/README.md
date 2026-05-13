# gtmux backend (Rust workspace)

본 디렉터리는 gtmux 백엔드의 Cargo workspace 루트다. ADR-0011 + R7 보고서가
정한 7-crate 구성(`mux-router` / `ws-server` / `http-api` / `lifecycle` /
`config` / `auth` / `gtmux-cli`)을 그대로 미러한다. C3 작업(`bin/gen-openapi`)도
워크스페이스 멤버로 포함된다.

## 빌드 / 테스트 / 교차 컴파일

빌드 (전체 워크스페이스): `cargo build --workspace`

테스트 (전체 워크스페이스): `cargo test --workspace`

교차 컴파일 (R7 §8 D9, cargo-zigbuild 사용): `cargo zigbuild --target <target> --release` (예: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`)

Rust toolchain은 `rust-toolchain.toml`이 1.85로 pin한다 (R7 §2 — clap·ring·rustls·config의 공통 floor).
