# 0055 — BE 본 sprint 완료 작업 요약 (FE 공유용)

- 작성일: 2026-05-17
- 종류: BE → FE 공유 — *FE 가 요청한 검증 항목 (0053) + BE 측 본 sprint 안 ship 한 모든 land 의 종합 정리*
- 대상 독자: FE 통합 agent + 사용자
- 검증 HEAD: `f0b7cc5` (직전 commit, BE 측 모든 ship 직후)
- 워크스페이스 baseline: **cargo test --workspace 376 PASS / 0 FAIL**
- release build: PASS (`cargo build --release --bin gtmux`)

---

## 0. 한 줄 요약

본 sprint 안 BE 측이 FE 요청 6항목 + P0 의존 2건 + D6 heartbeat 정합 + D14 cookie rotation 까지 land. **0053 P0 BLOCKER 0건**, **P2 follow-up 까지 모두 closed** (단 §7 RFC3339 통일 만 cosmetic follow-up). FE 의 ExportSessionModal / ImportSessionModal / silentReattach / PanelDanglingOverlay / SessionMenu 등 본 sprint ship 한 UX 모두 *BE 측 정합 ship* 완료 — wire 정상.

---

## 1. 0053 FE-요청 검증 항목 — 6/6 closed

`docs/reports/0053-be-verification-checklist.md` 가 정본. 각 항목의 BE 측 결과:

| # | 우선 | 항목 | BE 결과 | 핵심 commit |
|---|---|---|---|---|
| §2 | 🔴 P0 | `GET /api/sessions/{name}/export` 신규 endpoint | ✅ **SHIPPED** (envelope + Content-Disposition + sanitize + RFC3339 + Gate 0029-1~5 5/5 PASS) | `ecc8581` |
| §3 | 🔴 P0 | `POST /api/terminals/:id/respawn` 동시 호출 race | ✅ **SHIPPED** (per-UUID Mutex + idempotent `{reused: true}` short-circuit + integration test) | `d36f092`, `f0b7cc5` |
| §4 | 🟡 P1 | 0046 attach idempotent BE land | ✅ **SHIPPED** (cookie ownership 분기 + `reuse_existing_attach_response` helper + 5/5 검증 항목) | `e9eb9a6` |
| §5 | 🟡 P1 | D6 heartbeat ping/pong 양측 정합 | ✅ **정합 verified + flaky fix** (Hub timings config + integration tests + parallel-stress 5회 stable) | `cd15cba`, `481a4d7` |
| §6 | 🟢 P2 | import body size cap | ✅ **SHIPPED** (2 MiB → 16 MiB raise + doc 명시 + 회귀 가드 2개) | `f4c936e` |
| §7 | 🟢 P2 | `created_at` 형식 | ✅ **정합 verified** (`u64 unix_secs` ↔ FE `number` 정합. RFC3339 통일은 cosmetic follow-up) | (no-op) |

### 1.1 한 줄 정리

```
🔴 P0 BLOCKER : §2 export endpoint = SHIPPED   →  FE ExportSessionModal wire 정상
🔴 P0 BLOCKER : §3 respawn race    = CLOSED    →  multi-webpage auto-respawn 안전
🟡 P1         : §4 attach idempotent = SHIPPED →  refresh + silentReattach 자연 정상
🟡 P1         : §5 D6 heartbeat    = 정합     →  ADR-0021 D6.1 별 layer 의도 정합
🟢 P2         : §6 import body cap = 16 MiB   →  큰 layout import OK (8 MiB+)
🟢 P2         : §7 created_at      = 정합     →  break change 없음
```

---

## 2. 본 sprint BE 측 ship 누적 (시간순)

본 sprint 안 BE 가 land 한 작업 전체. 각 commit 의 FE 측 의존 / 영향 명시.

### 2.1 P0 항목

| commit | 작업 | FE 측 영향 |
|---|---|---|
| `e9eb9a6` | **0046 attach idempotent + D13 /auth SPA pivot (BE)** | `silentReattach` / 새로고침 reattach 가 same-cookie 시 409 → 200. `parseEnvelope` / SPA fallback 정상. mutation guard 의 `in_use` 분기 사실상 사라짐. |
| `cd15cba` | **D6 heartbeat — Hub timings config + integration tests** | abrupt close (browser crash) 시 30s 안 lock 자동 해제. 다른 webpage 의 takeover-style attach 가 ≤30s 안 가능. heartbeat.svelte.ts 의 isStale 임계와 정합. |
| `59bd0ab` | **D14 — `POST /auth/rotate` cookie rotation endpoint** | SettingsOverlay 의 [Rotate session] 버튼 wire 가능 (현 시점 FE 미 ship — BE 만 ready). revoke_others + caller cookie re-issue. |
| `ecc8581` | **`GET /api/sessions/:name/export` (0052 work package)** | FE ExportSessionModal 의 download path 정상. envelope shape + Content-Disposition + sanitize_filename + std-only RFC3339 (chrono 무도입). |
| `d36f092` | **respawn per-UUID lock** | `PanelDanglingOverlay` auto-respawn 의 multi-webpage 동시 race 차단. response `{ id, reused: bool }` 확장 (unbreaking). |

### 2.2 후속 / fix

| commit | 작업 | FE 측 영향 |
|---|---|---|
| `481a4d7` | **D6 heartbeat timeout test flaky fix** | BE 내부 — FE 영향 0. parallel `cargo test --workspace` 5회 stress, flake 0. test contract 정합 (close_code best-effort, disconnect emit strict). |
| `f4c936e` | **import body cap 16 MiB + doc 명시** | ImportSessionModal 의 큰 layout (1000+ items + inline documents) import 가능. axum default 2 MiB → 16 MiB raise. 16 MiB+ body 는 413 자연 처리. |
| `f0b7cc5` | **ADR-0021 D10.3 신규 + 0053 §3.4 close 정합 (docs)** | 위 `d36f092` 의 doc 짝. respawn response shape (`reused` field) 의 FE 활용 가능성 명시. |

### 2.3 검증 가드 / doc

| commit | 작업 |
|---|---|
| `fb9c3bc` | 0052 work package 초안 — export endpoint 의 BE spec 정본 |
| `14ca76b` | 0053 BE 검증 checklist 초안 (FE 발주) |
| `2c104c5` | 0053 BE 검증 결과 표기 (amend ①) |
| `4e3a0d8` | 0053 amend ③ — FE wire 정합 cross-check |
| (이 doc) | **0055 본 doc** — BE 측 종합 정리 |

---

## 3. 각 항목 상세 — FE 가 확인할 wire 표

각 BE endpoint 의 wire 정의 + FE 측 처리 권장 + 검증 smoke.

### 3.1 §2 Export endpoint — `GET /api/sessions/{name}/export`

**Wire**:
```
GET /api/sessions/{name}/export
Authorization: Bearer ... (or Cookie: gtmux_auth=...)

200 OK
Content-Type: application/json
Content-Disposition: attachment; filename="<sanitized-name>.gtmux-session.json"

{
  "kind": "gtmux.session.export",
  "export_version": 1,
  "exported_at": "2026-05-17T01:23:45Z",  // RFC3339 UTC, std-only Hinnant 알고리즘
  "session_name": "<name>",
  "layout": { "schema_version": 2, "groups": [...], "items": [...], "viewport": {...} },
  "metadata": { "app": "gtmux", "app_version": null }
}
```

**Error 분기**:
| 코드 | body | 의미 |
|---|---|---|
| 400 | `{ "error": "invalid_session_name" }` | path regex `[A-Za-z0-9_-]{1,64}` 실패 |
| 401 | (auth middleware) | bearer / cookie 둘 다 없음 |
| 404 | `{ "error": "not_found", "name": "<name>" }` | session 없음 |
| 500 | `{ "error": "save_failed", "details": "..." }` | serialize 실패 |
| 503 | `{ "error": "workspace_not_configured" }` | workspace 미 wire |

**FE 측 처리**: 이미 ship 됨 (`exportSession()` + ExportSessionModal). wire 정상.

**검증 smoke (curl)**:
```bash
curl -s -b /tmp/cookies.txt -i "http://127.0.0.1:9999/api/sessions/alpha/export" | head -10
# 기대: 200 + Content-Type: application/json + Content-Disposition: attachment; filename="alpha.gtmux-session.json"
```

---

### 3.2 §3 Respawn race — `POST /api/terminals/{id}/respawn`

**Wire (amend ⑤ 후)**:
```
POST /api/terminals/{id}/respawn
Authorization: Bearer ... (or Cookie)

200 OK
{
  "id": "<uuid>",
  "reused": false | true
}
```

**`reused` field 의미** (FE 활용 옵션):
- `false` → BE 가 실제로 kill+spawn 실행 (winner). 새 PaneId 가 0x88 broadcast 됨.
- `true` → 다른 호출이 이미 새 PaneId 등록한 상태에서 도달 (loser). kill 안 함, 새 spawn 안 함. **idempotent no-op**.

**FE 측 활용 권장**: `reused: true` 응답 시 *toast 안 띄움* (no-op 이므로). 사용자가 다른 webpage 에서 trigger 한 respawn 의 결과를 보는 것일 뿐. metric / log 로 활용 가능.

**기존 호환성**: FE 의 success 분기 (200 → "respawned") 가 `reused` 무시해도 정상 동작.

**불변 (FE 신뢰 가능)**:
- terminal_map 에는 UUID 당 *언제든* 최대 1 alive PaneId 바인딩.
- PTY leak 0 — 모든 race 분기에서 loser PaneId 자동 kill.
- multi-webpage 의 동시 auto-respawn → 단일 fresh PaneId 로 수렴, 1 webpage 가 `reused: false`, 나머지는 `reused: true`.

**ADR**: 0021 D10.3 (신규 amend ④).

---

### 3.3 §4 Attach idempotent — `POST /api/sessions/{name}/attach`

**동작 (amend ③ 후)**:
- 같은 cookie 의 same-name reattach → **200 OK** (idempotent, 기존 lock 유지, classify 재계산).
- 다른 cookie 의 same-name attach → **409 CONFLICT** (ADR-0019 D4 takeover 금지).
- 같은 cookie 의 different-name attach → 이전 session 의 lock 자동 release 후 새 session attach (D3 single-attach invariant).

**FE 측 영향**:
- `silentReattach` (plan-0008 Phase 2) 의 모든 trigger (WS reopen / visibility change + isIdle) → 200 자연 통과.
- 새로고침 race (`WS close → release_lock` 비동기 vs SPA reattach POST) → 200 자연 통과.
- `mutation guard` 의 `in_use` 분기 사실상 사라짐 (실제 다른 webpage 의 cookie 가 holder 인 경우만).

**검증 smoke**:
```bash
# 같은 cookie 두 번 attach — 둘 다 200
curl -s -b /tmp/jar.txt -X POST "http://localhost:9999/api/sessions/alpha/attach" -w "[%{http_code}]"
curl -s -b /tmp/jar.txt -X POST "http://localhost:9999/api/sessions/alpha/attach" -w "[%{http_code}]"
# 기대: [200][200]
```

**ADR**: 0019 D3 amend ③ + 0046 work package.

---

### 3.4 §5 D6 heartbeat — RFC 6455 transport + FE application-frame perception

**BE 측**:
- Ping = `Message::Ping(Bytes::new())` (RFC 6455 standard opcode 0x9), 15s 주기.
- Pong = browser auto-handle. server-side `last_pong = Instant::now()` 갱신.
- Timeout = 30s 무PONG → `Close(1011 INTERNAL "heartbeat timeout")` send + disconnect_sink fire → `release_lock_for_cookie` 자동.

**FE 측 (`heartbeat.svelte.ts`)**:
- `markFrame()` = *application-frame* (PANE_OUT 0x02 / NOTIFY 0x00 / 0x80~0x89 등) 수신 시 호출. RFC 6455 PING/PONG 은 browser auto-handles, JS 가 못 봄.
- `isStale = now - lastFrameAt > 30_000` — application-frame 30s 무수신.
- `isIdle = now - lastActivityAt > 15_000` — user 입력 15s 무.

**별 layer 의도** (ADR-0021 D6.1 명시):
- BE 의 PING/PONG = transport-level liveness (kernel TCP).
- FE 의 markFrame = application-level perception (Phase 2 silentReattach trigger 입력).
- 둘이 다른 차원이라 frame format 정합 불필요.

**정합 검증**:
- BE 30s timeout ↔ FE 30s isStale 임계 — 정확히 같음. abrupt close 시 BE 가 30s 부근에 close → FE 가 같은 시점에 isStale → silentReattach trigger 의 자연 동시 발화.

**ADR**: 0021 D6 + D6.1 + D6.2 amend ②/③.

---

### 3.5 §6 Import body cap — 16 MiB

**Wire (amend ④ 후)**:
- `/api/sessions/import` 라우트에 `DefaultBodyLimit::max(16 MiB)` layer 적용.
- 16 MiB 초과 시 axum 자동 413 Payload Too Large.
- PUT layout 과 동일 ceiling 공유 (`sessions::SESSION_PUT_MAX_BYTES`).

**FE 측 처리**: 
- 16 MiB 이하 → 정상 import (201 / 409 / 400 등 기존 분기).
- 16 MiB 초과 → 413. FE 의 generic error toast 분기로 처리됨.
- *Client-side body cap (16 MiB)* 도 FE 측 ship 됨 (`e9ba35f`) — friendly size guard.

**Layout 크기 가이드**:
- inline document `maxLength: 65536` × 100 items = ~6.5 MiB → 16 MiB cap 충분.
- 1000+ items 의 큰 layout 도 OK.

**ADR**: 0029 §6 amend ② + §7 보안 표 amend.

---

### 3.6 §7 `created_at` 형식

**BE 측**:
- `POST /api/sessions/import` 의 응답 body 의 `created_at` = `u64` (unix_secs).
- (sessions.rs:956-959 의 `as_secs()`)

**FE 측 (`http/sessions.ts`)**:
- `ImportSessionResponse { name: string; created_at: number; }` — 정합.

**Wire mismatch**: 없음.

**Follow-up (cosmetic)**: ADR-0029 envelope 의 `exported_at` (RFC3339 string) 와 형식 통일 시 양쪽 RFC3339 로 amend 가능. 현 시점 wire 호환성 break 없음, 우선 변경 안 함.

---

## 4. BE 잔여 / 향후 follow-up

### 4.1 0053 잔여

| 항목 | 우선 | 상태 |
|---|---|---|
| §7 RFC3339 통일 | 🟢 P3 cosmetic | `created_at` 도 RFC3339 string 으로 통일하면 envelope 의 `exported_at` 와 정합. **wire break 위험** 있음 — FE 가 number 받고 있으므로 dual-emit 일시 지원 또는 FE 측 amend 필요. 본 sprint 진행 안 함. |

### 4.2 BE 측 미 ship (별 BE work)

| 항목 | 출처 | 우선 |
|---|---|---|
| ADR-0026 Phase 1 `--session` → `--name` rename | 0048 handover §6 | P1 (1-2일 BE) |
| Schema v3 `item.order` field | 0048 handover §6 | P1 (FE Layer V2 enabler) |
| Tier 3 Template CRUD (G36) | frontend-handover-v3 Stage 4 | P2 |
| WS subscriber Lagged reconciliation | 0048 handover §6 | P2+ |
| Rate limiter X-Forwarded-For | Cloud mode | 보류 |

### 4.3 BE → FE 권장 wire 확인

다음 항목은 FE 측 manual / E2E 검증으로 BE wire 가 잘 작동하는지 확인 권장:

1. **0048 FE refresh validation S1~S10** (`docs/reports/0048-fe-refresh-validation-checklist.md`) — 0046 attach idempotent + D6 heartbeat 의 종합 검증. 현 시점 BE 가 모든 wire 준비됨.
2. **ExportSessionModal browser download smoke** — Blob URL + `<a download>` flow 가 BE 의 Content-Disposition 으로 자연 작동하는지.
3. **multi-webpage auto-respawn** — 두 탭으로 같은 session attach 시도 → 한쪽 terminal kill → 양쪽이 동시 dangling 감지 → 두 respawn → BE 의 `reused: true` 응답 한 번 확인.

---

## 5. 본 doc 와 관련된 정본 reference

| 종류 | 경로 |
|---|---|
| FE 측 발주 checklist | `docs/reports/0053-be-verification-checklist.md` |
| Export endpoint work package | `docs/reports/0052-be-session-export-endpoint.md` |
| 0046 attach idempotent work package | `docs/reports/0046-be-attach-handler-idempotent.md` |
| 직전 session migration handover | `docs/reports/0054-session-migration-handover.md` |
| 본 doc 의 부모 sprint | 0051 / 0048 handover, 0042/0045 attach recovery |
| ADR-0019 (single-attach) | `docs/adr/0019-session-and-workspace-model.md` D3/D4/D5.4 |
| ADR-0020 (Auth Lifecycle) | `docs/adr/0020-auth-lifecycle.md` D13/D14 |
| ADR-0021 (Terminal Pool + Heartbeat) | `docs/adr/0021-terminal-pool-and-mirror.md` D6/D6.1/D6.2/D10/D10.3 |
| ADR-0029 (Session Import/Export) | `docs/adr/0029-session-import-export.md` |

---

## 6. 검증 baseline

본 doc 작성 시점 (검증 HEAD: `f0b7cc5`):

| 항목 | 값 |
|---|---|
| `cargo test --workspace --no-fail-fast` | **376 PASS / 0 FAIL** |
| `cargo build --release --bin gtmux` | PASS (~31s) |
| `cargo clippy -p gtmux-http-api -p gtmux-ws-server --no-deps` | warnings only (모두 pre-existing) |

### 6.1 본 sprint 안 BE 측 신규 test 증가

| commit | 신규 test 수 | 누적 workspace count |
|---|---|---|
| `e9eb9a6` (P0-1 + P0-2) | +1 신규 (attach_idempotent_*) − 4 obsolete (server-rendered /auth 테스트들) | 365 → 362 |
| `cd15cba` (D6 ship 정합) | +2 신규 (heartbeat tests) | 362 → 364 |
| `59bd0ab` (D14 /auth/rotate) | +4 신규 (rotate tests) | 364 → 368 |
| `481a4d7` (D6 flaky fix) | 0 (rename + assertion 완화) | 368 |
| `ecc8581` (export endpoint) | +5 신규 (Gate 0029-1~5) | 368 → 373 |
| `f4c936e` (import body cap) | +2 신규 (cap 회귀 가드) | 373 → 375 |
| `d36f092` (respawn per-UUID lock) | +1 신규 (concurrent_same_uuid) | 375 → 376 |

총 +15 신규 - 4 obsolete = net +11. P0/P1/P2 cover 의 polish 가 견고.

---

## 7. 변경 이력

- 2026-05-17: 초안 — BE 본 sprint 의 ship 누적 + 0053 검증 결과 + FE 공유용 종합 정리. 다음 FE agent / 사용자 review 가 한 문서로 BE 진척도 + 잔여 후속 파악 가능하도록 구성.
