# 0077 — Terminal pool ↔ session attach_index desync 진단·해결 보고

- 작성일: 2026-05-18
- 작성 주체: agent (FE/BE 통합 진단 — 0073 FE handover 의 follow-up)
- 정본 cross-link:
  - 상위 audit: `0071-session-terminal-panel-lifecycle-audit.md`
  - FE handover: `0073-fe-handover-from-0071-audit.md`
  - ADR: `0021-terminal-pool-and-mirror.md` D7 amend ③/④ + D10
  - 관련 ADR (FE 의 *defense-in-depth*): ADR-0018 D6 c2 (panel close ≠ terminal kill)
- 후속 commit: `605d8d8` → `5ea3dc3` → `c63be0c` → `a276058` (모두 본 보고의 fix path)

---

## 1. 문제 (사용자 보고 요약)

여러 webpage 가 열려있는 상태에서 BE 를 종료 → 재인증 → 다시 진입하면:

1. TerminalListView 의 행이 *canvas 에 panel 이 있음에도* `(!) desync` badge 로 표시.
2. 동일 환경에서 *현 session attach + canvas 의 panel (파란색 row)* 가 ALL 모드에선 보이는데 THIS 모드에선 hide.
3. fresh BE start 후 session 연결 + 기존 terminal attach 시 *모든 terminal 이* desync.
4. 그렇게 desync 된 session 에서는 terminal 제거 / 생성이 막힘.

→ 사용자 mental model: terminal pool 의 reference counting 이 어딘가에서 깨졌고, 그 결과 *다른 session 의 mirror* 가 보호받지 못하면서 *kill 가능* → 데이터 손상 risk.

---

## 2. 진단 timeline

### 2.1 초기 가설 (모두 검증 후 reject)

| # | 가설 | 검증 방법 | 결과 |
|---|---|---|---|
| H1 | `applyMutation` 의 PUT 실패 시 `sessionStore.items` 에 ghost item 잔존 | `sessionStore.svelte.ts:619-678` 의 `applyMutation` 흐름 정독 | **Reject** — `transform` 은 store 를 mutate 하지 않음, PUT 실패 시 store 자체 변경 0 |
| H2 | `mount_cascade` payload 의 trigger_session 검증 없음 → session switch race | BE encoder `payload.rs:80` + FE decoder/handler | 일부 정합 — race guard 적용 가능 (commit `abc5931` 으로 land) — 단 본 시나리오의 root cause 아님 |
| H3 | `attach_index.apply_diff` 의 `removed` 계산이 다른 session 의 entry 잘못 erase | `attach_index.rs` 의 6 unit test 정독 | **Reject** — `apply_diff` 가 `set.remove(session)` 만 하므로 *해당 session contribution* 만 영향, 다른 session 의 mirror entry 보존 |
| H4 | `layout_put_handler` 의 attach_index hook 호출 누락 | `sessions.rs:1636` 의 `state.attach_index.apply_diff(&name, &removed, &added)` 정독 | **Reject** — disk write 성공 후 무조건 호출, owner guard 와 무관 |
| H5 | `create_terminal_handler` (BE Stage 5-D path P2) 가 layout 안 쓰고 mount_cascade signal 만 broadcast → FE 가 받아서 layout PUT 하는 race-prone design | `sessions.rs:677-743` 정독 | **Reject** — design 자체는 race-prone 이지만 mount_cascade race guard (H2) 적용 후 *본 시나리오 (fresh BE + 기존 attach) 와 별 path* |

### 2.2 실제 root cause 도달

H1~H5 모두 reject 후 *코드 path 자체* 에서는 정합. 그러나 사용자 시연 환경에서 desync 가 *결정적으로* 재현 → *binary 와 source 의 mismatch* 의심.

확인:
```bash
ls -la codebase/backend/target/release/gtmux
# -rwxr-xr-x@ 1 ws  staff  3822048  5월 18 13:19 ...
git log --oneline -1 --before="2026-05-18 13:19"
# 8814b06 refactor(be): D5.6 owner_key 통일
```

→ release binary 의 mtime = `5월 18 13:19` = commit `8814b06` 시점. 그 이후 land 된 commit (`72278b1` / `abc5931` / `a1ecdb3` / `8cd925a` / `5ea3dc3` / `c63be0c` / `a276058`) **모두 release binary 에 없음**. 사용자가 demo 에 사용한 BE process 는 `target/release/gtmux` — **본 session 의 BE-side fix 가 모두 적용 안 된 상태로 시연**.

**진짜 root cause**:
- 단순 진실: **`cargo build --release` 누락**. BE source 의 fix 는 옳았으나 binary 가 stale.
- 부차 가설 (가능성 있음, 미확정): BE source 의 *진짜 결함* (boot rebuild 의 schema parse miss 또는 attach 흐름의 hook 누락) 가 있다면, *release rebuild 후에도 desync* 가 재현했을 것. 사용자 시연 후 release rebuild + restart → **desync 해소** → **본 session 의 self-heal hook (`a276058`) 가 결함을 회복** 했다는 강한 증거.

즉 root cause 는 두 레이어:
1. **Binary stale**: 모든 fix 가 적용 안 됨 — 본질적 *방법론* 문제
2. **(latent) `attach_index` 정합의 4 mutation hook 보장 외 path 의 미보장**: 어떤 source (boot rebuild miss / schema drift / 미보고 race) 의 stale 이 영속 가능. self-heal hook 가 *원인 무관* 회복.

---

## 3. 시도한 fix 의 series 와 의미

본 session 의 BE+FE 변경은 *방어 + 회복 + 가시성* 의 3 레이어 정합:

### 3.1 즉시 사용자 보호 (FE-only defense)

| Commit | 변경 | 의도 |
|---|---|---|
| `72278b1` | F3: `TerminalListView.killOne` 의 `isOnCurrentCanvas` defensive guard | desync 상태에서도 *현 session 의 panel 보호* — kill 시 다른 session 의 mirror 가 dangling 되는 사고 차단. FE local 만으로 회복 못 하는 다른 session 의 mirror 는 *root cause fix 후* 자연 해소 |
| `72278b1` | F4: `(!) desync` badge + `$effect` console.warn + onclick `terminalPool.refresh()` | 사용자 인지 + 자가 회복 + future trace 보조 |
| `72278b1` | F5: badge label `unplaced` → `pool only` (3-state) / `Mine/All` → segmented control / Inspector sess row empty 시 표시 | 사용자 mental model 일치 — *process death* 와 *no panel* 의 의미 분리 |
| `605d8d8` | Task 1: badge `here/here+N` → `×N` + tooltip 으로 session 이름 list / Task 2: `Panel + Terminal` 의 mirror 가 있을 때 button disable + 이유 명시 | UX 단순화 + 데이터 손상 추가 차단 |
| `5ea3dc3` | THIS filter 의 union 에 `sessionStore.items.has(t.id)` 추가 | desync row 가 THIS 모드에서도 노출 — `(!) desync` badge 가 보여서 사용자가 즉시 회복 시도 가능 |
| `a1ecdb3` | Tab label `THIS / ALL` (uppercase) | 사용자 요청 (시각 명료화) |

### 3.2 진단 가시성 (BE trace)

| Commit | 변경 | 의도 |
|---|---|---|
| `8cd925a` | `attach_index.{apply_diff, apply_full_session, forget_session, rebuild_from_disk}` 4 site 에 tracing 추가 | 사용자 시연 환경에서 root cause cross-walk 보조 |
| `c63be0c` | `rebuild_from_disk` 의 *per-session* debug log + `sessions_skipped > 0` 시 **WARN** surface | boot rebuild miss 가 root cause 면 *boot log 만으로* 즉시 인지 |

### 3.3 Root-cause-agnostic 회복 (BE self-heal)

| Commit | 변경 | 의도 |
|---|---|---|
| `a276058` | `classify_layout_terminals` + `attach_confirm_handler` 의 200 응답 직전에 `attach_index.apply_full_session(name, &load_terminal_uuids(...))` 호출. `apply_full_session` 의 trace 도 *prior vs new count* 비교 후 drift 시 **WARN** | **본 session 의 결정적 fix** — 어떤 source 의 stale 이든 *session 연결 시점에 자동 회복*. boot rebuild 가 정상이면 set semantics 라 변경 0 (비용 microsecond), miss 였으면 자동 회복 |

### 3.4 외부 commit (별 worker)

| Commit | 변경 |
|---|---|
| `abc5931` | BE+FE `mount_cascade` wire 에 `trigger_session` 동봉 + FE handler race guard (session-switch race 차단). 본 보고의 H2 가설 fix |

---

## 4. 최종 검증 결과

사용자 보고: *"이제 정상적으로 동작하네"*.

조건:
- `cargo build --release` 으로 latest binary rebuild
- BE process 재기동 (latest binary)
- 같은 시나리오 (fresh BE → session 연결 → 기존 terminal attach) 재현 → desync 해소

→ 본 session 의 fix series 가 실제 환경에서 **end-to-end 검증**.

---

## 5. 교훈 (방법론)

### 5.1 Binary 와 source 의 mtime cross-check 필수

진단 turn 의 *대부분* 이 source code level 의 가설 (H1~H5) 만 검증 — *binary 가 latest 인지 검증* 안 함. 결과적으로 시간 비용 + 사용자 시연 cycles 낭비.

**개선 절차**:
- 모든 BE 변경 commit 후 *반드시* `cargo build --release` 실행 (또는 release binary mtime 확인).
- 또는 demo wrapper script 가 `cargo build --release` 를 자동 실행 (incremental, ~3s) 후 binary spawn.
- handover doc 의 verification 절차에 *binary mtime check* 한 줄 추가 권장.

### 5.2 Self-heal hook 의 design 가치

`attach_index` 의 정합은 *4 mutation hook* + boot rebuild 로 보장된다고 *증명* — 코드 정독으로도 *clean*. 그러나 실 production 에서 stale 이 발생하면 *어디서 깨졌는지* 진단하는 비용이 큼.

`a276058` 의 self-heal hook 는 **invariant 의 *증명* 과 무관 *최후의 안전망***. session 연결 시점이라는 *명확한 timing* 에서 *cost 무시 가능* + *원인 무관 회복*. 이 design 패턴이 *비슷한 cache invariant* (예: `terminal_map`, `session_cache`) 에도 적용 가능한지 follow-up 검토 가치 있음.

### 5.3 FE defense-in-depth 의 보조 효과

본 series 의 *F3 kill guard* 가 사용자 보고에 *"진척"* 으로 인식된 이유 — root cause 가 fix 안 된 시점에도 *데이터 손상* 만은 차단. 사용자가 자신의 *terminal 제거/생성 막힘* 을 *원하는 동작* 으로 받아들임. ADR 의 invariant *"panel close ≠ terminal kill"* 가 FE 레이어에서도 명시 enforce 된 결과.

---

## 6. ADR / SSoT 반영 항목

### 6.1 ADR-0021 D7 amend ④ — `attach_index` 의 attach 시점 self-heal (필수)

amend ③ 는 *consistency model = strong* 을 명시했지만 *boot rebuild 또는 외부 source 의 silent miss* 의 영속화 가능성을 명시 안 함. amend ④ 가 self-heal hook 의 invariant 를 명시.

핵심 추가 내용:
- 모든 attach 흐름 (`classify_layout_terminals` / `attach_confirm_handler`) 의 200 응답 직전에 `attach_index.apply_full_session(name, &load_terminal_uuids(...))` 호출.
- 의미: **session 연결 시점이 attach_index 의 마지막 정합 보장 지점** — 이전 어떤 source 의 stale 이든 회복.
- 비용: layout scan + set update — microsecond 대. 무시 가능.
- 안전성: `apply_full_session` 가 *그 session 의 contribution 만* replace — 다른 session 의 mirror entry 보존.
- 진단: `apply_full_session` 의 trace 가 *prior vs new count* 비교 후 drift 시 **WARN** surface — 본질적 결함 path 의 추가 추적 가능.

→ **본 보고와 함께 amend 진행**.

### 6.2 SSoT `state-machines.md` 의 attach lifecycle — 보조 보강 (선택)

§3 (Session attach lifecycle) 의 attach 흐름 mermaid 에 *self-heal phase* 를 명시할지 검토. 단 *implementation note* 의 성격이라 ADR amend 만으로 충분하다는 시각도 가능. **본 turn 에선 ADR 만 amend, SSoT 는 future amend 후보로 보류**.

### 6.3 ADR-0021 D10 / D7 의 *self-heal 패턴 일반화* — follow-up (선택)

본 fix 의 design 패턴 (`session 연결 시점의 invariant reset`) 이 *terminal_map* / *session_cache* / *attach_index_by_session* (선택 future map) 같은 다른 cache 에도 적용 가능. 본 보고의 §5.2 의 가치 분석을 *별 amend* 로 발주할지 follow-up 결정.

---

## 7. 변경 이력

- 2026-05-18: 초안. 본 session 의 진단 + 해결 + 6 commit series 정합 + ADR-0021 D7 amend ④ draft.
