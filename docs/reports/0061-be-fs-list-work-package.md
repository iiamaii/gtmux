# 0061 — BE work-package: `GET /api/fs/list` + picker roots (ADR-0035)

- 작성일: 2026-05-17
- 발주: FE 통합 agent + 사용자 (file_path 도구 의 file system picker UX)
- 정본 ADR: `docs/adr/0035-file-system-picker.md` (Draft)
- 우선: 🟢 P2 (file_path 의 path 선택 UX 완성)
- 예상 BE 작업량: 1-2 일 (Stage 1) + 2-3 일 (Stage 3 picker root mutation)

---

## 0. 한 줄 요약

`GET /api/fs/list?dir=...` (entries + truncated flag) + ADR-0023 allowlist 정합 + `[picker.roots]` toml section 도입. FE 측 picker modal 의 BE backend.

---

## 1. Stage 분리

### Stage 1 — Handler + initial roots (필수, MVP)

| 항목 | 위치 | 의존 |
|---|---|---|
| `crates/http-api/src/fs_list.rs` 신규 | new file | ADR-0023 allowlist module |
| `GET /api/fs/list` route | `lib.rs` | bearer/cookie middleware |
| `picker_initial_roots(workspace)` helper | `crates/http-api/src/file_open/allowlist.rs` 확장 | ADR-0023 toml parser |
| Entries sort + dot-filter + cap (500) | `fs_list.rs` | std only |
| Tests Gate 0035-1 ~ 0035-8 | `lib.rs` tests | tempdir fixture |

### Stage 2 — FE picker modal (FE 작업, BE Stage 1 land 후)

| 항목 | 위치 |
|---|---|
| `lib/http/fs.ts` 신규 — `listDir(dir)` | new file |
| `lib/chrome/FilePickerModal.svelte` 신규 — D5 UI form | new file |
| `lib/canvas/Canvas.svelte` onpaneclick — `tool === 'file_path'` 분기 가 picker trigger | edit |
| `lib/canvas/FilePathNode.svelte` 더블 클릭 → picker (InlineEdit 제거) | edit |

### Stage 3 — 사용자 root 동적 추가 (P2+, separate work-package)

| 항목 | 위치 |
|---|---|
| `POST /api/fs/allowlist/picker-root { path }` handler | `fs_list.rs` 또는 `file_open/allowlist.rs` |
| toml `[picker.roots]` mutation (atomic write) | ADR-0023 의 toml writer 확장 |
| Server-side path 검증 (hard blocklist) | new |
| FE [+ Add browse root] flow | `FilePickerModal.svelte` |
| Settings UI 의 picker roots 리스트 | `lib/chrome/SettingsDialog.svelte` 또는 별 component |

---

## 2. Stage 1 — Handler 구현

### 2.1 Wire (D3)

```
GET /api/fs/list?dir=<percent-encoded-absolute-path>
Authorization: Bearer ... (or Cookie)

200 OK
{
  "dir": "/Users/foo/repo/src",
  "parent": "/Users/foo/repo" | null,
  "entries": [
    { "name": "auth", "kind": "directory", "size_bytes": null, "mtime_unix": 1779000000 },
    { "name": "AuthCard.tsx", "kind": "file", "size_bytes": 4232, "mtime_unix": 1779000123 }
  ],
  "total": 24,
  "truncated": false
}
```

### 2.2 Handler 흐름

```rust
async fn list_handler(
    State(state): State<AppState>,
    Query(q): Query<FsListQuery>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else { return service_unavailable("workspace_not_configured") };

    // 1. dir 정규화 + ADR-0023 allowlist 검증
    let resolved = match resolve_with_picker_allowlist(wm, &q.dir) {
        Ok(p) => p,
        Err(PickerError::Invalid) => return (StatusCode::BAD_REQUEST, json!({ "error": "invalid_dir" })).into_response(),
        Err(PickerError::NotAllowed) => return (StatusCode::FORBIDDEN, json!({ "error": "dir_not_allowed" })).into_response(),
        Err(PickerError::NotFound) => return (StatusCode::NOT_FOUND, json!({ "error": "dir_not_found" })).into_response(),
    };

    if !resolved.is_dir() {
        return (StatusCode::BAD_REQUEST, json!({ "error": "not_a_directory" })).into_response();
    }

    // 2. entries 수집 + filter + sort
    let show_hidden = state.settings.read().await.picker.show_hidden;
    let mut entries: Vec<FsEntry> = std::fs::read_dir(&resolved)?
        .filter_map(|e| e.ok())
        .filter(|e| show_hidden || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some(FsEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                kind: if meta.is_dir() { Kind::Directory } else { Kind::File },
                size_bytes: if meta.is_file() { Some(meta.len()) } else { None },
                mtime_unix: meta.modified().ok()
                    .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()),
            })
        })
        .collect();
    entries.sort_by(|a, b| match (a.kind, b.kind) {
        (Kind::Directory, Kind::File) => std::cmp::Ordering::Less,
        (Kind::File, Kind::Directory) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    let total = entries.len();
    let truncated = total > MAX_ENTRIES;
    entries.truncate(MAX_ENTRIES);

    // 3. parent — root 면 null
    let parent = resolved.parent()
        .filter(|p| !is_root(p))
        .map(|p| p.display().to_string());

    (StatusCode::OK, Json(json!({
        "dir": resolved.display().to_string(),
        "parent": parent,
        "entries": entries,
        "total": total,
        "truncated": truncated,
    }))).into_response()
}

const MAX_ENTRIES: usize = 500;
```

### 2.3 Allowlist resolve (D2.1)

```rust
fn resolve_with_picker_allowlist(wm: &WorkspaceManager, dir: &str) -> Result<PathBuf, PickerError> {
    if dir.is_empty() || dir.contains("..") {
        return Err(PickerError::Invalid);
    }
    let p = PathBuf::from(dir);
    let canonical = p.canonicalize().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => PickerError::NotFound,
        _ => PickerError::Invalid,
    })?;

    // Hard blocklist (security floor)
    for blocked in HARD_BLOCK_PREFIXES {
        if canonical.starts_with(blocked) {
            return Err(PickerError::NotAllowed);
        }
    }

    // Initial roots check
    if canonical.starts_with(wm.path()) { return Ok(canonical); }
    for root in picker_roots()? {
        if canonical.starts_with(&root) { return Ok(canonical); }
    }

    Err(PickerError::NotAllowed)
}

const HARD_BLOCK_PREFIXES: &[&str] = &["/etc", "/usr", "/var", "/root", "/sys", "/proc", "/dev"];
```

### 2.4 Initial roots (Stage 1)

ADR-0023 toml 의 `[picker.roots]` section (D2.1 amend). Stage 1 의 fallback = `[]` (사용자 ADR-0023 D2.1 amend 후 추가). workspace 는 항상 root.

```toml
# ${XDG_CONFIG_HOME}/gtmux/file-open-allowlist.toml

[[entries]]
ext = "ts"
prefix = "/Users/foo/repo"

[picker]
roots = [
  "/Users/foo/repo",
  "/Users/foo/notes",
]
```

`picker_roots()` 함수 = toml 의 `[picker.roots]` 만 read. file-open 의 `[[entries]]` 와 *별 정책* (D2.1).

### 2.5 Tests (Gate 0035-1 ~ 0035-8)

| Gate | 시나리오 | 기대 |
|---|---|---|
| 0035-1 | happy — workspace root list | 200 + entries (dir 먼저, 이름순) + total + truncated=false |
| 0035-2 | nested dir within workspace | 200 + parent !== null |
| 0035-3 | dir outside allowlist | 403 dir_not_allowed |
| 0035-4 | hard-block prefix `/etc` | 403 |
| 0035-5 | traversal `..` | 400 invalid_dir |
| 0035-6 | non-existent path | 404 dir_not_found |
| 0035-7 | dir with 600 entries | 200 + entries.len()==500 + truncated=true + total=600 |
| 0035-8 | dot-files default skip | entries 에 `.git` 등 없음 |
| 0035-9 | 401 without auth | 401 |

---

## 3. Stage 2 — FE picker modal (FE 작업 참고)

```typescript
// lib/http/fs.ts
export interface FsEntry {
  name: string;
  kind: 'file' | 'directory';
  size_bytes: number | null;
  mtime_unix: number | null;
}

export interface FsListResponse {
  dir: string;
  parent: string | null;
  entries: FsEntry[];
  total: number;
  truncated: boolean;
}

export async function listDir(dir: string): Promise<FsListResponse> {
  const qs = new URLSearchParams({ dir });
  const res = await fetch(`/api/fs/list?${qs}`, { credentials: 'include' });
  if (res.status === 403) throw new DirNotAllowedError();
  if (res.status === 404) throw new DirNotFoundError();
  if (!res.ok) throw new Error(`GET fs/list returned ${res.status}`);
  return res.json();
}
```

FilePickerModal 의 layout 은 ADR-0035 D5 참조. SvelteFlow modal stack 위 별 Modal 컴포넌트.

---

## 4. Stage 3 — `POST /api/fs/allowlist/picker-root`

```
POST /api/fs/allowlist/picker-root
Content-Type: application/json

{ "path": "/Users/foo/notes" }

  ↓

201 Created
{ "path": "/Users/foo/notes", "added": true }

  | 400 invalid_path / hard_blocked
  | 409 already_present
```

흐름:
1. `path` canonical resolve + hard blocklist check.
2. ADR-0023 toml 의 `[picker.roots]` 에 atomic append.
3. 응답 = 추가된 path.

---

## 5. 의존성 (Crate)

| Crate | 용도 |
|---|---|
| 기존 | toml (이미 ADR-0023 의 allowlist 가 사용) — `[picker.roots]` 부분만 추가 parser/writer |
| 기존 | serde (entries serialize) |

---

## 6. 완료 기준

ADR-0035 의 §완료 기준 7 항목 + 본 work-package 의 Gate 0035-1 ~ 0035-9 PASS.

---

## 7. 변경 이력

- 2026-05-17: 초안 — ADR-0035 Draft 정합. Stage 1 (BE handler + initial roots) + Stage 2 (FE) + Stage 3 (사용자 root 동적 추가).
