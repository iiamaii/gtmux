# 0044 — BE Slice D work package (Settings + Auth + file_path + Import + Shutdown)

> 번호 변경: 본 문서는 처음 `0042` 로 작성되었으나, 같은 날 별 agent 가 `0042-session-attach-recovery.md` 를 land 해 prefix 가 충돌. cold-pickup 의 reading order 혼동 방지를 위해 `0044` 로 rename. 내용 변화 없음.

- 일자: 2026-05-16
- 작성자: FE 통합 세션 agent (0041 cold-pickup 후속 — Slice A/B/C ship 직후)
- 종류: cold-pickup work package — BE 가 받아 작업할 endpoint 매트릭스 + FE 측 consumer 상태 + wire-up 명세
- 후속 reading order: 본 문서 → `docs/agents/backend-handover-v3.md` §5/§6 → 각 ADR (아래 §4 reference)

---

## 0. 한 줄 요약

FE 측 Settings overlay shell + Theme/Shortcut section + chrome 정합은 모두 ship 완료. **나머지 Settings section 4종 (Storage / Auth / Behavior / Debug)** + **FE-NEW-8 file_path open UX** + **Session export/import (G28)** + **ServerShutdownConfirmModal (Tier 3)** 가 BE endpoint 의존으로 placeholder 상태. 본 문서는 BE 가 들어와 *6 endpoint 묶음* 을 ship 하면 FE 가 즉시 wire 가능하도록 *single source of truth* 로 정리.

---

## 1. 컨텍스트 — FE 측 현황

0041 cold-pickup 후 진행된 FE Slice A/B/C 결과 (`docs/reports/0041-fe-be-session-handover.md` §5.3):

- Slice A (Stage 6 의 minimal): ✅ Layer list V2 Tree/Z toggle + ChangeTerminalModal + Panel header more menu + GroupCloseConfirmModal
- Slice B (Stage 5 인프라 마무리): ✅ shortcutRegistry + Space-hold pan modifier (G29) + Viewport sync UI (FE-9)
- Slice C (Stage 7 Settings overlay): ✅ SettingsOverlay shell + Theme section (G27) + Shortcuts section (G26 read-only) + xtermTheme adapter (G27 xterm 부분)
- 추가: LeftPanel/RightPanel tab merge + collapsed rail + chrome 통일

상세 결과는 ADR-0017 의 amend ①~④ + 같은 날 follow-up 라인 참조.

**Settings overlay 의 4 section 이 의도적으로 placeholder 로 ship 됨** — 각 placeholder 가 *BE 의존 endpoint 명시* 로 작업 항목 가시화:

```
SettingsOverlay 안 (lib/chrome/SettingsOverlay.svelte):
  - storage  → "Waiting on BE: /api/file-path/*, /api/sessions/import"
  - auth     → "Waiting on BE: /auth/rotate, /auth/set-password"
  - behavior → "Waiting on BE: PATCH /api/settings"
  - debug    → "Waiting on BE: GET /api/settings"
```

본 work package 의 BE endpoint 가 ship 되면 위 placeholder 가 *그대로의 자리* 에 wire 됨 — `section === '<id>'` 분기 안만 채우면 됨.

---

## 2. BE 의존 endpoint 매트릭스 (priority 순)

backend-handover-v3 §5 의 BE-9 + BE-NEW-12 + Tier 3 shutdown 발췌 + 본 work package 의 진입 순서.

| # | Endpoint | BE 작업 ID | ADR | Stage | Priority | Blocks (FE) |
|---|---|---|---|---|---|---|
| 1 | `GET /api/settings` | BE-9 | 0020 D5 | 7 | 🔴 P0 (가장 작음) | Settings Debug + Behavior section |
| 2 | `PATCH /api/settings` | BE-9 | 0020 D5 | 7 | 🔴 P0 | Settings Behavior (`auto_kill_terminal_on_panel_close` G25) |
| 3 | `POST /api/settings/password` | BE-9 | 0020 D4 | 7 | 🟢 P1 | Settings Auth section (password change) |
| 4 | `POST /api/settings/logout-all` | BE-9 | 0020 D4 | 7 | 🟢 P1 | Settings Auth section (token rotate-style) |
| 5 | `GET /api/file-path/allowlist` | BE-NEW-12 | 0023 D2 | 5 | 🟢 P1 | Settings Storage section editor |
| 6 | `POST/DELETE /api/file-path/allowlist` | BE-NEW-12 | 0023 D2 | 5 | 🟢 P1 | Settings Storage section CRUD |
| 7 | `GET /api/file-path/allowlist-check?path=<p>` | BE-NEW-12 | 0023 D3 | 5 | 🟢 P1 | FilePathNode double-click 사전 confirmation |
| 8 | `POST /api/file-path/open` | BE-NEW-12 | 0023 D4 | 5 | 🟢 P1 | FilePathNode double-click 실제 open + audit |
| 9 | `POST /api/sessions/import` | BE-9 | sketch §11.2.A | 7 | 🟡 P2 | Settings Storage section export/import (G28) |
| 10 | `POST /api/shutdown` | BE-9 (Tier 3) | 0014 D7 | 7 | 🟡 P2 | ServerShutdownConfirmModal (별 컴포넌트) |

권장 ship 순서: **1 → 2 → 5/6/7/8 (file_path 일괄) → 3/4 → 9 → 10**.

각 endpoint 의 ADR 참조 + wire shape + FE consumer 위치는 §3 참조.

---

## 3. Endpoint 별 명세 + FE wire-up 지점

### 3.1 `GET /api/settings` (BE-9)

**ADR**: 0020 D5 (Settings model) + plan-0007 §13.7.

**Wire shape (제안)**:
```json
{
  "build": { "sha": "c60ba43", "version": "0.1.0", "rust": "1.85" },
  "server": { "pid": 12345, "bind": "127.0.0.1", "port": 9999, "log_path": "/tmp/gtmux.log" },
  "behavior": { "auto_kill_terminal_on_panel_close": false },
  "auth": { "token_present": true, "password_set": true, "argon2_cost": 2 }
}
```

**구분**:
- `build/server/auth` 는 *boot-immutable* (PATCH 금지, 403)
- `behavior` 만 mutable (다음 endpoint 참조)

**FE consumer**:
- `lib/chrome/SettingsOverlay.svelte:section === 'debug'` 분기 — 현재 placeholder. ship 시 `<dl>` 로 `build/server` 표시.
- `lib/chrome/SettingsOverlay.svelte:section === 'behavior'` 분기 — `behavior` snapshot 표시.

**FE wire 시 추가 작업**:
- `lib/http/settings.ts` 신규 — `getSettings()` / `patchSettings()` HTTP client.
- `lib/stores/settingsStore.svelte.ts` 신규 — settings snapshot + auto-refresh on overlay open.

---

### 3.2 `PATCH /api/settings`

**Wire shape**:
```json
{ "behavior": { "auto_kill_terminal_on_panel_close": true } }
```

**Validation**:
- `behavior` 만 허용 — boot-immutable section 포함 시 400 + `{ "error": "boot_immutable", "field": "build" }`.
- 알 수 없는 field → 400 + `{ "error": "unknown_field", "field": "<name>" }`.
- 성공 시 갱신된 전체 `behavior` snapshot 반환.

**FE consumer**:
- `SettingsOverlay.svelte` Behavior section 의 toggle 컨트롤 (체크박스).
- ADR-0021 G25.1.b — 본 toggle 이 true 면 panel close 시 mirror-only terminal 자동 SIGTERM.

---

### 3.3 `POST /api/settings/password`

**ADR**: 0020 D4 (password rotation), D5 (Argon2id m=64 MiB, t=3, p=4), D7 (token/password 동시 운용 거부 — 본 endpoint 는 password mode 또는 password 가 이미 설정된 token mode 에서만 동작).

**Wire shape**:
```json
// req
{ "current_password": "...", "new_password": "..." }
// res 200 + Set-Cookie: gtmux_auth=<new>
{ "ok": true, "revoked_count": 3 }
// res 401
{ "error": "current_password_mismatch" }
// res 400 — new password failed validation
{ "error": "weak_password", "min_length": 8 }
// res 503 — password not yet set (token mode 또는 첫 설정 전)
{ "error": "password_not_set", "message": "..." }
// res 500
{ "error": "save_failed" | "hash_failed" | "issue_failed" }
```

**Validation** (D5 의 amend ② — 2026-05-16):
- `new_password.len() >= 8` (`MIN_PASSWORD_LENGTH`)
- 영문자 1+ AND 숫자 1+ (D5 의 "영문 + 숫자" — zxcvbn 은 P2+)

**Side effect**:
- Argon2id rehash → `${XDG_STATE_HOME}/gtmux/password.argon2` atomic write (mode 0600, ADR-0020 D5)
- `AppState.password_hash` 의 in-memory swap
- `SessionTable::revoke_others(caller_cookie)` — caller 외 모든 session 무효화 (`revoked_count` 응답)
- caller 의 옛 cookie 도 revoke + 새 cookie 발급 (`Set-Cookie` header) — 같은 cookie 를 공유한 다른 탭도 로그아웃 (defense-in-depth)

**FE consumer**: SettingsOverlay Auth section — 2 input + submit. 성공 시 toast + 자동 cookie 갱신 (브라우저 의 `Set-Cookie` 자연 처리).

---

### 3.4 `POST /api/settings/logout-all`

**ADR**: 0020 D4 (logout all other sessions).

**Wire shape**:
```json
// req body 없음 (또는 무시)
// res 200
{ "revoked_count": 3 }
// res 403 — Bearer-only (cookie 없음)
{ "error": "session_cookie_required" }
```

**Side effect**:
- `SessionTable::revoke_others(caller_cookie)` — caller 의 cookie 는 보존, 다른 cookie 모두 `inner` map 에서 제거 (in-memory 추적이라 jti blacklist 불필요)
- 다른 webpage 의 다음 request → `validate(cookie)` Miss → 401 → `/auth` 로 bounce
- caller 의 cookie 는 그대로 valid

**FE consumer**: SettingsOverlay Auth section — 버튼 + confirm dialog. 응답 후 toast.

---

### 3.5 `GET /api/file-path/allowlist`

**ADR**: 0023 D2 (allowlist entry = `{ ext, prefix }` tuple) + D3 (저장 위치).

> **2026-05-16 amend (Slice D-2 ship 직전)**: 본 절의 원안 wire (single `path` directory root, prefix-only matching) 는 ADR-0023 D2 의 *명시 거부 분기* — 신뢰 dir 안 실행 파일 (`*.sh` 등) 의 자동 modal-bypass 위험 때문. 본 amend 가 ADR-0023 D2 의 ext+prefix tuple 모델 정합으로 wire 갱신. FE wire 의 변화: SettingsOverlay Storage section 의 add UI 가 *path 단일* → *ext + prefix 둘* 입력. Match algorithm = `path.starts_with(entry.prefix) && path.to_lowercase().ends_with("." + entry.ext.to_lowercase())`. ext case-insensitive, prefix case-sensitive (POSIX), prefix 끝 `/` 강제 (recursive subdir 포함).

**Wire shape**:
```json
{ "entries": [
  { "ext": "md",  "prefix": "/Users/ws/notes/",   "added_at": 1747000000, "label": "Notes" },
  { "ext": "pdf", "prefix": "/Users/ws/docs/",    "added_at": 1747100000, "label": null }
]}
```

`added_at` 는 Unix epoch seconds (u64) — `TerminalMetadataStore.created_at` 등 기존 wire 의 timestamp 패턴 정합.

**Storage**: `${XDG_CONFIG_HOME:-~/.config}/gtmux/file-open-allowlist.json` (ADR-0023 D3 amend ① — TOML 대신 JSON 채택, http-api 의 기존 serde_json dep 만으로 충분 + 새 toml dep 회피). Workspace-scoped allowlist (per-session) 은 ADR-0023 D10 의 P1+ 후보 — MVP 는 user-scoped 단일 파일.

**FE consumer**: SettingsOverlay Storage section list (ext + prefix + added_at + [Delete]).

---

### 3.6 `POST/DELETE /api/file-path/allowlist`

**Wire shape**:
```json
// POST req
{ "ext": "md", "prefix": "/Users/ws/notes/", "label": "Notes" }
// POST res 201 → §3.5 의 단일 entry shape
// POST res 400 → { "error": "<reason>" } where reason ∈ {
//   "ext_invalid"          | "prefix_not_absolute" | "prefix_not_directory" |
//   "prefix_must_end_slash" | "prefix_not_exists"  | "already_in_allowlist" |
//   "ext_contains_dot"     | "ext_empty"
// }
//
// DELETE /api/file-path/allowlist?ext=md&prefix=/Users/ws/notes/ → 204
// DELETE 비매치 → 404 { "error": "entry_not_found" }
```

**Validation**:
- `ext`: non-empty, no leading dot, no path separator. Lowercase 정규화 후 저장.
- `prefix`: 절대 경로, 끝 `/` 강제, `std::fs::canonicalize` 통과 (존재 + symlink resolve), canonical 결과의 `.is_dir()` 가 true.
- Compound key `(ext, prefix)` unique — 중복 entry 거부.

**FE consumer**: SettingsOverlay Storage section — Add (ext + prefix input) / Delete (per-entry button).

---

### 3.7 `GET /api/file-path/allowlist-check?path=<p>`

**ADR**: 0023 D5/D6 (allowlist 사전 검사).

**Wire shape**:
```json
{ "allowed": true,  "matched_entry": { "ext": "md", "prefix": "/Users/ws/notes/" } }
// 또는
{ "allowed": false, "reason": "not_in_allowlist" }
// 또는
{ "allowed": false, "reason": "path_not_absolute" | "path_not_exists" | "nul_byte" }
```

**검증 순서** (ADR-0023 D5 1~4):
1. `path` 절대경로 — 아니면 `path_not_absolute`
2. NUL byte 없음 — 있으면 `nul_byte`
3. `canonicalize` — 실패 (= 존재 X 또는 symlink loop) 시 `path_not_exists`
4. canonical path 가 entry 와 매칭 (D2 의 알고리즘) — 매치 없으면 `not_in_allowlist`

**FE consumer**:
- `lib/canvas/FilePathNode.svelte` 의 double-click handler — open 전 사전 확인 + 미허용 시 `FileOpenConfirmModal` (FE-NEW-8) 로 사용자에게 *추가 confirmation* 요청 + "Always for *.<ext> within <prefix>/" 체크박스 (D4 의 UX).

---

### 3.8 `POST /api/file-path/open`

**ADR**: 0023 D5 (실제 OS open) + D9 (audit) + D6 (one-time flow).

**Wire shape**:
```json
// req
{ "path": "/Users/ws/...", "user_confirmed": false }
// res 200 — allowlist 매치 또는 user_confirmed=true 통과
{ "opened": true, "allowed_via": "allowlist" | "one_time" }
// res 403 — 미매치 + user_confirmed=false
{ "error": "user_confirmation_required" }
// res 400 — path validation 실패
{ "error": "path_not_absolute" | "path_not_exists" | "nul_byte" }
// res 500 — spawn 실패 (xdg-open 없음 등)
{ "error": "spawn_failed", "reason": "no_handler" | "<other>" }
```

**검증 + side effect**:
- D5 1~3 (path 절대 / NUL / canonicalize) 거친 후
- 매치 OR `user_confirmed=true` 시 `std::process::Command::new("open"|"xdg-open"|"start").arg(canonicalized).spawn()` (shell 비경유, D5 step 5/6).
- 매 호출 audit log 1줄 NDJSON — `path`, `allowed_via` ∈ `{"allowlist", "one_time"}`, `timestamp`, `cookie_prefix` (D9). 거부된 시도도 audit `denied` 로 기록.
- `allowed_via: "one_time"` 은 *transient* — allowlist 자동 추가 X. 매번 confirm modal 필수 (D4 의 UX).

> **D6 의 nonce 정합**: MVP 는 *client 의 `user_confirmed=true` 직접 신뢰* — cookie SameSite=Strict + Origin check (ADR-0020, ADR-0003) 가 CSRF 방어 + single-user 환경 가정. ADR-0023 D6 의 *5s pre-issued nonce* (allowlist-check 응답에 nonce 부여, open 시 검증) 는 **P1+ defense-in-depth** — XSS injection 또는 third-party origin abuse 시 추가 막. ADR-0023 §변경이력 ① 의 nonce defer note 참조.

**FE consumer**:
- FilePathNode double-click → `GET /allowlist-check` → `allowed`면 `POST /open user_confirmed=false` (즉시 open) → modal X
- 비허용면 `FileOpenConfirmModal` → "Always" 체크 + Open → `POST /allowlist {ext, prefix}` + `POST /open user_confirmed=true`
- Open only → `POST /open user_confirmed=true` (one_time 으로 audit)

**FE wire 시 추가 작업**:
- `lib/chrome/FileOpenConfirmModal.svelte` 신규 — "Always for *.<ext> within <prefix>/" 체크박스 + auto-suggested ext (`path.extension()`) + auto-suggested prefix (`path.parent()/`)
- `lib/stores/fileOpenDialog.svelte.ts` 신규
- `lib/http/filePath.ts` 신규

---

### 3.9 `POST /api/sessions/import`

**ADR**: sketch §11.2.A (G28 import). Storage primitives 는 ADR-0018 (schema v2) + ADR-0019 (workspace + validate_session_name) 그대로 재사용.

**Wire shape** (Slice D-4 ship 2026-05-16 — multipart 제외, raw JSON only):
```json
// req — raw JSON body, application/json
{ "name": "imported-session", "layout": { ...schema_v2 SessionLayout... } }
// res 201
{ "name": "imported-session", "created_at": 1747200000 }
// res 409 — 같은 name 이 이미 존재
{ "error": "name_conflict", "name": "imported-session" }
// res 400 — name 검증 실패 (validate_session_name)
{ "error": "<workspace_error_code>", "message": "..." }
// res 400 — layout 검증 실패 (schema::validate)
{ "error": "schema_invalid", "field": "<ValidationError::code>", "details": "<display>" }
// res 503
{ "error": "workspace_not_configured" }
// res 500 — disk write 실패
{ "error": "<workspace_error_code>" }
```

`created_at` = Unix epoch seconds (u64), 본 endpoint 가 atomic file write 직후 발생시킨 timestamp. ADR-0019 의 file mtime 과 별 (수십 ms 차이).

**Validation 순서**:
1. `workspace.is_none()` → 503 `workspace_not_configured`
2. `validate_session_name(name)` → 400 (`empty_name` / `name_traversal` / `invalid_char` 등)
3. `schema::validate(layout)` → 400 `schema_invalid` + `field` (`ValidationError::code()`) + `details` (display)
4. `path.exists()` (workspace 안 `<name>.json`) → 409 `name_conflict` + `name`
5. `canonical_bytes(layout)` → `atomic_write_session(path, bytes)` → 500 or 400 on IO failure
6. `SessionCache.entries.insert(name, ...)` — 캐시 시드, 다음 GET /layout 이 disk 안 거치게
7. 201 `{ name, created_at }`

**Terminal item UUID 정합**: import 는 *side-effect-free* — Terminal pool 은 손대지 않음. 첫 attach 시 match-or-spawn 의 *spawn arm* (ADR-0018 D6) 가 동작 — 사용자 confirm dialog 통해 fresh spawn.

**Multipart 미지원 (MVP)**: 0044 원안은 multipart-or-JSON 둘 다 시사했으나 실 wire 는 *application/json only*. FE 의 Import button 이 file picker 결과를 클라이언트에서 JSON 파싱 후 본 endpoint 에 raw JSON 으로 POST. multipart 의 streaming 이 필요한 큰 layout 의 경우 P1+ 에서 확장.

**FE consumer**:
- SettingsOverlay Storage section — Export button (`GET /api/sessions/<name>/layout` 호출 후 `application/json` blob 다운로드 — 이미 endpoint 있음) + Import button (file picker → POST).

---

### 3.10 `POST /api/shutdown` (Tier 3)

**ADR**: 0014 D7 (graceful teardown).

**Wire shape**:
```json
// req body 없음
// res 202 — accepted, async shutdown 시작
{ "shutdown": "scheduled", "expected_exit_code": 6 }
```

**Behavior** (backend-handover-v3 §5 발췌):
1. 모든 WS connection 에 `server_shutdown` notify broadcast (WS frame 0x89 or 신 type — 별 ADR 필요)
2. WS connection 모두 close (1000 normal)
3. 모든 child process SIGHUP
4. 모든 session record sync flush (atomic write 완료 보장)
5. 모든 lock file 정리 (`.locks/*.lock` unlink)
6. exit code 6 (graceful)

**FE consumer**:
- 신규 `lib/chrome/ServerShutdownConfirmModal.svelte` — confirm dialog (활성 session 수 + terminal 수 + "all data flushed" 안내).
- 진입점: SessionMenu 의 새 "Server shutdown..." 항목 (현 "Session shutdown" 과 별 — 후자는 세션 단위, 전자는 서버 전체) + `Cmd+Shift+Q` shortcut 또는 별 단축키.
- ServerShutdown 후 사용자가 새 `gtmux start` 로 진입 — FE 는 close code 1000 + `server_shutdown` notify 받으면 ReconnectBanner 의 *재연결 시도 없는* 분기 로 진입.

**별 ADR 필요**: WS frame `server_shutdown` 의 type byte + payload 결정 (현재 0x80~0x88 사용 중). 후속 ADR 또는 ADR-0014 amend.

---

## 4. ADR / 문서 reference (cold-pickup 순서)

| # | 파일 | 관련 endpoint |
|---|---|---|
| 1 | `docs/reports/0041-fe-be-session-handover.md` | FE 측 진행 상황 base |
| 2 | `docs/agents/backend-handover-v3.md` §5 / §6 / §8 | BE-9 / BE-NEW-12 / Stage 7 매트릭스 |
| 3 | `docs/adr/0020-auth-lifecycle.md` D4/D5/D7 | Settings auth + password |
| 4 | `docs/adr/0023-file-path-open-security.md` D2/D3/D4/D5 | file_path open 5 endpoint |
| 5 | `docs/adr/0014-server-process-supervision.md` D7 | shutdown graceful teardown |
| 6 | `docs/adr/0021-terminal-pool-and-mirror.md` G25.1.b | `auto_kill_terminal_on_panel_close` toggle 의 의미 |
| 7 | `docs/adr/0018-canvas-item-data-model.md` D5/D6 | Schema v2 + import 의 match-or-spawn |
| 8 | `docs/sketch.md` §11.2.A | Tier 3 shutdown / G28 import 원안 |
| 9 | `docs/adr/0017-layout-grid-and-chrome.md` amend ④ | Settings overlay 구조 (FE 측 spec) |

---

## 5. 권장 진입 순서

### Slice D-1 — Settings API minimal (가장 작음, 즉시 가치 있음)
1. `GET /api/settings` — `build/server/behavior/auth` snapshot
2. `PATCH /api/settings` — `behavior.auto_kill_terminal_on_panel_close` toggle
3. cargo test 신규 — boot_immutable PATCH 거부 + behavior update 정상
4. FE wire (별 세션, BE ship 후): SettingsOverlay 의 Debug + Behavior section

### Slice D-2 — file_path open (Stage 5 잔여, ADR-0023 정합)
5. `GET /api/file-path/allowlist`
6. `POST/DELETE /api/file-path/allowlist` + 절대경로/존재 validation
7. `GET /api/file-path/allowlist-check`
8. `POST /api/file-path/open` + OS native open + audit log
9. cargo test 신규 — 5 test case (위 §3.6/§3.7/§3.8 의 응답 분기)
10. FE wire (별 세션): FilePathNode + FileOpenConfirmModal + SettingsOverlay Storage section

### Slice D-3 — Auth (Stage 7)
11. `POST /api/settings/password` + Argon2id cost + cookie 재발급
12. `POST /api/settings/logout-all` + cookie secret rotation 또는 jti blacklist
13. FE wire: SettingsOverlay Auth section

### Slice D-4 — Import / Export (G28)
14. `POST /api/sessions/import` + schema validation + name conflict
15. (Export 는 기존 `GET /api/sessions/<name>/layout` 재사용 — BE 작업 0)
16. FE wire: SettingsOverlay Storage section export/import buttons

### Slice D-5 — Server shutdown (Tier 3, 가장 큰 ADR work 필요)
17. **별 ADR amend** — WS `server_shutdown` notify frame type byte
18. `POST /api/shutdown` + 6-step teardown
19. cargo test — child SIGHUP + lock cleanup + exit code 6
20. FE wire: ServerShutdownConfirmModal + SessionMenu "Server shutdown..." 항목 + ReconnectBanner 의 1000 normal + server_shutdown notify 분기

---

## 6. 검증 plan (BE side)

각 Slice ship 후:

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo test --workspace --color=never
cargo build --release --bin gtmux
```

**Integration smoke (Slice D-1 후)**:
```bash
unset TMUX
GTMUX_FRONTEND_DIST=../frontend/dist GTMUX_SERVER__SESSION=demo \
  GTMUX_SERVER__PORT=9999 ./target/release/gtmux start --session demo --port 9999 &
TOKEN=...  # bootstrap 에서 얻음
curl -b "gtmux_auth=$COOKIE" http://127.0.0.1:9999/api/settings
curl -b "gtmux_auth=$COOKIE" -X PATCH -H 'Content-Type: application/json' \
  -d '{"behavior":{"auto_kill_terminal_on_panel_close":true}}' \
  http://127.0.0.1:9999/api/settings
```

**Integration smoke (Slice D-2 후)**:
```bash
curl -b "gtmux_auth=$COOKIE" "http://127.0.0.1:9999/api/file-path/allowlist-check?path=/etc/hosts"
curl -b "gtmux_auth=$COOKIE" -X POST -H 'Content-Type: application/json' \
  -d '{"path":"/etc/hosts","user_confirmed":true}' \
  http://127.0.0.1:9999/api/file-path/open
```

---

## 7. FE 측 sync 작업 (BE Slice 별 land 후)

BE ship 후 FE 측 work-package — 본 문서의 §3 의 각 endpoint 의 "FE consumer" / "FE wire 시 추가 작업" 라인 따라:

| BE Slice | FE 추가 파일 | FE 수정 파일 |
|---|---|---|
| D-1 | `lib/http/settings.ts`, `lib/stores/settingsStore.svelte.ts` | `lib/chrome/SettingsOverlay.svelte` (Debug + Behavior section 분기 채움) |
| D-2 | `lib/http/filePath.ts`, `lib/stores/fileOpenDialog.svelte.ts`, `lib/chrome/FileOpenConfirmModal.svelte` | `lib/canvas/FilePathNode.svelte` (double-click handler), `lib/chrome/SettingsOverlay.svelte` (Storage section list/CRUD), `+page.svelte` (modal mount) |
| D-3 | (auth utility 추가) | `lib/chrome/SettingsOverlay.svelte` (Auth section 2 input + 2 button) |
| D-4 | (export blob helper) | `lib/chrome/SettingsOverlay.svelte` (Storage section export/import buttons) |
| D-5 | `lib/chrome/ServerShutdownConfirmModal.svelte`, `lib/stores/shutdownDialog.svelte.ts` | `lib/chrome/SessionMenu.svelte` (별 menu item), `lib/ws/decode.ts` (server_shutdown frame), `lib/banner/ReconnectBanner.svelte` (1000 normal + server_shutdown 분기) |

---

## 8. 변경 이력

- 2026-05-16: 초안 — 0041 cold-pickup 후 FE Slice A/B/C ship 직후. BE 가 Slice D 진입 시 cold-pickup 용 단일 source. backend-handover-v3 §5/§6 발췌 + FE consumer 명세 + 권장 진입 순서 (D-1 → D-5) + 검증 plan.
- 2026-05-16: **Slice D-1 ship 완료** — `GET/PATCH /api/settings` 구현 (`crates/http-api/src/settings.rs`, +8 unit test, 워크스페이스 329 → 337 PASS). ADR-0020 D11 신규 (D11.1~D11.6, FE-consumer wire 의 ADR 승격). 검증 통과:
  - cargo test --workspace --no-fail-fast → 337 PASS / 0 FAIL
  - release binary 수동 E2E: GET 200 4-section snapshot, PATCH behavior 200 updated, PATCH boot-immutable → 400 `{error: "boot_immutable", field: "server"}`
  - 다음 진입 권장: **Slice D-2 (file_path open)** — 5 endpoint, ADR-0023 D2/D3/D4 따름
- 2026-05-16: **Slice D-2 ship 완료** — `/api/file-path/*` 5 endpoint (`crates/http-api/src/file_open/` 모듈: mod + allowlist + audit + spawn + handlers). 워크스페이스 337 → 368 PASS (+31 unit test). ADR-0023 amend ① 동반 (본 §3.5-§3.8 의 wire shape 를 ADR-0023 D2 의 ext+prefix tuple 모델 정합으로 재작성 — `*.sh` modal-bypass 막. nonce 는 P1+ defer). 검증 통과:
  - cargo test --workspace --no-fail-fast → 368 PASS / 0 FAIL
  - release binary smoke gate 5-9: GET empty / POST canonicalize / GET 1 entry / check (.md allowed, .sh denied — D2 invariant) / open denied without confirm 403 / DELETE 204 → 404
  - ADR-0023 D2 의 "신뢰 dir 안 `*.sh` 자동 허용" 공격 시나리오를 unit test (`shell_script_not_auto_matched_under_md_prefix`) + smoke gate 5-9 의 .sh check denied 양쪽으로 보호
  - 다음 진입 권장: **Slice D-3 (Auth Stage 7)** — `/auth/password` + `/auth/logout-all`, ADR-0020 D4 + D7
- 2026-05-16: **Slice D-3 ship 완료** — `POST /api/settings/password` + `POST /api/settings/logout-all` (`crates/http-api/src/settings.rs::password_handler/logout_all_handler` + `crates/http-api/src/auth.rs::SessionTable::revoke_others`). 워크스페이스 368 → 375 PASS (+7 unit test). ADR-0020 amend ② 동반 (D12 신규 — endpoint 명세 + atomic ordering + password validation MVP). `AppState.password_hash` 의 type 을 `Arc<RwLock<Option<String>>>` 로 runtime-mutable 화. `AppState.password_hash_path` 신규 — boot 시점 path pin. 검증:
  - cargo test --workspace --no-fail-fast → 375 PASS / 0 FAIL
  - 7 신규 unit test: happy path (cookie re-issue + disk persist + verify_password 검증), wrong current → 401, weak new → 400 (len + letter+digit 양쪽), no password set → 503, revoke_others 정합 (3 다른 session 제거 + caller 새 cookie), logout-all happy path, bearer-only → 403
  - 다음 진입 권장: **Slice D-4 (Import/Export G28)** — `POST /api/sessions/import`, sketch §11.2.A
- 2026-05-16: **Slice D-4 ship 완료** — `POST /api/sessions/import` (`crates/http-api/src/sessions.rs::import_handler`). 워크스페이스 375 → 380 PASS (+5 unit test) + smoke gate 5-11. 본 §3.9 wire shape 갱신 (multipart 제외 + 모든 error code 명시 + validation 순서). 본 endpoint 는 BE-only — Export 는 기존 `GET /api/sessions/<name>/layout` 재사용 (BE 작업 0). 검증:
  - cargo test --workspace --no-fail-fast → 380 PASS / 0 FAIL
  - 5 신규 unit test: 201 happy path (disk + cache + created_at), 409 name_conflict (create 후 import 동일 이름), 400 invalid_name (`../escape`), 400 schema_invalid (schema_version=1), 후속 list 가 import 결과 포함
  - smoke gate 5-11: 201 → 409 → 400 schema_invalid → list 정합
  - **새 ADR amend 없음** — sketch §11.2.A + ADR-0018 (schema v2 validator) + ADR-0019 (workspace) 의 기존 결정 그대로 재사용
  - 다음 진입 권장: **next-2 (ADR-0025 ratify)** — FE-NEW-6 land 의존, 또는 **D-5 (Server shutdown Tier 3)** — 별 ADR 선행 필요 (WS `server_shutdown` frame type byte)
- 2026-05-16: **Slice D-5 ship 완료** — `POST /api/shutdown` + WS `0x89 SERVER_SHUTDOWN` (`crates/http-api/src/shutdown.rs` + `crates/ws-server/src/{lib.rs::FrameType::ServerShutdown, hub.rs::ServerShutdownEvent, payload.rs::encode_server_shutdown}`). 워크스페이스 380 → 382 PASS (+2 unit test) + smoke gate 5-12. ADR-0014 amend ② 동반 (D12 신규 — HTTP-initiated graceful shutdown + WS frame 할당 + 6-step background task + R12 거절안 + 보안/가시성). 검증:
  - cargo test --workspace --no-fail-fast → 382 PASS / 0 FAIL (handover §2.3 의 frame 표 갱신 정합 — 0x89 SERVER_SHUTDOWN 추가, 0x8A~ unassigned)
  - 2 신규 unit test: 503 hub_not_configured (hub-less AppState 의 정상 분기) + 401 (인증 middleware 게이트). 실 exit 6 + 0x89 emission 은 smoke 의 별 process 에서 검증 (cargo test 안 std::process::exit 호출 X)
  - smoke gate 5-12 (release binary E2E): POST 202 → 0x89 SERVER_SHUTDOWN inner JSON {reason, expected_exit_code} → close frame 1000 → 서버 process exit code 6. 모든 단계 ordering 검증
  - 본 §3.10 wire shape + behavior 가 ADR-0014 D12 의 endpoint 명세 + 6-step 시퀀스와 정합
  - **모든 Slice D 완료**. 잔여 = next-2 (ADR-0025 ratify) — FE-NEW-6 land 의존
