# 0052 — BE work package: `GET /api/sessions/{name}/export`

- 작성일: 2026-05-17
- 종류: backend work package (FE 측 plan 진입 시 추출된 BE 의존 사항)
- 발주: FE Inspector / Undo / Import-Export 통합 agent
- 우선순위: 🟡 P1 — Import/Export FE UX 의 download path 의존
- 관련 ADR: **ADR-0029** (Session import/export 파일 포맷 및 API 정책)
- 관련 구현: `codebase/backend/crates/http-api/src/sessions.rs::import_handler` (대칭 reference)

---

## 1. 한 줄 요약

ADR-0029 D2 의 envelope (`gtmux Session Export Envelope v1`) 를 byte stream
으로 반환하는 `GET /api/sessions/{name}/export` handler 신규 추가. FE 가
`Content-Disposition` 의 sanitized filename 으로 즉시 download 트리거.

---

## 2. 신규 endpoint 시그니처

```
GET /api/sessions/{name}/export
Cookie: gtmux_session=...                 (ADR-0020 cookie auth)
Origin/Host: must match (ADR-0020)
```

응답:

```
200 OK
Content-Type: application/json
Content-Disposition: attachment; filename="<safe-session-name>.gtmux-session.json"

{
  "kind": "gtmux.session.export",
  "export_version": 1,
  "exported_at": "2026-05-17T01:23:45Z",
  "session_name": "<name>",
  "layout": { "schema_version": 2, "groups": [...], "items": [...], "viewport": {...} },
  "metadata": { "app": "gtmux", "app_version": null }
}
```

실패:

| 코드 | 분기 | body |
|---|---|---|
| 401 | cookie 누락 / 무효 | `{ "error": "unauthorized" }` (ADR-0020 정합) |
| 404 | session 없음 | `{ "error": "not_found", "name": "<name>" }` |
| 503 | workspace 미설정 | `{ "error": "workspace_not_configured" }` |
| 500 | read / serialize 실패 | `{ "error": "save_failed" }` (import_handler 와 동일 톤) |

---

## 3. 구현 location

### 3.1 `codebase/backend/crates/http-api/src/lib.rs`

기존 `/api/sessions/import` 라우트 (`lib.rs:550`) 와 정합한 위치에 추가:

```rust
.route(
    "/api/sessions/:name/export",
    axum::routing::get(sessions::export_handler),
)
```

### 3.2 `codebase/backend/crates/http-api/src/sessions.rs`

`import_handler` (line 920) 와 대칭. envelope serializer 는 small private fn.

```rust
#[derive(Serialize)]
struct ExportEnvelope<'a> {
    kind: &'static str,                  // "gtmux.session.export"
    export_version: u32,                 // 1
    exported_at: String,                 // RFC3339 UTC
    session_name: &'a str,
    layout: &'a Layout,
    metadata: ExportMetadata,
}

#[derive(Serialize)]
struct ExportMetadata {
    app: &'static str,                   // "gtmux"
    app_version: Option<&'static str>,   // None (P1+: env!("CARGO_PKG_VERSION"))
}

/// `GET /api/sessions/{name}/export` — ADR-0029 D4.
///
/// Outcomes:
/// - 200 OK + envelope JSON + Content-Disposition attachment.
/// - 401 unauthorized — cookie missing/invalid.
/// - 404 not_found — session record absent in workspace.
/// - 503 workspace_not_configured — server started without a workspace.
/// - 500 save_failed — read/serialize error.
///
/// Reads the *persisted* layout (SessionCache 의 commit 된 snapshot).
/// FE 측은 export 직전 pending mutation 을 flush 또는 last-saved 명시
/// 책임 (ADR-0029 D13).
pub async fn export_handler(
    State(state): State<crate::AppState>,
    Path(name): Path<String>,
) -> Response {
    let Some(wm) = state.workspace.as_ref() else {
        return service_unavailable("workspace_not_configured");
    };
    if let Err(e) = validate_session_name(&name) {
        return SessionError::Workspace(e).into_response();
    }
    // SessionCache 또는 disk 에서 layout 로드 — import_handler 와 동일
    // get-or-load pattern (sessions.rs 의 기존 helper 재사용).
    let layout = match load_session_layout(&state, wm, &name).await {
        Ok(l) => l,
        Err(LayoutLoadError::NotFound) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "name": name })),
            )
                .into_response();
        }
        Err(LayoutLoadError::Io(e)) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "save_failed", "details": e.to_string() })),
            )
                .into_response();
        }
    };

    let exported_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let envelope = ExportEnvelope {
        kind: "gtmux.session.export",
        export_version: 1,
        exported_at,
        session_name: &name,
        layout: &layout,
        metadata: ExportMetadata { app: "gtmux", app_version: None },
    };

    let body = match serde_json::to_vec(&envelope) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "save_failed", "details": e.to_string() })),
            )
                .into_response();
        }
    };

    let filename = sanitize_filename(&name);
    let disposition = format!(
        r#"attachment; filename="{filename}.gtmux-session.json""#
    );
    (
        StatusCode::OK,
        [
            ("Content-Type", "application/json".to_string()),
            ("Content-Disposition", disposition),
        ],
        body,
    )
        .into_response()
}

/// ASCII-safe, path-safe basename. `validate_session_name` 의 `[A-Za-z0-9_-]{1,64}`
/// regex 와 정합 — 모든 valid session name 은 이미 safe 라 fallback 만.
fn sanitize_filename(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    if safe.is_empty() { "session".into() } else { safe }
}
```

### 3.3 `load_session_layout` helper

기존 코드에 적절한 헬퍼가 있다면 재사용. 없다면:

```rust
enum LayoutLoadError { NotFound, Io(std::io::Error) }

async fn load_session_layout(
    state: &crate::AppState,
    wm: &WorkspaceManager,
    name: &str,
) -> Result<Layout, LayoutLoadError> {
    // 1) SessionCache 우선 — attached session 의 commit 된 snapshot.
    {
        let read = state.session_cache.entries.read().await;
        if let Some(arc) = read.get(name) {
            let cached = arc.read().await;
            return Ok(cached.layout.clone());
        }
    }
    // 2) Disk fallback — detached / 미캐싱 session 도 export 가능.
    let path = wm.session_path(name).map_err(|_| LayoutLoadError::NotFound)?;
    if !path.exists() { return Err(LayoutLoadError::NotFound); }
    let bytes = std::fs::read(&path).map_err(LayoutLoadError::Io)?;
    let layout: Layout = serde_json::from_slice(&bytes)
        .map_err(|e| LayoutLoadError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    Ok(layout)
}
```

### 3.4 chrono 의존성

`Cargo.toml` 에 `chrono = "0.4"` 가 없다면 추가. workspace 의 다른 crate 가 이미 사용 중인지 먼저 확인. 없다면 RFC3339 출력만을 위해 *얇은* 추가가 정당하지 않다면 `std::time::SystemTime + format!` 로 ISO8601 수동 생성 도 가능.

---

## 4. 테스트

`codebase/backend/crates/http-api/src/lib.rs` 의 `#[cfg(test)]` 안 (line 1575 의 Slice D-4 import test pattern 정합):

```rust
// Gate 0029-1 — happy path (200 + envelope).
#[tokio::test]
async fn export_returns_envelope_for_existing_session() {
    let dir = TempDir::new().unwrap();
    let (app, token, _) = make_app_with_workspace(&dir);
    create_session(&app, &token, "alpha").await;

    let req = Request::builder()
        .method("GET")
        .uri("/api/sessions/alpha/export")
        .header(COOKIE, format!("gtmux_session={token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let dispo = res.headers().get("content-disposition").unwrap();
    assert!(dispo.to_str().unwrap().contains(r#"filename="alpha.gtmux-session.json""#));
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let env: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(env["kind"], "gtmux.session.export");
    assert_eq!(env["export_version"], 1);
    assert_eq!(env["session_name"], "alpha");
    assert_eq!(env["layout"]["schema_version"], 2);
}

// Gate 0029-2 — 404 for missing session.
#[tokio::test]
async fn export_404_for_missing_session() {
    let dir = TempDir::new().unwrap();
    let (app, token, _) = make_app_with_workspace(&dir);
    let req = Request::builder()
        .method("GET")
        .uri("/api/sessions/missing/export")
        .header(COOKIE, format!("gtmux_session={token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// Gate 0029-3 — 401 without cookie.
#[tokio::test]
async fn export_401_without_cookie() {
    let dir = TempDir::new().unwrap();
    let (app, _, _) = make_app_with_workspace(&dir);
    let req = Request::builder()
        .method("GET")
        .uri("/api/sessions/anything/export")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

// Gate 0029-4 — invalid name 400.
#[tokio::test]
async fn export_400_for_invalid_name() {
    let dir = TempDir::new().unwrap();
    let (app, token, _) = make_app_with_workspace(&dir);
    let req = Request::builder()
        .method("GET")
        .uri("/api/sessions/..%2Fetc%2Fpasswd/export")
        .header(COOKIE, format!("gtmux_session={token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

// Gate 0029-5 — round-trip (export → import → equal layout).
#[tokio::test]
async fn export_import_round_trip_equal_layout() {
    let dir = TempDir::new().unwrap();
    let (app, token, _) = make_app_with_workspace(&dir);
    create_session(&app, &token, "src").await;
    // ... mutate layout, then export, then import as "dst",
    //     then GET /api/sessions/dst/layout, assert equal items[].
}
```

---

## 5. 보안 검증

| 위험 | 대응 |
|---|---|
| Path traversal via `:name` | `validate_session_name` (ADR-0019) regex 가드. invalid → 400 |
| Cookie 미인증 export | axum middleware 의 cookie validation — 라우트 register 시 보호된 router scope 사용 |
| Cross-origin download | ADR-0020 D9 의 Origin/Host check 미적용 — GET 이라 CSRF 위험은 낮지만 cookie SameSite=Lax 가 cross-site 차단 |
| 민감 정보 export 노출 | ADR-0029 D10 — terminal output/process/asset 제외 (envelope 의 `layout` 만 포함, schema v2 의 inert metadata) |
| filename injection | `sanitize_filename` 이 `[A-Za-z0-9_-]` 외 모두 `_` 치환. `validate_session_name` regex 와 이중 가드 |

---

## 6. FE 측 wire (ship 후)

FE 가 `GET /api/sessions/{name}/export` 응답 받으면:
1. `Content-Disposition` 의 filename 추출 (FE fallback: `<name>.gtmux-session.json`)
2. response body 를 Blob 으로 변환 → `URL.createObjectURL(blob)` → 임시 `<a download>` 클릭
3. URL revoke

ImportSessionModal 의 file picker 가 동일 envelope 을 parse 해 `POST /api/sessions/import { name, layout }` 으로 다시 보낸다. round-trip 정합.

---

## 7. 완료 기준

- `GET /api/sessions/{name}/export` 라우트 등록
- `export_handler` + `sanitize_filename` + `load_session_layout` (또는 기존 helper) 구현
- Gate 0029-1 ~ 0029-5 테스트 통과
- `cargo test --workspace` clean
- `smoke/02_stage5.sh` 등 기존 smoke 통과 (regression 없음)
- ADR-0029 의 "Backend" 섹션 의 "추가" 목록 ✅

---

## 8. FE 측 ship 의 BE 의존성 명시

FE 의 ExportSessionModal 은 본 endpoint 없이는 download 불가. ship 전까지
FE 는 button 을 *disabled + tooltip "Export endpoint not yet available"*
으로 표시. BE ship 후 PR description 에 본 0052 reference 명시.

Co-Authored-By 무관 — BE agent 가 본 work package 의 4~7 절을 implementation 진실로 사용.

---

## 9. 변경 이력

- 2026-05-17: 초안 — FE ExportSessionModal land 후 BE 의존성 추출.
- 2026-05-17: **BE ship (amend ①)**. 본 work package 의 §3 / §4 / §5 / §6 / §7 모두 정합 land:
  - **§3.1 route**: `lib.rs` 의 import route 옆에 `.route("/api/sessions/{name}/export", get(sessions::export_handler))` 추가 (axum 0.7 path syntax `{name}` 사용).
  - **§3.2 export_handler**: `sessions.rs` 의 `import_handler` 직후 위치. `ExportEnvelope` / `ExportMetadata` Serialize types + `EXPORT_ENVELOPE_KIND="gtmux.session.export"` / `EXPORT_ENVELOPE_VERSION=1` 상수. `validate_session_name` → `session_cache.get_or_load` → envelope serialize → `Content-Disposition` insert 흐름.
  - **§3.3 helper**: 별 `load_session_layout` fn 안 도입 — 기존 `state.session_cache.get_or_load(wm.as_ref(), &name)` 재사용. `SessionError::NotFound` 분기만 hand-crafted body (`{ "error":"not_found", "name":<name> }`) 로 변환. 그 외는 `IntoResponse` 위임.
  - **§3.4 chrono**: 의존성 미도입. `rfc3339_utc_now` + `civil_from_unix` (Howard Hinnant 알고리즘) std-only helper 도입 — `sessions.rs` 안 ~25 라인.
  - **§4 tests**: Gate 0029-1 ~ 0029-5 (`lib.rs:1762~`) 5/5 PASS. 각 test 는 0052 §4 의 pseudo-code 골격 그대로 + 추가 verifications (RFC3339 length / `T`/`Z` chars / envelope shape).
  - **§5 보안**: validate_session_name → 400 (`invalid_session_name`), bearer middleware → 401, sanitize_export_filename 이중 가드 모두 적용. ADR-0029 D10 의 "inert metadata only" 정합 (layout 만 export, terminal output / asset 제외).
  - **§7 완료 기준**: 모두 ✅. workspace 368 → **373 PASS** / 0 FAIL. release build PASS.
  
  HEAD (ship 시점, 본 commit 직전): `2c104c5` docs(verification): 0053 BE checklist verification.
