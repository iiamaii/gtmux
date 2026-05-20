# 보고서: tmux control mode (tmux -C) 프로토콜·이벤트·한계

## 요약 (3문장)

tmux control mode는 `tmux -C` 또는 `tmux -CC` 클라이언트가 stdin/stdout 위에서 텍스트 줄 프로토콜로 명령을 보내고, `%`로 시작하는 비동기 알림(`%begin`/`%end`/`%error`로 감싼 명령 응답과 `%output`/`%session-changed`/`%window-add`/`%layout-change` 등 상태 변경 푸시)을 받는 공식 통합 인터페이스이며, iTerm2가 사실상의 레퍼런스 구현이다 [1][2][6]. 초기 상태(세션·윈도·페인 트리)는 여전히 `list-sessions`/`list-windows -F`/`list-panes -F` 폴링으로 한 번 스냅샷을 잡아야 하지만, 이후의 변화는 알림 + tmux 3.2+의 `refresh-client -B` 포맷 구독으로 푸시받을 수 있어 gtmux의 "tmux state는 거울처럼 미러링" 불변식과 정확히 일치한다 [1][3][6]. gtmux는 control mode를 단일 전송 채널로 채택하되, 백프레셔(`pause-after` + `%pause`/`%continue` + `%extended-output`)와 구독을 사용하려면 최소 tmux 3.2를 강제해야 하며, 안전한 기본값을 위해 3.4 이상을 권장한다 [3][7].

## 조사 범위와 질문

본 보고서는 gtmux의 첫 ADR(전송·tmux 통합 전략)을 위한 R1 트랙 결과물이다. 다음을 1차 자료 위주로 확인했다.

- `tmux -C` / `-CC`의 진입·종료 시맨틱과 DCS 래핑.
- 모든 `%` 알림의 이름·인자·발생 시점.
- 명령–응답 상관(`%begin`/`%end`/`%error`의 정수 명령 번호 매칭) 및 순서 보장.
- `%output` 인코딩 규칙(8진수 이스케이프, UTF-8/바이너리 안전성)과 `%extended-output`의 차이.
- 흐름 제어: `pause-after`, `%pause`, `%continue`, `refresh-client -A`, 버퍼 워터마크.
- 포맷 구독(`refresh-client -B`)과 `%subscription-changed` (3.2+).
- control mode로 *오지 않는* 정보 — 폴링이 여전히 필요한 영역.
- 버전별 추가 시점(2.0 → 3.6) 및 gtmux 최소 버전.
- iTerm2의 통합 방식과 다른 구현(tmate, gotty/ttyd/wetty, Ghostty, Windows Terminal)의 채택 현황.
- 알려진 함정: SIGHUP 누수, attach-session 사이즈 협상, `\n` vs `\r`, 빈 줄 detach.

## 핵심 발견

### 1. 진입·종료와 DCS 래핑

- `-C`는 평범한 줄 지향(canonical) control mode 클라이언트로 시작한다. 명령은 newline-terminated 텍스트 라인이며, **빈 줄(엔터만)** 을 보내면 클라이언트가 detach된다 [1][2].
- `-CC`는 같은 프로토콜을 사용하지만 진입 시 `\033P1000p` (DCS 시퀀스)를 출력하고 종료 시 `%exit` 라인 뒤 `ST(\033\\)` 시퀀스를 출력하여, 호스트 터미널 에뮬레이터(iTerm2)가 "지금부터/지금까지 control mode"임을 검출하도록 한다 [1][2]. 즉 `-CC`는 *사용자가 직접 터미널에서 띄울 때* 필요한 모드이고, gtmux처럼 별도 프로세스가 stdin/stdout을 잡는 경우 `-C`만으로 충분하다.
- `%exit`는 control mode 클라이언트가 종료될 때 마지막으로 한 번 출력된다. `refresh-client -f wait-exit` 플래그를 켜둔 클라이언트는 `%exit` 뒤에 빈 줄이 들어올 때까지 실제 종료를 지연한다(클라이언트가 마지막 알림까지 모두 소비하도록 보장) [4][7].

### 2. 명령 응답 블록 형식 (`%begin`/`%end`/`%error`)

- 클라이언트가 보낸 모든 tmux 명령은 한 쌍의 가드 라인으로 감싼 출력 블록으로 응답된다 [1][6].
  - 성공: `%begin <seconds-since-epoch> <command-number> <flags>` … (명령 출력, 여러 줄 가능) … `%end <seconds-since-epoch> <command-number> <flags>`
  - 실패: `%begin … <number> …` … 오류 메시지 … `%error <seconds> <number> <flags>`
- `%begin`과 대응하는 `%end`/`%error`는 **같은 timestamp와 command number**를 가지며 이 정수 명령 번호가 상관(correlation) 키이다 [1][6]. 따라서 클라이언트는 송신 순으로 명령을 큐잉하고, FIFO로 응답을 매칭하면 충분하다 — iTerm2의 `TmuxGateway` 역시 같은 방식의 단일 큐 + 매칭 디스패치를 쓴다 [4][6].
- DeepWiki가 정리한 tmux 소스 분석에 따르면 `%output` 블록은 페인별·전역 두 큐로 나뉘어 흐르고 알림은 전역 큐로 흐르며, "다른 종류 출력 사이의 엄격한 순서 보장"이 유지된다 [6]. 즉 한 명령의 `%begin`/`%end` 사이에 다른 명령의 `%begin`이 끼어들 일은 없다(같은 클라이언트 안에서).

### 3. 모든 `%` 알림 목록 (tmux 마스터 기준)

`control-notify.c`와 `control.c` 소스에서 직접 확인한 알림 [5][6]:

| 알림 | 인자 | 발생 시점 |
|---|---|---|
| `%begin t n f` / `%end t n f` / `%error t n f` | timestamp, 명령 번호, 플래그 | 모든 명령 응답을 감쌈 [1][6] |
| `%output %<pane-id> <data>` | 페인 ID, 인코딩된 바이트 | 페인이 출력했을 때 [1][5] |
| `%extended-output %<pane-id> <age-ms> : <data>` | 페인 ID, 밀리초 지연, `:` 구분, 데이터 | `pause-after` 활성화 시 `%output` 대신 사용 [1][5][6] |
| `%pause %<pane-id>` | 페인 ID | 페인이 자동·수동 일시 정지될 때 [1][5][7] |
| `%continue %<pane-id>` | 페인 ID | `refresh-client -A '%<id>:continue'`로 재개 시 [1][5] |
| `%session-changed $<id> <name>` | 세션 ID, 이름 | 본 클라이언트가 다른 세션에 붙을 때 [1][5] |
| `%client-session-changed <client> $<id> <name>` | 클라이언트명, 세션 ID, 이름 | 다른 클라이언트가 세션을 바꿀 때 [1][5] |
| `%session-renamed $<id> <name>` | 세션 ID, 새 이름 | 세션 이름 변경 [1][5] |
| `%sessions-changed` | (없음) | 세션 생성/소멸 [1][5] |
| `%session-window-changed $<sid> @<wid>` | 세션 ID, 현재 윈도 ID | 세션의 현재 윈도 변경 [1][5] |
| `%window-add @<wid>` / `%unlinked-window-add @<wid>` | 윈도 ID | 붙어 있는 세션 / 다른 세션에 윈도 추가 [1][5] |
| `%window-close @<wid>` / `%unlinked-window-close @<wid>` | 윈도 ID | 윈도 닫힘 [1][5] |
| `%window-renamed @<wid> <name>` / `%unlinked-window-renamed @<wid> <name>` | 윈도 ID, 새 이름 | 윈도 이름 변경 [1][5] |
| `%window-pane-changed @<wid> %<pid>` | 윈도 ID, 활성 페인 ID | 윈도의 활성 페인 변경 [5] |
| `%pane-mode-changed %<pid>` | 페인 ID | 페인 모드(copy/view 등) 변경 [1][5] |
| `%layout-change @<wid> <layout> <visible-layout> <raw-flags>` | 윈도 ID, 정규 레이아웃 문자열, 보이는 레이아웃, 플래그 | 윈도 분할/리사이즈로 layout이 달라질 때 [5] |
| `%client-detached <client>` | 클라이언트명 | tmux 3.2+ 다른 클라이언트의 detach 알림 [3][5] |
| `%subscription-changed <name> $<sid> @<wid> <idx> %<pid> : <value>` | 구독명, 세션·윈도·idx·페인 키, `:`, 새 값 | 구독한 포맷의 평가 결과가 변할 때 (3.2+) [1][6] |
| `%config-error <message>` | 메시지 | 설정 파일 오류 (3.1+) [1][3] |
| `%paste-buffer-changed <name>` / `%paste-buffer-deleted <name>` | 버퍼명 | 클립보드 버퍼 변경/삭제 (3.5+에서 이름 수정) [5][8] |
| `%message <text>` | 텍스트 | `display-message`가 status 대신 control 클라이언트에 전달되는 경로 (2.4+ `display-message` 확장) [3] |
| `%exit [reason]` | 선택적 사유 | 클라이언트 종료 직전 [1][2] |

> 주의: 일부 블로그에서 언급되는 `%noop`은 마스터 소스에서 발견되지 않는다. 전사하지 말 것.

### 4. `%output` 인코딩과 바이너리·UTF-8 안전성

- `%output`의 데이터 필드는 **"ASCII 32 미만의 모든 문자와 `\`는 8진수 `\xxx`로 치환"** 된다. 따라서 `\`는 `\134`로 나타나고, ESC(0x1B)는 `\033` 등으로 나타난다 [1][6]. 그 외 0x20 이상의 바이트(UTF-8 멀티바이트 본체 바이트 포함, ≥0x80도 포함)는 **그대로** 전달된다 [1][6].
- 따라서 디코딩은 단순한 라인 기반 파서로 가능하다: 줄 끝까지 읽은 뒤 `\NNN` 토큰만 역치환하면 원 바이트가 복원된다. 멀티바이트 UTF-8은 별도 처리가 필요 없다.
- **흐름 제어가 켜져 있으면** `%output` 대신 `%extended-output %<pane-id> <age-ms> : <data>`가 사용된다. 인코딩 규칙은 동일하지만, 페이로드 앞에 `age`(이 청크가 tmux 내부에 쌓여 있던 시간, 밀리초)와 단일 `:` 구분자가 붙는다 [1][5][6].
- gtmux는 출력 데이터를 그대로 xterm.js 같은 클라이언트에 흘려보내야 하므로, 8진수 디코딩 후의 바이트열을 **WebSocket binary frame**으로 전달하는 것이 가장 깔끔하다. ANSI 이스케이프 시퀀스는 디코딩 후 그대로 보존된다 [1].

### 5. 흐름 제어와 백프레셔

- tmux 3.2에서 `pause-after`가 도입되었다 [3][7]. `refresh-client -f 'pause-after=<seconds>'`로 설정하면, 어떤 페인의 출력이 tmux 내부 버퍼에 그 시간 이상 머무를 때 tmux가 자동으로 그 페인의 `%output`을 끊고 `%pause %<id>`를 보낸다 [1][3][6].
- 클라이언트는 처리할 여력이 생기면 `refresh-client -A '%<id>:continue'`를 호출하여 재개시키고, 그 응답으로 `%continue %<id>`가 온다 [1][6].
- 추가로 클라이언트는 수동으로 `refresh-client -A '%<id>:pause'` 또는 `'%<id>:off'`를 호출해 페인 출력을 제어할 수 있다 [1][6].
- 내부 버퍼 워터마크는 **CONTROL_BUFFER_LOW = 512B, CONTROL_BUFFER_HIGH = 8192B** 이며, `pause-after`가 비활성인 상태에서 클라이언트가 **300초 이상 뒤처지면** tmux가 클라이언트를 강제 disconnect 한다 [6]. 즉 백프레셔를 *반드시* 구현하지 않으면 부하 상황에서 연결이 끊긴다.
- 보조 플래그(`refresh-client -f`):
  - `no-output`: 이 클라이언트에 `%output`을 일절 보내지 않음(원격 control용) [1][7].
  - `wait-exit`: `%exit` 후 빈 줄이 들어올 때까지 실제 종료를 지연 [1][7].
  - `pause-after=<sec>`: 위 자동 일시정지 임계값 [1][7].
- `refresh-client -C <w>x<h>`로 control 클라이언트의 가상 크기를 설정하면 tmux가 이 클라이언트를 "다른 클라이언트와 동일한 사이즈 협상 참여자"로 취급한다. **호출하지 않으면 다른 클라이언트의 크기에 영향을 주지 않는다** — 멀티 클라이언트 환경에서 매우 중요 [1][7].

### 6. 포맷 구독(`refresh-client -B`)과 `%subscription-changed` (tmux 3.2+)

- 3.2에서 "control mode 클라이언트가 포맷을 구독하고 값 변화 시 알림을 받는" 메커니즘이 추가되었다 [3]. 이전에는 `#{pane_current_path}`, `#{T:status-left}`, `#{pane_title}` 등을 알기 위해 폴링이 필요했다(이슈 #2242의 동기) [9].
- 등록 형식: `refresh-client -B <name>:<what>:<format>`. `<what>` 종류:
  - 빈 문자열 → attached session 대상
  - `%<n>` → 특정 페인 / `%*` → 세션의 모든 페인
  - `@<n>` → 특정 윈도 / `@*` → 세션의 모든 윈도 [1][6].
- tmux는 약 1초 주기로 평가 후 값이 바뀌면 `%subscription-changed <name> $<sid> @<wid> <idx> %<pid> : <value>`를 보낸다. 적용되지 않는 키 필드는 `-`로 채워진다 [1][6].
- 해제: 같은 `-B` 옵션에 이름만 주면 제거 [1].
- gtmux는 페인 제목, current path, 활성 페인, status-left 같은 동적 값에 대해 구독을 사용해 폴링을 없앨 수 있다.

### 7. control mode로 *오지 않는* 정보 — 폴링이 여전히 필요한 영역

- **초기 상태 스냅샷**: 연결 직후 존재하는 세션/윈도/페인 트리는 control mode 알림으로 흘러오지 않는다. 반드시 `list-sessions -F`, `list-windows -a -F`, `list-panes -a -F`를 한 번 실행해 초기 모델을 구성해야 한다 [1][6].
- **터미널 콘텐츠 스크롤백 히스토리**: `%output`은 *연결 이후*의 출력만 보낸다. 재접속 시 기존 스크린 내용을 보려면 `capture-pane -p -e -J -S -<lines>`를 폴링해야 한다 [1].
- **임의 포맷 변수의 현재 값**: 구독을 걸기 전까지의 현재 값은 `display-message -p -F '#{...}'`로 직접 조회해야 한다 [1].
- **레이아웃의 의미적 해석**: `%layout-change`는 tmux 분할 트리를 직렬화한 문자열(예: `b25d,80x24,0,0{40x24,0,0,1,39x24,41,0,2}`)을 보낸다. 캔버스 자유 배치와는 다른 개념이므로(불변식 #3), gtmux는 이 문자열을 변경 감지 트리거로만 사용하고 **렌더링에는 사용하지 않는다**.
- **클라이언트별 키 입력 라우팅**: control mode 클라이언트는 키 입력을 직접 PTY로 보낼 수 없다 — 키 전송은 `send-keys -t %<pid>` 명령으로 우회해야 한다 [1].

### 8. 버전 호환성 (2.x → 3.6)

소스의 `CHANGES` 파일에서 확인한 시점 [3]:

- **1.8** — control mode 자체 도입.
- **2.0** — `pane-mode-changed`, `window-pane-changed`, `client-session-changed`, `session-window-changed` 알림 추가.
- **2.3** — control 클라이언트는 `refresh-client -C` 없이는 세션 크기에 영향 주지 않도록 변경.
- **2.4** — `display-message`가 control 클라이언트에서도 동작; `%config-error` 도입.
- **3.1** — `%config-error` 응답 강화.
- **3.2** — **(중요)** `client-detached` 알림, `focused` 클라이언트 플래그, **포맷 구독(`refresh-client -B`/`%subscription-changed`)**, **`pause-after` + `%pause`/`%continue` + `refresh-client -A/-f`** 도입.
- **3.3** — `%config-error` reply 정식화; `window-resized`, `client-active` 훅.
- **3.5** — `refresh-client -r`로 control 클라이언트가 OSC 10/11 응답을 tmux로 되돌릴 수 있게 됨; `paste-buffer-deleted` 추가 및 `paste-buffer-changed` 이름 수정 [3][8].
- **3.6** — OSC 4 팔레트 요청을 최근 클라이언트로 라우팅.

**최소 권장 버전**: 백프레셔와 구독 없이는 gtmux의 미러링 모델이 부하 상황에서 깨진다 → **tmux 3.2 미만은 비지원**. 안정성·OSC 처리·버퍼 알림 이름 수정까지 고려해 **3.4 이상 강제 권장**. 정책상 "현 LTS-ish는 3.5"를 디폴트로 잡고 3.2~3.3은 degraded mode로 명시.

### 9. iTerm2 — 사실상의 레퍼런스 구현

- iTerm2는 `tmux -CC` / `tmux -CC attach`로 호스트 터미널에서 직접 진입하며, DCS 래핑(`\033P1000p` … `%exit` … `ST`)을 사용해 텍스트 모드와 control 모드를 구분한다 [2].
- iTerm2가 의존하는 알림 집합은 위 표의 거의 전부이며, 명령 응답은 `%begin`/`%end`로 감싼 단일 큐로 매칭한다(`TmuxGateway` / `TmuxController`의 디스패치 패턴) [4][6].
- iTerm2 문서는 다음을 *명시적 한계*로 둔다: "tmux 윈도가 들어있는 탭에는 non-tmux split pane을 둘 수 없다", 그리고 tmux는 모든 클라이언트에 동일 크기를 강제하므로 시각적 정렬 문제가 생길 수 있다 [2]. → gtmux는 *모든* 패널이 tmux 페인을 미러링하거나 또는 *완전히 분리된 웹 전용 패널*임을 보장해 동일 문제를 회피해야 한다.
- 재접속: tmux 서버가 살아 있는 한 상태가 보존되므로 클라이언트는 단순히 `tmux -CC attach`를 다시 실행하면 된다 [2]. gtmux의 "어떤 페인이 죽지 않고 그대로" 시나리오는 tmux 서버 생존성에만 의존한다.

### 10. 다른 구현 현황

- **tmate**: tmux의 포크지만 control mode는 그대로 상속한다. 단, 외부 동기화는 자체 msgpack 프로토콜을 사용하므로 control mode와는 직교 [10].
- **gotty / ttyd / wetty**: 모두 PTY 위 xterm.js 릴레이로, **control mode를 사용하지 않는다**. 그래서 세션 트리 인지·다중 패널 미러링 같은 gtmux 요구를 충족할 수 없다 — 즉 gtmux를 이런 도구로 대체하면 안 된다는 *부정적 발견*[14].
- **Windows Terminal #5612 / Ghostty #1935**: 모두 미구현 feature request 상태이며, control mode 도입의 어려움(레이아웃 불일치, 사이즈 협상)을 인정하고 있다 — gtmux의 설계 결정은 이들과 비슷한 고민을 공유한다 [11][13].

### 11. 알려진 함정

- **SIGHUP 누수**: `tmux -CC attach` 클라이언트는 컨트롤링 터미널이 죽어도 자동으로 정리되지 않을 수 있다(tmux 이슈 #3084). gtmux는 stdin EOF / 부모 PID 모니터링으로 명시적 cleanup을 해야 한다 [12].
- **빈 줄 = detach**: 명령 라인에 빈 줄을 보내면 tmux가 클라이언트를 detach한다 [1]. 절대 빈 줄을 우발적으로 송신하지 말 것(특히 사용자 입력을 명령 채널로 잘못 보내는 버그 회피).
- **줄 끝 처리**: 명령은 `\n` 종결 텍스트 라인이다. CRLF 환경에서 `\r`을 함께 보내면 일부 명령이 오해된다 — WebSocket 게이트웨이 단에서 정규화 필수 [1].
- **`tmux kill-server`**: 모든 클라이언트가 갑자기 `%exit`로 종료한다. gtmux는 이 경우 자동 재시도 없이 사용자에게 명시적으로 알려야 한다(서버가 사라졌기 때문).
- **중첩 tmux**: 호스트 tmux 안에서 `tmux -C`를 띄우면 alt-screen / passthrough 문제로 control 라인이 깨질 수 있다. gtmux 백엔드는 tmux 서버에 *직접* 붙어야 하며, 사용자가 띄운 tmux 안에서 띄우면 안 된다.
- **사이즈 협상**: `refresh-client -C`를 호출하지 않은 control 클라이언트는 윈도 크기에 영향을 주지 않는다(2.3+) [1][3]. 멀티 사용자/멀티 뷰를 도입하면 명시적으로 설정해야 한다.
- **`-C` vs `-CC`**: gtmux 백엔드는 항상 `-C`만 사용. `-CC`는 DCS 래핑이 추가되며 *터미널 에뮬레이터 사용자* 용이다 — 백엔드가 stdin/stdout을 직접 잡는 경우 오히려 방해된다 [1][2].

## 옵션 비교표

| 차원 | 옵션 A | 옵션 B | 비고 / 권장 |
|---|---|---|---|
| 전송 | tmux control mode (`tmux -C`) | tmux 쉘 호출 반복(`tmux list-... ; capture-pane`) | A. 폴링은 알림 누락·레이턴시·CPU 다 안 좋다. 불변식 #5 강제 [1][14] |
| 페인 출력 | `%output` 라인 디코딩 | `capture-pane -p` 폴링 | A. B는 라이브 스트림이 아니라 스크린샷이라 터미널 동작 자체가 안 됨 [1] |
| 흐름 제어 | `pause-after` + `%pause`/`%continue` (3.2+) | 무제한 버퍼링 | A. B는 tmux가 300s 임계로 강제 disconnect [6][7] |
| 동적 변수 추적 | `refresh-client -B` 구독 (3.2+) | `display-message -p` 주기 폴링 | A. 변화 시점만 푸시되어 네트워크·CPU 절감, iTerm2도 이슈 #2242에서 이 동기로 도입 [3][9] |
| 초기 상태 | `list-sessions/list-windows/list-panes -F` 1회 | 알림만으로 조립 | 혼합. 첫 스냅샷은 폴링, 이후는 푸시 — 알림은 *변화*만 보내기 때문에 [1][6] |
| 클라이언트 모드 | `-C` (백엔드 데몬용) | `-CC` (터미널 에뮬레이터 사용자용, DCS 래핑) | A. gtmux 백엔드는 항상 `-C` [1][2] |
| 입력 전달 | `send-keys -t %<pid>` 명령 라우팅 | (없음) | 유일안. control mode는 PTY 직접 쓰기 불가 [1] |
| 사이즈 협상 | `refresh-client -C <w>x<h>`로 명시 참여 | 미설정(다른 클라이언트 크기에 영향 X) | 멀티 뷰가 생길 때만 A; MVP에서는 B 유지로 안전 [1][3] |

## gtmux에의 함의 (불변식 검증 포함)

**불변식 #5 (control mode 강제) — PASS, 단 버전 강제 조건 있음.**
control mode는 모든 상태 변화에 대한 알림과 명령 응답 매칭을 공식적으로 보장한다. 쉘아웃 반복이나 스크린 스크레이핑이 필요한 케이스는 위 §7에 명시한 *초기 스냅샷*과 *스크롤백 캡처*뿐이며, 이들은 같은 control 채널을 통해 `%begin`/`%end`로 감싸진 `list-*` / `capture-pane` 명령으로 받을 수 있어 채널이 하나로 유지된다 [1][6]. 즉 "tmux 통합은 control mode를 사용" 불변식과 정합한다.
권장: **최소 tmux 3.2 강제(없으면 백프레셔·구독 부재로 모델이 깨짐), 기본 권장 3.4+, CI/문서 기준선 3.5.** 버전 미달 시 startup에서 명시적 에러로 거부.

**불변식 #1 (두 상태 도메인 분리) — PASS.**
control mode가 보내는 모든 `%` 알림은 *tmux state*이다 — 세션/윈도/페인 ID, 페인 출력, 레이아웃 문자열, 활성 페인 등. gtmux는 이를 한 방향(tmux → web)으로만 미러링하고, 패널 위치·z-index·라벨·노트 같은 *web state*는 control mode에 절대 보내지 않는다. 특히 `%layout-change`의 layout 문자열은 변경 감지 트리거로만 쓰고 캔버스 배치로 *해석하지 않는다*(불변식 #3과 일치). 라벨/노트도 web 전용으로, `select-pane -T <title>` 같은 tmux 명령을 라벨 동기화에 쓰는 유혹은 금지(라벨은 tmux 도메인이 아님).

**불변식 #2 (tmux-native vs web-only 기능 분할) — PASS, 라우팅 규칙 명확.**
- **tmux 명령으로 라우팅:** `new-session`, `kill-session`, `new-window`, `kill-window`, `split-window`, `select-window`, `select-pane`, `rename-window`, `rename-session`, `send-keys`, `resize-pane`, `capture-pane`, `list-*`, `display-message`, `refresh-client -B/-A/-C/-f` — 모두 **allowlist + 분리된 argv**로만 호출하고 사용자 입력을 절대 셸 보간하지 않음(불변식 #4).
- **웹 상태로만 다룸:** 패널 위치/크기/숨김/최소화/최대화/잠금/z-index/라벨/노트/포커스/뷰포트/저장된 레이아웃.
- 양쪽이 겹쳐 보이는 *select* 작업(활성 페인 포커스)은 양쪽에 다 영향을 준다 — 정책: gtmux UI에서 패널 클릭 시 *옵션*으로 `select-pane`을 호출(설정 가능), 기본은 web focus만.

**불변식 #4 (보안 기본값) — 부분 영향.**
control mode 자체는 stdin/stdout 위 텍스트라 추가 공격면을 만들지 않지만, gtmux가 *그 위에 WebSocket 게이트웨이*를 둔다는 점에서:
- WebSocket → control mode 사이의 명령 라우터는 **고정 allowlist**(위 목록)만 통과시키고, 사용자 문자열은 `-F '#{q:…}'` 인용 또는 argv 분리로만 사용.
- `send-keys`로 흐르는 입력은 PTY로 그대로 가므로 셸 인젝션의 책임은 사용자에게 있다(설계상 정상). 다만 라벨/노트는 *tmux로 절대 보내지 않으므로* 인젝션 표면이 아니다.
- `%output` 디코딩 결과는 ANSI 이스케이프를 포함한 바이트열이다. 브라우저에서는 xterm.js 같은 신뢰된 파서로만 렌더링하고 HTML로는 노출하지 않는다.

### 구체 권장(ADR이 결정해야 할 사항의 디폴트 후보)

1. **최소 tmux 버전 = 3.2, 권장 3.4+**, 시작 시 `tmux -V` 체크 후 미달이면 거부.
2. **부트스트랩 순서:** `tmux -C` 연결 → `list-sessions -F`/`list-windows -a -F`/`list-panes -a -F`로 초기 상태 → `refresh-client -f 'pause-after=10'`로 백프레셔 활성 → 필요한 동적 변수에 대해 `refresh-client -B` 구독 등록 → 이후 모든 변화는 `%` 알림으로 수신.
3. **명령 큐:** 단일 송신 FIFO + `command-number` 매칭. iTerm2의 `TmuxGateway`와 동일한 단순 구조면 충분 [4][6].
4. **`-CC` 사용 금지**, 백엔드는 항상 `-C`.
5. **`refresh-client -C` 호출 안 함**(MVP). 멀티 뷰 도입 시 ADR 재방문.
6. **`%output` 처리:** 라인 단위 파싱 → 8진수 디코딩 → 페인별 ring buffer + WebSocket binary frame 송신.
7. **`%pause` 수신 시 정책:** UI에 "느림" 배지 표시, 사용자가 자동 `refresh-client -A '%id:continue'` 또는 수동 따라잡기 선택.

## 미해결 질문 / 후속 ADR 필요 항목

- `pause-after` 임계값 디폴트는? (5초/10초/30초) — 가벼운 워크로드 vs 빌드 로그 폭주 사이 트레이드오프, 실측 필요.
- 페인당 클라이언트 측 ring buffer 크기 정책: 새로 attach 한 브라우저 탭에 *과거* 콘텐츠를 얼마나 줄지 결정 필요(스크롤백은 `capture-pane`으로 별도 처리하는 것이 깔끔).
- `select-pane` 동기화 정책: gtmux 패널 클릭이 항상 `select-pane`을 호출할지, 옵션으로 둘지.
- 멀티 윈도 동시 표시 시 `refresh-client -C` 도입 여부와 사이즈 협상 모델(클라이언트별 가상 크기 vs 글로벌 최대 크기).
- 재접속 정책: tmux 서버 재시작/`kill-server` 후의 복구는 web state(canvas layout)만 복원하고 tmux state는 새 트리를 받아들이는 것이 자연스러움 — 사용자 노출 메시지 디자인 필요.
- `%subscription-changed`로 노출할 포맷의 최종 목록(예: `pane_current_path`, `pane_title`, `pane_current_command`, `pane_dead`, `window_active`, `session_attached`) — UX 요구와 함께 결정.
- 보안: WebSocket 핸드셰이크 토큰의 수명·회전 정책, origin allowlist의 기본값(localhost only), 외부 노출 opt-in 시 TLS 강제 — 별도 R3 시큐리티 트랙에서 마무리.

## 출처 (URL + 접근일자)

[1] tmux Control Mode (공식 위키) — https://github.com/tmux/tmux/wiki/Control-Mode (접근: 2026-05-13)
[2] iTerm2 — tmux Integration — https://iterm2.com/documentation-tmux-integration.html (접근: 2026-05-13)
[3] tmux/CHANGES (master 브랜치) — https://raw.githubusercontent.com/tmux/tmux/master/CHANGES (접근: 2026-05-13)
[4] iTerm2 sources/TmuxGateway.m — https://github.com/gnachman/iTerm2/blob/master/sources/TmuxGateway.m (접근: 2026-05-13)
[5] tmux 소스 control-notify.c — https://raw.githubusercontent.com/tmux/tmux/master/control-notify.c (접근: 2026-05-13)
[6] DeepWiki: tmux Control Mode (소스 분석 미러) — https://deepwiki.com/tmux/tmux/7.1-control-mode (접근: 2026-05-13)
[7] tmux 소스 control.c (버퍼·pause·write callback) — https://raw.githubusercontent.com/tmux/tmux/master/control.c (접근: 2026-05-13)
[8] tmux Issue: paste-buffer-deleted / rename — https://github.com/tmux/tmux/blob/master/CHANGES (접근: 2026-05-13)
[9] tmux Issue #2242 — Feature request: New control mode notifications (구독 메커니즘의 동기) — https://github.com/tmux/tmux/issues/2242 (접근: 2026-05-13)
[10] tmate README / 아키텍처 — https://github.com/tmate-io/tmate (접근: 2026-05-13)
[11] microsoft/terminal Issue #5612 — Implement tmux control mode (-CC) — https://github.com/microsoft/terminal/issues/5612 (접근: 2026-05-13)
[12] tmux Issue #3084 — SIGHUP won't exit tmux client in control mode — https://github.com/tmux/tmux/issues/3084 (접근: 2026-05-13)
[13] ghostty-org/ghostty Issue #1935 — Support for tmux Control Mode — https://github.com/ghostty-org/ghostty/issues/1935 (접근: 2026-05-13)
[14] gotty/ttyd/wetty 아키텍처 비교(부정적 증거: control mode 미사용) — https://github.com/yudai/gotty (접근: 2026-05-13)
