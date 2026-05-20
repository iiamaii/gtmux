# gtmux — Frontend Agent Handover **v3**

> **v3 위치**: 2026-05-15 의 모든 grilling (G18~G29 + G32~G40 + Tier 3 + Tier 2 P2 deferred) + D 검증 + **별 session 의 FE 진행 분 (Stage 1~3 + Stage 5 부분 Toolbar2)** 반영. v1/v2 는 historical. **이 v3 만 읽으면 cold pickup OK**.
>
> 동반 문서: `docs/agents/backend-handover-v3.md` (BE v3)

---

## 0. v1 → v2 → v3 변경 요약

| 버전 | 추가 결정 + 진행 분 |
|---|---|
| v1 | G18~G25 + multi-session pivot 결과 |
| v2 | G26~G29 + D 검증 amend |
| **v3** | **G32~G40 + Tier 3 + Tier 2 P2 deferred + FE Stage 1~3 + Stage 5 (Toolbar2) 진행 분** |

v3 의 FE 신규 ⭐ 항목 (v2 대비):

| 변경 | 출처 | v3 영향 |
|---|---|---|
| **Cmd+D duplicate** (terminal=mirror) | G32 | §5/§10.5 P0 단축키 |
| **Viewport zoom (25%~200%)** + Cmd+0/1/2 (P0) + Cmd+./=/- (P1) | G33 | §5 FE-9 + §10.5 |
| **Focus mode (M 외 dim)** + Statusbar toggle + Cmd+Shift+F (P1) | G34 | §10.7 신규 |
| **Terminal sub-settings** (Global + Per-panel override) | G35 | §5 Panel Settings + Settings Terminal section |
| **Shell template** dropdown + Settings Templates CRUD | G36 | §5 Toolbar [Terminal▾] + Settings Templates section |
| **Resize semantic** Figma 표준 (Line/Free draw/Image/Multi) | G37~G40 | §10.6 신규 |
| **Server shutdown confirm modal** | Tier 3 | ✅ ship — `lib/chrome/ShutdownModal.svelte` (rename: `ServerShutdownConfirmModal` → `ShutdownModal`) |
| **WS reconnect backoff** (1s grace + exp 1/2/4/8/16, cap 30s, banner) | Tier 3 | ✅ ship — `lib/ws/client.ts` 안 통합 (별 `reconnect.svelte.ts` 분리 안 함) + `lib/ws/heartbeat.svelte.ts` (idle/stale detection) |
| **Session attach recovery — Case I (page entry) + Case II (idle reactivate)** | ADR-0019 D5.1 + D5.4 (G50 + 0045 P0 후속) | ✅ ship Phase 1+2 — `lib/stores/reconnectGate.svelte.ts` (8-state) + `lib/chrome/ReconnectModal.svelte` (4 mode) + `lib/stores/sessionStorageHint.ts` + `sessionStore.{attemptReattach, silentReattach, ensureMutationOk}` + `+page.svelte` boot screen + visibilitychange listener. 자세 = `docs/plans/0008-session-attach-recovery-impl.md` |
| **FE Stage 1~3 진행 분** (types/canvas, types/group, sessionStore, auth SPA, http modules, AuthDialog, NewSessionModal, SessionListModal, WorkspaceSwitcher, ActiveSessionDropdown, +page.svelte auth-gate, SessionMenu) | 별 session 작업 | §4 진행 분 표시 |
| **FE Stage 5 부분 (Toolbar2 + toolStore)** 진행 분 | 별 session 작업 | §4 |

---

## 1. Required reading (cold-pickup 순서)

| # | 파일 | 목적 |
|---|---|---|
| 1 | `/Users/ws/Desktop/projects/gtmux/CLAUDE.md` | 프로젝트 메타 (KO docs / EN code) |
| 2 | `/Users/ws/Desktop/projects/gtmux/CONTEXT.md` | 어휘 SoT + multi-session pivot + Terminal lifecycle + Z 정책 + Group 운영 |
| 3 | `/Users/ws/Desktop/projects/gtmux/docs/plans/0007-multi-session-pivot.md` | 본 plan §0~§18 — G18~G40 + Tier 2/3 모두 반영 단일 정본. **§14.20 공용 UX 운영 규칙 (§14.20.1~.7)** 특히. |
| 4 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0018-canvas-item-data-model.md` | Schema v2 + G20/G24/G35 amend (`terminal_overrides`) |
| 5 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0019-session-and-workspace-model.md` | Session/Workspace UI + lock peek 1s polling (G18) |
| 6 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0020-auth-lifecycle.md` | Auth + cookie |
| 7 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0021-terminal-pool-and-mirror.md` | Pool + multi-xterm + dangling + close dialog (G25) |
| 8 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0023-file-path-open-security.md` | file_path open modal + allowlist (G21) |
| 9 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0024-layer-tree-and-z-index-separation.md` | Layer/Z 분리 + 4 z 액션 (G24) |
| 10 | `/Users/ws/Desktop/projects/gtmux/docs/adr/0010-group-data-model.md` | Group propagation + G24/G25 amend |
| 11 | `/Users/ws/Desktop/projects/gtmux/docs/plans/0006-canvas-workspace-feature-roadmap.md` | UI 인벤토리 (supersede 됐으나 참조) |

선택:
- `docs/sketch.md` — KO 원본 design spec, 추가 UX 컨텍스트
- v1/v2 brief — historical
- `docs/reports/0030-sprint-7-closeout-and-handoff.md` — Sprint 7 종료

---

## 2. Mental model

```
Webpage (브라우저 탭) = 1 WS 연결 = 1 Session attach
│
├── 인증 통과 후 Dialog (새 session 추가 / 기존 session 연동)
│
├── Session attach → Canvas Layout (groups + items + viewport) 로드
│
│   Canvas Items (10 type):
│     - terminal (Panel) + optional terminal_overrides (G35)
│     - text/note/rect/ellipse/line/free_draw — 작업 메모/시각화
│     - image/document/file_path — asset 또는 bookmark
│
├── LeftPanel (ADR-0017 ②, 가로 탭 `[Layers | Terminals]` + 28px collapsed rail with tab icons):
│     - LayerTreeView (Tree | Z toggle, ADR-0024) — 구 `Sidebar.svelte` rename
│     - TerminalListView (server-wide pool) — 구 `TerminalListSection.svelte` 회수
│
├── RightPanel (ADR-0017 ③, 가로 탭 `[Inspect]` (현재 단일) + 28px collapsed rail):
│     - ItemInfoView — 구 `PaneInfoPanel.svelte` rename
│
├── Toolbar (Toolbar2 — 12 도구, [Terminal▾] dropdown G36)
│
├── Statusbar:
│     - ViewportCtrl cluster (zoom / fit all / fit selected / sync) G33
│     - [🎯 Focus mode] toggle G34
│
├── Modals / Overlays:
│     - SessionListModal (1s polling)
│     - NewSessionModal / AuthDialog
│     - AttachConfirmModal (match-or-spawn)
│     - PanelCloseConfirmModal (3 옵션, G25)
│     - GroupCloseConfirmModal (bulk, G25)
│     - FileOpenConfirmModal (G21)
│     - ShutdownModal (Tier 3, 구 `ServerShutdownConfirmModal` rename)
│     - ReconnectModal (ADR-0019 D5.1 + D5.4 — 4 mode loading/in_use/not_found/unreachable)
│     - SettingsOverlay (G19, full-screen)
│       + Auth/Theme/Shortcut/Terminal/Templates/Storage/Behavior/Debug sections
│
└── Stores / utilities:
      - sessionStore (FE-NEW-7) — 활성 session layout
      - toolStore (G22) — current tool + locked  [✅ 진행]
      - zStore (ADR-0024 D2) — Bring/Send 4 액션
      - focusStore (G34) — Focus mode on/off
      - shortcutRegistry (G26, `lib/keyboard/`)
      - chromeShortcuts (G26, `lib/keyboard/` — registry 첫 consumer: Cmd+Shift+L/I/,)
      - themeStore (G27, `lib/stores/theme.svelte.ts` — `mode` user / `resolved` $derived)
      - xtermTheme (G27, `lib/xterm/xtermTheme.ts` — XtermHost hot reload $effect)
      - settingsDialog (G19, `lib/stores/settingsDialog.svelte.ts` — open/section)
      - WS auto-reconnect (Tier 3, `lib/ws/client.ts` 통합) — 1s grace + exp backoff + ConnectionState 머신
      - heartbeat (`lib/ws/heartbeat.svelte.ts`) — frame/activity timestamp + `isStale` / `isIdle` derived (Phase 2 silent reattach 의 보조 신호)
      - reconnectGate (ADR-0019 D5.1 + D5.4 — 8 state `booting/idle/attaching/hydrating/in_use/not_found/unreachable/ready`, `canMountApp` / `modalState` derived, `start/cancel/retry/markIdle/markReady`)
      - sessionStorageHint (`gtmux-last-active-session` tab-scoped) — Page entry 자동 reattach hint
      - escRouter (§14.20.2) — 7 우선순위 키 라우팅
      - workspaceSwitcher (Stage 3 진행 분) — Auth Dialog stage 머신
```

핵심 관계:
- **Webpage : Session = 1:1** (single-attach reciprocal)
- **Tree : Z = 분리** (ADR-0024) — drag = organization, z = 명시 액션
- **Terminal mirror** = 1 terminal → N panel
- **Multi-xterm subscriber** — panel 마다 인스턴스
- **Session-scoped store** — 활성 session 만
- **Template : Terminal spawn = 1:N** (G36)

---

## 3. Architectural invariants (FE)

1. **session-scoped store** (ADR-0019 + ADR-0021 D5).
2. **Auto-mount = trigger session 만** (ADR-0021 D3).
3. **Multi-xterm subscriber 패턴** (ADR-0021 D1).
4. **Tree order ≠ Z** (ADR-0024) — Group z 없음.
5. **Maximize = FE-only ephemeral** (G20).
6. **Esc 라우팅** (§14.20.2 7 우선순위).
7. **Inline edit** (G23) — 공용 `InlineEditField` / `InlineEditTextarea`.
8. **Toolbar = one-shot + Q lock** (G22) — Select/Hand mode sticky. **Space+drag = pan modifier** (G29).
9. **Settings = full-screen overlay + auto-save** (G19).
10. **Keyboard shortcut Hybrid** (G26) — single-key 는 xterm 외, modifier 는 항상. **xterm Esc = shell ESC byte** (vim 자연).
11. **Panel close dialog 3 옵션** (G25) — auto_kill_terminal_on_panel_close default false.
12. **Dangling lazy spawn** (G25) — terminal SIGTERM → [exit] overlay → focus → respawn.
13. **Theme** (G27) — light/dark 2 fixed + system detect.
14. **Item duplicate** (G32) — terminal=mirror, Cmd+D, (20,20) cascade.
15. **Viewport zoom 25%~200%** (G33) — Cmd+0/1/2 P0, Cmd+./=/- P1.
16. **Focus mode = M 외 dim** (G34) — ephemeral, Statusbar [🎯], Cmd+Shift+F (P1).
17. **Resize semantic Figma 표준** (G37~G40).
18. **점진 어휘 통일** `pane` → `Terminal`.

---

## 4. 현 코드 상태 (2026-05-15)

### 4.1 Sprint 7 완료 (single-session 시대)
- HEAD `1e84f4c` (Sprint 7 closeout)
- `svelte-check`: 0/0 (Sprint 7 시점)
- Stage A~K: Figma adaptation, chrome 9건, SvelteFlow Canvas, single-session WS sync, xterm.js 1:1, NodeResizer Terminal Panel resize (corner+edge handles → xterm fit + PANE_RESIZE WS frame).

### 4.2 multi-session pivot 진행 분 (별 session 작업, 2026-05-15)
- ✅ **Stage 1** — Foundation 완성
  - `lib/types/canvas.ts` (CanvasItem 10-type discriminated union + Viewport + CanvasLayout envelope + 10 type guard)
  - `lib/types/group.ts` (Group + propagation helpers: effectiveVisibility AND, effectiveLocked OR, inheritedLabel/Color, descendant/ancestor walk)
  - `lib/stores/sessionStore.svelte.ts` (session-scoped layout/M/I/viewport/maximize)
  - 후속 wiring 은 Stage 4 BE-NEW-4 통합 후
- ✅ **Stage 2** — Auth + Dialog + Session list (BE 정합)
  - `/auth-preview` SPA page (`ref/frontend-design/auth.html` 차용 — BE `/auth` 는 server-rendered HTML 로 BE 가 owns)
  - `lib/types/{sessions,auth}.ts` / `lib/http/{sessions,auth}.ts` (cookie 인증 + `credentials:'include'`; BE actual contract — `{token? | password?}` + `retry_after_secs`)
  - `lib/chrome/{AuthDialog,NewSessionModal,SessionListModal}.svelte`
- ✅ **Stage 3 부분** — Workspace 진입 + auth-gate
  - `src/main.ts` path-based mount (`/auth-preview` SPA, 그 외 AppPage)
  - `lib/chrome/AttachConfirmModal.svelte` (BE confirm_required 미구현이라 *dead* — Stage 3+ wake-up)
  - `lib/chrome/ActiveSessionDropdown.svelte`
  - `lib/chrome/WorkspaceSwitcher.svelte` (modal stack 통합)
  - `lib/stores/workspaceSwitcher.svelte.ts` (stage 머신: closed/choice/create/list/attach_confirm)
  - `routes/+page.svelte` amend (auth-gate: `/api/sessions` 401 → `/auth` redirect / 200 + active session 없음 → `workspaceSwitcher.open()` 자동, `?t=<token>` 자동 sessionStorage 캡처 + URL clean, prompt fallback 제거)
  - `SessionMenu` amend ([Switch workspace session… / Sign out])
- ✅ **Stage 5 부분 — Toolbar2 (FE-2)** — G22 + 부분 G29 (12 도구)
  - `lib/stores/toolStore.svelte.ts` (G22 one-shot + Q lock + Esc chain)
  - `lib/toolbar/Toolbar2.svelte` (12 도구: Select/Hand/Terminal/Rect/Ellipse/Line/FreeDraw/Text/Note/Image/Document/FilePath + group dividers + tooltip + Q-lock visual ring)
  - 도구 ↔ ItemType 1:1 매핑 (ADR-0018 D4)
  - Stage 5 의 creation gestures + node renderer 는 후속
- ⚠️ BE-FE contract 정합 작업
  - `lib/http/sessions.ts`: `GET /api/sessions` bare array 정규화, `POST` flat `{name}` → synthesize SessionInfo, `attach` 의 200 → `getLayout()` 별도 호출 + 409 `holder.pid` 파싱 + 404 명시 throw, 새 `getLayout(name)` export

### 4.3 잔여 ❌ (Stage 4 + Stage 5 잔여 + Stage 6~7 + Tier 1/2/3 신규)
- ❌ **Multi-xterm subscriber** (Stage 4) — BE-NEW-4 (WS cookie 통합) 후
- ❌ **Terminal pool UI** (Stage 4) — BE-NEW-10 후
- ❌ **PanelDanglingOverlay** ⭐ (Stage 4, G25 c2) — BE-NEW-12.5 후
- ❌ **Toolbar [Terminal▾] dropdown** ⭐ (Stage 4/5, G36) — BE-NEW-10 template 후
- ❌ Non-terminal Item renderers (Stage 5) — TextNode/NoteNode/ShapeNode/LineNode/FilePathNode
- ❌ Creation gestures (Stage 5) — toolStore 는 done
- ❌ **file_path open UX + FileOpenConfirmModal** ⭐ (Stage 5, G21) — BE-NEW-12 후
- ❌ **LineNode endpoint handles + FreeDraw scale + Image aspect lock** ⭐ (Stage 5, G37~G40)
- ❌ **Multi-item bbox resize handler** ⭐ (Stage 5, G40)
- ❌ Layer list V2 (Stage 6) — Tree/Z toggle, group propagation 시각화, drag reorder
- ❌ **GroupCloseConfirmModal** ⭐ (Stage 6, G25)
- ❌ Panel header V2 (Stage 6) — 4 z 액션 + Kill/Remove + close dialog + dangling overlay
- ❌ **PanelCloseConfirmModal** ⭐ (Stage 6, G25)
- ❌ **Panel Settings modal + Terminal Override sub-section** ⭐ (Stage 6, G35)
- ❌ **ContextMenu (canvas right-click)** ⭐ (Stage 6, 4 z 액션 + close)
- ❌ **Cmd+D duplicate handler** ⭐ (Stage 6 또는 함께, G32) — multi-select 일괄 + (20,20) cascade
- ❌ **zStore + Bring/Send 4 액션** ⭐ (Stage 6, ADR-0024 D2)
- ❌ **ViewportCtrl 확장** ⭐ (Stage 7, G33) — fit all / fit selected / go to selection / zoom level / sync indicator
- ❌ **Statusbar [🎯 Focus mode] toggle** ⭐ (Stage 7, G34) + focusStore + dim 적용
- 🟨 **SettingsOverlay (G19)** ⭐ (Stage 7, ADR-0017 ④ 으로 chrome 부분 ship) — chrome + Theme · Shortcuts section wire 완료; Storage/Auth/Behavior/Debug/Terminal/Templates 은 BE wire 시 별 amend (placeholder + "Waiting on BE: ..." 명시)
- ✅ **shortcutRegistry** ⭐ (`lib/keyboard/`, ADR-0017 ④) — chromeShortcuts (Cmd+Shift+L LeftPanel / Cmd+Shift+I RightPanel / Cmd+, Settings) + zShortcuts 마이그레이션 완료. Esc 는 `escRouter` 별도 유지
- ✅ **themeStore + xtermTheme** ⭐ (ADR-0017 ④ + ④ follow-up) — `lib/stores/theme.svelte.ts` (`mode` user / `resolved` $derived, MediaQuery bind) + `lib/xterm/xtermTheme.ts` (XtermHost mount + hot reload $effect, `--canvas-bg` light flash 방지)
- ❌ **Session export/import** ⭐ (Stage 7, G28)
- ✅ **ShutdownModal** ⭐ (Stage 7, Tier 3) — 구 `ServerShutdownConfirmModal` rename. `lib/chrome/ShutdownModal.svelte` ship
- ✅ **WS auto-reconnect** ⭐ (Stage 7, Tier 3) — `lib/ws/client.ts` 통합 (별 utility 분리 안 함) + `lib/ws/heartbeat.svelte.ts` (idle/stale detection)
- ✅ **Session attach recovery (Case I + Case II)** ⭐ (Stage 7, ADR-0019 D5.1 + D5.4) — `reconnectGate.svelte.ts` 8-state 머신 (0045 P0 후속 P1.9) + `ReconnectModal.svelte` 4 mode + `sessionStorageHint.ts` + `sessionStore.{attemptReattach, silentReattach, ensureMutationOk}` + `+page.svelte` boot screen + visibilitychange listener + 7+ mutation 진입점 guard. plan-0008 §5 (P1.1~P1.9) + §6 ✅ ship
- ❌ Rotate-token UI (skipped — BE 다음 stage)

---

## 5. Frontend 기능 명세

### P0 (Stage 1~4)

| ID | 이름 | 진행 | Stage | ADR |
|---|---|---|---|---|
| FE-3 | TS `CanvasItem` discriminated union | ✅ done | 1 | 0018 D1 |
| FE-NEW-7 | Session-scoped store 분리 | ✅ done | 1 | 0019 + 0021 D5 |
| FE-1 | Auth page | ✅ done (`/auth-preview` SPA) | 2 | 0020 |
| FE-NEW-1 | Session UI (Dialog/Modal/Menu/Dropdown) | ✅ done (대부분) | 2 | 0019 + G18 |
| FE-NEW-2 | Webpage attach lifecycle | ✅ done — heartbeat client (`lib/ws/heartbeat.svelte.ts`) + visibilitychange listener (`+page.svelte`) + Phase 2 silent reattach trigger (`dispatcher.svelte.ts` 의 reconnecting→open) 모두 ship (plan-0008 §6) | 2~3 | 0019 + 0021 D6 + 0019 D5.1 |
| FE-NEW-5 | Match-or-spawn confirm dialog | ✅ done (dead until BE) | 3 | 0018 D6 |
| FE-NEW-3 | Terminal pool UI (`TerminalListView` in LeftPanel) | 🟨 chrome scaffold (ADR-0017 ②) — full wire = Stage 4 잔여 | 4 | 0021 D7 + ADR-0017 ② |
| FE-NEW-4 | Terminal binding UI | ❌ | 4 | 0021 D8 |
| FE-NEW-6 | Multi-xterm subscriber | ❌ | 4 | 0021 D1 |
| **PanelDanglingOverlay** ⭐ | Terminal SIGTERM 시 [exit] overlay → respawn (G25 c2) | ❌ | 4 | 0021 D10.1 |
| FE-6 | Layer list V2 (Tree/Z, propagation, group close) | ❌ | 6 | 0021 D7 + 0024 + G25 |
| FE-7 | Panel header V2 (4 z + Kill/Remove + close + dangling + **Settings modal + Terminal Override** G35) | ❌ | 6 | 0021 D8 + 0024 D2 + G25 + G35 |

### P1 (Stage 5~7)

| ID | 이름 | 진행 | Stage | ADR / Grilling |
|---|---|---|---|---|
| FE-2 | Toolbar2 (12 도구 + one-shot + Q lock) | ✅ done | 5 | G22 |
| **FE-2 amend (G36)** ⭐ | Toolbar [Terminal▾] dropdown + Manage templates | ❌ | 5 | G36 |
| FE-4 | Item Renderers (text/note/rect/ellipse/**line**/file_path) | ❌ | 5 | 0018 + G37 (line endpoint) |
| FE-NEW-8 | file_path open UX + confirm modal | ❌ | 5 | 0023 |
| FE-5 | Creation gestures | ❌ | 5 | G22 |
| **Cmd+D duplicate** ⭐ | terminal=mirror, multi-select 일괄, (20,20) cascade | ❌ | 5/6 | G32 |
| **zStore + 4 z 액션** ⭐ | Bring to front / Send to back / Bring forward / Send backward | ❌ | 6 | ADR-0024 D2 |
| FE-9 | Viewport sync UI + **ViewportCtrl 확장** (G33) | ❌ | 7 | 0019 + 0021 D5 + G33 |
| **focusStore + Statusbar toggle** ⭐ | Focus mode (M 외 dim) | ❌ | 7 | G34 |
| **shortcutRegistry** ⭐ | 전역 keydown + xterm focus 검사 + P0/P1 매트릭스 | ✅ (`lib/keyboard/`, ADR-0017 ④) | 5+ (인프라) / 7 (P1) | G26 + ADR-0017 ④ |
| **themeStore + xtermTheme** ⭐ | light/dark + system + chrome/xterm 동기 | ✅ (`lib/stores/theme.svelte.ts` + `lib/xterm/xtermTheme.ts`, ADR-0017 ④ + ④ follow-up) | 7 | G27 + ADR-0017 ④ |
| FE-8 | Settings UI (overlay + auto-save + **8 sections**) | 🟨 부분 (chrome + Theme + Shortcuts section wired, 나머지 BE 대기) | 7 | G19 + G26 + G27 + G28 + G35 + G36 + ADR-0017 ④ |
| **Session export/import** ⭐ | Settings Storage 의 [Export/Import] | ❌ | 7 | G28 |
| **ShutdownModal** ⭐ | Cmd+Shift+Q (P1) + SessionMenu — 구 `ServerShutdownConfirmModal` rename | ✅ (`lib/chrome/ShutdownModal.svelte`) | 7 | Tier 3 |
| **WS auto-reconnect** ⭐ | 1s grace + exp 1/2/4/8/16, cap 30s, ConnectionState 머신 | ✅ (`lib/ws/client.ts` 통합 + `heartbeat.svelte.ts`) | 7 | Tier 3 |
| **Session attach recovery** ⭐ | Case I (page entry blocking) + Case II (silent + mutation guard) — D5.1 + D5.4 | ✅ Phase 1+2 ship (plan-0008 §5/§6) | 7 | ADR-0019 D5.1 + D5.4 |
| **Session delete UI** ⭐ | SessionListModal `Available` row hover-kebab [⋯] + SessionMenu "Delete current session…" — entry point 2 곳 (D10.1) | ❌ | 7 | ADR-0019 D10 + D10.1 |
| **Text style 풀** ⭐ | `font_family / font_weight / font_style / text_decoration / line_height` 5 옵셔널 필드 — TextNode renderer + Inspector text section + InlineEditTextarea 정합. **ADR-0018 D4 `text` payload amend 필요** | ❌ | 5 | ADR-0018 D4 amend pending |
| **Figure stroke/fill 패턴** ⭐ | `stroke_dash` (solid/dash/dot/dashdot) + `fill_pattern` (solid/none/hatch) — ShapeNode + LineNode renderer + Inspector shape section. **ADR-0018 D4 `rect/ellipse/line` payload amend 필요** | ❌ | 5 | ADR-0018 D4 amend pending |
| **Item rotation** ⭐ | `rotation?: number` (deg, 0~360, default 0) — *ItemCommon cross-cut*. 모든 visual renderer 의 transform + Figma 컨벤션 rotate grip + 15° snap (Shift = 자유) + Inspector geometry row. BBox/Multi-item resize (G40) 정합 필요. **ADR-0018 D2 ItemCommon amend 필요** | ❌ | 5~6 | ADR-0018 D2 amend pending |

### P2 (Stage 8~10, deferred — plan-0007 §10.2)

| ID | 이름 | 비고 |
|---|---|---|
| FE-4 | Item Renderers (image/document) | Stage 8 + ADR-0022 |
| FE-4 | Item Renderers (free_draw) | Stage 9 + G38 |
| FE-10 | UX polish | 지속 |
| FE-11 | Tests (Vitest + Playwright) | Stage 10 |

### 공용 컴포넌트 / store

| 파일 | 용도 | 진행 |
|---|---|---|
| `lib/types/canvas.ts` | CanvasItem 10 type | ✅ |
| `lib/types/group.ts` | Group propagation helpers | ✅ |
| `lib/types/{sessions,auth}.ts` | API DTO | ✅ |
| `lib/http/{sessions,auth}.ts` | HTTP client | ✅ |
| `lib/stores/sessionStore.svelte.ts` | session-scoped layout | ✅ |
| `lib/stores/toolStore.svelte.ts` | Toolbar tool state | ✅ |
| `lib/stores/workspaceSwitcher.svelte.ts` | Auth+Dialog stage 머신 | ✅ |
| `lib/common/InlineEditField.svelte` | Single-line inline edit | ✅ (G23) |
| `lib/common/InlineEditTextarea.svelte` | Multi-line inline edit | ✅ (G23) |
| `lib/common/escRouter.svelte.ts` | Esc 7 우선순위 | ✅ (§14.20.2) |
| `lib/keyboard/shortcutRegistry.svelte.ts` ⭐ | 전역 keydown + xterm focus + editable 가드 | ✅ (G26 + ADR-0017 ④) |
| `lib/keyboard/chromeShortcuts.svelte.ts` ⭐ | registry 첫 consumer (Cmd+Shift+L/I/,) | ✅ (ADR-0017 ④) |
| `lib/keyboard/zShortcuts.svelte.ts` ⭐ | Z 4 액션 단축키 (registry 마이그레이션) | ✅ |
| `lib/stores/zStore.svelte.ts` ⭐ | Z mutation 4 액션 | ✅ (ADR-0024) |
| `lib/stores/focusStore.svelte.ts` ⭐ | Focus mode on/off | ❌ (G34) |
| `lib/stores/theme.svelte.ts` ⭐ | `mode` user + `resolved` $derived (MediaQuery bind) | ✅ (G27 + ADR-0017 ④) |
| `lib/stores/settingsDialog.svelte.ts` ⭐ | SettingsOverlay open/section store | ✅ (G19 + ADR-0017 ④) |
| `lib/xterm/xtermTheme.ts` ⭐ | light/dark xterm theme (XtermHost hot reload $effect) | ✅ (G27 + ADR-0017 ④ follow-up) |
| `lib/sidebar/LeftPanel.svelte` ⭐ | chrome owner (가로 탭 + 28px rail) | ✅ (ADR-0017 ②) |
| `lib/sidebar/LayerTreeView.svelte` ⭐ | 구 `Sidebar.svelte` rename — layer tree content | ✅ (ADR-0017 ②) — V2 wire = Stage 6 |
| `lib/sidebar/TerminalListView.svelte` ⭐ | Terminals tab content (server-wide pool) | 🟨 chrome scaffold (ADR-0017 ②) — full wire = Stage 4 |
| `lib/chrome/RightPanel.svelte` ⭐ | chrome owner (Inspect 탭 + 28px rail) | ✅ (ADR-0017 ③) |
| `lib/chrome/ItemInfoView.svelte` ⭐ | 구 `PaneInfoPanel.svelte` rename — Inspect tab content | ✅ (ADR-0017 ③) |
| `lib/chrome/PanelFoldButton.svelte` ⭐ | Panel header fold (▶/◀ chevron) | ✅ (ADR-0017 ① 유효 잔존) |
| `lib/chrome/SettingsOverlay.svelte` ⭐ | full-screen overlay (G19) | 🟨 chrome + Theme · Shortcuts ship, 그 외 placeholder (ADR-0017 ④) |
| ~~`lib/ws/reconnect.svelte.ts`~~ → `lib/ws/client.ts` 통합 ⭐ | WS auto-reconnect (1s grace + exp backoff + ConnectionState) | ✅ (Tier 3) — 별 파일 분리 안 함, client.ts 안 |
| `lib/ws/heartbeat.svelte.ts` ⭐ | Frame timestamp (`lastFrameAt`) + Activity timestamp (`lastActivityAt`) + `isStale` / `isIdle` derived. Phase 2 silent reattach 보조 신호 | ✅ (ADR-0021 D6 + plan-0008 §6 Phase 2) |
| `lib/stores/reconnectGate.svelte.ts` ⭐ | Page entry blocking 상태 머신 (8 state). `canMountApp` / `modalState` derived. `start/cancel/retry/markIdle/markReady`. AbortController 보유. Initial = `'booting'` | ✅ (ADR-0019 D5.4 + 0045 P0 후속 P1.9, plan-0008 §4.4) |
| `lib/stores/sessionStorageHint.ts` ⭐ | `gtmux-last-active-session` 의 tab-scoped get/set/clear (SSR/private safe) | ✅ (ADR-0019 D5.4, plan-0008 §4.5) |
| `lib/chrome/ReconnectModal.svelte` ⭐ | 4 mode (loading / in_use / not_found / unreachable) modal. backdrop + center card + focus trap + Esc/backdrop 비활성. prop 이름 `mode` (svelte-check $state heuristic 회피) | ✅ (D5.1 + D5.4, plan-0008 §1.3 / §4.3) |
| `lib/chrome/ShutdownModal.svelte` ⭐ | 구 `ServerShutdownConfirmModal` rename — Cmd+Shift+Q 또는 SessionMenu | ✅ (Tier 3) |
| `lib/stores/sessionStore.svelte.ts` 의 `attemptReattach` + `silentReattach` + `#silentReattachPromise` + `ensureMutationOk(message?)` exported helper ⭐ | Phase 1 의 attempt utility + Phase 2 의 silent path (in-flight singleton) + mutation guard 의 사용자-facing wrapper | ✅ (plan-0008 §4.4 / §6.1) |
| **회수**: `lib/chrome/Sidebar.svelte` → `LayerTreeView.svelte` rename / `lib/chrome/PaneInfoPanel.svelte` → `ItemInfoView.svelte` rename / `lib/chrome/RailToggle.svelte` 폐기 (두 panel 모두 self-contained 28px rail) — ADR-0017 ②/③

---

## 6. Stage-by-stage 잔여 업무 할당

### Stage 4 — Terminal pool + Multi-xterm + Dangling overlay + Template dropdown ⭐
**잔여 작업**:
1. `lib/sidebar/TerminalListView.svelte` (LeftPanel 의 Terminals 탭, ADR-0017 ②) — `GET /api/terminals`, 우클릭 menu [Attach to canvas] / [Kill terminal] (chrome scaffold ship, full wire = 본 stage 잔여)
2. `ChangeTerminalModal.svelte` — Panel context menu 진입
3. `PanelNode.svelte` 큰 amend — **multi-xterm subscriber pattern** (FE-NEW-6)
   - Panel 마다 xterm 인스턴스
   - WS frame `terminal_id` 분기 → 해당 id stream 만 write
   - Panel dispose 시 그 xterm 만 dispose
4. **`PanelDanglingOverlay.svelte` (G25 c2)** ⭐ — `terminal_died` WS frame → `[exit code N] — Click to restart` overlay → focus/click/input → `POST /api/terminals { id, fresh_spawn: true }`
5. **Toolbar [Terminal▾] dropdown (G36)** ⭐:
   - click = default template spawn
   - icon 우측 `▾` 또는 long-press → dropdown 의 template list
   - dropdown 의 [+ Manage templates...] → Settings → Templates 이동 (Stage 7)
   - `GET /api/terminal-templates` fetch + 표시

**Integration gate (smoke-6/6b/6c)**:
- smoke-6: multi-tab attach 같은 terminal mirror.
- smoke-6b (G25): [Panel + Terminal] → mirror panel [exit] overlay → click → respawn.
- smoke-6c (G36): template dropdown → python → python REPL terminal spawn.

### Stage 5 — Canvas Item 잔여
**잔여 작업** (toolStore + Toolbar2 는 done):
1. Per-type Node renderer: `TextNode`, `NoteNode`, `ShapeNode`, **`LineNode` (endpoint handles G37)**, **`FreeDrawNode` (scale path G38)**, **`FilePathNode`**
2. Creation gestures: click-to-create / drag-to-create / pointer capture / cancel on Esc (§14.20.2)
3. Inline edit (G23 공용 컴포넌트 신규 + 사용)
4. **FE-NEW-8 (G21) file_path open UX** ⭐:
   - `FilePathItem.svelte` — double-click → allowlist-check → confirm modal / 즉시 open
   - `FileOpenConfirmModal.svelte` — path + [✓ Always for *.{ext} within {prefix}/] + [Cancel] [Open]
5. **Image aspect ratio lock (Shift+drag, G39)** ⭐ — ImageNode renderer
6. **Multi-item bbox resize (G40)** ⭐ — M (multi-selection) corner drag → 모든 selected items 비례 scale
7. **Text style 풀 (ADR-0018 D4 `text` amend pending, 2026-05-17 등록)** ⭐:
   - Payload 추가 후보: `font_family?: string` (system stack | mono | serif | sans + free token), `font_weight?: 100~900 | "normal" | "bold"`, `font_style?: "normal" | "italic"`, `text_decoration?: "none" | "underline" | "line-through"`, `line_height?: number` (0.8~2.0).
   - `TextNode.svelte` renderer 가 CSS 매핑 (`font-family` / `font-weight` / `font-style` / `text-decoration` / `line-height`).
   - Inspector v2 의 text section 에 control row 4~5 개 — family dropdown / weight stepper / italic toggle / decoration toggle / line-height slider.
   - Inline edit (`InlineEditTextarea`) 도 표시 상태와 같은 font CSS 상속.
   - Default fallback: family = system stack / weight = 400 / style = normal / decoration = none / line-height = 1.4.
   - 본 batch land 시 ADR-0018 D4 `text` row + BE serde `Item::Text` struct + openapi 재발행 동시 정합.
8. **Figure stroke/fill 패턴 (ADR-0018 D4 `rect/ellipse/line` amend pending, 2026-05-17 등록)** ⭐:
   - Stroke 확장: `stroke_dash?: "solid" | "dash" | "dot" | "dashdot"` (SVG `stroke-dasharray` 매핑). `rect / ellipse / line` 공통.
   - Fill 확장: `fill_pattern?: "solid" | "none" | "hatch"`. `rect / ellipse` 만 (line 은 stroke only).
   - `ShapeNode.svelte` + `LineNode.svelte` renderer amend — SVG `stroke-dasharray` 분기 + `fill` 분기 (hatch 는 SVG `<pattern>` defs).
   - Inspector v2 의 shape section 에 stroke pattern dropdown + fill pattern dropdown.
   - 별 gradient / image fill 같은 복잡 패턴은 P2+ (별 ADR 후보).
   - 본 batch land 시 ADR-0018 D4 `rect/ellipse/line` row + BE serde `Item::Rect/Ellipse/Line` + openapi 재발행 동시 정합.
9. **Item rotation (ADR-0018 D2 `ItemCommon` amend pending, 2026-05-17 등록)** ⭐ — *cross-cut* 변경:
   - `ItemCommon` 에 `rotation?: number` (degree, 0~360, default 0) 추가 후보.
   - 모든 visual renderer (PanelNode / TextNode / NoteNode / ShapeNode / LineNode / FreeDrawNode / ImageNode / DocumentNode / FilePathNode / CaptionNode) 의 wrapper transform 에 `rotate(${rotation}deg)` 적용 (center 기준).
   - Resize handle 의 bbox 외부 +20px 위치에 추가 **rotate grip** (Figma / Excalidraw 컨벤션). Drag → 중심 기준 회전 + 각도 indicator.
   - Snap: 15° 단위 (Shift hold = 자유 회전).
   - Inspector geometry row 에 rotation slider (0~360) + 수치 입력 + reset 버튼 (= 0°).
   - **BBox 계산 영향** — 회전 후 axis-aligned bbox 로 재계산. Multi-item bbox resize (G40, §6 항목) + Alignment (plan-0010 §1) + Layer tree drag reorder 의 bbox 의존 부분과 정합 필요 — 별 sub-test.
   - SvelteFlow connection point / hover hitbox 도 회전 정합 (별 sub-test).
   - 본 batch land 시 ADR-0018 D2 ItemCommon row + BE serde `ItemCommon` + openapi 재발행 + 모든 renderer 의 transform amend 동시.

**Integration gate**:
- smoke-7: Text 도구 → click → text item → inline edit → reload 복원.
- smoke-7b (G21): File Path 도구 → path → file_path item → double-click → confirm → toast.
- smoke-7c (text style, §7 amend land 시): bold + italic + underline + 1.6 line-height 적용 → PUT layout → reload 시 CSS 정합.
- smoke-7d (figure pattern, §8 amend land 시): rect dash stroke + hatch fill → reload 정합.
- smoke-7e (rotation, §9 amend land 시): 45° rotation → multi-item resize → bbox 정합 + 별 alignment 흐름.

### Stage 6 — Layer list V2 + Panel header
**잔여 작업**:
1. `lib/sidebar/LayerTreeView.svelte` amend (구 `Sidebar.svelte` rename per ADR-0017 ②) — Layer list V2 (Tree/Z toggle, multi-select, per-row toggles, propagation 시각화, drag reorder, inline rename, group context menu)
2. **`zStore.svelte.ts` + 4 z 액션 (ADR-0024 D2)** ⭐
3. **`GroupCloseConfirmModal.svelte` (G25)** ⭐ — bulk 1 dialog (3 옵션 + mirror hint)
4. **`PanelNode.svelte` 큰 amend (FE-7)**:
   - Header redesign + more menu (… 4 z + Kill/Remove + Rename/Settings)
   - Close 버튼 (X) → **`PanelCloseConfirmModal.svelte` (G25)** ⭐ — 3 옵션 + mirror hint
   - Dangling overlay (Stage 4 의 산출)
5. **`PanelSettingsModal.svelte` (G35 + Panel Settings)** ⭐ — General + **Terminal Override sub-section** (per-panel font/wrap/scrollback/cursor/bell override)
6. **`ContextMenu.svelte` (canvas right-click)** ⭐ — 4 z 액션 + [Close panel...] + [Duplicate (Cmd+D)] + [Kill terminal] + [Remove panel]
7. **Cmd+D duplicate handler (G32)** ⭐ — multi-select 일괄 + terminal mirror + (20,20) cascade

**Integration gate (smoke-8)**: multi-select → Group → visibility → Group [Delete] → bulk modal → confirm. Panel close → PanelCloseConfirmModal → option. Cmd+D → duplicate (terminal mirror).

### Stage 7 — Viewport + Settings + Theme + Shortcut + Shutdown + Reconnect ⭐ (큰 amend)
**잔여 작업**:
1. **`FE-9 Viewport sync UI 확장` (G33)** ⭐:
   - ViewportCtrl: zoom in/out (`Cmd+=` / `Cmd+-`) / reset 100% (`Cmd+0`) / fit all (`Cmd+1`) / fit selected (`Cmd+2`) / go to selection (`Cmd+.`) / selection count / sync indicator
   - UI: Statusbar 우측 cluster
   - Zoom 범위 25%~200% (clamp)
   - Mouse wheel + trackpad native pinch/pan
2. **`focusStore.svelte.ts` + Statusbar [🎯 Focus mode] toggle (G34)** ⭐:
   - M 외 dim (50% opacity)
   - Streaming 영향 X
   - Ephemeral (attach 마다 fresh)
   - Cmd+Shift+F (P1)
   - M 변경 시 dim 자동 갱신
3. ✅ **`lib/keyboard/shortcutRegistry.svelte.ts` (G26, ADR-0017 ④ 으로 ship)**:
   - 전역 `keydown` listener (window) — done
   - xterm focus 검사 + editable 가드 (modifier 있으면 default `true`, plain key 는 default `false`, 호출자 override 가능) — done
   - 첫 consumer: `lib/keyboard/chromeShortcuts.svelte.ts` (Cmd+Shift+L LeftPanel / Cmd+Shift+I RightPanel / Cmd+, Settings) — done
   - `zShortcuts` registry 마이그레이션 — done
   - Esc 는 별 dispatcher (`escRouter`) 유지 — priority chain (inline-edit > modal > unmaximize > tool > select) 이 flat keycombo table 로 자연 매핑되지 않음
4. ✅ **`lib/stores/theme.svelte.ts` + `lib/xterm/xtermTheme.ts` (G27, ADR-0017 ④ + ④ follow-up 으로 ship)**:
   - `mode: ThemeMode = "system"|"light"|"dark"` ($state, user choice) — done
   - `resolved: Theme = "light"|"dark"` ($derived) — done
   - System mode = `bindSystemListener()` MediaQuery listener (`+page.svelte` onMount / onDestroy) — done
   - `:root[data-theme]` CSS variable + `index.html` FOUC guard — done
   - XtermHost mount 시 `xtermTheme(themeStore.resolved)` 적용 + 별도 $effect 로 hot reload (theme flip 시 live `term.options.theme` 교체) + xterm-host 컨테이너 background `--canvas-bg` (light 모드 black flash 방지) — done
   - localStorage `gtmux-theme` schema = `'system'|'light'|'dark'` (이전 `'light'|'dark'` 와 graceful fallback) — done
5. 🟨 **`lib/chrome/SettingsOverlay.svelte` + `lib/stores/settingsDialog.svelte.ts` (G19 + G26 + G27 + G28 + G35 + G36, ADR-0017 ④ 으로 chrome 부분 ship)**:
   - ✅ Full-screen overlay (880×640 max, viewport responsive) + 좌측 section nav + 오른쪽 section pane
   - ✅ 진입점 3개: SessionMenu "Settings…" / `Cmd+,` (shortcutRegistry) / `settingsDialog.show(section?)` 직접 호출
   - ✅ Theme section (G27) — `[System | Light | Dark]` radio wire
   - ✅ Shortcuts section (G26) — read-only matrix wire
   - ❌ Storage / Auth / Behavior / Debug / Terminal / Templates section — placeholder + "Waiting on BE: ..." 명시 (BE wire 시 별 amend)
   - ✅ 즉시 자동 저장 (debounce) — Theme 은 `themeStore.setMode()` → localStorage
   - 후속 (나머지 sections):
   - Section 8:
     - **Auth** — token rotate / password change/setup
     - **Theme** (G27) ⭐ — `[System | Light | Dark]` radio
     - **Shortcut** (G26) ⭐ — read-only list, 카테고리별
     - **Terminal** (G35) ⭐ — global default (font/wrap/scrollback/cursor/bell)
     - **Templates** (G36) ⭐ — CRUD (Add/Edit/Delete), default template radio
     - **Storage** — workspace path / file_open allowlist / **Session export/import (G28)** ⭐ ([Export this session] / [Import session])
     - **Behavior** (G25.1) — `auto_kill_terminal_on_panel_close: bool` default false
     - **Debug** — server pid, build sha, log path
6. ✅ **`lib/chrome/ShutdownModal.svelte` (Tier 3, 구 `ServerShutdownConfirmModal` rename)** ⭐ — Cmd+Shift+Q 또는 SessionMenu [Shutdown server…] → confirm → `POST /api/shutdown`
7. ✅ **`lib/ws/client.ts` 통합 WS auto-reconnect (Tier 3)** ⭐ — 별 `reconnect.svelte.ts` 파일 분리 안 함:
   - 1s grace + exp backoff (BACKOFF_MS array) / cap 30s
   - ConnectionState 머신 (`connecting / open / closing / closed / reconnecting`)
   - close code/reason 을 `connectionStore` 로 노출 — banner 가 1008/1011/4001 분기 표시 (attempt 카운터 포함)
   - `lib/ws/heartbeat.svelte.ts` (별 store) 가 frame/activity timestamp + `isStale`/`isIdle` derived 노출 — Phase 2 idle reactivate 의 보조 신호
8. ✅ **Session attach recovery (ADR-0019 D5.1 + D5.4, plan-0008)** ⭐ — Phase 1+2 모두 ship:
   - Phase 1 (Case I, page entry blocking): `reconnectGate.svelte.ts` 8-state 머신 + `ReconnectModal.svelte` 4 mode + `sessionStorageHint.ts` + `+page.svelte` boot screen (`{:else if state ∈ {booting, attaching, hydrating}}`) + auth-gate 의 try/catch + 모든 종료 경로의 `markIdle()` (booting 영구화 방지)
   - Phase 2 (Case II, in-use reactivate silent): `dispatcher.svelte.ts` 의 reconnecting→open trigger + `+page.svelte` 의 visibilitychange listener + `sessionStore.silentReattach(name, signal)` (in-flight singleton) + `ensureMutationOk(message?)` exported helper — 7+ mutation 진입점 일관화 (Canvas / TextNode / PanelNode / PanelDanglingOverlay / LayerTreeView / TerminalListView / zStore)
   - 자세 = plan-0008 §1~§9
9. **Session delete UI (ADR-0019 D10 + D10.1, G51, 2026-05-17 amend)** ✅ **shipped** (2026-05-17) — entry point 2 곳 모두 land:
   - `SessionListModal.svelte` amend ✅ — `Available` row 우측 hover-kebab [⋯] → SessionDeleteConfirmModal. *가시성*: `In use` row + 본 webpage 의 active row (= `sessionStore.active.name` 일치) 는 kebab 표시 X (`canDelete(name)` helper). 승인 후 `deleteSession(name)` → 1s polling (D6.4) 의 다음 tick row 자연 제거 (즉시 refresh 트리거 없음). 404 (race) 는 `deleteSession` 의 silent return 으로 동일 처리.
   - `SessionMenu.svelte` amend ✅ — Logout 아래 / Shutdown server… 위 "Delete current session…" item. *현 attached session 만* 대상 (`sessionStore.active === null` 시 disabled). 승인 후: `deleteSession(activeName)` → `sessionStore.clear()` + `reconnectGate.cancel()` + `sessionStorageHint.clear()` + `workspaceSwitcher.open()` (D5.4 cancel 흐름 + D10 "현 attached 였으면 dialog 회귀" 정합) + success toast.
   - `SessionDeleteConfirmModal.svelte` ✅ 신규 — `PanelCloseConfirmModal` 패턴 정합. Copy = "Delete session '<name>'?" + "Terminals stay running in the server pool" caveat. destructive button = `--color-danger`.
   - BE 변경 0 — `DELETE /api/sessions/<name>` (BE-NEW-2) 이미 ship + `deleteSession()` (`lib/http/sessions.ts:115`) wrapper 기존.
   - 비채택 대안 (Cmd+Click on row / row right-click context menu / LayerTreeView 진입) 사유 = ADR-0019 D10.1.

### Stage 8~10 (P2)
- Stage 8: Image/Document renderer + asset upload (file picker + drop)
- Stage 9: FreeDrawNode + point simplification + backpressure
- Stage 10: Playwright E2E + multi-tab race scenarios + accessibility audit

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

함정:
- `npm run dev` 금지 — BE 가 release binary 로 `dist/` 를 SPA 서빙.
- xyflow / lucide-svelte 1.0.1 의 `$$props` 가 Svelte 5 strict 빌드 충돌 — chrome 아이콘은 인라인 SVG.
- xterm.js WebGL renderer 권장.

---

## 8. BE 의존성 매트릭스 (plan-0007 §15 v3)

| 내 작업 | 필수 BE | v3 신규 ⭐ |
|---|---|---|
| FE-NEW-3 Terminal pool | BE-NEW-10, BE-NEW-12.5 | |
| FE-NEW-6 Multi-xterm | BE-NEW-4, BE-NEW-10 | |
| **Toolbar [Terminal▾] dropdown (G36)** ⭐ | BE-NEW-10 amend (template) | ⭐ |
| PanelDanglingOverlay (G25 c2) | BE-NEW-12.5 (respawn + terminal_died) | |
| FE-6 Layer list V2 | BE-2, BE-NEW-2, BE-NEW-10, BE-NEW-12.5 | |
| FE-7 Panel header V2 | BE-NEW-2, BE-NEW-10, BE-NEW-12.5 | |
| **Panel Settings Terminal Override (G35)** ⭐ | BE-2 amend (`terminal_overrides`) | ⭐ |
| FE-8 Settings UI | BE-1, BE-NEW-7, BE-NEW-12, sessions/import, **shutdown** ⭐, **Templates CRUD** ⭐ | ⭐ |
| FE-NEW-8 file_path open | BE-1, BE-NEW-7, BE-NEW-12 | |
| ShutdownModal (Tier 3) | **`POST /api/shutdown`** ⭐ | ✅ ship |
| WS auto-reconnect (Tier 3) | (없음 — client 만, `lib/ws/client.ts` 통합) | ✅ ship |
| Session attach recovery (Phase 1+2) | (BE 변경 0 — `attach_handler` 가 idempotent. 단 0046 의 same-cookie contract drift 가 별 BE work package — fix 시 silentReattach 의 false-positive `in_use` 분기 사라짐) | ✅ ship (BE 잔여 = 0046) |

**진입 순서**: Stage 4 (Multi-xterm + Pool + DanglingOverlay + **Template dropdown** ⭐) → Stage 5 (Item renderers + file_open + **Resize semantics** ⭐) → Stage 6 (Layer list + Panel header + close modals + **Terminal Override modal** ⭐ + **Duplicate** ⭐ + **z Store** ⭐) → Stage 7 (Viewport + **focusStore** ⭐ + **shortcut** ⭐ + **theme** ⭐ + **Settings** ⭐ + **shutdown** ⭐ + **reconnect** ⭐).

---

## 9. Glossary

| 용어 | 의미 |
|---|---|
| **Webpage** | 1 브라우저 탭 = 1 WS 연결 = 1 session attach. |
| **Session** | workspace 안 named record. FE 의 layout 단위. |
| **Canvas** | 1 session 의 무한 작업 공간 (SvelteFlow). |
| **Canvas Item** | 10 type discriminated union. |
| **Panel** | `type:"terminal"` 인 Canvas Item — xterm 컨테이너. |
| **Group** | 자식 묶음 (트리). **z 없음**. propagation: visibility AND / lock OR. |
| **M (Manipulation Selection)** | 사용자 제어 대상. session-scoped. |
| **I (Input Target)** | 키 입력 라우팅 terminal. 1 session 안 unique. |
| **Streaming State** | (session, panel) 쌍 단위. |
| **Dangling Terminal Reference** | layout terminal id 가 pool alive 와 매칭 X → focus interaction 시 same id fresh spawn (G25 c2). |
| **Mirror** | 1 terminal → N panel. 입출력 공유. |
| **Match-or-spawn** | Session attach 시 id 매칭 또는 same id spawn. |
| **Auto-mount** | trigger session 의 layout 에만 cascade PUT. |
| **Template** ⭐ | Shell spawn preset (G36). `terminal-templates.toml` server-side. |
| **terminal_overrides** ⭐ | Panel item 의 per-panel terminal settings (G35). |
| **Focus mode** ⭐ | M 외 dim 모드 (G34). ephemeral. |

---

## 10. UX 공용 룰 (plan-0007 §14.20 SoT)

### 10.1 Inline edit (G23, §14.20.1)
- Single: Enter commit / Esc cancel / blur commit.
- Multi: Cmd-Enter commit / Enter newline / Esc cancel / blur commit.
- 공용 컴포넌트 `InlineEditField.svelte` / `InlineEditTextarea.svelte`.

### 10.2 Esc 라우팅 (§14.20.2) 7 우선순위
1. Inline edit → cancel
2. Modal stack top → close
3. Panel maximize → unmax
4. Tool locked → 해제 + Select
5. Tool 비-Select → Select 복귀
6. Selection → clear
7. (그 외) no-op

### 10.3 Toolbar 도구 (G22, §14.20.3)
- Default one-shot, 사용 후 Select 복귀.
- Q 단축키 또는 long-press = locked sticky.
- Esc = lock 해제.
- Select/Hand mode always sticky.

### 10.4 Maximize (G20, §14.20.4)
- Canvas viewport area fill (Titlebar/Toolbar/Status bar 유지).
- FE-only ephemeral. 한 시점 1 panel.
- Unmaximize: Esc / 헤더 toggle / panel header double-click.

### 10.5 Keyboard shortcut (G26, §14.20.5)

#### 10.5.1 Layer 우선순위 (Hybrid)
- Modifier shortcut: 어디서든 발동.
- Single-key: xterm focus 외에만.
- **Esc xterm focus = shell ESC byte** (vim 자연).

#### 10.5.2 P0 매트릭스 (Stage 1~6)
| Shortcut | 동작 |
|---|---|
| `Esc` | escRouter 7 우선순위 |
| `Enter` | Inline single-line commit |
| `Cmd/Ctrl+Enter` | Inline multi-line commit |
| `Q` | Tool lock toggle |
| `]` / `[` | Bring forward / Send backward |
| `Shift+]` / `Shift+[` | Bring to front / Send to back |
| `Space hold + drag` | Viewport pan modifier (G29) |
| **`Cmd/Ctrl+D`** ⭐ | **Duplicate (terminal=mirror, G32)** |
| **`Cmd/Ctrl+0`** ⭐ | **Viewport reset 100% (G33)** |
| **`Cmd/Ctrl+1`** ⭐ | **Viewport fit all (G33)** |
| **`Cmd/Ctrl+2`** ⭐ | **Viewport fit selected (G33)** |
| **`Cmd/Ctrl+A`** ⭐ | **Select all** — focus 4 모드: canvas / LayerTreeView / xterm / editable (ADR-0017 D6 amend ⑤, 2026-05-17 신규) |
| **`Cmd/Ctrl+C` / `X` / `V`** ⭐ | **Copy / Cut / Paste** — terminal=Clone (default spawn), (24,24) offset cascade (ADR-0030 D5) |
| **`Cmd/Ctrl+Z` / `Shift+Z`** ⭐ | **Undo / Redo** — active session 단위 stack cap 50 (ADR-0028). editable focus 시 OS-default 로 routing. |

#### 10.5.3 P1 매트릭스 (Stage 7+)
| Shortcut | 동작 |
|---|---|
| `Cmd/Ctrl+N` | New Terminal (default template) |
| `Cmd/Ctrl+Shift+L` | LeftPanel toggle (Layers / Terminals 탭, ADR-0017 ④) |
| `Cmd/Ctrl+Shift+I` | RightPanel toggle (Inspect, ADR-0017 ④) |
| `Cmd/Ctrl+Shift+Q` | Server shutdown (confirm) |
| `Cmd/Ctrl+,` | Settings overlay (ADR-0017 ④) |
| **`Cmd/Ctrl+.`** ⭐ | **Viewport go to selection (G33)** |
| **`Cmd/Ctrl+=` / `Cmd/Ctrl+-`** ⭐ | **Viewport zoom ±10% (G33)** |
| **`Cmd/Ctrl+Shift+F`** ⭐ | **Focus mode toggle (G34)** |

#### 10.5.4 비범위 (P3 + 2026-05-17 ADR-0017 D6 amend ⑤ 합본)
- Customization (rebind UI)
- Chord shortcut (Cmd+K → ...)
- **OS-standard 의도적 제외**:
  - `Cmd/Ctrl+S` — auto-save (G19) 정합, 사용자 액션 불필요
  - `Cmd/Ctrl+P` — Print 비범위
  - `Cmd/Ctrl+W` — Tab close, browser default 우선
  - `Cmd/Ctrl+R` — Reload, Session attach recovery (ADR-0019 D5.1 / D5.4) 자연 처리
  - `Cmd/Ctrl+Tab` — App switch, OS 영역
- **Find / search (`Cmd/Ctrl+F`) — P2 deferred**: 별 ADR 후보, Cmd+K command palette (TBD) 와 분기 검토

#### 10.5.5 Discoverability
- Settings → **Shortcut section** (read-only list, 카테고리별).
- Tooltip 옆 단축키 표시 (`⌘⇧]` Mac / `Ctrl+Shift+]` Win).
- `navigator.platform` 으로 detect.

### 10.6 Resize semantic (G37~G40, §14.20.6) ⭐ **v3 신규**

| Item type | Resize 방법 | 비고 |
|---|---|---|
| terminal | SvelteFlow NodeResizer (이미 구현) + xterm ResizeObserver fit + PANE_RESIZE WS frame | Sprint 7 |
| text, note, rect, ellipse, document, file_path | NodeResizer 표준 frame resize | wrap/payload 영향 |
| **line (G37)** ⭐ | **별 LineNode — 양 endpoint 별 handle (Figma)** | NodeResizer 미사용 |
| **free_draw (G38)** ⭐ | **NodeResizer + 모든 points 비례 scale (Figma)** | drawing 시각 크기 변경 |
| **image (G39)** ⭐ | **NodeResizer + Shift+drag = aspect lock (Figma)** | default 자유 |

**Multi-item resize (G40)** ⭐: M 통합 bounding box corner drag → 모든 items 위치+크기 비례 scale (Figma).

**Group resize**: ADR-0010 D9 = **MVP 미지원** (group 은 frame 없음, pure organization).

**Minimum size constraints**:
- terminal: 120×60 / text,note: 80×40 / rect,ellipse: 20×20 / line: 길이 5 / free_draw: longest axis ≥ 20 / image,document,file_path: 60×40.

**Resize 영속**:
- `onResizeEnd` → `PUT /api/sessions/<name>/layout` (debounce 300ms).
- xterm winsize: 별 WS frame `PANE_RESIZE` (실시간).
- Resize 중: client state only.

### 10.7 Focus mode (G34, §14.20.7) ⭐ **v3 신규**
- M 외 dim (50% opacity). M items 정상.
- Streaming State 영향 X (dim panel 도 Streaming 그대로).
- Click 자유 — dim 된 item click → M 에 추가 → 자동 정상화.
- FE-only ephemeral (attach 마다 fresh).
- Toggle: Statusbar [🎯 Focus mode] + `Cmd+Shift+F` (P1).
- M 변경 시 dim 자동 갱신.
- Visual: statusbar button active 표시 + 선택적 viewport 상단 toast/badge.

---

## 11. 작업 룰

- English code/comments, Korean docs.
- 점진 어휘 통일 `pane` → `Terminal`.
- Svelte 5 runes (`$state` / `$derived` / `$effect`).
- xyflow / SvelteFlow node type registry.
- xterm.js WebGL renderer 권장. Theme adapter (G27).
- 불필요한 추가 금지.

---

## 12. 진입 시 첫 메시지 후보

- "Stage 4 부터" → §6 Stage 4 (Multi-xterm + Pool + DanglingOverlay + **Toolbar Template dropdown**).
- "Layer list V2" → Stage 6 + ADR-0024 + G25 + zStore.
- "Settings overlay 큰 작업" → Stage 7 의 §14.8 8 sections.
- "Resize Line/Free draw 어떻게?" → §10.6 + ADR-0018 D4 payload.
- "Focus mode 모름" → §10.7 + plan-0007 §14.20.7.
- "shortcutRegistry 구조?" → §10.5 + plan-0007 §14.20.5.6 구현 디테일.
- "Refresh / idle 이후 session 어떻게 복구?" → §1 헤더 표 의 *Session attach recovery* + §4.3 의 ✅ Phase 1+2 + §5 공용 컴포넌트 표 의 `reconnectGate` / `ReconnectModal` / `sessionStorageHint` / `heartbeat` + plan-0008 §1 (UI/UX) / §4 (구현 inventory) / §6 (Phase 2 ship) + ADR-0019 D5.1 / D5.4 / 변경 이력 *2026-05-16 (0045 P0 후속)* + 0045 (refresh effect-depth loop 분석) + 0046 (BE attach_handler contract drift work package).

---

## 13. 변경 이력

- 2026-05-15 v1: G18~G25 + multi-session pivot.
- 2026-05-15 v2: G26~G29 + D 검증 amend.
- **2026-05-15 v3**: G32~G40 + Tier 3 + Tier 2 P2 deferred + FE 진행 분 (Stage 1~3 + Stage 5 부분 Toolbar2) 반영.
  - §4 의 진행 분 표시 (✅ done / 🟨 부분 / ❌)
  - §5 FE 명세 진행 상태 표 + ⭐ 항목 (Cmd+D / 4 z 액션 / ViewportCtrl 확장 / focusStore / shortcutRegistry / themeStore / xtermTheme / Resize handlers / Panel Settings Terminal Override / Toolbar Template dropdown / ServerShutdownConfirmModal / reconnect)
  - §6 Stage 4~10 잔여 업무
  - §10.5/.6/.7 신규 (Keyboard shortcut + Resize + Focus mode)
  - §8 의존 매트릭스 v3 ⭐ 컬럼
- **2026-05-16 (v3 + chrome amend 정합)**: ADR-0017 ②/③/④ + ④ follow-up + ADR-0021 D7 amend ② 정합 반영.
  - §2 mental model — Sidebar → LeftPanel (가로 탭) + RightPanel parity (ADR-0017 ②/③), 어휘 재정의 (LayerTreeView / TerminalListView / ItemInfoView)
  - §2 stores — shortcutRegistry/chromeShortcuts (`lib/keyboard/`) + theme.svelte.ts (`mode`/`resolved`) + xtermTheme (`lib/xterm/`) + settingsDialog 추가
  - §4.3 잔여 status — SettingsOverlay 🟨, shortcutRegistry ✅, themeStore + xtermTheme ✅ (ADR-0017 ④)
  - §5 P1 매트릭스 — shortcutRegistry/themeStore/SettingsOverlay 상태 갱신
  - §5 공용 컴포넌트 표 — `lib/keyboard/` 경로 정합, InlineEdit/escRouter/zStore/RightPanel/ItemInfoView/PanelFoldButton/SettingsOverlay/settingsDialog 추가, **회수** entry (Sidebar/PaneInfoPanel rename + RailToggle 폐기)
  - §6 Stage 4 — TerminalListSection (Sidebar 하단) → TerminalListView (LeftPanel Terminals 탭)
  - §6 Stage 6 — Sidebar.svelte amend → LayerTreeView.svelte amend
  - §6 Stage 7 §3/4/5 — shortcutRegistry/themeStore/xtermTheme/SettingsOverlay chrome 부분 ship 표시 + 잔여 분리 (BE wire dependent sections)
  - §10.5.3 P1 매트릭스 — `Cmd+Shift+L` LeftPanel toggle 의미 정합 + `Cmd+Shift+I` (RightPanel) row 추가
  - ADR-0018 G39/G40 (text_align + text_vertical_align) 은 ADR-0018 D4 정본이 SoT — handover 의 별 명세 없음 (renderer 진입 시 ADR-0018 직접 참조)
- **2026-05-17 (Basic editing shortcut matrix — ADR-0017 D6 amend ⑤ 정합)**: §10.5.2 P0 매트릭스 + §10.5.4 비범위 동시 amend.
  - §10.5.2 — 6 row 추가: Cmd/Ctrl+A (Select all, focus 4 모드 분기, 신규) / Cmd/Ctrl+C/X/V (ADR-0030 D5 cross-link) / Cmd/Ctrl+Z·Shift+Z (ADR-0028 cross-link)
  - §10.5.4 — OS-standard 5종 비범위 (Cmd+S 자동저장 / Cmd+P print / Cmd+W tab close / Cmd+R reload — D5.1/D5.4 자연 처리 / Cmd+Tab app switch) + Cmd+F 의 P2 deferred (별 ADR 후보 / Cmd+K palette 분기)
  - 짝: ADR-0017 amend ⑦ + 변경 이력 / plan-0007 §14.20.5.2 + .4 + §18
- **2026-05-17 (Style/pattern/rotation 잔여 등록 — ADR-0018 D2/D4 amend pending)**: 3 보완 기능 확장 후보 정합 (a) text 풀-style: font_family / font_weight / font_style / text_decoration / line_height, (b) figure stroke pattern (stroke_dash) + fill pattern (fill_pattern: solid/none/hatch), (c) item rotation (ItemCommon 의 rotation 필드 + rotate grip + 15° snap + bbox/Multi-item resize G40 정합). 본 entry 는 register 만 — ADR-0018 D2 (ItemCommon) + D4 (text / rect / ellipse / line payload) schema row 갱신 + BE serde + openapi 재발행 + renderer transform amend 는 *별 batch* 로 land.
  - §5 P1 매트릭스 — 신규 3 row (`Text style 풀` / `Figure stroke·fill 패턴` / `Item rotation`) 모두 Stage 5 (Item rotation 은 5~6)
  - §6 Stage 5 §7~§9 신규 — 각 기능의 payload 후보 + renderer 매핑 + Inspector 통합 + cross-cut 영향 (rotation 의 BBox / Multi-item resize G40 / Alignment / Layer tree 정합)
  - smoke-7c / 7d / 7e 신규 (land 시점에 활성화)
  - plan-0007 §14.4 FE-4 body amend 짝 (확장 후보 명시)
  - ADR-0018 변경 이력 entry 짝 (register 만)
- **2026-05-17 (Session delete UI — ADR-0019 D10.1 amend 정합)**: D10.1 신규 (UI entry points: SessionListModal `Available` row hover-kebab + SessionMenu "Delete current session…") 반영.
  - §5 P1 매트릭스 — 신규 row `Session delete UI` (Stage 7, ❌, ADR-0019 D10 + D10.1)
  - §6 Stage 7 §9 신규 — SessionListModal/SessionMenu amend spec + 가시성 규칙 (In use + active row 차단) + 승인 후 flow + confirm copy + BE 변경 0 (DELETE /api/sessions/<name> + lib/http/sessions.ts:115 wrapper 기존) + 비채택 대안 cross-link
  - plan-0007 §14.12 FE-NEW-1 body amend 짝 (별 FE-NEW-9 신설 X — D10.1 은 FE-NEW-1 의 자연 확장으로 흡수)
- **2026-05-16 (Session attach recovery — Phase 1+2 ship 정합)**: ADR-0019 D5.1 + D5.4 + 변경 이력 *2026-05-16 (0045 P0 후속)* + plan-0008 §1~§9 정본 반영.
  - §1 헤더 표 (핵심 신규 결정) — `Server shutdown` ✅ `ShutdownModal` rename / `WS reconnect backoff` ✅ `lib/ws/client.ts` 통합 / **신규 row: `Session attach recovery — Case I + Case II`** ✅ Phase 1+2 ship (plan-0008 link)
  - §2 mental model modal list — `ServerShutdownConfirmModal` → `ShutdownModal` rename + `ReconnectModal` (4 mode) 추가
  - §2 stores — `reconnect (Tier 3)` → 분해: `WS auto-reconnect (client.ts 통합)` + `heartbeat (lib/ws/heartbeat.svelte.ts)` + `reconnectGate (8-state)` + `sessionStorageHint`
  - §4.3 잔여 — ❌ `ServerShutdownConfirmModal` / `WS reconnect backoff` → ✅ rename + 통합 표기. **신규**: `Session attach recovery (Case I + Case II)` ✅ Phase 1+2 ship
  - §5 P1 매트릭스 — `ShutdownModal` ✅ / `WS auto-reconnect` ✅ / **신규 row**: `Session attach recovery` ✅
  - §5 P0 매트릭스 — FE-NEW-2 의 `🟨 부분 (heartbeat client 잔여)` → ✅ done (heartbeat + visibilitychange + silent reattach trigger 모두 ship)
  - §5 공용 컴포넌트 표 — ~~`lib/ws/reconnect.svelte.ts`~~ → `lib/ws/client.ts` 통합 표기 + 신규 5 entry (`heartbeat.svelte.ts` / `reconnectGate.svelte.ts` / `sessionStorageHint.ts` / `ReconnectModal.svelte` / `ShutdownModal.svelte`) + `sessionStore.{attemptReattach, silentReattach, ensureMutationOk}` 명시
  - §6 Stage 7 §6/§7 — `ServerShutdownConfirmModal` ✅ `ShutdownModal` rename / `reconnect.svelte.ts` 별 파일 ❌ → `client.ts` 통합 ✅ + heartbeat 별도 store / **신규 §8** Session attach recovery 의 Phase 1+2 spec
  - §8 BE 의존성 매트릭스 — `ShutdownModal` ✅ ship / `WS auto-reconnect` ✅ ship / **신규 row**: `Session attach recovery (Phase 1+2)` BE 변경 0 + 0046 same-cookie contract drift work package cross-link
  - §12 진입 시 첫 메시지 후보 — "Refresh / idle 이후 session 어떻게 복구?" 추가 (plan-0008 / ADR-0019 D5.x / 0045 / 0046 cross-link)
