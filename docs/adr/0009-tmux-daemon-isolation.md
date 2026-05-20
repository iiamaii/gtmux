# ADR-0009: tmux daemon 격리 모델 — Dedicated daemon per Server

- 상태: **Deprecated (2026-05-14, superseded by ADR-0014 "Process supervisor")**. ADR-0013 채택으로 tmux backend 자체가 사라지면서 *daemon 분리* 컨셉이 무의미해짐. 본문은 historical context 로 보존. 2026-05-14 의 §D5 amend (graceful prevention) 의 정신은 ADR-0014 §D9 + CONTEXT.md §"tmux invariant 의 UI 측 mirror" 로 계승. 참조 우선순위: ADR-0014 + `docs/reports/0023-pty-poc-verification-and-decision.md`.
- 일자: 2026-05-13 (Proposed) / 2026-05-14 (Accepted → Deprecated 동일 일자)
- 결정자: system-architect (grill 산출)
- 근거 보고서: `docs/reports/0010-grill-amendments.md` (D10, 실측 footprint), `docs/reports/0005-security-model.md` (§E.3 tmux 전용 소켓, §A.2 XDG 디렉터리 권한)
- 관련 ADR: ADR-0007 (Server : Session : Port 1:1:1 바인딩), ADR-0001 (tmux 통합 = 컨트롤 모드, 발행 예정)

## 맥락

`docs/sketch.md` §13.3.6 "tmux socket 노출"은 세 가지를 권고한다 — (i) tmux socket 파일 권한 최소화, (ii) **별도 전용 socket 경로 사용 고려**, (iii) 웹 서버 프로세스 권한 최소화. 한편 ADR-0007이 정한 Server : Session : Port 1:1:1 모델은 *Server마다 격리된 단위로 동작*함을 명시한다. 두 결정의 자연스러운 합집합은 **각 gtmux Server마다 자신 전용 tmux daemon을 띄워 1:1:1:1 (Server : Session : Port : tmux-daemon) 격리를 완성**하는 것이다.

대안 경로 — 단일 공유 daemon, 사용자 main tmux server attach, `-S` 자체 디렉터리 — 의 보안·운영 trade-off는 본 grill에서 정량 평가됐고(`docs/reports/0010-grill-amendments.md` D10), tmux daemon 1개당 메모리 footprint는 실측 데이터로 뒷받침된다(본 ADR §실측 footprint).

본 ADR은 `docs/sketch.md` §10.1 "백엔드 구성"의 *lifecycle manager* 컴포넌트가 책임질 daemon 부팅·종료 절차의 SSoT다. CLI 명세(`gtmux start`/`stop`/`teardown` 등)는 D20에서 정의되며, 본 ADR §D6은 그중 `teardown` 서브명령의 절차를 확정한다.

## 결정 (Decisions)

- **D1.** 각 gtmux Server는 자신 전용 tmux daemon에 attach한다. 격리 단위 = **1:1:1:1 (Server : Session : Port : tmux-daemon)**.
- **D2.** 소켓 컨벤션 = **`tmux -L gtmux-<session>`**.
  - 경로: `${TMUX_TMPDIR:-/tmp}/tmux-${uid}/gtmux-<session>` (tmux 표준).
  - tmux가 부모 디렉터리(0700)·소켓 perm·SIGUSR1 재생성을 *자동 보장*. `-S` 자체 디렉터리 대비 안정성 ↑ (R3 참조).
- **D3.** 부팅 시 daemon 부재 → **자동 spawn** (`tmux -L gtmux-<session> start-server`).
- **D4.** ADR-0007 D3과 분리: *daemon은 자동 spawn하되 Session 자체는 자동 생성하지 않는다*. Session 부재 시 exit 3 (D20 exit code 규약).
- **D5.** gtmux Server 종료 시 **daemon은 살려둔다** — Session·Pane persistence 보존. R1 보고서 §9 "tmux 서버가 살아 있는 한 상태가 보존된다", D21 c5 "Server lifecycle ⊥ tmux daemon lifecycle"과 정렬.
    - **[2026-05-14 amend — graceful prevention, L-17]** *역방향* 경로 — 즉 *tmux daemon 이 먼저 죽을 수 있는 시나리오* — 에 대한 정책을 본 amend 로 명시화한다.
        - **위험 시나리오**: tmux 의 native invariant 는 *Session ≥ 1 Window ≥ 1 Pane*. 마지막 Window 의 마지막 Pane 이 종료되면 Window → Session → tmux Server (daemon) 가 연쇄 자체 종료한다. 사용자가 새 Panel 안의 shell 에 `exit` 또는 `Ctrl-D` 만 입력해도 이 연쇄가 트리거되어 *gtmux Server 의 control-mode pipe 가 broken pipe* 가 된다 (`docs/reports/0020-debug-classification.md` §2.6 / L-17).
        - **결정 — Prevention 모델**: 사후 recovery (LIFE-AUTOSPAWN) 가 아니라 *사전 prevention* 으로 invariant 를 보호. 다음 세 측면을 통합:
            1. **Auto-mount 의무**: tmux 측에 발견되는 모든 Pane (bootstrap snapshot + 내부 New Panel + 외부 CLI `new-window`) 은 Canvas Panel 로 *즉시* auto-mount. `CONTEXT.md` §"Placement principle" 정합. Available 류 사용자-명시 mount 단계는 두지 않는다.
            2. **Close 버튼 비활성화**: Canvas Panel 의 close 버튼은 *현재 tmux Window 수 = 1* 일 때 비활성화. UI 차원 invariant 보호 — 사용자가 UI 경로로는 invariant 를 깨뜨릴 수 없다. tooltip 사유 = "마지막 Window 는 close 할 수 없습니다 — Session shutdown 메뉴를 사용하세요". 외부 CLI 가 추가 Window 를 만들었다면 tmux Window 수 ≥ 2 가 되어 close 가능. visibility=hidden Panel 은 close 카운트에 영향을 주지 않는다 (트리거 기준 = tmux 측 *실제 Window 수*).
            3. **명시적 Session shutdown UI 액션**: 사용자가 *의도적으로* Server 를 종료하려면 Canvas 우상단 헤더 메뉴 → Session shutdown → confirm modal ("Session 'X' 를 종료합니다 — 모든 pane 이 닫히고 gtmux Server 도 종료됩니다") → CTRL `kill-session` 발사 → `%exit` 수신 → ADR-0001 §D12 graceful shutdown (WS close + layout flush) + exit 6. 이 경로가 *유일한* 명시 종료 경로 (`CONTEXT.md` §"Scope boundary" amend: Session 종료 = UI 액션 허용).
        - **CONTEXT.md amend 동반**: `CONTEXT.md` §"Scope boundary" 가 "Session 종료(= Server quit) 는 UI 액션 허용" 으로 정정됨 (이전엔 "모두 UI 밖"). 본 ADR D5 의 prevention 정책이 그 amend 의 implementation 측면.
        - **LIFE-AUTOSPAWN (자동 재기동) 명시 거절**: control-mode pipe broken 감지 시 자동 `TmuxDaemon::spawn` 재실행 + `daemon-restarted` NOTIFY_MIRROR broadcast 안은 grilling 에서 검토되었으나 거절. 정당화 — (a) ADR-0001 §D12 의 "자동 재시도 안 함" 정신 보존, (b) tmux 측에서 사용자 shell `exit` 으로 인한 종료 vs 외부 `kill-session` 으로 인한 종료 가 구별 불가능 → 의도 추정 기반 자동화는 fragile, (c) prevention 으로 시나리오 자체 회피 가능. 자세한 trade-off 는 `docs/reports/0022-logic-amendment-decisions.md` §1.2 참조.
        - **외부 CLI 가 마지막 Window 를 죽인 경우** (UI 가 막을 수 없는 경로): UI prevention 우회 — 결과는 ADR-0001 §D12 의 *외부 session kill* 경로로 자연 흡수 (exit 6).
        - **Sprint 6 task 정합**: 본 amend 가 `S6-LIFE-AUTOSPAWN` task 의 **취소** + 신규 task `S6-FE-SHUTDOWN` (UI 액션) / `S6-FE-CLOSE-GUARD` (close 비활성) / `S6-BE-AUTOMOUNT` (auto-mount loop) 의 정본 정의. 핸드오프 `docs/reports/0021-session-handoff.md` §6 갱신 의무.
        - Result: this clause was added retroactively after the debug session 2026-05-14. See `docs/reports/0020-debug-classification.md` §2.6 + `docs/reports/0022-logic-amendment-decisions.md` §1.
- **D6.** 명시적 정리 명령 = `gtmux teardown --session <name>` (D20 서브명령). **5단계 절차** (D21 c8: config 정리 포함):
  1. gtmux Server 프로세스 종료 — `${XDG_RUNTIME_DIR}/gtmux/<session>.pid`가 살아 있으면 SIGTERM 후 종료 대기 (graceful: WS close + layout flush, D20).
  2. `tmux -L gtmux-<session> kill-server` — daemon 종료.
  3. `rm -f ${TMUX_TMPDIR:-/tmp}/tmux-${uid}/gtmux-<session>` — 소켓 파일 명시 삭제. **실측 확인**: `kill-server`는 소켓 파일을 자동 정리하지 않는다 (보고서 D10).
  4. `rm -f ${XDG_STATE_HOME}/gtmux/<session>.token` + `${XDG_STATE_HOME}/gtmux/<session>.layout.json` + `${XDG_RUNTIME_DIR}/gtmux/<session>.pid` — state 파일 정리.
  5. `rm -f ${XDG_CONFIG_HOME}/gtmux/<session>.config.toml` — config 파일 정리 (port↔session 영속 mapping 제거, D21 c6·c8).

  플래그:
  - `--force` — 사용자 prompt skip (D20 정의).
  - `--keep-config` — 단계 5 skip (재기동 의도 시, D21 c8).

  부분 실패 시 **exit 7** (D20 규약) + 잔여 파일 경로 stderr 출력.

## 거절된 대안 (Rejected)

- **R1. (C-B) 단일 gtmux 공유 daemon** — 모든 gtmux Server가 `tmux -L gtmux` 공유 daemon에 attach. *Failure mode*: 한 Server 침해 시 동일 daemon의 sibling Session 모두 접근 표면 노출 (트러스트 경계 공유). `docs/sketch.md` §13.3.6 "tmux socket 접근 권한 = 제어권" 정신 위배 + 0005 보고서 §E.3 "사용자의 기존 tmux 세션과 격리되어, gtmux의 화이트리스트 실수가 사용자 운영 환경에 침범하는 폭발 반경을 줄인다"의 일반화된 원칙(격리 단위 ≤ Server)에 어긋남. 메모리 절감 ~170 MB의 이득이 격리 손실을 상쇄하지 못함.
- **R2. (C-C) 사용자 main tmux server attach** — default socket(`/tmp/tmux-<uid>/default`). *Failure mode*: gtmux가 사용자의 *모든* tmux session에 가시·잠재 조작 가능. allowlist 우회(예: argument injection — `docs/reports/0005-security-model.md` §C.2) 시 사용자 main 작업 환경 직접 침해. `docs/sketch.md` §13.3.6 위반 + §13.3.1 명령 인젝션 위험을 N배로 확대.
- **R3. `-S` 자체 디렉터리 경로** (예: `~/.gtmux/sockets/<session>.sock`) — *Failure mode*: gtmux가 부모 디렉터리 perm(0700) 보장·SIGUSR1 재생성·표준 discovery 컨벤션을 *직접 구현*해야 한다. tmux 표준 `-L` 컨벤션이 제공하는 검증된 dir-perm 자동 강제(`docs/reports/0005-security-model.md` §E.3)를 잃고 신규 코드 표면 = 신규 결함 가능성. 외부 attach 명령도 더 길어짐. tmux upstream의 향후 보안 패치에서 분기 발생 위험.

## 결과 (Consequences)

- 긍정:
  - **보안 표면 N으로 분할** — 한 Server 침해 = 그 daemon의 1 session 1 port에만 영향. sibling Server·사용자 main tmux 환경 무영향.
  - **모델 일관성** — 1:1:1:1 (Server : Session : Port : tmux-daemon)으로 깔끔. ADR-0007과 직접 정렬.
  - **Persistence 가치 보존** — gtmux Server kill에도 daemon은 살아 있어 Pane 상태 유지 (D21 c5 정렬).
  - **tmux 표준 보호 그대로 활용** — dir-perm 0700 자동 강제 + SIGUSR1 재생성 + upstream 보안 패치 자동 수혜.
  - **§13.3.6 권고 직접 충족** — "별도 전용 socket 경로 사용 고려"를 default 동작으로 잠금.
- 부정/비용:
  - daemon 메모리 baseline ≈ Server × 3.4 MB. 50 Server 시 ≈ 175 MB total (Rust 백엔드 프로세스 메모리의 약 6%, 실측 확인). MVP·stretch 메모리 예산(D19) 안에 수용.
  - 외부 attach 명령 길어짐 (`tmux -L gtmux-<session> a -t <session>` 풀 형태). 본 프로젝트 1차 사용 흐름이 아니어서 수용.
  - cleanup 명시 명령(`gtmux teardown`)이 본 프로젝트 범위에 포함됨 — 소켓·state·config leftover 방지 책임이 사용자가 아닌 gtmux 측에 있음을 명시.
- 후속 작업:
  - `docs/sketch.md` §10.1 "lifecycle manager" 컴포넌트 추가 (D20 dispatch에서 일괄 적용 예정).
  - ADR-0001(tmux 통합)의 부트스트랩 절차는 본 ADR의 daemon attach를 *전제*로 작성한다 — `tmux -L gtmux-<session> -C attach -t <session>` 호출이 진입점.
  - 외부 *활성 Server 목록 도구*(본 프로젝트 비범위)는 `${TMUX_TMPDIR}/tmux-${uid}/gtmux-*` 패턴으로 소켓 enumerate 가능 — 인터페이스가 자연 정의됨.
  - ADR-0011(Rust backend)의 lifecycle manager 모듈이 본 ADR의 spawn/teardown 절차를 구현 (`std::process::Command` argv 분리, shell 비경유).

### 실측 footprint (참고)

| 시나리오 | total RSS |
|---|---|
| 1 daemon, 1 session, 1 pane (baseline) | 3.4 MB |
| 1 daemon, 60 panes (= 60 windows, single-pane-per-window 컨벤션 ADR-0008) | 4.3 MB (window당 ≈ 15 KB) |
| 6 daemons 동시 | 22 MB |
| 50 Server × 5 pane 추정 | ~175 MB |

측정 환경: 2026-05-13, macOS Darwin 25.3, tmux 3.6a. 본 실측은 R1 거절 대안의 메모리 절감 근거(공유 daemon ~170 MB 추정)와 동일 단위에서 비교 가능.

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태/웹 상태 분리 | PASS — tmux daemon이 별개 프로세스로 격리되어 web 측이 침범할 표면이 기계적으로 작음. 전용 daemon은 분리의 *physical 경계*를 강화. |
| 2 | tmux-native vs web-only 분기 | PASS — 본 결정은 순수 tmux-native 운영 절차. web-only 상태(panel geometry 등)와 무관. |
| 3 | tmux Layout ≠ Canvas Layout | PASS (trivially) — 본 결정은 daemon 격리이며 layout 차원과 무관. ADR-0008(single-pane-per-window)이 별도로 보장. |
| 4 | 보안 기본값 | **PASS (강함)** — (a) `docs/sketch.md` §13.3.6 "별도 전용 socket 경로" 권고를 default로 잠금, (b) tmux 표준 `-L` 컨벤션이 부모 디렉터리 0700·소켓 perm·SIGUSR1 재생성을 자동 강제(보고서 0005 §E.3), (c) 격리 단위가 Server N으로 분할되어 한 Server 침해의 폭발 반경 ≤ 1 session, (d) 거절 대안 R1/R2가 모두 §13.3.6 정신을 약화시키는 반면 채택안은 직접 충족. |
| 5 | control mode 사용 | PASS — daemon 부팅 직후 gtmux Server는 `tmux -L gtmux-<session> -C attach -t <session>`로 control mode 진입. 격리된 소켓 컨벤션은 control mode 채널의 단일성을 해치지 않음 (오히려 채널 분리). |

## 미해결 항목 (Open)

- **O1. `TMUX` 환경변수 nested attach 처리** — gtmux Server가 *이미 다른 tmux 안에서* 실행 중일 때 (`TMUX` env 설정) 신규 daemon에 attach 시 tmux가 "sessions should be nested with care" 경고 또는 거부 가능. **R7 구현 검증 항목**: (i) Rust `std::process::Command`로 spawn 직전 `TMUX` env 명시 제거 vs `tmux -2 -L gtmux-<session>` 옵션 비교, (ii) 양쪽 모두에서 control mode 진입이 정상 동작하는지 macOS·Linux에서 각각 검증, (iii) 결과를 ADR-0001 또는 ADR-0011 부속 검증 절에 기록.
- **O2. macOS `${TMUX_TMPDIR}` 기본값과 systemd-tmpfiles 차이** — Linux에서 systemd-tmpfiles가 `/tmp` 정기 청소(`Age=10d` 등) 정책을 가진 경우 long-idle daemon의 소켓 inode가 청소 대상이 될 위험 vs macOS는 해당 정책 없음. **R7 구현 검증 항목**: (i) `${TMUX_TMPDIR}`가 unset일 때 Linux의 `/tmp/tmux-${uid}/`가 systemd-tmpfiles 기본 룰의 예외 디렉터리인지 확인, (ii) 청소 대상이면 `${XDG_RUNTIME_DIR}/gtmux/sockets/`로 명시 redirect 권장 (env export 또는 `--tmux-tmpdir` 플래그), (iii) macOS는 별도 조치 불필요 확인, (iv) 결과를 OS별 default 동작 표로 ADR-0011 또는 sketch §10.1에 기록.
