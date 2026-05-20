# 0067 — BE perf/logic 리뷰 (0066) 반영 전략

- 작성일: 2026-05-17
- 작성자: BE agent (cold-pickup 후 0066 review 반영 plan)
- 종류: **remediation plan** — `0066-backend-performance-and-logic-review.md` 의 6개 항목을 4 phase 로 분해 + ADR 영향 분리. 본 plan 이 work package 의 상위 정본.
- 정본 위치: `docs/reports/0066-backend-performance-and-logic-review.md` (review), 본 doc (plan), 각 phase 의 work package (이행 시 신규)

---

## 0. 결론 한 문단

0066 의 6개 항목 모두 실제 코드에서 재현됨 (file:line 검증 §A). 반영은 **ADR 영향 없음 (Phase 1) → ADR-0025 amend (Phase 2) → ADR-0006 amend (Phase 3) → ADR-0021 amend / 신규 ADR (Phase 4)** 순서가 가장 안전. 본 sprint 안 Phase 1 ship + Phase 2~4 는 개별 work package 분리. 단일-사용자 invariant 안 유지하면서 multi-session / large-layout 부하 시 traffic·lock·I/O 비용 절감.

---

## A. 진단 검증 (file:line)

| # | 항목 | 근거 | 검증 |
|---|---|---|---|
| BE-1 | catch-up replay 가 session scope 무시 | `ws-server/src/lib.rs:598` `for id in backend.pane_ids()` — `session_pane_set` 은 `:665~672` 의 cold-load 이후 `filter_armed=true` (`:673`) | ✅ |
| BE-2 | `/api/terminals` 매 요청 file scan | `http-api/src/terminals.rs:160` → `:206-235` 동기 `std::fs::read` + `serde_json::from_slice` per file | ✅ |
| BE-3 | PANE_IN/RESIZE/PAUSE/RESUME 의 session check 없음 | `ws-server/src/lib.rs:1201,1216,1231,1247` — pane_id 만 decode 후 backend 호출 | ✅ |
| BE-4 | layout GET/PUT lock 안 serialize + blocking I/O | `sessions.rs:1334,1347` (GET), `:1398,1412,1413` (PUT) — `workspace.rs:283` 의 sync `atomic_write_session` | ✅ |
| BE-5 | session pane-set 매 호출 HashMap 재구성 | `session_pane_set.rs:51-55` — `terminal_map.snapshot()` 전체 Vec → HashMap collect | ✅ |
| BE-6 | PTY Ctrl-C flaky | handover §6.7 + `crates/pty-backend/tests/integration_pane.rs:74` | ✅ |

추가 발견:
- TerminalMap public API 에 bulk lookup 없음 (`terminal_map.rs:108-125`) — `lookup_pane`/`lookup_uuid`/`snapshot` 만 — BE-5 fix 의 1단계가 이 API 부재 해소.
- `session_pane_set` 의 cookie path 가 이미 catch-up 직후 (`:665-672`) 동작 — BE-1 fix 는 이 lookup 을 catch-up *전* 으로 끌어올림 + filter 적용.

---

## B. Phase 1 — 🟢 Quick wins (no ADR, ~0.5~1 day)

ADR 영향 없는 내부 정합 작업 3건. 한 sprint 안 ship 목표.

### B-1. BE-3 — client→server input 의 session scope 검증

**파일**: `crates/ws-server/src/lib.rs:1176` `handle_client_envelope`

**변경**:
- signature 에 `session_pane_set: Option<&HashSet<u64>>` 인자 추가
- 호출 site (`:682-691`) 에서 `session_pane_set.as_ref()` 전달
- 4 branch (`PaneInput`, `PaneResize`, `PanePause`, `PaneResume`) 각각 decode 직후 set 멤버십 검증
- 실패 시 silent drop + `debug!` log (close 안 함 — single-user 의 stale-client 친화)
- `None` (legacy bearer-only path) 은 기존 통과 그대로

**ADR 영향**: ADR-0025 D1 은 outbound 만 명시. input scope check 는 D1 의 자연 대칭 — 본 plan 의 §F 에 small inline note 로 처리 (별 amend 안 함, 0066 § BE-3 의 정합 인용).

**test**: 신규 unit `pane_input_dropped_when_not_in_session_set` + `pane_resize_dropped_when_not_in_session_set` (ws-server 의 기존 test pattern 따름).

### B-2. BE-5(a) — `TerminalMap::resolve_uuids_to_panes` bulk API

**파일**: `crates/http-api/src/terminal_map.rs`, `crates/http-api/src/session_pane_set.rs`

**변경**:
- `TerminalMap` 에 `pub async fn resolve_uuids_to_panes(&self, uuids: &[String]) -> HashSet<u64>` 신설 — 한번의 read lock 안 `uuids.iter().filter_map(|u| g.by_uuid.get(u).map(|p| p.0)).collect()`
- `session_pane_set.rs:45-60` 의 `snapshot → HashMap → filter_map` 을 `terminal_map.resolve_uuids_to_panes(&uuids).await` 로 교체

**ADR 영향**: 없음 — 순수 내부 API.

**test**: `resolve_uuids_to_panes_matches_individual_lookup` + `resolve_uuids_to_panes_empty_uuids` + `resolve_uuids_to_panes_partial_dangling_match`.

### B-3. BE-6 — `gate1_signal_ctrl_c_interrupts_sleep` flaky 안정화

**파일**: `crates/pty-backend/tests/integration_pane.rs:74`

**변경**:
- `read_until(BEFORE)` 직후 `sleep(Duration::from_millis(200))` 추가 — 쉘이 `sleep 30` 으로 fork 완료될 시간 보장
- Ctrl-C 직후 `read_until(\$prompt)` budget 을 3s → 5s 로 확장 (CI 부하 시 안전 margin)

**ADR 영향**: 없음.

---

## C. Phase 2 — 🟡 BE-1 catch-up scope (ADR-0025 D1 amend ①)

### 변경 요약

`ws-server/src/lib.rs:544-637` 의 catch-up 단계도 cookie-attached 시 `session_pane_set` 으로 필터. `pane_ids()` 의 iteration 을 `.filter(|id| set.contains(&id.0))` 로 감쌈. `session_pane_set` cold-load 를 catch-up *전* 으로 끌어올리고, `filter_armed=true` 도 catch-up 시작 시점으로 이동.

### ADR-0025 D1 amend ① 핵심

- 현 D1.catch-up-replay-policy = "catch-up replay 는 filter bypass — 모든 PaneId 통과" (이유: cold-load race 시 정상 history 누락 위험)
- amend 후 = "cookie-attached path 는 catch-up 도 filter. cold-load race 의 false-negative 는 D3 (TerminalSpawned/LayoutChanged 의 set rebuild) 가 자연 회복. legacy bearer-only path 는 unfiltered."

### test

- `catchup_replay_skips_other_session_panes`
- `catchup_unfiltered_when_no_cookie` (legacy fallback)
- `catchup_filtered_set_resync_via_terminal_spawned` (D3 recovery)

⚠️ 회귀 risk: `:797, :962, :1015, :1036` 의 set rebuild logic 이 catch-up 의 set 과 동일 출처를 쓰는지 확인 필수. 같은 `provider.pane_ids_for_session(...)` 호출이라 정합.

---

## D. Phase 3 — 🟡 BE-4 layout I/O lock 축소 (ADR-0006 D5 amend)

### 변경 요약

`sessions.rs:1322` `layout_get_handler`:
- read-lock 안에서 `Layout` clone (또는 `Arc<Layout>` snapshot) 만 확보
- canonical serialize 는 lock 밖에서 수행

`sessions.rs:1357` `layout_put_handler`:
- write-lock 안에서 CAS 검증 → new_snap 준비
- `atomic_write_session` 호출을 `tokio::task::spawn_blocking` 으로 wrap (lock 보유 유지 — disk-first invariant 보존)
- disk write OK 시 snap 교체, 실패 시 snap 미교체

### ADR-0006 D5 amend 핵심

- 현 D5 = "disk-first CAS — write lock 안에서 atomic_write 완료 후 snap 교체"
- amend 후 = "disk-first invariant 유지. `atomic_write_session` 은 `spawn_blocking` 분리 — async worker block 회피. write lock 은 spawn_blocking await 동안 보유."

### test

- 기존 CAS / If-Match / atomicity test 전부 통과
- 신규: `layout_get_releases_lock_before_serialization` (lock 안 시간 측정 형태 — 가능하면)
- 신규: `layout_put_uses_spawn_blocking_for_disk_write` (mock 가능 시)

---

## E. Phase 4 — 🟡 BE-2 + BE-5(b) attach reverse index (ADR-0021 amend or 신규 ADR-0035)

### 변경 요약

`AppState::attach_index: Arc<RwLock<HashMap<TerminalUuid, BTreeSet<SessionName>>>>` 신설.

갱신 trigger:
- boot — 전체 session file scan 으로 index rebuild
- `PUT /api/sessions/:name/layout` — old vs new terminal UUID diff 적용 (disk write 성공 후)
- `POST /api/sessions/import` — import 된 layout 의 UUID 등록
- `DELETE /api/sessions/:name` — session 의 모든 UUID entry 제거

reader:
- `terminals.rs:160` `scan_session_terminal_refs` 제거 → `attach_index.read().await` 직접 read
- (선택) `session_pane_set.rs` 의 layout-read 도 attach_index 의 역방향 (session→uuids) 으로 단축

### ADR 영향

ADR-0021 amend or 신규 ADR-0035 — attach_index 의 lifecycle / consistency / boot rebuild 정책 명시. eventual consistency 허용 여부 (PUT 응답 200 ↔ index 갱신 사이의 stale window) ADR 본문 결정.

### test

- unit: diff_apply 정확성, boot rebuild parity (disk scan ≡ index 상태)
- integration: PUT → attach_count 변경, import → 신규 UUID 등록, DELETE → entry 사라짐
- 회귀: 기존 `/api/terminals` test 전부 통과

⚠️ index update 와 disk write 의 ordering — disk write 성공 후에만 index 갱신 (disk-of-truth 정합).

---

## F. 진입 순서 + 일정

| 우선 | Phase | 항목 | 예상 일정 | ADR |
|---|---|---|---|---|
| 🟢 1 | Phase 1 | BE-3 + BE-5(a) + BE-6 | 0.5~1d | 없음 |
| 🟡 2 | Phase 2 | BE-1 | 0.5d | ADR-0025 D1 amend ① |
| 🟡 3 | Phase 3 | BE-4 | 1d | ADR-0006 D5 amend |
| 🟡 4 | Phase 4 | BE-2 + BE-5(b) | 2~3d | ADR-0021 amend or ADR-0035 신규 |

### Phase 1 inline rationale

Phase 1 의 3건은 모두 ADR 영향 없는 내부 정합. BE-3 은 ADR-0025 D1 의 outbound 정책을 input 방향으로 대칭 적용 — D1 의 자연 결과. BE-5(a) 는 순수 내부 API 추가. BE-6 은 test 안정화. 어느 것도 새 architectural 결정 아님 — CLAUDE.md 의 "Trivial choices … don't need an ADR" 영역.

---

## G. 함정 / 회피 패턴

1. **parallel worker commit sweep** (handover §6.1) — phase 별 작은 commit 으로 분리. `git status` 로 staged 영역 확인 후 commit.
2. **BE-1 의 cold-load race** — amend ① 본문에서 D3 의 false-negative-safe 가 catch-up 의 filter 도 보호함을 재명시.
3. **BE-4 의 lock-during-spawn_blocking** — write lock 을 `spawn_blocking` await 동안 *보유*. 다른 task 의 read 가 disk-write latency 동안 대기 — 이게 disk-first invariant 의 비용. lock 밖 write + 사후 lock 교체로 풀면 CAS 가 깨짐.
4. **BE-2 index ↔ disk ordering** — disk write 성공 후에만 index 갱신. disk write 실패 시 index 미변경.
5. **BE-6 flaky** — Phase 1 안정화 후에도 재발 시 isolated re-run (handover §6.7).

---

## H. 검증 baseline

본 plan 작성 시점 (HEAD `9df0b07`):

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | tail -20
# 기대: 395 PASS / 0 FAIL (handover §7 baseline)
cargo build --release --bin gtmux --color=never
# 기대: PASS
```

Phase 1 ship 후 신규 test 증가 (예상):
- BE-3: +2 (pane_input_dropped + pane_resize_dropped)
- BE-5(a): +3 (resolve_uuids_to_panes 의 3 case)
- BE-6: 기존 1 test 안정화 — 증감 X

총 395 → 400 PASS 목표.

---

## I. 변경 이력

- 2026-05-17: 초안 — 0066 review 의 6개 항목을 4 phase 로 분해. Phase 1 즉시 진입.
