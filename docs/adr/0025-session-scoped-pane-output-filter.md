# ADR-0025: Session-scoped `PANE_OUT` outbound filter (ADR-0021 D2 amend)

- 상태: Accepted (2026-05-16) — Slice D 완료 직후 ratify + 즉시 코드 진입. FE-NEW-6 (multi-xterm subscriber) 의존성 분석 결과: 본 ADR 의 D2 set 정의 (= "session 의 layout 안 terminal_id 의 PaneId set") 는 *intra-session mirror* 든 *cross-session mirror* 든 정합 (item.id == terminal.id invariant 의 결과). 따라서 FE-NEW-6 의 mirror 구현 형태와 무관하게 본 filter 의 의미 변화 X — 보류 사유 (D8 의 "결정의 결합 의존") 가 해소됨.
- 일자: 2026-05-16 (Stage 5 BE 마감 후 — `c60ba43` 시점의 next-2 큐)
- 결정자: agent (backend role)
- 근거 plan: `docs/plans/0007-multi-session-pivot.md`
- 근거 reports:
  - `docs/reports/0038-stage-5-next-batch-queue.md` (next-2)
  - `docs/reports/0041-next-session-handover.md` §5.2 (next-2 명세 초안 — 본 ADR 이 그 명세를 ADR 로 승격)
- Amends: ADR-0021 D2 (Terminal output 의 server-wide → session-scoped + legacy-passthrough 2-mode)
- 관련 ADR: ADR-0019 (Session+Workspace Model), ADR-0021 (Terminal pool + multi-session mirror — 본 ADR 의 직접 전제), ADR-0020 (Auth lifecycle — cookie-driven session lookup), ADR-0013 (PTY + tokio::broadcast — 변경 X)
- 관련 SSoT: `docs/ssot/wire-protocol.md` (0x02 PANE_OUT 의 outbound 의미는 변경 X — server 의 *fanout 정책* 만 amend)

## 맥락

### 현 상태 (Stage 5 BE 마감 직후)

ADR-0021 D2 가 정의한 흐름은 *완전 server-wide*:

```
PTY master FD → backend reader task → tokio::broadcast.send(bytes)
                                          ↓ N subscribers
                                          모든 WS connection 에 (PaneId, Bytes)
```

각 WS connection 은 자기 `paneOutHandlers.get(paneId)` 의 client-side filter (frontend) 로 *자기 panel 의 paneId 만* 화면에 그림. 같은 session 의 webpage 만 mount 되어 있으므로 **결과적으로 격리** — 그러나 *책임이 frontend 에 있음*.

### 두 가지 결함

1. **Bandwidth**. session A 의 long PANE_OUT 폭주가 session B 의 WS 에도 그대로 전달. session B 의 frontend 가 paneId 매칭으로 자체 drop 해도, network → server CPU → WS frame 직렬화 → kernel send 비용은 이미 발생. 100 session × 10 alive terminal 환경에서 비례하지 않음.

2. **Isolation 의 책임 위치**. 격리가 *frontend 의 paneId 매칭* 에 의존 — frontend 의 stale state, 잘못된 PaneId mapping, 또는 미래의 multi-tab leak (예: tab A 가 session X attach, tab B 가 session Y attach, 같은 cookie 일 때) 모두 BE 로부터 비-격리 traffic 을 받음. defense-in-depth 의 first ring 부재.

### 본 ADR 의 범위

본 ADR 은 *WS connection 의 outbound 단계* 에 session-scoped filter 를 추가. **broadcast 자체는 server-wide 그대로** (ADR-0021 D2 의 D2 backbone — N:N 자연 + multi-session mirror 의 *enabling 기반* 은 보존). 즉:

```
변경 전: broadcast 가 server-wide  → WS 가 모두 forward
변경 후: broadcast 가 server-wide  → WS 가 [내 session 의 terminal 만] forward
                                          (filter 는 WS-level, broadcast 무변경)
```

핵심: kernel 의 multi-attach 자연 (ADR-0013 D11) 도 보존, ADR-0021 D2 의 mirror semantics 도 보존 — 단지 *outbound fanout 의 분배 정책* 만 amend.

#### Layer 구별 (CONTEXT.md 의 "server-scoped broadcast" 와 본 ADR 의 layer 가 다름)

CONTEXT.md 는 "Terminal output 의 broadcast 는 server-scoped" 로 명시. 본 ADR 은 그 명제를 부정하지 않음 — **broadcast 의 channel scope** (kernel/server-wide, ADR-0013 D11 의 tokio::broadcast subscribe 모델) 와 **outbound fanout 의 분배 정책** (어느 WS 가 어느 PaneId 의 frame 을 외부로 송신) 은 **다른 layer 의 결정**. 

| Layer | 결정 주체 | 본 ADR 영향 |
|---|---|---|
| Kernel broadcast (PTY → tokio::broadcast tx) | ADR-0013 D11 | 변경 X |
| Subscriber fanout (broadcast → 각 WS handler 의 rx) | ADR-0021 D2 의 D2 backbone | 변경 X (모든 WS handler 가 rx 보유) |
| WS handler 의 outbound forward (rx → Message::Binary) | 본 ADR 의 D1 | **session-scoped filter 추가** |

미래 reader 가 두 명제를 모순으로 오인하지 않도록 본 §의 layer 표 명시.

## 결정 (Decisions)

### D1. Outbound filter 의 위치 = WS handler 의 `pane_output` arm

WS handler 의 select! loop 에서 `pane_output` broadcast 를 받아 PANE_OUT envelope 으로 직렬화하는 분기. 그 직전 또는 직후에 *session terminal set 에 속하는 PaneId 만 통과*.

```rust
// 현 (ADR-0021 D2):
result = pane_output_rx.recv() => {
    let (pane_id, bytes) = result?;
    let env = Envelope::new(FrameType::PaneOut, encode_pane_out(pane_id, &bytes));
    sink.send(Message::Binary(env.encode()?)).await?;
}

// 본 ADR (D1):
result = pane_output_rx.recv() => {
    let (pane_id, bytes) = result?;
    if let Some(set) = session_pane_set.as_ref() {
        if !set.contains(&pane_id) { continue; }   // ★ session-scoped filter
    }
    let env = Envelope::new(FrameType::PaneOut, encode_pane_out(pane_id, &bytes));
    sink.send(Message::Binary(env.encode()?)).await?;
}
```

`session_pane_set` 이 `None` 이면 *legacy demo path* (cookie 미attach, single-session) — server-wide 통과. 본 ADR 의 D5 참조.

#### Catch-up replay 단계의 filter 정책 (amend ③ 2026-05-17 — 0066 §BE-1 / 0067 Phase 2)

**amend ③ 이후 (현재)**: cookie-attached path 의 catch-up replay (`pane-spawned` NOTIFY + PANE_OUT ring buffer dump) 는 **D1 filter 가 활성** — `session_pane_set` 의 cold-load 를 catch-up *전* 으로 끌어올리고, replay 의 두 envelope 도 같은 set 으로 필터. legacy demo path (cookie 무 / unattached / provider 없음) 는 unfiltered — D5 의 server-wide 통과 그대로.

amend ③ 이전 정책 (참고용 — 옛 결정):
- catch-up 의 cold-load 가 진행 중인 layout PUT 과의 race 로 stale 할 수 있어, replay 시 filter 적용하면 *자기 session 의 정상 history 도 누락* 위험.
- replay 의 bytes 는 *이미 server 의 ring buffer 에 존재하는 과거 frame* — security-wise 의 격리 의의는 *live 단계의 새 frame* 이 본질.
- 따라서 catch-up = filter bypass, live = filter on (`filter_armed` flag 로 boundary 표현).

amend ③ 의 동기 (0066 review):
- multi-session 환경에서 reconnect 폭주 시 *현재 session 과 무관한 모든 pane 의 ring buffer bytes* 가 모든 reconnect 마다 WS 로 forward → 네트워크/메모리/main-thread cost 가 session 수 × 비례. 단일-사용자 환경에서도 100 session × 10 alive pane 시 매 reconnect 의 catch-up 이 무관 bytes O(N) 전송.
- cold-load race 의 false-negative 위험 (옛 정책의 회피 사유) 은 D3 의 hot-update 채널들 (`layout_events` / `terminal_spawned_events` / `session_change_events`) 이 catch-up 직후 set 을 refresh 해 자연 회복. layout PUT 의 broadcast 가 reconnect 직후 도착하면 set 이 즉시 갱신되고, 누락된 history 는 사용자가 새 layout 을 fetch 한 직후 server 의 live broadcast 로 채워짐.
- replay 가 "이미 server 에 있는 과거 frame" 이라는 점은 변하지 않음 — 단 *지금 session 의* 과거 frame 만 보내면 충분.

구현 (amend ③):
```rust
// 1. cookie-attached path 의 set cold-load 를 catch-up 이전에 수행:
let session_pane_set: Option<HashSet<u64>> =
    if let (Some(provider), Some(cookie)) =
        (hub.session_pane_set_provider(), cookie_value.as_deref())
    {
        hub.session_for_cookie(cookie)
            .map(|name| provider.pane_ids_for_session(&name))
            .map(|fut| fut.await)
    } else { None };

// 2. catch-up loop 도 같은 set 으로 필터 (pane-spawned NOTIFY + PANE_OUT 둘 다):
for id in backend.pane_ids() {
    if !session_pane_set
        .as_ref()
        .map(|s| s.contains(&id.0))
        .unwrap_or(true)  // legacy demo path = passthrough
    { continue; }
    // emit pane-spawned NOTIFY + PANE_OUT replay
}

// 3. `filter_armed` flag 제거 — set 이 catch-up 이전에 commit 되므로 live 단계도
//    같은 set 을 동일 contains() 로 사용. boundary 표현 불요.
```

`TerminalSpawned` (0x88) burst (UUID↔PaneId binding) 는 catch-up 이전에 server-wide 로 emit — 본 amend 의 영향 범위에서 제외. 이유: 0x88 frame 은 small (UUID + paneid pair) 이고, FE 의 dangling overlay 등 panel-attach 외 용도가 있음. 향후 분리 필요 시 별 amend.

#### Race-1 (cold-load vs layout PUT) 의 위험 평가 (amend ③ 첨부)

옛 정책이 회피하던 race 의 잔여 위험 분석:

| race | 발생 조건 | 영향 | 회복 |
|---|---|---|---|
| cold-load 가 stale layout 읽음 (PUT 의 disk write 직후 ↔ in-mem snapshot 갱신 race) | layout PUT 이 reconnect handshake 와 동시 | catch-up 시 신규 추가된 pane 의 history 누락 | LAYOUT_CHANGED broadcast 가 reconnect 직후 도착 → FE 가 layout 재페치 → 신규 pane 의 live PANE_OUT 부터 정상 |
| cold-load 가 layout PUT 직후 읽음, 그러나 TerminalMap 이 stale (terminal_spawned 가 아직 broadcast 안 됨) | spawn 직후 reconnect | 신규 spawn 의 PaneId 가 set 에 없어 catch-up 누락 | `terminal_spawned_events` 가 직후 도착 → D3 hot-update 가 set 에 PaneId 추가 → 다음 PANE_OUT 부터 정상 |

두 race 모두 false-negative 는 *한 번의 history 누락* — fail-safe 영역 (D3 보수성 원칙). cold-load 의 false-negative 영향이 한 reconnect 의 history 누락에 그치므로, 옛 정책의 "filter bypass" 의 대가 (server-wide replay traffic) 보다 비용 효율적.

### D2. Session pane set 의 출처 = AppState 의 layout + TerminalMap

WS handler 는 자기 cookie 의 session_name (hub.session_for_cookie) 으로 *해당 session 의 SessionLayout.items 중 type:"terminal" 의 terminal_id list* 를 조회 → terminal_map 의 by_uuid 로 PaneId 로 변환 → `HashSet<u64>` 화.

이 변환은 *handshake 시 1 회 cold load* + *변동 이벤트마다 hot update* (D3 의 hook 들).

#### Cold load (handshake)

```rust
async fn load_session_pane_set(state: &AppState, session: &str) -> HashSet<u64> {
    let layout = state.read_session_layout(session).await?;
    let uuids: Vec<&str> = layout.items.iter()
        .filter_map(|i| if let Item::Terminal { common, .. } = i { Some(common.id.as_str()) } else { None })
        .collect();
    let map = state.terminal_map.snapshot().await;
    uuids.iter().filter_map(|u| map.by_uuid.get(*u).map(|p| p.0)).collect()
}
```

WS handler 가 hub 의 `Arc<dyn SessionPaneSetProvider>` (D6 의 trait) 를 호출 — http-api 가 impl. 의존성 그래프는 D10 α / 0040 option A 와 동일 패턴 (trait in ws-server, impl in http-api).

### D3. Hot update — session 의 terminal set 변동 이벤트들

WS handler 가 *자기 session terminal set* 을 stale 없이 유지하려면 변동 이벤트 마다 갱신. 이미 존재하는 broadcast 채널들의 hook 으로 충분:

| 이벤트 | 변동 | hook |
|---|---|---|
| `POST /attach/confirm` (unmatched spawn) | add | 기존 `terminal_list_change_events` 의 `added` 가 이미 emit — WS 가 자기 session 인지 확인 후 set 에 추가 |
| `POST /sessions/:name/terminals` (5-D P2) | add (trigger session only) | `mount_cascade_events` (trigger session) + `terminal_list_change_events.added` (non-trigger) — WS 가 자기 session 매칭 |
| `PUT /layout` (item 추가/제거) | add or remove | 기존 `layout_events` 의 broadcast — 본 ADR 에서 *WS 가 자기 session 의 layout_events 만 hook* |
| `DELETE /items/:id?kill_terminal=true` | remove (layout) + maybe remove (pool) | `layout_events` + `terminal_died_events` |
| Terminal exit / explicit kill | pool 에서 제거 — set 에서도 제거 | `terminal_died_events` (이미 server-wide) |
| Implicit detach-on-reattach (ADR-0019 D3) | 다른 session 으로 전환 | 본 ADR 에서 *cookie session change 시 set 통째로 재계산* — D4 참조 |

#### 보수성 원칙

stale 한 *false positive* (set 에 있지만 실제는 죽은 terminal) 는 무해 — broadcast 가 그 PaneId 로 오지 않음. stale 한 *false negative* (set 에 없는데 실제 alive 인 terminal) 가 위험 — 사용자 panel 이 "connecting..." 영구 보임. 따라서:
- *add* 는 빠르게 (이벤트 마다 즉시)
- *remove* 는 안전하게 — terminal_died 의 broadcast 가 already 격리 보장 (death 후 broadcast 안 옴), set 에서 remove 는 메모리 회수 목적만

#### Set ownership = per-WS owned + channel-driven update (옵션 B)

`session_pane_set` 의 실체는 **WS handler local 의 owned `HashSet<u64>`** — `Arc<RwLock<...>>` 같은 shared mutable 패턴 회피. 갱신은 위 표의 broadcast 채널 (layout_events / terminal_died / terminal_list_change / terminal_spawned / session_change) 의 `recv()` 분기에서 *직접 mutation*. 이유:
- contains() hot path 의 lock acquire 비용 회피 — extreme 시나리오 (250k-1M contains/s) 에서 RwLock 의 read-lock overhead 가 의미 있음
- 변동 빈도 낮음 (~5 events/s peak — D7 의 set 갱신 빈도 표) → channel-driven update 의 cost 가 lock 의 cost 보다 훨씬 낮음
- 각 WS 가 자기 set 을 owned 으로 보유 → cross-WS contention 0

거부된 대안:
- 옵션 A `Arc<RwLock<HashSet<u64>>>` (shared) — read-many write-rare 패턴이지만 매 contains() 의 lock 비용. 거부.
- 옵션 C `tokio::sync::watch::Receiver<Arc<HashSet<u64>>>` — atomic snapshot swap. read 는 cheap 이나 매 변동마다 set 통째 copy → 갱신 cost ↑. polling 모델보다 더 자연이지만 owned mutation 보다 복잡. 거부.

본 owned 패턴은 ADR-0021 의 *(session, panel) 쌍 단위 Streaming State* (CONTEXT.md "Panel Streaming State") 의 patterns 와도 정합 — broadcast subscriber 를 per-connection 으로 보유하는 흐름.

### D4. Cookie session change 시 set 재계산

ADR-0019 D3 implicit detach-on-reattach 가 발동하면 WS connection 의 cookie 가 다른 session 에 attach. WS connection 자체는 그대로 — 단 *그 connection 이 forward 할 session* 이 바뀜.

#### 방법 A (선택): hub.session_for_cookie 의 mtime watching

Hub 가 cookie session 의 변동을 broadcast 하는 `session_change_events: broadcast::Sender<(cookie, new_session_name)>` 신규 채널 추가. WS handler 가 자기 cookie 의 이벤트 받으면 set 재계산.

#### 방법 B (대안): polling

WS handler 가 매 N 초 또는 매 이벤트마다 `hub.session_for_cookie(cookie)` 호출 → 변경 감지 시 재계산. 단순하지만 polling 비용 + N 초 stale window.

#### 권장 = A

새 broadcast 채널 1 개 추가, 기존 broadcast 패턴과 정합. polling 없음. broadcast cap = 64 (low-freq, attach 전환은 분 단위).

### D5. Legacy demo path 보존 (`session_pane_set = None`)

WS connection 의 cookie 가 *어떤 session 에도 attach 미상태* (i.e., `hub.session_for_cookie(cookie) == None`) 시 → `session_pane_set = None` → filter bypass → server-wide 통과 (ADR-0021 D2 의 옛 동작).

이 모드는:
- single-session 시대의 (token-only, cookie 미발급) automation CLI / 데모 환경 보존
- 회귀 안전망 — multi-session 코드 경로의 버그가 cookie-attach 사용자에게만 영향, automation 은 안전

#### Test 회귀 가드

- *cookie 무, attach 무, WS handshake* → 기존 server-wide 통과 (회귀 X)
- *cookie 유, attach 무 (logout 직후), WS handshake* → set None → 통과
- *cookie 유, attach 유* → set 비어있어도 filter 활성 (empty set = 0 PaneId 통과) — 이건 *그 session 에 terminal 아직 없는* 정상 케이스

### D6. 신규 trait `SessionPaneSetProvider` (cross-crate hook)

`CookieValidator` (D10 α) / `TerminalUuidProvider` (0040 option A) 와 같은 패턴:

```rust
// crates/ws-server/src/hub.rs
#[async_trait]
pub trait SessionPaneSetProvider: Send + Sync {
    /// 주어진 session 의 layout 안 terminal item 들의 *현재 PaneId set*.
    /// 미존재 session 또는 빈 layout 은 빈 set 반환.
    async fn pane_ids_for_session(&self, session_name: &str) -> HashSet<u64>;
}

impl Hub {
    pub fn set_session_pane_set_provider(&self, p: Arc<dyn SessionPaneSetProvider>);
    pub fn session_pane_set_provider(&self) -> Option<Arc<dyn SessionPaneSetProvider>>;
}
```

http-api 측 impl 은 `AppState` 가 보유 — `SessionCache.read_session_layout(name)` + `terminal_map.snapshot()` 의 join. CLI boot 시 `hub.set_session_pane_set_provider(state.clone())` 등록 (state 가 Arc 패턴이므로 cheap clone).

### D7. 측정 가능한 성공 지표

본 ADR 채택 후:

| 지표 | 측정 | 목표 |
|---|---|---|
| Cross-session bytes leakage | Session A 의 PANE_OUT N bytes / Session B 의 WS 수신 bytes | **0 bytes** (legacy demo path 제외) |
| Bandwidth ratio (10 sessions, 1 hot terminal) | 본 ADR 전: 모든 WS 에 N bytes | 본 ADR 후: 1 WS 에 N, 다른 9 에 0 — 10× 감소 |
| WS catch-up latency 회귀 | handshake 시 cold load (D2) 의 시간 | < 10 ms (in-memory dict + Arc clone) |
| FE 변경 | 0 — outbound shape (0x02 PANE_OUT envelope) 무변경 |

#### 연산 부하 추정 (filter contains() hot path)

`HashSet<u64>::contains` 의 amortized O(1) lookup 비용 ~50 ns. WS connection 의 `pane_output` arm 이 매 broadcast frame 마다 1 회 호출.

| 시나리오 | broadcast frame/s | WS 수 | contains 호출/s | CPU 점유율 |
|---|---|---|---|---|
| typical (10 panel × 1 session) | ~1k | 1 | 10k | 0.05% |
| moderate (50 panel × 1 session) | ~5k | 1 | 50k | 0.25% |
| heavy (50 panel × 5 cross-session mirror) | ~5k | 5 | 250k | 1.25% |
| extreme (100 panel × 10 session) | ~10k | 10 | 1M | 5% |
| burst (`cat large.log`, 1 panel) | 100k | 10 | 1M | 5% |

→ 최악의 burst 케이스도 5% CPU 마진. 본 ADR 의 *동기 자체* (extreme 시나리오 부하 절감) 가 정당화 — 본 ADR 전: 모든 WS 가 10000 PaneId 의 모든 frame 의 직렬화+send. ADR 후: 자기 session 의 100 PaneId 의 frame 만 send → ~99% bandwidth 절감.

#### Set 갱신 빈도

- layout PUT (drag debounced): 1-5/s peak
- 0x88 spawn ([New Terminal]): ~0.1/s
- 0x85 died: ~0.01/s
- session_change (workspace switch): ~0.001/s

→ 총 갱신 빈도 ~5/s peak. 갱신당 작업 (per-WS owned set mutation, D3 의 옵션 B): ~10 μs. 무시 가능.

#### Memory footprint

WS 1 connection 의 `HashSet<u64>` ~640 bytes (capacity 64). 100 WS = 64 KB. 무해.

### D8. 도입 순서 (incremental)

본 ADR 의 land 는 FE-NEW-6 (ADR-0021 D1 의 multi-xterm subscriber, 같은 UUID 가 여러 panel 에 fan-out) land 후로 보류. 이유:
1. FE-NEW-6 가 *같은 UUID 의 PANE_OUT 을 multiple xterm 에 fan-out* 의 frontend 구현. 이 frontend 자체가 *bytes 가 도착하는 가정* 에 의존.
2. 본 ADR 의 filter 가 *그 UUID 의 PaneId 가 session set 에 있는지* 로 결정. 만약 frontend 가 "다른 session 의 mirror panel 도 이 PANE_OUT 으로 그려야 함" 이라면 → 다른 session 의 WS 에도 통과시켜야 함.
3. 결론: D2 의 *session pane set* 정의가 ADR-0021 D1 의 *mirror policy* 와 직접 결합. mirror 가 *한 session 내부 mirror* (같은 UUID 가 여러 panel) 만이면 본 ADR 의 D1 그대로. *cross-session mirror* (같은 UUID 가 session A 의 panel 1 + session B 의 panel 2) 이면 본 ADR 의 D2 정의가 *그 session 의 layout 에 등장하는 모든 UUID* 의 PaneId — 자연 정합.

FE-NEW-6 land 후 본 ADR 의 D2 정의를 ratify (단 1 line: "그 session 의 layout 안 terminal_id 의 PaneId set"), 코드 진입.

### D9. 옛 구현 (server-wide broadcast 그대로) 와의 양립

본 ADR 후에도 *broadcast 채널은 변경 X*. PTY reader task 는 여전히 모든 subscriber 에 send. ADR-0021 D2 의 *cross-session mirror enabling* 도 보존. 새 가능성은 *"같은 UUID 가 두 session 의 layout 에 동시 등장"* — 양쪽 session 의 WS 가 둘 다 filter 통과 (D2 의 set 정의 그대로) → 양쪽 frontend 가 PANE_OUT 받음 → 양쪽 화면에 mirror.

## 대안 검토

### A1. Broadcast 자체를 session 별 channel 로 분리

거부. ADR-0013 D11 의 PTY-level multi-attach 의 자연 (kernel-level multiplex) 을 추가 분기로 깸. 같은 UUID 가 cross-session mirror 되는 (ADR-0021 D2) 시나리오를 *channel join + fan-out* 으로 복원해야 함 — 본 ADR 의 D1 filter 보다 복잡.

### A2. Frontend 의 PaneId set 으로 backend 가 매번 IndexSet 받기

거부. 권한 검증을 frontend 입력에 의존 — *defense-in-depth 의 first ring 부재* 문제 그대로. 또 PaneId set 의 PUT 갱신 latency window 가 stale 의 새 출처.

### A3. PaneId → session_name 의 역방향 mapping 으로 broadcast 직전 분배

거부. broadcast subscriber 가 모두 동등 — 분배 시 *각 subscriber 의 session* 을 알아야 함. WS handler 가 자기 session 으로 filter 가 의미상 같음 + 구현 단순.

### A4. Cookie 의 session 변경 시 WS reconnect 강제

거부. UX 비싸 (xterm 의 ring buffer replay + scroll position 보존 X). 또 본 ADR 의 D4 broadcast 채널 추가 cost 작음.

## 위험

| 위험 | 영향 | 완화 |
|---|---|---|
| Cold load 의 race (handshake 시점에 layout PUT 이 in-flight) | session 의 새 terminal 의 PaneId 가 set 누락 → 짧은 "connecting" | D2 의 catch-up replay bypass + `filter_armed` 진입 시점에 set commit + layout_events 의 hot update 보강 (D3) |
| Layout PUT 와 broadcast 의 race (broadcast 가 PUT 직전 도착 + set 갱신은 PUT 후) | 단 ms 의 false negative — 새 spawn 의 첫 prompt 의 일부 줄 부재 | 사용자 keypress 의 echo 가 자연 회복 + layout_events 의 자연 hot update (수 ms 안). 추가로 catch-up replay 단계는 filter bypass 라 일반 reload 시나리오엔 영향 X |
| Set 갱신 events 의 ordering — terminal_died 가 layout_events 보다 먼저 도착 시 PaneId 흐름 stale | 죽은 PaneId 가 set 에 잠시 잔존 또는 alive PaneId 가 미리 제거 | terminal_died 의 frame 이 *(PaneId, UUID)* 둘 다 carry → set 에서 *그 PaneId* 만 제거 (UUID 일치성과 무관). 죽은 PaneId 는 monotonic increment 라 다시 등장 X — 무해 |
| `SessionPaneSetProvider` impl 의 lock 경합 | snapshot 마다 `terminal_map.snapshot().await + state.read_session_layout()` 의 두 lock 직렬 acquire — high-attach load 시 contention | snapshot 결과를 hub 에 short-TTL cache (예: 50 ms) — D3 hook 이 invalidate |
| Cross-session mirror 시나리오의 의미 변화 | ADR-0021 D2 가 *cross-session 도 mirror* 라고 explicit 한 곳: D7, D8 의 [Attach existing terminal] | D2 의 set 정의 = "그 session 의 layout 의 모든 UUID 의 PaneId" — A 의 layout 에도 B 의 layout 에도 등장하면 양쪽 filter 통과. 정합 |
| Legacy demo path 회귀 (cookie 없는 environment) | server-wide 통과 의도가 깨지면 single-session demo / automation 깨짐 | session_pane_set == None 의 D5 분기 + 회귀 test gate (smoke + cargo) |
| FE-NEW-6 land 전 본 ADR 진입 | mirror semantics 의 결합 X — set 정의가 *frontend 의 mirror policy* 와 misaligned | D8 의 의도적 보류 |

## 도입 단계 (FE-NEW-6 land 후)

1. **B1.** `SessionPaneSetProvider` trait + hub setter/getter + boot 시 register — D10 α / 0040 option A 와 같은 batch 패턴
2. **B2.** WS handler 의 `pane_output` arm 에 filter 추가 + cold load (handshake catch-up 의 0x88 burst 직후)
3. **B3.** layout_events / terminal_died_events / terminal_list_change_events / terminal_spawned_events 의 hot update hook
4. **B4.** `session_change_events` broadcast 신규 채널 + WS handler 의 hook (D4)
5. **B5.** smoke 02_stage5.sh 에 신규 gate — session A 의 PANE_OUT 이 session B 의 WS 에 도달하지 않는 invariant
6. **B6.** 회귀 test (cargo) — legacy demo path (cookie 무) 의 server-wide 통과 보존

추정 작업량: 2~3 일 (handover §5.2.5 의 같은 추정).

## 어휘 매트릭스 (CONTEXT.md 정합)

- **Session pane set** = 한 Session 의 `SessionLayout.items` 중 terminal item 의 `terminal_id` 의 `TerminalMap.by_uuid` 로 lookup 한 `PaneId` 의 `HashSet`. (본 ADR 의 신조어)
- **Legacy demo path** = cookie 미발급 또는 미attach 상태의 WS connection — server-wide broadcast 그대로 통과 (D5)
- **Cross-session mirror** = ADR-0021 D2 가 명시한 *같은 Terminal 이 두 Session 의 Panel 양쪽에 attach* 상태 — 본 ADR 의 D2 set 정의로 자연 정합

## 변경 이력

- 2026-05-16: 초안 (Proposed). FE-NEW-6 land 후 채택 확정 + 코드 진입.
- 2026-05-16: amend ① — grilling (grill-with-docs) 결과 inline 갱신:
  - §맥락에 "Layer 구별 (broadcast scope vs fanout 정책)" 표 추가 — CONTEXT.md 의 "server-scoped broadcast" 명제와 본 ADR 의 "session-scoped filter" 의 layer 차이 명시
  - D1 본문에 "Catch-up replay 단계의 filter 정책" 신규 — replay 는 bypass, live 단계만 filter (cold-load race 회피)
  - D3 본문에 "Set ownership = per-WS owned + channel-driven update (옵션 B)" 신규 — 매 contains() 의 lock 회피
  - D7 의 측정 표 아래 "연산 부하 추정" / "Set 갱신 빈도" / "Memory footprint" 추가
  - §위험표에 race-3 (set 갱신 events ordering) 추가, race-1/race-2 의 영향·완화 갱신
- 2026-05-17: amend ③ — D1 의 "Catch-up replay 단계의 filter 정책" 갱신. 0066 §BE-1 / 0067 Phase 2 의 multi-session reconnect traffic 부담 해소: cookie-attached path 의 catch-up replay (pane-spawned NOTIFY + PANE_OUT) 도 `session_pane_set` 으로 필터. cold-load 를 catch-up 이전으로 이동, `filter_armed` flag 제거. legacy demo path / 0x88 TerminalSpawned burst 는 amend 영향 외 (D5 그대로). 옛 정책의 cold-load race 회피는 D3 의 hot-update 채널 (layout_events / terminal_spawned_events / session_change_events) 의 false-negative-safe 정합으로 자연 회복. 회귀 가드: 신규 unit test 3 종 (`catchup_replay_filtered_to_session_panes`, `catchup_replay_unfiltered_when_no_cookie`, `catchup_replay_unfiltered_when_provider_unset`).
- 2026-05-16: amend ② — Status: Proposed → **Accepted** + 즉시 코드 진입. FE-NEW-6 의존성 분석 결과 D2 set 정의가 mirror policy 와 자연 정합 (intra-session 이든 cross-session 이든 layout-presence 기준) — 보류 사유 해소. 구현 완료:
  - B1: `SessionPaneSetProvider` trait + `Hub::set/session_pane_set_provider` (`crates/ws-server/src/hub.rs`) + impl on `AppState` (`crates/http-api/src/session_pane_set.rs`) + CLI 등록 (`bin/gtmux-cli/src/main.rs`)
  - B2: WS handler 의 cold load (catch-up replay 직후, `filter_armed` 진입 boundary) + per-WS owned `HashSet<u64>` + `pane_output` arm 의 contains() filter
  - B3: hot update — `layout_events` / `terminal_spawned_events` / `terminal_died_events` 의 set 갱신. `TerminalDiedEvent` 에 `pane_id` 추가 (race-3 처리)
  - B4: `session_change_events` 채널 신규 (`SESSION_CHANGE_BROADCAST_CAPACITY=64`) + WS handler 의 hook. `set_session_for_cookie` / `clear_session_for_cookie` / `clear_sessions_by_name` 가 변동 감지 시 emit
  - B5: smoke 02_stage5.sh 의 기존 12 gate 가 regression-free 통과 (legacy demo path 보존 검증) — 신규 cross-session isolation gate 는 multi-cookie 셋업 복잡도 때문에 unit test 6 종으로 대체 (`two_sessions_have_disjoint_sets`, `cross_session_mirror_uuid_is_in_both_sets`, etc.)
  - B6: 6 신규 unit test (workspace 382 → 388 PASS, regression 0). legacy demo path (cookie 없거나 unattached → server-wide 통과) 회귀 가드 = 기존 catch-up + manipulation/notify/mount_cascade gate 들 (cookie-only session 의존)
