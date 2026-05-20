# gtmux — Backend Agent Handover

> 본 문서는 *Backend 구현 agent* 의 cold-pickup brief. multi-session pivot 후 Stage 1~10 의 BE 작업을 cold 진입할 수 있도록 모든 context + reading list + 업무 할당을 정리. 산출물 디테일은 *참조 문서들*. 본 문서는 *진입 안내* + *Stage 별 할당*.
>
> 동반 문서: `docs/agents/frontend-handover.md` (FE 측 brief, BE/FE 의존성 정합)

---

## 0. 한 줄 정의

gtmux 는 **multi-session 가능한 PTY-direct 웹 터미널 워크스페이스** — 한 server / 한 workspace dir / N session record / 1:1 webpage attach / N:N terminal mirror. Backend = Rust (axum + tokio + tokio-tungstenite) + PTY-direct (no tmux).

---

## 1. Required reading (cold-pickup 순서)

| # | 파일 | 목적 | 분량 |
|---|---|---|---|
| 1 | `/Users/ws/Desktop/projects/gtmux/CLAUDE.md` | 프로젝트 메타 (KO docs / EN code 정책 + ADR-before-code rule) | 짧음 |
| 2 | `/Users/ws/Desktop/projects/gtmux/CONTEXT.md` | 어휘 SoT + multi-session pivot section + Z-index 정책 + Terminal lifecycle invariant | 중간 |
| 3 | `/Users/ws/Desktop/projects/gtmux/docs/plans/0007-multi-session-pivot.md` | 본 plan — Stage 0~10 + BE 기능 명세 §13 + cross-matrix §15 + 우선순위 §16 | 큼 (단일 정본) |
| 4 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0019-session-and-workspace-model.md` | Workspace / Session / Webpage / single-attach / lock | 큼 (D1~D11 + G18 amend) |
| 5 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0018-canvas-item-data-model.md` | Schema v2 (items[] discriminated union) + match-or-spawn algo | 중간 (D1~D8) |
| 6 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0020-auth-lifecycle.md` | Token + password + cookie + Argon2id + rate limit | 큼 (D1~D10) |
| 7 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0021-terminal-pool-and-mirror.md` | Server pool + multi-attach mirror + heartbeat + close/dangling | 큼 (D1~D10 + G25 amend) |
| 8 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0023-file-path-open-security.md` | file_path OS-level open (allowlist + argv spawn) — Stage 5 BE | 중간 (D1~D9) |
| 9 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0013-pty-direct-multiplexer.md` | PTY 직접 관리 (no tmux) — Terminal struct, tokio::broadcast 패턴 | 큼 |
| 10 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0014-server-process-supervision.md` | Server lifecycle (boot/shutdown, child SIGHUP, exit codes) | 중간 |
| 11 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0006-persistence-storage.md` | Session file format + atomic write (rename) + ETag — D15 amend (schema v2 hard cutover) | 중간 |
| 12 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0024-layer-tree-and-z-index-separation.md` | Z mutation 정책 (FE 중심, BE 는 schema z field 영속만) | 짧음 (참조용) |

선택 reading:
- ADR-0001~0017 — historical 결정 (single-session 시대 잔재 있음). 필요 시 참조.
- `docs/ssot/canvas-layout-schema.md` — v1 schema (v2 갱신 작업 예정 — BE-2)
- `docs/sketch.md` (KO 원본 design spec) — 큰 그림

---

## 2. Mental model

```
Server (process, 1 port, 1 workspace dir 바인딩)
├── Workspace (server 와 1:1, storage dir)
│   ├── Session record A (named JSON file)
│   │   └── Canvas Layout (groups + items + viewport)
│   │       └── items[]: { type: terminal|text|note|rect|ellipse|line|free_draw|image|document|file_path, ... }
│   ├── Session record B
│   └── ...
├── Terminal pool (server-wide, N:N attach)
│   ├── Terminal T1 (PTY pair + child process)
│   ├── Terminal T2
│   └── ...
├── WS connections (= Webpages, 각 0 또는 1 session attach)
└── Auth state (token + cookie + Argon2id password — ADR-0020)
```

핵심 관계:
- **Server : Workspace = 1:1** (불변, boot 시 확정)
- **Workspace : Session = 1:N**
- **Webpage : Session = 1:1** (single-attach reciprocal, takeover 금지)
- **Terminal : Panel = 1:N** (multi-session mirror, 입력 공유)
- **PTY broadcast 는 server-scoped** — 모든 attach 점에 동일 stream 전달, 어느 attach 에서 input 도 같은 shell

---

## 3. Architectural invariants (do not violate)

1. **PTY-direct** (ADR-0013) — tmux 없이 직접 PTY pair 관리. `Pane` 어휘 → `Terminal` 어휘 통일 진행 중 (점진).
2. **Schema v2 hard cutover** (ADR-0006 D15, ADR-0018 D5) — boot 시 v1 layout file 발견하면 groups[] 보존 + panels[] 폐기 + items[] = []. 마이그레이션 자동 + info log.
3. **Server-wide Terminal broadcast** (ADR-0013 D11, ADR-0021 D2) — tokio::broadcast 한 channel 이 모든 attach 점 (session × panel) 에 동일 stream 전달. PTY 의 자연 multi-attach 활용.
4. **단일 attach reciprocal** (ADR-0019 D3) — 한 session 은 한 시점 1 webpage 만 attach. Cross-server 도 `.locks/<name>.lock` 의 flock 으로 차단 (G18 amend).
5. **Auto-mount = trigger session 만** (ADR-0021 D3) — 한 webpage 의 [New Terminal] click 은 *그 session 의 layout* 에만 cascade mount, 다른 session 의 layout 무영향.
6. **Match-or-spawn** (ADR-0018 D6) — Session attach 시 layout 의 terminal item.id 와 server-pool 의 alive Terminal id 매칭. 매칭 없으면 *같은 id 로 fresh spawn*. unmatched 가 양쪽에 있으면 client confirm 요청.
7. **Heartbeat 15s ping / 30s timeout** (ADR-0021 D6) — axum + tokio-tungstenite 의 PING 활성화. timeout 시 그 session 의 active=false + lock release.
8. **Auth lifecycle** (ADR-0020) — Token + password 둘 다 MVP. Argon2id (memory 64 MiB / iter 3 / parallel 4). Cookie HttpOnly Secure SameSite=Strict 7d rolling. Rate limit 5/5min.
9. **보안** — 모든 path 는 canonicalize + NUL block + symlink resolve. Backend 의 OS spawn 은 *argv direct (no shell)*. file_path open 은 ADR-0023 의 allowlist + audit log.
10. **점진 어휘 통일** — code 의 `Pane` struct 등은 *작업 시* 같이 `Terminal` 로 rename. 별 task 로 분리 X.

---

## 4. 현 코드 상태 (2026-05-15)

- HEAD commit `1e84f4c` (Sprint 7 closeout)
- `cargo test`: 164 PASS (Sprint 7 시점, multi-session pivot 미반영)
- 작업 트리: clean (이전 grilling 의 docs 만 변경)
- 서버 (실행 중): pid 36215, `127.0.0.1:9999`, single-session 시대 — Stage 1 진입 전 종료 필요

**현 BE 의 위치** (Sprint 7 = single-session 시대):
- ✅ PTY-direct (ADR-0013) 완료
- ✅ Server lifecycle (ADR-0014) 완료
- ✅ Persistence (ADR-0006 — schema v1) 완료
- ❌ multi-session 모델 (ADR-0019) — Stage 1 ~ 4 진입 필요
- ❌ Schema v2 (ADR-0018) — Stage 1
- ❌ Auth lifecycle (ADR-0020) — Stage 2
- ❌ Terminal pool 정합 (ADR-0021) — Stage 4 (broadcast 자체는 이미 있음)
- ❌ file_path open (ADR-0023) — Stage 5

---

## 5. Backend 기능 명세 (plan-0007 §13 의 21 items)

각 항목의 디테일은 plan-0007 §13 참조. 본 절은 *목록 + 책임 영역*.

### P0 항목 (Stage 1~4, multi-session foundation)

| ID | 이름 | Stage | ADR | 산출 위치 (예상) |
|---|---|---|---|---|
| BE-NEW-1 | WorkspaceManager (XDG path + session enumeration + lock) | 1 | 0019 D1/D2 | `crates/http-api/src/workspace/` (신규) |
| BE-NEW-2 | SessionRecord CRUD (file I/O, schema v2) | 1 | 0019 D5 | `crates/http-api/src/sessions/` (신규) |
| BE-NEW-11 | v1→v2 hard cutover migration | 1 | 0018 D5, 0006 D15 | 위 boot 흐름 |
| BE-2 | Schema v2 (items[] discriminated union, serde Item enum) | 1 | 0018 D1 | `crates/http-api/src/storage/schema.rs` (큰 amend) |
| BE-3 | Schema validation v2 (id UUID, parent_id 무결성, payload cap) | 1 | 0018 D8 | 같은 storage crate |
| BE-1 | Auth handler (token + password) | 2 | 0020 D1/D5 | `crates/http-api/src/auth/` (신규) |
| BE-NEW-7 | Cookie lifecycle (Set-Cookie + 7d rolling renewal) | 2 | 0020 D2 | auth/ 안 |
| BE-NEW-8 | Token + password mode 분기 | 2 | 0020 D4/D5 | auth/ 안 |
| BE-NEW-3 | Session attach handler + match-or-spawn 알고리즘 + single-attach lock | 3 | 0018 D6, 0019 D3/D6 | `crates/http-api/src/sessions/attach.rs` |
| BE-NEW-4 | WS frame routing (session_id field 추가) | 3 | 0021 D5 | `crates/ws-server/` 안 |
| BE-NEW-9 | Cross-server session lock (flock + lease, G18) | 3 | 0019 D6.1~D6.7 | `crates/http-api/src/session_lock.rs` |
| BE-6 | WS sync extension (session-scoped notify) | 3 | 0021 D5 | ws-server amend |
| BE-NEW-5 | Heartbeat (15s ping / 30s timeout) | 3 | 0021 D6 | ws-server amend |
| BE-NEW-6 | Auto-mount trigger-aware (dispatcher hook 의 cascade target) | 4 | 0021 D3 | ws-server amend |
| BE-NEW-10 | Terminal pool list API (GET /api/terminals) | 4 | 0021 D7 | `crates/http-api/src/terminals/list.rs` |
| BE-8 | Terminal metadata (label / created_at / attach_count) | 4 | 0021 | terminal struct amend |
| **BE-NEW-12.5** | **Panel/Terminal close 분리 + respawn + Kill API + terminal_died broadcast (G25)** | **4** | **0021 D9/D10 amend** | `crates/http-api/src/terminals/{respawn,kill}.rs` + `crates/http-api/src/sessions/items.rs` DELETE amend |
| BE-7 | Conflict + lock (ETag PUT 의 lock 정합) | 3~7 | 0019 D6, 0006 | 분산 |

### P1 / P2 항목 (Stage 5~10)

| ID | 이름 | Stage | ADR |
|---|---|---|---|
| BE-5 | non-terminal payload validation | 5 | 0018 |
| BE-NEW-12 | file_path OS open + allowlist (G21) | 5 | 0023 |
| BE-9 | Settings API (PATCH/POST, G19) | 7 | 0020 D5 |
| BE-4 | Asset storage (image/document, ADR-0022 후보) | 8 | TBD |
| BE-10 | Performance / safety (payload cap, audit) | 9~10 | 0010 |

---

## 6. Stage-by-stage 업무 할당 (BE 관점)

### Stage 1 — Foundation (BE leading)
**목표**: multi-session 의 *workspace dir 기반* persistence + schema v2.

작업:
1. `crates/http-api/src/workspace/mod.rs` 신규 — `WorkspaceManager` struct
   - `new(path: PathBuf) -> Result<Self>` (XDG_DATA_HOME default, CLI override)
   - `enumerate_sessions(&self) -> Vec<SessionInfo>` (디렉터리 list, .lock 검사)
   - `boot_migration_v1_to_v2(&self)` (BE-NEW-11)
2. `crates/http-api/src/sessions/record.rs` 신규 — `SessionRecord` struct + CRUD
   - file 형식: `<workspace>/<session-name>.json` (atomic rename write, ADR-0006)
   - schema_version: 2, groups[] + items[] + viewport
3. `crates/http-api/src/storage/schema.rs` 큰 amend
   - `Item` enum (serde discriminated union, `#[serde(tag = "type", rename_all = "snake_case")]`)
   - Variants: Terminal, Text, Note, Rect, Ellipse, Line, FreeDraw, Image, Document, FilePath
   - 공통 field: `ItemCommon { id, parent_id, x, y, w, h, z, visibility, locked, label, description, minimized }`
     - ⚠️ G20 amend — `maximized` field **제거** (FE-only ephemeral)
   - Payload validation (각 variant 의 type-specific field)
4. `crates/http-api/src/sessions/handler.rs` 신규
   - `GET /api/sessions` (list)
   - `POST /api/sessions { name }` (create — 정규식 `^[A-Za-z0-9_-]{1,64}$`)
   - `DELETE /api/sessions/<name>`
   - `GET /api/sessions/<name>/layout`
   - `PUT /api/sessions/<name>/layout` (ETag — ADR-0006)
5. `cargo test` 의 *unit test* 부터 RGR — `WorkspaceManager`, `SessionRecord::load/save`, schema v2 round-trip

**산출물**:
- 새 crate 또는 module: `workspace/`, `sessions/`, `storage/schema.rs` 큰 amend
- Test: `cargo test workspace::tests::*`, `cargo test sessions::record::tests::*`
- Integration: `cargo test sessions::handler::tests::*` (axum test util)

**Integration gate**:
- smoke-1: `gtmux start` → workspace dir 자동 생성 (XDG_DATA_HOME) → `GET /api/sessions` = `[]`. `POST /api/sessions { name: "demo" }` → 201 + `<workspace>/demo.json` 생성. `GET /api/sessions/demo/layout` → empty schema v2.

### Stage 2 — Auth + Dialog + Session list (BE/FE parallel)
**목표**: 인증 lifecycle + Dialog 후 Session 선택 진입.

작업:
1. `crates/http-api/src/auth/mod.rs` 신규 — token/password 분기, Argon2id verify, rate limiter, cookie 발행
   - `GET /auth` (login page)
   - `POST /auth/login { mode: "token"|"password", value }`
   - `POST /auth/logout`
   - `POST /auth/rotate` (token only)
   - `POST /api/settings/password { current, new }` (Argon2 rehash)
2. Cookie: `gtmux_session` HttpOnly Secure SameSite=Strict, 7d, sliding expiration
3. Rate limit: 5 attempts / 5 min per IP — `tower::limit` 또는 자체
4. WS handshake amend: Sec-WebSocket-Protocol 의 cookie 검증 + session_id query param 검증

**Integration gate**:
- smoke-2: 첫 boot → `GET /` → 302 `/auth` → 로그인 → cookie 받음 → `GET /` → dialog HTML serve.

### Stage 3 — Session attach + match-or-spawn + Heartbeat + Lock
**목표**: 사용자가 *기존 session 연동* 또는 *새 session 만들고 attach* 했을 때 BE 흐름.

작업:
1. `crates/http-api/src/sessions/attach.rs` 신규
   - `POST /api/sessions/<name>/attach { ws_conn_id }` → match-or-spawn 알고리즘 (ADR-0018 D6)
   - Match: layout 의 terminal item.id ↔ server-pool alive Terminal id
   - Mismatch (canvas ✓ / pool ✗) → 같은 id 로 spawn (PtyConfig 기본값)
   - Mismatch (canvas ✗ / pool ✓) → 그 terminal 의 *attach 후보 list* 만 반환 (사용자 명시 binding 필요)
   - 양쪽 unmatched → response `{ confirm_required: true, summary: {...} }`
2. `crates/http-api/src/session_lock.rs` 신규 — flock + lease hybrid (G18, ADR-0019 D6.1~D6.7)
   - `acquire(session_name) -> Result<LockGuard>` (fs2::FileExt::try_lock_exclusive)
   - `peek(session_name) -> LockState` (peek 모드)
   - Lease renewal (heartbeat 마다 file 내용 갱신)
   - Shutdown hook: 모든 lock release + unlink
3. `crates/ws-server/src/heartbeat.rs` — axum/tungstenite 의 PING 15s, PONG timeout 30s, timeout 시 그 session 의 active=false + lock release
4. WS frame routing — frame 에 `session_id: SessionId` field 추가. 클라이언트가 어느 session 에 attach 했는지 명시

**Integration gate**:
- smoke-3: 두 webpage 열고 같은 session attach 시도 → 두 번째는 modal disabled 상태 (lock peek). 첫 webpage close → 30s 안 두 번째가 row enable.
- smoke-4: 한 탭에서 session1 만들고 terminal 추가 → 탭 close → reload → Dialog → 기존 session 연동 → match-or-spawn confirm dialog (terminal 1 개 spawn) → Canvas 에 panel 표시.

### Stage 4 — Terminal pool + Multi-attach mirror (BE/FE parallel)
**목표**: server-pool 의 Terminal 관리 + multi-session mirror + close UX.

작업:
1. `GET /api/terminals` — server-wide alive terminals, 각 terminal 의 attach 점 (sessions × panels)
2. `PUT /api/sessions/<name>/items/<id>/terminal { terminal_id }` — panel 의 terminal binding 변경
3. Auto-mount dispatcher hook 의 *cascade target 분기* (ADR-0021 D3) — 한 webpage 의 `POST /api/terminals` 응답을 그 webpage 의 session layout 에만 cascade PUT
4. **BE-NEW-12.5 (G25, ADR-0021 D9/D10 amend)**:
   - `DELETE /api/sessions/<name>/items/<id>?kill_terminal=<bool>` — panel item 제거. `kill_terminal=true` 면 terminal SIGTERM
   - `POST /api/terminals/<id>/respawn` — same id 로 새 child spawn (ADR-0021 D10.1 c2). 기존 id 가 alive 면 400 reject (idempotency)
   - `POST /api/terminals/<id>/kill` — terminal SIGTERM 만 (panel 들 유지, dangling broadcast)
   - WS broadcast: `{ kind: "terminal_died", terminal_id, reason }` — 모든 attached webpage 에 알림

**Integration gate**:
- smoke-6: 한 탭에서 [New Terminal] → 다른 탭의 Terminal list 갱신 (그 탭 layout 에는 mount 안 됨) → 다른 탭의 사용자가 그 terminal 을 [Attach to canvas] → 두 탭이 같은 terminal 의 다른 panel 들. 한 쪽 input → 두 쪽 모두 동일 출력.
- smoke-6b: 탭 A 에서 [Panel + Terminal] 액션 → 탭 B 의 mirror panel 에 [exit] overlay → 탭 B 의 panel click → respawn → toast.

### Stage 5 — Canvas Item (text/note/rect/ellipse/line/file_path) — FE leading + BE 검증
**BE 작업**:
1. Schema v2 validation 의 *non-terminal payload* (text/note/rect/ellipse/line/file_path)
2. Payload size cap (label/description 4 KB, text 64 KB)
3. **BE-NEW-12 (G21, ADR-0023)** — file_path OS-level open:
   - `POST /api/file-path/open { path }` — server-side allowlist 매칭 OR one-time nonce 검증 → argv spawn (no shell, canonicalize, NUL block)
   - `GET /api/file-path/allowlist-check?path=<abs>`
   - `POST /api/file-path/allowlist { ext, prefix }`
   - `DELETE /api/file-path/allowlist/<id>`
   - Audit log NDJSON to `${XDG_STATE_HOME}/gtmux/audit/file-open-YYYYMMDD.log`

### Stage 6 — Layer list V2 + Panel header (FE leading, BE 의존 적음)
BE 작업: smoke test 의 layout PATCH/PUT 검증 정도. 새 endpoint 없음. (Group close 의 bulk dialog 는 FE 의 `DELETE` 다중 호출 또는 *별 bulk endpoint* 신규 — TBD, plan-0007 §15 의 의존 mark 확인)

### Stage 7 — Viewport sync + Settings page (BE/FE parallel)
BE: BE-9 (Settings API, G19) — PATCH /api/settings + POST /api/settings/password + POST /api/settings/logout-all

### Stage 8 — Asset storage (P2, 별 ADR-0022 후보)
BE: image/document asset CRUD + storage path + content hash + GC

### Stage 9 — Free draw + drawing perf
BE: free_draw point cap + simplification

### Stage 10 — Hardening
BE: audit logs, payload cap, security review

---

## 7. Build / test / run

```bash
# 빌드
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo build --release

# 테스트
cargo test --workspace

# 실행 (외부 TMUX 안 가드 — ADR-0014 D10)
unset TMUX
GTMUX_FRONTEND_DIST=../frontend/dist gtmux start --port 9999
```

⚠️ **build / run 함정**:
- 외부 tmux 안에서 `gtmux start` 불가 — `unset TMUX` 우회.
- Frontend dev 모드 사용 금지 — `npm run build` 후 `GTMUX_FRONTEND_DIST=...dist` 로 release binary 가 SPA 서빙.
- `forbid(unsafe_code)` → ws-server crate 만 `deny` (libc::raise SIGTERM inline allow). 다른 crate 는 forbid 유지.

---

## 8. BE/FE 의존성 매트릭스 요약 (plan-0007 §15)

| FE 항목 | 필수 BE |
|---|---|
| FE-1 Auth page | BE-1, BE-NEW-7 |
| FE-NEW-1 Session UI | BE-1, BE-NEW-1, BE-NEW-2, BE-NEW-7 |
| FE-NEW-2 Attach lifecycle | BE-1, BE-NEW-3, BE-NEW-7 |
| FE-NEW-5 Attach confirm | BE-NEW-3 |
| FE-NEW-3 Terminal pool | BE-NEW-10, **BE-NEW-12.5** |
| FE-NEW-6 Multi-xterm | BE-NEW-4, BE-NEW-10 |
| FE-6 Layer list V2 | BE-2, BE-NEW-2, BE-NEW-10, **BE-NEW-12.5** |
| FE-7 Panel header V2 | BE-NEW-2, BE-NEW-10, **BE-NEW-12.5** |
| FE-8 Settings UI | BE-1, BE-NEW-7, BE-NEW-12 |
| FE-NEW-8 file_path open UX | BE-1, BE-2, BE-NEW-7, BE-NEW-12 |

**Critical path**: Stage 1 (BE-NEW-1/2/11 + BE-2/3) → Stage 2 (BE-1/NEW-7/8) → Stage 3 (BE-NEW-3/4/9 + BE-NEW-5 + BE-6) → Stage 4 (BE-NEW-6/10/8 + **BE-NEW-12.5**) → Stage 6 (FE 진행).

---

## 9. Glossary (BE 어휘)

| 용어 | 의미 |
|---|---|
| **Terminal** | PTY pair + child process. server-pool 소속. ADR-0013. 옛 어휘 `Pane` — 점진 rename. |
| **Workspace** | `${XDG_DATA_HOME}/gtmux/workspace/` (또는 config 명시) — server 와 1:1 storage dir. ADR-0019 D1/D2. |
| **Session record** | `<workspace>/<name>.json` — schema v2 (groups + items + viewport). ADR-0018 D1. |
| **Webpage** | 1 WS 연결. session attach 의 가축. ADR-0019 D3. |
| **Match-or-spawn** | Session attach 시 layout 의 terminal id 와 server-pool alive id 매칭, 없으면 같은 id 로 fresh spawn. ADR-0018 D6. |
| **Auto-mount** | 사용자 [New Terminal] 응답이 *trigger session 의 layout* 에만 cascade PUT 되는 dispatcher hook 패턴. ADR-0021 D3. |
| **Dangling Terminal Reference** | session layout 의 terminal item.id 가 server-pool 의 어떤 alive Terminal 과도 매칭 안 됨. 옛 어휘 *Stale Panel Reference*. CONTEXT.md. |
| **Streaming State** | (session, panel) 쌍 단위 — `Streaming` / `Suspended`. Suspended 시 broadcast subscriber drop. CONTEXT.md. |
| **Lock (cross-server)** | `<workspace>/.locks/<name>.lock` flock + JSON 내용. ADR-0019 D6.1~D6.7. |

---

## 10. 작업 룰

- **English code, English log, English commit messages**. Korean docs.
- **ADR-before-code** — 본 brief 와 referenced ADR 외 새 결정 발생 시 *ADR 작성 후 코드*. PR 에 ADR reference 의무.
- **TDD 권장** — RGR. plan-0007 §17 의 integration gate 가 smoke test 의 sink.
- **점진 rename** — `Pane` → `Terminal` 어휘는 *작업 영역에서 함께*. 별 task 로 분리 X.
- **불필요한 추가 금지** — backwards compat shim, 미사용 helper, *향후 가능성* 의 추상화 모두 거부. *지금 task 가 요구하는 것만*.
- **Risky action 은 확인** — destructive operations, force push, dep downgrade 모두 사전 confirm.

---

## 11. 진입 시 첫 메시지 후보

다음 작업이 무엇인지 명확히 알려진 경우 (예: "Stage 1 진입"), 본 brief 읽고 곧장 코드 작업. 모호한 경우 다음 질문:

- "Stage 1 의 어디부터?" → 본 brief §6 의 Stage 1 작업 1~5 순서.
- "Test 부터?" → RGR 권장 (TDD skill). unit test 작성 후 implementation.
- "Schema v2 의 enum 모양?" → plan-0007 §13.2 / §13.3 + ADR-0018 D1 / D3 / D8 의 Rust serde pattern 참조.
- "현 BE 코드의 어디를 손대야?" → `Pane` struct, `panels[]` 등 v1 schema 의존 코드. v2 hard cutover 후 *cleanup* 필요.

---

## 12. 변경 이력

- 2026-05-15: 초안 — multi-session pivot 후 BE agent 진입 brief. plan-0007 §13 + ADR 4 신규 (0018/0019/0020/0021) + ADR 2 추가 (0023 file_path security, 0024 Layer/Z) + G18~G25 grilling 결과 정합.
