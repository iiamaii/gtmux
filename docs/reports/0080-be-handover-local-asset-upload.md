# 0080 — BE handover: local file 기반 image/document asset upload

- 작성일: 2026-05-20
- 종류: backend work package
- 관련 FE 변경: `file_path` / `image` / `document` 삽입 UX 통일
- 관련 파일:
  - `codebase/frontend/src/lib/files/localFilePicker.ts`
  - `codebase/frontend/src/lib/http/assets.ts`
  - `codebase/frontend/src/lib/canvas/Canvas.svelte`
  - `codebase/frontend/src/lib/canvas/ImageNode.svelte`
  - `codebase/frontend/src/lib/canvas/DocumentNode.svelte`
  - `codebase/frontend/src/lib/canvas/FilePathNode.svelte`

## 1. 현재 FE 계약

FE는 세 도구를 같은 UX 단계로 정렬했다.

1. 사용자가 toolbar에서 `file_path`, `image`, `document` 도구를 선택한다.
2. 캔버스 위치를 클릭한다.
3. 브라우저 local file picker가 열린다.
4. `file_path`는 선택 파일의 브라우저 제공 경로 표시값을 layout item으로 삽입한다.
5. `image` / `document`는 선택 파일을 `POST /api/assets`로 업로드한다.
6. 업로드 성공 응답의 `asset_id` / metadata로 layout item을 삽입한다.
7. 기존 `file_path` / `image` / `document` item은 hover 시 change 버튼으로 다시 local file picker를 열어 교체한다.

중요 제약: 표준 브라우저는 local absolute path를 노출하지 않는다. `file_path`가 현재 저장하는 값은 `File.webkitRelativePath || File.name`이다. absolute path가 필요한 경우 BE/앱 shell 차원의 별도 picker 또는 사용자 승인 기반 native bridge가 필요하다.

## 2. 필요한 신규 BE API

### 2.1 `POST /api/assets`

목적: 브라우저가 선택한 local file bytes를 서버 asset store에 저장한다.

요청:

- `Content-Type: multipart/form-data`
- field `file`: 업로드 파일
- field `kind`: `"image"` 또는 `"document"`

응답 `201 Created`:

```json
{
  "asset_id": "<sha256-hex>",
  "mime": "image/png",
  "file_name": "screenshot.png",
  "size_bytes": 12345,
  "original_w": 1280,
  "original_h": 720
}
```

`original_w` / `original_h`는 image일 때만 필요하다. document는 생략 가능하다.

### 2.2 `GET /api/assets/:asset_id`

목적: `ImageNode`가 `<img src="/api/assets/{asset_id}">`로 렌더링하고, document viewer가 asset-based document를 읽을 수 있게 한다.

응답:

- `200 OK`
- `Content-Type`: 저장된 MIME
- body: 원본 bytes
- `404`: asset 없음

### 2.3 선택 사항: `GET /api/assets/:asset_id/meta`

목적: importer/exporter, inspector, dangling asset 복구 UI가 metadata만 조회할 수 있게 한다.

응답:

```json
{
  "asset_id": "<sha256-hex>",
  "mime": "application/pdf",
  "file_name": "brief.pdf",
  "size_bytes": 99123,
  "created_at": 1760000000
}
```

## 3. 저장/검증 규칙

- `asset_id`는 file bytes의 sha256 hex로 한다.
- 동일 bytes 업로드는 idempotent하게 같은 `asset_id`를 반환한다.
- 저장 위치는 workspace state directory 하위의 `assets/` 같은 전용 디렉터리로 둔다.
- path traversal 불가능해야 한다. `asset_id`는 64자 hex allowlist로 검증한다.
- 업로드 크기 제한이 필요하다.
  - image: MVP 기본 20 MB 이하 권장
  - document: MVP 기본 20 MB 이하 권장
  - inline document 64 KB 제한과 별개다. asset-based document는 큰 파일을 허용할 수 있다.
- MIME은 client 제공값을 신뢰하지 말고 최소한 extension + magic byte 기반 검증을 한다.
- image 허용 MIME: `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/svg+xml`
- document 허용 MIME: `text/*`, `application/json`, `application/pdf` 등 MVP allowlist로 시작한다.
- SVG는 active content 위험이 있으므로 그대로 serve할지, sanitize할지, `Content-Disposition`/CSP를 어떻게 둘지 결정해야 한다.

## 4. Layout schema 정합

현재 BE schema는 이미 asset-based document를 허용한다.

- `Image`: `asset_id`, `mime`, `original_w?`, `original_h?`
- `Document`: `asset_id: Some`, `content: None`, `mime`, `file_name`, `size_bytes`

`POST /api/assets`가 완료되면 FE는 다음 shape으로 layout mutation을 보낸다.

Image:

```json
{
  "type": "image",
  "asset_id": "<sha256>",
  "mime": "image/png",
  "original_w": 1280,
  "original_h": 720
}
```

Document:

```json
{
  "type": "document",
  "asset_id": "<sha256>",
  "content": null,
  "mime": "application/pdf",
  "file_name": "brief.pdf",
  "size_bytes": 99123
}
```

Serde에서는 `content`가 omit되는 것이 더 자연스럽다. FE는 `content: undefined`로 보내도록 구현되어 있다.

## 5. Acceptance checklist

- [ ] `POST /api/assets` multipart endpoint 추가.
- [ ] Cookie auth / existing auth middleware 적용.
- [ ] `kind=image|document` 검증.
- [ ] upload size cap 적용 및 초과 시 `413 Payload Too Large`.
- [ ] sha256 계산 및 content-addressed 저장.
- [ ] 동일 bytes 재업로드 idempotent 응답.
- [ ] MIME allowlist 및 extension/magic byte 검증.
- [ ] `GET /api/assets/:asset_id` endpoint 추가.
- [ ] `asset_id` 64-char hex validation 및 traversal 차단.
- [ ] missing asset은 `404 { "error": "asset_not_found" }`.
- [ ] image upload는 가능하면 `original_w` / `original_h` 계산.
- [ ] document upload는 `file_name`, `mime`, `size_bytes` 반환.
- [ ] integration test: image upload → GET bytes roundtrip.
- [ ] integration test: document upload → asset-based `Document` layout validation 통과.
- [ ] integration test: oversize upload reject.
- [ ] integration test: invalid `asset_id` path reject.
- [ ] integration test: unauthorized upload reject.

## 6. FE 확인 시나리오

BE 구현 후 FE에서 확인할 흐름:

1. image tool 선택 → canvas 클릭 → PNG 선택 → node 생성 → 이미지가 `/api/assets/:id`로 렌더링.
2. image node hover → change 버튼 → 다른 이미지 선택 → 같은 node의 `asset_id` 갱신.
3. document tool 선택 → canvas 클릭 → PDF/MD 선택 → document node 생성.
4. document node hover/header change 버튼 → 다른 document 선택 → metadata 갱신.
5. file_path tool 선택 → canvas 클릭 → 파일 선택 → path reference item 생성.

## 7. 후속 결정 필요

- Browser 환경에서 `file_path`에 absolute local path를 저장할 수 없다. 정말 absolute path가 제품 요구라면 web 표준 picker가 아니라 native bridge 또는 BE-side picker를 유지해야 한다.
- Session export/import에서 binary assets를 포함할지 여부는 ADR-0029 D10에 따라 별도 ADR 대상이다.
- ~~SVG serve 정책은 보안 결정이 필요하다.~~ → 본 land (2026-05-20) 에서 결정: `image/svg+xml` 응답에 `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox` 헤더 stamp. `<img src>` 안에서는 browser sandbox 가 자연 보호하고, 직접 URL navigation 으로도 inline script / event handler / foreignObject script 가 발화하지 않는다. ADR-0033 amend ① §변경 이력 참조.

## 8. Land 상태 (2026-05-20 amend)

본 handover 의 §2~§5 가 backend 에 land 되었다. acceptance checklist 매핑:

- [x] `POST /api/assets` multipart endpoint — `crates/http-api/src/assets.rs::upload_handler`.
- [x] Cookie/Bearer auth middleware — 기존 `/api/*` 게이트와 동일.
- [x] `kind=image|document` 검증 — `AssetKind` enum + 400 `missing_kind` / `invalid_kind`.
- [x] Upload size cap 20 MiB — `ASSET_MAX_BYTES` + 라우트 `DefaultBodyLimit::max(20MiB + 1MiB headroom)` + 핸들러 내부 recount → 413.
- [x] sha256 hex + content-addressed 저장 — `sha256_hex()` + `<workspace>/.assets/<sha256>` atomic write.
- [x] 동일 bytes idempotent — `assets_idempotent_same_bytes` integration test 가 보장.
- [x] MIME allowlist + magic-byte sniff — hand-rolled `sniff_image` / `sniff_document` (PNG/JPEG/GIF/WebP/SVG, PDF/JSON/text). client `Content-Type` 은 신뢰하지 않음.
- [x] `GET /api/assets/:asset_id` endpoint — `serve_handler`, `Content-Type` re-sniff + `Cache-Control: immutable` + ETag.
- [x] `asset_id` 64-char lowercase-hex validation + traversal 차단 — `is_valid_asset_id()`.
- [x] missing asset → 404 `asset_not_found`.
- [x] image dimensions — PNG / JPEG / GIF / WebP-VP8/VP8L/VP8X 파서. SVG 는 None (XML parser 의존 회피).
- [x] document 응답에 `file_name` / `mime` / `size_bytes`.
- [x] integration test — image roundtrip / idempotent / oversize 413 / invalid asset_id 400 / unauthorized 401 / kind/MIME mismatch 415 / document PDF roundtrip (7 종, 0080 §5 의 5+2).
- [ ] Settings UI / hard ceiling — ADR-0033 amend ② 로 분리.
- [ ] Boot orphan GC + DELETE endpoint — ADR-0033 amend ③ 로 분리.
- [ ] Session export/import 의 `.assets` portability — ADR-0033 amend ④ + ADR-0029 paired amend 로 분리.

FE 측 §6 시나리오 (image / document / file_path 도구) 는 BE land 직후 dev 환경에서 사용자 검증 — `pickLocalFile` + `uploadAsset` 의 wire 가 이미 본 endpoint 응답 모양에 맞춰져 있다.
