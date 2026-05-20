# SSoT: Canvas Layout Schema

- 일자: 2026-05-13
- 정의 ADR: ADR-0010 (Group 데이터 모델, G-hybrid). ADR-0006 (영속화 storage)에서 추가 정합.
- 변경 정책: 본 스키마는 HTTP `PUT /api/layout` 페이로드와 `GET /api/layout` 응답의 1차 계약이다. 변경은 PR + ADR-0010 또는 ADR-0006 갱신 동반. 코드는 본 문서를 직접 참조해 구현해야 한다.
- 관련 보고서: `docs/reports/0010-grill-amendments.md` D11·D12, `docs/reports/0011-coherence-review.md` G1·G4

본 SSoT는 ADR-0010의 부속 산출물로 초안 작성됨 (코히런스 리뷰 G1 해소). ADR-0006 dispatch 시 storage backend 결정(sqlite vs JSON file 등)에 맞춰 직렬화 디테일 확장 예정.

## 1. JSON Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "gtmux Canvas Layout",
  "type": "object",
  "required": ["etag", "groups", "panels"],
  "additionalProperties": false,
  "properties": {
    "etag": {
      "type": "string",
      "pattern": "^[0-9a-f]{32}$",
      "description": "16바이트 raw ETag의 lowercase hex 인코딩 (32자). §2 ETag 정규화 참조."
    },
    "schema_version": {
      "type": "integer",
      "const": 1,
      "description": "MVP 고정값 1. 마이그레이션 도입 시 ADR-0006에서 확장."
    },
    "groups": {
      "type": "array",
      "items": { "$ref": "#/$defs/Group" }
    },
    "panels": {
      "type": "array",
      "items": { "$ref": "#/$defs/Panel" }
    }
  },
  "$defs": {
    "Group": {
      "type": "object",
      "required": ["id", "parent_id", "label", "color", "visibility", "locked", "order"],
      "additionalProperties": false,
      "properties": {
        "id":         { "type": "string", "pattern": "^g[0-9a-zA-Z]{1,32}$" },
        "parent_id":  { "type": ["string", "null"], "pattern": "^g[0-9a-zA-Z]{1,32}$" },
        "label":      { "type": ["string", "null"], "maxLength": 128 },
        "color":      { "type": ["string", "null"], "pattern": "^#[0-9a-fA-F]{6}$" },
        "visibility": { "type": "boolean" },
        "locked":     { "type": "boolean" },
        "order":      { "type": "integer", "minimum": 0 }
      }
    },
    "Panel": {
      "type": "object",
      "required": ["id", "parent_id", "pane_id", "x", "y", "w", "h", "z", "visibility", "minimized", "locked"],
      "additionalProperties": false,
      "properties": {
        "id":        { "type": "string", "pattern": "^p[0-9a-zA-Z]{1,32}$" },
        "parent_id": { "type": ["string", "null"], "pattern": "^g[0-9a-zA-Z]{1,32}$" },
        "pane_id":   { "type": "string", "pattern": "^%[0-9]+$",
                       "description": "tmux pane id (예: %0, %1)" },
        "x":         { "type": "number" },
        "y":         { "type": "number" },
        "w":         { "type": "number", "exclusiveMinimum": 0 },
        "h":         { "type": "number", "exclusiveMinimum": 0 },
        "z":         { "type": "integer" },
        "visibility":{ "type": "boolean" },
        "minimized": { "type": "boolean" },
        "locked":    { "type": "boolean" },
        "label":     { "type": ["string", "null"], "maxLength": 128 },
        "note":      { "type": ["string", "null"], "maxLength": 2048 }
      }
    }
  }
}
```

### 1.1 필드 의미 보강

| 필드 | 의미 |
|---|---|
| `Group.id` / `Panel.id` | 클라이언트 발급 ULID/UUID (`g` / `p` prefix). 서버가 검증만 함. |
| `Group.parent_id` / `Panel.parent_id` | `null` = Canvas 루트 자식. 아니면 같은 페이로드 내 `Group.id` 참조. |
| `Group.color` | CSS hex color (`#RRGGBB`). `null` = 색 없음, ancestor에서 inherit (ADR-0010 D6). |
| `Group.visibility` / `Panel.visibility` | self 상태. effective = self AND 모든 ancestor (ADR-0010 D6). |
| `Group.locked` / `Panel.locked` | self 상태. effective = self **OR** 모든 ancestor (lock은 cascade-down: ancestor가 잠기면 자손도 잠긴다). |
| `Panel.minimized` | web-only. visibility=true이지만 작은 배지로만 렌더. Panel Streaming State Suspended 트리거. |
| `Panel.pane_id` | tmux pane id (mirror). 서버가 현재 mirror된 pane set과 정합성 검증 (§3 R3). |
| `Panel.note` | 사용자 메모. tmux로 절대 전달되지 않음. |
| `order` | 형제 노드 내 정렬 키 (사이드바 layer panel 순서). |

## 2. ETag 정규화 (G4 해소)

ETag는 **16바이트 raw**가 정본이며, 인코딩 표현은 채널에 따라 다음 규칙으로 *결정적*으로 변환한다.

| 위치 | 표현 | 변환 |
|---|---|---|
| 서버 내부 / 영속화 | 16-byte raw bytes | 정본 |
| WS envelope `0x80 LAYOUT_CHANGED` payload | 16-byte raw bytes | 그대로 |
| HTTP JSON body (`GET /api/layout` 응답) | 32자 **lowercase hex 문자열** | `hex.EncodeToString(bytes)` |
| HTTP `ETag` 헤더 (응답) | 표준 quoted-string `"<32-hex>"` | RFC 7232 따름 |
| HTTP `If-Match` 헤더 (요청) | 표준 quoted-string `"<32-hex>"` | 비교 시 양쪽 quote 제거 + 소문자 정규화 후 raw bytes로 디코딩하여 상수시간 비교 |

검증 규칙:
- 서버는 hex 문자열 검증 시 lowercase 강제 (`^[0-9a-f]{32}$`). uppercase는 거부.
- 모든 비교는 raw 16-byte로 환원 후 상수시간 (`crypto.subtle.timingSafeEqual` 또는 동등).

## 3. 페이로드 검증 규칙

`PUT /api/layout`이 요청을 받으면 서버는 다음을 *순서대로* 검증한다. 하나라도 실패하면 400 Bad Request + 검증 오류 항목 목록 반환.

- **R1. JSON Schema 적합성** — §1의 schema에 합치.
- **R2. ID 유일성** — `groups[].id`·`panels[].id`가 페이로드 안에서 각각 유일. 서로 다른 prefix(`g`/`p`)로 자연 격리.
- **R3. `Panel.pane_id` 존재성** — 서버가 현재 tmux daemon에서 mirror 중인 pane 집합 안에 있어야 함. 모르는 pane id는 거부.
- **R4. 트리 정합성** — `parent_id` 참조가 같은 페이로드의 `Group.id`에 존재하거나 `null`. 외부 ID 금지.
- **R5. 사이클 금지** — Group 트리에 사이클 없음. DFS 검증.
- **R6. 다중 부모 금지** — Schema가 자연히 보장 (각 노드 1개 `parent_id`). 검증 룰로 명시.
- **R7. `Panel.parent_id`도 Group만 참조** — Panel이 Panel 또는 자기 자신의 부모가 될 수 없음. Schema의 `pattern: ^g...`로 보장.
- **R8. ETag 헤더 매칭** — `If-Match` 헤더의 ETag가 서버 현재 ETag와 일치. 불일치 시 412 Precondition Failed.
- **R9. 페이로드 크기** — 최대 256 KB (소프트 캡). 초과 시 413 Payload Too Large.

## 4. HTTP API 계약

### 4.1 `GET /api/layout`

- 응답: 200 OK + JSON body (위 schema 합치) + `ETag: "<32-hex>"` 헤더.
- 인증 실패: 401 Unauthorized. 토큰은 ADR-0003 정책 따름.
- 빈 초기 상태(부팅 후 PUT 전): 200 OK + `{"etag":"00000000000000000000000000000000","schema_version":1,"groups":[],"panels":[]}`.

### 4.2 `PUT /api/layout`

- 요청 헤더: `If-Match: "<32-hex>"` 필수. `Authorization: Bearer <token>`.
- 요청 body: JSON, schema 적합.
- 응답 성공: 204 No Content + 새 `ETag: "<32-hex>"` 헤더 + WS `0x80 LAYOUT_CHANGED` 브로드캐스트 (모든 활성 WS 연결).
- 응답 실패:
  - 400: schema·검증 룰 위반. body에 오류 배열.
  - 401: 인증 실패.
  - 412: ETag mismatch. 클라는 GET 재호출 후 머지·재시도 (보고서 D12).
  - 413: 페이로드 너무 큼.

### 4.3 PATCH 미지원 (MVP)

MVP는 PUT 전체 교체만. PATCH 델타 전송은 P1+로 미룬다 (sketch §11.3 제외 목록 추가됨).

## 5. 변경 이력

- 2026-05-13: 초안 (코히런스 리뷰 G1 해소, ADR-0010 부속). schema_version = 1.
