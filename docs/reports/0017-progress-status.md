# 보고서: 현재 진행 상태 (Sprint 0~4 종료 시점) 및 데모 시연 가이드

- 일자: 2026-05-14
- 작성: PM
- 상태: **활성 스냅샷** — Sprint 0~4 closeout 직후의 단일 리포트. sketch §15 **1단계 (엔진 연결 검증) 정식 통과** 시점을 기록하고, *지금 시연 가능한 범위*와 *2단계 진입 직전의 갭*을 명문화한다. 후속 결정이 ADR/SSoT로 흡수되면 본 문서는 폐기 가능.
- 검토 범위: `docs/sketch.md` §15, `CONTEXT.md`, `docs/plans/0001/0002`, `docs/adr/0001~0012` (12개 Accepted), `docs/ssot/*` (3개), `docs/reports/0001~0016`, `codebase/` 인벤토리, smoke 9-step 실측 (commit `b4900ad`).

## 요약 (3문장)

12개 ADR Accepted + 3개 SSoT + R1~R8 보고서 + Sprint 0~4 코드 (backend 184 unit + 5 ignored, frontend 221 svelte-check 0/0, 12개 frame WS round-trip byte-equal, mux→WS Hub broadcaster + per-pane ring buffer 128 KiB + 역방향 client→tmux 라우팅 + LIFE-3 server pidfile + `gtmux stop` graceful + SPA 정적 서빙) 가 모두 main에 정착했고, **smoke 9-step 자동화 검증이 0 GATE로 통과** (step 8만 의도된 MANUAL N/A) — sketch §15 1단계는 *기계적으로 통과*. 데모 시연은 **CLI 수명주기 + 인증 흐름 + SPA 로드 + WS 핸드셰이크 + smoke 9-step** 다섯 묶음 모두 즉시 가능하고, **실제 tmux pane을 캔버스 위에서 시각/입력**하는 end-to-end 라이브 데모는 panel 등록 트리거(현재 PUT API 또는 환경변수)만 추가하면 가능. 차단성 갭은 **0건** (cors_origins 기본값 미설정은 데모 운영 갭이며 ADR-0003 정합 자체는 유지).

## 1. 진행 매트릭스 (Sprint 0~4 전체)

| Phase | 산출 | 상태 | 정착 commit |
|---|---|---|---|
| Grill D1~D23 + ADR ×9 + SSoT ×3 + Code skeleton | 12-decision + 9-ADR + 3-SSoT + 40-파일 빌드 가능 골격 | ✅ | `894ab69`까지 |
| **Sprint 0** (P0-AUTH-1 / P0-CFG-1 / P0-MUX-1) | ring 0.17 CSPRNG + figment + winnow parser + 256-LUT 8진수 디코더 | ✅ | `4a010a0` |
| **Sprint 1** (P0-LIFE-1 / P0-CLI-1) | `TmuxDaemon::spawn/attach/read_line/write_line/shutdown` + D20 1~13단계 wiring | ✅ | `951750b` |
| **Sprint 2** (HTTP-1+2 / WS-1 / LIFE-2 / CLI-3+4+5) | axum + tower-http + ETag 32-hex + Sec-WebSocket-Protocol + teardown 5단계 | ✅ | `da19003` |
| **Sprint 3** (FE-1 / FE-2 / FE-3) | WS dispatcher + Canvas+xterm + reconnect banner (D21 c2/c3) | ✅ | `80675d6` |
| **Sprint 4-A** (ADR-0004/0005/0006) | xterm.js v6 + `@xyflow/svelte` v1.5 + plain JSON + atomic-write-file | ✅ | `c0710b1` |
| **Sprint 4-B** (WIRE-1+2+3+4) | SSoT byte-level lock + Hub broadcaster + per-pane ring buffer + reverse routing | ✅ | `de73005` |
| **Sprint 4-C** (FE-1~5) | XtermHost.onData→PANE_IN / FitAddon→PANE_RESIZE / NOTIFY_MIRROR/LAYOUT_CHANGED dispatcher | ✅ | `04b4ec0` |
| **Sprint 4-D LIFE-3** | server pidfile (XDG_STATE_HOME) + `gtmux stop` graceful SIGTERM 5s + `--force` | ✅ | `e5dd63f` |
| **Sprint 4-D SMOKE + SPA** | bundled SPA static serving (ServeDir+ServeFile fallback) + smoke 9/9 PASS | ✅ | `b4900ad` |
| sketch §15 1단계 (엔진 연결 검증) | 9-step smoke (1 N/A 시각 검증 제외) | **✅ 통과** | `b4900ad` |
| sketch §15 2단계 (기본 UI 워크스페이스) | 진입 직전 — carry-forward 정리 + drag/drop UX 구축 | ⏳ **다음** | — |
| GitHub push (`iiamaii/gtmux`) | macOS keychain credential 충돌 | ⏳ | 사용자 영역 |

## 2. 데모 시연 가능 범위 (실질적 사용 수준)

### 2.1 즉시 시연 가능 — 추가 wiring 없이 *지금* 동작

| # | 시연 항목 | 검증 방식 |
|---|---|---|
| D1 | **CLI 수명주기** — `gtmux start`/`status`/`stop`/`teardown`/`rotate-token` | smoke step 3·9, 또는 직접 명령 |
| D2 | **인증 흐름** — `?token=` URL → `/auth/bootstrap` → cookie 발급 + `/` redirect | 브라우저 navigation 또는 curl |
| D3 | **SPA 정적 서빙** — `GET /` 200 + `<div id="app">` 포함 HTML | smoke step 5, browser navigation |
| D4 | **HTTP `/api/layout`** — GET·PUT + ETag (32-hex) + If-Match 412 | smoke step 7 + integration tests |
| D5 | **WebSocket 핸드셰이크** — `Sec-WebSocket-Protocol: gtmux.v1, bearer.<token>` → 101 + 서버는 `gtmux.v1`만 echo (bearer.* never echoed) | smoke step 6 + 19 ws-server tests |
| D6 | **tmux 외부 attach** — `tmux -L gtmux-<s> -S <sock> attach` 로 동일 세션 접속 (multi-mirror) | smoke step 4 |
| D7 | **D6 5단계 teardown** — daemon kill + socket + token + pid + (optional) config | smoke step 9 + 2 teardown tests |
| D8 | **D21 c2 1s grace + c3 zombie 배지** — backend SIGTERM 후 client banner 1s 지연 + 재연결 | frontend ReconnectBanner unit + manual probe |
| D9 | **WS Hub broadcast + per-pane ring buffer** — 새 attach 시 catch-up replay (128 KiB cap) | ws-server 70 tests (`catch_up_replay_on_new_attach`, `ring_buffer_oldest_drop`) |
| D10 | **자동화 smoke 9/9 PASS** — CI에서 `SMOKE_GATE_RUNTIME=0 ./codebase/smoke/01_engine_connect.sh` | commit `b4900ad` 직후 실측 |

### 2.2 데모 시연 절차 (수동 시연용 5-step)

```bash
# 1. 빌드 (1회)
make -C codebase build      # cargo build + vite build
make -C codebase codegen    # OpenAPI + TS 타입 생성 (선택)

# 2. 서버 기동
export GTMUX_FRONTEND_DIST=$PWD/codebase/frontend/dist
export GTMUX_SERVER__SESSION=demo
export GTMUX_SERVER__PORT=9988
export GTMUX_SERVER__BIND=127.0.0.1
# same-origin fetch를 허용하려면 cors_origins에 SPA origin을 명시 — §3.2 갭 G1
export GTMUX_SECURITY__CORS_ORIGINS='["http://127.0.0.1:9988"]'
export GTMUX_SECURITY__HOST_ALLOWLIST='["127.0.0.1:9988"]'

./codebase/backend/target/debug/gtmux start --session demo --port 9988
# stdout banner 가 출력:
#   Open URL:    http://127.0.0.1:9988/auth/bootstrap?token=<base64url>
#   Tmux socket: /tmp/gtmux-501/demo.sock
#   Tmux daemon: detached (label gtmux-demo), gtmux pid=<N>

# 3. 브라우저에서 banner URL 열기
#   - /auth/bootstrap?token=... → SameSite=Strict HttpOnly cookie 발급 → 302 /
#   - SPA 로드 + WS connect (subprotocol 'gtmux.v1, bearer.<token>')

# 4. 다른 터미널에서 같은 tmux session 외부 attach (mirror 확인)
tmux -L gtmux-demo -S /tmp/gtmux-501/demo.sock attach
#   → 같은 tmux session 안에 두 클라이언트 (gtmux + 일반 tmux) 가 mirror
#   → 외부에서 `echo hello` 입력 → WS PANE_OUT 으로 브라우저 측 (panel 등록 후)
#     xterm 에도 도착

# 5. 세션 정리
./codebase/backend/target/debug/gtmux stop --session demo        # server graceful
./codebase/backend/target/debug/gtmux teardown --session demo --force  # daemon kill + cleanup
```

### 2.3 라이브 end-to-end 데모를 위해 *추가로* 필요한 wiring (제한)

위 절차에서 **panel 1개를 캔버스에 등록하면** tmux ↔ xterm bidirectional 라이브 데모가 즉시 동작 (`PANE_OUT` byte→`term.write`, `term.onData`→`PANE_IN` 모두 wired). 단 현 상태에서 panel 등록 UI(drag/drop/create) 가 frontend에 미존재 — 다음 절차 중 하나가 필요:

- **옵션 A (curl 한 줄)**: `PUT /api/layout`으로 panel 1개 inject. SSoT canvas-layout-schema 정합 payload + `If-Match` ETag 헤더. `pane_id`는 `tmux -L gtmux-demo -S /tmp/gtmux-501/demo.sock list-panes -F "#{pane_id}"` 결과의 `%N` 중 N 값.
- **옵션 B (frontend FE-quick UX)**: 다음 sprint에서 추가될 "New Panel" 버튼 — 1줄 panel append + PUT. Sprint 5 첫 task 후보.

따라서 *2026-05-14 현재* "100% no-extra-step 데모"는 §2.1의 D1~D10 까지, *manual 1-curl 추가* 데모는 라이브 tmux↔xterm 까지 가능.

### 2.4 자동화 smoke 시연 (CI/회귀 검증)

```bash
SMOKE_GATE_RUNTIME=0 ./codebase/smoke/01_engine_connect.sh
```

출력:
```
PASS  step 1  make build
PASS  step 2  make codegen
PASS  step 3  daemon socket=/tmp/gtmux-501/smoke.sock token-file=<...>/smoke.token
PASS  step 4  tmux external attach reachable
PASS  step 5  SPA index served with Bearer auth
PASS  step 6  WS handshake verified via python fallback
PASS  step 7  /api/layout returned empty schema + ETag
N/A   step 8  MANUAL visual probe — Playwright/Cypress carry-forward
PASS  step 9  teardown removed socket/token/pid/layout/config
```

## 3. 알려진 갭 / 제약 (2단계 진입 전 정리 대상)

### 3.1 차단성 (블로커) — **0건**

5대 불변식 위반 0건. ADR 위반 0건. 12개 frame WS round-trip byte-equal 검증. clippy `-D warnings` clean. 테스트 PASS.

### 3.2 데모 운영 갭 (G1~G6) — *시연 직전* 처리 권고

| ID | 갭 | 영향 | 권고 처리 |
|---|---|---|---|
| **G1** | `cors_origins` 기본값이 빈 셋 | same-origin fetch도 `origin_forbidden` 거절 (브라우저 navigation 은 Origin 헤더 부재로 통과하나 SPA의 `fetch('/api/layout')` 는 차단) | `gtmux start` 시 `GTMUX_SECURITY__CORS_ORIGINS='["http://<bind>:<port>"]'` env, 또는 ADR-0003 D3 정합 하에 `effective_cors_origins` 합성 헬퍼 (bind+port 자동 동치 추가) 도입. Sprint 5 첫 task 후보. |
| **G2** | panel 등록 UI 부재 | end-to-end 라이브 데모에 manual curl 또는 별도 도구 필요 | Sprint 5에서 "New Panel" 버튼 + drag-to-create UX 추가. sketch §12 P0 "canvas panel placement". |
| **G3** | `LayoutSnapshot` in-memory 만 — JSON 파일 영속화 미연결 | server restart 시 layout 손실 (sketch §15 3단계 미진입이므로 의도된 상태) | ADR-0006 implement task (P0-LAYOUT-STORAGE-1) — Sprint 5/6 중 결정. |
| **G4** | `mux-router::Command::ResizeWindow` 변형 부재 | cmd_router가 `Command::ListWindows`에 임시 park + serialiser keyword override | mux-router에 정식 `ResizeWindow { window_id, cols, rows }` 추가. ws-server `cmd_router::build_pane_resize_request` 갱신. 1 PR. |
| **G5** | NOTIFY_MIRROR의 7개 kind (window-add/renamed/close, session-changed, layout-change, subscription-changed, pane-mode-changed) console.debug 만 | mux mirror store 미구축 — UI 가 외부 변화 인지 못 함 | `lib/stores/mux.svelte.ts` 신설 + dispatcher에서 routing. Sprint 5 후반. |
| **G6** | TLS / cloud 모드 helper 부재 | Local 데모만 가능 (sketch §15 1~3단계는 Local 한정) | sketch §15 4단계 (sketch §13.1 Cloud) 진입 시 별도 ADR + helper. |

### 3.3 코드 위생 (P1+) — 후속 sprint에서 묶음 처리

- `Arc<Mutex<TmuxDaemon>>` → `tokio::io::split` 분리 (성능 최적화, 측정 후 결정)
- `lifecycle::TeardownReport`에 pidfile 항목 명시 boolean 노출 (현재 `removed`/`absent` boolean 누적)
- `gen-openapi` rustfmt 자동 정렬 noise (이미 흡수됨, info 만)
- 시각 검증 자동화 (Playwright/Cypress) — sketch §15 5단계 (UX 폴리시) prereq
- code-graph 의 "untested" 보고 (현재 score 0.85 with 23~177 test gap) — graph parser tuning 또는 명시 test annotation. **실측 cargo test PASS** 와 별개의 graph 보고 한계.

## 4. 5대 불변식 — 현 시점 평가 (코드 차원)

| # | 불변식 | 평가 | 코드 차원 근거 |
|---|---|---|---|
| 1 | tmux 상태 ↔ web 상태 분리 | **컴파일 강제** | `mux-router::Event`/`Command` enum + `gtmux-canvas-layout` 페이로드(`groups`+`panels`)가 *별도 crate*. tmux 상태는 WS envelope 0x01~0x07로만 흐름, web 상태는 0x80~0x84 + HTTP `/api/layout`만. 12 frame round-trip 테스트 PASS. |
| 2 | tmux-native vs web-only 분기 | **컴파일 강제** | `mux-router::Command` enum이 11 variant (allowlist), `split-window`/`resize-pane`/`select-layout` 자체 부재. `ws-server::cmd_router::COMMAND_ALLOWLIST` 표 정합. |
| 3 | tmux Layout ≠ Canvas Layout | **기계적 보장** | ADR-0008 single-pane convention + WS envelope에 tmux Layout 문자열 슬롯 부재 (NOTIFY_MIRROR `kind: layout-change`는 trigger only, payload는 *불투명 식별자*로 취급). |
| 4 | 보안 기본값 | **컴파일 강제 + 미들웨어** | ADR-0003 D3/D6 middleware chain (Origin/Host/Bearer), Svelte 자동 escape, xterm option flags. 토큰 0600 + 부모 0700, atomic write. ETag SHA256-128. |
| 5 | control mode 사용 | **단일 채널 보장** | `lifecycle::TmuxDaemon::spawn`가 `-L gtmux-<s> -S <sock> -C` argv 고정. `read_line` 루프 + `write_line` 채널이 single-writer. 다른 tmux 호출 경로 없음. |

Sprint 4 종료 시점에 *코드가 5대 불변식을 강제*하는 상태. 회귀 위험은 `code-review-graph` MCP가 추적.

## 5. 메트릭 (Sprint 0~4 종합)

| 항목 | 값 |
|---|---|
| ADRs Accepted | **12** (0001~0012) |
| SSoTs | **3** (wire-protocol, security-defaults, canvas-layout-schema) |
| Reports (R1~R8 + coherence + status + handoff) | **16** (`0001~0017` 본 문서 포함) |
| Backend crates | 6 lib + 2 bin (`auth`/`config`/`mux-router`/`lifecycle`/`http-api`/`ws-server` + `gtmux-cli`/`gen-openapi`) |
| Backend unit tests | **184 passed + 5 ignored** (tmux-binary gate) |
| Frontend `npm run check` | **221 files / 0 errors / 0 warnings** |
| Frontend bundle gzip | main **7.21 KB**, svelteflow 67.63 KB, xterm 91.64 KB, total **166.48 KB** (cap 200 KB main, R8 §F7 PASS) |
| WS frame slots | 12 정의 + 21 reserved (32 슬롯 SSoT 정본) |
| Code graph | 522 nodes / 4805 edges / 162 flows / 10 communities (실측 commit `b4900ad`) |
| Smoke 9-step | **8 PASS / 1 MANUAL N/A / 0 GATE / 0 FAIL** |
| 1단계 prereq ADRs (9개) | 모두 Accepted |
| 2단계 prereq ADRs (ADR-0004/0005) | 모두 Accepted |
| 3단계 prereq ADRs (ADR-0006) | Accepted |

## 6. 후속 권고 (Sprint 5 진입 전)

1. **(즉시)** G1 처리 — `effective_cors_origins` 헬퍼 또는 default `bind+port` 자동 추가. ADR-0003 D3 정합 확인. 1 PR.
2. **(즉시)** G4 처리 — `mux-router::Command::ResizeWindow` 정식 변형. `cmd_router::build_pane_resize_request` + lifecycle `serialise_command` 갱신. 1 PR.
3. **(다음 sprint)** G2 처리 — frontend "New Panel" 버튼 + drag-to-create UX. sketch §12 P0 "canvas panel placement".
4. **(다음 sprint)** G5 처리 — mux mirror store (`lib/stores/mux.svelte.ts`) + dispatcher routing.
5. **(중기)** G3 처리 — ADR-0006 implement task (P0-LAYOUT-STORAGE-1). 영속화 storage 정착 후 sketch §15 3단계 진입.
6. **(중기)** 시각 회귀 자동화 도입 (Playwright/Cypress). smoke step 8 N/A 제거.
7. **(저우선)** GitHub push credential은 사용자 영역 — 안내만 유지.

## 7. 옵션 비교표 — 없음

본 보고서는 스냅샷 산출물.

## 8. gtmux에의 함의 (불변식 검증) — §4 참조

5대 불변식 전부 **코드 차원 컴파일/미들웨어/단일 채널 강제**. 회귀 시 cargo test + clippy 가 1차 검출, code graph가 보조 추적.

## 9. 미해결 / 후속

- §3.2의 G1~G6 권고 처리는 **Sprint 5** 내 진행. 본 보고서가 단일 source.
- §3.3 코드 위생 항목은 Sprint 5 closeout 시 묶음 처리 권장.
- sketch §15 1단계 통과 선언은 `b4900ad` 시점 기록 — 회귀 검출 시 본 commit으로 비교.
- 본 보고서는 sketch §15 2단계 진입 후 Sprint 5 closeout 보고서(`reports/0019-…`)로 갱신/대체 권장.

## 10. 출처 (URL + 접근일자) — 없음, 내부 문서만

- `docs/sketch.md` — 2026-05-14
- `CONTEXT.md` — 2026-05-14
- `docs/adr/0001~0012` (12개 Accepted) — 2026-05-14
- `docs/ssot/{wire-protocol, security-defaults, canvas-layout-schema}.md` — 2026-05-14
- `docs/reports/0001~0016` — 2026-05-14
- `codebase/smoke/01_engine_connect.sh` 실측 (commit `b4900ad`) — 2026-05-14
- `cargo test --workspace --tests` / `npm run check` / `npm run build` 실측 — 2026-05-14

## 11. 변경 이력

- 2026-05-14: 초안 (Sprint 0~4 closeout + sketch §15 1단계 통과 직후 PM 스냅샷)
