# ADR-0034: File-stat endpoint — `file_path` fp-foot meta (lines / size / branch)

- 상태: **Accepted (2026-05-17, amend ①)** — Draft 의 D1~D7 그대로 ship
- 일자: 2026-05-17
- 결정자: agent (system-architect role) + 사용자 grilling
- 근거 grilling: 2026-05-17 — image / document / file-path 연동 준비 grilling
- 근거 plan: TBD — 본 ADR Accepted 후 별 plan / work package 분리
- 관련 ADR: ADR-0018 (Canvas Item v2 — `file_path` payload), ADR-0023 (File open policy — allowlist + path resolver), ADR-0020 (Auth — bearer/cookie)
- 관련 work package: `docs/reports/0060-be-file-stat-work-package.md` (예정)
- 관련 시안: `ref/frontend-design/components-v3.html §03` — `.shape-filepath .fp-foot` sample (lines / size / branch)

## 맥락

ADR-0018 D4 의 `file_path` item 은 `path: string` + `kind?` 만 schema. ref/components-v3.html §03 의 시안은 fp-foot 에 `[TS] · 124 lines · 4.2 KB · main` 같은 풍부한 meta 표시 — FE 측은 `langBadge` 만 wire 가능하고 lines/size/branch 는 BE 의존.

본 sprint 의 FilePathNode 변경 (fp-foot placeholder `— lines · — KB · —`) 는 visual frame 만 잡았고 실 데이터 wire 는 본 ADR 의 BE endpoint 가 ship 된 후.

본 ADR 은 `GET /api/file-stat` 의 (a) wire shape, (b) ADR-0023 allowlist 정합, (c) git lookup 범위, (d) cache / FE wire 정책 4 차원을 잠금.

## 결정 (Decisions)

### D1. Endpoint wire — `GET /api/file-stat?path=<percent-encoded>`

```
GET /api/file-stat?path=src%2Fcomponents%2Fauth%2FAuthCard.tsx

200 OK
{
  "path": "src/components/auth/AuthCard.tsx",
  "kind": "file",                  // "file" | "directory"
  "size_bytes": 4232,
  "lines": 124,
  "branch": "main"                 // null if outside any git working tree
}
```

**Error 분기**:
| Code | 의미 |
|---|---|
| 400 | `invalid_path` (빈 path, 또는 명백한 traversal `..` 등) |
| 401 | (auth middleware) |
| 403 | `path_not_allowed` (ADR-0023 allowlist 미통과 — D2) |
| 404 | `path_not_found` (file system 에 없음) |
| 503 | `workspace_not_configured` |

### D2. ADR-0023 allowlist 정합

본 endpoint 는 ADR-0023 의 `file_path` open allowlist 정책을 *그대로 재사용*. 같은 path resolver / canonicalize / prefix allowlist / extension allowlist 적용.

**근거**:
- file-stat 도 *file system read access* 이므로 open 과 동일 보안 표면.
- 사용자가 stat 만 가능하고 open 은 불가인 path 는 의미 약함 — 같은 allowlist 가 자연.

**구현**: ADR-0023 의 allowlist 모듈 (예: `crates/http-api/src/file_open/allowlist.rs`) 를 그대로 import. resolve 실패 시 403.

### D3. lines = newline byte count (no full read)

```rust
// pseudo
let f = std::fs::File::open(&resolved)?;
let mut count = 0u64;
let mut reader = std::io::BufReader::new(f);
let mut buf = [0u8; 64 * 1024];
loop {
  let n = reader.read(&mut buf)?;
  if n == 0 { break; }
  count += buf[..n].iter().filter(|&&b| b == b'\n').count() as u64;
}
```

- byte scan only — large file (수 MiB+) 도 strung memory 부담 없음.
- 64 MiB cap — 초과 시 `lines: null` 응답 (성능 위해 truncate). FE 는 "— lines" placeholder 유지.
- binary file (UTF-8 invalid) 도 newline count 동일 동작 — 잘못된 lines 가 아니라 그저 "newline 수" 의 의미.

### D4. branch = git symbolic-ref HEAD (path 의 working tree root)

```rust
// pseudo
let dir = if resolved.is_file() { resolved.parent() } else { Some(resolved.as_path()) };
let branch = git2::Repository::discover(dir)
  .and_then(|repo| repo.head())
  .and_then(|head| head.shorthand().map(String::from))
  .ok();
```

- file 의 부모 dir 부터 `Repository::discover` 로 nearest .git 찾기.
- HEAD 의 `shorthand()` (e.g. `main`, `feature/x`, `detached: abc1234`).
- working tree 외부 file (e.g. `/tmp/...`) → `branch: null`.

**비채택**: A1. git porcelain status (dirty / ahead-behind / staged) — cost 부담 (대규모 repo 1초+). 첫 ship skip — 후속 amend.

**구현 노트**:
- crate 후보: `git2` (libgit2 binding, mature). `gix` (pure Rust, lighter) 도 대안.
- Build time cost 검토 — `git2` 의 libgit2 native dependency 가 cross-compile 부담 시 `gix` 우선.

### D5. Cache / TTL

- 본 endpoint 는 *light stat + 짧은 file read + git ref*. 모두 ms 단위 — server-side cache 불필요 (default 0s).
- 단 **FE 측 cache** = `useFileStat(path)` hook 의 in-memory map (path → result, no TTL — path 변경 시 무효화).
- 후속 amend 가능성: server-side LRU cache (path → result) — 같은 path 의 polling 부담 시 도입.

### D6. Auth

`/api/*` middleware 의 bearer/cookie. 다른 endpoint 와 동일.

### D7. FE wire 정책

FilePathNode 의 lifecycle:
1. **mount 시 1회 fetch** — `path.length > 0` 이면 `useFileStat(data.path)`.
2. **path 변경 시 refetch** — `$effect` 으로 data.path 변경 감지 → 새 fetch.
3. **fp-foot rendering**:
   - 성공: `[BADGE] · {lines} lines · {size}KB · {branch ?? "—"}`
   - 404 / 403: placeholder `— lines · — KB · —` (현재 placeholder 패턴 유지).
   - in-flight: placeholder 유지 (no spinner — light fetch, < 100ms 일반적).
4. **polling X** — file 의 변경은 git status 처럼 *실시간* 의미가 아님. 사용자 수동 refresh (e.g. F5 또는 다음 attach) 시 자연 재 fetch. 후속 amend 로 fs notify watcher 검토 가능.

## 어휘 매트릭스

- **File-stat** = file 의 size / lines / git branch meta (read-only).
- **Allowlist** = ADR-0023 의 file_path open 권한 정책.

## 대안 검토

본 결정의 거부 대안:
- A1. git status (dirty / ahead-behind) 포함 — cost 부담 첫 ship 비채택, 후속 amend.
- A2. lines 계산을 read_to_string 으로 — 대용량 file memory 부담 부적절.
- A3. ADR-0023 allowlist 우회 (별 정책) — 보안 표면 갈라짐, 거부.
- A4. polling / fs notify watcher — 첫 ship 의 scope creep. 후속 amend.

## 영향

### Code

**Backend**:
- 새 module `crates/http-api/src/file_stat.rs` — handler + lines counter + git lookup.
- 새 dependency: `git2` 또는 `gix` (work-package 에서 비교 후 결정).
- `WorkspaceManager` 또는 별 module 의 path resolver 재사용 (ADR-0023 의 allowlist).

**Frontend**:
- `lib/http/file_stat.ts` 신규 — `fetchFileStat(path)` http client.
- `lib/canvas/FilePathNode.svelte` — `useFileStat(data.path)` derived store (svelte runes pattern). fp-foot 의 placeholder 가 실 데이터로 자동 교체.
- 또는 `lib/stores/fileStat.svelte.ts` 신규 — path-key cache + fetch lifecycle.

### ADR

- ADR-0023 amend 가능성 — file_stat 도 같은 allowlist 적용 명시 entry (현 ADR-0023 은 open 의 정책만).

### Docs

- `docs/reports/0060-be-file-stat-work-package.md` (예정) — Stage 분리 + Gate test 정의.
- `plan-0011` 또는 별 plan — FilePathNode 의 fp-foot wire FE-after-BE-ship.

### 보안

- Path traversal: ADR-0023 allowlist 정합 — `..` / absolute path 등 reject.
- DOS: lines 계산의 64 MiB cap, git ref lookup 의 적정 timeout (e.g. 1 초).
- 인증: bearer/cookie middleware.

## 완료 기준

본 ADR Accepted 후 별 plan / work-package 의 진행 기준:

1. `GET /api/file-stat?path=Cargo.toml` (workspace root) — 200 + size / lines / branch.
2. allowlist 미통과 path → 403.
3. file missing → 404.
4. binary file (e.g. `.png`) → lines = `newline byte count` (의미 약하나 정상 응답).
5. workspace root 가 git repo 아님 → `branch: null`.
6. FE FilePathNode 의 fp-foot 가 실 데이터 표시 — placeholder 자동 교체.
7. path 변경 시 (사용자 inline-edit) refetch 자연 동작.

## 변경 이력

- 2026-05-17: **Draft** — 사용자 grilling 정합 (size + lines + branch). D1~D7 결정. 별 plan / 0060 BE work-package 진행 후 Accepted promote.
- 2026-05-17: **Accepted (amend ①, ship)**. BE 구현 land — 0060 work package 정합. 핵심 결정:
  - **D4 의 git lookup 구현 = std-only `.git/HEAD` parsing** (`crates/http-api/src/file_stat.rs::find_git_dir` + `read_git_branch`). 0060 §3.3 의 git2/gix 선택 대안 비-채택 — ADR §D4 의 "shorthand only" 요구가 작아 의존성 0 의 직접 parsing 으로 충분. `.git` *file* (worktree pointer) 은 v1 비지원 — `is_dir()` 분기로 `None` 반환. 후속 amend 시 검토.
  - **D2 의 allowlist 정합**: `state.file_open.allowlist.read().await.check(&canonical)` 직접 호출. `validate_path` 는 `file_open::handlers::validate_path` 와 동일 의미의 module-local 함수.
  - **D1 의 `kind: "directory"` 응답**은 *current behaviour 가 403* — ADR-0023 의 allowlist 가 (ext, prefix)-shape 라 directory 의 ext 매칭 안 됨. 본 amend 의 v1 scope 는 file-only. directory carve-out 은 follow-up (ADR-0034 D2 amend ② 또는 ADR-0023 amend 영역). 별 Gate 로 parked (`file_stat::tests` 의 주석 참조).
  - **D3 의 64 MiB cap** — `LINES_SCAN_MAX_BYTES = 64 * 1024 * 1024` 상수. file_size > cap 이면 `lines: null`.
  - **테스트**: 9 unit (count_lines, git_branch, validate_path 의 happy/edge 케이스) + 5 integration (happy / 403 / 404 / branch null / 401). workspace 381 → **395 PASS / 0 FAIL** (+14 신규).
  - **Route wire**: `/api/*` 의 file-path 라우트들 옆에 `.route("/api/file-stat", get(file_stat::file_stat_handler))` — bearer middleware 자동 401.
  - work package = `docs/reports/0060-be-file-stat-work-package.md` (해당 doc 의 §3.3 / §5 amend ② 에서 git lookup 의 std-only 선택 명시).
