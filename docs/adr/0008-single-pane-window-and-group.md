# ADR-0008: Single-pane-per-process 컨벤션 + Group 기반 UI 계층

- 상태: Accepted (2026-05-14, A0.7 + A4 게이트 통과. **2026-05-14 amend** — ADR-0013 채택 후 tmux allowlist 절 폐기, single-pane-per-window → single-pane-per-process 의미 단순화. Group 부분 그대로 유지. `docs/reports/0023-pty-poc-verification-and-decision.md` §4.)
- 일자: 2026-05-13 (Proposed) / 2026-05-14 (Accepted, amend 동일)
- 결정자: system-architect (grill 산출)
- 근거 보고서: `docs/reports/0010-grill-amendments.md` (D4 · D8 · D9 · D23), `docs/reports/0001-tmux-control-mode.md` §9 (iTerm2 한계)
- 관련 ADR: ADR-0007 (Server : Session : Port 1:1:1 바인딩 모델), ADR-0010 (Group 데이터 모델), ADR-0001 (tmux 통합 — 본 ADR의 command allowlist를 입력 제약으로 상속)

## 맥락

`docs/sketch.md` §4.1.3은 "tmux Layout과 Canvas Layout은 분리한다"를 핵심 설계 원칙으로, §4.1.4는 "tmux-native 기능과 web-only 기능을 구분한다"를 또 다른 축으로 둔다. §6.2 (tmux window 6개 제어 기능)는 사용자에게 *tmux Window 그 자체*를 노출하는 인터페이스를 전제하고, §6.3 (pane 제어)은 split·resize·select-layout 등 tmux Layout을 능동 변경하는 명령군을 포함한다. 본 ADR은 *Pane 생성 모델*과 *Window 개념의 UI 노출 여부*라는 두 결정을 한꺼번에 정리하며, 그 결과로 §6.2를 폐기하고 §6.3 명령군의 일부를 backend allowlist에서 제외한다.

`docs/reports/0001-tmux-control-mode.md` §9는 결정의 직접적 근거다 — iTerm2 문서가 명시적 한계로 인정한 "tmux 윈도가 들어있는 탭에는 non-tmux split pane을 둘 수 없다 + tmux는 모든 클라이언트에 동일 크기를 강제하므로 시각적 정렬 문제가 생긴다"는 같은 control mode 위에서 동일하게 적용된다. 같은 Window의 panes는 tmux Layout이 결정한 split 비율로 *size가 묶이고*, 다중 클라이언트가 attach된 경우 *최소 크기로 강제 수렴*한다. 자유로운 Panel resize를 보장하려면 같은 Window의 panes를 캔버스에서 독립적으로 크게/작게 만들 수 없다.

이 제약을 피하면서 §4.1.3의 spirit을 유지하는 가장 정직한 방법은 **gtmux UI가 만드는 모든 Pane을 single-pane-per-window 컨벤션으로 묶어 tmux Layout이 trivial(window-size = pane-size)이 되게 하고**, 동시에 tmux Window를 UI에 노출하지 않은 채 사용자 측 Panel 묶음은 web-only **Group**(Figma-식 layer 트리, ADR-0010)이 담당하게 만드는 것이다.

## 결정 (Decisions)

- **D1.** [grill D8] gtmux UI의 "Pane 생성" 액션은 항상 `tmux new-window -t <session>`을 발급한다. **gtmux가 만드는 모든 tmux Window는 정확히 1개의 Pane을 담는다** (single-pane-per-window 컨벤션).
- **D2.** [grill D8] gtmux backend의 tmux command allowlist에서 **`split-window` · `resize-pane` · `select-layout`을 영구 제외**한다 — gtmux는 이 명령들을 *발급하지 않는다*. (ADR-0001이 allowlist를 정식으로 정의하며, 본 ADR의 표를 그대로 상속한다.)
- **D3.** [grill D4] tmux **Window 개념은 gtmux UI에 노출되지 않는다**. tmux Window는 *implementation-only* 강등 상태이며, 사용자 측 Panel 묶음은 web-only 계층 **Group**(ADR-0010)이 담당한다. 기존 sketch §6.2 "tmux window 제어 6기능"은 본 ADR과 동반하는 sketch amend로 전면 폐기된다.
- **D4.** [grill D4 + D23] 외부 tmux CLI로 split된 multi-pane Window의 panes는 정직하게 mirror한다:
  - 각 pane이 새 Panel로 **즉시 캔버스에 자동 cascade 배치**된다 — 좌표는 미지정 처리되어 `CONTEXT.md` "Placement principle"의 자동 cascade 규칙(origin (0, 0) + (40px, 40px) offset 누적, cursor는 Server 메모리만)으로 결정한다. (**Unplaced Panel 트레이 개념은 D23에서 폐기되었으므로 본 ADR도 그것을 차용하지 않는다.**)
  - 그 Window에 속한 모든 Panel은 **canvas resize 잠금** 상태가 된다 (move · minimize · hide · close · z-index 조정은 가능). 이는 tmux가 같은 Window의 panes 크기를 강제 결합하기 때문이며 (R1 §9), gtmux가 자유 resize를 *약속하지 않는* 영역임을 시각화한다.
  - 사용자가 그 Window의 panes를 하나씩 close해 single-pane 상태가 되면 그 시점부터 resize 활성화로 복귀한다 (gtmux 컨벤션과 정합).
- **D5.** [grill D9] 사용자가 Panel에 label을 설정하면 gtmux는 `rename-window -t @W <label>`로 tmux 측 window 이름을 동기화한다 — 외부 attach 클라이언트(예: 별도 터미널에서 `tmux a -t <session>`)에서의 사용성을 보존하기 위함이다. 우선순위 P1 디테일.

### tmux command allowlist 표 (**Deprecated — 2026-05-14 amend**)

**본 표는 ADR-0013 채택과 함께 영구 폐기**. tmux 명령 어휘 자체가 사라지므로 allowlist 컨셉도 무효. 대체 모델 = ADR-0013 D10·D12 의 *우리 API command schema 를 Rust enum 으로 정의 + exhaustive match 로 compile-time 강제*. enum variant 추가 = 명시적 API 확장 = allowlist 의 역할 그대로.

historical 참조 (표 자체는 보존):

| 명령 | 발급? | 사용 | 근거 |
|---|---|---|---|
| `new-window -t <session>` | ✅ | Pane 생성 = Window 생성 (single-pane-per-window 컨벤션) | D1 (grill D8) |
| `kill-pane -t %<pid>` | ✅ | Panel close / Group close 재귀 | D1, D3 |
| `kill-window -t @<wid>` | ✅ | gtmux 내부 정리용 (마지막 pane이 kill된 빈 Window 청소) | D1 |
| `rename-window -t @<wid> <label>` | ✅ | Panel label → tmux window name 동기화 | D5 (grill D9) |
| `send-keys -t %<pid>` | ✅ | Input Target(I)로 지정된 Panel의 입력 전달 | (도메인 기본, sketch §6.3) |
| `refresh-client -A '%<pid>:pause/continue'` | ✅ | Panel Streaming State 전이 (Suspended/Streaming) | grill D16 → ADR-0001 |
| `refresh-client -B <subscription>` | ✅ | 포맷 구독 (tmux 3.2+ 푸시 모델) | R1 §6 → ADR-0001 |
| `capture-pane -p -e -J -S -<lines>` | ✅ | Deep scrollback 회복 (P1+ 명시 액션) | grill D15 → ADR-0001 |
| `list-sessions -F` / `list-windows -a -F` / `list-panes -a -F` | ✅ | 부트스트랩 1회 스냅샷 | R1 §1, §3 → ADR-0001 |
| `split-window` | ❌ | single-pane-per-window 컨벤션 직접 위반 | **D2 (grill D8)** |
| `resize-pane` | ❌ | single-pane window는 window-size = pane-size, 별도 명령 불필요 | **D2 (grill D8)** |
| `select-layout` | ❌ | tmux Layout을 능동 변경하지 않음 (§4.1.3 정신) | **D2 (grill D8)** |
| `-CC` (control mode variant) | ❌ | 백엔드는 `-C`만 사용 (DCS 래핑은 터미널 에뮬레이터 용) | R1 §1 → ADR-0001 |

### [2026-05-14 amend] 본 ADR 의 의미 단순화

ADR-0013 채택으로 tmux 가 backend 에서 사라지면서 다음과 같이 의미 변환된다:

- **D1 (gtmux UI의 "Pane 생성" 액션)**: *그대로 유지* 하되 어휘 변경 — *"`tmux new-window -t <session>` 을 발급한다"* 대신 *"`portable_pty::openpty()` + `pair.slave.spawn_command(shell)` 호출로 새 Pane (= PTY pair + child process) 1개 생성"*. **single-pane-per-window** 컨벤션은 **single-pane-per-process** 로 자연 단순화 — 한 Pane = 한 PTY pair = 한 child process 가 trivial 1:1.
- **D2 (command allowlist)**: *폐기* — ADR-0013 D12 의 *Rust enum exhaustive match* 가 allowlist 역할 자동 계승. `split-window` / `resize-pane` / `select-layout` 같은 tmux 명령 어휘 자체가 사라짐.
- **D3 (tmux Window 비노출)**: *영구 비범위* — tmux Window 컨셉 자체가 사라지므로 noop. 사용자 측 묶음은 Group 트리 (ADR-0010) 그대로.
- **D4 (외부 multi-pane Window mirror)**: *영구 비범위* — 외부 tmux CLI 자체가 사라짐. 외부 attach 시나리오는 ADR-0013 D8 의 명시 비범위.
- **D5 (Panel label → window name 동기화)**: *영구 비범위* — `rename-window` 발급 안 함. Panel label 은 web-only 상태로만 보유.

본 ADR 의 *Group 부분* (사용자 측 묶음 = web-only 계층 트리) 은 그대로 보존. ADR-0010 (Group 데이터 모델) 가 정본.

## 거절된 대안 (Rejected)

- **R1. (P-γ) multi-pane Window 1차 시민화** — 한 tmux Window 안에 split된 panes를 캔버스의 일반 Panel로 노출하고 자유 resize를 허용하는 안. tmux가 같은 Window의 모든 클라이언트에 *동일 크기를 강제*(R1 §9, iTerm2 문서 [2] 인용)하므로 자유 resize가 구조적으로 불가능. iTerm2조차 "tmux 윈도가 들어있는 탭에는 non-tmux split pane을 둘 수 없다"를 명시적 한계로 인정 (R1 §9). 거절. (grill D8)
- **R2. (P-α 채택 + tmux Window UI 노출 + flat 1-level 그룹화)** — single-pane-per-window 컨벤션을 유지하면서 tmux Window를 UI 1차 시민으로 노출하고, Window를 평면 그룹화(1-level)로 사용. 모든 Window가 single-pane이라 "왜 모든 window가 single-pane인가?"라는 사용자 멘탈모델 비대칭을 유발 + flat 1-level 그룹화는 Figma-식 계층 Group보다 표현력이 약함 (중첩 묶음·일괄 visibility 토글·drag-reparent 부재). 거절. (grill D4)
- **R3. (P-α 채택 + tmux Window 이름만 UI 표시)** — single-pane 컨벤션을 유지하면서도 사용자에게는 tmux Window를 도메인 개념인 양 보여줌. 사용자 멘탈모델이 *부정직* — implementation artifact를 도메인 개념으로 위장. 거절. (grill D4)
- **R4. (Unplaced Panel 트레이)** — 외부 tmux CLI로 생성된 Pane을 캔버스에 즉시 배치하지 않고 사이드바 "Unplaced Panel" 트레이에 모아두는 안. 사용자가 "보이지 않는 상태"의 Panel을 관리해야 하는 추가 멘탈모델 부담 발생. D23에서 *자동 cascade 즉시 배치*로 대체되며 본 ADR도 그 결정을 따른다. 거절. (grill D23)

## 결과 (Consequences)

- 긍정:
  - **자유 Panel resize 완전 보장** — gtmux-created 영역에서 single-pane window = window-size = pane-size, tmux Layout 제약이 *구조적으로 존재하지 않음*.
  - tmux command allowlist 축소 → **보안 표면 ↓** (sketch §13.3.3 "명령 주입 표면" 자동 감소, 세 종류 명령 어휘가 영구 제거됨).
  - sketch §4.1.3 "tmux Layout ≠ Canvas Layout" 불변식이 gtmux-created 영역에서 *기계적으로* 보장됨 — tmux Layout이 trivial이므로 비교 대상 자체가 없음.
  - Group 계층의 표현력 ↑ (재귀적 묶음, label/color/visibility/lock 일괄 적용, drag-reparent — ADR-0010 §G-hybrid 모델).
  - 외부 multi-pane Window mirror는 정직한 fallback path로 존재(D4) — gtmux 컨벤션을 강제하지 않으면서도 자유 resize 약속을 깨지 않음.
- 부정/비용:
  - tmux 측 Window 개수가 Panel 개수와 동일 → bootstrap 시 N개의 `%window-add`/list snapshot 이벤트 발생. 영향은 무시 가능 — R1 §5·§9 데이터 기반, 50 pane 시나리오에서 ms 단위 (ADR-0009 §실측 footprint 표 참조).
  - 외부 tmux CLI에서 attach한 사용자는 다수의 single-pane window를 보게 됨 (1 Panel = 1 tmux Window) → D5의 `rename-window` 동기화로 사용성 완화.
  - sketch §6.2 "tmux window 제어 6기능"은 전면 폐기 → sketch 본문 수정 필요 (본 ADR 동반 PR의 sketch amend로 처리).
- 후속 작업:
  - **ADR-0001** (tmux 통합)의 command allowlist 절은 본 ADR의 표를 그대로 인용한다.
  - **ADR-0010** (Group 데이터 모델)이 G-hybrid 상태 모델·트리 구조·운영 규칙을 정의한다.
  - sketch §10.1 "백엔드 구성 — tmux command router"의 발급 명령 집합 명시 (본 ADR 동반 amend).
  - sketch §6.5 "Pane List / 탐색" 절은 "window별 그룹 정렬" 표현을 **Group 계층 트리(사이드바 layer panel)**로 교체.

## 불변식 검증

| # | 불변식 | 검증 |
|---|--------|------|
| 1 | tmux 상태 / 웹 상태 분리 | **PASS** — tmux Window가 implementation-only로 강등되어 *도메인 어휘에서 제거*됨. 사용자에게 노출되는 묶음 어휘는 web-only **Group** 단일 — 두 상태 도메인이 어휘 수준에서 교차하지 않는다. tmux Window·Pane은 mirror-only, Group·Panel layout은 web-only authored. |
| 2 | tmux-native vs web-only 분기 | **PASS** — D1 (Pane 생성 = `new-window`), D2 (allowlist), D5 (label 동기화), 그리고 D4의 외부 multi-pane mirror는 모두 tmux-native 측면. 캔버스 자동 cascade 배치, Group 트리, resize lock UX는 모두 web-only. 두 카테고리가 한 결정 안에서 혼합되지 않으며, allowlist 표가 경계를 *기계적으로 강제*한다. |
| 3 | tmux 레이아웃 ≠ 캔버스 레이아웃 | **PASS** — gtmux-created 영역의 tmux Layout이 trivial(single-pane window-size = pane-size)이라 비교 대상 자체가 없음. 외부 multi-pane Window의 tmux Layout만 실질적 의미를 가지며, 그 경우에도 Canvas Layout(Panel x/y/w/h)과는 *별개 데이터 도메인*으로 처리됨. tmux Layout이 캔버스 좌표로 *변환되거나 강제되지 않음*. |
| 4 | 보안 기본값 | **PASS** — command allowlist에서 `split-window`·`resize-pane`·`select-layout` 영구 제외 → sketch §13.3.3 "명령 주입 표면"이 *구조적으로* 축소됨. argv 분리 정책(sketch §13.3.4)이 보호해야 할 명령 어휘 자체가 줄어들어 fail-closed 디폴트와 정합. |
| 5 | control mode 사용 | **PASS** — 본 ADR이 허용/금지하는 모든 발급 명령은 ADR-0001이 정의하는 `tmux -C` 채널 내에서만 흐른다. allowlist 표의 `-CC` 금지 항목이 이를 명시적으로 표현. 사용자 입력으로 임의 tmux 명령이 raw shell로 빠져나갈 경로 없음. |

## 미해결 항목 (Open)

- **O1.** **외부 multi-pane Window의 Panel resize lock UX 시각 표현** — D4가 lock 상태의 *존재*는 결정했지만, 사용자에게 보여줄 시각 어휘(자물쇠 아이콘 / 색 띠 / hover tooltip 문구 / cursor 변형 등)는 구현 단계(배치 D, 프론트엔드 부트스트랩)에서 ADR-0010 Group lock UX와 함께 일관되게 결정한다. 본 ADR은 *behavior*만 잠근다.
