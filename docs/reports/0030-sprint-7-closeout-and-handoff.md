# 세션 핸드오프 — Sprint 7 종합 closeout (Stage A~J 완료) + 다음 단계 (테스트 / 디버깅 / 안정화 / 리뷰)

본 문서는 `0027-session-resume-handoff.md` + `0028-s7-persistence-minimal-closeout.md` + `0029-frontend-design-ref-analysis.md` 의 후속이며, **콘텍스트 초기화 후 cold 픽업** 을 위한 *self-contained* 핸드오프다. 0027/0028/0029 는 historical snapshot 이며, 본 문서가 *지금 시점에 새 세션에 합류하는 agent 가 읽어야 할 단일 진실*.

새 세션은 다음 7 문서만 읽으면 작업 재개 가능:
1. `CLAUDE.md` (프로젝트 메타, EN)
2. `CONTEXT.md` (도메인 어휘 — 2026-05-14 amend ×2 반영)
3. **본 문서** (`0030-sprint-7-closeout-and-handoff.md`)
4. `docs/adr/0013-pty-direct-no-tmux.md` (canonical 아키텍처, 2026-05-15 amend ×4)
5. `docs/adr/0016-design-tokens-and-iconography.md` (Figma tokens + lucide, 2026-05-15 amend ×1)
6. `docs/adr/0017-layout-grid-and-chrome.md` (6 영역 layout grid + chrome 책임)
7. `docs/plans/0005-figma-layout-overhaul.md` (Stage A~K 마스터 로드맵 — §9 가 완료 stages 의 진실)

---

## 0. 한 줄 상태

- **현 시점**: Sprint 7 의 모든 핵심 backend / frontend 작업 완료. UI/UX 마스터 로드맵 (plan 0005) 의 Stage A~J **모두 완료**. Stage D (Toolbar2 — Select/Hand 도구) + Stage K (본 closeout) 가 잔여.
- **빌드/테스트**: cargo test 164 PASS · clippy clean · fmt clean · svelte-check 0/0 · vite build OK. 차단성 갭 0건.
- **Server 상태**: 9999 port 에서 listening (`pid 71064`, release binary). `/healthz` OK. PtyBackend supervisor.
- **다음 단계**: 본 §8 — **테스트 인프라 구축 (vitest 도입) · 디버깅 사이클 · 안정화 (S7-DEMO-STAB) · review (code-review-graph MCP + ultrareview)**.

---

## 1. Sprint 7 본 세션 (2026-05-14 → 05-15) commits 시간순

| commit | 작업 |
|---|---|
| (earlier) `b49a16e` | S7-PERSISTENCE-MINIMAL backend (ADR-0006 D1~D13) |
| `3e99cba` | 0028 closeout (S7-PERSISTENCE-MINIMAL) |
| `0f94a97` | 0002 demo prep — §15 1~3 단계 시연 패키지 |
| `a00c041` | 0003 plan — S7 lifecycle UI 구현 계획 |
| `91aa084` | 0004 plan — UI/UX 총체적 설계 10 phase |
| `5b76510` | Phase 0+1 v1 — design tokens + UI primitives (pre-Figma pivot) |
| `144198c` | 0029 frontend design ref 분석 — Figma adaptation pivot |
| `1984649` | **Stage A** — Figma token migration + ADR-0016 amend + plan 0005 |
| `3305f8d` | **Stage B** — theme store + FOUC guard |
| `cf88b9a` | visible chrome token refactor + ThemeToggle 임시 마운트 |
| `d200cbb` | fix — global.css 미import + Toolbar 56px + zoom placeholder 해소 |
| `4131313` | 사용자 피드백 #1-#5 — canvas theme + single/multi select + panel resize + layer rows |
| `d104c70` | **Stage C + J** — Titlebar + SessionMenu + ShutdownModal + Backend KillSession |
| `2c9550e` | **Stage E** — Sidebar floating + PaneInfoPanel + RailToggle ×2 |
| `7242a29` | **Stage F** — Canvas chrome (HelpBar + ViewportCtrl + ContextMenu) |
| `7cf8bab` | **Stage G** — PanelNode close button (S7-FE-CLOSE-GUARD) |
| `9a2bda8` | **Stage I** — S7-FE-AUTOMOUNT + ADR-0015 신규 |

net LOC delta (Stage A~I + closeout 포함): ≈ **+3,200 LOC** frontend + **+150 LOC** backend + **+1,600 LOC** docs (ADR 0015/0016 amend/0017 + plan 0003/0004/0005 + reports 0028/0029/0030).

---

## 2. 핵심 결정 요약

### 2.1 ADR 매트릭스 (2026-05-15 기준)

| ADR | 제목 | 상태 |
|---|---|---|
| 0001 | tmux 통합 = control mode | **Deprecated** (0013 supersede) |
| 0002 | 전송 = WebSocket + 이진 envelope | Accepted (amend ×2) |
| 0003 | 보안 디폴트 | Accepted (amend ×2) |
| 0004 | xterm.js v6 | Accepted |
| 0005 | @xyflow/svelte | Accepted |
| 0006 | persistence = JSON + atomic-write | Accepted (S7 에서 implement) |
| 0007 | 1:1:1 Server:Session:Port | Accepted (amend ×1) |
| 0008 | single-pane + Group | Accepted (amend ×1) |
| 0009 | tmux daemon | **Deprecated** (0014 supersede) |
| 0010 | Group 데이터 모델 | Accepted |
| 0011 | Rust + axum | Accepted |
| 0012 | Svelte 5 + Vite | Accepted |
| **0013** | PTY direct, no tmux | Accepted (**amend ×4** — 2026-05-15 KillSession variant) |
| **0014** | Process supervisor | Accepted (amend ×1) |
| **0015** | **Pane auto-mount = frontend cascade** | **신규 Accepted (2026-05-15, Stage I)** |
| **0016** | **Design tokens + lucide-svelte** | **신규 Accepted (2026-05-15, amend ×1 — Figma) — Stage A** |
| **0017** | **Layout grid + chrome 책임** | **신규 Accepted (2026-05-15, Stage C)** |

### 2.2 plan 0005 Stage 진척 (UI/UX 마스터 로드맵)

| Stage | 제목 | 상태 | 핵심 산출물 |
|---|---|---|---|
| **A** | Token v2 + ADR-0016 amend | ✅ | tokens.css 재작성 (light/dark + Figma accent), primitives 토큰 rename |
| **B** | Theme store + FOUC | ✅ | `$lib/stores/theme.svelte.ts`, index.html inline FOUC guard, ThemeToggle |
| **C** | Titlebar + Menu + Modal + Toggles | ✅ | `$lib/chrome/{Titlebar,SessionMenu,ShutdownModal,FocusToggle}.svelte` |
| D | Toolbar2 (Select/Hand) | ⏳ 잔여 | (선택적, Stage K 검토에서 D 도구 채택 여부 결정) |
| **E** | Sidebar floating + PaneInfoPanel | ✅ | `$lib/chrome/{PaneInfoPanel,RailToggle}.svelte`, `$lib/stores/chrome.svelte.ts`, Sidebar refactor |
| **F** | HelpBar + ViewportCtrl + ContextMenu | ✅ | `$lib/chrome/{HelpBar,ViewportCtrl,ContextMenu}.svelte`, SvelteFlowProvider wrap |
| **G** | PanelNode close (S7-FE-CLOSE-GUARD) | ✅ | PanelNode 헤더 X + last-pane disabled guard + CTRL kill-pane |
| **H** | +page.svelte 6-area grid | ✅ | Stage E 안에 흡수 (workspace=absolute overlay host) |
| **I** | S7-FE-AUTOMOUNT | ✅ | ADR-0015 + `appendPanelIfMissing` + dispatcher hook |
| **J** | Backend KillSession | ✅ | `BackendCommand::KillSession` + cmd_router + SIGTERM self |
| **K** | 검증 + 시연 + closeout | ✅ (본 문서) | cargo test 164 PASS, svelte-check 0/0, 0030 closeout |

### 2.3 사용자 피드백 4건 반영

본 세션 진행 중 사용자 피드백 → 즉시 흡수:

| 피드백 | 응답 |
|---|---|
| "화면 변화 없음" | 진단: `global.css` orphan (어디에도 import 안 됨) + Toolbar 56px 의도치 않은 적용 + zoom placeholder. `main.ts` 에 import 추가 + Toolbar 40px stopgap + isAtUnitZoom=true (`d200cbb`). |
| 5건 (canvas 색 / select 모드 / resize / layer 스타일) | `4131313` — canvas Background token화 / Cmd+click multi-select / NodeResizer / Sidebar row Figma 스타일 |
| 반응형 + layer 아이콘 + Stage C | Stage C + J + 반응형 media query + emoji → 인라인 SVG icons (`d104c70`) |
| Stage E / F / G / I 순차 진행 | 각 stage 별 commit + plan 0005 §9 업데이트 |

---

## 3. 신규 어휘 (CONTEXT.md 정합)

본 Sprint 7 에서 도입된 / 정착된 어휘 — 이후 코드/문서 일관성:

| 어휘 | 정의 |
|---|---|
| **Figma adaptation** | ref/frontend-design/ 의 Figma-inspired canvas editor design language 를 gtmux 도메인에 매핑한 결과 (ADR-0017) — 1920×1080 fixed-frame 은 폐기, design language (floating panels, dashed focus, accent #0d99ff) 채택 |
| **Chrome 컴포넌트** | `$lib/chrome/` 디렉터리의 시각 chrome — Titlebar / SessionMenu / ShutdownModal / Theme/FocusToggle / PaneInfoPanel / RailToggle / HelpBar / ViewportCtrl / ContextMenu. *기능적* 컴포넌트 (Canvas, Sidebar, PanelNode) 와 구분 |
| **floating panel** | `position: absolute` overlay, 8px gap + radius-lg + shadow-md. Sidebar / PaneInfoPanel 이 본 패턴 채택 (ADR-0017 §D2/D7) |
| **chrome store** | `$lib/stores/chrome.svelte.ts` — sidebarCollapsed + paneInfoCollapsed boolean + localStorage 영속 (`gtmux-chrome` 키) |
| **theme store** | `$lib/stores/theme.svelte.ts` — light/dark + localStorage 영속 (`gtmux-theme` 키) + html.dark 클래스 sync. Initial priority: localStorage → prefers-color-scheme → dark fallback |
| **FOUC guard** | `index.html` 의 inline script — Svelte hydrate 전에 html.dark 적용해 첫 paint 가 정확한 theme |
| **auto-mount** | dispatcher 의 `pane-spawned` NOTIFY hook 이 layout 에 없는 pane 발견 시 cascade PUT 으로 자동 추가 (ADR-0015) |
| **two-path race** | NewPanelButton path (viewport-center 좌표) + dispatcher hook (cascade 좌표) 가 같은 paneId 로 동시 PUT 시도 — `appendPanelIfMissing` 의 idempotent 가드가 처리 (ADR-0015 D3/D4) |
| **kill-session** | CTRL cmd allowlist 신규 entry. backend ack 후 `libc::raise(SIGTERM)` self → axum graceful_shutdown (ADR-0013 D10 amend ×4) |
| **CtrlOutcome::OkAndExit** | ws-server cmd_router 의 신규 outcome — `OkAndExit` 시 ack 인코딩 + SIGTERM self |
| **Pane Info** | 우측 268px floating panel 의 명칭 — Figma 의 Design 탭에 대응되는 gtmux 변형. 선택된 Panel 의 Identity / Geometry / State 표시 (Stage E) |
| **session 이름 surface** | `sessionStorage.gtmux_session` — `/auth/bootstrap` 의 inline script 가 token 과 함께 주입. Titlebar 가 표시 (ADR-0017 §D4) |

---

## 4. 현재 코드 상태 + git

### 4.1 git 브랜치

```
* main           9a2bda8 feat(frontend): Stage I — S7-FE-AUTOMOUNT + ADR-0015 신규
  poc/pty-direct c637c39 poc(pty-direct): throwaway PTY-direct experiment, no tmux
```

### 4.2 빌드 / 테스트 / 린트

| 검증 | 결과 |
|---|---|
| `cargo test --workspace --tests` | **164 PASS** / 0 FAIL / 0 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `svelte-check` | **0 errors / 0 warnings** (246 files) |
| `vite build` | OK. CSS 49.57 KB / JS 60.52 KB (main bundle, gzip 8.49 + 18.83) |

### 4.3 LOC 합계 (본 closeout 시점)

| 영역 | LOC |
|---|---|
| backend (crates + bin) | ~6,800 (S7-PERSISTENCE-MINIMAL + S7-J 누적) |
| frontend (src) | ~5,900 (Figma adaptation + chrome 컴포넌트 +1,500 LOC) |
| docs (ADR + plans + reports + ssot) | ~12,500 (0005 plan 단독 ~750 + ADR 0015/0016/0017 ~1,000) |

### 4.4 server smoke (port 9999)

| endpoint | 응답 |
|---|---|
| `GET /healthz` | `{"ok":true}` |
| `GET /api/layout` | 200 + ETag + JSON (영속화 layout 진실) |
| `GET /` | SPA index.html (1.65 KB + FOUC inline script) |
| WS `/ws` | 인증 통과 시 핸드셰이크 OK, 외 1008 |

---

## 5. 시연 절차 (`docs/demo/0002-demo-prep-stage3.md` + Sprint 7 추가)

브라우저 강제 새로고침 (Cmd-Shift-R) 후:

### 5.1 Stage A~B (visual foundation)
- **Titlebar 44px** chrome — gtmux · `demo` · 127.0.0.1:9999 · Local
- 우상단 ☀/☾ 클릭 → light/dark 즉시 전환. localStorage 영속.
- dashed accent (`#0d99ff`) focus ring — keyboard `Tab` 으로 확인

### 5.2 Stage C (Session UI)
- Titlebar 좌측 ≡ 클릭 → SessionMenu dropdown (Session shutdown / Rotate token / About)
- **Session shutdown** → ShutdownModal (활성 pane 수 + session 이름 + 3-bullet) → Shutdown 클릭 → backend graceful exit 6 → ReconnectBanner 의 "Session ended" 분기

### 5.3 Stage E (floating panels)
- 좌측 248px Layer panel (floating, radius + shadow)
- 우측 268px Pane Info panel ("No selection" empty state)
- 좌/우 RailToggle (16×64 ◀/▶) — 클릭 시 패널 슬라이드 + chevron 회전 + viewport 가장자리로 이동. localStorage 영속.

### 5.4 Stage F (canvas chrome)
- 상단 중앙 HelpBar pill — `Space + drag · pan | ⌘ + scroll · zoom | right-click · menu`
- 하단 중앙 ViewportCtrl pill — `[−] [N%] [+] · [⊟] · M:N` + 100% 클릭 시 zoom reset
- 우클릭 → ContextMenu (Copy pane_id / Close pane / Hide / Lock) + 좌표 클램프

### 5.5 Stage G + I (panel CRUD)
- New Panel 클릭 → CTRL new-pane → backend spawn → frontend auto-mount cascade → panel 표시 + Pane Info 갱신
- 패널 헤더 X → backend child SIGTERM + layout 에서 panel 제거 + Pane Info empty 복귀
- 마지막 1개 남으면 X disabled + tooltip "Last live pane — use Session shutdown"
- 다중 패널 클릭: 단일 click = single (1.5px solid accent outline), Cmd/Ctrl/Shift+click = multi (2px dashed)

### 5.6 다중 탭 (auto-mount + LAYOUT_CHANGED broadcast)
- 같은 URL 의 2 탭 동시 열기 → 한쪽의 New Panel / drag / X close 가 즉시 다른 탭에 반영

### 5.7 영속화 + 재기동 (S7-PERSISTENCE-MINIMAL)
- 패널 배치 + drag + resize
- `gtmux stop --session demo` + `gtmux start --session demo --port 9999`
- 새 token URL 로 재진입 → 같은 panel 배치 복원 + chrome 상태 (theme, collapsed) 도 localStorage 로 복원

---

## 6. Risk register

### 6.1 알려진 잔여 risks (Stage K 진입 시점)

| 카테고리 | risk | 완화 |
|---|---|---|
| Race | NewPanelButton 의 viewport-center 좌표가 dispatcher hook 의 cascade 좌표로 *유실* 가능 (ADR-0015 D3) | 안정화 단계에서 실 빈도 측정 + 필요 시 NewPanelButton 의 PUT 우선 정책 추가 |
| Test infra | Frontend 단위 테스트 부재 (vitest 미설치). svelte-check + vite build 만 통과 | §8.1 — vitest 도입 + primitive / chrome / store unit tests |
| Demo | PTY backend 위 *처음으로 실 사용자 demo* — 새 부류 결함 발견 가능성 | §8.3 — S7-DEMO-STAB 안에서 Sprint 5 17건 결함 회귀 검증 |
| Multiplex | Hub 의 multiplexed pane_output 50 pane × 5 burst backpressure 미실측 | §8.3 |
| SvelteFlow integration | NodeResizer + selection + viewport 가 multi-touch 환경에서 동작 검증 안 됨 | §8.3 |
| KillSession edge case | macOS 의 `libc::raise(SIGTERM)` 후 axum graceful 5초 내 마무리 — 50 pane × 200ms grace 가 실제로 ≤ 200ms 병렬 처리됨을 검증 안 함 | §8.3 |
| UI 빈 패널 | 다중 PanelNode 의 X 동시 click 시 race (closing flag 만 보호 — 컴포넌트 unmount race 추가 가능) | §8.2 |

### 6.2 미해결 (Open)

- **O1**: dispatcher 의 hook 이 dispatcher 의 책임 경계 확장 — 단순 fan-out 이상의 layout PUT 책임. 별도 *dispatch responsibility* ADR 필요 여부?
- **O2**: ThemeToggle 의 dark/light 전환 시 xterm theme 도 변경되어야 하나, xterm 의 ITheme 은 현재 hardcoded dark. light theme 시 xterm 가독성 검증 필요.
- **O3**: Stage D (Toolbar2 — Select/Hand 도구) 의 필요성 — 현재 가능한 캔버스 액션이 SvelteFlow 의 기본 (drag/pan/zoom) + ContextMenu 의 액션으로 *충분* 한지 별도 검증 필요.
- **O4**: `kbd` 단축키 (Cmd-N / Cmd-Shift-Q / Cmd-Shift-L) 의 전역 등록 — ADR-0017 §D6 에 spec, 별도 keyboard shortcut 시스템 phase.

---

## 7. 메모리 정합

본 Sprint 7 의 *architectural pivot* 후속:
- `~/.claude/projects/-Users-ws-Desktop-projects-gtmux/memory/project_gtmux.md` — gtmux 의 backend = PTY direct (2026-05-14 amend ×2). 본 closeout 으로 추가 변경 *없음* — Sprint 7 의 UI 작업은 *frontend chrome 의 진화* 라 메모리 갱신 불요.

---

## 8. 다음 단계 (테스트 / 디버깅 / 안정화 / 리뷰)

본 §8 은 *cold pickup* 직후의 작업 진입을 위한 가이드. 4 트랙 병렬 가능.

### 8.1 테스트 인프라 구축

**현 상태**:
- backend: cargo test 164 PASS — 충실
- frontend: **vitest 미설치**, primitive / chrome / store 단위 테스트 **0 건**. 컴파일 + 타입 검증만 (svelte-check)

**진입 task**:
- **task 8.1.1** (1일): `vitest` + `@testing-library/svelte` 도입
  - `npm install -D vitest @testing-library/svelte @testing-library/jest-dom jsdom`
  - `vitest.config.ts` 설정 (jsdom env, alias `$lib`)
  - `package.json` 의 `scripts` 에 `test: vitest`, `test:run: vitest run` 추가
  - 첫 smoke test — `src/lib/stores/theme.svelte.test.ts` (resolveInitial 의 3 priority chain 검증)
- **task 8.1.2** (1일): primitive unit tests
  - Button / IconButton / Tooltip / Dropdown / Modal / Banner / Toast / Input — 각 1~3 case (render + click + a11y attrs)
- **task 8.1.3** (1일): store unit tests
  - theme.svelte.ts — initial / set / toggle / apply + localStorage 영속 검증
  - chrome.svelte.ts — toggleSidebar / togglePaneInfo + persist
  - panels.svelte.ts — movePanel / resizePanel / removePanel idempotency
  - mux.svelte.ts — addPane / killPane idempotency
- **task 8.1.4** (1~2일): chrome 컴포넌트 통합 테스트
  - Titlebar / SessionMenu / ShutdownModal — 사용자 클릭 흐름 (open → confirm → close)
  - PanelNode — close button disabled state + click handler
  - ContextMenu — open at coords + clamp + close on Esc
- **task 8.1.5** (별도 phase, 1~3일): E2E — Playwright
  - sprint 7 의 demo 시나리오 8 step 을 Playwright 으로 자동화
  - golden screenshot 도입 시 light/dark 양 theme 의 시각 회귀 검증

**예상 LOC**: vitest 인프라 +200 LOC, primitive tests +800 LOC, store tests +400 LOC, chrome tests +600 LOC, Playwright (별 phase) +1,000 LOC.

### 8.2 디버깅 사이클 (alert 가 있을 때만 진입)

본 closeout 시점에 *알려진 bug 0건*. 진입 시 다음 우선순위:

1. **race / timing bug**: dispatcher hook race / two-path PUT race / NodeResizer + drag 동시 race
2. **시각 회귀**: light theme 의 xterm 가독성, dashed focus ring 의 어지러움, sidebar 좁은 viewport collapse
3. **WS reconnect**: server 강제 재기동 시 ReconnectBanner 의 close code 분기 정확성
4. **layout PUT 412 race**: 다중 탭에서 동시 drag + resize 시 412 처리

**진입 task**:
- **task 8.2.1** — `investigate` skill 사용. 현상 → hypothesis → 검증 → fix → 회귀 테스트 추가 (위 §8.1 의 vitest 셋업 후 흐름이 깔끔).

### 8.3 안정화 (S7-DEMO-STAB)

**현 상태**: 0028 closeout §5.2 의 dispatch prompt 가 존재. PTY backend 위 *처음 실 사용자 부하 테스트*. Sprint 5 17건 결함 부류 회귀 가능성 인지.

**진입 task** (3~7일):
- **task 8.3.1** (1일): smoke 스크립트 갱신 — 현 `codebase/smoke/01_engine_connect.sh` 는 pre-Sprint-0 stale. 본 Sprint 7 상태로 rewrite (Stage A~J 의 시나리오 8 step 자동화).
- **task 8.3.2** (1~2일): xterm 호환성 매트릭스 — vim / htop / less / man / nvim / tmux (안에서 안 됨, 외부) / ssh + 컬러 / alt-screen / cursor 위치 / OSC sequence. Sprint 5 의 0020 결함 17건 회귀 매트릭스 적용.
- **task 8.3.3** (1일): backpressure 측정 — 50 pane × 5 burst output 시 ws-server 의 broadcast cap (512) 도달 빈도 + frontend 의 xterm write 지연. ADR-0013 D3 의 layered late-mount 검증.
- **task 8.3.4** (1일): 다중 탭 동기화 — Sprint 7 의 auto-mount + LAYOUT_CHANGED broadcast 의 실 환경 race 측정. ADR-0015 D3 의 좌표 race 빈도.
- **task 8.3.5** (1일): KillSession graceful — 50 pane × 200ms SIGTERM grace 의 실제 지속 시간. 1초 내 마무리 인지 검증.

### 8.4 리뷰

**진입 옵션**:

- **task 8.4.1** — **`code-review-graph` MCP**:
  ```
  mcp__code-review-graph__detect_changes_tool({ repoPath: <repo> })
  mcp__code-review-graph__get_review_context_tool(...)
  ```
  Sprint 7 의 26개 commit 의 risk score + 변경 영향 분석. 다음 우선 검토:
  - chrome 컴포넌트 (9건) 의 props interface 일관성
  - dispatcher / http-api / ws-server 의 hook coupling
  - ADR-0015 D3 race 정책의 코드 정합

- **task 8.4.2** — **`/ultrareview`** (사용자 트리거 — agent 가 직접 실행 불가):
  - main branch 위에서 전체 multi-agent cloud review
  - 시간/비용 발생 — 사용자 결정

- **task 8.4.3** — **`code-review` skill** (claude side):
  - 본 closeout 직전 commit (`9a2bda8`) 까지의 PR 형태 가상 review
  - 우선순위: ADR 정합 / 보안 / a11y / 토큰 정합 / dead code

- **task 8.4.4** — **a11y audit**:
  - WCAG AA contrast 4.5:1 — light/dark 양 theme
  - focus order — Tab 순회 가 Titlebar → Toolbar → Sidebar → Canvas → PaneInfo 자연 흐름인지
  - aria-live — banner / toast / modal 의 announcement

- **task 8.4.5** — **`security-audit` skill**:
  - WS subprotocol 인증 통과 surface
  - bootstrap landing 의 inline script — `</` escape 검증
  - layout PUT 의 schema 검증 + ETag race

---

## 9. 사용자 작업 룰 (메모리 정합)

새 세션 agent 가 사용자와 협업할 때:

1. **기술 디테일 결정** → brief + 진행 (confirm 묻지 않음). 예: 변수명, 파일 위치, 빌드 명령.
2. **도메인 / UX / 정책** → 옵션 비교 + 확인. 예: 새 ADR 명, UI 액션 배치, 보안 정책.
3. **Docs = KO, code = EN**. README/CLAUDE.md/repo-meta = EN.
4. **ADR-before-code** — 새 결정은 ADR 먼저, 그 다음 코드.
5. **Grilling 패턴** — 사용자가 "옵션 비교 + 추천" 을 가장 선호. AskUserQuestion 으로 한 번에 한 결정.
6. **TaskCreate / TaskUpdate** — 작업 진입 시 task tracker 활용. 본 §8 의 4 트랙은 각각 별 task 로 분할.

---

## 10. 다음 세션 첫 메시지 가이드

| 사용자 메시지 | 행동 |
|---|---|
| "테스트 인프라 진행" / "vitest 도입" | §8.1.1 — vitest + @testing-library/svelte 셋업 부터 |
| "primitive unit tests" | §8.1.2 — Button / Modal / Dropdown 등 first batch |
| "store tests" | §8.1.3 — theme / chrome / panels / mux |
| "E2E Playwright" | §8.1.5 — 별도 phase |
| "안정화" / "S7-DEMO-STAB" | §8.3 — smoke 스크립트 갱신부터 |
| "리뷰" / "code review" | §8.4 — code-review-graph MCP 또는 code-review skill |
| "Stage D 도구" / "Toolbar2" | §8 잔여 — Toolbar2 의 필요성 §6.2 O3 결정 후 진입 |
| "현 상태 점검" | 본 §4 의 빌드/테스트 명령 재실행 |
| "ADR 목록" | 본 §2.1 |
| "어떤 결정이 있었어?" / "Sprint 7 가 뭐였어?" | 본 §1 + §2 + §3 |

---

## 11. Reading list (우선순위 순)

### 11.1 Tier 1 — 새 세션이 반드시 읽어야

1. `CLAUDE.md`
2. `CONTEXT.md`
3. **본 문서** (`0030-sprint-7-closeout-and-handoff.md`)
4. `docs/adr/0013-pty-direct-no-tmux.md` (amend ×4)
5. `docs/adr/0014-process-supervisor.md` (amend ×1)
6. `docs/adr/0016-design-tokens-and-iconography.md` (amend ×1)
7. `docs/adr/0017-layout-grid-and-chrome.md` (신규)
8. `docs/plans/0005-figma-layout-overhaul.md` §9 (완료 stages 진실)

### 11.2 Tier 2 — 작업 시작 전 권장

9. `docs/adr/0015-pane-auto-mount.md` (Stage I)
10. `docs/adr/0006-persistence-storage.md` (S7-PERSISTENCE-MINIMAL)
11. `docs/reports/0028-s7-persistence-minimal-closeout.md` (영속화 closeout)
12. `docs/reports/0029-frontend-design-ref-analysis.md` (Figma adaptation 분석)
13. `codebase/frontend/src/lib/chrome/` — 9 chrome 컴포넌트 정본
14. `codebase/frontend/src/lib/stores/` — theme / chrome / panels / mux

### 11.3 Tier 3 — 살아 있는 ADR (참조용)

15-20. ADR-0002 / 0003 / 0006 / 0007 / 0008 / 0011 / 0012

### 11.4 Tier 4 — Deprecated / Historical

- ADR-0001 (tmux control mode) — 폐기
- ADR-0009 (tmux daemon isolation) — 폐기
- `docs/reports/0021/0024/0025/0026/0027` — historical (각각 다른 시점의 snapshot)

---

## 변경 이력

- 2026-05-15: 초안 — Sprint 7 의 Stage A~J 완료 후, Stage D / K 잔여 + 다음 phase (테스트 / 디버깅 / 안정화 / 리뷰) 진입 직전 시점의 self-contained closeout + handoff.
