# 0054 — Session migration handover (cold-pickup brief)

- 작성일: 2026-05-17
- 작성자: FE 통합 agent (Inspector edit + Undo/Redo Phase 1~3+P0+P1 + Import/Export + Drag undo / Respawn auto fix ship 후)
- 종류: **cold-pickup brief** — 다음 세션이 본 문서 한 장으로 현재 상태 + 즉시 진입 우선순위 + 핵심 컨텍스트 모두 진입 가능
- HEAD: `74bf14b` feat(frontend): Toolbar Undo / Redo button
- baseline: FE `svelte-check 305/0/0` clean · BE workspace 에 uncommitted 변경 있음 (별 agent 의 0052 export endpoint ship 중)

---

## 0. 다음 세션 즉시 진입 — 우선순위 ⚠️ MUST READ

| 우선 | 항목 | 상태 | 정본 |
|---|---|---|---|
| 🟡 P1 | **0048 FE refresh validation 실측 (S1~S10)** | manual test only — BE 0046 land 완료 | `docs/reports/0048-fe-refresh-validation-checklist.md` |
| 🟡 P1 | **Undo/Redo end-to-end manual test** | code ship 완료 + **D11 audit re-verify 통과 (2026-05-17)** — manual: drag / Inspector / alignment / delete 각 시나리오 Cmd+Z 동작 확인. invariant 코드-측 0 발견 — manual 만 잔여 | ADR-0028 변경 이력 (re-verify entry) + 본 §3 |
| ✅ ~~P1~~ | **Export endpoint wire 검증** | ✅ **closed (2026-05-17)** — BE 0052 ship 완료 (`ecc8581`) + FE wire 정합 검증 완료 (0053 §2.3.1 amend ③, 0 mismatch). 잔여: browser manual download smoke 만. | `docs/reports/0053-be-verification-checklist.md` §2.3.1 |
| 🟢 P2 | **terminal 유 layout XtermHost fit() loop manual 검증** | 0045 §9 7항 중 1항 잔여 | `docs/reports/0045-...-amend.md` §11.6 |
| 🟢 P2 | **respawn per-UUID Mutex follow-up** | BE 측 follow-up — 현 동작 safe-with-minor-window | `docs/reports/0053-be-verification-checklist.md` §3.4 |
| ✅ ~~P2~~ | **PanelNode status LED 실 source** | ✅ **closed (2026-05-17, `e192397`)** — 4-state derived (running / connecting / dangling / offline) | 직전 batch handover `2026-05-17-session-handover-maximize-modal-and-ui-batch.md` §5.4 |
| 🟢 P2 | **PanelNode min disk persistence (ADR-0018 D11 draft 후속)** | **D11 amend draft 작성 ✅ 2026-05-17** — `restored_geom?` field. Accepted 전 grilling/review + 별 plan 필요 (BE schema + FE handler + E2E) | `docs/adr/0018-canvas-item-data-model.md` §D11 + 직전 batch handover §5.3 |
| ✅ ~~P2~~ | **Import body cap UX** | ✅ **closed (2026-05-17)** — ImportSessionModal 에 16 MiB client-side guard 추가 (BE `SESSION_PUT_MAX_BYTES` 와 동일 ceiling). 초과 파일 = 'pick' stage 의 inline error ("Session file too large (X MB). Maximum is 16 MB.") + network 안 탐. hint 에 "Max file size: 16 MB" 사전 안내. | ADR-0029 §6 amend ② + 본 §3 |

### 0.1 본 session 핵심 산출물 한 문단

Inspector v2 (geometry/label/color edit + STATE icon toggle + readonly muted color + .k fixed-width + .prop-row v2 시안 정합) ship 완료. Undo/Redo Phase 1~3 + P0 (Delete history capture) + P1 (Toolbar Undo/Redo button) ship — `sessionStore.applyMutation` / `applyDeletion` 두 단일 entry 가 historyStore 통해 모든 layout mutation 의 PRE-state capture. Drag commit 의 priorSnapshot option 으로 optimistic update 후에도 history 정확. Respawn 무한 loop 차단 (`dispatcher.handleTerminalSpawned` 의 `clear` 누락 fix + `danglingTerminals` single-flight lock + PanelDanglingOverlay auto-respawn 전환). Session import/export ship — `parseEnvelope` / `importSession` / `exportSession` http client + ImportSessionModal (pick→preview→importing→done) + ExportSessionModal (privacy warning) + SessionMenu entries. BE 의존: 0046 attach idempotent ✅ land, 0052 export endpoint ⏳ ship 중 (별 agent). FilePathNode height 셋 통일 (48/64/32 → 80px). Backspace lasso 회귀 fix (onselectionchange wire 복구).

---

## 1. 프로젝트 mental model (1 분 요약)

**gtmux** = tmux-backed web canvas workspace. *single-user* SPA. tmux 가 process lifecycle 의 진실, FE 가 *canvas layout 의 진실*.

### 1.1 어휘 (CONTEXT.md / ADR-0019 정합)

| 어휘 | 정의 |
|---|---|
| Server | gtmux process. 1 port owner, 1 workspace dir 바인딩 |
| Workspace | server 와 1:1, `<XDG_DATA_HOME>/gtmux/workspace/` dir |
| Session | workspace 안 named file record. canvas layout + viewport |
| Webpage | 브라우저 탭. 1 WS 연결, 0/1 session attach |
| Terminal | server-pool, multi-session 공유 가능 (mirror) |
| Canvas Item | canvas 위 시각 객체 (terminal / rect / ellipse / line / text / note / file_path / document / image) |
| Panel | `type:"terminal"` 인 Canvas Item |

### 1.2 핵심 invariant

1. tmux state ↔ web state 분리
2. layout ≠ tmux layout
3. single-attach: Webpage : Session = 1:1 (ADR-0019 D3)
4. takeover 금지 (ADR-0019 D4)
5. control-mode integration only
6. **ADR-0028 D1.1** (신규) — Undo 의 effect 는 layout snapshot 복원 only, tmux/pool/session lifecycle 은 손대지 않음

### 1.3 우선 단계

현재 = **Stage 7** — multi-session pivot 완료, UX 폴리시 + Undo/Redo + Import/Export ship 후속 + 회귀 가드.

---

## 2. 디렉토리 / 빌드

| | 경로 | 명령 |
|---|---|---|
| BE workspace root | `codebase/backend/` | `cargo test --workspace`, `cargo run -p gtmux -- start --session <name>` |
| FE | `codebase/frontend/` | `pnpm check`, `pnpm build`, `pnpm dev` |
| 문서 | `docs/` (ADR/plan/report 폴더) | — |

baseline:
- FE: `svelte-check 305 FILES 0 ERRORS 0 WARNINGS`
- BE: 별 agent uncommitted 변경 있음 (lib.rs/sessions.rs 의 0052 ship 중)

---

## 3. 본 session 의 산출물 (HEAD 기준 역순)

### 3.1 Undo/Redo Phase 1~3 + P0 + P1 (ADR-0028)

- **Accepted** ADR-0028 (D11 audit 통과 — `putLayout` 직접 호출 0건)
- 신규 `historyStore.svelte.ts` — per-session stack, capacity 50, canUndo/canRedo $derived
- `sessionStore.applyMutation(transform, options)` — 모든 layout mutation 의 단일 entry. `priorSnapshot` option 으로 drag optimistic 후에도 정확한 PRE capture (`0a52ce3`)
- `sessionStore.applyDeletion(ids, options)` — Delete 의 history capture entry (`459d772`). Canvas / ContextMenu / PanelNode 3 callers migration
- `sessionStore.undo()` / `redo()` — Cmd+Z / Cmd+Shift+Z / Ctrl+Y 키바인드 (Canvas.svelte)
- 16+ callsites migration → `mutateLayout` direct 호출 0건 (`b77bd4f`)
- Toolbar Undo/Redo button (`74bf14b`) — `historyStore.canUndo` derived disabled

### 3.2 Inspector v2 (ADR-0027 ship)

- Geometry/label/z input + STATE icon toggle (eye/lock/minimize) + line endpoint x2/y2 + note color
- `InspectorField.svelte` (mixed-aware, blur/Enter commit) + readonly muted color
- v2 시안 정합: `.prop-row` (1fr 1fr / full), `.input` (h28/mono11/surface-2), `.k` fixed 56px
- ColorPicker / align-group full-width, picker row 정렬

### 3.3 Import/Export FE (ADR-0029)

- `parseEnvelope` + `importSession` + `exportSession` http client (`1661f89`)
- ImportSessionModal: pick→preview→importing→done 4-stage, 409 rename, "Open imported session?" confirm + auto detach+attach
- ExportSessionModal: privacy warning + Blob URL download
- SessionMenu entries (Titlebar 좌 kebab) — [Import session…] / [Export session…]
- BE 의존: 0052 work package (`fb9c3bc`), 별 agent ship 중

### 3.4 Drag undo + Respawn loop fix

- `0a52ce3` — Canvas drag commit 의 priorSnapshot 명시 → Cmd+Z 정확 동작
- `dfd8efd` — dispatcher.handleTerminalSpawned 의 `danglingTerminals.clear` 추가 + `inFlight` lock + PanelDanglingOverlay auto-respawn 전환

### 3.5 FilePathNode height 통일 (`52310f6`)

`DEFAULT_FILE_PATH_SIZE.h` 48 → 80, NodeResizer minHeight 64 → 80, onResizeEnd clamp 32 → 80. resize 시 layout shift 차단.

### 3.6 Backspace lasso 회귀 fix (`64ef296`)

`onselectionchange` 가 SvelteFlow prop 에 attach 안 된 채로 남아 있던 회귀 — wire 복구 1-line.

### 3.7 documentation

- ADR-0028 Accepted (Undo/Redo)
- ADR-0029 reference (별 agent 의 Session import/export ADR draft)
- 0051 §0 amend (heartbeat flaky closed)
- 0052 BE work package (export endpoint handoff)
- 0053 BE verification checklist (6 항목 + amend ① by BE agent)

---

## 4. BE 의존 + 진행 중

### 4.1 0046 attach_handler idempotent — ✅ shipped (`e9eb9a6`)

FE 측 refresh / silentReattach 자연 정상화. 0048 S1~S10 실측 잔여.

### 4.2 0052 export endpoint — ✅ shipped (`ecc8581`) + FE wire verified

BE land 완료 — `export_handler` + `ExportEnvelope` + `sanitize_export_filename` + `rfc3339_utc_now` helper + Gate 0029-1~5 5/5 PASS. FE wire 정합 검증 (`SessionExportEnvelope` ↔ BE `ExportEnvelope` field-by-field cross-check + `parseEnvelope` validation + `exportSession` 의 path/credentials/분기/filename fallback) 완료 — **0 mismatch, FE patch 불필요**. 잔여: browser manual download smoke (Blob URL download / privacy warning UX). 정본: `docs/reports/0053-be-verification-checklist.md` §2.3.1 (amend ③).

### 4.3 D6 heartbeat — ✅ shipped + 정합 verified (0053 §5)

### 4.4 respawn per-UUID Mutex — 🟢 follow-up

현재 safe-with-minor-window (FE last-0x88-wins). ADR-0021 D10 amend 검토.

---

## 5. 일반화 교훈

### 5.1 Optimistic update + history capture

Caller 가 optimistic 으로 store mutate 한 *후* applyMutation 호출하면 PRE-snapshot 이 이미 새 state. `priorSnapshot` option 으로 명시 전달 필요 (`0a52ce3`).

### 5.2 Multi-source mutation entry 통일 invariant

`mutateLayout` 만이 entry 였으면 history capture 자연 정합 — 그러나 `deleteItem` 등 별 BE endpoint 가 별 path 라 추가 helper (`applyDeletion`) 필요. 새 BE endpoint 추가 시 store-side wrapper 동반 ADR-0028 D11 audit 항목 확장.

### 5.3 Multi-webpage broadcast race

같은 UUID 에 두 webpage 가 동시 동작 → BE 가 idempotent 또는 last-wins 보장 필요. FE 측은 single-flight lock (`danglingTerminals.inFlight`) + 0x88 broadcast 후 자연 clear 패턴.

---

## 6. 핵심 reference 위치

| 종류 | 경로 |
|---|---|
| Undo/Redo 정책 | `docs/adr/0028-undo-redo-policy.md` |
| Import/Export 정책 | `docs/adr/0029-session-import-export.md` |
| Inspector v2 | `docs/adr/0027-inspector-multi-select-and-alignment.md` |
| BE checklist (FE 의존) | `docs/reports/0053-be-verification-checklist.md` |
| BE export work package | `docs/reports/0052-be-session-export-endpoint.md` |
| BE attach idempotent (shipped) | `docs/reports/0046-be-attach-handler-idempotent.md` |
| FE refresh validation | `docs/reports/0048-fe-refresh-validation-checklist.md` |
| lasso/selection 회귀 | `docs/reports/0050-lasso-selection-regression-scenarios.md` |
| 직전 handover | `docs/reports/0051-session-migration-handover.md` |

**reading order** (cold-pickup):
1. 본 문서 §0~§3
2. 0053 BE verification checklist (BE 측 검증 결과 amend ①)
3. ADR-0028 (Undo/Redo) — D11 audit / D12 entry 패턴
4. ADR-0029 (Import/Export) — D2 envelope / D6 conflict
5. 0048 validation checklist (S1~S10 실측 진입)

---

## 7. 환경 / clean state

- Working tree: 별 agent 의 uncommitted 변경 (BE lib.rs/sessions.rs, 0053 amend, ADR-0029, 별 handover docs, G.png)
- 본 session 의 모든 FE 작업: commit 완료
- baseline 재현: §2 의 명령
