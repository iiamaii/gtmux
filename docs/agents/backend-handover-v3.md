# gtmux — Backend Agent Handover **v3**

> **v3 위치**: 2026-05-15 의 모든 grilling (G18~G29 + G32~G40 + Tier 3 + Tier 2 P2 deferred) + D 검증 반영. v1/v2 는 historical. **이 v3 만 읽으면 cold pickup OK**.
>
> 동반 문서: `docs/agents/frontend-handover-v3.md` (FE v3)

---

## 0. v1 → v2 → v3 변경 요약

| 버전 | 추가 결정 | 시점 |
|---|---|---|
| v1 | G18~G25 + multi-session pivot 결과 | 2026-05-15 P0 grilling |
| v2 | G26~G29 + D 검증 amend | 2026-05-15 P1 grilling |
| **v3** | **G32~G40 + Tier 3 + Tier 2 P2 deferred** | **2026-05-15 초기 기획 누락 점검** |

v3 의 BE 신규 ⭐ 항목 (v2 대비):

| 변경 | 출처 | v3 영향 |
|---|---|---|
| **POST /api/terminals { template_id }** + Template CRUD endpoints | G36 | §5 BE-NEW-10 amend + §6 Stage 4 amend |
| **POST /api/shutdown** (Tier 3) | sketch §11.2.A 정합 | §5 BE-9 amend |
| ADR-0018 D4 `terminal_overrides` schema field | G35 | §5 BE-2/BE-3 amend |
| Tier 2 P2 deferred — Stage 8+ 진입 전 grilling anchor 명시 | plan-0007 §10.2 | §5 P2 entries |

v3 의 코드 진입은 v2 의 *그 Stage 1 진입점 그대로* — v3 변경은 *Stage 4 (template) / Stage 5 (terminal_overrides field) / Stage 7 (shutdown / templates settings)* 까지 적용.

---

## 1. Required reading (cold-pickup 순서)

| # | 파일 | 목적 |
|---|---|---|
| 1 | `/Users/ws/Desktop/projects/gtmux/CLAUDE.md` | 프로젝트 메타 |
| 2 | `/Users/ws/Desktop/projects/gtmux/CONTEXT.md` | 어휘 SoT + multi-session pivot + Z 정책 + Terminal lifecycle + G25 close 정책 |
| 3 | `/Users/ws/Desktop/projects/gtmux/docs/plans/0007-multi-session-pivot.md` | 본 plan §0~§18 — G18~G40 + Tier 2/3 모두 반영 단일 정본 |
| 4 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0019-session-and-workspace-model.md` | Workspace / Session / Webpage / single-attach / lock (D1~D11 + G18) |
| 5 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0018-canvas-item-data-model.md` | Schema v2 + match-or-spawn + G20/G24/G35 amend (`terminal_overrides` 신규) |
| 6 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0020-auth-lifecycle.md` | Token + password + cookie + Argon2id + rate limit |
| 7 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0021-terminal-pool-and-mirror.md` | Server pool + mirror + heartbeat + close/dangling (D1~D10 + G25) |
| 8 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0023-file-path-open-security.md` | file_path OS open 보안 (G21) |
| 9 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0024-layer-tree-and-z-index-separation.md` | Layer/Z 분리 (G24) — FE 중심, BE 는 z field 영속만 |
| 10 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0013-pty-direct-multiplexer.md` | PTY 직접, tokio::broadcast |
| 11 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0014-server-process-supervision.md` | Server lifecycle, exit codes, child SIGHUP, **shutdown 흐름 (Tier 3 정합)** |
| 12 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0006-persistence-storage.md` | Session file format + atomic write + ETag (D15 amend) |
| 13 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0010-group-data-model.md` | Group propagation + G24/G25 amend |

선택:
- `docs/sketch.md` — KO 원본 design spec, 추가 컨텍스트
- v1/v2 brief — historical

---

## 2. Mental model

```
Server (process, 1 port, 1 workspace dir 바인딩)
├── Workspace (server 와 1:1, storage dir)
│   ├── Session record A (named JSON file)
│   │   └── Canvas Layout (groups + items + viewport)
│   │       └── items[]: { type: terminal|text|note|rect|ellipse|line|free_draw|image|document|file_path, ... }
│   │           └── terminal item: optional terminal_overrides (G35)
│   ├── Session record B
│   └── ...
├── Terminal pool (server-wide, N:N attach)
│   ├── Terminal T1 (PTY pair + child process, spawned from template)
│   └── ...
├── WS connections (= Webpages, 각 0 또는 1 session attach)
├── Auth state (token + cookie + Argon2id)
└── Config files:
    - ${XDG_CONFIG_HOME}/gtmux/config.toml
    - ${XDG_CONFIG_HOME}/gtmux/terminal-templates.toml (G36)
    - ${XDG_CONFIG_HOME}/gtmux/file-open-allowlist.toml (G21)
```

핵심 관계:
- **Server : Workspace = 1:1** (불변)
- **Workspace : Session = 1:N**
- **Webpage : Session = 1:1** (single-attach reciprocal)
- **Terminal : Panel = 1:N** (mirror)
- **PTY broadcast = server-scoped** (tokio::broadcast)
- **Template : Terminal spawn = 1:N** (one template → many spawns, G36)

---

## 3. Architectural invariants

1. **PTY-direct** (ADR-0013).
2. **Schema v2 hard cutover** (ADR-0006 D15, ADR-0018 D5).
3. **Server-wide Terminal broadcast** (ADR-0013 D11, ADR-0021 D2).
4. **단일 attach reciprocal** (ADR-0019 D3, `.locks/<name>.lock` flock G18).
5. **Auto-mount = trigger session 만** (ADR-0021 D3).
6. **Match-or-spawn** (ADR-0018 D6).
7. **Heartbeat 15s ping / 30s timeout** (ADR-0021 D6).
8. **Auth lifecycle** (ADR-0020).
9. **보안** — canonicalize / NUL block / argv direct / ADR-0023 allowlist.
10. **Panel close ≠ Terminal kill** (G25, ADR-0021 D9 amend).
11. **Dangling lazy spawn** (G25, ADR-0021 D10.1 c2).
12. **Template spawn** (G36) — `POST /api/terminals { template_id }` 의 server-side template resolution.
13. **Graceful shutdown** (Tier 3 + ADR-0014 D7) — `POST /api/shutdown` triggers WS close + child SIGHUP + flush + lock 정리 → exit 6.

---

## 4. 현 코드 상태 (2026-05-15)

- HEAD `1e84f4c` (Sprint 7 closeout)
- `cargo test`: 164 PASS
- 작업 트리: clean (이전 grilling 의 docs 만 변경)
- 서버 (실행 중): pid 36215, `127.0.0.1:9999`, Stage 1 진입 전 종료 필요

**현 BE 위치**:
- ✅ PTY-direct / Server lifecycle / Persistence v1
- ❌ multi-session 모델 (ADR-0019) — Stage 1~4
- ❌ Schema v2 + **terminal_overrides** field (G35) — Stage 1 / 5
- ❌ Auth lifecycle — Stage 2
- ❌ Terminal pool 정합 + **close 분리 (G25)** — Stage 4
- ❌ **Template spawn (G36)** — Stage 4
- ❌ file_path open (G21) — Stage 5
- ❌ Session export/import (G28) — Stage 7
- ❌ **Shutdown endpoint (Tier 3)** — Stage 7

---

## 5. Backend 기능 명세

### P0 (Stage 1~4)

| ID | 이름 | Stage | ADR | 산출 |
|---|---|---|---|---|
| BE-NEW-1 | WorkspaceManager (XDG + enumeration + lock) | 1 | 0019 D1/D2 | `crates/http-api/src/workspace/` |
| BE-NEW-2 | SessionRecord CRUD | 1 | 0019 D5 | `crates/http-api/src/sessions/` |
| BE-NEW-11 | v1→v2 migration | 1 | 0018 D5 | boot 흐름 |
| BE-2 | Schema v2 + **terminal_overrides** (G35) | 1 (frame) / 5 (validation) | 0018 D1/D4 amend | `storage/schema.rs` |
| BE-3 | Schema validation v2 | 1 | 0018 D8 | 같은 |
| BE-1 | Auth handler | 2 | 0020 | `auth/` |
| BE-NEW-7 | Cookie lifecycle | 2 | 0020 D2 | auth/ |
| BE-NEW-8 | Token + password 분기 | 2 | 0020 D4/D5 | auth/ |
| BE-NEW-3 | Session attach + match-or-spawn + lock | 3 | 0018 D6, 0019 D3/D6 | `sessions/attach.rs` |
| BE-NEW-4 | WS frame routing | 3 | 0021 D5 | ws-server |
| BE-NEW-9 | Cross-server lock (flock + lease, G18) | 3 | 0019 D6.1~D6.7 | `session_lock.rs` |
| BE-6 | WS sync extension | 3 | 0021 D5 | ws-server |
| BE-NEW-5 | Heartbeat 15s/30s | 3 | 0021 D6 | ws-server |
| BE-NEW-6 | Auto-mount trigger-aware | 4 | 0021 D3 | ws-server |
| BE-NEW-10 | Terminal pool list + multi-attach + **template spawn** (G36) | 4 | 0021 D7 + G36 | `terminals/list.rs` + `terminals/templates.rs` ⭐ |
| BE-8 | Terminal metadata | 4 | 0021 | terminal struct |
| BE-NEW-12.5 | Panel/Terminal close 분리 + respawn + Kill + terminal_died (G25) | 4 | 0021 D9/D10 | `terminals/{respawn,kill}.rs` + `sessions/items.rs` DELETE |
| BE-7 | Conflict + lock (ETag) | 3~7 | 0019 D6, 0006 | 분산 |

### P1 (Stage 5~7)

| ID | 이름 | Stage | ADR |
|---|---|---|---|
| BE-5 | non-terminal payload validation (text/note/rect/ellipse/line/file_path) | 5 | 0018 |
| BE-NEW-12 | file_path open + allowlist + audit (G21) | 5 | 0023 |
| BE-9 | Settings API + **POST /api/sessions/import** (G28) + **POST /api/shutdown** ⭐ (Tier 3) | 7 | 0020 D5 + sketch §11.2.A |
| BE-NEW-10 amend | **Terminal template CRUD** (G36) | 7 (UI 시점) / 4 (spawn) | G36 |

### P2 (Stage 8~10, deferred — plan-0007 §10.2)

| ID | 이름 | 비고 |
|---|---|---|
| BE-4 | Asset storage (image/document) | ADR-0022 후보, Stage 8 |
| BE-10 | Performance / safety | 지속 |

### v3 신규 BE endpoint 요약 ⭐

**Stage 4 (G36 template)**:
- `POST /api/terminals { template_id?, command?, cwd?, env?, fresh_spawn?, id? }` — template_id 또는 raw spawn, fresh_spawn=true 시 same id
- `GET /api/terminal-templates` — list (server-side `terminal-templates.toml` 조회)
- `POST /api/terminal-templates` — 신규 entry
- `PUT /api/terminal-templates/<name>` — 변경
- `DELETE /api/terminal-templates/<name>` — 삭제

**Stage 7 (Tier 3 shutdown)**:
- `POST /api/shutdown` — ADR-0014 D7 graceful teardown trigger:
  1. 모든 WS connection 에 `server_shutdown` notify broadcast
  2. WS connection 모두 close
  3. 모든 child process SIGHUP
  4. 모든 session record sync flush (atomic write 완료 보장)
  5. 모든 lock file 정리 (`.locks/*.lock` unlink)
  6. exit code 6 (graceful)

### v3 신규 BE field ⭐

ADR-0018 D4 의 `terminal` payload (G35):
```rust
#[derive(Deserialize, Serialize, Default)]
struct TerminalOverrides {
    font_size: Option<u32>,
    wrap: Option<bool>,
    scrollback: Option<u32>,
    cursor_style: Option<CursorStyle>, // "block"|"underline"|"bar"
    cursor_blink: Option<bool>,
    bell: Option<BellStyle>,           // "none"|"sound"|"visual"
}

enum Item {
    Terminal { 
        #[serde(flatten)] common: ItemCommon,
        #[serde(default, skip_serializing_if = "TerminalOverrides::is_empty")]
        terminal_overrides: TerminalOverrides,
    },
    // ...
}
```

---

## 6. Stage-by-stage 업무 할당

### Stage 1 — Foundation (v2 와 동일)
- WorkspaceManager + SessionRecord + schema v2 (terminal_overrides 포함) + 5 endpoint (GET/POST/DELETE sessions, GET/PUT layout)
- **Smoke-1**: `gtmux start` → workspace dir 자동 생성 → empty sessions → POST + GET 정상.

### Stage 2 — Auth (v2 와 동일)
- Auth handler + cookie + Argon2id + rate limit + WS handshake cookie 검증.
- Smoke-2: 로그인 → cookie → Dialog.

### Stage 3 — Attach + Heartbeat + Lock (v2 와 동일)
- match-or-spawn + flock + heartbeat + WS frame `session_id`.
- Smoke-3/4: 충돌 disabled / reload + match dialog.

### Stage 4 — Terminal pool + Multi-attach + **Close 분리 + Template** (v3 amend) ⭐
**v2 의 작업 + G36 template 추가**:
1. `GET /api/terminals` (BE-NEW-10)
2. `PUT /api/sessions/<name>/items/<id>/terminal` (BE-4.2 rebind)
3. Auto-mount trigger-aware (BE-NEW-6)
4. **BE-NEW-12.5 (G25)**:
   - `DELETE /api/sessions/<name>/items/<id>?kill_terminal=<bool>`
   - `POST /api/terminals/<id>/respawn`
   - `POST /api/terminals/<id>/kill`
   - WS `terminal_died` broadcast
5. **BE-NEW-10 amend (G36) ⭐**:
   - `POST /api/terminals { template_id?, command?, cwd?, env?, fresh_spawn?, id? }` — template resolution + spawn
   - Templates CRUD (`GET/POST /api/terminal-templates`, `PUT/DELETE /api/terminal-templates/<name>`)
   - Default 4 preset boot 시 자동 생성 (`bash` / `zsh` / `python` / `htop`) — `terminal-templates.toml` 미존재 시
- Smoke-6/6b 유지 + 신규 smoke-6c: template list → POST { template_id: "python" } → python REPL terminal spawn.

### Stage 5 — Canvas Item (text/note/rect/ellipse/line/file_path) (v2 와 동일)
- Schema v2 의 non-terminal validation (terminal_overrides field 도 here validate)
- BE-NEW-12 (G21) file_path open + allowlist + audit
- Smoke-7b 유지.

### Stage 6 — Layer list V2 + Panel header (FE leading, v2 와 동일)

### Stage 7 — Viewport sync + Settings + **Shutdown** ⭐ (v3 amend)
**v2 의 작업 + Tier 3 shutdown 추가**:
1. Viewport sync (`PUT layout` 의 viewport field)
2. BE-9 Settings API:
   - GET/PATCH /api/settings
   - POST /api/settings/password
   - POST /api/settings/logout-all
   - **POST /api/sessions/import (G28)**
   - **POST /api/shutdown (Tier 3) ⭐** — ADR-0014 D7 graceful teardown
   - Boot-immutable PATCH → 403
3. Terminal templates CRUD (Settings UI 가 이걸 호출)

### Stage 8~10 (P2)
- BE-4 Asset storage (ADR-0022 후보)
- BE-10 Performance/safety

---

## 7. Build / test / run

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo build --release

cargo test --workspace

unset TMUX  # 외부 tmux 안 가드
GTMUX_FRONTEND_DIST=../frontend/dist gtmux start --port 9999
```

함정:
- 외부 tmux 안 `gtmux start` 불가 (ADR-0014 D10).
- Frontend dev 모드 사용 금지.
- `forbid(unsafe_code)` → ws-server crate 만 `deny`.

---

## 8. BE/FE 의존성 매트릭스 (plan-0007 §15 v3)

| FE 항목 | 필수 BE |
|---|---|
| FE-1 Auth | BE-1, BE-NEW-7 |
| FE-NEW-1 Session UI | BE-1, BE-NEW-1, BE-NEW-2, BE-NEW-7 |
| FE-NEW-2 Attach | BE-1, BE-NEW-3, BE-NEW-7 |
| FE-NEW-5 Match confirm | BE-NEW-3 |
| FE-NEW-3 Terminal pool | BE-NEW-10, **BE-NEW-12.5** |
| FE-NEW-6 Multi-xterm | BE-NEW-4, BE-NEW-10 |
| **FE-2 Toolbar [Terminal▾] dropdown** ⭐ | **BE-NEW-10 amend (G36 template)** ⭐ |
| FE-6 Layer list V2 | BE-2, BE-NEW-2, BE-NEW-10, BE-NEW-12.5 |
| FE-7 Panel header V2 | BE-NEW-2, BE-NEW-10, BE-NEW-12.5 |
| **FE-7 Panel Settings modal Terminal Override** ⭐ | **BE-2 amend (G35 terminal_overrides)** ⭐ |
| FE-8 Settings UI | BE-1, BE-NEW-7, BE-NEW-12, `POST /api/sessions/import`, **`POST /api/shutdown`** ⭐, **Templates CRUD** ⭐ |
| FE-NEW-8 file_path open | BE-1, BE-NEW-7, BE-NEW-12 |
| **FE ServerShutdownConfirmModal** ⭐ (Tier 3) | **`POST /api/shutdown`** ⭐ |
| **FE WS reconnect backoff** ⭐ (Tier 3) | (없음 — client 만) |

**Critical path (v3)**: Stage 1 → Stage 2 → Stage 3 → Stage 4 (BE-NEW-12.5 + **G36 template**) → Stage 5 (BE-NEW-12) → Stage 7 (BE-9 + import + **shutdown**).

---

## 9. Glossary

| 용어 | 의미 |
|---|---|
| **Terminal** | PTY pair + child. 옛 `Pane`. |
| **Workspace** | `${XDG_DATA_HOME}/gtmux/workspace/`. |
| **Session record** | `<workspace>/<name>.json`. |
| **Webpage** | 1 WS 연결. |
| **Match-or-spawn** | Attach 시 layout id ↔ pool id 매칭. |
| **Auto-mount** | trigger session 의 layout 에만 cascade PUT. |
| **Dangling Terminal Reference** | layout id 가 pool 의 alive 와 매칭 X. → focus interaction 시 same id fresh spawn (G25 c2). |
| **Streaming State** | (session, panel) 쌍 단위. |
| **Lock (cross-server)** | `<workspace>/.locks/<name>.lock` flock (G18). |
| **terminal_died broadcast** | WS frame `{ kind, terminal_id, reason }` (G25). |
| **Template** ⭐ | `terminal-templates.toml` 의 spawn preset (`{ name, command, cwd?, env? }`). G36. |
| **terminal_overrides** ⭐ | Panel item 의 per-panel terminal settings (G35). ADR-0018 D4 amend. |

---

## 10. 작업 룰

- English code/log/commit, Korean docs.
- ADR-before-code (본 brief + ADR 외 새 결정 발생 시).
- TDD 권장 — RGR.
- 점진 rename `Pane` → `Terminal` (작업 영역과 함께).
- 불필요한 추가 금지.
- Risky action 사전 확인.

---

## 11. 진입 시 첫 메시지 후보

- "Stage 1 진입" → §6 Stage 1.
- "Schema v2 + terminal_overrides 모양?" → ADR-0018 D1/D3/D4 amend (G35) + §5 의 Rust struct 예시.
- "BE-NEW-12.5 close API" → Stage 4 + ADR-0021 D9/D10.
- "Template spawn (G36)" → Stage 4 의 BE-NEW-10 amend + plan-0007 §13.20.
- "Shutdown endpoint" → Stage 7 의 BE-9 amend + ADR-0014 D7.

---

## 12. 변경 이력

- 2026-05-15 v1: G18~G25 + multi-session pivot.
- 2026-05-15 v2: G26~G29 + D 검증 amend.
- **2026-05-15 v3**: G32~G40 + Tier 3 + Tier 2 P2 deferred. §5 BE 신규 endpoint (POST /api/terminals { template_id } + Templates CRUD + POST /api/shutdown) + §5 schema field (terminal_overrides). Stage 4 / Stage 7 amend.
