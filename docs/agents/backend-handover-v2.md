# gtmux — Backend Agent Handover **v2**

> **v2 위치**: 2026-05-15 의 P0 (G18~G25) + P1 (G26~G29) grilling + D 검증 결과 모두 반영. v1 (`backend-handover.md`) 의 *후속*. v1 은 보존 — historical reference. **이 v2 만 읽으면 cold pickup OK**.
>
> 동반 문서: `docs/agents/frontend-handover-v2.md` (FE v2)

---

## 0. v1 → v2 변경 요약

v1 (2026-05-15 G18~G25 직후) 의 단순 후속 — *추가 grilling 4건 + D 검증 정합* 반영. **architectural 변경 없음**, plan-0007 의 BE 항목에 *3 신규* + *1 endpoint amend*:

| 변경 | 출처 | v2 영향 |
|---|---|---|
| BE-NEW-12.5 (Panel/Terminal close 분리 + respawn + Kill API + terminal_died broadcast) | G25, ADR-0021 D9/D10 amend | §5 신규 + §6 Stage 4 신규 + §8 의존성 column |
| BE-NEW-12 (file_path OS open + allowlist + audit log) | G21, ADR-0023 신규 | §5 신규 + §6 Stage 5 신규 |
| `POST /api/sessions/import` (BE-9 amend) | G28 | §5 BE-9 amend + §6 Stage 7 amend |
| Stage 5 정의 — file_path 가 Stage 5 (string-only, asset 비의존) 로 이동 | D 검증 | §6 Stage 5 표현 갱신 |
| Critical path Stage 4 의 BE-NEW-12.5 명시 | D 검증 | §6 의 Stage 4 sub-items |
| §15 cross-matrix BE-NEW-12 / BE-NEW-12.5 컬럼 추가 | D 검증 | §8 의존 매트릭스 |
| 추가 ADR: 0023 (file_path security), 0024 (Layer/Z 분리 — FE 중심) | G21, G24 | §1 reading list |
| FE 측 의존: `shortcutRegistry`, `themeStore`, `xtermTheme`, Settings export/import 의 BE endpoint 정합 | G26~G28 | §5 BE-9 + §6 Stage 7 |

v2 의 코드 진입은 v1 의 *그 Stage 1 진입점 그대로* (BE-NEW-1, BE-NEW-2, BE-NEW-11, BE-2, BE-3) — v2 의 변경은 *Stage 4+ 진입 전까지* code 영향 0.

---

## 1. Required reading (cold-pickup 순서)

| # | 파일 | 목적 |
|---|---|---|
| 1 | `/Users/ws/Desktop/projects/gtmux/CLAUDE.md` | 프로젝트 메타 (KO docs / EN code + ADR-before-code) |
| 2 | `/Users/ws/Desktop/projects/gtmux/CONTEXT.md` | 어휘 SoT + multi-session pivot + Z 정책 + Terminal lifecycle |
| 3 | `/Users/ws/Desktop/projects/gtmux/docs/plans/0007-multi-session-pivot.md` | 본 plan §0~§18 — G18~G29 모든 결정 반영된 단일 정본 |
| 4 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0019-session-and-workspace-model.md` | Workspace / Session / Webpage / single-attach / lock (D1~D11 + G18) |
| 5 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0018-canvas-item-data-model.md` | Schema v2 + match-or-spawn + G20/G24 amend |
| 6 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0020-auth-lifecycle.md` | Token + password + cookie + Argon2id + rate limit |
| 7 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0021-terminal-pool-and-mirror.md` | Server pool + mirror + heartbeat + close/dangling (D1~D10 + G25) |
| 8 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0023-file-path-open-security.md` | **NEW** — file_path OS open 보안 (G21) |
| 9 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0024-layer-tree-and-z-index-separation.md` | **NEW** (FE 중심) — Layer/Z 분리, group z 없음 (G24, BE 는 z field 영속만) |
| 10 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0013-pty-direct-multiplexer.md` | PTY 직접 관리, tokio::broadcast |
| 11 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0014-server-process-supervision.md` | Server lifecycle, exit codes, child SIGHUP |
| 12 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0006-persistence-storage.md` | Session file format + atomic write + ETag (D15 amend) |
| 13 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0010-group-data-model.md` | Group propagation (G24+G25 amend — z 없음, D12 Ungroup, D13 multi-session) |

선택:
- `docs/sketch.md` (KO 원본 design spec)
- `docs/ssot/canvas-layout-schema.md` — v1, v2 갱신 작업 예정 (BE-2)
- v1 brief (`docs/agents/backend-handover.md`) — historical

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
│   └── ...
├── WS connections (= Webpages, 각 0 또는 1 session attach)
└── Auth state (token + cookie + Argon2id password)
```

핵심 관계:
- **Server : Workspace = 1:1** (불변, boot 시 확정)
- **Workspace : Session = 1:N**
- **Webpage : Session = 1:1** (single-attach reciprocal, takeover 금지)
- **Terminal : Panel = 1:N** (multi-session mirror, 입력 공유)
- **PTY broadcast 는 server-scoped** — 모든 attach 점에 동일 stream

---

## 3. Architectural invariants

1. **PTY-direct** (ADR-0013) — no tmux. `Pane` → `Terminal` 어휘 통일 점진.
2. **Schema v2 hard cutover** (ADR-0006 D15, ADR-0018 D5) — boot 시 v1 발견 → groups[] 보존, panels[] 폐기, items[]=[].
3. **Server-wide Terminal broadcast** (ADR-0013 D11, ADR-0021 D2) — tokio::broadcast 1 channel = N attach 점.
4. **단일 attach reciprocal** (ADR-0019 D3) — 1 session : 1 webpage. Cross-server 는 `.locks/<name>.lock` flock (G18 amend).
5. **Auto-mount = trigger session 만** (ADR-0021 D3).
6. **Match-or-spawn** (ADR-0018 D6) — session attach 시 layout terminal id ↔ pool alive id 매칭, 없으면 same id 로 fresh spawn.
7. **Heartbeat 15s ping / 30s timeout** (ADR-0021 D6) — axum + tokio-tungstenite PING. Lease 갱신도 매 ping (G18).
8. **Auth lifecycle** (ADR-0020) — Token + password 둘 다 MVP. Argon2id (64 MiB/3/4). Cookie HttpOnly Secure SameSite=Strict 7d rolling. Rate 5/5min.
9. **보안** — path canonicalize + NUL block + symlink resolve. OS spawn argv direct (no shell). file_path open 은 ADR-0023 의 allowlist + audit log.
10. **Panel close ≠ Terminal kill** (G25, ADR-0021 D9 amend) — close 마다 dialog 3 옵션 ([Cancel] / [Panel only] / [Panel + Terminal]) 또는 `Settings.behavior.auto_kill_terminal_on_panel_close` 자동화. Default false.
11. **Dangling lazy spawn** (G25, ADR-0021 D10.1) — terminal SIGTERM → mirror panel `[exit]` overlay → focus interaction → same id 로 fresh spawn.
12. **점진 어휘 통일** — `Pane` → `Terminal` (작업 영역과 함께).

---

## 4. 현 코드 상태 (2026-05-15)

- HEAD commit `1e84f4c` (Sprint 7 closeout)
- `cargo test`: 164 PASS (Sprint 7 시점)
- 작업 트리: clean (이전 grilling 의 docs 만 변경)
- 서버 (실행 중): pid 36215, `127.0.0.1:9999`, single-session 시대 — Stage 1 진입 전 종료 필요

**현 BE 의 위치** (Sprint 7 = single-session 시대):
- ✅ PTY-direct (ADR-0013) / Server lifecycle (ADR-0014) / Persistence (ADR-0006 v1)
- ❌ multi-session 모델 (ADR-0019) — Stage 1~4
- ❌ Schema v2 (ADR-0018) — Stage 1
- ❌ Auth lifecycle (ADR-0020) — Stage 2
- ❌ Terminal pool 정합 (ADR-0021) — Stage 4 (broadcast 자체는 있음)
- ❌ Panel/Terminal close 분리 (ADR-0021 G25 amend) — Stage 4
- ❌ file_path open (ADR-0023) — Stage 5
- ❌ Session export/import (G28) — Stage 7

---

## 5. Backend 기능 명세 (plan-0007 §13)

### P0 항목 (Stage 1~4)

| ID | 이름 | Stage | ADR | 산출 위치 |
|---|---|---|---|---|
| BE-NEW-1 | WorkspaceManager (XDG path + session enumeration + lock) | 1 | 0019 D1/D2 | `crates/http-api/src/workspace/` (신규) |
| BE-NEW-2 | SessionRecord CRUD (file I/O, schema v2) | 1 | 0019 D5 | `crates/http-api/src/sessions/` (신규) |
| BE-NEW-11 | v1→v2 hard cutover migration | 1 | 0018 D5, 0006 D15 | 위 boot 흐름 |
| BE-2 | Schema v2 (items[] discriminated union) | 1 | 0018 D1 | `crates/http-api/src/storage/schema.rs` (큰 amend) |
| BE-3 | Schema validation v2 | 1 | 0018 D8 | 같은 storage crate |
| BE-1 | Auth handler (token + password) | 2 | 0020 D1/D5 | `crates/http-api/src/auth/` (신규) |
| BE-NEW-7 | Cookie lifecycle | 2 | 0020 D2 | auth/ 안 |
| BE-NEW-8 | Token + password mode 분기 | 2 | 0020 D4/D5 | auth/ 안 |
| BE-NEW-3 | Session attach + match-or-spawn + single-attach lock | 3 | 0018 D6, 0019 D3/D6 | `crates/http-api/src/sessions/attach.rs` |
| BE-NEW-4 | WS frame routing (session_id field) | 3 | 0021 D5 | `crates/ws-server/` 안 |
| BE-NEW-9 | Cross-server session lock (flock + lease, G18) | 3 | 0019 D6.1~D6.7 | `crates/http-api/src/session_lock.rs` |
| BE-6 | WS sync extension (session-scoped notify) | 3 | 0021 D5 | ws-server amend |
| BE-NEW-5 | Heartbeat (15s ping / 30s timeout) | 3 | 0021 D6 | ws-server amend |
| BE-NEW-6 | Auto-mount trigger-aware | 4 | 0021 D3 | ws-server amend |
| BE-NEW-10 | Terminal pool list API (GET /api/terminals) | 4 | 0021 D7 | `crates/http-api/src/terminals/list.rs` |
| BE-8 | Terminal metadata | 4 | 0021 | terminal struct amend |
| **BE-NEW-12.5** ⭐ | **Panel/Terminal close 분리 + respawn + Kill API + terminal_died broadcast (G25)** | **4** | **0021 D9/D10 amend** | `crates/http-api/src/sessions/items.rs` DELETE amend + `crates/http-api/src/terminals/{respawn,kill}.rs` |
| BE-7 | Conflict + lock (ETag PUT) | 3~7 | 0019 D6, 0006 | 분산 |

### P1 / P2 항목 (Stage 5~10)

| ID | 이름 | Stage | ADR |
|---|---|---|---|
| BE-5 | non-terminal payload validation (text/note/rect/ellipse/line/**file_path**) | 5 | 0018 |
| **BE-NEW-12** ⭐ | **file_path OS open + allowlist + audit log (G21)** | 5 | 0023 |
| BE-9 | Settings API (PATCH/POST + **POST /api/sessions/import** G28) | 7 | 0020 D5 |
| BE-4 | Asset storage (image/document, ADR-0022 후보) | 8 | TBD |
| BE-10 | Performance / safety | 9~10 | 0010 |

### BE-NEW-12.5 의 디테일 (G25, v1 → v2 신규)

ADR-0021 D9 amend 의 endpoint 묶음:
- `DELETE /api/sessions/<name>/items/<id>?kill_terminal=<bool>` — panel item 제거. `kill_terminal=true` 면 terminal SIGTERM 까지.
- `POST /api/terminals/<id>/respawn` — same id 로 새 child spawn (D10.1 c2). 기존 id 가 alive 면 400 reject (idempotency).
- `POST /api/terminals/<id>/kill` — terminal SIGTERM 만 (panel item 들 유지, 모두 dangling broadcast).
- WS broadcast frame: `{ kind: "terminal_died", terminal_id, reason: "exit"|"killed_by_panel_close"|"killed_explicit" }` — 모든 attached webpage 알림.

### BE-NEW-12 의 디테일 (G21, v1 → v2 신규)

ADR-0023 D5 의 endpoint:
- `GET /api/file-path/allowlist-check?path=<abs>` → `{ allowed: bool }`
- `POST /api/file-path/open { path }` — server-side allowlist 매칭 OR one-time nonce 검증 → `Command::new("open"|"xdg-open").arg(canonical).spawn()`. shell 비경유, NUL block, canonicalize, absolute path 강제. **Headless fallback** = 500 + `reason: "no_handler"`.
- `POST /api/file-path/allowlist { ext, prefix }` — entry 추가 (confirm modal 의 [Always for] 체크 시).
- `DELETE /api/file-path/allowlist/<id>` — Settings UI 의 삭제.
- Audit log NDJSON → `${XDG_STATE_HOME}/gtmux/audit/file-open-YYYYMMDD.log`.

### BE-9 amend (G28, v1 → v2 신규 endpoint)

기존 (token rotate / password / logout-all / settings PATCH) + **신규**:
- `POST /api/sessions/import { content: <SessionExport>, on_conflict: "rename"|"override"|"reject", new_name?: string }` — content 의 schema validate (gtmux_export_version, schema_version=2) → 이름 conflict 처리 → 새 session record 생성. Response: `{ created_name: string }`.

`SessionExport` 형식 (FE 가 생성):
```json
{
  "gtmux_export_version": "1",
  "exported_at": "ISO8601",
  "source": "gtmux@v0.X.X (commit abc)",
  "session_name": "demo-build",
  "schema_version": 2,
  "groups": [...],
  "items": [...],
  "viewport": {...}
}
```

---

## 6. Stage-by-stage 업무 할당 (BE 관점)

### Stage 1 — Foundation (BE leading)
**목표**: multi-session persistence + schema v2.

작업:
1. `crates/http-api/src/workspace/mod.rs` 신규 — `WorkspaceManager`
   - `new(path: PathBuf) -> Result<Self>` (XDG_DATA_HOME default, CLI override)
   - `enumerate_sessions(&self) -> Vec<SessionInfo>` (.lock 검사)
   - `boot_migration_v1_to_v2(&self)` (BE-NEW-11)
2. `crates/http-api/src/sessions/record.rs` 신규 — `SessionRecord` CRUD
   - file 형식: `<workspace>/<session-name>.json` (atomic rename write)
   - schema_version=2, groups[] + items[] + viewport
3. `crates/http-api/src/storage/schema.rs` 큰 amend
   - `Item` enum (serde discriminated union, `#[serde(tag = "type", rename_all = "snake_case")]`)
   - Variants: Terminal, Text, Note, Rect, Ellipse, Line, FreeDraw, Image, Document, FilePath
   - `ItemCommon { id, parent_id, x, y, w, h, z, visibility, locked, label, description, minimized }`
     - ⚠️ G20: `maximized` field **제거** (FE-only ephemeral)
   - Payload validation (각 variant)
4. `crates/http-api/src/sessions/handler.rs` 신규
   - `GET /api/sessions` (list)
   - `POST /api/sessions { name }` (정규식 `^[A-Za-z0-9_-]{1,64}$`)
   - `DELETE /api/sessions/<name>`
   - `GET /api/sessions/<name>/layout`
   - `PUT /api/sessions/<name>/layout` (ETag)
5. RGR — `cargo test` unit / integration

**Integration gate (smoke-1)**: `gtmux start` → workspace dir 자동 생성 (XDG_DATA_HOME) → `GET /api/sessions = []`. `POST /api/sessions { name: "demo" }` → 201 + `<workspace>/demo.json`. `GET /api/sessions/demo/layout` → empty schema v2.

### Stage 2 — Auth + Dialog + Session list (BE/FE parallel)
작업:
1. `crates/http-api/src/auth/mod.rs` 신규
   - `GET /auth`, `POST /auth/login { mode, value }`, `POST /auth/logout`, `POST /auth/rotate`
   - Argon2id verify, rate limiter (5/5min)
2. Cookie: `gtmux_session` HttpOnly Secure SameSite=Strict, 7d sliding
3. WS handshake amend — cookie 검증 + session_id query

**Integration gate (smoke-2)**: 첫 boot → 302 `/auth` → 로그인 → cookie → `/` → AuthDialog HTML serve.

### Stage 3 — Session attach + match-or-spawn + Heartbeat + Lock
작업:
1. `crates/http-api/src/sessions/attach.rs` 신규 — match-or-spawn (ADR-0018 D6)
   - Match: layout terminal id ↔ pool alive id
   - Mismatch (canvas ✓ / pool ✗) → same id 로 spawn
   - Mismatch (canvas ✗ / pool ✓) → attach 후보 list 반환 (사용자 명시 binding)
   - 양쪽 unmatched → response `{ confirm_required: true, summary }`
2. `crates/http-api/src/session_lock.rs` 신규 — flock + lease (G18)
   - `acquire/peek/release` (fs2::FileExt)
   - Lease 갱신 (heartbeat)
   - Shutdown hook 전체 release
3. `crates/ws-server/src/heartbeat.rs` — PING 15s, PONG timeout 30s, timeout 시 active=false + lock release
4. WS frame: `session_id` field 추가

**Integration gate**:
- smoke-3: 두 webpage 같은 session attach → 두 번째 modal disabled. 첫 close → ~30s 후 row enable.
- smoke-4: terminal 있는 session reload → match-or-spawn confirm dialog → terminal spawn → Canvas panel 표시.

### Stage 4 — Terminal pool + Multi-attach + **Close 분리** ⭐ (v2 신규)
**목표**: server-pool Terminal 관리 + multi-session mirror + **close UX 분리** (G25).

작업:
1. `GET /api/terminals` (BE-NEW-10) — server-wide alive terminals + attach 점 정보
2. `PUT /api/sessions/<name>/items/<id>/terminal { terminal_id }` (BE-4.2) — panel rebind
3. Auto-mount dispatcher hook 의 trigger session 분기 (BE-NEW-6, ADR-0021 D3)
4. **BE-NEW-12.5 (G25) ⭐ 신규**:
   - `DELETE /api/sessions/<name>/items/<id>?kill_terminal=<bool>` — panel 제거 ± terminal SIGTERM
   - `POST /api/terminals/<id>/respawn` — same id fresh spawn (idempotency check)
   - `POST /api/terminals/<id>/kill` — terminal SIGTERM only
   - WS `terminal_died` broadcast — 모든 attached webpage 알림

**Integration gate**:
- smoke-6: 한 탭 [New Terminal] → 다른 탭 Terminal list 갱신 (그 탭 layout 영향 X) → 다른 탭 [Attach to canvas] → 두 탭 같은 terminal, 다른 panel → input mirror.
- smoke-6b (G25 신규): 탭 A 의 [Panel + Terminal] → 탭 B mirror panel 에 [exit] overlay → 탭 B panel click → respawn → toast.

### Stage 5 — Canvas Item (text/note/rect/ellipse/line/file_path) — FE leading + BE 검증
**v2 변경**: file_path 가 Stage 5 (string-only, asset 비의존) 로 이동 — image/document 만 Stage 8 잔여.

작업:
1. Schema v2 의 non-terminal payload validation (text/note/rect/ellipse/line/**file_path**)
2. Payload size cap (label/description 4 KB, text 64 KB)
3. **BE-NEW-12 (G21, ADR-0023) ⭐ 신규**:
   - `POST /api/file-path/open` (argv spawn, allowlist match or one-time nonce)
   - `GET /api/file-path/allowlist-check`
   - `POST /api/file-path/allowlist`
   - `DELETE /api/file-path/allowlist/<id>`
   - Audit log NDJSON to `${XDG_STATE_HOME}/gtmux/audit/file-open-YYYYMMDD.log`

**Integration gate (smoke-7b, G21 신규)**: File Path 도구 click → Canvas click → path 입력 → file_path item 생성 → double-click → confirm modal → [✓ Always for *.md within /home/me/notes/] + [Open] → toast "Opened externally" + allowlist 영속. 다시 double-click → confirm 생략 + 즉시 open.

### Stage 6 — Layer list V2 + Panel header (FE leading)
BE 작업: smoke test 의 layout PATCH/PUT 검증. Group close 의 *bulk DELETE* 처리는 FE 가 다중 DELETE 호출 (또는 별 bulk endpoint TBD — plan-0007 §15 의존 mark 참조).

### Stage 7 — Viewport sync + Settings + **Session export/import** ⭐ (v2 amend)
BE 작업:
- BE-9 Settings API:
  - `GET /api/settings`, `PATCH /api/settings { field: value }` (auto-save 정합)
  - `POST /api/settings/password { current, new }` (Argon2 rehash)
  - `POST /api/settings/logout-all` (destructive)
  - Boot-immutable PATCH → 403
- **`POST /api/sessions/import` (G28) ⭐ 신규** — content schema validate + 이름 conflict 처리 → 새 session record 생성

### Stage 8 — Asset storage (P2, ADR-0022 후보)
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

# 실행 (외부 TMUX 안 가드)
unset TMUX
GTMUX_FRONTEND_DIST=../frontend/dist gtmux start --port 9999
```

⚠️ **함정**:
- 외부 tmux 안에서 `gtmux start` 불가 — `unset TMUX` 우회.
- Frontend dev 모드 사용 금지 — `npm run build` 후 `GTMUX_FRONTEND_DIST=...dist` 로 release binary 가 SPA 서빙.
- `forbid(unsafe_code)` → ws-server crate 만 `deny`. 다른 crate forbid 유지.

---

## 8. BE/FE 의존성 매트릭스 (plan-0007 §15 v2)

| FE 항목 | 필수 BE |
|---|---|
| FE-1 Auth page | BE-1, BE-NEW-7 |
| FE-NEW-1 Session UI | BE-1, BE-NEW-1, BE-NEW-2, BE-NEW-7 |
| FE-NEW-2 Attach lifecycle | BE-1, BE-NEW-3, BE-NEW-7 |
| FE-NEW-5 Attach confirm | BE-NEW-3 |
| FE-NEW-3 Terminal pool | BE-NEW-10, **BE-NEW-12.5** ⭐ |
| FE-NEW-6 Multi-xterm | BE-NEW-4, BE-NEW-10 |
| FE-6 Layer list V2 | BE-2, BE-NEW-2, BE-NEW-10, **BE-NEW-12.5** ⭐ (group close DELETE) |
| FE-7 Panel header V2 | BE-NEW-2, BE-NEW-10, **BE-NEW-12.5** ⭐ |
| FE-8 Settings UI | BE-1, BE-NEW-7, **BE-NEW-12** ⭐ (allowlist editor), **`POST /api/sessions/import`** ⭐ |
| FE-NEW-8 file_path open UX | BE-1, BE-2, BE-NEW-7, **BE-NEW-12** ⭐ |

**Critical path (v2)**: Stage 1 (BE-NEW-1/2/11 + BE-2/3) → Stage 2 (BE-1/NEW-7/8) → Stage 3 (BE-NEW-3/4/9 + BE-NEW-5 + BE-6) → Stage 4 (BE-NEW-6/10/8 + **BE-NEW-12.5** ⭐) → Stage 5 (BE-5 + **BE-NEW-12** ⭐) → Stage 7 (BE-9 + **import** ⭐).

---

## 9. Glossary

| 용어 | 의미 |
|---|---|
| **Terminal** | PTY pair + child process. server-pool 소속. 옛 `Pane` — 점진 rename. |
| **Workspace** | `${XDG_DATA_HOME}/gtmux/workspace/` — server 와 1:1 storage dir. |
| **Session record** | `<workspace>/<name>.json` — schema v2. |
| **Webpage** | 1 WS 연결. session attach 단위. |
| **Match-or-spawn** | Session attach 시 layout id ↔ pool id 매칭, 없으면 same id spawn. |
| **Auto-mount** | trigger session 의 layout 에만 cascade PUT 되는 dispatcher hook. |
| **Dangling Terminal Reference** | layout terminal id 가 pool 의 alive 와 매칭 안 됨. → fresh spawn (lazy on interaction, G25). |
| **Streaming State** | (session, panel) 쌍 단위. Suspended 시 broadcast subscriber drop. |
| **Lock (cross-server)** | `<workspace>/.locks/<name>.lock` flock + JSON (G18). |
| **terminal_died broadcast** | WS frame `{ kind, terminal_id, reason }` — 모든 attached webpage 알림 (G25). |

---

## 10. 작업 룰

- **English code/log/commit**, Korean docs.
- **ADR-before-code** — 본 brief / referenced ADR 외 새 결정 발생 시 ADR 먼저.
- **TDD 권장** — RGR. plan-0007 §17 의 integration gate.
- **점진 rename** — `Pane` → `Terminal` 어휘 (작업 영역과 함께).
- **불필요한 추가 금지** — backwards compat, 미사용 helper, *향후 가능성* 추상화 모두 거부.
- **Risky action 사전 확인** — destructive ops, force push, dep downgrade.

---

## 11. 진입 시 첫 메시지 후보

- "Stage 1 진입" → §6 의 Stage 1 작업 1~5.
- "Schema v2 enum 모양?" → plan-0007 §13.2 / §13.3 + ADR-0018 D1/D3/D8 의 Rust serde pattern.
- "BE-NEW-12.5 close API 부터" → Stage 4 의 BE-NEW-12.5 항목 + ADR-0021 D9/D10.
- "file_path security 부터" → Stage 5 의 BE-NEW-12 + ADR-0023.

---

## 12. 변경 이력

- 2026-05-15 v1: 초안 — G18~G25 + multi-session pivot 결과.
- 2026-05-15 v2: G26~G29 + D 검증 amend 반영. §5 BE 명세에 BE-NEW-12.5 (G25) + BE-NEW-12 (G21) + BE-9 의 `POST /api/sessions/import` (G28) 추가. §6 Stage 4 에 BE-NEW-12.5 sub-items + Stage 5 의 file_path 이동 + Stage 7 의 import endpoint. §8 의존 매트릭스 BE-NEW-12 / BE-NEW-12.5 컬럼. Reading list 에 ADR-0023 / 0024 / 0010 (G25 amend) 추가.
