# 0037 — Backend Review Action Items

- 일자: 2026-05-15
- 작성자: Codex review
- 대상: backend 구현 agent
- 종류: 리뷰 후속 작업 문서
- 기준 상태: Stage 5-D P1 일부 구현이 반영된 작업트리
- 관련 문서:
  - `docs/reports/0034-stage-5-ab-ws-envelope-be-progress.md`
  - `docs/reports/0035-be-fe-coordination-stage-5.md`
  - `docs/reports/0036-frontend-review-action-items.md`
  - `docs/plans/0007-multi-session-pivot.md`
  - `docs/adr/0020-auth-lifecycle.md`
  - `docs/adr/0021-terminal-pool-and-mirror.md`

---

## 0. 목적

본 문서는 현재 backend 변경분을 다시 리뷰한 결과와, backend 구현 agent 가 다음 batch 에서 처리해야 할 항목을 정리한다.

중요한 전제:

- 현재 작업트리에는 `0x87 TERMINAL_LIST_UPDATE` 의 **P1 경로** 일부가 들어와 있다.
- `attach_confirm -> publish_terminal_list_change -> WS 0x87 fan-out` 경로는 큰 방향이 `0035` §7.2 와 일치한다.
- 그러나 `WS cookie auth additive`, `POST /api/sessions/:name/terminals`, `0x86 MOUNT_CASCADE`, `5-C connection_id 기반 echo-minus-sender`, `session-scoped terminal streaming` 은 아직 후속 구현 범위다.
- 본 문서는 후속 계획과 충돌하지 않도록, 당장 고칠 결함과 계획된 미구현 항목을 분리한다.

---

## 1. 현재 backend diff 요약

현재 backend working tree 기준 변경 파일:

| 파일 | 변경 요약 |
|---|---|
| `codebase/backend/crates/http-api/src/sessions.rs` | `attach_confirm_handler` 성공 spawn batch 후 `hub.publish_terminal_list_change(&name, &spawned, &[])` 호출 |
| `codebase/backend/crates/http-api/src/lib.rs` | `attach_confirm` 이 terminal-list-change 를 publish 하는지 검증하는 테스트 2개 추가 |
| `codebase/backend/crates/ws-server/src/hub.rs` | `TerminalListChangeEvent`, broadcast channel, publish/subscribe API, 단위 테스트 추가 |
| `codebase/backend/crates/ws-server/src/lib.rs` | `FrameType::TerminalListUpdate = 0x87`, WS select arm fan-out, client-origin 0x87 reject, codec 테스트 갱신 |
| `codebase/backend/crates/ws-server/src/payload.rs` | `encode_terminal_list_update()` 및 payload shape 테스트 추가 |

---

## 2. 리뷰 결론

### 2.1 현재 패치에서 해결된 항목

이전 리뷰에서 지적한 "0x87 publisher 가 attach_confirm 에 연결되지 않았다" 문제는 현재 작업트리에서 해결됐다.

근거:

- `attach_confirm_handler` 가 `spawned` 를 모은 뒤, 비어 있지 않을 때 `hub.publish_terminal_list_change(&name, &spawned, &[])` 를 호출한다.
- `ws-server` 의 `handle_socket` 이 `terminal_list_change_rx.recv()` arm 을 갖고 있고, cookie 가 trigger session 과 다를 때만 `FrameType::TerminalListUpdate` 를 송신한다.
- `gtmux-http-api` 테스트에 spawn 성공 시 publish, spawn 없음 시 no-publish 검증이 추가됐다.

### 2.2 남은 주요 결함/작업

| 우선순위 | 항목 | 성격 | 처리 시점 |
|---|---|---|---|
| P0 | WS cookie auth additive 미구현 | 현재 사용자 흐름 차단 가능 | 즉시 |
| P1 | 0x87 WS fan-out 통합 테스트 부족 | 회귀 방지 부족 | 0x87 merge 전 |
| P1 | session-scoped terminal streaming/catch-up 미구현 | 계획된 Stage 5-C/FE-NEW-6 범위 | 5-C/5-D 다음 batch |
| P1 | `POST /api/sessions/:name/terminals` + 0x86 `MOUNT_CASCADE` 미구현 | 계획된 5-D P2 범위 | 5-D P2 |
| P2 | docs/comments 의 auth 설명 일부 outdated | 구현 혼동 가능 | D10-alpha 구현 시 같이 정리 |

---

## 3. Finding A — WS cookie auth additive 미구현

### 3.1 심각도

- Severity: P0
- Confidence: 9/10
- 종류: 실제 사용자 흐름 차단 가능

### 3.2 증상

HTTP API 는 이미 cookie 인증을 허용한다. 사용자가 `/auth/login` 또는 `/auth?token=...` 경로를 통해 `gtmux_auth` cookie 를 받은 뒤 `/api/*` 를 사용할 수 있다.

하지만 WebSocket `/ws` 는 여전히 `sec-websocket-protocol: gtmux.v1, bearer.<token>` 를 필수로 요구한다. password 기반 로그인 사용자는 bearer token 을 프론트에서 알 수 없으므로, cookie 만으로는 WS upgrade 를 통과할 수 없다.

관련 위치:

| 파일 | 위치 | 내용 |
|---|---:|---|
| `codebase/backend/crates/ws-server/src/lib.rs` | `ws_handler` | subprotocol header 필수 |
| `codebase/backend/crates/ws-server/src/lib.rs` | `ws_handler` | `parsed.bearer_token` 없으면 401 |
| `codebase/backend/crates/ws-server/src/lib.rs` | `ws_handler` | cookie 는 disconnect routing 용으로만 사용한다는 주석 |
| `codebase/backend/crates/http-api/src/auth.rs` | `authenticate()` | HTTP API 는 bearer 또는 cookie 를 모두 허용 |
| `codebase/backend/bin/gtmux-cli/src/main.rs` | `build_router()` | HTTP router 와 WS router 를 별도로 만든 뒤 merge |

### 3.3 왜 문제인가

`0035` §7.3 의 FE 결정은 다음 순서다.

```text
α: BE 단독으로 cookie auth additive 즉시 land
β: Stage 6 cookie-first + bearer fallback
γ: Stage 7 cookie-only cleanup
```

현재 backend 는 α 단계도 아직 구현하지 않았다. 이 상태에서는 session auth page 의 password mode 또는 cookie-only client 가 terminal streaming/WebSocket 기능을 사용할 수 없다.

### 3.4 구현 지침

목표는 **additive** 다. bearer path 를 제거하면 안 된다.

필수 동작:

```text
WS upgrade auth:
  1. gtmux.v1 subprotocol 은 계속 요구한다.
  2. bearer.<token> 이 있고 valid 하면 허용한다.
  3. bearer 가 없거나 invalid 하더라도, gtmux_auth cookie 가 valid 하면 허용한다.
  4. 둘 다 invalid 이면 401.
  5. response subprotocol 은 계속 gtmux.v1 만 echo.
```

구현 설계는 다음 중 하나를 택한다.

#### Option A — auth session table 을 auth crate 로 이동

- `SessionTable`, cookie validate 로직을 `gtmux-auth` crate 로 옮기거나 public API 로 분리한다.
- `gtmux_ws_server::router(...)` 에 cookie validator/session table 을 넘긴다.
- 장점: HTTP/WS 가 같은 auth primitive 를 공유한다.
- 단점: crate 이동 범위가 다소 큼.

#### Option B — ws-server 에 async cookie validator 주입

- `gtmux_ws_server::router(...)` 가 token 외에 cookie validator trait/closure 를 받는다.
- `gtmux-cli` 의 `build_router()` 에서 `AppState.session_table` 기반 validator 를 넘긴다.
- 장점: ws-server 가 http-api 에 의존하지 않는다.
- 단점: async trait/closure 타입 설계가 필요하다.

#### Option C — `/ws` route 를 http-api 쪽으로 소유권 이전

- `http-api` 가 `AppState` 를 가진 상태로 `/ws` upgrade auth 를 수행하고, 인증 이후 ws-server 의 socket loop 를 호출한다.
- 장점: cookie auth 접근이 쉽다.
- 단점: ws-server public surface 재설계가 필요하고 변경 범위가 큼.

권장: **Option B**. 현재 crate 경계를 크게 흔들지 않으면서 D10-alpha 를 구현할 수 있다.

### 3.5 주의 사항

- Stage 7 전까지 bearer path 를 제거하지 않는다.
- `gtmux.v1` subprotocol requirement 는 유지한다. cookie auth 는 인증 credential 의 추가 경로이지 wire protocol negotiation 폐기가 아니다.
- cookie 검증 성공 시에도 기존 `cookie_value` capture 는 유지해야 한다. disconnect/heartbeat/session routing 에 필요하다.
- invalid bearer 가 있고 valid cookie 도 있는 경우 정책을 명확히 정해야 한다. `0035` §3.3 의 α 설명은 "subprotocol token > cookie" 를 권장했다. 즉 bearer 가 present-but-invalid 이면 즉시 reject 로 볼지, cookie fallback 을 허용할지 구현 전에 결정해야 한다. 안전한 쪽은 "valid bearer 우선, invalid bearer present 는 reject" 이다. 다만 password-only client 는 bearer 를 아예 보내지 않으므로 문제가 없다.

### 3.6 수용 기준

- bearer only: 기존 valid bearer WS upgrade 성공
- bearer wrong: WS upgrade 실패
- cookie only: valid `gtmux_auth` cookie 로 WS upgrade 성공
- no bearer/no cookie: WS upgrade 실패
- response `sec-websocket-protocol` 은 `gtmux.v1`
- 기존 disconnect sink / heartbeat sink 동작 유지

### 3.7 테스트 추가 권장

기존 ws-server socket 테스트에 다음 케이스를 추가한다.

- `ws_upgrade_cookie_only_success`
- `ws_upgrade_no_auth_fails`
- `ws_upgrade_wrong_bearer_valid_cookie_policy`

테스트 구조상 ws-server 단독으로 cookie validate 를 할 수 없으면, 새 validator injection 을 테스트용 fake validator 로 검증한다.

---

## 4. Finding B — 0x87 fan-out 통합 테스트 부족

### 4.1 심각도

- Severity: P1
- Confidence: 8/10
- 종류: 회귀 방지 부족

### 4.2 현재 상태

현재 테스트는 다음을 검증한다.

- `payload::encode_terminal_list_update()` shape
- `Hub::publish_terminal_list_change()` / `subscribe_terminal_list_change()`
- `attach_confirm_handler()` 가 spawn 성공 시 hub event 를 publish
- spawn 이 없으면 publish 하지 않음

하지만 아직 다음 end-to-end 경로는 테스트하지 않는다.

```text
Hub::publish_terminal_list_change
  -> handle_socket terminal_list_change_rx arm
  -> cookie_value
  -> hub.session_for_cookie(cookie)
  -> trigger session skip
  -> other session receives 0x87 envelope
```

### 4.3 왜 문제인가

0x87 의 핵심은 payload encode 자체가 아니라 **per-connection routing** 이다. 현재 결함이 생길 수 있는 지점은 다음이다.

- trigger session skip 이 깨져 같은 session 에도 중복 refresh 발생
- unattached cookie 에도 0x87 송신
- 다른 session 에 fan-out 되지 않음
- client-origin 0x87 reject 정책과 server-origin 0x87 송신 정책이 섞임

이 경로는 hub 단위 테스트만으로는 잡히지 않는다.

### 4.4 테스트 지침

가능하면 ws-server socket 테스트에 다음을 추가한다.

```text
terminal_list_update_sent_to_other_session_only
  - Hub 생성
  - cookie-A -> session "alpha"
  - cookie-B -> session "beta"
  - WS A 연결: Cookie gtmux_auth=cookie-A
  - WS B 연결: Cookie gtmux_auth=cookie-B
  - hub.publish_terminal_list_change("alpha", ["uuid-1"], [])
  - A 는 0x87 을 받지 않음
  - B 는 0x87 을 받고 payload added=["uuid-1"], removed=[]
```

추가 케이스:

- cookie 없는 WS 연결은 0x87 을 받지 않음
- cookie 는 있지만 `session_for_cookie` 가 없는 연결은 0x87 을 받지 않음
- client 가 0x87 을 보내면 policy violation close

### 4.5 주의 사항

이 테스트는 현재 로컬 sandbox 에서 `TcpListener::bind(127.0.0.1:0)` 권한 문제로 실패할 수 있다. CI 또는 권한 있는 로컬 환경에서 실행하는 것을 기준으로 둔다.

---

## 5. Planned Work C — session-scoped terminal streaming/catch-up

### 5.1 상태

- Severity: P1, but planned
- Confidence: 9/10
- 성격: 아직 구현되지 않은 Stage 5-C / FE-NEW-6 범위

현재 WS catch-up 은 모든 alive pane 을 모든 WS 연결에 보낸다.

```text
backend.pane_ids()
  -> pane-spawned NOTIFY
  -> PANE_OUT replay
```

live output 도 `pane_output` broadcast 를 모든 WS subscriber 에 전달한다.

### 5.2 왜 지금 고치면 안 되는가

이 항목은 단순 guard 하나로 고칠 수 없다. multi-session terminal streaming 은 다음 결정과 함께 들어가야 한다.

- terminal UUID ↔ pane id routing
- panel instance 별 multi-xterm subscriber
- session layout 에 포함된 terminal 만 stream 하는 subscription model
- FE 의 `XtermHost` UUID mode
- legacy single-session path 보존 범위

즉, 현재 0x87 P1 patch 에 억지로 session filtering 을 넣으면 legacy demo path 또는 future P2 endpoint 와 충돌할 수 있다.

### 5.3 구현 방향

Stage 5-C/FE-NEW-6 에서 다음을 함께 설계한다.

- WS connection 이 active session 을 알 수 있어야 한다.
- session layout 의 terminal UUID 목록을 기준으로 해당 pane id 만 stream한다.
- catch-up replay 도 session layout 에 포함된 terminal 로 제한한다.
- legacy bearer/single-session mode 는 `sessionStore.active == null` 대응이 끝날 때까지 별도 path 로 유지한다.

### 5.4 수용 기준

- session A 의 terminal output 이 session B webpage 로 전달되지 않는다.
- 같은 terminal UUID 를 여러 panel 이 mirror 할 때 output fan-out 이 깨지지 않는다.
- dangling/dead terminal 은 0x85 + terminal pool refresh 로 상태가 갱신된다.
- legacy single-session smoke 가 깨지지 않는다.

---

## 6. Planned Work D — 5-D P2 endpoint + 0x86 MOUNT_CASCADE

### 6.1 상태

- Severity: P1, but planned
- Confidence: 9/10
- 성격: `0035` §7.2 에서 결정된 후속 구현

`0035` §7.2 결정:

```text
POST /api/sessions/:name/terminals
  -> BE 가 terminal UUID 생성/spawn
  -> BE 가 layout item 좌표/크기 결정
  -> trigger session 에 0x86 MOUNT_CASCADE
  -> other sessions 에 0x87 TERMINAL_LIST_UPDATE
```

현재 backend 에는 이 endpoint 와 0x86 frame type 이 없다. `0x86` 은 FE decoder 예약 상태이며, BE codec 은 의도적으로 unknown 으로 둔다.

### 6.2 구현 지침

P2 를 구현할 때 한 batch 로 처리한다.

1. `FrameType::MountCascade = 0x86` 추가
2. `payload::encode_mount_cascade(terminal_id, x, y, w, h)` 추가
3. `POST /api/sessions/:name/terminals` 추가
4. cookie 가 해당 session attach lock 을 보유하는지 검증
5. terminal spawn + terminal pool record
6. session layout 에 terminal item 추가 또는 MOUNT_CASCADE 로 FE 에 append 지시
7. trigger session 에 0x86 송신
8. other sessions 에 0x87 송신

### 6.3 좌표 정책

`0035` §7.2 는 MVP 에서 BE 좌표 결정을 선택했다.

권장 기본값:

- empty layout: `x=80`, `y=80`, `w=720`, `h=420`
- existing terminal items: max x/y 기준 `+32` cascade
- request body 에 optional `x/y/w/h/label` 을 허용할 수 있지만, MVP 는 body `{}` 로 시작해도 된다.

### 6.4 주의 사항

- attach_confirm P1 은 이미 layout 에 UUID 가 있으므로 0x86 을 보내지 않는다.
- P2 endpoint 는 fresh session 에서 새 terminal 을 만들기 위한 사용자-facing path 다.
- FE 는 200 응답과 0x86 event 를 모두 받을 수 있으므로 idempotency 를 보장해야 한다.
- BE 가 layout 에 직접 item 을 persist 할지, FE 가 0x86 후 mutateLayout 할지는 구현 전에 한 번 더 정해야 한다. `0035` 의 문구는 "BE 가 default 좌표 결정, FE handleMountCascade 가 server-supplied x/y/w/h 사용"에 가깝다.

---

## 7. Planned Work E — 5-C echo-minus-sender routing

### 7.1 상태

- Severity: P1, but planned
- Confidence: 8/10
- 성격: `0035` §7.1 에서 결정된 후속 구현

FE 결정은 다음이다.

```text
5-C broadcast trigger: (B) echo minus sender
sender identity: BE connection-table 의 connection_id
session_id 위치: top-level
```

현재 hub 는 `cookie -> session_name` table 만 갖고 있다. 이는 0x87 P1 에는 충분하지만, 5-C echo-minus-sender 에는 부족하다.

### 7.2 구현 지침

5-C 에서 추가해야 할 것:

- WS connection 별 `connection_id` 생성
- hub connection table 또는 broadcast sender metadata
- client-origin 0x81~0x84 수신 시 sender connection id 를 제외하고 같은 session 에 fan-out
- outbound payload top-level `session_id`
- FE 의 `isFrameForActiveSession(sessionId)` 와 정합

### 7.3 주의 사항

- 0x87 P1 은 trigger session 전체를 skip 해도 된다.
- 5-C 는 trigger connection 만 skip 해야 한다.
- 따라서 0x87 의 session skip 로직을 5-C 에 그대로 복사하면 안 된다.

---

## 8. Non-Goals / 지금 하지 말아야 할 것

다음 작업은 현재 backend agent 가 임의로 끌어오면 기존 계획과 충돌한다.

- Stage 7 전 bearer subprotocol auth 제거
- 0x86 을 codec 에만 추가하고 publisher/endpoint 없이 방치
- attach_confirm P1 에서 trigger session 으로 0x86 발행
- session-scoped terminal streaming 을 임시 pane id filter 로만 구현
- legacy `/api/layout` 제거
- v1 `pane-spawned` catch-up 제거

특히 legacy `/api/layout` 및 legacy WS path 는 `0035` §7.4 의 FE 완료 조건 이후에 제거한다.

---

## 9. 검증 결과

실행한 명령:

```bash
cargo test -p gtmux-http-api --color=never
cargo test -p gtmux-ws-server terminal_list --color=never
```

결과:

- `gtmux-http-api`: 138 passed / 0 failed
- `gtmux-ws-server terminal_list`: 5 passed / 0 failed

추가 참고:

- `cargo test -p gtmux-ws-server --color=never` 전체 실행은 sandbox 환경에서 `TcpListener::bind(127.0.0.1:0)` 계열 테스트가 `Operation not permitted` 로 실패했다.
- 이는 전체 ws-server compile 실패가 아니라 로컬 sandbox socket 권한 문제로 보인다. CI 또는 권한 있는 환경에서 전체 ws-server 테스트를 다시 확인해야 한다.

---

## 10. Backend 구현 agent 권장 작업 순서

### Batch 1 — D10-alpha

1. WS cookie auth additive 구현
2. bearer-only 기존 테스트 유지
3. cookie-only WS upgrade 테스트 추가
4. outdated auth 주석 갱신

완료 기준: password/cookie login 사용자도 `/ws` upgrade 가능.

### Batch 2 — 0x87 routing hardening

1. 0x87 per-connection fan-out 통합 테스트 추가
2. trigger session skip / other session receive / unattached skip 검증
3. client-origin 0x87 policy violation 테스트 추가

완료 기준: P1 terminal pool update hint 의 routing 회귀 방지.

### Batch 3 — 5-D P2

1. `POST /api/sessions/:name/terminals` endpoint
2. 0x86 `MOUNT_CASCADE` frame/payload
3. trigger session 0x86, other sessions 0x87
4. FE `NewPanelButton` migration 과 coordination

완료 기준: empty session 에서 새 terminal 을 만들 수 있음.

### Batch 4 — 5-C

1. connection_id table
2. 0x81~0x84 top-level `session_id`
3. echo-minus-sender fan-out
4. FE dispatcher session guard wire-up 과 coordination

완료 기준: selection/viewport/focus/input-target 가 multi-tab/session scoped 로 동기화됨.

### Batch 5 — session-scoped terminal streaming

1. terminal UUID 기반 subscription model
2. session layout 기반 catch-up/output 제한
3. multi-xterm subscriber 정합
4. legacy path deprecation 준비

완료 기준: session 간 terminal output leakage 가 없고, multi-session terminal panel 이 실제 xterm 으로 동작함.

