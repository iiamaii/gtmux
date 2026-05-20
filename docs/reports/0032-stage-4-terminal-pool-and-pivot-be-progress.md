# 0032 — Stage 4 Terminal pool + multi-attach pivot BE progress

- 일자: 2026-05-15
- 작성자: backend agent (multi-session pivot 진행, 0031 의 후속)
- 종류: 진행 snapshot — Stage 4 의 전체 batch 완료 시점
- 후속 reading order: 본 문서 → `docs/reports/0031-stage-1-3-multi-session-be-progress.md` (Stage 1~3 진실) → `docs/agents/backend-handover.md` (전체 stage 명세) → 본 문서 §9 의 deferred 항목 ADR

---

## 0. 한 줄 요약

`docs/agents/backend-handover.md` 의 Stage 4 (Terminal pool + multi-attach mirror) 의 BE 작업이 **5 개 batch (4-A ~ 4-E)** 로 분할 완료. PaneId(u64) 는 pty-backend/ws-server 에서 그대로 유지 (Option B bridge map), schema/HTTP 표면은 UUID. 다음 cold-pickup 의 첫 후보는 **WS envelope refactor (session_id field + terminal-died UUID)** — FE-NEW-6 와의 정합 batch 로 격상되어 Stage 5 로 명시 deferral.

---

## 1. 현 코드 상태 스냅샷

- 작업 트리: `codebase/backend/`, 미커밋
- `cargo test --workspace`: **256 PASS / 0 FAIL** (Stage 3 종료 시점의 230 → +26)
- 신규 코드 clippy clean (기존 dead_code warning 2 개는 pre-existing, 본 stage 무관)
- TMUX 가드 그대로 — smoke 는 `env -u TMUX` 우회

### 1.1 본 stage 의 신규/수정 파일

| 파일 | 종류 | 핵심 변경 |
|---|---|---|
| `crates/http-api/src/terminal_map.rs` | **신규** | UUID ↔ PaneId bridge (`TerminalMap`, `MapError`, `fresh_terminal_uuid`) + 9 단위 테스트 |
| `crates/http-api/src/terminals.rs` | **신규** | `TerminalMetadata`/`TerminalMetadataStore` (in-memory), `TerminalInfo`, `list_handler`(GET), `kill_handler`(POST), `respawn_handler`(POST), `scan_session_terminal_refs` + 4 단위/통합 |
| `crates/http-api/src/sessions.rs` | amend | `attach_handler` 응답에 `matched/unmatched` 추가 (분류만, spawn X), `attach_confirm_handler` 신규, `delete_item_handler` 신규, `kill_and_unregister_terminal` 헬퍼, `classify_layout_terminals` / `load_terminal_uuids` / `release_attach` 헬퍼, `item_id` 디스크리미네이트, `DeleteItemQuery` 구조체 |
| `crates/http-api/src/lib.rs` | amend | `AppState.terminal_map`/`terminal_meta` 필드 (5 struct literal 갱신), `spawn_terminal_with_uuid` 비동기 메서드 + race-cleanup, `handle_pane_died` 비동기 메서드, `SpawnTerminalError` enum, 6 신규 라우트 마운트, +15 신규 lib-level 테스트 |
| `crates/http-api/Cargo.toml` | amend | `gtmux-pty-backend = { path = "../pty-backend" }` 추가 |
| `bin/gtmux-cli/src/main.rs` | amend | third consumer task: `hub.subscribe_notify()` → `BackendNotify::PaneDied` → `AppState::handle_pane_died` |

ws-server / pty-backend / config / auth crate **변경 없음** — Option B bridge 의 핵심.

---

## 2. Stage 4 batch 별 산출물

### 2.1 Batch 4-A — PaneId UUID bridge (단독 prereq)

**의도**: schema v2 의 `Item::Terminal.id` (UUID-shaped string) 과 pty-backend 의 `PaneId(pub u64)` 의 namespace 분리. wire protocol 의 varint encoding 영향 없는 *비파괴적* 옵션 (handover §4.2 의 Option B).

**산출**:
- `TerminalMap` (`crates/http-api/src/terminal_map.rs`):
  - `RwLock<{ by_uuid: HashMap<String, PaneId>, by_pane: HashMap<PaneId, String> }>`
  - `register / unregister_uuid / unregister_pane / lookup_pane / lookup_uuid / snapshot / len / is_empty`
  - bijection enforce: `register` 가 conflict 시 `MapError::UuidAlreadyBound { existing_pane }` 또는 `PaneAlreadyBound { existing_uuid }`
  - same-pair 재등록은 idempotent Ok(())
- `MapError` enum (둘 다 conflicting 측 carry — 호출자가 idempotent 처리 또는 race-loser cleanup)
- `fresh_terminal_uuid()` — `session_lock::fresh_server_id` (ring SystemRandom UUID v4) 재사용
- `AppState::spawn_terminal_with_uuid(uuid: String) -> Result<PaneId, SpawnTerminalError>`:
  1. `terminal_map.lookup_pane(uuid)` — fast-path idempotent return
  2. `hub.backend().spawn(SpawnSpec::default_shell())`
  3. `register(uuid.clone(), pane)` 시도
  4. `UuidAlreadyBound` 손실 시 → `backend.kill(pane)` cleanup, winner 의 existing_pane 반환
  5. `PaneAlreadyBound` (내부 일관성 위반) → 로그 + cleanup + 에러 반환
- `SpawnTerminalError { HubUnavailable | Backend | Map }`

**테스트**: 9 단위 (register/lookup/idempotent/UUID conflict/PaneId conflict/unregister 양쪽/snapshot copy/UUID v4 형식)

### 2.2 Batch 4-B — `GET /api/terminals` + 메타데이터

**의도**: ADR-0021 D7 의 Sidebar Terminal list. UUID/label/created_at/attach_count/attached_sessions 의 단일 정렬된 응답.

**산출**:
- `TerminalMetadata { label: String, created_at: u64 }` — UUID 키, 첫 spawn 시 created_at 고정 (re-spawn 도 보존), in-memory only
- `TerminalMetadataStore`: `record_spawn / forget / get / snapshot` — `RwLock<HashMap>`
- spawn_terminal_with_uuid 가 register 성공 시 `record_spawn` 호출 (커밋 시점에만 metadata 생성)
- `TerminalInfo` 응답 row: `{ id, alive, label, created_at, attach_count, attached_sessions }`
- `list_handler` (`GET /api/terminals`):
  1. `terminal_map.snapshot()` + `terminal_meta.snapshot()` 가져옴
  2. `scan_session_terminal_refs(wm)` — workspace 의 모든 session 파일을 직접 read+parse 해 UUID ↔ session-names 역인덱스 생성 (cache side-effect 없음)
  3. 두 source 를 join 해서 row 만듦, `created_at ASC then id ASC` 정렬
- 503 if workspace 미구성

**스코프 결정**: PATCH /api/terminals/:id { label } 은 본 batch 에 포함 X (handover 명시 없음; UX flow 분리). `MAX_LABEL_BYTES` 상수와 `set_label` helper 도 제거 — CLAUDE.md "사변적 추상화 금지" 정합. label 편집은 후속 batch.

**테스트**: 3 단위 (metadata store) + 3 통합 (empty pool / 503 / pool+meta+session_refs join)

### 2.3 Batch 4-C — Match-or-spawn (ADR-0018 D6)

**의도**: attach 가 layout 의 terminal item UUID 들을 server pool 과 매칭. spawn 은 명시 confirm 후에만.

**산출**:
- `attach_handler` 응답 확장:
  ```
  before: { name, attached, server_id }
  after:  { name, attached, server_id, matched: [uuid…], unmatched: [uuid…] }
  ```
  - flock acquire 성공 후 layout 로드 → terminal items 의 UUID 추출 → terminal_map.lookup_pane 으로 분류
  - flock 은 잡혀있고 layout 로드 실패 시 → `release_attach` 헬퍼로 flock + cookie reverse map 정리
- `attach_confirm_handler` (`POST /api/sessions/:name/attach/confirm`) 신규:
  1. workspace + valid name + hub 확인 (없으면 503)
  2. cookie 가 `session_locks_by_cookie` 에서 `name` 보유 확인 (아니면 403 `not_attached`)
  3. layout 의 terminal UUID 들 walk
  4. 각 UUID 별:
     - 이미 map 에 있음 → `already_present[]` 누적
     - 없음 → `spawn_terminal_with_uuid(uuid)` 호출 → 성공 시 `spawned[]`, 실패 시 `failed[]` (per-UUID 에러)
  5. 응답: `{ name, spawned, already_present, failed }`
- 헬퍼:
  - `classify_layout_terminals` — read-only 분류
  - `load_terminal_uuids` — session_cache.get_or_load + items[] filter
  - `release_attach` — flock + cookie map cleanup (실패 path 용)

**테스트**: 3 통합 (matched/unmatched split with UUID v4 / confirm 503 without hub / confirm 403 without prior attach)

**Gotcha**: schema validation 은 UUID format 을 강제 — 단위 테스트 의 layout JSON 도 8-4-4-4-12 hex shape 으로 작성해야 함. 4-B 의 `scan_session_terminal_refs` 는 validation 우회하지만, 4-C 의 attach 경로는 `session_cache.get_or_load` → `schema::validate` 를 거침.

### 2.4 Batch 4-D — Panel/Terminal close 분리 + Kill/Respawn API (ADR-0021 D9.2 / D9.4 / D10)

**의도**: panel 제거와 terminal 종료를 별 액션으로. dangling state 도 직접 트리거 가능.

**산출**:
- `delete_item_handler` (`DELETE /api/sessions/:name/items/:id[?kill_terminal=true]`):
  - SessionLayout write-lock 안에서 `items.retain(item_id != id)`
  - 0 개 제거 → 404 `item_not_found`
  - 제거된 item 이 Terminal 이고 `kill_terminal=true` 면 → `kill_and_unregister_terminal(uuid)` (lock 바깥에서 호출)
  - 응답: 204 + 새 ETag 헤더
  - ETag CAS 의 If-Match 는 사용 X (single-attach 단일-server 모델에서 동일 session 의 동시 mutate 가 없음)
- `kill_handler` (`POST /api/terminals/:id/kill`):
  - hub 없으면 503
  - UUID 가 map 에 없으면 404 `terminal_not_found`
  - 있으면 `kill_and_unregister_terminal(id)` 호출 + 204
- `respawn_handler` (`POST /api/terminals/:id/respawn`):
  - hub 없으면 503
  - `kill_and_unregister_terminal(id)` (best-effort, dangling UUID 면 no-op)
  - `spawn_terminal_with_uuid(id)` 재호출 (idempotent + 새 PaneId 발급)
  - 응답: 200 `{ id }`
- `kill_and_unregister_terminal` (sessions.rs pub(crate)) — 헬퍼: `lookup_pane` → `backend.kill` (실패 시 debug log, 계속) → `unregister_uuid` + `meta.forget`
- `item_id` discriminate (item enum 의 10 variant 모두 common.id 추출)

**테스트**: 6 통합 (panel-only delete keeps pool / kill_terminal=true drops pool / 404 missing id / kill 404 not in pool / kill 503 without hub / respawn 503 without hub)

### 2.5 Batch 4-E (narrowed) — PaneDied auto-unregister hygiene

**의도**: kernel SIGCHLD 또는 명시 SIGTERM 후 `TerminalMap` 의 stale binding 제거. Stage 4 의 모든 batch 의 invariant: *map 의 모든 UUID 는 alive PaneId 를 가리킨다*.

**산출**:
- `AppState::handle_pane_died(pane: PaneId)` 비동기 메서드 — `terminal_map.unregister_pane` → 반환된 UUID 로 `terminal_meta.forget`. 미존재 PaneId 는 idempotent no-op.
- CLI boot 의 third consumer task (gtmux-cli/src/main.rs):
  ```rust
  let mut notify_rx = hub.subscribe_notify();
  tokio::spawn(async move {
      loop {
          match notify_rx.recv().await {
              Ok(BackendNotify::PaneDied { id, .. }) => state.handle_pane_died(id).await,
              Ok(_) => {},  // 다른 NOTIFY 변종은 본 consumer 의 책임 X
              Err(Lagged(n)) => warn,  // broadcast cap 초과 시 단순 경고
              Err(Closed) => break,  // hub drop
          }
      }
  });
  ```
- broadcast subscriber 는 disconnect / heartbeat 의 mpsc 와 다른 패턴 (`broadcast::Receiver<BackendNotify>` 직접 사용) — pty-backend 의 기존 `subscribe_notify()` API 를 그대로 재사용

**스코프 narrowing**: 본 batch 의 원안 (handover §7.2 step 5) 은 WS frame 의 session_id envelope + UUID-carrying `terminal-died` frame 까지 포함. 두 항목은 FE 의 multi-xterm + dispatch table 과 정합이 필요하므로 FE-NEW-6 와 함께 별 batch (Stage 5) 로 격상. 본 batch 는 BE 단독으로 닫을 수 있는 *hygiene* 만 포함.

**테스트**: 2 단위 (`handle_pane_died_drops_map_and_metadata` / `handle_pane_died_is_idempotent_for_unknown_pane`)

---

## 3. AppState 의 현 모양 (Stage 4 추가분)

```rust
pub struct AppState {
    // ... Stage 1~3 의 11 개 필드 (0031 §2.1 참조) ...

    // Stage 4-A: UUID ↔ PaneId bridge
    pub terminal_map: Arc<TerminalMap>,
    // Stage 4-B: per-UUID label + created_at (in-memory only)
    pub terminal_meta: Arc<TerminalMetadataStore>,
}
```

신규 비동기 메서드:
- `spawn_terminal_with_uuid(uuid: String) -> Result<PaneId, SpawnTerminalError>` (4-A) — *production spawn 진입점*. attach_confirm + respawn 양쪽이 호출.
- `handle_pane_died(pane: PaneId)` (4-E) — CLI consumer task 의 진입점.

빌더 chain (변경 없음):
```rust
AppState::with_hub_and_path(config, token, hub, layout_path)
    .with_workspace(workspace)
    .with_password_hash(hash)        // optional
```

---

## 4. HTTP / WS 라우트 surface (Stage 1~4 통합)

### 4.1 HTTP (`http-api` crate)

```
GET    /healthz                                  # no auth
GET    /auth                                     # token-mode 자동 verify or password HTML form
GET    /auth/bootstrap                           # legacy → 303 redirect to /auth?token=…
POST   /auth/login                               # { token | password } + cookie 발행
POST   /auth/logout                              # session_table revoke + Max-Age=0

GET    /api/layout                               # 옛 v1 single-session (legacy)
PUT    /api/layout                               # ETag CAS (legacy)

GET    /api/sessions                             # list + active 플래그 via flock peek
POST   /api/sessions                             # create { name }
DELETE /api/sessions/:name                       # delete
GET    /api/sessions/:name/layout                # v2 schema + ETag
PUT    /api/sessions/:name/layout                # v2 ETag CAS
POST   /api/sessions/:name/attach                # ★ 응답에 matched/unmatched 추가 (4-C)
DELETE /api/sessions/:name/attach                # flock release
POST   /api/sessions/:name/attach/confirm        # ★ 신규 (4-C): unmatched 마다 spawn
DELETE /api/sessions/:name/items/:id             # ★ 신규 (4-D): ?kill_terminal=bool

GET    /api/terminals                            # ★ 신규 (4-B): pool + meta + cross-ref
POST   /api/terminals/:id/kill                   # ★ 신규 (4-D): SIGTERM, dangling
POST   /api/terminals/:id/respawn                # ★ 신규 (4-D): same UUID, fresh PaneId
```

★ = Stage 4 신규/확장.

### 4.2 WS (`ws-server` crate)

**변경 없음**. heartbeat 15s/30s, subprotocol `bearer.<token>`, cookie 추출, disconnect/heartbeat sink — 그대로.

- BackendNotify::PaneDied 의 **broadcast** 는 그대로 모든 WS subscriber 에 전달 (기존 wire frame). FE 가 PaneId 기반으로 panel 의 dangling overlay 표시.
- UUID-carrying `terminal-died` envelope 은 Stage 5 — §9 참조.

---

## 5. 핵심 결정 (다음 agent 가 기억해야 할 회로)

### 5.1 Option B 채택 (PaneId u64 wire 유지)

handover §4.2 의 두 옵션 중 B (bridge map) 채택. 결과:
- wire protocol (varint encoded PaneId) 변경 없음 → ws-server / pty-backend / FE wire 모두 무손
- bridge 는 `crates/http-api/src/terminal_map.rs` 에 격리 — schema layer / HTTP handler 만 UUID 사용
- 두 namespace 의 매칭 책임: `AppState::spawn_terminal_with_uuid` (생성) + `handle_pane_died` (소멸)

### 5.2 metadata 는 in-memory only

`TerminalMetadataStore` 는 영속화 X. 이유:
- `created_at` 의 정확성은 UX hint 수준 (sort key + display)
- 영속화하면 *crash 후에도 보존* 의 illusion → 실제로는 PaneId 자체가 ephemeral 이므로 의미가 없음
- 영속 ID 는 schema v2 의 UUID 자체 (ADR-0018 D2) — 이미 session 파일에 저장됨

### 5.3 race-cleanup 의 best-effort kill

`spawn_terminal_with_uuid` 의 `UuidAlreadyBound` race path:
- 두 concurrent attach/confirm 이 같은 unmatched UUID 를 spawn 시도
- 한 쪽만 register 성공, 다른 쪽 (loser) 는 방금 spawn 한 PaneId 를 `backend.kill` 로 정리 후 winner 의 PaneId 반환
- kill 실패는 warn log + 계속 — 사용자에게는 정상 응답 (winner 의 PaneId 가 표면)

이 race 는 실 production 에서 거의 발생 X (single-attach 정합 + cookie ↔ session lock). 이론적 safety net.

### 5.4 ETag CAS 의 부재

`delete_item_handler` 는 If-Match 검사 X. 이유:
- single-attach (한 webpage 만 한 session 의 lock 보유) + single-server 모델에서 동일 session 의 동시 layout mutate 가 없음
- `session_locks` 의 same-server mutex + cross-server flock 이 race 차단
- ETag CAS 가 의미 있는 경우는 *다중 client 의 동시 PUT* — 본 모델에는 없음

`PUT /api/sessions/:name/layout` 은 여전히 ETag CAS — 기존 코드 호환 + multi-tab 변종 attach 확장 시의 safety.

### 5.5 schema validation 우회 vs 강제

| 경로 | UUID format 강제? |
|---|---|
| `scan_session_terminal_refs` (4-B GET /terminals) | **우회** — 단순 walk, deserialize 가능하면 진행 |
| `session_cache.get_or_load` (4-C attach + 4-D delete_item) | **강제** — schema::validate 통과 필요 |

이유: GET /terminals 는 read-only 정보이며 corrupt session 도 cross-reference 안 깨야 함. attach/mutate 는 server invariant 보장 위해 validation 강제.

### 5.6 broadcast subscriber 의 Lagged 처리

`handle_pane_died` consumer 는 `RecvError::Lagged(n)` 시 단순 warn + 계속. 영향:
- broadcast cap 초과 시 일부 PaneDied 가 drop 될 수 있음
- 누락된 PaneDied 의 UUID 는 map 에 stale binding 으로 남음
- 다음 attach 의 match-or-spawn 분류에서 "matched" 로 잘못 분류될 수 있음

완화: broadcast cap 은 BROADCAST_CAPACITY=512 (pty-backend), PaneDied 빈도는 극히 낮음 → 실 production 영향 거의 없음. 정밀한 해결은 *주기적 reconciliation task* (P2+) — pool 의 모든 UUID 가 backend.pane_ids() 에 있는지 확인.

### 5.7 attach_confirm 의 cookie ownership check

`attach_confirm_handler` 는 cookie 가 `session_locks_by_cookie[cookie] == name` 인지 검증. 이유:
- spawn 은 expensive (실제 fork)
- 누구나 confirm 가능하면 — 다른 user 의 unmatched UUID 를 임의로 spawn 시킬 수 있음
- single-attach 정합과도 일치 (한 lock 보유자만 mutate)

403 응답 = `not_attached`.

---

## 6. 테스트 변화

| 시점 | PASS 개수 | 증감 | 신규 |
|---|---|---|---|
| Stage 3 종료 (0031) | 230 | — | — |
| 4-A 종료 | 239 | +9 | terminal_map 9 단위 |
| 4-B 종료 | 245 | +6 | metadata 3 + endpoint 3 |
| 4-C 종료 | 248 | +3 | match-or-spawn 3 |
| 4-D 종료 | 254 | +6 | delete_item 3 + kill/respawn 3 |
| 4-E 종료 | **256** | +2 | handle_pane_died 2 |

전체 증가: **+26**. 모두 http-api crate 내부 (108 → 125, +17 단위/통합) + ws-server / pty-backend / config 변경 없음 → 그쪽은 -0.

### 6.1 테스트 파일 위치

- `crates/http-api/src/terminal_map.rs` `#[cfg(test)] mod tests` — bridge map 단위
- `crates/http-api/src/terminals.rs` `#[cfg(test)] mod tests` — metadata 단위
- `crates/http-api/src/lib.rs` `mod tests` — handler 통합 (전체 라우터 oneshot)

### 6.2 hub-필요 테스트 패턴

`make_state_with_workspace_and_hub` 헬퍼 — `gtmux_pty_backend::PtyBackend::new()` + `gtmux_ws_server::Hub::new(backend)` + `AppState::with_hub_and_workspace`. PtyBackend 의 `new()` 는 child 를 spawn 하지 않음 — `backend.spawn(spec)` 호출 시점에만 fork. 단위 테스트에서는 실 spawn 회피 (map 만 사전 등록).

---

## 7. 빌드 / 실행 / 테스트 명령

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend

# 빌드
cargo build --workspace
cargo build --release --bin gtmux

# 테스트 (현재 256 PASS / 0 FAIL)
cargo test --workspace --color=never

# Stage 4 신규 endpoint 빠른 sanity
curl -X POST http://127.0.0.1:9999/api/sessions/demo/attach \
  -H "Authorization: Bearer $TOKEN"
# → { "name":"demo", "attached":true, "server_id":"...",
#     "matched":[<uuid>...], "unmatched":[<uuid>...] }

curl -X POST http://127.0.0.1:9999/api/sessions/demo/attach/confirm \
  -H "Authorization: Bearer $TOKEN" \
  -b "gtmux_auth=$COOKIE"
# → { "name":"demo", "spawned":[<uuid>...], "already_present":[], "failed":[] }

curl -X GET http://127.0.0.1:9999/api/terminals \
  -H "Authorization: Bearer $TOKEN"
# → [{ "id":"<uuid>", "alive":true, "label":"",
#      "created_at":1715800000, "attach_count":1,
#      "attached_sessions":["demo"] }, ...]

curl -X DELETE "http://127.0.0.1:9999/api/sessions/demo/items/<uuid>?kill_terminal=true" \
  -H "Authorization: Bearer $TOKEN"
# → 204 No Content + ETag header

curl -X POST http://127.0.0.1:9999/api/terminals/<uuid>/respawn \
  -H "Authorization: Bearer $TOKEN"
# → 200 { "id": "<uuid>" }
```

### 7.1 권장 smoke gate (release binary, env -u TMUX)

본 stage 의 smoke 가 추가될 자리:

- **smoke-7** (4-A/B/C): workspace + session create + PUT layout with 2 terminal UUIDs + attach → unmatched=[2] + confirm → spawned=[2] + GET /api/terminals → 2 entries 확인
- **smoke-8** (4-D): delete item with kill_terminal=true → /api/terminals 의 해당 UUID 사라짐
- **smoke-9** (4-D): respawn → 같은 UUID 가 새 created_at 으로 (혹은 보존된 created_at — 본 구현은 *보존*)

자동화 형태는 0031 §6 의 curl-only 패턴 (cookie jar awk 추출) 재사용 가능.

---

## 8. Gotchas / 컨벤션 회로

- **UUID format 강제 위치 차이** (5.5): GET /terminals 의 scan 은 우회, attach/mutate path 는 강제. 테스트 fixture 의 UUID 는 항상 8-4-4-4-12 hex 로.
- **single-attach + flock 으로 race 차단** (5.4): 새 mutate handler 가 추가될 때 ETag CAS 가 필요한지 다시 평가. 대부분 불필요.
- **handle_pane_died 의 broadcast Lagged** (5.6): 매우 드물지만 발생 가능. 정밀화는 P2+.
- **PtyBackend::new() 는 가벼움**: 단위 테스트에서 hub 가 필요할 때 가차없이 생성 가능. `backend.spawn(spec)` 호출 시에만 실 fork 발생.
- **spawn_terminal_with_uuid 의 idempotent fast-path**: 같은 UUID 로 두 번 호출하면 같은 PaneId 반환. confirm endpoint 가 retry-safe 한 핵심.
- **kill_and_unregister_terminal 은 sessions.rs `pub(crate)`**: terminals.rs 의 kill/respawn 도 사용. 두 모듈의 cleanup 진입점 단일화.

---

## 9. Deferred / 미해결

### 9.1 Stage 5 첫 후보 — WS envelope refactor (BE-NEW-4 + BE-NEW-6)

본 stage 의 4-E narrowing 의 결과로 격상.

작업:
1. **session_id 필드를 selection / viewport / focus WS frame 에 추가** (ADR-0021 D5)
2. **server 가 frame 을 *그 session 의 attached webpage* 에만 라우팅** — ws-server 의 connection-table refactor 필요 (현재는 모든 subscriber 에 broadcast). 각 WS 연결이 *어느 session 의 attach* 인지 알아야 함.
3. **`terminal-died` WS frame 신규** — UUID-carrying. PaneDied broadcast 와 별도. 호출 위치: `handle_pane_died` 안에서 (이미 UUID 알려있음). hub.publish_terminal_died(uuid, reason) 같은 API.
4. **Auto-mount trigger-aware** (ADR-0021 D3) — server 가 새 Terminal spawn 후 trigger session 만 mount-cascade, 다른 session 은 terminal-list-update 만. 의존: WS 연결의 session_id 보유.

FE-NEW-6 (multi-xterm + subscriber pattern) 와 정합 필요 → FE/BE 동시 진행 batch.

### 9.2 본 stage 의 후속 잔존

- **PATCH /api/terminals/:id { label }** — 4-B 의 metadata 쓰기. UX flow 분리 필요 시 분리 batch.
- **WS subscriber 의 Lagged reconciliation** (5.6) — 주기 task: `backend.pane_ids()` vs `terminal_map.snapshot()` 비교, 차이 reconcile. P2+.
- **`scan_session_terminal_refs` 의 IO 부담** — GET /terminals 마다 O(N_sessions) file read. N 이 크면 (N>100) cache 도입 검토. 현재로선 GET /terminals 가 hot path 가 아니므로 OK.
- **schema::validate 와 scan_session_terminal_refs 의 정책 차이** (5.5) 명시 ADR 작성 검토 (현재는 본 문서 + code comment 만).

### 9.3 Stage 1~3 잔존 (0031 §9 재게재)

- 옛 `/api/layout` v1 endpoint + `LayoutStore` cleanup
- `LayoutSnapshot` (v1) ↔ `SessionLayout` (v2) 통합
- `gtmux start --session <name>` flag 제거 (workspace+session 모델에서 무의미)
- WS handshake 의 cookie-only 인증 (ADR-0020 D10) — 현재 subprotocol bearer
- Settings API (`GET/PATCH /api/settings`, `POST /api/settings/password`, `POST /api/settings/logout-all`) — Stage 7 BE-9
- Rate limiter 의 X-Forwarded-For 신뢰 정책 (Cloud mode)

---

## 10. cold-pickup 권장 reading order

1. **본 문서 §0 + §1 + §2** — 한 줄 요약 + 파일 인벤토리 + batch 별 산출
2. **본 문서 §9.1** — Stage 5 첫 batch (WS envelope) 의 구체 명세
3. `docs/reports/0031-stage-1-3-multi-session-be-progress.md` — Stage 1~3 의 진실
4. `docs/agents/backend-handover.md` — 전체 stage 명세
5. `docs/adr/0021-terminal-pool-and-mirror.md` D3 / D5 — Stage 5 의 WS frame 정합
6. `docs/adr/0018-canvas-item-data-model.md` D2 / D6 — UUID 의 schema 정합
7. `docs/adr/0013-pty-direct-multiplexer.md` — PaneId 의 현 namespace

### 10.1 첫 명령

```bash
cd /Users/ws/Desktop/projects/gtmux
cat docs/reports/0032-stage-4-terminal-pool-and-pivot-be-progress.md
cat docs/reports/0031-stage-1-3-multi-session-be-progress.md
cat docs/adr/0021-terminal-pool-and-mirror.md
cat docs/adr/0018-canvas-item-data-model.md

cd codebase/backend
cargo test --workspace --color=never 2>&1 | grep "test result:"
# 256 PASS / 0 FAIL 확인

# Stage 5 첫 batch 시작 위치
grep -n "subscribe_pane_output\|subscriber_count\|run_multiplexer" crates/ws-server/src/hub.rs
```

---

## 11. 변경 이력

- 2026-05-15: 초안 — Stage 4 의 5 개 batch 완료 시점의 snapshot. Stage 5 WS envelope refactor brief 포함.
