# ADR-0035: File system picker — `file_path` 도구의 path 선택 UX + `GET /api/fs/list`

- 상태: **Draft** (2026-05-17)
- 일자: 2026-05-17
- 결정자: agent (system-architect role) + 사용자 grilling (4 항목)
- 근거 grilling: 2026-05-17 — file-system picker 정책 grilling
- 근거 plan: TBD — 본 ADR Accepted 후 별 plan / work-package
- 관련 ADR: ADR-0018 (Canvas Item v2 — `file_path.path` field), ADR-0023 (File-open allowlist — 본 ADR 의 보안 표면 공유), ADR-0020 (Auth), ADR-0034 (File-stat endpoint — 같은 allowlist resolver 재사용)
- 관련 work package: `docs/reports/0061-be-fs-list-work-package.md` (예정)
- Amends: ADR-0023 — picker 의 browse root 통합 명시 (D2 amend ①)

## 맥락

`file_path` 도구로 spawn 한 item 의 `path` 필드는 현재 *사용자 text 직접 입력* (InlineEdit) 으로만 set 가능. 큰 codebase 에서 path 를 매번 typing 은 비효율 — Figma / Linear 의 file picker 같은 *browse modal* 이 자연.

본 ADR 은 (a) trigger 시점, (b) browsable scope (ADR-0023 정합), (c) BE wire (`GET /api/fs/list`), (d) UI form 4 차원을 잠금. 시안 별 도큐먼트 (v3 components 의 file_path) 의 *path 선택 UX 보강* 이 목적.

## 결정 (Decisions)

### D1. Trigger — spawn 직후 자동 modal + 더블 클릭 재 호출

**주 trigger**:
- Toolbar `[file_path]` 도구 select → canvas click → **picker modal 자동 open** (사용자가 한 단계에서 path 까지 결정).
- 사용자가 path 선택 → file_path item *그 시점에* spawn (실 path 로 populated).
- Cancel → item spawn 안 함 (placeholder 생기지 않음).

**보조 trigger**:
- 기존 file_path item 의 더블 클릭 → picker modal 재 open (수정 의도). 기존 path 가 modal 의 initial focus.
- 또는 더블 클릭 시 InlineEdit (현 동작) 유지 — *사용자 선택*:
  - **결정**: 더블 클릭 = picker (수정 의도 자연). path 의 *손-typing 편집* 은 picker 안 input field 에서 가능 (path text input + browse tree 의 hybrid). InlineEdit 은 제거.

**근거**:
- 사용자가 file_path 도구를 선택했다는 것 = path 가 *지금 결정되어야 함* 의 intent. spawn-then-edit 의 두 단계 친화도 약함.
- Picker cancel = item 생성 자체 안 함 — empty file_path 의 의미 없음.

### D2. Scope — ADR-0023 allowlist 영역 (사용자 동적 확장)

**Initial roots** (server-side default):
1. Workspace dir (`<workspace>/`)
2. ADR-0023 의 file-open allowlist 안 prefix 들 (이미 사용자가 명시 허용한 area)

**사용자 동적 확장**:
- Picker 안 `[+ Add browse root]` 버튼 → path input modal → confirm:
  - 결정: ADR-0023 의 confirm modal 패턴 정합 (D4) — server 측 sandbox check + 사용자 한 번 더 confirm → allowlist 에 추가.
  - 추가된 root 는 ADR-0023 의 toml 의 *같은 allowlist* 에 기록 (`[picker.roots]` section 또는 ext=`*` + prefix 의 entries — D2.1).

### D2.1 Allowlist 통합 (ADR-0023 amend ①)

ADR-0023 의 allowlist 는 file-open 의 (ext + prefix) intersection 기반. picker 의 root 는 *ext 무관 prefix only*. 통합 방식:

**옵션 A**: 같은 entries 의 flag — `{ ext: "*", prefix: "<dir>", allow_picker: true, allow_open: false }`.

**옵션 B**: 별 toml section — `[picker.roots]` = absolute prefix list (ext 무관).

**결정 = 옵션 B**: 별 section. 명확 + 사용자 인지 쉬움 (open 과 browse 의 다른 정책). ADR-0023 D2 amend ① 로 entries 시각 분리.

```toml
# ${XDG_CONFIG_HOME}/gtmux/file-open-allowlist.toml

[[entries]]                # 옛 D2 — ext + prefix intersection (file-open)
ext = "ts"
prefix = "/Users/foo/repo"

[[entries]]
ext = "md"
prefix = "/Users/foo/notes"

[picker]                   # D2.1 amend ① — picker browse roots (ext 무관)
roots = [
  "/Users/foo/repo",
  "/Users/foo/notes",
]
```

### D3. Endpoint — `GET /api/fs/list?dir=<percent-encoded-path>`

```
GET /api/fs/list?dir=src%2Fcomponents

200 OK
{
  "dir": "/Users/foo/repo/src/components",          // resolved absolute
  "parent": "/Users/foo/repo/src",                   // null if dir is a root
  "entries": [
    { "name": "auth", "kind": "directory", "size_bytes": null, "mtime_unix": 1779000000 },
    { "name": "AuthCard.tsx", "kind": "file", "size_bytes": 4232, "mtime_unix": 1779000123 }
  ],
  "total": 24,
  "truncated": false                                 // entries > MAX_ENTRIES 면 true
}
```

**정렬**: dir 먼저 (이름순), file 다음 (이름순).

**Cap**: `MAX_ENTRIES = 500` per request. 초과 시 `truncated: true` + entries 의 first 500 만.

**Hidden files**: default skip (dot-prefixed name). Settings 의 `picker.show_hidden` true 면 표시.

### D4. Error 분기

| Code | Body |
|---|---|
| 400 | `{ "error": "invalid_dir" }` (빈 dir, traversal `..`) |
| 401 | (auth middleware) |
| 403 | `{ "error": "dir_not_allowed" }` (ADR-0023 allowlist / picker.roots 미통과) |
| 404 | `{ "error": "dir_not_found" }` |
| 503 | `{ "error": "workspace_not_configured" }` |

### D5. UI form — FilePickerModal.svelte

**Layout**:
```
┌─ Pick a file ────────────────────────────────────┐
│ ┌─ Roots ──┐  ┌─ Path: /Users/foo/repo/src ───┐ │
│ │ workspace│  │ [breadcrumb] / src / components │ │
│ │ ./notes  │  │ ┌─────────────────────────────┐ │ │
│ │ + add    │  │ │ 📁 auth                     │ │ │
│ └──────────┘  │ │ 📁 ui                       │ │ │
│               │ │ 📄 AuthCard.tsx  4.2 KB     │ │ │
│               │ │ 📄 index.ts      1.1 KB     │ │ │
│               │ └─────────────────────────────┘ │ │
│               │ Filter: [____________________]  │ │
│               └──────────────────────────────────┘ │
│  Selected: /Users/foo/repo/src/AuthCard.tsx       │
│                              [Cancel] [Select]    │
└──────────────────────────────────────────────────┘
```

**Interactions**:
- 더블 click directory → enter (현 dir 의 entries 로 navigate).
- 더블 click file → select + close (modal commit).
- single click → "Selected" footer 갱신 + Select 버튼 활성.
- breadcrumb segment click → 그 dir 로 navigate.
- Filter input (search) → client-side filter 의 fuzzy match (서버 search 는 후속).
- `[+ Add browse root]` → server-confirm flow (D2.1).

### D6. 사용자 root 추가 flow

```
1. picker 안 [+ Add browse root] click
2. path input modal — 사용자가 absolute path typing
3. submit → POST /api/fs/allowlist/picker-root { path }
4. server 가 검증:
   - path 가 home 또는 workspace 외부 → strict confirm modal
   - path 가 sensitive dir (/, /etc, /usr) → reject 400
   - 그 외 → toml 의 [picker.roots] 에 추가
5. 응답 → modal 의 Roots 리스트 갱신
```

**비채택**: 사용자 input 즉시 toml 추가 — security floor 미달.

### D7. Hidden files

Default = dot-prefixed name skip (e.g. `.git`, `.DS_Store`, `.env`). Settings 의 `picker.show_hidden: bool` 로 사용자 조정.

### D8. Cap / Pagination

- entries per request = 500.
- 초과 시 truncated=true. 현 단계는 *사용자 search 로 narrow* 권장. Pagination 은 후속 amend (P3+).

### D9. 인증

`/api/*` middleware 의 bearer/cookie.

### D10. MVP scope (Stage 분리)

- **Stage 1 (MVP, 본 work-package)**: BE handler + ADR-0023 정합 + Initial roots (workspace + allowlist prefix) + entries cap. FE Stage 미진행.
- **Stage 2 (FE)**: FilePickerModal + file_path 도구 의 spawn flow 변경 (Toolbar click → canvas click → modal) + 더블 클릭 재 호출.
- **Stage 3 (사용자 root 동적 추가)**: `[+ Add browse root]` + POST /api/fs/allowlist/picker-root + Settings UI 의 picker roots 리스트.
- **Stage 4 (P3+)**: server-side fuzzy search, fs watch / live refresh, pagination.

## 어휘 매트릭스

- **Picker** = file system tree navigate UI (FilePickerModal).
- **Browse root** = picker 의 entry-level dir — Roots rail 에 표시.
- **Allowlist (file-open)** = ADR-0023 의 ext+prefix intersection (open 권한).
- **Allowlist (picker.roots)** = D2.1 amend ① — picker browse 권한 (ext 무관).

## 대안 검토

- **A1. Native `<input type="file">`**: 거부. browser security 로 *server-side absolute path* 못 잡음. `file_path.path` 의 의도 (path 자체 표시 + ADR-0023 open) 와 mismatch.
- **A2. Tauri / Electron OS dialog**: 거부. gtmux 가 web app — 본 architecture out of scope.
- **A3. Workspace 만 (외부 access X)**: 거부. 사용자가 외부 source code reference 의 의도 — workspace-only 는 사용성 제약.
- **A4. `$HOME` 부터 자유 browse**: 거부. security floor 미달 (사용자가 sensitive dir 노출).
- **A5. Recursive tree (한 호출)**: 거부. 대규모 codebase 에서 5+ 만 entries 의 cost 부담.
- **A6. Search-driven only (tree navigate 없음)**: 거부. 사용자가 dir 구조 인지하지 못한 경우 의 UX 손실. tree + search 의 hybrid 가 자연.

## 영향

### Code

**Backend**:
- 새 module `crates/http-api/src/fs_list.rs` — handler + entries sort + cap + dot-file filter.
- 새 module 또는 기존 ADR-0023 allowlist 의 `picker_roots()` extension.
- `WorkspaceManager::picker_initial_roots()` helper.
- 새 endpoint: `GET /api/fs/list`.
- Stage 3 의 `POST /api/fs/allowlist/picker-root` (ADR-0023 amend ① 의 toml mutation).

**Frontend**:
- `lib/http/fs.ts` 신규 — `listDir(dir)` + `addPickerRoot(path)` http client.
- `lib/chrome/FilePickerModal.svelte` 신규 — D5 의 UI form.
- `lib/canvas/Canvas.svelte` 의 file_path 도구 spawn flow 변경 — onpaneclick 의 `tool === 'file_path'` 분기 가 picker modal trigger.
- `lib/canvas/FilePathNode.svelte` 의 더블 클릭 → picker modal (InlineEdit 제거).

### ADR

- ADR-0023 amend ① — `[picker.roots]` section + browse 권한 분리.
- ADR-0034 (file-stat) — 같은 allowlist resolver 공유 명시.

### Docs

- `docs/reports/0061-be-fs-list-work-package.md` (예정) — Stage 1 BE implementation.
- `docs/ssot/security-defaults.md` — picker section 추가 (Settings hard ceiling: roots ≤ 16 entries).

### 보안

- Path traversal: ADR-0023 allowlist 정합 — `..` segment 자연 reject.
- Symlink: server 가 canonical path 로 resolve 후 allowlist 검사 — symlink 우회 차단.
- DOS: entries cap 500 + dir size cap (e.g. `read_dir` iteration 의 timeout 1s).
- 사용자 home 의 sensitive dir 노출 차단: hard blocklist (`/etc`, `/var`, `/usr`, `/`, `/root`).
- 인증: bearer/cookie middleware.

## 완료 기준

ADR Accepted 후 별 plan / work-package 의 진행 기준:

1. `GET /api/fs/list?dir=<workspace>` → 200 + workspace 안 entries.
2. allowlist 미통과 dir → 403.
3. 대용량 dir (500+) → truncated=true.
4. dot-file skip (default).
5. FE FilePickerModal — workspace + initial roots navigate, double-click file → select + close.
6. file_path 도구 spawn → picker modal 자동 open → 선택 시 그 path 로 spawn.
7. Stage 3 — `[+ Add browse root]` 의 confirm flow + toml mutation 자연 정합.

## 변경 이력

- 2026-05-17: **Draft** — 사용자 grilling 4 항목 정합. D1~D10 결정. 별 plan / 0061 BE work-package 진행 후 Accepted promote.
