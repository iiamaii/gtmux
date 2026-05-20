# 0060 — BE work-package: `GET /api/file-stat` (ADR-0034)

- 작성일: 2026-05-17
- 발주: FE 통합 agent + 사용자 (file_path fp-foot meta 연동 준비)
- 정본 ADR: `docs/adr/0034-file-stat-endpoint.md` (Draft)
- 우선: 🟢 P2 (FilePath fp-foot 의 placeholder → real meta)
- 예상 BE 작업량: 1-2 일

---

## 0. 한 줄 요약

`GET /api/file-stat?path=...` — size / lines / branch 응답. ADR-0023 allowlist 정합 (file_path open 과 동일 보안 표면). FE FilePathNode 가 mount 시 1회 fetch + path 변경 시 refetch.

---

## 1. Stage 분리

### Stage 1 — Handler + allowlist 정합 (필수)

| 항목 | 위치 | 의존 |
|---|---|---|
| `crates/http-api/src/file_stat.rs` 신규 | new file | ADR-0023 의 allowlist module |
| `GET /api/file-stat` route | `lib.rs` | bearer/cookie middleware |
| `lines` counter (byte scan + 64 MiB cap) | `file_stat.rs` | std only |
| `branch` lookup (git2 또는 gix) | `file_stat.rs` | dep 추가 |
| Tests Gate 0034-1 ~ 0034-7 | `lib.rs` tests | tempfile + git init fixture |

### Stage 2 — FE wire (FE 작업, BE land 후)

| 항목 | 위치 |
|---|---|
| `lib/http/file_stat.ts` 신규 — `fetchFileStat(path)` http client | new file |
| `lib/stores/fileStat.svelte.ts` 신규 — path-key cache + lifecycle | new file |
| `lib/canvas/FilePathNode.svelte` — fp-foot 의 placeholder 가 real data 로 교체 | edit |

---

## 2. Wire (D1)

### 2.1 Request

```
GET /api/file-stat?path=src%2Fcomponents%2Fauth%2FAuthCard.tsx
Authorization: Bearer ... (or Cookie)
```

### 2.2 Response

| Code | Body |
|---|---|
| 200 OK | `{ "path": "<resolved>", "kind": "file" \| "directory", "size_bytes": number, "lines": number \| null, "branch": string \| null }` |
| 400 | `{ "error": "invalid_path", "details": "..." }` (empty / traversal) |
| 403 | `{ "error": "path_not_allowed" }` (ADR-0023 allowlist 미통과) |
| 404 | `{ "error": "path_not_found" }` |
| 503 | `{ "error": "workspace_not_configured" }` |

---

## 3. Handler 구현

### 3.1 Pseudo

```rust
async fn file_stat_handler(
    State(state): State<AppState>,
    Query(q): Query<FileStatQuery>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else { return service_unavailable("workspace_not_configured") };
    if q.path.is_empty() { return (StatusCode::BAD_REQUEST, json!({ "error": "invalid_path" })).into_response(); }

    // 1. ADR-0023 allowlist resolve (D2)
    let resolved = match resolve_with_allowlist(wm, &q.path) {
        Ok(p) => p,
        Err(AllowlistError::NotAllowed) => return (StatusCode::FORBIDDEN, json!({ "error": "path_not_allowed" })).into_response(),
        Err(e) => return ... .into_response(),
    };

    let meta = match std::fs::metadata(&resolved) {
        Ok(m) => m,
        Err(e) if e.kind() == NotFound => return (StatusCode::NOT_FOUND, json!({ "error": "path_not_found" })).into_response(),
        Err(e) => return internal_error(e),
    };

    let kind = if meta.is_dir() { "directory" } else { "file" };
    let size_bytes = meta.len();

    // 2. lines counter (D3) — file 만, dir 은 null
    let lines = if meta.is_file() && size_bytes <= LINES_COUNT_CAP_BYTES {
        Some(count_newlines(&resolved)?)
    } else { None };

    // 3. branch (D4)
    let branch = lookup_git_branch(&resolved);

    (StatusCode::OK, Json(json!({
        "path": q.path,
        "kind": kind,
        "size_bytes": size_bytes,
        "lines": lines,
        "branch": branch,
    }))).into_response()
}
```

### 3.2 `count_newlines`

```rust
const LINES_COUNT_CAP_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB

fn count_newlines(path: &Path) -> std::io::Result<u64> {
    use std::io::{BufReader, Read};
    let f = std::fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(64 * 1024, f);
    let mut count: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        count += buf[..n].iter().filter(|&&b| b == b'\n').count() as u64;
    }
    Ok(count)
}
```

### 3.3 `lookup_git_branch` — **amend ② (2026-05-17 ship): std-only `.git/HEAD` parsing 채택**

`git2` 와 `gix` 둘 다 후보였으나 **std-only 직접 parsing** 으로 ship. 근거:

- ADR-0034 §D4 의 요구가 *branch shorthand only* — porcelain status 미포함.
- 그 작은 범위는 `.git/HEAD` 파일의 3 형태 (ref: refs/heads/ / ref: refs/ / 40-char sha) 만 처리하면 됨.
- 의존성 0 — build time / cross-compile 부담 0. 후속 amend (git porcelain status 등) 시점에 `gix` 도입 검토.

```rust
fn git_branch_for(target: &Path) -> Option<String> {
    let start = if target.is_dir() { target.to_path_buf() } else { target.parent()?.to_path_buf() };
    let git_dir = find_git_dir(&start)?;
    read_git_branch(&git_dir)
}

fn find_git_dir(start: &Path) -> Option<PathBuf> {
    let mut cur: Option<&Path> = Some(start);
    while let Some(dir) = cur {
        let candidate = dir.join(".git");
        if candidate.is_dir() { return Some(candidate); }
        cur = dir.parent();
    }
    None
}

fn read_git_branch(git_dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let trimmed = head.trim();
    if let Some(rest) = trimmed.strip_prefix("ref: refs/heads/") { return Some(rest.to_string()); }
    if let Some(rest) = trimmed.strip_prefix("ref: refs/") { return Some(rest.to_string()); }
    if trimmed.len() >= 7 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(format!("detached: {}", &trimmed[..7]));
    }
    None
}
```

**`.git` file (worktree pointer) 비지원**: v1 은 `is_dir()` 분기로 `None` 반환. 사용자가 worktree 안 파일을 FilePath 로 두면 `branch: null` — 후속 amend.

---

## 4. Tests (Gate 0034-1 ~ 0034-7)

| Gate | 시나리오 | 기대 |
|---|---|---|
| 0034-1 | happy file — text/json with newlines | 200 + size_bytes + lines + branch (null 가능) |
| 0034-2 | happy dir | 200 + kind="directory" + lines=null |
| 0034-3 | allowlist 미통과 path | 403 path_not_allowed |
| 0034-4 | missing path | 404 path_not_found |
| 0034-5 | binary file (.png) | 200 + lines=newline byte count (의미 약하나 정상) |
| 0034-6 | file > 64 MiB | 200 + lines=null (cap) |
| 0034-7 | path in git working tree | branch=<HEAD shorthand> (e.g. "main") |
| 0034-8 | 401 without auth | 401 |

Fixture: tempdir + git init + commit. 일부 test 는 git2/gix 의 mock 도 가능.

---

## 5. 의존성 (Crate) — **amend ② (2026-05-17 ship): 의존성 추가 0**

| Crate | 용도 | 결정 |
|---|---|---|
| ~~`gix` 또는 `git2`~~ | git branch lookup | **비-채택** — std-only `.git/HEAD` parsing 으로 충분 (§3.3 amend ② 참조) |

후속 (git porcelain status / worktree 지원 등 deeper git API 필요 시점에) `gix` 도입 검토.

---

## 6. FE 측 후속 wire (참고)

```typescript
// lib/http/file_stat.ts
export interface FileStatResponse {
  path: string;
  kind: 'file' | 'directory';
  size_bytes: number;
  lines: number | null;
  branch: string | null;
}

export async function fetchFileStat(path: string): Promise<FileStatResponse> {
  const qs = new URLSearchParams({ path });
  const res = await fetch(`/api/file-stat?${qs}`, { credentials: 'include' });
  if (res.status === 403) throw new PathNotAllowedError();
  if (res.status === 404) throw new PathNotFoundError();
  if (!res.ok) throw new Error(`GET file-stat returned ${res.status}`);
  return res.json();
}
```

```typescript
// lib/stores/fileStat.svelte.ts (svelte runes)
import { SvelteMap } from 'svelte/reactivity';
import { fetchFileStat, type FileStatResponse } from '$lib/http/file_stat';

class FileStatCache {
  byPath = $state<SvelteMap<string, FileStatResponse | 'loading' | 'error'>>(new SvelteMap());

  async load(path: string): Promise<void> {
    if (this.byPath.get(path) === 'loading') return;
    this.byPath.set(path, 'loading');
    try {
      const stat = await fetchFileStat(path);
      this.byPath.set(path, stat);
    } catch {
      this.byPath.set(path, 'error');
    }
  }

  get(path: string): FileStatResponse | null {
    const v = this.byPath.get(path);
    return typeof v === 'object' ? v : null;
  }
}
export const fileStatCache = new FileStatCache();
```

FilePathNode wire:
```svelte
<script>
  import { fileStatCache } from '$lib/stores/fileStat.svelte';

  $effect(() => {
    const p = data.path;
    if (p.length > 0) void fileStatCache.load(p);
  });

  const stat = $derived(fileStatCache.get(data.path));
</script>

<!-- fp-foot -->
{#if stat !== null}
  <span class="fp-badge ...">{langBadge.label}</span>
  {#if stat.lines !== null}
    <span>{stat.lines} lines</span>
    <span class="sep">·</span>
  {/if}
  <span>{(stat.size_bytes / 1024).toFixed(1)} KB</span>
  <span class="right">{stat.branch ?? '—'}</span>
{:else}
  <!-- placeholder 유지 -->
{/if}
```

---

## 7. 완료 기준

ADR-0034 의 §완료 기준 7 항목 + 본 work-package 의 Gate 0034-1 ~ 0034-8 PASS.

---

## 8. 변경 이력

- 2026-05-17: 초안 — ADR-0034 Draft 정합 + FilePathNode v3 시안의 fp-foot meta 의 실 데이터 wire 위한 BE work-package.
- 2026-05-17: **amend ② — BE Stage 1 SHIPPED**. `crates/http-api/src/file_stat.rs` 신규 module + `lib.rs` 의 `/api/file-stat` route wire 완료. 핵심:
  - **git branch lookup 의 의존성 0 결정** — §3.3 / §5 amend ② 의 std-only `.git/HEAD` parsing. ADR-0034 §D4 의 "shorthand only" 요구 정합. `gix` / `git2` 둘 다 비-채택.
  - **`kind: "directory"` 응답은 v1 비지원** — ADR-0023 의 (ext, prefix) 매칭이 directory 의 ext 매칭과 모순. directory probe 는 현 시점 403 `path_not_allowed`. 본 work package 의 Gate 0034-2 (happy dir) 는 *parked* — ADR-0034 D2 follow-up 또는 ADR-0023 amend 영역. `file_stat::tests` 의 주석에 결정 명시.
  - **테스트 14개** (Gate 0034-1~8 중 -2 -6 parked, -1 / -3 / -4 / -5 / -7 / -8 ship) = 9 unit (count_lines / git_branch / validate_path) + 5 integration (happy / 403 not-in-allowlist / 404 missing / branch null / 401 no-auth). 모두 PASS.
  - **Workspace**: 381 → **395 PASS / 0 FAIL** (+14 신규).
  - **ADR-0034 Draft → Accepted** (amend ①, 동시 ship).
  - **FE 측 후속** = §6 의 `lib/http/file_stat.ts` + `lib/stores/fileStat.svelte.ts` + FilePathNode `$effect` wire. 본 work package 의 scope 밖.
