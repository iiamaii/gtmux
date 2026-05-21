# ADR-0028 — Undo / Redo 정책 (canvas layout mutation)

- 상태: **Accepted** (2026-05-17 — D11 audit 통과 후 promote)
- 정본 plan: 별도 plan TBD (구현은 본 ADR Accepted 후 별 plan 으로 분리)
- 관련 ADR: ADR-0018 (canvas item v2 — items[] discriminated union), ADR-0019 (single-attach + layout etag), ADR-0021 (terminal pool — `kill_terminal=false` G25), ADR-0024 (layer tree + z-index), ADR-0027 (Inspector multi-select)
- 관련 commit: `64ef296` (lasso → Backspace 회귀 fix, 본 ADR 의 transient prerequisite)
- 작성자: agent (system-architect role) — 사용자 요구 1번 선택 + D1 sub-decision 권장 합의

---

## Context

현재 canvas mutation 은 모두 `mutateLayout(name, transform)` 한 entry point 로 통과 (`http/sessions.ts:315`). Inspector 의 broadcastMutation, alignment 의 applyAlignMutation, Canvas 의 drag commit / delete, ContextMenu 의 onDeleteItem 등 모든 mutation 이 단일 PUT → etag rebase 한 번 → store loadLayout 패턴. 이미 transactional. 그러나 **이전 상태로 되돌리는 affordance 없음** — 사용자가 잘못된 align/delete/edit 직후 회복 수단 없어 UX 불완전.

별도로 ADR-0021 G25 의 `kill_terminal=false` (delete 시 terminal pool 보존) 가 **layout 도메인과 tmux 도메인의 분리** 를 명시. 본 ADR 의 핵심 invariant 도 이것 — Undo 는 layout 도메인의 mutation 만 reverse, tmux/pool/session lifecycle 은 손대지 않음.

### 비-목표

- terminal 입력 (xterm.js → tmux) 의 undo — tmux 도메인, ADR-0021 영역, 본 ADR 범위 밖
- session lifecycle (create / delete / attach / detach) 의 undo
- File system mutation (file_path open 등 ADR-0023) 의 undo
- 다른 webpage 의 mutation 의 undo (본 webpage 의 history 만 stack)
- BE persistence (server-side history) — D2 참조

---

## Decisions

### D1 — Scope: canvas layout mutation only

Undo / redo 의 대상 mutation:
- Inspector edit (geometry x/y/w/h/z, label, color, line endpoint, state visible/locked/minimized)
- Alignment / distribute (multi-select)
- Drag commit (single / multi)
- Item 생성 (Toolbar / ContextMenu add)
- Item 제거 (Backspace / Delete / ContextMenu)
- Layer tree drag reorder / reparent (ADR-0024 의 parent_id 변경)
- Text align / vertical align mutation

대상 외:
- terminal 입력
- session attach / detach / create / delete
- viewport pan / zoom
- WS subscription / heartbeat 등 transport 상태

#### D1.1 — Undo effect = layout snapshot 복원 only

Undo / redo 의 **effect 는 server-side layout file 의 snapshot 복원만**. tmux pane lifecycle, terminal pool entry, session lock, attached_sessions 등 부수 server-state 는 undo 가 reverse 하지 않음.

근거: ADR-0021 G25 의 `kill_terminal=false` 와 정합. Layout 도메인과 tmux 도메인 분리는 본 프로젝트의 core invariant.

#### D1.2 — Terminal panel undo 의 unmatched UUID 처리

Undo 가 terminal panel 의 add 인 경우:
- pool 에 같은 UUID 잔존 → BE 가 matched 응답, mirror 자연 회복.
- pool 에서 UUID 사라진 경우 (다른 webpage 가 kill 했거나 server restart 직후 등) → BE 가 unmatched UUID 응답 (ADR-0018 D6 의 attach_confirm path).

**unmatched 시 처리**: confirm modal 자동 띄우지 않음. 대신 **etag mismatch 와 동급 안전 처리** — toast "Cannot undo — terminal removed by another webpage" + history stack reset. 자동 spawn 안 함 (사용자 의도 추정 금지, undo 는 단순 reverse 의미).

근거: Cmd+Z 한 번에 confirm modal 띄우면 UX 깨짐. 또한 새 terminal spawn 은 implicit side-effect 라 단순 reverse 가 아님.

### D2 — History store: client memory only

History stack 은 **현 webpage 의 in-memory** (sessionStore 의 svelte $state). reload 시 손실. 단일-user 단일-webpage 사용을 default 가정 (ADR-0019 D3 single-attach).

대안 검토:
- (b) Server-persisted (per session file 의 history field): 구현 복잡 (BE schema 변경 + endpoint + size cap), multi-webpage attach 시 충돌 정책 추가 필요. 본 프로젝트는 단일-user 라 비용 대비 가치 낮음.
- (c) Hybrid (client memory + opt-in snapshot button): 추가 UX 부담. snapshot 은 향후 별 ADR 로 분리 가능.

→ (a) Client memory only 채택. 후속 amend 로 (b)/(c) 도입 여지 명시.

### D3 — Granularity: per `mutateLayout` PUT = 1 entry

History stack 의 entry 단위 = **하나의 사용자 액션 = 하나의 `mutateLayout` PUT = 1 stack entry**. drag 중의 매 frame, typing 중의 매 keystroke 는 entry 가 아님 — commit 시점만.

근거: 모든 mutation 이 이미 `mutateLayout` 단일 entry point 를 통과. drag 의 onnodedragstop, Inspector 의 blur/Enter, alignment 의 클릭 등 commit 단위가 명확.

### D4 — Multi-session: per-session history

History stack 은 **active session 별로 독립**. session 전환 시 직전 session 의 stack 은 보존되지 않음 (다음 attach 시 빈 stack 으로 재시작). cross-session undo 없음.

근거: ADR-0019 의 session 분리 정합. multi-session 의 history merge 는 의도 모호 — 사용자가 session A 에서 mutate 후 session B 로 전환 후 Cmd+Z 누르면 어느 session 의 액션을 undo 해야 할지 정의 모호.

### D5 — Stack 크기: 50 entries (FIFO)

Undo stack 의 capacity = **50**. 51 번째 push 시 oldest entry evict. redo stack 도 동일 cap.

근거: 메모리 footprint = layout snapshot * 50. 일반 session 의 items.length ~50, item size ~200byte → 1 snapshot ~10KB, 50 entries ~500KB. 단일 webpage 의 메모리 부담 미미. 일반 사용자 의 undo depth 5~10 시나리오 충분 cover.

### D6 — Reload 시 history 손실 명시

`window.beforeunload` 시 history flush 없음. reload / navigation 후 stack 은 빈 상태.

근거: D2 (client memory only) 의 자연 귀결. 사용자에게는 ADR / UI 의 hint 로 명시 (toolbar tooltip 또는 onboarding).

### D7 — Undo unit: full layout snapshot

Stack entry = **`CanvasLayout` snapshot (`{ schema_version, groups, items, viewport }`)**. diff 또는 inverse-op 패턴 채택 안 함.

근거:
- mutateLayout 이 이미 full PUT 패턴. undo 도 동일 PUT — 구현 일관.
- diff 추적은 ADR-0018 D1 의 discriminated union 위 type-aware 변환 필요 — 복잡도 큼 (item 추가/제거/필드별 diff).
- 50 entry × 10KB = 500KB — full snapshot OK.

### D8 — Keybind

- **Cmd+Z** (mac) / **Ctrl+Z** (others): undo
- **Cmd+Shift+Z** (mac) / **Ctrl+Y** (others): redo
- xterm / `<input>` / `<textarea>` / `[contenteditable]` focus 시 키바인드 무시 (Canvas.svelte 의 isEditableFocused 패턴 재활용).

UI affordance: Toolbar 의 undo/redo button 후속 (별 plan). 키바인드는 ship 1 단계.

### D9 — Conflict policy: etag mismatch → toast + history reset

Undo 의 PUT 이 EtagMismatchError 던지면:
1. mutateLayout 의 자동 rebase 시도 (1회) 가 이미 적용됨.
2. 그래도 mismatch 면 toast "Cannot undo — layout changed by another source" + 양쪽 stack (undo / redo) reset.

근거: 다른 webpage / API 가 layout 을 mutate 한 후엔 history 의 PRE-state 가 더 이상 의미 없음. 사용자에게 명시적 알림 + 신뢰 회복.

### D10 — Redo stack drop on new mutation

Undo 직후 redo stack 이 있는 상태에서 **새 mutation 발생** → redo stack drop. 일반 IDE/Figma 패턴 정합.

### D11 — Mutation entry point 통일 보장

본 ADR ship 의 prerequisite — **모든 layout mutation 이 `mutateLayout` 을 통과해야 함** (직접 putLayout 우회 금지). 일부 path (예: Layer tree drag reorder) 가 putLayout 을 직접 호출하면 history 가 그 mutation 을 capture 못 함. 본 ADR 의 plan 단계에서 audit 필요.

### D11.1 — Optimistic update 의 failure rollback 계약 (2026-05-17 amend)

`applyMutation` 은 caller 가 *호출 전* store 를 optimistic 갱신하는 path (drag-stop, NodeResizer, z-order 등) 를 지원하기 위해 `priorSnapshot` 옵션을 가진다. 직전 정책은 `priorSnapshot` 을 history capture 의 PRE-state 입력으로만 사용하고, PUT 실패 시 store 는 optimistic 상태 그대로 두었다 — 사용자는 변경된 상태를 보지만 BE 는 옛 상태로 남아 새로고침 / 재진입 시 *조용한 회귀* 발생.

**계약 (amend)**:

- `priorSnapshot` 명시 = caller 가 호출 *전* store 를 optimistic 갱신했다는 signal.
- `mutateLayout` 실패 catch path 에서 `loadLayout(priorSnapshot)` 호출 → store 가 PRE-optimistic 상태로 복원. SvelteFlow 의 `bind:nodes` 양방향 sync 가 DOM 까지 자연 회복.
- `priorSnapshot` 미지정 = optimistic update 없는 path (Inspector edit, item create 직후 setM 등) — 실패해도 store 는 변동 없어 별도 복원 무필요.
- toast 의 `failMessage` 는 *상태 변화를 명시* (예: "Drag commit failed — reverted to previous position.") — 사용자가 자기 액션이 회귀된 사실을 인지.

본 계약은 2026-05-17 의 reattach `unmatched > 0` silent 흡수 금지 invariant 와 동형 — *FE 가 BE truth 와 조용히 desync 되는 회귀 차단* 이라는 같은 모양이다.

**현재 적용 callsite (0065 FE-2)**:
- `Canvas.svelte:1113` (drag stop) — `priorSnapshot` 명시, 본 amend 로 자동 rollback.

**적용 후보 (latent same-shape, 0065 외 별 sprint)**:
- `zStore.svelte.ts:112` `#commit` — `#mutate`/`#applyTwo` 가 optimistic `sessionStore.items.set(...)` 후 fire-and-forget. `priorSnapshot` 전달하면 본 계약으로 자동 rollback 가능.
- `PanelNode.svelte:125` `onResizeEnd` — NodeResizer 가 DOM 을 controlled 로 그려 store 와 desync. resize start 시 또는 onResizeEnd 진입 시 (store 가 아직 PRE 상태) snapshot 캡처 → 동일 계약 적용 가능.

### D11.2 — `optimisticMutation` wrapper (2026-05-20 amend, batch-5 후속)

D11.1 의 *수동 optimistic pattern* (caller 가 (a) `layoutSnapshot()` 캡처 → (b) `items.set(...)` 로 store 갱신 → (c) `applyMutation(transform, { priorSnapshot })` 호출) 은 Canvas drag stop / NodeResizer 처럼 *이미 SvelteFlow 가 DOM-controlled* 인 path 에는 자연이지만, Inspector / TextNode / NoteNode 처럼 *store ↔ render 의 단방향 path* 에서는 boilerplate 가 큰 데다 누락 시 *round-trip 대기 (GET+PUT 합 50~500ms)* 동안 UI 가 stale 인 회귀가 발생한다 (2026-05-20 사용자 보고: "Text/figure inspector 변경이 선택 해제되어야 적용").

본 amend 는 `sessionStore.optimisticMutation(transform, options)` helper 를 추가해 caller boilerplate 를 1줄로 통일한다:

```ts
async optimisticMutation(transform, options): Promise<{ ok: boolean; layout? }> {
  const priorSnapshot = this.layoutSnapshot();
  const optimistic = transform(priorSnapshot);
  // surgical items.set / delete — clear+repopulate 의 reactive churn 회피
  for (const it of optimistic.items) this.items.set(it.id, it);
  for (const id of this.items.keys()) if (!nextIds.has(id)) this.items.delete(id);
  return await this.applyMutation(transform, { ...options, priorSnapshot });
}
```

**적용 callsite (본 amend 의 ship)**:
- `ItemInfoView.svelte` 의 15+ `applyXxx` helper — broadcastMutation / applyShapeColor·Boolean·StrokeWidth·Dash / applyTextAlign·VerticalAlign·FontWeight·Boolean·FontSize·Color / applyNoteColor / applyLineEndpoint / changeFilePath·Image·Document.
- `TextNode.svelte::onCommit` — text + label 동시 갱신.
- `NoteNode.svelte::commit` — title / body 갱신.

**Server 부하 영향 = 0**: Inspector 컨트롤 (toggle button / dropdown / InspectorField oncommit / ColorPicker oncommit) 가 *모두 commit-based* — 1 액션 = 1 PUT, optimistic 적용 전후 동일. 본 helper 의 효과는 *round-trip latency 의 가시화* 만 (50~500ms → 0ms).

**적용 제외**: drag stop / NodeResizer onResizeEnd — 이미 수동 optimistic. WS-driven mutation (dispatcher) — optimistic 의미 없음. itemFactory.commitNewItem — 신규 item 의 server 부여 z 가 필요해 PUT 응답 후 selection 정합 (본 helper 미적용).

### D11.3 — `flowNodes` cache signature 완전성 invariant (2026-05-20 amend, batch-5 후속)

`Canvas.svelte::makeSignature` 의 type-specific payload field 누락은 cache hit 으로 인한 *stale render bug*. 2026-05-20 batch-5 R1+R2+R3 ship 후 사용자 보고로 발견: text 의 `font_weight/italic/underline/strikethrough` + rect/ellipse 의 `fill_enabled/stroke_enabled/corner_rounded/stroke_dash` + line 의 `stroke_dash` 가 signature 에 빠져 Inspector 변경이 store 까지 반영되어도 cached Node object 재사용 → SvelteFlow 가 prop unchanged 로 판단 → TextNode/ShapeNode 가 mount-time 의 옛 data 그대로 렌더. selection 해제 시점에 `selected` bit 가 signature 를 바꿔 cache miss → 그제야 re-render.

**Invariant (batch-5 amend)**:

- `makeSignature` 는 *render 에 영향을 주는 모든 schema field* 를 명시 concat 한다.
- ADR-0018 D4 의 type-specific payload 가 amend 될 때마다 본 함수도 *반드시* 같이 amend.
- 누락 = stale render bug. 본 invariant 의 회귀를 차단하려면 schema field 추가 PR 의 self-check 에 "makeSignature 갱신" 명시.

**왜 JSON.stringify(item) 으로 통일 안 하는가**: SvelteMap entry 가 reactive proxy 라 stringify 가 *모든 field 의 subscription* 을 등록 → derived 가 전체 field 변경에 폭발적 re-derive (0045 P0-A 의 effect_update_depth_exceeded 원인). 명시 concat 이 subscription 면적을 의도된 field 로 제한한다.

### D12 — Implementation entry point

`historyStore.svelte.ts` (신규) 가 단일 진입점:

```ts
historyStore.captureBeforeMutation(layout: CanvasLayout): void
historyStore.undo(): Promise<void>
historyStore.redo(): Promise<void>
historyStore.canUndo: boolean (derived)
historyStore.canRedo: boolean (derived)
historyStore.reset(): void  // session 전환, etag mismatch, terminal unmatched 시
```

`mutateLayout` 의 wrapper (또는 sessionStore 의 helper) 가 PUT 직전 `captureBeforeMutation(currentLayout)` 호출. session 전환 시 sessionStore 가 `historyStore.reset()`.

---

## Consequences

### Positive

- 단순한 mental model — "layout 의 PUT 1회 = stack 1 entry, undo 는 PRE-state 의 PUT".
- ADR-0021 G25 와 정합 — Layout 도메인과 tmux 도메인 분리 invariant 유지.
- mutateLayout 단일 entry point 의 자연 확장 — 별 path 신규 추가 없음.
- multi-webpage 도메인 충돌 회피 — etag 기반 reset 으로 race 안전.

### Negative

- Reload 시 history 손실 — 사용자 학습 비용. Toolbar tooltip 으로 mitigate.
- Layer tree drag 등 일부 mutation 이 putLayout 직접 호출 시 history capture 누락 — D11 의 audit 단계 필요.
- viewport pan/zoom 은 undo 대상 아님 — 사용자가 의도치 않게 viewport 옮긴 후 회복 수단 별도. 후속 amend 여지.
- Server-persisted history (D2 alt) 미제공 — multi-device / 다른 webpage 협업 시 history 공유 불가.

### 후속 (별 ADR/amend)

- Viewport history (D1 scope 확장): N 초 idle commit 패턴 으로 별 stack.
- Server-persisted history (D2 alt): per-session file 의 history field + multi-webpage 충돌 정책.
- Snapshot button (D2 hybrid): 명시 save / restore.
- Group structure mutation (parent_id 변경 외 그룹 자체의 add/remove): ADR-0024 의 amend 동반 필요 여부 검토.
- Inspector / Toolbar 의 undo/redo button UI: plan-0012 (가칭) 의 UX slice.

### Implementation 진행 (별 plan)

본 ADR Accepted 후 별 plan 작성. 단계:
1. **Phase 0** — D11 audit: 모든 layout mutation 의 entry point 가 mutateLayout 인지 검증, 우회 path 정리.
2. **Phase 1** — `historyStore.svelte.ts` + `mutateLayout` wrapper. captureBeforeMutation 만, undo/redo 노출 X.
3. **Phase 2** — undo/redo public API + Cmd+Z 키바인드.
4. **Phase 3** — etag mismatch / terminal unmatched 시 history reset + toast.
5. **Phase 4** — Toolbar UI button.

---

## 변경 이력

- 2026-05-17: Draft — 1번 옵션 (Scope: layout / Store: client / Conflict: etag fail) + D1.1/D1.2 sub-decision (layout 도메인 분리 + terminal unmatched 안전 처리). Accepted 전에 D11 audit (mutateLayout uniform entry) 결과 필요.
- 2026-05-17: **Accepted** — D11 audit 결과: `putLayout` 직접 호출 0건, 모든 callers (Canvas/Inspector/Layer tree/Toolbar/factory/Node 컴포넌트들 — 16+ callsites) 가 `http/sessions.ts:mutateLayout` 통과. viewport 만 500ms debounce 존재 (`sessionStore.#viewportTimer`) — layout structure 변경은 즉시 PUT 이라 race window 없음. → Implementation Phase 1 진입 가능. Phase 1 의 구체 entry point: `sessionStore.applyMutation(transform)` helper 가 `historyStore.capture` + `mutateLayout` + `loadLayout` + error handling 통합 — 모든 callers 가 이 helper 통과하도록 migration.
- 2026-05-17: **D11.1 amend (0065 FE-2 — optimistic update failure rollback)** — `applyMutation` 의 `priorSnapshot` 옵션 의미 확장: history capture 입력 + PUT 실패 시 store rollback 양방향. 직전 정책은 실패 시 toast 만, store 는 optimistic 상태 그대로 두어 *FE 가 변경된 상태로 보이나 BE 는 옛 상태* 의 silent 회귀 발생 (drag stop). 본 amend 가 `sessionStore.svelte.ts:653-655` (rollback branch) + `Canvas.svelte:1122` (failMessage "reverted to previous position") 로 닫음. 동형 invariant: 2026-05-17 의 reattach `unmatched > 0` silent 흡수 금지 (handover §6). 적용 후보 zStore / PanelNode resize 는 동일 shape 이나 0065 외 — 후속 sprint. 검증 HEAD: 본 amend ship 시점.
- 2026-05-17: **D11 audit re-verify (Phase 1~3 + P0 + P1 ship 후)** — Undo/Redo 의 manual E2E test 진입 직전 코드 인스펙션 으로 invariant 재확인.
  - `putLayout` 직접 호출 = 1건 (`http/sessions.ts:322` 의 `mutateLayout` 내부 only). user code 의 직접 호출 0건 — D11 invariant **계속 통과**.
  - `mutateLayout` 직접 호출 = `sessionStore.svelte.ts` 내부 4건 (line 344 `saveViewport` — D11 명시 viewport exception; line 613 `applyMutation` — history capture entry; line 718 `undo` / line 750 `redo` — history pop 후 PRE snapshot 복원). 외부 caller 의 `mutateLayout` 직접 호출 0건.
  - `applyMutation` callsite = 24건 (Canvas / Inspector / Layer tree / Z-store / Modal / Node 컴포넌트 6종 / itemFactory / TerminalListView / dispatcher). user-driven 23건 + WS-driven 1건 (`ws/dispatcher.svelte.ts:436` 의 0x86 MOUNT_CASCADE — D1.1 정합으로 `captureHistory: false` 명시).
  - `applyDeletion` callsite = 3건 (Canvas:185 Backspace/Delete / PanelNode:238 close 버튼 / ContextMenu:229) — handover 0054 §3.1 와 정확히 정합.
  - sessionStore 내부 self-call 0건.
  - 결론: **patch 불필요**. Undo/Redo end-to-end manual test (drag / Inspector / alignment / delete 시나리오 Cmd+Z) 의 코드-측 confidence 보강 완료. baseline: FE `pnpm check 305 FILES 0 ERRORS 0 WARNINGS`. 검증 HEAD: `4e3a0d8` (0053 amend ③ 직후).
