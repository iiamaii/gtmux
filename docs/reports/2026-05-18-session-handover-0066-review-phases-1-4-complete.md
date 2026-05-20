# Session Handover — 2026-05-18 — 0066 BE review 의 4 phase remediation 종료

> 이 문서는 `session-handover` skill 로 생성된 session 인수인계 문서입니다.
> 새 session 으로 작업을 이어가려면 §7 "새 session 시작 방법" 을 먼저 보세요.
>
> - 생성일: 2026-05-18
> - 생성 session 의 마지막 BE 커밋: `5240cb4` (`docs(adr,reports): ADR-0021 D7 amend ③ + 0068 attach reverse index work package`)
> - HEAD (handover 작성 시점): `23140d4` (parallel FE worker 의 ssot 보강)
> - 이번 session 의 주요 주제: `0066-backend-performance-and-logic-review.md` 의 6 항목 (BE-1~BE-6) 을 4 phase 로 분해, 모두 closed. 누적 +21 신규 test (workspace 395 → 416 PASS / 0 FAIL).

---

## 1. 프로젝트 개요

- **이름**: gtmux — `CLAUDE.md` 의 canonical short name.
- **한 줄 정체성**: tmux 가 backend execution engine, web 이 infinite canvas — single-user web 앱.
- **현재 phase / 단계**: **Stage 7** — multi-session pivot 완료, UX 폴리시 + 회귀 가드 + Toolbar item 의 BE 정합 (0064 handover §1.3).
- **침범 불가능한 invariants** (코드/ADR 진실):
  - tmux state ↔ web state 분리 (`docs/sketch.md` §4 / CLAUDE.md "Architectural invariants")
  - single-attach: Webpage : Session = 1:1 (`docs/adr/0019` D3)
  - takeover 금지 — 다른 cookie 만 409 (`docs/adr/0019` D4)
  - PTY direct integration (ADR-0013), control-mode 폐기됨 (history note: CLAUDE.md 가 "tmux control mode" 라 적혀있으나 ADR-0013 cutover 이후 PTY 직접. 다음 session 이 헷갈리지 말 것)
  - ADR-before-code 정책 — 비-trivial 결정은 ADR 먼저 (`CLAUDE.md` "ADR-before-code is a hard rule")
  - **disk-of-truth ordering** (ADR-0006 D13 amend ③, 본 session ship) — disk write 성공 후에만 in-mem snap swap + 부수 index 갱신

## 2. 현재 session 요약

이번 session 에서 한 일 (시간 순):

- 0064 handover 로 cold-pickup → 진행 우선순위 파악
- `docs/reports/0066-backend-performance-and-logic-review.md` 의 6 항목 (BE-1~BE-6) 을 코드 file:line 으로 검증 → 모두 재현 확인
- 4 phase remediation 전략 작성 → `docs/reports/0067-be-perf-logic-remediation-plan.md` ship
- **Phase 1** (`70f3330`): BE-3 input session scope check + BE-5(a) `TerminalMap::resolve_uuids_to_panes` bulk API + BE-6 PTY gate1 flake 안정화 (`stty -echo` + `&&` chain root-cause fix)
- **Phase 2** (`074c465`): BE-1 catch-up replay 의 session scope filter — ADR-0025 D1 amend ③ + `filter_armed` flag 제거 + cold-load 를 catch-up *이전* 으로 이동 + 3 integration test
- **Phase 3** (`06ed3a4`): BE-4 layout I/O lock 축소 + spawn_blocking 분리 — ADR-0006 D13 amend ③ + `SessionLayout::new_with_bytes` helper (double-serialize 제거) + 2 test
- **Phase 4** (`656f9d7` + `5240cb4`): BE-2 attach reverse index — 신규 `attach_index.rs` module + ADR-0021 D7 amend ③ + `docs/reports/0068-be-attach-index-work-package.md` + 11 test
- parallel FE worker 가 동시 marathon-ship — 6 FE commit + sessions.rs logging 1 commit (`4b1367d`) 가 본 BE work 와 정상 interleaved. sweep 0.

### 결정사항

- **0066 의 6 항목을 4 phase 로 분해** — ADR 영향 없는 Phase 1 → ADR-0025/0006/0021 amend 각각 짝 commit 순서. 이유: ADR-before-code 정책 준수 + small focused commit. 사용자 확인: "권장 대로 진행", "git commit 후 Phase 3 진입해", "Phase 4 진입해" 순서로 명시 승인.
- **stash 패턴으로 parallel worker 의 sessions.rs 분 격리** — Phase 3 진입 전 worktree 의 unstaged logging diff 를 `git stash push -- <file>` 로 격리, BE-4 commit 후 pop. 한 commit 안 사weep 회피 (handover §6.1 정합). Phase 4 진입 시점에는 parallel worker 가 `4b1367d` 로 자체 commit 해 worktree 깨끗.
- **ADR-0021 amend vs 신규 ADR-0035** 의 선택 — Phase 4 의 attach_index 는 ADR-0021 D7 ("Terminal list UI 의 server-wide 노출") 의 자연 확장이라 amend 채택. 신규 ADR 분리 안 함. 사용자 명시 확인 없음 — agent 판단 (ADR-0021 D7 본문이 정확히 그 영역).
- **BE-1 의 옛 cold-load race 정책 (catch-up=bypass) 폐기** — ADR-0025 D3 의 hot-update 채널 (layout_events / terminal_spawned_events / session_change_events) 이 false-negative-safe 정합으로 자연 회복함을 amend 본문에 명시. race-1/race-2 위험 평가표 첨부.
- **BE-4 의 spawn_blocking 동안 write lock 유지** — disk-first invariant 보존을 위한 의도. lock 밖 write 로 풀면 CAS 깨짐. 비용은 다른 reader 의 disk-write latency 대기지만 worker thread 는 free.
- **BE-6 root cause** — 단순 timing 이 아닌 **PTY line discipline echo 의 input 문자열이 acc 에 섞임** (`echo BEFORE` 의 input echo 가 `read_until(BEFORE)` 매칭 + 최종 assertion 의 `echo AFTER` literal 이 false-positive). `stty -echo` + `&&` chain 으로 deterministic 화. handover §6.7 의 timing race 진단보다 deep root cause.

### 변경된 파일

| 파일 | 변경 요약 |
| ---- | --------- |
| `codebase/backend/crates/http-api/src/attach_index.rs` (신규) | Phase 4 — AttachIndex 신규 module (350 lines) + 6 unit test |
| `codebase/backend/crates/http-api/src/lib.rs` | Phase 4 — `attach_index` mod + AppState field + `with_workspace` 의 boot rebuild + 5 integration test |
| `codebase/backend/crates/http-api/src/sessions.rs` | Phase 3 — GET/PUT lock 축소 + spawn_blocking + new_with_bytes. Phase 4 — 4 mutation hook + `diff_terminal_uuids` |
| `codebase/backend/crates/http-api/src/terminals.rs` | Phase 4 — `scan_session_terminal_refs` 제거 → `attach_index.read_all_attach_refs()` |
| `codebase/backend/crates/http-api/src/terminal_map.rs` | Phase 1 — `resolve_uuids_to_panes` bulk API + 3 test |
| `codebase/backend/crates/http-api/src/session_pane_set.rs` | Phase 1 — snapshot→HashMap→filter_map 체인 제거 |
| `codebase/backend/crates/ws-server/src/lib.rs` | Phase 1 — `pane_id_in_session_set` predicate + 4 input branch check. Phase 2 — catch-up scope filter + `filter_armed` flag 제거 + 3 integration test |
| `codebase/backend/crates/pty-backend/tests/integration_pane.rs` | Phase 1 — gate1 flake fix (stty -echo + && chain + fork-settle margin) |
| `docs/adr/0006-persistence-storage.md` | Phase 3 — D13 amend ③ (spawn_blocking + GET serialize 외부) |
| `docs/adr/0025-session-scoped-pane-output-filter.md` | Phase 2 — D1 amend ③ (catch-up scope filter + race 평가표) |
| `docs/adr/0021-terminal-pool-and-mirror.md` | Phase 4 — D7 amend ③ (attach reverse index implementation note) |
| `docs/reports/0067-be-perf-logic-remediation-plan.md` (신규) | 4 phase 전략 정본 |
| `docs/reports/0068-be-attach-index-work-package.md` (신규) | Phase 4 work package |

미커밋 변경:

- 미커밋: **있음** (FE worker work — 본 session 외)
  - `codebase/frontend/src/lib/chrome/AttachConfirmModal.svelte`
  - `codebase/frontend/src/lib/chrome/WorkspaceSwitcher.svelte`
  - `codebase/frontend/src/lib/http/sessions.ts`
  - `codebase/frontend/src/lib/stores/workspaceSwitcher.svelte.ts`
  - `codebase/frontend/src/lib/types/sessions.ts`
  - `docs/adr/0019-session-and-workspace-model.md`
  - `docs/reports/0069-session-attach-confirm-cancel-recovery.md` (untracked)
  - 본 BE session 의 work 와 무관. 다음 BE session 의 work 와 충돌 가능성 = `docs/adr/0019` 변경 (BE 의 attach/detach 흐름 영향 가능) — Phase 5 진입 전 검수 필요.

## 3. 주요 참조 자료

| 영역 | 경로 | 왜 읽어야 하는가 |
| ---- | ---- | --------------- |
| 프로젝트 instructions | `CLAUDE.md` | 컨벤션·언어·invariants (영문 코드/한글 docs) + ADR-before-code 정책 |
| 스펙 | `docs/sketch.md` (한글) | scope · MVP · 우선순위 single source of truth |
| 본 session 의 review | `docs/reports/0066-backend-performance-and-logic-review.md` | BE-1~BE-6 의 원본 진단 |
| 본 session 의 plan | `docs/reports/0067-be-perf-logic-remediation-plan.md` | 4 phase 전략 정본 + 진단 검증표 §A |
| Phase 4 work package | `docs/reports/0068-be-attach-index-work-package.md` | attach_index design + test 계획 |
| 직전 handover | `docs/reports/0064-be-session-migration-handover.md` | 본 session 의 cold-pickup 진입점 |
| FE 측 review | `docs/reports/0065-frontend-performance-and-logic-review.md` | parallel FE worker 의 작업 영역 (FE-1~FE-6) |
| FE 측 신규 work | `docs/reports/0069-session-attach-confirm-cancel-recovery.md` (untracked) | parallel worker 가 진행 중인 attach confirm cancel recovery |
| ADR (본 session amend) | `docs/adr/0006-persistence-storage.md` D13 amend ③ | layout I/O spawn_blocking 정합 |
| ADR (본 session amend) | `docs/adr/0025-session-scoped-pane-output-filter.md` D1 amend ③ | catch-up replay scope filter |
| ADR (본 session amend) | `docs/adr/0021-terminal-pool-and-mirror.md` D7 amend ③ | attach reverse index |
| 활성 plan | `docs/plans/0011-component-design-batch-caption-document.md` | FE-side, 본 BE session 과 직접 관련 없음 |

## 4. 진행중인 작업

본 BE session 안 잔여 BE work 없음 — 0066 의 6 항목 모두 closed.

### 4.1 (BE-5(b)) session_pane_set provider 의 attach_index 활용 — *옵션*

- **상태**: Phase 4 의 attach_index 가 ship 되었으나 `session_pane_set.rs` 는 아직 옛 layout-read 패턴 사용. 0067 §E + 0068 §1.4 에 "follow-up, 필요성 측정 후" 로 parked.
- **관련 문서**: `docs/reports/0068-be-attach-index-work-package.md` §1.4
- **관련 파일·코드**: `codebase/backend/crates/http-api/src/session_pane_set.rs` — 현재 `entry.read().await` 후 layout iterate + `terminal_map.resolve_uuids_to_panes`
- **다음 한 step**: 진입 전 필요성 측정 — `session_pane_set` provider 호출 빈도가 layout-read 의 lock 비용 의미 있게 만드는지 확인 (`hub.session_for_cookie` 의 frequency × 평균 layout 크기). 의미 있으면 attach_index 에 *session→uuids* 역방향 map 추가 필요 (현재는 *uuid→sessions* 만).

### 4.2 BE worktree 의 FE worker work 검수

- **상태**: 본 session 종료 시점 worktree 에 parallel FE worker 의 attach confirm cancel recovery work 가 unstaged. `docs/adr/0019` 변경 포함 — BE 의 attach/detach 흐름 영향 가능성.
- **관련 문서**: `docs/reports/0069-session-attach-confirm-cancel-recovery.md` (untracked)
- **관련 파일·코드**: `codebase/frontend/src/lib/chrome/AttachConfirmModal.svelte` 외 4 FE file + `docs/adr/0019`
- **다음 한 step**: BE session 진입 시점에 `git diff docs/adr/0019-session-and-workspace-model.md` 로 변경 검수. BE 의 attach_handler / attach_confirm_handler / detach_handler 의 contract 영향 확인. 영향 없으면 leave-as-is (FE worker 가 자체 commit 할 것). 영향 있으면 별 BE follow-up 발주.

## 5. 향후 작업

0067 §F 의 P2 cosmetic 항목 — 본 review scope 외, 별 session 이양 가능:

### 5.1 0053 §7 RFC3339 통일 (P2 cosmetic)

- **목표**: `POST /api/sessions/import` 의 응답 `created_at` (u64 unix_secs) ↔ Export envelope `exported_at` (RFC3339 string) wire 형식 통일.
- **관련 문서**: `docs/reports/0053-be-verification-checklist.md` §7
- **선행 조건**: FE wire 의 `ImportSessionResponse.created_at: number` 타입 변경 영향 — FE 와 짝 commit 필요 (dual-emit 일시 지원 또는 한쪽 amend 결정).
- **예상 진입 지점**: `codebase/backend/crates/http-api/src/sessions.rs::import_handler` (현 `created_at: u64`) + `docs/adr/0029-session-import-export.md` (envelope spec).

### 5.2 ADR-0034 D2 directory carve-out (P2)

- **목표**: file-stat 의 `kind: "directory"` 응답이 현 시점 403 (allowlist 의 ext 매칭과 모순). 별 처리 정책 결정.
- **관련 문서**: `docs/reports/0060-be-file-stat-work-package.md` §3 amend ② note + `docs/adr/0034-file-stat-endpoint.md` D2
- **선행 조건**: ADR-0023 allowlist 의 (ext, prefix) 모델에 directory probe 의 ext-less mode 추가 또는 별 endpoint 분리 결정.
- **예상 진입 지점**: `codebase/backend/crates/http-api/src/file_stat.rs` + `crates/http-api/src/file_open/allowlist.rs`.

### 5.3 (외부 work) FE Phase 4 batches

- **목표**: 활성 plan (`docs/plans/0010-ui-ux-batch-4-inspector-and-layer-actions.md`, `docs/plans/0011-component-design-batch-caption-document.md`) — FE-side, 본 BE session 무관.
- **관련 문서**: 활성 plan
- **선행 조건**: parallel FE worker 가 driving.
- **예상 진입 지점**: FE session 별 발주.

## 6. 주의사항 / Gotchas

- **parallel worker commit sweep** (0064 handover §6.1) — 본 session 내내 parallel FE worker + 별 BE worker 가 동시 marathon-ship. **`git commit` 직전 `git status` 로 staged 영역 확인** 필수. Phase 3 진입 시 sessions.rs 의 parallel worker 분이 unstaged 였음 → `git stash push -- <file>` 로 격리 + commit 후 `git stash pop`. Phase 4 진입 시는 parallel 이 자체 commit (`4b1367d`) 함 → stash 불필요. 다음 session 도 같은 패턴.
- **ADR-0025 D1 amend ③ 의 race 잔여 위험** — cookie-attached path 의 catch-up replay 가 이제 filtered. cold-load 의 false-negative (PUT 직후 reconnect 시 신규 pane history 누락) 가 *한 reconnect* 의 history loss → D3 의 hot-update 채널이 즉시 회복. 단일-사용자 의 frequency 에서 무해. amend 본문의 race 평가표 §race-1/race-2 참조.
- **ADR-0006 D13 amend ③ 의 disk-first invariant** — `spawn_blocking` 분리 시 write lock 을 `.await` 동안 *보유*. 다른 reader 는 disk-write latency 만큼 대기 (옛 정책과 동일) — `worker thread block 회피` 만 amend. lock 밖 write + 사후 swap 으로 풀면 CAS 깨짐. 다음 session 이 "왜 lock 을 더 일찍 안 풀어?" 라 묻기 전에 amend 본문 §"불변식 보존" 읽을 것.
- **ADR-0021 D7 amend ③ 의 consistency model** — attach_index 의 갱신은 *disk write 성공 후* 같은 critical section. eventual consistency 아님 — strong consistency. 단 `attach_index.read_*` 는 read lock 만 잡으므로 일시 stale 가능 (다른 task 의 write 가 commit 직전인 순간). UI polling 5초 cycle 안 흡수.
- **PTY gate1 의 root cause = line discipline echo** (Phase 1 BE-6) — 옛 handover §6.7 의 "timing race 가능성" 진단은 *symptom* 만 본 것. 실제 root cause = `read_until(BEFORE)` 가 input echo 의 BEFORE 와 매칭 + 최종 assertion 의 input echo 의 AFTER literal false-positive. **다음 session 이 비슷한 PTY 테스트 작성 시 `stty -echo` 선행** 패턴 따를 것 (gate3 + 본 fix 의 정합).
- **0064 handover 의 "tmux control mode" 표현** — CLAUDE.md §"Architectural invariants" 5 가 "control mode integration only" 라 적혀있으나 **ADR-0013 cutover 이후 PTY 직접**. handover/SSOT 의 일부 문서가 옛 표현 사용. 다음 session 이 control mode 코드를 찾으려 하지 말 것 — `crates/pty-backend/` 가 진실.
- **BE-5(b) 의 design 미결** — attach_index 는 현재 *uuid→sessions* 만 (Phase 4 의 minimal scope). session_pane_set 가 활용하려면 *session→uuids* 역방향 map 추가 필요. add 의 design 비용 vs 성능 이득 의 측정이 진입 선행 (§4.1 의 "다음 한 step").
- **`pane_id_in_session_set` predicate 의 cookie-less semantics** — `None` (legacy demo path) 일 때 모든 pane_id pass-through. 단일-cookie 환경 외 bearer-only automation 호환 보존. 다음 session 이 "보안 강화 차원에서 None 도 reject 하자" 라 제안하면 **거부** — D5 의 legacy demo path 보존 (ADR-0025) 와 충돌.

## 7. 새 session 시작 방법

1. 이 handover 문서를 끝까지 읽는다.
2. `CLAUDE.md` (+ `docs/sketch.md`) 를 읽는다 — 컨벤션·invariants 확인. 단 `CLAUDE.md` 의 "tmux control mode" 표현은 ADR-0013 cutover 후 옛 표현임을 인지 (§6 Gotchas).
3. §3 의 활성 plan 들 — 본 session 의 plan = 0067/0068. **0066 의 4 phase 모두 closed**, 진행 work 없음.
4. §4 의 첫 항목 = (옵션) BE-5(b) session_pane_set 의 attach_index 활용 — 진입 전 *필요성 측정* 부터.
5. handover 작성 이후 변경 확인: `git log --oneline 5240cb4..HEAD` (본 session 의 마지막 BE 커밋 이후).
6. worktree 의 FE worker 변경 (`git status --short` 의 `M` 분) 검수 — `docs/adr/0019` 변경이 BE attach 흐름에 영향 있는지만 확인. 영향 없으면 leave-as-is.

만약 §4/§5 의 BE 항목이 모두 idle 이면 **새 BE review 발주 또는 FE worker 의 phase 진입 지원**을 사용자에게 확인.

### 검증 baseline (본 session 종료 시점, HEAD `5240cb4`)

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | tail -15
# 기대: 416 PASS / 0 FAIL
cargo build --release --bin gtmux --color=never
# 기대: PASS (~32s)
```

신규 test 누적 (본 session): +21
- Phase 1: +5 (BE-3 predicate 2 + BE-5(a) bulk 3 + BE-6 안정화 0)
- Phase 2: +3 (catch-up filtered / no-cookie / no-provider)
- Phase 3: +2 (new_with_bytes / disk-bytes ETag round-trip)
- Phase 4: +11 (AttachIndex unit 6 + integration 5)

---

_생성: `session-handover` skill v1_
