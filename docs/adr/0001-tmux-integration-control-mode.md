# ADR-0001: tmux 통합 = 컨트롤 모드 (`tmux -C`) 단일 채널

- 상태: **Deprecated (2026-05-14, superseded by ADR-0013 "PTY direct, no tmux")**. 본문은 historical context 로 보존. 2026-05-14 amend (§D12 sub-clause + §D13 argv quoting) 는 supersession 으로 자동 무효화 — 그러나 *결정 흐름의 기록* 으로 본문에 남김. 참조 우선순위: ADR-0013 + `docs/reports/0023-pty-poc-verification-and-decision.md`.
- 일자: 2026-05-13 (Proposed) / 2026-05-14 (Accepted → Deprecated 동일 일자, POC 검증 결과 channel switch)
- 결정자: system-architect (배치 A1, dispatch 0002 §1 A1)
- 근거 보고서: `docs/reports/0001-tmux-control-mode.md` (이하 *R1*), `docs/reports/0010-grill-amendments.md` (이하 *Grill*) D8 · D15 · D16 · D19
- 관련 ADR: ADR-0007 (Server : Session : Port 1:1:1 바인딩), ADR-0008 (single-pane + Group, **command allowlist 정본**), ADR-0009 (tmux daemon 격리, `-L gtmux-<session>`), ADR-0002 (전송 계층 + wire-protocol SSoT, 후속), ADR-0003 (보안 디폴트, 후속)

## 맥락

`docs/sketch.md` §10.1 "백엔드 구성"은 *tmux control mode client*를 백엔드 컴포넌트의 1차 시민으로 두고, §11.2.A "MVP tmux 연동"은 control mode 기반 연결 · pane 부트스트랩 · `send-keys` 입력 · `refresh-client -A` Panel Streaming State 제어를 MVP 포함 범위로 명시한다. §14 "기술적 난점"의 1·3·6·8번은 모두 control mode 채널의 동작(출력 동기화, 1:1 resize 매핑, bootstrap 이벤트 수 비례, long-suspend 버퍼 동작)을 직접 가리킨다. 따라서 *tmux와 어떤 채널로 어떻게 대화할지*는 다른 모든 ADR(전송 0002, 보안 0003, 영속화 0006)이 입력 제약으로 받는 가장 앞단 결정이다.

R1 보고서는 이 결정의 evidence base다 — R1 §"구체 권장"이 (1) 최소 tmux 3.2 / 권장 3.4+ (R1 §8), (2) `list-* → pause-after → -B 구독 → 라이브` 부트스트랩 순서 (R1 §1·§3·§6), (3) iTerm2 `TmuxGateway` 식 단일 FIFO 큐 + command-number 매칭 (R1 §2), (4) 백엔드는 `-C`만 (`-CC` 금지, R1 §1·§9), (5) MVP에서 `refresh-client -C` 미호출 (R1 §11), (6) `%output` 8진수 디코딩 → 페인별 ring buffer → WS binary frame (R1 §4), (7) `%pause` 수신 시 UI 정책 (R1 §5)을 *디폴트 후보*로 제시했고, 본 ADR은 이를 *단정문 결정*으로 격상하면서 Grill의 추가 입력 제약(D8 command allowlist, D15 ring buffer 128 KB, D16 Panel Streaming State pause/continue, D19 p99 < 100ms 성능 예산)을 흡수한다.

본 ADR은 ADR-0008이 정의한 *command allowlist 정본 표*(`docs/adr/0008-single-pane-window-and-group.md` §tmux command allowlist 표)와 ADR-0009가 정의한 *dedicated daemon 소켓 컨벤션*(`tmux -L gtmux-<session>`)을 상속하며, 본 ADR 안에서 다시 정의하지 않는다 — 인용만 한다. 새로운 발급 명령이 필요해지면 ADR-0008의 표를 먼저 갱신한 후 본 ADR을 amend한다.

## 결정 (Decisions)

- **D1.** [R1 §1·§9, §"구체 권장"·4] gtmux backend의 tmux 통신 채널은 **`tmux -C` (라인 지향 control mode) 단 하나**다. `-CC` (DCS 래핑 variant)는 *터미널 에뮬레이터가 직접 띄울 때 필요한 모드*이므로 백엔드에서는 영구 금지된다 — ADR-0008 allowlist 표의 `-CC ❌` 항이 이를 표현한다. 백엔드가 stdin/stdout을 직접 잡는 환경에서 DCS 래핑은 오히려 라인 파서를 깨뜨릴 위험만 발생시킨다.
- **D2.** [R1 §8, §"구체 권장"·1] **최소 tmux 버전 = 3.2, 권장 3.4+, CI/문서 기준선 = 3.5**. Server 부팅 첫 단계에서 `tmux -V`(또는 control mode 진입 후 `display-message -p '#{version}'`)를 호출하여 버전을 확인하고, 3.2 미만이면 **exit 6**(tmux daemon 통신 실패, ADR-0009 D6 / Grill D20 exit code 규약)으로 즉시 거부한다. 3.2 미만은 `pause-after`/`%pause`·`%continue`·`refresh-client -B` 구독이 부재하여 D7·D8·D9의 백프레셔·구독·Panel Streaming State 결정이 *구조적으로 성립하지 않으므로* degraded fallback도 두지 않는다.
- **D3.** [R1 §1·§3·§6·§7, §"구체 권장"·2] **부트스트랩 순서는 다음 4단계로 고정한다** — 각 단계는 직전 단계의 완료(대응하는 `%end`)를 기다린 후에만 다음으로 진행한다.
    1. **Snapshot**: `list-sessions -F …` → `list-windows -a -F …` → `list-panes -a -F …`를 단일 FIFO 큐로 순차 송신, 응답으로 초기 Session·Window·Pane 트리를 메모리에 구성. (control mode 알림은 *변화*만 보내므로 초기 상태는 반드시 폴링 1회로 시드해야 함 — R1 §7.)
    2. **Pause-after enable**: `refresh-client -f 'pause-after=<sec>'`로 백프레셔 활성화. `<sec>` 값은 D9 참조.
    3. **Subscriptions register**: 필요한 동적 포맷에 대해 `refresh-client -B <name>:<what>:<format>` 등록. MVP 대상 구독 집합은 §미해결 O3 참조 — 본 ADR은 *순서*만 잠근다.
    4. **Live mode**: 이후 모든 상태 변화는 `%output`/`%extended-output`/`%window-*`/`%session-*`/`%pane-*`/`%layout-change`/`%subscription-changed` 등 `%` 알림으로 push 수신. *이 시점 이후* 폴링은 명시 사용자 액션(deep scrollback `capture-pane`)에만 한정한다.
- **D4.** [R1 §2·§9, §"구체 권장"·3] 명령 전송은 **단일 FIFO 큐**로 직렬화하고, 응답은 `%begin t n f` / `%end t n f` / `%error t n f`의 **command-number `n` 매칭**으로 상관시킨다. 한 클라이언트 안에서 tmux는 명령 응답 블록 사이에 다른 명령의 `%begin`을 끼워 넣지 않으므로(R1 §2, DeepWiki 인용) iTerm2 `TmuxGateway`와 동일한 단순 단일 큐 + FIFO 매칭으로 충분하다. 응답을 기다리는 동안 사이에 들어오는 `%`-prefixed *알림*(output/notify)은 별개 dispatcher로 흘려 보내되, 그 알림이 명령 응답 순서를 침범하지 않는다는 R1 §2의 invariant에 의존한다.
- **D5.** [R1 §"구체 권장"·6 / Grill D8 + ADR-0008] tmux 명령은 **ADR-0008 §tmux command allowlist 표가 정의하는 발급 가능 명령 집합** 안에서만 발급된다. 본 ADR은 그 표를 재정의하지 않고 *상속*한다 — 특히 `split-window` · `resize-pane` · `select-layout` · `-CC`는 영구 금지다 (D8 single-pane-per-window 컨벤션 + R1 §9 size 강제 회피). 새 명령을 추가하려면 ADR-0008의 표를 먼저 갱신하고 본 ADR을 amend한다.
- **D6.** [R1 §5·§11, §"구체 권장"·5] **`refresh-client -C <w>x<h>`(control 클라이언트의 가상 size 협상 진입)는 MVP에서 호출하지 않는다.** R1 §11에 따르면 호출하지 않은 control 클라이언트는 다른 클라이언트의 size에 영향을 주지 않는다 — gtmux는 외부 attach 클라이언트(별도 터미널의 `tmux a -t <session>`)의 size 협상 결과를 *방해하지 않고 mirror only*로 동작한다 (Grill D5 mirror-only 정합). 멀티 뷰/multi-view 시나리오 도입 시 본 ADR을 amend.
- **D7.** [R1 §4, §"구체 권장"·6 / Grill D15] **`%output` 처리 파이프라인**:
    - **Decode**: 라인 단위 파싱 → 8진수 이스케이프(`\NNN`)만 역치환 → 원 바이트 복원 (R1 §4: ≥0x20 바이트는 그대로, 0x00–0x1F 및 `\`만 이스케이프). UTF-8 멀티바이트·ANSI escape 시퀀스는 별도 처리 없이 보존.
    - **Per-pane ring buffer**: 디코딩된 바이트를 페인별 ring buffer에 append. **크기 = 128 KB 기본**(50 pane × 128 KB = 6.4 MB Server RAM), **`<session>.config.toml` `[runtime] ring_buffer_size_kb`로 사용자 설정 가능** (Grill D15·D22). **메모리 전용 — disk 영속화 금지** (Grill D15: STDOUT 비밀 정보 노출 위험 + hot-path I/O 비용).
    - **WS binary frame**: 디코딩 바이트를 그대로 binary frame envelope(`PANE_OUT`)로 송출. 인코딩 정의는 ADR-0002 + `docs/ssot/wire-protocol.md`가 0x01–0x0F tmux-domain 영역에서 정한다 — 본 ADR은 *paneId + raw bytes* 페이로드 계약만 가정한다.
    - **`%extended-output` 동치성**: `pause-after`가 활성화되면 `%output` 대신 `%extended-output %<pane-id> <age-ms> : <data>`가 들어온다 (R1 §4). 같은 디코딩 규칙을 적용하고, `<age-ms>`는 telemetry에만 기록 (UI 노출 안 함).
- **D8.** [Grill D16, R1 §5 + §11.B / sketch §14·8] **Panel Streaming State lifecycle을 control mode 명령으로 구현한다.**
    - Panel의 `visibility=hidden` 또는 `minimized=true` 전이 → `Suspended` → `refresh-client -A '%<pid>:pause'`.
    - 그 외 전이 → `Streaming` → `refresh-client -A '%<pid>:continue'`.
    - **300ms 디바운스** — 빠른 토글 시 명령 폭주 방지 (Grill D16 + D22 `[runtime] panel_state_debounce_ms`).
    - MT-3 일관성: visibility는 모든 WS 연결에 broadcast되는 단일 상태 (CONTEXT.md "Multi-connection 정책")이므로 전이 결정도 글로벌 단일.
    - **Long-suspend 검증 항목** (sketch §14·8 + Grill D16 후속): R1 §5는 *수동 pause*(`refresh-client -A '%<id>:pause'`) 시에도 tmux 내부 버퍼가 CONTROL_BUFFER_HIGH(8192B) 이상 누적되거나 클라이언트가 300초 이상 뒤처지면 강제 disconnect 정책이 적용되는지를 명시적으로 답하지 않는다. **R7 구현 검증 항목** — long-suspend 시 (a) 누적 버퍼 동작, (b) 강제 disconnect 임계 적용 여부 측정. 측정 결과 disconnect가 발생하면 `pause` 대신 `off`로의 자동 승격(예: 5분 이상 Suspended 시 `refresh-client -A '%<pid>:off'`)을 D8 amend로 도입.
- **D9.** [R1 §5·§11, Grill D19 성능 예산] **`pause-after` 임계값 = MVP 10초, stretch 5초**. 근거 — D19의 *Per-pane output latency p99 < 100ms* 목표는 tmux 측 버퍼 누적 시간이 *수 초* 단위로 머무는 것을 허용한다는 의미이며, R1 §5의 default 후보 5/10/30 중 **10초**가 (a) 빌드 로그 폭주 시 정상 burst를 즉시 끊지 않고 (CONTROL_BUFFER_LOW 512B / HIGH 8192B 사이의 정상 흐름은 통과시킴), (b) WS write lag 5초 예산(D19) + 클라이언트 처리 여유 안에 들고, (c) 강제 disconnect 임계(300초)와 충분한 안전 거리를 둔다. stretch 목표(WS write lag < 1초)를 잡을 때는 5초로 좁힌다. 값은 `[runtime].pause_after_sec` config로 노출 (D22 amend로 추가 예정 — 본 ADR이 그 필드 도입을 *후속 작업*으로 트리거).
- **D10.** [R1 §"구체 권장"·7 / Grill D16] **`%pause %<pid>` 수신 시 UX 정책**: UI는 해당 Panel header에 "느림(slow)" 배지를 표시. 자동 catch-up은 *Panel Streaming State*가 `Streaming`인 경우에만 적용 — 즉 사용자가 그 Panel을 보고 있는 동안 backend가 `refresh-client -A '%<pid>:continue'`를 자동 발급해 따라잡는다. Suspended 상태이면 catch-up을 보류하고 다음 Streaming 전이 시 재개. 수동 catch-up 토글 UI는 P1+.
- **D11.** [R1 §1·§3 / ADR-0009 D2] tmux daemon 연결 진입점은 **`tmux -L gtmux-<session> -C attach -t <session>`** 정확히 1회. `-L gtmux-<session>` 소켓 컨벤션은 ADR-0009 D2가 정본이며 본 ADR은 인용만 한다. control mode 클라이언트 인스턴스는 Server 프로세스 안에 *정확히 1개*(ADR-0007 D1·D2의 1:1:1 모델 정합).
- **D12.** [R1 §11 함정 / Grill D21 c5·c8] **연결 종료 / 외부 kill / SIGHUP 누수 정책**:
    - 입력 채널(stdin) 정규화: 사용자 입력 또는 내부 명령에 **빈 줄(only `\n`) 송신 금지** — tmux가 빈 줄을 detach 트리거로 해석한다 (R1 §1·§11). 명령 라우터는 모든 outbound 라인이 non-empty임을 *어서션*한다.
    - CRLF 정규화: 내부 어디서든 outbound 명령은 `\n` 종결로 강제하고 `\r`은 strip (R1 §11).
    - SIGHUP 누수(R1 §11, tmux issue #3084): stdin EOF 또는 부모 PID 변화를 명시 모니터링하여 control mode client를 cleanup. ADR-0009 D6 teardown 절차와 별개로 *Server 프로세스가 외부에서 죽을 때*의 정리 책임을 본 ADR이 명시.
    - `tmux kill-server` 또는 외부 session kill 시 `%exit`를 받으면 자동 재시도하지 않고 ADR-0007 D4 / Grill D21 c5에 따라 Server 프로세스가 graceful shutdown(WS close + layout flush) 후 exit 6 종료.
    - **[2026-05-14 amend]** `%exit` 수신 경로의 범위 명확화 — 다음 세 경로 모두 동일 exit 6 흐름으로 합류한다:
        1. 외부 `tmux kill-server` / `tmux kill-session` 명령으로 인한 `%exit` (원안).
        2. **UI 의 Session shutdown 액션이 발사한 `kill-session`** 으로 인한 `%exit` (CONTEXT.md §"tmux invariant 의 UI 측 mirror" + ADR-0009 §D5 amend 정합). 사용자 명시 의도이므로 graceful shutdown 정신 그대로.
        3. (예외) 마지막 Window 가 외부 CLI 의 의도치 않은 동작으로 사라져 tmux server 가 자체 종료한 경우. 단 *UI 가 보유한 Panel 경로* 로는 이 경로가 발생하지 않도록 ADR-0009 §D5 prevention (close 비활성 + auto-mount) 가 차단. 우리 UI 가 만든 종료가 아닌 *진짜 외부* 사건만 본 경로로 흡수.
        - LIFE-AUTOSPAWN (자동 재기동) 은 명시 거절. 자세한 trade-off 는 `docs/reports/0022-logic-amendment-decisions.md` §1.2 / §1.3 참조.

- **D13.** **[2026-05-14 추가]** **argv 안전 quoting** (`lifecycle::serialise_command` 의 단일 책임).
    - 배경: tmux control-mode stdin 의 명령 파서는 shell 이 아닌 *tmux 자체* 의 토큰 규칙을 따른다. 인용 밖의 `#` 가 line-comment 시작 문자로 해석되어 `new-window -P -F #{pane_id}` 가 `-F` 만 남고 *인자 없음 에러* 로 죽는 quirk 가 본 세션 데모 안정화에서 발견되었다 (`8cbadee`, `docs/reports/0020-debug-classification.md` §3.1.6 / L-7).
    - **결정**: argv 토큰을 직렬화할 때, 토큰이 다음 문자 중 **하나라도** 포함하면 single-quote 로 감싼다 — `#`, whitespace (공백·탭), `'`, `"`, `\`. 안전 문자만 포함한 토큰 (`[A-Za-z0-9_./%@:=-]+`) 은 unquoted 그대로 통과한다 (로그 가독성 보존).
    - **Escape 규칙**: 토큰 내부의 `'` 는 shell-style `'\''` 패턴으로 표현 — 즉 *현재 single-quote 닫기 → escaped quote → single-quote 다시 열기* (`foo'bar` → `'foo'\''bar'`). tmux 의 single-quote 안에서는 `\` 가 일반 문자이므로 `\` 자체는 추가 escape 불요 (그러나 안전 집합에 포함되어 quoting 트리거).
    - **적용 위치**: `lifecycle::serialise_command` 가 단일 책임. 호출자 (cmd_router, http_api 핸들러 등) 는 raw argv 만 전달한다. 호출자 측 별도 quoting 금지 (이중 인용 위험).
    - **`-F #{pane_id}` 복귀 가능성**: 본 quoting 적용 후 `-F` 의 format-string 인자 (`#{pane_id}`) 가 `'#{pane_id}'` 로 안전하게 통과 → `new-window -P -F #{pane_id}` 정공 복귀 가능. 단 `S6-BE-CTRL-ACK` (CTRL response 정식 wire) 가 함께 완성되면 response 의 출력을 파싱해 pane_id 를 받을 수 있으므로 `-F` 의존도 자체가 줄어든다. 둘은 독립 결정 — quoting 자체는 다른 argv 경로 (사용자 입력 label 등) 에서도 필요하므로 그대로 적용.
    - **거절안**: (a) Universal always-quote — 로그 가독성 ↓, 디버깅 비용 누적. (b) Unsafe args reject — user-supplied 값 경로 (send-keys 등) 가 unsafe 일 수 있어 완전 제거 불가. 자세한 비교는 `docs/reports/0022-logic-amendment-decisions.md` §5.2.
    - **회귀 테스트 의무**: argv 단위 quoting 정확성 unit test — 각 안전 vs unsafe 문자 케이스 + escape 케이스 × 최소 6 종.
    - Result: this clause was added retroactively after the debug session 2026-05-14. See `docs/reports/0020-debug-classification.md` §3.1.6 + `docs/reports/0022-logic-amendment-decisions.md` §5.

## 거절된 대안 (Rejected)

- **R1. 스크린 스크레이핑** — `capture-pane -p -e -J -S -<lines>` 또는 외부 tmux 클라이언트가 그리는 화면을 *주 라이브 출력 채널*로 폴링하는 안. 거절 이유: (a) `capture-pane`은 *스크린샷* 의미라 터미널 동작(커서·alt-screen·`%pane-mode-changed`) 자체가 재현되지 않음 (R1 §7), (b) 폴링 주기보다 짧은 출력 burst가 통째로 누락됨, (c) tmux native 알림 14종(`%window-*`/`%session-*`/`%pane-mode-changed`/`%layout-change`/`%subscription-changed` …)이 *영원히 들어오지 않음* → 5대 불변식 #5 (control mode 사용) 정신 정면 위배. R1 §"옵션 비교표" 1행 참조. `capture-pane`은 Grill D15 후속으로 **사용자 명시 deep scrollback 회복 액션(P1+)에만 한정** — control 채널 위에서 명령으로 발급.
- **R2. `tmux list-sessions / list-windows / list-panes` 등 명령을 주기적으로 셸아웃 폴링** — 알림 푸시 대신 외부 셸에서 `tmux ...` 명령을 반복 실행하여 상태 변화 감지. 거절 이유: (a) 폴링 주기 = 상태 변화 인지 latency 하한, D19의 p99 < 100ms·*panel sync* < 500ms 예산 직접 위반, (b) `%output` 같은 라이브 스트림은 폴링으로 흉내 불가, (c) tmux 인스턴스 N개(50 Server × tick)의 fork/exec 오버헤드 누적, (d) 명령 인젝션 표면이 *문자열 셸 보간*으로 N배 증가 (sketch §13.3.3 위반). `list-*`는 **부트스트랩 1회 스냅샷(D3 step 1)에만 사용**하며 그것도 control mode 채널 안에서 `%begin`/`%end`로 감싸 발급 — fork/exec 셸아웃 아님 (R1 §"옵션 비교표" 5행).
- **R3. `tmux -CC`를 백엔드에서 사용** — control mode의 DCS 래핑(`\033P1000p` … `%exit` … `ST`) variant. 거절 이유: (a) `-CC`는 *호스트 터미널 에뮬레이터*(iTerm2 등)가 통신을 시각 모드와 구분하기 위한 래핑이며 *백엔드가 stdin/stdout을 직접 잡는 경우* 라인 파서가 DCS 바이트와 `%`-알림 라인을 혼동할 위험만 추가 (R1 §1·§9), (b) ADR-0008 §command allowlist 표가 `-CC ❌`로 영구 금지 — 본 ADR은 그 결정을 재확인. 백엔드는 항상 `-C`.
- **R4. `pause-after` 미적용 + 무제한 버퍼링** — 백프레셔 없이 tmux의 내부 버퍼에 의존. 거절 이유: R1 §5·§"옵션 비교표" 3행이 명시 — 클라이언트가 *300초 이상 뒤처지면 tmux가 강제 disconnect*. 부하 상황에서 연결이 끊기는 것은 D19의 안정성 예산 + sketch §14·1 "tmux output과 canvas 상태를 안정적으로 동기화" 직접 위반.
- **R5. 명령 응답을 multi-queue 또는 channel별로 분리** — 페인별 명령 큐를 여러 개 두어 동시에 송신. 거절 이유: R1 §2의 invariant("같은 클라이언트 안에서 명령 응답 블록은 끼어들지 않음")는 *단일 송신 직렬*을 전제로 한다. multi-queue로 명령 송신이 인터리브되면 tmux가 응답을 어느 큐로 보낼지 클라이언트가 알 방법이 없다 — command-number `n` 매칭은 작동하지만 *수신 순서가 보장되지 않으므로* 상태 머신이 복잡해진다. iTerm2 `TmuxGateway`가 단일 큐를 채택한 이유와 동일.
- **R6. ring buffer를 disk persistence** — Server 재시작 후에도 과거 출력 보존을 위해 disk에 flush. 거절 이유: Grill D15 — (a) STDOUT에 비밀 정보(API 키, password prompt 응답 등) 가능성으로 §13.3.5 "저장 데이터 노출 위험" 확장, (b) hot-path I/O 비용(50 pane × 출력 burst)이 D19의 backend memory baseline < 30 MB·per-pane latency p99 < 100ms 예산을 침범. Server restart 시 ring은 비어 있고 deep scrollback은 `capture-pane`으로 P1+ 회복.

## 결과 (Consequences)

- 긍정:
    - **단일 채널** — Server 프로세스 안에 control mode 클라이언트 1개·송신 큐 1개. 5대 불변식 #5 그대로 충족 + ADR-0007의 1:1:1 모델과 자연 합성.
    - **백프레셔 보장** — `pause-after=10` + `%pause`/`%continue` + Panel Streaming State (D8) 합성으로 부하 상황에서도 강제 disconnect를 피하고 D19의 latency·메모리 예산을 지킨다.
    - **iTerm2 검증 패턴** — 단일 FIFO + command-number 매칭은 R1 §"옵션 비교표"와 iTerm2 `TmuxGateway`가 14년간 검증한 패턴 (R1 §9). 구현 위험 ↓.
    - **command allowlist 자동 축소** — ADR-0008 표 인용으로 `split-window`/`resize-pane`/`select-layout`/`-CC` 영구 금지. sketch §13.3.3 명령 주입 표면 *구조적* 축소.
    - **버전 강제로 degraded path 0** — 3.2 미달 거부 → `pause-after`/구독 부재 시나리오를 코드 안에서 처리할 필요 없음. 분기 ↓.
- 부정/비용:
    - **tmux 3.2 미만 환경 차단** — 일부 LTS Linux 배포판(예: Debian 11 buster 기본 tmux 2.x)에서 사용자 별도 빌드 필요. 문서에 명시.
    - **`%output` 디코딩 비용** — 50 pane × burst 시 8진수 역치환 hot-path. Rust의 `bytes` crate + SIMD 가능 영역이지만 R7 benchmark에서 검증 항목으로 측정 (O3).
    - **Long-suspend 시 buffer 동작 미검증** — D8 검증 항목이 *blocker*는 아니지만 구현 단계에서 결과에 따라 D8을 amend해 `off` 자동 승격을 도입할 가능성.
    - **D9 `pause-after` 임계값 10초는 잠정** — 실측 데이터 부재 상태의 합리적 기본값. R7 benchmark가 50 pane × 5 burst 시나리오에서 실제 p99을 측정해 5–10초 사이 재조정 가능.
- 후속 작업:
    - **ADR-0002** (전송 계층 + wire-protocol SSoT) — 본 ADR의 D7 페이로드 계약(`PANE_OUT = paneId + raw bytes`)을 0x01–0x0F tmux-domain envelope에 그대로 정의. `refresh-client -A` pause/continue는 backpressure 신호로 ADR-0002의 큐 워터마크와 동일 메커니즘.
    - **ADR-0003** (보안 디폴트) — 본 ADR의 D5 allowlist 인용 + D12 입력 정규화(빈 줄 금지, CRLF strip)를 보안 SSoT(12개 체크리스트)의 명령 주입 항목에 흡수.
    - **ADR-0011** (Rust backend) — D8 long-suspend 검증·D9 `pause-after` 임계값 측정을 R7 benchmark DoD에 포함 (O3 closed 트리거).
    - **`<session>.config.toml` [runtime] amend** — `pause_after_sec` 필드 추가 (D9 노출). Grill D22 amend로 도입.
    - **A4 정합성 리뷰** — 본 ADR + ADR-0002 + ADR-0003 cross-reference (`docs/reports/0009-adr-coherence-review.md`). Status를 Accepted로 승격하는 게이트.

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태/웹 상태 분리 | **PASS** — 본 ADR이 control mode 채널 위에서 흐르는 모든 데이터(`%output`/`%window-*`/`%session-*`/`%pane-*`/`%layout-change`/`%subscription-changed`)를 *tmux state*로 명시적으로 분류하고, 한 방향(tmux → web)으로만 mirror한다. Panel geometry/visibility/lock/z/label/note 같은 *web state*는 본 ADR이 정의하는 어떤 명령으로도 tmux에 전달되지 않는다. 특히 D7은 ring buffer를 *서버 측 메모리*에 두어 web state 영속화 경로(ADR-0006 HTTP `PUT /api/layout`)와 *물리적으로 분리*된 자원을 사용한다. D8의 `refresh-client -A '%<pid>:pause'`는 web state(visibility) 변화에 *반응*하여 tmux state 흐름을 제어할 뿐, web state 값을 tmux에 *저장*하지 않는다. |
| 2 | tmux-native vs web-only 분기 | **PASS** — 본 ADR이 발급하는 모든 tmux 명령(D3 부트스트랩 list-*, D5 ADR-0008 allowlist, D8 `refresh-client -A`, D11 attach)은 *전부 tmux-native* 라우팅이다. D5가 인용하는 ADR-0008 §command allowlist 표가 이 분기를 *기계적으로* 강제 — 표 밖의 명령은 발급 불가, 표 안 명령 중 `split-window`/`resize-pane`/`select-layout`은 영구 금지로 single-pane-per-window 컨벤션을 깬다. web-only 액션(panel drag, hide, lock, group)은 본 ADR의 어떤 명령도 트리거하지 않는다 — 단, D8의 visibility 전이가 *Panel Streaming State*라는 *데이터 흐름 제어 신호*를 tmux에 보내는데, 이는 tmux Window/Pane의 *생성·소멸·이름* 같은 tmux state를 변경하지 않으므로 분기 위반이 아니다. |
| 3 | tmux Layout ≠ Canvas Layout | **PASS** — 본 ADR이 `%layout-change` 알림을 수신하지만 *변경 감지 트리거*로만 사용하고 (R1 §7 명시) layout 문자열을 캔버스 좌표로 *변환하지 않는다*. ADR-0008 single-pane-per-window 컨벤션 하에서 gtmux-created Window의 tmux Layout은 trivial(window-size = pane-size, 1:1)이므로 비교 대상 자체가 없고, 외부 multi-pane Window는 ADR-0008 D4가 별도 mirror 정책(canvas resize 잠금)으로 처리한다 — 본 ADR은 그 정책을 *다시 정의하지 않음*. D5가 `select-layout` 발급을 금지하므로 gtmux는 tmux Layout을 *능동 변경하지 않는다*. |
| 4 | 보안 기본값 | **PASS** — (a) D5가 ADR-0008 allowlist를 상속하여 명령 어휘를 고정 집합으로 제한, (b) D12가 빈 줄·CRLF·SIGHUP 누수를 명시적으로 차단하여 입력 채널 인젝션 표면(sketch §13.3.1·§13.3.3)을 좁힘, (c) `send-keys -t %<pid>` argv 분리는 ADR-0003가 별도 SSoT로 정본화하지만 본 ADR이 그 라우팅을 *유일한 입력 경로*로 못박음(라벨/노트는 tmux로 절대 흐르지 않음 = 인젝션 표면 없음), (d) D11이 ADR-0009 dedicated daemon 소켓에만 attach하므로 사용자 main tmux 환경에 transit하지 않음. |
| 5 | control mode 사용 | **PASS (강함)** — 본 ADR의 모든 결정이 *이 불변식의 정본*이다. D1이 단일 채널을 잠그고, D2가 control mode 기능(`pause-after`/구독)이 작동하는 버전 하한을 강제하고, D3가 부트스트랩 순서로 *control 채널 안에서만* 데이터를 받는다는 약속을 잠그고, R1·R2가 채택 가능한 비-control 대안(스크린 스크레이핑, 셸아웃 폴링)을 명시 거절한다. 본 ADR 시행 후 gtmux 백엔드 → tmux 통신은 **단일 stdin/stdout 라인 프로토콜 외 경로가 존재하지 않는다**. |

## 미해결 항목 (Open)

- **O1.** **Long-suspend 시 tmux 내부 버퍼 누적 + CONTROL_BUFFER_HIGH 강제 disconnect 적용 여부** (D8 검증 항목, sketch §14·8) → **R7 benchmark 보고서**에서 측정 후 본 ADR D8 amend로 결과 반영. 측정 결과 수동 pause에서도 disconnect가 발생하면 `pause` → `off` 자동 승격(예: 5분 이상 Suspended) 도입.
- **O2.** **`pause-after` 임계값 D9의 10초 잠정값 검증** → R7 benchmark에서 50 pane × 5 burst 시나리오로 p99 latency 측정 후 5–10초 사이 재조정. D19 p99 < 100ms 예산이 정량 게이트.
- **O3.** **MVP 구독(`refresh-client -B`) 대상 포맷 집합** — R1 §6이 후보로 제시한 `pane_current_path`, `pane_title`, `pane_current_command`, `pane_dead`, `window_active`, `session_attached` 중 어떤 것을 등록할지는 UX 요구(panel header 표시, zombie badge 등)에 따라 결정. → **ADR-0004**(터미널 렌더링, B1 후속) 또는 별도 UX ADR에서 closed. 본 ADR은 *D3 step 3의 자리만 잠그고* 집합은 비워 둠.
- **O4.** **`TMUX` 환경변수 nested attach 처리** → ADR-0009 O1과 통합 closed. 본 ADR의 D11이 ADR-0009 D2 소켓 컨벤션을 인용하므로 그 ADR의 결과를 그대로 받는다.
- **O5.** **A4 정합성 리뷰 게이트** → `docs/reports/0009-adr-coherence-review.md`에서 본 ADR과 ADR-0002·ADR-0003·각 SSoT의 cross-reference 점검 후 Status를 Accepted로 승격. 본 단계에서는 Proposed 유지.
