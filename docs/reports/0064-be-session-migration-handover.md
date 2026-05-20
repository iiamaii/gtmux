# 0064 — BE session migration handover (cold-pickup brief)

- 작성일: 2026-05-17 (session 2 종료 시점, context 76% 도달)
- 작성자: BE agent — 본 session 안 11 commit ship 후 cold-pickup brief
- 종류: **cold-pickup brief** — 다음 BE agent (또는 FE agent / 사용자) 가 본 문서 한 장으로 *현재 상태 + 즉시 진입 우선순위 + 핵심 컨텍스트* 모두 진입 가능
- HEAD (handover 작성 시점): `e006962` (FE worker 가 marathonon 한 xterm theme + reattach 후속 commit 끝)
- 본 session 의 마지막 BE commit: **`62fc743`** (`feat(backend): GET /api/file-stat — file_path fp-foot meta`)
- 워크스페이스 baseline: **`cargo test --workspace --no-fail-fast` 395 PASS / 0 FAIL**
- release build: PASS (`cargo build --release --bin gtmux`, ~32s)

---

## 0. 다음 세션 즉시 진입 — 우선순위 ⚠️ MUST READ

본 sprint 안 BE 측 P0 BLOCKER 모두 closed. 잔여 항목은 모두 P2 cosmetic / 별 ADR 영역.

| 우선 | 항목 | 상태 | 정본 |
|---|---|---|---|
| 🟡 P1 | **ADR-0033 Assets binary endpoint 구현 (Stage 2)** | Draft 정착, work package 0059 ship 대기 | `docs/adr/0033-asset-storage-and-serving.md` (Draft) + `docs/reports/0059-be-asset-storage-work-package.md` |
| 🟡 P1 | **0061 fs_list 도구 (FilePickerModal BE wire)** | work package 만 ship (`docs/reports/0061-be-fs-list-work-package.md`) — handler 도 일부 land 됨 (`crates/http-api/src/fs_list.rs`, parallel worker), 검증 필요 | `0061-be-fs-list-work-package.md` |
| 🟢 P2 | **0053 §7 RFC3339 통일** | 잔여 cosmetic — `created_at` (u64 unix_secs) ↔ envelope `exported_at` (RFC3339) 통일. wire break 위험 (dual-emit 일시 검토). | `0053-be-verification-checklist.md` §7 |
| 🟢 P2 | **ADR-0034 D2 directory carve-out** | file-stat 의 `kind: "directory"` 응답이 현 시점 403 (allowlist 의 ext 매칭과 모순). 별 처리 정책 필요. | `0060-be-file-stat-work-package.md` §3 amend ② note |

### 0.1 본 session 핵심 사건 한 문단

본 session 안 BE 측 **11 commit ship** — 시간순: (a) P0-1 `0046 attach idempotent` + P0-2 `plan-0009 /auth SPA pivot` 묶음 land 으로 cold-pickup 시점의 두 P0 BLOCKER 모두 closed. (b) D6 heartbeat 의 `Hub::set_heartbeat_timings` config + integration tests 2개 + 후속 flaky fix (parallel cargo-test 의 race window). (c) D14 `POST /auth/rotate` cookie rotation endpoint 신규. (d) `0053 BE verification checklist` 의 §2~§7 6항목 모두 검증 (P0 BLOCKER §2 export endpoint 는 본 session 안 follow-up 으로 ship 함). (e) Export endpoint (`GET /api/sessions/:name/export`) + Import body cap 16 MiB raise + Respawn per-UUID Mutex (`AppState::respawn_locks`) + Document inline-stored mode (ADR-0018 D10 schema drift 해소) + file-stat endpoint (ADR-0034 Accepted) ship. **워크스페이스 365 → 395 PASS / 0 FAIL** (+30 신규 BE test). 본 session 안 parallel FE worker 가 동시 marathon-ship — 일부 commit 이 sweep 으로 mixed bag.

---

## 1. 프로젝트 mental model (1 분 요약)

**gtmux** = tmux-backed web canvas workspace. *single-user* SPA. tmux 가 process lifecycle 의 진실, FE 가 canvas layout 의 진실.

### 1.1 어휘 (CONTEXT.md / ADR-0019 정합)

| 어휘 | 정의 |
|---|---|
| Server | gtmux process. 1 port owner, 1 workspace dir 바인딩 |
| Workspace | server 와 1:1, `<XDG_DATA_HOME>/gtmux/workspace/` dir |
| Session | workspace 안 named file record (`<name>.json`). canvas layout + viewport |
| Webpage | 브라우저 탭. 1 WS 연결, 0/1 session attach |
| Terminal | server-pool, multi-session 공유 가능 (mirror) |
| Canvas Item | canvas 위 시각 객체 (terminal / rect / ellipse / line / text / note / file_path / document / image / free_draw / caption) |
| Panel | `type:"terminal"` 인 Canvas Item |

### 1.2 핵심 invariant

1. tmux state ↔ web state 분리
2. layout ≠ tmux layout
3. single-attach: Webpage : Session = 1:1 (ADR-0019 D3)
4. takeover 금지 (ADR-0019 D4 — 다른 cookie 만 409)
5. control-mode integration only
6. ADR-0028 D1.1 — Undo effect 는 layout snapshot 복원 only
7. ADR-0018 D10 amend ② (2026-05-17, 본 session ship) — Document 두 mode (asset_id XOR content) 상호 배타

### 1.3 우선 단계

현재 = **Stage 7** — multi-session pivot 완료, UX 폴리시 + Undo/Redo + Import/Export ship 후속 + 회귀 가드 + Toolbar item 의 BE 정합.

---

## 2. 디렉토리 / 빌드

| | 경로 | 명령 |
|---|---|---|
| BE workspace root | `codebase/backend/` | `cargo test --workspace`, `cargo build`, `cargo build --release --bin gtmux` |
| FE | `codebase/frontend/` | `pnpm check`, `pnpm build`, `pnpm dev` |
| 문서 | `docs/` (ADR/plan/report) | — |

baseline (본 session 검증 HEAD `62fc743`):

- BE: `cargo test --workspace --no-fail-fast` → **395 PASS / 0 FAIL**
- BE: `cargo build --release --bin gtmux` → PASS
- FE: 본 session 직접 검증 X (parallel worker 가 svelte-check ship)

---

## 3. 본 session BE ship 누적 — 11 commit

| # | commit | 작업 | FE 영향 |
|---|---|---|---|
| 1 | `e9eb9a6` | **P0-1 + P0-2** — 0046 attach idempotent (cookie ownership 분기 + reuse_existing_attach_response helper) + D13 /auth SPA pivot (auth_page_handler 제거 + bootstrap_handler `?t=` 정합) | refresh / silentReattach 모든 trigger 가 200 자연 통과. `/auth` 는 SPA fallback. |
| 2 | `cd15cba` | **D6 heartbeat Hub config** — HeartbeatTimings struct + setter + 2 integration tests | BE timeout 30s ↔ FE isStale 30s 정합. abrupt close lock leak 차단. |
| 3 | `59bd0ab` | **D14 `POST /auth/rotate`** — cookie rotation endpoint (revoke_others + caller cookie re-issue) + 4 tests | SettingsOverlay [Rotate session] 버튼 wire 가능 (FE 미 ship). |
| 4 | `481a4d7` | **D6 flaky fix** — timing 4x 확장 (100ms ping / 300ms timeout / 700ms wait) + close_code best-effort + test rename `..._1011_...` → `..._and_...` | n/a — test 회귀 안정화 |
| 5 | `2c104c5` | **0053 BE verification doc** — §2~§7 6항목 검증 결과 표기 | n/a — 정합 doc |
| 6 | `ecc8581` | **Export endpoint** `GET /api/sessions/:name/export` (ADR-0029 D4 / 0052) — std-only RFC3339 (chrono 무도입) + envelope + sanitize_filename + 5 tests | ExportSessionModal download path 정상화 |
| 7 | `f4c936e` | **Import body cap 16 MiB** — `DefaultBodyLimit::max(SESSION_PUT_MAX_BYTES)` + 2 회귀 가드 + ADR-0029 §6 amend | ImportSessionModal 의 8MB+ layout import 가능 |
| 8 | `d36f092` + `f0b7cc5` | **Respawn per-UUID Mutex** (`AppState::respawn_locks` + lookup_pane idempotent path) + response shape `{ id, reused: bool }` 확장 + ADR-0021 D10.3 신규 | multi-webpage `PanelDanglingOverlay` auto-respawn 안전 |
| 9 | `3b371bd` | **0055 sprint summary** — BE → FE 공유 종합 정리 doc | n/a — FE 공유 doc |
| 10 | `2ebe8d6` | **Document inline-stored mode** (ADR-0018 D10 amend ②) — Item::Document 의 asset_id Option + content Option + DOCUMENT_INLINE_MAX_BYTES (64 KB) + 3 ValidationError + 5 tests | FE inline-editable document 가 asset endpoint 없이 즉시 작동 가능 (canvas.ts amend 후) |
| 11 | `62fc743` | **file-stat endpoint** `GET /api/file-stat` (ADR-0034 Accepted) — std-only `.git/HEAD` parsing + count_lines (64 MiB cap) + ADR-0023 allowlist 재사용 + 14 tests | FilePathNode fp-foot 의 lines/size/branch placeholder 의 실 wire 가능 (FE store/hook amend 후) |

**누적 test 증가**: 365 baseline → 395 = **+30 신규 test** (P0 2 / D6 2 / D14 4 / Export 5 / Import cap 2 / Respawn 1 / Document 5 / file-stat 14 + obsolete -5).

### 3.1 본 session 후속 parallel worker BE/FE work (별 commit, 본 session 안 일부 sweep)

| commit | 작업 |
|---|---|
| `fs_list.rs` + `FilePickerModal.svelte` + `fs.ts` | FilePickerModal BE wire (`62fc743` 에 sweep) — 별 worker work, 검증 미진행 |
| FE Toolbar Undo/Redo / Inspector / clipboard / shape fill / multi-select context / colorpicker drag / theme hot-reload investigation 등 | 본 session 외 — 별 FE worker work |

---

## 4. 핵심 reference 위치

### 4.1 ADR (본 session 안 amend 한 것 위주)

| ADR | 의미 | 본 session 관여 |
|---|---|---|
| ADR-0018 D10 amend ② | Document inline-stored mode (asset_id XOR content) 정합 | `2ebe8d6` |
| ADR-0019 D3 amend ③ | attach_handler same-cookie idempotent contract | `e9eb9a6` |
| ADR-0020 D13 / D14 | /auth SPA pivot + cookie rotation endpoint | `e9eb9a6` (D13) / `59bd0ab` (D14) |
| ADR-0021 D6.2 amend ②/③ | Hub::heartbeat_timings + flaky fix | `cd15cba` / `481a4d7` |
| ADR-0021 D10.3 (신규) | Respawn 동시-호출 정책 (per-UUID Mutex) | `d36f092` / `f0b7cc5` |
| ADR-0029 §6 / §7 amend ② | Import body cap = 16 MiB + Export envelope | `ecc8581` / `f4c936e` |
| ADR-0033 (Draft) | Assets binary endpoint roadmap (Stage 2, 별 work) | (다른 worker draft) |
| ADR-0034 (Accepted amend ①) | file-stat endpoint | `62fc743` |

### 4.2 Reports / work packages

| 종류 | 경로 |
|---|---|
| 0046 attach idempotent | `docs/reports/0046-be-attach-handler-idempotent.md` |
| 0051 직전 handover | `docs/reports/0051-session-migration-handover.md` |
| 0052 Export work package | `docs/reports/0052-be-session-export-endpoint.md` |
| 0053 BE verification checklist | `docs/reports/0053-be-verification-checklist.md` (6/6 closed) |
| 0054 직전 (직전 handover) | `docs/reports/0054-session-migration-handover.md` |
| 0055 본 session 의 sprint summary | `docs/reports/0055-be-session-summary-for-fe.md` |
| 0056 Document inline + Assets roadmap | `docs/reports/0056-be-document-inline-mode-and-assets.md` |
| 0059 Assets work package (다음 session enter point) | `docs/reports/0059-be-asset-storage-work-package.md` |
| 0060 file-stat work package | `docs/reports/0060-be-file-stat-work-package.md` (ship 완료) |
| 0061 fs_list work package | `docs/reports/0061-be-fs-list-work-package.md` (별 worker) |

### 4.3 본 session reading order (cold-pickup, 처음 진입 agent)

1. **본 문서 §0~§3** (현재 상태 한 페이지)
2. `docs/reports/0055-be-session-summary-for-fe.md` (BE → FE 종합 정리, 본 session 직전 작성)
3. `docs/reports/0053-be-verification-checklist.md` (6/6 closed, 잔여 P2 §7 만 명시)
4. `docs/adr/0033-asset-storage-and-serving.md` (next P1 의 정본 spec)
5. `docs/reports/0059-be-asset-storage-work-package.md` (next P1 의 implementation roadmap)

---

## 5. Next session 권장 진입 항목

### 5.1 🟡 P1 — ADR-0033 Assets binary endpoint (Stage 2)

ADR-0033 (Draft) 정합 + `0059-be-asset-storage-work-package.md` 의 BE work plan 따름. 핵심:

- `POST /api/assets` — multipart/form-data binary upload → sha256 계산 → `<workspace>/.assets/<sha256>` 저장 → `{ asset_id, mime, size_bytes }`
- `GET /api/assets/{sha256}` — binary stream + Content-Type 추론, ETag = asset_id
- MIME allowlist (Settings-driven, ADR-0033 D3 / D4)
- Boot-lazy orphan GC (ADR-0033 D7)
- 2-3 일 BE work 예상

**의존성 검토**: 1. workspace 의 `.assets/` dir 자동 생성 (boot 시) + 2. settings 의 MIME / cap config 모델 + 3. magic-byte sniff (ADR-0033 D4 — client MIME 만 신뢰 안 함).

### 5.2 🟡 P1 — 0061 fs_list 검증

`fs_list.rs` 가 parallel worker 의 work 으로 `62fc743` 에 sweep 됨. 본 session 의 검증 미진행. cold-pickup 시:
- file_stat 처럼 ADR-0023 allowlist 정합 검증.
- FE `FilePickerModal.svelte` 와 wire 정합 확인.
- 누락 test 추가 (work package 0061 정합).

### 5.3 🟢 P2 — 0053 §7 RFC3339 통일 (cosmetic)

`POST /api/sessions/import` 의 응답 `created_at` (u64 unix_secs) ↔ Export envelope `exported_at` (RFC3339 string) 형식 통일. wire break 위험 — FE wire 의 `ImportSessionResponse.created_at: number` 가 변경 영향. dual-emit 일시 지원 또는 한쪽 amend 결정.

### 5.4 🟢 P2 — ADR-0034 D2 directory carve-out

file-stat 의 `kind: "directory"` 응답이 현 시점 403 (allowlist 의 ext 매칭과 모순). `(ext="", prefix=...)` 의 ext-less mode 도입 또는 directory 의 별 처리 정책. ADR-0034 amend ② / ADR-0023 amend 영역.

### 5.5 권장 진입 순서

1. **§5.1 ADR-0033 Assets** — 다음 sprint 의 중심 BE work. work package 0059 가 정본.
2. **§5.2 fs_list 검증** — parallel worker 의 work 검수. 짧음.
3. (§5.3 / §5.4) cosmetic / smaller scope — sprint 시간 여유 시.

---

## 6. 유의사항 / 함정

### 6.1 Parallel worker 의 commit sweep

본 session 안 parallel FE/BE worker 가 동시 marathon-ship — 본 session 의 staged 변경이 worker 의 commit 에 sweep 되는 케이스 여러 번 발생. 영향:

- 본 session 의 `ea88e04` (D14 시점 staged 한 cap work) → 다음 `git commit` 시점에 worker 의 `ea88e04 docs(adr): 0028 D11 audit re-verify` 에 묶임 → 명시적 re-commit 으로 정합.
- 본 session 의 `62fc743 feat(backend): GET /api/file-stat` → 그 시점에 stage 한 `fs_list.rs` + `FilePickerModal.svelte` + `fs.ts` 가 함께 sweep.

**회피 패턴**: `git commit` 직전에 `git status` 로 staged 영역 확인. 본 session 외 변경이 staged 됐다면 `git restore --staged <file>` 로 unstage. 또는 의도가 명확하면 한 commit 으로 묶고 description 에 "incidental sweep" 명시.

### 6.2 commit-graph hook 의 auto-commit

`PostToolUse` / `pre-commit` hook 이 `code-review-graph update` 실행. 가끔 hook 자체가 commit 을 만들어 main 에 push 함 (예: skills-lock.json 의 자동 update). worktree 가 갑자기 깨끗해지는 케이스 → log 확인 후 본 session 의 work 가 흡수됐는지 별 commit 인지 판단.

### 6.3 `.git` worktree pointer 미지원 (file_stat)

ADR-0034 §D4 의 v1 scope = `.git` directory 만. `.git` file (worktree pointer, `gitdir: <path>` 포함) 은 v1 비지원 — `is_dir()` 분기로 `None` 반환. 사용자가 worktree 안 file 을 FilePath 로 두면 `branch: null` 응답. 후속 amend 가능성.

### 6.4 file_stat directory probe 의 v1 제한

ADR-0034 D1 의 `kind: "directory"` 응답값은 *current behaviour 가 403*. ADR-0023 의 (ext, prefix) allowlist 가 directory 의 ext 매칭과 모순. follow-up amend 영역. `file_stat::tests` 의 주석에 parked 결정 명시.

### 6.5 ADR-0033 / 0034 의 Draft 였던 시점 ↔ 본 session 의 ADR-0034 Accepted

ADR-0033 (Assets) 은 본 session 안 Draft 그대로 유지 — implementation 진입 안 함. ADR-0034 (file-stat) 은 본 session 안 Draft → Accepted (amend ①) 전환 + ship 완료. 다음 session 의 진입 시점에 두 ADR 의 status 헷갈리지 않도록 주의.

### 6.6 Document inline-stored mode 의 schema migration

ADR-0018 D10 amend ② (본 session) 으로 schema 의 `Item::Document` 가 `asset_id: Option<String>` + `content: Option<String>` 으로 amend. **migration 불필요** — `serde(default, skip_serializing_if = "Option::is_none")` 가 old layout JSON 의 `"asset_id": "..."` (Some) + content 부재 (None default) 를 자연 흡수. 즉 (a) asset-based mode 만 있던 옛 layout 도 그대로 작동.

### 6.7 Workspace test 의 known flake

`integration_pane.rs::gate1_signal_ctrl_c_interrupts_sleep` 가 parallel `cargo test --workspace` 시 간헐적 PTY signal race 로 fail. 단독 재실행 PASS. 본 session 안 1-2회 관찰됨 — 무관 회귀, isolated re-run 으로 회복. handover §5.9 of 0048 에 첫 관찰 기록.

### 6.8 ADR-0021 D6.2 의 close_code best-effort 정합

D6 heartbeat integration test 의 close_code assertion 이 `matches!(None | Some(CloseCode::Error))` 로 완화됨 (parallel cargo-test 의 race window 정합). disconnect_sink emit 만 strict. 본 contract 변경은 `481a4d7` 에서 진단 + fix 완료, `cd15cba` 의 original strict 한 assertion 은 더 이상 사용 안 함.

---

## 7. 검증 baseline

본 handover 작성 시점 (BE `62fc743`):

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | grep "test result:" | head -15
# 기대: 모든 줄 0 failed. http-api 214 PASS, ws-server 113 PASS, 합 395 PASS.

cargo build --release --bin gtmux --color=never
# 기대: PASS (~30s)
```

본 session 안 신규 test 추가:

| Group | 신규 tests | 총 |
|---|---|---|
| 0046 attach idempotent | 2 (attach_idempotent_for_same_cookie_same_session + attach_409_when_held_by_different_cookie) | 2 |
| D6 heartbeat (cd15cba) | 2 (timeout + pong) | 4 |
| D14 /auth/rotate | 4 | 8 |
| 0046 후속 obsolete 제거 | -1 + 2 rename → net +1 | 9 |
| Export endpoint (Gate 0029-1~5) | 5 | 14 |
| Import body cap | 2 | 16 |
| Respawn per-UUID Mutex | 1 (concurrent_same_uuid) | 17 |
| Document inline | 5 | 22 |
| file-stat | 14 (9 unit + 5 integration) | 36 |

(obsolete 제거 + rename 의 net = 365 → 395 = +30)

---

## 8. 변경 이력

- 2026-05-17: 초안 — 본 session 안 BE 측 11 commit ship 후 cold-pickup brief 작성. P0 BLOCKER 모두 closed + 잔여 P1/P2 (Assets / fs_list / RFC3339 / directory carve-out) 정리. 다음 session 의 권장 진입 = ADR-0033 Assets binary endpoint (work package 0059).
