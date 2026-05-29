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

/// Maximum bytes for an inline-stored `document` item's `content` payload
/// (ADR-0018 D10 amend ① — 2026-05-16 components batch). Matches the
/// "UTF-8 markdown, cap 64 KB" wording in the ADR.
pub const DOCUMENT_INLINE_MAX_BYTES: usize = 64 * 1024;

/// Maximum bytes for a `SnippetEntry::key` (ADR-0038 D2 / O3).
/// Badge display 길이 한도. truncate 는 FE 책임 — BE 는 hard cap 만 enforce.
pub const SNIPPET_KEY_MAX_BYTES: usize = 256;

/// Maximum bytes for a `SnippetEntry::body` (ADR-0038 D2 / O2).
/// 64 KB — `DOCUMENT_INLINE_MAX_BYTES` 와 동일. 단일 snippet 의 body 가
/// 64 KB 를 넘으면 그건 더 이상 snippet 이 아니라 document.
pub const SNIPPET_BODY_MAX_BYTES: usize = 64 * 1024;

/// Maximum number of entries per `Snippets` item (ADR-0038 D2).
/// 한 node 의 badge 가 1000 개 이상이면 wrap 자체가 UX 파괴. wire / storage
/// 의 hard cap 으로 1000 enforce — FE 의 [+ add] 는 999 entry 이후 disabled.
pub const SNIPPETS_ENTRIES_CAP: usize = 1000;

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

/// Horizontal alignment for text Canvas Items. Defaults to center so newly
/// created empty text boxes edit from their visual center.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl Default for TextAlign {
    fn default() -> Self {
        Self::Center
    }
}

/// Vertical alignment for text Canvas Items.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TextVerticalAlign {
    Top,
    Middle,
    Bottom,
}

impl Default for TextVerticalAlign {
    fn default() -> Self {
        Self::Middle
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

/// 2D point — shared payload for `free_draw` and `connector` items.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// ADR-0038 D2 — 1 snippet = 1 (key, body) pair. Multiple entries live in
/// a `Snippets` item's `entries` Vec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SnippetEntry {
    /// UUID v4 lowercase 36-char. Stable across edits — FE uses this for
    /// list reconciliation and reorder.
    pub id: String,
    /// Badge display label. Must be non-empty after `trim`. Length cap:
    /// [`SNIPPET_KEY_MAX_BYTES`]. Duplicate keys within the same item are
    /// allowed (FE shows a soft hint — ADR-0038 D7 / O9).
    pub key: String,
    /// Body payload — copied to clipboard verbatim on badge click. Allowed
    /// to be empty. Length cap: [`SNIPPET_BODY_MAX_BYTES`].
    pub body: String,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Connector enums (ADR-0036 D1)
// ─────────────────────────────────────────────────────────────────────────────

/// 9-point connector anchor — 8 cardinal/diagonal edges + center (ADR-0036 D2).
/// Wire form uses the uppercase keyword for the 8 edges and lowercase
/// `"center"` for the middle. The `"auto"` mode is P1+.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Anchor {
    #[serde(rename = "N")]
    N,
    #[serde(rename = "NE")]
    NE,
    #[serde(rename = "E")]
    E,
    #[serde(rename = "SE")]
    SE,
    #[serde(rename = "S")]
    S,
    #[serde(rename = "SW")]
    SW,
    #[serde(rename = "W")]
    W,
    #[serde(rename = "NW")]
    NW,
    #[serde(rename = "center")]
    Center,
}

/// Connector arrowhead direction mode (ADR-0036 D4). `head_from` / `head_to`
/// may still override the default mapping per-end.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Uni,
    Bi,
    None,
}

/// Per-end connector head marker (ADR-0036 D4).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Head {
    Arrow,
    Circle,
    Diamond,
    None,
}

/// Connector routing style (ADR-0036 D3). MVP wires `Straight` only; the
/// other two variants persist for round-trip but the FE renderer falls
/// back to straight until P1.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Routing {
    Straight,
    Orthogonal,
    Bezier,
}

/// Connector stroke dash pattern (ADR-0036 D1 — optional, `null` for solid).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StrokeDash {
    Dash,
    Dot,
}

/// Figure stroke dash pattern (ADR-0018 D4 amend ① — 2026-05-20 batch 5,
/// rect / ellipse / line). Kept distinct from connector's [`StrokeDash`]
/// because the figure form is a 4-variant enum with an explicit `Solid`
/// default, while connector's wire uses `null` for solid (round-trip
/// compatibility would break if the two were unified).
///
/// Wire form is `snake_case`: `"solid" | "dash" | "dot" | "dash_dot"`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FigureStrokeDash {
    Solid,
    Dash,
    Dot,
    DashDot,
}

impl Default for FigureStrokeDash {
    fn default() -> Self {
        Self::Solid
    }
}

/// Text font weight (ADR-0018 D4 amend ② — 2026-05-20 batch 5). MVP carries
/// three named buckets; numeric weights (100…900) are P1.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FontWeight {
    Light,
    Normal,
    Bold,
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::Normal
    }
}

/// Font family bucket for text-bearing items (ADR-0041 D1). Three named
/// stacks resolved to concrete `font-family` declarations FE-side; the BE
/// only persists the keyword. Applies to `text` / `rect` / `ellipse`
/// (ADR-0041 D3 — `note` / `document` keep their own typography).
///
/// Wire form is `lowercase`: `"sans" | "serif" | "mono"`. Default = `Sans`.
/// An unknown variant is a serde deserialize error (strict) — the enum is
/// FE-controlled so `#[serde(other)]` fallback is intentionally not added
/// (an unrecognised value is safer rejected than silently coerced). The
/// per-field `#[serde(default)]` keeps a *missing* field from rejecting the
/// whole layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FontFamily {
    #[default]
    Sans,
    Serif,
    Mono,
}

/// `#[serde(default = "default_true")]` helper — used by Rect/Ellipse to
/// keep `fill_enabled` / `stroke_enabled` defaulting to `true` for legacy
/// records that pre-date the 2026-05-20 batch 5 schema amend.
fn default_true() -> bool {
    true
}

/// `#[serde(default = "default_font_size")]` helper — embedded text on
/// `rect` / `ellipse` (ADR-0040 D1) defaults to 14 px when absent. Distinct
/// from `Item::Text`, where `font_size` stays a required field.
fn default_font_size() -> u32 {
    14
}

/// `#[serde(default = "default_stroke_width")]` helper — the box stroke on
/// `Item::Text` (ADR-0040 D1) defaults to 2 px when absent, matching the
/// figure stroke default. The legal range is the shared figure 1..=32.
fn default_stroke_width() -> u32 {
    2
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
        #[serde(default)]
        text_align: TextAlign,
        #[serde(default)]
        text_vertical_align: TextVerticalAlign,
        color: String,
        /// ADR-0018 D4 amend ② (2026-05-20 batch 5) — text font weight.
        /// Defaults to `Normal` when absent so legacy records round-trip.
        #[serde(default)]
        font_weight: FontWeight,
        /// ADR-0018 D4 amend ② — italic toggle. CSS `font-style: italic`.
        #[serde(default)]
        italic: bool,
        /// ADR-0018 D4 amend ② — underline toggle. Composes with
        /// `strikethrough` via CSS `text-decoration: underline line-through`.
        #[serde(default)]
        underline: bool,
        /// ADR-0018 D4 amend ② — strikethrough toggle.
        #[serde(default)]
        strikethrough: bool,
        /// ADR-0041 D3 — font family bucket. Defaults to `Sans`.
        #[serde(default)]
        font_family: FontFamily,
        // ── ADR-0040 D1/D2 BoxStyle — text box is default-OFF ──
        /// Box stroke color. Empty string = FE token default.
        #[serde(default)]
        stroke: String,
        /// Box fill color. Empty string = FE token default.
        #[serde(default)]
        fill: String,
        /// Box stroke width. Shared figure range 1..=32; default 2.
        #[serde(default = "default_stroke_width")]
        stroke_width: u32,
        /// Box fill on/off. ADR-0040 D2: **default `false`** (opposite of
        /// Rect/Ellipse) so legacy text records render box-less.
        #[serde(default)]
        fill_enabled: bool,
        /// Box stroke on/off. ADR-0040 D2: **default `false`** (opposite of
        /// Rect/Ellipse).
        #[serde(default)]
        stroke_enabled: bool,
        /// Rounded-corner toggle for the text box (radius computed FE-side).
        #[serde(default)]
        corner_rounded: bool,
        /// Box stroke dash pattern. `None` = solid.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stroke_dash: Option<FigureStrokeDash>,
        /// ADR-0040 D9 — label auto-derive hint. `None` = unset (no strict
        /// invariant); `Some(true)` = label tracks `text`, `Some(false)` =
        /// user pinned a custom label.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label_auto: Option<bool>,
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
        /// ADR-0018 D4 amend ① (2026-05-20 batch 5) — fill on/off.
        /// `false` is *not* alpha=0: hit-testing is also disabled by the FE
        /// (the painted area no longer captures pointer events). Legacy
        /// records default to `true`.
        #[serde(default = "default_true")]
        fill_enabled: bool,
        /// ADR-0018 D4 amend ① — stroke on/off. `false` removes both the
        /// rendered border and its hit-target band.
        #[serde(default = "default_true")]
        stroke_enabled: bool,
        /// ADR-0018 D4 amend ① — rounded-corner toggle (rect only). The
        /// actual radius is computed FE-side as `clamp(min(w,h)*0.15, 4, 16)`;
        /// the BE only persists the boolean.
        #[serde(default)]
        corner_rounded: bool,
        /// ADR-0018 D4 amend ① — stroke dash pattern. `None` means solid;
        /// `Some(Solid)` round-trips identically.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stroke_dash: Option<FigureStrokeDash>,
        // ── ADR-0040 D1 embedded TextStyle — empty `text` = not rendered ──
        /// Embedded text body. Default `""` (legacy rects render no text).
        #[serde(default)]
        text: String,
        /// Embedded text size. 8..=96; default 14.
        #[serde(default = "default_font_size")]
        font_size: u32,
        /// Embedded text color. Empty string = FE token foreground.
        #[serde(default)]
        color: String,
        #[serde(default)]
        text_align: TextAlign,
        #[serde(default)]
        text_vertical_align: TextVerticalAlign,
        #[serde(default)]
        font_weight: FontWeight,
        #[serde(default)]
        italic: bool,
        #[serde(default)]
        underline: bool,
        #[serde(default)]
        strikethrough: bool,
        /// ADR-0041 D3 — font family bucket. Defaults to `Sans`.
        #[serde(default)]
        font_family: FontFamily,
        /// ADR-0040 D9 — label auto-derive hint. See `Item::Text::label_auto`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label_auto: Option<bool>,
    },
    Ellipse {
        #[serde(flatten)]
        common: ItemCommon,
        stroke: String,
        fill: String,
        stroke_width: u32,
        /// ADR-0018 D4 amend ① — see `Rect::fill_enabled`.
        #[serde(default = "default_true")]
        fill_enabled: bool,
        /// ADR-0018 D4 amend ① — see `Rect::stroke_enabled`.
        #[serde(default = "default_true")]
        stroke_enabled: bool,
        /// ADR-0018 D4 amend ① — stroke dash pattern. `None` = solid.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stroke_dash: Option<FigureStrokeDash>,
        // ── ADR-0040 D1 embedded TextStyle — `corner_rounded` is rect-only ──
        /// Embedded text body. Default `""`.
        #[serde(default)]
        text: String,
        /// Embedded text size. 8..=96; default 14.
        #[serde(default = "default_font_size")]
        font_size: u32,
        /// Embedded text color. Empty string = FE token foreground.
        #[serde(default)]
        color: String,
        #[serde(default)]
        text_align: TextAlign,
        #[serde(default)]
        text_vertical_align: TextVerticalAlign,
        #[serde(default)]
        font_weight: FontWeight,
        #[serde(default)]
        italic: bool,
        #[serde(default)]
        underline: bool,
        #[serde(default)]
        strikethrough: bool,
        /// ADR-0041 D3 — font family bucket. Defaults to `Sans`.
        #[serde(default)]
        font_family: FontFamily,
        /// ADR-0040 D9 — label auto-derive hint. See `Item::Text::label_auto`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label_auto: Option<bool>,
    },
    Line {
        #[serde(flatten)]
        common: ItemCommon,
        stroke: String,
        stroke_width: u32,
        x2: f64,
        y2: f64,
        /// ADR-0018 D4 amend ① — stroke dash pattern. `None` = solid.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stroke_dash: Option<FigureStrokeDash>,
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
    /// ADR-0018 D10 amend ① — two-mode document item:
    ///   * (a) asset-based — `asset_id` is `Some(sha256)`, `content` is
    ///     `None`. The actual bytes live behind `/api/assets/<asset_id>`
    ///     (Stage 2, ADR-0030 to-be).
    ///   * (b) inline-stored — `asset_id` is `None`, `content` is
    ///     `Some(<utf-8 markdown>)` capped at [`DOCUMENT_INLINE_MAX_BYTES`].
    /// The two modes are *mutually exclusive*; `validate()` enforces this
    /// (`DocumentMissingSource` / `DocumentBothSources`).
    Document {
        #[serde(flatten)]
        common: ItemCommon,
        /// (a) asset-based mode: sha256 → `/api/assets/<asset_id>`.
        /// (b) inline-stored mode: `None`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asset_id: Option<String>,
        mime: String,
        file_name: String,
        /// (a) asset-based: real binary size.
        /// (b) inline-stored: `content.len()` bytes.
        size_bytes: u64,
        /// (b) inline-stored UTF-8 markdown, capped at
        /// [`DOCUMENT_INLINE_MAX_BYTES`]. (a) is `None`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    FilePath {
        #[serde(flatten)]
        common: ItemCommon,
        path: String,
        kind: Option<String>,
    },
    /// ADR-0018 D12 amend / ADR-0036 — Canvas component connector.
    ///
    /// Endpoint-bound wire connecting two other items. `x/y/w/h` on `common`
    /// is a *BBox cache* (not user input): `put_layout_handler` recomputes
    /// it from the two anchor points before validation. `from_id` / `to_id`
    /// must refer to non-connector items (refer-integrity + no chain), and
    /// `from_id != to_id` (self-loop reject, MVP — ADR-0036 O2).
    Connector {
        #[serde(flatten)]
        common: ItemCommon,
        from_id: String,
        to_id: String,
        from_anchor: Anchor,
        to_anchor: Anchor,
        direction: Direction,
        stroke: String,
        stroke_width: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stroke_dash: Option<StrokeDash>,
        head_from: Head,
        head_to: Head,
        routing: Routing,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        waypoints: Option<Vec<Point>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label_offset: Option<Point>,
    },
    /// ADR-0038 — Snippet collection. A canvas-local registry of (key, body)
    /// pairs. Click a badge ⇒ copy `body` to clipboard (FE-side action,
    /// no BE involvement). All edits round-trip through the standard
    /// `PUT /layout` endpoint.
    Snippets {
        #[serde(flatten)]
        common: ItemCommon,
        /// 0..[`SNIPPETS_ENTRIES_CAP`] entries. Order is preserved verbatim;
        /// FE renders badges in this order. Empty `Vec` is the *empty
        /// state* (FE shows just the "+ add" affordance).
        #[serde(default)]
        entries: Vec<SnippetEntry>,
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
            | Item::FilePath { common, .. }
            | Item::Connector { common, .. }
            | Item::Snippets { common, .. } => common,
        }
    }

    /// Mutable borrow of the common fields — used by
    /// [`recompute_connector_bboxes`] to rewrite the `x/y/w/h` cache on
    /// connector variants.
    fn common_mut(&mut self) -> &mut ItemCommon {
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
            | Item::FilePath { common, .. }
            | Item::Connector { common, .. }
            | Item::Snippets { common, .. } => common,
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
    /// ADR-0018 D10 amend ① — Document item carries neither an `asset_id`
    /// (asset-based mode) nor a `content` (inline-stored mode).
    #[error("document item must carry either asset_id or content")]
    DocumentMissingSource,
    /// ADR-0018 D10 amend ① — Document item carries *both* `asset_id` and
    /// `content`. The two modes are mutually exclusive.
    #[error("document item must not carry both asset_id and content")]
    DocumentBothSources,
    /// ADR-0018 D10 amend ① — inline-stored content exceeds the cap.
    #[error("document inline content exceeds {} bytes", DOCUMENT_INLINE_MAX_BYTES)]
    DocumentInlineTooLong,
    /// ADR-0018 D12 / ADR-0036 — connector `from_id` or `to_id` does not
    /// match any other item in this layout (refer-integrity violation).
    #[error("connector endpoint id not found in items[]")]
    ConnectorEndpointMissing,
    /// ADR-0036 Q2 — connector endpoint references another connector
    /// (chain reject, MVP).
    #[error("connector endpoint cannot reference another connector")]
    ConnectorInvalidEndpoint,
    /// ADR-0036 Q3 / O2 — self-loop (`from_id == to_id`) reject for MVP.
    #[error("connector from_id and to_id must differ (self-loop reject, MVP)")]
    ConnectorSelfLoop,
    /// ADR-0018 D4 amend ① (2026-05-20 batch 5) — figure stroke_width is
    /// out of the inspector-enforced 1..=32 range. Rect / Ellipse / Line.
    #[error("figure stroke_width {width} must be in 1..=32")]
    StrokeWidthOutOfRange { width: u32 },
    /// ADR-0018 D4 amend ② (2026-05-20 batch 5) — text font_size is out of
    /// the 8..=96 range.
    #[error("text font_size {font_size} must be in 8..=96")]
    TextFontSizeOutOfRange { font_size: u32 },
    /// ADR-0038 — `SnippetEntry::id` is not a UUID v4-shape lowercase string.
    #[error("snippet entry id is not a UUID v4-shape lowercase string: {0:?}")]
    BadSnippetEntryId(String),
    /// ADR-0038 — `SnippetEntry::key` is empty after `trim()`.
    #[error("snippet entry key must be non-empty (after trim)")]
    SnippetKeyEmpty,
    /// ADR-0038 — `SnippetEntry::key` exceeds [`SNIPPET_KEY_MAX_BYTES`].
    #[error("snippet entry key exceeds {} bytes", SNIPPET_KEY_MAX_BYTES)]
    SnippetKeyTooLong,
    /// ADR-0038 — `SnippetEntry::body` exceeds [`SNIPPET_BODY_MAX_BYTES`].
    #[error("snippet entry body exceeds {} bytes", SNIPPET_BODY_MAX_BYTES)]
    SnippetBodyTooLong,
    /// ADR-0038 — `Snippets::entries` length exceeds [`SNIPPETS_ENTRIES_CAP`].
    #[error("snippets entries length exceeds {}", SNIPPETS_ENTRIES_CAP)]
    SnippetsEntriesTooMany,
    /// ADR-0038 — duplicate `SnippetEntry::id` within a single `Snippets` item.
    #[error("duplicate snippet entry id within a single item: {0:?}")]
    DuplicateSnippetEntryId(String),
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
            Self::DocumentMissingSource => "document_missing_source",
            Self::DocumentBothSources => "document_both_sources",
            Self::DocumentInlineTooLong => "document_inline_too_long",
            Self::ConnectorEndpointMissing => "connector_endpoint_missing",
            Self::ConnectorInvalidEndpoint => "connector_invalid_endpoint",
            Self::ConnectorSelfLoop => "connector_self_loop",
            Self::StrokeWidthOutOfRange { .. } => "stroke_width_out_of_range",
            Self::TextFontSizeOutOfRange { .. } => "text_font_size_out_of_range",
            Self::BadSnippetEntryId(_) => "bad_snippet_entry_id",
            Self::SnippetKeyEmpty => "snippet_key_empty",
            Self::SnippetKeyTooLong => "snippet_key_too_long",
            Self::SnippetBodyTooLong => "snippet_body_too_long",
            Self::SnippetsEntriesTooMany => "snippets_entries_too_many",
            Self::DuplicateSnippetEntryId(_) => "duplicate_snippet_entry_id",
        }
    }
}

/// Shared text-payload byte cap check (ADR-0018 D8). Reused by `Item::Text`
/// and the embedded text on `Item::Rect` / `Item::Ellipse` (ADR-0040 D1).
fn check_text_cap(text: &str) -> Result<(), ValidationError> {
    if text.len() > TEXT_PAYLOAD_MAX_BYTES {
        return Err(ValidationError::TextTooLong);
    }
    Ok(())
}

/// Shared font-size range check (8..=96, ADR-0018 D4 amend ②). Reused by
/// `Item::Text` and the embedded text on `Item::Rect` / `Item::Ellipse`.
fn check_font_size(font_size: u32) -> Result<(), ValidationError> {
    if !(8..=96).contains(&font_size) {
        return Err(ValidationError::TextFontSizeOutOfRange { font_size });
    }
    Ok(())
}

/// Shared figure stroke-width range check (1..=32, ADR-0018 D4 amend ①).
/// Reused by Rect / Ellipse / Line and the text box on `Item::Text`
/// (ADR-0040 D1).
fn check_stroke_width(width: u32) -> Result<(), ValidationError> {
    if !(1..=32).contains(&width) {
        return Err(ValidationError::StrokeWidthOutOfRange { width });
    }
    Ok(())
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
    //
    // ADR-0036 Q1 — build an O(1) id → &Item index up front so connector
    // refer-integrity is O(C) instead of O(N × C). The index is also reused
    // by the connector arm to detect endpoint-points-to-connector chains
    // without re-scanning items[].
    let id_index: std::collections::HashMap<&str, &Item> = layout
        .items
        .iter()
        .map(|it| (it.common().id.as_str(), it))
        .collect();

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
            Item::Text {
                text,
                font_size,
                stroke_width,
                ..
            } => {
                check_text_cap(text)?;
                // ADR-0018 D4 amend ② — Inspector slider caps at 8..=96.
                check_font_size(*font_size)?;
                // ADR-0040 D1 — the text box stroke shares the figure band.
                check_stroke_width(*stroke_width)?;
            }
            // ADR-0018 D4 amend ① — figure stroke_width is bounded to the
            // inspector-enforced 1..=32 band. ADR-0040 D1 — Rect / Ellipse
            // now also carry embedded text, so they additionally reuse the
            // text byte cap + font_size range. Line stays stroke-only.
            Item::Rect {
                stroke_width,
                text,
                font_size,
                ..
            }
            | Item::Ellipse {
                stroke_width,
                text,
                font_size,
                ..
            } => {
                check_stroke_width(*stroke_width)?;
                check_text_cap(text)?;
                check_font_size(*font_size)?;
            }
            Item::Line { stroke_width, .. } => {
                check_stroke_width(*stroke_width)?;
            }
            Item::FreeDraw { points, .. } => {
                if points.len() > FREE_DRAW_POINT_CAP {
                    return Err(ValidationError::FreeDrawTooManyPoints);
                }
            }
            Item::Document {
                asset_id, content, ..
            } => {
                // ADR-0018 D10 amend ① — exactly one of (a) `asset_id` or
                // (b) `content` must be set. The two modes are mutually
                // exclusive; the `asset_id` form references a future
                // `/api/assets/<sha256>` (Stage 2), and the `content` form
                // inlines a small markdown payload in the layout JSON.
                match (asset_id.as_ref(), content.as_ref()) {
                    (None, None) => {
                        return Err(ValidationError::DocumentMissingSource);
                    }
                    (Some(_), Some(_)) => {
                        return Err(ValidationError::DocumentBothSources);
                    }
                    (None, Some(c)) => {
                        if c.len() > DOCUMENT_INLINE_MAX_BYTES {
                            return Err(ValidationError::DocumentInlineTooLong);
                        }
                    }
                    (Some(_), None) => {
                        // asset-based mode. The sha256 hex-string shape
                        // check lands alongside `/api/assets/*` ship —
                        // until then we accept any non-empty string so the
                        // existing on-disk records (asset_id strings the FE
                        // never validated either) keep loading.
                    }
                }
            }
            Item::Connector { from_id, to_id, .. } => {
                // ADR-0036 Q3 — self-loop reject (MVP scope).
                if from_id == to_id {
                    return Err(ValidationError::ConnectorSelfLoop);
                }
                // ADR-0036 Q2 — endpoint must exist and not be another
                // connector. `id_index` was built before this loop so this
                // is O(1).
                let from = id_index
                    .get(from_id.as_str())
                    .ok_or(ValidationError::ConnectorEndpointMissing)?;
                let to = id_index
                    .get(to_id.as_str())
                    .ok_or(ValidationError::ConnectorEndpointMissing)?;
                if matches!(from, Item::Connector { .. }) || matches!(to, Item::Connector { .. }) {
                    return Err(ValidationError::ConnectorInvalidEndpoint);
                }
            }
            Item::Snippets { entries, .. } => {
                // ADR-0038 — entries cap first (early exit for oversized
                // payloads), then per-entry: UUID → unique id (intra-item)
                // → key trim → key len → body len. fail-fast.
                if entries.len() > SNIPPETS_ENTRIES_CAP {
                    return Err(ValidationError::SnippetsEntriesTooMany);
                }
                let mut seen: std::collections::HashSet<&str> =
                    std::collections::HashSet::new();
                for e in entries {
                    if !is_uuid_shape(&e.id) {
                        return Err(ValidationError::BadSnippetEntryId(e.id.clone()));
                    }
                    if !seen.insert(&e.id) {
                        return Err(ValidationError::DuplicateSnippetEntryId(e.id.clone()));
                    }
                    if e.key.trim().is_empty() {
                        return Err(ValidationError::SnippetKeyEmpty);
                    }
                    if e.key.len() > SNIPPET_KEY_MAX_BYTES {
                        return Err(ValidationError::SnippetKeyTooLong);
                    }
                    if e.body.len() > SNIPPET_BODY_MAX_BYTES {
                        return Err(ValidationError::SnippetBodyTooLong);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Recompute the `x/y/w/h` BBox cache on every `Item::Connector` from its
/// two endpoint anchor points (ADR-0036 D1 / Q4-Q5). User-supplied
/// connector geometry is ignored — the BE is the single source of truth
/// for connector bounds so endpoint moves stay in sync without a FE
/// round-trip.
///
/// Two-pass to keep the borrow checker happy:
///   1. Build `endpoint_geom: HashMap<&str, (x, y, w, h)>` from the
///      non-connector items.
///   2. Walk connectors mutably and write the recomputed BBox.
///
/// Connectors whose endpoint is missing or also-a-connector are left
/// untouched — `validate()` will reject the layout shortly after.
pub fn recompute_connector_bboxes(layout: &mut Layout) {
    // Pass 1 — snapshot every non-connector item's geometry by id.
    let endpoint_geom: std::collections::HashMap<String, (f64, f64, f64, f64)> = layout
        .items
        .iter()
        .filter(|it| !matches!(it, Item::Connector { .. }))
        .map(|it| {
            let c = it.common();
            (c.id.clone(), (c.x, c.y, c.w, c.h))
        })
        .collect();

    // Pass 2 — walk connectors and rewrite the BBox cache.
    for it in layout.items.iter_mut() {
        let (from_id, to_id, from_anchor, to_anchor) = match it {
            Item::Connector {
                from_id,
                to_id,
                from_anchor,
                to_anchor,
                ..
            } => (from_id.clone(), to_id.clone(), *from_anchor, *to_anchor),
            _ => continue,
        };
        let Some(&from_geom) = endpoint_geom.get(&from_id) else {
            continue;
        };
        let Some(&to_geom) = endpoint_geom.get(&to_id) else {
            continue;
        };
        let (fx, fy) = anchor_point(from_geom, from_anchor);
        let (tx, ty) = anchor_point(to_geom, to_anchor);
        let x = fx.min(tx);
        let y = fy.min(ty);
        let w = (fx - tx).abs();
        let h = (fy - ty).abs();
        let common = it.common_mut();
        common.x = x;
        common.y = y;
        common.w = w;
        common.h = h;
    }
}

/// Resolve an item's anchor keyword to an absolute canvas point given the
/// item's bounding box (ADR-0036 D2).
fn anchor_point(geom: (f64, f64, f64, f64), anchor: Anchor) -> (f64, f64) {
    let (x, y, w, h) = geom;
    match anchor {
        Anchor::N => (x + w / 2.0, y),
        Anchor::NE => (x + w, y),
        Anchor::E => (x + w, y + h / 2.0),
        Anchor::SE => (x + w, y + h),
        Anchor::S => (x + w / 2.0, y + h),
        Anchor::SW => (x, y + h),
        Anchor::W => (x, y + h / 2.0),
        Anchor::NW => (x, y),
        Anchor::Center => (x + w / 2.0, y + h / 2.0),
    }
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
    obj.entry("viewport".to_string())
        .or_insert_with(|| serde_json::json!({ "x": 0.0, "y": 0.0, "zoom": 1.0 }));
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
                    text_align: TextAlign::Center,
                    text_vertical_align: TextVerticalAlign::Middle,
                    color: "#333".into(),
                    font_weight: FontWeight::Normal,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    font_family: FontFamily::Sans,
                    stroke: String::new(),
                    fill: String::new(),
                    stroke_width: 2,
                    fill_enabled: false,
                    stroke_enabled: false,
                    corner_rounded: false,
                    stroke_dash: None,
                    label_auto: None,
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
            text_align: TextAlign::Center,
            text_vertical_align: TextVerticalAlign::Middle,
            color: "#000".into(),
            font_weight: FontWeight::Normal,
            italic: false,
            underline: false,
            strikethrough: false,
            font_family: FontFamily::Sans,
            stroke: String::new(),
            fill: String::new(),
            stroke_width: 2,
            fill_enabled: false,
            stroke_enabled: false,
            corner_rounded: false,
            stroke_dash: None,
            label_auto: None,
        });
        assert_eq!(validate(&l), Err(ValidationError::TextTooLong));
    }

    #[test]
    fn free_draw_point_cap_enforced() {
        let mut l = Layout::empty();
        let points = (0..(FREE_DRAW_POINT_CAP + 1))
            .map(|i| Point {
                x: i as f64,
                y: 0.0,
            })
            .collect();
        l.items.push(Item::FreeDraw {
            common: item_common(UUID_A),
            stroke: "#000".into(),
            stroke_width: 1,
            points,
        });
        assert_eq!(validate(&l), Err(ValidationError::FreeDrawTooManyPoints));
    }

    // ── ADR-0018 D10 amend ① — Document inline-stored mode ──

    /// Inline-stored mode: `asset_id=None`, `content=Some(...)`. ADR's
    /// (b) branch. Must validate cleanly.
    #[test]
    fn document_inline_stored_validates() {
        let mut l = Layout::empty();
        l.items.push(Item::Document {
            common: item_common(UUID_A),
            asset_id: None,
            mime: "text/markdown".into(),
            file_name: "notes.md".into(),
            size_bytes: 5,
            content: Some("# Hi\n".into()),
        });
        assert_eq!(validate(&l), Ok(()));
    }

    /// Asset-based mode: `asset_id=Some(...)`, `content=None`. ADR's (a)
    /// branch. Must validate cleanly. The `asset_id` shape is intentionally
    /// not regex-checked yet — the `/api/assets/*` ship will add that
    /// alongside the binary endpoint (Stage 2, ADR-0030 to-be).
    #[test]
    fn document_asset_based_validates() {
        let mut l = Layout::empty();
        l.items.push(Item::Document {
            common: item_common(UUID_A),
            asset_id: Some("dead".repeat(16)), // placeholder 64-char hex-ish
            mime: "application/pdf".into(),
            file_name: "spec.pdf".into(),
            size_bytes: 12345,
            content: None,
        });
        assert_eq!(validate(&l), Ok(()));
    }

    /// Neither field set → ADR's "mode is undefined". The handler should
    /// surface this as a deterministic `document_missing_source` code so
    /// the FE can render a precise error message.
    #[test]
    fn document_missing_source_rejected() {
        let mut l = Layout::empty();
        l.items.push(Item::Document {
            common: item_common(UUID_A),
            asset_id: None,
            mime: "text/plain".into(),
            file_name: "ghost.txt".into(),
            size_bytes: 0,
            content: None,
        });
        assert_eq!(validate(&l), Err(ValidationError::DocumentMissingSource));
    }

    /// Both fields set → ADR's "mutually exclusive" rule is violated. The
    /// FE must pick exactly one mode for a given Document.
    #[test]
    fn document_both_sources_rejected() {
        let mut l = Layout::empty();
        l.items.push(Item::Document {
            common: item_common(UUID_A),
            asset_id: Some("abc".into()),
            mime: "text/markdown".into(),
            file_name: "conflicted.md".into(),
            size_bytes: 4,
            content: Some("body".into()),
        });
        assert_eq!(validate(&l), Err(ValidationError::DocumentBothSources));
    }

    /// Inline-stored content over [`DOCUMENT_INLINE_MAX_BYTES`] is
    /// rejected; the FE is expected to switch to asset-based mode at that
    /// scale (Stage 2 prerequisite).
    #[test]
    fn document_inline_cap_enforced() {
        let mut l = Layout::empty();
        l.items.push(Item::Document {
            common: item_common(UUID_A),
            asset_id: None,
            mime: "text/markdown".into(),
            file_name: "huge.md".into(),
            size_bytes: (DOCUMENT_INLINE_MAX_BYTES + 1) as u64,
            content: Some("a".repeat(DOCUMENT_INLINE_MAX_BYTES + 1)),
        });
        assert_eq!(validate(&l), Err(ValidationError::DocumentInlineTooLong));
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
        assert_eq!(
            detect_shape(&json!({ "schema_version": 2 })),
            SchemaShape::V2
        );
        assert_eq!(
            detect_shape(&json!({ "schema_version": 1 })),
            SchemaShape::V1
        );
        assert_eq!(
            detect_shape(&json!({ "schema_version": 0 })),
            SchemaShape::Unknown
        );
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

    // ── ADR-0018 D12 amend / ADR-0036 — Connector ──

    const UUID_C: &str = "c1c10000-0000-4111-8222-000000000004";

    fn rect_at(id: &str, x: f64, y: f64, w: f64, h: f64) -> Item {
        let mut c = item_common(id);
        c.x = x;
        c.y = y;
        c.w = w;
        c.h = h;
        Item::Rect {
            common: c,
            stroke: "#000".into(),
            fill: "#fff".into(),
            stroke_width: 1,
            fill_enabled: true,
            stroke_enabled: true,
            corner_rounded: false,
            stroke_dash: None,
            text: String::new(),
            font_size: 14,
            color: String::new(),
            text_align: TextAlign::Center,
            text_vertical_align: TextVerticalAlign::Middle,
            font_weight: FontWeight::Normal,
            italic: false,
            underline: false,
            strikethrough: false,
            font_family: FontFamily::Sans,
            label_auto: None,
        }
    }

    fn connector_between(
        id: &str,
        from_id: &str,
        to_id: &str,
        from_anchor: Anchor,
        to_anchor: Anchor,
    ) -> Item {
        Item::Connector {
            common: item_common(id),
            from_id: from_id.into(),
            to_id: to_id.into(),
            from_anchor,
            to_anchor,
            direction: Direction::Uni,
            stroke: "#0d99ff".into(),
            stroke_width: 2,
            stroke_dash: None,
            head_from: Head::None,
            head_to: Head::Arrow,
            routing: Routing::Straight,
            waypoints: None,
            label_offset: None,
        }
    }

    /// §D row 1 — Connector with two existing non-connector endpoints
    /// validates cleanly.
    #[test]
    fn connector_valid_endpoints_ok() {
        let l = Layout {
            schema_version: 2,
            groups: vec![],
            items: vec![
                rect_at(UUID_A, 0.0, 0.0, 100.0, 50.0),
                rect_at(UUID_B, 300.0, 200.0, 80.0, 40.0),
                connector_between(UUID_C, UUID_A, UUID_B, Anchor::E, Anchor::W),
            ],
            viewport: Viewport::default(),
        };
        assert_eq!(validate(&l), Ok(()));
    }

    /// §D row 2 — `from_id` does not appear in items[] → reject.
    #[test]
    fn connector_endpoint_missing_rejected() {
        let l = Layout {
            schema_version: 2,
            groups: vec![],
            items: vec![
                rect_at(UUID_A, 0.0, 0.0, 100.0, 50.0),
                // UUID_B is referenced but never defined.
                connector_between(UUID_C, UUID_A, UUID_B, Anchor::E, Anchor::W),
            ],
            viewport: Viewport::default(),
        };
        assert_eq!(validate(&l), Err(ValidationError::ConnectorEndpointMissing));
    }

    /// §D row 3 — `from_id == to_id` → reject (MVP self-loop).
    #[test]
    fn connector_self_loop_rejected() {
        let l = Layout {
            schema_version: 2,
            groups: vec![],
            items: vec![
                rect_at(UUID_A, 0.0, 0.0, 100.0, 50.0),
                connector_between(UUID_C, UUID_A, UUID_A, Anchor::N, Anchor::S),
            ],
            viewport: Viewport::default(),
        };
        assert_eq!(validate(&l), Err(ValidationError::ConnectorSelfLoop));
    }

    /// §D row 4 — connector endpoint points at another connector → reject
    /// (Q2 chain ban).
    #[test]
    fn connector_invalid_endpoint_rejected() {
        // First connector C1 ties UUID_A → UUID_B. A second connector C2
        // tries to attach UUID_A → C1, which is a connector → reject.
        let c1 = "11110000-0000-4111-8222-000000000005";
        let c2 = "22220000-0000-4111-8222-000000000006";
        let l = Layout {
            schema_version: 2,
            groups: vec![],
            items: vec![
                rect_at(UUID_A, 0.0, 0.0, 100.0, 50.0),
                rect_at(UUID_B, 200.0, 100.0, 80.0, 40.0),
                connector_between(c1, UUID_A, UUID_B, Anchor::E, Anchor::W),
                connector_between(c2, UUID_A, c1, Anchor::E, Anchor::W),
            ],
            viewport: Viewport::default(),
        };
        assert_eq!(validate(&l), Err(ValidationError::ConnectorInvalidEndpoint));
    }

    /// §D row 5 — user-supplied BBox is ignored; `recompute_connector_bboxes`
    /// rewrites `x/y/w/h` from the two anchor points. ADR-0036 Q4.
    #[test]
    fn connector_bbox_recomputed() {
        // Anchor A:E = (100, 25). Anchor B:W = (300, 220).
        // BBox = (100, 25, 200, 195).
        let mut l = Layout {
            schema_version: 2,
            groups: vec![],
            items: vec![
                rect_at(UUID_A, 0.0, 0.0, 100.0, 50.0),
                rect_at(UUID_B, 300.0, 200.0, 80.0, 40.0),
                {
                    // Build a connector with deliberately wrong x/y/w/h so
                    // we can prove the recompute overwrites them.
                    let mut c = connector_between(UUID_C, UUID_A, UUID_B, Anchor::E, Anchor::W);
                    let cc = c.common_mut();
                    cc.x = -9999.0;
                    cc.y = -9999.0;
                    cc.w = 1.0;
                    cc.h = 1.0;
                    c
                },
            ],
            viewport: Viewport::default(),
        };
        recompute_connector_bboxes(&mut l);
        let con = l
            .items
            .iter()
            .find(|it| matches!(it, Item::Connector { .. }))
            .unwrap();
        let cc = con.common();
        assert_eq!(cc.x, 100.0);
        assert_eq!(cc.y, 25.0);
        assert_eq!(cc.w, 200.0);
        assert_eq!(cc.h, 195.0);
        // validate must still pass after the recompute (endpoints exist,
        // no chain, no self-loop).
        assert_eq!(validate(&l), Ok(()));
    }

    /// §D row 6 — JSON example from ADR-0036 D1 deserializes and round-trips
    /// to an identical Value. Confirms enum rename mappings (Anchor's
    /// uppercase keywords, Direction lowercase, etc.).
    #[test]
    fn connector_serde_roundtrip() {
        let raw = json!({
            "schema_version": 2,
            "groups": [],
            "items": [
                {
                    "type": "rect",
                    "id": UUID_A, "parent_id": null,
                    "x": 0.0, "y": 0.0, "w": 100.0, "h": 50.0, "z": 0,
                    "visibility": "visible", "locked": false,
                    "label": "", "description": "", "minimized": false,
                    "stroke": "#000", "fill": "#fff", "stroke_width": 1,
                },
                {
                    "type": "rect",
                    "id": UUID_B, "parent_id": null,
                    "x": 300.0, "y": 200.0, "w": 80.0, "h": 40.0, "z": 0,
                    "visibility": "visible", "locked": false,
                    "label": "", "description": "", "minimized": false,
                    "stroke": "#000", "fill": "#fff", "stroke_width": 1,
                },
                {
                    "type": "connector",
                    "id": UUID_C, "parent_id": null,
                    "x": 100.0, "y": 25.0, "w": 200.0, "h": 195.0, "z": 12,
                    "visibility": "visible", "locked": false,
                    "label": "data flow", "description": "", "minimized": false,
                    "from_id": UUID_A, "to_id": UUID_B,
                    "from_anchor": "E", "to_anchor": "W",
                    "direction": "uni",
                    "stroke": "#0d99ff", "stroke_width": 2,
                    "head_from": "none", "head_to": "arrow",
                    "routing": "straight",
                },
            ],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        });
        let parsed: Layout = serde_json::from_value(raw.clone()).expect("parses");
        let v: Value = serde_json::to_value(&parsed).expect("serializes");
        // Re-parse the round-tripped form so we compare logical equality
        // (field ordering differences are fine).
        let reparsed: Layout = serde_json::from_value(v.clone()).expect("re-parses");
        assert_eq!(parsed, reparsed);
        // Spot-check the wire shape — connector keywords survive the trip.
        let conn = &v["items"][2];
        assert_eq!(conn["type"], "connector");
        assert_eq!(conn["from_anchor"], "E");
        assert_eq!(conn["to_anchor"], "W");
        assert_eq!(conn["direction"], "uni");
        assert_eq!(conn["head_to"], "arrow");
        assert_eq!(conn["routing"], "straight");
    }

    // ── ADR-0018 D4 amend ① — Rect / Ellipse / Line schema batch 5 ──

    /// Explicit values for every new field round-trip losslessly, and the
    /// `FigureStrokeDash::DashDot` wire form serialises as `"dash_dot"`
    /// (snake_case rename, distinct from the connector enum).
    #[test]
    fn rect_fill_stroke_enabled_round_trip() {
        let raw = json!({
            "type": "rect",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 100.0, "h": 100.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "stroke": "#000", "fill": "#fff", "stroke_width": 2,
            "fill_enabled": false, "stroke_enabled": true,
            "corner_rounded": true, "stroke_dash": "dash_dot",
        });
        let item: Item = serde_json::from_value(raw.clone()).unwrap();
        let Item::Rect {
            fill_enabled,
            stroke_enabled,
            corner_rounded,
            stroke_dash,
            ..
        } = &item
        else {
            panic!("expected Item::Rect");
        };
        assert!(!fill_enabled);
        assert!(stroke_enabled);
        assert!(corner_rounded);
        assert_eq!(*stroke_dash, Some(FigureStrokeDash::DashDot));
        let v = serde_json::to_value(&item).unwrap();
        assert_eq!(v["fill_enabled"], false);
        assert_eq!(v["stroke_enabled"], true);
        assert_eq!(v["corner_rounded"], true);
        assert_eq!(v["stroke_dash"], "dash_dot");
        let item2: Item = serde_json::from_value(v).unwrap();
        assert_eq!(item, item2);
    }

    /// A legacy rect record from before batch 5 — `fill_enabled`,
    /// `stroke_enabled`, `corner_rounded`, `stroke_dash` are all absent.
    /// `#[serde(default = "default_true")]` keeps the booleans `true` so
    /// existing layouts render the same way they always did.
    #[test]
    fn rect_old_layout_defaults_fill_stroke_enabled_true() {
        let raw = json!({
            "type": "rect",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 100.0, "h": 100.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "stroke": "#000", "fill": "#fff", "stroke_width": 2,
        });
        let item: Item = serde_json::from_value(raw).unwrap();
        let Item::Rect {
            fill_enabled,
            stroke_enabled,
            corner_rounded,
            stroke_dash,
            ..
        } = &item
        else {
            panic!("expected Item::Rect");
        };
        assert!(fill_enabled);
        assert!(stroke_enabled);
        assert!(!corner_rounded);
        assert_eq!(*stroke_dash, None);
    }

    /// Ellipse mirrors Rect for the enabled / dash fields (corner_rounded
    /// is rect-only).
    #[test]
    fn ellipse_fill_stroke_enabled_round_trip() {
        let raw = json!({
            "type": "ellipse",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 100.0, "h": 60.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "stroke": "#000", "fill": "#fff", "stroke_width": 3,
            "fill_enabled": false, "stroke_enabled": false,
            "stroke_dash": "dot",
        });
        let item: Item = serde_json::from_value(raw).unwrap();
        let Item::Ellipse {
            fill_enabled,
            stroke_enabled,
            stroke_dash,
            ..
        } = &item
        else {
            panic!("expected Item::Ellipse");
        };
        assert!(!fill_enabled);
        assert!(!stroke_enabled);
        assert_eq!(*stroke_dash, Some(FigureStrokeDash::Dot));
    }

    /// Line carries `stroke_dash` only (no fill side). `None` round-trips
    /// to a serialised form that omits the key entirely.
    #[test]
    fn line_stroke_dash_round_trip() {
        let raw = json!({
            "type": "line",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 100.0, "h": 0.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "stroke": "#000", "stroke_width": 4,
            "x2": 100.0, "y2": 0.0,
            "stroke_dash": "dash",
        });
        let item: Item = serde_json::from_value(raw).unwrap();
        let Item::Line { stroke_dash, .. } = &item else {
            panic!("expected Item::Line");
        };
        assert_eq!(*stroke_dash, Some(FigureStrokeDash::Dash));
        // None round-trips by skipping the field on serialise.
        let raw_solid = json!({
            "type": "line",
            "id": UUID_B, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 100.0, "h": 0.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "stroke": "#000", "stroke_width": 4,
            "x2": 100.0, "y2": 0.0,
        });
        let solid: Item = serde_json::from_value(raw_solid).unwrap();
        let v = serde_json::to_value(&solid).unwrap();
        assert!(
            v.get("stroke_dash").is_none(),
            "None must skip on serialise"
        );
    }

    /// `FigureStrokeDash` wire form is `snake_case`, not the connector
    /// enum's lowercase. The two enums are deliberately separate.
    #[test]
    fn figure_stroke_dash_snake_case_wire() {
        assert_eq!(
            serde_json::to_string(&FigureStrokeDash::Solid).unwrap(),
            "\"solid\""
        );
        assert_eq!(
            serde_json::to_string(&FigureStrokeDash::DashDot).unwrap(),
            "\"dash_dot\""
        );
        let parsed: FigureStrokeDash = serde_json::from_str("\"dash\"").unwrap();
        assert_eq!(parsed, FigureStrokeDash::Dash);
    }

    /// `stroke_width = 0` is rejected. Inspector slider caps at 1 so this
    /// only fires for direct PUTs that bypass the FE.
    #[test]
    fn figure_stroke_width_zero_rejected() {
        let mut l = Layout::empty();
        let mut c = item_common(UUID_A);
        c.w = 100.0;
        c.h = 50.0;
        l.items.push(Item::Rect {
            common: c,
            stroke: "#000".into(),
            fill: "#fff".into(),
            stroke_width: 0,
            fill_enabled: true,
            stroke_enabled: true,
            corner_rounded: false,
            stroke_dash: None,
            text: String::new(),
            font_size: 14,
            color: String::new(),
            text_align: TextAlign::Center,
            text_vertical_align: TextVerticalAlign::Middle,
            font_weight: FontWeight::Normal,
            italic: false,
            underline: false,
            strikethrough: false,
            font_family: FontFamily::Sans,
            label_auto: None,
        });
        let err = validate(&l).unwrap_err();
        assert_eq!(err.code(), "stroke_width_out_of_range");
    }

    /// `stroke_width > 32` is rejected. Inspector slider caps at 32.
    #[test]
    fn figure_stroke_width_over_32_rejected() {
        let mut l = Layout::empty();
        let mut c = item_common(UUID_A);
        c.w = 100.0;
        c.h = 50.0;
        l.items.push(Item::Ellipse {
            common: c,
            stroke: "#000".into(),
            fill: "#fff".into(),
            stroke_width: 33,
            fill_enabled: true,
            stroke_enabled: true,
            stroke_dash: None,
            text: String::new(),
            font_size: 14,
            color: String::new(),
            text_align: TextAlign::Center,
            text_vertical_align: TextVerticalAlign::Middle,
            font_weight: FontWeight::Normal,
            italic: false,
            underline: false,
            strikethrough: false,
            font_family: FontFamily::Sans,
            label_auto: None,
        });
        let err = validate(&l).unwrap_err();
        assert_eq!(err.code(), "stroke_width_out_of_range");
    }

    /// Boundary values 1 and 32 are accepted on Rect, Ellipse, and Line.
    #[test]
    fn figure_stroke_width_boundary_accepted() {
        let mut l = Layout::empty();
        l.items.push(Item::Rect {
            common: item_common(UUID_A),
            stroke: "#000".into(),
            fill: "#fff".into(),
            stroke_width: 1,
            fill_enabled: true,
            stroke_enabled: true,
            corner_rounded: false,
            stroke_dash: None,
            text: String::new(),
            font_size: 14,
            color: String::new(),
            text_align: TextAlign::Center,
            text_vertical_align: TextVerticalAlign::Middle,
            font_weight: FontWeight::Normal,
            italic: false,
            underline: false,
            strikethrough: false,
            font_family: FontFamily::Sans,
            label_auto: None,
        });
        l.items.push(Item::Line {
            common: item_common(UUID_B),
            stroke: "#000".into(),
            stroke_width: 32,
            x2: 100.0,
            y2: 0.0,
            stroke_dash: None,
        });
        assert_eq!(validate(&l), Ok(()));
    }

    // ── ADR-0018 D4 amend ② — Text full-style schema batch 5 ──

    /// Explicit values for every new field round-trip losslessly. Note that
    /// `italic`, `underline`, `strikethrough` are plain `bool` (not
    /// `Option<bool>`) so they always appear in the wire form.
    #[test]
    fn text_full_style_round_trip() {
        let raw = json!({
            "type": "text",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 160.0, "h": 56.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "text": "Hello", "font_size": 16, "color": "#333",
            "font_weight": "bold",
            "italic": true, "underline": true, "strikethrough": false,
        });
        let item: Item = serde_json::from_value(raw).unwrap();
        let Item::Text {
            font_weight,
            italic,
            underline,
            strikethrough,
            ..
        } = &item
        else {
            panic!("expected Item::Text");
        };
        assert_eq!(*font_weight, FontWeight::Bold);
        assert!(*italic);
        assert!(*underline);
        assert!(!*strikethrough);
        let v = serde_json::to_value(&item).unwrap();
        assert_eq!(v["font_weight"], "bold");
        assert_eq!(v["italic"], true);
        assert_eq!(v["underline"], true);
        assert_eq!(v["strikethrough"], false);
    }

    /// Legacy text record without any of the batch-5 fields. `font_weight`
    /// defaults to `Normal`; the three booleans default to `false`.
    #[test]
    fn text_old_layout_no_decorations() {
        let raw = json!({
            "type": "text",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 160.0, "h": 56.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "text": "Hello", "font_size": 16, "color": "#333",
        });
        let item: Item = serde_json::from_value(raw).unwrap();
        let Item::Text {
            font_weight,
            italic,
            underline,
            strikethrough,
            ..
        } = &item
        else {
            panic!("expected Item::Text");
        };
        assert_eq!(*font_weight, FontWeight::Normal);
        assert!(!italic);
        assert!(!underline);
        assert!(!strikethrough);
    }

    /// `font_size = 7` is rejected. Inspector slider caps at 8.
    #[test]
    fn text_font_size_under_8_rejected() {
        let mut l = Layout::empty();
        l.items.push(Item::Text {
            common: item_common(UUID_A),
            text: "Hi".into(),
            font_size: 7,
            text_align: TextAlign::Center,
            text_vertical_align: TextVerticalAlign::Middle,
            color: "#000".into(),
            font_weight: FontWeight::Normal,
            italic: false,
            underline: false,
            strikethrough: false,
            font_family: FontFamily::Sans,
            stroke: String::new(),
            fill: String::new(),
            stroke_width: 2,
            fill_enabled: false,
            stroke_enabled: false,
            corner_rounded: false,
            stroke_dash: None,
            label_auto: None,
        });
        let err = validate(&l).unwrap_err();
        assert_eq!(err.code(), "text_font_size_out_of_range");
    }

    /// `font_size = 97` is rejected. Inspector slider caps at 96.
    #[test]
    fn text_font_size_over_96_rejected() {
        let mut l = Layout::empty();
        l.items.push(Item::Text {
            common: item_common(UUID_A),
            text: "Hi".into(),
            font_size: 97,
            text_align: TextAlign::Center,
            text_vertical_align: TextVerticalAlign::Middle,
            color: "#000".into(),
            font_weight: FontWeight::Normal,
            italic: false,
            underline: false,
            strikethrough: false,
            font_family: FontFamily::Sans,
            stroke: String::new(),
            fill: String::new(),
            stroke_width: 2,
            fill_enabled: false,
            stroke_enabled: false,
            corner_rounded: false,
            stroke_dash: None,
            label_auto: None,
        });
        let err = validate(&l).unwrap_err();
        assert_eq!(err.code(), "text_font_size_out_of_range");
    }

    // ── ADR-0040 / ADR-0041 — box-on-text · text-on-figure · font family ──

    /// A1 — text + full BoxStyle round-trips losslessly, including the
    /// `dash_dot` snake_case wire and the `mono` font family.
    #[test]
    fn text_with_box_round_trip() {
        let raw = json!({
            "type": "text",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 160.0, "h": 56.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "text": "boxed", "font_size": 16, "color": "#333",
            "font_family": "mono",
            "stroke": "#000", "fill": "#eee", "stroke_width": 3,
            "fill_enabled": true, "stroke_enabled": true,
            "corner_rounded": true, "stroke_dash": "dash_dot",
        });
        let item: Item = serde_json::from_value(raw).unwrap();
        let Item::Text {
            font_family,
            stroke,
            fill,
            stroke_width,
            fill_enabled,
            stroke_enabled,
            corner_rounded,
            stroke_dash,
            ..
        } = &item
        else {
            panic!("expected Item::Text");
        };
        assert_eq!(*font_family, FontFamily::Mono);
        assert_eq!(stroke, "#000");
        assert_eq!(fill, "#eee");
        assert_eq!(*stroke_width, 3);
        assert!(fill_enabled);
        assert!(stroke_enabled);
        assert!(corner_rounded);
        assert_eq!(*stroke_dash, Some(FigureStrokeDash::DashDot));
        let v = serde_json::to_value(&item).unwrap();
        let item2: Item = serde_json::from_value(v).unwrap();
        assert_eq!(item, item2);
    }

    /// A2 — a legacy text record (no box fields) defaults the box OFF.
    /// ADR-0040 D2: text's `fill_enabled` / `stroke_enabled` default `false`
    /// (the opposite of Rect/Ellipse) so old text renders box-less.
    #[test]
    fn text_old_layout_no_box() {
        let raw = json!({
            "type": "text",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 160.0, "h": 56.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "text": "Hello", "font_size": 16, "color": "#333",
        });
        let item: Item = serde_json::from_value(raw).unwrap();
        let Item::Text {
            fill_enabled,
            stroke_enabled,
            corner_rounded,
            stroke_width,
            font_family,
            stroke_dash,
            ..
        } = &item
        else {
            panic!("expected Item::Text");
        };
        assert!(!fill_enabled);
        assert!(!stroke_enabled);
        assert!(!corner_rounded);
        assert_eq!(*stroke_width, 2);
        assert_eq!(*font_family, FontFamily::Sans);
        assert_eq!(*stroke_dash, None);
    }

    /// A3 — rect + embedded TextStyle round-trips, including align / weight /
    /// font family.
    #[test]
    fn rect_with_embedded_text_round_trip() {
        let raw = json!({
            "type": "rect",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 120.0, "h": 80.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "stroke": "#000", "fill": "#fff", "stroke_width": 2,
            "text": "label", "font_size": 18, "color": "#111",
            "text_align": "left", "text_vertical_align": "top",
            "font_weight": "bold", "italic": true,
            "font_family": "serif",
        });
        let item: Item = serde_json::from_value(raw).unwrap();
        let Item::Rect {
            text,
            font_size,
            color,
            text_align,
            text_vertical_align,
            font_weight,
            italic,
            font_family,
            ..
        } = &item
        else {
            panic!("expected Item::Rect");
        };
        assert_eq!(text, "label");
        assert_eq!(*font_size, 18);
        assert_eq!(color, "#111");
        assert_eq!(*text_align, TextAlign::Left);
        assert_eq!(*text_vertical_align, TextVerticalAlign::Top);
        assert_eq!(*font_weight, FontWeight::Bold);
        assert!(italic);
        assert_eq!(*font_family, FontFamily::Serif);
        let v = serde_json::to_value(&item).unwrap();
        let item2: Item = serde_json::from_value(v).unwrap();
        assert_eq!(item, item2);
    }

    /// A4 — a legacy rect (no embedded-text fields) defaults `text=""`,
    /// `font_size=14`, `font_family=Sans`, `text_align=Center`.
    #[test]
    fn rect_old_layout_empty_text() {
        let raw = json!({
            "type": "rect",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 120.0, "h": 80.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "stroke": "#000", "fill": "#fff", "stroke_width": 2,
        });
        let item: Item = serde_json::from_value(raw).unwrap();
        let Item::Rect {
            text,
            font_size,
            font_family,
            text_align,
            ..
        } = &item
        else {
            panic!("expected Item::Rect");
        };
        assert_eq!(text, "");
        assert_eq!(*font_size, 14);
        assert_eq!(*font_family, FontFamily::Sans);
        assert_eq!(*text_align, TextAlign::Center);
    }

    /// A5 — ellipse + embedded TextStyle round-trips (mirrors rect; ellipse
    /// has no `corner_rounded`).
    #[test]
    fn ellipse_with_embedded_text_round_trip() {
        let raw = json!({
            "type": "ellipse",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 120.0, "h": 80.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "stroke": "#000", "fill": "#fff", "stroke_width": 2,
            "text": "oval", "font_size": 20, "color": "#222",
            "text_align": "right", "text_vertical_align": "bottom",
            "font_weight": "light", "underline": true, "strikethrough": true,
            "font_family": "mono",
        });
        let item: Item = serde_json::from_value(raw).unwrap();
        let Item::Ellipse {
            text,
            font_size,
            text_vertical_align,
            font_weight,
            underline,
            strikethrough,
            font_family,
            ..
        } = &item
        else {
            panic!("expected Item::Ellipse");
        };
        assert_eq!(text, "oval");
        assert_eq!(*font_size, 20);
        assert_eq!(*text_vertical_align, TextVerticalAlign::Bottom);
        assert_eq!(*font_weight, FontWeight::Light);
        assert!(underline);
        assert!(strikethrough);
        assert_eq!(*font_family, FontFamily::Mono);
        let v = serde_json::to_value(&item).unwrap();
        let item2: Item = serde_json::from_value(v).unwrap();
        assert_eq!(item, item2);
    }

    /// B1 — `FontFamily` wire form is the lowercase keyword (snake_case is
    /// identical for these single-word variants).
    #[test]
    fn font_family_snake_case_wire() {
        assert_eq!(serde_json::to_string(&FontFamily::Sans).unwrap(), "\"sans\"");
        assert_eq!(
            serde_json::to_string(&FontFamily::Serif).unwrap(),
            "\"serif\""
        );
        assert_eq!(serde_json::to_string(&FontFamily::Mono).unwrap(), "\"mono\"");
        let parsed: FontFamily = serde_json::from_str("\"serif\"").unwrap();
        assert_eq!(parsed, FontFamily::Serif);
    }

    /// B2 — a missing `font_family` field defaults to `Sans`.
    #[test]
    fn font_family_default_sans() {
        assert_eq!(FontFamily::default(), FontFamily::Sans);
    }

    /// B3 — an unknown `font_family` variant is a strict deserialize error
    /// (no `#[serde(other)]` fallback).
    #[test]
    fn font_family_unknown_rejected() {
        let raw = json!({
            "type": "text",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 160.0, "h": 56.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "text": "Hi", "font_size": 16, "color": "#333",
            "font_family": "comic",
        });
        let parsed: Result<Item, _> = serde_json::from_value(raw);
        assert!(parsed.is_err(), "unknown font_family must reject");
    }

    /// C1 — embedded text on a rect reuses the 8..=96 font_size check.
    #[test]
    fn embedded_text_font_size_over_96_rejected() {
        let l: Layout = serde_json::from_value(json!({
            "schema_version": 2, "groups": [],
            "items": [{
                "type": "rect",
                "id": UUID_A, "parent_id": null,
                "x": 0.0, "y": 0.0, "w": 120.0, "h": 80.0, "z": 0,
                "visibility": "visible", "locked": false,
                "label": "", "description": "", "minimized": false,
                "stroke": "#000", "fill": "#fff", "stroke_width": 2,
                "text": "x", "font_size": 97,
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        }))
        .unwrap();
        let err = validate(&l).unwrap_err();
        assert_eq!(err.code(), "text_font_size_out_of_range");
    }

    /// C2 — embedded text on a rect reuses the 64 KiB text byte cap.
    #[test]
    fn embedded_text_64kb_cap_enforced() {
        let big = "x".repeat(TEXT_PAYLOAD_MAX_BYTES + 1);
        let l: Layout = serde_json::from_value(json!({
            "schema_version": 2, "groups": [],
            "items": [{
                "type": "rect",
                "id": UUID_A, "parent_id": null,
                "x": 0.0, "y": 0.0, "w": 120.0, "h": 80.0, "z": 0,
                "visibility": "visible", "locked": false,
                "label": "", "description": "", "minimized": false,
                "stroke": "#000", "fill": "#fff", "stroke_width": 2,
                "text": big, "font_size": 14,
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        }))
        .unwrap();
        assert_eq!(validate(&l), Err(ValidationError::TextTooLong));
    }

    /// C3 — the text box stroke reuses the figure 1..=32 stroke_width band.
    #[test]
    fn text_box_stroke_width_over_32_rejected() {
        let l: Layout = serde_json::from_value(json!({
            "schema_version": 2, "groups": [],
            "items": [{
                "type": "text",
                "id": UUID_A, "parent_id": null,
                "x": 0.0, "y": 0.0, "w": 160.0, "h": 56.0, "z": 0,
                "visibility": "visible", "locked": false,
                "label": "", "description": "", "minimized": false,
                "text": "Hi", "font_size": 16, "color": "#333",
                "stroke_width": 33,
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
        }))
        .unwrap();
        let err = validate(&l).unwrap_err();
        assert_eq!(err.code(), "stroke_width_out_of_range");
    }

    /// D1 — `label_auto` round-trips for Some(true)/Some(false) and is
    /// omitted from the wire form when `None`.
    #[test]
    fn label_auto_round_trip() {
        let item_true: Item = serde_json::from_value(json!({
            "type": "text",
            "id": UUID_A, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 160.0, "h": 56.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "text": "Hi", "font_size": 16, "color": "#333",
            "label_auto": true,
        }))
        .unwrap();
        let Item::Text { label_auto, .. } = &item_true else {
            panic!("expected Item::Text");
        };
        assert_eq!(*label_auto, Some(true));
        let v = serde_json::to_value(&item_true).unwrap();
        assert_eq!(v["label_auto"], true);

        let item_false: Item = serde_json::from_value(json!({
            "type": "text",
            "id": UUID_B, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 160.0, "h": 56.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "text": "Hi", "font_size": 16, "color": "#333",
            "label_auto": false,
        }))
        .unwrap();
        let Item::Text { label_auto, .. } = &item_false else {
            panic!("expected Item::Text");
        };
        assert_eq!(*label_auto, Some(false));

        let item_none: Item = serde_json::from_value(json!({
            "type": "text",
            "id": UUID_B, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 160.0, "h": 56.0, "z": 0,
            "visibility": "visible", "locked": false,
            "label": "", "description": "", "minimized": false,
            "text": "Hi", "font_size": 16, "color": "#333",
        }))
        .unwrap();
        let Item::Text { label_auto, .. } = &item_none else {
            panic!("expected Item::Text");
        };
        assert_eq!(*label_auto, None);
        let v_none = serde_json::to_value(&item_none).unwrap();
        assert!(
            v_none.get("label_auto").is_none(),
            "None must skip on serialise"
        );
    }

    // ── ADR-0038 — Snippets variant ────────────────────────────────────────

    const SNIPPET_ID_1: &str = "00000000-0000-4000-8000-000000000001";
    const SNIPPET_ID_2: &str = "00000000-0000-4000-8000-000000000002";

    #[test]
    fn snippets_round_trips_via_json() {
        let body = json!({
            "schema_version": 2,
            "groups": [],
            "items": [{
                "id": UUID_A,
                "type": "snippets",
                "parent_id": null,
                "x": 0.0, "y": 0.0, "w": 320.0, "h": 140.0, "z": 0,
                "visibility": "visible", "locked": false,
                "entries": [
                    { "id": SNIPPET_ID_1, "key": "gs", "body": "git status" },
                    { "id": SNIPPET_ID_2, "key": "deploy", "body": "pnpm build && rsync" }
                ]
            }],
            "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
        });
        let parsed: Layout = serde_json::from_value(body).unwrap();
        assert_eq!(parsed.items.len(), 1);
        match &parsed.items[0] {
            Item::Snippets { entries, .. } => assert_eq!(entries.len(), 2),
            _ => panic!("expected Snippets"),
        }
        let re = serde_json::to_value(&parsed).unwrap();
        let re_parsed: Layout = serde_json::from_value(re).unwrap();
        assert_eq!(parsed, re_parsed);
    }

    #[test]
    fn snippets_empty_entries_validates() {
        let mut l = Layout::empty();
        l.items.push(Item::Snippets {
            common: item_common(UUID_A),
            entries: vec![],
        });
        assert!(validate(&l).is_ok());
    }

    #[test]
    fn snippets_rejects_empty_key() {
        let mut l = Layout::empty();
        l.items.push(Item::Snippets {
            common: item_common(UUID_A),
            entries: vec![SnippetEntry {
                id: SNIPPET_ID_1.to_string(),
                key: "   ".to_string(),
                body: String::new(),
            }],
        });
        assert_eq!(validate(&l), Err(ValidationError::SnippetKeyEmpty));
    }

    #[test]
    fn snippets_rejects_too_many_entries() {
        let mut l = Layout::empty();
        let entries: Vec<SnippetEntry> = (0..(SNIPPETS_ENTRIES_CAP + 1))
            .map(|i| SnippetEntry {
                id: format!("00000000-0000-4000-8000-{:012x}", i + 1),
                key: format!("k{i}"),
                body: String::new(),
            })
            .collect();
        l.items.push(Item::Snippets {
            common: item_common(UUID_A),
            entries,
        });
        assert_eq!(
            validate(&l),
            Err(ValidationError::SnippetsEntriesTooMany)
        );
    }

    #[test]
    fn snippets_rejects_duplicate_entry_id() {
        let dup = SNIPPET_ID_1;
        let mut l = Layout::empty();
        l.items.push(Item::Snippets {
            common: item_common(UUID_A),
            entries: vec![
                SnippetEntry {
                    id: dup.to_string(),
                    key: "a".to_string(),
                    body: String::new(),
                },
                SnippetEntry {
                    id: dup.to_string(),
                    key: "b".to_string(),
                    body: String::new(),
                },
            ],
        });
        assert_eq!(
            validate(&l),
            Err(ValidationError::DuplicateSnippetEntryId(dup.to_string()))
        );
    }

    #[test]
    fn snippets_rejects_oversized_body() {
        let mut l = Layout::empty();
        l.items.push(Item::Snippets {
            common: item_common(UUID_A),
            entries: vec![SnippetEntry {
                id: SNIPPET_ID_1.to_string(),
                key: "big".to_string(),
                body: "x".repeat(SNIPPET_BODY_MAX_BYTES + 1),
            }],
        });
        assert_eq!(validate(&l), Err(ValidationError::SnippetBodyTooLong));
    }

    #[test]
    fn snippets_rejects_bad_entry_id() {
        let mut l = Layout::empty();
        l.items.push(Item::Snippets {
            common: item_common(UUID_A),
            entries: vec![SnippetEntry {
                id: "not-a-uuid".to_string(),
                key: "ok".to_string(),
                body: String::new(),
            }],
        });
        let err = validate(&l).unwrap_err();
        assert_eq!(err.code(), "bad_snippet_entry_id");
    }
}
