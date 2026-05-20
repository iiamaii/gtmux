# 0059 — BE work-package: `/api/assets/*` binary endpoint + Settings (ADR-0033)

- 작성일: 2026-05-17
- 발주: FE 통합 agent + 사용자 (image / document asset 연동 준비)
- 정본 ADR: `docs/adr/0033-asset-storage-and-serving.md` (Draft)
- 우선: 🟢 P2+ (image 도구의 placeholder → real asset 완성)
- 예상 BE 작업량: 2-3 일 (Stage 1+2 합산)
- 0056 의 Stage 2 의 정식 work-package 화

---

## 0. 한 줄 요약

`POST /api/assets` (multipart upload, sha256 hash) + `GET /api/assets/{id}` (binary stream + ETag) + boot 시 lazy orphan GC + Settings 의 `assets.*` section. ADR-0033 D1~D9 의 BE implementation.

---

## 1. Stage 분리

### Stage 1 — Storage / handlers / Settings (필수)

| 항목 | 위치 | 의존 |
|---|---|---|
| `crates/http-api/src/assets.rs` 신규 — handler / storage manager | new file | `WorkspaceManager`, `infer` crate |
| `WorkspaceManager::assets_dir()` 추가 — `<workspace>/.assets/` resolve | `workspace.rs` | ADR-0019 |
| `POST /api/assets` multipart handler | `assets.rs` + `lib.rs` route | axum `Multipart` extractor |
| `GET /api/assets/{id}` binary + ETag + Content-Type | `assets.rs` + `lib.rs` route | sha256 ↔ MIME metadata file (D2.1) |
| `DELETE /api/assets/{id}` (P3, manual cleanup) | `assets.rs` + `lib.rs` route | — |
| Settings `assets.*` section | `settings.rs` 또는 `crates/.../config.rs` | TBD — settings 의 hard ceiling validate |

### Stage 2 — Orphan GC + integration (필수, Stage 1 후속)

| 항목 | 위치 |
|---|---|
| Boot 시 GC scan — session_layouts 의 `asset_id` set 수집 → `.assets/` 의 reference 0 unlink | `WorkspaceManager::on_boot` (new hook) |
| Reference set 계산 helper — iterate items[] 의 image/document.asset_id | `crates/http-api/src/assets.rs::collect_referenced_ids` |
| Integration test — fixture session with reference + orphan → restart → unlink 검증 | `lib.rs` tests |

### Stage 3 — Settings UI (FE) (선택, BE land 후)

| 항목 | 위치 |
|---|---|
| Settings dialog 의 `Assets` section UI | `lib/chrome/SettingsDialog.svelte` (또는 별 component) |
| FE upload helper `lib/http/assets.ts` | new file |
| ImageNode / DocumentNode 의 picker + render wire | `ImageNode.svelte` / `DocumentNode.svelte` |

---

## 2. Storage layout (D1 + D2)

```
<workspace_dir>/
  .assets/
    a1b2c3d4e5f6...    ← binary
    a1b2c3d4e5f6.meta  ← JSON sidecar { mime, original_filename?, size_bytes, created_unix }
    f5e6d7c8...
    f5e6d7c8.meta
```

### D2.1 Sidecar `.meta` JSON

binary 자체는 MIME 정보 없음 (extension 도 없음). sidecar JSON 로 MIME / 원본 filename / 크기 / 생성 시각 기록.

```json
{
  "mime": "image/png",
  "size_bytes": 4232,
  "original_filename": "screenshot-2026-05-17.png",
  "created_unix": 1779000000
}
```

- `GET /api/assets/{id}` 가 sidecar 의 mime 을 Content-Type 으로 응답.
- 모든 sidecar 누락 시 fallback `application/octet-stream` + warn log.

---

## 3. Upload handler (Stage 1)

### 3.1 Wire

```
POST /api/assets
Content-Type: multipart/form-data; boundary=...

--<boundary>
Content-Disposition: form-data; name="file"; filename="screenshot.png"
Content-Type: image/png

<binary>
--<boundary>--

  ↓

201 Created
Content-Type: application/json

{ "asset_id": "a1b2c3d4...", "mime": "image/png", "size_bytes": 4232 }
```

### 3.2 흐름

```rust
async fn upload_handler(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else { return service_unavailable("workspace_not_configured") };
    let Some(settings) = state.settings.read().await.assets.clone() else { return ... };

    // 1. multipart field "file" 추출
    let field = match multipart.next_field().await { ... };
    let claimed_mime = field.content_type().map(String::from);
    let original_filename = field.file_name().map(String::from);
    let bytes = field.bytes().await.map_err(...);

    // 2. size cap check (D3 + D3.1)
    if bytes.len() > settings.max_size_bytes {
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(json!({ "error": "payload_too_large", "limit": settings.max_size_bytes }))).into_response();
    }
    if bytes.len() > ASSET_MAX_HARD_LIMIT_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, ...).into_response();
    }

    // 3. magic-byte sniff (D4)
    let sniffed_mime = infer::get(&bytes).map(|t| t.mime_type().to_string()).unwrap_or_default();
    if !mime_compatible(claimed_mime.as_deref(), &sniffed_mime, &settings) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, ...).into_response();
    }

    // 4. allowlist check (D3)
    if !settings.allowed_mimes_includes(&sniffed_mime) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, ...).into_response();
    }
    if ASSET_MIME_HARD_BLOCKLIST.contains(&sniffed_mime.as_str()) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, ...).into_response();
    }

    // 5. sha256 hash + write
    let hash = sha2::Sha256::digest(&bytes);
    let asset_id = hex_lowercase(&hash);
    let assets_dir = wm.assets_dir()?;
    std::fs::create_dir_all(&assets_dir)?;
    let bin_path = assets_dir.join(&asset_id);
    let meta_path = assets_dir.join(format!("{asset_id}.meta"));
    if !bin_path.exists() {
        atomic_write(&bin_path, &bytes)?;
    }
    atomic_write(&meta_path, &serde_json::to_vec(&Meta { mime, size_bytes, ... })?)?;

    // 6. response
    (StatusCode::CREATED, Json(json!({ "asset_id": asset_id, "mime": sniffed_mime, "size_bytes": bytes.len() }))).into_response()
}
```

### 3.3 Tests (Gate 0033-1 ~ 0033-6)

| Gate | 시나리오 | 기대 |
|---|---|---|
| 0033-1 | happy path — small PNG upload | 201 + asset_id 64-hex + GET 검증 |
| 0033-2 | MIME mismatch (claim=png, sniff=html) | 415 |
| 0033-3 | Size > Settings cap | 413 |
| 0033-4 | Size > hard ceiling | 413 |
| 0033-5 | Unsupported MIME (e.g. `application/x-msdownload`) | 415 |
| 0033-6 | 401 without auth | 401 |

---

## 4. Get handler (Stage 1)

```rust
async fn get_handler(
    State(state): State<AppState>,
    AxumPath(asset_id): AxumPath<String>,
) -> Response {
    if !ASSET_ID_REGEX.is_match(&asset_id) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_asset_id" }))).into_response();
    }
    let Some(wm) = state.workspace.as_ref() else { return service_unavailable("workspace_not_configured") };
    let bin_path = wm.assets_dir()?.join(&asset_id);
    let meta_path = wm.assets_dir()?.join(format!("{asset_id}.meta"));

    let bytes = match std::fs::read(&bin_path) {
        Ok(b) => b,
        Err(e) if e.kind() == ErrorKind::NotFound => return (StatusCode::NOT_FOUND, Json(json!({ "error": "asset_not_found" }))).into_response(),
        Err(e) => return internal_error(e),
    };
    let meta: Meta = serde_json::from_slice(&std::fs::read(&meta_path)?)?;

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, meta.mime)
        .header(ETAG, format!("\"{asset_id}\""))
        .header(CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(Body::from(bytes))
        .unwrap()
}
```

### Tests (Gate 0033-7 ~ 0033-10)

| Gate | 시나리오 |
|---|---|
| 0033-7 | GET happy — 200 + Content-Type + ETag |
| 0033-8 | If-None-Match 일치 → 304 Not Modified |
| 0033-9 | invalid asset_id (regex fail) → 400 |
| 0033-10 | not found → 404 |

---

## 5. Orphan GC (Stage 2)

### 5.1 Boot 시 hook

```rust
// crates/http-api/src/workspace.rs (또는 별 module)
pub async fn on_boot_assets_gc(wm: &WorkspaceManager) -> Result<GcReport> {
    let assets_dir = wm.assets_dir()?;
    if !assets_dir.exists() { return Ok(GcReport::empty()); }

    // 1. 모든 session layout 의 asset_id reference 수집
    let mut referenced: HashSet<String> = HashSet::new();
    for entry in std::fs::read_dir(wm.path())? {
        let p = entry?.path();
        if p.extension().and_then(|s| s.to_str()) == Some("json") {
            let bytes = std::fs::read(&p)?;
            let layout: Layout = match serde_json::from_slice(&bytes) {
                Ok(l) => l,
                Err(_) => continue, // corrupt — sidecar 처리 별경로
            };
            for it in &layout.items {
                match it {
                    Item::Image { asset_id, .. } => { referenced.insert(asset_id.clone()); }
                    Item::Document { asset_id: Some(id), .. } => { referenced.insert(id.clone()); }
                    _ => {}
                }
            }
        }
    }

    // 2. .assets/ 의 모든 file 검사
    let mut removed = 0u32;
    let mut kept = 0u32;
    for entry in std::fs::read_dir(&assets_dir)? {
        let p = entry?.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        // sidecar .meta 도 같이 처리
        let base = name.strip_suffix(".meta").unwrap_or(&name);
        if !ASSET_ID_REGEX.is_match(base) { continue; }
        if referenced.contains(base) { kept += 1; } else {
            std::fs::remove_file(&p)?;
            removed += 1;
        }
    }

    Ok(GcReport { removed, kept })
}
```

log: `info!("assets: GC removed {removed} orphan asset(s), kept {kept} asset(s)")`.

### 5.2 Tests (Gate 0033-11 ~ 0033-12)

| Gate | 시나리오 |
|---|---|
| 0033-11 | fixture: session A references X, session B references Y; `.assets/` has X+Y+Z | boot scan → Z removed |
| 0033-12 | corrupt session file → 그 session 무시, 다른 reference 정합 |

---

## 6. Settings (D3 + D3.1)

### 6.1 Settings store 변경

```rust
// crates/.../settings.rs
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AssetSettings {
    pub allowed_image_mimes: Vec<String>,
    pub allowed_document_mimes: Vec<String>,
    pub max_size_bytes: u64,
}

impl Default for AssetSettings {
    fn default() -> Self {
        Self {
            allowed_image_mimes: vec!["image/png".into(), "image/jpeg".into(), "image/webp".into(), "image/gif".into()],
            allowed_document_mimes: vec!["application/pdf".into(), "text/plain".into(), "text/markdown".into()],
            max_size_bytes: 52_428_800, // 50 MiB
        }
    }
}

pub const ASSET_MAX_HARD_LIMIT_BYTES: u64 = 209_715_200; // 200 MiB
pub const ASSET_MIME_HARD_BLOCKLIST: &[&str] = &[
    "application/javascript",
    "application/x-msdownload",
    "application/x-sh",
];
```

### 6.2 PUT settings validate

```rust
pub fn validate_asset_settings(s: &AssetSettings) -> Result<(), ValidationError> {
    if s.max_size_bytes > ASSET_MAX_HARD_LIMIT_BYTES {
        return Err(ValidationError::AssetMaxOverHardCeiling);
    }
    for mime in s.allowed_image_mimes.iter().chain(&s.allowed_document_mimes) {
        if ASSET_MIME_HARD_BLOCKLIST.contains(&mime.as_str()) {
            return Err(ValidationError::AssetMimeInBlocklist);
        }
    }
    Ok(())
}
```

### 6.3 Tests

| Gate | 시나리오 |
|---|---|
| 0033-13 | Settings PUT — max_size > hard ceiling → 400 invalid_settings |
| 0033-14 | Settings PUT — allowed_mimes 에 blocklist 항목 → 400 |
| 0033-15 | Settings happy — 200 + persist |

---

## 7. 의존성 (Crate)

| Crate | 용도 | 비고 |
|---|---|---|
| `sha2` | sha256 hash | `ring::digest::SHA256` 이미 사용 — 그것을 활용 가능 |
| `infer` | magic-byte sniff | light, no_std friendly. v0.x stable |
| `axum_extra` | Multipart extractor | 이미 사용 중일 수 있음 — 확인 후 |

`gix` 또는 `git2` 는 본 work-package 와 무관 (file-stat 의 0060 work-package).

---

## 8. FE 측 후속 wire (참고)

```typescript
// lib/http/assets.ts
export async function uploadAsset(file: File): Promise<AssetUploadResponse> {
  const form = new FormData();
  form.append('file', file);
  const res = await fetch('/api/assets', { method: 'POST', body: form, credentials: 'include' });
  if (res.status === 413) throw new AssetTooLargeError();
  if (res.status === 415) throw new AssetMimeError();
  if (!res.ok) throw new Error(`upload returned ${res.status}`);
  return res.json();
}

export function assetUrl(asset_id: string): string {
  return `/api/assets/${encodeURIComponent(asset_id)}`;
}
```

ImageNode / DocumentNode 의 picker + render wire 는 별 turn (BE Stage 1 land 후).

---

## 9. 완료 기준

ADR-0033 의 §완료 기준 7 항목 + 본 work-package 의 Gate 0033-1 ~ 0033-15 (Stage 1 6 + GET 4 + GC 2 + Settings 3) PASS.

---

## 10. 변경 이력

- 2026-05-17: 초안 — ADR-0033 Draft 정합 + 0056 §3 roadmap 의 Stage 2 정식 work-package 화.
