# 0038 — Stage 5 next-batch queue (progression marker)

- 일자: 2026-05-15
- 작성자: backend agent (0036+0037 review 결과 분석 후)
- 종류: progression marker / queue — 후속 batch 의 우선순위 + 의존성 + 예상 시간 기록. 다음 세션이 cold-pickup 으로 진입 가능하게 함.
- 기준 commit: `3d786b4` (Stage 5-A/5-B/5-D P1/D10 α 까지 ship)

---

## 0. 한 줄 요약

0037 BE review + 0036 FE review 의 후속 작업을 5 batch 로 우선순위화. **시급 (urgent-1/urgent-2)** 는 본 세션 안 진입. **next-batch** 3 항목 (5-C / session-scoped streaming / 5-D P2) 은 다음 세션 또는 FE-NEW-6 와 동시 작업 batch.

---

## 1. 작업 큐 (우선순위 + 의존성)

| ID | 작업 | 분류 | 예상 시간 | 의존 | 상태 |
|---|---|---|---|---|---|
| **urgent-1** | 0037 Finding B — 0x87 socket-level fan-out 통합 테스트 5 개 | hardening | 1~2h | 없음 | 본 세션 |
| **urgent-2** | FE Issue C unblock — `0x88 TERMINAL_SPAWNED` UUID↔PaneId binding | feature | 2~3h | 없음 | 본 세션 |
| ~~next-1~~ | ~~Stage 5-C — connection_id + session_id top-level + echo-minus-sender~~ | ~~feature~~ | ~~1~2 day~~ | 0037 §7.1 의 FE 결정 (B) 명시됨 | ✅ **본 batch 안 ship** (commit pending) — wire 는 binary varint + tail trailer (varint-len + UTF-8 session_id). 0039 §5.1 참조 |
| **next-2** | Planned C — session-scoped `pane_output` + catch-up filter | refactor | 2~3 day | FE-NEW-6 (multi-xterm subscriber) 와 동시 | 다음 batch (FE 진행 후) |
| ~~next-4~~ | ~~0040 option A — catch-up `0x88` re-emission + implicit detach-on-reattach~~ | ~~fix~~ | ~~1~2h~~ | 없음 (BE-alone) | ✅ **본 batch 안 ship** — `TerminalUuidProvider` trait + CLI 등록 + 회귀 fix. FE 변경 0 |
| ~~next-3~~ | ~~Planned D — 5-D P2 — `POST /terminals` + `0x86 MOUNT_CASCADE`~~ | ~~feature~~ | ~~1 day~~ | FE `[New Terminal]` migrate (BE 단독 land 후 wire 닫힘) | ✅ **본 batch 안 ship** — BE 가 endpoint + publisher 모두 land. FE 의 `[New Terminal]` 버튼이 `POST /api/sessions/:name/terminals` 호출만 추가하면 정합 |

---

## 2. 시급 batch 의 상세 (본 세션)

### 2.1 urgent-1 — 0x87 socket-level fan-out 통합 테스트

**근거**: 0037 §4 — hub-level + http-api-level 테스트는 있으나 *2 WS connection 의 routing 매트릭스* 미검증.

**케이스** (ws-server socket level):
1. cookie-A → session α, cookie-B → session β. `publish_terminal_list_change("α", [uuid], [])` → A 는 0x87 수신 X, B 는 수신 + payload 검증
2. cookie 없는 WS → 0x87 수신 X
3. cookie 있으나 attach 안 한 WS → 0x87 수신 X
4. client-origin 0x87 envelope 송신 → policy violation close

**산출**: `crates/ws-server/src/lib.rs` 의 `#[cfg(test)] mod tests` 에 5 단위 추가.

### 2.2 urgent-2 — `0x88 TERMINAL_SPAWNED` UUID↔PaneId binding

**근거**: 0036 FE §4 (Issue C) 가 placeholder 로 막힘. FE 의 C1 (실 streaming) 전환의 최소 enabler — FE 가 `/api/terminals` poll 없이 PaneId↔UUID 매핑 즉시 알 수 있게.

**wire**:
```
0x88 TERMINAL_SPAWNED
inner = varint 0 + UTF-8 JSON { "terminal_id": "<uuid>", "pane_id": <u64> }
```

- 라우팅: server-wide (mirror 정합 — 어떤 session 의 webpage 가 attach 됐든 binding 정보 필요)
- 발행 위치: `AppState::spawn_terminal_with_uuid` 의 register 성공 직후
- inbound 0x88 → policy violation close (server-only)

**산출**:
- `crates/ws-server/src/lib.rs` — `FrameType::TerminalSpawned = 0x88` + from_u8/as_u8
- `crates/ws-server/src/payload.rs` — `encode_terminal_spawned(uuid, pane_id)`
- `crates/ws-server/src/hub.rs` — `TerminalSpawnedEvent` + broadcast 채널 + `publish_terminal_spawned` + `subscribe_terminal_spawned`
- `crates/ws-server/src/lib.rs` — WS handler 의 새 select arm (server-wide, no cookie filter)
- `crates/http-api/src/lib.rs` — `spawn_terminal_with_uuid` 안에서 `hub.publish_terminal_spawned(uuid, pane_id.0)` 호출
- 단위 / 통합 테스트

---

## 3. 다음 batch 의 상세 (deferred)

### 3.1 next-1 — Stage 5-C echo-minus-sender

**FE 결정 (0037 §7.1)**:
- broadcast trigger: (B) echo minus sender
- sender identity: BE connection_id
- session_id 위치: top-level

**산출 예상**:
- Hub 의 connection-table 확장 — cookie → Vec<connection_id> + connection_id → session_id reverse
- WS handshake 에서 fresh connection_id 생성 + 등록
- 0x81~0x84 의 outbound 허용 (현재 inbound-only)
- 0x81~0x84 의 JSON payload 에 top-level `session_id`
- 송신자 connection_id 제외 fan-out
- FE `isFrameForActiveSession` 와 정합

**부수 효과**: 0036 FE Issue D (legacy `pane-spawned` 충돌) 자연 해소 (session-scoped routing 이 active 면 legacy auto-mount 의 trigger condition 자동 false).

### 3.2 next-2 — session-scoped pane_output streaming

**현재 문제** (0037 §5):
- `backend.pane_ids()` 의 catch-up 가 server-wide → 모든 WS subscriber 에 전달
- `pane_output` broadcast 도 server-wide

**목표**:
- WS 가 자기 session 의 terminal UUID set 알 수 있어야 함 — `state.session_cache.get_or_load(session_name)` 으로 layout 의 terminal item 모으기 → terminal_map.lookup_pane 으로 PaneId set
- catch-up 의 `backend.pane_ids()` 결과를 session set 으로 필터
- 라이브 `pane_output` arm 의 `(id, bytes)` 도 session set 멤버십 검사 후 송신

**위험**:
- legacy single-session path 와 dual-channel 유지 — `sessionStore.active == null` 일 때 legacy 전체 stream 유지
- session set 의 cache invalidation (PUT layout / attach_confirm / delete_item / kill 후 갱신)
- FE-NEW-6 의 multi-xterm subscriber 와 정합 필요 — 같은 UUID 가 여러 panel 에 mirror 될 때

**조치 의견**: BE 단독 진입 가능하나 *FE 측 XtermHost mode 'terminal' (Option C1)* 이 land 한 후 진입이 안전.

### 3.3 next-3 — 5-D P2 (`POST /api/sessions/:name/terminals` + 0x86)

**FE 정합 명세 (0036(FE) handover + 0037 §6.4)**:
- BE 가 spawn + default 좌표 결정
- FE 는 `handleMountCascade` 가 mutateLayout 호출 (FE 가 layout PUT)
- BE 는 layout 직접 persist X — `0x86 MOUNT_CASCADE` publish 만

**산출 예상**:
- `FrameType::MountCascade = 0x86` + `payload::encode_mount_cascade(uuid, x, y, w, h)`
- `POST /api/sessions/:name/terminals` 라우트 — cookie 가 lock holder 검증 → fresh UUID 생성 → spawn_terminal_with_uuid → default 좌표 계산 → MOUNT_CASCADE publish (trigger session) + TERMINAL_LIST_UPDATE publish (others) + TerminalSpawned publish (urgent-2 의 binding)
- default 좌표 정책: empty layout `(80, 80, 720, 420)`, existing `(max(x,y) + 32 cascade, prior w/h)`

**위험**: 적음. urgent-2 의 binding frame 이 land 한 후 진입 권장.

---

## 4. 비-기능 잔존 (Stage 5 외)

| 항목 | 분류 | 처리 시점 |
|---|---|---|
| Legacy `/api/layout` v1 + `LayoutStore` cleanup | refactor | FE migrate 완료 후 (0035 §3.4) |
| `LayoutSnapshot` ↔ `SessionLayout` 통합 | refactor | 위 cleanup 의 일부 |
| `--session <name>` flag 제거 | refactor | Stage 6+ — token/pid/config 명명 종속 |
| WS handshake cookie-only (D10 β/γ) | refactor | Stage 6/7 — FE deprecation 일정 후 |
| Settings API (BE-9) | feature | Stage 7 |
| Rate limiter X-Forwarded-For | feature | Cloud mode 진입 시 |
| WS subscriber Lagged reconciliation | hardening | P2+ — 주기 task |

---

## 5. cold-pickup 첫 명령

```bash
cd /Users/ws/Desktop/projects/gtmux

# 1. queue 상태
cat docs/reports/0038-stage-5-next-batch-queue.md

# 2. 최신 진실
cat docs/reports/0036-stage-5-dp1-and-d10-alpha-be-progress.md
cat docs/reports/0037-backend-review-action-items.md
cat docs/reports/0036-frontend-review-action-items.md  # FE 측 review

# 3. 현 코드 상태
cd codebase/backend
git log --oneline -5
cargo test --workspace --color=never 2>&1 | grep "test result:"
```

---

## 6. 변경 이력

- 2026-05-15: 초안 — 0037 BE review + 0036 FE review 분석 후 5 batch 우선순위화. urgent-1/urgent-2 본 세션 진입.
