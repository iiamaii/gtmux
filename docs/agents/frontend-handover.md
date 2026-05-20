# gtmux — Frontend Agent Handover

> 본 문서는 *Frontend 구현 agent* 의 cold-pickup brief. multi-session pivot 후 Stage 1~10 의 FE 작업을 cold 진입할 수 있도록 모든 context + reading list + 업무 할당을 정리. 산출물 디테일은 *참조 문서들*. 본 문서는 *진입 안내* + *Stage 별 할당*.
>
> 동반 문서: `docs/agents/backend-handover.md` (BE 측 brief, BE/FE 의존성 정합)

---

## 0. 한 줄 정의

gtmux 의 frontend = **Svelte 5 + SvelteKit + xterm.js + @xyflow/svelte (SvelteFlow) + xterm + lucide icons** 로 만든 *작업공간 canvas*. multi-session pivot 으로 *session 단위 layout* + *Webpage = WS connection* + *Terminal pool UI* + *Item v2 (10 type discriminated union)* 모델. 디자인 결과물이 아닌 *작업공간* (Figma 모델 아님) — Tree/Z 분리, propagation 등 차별.

---

## 1. Required reading (cold-pickup 순서)

| # | 파일 | 목적 | 분량 |
|---|---|---|---|
| 1 | `/Users/ws/Desktop/projects/gtmux/CLAUDE.md` | 프로젝트 메타 (KO docs / EN code) | 짧음 |
| 2 | `/Users/ws/Desktop/projects/gtmux/CONTEXT.md` | 어휘 SoT + multi-session pivot + Terminal lifecycle + Z 정책 + Group 운영 규칙 | 중간 |
| 3 | `/Users/ws/Desktop/projects/gtmux/docs/plans/0007-multi-session-pivot.md` | 본 plan — Stage 0~10 + FE 기능 명세 §14 + cross-matrix §15 + 우선순위 §16 + **§14.20 공용 UX 운영 규칙** | 큼 (단일 정본) |
| 4 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0018-canvas-item-data-model.md` | Schema v2 (10 item type 의 payload) + Tree/Z 정합 amend | 큼 (D1~D8) |
| 5 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0019-session-and-workspace-model.md` | Session/Workspace UI 흐름 (Dialog, Session list modal, lock peek) | 큼 (D1~D11 + G18) |
| 6 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0020-auth-lifecycle.md` | Auth page UI + Cookie + Token rotate | 큼 (D1~D10) |
| 7 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0021-terminal-pool-and-mirror.md` | Terminal pool UI + multi-xterm + dangling overlay + Panel close dialog | 큼 (D1~D10 + G25 amend) |
| 8 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0023-file-path-open-security.md` | file_path open modal + Settings allowlist editor | 중간 (D1~D9) |
| 9 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0024-layer-tree-and-z-index-separation.md` | Layer list V2 의 Tree/Z 분리, 4 z 액션 | 중간 (D1~D8) |
| 10 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0010-group-data-model.md` | Group propagation 규칙 (visibility AND / lock OR) + G25 amend | 큼 |
| 11 | `/Users/ws/Desktop/projects/gtmux/docs/plans/0006-canvas-workspace-feature-roadmap.md` | UI 인벤토리 (§4 의 도구 list, §5 의 FE 항목 명세) — supersede 되었으나 참조 가치 | 매우 큼 |

선택 reading:
- `docs/reports/0030-sprint-7-closeout-and-handoff.md` — Sprint 7 의 Stage A~K 완료 시점. 현 FE 코드의 위치.
- ADR-0008 (single-pane-per-window) — Group 의 UI 1차 시민 근거.

---

## 2. Mental model

```
Webpage (브라우저 탭) = 1 WS 연결 = 1 Session attach
│
├── 인증 통과 후 Dialog (새 session 추가 / 기존 session 연동)
│
├── Session attach → Canvas Layout (groups + items + viewport) 로드
│
│   Canvas 위 Items (10 type):
│     - terminal (Panel) — xterm 인스턴스 + dragging / resize / minimize
│     - text/note/rect/ellipse/line/free_draw — 디자인 element 아닌 *작업 메모/시각화*
│     - image/document/file_path — asset 또는 bookmark
│
├── Sidebar:
│     - Layer list (Tree | Z toggle, ADR-0024)
│     - Terminal pool list (server-wide alive terminals + attach 점)
│
├── Toolbar (Toolbar2 — 12 도구, one-shot + Q lock)
│
└── Modals:
    - SessionListModal (1s polling, ADR-0019 D6.4)
    - NewSessionModal
    - AttachConfirmModal (match-or-spawn, ADR-0018 D6)
    - PanelCloseConfirmModal (3 옵션, G25)
    - GroupCloseConfirmModal (bulk 1 dialog, G25)
    - FileOpenConfirmModal (G21)
    - SettingsOverlay (G19 — full-screen overlay)
```

핵심 관계:
- **Webpage : Session = 1:1** — single-attach reciprocal, takeover 금지
- **Tree : Z = 분리** (ADR-0024) — Layer list drag = organization, z 는 명시 액션
- **Terminal mirror** = 한 terminal 이 여러 session 의 panel 에 동시 attach (입출력 공유)
- **Multi-xterm subscriber** — 한 terminal_id 의 stream 을 panel 마다 subscribe (FE-NEW-6)
- **Session-scoped store** — 활성 session 의 layout state 만 보유 (FE-NEW-7)

---

## 3. Architectural invariants (FE 관점)

1. **session-scoped store** (ADR-0019 + ADR-0021 D5) — 모든 *layout / M / I / viewport / focus* state 는 *활성 session* 단위. session switch 시 store reset + new layout load.
2. **Auto-mount = trigger session 만** — *내 webpage* 의 [New Terminal] 만 *내 layout* 에 mount. 다른 webpage 의 [New Terminal] 은 *그 webpage 의 layout* 에 mount, 내 layout 은 영향 없음 (Terminal list 에는 표시).
3. **Multi-xterm subscriber 패턴** (ADR-0021 D1) — 한 terminal_id 가 여러 panel 에 attach 됐을 때 각 panel 의 xterm 인스턴스가 같은 broadcast stream subscribe. xterm lifecycle 은 panel 단위 — 한 panel dispose 가 다른 panel xterm 에 영향 X.
4. **Tree order ≠ Z** (ADR-0024) — Layer list drag = organization 변경만. Z 는 4 액션 (Bring/Send) 으로만. Group 은 z 없음.
5. **Maximize = FE-only ephemeral** (G20, ADR-0018 amend) — schema 영속 X. attach 마다 fresh. 한 시점 1 panel.
6. **Esc 라우팅** (§14.20.2) — 위에서 아래로: inline edit cancel → modal close → unmaximize → tool lock 해제 → Select 복귀 → selection clear.
7. **Inline edit** (G23) — Enter commit / Esc cancel / blur commit. Multi-line: Cmd-Enter commit, Enter newline. 공용 컴포넌트 `InlineEditField.svelte` / `InlineEditTextarea.svelte`.
8. **Toolbar = one-shot default + Q lock** (G22) — 도구 사용 후 자동 Select 복귀. Q 단축키 또는 long-press 로 lock sticky. Select / Hand 는 mode 라 always sticky.
9. **Settings = full-screen overlay + auto-save** (G19) — Esc/X 닫기, outside click X. Section nav sidebar (Auth/Theme/Shortcut/Storage/Behavior/Debug). 변경 즉시 PATCH (0.5s/1s debounce).
10. **점진 어휘 통일** — code 의 *pane* → *Terminal* 점진 rename (작업 영역과 함께).

---

## 4. 현 코드 상태 (2026-05-15, Sprint 7 closeout)

- HEAD commit `1e84f4c` (Sprint 7 closeout)
- `svelte-check`: 0/0 (Sprint 7 시점, multi-session 미반영)
- Stage A~K (Sprint 7) 완료:
  - Figma adaptation (디자인 시안)
  - Chrome 컴포넌트 9건 (Titlebar, Sidebar, Toolbar2, Statusbar, ...)
  - SvelteFlow 기반 Canvas
  - Single-session WS sync
  - xterm.js integration

**현 FE 의 위치** (Sprint 7 = single-session 시대):
- ✅ Chrome 컴포넌트 (9건)
- ✅ Canvas (SvelteFlow) — drag/resize/select
- ✅ xterm.js (1:1 attach)
- ❌ Multi-session UI (Auth, Dialog, Session list, attach lifecycle) — Stage 2~3
- ❌ Schema v2 type (10 item type discriminated union) — Stage 1
- ❌ Session-scoped store — Stage 1
- ❌ Multi-xterm subscriber — Stage 4
- ❌ Terminal pool UI — Stage 4
- ❌ Layer list V2 (Tree/Z toggle, group propagation) — Stage 6
- ❌ Panel close dialog + dangling overlay — Stage 6
- ❌ Non-terminal Item renderers — Stage 5
- ❌ file_path open UX + Settings allowlist editor — Stage 5
- ❌ Settings overlay (G19) — Stage 7

**2026-05-15 multi-session pivot 진행 분 (FE Stage 1~3 + Stage 5 부분)**:
- ✅ **Stage 1** — `lib/types/canvas.ts` (CanvasItem 10-type discriminated union + Viewport + CanvasLayout envelope + 10 type guard), `lib/types/group.ts` (Group + propagation helpers: effectiveVisibility AND, effectiveLocked OR, inheritedLabel/Color, descendant/ancestor walk), `lib/stores/sessionStore.svelte.ts` (session-scoped layout/M/I/viewport/maximize). 후속 wiring 은 Stage 4 BE-NEW-4 통합 후.
- ✅ **Stage 2** — `/auth-preview` SPA page (ref/frontend-design/auth.html 디자인 차용; BE `/auth` 는 server-rendered HTML 로 BE 가 owns), `lib/types/{sessions,auth}.ts`, `lib/http/{sessions,auth}.ts` (cookie 인증 + `credentials:'include'`; BE Stage 2 actual contract 정합 — `{token? | password?}` + `retry_after_secs`), `lib/chrome/{AuthDialog,NewSessionModal,SessionListModal}.svelte`.
- ✅ **Stage 3 부분** — `src/main.ts` path-based mount (`/auth-preview` SPA, 그 외 AppPage), `lib/chrome/AttachConfirmModal.svelte` (BE confirm_required 미구현이라 현재 dead — Stage 3+ wake-up), `lib/chrome/ActiveSessionDropdown.svelte`, `lib/chrome/WorkspaceSwitcher.svelte` (모달 stack 통합), `lib/stores/workspaceSwitcher.svelte.ts` (stage 머신: closed/choice/create/list/attach_confirm), `routes/+page.svelte` amend (auth-gate: `/api/sessions` 401 → `/auth` redirect / 200 + active session 없음 → `workspaceSwitcher.open()` 자동, `?t=<token>` 자동 sessionStorage 캡처 + URL clean, prompt fallback 제거), SessionMenu amend ([Switch workspace session… / Sign out]).
- ✅ **Stage 5 부분 — Toolbar2 (FE-2)** — `lib/stores/toolStore.svelte.ts` (G22 one-shot + Q lock + Esc chain), `lib/toolbar/Toolbar2.svelte` (12 도구: Select/Hand/Terminal/Rect/Ellipse/Line/FreeDraw/Text/Note/Image/Document/FilePath + group dividers + tooltip + Q-lock visual ring). 도구 ↔ ItemType 1:1 매핑 (ADR-0018 D4). Stage 5 의 creation gestures + node renderer 는 후속.
- ⚠️ BE-FE contract 정합 작업도 같이 — `lib/http/sessions.ts`: `GET /api/sessions` bare array 정규화, `POST` flat `{name}` → synthesize SessionInfo, `attach` 의 200 → `getLayout()` 별도 호출 + 409 `holder.pid` 파싱 + 404 명시 throw, 새 `getLayout(name)` export.

**2026-05-15 추가 (BE Phase 4-B 연동분, FE-NEW-3 Phase A + B)**:
- ✅ **FE-NEW-3 Phase A — Terminal pool 읽기 UI** — `lib/types/terminals.ts` (TerminalInfo type), `lib/http/terminals.ts` (`listTerminals()` — 401/503 graceful), `lib/sidebar/TerminalListSection.svelte` (Sidebar 하단 신규 section: alive dot + label/short-id + attach_count badge + unplaced placeholder + 5s polling), `Sidebar.svelte` amend (마운트). BE `GET /api/terminals` (BE-NEW-10) consumer.
- ✅ **FE-NEW-3 Phase B — Attach-to-canvas action** — `lib/http/sessions.ts` 에 `putLayout(name, layout, etag)` + `mutateLayout(name, mutate)` (412 자동 1회 rebase) + `EtagMismatchError` 신규 export. TerminalListSection row 의 우측 hover-revealed [+] 버튼 → 현 active session 의 layout fetch → terminal item 추가 (cascade 좌표 + max z + 1, 480×320, ADR-0015 정합) → PUT → sessionStore.loadLayout. 이미 canvas 에 있으면 ✓ icon disabled, active session 없으면 + icon disabled + tooltip. Kill terminal 은 BE Phase 4-C/D 후.
- ✅ **TerminalPoolStore — 공유 폴링 캐시** — `lib/stores/terminalPool.svelte.ts`. ref-count `subscribe()` (1+ 활성 시 5s polling, 0 으로 떨어지면 stop) + `terminals` reactive snapshot + `byId(id)` 헬퍼 + `refresh()` 즉시 fetch. TerminalListSection / PaneInfoPanel / ActiveSessionDropdown 이 단일 채널 공유.
- ✅ **PaneInfoPanel — Terminal · Pool section** — 선택된 panel id 가 pool 의 terminal UUID 와 일치하면 attach_count (`×N`) + attached_sessions chip (현 active session 은 accent) + alive/dangling 상태 표시. Pool 에 없으면 "missing" warn. sessionStore migrate 후 자동 정상화.
- ✅ **ActiveSessionDropdown — pool size 배지** — 현 session 이름 옆 accent pill `N` (pool 의 alive Terminal 수). Toolbar2 우측 (lock-indicator 뒤) 에 마운트 — 클릭 시 WorkspaceSwitcher.open().

**2026-05-15 추가 (BE Phase 4-C/D 연동분, FE-NEW-5 + FE-NEW-3 kill)**:
- ✅ **AttachConfirmModal 실 confirm 흐름 wire** — BE 4-C: attach 200 `{matched, unmatched}` 응답에서 `unmatched.length > 0` 면 FE 가 `confirm_required` 로 정규화. WorkspaceSwitcher 가 modal 진입 → 사용자 [Confirm attach] → `POST attach/confirm` (신규 helper `attachConfirm(name)`) 호출 → `{spawned, already_present, failed}` 응답 처리 (per-UUID failure toast + spawned count toast) → `getLayout(name)` → `sessionStore.loadLayout()` → close. confirm 전에 setActiveSession 미리 호출 (사용자 의도 신호). AttachConfirmModal UI 도 새 shape (`matched_item_ids` keep + `spawn_count` spawn 만, `unmatched_terminal_ids` 폐기) 정합.
- ✅ **FE-NEW-3 Kill terminal** — BE 4-D `POST /api/terminals/<id>/kill` consumer (`lib/http/terminals.ts`). TerminalListSection row 의 hover-revealed [×] 버튼 (danger 색). 클릭 즉시 kill — confirm 없이 (사용자 명시 액션) + mirror hint toast ("N other sessions affected — dangling"). `terminalPool.refresh()` 즉시 호출. busy state spinner.
- ✅ **`deleteItem(name, id, killTerminal?)` http helper** — BE 4-D `DELETE /api/sessions/<name>/items/<id>?kill_terminal=bool` consumer. Panel close + 선택적 terminal kill. Panel close button 의 multi-session 정합 wiring 은 후속 (현재는 unmounted helper).
- ✅ **`respawnTerminal(id)` http helper** — BE 4-D `POST /api/terminals/<id>/respawn` consumer. Dangling overlay 의 click handler 에 wire 예정 (dangling state 시각화는 Stage 5 의 terminal-died WS frame 후).

**2026-05-15 추가 (FE-only big surgery: Canvas/Sidebar/PaneInfoPanel sessionStore migrate)**:
- ✅ **Canvas dual-source** — `useSessionStore = $derived(sessionStore.active !== null)`. `useSessionStore` 시 sessionStore.items (terminal filter) → SvelteFlow nodes (itemToNode adapter: visibility string→bool, UUID 를 pane_id 슬롯에 노출). else legacy panelsStore. Click → `sessionStore.toggleM/setM/clearM`. Drag commit → `mutateLayout(activeSession, mutator)` (multi-session) 또는 legacy `putLayoutCommitCurrent(token)`. ADR-0018 D2 (terminal item.id = UUID) 정합.
- ✅ **Sidebar Layer list dual-source** — Tree 빌더가 sessionStore.groups / sessionStore.items.filter(isTerminal) 을 legacy GroupData/PanelData shape 으로 어댑트 (visibility 문자열 → bool, UUID 를 pane_id 슬롯에). select 도 sessionStore.M / ephemeralStore.m 분기.
- ✅ **PaneInfoPanel dual-source** — selectedPanelId 가 sessionStore.M 또는 ephemeralStore.m 의 first iteration. multi-session item 은 schema v2 → legacy panel shape 어댑터.
- ⚠️ **알려진 한계**: Stage 5 의 multi-xterm subscriber + WS cookie migrate 가 들어와야 *xterm streaming* 까지 정상. 현재는 layout/select/drag/pane info 까지만 multi-session 흐름. Legacy WS dispatcher 는 여전히 작동 중 (multi-session 흐름과 별 채널) — Stage 5 의 BE-NEW-4 통합 시 단일화.

**2026-05-15 추가 (FE-only P1 polish)**:
- ✅ **EscRouter (`lib/common/escRouter.svelte.ts`)** — 7-priority chain (inline edit / modal / unmaximize / tool lock / Select 복귀 / selection clear / no-op). `register({priority, handler})` 패턴 + 등록 없이도 작동하는 default chain (3~6). plan-0007 §14.20.2 정합.
- ✅ **InlineEditField + InlineEditTextarea (`lib/common/`)** — G23 공용 컴포넌트. Single: Enter commit / Esc cancel / blur commit / empty=cancel. Multi: Cmd-Enter commit / Enter newline / Esc cancel / blur commit / empty allowed (note body 등). escRouter priority 1 등록.
- ✅ **zStore (`lib/stores/zStore.svelte.ts`)** — ADR-0024 D2 의 4 z 액션 (`bringToFront/sendToBack/bringForward/sendBackward`). sessionStore.items 갱신 + `mutateLayout` PUT 자동 커밋. Tree 와 무관 (organization 분리). swap-based bring/send forward/backward 로 *연속 정수 z 가정 X*.
- ✅ **ContextMenu ARRANGE section** — multi-session 시 4 z 액션 + "Remove from canvas" (deleteItem killTerminal=false). 단축키 hint (⇧] / ] / [ / ⇧[ / ⌫). useSessionStore 분기.
- ✅ **PanelCloseConfirmModal (`lib/chrome/PanelCloseConfirmModal.svelte`)** — G25 3-option (Cancel / Panel only / Panel + Terminal). Mirror hint (otherSessions list + 영향 받는 panel 수 경고). ADR-0021 D9.3 정합.
- ✅ **PanelNode close dual-source** — multi-session 시 PanelCloseConfirmModal 열고 사용자 선택 → `deleteItem(name, id, killTerminal)` + sessionStore 갱신 + terminalPool.refresh. legacy 흐름 (sendCtrl + putLayoutCommitCurrent) 은 backwards-compat 으로 유지.

**잔여 ❌ (BE 의존 + 진행)**:
- ❌ Multi-xterm subscriber (Stage 4) — BE-NEW-4 (WS cookie + session_id frame routing) 후
- ❌ FE-NEW-3 [Kill terminal] — BE Phase 4-C/D (BE-NEW-12.5 kill endpoint) 후
- ❌ FE-NEW-4 [Change terminal...] panel context menu — BE-NEW-12.5 후
- ❌ Layer list V2 (Stage 6) — Tree/Z toggle, group propagation 시각화, drag reorder
- ❌ Panel close dialog + dangling overlay (Stage 6) — BE-NEW-12.5 (terminal_died broadcast) 후
- ❌ Non-terminal Item renderers (Stage 5) — TextNode/NoteNode/ShapeNode/LineNode/FilePathNode + creation gestures (toolStore 는 done)
- ❌ file_path open UX + Settings allowlist editor (Stage 5) — BE-NEW-12 + ADR-0023 후
- ❌ Settings overlay G19 (Stage 7)
- ❌ Rotate-token UI (BE 다음 stage)
- ❌ Canvas / Layer list 의 sessionStore migrate — 현재 두 surface 가 legacy panelsStore/groupsStore read 중. 본 amend 는 큰 surgery (Stage 4~6 전반).

---

## 5. Frontend 기능 명세 (plan-0007 §14 의 19 items)

각 항목 디테일은 plan-0007 §14 참조. 본 절은 *목록 + 책임 영역 + 출처*.

### P0 (Stage 1~4, multi-session foundation)

| ID | 이름 | Stage | ADR | 산출 위치 (예상) |
|---|---|---|---|---|
| FE-3 | TS `CanvasItem` discriminated union | 1 | 0018 D1 | `lib/types/canvas.ts` (신규) |
| FE-NEW-7 | Session-scoped store 분리 | 1 | 0019 + 0021 D5 | `lib/stores/sessionStore.svelte.ts` (신규) |
| FE-1 | Auth page (`/auth`) | 2 | 0020 D4/D5/D8 | `src/routes/auth/+page.svelte` (신규) |
| FE-NEW-1 | Session UI (AuthDialog, NewSessionModal, SessionListModal, SessionMenu, ActiveSessionDropdown) | 2 | 0019 D7/D8/D9 + G18 polling | `lib/chrome/` 안 5 신규 |
| FE-NEW-2 | Webpage attach lifecycle (cookie 자동 재인증, heartbeat client, single-attach 409 handling) | 2~3 | 0019 D3 + 0021 D6 | 분산 |
| FE-NEW-5 | Match-or-spawn confirm dialog | 3 | 0018 D6 | `AttachConfirmModal.svelte` (신규) |
| FE-NEW-3 | Terminal pool UI (Sidebar Terminal list) | 4 | 0021 D7 | `TerminalListSection.svelte` (신규) |
| FE-NEW-4 | Terminal binding UI (panel context menu [Change terminal...]) | 4 | 0021 D8 | `ChangeTerminalModal.svelte` (신규) + PanelNode amend |
| FE-NEW-6 | Multi-xterm subscriber | 4 | 0021 D1 | `PanelNode.svelte` (큰 amend — subscriber pattern) |
| FE-6 | Layer list V2 (Tree/Z toggle, group propagation, group close bulk modal) | 6 | 0021 D7 + 0024 + 0010 G25 | `lib/chrome/Sidebar.svelte` (amend) + `GroupCloseConfirmModal.svelte` (신규) |
| FE-7 | Panel header V2 (4 z 액션 + Kill/Remove + close dialog + dangling overlay) | 6 | 0021 D8 + 0024 D2 + G25 | `PanelNode.svelte` (큰 amend) + `PanelCloseConfirmModal.svelte` (신규) + `PanelDanglingOverlay.svelte` (신규) + `ContextMenu.svelte` (amend) |

### P1 / P2 (Stage 5~10)

| ID | 이름 | Stage | ADR |
|---|---|---|---|
| FE-2 | Toolbar2 + Tool state (12 도구 + one-shot + Q lock) | 5 | G22 |
| FE-4 | Item Renderers (text/note/rect/ellipse/line/file_path) | 5 | 0018 |
| FE-NEW-8 | file_path open UX + Settings allowlist editor | 5 | 0023 |
| FE-5 | Creation gestures (click/drag-to-create, pointer capture, Esc cancel) | 5 | G22 |
| FE-9 | Viewport sync UI | 7 | 0019 + 0021 D5 |
| FE-8 | Settings UI (overlay + auto-save + sections) | 7 | G19 |
| FE-4 | Item Renderers (image/document) | 8 | 0018 |
| FE-4 | Item Renderers (free_draw) | 9 | 0018 |
| FE-10 | UX polish (xterm theme, etc.) | 7~10 | TBD |
| FE-11 | Tests | 10 | — |

### 공용 컴포넌트 (§14.20)

| 파일 | 용도 |
|---|---|
| `lib/common/InlineEditField.svelte` | Single-line inline edit (G23) |
| `lib/common/InlineEditTextarea.svelte` | Multi-line inline edit (G23) |
| `lib/common/escRouter.svelte.ts` | Esc 키 라우터 (§14.20.2 의 7 우선순위) |
| `lib/stores/toolStore.svelte.ts` | Toolbar tool state (current + locked, G22) |
| `lib/stores/zStore.svelte.ts` | Z mutation 4 액션 (Bring/Send, ADR-0024 D2) |
| `lib/stores/sessionStore.svelte.ts` | Session-scoped layout state (FE-NEW-7) |

---

## 6. Stage-by-stage 업무 할당 (FE 관점)

### Stage 1 — Foundation (FE light)
**목표**: TS type system + store 분리 (BE 의존 적음, BE-2 의 schema 정합).

작업:
1. `lib/types/canvas.ts` 신규 — TS `CanvasItem` discriminated union (ADR-0018 D1)
   ```ts
   type ItemCommon = { id: string; parent_id: string|null; x: number; y: number; w: number; h: number; z: number; visibility: "visible"|"hidden"; locked: boolean; label?: string; description?: string; minimized: boolean }
   type TerminalItem = ItemCommon & { type: "terminal" }
   type TextItem = ItemCommon & { type: "text"; text: string; font_size: number; color: string }
   // ... 10 variants
   type CanvasItem = TerminalItem | TextItem | NoteItem | RectItem | EllipseItem | LineItem | FreeDrawItem | ImageItem | DocumentItem | FilePathItem
   ```
   - ⚠️ `maximized` 는 FE-only ephemeral state (schema 외, G20 amend)
2. `lib/types/group.ts` — Group type + propagation 계산 헬퍼 (effective visibility AND / lock OR, ADR-0010)
3. `lib/stores/sessionStore.svelte.ts` 신규 (Svelte 5 runes — `$state` / `$derived`)
   - 활성 session 의 layout / viewport / M / I 보유
   - Session switch 함수: `switchSession(name)` → fetch new layout → reset store
   - `panels.svelte.ts` / `mux.svelte.ts` 의 *server-wide* state 를 *session-scoped* 로 amend
4. `svelte-check` 통과

**산출물**:
- `lib/types/canvas.ts`, `lib/types/group.ts`
- `lib/stores/sessionStore.svelte.ts` (기존 store amend)
- 모든 import 정합

### Stage 2 — Auth + Dialog + Session list (BE/FE parallel)
**목표**: 로그인 → Dialog → Session list → attach 의 전체 흐름 UI.

작업:
1. `src/routes/auth/+page.svelte` 신규 — token mode (URL query 자동 처리) 또는 password mode (form)
   - 인증 통과 시 cookie 자동 발행 + `goto('/')` redirect
   - Rate limit 에러 (5/5min) UX
2. `lib/chrome/AuthDialog.svelte` 신규 — 인증 후 [새 session 추가] / [기존 session 연동] 선택
3. `lib/chrome/NewSessionModal.svelte` 신규 — 이름 입력 + Create + 중복 reject + 정규식 validation
4. `lib/chrome/SessionListModal.svelte` 신규
   - Available / In use 섹션
   - 활성 session = 50% opacity + "in use by server-pid X" badge + click disabled + tooltip
   - **1s polling (G18)** — modal open 동안 1s 주기로 `GET /api/sessions` 재호출, modal close 시 polling 중단
5. `lib/chrome/SessionMenu.svelte` (Titlebar 의 ≡ 드롭다운 amend) — "Switch session..." / "Settings..." / "Logout"
6. `lib/chrome/ActiveSessionDropdown.svelte` (Toolbar 우측) — 현재 session 표시 → 클릭 시 SessionListModal
7. Cookie 처리: SvelteKit 의 fetch 자동 cookie 포함 (credentials: 'include' 또는 SvelteKit load function 의 cookies API)

**Integration gate**:
- smoke-2: `/` 접근 → cookie 없음 → 302 `/auth` → 로그인 → cookie 받음 → `/` → AuthDialog → [새 session] → NewSessionModal → 이름 입력 → Canvas 진입 (빈 layout).

### Stage 3 — Attach lifecycle + match-or-spawn confirm
**목표**: Session attach 의 BE 응답 처리 + match-or-spawn 의 UI 흐름.

작업:
1. WS heartbeat client — 자동 (브라우저 WS API 의 PING/PONG 또는 명시 ping interval 15s)
2. Single-attach 충돌 (HTTP 409 from `POST /api/sessions/<name>/attach`) → SessionListModal 의 그 session 을 즉시 disabled 로 + toast
3. `AttachConfirmModal.svelte` (FE-NEW-5) — backend 의 attach 응답에 `confirm_required: true, summary: {...}` 있으면 modal:
   - "Attach session 'X'? Will spawn N new terminal(s). M panel(s) without matching terminal will be... [Cancel] [Confirm]"
   - Confirm → backend 가 spawn 진행 + layout 받음

**Integration gate**:
- smoke-3: 두 webpage 같은 session attach 시도 → 두 번째 modal 의 그 row disabled + tooltip. 첫 close → ~30s 후 row enable.
- smoke-4: terminal 있는 session reload → match-or-spawn confirm dialog → terminal 1 개 spawn → Canvas 에 panel 표시.

### Stage 4 — Terminal pool + Multi-xterm + Dangling overlay
**목표**: server-wide Terminal pool 인지 + 한 terminal 의 여러 panel attach.

작업:
1. `TerminalListSection.svelte` (Sidebar 하단 신규) — `GET /api/terminals` 결과 표시
   - 각 terminal: id, label, attach count, attached session 이름 list
   - 우클릭 menu: [Attach to canvas], [Kill terminal]
2. `ChangeTerminalModal.svelte` — Panel context menu 의 [Change terminal...] 진입
3. `PanelNode.svelte` 큰 amend — **multi-xterm subscriber pattern**:
   - Panel 마다 xterm 인스턴스 자체 보유
   - WS frame 의 `terminal_id` 로 분기 — 해당 id 의 stream 만 그 panel xterm 에 write
   - Panel dispose 시 그 xterm 만 dispose (다른 panel 의 xterm 영향 X)
4. **PanelDanglingOverlay.svelte (G25, ADR-0021 D10.1 c2)** — `terminal_died` WS frame 수신 시 그 id 의 모든 panel 에 `[exit code N] — Click to restart` overlay
   - Panel focus / click / input → `POST /api/terminals/<id>/respawn` (or 새 endpoint per BE-NEW-12.5)
   - 성공 시 overlay 제거 + xterm 재attach + toast "Terminal restarted"

**Integration gate**:
- smoke-6: 한 탭에서 [New Terminal] → 다른 탭 Terminal list 갱신 (그 탭 layout 영향 X) → 다른 탭 [Attach to canvas] → 두 탭 다른 panel, 같은 terminal → 한 쪽 input → 두 쪽 동일 표시.
- smoke-6b: 탭 A 에서 panel [Panel + Terminal] → 탭 B 의 mirror panel 에 [exit] overlay → 탭 B panel click → respawn → toast.

### Stage 5 — Canvas Item (text/note/rect/ellipse/line/file_path) — FE leading
**목표**: non-terminal item 의 도구 + renderer + creation + edit.

작업:
1. `Toolbar2.svelte` (FE-2) — 12 도구 (Select/Hand/Terminal/Text/Note/Rect/Ellipse/Line/FreeDraw/Image/Document/FilePath)
   - `toolStore` (current + locked, G22)
   - One-shot default + Q lock + Esc 해제 (§14.20.3 정합)
2. `TextNode.svelte`, `NoteNode.svelte`, `ShapeNode.svelte` (Rect/Ellipse), `LineNode.svelte`, `FilePathNode.svelte` — SvelteFlow node type
3. Creation gestures:
   - click-to-create (Text, Note, FilePath): 클릭 위치에 default size item
   - drag-to-create (Rect, Ellipse, Line): drag start ~ end 의 bounding box
   - pointer capture + cancel on Esc (§14.20.2)
4. Inline edit (G23, §14.20.1 공용 컴포넌트 사용):
   - Text content edit (multi-line)
   - Note title (single) + body (multi)
   - Item label rename (single, header)
5. **FE-NEW-8 (G21, ADR-0023) file_path open UX**:
   - `FilePathItem.svelte` — double-click handler → `GET /api/file-path/allowlist-check?path=` → allowed 면 즉시 `POST /api/file-path/open`, 아니면 confirm modal
   - `FileOpenConfirmModal.svelte` — path + [✓ Always for *.{ext} within {prefix}/] 자동 추론 체크박스 + [Cancel] [Open]
     - Always 체크 → `POST /api/file-path/allowlist` 추가 + `POST /api/file-path/open`
     - 미체크 → `POST /api/file-path/open?one_time=1`
   - `SettingsOverlay` 의 Storage section 안 allowlist editor (entry list + [Delete])
6. Failure UX: spawn 실패 시 toast, path canonicalize 실패 시 panel 의 visual stale indicator

### Stage 6 — Layer list V2 + Panel header (FE leading)
**목표**: 통합 트리 + Group propagation + Tree/Z 분리 + Panel header redesign + close UX.

작업:
1. `Sidebar.svelte` amend — Layer list V2:
   - Terminal Panel + non-terminal Canvas Item 통합 트리
   - 상단 toggle **[Tree | Z] (ADR-0024 D4)**:
     - Tree 모드 (default): group 계층 + 각 row 에 z badge + drag reorder/reparent = organization 만 (z 영향 X)
     - Z 모드: flat 정렬 (z 내림차순). drag reorder 비활성. group label hint.
   - Multi-select (Cmd+click, Shift+click, drag marquee)
   - Per-row toggles: visibility, lock + propagation 표시 (ADR-0010, 회색 + tooltip)
   - Inline rename (G23 공용 컴포넌트)
   - Group context menu: [Ungroup] (비파괴, ADR-0010 D12) + [Delete group] (`GroupCloseConfirmModal`, bulk 1 dialog)
2. **GroupCloseConfirmModal.svelte (G25)** — 자손 panel + non-terminal items + mirror hint + 3 옵션 [Cancel] / [Panels only] / [Panels + Terminals]
3. **Panel header V2 (FE-7)** — `PanelNode.svelte` 큰 amend:
   - Header: title / id / status / Input Target marker / minimize / maximize / invisible / close + more menu
   - Header more menu (…):
     - **4 z 액션 (ADR-0024 D2)**: ▲ Bring to front (Shift+]), ▼ Send to back (Shift+[), ↑ Bring forward (]), ↓ Send backward ([)
     - [Change terminal...] → ChangeTerminalModal (FE-NEW-4)
     - [Kill terminal] — terminal SIGTERM 만 (panel 들 유지)
     - [Remove panel] — 그 session 의 panel 만 제거 (terminal 영향 X)
     - Rename / Settings
   - **Close 버튼 (X)** → `PanelCloseConfirmModal` (G25, 3 옵션)
4. **PanelCloseConfirmModal.svelte (G25)** — [Cancel] / [Panel only] / [Panel + Terminal] + mirror hint (다른 session 이름 list)
   - `Settings.behavior.auto_kill_terminal_on_panel_close = true` 시 dialog 생략 + [Panel + Terminal] 즉시
5. **PanelDanglingOverlay.svelte (G25)** — Stage 4 의 항목 그대로 (이미 Stage 4 에 완성)
6. Canvas right-click context menu — `ContextMenu.svelte` (신규 또는 amend) 같은 4 z 액션 + [Close panel...] 등

**Integration gate**:
- smoke-8: 여러 item 다중 선택 → [Group] 액션 → Group row 생성 → Group visibility 토글 → 자손 모두 dim. Panel minimize → header bar 만 표시. Group [Delete] → bulk modal → confirm → 자손 처리. Panel close → PanelCloseConfirmModal → option 선택 → 효과.

### Stage 7 — Viewport sync + Settings UI (BE/FE parallel)
**목표**: Viewport state 양방향 sync + Settings overlay.

FE 작업:
1. `FE-9 Viewport sync UI` — 옛 server-authoritative 폐기. viewport 가 session layout 의 일부 (groups + items 와 같이 영속). 양방향 sync (debounce).
2. **`SettingsOverlay.svelte` (G19, FE-8)**:
   - Full-screen overlay + 좌측 sidebar nav (Auth / Theme / Shortcut / Storage / Behavior / Debug)
   - 즉시 자동 저장 (no Save/Cancel) — toggle / select 는 0.5s debounce, free text 는 1s debounce + blur commit
   - Destructive action (Reset config / Logout all sessions) — [버튼 + confirm modal]
   - Multi-field form (password change) — 별 panel + [Change password] 버튼
   - Boot-immutable field (workspace_path, port) — read-only + "재기동에 적용" hint
   - Storage section: workspace path, file_open allowlist editor (Stage 5 의 FE-NEW-8 와 같이)
   - Behavior section: `auto_kill_terminal_on_panel_close` (G25)

### Stage 8 — Asset items (image/document, P2)
**FE 작업**:
- ImageNode, DocumentNode renderer
- Asset upload (file picker + drop)
- Asset path (`/api/assets/<sha256>` 응답)

### Stage 9 — Free draw + drawing perf (P2)
**FE 작업**:
- FreeDrawNode renderer
- Point simplification (Ramer-Douglas-Peucker)
- Backpressure on upload

### Stage 10 — Hardening
**FE 작업**:
- Playwright E2E (qa skill)
- Multi-tab race scenarios
- Accessibility audit

---

## 7. Build / dev / test

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/frontend
npm install
npm run check          # svelte-check
npm run build          # ⚠️ dev 모드 사용 금지

# 빌드 후 backend 가 SPA 서빙
cd ..
GTMUX_FRONTEND_DIST=frontend/dist gtmux start --port 9999
```

⚠️ **함정**:
- **`npm run dev` 사용 금지** — backend 가 release binary 로 `dist/` 를 SPA 서빙. dev server 별로 띄우지 말 것.
- xyflow / lucide-svelte 1.0.1 의 `$$props` 가 Svelte 5 strict 빌드와 충돌 가능 — chrome 아이콘은 *인라인 SVG* 사용.
- xterm.js 의 *renderer* 옵션: WebGL renderer 권장 (성능). 한 panel 마다 인스턴스 — Stage 4 의 multi-xterm subscriber pattern 의 핵심.

---

## 8. BE 의존성 매트릭스 요약 (plan-0007 §15)

| 내 작업 | 의존하는 BE |
|---|---|
| FE-1 Auth page | BE-1 (auth handler), BE-NEW-7 (cookie) |
| FE-NEW-1 Session UI | BE-NEW-1 (WM), BE-NEW-2 (Session CRUD), BE-NEW-7 |
| FE-NEW-2 Attach lifecycle | BE-NEW-3 (attach handler) |
| FE-NEW-5 Attach confirm | BE-NEW-3 |
| FE-NEW-3 Terminal pool UI | BE-NEW-10 (terminal list API), **BE-NEW-12.5** (close API) |
| FE-NEW-6 Multi-xterm | BE-NEW-4 (WS routing), BE-NEW-10 |
| FE-6 Layer list V2 | BE-2 (schema), BE-NEW-2, BE-NEW-10, **BE-NEW-12.5** |
| FE-7 Panel header V2 | BE-NEW-2, BE-NEW-10, **BE-NEW-12.5** |
| FE-8 Settings UI | BE-1, BE-NEW-7, BE-NEW-12 |
| FE-NEW-8 file_path open UX | BE-1, BE-NEW-7, BE-NEW-12 |

**진입 순서**: FE-3 / FE-NEW-7 (Stage 1, BE 의존 적음) → FE-1 / FE-NEW-1 (Stage 2 — BE Stage 2 와 parallel) → FE-NEW-2 / FE-NEW-5 (Stage 3) → FE-NEW-3/4/6 (Stage 4) → FE-2/4/5/NEW-8 (Stage 5) → FE-6/7 (Stage 6).

---

## 9. Glossary (FE 어휘)

| 용어 | 의미 |
|---|---|
| **Webpage** | 1 브라우저 탭 = 1 WS 연결 = 1 session attach. ADR-0019 D3. |
| **Session** | workspace 안 named record. FE 의 *layout 단위*. attach 시 layout 로드. |
| **Canvas** | 1 session 의 무한 작업 공간 (SvelteFlow). 다른 session 과 독립. |
| **Canvas Item** | Canvas 위 시각 객체. 10 type discriminated union (terminal/text/note/rect/ellipse/line/free_draw/image/document/file_path). ADR-0018. |
| **Panel** | `type:"terminal"` 인 Canvas Item — xterm 컨테이너. ADR-0018 D1. |
| **Group** | 자식들 묶음 (트리). z 없음. propagation: visibility AND / lock OR. ADR-0010. |
| **Manipulation Selection (M)** | 사용자가 *제어 대상* 으로 잡은 Items. 다중 가능. session-scoped. CONTEXT.md. |
| **Input Target (I)** | 키보드 입력 라우팅 대상 terminal. 1 session 안 unique. CONTEXT.md. |
| **Streaming State** | (session, panel) 쌍 단위 — `Streaming` / `Suspended`. Suspended 시 broadcast subscriber drop. CONTEXT.md. |
| **Dangling Terminal Reference** | layout 의 terminal item.id 가 server-pool 의 alive Terminal 과 매칭 안 됨. → 같은 id 로 fresh spawn 자연 (lazy on interaction). ADR-0021 D10. |
| **Mirror** | 한 terminal 이 여러 panel (다른 session 까지) 에 동시 attach + 입출력 공유. ADR-0021 D1/D2. |

---

## 10. UX 공용 룰 (plan-0007 §14.20 SoT)

- **Inline edit (G23)**: Single = Enter commit / Esc cancel / blur commit. Multi = Cmd-Enter commit, Enter newline, Esc cancel, blur commit. 공용 컴포넌트 사용 의무.
- **Esc 라우팅 (G20+G22+G23)**: 위에서 아래로 7 우선순위 — inline edit cancel → modal close → unmaximize → tool lock 해제 → Select 복귀 → selection clear → no-op. `lib/common/escRouter.svelte.ts`.
- **Toolbar (G22)**: 모두 one-shot default. Q lock. Esc 해제. Select / Hand 는 mode always sticky.
- **Maximize (G20)**: Canvas viewport area fill (Titlebar/Toolbar/Status bar 유지). FE-only ephemeral. 한 시점 1 panel. Unmaximize: Esc / 헤더 toggle / panel header double-click.
- **Modal stack**: 상위 modal 이 Esc 흡수. Outside click 으로 닫음 X (실수 방지 — Settings 도 동일).
- **Inline edit validation 실패**: 빨간 hint + 키 비활성. Empty single = cancel 효과. Empty multi = 빈 string 허용.

---

## 11. 작업 룰

- **English code, English comments, Korean docs.**
- **점진 어휘 통일** — `pane` → `Terminal` 어휘 (점진, 작업 영역과 함께).
- **Svelte 5 runes** — `$state` / `$derived` / `$effect` 사용. 옛 store 패턴 (writable/readable) 점진 이전.
- **xyflow / SvelteFlow** — node type registry 패턴. 새 type 추가는 node component + node type id + Toolbar 등록.
- **xterm.js** — WebGL renderer 권장, theme 객체 amend (xterm theme adapter, G27 P1).
- **불필요한 추가 금지** — backwards compat, feature flag, *향후 가능성* 추상화 모두 거부.

---

## 12. 진입 시 첫 메시지 후보

- "Stage 1 FE 시작" → 본 brief §6 의 Stage 1 작업 1~4. TS type → store 분리 → svelte-check 통과.
- "FE-NEW-1 부터" → Stage 2 의 Auth + Dialog + Session list 패턴.
- "Layer list V2 시작" → Stage 6, ADR-0024 + ADR-0010 G25 정합.
- "Multi-xterm 패턴 모름" → ADR-0021 D1/D2 + plan-0007 §14.17 (FE-NEW-6) 의 subscriber pattern.

---

## 13. 변경 이력

- 2026-05-15: 초안 — multi-session pivot 후 FE agent 진입 brief. plan-0007 §14 + ADR 4 신규 (0018/0019/0020/0021) + ADR 2 추가 (0023/0024) + G18~G25 grilling 결과 정합.
