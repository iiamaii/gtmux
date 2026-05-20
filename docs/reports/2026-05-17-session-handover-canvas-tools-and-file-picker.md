# Session Handover — 2026-05-17 — Canvas tool 확장 + File picker + Session delete UI

> 이 문서는 `session-handover` skill 로 생성된 session 인수인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-17 (저녁, ColorPicker batch + canvas tool 확장 batch 이후)
> - 생성 session 의 마지막 커밋: `839393d` (feat(file-path): picker-only path 입력 + Settings.picker_show_hidden + show_hidden toggle)
> - 본 session 주제: 0054 cold-pickup brief 진입 후 — (a) 0053 BE 의존 wire 검증, (b) Canvas item 도구 확장 (image / document / free_draw + cursor=center + ghost), (c) Session delete UI (ADR-0019 D10.1), (d) v3 시안 차용 (FilePath / Image / Document), (e) Document inline edit wire (plan-0011 FE Slice-A2), (f) image/document/file-path 연동 준비 — 3 ADR Draft (0033 asset / 0034 file-stat / 0035 picker) + 3 BE work-package (0059/0060/0061), (g) File picker MVP ship (BE fs_list + FE FilePickerModal) + FilePath picker-only (InlineEdit 폐기) + Settings.picker_show_hidden toggle.
> - 같은 날 이전 handover 2 건: `2026-05-17-session-handover-component-design-batch.md`, `2026-05-17-session-handover-maximize-modal-and-ui-batch.md` — 본 session 은 그 이후 (canvas tools / file picker batch).

---

## 1. 프로젝트 개요

- **이름**: gtmux
- **한 줄 정체성**: tmux 를 backend execution engine 으로 쓰는 single-user 의 web canvas workspace — tmux 가 process/session lifecycle 의 진실, FE 가 *canvas layout 의 진실*.
- **현재 phase / 단계**: **Stage 7+** — multi-session pivot 완료 + Canvas item 도구 확장 (image / document / free_draw 신규 + 5 type ghost / cursor=center spawn) + Session delete UI ship + Document inline-stored mode FE wire ship + File picker MVP ship + 3 ADR Draft (asset / file-stat / picker) 후 다음 step 사용자 브리핑 대기.
- **침범 불가능한 invariants**:
  - **두 state 분리**: tmux state (mirror only) ↔ web state (FE 진실) — `docs/sketch.md` §4 + `CLAUDE.md`
  - **single-attach + no-takeover**: Webpage:Session = 1:1, 활성 session 강제 takeover 없음 — `docs/adr/0019-session-and-workspace-model.md` D3/D4
  - **control-mode integration**: tmux CLI shell-out 금지 — `docs/adr/0021-terminal-pool-and-mirror.md`
  - **ADR-before-code hard rule**: 비-trivial 결정은 ADR 우선, ADR amend 시 linked plan/handover 도 동시 갱신 — `CLAUDE.md`
  - **Layout ≠ tmux layout**: 캔버스 free 배치 ≠ tmux split — `docs/sketch.md` §4
  - **applyMutation 단일 entry**: 모든 user-driven layout mutation 은 `sessionStore.applyMutation` 통과 (ADR-0028 D11) — 직접 `mutateLayout` 호출 금지 (viewport debounce 만 예외)
  - **path picker-only**: file_path item 의 path 는 **FilePickerModal 통과만** — InlineEdit 폐기 (ADR-0035 D1 amend 2026-05-17, traversal/typo risk 차단)

---

## 2. 현재 session 요약

본 session 은 *세 큰 batch* 가 주축:
1. **0054 cold-pickup brief 진입 + BE 의존 wire 검증** — Export endpoint wire (0053 amend ③), ADR-0028 D11 audit re-verify, Import body cap UX, PanelNode status LED 4-state, ADR-0018 D11 amend draft (restored_geom), Session delete UI ship (ADR-0019 D10.1)
2. **Canvas item 도구 확장** — Part A (cursor=center + ghost 5 type) / Part B (ImageNode + DocumentNode placeholder + spawn) / Part C (Free draw drag-to-stroke + FreeDrawNode) + free_draw 이동 fix + Document mime/size_bytes 누락 fix + v3 시안 차용 (FilePath / Image / Document) + Document InlineEdit wire (plan-0011 FE Slice-A2)
3. **Image/Document/File-path 연동 준비 + File picker MVP ship** — 3 ADR Draft (0033 asset storage + 0034 file-stat + 0035 file picker) + 3 BE work-package (0059/0060/0061) + ADR-0023 amend ① (picker.roots section) + File picker MVP (BE fs_list + FE FilePickerModal) + FilePath picker-only (InlineEdit 폐기) + Settings.picker_show_hidden + show_hidden per-session toggle

본 session 의 commit 흐름 (시간순, author 가 본 agent 인 것만):

- `4e3a0d8` docs(verification): 0053 amend ③ — FE wire 정합 검증 결과 (export endpoint)
- `ec916a5` docs(adr): 0028 D11 audit re-verify — Undo/Redo Phase 1~3+P0+P1 ship 후 invariant 재확인
- `e9ba35f` feat(frontend/import): client-side body cap (16 MiB) — ImportSessionModal 의 friendly size guard
- `e192397` feat(frontend/panel): Status LED 의 실 source — running / connecting / dangling / offline 4-state
- `985ee2d` docs(adr): 0018 D11 amend draft — ItemCommon.restored_geom? (minimize disk persistence)
- `162a171` feat(frontend/session-delete): SessionListModal hover-kebab + SessionMenu "Delete current…" (ADR-0019 D10.1)
- `0da2206` feat(frontend/canvas): cursor=center spawn + 가이드 박스 5 type 일반화 (Part A)
- `65c4de9` feat(frontend/canvas): ImageNode + DocumentNode placeholder + spawn (Part B)
- `6f8df17` feat(frontend/canvas): Free draw drag-to-stroke + FreeDrawNode (Part C)
- `f3fb3e6` fix(frontend/canvas): free_draw 이동 path 평행 이동 + Document mime/size_bytes 누락 fix
- `41e6fc8` style(frontend/canvas): FilePath / Image / Document — components-v3.html 시안 차용
- `cf4bb69` style(frontend/file-path): fp-foot placeholder meta + v3 .sep / .right 정합
- `64245ee` feat(frontend/document): inline edit wire — filename + content (plan-0011 FE Slice-A2)
- `b6ecfda` docs(adr): 0033 asset storage + 0034 file-stat — image/document/file-path 연동 준비 (Draft)
- `28a0be6` fix(frontend/file-path): fp-foot 의 placeholder 항상 표시 — path 빈 상태도
- `7b6db09` docs(adr): 0035 file system picker — file_path 도구의 path 선택 UX (Draft)
- `62fc743` feat(backend): GET /api/file-stat — file_path fp-foot meta (ADR-0034 Accepted, 0060 §3 ship) — *다른 worker 의 file-stat 작업이 본 agent 의 fs_list BE + FE picker MVP 도 동시 흡수* (attribution 혼합, §6 의 GOTCHA 참조)
- `839393d` feat(file-path): picker-only path 입력 + Settings.picker_show_hidden + show_hidden toggle

본 session 진행 중 *다른 worker* 가 author 한 commit 다수 — 본 session 작업과 분리:
- ColorPicker phase 1~4 시리즈 (Figma-style popover, SV/hue/alpha drag, Format toggle, Eyedropper, Recent history, OKLCH + localStorage + token-aware palette, RightPanel anchor 등)
- favicon (3 PNG set), ESC tool 취소 wire, readme KR, BE Document inline-stored (2ebe8d6), clipboard/shift constraint/multi-select context menu ADR (6e43abc), 0055 BE summary, ADR-0021 D10.3 + 0053 §3.4 close, respawn per-UUID lock, 0056 amend 등.

### 결정사항 (사용자 합의 / 거부 포함)

- **Cursor=center 적용 도구** (#1+#2 사용자 결정): terminal + note + file_path + image + document 5 type. text(160×56)는 작아서 ghost 의미 약함 — corner 유지.
- **가이드 박스 추가 type**: note + image + document + file_path (사용자 #2 명시). text 는 ghost 없음.
- **Free draw 패턴**: Drag-to-stroke 표준 (Excalidraw/Figma marker). 1 stroke = 1 free_draw item. point cap 5000 (ADR-0018 D4 정합).
- **Image / Document 도구 처리**: 이번 turn 에 placeholder Node + BE 후속 wire (사용자 결정). Image=빈 asset_id placeholder, Document=inline-stored mode (BE schema amend ② 정합).
- **DocumentNode 시안 차용**: components-v3.html §02 의 grid 30/1fr/26 (head/body/foot) + eyebrow + h2 + p. content 의 markdown heading 자동 parse.
- **Document InlineEdit wire**: filename 더블 클릭 → InlineEditField, body 더블 클릭 → InlineEditTextarea (rows=8). content 64KB cap (BE DOCUMENT_INLINE_MAX_BYTES 정합) + client toast.
- **FilePath InlineEdit 폐기**: 더블 클릭 = picker modal 진입. path 의 free-form typing = traversal/typo risk (사용자 결정).
- **Asset storage (ADR-0033 grilling)**: `<workspace>/.assets/<sha256>` (portability), unified 50 MiB + image/document MIME allowlist + **Settings 에서 사용자 조정 가능** (hard ceiling 200 MiB server-only), lazy GC (boot scan).
- **File-stat (ADR-0034)**: size + lines + branch (git status / dirty / ahead-behind 첫 ship 제외).
- **File picker (ADR-0035)**: trigger = file_path 도구 spawn 직후 자동 modal (cancel = item spawn 안 함). scope = ADR-0023 allowlist 영역 (사용자 동적 확장 — Stage 3). BE = lazy per-dir list. MVP = ADR draft 먼저 → 사용자 즉시 확인 의도 → BE + FE 동시 ship.
- **picker_show_hidden** = Settings 영구 default + 모달 내 "Show hidden" checkbox 의 per-session override (사용자 결정 — "server config option 에서 ... 선택").

### 본 session 의 신규 / 변경 파일 (commit 단위 누적, file:line 정밀)

**Frontend (lib/canvas/)**:
- `Canvas.svelte` — POINT_SPAWN_DEFAULTS 5 type map + `pointSpawnGhost` derived (cursor=center) + onpaneclick 의 5 type spawn 분기 + DragShape 에 'free_draw' 추가 + free_draw 의 points/pointsLocal sequence 수집 + ghostPreview 의 free_draw 분기 (SVG path) + onnodedragstop 의 free_draw 의 points 평행 이동 (line endpoint 패턴 정합) + nodeTypes 에 image/document/free_draw 등록 + filePicker store 사용 + FilePickerModal mount
- `PanelNode.svelte` — Status LED 의 4-state derived (statusKind: running/connecting/dangling/offline) + CSS data-status 분기 (`e192397`)
- `FilePathNode.svelte` — editing/InlineEditField 폐기 + onDblClick = filePicker.openFor(parentDir, onCommit) + fp-foot placeholder em-dash + 항상 표시
- `ImageNode.svelte` (신규) — placeholder is-empty pattern (v3 §04 정합), asset_id set 시 `<img src="/api/assets/{id}">` 자동 교체
- `DocumentNode.svelte` (신규 + v3 차용) — grid 30/1fr/26 + doc-head/body/foot + eyebrow/h2/p + InlineEdit wire (filename + content) + 64KB cap
- `FreeDrawNode.svelte` (신규) — `<svg viewBox>` + `<path>` of node-local coord points
- `itemFactory.ts` — DEFAULT_*_SIZE export (5 type) + createImageItem / createDocumentItem / createFreeDrawItem 신규. createDocumentItem 의 mime='' + size_bytes=byteLength(content) (BE schema required 정합)
- `alignment.ts` — 변경 없음 (참고)

**Frontend (lib/chrome/)**:
- `SessionListModal.svelte` — Available row 우측 hover-kebab + canDelete() 가시성 + onConfirmDelete (1s polling 의 자연 row 제거)
- `SessionMenu.svelte` — "Delete current session…" item (Logout 아래 / Shutdown 위) + 4-step (deleteSession → sessionStore.clear → reconnectGate.cancel → sessionStorageHint.clear → workspaceSwitcher.open)
- `SessionDeleteConfirmModal.svelte` (신규) — D10 copy + destructive button
- `ImportSessionModal.svelte` — IMPORT_MAX_BYTES=16 MiB + onFileChange 의 size guard + hint "Max file size: 16 MB"
- `FilePickerModal.svelte` (신규) — ADR-0035 D5 UI form (breadcrumb + Filter + entries list + Selected footer) + "Show hidden" checkbox (per-session)

**Frontend (lib/http/, lib/stores/, lib/types/)**:
- `types/canvas.ts` — DocumentItem 의 `asset_id?` + `mime: string` + `size_bytes: number` + `content?: string` (ADR-0018 D4 amend ② 정합, BE struct required field)
- `http/fs.ts` (신규) — listDir(dir, { showHidden? }) + DirNotAllowedError / DirNotFoundError
- `stores/filePicker.svelte.ts` (신규) — 전역 picker store (spawn flow + rename flow 공유 single modal instance)

**Backend (crates/http-api/src/)**:
- `fs_list.rs` (신규) — ADR-0035 D3 handler (GET /api/fs/list?dir=&show_hidden=) + workspace 안만 canonical resolve + dot-file filter (Settings 또는 query param) + entries sort + cap 500
- `settings.rs` — BehaviorSettings 에 `picker_show_hidden: bool` field 추가 + PATCH match arm + type_mismatch 분기
- `lib.rs` — mod fs_list + route `/api/fs/list` (다른 worker 의 `62fc743` 에 흡수)
- `file_stat.rs` — 다른 worker 의 BE work-package 0060 ship (`62fc743`, FE wire 도 함께)

**Docs (docs/adr/)**:
- `0023-file-path-open-security.md` — amend ① picker.roots section 분리 (ext+prefix open allowlist 와 시각 분리)
- `0028-undo-redo-policy.md` — D11 audit re-verify entry (Undo/Redo Phase 1~3+P0+P1 ship 후 invariant 통과)
- `0018-canvas-item-data-model.md` — D11 amend draft (`ItemCommon.restored_geom?`), D4 의 image/document 영역 ADR-0033 reference + file_path fp-foot ADR-0034 reference (다른 worker 의 amend ② Document inline-stored 와 합산)
- `0033-asset-storage-and-serving.md` (신규, Draft) — D1~D9, /api/assets/* binary endpoint roadmap
- `0034-file-stat-endpoint.md` (신규, Accepted via amend ①) — D1~D7, GET /api/file-stat
- `0035-file-system-picker.md` (신규, Draft) — D1~D10, GET /api/fs/list + picker.roots toml schema mutation

**Docs (docs/reports/)**:
- `0053-be-verification-checklist.md` — amend ③ FE wire 정합 검증 결과 (export endpoint)
- `0054-session-migration-handover.md` — P1/P2 row sync (Export wire closed, status LED closed, Import cap closed, Undo/Redo audit closed, D11 draft 진행)
- `0059-be-asset-storage-work-package.md` (신규) — ADR-0033 BE implementation (3 Stage, Gate 0033-1~15)
- `0060-be-file-stat-work-package.md` (신규 + 다른 worker amend ② std-only branch parser ship) — ADR-0034 implementation
- `0061-be-fs-list-work-package.md` (신규) — ADR-0035 BE implementation (3 Stage, Gate 0035-1~9)
- `0056-be-document-inline-mode-and-assets.md` — ADR-0030 placeholder → ADR-0033 reference 갱신
- 직전 batch handover 2 (component-design / maximize-modal) — §5.3 / §5.4 status sync

미커밋 변경: 없음. 본 session 의 모든 FE/BE/doc 작업 commit 완료.

---

## 3. 주요 참조 자료

| 영역 | 경로 | 왜 읽어야 하는가 |
|---|---|---|
| 프로젝트 instructions | `CLAUDE.md` | 언어 컨벤션 (docs KO / code EN), ADR-before-code hard rule, MCP graph 우선, applyMutation 단일 entry, **path picker-only** |
| 스펙 | `docs/sketch.md` | scope/MVP/우선순위/threat model (KO) |
| 직전 cold-pickup brief | `docs/reports/0054-session-migration-handover.md` | 본 session 진입 시점 — 모든 P1/P2 row status sync 완료 |
| BE 측 본 sprint 종합 | `docs/reports/0055-be-session-summary-for-fe.md` | BE 가 land 한 모든 wire (attach idempotent, D6 heartbeat, export endpoint, respawn race, import body cap) |
| Document inline + Assets roadmap | `docs/reports/0056-be-document-inline-mode-and-assets.md` | Stage 1 ship (`2ebe8d6` Document inline-stored) + Stage 2 = ADR-0033 reference |
| 본 session 신규 ADR (Draft / Accepted) | `docs/adr/0033-asset-storage-and-serving.md` (Draft), `docs/adr/0034-file-stat-endpoint.md` (Accepted amend ①), `docs/adr/0035-file-system-picker.md` (Draft) | image/document/file-path 의 BE 의존 정본. 다음 session 의 BE land 결정 필요 |
| 본 session 신규 BE work-package | `docs/reports/0059-...`, `0060-...`, `0061-...` | ADR 별 Stage 분리 + Gate test 매트릭스 |
| ADR-0018 D11 draft | `docs/adr/0018-canvas-item-data-model.md` §D11 | PanelNode min/max disk persistence — `ItemCommon.restored_geom?` schema amend. Accepted 전 grilling + 별 plan 필요 |
| ADR-0019 D10.1 | `docs/adr/0019-session-and-workspace-model.md` §D10.1 (G51) | Session delete FE entry points 정본 — 본 session 에 FE ship (`162a171`) |
| ADR-0023 amend ① | `docs/adr/0023-file-path-open-security.md` §변경 이력 | `[picker.roots]` toml section 신규 — file-open 과 picker browse 의 별 권한 |
| 활성 plan #1 | `docs/plans/0011-component-design-batch-caption-document.md` | caption / document FE Slice-A2 — document inline edit 본 session ship (`64245ee`). **caption 미진행** (CaptionNode 신규 + toolStore 의 caption tool 추가 필요) |
| 시안 (v3 / v4) | `ref/frontend-design/components-v3.html` (untracked, 사용자 source), `components-v4.html` (untracked) | v3 §01~§05 시안 정본 (FilePath/Image/Document/Note 등). v4 = ColorPicker 시안 |
| 직전 batch handover 2 | `docs/reports/2026-05-17-session-handover-component-design-batch.md`, `...-maximize-modal-and-ui-batch.md` | 같은 날의 *이전 시점* — Inspector v2 / Maximize modal / Note minimize chip 등 |

---

## 4. 진행중인 작업

본 session 의 *자체* 작업은 모두 commit 완료. *다음 session 이 이어야 할* 진행중 항목:

### 4.1 caption type — plan-0011 잔여

- **상태**: ADR-0018 D10 amend (2026-05-16) 가 caption payload 정의 (`head/body/meta?`). BE schema 가 caption variant 를 Item enum 에 추가했는지 확인 필요 (다른 worker 의 amend 가능). FE 측 = **toolStore 에 'caption' 도구 없음** + **CaptionNode 미존재**.
- **관련 문서**: `docs/plans/0011-component-design-batch-caption-document.md` §3 (FE Slice-A2)
- **다음 한 step**:
  1. `codebase/backend/crates/http-api/src/schema.rs` 의 `Item::Caption` variant 존재 여부 확인. 없으면 BE work-package 작성.
  2. FE: `lib/types/canvas.ts` 의 `CaptionItem` interface 신규 + `CanvasItem` union 추가.
  3. FE: `lib/canvas/CaptionNode.svelte` 신규 — `ref/frontend-design/components-v3.html §01` 의 *pinned annotation block* (accent rail + head + body) 시안 정합.
  4. FE: `lib/canvas/itemFactory.ts` 에 `createCaptionItem` + DEFAULT_CAPTION_SIZE 추가.
  5. FE: `lib/stores/toolStore.svelte.ts` 의 `ToolId` 에 `'caption'` 추가 + Toolbar2 의 group 에 등록.
  6. FE: `Canvas.svelte` 의 onpaneclick 분기 + nodeTypes 등록.

### 4.2 Image / Document asset endpoint — ADR-0033 (Draft) 후속

- **상태**: ADR Draft 작성 완료. BE work-package 0059 작성 완료. **BE 구현 미진행**.
- **관련 문서**: `docs/adr/0033-asset-storage-and-serving.md`, `docs/reports/0059-be-asset-storage-work-package.md`
- **다음 한 step**:
  1. ADR-0033 grilling/review → Accepted promote.
  2. 0059 §1 Stage 1 BE implementation — `crates/http-api/src/assets.rs` 신규 (POST multipart + GET binary + ETag).
  3. Settings amend — `assets.*` section + hard ceiling validate.
  4. Stage 2 — boot 시 lazy GC.
  5. Stage 3 — FE ImageNode 의 file picker + upload + `<img src="/api/assets/...">` wire.

### 4.3 File-stat FE wire 검증 + Settings.picker_show_hidden Settings UI

- **상태**: BE `GET /api/file-stat` ship (`62fc743`, ADR-0034 Accepted). FE wire 도 함께 — `lib/stores/fileStat.svelte.ts` + FilePathNode 의 `$effect` 로 data.path 변경 감지 + fetch. **manual 확인 필요** — fp-foot 의 real data (lines/KB/branch) 실제 표시 되는지.
- **picker_show_hidden Settings UI**: `lib/chrome/SettingsOverlay.svelte` 의 `behavior` section 이 placeholder 상태. 영구 토글 (PATCH /api/settings) wire 미.
- **관련 문서**: `docs/adr/0034-file-stat-endpoint.md`, `docs/reports/0060-be-file-stat-work-package.md`
- **다음 한 step**:
  1. browser reload 후 file_path item 의 fp-foot 가 real data 보이는지 확인. ADR-0023 allowlist 통과 path 만 200, 그 외 403 → placeholder em-dash 유지.
  2. SettingsOverlay 의 behavior section wire — `auto_kill_terminal_on_panel_close` + `picker_show_hidden` 두 checkbox + PATCH 호출 + 응답 snapshot 으로 UI 갱신.

### 4.4 File picker Stage 3 — 사용자 root 동적 추가

- **상태**: ADR-0035 Draft + 0061 work-package 의 Stage 3 정의. **BE 미 ship**. FE picker modal 의 `[+ Add browse root]` 버튼 도 미 추가.
- **관련 문서**: `docs/adr/0035-file-system-picker.md` §D6, `docs/reports/0061-be-fs-list-work-package.md` §4
- **다음 한 step**:
  1. BE: `POST /api/fs/allowlist/picker-root { path }` handler — hard blocklist check + ADR-0023 toml 의 `[picker.roots]` 에 atomic append. toml schema mutation (ADR-0023 amend ① 정합).
  2. BE: `fs_list_handler` 의 allowlist 검사 확장 — `picker.roots` 도 accept (현재 workspace only).
  3. FE: FilePickerModal 의 좌측 Roots rail + `[+ Add browse root]` 버튼 + path input modal + confirm flow.

### 4.5 ADR-0018 D11 amend (restored_geom) — Accepted 전

- **상태**: Draft. BE schema + FE handler + E2E 의 implementation step 정의 완료 (D11 §"구현 step" 5개).
- **관련 문서**: `docs/adr/0018-canvas-item-data-model.md` §D11
- **다음 한 step**:
  1. ADR D11 grilling/review → Accepted promote.
  2. 별 plan 분리 (TBD numbering, 예: `docs/plans/0012-restored-geom-schema.md`).
  3. BE schema.rs `ItemCommon` 의 `restored_geom: Option<RestoredGeom>` 필드 추가 + serde round-trip test.
  4. FE `canvas.ts::ItemCommon` 정합 + PanelNode/NoteNode 의 onMinimizeClick 변경 (backupItemGeom → schema field 함께 set).

### 4.6 0048 / Undo-Redo manual E2E (browser 필요)

- **상태**: code-side D11 audit re-verify 완료 (`ec916a5`). 0046 BE attach idempotent + Export wire 모두 ship — 별 BE 의존 없음. **browser manual 만 잔여**.
- **관련 문서**: `docs/reports/0048-fe-refresh-validation-checklist.md` S1~S10, ADR-0028
- **다음 한 step**: 사용자 또는 다음 session 의 manual 시점에 §0048 의 S1~S10 + Undo/Redo 의 drag / Inspector / alignment / delete 시나리오 (Cmd+Z).

---

## 5. 향후 작업

본 session 종료 시점에 사용자가 명시:

> "다음 작업내용 브리핑 후 진행하기전 먼저 session context migration을 위한 handover 문서를 작성해줘. 다음 세션에서 이어서 진행하자."

→ **다음 session 의 작업 내용 = 사용자가 다음 session 시작 시 명시 브리핑할 예정** (TBD). 본 handover §4 의 진행중 항목들이 후보. 추가로 명시 안 된 영역 (예: 새 component 도입, BE feature, security amend 등) 도 가능.

가능한 후속 path:
- **§4.2 ADR-0033 asset endpoint BE land** — image 도구의 placeholder → real upload UX 완성
- **§4.1 caption type 도구 추가** — plan-0011 의 잔여 type
- **§4.4 picker Stage 3** — 사용자 dynamic root 추가 (외부 source code reference 의 핵심)
- **§4.5 D11 amend Accepted + 구현** — minimize disk persistence
- 새 영역 (ColorPicker v4 phase 5+, brand SVG 후속, plan-0011 § 5.4~5.8 의 v3 차용 batch 후속, etc.)

---

## 6. 주의사항 / Gotchas

- **`62fc743` commit attribution 혼합** — 다른 worker 의 file-stat (ADR-0034 BE) commit 이 *본 agent 의 fs_list BE handler + FE FilePickerModal + Canvas.svelte 변경 + fs.ts 까지 흡수*. commit message 의 첫 줄은 file-stat 만 명시하나 stat 결과 = 4 BE/FE files 추가. push 안 했으므로 후속 history 정정 가능 — *기능 적용 우선* 으로 그대로 두었음.
- **BE 변경 시 release binary 재컴파일 + 서버 재시작 필수** — `target/release/gtmux` 가 runtime entry. dev mode 자동 reload 없음. 사용자가 `cargo build --release --bin gtmux` + `Ctrl+C` + 재실행 해야 새 endpoint / settings field 활성.
- **path picker-only invariant** — file_path item 의 path 는 *FilePickerModal 통과만*. InlineEdit / direct typing 폐기 (`839393d`). 새 entry point 추가 시 picker 통과 필수 (security floor + UX 일관성).
- **`filePicker` 전역 store** — `lib/stores/filePicker.svelte.ts`. Caller (spawn / rename) 가 `openFor(initialDir, onSelect)` 호출 후 callback 처리. Canvas 의 single FilePickerModal 이 store 의 state 바인딩. 새 caller (e.g. layer panel 의 file_path drag) 추가 시 같은 store 사용.
- **Document FE type 의 BE 정합 위험** — `DocumentItem.mime/size_bytes` 가 *required* (BE struct 정합). placeholder 시 mime='' + size_bytes=byteLength(content). 새 path 에서 inline document 생성 시 둘 다 set 누락하면 BE reject. (이전 회귀: `f3fb3e6` fix)
- **FilePicker modal 의 workspace-only scope** — 현 MVP 는 workspace 안 만 accept. 외부 path 입력 시 403 dir_not_allowed. Stage 3 (사용자 dynamic root) 미 ship — *외부 source code reference 가 필요한 사용자는 ADR-0023 file-open allowlist 의 prefix 를 추가 한 후* 그 prefix 를 picker.roots 로 사용 (현 MVP 미지원, Stage 3 후).
- **다른 worker 와의 동시 commit 빈번** — 본 session 진행 중 ColorPicker 시리즈 (`fab05b8`~`302fadb` 등 11+ commits) + 0055 BE summary + 0056 doc amend + 6e43abc (clipboard ADR) + 9a06dbf (readme KR) 등 *별 worker* 가 동시 land. 다음 session 도 동일 가능성 — `git log --oneline 2026-05-17..HEAD` 또는 본 commit 시점 이후 확인.
- **사용자가 거부한 접근**:
  - file_path 의 free-form typing — picker-only 의 보안/UX 우월. InlineEdit 폐기 (`839393d`).
  - picker scope = workspace 만 → 외부 source 접근 불가 (사용자가 ADR-0023 allowlist 영역 으로 확장 결정).
  - picker scope = $HOME 부터 자유 → security floor 미달 (사용자가 ADR-0023 allowlist 영역 으로 한정).
  - asset MIME hard allowlist (Settings 미허용) → 사용자가 "설정에서 정할 수 있도록" 명시.
- **사용자가 명시 결정**:
  - picker_show_hidden = Settings 영구 default + per-session checkbox 둘 다 (`839393d`).
  - asset storage = workspace 안 (.assets/) — portability 우선.
  - asset GC = boot 시 lazy scan — 구현 단순.
  - free draw = drag-to-stroke 표준.
  - image / document = 이번 turn 에 placeholder Node + BE 후속 wire.
  - cursor=center 적용 = terminal/note/file_path/image/document (text 제외).
- **Untracked source files**: `ref/frontend-design/components-v3.html`, `components-v4.html` — 사용자 source 시안. git tracked 아닌데 활용 중. push 시점에 사용자가 add 또는 .gitignore 처리.
- **converted_logo.svg deletion** — `docs/src/converted_logo.svg` 가 `D` 상태로 working tree 에 있으나 다른 worker 의 brand asset 후속 작업 영역. 본 session 영역 아님.

---

## 7. 새 session 시작 방법

이 문서를 받은 session 은 다음 순서로 부트스트랩한다:

1. **이 handover 문서 (`docs/reports/2026-05-17-session-handover-canvas-tools-and-file-picker.md`) 를 끝까지 읽는다**.
2. **`CLAUDE.md`** 를 읽는다 — 언어 컨벤션 (docs KO / code EN), ADR-before-code hard rule, MCP graph 우선, **applyMutation 단일 entry**, **path picker-only invariant**.
3. **`docs/sketch.md`** + (옵션) `CONTEXT.md` (있으면) — 프로젝트 scope/MVP/threat model.
4. **`docs/reports/0054-session-migration-handover.md`** + **`0055-be-session-summary-for-fe.md`** — 직전 cold-pickup brief + BE 측 본 sprint 종합.
5. **§3 의 활성 plan 또는 ADR 정본** 읽기:
   - 다음 session 의 작업 영역 사용자 브리핑 listen.
   - 영역 이 §4 의 항목에 해당하면 그 §의 정본 ADR/report 우선 read.
6. **§4 의 진행중 작업** 중 사용자가 지정한 항목의 "다음 step" 부터 진행.
7. **handover 작성 이후 변경 확인**: `git log --oneline 839393d..HEAD` — 본 session 종료 후 다른 worker 가 commit 했을 가능성. 특히 ColorPicker 시리즈 + plan-0011 caption + ADR-0033 BE 등.

만약 §5 의 사용자 브리핑이 *§4 의 항목이 아닌 새 영역* 이면:
- 그 영역의 ADR 존재 여부 확인 (ADR-before-code hard rule).
- 없으면 grilling 진행 → ADR draft → 사용자 review → implementation step 분리.

---

_생성: `session-handover` skill v1_
