# Session Handover — 2026-05-20 — FE terminal history 보존 + selection bbox 진행 중

> 이 문서는 `session-handover` skill 로 생성된 session 인수인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-20
> - 생성 session 의 마지막 커밋: `320a13a` (Revert "revert: selection ring fix series 3건 revert")
> - 이번 session 의 주요 주제:
>   (a) 0073 FE handover 의 5 task land + 후속 D5.6 wire / 0077 attach_index self-heal hook 까지 (terminal pool desync 완전 해소)
>   (b) Settings reload toggle / refresh button / pre-session offline UI / mount_cascade race guard 같은 session UX 가드레일
>   (c) **terminal history 보존** — svelte-flow virtualization OFF + dispatcher late-buffer cap 정리
>   (d) **selection bbox (panel selection ring) 시각 정리 — *진행 중, 사용자 의도 명확화 대기***

---

## 1. 프로젝트 개요

- **이름**: gtmux
- **한 줄 정체성**: tmux 를 backend execution engine 으로 쓰는 single-user 의 web canvas workspace — tmux/PTY 가 process/session lifecycle 의 진실, FE 가 canvas layout 의 진실.
- **현재 phase**: Stage 7+ — multi-session pivot 완료 + 0073 5-task land + 0077 self-heal hook 으로 *terminal pool ↔ session ↔ panel* 의 정합 안정화. (0078 Connector 등 별 phase 가 외부 worker 로 동시 진행 — 본 session 범위 아님.)
- **침범 불가능한 invariants** (본 session 누적):
  - **두 state 분리**: tmux/PTY state (mirror only) ↔ web state (FE 진실) — `docs/sketch.md` §4 + `CLAUDE.md`
  - **single-attach + no-takeover**: Webpage:Session = 1:1, 활성 session 강제 takeover 없음 — `docs/adr/0019-session-and-workspace-model.md` D3/D4
  - **terminal 은 process, panel 은 view**: panel close ≠ terminal kill (ADR-0018 D6 c2, ADR-0021 D1)
  - **attach_index 정합 = boot rebuild + 4 mutation hook + attach 시점 self-heal** (ADR-0021 D7 amend ③ + amend ④ — 본 session 신규)
  - **panel close 의 mirror 보호**: `attach_index` 의 file ref 기준 (= `attached_sessions`). UI badge 는 `live_attached_sessions` (= attach lock + file ref intersection). 두 의미의 wire 분리는 본 session `dc0d4f1` 에서 land.
  - **applyMutation 단일 entry** + D11.1 priorSnapshot rollback (ADR-0028)
  - **panel re-mount = WS catch-up replay 안 받음** — xterm scrollback 의 lifetime invariant. svelte-flow virtualization OFF 가 그것의 가드.
  - **ADR-before-code hard rule + ADR↔plan/handover coherence** — `CLAUDE.md`

---

## 2. 이번 session 요약 (시간순)

본 session 은 단일 backend (FE 메인) 입장에서 0073 handover 의 land → 후속 진단/회복 → UX 강화 → terminal history 보존 → selection bbox 의 의도 불명 → 추측 fix → revert 의 큰 cycle 을 거쳤다.

### 2.1 0073 FE handover land (commit `df05425` → `af4aed1` → `ba7069e` → `72a16e4`)

- **`df05425`** `fix(fe/stores): FE-A reconnectGate.cancel tentative detach (0071 §B-1)` — `reconnectGate.cancel()` 의 fire-and-forget detach + ADR-0019 D5.4 amend ② (외부 commit `8814b06` 에 본 amend 동봉) + state-machines §3.3/§5.1 row 갱신.
- **`af4aed1`** `feat(fe/chrome): FE-B AttachConfirmModal history loss notice` — *"New terminals start fresh — previous output cannot be restored."* note row + dashed badge.
- **`ba7069e`** `feat(fe/session): D5.6 webpage_id wire + D5.5.1 cancelAttachConfirm chain` — in-flight 8 file (lib/session/webpageId.ts 신규 + WorkspaceSwitcher 의 5-step chain + `POST /detach` → `DELETE /attach` 전환 등).
- **`72a16e4`** `feat(fe/lifecycle): FE-C leaveBeacon on page unload` — `lib/lifecycle/leaveBeacon.ts` 신규 + `+page.svelte` bind/unbind.

### 2.2 0077 — terminal pool ↔ session ↔ panel desync 진단·해결 (commit `72278b1` → `a1ecdb3` → `8cd925a` → `5ea3dc3` → `c63be0c` → `a276058` → `452e63c` → `dc0d4f1` → `bd04e43`)

사용자 보고: TerminalListView 의 row 가 *canvas 에 panel 있음에도* "unplaced"/"desync" 로 표시 + 다른 session 의 reference 인데 다른 webpage 에서 `pool only` 로 표시.

진단 → 시도 → 실패 → release rebuild → 진짜 root cause (attach_index 의 mutation hook 외 source 의 영속 stale) → **self-heal hook** 으로 해소:

- **`72278b1`** F3/F4/F5 통합 — TerminalListView kill defensive guard (`isOnCurrentCanvas` 시 kill 차단) + `(!) desync` badge + `$effect` console.warn + Mine/All segmented + Inspector sess row 항상 표시.
- **`a1ecdb3`** tab label `THIS / ALL`.
- **`8cd925a`** BE attach_index 의 4 mutation site 에 tracing.
- **`5ea3dc3`** THIS filter 의 `sessionStore.items.has(t.id)` union (desync row 가 THIS 모드에서도 보이도록).
- **`c63be0c`** BE `rebuild_from_disk` 의 per-session debug + `sessions_skipped > 0` 시 WARN.
- **`a276058`** **BE self-heal hook** — `classify_layout_terminals` + `attach_confirm_handler` 의 200 직전 `attach_index.apply_full_session(name, &uuids)` 호출. boot rebuild miss / race 어떤 source 든 *session 연결 시점에 자동 회복*. **결정적 fix.**
- **`452e63c`** `docs(adr+report): 0077 + ADR-0021 D7 amend ④` — 진단/해결 보고서 + ADR-0021 D7 amend ④ (self-heal hook 을 invariant 로 격상).
- **`dc0d4f1`** `live_attached_sessions` wire — BE `TerminalInfo.live_attached_sessions` 추가 (file ref 기준 `attached_sessions` 와 *attach lock 보유* session 의 intersection). UI badge 는 live count, kill guard 는 file ref (data safety 보존).
- **`bd04e43`** badge 표현 `×N+M` → `×N` (main) + 작은 `+M` superscript (dim).

핵심 진단 비효율: 본 session 의 초반 진단에서 *binary 가 latest 인지 검증* 안 함 → `cargo build --release` 누락으로 BE source fix 가 적용 안 된 채 사용자 시연 → root cause 확정 cycle 추가. 0077 §5.1 의 교훈으로 기록.

### 2.3 Session UX 가드레일 (commit `638b133` → `8a897d5` → `89d7ba4`)

- **`638b133`** `feat(be+fe/settings): reload_on_session_switch toggle` — Settings `BehaviorSettings.reload_on_session_switch: bool` (default true) + `WorkspaceSwitcher.maybeReloadOnSwitch(prev, next)` (`prev === null || prev === next || setting=false` skip) + SettingsOverlay toggle UI. session A→B switch 시 `window.location.reload()`.
- **`8a897d5`** `feat(fe/chrome+canvas): pre-session offline UI + app/panel refresh buttons` — `ReconnectBanner` grace 1000→300ms + Titlebar 우측 refresh 버튼 (`window.location.reload()`) + PanelNode header refresh 버튼 (`{#key terminalReloadKey}<XtermHost />{/key}`).
- **`89d7ba4`** **revert** — panel refresh button (PanelNode 의 `{#key}` re-mount) 가 *history 손실* 회귀. XtermHost re-mount = WS subscribe 새로 시작 + catch-up replay 못 받음. Titlebar / Settings reload 는 그대로 유지. **future fix design 미정** (BE replay endpoint 또는 xterm.reset 또는 button 제거 유지 — 사용자 결정 대기).

또한 외부 worker commit:
- **`abc5931`** (외부) — BE+FE `mount_cascade` wire 에 `trigger_session` 동봉 + FE handler race guard. 본 session 의 F1 가설을 외부 worker 가 land.

### 2.4 Terminal history 보존 (commit `4795c08` → `70640c3`)

사용자 보고: viewport 확대 후 새로고침 + 축소 시 viewport 밖 panel 의 history 사라짐. 추가 — *새로고침 없이도* viewport 벗어남 만으로 같은 현상.

- **`4795c08`** `fix(fe/canvas+dispatcher): svelte-flow virtualization OFF + late-buffer cap 4 MiB` — `Canvas.svelte:1277` `onlyRenderVisibleElements={true}` → `{false}` (root cause fix — PanelNode unmount → xterm scrollback destroy 의 path 자체 차단) + `PANE_LATE_BUFFER_CAP` 256 KiB → 4 MiB.
- **`70640c3`** **revert** — late-buffer cap 4 MiB → 256 KiB. BE `pty-backend/src/lib.rs:70` `RING_CAPACITY = 128 * 1024` (128 KiB) 확인 후, 옛 256 KiB cap 이 *2× margin 으로 이미 충분 cover*. 4 MiB 는 잘못된 추정. virtualization OFF 만이 real fix.

### 2.5 Panel minimize 시 resize + selection ring (commit `c83a22c` → `90d529c` → `d304f3f` → `bbed597` → `74a748f` → `25562ef` → `7659bf3` → `320a13a`)

사용자 보고 (다단계):
1. terminal panel 최소화 상태에서 resize → 빈 contents
2. selection 파란 ring 의 z-index 가 header 에 의해 가려짐
3. panel selection 시 header 색 변하는데 "파란색 테두리만 통일"
4. *"내 의도와 전혀 달라. 원래대로 돌리고, 지금 내가 의미하는 bbox 가 무엇인지부터 파악해야"*
5. **bbox = 직사각형 (no rounded corner)**

진행:
- **`c83a22c`** resize while minimized 시 minimized 자동 해제 — *반려* (mental model 모호).
- **`90d529c`** **유지된 fix** — NodeResizer `isVisible` 에 `data.minimized !== true` 추가. minimize 시 resize handle 자체 차단 (사용자 채택 design).
- **`d304f3f`** selection ring `box-shadow` → `outline` — `.svelte-flow__node` 의 `outline: none !important` specificity 충돌로 *ring 자체 사라짐* 회귀.
- **`bbed597`** d304f3f 의 outline 변경 revert + header tint 제거 (`.panel.m-single/m-multi .panel-header` 의 background/border-bottom 변화 제거).
- **`74a748f`** `MIN_HEADER_H` 32→34 — minimize 시 header overflow 가정 fix (잘못된 가정).
- **`25562ef`** `.panel.m-single/m-multi` 의 `border-color: var(--color-accent)` — *.panel 회색 border 와 ring 의 시각 충돌 가정 fix (잘못된 가정).
- **`7659bf3`** 위 3 commit (bbed597 + 74a748f + 25562ef) 모두 revert — 사용자가 *"전혀 다르다"* 명시.
- **`320a13a`** 7659bf3 의 revert (사용자 *"한 단계 이전으로"*) — 현 HEAD = `25562ef` 적용된 state (header tint 제거 + MIN_HEADER_H=34 + .panel border accent).

**현 working tree HEAD = `320a13a`**. FE check / build 모두 통과. dist 갱신 (`index-DPkmQFPI.js` + `index-CB7gwCz7.css`).

### 2.6 정합 검증

각 commit 단계마다 `pnpm check` + `pnpm build` 통과. cargo build / test 도 BE 변경 단계마다 통과 (release rebuild 까지 cycle 의 마지막에 진행).

---

## 3. 주요 참조 자료

### 3.1 새 session 이 가장 먼저 읽어야 할 문서

| 우선 | 문서 | 의미 |
|---|---|---|
| 1 | `/Users/ws/Desktop/projects/gtmux/CLAUDE.md` | 프로젝트 invariants + 어휘 + ADR/SSoT 정합 hard rule |
| 2 | `docs/reports/0077-terminal-pool-attach-index-desync-resolution.md` | 본 session 의 결정적 진단 + commit series 의 layer 분류 + ADR amend ④ draft. self-heal hook design 의 근거. |
| 3 | `docs/adr/0021-terminal-pool-and-mirror.md` D7 amend ③ + amend ④ | attach_index 의 정합 invariant — boot rebuild + 4 mutation hook + attach 시점 self-heal. |
| 4 | `docs/reports/0073-fe-handover-from-0071-audit.md` | 본 session 이 land 한 FE-A/B/C 의 origin 명세 (anchor + acceptance criteria). |
| 5 | `docs/adr/0019-session-and-workspace-model.md` D5.4/D5.5/D5.6 | session attach lifecycle 의 ground truth (외부 commit `8814b06` 의 D5.4 amend ② 포함). |
| 6 | `docs/ssot/state-machines.md` §3.3 / §5.1 | reconnectGate / workspaceSwitcher 5-stage / AttachConfirmModal entry 분기. |

### 3.2 활성 plan / 참고

- 활성 plan = `docs/plans/0011-component-design-batch-caption-document.md` (외부 worker 영역, 본 session 무관) — 단 *어떤 frontend-design 결정* 인지 확인 필요.
- 본 session 의 *직접* plan 문서 없음 — 0073 handover (`docs/reports/0073-...`) 가 plan 역할.

### 3.3 핵심 코드 anchor (본 session 누적)

| 파일 | 의미 |
|---|---|
| `codebase/frontend/src/lib/stores/reconnectGate.svelte.ts:111-145` | FE-A: `cancel()` 의 fire-and-forget `detachSession(attemptName)` |
| `codebase/frontend/src/lib/chrome/WorkspaceSwitcher.svelte:80-103, 191-260` | `maybeReloadOnSwitch` + `cancelAttachConfirm` 5-step chain |
| `codebase/frontend/src/lib/sidebar/TerminalListView.svelte` | THIS/ALL toggle + badge 표현 + desync badge + Kill defensive guard |
| `codebase/frontend/src/lib/chrome/ItemInfoView.svelte:566-604` | Inspector 의 attach/sess row (live + inactive 분리) |
| `codebase/frontend/src/lib/lifecycle/leaveBeacon.ts` | FE-C — `beforeunload`/`pagehide` 의 sendBeacon |
| `codebase/frontend/src/lib/ws/dispatcher.svelte.ts:86, 437-499` | `PANE_LATE_BUFFER_CAP = 256 KiB` + `handleMountCascade` trigger_session race guard |
| `codebase/frontend/src/lib/canvas/Canvas.svelte:1277` | `onlyRenderVisibleElements={false}` (virtualization OFF) |
| `codebase/frontend/src/lib/canvas/PanelNode.svelte:314,381,550-566` | `MIN_HEADER_H=34` + NodeResizer `isVisible` 의 `minimized !== true` + `.panel.m-single/m-multi` 의 selection 시각 |
| `codebase/backend/crates/http-api/src/attach_index.rs:60-117` | `apply_diff` / `apply_full_session` + drift WARN logging |
| `codebase/backend/crates/http-api/src/sessions.rs:795-830, 605-625` | `classify_layout_terminals` + `attach_confirm_handler` 의 self-heal hook |
| `codebase/backend/crates/http-api/src/terminals.rs:140-200` | `TerminalInfo.live_attached_sessions` + `list_handler` 의 holders snapshot |

---

## 4. 진행중인 작업

### 4.1 selection bbox 의 정확한 design 명확화 (P0 — *이번 session 의 마지막 in-progress*)

- **상태**: revert 의 revert (`320a13a`) 완료, dist 갱신. 현 HEAD 의 selection 시각 = `25562ef` 적용된 state. 사용자가 *"bbox = 직사각형 (no rounded)"* 명시한 후 본 session 종료.
- **관련 문서**: 없음 (사용자 채팅 만)
- **관련 파일**:
  - `codebase/frontend/src/lib/canvas/Canvas.svelte:1394-1398` — `.svelte-flow__node.selected` 의 `box-shadow: 0 0 0 1.5px var(--color-accent)` (rounded — border-radius inherit)
  - `codebase/frontend/src/lib/canvas/PanelNode.svelte:533-545` — `.panel` 의 `border-radius: var(--radius-md)` (rounded)
  - `codebase/frontend/src/lib/canvas/PanelNode.svelte:550-562` — `.panel.m-single` / `.m-multi` (현 state: border-color accent + outline none)
- **사용자 채택 design** (직전 turn 의 마지막 정보):
  - bbox = **직사각형 (no rounded corner)**
  - panel selection 시 *header 색 변화 X*, *파란색 테두리만 통일*
- **본인이 *확정 못 함* 한 항목** (다음 session 의 grilling 필요):
  - bbox 위치: wrapper outer / .panel outer 의 outside / 다른 위치?
  - bbox 두께: 1.5px / 1px / 다른?
  - bbox 가 *square* 강제 — `border-radius: 0` 강제 (panel 까지 square) / absolutely positioned `<div>` overlay (panel 의 rounded 와 별 layer)?
  - figma reference 또는 sketch 있는지?
- **다음 step**: 새 session 첫 turn 에 *4 옵션 표* 제시 후 사용자 결정 → 그 위에 정확한 css path. **추측으로 진행 금지** — 본 session 이 추측 fix 시리즈로 신뢰 손실.

### 4.2 0073 FE-D / FE-E manual E2E (P2 — pending)

- **상태**: 코드 변경 0. BE demo running + 시연 환경에서 확인 만.
- **FE-D**: AttachConfirmModal cancel chain 의 8s warning toast 실 출력 시연 (`docs/reports/0073-...` §E).
- **FE-E**: rebind history replay 부재 시연 (`docs/reports/0073-...` §F). 외부 commit `6de30bb` (RB-A AttachReplayEvent broadcast) 가 그 사이 land — 즉 *replay 가 이제 동작* 가능성. 그 fix 의 시연 검증 으로 의미 변경.
- **다음 step**: BE demo 재기동 (latest release binary) 후 시연 시나리오 진행. 결과 따라 follow-up report 발주.

### 4.3 Panel header refresh button (P2 — design 미정)

- **상태**: 본 session 의 `89d7ba4` 로 revert. Titlebar refresh / Settings reload toggle 만 유지.
- **관련 문서**: `89d7ba4` 의 commit message — 3 option (BE replay endpoint / xterm.reset / 제거 유지) 명시.
- **다음 step**: 사용자 결정 후 진행. 본 session 의 virtualization OFF (4795c08) 가 *xterm destroy* path 자체 차단 — *re-mount = history 손실* 의 source 가 거의 없어서 panel refresh 의 필요성 자체 감소. *제거 유지* 가 가장 단순.

---

## 5. 향후 작업

### 5.1 ADR amend ⑤ — `live_attached_sessions` wire spec 명시 (P1)

- **목표**: ADR-0021 D7 amend ⑤ (또는 별 amend) 로 `live_attached_sessions` field 의 wire contract 명시. 본 session `dc0d4f1` 이 *코드 land* + commit message 만 — ADR 본문 미반영.
- **관련 문서**: `docs/adr/0021-terminal-pool-and-mirror.md` D7 (amend ③/④ 직후 위치)
- **선행 조건**: 사용자 시연 시 표시 의미가 mental model 정합인지 확인 (이미 종합 진행 중인 시연 후 확정 가능).

### 5.2 ADR-0021 D2 의 *catch-up timing* 명시 (P2)

- **목표**: virtualization OFF (4795c08) 의 design 근거 — *WS catch-up replay 는 cookie attach 시점 1회* 의 invariant 를 ADR-0021 D2 (또는 D6) 에 명시. 미래 worker 가 *xterm re-mount = history 보존* 의 잘못된 가정 회피.
- **관련 문서**: `docs/adr/0021-terminal-pool-and-mirror.md` D2 + D6 (heartbeat)
- **선행 조건**: 본 session 의 virtualization OFF + late-buffer cap 256 KiB 정합이 사용자 시연에서 stable 확인 후.

### 5.3 옵션 — Settings reload toggle 의 UX 시연 결과 (P2)

- **목표**: 사용자가 *session switch 시 reload* 의 실 perception (400-700ms blink) 가 자연한지 시연 후 결정. 만약 blink 가 손상 perception 이면 *option D (soft reset — store reset + GET layout)* path 검토.
- **관련 문서**: 638b133 commit message 의 cost 분석
- **선행 조건**: 충분한 사용자 시연 + 명시 평가.

---

## 6. 주의사항 / Gotchas

### 6.1 Binary mtime check 절차 — 본 session 의 가장 큰 비효율 source

본 session 의 0077 진단에서 *release binary stale 인 채로 사용자 시연* → root cause 확정 cycle 추가. **모든 BE 변경 commit 후 반드시** `cargo build --release` 실행 + binary mtime 확인 후 사용자 시연 요청. 본 session 의 `0077 report §5.1` 의 교훈 그대로.

### 6.2 PanelNode 의 `box-sizing: border-box` + 1px border + header height 32px

- panel outer height = data.h = wrapper height (svelte-flow inline style)
- panel content area = (h - 2)px (border 1px top + bottom)
- panel-header height: 32px → minimize 시 h=32 면 content=30, header overflow 2px (panel `overflow: visible` 라 외부로 그려짐)
- 본 session `74a748f` 가 `MIN_HEADER_H=34` 로 fix — 현 HEAD 유지. 단 *진짜 root cause* 모름 — 사용자 *"bbox 직사각형"* 명시로 더 큰 design 변경 의도 가능.

### 6.3 xterm re-mount = history 손실 (절대 회귀 금지)

- BE 의 PANE_OUT *catch-up replay* 는 *cookie attach 시점 1회* 만 — *mid-session 의 XtermHost re-mount* 는 broadcast 의 *후속 frame* 만 받음 = 화면 history 손실.
- 본 session 의 `89d7ba4` revert 의 root cause. `{#key}` block 으로 XtermHost 강제 re-mount 는 *반드시* BE replay endpoint 와 짝 land.
- `4795c08` 의 virtualization OFF 가 *PanelNode unmount → XtermHost destroy* path 자체 차단 — 의도된 invariant. 미래 worker 가 `onlyRenderVisibleElements={true}` 복원 시도 시 **반드시** virtualization 의 history 손실 path 가 fix 됐는지 확인 후.

### 6.4 `attach_index` 의 `apply_full_session` 의 *full replace* 의미

- `apply_full_session(session, &uuids)` 는 *그 session 의 모든 contribution 을 drop 후 새 uuids 로 reinsert*. 만약 `load_terminal_uuids` 가 빈 array 반환 (예: schema parse failure) 시 *그 session 의 attach_index entry 모두 erase*.
- 본 session `a276058` 의 self-heal hook 이 이 함수 사용 — *load_terminal_uuids 결과가 정확* 가정. 미래 schema 변경 시 *schema 호환성* 검증 후에만 self-heal 안전.

### 6.5 외부 worker 의 in-flight 작업 (본 session 무관, 그러나 인지 필요)

본 session 종료 시점 `git status --short` 에 backend 측 다수 modified file (~15+ file: `assets.rs`, `attach_index.rs`, `auth.rs`, `file_open/*`, `fs_list.rs`, `lib.rs`, `schema.rs` 등). 이는 외부 worker 의 0078 BE-A Connector / 0080 local asset upload 등의 in-flight. 본 session 의 작업과 conflict 없음 — 그러나 BE attach_index 의 in-flight 변경이 본 session 의 self-heal hook 과 *함께* land 되면 정합 검증 필요.

### 6.6 사용자가 명시적으로 거부한 접근법

- **xterm re-mount 로 *terminal refresh*** — `89d7ba4` 의 revert. *history 손실* 의 source.
- **resize 시 minimized 자동 해제** — `c83a22c` 의 revert. 사용자가 *NodeResizer `isVisible` 차단* (= 명료한 UI signal) 선호.
- **outline 으로 selection ring 변경** — `d304f3f` 의 revert. specificity 충돌로 invisible 회귀.
- **selection 시 header tint** — *유지 vs 제거* 는 본 session 의 in-progress 영역 (4.1). 사용자 명시 *"파란색 테두리만 통일"* + 현 HEAD 는 header tint 제거된 state.

---

## 7. 새 session 시작 방법

1. **이 handover 문서를 끝까지 읽는다.**
2. `CLAUDE.md` (+ §3.1 의 SSoT/ADR) 를 읽는다.
3. §4.1 (selection bbox design 명확화) 의 *4 옵션 표* 를 사용자에게 제시 — *추측으로 fix 진행 금지*. 사용자 채택 design 결정 후에만 css 변경.
4. 그 전에 `git log --oneline 320a13a..HEAD` 로 *handover 이후의 외부 worker 변경* 확인.
5. selection bbox design 결정 + land 후, §4.2 (0073 FE-D/E manual E2E) 또는 §5 (ADR amend ⑤ — `live_attached_sessions` wire) 중 사용자 우선순위에 따라 진행.
6. **BE 변경 시 반드시** `cargo build --release` + release binary mtime 확인 후 사용자 시연 요청 (§6.1).

---

## 8. 본 session 의 commit list (요약)

(시간순, 위 §2 의 각 단계 commit. 총 ~25 commit, 모두 main 에 land. 추가 외부 worker commit 도 동시 land.)

본 session 주요:
- 0073 FE handover: `df05425`, `af4aed1`, `ba7069e`, `72a16e4`
- 0077 desync resolution: `72278b1`, `a1ecdb3`, `8cd925a`, `5ea3dc3`, `c63be0c`, `a276058`, `452e63c`, `dc0d4f1`, `bd04e43`
- Session UX 가드레일: `638b133`, `8a897d5`, `89d7ba4` (revert)
- Terminal history 보존: `4795c08`, `70640c3` (revert)
- Panel minimize / selection bbox cycle: `c83a22c` (revert), `90d529c`, `d304f3f` (revert), `bbed597`, `74a748f`, `25562ef`, `7659bf3` (revert), `320a13a` (revert of revert)

외부 worker (본 session 무관):
- `abc5931` — mount_cascade `trigger_session` race guard
- `8814b06` — D5.6 owner_key wire+naming (FE-A 의 ADR amend ② 동봉)
- `111378c` — `/api/leave` endpoint (FE-C 의 짝)
- `df90859` — BE-C stale lock scan
- `6de30bb` — RB-A AttachReplayEvent broadcast (FE-E 의 의미 변경)
- 그 외 0078 Connector / 0080 asset upload 등

---

## 9. 변경 이력

- 2026-05-20: 초안. 본 session 의 0073 land → 0077 desync → UX 가드레일 → history 보존 → selection bbox in-progress 정리.
