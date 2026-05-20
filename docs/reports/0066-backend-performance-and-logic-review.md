# 0066. Backend 성능/로직 충돌 리뷰

작성일: 2026-05-17  
범위: 현재 구현된 backend 코드의 성능 위험, 예외 사용 흐름, session/pane 로직 충돌 가능성  
관련 코드: `codebase/backend/crates/ws-server/`, `codebase/backend/crates/http-api/`, `codebase/backend/crates/pty-backend/`

## 요약

backend의 주요 위험은 “단일 사용자” 전제를 벗어나지 않더라도, 세션/터미널 수와 layout 크기가 늘어날 때 발생한다. 특히 WS catch-up replay, `/api/terminals` 전체 workspace scan, session-scoped input 검증 누락, layout serialization/write lock 범위를 우선 개선해야 한다.

우선순위는 다음 순서가 적절하다.

1. WS catch-up replay를 session scope로 제한
2. `/api/terminals` attach reference 계산 방식 개선
3. client-origin pane input/resize에도 session pane-set 검증 적용
4. layout GET/PUT lock 및 blocking I/O 범위 축소
5. PTY integration test flaky 원인 추적

## BE-1. WS catch-up replay가 모든 pane output을 전송

### 증상

WS 연결 직후 catch-up 단계에서 모든 alive pane의 spawn notify와 ring buffer를 replay한다. session pane-set 필터는 그 이후 live phase에서만 적용된다.

### 근거

- `codebase/backend/crates/ws-server/src/lib.rs`
  - catch-up: `for id in backend.pane_ids()`
  - 각 pane에 대해 `subscribe_output(id)` 후 ring replay
  - session pane-set 계산과 `filter_armed = true`는 catch-up 이후 수행

### 위험 흐름

- session이 여러 개이고 각 session에 terminal이 많음
- terminal ring buffer가 커짐
- 사용자가 새로고침 또는 reconnect를 반복

이 경우 현재 session과 관계없는 pane output까지 브라우저로 전송된다. frontend는 해당 pane subscriber가 없으면 late buffer에 저장하므로, 네트워크/메모리/main-thread 비용이 같이 증가한다.

### 보완 방향

- WS upgrade 후 cookie가 있으면 catch-up 전에 session name과 pane-set을 먼저 계산한다.
- PANE_OUT replay는 해당 session pane-set에 포함된 pane만 보낸다.
- `TerminalSpawned` UUID binding catch-up은 server-wide로 유지할 수 있으나, output replay와 분리한다.
- bearer-only legacy path는 별도 fallback으로 남기되, cookie session path가 기본이어야 한다.

## BE-2. `/api/terminals`가 매 요청마다 모든 session file을 scan

### 증상

terminal list API는 terminal pool/meta snapshot을 만든 뒤, 모든 session layout 파일을 읽고 parse해서 attach reference를 계산한다.

### 근거

- `codebase/backend/crates/http-api/src/terminals.rs`
  - `list_handler`: `scan_session_terminal_refs(wm)` 호출
  - `scan_session_terminal_refs`: `enumerate_sessions`, `std::fs::read`, `serde_json::from_slice`

### 위험 흐름

- frontend terminal pool polling 5초 주기
- terminal died/list update/mount cascade 직후 refresh
- session 수 증가
- free draw/document/image metadata 등으로 layout 파일 크기 증가

요청마다 동기 파일 I/O와 JSON parse가 반복되므로, API latency와 tokio worker 점유 시간이 늘어난다.

### 보완 방향

- terminal UUID → attached session names reverse index를 메모리에 유지한다.
- layout PUT/import/delete 시 index를 갱신한다.
- 단기 보완으로는 `terminalPool.refresh()` in-flight dedupe/debounce와 backend `spawn_blocking` 적용을 검토한다.
- API 응답에 attach count가 반드시 최신이어야 하는지, eventual consistency를 허용할지 ADR/SSoT에서 명확히 한다.

## BE-3. client-origin PANE_IN/PANE_RESIZE의 session scope 검증 누락

### 증상

server-to-client PANE_OUT은 session pane-set으로 필터링하지만, client-to-server `PANE_IN`과 `PANE_RESIZE`는 pane id만 decode한 뒤 바로 backend에 전달한다.

### 근거

- `codebase/backend/crates/ws-server/src/lib.rs`
  - live output filter: `session_pane_set`에 포함되지 않은 pane output drop
  - `handle_client_envelope`: `PaneInput`, `PaneResize`에서 session membership 확인 없이 `backend.send_input`, `backend.resize`

### 위험 흐름

- stale client가 이전 session의 pane id를 계속 들고 있음
- frontend 버그로 잘못된 pane id 전송
- 수동/악성 WS client가 같은 cookie로 임의 pane id에 입력 전송

단일 사용자 앱이라 권한 침해 범위는 제한적이지만, “현재 session에 연결된 pane만 조작한다”는 UX/로직 경계와 충돌한다.

### 보완 방향

- `handle_client_envelope`에 session pane-set 또는 membership checker를 전달한다.
- `PANE_IN`, `PANE_RESIZE`, `PANE_PAUSE`, `PANE_RESUME` 모두 동일한 membership 검증을 적용한다.
- 실패 시 close보다 frame drop + debug/audit counter가 UX상 적절한지 결정한다.

## BE-4. layout GET/PUT의 lock 및 blocking I/O 범위

### 증상

layout GET은 read lock을 잡은 상태에서 canonical JSON을 생성한다. layout PUT은 write lock을 잡은 상태에서 canonical serialize와 atomic disk write를 수행한다.

### 근거

- `codebase/backend/crates/http-api/src/sessions.rs`
  - `layout_get_handler`: `arc.read().await` 이후 `canonical_bytes(&snap.layout)`
  - `layout_put_handler`: `arc.write().await` 이후 `canonical_bytes`, `atomic_write_session`

### 위험 흐름

- layout item 수 증가
- free draw points 증가
- document/image/file metadata 증가
- import/export 이후 큰 layout 사용

동일 session에 대한 GET/PUT이 lock 대기하고, blocking file write가 async worker를 점유할 수 있다.

### 보완 방향

- GET은 lock 안에서 layout clone 또는 Arc snapshot만 확보하고, serialization은 lock 밖에서 수행한다.
- PUT은 CAS 검증과 snapshot 교체를 최소 범위로 유지한다.
- disk write는 `spawn_blocking` 또는 session write queue로 분리한다.
- 단, CAS와 disk-first 원자성 요구가 있으므로 ADR-0006의 persistence 정책과 함께 설계해야 한다.

## BE-5. session pane-set 계산의 반복 비용

### 증상

session pane-set provider는 호출될 때마다 layout에서 terminal UUID 목록을 만들고, terminal map 전체 snapshot을 HashMap으로 재구성한다.

### 근거

- `codebase/backend/crates/http-api/src/session_pane_set.rs`
  - layout read lock 후 terminal UUID 수집
  - `terminal_map.snapshot()` 전체를 HashMap으로 변환

### 위험 흐름

- WS connection 증가
- terminal spawn/session change/reconnect 증가
- terminal pool 크기 증가

### 보완 방향

- `TerminalMap`에 bulk lookup 또는 `lookup_uuid` 반복 API를 제공한다.
- session별 pane-set cache를 두고 layout/terminal map 변경 event에서 invalidate한다.
- WS catch-up 개선과 함께 같은 작업 단위로 처리한다.

## BE-6. PTY Ctrl-C integration test flaky

### 증상

`cargo test --workspace` 최초 실행에서 `gtmux-pty-backend` integration test `gate1_signal_ctrl_c_interrupts_sleep`가 1회 실패했다. 이후 단독 재실행과 integration suite 재실행은 통과했다.

### 실패 내용

`Ctrl-C did not interrupt sleep; output contained AFTER`

### 해석

현재 재현성 있는 실패로 단정하기는 어렵지만, terminal input/foreground process UX와 직접 연결된다. 부하가 있거나 테스트 실행 순서가 달라질 때 Ctrl-C 전달 타이밍이 흔들릴 가능성을 배제할 수 없다.

### 보완 방향

- 해당 테스트에 command start 확인 조건을 강화한다.
- Ctrl-C 전송 전 foreground process가 실제 sleep에 진입했는지 관측한다.
- PTY writer thread, shell prompt detection, read_until 조건을 분리해 flaky 원인을 좁힌다.
- 장기적으로 terminal input latency/interrupt latency 측정 테스트를 추가한다.

## 검증 메모

- `cargo test --workspace`
  - 최초 실행: `gtmux-pty-backend` integration test 1건 실패
  - 재실행: `gate1_signal_ctrl_c_interrupts_sleep` 단독 통과
  - 재실행: `gtmux-pty-backend --test integration_pane` 전체 통과
- 실패는 현재 리뷰 문서에서 “flaky 가능성”으로 기록하고, 별도 안정화 이슈로 추적하는 것이 적절하다.
