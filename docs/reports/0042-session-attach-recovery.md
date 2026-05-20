# 0042 — Session attach recovery (idle reconnect UX)

- 일자: 2026-05-16
- 작성자: agent (system-architect role) — 사용자 grilling G50 결과
- 종류: **decision report + FE 구현 계획** (BE 변경 0)
- 근거 ADR: ADR-0019 D5 / D5.1 / D5.2 / D5.3 (본 결정의 amend)
- 관련 ADR: ADR-0021 D6 (heartbeat 15s/30s), ADR-0019 D6 (lease 정합), ADR-0020 (cookie lifecycle)
- 관련 plan: plan-0007 §14.10 FE-10 Tier 3 (WS reconnect backoff) — 본 report 의 attach recovery 가 reconnect 위에 layer 됨

---

## 0. TL;DR

사용자 시나리오:

> 1번 session 으로 진입한 1번 web 이 idle (sleep / tab background) 30s+ 후 BE heartbeat timeout → flock release / active=false / `session_locks_by_cookie` cookie 제거. 그 뒤 사용자가 web 으로 돌아와 [New Terminal] 클릭 시 `Terminal create failed: not the lock holder of "…" — call attach first` 토스트 + 빈 panel.

**원인**: FE WS client 는 transport reconnect 자동 (`lib/ws/client.ts` 의 `auto-reconnect`, Tier 3 spec). 그러나 *attach 의 BE-side 상태 복구* 가 없음. `sessionStore.active` 는 그대로 `{ name: "A" }` 라고 알지만 BE 는 attach 가 없음. 이후 mutation 진입점이 모두 403/orphan.

**해결**: ADR-0019 D5.1 + D5.4 신규 — 두 케이스 분리:
- **Case I (initial entry, D5.4)**: AppPage onMount + sessionStorage hint 존재 시 — **blocking ReconnectModal** + 자동 attempt + [Switch session…] always-visible. 본 화면 mount 차단. 사용자 명시 cancel always 가능.
- **Case II (in-use reactivate, D5.1)**: WS reopen / visibilitychange + page 사용 중 — **silent + mutation guard**. 평소 무음, 결과 분기 (409/404) 만 modal.

**선택 (사용자)**:
- Case II UX: **Silent + mutation guard** (Layer C4)
- Case I UX: **Blocking ReconnectModal** + always-visible [Switch session…] (사용자 grilling G50 follow-up)
- BE: **변경 0** (`attach_handler` 가 이미 idempotent). FE-only.

**우선 구현**: ~~Phase 1 = Case I (D5.4) 먼저, Phase 2 = Case II (D5.1) 후속~~ — **✅ Phase 1 + Phase 2 모두 ship 완료 (2026-05-16)**. 자세 구현 계획 + 8-state 머신 + 실제 ship 정합은 `docs/plans/0008-session-attach-recovery-impl.md`. 후속 분석 (refresh effect-depth loop) 은 `docs/reports/0045-refresh-session-reconnect-loop-analysis.md` — 0045 의 P0 후속으로 reconnectGate 가 4→8 state 로 amend 됐고 (`booting/idle/attaching/hydrating/ready/in_use/not_found/unreachable`), `markReady/markIdle/modalState` 신규. BE attach_handler 의 same-cookie idempotent contract drift 는 별 work package `0046-be-attach-handler-idempotent.md` 로 격리.

---

## 1. Root cause 정확 흐름

| t | BE 상태 | FE 상태 |
|---|---|---|
| t0 (attach) | `holders["A"] = guard{cookie-1}`<br>`by_cookie["cookie-1"] = "A"`<br>flock 보유 | `sessionStore.active = { name: "A" }`<br>WS open |
| t0+30s+ (idle) | heartbeat 30s 무응답 → WS close + flock release + `by_cookie` cookie-1 remove + `holders` "A" remove | (sleep/background) JS 정지 |
| t0+Δ (reactivate) | (변경 없음) | WS client `onclose` → `#scheduleReconnect()` → reconnect 성공 (cookie-1 valid)<br>**`sessionStore.active` 그대로 `{ name: "A" }`** ← gap |
| t0+Δ' (사용자 mutation) | `create_terminal_handler` → cookie-1 not in `by_cookie` → 403 `not_attached`<br>또는 `attach_confirm_handler` 도 동일 | `Canvas.svelte:686-720` 의 `spawnMultiSessionTerminal`:<br>1+2) `mutateLayout` PUT 성공 (BE 가 attach 검사 안 함)<br>3) `attachConfirm` 403 → `Error("not the lock holder…")`<br>catch → toast + orphan panel layout 잔류 |

**Gap**: WS transport 만 복구, *attach state* 복구 없음. mutation 진입점이 attach 가정으로 fail.

## 2. 결정 (D5.1 / D5.2 사본)

### 2.1 Trigger 합집합

- **(a) WS state `reconnecting → open`** — `lib/ws/client.ts` 의 `onStateChange` hook
- **(b) `document.visibilitychange === 'visible'`** — `routes/+page.svelte` 의 lifecycle
- Pre-condition: `sessionStore.active !== null`
- In-flight guard: 중복 trigger 시 동일 promise 반환

### 2.2 attemptReattach 분기

```
POST /api/sessions/{name}/attach
  200          → fresh GET /layout → sessionStore.loadLayout → resume guard 해제
  409          → modal "Session in use by another window" → [Switch session…] / [Logout]
                 → sessionStore.clear() → 큐된 write 모두 abort
  404          → modal "Session no longer exists" → [Switch session…]
                 → sessionStore.clear() → 큐된 write 모두 abort
  401          → /auth redirect (UnauthorizedError 정합)
  5xx/network  → toast "재연결 실패" + retry (1s/2s/4s, 3회) → modal "[Retry]/[Switch]"
                 → guard 는 retry 동안 유지
```

### 2.3 Mutation guard 범위

`sessionStore.reattachInProgress: boolean` + `reattachPromise: Promise<ReattachResult> | null`.

**Guarded write 진입점**:
- `lib/canvas/Canvas.svelte`
  - `spawnMultiSessionTerminal` (createTerminalItem + mutateLayout + attachConfirm)
  - `onnodedragstop` 의 mutateLayout
  - `handleTerminalClick` 의 attachConfirm path
  - `handleNewPanelLegacy` (legacy 분기는 sessionStore.active==null 이므로 guard 적용 안 됨, 그대로)
- `lib/stores/sessionStore.svelte.ts`
  - `#flushViewport` (debounce fire)
  - 향후 추가될 모든 mutation 메서드
- `lib/http/sessions.ts` / `lib/http/terminals.ts` 의 caller — 직접 호출이 있다면 guard 추가
- `lib/ws/dispatcher.svelte.ts` 의 outbound 0x81~0x84 (M/I/Viewport/Focus) — outbound 은 broadcast 라 attach 무관, guard 불필요

**Pattern**:

```typescript
async function spawnMultiSessionTerminal(coords) {
  if (sessionStore.reattachInProgress) {
    const result = await sessionStore.reattachPromise;
    if (result.kind !== 'success') return;
  }
  // ... 기존 로직
}
```

Viewport fire-and-forget 의 경우:

```typescript
async #flushViewport() {
  if (this.active === null) return;
  if (this.reattachInProgress) {
    const result = await this.reattachPromise;
    if (result.kind !== 'success') return;  // abort silently
  }
  // ... 기존 PUT
}
```

## 3. 시나리오 분기 (D5.3 사본)

| # | 시나리오 | 분기 | 사용자 체감 |
|---|---|---|---|
| (a) | page reload | D5 그대로 — Auth/Dialog 흐름 | 명시 선택 |
| (b) | idle → 자기 자신 가능 | silent → 200 → fresh GET layout → resume | 무음, 첫 mutation 만 수십 ms wait |
| (c) | idle → 다른 web 가 takeover | silent → 409 → modal | "다른 창에서 사용 중" — [Switch session…] |
| (d) | idle → session [Delete] | silent → 404 → modal | "Session 이 더 이상 없음" — [Switch session…] |
| (e) | idle → cookie 만료 | silent → 401 → /auth | 재로그인 화면 |
| (f) | idle → BE down | silent → 5xx/network → retry → modal | "재연결 실패" — [Retry] / [Switch] |
| (g) | 명시 [Switch session…] | 사용자 명시 흐름 (D5 + D7) | 정상 |
| (h) | 명시 [Logout] | DELETE /attach + /auth/logout (D5) | 정상 |

## 4. 사용자 시나리오 trace — web-1 idle / web-2 takeover / web-1 복귀 (분기 c)

```
t0:   web-1 cookie-1, attach "A". holders["A"]=guard{c1}, by_cookie[c1]="A"
t0+30s+ (web-1 idle):
      BE heartbeat timeout → flock release, by_cookie -= c1, holders -= "A"
      web-1: 그 사이 background — JS 정지, WS 가 살아 있을 수도 끊겼을 수도

(web-2 가 SessionListModal 에서 1s polling 으로 "A" enable 확인):
      web-2 cookie-2 → POST /api/sessions/A/attach
      BE: by_cookie[c2] 비어있음 → previous_session None
         holders["A"] 비어있음 → 신규 acquire 성공
         holders["A"]=guard{c2}, by_cookie[c2]="A"
      web-2: sessionStore.active = { name: "A" }, layout load, 진행

(web-1 사용자 reactivate):
      WS reconnect 성공 (cookie-1 valid). state reconnecting→open trigger 발화
      또는 visibilitychange 'visible' trigger 발화 (먼저 도착하는 쪽).
      In-flight guard 로 단일 attemptReattach("A") 만 진행.

      FE: sessionStore.reattachInProgress = true, reattachPromise = ...
      POST /api/sessions/A/attach (cookie-1)
      BE: by_cookie[c1] 없음 (release 후) → previous_session None
         holders["A"]=guard{c2} 보유 중 → lock_conflict_response
         → 409 { error: "lock_conflict", holder: { pid:…, ws_conn_id:c2 } }

      FE: 409 분기 → 큐된 outgoing write 모두 abort
         → modal "Session A is now in use by another window
                  (idle timeout 후 다른 webpage 가 가져감)"
            [Switch session…] / [Logout]
         → sessionStore.reattachInProgress = false (분기 종료)

(사용자 선택):
      [Switch session…] → sessionStore.clear() → workspaceSwitcher.open()
                         → SessionListModal 진입
                         → A 는 "in use by server-pid X" disabled row 로 표시
                         → 다른 session 선택 또는 새 session 생성
      [Logout]          → DELETE /attach (cookie-1, 효과 없음 — holder 아니라)
                         → POST /auth/logout → /auth
```

## 5. FE 구현 계획 (코드 변경 inventory)

| 파일 | 변경 |
|---|---|
| `lib/stores/sessionStore.svelte.ts` | + `reattachInProgress: boolean` $state<br>+ `reattachPromise: Promise<ReattachResult> \| null`<br>+ `attemptReattach(name): Promise<ReattachResult>` method<br>+ `clear()` 가 reattach state 도 reset<br>+ `ReattachResult = { kind: 'success' \| 'aborted' }` export |
| `lib/ws/dispatcher.svelte.ts` | `onStateChange` 의 `open` 진입 시 (직전 state = 'reconnecting' 또는 'connecting' 1회 이상 발생) `attemptReattach` trigger.<br>최초 attach 직후의 'connecting'→'open' 은 trigger 제외 (in-flight guard 가 자연 회피 — first attach 시 reattachInProgress=false 이고 sessionStore.active 도 *attach 응답 후 set*) |
| `routes/+page.svelte` | onMount: `visibilitychange` listener bind — `'visible'` + `sessionStore.active != null` 시 `attemptReattach` trigger.<br>onDestroy: cleanup |
| `lib/canvas/Canvas.svelte` | `spawnMultiSessionTerminal`, `onnodedragstop`, `handleTerminalClick` 등 mutation 진입점에 guard pattern |
| `lib/canvas/PanelDanglingOverlay.svelte` | respawn 호출 진입점에 guard pattern |
| `lib/canvas/PanelNode.svelte` | label PATCH / delete 진입점에 guard pattern |
| `lib/sidebar/TerminalListView.svelte` | kill / attach 진입점에 guard pattern |
| `lib/chrome/AttachConfirmModal.svelte` (있다면) | 사용자 명시 attach 진입점은 reattach 와 다른 흐름 — guard 불요 |
| `lib/chrome/SessionInUseModal.svelte` ⭐ 신규 | 409 분기 modal (제목/메시지/[Switch session…]/[Logout]) |
| `lib/chrome/SessionGoneModal.svelte` ⭐ 신규 | 404 분기 modal — 단순 ("Session no longer exists" + [Switch session…])<br>실제로는 SessionInUseModal 의 variant 로 통합 가능 — 한 modal 의 2 mode |
| `lib/chrome/ReattachFailedModal.svelte` ⭐ 신규 | 5xx/network 분기 modal — [Retry] / [Switch session]<br>같이 통합 가능 |
| `lib/stores/connection.svelte.ts` (있다면 활용, 없으면 sessionStore 안) | reattach attempt 카운터 + 마지막 error 노출 (Debug section 용) |

**Test cases** (Vitest + 별 mock):
1. WS reconnecting→open 시 attemptReattach 1회 호출
2. visibilitychange 'visible' 시 attemptReattach 1회 호출
3. 두 trigger 동시 발화 시 in-flight guard 로 1회만
4. 200 → success → guard 해제 + fresh GET layout
5. 409 → modal + sessionStore.clear + 큐된 write abort
6. 404 → modal + clear
7. 401 → /auth redirect
8. 5xx → retry 3회 후 modal
9. Mutation guard: reattachInProgress 동안 spawnMultiSessionTerminal 진입 → wait
10. attempt → 'aborted' → mutation 의 후속 단계 진입 안 함 (orphan layout 방지)

## 6. ADR-0019 D5.1 와의 정합 보장

- `attach_handler` (codebase/backend/crates/http-api/src/sessions.rs:330) 의 idempotent 성:
  - Same cookie / same name reattach 시:
    - `previous_session` lookup (l.369-375): `.filter(|prev| prev.as_str() != name)` → None → l.376-394 skip
    - `holders.contains_key(&name)` (l.398-402): cookie 의 lock 이 timeout-release 된 케이스 → false → l.404+ 신규 acquire 진입
  - Different cookie (web-2 가 takeover): `holders.contains_key(&name) == true` → `lock_conflict_response` (409)
  - ✓ BE 변경 0 으로 D5.1 의 200/409 분기 자연 처리

## 7. 후속 보완 (deferred)

- **PUT layout 의 attach gate (BE-side)** — single-user 환경 + FE guard 로 race window 가 사실상 0 이라 현 결정에서 제외. Stage 7+ BE-9 amend 후보로 backlog.
- **0x89 SESSION_DETACHED frame (BE-side push)** — 두 trigger 합집합으로 충분 + BE 추가 작업 정당화 X. 거절 (D5.1 R2).
- **Settings Debug section 의 last-reattach log** — BE Settings API (ADR-0020 D11) wire 후. 현재는 `console.info('[gtmux] silent reattach', { name, result, http_status })` 만.
- ~~**frontend-handover-v3 §4.3 의 Tier 3 status**~~ — Phase 1+2 ship 후 정합 amend 필요 (Phase 2 의 ensureMutationOk helper / silentReattach / reattachInProgress 까지 ✅ 로 갱신).
- **0045 의 Canvas mount 원자성 후속 sprint (P0-A flowNodes cache + P0-B viewport one-shot)** — plan-0008 P1.9 의 state-level 차단은 *partial mount race* 의 일차 방어. effect-level race (`effect_update_depth_exceeded`) 의 이차 방어는 별 후속. 0045 §6 / §7 의 구현 방향 참조.
- **0046 의 BE attach_handler same-cookie idempotent contract drift** — BE-only work package. attach_handler 가 코멘트의 약속 (same-cookie same-name reattach = no-op) 과 달리 409 반환하는 drift. silentReattach 의 *모든* 호출 회귀 근본 원인 → BE 가 contract 회복 시 FE guard 의 false-positive 분기 (in_use → modal) 가 사라짐. 0046 §3~§6 의 구현 안.

## 8. 변경 이력

- 2026-05-16: 초안 — 사용자 grilling G50 + 시나리오 (web-1 idle / web-2 takeover / web-1 복귀) 합본. ADR-0019 D5/D5.1/D5.2/D5.3 amend 와 짝.
- 2026-05-16 (G50 follow-up): TL;DR amend — Case I (initial entry, D5.4) vs Case II (in-use reactivate, D5.1) 분리. 우선 구현 Phase 1 = Case I 표기. 자세 구현 계획은 plan-0008 으로 분리.
- 2026-05-16 (Phase 1+2 ship + 0045 P0 후속 정합): TL;DR 의 "우선 구현 Phase 1" → "Phase 1+2 모두 ship 완료" 로 갱신. reconnectGate 가 0045 P0 후속으로 4→8 state 머신으로 amend 됐음 + `markReady`/`markIdle`/`modalState` 신규 명시. BE attach_handler 의 same-cookie idempotent contract drift 는 별 work package `0046-be-attach-handler-idempotent.md` 로 격리. §7 deferred 의 0045 Canvas mount 원자성 후속 sprint + 0046 BE work 항목 추가. frontend-handover-v3 status amend 도 Phase 2 까지 확장 표기.
