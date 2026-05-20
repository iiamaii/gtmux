# ADR-0019: Session 과 Workspace 모델 — multi-session pivot (ADR-0007 supersede)

- 상태: Accepted (2026-05-15)
- 일자: 2026-05-15 (Proposed + Accepted, plan 0006 의 multi-session pivot grilling 결과)
- 결정자: agent (system-architect role) + user grilling 17 결정
- 근거 grilling: 2026-05-15 plan 0006 grilling 의 Q4 / Q5 / Q6 / Q8 / Q11 / Q13 / Q14 / Q15 / Q16
- 근거 plan: `docs/plans/0007-multi-session-pivot.md`
- **Supersedes: ADR-0007** (Server : Session : Port = 1:1:1 — multi-session pivot 으로 폐기)
- Amends: CONTEXT.md (Session/Workspace/Webpage 어휘 재정의), ADR-0002 D3 (MT-3 → session-scoped + server-wide 2-layer), ADR-0006 D14 (panels[] strip → match-or-spawn, D15 신규)
- 관련 ADR: ADR-0018 (Canvas Item Data Model v2), ADR-0020 (Auth Lifecycle), ADR-0021 (Terminal Pool + Mirror, ADR-0015 amend)
- 관련 SSoT: `docs/ssot/canvas-layout-schema.md` (v2 갱신 필요)

## 맥락

ADR-0007 의 1:1:1 (Server : Session : Port) 모델은 single-session 시대 — *한 server lifetime = 한 사용자 작업 단위* — 의 가정이었다. 2026-05-15 의 큰 사용자 요청은 이 가정을 흔든다:

1. **여러 작업 흐름을 한 server 에서** — workspace 별 분리, 사용자가 명시 save/load.
2. **인증 lifecycle** — 매번 재인증 없이 cookie 등으로 자동 진입.
3. **여러 webpage 가 같은 사용자, 다른 작업** — 한 사용자가 동시에 여러 session 을 다른 브라우저 탭에서 운용.

ADR-0007 의 single-session 모델은 이 요구를 자연 표현 못 한다. 본 ADR 은 entity 모델을 재정렬한다.

### 옛 모델 (ADR-0007)

```
Server : Session : Port = 1:1:1
Session = Server 의 logical 이름 (CLI --session demo)
사용자 mental model = Session = workspace 단위
```

문제: workspace 별 작업 분리를 위해 *여러 server 를 다른 port 로 띄움* 이 유일 수단. 사용자 인지 부담 + port 관리 + 인증 / lock / log file 의 분산.

### 새 모델 (본 ADR)

```
Server : Port = 1:1            (변경 없음)
Server : Workspace = 1:1       (Workspace 신규)
Workspace : Session = 1:N      (Session 의미 재정의)
Webpage : Session = 1:1        (single-attach reciprocal)
Webpage : Server = N:1         (여러 탭 가능)
Terminal : Session = N:N       (multi-session mirror, ADR-0021)
```

Workspace 는 *storage location 의 책임* 만 가지는 신규 1차 어휘 — 그 안의 named record 가 Session 이다.

## 결정 (Decisions)

### D1. Server : Workspace = 1:1, Workspace : Session = 1:N

- 한 **Server** 는 정확히 한 **Workspace 디렉터리** 에 바인딩.
- 한 **Workspace** 디렉터리 안에 0 개 이상의 **Session** file record (`<session-name>.json`).
- 한 **Session** 은 정확히 하나의 **Canvas Layout + viewport + selection** 을 가진다.
- Session 은 사용자가 명시 *생성 / 삭제 / 이름 변경* — workspace 안에서 자유 관리.

### D2. Workspace 의 storage location

| 항목 | 값 |
|---|---|
| Default workspace path | `${XDG_DATA_HOME:-~/.local/share}/gtmux/workspace/` |
| Config file | `${XDG_CONFIG_HOME:-~/.config}/gtmux/config.toml` |
| Config key | `workspace_path = "/some/path"` (절대 경로) |
| CLI override | `gtmux start --workspace <path>` (boot 1회) |
| 디렉터리 미존재 시 | boot 시 자동 생성 |
| Runtime path 변경 | 불가 (재기동 필요) — config 변경은 다음 server lifetime 부터 |
| 사용자 명시 backup 단위 | workspace dir 전체 또는 개별 session file (file export) |

선택 이유: XDG Base Directory 표준 정합. 사용자 데이터는 `XDG_DATA_HOME`, 설정은 `XDG_CONFIG_HOME`, 휘발성 상태 (lock/token) 는 `XDG_STATE_HOME`. dotfile 도구 (stow, chezmoi) 정합.

대안 검토:
- `~/Documents/gtmux/workspace/` (GUI 도구 가시) — 거부. Linux server 환경에 ~/Documents 가 자주 없음. Photoshop/Figma 류와 달리 gtmux 는 *서버형 도구* 의 성격이 강함.
- `~/.local/state/gtmux/workspace/` (현 layout file 의 자리) — 거부. STATE_HOME 은 *transient/recreatable* 용도 (XDG spec) 이며 사용자 적극 관리 데이터는 DATA_HOME 이 적절.
- `~/.gtmux/` 단일 dir — 거부. XDG 비표준.

### D3. Webpage : Session = 1:1 (single-attach reciprocal)

- 한 Webpage 는 한 시점에 정확히 0 또는 1 Session 에 attach (인증 dialog 통과 전까지 0, 통과 후 1).
- 한 Session 은 한 시점에 정확히 0 또는 1 Webpage 의 attach 를 받음.
- 즉 **multi-webpage = 다른 session 들**. 동일 session 의 다중 attach 는 금지.

이유 (사용자 명시):
- 동일 session 의 multi-attach 는 race / 양방향 sync overhead / UX 모호 (어느 webpage 가 "primary" 인가) 야기.
- multi-monitor mirror 사용 케이스는 **Terminal 의 multi-attach (ADR-0021)** 가 별도로 처리.
- WS heartbeat (ADR-0021 D6) 로 active 여부를 안정 감지 가능 (수십 sessions 부담 0).

### D4. 활성 session takeover 금지

활성 session (= attach 한 webpage 있음) 은 modal 의 session list 에서 *disabled row + "in use" badge* 로 표시. 강제 takeover 경로 없음. 다른 webpage 가 그 session 을 사용하려면 *현재 attached webpage 가 close 되어야* (정상/비정상 모두, 비정상은 30s 안에 heartbeat 으로 감지).

이유: takeover 허용 시 *원래 webpage 의 사용자 경험 파괴* (강제 reload) + race condition + abuse vector. single-user 환경이라 *takeover 의 효용* 자체가 낮음 (다른 session 만들면 됨).

### D5. Session lifecycle — file 영속, active flag ephemeral

- **Session file record** 는 workspace dir 의 file (`<session-name>.json`). 사용자 명시 [Delete] 만 destroy.
- **Session 의 active state** 는 server memory ephemeral. attached webpage 의 WS lifetime 과 일치.
  - 정상 WS close (탭 닫기, 명시 logout) → 즉시 active=false
  - 비정상 close (network drop, OS crash) → heartbeat 15s ping / 30s timeout 로 active=false (ADR-0021 D6)
- 동일 server lifetime 안에 같은 webpage 가 **reload** → 새 WS 연결 → 인증 후 dialog 의 흐름으로 다시 선택 (자동 재attach 안 함).
- 동일 webpage 가 **page 유지 + idle reconnect** (sleep/wake, 탭 background 후 활성화 등) → **silent attach recovery** 를 시도 (D5.1).

이유: workspace 의 *명시 save/load* 가 영속성의 1차 메커니즘이라, *자동 재attach* 류는 책임 경계를 흐림. session record 가 file 영속이므로 사용자 작업 손실 위험은 0. 단 **page 가 유지된 상태의 idle reconnect** 는 *사용자가 명시 "이 session 으로 돌아가겠다" 라는 의도를 이미 표현한 상태* 이므로 silent 복구가 정당화 — `reload` 의 dialog 의 흐름과 다른 케이스 (D5.1).

### D5.1 Idle reconnect — silent attach recovery (2026-05-16 amend, G50)

#### 맥락

D5 의 *reload* 케이스는 사용자가 *명시* 새 WS 연결을 시작하므로 dialog 흐름이 자연. 그러나 **page 가 유지된 상태에서 transport 만 끊겼다 복구된 케이스** 는 다르다:

- 사용자 시나리오: 노트북 sleep, 탭 background, OS network blip 등. `sessionStore.active = { name: "A" }` 그대로, 단지 WS 만 끊겼다 복구.
- BE 측 상태: heartbeat 30s timeout → flock release + `session_locks_by_cookie` entry remove + active=false. 즉 *재연결된 WS connection 은 attach 가 없는 상태*.
- 사용자가 어떤 mutation (e.g., [New Terminal]) 을 시도하면 BE 가 `403 not_attached` → FE 가 `Terminal create failed: not the lock holder of "A" — call attach first` 노출. 사용자는 *무슨 조치를 해야 할 지 모름* — 빈 panel 만 layout 에 남음.

본 D5.1 amend 는 이 gap 을 *silent 자동 복구 + mutation guard* 로 닫는다.

#### 결정

- **Trigger 의 합집합**: 다음 두 이벤트 중 하나라도 발생하면 `silentReattach()` 호출 (in-flight singleton 으로 중복 방지, D5.2):
  - **(a) WS state transition `reconnecting → open`** — `lib/ws/client.ts` 의 `onStateChange` hook → `dispatcher.svelte.ts:541-559` 의 `prevWsState === 'reconnecting' && state === 'open'` 분기. transport-level recovery 시점.
  - **(b) `document.visibilitychange === 'visible'`** — 탭이 background 였다가 다시 foreground (`+page.svelte:204-208` listener). JS throttled 케이스 (WS 가 alive 인데 BE 가 30s timeout 한) 도 cover.
- **Pre-condition (2026-05-18 amend ② — 6 precondition AND-gate)**: `maybeSilentReattach` (`+page.svelte:148-157`) 가 다음 6 조건 모두 충족 시에만 silent 진입. 한 개라도 false 면 no-op:
  1. `typeof document !== 'undefined'` — SSR/test 가드
  2. `document.visibilityState === 'visible'` — Case II 정의
  3. `reconnectGate.canMountApp === true` (= `ready` ∨ `idle`) — booting/attaching 중에는 silent 진입 안 함 (D5.4 의 blocking modal 이 owner)
  4. `sessionStore.active !== null` — Auth/Dialog 단계 webpage 는 trigger 무시
  5. `sessionStore.reattachInProgress === false` — 이미 silent 중이면 중복 안 함 (D5.2 의 singleton)
  6. `heartbeatStore.isIdle === true` (= 15s+ 사용자 무활동) — server frame 곧 흐를 가능성 낮을 때만 (활성 사용자는 곧 server frame 받음 → silent 불필요)
- **`attemptReattach(name)` 흐름**:
  ```
  POST /api/sessions/{name}/attach   (cookie 자동 동봉)
    ├─ 200 + unmatched=0 → 정상 (idempotent — 자기 자신이 다시 holder.
    │        attach_handler 의 previous_session lookup + holders.contains_key
    │        분기 둘 다 no-op 으로 통과).
    │        후속: GET /api/sessions/{name}/layout → sessionStore.loadLayout
    │              (idle 동안 자기 자신이 마지막 PUT 했다면 그대로, 다른 webpage
    │               가 짧게 attach 했다 detach 했다면 그쪽 변경 흡수).
    │        resume — guard 해제 → 큐된 outgoing write 진행.
    │
    ├─ 200 + unmatched>0 (silent→modal escalation, 2026-05-17 amend ②) →
    │        BE 가 재기동되어 layout 의 terminal UUID 가 stale (PaneId 미존재).
    │        Case II 라도 *respawn 결정은 사용자 몫* 이므로 silent 유지 불가.
    │        FE 가 `setActiveSession({ name }) + workspaceSwitcher.goAttachConfirm`
    │        호출 → AttachConfirmModal 노출. 본 escalation 이 없으면 panel 만
    │        남고 respawn 누락되는 회귀 (page.svelte:169-176, sessionStore.ts:
    │        attemptReattach 의 confirm_required 분기). 본 분기는 D5.5 의
    │        *Cancel = 5-step fallback chain* 와 짝.
    │
    ├─ 409 lock_conflict → 다른 webpage 가 attach 보유. 응답의 holder.pid 표시.
    │        UX: modal "Session A 는 다른 창에서 사용 중 — idle timeout 후
    │            다른 webpage 가 가져감". 옵션 [Switch session…] / [Logout].
    │        sessionStore.clear() 후 SessionListModal/Logout 으로 라우팅.
    │        큐된 outgoing write 는 **모두 abort**.
    │
    ├─ 404 → session 이 그 사이 [Delete] 됨. modal "Session A 가 더 이상
    │        존재하지 않음" + [Switch session…]. write 모두 abort.
    │
    ├─ 401 → cookie 만료 / token rotation. /auth 로 redirect (현 unhandled
    │        UnauthorizedError 패턴 정합).
    │
    └─ 5xx / network → toast (silent 가 아닌 명시 fail). exponential retry
              (1s/2s/4s/cap 30s, 3회 후 modal "재연결 실패 — [Retry] /
              [Switch session]"). guard 는 retry 동안 유지.
  ```
- **Mutation guard 범위 (D5.2 정의)**: silent reattach in-flight 동안 *모든 outgoing write* (PUT layout / POST attach_confirm / POST terminals / DELETE items / viewport debounce 의 fire / kill / respawn / patch label 등) 는 promise 대기 또는 abort. 결정 분기 후 resume 또는 모두 abort.
- **BE 변경 ✓** (2026-05-16 amend ③, work package 0046) — `attach_handler` (sessions.rs:330) 가 같은 cookie 의 same-name reattach 를 idempotent 200 으로 normalize. 코멘트는 이미 약속한 동작이었으나 코드가 어겼던 contract drift — line 396 직전에 cookie ownership 분기 + `reuse_existing_attach_response` 헬퍼 추가로 정합. 다른 cookie 의 takeover 는 D4 그대로 409.

#### D5.2 Mutation guard — outgoing write 의 in-flight 정의

`sessionStore.reattachInProgress: boolean` flag + `#silentReattachPromise: Promise<ReattachResult> | null` (private singleton). 모든 outgoing write 진입점 (`lib/canvas/Canvas.svelte` 의 `spawnMultiSessionTerminal` 등, `lib/stores/sessionStore.svelte.ts` 의 `#flushViewport` 등, `lib/http/sessions.ts` / `lib/http/terminals.ts` 의 caller 들) 은 시작 시점에 다음을 점검 (`ensureMutationOk` helper 로 일관화):

```typescript
// sessionStore.ts ~line 822 ensureMutationOk 의 본질:
if (this.#silentReattachPromise !== null) {
  const result = await this.#silentReattachPromise;
  if (result.kind !== 'success') return false;   // 409/404/unreachable — abort
}
return true;
```

##### D5.2.1 In-flight singleton (2026-05-18 amend — 코드 SoT 정합)

`silentReattach(name, signal)` 의 *singleton 동작* (sessionStore.ts:519-552):

```typescript
silentReattach(name, signal) {
  if (this.#silentReattachPromise !== null) {
    return this.#silentReattachPromise;     // 두 번째 호출은 같은 promise 반환
  }
  this.reattachInProgress = true;
  this.#silentReattachPromise = (async () => {
    try { return await this.attemptReattach(name, signal); }
    finally {
      this.reattachInProgress = false;
      this.#silentReattachPromise = null;
    }
  })();
  return this.#silentReattachPromise;
}
```

**효과**:
- D5.1 의 trigger 2개 (WS reconnecting→open + visibilitychange) 가 *같은 tick* 에 발화해도 fetch 1번만 실제 진행.
- ensureMutationOk 가 그 promise 를 await — 즉 mutation 진입점들이 silent reattach 결과를 *기다린 후* mutation 진행 (race 차단).
- AbortController 가 별 — D5.4 의 reconnectGate 가 명시 cancel 시 abort 전파, silent 흐름의 in-flight singleton 은 cancel 안 됨 (silent 는 사용자 perception 없음 → 끊을 이유 없음).

Viewport debounce 처럼 *fire-and-forget* 패턴은 `#flushViewport` 진입 시 동일 점검 + `aborted` 시 silent skip. PUT layout 의 ETag CAS 가 추가 안전망 (412 → 자연 conflict 처리).

#### 거절된 대안

- **R1.** BE 가 PUT layout 에 attach gate 추가 — FE-only 결정과 충돌, BE 의 endpoint 책임 경계 변경. R1 의 race risk 는 FE guard 가 cover 함. 거절.
- **R2.** 새 WS frame `0x89 SESSION_DETACHED` 로 BE → FE push — 두 trigger 합집합 (WS reopen + visibilitychange) 만으로 충분, BE 추가 작업 정당화 X. 거절.
- **R3.** Modal block 으로 차단 (Layer C3) — 빈번하면 사용자 짜증. 복구 가능한 케이스는 silent 가 적절. 거절.
- **R4.** Subtle banner (Layer C2) — 평소 OK 케이스에 noise. mutation guard 의 *체감 latency* (수십 ms) 가 충분히 짧아 별 표시 불필요. 거절.
- **R5.** 단일 trigger (WS reopen 만) — JS throttled 의 long-lived WS 케이스 누락. visibilitychange 와 합집합 필수. 거절.

#### 결과

- 긍정: 사용자 idle 후 복귀 의 *주요 UX 끊김* 제거. "Terminal create failed: not the lock holder" 류 메시지 사라짐. takeover 케이스 (web-2 가 가져간 상태에서 web-1 복귀) 도 modal 한 번으로 자연 흐름.
- 부정: silent path 의 *디버깅 가시성* 감소. mitigations: `console.info('[gtmux] silent reattach', ...)` + Settings · Debug section (BE wire 후) 의 마지막 reattach log.
- 후속: BE 의 stale layout PUT (cookie 가 holder 아닌 cookie 의 PUT) 의 long-term 보호는 Stage 7+ 의 BE-9 amend 후보. 현재는 single-user 환경 + FE guard 로 race window 가 사실상 0.

### D5.3 결정 매트릭스 — D5/D5.1/D5.2/D5.4 vs 시나리오

| 시나리오 | 분기 |
|---|---|
| **(a-1) page reload, sessionStorage hint 없음** | 새 WS 연결 + Auth/Dialog 흐름 (D5 그대로) — 사용자 명시 선택 |
| **(a-2) page reload, sessionStorage hint 있음** | **Blocking ReconnectLoadingModal** + 자동 attempt + [Switch session…] (D5.4) |
| **(b) idle reconnect (page 유지), 자기 자신이 다시 holder 가능** | silent attemptReattach → 200 → fresh GET /layout → resume (D5.1) |
| **(c) idle reconnect (page 유지), 다른 webpage 가 takeover** | silent attemptReattach → 409 → modal → [Switch session…] / [Logout] (D5.1) |
| **(d) idle reconnect (page 유지), session [Delete]** | silent attemptReattach → 404 → modal → [Switch session…] (D5.1) |
| **(e) idle reconnect (page 유지), cookie 만료** | silent attemptReattach → 401 → /auth (D5.1) |
| **(f) idle reconnect (page 유지), BE down** | silent attemptReattach → 5xx/network → toast + retry → modal "재연결 실패" (D5.1) |
| **(g) 사용자가 명시 [Switch session…]** | sessionStore.clear() + sessionStorage hint 제거 + 사용자 명시 선택 (D5 + D7) |
| **(h) 사용자가 명시 [Logout]** | DELETE /attach + /auth/logout (D5) |

### D5.4 Initial entry attach recovery — blocking ReconnectLoadingModal (2026-05-16 amend ②, G50 follow-up)

#### 맥락

D5.1 의 silent path 는 *page 가 이미 mount 됐고 사용자가 작업 중인 상태* (Case II) 의 reactivate 케이스를 cover. 그러나 **사용자가 page 를 reload 또는 새 진입 시점 (AppPage onMount)** 에서 *이전 session 으로 자동 복귀를 시도하는 흐름* (Case I) 은 다른 UX 가 필요:

- 본 화면 (Canvas / Toolbar / LeftPanel 등) 이 *아직 mount 안 됨* — silent 가 자연 X. *진행 상태 + 선택 옵션* 명시 노출이 자연.
- 사용자 의도 "session 자동 복귀 시도 중인데, 명시 cancel 도 선택 가능" — blocking modal + [Switch session…] always-visible 옵션.

본 D5.4 는 D5 의 "page reload → 인증 후 dialog 흐름으로 다시 선택 (자동 재attach 안 함)" 정책을 *sessionStorage hint 가 있는 경우만 자동 attempt + 사용자 cancel 가능* 으로 amend.

#### 결정

- **sessionStorage schema**: key = `gtmux-last-active-session`, value = `string` (session name) 또는 absent. tab-scoped (다른 탭 영향 0). attach 성공 시 set, 명시 detach/logout/clear/`[Switch session…]` 시 remove, 그 session [Delete] 후 그 이름이었으면 remove.
- **Page entry 의사결정 tree**:
  ```
  AppPage onMount
    ├─ Auth gate (GET /api/sessions)
    │   ├─ 401 → /auth (현 흐름)
    │   └─ 200 [...] → 다음 단계
    │
    └─ sessionStorage 의 hint 검사
        ├─ 없음 → workspaceSwitcher.open() (D7/D8 흐름, 기존)
        └─ 있음 (= name) → ReconnectLoadingModal.open(name) + 자동 attemptReattach
  ```
- **ReconnectLoadingModal** 의 책임:
  - Full-screen backdrop + center card. ARIA dialog `aria-modal="true"`, focus trap, Esc 비활성, backdrop click 비활성.
  - 본 화면 (`<Canvas /> <Toolbar /> <LeftPanel /> <RightPanel />` 등) 은 *mount 안 됨* (Svelte `{#if !reconnectGate.blocking}` 로 게이트).
  - State 머신 4 state — `loading` / `in_use` (409) / `not_found` (404) / `unreachable` (5xx/network). 401 은 즉시 /auth redirect (modal state 안 거침).
  - Footer 의 [Switch session…] 은 *항상* 노출 — `loading` state 에서는 less-emphasis (text link 톤), 실패 state 에서는 primary.
  - 진입에 100ms grace — 빠른 200 케이스의 modal flicker 방지.
- **attemptReattach** 흐름 (D5.1 와 공통 utility, `lib/stores/sessionStore.svelte.ts`):
  ```
  POST /api/sessions/{name}/attach   (cookie 자동, AbortController signal 동봉)
    200          → sessionStorage hint 유지 (success 시점에 새로 set) + GET /layout
                   → ReconnectLoadingModal.close() → 본 화면 mount → resume
    409          → state = 'in_use' (modal transition)
    404          → state = 'not_found' (modal transition) + sessionStorage hint remove
    401          → /auth redirect
    5xx/network  → state = 'unreachable' + attempt 카운터 ++ — [Retry] / [Switch session…]
  ```
- **[Switch session…] 클릭** (사용자 명시 cancel — `loading` 포함 모든 state 에서):
  1. `AbortController.abort()` → in-flight `POST /attach` cancel (BE 도 idempotent)
  2. `sessionStorage.removeItem('gtmux-last-active-session')` — 다음 reload 도 dialog 흐름
  3. ReconnectLoadingModal.close()
  4. `workspaceSwitcher.open()` → SessionListModal (기존 D7/D8 흐름 — 상단 `[+ New session…]` + 하단 list)
  5. 사용자가 명시 선택 (새/기존)
- **[Retry] 클릭** (unreachable state 만): 새 AbortController 로 attempt 시작 → state = `loading`.

#### Case I vs Case II 정리

| | Case I (D5.4) | Case II (D5.1) |
|---|---|---|
| Trigger | AppPage onMount + sessionStorage hint | WS reopen + visibilitychange |
| 본 화면 | *mount 안 됨* (blocking) | mount 유지 |
| UX | Modal full-block + [Switch session…] always | Silent + mutation guard, 실패 시만 modal |
| 사용자 cancel | [Switch session…] always 노출 | 없음 (silent — 결과 분기만) |
| 공통 utility | `attemptReattach(name, signal)` (sessionStore method) | 동일 |
| 우선 구현 | **Phase 1** | Phase 2 |

#### 거절된 대안 (D5.4 시점)

- **R6.** sessionStorage 대신 localStorage — multi-tab 케이스 (탭 A=session-X, 탭 B=session-Y) 가 둘 다 reload 시 동일 hint 로 진입 → 마지막 attach 한 session 으로 둘 다 시도 → 한 쪽이 409. 사용자 의도 위반. 거절.
- **R7.** sessionStorage 대신 BE-side last-attach 캐시 (`GET /api/sessions/me/last-attach`) — BE 변경 + cookie ↔ session mapping 의 stale 처리 필요. 사용자 결정 "FE-only" 정합 위반. 거절.
- **R8.** Modal 없이 즉시 sessionListModal 표시 + 그 안에 progress hint — *진행 상태* 와 *명시 선택* 의 인지 부담 mix. 거절. 별 modal 이 명확.
- **R9.** [Switch session…] 을 `loading` state 에서는 hide, 결과 분기 후만 노출 — 사용자 요구 "옵션으로 새로 session 진입도 할 수 있도록" 정면 위반. 거절.
- **R10.** Modal 진입에 grace 없음 (즉시 표시) — 50ms 안에 200 으로 끝나는 빠른 케이스의 flicker. 100ms grace 추가. R10 거절.
- **R11.** Esc / backdrop click 활성 — 실수 close → 사용자가 다시 동일 modal 을 만남 (sessionStorage hint 미변경). 사용자 의도 cancel 은 [Switch session…] 의 명시 선택만 valid. R11 거절.

#### 결과

- 긍정: page entry 시점의 *첫 인상 UX* 명확화. 사용자가 *기다리고 있는 동안 무엇이 진행 중인지* + *명시 선택지* 동시 노출. takeover/삭제/실패 분기도 자연 흐름. sessionStorage hint 가 stale 케이스 (그 session 이 BE 에서 삭제) 도 attempt → 404 → modal 의 자연 처리.
- 부정: AppPage onMount 의 진입 흐름 amend (현 `+page.svelte` 의 auth-gate 로직 amend 필요). Modal 컴포넌트 신규.
- 후속:
  - `lib/chrome/ReconnectLoadingModal.svelte` 신규 (Phase 1)
  - `lib/stores/sessionStore.svelte.ts` 의 `attemptReattach(name, signal)` method 신규 (D5.1 와 공통)
  - `routes/+page.svelte` 의 onMount 흐름 amend (sessionStorage 검사 → modal gate)
  - sessionStorage hint write — `setActiveSession()` 안에서, remove — `clear()` / 명시 detach / logout / [Delete] 흐름 안에서
  - Plan-0008 implementation 계획서 신규 (Phase 1 = D5.4 / Phase 2 = D5.1)

##### D5.4 amend ② — `reconnectGate.cancel()` 의 tentative lock release (2026-05-18, 0071 §B-1)

사용자가 ReconnectModal 의 [Switch session…] 을 누른 시점에는 직전 `POST /attach` 가 이미 BE 의 flock + `session_locks_by_*[owner_key] = name` 을 잡았을 가능성이 있다. D5.4 의 `cancel()` 동작이 `AbortController.abort()` + `sessionStorage` hint clear + state reset 만 수행하면, BE 측 attach lock 은 다음 30s heartbeat timeout (ADR-0021 D6.2) 까지 잔존한다. 같은 owner_key 의 재시도는 D3 의 same-owner idempotent 분기로 자가 회복 가능하지만, *다른 webpage* 가 같은 session 진입 시도 시 409 conflict 가 그 30s 동안 유지된다.

본 amend 의 결정:

- Trigger: `reconnectGate.state === 'attaching'` 중 사용자 [Switch session…] 클릭.
- 동작: `AbortController.abort()` + `markIdle()` + `sessionStorageHint.clear()` 에 더해 **best-effort `detachSession(attemptName)` (fire-and-forget)**.
- 호출 패턴: `void detachSession(name).catch(...)` — `cancel()` signature 는 `(): void` 유지. caller (ReconnectModal button handler) 가 await 하지 않도록 보장 — modal 전환의 perception 지연 방지.
- 실패 정책: silent + `console.debug`. 사용자 toast 없음 — 진입 자체 포기 의도라 부수효과 정보가 무의미. (`WorkspaceSwitcher.cancelAttachConfirm` 의 5-step chain 은 *직전 active session 복귀* 라는 부수효과가 있어 8s warning toast 가 의미 있으나, 본 case 는 그 부수효과 자체가 없음.)
- Fallback: detach 호출이 실패해도 BE 의 30s heartbeat timeout 이 lock 회수 — 사용자 perception 의 지연만 발생, 영구화 위험 0.
- 정합: `WorkspaceSwitcher.cancelAttachConfirm` (D5.5.1) step 1 의 `DELETE /api/sessions/{pending}/attach` 와 동형 패턴 — *명시 cancel = tentative lock 즉시 release* 의 invariant 일관성.

본 amend 가 없으면 사용자가 reload + 자동 reattach 진행 중 [Switch session…] 을 누른 직후 같은 session 을 *다른 탭* 에서 열려고 하면 30s 까지 409 — 사용자 mental model 위반 (`내가 cancel 했는데도 session 이 잡혀있다`).

### D5.5 Attach confirm cancel — tentative attach 와 FE active 전환 시점 (2026-05-18 amend)

#### 맥락

`POST /api/sessions/{name}/attach` 는 BE attach lock 을 먼저 잡고 layout 의 terminal item 과 server terminal pool 을 분류한다. 이 응답이 `unmatched.length > 0` 이면 FE 는 `AttachConfirmModal` 을 띄워 사용자에게 fresh terminal spawn 여부를 묻는다.

이 시점은 **서버 lock 은 잡혔지만 FE workspace 는 아직 load 되지 않은 중간 상태**다. 따라서 이를 곧바로 `sessionStore.active` 로 반영하면 다음 모순이 생긴다.

- no-session 첫 진입인데 `sessionStore.active !== null` 이 되어 AuthDialog/SessionList 의 Cancel affordance 가 잘못 활성화된다.
- 사용자가 AttachConfirmModal 에서 Cancel 해도 session 이 실제 attach 된 것처럼 session list 에 표시될 수 있다.
- session switch 중 Cancel 의 의미가 "이전 workspace 유지" 인지 "선택 session 유지" 인지 불명확해진다.

#### 결정

- `confirm_required` 는 **tentative attach** 로 취급한다.
- `confirm_required` 분기에서는 FE active session 을 변경하지 않는다.
- FE active session 전환은 반드시 다음 절차가 성공한 뒤에만 수행한다.
  1. `POST /api/sessions/{name}/attach/confirm`
  2. `GET /api/sessions/{name}/layout`
  3. `sessionStore.setActiveSession({ name })`
  4. `sessionStore.loadLayout(layout)`
- AttachConfirmModal 의 Cancel 은 `DELETE /api/sessions/{name}/attach` 로 tentative lock 을 해제한다.
- Cancel 전 active session 이 없었다면 no-session 상태를 유지하고 session 선택 흐름으로 돌아간다. 이 상태에서는 session 없이 main page 로 빠지는 Cancel affordance 를 노출하지 않는다.
- Cancel 전 active session 이 있었다면, Cancel 은 "session switch 취소" 이므로 이전 session 으로 재attach 를 시도한다. 이전 session 재attach 가 실패하면 FE 는 active state 를 clear 하고 사용자에게 복구 실패 toast 를 표시한다.
- FE HTTP client 의 detach 계약은 `DELETE /api/sessions/{name}/attach` 이다. `POST /api/sessions/{name}/detach` 같은 별도 endpoint 는 존재하지 않는다.

##### D5.5.1 Cancel 의 5-step fallback chain (2026-05-18 amend — 코드 SoT 정합)

`cancelAttachConfirm` (`WorkspaceSwitcher.svelte:215-250`) 의 정확한 5 step:

| Step | 조건 | 동작 |
|---|---|---|
| 1 | `pendingAttachHasTentativeLock && pending !== active.name` | `DELETE /api/sessions/{pending}/attach` — tentative lock release |
| 2 | `previous !== null && previous !== pending` | `restorePreviousSession(previous)` = `POST /attach` + (성공 시) `setActiveSession + loadLayout` |
| 3 | step 2 가 `confirm_required` 반환 | `goAttachConfirm(previous, summary)` 재진입 — 이때 `pendingAttachPreviousSession = null` 이미 clear 됨이라 한 번만 재귀 가능 (재귀 자체는 stage 머신의 self-loop) |
| 4 | step 2 가 예외 throw (UnauthorizedError 외) | `sessionStore.clear()` + 8s warning toast `"Attach cancelled, but previous session could not be restored: …"` |
| 5 | 항상 마지막 | `workspaceSwitcher.goList()` — SessionListModal 복귀 (`listCloseTarget` 가 `'choice'` 면 AuthDialog 회귀, `'closed'` 면 closed) |

##### D5.5.2 AttachConfirmModal 의 3 entry source (2026-05-18 amend)

AttachConfirmModal 은 3 곳에서 진입 가능 — entry 별 cancel chain 동작 차이:

| Entry source | `pendingPrevious` | `hasTentativeLock` | Cancel chain |
|---|---|---|---|
| `tryAttach` (사용자 명시 attach via WorkspaceSwitcher) | 직전 active session | `true` | **5-step chain 전체** (D5.5.1) |
| `silentReattach` Phase 2 escalation (`+page.svelte:173-174`) | `null` | `false` | step 1 skip + step 2 의 `previous !== null` false → clear() + goList |
| `reconnectGate` Phase 1 escalation (`reconnectGate.svelte.ts:#run`) | `null` | `false` | 같음 (단순 cancel) |

→ 본 분기는 silent 흐름이 modal 까지 escalate 됐을 때 *사용자 cancel = 이전으로 돌아갈 곳 없음* (silent 진입은 이미 active session 에서 시작했기 때문) 이라 fallback 만으로 충분. tryAttach 만 *진짜 5-step chain* 필요.

#### 결과

- no-session guard 와 match-or-spawn confirm UX 가 충돌하지 않는다.
- 사용자가 spawn 을 취소하면 서버 attach lock 이 남지 않아 session list 의 active/in-use 표시가 사용자 의도와 일치한다.
- session switch 중 confirm_required 를 취소해도 이전 workspace 유지 의미가 보존된다.

### D5.6 Webpage identity — auth cookie 와 attach owner 분리 (2026-05-18 amend)

#### 맥락

D3 의 단위는 **Webpage : Session = 1:1** 이다. 그러나 auth cookie 는 브라우저 origin 단위라 같은 브라우저의 여러 탭이 공유한다. attach lock / WS session routing 을 cookie 값만으로 keying 하면 다음 모순이 생긴다.

- 같은 인증 사용자의 두 탭이 같은 cookie 로 동일 session 에 attach 해도 BE 는 "같은 holder" 로 오인할 수 있다.
- server 재시작 후 열린 채 남아있던 기존 session page 가 server-side active 로 잡히지 않은 동안, 새 탭이 같은 session 을 열 수 있다.
- 이후 기존 page 가 다시 인증 cookie 를 공유받으면 두 page 가 같은 session layout 을 동시에 mutate 할 수 있어 ETag 충돌과 사용자 인지 불가능한 layout overwrite 가 발생한다.

#### 결정

- Auth cookie 는 **사용자 인증** 단위로만 사용한다.
- Attach owner / WS routing owner 는 **`auth cookie + tab-scoped webpage id`** 조합으로 정의한다.
- FE 는 `sessionStorage` 에 `gtmux_webpage_id` 를 저장한다. 같은 탭의 reload 는 같은 webpage id 를 유지하고, 다른 탭은 다른 webpage id 를 가진다.
- FE 는 다음 경로에 webpage id 를 전송한다.
  - HTTP: `X-Gtmux-Webpage-Id` header
  - WS: `/ws?webpage_id=<id>` query
- BE 는 session lock reverse index 와 WS hub session table 을 cookie 단독이 아니라 owner key 로 keying 한다.
- 같은 owner key 의 same-session reattach 는 D3/D5.1 의 idempotent 200 을 유지한다.
- 같은 auth cookie 라도 다른 webpage id 가 동일 session 에 attach 하면 D4 no-takeover 정책에 따라 409 conflict 를 반환한다.
- `DELETE /api/sessions/{name}/attach` 는 owner-scoped 이다. 다른 webpage 의 detach 는 idempotent 200 일 수 있지만 현재 holder 의 lock 을 해제하지 않는다.
- Layout-changing HTTP mutation (`PUT /layout`, `DELETE /items`, `POST /terminals`, `POST /attach/confirm`) 은 owner key 가 해당 session attach 를 보유할 때만 허용한다. Ghost page 가 server restart 후 attach 복구에 실패한 상태로 mutation 을 시도하면 403 `not_attached` 로 막는다.
- `GET /api/sessions` 의 `active` 는 **어느 Webpage 에서든 이미 열려 있어 session picker 에서 선택 불가** 라는 UI-facing flag 이다. 현재 Webpage 가 보유한 session 도 이미 열린 session 이므로 `active:true` 로 내려야 한다. 즉 list 의 `active` 는 owner-scoped 권한 판정이 아니라 raw lock 존재 여부에 가깝고, owner-scoped 구분은 `POST /attach`, `DELETE /attach`, layout-changing mutation, WS routing 에서만 사용한다.

#### 결과

- cookie 공유 때문에 같은 session 이 두 Webpage 에 동시에 열리는 경로를 차단한다.
- server restart 후 남아있는 page 는 WS reconnect/silent reattach 로 다시 owner 가 되거나, 이미 다른 page 가 owner 면 attach conflict / mutation guard 로 막힌다.
- 인증 상태와 session ownership 이 분리되어 "한쪽에서 인증하면 다른 고스트 페이지도 session owner 로 부활" 하는 모순을 줄인다.
- session list 는 Webpage owner 와 무관하게 열려 있는 모든 session 을 disabled row 로 표시한다. 자기 자신 session 을 다시 선택하는 흐름도 session picker 에서는 막는다.

##### D5.6 amend ② — code symbol naming = owner_key 통일 (2026-05-18, 0071 §C-1)

D5.6 본문이 owner_key (`auth_cookie + 0x1f + webpage_id`) 를 attach lock /
heartbeat / WS routing 의 통일 식별자로 잠궜지만, 코드 symbol 은 여전히 옛
`cookie` 명을 사용해 *값과 이름이 어긋난* 상태였다. 0071 감사가 false-positive
6 건 양산한 직접 원인. 본 amend 가 명명을 일관화한다:

| 옛 이름 | 새 이름 | 위치 |
|---|---|---|
| `session_locks_by_cookie` | `session_locks_by_owner` | `http-api/src/lib.rs` `AppState` field |
| `release_lock_for_cookie` | `release_lock_for_owner` | `http-api/src/lib.rs` `AppState` method |
| `refresh_lease_for_cookie` | `refresh_lease_for_owner` | 같음 |
| `Hub::set_session_for_cookie` | `Hub::set_session_for_owner` | `ws-server/src/hub.rs` |
| `Hub::clear_session_for_cookie` | `Hub::clear_session_for_owner` | 같음 |
| `Hub::session_for_cookie` | `Hub::session_for_owner` | 같음 |
| `SessionChangeEvent.cookie` | `SessionChangeEvent.owner_key` | event payload field |
| `handle_socket(... cookie_value ...)` | `handle_socket(... owner_key ...)` | `ws-server/src/lib.rs` |
| `emit_heartbeat(... cookie ...)` | `emit_heartbeat(... owner_key ...)` | 같음 |
| `ws_handler` 의 `cookie_value` (실제 cookie) | `auth_cookie` | 같음 — 진짜 cookie 영역은 명시 분리 |

`CookieValidator::validate(cookie_value)` 와 auth-side 의
`extract_cookie_value` 는 *진짜 cookie* 를 다루는 site 이므로 cookie 명을 유지.
naming 영역만의 amend — wire 동작은 D5.6 본문 그대로, behavior change 0.

### D6. Cross-server session lock (동일 workspace path)

여러 Server 인스턴스가 같은 `workspace_path` 를 가리킬 수 있다. 이 경우 *session record pool 공유*. 그러나 한 session 의 active webpage 단위는 그대로 1 유지 — server-cross active 충돌 방지.

#### D6.1 메커니즘: OS file lock + lease 내용 hybrid (2026-05-15 G18 grilling)

**OS-level lock (primary)**: `flock(2)` (BSD/Linux native, Rust `fs2::FileExt::try_lock_exclusive`) — *server crash 시 kernel 이 자동 해제*. 따라서 server SIGKILL / panic 케이스에서도 stale lock 0.

**File 내용 (진단 + UI 표시)**: lock 보유 후 JSON 1줄 write — `{ "server_id": "<uuid>", "pid": <i32>, "ws_conn_id": "<uuid>", "lease_until": "<ISO8601>" }`.
- `server_id` = server boot 시 1회 생성 UUID. PID 재사용 우려 해소.
- `pid` = 진단용 표시 ("in use by server-pid 12345").
- `ws_conn_id` = 어느 WS connection 이 보유 중인지. 같은 server 안 multi-tab 진단.
- `lease_until` = 다음 *예상* 만료 시각. 다른 server modal 의 *expected expiry hint*. 실제 release 는 lease 시각 보다 flock 의 OS-level 보장이 우선.

**Lock file 위치**: `${workspace_path}/.locks/<session-name>.lock`. 파일 영속 안 함 — release 시 `LOCK_UN + unlink`. workspace dir 안 위치 = workspace 공유 = lock 공유 의 자연 정합.

#### D6.2 Lease 갱신 주기 (ADR-0021 D6 정합)

| 항목 | 값 |
|---|---|
| Client → Server WS ping 주기 | 15s (ADR-0021 D6) |
| Server 의 lease 갱신 시점 | 매 ping 수신 시 lock file 의 `lease_until` 을 `now + 30s` 로 재기록 |
| Lease timeout | 30s — 마지막 ping 후 30s heartbeat timeout 시 server 가 명시 release |
| WS close 정상 (탭 닫기, logout) | 즉시 `LOCK_UN + unlink` |
| Server SIGTERM/Ctrl-C | Shutdown hook 이 모든 보유 lock 일괄 `LOCK_UN + unlink` |
| Server SIGKILL / panic | OS kernel 이 flock 자동 해제, file 은 남음. 다음 acquirer 가 LOCK_NB 성공 시 *내용 덮어쓰기* (이전 stale 폐기) |

#### D6.3 Acquire / Peek 프로토콜

**Acquire (다른 webpage 가 session attach 시도)**:
```
fd = open(lock_path, O_RDWR | O_CREAT, 0600)
match flock(fd, LOCK_EX | LOCK_NB):
    Err(EWOULDBLOCK) →
        # 다른 server / WS 가 보유 중
        peek 로직으로 holder 정보 표시
    Ok →
        ftruncate(fd, 0)
        write(fd, JSON{server_id, pid, ws_conn_id, lease_until})
        # 이 시점부터 active lock 보유
```

**Peek (modal 의 row 상태 판정)**:
```
fd = open(lock_path, O_RDONLY)
match flock(fd, LOCK_SH | LOCK_NB):
    Err(EWOULDBLOCK) →
        # 누군가 EX 보유 = in use
        read(fd) → JSON parse → server_id/pid 표시
        # write 중 race 로 빈 file 인 경우 "acquiring..." 표시 (다음 poll 에서 갱신)
    Ok →
        # 사실 free → stale file → unlink 후 acquire 가능
        flock(fd, LOCK_UN)
        unlink(lock_path)
```

#### D6.4 Modal 의 lock state 갱신 — 1s polling

- Session list modal open 동안 **1s 주기로 각 session 의 lock peek**.
- Modal close 시 polling 중단.
- 다른 server / webpage 가 lock release → ~1s 내에 row 가 자동 enable.
- 비용: `stat() + flock peek` = ms 단위, single-user 환경의 active session 수가 한 자릿수, modal lifespan 짧음 → 충분.
- FS-level notify (inotify/FSEvents) / server WS push 는 MVP 비범위.

#### D6.5 Acquire failure UX

- D9 modal 의 row 가 *비활성 (disabled, 50% opacity)* + "in use" badge + tooltip ("다른 webpage 에서 사용 중 — 그 webpage close 시 자동 활성화").
- Acquire 시도 (row click) → 비활성이라 client-side 차단.
- *자동 retry 없음* — sub-second polling 이 row state 를 곧 갱신하므로 사용자가 자연스레 click 가능 시점을 인지.
- Takeover 경로 없음 (D4 와 정합).

#### D6.6 같은 server 안의 동시 attach 시도

- 같은 server 안 두 webpage 가 거의 동시에 같은 session attach 요청 시 *server-internal mutex* 로 직렬화 (attach handler 의 critical section 안에서 flock 시도).
- Cross-server 의 race 는 flock 의 OS-level 보장.

#### D6.7 대안 검토 + MVP 범위

- 같은 workspace 공유 자체 금지 (file-level exclusive lock) — 거부. 다중 server 시나리오 자체가 사용자 명시 use case.
- 그저 race condition 무시 — 거부. 사용자 인지 부담 발생.
- Application-level lease only (kernel lock 안 함) — 거부 (G18.1 grilling). 시계 skew / NTP step / process pause 에 취약.
- fs notify 기반 zero-poll — 거부 (G18.3 grilling). MVP overkill.

본 ADR 의 D6 은 **MVP 의 best-effort 분산 협조**. NFS / SMB 의 flock semantics 차이는 비범위 (사용자 단일 머신 가정). 완전한 distributed lock 은 P1+.

### D7. 새 session 추가 UI (이름 입력 modal)

`[새 session 추가]` 클릭 후 흐름:

```
modal: Session name: [______]
       [Cancel]    [Create]
       ↓ "build-monitor" 입력
       ↓ Create
file 생성: workspace/build-monitor.json (빈 layout)
webpage 활성을 그 session 으로 attach
```

- 이름 unique constraint: workspace 안 unique. 중복 입력 시 reject + 인라인 에러.
- 이름 규칙: ASCII letter/digit/dash/underscore, 1~64자 (path traversal 차단). 정규식: `^[A-Za-z0-9_-]{1,64}$`.
- Default name 자동 부여: **없음** — 사용자가 이름 의도 명시. 이유: 사용자 인용 *"무분별 생성 방지"*.

대안:
- Auto default name (untitled-1, untitled-2) — 거부. 사용자 명시.
- Anonymous start + [Save As] 까지 file 없음 — 거부. WS close 시 작업 무명 상실 위험.

### D8. 인증 후 dialog (새 / 기존 선택)

```
Auth page (token or password) 
  ↓ 인증 통과
Cookie 발행 (lifecycle, ADR-0020)
  ↓
Dialog: [ 새 session 추가 ] [ 기존 session 연동 ]
   ↓                              ↓
   D7 modal                  D9 session list modal
   ↓                              ↓
   빈 layout 으로 attach          선택 → snapshot shift (simple attach)
```

Dialog 자체는 *우회 불가* — 인증 후 attach 까지 항상 거침. URL 의 `?session=<name>` query 같은 deep link 는 P1+ (보안/race 표면 추가 후 검토).

### D9. Session 목록 UI (드롭다운 + modal)

두 진입점 모두 같은 modal 사용:
- **메뉴 드롭다운**: Titlebar 의 SessionMenu (≡) → "Switch session..." → modal
- **툴바 우측 드롭다운**: Toolbar 의 우측 활성 session 라벨 (현재 session 명 표시) 클릭 → modal
- **인증 후 dialog 의 [기존 session 연동]**: 같은 modal

Modal 구조:

```
┌─ Sessions ──────────────────────┐
│  Available                       │
│  ┃ demo-build                    │
│  ┃ experiment-1                  │
│  ┃ archived                      │
│                                  │
│  In use                          │
│  · monitoring (disabled)         │
│                                  │
│   [ + New session ] [ Cancel ]   │
└──────────────────────────────────┘
```

활성 session = 50% opacity + "in use" badge + click disabled + tooltip *"다른 webpage 에서 사용 중 — 그 webpage close 시 자동 활성화"*.

### D10. Session delete cascade

Session 의 [Delete] 액션 시:
- Session file record 즉시 unlink (atomic rename + remove via storage layer, ADR-0006 와 정합)
- Confirm modal: "Delete session '<name>'? (Terminal 들은 server-pool 에 남음)"
- **Terminal 은 cascade kill 안 함**. 다른 session 의 panel 이 그 terminal 을 attach 중일 수 있으므로 침해 방지 — 사용자가 명시 [Kill terminal] 또는 server shutdown 으로 정리.
- Delete 후 현재 attached webpage 가 그 session 이었다면 → dialog 로 되돌아감.

### D10.1 UI entry points (2026-05-17 amend, G51)

D10 cascade 정책의 trigger 는 FE 두 entry point 에서만 가능:

**(a) SessionListModal — `Available` row 의 hover-kebab**
- Row 우측 [⋯] (hover 시 노출) → small popover [Delete session…] 1 액션.
- 가시성 규칙:
  - *In use* 섹션 row (다른 webpage 가 attach 중) → kebab 표시 X. 다른 webpage 의 작업 중 file unlink = 침해.
  - *Available* 안에서도 본 webpage 의 현 attached row (= `sessionStore.activeName` 일치) → kebab 표시 X. 본 entry 는 (b) SessionMenu 가 own — 둘 다 노출하면 confirm 흐름 분기 혼선.
- 승인 후: `deleteSession(name)` (HTTP) → 200 ok → SessionListModal 의 1s polling (D6.4) 이 다음 tick row 제거. *별도 즉시 refresh 트리거 없음* — race 단순화. 404 (race — 다른 webpage 가 이미 삭제) 도 동일 처리 (다음 poll 이 row 자연 제거).

**(b) SessionMenu (Titlebar `≡` 드롭다운) — "Delete current session…"**
- *현 attached session 만* 대상 (= `sessionStore.activeName`). Logout 아래 / Shutdown server… 위 위치.
- 승인 후 순서:
  1. `deleteSession(activeName)` → 200 ok
  2. `sessionStore.clear()` + `reconnectGate.cancel()` + `sessionStorageHint.clear()` — D5.4 의 명시 cancel 흐름과 정합 (다음 reload 도 dialog 흐름)
  3. `workspaceSwitcher.open()` — dialog 회귀 (D10 의 "현 attached 였으면 dialog 회귀" 정합)

**확장 가능성 (P2 후보, 본 amend 비범위)**:
- Bulk delete (multi-select on SessionListModal) — 현 single-action 으로 충분.
- Workspace settings modal 의 sessions table (G34 후속) — table 위 [Delete] 컬럼.

**비채택 대안**:
- Cmd+Click on row — discover 불가 + 우발 클릭 위험.
- Row right-click 컨텍스트 메뉴 — modal 안 right-click 컨벤션 비표준 (Figma/Linear/Notion 모두 hover-kebab 표준).
- LayerTreeView 진입 — LayerTreeView 는 한 session 의 item tree owner (ADR-0024). Session 자체 관리는 본 ADR scope.

**Confirm 정책 (D10 정합)**: copy = "Delete session '<name>'? (Terminal 들은 server-pool 에 남음)". 우측 destructive button = `--color-danger`, 좌측 Cancel = ghost.

**BE 변경 0** — `DELETE /api/sessions/<name>` (BE-NEW-2) 이미 ship + `deleteSession()` wrapper (`codebase/frontend/src/lib/http/sessions.ts:115`) 기존.

### D11. Boot binding immutable

- Workspace path, port 는 boot 시 1회 확정. Runtime 변경 불가.
- Config 의 변경은 다음 server 부팅에서 반영.
- CLI flag (`--workspace`, `--port`) 가 있으면 config 보다 우선.

## 어휘 매트릭스 (CONTEXT.md 정합 요약)

| 어휘 | 정의 (CONTEXT.md 의 정의 압축) |
|---|---|
| Server | process, 1 port owner, 1 workspace dir 바인딩 |
| Workspace | server 와 1:1, storage dir |
| Session | workspace 안 named file record |
| Webpage | WS 연결, session 의 편집 채널, 0 or 1 attach |
| Terminal | server-pool, multi-session 공유 가능 (ADR-0021) |
| Canvas | 한 session 의 무한 작업 공간 |
| Canvas Item | canvas 위 시각 객체 (terminal Panel + non-terminal, ADR-0018) |
| Panel | type:"terminal" 인 Canvas Item |
| Group | session 안 item 의 묶음 (ADR-0010) |

## 대안 검토

### A1. ADR-0007 보존 (single-session 유지)
**거부.** 사용자 요구 (workspace 별 분리, multi-tab 동시 운용) 를 자연 표현 불가.

### A2. Session multi-attach 허용 (옛 MT-3 server-wide mirror 유지)
**거부.** Q8 grilling 에서 *다중 탭 mirror* 보다 *각 탭 독립 layout* 이 사용자 의도. multi-monitor mirror 욕구는 *Terminal 의 multi-attach (ADR-0021)* 로 별 layer 에서 해결.

### A3. Session = ephemeral (file 영속 안 함)
**거부.** workspace 의 의미 자체가 약화됨. file 영속이 핵심 가치.

### A4. Session storage = browser localStorage
**거부.** Browser scope 라 multi-device / backup / 외부 도구 접근성 0. server-side file 이 정합.

### A5. Multi-server lock 안 함 (race accept)
**거부.** 사용자 인지 부담 + UX 모호 (활성 session 의 충돌). D6 의 file-lock 으로 안정화.

### A6. Auto-default session name
**거부.** 사용자 명시 "무분별 생성 방지".

## 영향

### Code
- **Backend**:
  - 새 `WorkspaceManager` (storage path 관리 + session enumeration + cross-server lock)
  - 새 `SessionRecord` (file CRUD, schema v2, ADR-0018)
  - `ws-server` 의 connection state — session 단위 분리 (Auto-mount, M/I/Viewport 의 session-scoped 화)
  - HTTP API 신규: `GET /api/sessions`, `POST /api/sessions`, `DELETE /api/sessions/<name>`, `PUT /api/sessions/<name>/layout`
  - HTTP API amend: `GET /api/layout` → `GET /api/sessions/<name>/layout` (session-scoped path)
- **Frontend**:
  - 새 Auth page + cookie lifecycle (ADR-0020)
  - 새 Dialog after auth (새 / 기존 선택)
  - 새 Session list modal (D9)
  - 새 Session menu (Titlebar) + 우측 활성 session 드롭다운 (Toolbar)
  - 기존 layout store → session-scoped 화 (활성 session 의 layout 만 sync)

### ADR
- ADR-0007 supersede 명시 (header amend 필요)
- ADR-0002 D3 amend (MT-3 → 2-layer, 본 ADR D3/D5 가 새 진실)
- ADR-0006 D15 신규 amend (panels[] strip 폐기, schema v2 hard cutover)
- ADR-0015 ADR-0021 로 amend (auto-mount trigger session 전용)

### Docs
- CONTEXT.md 큰 amend (이미 완료, §2026-05-15 amend (multi-session pivot))
- `docs/ssot/canvas-layout-schema.md` v2 갱신
- plan-0007 작성 (Stage 순서 + FE/BE parallel + integration gate, 본 ADR 의 D1~D11 을 reference)

### 보안
- Workspace path 의 검증 (절대 경로, traversal 차단)
- Session name 정규식 (D7)
- Cross-server lock 의 stale lease (D6)
- Cookie 정책 (HttpOnly Secure SameSite=Strict, ADR-0020)
- Asset path traversal (P2+ 의 image/document item, ADR-0018 후속)

## 변경 이력

- 2026-05-15: 초안 + Accepted. plan 0006 의 multi-session pivot grilling 17 결정 중 Q4 / Q5 / Q6 / Q8 / Q11 / Q13 / Q14 / Q15 / Q16 합본. ADR-0007 supersede 명시.
- 2026-05-15 (G18 grilling): D6 sub-points 추가 (D6.1 flock+lease hybrid, D6.2 15s ping / 30s lease, D6.3 acquire/peek 프로토콜, D6.4 modal 1s polling, D6.5 acquire failure UX, D6.6 same-server mutex, D6.7 대안 정리).
- 2026-05-16 (G50 idle reconnect grilling): D5 amend — *page reload* 와 *page-유지 idle reconnect* 케이스 분리. D5.1 Idle reconnect silent attach recovery 신규 (trigger = WS reopen + visibilitychange 합집합, 결과 분기 200/409/404/401/5xx, BE 변경 0). D5.2 Mutation guard 정의 (모든 outgoing write 의 in-flight 점검). D5.3 결정 매트릭스 신규 (8 시나리오 분기 표). 배경: 사용자 시나리오 "idle 30s+ 후 [New Terminal] → `Terminal create failed: not the lock holder of \"…\" — call attach first`" 의 UX 끊김 + web-1 idle/web-2 takeover/web-1 복귀 의 takeover 분기 처리.
- 2026-05-16 (G50 follow-up — initial entry UX grilling): D5 amend (sessionStorage hint 흐름 추가) + D5.3 매트릭스 amend (a → a-1 / a-2 분리) + **D5.4 Initial entry attach recovery 신규** — sessionStorage hint 가 있으면 AppPage onMount 시 blocking `ReconnectLoadingModal` + 자동 attempt + [Switch session…] always-visible. 본 화면 mount 차단. Case I (blocking) vs Case II (silent) 분리. 우선 구현 Phase 1 = D5.4. plan-0008 implementation 계획서 짝.
- 2026-05-16 (0045 P0 후속 — reconnectGate 5-state amend): D5.4 의 reconnectGate state 머신을 4 → 5 단계로 세분화 — `booting/attaching/hydrating/in_use/not_found/unreachable/ready` (+ `idle`). 의미: **booting** = auth gate / hint 검사 전 (빈 Canvas mount 금지), **attaching** = POST /attach 진행 중, **hydrating** = 200 응답 후 GET /layout + loadLayout 진행 중, **ready** = hydrate 완료 후 본 화면 mount 허용 (기존 `success` rename), **idle** = hint 없거나 사용자 cancel 후 (workspaceSwitcher 가 mount 결정), 그 외 failed (`in_use`/`not_found`/`unreachable`) = ReconnectModal 만 노출. **canMountApp = ready || idle** — 빈/partial Canvas mount 차단의 정합 표현. **modalState** derived 가 attaching/hydrating 을 'loading' 으로 normalize 해 ReconnectModal 의 mode prop 4-mode 유지. **markReady()** 신규 메서드 (markSuccess() 는 호환 alias). 본 amend 는 0045 분석의 P0-A (flowNodes identity churn) + P0-B (viewport one-shot) 와 paired — partial mount 의 effect-depth loop 가설을 state-level 에서 추가 방어. plan-0008 §4.6 의 canMountApp 정의도 본 새 enumeration 으로 정합. 코드 land: `lib/stores/reconnectGate.svelte.ts` + `routes/+page.svelte` boot screen 분기 (attaching/hydrating 진행 상태 가시화) + `lib/chrome/WorkspaceSwitcher.svelte` 의 markSuccess → markReady alias 호출. **D3 의 same-cookie idempotent contract drift** (sessions.rs:330 attach_handler 가 코멘트 약속과 달리 409 반환) 별도 BE work package `0046-be-attach-handler-idempotent.md` 로 격리 발주 — 0045 의 refresh race + Phase 2 silentReattach 의 *모든* 호출 회귀 근본 원인.
- 2026-05-16 (amend ③ — 0046 BE land): D3 의 same-cookie idempotent contract 를 코드로 정합. `attach_handler` (sessions.rs:330) 의 line 396 직전에 cookie ownership 분기 + `reuse_existing_attach_response` 헬퍼 추가. 같은 cookie 의 same-name reattach 는 200 idempotent (기존 lock 유지 + classify_layout_terminals 만 재계산), 다른 cookie 의 takeover 는 D4 그대로 409. 테스트 정합: 신규 `attach_idempotent_for_same_cookie_same_session` + `attach_409_when_held_by_different_cookie`, 기존 `attach_409_when_already_held_same_server` 의 의미를 두 테스트로 분리, `attach_same_name_same_cookie_is_idempotent_409` → `_200` rename + 200 assertion, `release_lock_for_cookie_drops_the_attach` 의 second-attach 를 다른 cookie 로 변경. D5.1 §148 의 "BE 변경 0" claim 도 "BE 변경 ✓" 으로 정합 (코멘트와 코드 정합).
- 2026-05-17 (G51 session delete UI entry): **D10.1 신규** — D10 cascade 정책의 FE entry point 2 곳 명시. (a) SessionListModal 의 `Available` row 우측 hover-kebab [⋯] → [Delete session…] (가시성: *In use* row + 본 webpage 의 현 active row 는 kebab 차단), (b) SessionMenu 의 "Delete current session…" item (Logout 아래 / Shutdown 위 — 현 attached session 만 대상). 승인 후 (a) = 1s polling (D6.4) 의 다음 tick row 제거 / (b) = `sessionStore.clear()` + `reconnectGate.cancel()` + `sessionStorageHint.clear()` + `workspaceSwitcher.open()` (D5.4 cancel 흐름 + D10 의 "현 attached 였으면 dialog 회귀" 정합). 비채택 대안 (Cmd+Click / row right-click 컨텍스트 메뉴 / LayerTreeView 진입) 사유 명시. Confirm copy = D10 의 "Delete session '<name>'? (Terminal 들은 server-pool 에 남음)" 그대로 + destructive button (`--color-danger`). **BE 변경 0** — `DELETE /api/sessions/<name>` (BE-NEW-2) + `deleteSession()` (`lib/http/sessions.ts:115`) 기존. plan-0007 §14.12 FE-NEW-1 body amend + handover-v3 §5 P1 매트릭스 + §6 Stage 7 §9 정합.
- 2026-05-18 (attach confirm cancel amend): **D5.5 신규** — `confirm_required` 는 FE active session 이 아니라 tentative attach 로 정의. Confirm 성공 + layout fetch 후에만 `sessionStore.setActiveSession` 허용. Cancel 은 `DELETE /api/sessions/{name}/attach` 로 tentative lock 해제 후 no-session 상태 유지 또는 이전 active session 재attach. 배경: no-session 첫 attach confirm cancel 시 FE detach client 가 잘못된 `POST /api/sessions/{name}/detach` 를 호출해 405, 선택 session 이 attached 로 남던 회귀. 관련 리포트: `docs/reports/0069-session-attach-confirm-cancel-recovery.md`.
- 2026-05-18 (webpage identity amend): **D5.6 신규** — auth cookie 와 attach owner 분리. FE tab-scoped `gtmux_webpage_id` 를 HTTP `X-Gtmux-Webpage-Id` / WS query 로 전송하고, BE 는 `cookie + webpage_id` 를 session lock / hub routing key 로 사용. 같은 cookie 의 다른 탭은 다른 Webpage 이므로 동일 session attach 시 409. `DELETE /attach` 와 layout-changing mutation 도 owner-scoped 으로 보강. 후속 보완: `GET /api/sessions.active` 는 "현재 Webpage 와 다른 owner 가 attach 중" 인 conflict flag 로 정의 — 자기 Webpage 가 보유한 session 은 `active:false`, 다른 Webpage 가 보유한 session 은 `active:true`.
- 2026-05-18 (reconnect cancel lock leak fix, 0071 §B-1): **D5.4 amend ② 신규** — `reconnectGate.cancel()` 에 fire-and-forget `detachSession(attemptName)` 추가. `WorkspaceSwitcher.cancelAttachConfirm` (D5.5.1 step 1) 과 동형 — 사용자 명시 cancel = tentative attach lock 의 즉시 release. 실패는 silent + BE 30s heartbeat fallback. 본 amend 가 없으면 ReconnectModal [Switch session…] 직후 다른 webpage 의 같은-session 진입이 30s 까지 409. 관련 보고: `docs/reports/0071-session-terminal-panel-lifecycle-audit.md` §B-1, `0073-fe-handover-from-0071-audit.md` §B.
- 2026-05-18 (코드 SoT 정합 amend — state-machines.md §7.1 의 ADR amend 후보 land):
  - **D5.1 amend ②** — Pre-condition 을 *single (`sessionStore.active !== null`)* → *6 precondition AND-gate* (visibility/canMountApp/active/!reattachInProgress/isIdle + SSR guard) 로 정밀화. `attemptReattach` 흐름의 `200 + unmatched>0` 분기 신규 — **silent → AttachConfirmModal escalation** (2026-05-17 회귀 fix, `page.svelte:169-176`). Trigger source 코드 경로 (dispatcher.svelte.ts:541-559 / +page.svelte:204-208) 명시.
  - **D5.2 amend (D5.2.1 신규)** — `#silentReattachPromise` in-flight singleton 동작 명시. 두 번째 호출은 같은 promise 반환 (fetch 1번). `ensureMutationOk` helper 가 그 promise await + 결과 검사. AbortController 동작 (silent 는 cancel 안 됨, D5.4 reconnectGate 만 cancel) 정합.
  - **D5.5 amend (D5.5.1 + D5.5.2 신규)** — `cancelAttachConfirm` 의 5-step fallback chain 정밀 명시 (tentative detach → previous restore → recursive confirm_required → failure fallback → goList). 또 AttachConfirmModal 의 3 entry source (tryAttach / silentReattach Phase 2 / reconnectGate Phase 1) 별 cancel chain 동작 차이 — *tryAttach 만 5-step 전체* 발동.
  - 짝: `docs/ssot/state-machines.md` §3.2 / §3.2.1 / §3.4 / §3.4.1 / §3.4.2 / §4.4 / §4.4.1 / §5.1.1 / §6.3 / §7.1 + ADR-0020 D9.1 신규 + 코드 cross-link (sessionStore.ts:519-552 / 572-573 / 822, WorkspaceSwitcher.svelte:86-250, +page.svelte:148-176, dispatcher.svelte.ts:541-559).
- 2026-05-18 (D5.6 amend ② — code symbol naming, 0071 §C-1): owner_key 가 attach lock / heartbeat / WS routing 의 통일 식별자임을 코드 symbol 명에 반영. 8 anchor mechanical rename (`session_locks_by_cookie` → `session_locks_by_owner`, `release_lock_for_cookie` / `refresh_lease_for_cookie` → `_for_owner`, `Hub::*_for_cookie` → `*_for_owner`, `SessionChangeEvent.cookie` → `owner_key`, WS `handle_socket` / `emit_heartbeat` 의 `cookie_value` 파라미터 → `owner_key`, `ws_handler` 상단의 *진짜 cookie* 변수는 `auth_cookie` 로 분리). behavior change 0 — D5.6 wire 동작 그대로. 본 감사 (0071 §A) 가 false-positive 6 건 양산한 직접 원인 차단. `CookieValidator::validate` / `extract_cookie_value` 같은 *진짜 cookie* site 는 유지. 짝: `docs/reports/0072-be-handover-from-0071-audit.md` §B BE-A.
