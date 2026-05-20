# 0078 — BE Handover: Component connector (ADR-0036) schema + validate + openapi

- 작성일: 2026-05-19
- 작성 주체: agent (system-architect role, 본 batch dispatch)
- 정본 cross-link:
  - **결정 출처**: [ADR-0036](../adr/0036-canvas-component-connector.md) — Canvas component connector (Accepted 2026-05-19)
  - **schema amend**: [ADR-0018 D12 amend (2026-05-19)](../adr/0018-canvas-item-data-model.md) — D3 type discriminant + D4 payload row + D8 `Item::Connector` variant + Validation
  - **prior land 정합**: ADR-0018 D10 amend ② (2026-05-17, 0056 work package) 의 `Item::Document` 정합 패턴 — `crates/http-api/src/schema.rs` 의 enum variant + validate() arm + ValidationError variant + unit test 6종
  - **FE 짝**: [0079-fe-handover-connector.md](./0079-fe-handover-connector.md)
- 관련 ADR: ADR-0018 (Canvas Item Data Model), ADR-0036 (Canvas component connector), ADR-0006 (Persistence — ETag CAS)

## 핵심 원칙 — 거짓 ship 방지

[`0072` §0 의 5 원칙](./0072-be-handover-from-0071-audit.md#핵심-원칙---거짓-ship-방지) 그대로 적용 — anchor / acceptance criteria / anti-pattern / behavior change 정확성 / self-check 표.

특히 본 task 는 *schema validation* 영역 — refer-무결성 / self-loop / BBox 자동 재계산 셋이 *invariant 보호 region*. validate() 의 match arm 이 모든 reject 경로를 cover 해야 함. 5+ unit test 필수.

---

## 0. Self-grilling 결정 사항

### Q1. `from_id` / `to_id` 의 refer-무결성 — items[] 의 전체 scan?

**결정**: ✅ **HashMap<&str, &Item> 한 번 build 후 lookup**. validate() 의 prelude 에서 `let id_index: HashMap<&str, &Item> = items.iter().map(|it| (it.id(), it)).collect();`. connector validation 의 각 arm 이 `id_index.get(&conn.from_id)` 로 O(1) 확인. items[] 의 size N 에서 O(N) build + O(C) connector check, total O(N+C) — 16 MB cap (≈ 50K items) 안 충분.

대안 (단순 `iter().any`) = O(N×C). 거절 — connector 가 많으면 N² 가 됨.

### Q2. Connector → Connector chain — connector 의 endpoint 가 다른 connector 라면?

**결정**: ✅ **Reject** (`ConnectorInvalidEndpoint`). ADR-0036 D1 의 *connector 도 item* 의 자연 귀결로 *technically 가능* 이지만 MVP scope 외. 사용자 mental 모델 = "panel → panel" 의 *2 step indirection* 의미 모호. 명시 reject 가 P1 의 multi-hop wire 표현 (별 ADR) 과 mental 충돌 회피.

validate arm:
```rust
let from = id_index.get(&conn.from_id).ok_or(ConnectorEndpointMissing)?;
let to   = id_index.get(&conn.to_id).ok_or(ConnectorEndpointMissing)?;
if matches!(from, Item::Connector { .. }) || matches!(to, Item::Connector { .. }) {
    return Err(ConnectorInvalidEndpoint);
}
```

### Q3. Self-loop (`from_id == to_id`) — ADR-0036 O2 의 MVP reject

**결정**: ✅ **Reject** (`ConnectorSelfLoop`). validate 의 별 arm:
```rust
if conn.from_id == conn.to_id {
    return Err(ConnectorSelfLoop);
}
```
P1 검토 (flow chart 의 recursion 표현 가능) 시 본 reject 해제.

### Q4. BBox (`x/y/w/h`) 의 자동 재계산 — PUT 직전?

**결정**: ✅ **`put_layout_handler` 에서 validate() 직전에 connector 들의 BBox 재계산**. 사용자가 보낸 connector 의 `x/y/w/h` 무시 + 두 endpoint 의 anchor 좌표 → BBox 계산:

```rust
fn anchor_point(item: &Item, anchor: Anchor) -> (f64, f64) {
    let c = item.common();
    let (x, y, w, h) = (c.x, c.y, c.w, c.h);
    match anchor {
        Anchor::N      => (x + w / 2.0, y),
        Anchor::NE     => (x + w,       y),
        Anchor::E      => (x + w,       y + h / 2.0),
        Anchor::SE     => (x + w,       y + h),
        Anchor::S      => (x + w / 2.0, y + h),
        Anchor::SW     => (x,           y + h),
        Anchor::W      => (x,           y + h / 2.0),
        Anchor::NW     => (x,           y),
        Anchor::Center => (x + w / 2.0, y + h / 2.0),
    }
}
// connector 의 from/to anchor point 의 BBox = min-max.
```

근거: BBox 가 endpoint 와 desync 되면 alignment / hit-test / SvelteFlow edge layer 의 *visible bounds* 가 stale. 항상 server 가 canonical — FE 가 보낸 값은 *fail-open suggestion* 이라 무시 OK.

대안 (FE 가 매번 계산 후 send) = endpoint 좌표 변경 시마다 FE 가 dependent connector 까지 mutation 해야 — race 부담. 거절 (BE 단일 계산).

### Q5. Endpoint item 의 좌표 변경 시 connector BBox 도 갱신?

**결정**: ✅ **PUT 마다 *모든 connector 의 BBox 재계산*** — `put_layout_handler` 의 validate 직전 *전체 connector 들* 의 BBox 일괄 update. 한 layout 의 connector 가 많아야 ~10개, 계산 비용 무시.

대안 (변경된 endpoint 만 dependent connector 갱신) = dependency 추적 부담. 거절 — 단순 일괄 갱신이 충분.

### Q6. Cascade delete 의 BE 측 처리?

**결정**: ✅ **BE 가 자동 cascade 안 함**. FE 의 `sessionStore.applyDeletion` 이 transform 안에서 cascade — *FE 단일 책임*. BE 는 PUT 받는 시점에 *이미 cascade 된 layout* 받음.

근거: BE 가 cascade 하면 *2 step PUT* (delete item → BE 가 connector 도 자동 추가 delete → 응답이 더 적은 items) — 사용자가 보낸 layout 과 응답 layout 의 diff 가 ETag CAS 모델과 충돌 (낙관적 lock 의 race window). FE 가 cascade 책임이면 *한 PUT 의 transform 안에서* atomic.

단 *validate 시점* 에 *orphan connector* (endpoint 가 sibling items 에 없음) 는 reject — Q2 의 `ConnectorEndpointMissing`. 즉 FE 의 cascade 누락 시 BE 가 reject 로 정합 보호.

### Q7. JSON Schema 의 enum encoding — `serde(rename_all = "snake_case")` 또는 그대로?

**결정**: ✅ **그대로** (Anchor 는 대문자 enum). `#[serde(rename_all = "UPPERCASE")]` 또는 individual `#[serde(rename = "N")]` 명시.

```rust
#[derive(Deserialize, Serialize, PartialEq)]
enum Anchor {
    #[serde(rename = "N")] N,
    #[serde(rename = "NE")] NE,
    #[serde(rename = "E")] E,
    #[serde(rename = "SE")] SE,
    #[serde(rename = "S")] S,
    #[serde(rename = "SW")] SW,
    #[serde(rename = "W")] W,
    #[serde(rename = "NW")] NW,
    #[serde(rename = "center")] Center,
}
```

ADR-0036 D1 의 JSON 예시 정합.

### Q8. Validation error code mapping — 신규 5 variant

`ValidationError` 의 `code()` mapping (FE error message 정합):

| Variant | Code | Message |
|---|---|---|
| `ConnectorEndpointMissing` | `connector_endpoint_missing` | "Connector endpoint id not found in items[]" |
| `ConnectorInvalidEndpoint` | `connector_invalid_endpoint` | "Connector endpoint cannot reference another connector" |
| `ConnectorSelfLoop` | `connector_self_loop` | "Connector from_id and to_id must differ (self-loop reject, MVP)" |
| `ConnectorInvalidAnchor` | `connector_invalid_anchor` | "Connector anchor must be N/NE/E/SE/S/SW/W/NW/center" |
| `ConnectorInvalidRouting` | `connector_invalid_routing` | "Connector routing must be straight/orthogonal/bezier" |

마지막 두 개는 serde deserialize 가 이미 거절 (enum 외 값 reject) — *implicit*. 그러나 *explicit* 으로 ValidationError 에 두면 future-proof.

---

## §A. Task 목록

| Task | 영역 | 출처 | 예상 소요 |
|---|---|---|---|
| **BE-A** | `crates/http-api/src/schema.rs` 의 `Item::Connector` variant + `Anchor` / `Direction` / `Head` / `Routing` / `StrokeDash` enum + `ValidationError` 5 신규 variant + validate() 의 connector arm + put_layout_handler 의 BBox 자동 재계산 hook + openapi 재발행 + 5+ unit test | ADR-0036 + ADR-0018 D12 amend | 1 commit |

단일 task. BE-only, FE 영향 0 (openapi 갱신 분 외).

---

## §B. Anchor — 변경 대상

| # | 파일 | 변경 |
|---|---|---|
| B1 | `codebase/backend/crates/http-api/src/schema.rs` | `Item::Connector` variant 추가 (ADR-0018 D12 amend 의 Rust 코드). 5 enum (`Anchor`/`Direction`/`Head`/`Routing`/`StrokeDash`). `Point` struct 재사용 (FreeDraw 와 공유). |
| B2 | `codebase/backend/crates/http-api/src/schema.rs` | `ValidationError` 에 5 신규 variant. `code()` mapping. `validate()` 의 prelude 에 `id_index: HashMap<&str, &Item>` build + connector arm (Q1-Q4 의 4 reject + BBox 재계산은 별 hook). |
| B3 | `codebase/backend/crates/http-api/src/sessions.rs` 의 `put_layout_handler` | `validate()` 직전 `recompute_connector_bboxes(&mut layout)` hook 호출. 함수는 schema.rs 에 정의. |
| B4 | `codebase/backend/crates/http-api/src/schema.rs` | `fn recompute_connector_bboxes(layout: &mut CanvasLayout)` 신규. 모든 connector item 을 iterate, `from_id`/`to_id` 의 endpoint 좌표 lookup, `anchor_point` 계산, BBox = min-max → connector 의 `common.x/y/w/h` 직접 mutate. Q4 의 alg. |
| B5 | `codebase/backend/crates/http-api/bin/gen-openapi.rs` (또는 동등 entry) | 재실행 → `dist/openapi.json` 또는 frontend types regenerate. Connector variant 의 schema 가 FE typings 에 반영. |
| B6 | `codebase/backend/crates/http-api/src/schema.rs` 의 test module | 신규 unit test 6종 (§D 참조). |

---

## §C. Acceptance criteria

### C1. Schema round-trip
`Item::Connector` 의 JSON 예시 (ADR-0036 D1) 를 serde 로 deserialize → serialize → 동일성 round-trip. `serde_json::from_str` 이 enum 의 모든 case 받아들임.

### C2. Validate — 성공 case
endpoint 가 alive item 가리키고 anchor / direction / routing 모두 valid 한 connector → `validate()` PASS.

### C3. Validate — reject 4 case
- `from_id` 가 items[] 에 없음 → `ConnectorEndpointMissing`
- `from_id == to_id` → `ConnectorSelfLoop`
- endpoint 가 다른 connector → `ConnectorInvalidEndpoint`
- 정상 case 에 *unknown anchor 값* 보내 deserialize fail (serde level)

### C4. BBox 재계산
사용자가 `connector.x = -9999` 같은 stale BBox 보내도, PUT 응답의 `x/y/w/h` = endpoint anchor 의 실제 BBox. 별 unit test (`recompute_connector_bboxes_test`).

### C5. ETag CAS 정합
Connector 가 추가된 PUT 의 `etag_changes`. 기존 `etag` 가 stale 인 PUT 은 412 reject (변경 없음 — connector 가 다른 변경 흐름과 동일).

### C6. openapi 재발행
`dist/openapi.json` 의 `Item` schema 에 `Connector` variant 가 들어있음. FE 의 `npm run gen-types` 또는 동등 흐름이 새 type 인식.

### C7. 기존 test 회귀 0
prior `cargo test -p http-api` 의 모든 test (Document validation 5종 등) 통과. **376 → 382 PASS / 0 FAIL** 목표 (+6 신규 connector tests).

---

## §D. Self-check 표 — 신규 unit test 6종

| Test | 의도 | 핵심 assertion |
|---|---|---|
| `connector_valid_endpoints_ok` | 정상 connector validate PASS | `validate()` 가 `Ok(())` |
| `connector_endpoint_missing_rejected` | `from_id` 가 sibling 에 없음 → reject | `ValidationError::ConnectorEndpointMissing` |
| `connector_self_loop_rejected` | `from_id == to_id` → reject | `ValidationError::ConnectorSelfLoop` |
| `connector_invalid_endpoint_rejected` | endpoint 가 다른 connector → reject | `ValidationError::ConnectorInvalidEndpoint` |
| `connector_bbox_recomputed` | 사용자 stale BBox → 자동 재계산 | post-`recompute_connector_bboxes` 의 x/y/w/h 가 endpoint anchor BBox 와 동일 |
| `connector_serde_roundtrip` | JSON example → deserialize → serialize 동일 | `serde_json::to_value(deserialized) == original_value` |

---

## §E. Anti-pattern — *하지 말 것*

1. **Validate 에서 connector cascade delete 자동 수행** — Q6 정합 위반. BE 는 cascade 안 함. 사용자가 보낸 layout 의 *orphan connector* 만 reject (사용자 명시 책임 — FE 가 cascade 보낸 후).
2. **`from_id` / `to_id` 의 *transitive* validation** — connector A → connector B → item C 같은 chain 추적 안 함. Q2 의 *직접 endpoint 가 connector 면 reject* 가 충분. (P1 의 multi-hop 시 ADR amend.)
3. **Anchor 좌표 의 client-side 계산 의존** — Q4 정합 위반. BE 가 *진실의 source*. FE 가 보낸 x/y/w/h 무시.
4. **BBox 재계산 의 `to_owned` cascade** — borrow checker 가 끼어들 가능. `recompute_connector_bboxes` 의 구현은 2-pass:
   - Pass 1: `id_index: HashMap<&str, (f64, f64, f64, f64)>` build (endpoint 의 x/y/w/h 만 복사).
   - Pass 2: connector 의 mut borrow + lookup 으로 BBox 갱신.
   - 이 패턴이 borrow checker 친화.
5. **별 endpoint 가 *self* 인 self-loop reject 의 `validate()` arm 누락** — Q3 정합. test C3 의 `connector_self_loop_rejected` 가 cover.

---

## §F. Behavior change 정확성

본 batch 이후 BE 의 *layout PUT* 흐름 변경:

| Step | 기존 | 신규 (본 batch 후) |
|---|---|---|
| 1 | ETag CAS check | 동일 |
| 2 | `validate(&layout)` | `recompute_connector_bboxes(&mut layout)` → `validate(&layout)` |
| 3 | disk atomic write | 동일 |
| 4 | `attach_index.apply_diff` | 동일 |
| 5 | `hub.publish_layout_changed` | 동일 |

즉 *step 2* 에 hook 1개 추가 + validate arm 1개 추가. 다른 step 은 unchanged. 기존 회귀 risk 0.

---

## §G. Verification 순서

1. `cargo check -p http-api` — type-level pass.
2. `cargo test -p http-api` — 382 PASS / 0 FAIL.
3. `cargo build --release -p http-api` — clean.
4. openapi 재발행 + FE 의 types 재생성 (CI 또는 local).
5. 본 batch 의 commit 후, FE handover (0079) 의 wire 작업 unblock.

---

## §H. 의존성 / unblock

- 본 BE batch 는 **0079 FE handover 의 prerequisite**. FE 의 connector renderer / creation gesture / Inspector 는 BE 의 `Item::Connector` schema land 후 시작.
- 단 FE 의 *typings 재생성* 만 의존 — FE 작업의 모든 영역 (renderer / gesture / Inspector) 의 *구조* 는 본 handover + ADR-0036 으로 자족적.

---

## 변경 이력
- 2026-05-19: 초안 land. ADR-0036 Accepted 직후 발주. 단일 task (BE-A) — schema variant + validate + BBox 재계산 + openapi + 6 unit test.
- 2026-05-20: **BE-A land 완료**. baseline 표 갱신 — §C-7 의 `376 → 382` 는 0078 작성 시점 (2026-05-19) 의 BE test 수. 본 land 직전 baseline 은 0080 land 후 **447 PASS / 0 FAIL**, BE-A 후 **453 PASS / 0 FAIL** (+6 신규 connector test). schema.rs 의 `Item::Connector` variant + 5 enum + 3 신규 ValidationError variant + validate() connector arm + recompute_connector_bboxes() 2-pass impl + sessions.rs::put_layout_handler 에 hook + 6 unit test (§D 표 그대로) 모두 wire. **openapi 재발행 (§B5)** 은 본 land 에 미포함 — gen-openapi.rs entry 의 갱신 흐름이 별 batch (FE typings 재생성 시 함께 처리). ADR-0036 변경 이력 amend 도 동봉.
