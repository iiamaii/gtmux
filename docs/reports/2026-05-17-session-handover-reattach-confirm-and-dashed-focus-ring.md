# Session Handover — 2026-05-17 — Reattach confirm_required escalation + Dashed focus ring 제거

> 이 문서는 `session-handover` skill 로 생성된 session 인수인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-17 (밤)
> - 본 session 의 마지막 commit: `bde370e` (style(frontend): Figma-signature dashed accent focus ring 제거)
> - 본 session 주제: 직전 handover (`...-canvas-tools-and-file-picker.md`) cold-pickup 후 — (a) 사용자 보고 회귀 fix: server 종료→재시작→새로고침 시 hint-based reattach 가 `unmatched > 0` 을 silent 흡수해 panel 만 남기고 respawn dialog 누락 → `confirm_required` 분기 escalation, (b) UX cleanup: Figma-signature 파란 dashed `:focus-visible` focus ring 19 rule 일괄 제거.
> - 같은 날 이전 handover 3 건 (시간 순):
>   1. `2026-05-17-session-handover-component-design-batch.md` (Inspector v2 / 컴포넌트 디자인 batch)
>   2. `2026-05-17-session-handover-maximize-modal-and-ui-batch.md` (MaximizedItemModal / Note minimize 등)
>   3. `2026-05-17-session-handover-canvas-tools-and-file-picker.md` (canvas tool 확장 + file picker MVP)
>   본 session 은 그 다음 시점 (4번째 handover).
> - 본 session 종료 후 다른 worker 의 commit chain: theme hot-reload 관련 6 commit (`092c8e9` ~ `e006962`) — XtermHost force remount / silentReattach trigger / theme buffer preservation. 본 session 영역 아님.

---

## 1. 프로젝트 개요

- **이름**: gtmux
- **한 줄 정체성**: tmux 를 backend execution engine 으로 쓰는 single-user 의 web canvas workspace — tmux 가 process/session lifecycle 의 진실, FE 가 canvas layout 의 진실.
- **현재 phase / 단계**: **Stage 7+** — multi-session pivot 완료 + canvas tool 확장 (image/document/free_draw) + file picker MVP + reattach 회귀 fix (본 session) + dashed focus ring cleanup (본 session) 이후, theme hot-reload 안정화 chain 진행 중 (다른 worker).
- **침범 불가능한 invariants**:
  - **두 state 분리**: tmux state (mirror only) ↔ web state (FE 진실) — `docs/sketch.md` §4 + `CLAUDE.md`
  - **single-attach + no-takeover**: Webpage:Session = 1:1, 활성 session 강제 takeover 없음 — `docs/adr/0019-session-and-workspace-model.md` D3/D4
  - **control-mode integration**: tmux CLI shell-out 금지 — `docs/adr/0021-terminal-pool-and-mirror.md`
  - **ADR-before-code hard rule**: 비-trivial 결정은 ADR 우선, ADR amend 시 linked plan/handover 도 동시 갱신 — `CLAUDE.md`
  - **Layout ≠ tmux layout**: 캔버스 free 배치 ≠ tmux split — `docs/sketch.md` §4
  - **applyMutation 단일 entry**: 모든 user-driven layout mutation 은 `sessionStore.applyMutation` 통과 (ADR-0028 D11)
  - **path picker-only**: file_path item 의 path 는 FilePickerModal 통과만 — InlineEdit 폐기 (ADR-0035 D1 amend)
  - **reattach 의 `unmatched > 0` 처리**: silent 흡수 금지 — `confirm_required` 로 escalate, AttachConfirmModal 사용자 확인 통과 (본 session 신규 invariant, 아래 §6 GOTCHA 참조)

---

## 2. 현재 session 요약

본 session 은 *두 작업 batch*:

### 2.1 Batch A — Reattach unmatched 회귀 fix (commit `9bd2eea`)

**사용자 보고 (verbatim)**:
> "메인 페이지(canvas) 작업 중 서버 종료 후 다시 새로고침 & 인증 후 진입을 하면 원래는 session 선택 modal이 무조건 떠야함. 그런데 지금은 바로 진입을 함. 그러다보니 terminal은 모두 종료되어서 없는데 respawn 없이 panel만 남아있음. (정상적인 과정으로 진입하면 respawn dialog가 뜸)."

**근본 원인 (코드 추적 결과)**:
- `sessionStore.svelte.ts::attemptReattach` (line 424-430 직전) 가 200 응답의 body 를 `await attachRes.json()` 으로 *drain 만* — `matched`/`unmatched` 무시. 주석은 "plan-0008 §8 risk row" 참조하나 실 plan-0008 §8 에는 해당 결정 근거 없음 (기존 design 오판으로 추정).
- 비교 대상: `WorkspaceSwitcher.svelte::tryAttach` (line 119-125) 는 `unmatched > 0` 시 `kind: 'confirm_required'` 분기 → `goAttachConfirm` → `AttachConfirmModal`.
- BE side `crates/http-api/src/sessions.rs::classify_layout_terminals` (line 752-768) 가 `terminal_map.lookup_pane(&uuid)` 로 live/stale 분류. **BE process 재시작 → terminal_map 비어있음 → 모든 layout item UUID 가 `unmatched` 로 분류 → 200 + unmatched=[전체]**.
- FE silent 흡수 → setActiveSession + loadLayout → panel mount + WS 가 그 UUID 로 frame 못 받음 → terminal 빈 상태 panel 만 남음.

**Fix 정합 (`WorkspaceSwitcher.tryAttach` 패턴과 1:1)**:
1. `ReattachResult` 에 `{ kind: 'confirm_required'; summary: AttachConfirmSummary }` 추가 (sessionStore.svelte.ts:55-58).
2. `attemptReattach` 의 200 path 가 `attachBody.unmatched.length > 0` 시 layout fetch 건너뛰고 즉시 `confirm_required` 반환 (sessionStore.svelte.ts:435-446).
3. `reconnectGate.#run` 의 switch 에 `case 'confirm_required'` 추가 (reconnectGate.svelte.ts:142-150) — `sessionStore.setActiveSession({name})` + `this.markIdle()` + `workspaceSwitcher.goAttachConfirm(name, summary)`. canvas mount 허용 (items 비어있어 빈 canvas) + AttachConfirmModal 가 덮음.
4. `+page.svelte::maybeSilentReattach` (Phase 2 silent path) 의 result handling 에 `confirm_required` 분기 추가 (+page.svelte:167-172) — Case II 라도 respawn 결정은 사용자 몫이므로 같은 modal 진입.
5. `reconnectGate.svelte.ts:130-138` 의 phase comment 도 "(200 + unmatched=0 시) GET /layout" 으로 갱신.

**복구된 사용자 흐름**:
1. canvas 작업 중 BE Ctrl+C → 재기동 → 브라우저 새로고침 → cookie 인증 통과
2. hint 존재 → `reconnectGate.start(name)` → state='attaching' → boot screen
3. POST /attach → 200 + unmatched=[전체] (BE classify_layout_terminals 분류)
4. **NEW**: `confirm_required` 반환 → AttachConfirmModal 노출 ("N new terminals will be started for missing panels")
5. [Confirm attach] → `attachConfirm` → BE spawn → loadLayout → markReady → 정상 canvas
6. [Cancel] → `goList()` → SessionListModal (사용자 보고의 "session 선택 modal")

**검증 상태**: svelte-check 0 errors / vite build 통과. **browser manual E2E 미진행** — §4.1 참조.

### 2.2 Batch B — Dashed focus ring 19 rule 제거 (commit `bde370e`)

**사용자 요구 (verbatim)**:
> "component들 중 버튼의 테두리에 파란색 dashed line 효과가 있는게 있나? 이 효과 자체를 제거해줘."

**조사 결과**:
- `styles/global.css:26-32` 의 전역 `:focus-visible` 이 *Figma-signature dashed accent focus ring* 의 source — `outline: 2px dashed var(--color-accent)` + `outline-offset: 1px` + `border-radius: var(--radius-sm)` (focusable 모든 element 적용).
- 14 component 가 동일 pattern 으로 `:focus-visible` override — 19 rule 총합.
- 의도적 design 으로 dashed 사용 4 곳: drag/drop "inside group" indicator, `point-spawn-ghost` 도구 미리보기, image placeholder gray border, file-pick drop zone gray border. **별도 visual cue 라 유지**.

**제거 inventory** (file:line → selector):
| File | 제거 selector |
|---|---|
| `styles/global.css` | 전역 `:focus-visible` (line 26-32 block 전체) |
| `lib/toolbar/Toolbar2.svelte` | `.tool:focus-visible` |
| `lib/sidebar/LayerTreeView.svelte` | `.z-btn:focus-visible` |
| `lib/ui/ColorPicker.svelte` | `.swatch-trigger`, `.cp-btn`, `.cp-eye`, `.cp-swatches .sw` (4개 `:focus-visible`) |
| `lib/chrome/SessionListModal.svelte` | `.row`, `.row-kebab` |
| `lib/chrome/ItemInfoView.svelte` | `.align-btn`, `.state-btn` |
| `lib/chrome/ActiveSessionDropdown.svelte` | `.active-session` |
| `lib/chrome/MaximizedItemModal.svelte` | `.max-btn` |
| `lib/canvas/NoteNode.svelte` | `.note-btn:focus-visible` 의 outline 만 제거 (opacity:1 유지), `.note-node.is-min:focus-visible` 전체 |
| `routes/auth/+page.svelte` | `.icon-btn`, `.tab`, `.toggle-eye`, `.submit` (4개) |

총 80 줄 deletion / 0 줄 insertion. CSS bundle 1.8KB 감소.

**유지 (의도적 dashed visual)**:
- `lib/sidebar/LayerTreeView.svelte:1028` — `.row.drop-inside` (drag/drop 그룹 안으로 indicator)
- `lib/canvas/Canvas.svelte:1397` — `.point-spawn-ghost` (도구 spawn 미리보기 5 type)
- `lib/canvas/ImageNode.svelte:129` — image placeholder gray (`--color-border-strong`)
- `lib/chrome/ImportSessionModal.svelte:398` — `.file-pick` drop zone gray
- `lib/canvas/PanelNode.svelte:536` — 주석만, 실 code 는 `outline: none`

**A11y note**: 전역 `:focus-visible` 제거로 키보드 focus 시 *브라우저 default outline* 적용 — focus indicator 가 완전히 사라지지는 않음. 만약 추후 별도 focus style 필요 시 별 패치로 추가.

### 2.3 본 session 의 신규 / 변경 파일 (commit 단위 누적)

**`9bd2eea` (3 file +54/-7)**:
- `codebase/frontend/src/lib/stores/sessionStore.svelte.ts` — `ReattachResult` 확장 (+`confirm_required`) + `AttachConfirmSummary` import + `attemptReattach` 200 path 의 unmatched 검사 분기
- `codebase/frontend/src/lib/stores/reconnectGate.svelte.ts` — `workspaceSwitcher` import + `#run` switch 의 `confirm_required` case + phase comment 갱신
- `codebase/frontend/src/routes/+page.svelte` — `maybeSilentReattach` 의 `confirm_required` 분기

**`bde370e` (10 file -80)**:
- `codebase/frontend/src/styles/global.css`
- `codebase/frontend/src/lib/toolbar/Toolbar2.svelte`
- `codebase/frontend/src/lib/sidebar/LayerTreeView.svelte`
- `codebase/frontend/src/lib/ui/ColorPicker.svelte`
- `codebase/frontend/src/lib/chrome/SessionListModal.svelte`
- `codebase/frontend/src/lib/chrome/ItemInfoView.svelte`
- `codebase/frontend/src/lib/chrome/ActiveSessionDropdown.svelte`
- `codebase/frontend/src/lib/chrome/MaximizedItemModal.svelte`
- `codebase/frontend/src/lib/canvas/NoteNode.svelte`
- `codebase/frontend/src/routes/auth/+page.svelte`

미커밋 변경: 없음 (본 session 범위). `docs/src/converted_logo.svg` 의 `D` 상태는 다른 worker 의 brand 후속 영역 — 본 session 영역 아님.

---

## 3. 주요 참조 자료

| 영역 | 경로 | 왜 읽어야 하는가 |
|---|---|---|
| 프로젝트 instructions | `CLAUDE.md` | 언어 컨벤션 (docs KO / code EN), ADR-before-code hard rule, MCP graph 우선, applyMutation 단일 entry, path picker-only |
| 직전 handover (cold-pickup base) | `docs/reports/2026-05-17-session-handover-canvas-tools-and-file-picker.md` | 본 session 진입 시점 — canvas tool 확장 / file picker MVP / 3 ADR Draft (0033/0034/0035) 의 직전 상태 |
| Attach recovery 정본 | `docs/plans/0008-session-attach-recovery-impl.md` §1.1 (진입 흐름) + §4.4 (reconnectGate) | 본 session fix 가 §1.1 의 step 4 ("attempt reattach")에 `confirm_required` 분기 신규. **§1.1 / §1.2 / §4.4 의 의사코드 amend 가 plan 측에 미반영** — §4.5 참조 |
| Match-or-spawn 결정 | `docs/adr/0018-canvas-item-data-model.md` §D6 | `unmatched > 0` 시 confirm 필요라는 invariant. 본 session fix 는 hint 경로에도 동일 invariant 적용 |
| Session/workspace model | `docs/adr/0019-session-and-workspace-model.md` §D3/D4/D5.4 | single-attach + no-takeover + initial entry attach recovery 의 정본 |
| BE classify 로직 | `codebase/backend/crates/http-api/src/sessions.rs::classify_layout_terminals` (line 752-768) | BE 가 terminal_map lookup 으로 matched/unmatched 분류 — 재기동 후 모든 item 이 unmatched 가 되는 BE 측 근거 |
| 활성 plan (FE component) | `docs/plans/0011-component-design-batch-caption-document.md` | caption / document FE Slice. document 는 직전 handover 가 ship — caption 미진행 (직전 handover §4.1) |
| 본 session 신규 invariant | §6 GOTCHA "reattach 의 unmatched 처리" | hint-based path / silent path 모두 confirm_required escalate — 추가 reattach call site 가 생기면 같은 패턴 적용 필요 |

---

## 4. 진행중인 작업

본 session 의 *자체* 작업은 모두 commit 완료. *다음 session 이 이어야 할* 진행중 항목:

### 4.1 Reattach fix 의 browser manual E2E (본 session 의 직접 후속)

- **상태**: 코드 ship + svelte-check + vite build 모두 통과. **browser 에서 실 server-restart 시나리오 확인 미진행**.
- **관련 문서**: `docs/plans/0008-session-attach-recovery-impl.md` §5.1 (E2E 검증 시나리오) — server-restart row 추가 필요.
- **관련 파일**: `codebase/frontend/src/lib/stores/sessionStore.svelte.ts:435-446`, `codebase/frontend/src/lib/stores/reconnectGate.svelte.ts:142-150`, `codebase/frontend/src/routes/+page.svelte:167-172`
- **다음 한 step**:
  1. `cargo build --release --bin gtmux` 후 server 기동
  2. 브라우저 canvas 진입 → terminal item 1+ 생성
  3. server Ctrl+C → 재기동 (= `cargo build --release && ./target/release/gtmux`)
  4. 브라우저 새로고침 → AttachConfirmModal 등장 확인 (header "Attach session ‘{name}’?" + "N new terminals will be started for missing panels")
  5. Cancel → SessionListModal 로 회귀 확인
  6. 재진입 후 Confirm → 모든 terminal respawn 후 canvas 정상 mount 확인
  7. (옵션) Phase 2 silent path 검증: canvas 활성 중 server 재기동 → tab background ↔ foreground → silentReattach 가 confirm_required 받아 modal 노출

### 4.2 plan-0008 의 §1.1 / §1.2 / §4.4 amend — `confirm_required` 분기 반영

- **상태**: 본 session 의 fix 가 plan-0008 의 의사코드 (8-state machine + reattach 진입 흐름) 와 정합 어긋남. 코드는 ship 됐으나 plan-side 미반영 — **ADR ↔ plan/handover coherence hard rule** (CLAUDE.md) 위반 상태.
- **관련 문서**: `docs/plans/0008-session-attach-recovery-impl.md` — §1.1 step 4 의 분기 (200/409/404/401/5xx 4-tuple 에 confirm_required 추가), §1.2 의 state machine 의사코드 (`attaching` → `confirm_required` arrow 추가), §4.4 의 reconnectGate switch 의사코드 (case 추가)
- **다음 한 step**:
  1. plan-0008 §1.1 의사결정 tree 의 step 4 에 새 분기 "→ 200 + unmatched > 0 → workspaceSwitcher.goAttachConfirm" 추가
  2. §1.2 의 state machine ASCII art 에 `confirm_required` transition arrow (attaching → markIdle + AttachConfirmModal mount) 추가
  3. §4.4 의 reconnectGate switch 의사코드에 case 추가
  4. §9 변경 이력에 `2026-05-17 (회귀 fix amend)` entry 추가 — commit `9bd2eea` 링크 + 변경된 §들 명시

### 4.3 ADR-0019 D5.4 의 amend — reattach confirm_required invariant 추가

- **상태**: ADR-0019 D5.4 (Initial entry attach recovery) 는 8-state machine 정의했으나 `confirm_required` state 가 없음. 본 session 코드는 markIdle + workspaceSwitcher 로 우회했지만, 명시 state 가 없는 게 enumeration-level 정합 부족.
- **관련 문서**: `docs/adr/0019-session-and-workspace-model.md` §D5.4
- **다음 한 step**: ADR-0019 변경 이력에 `2026-05-17 (reattach unmatched 회귀 fix)` entry — 두 옵션 중 선택:
  - (a) D5.4 의 reattach state 8개 외 별 modal-overlay 흐름으로 명시 (현 구현 — markIdle + workspaceSwitcher.goAttachConfirm).
  - (b) state 머신에 `confirm_required` state 신규 추가 + canMountApp / modalState derived 갱신 (더 큰 amend).
  현 코드 = (a) 방향. ADR 도 (a) 로 amend 가 가장 작은 변경.

### 4.4 직전 handover §4 잔여 — 본 session 진행 안 함

직전 handover (`...-canvas-tools-and-file-picker.md`) §4 의 항목들 — 본 session 에서 작업 안 함:
- §4.1 caption type — plan-0011 잔여 (CaptionNode 신규 + toolStore 'caption' 추가)
- §4.2 ADR-0033 asset endpoint BE land — image real upload UX
- §4.3 file-stat FE wire 검증 + Settings.picker_show_hidden Settings UI
- §4.4 File picker Stage 3 — 사용자 root 동적 추가
- §4.5 ADR-0018 D11 amend (restored_geom) Accepted + 구현
- §4.6 0048 / Undo-Redo manual E2E

§4.1 caption 이 가장 자연스러운 후보 (BE schema 정합 확인 + FE Slice-A2 직 후속).

### 4.5 Theme hot-reload chain 의 본 session 영향 평가 (방어적)

- **상태**: 본 session 종료 후 다른 worker 가 theme hot-reload 안정화 6 commit 진행 — `092c8e9` (XtermHost force remount) ~ `e006962` (xterm theme buffer preservation). silentReattach trigger 도 포함 (`6b02f65` 후 `40966ab` revert).
- **잠재 영향**: theme 변경 시 silentReattach 가 trigger 되도록 시도된 흐름 (`6b02f65`) 이 revert 됐으나, 본 session 의 silentReattach confirm_required 분기 (+page.svelte:167-172) 와 *호환되어야 함*. theme 관련 reattach 가 미래 다시 활성화되면 modal 진입 path 가 자동 적용됨.
- **다음 한 step**: 본 session 작업이 회귀 없는지만 확인 — `git log --oneline bde370e..HEAD -- codebase/frontend/src/lib/stores/sessionStore.svelte.ts codebase/frontend/src/lib/stores/reconnectGate.svelte.ts codebase/frontend/src/routes/+page.svelte` 로 본 3 파일에 다른 worker 의 후속 변경 있는지 확인.

---

## 5. 향후 작업

본 session 종료 시점 사용자 명시:
> "session context migration을 위한 handover 문서 작성"

→ 다음 session 의 작업 영역 = 사용자가 다음 session 시작 시 명시 브리핑 예정 (TBD). 본 handover §4 의 항목들이 후보.

가능한 후속 path:
- **§4.1 browser manual E2E** — 본 session fix 의 직접 검증 (가장 자연스러운 다음 step)
- **§4.2 plan-0008 amend** — coherence hard rule 정합
- **§4.3 ADR-0019 D5.4 amend**
- **§4.4 직전 handover §4 잔여** — 특히 caption type (plan-0011)
- 새 사용자 요구 (UX 회귀 / 새 기능 / 다른 batch)

---

## 6. 주의사항 / Gotchas

- **신규 invariant: reattach 의 `unmatched > 0` 은 silent 흡수 금지** — 본 session fix 핵심. 어떤 reattach 경로 (hint-based / silent visibilitychange / future WS-reconnect 등) 든 200 + unmatched > 0 시 `confirm_required` 반환 + AttachConfirmModal escalate. `WorkspaceSwitcher.tryAttach` (line 119-125) 와 1:1 정합. 회귀 차단을 위해 새 reattach call site 가 생기면 이 패턴 적용 필수.
- **`lastSilentReattachResult` 의 stale-block 회피 미구현** — `sessionStore.svelte.ts:506` 의 state 가 silent 의 `confirm_required` (그리고 in_use/not_found/unreachable 도) 결과를 영구 보관. `guardOutgoingMutation` 이 `kind !== 'success'` 면 mutation 차단 → 사용자가 modal 으로 resolve 해도 stale 차단 가능. **본 session 범위 외** — 별 후속 sprint 에서 `setActiveSession` 또는 `loadLayout` 에 clear 추가 검토. (pre-existing 패턴 — 본 fix 가 악화시키진 않음.)
- **AttachConfirmModal Cancel 시 active session 일관성** — `WorkspaceSwitcher.tryAttach` 의 기존 패턴 (`sessionStore.setActiveSession({name})` 먼저 호출) 정합. Cancel → goList → 사용자가 modal close 시 active 는 set 됐지만 layout 미로드 상태. pre-existing 패턴 — 변경 없음.
- **plan-0008 / ADR-0019 의 분기 미반영** — 본 session 의 code-side change 가 plan/ADR 측에 amend 안 됨 (§4.2 / §4.3). CLAUDE.md 의 "ADR ↔ plan/handover coherence" hard rule 위반 상태 — 다음 session 의 우선 정리 권장.
- **A11y: focus indicator 가 OS-default 로 fallback** — `bde370e` 가 19 rule 제거하면서 명시 `outline: none` 은 *추가하지 않음*. 즉 키보드 focus 시 브라우저 default outline 노출 (Chrome=파란 solid, Safari=검정 solid 등). 사용자가 "이 효과 자체 제거" 요구 충족 + a11y floor 유지. 만약 추후 default 도 거슬리면 별 패치로 `outline: none` 명시 + 별도 focus style 디자인.
- **의도적 dashed 유지 4 곳** — drag-drop "inside group" indicator (LayerTreeView:1028), point-spawn-ghost (Canvas:1397), image placeholder gray (ImageNode:129), file-pick drop zone gray (ImportSessionModal:398). 새 dashed border 추가 시 *focus ring 패턴* 인지 *visual cue* 인지 명확히 구분.
- **다른 worker 의 동시 commit 빈번** — 본 session 진행 중에도 XtermHost svelte parser 회피 4 commit (`084984c`~`7b400c3`) + 본 session 종료 후 theme hot-reload 6 commit (`092c8e9`~`e006962`). 다음 session 도 동일 가능성 — `git log --oneline bde370e..HEAD` 로 본 commit 후 다른 변경 확인.
- **PanelNode.svelte:536** 의 주석 "Multi-select — dashed 2px accent + 헤더 색조 강화" 는 실 코드 (`outline: none`) 와 부정합 — historical artifact. 본 session 에서 건드리지 않음. 추후 정리 시 주석 갱신 권장.
- **사용자가 거부한 접근**:
  - reattach 의 silent success 흡수 — panel 만 남기는 회귀로 명시 거부.
  - 버튼 focus 의 파란 dashed 시각 효과 자체 — UX 거슬림으로 명시 제거 요구.
- **사용자가 명시 결정**:
  - reattach unmatched 시 confirm dialog 노출 = "normal flow 와 동일하게 respawn dialog 가 떠야 함".
  - dashed focus ring = 효과 자체 제거 (의도적 visual 4 곳은 예외).

---

## 7. 새 session 시작 방법

이 문서를 받은 session 은 다음 순서로 부트스트랩한다:

1. **이 handover 문서 (`docs/reports/2026-05-17-session-handover-reattach-confirm-and-dashed-focus-ring.md`) 를 끝까지 읽는다**.
2. **`CLAUDE.md`** 를 읽는다 — 언어 컨벤션 (docs KO / code EN), ADR-before-code hard rule, MCP graph 우선, applyMutation 단일 entry, **path picker-only invariant**, **ADR ↔ plan coherence**.
3. **`docs/sketch.md`** + (옵션) `CONTEXT.md` (있으면) — 프로젝트 scope / MVP / threat model.
4. **직전 handover (`2026-05-17-session-handover-canvas-tools-and-file-picker.md`)** + 본 handover 의 §3 표 — 본 session 진입 시점의 state 와 본 session 의 fix delta.
5. **§4 의 진행중 작업** 중 사용자가 지정한 항목의 "다음 step" 부터 진행 — 미지정 시 §4.1 (browser manual E2E) 가 가장 자연스러운 후속.
6. **handover 작성 이후 변경 확인**: `git log --oneline bde370e..HEAD` — 본 session 종료 후 다른 worker 의 commit 있을 가능성. 특히 theme hot-reload chain 이 본 session 의 reattach/silentReattach 파일에 영향 줬는지 (§4.5 의 명령).

만약 §5 의 사용자 브리핑이 *§4 의 항목이 아닌 새 영역* 이면:
- 그 영역의 ADR 존재 여부 확인 (ADR-before-code hard rule).
- 없으면 grilling 진행 → ADR draft → 사용자 review → implementation step 분리.

---

_생성: `session-handover` skill v1_
