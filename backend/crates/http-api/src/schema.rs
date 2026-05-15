//! Canvas Layout schema v2 — unified `items[]` discriminated union.
//!
//! Source-of-truth: `docs/adr/0018-canvas-item-data-model.md` (D1, D3, D4, D8).
//! Companion ADRs:
//! - ADR-0010 — Group data model (`groups[]` shape, unchanged across v1→v2)
//! - ADR-0019 — Session/Workspace model (this schema is the body of a
//!   `<workspace>/<name>.json` SessionRecord)
//! - ADR-0024 — Layer tree / Z separation (z mutates only via 4 actions; tree
//!   reorder leaves z untouched). Persistence treats z as plain `i32` data.
//!
//! Validation order (subset of ADR-0018 D8 + ADR-0006 R-rules):
//!   1. serde shape (top-level `schema_version: 2`, `groups`, `items`, `viewport`)
//!   2. id is UUID-shaped (lowercase 8-4-4-4-12 hex)
//!   3. parent_id refers to a known group id (or null)
//!   4. payload caps: label/description 4 KiB, text 64 KiB, free_draw points 5000
//!   5. (file-size cap 16 MiB is enforced at the HTTP body-read layer)
//!
//! `maximized` is intentionally absent — ADR-0018 D3 (G20 amend) demotes it to
//! FE-only ephemeral state.

// Field-level docs for the schema structs live in ADR-0018 §D3/§D4 (this
// module is intentionally a 1:1 mirror of the table there). Suppressing the
// per-field `missing_docs` lint keeps the source close to the ADR text
// instead of duplicating it 80 times.
#![allow(missing_docs)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The schema version this module reads and writes.
pub const SCHEMA_VERSION: u32 = 2;

/// Maximum bytes for a `label` or `description` string (ADR-0018 D8).
pub const LABEL_DESCRIPTION_MAX_BYTES: usize = 4 * 1024;

/// Maximum bytes for a `text` item's `text` payload (ADR-0018 D8).
pub const TEXT_PAYLOAD_MAX_BYTES: usize = 64 * 1024;

/// Maximum number of points in a `free_draw` item (ADR-0018 D8).
pub const FREE_DRAW_POINT_CAP: usize = 5000;

// ─────────────────────────────────────────────────────────────────────────────
//  Top-level Layout
// ─────────────────────────────────────────────────────────────────────────────

/// Body of a Session file record. Serde-derived: round-trips losslessly with
/// the on-disk JSON shape defined in ADR-0018 D1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// Always `2` for v2 records. v1 → v2 hard cutover lives in `workspace.rs`.
    pub schema_version: u32,
    /// Group tree (unchanged across v1/v2 — ADR-0010).
    pub groups: Vec<Group>,
    /// Canvas Items (terminal Panel + non-terminal) — discriminated union by `type`.
    pub items: Vec<Item>,
    /// Canvas viewport state (pan + zoom).
    #[serde(default)]
    pub viewport: Viewport,
}

impl Layout {
    /// Empty v2 layout — used for newly-created sessions.
    pub fn empty() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            groups: Vec::new(),
            items: Vec::new(),
            viewport: Viewport::default(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Group, Viewport, Visibility
// ─────────────────────────────────────────────────────────────────────────────

/// Group node — shape locked by ADR-0010, kept identical across the v1→v2
/// cutover so `boot_migration_v1_to_v2` can preserve `groups[]` verbatim.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub parent_id: Option<String>,
    pub label: String,
    pub color: Option<String>,
    pub visibility: Visibility,
    pub locked: bool,
    pub order: i32,
}

/// Visibility tri-state. The JSON wire form is `"visible" | "hidden"`. We keep
/// it as a typed enum here instead of `bool` because ADR-0010 explicitly leaves
/// room for an `inherit` variant in the layer-tree work, and so that round-trip
/// against legacy v1 payloads (which used `true`/`false`) is detected as an
/// error rather than silently coerced.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Visible,
    Hidden,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::Visible
    }
}

/// Canvas viewport state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Item enum (discriminated union)
// ─────────────────────────────────────────────────────────────────────────────

/// Fields common to every Canvas Item — flattened into each variant of [`Item`]
/// via `#[serde(flatten)]` so the on-disk JSON keeps a single flat object per
/// item (no nested `common: { ... }` envelope).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemCommon {
    pub id: String,
    pub parent_id: Option<String>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub z: i32,
    pub visibility: Visibility,
    pub locked: bool,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub minimized: bool,
}

/// 2D point — payload of `free_draw` items.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Canvas Item discriminated union (ADR-0018 D1, D4).
///
/// On the wire each variant becomes `{ "type": "<snake>", ...common, ...payload }`
/// thanks to `#[serde(tag = "type")]` + `#[serde(flatten)]` on `common`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    Terminal {
        #[serde(flatten)]
        common: ItemCommon,
    },
    Text {
        #[serde(flatten)]
        common: ItemCommon,
        text: String,
        font_size: u32,
        color: String,
    },
    Note {
        #[serde(flatten)]
        common: ItemCommon,
        title: String,
        body: String,
        color: String,
    },
    Rect {
        #[serde(flatten)]
        common: ItemCommon,
        stroke: String,
        fill: String,
        stroke_width: u32,
    },
    Ellipse {
        #[serde(flatten)]
        common: ItemCommon,
        stroke: String,
        fill: String,
        stroke_width: u32,
    },
    Line {
        #[serde(flatten)]
        common: ItemCommon,
        stroke: String,
        stroke_width: u32,
        x2: f64,
        y2: f64,
    },
    FreeDraw {
        #[serde(flatten)]
        common: ItemCommon,
        stroke: String,
        stroke_width: u32,
        points: Vec<Point>,
    },
    Image {
        #[serde(flatten)]
        common: ItemCommon,
        asset_id: String,
        mime: String,
        original_w: Option<u32>,
        original_h: Option<u32>,
    },
    Document {
        #[serde(flatten)]
        common: ItemCommon,
        asset_id: String,
        mime: String,
        file_name: String,
        size_bytes: u64,
    },
    FilePath {
        #[serde(flatten)]
        common: ItemCommon,
        path: String,
        kind: Option<String>,
    },
}

impl Item {
    /// Borrow the common fields regardless of variant.
    pub fn common(&self) -> &ItemCommon {
        match self {
            Item::Terminal { common }
            | Item::Text { common, .. }
            | Item::Note { common, .. }
            | Item::Rect { common, .. }
            | Item::Ellipse { common, .. }
            | Item::Line { common, .. }
            | Item::FreeDraw { common, .. }
            | Item::Image { common, .. }
            | Item::Document { common, .. }
            | Item::FilePath { common, .. } => common,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Validation
// ─────────────────────────────────────────────────────────────────────────────

/// Structured validation failure. Each variant is mapped to HTTP 400 by the
/// handler with the variant name as a machine-readable code.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("schema_version must be {expected}, got {actual}")]
    BadSchemaVersion { expected: u32, actual: u32 },
    #[error("item id is not a UUID v4-shape lowercase string: {0:?}")]
    BadItemId(String),
    #[error("group id is not a UUID v4-shape lowercase string: {0:?}")]
    BadGroupId(String),
    #[error("item parent_id {parent:?} does not refer to a known group id")]
    DanglingItemParent { parent: String },
    #[error("group parent_id {parent:?} does not refer to a known group id")]
    DanglingGroupParent { parent: String },
    #[error("duplicate group id: {0:?}")]
    DuplicateGroupId(String),
    #[error("duplicate item id: {0:?}")]
    DuplicateItemId(String),
    #[error("label exceeds {} bytes", LABEL_DESCRIPTION_MAX_BYTES)]
    LabelTooLong,
    #[error("description exceeds {} bytes", LABEL_DESCRIPTION_MAX_BYTES)]
    DescriptionTooLong,
    #[error("text payload exceeds {} bytes", TEXT_PAYLOAD_MAX_BYTES)]
    TextTooLong,
    #[error("free_draw exceeds {} points", FREE_DRAW_POINT_CAP)]
    FreeDrawTooManyPoints,
}

impl ValidationError {
    /// Stable machine-readable code surfaced in HTTP error envelopes.
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadSchemaVersion { .. } => "bad_schema_version",
            Self::BadItemId(_) => "bad_item_id",
            Self::BadGroupId(_) => "bad_group_id",
            Self::DanglingItemParent { .. } => "dangling_item_parent",
            Self::DanglingGroupParent { .. } => "dangling_group_parent",
            Self::DuplicateGroupId(_) => "duplicate_group_id",
            Self::DuplicateItemId(_) => "duplicate_item_id",
            Self::LabelTooLong => "label_too_long",
            Self::DescriptionTooLong => "description_too_long",
            Self::TextTooLong => "text_too_long",
            Self::FreeDrawTooManyPoints => "free_draw_too_many_points",
        }
    }
}

/// Validate a v2 [`Layout`] against ADR-0018 D8 rules. Returns the first
/// failure encountered — callers that want the full set should fix one and
/// re-call.
pub fn validate(layout: &Layout) -> Result<(), ValidationError> {
    if layout.schema_version != SCHEMA_VERSION {
        return Err(ValidationError::BadSchemaVersion {
            expected: SCHEMA_VERSION,
            actual: layout.schema_version,
        });
    }

    // Groups: id format + uniqueness.
    let mut group_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for g in &layout.groups {
        if !is_uuid_shape(&g.id) {
            return Err(ValidationError::BadGroupId(g.id.clone()));
        }
        if !group_ids.insert(&g.id) {
            return Err(ValidationError::DuplicateGroupId(g.id.clone()));
        }
        if g.label.len() > LABEL_DESCRIPTION_MAX_BYTES {
            return Err(ValidationError::LabelTooLong);
        }
    }
    // Groups: parent integrity. Reference must point to a sibling group id.
    for g in &layout.groups {
        if let Some(parent) = &g.parent_id {
            if !group_ids.contains(parent.as_str()) {
                return Err(ValidationError::DanglingGroupParent {
                    parent: parent.clone(),
                });
            }
        }
    }

    // Items: id format + uniqueness + parent + per-variant caps.
    let mut item_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for it in &layout.items {
        let common = it.common();
        if !is_uuid_shape(&common.id) {
            return Err(ValidationError::BadItemId(common.id.clone()));
        }
        if !item_ids.insert(&common.id) {
            return Err(ValidationError::DuplicateItemId(common.id.clone()));
        }
        if let Some(parent) = &common.parent_id {
            if !group_ids.contains(parent.as_str()) {
                return Err(ValidationError::DanglingItemParent {
                    parent: parent.clone(),
                });
            }
        }
        if common.label.len() > LABEL_DESCRIPTION_MAX_BYTES {
            return Err(ValidationError::LabelTooLong);
        }
        if common.description.len() > LABEL_DESCRIPTION_MAX_BYTES {
            return Err(ValidationError::DescriptionTooLong);
        }
        match it {
            Item::Text { text, .. } => {
                if text.len() > TEXT_PAYLOAD_MAX_BYTES {
                    return Err(ValidationError::TextTooLong);
                }
            }
            Item::FreeDraw { points, .. } => {
                if points.len() > FREE_DRAW_POINT_CAP {
                    return Err(ValidationError::FreeDrawTooManyPoints);
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Lowercase 8-4-4-4-12 hex check. We don't import the `uuid` crate because
/// the server only consumes the string form — generation is the spawn-side
/// concern (Stage 4) and any UUID generator we adopt later writes the same
/// canonical shape this validator accepts.
fn is_uuid_shape(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    let hex_at = |idx: usize| matches!(b[idx], b'0'..=b'9' | b'a'..=b'f');
    let dash_at = |idx: usize| b[idx] == b'-';
    if !(dash_at(8) && dash_at(13) && dash_at(18) && dash_at(23)) {
        return false;
    }
    for i in 0..36 {
        if matches!(i, 8 | 13 | 18 | 23) {
            continue;
        }
        if !hex_at(i) {
            return false;
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
//  v1 → v2 hard cutover (ADR-0018 D5, ADR-0006 D15)
// ─────────────────────────────────────────────────────────────────────────────

/// Result of inspecting an on-disk JSON object.
#[derive(Debug, PartialEq)]
pub enum SchemaShape {
    /// `schema_version == 2`. The caller can `serde_json::from_value` it.
    V2,
    /// `schema_version == 1`. Caller should run `migrate_v1_to_v2` and
    /// atomic-write the result back.
    V1,
    /// Missing or unknown `schema_version`. Quarantine via the existing
    /// sidecar policy (ADR-0006 D10 row 5).
    Unknown,
}

/// Inspect a parsed JSON object and classify its schema version.
pub fn detect_shape(body: &Value) -> SchemaShape {
    match body.get("schema_version").and_then(Value::as_u64) {
        Some(2) => SchemaShape::V2,
        Some(1) => SchemaShape::V1,
        _ => SchemaShape::Unknown,
    }
}

/// Transform a v1 body to a v2 body in-place (ADR-0018 D5 hard cutover):
/// preserve `groups[]`, drop `panels[]`, install empty `items[]`, bump
/// `schema_version` to 2. Idempotent — calling it on a v2 body is a no-op.
pub fn migrate_v1_to_v2(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    obj.insert("schema_version".into(), Value::from(SCHEMA_VERSION));
    obj.remove("panels");
    obj.entry("items".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    obj.entry("groups".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    obj.entry("viewport".to_string()).or_insert_with(|| {
        serde_json::json!({ "x": 0.0, "y": 0.0, "zoom": 1.0 })
    });
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    const UUID_A: &str = "7f3a0000-b9e2-4111-8222-000000000001";
    const UUID_B: &str = "8a4c0000-c7f1-4111-8222-000000000002";
    const UUID_G: &str = "0d990000-0000-4111-8222-000000000003";

    fn item_common(id: &str) -> ItemCommon {
        ItemCommon {
            id: id.to_string(),
            parent_id: None,
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
            z: 0,
            visibility: Visibility::Visible,
            locked: false,
            label: String::new(),
            description: String::new(),
            minimized: false,
        }
    }

    #[test]
    fn empty_layout_validates() {
        let l = Layout::empty();
        assert!(validate(&l).is_ok());
    }

    #[test]
    fn round_trip_terminal_item() {
        let l = Layout {
            schema_version: 2,
            groups: vec![],
            items: vec![Item::Terminal {
                common: item_common(UUID_A),
            }],
            viewport: Viewport::default(),
        };
        let s = serde_json::to_string(&l).unwrap();
        let parsed: Layout = serde_json::from_str(&s).unwrap();
        assert_eq!(l, parsed);
        // Confirm the wire shape has flat fields, not a nested `common`.
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["items"][0]["type"], "terminal");
        assert_eq!(v["items"][0]["id"], UUID_A);
        assert!(v["items"][0].get("common").is_none());
    }

    #[test]
    fn round_trip_all_variants() {
        let l = Layout {
            schema_version: 2,
            groups: vec![Group {
                id: UUID_G.to_string(),
                parent_id: None,
                label: "main".into(),
                color: Some("#abcdef".into()),
                visibility: Visibility::Visible,
                locked: false,
                order: 0,
            }],
            items: vec![
                Item::Text {
                    common: item_common(UUID_A),
                    text: "hi".into(),
                    font_size: 14,
                    color: "#333".into(),
                },
                Item::FreeDraw {
                    common: item_common(UUID_B),
                    stroke: "#000".into(),
                    stroke_width: 2,
                    points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 1.0, y: 1.0 }],
                },
            ],
            viewport: Viewport {
                x: 10.0,
                y: 20.0,
                zoom: 1.5,
            },
        };
        let s = serde_json::to_string(&l).unwrap();
        let parsed: Layout = serde_json::from_str(&s).unwrap();
        assert_eq!(l, parsed);
    }

    #[test]
    fn bad_schema_version_rejected() {
        let mut l = Layout::empty();
        l.schema_version = 1;
        assert!(matches!(
            validate(&l),
            Err(ValidationError::BadSchemaVersion { .. })
        ));
    }

    #[test]
    fn bad_item_id_rejected() {
        let mut l = Layout::empty();
        l.items.push(Item::Terminal {
            common: item_common("not-a-uuid"),
        });
        assert!(matches!(validate(&l), Err(ValidationError::BadItemId(_))));
    }

    #[test]
    fn duplicate_item_id_rejected() {
        let mut l = Layout::empty();
        l.items.push(Item::Terminal {
            common: item_common(UUID_A),
        });
        l.items.push(Item::Terminal {
            common: item_common(UUID_A),
        });
        assert!(matches!(
            validate(&l),
            Err(ValidationError::DuplicateItemId(_))
        ));
    }

    #[test]
    fn dangling_item_parent_rejected() {
        let mut l = Layout::empty();
        let mut common = item_common(UUID_A);
        common.parent_id = Some(UUID_G.to_string());
        l.items.push(Item::Terminal { common });
        assert!(matches!(
            validate(&l),
            Err(ValidationError::DanglingItemParent { .. })
        ));
    }

    #[test]
    fn item_parent_pointing_to_known_group_ok() {
        let mut l = Layout::empty();
        l.groups.push(Group {
            id: UUID_G.to_string(),
            parent_id: None,
            label: "g".into(),
            color: None,
            visibility: Visibility::Visible,
            locked: false,
            order: 0,
        });
        let mut common = item_common(UUID_A);
        common.parent_id = Some(UUID_G.to_string());
        l.items.push(Item::Terminal { common });
        assert!(validate(&l).is_ok());
    }

    #[test]
    fn label_cap_enforced() {
        let mut l = Layout::empty();
        let mut common = item_common(UUID_A);
        common.label = "x".repeat(LABEL_DESCRIPTION_MAX_BYTES + 1);
        l.items.push(Item::Terminal { common });
        assert_eq!(validate(&l), Err(ValidationError::LabelTooLong));
    }

    #[test]
    fn text_cap_enforced() {
        let mut l = Layout::empty();
        l.items.push(Item::Text {
            common: item_common(UUID_A),
            text: "x".repeat(TEXT_PAYLOAD_MAX_BYTES + 1),
            font_size: 14,
            color: "#000".into(),
        });
        assert_eq!(validate(&l), Err(ValidationError::TextTooLong));
    }

    #[test]
    fn free_draw_point_cap_enforced() {
        let mut l = Layout::empty();
        let points = (0..(FREE_DRAW_POINT_CAP + 1))
            .map(|i| Point { x: i as f64, y: 0.0 })
            .collect();
        l.items.push(Item::FreeDraw {
            common: item_common(UUID_A),
            stroke: "#000".into(),
            stroke_width: 1,
            points,
        });
        assert_eq!(
            validate(&l),
            Err(ValidationError::FreeDrawTooManyPoints)
        );
    }

    #[test]
    fn maximized_field_is_dropped_on_round_trip() {
        // ADR-0018 D3 (G20 amend): `maximized` is FE-only ephemeral. Persisted
        // payloads that carry it must NOT round-trip the field — serde
        // `#[serde(flatten)]` on `common` makes the per-item shape an open
        // map at parse time, so unknown fields are silently dropped instead
        // of rejected. The functional contract (the field never survives a
        // round trip) is what G20 actually requires.
        let raw = json!({
            "schema_version": 2,
            "groups": [],
            "items": [{
                "type": "terminal",
                "id": UUID_A,
                "parent_id": null,
                "x": 0, "y": 0, "w": 10, "h": 10, "z": 0,
                "visibility": "visible", "locked": false,
                "label": "", "description": "", "minimized": false,
                "maximized": true,
            }],
            "viewport": { "x": 0, "y": 0, "zoom": 1.0 },
        });
        let parsed: Layout = serde_json::from_value(raw).expect("parses with unknown field");
        let s = serde_json::to_string(&parsed).unwrap();
        assert!(
            !s.contains("maximized"),
            "maximized must not appear in the serialized form: {s}"
        );
    }

    #[test]
    fn detect_shape_classifies_versions() {
        assert_eq!(detect_shape(&json!({ "schema_version": 2 })), SchemaShape::V2);
        assert_eq!(detect_shape(&json!({ "schema_version": 1 })), SchemaShape::V1);
        assert_eq!(detect_shape(&json!({ "schema_version": 0 })), SchemaShape::Unknown);
        assert_eq!(detect_shape(&json!({})), SchemaShape::Unknown);
    }

    #[test]
    fn migrate_v1_to_v2_preserves_groups_drops_panels() {
        let mut body = json!({
            "schema_version": 1,
            "groups": [{
                "id": UUID_G,
                "parent_id": null,
                "label": "main",
                "color": null,
                "visibility": "visible",
                "locked": false,
                "order": 0,
            }],
            "panels": [{ "id": "%2", "x": 10, "y": 20 }],
        });
        migrate_v1_to_v2(&mut body);
        assert_eq!(body["schema_version"], 2);
        assert_eq!(body["groups"].as_array().unwrap().len(), 1);
        assert_eq!(body["groups"][0]["id"], UUID_G);
        assert_eq!(body["items"].as_array().unwrap().len(), 0);
        assert!(body.get("panels").is_none(), "panels must be dropped");
        assert_eq!(body["viewport"]["zoom"], 1.0);
        // After migration, the body must parse as a v2 Layout.
        let _layout: Layout = serde_json::from_value(body).expect("parses as v2");
    }

    #[test]
    fn migrate_v1_to_v2_is_idempotent_on_v2() {
        let mut body = json!({
            "schema_version": 2,
            "groups": [],
            "items": [],
            "viewport": { "x": 0, "y": 0, "zoom": 1.0 },
        });
        let before = body.clone();
        migrate_v1_to_v2(&mut body);
        assert_eq!(before, body);
    }
}
