# ADR-0007: Server : Session : Port 1:1:1 바인딩 모델

- 상태: **Superseded by ADR-0019** (2026-05-15 multi-session pivot). 본 ADR 의 1:1:1 (Server : Session : Port) 모델은 single-session 시대 정합이며, ADR-0019 의 1:N (Server : Workspace : Session) 모델로 대체됨. (이전 상태: Accepted 2026-05-14, A0.7 + A4 게이트 통과. **2026-05-14 amend** — ADR-0013 채택 후 "tmux Session" 어휘를 "logical Session" 으로 정정. `docs/reports/0023-pty-poc-verification-and-decision.md` §7.2 O5.)
- 일자: 2026-05-13 (Proposed) / 2026-05-14 (Accepted, amend 동일)
- 결정자: system-architect (grill 산출)
- 근거 보고서: `docs/reports/0010-grill-amendments.md` (D1, D2, D3)
- 관련 ADR: ADR-0008 (single-pane + Group), ADR-0009 (tmux daemon 격리), ADR-0010 (Group 데이터 모델), ADR-0003 (보안 디폴트, 후속)

## 맥락

`docs/sketch.md` §1.3은 gtmux를 **단일 사용자용 로컬/개인 서버 웹 앱**으로 정의하고, 멀티테넌시·계정·팀 권한·조직 협업을 명시적으로 비범위로 둔다. §5.2의 사용자 시나리오 4종(A 한 session의 여러 pane을 한 캔버스로, B 장기 pane 모니터링, C GUI+CLI 병행, D 세션 복원)은 모두 *단일 Session 내부*의 작업 흐름이다. 한편 §6.1은 session 제어 6기능(생성/조회/선택/종료/이름변경/attach)을 잠정 UI 범위에 두고 있는데, grill 세션(보고서 §1 D1·D2·D3)에서 사용자는 단순성 우선으로 *session 제어를 UI 밖*으로 빼고 **1 gtmux Server = 1 tmux Session = 1 포트** 모델을 채택했다. 여러 Session을 운영하려면 여러 Server를 다른 포트로 띄운다.

이 결정은 URL 라우팅·인증 토큰 스코프·Canvas Layout 영속화 키·UI 범위(§6.1)에 광범위하게 영향을 미친다. 또한 후속 ADR-0009(tmux daemon 격리, 1:1:1:1로 확장)와 ADR-0010(Group 데이터 모델, Canvas:Session 1:1 전제)이 본 결정을 입력 제약으로 받기 때문에, 배치 A0 안에서 *가장 먼저* 굳혀야 한다.

## 결정 (Decisions)

- **D1.** 한 gtmux Server 프로세스는 정확히 한 **logical Session** (사용자 부여 식별자) 에 바인딩되고, 단일 포트를 점유한다 (Server : Session : Port = 1 : 1 : 1). **[2026-05-14 amend]** *Session* 은 더 이상 *tmux session* 을 가리키지 않는다 — ADR-0013 채택으로 tmux 가 사라지면서 *Session* = *사용자가 부여하는 logical 식별자* (CLI `--session <name>` 인자, 상태 파일 `<session>.token` / `<session>.layout.json` / `<session>.config.toml` 의 키, 토큰 스코프) 로 의미 단순화. 어휘 사용은 그대로.
- **D2.** 바인딩은 Server **부팅 시 CLI 인자**(`--session <name> --port <port>`)로 결정된다. **런타임 중 변경 불가**(immutable) — 재바인딩 API·UI를 두지 않는다.
- **D3.** **[2026-05-14 amend]** 부팅 시점에 *해당 Session 의 상태 파일* (`${XDG_STATE_HOME}/gtmux/<session>.lock` 등) 이 *다른 활성 Server* 에 의해 점유 중이면 exit 4 (port 중복 또는 session 충돌). *Session 의 부재* 는 더 이상 에러가 아님 — 새 logical session 시작 OK (ADR-0014 D4 정합). 첫 Pane spawn 은 사용자 명시 액션 (frontend bootstrap 의 첫 [New Panel] 자동 호출 포함, ADR-0014 D3) 으로 미뤄짐.
- **D4.** **[2026-05-14 amend]** UI 의 Session shutdown 액션 또는 외부 사유로 Server 프로세스가 종료될 때 모든 child process 가 자손으로서 함께 정리됨 (ADR-0014 D5 + D7). 런타임 중 다른 Session 으로의 재바인딩 경로는 없다 (exit 6).
- **D5.** **[2026-05-14 amend]** 사용자는 여러 Server를 서로 다른 포트로 **동시 실행 가능**하다. 활성 Server 목록·오케스트레이션·디스커버리 도구는 본 프로젝트 비범위이며, 별도 프로젝트가 *Server 식별자 디렉터리* (`${XDG_RUNTIME_DIR}/gtmux/*.pid`, ADR-0014 D8 컨벤션) 으로 enumerate한다. (구) tmux 소켓 디렉터리 패턴은 폐기.

## 거절된 대안 (Rejected)

- **R1. 한 Server가 다중 Session을 UI에서 다중화 (Canvas:Session = 1:N)** — 보고서 §1 D1·D2 grill에서 사용자가 *모델 정직성* 기준으로 거부했다. Canvas:Session이 1:N이 되면 §5.2.D "session과 layout 페어링" 어휘가 깨지고, Canvas Layout 영속화 키(`<session>.layout.json`)가 복합 키로 비대해지며, 인증 토큰 스코프가 "Server 단위"인지 "Session 단위"인지 모호해진다. ADR-0010(Group)이 가정하는 *한 캔버스 = 한 Session 도메인*도 무너진다.
- **R2. 런타임 중 바인딩 변경 UI** — 보고서 §1 D3에서 거절. gtmux UI 범위(Pane 제어 + Panel 제어 + Group 관리, sketch §6.4 재정의)에 *session 제어*가 새는 것을 막기 위함이다. 재바인딩을 허용하면 §6.1의 6기능이 사실상 부활하고, 토큰·layout 영속화 키가 런타임에 가변이 되어 §13.3.2 WebSocket 인증 모델의 단일 토큰 전제와 충돌한다.
- **R3. Session 부재 시 자동 생성** — 보고서 §1 D2에서 *결정성 우선*으로 거절. 사용자가 `--session` 인자를 typo하면 빈 Session이 양산되어 ADR-0009 daemon footprint(per-Server 3.4 MB baseline)가 사용자 의도 없이 증식한다. 부재 = 에러 종료가 사용자에게 *오타 즉시 피드백*을 준다.
- **R4. 활성 Server 목록·오케스트레이션을 본 프로젝트 안에 포함** — sketch.md §1.3 비범위(단일 사용자)와 §11.3 MVP 제외 정신에 위배된다. 본 프로젝트는 한 Server의 품질에 집중하고, 다중 Server enumerate는 외부 도구가 소켓 디렉터리 패턴(ADR-0009 D2)으로 처리한다.

## 결과 (Consequences)

- 긍정:
  - URL·포트·인증 토큰·Canvas Layout 영속화 키 모두 단순 1:1로 정렬된다(`http://<bind>:<port>/?token=...` → 그 Server 단일 Session).
  - UI scope가 *Pane 제어 + Panel 제어 + Group 관리* 세 축으로 축소되어 §6.1 6기능과 §6.2 6기능이 일괄 폐기 대상이 된다(sketch.md 본문 수정 동반).
  - 한 Server가 침해되어도 영향은 그 Session 1개로 한정(ADR-0009 daemon 격리와 시너지).
  - Server 재기동에도 tmux daemon은 살아남아 Session·Pane persistence가 유지된다(ADR-0009 D5 참조). 사용자는 `gtmux start --port <N>` 한 줄로 동일 Session에 재attach(ADR-0009 의존 D21 c6 port-based lookup).
- 부정/비용:
  - 멀티-Session 운영을 원하는 사용자는 외부 launcher 또는 수동 다중 실행을 직접 관리해야 한다. *활성 Server 목록 도구*의 잠재 필요성이 발생하지만 본 프로젝트 비범위로 명시.
  - sketch.md §6.1 session 제어 6기능과 §6.2 window 제어 6기능이 UI에서 완전히 제거되므로 spec 본문 수정 필요(`docs/reports/0010-grill-amendments.md` §2 amendment list로 추적, 동반 PR에서 적용).
  - 사용자가 동일 port를 두 Server에 부여하려고 시도하면 exit 4(포트 사용 중 또는 중복 Server, ADR-0009 D6·CLI 명세)로 거부된다 — port 충돌 진단을 사용자에게 위임.
- 후속 작업:
  - **ADR-0009** (tmux daemon 격리) — daemon도 1:1로 분리하여 1:1:1:1 (Server : Session : Port : tmux-daemon) 일관성 확립. 본 ADR의 D3·D4 exit 코드(3, 6)는 ADR-0009 D6 teardown 5단계와 정합.
  - **ADR-0008** (single-pane + Group) — UI scope 축소의 다른 축(Window 제어 → Group 관리)을 정의.
  - **ADR-0010** (Group 데이터 모델) — "한 캔버스 = 한 Session" 전제 그대로 영속화 스키마 정의.
  - **ADR-0003** (보안 디폴트, 후속) — 토큰 파일 경로 `${XDG_STATE_HOME}/gtmux/<session>.token`이 *Server 단위 스코프*임을 명시(본 ADR이 토큰 스코프를 1:1로 고정).
  - **sketch.md §6.1·§6.2** 본문을 *"UI 비범위, 외부 도구 담당"*으로 재서술(grill report §2 amendment list, 동반 PR).

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태/웹 상태 분리 | PASS — 본 ADR이 *바인딩의 1차 키*(Session_name)를 단일 값으로 고정함으로써 두 상태 도메인 모두에 *공통 식별자 1개*만 부여한다. tmux 상태는 그 Session_name으로 tmux daemon이 보유하고, 웹 상태(Canvas Layout)는 같은 Session_name을 파일명 키(`<session>.layout.json`)로 별도 영속화한다. 두 도메인이 같은 키를 *읽기 전용으로 공유*할 뿐 *쓰기 경로가 교차하지 않는다*. 1:N이었다면 어느 쪽 도메인이 N의 권위인지 모호해진다. |
| 2 | tmux-native vs web-only 분기 | PASS — Session 제어(create/list/select/kill/rename/attach)가 *명시적으로 UI 밖*(외부 OS·tmux CLI·외부 도구)으로 빠지고, gtmux Server는 부팅 시점에 Session에 *바인딩만* 한다. 결과적으로 UI 안의 액션 표면은 Pane(tmux-native: create=new-window/close=kill-pane/입력=send-keys) + Panel/Group(web-only: visibility/lock/이동/z) 두 축으로 정확히 갈라지며, sketch §6.1·§6.2의 12개 후보 기능이 UI 표면에서 한꺼번에 제거된다. |
| 3 | tmux Layout ≠ Canvas Layout | PASS — 본 ADR과 직접 무관(Layout 차원은 ADR-0008 single-pane-per-window 컨벤션이 기계적으로 보장). 단, 본 ADR이 *Canvas 1개 = Session 1개*를 잠가둔 덕분에 ADR-0008이 가정하는 "한 Canvas Layout이 한 Session의 trivial tmux Layout들만 mirror하면 된다"는 단순화가 유지된다. |
| 4 | 보안 기본값 | PASS — 인증 토큰이 *Server 단위 = Session 단위 = Port 단위*로 정확히 한 표면을 가리므로 토큰 유효 범위가 사용자에게 직관적이다(`<session>.token` 파일 한 개). 한 Server 침해 시 그 토큰은 그 Session 1개에만 권한을 부여하며, sibling Server·user main tmux 환경에 transit하지 않는다(ADR-0009 격리와 합성). 다중 Session UI(R1)였다면 한 토큰이 N Session을 가리켜 침해 반경이 N배가 된다. |
| 5 | control mode 사용 | PASS — Server는 부팅 시 D2의 인자를 받아 ADR-0009 D2의 전용 daemon 소켓에 `tmux -L gtmux-<session> -C attach -t <session>`로 control mode 1회 attach한다. 1:1:1 모델은 control mode 채널이 *Server 프로세스 안에 정확히 1개*만 존재함을 보장하여 ADR-0001이 가정하는 "단일 FIFO 명령 큐"의 전제를 침범하지 않는다. |

## 미해결 항목 (Open)

- **O1. 활성 Server 목록·오케스트레이션 도구의 인터페이스 규약** → **ADR-0009에서 결정**. 본 ADR은 *비범위 선언*에서 멈추고, ADR-0009가 소켓 디렉터리 컨벤션(`${TMUX_TMPDIR}/tmux-${uid}/gtmux-*`)을 안정화하여 외부 도구의 enumerate 진입점을 정의한다.
- **O2. port↔session 영속 매핑의 SSoT 위치** → **ADR-0009 D6 teardown 5단계 + grill D21 c6 / D22 config 스키마에서 closed.** `<session>.config.toml`의 `[server].port` 필드가 단일 진실로 잠겨 있으며, `--port` 단독 호출 시 그 디렉터리 스캔으로 session을 역조회한다. 본 ADR은 이 매핑이 *immutable bind 결정의 외부 메모*임을 확인하는 데서 그친다.
- **O3. 부재 Session 에러 메시지·exit code 표기 일관성** → **ADR-0011(Backend stack) `clap` CLI 명세에서 결정**. 본 ADR은 exit 3(session 부재)·exit 6(외부 kill 후 종료)을 잠정 부여(grill D20 exit 코드 규약과 정합)하지만, 사용자에게 보이는 stderr 1-line 포맷은 CLI ADR이 확정한다.
- **O4. A0.7 정합성 리뷰 게이트** → `docs/reports/0011-coherence-review.md`에서 본 ADR과 ADR-0008/0009/0010의 cross-reference 정합성을 점검한 후 Status를 Accepted로 승격. 본 단계에서는 Proposed 유지.
