# 0033 — Next-session handover (Stage 5 entry brief)

- 일자: 2026-05-15
- 작성자: backend agent (Stage 4 + C cleanup 세션의 마지막 산출)
- 종류: cold-pickup brief — 다음 세션이 Stage 5 (WS envelope refactor) 부터 진입할 수 있도록 *전체 context + 완료 + 진행 예정* 을 한 문서에 합본
- 후속 reading order: 본 문서 → `0032-stage-4-terminal-pool-and-pivot-be-progress.md` (Stage 4 진실) → `0031-stage-1-3-multi-session-be-progress.md` (Stage 1~3 진실) → `docs/agents/backend-handover.md` (전체 stage 명세)

---

## 0. 한 줄 요약 + 현 위치

multi-session pivot 의 **Stage 1~4 BE + C 의 cleanup (metadata 보존 + PATCH label)** 이 main 에 커밋됨. **256 → 261 PASS / 0 FAIL**. 다음 진입점 = **Stage 5 WS envelope refactor** (BE-NEW-4 + BE-NEW-6) — FE-NEW-6 와 정합 필요한 큰 refactor.

```
HEAD → 03056bb  fix(backend): preserve terminal metadata across kill+respawn cycle
       bcd54de  feat(backend): multi-session pivot stages 1-4
       1e84f4c  feat(backend): boot-time orphan reap + panels[] strip + GTMUX env marker  ← prev main
```

본 세션의 두 commit (`bcd54de`, `03056bb`) + 0032/0033 doc 가 cold-pickup 의 진입 set.

---

## 1. 본 세션 (2026-05-15) 의 진행

### 1.1 시작 상태 (compact 전)

- 0031 의 Stage 1~3 작업 BE 미커밋 (작업 트리)
- 230 PASS / 0 FAIL
- handover doc: `docs/reports/0031-stage-1-3-multi-session-be-progress.md`

### 1.2 Stage 4 (5 batch) 진행

본 세션 안에서 모든 5 batch 완료 → 그 진실은 `docs/reports/0032-stage-4-terminal-pool-and-pivot-be-progress.md` 의 §2.

| batch | 산출 | PASS 증가 |
|---|---|---|
| 4-A | UUID ↔ PaneId bridge (`TerminalMap` + `spawn_terminal_with_uuid`) | +9 |
| 4-B | `GET /api/terminals` + `TerminalMetadataStore` | +6 |
| 4-C | Match-or-spawn (`attach` 응답 확장 + `POST /attach/confirm`) | +3 |
| 4-D | DELETE item + `kill_handler` + `respawn_handler` | +6 |
| 4-E (narrowed) | `BackendNotify::PaneDied` auto-unregister consumer | +2 |
| **소계** | 226 → **256** | **+26** |

### 1.3 commit A — Stage 1~4 BE (`bcd54de`)

34 files, +12,102 / -404. 포함:
- BE 코드: `codebase/backend/{Cargo.{lock,toml}, bin/gtmux-cli/*, crates/{config,http-api,ws-server}/**}`
- 신규 ADR: 0018 (canvas item data model), 0019 (session+workspace model), 0020 (auth lifecycle), 0021 (terminal pool + mirror), 0023 (file-path open security), 0024 (layer tree + z-index separation)
- 기존 ADR amend: 0006 D14/D15, 0007 supersede note, 0010 D10, 0015 D3
- plans: 0006 (canvas workspace feature roadmap), 0007 (multi-session pivot)
- agents: backend-handover (v1/v2/v3)
- reports: 0031, 0032
- CONTEXT.md vocabulary refresh (Pane→Terminal, Workspace 신규, Session 의미 재정의)

**의도적 미커밋** (다른 commit 으로):
- `codebase/frontend/` 모든 변경 — 병렬 FE agent 의 작업
- `docs/demo*`, `docs/agents/frontend-handover*` — FE 관련
- `.agents/`, `.codex/`, `.claude/*`, `AGENTS.md`, `skills-lock.json` — system/external config
- `experiments/`, `ref/` — workspace artifacts

### 1.4 smoke gate 7~9 검증

release binary 로 end-to-end 검증 (`env -u TMUX target/release/gtmux start --session stage4smoke --port 9991 --workspace /tmp/...`):

- **7a~7e**: workspace + session create + PUT 2 terminal items + attach → matched=0/unmatched=2 + confirm → spawned=2 + GET /api/terminals → 2 entries (모두 PASS)
- **8**: DELETE item `?kill_terminal=true` → 204 + GET → 1 entry, UUID 사라짐 (PASS)
- **9**: respawn UUID → 200 + created_at 보존 확인 → **첫 run 시 DRIFT 검출** (documented behavior 위반)

smoke 스크립트는 `/tmp/gtmux-smoke-stage4.sh` 에 머무름 (다음 세션에서 재사용 가능, `--session stage4smoke` 로 unique 한 이름 사용).

### 1.5 commit B — C cleanup (`03056bb`)

3 files, +243 / -12. 의도:
- smoke-9 가 적발한 *created_at 가 respawn 후 변함* 문제 수정
- BE-8 의 label 쓰기 endpoint 추가

**메타데이터 라이프사이클 재설계**:
- 이전: `handle_pane_died` / `kill_and_unregister_terminal` 둘 다 `meta.forget(uuid)` 호출 → respawn 시 새 `created_at`
- 이후: 두 함수에서 `forget` 분리. metadata 는 *사용자 명시 retire* 에만 drop —
  - `DELETE /api/sessions/:name/items/:id?kill_terminal=true` (schema item gone)
  - `POST /api/terminals/:id/kill` (explicit "I'm done")
- transient kill (kernel SIGCHLD, respawn) 은 metadata 보존
- 회귀 unit `pane_died_then_respawn_round_trip_preserves_created_at` 추가

**PATCH /api/terminals/:id**:
- `{ label: string }` body
- 4 KiB cap (`MAX_LABEL_BYTES = 4096`)
- 404 if UUID not in `terminal_meta`
- 400 if label too long
- `TerminalMetadataStore::set_label` helper 복원 (4-B 에서 사변적 제거됐던 것)

테스트 +5 (260 → 261): 1 회귀 + 1 set_label 단위 + 3 PATCH integration.

### 1.6 본 세션 의 deferral

- **B (Stage 5 WS envelope refactor)** — 본 세션 시작 시 권장한 `A → C → B` 순서 중 B 는 다음 세션. context 빌드업 필요 + FE-NEW-6 의존.
- **`--session <name>` flag 제거** — Stage 4 §9.3 잔재. load-bearing (token 파일 명명 / pid lock), Stage 5 의 CLI 정리와 함께.
- **legacy `/api/layout` v1 + `LayoutStore` cleanup** — FE 가 아직 참조 가능성. Stage 5+.
- **`LayoutSnapshot` ↔ `SessionLayout` 통합** — 위 cleanup 의 일부.

---

## 2. 누적 진척 (Stage 1~4 + cleanup)

### 2.1 단계별 진실의 출처

| Stage | 진실 문서 | 한 줄 |
|---|---|---|
| 1 (Foundation) | 0031 §2 (Stage 1) | Workspace + Session storage + path resolution + v1→v2 migration |
| 2 (Auth lifecycle) | 0031 §2 (Stage 2) | cookie + token + password (Argon2id) + RateLimiter + session_table |
| 3 (Cross-server lock) | 0031 §2 (Stage 3) | flock + lease + WS heartbeat 15s/30s + cookie-driven auto-release |
| 4 (Terminal pool + pivot) | 0032 §2 (Stage 4 batches) | UUID bridge + match-or-spawn + kill/respawn + auto-unregister |
| C (cleanup) | 본 문서 §1.5 | metadata preservation fix + PATCH label |

### 2.2 ADR 매트릭스 (현재 효력)

| ADR | 제목 | 본 stage 안 역할 |
|---|---|---|
| 0006 | Persistence storage | D13 atomic write, D5 ETag, D10 7-state, D15 (v2 hard cutover, D14 obsolete) |
| 0007 | Server session port binding | 본 pivot 으로 supersede 됨 (single-session) → 0019 의 multi-session 으로 대체 |
| 0010 | Group data model | D10 amend (group close 의 bulk dialog) |
| 0013 | PTY direct multiplexer | PaneId 의 namespace 정의 (Option B bridge 의 left side) |
| 0014 | Process supervisor | D1 owner / D10 child reap / D11 GTMUX env marker |
| 0015 | Pane auto-mount | D3 amend by 0021 D3 (trigger session 만 cascade) |
| 0018 | Canvas item data model (v2) | D2 (terminal item.id = UUID = backend Terminal.id), D6 (match-or-spawn), D8 (validation) |
| 0019 | Session + Workspace model | D1 workspace, D3 single-attach, D6 (cross-server flock + lease) |
| 0020 | Auth lifecycle | D2 cookie session, D5 rate limit + Argon2id, D10 cookie-only WS (Stage 5+) |
| 0021 | Terminal pool + mirror | D1 multi-attach, D3 trigger-aware auto-mount, D5 session-scoped state, D6 heartbeat, D9 close 분리, D10 dangling 의 lazy spawn |
| 0023 | File-path open security | (FE 관련) |
| 0024 | Layer tree + z-index | D2 z 4 액션, ADR-0018 D3 의 z 정합 amend |

### 2.3 AppState 의 최종 모양 (Stage 4 + C 종료)

```rust
pub struct AppState {
    // 인증 (Stage 2)
    pub config: Arc<Config>,
    pub token: Arc<TokenString>,              // /gtmux start 가 발행한 Bearer
    pub auth_failure_counter: Arc<AtomicU64>,
    pub session_table: Arc<SessionTable>,     // cookie → AuthSession (in-memory, rolling 7d)
    pub rate_limiter: Arc<RateLimiter>,       // sliding-window per-IP for /auth/login
    pub password_hash: Option<Arc<String>>,   // PHC Argon2id, password mode only

    // 정적 자원 (Stage 1 legacy + Stage 4 hub)
    pub hub: Option<gtmux_ws_server::Hub>,
    pub store: Option<Arc<LayoutStore>>,      // 옛 /api/layout v1 storage
    pub layout: Arc<RwLock<LayoutSnapshot>>,  // 옛 v1 in-memory snapshot

    // 멀티 세션 (Stage 1+)
    pub workspace: Option<Arc<WorkspaceManager>>,
    pub session_cache: Arc<SessionCache>,     // name → SessionLayout (lazy load)

    // Cross-server lock (Stage 3)
    pub server_id: Arc<str>,                  // UUID v4, boot 시 1회 mint
    pub session_locks: Arc<Mutex<HashMap<String, LockGuard>>>,           // name → flock
    pub session_locks_by_cookie: Arc<Mutex<HashMap<String, String>>>,    // cookie → name

    // Terminal pool (Stage 4)
    pub terminal_map: Arc<TerminalMap>,                  // UUID ↔ PaneId bridge (4-A)
    pub terminal_meta: Arc<TerminalMetadataStore>,       // UUID → label/created_at (4-B)
}
```

비동기 메서드:
- `spawn_terminal_with_uuid(uuid) -> Result<PaneId, SpawnTerminalError>` (4-A) — production spawn 진입
- `handle_pane_died(pane)` (4-E) — CLI consumer task 의 진입
- `release_lock_for_cookie(cookie)` (Stage 3) — WS disconnect consumer 의 진입
- `refresh_lease_for_cookie(cookie)` (Stage 3) — WS heartbeat consumer 의 진입

빌더 chain:
```rust
AppState::with_hub_and_path(config, token, hub, layout_path)
    .with_workspace(workspace)
    .with_password_hash(hash)        // optional
```

### 2.4 라우트 surface (Stage 1~4 + C 종료)

```
GET    /healthz                                  # no auth
GET    /auth                                     # token verify or password form
GET    /auth/bootstrap                           # legacy → 303 to /auth?token=
POST   /auth/login                               # { token | password } + cookie
POST   /auth/logout                              # session_table revoke

GET    /api/layout                               # legacy v1 single-session
PUT    /api/layout                               # ETag CAS (legacy)

GET    /api/sessions                             # list + active 플래그
POST   /api/sessions                             # create { name }
DELETE /api/sessions/:name                       # delete record
GET    /api/sessions/:name/layout                # v2 + ETag
PUT    /api/sessions/:name/layout                # v2 ETag CAS
POST   /api/sessions/:name/attach                # flock acquire + matched/unmatched 반환 (Stage 4-C)
DELETE /api/sessions/:name/attach                # flock release
POST   /api/sessions/:name/attach/confirm        # unmatched 마다 spawn (Stage 4-C)
DELETE /api/sessions/:name/items/:id             # ?kill_terminal=bool (Stage 4-D)

GET    /api/terminals                            # pool + meta + cross-ref (Stage 4-B)
PATCH  /api/terminals/:id                        # { label } (Stage 4 cleanup C)
POST   /api/terminals/:id/kill                   # explicit terminate (Stage 4-D)
POST   /api/terminals/:id/respawn                # same UUID, fresh PaneId (Stage 4-D)
```

WS (`ws-server`): `/ws` — subprotocol `bearer.<token>`, cookie 추출 (heartbeat/disconnect routing), 15s/30s heartbeat. **Stage 4 안에서 변경 없음.** Stage 5 의 refactor 대상.

---

## 3. 현 git / 작업 트리 상태

```bash
$ git log --oneline -3
03056bb fix(backend): preserve terminal metadata across kill+respawn cycle
bcd54de feat(backend): multi-session pivot stages 1-4
1e84f4c feat(backend): boot-time orphan reap + panels[] strip + GTMUX env marker
```

미커밋 (의도적):
- `codebase/frontend/**` (modified + 다수 untracked) — 병렬 FE 세션의 작업
- `docs/agents/frontend-handover{,-v2,-v3}.md`
- `docs/demo-guide.md`, `docs/demo/`
- `experiments/`, `ref/`
- `.agents/`, `.codex/`, `.claude/scheduled_tasks.lock`, `AGENTS.md`, `skills-lock.json`

이들은 BE 의 책임이 아니므로 본 핸드오버는 다루지 않음. FE handover 가 별도 존재.

---

## 4. Stage 5 (다음 진입점) — 상세 명세

원안: 0032 §9.1 "WS envelope refactor (BE-NEW-4 + BE-NEW-6)". 본 문서에서 구체화.

### 4.1 작업 4 항목

#### 4.1.A — `session_id` 필드를 selection/viewport/focus WS frame 에 추가 (ADR-0021 D5)

현재: server 가 selection-changed / viewport-changed / focus-changed 같은 frame 을 **모든 WS subscriber** 에 broadcast. ADR-0021 D5 는 *session-scoped* — 즉 그 session 의 attached webpage 에만 송신.

```
구 (현재):
  { type: "selection-changed", panels: [...] }  // server-wide
새 (Stage 5):
  { type: "selection-changed", session_id: "demo", panels: [...] }
  // → server 가 frame 을 *그 session 의 attached webpage* 에만 send.
```

#### 4.1.B — WS 연결의 session_id 보유 (4.1.A 의 prereq)

각 WS 연결이 *어느 session 의 attach* 인지 알아야 함. 현재 ws-server 의 connection-table 은 cookie + bearer 만 트랙.

option:
- **(i) WS handshake 의 query string** `?session=<name>` (clean, FE 의 attach 흐름과 정합)
- **(ii) attach handler 가 cookie ↔ session 매핑을 ws-server 에 push** (server-side, cookie 만 신뢰)
- **(iii) WS frame 안에서 first message 로 session 선언** (handshake X, app layer)

권장: **(ii)** — 이미 `AppState.session_locks_by_cookie` 가 매핑 보유. ws-server 가 cookie 를 lookup 으로 변환할 수 있도록 callback 또는 Arc<RwLock<HashMap>> 공유.

ws-server 의 connection-table 에 `session_id: Option<String>` 추가:
- handshake 직후 cookie 로 `session_locks_by_cookie` lookup → session_id 캐시
- attach 가 *나중에* 일어나면 (WS 먼저 → attach 후) 어떻게 알리나? → session_id 가 변경되면 push 알림 채널 필요. 또는 attach handler 에서 직접 ws-server 의 connection-table 갱신 (cookie key 로).

**구체 plan**: 새 `Hub::set_session_for_cookie(cookie, session_name)` API. attach_handler 가 호출. dispatcher 가 frame 라우팅 시 connection-table 에서 cookie → session 확인.

#### 4.1.C — UUID-carrying `terminal-died` WS frame 신규

현재: `BackendNotify::PaneDied { id: PaneId }` 가 broadcast. FE 는 PaneId 만 받고 schema UUID 와 어떻게 매칭하나? → GET /api/terminals 로 cross-ref. 불편함.

새 frame:
```json
{ "type": "terminal-died", "terminal_id": "<uuid>", "reason": "exit" | "killed" }
```

발행 위치: `AppState::handle_pane_died` 안에서 — UUID 가 막 unregister 되기 직전에 cache 한 뒤 broadcast. 또는 hub 에 `publish_terminal_died(uuid, reason)` API 추가.

server-wide broadcast (모든 session 의 webpage 에 전달) — 같은 terminal 이 여러 session 의 panel 일 수 있으므로 (ADR-0021 D1 mirror).

#### 4.1.D — Auto-mount trigger-aware (ADR-0021 D3)

server 가 새 Terminal spawn 후:
- **trigger session 의 webpage**: `mount-cascade { terminal_id, x, y, w, h }` → FE 가 layout 에 panel append + spawn 의 좌표 사용
- **다른 session 의 webpage**: `terminal-list-update { added: [terminal_id] }` → FE 의 Terminal list UI 갱신만, 자기 layout 은 건드리지 않음

prereq: 4.1.B (WS 연결의 session_id 보유).

발행 위치: `attach_confirm_handler` 의 spawn loop 안 — spawn 1 회마다 publish. 또는 spawn_terminal_with_uuid 자체에 trigger_session 파라미터 추가.

권장: handler 안에서 명시 publish — `spawn_terminal_with_uuid` 는 trigger 의식 없어야 함 (어떤 path 에서든 호출 가능).

### 4.2 진행 순서 (Stage 5 batch)

| ID | 의존 | 작업 |
|---|---|---|
| **5-A** | (단독) | Hub 의 connection-table 에 cookie → session_id 매핑 + `Hub::set_session_for_cookie` API. attach_handler 가 호출. 회귀 테스트: WS 연결 시 attach 가 있으면 lookup 성공 |
| **5-B** | 5-A | `terminal-died` frame 발행 — `handle_pane_died` 가 broadcast. WS subscriber 가 frame 받음 확인 |
| **5-C** | 5-A | `session_id` 필드를 기존 session-scoped frames 에 추가 + ws-server 의 dispatcher 가 session_id 로 라우팅. selection / viewport / focus 가 cross-session leakage 없음 확인 |
| **5-D** | 5-A | Auto-mount trigger-aware: spawn 호출 점에서 `mount-cascade` (trigger session) vs `terminal-list-update` (other sessions) 분기 |

각 batch 의 산출물:
- 5-A: `crates/ws-server/src/hub.rs` 의 connection-table 확장 + `crates/http-api/src/sessions.rs` 의 attach_handler 가 hub API 호출
- 5-B: `BackendNotify` 와 별도의 새 frame kind `terminal-died` + hub publisher
- 5-C: session-scoped frame envelope + ws-server dispatcher
- 5-D: trigger-aware spawn publisher

FE-NEW-6 (multi-xterm subscriber pattern) 와의 정합이 필요. FE 측 진행을 모르는 상태에서는 BE 단독으로 5-A/5-B 까지 진행 가능 (FE wire 변경 없음 — *추가* frame 만). 5-C/5-D 는 FE dispatcher 의 새 frame 처리 필요.

### 4.3 Stage 5 의 위험 / 주의

- **WS-server 의 connection-table 동시성**: 다수 WS 가 동시 handshake — locking 의 fast/slow path 설계 필요. `tokio::sync::RwLock<HashMap<ConnId, Conn>>` 단순화 가능.
- **session_id 의 변경 timing**: WS handshake 시 attach 가 아직 없을 수 있음 (FE 가 WS 먼저, attach 나중에). connection-table 의 session_id 는 *변경 가능* 필드여야 함. attach handler 가 set, detach handler 가 unset.
- **frame 라우팅의 fallback**: session_id 가 없는 connection (어떤 session 도 attach 안 함) 에는 session-scoped frame 송신 X. server-wide frame (pane-output / terminal-died) 은 그대로 broadcast.
- **`subscribe_pane_output`** (기존 broadcast) 는 변경 X — output stream 은 항상 server-wide.
- **wire backward compat**: FE 가 옛 frame shape 도 받아들이도록 — session_id 가 *optional* 필드. 이 점은 FE-NEW-6 와 같이 결정.

### 4.4 Stage 5 의 deferred (5+1)

- `--session <name>` flag 제거 — Stage 5 의 CLI 정리 sub-batch
- legacy `/api/layout` v1 + `LayoutStore` cleanup
- `LayoutSnapshot` (v1) ↔ `SessionLayout` (v2) 통합
- WS handshake 의 cookie-only 인증 (ADR-0020 D10) — 현재 subprotocol bearer
- Settings API (`GET/PATCH /api/settings`, `POST /api/settings/password`, `POST /api/settings/logout-all`) — Stage 7 BE-9
- Rate limiter 의 X-Forwarded-For 신뢰 정책 (Cloud mode)

---

## 5. 핵심 결정 / 회로 요약 (cold-pickup 시 필수 인지)

본 stage 동안 굳어진 회로. 자세한 reasoning 은 0031 §4 + 0032 §5.

| 영역 | 결정 | 출처 |
|---|---|---|
| PaneId namespace | Option B (bridge map) — wire 는 u64 그대로, schema 만 UUID | 0032 §5.1 |
| metadata 라이프사이클 | transient kill 보존, *명시 사용자 retire* 에만 forget | 본 §1.5 |
| schema validation | `attach`/`PUT layout` 강제, `GET /terminals` scan 우회 | 0032 §5.5 |
| ETag CAS | `layout PUT` 만 적용. `DELETE item` 은 single-attach 모델로 불필요 | 0032 §5.4 |
| cookie의 두 의미 | bearer = stable token, session cookie = base64url 43-char | 0031 §4.3 |
| WS heartbeat | 15s ping / 30s timeout (RFC 6455 0x9/0xA) | 0031 §3.2 |
| spawn idempotency | same UUID 재호출 → 같은 PaneId, race 시 loser 의 PaneId kill | 0032 §5.3 |
| broadcast Lagged | `handle_pane_died` consumer 는 warn + 계속. 정밀화 P2+ | 0032 §5.6 |
| attach_confirm 권한 | cookie 가 lock holder 일 때만 spawn 허용 (403 otherwise) | 0032 §5.7 |
| `kill_and_unregister_terminal` 의 부재 forget | metadata 보존 의 핵심. 호출 후 명시 `meta.forget` 필요한 path 만 호출 | 본 §1.5 |

---

## 6. 빌드 / 실행 / 테스트 명령

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend

# 빌드
cargo build --workspace
cargo build --release --bin gtmux

# 테스트 (current 261 PASS / 0 FAIL)
cargo test --workspace --color=never 2>&1 | grep "test result:"

# clippy (신규 코드만 clean, 기존 dead_code 2 warnings 는 pre-existing)
cargo clippy -p gtmux-http-api --no-deps --color=never

# Stage 4 smoke 재실행
/tmp/gtmux-smoke-stage4.sh   # 기존 스크립트 보존, --session stage4smoke 사용

# 외부 tmux 안에서는 실행 불가 — 항상 `env -u TMUX`
env -u TMUX ./target/release/gtmux start \
  --session demo \
  --port 9999 \
  --workspace /tmp/ws-demo
```

새 endpoint 의 curl sanity:

```bash
TOKEN=$(cat ~/.local/state/gtmux/demo.token)
H="-H Authorization: Bearer $TOKEN -H Host: 127.0.0.1:9999"

# attach
curl -sS $H -X POST http://127.0.0.1:9999/api/sessions/demo/attach | jq

# confirm
curl -sS $H -X POST http://127.0.0.1:9999/api/sessions/demo/attach/confirm | jq

# list
curl -sS $H http://127.0.0.1:9999/api/terminals | jq

# label set
curl -sS $H -X PATCH http://127.0.0.1:9999/api/terminals/<uuid> \
  -H "Content-Type: application/json" -d '{"label":"build watch"}'

# kill (drops metadata)
curl -sS $H -X POST http://127.0.0.1:9999/api/terminals/<uuid>/kill

# respawn (preserves metadata)
curl -sS $H -X POST http://127.0.0.1:9999/api/terminals/<uuid>/respawn

# delete item with kill
curl -sS $H -X DELETE "http://127.0.0.1:9999/api/sessions/demo/items/<uuid>?kill_terminal=true"
```

---

## 7. 다음 세션 진입 명령 + reading order

```bash
cd /Users/ws/Desktop/projects/gtmux

# 1. 본 문서 + 두 진행 보고
cat docs/reports/0033-next-session-handover-stage-5-entry.md
cat docs/reports/0032-stage-4-terminal-pool-and-pivot-be-progress.md
cat docs/reports/0031-stage-1-3-multi-session-be-progress.md

# 2. 전체 stage 명세
cat docs/agents/backend-handover.md

# 3. Stage 5 의 핵심 ADR
cat docs/adr/0021-terminal-pool-and-mirror.md   # D3 trigger-aware + D5 session-scoped
cat docs/adr/0019-session-and-workspace-model.md  # D3 single-attach
cat docs/adr/0018-canvas-item-data-model.md    # D2 UUID = terminal.id

# 4. 현 코드 상태 확인
cd codebase/backend
git log --oneline -5
cargo test --workspace --color=never 2>&1 | grep "test result:"
# expected: 256 PASS / 0 FAIL (workspace 합산)

# 5. Stage 5-A 진입 — Hub connection-table 확장
grep -rn "subscribe_pane_output\|set_disconnect_sink\|set_heartbeat_sink" \
  crates/ws-server/src/hub.rs

# 6. Stage 5-A 의 시작 결정 포인트
#    - Hub 의 새 API set_session_for_cookie 의 시그니처
#    - connection-table 자료구조 (Arc<RwLock<HashMap<...>>>?)
#    - attach_handler 와의 정합 (call site)
```

### 7.1 reading order 권장

1. **본 문서 §0 + §1 + §4** — 한 줄 요약 + 본 세션 진행 + Stage 5 4 항목
2. **0032 §5 + §9** — 핵심 결정 (cold-pickup 시 회로 인지) + deferred
3. **0031 §2** — Stage 1~3 의 진실 (AppState 의 전체 모양 이해)
4. **handover.md** — 전체 stage 명세 (Stage 5 의 큰 그림)
5. **ADR 0021 + 0019 + 0018** — Stage 5 의 wire frame 정합

---

## 8. Stage 5 시작 시 의사결정 포인트

다음 agent 가 처음 마주칠 결정들 — 본 문서에서 답 제시 또는 옵션 표시.

### 8.1 5-A 의 connection-table 자료구조

option:
- **(i) `Arc<RwLock<HashMap<ConnId, ConnState>>>`** — ws-server 의 새 module
- **(ii) `DashMap<ConnId, ConnState>`** — lock-free, dep 추가 X (`dashmap` 이미 workspace 일 수 있음, 확인)
- **(iii) Hub 의 기존 broadcast Sender 옆에 in-process registry**

권장: **(i)** — 단순. ConnId 는 fresh UUID v4 mint (terminal_map 의 fresh_terminal_uuid 재사용). 동시성 부담 작음 (handshake 마다 1 회 lock).

### 8.2 attach_handler 가 ws-server 와 통신하는 방법

option:
- **(i) Hub method 직접 호출** `state.hub.as_ref()?.set_session_for_cookie(cookie, name)` — 의존성 hop 추가
- **(ii) attach_handler 가 채널로 ws-server 에 push** (현재 disconnect/heartbeat 패턴)
- **(iii) Hub 가 *cookie ↔ session_locks_by_cookie* 의 Arc 사본을 보유** — pull model

권장: **(i)** — read path 의 단순함. WS handshake 가 cookie 로 session lookup 시 hub method 호출.

### 8.3 `terminal-died` frame 의 발행 위치

option:
- **(i) `handle_pane_died` 안에서** — UUID 가 막 unregister 되기 직전에 cache
- **(ii) hub 의 `publish_terminal_died(uuid, reason)` API** — handler 가 호출

권장: **(ii)** — 명시적이고 같은 hub 의 publish 패턴 (publish_layout_changed 와 정합)

### 8.4 session_id field 의 wire 호환

option:
- **(i) `session_id: String` (required)** — FE 가 항상 보냄/받음
- **(ii) `session_id: Option<String>` (optional)** — 옛 FE 는 fallback (server-wide 으로 해석)

권장: **(ii)** — Stage 5 의 점진 rollout 안전. FE-NEW-6 가 동시 진행 안 되어도 BE-only deploy 가능.

---

## 9. 변경 이력

- 2026-05-15: 초안. Stage 4 + C cleanup 완료 시점의 cold-pickup brief.

---

## 10. 핵심 file 인벤토리 (Stage 5 진입 시 자주 보는)

| 파일 | 본 stage 안 책임 | Stage 5 의 변경 예상 |
|---|---|---|
| `crates/http-api/src/lib.rs` | `AppState` (15 필드), 비동기 메서드, 라우트 mount, 미들웨어 | 5-A 의 hub API 호출 (attach handler) |
| `crates/http-api/src/sessions.rs` | session HTTP handler 7 개 + match-or-spawn helper | 5-A 의 attach 시 hub 알림 추가 |
| `crates/http-api/src/terminals.rs` | terminal HTTP handler 4 개 + metadata store | 5-D 의 spawn publisher 추가 가능 |
| `crates/http-api/src/terminal_map.rs` | UUID ↔ PaneId bridge | 변경 없음 (read-only by 5-A/B/C/D) |
| `crates/http-api/src/schema.rs` | v2 schema + validation | 변경 없음 |
| `crates/http-api/src/session_lock.rs` | flock + lease | 변경 없음 |
| `crates/http-api/src/auth.rs` | 인증 lifecycle | 변경 없음 |
| `crates/http-api/src/workspace.rs` | workspace path / enumerate | 변경 없음 |
| `crates/ws-server/src/lib.rs` | WS handler + heartbeat + cookie 추출 | **5-A의 핵심** — connection-table 통합 |
| `crates/ws-server/src/hub.rs` | Hub broadcast + sink 등록 | **5-A의 핵심** — set_session_for_cookie / publish_terminal_died |
| `bin/gtmux-cli/src/main.rs` | boot wiring + 3 consumer task | 5-A 의 hub API 와 정합 확인 |
| `crates/pty-backend/src/lib.rs` | PtyBackend + PaneId + BackendNotify | 변경 없음 (Option B 유지) |
| `crates/config/src/lib.rs` | Config schema | 변경 없음 |

---

본 문서 + 0032 + 0031 + handover.md = cold-pickup 진입 set. 다음 세션은 위 §7 명령으로 시작.
