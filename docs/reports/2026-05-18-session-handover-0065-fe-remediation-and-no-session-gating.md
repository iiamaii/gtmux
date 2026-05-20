# Session Handover — 2026-05-18 — 0065 FE perf/logic remediation 전체 + no-session UI gating

> 이 문서는 `session-handover` skill 로 생성된 session 인수인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-18 (오전)
> - 생성 session 의 마지막 커밋: `f086e32` (feat(frontend/chrome): no-session 시 toolbar / 좌우 panel / shutdown 비활성화)
> - 이번 session 의 주요 주제: (a) `docs/reports/0065-frontend-performance-and-logic-review.md` 의 6 finding 전부 land (Phase 1 mechanical 4건 + Phase 2 correctness + ADR-0028 D11.1 amend + Phase 3 free_draw refactor) (b) 사용자 요구 — no-session 상태에서 toolbar / 좌우 panel / SessionMenu 의 Session shutdown 비활성화 (c) `browse` CLI 로 BE+browser E2E 검증.
> - 생성 직후 다른 worker land: `5240cb4` (ADR-0021 D7 amend ③ + 0068 work package) → `656f9d7` (BE-2 attach reverse index, 0066 Phase 4) → `4b1367d` (kill 흐름 trace) → `f086e32` (본 session — no-session gating) → `d26aa83` (state-machines SoT) → `23140d4` (mermaid fix). 동시 진행되는 batch 들 (BE 0066 chain, FE AttachConfirm 0069, Settings/Auth chrome) 와 commit 충돌 없이 본 작업 분리 land 완료.

---

## 1. 프로젝트 개요

- **이름**: gtmux
- **한 줄 정체성**: tmux 를 backend execution engine 으로 쓰는 single-user 의 web canvas workspace — tmux 가 process/session lifecycle 의 진실, FE 가 canvas layout 의 진실.
- **현재 phase / 단계**: **Stage 7+** — multi-session pivot 완료 + canvas tool 확장 + reattach 회귀 fix + dashed focus ring cleanup + theme hot-reload 안정화 + **0065 perf/logic 전 finding land + no-session UI gating** + (병행) BE 0066 4-phase remediation + (병행) AttachConfirm cancel recovery (0069).
- **침범 불가능한 invariants**:
  - **두 state 분리**: tmux state (mirror only) ↔ web state (FE 진실) — `docs/sketch.md` §4 + `CLAUDE.md`
  - **single-attach + no-takeover**: Webpage:Session = 1:1, 활성 session 강제 takeover 없음 — `docs/adr/0019-session-and-workspace-model.md` D3/D4
  - **control-mode integration**: tmux CLI shell-out 금지 — `docs/adr/0021-terminal-pool-and-mirror.md`
  - **ADR-before-code hard rule + ADR↔plan/handover coherence** — `CLAUDE.md`
  - **applyMutation 단일 entry** (ADR-0028 D11) — 모든 user-driven layout mutation 통과
  - **D11.1 (본 session 신규)** — `applyMutation` 이 `priorSnapshot` 옵션을 받으면 PUT 실패 시 `loadLayout(priorSnapshot)` 로 store 복원 책임. caller 가 optimistic update 했다는 signal 로 활용. failMessage 는 상태 변화 명시. ADR-0028 D11.1.
  - **path picker-only**: file_path item path 는 FilePickerModal 통과만 (ADR-0035 D1 amend)
  - **reattach 의 `unmatched > 0` silent 흡수 금지** — `confirm_required` 로 escalate (직전 session, 2026-05-17 fix). 본 session 의 FE-2 와 동형 shape.
  - **No-session UI gating (본 session 신규)** — `sessionStore.active === null` 시 Toolbar 12 도구 + LeftPanel/RightPanel tab + body + SessionMenu 의 Session shutdown + chromeShortcuts Cmd+N/Cmd+Shift+Q 모두 disabled. ActiveSessionDropdown + Titlebar 의 SessionMenu kebab + fold/expand 만 유지 (사용자 진입점).

---

## 2. 현재 session 요약

본 session 은 세 단계 작업 + 부수 검증:

### 2.1 Phase 1 — 0065 mechanical 4건 (commit `4415f76` → `c65f4fb` → `d5ed810` → `215c3c8`)

| Commit | Finding | 핵심 |
|---|---|---|
| `4415f76` | **FE-6 LineNode unmount listener leak** | `onDestroy` 추가 — drag 중 component unmount (session switch / item delete / layout reload) 시 `removeWindowListeners()` + state reset. pointerup/cancel 정상 종료 path 와 idempotent. (`codebase/frontend/src/lib/canvas/LineNode.svelte` +13) |
| `c65f4fb` | **FE-3 terminalPool.byId O(N)→O(1)** | `terminalsById = $state<SvelteMap<string, TerminalInfo>>(...)` 신규. `refresh()` 가 array + map 동시 갱신. `byId` 가 map.get. (`codebase/frontend/src/lib/stores/terminalPool.svelte.ts` +10/-2) |
| `d5ed810` | **FE-4 viewport debounce session-switch race** | `updateViewport` 가 예약 시점에 `{sessionName, snapshot}` closure 캡처. `#flushViewport(sessionName, viewport)` 가 active 와 sessionName 비교로 cross-session flush 폐기. `clear()` 가 pending timer 취소. (`codebase/frontend/src/lib/stores/sessionStore.svelte.ts` +15/-5) |
| `215c3c8` | **FE-5 PANE_OUT late-buffer O(k²)→O(k) + hot-path log dev-gate** | `LateBufferEntry = {chunks, total}` 로 running total → drop loop O(k). `DEBUG_PANE_OUT = import.meta.env.DEV` 게이트 — prod build 에서 hot-path 5 console.debug 모두 DCE (검증됨, §2.4). (`codebase/frontend/src/lib/ws/dispatcher.svelte.ts` +42/-22) |

### 2.2 Phase 2 — 0065 FE-2 correctness + ADR (commit `f564ce8` → `ae01d49`)

| Commit | 영역 | 핵심 |
|---|---|---|
| `f564ce8` | code | `sessionStore.applyMutation` failure path 에 `if (priorSnapshot !== null) this.loadLayout(priorSnapshot)` 신규. Canvas drag stop (`Canvas.svelte:1113`) 이 이미 priorSnapshot 전달 중이라 자동 rollback. failMessage "Drag commit failed — reverted to previous position." 갱신. (`sessionStore.svelte.ts` +11/-2 + `Canvas.svelte` +2/-1) |
| `ae01d49` | ADR | **ADR-0028 D11.1** subsection 신규 — `applyMutation` 의 `priorSnapshot` 의미 양방향 확장 (history capture + failure rollback). 변경 이력 entry 동시 추가. 동형 invariant: 2026-05-17 reattach `unmatched > 0` silent 흡수 금지. (`docs/adr/0028-undo-redo-policy.md` +21) |

**Scope 결정**: 0065 doc 의 명시 FE-2 = Canvas drag stop. zStore/`#commit` + PanelNode/`onResizeEnd` 도 같은 shape (optimistic update + fire-and-forget) 이지만 본 session 미터치 — ADR D11.1 의 "적용 후보" 섹션에 latent로 명시. 후속 sprint 권장.

### 2.3 Phase 3 — 0065 FE-1 free_draw refactor (commit `d55f372`)

`Canvas.svelte` 의 free_draw 입력 흐름 3 단계 분리:

1. **비반응 buffer**: `freeDrawPoints` / `freeDrawPointsLocal` 을 plain `let` array 로 (DragState 에서 제거). pointermove 가 spread 없이 `.push(...)` — O(1) per sample, $state flush 무.
2. **rAF coalesce**: `freeDrawFrame = $state(0)` + `freeDrawRafId`. ghostPreview 의 free_draw 분기가 `freeDrawFrame` 만 reactive dep 으로 읽음 → 한 frame 의 N 개 pointer event 가 1회 재계산.
3. **최소거리 prune**: `FREE_DRAW_MIN_POINT_DELTA_SQ = 0.5 * 0.5` — 연속 sample 의 screen-px² 거리가 0.25 미만이면 drop. 100-1000 Hz pointer event 가 1/4~1/100 로 압축.

저장 cap (`FREE_DRAW_MAX_POINTS = 5000`, ADR-0018 D4) 그대로. 입력 cap 분리 정책 도입 안 함 — 현 구조로 충분, ADR-0018 D4 amend 회피. `FreeDrawNode.localPath` $derived (commit 후 item 렌더) 는 hot path 아니라 미터치.

(`codebase/frontend/src/lib/canvas/Canvas.svelte` +73/-27)

### 2.4 Static / build-time 검증

`pnpm check 0 errors / pnpm build OK` 각 Phase 후 통과. 추가:

- **FE-5 prod console DCE 검증** (production bundle grep):
  - `PANE_OUT pane=` / `late-buffer` / `subscriber(s)` / `buffered chunk` / `no buffered bytes` / `subscriber=%d` **모두 0건** in `dist/assets/index-*.js`. Vite 가 `import.meta.env.DEV` 를 `false` literal 로 inline → `if (false) console.debug(...)` DCE.
- **FE-1/2/3/6 prod fingerprint**: `requestAnimationFrame`, `reverted to previous position`, `terminalsById`, LineNode 의 `removeEventListener` 모두 bundle 에 존재.
- **dev server boot**: `vite` 300ms ready · HTML 1832 bytes HTTP 200.

### 2.5 No-session UI gating (commit `f086e32`) — 사용자 요구

사용자 verbatim:
> "no session page에서는 menu button, session state button(toolbar 왼쪽에 있는)을 제외한 component들 (toolbar, 좌/우 패널)은 비활성화 되도록 하는게 좋지 않을까. → 사용자가 session을 연결하도록 유도하며 로직 충돌을 방지하기 위해. & menu button에서도 session shutdown, delete current sesssion, export session은 no session에서 실행 불가한 기능으로 함께 비활성화 해야함."

**비활성화 (sessionStore.active === null 시)**:

| File | 변경 |
|---|---|
| `lib/toolbar/Toolbar2.svelte` | `noActiveSession = $derived(sessionStore.active === null)` + 12 도구 (Select/Hand/Terminal/Rect/Ellipse/Line/FreeDraw/Text/Note/Image/Document/FilePath) `disabled` + title "Connect a session to use canvas tools". Undo/Redo 는 historyStore 자동 disable. |
| `lib/sidebar/LeftPanel.svelte` | Layers/Terminals tab + rail icon `disabled`. body 에 `inert` + `.no-session` (opacity 0.4 + pointer-events:none). fold/expand 그대로. |
| `lib/chrome/RightPanel.svelte` | Inspect tab + rail icon `disabled`. body `inert` + dimming. fold/expand 그대로. |
| `lib/chrome/SessionMenu.svelte` | Session shutdown 에 `disabled={sessionStore.active === null}` 추가 (Export/Delete 와 정합 — 이미 적용된 패턴). |
| `lib/keyboard/chromeShortcuts.svelte.ts` | Cmd+N (New terminal) + Cmd+Shift+Q (Shutdown) handler 가 `sessionStore.active === null` 시 no-op consume (`return true`). 도구만 highlight 되는 confusing UX 차단. Settings/LeftPanel toggle/RightPanel toggle 은 chrome 정리용으로 유지. |

**유지 항목**: Titlebar 의 SessionMenu kebab (사용자 진입점), Toolbar 좌측 ActiveSessionDropdown ("No session" placeholder + 클릭으로 SessionListModal), SessionMenu 의 Switch workspace / Import / Sign out / Rotate token / Settings / About, 좌/우 panel 의 fold/expand 버튼 + resize handle.

(5 file +68/-6)

### 2.6 E2E browser 검증 (browse CLI + BE demo)

- BE `demo` 서버 이미 running (pid 411, port 9998, `~/.local/state/gtmux/demo.token` = `zM-N4QblicLmbfenMKbBZ1nxYy1pfXhYaZJQaVjaIRs`).
- `browse goto / → /auth 자동 redirect → fill token → Continue → / → SessionListModal (dismissable=false) 자동 노출` = no-session state 진입.
- Screenshot (`/tmp/gtmux-no-session.png`) 확인: 12 도구 페이드 + Undo/Redo 페이드 + LeftPanel/RightPanel body opacity 0.4 + ActiveSessionDropdown "No session".
- Playwright locator 출력 (definitive proof) — disabled attribute + title 모두 정상 적용:
  - Terminal tool `<button disabled aria-pressed="false" title="Connect a session to use canvas tools">`
  - LeftPanel Terminals tab `<button disabled role="tab" title="Connect a session to view terminals">`
  - RightPanel Inspect tab `<button disabled role="tab" title="Connect a session to inspect items">`
  - 모두 click 거부 — "element is not enabled".
- **검증 못한 항목**: SessionMenu 의 Session shutdown disabled — modal backdrop 가 kebab click intercept (dismissable=false). 다만 추가된 disabled 패턴이 기존 Export/Delete 와 100% identical 이라 동치 신뢰.

### 결정사항

- **Phase 진입 순서 = 위험-적은 mechanical 먼저** (doc 의 impact-ordering 1→6 역순). 사용자 즉시 confirm 없이 진행 — strategy 단계에서 명시 후 action.
- **FE-2 scope 좁힘** — doc 의 명시 = Canvas drag stop 만. zStore/PanelNode resize 도 같은 shape 이나 0065 외라 ADR D11.1 의 "적용 후보 (latent)" 섹션에 노트만. 사용자 즉시 confirm 없이 진행.
- **FE-1 simplification 알고리즘 = min-distance prune** (0.5 px² 임계) over Douglas-Peucker — 더 단순, 결과 충분, 외부 lib 무. 사용자 즉시 confirm 없이 진행 (no-question 모드).
- **FE-1 입력 cap 분리 안 함** — 저장 cap 5000 (ADR-0018 D4) 그대로. ADR amend 회피.
- **Phase 2 commit 분리** — code (`f564ce8`) + ADR (`ae01d49`) 별 commit. coherence hard rule 준수.
- **No-session gating 의 keyboard shortcut 도 게이트** — Cmd+N / Cmd+Shift+Q. 도구는 disabled 인데 키보드는 fire 되면 inconsistent.
- **No-session gating body 처리 = inert + opacity 0.4 + pointer-events:none** — inert 가 Playwright snapshot 에서 안 가려질 가능성 있으나 CSS 가 동시 차단.

### 변경된 파일 (8 commit 누적)

| 파일 | 변경 요약 |
|---|---|
| `codebase/frontend/src/lib/canvas/LineNode.svelte` | FE-6 onDestroy cleanup (+13) |
| `codebase/frontend/src/lib/stores/terminalPool.svelte.ts` | FE-3 SvelteMap byId (+10/-2) |
| `codebase/frontend/src/lib/stores/sessionStore.svelte.ts` | FE-4 viewport race snapshot (+15/-5) + FE-2 applyMutation rollback (+11/-2) |
| `codebase/frontend/src/lib/ws/dispatcher.svelte.ts` | FE-5 late buffer + log gate (+42/-22) |
| `codebase/frontend/src/lib/canvas/Canvas.svelte` | FE-1 free_draw refactor (+73/-27) + FE-2 failMessage (+2/-1) |
| `codebase/frontend/src/lib/toolbar/Toolbar2.svelte` | No-session: 12 도구 disabled (+8/-2) |
| `codebase/frontend/src/lib/sidebar/LeftPanel.svelte` | No-session: tab/rail disabled + body inert (+28/-2) |
| `codebase/frontend/src/lib/chrome/RightPanel.svelte` | No-session: tab/rail disabled + body inert (+22/-2) |
| `codebase/frontend/src/lib/chrome/SessionMenu.svelte` | No-session: Shutdown disabled (+3) |
| `codebase/frontend/src/lib/keyboard/chromeShortcuts.svelte.ts` | No-session: Cmd+N/Cmd+Shift+Q gate (+7) |
| `docs/adr/0028-undo-redo-policy.md` | D11.1 amend + 변경 이력 entry (+21) |

미커밋: **없음 (본 session 범위)**. `git status` 의 dirty file 들 (`AttachConfirmModal.svelte`, `WorkspaceSwitcher.svelte`, `http/sessions.ts`, `stores/workspaceSwitcher.svelte.ts`, `types/sessions.ts`, `adr/0019-session-and-workspace-model.md`, `reports/0069-session-attach-confirm-cancel-recovery.md`) 는 모두 다른 worker 의 AttachConfirm cancel recovery batch — **본 session 영역 아님**.

---

## 3. 주요 참조 자료

| 영역 | 경로 | 왜 읽어야 하는가 |
|---|---|---|
| 프로젝트 instructions | `CLAUDE.md` | 언어 (docs KO / code EN), ADR-before-code, ADR↔plan coherence, MCP graph 우선, applyMutation 단일 entry, path picker-only |
| 본 session 의 source review | `docs/reports/0065-frontend-performance-and-logic-review.md` | FE-1~FE-6 finding 정본. 본 session 이 6/6 land. |
| 직전 handover (cold-pickup base) | `docs/reports/2026-05-17-session-handover-reattach-confirm-and-dashed-focus-ring.md` | reattach `unmatched > 0` invariant + dashed focus ring cleanup. 본 session 의 FE-2 (D11.1) 와 동형 shape (silent absorb 금지). §4 의 잔여 항목들이 본 handover §5 의 후보. |
| **신규 invariant — D11.1** | `docs/adr/0028-undo-redo-policy.md` §D11.1 + 변경 이력 마지막-1 entry | 본 session 의 핵심 contract. caller 가 priorSnapshot 명시 시 failure rollback 자동. zStore / PanelNode resize 의 latent same-shape 도 명시. |
| 활성 plan | `docs/plans/0011-component-design-batch-caption-document.md` | caption / document FE Slice. document 는 직전 session 들이 ship — caption 미진행 (직전 handover §4.1, 본 handover §5.1). |
| 병행 BE batch | `docs/reports/0066-frontend-performance-and-logic-review.md` (가칭) + `docs/reports/0067-be-remediation-plan.md` + `docs/reports/0068-attach-reverse-index-work-package.md` | BE 0066 4-phase remediation (Phase 1~4 ship 완료, 본 session 시간대 병행). |
| 병행 AttachConfirm batch | `docs/reports/0069-session-attach-confirm-cancel-recovery.md` (untracked) | 다른 worker 의 AttachConfirm cancel recovery 작업. 본 session 외. |
| ADR-0018 D4 | `docs/adr/0018-canvas-item-data-model.md` §D4 | free_draw point cap 5000. 본 session 의 FE-1 이 cap 정책 유지 (amend 무). |
| 어드민 / BE 운용 | `~/.local/state/gtmux/demo.token` | BE demo 의 인증 token (E2E 검증용). |

---

## 4. 진행중인 작업

본 session 의 *자체* 작업은 모두 commit 완료. *다음 session 이 이어야 할* 항목들:

### 4.1 Browser manual E2E — Phase 1+2+3 의 timing/perf 류 시나리오 (본 session 미실행)

- **상태**: 본 session 에서 static 검증 + dev server boot + no-session UI 의 disabled attribute 까지만 확인. 아래는 미실행.
- **관련 문서**: `docs/reports/0065-frontend-performance-and-logic-review.md` 각 finding 의 "위험 흐름" 섹션.
- **관련 파일**: 본 handover §2.1~§2.3 의 변경 파일들.
- **다음 한 step**:
  1. **FE-1**: dev tool Performance 탭 → drawing pad 에 1000+ point stroke → fps drop 측정. min-distance prune 으로 stroke 의 실 point 수가 cap 5000 보다 훨씬 작은지 commit 후 layout JSON 으로 확인.
  2. **FE-2**: DevTools Network → Offline → 임의 panel drag drop → "Drag commit failed — reverted to previous position." toast + 위치 회귀 확인.
  3. **FE-4**: 2 session 준비 → session A 에서 pan/zoom → 200 ms 내 session B switch → 500 ms 대기 → 각 session 의 layout file (`~/.local/state/gtmux/<name>.layout.json`) 의 viewport 가 cross-session 으로 안 묻는지 확인.
  4. **FE-6**: endpoint drag 진입 → Delete 키로 self-delete → DevTools `getEventListeners(window)` 로 누수 0 확인.
  5. **No-session SessionMenu Shutdown disabled**: 1번 검증 후 session attach → detach → SessionMenu kebab 열어 Shutdown disabled 확인.

### 4.2 zStore / PanelNode resize 의 D11.1 적용 (latent same-shape)

- **상태**: ADR-0028 D11.1 의 "적용 후보" 섹션에 명시. 코드 미터치.
- **관련 문서**: `docs/adr/0028-undo-redo-policy.md` §D11.1.
- **관련 파일**: `codebase/frontend/src/lib/stores/zStore.svelte.ts:80-115` (`#mutate` / `#applyTwo` / `#commit`) · `codebase/frontend/src/lib/canvas/PanelNode.svelte:122-145` (`onResizeEnd`).
- **다음 한 step**:
  1. `zStore.#mutate` 와 `#applyTwo` 가 `sessionStore.items.set(...)` 직전에 `const priorSnapshot = sessionStore.layoutSnapshot()` 캡처 → `#commit(priorSnapshot, mutator)` 시그니처 변경 → `#commit` 이 `applyMutation(mutator, { ..., priorSnapshot })`. failMessage "Z order change failed — reverted to previous order."
  2. `PanelNode.onResizeEnd` 가 NodeResizer 가 DOM 갱신 *전* (store w/h 는 PRE 상태) `sessionStore.layoutSnapshot()` 캡처 → `applyMutation(..., { priorSnapshot, failMessage: 'Resize failed — reverted to previous size.' })`.
  3. **주의**: `PanelNode.svelte` 는 다른 worker (auto-kill setting) 가 동시 작업 중일 수 있음. `git status` 로 확인 후 충돌 회피.
  4. `pnpm check` + `pnpm build` 통과 후 commit.

### 4.3 plan-0008 의 §1.1 / §1.2 / §4.4 amend (직전 handover §4.2 이월 — 미진행)

- **상태**: 2026-05-17 의 reattach `confirm_required` fix 가 plan-0008 의사코드 와 정합 어긋남. coherence hard rule 위반 *계속*.
- **관련 문서**: `docs/plans/0008-session-attach-recovery-impl.md` §1.1 / §1.2 / §4.4 / §9.
- **다음 한 step**: 직전 handover (`2026-05-17-session-handover-reattach-confirm-and-dashed-focus-ring.md`) §4.2 의 4 step 그대로.

### 4.4 ADR-0019 D5.4 amend — reattach confirm_required state 명시 (직전 handover §4.3 이월)

- **상태**: 직전 handover 의 (a) modal-overlay 흐름 명시 옵션이 가장 작은 amend.
- **관련 문서**: `docs/adr/0019-session-and-workspace-model.md` §D5.4.
- **주의**: ADR-0019 는 *다른 worker* 가 동시 작업 중 (`git status` 에 `M docs/adr/0019-session-and-workspace-model.md` 표시 — 본 session 미터치). 충돌 회피 위해 다른 worker 의 batch 종료 후 진입 권장.
- **다음 한 step**: 직전 handover §4.3 옵션 (a).

### 4.5 직전 handover (canvas-tools-and-file-picker) §4 잔여 — 본 session 미진행

- **상태**: §4.1 caption type / §4.2 ADR-0033 asset endpoint BE land / §4.3 file-stat FE wire 검증 + Settings.picker_show_hidden UI / §4.4 File picker Stage 3 / §4.5 ADR-0018 D11 amend (restored_geom) Accepted + 구현 / §4.6 0048 / Undo-Redo manual E2E.
- **관련 문서**: `docs/reports/2026-05-17-session-handover-canvas-tools-and-file-picker.md` §4.
- **다음 한 step**: 각 항목별로 별 sprint. caption (plan-0011) 이 가장 자연스러운 후보.

---

## 5. 향후 작업

### 5.1 caption type 도입 (plan-0011 잔여)

- **목표**: `CaptionNode.svelte` 신규 + `toolStore.tools` 에 `'caption'` 추가 + `Toolbar2.svelte` 의 GROUPS 에 entry. document 와 동일 한 작은 type — 별도 ADR 무관.
- **관련 문서**: `docs/plans/0011-component-design-batch-caption-document.md`.
- **선행 조건**: 없음 — 본 session 의 작업 과 독립.
- **예상 진입 지점**: `codebase/frontend/src/lib/canvas/DocumentNode.svelte` 를 reference 로 CaptionNode 신규.

### 5.2 0068 의 BE attach reverse index 후속 (병행 worker 의 ship 후)

- **목표**: `656f9d7` 가 BE-2 reverse index ship — FE 측 활용 (있다면) 확인.
- **관련 문서**: `docs/reports/0068-attach-reverse-index-work-package.md`.
- **선행 조건**: BE 0066 chain 의 Phase 5+ 가 있는지 확인.
- **예상 진입 지점**: `docs/reports/0067-be-remediation-plan.md` 의 Phase 5 항목.

### 5.3 AttachConfirm cancel recovery (0069) 의 본 session 영향 평가

- **목표**: 다른 worker 가 진행중 (untracked `docs/reports/0069-...`). 본 session 의 FE-2 (`applyMutation` D11.1) + reattach `confirm_required` invariant 와의 정합 확인.
- **관련 문서**: `docs/reports/0069-session-attach-confirm-cancel-recovery.md` (작성중).
- **선행 조건**: 다른 worker 의 batch 종료.
- **예상 진입 지점**: 0069 doc 의 §4 또는 commit chain.

---

## 6. 주의사항 / Gotchas

- **신규 invariant — ADR-0028 D11.1**: `applyMutation` 에 `priorSnapshot` 전달 = caller 가 호출 *전* store 를 optimistic 갱신했다는 signal. PUT 실패 시 `loadLayout(priorSnapshot)` 자동 호출. 새 optimistic-update 패턴 추가 시 *반드시* `priorSnapshot` 전달 + failMessage 에 "reverted to ..." 명시. WS-driven dispatcher 처럼 optimistic 없는 path 는 priorSnapshot 미전달 그대로.
- **신규 invariant — No-session UI gating**: `sessionStore.active === null` 시 비활성 영역 = Toolbar 12 도구 + LeftPanel/RightPanel tab + body + SessionMenu 의 Session shutdown/Export/Delete + chromeShortcuts Cmd+N / Cmd+Shift+Q. *유지* 영역 = ActiveSessionDropdown (Toolbar 좌측), Titlebar 의 SessionMenu kebab, fold/expand 버튼, resize handle. 새 도구/버튼 추가 시 같은 정책 적용 필요.
- **inert 속성 — Playwright 검증 한계**: `inert` HTML 속성을 panel body 에 추가했으나 Playwright snapshot 의 a11y 트리에 inert subtree 가 그대로 표시됨. CSS `pointer-events: none + opacity: 0.4` 가 동시 차단해 사용자 click 차단은 *시각적 + 인터랙션* 모두 보장. 그러나 keyboard tab focus 가 inert 무시하고 들어가지 않는지는 미검증 — 별 sprint 에서 직접 keyboard tab navigation 확인 권장.
- **chromeShortcuts 일관성**: 버튼 disabled 와 keyboard shortcut gating 은 항상 같이 가야 함. 새 shortcut 추가 시 동일 정책. Settings (`Cmd+,`) / LeftPanel toggle (`Cmd+Shift+L`) / RightPanel toggle (`Cmd+Shift+I`) 는 *chrome 정리* 용이라 no-session 에서도 active 유지.
- **다른 worker batch 동시 진행 — 충돌 회피**: 본 session 중 BE 0066 4-phase + FE AttachConfirm 0069 + Settings/Auth chrome batch 가 동시 진행. `PanelNode.svelte` / `AttachConfirmModal.svelte` / `WorkspaceSwitcher.svelte` / `sessionStore.svelte.ts` (Reattach 영역) / `+page.svelte` 등 hot file 작업 시 `git status` 로 사전 확인 필수. 본 session 의 `lib/stores/sessionStore.svelte.ts` 는 D11.1 이후 다른 worker 가 reattach 영역 더 손댐 — `git diff <new-head>..HEAD` 비교 후 진행.
- **BE demo 서버 항상 running**: pid 411, port 9998. token = `~/.local/state/gtmux/demo.token`. E2E 검증 시 별도 BE 기동 무필요. **본 session 의 browse cookie 가 BE 에 남아있을 가능성** — 다음 session 이 browse 로 진입하면 이미 인증된 상태일 수 있음. clean state 원하면 `browse cookies clear` 또는 새 incognito 컨텍스트.
- **browse CLI 사용법**: `/Users/ws/Desktop/projects/termcanvas/dist-cli/browse <command>` — goto / snapshot / text / screenshot / click / fill / hover / scroll / press / select / cookies / status / tabs / tab / stop. locator 는 `@e1`, `@e2` 등 snapshot 출력의 식별자. **eval 없음** — JS 평가 불가능. DOM HTML 보고 싶으면 Playwright 의 click 에러 메시지에 locator resolved HTML 이 노출되는 패턴 활용 가능.
- **0065 doc 의 우선순위 1→6 = 영향 큰 순**: 본 session 은 *위험 적은 mechanical 먼저* (역순) 진행. 다른 sprint 에서 같은 review doc 진입 시 ordering 정책 선택 의식 필요.
- **사용자 명시 거부**: (직전 session 이월) reattach 의 silent success 흡수 / 파란 dashed focus ring 시각효과. **본 session 의 명시 결정**: no-session 에서 chrome 다 활성화 보이는 것 (혼란 + 로직 충돌) → 비활성화 강요.

---

## 7. 새 session 시작 방법

이 문서를 받은 session 은 다음 순서로 부트스트랩한다:

1. **이 handover 문서 (`docs/reports/2026-05-18-session-handover-0065-fe-remediation-and-no-session-gating.md`) 끝까지 읽는다**.
2. **`CLAUDE.md`** 를 읽는다 — 언어 컨벤션, ADR-before-code hard rule, MCP graph 우선, applyMutation 단일 entry, path picker-only, **D11.1 (본 session 추가)**, **No-session UI gating (본 session 추가)**.
3. **`docs/sketch.md`** + (옵션) `CONTEXT.md` 가 있으면 — 프로젝트 scope.
4. **직전 handover (`2026-05-17-session-handover-reattach-confirm-and-dashed-focus-ring.md`)** + 본 handover 의 §3 표 — 본 session 진입 시점의 state 와 본 session 의 delta.
5. **§4 의 진행중 작업 첫 항목 (4.1 browser manual E2E) 의 "다음 step"** 또는 사용자 별 지시.
6. **handover 작성 이후 변경 확인**: `git log --oneline 23140d4..HEAD` — 본 session 종료 시점의 main HEAD 가 `23140d4` (mermaid syntax fix). 그 이후 BE 0066 / FE 0069 / 기타 batch 가 추가됐을 가능성. 특히 본 session 의 영향 파일 (`Canvas.svelte`, `sessionStore.svelte.ts`, `Toolbar2.svelte`, `LeftPanel.svelte`, `RightPanel.svelte`, `SessionMenu.svelte`, `chromeShortcuts.svelte.ts`, `LineNode.svelte`, `terminalPool.svelte.ts`, `dispatcher.svelte.ts`, `docs/adr/0028-undo-redo-policy.md`) 에 다른 worker 의 추가 변경 있는지 확인.

만약 §5 의 사용자 브리핑이 *§4 의 항목이 아닌 새 영역* 이면:
- 그 영역의 ADR 존재 여부 확인 (ADR-before-code).
- 없으면 grilling → ADR draft → 사용자 review → implementation 분리.

---

_생성: `session-handover` skill v1_
