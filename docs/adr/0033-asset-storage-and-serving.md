# ADR-0033: Asset storage and serving — binary `image`/`document` asset endpoint

- 상태: **Accepted** (2026-05-20, MVP amend ①)
- 일자: 2026-05-17 (Draft) → 2026-05-20 (Accepted, amend ① 동봉)
- 결정자: agent (system-architect role) + 사용자 grilling (4 항목)
- 근거 grilling: 2026-05-17 — image / document / file-path 연동 준비 grilling
- 근거 plan: `docs/reports/0080-be-handover-local-asset-upload.md` — 본 ADR MVP scope 정본 (handover land 후 Accepted promote)
- 관련 ADR: ADR-0018 (Canvas Item v2 — image/document payload), ADR-0019 (Workspace 모델 — `.assets/` 위치), ADR-0020 (Auth — bearer/cookie), ADR-0029 (Session import/export — `.assets` portability)
- 관련 work package: `docs/reports/0080-be-handover-local-asset-upload.md` (Accepted 의 정본)
- Supersedes: ADR-0018 line 102 의 `image/document asset storage 정책은 ADR-0018 후속 또는 별 ADR (P2+)` deferred 영역

## 맥락

ADR-0018 D4 의 `image`/`document` (asset-based mode) 는 `asset_id: String (sha256)` 필드 만 정의하고 실제 binary 저장/서빙은 P2+ 로 미루었음. 본 sprint 의 Toolbar 도구 land 후 image 도구는 *placeholder Node* 만 표시 (`asset_id=''`) — 사용자 UX 완성에 binary endpoint 필수.

본 ADR 은 `/api/assets/*` endpoint 의 (a) storage location, (b) MIME / size cap, (c) GC 정책, (d) 인증, (e) 공유, (f) 추론 정책 6 차원을 잠금.

## 결정 (Decisions)

### D1. Storage location — `<workspace>/.assets/<sha256>`

asset binary 는 *workspace root 안 hidden directory `.assets/`* 에 sha256 hex 파일명으로 저장.

```
<workspace_dir>/
  .assets/
    a1b2c3d4...    ← binary (no extension; MIME 은 별도 metadata)
    f5e6d7c8...
  <session_a>.json
  <session_b>.json
  ...
```

**근거**:
- 사용자가 workspace 를 그대로 backup / sync 하면 asset 도 자동 동반.
- ADR-0029 session import/export 시 `.assets` 도 envelope 에 함께 포함 가능 (P2+, 별 amend).
- portability 우선 — `<XDG_DATA_HOME>/gtmux/assets/` 분리 case 보다 정합.

**비채택**: A1. `<XDG_DATA_HOME>/gtmux/assets/<sha256>` (workspace 분리) — cross-workspace 공유 효율은 좋으나 import 시 asset 분실 위험 + backup 분리 부담.

### D2. `asset_id` = sha256 hex (lowercase, 64 chars)

binary 전체의 SHA-256 digest 를 hex 로 인코딩한 64-char lowercase string. 충돌 확률 1/2^128 — 실용상 0.

- `[a-f0-9]{64}` regex 로 1차 validate.
- 같은 sha256 의 asset 이 여러 layout 에 reference 되어도 single physical file 공유 (D8).

**비채택**: UUID v4 — content-addressing 가치 (deduplication / cache validity) 손실.

### D3. MIME allowlist + size cap — Settings-driven default

**Default**:
- `assets.allowed_image_mimes`: `["image/png", "image/jpeg", "image/webp", "image/gif"]`
- `assets.allowed_document_mimes`: `["application/pdf", "text/plain", "text/markdown"]`
- `assets.max_size_bytes`: `52_428_800` (50 MiB unified)

**Settings UI**:
- `Settings → Assets` section 신규 — 사용자가 위 3 항목 조정 가능.
- 사용자 조정 값은 server 의 *hard ceiling* (D3.1) 안에서만 적용 — 초과 시 settings 저장 자체 reject.

### D3.1 Hard ceiling (server config-only, not user-overridable)

- `ASSET_MAX_HARD_LIMIT_BYTES`: 200 MiB — Settings 의 `max_size_bytes` 가 이를 넘으면 400 invalid_settings.
- `ASSET_MIME_HARD_BLOCKLIST`: `["application/javascript", "application/x-msdownload", "application/x-sh"]` — 사용자가 allowlist 에 추가해도 server 가 reject.

**근거**: DOS / security floor. Settings 가 사용자 flexibility 를 주되 server admin (CLI 또는 config file) 만이 hard ceiling 조정 가능.

**비채택**: A2. hard MIME allowlist (Settings 미허용) — 사용자가 명시 거부 ("설정에서 정할 수 있도록").

### D4. Content-Type 추론 — magic-byte sniff + client MIME 둘 다 검증

업로드 시:
1. multipart `Content-Type` 또는 form field `mime` 값 → client-claimed.
2. binary 의 첫 N bytes (e.g. 32) magic-byte sniff → server-verified.
3. 두 값이 *같은 MIME 또는 양립 가능* 일 때만 통과. 불일치 시 **415 Unsupported Media Type**.

**근거**:
- client MIME 만 신뢰 → wrong-extension upload / XSS payload risk.
- magic-byte only → client 가 보낸 `image/jpeg` 가 실제로 `text/html` 인 경우 reject.

**구현 노트**: Rust crate 후보 — `infer` (light, no_std-friendly) 또는 `mime_sniffer`. crate-doc 비교 후 work-package 에서 결정.

**비채택**: A3. client MIME 신뢰 only — security floor 미달.

### D5. Endpoint wire

```
POST   /api/assets                        → 201 { asset_id, mime, size_bytes }
                                            multipart/form-data, field `file` (binary)
GET    /api/assets/{asset_id}             → 200 binary stream
                                            Content-Type, ETag = "<asset_id>", Cache-Control: immutable
DELETE /api/assets/{asset_id}             → 204 (P3 — orphan 수동 cleanup, 보통 GC 처리)
```

**Error 분기**:
| Code | 의미 |
|---|---|
| 400 | `invalid_asset_id` (regex 실패), `missing_file` (multipart `file` field 없음) |
| 401 | (auth middleware — bearer/cookie 둘 다 없음) |
| 404 | `asset_not_found` (GET / DELETE) |
| 413 | `payload_too_large` (cap 초과 — Settings 또는 hard ceiling) |
| 415 | `unsupported_media_type` (MIME allowlist 미통과 또는 sniff mismatch) |
| 503 | `workspace_not_configured` |

### D6. 인증

`/api/*` middleware 의 bearer/cookie 두 path 모두 정합 (다른 endpoint 와 동일, ADR-0020 D2).

**비채택**: public asset 직접 서빙 (인증 없음) — single-user workspace 의 정합성 약화 + Cloud mode 외부 노출 시 risk.

### D7. Orphan GC — boot 시 lazy scan

Server 부팅 시 1회:
1. `<workspace>/<session_*>.json` 모두 read → `items[].asset_id` 수집 → reference set.
2. `<workspace>/.assets/` 의 파일 list → reference set 안 없으면 unlink.
3. log: `assets: GC removed N orphan asset(s), kept M asset(s)`.

**근거**:
- 구현 단순 (atomic, lockless — boot 시점은 다른 mutation 없음).
- 장기 실행 server 에 잘 맞음 (월 단위 boot 일반적이면 1회 cleanup).
- eager (item delete 시 즉시 unlink) 패턴은 reference count 추적 부담 — 별 layout 도 scan 필요.

**비채택**: A4. eager GC — atomic 하지만 cross-session reference 추적 복잡. A5. cron-style 주기적 — boot scan 의 보완 (extra timer), 첫 ship 에서 skip.

**후속 amend 가능성**: layout PUT 시 *그 session 의* removed asset_id 만 lazy-check 하는 mini-GC 추가 검토 (boot scan 의 보완).

### D8. 공유 / reference count

한 sha256 의 asset 이 여러 layout 또는 여러 item 에서 reference 가능. *공유 허용 default*. GC 는 reference set 의 size 만 check (≥ 1 → keep).

**근거**: SHA-256 deduplication 의 자연 효과 — 같은 image 를 두 곳에 paste 해도 storage 1배.

### D9. ETag = `"<asset_id>"`

asset_id 자체가 content hash 이므로 ETag 와 동일. 브라우저 `If-None-Match` → 304 자연 정합. `Cache-Control: immutable` 도 함께 send (sha256 이 변하지 않으면 binary 도 변하지 않음).

## 어휘 매트릭스 (CONTEXT.md 정합)

- **Asset** = binary 콘텐츠 (image / document) 의 server-side persistent record.
- **Asset id** = sha256 hex hash.
- **Asset reference** = layout item 의 `asset_id` field.

## 대안 검토

본 결정의 4 거부된 대안:
- A1. XDG_DATA_HOME 분리 (D1)
- A2. hard MIME allowlist (D3)
- A3. client MIME 신뢰 only (D4)
- A4. eager GC (D7)
- A5. cron-style GC (D7) — 첫 ship 비채택, 후속 amend 가능

## 영향

### Code

**Backend**:
- 새 module `crates/http-api/src/assets.rs` — handler + storage manager + GC.
- 새 dependency: `sha2` (이미 ring 의 SHA-256 사용 가능 — `digest::SHA256` activate), `infer` (magic-byte sniff, light crate).
- `WorkspaceManager` extension: `assets_dir()` 메서드, boot 시 GC scan hook.
- multipart parsing: axum extractor `Multipart` 또는 `axum_extra::extract::Multipart`.
- Settings store amend: `assets.allowed_image_mimes` / `allowed_document_mimes` / `max_size_bytes`.

**Frontend**:
- `lib/http/assets.ts` 신규 — `uploadAsset(file)` + `assetUrl(asset_id)` helper.
- `ImageNode.svelte` — file picker (drag-and-drop + button) + upload → `asset_id` set → `<img src="/api/assets/{asset_id}">`.
- `DocumentNode.svelte` — asset-based mode 의 download 버튼 (현 placeholder).
- Settings UI — Assets section 신규 (mime list editor, cap slider 또는 number input).

### ADR

- ADR-0018 D4 의 image/document payload — 본 ADR reference (asset storage 정책 정본).
- ADR-0029 — `.assets` 의 import/export portability 검토 (후속 amend).

### Docs

- `docs/reports/0059-be-asset-storage-work-package.md` (예정) — Stage 분리 + Gate test 정의.
- `docs/ssot/security-defaults.md` — assets section 추가 (Settings hard ceiling).
- `plan-0011` — image 의 후속 wire (FE-after-BE-ship).

### 보안

- Path traversal: `asset_id` 가 sha256 regex 통과한 후에만 path resolve (`.assets/<sha256>` 만 — `..` segment 자연 차단).
- MIME sniff: magic-byte 검증으로 wrong-MIME upload (XSS payload, script masquerade) 차단.
- Hard ceiling: Settings 의 사용자 조정이 server 안정성을 위협 못 함.
- 인증: bearer/cookie middleware — single-user 가정.

## 완료 기준

본 ADR Accepted 후 별 plan / work-package 의 진행 기준:

1. `POST /api/assets` happy path — 200 → upload → GET 검증.
2. MIME sniff vs claim mismatch → 415 → asset 저장 안 됨.
3. Hard ceiling 초과 → 413.
4. Boot orphan GC — fixture 로 reference 0 asset 생성 → restart → unlink 검증.
5. Settings 의 `max_size_bytes > hard ceiling` → 400 invalid_settings.
6. FE Image 도구 — file picker → upload → ImageNode 가 real image render.
7. FE Document 도구 (asset-based) — `asset_id` set 시 download 버튼 wire.

## 변경 이력

- 2026-05-17: **Draft** — 사용자 grilling 4 항목 + 0056 §3 roadmap 정합. D1~D9 결정. 별 plan / 0059 BE work-package 진행 후 Accepted promote.
- 2026-05-20: **Accepted** (amend ①) — 0080 BE handover 의 MVP scope 로 land. Draft 의 다음 항목 amend:
  - **D3 size cap**: MVP `20 MiB` (Settings UI 미land — 사용자 조정 UI 는 P2 amend ② 로 분리). Settings hard ceiling (D3.1) 도 함께 P2 로 이연.
  - **D5 endpoint shape**: MVP 응답에 `file_name` / `original_w?` / `original_h?` 추가 — FE 의 ImageNode/DocumentNode 가 panel seed 시 natural aspect ratio 와 download filename 을 즉시 사용. multipart field `kind: "image"|"document"` 필수.
  - **D4 MIME allowlist (MVP)**:
    - image: `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/svg+xml`
    - document: `application/pdf`, `application/json`, `text/plain` (UTF-8 fallback)
    - sniff: hand-rolled magic-byte parser (no `infer` crate dep) — PNG/JPEG/GIF/WebP/SVG/PDF/JSON 인식. Draft 의 `infer` crate 권고는 의존성 절감 명목으로 비채택.
  - **D5 SVG serve 정책**: `<img src>` 에서는 browser sandbox, 그러나 직접 navigation 시 script 실행 가능 — 본 amend 는 `image/svg+xml` 응답에 `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox` 헤더 stamp. 이로써 직접 URL 입력해도 inline `<script>` / event handler / foreignObject script 가 활성화되지 않음.
  - **D7 GC (boot orphan scan)**: MVP 미land. 사용자가 명시적으로 P3 cleanup 으로 분리 — `0080 §5` 의 acceptance checklist 도 GC 항목을 누락. 후속 별 work-package 로 분리.
  - **DELETE endpoint**: MVP 미land (D5 의 `DELETE /api/assets/{id}` 줄은 P3 GC 와 함께 분리).
  - 본 land: `crates/http-api/src/assets.rs` 신규 + `WorkspaceManager::assets_dir()` + `ensure_assets_dir()` + lib.rs 라우터 등록 (`POST /api/assets`, `GET /api/assets/{id}`) + integration test 7종 (image roundtrip, idempotent, oversize 413, invalid asset_id 400, unauthorized 401, kind/MIME mismatch 415, document PDF roundtrip) + unit test 11종 (sniff / dimensions / sha256 / asset_id validation).
  - 후속 amend (P2/P3):
    - amend ②: Settings UI + hard ceiling (D3 / D3.1) — 사용자가 cap / MIME 조정 가능하게.
    - amend ③: Boot orphan GC (D7) + DELETE endpoint — reference set scan + unlink.
    - amend ④: Session import/export 의 `.assets` portability (ADR-0029 와 paired amend).
