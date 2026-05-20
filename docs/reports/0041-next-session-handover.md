# 0041 — Next-session handover (Stage 5 BE 마감 직후)

- 일자: 2026-05-16
- 작성자: backend agent (`5932d00` 직후, 본 세션의 마지막 산출)
- 종류: **cold-pickup brief** — 다음 세션이 *현 진행 상황 + 향후 계획 + 핵심 의사 결정* 을 한 문서로 진입 가능하게 만드는 합본
- 후속 reading order: 본 문서 → `0042` (Slice D BE work-package — FE Slice A/B/C ship 후 BE 의존 endpoint 매트릭스) → `0040` (terminal↔panel 검증) → `0039` (FE 연동 가이드) → `0038` (큐) → `0036(BE)` (5-D P1+D10 α 진행) → `0034` (5-A/5-B 진행) → `0032` (Stage 4 진실) → `0031` (Stage 1~3 진실)
- **amend (2026-05-16, post-handover)**: `c60ba43` 후속 세션이 ADR-0025 (session-scoped pane_output filter) 초안 작성 + `0042` 의 Slice D 매트릭스 반영. §5.1 의 우선순위 표 갱신, §5.2 의 next-2 가 ADR-0025 로 승격. 본 amend 후 우선순위 = **Slice D-1 (Settings API minimal) > D-2 (file_path open) > D-3 (Auth) > D-4 (Import/Export) > next-2 (ADR-0025) > D-5 (Server shutdown)**.

---

## 0. 한 줄 요약 + 현 위치

본 세션에서 **Stage 5-A, 5-B, 5-D P1, D10 α, 5-C, 5-D P2, 0040 option A** 가 모두 BE 에 ship — `bcd54de`(Stage 1~4) 이후 7 commit 추가, **261 → 329 PASS / 0 FAIL** (+68 테스트). **Terminal ↔ canvas panel 의 end-to-end 흐름이 모든 시나리오 (fresh spawn, page reload, WS reconnect, cookie session 전환) 에서 BE-측에서 완전 보장**. FE 측 작업 *추가 요구 없음* — 작업트리의 0x85/0x86/0x87/0x88 decoder + handler + `XtermHost` dual-mode + `terminalPool.bindPaneId` 이 본 BE 와 정합.

```
HEAD → 5932d00  feat(backend): 0040 option A — catch-up 0x88 + implicit detach-on-reattach
       3b25ba9  docs: 0040 terminal↔panel 연동 검증 — reload/reconnect gap 식별
       e5606f9  feat(backend): Stage 5-D P2 — POST /terminals + 0x86 MOUNT_CASCADE
       47365fd  feat(backend): Stage 5-C — echo-minus-sender + session-scoped routing
       3aaf840  docs: 0039 FE 연동 가이드 — Stage 5 BE wire surface (HEAD d00db66)
       d00db66  feat(backend): 0x87 routing tests + 0x88 TERMINAL_SPAWNED binding
       3d786b4  feat(backend): Stage 5-D P1 + D10 α — terminal-list-update + cookie WS auth
       4fb9ecb  feat(backend): Stage 5-A/5-B — hub session table + terminal-died frame
       03056bb  fix(backend): preserve terminal metadata across kill+respawn cycle
       bcd54de  feat(backend): multi-session pivot stages 1-4
```

다음 BE 단독 진입 가능 항목: 모두 *대기 권장* — 본 세션의 BE 가 FE 와의 동등 정합에 도달. FE-NEW-6 (multi-xterm subscriber) 진행 또는 추가 외부 결정 후 진입.

---

## 1. 본 세션 진행 흐름 (chronological)

| # | commit | 작업 | 산출 | PASS |
|---|---|---|---|---|
| 1 | `4fb9ecb` | Stage 5-A + 5-B | Hub `session_table` (cookie→session_name) + `TerminalDied` 0x85 frame + 5 socket-level tests | 261 → 278 |
| 2 | `3d786b4` | Stage 5-D P1 + D10 α | `TERMINAL_LIST_UPDATE` 0x87 + WS cookie additive auth (`CookieValidator` trait) | 278 → 292 |
| 3 | `d00db66` | urgent — 0x87 routing tests + 0x88 binding | socket-level routing 5 tests + `TerminalSpawned` 0x88 frame (UUID↔PaneId binding) | 292 → 303 |
| 4 | `3aaf840` | docs: 0039 FE 연동 가이드 | wire 표 + FE 작업 매트릭스 + Issue C 마이그레이션 path | (docs) |
| 5 | `47365fd` | Stage 5-C echo-minus-sender | `ManipulationEvent` + connection_id minting + 0x81~0x84 inbound→broadcast (binary varint + tail trailer) | 303 → 309 |
| 6 | `e5606f9` | Stage 5-D P2 | `POST /api/sessions/:name/terminals` + `MountCascade` 0x86 + default cascade coord 정책 | 309 → 318 |
| 7 | `3b25ba9` | docs: 0040 검증 | end-to-end 추적 + reload/reconnect gap 식별 | (docs) |
| 8 | `5932d00` | 0040 option A + 회귀 fix | `TerminalUuidProvider` trait + WS catch-up 0x88 재발행 + `attach_handler` implicit detach-on-reattach | 318 → 329 |

전체 commit 8 개 (BE 6 + docs 2). 작성된 doc 4 개 (0038/0039/0040/0041).

---

## 2. 누적 완료 상태

### 2.1 Stage 별 진실 출처

| Stage | 진실 문서 | 한 줄 |
|---|---|---|
| 1 (Foundation) | 0031 §2 | Workspace + Session storage + path resolution + v1→v2 migration |
| 2 (Auth lifecycle) | 0031 §2 | cookie + token + password (Argon2id) + RateLimiter + session_table |
| 3 (Cross-server lock) | 0031 §2 | flock + lease + WS heartbeat 15s/30s + cookie-driven auto-release |
| 4 (Terminal pool + pivot) | 0032 §2 | UUID bridge + match-or-spawn + kill/respawn + auto-unregister |
| 4-C (cleanup) | 0033 §1.5 | metadata preservation fix + PATCH label |
| **5-A** | 0034 §2 | Hub `session_table` cookie → session_name |
| **5-B** | 0034 §3 | `0x85 TERMINAL_DIED` (UUID-carrying, server-wide) |
| **5-D P1** | 0036(BE) §2 | `0x87 TERMINAL_LIST_UPDATE` + attach_confirm publisher |
| **D10 α** | 0036(BE) §3 | WS cookie additive auth (`CookieValidator` trait) |
| **urgent-1** | (d00db66) | 0x87 socket-level fan-out tests |
| **urgent-2** | (d00db66) | `0x88 TERMINAL_SPAWNED` UUID↔PaneId binding |
| **5-C** | (47365fd) | 0x81~0x84 echo-minus-sender + binary varint + tail trailer |
| **5-D P2** | (e5606f9) | `POST /terminals` + `0x86 MOUNT_CASCADE` |
| **0040 option A** | 0040 §0 + (5932d00) | WS catch-up 0x88 재발행 + implicit detach-on-reattach |

### 2.2 테스트 trajectory

| 시점 | PASS | 증분 |
|---|---|---|
| Stage 1~3 종료 (0031) | 230 | — |
| Stage 4 + 4-C 종료 (0033) | 261 | +31 |
| 본 세션 시작 | 261 | — |
| 본 세션 종료 (`5932d00`) | **329** | **+68** |

전체 +68 분배:
- ws-server: 63 → 109 (+46) — 본 세션의 6 wire frame + connection_id + cookie validator + provider trait + multi 통합 단위 + socket-level routing tests
- http-api: 130 → 150 (+20) — 신규 endpoint + 통합 tests
- pty-backend / config / cli / auth: 무변동

### 2.3 WS frame 표 (본 세션 종료 시)

```
0x01 CTRL                   (bidi, 변경 X)
0x02 PANE_OUT               (server-only, 변경 X)
0x03 PANE_IN                (client-only, 변경 X)
0x04 PANE_RESIZE            (client-only, 변경 X)
0x05 PANE_PAUSE             (client-only, 변경 X)
0x06 PANE_RESUME            (client-only, 변경 X)
0x07 NOTIFY_MIRROR          (server-only, 변경 X)
0x80 LAYOUT_CHANGED         (server-only, legacy v1)
0x81 M_CHANGED              ★ inbound + outbound (5-C, echo-minus-sender)
0x82 I_CHANGED              ★ inbound + outbound (5-C)
0x83 VIEWPORT_CHANGED       ★ inbound + outbound (5-C)
0x84 FOCUS_MODE             ★ inbound + outbound (5-C)
0x85 TERMINAL_DIED          ★ server-wide (5-B)
0x86 MOUNT_CASCADE          ★ trigger-session-only (5-D P2)
0x87 TERMINAL_LIST_UPDATE   ★ non-trigger-session (5-D P1)
0x88 TERMINAL_SPAWNED       ★ server-wide UUID↔PaneId binding
0x89 SERVER_SHUTDOWN        ★ server-wide (Slice D-5, ADR-0014 D12)
0x8A~ unassigned
```

### 2.4 HTTP endpoint surface (본 세션 종료 시)

```
GET    /healthz                                  # no auth
GET    /auth                                     # token verify or password form
GET    /auth/bootstrap                           # legacy → 303
POST   /auth/login                               # { token | password } + cookie
POST   /auth/logout                              # revoke

GET    /api/layout                               # legacy v1 (FE 가 active 사용)
PUT    /api/layout                               # ETag CAS (legacy)

GET    /api/sessions                             # list + active
POST   /api/sessions                             # create
DELETE /api/sessions/:name                       # delete record
GET    /api/sessions/:name/layout                # v2 + ETag
PUT    /api/sessions/:name/layout                # v2 ETag CAS
POST   /api/sessions/:name/attach                # flock + matched/unmatched 반환 (+ 5932d00 의 implicit detach-on-reattach)
DELETE /api/sessions/:name/attach                # flock release
POST   /api/sessions/:name/attach/confirm        # unmatched spawn (+ 5-D P1 의 0x87 publish)
POST   /api/sessions/:name/terminals             ★ 5-D P2 (`[New Terminal]` button) + 0x86/0x87/0x88
DELETE /api/sessions/:name/items/:id             # ?kill_terminal=bool

GET    /api/terminals                            # pool + meta + cross-ref
PATCH  /api/terminals/:id                        # { label }
POST   /api/terminals/:id/kill                   # explicit
POST   /api/terminals/:id/respawn                # same UUID, fresh PaneId
```

WS: `/ws` — subprotocol `gtmux.v1`, bearer or cookie auth (D10 α additive), connection_id 자동 발급.

---

## 3. AppState / Hub 의 최종 모양

### 3.1 AppState (변동 없음 — 본 세션 안)

`AppState` 의 15 필드는 0036(BE) §6.1 의 모양 그대로. 본 세션은 `Hub` 의 hook + WS handler 의 로직 확장만 했고 `AppState` 의 자료구조는 무변동.

### 3.2 Hub (본 세션 의 7 가지 확장)

```rust
pub struct Hub {
    // 기존 (Stage 4 까지)
    backend, pane_output, layout_events, _mux_task,
    disconnect_tx, heartbeat_tx,

    // Stage 5-A
    session_table: Arc<std::sync::RwLock<HashMap<String, String>>>,

    // Stage 5-B
    terminal_died_events: broadcast::Sender<TerminalDiedEvent>,

    // Stage 5-D P1
    terminal_list_change_events: broadcast::Sender<TerminalListChangeEvent>,

    // urgent-2
    terminal_spawned_events: broadcast::Sender<TerminalSpawnedEvent>,

    // 5-C
    manipulation_events: broadcast::Sender<ManipulationEvent>,

    // 5-D P2
    mount_cascade_events: broadcast::Sender<MountCascadeEvent>,

    // D10 α
    cookie_validator: Arc<Mutex<Option<Arc<dyn CookieValidator>>>>,

    // 0040 option A
    terminal_uuid_provider: Arc<Mutex<Option<Arc<dyn TerminalUuidProvider>>>>,
}
```

7 신규 broadcast/hook + 6 신규 publish/subscribe API + 2 신규 trait (CookieValidator, TerminalUuidProvider).

---

## 4. FE 측 정합 상태 (작업트리, 미커밋)

본 세션이 시작될 때 FE 측 작업트리는 이미 다음을 보유:

| FE 컴포넌트 | 상태 |
|---|---|
| `lib/ws/decode.ts` 의 0x85/0x86/0x87/0x88 decoder | ✅ 작성됨 |
| `lib/ws/dispatcher.svelte.ts` 의 handler 4 개 | ✅ 작성됨 |
| `lib/stores/terminalPool.svelte.ts` 의 `paneIdByUuid` map + `bindPaneId` | ✅ |
| `lib/canvas/PanelNode.svelte` 의 3-way mount (legacy / terminal mode / pending) | ✅ |
| `lib/canvas/XtermHost.svelte` 의 numeric paneId 수신 (UUID resolve 는 PanelNode 책임) | ✅ |
| `lib/canvas/Canvas.svelte` 의 `spawnMultiSessionTerminal` (manual UUID + mutateLayout + attach_confirm) | ✅ |
| `lib/canvas/Canvas.svelte` 의 `handleTerminalClick` legacy/multi-session 분기 | ✅ |
| FE Issue A (token cookie 교환) | 0036(FE) §2 의 작업 미진행 (FE agent 책임) |
| FE Issue B (terminal panel close) | 0036(FE) §3 의 작업 미진행 |
| `isFrameForActiveSession` scaffold (5-C) | ✅ helper 있음 (decoder amend 는 BE wire 확정 후) |

**= FE 가 BE-측 작업 land 만 기다리는 상태**. 본 세션의 BE 완성 후, FE 작업트리 그대로 *page reload / WS reconnect / fresh spawn / cookie session 전환* 모두 정합.

본 세션 안 BE 가 작성한 FE 정합 doc:
- 0039 §5.1 (5-C wire 결정 — binary varint + tail trailer + decoder amend 사양)
- 0039 §5.3 (5-D P2 endpoint + wire + persistence 모델)
- 0040 §3.1 (옵션 A 채택 시 FE 요구 0)

**FE 미진행 작업** (FE 자체 책임):
- 0036(FE) Issue A: `/?t=<token>` cookie login 교환
- 0036(FE) Issue B: multi-session terminal panel close 의 v2 분기
- 0036(FE) Issue D: legacy `pane-spawned` auto-mount 의 active-session guard
- 5-C decoder 의 sessionId trailer 처리 (FE 가 outbound 송신 시작할 때)

---

## 5. 향후 구현 계획

### 5.1 BE 단독 가능 / 권장 대기 (2026-05-16 amend — 0042 Slice D 의 FE 의존 unblock 우선)

> **amend rationale**: 본 §의 원안은 *Stage 5 BE 마감 직후* 의 시점 — FE 가 Slice A/B/C 를 ship 하기 *전*. 후속 세션이 `0042-be-slice-d-work-package.md` 를 받아 보면, FE 측 Settings overlay 의 4 section (Storage/Auth/Behavior/Debug) + FilePathNode + ServerShutdownConfirmModal 이 *BE endpoint placeholder* 로 ship 됨 — BE 의 endpoint 가 ship 되면 *FE 의 wire 가 즉시 해제*. 따라서 next-2 (ADR-0025) 보다 *Slice D 의 unblock 가치 우위*. 표 갱신:

| 작업 | BE 단독? | 진행 권장 (2026-05-16 amend) |
|---|---|---|
| **Slice D-1 (Settings API minimal)** | ✅ 단독 | ✅ **SHIPPED 2026-05-16** — `crates/http-api/src/settings.rs` (+8 test, 337 PASS workspace). ADR-0020 D11 신규. FE Settings overlay Debug + Behavior section unblock 됨 |
| **Slice D-2 (file_path open, ADR-0023)** | ✅ 단독 | ✅ **SHIPPED 2026-05-16** — `crates/http-api/src/file_open/` (mod + allowlist + audit + spawn + handlers). +31 unit test (workspace 337 → 368 PASS) + smoke gate 5-9. ADR-0023 amend ① (0044 wire 정합 + nonce P1+ defer + JSON encoding) |
| **Slice D-3 (Auth Stage 7)** | ✅ 단독 | ✅ **SHIPPED 2026-05-16** — `crates/http-api/src/settings.rs::password_handler` + `logout_all_handler`. ADR-0020 D12 신규 amend. SessionTable::revoke_others. 368 → 375 PASS workspace 안 +7 unit test |
| **Slice D-4 (Import/Export G28)** | ✅ 단독 | ✅ **SHIPPED 2026-05-16** — `crates/http-api/src/sessions.rs::import_handler`. 375 → 380 PASS workspace 안 +5 unit test + smoke gate 5-11. 새 ADR amend 없음 (sketch §11.2.A + ADR-0018/0019 재사용). Export 는 작업 0 (기존 endpoint) |
| **next-2 ADR-0025 (session-scoped `pane_output` filter)** | ✅ 단독 | ✅ **SHIPPED 2026-05-16** — `SessionPaneSetProvider` trait + impl on AppState + WS handler 의 per-WS owned set + 4 hot-update hook (layout / terminal_died / terminal_spawned / session_change). ADR-0025 Accepted (amend ②). 382 → 388 PASS workspace 안 +6 unit test (`two_sessions_have_disjoint_sets`, cross-session mirror invariant 검증). legacy demo path 보존 = 기존 12 smoke gate regression-free |
| **Slice D-5 (Server shutdown Tier 3)** | ✅ 단독 | ✅ **SHIPPED 2026-05-16** — `crates/http-api/src/shutdown.rs` + `crates/ws-server/src/{lib,hub,payload}.rs` (FrameType::ServerShutdown 0x89). ADR-0014 amend ② (D12 신규). 380 → 382 PASS workspace + smoke gate 5-12 (202 + 0x89 + close 1000 + exit 6) |
| `--session <name>` flag 제거 | ✅ ADR draft 완료 | 📐 **DRAFTED 2026-05-16** — `docs/adr/0026-server-identity-and-session-flag-retirement.md` (Proposed). Workspace-derived machine_id (sha256(canonical_path)[..6]) 채택. Phase 1 (rename + dep warning) + Phase 2 (compile-time 제거). 코드 진입은 별 batch — 11 file + 1-2 일 추정 |
| WS handshake D10 β (FE deprecation) | 의존 | FE 가 subprotocol 송신 중단 결정 후 |
| WS handshake D10 γ (BE bearer 폐기) | 의존 | β 완료 후 |
| Legacy `/api/layout` v1 + `LayoutStore` cleanup | ✅ 단독 | ✅ **SHIPPED 2026-05-16** — FE 가 v2 완전 migrate 확인 후 BE 측 dead code 제거. `AppState.layout` / `.store` 필드, `LayoutSnapshot` struct, `storage.rs` 모듈, `layout_get_handler` / `layout_put_handler` + 4 helper 모두 제거. `AppState::with_hub_and_path()` constructor 제거 (`with_hub_and_workspace` 가 대체). ADR-0006 변경이력 amend. 23 v1 unit test 제거, 388 → 365 PASS, smoke 12/12 PASS regression-free |
| `LayoutSnapshot` ↔ `SessionLayout` 통합 | — | 위 cleanup 으로 자동 해소 — `LayoutSnapshot` 자체가 사라져 통합 의무 소멸 |
| Rate limiter X-Forwarded-For 신뢰 | ❌ edge | Cloud mode 진입 시 |
| WS subscriber Lagged reconciliation | ❌ P2+ | 주기 task 설계 |

권장 ship 순서 (cold-pickup 즉시 진입): ~~**D-1**~~ → ~~**D-2**~~ → ~~**D-3**~~ → ~~**D-4**~~ → ~~**D-5**~~ → ~~**next-2 (ADR-0025)**~~. (D-1~D-5 + next-2 = 모두 ship 완료. 잔여 = Stage 6+ deferred 항목들 — D10 β/γ / `--session` flag 제거 / legacy `/api/layout` cleanup)

각 Slice 의 endpoint wire shape / FE consumer / 진입 checklist 는 `docs/reports/0042-be-slice-d-work-package.md` §3 + §5 의 단일 진실.

### 5.2 next-2 (session-scoped pane_output) 구체 명세 — 진행 결정 시

> **2026-05-16 amend**: 본 절의 원안 명세는 후속 세션이 `docs/adr/0025-session-scoped-pane-output-filter.md` 로 *Proposed 상태 ADR 로 승격* 완료. ADR-0025 가 D1~D9 결정 + 4 대안 + 위험표 + 6-step 도입 단계의 단일 진실. 본 절은 *원안 명세의 historical record* 로 보존하되, 실제 진입 시 ADR-0025 를 참조. grilling (grill-with-docs) 결과의 3 inline amend (catch-up replay 정책 + set ownership 옵션 B + broadcast scope vs fanout layer 구별 + race-3 set 갱신 ordering) 포함.

목표: server-wide `pane_output` broadcast 를 *session 별 필터링* 으로 전환. session A 의 PANE_OUT bytes 가 session B 의 attached webpage 로 leak 되지 않게.

#### 5.2.1 현재 흐름

```
[backend.spawn] → per-pane broadcast → run_multiplexer → hub.pane_output
                                                          ↓ server-wide
                                  모든 WS subscriber 에게 (PaneId, Bytes)
```

각 WS 가 자기 `paneOutHandlers.get(key)` 로 필터 (paneId 기반). 같은 session 의 panel 만 mount 됐으므로 자연 격리 (FE 의 XtermHost 가 subscribe). 하지만:
- bandwidth: 모든 session 의 bytes 가 모든 WS 에 전달
- isolation 보장 X (FE 에서 paneId 매칭 누락 시 leak)

#### 5.2.2 목표 흐름

```
[backend.spawn] → per-pane broadcast → run_multiplexer → hub.pane_output
                                                          ↓ server-wide

WS handler (per connection):
  cookie → session_for_cookie → session_name
  session_terminal_uuid_set = layout 의 terminal UUID 들
  PaneId 매핑 = TerminalMap 으로 UUID → PaneId resolve

  pane_output recv:
    if (PaneId 이 my_session_terminal_paneid_set 안) → emit PANE_OUT
    else → drop
```

#### 5.2.3 핵심 구현 항목

1. **WS connection 의 session terminal set 관리**: WS 가 자기 session 의 terminal UUID list 를 알아야 함. 출처:
   - (a) WS handshake 시 session_cache + terminal_map snapshot (cold)
   - (b) attach/confirm/delete_item/kill/respawn 마다 갱신 — hub broadcast 로 알림 (warm)
   - (c) layout PUT 마다 갱신 — 0x80 LAYOUT_CHANGED 의 hook
   - (d) 0x87 TERMINAL_LIST_UPDATE / 0x86 MOUNT_CASCADE / 0x88 TERMINAL_SPAWNED 의 hook

   권장: **(a)+(d)** — handshake 시 cold load + 이벤트 발생 시 hot update. (a) 만으로는 새 spawn 즉시 추적 불가.

2. **TerminalMap 의 reverse-lookup**: PaneId → UUID 가 이미 있음 (snapshot 의 by_pane). 매핑 시 사용.

3. **legacy single-session path 보존**: `sessionStore.active == null` 시 server-wide 그대로. WS handler 의 session_for_cookie 가 None 이면 *모든 PANE_OUT 통과* — 기존 demo path 보존.

4. **catch-up replay 의 session filter**: 본 세션 의 catch-up 도 session 의 terminal 만 replay. 단 0x88 catch-up (option A) 도 session filter 필요? *아니* — 0x88 은 binding 정보, session 무관하게 모든 webpage 에 필요 (FE 가 mirror 표시할 가능성). 0x88 은 server-wide 유지.

5. **FE-NEW-6 정합**: 같은 UUID 가 multiple panel 에 mirror 될 때 `registerPaneOut` 의 multi-subscriber 패턴 (ADR-0021 D1). BE 의 server-wide PANE_OUT broadcast 가 그대로 FE 에 도달 — FE 가 자기 panel 마다 따로 register 한 handler 에 fan-out.

#### 5.2.4 위험

| 위험 | 완화 |
|---|---|
| legacy demo path 회귀 | `sessionStore.active == null` 시 filter X — 기존 그대로. 회귀 test 추가 |
| FE 의 multi-panel mirror 의 session-scoped filter 가 BE 의 paneId 매핑 stale 일 때 leak | layout PUT 의 LAYOUT_CHANGED 가 hook → session terminal set 갱신 |
| dangling terminal 의 PANE_OUT (kill 후 ring buffer 잔여) | 0x85 TERMINAL_DIED 가 이미 처리 — terminalPool.refresh 후 PaneId 등록 해제 |
| 매핑 갱신 timing race | 보수적: session terminal set 은 "현재 layout 에 있는 UUID" 의 superset 만 허용. layout PUT 후 갱신은 짧은 race window 가능 — 결과는 *bytes drop* 또는 *짧은 leak*. 후자가 안전 |

#### 5.2.5 예상 작업량

2~3 일. FE-NEW-6 과 동시 작업 권장 (BE 단독 진입 위험).

### 5.3 다른 deferred 의 구체 명세

#### 5.3.1 `--session <name>` flag 제거 (Stage 6+) — **ADR-0026 draft 완료 (2026-05-16)**

> 본 §의 원안 (3 대안: workspace name / port / random UUID) 은 `docs/adr/0026-server-identity-and-session-flag-retirement.md` 로 design lock 완료 (Proposed status). 결정: **workspace-derived `machine_id = sha256(canonical_workspace_path)[..6]` (12 hex)** + human label = last segment of workspace. Phase 1 (rename `--session` → `--name`, deprecation warning) + Phase 2 (Stage 7+ compile-time 제거) 단계 진행.
>
> Phase 1 코드 진입 시 변경 매트릭스 (ADR-0026 D8.1): `config/src/lib.rs` (session → name), `bin/gtmux-cli/src/state_files.rs` (4 path function), `bin/gtmux-cli/src/main.rs` (14 callsite), `process_audit.rs` (boot_scanner), 신규 `bin/gtmux-cli/src/instance_id.rs`. 추정 1-2 일.
>
> 본 §의 historical record 유지 — 원안 검토의 4 path/env 종속 + 3 대안 비교 그대로 보존. ADR-0026 §D6 (R26-A/B/C/D) 가 그 4 대안의 거절 사유 명세.

CLI 의 4 곳 종속:
- `${XDG_STATE_HOME}/gtmux/<session>.pid` (pidfile path)
- `${XDG_STATE_HOME}/gtmux/<session>.token` (token path)
- `${XDG_CONFIG_HOME}/gtmux/<session>.config.toml` (per-session config)
- tracing/log context 의 session label

대안: 1) workspace name 기반 (workspace 가 multi-session) 2) port 기반 3) random UUID

scope: 큰 refactor — 본 batch 외 별 stage.

#### 5.3.2 D10 β/γ (cookie-only WS auth)

- β: FE 가 subprotocol bearer 송신 중단. BE 무변경 — 이미 cookie path 가 통과
- γ: BE 의 bearer 검증 path 폐기. 단 CLI/automation 의 자동화 진입은 별 대안 필요 (e.g., cookie 자동 발급 endpoint `POST /auth/login` + 응답의 cookie 사용)

순서: FE β 먼저 → BE γ 후속.

#### 5.3.3 Legacy `/api/layout` cleanup

FE 가 `lib/http/layout.ts` 에서 v1 사용 중. v2 (`/api/sessions/:name/layout`) 로 완전 migrate 후:
- BE `/api/layout` route 제거
- `LayoutStore` + `LayoutSnapshot` 제거
- `AppState.store` / `AppState.layout` 필드 제거 — `SessionCache` 만 유지

#### 5.3.4 Settings API (Stage 7 BE-9)

ADR-0020 D11 의 spec. 4 endpoint:
- `GET /api/settings` — 현재 설정
- `PATCH /api/settings` — 부분 갱신
- `POST /api/settings/password` — password 설정 / 변경
- `POST /api/settings/logout-all` — session_table.revoke_all

---

## 6. 핵심 결정 / 회로 (본 세션 안 굳어진 것)

| 영역 | 결정 | 출처 |
|---|---|---|
| Hub 의 callback trait 패턴 | `CookieValidator` / `TerminalUuidProvider` 둘 다 `Arc<Mutex<Option<Arc<dyn ...>>>>` + setter/getter. dep 그래프 acyclic (ws-server 가 trait 정의, http-api 가 impl) | 0036(BE) §3.2 + 0040 §5 |
| WS handshake auth | 둘 다 valid 시 accept (permissive). bearer present-but-invalid + cookie valid = accept (reviewer 의 strict 권장과 다름) | 0036(BE) §2.1 |
| connection_id 생성 | monotonic counter `conn-{n}`. server boot 내 unique. 외부 노출 X | (47365fd) |
| 5-C wire | binary varint 유지 + tail trailer `varint(len) + UTF-8 session_id`. FE decoder strict-length 완화 필요 | 0039 §5.1 |
| 5-D P2 persistence | BE 가 layout 에 직접 write X. FE 의 `handleMountCascade` 가 mutateLayout → PUT | 0039 §5.3.5 + 0037 §6.4 |
| 5-D P2 default coords | empty: `(80, 80, 720, 420)`. existing: `max(x,y) + 32` cascade. race window 허용 | 0039 §5.3.1 |
| 0x86 MOUNT_CASCADE routing | trigger session only (`hub.session_for_cookie == trigger_session` filter) | (e5606f9) |
| 0x87 TERMINAL_LIST_UPDATE routing | non-trigger session (trigger 의 webpage 는 자기 layout 갱신 이미 했음) | 0036(BE) §2 |
| 0x88 TERMINAL_SPAWNED routing | server-wide (mirror 정합) | (d00db66) |
| catch-up 0x88 emission 순서 | provider.alive_bindings → for each emit BEFORE pane-spawned NOTIFY + PANE_OUT replay (FE 가 binding 받은 후 register) | 0040 §5.2 |
| `attach_handler` 의 implicit detach | cookie 가 다른 session 으로 attach 전환 시 prev 정리. same-name 은 no-op (409 lock_conflict) | (5932d00) |

---

## 7. file 인벤토리 (Stage 5 안에서 핫스팟)

| 파일 | 본 세션 안 책임 | 다음 작업 시 변경 가능성 |
|---|---|---|
| `crates/ws-server/src/hub.rs` | 7 broadcast/hook + 2 trait + 모든 publisher API. 본 세션 의 최대 변경 surface | next-2 시 session-terminal-set 또는 비슷한 hook 추가 가능 |
| `crates/ws-server/src/lib.rs` | FrameType 5 신규 + WS handshake auth + select! 6 arm + handle_client_envelope 8 args + catch-up 0x88 | next-2 시 PANE_OUT arm 의 session filter |
| `crates/ws-server/src/payload.rs` | encode_* 5 신규 (terminal_died / terminal_list_update / terminal_spawned / mount_cascade / viewport_marker_only) | 변경 적음 |
| `crates/http-api/src/lib.rs` | `AppState::spawn_terminal_with_uuid` 의 0x88 publish + 새 route mount | 변경 적음 |
| `crates/http-api/src/sessions.rs` | attach_handler 의 implicit detach + create_terminal_handler (5-D P2) + next_mount_cascade_coords | 변경 적음 |
| `crates/http-api/src/terminal_map.rs` | TerminalUuidProvider impl | 변경 적음 |
| `crates/http-api/src/auth.rs` | SessionTable 의 CookieValidator impl | D10 β/γ 시 변경 |
| `bin/gtmux-cli/src/main.rs` | hub.set_cookie_validator + hub.set_terminal_uuid_provider | next-2 시 추가 provider 등록 가능 |
| `crates/http-api/src/terminals.rs` | 변경 없음 (Stage 4-B 의 모양) | (선택) `/api/terminals` 응답에 pane_id 추가 (옵션 B 보완) |

---

## 8. 빌드 / 테스트 / smoke 명령

### 8.1 표준

```bash
cd /Users/ws/Desktop/projects/gtmux/codebase/backend

# 빌드
cargo build --workspace --color=never
cargo build --release --bin gtmux --color=never

# 테스트 (329 PASS / 0 FAIL)
cargo test --workspace --color=never 2>&1 | grep "test result:"

# clippy (pre-existing 2 warnings 외 무)
cargo clippy -p gtmux-ws-server -p gtmux-http-api --no-deps --color=never
```

### 8.2 release binary + WS sanity (5-D P2 + option A 검증)

```bash
env -u TMUX ./target/release/gtmux start \
  --session demo \
  --port 9999 \
  --workspace /tmp/ws-demo

TOKEN=$(cat ~/.local/state/gtmux/demo.token)

# 5-D P2: POST /terminals
COOKIE=$(curl -sS -i -X POST http://127.0.0.1:9999/auth/login \
  -H "Host: 127.0.0.1:9999" -H "Origin: http://127.0.0.1:9999" \
  -H "Content-Type: application/json" \
  -d "{\"token\":\"$TOKEN\"}" \
  | awk -F': ' '/^set-cookie: gtmux_auth=/ { sub(/;.*/,"",$2); print $2 }' | head -1)

curl -sS -X POST http://127.0.0.1:9999/api/sessions/demo/attach \
  -H "Cookie: $COOKIE" -H "Host: 127.0.0.1:9999"

curl -sS -X POST http://127.0.0.1:9999/api/sessions/demo/terminals \
  -H "Cookie: $COOKIE" -H "Host: 127.0.0.1:9999" \
  -H "Content-Type: application/json" -d '{}'
# → 200 { "terminal_id": "...", "pane_id": 1, "x": 80, "y": 80, "w": 720, "h": 420 }

# option A: WS catch-up 0x88 verify (wscat)
wscat --header "Cookie: $COOKIE" --subprotocol "gtmux.v1" -c "ws://127.0.0.1:9999/ws"
# → 0x80 LAYOUT_CHANGED hello
# → 0x88 TERMINAL_SPAWNED for each alive UUID  ★ option A
# → 0x07 NOTIFY pane-spawned
# → 0x02 PANE_OUT (shell prompt)
```

### 8.3 자동 smoke

기존 `/tmp/gtmux-smoke-stage4.sh` 이 4-A/4-B/4-C/4-D 의 7~9 gate 자동 검증. Stage 5 신규 gate 자동화는 미작성 — 필요 시 추가.

---

## 9. 다음 세션 진입 순서

### 9.1 reading order

1. **본 문서 §0 + §1 + §2** — 한 줄 + 진행 흐름 + 누적 상태
2. **0040 §0-§5** — terminal↔panel 검증 결과 (option A ship 완료 상태)
3. **0039 §1-§5** — FE 연동 wire 표 + Issue C 마이그레이션 path
4. **0038** — 큐 (모든 next 완료, next-2 만 대기)
5. **0036(FE)** — FE 측 review (Issue A/B/C/D)
6. **0037** — BE review (모두 land 됨 — Finding A/B + Planned C/D/E 의 상태)

### 9.2 첫 명령

```bash
cd /Users/ws/Desktop/projects/gtmux

# 1. 본 handover
cat docs/reports/0041-next-session-handover.md

# 2. 후속 진실
cat docs/reports/0040-terminal-panel-integration-verification.md

# 3. 현 코드 상태
cd codebase/backend
git log --oneline -10
cargo test --workspace --color=never 2>&1 | grep "test result:"
# expected: 329 PASS / 0 FAIL

# 4. 미커밋 (FE + agent system files — BE 책임 밖)
git status --short
```

### 9.3 다음 작업 결정 매트릭스

| 시나리오 | 진입 작업 |
|---|---|
| FE-NEW-6 (multi-xterm subscriber) 가 land | next-2 session-scoped pane_output filter |
| Stage 6 정합 시점 | `--session` flag 제거 + legacy `/api/layout` cleanup (FE migrate 후) |
| Stage 7 진입 시점 | Settings API (BE-9) + D10 β/γ |
| 외부 issue (bug report 등) | 우선 fix |
| 정체기 | smoke 자동화 / 회귀 테스트 보강 / 본 문서 §5 의 deferred 정리 |

---

## 10. 미커밋 (의도적 — BE 책임 밖)

```
M codebase/frontend/**                   # FE agent 의 병렬 작업
M skills-lock.json                       # skills system
?? .agents/                              # agent system
?? .claude/                              # claude config
?? .codex/                               # codex config
?? AGENTS.md                             # agent meta
?? docs/agents/frontend-handover{,-v2,-v3}.md  # FE handover
?? docs/demo-guide.md / docs/demo/       # demo
?? experiments/ / ref/                   # workspace artifacts
```

본 handover 는 BE 의 책임만 다룸. FE handover 는 별도 진실 (FE agent 의 작업트리 안 `docs/agents/frontend-handover-v3.md`).

---

## 11. 핵심 결정 요약 (cold-pickup 시 즉시 필요)

본 세션 동안 굳어진 invariant — 다음 세션이 깨면 안 되는 약속:

1. **Hub trait 패턴**: 새 cross-crate hook 은 `CookieValidator` / `TerminalUuidProvider` 와 같이 trait 정의 (ws-server) + impl (http-api). 직접 type 의존 X
2. **wire backward compat**: 새 frame 은 추가만 (기존 frame 의 inner shape 변경 X). 5-C 의 tail trailer 도 backward-compat extend
3. **broadcast channel cap**: high-freq (output) = 256, low-freq (notify-like) = 32~64
4. **WS catch-up 순서**: 0x80 hello → 0x88 binding burst → pane-spawned NOTIFY + PANE_OUT replay
5. **session-scoped routing 의 통일 패턴**: `hub.session_for_cookie(cookie)` lookup → filter. cookie 없거나 unattached → skip
6. **echo-minus-sender (5-C)**: `event.sender_conn_id != my_conn_id` AND `event.session_id == my_session` 둘 다
7. **persistence 모델 (5-D P2)**: BE 가 spawn + publish, FE 가 layout PUT. layout 의 진실은 FE
8. **race window 허용**: 사용자-친화. 일관성보다 응답성 우선 — race 결과는 보통 "다음 액션이 정정"
9. **legacy demo path 보존**: `sessionStore.active == null` 또는 `cookie 의 session 부재` 시 server-wide 그대로
10. **strict-length wire 의 confirm**: tail trailer 가 추가되면 기존 strict-length 디코더는 깨짐. FE/BE 양쪽이 *항상* trailer-aware 하게 작성

---

## 12. 변경 이력

- 2026-05-16: 초안 — `5932d00` 직후. Stage 5 BE 의 모든 마일스톤 완료 시점의 cold-pickup brief.
- 2026-05-16: amend ① (post-handover) — `c60ba43` 후속 세션이 §5.1 우선순위 표 갱신 (`0042-be-slice-d-work-package.md` 의 Slice D-1~D-5 endpoint 매트릭스 반영, FE Slice A/B/C ship 후 BE 의존 unblock 우선). §5.2 의 next-2 명세는 `docs/adr/0025-session-scoped-pane-output-filter.md` 로 Proposed-status ADR 승격 + grill-with-docs 3 inline amend (catch-up policy / set ownership / layer 구별 / race-3) 포함. reading order 의 첫 entry 로 `0042` 추가. 본 amend 의 작업물은 모두 untracked — `codebase/smoke/02_stage5.sh` (Stage 5 smoke 7 gates) + `docs/adr/0025-*.md` (next-2 ADR).
