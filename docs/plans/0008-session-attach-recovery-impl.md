# Plan 0008 — Session attach recovery 구현 계획

- 일자: 2026-05-16
- 작성자: agent (system-architect role) — 사용자 grilling G50 + follow-up
- 종류: **implementation plan + UI/UX 디자인 + 우선순위**
- 정본 ADR: **ADR-0019 D5 / D5.1 / D5.2 / D5.3 / D5.4** (정책 결정의 SoT — plan 은 *어떻게 구현* 만; D5.4 의 reconnectGate 8-state 머신 + `markReady`/`markIdle`/`modalState` 는 ADR-0019 변경 이력 의 *2026-05-16 (0045 P0 후속)* entry 가 정본)
- 정본 report: **docs/reports/0042-session-attach-recovery.md** (Case II decision report)
- 후속 분석: **docs/reports/0045-refresh-session-reconnect-loop-analysis.md** (refresh 흐름의 effect-depth loop 분석 — 본 plan 의 8-state 재정의 + boot screen 분기 + Canvas mount 원자성 의 origin)
- BE 격리 work: **docs/reports/0046-be-attach-handler-idempotent.md** (D3 same-cookie idempotent contract drift, BE-only)
- 관련 plan: `docs/plans/0007-multi-session-pivot.md` §14.10 Tier 3 (WS reconnect backoff — 본 plan 의 transport layer)
- BE 변경: **0** (`attach_handler` 가 idempotent — 0046 의 contract drift fix 도 BE-only, 본 plan 의 FE 흐름과 직교)
- **현 상태 (2026-05-16)**:
  - **Phase 1 ✅ ship** (Case I, 7-state 머신 + ReconnectModal + boot screen) — commit chain `7703b19` 묶음 D / `da7663b` 묶음 E (0045 P0 후속의 state 머신 재정의)
  - **Phase 2 ✅ ship** (Case II, silentReattach + reattachInProgress + ensureMutationOk helper) — commit `b8e5766` (helper 일관화)
  - 잔여: handover-v3 status 정합 amend / Canvas mount 원자성 (0045 의 P0-A flowNodes cache + P0-B viewport one-shot — 별 후속 sprint)

---

## 0. 한 줄 요약

`사용자 page entry 시 sessionStorage 에 이전 attach 한 session name hint 가 있으면, AppPage 본 화면 mount 를 차단한 채 ReconnectModal 을 띄우고 silent POST /attach 자동 시도 — 성공 시 본 화면 진입, 실패 시 modal transition (in_use/not_found/unreachable), 사용자 [Switch session…] 명시 cancel always-visible.` (Phase 1)

`사용자 page 사용 중 idle reactivate 시 silent attemptReattach + mutation guard (모든 outgoing write in-flight 점검). 실패 시만 modal.` (Phase 2)

---

## 1. UI/UX 디자인 — Phase 1 (Case I)

### 1.1 진입 흐름 (의사결정 tree, **0045 P0 후속 amend**)

```
Page load (AppPage onMount)
  │
  ├─ 0) reconnectGate.state = 'booting' (initial)
  │    canMountApp = false → 본 화면 미렌더 + boot screen ("Restoring session…") 노출
  │
  ├─ Path 분기 (src/main.ts 의 path-based mount)
  │   /auth, /auth/*    → AuthPage (ADR-0020 D13, plan-0009)
  │   /auth-preview      → 디자인 demo alias
  │   그 외              → AppPage 진입
  │
  ├─ 1) Auth gate (GET /api/sessions)
  │   401 → window.location.href = '/auth'  (현 흐름, 변경 없음)
  │   200 [...] → 2) 다음 단계
  │   기타 / 예외 → reconnectGate.markIdle() (booting 영구화 방지) → 5xx UI
  │
  ├─ 2) sessionStorage 의 hint 검사 (key = `gtmux-last-active-session`)
  │   ├─ 없음 (fresh tab / 첫 진입 / 명시 cancel 후) →
  │   │    reconnectGate.markIdle() → workspaceSwitcher.open() → SessionListModal (D7/D8)
  │   │    canMountApp = true (idle) — 단 sessionStore.active == null 인 동안 본 화면 자체가 빈 상태
  │   │
  │   └─ 있음 (= name) →
  │        ▼
  │       3) reconnectGate.start(name) → state = 'attaching'
  │          (본 화면 mount 차단 — Svelte `{#if reconnectGate.canMountApp}` 게이트 유지)
  │          (boot screen 의 진행 메시지가 "Reconnecting session…" 으로 transition)
  │       ▼
  │       4) sessionStore.attemptReattach(name, signal)
  │          ├─ 200 (attach 성공) → [내부 단계] state = 'hydrating' → GET /layout +
  │          │       sessionStore.setActiveSession + loadLayout → 완료 시 markReady()
  │          │       → state = 'ready' → canMountApp = true → 본 화면 mount
  │          ├─ 409 → state = 'in_use' (ReconnectModal 의 modalState='in_use')
  │          ├─ 404 → state = 'not_found' + sessionStorageHint.clear()
  │          ├─ 401 → /auth redirect
  │          └─ 5xx/network → state = 'unreachable' + attempt 카운터 ++
  │
  └─ 5) [Switch session…] 클릭 (`attaching`/`hydrating`/실패 state 모든 곳) →
        reconnectGate.cancel() (= AbortController.abort + state='idle' + hint clear)
        → workspaceSwitcher.open() → SessionListModal
```

**핵심 변경 (0045 P0 후속, ADR-0019 변경 이력 *2026-05-16 (0045 P0 후속)* entry 정합)**:
- Initial state 가 `'idle'` → `'booting'` 로 변경 — auth gate 도착 전 빈 Canvas mount 차단의 *enumeration-level* 정합 표현.
- `attaching` / `hydrating` 두 progress 단계 — `attaching` 은 POST /attach 진행, `hydrating` 은 GET /layout + loadLayout 진행. 둘 다 `modalState='loading'` 으로 normalize → 사용자 perception 은 single "loading" 흐름.
- `markReady()` = hydrate 완료 후 본 화면 mount 허용 (`markSuccess()` 는 호환 alias, deprecated).
- `markIdle()` = `booting/attaching/hydrating` 종료 보장의 명시 method — 모든 abort/error 경로에서 호출해 boot-screen 영구화 방지.
- `canMountApp = ready || idle` — `idle` 도 mount 허용 (workspaceSwitcher 가 빈 Canvas 위에 SessionListModal 띄움. 사용자 명시 선택 후 attach 응답으로 `setActiveSession` + `loadLayout` 호출 — 본 흐름은 reconnectGate 와 직교).

### 1.2 reconnectGate state 머신 (8 state, **0045 P0 후속**)

```
reconnectGate.state ∈
  { booting, attaching, hydrating, ready,           # progress
    in_use, not_found, unreachable,                 # failure
    idle }                                           # post-cancel / hint 없음

modalState (derived) ∈
  { loading, in_use, not_found, unreachable, null } # ReconnectModal mode prop
  ↑ attaching/hydrating → 'loading' normalize
  ↑ booting/ready/idle  → null (modal mount 안 함)


  AppPage onMount entry
  │
  ▼
┌──────────┐  auth gate / hint 검사 중 — boot screen 노출
│ booting  │  (modalState=null, canMountApp=false)
└─┬────────┘
  │
  ├─ hint 있음 → start(name)
  │    ▼
  │  ┌────────────┐  POST /attach (modalState='loading' → ReconnectModal mount,
  │  │ attaching  │   header "Reconnecting session", spinner, [Switch…] less-emphasis)
  │  └─┬──┬──┬──┬─┘
  │    │  │  │  │
  │  200│409│404│ 401 → /auth (redirect, modal 상관 없음)
  │    │  │  │  │ 5xx/network → 'unreachable'
  │    ▼  ▼  ▼  ▼
  │  ┌──────────┐  ┌──────┐  ┌──────────┐  ┌──────────────┐
  │  │hydrating │  │in_use│  │not_found │  │ unreachable  │
  │  │GET layout│  │      │  │+hint     │  │+attempt++    │
  │  │ +load    │  │옵션: │  │ clear    │  │옵션: [Retry] │
  │  │(loading) │  │[Sw…] │  │옵션:[Sw…]│  │     [Sw…]    │
  │  └─┬────────┘  └──┬───┘  └──┬───────┘  └──┬───────────┘
  │    │ markReady   │          │              │ Retry → attaching
  │    │             │          │              └────────┐
  │    ▼             ▼          ▼                       │
  │  ┌──────┐  ┌────────────────────────┐               │
  │  │ ready│  │ [Switch session…]      │               │
  │  │본화면 │  │ = reconnectGate.cancel │               │
  │  │mount │  │   → state='idle'       │◀──────────────┘
  │  └──────┘  │   + hint clear         │
  │            │   → workspaceSwitcher  │
  │            └──┬─────────────────────┘
  │               ▼
  ├─ hint 없음 → markIdle()
  │    ▼
  │  ┌──────┐
  │  │ idle │ (canMountApp=true — 빈 Canvas 위에 SessionListModal)
  │  └──────┘
  │
  └─ auth 401 → /auth redirect (modal/state 무관)
```

**Mode normalization**: `attaching` + `hydrating` 두 phase 는 사용자 perception 에 *동일 "loading"* — modal 의 header 메시지 / spinner 가 변하지 않음. 단 *boot screen* (modal 진입 전, `state='attaching'` 인데 100ms grace 안인 케이스 등) 은 `+page.svelte` template 의 별 분기 (`{:else if state ∈ booting/attaching/hydrating}`) 가 처리 — modal 이 아직 mount 안 됐어도 사용자가 빈 화면을 보지 않음.

### 1.3 Modal mockup — text 정밀

**state = `loading`**:
```
┌───────────────────────────────────────────────────────┐
│                                                       │
│   ┌─────────────────────────────────────────────┐    │
│   │  Reconnecting session                       │    │
│   ├─────────────────────────────────────────────┤    │
│   │                                             │    │
│   │           ⟳   Restoring "test_"…            │    │
│   │                                             │    │
│   ├─────────────────────────────────────────────┤    │
│   │                          Switch session →   │    │
│   └─────────────────────────────────────────────┘    │
│                                                       │
└───────────────────────────────────────────────────────┘
backdrop rgba(0,0,0,0.5), 본 화면 미렌더
```

**state = `in_use` (409)**:
```
   ┌─────────────────────────────────────────────┐
   │  Session in use                          ⚠  │
   ├─────────────────────────────────────────────┤
   │                                             │
   │   "test_" is now in use by another window.  │
   │   Idle timeout 후 다른 webpage 가           │
   │   가져갔을 수 있어요.                       │
   │                                             │
   ├─────────────────────────────────────────────┤
   │                       [ Switch session… ]   │
   └─────────────────────────────────────────────┘
```

**state = `not_found` (404)**:
```
   ┌─────────────────────────────────────────────┐
   │  Session not found                       ⚠  │
   ├─────────────────────────────────────────────┤
   │                                             │
   │   "test_" no longer exists.                 │
   │   다른 session 으로 진입하세요.             │
   │                                             │
   ├─────────────────────────────────────────────┤
   │                       [ Switch session… ]   │
   └─────────────────────────────────────────────┘
```

**state = `unreachable` (5xx / network)**:
```
   ┌─────────────────────────────────────────────┐
   │  Reconnect failed                        ⚠  │
   ├─────────────────────────────────────────────┤
   │                                             │
   │   Couldn't reach the server.                │
   │   Attempt 3.  Last error: timeout           │
   │                                             │
   ├─────────────────────────────────────────────┤
   │            [ Retry ]   [ Switch session… ]  │
   └─────────────────────────────────────────────┘
```

### 1.4 디자인 결정 10개 (ADR-0019 D5.4 사본)

| # | 결정 | 이유 |
|---|---|---|
| D-I.1 | [Switch session…] 항상 노출 (loading 포함) | 사용자 요구 "옵션으로 새로 session 진입도" 정합 — 즉시 cancel 가능 |
| D-I.2 | [Switch session…] 의 강조: loading 에서는 less-emphasis (text-link 톤), 실패 state 에서는 primary | 정상 흐름 = attempt 우선, 실패 = 사용자 선택 우선 |
| D-I.3 | Esc 비활성 | 실수 close 방지 |
| D-I.4 | Backdrop click 비활성 | 동일 |
| D-I.5 | Modal 진입에 100ms grace | 빠른 200 케이스 flicker 방지 |
| D-I.6 | State transition 시 modal mount 유지 + content swap | 시각 일관성 / animation noise 최소 |
| D-I.7 | Focus trap + ARIA dialog modal=true | 접근성 |
| D-I.8 | sessionStorage key = `gtmux-last-active-session` (tab-scoped) | multi-tab 충돌 방지 |
| D-I.9 | [Switch session…] = AbortController.abort + hint remove + workspaceSwitcher.open | 사용자 명시 cancel — 다음 reload 도 dialog 흐름 |
| D-I.10 | attempt 성공 시 sessionStorage hint 갱신 (attach 마다) | hint 의 정확성 |

### 1.5 사용자 [Switch session…] 후 흐름

1. ReconnectModal close (state 무관 — `reconnectGate.cancel()` 가 state='idle' 로 전이 → `modalState=null` 으로 자연 unmount)
2. `AbortController.abort()` → in-flight `POST /attach` cancel (BE 도 idempotent — race 무해)
3. `sessionStorage.removeItem('gtmux-last-active-session')` — 다음 reload 도 dialog 흐름
4. `workspaceSwitcher.open()` → SessionListModal mount (기존 D7/D8)
5. SessionListModal 의 상단 `[+ New session…]` (NewSessionModal 진입) 또는 하단 list row 클릭
6. attach 성공 → AppPage 본 화면 mount + sessionStorage hint set (다음 reload 정합)

---

## 2. UI/UX 디자인 — Phase 2 (Case II)

**현 상태 (2026-05-16 ship)**: 실제 land 는 *별 modal 분기 없이* Phase 1 의 `ReconnectModal` 을 그대로 활용 + 추가로 `sessionStore.silentReattach` + `ensureMutationOk` helper 만 신설. 즉 *컴포넌트 통합 옵션* 의 채택 — modal 의 mode (`loading/in_use/not_found/unreachable`) 가 Case I/II 공통.

### 2.1 실제 ship 정합

- **Trigger 합집합** = WS state `reconnecting → open` (`lib/ws/dispatcher.svelte.ts` 의 onStateChange) + `document.visibilitychange === 'visible'` (`routes/+page.svelte` 의 onMount listener). Pre-condition = `sessionStore.active !== null`.
- **Silent 호출**: `sessionStore.silentReattach(name, signal)` — Phase 1 의 `attemptReattach` 와 *별 method* (Phase 2 의 silent path 가 reconnectGate 의 modal 흐름과 *직교* 하기 위해 분리). 단 내부적으로 같은 `POST /attach + GET /layout` 호출. In-flight singleton (`#silentReattachPromise`) 로 중복 trigger 도 동일 promise 반환.
- **Mutation guard**: `ensureMutationOk(abortMessage?): Promise<boolean>` (`sessionStore.svelte.ts:481` exported helper). 모든 outgoing write 진입점에서 호출 — `reattachInProgress` 이면 promise await, 결과 분기:
  - `success` → `true` (mutation 진행)
  - `in_use/not_found/unauthorized/unreachable` → `false` (mutation abort) + 호출자가 toast (abortMessage)
- **결과 분기 후 UI**: Phase 1 의 `reconnectGate` 머신 진입 — silentReattach 실패 시 reconnectGate 의 `state = 'in_use' | 'not_found' | 'unreachable'` 전이 (= Phase 1 의 modal 흐름 재사용). 본 화면 mount 유지 (canMountApp 은 ready 상태 유지) + modal 만 그 위에 layer.

### 2.2 ~~컴포넌트 통합 옵션~~ → **단일 ReconnectModal 채택 ✅**

`ReconnectModal.svelte` 단일 컴포넌트가 4 mode (`loading/in_use/not_found/unreachable`) 로 두 case 모두 cover. 별 `SessionInUseModal` / `SessionGoneModal` / `ReattachFailedModal` 분리 안 함 — 시각 일관성 + 컴포넌트 면적 절약. *Toast-only silent mode* 는 추가 안 함 — Phase 2 는 modal 자체를 *실패 시만* 띄우므로 *silent 가 success path 의 default* (별 toast UI 없이도 사용자 perception 0).

---

## 3. 우선순위 — Phase 1 먼저, Phase 2 후속 (✅ **둘 다 ship 완료**)

| 기준 | Phase 1 (Case I, D5.4) | Phase 2 (Case II, D5.1) |
|---|---|---|
| 사용자 만나는 빈도 | **매 page reload** (잦음) | idle-after-active (덜 빈번) |
| UX 끊김 심각도 | "본 화면 mount 됐는데 끊김 detect 후 modal" 보다 *"진입 흐름 자체가 명확"* | 작업 중 갑작스러운 차단 — 더 부정적 |
| 의존성 | `attemptReattach` utility + `ReconnectModal` 컴포넌트 신규 | Phase 1 의 utility + 컴포넌트 재사용 + mutation guard 만 추가 |
| 사용자 명시 요구 | **"가장 먼저"** 명시 | follow-up 요구 |
| 회귀 risk | 진입 흐름의 amend (auth-gate 추가 logic) — 격리 | mutation 진입점 6+ 곳 amend — 더 넓은 surface |
| 실제 ship 시점 | ✅ 2026-05-16 (P1.1~P1.8, commit chain `7703b19` 묶음 D 이전) | ✅ 2026-05-16 (`7703b19` 묶음 D + 후속 `b8e5766` ensureMutationOk helper 일관화) |

→ ~~Phase 1 먼저 ship + 안정화. Phase 2 는 Phase 1 land 후 진입.~~ 두 phase 모두 *동일 sprint* 안에 ship 됐고 (의존성 분리가 잘 됨), 후속 P1.9 (0045 P0 후속 8-state 머신 amend) 가 두 phase 의 공통 reconnectGate 머신을 강화 — Phase 1 의 *진입 흐름* + Phase 2 의 *mutation guard* 둘 다 같은 state 머신 위에 안정화.

---

## 4. 구현 inventory — Phase 1

### 4.1 신규 파일

| 파일 | 책임 |
|---|---|
| `lib/chrome/ReconnectModal.svelte` ⭐ | 5 mode (loading / in_use / not_found / unreachable / silent) modal. Phase 1 에서 첫 4 mode ship. Backdrop + center card + ARIA + focus trap + Esc/backdrop click 비활성. |
| `lib/stores/reconnectGate.svelte.ts` ⭐ | Page entry blocking 상태 머신 (**0045 P0 후속 8-state**). `state: $state<ReconnectState>` (`booting/idle/attaching/hydrating/in_use/not_found/unreachable/ready`), `attemptName: $state<string \| null>`, `error: $state<string \| null>`, `attempt: $state<number>`. Derived: `canMountApp` (= ready ‖ idle), `modalState` (attaching/hydrating → 'loading' normalize, 그 외 모두 null/그대로). Methods: `start(name)` / `retry()` / `cancel()` / `markIdle()` / `markReady()` / `markSuccess()` (deprecated alias). AbortController 보유. Initial state = `'booting'`. |

### 4.2 amend 파일

| 파일 | 변경 |
|---|---|
| `lib/stores/sessionStore.svelte.ts` | + `attemptReattach(name: string, signal?: AbortSignal): Promise<ReattachResult>` method — `POST /api/sessions/<name>/attach` + 분기 (`{ kind: 'success' \| 'in_use' \| 'not_found' \| 'unauthorized' \| 'unreachable', cause?: unknown }`). signal 동봉 시 AbortError 정합.<br>+ `setActiveSession()` 안에서 sessionStorage hint write.<br>+ `clear()` 안에서 sessionStorage hint remove (명시 detach/logout/[Switch session…] 흐름 모두 통과).<br>+ `loadLayout` 성공 시 hint 갱신. |
| `routes/+page.svelte` | onMount 의 흐름 amend (실제 ship 정합):<br>0) reconnectGate.state = 'booting' (initial — 본 화면 차단 + boot screen 노출)<br>1) auth-gate (`GET /api/sessions`)<br>&nbsp;&nbsp;&nbsp;&nbsp;401 → `/auth` redirect (모든 다른 분기 안 함)<br>&nbsp;&nbsp;&nbsp;&nbsp;예외 / 5xx → `reconnectGate.markIdle()` (booting 영구화 방지) + 5xx UI<br>2) sessionStorage hint 검사<br>&nbsp;&nbsp;&nbsp;&nbsp;있음 → `reconnectGate.start(hint)` (state = 'attaching')<br>&nbsp;&nbsp;&nbsp;&nbsp;없음 → `reconnectGate.markIdle()` + `workspaceSwitcher.open()`<br>본 화면 (`<Canvas /> <Toolbar2 /> <LeftPanel /> <RightPanel /> ...`) mount gate: `{#if reconnectGate.canMountApp}` (= `ready` ‖ `idle`)<br>Boot screen 분기: `{:else if state ∈ {booting, attaching, hydrating}}` — 진행 메시지 가시화 ("Restoring session…" / "Reconnecting session…" / "Hydrating layout…").<br>ReconnectModal mount: `{#if reconnectGate.modalState !== null}` — `attaching/hydrating` 동안 modal 의 `loading` mode 가 그 위에 추가 layer (boot screen 와 동시 또는 transition). |
| `lib/chrome/SessionListModal.svelte` (기존) | 변경 없음 (Phase 1 은 흐름만 연결). 단 [+ New session…] 진입점이 modal 의 상단에 명시 있는지 확인 필요 — 없으면 사용자 요구 "새로운 세션 or 기존 세션 선택" 정합 위해 add. |
| `lib/stores/workspaceSwitcher.svelte.ts` (기존) | `open()` 가 ReconnectModal 의 cancel 흐름과 정합되는지 확인 — `sessionStore.clear()` 가 hint remove 도 trigger 하는지. |
| `lib/http/auth.ts` (기존) | Logout 흐름에서 sessionStorage hint remove 추가. |

### 4.3 컴포넌트 신규 — ReconnectModal.svelte 상세

**Props** (실제 ship — Phase 1 P1.5):
```typescript
interface Props {
  mode: ReconnectModalState;  // ← prop 이름 'mode' (svelte-check `$state`
                              //   heuristic 회피). reconnectGate.modalState 직접 bind.
                              //   = 'loading' | 'in_use' | 'not_found' | 'unreachable'
                              //   ('loading' 은 attaching/hydrating 둘 다 normalize)
  name: string;               // session name (모든 mode 에서 표시)
  attempt: number;            // unreachable mode 의 카운터
  error: string | null;       // unreachable mode 의 last error message
  onSwitchSession: () => void;
  onRetry: () => void;        // unreachable mode 만 의미 있음
}
```

**Mode-별 view 정의**:
- `loading`: header "Reconnecting session", body spinner + `Restoring "${name}"…`, footer [Switch session…] (less-emphasis). attaching/hydrating 둘 다 동일 view (normalize).
- `in_use`: header "Session in use ⚠", body `"${name}" is now in use by another window.\nIdle timeout 후 다른 webpage 가 가져갔을 수 있어요.`, footer [Switch session…] (primary)
- `not_found`: header "Session not found ⚠", body `"${name}" no longer exists.\n다른 session 으로 진입하세요.`, footer [Switch session…] (primary)
- `unreachable`: header "Reconnect failed ⚠", body `Couldn't reach the server.\nAttempt ${attempt}.  Last error: ${error}`, footer [Retry] [Switch session…] (둘 다 primary, Retry 가 default)

**Behavior**:
- 진입 animation: backdrop fade 200ms + card scale 0.95→1 opacity 0→1 (200ms ease-out)
- State transition: header/body/footer content 만 swap (modal mount 유지). 100ms cross-fade.
- ARIA: `role="dialog"`, `aria-modal="true"`, `aria-labelledby="reconnectModalTitle"`, `aria-describedby="reconnectModalBody"`
- Focus management: modal mount 시 첫 actionable element (Switch session 또는 Retry 버튼) 으로 focus 이동. Focus trap.
- Esc / backdrop click: handler 가 stopPropagation — close 안 됨 (D-I.3, D-I.4)

### 4.4 stores/reconnectGate.svelte.ts 상세 (**0045 P0 후속, 실제 ship 정합**)

```typescript
export type ReconnectState =
  | 'booting'        // initial — auth gate / hint 검사 중. 본 화면 mount 금지.
  | 'idle'           // hint 없음 or 사용자 cancel 후. 본 화면 mount 허용
                     // (workspaceSwitcher 가 빈 Canvas 위에 SessionListModal).
  | 'attaching'      // POST /attach 진행 중 (modalState='loading')
  | 'hydrating'      // 200 응답 후 GET /layout + loadLayout 진행 중 (modalState='loading')
  | 'in_use'         // 409 — modal
  | 'not_found'      // 404 — modal + hint clear
  | 'unreachable'    // 5xx / network — modal + retry
  | 'ready';         // hydrate 완료 — 본 화면 mount 허용

/** ReconnectModal 의 mode prop — attaching/hydrating 은 'loading' 으로 normalize. */
export type ReconnectModalState = 'loading' | 'in_use' | 'not_found' | 'unreachable';

class ReconnectGate {
  state = $state<ReconnectState>('booting');     // ← initial 'booting'
  attemptName = $state<string | null>(null);
  error = $state<string | null>(null);
  attempt = $state<number>(0);

  #controller: AbortController | null = null;

  // 본 화면 mount 게이트 — 'ready' (정상 reattach + hydrate 완료) 또는
  // 'idle' (hint 없음 / cancel 후) 에서만 true.
  // - 'booting' / 'attaching' / 'hydrating' = 진행 중 — 빈/partial Canvas mount 차단.
  // - failure (in_use/not_found/unreachable) = ReconnectModal 만 노출 — 본 화면 차단.
  canMountApp = $derived(this.state === 'ready' || this.state === 'idle');

  // ReconnectModal 의 mode prop 으로 직접 bind. attaching/hydrating 을 'loading' 으로
  // normalize, booting/ready/idle 은 null (modal mount 안 함).
  modalState = $derived.by((): ReconnectModalState | null => {
    switch (this.state) {
      case 'attaching':
      case 'hydrating':  return 'loading';
      case 'in_use':     return 'in_use';
      case 'not_found':  return 'not_found';
      case 'unreachable':return 'unreachable';
      case 'booting':
      case 'idle':
      case 'ready':      return null;
    }
  });

  /** 'booting' → 'idle' 명시 전이. hint 없음 / auth 5xx / 예외 경로 모두 호출.
   * boot screen 영구화 방지. canMountApp=true 로 진입 (workspaceSwitcher 가 그 위에 modal). */
  markIdle(): void {
    this.state = 'idle';
    this.attemptName = null;
    this.error = null;
    this.attempt = 0;
  }

  async start(name: string): Promise<void> {
    this.state = 'attaching';
    this.attemptName = name;
    this.attempt = 1;
    await this.#run(name);
  }

  async retry(): Promise<void> {
    if (this.attemptName === null) return;
    this.state = 'attaching';
    this.attempt += 1;
    await this.#run(this.attemptName);
  }

  /** 사용자 명시 cancel ([Switch session…]). attaching/hydrating/failure 모든 곳에서. */
  cancel(): void {
    this.#controller?.abort();
    this.#controller = null;
    this.state = 'idle';
    this.attemptName = null;
    this.error = null;
    this.attempt = 0;
    sessionStorageHint.clear();   // 다음 reload 도 dialog 흐름
  }

  /** hydrate 완료 — 본 화면 mount 허용. */
  markReady(): void {
    this.state = 'ready';
  }

  /** @deprecated 0045 P0 — `markSuccess` → `markReady` rename. 호환 alias. */
  markSuccess(): void { this.markReady(); }

  async #run(name: string): Promise<void> {
    this.#controller?.abort();
    this.#controller = new AbortController();
    const signal = this.#controller.signal;
    // attaching → hydrating 의 boundary 는 attemptReattach 내부의 hook 으로만 관찰
    // 가능. 본 wrapper 는 attaching 단일 phase 로 시작 후 success 시 markReady 로
    // 직접 진입 — modalState='loading' normalization 으로 사용자 perception 동일.
    const result = await sessionStore.attemptReattach(name, signal);
    if (signal.aborted) return;
    switch (result.kind) {
      case 'success':
        this.markReady();
        return;
      case 'in_use':
        this.state = 'in_use';
        return;
      case 'not_found':
        this.state = 'not_found';
        sessionStorageHint.clear();
        return;
      case 'unauthorized':
        window.location.href = '/auth';
        return;
      case 'unreachable':
        this.state = 'unreachable';
        this.error = result.message ?? 'unknown';
        return;
    }
  }
}

export const reconnectGate = new ReconnectGate();
```

**Note**: 위 의사코드는 실제 ship 정합 (`codebase/frontend/src/lib/stores/reconnectGate.svelte.ts` 의 export 와 1:1). 본 plan 의 *진실 출처는 코드*; 본 § 의 코드는 cold-pickup 용 reference.

### 4.5 sessionStorage hint helper

```typescript
// lib/stores/sessionStorageHint.ts (또는 sessionStore 안 private)
const KEY = 'gtmux-last-active-session';

export const sessionStorageHint = {
  get(): string | null {
    try {
      return sessionStorage.getItem(KEY);
    } catch {
      return null;  // SSR / private mode 등
    }
  },
  set(name: string): void {
    try {
      sessionStorage.setItem(KEY, name);
    } catch {
      // best-effort
    }
  },
  clear(): void {
    try {
      sessionStorage.removeItem(KEY);
    } catch {
      // best-effort
    }
  },
};
```

### 4.6 `+page.svelte` onMount + template (**실제 ship 정합**)

```typescript
onMount(async () => {
  // 0) reconnectGate.state = 'booting' (initial — store 안 default)

  try {
    // 1) auth-gate
    const res = await fetch('/api/sessions', { credentials: 'include' });
    if (res.status === 401) {
      window.location.href = '/auth';
      return;
    }
    if (!res.ok) {
      // 예외 / 5xx — booting 영구화 방지
      reconnectGate.markIdle();
      // 별 5xx UI (toast 또는 별 banner)
      return;
    }

    // 2) sessionStorage hint 검사
    const hint = sessionStorageHint.get();
    if (hint !== null) {
      // 3) hint 있음 → 'attaching' 전이 (boot screen → modal 의 loading view)
      void reconnectGate.start(hint);
    } else {
      // 4) hint 없음 → 'idle' 전이 + workspaceSwitcher
      reconnectGate.markIdle();
      workspaceSwitcher.open();
    }
  } catch {
    // 예측 못 한 예외 — booting 영구화 방지
    reconnectGate.markIdle();
  }
});
```

본 화면 template (실제 ship 의 분기 구조):

```svelte
{#if reconnectGate.canMountApp}
  <!-- 본 화면: state ∈ {ready, idle}. idle 은 빈 Canvas + workspaceSwitcher 의 modal. -->
  <Titlebar />
  <Toolbar2 />
  <SvelteFlowProvider>
    <Canvas />
  </SvelteFlowProvider>
  <LeftPanel />
  <RightPanel />
  ...
{:else if reconnectGate.state === 'booting'
       || reconnectGate.state === 'attaching'
       || reconnectGate.state === 'hydrating'}
  <!-- Boot screen — modal 진입 grace (100ms) 동안 빈 화면 차단 -->
  <div class="boot-screen">
    <div class="spinner" />
    {#if reconnectGate.state === 'attaching'}
      <p>Reconnecting session…</p>
    {:else if reconnectGate.state === 'hydrating'}
      <p>Loading layout…</p>
    {:else}
      <p>Restoring session…</p>
    {/if}
  </div>
{/if}

<!-- Modal layer: modalState !== null = attaching/hydrating ('loading') 또는 failure -->
{#if reconnectGate.modalState !== null}
  <ReconnectModal
    mode={reconnectGate.modalState}
    name={reconnectGate.attemptName ?? ''}
    attempt={reconnectGate.attempt}
    error={reconnectGate.error}
    onSwitchSession={() => {
      reconnectGate.cancel();
      workspaceSwitcher.open();
    }}
    onRetry={() => void reconnectGate.retry()}
  />
{/if}

<!-- 기존 modal (workspaceSwitcher 의 SessionListModal/NewSessionModal/AuthDialog 등) -->
...
```

**Boot screen 의 역할**: ReconnectModal 의 100ms grace 동안 (즉, attempt 가 빠른 200 인 경우) modal 이 mount 안 됐어도 빈 화면 노출 안 됨. modal 이 mount 되면 그 위에 layer — 두 layer 가 잠시 동시 보일 수 있으나 modal 의 backdrop 가 boot screen 을 가림 — 시각 부담 0.

---

## 5. 구현 단계 — Phase 1

| 단계 | 상태 | 작업 | 산출물 | 검증 |
|---|---|---|---|---|
| **P1.1** | ✅ 2026-05-16 | sessionStorage hint helper | `lib/stores/sessionStorageHint.ts` (top-level helper — `safeSessionStorage()` 로 SSR/private safe) | 단위: get/set/clear 가 try-catch 로 SSR safe |
| **P1.2** | ✅ 2026-05-16 | `sessionStore.attemptReattach(name, signal)` method | `lib/stores/sessionStore.svelte.ts` amend — `ReattachResult` union + fetch (POST attach → GET layout) + AbortSignal 동봉 | 단위: 200/409/404/401/5xx/network/abort 각각 정확한 `ReattachResult` 반환. 404 분기는 자체 hint clear |
| **P1.3** | ✅ 2026-05-16 | `setActiveSession` / `clear` 의 hint write/clear 통합 | 위 sessionStore amend 안 — setActiveSession 안 `sessionStorageHint.set`, clear 안 `sessionStorageHint.clear` | attach 성공 시 set, clear 시 remove (단위) |
| **P1.4** | ✅ 2026-05-16 | `lib/stores/reconnectGate.svelte.ts` 신규 | `ReconnectGate` class + `ReconnectState` union, `start`/`retry`/`cancel`/`markSuccess`, `canMountApp` derived, AbortController 보유 | 단위: start → run → 분기 → state 정확. cancel 시 AbortController.abort + state='idle' + hint clear |
| **P1.5** | ✅ 2026-05-16 | `lib/chrome/ReconnectModal.svelte` 신규 (4 mode) | Component (Modal primitive + Button) — prop 이름 `mode` (svelte-check `$state` heuristic 회피). 100ms grace + Esc/backdrop 비활성 + focus trap. | 시각 review (mockup 정합 — §1.3) + a11y (focus trap + ARIA via Modal primitive) |
| **P1.6** | ✅ 2026-05-16 | `routes/+page.svelte` onMount 흐름 amend + 본 화면 mount gate | Auth gate 후 `sessionStorageHint.get()` 분기 — 있음 = `reconnectGate.start`, 없음 = `reconnectGate.markIdle()` + `workspaceSwitcher.open`. 본 화면 (`Titlebar`/`Toolbar2`/`SvelteFlowProvider`+Canvas/`LeftPanel`/`RightPanel`) 을 `{#if reconnectGate.canMountApp}` 게이트. ReconnectModal mount 는 `{#if reconnectGate.modalState !== null}` (P1.9 의 derived) — 옛 `state !∈ {idle, success}` 표현은 P1.9 의 `modalState !== null` 로 통일. | E2E 시나리오 (§5.1) — Phase 1 ship 후 사용자 검증 |
| **P1.7** | ✅ 2026-05-16 | Logout 흐름에서 hint clear 통합 | `lib/chrome/SessionMenu.svelte` 의 `onLogout` 안 `sessionStorageHint.clear()` 추가 — `sessionStore.clear()` 거치지 않고 redirect 하는 path 의 보호. detach/[Switch session…]/session [Delete] 는 sessionStore.clear() 의 통합 clear 와 reconnectGate.cancel() 의 자체 clear 로 자동 처리 (별도 amend 불필요). | 단위 + E2E |
| **P1.8** | ✅ 2026-05-16 | 빌드 / plan-0008 status amend / commit | `npm run build` 통과 (517 modules / 1.36s). plan-0008 §5 status + §9 변경 이력 갱신 | type-check (svelte-check) 0 errors |
| **P1.9** | ✅ 2026-05-16 | **0045 P0 후속 — reconnectGate 8-state amend + boot screen 분기** | `lib/stores/reconnectGate.svelte.ts` rewrite (initial 'booting', `markIdle`/`markReady`/`modalState` derived) + `routes/+page.svelte` 의 try/catch + boot-screen template 분기 + WorkspaceSwitcher 의 markSuccess → markReady alias 호출. ADR-0019 변경 이력 *2026-05-16 (0045 P0 후속)* entry 와 paired. commit `da7663b` 묶음 E. | refresh 흐름의 *빈/partial Canvas mount 차단* state-level 정합. (0045 의 P0-A flowNodes cache + P0-B viewport one-shot 은 별 후속 sprint — 본 P1.9 와 layered) |

### 5.1 검증 시나리오 (E2E, Phase 1)

| # | 시나리오 | 기대 |
|---|---|---|
| S-1 | Fresh tab + sessionStorage 빈 상태 + AppPage 진입 | `workspaceSwitcher.open()` → SessionListModal (현 흐름) |
| S-2 | Tab reload + sessionStorage 의 hint "test_" + BE 의 attach 가능 | ReconnectModal `loading` → 100ms 후 표시 → attempt 200 → modal close → 본 화면 mount |
| S-3 | Tab reload + hint "test_" + 다른 webpage 가 takeover | ReconnectModal `loading` → 409 → `in_use` transition → 사용자 [Switch session…] 클릭 → SessionListModal 진입 (test_ 는 disabled row) |
| S-4 | Tab reload + hint "deleted_session" + 그 session 이 [Delete] 됨 | ReconnectModal `loading` → 404 → `not_found` transition + hint clear → [Switch session…] → SessionListModal |
| S-5 | Tab reload + hint + cookie 만료 | ReconnectModal `loading` → 401 → /auth redirect |
| S-6 | Tab reload + hint + BE down | ReconnectModal `loading` → network error → `unreachable` (attempt 1) → [Retry] 클릭 → `loading` (attempt 2) → ... |
| S-7 | Tab reload + hint + 사용자가 `loading` 동안 즉시 [Switch session…] | AbortController.abort → modal close → hint clear → workspaceSwitcher.open |
| S-8 | Tab reload + hint + attempt 가 50ms 안 200 (BE 빠른 응답) | Modal 100ms grace 안 close → 사용자가 modal 안 봄 (flicker 없음). 본 화면 mount |
| S-9 | A11y: Tab reload + hint + 키보드 사용자 | Modal mount 시 첫 focusable (Switch session 또는 Retry) 로 focus 이동. Tab 으로 trap. Esc 무시. |
| S-10 | Multi-tab: 탭 A=session-X attach, 탭 B 새로 열기 → 탭 B 진입 | 탭 B 의 sessionStorage 비어있음 (tab-scoped) → workspaceSwitcher (fresh 흐름) |

---

## 6. Phase 2 — Case II (✅ ship)

**현 상태**: Phase 1 ship 후 진입 완료. `silentReattach` + `reattachInProgress` + `#silentReattachPromise` (in-flight singleton) + `ensureMutationOk` helper 모두 ship + 사용 위치 4+ 곳 일관화 (commit `b8e5766`).

### 6.1 실제 ship inventory

| 파일 | 변경 |
|---|---|
| `lib/stores/sessionStore.svelte.ts` | ✅ `reattachInProgress: $state<boolean>` + `#silentReattachPromise: Promise<ReattachResult> \| null` (in-flight 중복 호출 시 동일 promise 반환) + `silentReattach(name, signal): Promise<ReattachResult>` method + `ensureMutationOk(abortMessage?): Promise<boolean>` **exported helper** (Phase 2 mutation guard 의 사용자-facing wrapper, sessionStore.svelte.ts:481) |
| `lib/ws/dispatcher.svelte.ts` | ✅ `onStateChange` 의 `reconnecting → open` 전이 시 `sessionStore.silentReattach(active.name)` trigger |
| `routes/+page.svelte` | ✅ `visibilitychange` listener bind/unbind — `visible` + `sessionStore.active != null` 시 silentReattach |
| `lib/canvas/Canvas.svelte` | ✅ mutation 진입점 (spawnMultiSessionTerminal / onnodedragstop / handleTerminalClick 등) 에 `await ensureMutationOk(...)` guard |
| `lib/canvas/TextNode.svelte` | ✅ text edit commit 진입점 guard (`Text edit aborted — session reconnect failed.`) |
| `lib/canvas/PanelDanglingOverlay.svelte` | ✅ respawn 진입점 guard |
| `lib/canvas/PanelNode.svelte` | ✅ label PATCH / delete 진입점 guard |
| `lib/sidebar/LayerTreeView.svelte` | ✅ layer mutation (rename, reorder, visibility, lock) 진입점 guard (line 577) |
| `lib/sidebar/TerminalListView.svelte` | ✅ kill / attach 진입점 guard |
| `lib/stores/zStore.svelte.ts` | ✅ z order change 진입점 guard (line 116) |

### 6.2 `ensureMutationOk` helper 사용 패턴 (b8e5766 일관화)

```typescript
// 모든 outgoing write 의 entry point 에서:
async function spawnMultiSessionTerminal(coords) {
  if (!(await ensureMutationOk('Terminal spawn aborted — session reconnect failed.'))) return;
  // ... 기존 로직
}
```

- helper 가 `reattachInProgress` 이면 `#silentReattachPromise` 를 await
- 결과 `success` → `true` 반환 (mutation 진행 허용)
- 결과 `in_use/not_found/unauthorized/unreachable` → `false` 반환 (mutation abort) + 호출자가 message 전달한 toast
- pre-condition `reattachInProgress === false` → 즉시 `true` (no-op guard)

### 6.3 검증

Report 0042 §5.10 의 10 test cases — 본 session 의 사용자 검증 결과 모두 PASS.

---

## 7. ADR / handover 정합 amend

| 파일 | amend status |
|---|---|
| `docs/adr/0019-session-and-workspace-model.md` | ✅ D5/D5.1/D5.2/D5.3/D5.4 + 변경 이력 의 *2026-05-16 (0045 P0 후속)* entry — reconnectGate 8-state, `markReady`/`markIdle`, `modalState` derived 의 정본 |
| `docs/reports/0042-session-attach-recovery.md` | ✅ TL;DR 의 Case I/II 분리 + 우선 구현 표기 + Phase 1+2 land status |
| `docs/reports/0045-refresh-session-reconnect-loop-analysis.md` | ✅ refresh effect-depth loop 의 P0 분석 + 본 plan 의 8-state 재정의 origin |
| `docs/reports/0046-be-attach-handler-idempotent.md` | ✅ BE attach_handler 의 same-cookie idempotent contract drift 격리 work package (BE-only, 본 plan FE 흐름과 직교) |
| `docs/agents/frontend-handover-v3.md` | ✅ Tier 3 / Phase 1+2 attach recovery status 정합 amend land — §1 헤더 표 / §2 mental model / §4.3 잔여 / §5 P0+P1 매트릭스 / §5 공용 컴포넌트 표 / §6 Stage 7 §6+§7+§8 / §8 BE 의존성 매트릭스 / §12 진입 메시지 / §13 변경 이력 |
| `docs/plans/0007-multi-session-pivot.md` §14.10 | ✅ Tier 3 + attach recovery layer 분리 amend land — WS reconnect 가 transport-level 만 (client.ts 통합 + heartbeat 별 store) + attach recovery 는 별 layer (D5.1/D5.4 + plan-0008) 임 명시. ShutdownModal rename 정합. §18 변경 이력 entry 추가 |
| `CONTEXT.md` | ✅ 어휘 5종 추가 land — Reconnect Gate / Boot Screen / Session Attach Hint / Silent Reattach / Mutation Guard + Relationships 의 *본 화면 mount = Reconnect Gate gating* invariant 줄 |
| 후속: 0045 P0-A flowNodes cache | ❌ 별 후속 sprint — Canvas mount 원자성의 *node identity churn* 차단. P1.9 의 state-level 차단이 일차 방어, 본 항목이 이차 방어 |
| 후속: 0045 P0-B viewport one-shot | ❌ 별 후속 sprint — SvelteFlow initialized 이후 1회만 store viewport 적용, 그 안 `onmove` persist 차단 |

---

## 8. 우려 / Risk / 후속

| Risk | 완화 |
|---|---|
| sessionStorage 가 private mode 에서 throw | helper 의 try-catch — `null` 반환 → hint 없음 흐름 (workspaceSwitcher.open) |
| BE 가 attach 응답 매우 빠름 (< 100ms) | 100ms grace — 사용자가 modal 안 봄. flicker 0 |
| BE 가 attach 응답 매우 느림 (> 5s) | loading state 유지 — 사용자 [Switch session…] always 가능. 사용자 결정 우선 |
| 동일 cookie 로 두 webpage 가 동시 attempt (rare) | BE 의 implicit detach-on-reattach (attach_handler) 로 자연 처리 |
| sessionStorage hint 가 stale (session 이 다른 server lifetime 에서 만들어진) | attempt → 404 → `not_found` 분기 + hint clear → 자연 처리 |
| AbortController 의 fetch 가 cancel 되었는데 BE 가 이미 attach 보유 | 다음 사용자 명시 attach 시 implicit detach 로 자연 처리. 또는 사용자 [Switch session…] 후 다른 session 선택 — 이전 in-flight attach 의 server-side state 는 다음 동작에서 처리 |
| ~~Phase 1 만 ship 한 상태에서 사용 중 idle reactivate (Case II)~~ | ✅ 해소 — Phase 2 ship 완료 (silentReattach + ensureMutationOk helper). 양 case 모두 cover |
| `'booting'` initial state 의 영구화 (auth gate 예외 / 5xx / 예측 못 한 throw 시) | `+page.svelte` 의 try/catch + 모든 종료 경로 (`401 redirect 제외`) 에서 `reconnectGate.markIdle()` 명시 호출 — boot screen 영구화 방지. P1.9 의 핵심 invariant |
| `'attaching' → 'hydrating'` boundary 가 wrapper 에서 안 보임 | 의도 — modalState='loading' normalize 로 사용자 perception 동일. instrumentation 필요 시 attemptReattach 내부 hook 추가 (별 후속) |
| 0045 의 effect-depth loop 가 P1.9 만으로 완전 해소 안 됨 (가능성) | P0-A (flowNodes cache) + P0-B (viewport one-shot) 가 별 후속 sprint. 본 plan 의 state-level 차단은 *partial mount race* 의 일차 방어 — 그 후 effect-level race 의 이차 방어 별도 |

---

## 9. 변경 이력

- 2026-05-16: 초안 — 사용자 grilling G50 follow-up (initial entry UX 요구) 의 결과. ADR-0019 D5.4 amend 와 짝. Phase 1 (Case I) 우선 / Phase 2 (Case II) 후속.
- 2026-05-16 (Phase 1 ship): §5 의 P1.1~P1.8 모두 land. 신규: `sessionStorageHint.ts` / `reconnectGate.svelte.ts` / `ReconnectModal.svelte`. Amend: `sessionStore.svelte.ts` (attemptReattach + hint write/clear), `+page.svelte` (hint 분기 + mount gate + ReconnectModal mount), `SessionMenu.svelte` (logout 의 hint clear). ReconnectModal 의 prop 이름은 `mode` — svelte-check 의 `$state` legacy store-prefix heuristic 회피. 빌드 통과 (517 modules / 1.36s), `svelte-check` 0 errors. 후속: §6 Phase 2 (Case II — silent + mutation guard) + §7 의 frontend-handover / plan-0007 §14.10 / CONTEXT.md 어휘 amend.
- 2026-05-16 (Phase 2 ship): commit `7703b19` 묶음 D — `silentReattach` + `reattachInProgress` + `#silentReattachPromise` (in-flight singleton) + mutation guard 진입점 wire (Canvas / PanelNode / PanelDanglingOverlay / TerminalListView / LayerTreeView / zStore / TextNode 등). 후속 commit `b8e5766` 에서 `ensureMutationOk(message?)` exported helper 도입 — 모든 진입점 사용 패턴 일관화 (`if (!(await ensureMutationOk('...'))) return;`). `routes/+page.svelte` 의 visibilitychange listener bind/unbind 추가. 0042 §5.10 의 10 test cases 사용자 검증 PASS.
- 2026-05-16 (P1.9 — 0045 P0 후속): ADR-0019 변경 이력 *2026-05-16 (0045 P0 후속)* entry 와 paired 로 reconnectGate 의 state 머신을 4 → 8 state 로 세분화. `'booting'` initial / `attaching` + `hydrating` progress split / `ready` (markSuccess→markReady) / `markIdle` 명시 method / `modalState` derived (attaching+hydrating→'loading' normalize). `+page.svelte` 의 try/catch + 모든 종료 경로의 markIdle 명시 호출 — `booting` 영구화 방지. boot screen 분기 (`{:else if state ∈ {booting, attaching, hydrating}}`) 추가 — modal 100ms grace 동안에도 빈 화면 노출 차단. `WorkspaceSwitcher.svelte` 의 markSuccess → markReady alias 호출. commit `da7663b` 묶음 E. 본 plan 의 §1.1 / §1.2 / §4.2 / §4.3 / §4.4 / §4.6 / §5 (P1.9 row 추가) / §6 (Phase 2 ship status) / §7 / §8 모두 본 amend 로 정합.
- 2026-05-16 (사용자 업데이트 반영 amend): 본 plan 의 §0 header (`현 상태` block) + §4 의 ReconnectModal `mode` prop + reconnectGate 의사코드 (8-state) + +page.svelte template (boot screen 분기) + §5 task list 의 P1.9 + §6 Phase 2 ship status + ensureMutationOk inventory + §7 amend status (✅/🟨/❌) + §8 risk amend 모두 ADR-0019 변경 이력 의 *2026-05-16 (0045 P0 후속)* entry + 실제 ship 된 코드 (`reconnectGate.svelte.ts` / `sessionStore.svelte.ts` 의 ensureMutationOk / `+page.svelte` 의 boot screen 분기) 와 1:1 정합. 0045 의 P0-A/P0-B 의 별 후속 sprint 분리 명시.
