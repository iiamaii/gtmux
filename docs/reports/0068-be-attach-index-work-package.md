# 0068 — BE attach reverse index (BE-2 + BE-5(b)) work package

- 작성일: 2026-05-18
- 작성자: BE agent (0067 Phase 4 진입 시점)
- 종류: **work package** — `0067-be-perf-logic-remediation-plan.md` §E 의 Phase 4 의 구현 정본
- 정본 ADR: `docs/adr/0021-terminal-pool-and-mirror.md` D7 amend (본 work 와 함께 land)
- 관련 review: `docs/reports/0066-backend-performance-and-logic-review.md` §BE-2, §BE-5

---

## 0. 결론 한 문단

`AppState::attach_index: Arc<RwLock<HashMap<TerminalUuid, BTreeSet<SessionName>>>>` 신설. boot 시 workspace 의 모든 session file scan → index rebuild. 이후 4 mutation path (`layout_put_handler` / `delete_item_handler` / `import_handler` / `delete_handler`) 가 diff 적용. reader = `terminals.rs::list_handler` (BE-2) + (선택) `session_pane_set.rs` (BE-5b). disk-of-truth ordering 보존 — index 갱신은 *disk write 성공 후*. eventual consistency 안 됨 — write lock 안 atomic 갱신.

---

## 1. 변경 범위

### 1.1 신규 module

- `crates/http-api/src/attach_index.rs` — `AttachIndex` struct + `apply_diff` / `apply_full_session` / `forget_session` / `rebuild_from_disk` / `read_attached_sessions` / `read_all_attach_refs`.

### 1.2 신규 AppState field

```rust
pub attach_index: Arc<AttachIndex>,
```

`AppState::new` 에 default 초기화 (빈 HashMap), `with_workspace` 에서 `attach_index.rebuild_from_disk(&wm)` 호출.

### 1.3 mutation hook

| handler | 변경 | hook 시점 |
|---|---|---|
| `layout_put_handler` | old vs new terminal UUID set diff → `apply_diff(name, &removed, &added)` | disk write 성공 + snap swap 직후 (lock 안) |
| `delete_item_handler` | 제거된 item 이 Terminal variant 면 → `apply_diff(name, &[uuid], &[])` | disk write 성공 + snap swap 직후 (lock 안) |
| `import_handler` | imported layout 의 terminal UUID 전체 → `apply_full_session(name, &uuids)` | disk write + cache seed 직후 |
| `delete_handler` | `forget_session(name)` | file unlink 성공 + cache evict 직후 |

`create_handler` (empty layout) 는 no-op — index 갱신 불필요.

### 1.4 reader 교체

| 위치 | 변경 |
|---|---|
| `crates/http-api/src/terminals.rs::list_handler` | `scan_session_terminal_refs(wm)` 제거 → `state.attach_index.read_all_attach_refs()` |
| `crates/http-api/src/session_pane_set.rs` (BE-5b, 선택) | layout 읽지 않고 attach_index 의 역방향 (session→uuids) 으로 단축. 단 attach_index 가 *session→uuids* 도 보유해야 함 (현 design 의 *uuid→sessions* 만으로는 부족). 본 work package 안에서는 reader 의 single side 만 — *uuid→sessions* — 추가, session_pane_set 변경은 별 follow-up (필요 시). |

### 1.5 ADR-0021 D7 amend

D7 본문 (Terminal list UI server-wide 노출) 에 implementation note 추가 — `attach_count` / `attached_sessions` 계산이 매 GET 마다 file scan 대신 in-memory reverse index 에서 read. boot rebuild + 4 mutation hook 의 정합 명시. consistency model = strong (lock 안 갱신).

---

## 2. AttachIndex API design

```rust
pub struct AttachIndex {
    inner: RwLock<HashMap<TerminalUuid, BTreeSet<SessionName>>>,
}

impl AttachIndex {
    pub fn new() -> Self;

    /// Apply a diff for one session — `removed` UUIDs lose this session,
    /// `added` UUIDs gain this session. Empty entries are GC'd from the map.
    pub async fn apply_diff(
        &self,
        session: &str,
        removed: &[String],
        added: &[String],
    );

    /// Replace this session's contribution with `uuids` (drop prior
    /// membership, add fresh). Used by import_handler.
    pub async fn apply_full_session(&self, session: &str, uuids: &[String]);

    /// Drop `session` from every entry. Empty entries are GC'd.
    pub async fn forget_session(&self, session: &str);

    /// Cold boot rebuild — scan every session file in `wm`, extract
    /// terminal UUIDs, build the index from scratch.
    pub async fn rebuild_from_disk(&self, wm: &WorkspaceManager) -> Result<(), WorkspaceError>;

    /// Read-only: snapshot of `uuid → BTreeSet<session>` (Vec<session>
    /// returned for serialization friendliness).
    pub async fn read_all_attach_refs(&self) -> HashMap<TerminalUuid, Vec<SessionName>>;

    /// Read-only: sessions referencing `uuid`. Empty vec if none.
    pub async fn read_attached_sessions(&self, uuid: &str) -> Vec<SessionName>;
}
```

### 2.1 동시성 모델

- 단일 `RwLock<HashMap<...>>` — reader 다수, writer 직렬. mutation 은 짧음 (HashMap entry insert/remove + BTreeSet insert/remove).
- mutation 은 **handler 의 write lock (per-session SessionLayout) 안에서** 호출. 한 session 의 동시 mutation 직렬화 → diff 가 stale 없음.
- 동시에 다른 session 들의 mutation 은 attach_index 의 RwLock 으로 직렬. 짧은 critical section.

### 2.2 disk-of-truth ordering

각 mutation hook 의 순서:
1. layout serialize + disk write (성공)
2. in-memory SessionLayout snap swap
3. **attach_index 갱신** (이 순서가 핵심 — disk 가 진실 → in-mem → index)

disk write 실패 시 index 미갱신 → invariant 유지. 옛 disk 의 진실과 index 의 stale 가능성은 *zero* (disk write 후에만 index 갱신).

---

## 3. test 계획

### 3.1 unit (attach_index.rs)

- `apply_diff_adds_uuid_to_session` — 빈 상태 → diff (added=[u1]) → {u1: {sess1}}
- `apply_diff_removes_uuid_from_session` — {u1: {sess1}} → diff (removed=[u1]) → {} (entry GC)
- `apply_diff_multi_session_uuid_partial_remove` — {u1: {a, b}} → diff(a, removed=[u1]) → {u1: {b}} (entry 살아있음)
- `apply_full_session_replaces_session_contribution` — sess1 의 옛 UUID 들 다 drop, 새 UUID 들 add
- `forget_session_removes_from_all_entries` — sess1 → 어디서든 사라짐
- `rebuild_from_disk_parity_with_scan` — disk seed → rebuild → 결과 == 옛 `scan_session_terminal_refs` 의 결과 (parity 보장)

### 3.2 integration (lib.rs app-level)

- `sessions_layout_put_updates_attach_index` — 1 session, PUT add terminal A → GET /api/terminals 의 attach_count 가 1, attached_sessions = [name]
- `sessions_layout_put_remove_terminal_updates_index` — PUT remove → attach_count 0, attached_sessions []
- `sessions_delete_item_terminal_updates_index` — DELETE item → 같은 결과
- `sessions_import_populates_index` — POST /import with terminal item → /api/terminals 에 즉시 등장
- `sessions_delete_session_clears_index_for_uuids` — DELETE session → 그 session 의 UUID 들 entry 에서 session 제거

### 3.3 회귀 가드

- 기존 `GET /api/terminals` test 전부 통과 (`attach_count` 의 값 동일)
- 기존 layout PUT / delete_item / import / delete test 전부 통과

---

## 4. 회피 패턴 / 함정

- **boot rebuild 시 missing workspace** — `AppState::new` 는 workspace 없이 호출 가능. `with_workspace` 에서 *동기 boot rebuild* 호출. async runtime context 가 없을 수 있어 `tokio::runtime::Handle::try_current().block_on` 또는 `std::fs` 의 sync scan + manual lock 사용 (`blocking_write` 같은 패턴).
- **disk parse 실패 session 처리** — 옛 `scan_session_terminal_refs` 는 silently skip. boot rebuild 도 동일 — 부재/손상 session 은 index 에 미등록. 다음 successful PUT 에 자연 등록.
- **race: PUT 의 disk write 직후 ↔ 다른 reader 의 index read** — disk write 성공 후 in-mem snap swap 까지의 짧은 window. reader 가 옛 attach_count 를 잠시 봄 → eventual consistency 영역. P0 영향 없음 — UI 의 5초 polling 이 다음 cycle 에 흡수.
- **delete_item 이 *Terminal variant 외* item 을 제거할 때** — UUID 추출 안 됨 → no-op (현 code 의 `removed_terminal_id == None` 분기). attach_index 갱신 skip.
- **본 work 와 평행으로 진행되는 parallel worker** — handover §6.1 의 sweep 회피. `git add` 시 명시 파일 선택. 현 worktree 깨끗 (parallel 의 logging 분 `4b1367d` 로 commit 됨).

---

## 5. 검증 baseline

본 plan 작성 시점 (HEAD `4b1367d`):

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | tail -15
# 기대: 405 PASS / 0 FAIL (0067 §H 기록 + Phase 3 의 +2)
cargo build --release --bin gtmux --color=never
# 기대: PASS
```

Phase 4 ship 후 신규 test 증가 (예상):
- AttachIndex unit: +6 (apply_diff 의 3 case + apply_full + forget + rebuild parity)
- integration: +5 (PUT add/remove + delete_item + import + delete)

총 405 → 416 PASS 목표.

---

## 6. 진입 순서

1. **ADR-0021 D7 amend ③** — implementation note 추가 (본 plan 의 §1.5 정합)
2. **AttachIndex module** — 신규 `attach_index.rs` + unit test
3. **AppState integration** — field + `with_workspace` 의 boot rebuild + sync/async 패턴 결정
4. **4 mutation hook** — sessions.rs 의 4 handler 에 갱신 호출 삽입
5. **terminals.rs reader 교체** — BE-2 의 핵심
6. (선택) **session_pane_set.rs reader** — BE-5(b). 본 work package 의 *우선 외* (필요성 측정 후 진행).
7. **integration test 5 종**
8. cargo test + build → commit

---

## 7. 변경 이력

- 2026-05-18: 초안 — 0067 Phase 4 진입 시점 work package 작성.
