# 2026-05-20 — FE 컨텍스트 / 오류 history / 사용자 요구 align 리포트

- 작성일: 2026-05-20
- 작성 주체: agent (system-architect role, cold-pickup 후 회고 정리)
- 범위: gtmux frontend (Svelte 5 + SvelteFlow + xterm v6) 의 최근 1~2개월 land 영역
- 정본 cross-link:
  - 직전 handover: `docs/reports/2026-05-20-session-handover-0080-asset-upload-and-phase1-recap.md`
  - 상위 audit: `docs/reports/0071-session-terminal-panel-lifecycle-audit.md`
  - FE perf/logic review: `docs/reports/0065-frontend-performance-and-logic-review.md`
  - Theme buffer resolution: `desktop/projects/gtmux/docs/reports/0063-xterm-theme-buffer-preservation-resolution.md`
  - Attach confirm cancel: `docs/reports/0069-session-attach-confirm-cancel-recovery.md`
  - Webpage owner active 회귀: `docs/reports/0070-webpage-owner-session-list-regression.md`
  - terminal pool / attach_index desync: `docs/reports/0077-terminal-pool-attach-index-desync-resolution.md`
  - FE handover 묶음: `docs/reports/0073-fe-handover-from-0071-audit.md` / `0079-fe-handover-connector.md`

---

## 0. 한눈 요약

| 축 | 한 줄 결론 |
|---|---|
| Phase | **Stage 7+** — multi-session pivot 완료. canvas tool 확장 (image/document/file_path/free_draw/connector) + file picker MVP + reattach 회귀 fix + 0065 6 finding 전수 land + 0071 audit cluster (B/C/D 영역) land 완료. **다음 P0 = 0080 asset upload BE endpoint**, 그 후 connector FE (0079) batch. |
| FE 영역 |  canvas (PanelNode/XtermHost/ShapeNode/LineNode/FreeDrawNode/NoteNode/ImageNode/DocumentNode/FilePathNode/Connector), chrome (AttachConfirmModal/WorkspaceSwitcher/SessionListModal/SessionMenu/SettingsOverlay/MaximizedItemModal/Toolbar2/LeftPanel/RightPanel/InspectView), stores (sessionStore/terminalPool/reconnectGate/workspaceSwitcher/zStore/clipboardStore/filePicker), ws (dispatcher/decode), keyboard (chromeShortcuts/clipboardShortcuts/editingShortcuts), session/lifecycle (webpageId/serverId/leaveBeacon) 등 광범위. |
| 오류 패턴 | (1) Svelte 5 effect dependency 오인 lifecycle, (2) FE↔BE wire contract drift, (3) silent absorption (응답 일부만 사용), (4) optimistic update 의 rollback 부재, (5) 자료구조 O(N²)/O(k²) hot path, (6) listener / timer leak, (7) naming debt (owner_key 가 `cookie` 변수명), (8) release binary stale. |
| 사용자 요구 misalign | (a) "active 의미 = picker 선택 불가" 를 owner-relative 로 잘못 재해석, (b) AttachConfirm Cancel 시 405 + lock 잔존, (c) no-session 에서 chrome 모두 활성화돼 혼란, (d) Figma-signature dashed focus ring 잔존, (e) theme 변경 시 terminal contents 빈 화면, (f) panel minimize 시 xterm buffer 손실 (현재 진행 unstaged 영역). |

---

## 1. 프로젝트·FE 컨텍스트 요약

### 1.1 정체

- **gtmux** — PTY 직접 (ADR-0013, `crates/pty-backend/`) + infinite web canvas 의 single-user 웹 앱.
- 두 state 분리는 invariant: **PTY/session/terminal state** = BE 진실(mirror), **panel 배치·visibility·minimize·lock·z·focus·viewport** = FE 진실.
- FE 정합 hard rule: (1) `applyMutation` 단일 entry (ADR-0028 D11), (2) `priorSnapshot` 전달 시 PUT 실패 자동 rollback (ADR-0028 **D11.1, 본 batch 신규**), (3) path picker-only (ADR-0035), (4) reattach 의 `unmatched > 0` silent 흡수 금지 → `confirm_required` escalate, (5) no-session UI gating, (6) tentative attach 의 Cancel 은 명시 detach 호출, (7) tmux/control-mode 어휘 폐기 — PTY 직접.

### 1.2 영역별 코드 anchor (현재 main HEAD = `eacccb5`)

| 영역 | 주요 file |
|---|---|
| Canvas core | `lib/canvas/Canvas.svelte`, `nodeAdapter.ts`, `itemFactory.ts` |
| Node 렌더러 | `PanelNode.svelte`, `XtermHost.svelte`, `ShapeNode.svelte`, `TextNode.svelte`, `NoteNode.svelte`, `LineNode.svelte`, `FreeDrawNode.svelte`, `ImageNode.svelte`, `DocumentNode.svelte`, `FilePathNode.svelte` |
| Chrome | `chrome/AttachConfirmModal.svelte`, `WorkspaceSwitcher.svelte`, `SessionListModal.svelte`, `SessionMenu.svelte`, `RightPanel.svelte`, `LeftPanel.svelte`, `Titlebar.svelte`, `MaximizedItemModal.svelte`, `ItemInfoView.svelte`, `SettingsOverlay.svelte` |
| Toolbar/Sidebar | `toolbar/Toolbar2.svelte`, `sidebar/LayerTreeView.svelte`, `sidebar/TerminalListView.svelte` |
| Stores | `stores/sessionStore.svelte.ts`, `terminalPool.svelte.ts`, `reconnectGate.svelte.ts`, `workspaceSwitcher.svelte.ts`, `zStore.svelte.ts`, `clipboardStore.svelte.ts`, `filePicker.svelte.ts`, `themeStore.svelte.ts` |
| Network | `ws/dispatcher.svelte.ts`, `ws/decode.ts`, `http/sessions.ts`, `http/terminals.ts`, `http/assets.ts` |
| Session/lifecycle | `session/webpageId.ts`, `session/serverId.ts`, `lifecycle/leaveBeacon.ts` |
| Keyboard | `keyboard/chromeShortcuts.svelte.ts`, `keyboard/clipboardShortcuts.svelte.ts`, `keyboard/editingShortcuts.svelte.ts` |

---

## 2. 발생한 주요 오류와 fix 패턴

각 항목은 **(증상) → (root cause) → (fix) → (관련 commit/문서)** 구조.

### 2.1 Theme 변경 시 terminal cell blank (lifecycle 오류)

- **증상**: Settings → theme(light/dark/system) 변경 시 chrome 은 새 theme 으로 즉시 반영되지만 terminal panel 의 이전 output 영역이 빈 화면처럼 보임. page refresh 만 해결.
- **잘못된 1차 가설**: xterm v6 DOM renderer 의 cell stale 색 — `clearTextureAtlas` / `refresh` / cell span inline reset / `{#key themeStore.resolved}` remount / silentReattach $effect / SettingsOverlay reload 등 7 시도 **전부 실패** (시도들은 `0062-theme-hot-reload-investigation.md` 표).
- **실제 root cause**: `XtermHost.svelte` 의 mount `$effect` 가 `new Terminal({ theme: xtermTheme(themeStore.resolved) })` 안에서 `themeStore.resolved` 를 *직접 read* → Svelte 5 가 그 read 를 effect dependency 로 추적 → theme 변경 시 mount effect 재실행 → 이전 Terminal `dispose()` + 새 인스턴스 생성 → client-side terminal buffer 손실. WS 는 유지되므로 BE replay 도 안 일어남.
- **fix**:
  1. mount effect 의 초기 theme read 를 `untrack(() => themeStore.resolved)` 로 감싸 dependency 제외.
  2. 별도 theme `$effect` 에서 `term.options.theme = { ...xtermTheme(resolved) }` + `term.refresh(0, rows-1)` 로 live 인스턴스에 적용.
  3. `SettingsOverlay.setMode()` 는 reload 없이 `themeStore.setMode(mode)` 만 (ADR-0017 D2 auto-save 정책 보존).
- **관련**: `0063-xterm-theme-buffer-preservation-resolution.md`, commit `e006962`.
- **재발 방지 원칙** (보고서 §8): widget identity 를 바꾸는 입력만 mount effect 에. presentation state (theme/font/색) 는 *별 effect* 에서 *live 인스턴스* 에 적용. backend ring replay 는 attach catch-up 전용이지 FE remount 보정용 아님.

### 2.2 Server 종료 후 새로고침 시 respawn dialog 누락 (silent absorption)

- **증상**: canvas 작업 중 BE Ctrl+C → 재기동 → 새로고침 → cookie 인증 → 곧장 canvas 진입. 그러나 terminal 들이 다 죽은 상태라 panel 만 남고 respawn dialog 가 안 뜸. 정상 entry (`SessionListModal` 통과) 흐름에서는 dialog 가 뜬다.
- **root cause**:
  - BE `classify_layout_terminals` 는 process 재기동 후 `terminal_map` 이 비었으므로 layout 의 UUID 를 모두 `unmatched` 로 분류 → 200 + `unmatched=[전체]`.
  - 비교 대상 (`WorkspaceSwitcher.tryAttach`) 은 `unmatched > 0` 시 `confirm_required` 분기로 escalate.
  - 그러나 hint-based path (`sessionStore.attemptReattach`) 는 `await attachRes.json()` 으로 **drain 만** — `matched/unmatched` 무시. 주석은 plan-0008 §8 위험 row 인용하나 실제 plan-0008 §8 에 해당 결정 근거 없음 (design 오판).
- **fix** (`9bd2eea`):
  1. `ReattachResult` 에 `{kind:'confirm_required', summary}` 추가.
  2. `attemptReattach` 의 200 path 가 `attachBody.unmatched.length > 0` 이면 layout fetch 건너뛰고 즉시 `confirm_required` 반환.
  3. `reconnectGate.#run` 의 switch 에 `case 'confirm_required'` — `setActiveSession({name})` + `markIdle()` + `workspaceSwitcher.goAttachConfirm(name, summary)`. canvas 는 빈 채 mount 되고 modal 이 즉시 덮음.
  4. `+page.svelte::maybeSilentReattach` 의 Phase 2 silent path 도 같은 분기 추가.
- **신규 invariant**: *reattach 의 `unmatched > 0` 은 silent 흡수 금지, `confirm_required` 로 escalate*. 향후 새 reattach call site 추가 시 동일 패턴 강제.

### 2.3 AttachConfirm Cancel 시 405 + lock 잔존 (FE↔BE wire contract drift)

- **증상**: no-session 상태에서 SessionListModal → 임의 session 선택 → AttachConfirmModal → **Cancel** 클릭 → toast `Attach cancelled, but previous session could not be restored: POST detach returned 405` + 선택했던 session 이 list 에서 in-use 로 보임.
- **root cause** (`0069`):
  - (a) `WorkspaceSwitcher.tryAttach` 가 `confirm_required` 시점에 `sessionStore.setActiveSession({name})` 을 *먼저* 호출 — BE lock 은 잡혔지만 spawn 미승인 + layout 미load 중간 상태를 active 로 올림 → `AuthDialog` dismissable 조건과 충돌.
  - (b) FE `detachSession()` 이 `POST /api/sessions/:name/detach` 호출 — 실제 BE 계약은 `DELETE /api/sessions/:name/attach` → 405 Method Not Allowed.
- **fix** (`0069` §3):
  1. `confirm_required` 분기에서 `sessionStore.active` 는 변경 안 함. 별도 `pendingAttachPreviousSession` / `pendingAttachHasTentativeLock` 만 보관.
  2. Confirm 성공 시 `attachConfirm → getLayout → setActiveSession → loadLayout → close()` 순서로 active 전환.
  3. `cancelAttachConfirm()` 5-step chain: tentative `detachSession(pending)` → 이전 active 가 있으면 재attach → 없으면 `sessionStore.clear()` → `workspaceSwitcher.goList()` → pending state clear.
  4. `detachSession()` 을 `DELETE /api/sessions/:name/attach` 로 정정. response body 미사용이라 2xx 시 `{kind:'ok'}` normalize.

### 2.4 `reconnectGate.cancel()` 의 tentative attach BE lock 누락 (0071 §B-1)

- **증상**: ReconnectModal 의 `[Switch session…]` 클릭 시 in-flight `POST /attach` abort 만 발생 — BE 측 flock + `session_locks_by_owner` 잔존. **다른 webpage_id 의 새 탭** 으로 같은 session attach 시도 시 30s heartbeat timeout 까지 409 conflict.
- **fix** (FE-A, `df05425`):

```ts
cancel(): void {
  this.#controller?.abort();
  this.#controller = null;
  const wasAttaching = this.state === 'attaching' && this.attemptName !== null;
  const tentativeName = this.attemptName;
  this.markIdle();
  sessionStorageHint.clear();
  if (wasAttaching && tentativeName !== null) {
    void detachSession(tentativeName).catch((err) => {
      console.debug('[gtmux] reconnectGate.cancel: tentative detach failed', err);
    });
  }
}
```

- 결정: `fire-and-forget` (UI 즉시 modal 전환 보장) + 실패 silent (toast 없음, 30s heartbeat fallback). `attemptName` 캡처 후 `markIdle()` — markIdle 이 attemptName 을 null 로 reset 하므로.
- **ADR**: ADR-0019 D5.4 amend ②.

### 2.5 mount_cascade wire 의 session-switch race (0072 §1)

- **증상**: server-side `hub.session_for_owner(K)` 필터만으로는 frame 비행 중 owner 가 session switch 하면 부족 — 이전 session 의 mount_cascade 가 새 session 에 적용 위험.
- **fix** (`abc5931`):
  1. BE encoder 가 envelope 에 `trigger_session` 동봉.
  2. FE `dispatcher.handleMountCascade` 가 `triggerSession !== sessionStore.active?.name` 이면 drop.
  3. 옛 FE / 옛 BE 모두 fail-safe (decode null → drop).

### 2.6 Terminal pool ↔ attach_index desync — kill 시 다른 session mirror 손상 위험 (0077)

- **증상**: 여러 webpage 가 열린 상태에서 BE 종료 → 재인증 → 진입 시 (1) TerminalListView 가 canvas 에 panel 이 있는데도 `(!) desync` badge, (2) 같은 row 가 ALL 모드만 보이고 THIS 에선 hide, (3) fresh BE start + 기존 terminal attach 시 모든 row desync, (4) desync session 에서는 terminal 제거/생성 막힘.
- **잘못된 가설**: H1~H5 (applyMutation ghost item, attach_index.apply_diff erase, layout_put_handler hook 누락, create_terminal_handler race) 모두 검증 후 reject.
- **실제 root cause** (2 layer):
  1. **Binary stale** — release binary mtime 가 commit `8814b06` (D5.6 owner_key) 시점에 고정, 그 이후 land 한 commit (`72278b1`, `abc5931`, `a1ecdb3`, `5ea3dc3`, `8cd925a`, `c63be0c`, `a276058`) **모두 binary 에 없음**. 사용자 시연 환경의 BE 가 stale.
  2. **(latent)** boot rebuild miss / schema drift / 미보고 race 가 영속 가능 — *어디서 깨졌는지 진단 비용* 이 큼.
- **fix series (3 layer)**:
  - **FE 즉시 보호** (`72278b1`/`605d8d8`/`5ea3dc3`/`a1ecdb3`): `killOne` 의 `isOnCurrentCanvas` defensive guard (현 session panel kill 차단), `(!) desync` badge + auto refresh, `unplaced`→`pool only` 3-state, Mine/All→ segmented THIS/ALL, THIS filter union 에 `sessionStore.items.has(t.id)` 추가, badge `×N` + tooltip session 이름 list, `Panel+Terminal` mirror 시 button disable + 이유 명시.
  - **BE 진단 가시성** (`8cd925a`/`c63be0c`): attach_index 4 mutation site tracing + `rebuild_from_disk` per-session log + `sessions_skipped>0` 시 WARN.
  - **BE self-heal** (`a276058`): `classify_layout_terminals` + `attach_confirm_handler` 의 200 응답 직전에 `attach_index.apply_full_session(name, &load_terminal_uuids(...))` 호출. set semantics 라 정상이면 비용 0, miss 였으면 자동 회복. ADR-0021 D7 amend ④.
- **교훈** (§5.1): 모든 BE 변경 commit 후 `cargo build --release` + binary mtime 확인 *반드시*. handover doc 의 verification 절차에 mtime check 추가.

### 2.7 0065 Frontend perf/logic review 의 6 finding (방어/성능)

| Finding | 증상 | fix | Commit |
|---|---|---|---|
| **FE-1 free_draw 입력 누적 비용** | pointer move 마다 points 배열 spread 복사 + bbox 전체 순회 → 긴 stroke 의 입력 지연 | (1) 비반응 `let` array buffer (`.push(...)`) + DragState 분리, (2) `freeDrawFrame` $state + `requestAnimationFrame` coalesce, (3) `FREE_DRAW_MIN_POINT_DELTA_SQ=0.25` 거리 기반 prune. 저장 cap 5000 (ADR-0018 D4) 그대로. | `d55f372` |
| **FE-2 drag commit 실패 시 회귀** | `Canvas.svelte` drag stop 이 store 를 optimistic 갱신 후 `applyMutation` fire-and-forget → 실패 시 toast 만, 위치는 reflesh 까지 회귀처럼 보임 | `applyMutation` 에 `priorSnapshot` 옵션 — failure path 에서 `loadLayout(priorSnapshot)` 자동 호출. failMessage "Drag commit failed — reverted to previous position." | `f564ce8` |
| **FE-3 terminalPool.byId O(N)** | array `.find()` — panel × terminal 반복 | `terminalsById: SvelteMap<string,TerminalInfo>` 신규. `refresh()` 가 array + map 동시 갱신 | `c65f4fb` |
| **FE-4 viewport debounce 와 session switch race** | 500ms timer 살아있는 동안 session switch → flush 시점의 active 와 viewport 가 cross-session 으로 저장 가능 | timer 예약 시 `{sessionName, viewport}` snapshot 캡처. flush 가 active 와 sessionName 비교 후 mismatch 시 폐기. `clear()` 가 pending timer 취소 | `d5ed810` |
| **FE-5 PANE_OUT late-buffer O(k²) + hot-path log** | trimming 의 while 안 `reduce` 반복 + `console.debug` 매 PANE_OUT | `LateBufferEntry={chunks,total}` 로 running total → drop loop O(k). hot-path 5 console.debug 를 `DEBUG_PANE_OUT = import.meta.env.DEV` gate → prod bundle DCE 검증 | `215c3c8` |
| **FE-6 LineNode endpoint drag listener leak** | drag 중 unmount (session switch / item delete / layout reload) 시 window listener 안 제거 | `onDestroy(removeWindowListeners)` 추가 + state reset | `4415f76` |

**신규 invariant — ADR-0028 D11.1**: caller 가 `applyMutation` 에 `priorSnapshot` 전달 = "optimistic update 했음" signal. PUT 실패 시 `loadLayout(priorSnapshot)` 자동 호출. failMessage 는 *실제 상태 변화* 명시. 후보 latent 영역 (zStore.#commit / PanelNode onResizeEnd) 명시 후 별 sprint.

### 2.8 Dashed focus ring 시각 정리 (UX 명시 요구)

- 사용자 verbatim: *"component 들 중 버튼의 테두리에 파란색 dashed line 효과가 있는게 있나? 이 효과 자체를 제거해줘."*
- **fix** (`bde370e`): `styles/global.css` 전역 `:focus-visible` block (outline:2px dashed accent + offset 1px) 제거 + 14 component 의 동일 패턴 19 rule 일괄 제거 — 총 80줄 deletion, CSS bundle 1.8KB 감소.
- **유지** (의도된 dashed visual): `LayerTreeView .row.drop-inside` (그룹 안 indicator), `Canvas.point-spawn-ghost` (도구 미리보기), `ImageNode` placeholder gray, `ImportSessionModal .file-pick` drop zone, `PanelNode` 의 *주석만 dashed, code는 outline:none*.
- **A11y note**: 전역 `:focus-visible` 제거로 keyboard focus 시 *브라우저 default outline* 적용. 완전한 손실은 아니나 추후 focus style 별 패치로 추가 권장.

### 2.9 No-session UI gating (사용자 명시 요구)

- 사용자 verbatim: *"no session page 에서는 menu button, session state button(toolbar 왼쪽)을 제외한 component 들 (toolbar, 좌/우 패널)은 비활성화 되도록 — 사용자가 session 을 연결하도록 유도하며 로직 충돌을 방지하기 위해. & menu button 에서도 Session shutdown / Delete current session / Export session 도 비활성화."*
- **fix** (`f086e32` + `9e0d59c`/`f086e32`): `sessionStore.active === null` 시 Toolbar 12 도구 + LeftPanel/RightPanel tab + body + SessionMenu 의 Shutdown/Export/Delete + chromeShortcuts `Cmd+N` / `Cmd+Shift+Q` 모두 disabled. body 는 `inert + opacity 0.4 + pointer-events:none` 3중. *유지*: ActiveSessionDropdown (`No session` placeholder + 클릭으로 SessionListModal), Titlebar 의 SessionMenu kebab, fold/expand, resize handle.
- **신규 invariant** — 새 도구/shortcut 추가 시 같은 정책 적용. Settings(`Cmd+,`) / LeftPanel toggle(`Cmd+Shift+L`) / RightPanel toggle(`Cmd+Shift+I`) 는 chrome 정리용이라 no-session 에서도 active 유지.

### 2.10 WebpageId 적용 후 `active` 의미 회귀 (요구 misalign → 정정)

- **misalign**: 직전 batch 가 attach owner 를 `auth_cookie + 0x1f + webpage_id` (owner_key) 로 통일하면서 `GET /api/sessions` 의 `active` 까지 owner-relative conflict flag 로 잘못 재해석.
  - 잘못된 의미: 현 webpage 보유 lock → `active:false`, 다른 webpage 보유 lock → `active:true`. 결과: "현재 webpage 의 자기 session 은 picker 에서 선택 가능" 가능성.
- **사용자 요구 (재정의)**: *웹페이지에 열려 있는 모든 session 은 선택 불가*. owner 와 무관하게 lock 이 있으면 picker disabled.
- **fix** (`0070` §3):
  - `list_handler` 를 raw lock 판정으로 복원. Vacant/Stale→false, InUse/InUseRaceyBody→true. owner 무관.
  - owner-scoped 분리는 `POST/DELETE /attach`, layout-changing mutation, WS routing 에만 적용.
  - 회귀 테스트 `session_list_disables_any_open_webpage_session` 추가 — page-a / page-b 모두 `alpha.active == true` 확인.
- **ADR-0019 D5.6 보완**: `active` 는 "어느 webpage 에서든 이미 열려 있어 picker 에서 선택 불가" UI-facing flag 로 명시. *후속*: `open` / `selectable` 같은 의미 명확 필드로 분리 가능.

### 2.11 0074 Server boot identity (stale tab race)

- **증상**: BE process 재시작 후 같은 cookie 의 이전 탭이 stale state 로 살아있어 신/구 server 의 lock/state 혼동 가능.
- **fix Phase 1** (`2911c2c`) — *FE detection only*: BE 가 `GET /api/sessions` 응답에 `X-Gtmux-Server-Id` header 동봉. FE `lib/session/serverId.ts` 가 첫 응답 server_id 저장, 이후 mismatch 발견 시 `sessionStore.clear()` + `reconnectGate.cancel()` + `workspaceSwitcher.open()` + warning toast. *onMount 첫 라인에 mismatch handler 등록 필수* (listSessions 호출 전).
- **거절된 옵션**: token 을 webpage 별 쪼개기 (auth domain 책임 오염).
- **Phase 2/3 (BE boot capability)** = 큰 design change, 별 cycle 로 분리 (현재 deferred).

### 2.12 leaveBeacon — page unload 시 즉시 lock release

- **fix** (FE-C, `4bcf810`/`72a16e4`): `lib/lifecycle/leaveBeacon.ts` 신규 — `beforeunload` + `pagehide` 양쪽 listen → `navigator.sendBeacon('/api/leave?webpage_id=...', Blob([]))`. `+page.svelte` 의 onMount/onDestroy 에 bind/unbind. webpage_id 는 URL query (sendBeacon 의 custom header 제한 우회). BE 가 idempotent 라 logout 흐름과 race 안전.
- **anti-pattern 차단**: fetch keepalive (브라우저 별 지원 차이), `event.preventDefault()` 로 prompt 띄움 (의도 외), pagehide 누락 (iOS Safari miss), `lib/session/` 위치 (책임 분리 위반 — `lib/lifecycle/` 위치 필수).

### 2.13 AttachConfirmModal copy 의 history 손실 경고 (UX 명시)

- 0071 §B-2(a): "Will spawn N new terminal(s)" copy 가 *fresh process 라 history 없음* 사실 미고지 → mental model 어긋남.
- **fix** (FE-B, `d21f9e6`): 영문 한 줄 추가 — `Note: New terminals start fresh — previous output cannot be restored.` 중립 "Note:" (공포 문구 거절). 시각 표현은 design 자율.

### 2.14 svelte-flow virtualization off + late buffer cap

- `4795c08`: svelte-flow 의 virtualization 가 panel scroll 시 unmount/remount 유발 → xterm dispose. virtualization OFF + late buffer cap 4 MiB 로 일시 정합. **revert** (`70640c3`): BE RING_CAPACITY 128 KiB 와 정합 위해 late-buffer cap 256 KiB 로 되돌림.

### 2.15 [현재 working tree, unstaged] Panel minimize 시 xterm buffer 손실

- **증상 (직접 관찰)**: PanelNode 의 minimize 상태에서 xterm 인스턴스가 unmount 됨 (`isStreaming = isVisible && data.minimized !== true` 조건). restore 시 새 인스턴스 mount → 빈 buffer → 새 output 도착 전까지 빈 화면.
- **현재 unstaged fix** (`PanelNode.svelte` + `XtermHost.svelte`):
  1. `PanelNode.isStreaming` → `shouldMountTerminal = $derived(isVisible)` 로 변경 — minimize 와 무관, visibility 만 mount gate.
  2. XtermHost 의 D16 design 주석을 amend: visibility=false 면 unmount, **minimized=true 는 인스턴스 유지하고 chrome 만 접음**.
  3. ResizeObserver 가 minimize 시 0x0 측정을 무시하도록 — `w<=0 || h<=0` 시 early return + `lastObservedW/H = -1` 로 reset 해 restore 후 첫 visible 측정에서 강제 refit.
  4. resize fit 후 `term.refresh(0, rows-1)` 호출 — viewport repaint 보강.
- **이유**: 2.1 의 *xterm buffer 보존 원칙* 동형. minimize 는 *visual chrome* 일 뿐 widget identity 가 아니므로 dispose/recreate 금지.
- **commit 미발생** — 본 unstaged 영역은 별 commit + ADR-0021 D16 amend 짝 권장 (handover §6 의 "panel chrome state ≠ widget lifecycle" invariant 신규).
- **회귀 검증 권장**: (a) minimize 직후 PANE_OUT 도착 → restore 시 새 output 까지 누적 표시, (b) maximize→minimize→restore 의 in-flow geom override 패턴 동작 확인, (c) 빈 buffer 0x0 ResizeObserver 측정이 fit 호출 안 함 확인.

### 2.16 그 외 minor regression chain

- `panel refresh button` revert (`89d7ba4`) — history 손실 회귀. 사용자 명시 거절: *re-render 만으로 회복 불가, ring replay 가 BE 측 책임*.
- `selection ring` 시리즈 (`d304f3f`/`bbed597`/`74a748f`/`25562ef`/`7659bf3`/`320a13a`/`b6f3335`) — box-shadow→outline 변경 → header 가림 → header tint 제거 → minimize strip 32→34 → border-color accent → revert → re-apply. 결국 *minimize+selected 시각 통일* (PanelNode + NoteNode).
- `dropdown` 의 menu item 1-line wrap 차단 (`f34ae64`).
- `inspector minimize 정합` (`ddabda7`).
- `pre-session offline UI + app/panel refresh buttons` (`8a897d5`) → 후속 `panel refresh` 만 revert.
- `terminal badge` 정리: `here/here+N` → `×N` main + `+M` superscript (`bd04e43`), `Panel+Terminal` mirror guard (`605d8d8`), THIS filter 가 FE local panel 보호 (`5ea3dc3`).
- `kill defensive guard + desync detection` (`72278b1`).
- `auth topbar + theme toggle 제거` (`8f8e432`).
- `align-btn 아이콘 + tool guide stroke` (`7034051`).
- `5 minor UX regressions` (`95b10f9`) — 묶음.

---

## 3. 사용자 요구 misalignment → align 정리

| # | 사용자 요구 (verbatim 또는 의도) | 잘못 구현된 1차 상태 | Align fix |
|---|---|---|---|
| **R1** | server 종료 후 새로고침 시 *반드시* session 선택 modal 또는 respawn dialog | hint-based reattach 가 silent attach 후 빈 panel 만 노출 | `confirm_required` escalate (2.2) |
| **R2** | AttachConfirm Cancel 은 *원상 복구* (선택했던 session 안 잡힘) | 405 + lock 잔존 + active 회귀 noise | `cancelAttachConfirm` 5-step + `DELETE /attach` 정정 (2.3) |
| **R3** | session picker 는 *어느 webpage 에서든 열린 session* 선택 불가 | owner-relative 로 *자기 session 선택 가능* 가능성 | `list_handler` raw-lock 복원 + ADR-0019 D5.6 보완 (2.10) |
| **R4** | no-session 에서 chrome 비활성화 — *session 연결 유도 + 로직 충돌 방지* | Toolbar/좌우 panel 모두 활성화, Shutdown 도 enabled | `sessionStore.active===null` gating (2.9) |
| **R5** | 파란 dashed focus ring 효과 자체 제거 | 19 rule 전역 + 14 component | global + 14 component 일괄 제거, 의도 dashed 5곳만 유지 (2.8) |
| **R6** | theme 변경 시 terminal contents 보존 | xterm 인스턴스 재생성 → buffer loss | mount/theme effect 분리 + `untrack` (2.1) |
| **R7** | terminal pool ↔ canvas panel 의 *정합 신뢰* (kill 가 다른 session mirror 손상 X) | desync 시 ALL/THIS 표시 비대칭 + kill 가능 위험 | FE defense + BE self-heal (2.6) |
| **R8** | `[Switch session…]` 클릭 시 *즉시* SessionListModal | abort + idle 만, BE lock 30s 잔존 | `reconnectGate.cancel` 의 fire-and-forget detach (2.4) |
| **R9** | webpage_id 적용 후 *attach 충돌 의미* 명확화 | 같은 cookie 다른 탭의 의미 모호 | owner_key = `auth_cookie + 0x1f + webpage_id` 통일, 명명까지 강제 (BE 측, `8814b06` + ADR-0019 D5.6 amend ②) |
| **R10** | 탭 close 시 *즉시* lock release | `DELETE /attach` 의 명시 호출만, 탭 close 는 30s 대기 | `leaveBeacon` (2.12) |
| **R11** | server boot identity 감지 (stale tab race 차단) | server 재시작 후 stale tab 미감지 | `X-Gtmux-Server-Id` header + FE mismatch handler (2.11) |
| **R12** | AttachConfirm "N new terminal(s)" 의 *history 손실 고지* | spawn 만 명시, history 손실 미고지 | "Note: previous output cannot be restored" 한 줄 (2.13) |
| **R13** | panel minimize 후 restore 시 *이전 output 보존* | 현 main = minimize 가 xterm dispose → restore 빈 화면 | **현재 unstaged** — `shouldMountTerminal = isVisible` + ResizeObserver 0x0 guard (2.15) |

---

## 4. 향후 작업 (FE 우선)

- **FE-A 후보 (지금 unstaged)**: 2.15 의 panel minimize buffer 보존 — commit + ADR-0021 D16 amend 짝 land.
- **FE-B**: 0079 connector batch (FE-A~FE-G 7 task) — BE 짝 0078 의 schema variant + validate land 후 진입. 3 commit 묶음 (renderer+gesture / clipboard+cascade / Inspector) 권장.
- **FE-C**: 0080 endpoint land 후 image/document 도구의 실제 upload wire 검증 (현재 mocked / placeholder 상태).
- **FE-D (verify-only)**: 0073 §E AttachConfirmModal cancel chain 의 8s warning toast 실 출력.
- **FE-E (verify-only)**: 0076 RB-A land 로 rebind history replay 정상 동작 — 회귀 가드 시연.
- **FE-F (latent same-shape)**: ADR-0028 D11.1 의 `priorSnapshot` 패턴을 `zStore.#commit` / `PanelNode.onResizeEnd` 에 확장 — failMessage *"Z order change failed — reverted to previous order."* / *"Resize failed — reverted to previous size."*.
- **FE-G (manual E2E 미실행)**: 0065 finding 5종 (FE-1/2/4/6 + No-session SessionMenu Shutdown) 의 real browser 시나리오 검증.

---

## 5. 교훈 (방법론)

1. **Svelte 5 `$effect` 안의 store read = lifecycle dependency**. mount effect 에는 widget identity 입력만. presentation state 는 *별 effect*. (2.1 / 2.15)
2. **Response body 의 *일부만* 사용하면 silent absorption 위험**. `unmatched/matched/conflict/server_id/replay` 같은 field 는 *반드시* 분기 처리 또는 명시 무시 사유 주석.
3. **Optimistic update 는 항상 rollback 짝**. `applyMutation({priorSnapshot, failMessage})` invariant (ADR-0028 D11.1). 새 패턴 추가 시 자동 적용.
4. **Hot path 의 자료구조 = Map**. byId 류 lookup, late buffer running total, free-draw min-distance prune.
5. **window-level listener 는 반드시 onDestroy 짝**. drag 중 unmount path 가 가장 큰 leak source.
6. **Naming debt 가 정합 인지를 파괴**. owner_key 를 `cookie` 로 부르면 audit agent 가 false-positive 양산 — 명명 강제 (`_for_owner` / `_by_owner`).
7. **release binary mtime cross-check 필수**. source fix 만으로 demo 환경 즉시 갱신 안 됨 (0077 §5.1).
8. **사용자 요구의 *역해석* 위험**. R3 (active 의미) 처럼 *기존 의미* 를 새 context 로 잘못 일반화하기 쉽다. ADR amend 시 *UI-facing 의미* 와 *internal 권한 분리* 를 *별 절* 로 명시.

---

## 6. 검증 baseline (직전 handover, `eacccb5`)

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | tail -3
# 429 PASS / 0 FAIL (직전 handover §7)

cd /Users/ws/Desktop/projects/gtmux/codebase/frontend
pnpm check     # 317 files / 0 errors / 0 warnings
pnpm build     # OK
```

unstaged (FE 영역, 본 리포트 작성 시점):
- `M codebase/frontend/src/lib/canvas/PanelNode.svelte` — `isStreaming` → `shouldMountTerminal = isVisible`
- `M codebase/frontend/src/lib/canvas/XtermHost.svelte` — D16 amend (minimize 시 xterm 보존) + ResizeObserver 0x0 guard + refit 후 refresh

unstaged (BE 영역, 본 FE 리포트 범위 외): `M crates/http-api/src/*` 다수 — 0080 asset upload (P0) 진행중인 별 worker 영역.

---

## 변경 이력

- 2026-05-20: 초안. 사용자 요청 — FE 영역 컨텍스트 + 오류 + align 정리 회고. 직전 handover (2026-05-20 0080) cold-pickup 후 작성.
