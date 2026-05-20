# 0046 — BE work package: `attach_handler` same-cookie idempotent fix

- 작성일: 2026-05-16
- 종류: backend work package (FE 측 분석 결과 BE 의존 사항 추출)
- 발주: FE 통합 세션 agent (0045 refresh reconnect loop 분석 후속)
- 우선순위: 🔴 P0 — refresh UX + Phase 2 silentReattach 신뢰성 직타격
- 관련 ADR: ADR-0019 D3 (single-attach invariant), D5.4 (initial entry recovery), ADR-0021 D6 (heartbeat)
- 관련 FE 정본: `docs/plans/0008-session-attach-recovery-impl.md`, `docs/reports/0045-refresh-session-reconnect-loop-analysis.md`

---

## 1. 한 줄 요약

`POST /api/sessions/<name>/attach` 가 **같은 cookie 가 같은 session 을 이미 보유한 경우** 코멘트의 약속과 달리 **409 CONFLICT** 반환. cookie ownership 확인 분기를 추가해 idempotent 200 OK 로 normalize.

---

## 2. 현재 동작 (버그)

`codebase/backend/crates/http-api/src/sessions.rs:330 attach_handler`:

```rust
// (line 358-368) 코멘트 — 의도된 동작 명시:
// ADR-0019 D3 single-attach invariant — implicit detach-on-reattach.
// If this cookie already holds a *different* session's lock (e.g. user
// switched session in WorkspaceSwitcher without an explicit DELETE
// attach), release it before acquiring the new one.
// ...
// Same-name reattach is an idempotent no-op — the cleanup branch is
// skipped and the rest of this handler short-circuits via the
// `holders.contains_key(&name)` check immediately below.

// (line 396-402) 실제 코드 — 코멘트 약속과 불일치:
let mut holders = state.session_locks.lock().await;
if holders.contains_key(&name) {
    // Even from the same server, takeover is forbidden.
    return lock_conflict_response(&state, wm, &name);  // ← 409 CONFLICT
}
```

`lock_conflict_response` (line 814) 는 **cookie ownership 확인 없이** 무조건 409 + body `{"error": "session_in_use", "holder": {server_id, pid, lease_until_unix}, "this_server_id"}`.

### 재현 시나리오

1. 같은 cookie A 로 `POST /api/sessions/alpha/attach` → 200 OK
2. 같은 cookie A 로 `POST /api/sessions/alpha/attach` 다시 → **409 CONFLICT** (코멘트 약속과 달리)

기존 단위 테스트 `attach_409_when_already_held_same_server` (lib.rs:3180) 가 이 (잘못된) 동작을 *명시 검증* 중. takeover 금지 의도가 cookie ownership 분기보다 우선되어 있음.

---

## 3. 영향 (FE 측 UX 회귀)

### 3.1 새로고침 race

```
브라우저 reload → tab JS unload
  ├─ WS close → BE `release_lock_for_cookie` 비동기 발화
  └─ 새 SPA load → /api/sessions 200 → hint → reconnectGate.start
       → POST /api/sessions/<name>/attach
            ↓
  ┌─ release_lock 가 먼저 끝남 → 200 OK ✓
  └─ release_lock 가 늦음 → 409 CONFLICT
       → reconnectGate.state = 'in_use'
       → ReconnectModal "Session is in use by another webpage" 노출
       → 사용자가 [Retry] 클릭해야 통과 (release_lock 완료 후 200)
```

전형적으로 release_lock 이 빠르지만 (cookie WS close 즉시 fire), 다음 조건에서 race 발생:
- BE 가 다른 cookie 의 attach 요청 처리 중 (mutex 대기)
- BE 가 spawn-blocking I/O 중 (layout JSON write 등)
- WS close 가 graceful 이 아닌 abrupt (browser crash, OS-level kill) — `disconnect_sink` fire 가 지연

### 3.2 plan-0008 Phase 2 silentReattach 의 *모든* 호출이 fail

Phase 2 (Case II) trigger 시나리오:
- WS dispatcher 의 `reconnecting → open` 전이 (network blip 복구 후)
- visibility change + heartbeat.isIdle (탭 background → visible)

이 trigger 시점에 cookie 는 *여전히 lock 보유 중* (WS reconnect 는 cookie 유지). 그러므로:
```
silentReattach → POST /attach → 409 CONFLICT
  → sessionStore.lastSilentReattachResult = { kind: 'in_use' }
  → +page.svelte 의 toast "Session X is in use by another webpage" (사실은 같은 webpage)
  → 그 이후 모든 mutation 진입점이 guardOutgoingMutation → !ok 분기 → 모든 mutation 차단
```

→ Phase 2 가 사실상 모든 silentReattach 에서 **항상 fail** 하고 사용자가 mutation 못 함. **Phase 2 의 의도 (silent transparent recovery) 완전히 깨짐**.

### 3.3 silent 의도 위반

ADR-0019 D3 의 single-attach invariant 는 *다른 webpage* 의 takeover 방지가 목적이지, 같은 webpage 의 idempotent recovery 를 막을 의도가 아님. 코멘트도 이를 명시. 그러나 코드가 그 약속을 어김.

---

## 4. 권장 fix

### 4.1 attach_handler 분기 추가

`sessions.rs:330 attach_handler` 의 line 396 (same-server serialisation 검사) 직전에 cookie ownership 확인 분기 추가:

```rust
    // ADR-0019 D3 — same-cookie same-session 재attach 는 idempotent.
    // 새로고침 race 또는 plan-0008 Phase 2 silentReattach 가 발화한 경우.
    // cookie 가 *이미 이 session 의 lock 을 보유한* 상태에서 다시 attach 하는 것은
    // takeover 가 아니므로 409 가 아닌 200 OK 로 normalize.
    {
        let by_cookie = state.session_locks_by_cookie.lock().await;
        if by_cookie.get(&cookie).map(|s| s == &name).unwrap_or(false) {
            drop(by_cookie);
            // 기존 lock 유지 — 새 acquire 안 함. 기존 attach 의 layout
            // classification (matched/unmatched) 만 다시 계산해 응답.
            return reuse_existing_attach_response(&state, wm, &name).await;
        }
    }

    // (기존) Same-server serialisation (D6.6)
    let mut holders = state.session_locks.lock().await;
    if holders.contains_key(&name) {
        return lock_conflict_response(&state, wm, &name);
    }
    // ...
```

### 4.2 `reuse_existing_attach_response` 헬퍼

기존 attach 의 path (acquire 후 classify) 와 정합하되 lock acquire 만 skip. 응답은 정상 200 + `{ attached: true, matched, unmatched, name, server_id }`.

```rust
async fn reuse_existing_attach_response(
    state: &AppState,
    wm: &WorkspaceManager,
    name: &str,
) -> Response {
    let (matched, unmatched) = match classify_layout_terminals(state, wm, name).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),  // 본 분기는 일반적이지 않음 — 이미 lock 보유 중인데 corrupt 발생
    };
    let body = json!({
        "attached": true,
        "name": name,
        "server_id": &*state.server_id,
        "matched": matched,
        "unmatched": unmatched,
    });
    (StatusCode::OK, Json(body)).into_response()
}
```

### 4.3 기존 테스트 수정 + 신규 테스트

**수정**: `attach_409_when_already_held_same_server` (lib.rs:3180) 의 의도가 변경:
- *다른 cookie* 의 same-session 재attach 만 409
- *같은 cookie* 의 same-session 재attach 는 200

→ 테스트명도 `attach_409_when_held_by_different_cookie` 으로 rename + 2 cookie 환경에서 검증.

**신규**:
```rust
#[tokio::test]
async fn attach_idempotent_for_same_cookie_same_session() {
    let dir = TempDir::new().unwrap();
    let (app, token, _) = make_app_with_workspace(&dir);
    create_session(&app, &token, "alpha").await;
    
    // 첫 attach
    assert_eq!(attach(&app, &token, "alpha").await, StatusCode::OK);
    
    // 같은 cookie/token 으로 다시 attach — 200 idempotent
    assert_eq!(attach(&app, &token, "alpha").await, StatusCode::OK);
    
    // 응답 body 확인 — attached:true, matched/unmatched 정상
    let res = attach_get_body(&app, &token, "alpha").await;
    assert_eq!(res["attached"], true);
    assert_eq!(res["name"], "alpha");
    
    // detach 1회로 정상 해제
    assert_eq!(detach(&app, &token, "alpha").await, StatusCode::OK);
}

#[tokio::test]
async fn attach_409_when_different_cookie_holds() {
    // 2 cookie 환경 — 별 token 으로 분리
    let dir = TempDir::new().unwrap();
    let (app, token_a, _) = make_app_with_workspace(&dir);
    let token_b = mint_second_token(&app);  // ← helper, 별 cookie 발급
    
    create_session(&app, &token_a, "alpha").await;
    assert_eq!(attach(&app, &token_a, "alpha").await, StatusCode::OK);
    
    // 다른 cookie 의 same-session attach 는 409 (takeover 금지)
    assert_eq!(attach(&app, &token_b, "alpha").await, StatusCode::CONFLICT);
    
    assert_eq!(detach(&app, &token_a, "alpha").await, StatusCode::OK);
}
```

### 4.4 코멘트 정합

attach_handler 의 line 366-368 코멘트는 이미 정확. 코드가 그 약속을 따르지 않았던 것뿐. fix 후엔 코멘트와 코드 정합.

---

## 5. FE 측 영향 (ship 후 wire)

BE 0046 ship 후 FE 의 다음 동작이 자연 정상화:

1. **새로고침 시 race-free**: hint 기반 reconnectGate.start 가 release_lock 보다 빨라도 200 → success → 본 화면 mount 즉시. ReconnectModal "in_use" 전이 사라짐.

2. **Phase 2 silentReattach 정상 작동**:
   - WS reconnecting → open transition → silentReattach → 200 (cookie 가 이미 보유)
   - visibility + isIdle → silentReattach → 200
   - mutation guard 가 차단 안 됨 — 사용자 mutation 정상 진행
   - **`lastSilentReattachResult = { kind: 'success' }`** 가 정상 경로
   - heartbeatStore.reset() 호출되어 idle counter 갱신

3. **toast UX 정합**: Phase 2 silent reattach 가 fail 시 toast 노출 흐름은 그대로 — 단 실제 fail (BE 가 진짜로 session 잃었거나 cookie 만료) 에만 토스트 발생.

4. **mutation guard 의도 정합**: guard 가 차단해야 할 시점 = 실제로 cookie 가 lock 잃은 경우. 그 외에는 노이즈 0.

---

## 6. 진행 순서 (BE side)

1. **버그 재현 테스트 추가** (RED) — `attach_idempotent_for_same_cookie_same_session` 만 추가, 실패 확인.
2. **분기 추가** (GREEN) — line 396 직전에 cookie ownership 분기.
3. **헬퍼 추가** — `reuse_existing_attach_response` (`classify_layout_terminals` 재사용).
4. **기존 테스트 amend** — `attach_409_when_already_held_same_server` → `attach_409_when_different_cookie_holds` rename + 2 cookie 환경.
5. **회귀 smoke** — workspace_dir 의 lock 파일 정합 (acquire-skip 시 lease 갱신 여부 결정 — 본 PR 에선 lease 그대로 유지 권장).
6. **ADR-0019 D3 amend** — same-cookie idempotent 분기 명시 (코멘트가 이미 약속한 동작을 코드로 land 했음 표기).

---

## 7. 후속 확장 (별 PR)

본 fix 의 자연 확장:
- **D6 heartbeat ship** — 15s ping / 30s timeout + cookie ↔ lease lifetime 연계. 현재는 ws-close 만 release trigger — heartbeat 추가로 abrupt close 의 lock leak 방어.
- **PUT /api/sessions/<name>/attach** alias — REST-purist 의 idempotent 의도 명시 (POST 그대로 유지하되 PUT 도 alias). 선택.
- **Phase 2 mutation guard 의 in-use 분기 별 처리** — BE 0046 ship 후엔 in-use 가 거의 발생 안 함 → FE 의 toast 메시지를 "cookie may have expired" 같은 더 정확한 안내로 정합.

---

## 8. 검증 plan

### 8.1 단위 테스트 (BE)

```bash
cd codebase/backend
cargo test -p gtmux-http-api attach 2>&1 | tail -20
# 신규 attach_idempotent_for_same_cookie_same_session 통과
# 기존 attach_409_when_different_cookie_holds 통과
# 그 외 기존 attach 관련 테스트 모두 통과
```

### 8.2 라이브 smoke (curl)

```bash
TOKEN="<magic-link-token>"
# Step 1: bootstrap → cookie 발급
curl -s -c /tmp/cookies.txt "http://127.0.0.1:9999/auth/bootstrap?token=$TOKEN" -L

# Step 2: 첫 attach
curl -s -b /tmp/cookies.txt -X POST -H "Content-Type: application/json" \
  -d '{"ws_conn_id":"test1"}' \
  http://127.0.0.1:9999/api/sessions/<name>/attach \
  -w "\n[%{http_code}]\n"
# 기대: 200 + {"attached":true, ...}

# Step 3: 같은 cookie 로 재attach (idempotent)
curl -s -b /tmp/cookies.txt -X POST -H "Content-Type: application/json" \
  -d '{"ws_conn_id":"test2"}' \
  http://127.0.0.1:9999/api/sessions/<name>/attach \
  -w "\n[%{http_code}]\n"
# 기대 (fix 후): 200 + {"attached":true, ...}
# 현재 (버그): 409 CONFLICT
```

### 8.3 FE E2E 시나리오 (BE ship 후)

1. attach → 새로고침 → ReconnectModal 노출 없이 본 화면 직접 진입
2. attach → tab background 15s+ → 다시 active → toast 없이 silent 통과 (devtools console 로 확인)
3. attach → network blip → WS reconnect → silent 통과
4. attach → 다른 브라우저 (다른 cookie) 로 same session attach → 409 + holder 정보 유지

---

## 9. 변경 이력

- 2026-05-16: 초안 작성 — FE 통합 세션 agent 가 0045 분석 후속으로 BE 의존 사항을 격리 + work package 화. ADR-0019 D3 코멘트와 실제 코드의 contract drift 발견.
- 2026-05-16: BE land — §6 의 RED-GREEN-amend 6단계 수행 완료. `sessions.rs:330 attach_handler` 의 line 396 직전에 cookie ownership 분기 (`session_locks_by_cookie.get(&cookie).map(|s| s == &name)`) + 신규 헬퍼 `reuse_existing_attach_response(state, wm, name) -> Response` 추가 (`classify_layout_terminals` 재사용, lock acquire skip, body shape 정합 — `{ name, attached:true, server_id, matched, unmatched }`). 테스트 정합: 신규 `attach_idempotent_for_same_cookie_same_session` 추가 (RED → GREEN), 신규 `attach_409_when_held_by_different_cookie` 추가 (2 cookie 환경 takeover 검증), 기존 `attach_409_when_already_held_same_server` 의미 분할 후 제거, `attach_same_name_same_cookie_is_idempotent_409` → `_200` rename + 200 assertion + hub 의 session_for_cookie 정합 검증 추가, `release_lock_for_cookie_drops_the_attach` 의 second-attach cookie 를 `OTHER` 로 분리 (auto-release path 의 본 의도 보존). 워크스페이스 회귀: **365 → 366 PASS / 0 FAIL** (+1 새 idempotent 테스트). ADR-0019 D3 §148 의 stale "BE 변경 0" claim 을 amend ③ 으로 정합 — 코멘트가 약속한 동작을 코드로 land 했음 표기. cargo build --release PASS.
