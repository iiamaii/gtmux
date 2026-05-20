# 디버깅 이슈 분류 보고 — 2026-05-14 데모 안정화 세션

본 보고는 sketch §15 2단계 데모를 *실제로 사용 가능*한 상태로 끌어올리는 과정에서 발견된 **14건 의 결함을 두 갈래로 분류**한다. 같은 세션 내 commits `da5c221`~`bcb37a8` 를 모두 포괄한다.

- **L (Logic)** — *기획 단계에서 미정의 또는 정의 충돌*. 코드는 명시된 결정/SSoT 를 그대로 따랐지만 그 결정이 운영 현실/사용자 경험과 어긋난 경우. 관련 기획 문서를 *amend* 해야 한다.
- **I (Implementation)** — *기획은 옳았으나 구현이 어긋남*. SSoT/ADR이 정한 계약을 코드가 위반 (오구현) 또는 미배선 (미구현).

## 1. 한눈에 보기

| # | Code | 증상 | 분류 | 관련 기획 / commit |
|---|---|---|---|---|
| 1 | CFG-VALIDATE-ORDER | `--port 9999` 가 port=0 sentinel 로 죽음 | **I** 오구현 | ADR-0011 R7-T6 §7 / `da5c221` |
| 2 | CORS-LOOPBACK-ALIAS | `localhost:9999` 접속 시 403 | **L** | [ADR-0003 §D3](#) / `da5c221` |
| 3 | WS-SESSION-CATCHUP | New-Panel 클릭 → "tmux session not ready yet" | **L** | [ADR-0002 §D8](#) + [ADR-0001 §D7](#) / `11778fb` |
| 4 | AUTH-TOKEN-DELIVERY | 새 server token 무시 → 401 / WS upgrade rejected | **L** | [ADR-0003 §D6](#) + sketch §13 / `dea7c13` |
| 5 | TMUX-SPAWN-DETACH-FLAG | `%session-changed` 영구 미수신 → 캐시 빈 채 | **I** 오구현 | ADR-0009 D2 / `3a36f0f` |
| 6 | LIFECYCLE-MUTEX-DEADLOCK | CTRL request 5s timeout | **I** 오구현 | (코드 주석에 latent 인지) / `4a5faf0` |
| 7 | TMUX-CTRL-COMMENT-CHAR | `parse error: -F expects an argument` | **L** | [ADR-0001 §D11/D14](#) / `8cbadee` |
| 8 | MUX-RESIZEWINDOW-PARK | `Command::ResizeWindow` 임시 우회 코드 잔존 | **I** 미구현 | [ADR-0008 §allowlist](#) / `50bad9c` |
| 9 | CORS-EMPTY-DEFAULT | same-origin SPA fetch 차단 | **L** | [ADR-0003 §D3](#) / `50bad9c` |
| 10 | HTTP-PUT-STATUS | "PUT /api/layout returned 200" 에러 토스트 | **I** 오구현 | [canvas-layout-schema §4.2](#) / `9268bc6` |
| 11 | CANVAS-DRAG-COMMIT | 클릭 시 panel 위치 초기 배열 회귀 | **I** 미구현 | sketch §10.2 + ADR-0010 D11 / `9268bc6` |
| 12 | DISPATCHER-PANE-RACE | New panel 내 검은 화면 (수정 1단계) | **L** | [ADR-0002 §D8 catch-up](#) / `9268bc6` |
| 13 | LAYOUT-CHANGED-BROADCAST | New panel 새로고침 해야 표시 | **I** 미구현 | [canvas-layout-schema §4.2](#) / `c2af73a` |
| 14 | LAYOUT-ETAG-RACE | New panel 새로고침 해야 표시 (수정 2단계) | **I** 오구현 | [SSoT §2.2 pull-through-notify](#) / `21a1fe2` |
| 15 | PANEL-PANEID-CONTRACT | New panel 내 검은 화면 (수정 2단계) | **I** 오구현 | XtermHost.svelte L53 contract / `21a1fe2` |
| 16 | XTERM-CSS-IMPORT | term.write 후에도 검은 화면 | **I** 미구현 | [ADR-0004](#) / `bcb37a8` |
| 17 | TMUX-DAEMON-EXIT-RECOVERY | shell `exit` 시 서버가 broken-pipe로 무응답 | **L** | [ADR-0009 §D5](#) / 미수정 (운영 회피) |

총 17건 — **L 7건 / I 오구현 7건 / I 미구현 3건**.

## 2. 기획상 logic 문제 (L)

코드가 기획을 따랐는데 결과가 잘못된 경우. 각 항목은 *어느 기획 문서의 어느 결정이 amend 되어야 하는지* 명시한다.

### 2.1 L-2 / L-9 — CORS 운영 현실과 정확 일치 정책의 충돌

- **관련 기획**: [`docs/adr/0003-security-defaults.md`](../adr/0003-security-defaults.md) §D3 "정확 일치 화이트리스트, wildcard 거부"
- **충돌점**:
  - D3는 보안성 측면에서 wildcard origin 을 거부한다. 그러나 *빈 셋 디폴트* 의 의미가 정의되지 않았고, *127.0.0.1 / localhost / [::1]* 가 브라우저 운영상 same-origin 으로 자유롭게 섞이는 현실도 미반영.
  - 결과: 사용자가 배너 URL의 `127.0.0.1` 대신 `localhost` 로 접속하면 *동일 호스트* 임에도 403 차단. 빈 셋 디폴트는 같은 origin 의 SPA fetch도 차단.
- **Amend 방향**: D3 본문에 다음 두 sub-clause 추가
  1. *디폴트 합성*: `cors_origins` 가 비어 있고 `bind` 가 loopback 이면 `["http://127.0.0.1:<port>", "http://localhost:<port>", "http://[::1]:<port>"]` 합성.
  2. *Cloud 모드 제외*: non-loopback bind 는 사용자가 명시해야 한다는 fail-closed 정신 유지.
- **현 구현**: `Config::effective_cors_origins` 에 두 절 모두 반영 (`da5c221`, `50bad9c`).

### 2.2 L-3 — `%session-changed` / `%window-add` 같은 정적 state 의 catch-up 미정

- **관련 기획**:
  - [`docs/adr/0001-tmux-integration-control-mode.md`](../adr/0001-tmux-integration-control-mode.md) §D7 (`%output` → per-pane ring buffer)
  - [`docs/adr/0002-transport-websocket.md`](../adr/0002-transport-websocket.md) §D8 (catch-up replay on attach)
- **충돌점**: 두 ADR 모두 *PANE_OUT 의 ring-buffer 기반 catch-up* 만 다룬다. `%session-changed`, `%window-add`, `%window-renamed` 같은 *정적 state* 는 단발 emit 후 사라지는데, `broadcast::Sender` 는 subscribe *전에* 발생한 메시지를 재배달하지 않는다 → 매 browser load 마다 session id 를 영영 못 받음.
- **Amend 방향**: ADR-0002 D8 에 *static-state cache* 절 추가
  - Hub 는 `%session-changed` 최신값 + `%window-add` / `%window-renamed` 누적 view 를 캐시 → 새 subscribe 시 즉시 replay.
  - 본 세션은 session 만 반영 (`11778fb`). window 누적 catch-up 은 Sprint 6 carry.
- **현 구현 한계**: 윈도우 단위 catch-up은 아직 없음 → 외부 attach 가 만든 window 가 미러되지 않음.

### 2.3 L-4 — Token 전달 방식과 SPA 호환성 충돌

- **관련 기획**: [`docs/adr/0003-security-defaults.md`](../adr/0003-security-defaults.md) §D6 (HttpOnly cookie 채택) + `docs/sketch.md` §13.3
- **충돌점**: D6 는 XSS 하드닝 차원에서 *HttpOnly cookie* 채택. 그러나
  - WS `Sec-WebSocket-Protocol: gtmux.v1, bearer.<token>` 헤더는 *JavaScript 에서 조립*해야 한다 (브라우저가 cookie 를 WS subprotocol 에 자동 첨가하지 않음).
  - HTTP `Authorization: Bearer <token>` 도 SPA 가 명시 헤더로 보내야 한다.
  - HttpOnly cookie 는 JS 에서 읽지 못하므로 *SPA 가 token 에 접근할 채널이 사라짐*.
- **Amend 방향**: D6 에 *one-shot landing* 단서 추가
  - `/auth/bootstrap` 응답이 HttpOnly cookie 외에 *inline script 가 sessionStorage 에 token 을 mirror* 하는 minimal HTML 을 반환한다.
  - `Cache-Control: no-store` 강제 + `</` → `<\/` JS escape 로 inline-script termination 보호.
- **현 구현**: `dea7c13` 에 위 방향 그대로 반영.

### 2.4 L-7 — tmux control-mode stdin 의 `#` comment quirk 미고려

- **관련 기획**: [`docs/adr/0001-tmux-integration-control-mode.md`](../adr/0001-tmux-integration-control-mode.md) §D11/D14 (argv 처리)
- **충돌점**: D11/D14 는 argv-vs-shell 분리만 다룬다. 그러나 tmux 의 control-mode stdin 파서는 별개 quirk — *`#` 로 시작하는 토큰을 line comment 로 잘라낸다*. `new-window … -F #{pane_id}` 가 `-F` 만 남고 인자 없음 에러.
- **Amend 방향**: D11 (또는 별도 §D16) 에 *argv 안전 quoting* 절 추가
  - `serialise_command` 가 argv 토큰에 `#`, 공백, 따옴표 가 있으면 single-quote escape 적용.
  - 또는 `#{...}` 같은 format 문자열은 별도 `display-message -p` 같은 후속 명령으로 분리.
- **현 구현 한계**: `-P -F #{pane_id}` 를 *제거*하는 우회로 해소 (`8cbadee`). 정공인 quoting helper 는 Sprint 6 S6-BE-CTRL-ACK 와 묶어 처리 예정.

### 2.5 L-12 — Frontend 측 PANE_OUT mount-vs-emit race 미정의

- **관련 기획**: [`docs/adr/0002-transport-websocket.md`](../adr/0002-transport-websocket.md) §D8 (catch-up replay)
- **충돌점**: D8 의 catch-up 은 *연결 시 일괄 replay* 가정. 그러나 *런타임 중 새 pane 생성* 시 backend 의 PANE_OUT 첫 burst 는 frontend XtermHost 가 *마운트되기 전* 에 도착한다 (PUT → LAYOUT_CHANGED → re-hydrate → PanelNode → XtermHost 라는 4-hop). 첫 prompt redraw 가 dropped → 검은 화면.
- **Amend 방향**: D8 에 *late-mount buffer* 절 추가
  - dispatcher 가 handler 미등록 pane 의 PANE_OUT 을 per-pane 버퍼에 stash, registerPaneOut 호출 시 flush.
  - 버퍼 cap 으로 폭주 방지 (현 구현 256 KiB).
- **현 구현**: `9268bc6` (`dispatcher.handlePaneOut` + `appendLateBuffer`).

### 2.6 L-17 — 마지막 window 종료 시 graceful recovery 정책 미정

- **관련 기획**: [`docs/adr/0009-tmux-daemon-isolation.md`](../adr/0009-tmux-daemon-isolation.md) §D5 (daemon outlives Server)
- **충돌점**: D5 는 *서버 종료 vs daemon 생존* 만 다룬다. 역의 케이스 — 사용자가 검은 화면의 새 pane 에 `exit` 또는 `Ctrl-D` 입력 → 마지막 window 닫힘 → tmux daemon 이 자체 종료 → 우리 server 의 control-mode pipe 가 broken pipe 로 침묵 — 은 미정의.
- **Amend 방향**: D5 에 *graceful recovery* 절 추가
  - control-mode pipe broken 감지 시 우리 server 가 자동으로 `lifecycle::TmuxDaemon::spawn` 재실행 + 모든 WS 클라이언트에 `daemon-restarted` NOTIFY_MIRROR 전송.
  - 또는 server 가 그 시점에서 종료 (현재 idle process 잔존 상태) + 사용자에게 "세션 종료됨" 안내.
- **현 운영 회피**: server 재기동으로 해소. Sprint 6 LIFE-AUTOSPAWN task 로 carry.

## 3. 구현상 문제 (I)

### 3.1 오구현 (계약 위반)

#### 3.1.1 I-1 — figment chain 의 CLI override 가 validate 이후에 적용

- **원인**: `bin/gtmux-cli/src/main.rs::start` 가 `load_config()` *반환 후* `config.server.port = p` 로 mutation. `load_config` 는 내부에서 validate 를 돌리므로 sentinel `port = 0` 이 그 단계에서 죽는다. ADR-0011 R7-T6 §7 선행순위 `CLI > Env > TOML > defaults` 의도와 어긋남.
- **해결 접근**: `load_with_overrides(path, session, port_override)` 신설 → `port_override` 를 figment provider 의 *마지막 layer* 로 합류 후 validate. `load` 는 thin wrapper 로 유지 → 14개 기존 호출자 무영향. (`da5c221`)

#### 3.1.2 I-5 — `TmuxDaemon::spawn` 의 `-d` flag

- **원인**: `tmux -C new-session -A -s <name> -d` 의 `-d` 는 detached 모드 → 새 session 만 만들고 control-mode client 가 즉시 exit (`%exit` emit) → `run_event_loop` EOF → loop 종료 → 이후 `%session-changed $0 <name>` 같은 attach-tied event 영영 미수신.
- **해결 접근**: argv 에서 `-d` 제거. tmux 는 stdin pipe 가 열려있는 동안 attached client 로 동작. ADR-0009 D5 의 daemon 생존성은 *session 에 window 가 있으면 server 가 산다* 는 tmux 내장 규칙으로 그대로 유지. (`3a36f0f`)

#### 3.1.3 I-6 — `Arc<Mutex<TmuxDaemon>>` 단일 mutex deadlock

- **원인**: `run_event_loop` 가 `read_line.await` 동안 outer mutex 를 점유. tmux idle 구간에 `await` 가 영구 대기 → `run_command_loop` 의 `write_line` 이 같은 mutex 를 영원히 못 잡음. 코드 주석에 *"future design may need tokio::io::split"* 라며 latent로 인지했지만 P1+ 으로 분류돼 있었음 (실은 `-d` 제거로 long-lived client가 되는 순간 correctness bug).
- **해결 접근**: `TmuxDaemon` 구조체 내부에 `child / stdout_lock / stdin_lock` 세 개 독립 `tokio::sync::Mutex`. `read_line(&self)` 은 stdout 만, `write_line(&self)` 은 stdin 만 잡는다 → 두 loop 가 서로 starve 안 함. (`4a5faf0`)

#### 3.1.4 I-10 — PUT /api/layout 응답이 SSoT 와 불일치

- **원인**: `layout_put_handler` 가 `200 OK + body{}` 반환. [canvas-layout-schema](../ssot/canvas-layout-schema.md) §4.2 정본은 `204 No Content + ETag header + WS broadcast`. SPA 의 `attemptAppend` 는 204 만 success 로 분류 → 정상 PUT 이 "PUT returned 200" 에러로 표면화.
- **해결 접근**: status `OK` → `NO_CONTENT`, body 제거. 기존 단위테스트 `layout_put_success_updates_etag` 도 동시에 갱신. (`9268bc6`)

#### 3.1.5 I-14 — LAYOUT_CHANGED 수신 직후 setEtag → If-None-Match 충돌

- **원인**: `dispatcher.handleLayoutChanged` 가 `layoutStore.setEtag(new_etag)` 후 refetch handler 호출. refetch 는 `fetchLayoutAndHydrate(token, layoutStore.etag)` 인데 그 시점에 store 의 etag = 방금 캐시한 새 값 → If-None-Match 와 서버 etag 일치 → **304** → panelsStore 미갱신. Pull-through-notify 의 원칙 ("broadcast = 트리거, GET = 권위") 위반.
- **해결 접근**: `handleLayoutChanged` 가 layoutStore 를 만지지 않음. etag 전환은 `fetchLayoutAndHydrate` 가 200 응답 시 hydratePanels 직후 수행, 304 시는 그대로 유지. (`21a1fe2`)

#### 3.1.6 I-15 — PanelNode 가 XtermHost contract 위반

- **원인**: `XtermHost.svelte` L53 contract 명시 `"paneId 는 '37' 같은 정수 문자열 (PanelNode 가 SSoT pane_id 의 정수 부분만 전달)"`. 그러나 PanelNode 는 `data.pane_id` (= SSoT form `%1`) 를 그대로 전달 → `registerPaneOut("%1", h)`. dispatcher 는 PANE_OUT 매칭 시 `String(decoded.paneId)` = `"1"` → key mismatch → handler 영원히 매칭 실패.
- **해결 접근**: PanelNode 에서 `data.pane_id.replace(/^%/, '')` 로 prefix 제거 후 전달. (`21a1fe2`)

#### 3.1.7 I-7 (보조) — `Command::ResizeWindow` 임시 park 코드

- **원인**: `cmd_router::build_pane_resize_request` 가 `Command::ListWindows + args[0] == "resize-window"` 키워드 우회 사용 중. ADR-0008 D2 allowlist 에 `resize-window` 가 정식 명령으로 등록돼 있는데도 enum variant 미정의로 인한 임시 우회.
- **해결 접근**: `mux-router::Command::ResizeWindow { window_id, cols, rows }` variant 추가. `cmd_router` 와 `serialise_command` 가 직접 발급/직렬화. 우회 코드 제거. (`50bad9c`)

### 3.2 미구현 (스펙은 있으나 코드 누락)

#### 3.2.1 I-8 — `Command::ResizeWindow` (위 I-7 의 미구현 측면)

- 위 항목과 동일 사안. ADR-0008 의 11-command 정본 중 마지막 누락분이었음. 정식 enum variant 추가로 해소. (`50bad9c`)

#### 3.2.2 I-11 — Canvas `onnodedragstop` 가 store / server 미반영

- **원인**: 코드 주석에 *"store mutation API 미배선까지의 임시 동작"* 명시. SvelteFlow 는 controlled mode (nodes = panelsStore derived) 라 store 가 갱신되지 않으면 다음 selection 이벤트가 derived 를 재계산하며 원래 position 으로 snap back.
- **해결 접근**:
  1. `PanelsStore.movePanel(id, x, y)` 메서드 추가 (entry-level mutation).
  2. `lib/http/layout.ts::putLayoutCommitCurrent` 헬퍼 추가 — 현재 panels/groups 전체를 PUT, 412 자동 1회 rebase.
  3. `onnodedragstop` 가 두 단계 모두 호출. (`9268bc6`)

#### 3.2.3 I-13 — PUT 성공 후 WS LAYOUT_CHANGED 미발사

- **원인**: SSoT [`canvas-layout-schema §4.2`](../ssot/canvas-layout-schema.md) 정본은 `"204 + ETag + WS 0x80 LAYOUT_CHANGED 브로드캐스트"` 인데 `layout_put_handler` 가 broadcast 부분을 빼먹음. WS 측 catch-up 도 없으므로 SPA 가 변경 사실을 영영 모름.
- **해결 접근**:
  1. `gtmux-ws-server::Hub` 에 `layout_events: broadcast::Sender<[u8;16]>` 신설 + `publish_layout_changed` / `subscribe_layout` API.
  2. `handle_socket` 의 `tokio::select!` 에 layout broadcast arm 추가 → 도착 시 `0x80 LAYOUT_CHANGED` envelope 송신.
  3. `http-api` 가 `gtmux-ws-server` 의존성 추가, `AppState::with_hub` 생성자로 hub clone 주입. `layout_put_handler` 가 PUT 성공 직후 `hub.publish_layout_changed(new_etag)` 호출. (`c2af73a`)

#### 3.2.4 I-16 — `@xterm/xterm/css/xterm.css` import 누락

- **원인**: ADR-0004 (xterm.js v6 채택) 에 *stylesheet import 의무* 가 명시되지 않음. xterm v6 의 cell rendering 은 본 stylesheet 가 없으면 DOM 은 만들어지지만 cell width/height = 0 으로 collapse 되어 글자가 화면에 안 보임.
- **진단 trail** (모든 단계 정상이었지만 stylesheet 만 누락):
  ```
  [ws] PANE_OUT pane=1 len=104 → late-buffer
  [ws] registerPaneOut pane=1 flushing 2 buffered chunk(s)
  [ws] PANE_OUT pane=1 len=51 → handler          ← term.write 호출 정상
  [xterm] mount  pane=1 container=478x285        ← container 사이즈 정상
  [xterm] post-fit pane=1 cols=59 rows=19        ← fit 정상
  ```
- **해결 접근**: `XtermHost.svelte` 에 `import '@xterm/xterm/css/xterm.css';` 한 줄. CSS bundle 21.85 → 25.45 KB raw (gzip 4.10 → 4.82 KB). ADR-0004 본문에 *Required imports* 절 추가 권고. (`bcb37a8`)

## 4. 종합 권고

### 4.1 즉시 amend 권고 (logic 7건)

| 기획 문서 | 추가/수정해야 할 절 |
|---|---|
| ADR-0001 §D11 | argv 안전 quoting (`#` / 공백 / 따옴표) — L-7 |
| ADR-0002 §D8 | static-state cache (`%session-changed` / `%window-add`) — L-3 |
| ADR-0002 §D8 | frontend late-mount buffer 정책 — L-12 |
| ADR-0003 §D3 | cors_origins 빈 셋 디폴트 합성 + loopback alias — L-2 / L-9 |
| ADR-0003 §D6 | bootstrap landing inline-script sessionStorage 미러 — L-4 |
| ADR-0009 §D5 | 마지막 window 종료 시 server graceful recovery — L-17 |

### 4.2 회귀 방지 권고

- **단위 테스트가 통과해도 정합성을 100% 보증하지 못함** 의 사례:
  - I-10 (PUT 200 vs 204) 는 backend test 가 OK 만 검사해 SSoT 명시 204 와 어긋남을 못 잡았다.
  - I-13 (LAYOUT_CHANGED broadcast 누락) 도 backend test 가 broadcast 측을 검증 안 함.
  - 대안: *SSoT-conformance 테스트* — schema 명시값과 실제 응답을 직접 대조하는 integration probe 를 smoke 9-step 에 추가.
- **End-to-end debugging trace 인스트루멘테이션 유지**: 본 세션에 추가된 `[ws] PANE_OUT pane=...`, `[ws] registerPaneOut ...`, `[xterm] mount container=...`, `[xterm] post-fit cols= rows=` 5개 `console.debug` 는 매우 저렴하며 차후 발생할 wire 측 결함을 즉시 시각화한다. polish 단계까지 유지 권고.

### 4.3 Sprint 6 carry-forward 정렬

본 세션 동안 발견된 잔여 (미수정) 항목:
- L-7 의 argv quoting 정공 (Sprint 6 **S6-BE-CTRL-ACK** 와 동반 — `-F #{pane_id}` 복귀)
- L-17 의 graceful recovery (**S6-LIFE-AUTOSPAWN**)
- L-3 의 window-level catch-up (**S6-WS-WINDOW-CATCHUP**)
- `attemptAppend` 의 setEtag 대칭성 (`commitCurrent` 와 통일)
- drag-commit 디바운스 (연속 드래그 시 in-flight PUT 직렬화)

## 변경 이력

- 2026-05-14: 초안 (commits `da5c221`~`bcb37a8` 17건 분류).
