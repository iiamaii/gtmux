# ADR-0029: Session import/export 파일 포맷 및 API 정책

- 상태: Proposed (2026-05-17)
- 결정자: agent (implementation handoff 준비)
- 관련 ADR:
  - ADR-0006: Canvas Layout 영속화 storage
  - ADR-0018: Canvas Item Data Model — schema v2
  - ADR-0019: Session 과 Workspace 모델
  - ADR-0020: Auth Lifecycle
  - ADR-0023: file_path Item 의 OS-level Open 보안 정책
- 관련 구현:
  - `codebase/backend/crates/http-api/src/sessions.rs::import_handler`
  - `codebase/backend/crates/http-api/src/lib.rs` `/api/sessions/import`
  - `codebase/smoke/02_stage5.sh` Gate 5-11
- 관련 리포트:
  - `docs/reports/0048-session-migration-handover.md` Slice D-4 Sessions Import

## 1. 기존 ADR 분석 결과

import/export 전용 ADR은 존재하지 않는다. 관련 결정은 다음 문서에 흩어져 있다.

| 문서 | 관련 결정 | import/export에 주는 제약 |
|---|---|---|
| ADR-0006 | Session Layout은 plain JSON + atomic write + schema 검증으로 영속화 | import는 검증 통과 전 디스크를 변경하면 안 됨. export는 사용자가 검수 가능한 JSON이어야 함. |
| ADR-0018 | schema v2 `groups[] + items[] + viewport`, terminal item id는 match-or-spawn join key | import는 terminal UUID를 live pool과 즉시 맞추지 않음. 첫 attach에서 match-or-spawn 처리. |
| ADR-0019 | Workspace 안에 named Session file record가 존재. 사용자의 명시 backup 단위는 workspace dir 또는 개별 session file | export/import 대상은 Workspace 전체가 아니라 개별 Session file record가 MVP 범위. |
| ADR-0020 | HTTP API는 cookie auth + Origin/Host check 적용 | import/export API도 인증된 API surface로만 제공. |
| ADR-0023 | `file_path`는 metadata이며 OS-level open은 별도 allowlist + confirm | import된 file_path는 inert bookmark다. import/export가 로컬 파일을 읽거나 열면 안 됨. |

코드에는 이미 `POST /api/sessions/import { name, layout }`가 존재한다. 해당 구현은 schema v2 validation, name conflict 409, atomic write, cache seed를 수행한다. 그러나 export API, 다운로드 파일 envelope, FE UX, privacy warning, asset handling, overwrite 정책은 ADR로 잠겨 있지 않다.

따라서 본 ADR은 기존 import 구현을 정본으로 흡수하고, export와 UX/보안 정책을 추가로 결정한다.

## 2. 문제

사용자는 Session을 파일로 내보내고 다른 Workspace 또는 같은 Workspace에 다시 가져오고 싶다. 이 기능은 다음 요구를 동시에 만족해야 한다.

1. 사용자가 작업한 Canvas Layout을 백업할 수 있어야 한다.
2. import는 기존 Session을 조용히 덮어쓰면 안 된다.
3. terminal process/output/runtime state를 export에 포함한다고 오해하면 안 된다.
4. `file_path`, inline document, note 등 민감 데이터가 export 파일에 들어갈 수 있음을 UI가 명확히 알려야 한다.
5. import된 terminal item은 import 시점에 process를 만들지 않고, attach 시점에 ADR-0018의 match-or-spawn 흐름을 따른다.
6. 향후 asset archive나 workspace sync와 충돌하지 않는 파일 포맷이어야 한다.

## 3. 범위

### 포함

- 개별 Session export
- 개별 Session import
- schema v2 Canvas Layout validation
- name conflict 처리
- terminal item match-or-spawn과의 정합
- file_path/document/note privacy warning
- FE import/export modal UX

### 제외

- Workspace 전체 archive
- multi-machine sync / cloud sync
- CRDT merge
- terminal output/ring buffer export
- child process state export
- binary asset bundle export
- import 시 기존 Session overwrite
- import 즉시 terminal spawn

Workspace 전체 archive와 asset bundle은 P1+ 별도 ADR 대상이다.

## 4. 결정

### D1. MVP export/import 단위는 개별 Session이다

export/import의 MVP 단위는 Workspace 전체가 아니라 **Session file record 1개**다.

정당화:

- ADR-0019에서 Workspace는 storage directory, Session은 named layout snapshot record로 정의된다.
- 사용자가 실질적으로 백업·공유하고 싶은 단위는 “현재 작업 세션”이다.
- Workspace 전체 archive는 lock, settings, allowlist, audit log, server identity까지 포함할 수 있어 별도 보안/마이그레이션 결정이 필요하다.

### D2. export 파일은 `gtmux Session Export Envelope v1`이다

다운로드 파일은 raw layout만 담지 않고 다음 envelope를 사용한다.

```json
{
  "kind": "gtmux.session.export",
  "export_version": 1,
  "exported_at": "2026-05-17T00:00:00Z",
  "session_name": "demo",
  "layout": {
    "schema_version": 2,
    "groups": [],
    "items": [],
    "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
  },
  "metadata": {
    "app": "gtmux",
    "app_version": null
  }
}
```

규칙:

- `kind`는 고정 문자열이다.
- `export_version`은 export envelope 버전이다. Canvas Layout의 `schema_version`과 다르다.
- `layout`은 ADR-0018 schema v2 Layout 그대로다.
- `metadata`는 import 정합성에 영향을 주지 않는다.
- export 파일 확장자는 `.gtmux-session.json`을 권장한다.

raw layout과 envelope를 분리하는 이유:

- 사용자가 파일을 열었을 때 이것이 “gtmux session export”임을 즉시 알 수 있다.
- 향후 archive/export_version migration을 layout schema migration과 독립시킬 수 있다.
- FE가 import 전 session name 기본값을 제공할 수 있다.

### D3. import API의 정본 body는 `{ name, layout }`이다

MVP 서버 API는 기존 구현과 같이 다음 body를 정본으로 한다.

```json
{
  "name": "imported-session",
  "layout": {
    "schema_version": 2,
    "groups": [],
    "items": [],
    "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
  }
}
```

FE는 `.gtmux-session.json` 파일을 읽어 envelope를 검증하고, 사용자가 확정한 target name과 `layout`만 서버에 보낸다.

서버가 envelope 전체를 직접 받는 API는 MVP 범위가 아니다. 이유:

- 현재 구현된 `POST /api/sessions/import`와 정합한다.
- import name 결정은 UX 상 사용자 확인이 필요하다.
- 서버 API surface를 작게 유지한다.

P1+에서 CLI 또는 direct API import 편의를 위해 서버가 envelope도 받을 수 있도록 확장할 수 있다. 단 이 경우에도 최종 write 전 target name은 명시되어야 한다.

### D4. export API는 persisted layout을 읽는다

신규 API:

```http
GET /api/sessions/{name}/export
```

성공 응답:

- `200 OK`
- `Content-Type: application/json`
- `Content-Disposition: attachment; filename="<safe-session-name>.gtmux-session.json"`
- body = D2 envelope

실패 응답:

- `401` auth 실패
- `404` session 없음
- `503` workspace 미설정
- `500` read/serialize 실패

서버 export는 현재 디스크/SessionCache에 commit된 Layout을 읽는다. 현재 Webpage에 아직 debounce 중인 local mutation은 서버가 알 수 없다.

FE 규칙:

- 현재 active Session export 시에는 pending layout mutation을 먼저 flush하거나, 최소 300ms debounce가 끝난 뒤 export한다.
- flush를 보장할 수 없는 구현 단계에서는 “마지막 저장된 상태를 내보냄” 문구를 export modal에 표시한다.

### D5. import는 side-effect-free이다

import는 새로운 Session file record만 생성한다.

import 시점에 하지 않는 것:

- terminal spawn
- terminal kill
- existing Terminal attach
- current active Session 변경
- WebSocket broadcast
- file_path open
- external asset read

import 성공 후 FE는 사용자에게 다음 선택을 제공할 수 있다.

- `Open imported session`
- `Stay here`

`Open imported session`을 선택하면 기존 Session attach 흐름으로 이동한다. attach 시 terminal item은 ADR-0018 D6 match-or-spawn 정책을 따른다.

### D6. name conflict는 overwrite 금지 + rename/import-as로 처리한다

동일한 Session name이 이미 존재하면 서버는 `409 name_conflict`를 반환한다.

서버는 다음을 제공하지 않는다.

- overwrite
- merge
- auto suffix 생성

FE는 conflict 시 사용자에게 새 이름 입력을 요구한다. 추천 기본값은 다음 순서다.

1. export envelope의 `session_name`
2. 파일명에서 `.gtmux-session.json`을 제거한 값
3. `imported-session`

자동 suffix는 UI에서 제안할 수 있지만, 최종 name은 사용자가 확인해야 한다. ADR-0019 D7의 “무분별 생성 방지”와 정합한다.

### D7. import validation은 server-side가 최종 진실이다

FE는 사용자 경험을 위해 사전 검증할 수 있지만, 최종 검증은 서버가 한다.

서버 검증 순서:

1. JSON body size cap
2. `name` 정규식 검증
3. `layout.schema_version == 2`
4. ADR-0018 schema validation
5. group/item id uniqueness
6. parent reference / cycle validation
7. payload별 cap 검증
8. session path conflict 검증
9. atomic write

검증 실패 시 파일은 생성되지 않는다. cache도 갱신되지 않는다.

현재 구현은 `crate::schema::validate(&body.layout)` 후 `atomic_write_session`을 수행하므로 본 결정의 핵심 경로와 정합한다.

### D8. terminal item UUID는 보존한다

export/import는 `type:"terminal"` item의 `id`를 변경하지 않는다.

정당화:

- ADR-0018은 terminal item `id`를 backend Terminal id와 같은 join key로 정의한다.
- import 시 UUID를 새로 발급하면 사용자가 저장한 layout의 terminal identity가 깨진다.
- UUID 충돌 가능성은 낮고, 실제 live pool 정합은 attach 시 match-or-spawn이 처리한다.

단, import 시 같은 Workspace 안의 다른 Session이 동일 terminal UUID를 포함할 수 있다. 이것은 ADR-0021의 multi-session mirror 모델과 충돌하지 않는다. terminal UUID는 여러 Session layout에서 같은 backend Terminal을 가리키는 join key가 될 수 있다.

### D9. file_path는 inert metadata로 import/export한다

`file_path.path`는 export 파일에 문자열로 포함된다. import도 해당 문자열을 그대로 저장한다.

보안 규칙:

- import/export는 해당 path를 읽지 않는다.
- import/export는 해당 path를 canonicalize하지 않는다.
- import/export는 해당 path를 open하지 않는다.
- double-click open 시에만 ADR-0023의 allowlist/confirm/API 검증을 탄다.

FE는 export modal에 “local file paths may be included” 경고를 표시해야 한다.

### D10. inline content는 포함하되 binary asset bundle은 제외한다

export에 포함되는 것:

- text item content
- note title/body
- caption payload
- inline document content
- file_path 문자열
- image/document의 metadata와 `asset_id`

export에 포함되지 않는 것:

- terminal output/ring buffer
- terminal process state
- binary image data
- binary document data
- OS file contents referenced by file_path
- file-open allowlist
- auth/session cookie/token
- settings/password hash/audit log

결과:

- inline document는 다른 Workspace에서도 즉시 복원된다.
- asset-based image/document는 import 후 asset resolver가 해당 `asset_id`를 찾지 못하면 dangling visual state로 표시해야 한다.
- “assets included” archive는 P1+ 별도 ADR에서 `.gtmux-archive` 같은 format으로 결정한다.

### D11. export는 privacy warning을 요구한다

export modal은 최소 다음 사실을 알려야 한다.

- notes/text/document inline content가 파일에 포함될 수 있다.
- local file paths가 파일에 포함될 수 있다.
- terminal output과 process state는 포함되지 않는다.
- import한 terminal panels는 session open 시 새 terminal을 spawn할 수 있다.

UI 문구는 짧아야 한다. 기능 설명을 장황하게 넣지 않고, export 전 확인 영역에 privacy-sensitive 항목만 표시한다.

### D12. import UX는 preview → name confirm → import 순서다

FE 흐름:

1. 사용자가 Import 버튼 클릭
2. file picker에서 `.gtmux-session.json` 또는 `.json` 선택
3. FE가 JSON parse
4. envelope 검증
5. preview 표시
   - source session name
   - item count
   - terminal item count
   - file_path count
   - inline document count
   - asset reference count
6. target Session name 입력/확인
7. `POST /api/sessions/import { name, layout }`
8. 성공 시 `Open imported session` 또는 `Stay here`

import modal에서 바로 current Session을 덮어쓰거나 merge하지 않는다.

### D13. active Session export는 current write와 순서를 맞춘다

현재 active Session export에서 가장 중요한 실패 모드는 “사용자는 방금 움직인 panel까지 포함됐다고 생각하지만, debounce write 전 상태가 export되는 것”이다.

따라서 FE 구현은 다음 중 하나를 만족해야 한다.

- pending layout write를 즉시 flush하는 helper를 추가하고 export 전에 await한다.
- flush helper가 없으면 `sessionStore`의 dirty/pending 상태가 false일 때만 export 버튼을 활성화한다.
- 위 둘 다 없으면 export modal에 “last saved state”를 명시하고, 구현 이슈로 follow-up을 남긴다.

장기적으로는 `sessionStore.flushPendingLayout()` 같은 명시 API를 두는 것이 적절하다.

## 5. 대안

### A1. raw layout JSON만 다운로드

거부. 파일 정체성, versioning, session name 기본값, 향후 archive 확장성이 부족하다.

### A2. import 시 기존 Session overwrite 허용

거부. 사용자 작업 손실 위험이 크다. overwrite는 undo/backup 정책이 별도로 필요하다.

### A3. import 시 terminal을 즉시 spawn

거부. import는 파일 생성 작업이어야 한다. process lifecycle은 Session attach의 책임이다.

### A4. Workspace 전체 zip archive를 MVP로 구현

거부. settings, auth, allowlist, audit, assets, lock file의 포함/제외 정책이 모두 필요하다. 개별 Session export 이후 P1+로 다룬다.

### A5. file_path가 가리키는 실제 파일 내용까지 export

거부. 사용자가 의도하지 않은 민감 파일 유출 위험이 크고, ADR-0023의 “file_path는 inert metadata” 정신과 충돌한다.

## 6. 구현 영향

### Backend

- 유지:
  - `POST /api/sessions/import`
  - `{ name, layout }` request body
  - `201 { name, created_at }`
  - `409 name_conflict`
  - schema validation 후 atomic write
  - **Body cap = 16 MiB** (`sessions::SESSION_PUT_MAX_BYTES`, ADR-0018 D8 의 layout file cap 과 공유) — `lib.rs` 의 import 라우트에 `DefaultBodyLimit::max(SESSION_PUT_MAX_BYTES)` layer 로 적용. 초과 시 axum 가 413 Payload Too Large 자동 응답. PUT layout 과 동일 ceiling — 둘 다 v2 layout 을 쓰므로 같은 accept-band. 회귀 가드: `sessions_import_413_when_body_exceeds_cap` (17 MiB → 413) + `sessions_import_accepts_body_below_cap` (5 MiB → 201).

- 추가 (✅ ship, 2026-05-17 — 0052 work package):
  - `GET /api/sessions/{name}/export` — `crates/http-api/src/lib.rs` outer `/api/*` 라우터의 import 라우트 옆 mount, bearer middleware 자동 적용.
  - `export_handler` (`crates/http-api/src/sessions.rs::export_handler`) — `ExportEnvelope` / `ExportMetadata` Serialize types, `state.session_cache.get_or_load` 재사용 (cache + disk fallback), `Content-Disposition: attachment; filename="<safe>.gtmux-session.json"`.
  - `sanitize_export_filename` — `[A-Za-z0-9_-]` 외 `_` 치환, fallback `"session"`.
  - `rfc3339_utc_now` + `civil_from_unix` — std-only (chrono/time 무도입). Howard Hinnant 알고리즘.
  - 응답 분기: 200 (envelope) / 400 (`invalid_session_name`) / 401 (bearer middleware) / 404 (`{ error:"not_found", name }`) / 500 (`save_failed`) / 503 (`workspace_not_configured`).
  - export tests: Gate 0029-1 (happy + envelope shape + Content-Disposition + RFC3339 length) / 0029-2 (404 not_found) / 0029-3 (401 no-auth) / 0029-4 (400 invalid-name) / 0029-5 (round-trip = export→import-as-dst→GET dst/layout 동일). 5/5 PASS — workspace **373 PASS / 0 FAIL**.

### Frontend

- Session menu 또는 Settings Storage section에 Import/Export 진입점 추가
- Export modal:
  - active Session 기본 선택
  - privacy warning
  - last-saved-state 처리
  - download trigger
- Import modal:
  - file picker
  - envelope/raw layout parse
  - preview
  - target name confirm
  - conflict handling
  - success action: open/stay

### Docs/SSoT

- `docs/ssot/canvas-layout-schema.md`는 현재 schema v1 내용이 남아 있어 ADR-0018 schema v2와 불일치한다. import/export 구현 전 v2 SSoT 정리가 필요하다.
- API 문서 또는 OpenAPI 산출물에 `GET /api/sessions/{name}/export`, `POST /api/sessions/import`를 반영한다.

## 7. 보안 검증

| 위험 | 대응 |
|---|---|
| 악성 JSON import | server-side schema validation + body cap (16 MiB, §6 Backend 의 `SESSION_PUT_MAX_BYTES`) + atomic write 전 검증 |
| path traversal session name | ADR-0019 session name regex + WorkspaceManager path resolver |
| 기존 Session overwrite | 409 conflict, overwrite API 없음 |
| file_path를 통한 로컬 파일 접근 | import/export는 inert string만 처리, open은 ADR-0023 경로 |
| 민감 정보 유출 | export modal privacy warning |
| terminal process 혼동 | terminal output/process state export 금지, attach 시 match-or-spawn |
| XSS payload in text/note/document | import는 저장만 수행, render 시 기존 escape 정책 유지 |
| CSRF | ADR-0020 auth + Origin/Host check |

## 8. 완료 기준

- ADR-0029가 implementation source로 참조된다.
- `POST /api/sessions/import`가 본 ADR과 정합한다.
- `GET /api/sessions/{name}/export`가 추가된다.
- FE Import modal은 preview/name confirm/conflict handling을 제공한다.
- FE Export modal은 privacy warning과 saved-state 처리를 제공한다.
- import된 Session은 성공 후 session list에 표시된다.
- import는 terminal spawn 없이 끝난다.
- imported Session attach 시 terminal item은 ADR-0018 match-or-spawn confirm 흐름을 따른다.

## 9. 변경 이력

- 2026-05-17: 초안. 기존 ADR 분석 결과 import/export 전용 ADR 부재 확인. 기존 `POST /api/sessions/import` 구현을 흡수하고 export envelope/API/UX/security 정책 신규 정의.
- 2026-05-17: **amend ① — BE export endpoint ship**. §6 의 "Backend 추가" 항목 모두 ✅ ship 표기. 0052 work package (`docs/reports/0052-be-session-export-endpoint.md`) 의 §3 / §4 / §5 / §6 / §7 정합 land — export_handler + envelope + tests + ADR 흐름. 핵심 결정: chrono/time 의존성 무도입 (std-only `civil_from_unix` Hinnant 알고리즘), 404 body shape spec 정합 (`{error:"not_found", name}`), `state.session_cache.get_or_load` 재사용. 검증: Gate 0029-1~5 5/5 PASS, workspace 368 → 373 PASS / 0 FAIL.
- 2026-05-17: **amend ② — import body cap 명시 + 16 MiB raise (0053 §6 closed)**. 이전 동작 = axum `Json` 의 default 2 MiB cap (자동 413). FE 의 큰 layout (예: 1000+ items + inline documents) 시 2 MiB 가 부족. 본 amend: `lib.rs` 의 `/api/sessions/import` 라우트에 `DefaultBodyLimit::max(SESSION_PUT_MAX_BYTES)` (16 MiB) layer 추가. `sessions::SESSION_PUT_MAX_BYTES` 의 visibility 를 `pub(crate)` 로 변경해 PUT layout 과 동일 ceiling 공유 (둘 다 v2 layout 을 write 하므로 같은 accept-band). 회귀 가드 2개 추가 — `sessions_import_413_when_body_exceeds_cap` (17 MiB → 413) + `sessions_import_accepts_body_below_cap` (5 MiB → 201). `import_handler` doc-comment 에 413 branch + `SESSION_PUT_MAX_BYTES` reference 명시. 검증: workspace 373 → 375 PASS / 0 FAIL.
