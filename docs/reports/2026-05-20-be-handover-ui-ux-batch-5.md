# 2026-05-20 — BE Handover: UI/UX Batch 5 — Schema amend

> ⚠️ **2026-05-20 Grill #1-19 amend 적용 후** — 본 doc 의 *§A Task* / *§B Anchor* / *§D AC* 가 *grill 결과* 반영. 주요 변경:
> - **BE-E (Settings text_label_max_chars) 완전 폐기** — Settings 신설 안 함. 기존 4 KB label cap 자연 활용.
> - **`corner_radius: u32` → `corner_rounded: bool`** — 수치 input 폐기, 자동 radius (FE 가 `clamp(min(w,h)*0.15, 4, 16)` 계산).
> - **`RectCornerRadiusExceedsBox` ValidationError 폐기**.
> - **`StrokeWidthOutOfRange` + `TextFontSizeOutOfRange` ValidationError 신규**.
> - **FontWeight** = Light/Normal/Bold 3 variant.
> - **FontStyle enum 폐기** — `italic` 도 `bool + #[serde(default)]`. underline/strikethrough 도 동일.
>
> 자세한 결정 trace 는 `2026-05-20-ui-ux-batch-5-analysis.md` 의 변경 이력 절.

- 작성일: 2026-05-20
- 작성 주체: agent (system-architect role, FE/BE 짝)
- 정본 cross-link:
  - **상위 분석**: [`2026-05-20-ui-ux-batch-5-analysis.md`](./2026-05-20-ui-ux-batch-5-analysis.md) (요구·위험·결정 의 정본)
  - **FE 짝**: [`2026-05-20-fe-handover-ui-ux-batch-5.md`](./2026-05-20-fe-handover-ui-ux-batch-5.md)
  - **결정 출처 ADR**: ADR-0018 D4 / D8 (`docs/adr/0018-canvas-item-data-model.md`), ADR-0017 §Settings (`docs/adr/0017-layout-grid-and-chrome.md`), ADR-0030 D3 (clipboard) — 본 batch 가 amend
  - **schema 코드**: `codebase/backend/crates/http-api/src/schema.rs`
  - **settings 코드**: `codebase/backend/crates/http-api/src/settings.rs`
  - **layout PUT 핸들러**: `codebase/backend/crates/http-api/src/sessions.rs::put_layout_handler`
  - 직전 BE handover: `docs/reports/2026-05-20-session-handover-0080-asset-upload-and-phase1-recap.md`

## 핵심 원칙 — 거짓 ship 방지

- **Anchor 명시**: 변경 위치는 *file:line* 단위.
- **AC 검증 가능**: `cargo test` 또는 `curl` + JSON 비교로 정의.
- **Backward-compat additive only**: 모든 신규 field 는 `Option<T> + #[serde(default)]` 또는 `bool + #[serde(default)]`. v2 schema 의 *옛 record* 가 그대로 deserialize 되어야.
- **release binary mtime cross-check**: 모든 commit 후 `cargo build --release` (0077 §5.1 교훈).
- **owner_key 명명 강제** (ADR-0019 D5.6 amend ②): `*_for_cookie` / `cookie_value` 신규 작성 금지.
- **disk-of-truth ordering** (ADR-0006 D13): broadcast / index 갱신은 disk write 성공 후.

---

## 0. Self-grilling 결정

### Q1. Rect 의 corner_radius 가 `min(w, h) / 2` 초과 시 어떻게?

**결정**: ✅ **strict reject** (`RectCornerRadiusExceedsBox` 신규 ValidationError). 이유:
- FE Inspector 가 cap slider 로 보장하지만 *직접 PUT* (FE 의 race / 외부 tool) 가 가능.
- BE 의 *시각 정합 의무* — corner_radius 가 box 초과면 ellipse 처럼 보이거나 SVG 렌더 깨짐.
- backward-compat 영향 0 — 옛 record 는 `corner_radius` field 없음 → default 0 → 자연 통과.

**거절**: 자동 clamp (silently `min(w, h) / 2` 로 보정) — *silent state drift*, ADR-0006 의 disk-of-truth 위반.

### Q2. FigureStrokeDash enum 의 connector 와 통합?

**결정**: ✅ **별 enum 신규**. 이유:
- connector 의 `StrokeDash: Dash | Dot` (2 variant) + `Option<>` 으로 solid 표현.
- figure 는 `FigureStrokeDash: Solid | Dash | Dot | DashDot` (4 variant, default Solid).
- 의미 / default / wire 모두 다름 — 통합 시 enum naming clash + 옛 connector 의 직렬화 의미 변경 → drift.

**거절**: 단일 enum 으로 `StrokeDash: Solid | Dash | Dot | DashDot` 만들고 connector 도 reuse — connector wire 의 backward-compat 깨짐 (현재 `null` 이 solid).

### Q3. Text 의 decoration — enum 또는 4 boolean?

**결정**: ✅ **4 boolean** (`bold` / `italic` / `underline` / `strikethrough` — Option<bool>). 이유:
- 사용자가 underline + strikethrough 동시 가능해야 (CSS `text-decoration: underline line-through`).
- enum 으로 표현 시 array 또는 bitflag — wire 부피.
- 4 boolean = 4 byte wire. 단순.

**거절**: register 의 `text_decoration: "none" | "underline" | "line-through"` enum — 동시 표현 불가.

### Q4. `fill_enabled` / `stroke_enabled` default 정책?

**결정**: ✅ **둘 다 default true** (옛 record backward-compat). 옛 layout 의 `fill: "transparent"` 는 *알파 0* 의 의미로 그대로 보존 — fill_enabled 의 의미 변경 X.

### Q5. text_label_max_chars 의 cap?

**결정**: ✅ **u32, 1 ≤ value ≤ 128**. default 24. 이유:
- 0 = label derive 무 → 의미 모호. 최소 1.
- 128 = label cap (`LABEL_DESCRIPTION_MAX_BYTES = 4096` 의 32배 미만, UI 자연 노출 한도).
- Settings PUT 시 validate.

### Q6. terminal paste payload filter 는 BE 가 강제하는가?

**결정**: ❌ **FE-only 영역**. BE 는 ADR-0030 의 *FE 책임* 표현 그대로 — clipboard 는 FE in-memory (D1). BE 는 layout PUT 의 *결과* 만 처리. terminal item 의 label / description 등이 fresh default 인지는 FE 가 책임. BE 의 schema validation 은 *그 field 들의 cap* 만 확인.

---

## §A. Task 목록 (Grill #1-19 amend)

| Task | 영역 | 출처 | 예상 소요 |
|---|---|---|---|
| **BE-A** | `schema.rs::Item::Rect` 의 `fill_enabled: bool` (default true) + `stroke_enabled: bool` (default true) + **`corner_rounded: bool`** (default false, *수치 X*) + `stroke_dash: Option<FigureStrokeDash>` field 신규 + ADR-0018 D4 amend ① | R1 + R2 | 1 commit |
| **BE-B** | `schema.rs::Item::Ellipse` 의 `fill_enabled` / `stroke_enabled` / `stroke_dash` + `Item::Line` 의 `stroke_dash` | R1 + R2 | BE-A 와 같은 commit (묶음) |
| **BE-C** | `schema.rs::Item::Text` 의 `font_weight: FontWeight` (3-variant **Light/Normal/Bold**, default Normal, `#[serde(default)]`) / **`italic: bool`** / **`underline: bool`** / **`strikethrough: bool`** (모두 `#[serde(default)]`). FontStyle enum *불생성*. + ADR-0018 D4 amend ② | R3 | 1 commit |
| **BE-D** | `schema.rs::validate()` 의 **`StrokeWidthOutOfRange`** + **`TextFontSizeOutOfRange`** ValidationError variant 신규 (각 1≤≤32 / 8≤≤96). `RectCornerRadiusExceedsBox` *불생성*. + 5 신규 unit test | R1 + R2 + R3 | BE-A/B/C 와 같은 PR 의 별 commit |
| ~~**BE-E**~~ | ~~`settings.rs::Settings::text_label_max_chars`~~ | ~~R7~~ | **폐기 (Grill #18)** — Settings 신설 안 함. 기존 label cap 자연 활용 |
| **BE-F** | `bin/gen-openapi` 재실행 (또는 동등) → `shared/openapi.yaml` + FE `lib/types/api.d.ts` 재발행 | A-D 의 후속 | 1 commit (typings only) |

총 **3 commit** (BE-A/B/D 묶음 + BE-C + BE-F).

---

## §B. Anchor 변경 매트릭스

### B1. `schema.rs` 의 `Item::Rect` / `Item::Ellipse` / `Item::Line`

위치: `codebase/backend/crates/http-api/src/schema.rs:297-318`

```rust
// 변경 전 (현)
Rect {
    #[serde(flatten)] common: ItemCommon,
    stroke: String,
    fill: String,
    stroke_width: u32,
},

// 변경 후 (본 batch)
Rect {
    #[serde(flatten)] common: ItemCommon,
    stroke: String,
    fill: String,
    stroke_width: u32,
    /// ADR-0018 D4 amend ① (2026-05-20 batch 5) — fill on/off (≠ alpha).
    /// `false` 면 hit-test 도 제외 (FE-side ShapeNode 의 pointer-events 분기).
    #[serde(default = "default_true")]
    fill_enabled: bool,
    /// ADR-0018 D4 amend ① — stroke on/off. `false` 면 border 렌더 + hit-test 모두 제거.
    #[serde(default = "default_true")]
    stroke_enabled: bool,
    /// ADR-0018 D4 amend ① — corner radius (rect only). 0 = 직각.
    /// Validation: ≤ min(w, h) / 2 (ValidationError::RectCornerRadiusExceedsBox).
    #[serde(default)]
    corner_radius: u32,
    /// ADR-0018 D4 amend ① — stroke dash pattern. `None` = solid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stroke_dash: Option<FigureStrokeDash>,
},

Ellipse {
    #[serde(flatten)] common: ItemCommon,
    stroke: String,
    fill: String,
    stroke_width: u32,
    #[serde(default = "default_true")]
    fill_enabled: bool,
    #[serde(default = "default_true")]
    stroke_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stroke_dash: Option<FigureStrokeDash>,
},

Line {
    #[serde(flatten)] common: ItemCommon,
    stroke: String,
    stroke_width: u32,
    x2: f64,
    y2: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stroke_dash: Option<FigureStrokeDash>,
},
```

신규 helper `fn default_true() -> bool { true }` 추가 (module-private). 신규 enum:

```rust
/// ADR-0018 D4 amend ① (2026-05-20) — figure stroke dash pattern. connector 의
/// [`StrokeDash`] 와 의미 / default 가 달라 별 enum.
///
/// Wire: snake_case ("solid", "dash", "dot", "dashdot").
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FigureStrokeDash {
    Solid,
    Dash,
    Dot,
    DashDot,
}

impl Default for FigureStrokeDash {
    fn default() -> Self { Self::Solid }
}
```

### B2. `schema.rs::Item::Text`

위치: `schema.rs:279-289`

```rust
Text {
    #[serde(flatten)] common: ItemCommon,
    text: String,
    font_size: u32,
    #[serde(default)] text_align: TextAlign,
    #[serde(default)] text_vertical_align: TextVerticalAlign,
    color: String,
    /// ADR-0018 D4 amend ② (2026-05-20) — text font weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    font_weight: Option<FontWeight>,
    /// ADR-0018 D4 amend ② — italic 여부.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    font_style: Option<FontStyle>,
    /// ADR-0018 D4 amend ② — underline. CSS text-decoration 의 underline 단독.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    underline: Option<bool>,
    /// ADR-0018 D4 amend ② — strikethrough. CSS text-decoration 의 line-through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    strikethrough: Option<bool>,
},
```

신규 enums:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FontWeight {
    Normal,
    Bold,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FontStyle {
    Normal,
    Italic,
}
```

### B3. `schema.rs::ValidationError` 의 신규 variant

위치: `schema.rs:438-509`

```rust
// enum 신규 variant 추가
/// ADR-0018 D4 amend ① — Rect.corner_radius > min(w, h) / 2.
#[error("rect corner_radius {radius} exceeds min(w, h) / 2 = {cap}")]
RectCornerRadiusExceedsBox { radius: u32, cap: u32 },

// code() match arm 에 추가
Self::RectCornerRadiusExceedsBox { .. } => "rect_corner_radius_exceeds_box",
```

`validate()` 의 items loop 안 Rect 분기에 검증 코드 추가:

```rust
Item::Rect { common, corner_radius, .. } => {
    let cap = (common.w.min(common.h) / 2.0).floor() as u32;
    if *corner_radius > cap {
        return Err(ValidationError::RectCornerRadiusExceedsBox {
            radius: *corner_radius,
            cap,
        });
    }
}
```

(주의: `common.w` 가 `f64` 라 cap 도 f64 → u32 truncation 정합. `common.w / 2.0` 이 음수일 일 없음 — 음수 width 는 다른 invariant.)

### B4. `settings.rs` 의 `Settings::text_label_max_chars`

위치: `codebase/backend/crates/http-api/src/settings.rs` (현 Settings struct definition)

```rust
pub struct Settings {
    // ... 기존 field ...
    /// ADR-0017 §Settings amend (2026-05-20 batch 5) — text item 의 label
    /// auto-derive 시 *앞 N자* cap. 1 ≤ N ≤ 128. default 24.
    #[serde(default = "default_text_label_max_chars")]
    pub text_label_max_chars: u32,
}

fn default_text_label_max_chars() -> u32 { 24 }
```

PUT handler 의 validation 분기:

```rust
if !(1..=128).contains(&settings.text_label_max_chars) {
    return Err(SettingsError::InvalidField {
        field: "text_label_max_chars",
        reason: format!("must be 1..=128, got {}", settings.text_label_max_chars),
    });
}
```

(또는 BE 의 `SettingsError` enum 의 기존 variant 활용 — 현 codebase 의 패턴 따름.)

### B5. `bin/gen-openapi` 재실행 후 commit

```bash
cargo run --bin gen-openapi  # 또는 동등 명령
git add shared/openapi.yaml codebase/frontend/src/lib/types/api.d.ts
```

→ 본 commit 은 BE-A/B/C/D/E 모두 land 후 별 commit.

---

## §C. API contract

본 batch 의 API 변경은 *모두 schema 확장* — endpoint 추가 없음.

### C1. `GET /api/sessions/:name/layout` — 응답 body 의 신규 field

```json
{
  "schema_version": 2,
  "groups": [...],
  "items": [
    {
      "id": "...",
      "type": "rect",
      "parent_id": null,
      "x": 100, "y": 100, "w": 200, "h": 140, "z": 0,
      "visibility": "visible", "locked": false,
      "label": "", "description": "", "minimized": false,
      "stroke": "#0d99ff",
      "fill": "transparent",
      "stroke_width": 2,
      "fill_enabled": true,
      "stroke_enabled": true,
      "corner_radius": 8,
      "stroke_dash": "dash"
    },
    {
      "id": "...",
      "type": "text",
      "x": 100, "y": 100, "w": 160, "h": 56, "z": 1,
      "visibility": "visible", "locked": false,
      "label": "Hello world", "description": "", "minimized": false,
      "text": "Hello world",
      "font_size": 16,
      "text_align": "center",
      "text_vertical_align": "middle",
      "color": "#333",
      "font_weight": "bold",
      "font_style": "italic",
      "underline": true,
      "strikethrough": false
    }
  ],
  "viewport": {...}
}
```

새 field 의 부재는 default 로 deserialize — 옛 layout backward-compat.

### C2. `PUT /api/sessions/:name/layout` — 요청 body 의 신규 field

- 같은 field set 받음. 누락 시 default 적용.
- `If-Match` ETag 정합 (변경 없음).
- 신규 ValidationError:
  - **400** `{"code": "rect_corner_radius_exceeds_box", "details": {...}}`
- 기존 ValidationError 코드 유지.

### C3. `GET /api/settings` — 응답 body 의 신규 키

```json
{
  ...,
  "text_label_max_chars": 24
}
```

### C4. `PUT /api/settings` — 요청 body 의 신규 키

- `text_label_max_chars` 옵셔널 (default 24 적용).
- validation: `1 ≤ value ≤ 128`. 범위 밖 → 400.

---

## §D. Acceptance criteria

### AC-BE-A1. Schema round-trip (rect)

```bash
cd codebase/backend
cargo test --test schema -- rect_fill_stroke_enabled_round_trip
# 기대: PASS
```

신규 test (in `schema.rs::tests`):

```rust
#[test]
fn rect_fill_stroke_enabled_round_trip() {
    let json = r#"{
        "id": "..uuid..", "type": "rect", "parent_id": null,
        "x": 0, "y": 0, "w": 100, "h": 100, "z": 0,
        "visibility": "visible", "locked": false, "minimized": false,
        "stroke": "#000", "fill": "#fff", "stroke_width": 2,
        "fill_enabled": false, "stroke_enabled": true,
        "corner_radius": 4, "stroke_dash": "dash_dot"
    }"#;
    let item: Item = serde_json::from_str(json).unwrap();
    let Item::Rect { fill_enabled, stroke_enabled, corner_radius, stroke_dash, .. } = item else { panic!() };
    assert!(!fill_enabled);
    assert!(stroke_enabled);
    assert_eq!(corner_radius, 4);
    assert_eq!(stroke_dash, Some(FigureStrokeDash::DashDot));
    // round-trip back
    let serialized = serde_json::to_string(&item).unwrap();
    let item2: Item = serde_json::from_str(&serialized).unwrap();
    assert_eq!(item, item2);
}
```

### AC-BE-A2. Backward-compat (옛 layout)

```rust
#[test]
fn rect_old_layout_defaults_fill_stroke_enabled_true() {
    let json = r#"{
        "id": "..uuid..", "type": "rect", "parent_id": null,
        "x": 0, "y": 0, "w": 100, "h": 100, "z": 0,
        "visibility": "visible", "locked": false, "minimized": false,
        "stroke": "#000", "fill": "#fff", "stroke_width": 2
    }"#;
    let item: Item = serde_json::from_str(json).unwrap();
    let Item::Rect { fill_enabled, stroke_enabled, corner_radius, stroke_dash, .. } = item else { panic!() };
    assert!(fill_enabled);
    assert!(stroke_enabled);
    assert_eq!(corner_radius, 0);
    assert_eq!(stroke_dash, None);
}
```

### AC-BE-A3. corner_radius validation

```rust
#[test]
fn rect_corner_radius_exceeding_box_rejected() {
    let mut layout = Layout::empty();
    layout.items.push(Item::Rect {
        common: make_common("rect-1", 0.0, 0.0, 100.0, 60.0),
        stroke: "#000".into(), fill: "#fff".into(),
        stroke_width: 2,
        fill_enabled: true, stroke_enabled: true,
        corner_radius: 40,  // cap = 30, exceeds
        stroke_dash: None,
    });
    let err = validate(&layout).unwrap_err();
    assert_eq!(err.code(), "rect_corner_radius_exceeds_box");
}

#[test]
fn rect_corner_radius_at_cap_accepted() {
    let mut layout = Layout::empty();
    layout.items.push(Item::Rect {
        common: make_common("rect-1", 0.0, 0.0, 100.0, 60.0),
        stroke: "#000".into(), fill: "#fff".into(),
        stroke_width: 2,
        fill_enabled: true, stroke_enabled: true,
        corner_radius: 30,  // cap = 30, OK
        stroke_dash: None,
    });
    assert!(validate(&layout).is_ok());
}
```

### AC-BE-C1. Text font_weight / font_style / underline / strikethrough round-trip

```rust
#[test]
fn text_full_style_round_trip() {
    let json = r#"{
        "id": "..uuid..", "type": "text", "parent_id": null,
        "x": 0, "y": 0, "w": 160, "h": 56, "z": 0,
        "visibility": "visible", "locked": false, "minimized": false,
        "text": "Hello", "font_size": 16, "color": "#333",
        "font_weight": "bold", "font_style": "italic",
        "underline": true, "strikethrough": false
    }"#;
    let item: Item = serde_json::from_str(json).unwrap();
    let Item::Text { font_weight, font_style, underline, strikethrough, .. } = item else { panic!() };
    assert_eq!(font_weight, Some(FontWeight::Bold));
    assert_eq!(font_style, Some(FontStyle::Italic));
    assert_eq!(underline, Some(true));
    assert_eq!(strikethrough, Some(false));
}

#[test]
fn text_old_layout_no_decorations() {
    let json = r#"{
        "id": "..uuid..", "type": "text", "parent_id": null,
        "x": 0, "y": 0, "w": 160, "h": 56, "z": 0,
        "visibility": "visible", "locked": false, "minimized": false,
        "text": "Hello", "font_size": 16, "color": "#333"
    }"#;
    let item: Item = serde_json::from_str(json).unwrap();
    let Item::Text { font_weight, font_style, underline, strikethrough, .. } = item else { panic!() };
    assert_eq!(font_weight, None);
    assert_eq!(font_style, None);
    assert_eq!(underline, None);
    assert_eq!(strikethrough, None);
}
```

### AC-BE-E1. Settings text_label_max_chars round-trip

```rust
#[test]
fn settings_text_label_max_chars_default_24() {
    let json = r#"{}"#;  // 옛 settings body
    let s: Settings = serde_json::from_str(json).unwrap_or_default();
    assert_eq!(s.text_label_max_chars, 24);
}

#[test]
fn settings_text_label_max_chars_range_validate() {
    // 0 reject
    let json = r#"{"text_label_max_chars": 0}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert!(validate_settings(&s).is_err());

    // 1 OK
    let json = r#"{"text_label_max_chars": 1}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert!(validate_settings(&s).is_ok());

    // 128 OK
    let json = r#"{"text_label_max_chars": 128}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert!(validate_settings(&s).is_ok());

    // 129 reject
    let json = r#"{"text_label_max_chars": 129}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert!(validate_settings(&s).is_err());
}
```

### AC-BE-E2. Settings GET/PUT integration

```rust
#[tokio::test]
async fn settings_text_label_max_chars_get_put_round_trip() {
    let state = make_test_state().await;
    // PUT custom value
    let body = json!({"text_label_max_chars": 50});
    let res = put_settings(&state, body).await.unwrap();
    assert_eq!(res.status(), 200);
    // GET back
    let got = get_settings(&state).await.unwrap();
    assert_eq!(got["text_label_max_chars"], 50);
}
```

### AC-BE-F. workspace test baseline

```bash
cd codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | tail -3
# 기대: 429 + 9 (BE-A 5종 + BE-C 2종 + BE-E 2종) ≈ 438+ PASS / 0 FAIL
cargo build --release --bin gtmux --color=never
# 기대: PASS
```

---

## §E. Anti-pattern — *하지 말 것*

1. **`#[serde(deny_unknown_fields)]` 를 Item 변형에 추가**: 신규 field 가 옛 layout 의 자연 deserialize 를 깬다 (현 schema 의 ItemCommon 도 deny 안 함). 본 ADR-0018 D5 의 *additive backward-compat* 위반.
2. **`fill_enabled` / `stroke_enabled` default = false**: 옛 record (없는 field) 의 의미가 변경 — 데이터 손실. **반드시 default = true**.
3. **corner_radius 의 silent clamp**: ADR-0006 의 *disk-of-truth* 위반. strict reject 후 FE 가 cap 적용.
4. **FigureStrokeDash 를 connector StrokeDash 와 통합**: wire 형식 차이로 옛 connector layout 의 round-trip 깨짐. 별 enum 유지.
5. **text 의 decoration 을 enum 1개**: 동시 underline+strikethrough 불가. 4 boolean 유지.
6. **text_label_max_chars 의 0 또는 음수 허용**: u32 라 음수는 컴파일러가 차단. 0 은 의미 모호 → range validate 강제.
7. **`bin/gen-openapi` 실행 누락**: FE 의 `lib/types/api.d.ts` 가 stale → svelte-check 깨짐. BE PR 의 *마지막 commit* 으로 typings regenerate.
8. **release build 누락** (0077 §5.1): 사용자 시연 환경의 binary stale → fix 가 적용 안 됨. 모든 commit 후 `cargo build --release` 확인.

---

## §F. Test plan

### F-1. 개별 단위 test (schema.rs::tests)

| Test | Module | 검증 |
|---|---|---|
| `rect_fill_stroke_enabled_round_trip` | schema | AC-BE-A1 |
| `rect_old_layout_defaults_fill_stroke_enabled_true` | schema | AC-BE-A2 |
| `rect_corner_radius_exceeding_box_rejected` | schema | AC-BE-A3 |
| `rect_corner_radius_at_cap_accepted` | schema | AC-BE-A3 |
| `ellipse_fill_stroke_enabled_round_trip` | schema | AC-BE-B (parallel) |
| `line_stroke_dash_round_trip` | schema | AC-BE-B (parallel) |
| `figure_stroke_dash_none_means_solid` | schema | enum 정합 (Option<None> ↔ Solid wire 의미) |
| `text_full_style_round_trip` | schema | AC-BE-C1 |
| `text_old_layout_no_decorations` | schema | AC-BE-C2 |
| `text_font_weight_serde_lowercase` | schema | `"bold"` ↔ `Bold` |
| `settings_text_label_max_chars_default_24` | settings | AC-BE-E1 |
| `settings_text_label_max_chars_range_validate` | settings | AC-BE-E1 |

총 신규 12 unit test.

### F-2. Integration test (http-api/src/lib.rs::tests 또는 별 file)

| Test | 검증 |
|---|---|
| `layout_put_with_rect_full_payload_then_get` | end-to-end — PUT layout with new rect → 204 + ETag → GET layout → 옛 + 새 field 모두 정합 |
| `layout_put_rect_corner_radius_overflow_400` | corner_radius=200, w=h=100 → 400 + `rect_corner_radius_exceeds_box` |
| `layout_put_old_rect_layout_then_get_defaults` | PUT 옛 layout (fill_enabled/stroke_enabled/corner_radius 부재) → 200 → GET 시 default true/true/0 |
| `settings_text_label_max_chars_get_put_round_trip` | AC-BE-E2 |
| `settings_text_label_max_chars_invalid_400` | PUT `text_label_max_chars: 200` → 400 |

총 신규 5 integration test.

### F-3. 통합 검증 (manual 또는 CI)

```bash
cd codebase/backend
cargo test --workspace --no-fail-fast --color=never 2>&1 | tail -3
# 기대: 429 + 12 (unit) + 5 (integration) = ~446 PASS / 0 FAIL

cargo build --release --bin gtmux --color=never
# 기대: PASS

# FE typings 재발행 (BE-F)
cargo run --bin gen-openapi
git diff shared/openapi.yaml codebase/frontend/src/lib/types/api.d.ts
# 기대: 신규 field 들이 spec 에 추가됨

# FE check (별 PR — BE-F 가 FE 의 prerequisite)
cd ../frontend
pnpm check
# 기대: 0 errors
```

### F-4. 시연 — manual E2E (BE 단독)

```bash
# 1. 신규 binary 기동
./target/release/gtmux --workspace /tmp/gtmux-test serve

# 2. FE 미 ship 환경에서 curl 로 layout PUT
curl -X PUT "http://localhost:9998/api/sessions/test/layout" \
  -H "Cookie: gtmux=<token>" \
  -H "If-Match: \"<etag>\"" \
  -H "Content-Type: application/json" \
  -d '{
    "schema_version": 2,
    "groups": [],
    "items": [{
      "id": "00000000-0000-0000-0000-000000000001",
      "type": "rect",
      "parent_id": null,
      "x": 0, "y": 0, "w": 100, "h": 100, "z": 0,
      "visibility": "visible", "locked": false, "minimized": false,
      "stroke": "#000", "fill": "#fff", "stroke_width": 2,
      "fill_enabled": false, "stroke_enabled": true,
      "corner_radius": 10, "stroke_dash": "dash"
    }],
    "viewport": {"x": 0, "y": 0, "zoom": 1.0}
  }'
# 기대: 204 + 새 ETag

# 3. GET layout — round-trip 검증
curl "http://localhost:9998/api/sessions/test/layout" -H "Cookie: gtmux=<token>"
# 기대: items[0].fill_enabled=false, corner_radius=10, stroke_dash="dash"

# 4. corner_radius 초과 시 400
curl -X PUT ... -d '{...corner_radius: 200, w: 100, h: 100...}'
# 기대: 400 + {"code": "rect_corner_radius_exceeds_box", ...}

# 5. text full style
curl -X PUT ... -d '{...text: "Hello", font_weight: "bold", italic: ...}'
# 기대: 204 → GET → 모든 field 정합
```

---

## §G. Commit 분리 권장

| Commit | 내용 |
|---|---|
| `feat(be/schema): batch-5 rect/ellipse/line fill·stroke enabled + corner_radius + stroke_dash (ADR-0018 D4 amend ①)` | BE-A + BE-B + BE-D 의 rect 관련 + 신규 enum + ADR D4 amend ① + 8 unit + 3 integration |
| `feat(be/schema): batch-5 text font_weight/font_style/underline/strikethrough (ADR-0018 D4 amend ②)` | BE-C + 4 unit + ADR D4 amend ② |
| `feat(be/settings): text_label_max_chars (ADR-0017 §Settings amend)` | BE-E + 2 unit + 2 integration + ADR amend |
| `chore(be/openapi): regen typings — batch-5 schema amend` | BE-F (생성된 yaml/d.ts 의 commit) |

→ 4 commit. ADR coherence hard rule 따라 각 commit 에 ADR amend 동봉.

---

## §H. 의존성

- **Prerequisite**: 0080 asset upload BE endpoint 의 *현재 진행* (`docs/reports/2026-05-20-session-handover-...-0080-...md`) 와 file 충돌 없음 (다른 module). parallel land 가능.
- **Independent**: 0078 connector BE-B (`Item::Connector` schema variant) 와도 file 같음 (`schema.rs`) — *commit 순서 결정*. **권장**: 0078 BE 가 먼저 (Connector variant 가 더 복잡, 본 batch 는 단순 field 추가). 0078 land 후 본 batch 진입.
- **FE 의존**: 본 batch land + BE-F (typings regenerate) 후 FE handover 의 진입.

---

## §I. Self-check 표

- [ ] schema.rs::Item::Rect / Ellipse / Line 의 신규 field 추가됨
- [ ] schema.rs::Item::Text 의 신규 4 field 추가됨
- [ ] FigureStrokeDash / FontWeight / FontStyle enum 신규 + serde 정합
- [ ] schema.rs::ValidationError::RectCornerRadiusExceedsBox + code() 매핑 + validate() 분기
- [ ] settings.rs::Settings::text_label_max_chars + default + validate range
- [ ] 12 신규 unit test + 5 신규 integration test PASS
- [ ] workspace cargo test PASS
- [ ] cargo build --release PASS
- [ ] bin/gen-openapi 재발행 후 commit
- [ ] ADR-0018 D4 amend ① + amend ② + D8 의 신규 ValidationError 명시 동봉
- [ ] ADR-0017 §Settings amend 동봉 (text_label_max_chars)
- [ ] CLAUDE.md 의 ADR ↔ plan/handover coherence 정합 — 본 handover 가 latest

---

## 변경 이력

- 2026-05-20: 초안. 8 UI/UX 요구의 BE-side amend (R1 + R2 + R3 schema + R7 settings 키). R4/R5/R6/R8 은 FE-only — `2026-05-20-fe-handover-ui-ux-batch-5.md` 참조.
- 2026-05-20 (Grill #1-19 amend): 본 doc 상단의 ⚠️ amend 표 적용. **BE-E 폐기** (`text_label_max_chars` Settings 신설 안 함). `corner_radius` → `corner_rounded` (bool). `RectCornerRadiusExceedsBox` ValidationError 폐기. `StrokeWidthOutOfRange` + `TextFontSizeOutOfRange` 신규. FontWeight 3 variant. FontStyle enum 폐기. italic/underline/strikethrough = `bool + #[serde(default)]`. 총 3 commit.
