# 0071 — Session·Terminal·Panel·History 라이프사이클 정합 감사

- 작성일: 2026-05-18
- 작성 주체: agent (system-architect role, root-cause-analyst pass)
- 범위: BE (Rust http-api + ws-server + auth + pty-backend) ↔ FE (Svelte chrome + stores + ws + http) ↔ ADR (0019/0020/0021/0025) ↔ SSoT (`state-machines.md`) 정합
- 트리거: 사용자 보고 — "session 관리, terminal, 연결된 panel 관리, terminal history 미표시 문제가 매우 심각하게 꼬여있다. 여러 예외 시나리오(예: session 이 연결된 webpage 가 존재하는 상태에서 server 가 죽는 ghost 시나리오)에 영향받지 않도록 설계해야"
- 정본 cross-link:
  - `docs/ssot/state-machines.md` — 3-layer (Auth / Session attach / Terminal) 합성 SoT
  - `docs/adr/0019-session-and-workspace-model.md` — D3 single-attach, D4 no-takeover, D5.5 tentative attach, D5.6 webpage owner
  - `docs/adr/0020-auth-lifecycle.md` — cookie 정책, in-memory SessionTable
  - `docs/adr/0021-terminal-pool-and-mirror.md` — D6 heartbeat, D7 amend ③ attach_index, D10.x lifecycle
  - `docs/adr/0025-session-scoped-pane-output-filter.md` — D1 amend ③ catch-up replay filter
- 직전 관련 보고:
  - `0069-session-attach-confirm-cancel-recovery.md` (D5.5 신규의 land)
  - `0070-webpage-owner-session-list-regression.md` (D5.6 보완)

---

## 0. Executive summary

| 결론 | 수 | 분류 |
|---|---|---|
| **실제 정합 결함 (P0)** | 2 | B-1 reconnectGate.cancel 의 tentative lock leak / B-2 "terminal history 미표시" by-design 함정 |
| **Naming debt (P0 maintenance 안정성)** | 1 | C-1 BE 전역의 `cookie` 변수명이 실제로는 owner_key 를 담음 — 본 감사 자체가 false-positive 양산 |
| **P1·P2 후속** | 6 | D-1 ~ D-6 |
| **검증된 invariants (변경 불요)** | 8 | F-1 ~ F-8 |
| **본 감사가 reject 한 agent 의 false-positive 보고** | 6 | §A 표 |

**핵심 메시지**: 코드 자체의 라이프사이클 정합은 ADR 의도와 *대체로* 일치한다. 사용자가 체감한 "꼬임" 의 주된 원인은 (1) **fresh-spawn 후 history 가 없음** 이라는 by-design fact 가 UX 에 노출 안 됨, (2) **`reconnectGate.cancel()` 의 lock 누락** 의 30s 회수 지연, (3) **BE naming debt 가 정합 인지를 방해** 의 3 축이 동시에 작용했을 가능성이 가장 크다.

---

## §A. Agent 1차 보고 — verification 후 reject 된 항목

본 감사에서 두 Explore agent (BE + FE) 가 1차 보고한 항목 중, 코드 직접 검증으로 *false-positive* 판명된 항목들. 다음 감사·점검에서 같은 오인을 피하기 위해 명시 기록.

| Agent claim | 검증 결과 | 검증 근거 |
|---|---|---|
| BE: `attach_index` 의 mutation hook 4 개 중 3 개 (import / delete-session / delete-item) 누락 | **거짓** | `grep -n "apply_diff\|apply_full_session\|forget_session" codebase/backend/crates/http-api/src/sessions.rs` → 4 hook 모두 wire 확인: `apply_full_session`(1006 import), `forget_session`(1221 delete-session), `apply_diff`(1315 delete-item, 1588 PUT) |
| BE: WS handler 가 `hub.session_for_cookie(cookie-only)` 호출 → multi-tab session 격리 깨짐 | **거짓 (naming 혼선)** | `ws-server/lib.rs:484` 가 closure 에 `owner_key` 를 넘기지만 `handle_socket` 의 param 이름이 `cookie_value` 라 agent 오인. `handle_socket:577` 의 `hub.session_for_cookie(cookie)` 의 `cookie` 는 실제 owner_key. `session_locks_by_cookie` 의 key 도 owner_key (`attach_handler:520`) 이라 정합 |
| BE: heartbeat 가 cookie-only 로 lookup → multi-webpage collision | **거짓 (naming 혼선)** | `ws-server/lib.rs:741/745` `emit_heartbeat(&heartbeat_sink, cookie_value.as_deref())` → `bin/gtmux-cli/src/main.rs:474` `state_for_heartbeat.refresh_lease_for_cookie(&cookie)` — 양쪽의 `cookie` 모두 owner_key |
| FE: `getWebpageId()` 가 매 호출 새 UUID return → 같은 탭 내 불안정 | **거짓** | `webpageId.ts:14-15` 가 `sessionStorage.getItem` 이 있으면 existing 반환. 첫 호출만 mint+set, 이후 stable. `mintWebpageId()` 가 매번 호출되는 비효율은 있지만 결과는 stable (sessionStorage 의 setItem 이 throw 안 하는 정상 환경) |
| FE: `listCloseTarget` 분기 미구현 | **거짓** | `workspaceSwitcher.svelte.ts:30,48,55-56` 완전 구현. `closeList()` 가 `listCloseTarget === 'closed'` 이면 close, 그 외 choice 회귀 |
| FE: `confirmAttach` 의 `setActiveSession` 이 `getLayout` 결과 적용 전에 호출 → race | **거짓** | `WorkspaceSwitcher.svelte:174-176` 순서 정상: `await getLayout` → `setActiveSession` → `loadLayout`. `getLayout` throw 시 setActiveSession 미호출 (try-catch 의 fall-through) |

→ **6 건 중 5 건이 naming debt 의 부산물.** §C-1 의 refactor 가 미래 점검의 false-positive 율을 결정적으로 낮춘다.

---

## §B. 실제 결함 (P0)

### B-1. `reconnectGate.cancel()` 가 tentative attach 의 BE lock 을 release 안 함

#### B-1.1 위치

`codebase/frontend/src/lib/stores/reconnectGate.svelte.ts:111-116`

```typescript
cancel(): void {
  this.#controller?.abort();
  this.#controller = null;
  this.markIdle();
  sessionStorageHint.clear();
  // ❌ DELETE /api/sessions/<attemptName>/attach 호출 없음
}
```

#### B-1.2 시나리오 (재현 가능)

1. 사용자 브라우저: `sessionStorage.gtmux-last-active-session = "alpha"`
2. 페이지 reload → `AppPage.onMount` 가 hint 발견 → `reconnectGate.start("alpha")` → 내부적으로 `attemptReattach("alpha", signal)` → `POST /api/sessions/alpha/attach`
3. BE `attach_handler` 가 flock 잡고 lease body 쓰는 도중 또는 200 응답 직전
4. 사용자가 ReconnectModal 의 [Switch session…] 클릭
5. `cancel()` 호출 → `AbortController.abort()` → in-flight fetch 취소
6. **BE 는 이미 200 응답 직전이거나 응답을 보냈는데 FE 가 못 받은 상태** — 서버 측에서 flock + `session_locks_by_cookie[owner_key] = "alpha"` 잡혀있음
7. FE 는 SessionListModal 로 이동 (state-machines.md §3.3 의 `'choice'` routing)

#### B-1.3 영향

- **자가 회복 케이스 (다행)**: 사용자가 같은 owner_key (= 같은 탭 reload 안 함) 로 `"alpha"` 다시 시도 → `sessions.rs:461-466` 의 `reuse_existing_attach_response` 가 idempotent 200 반환
- **회복 안 되는 케이스 (문제)**: 사용자가 **다른 webpage_id 의 새 탭** 으로 `"alpha"` 진입 시도 → 409 conflict + "in use" badge → 30s 의 heartbeat timeout 까지 (D6.2 의 `release_lock_for_cookie`) lock 잔존. 사용자 perception: "내가 cancel 했는데도 session 이 잡혀있다"
- **회복 안 되는 케이스 (보안)**: cookie 만료 직전 reconnect 시 cancel → cookie 가 곧 invalid → heartbeat 의 owner-key 와 매핑 실패 → 30s 보다 더 오래 lock 잔존 가능 (작은 race)

#### B-1.4 대비 사례

`WorkspaceSwitcher.svelte:215-250` 의 `cancelAttachConfirm` 은 명시적으로 `detachSession(pending)` 호출 (state-machines.md §3.2.1 의 step 1, ADR-0019 D5.5.1 의 step 1). reconnectGate 만 이 패턴에서 누락.

#### B-1.5 ADR 정합성

ADR-0019 D5.4 본문 + state-machines.md §3.3 의 "ReconnectModal [Switch session…]" 행은 **명시 cancel = lock release 의도** 가 직접 명시되어 있지 않음. `reconnectGate.svelte.ts:13-19` 주석은 `"cancel() 안에서 sessionStorageHint.clear() 호출"` 만 명시. → **이 비-amend 가 결함의 근원**.

#### B-1.6 권장 fix

1. **(즉시 land, FE-only)** `reconnectGate.cancel()` 에 `state === 'attaching' && attemptName !== null` 분기에 best-effort `detachSession(attemptName)` 호출. 실패는 silent — 30s heartbeat fallback. 비동기 fire-and-forget OK.

   ```typescript
   async cancel(): Promise<void> {
     this.#controller?.abort();
     this.#controller = null;
     const wasAttaching = this.state === 'attaching' && this.attemptName !== null;
     const tentativeName = this.attemptName;
     this.markIdle();
     sessionStorageHint.clear();
     if (wasAttaching && tentativeName) {
       try {
         const { detachSession } = await import('$lib/http/sessions');
         await detachSession(tentativeName);
       } catch {
         /* silent — heartbeat 30s fallback */
       }
     }
   }
   ```

2. **(짝 commit, ADR amend)** ADR-0019 D5.4 amend ② — "사용자 명시 cancel = tentative lock release. detach 실패는 30s heartbeat 가 fallback." 명시. state-machines.md §3.3 의 ReconnectModal 행에 같은 의미 보강.

---

### B-2. "terminal history 미표시" — by-design 함정 + UX 미고지

#### B-2.1 근본 원인

PaneId 는 *process-bound*. 서버 재시작 시:
- 모든 PaneId 가 fresh (`pty-backend` 가 monotonic counter)
- 옛 process 의 ring buffer 가 OS-level 로 사라짐 (process kill = memory free)
- terminal_map 의 UUID↔PaneId binding 도 in-memory 라 사라짐
- AttachConfirm 흐름이 새 UUID→PaneId 를 새 process 로 binding 하지만 **새 process 의 ring buffer 는 0 bytes**

#### B-2.2 현 동작 (verified)

`ws-server/lib.rs:613-686` 의 catch-up replay 흐름:

```
1. TerminalSpawned 0x88 burst (모든 alive (PaneId, UUID) pair) — FILTER 없음
2. session_pane_set 가 cookie-attached 면 cold-load (lib.rs:573-583)
3. for each id in backend.pane_ids():
     if !session_pane_set.contains(id.0) { continue; }    ← amend ③ filter
     send NOTIFY pane-spawned
     send PANE_OUT (ring buffer replay, backend.subscribe_output(id).0)
```

- 서버 재기동 직후: `backend.pane_ids()` 가 empty → catch-up replay 가 **단 1 byte 도 안 보냄**
- AttachConfirm 후 respawn 시점: live broadcast 채널 `terminal_spawned_events` 가 새 (UUID, PaneId) 를 emit → FE 의 terminalPool 가 binding 추가 → panel 이 xterm mount → **새 process 의 fresh shell**

#### B-2.3 왜 "꼬여보이는가" (사용자 mental model)

- 사용자: "session 은 file 영속이니 history 도 영속이어야지" — ADR-0019 D5 의 *session file 영속* 과 *terminal process 영속* 의 분리를 인지 못 함
- ADR-0021 D10 (lifecycle) 은 process die = history die 를 명시. 그러나 **UX layer 에서 이 사실이 노출 안 됨**
- AttachConfirmModal 의 현 copy 는 "Will spawn N new terminal(s). Continue?" 정도 — *"이전 출력은 복원 불가"* 라는 경고 없음

#### B-2.4 잠재적 부가 원인 (verification 필요)

ADR-0021 D8 의 [Attach existing terminal] (Terminal list → 현 session 으로 mount):
- 이 경우 PaneId 는 alive — ring buffer 가 BE 메모리에 있음
- 사용자 클릭 → FE PUT /layout 로 새 panel item (`{type:'terminal', terminal_id: '<existing-uuid>'}`) 추가
- BE 가 layout_events broadcast → 모든 WS 의 `session_pane_set` hot-update 가 PaneId 추가
- **그러나 `backend.subscribe_output()` 의 *replay* tuple 은 catch-up 시점에만 보내짐** — hot-update path 에는 ring buffer 의 *그 시점 replay* 가 포함 안 됨
- 사용자 perception: "이 terminal 에 분명 history 가 있는데 attach 후 빈 화면이다"

이게 verify 되면 두 번째 P0 — 코드 fix 가 필요한 진짜 결함. 본 감사에서 *재현* 까지는 못 했고 *코드 경로만* 식별. 다음 점검 또는 manual E2E 에서 확인 필요.

#### B-2.5 권장 fix

**옵션 (a) — UX-only (저비용)**

- AttachConfirmModal 의 copy 에 명시 경고: "*기존 terminal 들은 새 process 로 spawn 됩니다 — 이전 output 은 복원되지 않습니다.*"
- ADR-0018 D6 (match-or-spawn) 의 *fresh spawn* arm 본문에 같은 사실 명시
- ADR-0021 D10 의 *lifecycle* 본문에 "history = process lifetime" 의 invariant 명시

**옵션 (b) — Attach-time ring replay (B-2.4 의 부가 원인 직접 해결)**

ADR-0021 D8 amend — [Attach existing terminal] 흐름에 *그 시점의 ring buffer 1회 replay* 추가:

- FE: PUT /layout 후 *직접* `POST /api/terminals/<uuid>/replay` 호출 (또는 PUT 응답에 replay envelope 동봉)
- BE: 새 endpoint 가 `terminal_map.lookup_pane(uuid)` → `backend.subscribe_output(pane_id)` 의 `replay` tuple 반환 → FE 가 xterm.write 로 prepend
- 또는 BE 가 PUT /layout 의 diff 의 `added` 분기에 *그 layout 의 새 attached uuid 의 replay* 를 layout_events broadcast 에 동봉

**옵션 (c) — Auto-replay on layout PUT (구현 단순, 비용 측정 필요)**

`http-api/sessions.rs::put_layout_handler` 의 attach_index.apply_diff 직후, `added` UUIDs 의 ring buffer 를 그 session 의 WS 로 emit. 같은 PUT 의 broadcast 흐름에 piggy-back. 단점: 모든 layout PUT 에 ring buffer copy 비용 — frequent drag PUT 에서 부담. *오직 첫 추가만* trigger 의 dedup 필요.

권장: **(a) 즉시 land + (b) ADR amend draft + 사용자 결정 후 구현.**

---

## §C. Naming debt (P0 maintenance 안정성)

### C-1. BE 전역 변수/함수명이 owner_key 를 `cookie` 라 부름

`§A` 의 false-positive 6 건 중 5 건이 본 naming debt 에 기인. 6 개월 후 reviewer 가 "왜 cookie 단위로 lookup 해? multi-tab 깨질 텐데" 라며 **불필요한 fix 시도하다 실제 정합을 깰** 위험이 매우 높다.

#### C-1.1 영향 받는 정의 (BE)

| 파일:line | 변수/함수 | 실제 의미 |
|---|---|---|
| `http-api/lib.rs:164` (선언) | `session_locks_by_cookie: Mutex<HashMap<String, String>>` | key 는 owner_key |
| `http-api/lib.rs:272` | `pub async fn refresh_lease_for_cookie(&self, cookie: &str)` | param 은 owner_key |
| `http-api/lib.rs:326` | `pub async fn release_lock_for_cookie(&self, cookie: &str)` | 같음 |
| `ws-server/lib.rs:415-425` | `let cookie_value = ... gtmux_auth 추출` | 여기까지는 진짜 cookie |
| `ws-server/lib.rs:426-431` | `let owner_key = cookie_value.as_ref().map(...)` | 여기서 owner_key 형성 |
| `ws-server/lib.rs:484` | `if let Some(cookie) = owner_key { sink.send(cookie) }` | shadowing — `cookie` 가 owner_key |
| `ws-server/lib.rs:532` | `async fn handle_socket(..., cookie_value: Option<String>, ...)` | **param 은 owner_key** (호출 site 가 owner_key 전달) |
| `ws-server/lib.rs:577` | `hub.session_for_cookie(cookie)` | arg = owner_key |
| `bin/gtmux-cli/src/main.rs:468` | `state_for_disconnect.release_lock_for_cookie(&cookie)` | `cookie` 변수 = owner_key |

#### C-1.2 권장 refactor

별 1 commit, mechanical, behavior change 0:

| 옛 이름 | 새 이름 |
|---|---|
| `session_locks_by_cookie` | `session_locks_by_owner` |
| `release_lock_for_cookie` | `release_lock_for_owner` |
| `refresh_lease_for_cookie` | `refresh_lease_for_owner` |
| `handle_socket(..., cookie_value, ...)` | `handle_socket(..., owner_key, ...)` |
| WS 의 `cookie` shadow → `owner` |  |
| `Hub::session_for_cookie` | `Hub::session_for_owner` (또는 docstring 만 보강) |
| `Hub::set_session_for_cookie` | `Hub::set_session_for_owner` |
| `Hub::clear_session_for_cookie` | `Hub::clear_session_for_owner` |

또는 가장 가벼운 옵션: **변수명만 refactor + public API 는 유지 + docstring 에 `"@param cookie: 실제로는 cookie + 0x1f + webpage_id 형태의 owner_key. ADR-0019 D5.6 참조."` 한 줄씩 보강**. 점진적 개선.

#### C-1.3 ADR 정합

ADR-0019 D5.6 본문은 "owner key" 개념을 명시하지만 *코드 안 변수명* 까지는 강제 안 함. 본 amend 를 ADR-0019 D5.6 amend ② 로 "code symbol naming = owner_key 통일" 라인 추가 권장.

---

## §D. P1·P2 후속

| # | 항목 | 영역 | 영향도 | 권장 |
|---|---|---|---|---|
| **D-1** | boot-time stale `.lock` 파일 scan 없음 | BE | functional 0 (peek 동작 정상), 시간 누적 시 `<workspace>/.locks/` dirty | `with_workspace()` 안에 `for each session in enumerate, if peek == Stale: unlink_stale` 추가. P2 housekeeping |
| **D-2** | ADR-0025 amend ③ 의 race-1/race-2 잔여 | BE | 단일 reconnect 의 history loss → D3 hot-update 자가 복구. amend 본문 의 risk 평가표가 이미 명시 | 추가 fix 불요. 현 상태 OK |
| **D-3** | WS handshake 가 `webpage_id` query 만 인식 | BE | `ws-server/lib.rs:506-522 webpage_id_from_query` 만. WS subprotocol 에는 미동봉 | 현 OK (HTTP 는 header, WS 는 query 의 분리 가 자연). amend 불요 |
| **D-4** | AttachConfirmModal cancel chain 의 toast 실 출력 verification | FE | 0069 report 가 8s warning toast 명시. 코드 직접 확인 안 함 | manual E2E 또는 다음 코드 review 에서 verify |
| **D-5** | `POST /api/leave` sendBeacon endpoint 부재 | BE | `grep -rn "/api/leave\|leave_handler"` 결과 0 건. ADR-0021 D6 의 "best-effort sendBeacon" 미구현 | P1 amend — `POST /api/leave` 추가 + FE 의 `beforeunload` 에 `navigator.sendBeacon('/api/leave')`. 정상 탭 close 의 즉시 회수 |
| **D-6** | multi-webpage rebind 의 history replay 부재 (B-2.4 의 부가 원인) | BE+FE | ADR-0021 D8 amend 필요. *재현 verify 후* P1 또는 P0 결정 | B-2 의 옵션 (b) 와 동일 |

---

## §E. 권장 land 순서

1. **C-1 naming refactor** (BE-only, mechanical, 1 commit + ADR-0019 D5.6 amend ②) — 점검·리뷰의 false-positive 감소가 다른 모든 land 의 토대
2. **B-1 reconnectGate.cancel detach** (FE-only, 1 commit + ADR-0019 D5.4 amend ②) — orphan lock 회피
3. **B-2 옵션 (a) AttachConfirmModal copy** (FE-only, 1 commit + ADR-0018 D6 / ADR-0021 D10 amend) — history 손실 UX 함정 해소
4. **D-4 manual E2E** — AttachConfirmModal cancel chain 의 toast 실 출력 verify
5. **B-2 옵션 (b) ADR amend draft** → 사용자 결정 → 구현
6. **D-5 `/api/leave` sendBeacon** (BE+FE pair commit, ADR-0021 D6 amend) — 정상 탭 close 의 즉시 회수
7. **D-1 boot-time stale lock scan** (BE only, 1 commit) — housekeeping

---

## §F. 검증된 invariants (변경 불요)

1. **ADR-0019 D3 single-attach**: `attach_handler` 의 idempotent 분기 (sessions.rs:461-466) + 409 분기 (472-475) 정합
2. **ADR-0019 D5.5 tentative lock**: `confirm_required` 응답 시 BE flock 잡힌 상태, `attach_confirm_handler` 가 owner-scope 검증 (sessions.rs:566+)
3. **ADR-0019 D5.6 webpage owner**: HTTP `X-Gtmux-Webpage-Id` header / WS `?webpage_id=` query — `attach_owner_key` / `webpage_id_from_query` 가 일관 적용
4. **ADR-0021 D7 amend ③ attach_index**: boot rebuild + 4 mutation hook 완비 (lib.rs:425, sessions.rs:1006/1221/1315/1588)
5. **ADR-0021 D10.3 respawn per-UUID Mutex**: `AppState.respawn_locks` + `reused` flag (terminals.rs:283+)
6. **ADR-0025 amend ③ session-scoped PANE_OUT filter**: cold-load + catch-up + 4 hot-update channels (ws-server/lib.rs:573-583, 641-647)
7. **ADR-0028 D11.1 applyMutation priorSnapshot rollback**: sessionStore.ts:619-678
8. **Session lock acquire 의 stale body overwrite**: session_lock.rs:200 `file.set_len(0)` 가 stale body 덮어쓰기 — SIGKILL 후 재기동의 자가 회복

---

## Appendix A. 본 감사의 조사 방법론

1. **Phase 1 — ADR/SSoT 정독**: state-machines.md (full) → ADR-0019 (full) → ADR-0020 (full) → ADR-0021 (full) → ADR-0025 (full). 합성 SoT 우선, 정본 ADR 으로 cross-check.
2. **Phase 2 — BE/FE 병렬 정찰**: Explore agent 2 개 spawn, ADR 명세 ↔ 코드 정합 1차 보고.
3. **Phase 3 — Verification**: agent 의 *고심각도 claim* 마다 `grep` + `Read` 로 직접 검증. False-positive 6 건 reject.
4. **Phase 4 — Synthesis**: 실 결함 / naming debt / by-design 함정 분류, ADR amend 후보 짝.

## Appendix B. 본 감사가 *완전 검증* 안 한 영역

- **D-4 cancel chain toast**: 0069 보고서가 명시한 8s warning toast 의 실제 출력 trace 안 함
- **B-2.4 rebind history replay**: 코드 경로만 식별, *재현 시연* 안 함. 다음 manual E2E 에서 확인
- **WS subprotocol 의 webpage_id 누락 영향**: HTTP 만 header, WS 는 query — 의도된 분리 같으나 *정합 의도* 의 ADR 명시는 없음
- **multi-cookie scenario**: 같은 호스트의 multi-user 가 아닌 multi-tab 만 검토. ADR scope 가 single-user 라 자연 OK 이지만 invariant 명시 없음
- **`/api/shutdown` 의 정상 종료 vs SIGKILL 차이**: 정상 종료 시 `LOCK_UN + unlink`, SIGKILL 시 kernel auto-release + body 잔존 — 두 path 의 boot 회복 검증 안 함

---

## §G. 후속 handover (2026-05-18 추가)

본 감사의 land 를 위해 BE/FE 분리 handover 발주:

| 영역 | Handover | 다루는 task |
|---|---|---|
| BE | [`0072-be-handover-from-0071-audit.md`](./0072-be-handover-from-0071-audit.md) | BE-A (C-1 naming refactor) / BE-B (D-5 `/api/leave`) / BE-C (D-1 boot stale scan) |
| FE | [`0073-fe-handover-from-0071-audit.md`](./0073-fe-handover-from-0071-audit.md) | FE-A (B-1 reconnect cancel detach) / FE-B (B-2a modal copy) / FE-C (D-5 FE sendBeacon) / FE-D (D-4 toast verify) / FE-E (B-2.4 rebind verify) |

각 handover 는 self-grilling 결정 + anchor + acceptance criteria + integration test + anti-pattern + self-check 표 동봉 — false-ship 방지.

---

## 변경 이력

- 2026-05-18: 초안 + verification. agent 보고 ↔ 코드 검증 cross-walk. 실 결함 2 + naming debt 1 + 후속 6 + invariants 8 + false-positive 6.
- 2026-05-18: §G 추가 — BE/FE 분리 handover (0072, 0073) cross-link.
