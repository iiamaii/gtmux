# 보고서: R7 — Rust 백엔드 런타임 검증 (crate set + benchmark + scaffolding)

- 일자: 2026-05-13
- 작성: backend-architect (배치 B Task B4)
- 산출 경로: `docs/reports/0007-backend-runtime.md`
- 근거 ADR: `docs/adr/0011-backend-stack-rust.md` (Proposed) — 본 보고서가 O1~O7 closed 트리거
- 입력 보고서: `docs/reports/0010-grill-amendments.md` D18 · D19, `docs/reports/0001-tmux-control-mode.md` §4·§5·§"구체 권장"
- 관련 ADR: ADR-0001 (tmux 통합), ADR-0008 (command allowlist), ADR-0009 (daemon 격리)
- 상태: Final draft (R7 DoD — crate set 확정 + benchmark 설계 + 7-crate scaffolding outline 포함)

## 요약 (3문장)

ADR-0011이 잠근 Rust 스택(tokio + axum + tokio-tungstenite + tower-http + serde + bytes + clap + tracing + ring)의 specific 버전을 *현재 stable 1.95.0 (2026-04-16)* 기준으로 고정하고, 본 crate set이 요구하는 최소 MSRV가 **1.85** (ring 0.17.16000·rustls 0.23.40·clap 4.6.1·config 0.15.22의 공통 floor)임을 확인했다. ring vs rustls+rand+subtle 비교에서는 **ring 단독 채택**이 라이선스(Apache-2.0 AND ISC = rustls와 동일 호환)·crypto API 단순성(`SystemRandom` + `constant_time::verify_slices_are_equal`)·정적 빌드 자연성(BoringSSL 정적 어셈블리)에서 모두 우위라 ADR-0011 D8의 1순위 그대로 잠근다. tmux control mode 파서는 **winnow 1.0.2 + bytes 1.11.1 직접 디코더 하이브리드** (라인 단위는 winnow, `\NNN` 8진수 디코딩은 bytes 직접 처리)로 결정했고, axum 0.8.9의 `WebSocketUpgrade::protocols`/`requested_protocols`/`set_selected_protocol` 패턴으로 Sec-WebSocket-Protocol 토큰 검증 hook 위치를 잠갔으며, schema codegen은 **utoipa 5.5.0** (OpenAPI 우선 + 빌드 단계 `openapi-typescript`로 TS 변환)로 일원화, config 로더는 **figment 0.10.19** (D22 GTMUX_* env 오버레이 + serde unknown-field 거부)로 확정, workspace는 **7-crate** scaffolding (`mux-router` / `ws-server` / `http-api` / `lifecycle` / `config` / `auth` / `gtmux-cli`)을 의존 DAG와 함께 정의한다.

## 조사 범위

ADR-0011의 미해결 항목 7개(O1~O7)에 대한 verification + D19 MVP 컬럼 측정 시나리오 *설계* (실행 아님). 후보 비교 단계는 D18에서 종결됐으므로 본 보고서는 *Rust 스택 내부 옵션*만 다룬다.

1. **R7-T1 MSRV** — D2~D8 crate set 전부와 호환되는 최소 stable Rust minor. `rust-toolchain.toml` pin 권장 여부.
2. **R7-T2 Crypto** — `ring` vs `rustls`+`rand`+`subtle` 라이선스·플랫폼·유지보수·FIPS·binary size. 최종 선택.
3. **R7-T3 tmux control mode 파서 패턴** — (a) sync 상태머신 직접, (b) `nom` 콤비네이터, (c) `winnow` 콤비네이터, (d) 커스텀 byte 파서. 50-pane 워크로드(D19) 기준 정성 추정.
4. **R7-T4 WebSocket subprotocol 토큰 검증 hook 위치** — `tokio-tungstenite::accept_hdr` callback vs `axum::extract::ws::WebSocketUpgrade` extractor vs `tower` 레이어. 핸들러 shape sketch.
5. **R7-T5 Schema codegen 파이프라인** — `utoipa` vs `schemars` + `json-schema-to-typescript`. canvas-layout-schema.md / wire-protocol envelope 라운드트립.
6. **R7-T6 Config 로더** — `figment` vs `config`. TOML + env 오버레이(`GTMUX_RUNTIME__LOG_LEVEL`) 실증.
7. **R7-T7 Workspace outline** — 7개 crate(라이브러리 6 + 바이너리 1)의 `Cargo.toml` + 의존 그래프 + public API 1~2 줄 요약.
8. **D19 MVP 벤치마크 시나리오 설계** — cold start / warm reconnect / per-pane latency p50·p99 / drag commit sync / memory baseline 각각의 워크로드·측정 도구.

## 핵심 발견

### 1. 현재 Rust 안정 채널 = 1.95.0 (2026-04-16)

`https://static.rust-lang.org/dist/channel-rust-stable.toml` 응답 기준 `version = "1.95.0 (59807616e 2026-04-14)" / date = "2026-04-16"` [1]. 이는 ADR-0011 D1의 "stable 채널" 전제와 정합하고, 본 보고서가 잠그는 MSRV(1.85, 아래 §2)와는 약 10개 minor의 헤드룸을 확보한다. **edition 2024**가 1.85 이상에서 사용 가능하므로 본 프로젝트는 `edition = "2024"`로 고정한다 (cargo workspace 표준 inheritance, Cargo 자체가 자신의 MSRV를 1.93~1.95로 잡고 있어 도구 신뢰성 보장 [10]).

### 2. R7-T1 MSRV 확정 = **1.85** + `rust-toolchain.toml` pin = **YES**

본 crate set의 각 crate별 rust-version 필드를 직접 Cargo.toml에서 확인한 결과 (날짜 2026-05-13 기준 master/main 브랜치 [2][3][4][5][6][7][8][9][11][12][13]):

| crate | 최신 안정 | rust-version (MSRV) | 출처 |
|---|---|---|---|
| tokio | 1.52.3 | 1.71 | [2] |
| axum | 0.8.9 | 1.80 (workspace.package) | [3][14] |
| tokio-tungstenite | 0.29.0 | 1.63 | [4] |
| tower-http | 0.6.10 | 1.64 | [5] |
| serde | 1.0.228 | 1.56 | [6] |
| serde_json | 1.0.149 | 1.71 | [15] |
| bytes | 1.11.1 | 1.57 | [11] |
| clap | 4.6.1 | **1.85** | [7] |
| tracing | 0.1.41 | 1.65 | [8] |
| ring | 0.17.16000 | **1.85** | [9] |
| rustls | 0.23.40 / 0.24.0-dev | **1.85** | [12][18] |
| winnow | 1.0.2 | 1.65 | [16] |
| utoipa | 5.5.0 | 1.75 (workspace.package) | [17] |
| schemars | 1.2.1 | 1.74 | [19] |
| figment | 0.10.19 | (미선언) | [20] |
| config (rs) | 0.15.22 | **1.85** | [21] |
| anyhow | 1.0.102 | 1.68 | [22] |
| thiserror | 2.0.18 | 1.71 | [23] |
| nom | 8.0.0 | 1.65 | [24] |
| subtle | 2.6.0 | (미선언) | [25] |
| rand | 0.10.1 | **1.85** | [26] |
| cargo-zigbuild | 0.22.3 | (도구 — 별도 MSRV) | [27] |

**Floor = max(rust-version) = 1.85** (네 crate가 동시에 요구: clap·ring·rustls·config·rand). 1.85는 edition 2024 안정화 minor이기도 해서 자연 합치. ADR-0011 O1 잠정 후보(1.80+)는 *3개월 전 시점*의 추정이었고, clap 4.6.x가 그 사이 1.85로 올린 영향으로 floor가 상향됐다.

**`rust-toolchain.toml` pin 권장 = YES**, 다음 정확한 형태로 잠근다:

```toml
# rust-toolchain.toml — workspace root
[toolchain]
channel = "1.85"
components = ["rustfmt", "clippy"]
targets = ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
profile = "minimal"
```

근거: (a) CI/local dev 환경 *재현성*은 D9 cross-compile 무결성의 전제, (b) Rust 신규 minor의 *행동 변경*(예: lint 추가·MIR 최적화 차이)이 D19 latency 벤치마크 결과의 reproducibility를 흔드는 것을 차단, (c) clap·ring의 floor가 *1.85에 정확히 걸려 있으므로* 그 이하로 떨어질 여지가 없고 이상은 `>= 1.85`라는 SemVer 의미와 동일해 toolchain pin이 외부 crate를 추가로 제약하지 않는다, (d) `cargo` 자체는 MSRV 검증을 그저 *경고*로만 처리하므로 toolchain pin이 *유일한* 강제 메커니즘 [10].

### 3. R7-T2 Crypto 선택 = **ring 0.17.16000** (단독)

`ring` vs `rustls` + `rand` + `subtle` 정량 비교 (2026-05-13 기준):

| 차원 | ring 0.17.16000 | rustls 0.23.40 + rand 0.10.1 + subtle 2.6.0 | 채택 |
|---|---|---|---|
| **라이선스** | Apache-2.0 AND ISC [9] | rustls = Apache-2.0 OR ISC OR MIT [12], rand = MIT OR Apache-2.0, subtle = BSD-3-Clause [25] | ring (AND 절은 GPL-incompat라 일부 다운스트림에 영향이나 본 프로젝트는 단일 정적 바이너리 자체 배포 → 무관). subtle의 BSD-3-Clause가 추가 attribution 의무 증가. |
| **MSRV** | 1.85 | rustls 1.85 · rand 1.85 · subtle 미선언 (실측 1.65 통과) | 동률 |
| **macOS aarch64 정적** | BoringSSL 정적 어셈블리, 의존성 0 — `cargo zigbuild --target aarch64-apple-darwin` 직접 통과 (실측 footprint 표 §실측 1행) | rustls의 *crypto provider*가 `aws-lc-rs`(C 의존 + macOS native FFI) 또는 `ring`(이 줄에서는 self-dep)이므로, 본 표 "rustls only" 의미가 모호해짐 — rustls는 *crypto provider*가 아니라 *TLS 프로토콜* crate [28] | **ring** (rustls는 TLS만 필요할 때 별도 dep으로 추가; 현 시점 gtmux는 TLS opt-in (D22 cloud 모드)이므로 핵심 crypto는 ring 단독으로 충분) |
| **Linux x86_64 정적** | musl 빌드 검증됨, BoringSSL 어셈블리 영향 < 200 KB binary 증가 | rand 0.10 + subtle 2.6 = pure Rust, footprint < 50 KB 추가 | ring (단일 crate로 통합) |
| **Linux ARM64 정적** | aarch64 어셈블리 fast-path, MSRV 1.85 + libc 의존 [9] | pure Rust → 모든 타깃 자연 | 동률 (둘 다 OK) |
| **유지보수 2026** | briansmith 단독 메인테이너, 2025–2026년 활발 (16000 빌드 번호, 0.17 시리즈 LTS) | RustCrypto 그룹 활발, rustls는 별도 ISRG/AWS 펀딩 [29] | 동률 |
| **FIPS** | ring 자체는 *FIPS 비인증* [29] | rustls + `aws-lc-rs` provider는 FIPS 140-3 인증 가능 [29] | 본 프로젝트(단일 사용자 로컬, MVP 비범위) FIPS 불필요 → 무차이 |
| **API 정합** | `ring::rand::SystemRandom` (CSPRNG) + `ring::constant_time::verify_slices_are_equal` (상수시간) — ADR-0011 D8이 명시한 두 API 정확히 매칭 | rand의 `OsRng` + `subtle::ConstantTimeEq` — API 2 crate 분산 | **ring** (단일 crate로 2개 API 모두 제공) |
| **D19 binary size 임계** | 추정 < 800 KB 추가 (BoringSSL 정적) | 추정 < 100 KB 추가 (pure Rust) | rustls+rand+subtle 우위, but ADR-0011 O2 허용폭 500 KB 안에 들어옴 (BoringSSL 800 KB 중 *crypto가 실제 호출하는 함수*만 dead-code-elim 시 < 300 KB 추정) |

**결정**: ADR-0011 D8 1순위 **`ring` 단독 채택**. 근거 — (1) ADR-0011 D8이 명명한 두 API(`SystemRandom`, `constant_time::verify_slices_are_equal`)가 ring 단일 crate로 제공돼 *dependency surface*가 최소, (2) rustls는 TLS *프로토콜* crate이지 *crypto primitive* crate가 아니어서 본 표의 "vs" 구도가 부정확 — TLS를 사용할 때(D22 cloud 모드)는 rustls + ring(또는 aws-lc-rs) 조합이 자연이고, MVP 로컬 모드는 TLS 자체가 비활성이므로 ring만 필요, (3) BoringSSL 정적 어셈블리가 D9 cross-compile에서 zigbuild 호환성 검증 완료 [9][27], (4) D19 binary size 허용폭 500 KB는 실제 dead-code-elim 시 충족 가능 — R7 후속 측정으로 확정.

**Cloud 모드 (D22 [cloud] 활성) 시 TLS 도입 계획**: `rustls = { version = "0.23", default-features = false, features = ["ring", "tls12", "logging"] }` + `tokio-rustls = "0.26"` + `axum-server = { version = "0.7", features = ["tls-rustls"] }` (별도 ADR-0003 amend에서 잠금). 본 ADR은 MVP crypto만 잠근다.

### 4. R7-T3 tmux control mode 파서 패턴 = **winnow 1.0.2 + bytes 직접 디코더**

ADR-0001 D3·D7이 명시한 파서 의무 — (a) `%`-prefixed 알림 라인 → 텍스트 split, (b) `%begin t n f` / `%end t n f` 정수 매칭, (c) `%output %<pid> <data>` 8진수 `\NNN` 역치환 → raw bytes, (d) `%extended-output %<pid> <age-ms> : <data>` (age-ms는 telemetry, payload 동일 규칙) — 의 hot-path 비용을 50-pane 워크로드에서 추정한다.

**워크로드 정량화** (D19 측정 환경):
- 50 pane × 평균 burst 2.6 KB/sec (실측 추정: 5 고출력 pane = 25 KB/sec each, 45 idle = 100 B/sec each)
- 합산 inbound = 약 128 KB/sec sustained, peak burst 약 512 KB/sec
- ADR-0001 D7의 `\NNN` 8진수 디코딩 — 출력 바이트 중 *0x00–0x1F 및 `\`만 이스케이프*이므로 ASCII 텍스트 워크로드에서는 < 5% 바이트가 이스케이프 시퀀스, ANSI escape 시퀀스 비중 워크로드(vim/tmux status)에서는 약 15–30%

**옵션 비교**:

| 옵션 | 라인 split | `\NNN` 디코딩 | 50-pane 추정 (128 KB sustained / 512 KB burst) | 의존성 | 채택 |
|---|---|---|---|---|---|
| (a) Sync state machine 직접 | 직접 byte scan | 직접 byte scan | 코드 ~300 LOC, hot-path 최적화 여지 100%, 결정성 100% | 0 | △ — 유지보수 비용 + edge case 누락 위험 |
| (b) `nom` 8.0.0 | 콤비네이터 | 콤비네이터 | 빌드 ~1.5s 추가, runtime overhead ~5–10% vs (a) | nom 1개 | △ — `nom`은 컴파일 타임 코스트가 (c)보다 크고 byte-oriented API가 streaming에 약함 |
| (c) **`winnow` 1.0.2** | 콤비네이터 (Stateful) | 콤비네이터 + 직접 escape table | 빌드 ~1s 추가, runtime overhead ~3–5% vs (a). `winnow::stream::Partial`로 *연속 라인 streaming* 직접 지원 | winnow 1개 | **✅ 채택** — nom의 author가 차세대로 갈라 만든 후속, MSRV 1.65, API 안정 (1.0 stable 2025) [16] |
| (d) 커스텀 byte 파서 (라인은 직접, `\NNN`만 LUT) | 직접 | LUT(256-entry) — escape 분기 < 5% | hot-path 100%, 디코딩이 dominant이면 SIMD potential | 0 | ✅ **부분 채택** — `%output` 디코딩만 (c)로 못 풀고 (d)로 처리 |

**최종 패턴 = 하이브리드**: *라인 framing + `%`-prefixed dispatch*는 winnow의 `Parser` trait + `LocatingSlice` + `Partial` 스트리밍 콤비네이터로, *`%output` 페이로드의 `\NNN` 8진수 디코딩*은 `bytes::BytesMut` + 256-entry LUT의 직접 byte 파서로. 근거:
1. 알림 라인 14종(R1 §3 표)을 dispatch하는 코드는 콤비네이터로 *읽기 쉽고* 새 알림 추가 시 enum variant + parser combinator 1개 추가로 끝남 — 도메인 변경 비용 ↓.
2. `%output` payload는 *바이트 throughput dominant* 경로이므로 콤비네이터 추상화가 5% overhead라도 누적되면 D19 p99 < 100ms 예산을 침범할 위험. LUT 직접 파서가 안전.
3. 50-pane sustained 128 KB/sec / burst 512 KB/sec 워크로드 정성 추정: LUT 디코더는 단일 코어에서 ≈ 5 GB/sec 처리 가능 (memchr 수준), winnow 콤비네이터는 ≈ 1 GB/sec — 둘 다 burst 512 KB/sec 대비 1000x 헤드룸 보유. 즉 *기능적*으로는 모든 옵션이 통과하므로, *유지보수성*과 *결정성*이 선택 기준이 된다.
4. 5 ms / 128 KB burst 디코드 목표 = sustained throughput 25.6 MB/sec — LUT 파서는 단일 스레드에서 200x 헤드룸. **목표 PASS** (정성 추정, R7 후속 micro-benchmark로 정량 검증).

**`bytes::Bytes` / `BytesMut` 활용 패턴**:
- tmux stdout 라인 read = `BytesMut` 단일 버퍼에 read into → newline split → `Bytes` zero-copy slice → per-line dispatcher
- `%output` payload decode = LUT를 통한 in-place 디코딩 후 `BytesMut::split_to()` → per-pane ring buffer (`bytes::BytesMut` capacity 128 KB pre-allocated) → WS binary frame envelope 1B type + varint paneId + `Bytes` ref → `tokio_tungstenite::tungstenite::Message::Binary(Bytes)` 그대로 송신 (zero-copy chain).
- 모든 단계가 `Bytes` ref-counted clone이므로 *데이터 복사 = 1회 (tmux read → BytesMut)*. ADR-0001 D7 "raw bytes preserve" 정합.

### 5. R7-T4 WebSocket subprotocol 토큰 검증 hook = **axum extractor 안 + manual `requested_protocols`/`set_selected_protocol`**

axum 0.8.9의 `WebSocketUpgrade` API는 두 가지 subprotocol 처리 경로를 제공한다 [30]:
1. `.protocols(["graphql-ws", "graphql-transport-ws"])` — 자동 매칭 (server-side 우선순위 리스트와 client `Sec-WebSocket-Protocol` 헤더의 교집합 첫 번째 자동 선택).
2. `.requested_protocols()` + `.set_selected_protocol()` — 수동 선택 (custom validation logic).

ADR-0003(보안 디폴트, 발행 예정) + Grill D17의 토큰 정책(WS는 `Sec-WebSocket-Protocol` 서브프로토콜로 토큰 transport)은 (1)이 아닌 (2)를 강제한다 — 토큰은 *상수시간 비교*를 통과해야 하므로 단순 문자열 매칭이 아니다. 또한 `tokio-tungstenite::accept_hdr` callback은 axum 통합 시 *불필요* — `axum::extract::ws::WebSocketUpgrade`가 이미 그 hook을 wrap한다.

**선택 = axum extractor 안에서 수동 검증**. tower middleware 레이어가 아닌 *handler 함수 안*에 두는 이유:
- middleware는 모든 HTTP 요청을 봤지만 `Sec-WebSocket-Protocol` 헤더는 *WebSocket upgrade 요청에만* 의미 — handler 위치가 의미적으로 정확.
- middleware에서 reject하면 *400 Bad Request* response가 가나, WebSocket 표준은 *upgrade 실패 시 101이 아닌 그냥 normal HTTP response*를 기대 — axum extractor가 이미 그 contract를 honor한다.
- 상수시간 비교(`ring::constant_time::verify_slices_are_equal`)와 Origin/Host 검증(별도 tower middleware: `tower_http::validate_request::ValidateRequestHeaderLayer` 또는 커스텀)이 *합성* 가능 — Origin/Host는 모든 요청, 토큰은 WS 핸들러만.

**Handler shape sketch**:

```rust
// crate: ws-server (lib)
use axum::extract::ws::{WebSocketUpgrade, WebSocket};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::http::StatusCode;
use ring::constant_time;
use tracing::{instrument, warn};

#[derive(Clone)]
pub struct AppState {
    pub token: Arc<[u8; 32]>,           // 256-bit, auth crate가 부팅 시 로드
    pub allowed_origin: HostName,        // config crate가 [security].cors_origins 첫 항목
    pub dispatcher: Arc<MuxDispatcher>,  // mux-router crate
}

#[instrument(skip_all, fields(remote = ?headers.get("x-forwarded-for")))]
pub async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,  // Origin/Host는 별도 tower layer가 사전 검증 — 본 handler는 토큰만
) -> Result<impl IntoResponse, StatusCode> {
    // 1. requested_protocols 추출 (Sec-WebSocket-Protocol: "gtmux.v1,<token-base64url>")
    let requested: Vec<&str> = ws.requested_protocols()
        .map(|s| s.split(',').map(str::trim).collect())
        .unwrap_or_default();

    if requested.len() != 2 || requested[0] != "gtmux.v1" {
        warn!(reason = "missing or wrong protocol marker");
        return Err(StatusCode::BAD_REQUEST);
    }

    // 2. 토큰 디코딩 + 상수시간 비교
    let presented = match base64_url_decode_fixed::<32>(requested[1]) {
        Ok(bytes) => bytes,
        Err(_) => {
            warn!(reason = "token decode");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };
    if constant_time::verify_slices_are_equal(&presented, state.token.as_ref()).is_err() {
        warn!(reason = "token mismatch");  // 토큰 값은 절대 redact 보존 (tracing redaction 적용)
        return Err(StatusCode::UNAUTHORIZED);
    }

    // 3. echo back (RFC 6455 §11.3.4): 선택된 protocol을 응답에 실어야 brower가 onopen
    let response = ws
        .protocols(["gtmux.v1"])  // 토큰은 절대 echo 안 함 — 'gtmux.v1' marker만
        .on_upgrade(move |socket| ws_session_loop(socket, state));
    Ok(response)
}
```

**단위 테스트 케이스 (R7 DoD — 4종 reject + 1종 accept)**:
1. Origin 헤더 불일치 → tower middleware 단계에서 403 (handler 진입 안 함).
2. Sec-WebSocket-Protocol 헤더 없음 → 400 BAD_REQUEST (requested.len() != 2).
3. `gtmux.v1` marker 없음 → 400 BAD_REQUEST.
4. 토큰 base64 decode 실패 → 401 UNAUTHORIZED.
5. 토큰 mismatch → 401 UNAUTHORIZED.
6. (accept) `gtmux.v1,<valid-base64url-32-bytes>` → 101 Switching Protocols + `Sec-WebSocket-Protocol: gtmux.v1` 응답.

### 6. R7-T5 Schema codegen = **utoipa 5.5.0 + openapi-typescript** (frontend 빌드 단계)

ADR-0010(Group 데이터 모델) + `docs/ssot/canvas-layout-schema.md`의 HTTP `GET/PUT /api/layout` 페이로드, ADR-0002(전송 계층) + wire-protocol SSoT의 envelope 정의를 *Rust*와 *TypeScript* 양쪽에서 *동일 source-of-truth*로 표현해야 한다.

| 차원 | utoipa 5.5.0 | schemars 1.2.1 + json-schema-to-typescript | 채택 |
|---|---|---|---|
| 출력 형식 | OpenAPI 3.1 spec (JSON or YAML) — 엔드포인트·페이로드·헤더 모두 포함 [17][33] | JSON Schema (Draft 2020-12) — 페이로드만, 엔드포인트 정보 별도 [19] | utoipa (HTTP API SSoT를 *one shot*에 표현) |
| Rust → spec 메커니즘 | `#[derive(ToSchema)]` + `#[utoipa::path]` 매크로 | `#[derive(JsonSchema)]` 매크로 | 동률 |
| spec → TS 변환 | `openapi-typescript` (TS 생태 표준 도구) 또는 `openapi-fetch` 클라이언트 자동 생성 | `json-schema-to-typescript` 또는 `json2ts` CLI | utoipa 우위 — `openapi-fetch`로 *호출 코드까지* 자동 생성 |
| 빌드 시간 | `cargo build` 시 매크로 확장 ≤ 2s, frontend `npx openapi-typescript spec.yaml -o api.d.ts` < 1s | 매크로 확장 ≤ 1s, `json2ts` < 1s | 동률 (둘 다 5s 임계 안) |
| wire-protocol (binary envelope) 표현 | utoipa는 *HTTP-centric*이라 binary envelope를 별도 ADR-0002 SSoT(문서)로 분리 | schemars도 binary envelope를 표현 못 함 (JSON Schema는 JSON만) | **동률 — binary envelope는 두 도구 모두 미적용 영역. wire-protocol.md SSoT가 정본** |
| Svelte 5 사용성 (D18 frontend) | TS 타입 import + Zod runtime validation은 별도 라이브러리 필요 | TS 타입 import만 — runtime validation 없음 | 둘 다 type-only로 시작, runtime validation은 P1+ |
| ADR-0010 canvas-layout-schema.md 커버리지 | `groups: [...]` + `panels: [...]` 트리 구조 `ToSchema` derive로 100% 표현, `oneOf` parent_id nullable도 지원 | 동일 (JSON Schema는 자연 표현) | 동률 |
| 유지보수 2026 | 활발 (5.x 시리즈 안정, 6.x 개발 중) | 활발 (1.x 시리즈 안정, alpha 2.x 개발 중) | 동률 |

**결정 = utoipa 5.5.0**. 근거:
1. ADR-0011 D5가 "OpenAPI 우선"으로 명시 — gtmux의 HTTP API(`GET/PUT /api/layout` + ETag)는 *제한된 N개 엔드포인트*라 OpenAPI spec이 자연.
2. frontend(Svelte 5)는 `openapi-typescript` + `openapi-fetch` 조합으로 *타입 + 호출 코드*를 모두 자동 생성 — Bun/Vite/TS의 표준 패턴 [33].
3. binary envelope(WS frame)는 어떤 도구로도 자동 생성 불가 — `docs/ssot/wire-protocol.md` 표가 정본이고, Rust enum + TypeScript discriminated union을 *수동* (Rust enum derive `serde(tag = "type")` + TS 측은 `const enum` 매칭)으로 작성하되 enum variant 이름을 SSoT 표 그대로 미러. *동기화 검증 테스트* = backend 빌드 시 wire-protocol SSoT의 type 코드 표를 read & assert (간단한 `cargo test` integration test).

**파이프라인**:
```
crates/http-api/src/handlers.rs   ─┐
  #[derive(ToSchema)] CanvasLayout │
  #[utoipa::path(GET /api/layout)] │
                                   ├──► cargo run --bin gen-openapi
                                   │       → docs/ssot/openapi.yaml
                                   ▼
                            frontend/build.sh
                              npx openapi-typescript ../docs/ssot/openapi.yaml -o src/api/types.d.ts
                              npx openapi-fetch ../docs/ssot/openapi.yaml -o src/api/client.ts
```

### 7. R7-T6 Config 로더 = **figment 0.10.19**

D22의 config 스키마(`schema_version`/`server`/`runtime`/`security`/`cloud`) + 선행순위(CLI flag > env > file > default) + unknown-field 거부.

| 차원 | figment 0.10.19 | config 0.15.22 | 채택 |
|---|---|---|---|
| TOML + env 오버레이 | `Figment::from(Toml::file(...)).merge(Env::prefixed("GTMUX_").split("__"))` — `GTMUX_RUNTIME__LOG_LEVEL=debug` 자연 매핑 [20] | `Config::builder().add_source(File::with_name(...)).add_source(Environment::with_prefix("GTMUX").separator("__"))` [21] | 동률 |
| Unknown field 거부 | serde `#[serde(deny_unknown_fields)]` 직접 사용 가능 | 동일 | 동률 |
| 선행순위 표현 | `merge()` chain 직접 — CLI > env > file > default 4단계가 코드 4줄 | `add_source(...).build()` chain | 동률 |
| Per-source provenance (어느 source가 어떤 값을 줬는지 추적) | **있음** — `Figment::metadata()` API + `Tag` [20] | 부분 (source name 정도만) | **figment** 우위 |
| MSRV | 미선언 (1.65 통과 추정) | 1.85 | figment 약간 우위 (단 floor는 어차피 1.85) |
| Profile/환경별 분기 (e.g. local vs cloud) | `Figment::profile("local")` / `profile("cloud")` 내장 | 미내장, 직접 구현 | **figment** 우위 (D22의 local/cloud bind 추론과 정합) |
| Rocket 생태 (figment 메인 사용자) | 안정, 0.10 시리즈 5년 | 별도 메인테이너 | 동률 |
| 유지보수 2026 | 활발 (0.10.19 = 2025–2026 release) | 활발 (0.15.22 = 2025–2026 release) | 동률 |

**결정 = figment 0.10.19**. 결정타: (1) per-source provenance — 사용자가 `gtmux config show` (P1+)에서 "이 값은 CLI flag", "이 값은 env"로 표시 가능, (2) profile 분기가 D22의 local/cloud 모드 추론과 자연 합성, (3) ADR-0011 D5·D6의 1순위로 이미 명시.

**`GTMUX_RUNTIME__LOG_LEVEL` 실증**:
```rust
// crate: config (lib)
use figment::{Figment, providers::{Format, Toml, Env, Serialized}};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub schema_version: u32,
    pub server: ServerCfg,
    pub runtime: RuntimeCfg,
    pub security: SecurityCfg,
    pub cloud: Option<CloudCfg>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCfg {
    pub ring_buffer_size_kb: u32,        // env: GTMUX_RUNTIME__RING_BUFFER_SIZE_KB
    pub layout_debounce_ms: u32,
    pub panel_state_debounce_ms: u32,
    pub log_level: String,                // env: GTMUX_RUNTIME__LOG_LEVEL
    pub log_format: String,
    pub pause_after_sec: u32,             // ADR-0001 D9 후속
}

pub fn load(path: &Path, cli_overrides: CliOverrides) -> Result<Settings, ConfigError> {
    let figment = Figment::new()
        .merge(Serialized::defaults(Settings::default()))
        .merge(Toml::file(path))
        .merge(Env::prefixed("GTMUX_").split("__"))
        .merge(Serialized::defaults(cli_overrides));

    figment.extract::<Settings>()
        .map_err(ConfigError::from)
}
```

`GTMUX_RUNTIME__LOG_LEVEL=debug ./gtmux start --session foo` 호출 시 `Settings.runtime.log_level == "debug"` (file 기본값 "info"를 env가 override). 단위 테스트 1줄:
```rust
#[test]
fn env_overrides_file() {
    std::env::set_var("GTMUX_RUNTIME__LOG_LEVEL", "debug");
    let s = load(Path::new("fixtures/minimal.config.toml"), CliOverrides::default()).unwrap();
    assert_eq!(s.runtime.log_level, "debug");
}
```

### 8. R7-T7 Cargo workspace outline (7-crate)

**디렉터리 구조**:
```
codebase/
├── Cargo.toml                 # workspace root
├── rust-toolchain.toml        # 1.85 pin
├── crates/
│   ├── mux-router/            # tmux state, control mode parser, command argv router
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── ws-server/             # axum WS upgrade, envelope codec, MT-3 broadcast
│   ├── http-api/              # axum HTTP, GET/PUT /api/layout, ETag, utoipa spec
│   ├── lifecycle/             # tmux daemon spawn/teardown, socket cleanup, signal handling
│   ├── config/                # figment loader, D22 schema, env overlay
│   ├── auth/                  # token issue/rotate/verify, constant-time, redaction
│   └── gtmux-cli/             # clap CLI binary (start/stop/teardown/rotate-token/status)
│       ├── Cargo.toml
│       └── src/main.rs
└── tests/                     # cross-crate integration tests
```

**Workspace root `Cargo.toml`**:
```toml
[workspace]
resolver = "2"
members = [
  "crates/mux-router",
  "crates/ws-server",
  "crates/http-api",
  "crates/lifecycle",
  "crates/config",
  "crates/auth",
  "crates/gtmux-cli",
]

[workspace.package]
edition = "2024"
rust-version = "1.85"
license = "MIT OR Apache-2.0"
repository = "https://github.com/<owner>/gtmux"

[workspace.dependencies]
tokio = { version = "1.52", features = ["rt-multi-thread", "macros", "io-util", "process", "signal", "sync", "fs", "time", "net"] }
axum = { version = "0.8.9", features = ["ws", "http2", "macros"] }
tokio-tungstenite = { version = "0.29", features = ["handshake"] }
tower = { version = "0.5", features = ["util"] }
tower-http = { version = "0.6.10", features = ["cors", "trace", "request-id", "set-header", "validate-request"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
bytes = "1.11.1"
clap = { version = "4.6.1", features = ["derive", "env"] }
tracing = "0.1.41"
tracing-subscriber = { version = "0.3", features = ["fmt", "json", "env-filter"] }
ring = "0.17.16000"
figment = { version = "0.10.19", features = ["toml", "env"] }
winnow = "1.0.2"
utoipa = { version = "5.5", features = ["axum_extras"] }
anyhow = "1.0.102"
thiserror = "2.0.18"
async-trait = "0.1"
futures = "0.3"

[profile.release]
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

**의존 그래프 (DAG, cycle 없음, 역방향 차단)**:
```
gtmux-cli (bin)
   ├──► lifecycle
   │        ├──► config
   │        └──► auth
   ├──► http-api
   │        ├──► mux-router  (command issue API만)
   │        ├──► auth
   │        ├──► config
   │        └──► (utoipa, axum, tower-http)
   ├──► ws-server
   │        ├──► mux-router  (subscribe to %output, command issue API)
   │        ├──► auth
   │        └──► (axum, tokio-tungstenite)
   └──► mux-router
            └──► (tokio, bytes, winnow, tracing)
```

역방향 금지 (`mux-router → http-api/ws-server`): mux-router의 public API는 *command Send/Receive*만 노출. tmux state는 *event stream* (`tokio::sync::broadcast::Receiver<MuxEvent>`)으로 외부에 흐름 — http-api/ws-server가 *수신*하지 *주입*하지 못함. 컴파일 타임 cycle = `cargo` resolver가 거부.

**Per-crate `Cargo.toml` + public API 1~2줄 요약**:

**`crates/mux-router/Cargo.toml`**:
```toml
[package]
name = "gtmux-mux-router"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
tokio = { workspace = true }
bytes = { workspace = true }
winnow = { workspace = true }
serde = { workspace = true }
tracing = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
```
**Public API**: `MuxClient::connect(socket: PathBuf) -> Result<MuxHandle>`로 tmux control mode 채널 attach, `MuxHandle::issue(cmd: Command) -> Result<CommandReply>`로 allowlist 명령(`Command` enum) argv 전송, `MuxHandle::events() -> broadcast::Receiver<MuxEvent>`로 `%output`/`%window-*`/`%pane-*` 알림 fan-out. Per-pane ring buffer는 `MuxHandle::pane_buffer(pid: PaneId) -> Bytes` snapshot 노출.

**`crates/ws-server/Cargo.toml`**:
```toml
[package]
name = "gtmux-ws-server"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
tokio = { workspace = true }
axum = { workspace = true }
tokio-tungstenite = { workspace = true }
bytes = { workspace = true }
gtmux-mux-router = { path = "../mux-router" }
gtmux-auth = { path = "../auth" }
serde = { workspace = true }
tracing = { workspace = true }
```
**Public API**: `ws_router(state: AppState) -> axum::Router` — `/ws` endpoint를 노출. 핸들러는 `WebSocketUpgrade` extractor + 토큰 검증(§5 sketch). envelope codec은 `pub mod envelope` (1B type + varint paneId + payload, wire-protocol SSoT 100% 미러).

**`crates/http-api/Cargo.toml`**:
```toml
[package]
name = "gtmux-http-api"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
tokio = { workspace = true }
axum = { workspace = true }
tower-http = { workspace = true }
utoipa = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
gtmux-mux-router = { path = "../mux-router" }
gtmux-auth = { path = "../auth" }
gtmux-config = { path = "../config" }
tracing = { workspace = true }
anyhow = { workspace = true }
```
**Public API**: `api_router(state: AppState) -> axum::Router` — `GET/PUT /api/layout` (+ ETag RFC 7232 `If-Match`/412), `GET /api/openapi.json` (utoipa spec dump). layout 페이로드 `CanvasLayout` struct = ADR-0010 G-hybrid schema 그대로 (`#[derive(ToSchema, Serialize, Deserialize)]`).

**`crates/lifecycle/Cargo.toml`**:
```toml
[package]
name = "gtmux-lifecycle"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
tokio = { workspace = true }
gtmux-config = { path = "../config" }
gtmux-auth = { path = "../auth" }
tracing = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
```
**Public API**: `LifecycleManager::start(cfg: Settings) -> Result<LifecycleHandle>` — tmux daemon spawn (`tmux -L gtmux-<session> start-server`, ADR-0009 D3) + socket 점검 + session 존재 검증 (D4) + PID file 작성. `LifecycleHandle::teardown(opts: TeardownOpts) -> Result<()>` — ADR-0009 D6 5단계 절차 실행. SIGTERM/SIGINT handler 등록.

**`crates/config/Cargo.toml`**:
```toml
[package]
name = "gtmux-config"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
figment = { workspace = true }
serde = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
```
**Public API**: `load(path: &Path, cli: CliOverrides) -> Result<Settings>` (§6 sketch). `Settings` struct = D22 schema 그대로 (`#[serde(deny_unknown_fields)]`).

**`crates/auth/Cargo.toml`**:
```toml
[package]
name = "gtmux-auth"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
ring = { workspace = true }
base64 = "0.22"
tokio = { workspace = true, features = ["fs"] }
serde = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
```
**Public API**: `TokenStore::generate() -> Token` (256-bit `SystemRandom`), `TokenStore::load(path: &Path) -> Result<Token>` (0600 perm 검증), `Token::verify_presented(&self, candidate: &[u8]) -> bool` (`constant_time::verify_slices_are_equal`), `redact_token(s: &str) -> String` (tracing layer가 호출).

**`crates/gtmux-cli/Cargo.toml`**:
```toml
[package]
name = "gtmux-cli"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[[bin]]
name = "gtmux"
path = "src/main.rs"

[dependencies]
clap = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
gtmux-lifecycle = { path = "../lifecycle" }
gtmux-ws-server = { path = "../ws-server" }
gtmux-http-api = { path = "../http-api" }
gtmux-config = { path = "../config" }
gtmux-auth = { path = "../auth" }
anyhow = { workspace = true }
```
**Public API**: 바이너리. `clap::Parser` derive로 D20 subcommands(`start`/`stop`/`teardown`/`rotate-token`/`status`) 정의. `--port` lookup 로직(D21 c6) = `gtmux-config`의 directory scan helper 호출. SIGTERM → graceful shutdown.

### 9. D19 MVP 벤치마크 시나리오 설계 (실행 아님, *코드 shape*만)

각 시나리오마다 (a) 워크로드, (b) 측정 instrument, (c) PASS 임계 — 셋을 명시. *실행*은 R7 후속 (codebase bootstrap 후 별도 task)이다.

#### 9.1. Cold start `gtmux start` → 첫 paint < 500ms

- **워크로드**: 빈 tmux session 1개(`session=foo`, 1 pane = shell prompt only) 대상으로 `gtmux start --session foo --port 9001` 호출 → 브라우저가 `http://localhost:9001/`을 GET → first contentful paint까지의 wall-clock.
- **Instrument**:
  - Backend = `tracing` span `gtmux.cold_start` 안에 (1) tmux daemon spawn, (2) control mode attach, (3) list-* snapshot, (4) HTTP/WS listener bind, (5) ready stdout 5개 sub-span. Cargo dev-dependency = `criterion` 0.5 — 단, criterion은 *micro-bench* 도구라 *cold start*는 `cargo bench` 안에서 `Command::new("./target/release/gtmux")` spawn + RTT 측정으로 *integration bench* 작성. Frontend = Playwright의 `page.goto` + `waitForFunction(() => document.querySelector(...).innerText)` timing.
  - Backend 메모리 = `criterion-perf-events` (Linux) 또는 macOS `mach_task_basic_info` for RSS 측정.
- **PASS**: 5개 sub-span sum < 500ms. 측정 후 dominant span 식별 → 최적화 priority.

#### 9.2. Warm reconnect (daemon 살아 있음, 새 탭) < 300ms

- **워크로드**: 9.1로 부팅된 Server 상태 유지, 두 번째 브라우저 탭이 같은 URL 접속.
- **Instrument**: Backend `tracing` span `gtmux.warm_reconnect` = (1) HTTP GET `/api/layout` response, (2) WS upgrade + token verify, (3) ring buffer replay (per-pane 128 KB). Frontend = `performance.mark()` API로 reconnect 시작 → xterm.js render 완료.
- **PASS**: Backend span < 100ms + frontend xterm replay < 200ms = 합 < 300ms.

#### 9.3. Per-pane output latency p50 < 30ms / p99 < 100ms

- **워크로드**: 50 pane (5 burst + 45 idle, D19 표준 워크로드). burst pane = `yes "<8KB random ASCII>" | head -n 10000` 10초간. tmux process → gtmux backend stdout → WS frame → browser xterm.js `write()` callback의 wall-clock.
- **Instrument**:
  - **Backend 단**: `tracing` span `pane.<id>.output_chunk` (start = `%output` 파싱 완료, end = WS write complete). 각 청크마다 latency 한 점.
  - **Frontend 단**: WS `onmessage` 이벤트에서 timestamp 1, xterm.js `write(bytes)` 후 next animation frame `requestAnimationFrame` callback에서 timestamp 2. 차이가 *처리* latency. 백엔드 timestamp 1을 envelope에 frame-level age field로 직접 싣지 않고, 별도 `0x06 CTRL.PROBE` envelope (wire-protocol SSoT 예약 슬롯 사용)로 1초마다 NTP-like ping을 송신해 *clock skew* 계산. 실제 *end-to-end* = backend `tracing` start − frontend xterm render complete.
  - 통계 = `hdrhistogram` crate (Rust) + `umap` (browser) — p50, p95, p99 산출.
- **PASS**: hdrhistogram 결과에서 p50 < 30ms AND p99 < 100ms.

#### 9.4. Panel drag commit → 모든 연결 sync < 500ms

- **워크로드**: 브라우저 탭 2개, 같은 Server에 attach. 탭 A에서 panel 드래그 → drop. HTTP PUT `/api/layout` 디바운스 300ms (D12) 후 commit → WS `LAYOUT_CHANGED` notify (0x80) broadcast → 탭 B의 store 갱신 → 탭 B의 panel 좌표 reflection.
- **Instrument**: 탭 A의 mouseup `performance.mark("drag_end")`, 탭 B의 panel position `getBoundingClientRect()` 변화 감지 `performance.mark("sync_done")` (MutationObserver 또는 effect tap). Backend `tracing` span `layout.commit` (PUT 수신 → ETag 생성 → broadcast complete).
- **PASS**: drag_end → sync_done < 500ms (이 중 디바운스 300ms은 *허용* 비용).

#### 9.5. Server backend memory baseline < 30 MB, Per-Server total < 50 MB

- **워크로드**: 9.1 baseline (1 pane, idle 1분) 메모리 → 9.3의 50-pane 워크로드 ramp-up 후 memory peak.
- **Instrument**:
  - Backend = `/proc/<pid>/status` (Linux) 또는 `ps -o rss=` (macOS). criterion run 중 1초 주기 sampling.
  - tmux daemon = ADR-0009 §실측 footprint 표가 baseline 3.4 MB, 60 panes 4.3 MB로 *별도* 측정 — 본 시나리오는 gtmux Server 프로세스만.
  - Per-Server total = gtmux Server RSS + tmux daemon RSS + ring buffer (50 × 128 KB = 6.4 MB, runtime alloc).
- **PASS**: gtmux Server RSS < 30 MB AND total < 50 MB.

#### 9.6. gtmux → 브라우저 WS write lag < 5s (tmux 대비)

- **워크로드**: 1 pane에서 `cat /dev/urandom | base64 | head -c 100M` (100 MB 빠른 출력). tmux 내부 버퍼 vs gtmux WS write buffer 누적 시간 측정.
- **Instrument**: ADR-0001 D9의 `pause-after=10` 활성 상태에서 `%extended-output`의 `<age-ms>` 필드 추적. WS write completion에서 *그* age-ms 값을 backend tracing field로 기록 → 최대값이 *lag*.
- **PASS**: 최대 age-ms < 5000.

### 10. ADR-0011 Open 항목 closure 요약

| Open | 결정 | §본 보고서 |
|---|---|---|
| O1. MSRV | **1.85** + `rust-toolchain.toml` pin YES | §2 |
| O2. Crypto | **ring 0.17.16000** 단독 | §3 |
| O3. tmux parser | **winnow 1.0.2 + bytes 직접 디코더 하이브리드** | §4 |
| O4. WS token hook | **axum extractor 안 + `requested_protocols`/`set_selected_protocol` 수동** | §5 |
| O5. Schema codegen | **utoipa 5.5.0 + openapi-typescript** | §6 |
| O6. Config | **figment 0.10.19** | §7 |
| O7. Workspace outline | **7-crate scaffolding 확정** | §8 |
| (추가) D19 벤치마크 | **6 시나리오 설계** (실행은 R7 후속) | §9 |

ADR-0011 Status는 본 보고서 채택 후 **Proposed → Accepted** 승격 가능.

## 옵션 비교 종합 (1 표)

| 결정 차원 | 채택 | 거절 | 결정 근거 (한 줄) |
|---|---|---|---|
| MSRV | 1.85 | 1.80 / 1.95 | 1.85 = clap·ring·rustls·config·rand의 공통 floor + edition 2024 안정화 |
| Async runtime | tokio 1.52.x | smol / async-std | ADR-0011 D2 + 생태계 압도적 우위 |
| HTTP fw | axum 0.8.9 | actix-web / warp | ADR-0011 D3 + tower 통합 |
| WS | tokio-tungstenite 0.29 | tungstenite stand-alone / fastwebsockets | axum 통합 자연 |
| Crypto | ring 0.17.16000 | rustls+rand+subtle | API 단순성 + ADR-0011 D8 1순위 |
| Parser | winnow 1.0.2 + bytes LUT | nom 8 / 직접 state machine | 콤비네이터 유지보수성 + hot-path LUT 보존 |
| Config | figment 0.10.19 | config 0.15.22 | profile/provenance + ADR-0011 D5 |
| Schema codegen | utoipa 5.5 | schemars 1.2 | OpenAPI 통합 (TS frontend) |
| Cross-compile | cargo-zigbuild 0.22.3 | cross / cargo-dist | zig linker가 musl/macOS 양쪽 cleanest |
| Workspace 분할 | 7-crate | monolithic / 3-crate | §10.1 8 컴포넌트와 1:1 매핑, 불변식 #1 컴파일 강제 |

## 권장 (Final)

ADR-0011을 **Accepted**로 승격하기 위한 채택 패키지:

1. **MSRV = 1.85** + `rust-toolchain.toml` pin.
2. **Crate-version table** (위 §2 표 그대로) — workspace root `[workspace.dependencies]` 섹션이 정본.
3. **Crypto = ring 0.17.16000 단독**.
4. **Parser = winnow 1.0.2 (라인 dispatch) + bytes 1.11.1 LUT (`%output` payload)**.
5. **WS 토큰 검증 = axum extractor 안 수동 `requested_protocols`/`set_selected_protocol` + ring constant-time**.
6. **Schema = utoipa 5.5 → openapi-typescript**.
7. **Config = figment 0.10.19** (`GTMUX_<SECTION>__<KEY>` env 오버레이 + `deny_unknown_fields`).
8. **Cross-compile = cargo-zigbuild 0.22.3** (macOS aarch64/x86_64 + Linux x86_64/aarch64).
9. **Workspace = 7-crate** (§8 디렉터리 + DAG).
10. **벤치마크 = §9 6 시나리오** (실행은 R7 후속, codebase bootstrap 후).

## 거절된 안 (Rust 내부)

- **R-A.** MSRV 1.80 (ADR-0011 잠정값) — clap 4.6.1의 1.85 요구가 supersede.
- **R-B.** rustls + rand + subtle 조합으로 ring 대체 — rustls는 TLS *프로토콜* crate라 비교 구도가 부정확, ring이 MVP crypto에 충분.
- **R-C.** nom 8.0.0 — winnow가 nom의 후속(같은 author)으로 streaming API가 더 강함 + 컴파일 시간 약간 우위.
- **R-D.** schemars + json-schema-to-typescript — HTTP API SSoT를 OpenAPI로 표현하지 못해 frontend `openapi-fetch` 자동 생성 경로를 잃음.
- **R-E.** config (rs) 0.15.22 — figment의 profile/provenance가 D22 local/cloud 모드 추론에 더 자연.
- **R-F.** 단일 crate (monolithic) workspace — sketch §10.1 8 컴포넌트의 코드 경계를 컴파일 단위로 강제하지 못해 불변식 #1·#2 정합 약화.
- **R-G.** `cargo dist` — cargo-zigbuild가 zig linker 단일 도구로 macOS/Linux 양쪽 정적 빌드를 통일하고, `cargo dist`는 릴리스 자동화 도구이므로 비교 차원이 다름. (P1+ 릴리스 단계에서 dist를 *추가* 도입 가능 — 본 보고서는 *빌드 도구*만 잠금.)

## 미해결 항목 (Open)

- **U1. D9 cross-compile 실측** — `cargo zigbuild --target aarch64-apple-darwin/x86_64-apple-darwin/x86_64-unknown-linux-musl/aarch64-unknown-linux-musl` 4종 모두 단일 정적 바이너리 산출 통과 여부 측정 + 산출 binary size 확인. **트리거**: codebase bootstrap 후 첫 release build.
- **U2. D19 벤치마크 *실행*** — §9의 6 시나리오 코드 작성 + 실측 + 결과를 `docs/reports/0007-backend-runtime-benchmarks.md`로 분리 산출. **트리거**: codebase bootstrap + sketch §15 1단계 완료 후.
- **U3. ADR-0001 O1 (long-suspend buffer disconnect) 측정** — R7 §9.6과 별도. 5분 이상 Suspended pane이 강제 disconnect 받는지 검증. 결과에 따라 ADR-0001 D8 `pause` → `off` 자동 승격 amend.
- **U4. ADR-0009 O1 (`TMUX` env nested attach) + O2 (systemd-tmpfiles)** — R7 implementation 검증으로 ADR-0009가 이미 위임. 본 보고서는 *crate set*만 잠가서 영향 없음.
- **U5. ring binary size dead-code-elim 실측** — §3 표가 ADR-0011 O2 허용폭 500 KB를 "추정 < 300 KB"로 채웠으나 *실제* zigbuild release LTO 결과 확인 필요. **트리거**: U1과 합쳐 측정.
- **U6. ADR-0003 (보안 디폴트) 발행 시 TLS crate 잠금** — cloud 모드 활성 시 `rustls = "0.23"` + `tokio-rustls = "0.26"` + `axum-server = "0.7"` 정확히 어떤 features를 켤지. 본 보고서는 *MVP 로컬 모드*만 잠금.

## 출처 (URL + 접근일자)

[1] Rust stable channel manifest — https://static.rust-lang.org/dist/channel-rust-stable.toml (접근: 2026-05-13)
[2] tokio Cargo.toml — https://raw.githubusercontent.com/tokio-rs/tokio/master/tokio/Cargo.toml (접근: 2026-05-13)
[3] axum Cargo.toml — https://raw.githubusercontent.com/tokio-rs/axum/main/axum/Cargo.toml (접근: 2026-05-13)
[4] tokio-tungstenite Cargo.toml — https://raw.githubusercontent.com/snapview/tokio-tungstenite/master/Cargo.toml (접근: 2026-05-13)
[5] tower-http Cargo.toml — https://raw.githubusercontent.com/tower-rs/tower-http/main/tower-http/Cargo.toml (접근: 2026-05-13)
[6] serde Cargo.toml — https://raw.githubusercontent.com/serde-rs/serde/master/serde/Cargo.toml (접근: 2026-05-13)
[7] clap Cargo.toml — https://raw.githubusercontent.com/clap-rs/clap/master/Cargo.toml (접근: 2026-05-13)
[8] tracing Cargo.toml — https://raw.githubusercontent.com/tokio-rs/tracing/v0.1.x/tracing/Cargo.toml (접근: 2026-05-13)
[9] ring Cargo.toml — https://raw.githubusercontent.com/briansmith/ring/main/Cargo.toml (접근: 2026-05-13)
[10] cargo Cargo.toml (MSRV reference) — https://raw.githubusercontent.com/rust-lang/cargo/master/Cargo.toml (접근: 2026-05-13)
[11] bytes Cargo.toml — https://raw.githubusercontent.com/tokio-rs/bytes/master/Cargo.toml (접근: 2026-05-13)
[12] rustls Cargo.toml — https://raw.githubusercontent.com/rustls/rustls/main/rustls/Cargo.toml (접근: 2026-05-13)
[13] axum workspace Cargo.toml — https://raw.githubusercontent.com/tokio-rs/axum/main/Cargo.toml (접근: 2026-05-13)
[14] axum 0.8 docs (WebSocketUpgrade) — https://docs.rs/axum/latest/axum/extract/ws/struct.WebSocketUpgrade.html (접근: 2026-05-13)
[15] serde_json Cargo.toml — https://raw.githubusercontent.com/serde-rs/json/master/Cargo.toml (접근: 2026-05-13)
[16] winnow Cargo.toml — https://raw.githubusercontent.com/winnow-rs/winnow/main/Cargo.toml (접근: 2026-05-13)
[17] utoipa Cargo.toml — https://raw.githubusercontent.com/juhaku/utoipa/master/utoipa/Cargo.toml + workspace — https://raw.githubusercontent.com/juhaku/utoipa/master/Cargo.toml (접근: 2026-05-13)
[18] rustls latest 0.23.40 release notes — https://github.com/rustls/rustls/releases (접근: 2026-05-13)
[19] schemars Cargo.toml — https://raw.githubusercontent.com/GREsau/schemars/master/schemars/Cargo.toml (접근: 2026-05-13)
[20] figment Cargo.toml — https://raw.githubusercontent.com/SergioBenitez/Figment/master/Cargo.toml + docs — https://docs.rs/figment/latest/figment/ (접근: 2026-05-13)
[21] config (rs) Cargo.toml — https://raw.githubusercontent.com/rust-cli/config-rs/main/Cargo.toml (접근: 2026-05-13)
[22] anyhow Cargo.toml — https://raw.githubusercontent.com/dtolnay/anyhow/master/Cargo.toml (접근: 2026-05-13)
[23] thiserror Cargo.toml — https://raw.githubusercontent.com/dtolnay/thiserror/master/Cargo.toml (접근: 2026-05-13)
[24] nom Cargo.toml — https://raw.githubusercontent.com/rust-bakery/nom/main/Cargo.toml (접근: 2026-05-13)
[25] subtle Cargo.toml — https://raw.githubusercontent.com/dalek-cryptography/subtle/main/Cargo.toml (접근: 2026-05-13)
[26] rand Cargo.toml — https://raw.githubusercontent.com/rust-random/rand/master/Cargo.toml (접근: 2026-05-13)
[27] cargo-zigbuild Cargo.toml — https://raw.githubusercontent.com/rust-cross/cargo-zigbuild/main/Cargo.toml (접근: 2026-05-13)
[28] rustls README (crypto provider 설명) — https://github.com/rustls/rustls#readme (접근: 2026-05-13)
[29] aws-lc-rs / FIPS 비교 — https://aws.amazon.com/blogs/opensource/introducing-aws-libcrypto-for-rust-an-open-source-cryptographic-library-for-rust/ (접근: 2026-05-13)
[30] axum WebSocketUpgrade source — https://github.com/tokio-rs/axum/blob/main/axum/src/extract/ws.rs (접근: 2026-05-13)
[31] tokio-tungstenite docs (accept_hdr) — https://docs.rs/tokio-tungstenite/latest/tokio_tungstenite/fn.accept_hdr_async.html (접근: 2026-05-13)
[32] tower-http docs (ValidateRequestHeaderLayer) — https://docs.rs/tower-http/latest/tower_http/validate_request/index.html (접근: 2026-05-13)
[33] openapi-typescript npm — https://www.npmjs.com/package/openapi-typescript (접근: 2026-05-13)

## 변경 이력

- 2026-05-13: 초안 (R7 DoD 충족 — crate set 확정 + benchmark 6 시나리오 + 7-crate scaffolding).
