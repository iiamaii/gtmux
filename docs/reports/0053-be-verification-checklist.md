# 0053 — BE 작업 완료 확인 checklist (FE 의존 항목)

- 작성일: 2026-05-17
- **BE 검증 일자: 2026-05-17 (amend ①)**
- **FE wire 정합 검증 일자: 2026-05-17 (amend ③)**
- 종류: BE-side verification checklist — 본 sprint 의 FE 작업으로 BE 측 검증/완료가 필요한 항목 일람
- 발주: FE 통합 agent (Inspector + Undo/Redo Phase 1~3 + Import/Export + Drag undo / Respawn loop fix ship 후)
- 관련 ADR: ADR-0019 (D3/D4 single-attach), ADR-0021 (D6 heartbeat + D10 dangling recovery), ADR-0028 (undo/redo), ADR-0029 (import/export)
- 관련 work package: `0046-be-attach-handler-idempotent.md`, `0052-be-session-export-endpoint.md`
- HEAD (FE): `1661f89` feat(frontend): Session import / export
- HEAD (BE 검증 시점): `59bd0ab` feat(backend): D14 — POST /auth/rotate cookie rotation endpoint

---

## 0. 한 줄 요약

본 sprint FE 작업이 ship 된 BE 동작 — *attach idempotent (0046)*, *D6 heartbeat*, *respawn endpoint* — 에 의존하고 있고, FE 신규 작업 *Export endpoint (0052)* 는 BE ship 대기. 각 항목의 검증 방법 + 발견 시 분기를 본 checklist 가 명시.

---

## 1. 우선순위 일람

| 우선 | 항목 | BE 상태 (2026-05-17 검증) | 검증 방법 | 영향 받는 FE 작업 |
|---|---|---|---|---|
| 🔴 ~~P0~~ | **`GET /api/sessions/{name}/export` 신규 endpoint** | ✅ **SHIPPED (2026-05-17 amend ②)** + ✅ **FE wire 정합 verified (2026-05-17 amend ③)** — `export_handler` + envelope types + helpers 추가, route wire, Gate 0029-1~5 5/5 PASS. FE `SessionExportEnvelope`/`parseEnvelope`/`exportSession` 모두 cross-check 정합. | §2 / §2.3.1 참조 | ExportSessionModal (commit `1661f89`) — wire 정상 |
| 🔴 ~~P0~~ | **`POST /api/terminals/:id/respawn` 의 동시 호출 race 정의** | ✅ **CLOSED (2026-05-17 amend ⑤)** — per-UUID Mutex (`AppState::respawn_locks`) 추가, kill→spawn 직렬화, lookup_pane 분기로 idempotent `{reused: true}` short-circuit. ADR-0021 D10.3 신규. integration test `respawn_concurrent_same_uuid_yields_single_alive_binding` (workspace 376 PASS). | §3 참조 | PanelDanglingOverlay auto-respawn — multi-webpage 안전 |
| 🟡 P1 | **0046 attach idempotent 실측 (0048 validation S1~S10)** | ✅ **BE land 완료** (commit `e9eb9a6`) — 5/5 체크리스트 verified (§4.1). FE 측 S1~S10 실측은 별 작업. | §4 참조 | refresh / silentReattach (이미 ship) |
| 🟡 P1 | **D6 heartbeat ping/pong 양측 정합** | ✅ **정합 verified** (commit `cd15cba` + `cd15cba` flaky fix amend ③) — BE ping = RFC 6455 0x9, close = 1011, FE markFrame = application frame 차원 (의도된 별 layer, ADR-0021 D6.1). BE 15s/30s timing 이 FE 30s isStale 임계와 정확히 정합. | §5 참조 | WS reconnect / Phase 2 trigger 의 신뢰성 |
| 🟢 ~~P2~~ | **import body size cap** | ✅ **SHIPPED (2026-05-17 amend ④)** — `/api/sessions/import` 라우트에 `DefaultBodyLimit::max(SESSION_PUT_MAX_BYTES)` (16 MiB) layer 적용. PUT layout 과 동일 ceiling 공유. doc 명시 (sessions.rs const doc-comment + `import_handler` doc + ADR-0029 §6/§7). 회귀 가드 2개: 17 MiB → 413 / 5 MiB → 201. | §6 참조 | ImportSessionModal — 8 MiB 까지 OK |
| 🟢 P2 | **`POST /api/sessions/import` 의 `created_at` 형식** | ✅ **정합** — sessions.rs:956-959 가 `unix_secs` (u64) 반환, FE wire `number` 정합. ADR-0029 envelope 의 RFC3339 `exported_at` 와 형식 불일치 follow-up 은 잔여. | §7 참조 | ImportSessionModal 의 후속 displays |

### 1.1 요약 — BE 측 액션 잔여

| 항목 | 즉시 액션 필요 | 근거 |
|---|---|---|
| §2 Export endpoint | ✅ **SHIPPED + FE wire verified (2026-05-17 amend ②/③)** | export_handler + envelope + tests + ADR ship. FE wire 정합 검증 완료 (§2.3.1) — 0 mismatch. |
| §3 Respawn per-UUID Mutex | ✅ **SHIPPED (2026-05-17 amend ⑤)** | per-UUID lock + idempotent `{reused:true}` path + ADR-0021 D10.3 amend + integration test (concurrent join → sorted `[false, true]`) |
| §4 attach idempotent | NO (BE land 완료) | FE 측 실측 책임 |
| §5 heartbeat | NO (정합) | FE markFrame 의 별 차원 의도 정합 |
| §6 import cap | ✅ **SHIPPED (2026-05-17 amend ④)** | 16 MiB raise + doc 명시 + 회귀 가드 2개 |
| §7 created_at | NO (정합) | follow-up: ADR-0029 envelope 와의 형식 통일 (RFC3339) 은 별 amend |

---

## 2. 🔴 P0 — `GET /api/sessions/{name}/export`

### 2.1 의존성

`docs/reports/0052-be-session-export-endpoint.md` 의 work package 가 정본. FE 의 `exportSession(name)` (`http/sessions.ts`) + `ExportSessionModal` 이 본 endpoint 의 응답에 직접 의존.

### 2.2 검증 항목 (BE PR review 시 확인) — **검증 결과 (2026-05-17 amend ②): ✅ 전 항목 SHIPPED**

- [x] ✅ 라우트 등록 — `lib.rs:554` 의 import 라우트 옆 `.route("/api/sessions/{name}/export", get(sessions::export_handler))`
- [x] ✅ `export_handler` 구현 (sessions.rs:972~) — 0052 §3.2 envelope shape 정합 (`kind` / `export_version` / `exported_at` / `session_name` / `layout` / `metadata`)
- [x] ✅ `Content-Disposition: attachment; filename="<safe-session-name>.gtmux-session.json"` 헤더 — `HeaderValue::from_str` + `header::CONTENT_DISPOSITION` insert
- [x] ✅ `sanitize_export_filename` — `[A-Za-z0-9_-]` 외 `_` 치환 (sessions.rs). `validate_session_name` regex 와 이중 가드
- [x] ✅ SessionCache 우선 + disk fallback — 기존 `state.session_cache.get_or_load(wm, name)` 재사용 (별 `load_session_layout` helper 신규 도입 안 함, 더 작은 surface)
- [x] ✅ 응답 body 의 `layout` 이 ADR-0018 schema v2 정합 — `serde_json::to_vec(&envelope)` 가 `Layout` derive(Serialize) 사용, schema_version=2 / groups / items / viewport 그대로
- [x] ✅ 분기 — 503 (`workspace_not_configured`) / 400 (`invalid_session_name`, `SessionError::Workspace`) / 404 (`{ error:"not_found", name }`) / 500 (`save_failed` + details). 401 은 `/api/*` bearer middleware 자동 처리
- [x] ✅ Gate 0029-1 ~ 0029-5 테스트 (happy / 404 / 401 / 400 invalid-name / round-trip) — `lib.rs:1762~` 의 export 테스트 5개 모두 PASS

**구현 노트**:
- RFC3339 timestamp 는 chrono / time 의존성 없이 std-only — `civil_from_unix` (Howard Hinnant 알고리즘) + `rfc3339_utc_now` helper 신규.
- 404 body 는 `{ "error": "not_found", "name": "<name>" }` (spec 정합, `SessionError::NotFound` 의 default `session_not_found` 코드 대신).
- 401 은 `/api/*` 의 `bearer_auth_middleware` 자동 처리 (handler 가 진입 안 함, 0052 §2 의 401 branch 만족).

**검증**: workspace **368 → 373 PASS / 0 FAIL** (+5 export tests). release build PASS.

### 2.3 BE ship 후 FE 측 wire 확인 (smoke)

```bash
# 1) auth → cookie
TOKEN="<magic-link>"
curl -s -c /tmp/cookies.txt "http://127.0.0.1:9999/auth/bootstrap?token=$TOKEN"

# 2) export 요청
curl -s -b /tmp/cookies.txt -i "http://127.0.0.1:9999/api/sessions/alpha/export" | head -20
```

기대:
- `HTTP/1.1 200 OK`
- `Content-Type: application/json`
- `Content-Disposition: attachment; filename="alpha.gtmux-session.json"`
- body `{ "kind": "gtmux.session.export", "export_version": 1, ... }`

### 2.3.1 FE wire 정합 검증 결과 (2026-05-17 amend ③) — ✅ 정합

코드 인스펙션으로 BE `ExportEnvelope` (sessions.rs:980-993) ↔ FE `SessionExportEnvelope` (`http/sessions.ts:348-355`) 필드 단위 cross-check 수행:

| BE field (Rust) | FE field (TS) | 정합 |
|---|---|---|
| `kind: &'static str = "gtmux.session.export"` | `kind: 'gtmux.session.export'` | ✅ |
| `export_version: u32 = 1` | `export_version: 1` | ✅ |
| `exported_at: String (RFC3339)` | `exported_at: string` | ✅ |
| `session_name: &'a str` | `session_name: string` | ✅ |
| `layout: &'a Layout` | `layout: CanvasLayout` | ✅ |
| `metadata: { app: "gtmux", app_version: Option<&str> }` | `metadata?: { app?: string; app_version?: string \| null }` | ✅ (FE 가 optional + nullable 으로 wider 가드) |

FE `parseEnvelope` 검증 항목 (`http/sessions.ts:364-392`):
- `kind === 'gtmux.session.export'` ✅
- `export_version === 1` ✅ (값이 다르면 `unsupported export_version` throw)
- `session_name` string + length > 0 ✅
- `layout` object + `schema_version === 2` + `items/groups` array + `viewport` object ✅
- 실패 시 `EnvelopeParseError(reason)` throw → ImportSessionModal toast

FE `exportSession` (`http/sessions.ts:453-469`) 의 wire:
- 경로 `GET /api/sessions/${encodeURIComponent(name)}/export` ✅ (axum `{name}` 정합, name 이 path traversal 시도 시 `validate_session_name` 가 400 으로 자연 차단)
- `credentials: 'include'` ✅ (cookie 인증 — BE `bearer_auth_middleware` 가 `auth::authenticate(req.headers())` 통해 `gtmux_auth` cookie 검증, lib.rs:745+ / auth.rs:1-22)
- 분기 401 → `UnauthorizedError` / 404 → `Error('Session not found')` / 기타 4xx,5xx → generic `Error` ✅
- `Content-Disposition` filename 파싱 — `match?.[1] ?? "${name}.gtmux-session.json"` fallback ✅ (BE `sanitize_export_filename` 의 안전 basename 정합)
- Blob 변환 — `res.text()` → `new Blob([text], {type:'application/json'})` ✅ (ExportSessionModal 이 Blob URL download 에 사용)

baseline: FE `pnpm check 305 FILES 0 ERRORS 0 WARNINGS`. **FE 측 patch 불필요 — BE/FE wire 0 mismatch**.

### 2.4 발견 시 분기

| 상황 | FE 측 처리 |
|---|---|
| BE 가 envelope 의 `kind`/`export_version` 다른 값으로 응답 | FE `parseEnvelope` 가 `EnvelopeParseError` throw → toast "Invalid session export file" |
| `Content-Disposition` filename 누락 | FE fallback `${name}.gtmux-session.json` |
| 응답 timeout / 5xx | FE toast "Export failed: <message>" |

---

## 3. 🔴 P0 — `POST /api/terminals/:id/respawn` 동시 호출 race

### 3.1 배경

FE 의 `PanelDanglingOverlay` (commit `dfd8efd`) 가 **mount 시 자동 respawn** 으로 전환됨. 두 webpage 가 같은 terminal UUID 의 panel 을 mirror 하는 상황에서 *동시에* dead 감지 → 두 webpage 가 거의 동시에 `POST /api/terminals/:id/respawn` 호출 가능성.

FE 측 mitigation 은 `danglingTerminals.startRespawn(id)` 의 client-side single-flight — 같은 webpage 안에서는 1 호출. webpage *간* race 는 BE 가 처리.

### 3.2 현재 BE 동작 (`terminals.rs:283`)

```rust
pub async fn respawn_handler(...) -> Response {
    if state.hub.is_none() { return service_unavailable("hub_not_configured"); }
    // Best-effort kill of the existing pane.
    crate::sessions::kill_and_unregister_terminal(&state, &id).await;
    match state.spawn_terminal_with_uuid(id.clone()).await {
        Ok(_) => 200,
        Err(e) => 500,
    }
}
```

→ kill + spawn 2단계. 동시 호출 시:
- request A: kill (alive PaneId 제거) → spawn 시작
- request B: 거의 동시 kill (이미 unbound) → spawn 시작
- 두 spawn 이 같은 UUID 로 진행 → BE 가 둘 다 `hub.bind_pane_id(uuid, new_pane_id)` 호출?

### 3.3 확인 / 결정 필요 사항 — **검증 결과 (2026-05-17)**

- [x] ✅ `spawn_terminal_with_uuid` 의 동시 spawn 처리 — **race-safe via `terminal_map.register`**. `lib.rs:327-378` 의 구현:
  - line 331-333: fast-path early return (UUID 가 이미 alive 면 새 spawn 없음, 기존 PaneId 반환)
  - line 338: `register` 시도 — 동시 case 의 serialization point
  - line 349-361: `UuidAlreadyBound { existing_pane }` 분기 — 진 spawn 의 PaneId 를 `hub.backend().kill(pane)` 으로 즉시 정리, winner 의 `existing_pane` 반환
  - 검증 테스트: `spawn_terminal_with_uuid_does_not_double_publish_on_idempotent_path` (lib.rs:3244)
- [x] ⚠ 두 번째 spawn 의 paneId 가 첫 번째를 덮어쓰는 경우 — **respawn_handler 의 race**: kill→spawn 의 2단계가 non-atomic. A's spawn 직후 B's kill 이 A의 binding 을 제거 → B 가 새 spawn. 0x88 broadcast 가 두 번 (A's PaneId, then B's PaneId). FE 의 `dispatcher.handleTerminalSpawned` 의 `clear` 가 idempotent — last 0x88 wins, FE 가 마지막 값으로 settle.
- [x] ✅ **PTY leak 없음** — `kill_and_unregister_terminal` (sessions.rs:1148) 가 PaneId 의 real OS kill 호출 + map unregister. duplicate spawn race 의 losing PaneId 도 line 353 의 `hub.backend().kill(pane)` 으로 정리됨.
- [x] ⚠ **brief paneId mismatch window 가능** — A's webpage 가 PaneId_A 로 stream subscribe 중 B의 respawn 이 PaneId_A 를 kill → A의 stream 끊김 + 0x88 (PaneId_B) 도착으로 자연 re-subscribe. *현 동작 acceptable* — 단 brief output loss 가능 (수 ms 내).

**결론**: spawn 측은 race-safe, respawn 측은 minor race window 있으나 PTY leak 없음 + FE 가 last-0x88-wins 로 수렴. §3.4 의 per-UUID Mutex 는 follow-up — 현 동작이 user-perceivable 영향 거의 0.

**2026-05-17 amend ⑤ — per-UUID Mutex SHIPPED**:
- `AppState::respawn_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>` slot 추가.
- `respawn_handler`: outer mutex 로 per-UUID Arc<Mutex<()>> entry 발급/획득 → inner lock 안 `terminal_map.lookup_pane(&id)` alive 체크 → Some 이면 idempotent `{id, reused: true}` short-circuit (kill 안 함, 새 spawn 안 함, 새 0x88 broadcast 안 함). None 이면 기존 kill+spawn flow → `{id, reused: false}`.
- Response shape 확장: `{ id, reused: bool }`. unbreaking — FE 가 `reused` 무시 가능. 단 amend ⑤ 후 FE 는 logging / metric 에 활용 가능.
- ADR-0021 D10.3 신규 amend — D10 의 lazy fresh-spawn (D10.1) + attach vs live 차별화 (D10.2) 옆에 동시-호출 정책 (D10.3) 으로 정합.
- 검증: `respawn_concurrent_same_uuid_yields_single_alive_binding` test 추가 — `tokio::join!` 으로 동시 2개 호출, sorted `reused` flags `[false, true]` + `terminal_map.lookup_pane(uuid)` Some 검증. workspace 375 → 376 PASS / 0 FAIL.

### 3.4 권장 BE-side fix (race 위험 확인 시) — **follow-up, 즉시 액션 불필요 (2026-05-17 검증 후)**

3.3 결과에 따라 race window 의 user-visible 영향은 거의 0 (PTY leak 없음 + FE 가 last-0x88-wins 로 수렴). 단 follow-up 으로 tightening 검토 시:

`respawn_handler` 안에 *per-UUID Mutex* 또는 in-flight set 추가:

```rust
// pseudo
let _guard = state.respawn_locks.lock_for(&id).await;
if state.terminal_pool.is_alive(&id) {
    // 누군가 이미 spawn 했음 — 200 OK + 기존 binding 반환 (idempotent path)
    return (StatusCode::OK, Json(json!({ "id": id, "reused": true }))).into_response();
}
// kill + spawn 진행
```

이게 ship 되면 FE 의 자동 respawn 이 multi-webpage 환경에서 안전 — 두 번째 호출은 noop. *현 시점은 acceptable 수준이라 본 fix 는 P2 follow-up*.

### 3.5 검증 smoke (BE side)

```bash
# 동시 2개 respawn 요청 — race 재현
( curl -X POST -b /tmp/cookies.txt http://127.0.0.1:9999/api/terminals/$UUID/respawn &
  curl -X POST -b /tmp/cookies.txt http://127.0.0.1:9999/api/terminals/$UUID/respawn ) | tee /tmp/race.log
# 기대: 둘 다 200 OK, terminal pool 에 UUID 가 *한 번만* alive
# (BE 의 `GET /api/terminals` 응답에 UUID 가 single entry)
```

### 3.6 FE 측 처리 (이미 ship)

FE 는 BE 의 어떤 race 처리 방식과도 호환 — `dispatcher.handleTerminalSpawned` 가 모든 0x88 에서 `clear` 호출하므로 두 broadcast 모두 자연 해제. 단 paneId mismatch / PTY leak 은 BE 가 책임.

---

## 4. 🟡 P1 — 0046 attach idempotent 실측

### 4.1 BE 측 ship 확인 (이미 land — commit `e9eb9a6`) — **2026-05-17 verified ✅**

- [x] ✅ `sessions.rs::attach_handler` 의 line 398 직전 cookie ownership 분기 — *verified at sessions.rs:408 (line slight shift due to docstring growth)*
- [x] ✅ `reuse_existing_attach_response` helper — *verified at sessions.rs:788*
- [x] ✅ `attach_idempotent_for_same_cookie_same_session` 테스트 — *verified at lib.rs:2737*
- [x] ✅ `attach_409_when_held_by_different_cookie` 테스트 — *verified at lib.rs:2794*
- [x] ✅ 기존 `attach_409_when_already_held_same_server` 제거 — *grep 결과 0건, 분할되어 위 2개 테스트로 대체됨*
- [x] ✅ ADR-0019 D3 amend ③ entry — *0046 work package 변경 이력 + ADR-0019 changelog 모두 land*

### 4.2 FE 측 실측 (미진행 — 0048 checklist)

`docs/reports/0048-fe-refresh-validation-checklist.md` 의 S1~S10 을 BE 0046 land 후 manual 또는 puppeteer 로 실측.

특히:
- [ ] **S2**: 새로고침 → 같은 cookie reattach → 200 OK (이전엔 409 → "in_use" toast)
- [ ] **S5**: visibility change + heartbeat idle → silentReattach 200 OK (이전엔 mutation guard 전체 차단)
- [ ] **S7~S10**: 신규 5-state reconnectGate / canMountApp / Modal grace 1s

### 4.3 발견 시 분기

FE 측 회귀 없음 — BE 가 idempotent 응답이라 `silentReattach.result.kind === 'success'` 자연 정상. 실패 시 그건 별 BE bug 라 work package 분리.

---

## 5. 🟡 P1 — D6 heartbeat ping/pong 양측 정합

### 5.1 BE 측 ship 확인 (commit `cd15cba` + amend ③ flaky fix) — **2026-05-17 verified ✅**

- [x] ✅ Hub timings config (interval / timeout) — `HeartbeatTimings` struct + `Hub::set_heartbeat_timings` setter (hub.rs:454-475). production default 15s / 30s.
- [x] ✅ integration tests — `heartbeat_timeout_closes_and_emits_disconnect` (amend ③ rename + contract 정합) + `heartbeat_pong_reply_emits_heartbeat_sink`. workspace `cargo test --workspace` 5회 stress, flake 0.

### 5.2 FE 측 정합 (이미 ship — `heartbeat.svelte.ts`)

- [x] ✅ 15s ping interval / 30s stale 임계
- [x] ✅ WS frame markFrame on receive (application-frame 차원, RFC 6455 transport 와 별 layer)

### 5.3 검증 항목 — **2026-05-17 verified ✅**

- [x] ✅ BE 의 ping frame opcode / payload 가 FE 의 `markFrame` 호출 트리거 시점과 *별 차원* — BE 는 `Message::Ping(Bytes::new())` (RFC 6455 standard opcode 0x9, lib.rs:1159) 사용 → browser auto-handles, JS 가 못 봄. FE `markFrame` (heartbeat.svelte.ts:116) 은 application-frame (PANE_OUT 0x02 / NOTIFY 0x00 / 0x80~0x89 등) 수신 시 호출. ADR-0021 D6.1 amend 가 본 별 차원 명시 ("RFC 6455 PING/PONG 의 browser-auto 처리와 별 application-level perception"). **의도된 정합**.
- [x] ✅ BE 의 timeout 시 close code = 1011 — `close_codes::INTERNAL = 1011` (lib.rs:108) → handle_socket 의 timeout 분기 (lib.rs:1154) 가 `close_codes::INTERNAL` 으로 send. tungstenite 의 `CloseCode::Error` 매핑.
- [x] ✅ BE timing config (15s ping / 30s pong-timeout) 이 FE 의 isStale 임계 (30s) 와 *정확히 정합* — BE 가 30s 후 close → FE 가 30s 부근에 isStale → silentReattach trigger 의 자연 동시 발화. *짧지 않음* OK.
- [x] ✅ 정합 명시 위치: ADR-0021 D6 + D6.1 (browser-auto 별 차원) + D6.2 amend ②/③ (BE ship 정합 + flaky fix). `docs/ssot/wire-protocol.md` ping 섹션은 없음 — ADR-0021 D6 표가 single source.

### 5.4 검증 smoke

```bash
# WS 연결 후 16s 무활동 → ping 도착 확인
websocat ws://127.0.0.1:9999/ws --header "Cookie: gtmux_session=<token>" &
# 약 16s 후 ping frame (또는 binary 0x?? heartbeat) 도착 → FE markFrame 트리거
```

---

## 6. 🟢 P2 — `POST /api/sessions/import` body size cap

### 6.1 배경

FE ImportSessionModal 이 사용자 파일 선택 → JSON parse → POST body 로 전송. 큰 layout (예: 1000+ items, inline document content) 의 body 크기 cap 명시 필요.

### 6.2 확인 항목 — **2026-05-17 amend ④: ✅ SHIPPED**

- [x] ✅ `import_handler` 가 body 받기 전 Content-Length cap 검증 — `/api/sessions/import` 라우트에 `DefaultBodyLimit::max(sessions::SESSION_PUT_MAX_BYTES)` layer 적용 (`lib.rs`). 16 MiB ceiling.
- [x] ✅ cap 초과 시 응답 — **413 Payload Too Large** (axum tower-http RequestBodyLimitLayer 자동).
- [x] ✅ cap 값 명시 — `sessions::SESSION_PUT_MAX_BYTES` const doc-comment 가 "POST /api/sessions/import 도 같은 cap" 명시 + `import_handler` doc-comment 에 413 branch 명시 + ADR-0029 §6 Backend "유지" 의 마지막 항목 + §7 보안 표 의 "악성 JSON import" 행에 cap = 16 MiB 명시.
- [x] ✅ FE 측 처리 — 413 이 자연 toast 분기로 처리됨. FE 측 추가 wire 없이 정합.

**결론**: ship 완료. 16 MiB 까지 import OK, 그 이상은 413 + 명시 doc. PUT layout 과 동일 ceiling 으로 운영 일관성 확보.

### 6.3 ship 후 BE 동작 (amend ④ 후)

`lib.rs` 의 `/api/sessions/import` 라우트 mount 시점에 `.layer(DefaultBodyLimit::max(sessions::SESSION_PUT_MAX_BYTES))` (= 16 MiB) 추가됨. PUT layout 과 같은 ceiling 공유 — 둘 다 v2 layout 을 write 하므로 같은 accept-band. 회귀 가드: `sessions_import_413_when_body_exceeds_cap` (17 MiB → 413) + `sessions_import_accepts_body_below_cap` (5 MiB → 201) — workspace 373 → 375 PASS.

---

## 7. 🟢 P2 — `created_at` 형식 정합

### 7.1 현재 BE 응답 (`sessions.rs:956-959`) — **2026-05-17 verified ✅**

```rust
let created_at = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_secs())
    .unwrap_or(0);
```

→ `u64` (unix seconds). FE wire 의 `created_at: number` 와 정합. *코드 위치는 0053 초안 의 :957 에서 약간 shift — :956 부터 4 lines*.

### 7.2 FE 측 (`http/sessions.ts`)

```ts
export interface ImportSessionResponse {
  name: string;
  created_at: number;  // unix seconds
}
```

→ 정합. 단 ImportSessionModal 의 done stage 에서 사용 안 함 (display 안 함). 향후 export envelope 의 `exported_at` (RFC3339 string) 과 형식 불일치 — 통일 권장:
- 옵션 A: 양쪽 unix seconds (number)
- 옵션 B: 양쪽 RFC3339 string

ADR-0029 D2 의 envelope 는 RFC3339 (`"2026-05-17T00:00:00Z"`) 명시 — 그쪽이 표준. `import_handler` 의 `created_at` 도 향후 RFC3339 로 통일 amend 검토. *지금은 wire mismatch 없음* (FE 가 number 그대로 받음).

---

## 8. BE next-session brief (요약) — **2026-05-17 amend ① 후**

검증 후의 next-action 정리:

1. ✅ ~~즉시 진입 (P0, BLOCKER)~~: §2 — **closed (2026-05-17 amend ②)**. 0052 export endpoint ship 완료. FE ExportSessionModal 의 wire 정상.
2. ✅ ~~race 분석 (P0)~~: §3 — **closed**. spawn 측 race-safe, respawn 측 minor window 있으나 PTY leak 없음 + FE 가 자연 수렴. §3.4 per-UUID Mutex 는 P2 follow-up.
3. ✅ ~~smoke (P1)~~: §5 — **closed**. BE ping = RFC 6455 standard, close = 1011, FE markFrame 의 별 차원 의도 ADR-0021 D6.1 명시.
4. ✅ ~~review-only (P1)~~: §4 — **closed**. 0046 BE land 5/5 verified. FE 측 0048 checklist S1~S10 실측은 FE agent 책임.
5. **🟢 후속 (P2)**: §6 — import body cap 값 명시 (코드 코멘트 + ADR-0029 §4 amend). 권장 8~16MB raise via `DefaultBodyLimit::max(...)` layer. 현 동작 (2MB 자동) 은 OK.
6. **🟢 후속 (P2)**: §7 — created_at 형식의 RFC3339 통일 (ADR-0029 envelope 와 정합). 현 wire 는 정합 (number ↔ unix_secs).
7. **🟢 후속 (P2, §3.4)**: respawn per-UUID Mutex — tightening only, user-visible impact 0.

**P0 BLOCKER 모두 closed** (2026-05-17 amend ②). **P2 §6 import cap closed** (amend ④). **§3 respawn race closed** (amend ⑤, per-UUID Mutex). 잔여 = §7 RFC3339 통일 만 (wire mismatch 없음 — pure cosmetic follow-up).

---

## 9. 변경 이력

- 2026-05-17: 초안. FE 본 sprint 의 Inspector + Undo/Redo + Import/Export + Drag undo / Respawn loop fix ship 후 BE 의존성 정리.
- 2026-05-17: **amend ① — BE 측 검증 결과 표기**. BE agent 가 §2~§7 의 모든 검증 항목 수행 후 완료 수준 표시. 결과: **P0 1건 (§2 export endpoint) blocker 남음 — next-BE 즉시 진입**. 그 외:
  - §3 respawn race = safe-with-minor-window (PTY leak 없음, FE 가 last-0x88-wins 로 수렴). §3.4 per-UUID Mutex 는 P2 follow-up.
  - §4 0046 attach idempotent = 5/5 land verified (sessions.rs:408 / 788, lib.rs:2737 / 2794, 기존 _409_already 제거).
  - §5 D6 heartbeat = 정합 verified. BE ping = RFC 6455 standard (lib.rs:1159 Bytes::new() ping), close = 1011 (close_codes::INTERNAL, lib.rs:108), FE markFrame 별 차원 (application frame perception) 정합 (ADR-0021 D6.1 명시).
  - §6 import body cap = default-only (axum Json DefaultBodyLimit 2MB 자동, 코드/ADR 어디에도 cap 값 명시 없음 — doc amend follow-up).
  - §7 created_at = `u64 unix_secs` 정합 (sessions.rs:956-959). ADR-0029 envelope 의 RFC3339 와는 별 형식 — 통일 follow-up.

  검증 HEAD: BE `59bd0ab` (D14 /auth/rotate land 직후).
- 2026-05-17: **amend ② — §2 P0 BLOCKER closed (export endpoint BE ship)**. 0052 work package 의 spec (envelope `{kind, export_version, exported_at, session_name, layout, metadata}` + `Content-Disposition: attachment; filename="<safe>.gtmux-session.json"` + sanitize_filename + RFC3339 timestamp) 그대로 구현. 핵심 결정:
  - **RFC3339 timestamp**: chrono/time crate 미도입 — std-only `civil_from_unix` (Howard Hinnant 알고리즘) + `rfc3339_utc_now` helper 도입 (sessions.rs).
  - **404 body shape**: 0052 §2 spec 정합 (`{ "error": "not_found", "name": "<name>" }`) — 기본 `SessionError::NotFound` 의 `session_not_found` 코드 대신 hand-craft.
  - **load helper**: 별 `load_session_layout` fn 신규 도입 안 함, 기존 `state.session_cache.get_or_load(wm, name)` 재사용 — 더 작은 surface, 기존 SessionError 분기 (NotFound/Workspace/Io/Corrupt) 정합.
  - **Route 위치**: `/api/*` 의 `/api/sessions/import` 옆에 `.route("/api/sessions/{name}/export", get(export_handler))` — bearer middleware 자동 적용 → 401 자연 분기.
  - **Tests**: Gate 0029-1~5 (happy / 404 / 401 / 400 invalid-name / round-trip) — `lib.rs:1762~`, 5/5 PASS.
  검증: workspace 368 → 373 PASS / 0 FAIL. release build PASS.
- 2026-05-17: **amend ③ — §2.3.1 신규 (FE wire 정합 검증)**. BE `ExportEnvelope` (sessions.rs:980-993) ↔ FE `SessionExportEnvelope` (`http/sessions.ts:348-355`) field-by-field cross-check, `parseEnvelope` validation rule 정합 (kind / export_version / session_name / layout.schema_version=2 / items+groups+viewport), `exportSession` http client 의 wire (URL path / credentials:'include' / 401/404 분기 / Content-Disposition filename fallback / Blob 변환) 모두 코드 인스펙션으로 검증. **0 mismatch — FE 측 patch 불필요**. cookie 인증 정합 확인 (BE `bearer_auth_middleware` → `auth::authenticate(headers)` 가 `gtmux_auth` cookie 검증, FE `credentials:'include'` 동봉). baseline: FE `pnpm check 305 FILES 0 ERRORS 0 WARNINGS`. 검증 HEAD: BE `ecc8581`, FE working tree (uncommitted: 다른 worker 의 untracked handover docs + brand SVG).
- 2026-05-17: **amend ④ — §6 P2 import body cap closed**. 이전 동작 = axum `Json` 의 default 2 MiB cap (자동 413). FE 의 큰 layout (1000+ items + inline documents) 시 부족. 본 amend: (1) `sessions::SESSION_PUT_MAX_BYTES` 의 visibility `pub(crate)` 변경 + doc-comment 에 "POST /api/sessions/import 도 같은 cap" 명시, (2) `lib.rs` 의 `/api/sessions/import` 라우트에 `.layer(DefaultBodyLimit::max(sessions::SESSION_PUT_MAX_BYTES))` 추가 (= 16 MiB), (3) `import_handler` doc-comment 의 outcomes 표에 "413 (axum auto)" branch + cap 상수 reference 명시, (4) ADR-0029 §6 Backend "유지" 마지막 항목 + §7 보안 표에 16 MiB cap 명시, (5) 회귀 가드 2개 (`sessions_import_413_when_body_exceeds_cap` — 17 MiB → 413 / `sessions_import_accepts_body_below_cap` — 5 MiB → 201). PUT layout 과 동일 ceiling 공유 — 둘 다 v2 layout 을 write 하므로 같은 accept-band. 검증: workspace 373 → **375 PASS / 0 FAIL** (+2 cap tests). release build PASS. P2 잔여 = §3.4 per-UUID Mutex / §7 RFC3339 통일 (둘 다 user-visible 영향 거의 0).
- 2026-05-17: **amend ⑤ — §3 respawn per-UUID Mutex SHIPPED (0053 §3.4 closed)**. multi-webpage `PanelDanglingOverlay` auto-respawn 의 race window 차단. 본 amend: (1) `AppState::respawn_locks` slot 신규 (`Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>` — outer mutex 로 per-UUID Arc<Mutex<()>> entry 발급, inner mutex 로 같은 UUID 의 kill+spawn 쌍 직렬화), (2) `respawn_handler` 변경 — per-UUID lock 진입 후 `terminal_map.lookup_pane(&id)` alive 체크 → Some 이면 idempotent `{id, reused: true}` short-circuit (kill 안 함, 새 spawn 안 함, 새 0x88 broadcast 안 함). None 이면 기존 kill+spawn flow → `{id, reused: false}`. (3) Response shape 확장 — `{ id, reused: bool }` 필드 추가 (unbreaking, FE 의 success 분기 호환). (4) ADR-0021 D10.3 신규 amend ④ — D10.1 (lazy spawn) + D10.2 (attach vs live 차별화) 옆에 D10.3 (동시-호출 정책) 으로 정합. (5) integration test `respawn_concurrent_same_uuid_yields_single_alive_binding` 추가 — `tokio::join!` 으로 동시 2개 호출, sorted `reused` flags `[false, true]` + `terminal_map.lookup_pane(uuid)` Some 검증. **P0 §3 closed**. 검증: workspace 375 → **376 PASS / 0 FAIL** (+1 신규 concurrent test). release build PASS. P2 잔여 = §7 RFC3339 통일 만 (wire mismatch 없음, pure cosmetic).
