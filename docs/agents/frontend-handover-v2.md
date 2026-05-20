# gtmux — Frontend Agent Handover **v2**

> **v2 위치**: 2026-05-15 의 P0 (G18~G25) + P1 (G26~G29) grilling + D 검증 결과 모두 반영. v1 (`frontend-handover.md`) 의 *후속*. v1 은 보존 — historical reference. **이 v2 만 읽으면 cold pickup OK**.
>
> 동반 문서: `docs/agents/backend-handover-v2.md` (BE v2)

---

## 0. v1 → v2 변경 요약

v1 (2026-05-15 G18~G25 직후) 의 단순 후속 — *P1 grilling 4건* (G26~G29) + *D 검증 정합* 반영. **architectural 변경 없음**, FE 항목에 *3 신규 store/utility* + *Setting section 3 신규* + *Toolbar amend*:

| 변경 | 출처 | v2 영향 |
|---|---|---|
| `shortcutRegistry.svelte.ts` (Hybrid layer + Esc 처리 + P0/P1 매트릭스) | G26 | §5 / §6 / §10 신규 |
| `themeStore.svelte.ts` + `xtermTheme.ts` (light+dark 2 fixed, system detect, chrome+xterm 동기) | G27 | §5 / §6 신규 |
| Settings 의 [Export this session] / [Import session] (G28) | G28 | §5 / §6 Stage 7 amend |
| Toolbar2 의 Select+Hand mode + Space hold + drag = pan modifier | G29 | §5 / §6 Stage 5 + §10 UX 룰 amend |
| Stage 5 정의 — file_path 가 Stage 5 (string-only) 로 이동 | D 검증 | §6 Stage 5 표현 갱신 |
| Critical path 의 Stage 4 BE-NEW-12.5 명시 (FE-NEW-3/6/7 의 의존) | D 검증 | §8 의존 매트릭스 |
| 추가 ADR: 0023 (file_path security), 0024 (Layer/Z 분리) | G21, G24 | §1 reading list |
| FE-UX-Common §14.20 의 §14.20.5 신규 (Shortcut 정책) | G26 | §10 UX 룰 신규 |

v2 의 코드 진입은 v1 의 *그 Stage 1 진입점 그대로* (FE-3 TS type + FE-NEW-7 store 분리) — v2 변경은 *Stage 5+ 진입 전까지* code 영향 0.

---

## 1. Required reading (cold-pickup 순서)

| # | 파일 | 목적 |
|---|---|---|
| 1 | `/Users/ws/Desktop/projects/gtmux/CLAUDE.md` | 프로젝트 메타 (KO docs / EN code) |
| 2 | `/Users/ws/Desktop/projects/gtmux/CONTEXT.md` | 어휘 SoT + multi-session pivot + Terminal lifecycle + Z 정책 + Group 운영 |
| 3 | `/Users/ws/Desktop/projects/gtmux/docs/plans/0007-multi-session-pivot.md` | 본 plan §0~§18 — G18~G29 + D 검증 반영 단일 정본. **§14.20 공용 UX 운영 규칙** 특히 §14.20.5 (Shortcut) |
| 4 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0018-canvas-item-data-model.md` | Schema v2 (10 item type) + G20/G24 amend |
| 5 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0019-session-and-workspace-model.md` | Session/Workspace UI 흐름 (Dialog, Session list modal, lock peek, 1s polling G18) |
| 6 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0020-auth-lifecycle.md` | Auth page UI + Cookie + Token rotate |
| 7 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0021-terminal-pool-and-mirror.md` | Terminal pool UI + multi-xterm + dangling overlay + Panel close dialog (D1~D10 + G25 amend) |
| 8 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0023-file-path-open-security.md` | **NEW** — file_path open modal + Settings allowlist editor (G21) |
| 9 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0024-layer-tree-and-z-index-separation.md` | **NEW** — Layer list V2 의 Tree/Z 분리, 4 z 액션 (G24) |
| 10 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0010-group-data-model.md` | Group propagation (G24+G25 amend — group z 없음, D12 Ungroup, D13 multi-session) |
| 11 | `/Users/ws/Desktop/projects/gtmux/docs/plans/0006-canvas-workspace-feature-roadmap.md` | UI 인벤토리 (supersede 됐으나 §4 도구 list 참조) |

선택:
- `docs/reports/0030-sprint-7-closeout-and-handoff.md` — Sprint 7 완료 시점.
- v1 brief (`docs/agents/frontend-handover.md`) — historical.

---

## 2. Mental model

```
Webpage (브라우저 탭) = 1 WS 연결 = 1 Session attach
│
├── 인증 통과 후 Dialog (새 session 추가 / 기존 session 연동)
│
├── Session attach → Canvas Layout (groups + items + viewport)
│
│   Canvas 위 Items (10 type):
│     - terminal (Panel) — xterm + dragging
│     - text/note/rect/ellipse/line/free_draw — 메모/시각화
│     - image/document/file_path — asset 또는 bookmark
│
├── Sidebar:
│     - Layer list (Tree | Z toggle, ADR-0024)
│     - Terminal pool list
│
├── Toolbar (12 도구, Select+Hand mode + 10 one-shot + Q lock)
│
├── Modals:
│     - SessionListModal (1s polling)
│     - NewSessionModal
│     - AttachConfirmModal (match-or-spawn)
│     - PanelCloseConfirmModal (3 옵션, G25)
│     - GroupCloseConfirmModal (bulk 1 dialog, G25)
│     - FileOpenConfirmModal (G21)
│     - SettingsOverlay (G19 — full-screen overlay)
│
└── Stores / utilities:
      - sessionStore (FE-NEW-7) — 활성 session 의 layout state
      - toolStore (G22) — current tool + locked
      - zStore (ADR-0024 D2) — Bring/Send 4 액션
      - shortcutRegistry (G26) — 전역 keydown + xterm focus 검사
      - themeStore (G27) — currentTheme + resolvedTheme
      - escRouter (§14.20.2) — 7 우선순위 키 라우팅
```

핵심 관계:
- **Webpage : Session = 1:1** — single-attach reciprocal, takeover 금지
- **Tree : Z = 분리** (ADR-0024) — Layer list drag = organization, z 는 명시 액션
- **Terminal mirror** = 1 terminal → N panel attach (입출력 공유)
- **Multi-xterm subscriber** — 1 terminal_id 의 stream 을 panel 마다 subscribe
- **Session-scoped store** — 활성 session 의 layout state 만

---

## 3. Architectural invariants (FE)

1. **session-scoped store** (ADR-0019 + ADR-0021 D5) — layout / M / I / viewport / focus 는 활성 session 단위.
2. **Auto-mount = trigger session 만** — 내 [New Terminal] 만 내 layout 에 mount.
3. **Multi-xterm subscriber 패턴** (ADR-0021 D1) — panel 마다 xterm 인스턴스. dispose 독립.
4. **Tree order ≠ Z** (ADR-0024) — drag = organization. Z 는 4 액션 (Bring/Send). Group z 없음.
5. **Maximize = FE-only ephemeral** (G20) — schema 영속 X. 한 시점 1 panel.
6. **Esc 라우팅** (§14.20.2) — 7 우선순위: inline edit cancel → modal close → unmax → tool lock 해제 → Select 복귀 → selection clear → no-op.
7. **Inline edit** (G23) — Enter/Esc/blur. Multi-line: Cmd-Enter, Enter newline, blur commit. 공용 `InlineEditField` / `InlineEditTextarea`.
8. **Toolbar = one-shot + Q lock** (G22) — Select/Hand 는 mode always sticky. 다른 10 도구는 one-shot default. Esc 해제. **Space hold + drag = pan modifier (G29)**.
9. **Settings = full-screen overlay + auto-save** (G19) — Esc/X 닫기, outside click X. 즉시 PATCH (debounce).
10. **Keyboard shortcut Hybrid (G26)** — single-key 는 xterm focus 외에만, modifier 는 항상. Esc = xterm focus 시 shell ESC byte 전달 (vim 자연), 그 외 escRouter.
11. **Panel close dialog (G25)** — 3 옵션 ([Cancel] / [Panel only] / [Panel + Terminal]) + mirror hint. `auto_kill_terminal_on_panel_close: false` default.
12. **Dangling lazy spawn (G25)** — terminal SIGTERM 시 [exit] overlay → focus interaction → respawn POST.
13. **Theme system (G27)** — `[System | Light | Dark]`. system mode 는 `prefers-color-scheme`. ANSI 16 색 보존. Hot reload 모든 xterm.
14. **점진 어휘 통일** — `pane` → `Terminal` (작업 영역과 함께).

---

## 4. 현 코드 상태 (2026-05-15, Sprint 7 closeout)

- HEAD `1e84f4c` (Sprint 7 closeout)
- `svelte-check`: 0/0 (Sprint 7 시점)
- Stage A~K (Sprint 7) 완료: Figma adaptation, chrome 9건, SvelteFlow Canvas, single-session WS sync, xterm.js 1:1 attach

**현 FE 의 위치**:
- ✅ Chrome 컴포넌트 (9건) / Canvas / xterm.js 1:1
- ❌ Multi-session UI — Stage 2~3
- ❌ Schema v2 TS type — Stage 1
- ❌ Session-scoped store — Stage 1
- ❌ Multi-xterm subscriber — Stage 4
- ❌ Terminal pool UI — Stage 4
- ❌ **Panel close dialog + dangling overlay (G25)** — Stage 4/6
- ❌ Layer list V2 (Tree/Z toggle, group propagation) — Stage 6
- ❌ Non-terminal Item renderers (text/note/.../file_path) — Stage 5
- ❌ **file_path open UX + Settings allowlist editor (G21)** — Stage 5
- ❌ **Toolbar2 (Select+Hand mode + 10 one-shot + Q lock + Space-pan, G22+G29)** — Stage 5
- ❌ Settings overlay (G19) + **Theme/Shortcut/Storage section (G26/G27/G28)** — Stage 7
- ❌ **shortcutRegistry / themeStore / xtermTheme (G26/G27)** — Stage 7 (또는 Stage 5 의 인프라로 미리)

---

## 5. Frontend 기능 명세 (plan-0007 §14)

### P0 (Stage 1~4)

| ID | 이름 | Stage | ADR | 산출 위치 |
|---|---|---|---|---|
| FE-3 | TS `CanvasItem` discriminated union | 1 | 0018 D1 | `lib/types/canvas.ts` (신규) |
| FE-NEW-7 | Session-scoped store 분리 | 1 | 0019 + 0021 D5 | `lib/stores/sessionStore.svelte.ts` (신규) |
| FE-1 | Auth page (`/auth`) | 2 | 0020 D4/D5/D8 | `src/routes/auth/+page.svelte` (신규) |
| FE-NEW-1 | Session UI (AuthDialog, NewSessionModal, SessionListModal, SessionMenu, ActiveSessionDropdown) | 2 | 0019 D7/D8/D9 + G18 polling | `lib/chrome/` 안 5 신규 |
| FE-NEW-2 | Webpage attach lifecycle | 2~3 | 0019 D3 + 0021 D6 | 분산 |
| FE-NEW-5 | Match-or-spawn confirm dialog | 3 | 0018 D6 | `AttachConfirmModal.svelte` (신규) |
| FE-NEW-3 | Terminal pool UI (Sidebar Terminal list) | 4 | 0021 D7 | `TerminalListSection.svelte` (신규) |
| FE-NEW-4 | Terminal binding UI (Panel context menu [Change terminal...]) | 4 | 0021 D8 | `ChangeTerminalModal.svelte` (신규) + PanelNode amend |
| FE-NEW-6 | Multi-xterm subscriber | 4 | 0021 D1 | `PanelNode.svelte` (큰 amend) |
| **PanelDanglingOverlay** ⭐ | Terminal SIGTERM 시 [exit] overlay → focus → respawn (G25 c2) | 4 | 0021 D10.1 | `PanelDanglingOverlay.svelte` (신규) |
| FE-6 | Layer list V2 (Tree/Z toggle, group propagation, group close bulk modal) | 6 | 0021 D7 + 0024 + G25 | `Sidebar.svelte` (amend) + `GroupCloseConfirmModal.svelte` (신규) |
| FE-7 | Panel header V2 (4 z 액션 + Kill/Remove + close dialog + dangling overlay) | 6 | 0021 D8 + 0024 D2 + G25 | `PanelNode.svelte` (큰 amend) + `PanelCloseConfirmModal.svelte` (신규) + `ContextMenu.svelte` (amend) |

### P1 (Stage 5~7)

| ID | 이름 | Stage | ADR / Grilling |
|---|---|---|---|
| FE-2 | Toolbar2 (12 도구, one-shot + Q lock + Space-pan) | 5 | G22 + G29 |
| FE-4 | Item Renderers (text/note/rect/ellipse/line/**file_path**) | 5 | 0018 |
| FE-NEW-8 | file_path open UX + confirm modal | 5 | 0023 |
| FE-5 | Creation gestures | 5 | G22 |
| FE-9 | Viewport sync UI | 7 | 0019 + 0021 D5 |
| FE-8 | Settings UI (overlay + auto-save + sections: Auth/Theme/Shortcut/Storage/Behavior/Debug) | 7 | G19 + G26 + G27 + G28 |
| **shortcutRegistry** ⭐ | 전역 keydown + xterm focus 검사 + P0/P1 매트릭스 | 5+ (인프라) / 7 (P1 application shortcut) | G26 |
| **themeStore + xtermTheme** ⭐ | light/dark 2 fixed + system detect + chrome/xterm 동기 | 7 | G27 |
| **Session export/import** ⭐ | Settings Storage 의 [Export/Import] 버튼 + JSON + meta + 충돌 dialog | 7 | G28 |

### P2 (Stage 8~9)

| ID | 이름 | Stage | ADR |
|---|---|---|---|
| FE-4 | Item Renderers (image/document) | 8 | 0018 |
| FE-4 | Item Renderers (free_draw) | 9 | 0018 |
| FE-10 | UX polish | 7~10 | TBD |
| FE-11 | Tests (Vitest + Playwright) | 10 | — |

### 공용 컴포넌트 / store (§14.20)

| 파일 | 용도 | Grilling |
|---|---|---|
| `lib/common/InlineEditField.svelte` | Single-line inline edit | G23 |
| `lib/common/InlineEditTextarea.svelte` | Multi-line inline edit | G23 |
| `lib/common/escRouter.svelte.ts` | Esc 7 우선순위 라우터 | G20+G22+G23 |
| `lib/common/shortcutRegistry.svelte.ts` ⭐ | 전역 keydown + xterm focus + P0/P1 매트릭스 | G26 |
| `lib/stores/toolStore.svelte.ts` | Toolbar tool state (current + locked) | G22 |
| `lib/stores/zStore.svelte.ts` | Z mutation 4 액션 | ADR-0024 D2 |
| `lib/stores/sessionStore.svelte.ts` | Session-scoped layout state | FE-NEW-7 |
| `lib/stores/themeStore.svelte.ts` ⭐ | currentTheme + resolvedTheme (system detect) | G27 |
| `lib/utils/xtermTheme.ts` ⭐ | light/dark xterm theme 객체 (ANSI 16 색 보존) | G27 |

---

## 6. Stage-by-stage 업무 할당 (FE)

### Stage 1 — Foundation (FE light)
**목표**: TS type + store 분리.

작업:
1. `lib/types/canvas.ts` 신규 — `CanvasItem` discriminated union
   ```ts
   type ItemCommon = {
     id: string; parent_id: string|null;
     x: number; y: number; w: number; h: number; z: number;
     visibility: "visible"|"hidden"; locked: boolean;
     label?: string; description?: string;
     minimized: boolean;
     // ⚠️ maximized 는 FE-only ephemeral (G20)
   };
   type TerminalItem = ItemCommon & { type: "terminal" };
   type TextItem = ItemCommon & { type: "text"; text: string; font_size: number; color: string };
   // ... 10 variants
   ```
2. `lib/types/group.ts` — Group type + propagation 헬퍼 (effective visibility AND / lock OR)
3. `lib/stores/sessionStore.svelte.ts` 신규 (Svelte 5 runes)
   - 활성 session 의 layout / viewport / M / I
   - `switchSession(name)` → fetch new layout → reset store
4. `svelte-check` 통과

**산출물**: `lib/types/canvas.ts`, `lib/types/group.ts`, `lib/stores/sessionStore.svelte.ts` + 기존 store amend.

### Stage 2 — Auth + Dialog + Session list (BE/FE parallel)
작업:
1. `src/routes/auth/+page.svelte` 신규 — token (URL query) / password (form)
2. `lib/chrome/AuthDialog.svelte` — [새 session 추가] / [기존 session 연동]
3. `lib/chrome/NewSessionModal.svelte` — 이름 + Create + 정규식 + 중복 reject
4. `lib/chrome/SessionListModal.svelte` — Available / In use sections, "in use" badge, **1s polling (G18)**
5. `lib/chrome/SessionMenu.svelte` amend — "Switch session...", "Settings..."
6. `lib/chrome/ActiveSessionDropdown.svelte` — Toolbar 우측

**Integration gate (smoke-2)**: `/` → 302 `/auth` → 로그인 → cookie → `/` → AuthDialog → NewSessionModal → Canvas 진입.

### Stage 3 — Attach lifecycle + match-or-spawn confirm
작업:
1. WS heartbeat client (자동 또는 명시 15s)
2. Single-attach 409 → SessionListModal 의 그 row disabled + toast
3. `AttachConfirmModal.svelte` — backend 응답의 `confirm_required` 시 modal

**Integration gate**: smoke-3 (multi-tab attach 충돌) + smoke-4 (reload + match-or-spawn).

### Stage 4 — Terminal pool + Multi-xterm + **Dangling overlay** ⭐ (v2 신규)
작업:
1. `TerminalListSection.svelte` (Sidebar 하단) — `GET /api/terminals` + attach 점 정보 + 우클릭 menu
2. `ChangeTerminalModal.svelte` — Panel context menu 진입
3. `PanelNode.svelte` 큰 amend — **multi-xterm subscriber pattern** (FE-NEW-6)
   - Panel 마다 xterm 인스턴스
   - WS frame `terminal_id` 분기 → 해당 id stream 만 write
   - Panel dispose 시 그 xterm 만 dispose
4. **`PanelDanglingOverlay.svelte` (G25 c2) ⭐ 신규** — `terminal_died` WS frame 수신 시 그 id 의 모든 panel 에 `[exit code N] — Click to restart` overlay
   - Panel focus / click / input → `POST /api/terminals/<id>/respawn`
   - 성공 시 overlay 제거 + xterm 재attach + toast "Terminal restarted"

**Integration gate**:
- smoke-6: 한 탭 [New Terminal] → 다른 탭 Terminal list 갱신 (그 탭 layout 영향 X) → 다른 탭 [Attach] → 두 탭 같은 terminal mirror.
- smoke-6b (G25 신규): 탭 A 의 panel close [Panel + Terminal] → 탭 B mirror panel 에 [exit] overlay → 탭 B panel click → respawn → toast.

### Stage 5 — Canvas Item (text/note/rect/ellipse/line/**file_path**) ⭐ — FE leading
**v2 변경**: file_path 가 Stage 5 (string-only, asset 비의존) 로 이동. image/document 는 Stage 8.

작업:
1. **`Toolbar2.svelte` (FE-2, G22 + G29) ⭐ amend**:
   - 12 도구 (Select/Hand/Terminal/Text/Note/Rect/Ellipse/Line/FreeDraw/Image/Document/FilePath)
   - `toolStore` (current + locked)
   - **Select/Hand 는 mode always sticky** (G29)
   - 나머지 10 도구 one-shot default + Q lock + Esc 해제 (§14.20.3)
   - **Space hold + drag = pan modifier (G29)** — Canvas 컴포넌트 안 handler
2. Per-type Node renderer: `TextNode`, `NoteNode`, `ShapeNode` (Rect/Ellipse), `LineNode`, **`FilePathNode`** ⭐
3. Creation gestures: click-to-create / drag-to-create / pointer capture / cancel on Esc
4. Inline edit (G23, §14.20.1 공용 컴포넌트)
5. **FE-NEW-8 (G21) ⭐ 신규 — file_path open UX**:
   - `FilePathItem.svelte` — double-click → `GET /api/file-path/allowlist-check` → allowed 면 즉시 `POST /api/file-path/open`, 아니면 confirm modal
   - `FileOpenConfirmModal.svelte` — path + [✓ Always for *.{ext} within {prefix}/] 자동 추론 + [Cancel] [Open]

**Integration gate (smoke-7b, G21 신규)**: File Path 도구 → click → path 입력 → file_path item → double-click → confirm modal → [✓ Always for] + [Open] → toast + allowlist 영속. 다시 double-click → confirm 생략 + 즉시 open.

### Stage 6 — Layer list V2 + Panel header (FE leading)
작업:
1. `Sidebar.svelte` amend — Layer list V2:
   - 상단 toggle **[Tree | Z] (G24)**
   - Multi-select, per-row toggles + propagation 표시
   - Inline rename (G23)
   - Group context menu: [Ungroup] / [Delete group] (`GroupCloseConfirmModal` bulk dialog)
2. **`GroupCloseConfirmModal.svelte` (G25) ⭐** — 자손 + mirror hint + 3 옵션
3. **Panel header V2 (FE-7)**:
   - Header redesign + more menu (…) — **4 z 액션 + [Kill terminal] + [Remove panel] + Rename/Settings**
   - Close 버튼 (X) → **`PanelCloseConfirmModal.svelte` (G25) ⭐** — 3 옵션 + mirror hint
4. `ContextMenu.svelte` (신규/amend) — canvas right-click 의 4 z 액션 + close 액션

**Integration gate (smoke-8)**: 다중 선택 → Group → visibility 토글 → 자손 dim. Group [Delete] → bulk modal. Panel close → PanelCloseConfirmModal → option → 효과.

### Stage 7 — Viewport sync + Settings + **shortcutRegistry + themeStore + export/import** ⭐ (v2 amend)
**큰 amend** — G26/G27/G28 모두 여기 정착.

작업:
1. `FE-9 Viewport sync UI` — 양방향 sync (debounce)
2. **`shortcutRegistry.svelte.ts` (G26) ⭐ 신규**:
   - 전역 `keydown` listener (window-level)
   - xterm focus 검사 (`document.activeElement` 가 xterm 안인지)
   - Modifier + key code 매칭
   - P0 매트릭스 (Esc/Enter/Cmd-Enter/Q/]/[/Shift+]/[) — Stage 5/6 까지 이미 등록 가능
   - P1 매트릭스 (Cmd+N/Cmd+Shift+L/Cmd+Shift+Q/Cmd+,) — Stage 7 추가
3. **`themeStore.svelte.ts` + `xtermTheme.ts` (G27) ⭐ 신규**:
   - `currentTheme: "system"|"light"|"dark"` ($state)
   - `resolvedTheme: "light"|"dark"` ($derived, MediaQueryList listener)
   - `:root[data-theme="..."]` CSS variable — chrome 컴포넌트 정합
   - 모든 xterm 인스턴스의 `terminal.options.theme = newTheme` (hot reload)
4. **`SettingsOverlay.svelte` (G19 + G26 + G27 + G28) ⭐ 큰 amend**:
   - Full-screen overlay + 좌측 sidebar nav
   - 즉시 자동 저장 (debounce)
   - Section:
     - **Auth** — token rotate / password change/setup
     - **Theme** (G27 ⭐) — `[System | Light | Dark]` radio
     - **Shortcut** (G26 ⭐) — read-only list, 카테고리별 (Editing / Layer-Z / Tool / Application)
     - **Storage**:
       - workspace path (read-only)
       - file_open allowlist editor (G21)
       - **Session export/import (G28 ⭐)** — [Export this session] / [Import session]
         - Export → `GET /api/sessions/<active>/layout` + meta header → Blob download
         - Import → file picker (`.gtmux.json`) → `POST /api/sessions/import` → 충돌 dialog [Rename / Override / Cancel]
     - **Behavior** (G25) — `auto_kill_terminal_on_panel_close: bool` default false
     - **Debug** — server pid, build sha, log path

### Stage 8 — Asset items (image/document, P2)
FE: ImageNode, DocumentNode + asset upload (file picker + drop)

### Stage 9 — Free draw + drawing perf
FE: FreeDrawNode + point simplification + backpressure

### Stage 10 — Hardening
FE: Playwright E2E + multi-tab race scenarios + accessibility

---

## 7. Build / dev / test

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/frontend
npm install
npm run check          # svelte-check
npm run build          # ⚠️ dev 모드 사용 금지

cd ..
GTMUX_FRONTEND_DIST=frontend/dist gtmux start --port 9999
```

⚠️ **함정**:
- `npm run dev` 사용 금지 — backend 가 release binary 로 `dist/` 를 SPA 서빙.
- xyflow / lucide-svelte 1.0.1 의 `$$props` 가 Svelte 5 strict 빌드와 충돌 — chrome 아이콘은 인라인 SVG.
- xterm.js WebGL renderer 권장. 한 panel = 한 인스턴스 — Stage 4 의 multi-xterm subscriber pattern 의 핵심.

---

## 8. BE 의존성 매트릭스 (plan-0007 §15 v2)

| 내 작업 | 의존 BE |
|---|---|
| FE-1 Auth page | BE-1 (auth handler), BE-NEW-7 (cookie) |
| FE-NEW-1 Session UI | BE-NEW-1 (WM), BE-NEW-2 (Session CRUD), BE-NEW-7 |
| FE-NEW-2 Attach lifecycle | BE-NEW-3 (attach handler) |
| FE-NEW-5 Attach confirm | BE-NEW-3 |
| FE-NEW-3 Terminal pool UI | BE-NEW-10 (terminal list), **BE-NEW-12.5** ⭐ |
| FE-NEW-6 Multi-xterm | BE-NEW-4 (WS routing), BE-NEW-10 |
| **PanelDanglingOverlay** ⭐ | **BE-NEW-12.5** ⭐ (terminal_died broadcast + respawn endpoint) |
| FE-6 Layer list V2 | BE-2, BE-NEW-2, BE-NEW-10, **BE-NEW-12.5** ⭐ (group close DELETE) |
| FE-7 Panel header V2 | BE-NEW-2, BE-NEW-10, **BE-NEW-12.5** ⭐ |
| FE-8 Settings UI | BE-1, BE-NEW-7, **BE-NEW-12** ⭐ (allowlist), **`POST /api/sessions/import`** ⭐ (G28) |
| FE-NEW-8 file_path open UX | BE-1, BE-NEW-7, **BE-NEW-12** ⭐ |

**진입 순서**: FE-3 / FE-NEW-7 (Stage 1) → FE-1 / FE-NEW-1 (Stage 2) → FE-NEW-2 / FE-NEW-5 (Stage 3) → FE-NEW-3/4/6 + **PanelDanglingOverlay** (Stage 4) → FE-2/4/5/NEW-8 (Stage 5) → FE-6/7 (Stage 6) → FE-9/8 + **shortcutRegistry/themeStore/import-export** (Stage 7).

---

## 9. Glossary

| 용어 | 의미 |
|---|---|
| **Webpage** | 1 브라우저 탭 = 1 WS 연결 = 1 session attach. |
| **Session** | workspace 안 named record. FE 의 layout 단위. |
| **Canvas** | 1 session 의 무한 작업 공간 (SvelteFlow). |
| **Canvas Item** | 10 type discriminated union. |
| **Panel** | `type:"terminal"` 인 Canvas Item — xterm 컨테이너. |
| **Group** | 자식들 묶음 (트리). z 없음. propagation: visibility AND / lock OR. |
| **Manipulation Selection (M)** | 사용자 제어 대상 Items. 다중. session-scoped. |
| **Input Target (I)** | 키보드 입력 라우팅 terminal. 1 session 안 unique. |
| **Streaming State** | (session, panel) 쌍 단위 — Streaming / Suspended. |
| **Dangling Terminal Reference** | layout terminal id 가 pool alive 와 매칭 안 됨. → focus interaction 시 same id fresh spawn (G25 c2). |
| **Mirror** | 1 terminal → N panel (다른 session 까지) attach + 입출력 공유. |
| **Match-or-spawn** | Session attach 시 id 매칭 또는 same id 로 fresh spawn. |
| **Auto-mount** | trigger session 의 layout 에만 cascade PUT. |

---

## 10. UX 공용 룰 (plan-0007 §14.20 SoT)

### 10.1 Inline edit (G23, §14.20.1)
- Single-line: Enter commit / Esc cancel / blur commit. 공용 `InlineEditField.svelte`.
- Multi-line: Cmd-Enter commit / Enter newline / Esc cancel / blur commit. 공용 `InlineEditTextarea.svelte`.

### 10.2 Esc 라우팅 (G20+G22+G23, §14.20.2)
7 우선순위 (위에서 아래로 첫 매치):
1. Inline edit 활성 → cancel
2. Modal stack top → close
3. Panel maximize 활성 → unmax
4. Tool locked → lock 해제 + Select 복귀
5. Tool 비-Select 인 상태 → Select 복귀
6. Selection 있음 → clear
7. (그 외) no-op

### 10.3 Toolbar 도구 (G22, §14.20.3)
- Default: 모두 one-shot. 사용 후 자동 Select 복귀.
- Q 단축키 또는 toolbar 아이콘 long-press = locked sticky.
- Esc = lock 해제 + Select 복귀.
- Select / Hand 는 mode always sticky.

### 10.4 Maximize (G20, §14.20.4)
- Canvas viewport area fill (Titlebar/Toolbar/Status bar 유지).
- FE-only ephemeral. 한 시점 1 panel.
- Unmaximize: Esc / 헤더 toggle / panel header double-click.

### 10.5 Keyboard shortcut (G26, §14.20.5) ⭐ **v2 신규**

#### 10.5.1 Layer 우선순위 (Hybrid, G26.1)
- **Modifier shortcut** (Cmd/Ctrl/Shift + key): 어디서든 발동 (xterm focus 포함).
- **Single-key shortcut** (`]` `[` `Q` `Enter`): xterm focus 외에만. xterm focus 시 shell typing.
- **Esc 특수 처리 (G26.2 Option i)**:
  - xterm focus → 항상 shell `\x1b` 전달 (vim/less/htop 자연)
  - 그 외 focus → `escRouter` (§14.20.2)
  - Modal stack 활성 시는 focus trap 자연

#### 10.5.2 P0 매트릭스 (Stage 1~6 필수)
| Shortcut | 동작 | 출처 |
|---|---|---|
| `Esc` | escRouter 7 우선순위 | G20+G22+G23 |
| `Enter` | Inline single-line commit | G23 |
| `Cmd/Ctrl+Enter` | Inline multi-line commit | G23 |
| `Q` | Tool lock toggle | G22 |
| `]` | Bring forward | ADR-0024 D2 |
| `[` | Send backward | ADR-0024 D2 |
| `Shift+]` | Bring to front | ADR-0024 D2 |
| `Shift+[` | Send to back | ADR-0024 D2 |
| **`Space hold + drag`** ⭐ | Viewport pan modifier | G29 |

#### 10.5.3 P1 매트릭스 (Stage 7+)
| Shortcut | 동작 |
|---|---|
| `Cmd/Ctrl+N` | New Terminal |
| `Cmd/Ctrl+Shift+L` | Layer list / Sidebar toggle |
| `Cmd/Ctrl+Shift+Q` | Server shutdown (confirm) |
| `Cmd/Ctrl+,` | Settings overlay |

#### 10.5.4 비범위 (P3)
- 사용자 customization (rebind UI)
- Chord shortcut (Cmd+K → Cmd+S 같은 2단계)

#### 10.5.5 Discoverability
- Settings → **Shortcut section** (read-only list, 카테고리별).
- Tooltip 옆 단축키 표시 (`⌘⇧]` Mac / `Ctrl+Shift+]` Win-Linux).
- `navigator.platform` 으로 detect.

#### 10.5.6 구현
- `lib/common/shortcutRegistry.svelte.ts` (신규):
  - 전역 `keydown` listener (window)
  - xterm focus 검사 (`document.activeElement.closest('.xterm')`)
  - Modifier + key code 매칭
  - Modal stack 정합 (focus trap 자연)
- Stage 6 까지 P0 만, Stage 7 에 P1 추가.

---

## 11. 작업 룰

- **English code/comments**, Korean docs.
- **점진 어휘 통일** — `pane` → `Terminal`.
- **Svelte 5 runes** — `$state` / `$derived` / `$effect`. 옛 writable/readable 점진 이전.
- **xyflow / SvelteFlow** — node type registry.
- **xterm.js** — WebGL renderer 권장. Theme adapter (G27 ⭐).
- **불필요한 추가 금지** — backwards compat, feature flag, 향후 가능성 추상화 거부.

---

## 12. 진입 시 첫 메시지 후보

- "Stage 1 FE 시작" → §6 의 Stage 1 (TS type + store).
- "FE-NEW-1 부터" → Stage 2 Auth + Dialog + Session list.
- "Layer list V2 시작" → Stage 6, ADR-0024 + ADR-0010 G25.
- "Multi-xterm 패턴 모름" → ADR-0021 D1/D2 + plan-0007 §14.17.
- **"shortcutRegistry 부터" ⭐** → Stage 7 (또는 Stage 5 에 인프라 준비), G26 + plan-0007 §14.20.5.
- **"Theme adapter 부터" ⭐** → Stage 7, G27 + plan-0007 §14.10.
- **"Session export/import" ⭐** → Stage 7 Settings Storage section, G28 + plan-0007 §13.9 / §14.8.

---

## 13. 변경 이력

- 2026-05-15 v1: 초안 — G18~G25 + multi-session pivot.
- 2026-05-15 v2: G26~G29 + D 검증 amend. §5 에 `shortcutRegistry` / `themeStore` / `xtermTheme` / Settings export/import + Toolbar2 (Select+Hand + Space-pan) 추가. §6 Stage 4 에 PanelDanglingOverlay (G25 c2), Stage 5 의 file_path 이동, Stage 7 의 G26/G27/G28 amend. §10 UX 공용 룰에 §10.5 Keyboard shortcut 신규 (P0/P1 매트릭스 + Discoverability + 구현). Reading list 에 ADR-0023 / 0024 / 0010 G25 amend 추가.
