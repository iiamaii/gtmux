---
name: gtmux-cli
description: >-
  Control the gtmux canvas layout and terminals from inside a gtmux pane via the
  `gtmux` CLI. Use when running inside a gtmux terminal, or when the user mentions
  gtmux, canvas panels/panes, layout, or driving another terminal panel. Covers
  layout move/resize/create/delete, figure & note editing, group, align, path
  connections, terminal spawn/mount/read/send, workspace & session, fs upload.
---

# gtmux CLI

gtmux 터미널 안의 AI agent 가 `gtmux` CLI 로 canvas layout 과 terminal 을 제어한다. CLI 는 live 서버의 HTTP API 를 경유하며 layout(web state)을 제어하고(ADR-0053), 다른 terminal 의 내용을 읽고 command 를 보낼 수 있다(ADR-0054). 사용자는 브라우저 canvas 에서 이 조작을 실시간으로 본다.

## 0. 전제

- `gtmux` binary 가 PATH 에 있어야 한다.
- 서버가 떠 있어야 한다. 인스턴스가 여럿이면 `GTMUX_SERVER_INSTANCE` env 또는 `--instance`.
- 인증은 자동(로컬 token 파일 → Bearer). 별도 설정 불요.
- 이 문서 자체를 CLI 로 다시 읽으려면 `gtmux skill`(전체) / `gtmux skill --section <n>`.

## 1. 자기 식별 (어느 panel/session 이 "나"인가)

gtmux 가 spawn 한 터미널에 주입되는 env:

| env | 의미 |
|---|---|
| `GTMUX_TERMINAL_ID` | 이 터미널의 UUID = **canvas 의 내 terminal item id**. `gtmux layout move $GTMUX_TERMINAL_ID …` 처럼 바로 사용 |
| `GTMUX_CANVAS_SESSION` | 내가 속한 canvas session 이름. `--session` 생략 시 기본값 |
| `GTMUX_SERVER_INSTANCE` | 서버 인스턴스명 (canvas session 아님 — 혼동 금지) |

env 가 없으면 `gtmux layout list` 로 현황을 보고 label 로 추정하되, 자기 자신에 대한 파괴적 조작은 피한다.

## 2. 명령 표면

조회 (`--json` 지원):
```
gtmux layout list                      # 전체 item 요약: id/type/label/geometry/visibility/locked/z
gtmux layout get <target>              # 단건 상세 (--json 시 payload 전체)
gtmux layout connections <target>      # path 로 연결된 상대 item 목록
gtmux terminal ls                      # 서버 전체 terminal pool (attach 현황 포함)
gtmux workspace get --session <name>
```

layout mutation (`<target>` = UUID 또는 label 정확일치):
```
gtmux layout move <target> --x <f> --y <f>
gtmux layout resize <target> --w <f> --h <f>
gtmux layout show|hide|minimize|restore <target>   # minimize/restore: terminal/note/document/snippets/web_view only
gtmux layout label <target> <text>|--clear
gtmux layout raise|raise-top|lower|lower-bottom <target>
gtmux layout edit <target> --set k=v ... | --json '<partial payload>'
gtmux layout create <type> [--x --y --w --h] [--set ...|--json ...]   # text/note/rect/ellipse/line/free_draw/image/document/web_view/file_path/path/snippets
gtmux layout delete <target> [--kill-terminal] [--force]
gtmux layout group create <t1> <t2> ... [--label <s>] | ungroup <g> | reparent <t> --parent <g|root>
gtmux layout align <mode> <t1> <t2> ...    # left/right/top/bottom/center-h/center-v/distribute-h/distribute-v
gtmux layout batch --json '<ops[]>'        # 원자적 일괄 적용
```

terminal / workspace / session / fs:
```
gtmux terminal spawn [--x --y --w --h]     # 새 terminal panel (headless 완결, 기본 shell)
gtmux terminal mount <uuid> [--x --y]      # pool 의 기존 terminal 을 canvas 에 다시 올림
gtmux terminal unmount <target>            # panel 만 제거, terminal 은 pool 잔류
gtmux terminal kill <target>               # process 종료
gtmux terminal read <target> [--tail N] [--raw]   # 터미널 출력 읽기 (기본 ANSI strip 텍스트, --raw 는 원본 바이트)
gtmux terminal send <target> <text> [--no-enter]  # 입력 주입 (기본 뒤에 \n → 명령 실행; --no-enter 는 개행 없이)
gtmux terminal send <target> --bytes <hex>        # 제어 시퀀스 (03=Ctrl-C, 1b5b41=Up)
gtmux workspace set <path> --session <name>
gtmux session create <name> [--workspace <p>] [--yes|--password <p>]  # 추가 인증 게이트
gtmux session delete <name> [--yes|--password <p>]
gtmux fs upload <src-path> --dir <ws-relative> --session <name>       # workspace 로 파일 반입
```

## 3. 규칙 (위반 시 서버가 거부)

- **locked item 은 기본 거부**(409) — 사용자가 잠근 것. `--force` 는 사용자가 명시 지시했을 때만.
- z 는 4액션만(임의 z 값 지정 불가). `maximized`/viewport/selection 은 CLI 로 제어 불가.
- terminal item 은 `create` 불가 — `terminal spawn`/`mount` 사용.
- image/document 의 `path` 는 workspace 상대경로 + 실존 파일이어야 함. 로컬 파일은 `fs upload` 로 먼저 반입 후 참조 (2-step).
- **web_view** 는 주소를 렌더하는 읽기 전용 라이브 뷰. `url` 은 두 형태만 허용: **`http(s)://` 절대 URL** 또는 **workspace 상대 경로**(html/md/image 를 노드 안에서 렌더). `javascript:`/`file:`/`data:`/절대 로컬 경로·`..` 등 기타 형태는 생성·수정 모두 거부(`web_view_url_invalid`). 앱 자신의 origin URL 도 거부(`web_view_own_origin`) — 단 `http://localhost:<다른 포트>` 같은 loopback 타 포트(로컬 dev 서버 미리보기)는 허용. `url` 4KiB 상한. `edit --set url=…` 도 create 와 동일 검증. **CLI 는 스킴 명시 필수** — `localhost:5173` 같은 스킴 생략형은 거부되므로 `http://localhost:5173` 으로 쓸 것(스킴 자동 보정은 브라우저 change 모달 전용).
- session-level 명령(workspace/session)은 session 명시 필수. session create/delete 는 인증 게이트(password 설정 시 `--password`, 미설정 local 은 `--yes`).
- 에러는 exit code ≠ 0 + stderr 한 줄(machine-readable code). label 이 모호하면(0건/복수) 후보가 나열된다 — UUID 로 재시도.

## 4. 패턴

- **상태판(status board)**: `create note --set title="작업 상태"` 후 진행 상황을 `edit` 로 갱신 — 사용자가 canvas 에서 실시간 모니터링. 완료/실패는 label·색으로도 표시.
- **구성 작업은 batch**: 여러 panel 을 배치·정렬할 때는 개별 명령 반복 대신 `align` 또는 `batch --json` (원자 적용, 화면 깜빡임 최소).
- **연결 표현**: 관련 component 는 `create path --json '{"from":{"kind":"connected","item_id":"...","anchor":"E",...},...}'` 로 연결하고, `connections` 로 그래프를 역조회.
- **주소 렌더(web_view)**: 원격 페이지·대시보드나 workspace 문서를 캔버스에 라이브로 띄운다.
  ```
  gtmux layout create web_view --set url=https://example.com      # 원격 URL
  gtmux layout create web_view --set url=docs/report.md           # workspace 파일 (html/md/image)
  gtmux layout edit <target> --set url=https://other.example.com  # 주소 변경
  ```
  규칙(스킴 제한·own-origin 거부·loopback 허용·4KiB)은 §3. 스킴 위반 등은 exit≠0 + stderr code(`web_view_url_invalid`/`web_view_own_origin`).
- **worker 터미널**: `terminal spawn` 으로 하위 터미널을 만들고, `terminal read`/`terminal send` 로 출력을 읽고 명령을 주입. 흐름: 다른 패널을 `terminal read <other>` 로 관찰 → 구상한 작업을 `terminal send <other> "ls"` 로 실행. 단 `read` 는 raw ring(128KiB drop-oldest) 스냅샷이라 **긴 출력의 앞부분은 유실**되고 ANSI 가 섞인다(기본 출력은 CLI-side ANSI strip). claude/codex 대화형 TUI·vim 같은 **full-screen 재그리기 화면은 raw 가 재그리기 스트림이라 거의 무의미** — 완전 캡처·명령 완료 신호가 필요하면 headless(`claude -p --output-format stream-json`, `codex exec --json`)를 그 터미널에서 돌려 NDJSON 을 직접 파싱하라.

## 5. 하지 말 것

- layout JSON 파일 직접 편집(서버 lock/ETag/broadcast 우회 — 항상 CLI/HTTP 경유).
- 사용자가 드래그 중인 item 을 연속 mutate(충돌 시 last-writer-wins 로 서로 덮어씀).
- 자기 자신(`$GTMUX_TERMINAL_ID`)의 `delete --kill-terminal` / `terminal kill` — 자기 프로세스가 죽는다.
- 자기 자신(`$GTMUX_TERMINAL_ID`)에 `terminal send` — 자기 입력 스트림 오염(self-injection). send 대상은 항상 다른 terminal.
- `terminal read` 폴링만으로 하위 작업 "완료" 판정 — raw ring 은 완료 신호가 없고 손실성이라 상태 추측이 어긋난다. 완료 판정이 필요하면 §4 의 headless NDJSON 경로.
