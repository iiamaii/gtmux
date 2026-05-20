# 0047 — BE next-session handover (post Slice D + 0045 followup)

- 작성일: 2026-05-16
- 작성자: FE 통합 세션 agent (0045 분석 + 묶음 E ship 직후)
- 종류: BE cold-pickup handover — Slice D 전체 ship 직후 + 0046 P0 발주 후 진입점
- 우선 reading: 본 문서 §1~§3 → `0046-be-attach-handler-idempotent.md` → 본 §4 잔여
- 대상 독자: BE agent (cold-pickup, 다음 세션 진입자)

---

## 0. 한 줄 요약

Slice D-1~D-5 모두 ship 완료 + Stage 5-C session-scoped routing 완성 + legacy `/api/layout` v1 retire + ADR-0026 server identity 까지 land. **다음 BE 진입의 🔴 P0 = 0046 attach_handler same-cookie idempotent fix** — FE refresh + Phase 2 silentReattach 의 *모든* 호출 회귀의 근본 원인. 그 외 누적 BE work 는 §4 매트릭스 참조 (D6 heartbeat 미 ship 상태 등).

---

## 1. 직전 BE ship 누적 (chronological)

| commit | 내용 |
|---|---|
| `51f3a86` | **Slice D-5** graceful shutdown — `POST /api/shutdown` + WS `0x89 SERVER_SHUTDOWN` + ADR-0014 D12 |
| `e583853` | **Slice D-4** `POST /api/sessions/import` (G28) |
| `e61c4ac` | **Slice D-3** password rotation + logout-all + ADR-0020 D12 |
| `032b83a` | **Slice D-2** file_path open + ADR-0023 amend |
| `349ea2c` | **Slice D-1** settings API |
| `92a507b` | **Stage 5-C** session-scoped PANE_OUT filter + ADR-0025 Accepted |
| `21ea4ea` | Legacy `/api/layout` v1 + LayoutStore retire (Stage 6 cleanup) |
| `53f11cf` | ADR-0026 server identity (workspace-derived) + `--session` retirement |
| `65cd120` | text-align schema enums (TextAlign/TextVerticalAlign) + ws-server stale cookie log debug |

`cargo test --workspace`: 382 PASS / 0 FAIL (D-5 ship 시점).

---

## 2. 🔴 P0 — 0046 attach_handler same-cookie idempotent

**정본**: `docs/reports/0046-be-attach-handler-idempotent.md` — full work package.

### 2.1 한 줄

`attach_handler` (sessions.rs:330) 의 같은-cookie 같은-session 재attach 가 **코멘트 약속과 달리 409 CONFLICT** 반환. cookie ownership 분기 추가 필요.

### 2.2 영향 (UX critical)

1. **FE 새로고침 race**: WS close → release_lock 비동기. SPA 의 reattach POST 가 그보다 빠르면 → 409 → ReconnectModal "in_use" → 사용자가 [Retry] 클릭해야 통과
2. **plan-0008 Phase 2 silentReattach 의 *모든* 호출 fail**:
   - WS dispatcher 의 `reconnecting → open` 전이 → silentReattach → 409
   - visibility change + heartbeat.isIdle → silentReattach → 409
   - `sessionStore.lastSilentReattachResult = { kind: 'in_use' }` → mutation guard 가 *모든 mutation 진입점 차단*
   - 사용자가 같은 webpage 인데도 "Session is in use by another webpage" toast
   - **Phase 2 의 silent transparent recovery 의도 완전히 깨짐**

### 2.3 fix scope (sessions.rs)

```rust
// line 396 직전에 cookie ownership 분기 추가
{
    let by_cookie = state.session_locks_by_cookie.lock().await;
    if by_cookie.get(&cookie).map(|s| s == &name).unwrap_or(false) {
        drop(by_cookie);
        return reuse_existing_attach_response(&state, wm, &name).await;
    }
}
```

`reuse_existing_attach_response` 헬퍼 추가 — 기존 lock 유지 + layout classification 만 새로 계산 + 200 응답 (`{ attached, name, server_id, matched, unmatched }`).

### 2.4 테스트 변경

- 신규: `attach_idempotent_for_same_cookie_same_session` — 같은 cookie 로 attach 2회 → 둘 다 200
- amend: `attach_409_when_already_held_same_server` → `attach_409_when_held_by_different_cookie` rename + 2 cookie 환경 (다른 cookie 만 409 보장)

### 2.5 진행 순서

1. RED — 신규 idempotent test 추가, 실패 확인
2. GREEN — line 396 직전 cookie 분기 + reuse helper
3. amend — 기존 same-server 409 테스트를 different-cookie 환경으로
4. ADR-0019 D3 amend (코멘트가 약속한 동작을 코드로 land 했음 표기)
5. smoke (curl 2회 attach + cargo test --workspace)

### 2.6 FE 측 ship 후 정상화

0046 ship 후 FE 의 다음 동작이 자연 정상화:
- 새로고침 시 ReconnectModal "in_use" 노출 사라짐
- Phase 2 silentReattach 가 항상 200 → 사용자 모르게 transparent recovery
- mutation guard 가 차단해야 할 시점 = 실제 cookie 가 lock 잃은 경우만 (cookie expiry, BE restart 등)

---

## 3. BE next-2 (Mid-priority)

### 3.1 ADR-0019 D3 amend (0046 와 paired)

0046 코드 fix 와 함께 ADR-0019 D3 의 single-attach invariant 절을 amend — same-cookie idempotent 분기를 명시. 코드와 spec 의 contract drift 차단.

### 3.2 D6 Webpage heartbeat 구현 (ADR-0021 D6)

현재 hub.rs 에 `heartbeat_tx: Option<mpsc::UnboundedSender<String>>` slot 있으나 *plumbing 만*. 15s ping / 30s timeout 실제 구현 미 ship.

영향:
- 현재 abrupt close (browser crash, OS kill) 시 cookie 가 lock 보유 채로 leak (release_lock 은 WS close 만 trigger)
- 다음 reattach 가 0046 fix 후엔 idempotent OK 로 정상화되긴 하나 — heartbeat 가 ship 되면 leak 자체가 사라져 다른 webpage 의 attach 도 자연스럽게 가능

→ 0046 ship 후 진행. 별 work package 작성 권장 (0048?).

### 3.3 Schema v3 item.order field (0045 §10.6 + 0024 D1 amend 후속)

FE Layer list V2 의 drag reorder 가 *item 의 sibling 안 정확 위치* 를 보장하려면 `ItemCommon` 에 `order: number` field 가 필요. 현재 FE 는 parent_id reparent 만 보장, sibling order 는 id-sort 폴백.

scope:
- `crates/http-api/src/schema.rs` 의 ItemCommon 에 `order: f64` (또는 i64) 추가, Default = 0
- canvas-layout-schema.md §1 amend
- v2 → v3 migration: 기존 items 에 order=0 fill (모두 0 이면 id-sort 폴백 유지)
- validation: order 의 NaN/Infinity 거부

FE 측 wire: `LayerTreeView.commitReparent` 의 group `finalSequence` 패턴을 item 에도 적용.

---

## 4. BE work 매트릭스 (cumulative status)

| 영역 | 상태 | 비고 |
|---|---|---|
| Stage 5-A/5-B (hub session table + terminal-died) | ✅ ship | `4fb9ecb`, `0a7cd65` 외 |
| Stage 5-C (session-scoped PANE_OUT filter) | ✅ ship | `92a507b` |
| Stage 5-D P1 (terminal-list-update) | ✅ ship | `3d786b4` |
| Stage 5-D P2 (POST /terminals + 0x86 MOUNT_CASCADE) | ✅ ship | `e5606f9` |
| 0x88 TERMINAL_SPAWNED binding | ✅ ship | `d00db66` |
| Catch-up 0x88 + implicit detach-on-reattach | ✅ ship | `5932d00` |
| Slice D-1 (settings) | ✅ ship | `349ea2c` |
| Slice D-2 (file_path open) | ✅ ship | `032b83a` |
| Slice D-3 (password / logout-all) | ✅ ship | `e61c4ac` |
| Slice D-4 (sessions import) | ✅ ship | `e583853` |
| Slice D-5 (shutdown + 0x89) | ✅ ship | `51f3a86` |
| Legacy `/api/layout` v1 retire | ✅ ship | `21ea4ea` |
| ADR-0026 server identity | ✅ ship | `53f11cf` |
| text-align schema enums | ✅ ship | `65cd120` (FE 묶음 ToolbarSubbar 와 짝) |
| **🔴 0046 attach_handler same-cookie idempotent** | **❌ pending** | 본 §2 — refresh / Phase 2 직타격 |
| D6 webpage heartbeat 구현 | ❌ stub only | hub.rs slot 있으나 ping/pong 미 ship |
| Schema v3 item.order field | ❌ pending | FE Layer V2 의 item 정확 위치 의존 |
| Tier 3 — Template CRUD | ❌ pending | G36, frontend-handover-v3 의 Stage 4 amend |
| Tier 3 — Token rotation `/auth/rotate` | ❌ pending | ADR-0020, FE Settings Auth section wire 대기 |

---

## 5. Cold-pickup reading order

| # | 파일 | 목적 |
|---|---|---|
| 1 | `CLAUDE.md` | 프로젝트 메타 (priority 영역 P0 → 진행 단계 confirmation) |
| 2 | `CONTEXT.md` | 어휘 SoT |
| 3 | `docs/agents/backend-handover-v3.md` | BE v3 의 cumulative 기반 (Stage 5 진입 직전 시점) |
| 4 | `docs/reports/0046-be-attach-handler-idempotent.md` | **본 세션 우선 P0** |
| 5 | `docs/reports/0044-be-slice-d-work-package.md` (변경 이력 §10) | Slice D 전체 ship 기록 |
| 6 | `docs/reports/0045-refresh-session-reconnect-loop-analysis.md` | FE 측 분석 (0046 의 motivation context) |
| 7 | `docs/reports/0043-fe-integrated-session-handover.md` §1.14 | FE 측 묶음 E (0045 P0 FE 후속 land) |
| 8 | `docs/adr/0019-session-and-workspace-model.md` D3 | single-attach invariant — 0046 fix 와 amend 대상 |
| 9 | `docs/adr/0021-terminal-pool-and-mirror.md` D6 | webpage heartbeat (§3.2 잔여) |
| 10 | `docs/adr/0018-canvas-item-data-model.md` D3 | ItemCommon — schema v3 item.order 추가 위치 (§3.3) |

---

## 6. 빌드 / 검증 / 실행

```bash
cd /Users/ws/Desktop/projects/gtmux

# BE
cd codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | tail -10
# 기대: 382 PASS / 0 FAIL (현 baseline)

# Single attach idempotent smoke (manual, after 0046 ship)
TOKEN="<magic-link token>"
curl -s -c /tmp/cookies.txt -L "http://127.0.0.1:9999/auth/bootstrap?token=$TOKEN"
curl -s -b /tmp/cookies.txt -X POST -H "Content-Type: application/json" \
  -d '{"ws_conn_id":"t1"}' http://127.0.0.1:9999/api/sessions/<name>/attach \
  -w "\n[%{http_code}]\n"
# 기대 (0046 ship 전): 첫 호출 200, 둘째 호출 409
# 기대 (0046 ship 후): 둘 다 200
```

---

## 7. 진입 시 첫 메시지 후보

- **"0046 attach_handler same-cookie idempotent"** → 본 §2 RED-GREEN-amend 순서
- **"D6 webpage heartbeat 구현"** → §3.2 — 0046 ship 후 진입 (의존성 정합)
- **"Schema v3 item.order"** → §3.3 — FE Layer V2 의 item 정확 위치 enabler
- **"Tier 3 Template CRUD 진입"** → frontend-handover-v3 의 Stage 4 amend 부분 정합

---

## 8. 변경 이력

- 2026-05-16: 초안 — Slice D 전체 ship + Stage 5-C 완성 + legacy v1 retire + 0046 P0 발주 시점의 BE next-session handoff. FE 측 0045 분석 결과의 BE 의존 사항 격리 + 우선순위 매트릭스 정리.
