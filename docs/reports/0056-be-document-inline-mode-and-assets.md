# 0056 — Document inline-stored mode (BE schema 정합) + Assets endpoint roadmap

- 작성일: 2026-05-17
- 종류: BE work package + FE 공유 doc — Document/Image 의 BE 측 implementation gap 정리 + 두 단계 ship plan
- 발주: 사용자 grilling ("toolbar 의 free form / document / image 들도 server api 에 구현되어있나?")
- 정본 ADR: **ADR-0018 D10 amend ① (2026-05-16, components batch)** — Document 의 inline-stored mode 도입
- 관련 시안: `ref/frontend-design/components.html §02` (inline-editable document)
- 우선순위: 🟡 P1 — Document inline mode (즉시 BE work, 1-stage), 🟢 P2+ — Assets binary endpoint (별 ADR, 2-stage)

---

## 0. 한 줄 요약

Toolbar 의 free-form / document / image 중 **FreeDraw 는 BE 완비**, **Image/Document 는 schema 만 land + 실 binary endpoint 미 ship**. ADR-0018 D10 amend ① 의 *Document inline-stored mode* 가 schema 에 미반영 (drift) — 본 work 의 **1단계** 가 해소. **2단계** 의 `/api/assets/*` binary endpoint 는 별 ADR 후속.

---

## 1. 현황 진단 (2026-05-17 시점)

### 1.1 Toolbar tool별 BE 측 ship 상태

| 항목 | Schema | Validation | Layout I/O | Binary endpoint | 종합 |
|---|---|---|---|---|---|
| **FreeDraw (free form)** | ✅ `Item::FreeDraw { points: Vec<Point>, stroke, stroke_width }` (`schema.rs:246`) | ✅ `FREE_DRAW_POINT_CAP` 검증 | ✅ PUT/GET/import/export 자동 | n/a — points 가 layout 안 inline | **완비** |
| **Image** | ✅ `Item::Image { asset_id, mime, original_w?, original_h? }` (`schema.rs:253`) | ⚠ asset_id 형식 검증 없음 | ✅ asset_id reference 만 직렬화 | ❌ `/api/assets/*` 미 ship | **부분 ship** — placeholder 메타데이터만 |
| **Document** | ⚠ `Item::Document { asset_id: String (required), mime, file_name, size_bytes }` (`schema.rs:261`) | ⚠ asset_id 검증 없음 + **ADR drift** | ✅ asset_id reference 만 | ❌ `/api/assets/*` 미 ship | **부분 ship + drift** |

### 1.2 ADR-0018 D10 drift 의 정확한 내용

ADR-0018 line 97-98:
```
| `image`    | `asset_id: string` (sha256 hash), `mime: string`, optional `original_w/h` |
| `document` | **D10 amend (2026-05-16)**: 두 mode 지원.
              (a) asset-based — `asset_id: string`, `mime: string`,
                  `file_name: string`, `size_bytes: number`.
              (b) inline-stored — `content: string` (UTF-8 markdown, cap 64 KB),
                  `file_name: string`. 두 mode 는 *상호 배타*: `asset_id` 가
                  있으면 (a), 없으면 (b). asset_id 는 *optional* 로 amend.
```

ADR 가 "asset_id optional + content 필드 신규" 를 약속했으나 `schema.rs:261` 의 `asset_id: String` 은 여전히 required, `content` 필드 없음. **schema ↔ ADR 정합 깨짐**.

FE 의 `lib/types/canvas.ts:132~138` 의 `DocumentItem` 인터페이스도 동일 (a)-only — *FE 와 ADR 도 같은 drift*. ADR 만 amend, 코드 (BE+FE) 미반영.

### 1.3 ADR-0018 의 P2+ 명시 deferred 항목

ADR-0018 line 102 가 명시:
> 비고: `image`/`document` 의 asset storage 정책은 ADR-0018 후속 또는 별 ADR (P2+).

**의도된 deferred** — 별 ADR 의 영역. 본 work package 의 §3 (단계 B) 는 그 roadmap 만 정리, 즉시 구현 안 함.

---

## 2. Stage 1 — Document inline-stored mode schema 정합 (🟡 P1, BE-only)

ADR-0018 D10 amend 정합. asset endpoint 없이도 *시안의 inline-editable document* 가 즉시 작동.

### 2.1 Schema 변경 (`crates/http-api/src/schema.rs`)

```rust
Document {
    #[serde(flatten)]
    common: ItemCommon,
    /// (a) asset-based mode: sha256 → `/api/assets/<asset_id>` (P2+).
    /// (b) inline-stored mode: `None`. 두 mode 상호 배타.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asset_id: Option<String>,
    mime: String,
    file_name: String,
    /// (a) asset-based: 실제 binary 크기. (b) inline-stored: `content.len()` bytes.
    size_bytes: u64,
    /// (b) inline-stored mode 의 UTF-8 markdown (cap = [`DOCUMENT_INLINE_MAX_BYTES`]).
    /// (a) asset-based 일 때는 `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}
```

### 2.2 Validation 신규 (3개 ValidationError variant)

```rust
const DOCUMENT_INLINE_MAX_BYTES: usize = 64 * 1024; // ADR-0018 D10

pub enum ValidationError {
    // ... 기존 variants 유지 ...
    /// (a) asset_id 도 (b) content 도 둘 다 None — Document mode 가 결정 안 됨.
    DocumentMissingSource,
    /// asset_id 와 content 가 *둘 다* Some — 두 mode 가 충돌. ADR-0018 D10 의
    /// "상호 배타" 위반.
    DocumentBothSources,
    /// inline-stored content 가 cap 초과.
    DocumentInlineTooLong,
}
```

`validate()` 의 `match it { ... Item::Document { asset_id, content, .. } => { ... } }` 분기:

```rust
Item::Document { asset_id, content, .. } => {
    match (asset_id.as_ref(), content.as_ref()) {
        (None, None) => return Err(ValidationError::DocumentMissingSource),
        (Some(_), Some(_)) => return Err(ValidationError::DocumentBothSources),
        (None, Some(c)) => {
            if c.len() > DOCUMENT_INLINE_MAX_BYTES {
                return Err(ValidationError::DocumentInlineTooLong);
            }
        }
        (Some(_), None) => {
            // asset-based mode. asset_id 형식 검증은 P2+ — `/api/assets/*`
            // ship 시점에 sha256 hex 64 char regex 추가.
        }
    }
}
```

### 2.3 Backward compatibility

- 기존 layout 의 `Item::Document { asset_id: "...", ... }` 는 그대로 (a) mode 로 유지. `content` 는 직렬화 안 됨 (`skip_serializing_if`).
- 기존 layout JSON 의 `"asset_id": "<string>"` 는 새 schema 의 `Option<String>` 에 `Some(...)` 로 deserialize.
- 새로 도입되는 inline-stored layout 의 `"content": "<markdown>"` 도 backward-compatible — 이전 BE 가 못 읽음 (downgrade 위험), 하지만 본 sprint 안 모두 land 후 deploy 라 문제 없음.

### 2.4 FE 측 wire 정합 (참고)

FE 의 `lib/types/canvas.ts:132~138` 의 `DocumentItem` 도 amend 필요:

```typescript
export interface DocumentItem extends ItemCommon {
  type: 'document';
  /** (a) asset-based: sha256, /api/assets/<sha256>. (b) inline-stored: undefined. */
  asset_id?: string;
  mime: string;
  file_name: string;
  size_bytes: number;
  /** (b) inline-stored: UTF-8 markdown, cap 65536 bytes. */
  content?: string;
}
```

**BE 측은 본 work package 가 cover. FE 측 wire 변경은 별 작업 (FE agent 책임)**.

### 2.5 Tests (5개, `schema.rs` `#[cfg(test)]` 안)

| Gate | 시나리오 | 기대 |
|---|---|---|
| 1 | inline-stored (asset_id=None, content=Some("# Hi")) | validate Ok |
| 2 | asset-based (asset_id=Some("abc..."), content=None) | validate Ok |
| 3 | both None | `Err(DocumentMissingSource)` |
| 4 | both Some | `Err(DocumentBothSources)` |
| 5 | content len > 64 KB | `Err(DocumentInlineTooLong)` |

### 2.6 영향 / risk

- **Risk = 0** — `Item::Document { common, .. }` 의 두 caller (schema.rs:289 `common()` getter + sessions.rs:1289 `id` getter) 모두 `..` rest pattern → 새 field 안전.
- **Schema migration 불필요** — `Option<String>` 의 default + skip_serializing_if 가 old 데이터 자연 호환.
- **API 변경 없음** — layout PUT/GET/import/export 모두 자동 처리.

### 2.7 검증 plan

```bash
cargo test -p gtmux-http-api --no-fail-fast schema::tests 2>&1 | tail -10
# 기대: 모든 기존 schema test PASS + 5 신규 document_* test PASS

cargo test --workspace --no-fail-fast --color=never 2>&1 | grep "test result:" | head -10
# 기대: workspace 376 → 381 PASS / 0 FAIL (+5 신규)

cargo build --release --bin gtmux --color=never
# 기대: PASS
```

---

## 3. Stage 2 — `/api/assets/*` binary endpoint (🟢 P2+, 별 ADR 필요)

ADR-0018 line 102 의 deferred 영역. 본 work package 는 **roadmap 만** 정리, 즉시 구현 안 함. 별 sprint 의 큰 ADR (예: ADR-0033 "Asset storage and serving") 으로 follow-up.

### 3.1 필요 endpoint

| Method | Path | 동작 |
|---|---|---|
| `POST` | `/api/assets` | multipart/form-data binary upload → sha256 계산 → workspace `.assets/<sha256>` 저장 → `{ asset_id, mime, size_bytes }` |
| `GET` | `/api/assets/{sha256}` | binary stream + 추론 Content-Type, cookie auth, ETag = asset_id |
| `DELETE` | `/api/assets/{sha256}` | orphan asset 명시 삭제 (P3, 보통은 GC 가 처리) |

### 3.2 결정 필요 항목 (큰 ADR 의 영역)

- **Storage location**: `<workspace>/.assets/<sha256>` (workspace 안) vs `<XDG_DATA_HOME>/gtmux/assets/<sha256>` (workspace 분리) — workspace export/import 시 portability 영향.
- **MIME allowlist**: image (`image/png`, `image/jpeg`, `image/webp`, `image/gif`?), document (`application/pdf`, `text/plain`, `text/markdown`?). 외 모두 415 Unsupported Media Type.
- **Size cap**: image 50 MiB / document 20 MiB? 또는 unified 32 MiB?
- **Orphan GC**: layout import/delete 후 reference 안 되는 asset 의 cleanup 정책. lazy (boot 시) vs eager (DELETE item 시) vs cron-style 주기적.
- **Content-Type 추론**: 업로드 시 client 가 보낸 MIME 신뢰 vs 서버가 magic-byte sniff?
- **Sha256 collision**: 1/2^128 — 무시 가능. SHA256 그대로 사용.
- **공유 정책**: 한 sha256 의 asset 이 여러 layout 에 reference 될 수 있는지 (ADR-0021 D1 의 mirror pattern 과 유사). default = 공유 허용 + orphan GC 가 reference count 계산.

### 3.3 FE 측 wire 의 미래

```typescript
// FE 의 image/document upload flow (Stage 2 ship 후 land 예정):
const form = new FormData();
form.append('file', file);
const res = await fetch('/api/assets', { method: 'POST', body: form, credentials: 'include' });
const { asset_id, mime, size_bytes } = await res.json();

// Then add ImageItem / DocumentItem to layout with asset_id.
```

### 3.4 Stage 1 / Stage 2 분리 이유

- Stage 1 (Document inline-stored) 은 ADR-0018 D10 의 *이미 결정된* drift 해소. 작은 surface (~50 line). FE 시안의 즉시 사용.
- Stage 2 (asset endpoint) 는 *새 결정* 영역 — storage / GC / mime / cap 의 큰 design 작업. 1-2일 +. 별 sprint 가 적절.

Image (asset-based only) 는 Stage 1 후도 *placeholder* 상태 — 사용자 UX 완성도 측면에선 Stage 2 까지 land 후에 완비.

---

## 4. FE 측 권장 후속

본 work package 의 Stage 1 BE land 후 FE 측 작업:

1. **`lib/types/canvas.ts` `DocumentItem` 인터페이스 amend** — `asset_id?: string`, `content?: string` 추가.
2. **DocumentNode (또는 그 equivalent) 의 dual mode rendering**:
   - `content` 가 set → markdown render (read-only or editable per design).
   - `asset_id` 가 set → "Open document" 버튼 (현재는 placeholder, Stage 2 ship 후 download).
3. **DocumentItem 생성 flow**: 시안 (`ref/frontend-design/components.html §02`) 의 inline-edit modal — content state 가 sessionStore 의 layout 에 직접 mutation (asset upload 단계 없이).
4. **Image item**: Stage 2 ship 전까지는 *명시 disabled* 또는 *placeholder* — UX consistency.

---

## 5. 정본 reference

| 종류 | 경로 |
|---|---|
| 본 work package 의 정본 ADR | `docs/adr/0018-canvas-item-data-model.md` D10 amend ① |
| Stage 1 의 코드 시작점 | `codebase/backend/crates/http-api/src/schema.rs:261` (Item::Document variant) |
| Stage 1 의 validate 시작점 | `codebase/backend/crates/http-api/src/schema.rs:404` (validate fn 의 match arm) |
| FE 인터페이스 정의 | `codebase/frontend/src/lib/types/canvas.ts:132~138` |
| FE 시안 | `ref/frontend-design/components.html §02` (inline-editable document) |
| Stage 2 의 정본 ADR (Draft) | `docs/adr/0033-asset-storage-and-serving.md` (2026-05-17 Draft) |
| Stage 2 의 BE work-package | `docs/reports/0059-be-asset-storage-work-package.md` (2026-05-17) |

---

## 6. 완료 기준

### Stage 1
- `Item::Document` 의 `asset_id: Option<String>` + `content: Option<String>` schema 정합.
- `DOCUMENT_INLINE_MAX_BYTES` 상수 정의.
- 3 신규 ValidationError variants + `code()` mapping.
- 5 신규 validation tests PASS (inline-valid / asset-valid / both-none / both-some / inline-too-long).
- workspace 376 → 381 PASS / 0 FAIL.
- ADR-0018 D10 의 schema 정합 amend ② 표기 — drift closed.
- 본 doc §2 의 모든 항목 ✅.

### Stage 2 (별 sprint)
- ADR-0033 "Asset storage and serving" Accepted.
- `POST /api/assets` / `GET /api/assets/{sha256}` ship.
- Orphan GC policy 명시.
- FE 측 image/document upload UX wire.

---

## 7. 변경 이력

- 2026-05-17: 초안. 사용자 grilling 의 답변으로 toolbar tool BE 측 ship 상태 정리 + Document inline mode 의 schema drift 해소 work package + Asset endpoint roadmap.
- 2026-05-17: **amend ① — Stage 1 SHIPPED**. 본 doc §2 의 모든 항목 land:
  - `schema.rs::Item::Document` 의 `asset_id: Option<String>` + `content: Option<String>` 신규.
  - `DOCUMENT_INLINE_MAX_BYTES: usize = 64 * 1024` 상수 (line ~46).
  - `ValidationError` 3 신규 variant — `DocumentMissingSource` / `DocumentBothSources` / `DocumentInlineTooLong` + `code()` mapping (`document_missing_source` / `document_both_sources` / `document_inline_too_long`).
  - `validate()` 의 Document match arm: (None,None) → DocumentMissingSource / (Some,Some) → DocumentBothSources / (None, Some(c) where c.len() > cap) → DocumentInlineTooLong / (None, Some(small)) → Ok / (Some, None) → Ok (asset_id 의 sha256 regex 는 Stage 2 ship 시 추가).
  - 5 신규 tests (`schema::tests::document_*`) — inline valid / asset valid / both none / both some / inline too long.
  - ADR-0018 D10 amend ② entry + line 98 의 "(b) inline-stored 도 BE schema 에 land" 표기 갱신.
  - 검증: workspace 376 → **381 PASS / 0 FAIL** (+5 신규 document validation tests). release build PASS.
  - 검증 HEAD (예정): 본 doc commit 직후.

  **FE 측 권장 후속** (§4 의 1번 항목 그대로): `lib/types/canvas.ts:132~138` 의 `DocumentItem` 인터페이스 amend — `asset_id?: string` + `content?: string` 으로 두 mode 표현. 별 FE work 로 진행.

  **Stage 2 (`/api/assets/*` binary endpoint)** 는 본 amend 와 분리 — 별 ADR (ADR-0033 to-be) 의 영역.
