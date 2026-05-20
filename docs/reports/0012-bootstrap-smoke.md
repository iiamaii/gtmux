# 0012 — 부트스트랩 통합 Smoke 보고서 (C4)

- 일자: 2026-05-14
- 작성: `quality-engineer`
- 입력: `docs/plans/0002-work-dispatch.md` §3 (C4 9단계 시나리오), 배치 C1·C2·C3 산출 (commit `3af3abe`), ADR-0001/0002/0003/0007/0008/0009/0011/0012, `docs/reports/0007-backend-runtime.md`, `docs/reports/0008-frontend-stack.md`
- 산출 동반: `codebase/smoke/01_engine_connect.sh` (재현 스크립트, `chmod +x`)

## §1. 요약

C1·C2·C3 산출의 빌드·코드젠 파이프라인은 무손상으로 PASS한다 (`make build`·`make codegen`·`make test` 모두 통과). 그러나 `gtmux-cli` 5개 서브명령 본문과 6개 crate 함수가 모두 `todo!()` 상태이므로, C4 시나리오 9단계 중 **실측 PASS = 2단계, GATE(P0 미구현) = 6단계, MANUAL N/A = 1단계**다. 따라서 본 보고서는 (i) 통과한 2단계의 산출 증거를 기록하고, (ii) 나머지 7단계가 PASS로 전환되기 위해 필요한 구현 작업을 *파일·함수·ADR 계약·수용 기준* 4튜플로 적시한 **P0 작업 목록**을 발행하여 sketch §15 1단계 정식 진입의 선결 조건을 명확히 한다.

## §2. 실측 결과

실행 명령: `./codebase/smoke/01_engine_connect.sh` (HEAD = `3af3abe` + C4 산출). `SMOKE_GATE_RUNTIME=1`(기본) 하에서 step 3~9는 GATE로 short-circuit되며, 본 보고서가 발행하는 P0 작업 완료 후 `SMOKE_GATE_RUNTIME=0`으로 재실행한다.

| 단계 | 시나리오 | 결과 | 증거 |
|------|----------|------|------|
| 1 | `make build` | **PASS** | `cargo build --workspace` Finished + `vite build` 6 chunk 생성 (`dist/assets/index-DH29Y5bH.js` 0.95 kB 등). 빈 `xterm` chunk는 placeholder 효과 — R8 `manualChunks` 설정 동작 확인. |
| 2 | `make codegen` | **PASS** | `gen-openapi` 바이너리가 `shared/openapi.yaml` (1005 B, Group/Panel placeholder) 산출 → `openapi-typescript 7.13.0` 9.5 ms로 `src/lib/types/api.d.ts` 생성. 사이클 한 번에 backend → shared → frontend 모두 갱신. |
| 3 | `gtmux start --session smoke --port 9999` (daemon auto-spawn + token URL 출력) | **GATE (P0)** | `bin/gtmux-cli/src/main.rs` L55: `Cmd::Start => todo!("...")`. 호출 체인의 `lifecycle::spawn_daemon` (lifecycle/src/lib.rs L15) · `auth` 토큰 발급 함수 · `http-api::router`·`ws-server::router`·axum bind 전부 미구현. |
| 4 | 별도 셸에서 `tmux -L gtmux-smoke a -t smoke` 외부 attach | **GATE (P0)** | step 3 의존. socket 파일 `${TMUX_TMPDIR}/tmux-${uid}/gtmux-smoke` 미존재. ADR-0009 D2 컨벤션 자체는 `lifecycle::spawn_daemon` 구현 시 자동 확보. |
| 5 | `curl -H "Authorization: Bearer …" http://127.0.0.1:9999/` (SPA 인덱스 + CSP 헤더) | **GATE (P0)** | `http-api::router` 미구현. 또한 보고서 본문이 plan 원문의 `?token=` 형태를 ADR-0003 R(rej)2(쿼리스트링 토큰 금지) 위반으로 판단하여 smoke는 **Bearer 헤더 경로**로 변경했음을 기록 (cookie issuance flow는 D21 c1 first-run banner 흐름의 일부로 별도 단계). |
| 6 | WS handshake (`Sec-WebSocket-Protocol: gtmux.v1, bearer.<token>` → 101 + `gtmux.v1` echo only) | **GATE (P0)** | `ws-server::router` 미구현 + `auth::verify_token`이 `todo!()`. ADR-0002 D5 + ADR-0003 D5 Kubernetes-PR-47740 패턴 검증 불가. |
| 7 | `GET /api/layout` → 200 + `{groups:[],panels:[]}` + ETag `^"[0-9a-f]{32}"$` | **GATE (P0)** | `http-api` 라우터 + ETag middleware 미구현. `gen-openapi`가 산출한 `openapi.yaml`은 `paths: {}`로 아직 endpoint 정의 부재. ADR-0006/0010 영속 layer 미착수. |
| 8 | 브라우저에서 xterm.js 1개 인스턴스 화면 표시 (시각 점검) | **N/A (MANUAL)** | 프론트엔드 `Canvas.svelte`·`XtermHost.svelte`·`ws/client.ts`·`ws/dispatcher.svelte.ts`가 모두 placeholder. 자동화 외 manual 검증 필요. |
| 9 | `gtmux teardown --session smoke` 5단계 정리 후 exit 0 | **GATE (P0)** | `bin/gtmux-cli/src/main.rs` L57 `Cmd::Teardown => todo!()` + `lifecycle::teardown` (lifecycle/src/lib.rs L23) `todo!()`. ADR-0009 D6 5단계 순서·exit code 7 의미 모두 미실현. |

PASS 단계 출력 발췌:

```
==[1/9]== make build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
✓ built in 466ms                                       (vite build)
  PASS  step 1  make build

==[2/9]== make codegen
wrote /Users/.../codebase/shared/openapi.yaml
✨ openapi-typescript 7.13.0
🚀 ../shared/openapi.yaml → src/lib/types/api.d.ts [9.5ms]
  PASS  step 2  make codegen (openapi.yaml + api.d.ts emitted)
```

`make test` 결과(워크스페이스 doc-tests 포함 8 묶음 모두 0 passed / 0 failed, `svelte-check` 101 FILES 0 ERRORS 0 WARNINGS)는 별도 게이트로서 PASS 확정.

## §3. P0 작업 목록

각 작업은 `<file>::<function>` + ADR 계약 + 수용 기준(acceptance) 4튜플. 컴포넌트별 분류. C4 step 컬럼은 본 작업이 어떤 smoke 단계의 GATE를 해제하는지 가리킨다.

### §3.1 `gtmux-cli` (5건 — 전부 step 3·9의 직접 차단자)

| ID | 파일·함수 | 계약 | 수용 기준 | C4 step |
|----|-----------|------|-----------|---------|
| P0-CLI-1 | `codebase/backend/bin/gtmux-cli/src/main.rs::Cmd::Start` (L55) | ADR-0009 D3 (auto-spawn), ADR-0007 D2 (immutable bind), ADR-0003 D4·D13·D21 c1 (token 발급+banner) | `lifecycle::spawn_daemon()` 호출 → 소켓 검증 → `auth` 토큰 발급(`${XDG_STATE_HOME}/gtmux/<s>.token` 0600) → `${XDG_RUNTIME_DIR}/gtmux/<s>.pid` 작성 → axum app(`http-api::router()` + `ws-server::router()`) 127.0.0.1:`<port>` bind → stdout에 `http://127.0.0.1:<port>/?token=<…>` 1회 banner. exit 0 with daemon backgrounded; 충돌 시 exit 4 (port). | 3 |
| P0-CLI-2 | `codebase/backend/bin/gtmux-cli/src/main.rs::Cmd::Stop` (L56) | ADR-0009 D5 (daemon 살림), grill D20 graceful 종료 | `${XDG_RUNTIME_DIR}/gtmux/<s>.pid` SIGTERM → WS close + layout flush 완료 대기 → pid file rm. daemon socket·token·layout·config는 그대로. | (보너스, smoke 9단계 외) |
| P0-CLI-3 | `codebase/backend/bin/gtmux-cli/src/main.rs::Cmd::Teardown` (L57) | ADR-0009 D6 5단계 (Server kill → tmux kill-server → socket rm → state files rm → config rm), exit 7 부분 실패 규약 | `lifecycle::teardown()` 호출. 종료 후 socket·token·pid·layout·config 5경로 모두 부재. `tmux -L gtmux-<s> list-sessions`가 "no server running" 응답. | 9 |
| P0-CLI-4 | `codebase/backend/bin/gtmux-cli/src/main.rs::Cmd::RotateToken` (L58) | ADR-0003 D13.2 (cloud 모드 명시 회전), 활성 WS close code 4001 | 신규 토큰 생성 후 `<s>.token` 원자적 교체(임시 파일 + rename) → broadcast 1008/4001 → 새 URL stdout. local 모드에서 호출 시 exit 6 (mode mismatch). | (보너스) |
| P0-CLI-5 | `codebase/backend/bin/gtmux-cli/src/main.rs::Cmd::Status` (L59) | grill D20 status 명세 | `${XDG_RUNTIME_DIR}/gtmux/*.pid` enumerate → 각 Server의 port + 토큰 file mtime + tmux daemon health probe 결과 표 stdout. exit 0. | (보너스) |

### §3.2 `lifecycle` (2건 — step 3·4·9 차단자)

| ID | 파일·함수 | 계약 | 수용 기준 | C4 step |
|----|-----------|------|-----------|---------|
| P0-LIFE-1 | `codebase/backend/crates/lifecycle/src/lib.rs::spawn_daemon` (L15) | ADR-0009 D2 (socket convention `-L gtmux-<s>`), D3 (auto-spawn), D4 (Session 자동 생성 안 함 → exit 3) | `std::process::Command::new("tmux").args(["-L", &format!("gtmux-{}", session), "start-server"])` argv 분리(shell 미경유). 종료 후 `${TMUX_TMPDIR:-/tmp}/tmux-${uid}/gtmux-<s>` socket 존재 확인. Session 부재 시 caller에 `Err(SessionMissing)` 반환. ADR-0009 O1 nested-`TMUX` 대응 (env 제거 또는 `-2`). | 3, 4 |
| P0-LIFE-2 | `codebase/backend/crates/lifecycle/src/lib.rs::teardown` (L23) | ADR-0009 D6 5단계 순서·`--force`·`--keep-config` 플래그 | 5단계 순서대로 실행: (1) `${XDG_RUNTIME_DIR}/gtmux/<s>.pid` SIGTERM (graceful, timeout 후 SIGKILL), (2) `tmux -L gtmux-<s> kill-server`, (3) socket rm, (4) token·layout.json·pid rm, (5) config.toml rm. 단계별 실패 시 ErrorKind 보존 후 stderr 잔여 경로 출력 + exit 7. | 9 |

### §3.3 `auth` (1건 — step 5·6 차단자)

| ID | 파일·함수 | 계약 | 수용 기준 | C4 step |
|----|-----------|------|-----------|---------|
| P0-AUTH-1 | `codebase/backend/crates/auth/src/lib.rs::verify_token` (L12) + 신규 `issue_token`/`load_token` | ADR-0003 D4 (256-bit CSPRNG base64url), ADR-0011 D8 (`ring::rand::SystemRandom` + `ring::constant_time::verify_slices_are_equal`) | `issue_token` → 32 bytes from `SystemRandom`, base64url 인코드, `${XDG_STATE_HOME}/gtmux/<s>.token` 0600 원자적 쓰기. `verify_token(presented, expected)` constant-time 비교 반환. 파일 권한이 0600보다 넓으면 fail-closed exit 5 (D13.3). | 5, 6 |

### §3.4 `http-api` (1건 거대 — step 5·7 차단자)

| ID | 파일·함수 | 계약 | 수용 기준 | C4 step |
|----|-----------|------|-----------|---------|
| P0-HTTP-1 | `codebase/backend/crates/http-api/src/lib.rs::router` (L9) — 라우터 + 미들웨어 체인 + 첫 endpoint 2개 | ADR-0002 D9 (HTTP-only durable), ADR-0003 D6/D11 (Bearer + CSP), R7 §4 (tower-http chain), `docs/ssot/canvas-layout-schema.md` §2 (ETag 정규화 32-hex) | axum `Router` 반환. 라우트: (a) `GET /` → SPA index (frontend/dist 정적 서빙), (b) `GET /api/layout` → `{groups:[],panels:[]}` 초기값 + `ETag: "<32-hex>"`, (c) `PUT /api/layout` (If-Match 검증, 412 rebase). 미들웨어 체인: Origin allowlist → Host allowlist → Authorization Bearer extract + `auth::verify_token` → CSP `connect-src` 모드별(D14), 쿼리스트링 `?token=` *ongoing auth 거부* (R(rej)2). | 5, 7 |
| **P0-HTTP-2** | `codebase/backend/crates/http-api/src/lib.rs::bootstrap_handler` — *one-shot bootstrap exchange endpoint* (C5 B1 추가) | ADR-0003 R(rej)2 예외 절 + D17 c1 (Jupyter `/login?token=` 패턴) | `GET /auth/bootstrap?token=<token>` 처리: (i) `auth::verify_token` 상수시간 비교 → (ii) `Set-Cookie: gtmux_session=<token>; HttpOnly; Secure; SameSite=Strict; Path=/` + (iii) `302 /` 리다이렉트. 토큰 미일치 시 401. **access log redaction** 미들웨어가 `?token=*` 마스킹 (D9). 부팅 콘솔 URL 출력은 본 endpoint를 가리킨다. | 5 (precondition) |

### §3.5 `ws-server` (1건 — step 6 차단자)

| ID | 파일·함수 | 계약 | 수용 기준 | C4 step |
|----|-----------|------|-----------|---------|
| P0-WS-1 | `codebase/backend/crates/ws-server/src/lib.rs::router` (L9) — `/ws` 업그레이드 + envelope codec skeleton | ADR-0002 D1 단일 endpoint, D5 subprotocol echo, ADR-0003 D5 close 1008, R7 §5 tokio-tungstenite | `GET /ws` upgrade. `Sec-WebSocket-Protocol` 헤더 파싱 → 콤마-구분 두 값 `gtmux.v1` + `bearer.<token>` 추출 → `auth::verify_token` 상수시간 비교. 통과 시 응답 헤더는 `Sec-WebSocket-Protocol: gtmux.v1`만 (bearer.* echo 금지). 실패 시 1008 close + log redaction. Origin/Host 검증은 미들웨어로 공유. | 6 |

### §3.6 `mux-router` (1건 — step 8 (manual)·9 보조 차단자)

| ID | 파일·함수 | 계약 | 수용 기준 | C4 step |
|----|-----------|------|-----------|---------|
| P0-MUX-1 | `codebase/backend/crates/mux-router/src/lib.rs::connect` (L43) + `Command` argv builder | ADR-0001 D1·D3·D5·D7 (control mode attach, bootstrap order, FIFO command queue, %output decode → ring buffer), ADR-0008 allowlist (이미 enum으로 차단됨) | `tmux -L gtmux-<s> -C attach -t <s>` spawn → list-sessions / list-windows / list-panes 스냅샷 → `refresh-client -B` 구독 → `%output` 디코드 → per-pane 128 KB ring buffer (D15). `Command::SendKeys`·`KillPane`·`NewWindow` argv builder 함수 추가(shell 비경유). | 8 (manual), 첫 pane 입출력 |

### §3.7 `config` (1건 — step 3 보조)

| ID | 파일·함수 | 계약 | 수용 기준 | C4 step |
|----|-----------|------|-----------|---------|
| P0-CFG-1 | `codebase/backend/crates/config/src/lib.rs::load` (L37) + 4 struct 본문 | grill D22 schema(`schema_version`/`server`/`runtime`/`security`/`cloud`), `${XDG_CONFIG_HOME}/gtmux/<s>.config.toml` | figment chain: 기본값 → TOML(`<s>.config.toml`) → ENV(`GTMUX_*`) → CLI flag. `#[serde(deny_unknown_fields)]` 활성화. `[server].port`가 1:1:1 SSoT(ADR-0007 O2). | 3 |

### §3.8 `frontend` (3건 — step 8 차단자, step 5/6/7은 백엔드 측만으로도 PASS 가능)

| ID | 파일·함수 | 계약 | 수용 기준 | C4 step |
|----|-----------|------|-----------|---------|
| P0-FE-1 | `codebase/frontend/src/lib/ws/client.ts::connect` (L4) | ADR-0002 D5, ADR-0003 D5, R8 §F4 | `new WebSocket(url, ['gtmux.v1', 'bearer.<token>'])`. binaryType='arraybuffer'. close code 1008/4001 → banner store 갱신(`stores/connection.svelte.ts`). 자동 재연결(지수 백오프). | 8 |
| P0-FE-2 | `codebase/frontend/src/lib/ws/dispatcher.svelte.ts` (현 9 LOC) | ADR-0002 D2 envelope 표, R8 §F4 main-thread dispatcher | `[1B type][varint paneId|0][payload]` decode. `0x02 PANE_OUT` → `handlers.get(paneId)(buf, cb)`, `0x80 LAYOUT_CHANGED` → `http/layout.ts::getLayout()` trigger, `0x81..0x84` → stores 갱신. | 8 |
| P0-FE-3 | `codebase/frontend/src/lib/canvas/XtermHost.svelte` + `Canvas.svelte` | R8 §F1 xterm wrapper, ADR-0012 D7 + R3 `@xyflow/svelte` | `XtermHost`의 `$effect`에서 `Terminal` 생성 → `term.open(div)` → `registerPaneOut(paneId, (b,cb)=>{term.write(b,cb)})`. `Canvas.svelte`는 `panels.svelte.ts` store에서 노드 mapper로 `SvelteFlow` 렌더. 1개 pane이 화면에 표시되고 키 입력 echo. | 8 |

### §3.9 우선순위 그래프

```
P0-AUTH-1 ─┐
P0-CFG-1  ─┼─→ P0-LIFE-1 ─→ P0-CLI-1 ─┬─→ step 3 PASS
P0-MUX-1  ─┘                          ├─→ step 4 PASS (tmux external attach)
                                      │
P0-AUTH-1 + P0-HTTP-1 (Bearer mw)    ─┴─→ step 5 PASS
P0-AUTH-1 + P0-WS-1                  ───→ step 6 PASS
P0-HTTP-1 (/api/layout + ETag)       ───→ step 7 PASS
P0-FE-1·2·3 + P0-MUX-1 (output flow) ───→ step 8 PASS (manual)
P0-LIFE-2 + P0-CLI-3                 ───→ step 9 PASS
```

따라서 **임계 경로 = P0-LIFE-1 → P0-CLI-1** (step 3 해제가 step 4·5·6·7·9 검증의 *시동 조건*). step 3을 제외한 모든 GATE 단계는 step 3이 살아나야 비로소 *실행* 자체가 가능해진다.

## §4. 1단계 진입 권고

sketch §15 1단계(엔진 연결 검증) 정식 진입 조건은 본 보고서 §2 9단계 중 step 3·5·6·7·9가 PASS여야 한다(step 8은 manual·시각 검증이라 1단계 closeout보다는 후속 P1 UX 폴리시 단계로 분리 가능). 현 시점의 권고는 다음 두 갈래로 나뉜다.

1. **C4 DoD 관점**: plan §3 C4 DoD는 *"실패 단계가 있으면 그 단계의 fix를 P0 작업으로 발행"*이다. 본 보고서가 §3에서 7건의 GATE/N/A를 P0-CLI/LIFE/AUTH/HTTP/WS/MUX/CFG/FE 4튜플로 발행함으로써 **C4 DoD는 충족**된다. 단, 9단계 *실측 PASS*는 §3 작업이 완료될 때까지 미루어진다.

2. **sketch §15 1단계 진입 관점**: 1단계 success criteria(엔진 연결 검증)는 step 3·4·5·6·7의 실측 PASS를 전제로 한다. 따라서 **현재로서는 1단계 진입 미허용**. 다만 임계 경로(§3.9)가 좁고 명확하므로, 권장 순서는:

   - **Sprint 0 (병렬 가능)**: P0-AUTH-1, P0-CFG-1, P0-MUX-1 (외부 의존이 없는 leaf 작업).
   - **Sprint 1**: P0-LIFE-1 + P0-CLI-1 (step 3 GATE 해제 → step 4 자동 PASS).
   - **Sprint 2 (병렬)**: P0-HTTP-1 (step 5·7), P0-WS-1 (step 6), P0-LIFE-2 + P0-CLI-3 (step 9).
   - **Sprint 3**: P0-FE-1·2·3 (step 8 manual PASS) — 1단계 시연 가능.

   Sprint 0~2가 닫히면 `SMOKE_GATE_RUNTIME=0`으로 본 smoke 스크립트를 재실행해 step 1·2·3·4·5·6·7·9 = 8 PASS / 1 manual을 확정하고, 그 결과를 본 보고서 §2 표에 추기(amendment block)한 뒤 C5 정합 리뷰(`docs/reports/0013-bootstrap-coherence-review.md`)를 거쳐 1단계 진입을 허가하는 흐름이 자연스럽다.

3. **위험 메모**: smoke 스크립트가 채택한 `Authorization: Bearer` 헤더 경로는 plan 원문의 `?token=` URL 경로를 ADR-0003 R(rej)2(쿼리스트링 토큰 금지)에 정합하도록 *조용히* 보정한 결과다. 만약 1단계 시연 UX에서 사용자 친화적 1회 URL 진입 흐름이 필요하다면, ADR-0003 D21 c1의 *cookie-issuance handshake* (URL의 토큰을 받아 HttpOnly cookie를 set하고 즉시 URL에서 토큰을 제거) 별도 endpoint를 plan에 명시 추가하기를 권한다. 본 보고서는 그 결정을 C5 정합 리뷰 또는 후속 ADR(예: ADR-0013 first-run UX flow)에 위임한다.

## §5. 변경 이력

- 2026-05-14: 초안 (Task C4, commit `3af3abe` HEAD 기준).
