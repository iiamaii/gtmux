# 0013 — 배치 C 부트스트랩 정합성 리뷰 (C5 게이트)

- 일자: 2026-05-14
- 작성: `self-review`
- 입력: `docs/plans/0002-work-dispatch.md` §3 Task C5, `docs/reports/0012-bootstrap-smoke.md` (C4), `codebase/smoke/01_engine_connect.sh`, `codebase/Makefile`, `codebase/README.md`, `codebase/backend/**`, `codebase/frontend/**`, `codebase/shared/**`, `.github/workflows/ci.yml`, ADR-0001/0002/0003/0007/0008/0009/0010/0011/0012 (Accepted), SSoT 4건 (`docs/ssot/canvas-layout-schema.md`, `docs/ssot/wire-protocol.md`, `docs/ssot/security-defaults.md`, 그리고 R7 §8·R8 §Scaffolding outline)
- 선행 리뷰: `docs/reports/0009-adr-coherence-review.md` (A4), `docs/reports/0011-coherence-review.md` (A0.7)
- 후속 결정 게이트: 본 보고서 §6 1단계 진입 권고

## §1. 요약

C1·C2·C3 산출(`commit 3af3abe` + C4 amendment HEAD)은 **빌드 사이클·코드젠 파이프라인·디렉터리 골격·CI placeholder·5대 불변식·라이선스 합치**의 6가지 정합 축에서 모두 PASS다. 본 리뷰가 발견한 갭은 총 **9건** (Blocking 2 · Advisory 5 · Cosmetic 2)이며, 그중 `/auth/bootstrap` 엔드포인트 P0 누락(B1)과 ADR-0006 미작성으로 인한 sketch §15 1단계 진입 조건 불충족(B2) 두 건이 **차단성**으로 분류된다. P0 작업 목록 자체는 §3의 9개 항목(7건 코드 + 1건 ADR + 1건 smoke 보정)으로 보강되면 **배치 C closeout** + **C4 DoD 충족**이 동시에 확보된다. 본 시점의 권고는 (i) 배치 C 코드 부트스트랩 자체는 **Accept-with-amendments** (B1·B2 해소 또는 명시적 deferral 후), (ii) sketch §15 1단계 *구현 진입* 은 ADR-0004·0005·0006 발행(배치 B6 후속) + B1 해소 + P0-LIFE-1·CLI-1·HTTP-1·WS-1·AUTH-1 완료 시 비로소 허용 — 으로 정리한다.

## §2. 조사 범위

본 리뷰는 plan §3 C5 "specific checks" 10개 항목을 모두 cover한다. 각 항목은 §3·§4·§5에 분산 배치되며, 결론과 갭은 §7 매트릭스에 다시 정리된다.

| Check | 위치 | 결과 |
|-------|------|------|
| 1. make targets 실행 | §3.1 | PASS |
| 2. ADR 계약 ↔ 스켈레톤 매핑 (Command enum·CLI subcmd·src/lib tree) | §3.2 | PASS w/ 1 advisory (A1) |
| 3. Codegen 파이프라인 utoipa-only / schemars residue | §3.3 | PASS w/ 1 cosmetic (C1) |
| 4. `/auth/bootstrap?token=` 계약 ↔ ADR-0003 R(rej)2 예외절 | §3.4 | **FAIL — Blocking B1** |
| 5. WS subprotocol `gtmux.v1, bearer.<tok>` advertise + echo `gtmux.v1` | §3.5 | PASS |
| 6. 5대 불변식이 스켈레톤 단계에서도 보존되는가 | §4 | PASS (전 항목) |
| 7. P0 task 목록 충분성 (smoke graduation 완비성) | §3.6 | PASS w/ 1 blocking (B1 = §3.4와 같은 갭) + 2 advisory (A2·A3) |
| 8. CI workflow의 `codegen-verify` 무드리프트 검증 | §3.7 | PASS w/ 2 advisory (A4·A5) |
| 9. 디렉터리 구조 청결성 | §3.8 | PASS w/ 1 cosmetic (C2) |
| 10. 라이선스 합치 (xyflow/svelte MIT · ring · utoipa · axum) | §3.9 | PASS |

## §3. 핵심 발견

### §3.1 Make targets — 실측 PASS (6개)

`docs/plans/0002-work-dispatch.md` §3 Task C5 specific check #1 (실측 명령 6개) 실행 결과:

| 명령 | 결과 | 출력 발췌 |
|------|------|-----------|
| `make help` | PASS | 6개 타겟 (`build`/`test`/`codegen`/`smoke`/`clean`/`help`) 헬프 줄 출력 |
| `make build` | PASS | `cargo build --workspace` Finished 0.05s + vite 461 ms (6 chunks: index/svelteflow/xterm placeholder/CSS) |
| `make test` | PASS | cargo: 8 doctest suites (0 passed/0 failed), svelte-check: 101 FILES 0 ERRORS 0 WARNINGS |
| `make codegen` | PASS | `gen-openapi` → `shared/openapi.yaml` (1005 B), `openapi-typescript 7.13.0` → `api.d.ts` (10.2 ms) |
| `make smoke` | PASS (placeholder semantics) | `SMOKE_GATE_RUNTIME=1` 기본; step 1·2 PASS, step 3~9 GATE/N/A short-circuit. 본 동작은 plan §3 C4 본문이 "실패 단계가 있으면 P0 발행"으로 명시한 *문서화된 실패*에 해당 |
| `make clean` | PASS | `target/` · `node_modules/` · `dist/` · `.svelte-kit/` · `shared/openapi.yaml` · `api.d.ts` 모두 제거됨 (verify: `make codegen && make build` 클린 상태에서 한 번에 재구축 성공) |

**현상**: 모든 6 타겟이 documented 동작대로 작동. `make clean` 후 `make codegen` → `make build` 한 사이클로 클린-슬레이트 재구축 가능 (frontend `npm install` 만 별도 — `make build` 자체는 node_modules 부재 시 fail-fast 하지 않음).
**영향**: 빌드·테스트·코드젠·청소 4축 모두 단일 entrypoint(`make`) 보장.
**권고**: 별도 fix 불필요. `make build`가 `npm install`을 자동 트리거하지 않는 점은 R7/R8 모두에서 별도 결정사항이 아니므로 advisory로 분류하지 않는다 (A2 참조하여 P0 후속 시 보강).

### §3.2 ADR 계약 ↔ 스켈레톤 매핑

#### 3.2.1 `mux-router::Command` enum ↔ ADR-0008 allowlist 표

ADR-0008 §"tmux command allowlist 표"는 11개 ALLOW 행 + 4개 FORBIDDEN 행을 정본으로 둔다. ALLOW 11개:

1. `new-window -t <session>` → `Command::NewWindow` ✓
2. `kill-pane -t %<pid>` → `Command::KillPane` ✓
3. `kill-window -t @<wid>` → `Command::KillWindow` ✓
4. `rename-window -t @<wid> <label>` → `Command::RenameWindow` ✓
5. `send-keys -t %<pid>` → `Command::SendKeys` ✓
6. `refresh-client -A '%<pid>:pause/continue'` → `Command::RefreshClientPause` ✓
7. `refresh-client -B <subscription>` → `Command::RefreshClientSubscribe` ✓
8. `capture-pane -p -e -J -S -<lines>` → `Command::CapturePane` ✓
9. `list-sessions -F` → `Command::ListSessions` ✓
10. `list-windows -a -F` → `Command::ListWindows` ✓
11. `list-panes -a -F` → `Command::ListPanes` ✓

FORBIDDEN 4 (`split-window`·`resize-pane`·`select-layout`·`-CC`)는 enum에 *그 어떤 변형도 없음*. 타입 시스템이 불변식 #4를 *구조적으로* 강제. **PASS**.

#### 3.2.2 `gtmux-cli` clap subcommand ↔ Grill D20

D20 5개 subcommand vs `bin/gtmux-cli/src/main.rs::Cmd` enum:

| D20 | clap 변형 | 인자 | 결과 |
|-----|-----------|------|------|
| `start --session <s> --port <p>` | `Cmd::Start { session, port: Option<u16> }` | 일치 (`port`가 `Option`인 것은 D22 자동 추론 정합) | ✓ |
| `stop --session <s>` | `Cmd::Stop { session }` | 일치 | ✓ |
| `teardown --session <s>` | `Cmd::Teardown { session }` | 일치 (`--force`·`--keep-config` 플래그는 §3.6 A3 참조) | ✓ |
| `rotate-token --session <s>` | `Cmd::RotateToken { session }` | 일치 | ✓ |
| `status` | `Cmd::Status` | 일치 (인자 없음) | ✓ |

**PASS**.

#### 3.2.3 Frontend `src/lib/` 디렉터리 트리 ↔ R8 §Scaffolding outline

R8 outline (보고서 0008 line 536~592)이 정의한 8 서브디렉터리 (`types/`·`stores/`·`ws/`·`http/`·`xterm/`·`canvas/`·`sidebar/`·`toolbar/`·`banner/`·`utils/`) + `styles/` + `routes/`.

실측:

| R8 outline | 실측 디렉터리 | 결과 |
|------------|---------------|------|
| `src/routes/` (+layout.ts + +page.svelte) | 존재 | ✓ |
| `src/lib/types/` (`canvas-layout.d.ts`, `envelope.ts`) | 존재 + `api.d.ts` (codegen 산출) | ✓ |
| `src/lib/stores/` (`panels`·`groups`·`ephemeral`·`layout`·`connection`) | 5 파일 모두 존재 | ✓ |
| `src/lib/ws/` (`client.ts`·`dispatcher.svelte.ts`·`decode.ts`) | 존재 | ✓ |
| `src/lib/http/` (`layout.ts`) | 존재 | ✓ |
| `src/lib/xterm/` (`options.ts`) | 존재 | ✓ |
| `src/lib/canvas/` (`Canvas`·`PanelNode`·`XtermHost`·`PanelPlaceholder`) | 4 파일 모두 존재 | ✓ |
| `src/lib/sidebar/` (`Sidebar`·`GroupTree`·`PanelRow`) | 3 파일 모두 존재 | ✓ |
| `src/lib/toolbar/` (`Toolbar`·`CommandPalette`·`MIndicator`) | 3 파일 모두 존재 | ✓ |
| `src/lib/banner/` (`ReconnectBanner.svelte`) | 존재 | ✓ |
| **`src/lib/utils/`** (`debounce.ts`·`etag.ts`) | **부재** | ✗ Advisory A1 |
| `src/styles/` (`tokens.css`·`global.css`) | 존재 | ✓ |

→ **Advisory A1** — `utils/` 부재. R8 outline 정본 위반.

**A1 갭 카드**:
- **현상**: R8 outline은 `src/lib/utils/{debounce.ts, etag.ts}` 두 파일을 명시했으나 C2 산출에 부재. `etag.ts`는 SSoT `canvas-layout-schema.md` §2의 32-hex ↔ Uint8Array 변환 정본 위치로 지정돼 있음.
- **영향**: P0-FE-1/2 (WS dispatcher·layout HTTP 디바운스) 구현 시 `debounce.ts` 부재가 직접 차단자. ETag normalize/decode 헬퍼가 부재하면 WS 0x80 LAYOUT_CHANGED 페이로드(`etag(16B)`) 와 HTTP `ETag` 헤더(32-hex) 변환 코드가 *각 호출 지점에 인라인*으로 흩어질 위험.
- **권고**: 1단계 진입 전 `src/lib/utils/debounce.ts` + `src/lib/utils/etag.ts` placeholder 2개 추가 (각 5~10 LOC, signature-only로도 OK). P0-FE 작업 항목에 micro-task로 부착.

### §3.3 Codegen 파이프라인 — utoipa 5.x / schemars residue 검증

ADR-0011 D5 + ADR-0012 D7 (A4 §A2 통일) 정본 = **utoipa-only**, `openapi-typescript` 소비.

코드 실측:
- `codebase/backend/Cargo.toml` `[workspace.dependencies]` → `utoipa = "5"` (실효 5.5.0, Cargo.lock 확인). **`schemars` 부재** ✓
- `bin/gen-openapi/Cargo.toml` → `utoipa = { workspace = true }`. `schemars` 부재. ✓
- `bin/gen-openapi/src/main.rs` → `use utoipa::{OpenApi, ToSchema};`. `schemars` 부재. ✓
- `frontend/package.json` → `openapi-typescript ^7.13.0`. `json-schema-to-typescript` 부재. ✓
- `frontend/codegen/run.sh` → `npx --no-install openapi-typescript`. ✓
- `shared/openapi.yaml` 산출 = `openapi: 3.1.0`, `Group`/`Panel` 스키마 export. ✓
- `frontend/src/lib/types/api.d.ts` = openapi-typescript 산출, 헤더에 "auto-generated by openapi-typescript". ✓

**코드 차원 PASS**. 그러나 **ADR-0011 D5 본문에 schemars 잔재 phrase 존재**:

> "JSON Schema 자동 생성은 **`utoipa`** (OpenAPI 우선) 또는 **`schemars`** (R7-T6 검증)" — ADR-0011 line 39

A4 §A2가 utoipa-only로 결정했지만 ADR-0011 D5/R3/O5 본문은 후속 amend되지 않음 (ADR-0012 D7만 amend됨).

→ **Cosmetic C1** — ADR-0011 D5 phrasing이 stale.

**C1 갭 카드**:
- **현상**: ADR-0011 line 39 D5 "`utoipa` 또는 `schemars`"·line 60 R3 "Rust + `utoipa`/`schemars`"·line 82 "wire-protocol/schema 공유는 `utoipa`/`schemars` 산출물"·line 102 O5 "`utoipa`(OpenAPI 우선) vs `schemars`(JSON Schema 우선) vs 둘 다" 네 곳이 schemars 대안을 여전히 언급. A4 §A2 (보고서 0009 line 246~253)가 utoipa-only로 closed.
- **영향**: 정합 위험은 *0* (코드는 schemars 부재). 그러나 신규 기여자가 D5 본문만 보고 `schemars`를 추가할 가능성. 문서 정합도 ↓.
- **권고**: ADR-0011 D5 본문에 1줄 amend ("A4 §A2 결정으로 schemars 대안은 supersede. 본 D5의 정본은 `utoipa` 단일") + O5 항목 strike-through 처리. 별도 ADR 재발행 불필요.

### §3.4 Token bootstrap 계약 — Plan §3 C4 step 5 ↔ ADR-0003 R(rej)2 + D17 c1

Plan §3 C4 step 5 정본 인용:
> "브라우저로 `http://localhost:9999/auth/bootstrap?token=<token>` (콘솔 출력 URL) 1회 접속 — `SameSite=Strict` HttpOnly Secure cookie 발급 + `/` 리다이렉트 확인."

ADR-0003 R(rej)2 본문 (line 109):
> "**예외 (D17 c1 bootstrap exchange)**: 첫 부팅 콘솔 URL `http://localhost:<port>/auth/bootstrap?token=<token>`은 *일회용 cookie 발급* 전용 엔드포인트. 서버는 이 URL을 받으면 (i) token 검증 → (ii) `SameSite=Strict` HttpOnly Secure cookie set → (iii) 즉시 `/` 리다이렉트하고 access log redaction 미들웨어(D9)가 query를 `***REDACTED***`로 마스킹."

→ **Plan ↔ ADR 계약 100% 합치**. C4 step 5의 URL 형태는 ADR-0003 예외절 그대로의 사용.

그러나 **smoke 스크립트 실제 구현** (`codebase/smoke/01_engine_connect.sh` line 138~169) 은 *이 URL을 사용하지 않는다*. 대신 `curl -H "Authorization: Bearer ${TOKEN}" http://127.0.0.1:${PORT}/`로 즉시 Bearer 헤더 경로를 시험. 스크립트 line 152~154 comment가 그 이유를 "R(rej)2가 query-string token logging을 금지하므로 Bearer 경로로 직접" 으로 *조용히* 우회.

→ **Blocking B1** — smoke step 5의 *실측 시나리오* 가 plan 원문 + ADR-0003 예외절을 검증하지 않음. 동시에 P0 task 목록 (smoke report §3) 의 `P0-HTTP-1`이 라우트 (a) `GET /` (b) `GET /api/layout` (c) `PUT /api/layout` 세 개만 명시. **`POST /auth/bootstrap` (또는 `GET /auth/bootstrap?token=…`) 엔드포인트 핸들러 P0 task 부재**.

**B1 갭 카드** (Blocking):
- **현상**:
  1. plan §3 C4 step 5 시나리오 본문은 `/auth/bootstrap?token=…` URL 1회 접속 + cookie 발급 + `/` 리다이렉트를 *실측 검증 대상*으로 적시.
  2. ADR-0003 R(rej)2 예외절 (D17 c1)이 이 URL을 *유일하게 허가된* query-string-token 경로로 정본화.
  3. smoke 스크립트는 이 step을 우회하고 Bearer 헤더 경로로만 시험 (line 152~154 comment가 deviation 사실은 인정).
  4. P0 task 목록(`P0-HTTP-1`)에 bootstrap 엔드포인트 구현 작업이 누락.
- **영향**:
  - **C4 DoD 미충족 위험**: plan §3 C4 본문의 9단계 시나리오 중 step 5가 *명시 변경 없이 우회*된 상태. DoD가 "실패 단계 fix는 P0로 발행"이라 했으므로, 누락된 P0가 있다면 DoD 미충족.
  - **1단계 시연 UX 차단**: 사용자 친화적 1회 URL 진입 흐름(D21 c1 first-run banner)이 부재하면 사용자는 매시작마다 Bearer 헤더를 수동 주입해야 함. 로컬 모드 매시작 토큰 재발급 패턴(ADR-0003 D13.1)이 깨짐.
  - **로그 redaction 미들웨어(ADR-0003 D9) 검증 누락**: bootstrap endpoint가 부재하면 redaction 동작 시험 불가.
- **권고**:
  1. **plan §3 P0 task 추가** (P0-HTTP-2 신설): `codebase/backend/crates/http-api/src/lib.rs::router`에 `GET /auth/bootstrap` 라우트 추가. 동작 = (i) query `token` 추출 + `auth::verify_token` 상수시간 비교, (ii) `SameSite=Strict` HttpOnly Secure cookie set (cookie value = 동일 토큰의 별칭 또는 단기 session key), (iii) 302 → `/` 리다이렉트 + `Location` 헤더, (iv) access log 미들웨어가 query를 `***REDACTED***`로 마스킹.
  2. **smoke step 5 보정**: `01_engine_connect.sh` step 5를 plan §3 원문대로 `/auth/bootstrap?token=…` curl + cookie jar 검증 + 302 검증으로 재작성. Bearer 경로 검증은 step 5b로 분리 (또는 step 7 `GET /api/layout` 안에서 cookie+Bearer 양축 모두 시험).
  3. (대안) 명시 deferral: plan §3 C4 step 5를 amend해 bootstrap을 P1로 미루고 Bearer 직접 검증을 정본화. 이 경우 ADR-0003 D21 c1과 충돌하므로 **별도 ADR-0013 발행 필요** — Plan-A 권장.

### §3.5 WS subprotocol 계약 (B1 carry-forward)

smoke step 6 정본 검증 대상 (스크립트 line 191~229):
- 클라 advertise: `Sec-WebSocket-Protocol: gtmux.v1, bearer.<TOKEN>` (line 215)
- 서버 echo 기대: `Sec-WebSocket-Protocol: gtmux.v1` 만 (line 222)
- 토큰 echo 금지: `assert "bearer." not in resp` (line 223)
- 실패 시 close code 1008 (line 197 fallback)

ADR-0002 D5 (line 47): `["gtmux.v1", "bearer.<base64url-token>"]` advertise, 서버 응답 `gtmux.v1`만, Kubernetes PR #47740 패턴.
ADR-0003 D5 (line 34): 동일 결정 SSoT 정본.
SSoT `security-defaults.md` line 47~48: `ws_subprotocol_advertise = ["gtmux.v1", "bearer.<base64url-token>"]` + `ws_subprotocol_echo = "gtmux.v1"`.

→ **4개 정본 (plan §3 C4 step 6 + ADR-0002 D5 + ADR-0003 D5 + SSoT)이 1:1 매칭**. smoke 스크립트가 이 4축 모두를 정확히 시험. **PASS**.

추가: P0 task `P0-WS-1` (smoke report §3.5)의 acceptance 기준이 "응답 헤더는 `Sec-WebSocket-Protocol: gtmux.v1`만 (bearer.* echo 금지)"를 명시. P0와 smoke의 검증 기준이 동일. **PASS**.

### §3.6 P0 task 목록 충분성

smoke report §3은 9개 P0 항목(CLI-1~5·LIFE-1~2·AUTH-1·HTTP-1·WS-1·MUX-1·CFG-1·FE-1~3 = 총 12개) 발행. 각 항목은 `<file>::<func>` + ADR 계약 + 수용 기준 4튜플. 임계 경로: `P0-LIFE-1 → P0-CLI-1` (step 3 GATE 해제).

**충분성 분석**:

| Step | 차단자 P0 | 충분? |
|------|-----------|-------|
| step 3 (daemon spawn) | P0-LIFE-1 + P0-CLI-1 + P0-AUTH-1 + P0-CFG-1 | 충분 |
| step 4 (외부 tmux attach) | step 3 의존 | 충분 |
| step 5 (HTTP /) | P0-AUTH-1 + P0-HTTP-1 | **불충분 — `/auth/bootstrap` 핸들러 부재 (B1)** |
| step 6 (WS handshake) | P0-AUTH-1 + P0-WS-1 | 충분 |
| step 7 (`GET /api/layout`) | P0-HTTP-1 (b/c 절) | 충분 |
| step 8 (xterm 시각) | P0-FE-1·2·3 + P0-MUX-1 | 충분 (manual N/A) |
| step 9 (teardown) | P0-CLI-3 + P0-LIFE-2 | 충분 |

→ step 5를 제외한 모든 단계에 대해 P0 목록 충분. **step 5의 P0-HTTP-2 (bootstrap endpoint) 추가가 필요** (§3.4 B1과 합쳐 동일 갭).

**A2 갭 카드** (Advisory) — `npm install` auto-trigger 부재:
- **현상**: `make build`는 `npm install`을 수행하지 않음. clean 후 `make build` 호출 시 `npm run build`가 `node_modules` 부재로 fail.
- **영향**: CI에서 `build` job 직전 `npm ci`가 명시돼 있어 (line 70~71) CI는 정상. 그러나 로컬 클린 슬레이트에서 README 1줄 명령으로 빌드 실패 발생.
- **권고**: Makefile `build-frontend` 타겟에 `cd $(FRONTEND) && $(NPM) ci` 사전 단계 추가 또는 README의 "Bootstrap" 절에 `npm install` 1줄 명시 추가. cosmetic이지만 첫 인상 측면에서 advisory.

**A3 갭 카드** (Advisory) — teardown `--force` / `--keep-config` 플래그 부재:
- **현상**: ADR-0009 D6 5단계 + smoke report P0-LIFE-2 acceptance가 `--force`·`--keep-config` 두 플래그를 언급. `bin/gtmux-cli/src/main.rs::Cmd::Teardown`은 `{ session }` 단일 인자만 노출.
- **영향**: P0-CLI-3 + P0-LIFE-2 구현 시 ADR-0009 D6 부분 실패 회복(force) 및 config 보존(keep-config) 변형이 빠진 채 land될 위험. teardown CLI 인터페이스 후속 변경이 P0 후속에서 발생 → 불필요한 API churn.
- **권고**: clap 파생에 `#[arg(long)] force: bool` + `#[arg(long)] keep_config: bool` 두 필드 추가 (각 1 LOC). 본 시점은 placeholder 단계이므로 amend 비용 최저.

### §3.7 CI workflow `codegen-verify`

`.github/workflows/ci.yml` 분석:

- `check` job (line 26~60): cargo check + fmt + clippy + svelte-check. PASS 형태 — 코드 부트스트랩 상태에서도 0 warnings 보장.
- `build` job (line 62~73): `make build` 호출. `npm ci`가 단계로 *부재* — 그러나 `make build`가 frontend로 위임할 때 `node_modules`가 없으면 fail. → **Advisory A4**.
- `codegen-verify` job (line 75~98): `npm ci` 명시 (line 86~88) + `make codegen` + `git diff --exit-code -- shared/openapi.yaml frontend/src/lib/types/api.d.ts`. **무드리프트 검증 메커니즘이 sound** — `make codegen`이 신규 산출을 commit과 비교하므로, 기여자가 backend struct를 변경하고 codegen을 잊으면 CI fail.

**A4 갭 카드** (Advisory) — `build` job 의 `npm ci` 누락:
- **현상**: `build` job (line 62~73)에 `npm ci` step 부재. `make build`가 `cargo build` → `cd frontend && npm run build`로 들어가는 순간 `node_modules` 미존재 가능.
- **영향**: GitHub Actions ubuntu runner는 clean image이므로 `node_modules` 부재 보장. CI build job 항상 fail 예상.
- **권고**: `build` job에 `codegen-verify`와 동일한 `npm ci` step 추가 (line 70 이후, `make build` 직전). 또는 Makefile build-frontend에 `npm ci` 내장 (§3.6 A2와 동일 fix로 합쳐도 무방).

**A5 갭 카드** (Advisory) — Rust toolchain pin 우회:
- **현상**: CI 전 job이 `dtolnay/rust-toolchain@stable` 사용 (line 33, 67, 81). `with: toolchain:` 미지정. `rust-toolchain.toml`의 `channel = "1.85"` pin은 cargo가 *경고만* 처리하므로 CI에서 stable(현 1.95+)로 빌드 실행. R7 §2 D2 ("toolchain pin이 유일한 강제 메커니즘")의 의도와 정합 안 됨.
- **영향**: CI가 1.95에서 통과해도 로컬 1.85 빌드가 실패할 수 있는 *역방향 안전* 위험 (예: `let-chain` 같은 1.85+ feature 미사용 보장 불가). 그러나 현 placeholder는 stable subset 전용이라 즉시 영향 없음.
- **권고**: `dtolnay/rust-toolchain@stable` → `dtolnay/rust-toolchain@1.85` (또는 `master` + `with: toolchain: 1.85`) 로 교체. 3 job 모두에 적용. cosmetic-borderline-advisory.

### §3.8 디렉터리 구조 청결성

**잔여 / 누락 점검**:

| 경로 | 상태 | 비고 |
|------|------|------|
| `codebase/backend/Cargo.lock` | tracked | R7 §8 의도(binary 워크스페이스이므로 commit). ✓ |
| `codebase/backend/target/` | gitignored | ✓ |
| `codebase/frontend/node_modules/` | gitignored | ✓ |
| `codebase/frontend/dist/` | gitignored | ✓ |
| `codebase/shared/.gitkeep` | tracked | `openapi.yaml` 산출 디렉터리 보존용. 산출 후에는 redundant — cosmetic. |
| `codebase/frontend/codegen/README.md` line 17 | stale | "C2 스켈레톤 단계에서 `run.sh`는 미작성 — C3에서 추가" — 이미 C3 완료. → **Cosmetic C2** |
| `codebase/backend/bin/gen-openapi/Cargo.toml` 디렉터리 위치 | `bin/` (R7 §8은 `crates/gtmux-cli`에 포함) | 분리가 *합리적*이나 R7 본문 drift. README가 명시 변경하여 합리화 → §3.8 advisory 아님. |
| `codebase/frontend/src/lib/types/canvas-layout.d.ts` | 1 LOC stub `// generated by codegen` | R8 F2가 *committed*로 지정. 본 코드젠 path는 `api.d.ts`로 통일 (A4 §A2). canvas-layout.d.ts는 비활성 placeholder — codegen이 *덮어쓰지 않음*. 다음 ADR-0006 발행 시 정본화. |

**C2 갭 카드** (Cosmetic):
- **현상**: `codebase/frontend/codegen/README.md` line 17 본문이 "본 C2 스켈레톤 단계에서 `run.sh`는 미작성 — C3 (devops-architect 담당)에서 추가"라고 stale 안내. `run.sh`는 C3에서 이미 land.
- **영향**: 신규 기여자가 README 절을 보고 "C3가 아직 안 끝났나" 오해.
- **권고**: line 17 본문을 "C3 task가 발행한 `run.sh`가 본 디렉터리에 존재. 자세한 흐름은 위 §1 참조." 로 amend.

### §3.9 라이선스 합치

(plan §3 C5 specific check #10)

| 의존 | 라이선스 | 합치 |
|------|---------|------|
| `@xyflow/svelte` 1.5.2 | MIT (package.json 확인) | ✓ |
| `ring` 0.17.14 | ISC AND OpenSSL AND MIT (well-known compound; rustls 패밀리와 동일 호환) | ✓ |
| `utoipa` 5.5.0 | MIT OR Apache-2.0 | ✓ |
| `axum` 0.8.x (workspace dep) | MIT | ✓ |
| `tokio`·`serde`·`tower-http`·`tracing`·`clap` | MIT OR Apache-2.0 (Rust 생태 표준) | ✓ |
| `openapi-typescript` 7.13 (devDep) | MIT | ✓ |
| `@xterm/xterm`·`@xterm/addon-fit`·`@xterm/addon-unicode11` | MIT | ✓ |
| `svelte` 5.x, `vite` 7.x | MIT | ✓ |

워크스페이스 `[workspace.package].license = "MIT OR Apache-2.0"`. 모든 의존이 *최소* MIT를 포함 → 합치. GPL/LGPL/AGPL 오염 없음. **PASS**.

## §4. 5대 불변식 검증 (스켈레톤 단계)

`todo!()` stub 상태에서도 디렉터리·crate 경계·타입 선언만으로 5대 불변식이 *구조적으로* 보존되는가:

| # | 불변식 | 검증 |
|---|--------|------|
| 1 | tmux 상태 / 웹 상태 분리 | **PASS** — Rust 측 `mux-router` crate가 tmux 상태(sessions/windows/panes/output streams)의 단일 owner로 격리, `http-api`/`ws-server`/`gtmux-cli` 어느 것도 `mux-router`를 reverse import하지 않음. Frontend 측 `stores/panels.svelte.ts`·`groups.svelte.ts`·`layout.svelte.ts`가 web-state, `canvas/XtermHost.svelte`가 tmux-state mirror — 분리 명확. |
| 2 | tmux-native vs web-only 분기 | **PASS** — `types/envelope.ts`가 tmux-domain(0x02 PANE_OUT) ↔ web-domain(0x80~0x84) opcode 표를 분리 export. `Command` enum이 tmux-native 명령만 포함, web-only 동작(panel geometry/hide/lock)은 `stores/`에 격리. dispatcher가 분기를 *명시적* 처리. |
| 3 | tmux Layout ≠ Canvas Layout | **PASS** — `mux-router::Command`에 `select-layout`/`split-window`/`resize-pane` 변형 *부재* (§3.2.1). Frontend `canvas/Canvas.svelte`가 SvelteFlow node mapper로 panel position을 자체 owning. tmux layout string은 어디에도 나타나지 않음. |
| 4 | 보안 기본값 | **PASS** — `auth` crate가 `ring` 단독 의존, `verify_token`이 상수시간 비교 시그니처 명시. `Cargo.toml`이 `#![forbid(unsafe_code)]`를 6개 crate 전부에 부착. SSoT `security-defaults.md`가 코드와 동기 (token storage path, CSP template, allowlist). WS subprotocol 정본 §3.5 일치. `/auth/bootstrap` 핸들러는 부재(B1)이나 *negative invariant* — query-string token이 정상 경로로 land하지 않은 상태. |
| 5 | control mode 사용 | **PASS** — `mux-router::Command` doc comment가 "`-CC` 금지", `Command` enum의 어떤 변형도 `-CC` 모드를 invoke하지 않음. `lifecycle::spawn_daemon` 시그니처가 ADR-0009 D2 `-L gtmux-<session>` 컨벤션 commentable. screen-scraping 또는 repeated `tmux list-*` shell-out 경로 *부재* — `Command::ListSessions/Windows/Panes` 세 변형이 control mode subscription 패턴 전제. |

**5대 불변식 모두 스켈레톤 단계에서 구조적으로 보존**. P0 구현 시 본 보고서가 적시한 4튜플 (file/func/contract/acceptance) 을 준수하면 동일 합치 상태 유지.

## §5. 갭 분류 매트릭스

| ID | 종류 | 영역 | 요약 | 차단 단계 |
|----|------|------|------|-----------|
| **B1** | Blocking | http-api · smoke | `/auth/bootstrap?token=…` 엔드포인트 미설계 + P0 누락 + smoke step 5 우회 | C4 DoD · 1단계 시연 UX |
| **B2** | Blocking | doc / prereq | sketch §15 1단계 prereq 중 ADR-0006(영속화 storage·스키마) 미발행 | sketch §15 1단계 entry |
| A1 | Advisory | frontend | `src/lib/utils/{debounce.ts,etag.ts}` 부재 (R8 outline 위반) | P0-FE 구현 효율 |
| A2 | Advisory | Makefile | `make build`가 `npm install` auto-trigger 없음 | 로컬 클린 슬레이트 빌드 UX |
| A3 | Advisory | gtmux-cli | `teardown --force`·`--keep-config` 플래그 누락 (ADR-0009 D6 fully fledged 미반영) | teardown 부분 실패 복구 |
| A4 | Advisory | CI | `build` job에 `npm ci` step 부재 | CI build 단계 실패 예상 |
| A5 | Advisory | CI | `dtolnay/rust-toolchain@stable` 사용 (1.85 pin 우회) | 로컬↔CI 행동 차 위험 |
| C1 | Cosmetic | ADR-0011 | D5/R3/O5 본문 `schemars` 잔재 phrasing (A4 §A2 supersede 미반영) | 문서 정합 |
| C2 | Cosmetic | frontend/codegen | `codegen/README.md` line 17 stale ("C3에서 추가" 안내) | onboarding 명료성 |

**B2 갭 카드 보강** (Blocking):
- **현상**: `docs/sketch.md` §15 (line 761) "선행 조건 (1단계 시작 전 필수)"이 ADR-0001~0003 + 0006 + 0007~0012 발행을 명시. ADR-0006(영속화 storage/스키마, batch B6 후속)은 *아직 미발행* (`docs/adr/` 디렉터리에 0004·0005·0006 부재).
- **영향**: 본 C5 게이트는 **배치 C 코드 부트스트랩 closeout**까지는 청산 가능하나, **sketch §15 1단계 *구현* 진입**은 sketch가 정의한 선행 조건 미충족으로 차단됨. `http-api::router`의 `GET/PUT /api/layout` 본 구현이 ADR-0006의 storage backend 결정(sqlite vs file json) 없이는 사양 미결.
- **권고**: PM이 batch B6 후속(ADR-0004/0005/0006 발행)을 sketch §15 1단계 시작 *직전* trigger. 본 C5 게이트와는 별도 axis로 추적. plan §3 P0 작업 중 layout-persistence-touching 항목(`P0-HTTP-1` (b)·(c) 절)은 ADR-0006 발행 후에 시작.

## §6. sketch §15 1단계 진입 권고

plan §3 C4 DoD ("실패 단계 fix는 P0 발행") vs sketch §15 1단계 prereq를 두 축으로 분리:

### 6.1 배치 C closeout (본 C5의 직접 목적)

**Y, with amendments**. 다음 두 조건 중 하나 충족 시 청산:

- **A안 (권장)**: B1·B2 갭 카드의 "권고"를 plan §3에 amend로 흡수 — P0-HTTP-2 (bootstrap endpoint) + smoke step 5 보정 추가, ADR-0006 발행 prereq를 1단계 entry 게이트로 명시 분리. 본 amend 후 본 보고서 §변경 이력 update.
- **B안 (deferral)**: B1을 별도 ADR-0013 발행으로 deferral — bootstrap UX flow를 정식 ADR로 분리 후 P0 발행. B2는 sketch §15 1단계 prereq의 "ADR-0006" 항목을 strike-through 처리 (영속화는 1단계 외 P1로 reclassify). 변경 시 sketch.md 본문 amend 동반.

Advisory 5건 (A1~A5)은 closeout 차단자가 아닌 *후속 hardening* 항목으로 분리 추적.

### 6.2 sketch §15 1단계 *구현* 진입

**N, blocked by B1+B2 + P0 임계 경로**. 진입 허용 조건:

1. B1 해소 (bootstrap endpoint 설계 + smoke 정합).
2. B2 해소 (ADR-0006 발행 또는 sketch §15 prereq amend).
3. smoke report §3 임계 경로 (P0-LIFE-1 → P0-CLI-1)와 그 의존 (P0-AUTH-1·P0-CFG-1) 완료.
4. P0-HTTP-1 (라우터 + ETag 미들웨어) + P0-WS-1 + P0-LIFE-2 + P0-CLI-3 완료 (smoke step 5·6·7·9 PASS).
5. `SMOKE_GATE_RUNTIME=0` 으로 `01_engine_connect.sh` 재실행 시 step 1·2·3·4·5·6·7·9 = 8 PASS / step 8 manual 확정. 결과를 smoke report §2 표에 추기(amendment block).
6. Advisory A4 해소 (CI build job `npm ci` 추가) — CI 신호가 진입 직후 즉시 안정해야 함.

권장 sprint 순서 (smoke report §4의 sprint 0~3에 본 보고서가 추가하는 항목):

- **Sprint 0 (병렬, leaf)**: P0-AUTH-1, P0-CFG-1, P0-MUX-1, A1 (utils placeholder), A3 (clap flag), A4·A5 (CI fix).
- **Sprint 1**: P0-LIFE-1 + P0-CLI-1 + **P0-HTTP-2 신설** (bootstrap endpoint, B1 해소). 그 결과로 step 3·4 PASS.
- **Sprint 2 (병렬)**: P0-HTTP-1 (step 5·7), P0-WS-1 (step 6), P0-LIFE-2 + P0-CLI-3 (step 9). + smoke step 5 본문 plan §3 정본대로 재작성.
- **Sprint 3**: P0-FE-1·2·3 (step 8 manual PASS).
- **Sprint 4 (1단계 closeout 직전)**: ADR-0006 발행(B2) + ADR-0004·0005 발행. `make smoke SMOKE_GATE_RUNTIME=0` PASS 8/9 확정. 본 보고서 §변경 이력에 entry 추가.

본 6단계 closeout이 끝나면 sketch §15 1단계 (엔진 연결 검증) 정식 시작.

## §7. 최종 권고 요약

- **배치 C 게이트 (C5 자체)**: **PASS — Accept with amendments**. B1·B2 두 Blocking은 plan amend로 흡수 가능, 코드 자체는 5대 불변식·라이선스·codegen·디렉터리 모두 합치. Advisory 5건은 후속 hardening.
- **sketch §15 1단계 진입**: **N (현 시점)**. §6.2 6항목 충족 후 진입.
- **즉시 조치 후보**:
  1. (PM) plan §3에 P0-HTTP-2 추가 + smoke step 5 본문 amend (B1).
  2. (PM) ADR-0006 발행 trigger 또는 sketch §15 prereq amend (B2).
  3. (devops) `.github/workflows/ci.yml` build job `npm ci` 추가 (A4).
  4. (frontend) `src/lib/utils/{debounce.ts,etag.ts}` placeholder 2개 (A1).
  5. (system-architect) ADR-0011 D5/R3/O5 schemars 잔재 strike-through (C1).

## §8. 변경 이력

- 2026-05-14: 초안 (Task C5, HEAD = commit `3af3abe` + C4 amendment). 9개 갭 발행(Blocking 2·Advisory 5·Cosmetic 2). 배치 C closeout = Accept-with-amendments. sketch §15 1단계 entry = blocked (B1+B2+P0 임계 경로).
