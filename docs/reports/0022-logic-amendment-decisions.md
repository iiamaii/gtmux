# Logic amendment 결정 — 2026-05-14 grilling 세션

본 보고서는 `0020-debug-classification.md` 가 분류한 **Logic 7건** (L-2 / L-3 / L-4 / L-7 / L-9 / L-12 / L-17) 의 *기획 측 amend 방향* 을 grilling 세션 (sketch §15 2단계 마무리 직전 모호성 배제) 의 결과로 확정한다. ADR amend 의 *본문 작성 input* 으로 사용된다. L-2 와 L-9 는 CORS 항목 합본이므로 결정 단위는 **6개**.

용어 사용은 본 세션에서 amend 된 `CONTEXT.md` 의 어휘를 따른다 (Pane ↔ Panel 1:1 auto-mount, Session shutdown UI 액션).

## 0. 한눈에 보기

| L | Code | 채택 정책 | 영향 ADR / SSoT |
|---|---|---|---|
| L-17 | TMUX-DAEMON-EXIT-RECOVERY | Recovery → **Prevention** 전환. tmux invariant 의 UI 측 mirror. | ADR-0009 §D5, ADR-0001 §D12, CONTEXT.md Scope boundary + new §"tmux invariant" |
| L-3 | WS-SESSION-CATCHUP | 별도 mirror cache 폐기. auto-mount → layout PUT → LAYOUT_CHANGED → GET 으로 흡수. | ADR-0002 §D8 |
| L-12 | DISPATCHER-PANE-RACE | 현 구현 ratify (per-pane 256 KiB FIFO drop-oldest, lifetime = first registerPaneOut OR pane close). | ADR-0002 §D8 |
| L-4 | AUTH-TOKEN-DELIVERY | HttpOnly cookie **폐기**. sessionStorage 단일 채널. 서버 측 3축 → 2축 (Authorization: Bearer + Sec-Fetch-Site/Origin). | ADR-0003 §D6 |
| L-7 | TMUX-CTRL-COMMENT-CHAR | argv 토큰 **selective single-quote wrap** (`#`/whitespace/`'`/`"`/`\`). `-F #{pane_id}` 정공 복귀 가능. | ADR-0001 §D11 |
| L-2/9 | CORS-LOOPBACK-ALIAS + CORS-EMPTY-DEFAULT | loopback bind + 빈 셋 → 3 origin 자동 합성. 0.0.0.0 = cloud → 명시 필수. | ADR-0003 §D3 |

## 1. L-17 — tmux daemon 종료 시 recovery 정책

### 1.1 결정
**Recovery (사후) → Prevention (사전) 모델 전환.**

- tmux 의 native invariant (Session ≥ 1 Window ≥ 1 Pane) 을 UI 측에서 mirror 하여 그 깨짐 자체를 방지.
- Canvas Panel 의 close 버튼은 **현재 tmux Window 수 = 1** 일 때 비활성화. tooltip 으로 사유 표시.
- 모든 tmux Pane 은 즉시 Canvas Panel 로 **auto-mount** (bootstrap + 내부 New Panel + 외부 CLI 의 window 추가 모두). Available 류 사용자-명시 mount 단계는 두지 않는다.
- 사용자가 명시적으로 Server 를 종료하려면 **Canvas 우상단 헤더 메뉴 → Session shutdown → confirm modal** → `kill-session` 발사 → ADR-0001 D12 graceful exit 6.
- ADR-0001 D12 의 "자동 재시도 안 함" 정신 그대로 유지. LIFE-AUTOSPAWN 자동 재기동 task 는 **취소**.

### 1.2 거절안
- **자동 재기동 + `daemon-restarted` NOTIFY_MIRROR** (0020 §2.6 원안): 사용자 의도 (shell `exit` 으로 종료 vs 외부 kill) 가 tmux 측에서 구별 불가능 → 의도 추정 기반 자동화는 fragile. ADR-0001 D12 의 명시 의도 우선 정신과 충돌.
- **UI-initiated vs shell-initiated 구분 추적**: 우리가 직전에 `kill-window` 를 보냈는지 추적해 분기. 코드 복잡도 ↑, 의도는 가장 명확하나 prevention 으로 시나리오 자체 회피 가능하므로 over-engineering.
- **Canvas Panel 수 / visible Panel 수 기준 close 비활성**: tmux 의 실제 invariant 는 *window 수* — Panel 수나 visibility 와 무관. 외부 CLI 가 추가 window 를 만든 경우 *불필요하게* 비활성화될 위험.

### 1.3 Downstream
- **CONTEXT.md amend**:
  - Scope boundary: "Session 종료 (= Server quit) 는 UI 액션 허용".
  - Placement principle: auto-mount 명시.
  - Pane ↔ Panel 관계: 0개 이상 → **정확히 1개** (auto-mount).
  - 새 절 §"tmux invariant 의 UI 측 mirror".
- **ADR-0001 §D12** amend: Session shutdown UI 액션이 `kill-session` 발사를 통해 D12 의 exit 6 경로로 합류함을 명시. "외부 kill" 의 정의에 "UI 의 Session shutdown 액션 호출 결과로 발생한 `%exit`" 도 포함.
- **ADR-0009 §D5** amend: 새 절 *graceful prevention* — invariant 보호 (close 비활성 + auto-mount + Session shutdown). 자동 재기동은 명시 거절.
- **Sprint 6 plan 변경**:
  - `S6-LIFE-AUTOSPAWN` task **취소** (handoff 0021 §6.4).
  - `S6-WS-WINDOW-CATCHUP` 재정의 (§2 참조).
  - 신규 task: **`S6-FE-SHUTDOWN`** (헤더 메뉴 + confirm modal + CTRL `kill-session`).
  - 신규 task: **`S6-FE-CLOSE-GUARD`** (panel close 비활성화 + tooltip).
  - 신규 task: **`S6-BE-AUTOMOUNT`** (backend 가 tmux %window-add / %pane-add 수신 시 auto-mount layout PUT).
  - `S6-FE-MUX-VIS` 축소: Available 섹션은 hidden panel 표시 정도로 축소 또는 제거.

### 1.4 회귀 검증 시 확인 항목
- Panel 1개 상태에서 그 Panel 의 close 버튼이 비활성화되는지.
- 외부 CLI 로 `tmux new-window -t demo` 실행 시 Canvas 에 panel 이 자동 cascade 배치로 추가되는지.
- Session shutdown 액션 호출 후 graceful exit 6 + WS close + layout flush 가 모두 발생하는지.

## 2. L-3 — `%session-changed` / `%window-add` 정적 state catch-up

### 2.1 결정
**별도 mirror cache 없음. Layout 이 진실.**

- tmux `%window-add` / `%pane-add` → backend 가 즉시 auto-mount → `PUT /api/layout` → `LAYOUT_CHANGED` broadcast.
- 새 WS subscriber 는 `GET /api/layout` 으로 canonical state 확보. 기존 pull-through-notify 흐름이 그대로 catch-up 채널을 겸함.
- Hub 의 `last_session` cache 는 그대로 유지 (session_id 만 lightweight catch-up). 그 외 윈도우·페인 view 는 Hub 가 보관하지 않는다.

### 2.2 거절안
- **Hub 가 별도 mux-mirror snapshot 캐시** (0020 §2.2 원안): Layout 과 무관하게 sessions/windows/panes view 를 Hub 가 유지. 두 진실이 sync 책임 이중 — L-17 의 auto-mount default 안에서는 layout 이 이미 완전한 mirror 이므로 잉여.
- **매 subscribe 마다 bootstrap re-run**: list-* 명령 재발사. 캐시 없음. 단순하지만 다중 클라이언트 환경에서 비용 큼 + Hub 가 이미 가진 정보 폐기.
- **Hybrid (lightweight session_id + pane_id set 캐시)**: 중간 절충. Layout 이 이미 그 역할을 함.

### 2.3 Downstream
- **ADR-0002 §D8** amend: "static-state catch-up" 절 신설 — *"정적 state (Pane 존재성·이름 등) 의 catch-up 은 별도 mirror cache 가 아니라 auto-mount → layout PUT → LAYOUT_CHANGED → GET 의 Pull-through-notify 사이클로 흡수된다. Hub 는 session_id 만 lightweight cache (last_session)"*. 라이브 PANE_OUT 의 ring buffer replay (기존 D8) 는 그대로 유지.
- **`S6-WS-WINDOW-CATCHUP` 재정의**: Hub mirror 캐시 추가 → backend auto-mount loop 구현으로 변형 (§1.3 의 `S6-BE-AUTOMOUNT`).

### 2.4 회귀 검증
- 첫 브라우저 attach 시 Hub.last_session 으로 session_id 즉시 catch-up.
- 외부 CLI 가 새 window 만든 직후 *다른 탭* 에서 attach 했을 때 Canvas 에 그 panel 이 보임 (= auto-mount + PUT 이 완료된 상태에서 GET).
- 두 번째 동시 WS 클라이언트가 broadcast LAYOUT_CHANGED 를 받고 GET 으로 동기화하는지.

## 3. L-12 — Frontend PANE_OUT mount-vs-emit race

### 3.1 결정
**현 구현 (`9268bc6`) ratify + ADR 본문에 정책 명시.**

- **Per-pane buffer**, lifetime = until first `registerPaneOut` 호출 OR 해당 pane 닫힘.
- **Cap = 256 KiB**, overflow 시 **FIFO drop-oldest** (터미널의 *최근 상태* 가 본질이므로 oldest 부터 폐기).
- Hidden panel 은 ADR-0001 D8 의 Panel Streaming State 가 *Suspended → `refresh-client -A pause`* 로 전이하여 tmux 측에서 %output 자체가 안 옴 → buffer 영구화 시나리오 차단.
- **Telemetry**: overflow count metric (구현 단계에서 추가 가능, UI 노출 X).
- L-17 의 auto-mount default 안에서는 race window 가 일정하게 짧다 (50~200ms) → cap 256 KiB 는 generous 한 defensive 디폴트. configurable 화 불요.

### 3.2 거절안
- **Cap 축소 (32~64 KiB)**: 메모리 baseline 약간 ↓ 하지만 사실상 무의미한 절약. 무용한 최적화.
- **Lifetime timeout (30s) 추가**: backend 결함으로 마운트 신호가 안 오는 시나리오 방어. 안전 마진. 그러나 L-17 의 auto-mount 가 layout PUT 까지 직선 경로이므로 timeout 시나리오는 backend bug 의 신호이지 정상 흐름이 아님 → drop+warn 보다 panic/error 로 *드러내는* 게 옳음. 코드 추가 거절.
- **Backend 직렬화 (buffer 자체 제거)**: backend 가 layout PUT 이전엔 PANE_OUT 을 판매소 대기. 가장 깔끔하나 backend 변경 폭 큼. Sprint 6 backend 작업 증가로 인해 비용 대비 이득 작음.

### 3.3 Downstream
- **ADR-0002 §D8** amend (L-3 와 동일 절 안에 sub-clause 로): "frontend late-mount buffer" 정책 본문화. (cap / FIFO / lifetime 모두 명시)

## 4. L-4 — Token 전달 + bootstrap landing

### 4.1 결정
**HttpOnly cookie 폐기. sessionStorage 단일 채널.**

- `/auth/bootstrap` 응답 = inline-script HTML (token 을 sessionStorage 에 mirror) + `Cache-Control: no-store` + `</` → `<\/` JS escape. **HttpOnly cookie set 은 제거**.
- SPA 는 sessionStorage 의 token 을 *HTTP Authorization: Bearer* 와 *WS Sec-WebSocket-Protocol* 둘 다에 사용 (현 동작 유지).
- 서버 측 인증 검증은 **3축 → 2축**:
  1. Authorization: Bearer (또는 WS subprotocol)
  2. Sec-Fetch-Site: same-origin + Origin/Host allowlist
- (구) HttpOnly cookie 항 (D6 secondary 2차 축) 은 *현재 정책상 redundant* 임이 grilling 으로 드러나 폐기.

### 4.2 거절 / 미채택안
- **현 hybrid 유지 (sessionStorage + HttpOnly cookie + 트리플 검증)**: ADR-0003 D6 그대로. "defense-in-depth" 명분으로 cookie 항 유지. 그러나 *같은 token 이 sessionStorage 에도 있음* → XSS 시 attacker 가 sessionStorage 에서 읽을 수 있어 HttpOnly cookie 의 XSS 방어가 우회됨. cookie 의 실제 기여 = "존재 확인 (CSRF)" 정도인데, Authorization Bearer + Origin/Host check 도 동등 방어 제공 → 사실상 redundant.
- **메모리만 (window var)**: 페이지 reload 시 token 소멸 → 매 reload 마다 bootstrap 재경유. XSS 노출 시간 감소 + 북마크는 path 만 권장하는 정신과 일치. 단점: UX 저하 (개발 시 자주 reload), 단일-사용자 local 환경에서는 보안 이익 미미. 현 단계 미채택, P1+ 재방문 여지.
- **Cookie 단일 (sessionStorage 제거)**: WS Sec-WebSocket-Protocol 헤더는 JS 가 조립해야 하는데 HttpOnly cookie 는 JS 가 읽지 못함 → 구조적 충돌. ADR-0002 D5 와 정면 모순. 거절.

### 4.3 Downstream
- **ADR-0003 §D6** amend: 본문에서 "HttpOnly cookie (secondary 2차 축)" 항 삭제. 3축 → 2축으로 단순화. inline-script bootstrap landing 패턴 본문화 (Cache-Control + JS escape 의무).
- **백엔드 변경**: `/auth/bootstrap` 핸들러에서 Set-Cookie 헤더 제거. 모든 보호 라우트의 cookie 검증 미들웨어 제거 (Authorization Bearer + Sec-Fetch-Site 검증으로 단일화).
- **SSoT `security-defaults.md`** 동반 amend (D6 변경 반영).

### 4.4 회귀 검증
- Bootstrap landing 응답에 Set-Cookie 가 없음.
- SPA 가 모든 보호 API 호출에 Authorization: Bearer 헤더 첨가.
- WS handshake 에 `Sec-WebSocket-Protocol: gtmux.v1, bearer.<token>` 첨가.
- Cookie 없는 상태로 PUT /api/layout 이 204 (=성공) 응답.

## 5. L-7 — tmux control-mode argv quoting

### 5.1 결정
**Selective single-quote wrap.**

- argv 토큰이 다음 중 하나를 포함하면 single-quote 로 감싼다:
  - `#` (line-comment 시작 문자)
  - whitespace (공백·탭)
  - `'` (single-quote 자체)
  - `"` (double-quote)
  - `\` (backslash)
- 내부 `'` 는 shell-style `'\''` 패턴으로 escape (single-quote 닫고 → escaped quote → single-quote 다시 열고).
- 그 외 안전 문자열 (`[A-Za-z0-9_./%@:=-]+`) 은 unquoted 그대로 통과. 로그 가독성 보존.
- `lifecycle::serialise_command` 가 단일 책임으로 quoting 적용. 호출자는 raw argv 만 신경.

### 5.2 거절안
- **Universal always-quote**: 모든 토큰을 `'...'` 로. 의사결정 분기 0 으로 단순. 단점: `kill-window '-t' '@1'` 같은 로그 노이즈, 디버깅 가독성 ↓. 운영 비용 누적이 selective 대비 의미 있게 큼.
- **Unsafe args reject**: 호출자가 사전 sanitize 의무. 단점: user-supplied 값 (label/note → send-keys 경로 등) 이 unsafe 일 수 있어 완전 제거 불가. 결국 quoting 이 필요.

### 5.3 Downstream
- **ADR-0001 §D11** amend: 새 §D14 (또는 D11 내부 sub-clause) 로 argv quoting 정책 본문화. 안전 문자 집합 + escape 규칙 + 적용 대상 (lifecycle::serialise_command).
- **Sprint 6 task `S6-ARGV-QUOTE`** 가 본 결정 그대로 구현 input. 본 결정 후 `S6-BE-CTRL-ACK` 가 정식 wire 되면 `NewPanelButton` 의 `-P -F #{pane_id}` 복귀 가능 (pane_id 를 response 에서 받을 수 있게 됨).
- **회귀 테스트**: argv 단위 quoting 정확성 unit test ×N (각 unsafe 문자 케이스 + escape 케이스).

### 5.4 ADR-0003 §D8 (Pane ID 정규식) 와의 정합
- `Pane ID: ^p[0-9a-zA-Z]{1,32}$`, `tmux pane id: ^%[0-9]+$` 는 quoting 적용 전에 이미 안전 문자 집합 안 → quoting 대상 아님. 본 결정과 직교.

## 6. L-2 / L-9 — CORS 디폴트 + loopback alias

### 6.1 결정
**`Config::effective_cors_origins` 의 현 구현 ratify.**

- `cors_origins` 가 빈 셋이고 bind ∈ {`127.0.0.1`, `localhost`, `::1`, `unix:/...`} 이면 다음 3 origin 자동 합성:
  - `http://127.0.0.1:<port>`
  - `http://localhost:<port>`
  - `http://[::1]:<port>`
- bind 가 위 집합 밖 (예: `0.0.0.0`, public IP, hostname) 이면 사용자가 `cors_origins` 명시 의무. 빈 셋 + non-loopback = fail-closed 거부 (startup error).
- Scheme 은 합성 시 `http` 고정. TLS (cloud 모드) 가 적용되는 시점에는 이미 사용자 명시 의무이므로 모순 없음.

### 6.2 거절안
- **0.0.0.0 도 loopback-같은 행위로 분류**: 0.0.0.0 은 모든 인터페이스 catch-all 이라 같은 머신에서 127.0.0.1 으로 접근하는 시나리오를 합성 허용. 단점: cloud-mode 자동 추론 (ADR-0003 D22) 과 충돌 — 0.0.0.0 이 cloud 이면서 동시에 loopback 이기도 한 이중 정체성. fail-closed 정신 위반.
- **0.0.0.0 bind 시 startup warn**: 합성은 안 하되 경고 메시지. 독립적으로 (A) 와 병행 가능하나 본 결정의 scope 밖 (운영 개선 task 로 carry).

### 6.3 Downstream
- **ADR-0003 §D3** amend: 두 sub-clause 추가:
  1. *디폴트 합성*: 위 조건 충족 시 3 origin 자동 합성.
  2. *Cloud 정신*: non-loopback bind 는 명시 의무 (fail-closed 유지).
- **SSoT `security-defaults.md` §"host_allowlist"** 와 정합 정렬. (현재 ADR-0003 §O3 가 "명시 우선, 미설정 시 자동 합성" 권장과 일치)
- **회귀 테스트**: 4개 unit test — (loopback bind + 빈 셋 → 3 origin), (loopback bind + 명시 1개 → 명시 그대로), (0.0.0.0 + 빈 셋 → startup error), (0.0.0.0 + 명시 → 명시 그대로).

### 6.4 운영 가이드 (사용자 노출)
- 배너 URL 은 `http://127.0.0.1:<port>` 를 default 로 출력하되, *localhost / ::1 도 동등하게 동작* 한다는 사실은 README / startup help 에 명시.
- TLS / cloud 모드 가이드 분리 (ADR-0003 D12 후속) — 본 결정과 무관.

## 7. ADR amend 위치 요약표

| 위치 | 작업 | 관련 L |
|---|---|---|
| ADR-0001 §D11 | 새 sub-clause "argv quoting" 추가 | L-7 |
| ADR-0001 §D12 | "Session shutdown UI 액션 → kill-session → exit 6" 경로 본문 추가. 외부 kill 정의 명시화. | L-17 |
| ADR-0002 §D8 | 새 sub-clause "static-state catch-up = layout pull-through-notify" + "frontend late-mount buffer (per-pane 256 KiB FIFO drop-oldest)" 추가 | L-3 + L-12 |
| ADR-0003 §D3 | 새 sub-clause "디폴트 합성 + loopback alias" + "Cloud bind 명시 의무" 추가 | L-2/9 |
| ADR-0003 §D6 | HttpOnly cookie 항 삭제. 3축 → 2축으로 본문 갱신. inline-script bootstrap landing 본문화. | L-4 |
| ADR-0009 §D5 | 새 절 "graceful prevention" — close 비활성 / auto-mount / Session shutdown UI 액션 / LIFE-AUTOSPAWN 명시 거절 | L-17 |

각 amend 본문 끝에 다음 cross-link 첨부:

> Result: this clause was added retroactively after the debug session 2026-05-14. See `docs/reports/0020-debug-classification.md` for the failure analysis and `docs/reports/0022-logic-amendment-decisions.md` for the grilling outcome.

## 8. Sprint 6 plan 변경 종합

### 8.1 취소
- `S6-LIFE-AUTOSPAWN` (handoff 0021 §6.4) — Prevention 모델로 흡수.

### 8.2 재정의
- `S6-WS-WINDOW-CATCHUP` → **`S6-BE-AUTOMOUNT`** (backend 가 tmux %window-add / %pane-add 수신 시 자동 layout PUT + LAYOUT_CHANGED broadcast). Hub mirror 캐시 작업 없음.
- `S6-FE-MUX-VIS` → 축소. Available 섹션 제거 가능 (모든 pane auto-mount default). Sidebar 는 hidden panel 표시 + group tree 역할만.

### 8.3 신규
- `S6-FE-SHUTDOWN` (Sprint 6-B): Canvas 우상단 헤더 메뉴 + Session shutdown 항 + confirm modal + CTRL `kill-session` 발사. ADR-0009 D5 amend 동반.
- `S6-FE-CLOSE-GUARD` (Sprint 6-B): panel header / sidebar 의 close 버튼 disabled state (tmux window 수 = 1 일 때) + tooltip. ADR-0009 D5 amend 동반.
- `S6-BE-AUTOMOUNT` (Sprint 6-D): backend 의 tmux mirror 가 %window-add / %pane-add 수신 시 자동 panel append + PUT + broadcast.

### 8.4 변경 없음
- `S6-A` (ADR amend 6건) — 본 결정으로 amend *내용* 이 확정됨. 본 보고서가 input.
- `S6-D` 의 `S6-BE-CTRL-ACK` / `S6-BE-CLOSE` / `S6-ARGV-QUOTE` — 그대로. 단 `S6-BE-CLOSE` 는 `S6-FE-CLOSE-GUARD` 와 정합 (FE 가 막은 close 가 BE 에 도달했을 때의 처리).

## 9. 회귀 게이트 (Sprint 6 closeout 전 확인)

본 결정들이 모두 반영된 후 smoke 9-step 외에 다음 manual probe 추가 권고:

1. **L-17 prevention probe**: Panel 1개 상태에서 close 비활성, 우상단 메뉴 → Session shutdown → confirm 진행 → exit 6.
2. **L-3/L-17 auto-mount probe**: 외부 터미널에서 `tmux -L gtmux-demo new-window` → Canvas 에 panel 자동 추가.
3. **L-12 buffer probe**: New Panel 직후 첫 prompt 가 검은 화면 없이 즉시 보임.
4. **L-4 token probe**: 브라우저 DevTools → Application → Cookies 에 gtmux 토큰 cookie 가 *없음* (sessionStorage 만 존재).
5. **L-7 quoting probe**: tmux 측 디버그 로그에서 `-F #{pane_id}` argv 가 `'-F' '#{pane_id}'` 로 serialise 되어 % 없이 통과.
6. **L-2/9 CORS probe**: `localhost:<port>` / `127.0.0.1:<port>` / `[::1]:<port>` 세 URL 모두 동등하게 SPA 진입 가능. `0.0.0.0` bind + 빈 셋 → startup error.

## 변경 이력

- 2026-05-14: 초안 — grilling 세션 (사용자 ↔ PM) 의 6개 L 결정 정본화. ADR amend 본문 작성 input.
