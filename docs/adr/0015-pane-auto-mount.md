# ADR-0015: Pane auto-mount 책임 경계 — frontend cascade PUT

- 상태: Accepted (2026-05-15) — **2026-05-15 amend by ADR-0021**: cascade target 이 *server-wide 모든 frontend* 에서 *trigger session 의 active webpage 만* 으로 좁아짐. multi-session pivot 정합. 본 ADR 의 frontend cascade PUT 패턴은 유지, 다만 routing 만 amend.
- 일자: 2026-05-15 (Proposed + Accepted, plan 0005 Stage I 진입 정합)
- 결정자: agent (frontend-architect role + 사용자 결정 Q4=FE-AUTOMOUNT, 0029 §4)
- 근거 plan: `docs/plans/0005-figma-layout-overhaul.md` Stage I + `docs/plans/0003-s7-lifecycle-ui-implementation.md` §1.2
- 근거 분석: `docs/reports/0029-frontend-design-ref-analysis.md` Q4 (auto-mount 책임)
- 관련 ADR: ADR-0002 (durable=HTTP / ephemeral=WS), ADR-0006 (Canvas Layout 영속화), ADR-0013 (PTY direct — BackendCommand allowlist), ADR-0017 (Layout grid)
- 관련 CONTEXT: `CONTEXT.md` §"Placement principle" (auto-mount 정신) + §"Relationships" (1:1 Pane ↔ Panel)

## 맥락

`CONTEXT.md` §"Relationships" amend (2026-05-14) 가 **모든 Pane 이 정확히 1개의 Panel 로 auto-mount** 됨을 잠갔다. 즉:
- 사용자 명시 New Panel 액션
- frontend bootstrap 의 첫 auto-mount
- 외부 사유로 새로 spawn 된 child process (현재 비범위 — 미래 다중 탭/외부 CLI client)

세 경로 모두 *자동으로* Canvas Panel 로 표시되어야 한다.

현 시점 (plan 0005 Stage H 직후) 의 spawn 흐름:
- `NewPanelButton` 이 CTRL `new-pane` 송신 후 `putLayoutAppendPanel` 로 명시 PUT
- `dispatcher.handleNotifyMirror` 의 `case 'pane-spawned'` 은 `muxStore.addPane(decoded.paneId)` 만 호출 — *layout 에는 진입하지 않음*

따라서 NewPanelButton path 외 경로 (다중 탭에서 한쪽 탭이 spawn 한 pane 을 *다른* 탭이 인식, 또는 미래 외부 spawn) 에서는 *Pane 은 살아있는데 Canvas 에 Panel 이 없는* invariant 위반 상태가 발생 가능.

`plan 0005 §1.2` 의 `S7-BE-AUTOMOUNT` 작명은 backend 가 layout PUT 까지 책임지는 모델을 가정했으나, 0027 §10 분석 + 0029 Q4 사용자 결정으로 *frontend 가 책임* 으로 재배치 됐다 (작명도 **S7-FE-AUTOMOUNT** 로 재명명).

본 ADR 은 그 책임 경계 + 구체 hook 위치 + race 정책 + idempotent 가드 + 거절안 4건 (backend POST /api/spawn, hybrid, etc.) 을 잠근다.

## 결정 (Decisions)

### D1. 책임 = frontend (dispatcher hook)

`dispatcher.handleNotifyMirror` 의 `case 'pane-spawned'` 안에서 *layout 에 없는 pane 발견 시* cascade PUT 으로 자동 추가한다.

```ts
case 'pane-spawned':
  muxStore.addPane(decoded.paneId);
  void appendPanelIfMissing(decoded.paneId);  // ← Stage I 추가
  return;
```

`appendPanelIfMissing(paneId)` 의 책임:
- `panelsStore.panels` 안에 `pane_id = "%${paneId}"` 인 panel 이 이미 있으면 *no-op return*
- 없으면 cascade 좌표 계산 + `putLayoutAppendPanel` 호출
- 412 race 시 1회 rebase (`fetchLayoutAndHydrate` 후 재시도) — 그 사이 다른 탭/source 가 추가했다면 idempotent check 가 두 번째 시도에서 *no-op return*

### D2. Cascade 좌표 정책

CONTEXT.md §"Placement principle" 정합:
- 시작점 = Canvas origin `(0, 0)`
- N 번째 auto-mount panel = `(0 + N×40px, 0 + N×40px)` — N 은 *현 panels.size* (실패한 race 후에도 정확한 값으로 자연 보정)
- Server 메모리에 *별도 cascade index* 보관 안 함 — `panels.size` 만 진실

기존 `NewPanelButton` 의 *viewport center* 좌표 계산은 *사용자 명시 클릭 경로* 에서만 사용 (사용자 mental model — "지금 보고 있는 곳에 만들어 줘"). dispatcher hook 은 cascade 만 사용.

### D3. 두 spawn path 의 정합

기존 NewPanelButton 흐름:
1. CTRL new-pane 송신
2. `pane-spawned` NOTIFY 수신 대기 (response 또는 first-sight)
3. paneId 캡처 후 `putLayoutAppendPanel(token, { ..., x: vpCenterX, ... })` — *viewport center 좌표*

본 ADR 도입 후 동일 흐름 보존. 그러나 *dispatcher 의 pane-spawned hook 이 먼저 처리* 할 수도 있음 (race):
- dispatcher path 가 먼저 → cascade 좌표 (0,0) 류 로 panel 추가
- NewPanelButton path 도 동일 paneId 로 `putLayoutAppendPanel` 시도 → idempotent check 가 *이미 panel 있음* 감지 → no-op

→ NewPanelButton 의 명시 좌표가 *유실되는* edge case 발생. 해결:
- **D3 결정**: NewPanelButton 의 `putLayoutAppendPanel` 호출도 `appendPanelIfMissing` 로 교체 + *cascade 가 아닌 viewport center 좌표를 인자로 전달*. 그 결과 cascade 가 cascade 외 좌표를 받을 수 있도록 helper 시그너처를 일반화:
  ```ts
  appendPanelIfMissing(paneId, { coords?: { x, y } | 'cascade' = 'cascade' })
  ```
- dispatcher path 는 `coords: 'cascade'` (디폴트), NewPanelButton path 는 `coords: { x: vpX, y: vpY }`.
- *먼저 도착한 쪽의 좌표가 쓰임* — race 시 NewPanelButton 이 늦으면 cascade 좌표가 적용되고, NewPanelButton 좌표는 유실. UX 영향: 사용자가 "여기 만들어 줘" 한 위치가 가끔 cascade 위치로 떨어질 수 있음. 정량적으로는 dispatcher hook 의 NOTIFY 수신과 NewPanelButton 의 PUT 사이 ms 단위 race — 일반적으로 NewPanelButton 이 *더 빠름* (자기 자신이 트리거한 NOTIFY 는 다른 단계가 처리되기 전 도착 가능). 실제 빈도는 데모 안정화 단계에서 확인.

### D4. Idempotent 가드의 정확한 조건

`appendPanelIfMissing(paneId, opts)` 는 다음 시점에 *no-op return* 한다:
- `panelsStore.panels` 의 어떤 entry 든 `pane_id === '%${paneId}'` 일 때

부가:
- 두 path 가 동시에 fire 했고 둘 다 가드 통과 → 둘 다 PUT 시도 → 첫 PUT 성공, 두 번째 PUT 의 body 가 같은 pane_id 의 panel 을 *두 번째* entry 로 추가하려 시도 (panelsStore 가 hydrate 전이라). 이 경우 backend 의 schema 검증 R2 (ID 유일성) 가 *Panel.id* 만 검사하므로 *Panel.id 가 다르면* 두 panel 이 같은 pane_id 를 가질 수 있음. 본 시점에 SSoT 에 *Panel.pane_id 유일성* 규칙은 없음 — *recommendation* 만. → 본 ADR 은 frontend 측 idempotent 가드로 *프로세스 안* 에서 보장, *cross-process* 보장은 backend 의 R3 (Panel.pane_id 가 mirror 안에 존재) 와 frontend 의 sanity check 에 의존.

### D5. 외부 spawn 경로 (현재 비범위)

ADR-0013 D8 의 *외부 CLI client* 가 도입되면, 그쪽이 보내는 `pane-spawned` NOTIFY 도 dispatcher 의 같은 hook 으로 흡수. 본 ADR 의 D1~D4 가 그대로 적용 — 새 코드 경로 불필요.

### D6. 다중 탭 동기화

같은 사용자의 두 탭이 모두 dispatcher hook 을 가짐 — 첫 spawn 시 두 탭 모두 `appendPanelIfMissing` 호출:
- 한쪽이 먼저 PUT 성공 → LAYOUT_CHANGED broadcast → 다른 탭은 GET 으로 hydrate → 그 후 dispatcher 의 hook 이 *두 번째* idempotent check 에서 *이미 있음* 감지 → no-op
- 양쪽이 *동시에* PUT (broadcast 도착 전) → 둘 다 panel 추가 시도 → 한쪽 412 → rebase → idempotent check → no-op
- 정합: D2 cascade 좌표가 *현 panels.size* 기준이라 두 탭의 cascade index 가 다르더라도 첫 PUT 의 좌표가 진실로 잠김.

## 거절된 대안 (Rejected)

- **R1. Backend POST /api/spawn 신규 endpoint** — backend 가 PTY spawn 과 동시에 layout PUT 까지 책임. ADR-0002 D9 (durable=HTTP / ephemeral=WS) 정신에는 부합하지만, (a) backend 가 frontend 의 layout schema (Panel.id 패턴, parent_id, visibility 등) 를 알아야 함 → coupling↑ (b) backend 의 multiple WS subscribers 에 *동일한 PUT* 을 보내거나 각 subscriber 가 PUT 트리거 → 결국 frontend 의 두 path 와 동일 race. **거절.**
- **R2. Backend NOTIFY_MIRROR `pane-spawned` 에 *layout 정보* 포함** — `pane-spawned { id, request_id, suggested_x, suggested_y, suggested_w, suggested_h, ... }` 식. backend 가 layout schema 를 알아야 한다는 R1 의 약한 변종. backend 는 *layout 무지* 가 정답 (ADR-0006 D6 의 정합) — 시각 정책은 frontend 의 책임. **거절.**
- **R3. Hybrid — backend cascade index + frontend coords**: backend 가 *N* (cascade index) 만 broadcast 하고 frontend 가 좌표 계산. *backend 는 spawn 횟수 추적해야 함* → state 추가. 본 ADR 의 D2 의 *panels.size* 가 backend state 없이 같은 효과를 냄. **거절.**
- **R4. Per-panel debounce — 0.5s 안에 같은 paneId 의 두 spawn 무시**: race 자체를 흡수하지만 *외부 spawn* 시나리오에서 의미가 모호. D4 의 idempotent 가드가 더 명확. **거절.**
- **R5. dispatcher hook 만 PUT, NewPanelButton 의 명시 PUT 폐기** — 사용자 명시 좌표 (viewport center) 가 *항상* 유실됨. UX 후퇴 — 사용자가 "여기 만들어 줘" 의 직관이 깨짐. D3 의 *two-path with coords* 가 좋은 절충. **거절.**

## 결과 (Consequences)

### 긍정
- **invariant 보존** — Pane 이 살아 있으면 *반드시* Panel 이 표시됨. CONTEXT.md 의 auto-mount 정합.
- **외부 spawn 경로 미래 확장 시 코드 변경 0** — dispatcher hook 이 모든 경로 흡수.
- **backend 의 frontend 무지 유지** — backend 는 layout schema 를 모름. ADR-0006 D6 정합.
- **다중 탭 자연 동기화** — LAYOUT_CHANGED broadcast + idempotent 가드의 조합으로 race 자동 처리.

### 부정
- **race 시 NewPanelButton 의 viewport-center 좌표 유실 가능** — D3 의 첫-도착-쪽 우선 정책. 데모 안정화 단계에서 실 빈도 측정. UX 영향 미미 예상 (사용자가 패널을 즉시 드래그하면 좌표 재정의됨).
- **frontend 의 hook 부담** — dispatcher 가 layout PUT 책임을 가짐. dispatcher 가 단순 fan-out 이상의 역할을 함 — 본 ADR 이 명시.

### 후속 작업
- **Stage I 구현**:
  - `appendPanelIfMissing(paneId, { coords?: ... })` helper 신규 in `src/lib/http/layout.ts`
  - `dispatcher.handleNotifyMirror` 의 `pane-spawned` 안에서 호출 (cascade 디폴트)
  - `NewPanelButton.svelte` 의 `putLayoutAppendPanel` 직접 호출 → `appendPanelIfMissing(paneId, { coords: vpCenter })` 교체
- 다중 탭 시연 — Stage K 의 종합 시연 단계에서 *실 검증*

## 불변식 검증

| # | 불변식 | 검증 |
|---|---|---|
| 1 | tmux 상태 / 웹 상태 분리 | **PASS** — 본 ADR 은 *web-only* Canvas Layout 측 책임 정의. backend 는 layout schema 무지 유지. |
| 2 | tmux-native vs web-only 분기 | **PASS** — Pane lifecycle (backend, child process) 와 Panel mount (frontend, layout) 분기 |
| 3 | tmux Layout ≠ Canvas Layout | **N/A** — ADR-0013 으로 tmux Layout 어휘 폐기 |
| 4 | 보안 기본값 | **PASS** — `appendPanelIfMissing` 도 `Authorization: Bearer` 통과 후 PUT. ADR-0003 정합. |
| 5 | control mode 사용 | **N/A** — control mode 폐기 |

## 미해결 항목 (Open)

- **O1.** D3 의 race 빈도 실측 — Stage K (종합 시연) 또는 별도 phase 에서 정량.
- **O2.** SSoT canvas-layout-schema 의 *Panel.pane_id 유일성* 명시화 — 현재 D4 에서 다룬 frontend 가드는 SSoT 갱신을 *권장* 으로 둠. 추후 SSoT 보강 PR.
- **O3.** 외부 CLI client (ADR-0013 D8 비범위 — P1+) 도입 시 본 ADR 의 D1 hook 이 자연 흡수하는지 *통합 시나리오 검증* 필요.

## 변경 이력

- 2026-05-15: 초안 + Accepted — plan 0005 Stage I 진입 시점. backend 무지 + frontend dispatcher hook + cascade 좌표 + two-path race 정책 + 거절안 5건.
