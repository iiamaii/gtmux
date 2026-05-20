# ADR-0023: file_path Item 의 OS-level Open 보안 정책

- 상태: Accepted (2026-05-15)
- 일자: 2026-05-15 (Proposed + Accepted, G21 grilling)
- 결정자: agent (security-engineer + system-architect role) + user grilling G21
- 근거 grilling: G21 / G21.1 / G21.2
- 관련 ADR: ADR-0018 (Canvas Item Data Model — file_path payload), ADR-0019 (Workspace + Session), ADR-0020 (Auth Lifecycle)
- 관련 SSoT: `docs/ssot/canvas-layout-schema.md` (file_path payload), `docs/ssot/file-open-allowlist.md` (P1+)

## 맥락

ADR-0018 D4 의 `file_path` item 은 *path string 의 시각 bookmark*. ADR-0018 line 245 의 보안 baseline = *"path 는 string metadata 만, backend 가 자동 read/open 안 함"*. G21 grilling 에서 사용자가 *OS-level open 까지 허용* 으로 결정 — 보안 표면이 커진다. 본 ADR 은 *어떻게 안전히 허용하는지* 의 디테일.

핵심 위험:
1. **Command injection** — path 가 shell 을 거치면 임의 명령 실행.
2. **Path traversal** — 사용자가 의도 안 한 경로 접근.
3. **Drive-by execution** — 무심코 double-click 으로 위험 파일 (rootkit, payload) 실행.
4. **Confused deputy** — server 가 사용자 권한 (server uid) 으로 임의 path open — 의도 외 권한 사용 가능.

본 ADR 은 *MVP 의 적절한 안전 막* 을 세운다.

## 결정 (Decisions)

### D1. Open trigger = double-click → confirm modal → backend OS open

- **Single-click**: item selection (다른 Canvas Item 과 일관).
- **Double-click**: `Open externally?` confirm modal 발동.
- **Modal 통과 후**: 클라이언트가 `POST /api/file-path/open { path }` 호출 → backend 가 검증 후 `xdg-open` (Linux) / `open` (macOS) 명령 spawn.
- 다른 trigger 없음 (single-click 으로 자동 open 안 함, hover 로 자동 open 안 함).

### D2. Allowlist scope = ext + path prefix intersection

각 allowlist entry = `{ ext: string, prefix: string }`.

매칭 규칙:
```
allow(path) = ∃ entry:
   path.starts_with(entry.prefix)
   && path.lowercase().ends_with("." + entry.ext.lowercase())
```

- ext 는 *case-insensitive* (`md` 와 `MD` 동일).
- prefix 는 *case-sensitive* (POSIX 정합).
- prefix 는 *디렉터리* — 끝에 `/` 강제.
- prefix 안 의 subdirectory 모두 포함 (recursive).

대안 검토:
- ext only — 거부. 다른 dir 의 같은 ext 까지 자동 허용 = 표면 큼.
- prefix only — 거부. 신뢰 dir 안 의 실행파일 (`*.sh`) 까지 자동 허용 = 표면 큼.
- 정확한 path 한 건 — 거부. "always" 의미 약함, 매 새 file 마다 confirm 부담.

### D3. Allowlist 저장 위치 + 편집 UI

- **저장 위치**: `${XDG_CONFIG_HOME:-~/.config}/gtmux/file-open-allowlist.toml`
- **Schema**:
  ```toml
  [[entry]]
  ext = "md"
  prefix = "/home/me/notes/"
  added_at = "2026-05-15T12:00:00Z"

  [[entry]]
  ext = "pdf"
  prefix = "/home/me/docs/"
  added_at = "2026-05-15T12:30:00Z"
  ```
- **편집 UI**: Settings overlay (G19) 의 **Storage** section. 표시:
  - 각 entry: ext + prefix + added_at + [Delete] 버튼
  - [Add entry] 버튼 **없음** — entry 는 *confirm modal 의 체크박스로만 추가* (D4). 직접 추가는 보안 표면 큼.
- 변경 동작: D1 의 정책 (G19.1 의 auto-save 정합) — Delete 즉시 PATCH.

### D4. Confirm modal UX

```
┌─ Open externally? ──────────────────────┐
│                                         │
│ Path: /home/me/notes/spec.md            │
│                                         │
│ [✓] Always for *.md within             │
│     /home/me/notes/                     │
│   (auto-suggested prefix + ext)         │
│                                         │
│   [Cancel] [Open]                       │
└─────────────────────────────────────────┘
```

- *Always for* 체크박스의 표시 패턴은 자동 추론:
  - `ext` = `path.extension()` (없으면 체크박스 disabled — extensionless 파일은 1회 confirm 만)
  - `prefix` = `path.parent()` (file_path item) 또는 `path.parent().parent()` 같은 *상위 dir* (사용자가 dropdown 으로 한 단계 ↑/↓ 가능 — P1+ 에서 검토)
  - MVP 는 *immediate parent + ext* 만 자동 추론, 사용자가 [+] 버튼으로 상위로 확장 가능 (P1+)
- 체크박스가 *기본 unchecked* — 첫 매번 명시 의도 필요.
- 체크 + Open → allowlist 추가 → 이후 같은 ext+prefix 매칭 path 는 modal 우회.
- Allowlist 매칭 path 의 double-click 은 *modal 우회하고 즉시 backend POST* — 사용자가 의도적으로 "Always" 선택한 결과.

### D5. Backend implementation

- HTTP: `POST /api/file-path/open { path: string }`
- Auth: 현재 인증된 cookie 검증 (ADR-0020).
- **검증 순서**:
  1. `path` 가 절대 경로 (`PathBuf::is_absolute()`) — relative 거부.
  2. `path` 가 NUL byte 미포함.
  3. `path` 의 *canonicalize* (`std::fs::canonicalize`) — symlink resolve, `..` 정리. 실패 시 거부 (= 존재하지 않는 path).
  4. Canonicalized path 가 **allowlist 매칭** OR **사용자가 confirm modal 통과 (clientside intent flag)**. *클라이언트의 intent flag 는 modal 통과를 의미하지만 backend 의 신뢰는 못 함* — modal 우회 공격 방지를 위해 **체크박스 추가 시점에 server 가 allowlist 에 직접 기록** 후 그 호출의 path 는 매칭으로 통과시킨다. 즉 사실상의 source of truth = server 의 allowlist file.
  5. Spawn: `std::process::Command::new("open"|"xdg-open").arg(canonicalized_path).spawn()` — **`.spawn()` 만 사용, `.output()` 안 사용** (서버 측 hang 방지).
  6. Failure (exit code ≠ 0 OR spawn 자체 실패) → 500 + 에러 body (no panic).
- **Shell 비경유**: `Command::new` 의 argv 형식. 절대 `Command::new("sh").arg("-c").arg(format!(...))` 류 X.

### D6. Allowlist 추가의 server-side flow

```
Client 의 double-click:
  1. path 로 GET /api/file-path/allowlist-check?path=<>
     → allowed: bool 반환
  2. allowed === true:
       즉시 POST /api/file-path/open { path }
       (modal 생략)
     allowed === false:
       Confirm modal 표시
       사용자 [✓ Always for] + [Open]:
         POST /api/file-path/allowlist { ext, prefix } (entry 추가)
         그 다음 POST /api/file-path/open { path }
       사용자 [Open] only:
         POST /api/file-path/open?one_time=1 { path }
         (allowlist 미추가, 그 호출만 통과)
```

- `one_time=1` 의 통과는 server-side 의 *짧은 lifetime nonce* 로 보호 — modal 응답 후 5s 안 그 path 한 번만 유효. 그 외 시도는 거부 + audit log.

### D7. Failure 모드 + UX

- **Allowlist 미매칭 + 사용자 [Cancel]**: action 없음.
- **Spawn 실패 (xdg-open 없음, headless server)**: toast "Failed to open externally. Headless environment?" — backend 의 500 응답에 `reason: "no_handler"` 명시.
- **Path 존재 안 함 (canonicalize 실패)**: toast "Path not found: <path>". file_path item 의 *visual* indicator 로 *stale* 표시 (예: 빨간 점). P1+ 에서 *background path-check* 로 자동 stale 표시.
- **NUL byte / relative path / symlink 외부 escape**: 400 + audit log.

### D8. Security baseline

| 위협 | 막음 |
|---|---|
| Command injection | Argv direct spawn (no shell), path canonicalize, NUL 차단 |
| Path traversal (`../..`) | canonicalize 후 그 결과만 사용 — `..` resolve 됨 |
| Symlink escape from prefix | canonicalize 후 *prefix 매칭* 적용 — symlink 가 prefix 외부 가리키면 매칭 실패 |
| Drive-by | Default unchecked + 명시 double-click + first-time confirm |
| Allowlist bypass | server-side allowlist 가 SoT, client intent 는 무시 (D5 step 4) |
| Confused deputy | server uid 권한으로만 spawn (현 server 의 권한 = 사용자의 권한, single-user — 추가 escalation 없음) |
| Env var injection | path 의 `$VAR`, `~` 자동 expand 안 함 — literal string. 사용자가 명시 expanded path 를 file_path item 에 저장해야 함. |

### D9. Audit log

- 모든 `POST /api/file-path/open` 호출은 audit log 에 기록 (path, allowed_via_allowlist, timestamp, ws_conn_id).
- 거부된 시도 (400, 403) 도 기록.
- Log 위치: `${XDG_STATE_HOME:-~/.local/state}/gtmux/audit/file-open-YYYYMMDD.log` (NDJSON).
- 별 ADR 의 audit logging 정책 정합 (P1+, 후속 ADR-0024 후보).

### D10. P1+ 확장 후보 (비범위)

- **Per-session allowlist**: workspace dir 안 `.file-open-allowlist.toml` — workspace 별 다른 정책.
- **Hover preview** (image / pdf): file_path item hover 시 thumbnail 표시. asset storage 정책 (ADR-0022 후보) 정합 후 진행.
- **Multiple parent expansion**: 사용자가 confirm modal 에서 상위 dir 로 확장 (예: `/home/me/notes/` 대신 `/home/me/`).
- **MIME-based handler 분기**: ext 외 magic byte 검사 — backend 부담 큼, MVP 외.
- **Sandboxed open** (Firejail / sandbox-exec) — UX 복잡 + cross-platform 어려움, P2+.

## 대안 검토

### A1. OS-level open 자체 금지 (ADR-0018 line 245 의 baseline 유지)
**거부.** G21 grilling 에서 사용자 명시 — 가치 있음, 다만 안전 막 필수.

### A2. Backend 가 아닌 client-side window.open(file://)
**거부.** Browser 의 file:// 정책으로 차단 + path traversal 보호 약함 + UX 제약.

### A3. Sandboxed wrapper (Firejail / sandbox-exec) 의무
**거부.** Cross-platform 의무화 = MVP 부담 큼. P2+ 에서 검토.

### A4. Allowlist 미사용, 매번 confirm
**거부.** 사용성 떨어짐. "Always for" 의 가치 = D2 의 좁은 scope 으로 충분 통제.

### A5. Auto-detect by MIME (libmagic)
**거부.** MIME 검사 = backend FS read 의무 = file 변조 시점 race. ext+prefix 의 단순함이 MVP 적절.

## 영향

### Code
- **Backend** (신규 crate or module):
  - `http-api/src/file_open/handler.rs` — POST /api/file-path/open, GET /api/file-path/allowlist-check, POST /api/file-path/allowlist
  - `http-api/src/file_open/allowlist.rs` — TOML load/save + 매칭 함수
  - `http-api/src/file_open/spawn.rs` — `Command::new` 안전 spawn
  - `http-api/src/file_open/audit.rs` — NDJSON audit log
- **Frontend**:
  - `lib/canvas/items/FilePathItem.svelte` — double-click handler + GET allowlist-check
  - `lib/canvas/items/FileOpenConfirmModal.svelte` — 신규 modal
  - `lib/chrome/SettingsOverlay/StorageSection.svelte` — allowlist list + delete UI

### ADR
- ADR-0018 D4 의 `file_path` 항목 amend (security policy reference)
- ADR-0018 보안 노트 line 245 amend (baseline 보존 + ADR-0023 의 opt-in 명시)

### Docs
- `docs/ssot/canvas-layout-schema.md` 의 file_path payload 에 security ref
- plan-0007 §13 신규 BE-NEW-12 (file open API + allowlist) + §14 신규 FE-NEW-8 (file open modal + Settings allowlist editor)

### 보안
- 본 ADR 의 D5~D9 가 핵심.
- 외부 보안 검토 시 *attack surface map* 에 file_open 추가 — drive-by execution 의 막을 명시.
- security-audit skill 실행 시 본 ADR 의 D5~D9 검증.

## 변경 이력

- 2026-05-15: 초안 + Accepted. G21 grilling 의 G21 / G21.1 / G21.2 합본. ADR-0018 baseline (line 245) 의 opt-in 정합.
- 2026-05-16: amend ① — Slice D-2 ship 직전 coherence sync (CLAUDE.md "ADR ↔ plan 동시 갱신" 룰 적용):
  - `docs/reports/0044-be-slice-d-work-package.md` §3.5-§3.8 의 prefix-only wire 가 본 ADR D2 의 *명시 거부 분기* 였음을 발견 → 0044 wire 를 D2 의 ext+prefix tuple 모델 정합으로 amend (0044 §3.5 의 inline amend note 참조). FE Settings Storage 의 add UI 가 *ext + prefix 둘 입력* 으로 갱신 필요.
  - D6 의 *5s pre-issued nonce* 는 **P1+ defense-in-depth** 로 defer. MVP 는 client 의 `user_confirmed=true` 를 cookie SameSite=Strict (ADR-0020) + Origin check (ADR-0003) 보호 아래 직접 신뢰. 본 결정의 근거 = single-user 환경 + 명시 modal confirm + audit log + 작은 attack surface (CSRF/XSS 가 가능해야 abuse). P1+ nonce 추가 시 본 ADR D6 의 5s 토큰 모델 그대로 채택.
  - Storage 위치 = `${XDG_CONFIG_HOME:-~/.config}/gtmux/file-open-allowlist.json` (D3 의 위치는 그대로, 0044 의 `<workspace>/...` 오기 정정 + 파일 포맷 TOML → JSON 으로 amend — 기존 `serde_json` dep 만으로 충분 + 새 `toml` 직접 dep 회피. D3 의 schema 그대로, encoding 만 변경).
  - `added_at`: Unix epoch seconds (u64). codebase 의 `TerminalMetadataStore.created_at`, `session_lock.lease_until_unix` 와 같은 패턴.
  - Slice D-2 ship 시점에 implementation 위치: `crates/http-api/src/file_open/` (mod + allowlist + handlers + audit). 본 ADR 의 §영향 코드 매트릭스 그대로.
- 2026-05-17: **amend ① — Picker browse roots 통합 (ADR-0035 정합)**. D2 의 `[[entries]]` (ext+prefix intersection, file-open 권한) 옆에 신규 `[picker.roots]` section 추가 — file system picker 의 browse root list (ext 무관, dir prefix 만). 두 section 은 *시각 분리* — 사용자가 (a) "이 dir 의 *ts 파일은 OS open* 자동 통과" 와 (b) "이 dir 의 *전체 file* picker browse 가능" 을 명확히 구분. 본 amend 의 storage 변경:

  ```toml
  # ${XDG_CONFIG_HOME}/gtmux/file-open-allowlist.json (또는 toml)

  [[entries]]              # 옛 D2 — file-open 권한 (ext + prefix)
  ext = "ts"
  prefix = "/Users/foo/repo"
  added_at = 1779000000

  [picker]                 # amend ① — file picker browse roots (ext 무관)
  roots = [
    "/Users/foo/repo",
    "/Users/foo/notes",
  ]
  ```

  Mutation flow:
  - file-open 의 `POST /api/file-path/allowlist` 는 그대로 (D6 의 server-side write 흐름).
  - picker 의 `POST /api/fs/allowlist/picker-root { path }` 신규 (ADR-0035 D6) — hard blocklist check 후 `[picker.roots]` 에 append.

  Security 표면:
  - 둘 다 같은 toml file — atomic write 보장. 두 section 의 별 mutation 이 race 안 됨 (toml writer 가 file-level lock).
  - hard blocklist (e.g. `/etc`, `/usr`, `/var`, `/root`, `/sys`, `/proc`, `/dev`) 는 두 section 모두에서 reject (server-side enforced).
  - 인증 = bearer/cookie middleware (다른 /api/* 정합).

  정본 ADR = `docs/adr/0035-file-system-picker.md`. BE work-package = `docs/reports/0061-be-fs-list-work-package.md`. 본 amend 가 ADR-0035 의 prerequisite (allowlist 의 toml schema 분리).
