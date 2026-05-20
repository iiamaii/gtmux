# gtmux 도메인 컨텍스트

gtmux는 PTY 와 child process 를 직접 관리하는 backend (process supervisor, ADR-0014) 위에서 사용자가 그 Pane 들을 무한 캔버스 GUI 로 시각화·제어하는 단일 사용자용 웹 앱이다. 본 문서는 프로젝트 도메인 어휘·관계·모호성을 정의한다.

> **2026-05-14 amend**: ADR-0013 채택으로 tmux backend 가 폐기되고 portable-pty 직접 ownership 모델로 전환됨. (구) "tmux 측 (mirror)" 도메인이 *gtmux Server 의 직접 관리 도메인* 으로 흡수되어 *tmux 어휘* (Window, tmux Layout, tmux session) 가 폐기됨. 자세한 배경은 `docs/reports/0023-pty-poc-verification-and-decision.md` 참조.
>
> **2026-05-15 amend**: PTY-direct 재기동 정합 정책이 확정됨. Pane 은 Server lifetime 에 종속되는 child process 이므로, 부팅 시 디스크의 `panels[]` 는 stale Pane 참조로 간주해 제거한다 (ADR-0006 D14). 비정상 종료로 살아남은 child process 는 `GTMUX_SESSION` / `GTMUX_SERVER_PID` marker 로 식별해 다음 부팅에서 best-effort reap 한다 (ADR-0014 D11).
>
> **2026-05-15 amend (multi-session pivot)**: Workspace / Session / Webpage 의 의미가 재정의되었다. Session 은 더이상 *Server 의 logical 이름* 이 아니다. 새 모델:
> - **Server : Port = 1:1** (변경 없음)
> - **Server : Workspace = 1:1** (Workspace 신규 어휘 — server 가 사용하는 storage 디렉터리, default `${XDG_DATA_HOME:-~/.local/share}/gtmux/workspace/`, config 로 override 가능)
> - **Workspace : Session = 1:N** (Session 은 workspace 안의 named layout snapshot record. 사용자 명시 생성/관리/삭제)
> - **Webpage : Session = 1:1 (single-attach)** (Webpage = WS 연결 = session 의 편집 채널. 동일 session 동시 attach 금지)
> - **Terminal pool : Session = N:N** (Terminal 은 server-pool, 여러 session 의 panel 이 동시에 attach 가능 + 입력 공유 mirror)
>
> 인증은 lifecycle (cookie) 적용 — 매번 재인증 없음. 인증 후 dialog 로 "새 session 추가" 또는 "기존 session 연동" 선택. 활성 session (attach 한 webpage 있음) 은 modal 에서 disabled 표시 + takeover 금지. webpage close (정상/비정상) 는 WS heartbeat (15s ping / 30s timeout) 로 감지하고 session 의 active 플래그 해제. 자세한 결정 배경은 `docs/adr/0019-session-and-workspace-model.md` (ADR-0007 supersede), `docs/adr/0020-auth-lifecycle.md`, `docs/adr/0021-terminal-pool-and-mirror.md` (ADR-0015 amend), `docs/plans/0007-multi-session-pivot.md` 참조.

## Language

### Backend 측 (gtmux Server 가 직접 관리)

**Terminal** (구 어휘 *Pane* — 2026-05-15 amend 로 *Terminal* 으로 일원화)
gtmux Server 가 직접 관리하는 PTY pair (master + slave) + child process (shell) 의 1:1 묶음. 실행의 호스트. **Server-pool 에 소속** — 즉 Server lifetime 안에서 alive, 어느 Session 에도 종속되지 않는다. 한 Terminal 은 여러 Session 의 여러 Panel 에 동시 attach 가능하다 (= multi-session mirror, ADR-0021). 어휘 통합 배경: ADR-0013 채택으로 tmux 폐기 후 *Pane* 의 tmux 어휘 잔재 색이 약해졌고, 사용자 UI 에서도 "Terminal" 이 더 직관적이라 판단.
_Avoid_: pane (이전 어휘, 같은 의미지만 *Terminal* 로 통일), process(단독), shell(단독)

### gtmux 측 (own)

**Server (gtmux Server)**
한 Workspace 디렉터리에 1:1 바인딩되어 단일 포트에서 동작하는 gtmux 웹 앱 프로세스. 모든 Terminal (PTY pair + child process) 의 owner (ADR-0014 D1). 한 Server 는 0개 이상의 Session record 와 0개 이상의 Webpage 연결, 0개 이상의 Terminal 을 호스트한다. 사용자는 여러 Server 를 다른 포트로 동시에 실행할 수 있다 (다른 workspace path 면 완전 독립, 같은 path 면 session record 풀 공유 — 다만 한 server lifetime 마다 active webpage 단위 1 attach 라는 invariant 는 동일).
_Avoid_: server(단독), instance, daemon, supervisor (둘 다 우리 Server 의 부분 책임이지 동등 명칭 아님)

**Workspace** (2026-05-15 amend 로 신규 1차 어휘)
한 Server 가 사용하는 *storage 디렉터리*. Server 와 1:1 바인딩 (ADR-0019 D1). 내부에 0개 이상의 Session file record (`<session-name>.json`) 가 들어간다. Default 위치 = `${XDG_DATA_HOME:-~/.local/share}/gtmux/workspace/`, server config (`${XDG_CONFIG_HOME:-~/.config}/gtmux/config.toml`) 의 `workspace_path` 로 override 가능. 디렉터리 미존재 시 boot 시 자동 생성. 변경된 path 는 server 재기동 시에만 반영 (runtime 변경 X). Workspace 자체는 *storage 의 location 책임* 만 가지며, 그 안의 session 데이터/스키마는 ADR-0018 에 정의.
_Avoid_: workspaces (단수형 사용), board, data dir (모호), 구 의미의 *workspace 단위* (= 옛 Session 정의)

**Session** (2026-05-15 amend 로 의미 완전 재정의)
한 Workspace 안의 named layout snapshot record (파일). 사용자 명시 생성/관리/삭제. 각 Session 은 자기 Canvas Layout + viewport + Manipulation Selection (M) + Input Target (I) + focus mode 를 가진다. 한 시점에 정확히 한 Webpage 만 attach 가능 (single-attach, ADR-0019 D3) — 다른 webpage 가 attach 하려면 현재 attached webpage 가 close 되어야 한다 (heartbeat 15s ping / 30s timeout 으로 감지). Webpage attach 상태는 server memory 의 ephemeral flag 이고 Session file record 자체는 영속 (사용자 명시 [Delete] 만 disk 제거).
_Avoid_: workspace (구 의미, 이번 amend 로 어휘 분리), tmux session (영구 폐기), tab session, view, server-logical-name (= ADR-0007 의 옛 Session 의미, supersede)

**Webpage** (2026-05-15 amend 로 신규 1차 어휘)
gtmux UI 의 한 WebSocket 연결 채널. 보통은 한 브라우저 탭 = 한 Webpage. **Session 의 편집·관리 채널.** 한 Webpage 는 한 시점에 정확히 0 또는 1 Session 에 attach 한다 (인증 후 dialog 통과 전까지는 0, 통과 후 1). Attach lifetime 은 WS lifetime 과 일치 — WS close (정상/비정상) 시 그 Session 의 active 플래그 해제. 인증은 lifecycle (cookie) 적용이라 매번 재인증 안 함.
_Avoid_: tab (browser 측 어휘), connection (transport 차원), client (multi-user 함의 — gtmux 는 single-user)

**Canvas**
한 *Session* 이 가지는 단일 무한 작업 공간 (2026-05-15 amend 로 scope 가 Server → Session 으로 좁아짐). 자기 Session 의 모든 Item (terminal Panel + non-terminal Canvas Item) 을 0 개 이상 노출한다. 같은 Server 의 다른 Session 은 자기 별도의 Canvas 를 가진다.
_Avoid_: workspace (이번 amend 로 다른 의미), board, viewport (viewport 는 Canvas 의 부분 화면)

**Panel**
Canvas 위에서 한 Terminal 을 표현하는 시각 객체. 위치·크기·visibility·minimize·lock·z-index·label·note 등 web-only 상태를 가진다. **Panel ⊂ Canvas Item** — schema v2 (ADR-0018) 에서 Panel 은 `type:"terminal"` 인 Canvas Item 이며, non-terminal Canvas Item (text/note/shape/image/document/file_path) 과 같은 array `items[]` 에 산다.
_Avoid_: window, tile, widget

**Canvas Item** (2026-05-15 amend 로 신규 어휘 — schema v2 의 1차 도메인 단위)
Canvas 위 모든 시각 객체의 상위 개념. `type` discriminant 로 분기 — `terminal` (= Panel), `text`, `note`, `rect`, `ellipse`, `line`, `free_draw`, `image`, `document`, `file_path`. 공통 필드 (id / x / y / w / h / z / visibility / locked / label / description / minimized / parent_group_id) + 타입별 payload. 자세한 schema 는 ADR-0018. **2026-05-15 G20 grilling amend**: 옛 `maximized` schema field 는 *제거* — FE-only ephemeral state 로 강등 (영속 안 함, 다음 attach 시 fresh). **G24 grilling amend (ADR-0024)**: `z` field 와 Layer tree order 는 *완전 분리* — 작업공간 canvas 모델 (조직 ≠ 시각 stacking). Tree drag 는 organization 만, z 는 4 액션 (Bring/Send) 으로만. Group 은 z 없음.
_Avoid_: element (HTML element 와 혼동), object (모호), node (graph 함의)

**Canvas Layout**
한 Session 의 Canvas 의 모든 Item 의 배치 (좌표·크기·visibility·lock·z) + viewport state + selection state 의 직렬화. **Session file record 의 본체** (`<workspace>/<session-name>.json`). gtmux 가 영속화 (ADR-0006 + ADR-0018 schema v2). **2026-05-15 amend (multi-session pivot)**: Session attach 시 *match-or-spawn* 알고리즘으로 layout 의 terminal item.id 와 server-pool 의 alive Terminal id 매칭 (ADR-0018 §매칭). 매칭 없으면 fresh spawn, unmatched 가 양쪽에 있으면 confirm dialog. 즉 *재기동 후 Layout 손실* 정책은 폐기되고 *명시 save/load + match-or-spawn* 으로 대체된다 — ADR-0006 D14 의 "panels[] strip" 는 schema v1 시대의 도식이며 schema v2 로 hard cutover 후 무의미해진다 (ADR-0006 D15).
_Avoid_: layout(단독, 모호)

**Manipulation Selection (M)**
한 Session 의 Canvas 위에서 사용자가 *제어 대상* 으로 잡은 Item (들). 다중 선택 가능. 위치 이동·정렬·minimize/maximize·invisible (hide)·lock·close (= terminal Panel 의 close 는 그 Panel 의 layout 에서 제거 + 매칭된 Terminal 의 SIGTERM 여부는 별 결정) 등의 액션 대상. **Session-scoped** — 다른 Session 의 M 과 독립.
_Avoid_: focus(단독), active panel(단독)

**Input Target (I)**
키보드 터미널 입력이 라우팅될 단일 Terminal. 한 Session 안에서 unique (= 그 Session 의 active webpage 가 키보드 타이핑할 때 라우팅되는 대상). M 과 직교 (같을 수 있지만 같아야 하는 건 아님). **Session-scoped** — 다른 Session 의 I 와 독립. Terminal 자체는 server-pool 이므로 한 Terminal 이 여러 Session 의 I 일 수 있음 (각 session 의 webpage 가 동시에 그 terminal 에 타이핑하면 모두 같은 shell 에 도달 — multi-session mirror).
_Avoid_: focus(단독), active pane(단독), input session

**Panel Streaming State**
Panel 별 *데이터 흐름 활성 여부*. 값 = `Streaming` 또는 `Suspended`. visibility=hidden 이거나 minimized=true 이면 Suspended, 그 외엔 Streaming. **2026-05-14 amend**: backend 구현은 *broadcast subscribe 의 drop / 재등록* 으로 변환 (구 `refresh-client -A pause/continue` 컨셉 폐기). Suspended 상태에서는 dispatcher 가 그 terminal 의 broadcast receiver 를 drop → WS 트래픽 절감 + xterm 렌더 부하 절감. PTY master fd 의 byte stream 은 backend 안에서 계속 흐르되 broadcast cap 도달 시 자연 backpressure (ADR-0002 §D7 amend, ADR-0013 D3). **2026-05-15 amend (multi-session pivot)**: Streaming/Suspended 는 *(session, panel)* 쌍 단위. 한 Terminal 이 여러 Session 의 panel 에 attach 되어 있을 때 각 panel 의 streaming 상태가 독립이다 — 한 session 의 panel 이 Suspended 여도 다른 session 의 panel 이 Streaming 이면 그 terminal 의 broadcast 는 살아있다.
_Avoid_: active(단독, M/I 와 충돌), live(단독)

**Group**
사용자가 Panel 들을 묶는 web-only 계층적 분류 단위. Figma 의 layer 처럼 트리 구조를 가지며 (Group 안에 Group 가능, 다중 부모 금지), 한 Panel/Group 은 정확히 한 부모 Group 또는 Canvas 루트에 속한다. Group 은 **frame 을 1차 상태로 저장하지 않는다** — 자체 상태는 label·color·visibility·lock·order 뿐. "Group 이동" 은 사용자가 드래그한 delta 를 모든 자손 Panel 좌표에 일괄 적용하는 *액션* (G-hybrid 모델). 사용자 측 묶음의 **유일한 1차 도메인 개념** (ADR-0010).
_Avoid_: folder, category, cluster, frame, window

**Dangling Terminal Reference** (구 어휘 *Stale Panel Reference* — 2026-05-15 multi-session pivot 으로 이름·의미 모두 변경)
한 Session 의 Canvas Layout 안의 terminal Canvas Item 의 id 가 *server-pool 의 어떤 alive Terminal 과도 매칭되지 않는 상태*. 보통 Server 재기동 또는 명시 [Kill terminal] 또는 외부 import 된 workspace file 로부터 발생. **이전 정책 (ADR-0006 D14) 은 *제거 대상* 이었으나, schema v2 (ADR-0018) hard cutover 로 *fresh spawn 대상* 으로 의미 변경** — Session attach 시 dangling reference 는 같은 id 로 새 Terminal spawn (= match-or-spawn 알고리즘의 spawn 분기, ADR-0018 §매칭). 이로써 사용자의 Layout 작업이 server 재기동 후에도 보존되며, 매칭 안 되는 terminal item 의 수는 attach modal 에서 사용자 confirm 받는다 (현재 Canvas ✓ / Session record ✗ 와 반대 분기 모두 dialog 로 surface).
_Avoid_: stale panel reference (옛 어휘, 폐기), detached terminal, zombie terminal (zombie 는 OS-level 어휘와 혼동)

**Orphan Child Process**
gtmux Server 가 비정상 종료된 뒤 OS 에 남아 있는 이전 child shell/process. 정상 종료에서는 `PtyBackend` teardown 이 정리하지만, `SIGKILL`·OOM·시스템 crash 에서는 남을 수 있다. 모든 child process 는 `GTMUX_SESSION` 과 `GTMUX_SERVER_PID` env marker 를 가지며, 다음 Server 부팅 시 같은 Session marker 를 가진 이전 process 를 best-effort 로 reap 한다 (ADR-0014 D11).
_Avoid_: orphan pane (Pane 은 Server 가 소유하는 런타임 객체이고, process 만 OS 에 남음)

**Reconnect Gate** (2026-05-16 amend — ADR-0019 D5.4 + 0045 P0 후속, plan-0008)
Webpage 의 *page entry 흐름* 을 통제하는 FE-only 상태 머신 (`lib/stores/reconnectGate.svelte.ts`). 8 state — `booting` (initial, auth gate / hint 검사 중) / `idle` (hint 없거나 사용자 cancel 후) / `attaching` (`POST /attach` 진행) / `hydrating` (`GET /layout` + loadLayout 진행) / `ready` (hydrate 완료, 본 화면 mount 허용) / `in_use` (409) / `not_found` (404) / `unreachable` (5xx/network). Derived `canMountApp = ready ‖ idle` 가 본 화면 (Canvas / Toolbar / LeftPanel / RightPanel) 의 mount gate. Derived `modalState` 는 attaching/hydrating 을 `'loading'` 으로 normalize 해 `ReconnectModal` 의 mode prop 4 mode (loading/in_use/not_found/unreachable) 로 노출. Methods: `start(name)` / `retry()` / `cancel()` / `markIdle()` (booting 영구화 방지의 명시 종료) / `markReady()` (markSuccess 는 호환 alias). AbortController 보유 — 사용자 [Switch session…] 시 in-flight `POST /attach` cancel.
_Avoid_: reconnect store(단독), session recovery (모호 — recovery 는 Case II 의 silent path 와 혼동), reconnect modal store (modal 은 view, gate 는 state 머신)

**Boot Screen** (2026-05-16 amend — ADR-0019 D5.4 + 0045 P0 후속)
Webpage 의 *진입 grace* 화면. AppPage onMount 직후 + `ReconnectModal` 의 100ms grace 동안 빈 Canvas 노출을 차단하는 placeholder. `+page.svelte` 의 `{:else if reconnectGate.state ∈ {booting, attaching, hydrating}}` 분기 — spinner + 진행 메시지 ("Restoring session…" / "Reconnecting session…" / "Loading layout…"). `ReconnectModal` 이 mount 되면 그 위에 layer (modal 의 backdrop 이 boot screen 을 가림 — 시각 부담 0).
_Avoid_: splash screen (브랜딩 함의), loading overlay (모달 의 일부 — boot screen 은 modal 진입 전 placeholder), skeleton (구조적 콘텐츠 hint 함의)

**Session Attach Hint** (2026-05-16 amend — ADR-0019 D5.4)
다음 page reload 시 자동 reattach 대상이 될 *직전 active session 의 이름*. `sessionStorage` 의 `gtmux-last-active-session` key 에 저장 — **tab-scoped** (다른 탭 영향 0). 기록 시점: `sessionStore.setActiveSession()` 안 (= attach 성공 시). 제거 시점: `sessionStore.clear()` (명시 detach / [Switch session…] / `reconnectGate.cancel()` 모두 통과) + logout + session [Delete] 흐름. `sessionStorageHint.{get, set, clear}` helper (`lib/stores/sessionStorageHint.ts`) — SSR / private mode 의 throw 를 try-catch 로 graceful. AppPage onMount 의 *page entry 의사결정 tree* 에서 hint 존재 → `reconnectGate.start(name)`, 없음 → `reconnectGate.markIdle()` + `workspaceSwitcher.open()`.
_Avoid_: last session (모호), cached session (영속 함의), recent session (히스토리 함의), session cookie (cookie 는 auth 의 영역)

**Silent Reattach** (2026-05-16 amend — ADR-0019 D5.1, plan-0008 Phase 2)
*Page 가 이미 mount 된 상태에서* WS reconnect 후 또는 tab visibility 복귀 후 자동으로 시도하는 `POST /attach` 흐름 — 사용자 perception 없이 BE attach 상태 복구. Trigger 합집합: (a) `dispatcher.svelte.ts` 의 WS state `reconnecting → open` 전이, (b) `+page.svelte` 의 `document.visibilitychange === 'visible'`. Pre-condition: `sessionStore.active !== null`. `sessionStore.silentReattach(name, signal)` — in-flight singleton (`#silentReattachPromise`) 으로 중복 trigger 도 동일 promise. 결과 분기 = `ReattachResult` (`success` / `in_use` / `not_found` / `unauthorized` / `unreachable`). **Case I (Reconnect Gate 의 blocking flow) 와 직교** — Case II 는 본 화면 mount 유지, mutation 의 동시성만 `Mutation Guard` 로 차단.
_Avoid_: auto reattach (Case I 의 blocking flow 와 혼동), background reattach (background tab 함의), silent reconnect (transport-level reconnect 와 혼동 — reconnect 는 client.ts, reattach 는 sessionStore)

**Mutation Guard** (2026-05-16 amend — ADR-0019 D5.2, plan-0008 Phase 2)
*Silent Reattach in-flight 동안 모든 outgoing write 가 wait/abort 하도록 강제* 하는 invariant + helper. `sessionStore.reattachInProgress: $state<boolean>` 가 진실 신호. `ensureMutationOk(abortMessage?): Promise<boolean>` exported helper (`lib/stores/sessionStore.svelte.ts:481`) 가 사용자-facing wrapper — 모든 mutation 진입점 (Canvas 의 spawn / drag / handleTerminalClick, TextNode 의 commit, PanelNode 의 label/delete, PanelDanglingOverlay 의 respawn, LayerTreeView 의 reorder/rename/visibility/lock, TerminalListView 의 kill/attach, zStore 의 z action) 가 시작 시점에 `if (!(await ensureMutationOk('...'))) return;` 패턴으로 호출. 결과: `success` → mutation 진행 / `in_use|not_found|unauthorized|unreachable` → abort + 호출자가 message 로 toast. `reattachInProgress === false` 면 즉시 `true` (no-op guard, hot path 비용 0). 본 guard 가 web-1 idle / web-2 takeover / web-1 복귀 시나리오의 stale layout PUT race 를 차단 — BE 의 PUT layout 에 attach gate 가 없는 (FE-only 결정) 의 정합 보완.
_Avoid_: write lock (DB 함의), mutation lock (transaction 함의), reattach lock (의미 과부족 — lock 이 아닌 guard), 단순 `await pendingReattach` (helper 의 결과 분기 + abort message UX 누락)

## Relationships (2026-05-15 multi-session pivot 으로 큰 갱신)

- 한 **Server** 는 정확히 한 **Workspace 디렉터리** 에 바인딩되고, 정확히 하나의 포트를 점유한다 (ADR-0019 D1, ADR-0007 supersede).
- 한 **Workspace** 는 0 개 이상의 **Session** 을 호스트한다 (workspace dir 안의 file record `<session-name>.json`).
- 한 **Server** 는 한 시점에 0 개 이상의 **Webpage** (WS 연결) 와 0 개 이상의 **Terminal** (PTY+child) 을 호스트한다.
- 한 **Webpage** 는 한 시점에 0 또는 1 **Session** 에 attach (single-attach, ADR-0019 D3). 인증 후 dialog 통과 전까지 0, 통과 후 1. WS close (정상/비정상) 시 attach 해제 — heartbeat 15s ping / 30s timeout 으로 비정상 종료 감지 (ADR-0021 D6).
- 한 **Session** 은 한 시점에 0 또는 1 **Webpage** 에 attach (single-attach reciprocal). 다른 webpage 가 attach 하려면 현재 attached webpage 가 close 되어야 (= 활성 session 의 takeover 금지, ADR-0019 D4).
- 한 **Session** 은 정확히 하나의 **Canvas** 를 가진다 (= Session file record 의 본체).
- 한 **Canvas** 는 0 개 이상의 **Canvas Item** (terminal Panel + non-terminal) 을 호스트한다. 한 Item 은 정확히 한 **Group** 또는 Canvas 루트에 속한다. **Group 은 0 개 이상의 Item · 하위 Group 을 자식으로 가질 수 있다** (트리, 다중 부모 금지).
- 한 **Terminal** 은 **0 개 이상의 Panel 에 attach 가능 (multi-session mirror)** — 같은 Session 의 panel 들, 다른 Session 의 panel 들, 모두 같은 PTY stream 을 받고 모두 같은 shell 에 입력 (ADR-0021 D1).
- 한 **Panel** 은 정확히 한 **Terminal** 을 가리킨다 (terminal item.id == Terminal id, ADR-0018 D2).
- 한 **Terminal** = 1 PTY pair + 1 child process. 1:1:1 (Terminal : PTY pair : process), ADR-0014 D2.
- **Canvas Layout** 은 한 Session 의 Canvas 위 Item 들의 자유 배치 + viewport + selection 의 직렬화 — 이것이 그 session 의 도메인 진실 (ADR-0006 영속 + ADR-0018 schema v2).
- **Manipulation Selection (M)** 과 **Input Target (I)** 는 직교한다 — 같을 수 있지만 같아야 하는 건 아니다.
- **M · I · Viewport · Focus mode 는 모두 Session-scoped 단일 상태** 이며, Session 의 active webpage 와 양방향 sync (ADR-0021 D5). 같은 server 안 다른 session 의 M/I/Viewport 는 독립. *옛 ADR-0002 MT-3 의 "Server-wide broadcast" 는 multi-session pivot 으로 폐기되고 session-scoped 으로 amend.*
- **Terminal output / input broadcast 는 Server-scoped** — 한 Terminal 의 output stream 은 그 Terminal 을 attach 한 *모든 session 의 모든 panel* 에 broadcast (ADR-0021 D2). 입력도 마찬가지로 어느 attach 점에서든 같은 shell 로 forward. ADR-0013 D11 의 tokio::broadcast 가 이 N:N 패턴을 자연 구현.
- **Webpage 의 본 화면 mount 는 Reconnect Gate 의 gating** 을 통과해야 한다 (ADR-0019 D5.4 + 0045 P0 후속) — `canMountApp = ready ‖ idle` 만 본 화면 (Canvas / Toolbar / LeftPanel / RightPanel) mount 허용. `booting / attaching / hydrating` 동안은 *Boot Screen* placeholder, `in_use / not_found / unreachable` 동안은 *ReconnectModal* — 둘 다 본 화면 차단. *Session Attach Hint* 가 hint 의 source, *Silent Reattach* + *Mutation Guard* 가 page 사용 중 (= ready 상태) 의 BE attach 복구 path 의 invariant.

## Example dialogue (2026-05-15 multi-session pivot 반영)

> **Dev**: "두 번째 Session 을 같은 Server 에 띄울 수 있어?"
> **Domain expert**: "그래. 한 **Server** 는 한 **Workspace 디렉터리** 에 바인딩되어 0 개 이상의 **Session** 을 호스트한다 (ADR-0019). 새 Session 은 인증 후 dialog 의 [새 session 추가] 또는 UI 의 [File > New Session] 으로 만든다. 다만 한 webpage 는 한 시점에 한 session 에만 attach — 같은 session 을 둘이 동시에 편집하지는 못해 (활성 session 은 modal 에서 disabled, ADR-0019 D4). 다른 session 을 보려면 다른 브라우저 탭에서 같은 server URL 로 들어가 dialog 에서 다른 session 선택."

> **Dev**: "Workspace 와 Session 은 어떤 사이야?"
> **Domain expert**: "**Workspace** 는 한 Server 가 사용하는 *storage 디렉터리* (default `~/.local/share/gtmux/workspace/`, config 로 override 가능). 그 안에 0 개 이상의 **Session** file record 가 들어가. Session 은 사용자가 명시 생성/삭제하는 named layout snapshot. 즉 *Workspace = 데이터의 저장 위치*, *Session = 그 안의 하나의 named record*."

> **Dev**: "Terminal 이 뭐고 Panel 이 뭐야?"
> **Domain expert**: "**Terminal** (구 어휘 *Pane*) 은 backend 가 직접 관리하는 *실행 단위* — 한 PTY pair + 한 shell process. **Server-pool** 에 살아있고 어느 Session 의 어느 panel 에도 attach 가능. **Panel** 은 한 Session 의 Canvas 위에 *그 Terminal 을 시각화한 Canvas Item* (= `type:'terminal'` 인 item) — 위치·크기·visibility·label 같은 web-only 상태를 가짐. 한 Terminal 이 여러 Session 의 여러 panel 에 동시 attach 가능 — 같은 PTY stream 을 모두 받고 어느 panel 에서 타이핑하든 같은 shell 로 forward (multi-session mirror, ADR-0021)."

> **Dev**: "Panel A 와 B 를 같이 묶어서 라벨 붙이고 한꺼번에 hide 할 수 있어?"
> **Domain expert**: "그렇다. **Group** 으로 묶어. Group 에 label·색·visibility·lock 을 주면 children 에 일괄 적용된다. Group 은 web-only 도메인 개념으로 backend 와 무관 — 그리고 한 session 안에서만 의미가 있다 (다른 session 으로 옮길 수 없음)."

> **Dev**: "Server 를 종료하려면?"
> **Domain expert**: "Canvas 우상단 헤더 메뉴 → Server shutdown → confirm → 모든 Terminal (child process) 이 자손으로 정리되고 Server 도 종료 (exit 6). Session file record 들은 *workspace dir 에 그대로 남아있다* — 재기동 후 같은 session 으로 attach 하면 layout 복원되고 terminal 들은 match-or-spawn 으로 새로 spawn (ADR-0018)."

## Scope boundary (비범위, 2026-05-15 multi-session pivot 반영)

- **Session 생성/선택/삭제는 UI 의 1차 흐름.** 인증 후 dialog 에서 [새 session 추가] 또는 [기존 session 연동] 선택, modal 의 session 목록에서 활성 표시 (`in use`) + 비활성 selectable. 외부 CLI 의 `--session` flag 폐기. Server boot 시 *어느 session 도 자동 attach 안 함* (webpage 의 dialog 통과 후 사용자 선택). 단 CLI `--workspace <path>` 로 workspace 디렉터리만 명시 가능 (boot-time, immutable).
- **부팅 시 Workspace 바인딩은 immutable.** Server 는 기동 시 (CLI flag 또는 config 의) workspace path + port 에 1:1 바인딩되고 런타임에 바꿀 수 없다 (ADR-0019 D1). config 의 workspace_path 변경은 server 재기동 시에만 반영. 같은 workspace path 의 두 active server 는 session record 풀 공유 — cross-server session lock 메커니즘으로 active session 의 충돌 방지 (ADR-0019 D6).
- **Server 종료 시 모든 Terminal 정리.** Server 프로세스가 종료되면 모든 PTY + child shell 이 자손으로서 SIGHUP 받고 정리된다 (ADR-0014 D5). Session file record 는 workspace dir 에 영속, 재기동 시 다시 attach 가능.
- **비정상 종료 orphan 은 다음 부팅에서 정리.** 정상 teardown 이 안 됐어도 child process 는 marker 로 식별 (ADR-0014 D11). 동일 workspace path 의 다음 server 부팅은 marker 기반 reap 후 시작.
- **활성 Server 목록·오케스트레이션 도구는 별도 프로젝트.** 여러 Server 를 띄우고 관리하는 인덱서/런처는 gtmux 본 프로젝트의 비범위.
- **외부 attach (다른 도구에서 gtmux 의 PTY 에 직접 attach) 비범위.** ADR-0013 D8. 필요 시 P1+ 우리 측 CLI client 검토.

## Transport split (durable vs ephemeral)

- **Durable 상태**(Canvas Layout = Group 트리 + Panel 좌표/상태)는 **HTTP**가 담당한다 (`GET/PUT /api/layout`, ETag 기반 optimistic concurrency).
- **Ephemeral 신호**(live pane output, M/I/viewport/focus mode, LAYOUT_CHANGED notify)는 **WebSocket**이 담당한다.
- 이 분리는 절대적이다 — Canvas Layout 영속화 메시지를 WS로 보내거나, live pane output을 HTTP로 폴링하는 식의 교차는 금지한다.

## Multi-connection 정책 (2026-05-15 multi-session pivot 으로 큰 amend, ADR-0021 D5)

이전 정책 (MT-3 Live Mirror) 의 *server-wide mirror* 는 폐기되고 **session-scoped state + server-wide terminal stream** 의 2-layer 모델로 변경.

- **Session-scoped state** — 각 Session 의 single attached webpage 와만 양방향 sync:
  - **M (Manipulation Selection)** — 그 session 의 webpage 와만
  - **I (Input Target)** — 그 session 의 webpage 와만. 단 *입력의 실행* 은 terminal-wide (같은 terminal 을 attach 한 다른 session 의 panel 에도 같은 shell 로 forward, ADR-0021 D2)
  - **Viewport (pan/zoom)** — 그 session 의 webpage 와만. 다른 session 의 viewport 는 독립.
  - **Focus mode** — 그 session 의 webpage 와만.
- **Server-scoped terminal stream**:
  - Terminal 의 PTY output 은 server-wide broadcast — 그 terminal 을 attach 한 *모든 session 의 모든 panel* 이 같은 stream 받음 (ADR-0021 D2, tokio::broadcast 의 N:N 패턴).
  - Terminal input (키 입력) 도 attach 점 어디서든 같은 shell 로 forward.
  - Terminal lifecycle (alive/dead, kill) 도 server-wide event 로 모든 session 에 알림.
- **Session 의 active flag** 는 server memory ephemeral. attached webpage 의 WS heartbeat (15s ping / 30s timeout) 로 추적 (ADR-0021 D6). 정상 close = 즉시 inactive, 비정상 close = 30s 안에 inactive.
- 한 Webpage 는 한 시점에 한 Session 만 attach (single-attach, ADR-0019 D3). 동일 Session 다중 attach 금지 — modal 에서 활성 session 은 disabled + "in use" badge (ADR-0019 D4 takeover 금지).
- 서버 메모리에만 보관 (durable 아님). Server 재시작 시 session 의 ephemeral state 는 reset, file record 는 유지. webpage attach 시 layout 복원되고 M/I/viewport 는 default 로 초기화.

## Placement principle (Canvas Item 좌표 정책, 2026-05-15 multi-session pivot 반영)

- **Auto-mount 는 trigger session 에만** (ADR-0021 D3, ADR-0015 amend). 한 Session 의 active webpage 가 [New Terminal] 누르면 *그 session 의 layout* 에만 새 Panel 이 cascade mount. 다른 Session 의 Terminal list 에는 표시되지만 명시 [Attach] 까지는 그 session 의 layout 에 들어가지 않음.
- **Session attach 시 layout 의 terminal item 매칭** — match-or-spawn 알고리즘 (ADR-0018 §매칭). server-pool alive Terminal 과 id 매칭되면 그대로 reconnect, 매칭 없으면 같은 id 로 fresh spawn. 매칭 안 되는 unmatched 가 양쪽 (current canvas ✓ / record ✗, 또는 current ✗ / record ✓) 에 있으면 attach 직후 confirm dialog.
- **신규 Item 의 캔버스 좌표는 optional** — 사용자가 명시 입력하면 그 위치, 미지정이면 자동 cascade 배치.
- 명시 입력 매커니즘: 빈 캔버스 영역 클릭 + "Create here" 컨텍스트 메뉴, toolbar 의 drag-to-create gesture.
- **자동 cascade 배치**: 시작점 = Canvas 좌표계의 origin (0, 0). 직전 자동 배치 item 위치 + (40px, 40px) offset. **Session-scoped** (각 session 의 cascade 카운터 독립), session 메모리만, 영속화 안 함.
- Item 은 자유롭게 겹칠 수 있다 (overlap 허용).

## Terminal lifecycle invariant 의 UI 측 mirror (2026-05-15 multi-session pivot 으로 큰 amend)

> 본 절은 (구) "Pane lifecycle invariant" 의 후속. multi-session pivot 으로 invariant 자체가 약화됨 — *최소 1 Terminal 보유* 정책은 single-session 시대의 도식이고, multi-session + named session record 모델에서는 *빈 session* 이 valid empty state 가 된다.

이전 (single-session) 정책 — *server lifetime invariant 로 최소 1 Pane 보유*, *마지막 Pane close 시 close 버튼 disable* — 은 **폐기**. 새 정책:

- **Server 의 Terminal 수 = 0 가능.** 모든 Session 이 빈 layout 이거나 모든 Terminal 이 kill 됐어도 OK. Server 는 그대로 살아있다 — 사용자가 dialog 에서 새 session 만들거나 새 terminal spawn 으로 작업 재개. *"마지막 Pane 종료 = Server 의도 모호"* 의 옛 우려는 *session record 가 영속이므로* 더 이상 성립 안 함 (작업 손실 위험 0).
- **Session 의 Panel 수 = 0 가능.** 빈 Canvas 의 session 도 valid 한 state — 사용자가 [New Terminal] 또는 다른 Canvas Item 으로 작업 시작.
- **Close 버튼 항상 활성** (ADR-0021 §close-semantic). 마지막 panel close 도 OK — 그 session 의 canvas 가 빈 상태가 될 뿐. **2026-05-15 G25.1 amend**: Close 액션의 default 정책은 *매번 confirm dialog* (ADR-0021 D9.2) — 옵션 = `[Cancel]` / `[Panel only]` (terminal 은 server-pool alive) / `[Panel + Terminal]` (terminal SIGTERM). Dialog 안에 그 terminal 이 mirror 된 다른 session 이름 hint 표시. `Settings.behavior.auto_kill_terminal_on_panel_close = true` 시 dialog 없이 `[Panel + Terminal]` 즉시 (default false — 안전). 다른 session 의 mirror panel 은 SIGTERM 시 dangling 상태가 되고, *focus / click / input interaction* 시 same id 로 fresh spawn (ADR-0021 D10.1 의 c2 lazy spawn).
- **Server shutdown 액션**: Titlebar 메뉴에서 명시 호출 → confirm modal → graceful teardown (ADR-0014 §D7: WS close + 모든 child SIGHUP 정리 + 모든 session record sync flush + state/lock 정리) → exit 6. Session file record 는 *workspace dir 에 영속* — 재기동 시 attach 가능.
- **Visibility 와의 직교성** (그대로 유지): visibility=hidden Panel 은 close 정책에 영향 없음. Streaming State (Suspended → broadcast subscribe drop) 는 *(session, panel)* 쌍 단위 — 다른 session 의 같은 terminal panel 이 Streaming 이면 그 terminal 의 broadcast 는 살아있다.
- **자동 재기동 (LIFE-AUTOSPAWN) 미채택** (그대로 유지) — 검토되었으나 ADR-0001 D12 의 "자동 재시도 안 함" 정신 + 사용자 의도 불명확성으로 채택 안 함. 배경: `docs/reports/0022-logic-amendment-decisions.md` §1.
- **부팅 시 정합 처리**: previous server 의 orphan child process 는 marker 기반 reap (ADR-0014 D11). Session record 의 terminal item.id 는 다음 server 의 alive Terminal 과 매칭되지 않으므로 attach 시 *match-or-spawn 의 spawn 분기* 로 새 Terminal 생성 (ADR-0018). 즉 *"terminal 이 안 붙은 layout"* 이 잠시 보이는 시간은 attach 직후 ~수 100ms (fresh spawn 시간) 뿐.

## Z-index 정책 (2026-05-15 G24 grilling amend, ADR-0024)

- 모든 Canvas Item 은 정수 z 를 갖는다 (Canvas Layout schema 의 `z` 필드, item-level — Group 은 z 없음).
- 신규 Item z = 현재 최대 z + 1.
- Item 이 Manipulation Selection (M) 에 들어오면 z = 현재 최대 z + 1 (자동 최상위). M 에서 빠진 후에도 z 를 유지.
- 명시 z 조정 = ADR-0024 D2 의 4 액션 — *Bring to front* (Shift + `]`) / *Send to back* (Shift + `[`) / *Bring forward* (`]`) / *Send backward* (`[`).
- **Tree order 와 z 는 완전 분리** (ADR-0024 D1) — Layer list 의 drag reorder 는 *organization 만* 변경, z 영향 없음. Layer list 상단 toggle `[Tree | Z]` 의 Z 모드는 read-only flat 정렬 보기.

## Flagged ambiguities (2026-05-15 multi-session pivot 후 갱신)

- "server"(단독, 소문자) 사용 금지. 항상 **Server (gtmux Server)** 로 명시한다.
- "window"(단독) 사용 금지. **Group**(UI 측 묶음) 또는 *브라우저 창*(OS-측 window)으로 명시.
- "layout"(단독) 사용 시 **Canvas Layout** 으로 명시.
- "session"(단독) 사용 금지. **Session** (Workspace 안의 named layout record) 로 명시. 옛 의미 (Server 의 logical 이름) 는 *이번 amend 로 폐기* — 그 자리 (token scope, lock, log key) 는 사라지거나 *Workspace path* 로 옮겨감.
- "workspace"(단독) 사용 시 **Workspace** (Server 의 storage dir) 로 명시. *"workspace 단위"* 의 옛 사용 (= Session 의 의미) 은 폐기.
- "pane"(단독) 사용 시 **Terminal** 로 통일 (2026-05-15 amend) — 옛 어휘 *Pane* 은 호환성을 위해 코드/문서에 잠시 공존하나 *Terminal* 로 점진 통일.
- ~~§6.1 session 제어 기능 의미~~ → 해소 (2026-05-15 multi-session pivot: Session 어휘 재정의 + UI 1차 관리. [New / Delete / Attach] 모두 UI).
- ~~§6.2 window 제어 기능 의미~~ → 해소 (tmux Window 컨셉 폐기, UI 측 묶음은 Group 이 담당).
- ~~Group 의 spatial cohesion 정책~~ → 해소 (G-hybrid: frame 저장 안 함, 이동은 사용자 드래그 delta 를 자손 좌표에 일괄 적용).
- ~~Pane ↔ Panel 매핑 cardinality~~ → 더 큰 amend (2026-05-15 multi-session pivot: Terminal:Panel = 1:N multi-session mirror, ADR-0021 D1).
- ~~tmux 어휘 vs PTY-직접 어휘 충돌~~ → 해소 (2026-05-14: ADR-0013 채택으로 tmux 어휘 영구 폐기).
- ~~재기동 후 Panel persistence vs Pane process lifetime 충돌~~ → 다른 결론으로 해소 (2026-05-15 multi-session pivot: schema v2 + match-or-spawn 으로 layout 보존, ADR-0018).
- ~~비정상 종료 후 child shell orphan 처리~~ → 해소 (2026-05-15: marker + boot-time best-effort reap, ADR-0014 D11).
- ~~MT-3 server-wide broadcast~~ → 해소 (2026-05-15 multi-session pivot: session-scoped state + server-wide terminal stream 의 2-layer 로 amend, ADR-0021 D5).

여전히 미해결 (ADR 결정 후 closure):
- **Cross-server session lock** — 동일 workspace path 의 두 server 가 같은 session 에 동시 attach 요청 시 처리 (ADR-0019 D6).
- **Auth password mode 의 보안 표면** — Argon2id hash + rate limit + rotate UI 의 구체 정책 (ADR-0020).
- **Asset storage** — image / document / file_path item 의 storage 정책 (P2+, ADR-0018 후속 또는 별 ADR).
- **Auto-load on attach 의 race** — 두 webpage 가 같은 비활성 session 을 거의 동시에 클릭할 때의 lock acquire 정합 (ADR-0019 D6 결정 후 명시).

## Group 운영 규칙 (G-hybrid 확정)

- **생성·해체**: Panel/Group 다중 선택(M) → `Group` 액션이 새 Group으로 묶음. Group 단일 선택 → `Ungroup` 액션이 해체하고 자식을 grandparent 또는 Canvas 루트로 재부모화. **빈 Group은 명시적 생성 경로 없음** (다중 선택 ≥ 1 필수). 자식이 모두 제거되어 발생한 빈 Group은 명시적 사용자 액션(Ungroup/Delete)으로만 제거 — auto-prune 안 함.
- **Drag-reparent UX**: 사이드바 layer panel 안 드래그로만 (MVP). 캔버스 hover 기반 reparent는 P1+ 검토.
- **상태 전파**: visibility와 lock의 전파 방식이 다르다.
  - **visibility = AND**: effective visible = self AND 모든 ancestor. 어떤 ancestor가 hidden이면 자손도 hidden (override 불가).
  - **lock = OR**: effective locked = self OR 모든 ancestor. 어떤 ancestor가 locked이면 자손도 locked (cascade-down lock). 자손은 ancestor 잠금을 풀 수 없음.
  - **label/color**: 자기 값이 있으면 그게 표시, 없으면 가장 가까운 ancestor 값 inherit.
- **Group → M 확장**: 사이드바에서 Group 클릭 = 그 Group의 모든 자손 Panel을 M에 등록 (재귀). 캔버스에서 Panel 클릭 = 그 Panel만 M (Group 자동 확장 없음).
- **Group close (destructive)**: 자손의 모든 Panel을 `kill-pane` 재귀 발급. §7.6 confirm modal 필수. Group 자체는 자손이 모두 사라진 후 명시 삭제 또는 ungroup으로 제거.
- **영속화 스키마 (2026-05-15 multi-session pivot 으로 schema v2 로 갱신)**: 한 Session file record 안에 `schema_version: 2` + `groups: [{id, parent_id|null, label, color|null, visibility, locked, order}]` + `items: [{id, type, parent_id|null, x, y, w, h, z, visibility, locked, label, description, minimized, ...type-specific payload...}]` + `viewport: {x, y, zoom}` + (optional) `last_M, last_I`. `items[].type` ∈ `terminal/text/note/rect/ellipse/line/free_draw/image/document/file_path`. ADR-0010 (groups) + ADR-0018 (items v2) 에서 확정. v1 → v2 hard cutover 는 ADR-0006 D15. **G20 grilling amend**: 옛 `maximized` schema field 제거 — FE-only ephemeral.
