# 0031 — Stage 1~3 multi-session pivot backend progress (cold-pickup handover)

- 일자: 2026-05-15
- 작성자: backend agent (multi-session pivot 진행)
- 종류: 진행 snapshot — 다음 세션의 cold-pickup brief
- 후속 reading order: 본 문서 → `docs/agents/backend-handover.md` (전체 stage 명세) → 본 문서 §"다음 단계" 항목별 ADR

---

## 0. 한 줄 요약

`docs/agents/backend-handover.md` 의 **Stage 1 (Foundation) / Stage 2 (Auth lifecycle) / Stage 3 batches 1·2·3 (cross-server lock + heartbeat + lease refresh)** 의 BE 작업이 완전 구현됨. 다음 cold-pickup 은 **Stage 4 (Terminal pool + multi-attach mirror)** 진입 — 본 stage 의 핵심은 PaneId(u64) → UUID 마이그레이션 + match-or-spawn 알고리즘.

---

## 1. 현 코드 상태 스냅샷

- 작업 트리: `codebase/backend/` 만 amend, 미커밋
- `cargo test --workspace`: **230 PASS / 0 FAIL**
- smoke gates 1~6 모두 release-binary 에서 통과
- TMUX 가드 (ADR-0014 D10) 활성 — 모든 smoke 는 `env -u TMUX` 로 우회
- workspace MSRV: `rust-version = "1.85"` (rpassword 7.5 의 let-chains 회피 위해 7.3.1 pin)

### 1.1 누적 신규/수정 파일

| 파일 | 종류 | 핵심 책임 |
|---|---|---|
| `crates/http-api/src/schema.rs` | 신규 | Canvas Layout schema v2 (`Layout/Item/Group/Viewport`), 10-variant discriminated union, validation, v1→v2 migration helper |
| `crates/http-api/src/workspace.rs` | 신규 | `WorkspaceManager` — XDG_DATA_HOME 기반 path resolve + `.locks/` 0700 + session name regex + enumerate + boot migration |
| `crates/http-api/src/sessions.rs` | 신규 | `SessionCache` (lazy load) + 5 handler (list/create/delete/layout get/put + attach/detach) + per-session quarantine |
| `crates/http-api/src/auth.rs` | 신규 (~700 LoC) | `SessionTable` + `RateLimiter` + Argon2id + 3 handler (`GET /auth`, `POST /auth/login`, `POST /auth/logout`) + cookie helpers |
| `crates/http-api/src/session_lock.rs` | 신규 | `acquire`/`peek`/`release`/`refresh_lease` + `LockGuard` (RAII) + `fresh_server_id` |
| `crates/http-api/src/lib.rs` | 큰 amend | `AppState` 가 9 개 도메인 필드를 보유 (§2.1 참조); 라우트 wire; 미들웨어 단일화 |
| `crates/http-api/src/storage.rs` | 변경 없음 (Stage 1~3) | 옛 `/api/layout` v1 — 그대로 작동, 추후 cleanup |
| `crates/config/src/lib.rs` | amend | `workspace_path: Option<PathBuf>` + `AuthConfig { mode, cookie_max_age_days, rate_limit_per_5min }` |
| `crates/ws-server/src/lib.rs` | amend | heartbeat 15s/30s, cookie 추출 + `emit_heartbeat` / disconnect_sink push |
| `crates/ws-server/src/hub.rs` | amend | `disconnect_tx` + `heartbeat_tx` (Arc<Mutex<Option<UnboundedSender<String>>>>) + setter/getter |
| `bin/gtmux-cli/src/main.rs` | amend | `--workspace` flag; `SetPassword/ResetPassword` 서브커맨드; `build_app` → `build_app_state` + `build_router` 분리; mpsc 두 채널 + consumer tasks |
| `bin/gtmux-cli/Cargo.toml` | amend | `rpassword` dep |
| `Cargo.toml` (workspace) | amend | `argon2 = "0.5"`, `rpassword = "=7.3.1"`, `fs2 = "0.4"` 추가 |
| `crates/http-api/Cargo.toml` | amend | argon2 / base64 / fs2 / atomic-write-file deps |

---

## 2. AppState 의 현 모양 (다음 작업의 진실)

### 2.1 필드

```rust
pub struct AppState {
    // 인증
    pub config: Arc<Config>,
    pub token: Arc<TokenString>,              // /gtmux start 시 발행한 stable token (Bearer)
    pub auth_failure_counter: Arc<AtomicU64>,
    pub session_table: Arc<SessionTable>,     // cookie → AuthSession (in-memory, rolling 7d)
    pub rate_limiter: Arc<RateLimiter>,       // sliding-window per-IP for /auth/login
    pub password_hash: Option<Arc<String>>,   // PHC Argon2id, password mode only

    // 정적 자원
    pub hub: Option<gtmux_ws_server::Hub>,
    pub store: Option<Arc<LayoutStore>>,      // 옛 /api/layout v1 storage
    pub layout: Arc<RwLock<LayoutSnapshot>>,  // 옛 v1 in-memory snapshot

    // 멀티 세션 (Stage 1+)
    pub workspace: Option<Arc<WorkspaceManager>>,
    pub session_cache: Arc<SessionCache>,     // (name) → SessionLayout (lazy load)

    // Cross-server lock (Stage 3)
    pub server_id: Arc<str>,                  // UUID v4, boot 시 1회 mint
    pub session_locks: Arc<Mutex<HashMap<String, LockGuard>>>,           // name → flock
    pub session_locks_by_cookie: Arc<Mutex<HashMap<String, String>>>,    // cookie → name
}
```

### 2.2 빌더 chain

```rust
AppState::with_hub_and_path(config, token, hub, layout_path)
    .with_workspace(workspace)
    .with_password_hash(hash)        // optional
```

### 2.3 비동기 API

- `release_lock_for_cookie(cookie)` — WS-close consumer 가 호출. flock + unlink + 두 map prune.
- `refresh_lease_for_cookie(cookie)` — WS-heartbeat consumer 가 호출. `LockGuard::refresh_lease` → `lease_until_unix = now + 30s`.

---

## 3. HTTP / WS 라우트 surface

### 3.1 HTTP (`http-api` crate)

```
GET    /healthz                       # no auth
GET    /auth                          # token-mode 자동 verify or password HTML form
GET    /auth/bootstrap                # legacy → 303 redirect to /auth?token=…
POST   /auth/login                    # { token | password } + cookie 발행
POST   /auth/logout                   # session_table revoke + Max-Age=0
GET    /api/layout                    # 옛 v1 single-session (legacy, 살아있음)
PUT    /api/layout                    # ETag CAS (legacy)
GET    /api/sessions                  # list + active 플래그 via flock peek
POST   /api/sessions                  # create { name }
DELETE /api/sessions/:name            # delete
GET    /api/sessions/:name/layout     # v2 schema + ETag
PUT    /api/sessions/:name/layout     # v2 ETag CAS
POST   /api/sessions/:name/attach     # flock acquire + cookie binding
DELETE /api/sessions/:name/attach     # flock release
```

### 3.2 WS (`ws-server` crate)

- `/ws` — 변동 없음 (subprotocol bearer 인증 그대로)
- Heartbeat: server-side `PING_INTERVAL = 15s`, `PONG_TIMEOUT = 30s`
- 새 side-effect: Pong/Ping 수신 시 `Hub.heartbeat_tx` 로 cookie emit; close 시 `Hub.disconnect_tx` 로 cookie emit
- WS 인증은 여전히 subprotocol `bearer.<token>` (ADR-0020 D10 의 "cookie-only WS auth" 는 Stage 5+)

---

## 4. 핵심 결정 (다음 agent 가 기억해야 할 것)

### 4.1 schema.rs 의 `#![allow(missing_docs)]`

`Item` enum 의 10 variant + flat field 들은 ADR-0018 D3 표가 정본 — module-level allow 로 처리. 새 필드 추가 시 ADR-0018 amend 가 먼저, 그다음 코드.

### 4.2 PaneId vs Terminal UUID 분리

현재 pty-backend 의 `PaneId(pub u64)` 는 손대지 **않음**. schema v2 의 `Item::Terminal.id` 는 UUID-shaped string. **두 식별자가 같은 entity 를 가리키지만 wire/storage 형식이 다름**. Stage 4 의 첫 작업이 이 두 id 의 매칭 layer 결정.

### 4.3 Cookie 의 두 가지 의미

- **Stable token** (`gtmux start` 발행, `<session>.token` 파일에 저장) — Authorization: Bearer 로 들어옴, constant-time 비교
- **Session cookie** (`auth.rs` 의 `SessionTable::issue` 가 발행) — `gtmux_auth=<43-char base64url>`, 반드시 session_table lookup. **raw token 과 절대 같지 않음**

미들웨어가 둘 다 인정함 (`crates/http-api/src/auth.rs::authenticate`).

### 4.4 WS heartbeat / disconnect 채널의 ordering

CLI boot 에서:
1. Hub 생성
2. mpsc 채널 두 개 (disconnect, heartbeat) 생성
3. `hub.set_disconnect_sink(tx)` / `hub.set_heartbeat_sink(tx)`
4. AppState 빌드
5. consumer task 두 개 spawn (release_lock_for_cookie / refresh_lease_for_cookie)
6. axum::serve

ordering 이 뒤집히면 race — sink 등록 전에 첫 WS 가 들어오면 disconnect event 가 drop 됨 (현재로선 bind 전이라 안전). 명시 주석 필요 시 `bin/gtmux-cli/src/main.rs` §6c 직후 참조.

### 4.5 atomic-write-file 의 OpenOptionsExt 두 trait 임포트

```rust
use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;
use atomic_write_file::unix::OpenOptionsExt as AwfOpenOptionsExt;
```

`mode()` 는 std trait, `preserve_mode()` 는 awf trait. 둘 다 같은 builder 의 메서드처럼 보이지만 별 trait. 새 atomic-write 코드 작성 시 두 import 모두 필요.

### 4.6 fs2 contention 의 std::io::ErrorKind

`fs2::FileExt::try_lock_exclusive` 의 contention 은 `io::Error` 의 `kind() == ErrorKind::WouldBlock` (NOT EWOULDBLOCK errno). `session_lock.rs::acquire` 의 분기 참조.

### 4.7 `deny_unknown_fields` + `#[serde(flatten)]` 의 한계

flat 으로 펼친 `ItemCommon` 안쪽으로 unknown field 가 흘러들어와도 serde 가 silently drop. ADR-0018 G20 의 `maximized` field 제거는 이 silent-drop 으로 만족 (round-trip 안 함). 새 schema field 추가 시 동일 함정 주의.

---

## 5. 빌드 / 실행 / 테스트 명령

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend

# 빌드
cargo build --workspace
cargo build --release --bin gtmux

# 테스트 (현재 230 PASS / 0 FAIL)
cargo test --workspace --color=never

# Smoke gates — 모두 release binary 로 실행
# 1: workspace + session create + layout get          → §6.1
# 2: token mode → /auth?token=X → cookie + 303          → §6.2
# 3: password mode + set-password + /auth/login        → §6.2
# 5: cross-server attach lock + 409 + holder info       → §6.3
# 6: WS close → 자동 lock release                       → §6.3

# 외부 tmux 안에서는 실행 불가 — `env -u TMUX` 우회
env -u TMUX ./target/release/gtmux start \
  --session demo --port 9999 --workspace /tmp/ws
```

### Smoke-2/3 패턴 (외부 도구 없이 curl + python3 만 사용)

`docs/reports/0031` 본문 작성 시 inline 으로 사용한 패턴 — 새 smoke 작성 시 참고:

```bash
COOKIE_JAR=$(mktemp)
curl -sS -c "$COOKIE_JAR" -o /dev/null -H "Host: 127.0.0.1:$PORT" \
  "http://127.0.0.1:$PORT/auth?token=$TOKEN"

# Netscape cookie file: TAB 구분, field 6 = name, field 7 = value
COOKIE_VALUE=$(awk -F'\t' '/gtmux_auth/{print $7}' "$COOKIE_JAR" | head -1)
COOKIE_HEADER="gtmux_auth=$COOKIE_VALUE"

# 이후 curl 호출은 -b "$COOKIE_HEADER" 로 cookie 보냄
```

---

## 6. 통과한 smoke gate 상세

### 6.1 smoke-1 (Stage 1)
- `gtmux start --workspace /tmp/ws` → workspace dir + `.locks/` 0700 자동 생성
- `POST /api/sessions {name:"demo"}` → 201 + on-disk `demo.json` 0600
- `GET /api/sessions/demo/layout` → 200 + ETag + canonical empty v2 JSON

### 6.2 smoke-2/3 (Stage 2)
- token mode: `GET /auth?token=X` → 303 + opaque cookie (raw token 과 다름)
- cookie 만 보내고 `/api/sessions` → 200
- `POST /auth/logout` → cookie revoke + Max-Age=0
- password mode (config `[auth] mode = "password"`):
  - `gtmux set-password` stdin → `${XDG_STATE_HOME}/gtmux/password.argon2` 0600
  - `POST /auth/login {password}` → cookie 발행
  - 5 회 실패 후 6 번째 → 429 + Retry-After: 300

### 6.3 smoke-5/6 (Stage 3)
- smoke-5: 두 webpage 같은 session attach → 두 번째 409 + holder JSON `{server_id, pid, lease_until_unix}`; detach 후 즉시 두 번째 attach 가능
- smoke-6: cookie attach → `.lock` 생성 → 같은 cookie 로 WS upgrade → WS close → 1초 안에 lock auto-release + active=false

---

## 7. 다음 단계 — Stage 4 (Terminal pool + multi-attach mirror)

### 7.1 Stage 4 전체 작업 (handover.md §6 Stage 4 + plan-0007 §13)

| ID | 작업 | 의존 |
|----|------|------|
| **Stage 4.0** | **PaneId(u64) → UUID 마이그레이션** | 모든 후속의 prereq, **단독 batch 권장** |
| BE-NEW-10 | `GET /api/terminals` (server-wide alive + attach 점) | 4.0 |
| BE-NEW-3 | Match-or-spawn (ADR-0018 D6) | 4.0 + sessions/attach handler amend |
| BE-NEW-6 | Auto-mount trigger-aware (ADR-0021 D3) | 4.0 + WS dispatcher amend |
| BE-NEW-12.5 | Panel/Terminal close 분리 + respawn + Kill API (G25, ADR-0021 D9/D10) | 4.0 |
| BE-NEW-4 | WS frame `session_id` envelope (ADR-0021 D5) | 4.0 |
| BE-8 | Terminal metadata (label/created_at/attach_count) | 4.0 |

### 7.2 추천 진행 순서

1. **Batch 4-A: PaneId UUID 마이그레이션 (단독)**
   - `pty-backend::PaneId(pub u64)` → `PaneId(pub String)` 또는 별 wrapper
   - wire protocol 의 varint encoding 영향 검토 — `ws-server::varint` 가 PaneId 를 u64 로 가정. 두 옵션:
     - **Option A (전면 wire 변경)**: PaneId wire 형식을 length-prefixed string 으로. 모든 envelope 영향, FE 도 변경 필요.
     - **Option B (bridge map)**: PaneId 는 u64 그대로, http-api 에 `terminal_uuid_map: RwLock<BiMap<String, PaneId>>`. UUID 는 schema/HTTP 표면에서만, 내부 u64 는 ws-server/pty-backend 가 그대로. *비파괴적*.
   - **Option B 권장** (handover §10 "지금 task 가 요구하는 것만" + protocol 안정성).
   - 산출물: `crates/http-api/src/terminal_map.rs` (BiMap or two-map) + `AppState::spawn_terminal_with_uuid(uuid)` async method

2. **Batch 4-B: `GET /api/terminals` + BE-8 metadata**
   - `crates/http-api/src/terminals.rs` (신규)
   - `GET /api/terminals` → `[{ id: UUID, alive: bool, label, created_at, attach_count, attached_sessions: [...] }]`
   - 의존: 4-A 의 terminal_uuid_map

3. **Batch 4-C: Match-or-spawn 알고리즘 (ADR-0018 D6)**
   - `attach_handler` 가 layout 의 terminal items 와 server-pool alive PaneId 매칭
   - 새 endpoint `POST /api/sessions/:name/attach/confirm` (또는 `?confirm=true` query) — 미매칭 UUID 마다 spawn + map 등록
   - Response 의 `unmatched_count` 가 FE confirm dialog 트리거
   - smoke-4 gate 통과

4. **Batch 4-D: BE-NEW-12.5 Close 분리 + respawn**
   - `DELETE /api/sessions/:name/items/:id?kill_terminal=<bool>` — panel 만 제거 vs terminal 도 SIGTERM
   - `POST /api/terminals/:id/respawn` (same UUID, fresh PaneId)
   - `POST /api/terminals/:id/kill` (terminal SIGTERM, panel 유지 → dangling)
   - WS broadcast `terminal_died { terminal_id, reason }`

5. **Batch 4-E: BE-NEW-6 auto-mount trigger-aware + BE-NEW-4 WS session_id envelope**
   - dispatcher hook 의 cascade target 분기 (trigger_session vs others)
   - 새 WS envelope type byte for session-scoped frames
   - FE-NEW-6 (multi-xterm) 와 의존 정합

### 7.3 시작 명령 (다음 세션의 첫 동작)

```bash
cd /Users/ws/Desktop/projects/gtmux
# 1. 본 progress 문서 읽기
cat docs/reports/0031-stage-1-3-multi-session-be-progress.md

# 2. 전체 brief 재확인 (필요 시)
cat docs/agents/backend-handover.md

# 3. Stage 4 ADR 정독 — 이 셋이 4-A/B/C 를 가른다
cat docs/adr/0018-canvas-item-data-model.md   # §D2/§D6 match-or-spawn
cat docs/adr/0021-terminal-pool-and-mirror.md # D1/D7/D9/D10
cat docs/adr/0013-pty-direct-multiplexer.md   # PaneId 현 형태

# 4. 현 PaneId 사용 인벤토리
cd codebase/backend
grep -rn "PaneId" crates/ bin/ --include="*.rs" | wc -l
# (참고: 이전 audit 때 100+ 참조점, 가장 큰 surface 는 ws-server/src/lib.rs)

# 5. 빌드 + 테스트 — 230 PASS 확인
cargo test --workspace --color=never 2>&1 | grep "test result:"

# 6. Batch 4-A 시작 — terminal_map.rs 작성부터
```

### 7.4 Batch 4-A 의 핵심 결정 포인트 (다음 agent 가 잡아야 함)

1. **BiMap 라이브러리 선정** — `bimap = "0.6"` 추가? 또는 두 HashMap 수동 관리?
   - 권장: 두 HashMap 수동 — 단일 mutex 안에 묶여 있어 동기화 부담 미미
2. **UUID 생성 — `uuid = "1"` crate 추가 vs 자체 `fresh_server_id` 재사용**
   - `session_lock.rs::fresh_server_id` 가 이미 ring 기반 UUID v4 발행. 재사용 권장 (의존성 추가 0)
3. **first attach 의 spawn → 누가 호출하나**
   - 사용자가 `attach/confirm` 호출 → http-api 가 `spawn_terminal_with_uuid(uuid)` 호출 → 내부적으로 `hub.backend().spawn(spec)` + map 등록
   - vs FE 가 명시 `POST /api/terminals { id: uuid }` 호출
   - ADR-0018 D6 의 *"같은 id 로 fresh spawn"* 흐름을 따르면 첫 분기가 명료 (attach 흐름의 일부)

---

## 8. Gotchas / 컨벤션 회로

- **Korean docs, English code/log/commit**. 본 문서는 KO, 모든 신규 .rs 파일의 문서/주석은 EN.
- **ADR-before-code**. 새 BE 결정이 도출되면 ADR 추가 또는 amend 후 코드.
- **점진 rename** `Pane → Terminal` — 작업 영역에서 함께. 별 task X.
- **`forbid(unsafe_code)`** http-api crate. ws-server 는 `deny(unsafe_code)` (libc::raise SIGTERM inline allow).
- **test 추가는 같은 PR 안에서**. unit + 가능하면 통합 + (스코프 맞으면) smoke gate.
- **release binary 로 smoke**. dev 빌드는 cargo가 path traversal 등 detection 일부 다름.
- **smoke 시 외부 tmux 안 가드** — 항상 `env -u TMUX`.
- **CLAUDE.md 의 graph 사용 정책** — code-review-graph MCP 가 가능하면 Grep/Glob 보다 먼저. 다만 새 코드가 막 amend 된 상태라 graph 가 stale 일 수 있음 — 빌드/테스트가 권위.

---

## 9. 미해결 / 남은 빚

- 옛 `/api/layout` v1 endpoint + `LayoutStore` — Stage 4+ 에서 cleanup 검토 (현재 살아있음, 새 session record 와 양립).
- `LayoutSnapshot` (v1) 과 `SessionLayout` (v2) 의 두 메모리 표현 — 위 cleanup 시 통합.
- `gtmux start --session <name>` flag — 옛 single-session 시대 잔재. workspace+session 모델에서는 별 의미 없음. Stage 5+ 의 CLI 정리 단계에 함께.
- WS handshake 의 cookie-only 인증 (ADR-0020 D10) — 현재 subprotocol bearer 만. Stage 5+.
- Settings API (`GET/PATCH /api/settings`, `POST /api/settings/password`, `POST /api/settings/logout-all`) — Stage 7 BE-9.
- Rate limiter 의 X-Forwarded-For 신뢰 정책 — Cloud mode 에서 proxy 뒤일 때 (P1+).

---

## 10. 변경 이력

- 2026-05-15: 초안 — Stage 1~3 BE 완료 시점의 snapshot. 다음 session 의 Stage 4 진입 brief.
