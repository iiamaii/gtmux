# ADR-0011: Backend stack = Rust + axum + tokio

- 상태: Accepted (2026-05-14, R7 보고서로 Open O1~O7 모두 closed, A4 게이트 통과)
- 일자: 2026-05-13 (Proposed) / 2026-05-14 (Accepted)
- 결정자: backend-architect (grill D18 산출, /grill-with-docs 세션)
- 근거 보고서: `docs/reports/0010-grill-amendments.md` §1 D18 (tech stack), D19 (performance budget)
- 관련 ADR:
  - ADR-0009 (tmux daemon 격리) — control mode 통신 주체로서 Rust 프로세스가 dedicated daemon에 attach
  - ADR-0012 (frontend stack) — wire-protocol·schema 공유 패턴 정합
  - ADR-0001 (예정, tmux 통합) — 본 ADR이 잠근 crate set을 입력 제약으로 사용
  - ADR-0002 (예정, 전송 계층) — `axum` + `tokio-tungstenite` 기반 구현 가정
  - ADR-0003 (예정, 보안 디폴트) — `tower-http` 미들웨어 + 정적 타입 allowlist 강제

## 맥락

본 ADR은 gtmux backend의 기반 언어·async runtime·HTTP/WS framework·핵심 보조 crate를 잠근다. `docs/sketch.md` §10.1 (백엔드 구성)이 요구하는 8개 구성요소 — tmux control mode client / WebSocket server / HTTP API server / tmux command router / state collector / WS notify dispatcher / lifecycle manager / auth manager — 의 구현 기반을 단일 stack으로 통일하기 위함이다.

사용자가 grill 세션에서 두 가지 1순위 제약을 명시했다.
- **AI agent로 구현 진행** — dev velocity 페널티는 *예측 가능한 wall-clock 비용*이며 코드 품질 손실은 동반하지 않는다.
- **성능 우선** — binary throughput, predictable latency, 메모리 footprint 모두 우선순위 상위.

도메인 제약 (grill D1~D17 누적):
- tmux control mode stdin/stdout 라인 파싱 + 백프레셔 처리 (D15·D16) — GC pause는 streaming 단절 위험.
- per-pane 128 KB ring buffer × N panes 메모리 관리 (D15) — zero-copy 처리가 자연.
- WebSocket binary frame envelope (D14) + HTTP API + ETag (D12).
- 다수 Server 동시 실행 (D2) — Server당 메모리 baseline이 곱셈으로 누적.
- CSPRNG + 상수시간 비교 + Origin/Host/CSRF 미들웨어 (D17).
- 단일 정적 바이너리 배포 (D20 CLI: `start`/`stop`/`teardown`/`rotate-token`/`status`).
- 5대 불변식 중 #4 (보안 디폴트) — argv 분리 + tmux 명령 allowlist를 가능한 한 컴파일 타임에 강제.

후보 비교 단계는 D18에서 종결되었다. 본 ADR은 *선택을* 잠그고, R7 보고서가 *specific crate version + benchmark + scaffolding* 을 산출하여 Accepted 승격으로 이끈다.

## 결정 (Decisions)

- **D1. 기반 언어 = Rust** (edition 2021 이상, stable 채널). MSRV 확정은 R7-T1 검증 후.
- **D2. async runtime = `tokio`** — multi-thread scheduler, `tokio::sync` 채널, `tokio::process::Command`로 tmux daemon spawn (셸 미경유, argv 직접 전달).
- **D3. HTTP framework = `axum`** + 미들웨어 = **`tower-http`** (`CorsLayer`, `RequireAuthorizationLayer`/커스텀 Bearer extractor, `TraceLayer`, `CompressionLayer`, `SetRequestIdLayer`/`PropagateRequestIdLayer`). ETag 핸들링은 핸들러 레벨 (RFC 7232 강한 비교, `If-Match`/412 Precondition Failed).
- **D4. WebSocket = `tokio-tungstenite`** + `axum::extract::ws` 통합. binary frame 네이티브, handshake `Sec-WebSocket-Protocol` subprotocol에서 토큰 추출 후 `tower-http`와 동일 검증 경로 호출.
- **D5. Serialization** = **`serde` + `serde_json`** (HTTP 페이로드) + **`bytes::Bytes`/`BytesMut`** 기반 직접 binary 인코더/디코더 (WS envelope: `[1B type][varint paneId|0][payload]`). JSON Schema 자동 생성은 **`utoipa`** (OpenAPI 우선) 또는 **`schemars`** (R7-T6 검증).
- **D6. CLI = `clap`** (derive macro, subcommand: `start`, `stop`, `teardown`, `rotate-token`, `status`). D20의 exit code 표 그대로. port 기반 lookup (D21 c6)도 동일 crate 안에서 구현.
- **D7. Logging/Tracing = `tracing`** + **`tracing-subscriber`** (`fmt` 레이어: tty=ANSI banner, pipe=JSON, `--log-format json` 강제 가능). 토큰·stdin payload·`Authorization` 헤더는 미들웨어/`Span` 필드 sanitizer에서 `***REDACTED***`로 마스킹 (D17 정합).
- **D8. Crypto = `ring`** 1순위 (CSPRNG = `ring::rand::SystemRandom`, 상수시간 비교 = `ring::constant_time::verify_slices_are_equal`). 대안 `rustls`+`rand` + `subtle::ConstantTimeEq`는 R7-T2에서 라이선스·플랫폼 호환성 비교 후 확정.
- **D9. 빌드·배포 = `cargo zigbuild`** (또는 `cargo dist`) 기반 cross-compile로 macOS aarch64/x86_64 + Linux x86_64/aarch64 단일 정적 바이너리 생성. `rustls`는 OpenSSL 동적 링크를 회피해 정적 빌드를 자연스럽게 한다.
- **D10. 모듈 분리 (Cargo workspace)** — 다음 crate 경계로 §10.1과 1:1 매핑하며, 5대 불변식 중 #1·#2의 코드 경계를 컴파일 단위로 강제한다.
  - `mux-router` — tmux control mode 파서 + tmux 측 명령 router (argv 전용). tmux state 전담.
  - `ws-server` — `tokio-tungstenite` 통합, envelope 인코딩/디코딩, MT-3 broadcast.
  - `http-api` — `axum` 라우터, `GET/PUT /api/layout` + ETag, durable 영속화 게이트웨이.
  - `lifecycle` — dedicated tmux daemon spawn/teardown, 소켓 cleanup, 디렉터리 컨벤션.
  - `config` — TOML + `figment`(또는 `config`) 로딩, env var 오버레이, schema validation.
  - `auth` — 토큰 발급/검증/회전, 상수시간 비교, redaction.
  - `gtmux-cli` (bin) — `clap` entrypoint, 위 crate 조합.
  - 구체 모듈 outline은 R7-T7에서 확정.

## 거절된 대안 (Rejected)

| # | 후보 | 거절 사유 | 근거 |
|---|---|---|---|
| R1 | **Bun (TypeScript)** | 메모리 footprint 2–3x. 50 Server 동시 시나리오에서 baseline 1.5–2.5 GB 추가 비용. AI-generated 1.5–3x 비효율을 흡수할 headroom 부족. | 보고서 D18 표 |
| R2 | **Node.js (TypeScript)** | Bun이 메모리·throughput·단일 바이너리 배포(`bun build --compile`) 모두 dominant. | 보고서 D18 |
| R3 | **Go** | 단일 바이너리 + 적당한 성능은 OK이나 프론트엔드(TS)와 *언어 분리* 발생 → wire-protocol/schema 공유 비용. Rust + `utoipa`/`schemars` → TS 타입 자동 생성 패턴이 우월. GC pause도 Rust 대비 비결정적. | 보고서 D18 |
| R4 | **Python (FastAPI)** | binary throughput 약함, 단일 바이너리 배포 비표준, GIL 제약. tmux control mode 라인 파싱의 hot path에 부적합. | 보고서 D18 |
| R5 | **Deno (TypeScript)** | Bun과 유사하나 단일 바이너리·WS 라이브러리 성숙도에서 약간 뒤. Bun 대비 추가 이점 없음. | 보고서 D18 |

## 결과 (Consequences)

- **긍정**
  - 메모리 footprint baseline ≈ 10–30 MB/Server (vs Bun 40–80 MB). 50 Server 동시 시 약 1.5 GB 절감.
  - GC pause 없음 → predictable streaming latency. tmux output burst 상황에서도 p99 < 100ms 예산 (D19) 달성 헤드룸 확보.
  - Zero-copy binary 처리 (`Bytes`/`BytesMut`) — WS frame·ring buffer 모두 동일 abstraction.
  - 정적 타입이 §13.3.3 명령 주입 방어를 컴파일 타임에 검증 (allowlist enum + argv 배열 타입 분리).
  - 단일 정적 바이너리 배포 자연 (`cargo build --release` → 단일 파일).
  - Cargo workspace 모듈 분리 (D10)가 §10.1 8개 구성요소와 1:1 매핑되어 코드 경계 = 도메인 경계.
- **부정/비용**
  - AI agent의 Rust iteration 비용 1.5–2x (LLM의 TS 학습 데이터 우위). axum/tokio/tungstenite는 광범위 학습 데이터로 *해결 가능 범위*.
  - 컴파일 시간 (clean 30s~2min). `cargo-watch` + `sccache` + workspace incremental로 1–5s까지 완화.
  - tmux control mode 파서는 어차피 직접 구현 (R1 보고서) — 언어 선택과 무관한 고정 비용.
- **후속 작업**
  - **R7 보고서** (`docs/reports/0007-backend-runtime.md`) — 본 ADR의 미해결 항목 (O1~O7) 전부 결정 + D19 MVP 컬럼 측정 + scaffolding 산출. R7 완료 시 본 ADR Proposed → **Accepted** 승격.
  - **ADR-0001 (tmux 통합)** — 본 ADR의 `mux-router` 모듈 경계와 tmux command allowlist (ADR-0008)를 Rust enum + 컴파일 타임 검사로 표현하도록 구현 제약 인계.
  - **ADR-0002 (전송 계층)** — D4 `tokio-tungstenite` + D5 envelope 인코더 구현 가정.
  - **ADR-0003 (보안 디폴트)** — D7 redaction + D8 crypto crate + `tower-http` 미들웨어 체인을 구현 입력으로 사용.
  - **ADR-0012 (frontend)** — wire-protocol/schema 공유는 `utoipa`/`schemars` 산출물을 TS 타입으로 변환하는 빌드 단계로 합의.

## 불변식 검증

| # | 불변식 | 검증 |
|---|--------|------|
| 1 | tmux 상태/웹 상태 분리 | **PASS** — Cargo workspace 분리(D10)가 `mux-router`(tmux state) ↔ `http-api`+`ws-server`(web state) 코드 경계를 컴파일 단위로 강제. 두 도메인 간 데이터 흐름은 명시적 채널 type을 통해서만 가능. |
| 2 | tmux-native vs web-only 분기 | **PASS** — 모듈 분리가 features를 crate 경계에 매핑. 잘못된 호출은 dependency graph에서 차단됨 (`http-api` → `mux-router`는 router의 public command API만 호출 가능, 역방향 불가). |
| 3 | tmux Layout ≠ Canvas Layout | **PASS** — `mux-router`는 tmux 레이아웃 문자열을 *불투명 type*으로만 다루며 외부 노출 안 함. Canvas Layout schema는 `http-api`/`ws-server` 쪽 별도 type. 두 type 사이 변환 함수가 존재하지 않으므로 compile-time impossible. |
| 4 | 보안 기본값 | **PASS (강함)** — (a) tmux 명령 발급 경로는 `mux-router::Command` enum으로만 표현되며 allowlist 외 변종은 enum variant 자체가 존재하지 않음 → 컴파일 타임 보장. (b) argv는 `Vec<OsString>` 타입으로 분리, 셸 string concatenation 경로 없음 (`tokio::process::Command::arg`만 사용). (c) `tower-http` 표준 미들웨어가 Origin/Host/CSRF/Authorization 검증을 일관 적용. (d) `ring::constant_time` 상수시간 비교, `tracing` redact가 토큰 누출 차단. |
| 5 | control mode 사용 | **PASS** — `lifecycle` crate가 `tmux -L gtmux-<session> -C` 프로세스 spawn 단일 책임 (ADR-0009). `mux-router`가 그 stdin/stdout에 attach. 셸 호출 경로 없음 (`tokio::process::Command::new("tmux").arg(...)` 형태 — argv 직접). 스크린 스크레이핑/반복 shell-out 경로 없음. |

## 미해결 항목 (Open) — R7 보고서에서 결정

각 항목은 R7 verification task로 풀어진다. R7 DoD = 모든 항목 closed + D19 MVP 컬럼 측정 시나리오 코드.

- **O1. Rust MSRV 확정.** Target = stable 채널 1.80+. R7-T1 = 본 ADR이 잠근 crate set (D2~D8) 전부와 호환되는 최소 minor version 1개 식별 + `rust-toolchain.toml` pin 권장 여부 결정. **측정**: 후보 minor마다 `cargo build --release` workspace 빌드 통과 여부.
- **O2. Crypto crate 최종.** `ring` vs `rustls` + `rand` + `subtle`. R7-T2 = 라이선스 (`ring`은 BoringSSL 파생, RustCrypto 진영은 dual MIT/Apache) + 플랫폼 호환성 (macOS aarch64, Linux x86_64/aarch64 정적 빌드) + binary size 영향 비교 표. **측정**: D9 cross-compile 결과 binary size 차이 < 500 KB 허용 폭 안인지.
- **O3. tmux control mode parser 구현 패턴.** state machine 직접 구현 vs `nom` 콤비네이터 vs `winnow`. R7-T3 = 50-pane 워크로드 (5 고출력 + 45 idle, D19 측정 환경) 벤치마크. **측정**: D19 행 "Per-pane output latency p50 < 30ms / p99 < 100ms", "Server backend memory baseline < 30 MB".
- **O4. WebSocket subprotocol 토큰 검증 hook 위치.** `tokio-tungstenite::accept_hdr` callback vs `axum` extractor vs `tower` 레이어. R7-T4 = D17 정합 (상수시간 비교 + redaction + Origin/Host 통합) + Sec-WebSocket-Protocol echo 규약 검증. **측정**: 단위 테스트로 잘못된 토큰/Origin 4종 변종 모두 reject + 정상 1종 accept.
- **O5. JSON Schema → TS 타입 자동 생성 도구.** `utoipa`(OpenAPI 우선) vs `schemars`(JSON Schema 우선) vs 둘 다. R7-T5 = `http-api`의 `GET/PUT /api/layout` 페이로드(ADR-0010 G-hybrid schema)에 적용했을 때 출력 TS의 정확성 + Svelte 5 (ADR-0012)에서의 사용성. **측정**: ADR-0010 SSoT `canvas-layout-schema.md` 100% 커버 + 빌드 시간 영향 < 5s.
- **O6. Config 로더.** `figment` vs `config`. R7-T6 = D22 스키마(`schema_version`/`server`/`runtime`/`security`/`cloud`) + 선행순위 (CLI flag > env > file > default) + 알 수 없는 필드 거부 (오타 방지). **측정**: D22 예제 TOML이 모든 필드에서 round-trip 가능 + 1개 임의 오타가 startup 실패로 이어짐.
- **O7. Cargo workspace 모듈 outline 확정.** D10의 7개 crate(라이브러리 6 + 바이너리 1)의 내부 모듈 경계와 의존 그래프. R7-T7 = scaffolding 산출 (`Cargo.toml` workspace + 각 crate의 빈 lib.rs/main.rs + dependency graph). **측정**: `cargo check --workspace` 통과 + dependency graph DAG (cycle 없음) + 역방향 의존(`mux-router` → `http-api`) 차단 lint.

추가 R7 측정 항목 (D19 MVP 컬럼 직접 검증):
- Cold start < 500ms, Warm reconnect < 300ms (D19) — R7 측정 plan.
- Per-pane output latency p50 < 30ms / p99 < 100ms (D19) — R7-T3 벤치마크에 흡수.
- Panel drag commit → sync < 500ms (D19) — R7-T4·T5 통합 측정.
- Server backend memory baseline < 30 MB, Per-Server total < 50 MB (D19) — R7-T3 워크로드에서 실측.
- gtmux→브라우저 WS write lag < 5s (D19) — `refresh-client -A` pause/continue 임계값 튜닝 결과로 인계 (ADR-0001).
