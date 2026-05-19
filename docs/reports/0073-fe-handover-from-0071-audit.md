# 0073 — FE Handover: 0071 감사 결과의 FE 영역 land

- 작성일: 2026-05-18
- 작성 주체: agent (system-architect role) — 0071 감사 후속
- 정본 cross-link:
  - **상위 감사 보고**: [`0071-session-terminal-panel-lifecycle-audit.md`](./0071-session-terminal-panel-lifecycle-audit.md) (정독 필수 — §B-1, §B-2, §D-5 본 handover 의 trigger)
  - **paired BE handover**: [`0072-be-handover-from-0071-audit.md`](./0072-be-handover-from-0071-audit.md) (BE 측 짝 작업)
  - **관련 SSoT**: `docs/ssot/state-machines.md`
  - **관련 ADR**: ADR-0019 D5.4, ADR-0018 D6, ADR-0021 D6/D10

## 핵심 원칙 — 거짓 ship 방지

본 handover 의 모든 task 는 다음 5 원칙을 따른다. 위반 시 **ship 거부**:

1. **Anchor 명시**: 모든 작업 위치는 *file:line* 으로 명시. 추측 금지.
2. **Acceptance criteria 가 검증 가능**: "잘 됨" 이 아니라 *grep / pnpm check / browse E2E* 명령 + 기대 출력으로 정의.
3. **Anti-pattern 명시**: "이렇게 하면 false ship" 의 패턴을 task 별 명시.
4. **`ensureMutationOk` invariant 보존**: 본 handover 의 어떤 작업도 sessionStore 의 mutation guard / silent reattach singleton 영향 0 보장.
5. **Self-check 표 통과**: handover 끝의 표 모든 항목 ☑ 되어야 commit.

---

## 0. Self-grilling 결정 사항

### Q1. B-1 detach 호출의 sync vs fire-and-forget?

**결정**: ✅ **fire-and-forget**. `cancel()` signature 는 현 `cancel(): void` 유지. body 안에 `void detachSession(name).catch(() => {})` 패턴. 이유:
- ReconnectModal [Switch session…] 클릭 직후 사용자는 *즉시* SessionListModal 보고 싶어함
- detach 의 100-500 ms 대기가 modal transition 의 perception 손상
- 실패는 BE 의 30s heartbeat fallback 으로 회복

**거절**: `async cancel(): Promise<void>` — caller (Reconnect Modal button handler) 가 await 하면 UI freeze.

### Q2. B-1 detach 실패 시 사용자 알림?

**결정**: ❌ **toast 없음**. console.debug 로만 기록. 이유:
- 사용자는 *방금 cancel 누름* — 시각 흐름 중간에 error toast 가 컨텍스트 깸
- 30s heartbeat fallback 이 무조건 회복 (lock-leak 영구화 위험 0)
- cancelAttachConfirm (WorkspaceSwitcher.svelte:215+) 는 toast 띄우는데 *previous session restore* 의 부수효과가 있어 사용자에게 의미 있는 정보. reconnectGate.cancel 은 *진입 자체 포기* — 부수효과 없음.

### Q3. B-2(a) AttachConfirmModal 의 modal copy 언어?

**결정**: ✅ **영문 기본**. CLAUDE.md "i18n 은 별 결정" 정합. 한글 코멘트만 코드 옆.

### Q4. B-2(a) copy 의 정확한 문구?

**결정**: 현 description 영역에 *경고 줄 추가*:

```
This will spawn N new terminal(s) for missing IDs.
Note: New terminals start fresh — previous output cannot be restored.
[Cancel]  [Confirm]
```

→ "Note:" 의 시각 강조 (예: subtle warning color or italic) 는 design 영역 — 본 handover 에선 *문구만* 결정, 시각 표현은 FE-design 의 자율.

### Q5. D-5 sendBeacon 호출 시점?

**결정**: ✅ **`beforeunload` + `pagehide` 둘 다**. 이유:
- `beforeunload`: 데스크탑 navigate away 의 표준
- `pagehide`: iOS Safari + 모바일의 page lifecycle 이 `beforeunload` 안 fire 하는 케이스 cover (web platform spec)
- 양쪽 fire 시 BE 가 idempotent 호출 (BE-B 의 acceptance: 보유 lock 없으면 no-op)

### Q6. D-5 sendBeacon module 위치?

**결정**: ✅ **새 module `lib/lifecycle/leaveBeacon.ts`**. `+page.svelte` 의 onMount 에서 listener bind, onDestroy 에서 unbind. 이유:
- 단일 책임 — 다른 lifecycle 로직과 섞이지 않음
- 별 vitest 가능 (mock window + assert sendBeacon call)
- 기존 `lib/session/webpageId.ts` 와 자연 짝

### Q7. D-5 가 logout 흐름과 race 인가?

**결정**: ✅ **race 없음**. logout 흐름은 `SessionMenu.onLogout` 의 명시 액션 — `await logout()` 후 `window.location.href = '/auth'`. window.location 변경이 `beforeunload` trigger 하므로 sendBeacon 도 발화. 두 호출이 동시 도착해도 BE 는 idempotent (`release_lock_for_owner` 가 보유 lock 없으면 no-op).

---

## §A. Task 목록

| Task | 영역 | 출처 | 예상 소요 | 우선 |
|---|---|---|---|---|
| **FE-A** | `reconnectGate.cancel` 의 detach 호출 추가 | 0071 §B-1 | 1 commit + ADR amend | **P0** |
| **FE-B** | AttachConfirmModal copy 의 history 손실 경고 | 0071 §B-2(a) | 1 commit + ADR amend | P0 |
| **FE-C** | `lib/lifecycle/leaveBeacon.ts` + page lifecycle bind | 0071 §D-5 (FE 측) | 1 commit | P1 (BE-B 와 짝) |
| **FE-D** | (verify) AttachConfirmModal cancel chain toast 실 출력 | 0071 §D-4 | manual E2E | P1 |
| **FE-E** | (verify) rebind history replay 부재 — 시연 또는 미존재 확인 | 0071 §B-2.4 | manual E2E | P1 |

총 5 task. **FE-A 가 가장 영향 큼**, FE-D/FE-E 는 verify-only (코드 변경 없을 수도 있음).

---

## §B. Task FE-A — `reconnectGate.cancel()` 에 tentative detach 추가

### B-1. Trigger

0071 §B-1: ReconnectModal 의 [Switch session…] 클릭 시 BE 측 attach lock 이 release 안 됨 → 30s heartbeat timeout 까지 orphan. `WorkspaceSwitcher.svelte:215-250` 의 `cancelAttachConfirm` 패턴과 비대칭. ADR-0019 D5.4 amend ② 짝.

### B-2. Anchor

| 파일 | 변경 |
|---|---|
| `codebase/frontend/src/lib/stores/reconnectGate.svelte.ts:111-116` | `cancel()` body 에 `if (state==='attaching' && attemptName) detachSession(...)` fire-and-forget 추가 |
| `codebase/frontend/src/lib/http/sessions.ts` | `detachSession` export 가 이미 있음 (line ~115). import 만 추가. 변경 0. |
| `docs/adr/0019-session-and-workspace-model.md` D5.4 | amend ② — "사용자 명시 cancel = tentative lock release" 명시 |
| `docs/ssot/state-machines.md` §3.3 | "ReconnectModal [Switch session…]" 행에 *DELETE /attach 호출* 명시 |

### B-3. 의사 코드

```typescript
// reconnectGate.svelte.ts:111-116 의 cancel() 교체

import { detachSession } from '$lib/http/sessions';

/**
 * 사용자 명시 cancel ([Switch session…] 클릭).
 *
 * - 진행 중 fetch 가 있으면 abort.
 * - sessionStorage hint clear — 다음 reload 도 dialog 흐름.
 * - state = 'idle' 로 reset — 본 화면 mount 게이트는 통과하지만, AppPage
 *   에서 그 직후 `workspaceSwitcher.open()` 을 호출하므로 사용자가 본
 *   화면 빈 상태를 흘끗 보더라도 즉시 modal 이 덮음.
 * - **tentative attach lock release** (0071 §B-1 / ADR-0019 D5.4 amend ②):
 *   `state === 'attaching' && attemptName !== null` 분기에서 best-effort
 *   `detachSession` 호출. fire-and-forget — 실패는 30s heartbeat fallback.
 *   사용자 UI 가 [Switch session…] 클릭 직후 즉시 modal 전환 보장.
 */
cancel(): void {
  this.#controller?.abort();
  this.#controller = null;
  const wasAttaching = this.state === 'attaching' && this.attemptName !== null;
  const tentativeName = this.attemptName;
  this.markIdle();
  sessionStorageHint.clear();
  if (wasAttaching && tentativeName !== null) {
    // Best-effort tentative detach. 30s heartbeat 가 절대적 fallback이라
    // 실패 silent. console.debug 만 — toast 띄우면 modal 전환 noise.
    void detachSession(tentativeName).catch((err) => {
      console.debug('[gtmux] reconnectGate.cancel: tentative detach failed', err);
    });
  }
}
```

### B-4. ADR amend 짝

```markdown
### D5.4 amend ② — reconnectGate.cancel 의 tentative lock release (2026-05-18, 0071 §B-1)

사용자 명시 cancel ([Switch session…]) 시 BE attach lock 의 best-effort release. WorkspaceSwitcher 의 `cancelAttachConfirm` (D5.5.1) 패턴과 정합:

- Trigger: `reconnectGate.state === 'attaching'` 중 사용자가 [Switch session…] 클릭
- 동작: `AbortController.abort()` + `markIdle()` + `sessionStorageHint.clear()` + **`detachSession(attemptName)` (fire-and-forget)**
- 실패 정책: silent + console.debug. 사용자 toast 없음 — 진입 자체 포기 의도라 부수효과 정보 없음
- Fallback: detach 호출 실패 시 BE 의 30s heartbeat timeout 이 lock 회수 (ADR-0021 D6.2)

본 amend 가 없으면 cancel 직후 같은 session 을 다른 webpage 가 attach 시도 시 409 conflict (30s 까지). 같은 owner_key 의 재시도는 idempotent (D3 의 same-owner 분기) — 자가 회복은 가능하나 별 webpage 의 UX 손상.
```

### B-5. Acceptance criteria

| # | 검증 명령 | 기대 결과 |
|---|---|---|
| AC-FA1 | `cd codebase/frontend && pnpm check` | 0 errors / 0 warnings |
| AC-FA2 | `cd codebase/frontend && pnpm build` | 성공 |
| AC-FA3 | `grep -n "detachSession" codebase/frontend/src/lib/stores/reconnectGate.svelte.ts` | 1 hit (import) + 1 hit (호출) |
| AC-FA4 | **manual E2E**: BE demo running. browse 로 `/auth` → `?t=<token>` → ReconnectModal 진입 (또는 hint 기반 attach 진행 중 → [Switch session…] 클릭) → SessionListModal 로 즉시 전환 (delay 0) | UI 전환 < 100ms (sync abort + fire-and-forget detach) |
| AC-FA5 | **manual E2E**: 위 5 의 BE 측 log 확인 — `release_lock_for_owner` 호출 trace 가 100ms 안 도착 | log 1줄 |
| AC-FA6 | **manual E2E**: 위 5 의 직후 같은 owner_key 의 *다른* webpage 로 같은 session attach 시도 → 200 OK (대기 없음, 409 안 봄) | 200 OK |

### B-6. Anti-pattern

❌ **이런 fix 는 ship 거부**:

1. **`await detachSession()`**: cancel 이 detach 끝까지 기다리면 UI freeze. **fire-and-forget 필수**.
2. **toast 추가**: Q2 결정 위반. 부수 noise.
3. **`if (state === 'attaching' || state === 'hydrating')` 까지 확장**: hydrating 은 layout fetch 단계, attach lock 은 이미 잡혀있고 200 응답도 받음. detach 호출이 200 보낸 fetch 와 race — 의도 외 lock release 가능. **`attaching` 만**.
4. **`attemptName` null check 누락**: TypeScript 가 null 인 채 호출 → 런타임 에러. strict null check.
5. **abort 순서**: `markIdle()` 이 `attemptName` 을 null 로 reset 하므로, **detach 호출 전 변수 캡처 필수**.
6. **import 누락**: dynamic import (`await import(...)`) 사용 시 cancel 의 fire-and-forget 가 async 흐름 — Q1 결정 위반. **top-level import**.

### B-7. Self-check

- [ ] `reconnectGate.svelte.ts` 의 `cancel()` body 가 위 의사 코드대로 변경됨
- [ ] `detachSession` import 추가됨
- [ ] AC-FA1 ~ AC-FA6 모두 PASS
- [ ] ADR-0019 D5.4 amend ② 동봉 commit
- [ ] state-machines.md §3.3 의 ReconnectModal 행 명시 갱신
- [ ] commit message 가 "FE-A reconnectGate.cancel tentative detach" + 0071 §B-1 anchor

---

## §C. Task FE-B — AttachConfirmModal copy 의 history 손실 경고

### C-1. Trigger

0071 §B-2(a): AttachConfirmModal 의 "Will spawn N new terminal(s)" copy 가 *fresh process 라 history 없음* 사실 미고지 → 사용자 mental model 와 어긋남. UX-only fix.

### C-2. Anchor

| 파일 | 변경 |
|---|---|
| `codebase/frontend/src/lib/chrome/AttachConfirmModal.svelte` | description 영역에 한 줄 추가. 정확 위치는 현재 copy 의 직후 |
| `docs/adr/0018-canvas-item-data-model.md` D6 | (선택) match-or-spawn 의 fresh spawn arm 본문에 "previous output cannot be restored" 명시 |
| `docs/adr/0021-terminal-pool-and-mirror.md` D10 | (선택) lifecycle 의 *process = history* invariant 한 줄 추가 |

### C-3. 정확한 문구

기존:
```
This will spawn <N> new terminal(s) for missing IDs.
```

새:
```
This will spawn <N> new terminal(s) for missing IDs.
Note: New terminals start fresh — previous output cannot be restored.
```

→ 시각 표현 (italic, color, icon 등) 은 FE-design 의 자율. *최소 변경* 으로 시작 가능.

### C-4. Acceptance criteria

| # | 검증 명령 | 기대 결과 |
|---|---|---|
| AC-FB1 | `pnpm check` | 0 errors |
| AC-FB2 | `pnpm build` | 성공 |
| AC-FB3 | `grep -n "previous output cannot be restored" codebase/frontend/src/lib/chrome/AttachConfirmModal.svelte` | 1 hit |
| AC-FB4 | **manual E2E**: BE restart → FE silentReattach 시 stale UUID 발견 → AttachConfirmModal 표시 → 새 문구 노출 확인 | screenshot 캡처 |
| AC-FB5 | **manual E2E** (재시연): 사용자가 copy 변경된 modal 보고 *납득* 가능한지 본인 평가 | subjective OK |

### C-5. Anti-pattern

❌ **이런 fix 는 ship 거부**:

1. **modal 의 핵심 액션 변경**: copy 만 변경. button label, confirm 흐름, 호출 endpoint 등 logic 변경 0.
2. **번역 추가 (불완전)**: i18n hook 없는데 한글/영문 mix → 사용자 혼란. *영문 한 줄만*.
3. **공포 문구**: "**Warning**: ALL HISTORY IS LOST" 같은 과장 — 사용자 의도 차단. *중립 "Note:"*.
4. **fresh spawn 자체 차단**: copy 만으로 사용자 결정 보조. spawn 자체는 막지 않음.

### C-6. Self-check

- [ ] AttachConfirmModal copy 한 줄 추가
- [ ] AC-FB1 ~ AC-FB5 모두 PASS
- [ ] (선택) ADR-0018 D6 또는 ADR-0021 D10 의 amend 동봉
- [ ] commit message 가 "FE-B AttachConfirmModal history loss notice" + 0071 §B-2(a) anchor

---

## §D. Task FE-C — `lib/lifecycle/leaveBeacon.ts` + page lifecycle bind

### D-1. Trigger

0071 §D-5: BE 측 `POST /api/leave` (BE-B) 의 FE 짝. 정상 탭 close 시 즉시 lock release.

**전제**: BE-B 의 `/api/leave` endpoint 가 먼저 ship 되어야 함. 안 ship 된 상태에서 본 task 진행 시 sendBeacon 이 404 받음 (no-op, 사용자 영향 0이나 무용한 호출).

### D-2. Anchor — 신규 module + bind 위치

| 파일 | 변경 |
|---|---|
| `codebase/frontend/src/lib/lifecycle/leaveBeacon.ts` (신규) | sendBeacon helper + bind/unbind 함수 |
| `codebase/frontend/src/routes/+page.svelte` `onMount` | `leaveBeacon.bind()` 호출 |
| `codebase/frontend/src/routes/+page.svelte` `onDestroy` | `leaveBeacon.unbind()` 호출 |

### D-3. 의사 코드

```typescript
// codebase/frontend/src/lib/lifecycle/leaveBeacon.ts

import { getWebpageId } from '$lib/session/webpageId';

/**
 * `beforeunload` + `pagehide` 시 `POST /api/leave?webpage_id=<id>` 를
 * `navigator.sendBeacon` 으로 발화 — page unload 시 BE attach lock 의
 * 즉시 release (ADR-0021 D6 amend ②, 0071 §D-5).
 *
 * - sendBeacon 의 body 는 비어있음 (Blob length 0). Content-Type 은
 *   `text/plain;charset=UTF-8` 의 sendBeacon default.
 * - webpage_id 는 URL query — sendBeacon 의 custom header 제한 우회
 *   (ADR-0019 D5.6 의 WS query 패턴과 정합).
 * - beforeunload 와 pagehide 둘 다 listen — beforeunload 가 안 fire 하는
 *   iOS Safari + page cache 진입 케이스도 cover (Q5 결정).
 * - best-effort: sendBeacon 의 return value (boolean) 무시. 30s heartbeat
 *   fallback 이 안전망.
 *
 * **idempotent**: 두 listener 가 같은 cycle 에 fire 해도 BE 는 idempotent
 * (보유 lock 없으면 no-op). 중복 호출 안 막음.
 */

let bound = false;

function sendLeave(): void {
  if (typeof navigator === 'undefined' || typeof navigator.sendBeacon !== 'function') return;
  try {
    const webpageId = encodeURIComponent(getWebpageId());
    const url = `/api/leave?webpage_id=${webpageId}`;
    navigator.sendBeacon(url, new Blob([], { type: 'text/plain;charset=UTF-8' }));
  } catch (e) {
    // sendBeacon 호출 실패는 page unload 직전이라 toast / console.warn
    // 둘 다 의미 X. console.debug 만.
    console.debug('[gtmux] leaveBeacon: send failed', e);
  }
}

export function bind(): void {
  if (bound) return;
  if (typeof window === 'undefined') return;  // SSR/test guard
  window.addEventListener('beforeunload', sendLeave);
  window.addEventListener('pagehide', sendLeave);
  bound = true;
}

export function unbind(): void {
  if (!bound) return;
  if (typeof window === 'undefined') return;
  window.removeEventListener('beforeunload', sendLeave);
  window.removeEventListener('pagehide', sendLeave);
  bound = false;
}
```

`+page.svelte` 의 변경:

```svelte
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import * as leaveBeacon from '$lib/lifecycle/leaveBeacon';
  // ...
  onMount(() => {
    leaveBeacon.bind();
    // ... 기존 onMount
  });
  onDestroy(() => {
    leaveBeacon.unbind();
    // ... 기존 onDestroy
  });
</script>
```

### D-4. Acceptance criteria

| # | 검증 명령 | 기대 결과 |
|---|---|---|
| AC-FC1 | `pnpm check` | 0 errors |
| AC-FC2 | `pnpm build` | 성공 |
| AC-FC3 | `grep -rn "sendBeacon\|leaveBeacon" codebase/frontend/src --include='*.ts' --include='*.svelte'` | ≥ 3 hits (helper module + bind/unbind call site) |
| AC-FC4 | **manual E2E**: BE demo running + FE attach 후 BE 의 `cargo build --release && cargo run -- ...` 의 stderr 모니터. 탭 close → BE log 에 `release_lock_for_owner` 호출 trace | log 1줄 |
| AC-FC5 | **prod bundle DCE 검증**: `grep "sendBeacon" codebase/frontend/dist/assets/*.js` | 1+ hit (실제 코드 inline) |
| AC-FC6 | **manual E2E (negative)**: BE-B 미 ship 된 환경 (404 응답)에서도 page 가 깨지지 않음 (sendBeacon 실패 silent) | console.debug 만, error 0 |

### D-5. Anti-pattern

❌ **이런 fix 는 ship 거부**:

1. **fetch 사용**: `fetch('/api/leave', {keepalive: true})` 는 browser 마다 keepalive 지원 차이 — sendBeacon 의 명시 spec 이 더 신뢰. **sendBeacon 만 사용**.
2. **listener 중복 bind**: page navigation 시 onMount 가 다시 호출되어 bind 중복 → leak. `bound` flag 로 dedup.
3. **beforeunload 만 listen**: Q5 결정 위반. iOS Safari 케이스 누락. **pagehide 도 listen**.
4. **prompt 추가**: `beforeunload` 에 `event.preventDefault()` + `returnValue` 추가 시 사용자에게 "Are you sure?" prompt — 본 task 의 의도와 무관. *추가 안 함*.
5. **detachSession 호출**: `fetch(DELETE /attach, ...)` 호출 시 unload 의 race 로 무용. **sendBeacon 만**.
6. **webpage_id 누락**: query 에 `?webpage_id=...` 없으면 BE 가 cookie-only owner_key 만 release — 다른 webpage 의 lock 가 동일 cookie 라 *과도 release* 위험. **query 필수**.
7. **alpine helper module 위치**: `lib/session/` 안 위치 — session lifecycle 과 page lifecycle 의 책임 분리. **`lib/lifecycle/` 위치 필수** (Q6 결정).

### D-6. Self-check

- [ ] `lib/lifecycle/leaveBeacon.ts` 신규
- [ ] `+page.svelte` 의 `onMount` / `onDestroy` 에 bind/unbind 추가
- [ ] AC-FC1 ~ AC-FC6 모두 PASS
- [ ] BE-B (`/api/leave` endpoint) 가 ship 됐는지 확인 (또는 본 commit 보류)
- [ ] commit message 가 "FE-C leaveBeacon lifecycle bind" + 0071 §D-5 anchor

---

## §E. Task FE-D — AttachConfirmModal cancel chain toast verify (manual E2E only)

### E-1. Trigger

0071 §D-4: 0069 보고서가 명시한 *"8s warning toast — 'Attach cancelled, but previous session could not be restored'"* 의 실 출력 코드 verification 안 함. 본 verify task 는 코드 변경 없을 가능성 — 단순 manual E2E.

### E-2. 검증 시나리오

| Step | 행동 | 기대 |
|---|---|---|
| 1 | BE running + FE main page 진입 + session α attach 성공 | sessionStore.active = α |
| 2 | (시뮬레이션) BE process kill (Ctrl+C 또는 kill -9) | FE WS disconnect detected |
| 3 | BE 재기동 (같은 workspace) | terminal_map fresh, attach_index rebuild from disk |
| 4 | FE 가 silentReattach 시도 (visibility/WS reconnect trigger) | POST /attach 결과 = 200 + unmatched>0 (stale UUID 발견) |
| 5 | FE 가 AttachConfirmModal 진입 | modal 표시 (sessionStore.active 는 α 그대로) |
| 6 | 사용자 다른 session β 를 SessionListModal 에서 attach 시도 (옵션: 또는 직접 [Cancel]) | tryAttach 가 confirm_required 반환 → AttachConfirm 또 진입 |
| 7 | AttachConfirmModal [Cancel] 클릭 | `cancelAttachConfirm` 5-step chain 발화 |
| 8 | step 2 의 `restorePreviousSession(α)` 호출 → α 가 또 confirm_required → step 3 recursive 진입 또는 throw | 분기 따라 toast 또는 chain 재진입 |
| 9 | step 4 의 failure fallback 분기 발화 시 `sessionStore.clear()` + **8s warning toast 표시** | toast 정확히 노출 + 8s 후 자동 dismiss |

### E-3. Acceptance criteria

| # | 검증 | 기대 |
|---|---|---|
| AC-FD1 | step 7 의 [Cancel] 직후 console.log 로 cancelAttachConfirm 진행 trace 확인 | 5-step 의 각 step trace 1줄씩 |
| AC-FD2 | step 9 의 toast 가 *실 시각으로* 노출됨 (screen capture) | screenshot 첨부 |
| AC-FD3 | toast 의 정확한 copy 가 `"Attach cancelled, but previous session could not be restored: <reason>"` | 정확 매치 |
| AC-FD4 | toast 가 8s 후 자동 dismiss | timer 측정 |
| AC-FD5 | toast 미노출 시 → 0069 보고서의 명시 vs 실제 코드 mismatch → **report 0074 작성** (별 follow-up) | report 또는 PASS |

### E-4. Self-check

- [ ] 위 9-step 시나리오 실행
- [ ] AC-FD1 ~ AC-FD5 모두 결과 기록
- [ ] toast 실 출력 OK → 0071 §D-4 closed, 본 handover 안 closed 표시
- [ ] toast 미출력 → 별 follow-up report `0074-attachconfirm-cancel-toast-regression.md` 발주

---

## §F. Task FE-E — Rebind history replay 부재 시연 (manual E2E only)

### F-1. Trigger

0071 §B-2.4: ADR-0021 D8 [Attach existing terminal] 흐름에서 alive terminal 의 ring buffer 가 *layout PUT 후* replay 되지 않을 가능성 — 코드 경로만 식별, 재현 시연 안 함. 본 verify task 는 *재현 시도* — 재현되면 B-2(b) 의 진짜 결함 confirmed.

### F-2. 검증 시나리오

| Step | 행동 | 기대 |
|---|---|---|
| 1 | BE running + FE 의 session α attach | session α active |
| 2 | session α 에 [New Terminal] → terminal T spawn → some output (e.g., `echo hello && ls`) | T 의 ring buffer 에 N bytes |
| 3 | 다른 탭 (다른 webpage_id) 으로 session β attach | β active in 다른 탭 |
| 4 | β 에서 TerminalListView → T 의 [Attach to this session] 클릭 | layout PUT 으로 β layout 에 panel item 추가 (`terminal_id: T`) |
| 5 | β 측 화면에서 panel 이 mount + xterm 표시 | xterm 인스턴스 alive |
| 6 | **β 측 xterm 가 T 의 기존 output (`hello` + `ls` 결과) 을 표시하는가?** | **이게 핵심 측정** |
| 7 | 추가 검증: β 에서 `echo from-beta` 입력 → α 측 xterm 도 같은 output 표시 | live mirror 정상 |

### F-3. 가능한 결과

- **(a) β 가 history 표시** → 코드 경로에 replay 가 *어디선가* 있음. 0071 §B-2.4 의 가설 reject. 안심.
- **(b) β 가 history 미표시** → 0071 §B-2.4 의 가설 confirmed. **별 follow-up report `0075-rebind-history-replay-missing.md`** 발주 + B-2(b) 의 ADR amend draft 진행.

### F-4. Acceptance criteria

| # | 검증 | 기대 |
|---|---|---|
| AC-FE1 | 시나리오 7-step 완료 | screen 캡처 첨부 |
| AC-FE2 | step 6 의 결과 (a)/(b) 명시 | 본 handover § F-5 에 기록 |
| AC-FE3 | (b) 인 경우 별 report 발주 | report 파일 작성 |

### F-5. 결과 기록 (manual E2E 후 채움)

- [ ] 시나리오 시연 완료
- [ ] 결과: ☐ (a) replay OK / ☐ (b) replay 미표시
- [ ] (b) 시 follow-up report 발주

---

## §G. 통합 검증 — 5 task land 후

### G-1. Frontend pnpm check + build

```bash
cd codebase/frontend
pnpm check     # 기대: 0 errors, 0 warnings
pnpm build     # 기대: 성공
```

### G-2. Production bundle 검증

```bash
cd codebase/frontend
grep -c "sendBeacon\|leaveBeacon" dist/assets/*.js    # 기대: 1+
grep -c "previous output cannot be restored" dist/assets/*.js    # 기대: 1+
grep -c "tentative detach" dist/assets/*.js          # 기대: 0 (주석은 build 시 strip)
```

### G-3. dev server boot 확인

```bash
cd codebase/frontend
pnpm dev &
sleep 3
curl -s http://localhost:5173 | head -1    # 기대: HTML 200
kill %1
```

### G-4. browse CLI E2E

```bash
/Users/ws/Desktop/projects/termcanvas/dist-cli/browse goto http://localhost:9998/auth?t=<demo_token>
/Users/ws/Desktop/projects/termcanvas/dist-cli/browse snapshot
# 본 시점 SPA 가 정상 mount + ReconnectModal 가 표시되지 않으면 (idle), AuthDialog → SessionListModal 흐름 진입 확인
```

### G-5. ADR 정합 확인

- [ ] ADR-0019 D5.4 amend ② (FE-A) 동봉
- [ ] ADR-0018 D6 또는 ADR-0021 D10 의 amend (FE-B, 선택)
- [ ] state-machines.md §3.3 의 ReconnectModal 행 갱신

### G-6. 거짓 ship 방지 — final cross-check

| 검증자 | 명령 | 통과 기준 |
|---|---|---|
| **본인 (구현자)** | G-1 + G-2 + G-3 | 모두 PASS |
| **AI reviewer** | 본 handover 의 §B-7, §C-6, §D-6 의 self-check 모두 ☑ | ☐ 한 개라도 남으면 ship 보류 |
| **manual E2E** | §B-5 AC-FA4~FA6, §C-4 AC-FB4~FB5, §D-4 AC-FC4~FC6, §E (FD), §F (FE) | screenshot 또는 log 첨부 |

---

## §H. Commit 분리 권장

| Commit | 내용 |
|---|---|
| `fix(fe/stores): FE-A reconnectGate.cancel tentative detach (0071 §B-1)` | FE-A 코드 + import + ADR-0019 D5.4 amend ② + state-machines.md §3.3 갱신 |
| `feat(fe/chrome): FE-B AttachConfirmModal history loss notice (0071 §B-2a)` | FE-B copy + (선택) ADR amend |
| `feat(fe/lifecycle): FE-C leaveBeacon on unload (0071 §D-5)` | FE-C 새 module + +page.svelte bind/unbind. BE-B 가 ship 된 후 |
| `docs(reports): FE-D AttachConfirmModal cancel toast verify (0071 §D-4)` | 코드 변경 0 — 본 handover 의 §E-5 결과 기록 또는 follow-up report 발주 |
| `docs(reports): FE-E rebind history replay verify (0071 §B-2.4)` | 코드 변경 0 — 본 handover 의 §F-5 결과 기록 또는 follow-up report 발주 |

→ FE-A / FE-B 는 BE 영향 0, 독립 land 가능. FE-C 는 BE-B 와 짝.

---

## §I. 본 handover 가 *완전 검증* 안 한 영역

다음은 본 handover 의 task 범위 밖. 후속 session 또는 별 handover 에서 처리:

- **B-2(b)** rebind history replay 의 *코드 fix* — FE-E 의 verify 결과에 따라 BE+FE 짝 fix 또는 ADR amend draft 만
- **D-2** ADR-0025 amend ③ race 잔여 — 현 상태 OK
- **D-3** WS subprotocol 의 webpage_id 동봉 — 현 query 만으로 충분

---

## 변경 이력

- 2026-05-18: 초안. 0071 감사의 §B-1, §B-2(a), §D-5, §D-4, §B-2.4 의 FE-side land 를 위한 FE-A/B/C/D/E 5 task. Self-grilling 7 Q resolve. 각 task 에 anchor + acceptance criteria + manual E2E + anti-pattern + self-check.
