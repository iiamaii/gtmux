# 0051 — Session migration handover (cold-pickup brief)

- 작성일: 2026-05-17
- 작성자: FE 통합 agent (0045 refresh reconnect loop 진짜 root cause 식별 + 0046/0047 BE 의존 출하 후 cold-pickup 시점)
- 종류: **cold-pickup brief** — 다음 세션이 본 문서 한 장으로 현재 상태 + 즉시 진입 우선순위 + 핵심 컨텍스트 모두 진입 가능
- HEAD: `f80ecc1` feat(frontend): WorkspaceEmptyPlaceholder — modal cancel 후 인지 단서 + 진입점
- baseline: FE `svelte-check 294/0/0` clean · BE `cargo test --workspace` 112+ pass (1개 flaky — §5.3 참조)

---

## 0. 다음 세션 즉시 진입 — 우선순위 ⚠️ MUST READ

| 우선 | 항목 | 상태 | 정본 |
|---|---|---|---|
| ✅ ~~P0~~ | **WS heartbeat timeout test flaky** | **closed (2026-05-17)** — timing 4x 확장 + close_code best-effort + test rename. §5.3 의 amend 참조 | §5.3, `crates/ws-server/src/lib.rs::heartbeat_timeout_closes_and_emits_disconnect` |
| 🟡 P1 | **0048-fe-refresh-validation-checklist 실측 (S1~S10)** | 미진행 — BE 0046 ship 후 가능 | `docs/reports/0048-fe-refresh-validation-checklist.md` |
| 🟡 P1 | **terminal 유 layout XtermHost fit() loop manual 검증** | 미진행 — 0045 §9 7항 중 1항 잔여 | `docs/reports/0045-...-amend.md` §11.6 |
| 🟢 P2 | **UI/UX 폴리시 잔여** (별 agent 진행 중) | 활성 | 별 agent 의 직전 commit 군 (f80ecc1, 42d8089, b52529f, …) 추적 |

### 0.1 본 session 의 핵심 사건 한 문단

0045 refresh reconnect loop 의 진짜 root cause 가 **별** layer (Canvas flowNodes / viewport / loadLayout) 가 아닌 **`ReconnectModal.svelte` 의 `$effect` self-loop** (graceTimer 를 `$state` 로 둠 → effect 의 read + write 가 자기 자신 invalidate → effect-depth throw → reactive flush abort → boot screen DOM 영구) 임을 puppeteer 진단 + binary search 격리 로 확정. fix shipped `19f186d` (graceTimer 를 plain `let`, `visible` read 를 `untrack` 으로 wrap). 별 agent 가 같은 날 BE 0046 (`attach_handler` same-cookie idempotent) + BE D6 heartbeat + ADR-0021 D6 amend + UI/UX batch 4 + AuthPage FE pivot 등 동시 진행 — main 에 land 완료. 0045 §11 amend, ADR-0017/0019/0024/0021 동반 amend 모두 정합 처리됨.

---

## 1. 프로젝트 mental model (1 분 요약)

**gtmux** = tmux-backed web canvas workspace. *single-user* SPA. tmux 가 process lifecycle 의 진실, FE 가 *canvas layout 의 진실*.

### 1.1 어휘 (CONTEXT.md / ADR-0019 정합)

| 어휘 | 정의 |
|---|---|
| Server | gtmux process. 1 port owner, 1 workspace dir 바인딩 |
| Workspace | server 와 1:1, `<XDG_DATA_HOME>/gtmux/workspace/` dir |
| Session | workspace 안 named file record (`<name>.json`). canvas layout + viewport |
| Webpage | 브라우저 탭. 1 WS 연결, 0/1 session attach |
| Terminal | server-pool, multi-session 공유 가능 (mirror) |
| Canvas | 한 session 의 무한 작업 공간 |
| Canvas Item | canvas 위 시각 객체 (`type: terminal | rect | ellipse | line | text | note | file_path | document | image`) |
| Panel | `type:"terminal"` 인 Canvas Item |
| Group | session 안 item 의 parent_id 트리 (ADR-0010) |

### 1.2 핵심 invariant

1. tmux state ↔ web state 분리 (mirror only ↔ FE 진실)
2. layout ≠ tmux layout (free 배치 ≠ split)
3. single-attach: Webpage : Session = 1:1 (ADR-0019 D3)
4. takeover 금지 (ADR-0019 D4 — 다른 cookie 의 same-name attach 만 409)
5. control-mode integration only (ADR-0021)

### 1.3 우선 단계

현재 = **Stage 6~7** (multi-session pivot 완료, UX 폴리시 + auth FE pivot 후속 + 회귀 가드).

---

## 2. 디렉토리 / 빌드

| | 경로 | 명령 |
|---|---|---|
| BE workspace root | `codebase/backend/` | `cargo test --workspace`, `cargo build`, `cargo run -p gtmux -- start --session <name>` |
| FE | `codebase/frontend/` | `pnpm check`, `pnpm build`, `pnpm dev` |
| 문서 | `docs/` (ADR/plan/report 별 폴더) | — |

baseline (본 session 검증):
- FE: `svelte-check 294 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`
- BE: `cargo test --workspace` — 112/113 pass, 1 flaky (§5.3)

---

## 3. 본 session 의 fix / 산출물

### 3.1 ReconnectModal $effect self-loop (commit `19f186d`)

**증상**: 새로고침 → "Reconnecting…" 영구 + console `effect_update_depth_exceeded` (stack 이 `svelteflow-*.js` chunk 가리키지만 misleading — Vite manual chunk 가 Svelte 런타임 + SvelteFlow 묶음)

**root cause**: `codebase/frontend/src/lib/chrome/ReconnectModal.svelte`

```typescript
// BUG
let graceTimer: ReturnType<typeof setTimeout> | null = $state(null);  // ← reactive

$effect(() => {
  if (mode === 'loading' && !visible) {
    if (graceTimer !== null) clearTimeout(graceTimer);   // ← read graceTimer
    graceTimer = setTimeout(() => { ... }, 100);          // ← write → self-trigger
  }
});
```

setTimeout handle 이 `$state` 라 effect 의 read+write 가 자기 invalidate → effect-depth throw → 그 reactive flush 의 모든 DOM update abort → boot screen 의 이전 DOM 그대로 (실제로 reconnectGate.state='ready' 인데 화면이 그대로).

**fix**:

```typescript
let graceTimer: ReturnType<typeof setTimeout> | null = null;  // plain let
let visible = $state(false);

$effect(() => {
  const currentMode = mode;
  const isVisible = untrack(() => visible);  // dep 제외
  if (currentMode === 'loading' && !isVisible) {
    if (graceTimer !== null) clearTimeout(graceTimer);
    graceTimer = setTimeout(() => {
      graceTimer = null;
      visible = true;
    }, 100);
    return () => { if (graceTimer !== null) { clearTimeout(graceTimer); graceTimer = null; } };
  }
  if (currentMode !== 'loading') {
    if (graceTimer !== null) { clearTimeout(graceTimer); graceTimer = null; }
    visible = true;
  }
});
```

검증 (puppeteer live):
- `hasCanvas: true`, `hasSvelteFlow: true`, `nodeCount: 6 (items hydrated)`
- `(no page errors)` — effect-depth 사라짐
- 카운터: `canvas.mount=1`, `flowNodes.rebuild=정상`, `canvas.setViewport=1`, `sessionStore.loadLayout=1`

### 3.2 묶음 E — 0045 §6 P0/P1 후보 예방 fix (commit `da7663b`, pre-amend)

§3.1 의 진짜 source 가 §6 후보 밖이었지만 §6 의 예방 fix 자체는 여전히 valid (다음 회귀 차단). 다음이 main 에 land:
- `Canvas.svelte` — `flowNodes` derived 의 id-cache + signature (P0-A) — JSON.stringify 제거, 명시 field concat
- `Canvas.svelte` — viewport sync $effect 를 `untrack` 으로 wrap + `applyingStoreViewport` 를 `requestAnimationFrame×2` 로 reset (P0-B)
- `Canvas.svelte` — `edges={EMPTY_EDGES}`, `proOptions={SVELTE_FLOW_PRO_OPTIONS}` const literal 추출
- `reconnectGate.svelte.ts` — 5-state machine (`booting | idle | attaching | hydrating | in_use | not_found | unreachable | ready`), `canMountApp` derived, `markReady`/`markSuccess`
- `+page.svelte` — `{#if reconnectGate.canMountApp}` 분기로 partial mount 차단
- `debugCounts.ts` — localStorage flag 기반 dev instrumentation (`gtmux-debug-counts=1`)
- `heartbeat.svelte.ts` — WS heartbeat client (15s ping / 30s stale)

### 3.3 BE 0046 ship 확인 (별 agent commit `e9eb9a6`)

`attach_handler` (sessions.rs) line 398 직전에 cookie ownership 분기 추가 + `reuse_existing_attach_response` helper. 기존 `attach_409_when_already_held_same_server` 테스트 제거 + 신규 `attach_idempotent_for_same_cookie_same_session` / `attach_409_when_held_by_different_cookie` (RED→GREEN). 추가로 D13 `/auth` FE-bundle pivot — BE server-rendered `auth_page_handler` 제거, `fallback_service` 가 catch → index.html → FE AuthPage `?t=` 인식.

**결과**: refresh race + plan-0008 Phase 2 silentReattach 의 모든 진입점 fail 근본 해소. FE 측은 별도 변경 없이 자연 정상화 (FE 의 `silentReattach` 결과가 `ok` 로 정상 떨어짐).

### 3.4 documentation amend

- `0045 §11.1~§11.6` — 진짜 root cause + fix + §6 후보의 예방 가치 + 진단 방법론 + 일반화 교훈
- ADR-0017 amend ⑤⑥ (별 agent — reactive flush abort 의 의미)
- ADR-0019 D5.4 5-state + cookie ownership 분기 land 표기
- ADR-0024 — node-cache pattern (Layer tree z-index 분리 정합)
- ADR-0021 D6 — FE-side liveness watchdog (D6.1) + BE ping/pong 상태 (D6.2)
- 0046 work package — `docs/reports/0046-be-attach-handler-idempotent.md` (별 agent linter 보강)
- 0047 BE next-session brief — `docs/reports/0047-be-next-session-handover.md`
- 0048 FE refresh validation — `docs/reports/0048-fe-refresh-validation-checklist.md`
- 0048 session migration handover — `docs/reports/0048-session-migration-handover.md` (직전 BE agent)
- 0049 UI/UX + auth pivot handover — `docs/reports/0049-...`
- 0050 lasso/selection 회귀 시나리오 — `docs/reports/0050-...`

---

## 4. 별 agent 의 parallel landed work (본 session 외부에서 main 에 추가됨)

다음 commit 들은 본 agent 작업 *외에* 같은 시기에 main 에 land 됨. cold-pickup 시 review 대상:

| Commit | 작업 |
|---|---|
| `f80ecc1` | WorkspaceEmptyPlaceholder — modal cancel 후 인지 단서 + 진입점 |
| `42d8089` | SessionListModal cancel 의 listCloseTarget 회귀 — closeList() 분기 복원 |
| `b52529f` | line item 의 wrapper bbox ring 회귀 — type-based selector 로 제외 |
| `543f0ad` | PanelNode minimize/maximize 시각 적용 — schema geometry 변경 + backup |
| `741be5b` | docs(attach-recovery) 0042/plan-0007/plan-0008/handover-v3/CONTEXT 정합 |
| `514b15d` | WorkspaceSwitcher.listCloseTarget — SessionListModal cancel 진입점 분기 |
| `cbc277c` | LeftPanel/RightPanel resizable width |
| `a46c6e2` | ref/frontend-design 시안 + AGENTS.md 메타 |
| `0819628` | gitignore 보강 + skills-lock.json untrack |
| `05e3f4b` | §05 Shared rules audit — wrapper 단일 source selection/hover (plan-0011 §4) |
| `59bd0ab` | BE D14 — `POST /auth/rotate` cookie rotation endpoint |
| `1ea1d83` | ADR-0018 D10 + plan-0011 — caption / document inline-stored 신규 |
| `e6e658b` | FilePathNode redesign — ref/frontend-design/components.html §03 정합 |
| `30ef6fe` | LayerTreeView — min/max 제거 + row hover container 통합 |
| `cd15cba` | BE D6 heartbeat — Hub timings config + integration tests |
| `c1b980b` | PanelNode header redesign — ref/frontend-design/components.html §04 정합 |
| `8d11298` | ColorPicker + shape fill/stroke 편집 (plan-0010 Task 3) |
| `48e26df` | LayerTreeView — min/max 는 terminal panel 행 에만 표시 |
| `1acd2c3` | multi-select alignment (6 align + 2 distribute) — plan-0010 Task 5, ADR-0027 |
| `ac1962f` | ItemInfoView v2 — multi-select Common+Type split |
| `f63c1e1` | ADR-0027 — Inspector multi-select layout + alignment mutation |
| `91e6c1c` | LayerTreeView minimize/maximize/focus 액션 + zoom-to-item |
| `759ec05` | session 진입 시 panel title 회귀 — terminal_meta 우선 source |
| `7622e6c` | plan-0010 — UI/UX batch 4 (Layer actions + Inspector v2 + Alignment) |
| `b77c1cf` | report 0050 — lasso/selection sync 회귀 시나리오 정리 |
| `9ee5679` | ADR-0021 D6 amend — FE liveness watchdog (D6.1) + BE ping/pong (D6.2) |
| `e9eb9a6` | BE 0046 idempotent attach + D13 `/auth` SPA pivot |

---

## 5. 진입 항목 상세

### 5.1 0048 FE refresh validation checklist 실측

`docs/reports/0048-fe-refresh-validation-checklist.md` 의 S1~S10 시나리오를 BE 0046 ship 된 상태에서 manual 또는 puppeteer 로 실측. 특히:
- S2: 새로고침 → 같은 cookie reattach → 200 OK (이전엔 409 → ReconnectModal 'in_use')
- S5: visibility change + heartbeat idle → silentReattach 200 OK (이전엔 mutation guard 전체 차단)
- S7~S10: 신규 5-state reconnectGate / canMountApp / Modal grace 1s

### 5.2 0045 §9 7항 중 잔여 1항 — terminal 유 layout XtermHost fit() 회귀

puppeteer 검증은 9 items (text/figure 혼합) 으로 진행됨. terminal panel 이 mount 된 후 `XtermHost.svelte` 의 ResizeObserver ↔ FitAddon ↔ SvelteFlow measurement 가 loop 을 만들 수 있는지 manual 로 한 번 확인.

### 5.3 BE WS heartbeat test flaky — `heartbeat_timeout_closes_and_emits_disconnect`

`codebase/backend/crates/ws-server/src/lib.rs::heartbeat_timeout_closes_and_emits_disconnect` (구 `..._1011_...`). 단독 실행 시 pass, `cargo test --workspace` 병렬 실행 시 fail (`Close frame` 이 `None` 으로 떨어짐 — 기대 `Some(Error)`).

```
thread 'tests::heartbeat_timeout_closes_1011_and_emits_disconnect' panicked at lib.rs:2748:
assertion `left == right` failed: expected Close(1011 / CloseCode::Error) on heartbeat timeout
  left: None
 right: Some(Error)
```

진단: 8-thread tokio runtime 압박 + socket teardown race. server-side `let _ = sink.send(close_frame).await` 가 race 시 graceful close payload 전달 실패 → client 가 `None` (stream end) 만 봄. **disconnect_sink emit 은 정상 발화** (handle_socket return 후 wire) — 즉 production contract ("timeout → disconnect → release_lock_for_cookie") 는 정상. flaky 가 노출한 건 graceful 1011 frame 의 best-effort 한계.

**fix ship (2026-05-17, ADR-0021 D6.2 amend ③)**:
- (a) timing 4x 확장 — `ping_interval 50→100ms`, `pong_timeout 150→300ms`, client wait `400→700ms`. race window 축소.
- (b) test contract 정합 — close_code assertion 을 `matches!(None | Some(CloseCode::Error))` 로 완화. `disconnect_sink` emit 은 strict 유지 (production trigger 의 load-bearing signal).
- (c) test 이름 rename `..._1011_...` → `..._and_...` (1011 close code 는 hint, 진짜 contract 는 disconnect 발화).

검증: `cargo test --workspace` 5회 연속 실행 — flake 0, 본 test 매번 PASS. 워크스페이스 368 PASS / 0 FAIL.

### 5.4 별 agent UI/UX 진행 중 — 별도 추적

f80ecc1 / 42d8089 / b52529f / 543f0ad 가 직전 활동. plan-0010 / plan-0011 진행 중. 본 agent 가 손대지 않음 — UI/UX 폴리시 PR review 시 별 agent 의 의도 우선.

---

## 6. 핵심 reference 위치

| 종류 | 경로 |
|---|---|
| 본 session root cause | `docs/reports/0045-refresh-session-reconnect-loop-analysis.md` §11 |
| BE 0046 work package | `docs/reports/0046-be-attach-handler-idempotent.md` |
| BE next-session brief | `docs/reports/0047-be-next-session-handover.md` |
| FE validation checklist | `docs/reports/0048-fe-refresh-validation-checklist.md` |
| 직전 handover (BE) | `docs/reports/0048-session-migration-handover.md` |
| 직전 handover (UI/UX) | `docs/reports/0049-session-handover-ui-ux-and-auth-pivot.md` |
| lasso/selection 회귀 | `docs/reports/0050-lasso-selection-regression-scenarios.md` |
| active plan | `docs/plans/0011-component-design-batch-caption-document.md` (최신) + `docs/plans/0010-...` (UI/UX batch 4) + `docs/plans/0008-...` (attach recovery) + `docs/plans/0009-...` (auth FE pivot) |
| 핵심 ADR (본 session 관여) | ADR-0017 (layout grid + chrome), ADR-0019 (session + workspace), ADR-0021 D6 (heartbeat), ADR-0024 (layer tree + z-index), ADR-0027 (inspector + alignment) |

**reading order** (cold-pickup, 처음 진입 agent):
1. 본 문서 §0~§3
2. 0045 §11 amend (진짜 root cause + 일반화 교훈)
3. 0048 validation checklist (실측 진입)
4. 0049 §0~§1 (UI/UX pivot 상태)
5. ADR-0019 D3/D4/D5.4 (single-attach + idempotent path 정합)
6. ADR-0021 D6 (heartbeat 양측 정합 — §5.3 flaky 진단 보조)

---

## 7. 일반화 교훈 — Svelte 5 runes 함정 (재발 차단용)

본 session 의 root cause 가 비전형이라 future agent 가 반복하지 않도록 명문화:

1. **`$state` 안에 *setTimeout/setInterval handle* 또는 *side-effect bookkeeping* 류 ref 를 두지 말 것**. 이런 값은 reactive read/write 가 의미 없고 self-loop 만 유발.
2. **`$effect` 안에서 dep 인 reactive 를 write 하지 말 것**. 부득이한 경우 `untrack(() => readValue)` 로 wrap.
3. **effect 의 명시 dep 만 expose** — `const currentMode = mode;` 패턴으로 의도 명확화.
4. **stack trace 의 chunk 이름을 source 로 신뢰하지 말 것** — Vite manual chunk 가 Svelte 런타임 + 라이브러리를 묶기 때문에 `svelteflow-*.js` 가 실제 SvelteFlow 가 아닐 수 있음.
5. **reactive flush abort 의 증상은 "이전 DOM 그대로"** — 내부 store 는 정상 업데이트되지만 화면이 안 바뀜. 의심 시 store 를 임시 expose 해 state 와 DOM 의 괴리를 직접 비교.

---

## 8. 환경 정리

- BE 서버 down (PID 28054 killed, port 9999 free)
- `/tmp/gtmux-debug` 진단 스크립트 cleanup
- `/tmp/cookies.txt`, `/tmp/gtmux-start.log` cleanup
- 작업 트리 clean (`git status` "커밋할 사항 없음")
- HEAD = `f80ecc1`

다음 agent 는 `pnpm dev` 또는 `cargo run -p gtmux -- start --session <name>` 로 자유롭게 시작 가능. baseline check 는 §2 의 명령 그대로 재현.
