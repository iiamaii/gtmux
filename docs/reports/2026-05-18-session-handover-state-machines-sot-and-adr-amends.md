# Session Handover — 2026-05-18 — state-machines SoT + ADR amend 4종 + Cytoscape viewer

> 이 문서는 `session-handover` skill 로 생성된 session 인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-18 (저녁)
> - 본 session 의 마지막 commit: `e6bd10a` (docs(ssot/state-machines): mermaid → Cytoscape.js + dagre 재작성 + 반응형)
> - 본 session 의 주요 주제: ① ADR-0017 D6 amend ⑤ (basic editing shortcut matrix) ② ADR-0018 schema 보완 등록 (text style / figure pattern / rotation) ③ ADR-0019 D10.1 신규 (session delete UI entry) ④ **state-machines.md SoT 작성 (3 mermaid → 7 cytoscape graph + 1 SVG sequence)** ⑤ **코드 정밀 read 로 ADR 미명시 흐름 11종 발견 → ADR-0019 D5.1/D5.2/D5.5 amend + ADR-0020 D9.1 신규** ⑥ HTML viewer 반응형 (Cytoscape.js + dagre + ResizeObserver) ⑦ README install section 분리 + legacy(tmux) 언급 제거 ⑧ Origin (`git@github.com:iiamaii/gtmux.git`) subtree push 흐름 정착
> - 같은 날 이전 handover 2 건 (별 session): `2026-05-18-session-handover-0065-fe-remediation-and-no-session-gating.md`, `2026-05-18-session-handover-0066-review-phases-1-4-complete.md` — 본 handover 는 그 두 session 이후 *문서/ADR 정합 batch* 시간대.

---

## 1. 프로젝트 개요

- **이름**: gtmux
- **한 줄 정체성**: Rust supervisor (axum/tokio) + Svelte 5 frontend 의 single-user 웹 캔버스 workspace. PTY-direct (ADR-0013) — tmux 의존 없음 (옛 이름의 잔재). per-session token + HttpOnly cookie auth.
- **현 phase**: multi-session pivot Stage 5+ (`docs/plans/0007-multi-session-pivot.md`) 진행 중, plan-0011 (component design batch) 까지 land. 본 batch 부터 *보완 기능* (text style / figure pattern / rotation) + *상태 머신 SoT* 정합.
- **architectural invariants (CLAUDE.md 본문 + ADR-0019 정합)**:
  1. **2 state domain 분리** — tmux state (sessions/windows/panes) 와 web state (panel geometry/visibility/z-index) 의 storage / mutation 경계 절대 mixing 금지
  2. **Webpage : Session = 1:1 single-attach reciprocal** (ADR-0019 D3) — multi-tab = 다른 session
  3. **Terminal : Panel = 1:N multi-mirror** (ADR-0021 D1) — Terminal 의 multi-attach 가 multi-monitor mirror 의 owner
  4. **ADR-before-code 는 hard rule** + **ADR ↔ plan/handover coherence 는 hard rule** (CLAUDE.md) — 한 쪽 amend 시 모두 동기화
  5. **Default bind 127.0.0.1 + cookie HttpOnly** — single-user 라도 보안 default 거부 안 함 (ADR-0020 D2)
- **언어 컨벤션** (CLAUDE.md): 코드는 영문, 문서는 한국어. README / CLAUDE.md / repo-meta 는 영문.
- **Origin push**: `git@github.com:iiamaii/gtmux.git` (SSH, `id_ed25519_git` key, `core.sshCommand` local 설정). subtree push 패턴 — `git subtree push --prefix=codebase origin main`. docs/ 변경은 origin 미반영 (codebase/ 만 발행 mirror).

---

## 2. 현재 session 요약

본 session 의 11 commit (시간순):

| # | commit | 핵심 |
|---|---|---|
| 1 | `13c2844` | `docs(session-delete)`: ADR-0019 **D10.1 신규** — SessionListModal hover-kebab + SessionMenu "Delete current session…" entry. plan-0007 §14.12 + handover-v3 §5/§6/§13 동시. |
| 2 | `d36f092` | `fix(backend/respawn)`: per-UUID lock (ADR-0021 **D10.3**) — concurrent same-UUID respawn race 차단 + `{reused: true}` 분기 |
| 3 | `f0b7cc5` | `docs(respawn-race)`: ADR-0021 D10.3 + 0053 §3.4 close 정합 (위 #2 의 짝 doc 빠뜨림 → 별 commit) |
| 4 | `6c96a33` | `docs(readme)`: Installation section 분리 (clone → codegen → npm install → make build → `/usr/local/bin` install) + Quickstart 단순화 + legacy(tmux) 언급 4 README 에서 제거 |
| 5 | `3644f1b` | `docs(item-schema)`: 보완 기능 3종 등록 — text 풀-style (font_family / weight / style / decoration / line_height) + figure stroke_dash / fill_pattern + ItemCommon rotation. plan-0007 §14.4 + handover-v3 §5 P1 매트릭스 + §6 Stage 5 §7~§9 + ADR-0018 변경 이력 register |
| 6 | `8761ba8` | `docs(shortcut)`: ADR-0017 **D6 amend ⑤** — basic editing matrix. P0 6 row 추가 (Cmd+A 신규 / Cmd+C/X/V ADR-0030 cross-link / Cmd+Z/Shift+Z ADR-0028 cross-link). P3 비범위 OS-standard 5종 + Cmd+F P2 deferred 명시 |
| 7 | `d26aa83` | `docs(ssot)`: **state-machines.md 신규 SoT** (436 line) — Auth/Session/Terminal 3 layer 합성. 6 mermaid (3 stateDiagram + 2 flowchart + 1 sequence) + listCloseTarget 분기 표 + dismissOnBackdrop 정책 + 21 예외 시나리오 |
| 8 | `23140d4` | `fix(ssot/state-machines)`: §3.4 + §4.2 mermaid syntax error 해소. 근본 원인 = `I[/auth redirect]` 의 slash 시작 (parallelogram shape `[/text/]` 와 충돌). 모든 node text + decision `{...}` 을 quote `"..."` 로 + `<br/>` → single-line / unicode arrow `→` → `to` |
| 9 | `0466915` | `docs(ssot/state-machines)`: **코드 정밀 read 로 ADR 미명시 흐름 11종 발견** → 본 doc 의 임시 SoT 채택 (+206 line). §3.2.1 / §3.4.1 / §3.4.2 / §4.4 / §4.4.1 / §5.1.1 / §7.1 신규 |
| 10 | `2beb5e0` | `docs(adr)`: **ADR amend 4종** — D5.1 amend ② (6-row precondition AND-gate + `200 + unmatched>0` silent→modal escalation) / D5.2.1 신규 (in-flight singleton) / D5.5.1 신규 (5-step fallback chain) / D5.5.2 신규 (3 entry source) / **ADR-0020 D9.1 신규** (onLogout 3-step load-bearing 순서). D8.1 신규 신청 → **D5.5 가 이미 cover → D5.5 amend 로 변경** |
| 11 | `8f7ca6f` | `docs(ssot/state-machines)`: HTML viewer 1차 — mermaid CDN (jsdelivr) + sidebar TOC + 8 diagram |
| 12 | `e6bd10a` | `docs(ssot/state-machines)`: **mermaid → Cytoscape.js + dagre 재작성** + 반응형. 7 interactive graph (Cytoscape) + 1 hand-crafted SVG (sequence). `clamp(360px, 70vh, 640px)` + ResizeObserver auto-refit + `[Fit]/[+]/[−]` 버튼. node class 6 (start/normal/ready/error/decision/end) |

본 session 의 코드 변경 0 — *문서 / ADR / SoT* 만. 별 process 가 동시에 BE/FE batch 진행 (commit `f086e32`, `656f9d7`, `5240cb4`, `8814b06`~`72a16e4` 등) — *본 session 작업 외*.

---

## 3. 주요 참조 자료

다음 session 이 가장 먼저 읽어야 할 문서 순서:

| # | path | 우선순위 / 용도 |
|---|---|---|
| 1 | `CLAUDE.md` | 프로젝트 root 인지 정합 + ADR-before-code rule + hard rule |
| 2 | **`docs/ssot/state-machines.md`** | ★ 본 session 핵심 산출. Auth / Session / Terminal 3-layer state machine 의 SoT |
| 3 | **`docs/ssot/state-machines.html`** | Cytoscape.js viewer — 브라우저 open 시 interactive graph |
| 4 | `docs/adr/0019-session-and-workspace-model.md` | D5.1 amend ② / D5.2.1 / D5.5.1 / D5.5.2 / D10.1 (본 session land) + D5.5 (별 session land) |
| 5 | `docs/adr/0020-auth-lifecycle.md` | D9.1 신규 (본 session land) |
| 6 | `docs/adr/0017-layout-grid-and-chrome.md` | D6 amend ⑤ (basic editing shortcut matrix) |
| 7 | `docs/adr/0018-canvas-item-data-model.md` | 변경 이력의 schema 확장 후보 register (text/figure/rotation) |
| 8 | `docs/adr/0021-terminal-pool-and-mirror.md` | D10.3 (respawn per-UUID lock — 별 session land) + D7 amend ③ (attach reverse index, 별 session land) |
| 9 | `docs/plans/0007-multi-session-pivot.md` | 활성 plan (Stage 5+). §14.4 / §14.12 / §14.20.5 amend |
| 10 | `docs/plans/0011-component-design-batch-caption-document.md` | 가장 높은 번호 plan (component design) |
| 11 | `docs/agents/frontend-handover-v3.md` | FE 진행 상태 매트릭스 (§5 P0/P1, §6 Stage 별 잔여, §10.5 단축키, §13 변경 이력) |
| 12 | `docs/agents/backend-handover-v3.md` | BE 진행 상태 (별 session 의 작업과 짝) |

본 session 의 *코드 정밀 read* 가 가리키는 코드 cross-link (state-machines.md §7.1 참조):

| 코드 | 라인 | 역할 |
|---|---|---|
| `codebase/frontend/src/lib/stores/reconnectGate.svelte.ts` | 전체 (~180 line) | 8-state 머신 |
| `codebase/frontend/src/lib/stores/workspaceSwitcher.svelte.ts` | 전체 (~80 line) | 5-stage modal stack + `listCloseTarget` 분기 |
| `codebase/frontend/src/lib/stores/sessionStore.svelte.ts` | 519-552, 572-573, 822 | `silentReattach` singleton + `ensureMutationOk` |
| `codebase/frontend/src/lib/chrome/WorkspaceSwitcher.svelte` | 86-250 | `tryAttach` / `confirmAttach` / `restorePreviousSession` / `cancelAttachConfirm` 5-step |
| `codebase/frontend/src/lib/chrome/SessionMenu.svelte` | 32-103 | `onLogout` 3-step (D9.1) + `onConfirmDelete` (D10.1) |
| `codebase/frontend/src/routes/+page.svelte` | 148-176, 204-208 | `maybeSilentReattach` 6 precondition + visibilitychange listener |
| `codebase/frontend/src/lib/ws/dispatcher.svelte.ts` | 541-559 | `prevWsState='reconnecting' && state='open'` silentReattach trigger |
| `codebase/backend/crates/http-api/src/sessions.rs` | 842 | `detach_handler` — DELETE /api/sessions/<name>/attach (ADR 명시 X, D5.5 line 296 만 cross-link) |

---

## 4. 진행중인 작업

### 4.1 본 session 의 산출은 *모두 land + push* — 추가 작업 없음

본 session 의 11 commit (state-machines + ADR amend + viewer) 모두 origin 까지 land. local working tree 의 변경 0 (state-machines 관련).

### 4.2 stray 8개 — *별 session 작업*, 본 handover 처리 안 함

본 session 의 마지막 turn 시점에 untracked 인 8개:

| 파일 | 추정 출처 |
|---|---|
| `docs/reports/0069-session-attach-confirm-cancel-recovery.md` | 별 session — D5.5 짝 report |
| `docs/reports/0070-webpage-owner-session-list-regression.md` | 별 session — D5.6 owner_key 회귀 |
| `docs/reports/0071-session-terminal-panel-lifecycle-audit.{md,html}` | 별 session — audit batch (HTML viewer 도 별 작성) |
| `docs/reports/0071-0073-verification-dashboard.html` | 같음 — 검증 dashboard |
| `docs/reports/0072-be-handover-from-0071-audit.md` | 같음 — BE work package |
| `docs/reports/0073-fe-handover-from-0071-audit.md` | 같음 — FE work package |
| `docs/reports/2026-05-18-session-handover-0065-fe-remediation-and-no-session-gating.md` | 별 session handover (오늘 오전) |
| `docs/reports/2026-05-18-session-handover-0066-review-phases-1-4-complete.md` | 별 session handover (오늘 정오) |

**다음 step (다음 session)**: 사용자가 명령 시 별 commit 으로 묶어 push. 단 *0071 audit* 의 BE/FE handover (`0072` / `0073`) 는 본 별 session 의 결과물 — 별 process 가 이미 BE/FE 코드 작업도 진행 중 (commit `8814b06`~`72a16e4` 본 batch 의 일부).

### 4.3 ADR-0018 schema 확장 본문 amend — 코드 land 시 짝

본 session 의 `3644f1b` 가 *register 만* land — ADR-0018 D2 (ItemCommon) + D4 (text/rect/ellipse/line) 본문 표는 *코드 land 시점에* 4종 동시 정합 (ADR row + BE serde struct + openapi 재발행 + FE renderer/Inspector). plan-0011 / 0012 후속 batch.

**다음 step (별 batch)**: text style 먼저 → figure pattern → rotation (cross-cut 라 마지막) 순. ADR-0018 D4 의 row 갱신 + `crates/http-api/src/schema.rs` 의 `Item::*` Option 필드 추가 + `bin/gen-openapi` 재실행.

---

## 5. 향후 작업 (아직 시작 안 함)

| 항목 | 목표 | 관련 문서 | 선행 조건 |
|---|---|---|---|
| **POST /auth/login body 형식 wire 명시** | ADR-0020 D4/D5 에 body 형식 (form-encoded vs JSON) 명시 — state-machines §7.1 에서 deferred | `docs/adr/0020-auth-lifecycle.md` D4/D5 | 코드 (`auth.ts:35-95`) 확인 후 D4/D5 sub-section amend |
| **Cmd+F (Find/search)** | 별 ADR 또는 ADR-0017 D6 amend ⑥ — Cmd+K command palette 와 분기 검토 | `docs/adr/0017` D6 amend ⑤ 의 "P2 deferred" 표시 | UX 결정 (search vs palette 분리 vs 통합) |
| **Text style 실 구현** | TextNode renderer + Inspector text section + InlineEditTextarea 정합 | handover-v3 §6 Stage 5 §7, plan-0007 §14.4 (a) | ADR-0018 D4 `text` 본문 row + BE serde + openapi |
| **Figure stroke/fill pattern 실 구현** | ShapeNode + LineNode renderer + Inspector shape section | handover-v3 §6 Stage 5 §8, plan-0007 §14.4 (b) | ADR-0018 D4 `rect/ellipse/line` 본문 row |
| **Item rotation 실 구현 (cross-cut)** | ItemCommon + 모든 renderer transform + rotate grip + 15° snap + Inspector geometry | handover-v3 §6 Stage 5 §9, plan-0007 §14.4 (c) | ADR-0018 D2 ItemCommon 본문 row + 모든 renderer amend |
| **Session delete UI 실 구현** | SessionListModal hover-kebab + SessionMenu "Delete current session…" entry (현 `SessionMenu.onConfirmDelete` 는 land — SessionListModal 의 kebab 만 잔여) | ADR-0019 D10.1, handover-v3 §6 Stage 7 §9 | (코드 부분 land — handover 확인 후) |

---

## 6. 주의사항 / Gotchas

본 session 에서 *코드 / git 으로는 잘 안 보이는* 함정:

### 6.1 D8.1 신규 신청 → D5.5 amend 로 변경
초기 ADR amend 계획은 `ADR-0019 D8.1 신규` 였음. 그러나 별 session 이 이미 *2026-05-18 amend* 로 `D5.5 Attach confirm cancel — tentative attach 와 FE active 전환 시점` 을 land (line 272-302) — *tentative lock + DELETE /attach + previous restore* 의 *기본 spec* 이미 존재. 본 session 은 **D5.5 amend (D5.5.1 + D5.5.2 신규)** 로 디테일 (5-step chain + 3 entry source) 만 추가. 다음 session 이 *별 ADR D8.1 신청* 같은 중복 작업 하지 않도록 — **D5.5 가 본 영역의 SoT**.

### 6.2 mermaid syntax 함정 — `[/text/]` slash 시작
`I[/auth redirect]` 의 *slash 시작* 이 mermaid parallelogram shape syntax `[/text/]` 와 충돌 → unclosed shape error. **모든 mermaid node text + decision `{...}` 를 quote `"..."` 로 감싸는 게 안전**. 단 본 session 의 최종 산출은 **cytoscape.js + dagre** 로 재작성 → mermaid 의존 0. state-machines.md 의 `.md` 는 mermaid 유지, `.html` 만 cytoscape.

### 6.3 `cancelAttachConfirm` 5-step chain 은 *코드 SoT* 의 정밀 흐름
ADR-0019 D5.5 는 기본 spec 만, *5 step* 의 정밀 흐름은 `WorkspaceSwitcher.svelte:215~250` 코드만 SoT. 본 session 의 D5.5.1 amend 가 표로 정합 — *recursive `confirm_required` 처리 + 8s warning toast + `goList()` 후 listCloseTarget 분기* 의 4 단계 모두 코드 보지 않고는 알 수 없음.

### 6.4 `silentReattach` 6 precondition AND-gate
ADR-0019 D5.1 본문은 *trigger 2 가지* 만 명시 (visibility + WS reconnecting→open). 코드 `+page.svelte:148-157` 는 *6 precondition AND-gate* (SSR guard / visibility / canMountApp / active / !reattachInProgress / isIdle). **한 개라도 false 면 silent 진입 안 함**. 본 session 의 D5.1 amend ② 가 6-row 표로 정합.

### 6.5 `DELETE /api/sessions/<name>/attach` = 명시 detach
ADR-0019 D5.5 line 296 명시: "`POST /api/sessions/{name}/detach` 같은 별도 endpoint 는 존재하지 않는다". 즉 detach 는 *DELETE attach* 라우트만. `WorkspaceSwitcher.svelte:228` + `ImportSessionModal.svelte:202` 두 사용처. 다음 session 이 별 detach endpoint 만들지 않도록.

### 6.6 `silentReattach` → AttachConfirmModal escalation (silent→modal)
2026-05-17 회귀 fix — silent 흐름이 `200 + unmatched>0` 만나면 *modal 진입* (`+page.svelte:169-176`). 즉 Case II 도 modal 까지 escalation 가능. 본 session 의 D5.1 amend ② `200 + unmatched>0` 분기 명시. *silent = 100% silent* 아니므로 다음 session 의 silent 흐름 변경 시 본 분기 주의.

### 6.7 별 process 가 동시 작업 중
본 session 작업 중 *별 process* 가 BE/FE 코드 batch 진행 (commit `8814b06` `111378c` `df90859` `af4aed1` `df05425` `ba7069e` `72a16e4` `f086e32` — owner_key 통일 / leaveBeacon / boot-time stale lock scan / no-session gating 등). 즉 git status 의 stray 가 자주 바뀜 — 다음 session 시작 시 `git status` 확인 후 본 handover 의 §4.2 와 비교 필요.

### 6.8 origin push = subtree
`git push -u origin main` 으로 본 repo 의 main 통째 push 하지 말 것 — origin (`iiamaii/gtmux`) 은 *codebase/ 만* 의 mirror (subtree 패턴). 항상 `git subtree push --prefix=codebase origin main`. docs/ 변경은 push 시 no-op (Everything up-to-date).

### 6.9 HTML viewer = self-contained, CDN 의존
`state-machines.html` 은 Cytoscape.js + dagre 의 jsdelivr CDN 의존. **인터넷 연결 필요**. 사용자가 offline view 원하면 CDN script tag 의 `src` 를 local vendor file 로 변경 + `npm install cytoscape cytoscape-dagre dagre` 같은 step 필요.

---

## 7. 새 session 시작 방법

다음 session 은 다음 5 step 으로 bootstrap:

1. **본 handover 를 끝까지 읽는다** (§1 ~ §6).
2. **`/Users/ws/Desktop/projects/gtmux/CLAUDE.md`** + **`docs/ssot/state-machines.md`** 읽기. 후자는 본 session 의 핵심 산출 SoT — Auth/Session/Terminal 3-layer state machine 의 합성. *시각 확인* 은 `open docs/ssot/state-machines.html` 으로 Cytoscape interactive viewer.
3. **활성 plan 확인** — `docs/plans/0007-multi-session-pivot.md` (현 Stage 5+). 가장 높은 번호는 `0011-component-design-batch-caption-document.md`.
4. **stray 처리 결정** — `git status --short` 후 본 handover §4.2 의 stray 8개 list 와 비교. 새로 추가된 stray 가 있으면 *별 session 작업의 결과물* 일 가능성 — 의도 파악 후 batch commit + subtree push.
5. **본 handover 이후 commit 확인** — `git log --oneline e6bd10a..HEAD` 로 그 사이 commit (별 process 가 동시 작업 가능). 본 handover 작성 시점 = 본 session 마지막 commit `e6bd10a`.

**자주 쓰는 명령** (본 session 정착):
- `git subtree push --prefix=codebase origin main` — origin (`iiamaii/gtmux`) 에 codebase/ 만 발행
- `git log -10 --oneline` — 최근 commit 빠른 확인
- `open docs/ssot/state-machines.html` — state machine viewer (모바일 반응형)

**진입 우선순위 (다음 session 의 첫 question 후보)**:
- "stray 8개 의도 파악 후 commit 진행해줘" — 본 handover §4.2 직접 처리
- "schema 확장 (text/figure/rotation) 의 실 구현 진행" — handover-v3 §6 Stage 5 §7~§9 의 코드 land + ADR-0018 D2/D4 본문 amend 4종 정합
- "0071 audit 결과 BE/FE 작업 진행" — `docs/reports/0071-session-terminal-panel-lifecycle-audit.md` 의 work package 처리
- "state-machines.html viewer 보강 — 별 layer/diagram 추가" — Cytoscape JSON 추가만으로 가능

---

## 변경 이력

- **2026-05-18 (저녁)**: 초안. 본 session 의 핵심 산출 = state-machines.md SoT (436 → 613 line 의 amend) + ADR amend 4종 (D5.1 amend ② / D5.2.1 / D5.5.1 / D5.5.2 / D9.1 신규) + Cytoscape.js + dagre HTML viewer (mermaid 의존 회피) + 반응형. 별 보완: ADR-0017 D6 amend ⑤ basic editing shortcut + ADR-0018 schema 보완 register + ADR-0019 D10.1 신규 + README install 분리.
