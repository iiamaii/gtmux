# 0072 — BE Handover: 0071 감사 결과의 BE 영역 land

- 작성일: 2026-05-18
- 작성 주체: agent (system-architect role) — 0071 감사 후속
- 정본 cross-link:
  - **상위 감사 보고**: [`0071-session-terminal-panel-lifecycle-audit.md`](./0071-session-terminal-panel-lifecycle-audit.md) (정독 필수 — §C-1, §D-5, §D-1 본 handover 의 trigger)
  - **paired FE handover**: [`0073-fe-handover-from-0071-audit.md`](./0073-fe-handover-from-0071-audit.md) (FE 측 짝 작업)
  - **관련 SSoT**: `docs/ssot/state-machines.md`
  - **관련 ADR**: ADR-0019 D5.4/D5.6, ADR-0021 D6, ADR-0006

## 핵심 원칙 — 거짓 ship 방지

본 handover 의 모든 task 는 다음 5 원칙을 따른다. 위반 시 **ship 거부**:

1. **Anchor 명시**: 모든 작업 위치는 *file:line* 으로 명시. 추측 금지.
2. **Acceptance criteria 가 검증 가능**: "잘 됨" 이 아니라 *grep / cargo test 명령 + 기대 출력* 으로 정의.
3. **Anti-pattern 명시**: "이렇게 하면 false ship" 의 패턴을 task 별 명시.
4. **Behavior change 0 보장 (refactor)** vs **신규 contract (신규 endpoint)** 분리.
5. **Self-check 표 통과**: handover 끝의 표 모든 항목 ☑ 되어야 commit. ☐ 한 개라도 남으면 ship 보류.

---

## 0. Self-grilling 결정 사항 (handover 작성 전 resolve)

본 handover 의 task 들이 land 전, 다음 결정 사항을 self-grilling 으로 resolve. 후속 reviewer 가 의문 제기 시 본 §0 가 1차 근거:

### Q1. C-1 refactor 의 범위 — public API 까지 변경?

**결정**: ✅ 변경. `Hub::session_for_cookie` 등 cross-crate public method 도 함께 rename. doc comment 도 함께 추가. 동시 commit. → behavior change 0 보장하면 외부 caller 가 같은 commit 안 함께 변경되어 깨짐 없음.

**거절**: 변수명만 shadow 변경 — method name 이 `cookie` 면 외부 caller 가 계속 헷갈림. naming debt 의 본질이 그대로.

### Q2. C-1 rename 시 옛 이름 deprecated alias 남길지?

**결정**: ❌ 남기지 않음. *Single-repo* 환경 + behavior change 0 의 mechanical refactor 라 caller 가 같은 commit 안 모두 갱신 가능. deprecated alias 가 오히려 명명 혼란 유지.

### Q3. D-5 `/api/leave` 의 webpage_id 전송 방법?

**결정**: ✅ **URL query**. `navigator.sendBeacon('/api/leave?webpage_id=...')`. 이유:
- `sendBeacon` 의 첫 인자는 URL → query string 으로 부수 정보 전달이 web platform-standard
- 이미 WS handshake (`?webpage_id=`) 와 동일 패턴 (ADR-0019 D5.6 명시)
- Body 는 무 또는 작은 blob — header (X-Gtmux-Webpage-Id) 는 `sendBeacon` 에서 지원 불가 (sendBeacon 은 fetch 가 아니라 custom header 제한)

### Q4. D-5 와 `DELETE /attach` 의 의미 차이?

**결정**: ✅ **wire 는 다르지만 BE 처리 함수는 같음**. `/api/leave` 는 *page unload best-effort* 의 sendBeacon 채널 보장. `DELETE /attach` 는 *명시 user action* 의 reliable channel. 둘 다 `release_lock_for_owner(owner_key)` 호출 → 동작 동일.

**거절**: 단일 endpoint 로 합치기 — sendBeacon 이 DELETE method 자체를 안전 지원 안 함 (일부 브라우저). POST 가 더 안전.

### Q5. D-5 가 rate-limited 인가?

**결정**: ❌ 적용 안 함. cookie auth 통과 + same-origin 가드 + sendBeacon 의 호출 빈도가 page unload 시점 1회 — brute-force 표면 0. P1+ 검토.

### Q6. D-1 boot-time stale lock scan 의 cleanup 정책 — fail-fast vs silent?

**결정**: ✅ **silent + log**. 한 .lock 의 unlink 실패 시 warn 로그만, boot 계속. 이유:
- stale lock unlink 는 *housekeeping* 이지 functional 의무 아님 — peek path 가 stale 인식 처리
- boot 가 단일 stale 파일 때문에 fail-fast 면 prod 안정성 손해
- session_lock.rs:157 의 기존 release 측 unlink 도 같은 warn-only 패턴

---

## §A. Task 목록

| Task | 영역 | 출처 | 예상 소요 | 우선 |
|---|---|---|---|---|
| **BE-A** | naming refactor (8 symbol) | 0071 §C-1 | 1-2 commit | **P0** — 다른 task 의 토대 |
| **BE-B** | `POST /api/leave` sendBeacon endpoint | 0071 §D-5 | 1 commit + FE 짝 | P1 |
| **BE-C** | boot-time stale `.lock` scan + unlink | 0071 §D-1 | 1 commit | P2 |

총 3 task. **BE-A 가 가장 먼저 land 되어야** BE-B/BE-C 도 새 이름 따름.

---

## §B. Task BE-A — Naming refactor (owner_key 통일)

### B-1. Trigger

0071 §C-1: BE 전역의 `cookie` 변수/함수명이 실제는 owner_key (= `cookie + 0x1f + webpage_id`) 를 담음. 본 감사가 false-positive 6 건 양산한 직접 원인. ADR-0019 D5.6 amend ② 짝.

### B-2. Anchor — 변경 대상 (정확한 file:line)

| # | 파일 | 옛 이름 | 새 이름 | 종류 |
|---|---|---|---|---|
| 1 | `codebase/backend/crates/http-api/src/lib.rs:164` | `session_locks_by_cookie: Arc<Mutex<HashMap<String, String>>>` | `session_locks_by_owner` | struct field |
| 2 | `codebase/backend/crates/http-api/src/lib.rs:272` | `pub async fn refresh_lease_for_cookie(&self, cookie: &str)` | `refresh_lease_for_owner(&self, owner_key: &str)` | public method + param |
| 3 | `codebase/backend/crates/http-api/src/lib.rs:326` | `pub async fn release_lock_for_cookie(&self, cookie: &str)` | `release_lock_for_owner(&self, owner_key: &str)` | public method + param |
| 4 | `codebase/backend/crates/ws-server/src/lib.rs:484` | `if let Some(cookie) = owner_key { ... sink.send(cookie) }` | `if let Some(owner) = owner_key { ... sink.send(owner) }` | local shadow |
| 5 | `codebase/backend/crates/ws-server/src/lib.rs:529-534` | `async fn handle_socket(socket: WebSocket, hub: Hub, cookie_value: Option<String>, connection_id: Arc<str>)` | `... owner_key: Option<String>, ...` | param name |
| 6 | `codebase/backend/crates/ws-server/src/lib.rs:577, 724, 741, 745` (모든 `cookie_value.as_deref()` 호출) | `cookie_value.as_deref()` | `owner_key.as_deref()` | param 변경 따라 |
| 7 | `codebase/backend/crates/ws-server/src/hub.rs` (검색: `session_for_cookie`, `set_session_for_cookie`, `clear_session_for_cookie`, `clear_sessions_by_name` 의 cookie 참조) | `*_for_cookie` | `*_for_owner` | public Hub API |
| 8 | `codebase/backend/bin/gtmux-cli/src/main.rs:432-475` | `(heartbeat_tx, heartbeat_rx)`, `(disconnect_tx, ...)` consumer 의 `cookie` 변수명 + 호출 site | `owner_key`, `release_lock_for_owner`, `refresh_lease_for_owner` | call site |

**전체 grep 자가 검증**: 다음 명령으로 작업 후 cookie 가 *cookie 만* 의미하는 곳에만 남는지 확인:

```bash
# 작업 전 baseline
cd codebase/backend
grep -rn "for_cookie\|by_cookie\|cookie_value" crates/http-api/src crates/ws-server/src bin/gtmux-cli/src \
  | grep -v test | grep -v "^.*://" > /tmp/before_cookie_refs.txt
wc -l /tmp/before_cookie_refs.txt
```

작업 후:
```bash
grep -rn "for_cookie\|by_cookie\|cookie_value" crates/http-api/src crates/ws-server/src bin/gtmux-cli/src \
  | grep -v test | grep -v "^.*://" > /tmp/after_cookie_refs.txt
diff /tmp/before_cookie_refs.txt /tmp/after_cookie_refs.txt
```

**기대**: diff 가 *변경된 라인* 만 보여줌. `gtmux_auth=` 추출하는 곳 (`ws-server/lib.rs:415-425`) 의 `cookie_value` 만 *진짜 cookie* 이므로 유지하거나 `auth_cookie` 로 rename. 나머지 모두 owner 로 변경.

### B-3. ADR amend 짝

별 동봉 commit 으로 `docs/adr/0019-session-and-workspace-model.md` D5.6 amend ② 추가:

```markdown
##### D5.6 amend ② — code symbol naming = owner_key 통일 (2026-05-18)

owner_key (cookie + 0x1f + webpage_id) 가 attach lock / heartbeat / WS routing 의 통일 식별자임을 코드 symbol 명에 반영:

- `session_locks_by_cookie` → `session_locks_by_owner` (struct field)
- `release_lock_for_cookie` → `release_lock_for_owner` (public method)
- `refresh_lease_for_cookie` → `refresh_lease_for_owner` (public method)
- `Hub::session_for_cookie` → `Hub::session_for_owner` (public Hub API)
- `Hub::set_session_for_cookie` → `Hub::set_session_for_owner`
- `Hub::clear_session_for_cookie` → `Hub::clear_session_for_owner`
- WS handler 의 `cookie_value` 파라미터 → `owner_key`

순수 cookie 만 의미하는 곳 (auth cookie 추출 site `ws-server/lib.rs:415-425`) 의 `cookie_value` 는 `auth_cookie` 로 분리. 본 amend 의 의도는 *이름이 의미를 정직하게 표현* — 본 감사가 false-positive 6 건 양산한 직접 원인을 차단.
```

### B-4. Acceptance criteria (검증 가능)

| # | 검증 명령 | 기대 결과 |
|---|---|---|
| AC-A1 | `cd codebase/backend && cargo build --color=never` | clean build, 0 error/warn |
| AC-A2 | `cd codebase/backend && cargo test --workspace --no-fail-fast --color=never 2>&1 \| tail -3` | **변경 전 baseline 과 PASS 수 동일** (변경 전 baseline 을 commit message 에 기록) |
| AC-A3 | `grep -rn "for_cookie\|by_cookie" codebase/backend/crates --include='*.rs' \| grep -v test` | 0 hit (또는 *순수 cookie* 영역만 — docstring 으로 정당화) |
| AC-A4 | `grep -rn "X-Gtmux-Webpage-Id" codebase/backend/crates --include='*.rs'` | 변경 전 hit 수와 동일 (header 명 자체는 변경 X) |
| AC-A5 | `git diff --stat HEAD~1 HEAD` | 본 commit 의 변경 라인 수 == rename + ADR amend 만. 다른 logic 변경 0 |

### B-5. Anti-pattern (false ship)

❌ **이런 fix 는 ship 거부**:

1. **변수명 일부만 변경**: 1 commit 안에서 `session_locks_by_cookie` → `session_locks_by_owner` 했는데 method `release_lock_for_cookie` 는 그대로 둠. → naming debt 그대로, false-positive 양산 계속.
2. **method 호출 site 누락**: refactor 시 grep 으로 모든 call site 찾지 않고 컴파일 에러만 fix → 동일 method 가 두 이름으로 존재.
3. **deprecated alias 추가**: `#[deprecated]` alias 남기면 새 코드도 옛 이름 호출 가능 — refactor 목적 흐려짐. 거부 (Q2).
4. **logic 변경 동봉**: rename commit 안에 small logic fix 끼워넣음 — review 가 어려워지고 regression risk 증가.
5. **ADR amend 누락**: 코드만 변경하고 ADR 본문 안 갱신 → CLAUDE.md 의 "ADR↔code coherence hard rule" 위반.
6. **doc comment 누락**: `release_lock_for_owner` 의 docstring 에 "owner_key = cookie + 0x1f + webpage_id" 명시 안 함 → 다음 reader 가 또 헷갈림.

### B-6. Self-check (commit 전 ☑)

- [ ] 위 B-2 표의 8 위치 모두 rename 됨
- [ ] AC-A1 ~ AC-A5 모두 PASS
- [ ] AC-A2 baseline 의 PASS 수가 commit message 에 명시됨 (e.g., `416 PASS / 0 FAIL → 416 PASS / 0 FAIL`)
- [ ] ADR-0019 D5.6 amend ② 가 같은 commit 또는 짝 commit 으로 동봉
- [ ] `Hub::*_for_owner` 의 docstring 에 owner_key 정의 한 줄 추가
- [ ] `release_lock_for_owner` / `refresh_lease_for_owner` 의 docstring 에 owner_key 정의 한 줄 추가
- [ ] commit message 가 "C-1 naming refactor" 임을 명시 + 0071 §C-1 anchor

---

## §C. Task BE-B — `POST /api/leave` sendBeacon endpoint

### C-1. Trigger

0071 §D-5: ADR-0021 D6 가 명시한 *"보조: `beforeunload` 의 `navigator.sendBeacon('/api/leave')` (best-effort) — server 가 cookie 기반 attach 즉시 해제"* 가 BE 측 미구현. 현재 정상 탭 close 도 30s heartbeat timeout 까지 lock 잔존. ADR-0021 D6 amend ② 짝.

### C-2. Anchor — endpoint 설계 (Q3/Q4/Q5 결정 반영)

```
POST /api/leave?webpage_id=<id>
  Cookie: gtmux_auth=<value>
  (body 없음, Content-Type: text/plain;charset=UTF-8 — sendBeacon default)
→ 204 No Content   (성공)
→ 401              (cookie invalid)
→ 200 + {released: false}  (해당 owner 가 어떤 lock 도 보유 안 함 — idempotent)
```

**처리 함수**: 새 `leave_handler` 추가 — 본질은 `detach_handler` 의 *body 없는 + sendBeacon 친화* 변종. 내부적으로 `release_lock_for_owner(owner_key)` 호출.

### C-3. 구현 파일

| 파일 | 변경 |
|---|---|
| `codebase/backend/crates/http-api/src/sessions.rs` | `leave_handler` 신규 (line 끝부분 `detach_handler` 옆에 배치). cookie + webpage_id query 에서 owner_key 형성. `release_lock_for_owner` 호출. |
| `codebase/backend/crates/http-api/src/lib.rs` (라우터 정의 부분) | `.route("/api/leave", post(sessions::leave_handler))` 추가. `/api/*` middleware 통과 (cookie auth) |
| `docs/adr/0021-terminal-pool-and-mirror.md` D6 | amend ② — sendBeacon endpoint shape 명시 |
| `docs/ssot/state-machines.md` §4.4 | endpoint matrix 표 갱신 — `/api/leave` 의 method/body/효과 |

### C-4. 의사 코드

```rust
/// `POST /api/leave?webpage_id=<id>` — page unload 시 best-effort release.
///
/// `navigator.sendBeacon` 의 호출 채널. Content-Type 은 sendBeacon 의
/// default (`text/plain;charset=UTF-8`). body 는 비어있음. webpage_id 는
/// URL query 로 전달 (sendBeacon 이 custom header 제한 — ADR-0019 D5.6
/// 가 HTTP header / WS query 분리 명시).
///
/// 응답: 204 No Content — sendBeacon 은 응답을 읽지 않으므로 body 무용.
///
/// 동작: cookie + webpage_id → owner_key → release_lock_for_owner.
/// 보유 lock 없으면 no-op (idempotent).
pub async fn leave_handler(
    State(state): State<crate::AppState>,
    req: Request<Body>,
) -> Response {
    let auth_cookie = match extract_auth_cookie(req.headers()) {
        Some(c) => c,
        None => return (StatusCode::UNAUTHORIZED, "cookie required").into_response(),
    };
    let webpage_id = webpage_id_from_query(req.uri().query()).unwrap_or_default();
    let owner_key = if webpage_id.is_empty() {
        auth_cookie
    } else {
        format!("{auth_cookie}\x1f{webpage_id}")
    };
    state.release_lock_for_owner(&owner_key).await;
    StatusCode::NO_CONTENT.into_response()
}
```

→ `webpage_id_from_query` 의 helper 가 이미 `ws-server/lib.rs:506-522` 에 있음. http-api 측에서 동일 정합 helper 필요 (검증/escaping 정합).

### C-5. ADR amend 짝 (ADR-0021 D6)

```markdown
##### D6 amend ② — `/api/leave` sendBeacon endpoint (2026-05-18, 0071 §D-5)

D6 본문은 `navigator.sendBeacon('/api/leave')` 를 *"best-effort"* 보조 흐름으로 명시했으나 BE 측 endpoint 가 미구현이었음. 본 amend 가 endpoint shape 잠금:

- Method/Path: `POST /api/leave?webpage_id=<id>`
- Auth: cookie (`/api/*` middleware 통과)
- Body: 없음 (Content-Type: text/plain — sendBeacon default)
- 응답: 204 No Content (성공) / 401 (cookie invalid)
- 동작: `release_lock_for_owner(owner_key)` 호출. 보유 lock 없으면 no-op (idempotent).

webpage_id 는 *URL query* 로 전달 — sendBeacon 의 custom header 제한 우회 (ADR-0019 D5.6 의 WS query 패턴과 정합).
```

### C-6. Acceptance criteria

| # | 검증 명령 | 기대 결과 |
|---|---|---|
| AC-B1 | `cargo build --color=never` | clean |
| AC-B2 | `cargo test --workspace -- leave` | 새 test ≥ 2 PASS (happy + idempotent) |
| AC-B3 | `grep -n "POST /api/leave\|/api/leave" codebase/backend/crates/http-api/src/lib.rs` | route 등록 1 hit + handler import 1 hit |
| AC-B4 | `cargo test --workspace --no-fail-fast 2>&1 \| tail -3` | 직전 baseline + 새 test 수만큼 증가 |
| AC-B5 | manual curl: `curl -X POST 'http://localhost:9998/api/leave?webpage_id=test' -H 'Cookie: gtmux_auth=<demo>'` | `204 No Content` 응답 |

### C-7. 필요한 integration test (이름 + 의도 명시)

```rust
// codebase/backend/crates/http-api/src/lib.rs (#[cfg(test)] 블록 또는 sessions.rs 의 tests 모듈)

#[tokio::test]
async fn leave_releases_lock_for_owner() {
    // 1. attach session α via POST /api/sessions/α/attach (cookie C + webpage_id W1)
    // 2. GET /api/sessions → α.active == true
    // 3. POST /api/leave?webpage_id=W1 with cookie C
    //    → 204 No Content
    // 4. GET /api/sessions → α.active == false
    // 5. 같은 owner_key 로 GET /api/sessions/α/layout → 403 not_attached (lock 풀림)
}

#[tokio::test]
async fn leave_idempotent_when_no_lock() {
    // 1. cookie C 가 어떤 session 도 attach 안 한 상태
    // 2. POST /api/leave?webpage_id=W1 with cookie C
    //    → 204 (idempotent — no-op)
}

#[tokio::test]
async fn leave_requires_cookie() {
    // 1. cookie 없이 POST /api/leave?webpage_id=W1
    //    → 401 cookie required
}

#[tokio::test]
async fn leave_with_different_webpage_id_releases_only_matching_owner() {
    // 1. attach α with (C, W1) → owner_key_1
    // 2. attach β with (C, W2) → owner_key_2 (different owner, same cookie)
    // 3. POST /api/leave?webpage_id=W1 with cookie C
    //    → 204. α lock released. β lock 유지.
    // 4. GET /api/sessions → α.active=false, β.active=true.
}
```

### C-8. Anti-pattern

❌ **이런 fix 는 ship 거부**:

1. **`DELETE /attach` 와 같은 handler 호출**: `leave_handler` 가 `detach_handler` 를 그대로 호출 — Path param `name` 이 없는데 어떻게? `release_lock_for_owner` 가 어차피 owner 의 *현재 보유 session* 을 찾으므로 path-less handler 가 자연.
2. **webpage_id 검증 누락**: `webpage_id_from_query` 의 alnum-only 검증 없이 raw query 사용 → injection 표면.
3. **rate limit 추가**: Q5 결정대로 적용 안 함. 본 endpoint 의 호출 빈도 = page unload 시점 1회.
4. **body 파싱**: sendBeacon 의 body 는 일반적으로 비어있거나 작은 blob. 본 handler 는 body 무시.
5. **응답 body 추가**: 204 No Content 가 sendBeacon-friendly. JSON body 추가 시 sendBeacon 이 못 읽어서 무용 + bandwidth 손실.
6. **FE 측 `beforeunload` 호출 누락 chk 안 함**: BE 만 ship 하고 FE 측 sendBeacon 없으면 endpoint 가 dead code. → paired 0073 §B 의 ship 검증과 짝.

### C-9. Self-check

- [ ] `leave_handler` 가 `sessions.rs` 에 추가됨
- [ ] `lib.rs` 의 router 에 `.route("/api/leave", post(sessions::leave_handler))` 추가됨
- [ ] AC-B1 ~ AC-B5 모두 PASS
- [ ] 4 integration test 추가 + 모두 PASS
- [ ] ADR-0021 D6 amend ② 동봉 commit
- [ ] state-machines.md §4.4 endpoint 매트릭스 갱신
- [ ] FE 측 (0073 §B) 의 sendBeacon 호출 ship 확인 — 짝 commit 또는 다음 commit
- [ ] commit message 가 "BE-B `/api/leave` sendBeacon endpoint" + 0071 §D-5 anchor

---

## §D. Task BE-C — Boot-time stale `.lock` scan + cleanup

### D-1. Trigger

0071 §D-1: ADR-0019 D6 의 *"Server SIGKILL / panic: OS kernel 이 flock 자동 해제, file 은 남음. 다음 acquirer 가 LOCK_NB 성공 시 내용 덮어쓰기"* — 기능적으로는 OK, 그러나 `.locks/` 디렉터리에 stale 파일이 시간 누적. peek 이 정상 동작하므로 functional 영향 0, **housekeeping only**.

### D-2. Anchor — 변경 위치

| 파일 | 변경 |
|---|---|
| `codebase/backend/crates/http-api/src/lib.rs:423` `with_workspace` 메서드 | `attach_index.rebuild_from_disk(&wm)` 옆에 `scan_and_cleanup_stale_locks(&wm)` 호출 추가 |
| `codebase/backend/crates/http-api/src/session_lock.rs` | 새 helper `pub fn scan_and_cleanup_stale_locks(wm: &WorkspaceManager)` 추가. enumerate_sessions 으로 각 session 의 lock peek → Stale 면 `unlink_stale` 호출 |

### D-3. 의사 코드

```rust
// codebase/backend/crates/http-api/src/session_lock.rs

/// Boot-time housekeeping: scan workspace 의 모든 .lock 파일을 peek 해
/// LockState::Stale 인 것만 unlink. 한 파일의 실패는 warn log + 다음 파일 계속.
///
/// functional 의무 아님 — peek path 가 stale 인식 자체 처리. 본 helper 의
/// 의도는 .locks/ 디렉터리의 누적 정리만.
pub fn scan_and_cleanup_stale_locks(wm: &crate::workspace::WorkspaceManager) {
    let infos = match wm.enumerate_sessions() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "session_lock: stale scan enumerate failed");
            return;
        }
    };
    let locks_dir = wm.locks_dir();
    let mut cleaned = 0u32;
    for info in &infos {
        match peek(&locks_dir, &info.name) {
            LockState::Stale => {
                match unlink_stale(&locks_dir, &info.name) {
                    Ok(()) => { cleaned += 1; }
                    Err(e) => {
                        tracing::warn!(
                            name = %info.name,
                            error = %e,
                            "session_lock: stale unlink failed"
                        );
                    }
                }
            }
            _ => {}
        }
    }
    if cleaned > 0 {
        tracing::info!(count = cleaned, "session_lock: boot-time stale cleanup");
    }
}
```

### D-4. Acceptance criteria

| # | 검증 명령 | 기대 결과 |
|---|---|---|
| AC-C1 | `cargo build` | clean |
| AC-C2 | `cargo test --workspace -- stale_lock_scan` | 새 unit test ≥ 2 PASS |
| AC-C3 | `cargo test --workspace --no-fail-fast 2>&1 \| tail -3` | baseline + 신규 test 수만큼 증가 |
| AC-C4 | manual: `<workspace>/.locks/` 에 stale 파일 깔고 server boot → tracing log 에 `boot-time stale cleanup count=N` 확인 | 로그 1줄 |

### D-5. Integration test

```rust
// codebase/backend/crates/http-api/src/session_lock.rs::tests

#[test]
fn scan_and_cleanup_unlinks_stale_files() {
    // 1. tempdir workspace
    // 2. write 두 session file (alpha.json, beta.json)
    // 3. write .locks/alpha.lock + .locks/beta.lock (둘 다 valid JSON body)
    //    — flock 안 잡힘, 즉 stale 상태
    // 4. scan_and_cleanup_stale_locks 호출
    // 5. alpha.lock + beta.lock 모두 unlink 됨 확인
}

#[test]
fn scan_and_cleanup_preserves_held_locks() {
    // 1. tempdir workspace
    // 2. write session α + actually acquire flock with `acquire()`
    // 3. spawn 두 번째 thread 가 scan_and_cleanup_stale_locks 호출
    // 4. α 의 .lock 은 unlink 안 됨 (peek 결과 = InUse)
    // 5. acquire 된 guard 가 여전히 유효 (release 호출 가능)
}
```

### D-6. Anti-pattern

❌ **이런 fix 는 ship 거부**:

1. **InUse 까지 unlink**: peek 결과 *Stale 만* unlink. InUse 도 unlink 하면 active webpage 의 lock 깸 — CRITICAL bug.
2. **fail-fast**: 한 파일 실패에 boot abort — Q6 결정 위반.
3. **start_handler 안 호출**: boot path 가 아닌 다른 곳에서 호출 → 의도와 다른 시점.
4. **scan 비용**: 1000+ session 의 환경에서도 enumerate + peek 가 O(N) — single-user 환경의 boot 시점 1회라 OK. 그러나 boot 지연이 측정될 수준이면 issue.

### D-7. Self-check

- [ ] `scan_and_cleanup_stale_locks` helper 가 `session_lock.rs` 에 추가됨
- [ ] `with_workspace` 안에 호출 추가됨 (rebuild_from_disk 직후)
- [ ] AC-C1 ~ AC-C4 모두 PASS
- [ ] 2 integration test 추가 + 모두 PASS
- [ ] commit message 가 "BE-C boot-time stale lock scan" + 0071 §D-1 anchor

---

## §E. 통합 검증 — 3 task land 후

### E-1. 전체 workspace test

```bash
cd codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | tail -3
```

**기대**: 직전 baseline (이 handover 작성 시점 `416 PASS / 0 FAIL`) + BE-B 의 4 test + BE-C 의 2 test = **422 PASS / 0 FAIL**.

### E-2. release build 검증

```bash
cd codebase/backend
cargo build --release --bin gtmux --color=never
```

**기대**: PASS.

### E-3. smoke test (있다면)

```bash
# 본 시점 smoke 가 wire 되어있는지 확인 필요
# 0034-stage-5-ab-ws-envelope-be-progress.md 의 02_stage5.sh 같은 패턴
bash codebase/backend/smoke/02_stage5.sh 2>&1 | tail -20
```

**기대**: regression 0.

### E-4. ADR 정합 확인

- [ ] ADR-0019 D5.6 amend ② (BE-A) 동봉
- [ ] ADR-0021 D6 amend ② (BE-B) 동봉
- [ ] ADR-0021 D6 의 본문 (D-1 의 stale unlink 본 amend 필요 시) — 본 BE-C 가 본 amend 의 *implementation note* 수준이라 ADR 본문 변경 0 도 OK. 단, ADR-0019 D6.x 또는 ADR-0021 D6 에 *"boot-time stale scan"* 한 줄 추가 권장
- [ ] state-machines.md §4.4 endpoint 매트릭스 갱신 (`/api/leave` row 추가)

### E-5. 거짓 ship 방지 — final cross-check

| 검증자 | 명령 | 통과 기준 |
|---|---|---|
| **본인 (구현자)** | E-1 + E-2 + E-3 | 모두 PASS, baseline 대비 신규 test 만 증가 |
| **AI reviewer** | 본 handover 를 다시 읽고 §B-6, §C-9, §D-7 의 self-check 모두 ☑ 인지 검증 | 한 개라도 ☐ 면 ship 보류 |
| **다음 session** | 0071 §C-1 의 false-positive 패턴 발생하는지 grep 으로 자가 검증 | 작업 후 동일 grep 으로 false-positive 0 |

---

## §F. Commit 분리 권장

본 handover 의 3 task 는 별 commit:

| Commit | 내용 |
|---|---|
| `refactor(be): C-1 owner_key naming 통일 (0071 §C-1)` | BE-A: 8 symbol rename + ADR-0019 D5.6 amend ② |
| `feat(be/sessions): leave endpoint for sendBeacon (0071 §D-5)` | BE-B: `/api/leave` + 4 test + ADR-0021 D6 amend ② + state-machines.md §4.4 갱신 |
| `chore(be/session_lock): boot-time stale scan (0071 §D-1)` | BE-C: scan_and_cleanup + 2 test |

→ FE 측 (0073) 의 commit 과 *반드시 같은 시점에 ship 할 필요 없음*. BE-A 가 FE 영향 0 (BE-internal naming), BE-B 만 FE 와 짝 — BE-B 가 먼저 land 되면 FE-B 가 follow-up 가능.

---

## §G. 본 handover 가 *완전 검증* 안 한 영역

다음은 본 handover 의 task 범위 밖. 후속 session 또는 별 handover 에서 처리:

- **B-2(b)** rebind history replay — 0071 §B-2 의 옵션 (b). verify 후 별 amend 필요
- **D-2** ADR-0025 amend ③ race 잔여 — 현 상태 OK, 추가 fix 불요
- **D-3** WS subprotocol 의 webpage_id 동봉 — 현 query 만으로 충분
- **D-4** AttachConfirmModal cancel chain toast verify — FE 영역 (0073)
- **D-6** multi-webpage rebind replay — B-2(b) 와 동일

---

## 변경 이력

- 2026-05-18: 초안. 0071 감사의 §C-1, §D-5, §D-1 의 BE-side land 를 위한 BE-A/BE-B/BE-C 3 task. Self-grilling 6 Q resolve. 각 task 에 anchor + acceptance criteria + integration test 명세 + anti-pattern + self-check.
