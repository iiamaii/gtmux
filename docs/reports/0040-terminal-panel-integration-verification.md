# 0040 — Terminal ↔ Canvas panel 연동 검증 + FE 요구 사항 정리

- 일자: 2026-05-16
- 작성자: backend agent (`e5606f9` 직후, 검증 결과)
- 종류: **검증 보고 + FE 요구 사항 doc** — 0039 의 후속 검증으로 발견된 reload/reconnect gap 및 그에 따른 FE/BE 작업 분담

---

## 0. 한 줄 요약

terminal ↔ canvas panel 연동의 end-to-end 흐름을 코드 레벨까지 추적한 결과 **fresh spawn 경로는 완전 보장**되지만, **page reload / WS reconnect 시 UUID↔PaneId 매핑이 영구 손실되어 *기존 alive terminal* 의 panel 이 'connecting' placeholder 에서 멈춤**. 본 gap 은 단순 추가 작업 없이는 닫히지 않으며, **BE-측 catch-up 0x88 재발행 (BE-alone fix) 으로 닫히면 FE 측 요구 사항 0**. BE-측 fix 가 land 하지 않으면 FE 가 자체적으로 회피할 수 없음 (decode 만으로는 매핑 복원 불가).

### 후속 — ✅ 옵션 A ship 완료

본 문서 작성 후 옵션 A 가 BE 작업트리에 land — `TerminalUuidProvider` trait + `Hub::set/get_terminal_uuid_provider` + ws-server 의 catch-up loop 가 `provider.alive_bindings()` 받아 모든 alive UUID 에 대해 `0x88 TERMINAL_SPAWNED` envelope emit + CLI 가 boot 시 `hub.set_terminal_uuid_provider(state.terminal_map.clone())` 등록. **FE 변경 0, end-to-end 보장 완성.**

추가로 같은 batch 에 `attach_handler` 의 implicit detach-on-reattach (ADR-0019 D3 single-attach invariant) 회귀 fix 동시 land — cookie 가 다른 session 으로 attach 전환 시 prev session 의 flock + hub session_table 정리.

---

## 1. 코드 레벨 흐름 추적 (verification)

### 1.1 ✅ Fresh spawn 경로 (사용자가 새 terminal 클릭) — 완전 보장됨

```
[User]                           [FE]                                [BE]
  | click Terminal tool             |                                   |
  |--------------------------------→| handleTerminalClick               |
  |                                 | spawnMultiSessionTerminal(coords) | Canvas.svelte:493
  |                                 | createTerminalItem(coords)        | itemFactory
  |                                 | → fresh UUID                      |
  |                                 |                                   |
  |                                 |--- PUT /sessions/:n/layout ------→| (UUID unmatched)
  |                                 |<-- ETag --------------------------|
  |                                 |                                   |
  |                                 |--- POST /attach/confirm ---------→| spawn_terminal_with_uuid
  |                                 |                                   | hub.publish_terminal_spawned
  |                                 |<-- 0x88 {terminal_id, pane_id} ---|
  |                                 |                                   |
  |                                 | handleTerminalSpawned             | dispatcher:538
  |                                 | → terminalPool.bindPaneId         | terminalPool:85
  |                                 |                                   |
  |                                 | PanelNode reactive:               | PanelNode:125
  |                                 | terminalPaneId = paneIdFor(uuid)  |
  |                                 | (이제 number)                       |
  |                                 |                                   |
  |                                 | <XtermHost paneId={...} />        | PanelNode:402
  |                                 | → registerPaneOut(paneId, h)      | XtermHost:100
  |                                 |                                   |
  |                                 |<-- 0x02 PANE_OUT (shell prompt) --|
  |                                 | term.write(bytes) → 화면 표시      |
  | keys "ls\n" --------------------→| term.onData → encodePaneIn        |
  |                                 |--- 0x03 PANE_IN ----------------→| backend.send_input
  |                                 |<-- 0x02 PANE_OUT (ls 결과) ------|
```

**모든 단계 코드 검증 완료** (file:line 인용):
- `codebase/frontend/src/lib/canvas/Canvas.svelte:447` — `handleTerminalClick` multi-session branch
- `codebase/frontend/src/lib/canvas/Canvas.svelte:493-534` — `spawnMultiSessionTerminal`
- `codebase/frontend/src/lib/ws/dispatcher.svelte.ts:538-544` — `handleTerminalSpawned` → `bindPaneId`
- `codebase/frontend/src/lib/stores/terminalPool.svelte.ts:85-87` — `bindPaneId` 구현
- `codebase/frontend/src/lib/canvas/PanelNode.svelte:125, 397-402` — `terminalPaneId` derived + 3-way mount
- `codebase/frontend/src/lib/canvas/XtermHost.svelte:40-110` — numeric `paneId` 수신, register/send 흐름
- `codebase/backend/crates/http-api/src/lib.rs:spawn_terminal_with_uuid` — register 성공 직후 `hub.publish_terminal_spawned(uuid, pane.0)`

### 1.2 ❌ Reload / reconnect 경로 — gap 발견

#### 1.2.1 시나리오

기존 alive terminal 이 있는 session 에 사용자가 다음 중 하나 수행:
- 브라우저 reload (F5 / Cmd+R)
- 네트워크 일시 단절 후 WS auto-reconnect (`WsClient` 의 attempt loop)
- 다른 tab 으로 attach 옮긴 후 다시 원 tab 복귀

#### 1.2.2 흐름 + 손실 지점

```
[User reloads page]
  |
  |  fresh JS state — terminalPool.paneIdByUuid 비어있음
  |
  | → GET /api/sessions/:name/layout
  |   ← { items: [3 terminal items with UUIDs] }
  |
  | → POST /attach
  |   ← { matched: [u1, u2, u3], unmatched: [] }
  |
  | → unmatched.length === 0 → /attach/confirm 호출 안 함
  |    BE: spawn_terminal_with_uuid 호출 안 됨 → 0x88 발행 안 됨 ★★★ 손실 지점 ★★★
  |
  | → WS handshake (catch-up replay):
  |   ← 0x80 LAYOUT_CHANGED hello
  |   ← 0x07 NOTIFY pane-spawned (3 frames — body 는 {"kind":"pane-spawned"} only,
  |                                        terminal_id 없음)
  |   ← 0x02 PANE_OUT × N (ring buffer replay — paneId 만)
  |
  | → PanelNode (3 개 panel):
  |     terminalPaneId = terminalPool.paneIdFor(uuid) === undefined
  |     → "Terminal stream connecting…" placeholder ★ 영구 표시 ★
  |
  | → 5-s 후: terminalPool.refresh() poll
  |   → GET /api/terminals
  |   ← [{ id: u1, alive, label, ... }, ...]   ★ pane_id 필드 없음 — 매핑 불가 ★
  |
  | → 다음 5-s poll 도 동일 — 영구 placeholder
  |
  | → 새 terminal 추가 시에만 0x88 발행 → bindPaneId — 기존 3 개는 평생 unbound
```

#### 1.2.3 손실 원인 3 곳

| 원인 | 위치 | 영향 |
|---|---|---|
| 0x88 TERMINAL_SPAWNED 는 *fresh spawn* 전용 | BE `spawn_terminal_with_uuid` (단 1 회) | alive terminal 은 fresh 아님 → 발행 안 됨 |
| WS catch-up 의 NOTIFY `pane-spawned` 가 terminal_id 미포함 | `ws-server/src/lib.rs:notify_to_envelope` (body = `{"kind":"pane-spawned"}`) | FE 가 pane-spawned 받아도 UUID 모름 |
| `/api/terminals` 응답 의 `TerminalInfo` 가 `pane_id` 필드 미포함 | `http-api/src/terminals.rs:125-143` (`pool.into_iter().map((uuid, _pane))` 의 `_pane` drop) | FE polling 으로도 rebuild 불가 |

#### 1.2.4 FE-측 단독 회피 불가

FE 가 자체적으로 매핑 복원할 수 있는 *알려진 소스* 부재:
- `0x88` 은 BE 가 fresh spawn 시만 발행
- `0x07 NOTIFY pane-spawned` 의 body 는 paneId 만
- `/api/terminals` 응답은 UUID 만
- `/api/sessions/:name/layout` 응답은 UUID 만 (layout — pane 무관)

따라서 **BE 가 어떤 형태로든 PaneId↔UUID 매핑을 노출하지 않으면 FE 단독 회피 불가**.

---

## 2. 해결 옵션 — BE-측 fix 3 안

| 옵션 | 설명 | BE 변경 | FE 변경 | latency | 위험 |
|---|---|---|---|---|---|
| **A (권장)** | WS catch-up 시 alive 모든 UUID 에 대해 `0x88 TERMINAL_SPAWNED` 재발행 | ws-server 의 catch-up loop 확장 + `TerminalUuidProvider` trait (CookieValidator 와 같은 패턴). http-api 가 `TerminalMap` 에 impl | **없음** — `handleTerminalSpawned` 가 이미 동일 처리 | reconnect 직후 (handshake 와 동시) | 작음. ws-server 의존성 trait 1 개 추가 |
| **B** | `/api/terminals` 응답의 `TerminalInfo` 에 `pane_id` 추가 | terminals.rs 의 `_pane` → `pane.0`, struct field 추가 | terminalPool.refresh() 의 success 분기에서 `bindPaneId` 호출 | 5-s polling 또는 즉시 (WS open 시 refresh 트리거 추가 시) | 작음. wire 추가-only field |
| **C** | 둘 다 (A + B) | A 와 B 의 합 | B 만 | 즉시 + 5-s fallback | 작음. 가장 robust |

**권장**: **옵션 A** — FE 무변경, BE-alone fix, latency 가장 짧음 (catch-up 과 함께 묶음).

옵션 B 보완 가치: WS 가 잠시 unavailable 한 상태 (서버 재시작 grace, network blip) 에서도 polling 만으로 복원 가능. 하지만 옵션 A 만으로도 본질적 gap 은 닫힘.

---

## 3. FE 요구 사항 매트릭스 (최종)

본 문서의 핵심 질문: **"FE 에 요구할 사항이 무엇인가?"**

### 3.1 (옵션 A 채택 시) — **FE 요구 사항 = 0**

BE 가 catch-up 0x88 재발행을 land 하면:
- FE 의 기존 `handleTerminalSpawned` handler 가 catch-up 0x88 받음 → `terminalPool.bindPaneId` 호출 → `terminalPaneId` derived 갱신 → XtermHost 자동 마운트
- **FE 코드 변경 0 줄**. wire backward-compat (FE 가 0x88 받든 안 받든 무관).

### 3.2 (옵션 B 채택 시) — FE 요구 사항 = 작음

| 작업 | 위치 | 분량 | 필수 |
|---|---|---|---|
| `TerminalInfo` 타입에 `pane_id?: number` 필드 추가 | `lib/types/terminals.ts:15` | 1 줄 | ✅ |
| `terminalPool.refresh()` 의 fetch 성공 후 each row 에 대해 `bindPaneId(row.id, row.pane_id)` (pane_id 있을 때만) | `lib/stores/terminalPool.svelte.ts:62-77` | ~5 줄 | ✅ |
| (선택) WS state 'open' 시 `terminalPool.refresh()` 트리거 — latency 5-s → ~100ms | `lib/ws/dispatcher.svelte.ts:561` `adaptStateChange` | ~3 줄 | ⚠️ optional |

### 3.3 (옵션 C 채택 시) — 옵션 B 와 동일

본 문서는 옵션 A 를 권장 — 따라서 **FE 요구 사항은 사실상 0**.

---

## 4. 본 gap 외의 잠재 FE 작업 (참고용 — 모두 비-필수)

종합 검토 결과, 본 gap 외의 모든 항목은 *현재 시점에서 end-to-end 보장에 필수 X*.

| 항목 | 상태 | 필수성 |
|---|---|---|
| `decodeMountCascade` / `handleMountCascade` (0x86) | ✅ FE 작업트리 완성 | 필수 X — FE 의 `spawnMultiSessionTerminal` 이 manual UUID + `attach_confirm` flow 로 동등 동작. BE 의 `POST /terminals` endpoint 는 alternative path 일 뿐 |
| `decodeTerminalListUpdate` / `handleTerminalListUpdate` (0x87) | ✅ FE 작업트리 완성 | 필수 X — sidebar refresh 의 latency 단축 hint 일 뿐 (`terminalPool` polling 이 authoritative) |
| `decodeTerminalDied` / `handleTerminalDied` (0x85) | ✅ FE 작업트리 완성 | 보호 가치 있음 — dangling overlay 의 즉시 표시. 단 polling 으로도 비슷한 결과 |
| `decodeTerminalSpawned` / `handleTerminalSpawned` (0x88) | ✅ FE 작업트리 완성 | **본 문서 §1.1 의 fresh spawn 경로 핵심**. 본 gap fix (옵션 A) 의 핵심 소비자 |
| 5-C decoder 의 `sessionId` trailer 처리 | ❌ 미진행 | 필수 X — FE 가 outbound 0x81~0x84 송신 시작 시점에만 필요. 현재 inbound-only |
| `NewPanelButton` → `POST /terminals` migrate | ❌ 미진행 | 필수 X — 현재 `spawnMultiSessionTerminal` 의 manual UUID flow 가 동작. cleaner alternative 일 뿐 |
| `XtermHost` 의 mode prop refactor (legacy/terminal 분기) | N/A | 필수 X — 현 구현이 *PanelNode 가 resolve 후 numeric paneId 만 전달* 패턴으로 충분 (XtermHost.svelte:40-43 주석 참조) |

---

## 5. BE-측 옵션 A 의 구현 명세 (참고)

권장 옵션 채택 시 BE 가 land 해야 할 코드 (FE 무관):

### 5.1 ws-server

```rust
// hub.rs — CookieValidator 와 같은 패턴
#[async_trait]
pub trait TerminalUuidProvider: Send + Sync {
    /// 현재 alive 한 (PaneId, terminal UUID) 쌍 전체. WS handshake catch-up
    /// 시 호출되어 each binding 마다 0x88 TERMINAL_SPAWNED 환경 emit.
    async fn alive_bindings(&self) -> Vec<(u64, Arc<str>)>;
}

impl Hub {
    pub fn set_terminal_uuid_provider(&self, p: Arc<dyn TerminalUuidProvider>);
    pub fn terminal_uuid_provider(&self) -> Option<Arc<dyn TerminalUuidProvider>>;
}
```

### 5.2 ws-server handle_socket

```rust
// 기존 catch-up 의 `for id in backend.pane_ids()` 직전에 추가:
if let Some(provider) = hub.terminal_uuid_provider() {
    for (pane_id, uuid) in provider.alive_bindings().await {
        let env = Envelope::new(
            FrameType::TerminalSpawned,
            Bytes::from(payload::encode_terminal_spawned(&uuid, pane_id)),
        );
        if let Ok(buf) = env.encode() {
            if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                return;
            }
        }
    }
}
```

### 5.3 http-api

```rust
// terminal_map.rs
#[async_trait]
impl gtmux_ws_server::TerminalUuidProvider for TerminalMap {
    async fn alive_bindings(&self) -> Vec<(u64, Arc<str>)> {
        self.snapshot()
            .await
            .into_iter()
            .map(|(uuid, pane)| (pane.0, Arc::from(uuid.as_str())))
            .collect()
    }
}
```

### 5.4 CLI 등록

```rust
hub.set_terminal_uuid_provider(app_state.terminal_map.clone());
```

### 5.5 테스트 추가

- ws-server socket-level: catch-up replay 시 모든 alive UUID 에 대해 0x88 frame 수신
- http-api: `TerminalMap::alive_bindings()` 정합성

추정 작업량: ~1-2 시간. 본 gap 만 닫음.

---

## 6. 결론 + 권장 action

1. **terminal ↔ canvas panel 연동의 fresh spawn 경로는 완전 보장됨** (검증 완료, §1.1)
2. **reload / reconnect 시 매핑 손실 gap 존재** — FE 단독 회피 불가 (§1.2)
3. **권장: BE 옵션 A (catch-up 0x88 재발행) land — FE 요구 사항 0** (§2, §3.1)
4. 옵션 A land 후 end-to-end 보장 완전 — page reload / WS reconnect 후에도 기존 panel 자동 마운트

### 6.1 BE 다음 batch (옵션 A 시 추가)

0038 큐의 next-2 (session-scoped pane_output) 보다 *우선 진입 가치 높음* — 본 gap 은 daily-use scenario 차단.

### 6.2 FE-측 액션 매트릭스 (최종)

| 시급도 | 항목 | 진행 조건 |
|---|---|---|
| ✅ 완료 | 본 문서 §4 의 7 개 ✅ 표시 | — |
| 🟢 nothing | 본 gap 의 FE-단독 회피 | BE 옵션 A 채택 시 |
| 🟡 optional | 본 gap 의 BE 옵션 B 보완 시 small FE 작업 | BE 옵션 B 채택 시만 |
| 🟡 optional | 5-C outbound + `decode` 의 sessionId trailer 처리 | FE 가 selection/viewport 송신 시작할 때 |
| 🟡 optional | `NewPanelButton` 의 `POST /terminals` migrate | 어느 시점이든 |

---

## 7. 변경 이력

- 2026-05-16: 초안 — 본 검증 결과 (fresh spawn ✅, reload/reconnect ❌) + 옵션 A 권장 + FE 요구 사항 사실상 0 의 결론.
