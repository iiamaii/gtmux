//! Batch layout ops — pure transformation core for
//! `POST /api/sessions/{name}/layout/ops` (ADR-0053 D5/D10/D13).
//!
//! Source-of-truth:
//! - ADR-0053 D5 (op enum, atomicity, pipeline), D10 (edit/create/delete
//!   semantics), D13 (group/reparent semantics), D6 (locked policy)
//! - ADR-0024 (z 4-action semantics — port of FE `zSpace.ts`)
//! - ADR-0010 D12/D14 (group create/ungroup — port of FE
//!   `sessionStore.svelte.ts` group helpers)
//! - ADR-0018 D7 (new item z = max+1), FE `itemFactory.ts` (type default
//!   sizes/payloads — the constants here mirror those verbatim)
//!
//! This module is *pure*: every function transforms a [`Layout`] in memory
//! and never touches disk, the terminal pool, or the WS hub. The HTTP
//! handler (`sessions::layout_ops_handler`) owns I/O, locking, the
//! degrade→recompute→validate pipeline, and side effects (kill/publish).

// Wire enum variants/fields mirror ADR-0053 D5 — per-field rustdoc would
// duplicate the ADR table (same policy as `schema.rs`).
#![allow(missing_docs)]

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::schema::{Group, Item, Layout, Viewport, Visibility};

// ─────────────────────────────────────────────────────────────────────────────
//  Wire types
// ─────────────────────────────────────────────────────────────────────────────

/// `POST /api/sessions/{name}/layout/ops` request body.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutOpsRequest {
    pub ops: Vec<LayoutOp>,
}

/// One layout op (ADR-0053 D5). Wire tag is `op`, snake_case (`raise_top`,
/// `group_create`, …). Terminal lifecycle ops (`spawn`/`mount` — ADR-0053
/// D11) mutate the layout here; their side effects (PTY spawn, alive-pool
/// pre-flight) live in the handler. An unknown `op` value is a serde error
/// mapped to 400 `bad_request` by the handler.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum LayoutOp {
    Move {
        id: String,
        x: f64,
        y: f64,
        #[serde(default)]
        force: bool,
    },
    Resize {
        id: String,
        w: f64,
        h: f64,
        #[serde(default)]
        force: bool,
    },
    Show {
        id: String,
        #[serde(default)]
        force: bool,
    },
    Hide {
        id: String,
        #[serde(default)]
        force: bool,
    },
    Minimize {
        id: String,
        #[serde(default)]
        force: bool,
    },
    Restore {
        id: String,
        #[serde(default)]
        force: bool,
    },
    Label {
        id: String,
        /// `null` clears the label (back to the empty default).
        label: Option<String>,
        #[serde(default)]
        force: bool,
    },
    Raise {
        id: String,
        #[serde(default)]
        force: bool,
    },
    RaiseTop {
        id: String,
        #[serde(default)]
        force: bool,
    },
    Lower {
        id: String,
        #[serde(default)]
        force: bool,
    },
    LowerBottom {
        id: String,
        #[serde(default)]
        force: bool,
    },
    Edit {
        id: String,
        /// Partial payload merged into the current item at the JSON level
        /// (ADR-0053 D10). `type`/`id` are immutable; of the common fields
        /// only `description` is editable here (geometry/visibility/lock/
        /// label/z all have dedicated ops).
        fields: Value,
        #[serde(default)]
        force: bool,
    },
    Create {
        /// Item type keyword (`text`, `note`, `rect`, …). `terminal` is
        /// rejected — terminal creation is the Batch B `spawn` op (D11).
        item_type: String,
        x: Option<f64>,
        y: Option<f64>,
        w: Option<f64>,
        h: Option<f64>,
        /// Type-specific payload overrides (same keys as the wire item).
        #[serde(default)]
        fields: Option<Value>,
    },
    Delete {
        id: String,
        /// ADR-0053 D10 — `true` also SIGTERMs the backing terminal
        /// ("Panel + Terminal" parity). Default `false` = panel-only.
        #[serde(default)]
        kill_terminal: bool,
        #[serde(default)]
        force: bool,
    },
    Spawn {
        /// Placement overrides — same default rule as `create` (ADR-0053
        /// D11 / 잔여 확인 3: stored-viewport center + terminal default
        /// size). The terminal UUID (= item id) is server-issued and
        /// returned via `created_ids`; the handler PTY-spawns it after the
        /// layout commit (headless-complete — no browser required).
        x: Option<f64>,
        y: Option<f64>,
        w: Option<f64>,
        h: Option<f64>,
    },
    Mount {
        /// UUID of an *alive pool terminal* (handler pre-flights against
        /// the terminal map — dead/unknown → 400 `terminal_not_alive`).
        /// Adds a TerminalItem referencing it, no spawn (ADR-0053 D11 —
        /// web `attachToCanvas` parity).
        uuid: String,
        x: Option<f64>,
        y: Option<f64>,
        w: Option<f64>,
        h: Option<f64>,
    },
    GroupCreate {
        ids: Vec<String>,
        label: Option<String>,
    },
    Ungroup {
        group_id: String,
        #[serde(default)]
        force: bool,
    },
    Reparent {
        id: String,
        /// Target group id, or `null` for canvas root. Cycles are caught by
        /// `schema::validate` (GroupCycle) in the pipeline (ADR-0053 D13).
        parent_id: Option<String>,
        #[serde(default)]
        force: bool,
    },
}

/// Per-op failure. `locked: true` maps to HTTP 409 (ADR-0053 D6), everything
/// else to 400. The whole batch is rejected on the first failure (D5).
#[derive(Debug)]
pub struct OpError {
    pub code: &'static str,
    pub message: String,
    pub locked: bool,
}

impl OpError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            locked: false,
        }
    }

    fn locked(id: &str) -> Self {
        Self {
            code: "locked",
            message: format!("target {id:?} is locked (retry with force:true)"),
            locked: true,
        }
    }

    fn not_found(id: &str) -> Self {
        Self::new("item_not_found", format!("target {id:?} is not in the layout"))
    }
}

/// Result of a successful batch application.
#[derive(Debug, Default)]
pub struct ApplyOutcome {
    /// Server-issued ids in op order (`create` item ids + `group_create`
    /// group ids — ADR-0053 D5 `created_ids`).
    pub created_ids: Vec<String>,
    /// Terminal UUIDs whose items were deleted with `kill_terminal: true`.
    /// The handler SIGTERMs + forgets these after the layout commit.
    pub kill_terminal_uuids: Vec<String>,
    /// Fresh terminal UUIDs minted by `spawn` ops (ADR-0053 D11). Also
    /// present in `created_ids`; the handler PTY-spawns these (with D4 env
    /// injection) after the layout commit.
    pub spawned_terminal_uuids: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
//  FE parity constants (itemFactory.ts / ItemInfoView.svelte)
// ─────────────────────────────────────────────────────────────────────────────

/// ADR-0053 D10 — default placement centers on the *stored* viewport. The
/// server has no client screen dimensions, so the screen center is
/// approximated with a nominal FHD window; the resulting canvas point is
/// `(nominal/2 - viewport.xy) / zoom` (SvelteFlow screen→flow transform).
const NOMINAL_VIEWPORT_W: f64 = 1920.0;
const NOMINAL_VIEWPORT_H: f64 = 1080.0;

/// FE `itemFactory.ts` line defaults: second endpoint at `+ (240, 80)`,
/// hit box padded by 8 on every side.
const LINE_DEFAULT_DX: f64 = 240.0;
const LINE_DEFAULT_DY: f64 = 80.0;
const LINE_HIT_PADDING: f64 = 8.0;
/// FE `itemFactory.ts` path default second endpoint delta.
const PATH_DEFAULT_DX: f64 = 240.0;
const PATH_DEFAULT_DY: f64 = 80.0;
/// FE `itemFactory.ts` `DEFAULT_TERMINAL_SIZE` — spawn/mount default panel
/// size (ADR-0053 D11, placement rule = create's).
const TERMINAL_DEFAULT_W: f64 = 480.0;
const TERMINAL_DEFAULT_H: f64 = 320.0;
/// FE `itemFactory.ts` free-draw bbox padding.
const FREE_DRAW_PADDING: f64 = 8.0;

// Minimize collapse / restore fallback geometry — mirrors
// `ItemInfoView.svelte::applyMinimizeGeom` + node-side constants.
// (`restored_geom` — ADR-0018 D11 — is still Draft and absent from the BE
// schema, so restore falls back to the FE default sizes; the FE keeps its
// own in-memory backup map for interactive restores.)
const NOTE_CHIP: f64 = 32.0;
const NOTE_RESTORE_W: f64 = 240.0;
const NOTE_RESTORE_H: f64 = 96.0;
const DOC_STRIP_H: f64 = 35.0;
const DOC_RESTORE_W: f64 = 360.0;
const DOC_RESTORE_H: f64 = 220.0;
const SNIP_STRIP_H: f64 = 35.0;
const SNIP_RESTORE_W: f64 = 320.0;
const SNIP_RESTORE_H: f64 = 150.0;
/// FE `MINIMIZED_TERMINAL_PANEL_HEIGHT` (types/canvas.ts).
const PANEL_STRIP_H: f64 = 35.0;
const PANEL_RESTORE_H: f64 = 220.0;

/// Type default sizes — FE `itemFactory.ts` constants, verbatim.
fn default_size(item_type: &str) -> (f64, f64) {
    match item_type {
        "text" => (160.0, 56.0),
        "note" => (300.0, 96.0),
        "file_path" => (320.0, 80.0),
        "rect" | "ellipse" | "free_draw" => (200.0, 140.0),
        // Line: |dx|,|dy| = (240, 80) plus 8px hit padding per side.
        "line" => (
            LINE_DEFAULT_DX + 2.0 * LINE_HIT_PADDING,
            LINE_DEFAULT_DY + 2.0 * LINE_HIT_PADDING,
        ),
        "image" => (320.0, 240.0),
        "document" => (360.0, 280.0),
        "snippets" => (320.0, 150.0),
        // Path x/y/w/h is a bbox cache — recomputed by the pipeline.
        "path" => (1.0, 1.0),
        _ => (200.0, 140.0),
    }
}

const KNOWN_CREATE_TYPES: &[&str] = &[
    "text",
    "note",
    "rect",
    "ellipse",
    "line",
    "free_draw",
    "image",
    "document",
    "file_path",
    "path",
    "snippets",
];

// ─────────────────────────────────────────────────────────────────────────────
//  Small lookup helpers
// ─────────────────────────────────────────────────────────────────────────────

fn item_idx(layout: &Layout, id: &str) -> Option<usize> {
    layout.items.iter().position(|it| it.common().id == id)
}

fn group_idx(layout: &Layout, id: &str) -> Option<usize> {
    layout.groups.iter().position(|g| g.id == id)
}

/// UUID v4 mint — reuses the ring-based generator boot uses for `server_id`
/// (same canonical lowercase 8-4-4-4-12 shape `schema::is_uuid_shape`
/// accepts; the `uuid` crate stays out of the dependency tree).
fn fresh_uuid() -> String {
    crate::session_lock::fresh_server_id()
}

fn viewport_center(v: &Viewport) -> (f64, f64) {
    let zoom = if v.zoom.is_finite() && v.zoom > 0.0 {
        v.zoom
    } else {
        1.0
    };
    (
        (NOMINAL_VIEWPORT_W / 2.0 - v.x) / zoom,
        (NOMINAL_VIEWPORT_H / 2.0 - v.y) / zoom,
    )
}

fn check_finite(pairs: &[(&str, f64)]) -> Result<(), OpError> {
    for (name, v) in pairs {
        if !v.is_finite() {
            return Err(OpError::new(
                "bad_geometry",
                format!("{name} must be a finite number"),
            ));
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Z-space model (port of FE zSpace.ts — ADR-0024)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Item,
    Group,
}

#[derive(Debug, Clone)]
struct Block {
    id: String,
    kind: BlockKind,
}

/// Parent → direct atomic blocks, each list ascending by min z (a group
/// block's min z = min z over its descendant items, recursively). Port of
/// FE `buildChildBlocks`.
fn child_blocks(layout: &Layout) -> HashMap<Option<String>, Vec<Block>> {
    let mut out: HashMap<Option<String>, Vec<Block>> = HashMap::new();
    for it in &layout.items {
        out.entry(it.common().parent_id.clone()).or_default().push(Block {
            id: it.common().id.clone(),
            kind: BlockKind::Item,
        });
    }
    for g in &layout.groups {
        out.entry(g.parent_id.clone()).or_default().push(Block {
            id: g.id.clone(),
            kind: BlockKind::Group,
        });
    }

    let item_z: HashMap<&str, i32> = layout
        .items
        .iter()
        .map(|it| (it.common().id.as_str(), it.common().z))
        .collect();
    // children-of index for the group-min-z walk.
    let mut children_of: HashMap<&str, Vec<Block>> = HashMap::new();
    for g in &layout.groups {
        children_of.entry(g.id.as_str()).or_default();
    }
    for it in &layout.items {
        if let Some(p) = it.common().parent_id.as_deref() {
            children_of.entry(p).or_default().push(Block {
                id: it.common().id.clone(),
                kind: BlockKind::Item,
            });
        }
    }
    for g in &layout.groups {
        if let Some(p) = g.parent_id.as_deref() {
            children_of.entry(p).or_default().push(Block {
                id: g.id.clone(),
                kind: BlockKind::Group,
            });
        }
    }
    let mut group_min_z: HashMap<String, i32> = HashMap::new();
    let mut min_z = |block: &Block| -> i32 {
        match block.kind {
            BlockKind::Item => item_z.get(block.id.as_str()).copied().unwrap_or(0),
            BlockKind::Group => {
                if let Some(&cached) = group_min_z.get(&block.id) {
                    return cached;
                }
                let mut min = i32::MAX;
                let mut stack: Vec<&str> = vec![block.id.as_str()];
                while let Some(cur) = stack.pop() {
                    for k in children_of.get(cur).map(|v| v.as_slice()).unwrap_or(&[]) {
                        match k.kind {
                            BlockKind::Item => {
                                if let Some(&z) = item_z.get(k.id.as_str()) {
                                    if z < min {
                                        min = z;
                                    }
                                }
                            }
                            BlockKind::Group => stack.push(k.id.as_str()),
                        }
                    }
                }
                let result = if min == i32::MAX { 0 } else { min };
                group_min_z.insert(block.id.clone(), result);
                result
            }
        }
    };
    for arr in out.values_mut() {
        // Rust's sort_by is stable — ties keep insertion order, matching the
        // FE (JS Array.prototype.sort is stable).
        arr.sort_by_key(&mut min_z);
    }
    out
}

/// Rewrite every item's z to the consecutive DFS order (ADR-0024 D3'
/// invariant). `overrides` forces the block order at specific parents; other
/// parents keep the current min-z order. Port of FE `normalizeLayout`.
fn normalize_z(layout: &mut Layout, overrides: &HashMap<Option<String>, Vec<String>>) {
    let mut blocks = child_blocks(layout);
    for (parent, new_order) in overrides {
        let arr = blocks.entry(parent.clone()).or_default();
        let index_of: HashMap<&str, usize> = new_order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();
        // Known ids sort by override index; unknown ids sink after known
        // ones, preserving relative order (FE parity).
        arr.sort_by(|a, b| {
            match (index_of.get(a.id.as_str()), index_of.get(b.id.as_str())) {
                (Some(ai), Some(bi)) => ai.cmp(bi),
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
            }
        });
    }
    let mut new_item_z: HashMap<String, i32> = HashMap::new();
    let mut cursor: i32 = 0;
    fn recurse(
        parent: &Option<String>,
        blocks: &HashMap<Option<String>, Vec<Block>>,
        new_item_z: &mut HashMap<String, i32>,
        cursor: &mut i32,
    ) {
        let Some(arr) = blocks.get(parent) else {
            return;
        };
        for block in arr {
            match block.kind {
                BlockKind::Item => {
                    new_item_z.insert(block.id.clone(), *cursor);
                    *cursor += 1;
                }
                BlockKind::Group => {
                    recurse(&Some(block.id.clone()), blocks, new_item_z, cursor);
                }
            }
        }
    }
    recurse(&None, &blocks, &mut new_item_z, &mut cursor);
    for it in layout.items.iter_mut() {
        if let Some(&z) = new_item_z.get(&it.common().id) {
            set_common(it, |c| c.z = z);
        }
    }
}

/// The 4 z actions (ADR-0024). CLI `raise`/`raise_top`/`lower`/`lower_bottom`
/// ≙ FE `forward`/`front`/`backward`/`back`.
#[derive(Debug, Clone, Copy)]
enum ZAction {
    Raise,
    RaiseTop,
    Lower,
    LowerBottom,
}

/// Apply a z action to one atomic block (item or group). A boundary no-op
/// (already top/bottom, or no siblings) succeeds without change — the op is
/// idempotent, matching the FE's silent-noop handling.
fn apply_z_action(layout: &mut Layout, id: &str, action: ZAction) -> Result<(), OpError> {
    let parent: Option<String> = if let Some(i) = item_idx(layout, id) {
        layout.items[i].common().parent_id.clone()
    } else if let Some(i) = group_idx(layout, id) {
        layout.groups[i].parent_id.clone()
    } else {
        return Err(OpError::not_found(id));
    };
    let blocks = child_blocks(layout);
    let current: Vec<String> = blocks
        .get(&parent)
        .map(|v| v.iter().map(|b| b.id.clone()).collect())
        .unwrap_or_default();
    let non_m: Vec<String> = current.iter().filter(|b| b.as_str() != id).cloned().collect();
    if non_m.is_empty() {
        return Ok(()); // sole block at this level — nothing to reorder.
    }
    let pos = current
        .iter()
        .position(|b| b == id)
        .expect("id resolved above must appear at its parent level");
    // Count of non-target blocks below the target (its "slot").
    let slot = current[..pos].iter().filter(|b| b.as_str() != id).count();
    let new_order: Vec<String> = match action {
        ZAction::RaiseTop => {
            let mut v = non_m.clone();
            v.push(id.to_string());
            v
        }
        ZAction::LowerBottom => {
            let mut v = vec![id.to_string()];
            v.extend(non_m.clone());
            v
        }
        ZAction::Raise => {
            if slot >= non_m.len() {
                return Ok(()); // already on top.
            }
            let mut v: Vec<String> = non_m[..slot + 1].to_vec();
            v.push(id.to_string());
            v.extend(non_m[slot + 1..].iter().cloned());
            v
        }
        ZAction::Lower => {
            if slot == 0 {
                return Ok(()); // already at the bottom.
            }
            let mut v: Vec<String> = non_m[..slot - 1].to_vec();
            v.push(id.to_string());
            v.extend(non_m[slot - 1..].iter().cloned());
            v
        }
    };
    if new_order == current {
        return Ok(());
    }
    let mut overrides = HashMap::new();
    overrides.insert(parent, new_order);
    normalize_z(layout, &overrides);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Common-field mutation helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Run `f` against the item's common block, whichever variant it is.
fn set_common(item: &mut Item, f: impl FnOnce(&mut crate::schema::ItemCommon)) {
    match item {
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
        | Item::Path { common, .. }
        | Item::Snippets { common, .. } => f(common),
    }
}

fn ensure_unlocked_item(item: &Item, force: bool) -> Result<(), OpError> {
    if item.common().locked && !force {
        return Err(OpError::locked(&item.common().id));
    }
    Ok(())
}

fn ensure_unlocked_group(group: &Group, force: bool) -> Result<(), OpError> {
    if group.locked && !force {
        return Err(OpError::locked(&group.id));
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Individual ops
// ─────────────────────────────────────────────────────────────────────────────

/// Move = set `x/y`, translating the type's absolute-coordinate payload by
/// the same delta (line second endpoint, free-draw points, path free
/// endpoints + waypoints + fallback points — connected endpoints keep
/// tracking their target and are left alone).
fn op_move(layout: &mut Layout, id: &str, x: f64, y: f64, force: bool) -> Result<(), OpError> {
    check_finite(&[("x", x), ("y", y)])?;
    let idx = item_idx(layout, id).ok_or_else(|| OpError::not_found(id))?;
    ensure_unlocked_item(&layout.items[idx], force)?;
    let item = &mut layout.items[idx];
    let (dx, dy) = {
        let c = item.common();
        (x - c.x, y - c.y)
    };
    set_common(item, |c| {
        c.x = x;
        c.y = y;
    });
    match item {
        Item::Line { x2, y2, .. } => {
            *x2 += dx;
            *y2 += dy;
        }
        Item::FreeDraw { points, .. } => {
            for p in points.iter_mut() {
                p.x += dx;
                p.y += dy;
            }
        }
        Item::Path {
            from,
            to,
            waypoints,
            ..
        } => {
            for ep in [from, to] {
                match ep {
                    crate::schema::PathEndpoint::Free { point } => {
                        point.x += dx;
                        point.y += dy;
                    }
                    crate::schema::PathEndpoint::Connected { .. } => {
                        // Tracks its target — the pipeline recomputes the
                        // fallback point from the live anchor.
                    }
                }
            }
            for wp in waypoints.iter_mut() {
                wp.x += dx;
                wp.y += dy;
            }
        }
        _ => {}
    }
    Ok(())
}

fn op_resize(layout: &mut Layout, id: &str, w: f64, h: f64, force: bool) -> Result<(), OpError> {
    check_finite(&[("w", w), ("h", h)])?;
    if w <= 0.0 || h <= 0.0 {
        return Err(OpError::new("bad_geometry", "w/h must be positive"));
    }
    let idx = item_idx(layout, id).ok_or_else(|| OpError::not_found(id))?;
    ensure_unlocked_item(&layout.items[idx], force)?;
    match &layout.items[idx] {
        Item::Line { .. } | Item::Path { .. } | Item::FreeDraw { .. } => {
            return Err(OpError::new(
                "resize_unsupported",
                "line/path/free_draw geometry is endpoint-derived — edit the endpoints instead",
            ));
        }
        _ => {}
    }
    set_common(&mut layout.items[idx], |c| {
        c.w = w;
        c.h = h;
    });
    Ok(())
}

fn op_set_visibility(
    layout: &mut Layout,
    id: &str,
    visibility: Visibility,
    force: bool,
) -> Result<(), OpError> {
    if let Some(idx) = item_idx(layout, id) {
        ensure_unlocked_item(&layout.items[idx], force)?;
        set_common(&mut layout.items[idx], |c| c.visibility = visibility);
        return Ok(());
    }
    if let Some(idx) = group_idx(layout, id) {
        ensure_unlocked_group(&layout.groups[idx], force)?;
        layout.groups[idx].visibility = visibility;
        return Ok(());
    }
    Err(OpError::not_found(id))
}

fn op_label(
    layout: &mut Layout,
    id: &str,
    label: Option<&str>,
    force: bool,
) -> Result<(), OpError> {
    if let Some(idx) = item_idx(layout, id) {
        ensure_unlocked_item(&layout.items[idx], force)?;
        let value = label.unwrap_or_default().to_string();
        set_common(&mut layout.items[idx], |c| c.label = value);
        return Ok(());
    }
    if let Some(idx) = group_idx(layout, id) {
        ensure_unlocked_group(&layout.groups[idx], force)?;
        layout.groups[idx].label = label.unwrap_or_default().to_string();
        return Ok(());
    }
    Err(OpError::not_found(id))
}

/// Minimize / restore — FE parity (`ItemInfoView.svelte::applyMinimizeGeom`).
/// Only terminal / note / document / snippets support minimize; other types
/// have no minimized visual and are rejected, matching the FE (the inspector
/// hides the button for them). Already-in-state targets are a silent no-op.
fn op_minimize(layout: &mut Layout, id: &str, next: bool, force: bool) -> Result<(), OpError> {
    let idx = item_idx(layout, id).ok_or_else(|| OpError::not_found(id))?;
    ensure_unlocked_item(&layout.items[idx], force)?;
    let item = &mut layout.items[idx];
    let supported = matches!(
        item,
        Item::Terminal { .. } | Item::Note { .. } | Item::Document { .. } | Item::Snippets { .. }
    );
    if !supported {
        return Err(OpError::new(
            "minimize_unsupported",
            format!("item {id:?} type does not support minimize/restore"),
        ));
    }
    if item.common().minimized == next {
        return Ok(());
    }
    if next {
        match item {
            Item::Note { .. } => set_common(item, |c| {
                c.minimized = true;
                c.w = NOTE_CHIP;
                c.h = NOTE_CHIP;
            }),
            Item::Document { .. } => set_common(item, |c| {
                c.minimized = true;
                c.h = DOC_STRIP_H;
            }),
            Item::Snippets { .. } => set_common(item, |c| {
                c.minimized = true;
                c.h = SNIP_STRIP_H;
            }),
            _ => set_common(item, |c| {
                c.minimized = true;
                c.h = PANEL_STRIP_H;
            }),
        }
    } else {
        // No schema-level geometry backup yet (ADR-0018 D11 is Draft) —
        // restore to the FE default fallback sizes.
        match item {
            Item::Note { .. } => set_common(item, |c| {
                c.minimized = false;
                c.w = NOTE_RESTORE_W;
                c.h = NOTE_RESTORE_H;
            }),
            Item::Document { .. } => set_common(item, |c| {
                c.minimized = false;
                c.w = DOC_RESTORE_W;
                c.h = DOC_RESTORE_H;
            }),
            Item::Snippets { .. } => set_common(item, |c| {
                c.minimized = false;
                c.w = SNIP_RESTORE_W;
                c.h = SNIP_RESTORE_H;
            }),
            _ => set_common(item, |c| {
                c.minimized = false;
                c.h = PANEL_RESTORE_H;
            }),
        }
    }
    Ok(())
}

/// Common fields that `edit` must not touch (dedicated ops / server-owned).
/// `description` is deliberately absent — it is the one common field `edit`
/// may set (ADR-0053 D10).
const EDIT_FORBIDDEN_COMMON: &[&str] = &[
    "x",
    "y",
    "w",
    "h",
    "z",
    "visibility",
    "locked",
    "label",
    "minimized",
    "parent_id",
];

fn op_edit(layout: &mut Layout, id: &str, fields: &Value, force: bool) -> Result<(), OpError> {
    let Value::Object(fields) = fields else {
        return Err(OpError::new("bad_edit_payload", "fields must be a JSON object"));
    };
    let idx = item_idx(layout, id).ok_or_else(|| OpError::not_found(id))?;
    ensure_unlocked_item(&layout.items[idx], force)?;

    let mut current = match serde_json::to_value(&layout.items[idx]) {
        Ok(Value::Object(m)) => m,
        _ => return Err(OpError::new("internal", "item failed to serialize")),
    };
    for (key, value) in fields {
        match key.as_str() {
            "id" | "type" => {
                // Immutable — reject any *change*; an echo of the current
                // value is tolerated so CLI round-trips of a full payload
                // don't fail spuriously.
                if current.get(key) != Some(value) {
                    return Err(OpError::new(
                        "edit_field_immutable",
                        format!("{key} is immutable"),
                    ));
                }
            }
            k if EDIT_FORBIDDEN_COMMON.contains(&k) => {
                return Err(OpError::new(
                    "edit_field_not_allowed",
                    format!("common field {k:?} is not editable via edit — use its dedicated op"),
                ));
            }
            _ => {
                current.insert(key.clone(), value.clone());
            }
        }
    }
    let next: Item = serde_json::from_value(Value::Object(current)).map_err(|e| {
        OpError::new("bad_edit_payload", format!("merged payload is invalid: {e}"))
    })?;
    layout.items[idx] = next;
    Ok(())
}

fn op_create(
    layout: &mut Layout,
    item_type: &str,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    fields: Option<&Value>,
) -> Result<String, OpError> {
    if item_type == "terminal" {
        return Err(OpError::new(
            "create_terminal_not_allowed",
            "terminal creation is the spawn op (ADR-0053 D11), not create",
        ));
    }
    if !KNOWN_CREATE_TYPES.contains(&item_type) {
        return Err(OpError::new(
            "unknown_item_type",
            format!("unknown item type {item_type:?}"),
        ));
    }
    let fields_map: Map<String, Value> = match fields {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return Err(OpError::new(
                "bad_create_payload",
                "fields must be a JSON object",
            ))
        }
    };
    if fields_map.contains_key("id") {
        return Err(OpError::new(
            "create_field_immutable",
            "id is server-issued on create",
        ));
    }
    if let Some(t) = fields_map.get("type") {
        if t != &Value::String(item_type.to_string()) {
            return Err(OpError::new(
                "create_field_immutable",
                "fields.type must match item_type",
            ));
        }
    }

    let field_num = |key: &str| fields_map.get(key).and_then(Value::as_f64);
    let (def_w, def_h) = default_size(item_type);
    let w = w.or_else(|| field_num("w")).unwrap_or(def_w);
    let h = h.or_else(|| field_num("h")).unwrap_or(def_h);
    let (cx, cy) = viewport_center(&layout.viewport);
    let x = x.or_else(|| field_num("x")).unwrap_or(cx - w / 2.0);
    let y = y.or_else(|| field_num("y")).unwrap_or(cy - h / 2.0);
    check_finite(&[("x", x), ("y", y), ("w", w), ("h", h)])?;

    let id = fresh_uuid();
    let max_z = layout.items.iter().map(|it| it.common().z).max().unwrap_or(0);

    // Base object: type defaults (FE itemFactory parity) → user fields →
    // server-owned fields last.
    let mut obj = default_payload(item_type, x, y);
    for (k, v) in &fields_map {
        obj.insert(k.clone(), v.clone());
    }
    obj.insert("type".into(), json!(item_type));
    obj.insert("id".into(), json!(id));
    obj.insert("parent_id".into(), Value::Null); // ADR-0053 D10 — root; reparent op exists.
    obj.insert("x".into(), json!(x));
    obj.insert("y".into(), json!(y));
    obj.insert("w".into(), json!(w));
    obj.insert("h".into(), json!(h));
    obj.insert("z".into(), json!(max_z.saturating_add(1)));
    obj.entry("visibility").or_insert(json!("visible"));
    obj.entry("locked").or_insert(json!(false));
    obj.entry("minimized").or_insert(json!(false));

    match item_type {
        "document" => {
            // Exactly-one-origin invariant (ADR-0047 D1): default to the
            // inline empty-markdown origin only when the caller supplied none.
            let has_origin = ["path", "content", "asset_id"]
                .iter()
                .any(|k| matches!(obj.get(*k), Some(v) if !v.is_null()));
            if !has_origin {
                obj.insert("content".into(), json!(""));
            }
        }
        "snippets" => {
            // ADR-0053 D10(c) — snippet entry ids are server-issued.
            if let Some(Value::Array(entries)) = obj.get_mut("entries") {
                for e in entries.iter_mut() {
                    if let Value::Object(entry) = e {
                        entry.insert("id".into(), json!(fresh_uuid()));
                    }
                }
            }
        }
        "line" => {
            // Default second endpoint, then keep the hit box canonical.
            let x2 = field_num("x2").unwrap_or(x + LINE_DEFAULT_DX);
            let y2 = field_num("y2").unwrap_or(y + LINE_DEFAULT_DY);
            check_finite(&[("x2", x2), ("y2", y2)])?;
            obj.insert("x2".into(), json!(x2));
            obj.insert("y2".into(), json!(y2));
            let bw = (x2 - x).abs().max(1.0) + 2.0 * LINE_HIT_PADDING;
            let bh = (y2 - y).abs().max(1.0) + 2.0 * LINE_HIT_PADDING;
            obj.insert("w".into(), json!(bw));
            obj.insert("h".into(), json!(bh));
        }
        "free_draw" => {
            // Points are absolute — recompute the padded bbox when provided
            // (FE createFreeDrawItem parity).
            if let Some(Value::Array(points)) = obj.get("points") {
                let coords: Vec<(f64, f64)> = points
                    .iter()
                    .filter_map(|p| {
                        Some((p.get("x")?.as_f64()?, p.get("y")?.as_f64()?))
                    })
                    .collect();
                if let (Some(min_x), Some(min_y), Some(max_x), Some(max_y)) = (
                    coords.iter().map(|c| c.0).reduce(f64::min),
                    coords.iter().map(|c| c.1).reduce(f64::min),
                    coords.iter().map(|c| c.0).reduce(f64::max),
                    coords.iter().map(|c| c.1).reduce(f64::max),
                ) {
                    obj.insert("x".into(), json!(min_x - FREE_DRAW_PADDING));
                    obj.insert("y".into(), json!(min_y - FREE_DRAW_PADDING));
                    obj.insert(
                        "w".into(),
                        json!((max_x - min_x).max(1.0) + 2.0 * FREE_DRAW_PADDING),
                    );
                    obj.insert(
                        "h".into(),
                        json!((max_y - min_y).max(1.0) + 2.0 * FREE_DRAW_PADDING),
                    );
                }
            }
        }
        _ => {}
    }

    let item: Item = serde_json::from_value(Value::Object(obj)).map_err(|e| {
        OpError::new(
            "bad_create_payload",
            format!("create payload is invalid: {e}"),
        )
    })?;
    layout.items.push(item);
    Ok(id)
}

/// Type-specific required-field defaults — FE `itemFactory.ts` parity.
/// Fields with serde-level defaults in `schema.rs` are omitted unless the FE
/// factory sets a different explicit value.
fn default_payload(item_type: &str, x: f64, y: f64) -> Map<String, Value> {
    let v = match item_type {
        "text" => json!({
            "text": "",
            "font_size": 16,
            "color": "var(--color-fg)",
            "stroke": "var(--color-fg)",
            "fill": "var(--color-surface)",
            "stroke_width": 2,
            "fill_enabled": false,
            "stroke_enabled": false,
        }),
        "note" => json!({
            "title": "",
            "body": "",
            "color": "var(--color-accent)",
        }),
        "rect" | "ellipse" => json!({
            "stroke": "var(--color-fg)",
            "fill": "#D9D9D9",
            "stroke_width": 2,
            "fill_enabled": false,
            "stroke_enabled": true,
            "text": "",
            "font_size": 14,
            "color": "var(--color-fg)",
        }),
        "line" => json!({
            "stroke": "var(--color-fg)",
            "stroke_width": 2,
            "x2": x + LINE_DEFAULT_DX,
            "y2": y + LINE_DEFAULT_DY,
            "head_from": "none",
            "head_to": "none",
        }),
        "free_draw" => json!({
            "stroke": "var(--color-fg)",
            "stroke_width": 2,
            "points": [],
        }),
        "image" => json!({}),
        "document" => json!({}),
        "file_path" => json!({ "path": "" }),
        "path" => json!({
            "from": { "kind": "free", "point": { "x": x, "y": y } },
            "to": {
                "kind": "free",
                "point": { "x": x + PATH_DEFAULT_DX, "y": y + PATH_DEFAULT_DY },
            },
            "routing": "orthogonal",
            "head_from": "none",
            "head_to": "none",
            "stroke": "var(--color-fg)",
            "stroke_width": 2,
        }),
        "snippets" => json!({ "entries": [] }),
        _ => json!({}),
    };
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Terminal lifecycle ops (ADR-0053 D11 — Batch B)
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve the panel geometry for `spawn`/`mount` — the `create` default
/// rule (ADR-0053 D11 / 잔여 확인 3): explicit values win, otherwise the
/// stored-viewport center with the FE terminal default size.
fn terminal_placement(
    layout: &Layout,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
) -> Result<(f64, f64, f64, f64), OpError> {
    let w = w.unwrap_or(TERMINAL_DEFAULT_W);
    let h = h.unwrap_or(TERMINAL_DEFAULT_H);
    if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
        return Err(OpError::new(
            "bad_geometry",
            "w/h must be positive finite numbers",
        ));
    }
    let (cx, cy) = viewport_center(&layout.viewport);
    let x = x.unwrap_or(cx - w / 2.0);
    let y = y.unwrap_or(cy - h / 2.0);
    check_finite(&[("x", x), ("y", y)])?;
    Ok((x, y, w, h))
}

/// Append a TerminalItem with the given id at top z (ADR-0018 D7 parity
/// with `create`).
fn push_terminal_item(layout: &mut Layout, id: &str, x: f64, y: f64, w: f64, h: f64) {
    let max_z = layout
        .items
        .iter()
        .map(|it| it.common().z)
        .max()
        .unwrap_or(0);
    layout.items.push(Item::Terminal {
        common: crate::schema::ItemCommon {
            id: id.to_string(),
            parent_id: None,
            x,
            y,
            w,
            h,
            z: max_z.saturating_add(1),
            visibility: Visibility::Visible,
            locked: false,
            label: String::new(),
            description: String::new(),
            minimized: false,
        },
    });
}

/// `spawn` — mint a fresh terminal UUID (the canonical terminal-id mint,
/// ADR-0018 D2 global namespace) and persist its TerminalItem. The PTY
/// spawn itself is a handler side effect after the layout commit.
fn op_spawn(
    layout: &mut Layout,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
) -> Result<String, OpError> {
    let (x, y, w, h) = terminal_placement(layout, x, y, w, h)?;
    let id = crate::terminal_map::fresh_terminal_uuid();
    push_terminal_item(layout, &id, x, y, w, h);
    Ok(id)
}

/// `mount` — add a TerminalItem referencing an existing pool terminal
/// (no spawn). Aliveness is pre-flighted by the handler; the pure core
/// rejects only the structural duplicate (same UUID already mounted in
/// this session layout — ADR-0053 D11, explicit code ahead of the
/// pipeline's id-uniqueness validate).
fn op_mount(
    layout: &mut Layout,
    uuid: &str,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
) -> Result<(), OpError> {
    if item_idx(layout, uuid).is_some() {
        return Err(OpError::new(
            "already_mounted",
            format!("terminal {uuid:?} already has an item in this session layout"),
        ));
    }
    let (x, y, w, h) = terminal_placement(layout, x, y, w, h)?;
    push_terminal_item(layout, uuid, x, y, w, h);
    Ok(())
}

fn op_delete(
    layout: &mut Layout,
    id: &str,
    kill_terminal: bool,
    force: bool,
    outcome: &mut ApplyOutcome,
) -> Result<(), OpError> {
    let idx = item_idx(layout, id).ok_or_else(|| OpError::not_found(id))?;
    ensure_unlocked_item(&layout.items[idx], force)?;
    let removed = layout.items.remove(idx);
    if kill_terminal {
        if let Item::Terminal { common } = &removed {
            outcome.kill_terminal_uuids.push(common.id.clone());
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Group ops (port of FE sessionStore group helpers — ADR-0010 D12/D14)
// ─────────────────────────────────────────────────────────────────────────────

/// Root-first ancestor chain `[None, root-group, …, direct-parent]` for an
/// item or group id. Port of FE `#ancestorChainTopDown` (the `None` sentinel
/// makes chains with no common group intersect at the canvas root).
fn ancestor_chain(layout: &Layout, id: &str) -> Option<Vec<Option<String>>> {
    let parent: Option<String> = if let Some(i) = group_idx(layout, id) {
        layout.groups[i].parent_id.clone()
    } else if let Some(i) = item_idx(layout, id) {
        layout.items[i].common().parent_id.clone()
    } else {
        return None;
    };
    let mut chain: Vec<Option<String>> = Vec::new();
    let mut cur = parent;
    let mut hops = 0usize;
    while let Some(gid) = cur {
        chain.insert(0, Some(gid.clone()));
        cur = group_idx(layout, &gid).and_then(|i| layout.groups[i].parent_id.clone());
        hops += 1;
        if hops > layout.groups.len() {
            // Defensive: a cycle in the stored layout (validate rejects these
            // at the pipeline) — treat as rooted.
            break;
        }
    }
    chain.insert(0, None);
    Some(chain)
}

fn common_ancestor(layout: &Layout, ids: &[String]) -> Option<String> {
    let mut iter = ids.iter();
    let first = iter.next()?;
    let mut common = ancestor_chain(layout, first)?;
    for id in iter {
        let c = ancestor_chain(layout, id)?;
        let lim = common.len().min(c.len());
        let mut k = 0;
        while k < lim && common[k] == c[k] {
            k += 1;
        }
        common.truncate(k);
    }
    common.last().cloned().flatten()
}

/// All ids (items + groups) inside group `gid`, recursively.
fn descendant_ids(layout: &Layout, gid: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut stack = vec![gid.to_string()];
    while let Some(cur) = stack.pop() {
        for g in &layout.groups {
            if g.parent_id.as_deref() == Some(cur.as_str()) && out.insert(g.id.clone()) {
                stack.push(g.id.clone());
            }
        }
        for it in &layout.items {
            if it.common().parent_id.as_deref() == Some(cur.as_str()) {
                out.insert(it.common().id.clone());
            }
        }
    }
    out
}

/// ADR-0010 D14 — auto label "Group N", N = max over live auto labels + 1.
fn next_group_label(layout: &Layout) -> String {
    let mut max = 0u32;
    for g in &layout.groups {
        if let Some(rest) = g.label.strip_prefix("Group ") {
            if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
                if let Ok(n) = rest.parse::<u32>() {
                    if n > max {
                        max = n;
                    }
                }
            }
        }
    }
    format!("Group {}", max.saturating_add(1))
}

fn block_ids_at_parent(layout: &Layout, parent: &Option<String>) -> Vec<String> {
    child_blocks(layout)
        .get(parent)
        .map(|v| v.iter().map(|b| b.id.clone()).collect())
        .unwrap_or_default()
}

fn op_group_create(
    layout: &mut Layout,
    ids: &[String],
    label: Option<&str>,
) -> Result<String, OpError> {
    // Existence check first — a missing target fails the whole batch
    // (atomicity) rather than silently shrinking the group.
    let mut seen: HashSet<&str> = HashSet::new();
    let unique: Vec<String> = ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect();
    for id in &unique {
        if item_idx(layout, id).is_none() && group_idx(layout, id).is_none() {
            return Err(OpError::not_found(id));
        }
    }
    // Dedup: drop targets that are descendants of another selected group
    // (FE `#dedupForGrouping` parity).
    let mut removed: HashSet<String> = HashSet::new();
    for id in &unique {
        if group_idx(layout, id).is_some() {
            removed.extend(descendant_ids(layout, id));
        }
    }
    let members: Vec<String> = unique
        .iter()
        .filter(|id| !removed.contains(id.as_str()))
        .cloned()
        .collect();
    if members.is_empty() {
        return Err(OpError::new(
            "group_targets_empty",
            "group_create needs at least one target (ADR-0010 D4 — no empty groups)",
        ));
    }

    let ca = common_ancestor(layout, &members);
    let gid = fresh_uuid();
    let label = match label {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => next_group_label(layout),
    };
    let sibling_max_order = layout
        .groups
        .iter()
        .filter(|g| g.parent_id == ca)
        .map(|g| g.order)
        .max()
        .unwrap_or(0);

    let member_set: HashSet<&str> = members.iter().map(|s| s.as_str()).collect();
    for it in layout.items.iter_mut() {
        if member_set.contains(it.common().id.as_str()) {
            let gid = gid.clone();
            set_common(it, move |c| c.parent_id = Some(gid));
        }
    }
    for g in layout.groups.iter_mut() {
        if member_set.contains(g.id.as_str()) {
            g.parent_id = Some(gid.clone());
        }
    }
    layout.groups.push(Group {
        id: gid.clone(),
        parent_id: ca.clone(),
        label,
        color: None,
        visibility: Visibility::Visible,
        locked: false,
        order: sibling_max_order.saturating_add(1),
    });

    // Z: the new group lands on top of the common-ancestor level; its
    // children keep their relative order (FE createGroup parity).
    let mut ca_order: Vec<String> = block_ids_at_parent(layout, &ca)
        .into_iter()
        .filter(|id| id != &gid)
        .collect();
    ca_order.push(gid.clone());
    let child_order = block_ids_at_parent(layout, &Some(gid.clone()));
    let mut overrides = HashMap::new();
    overrides.insert(ca, ca_order);
    overrides.insert(Some(gid.clone()), child_order);
    normalize_z(layout, &overrides);
    Ok(gid)
}

/// ADR-0010 D12 — non-destructive ungroup: promote direct children to the
/// group's parent, remove the entity, splice the children into the old
/// group's z slot.
fn op_ungroup(layout: &mut Layout, group_id: &str, force: bool) -> Result<(), OpError> {
    let idx = group_idx(layout, group_id).ok_or_else(|| OpError::not_found(group_id))?;
    ensure_unlocked_group(&layout.groups[idx], force)?;
    let parent = layout.groups[idx].parent_id.clone();

    // Orders computed on the pre-removal layout (FE parity).
    let parent_order_before = block_ids_at_parent(layout, &parent);
    let child_order = block_ids_at_parent(layout, &Some(group_id.to_string()));

    for it in layout.items.iter_mut() {
        if it.common().parent_id.as_deref() == Some(group_id) {
            let parent = parent.clone();
            set_common(it, move |c| c.parent_id = parent);
        }
    }
    for g in layout.groups.iter_mut() {
        if g.parent_id.as_deref() == Some(group_id) {
            g.parent_id = parent.clone();
        }
    }
    layout.groups.retain(|g| g.id != group_id);

    let mut new_order: Vec<String> = Vec::with_capacity(parent_order_before.len() + child_order.len());
    for id in parent_order_before {
        if id == group_id {
            new_order.extend(child_order.iter().cloned());
        } else {
            new_order.push(id);
        }
    }
    let mut overrides = HashMap::new();
    overrides.insert(parent, new_order);
    normalize_z(layout, &overrides);
    Ok(())
}

fn op_reparent(
    layout: &mut Layout,
    id: &str,
    parent_id: Option<&str>,
    force: bool,
) -> Result<(), OpError> {
    if let Some(p) = parent_id {
        if group_idx(layout, p).is_none() {
            return Err(OpError::new(
                "parent_not_found",
                format!("parent {p:?} is not a group in the layout"),
            ));
        }
    }
    if let Some(idx) = item_idx(layout, id) {
        ensure_unlocked_item(&layout.items[idx], force)?;
        let parent = parent_id.map(|s| s.to_string());
        set_common(&mut layout.items[idx], move |c| c.parent_id = parent);
    } else if let Some(idx) = group_idx(layout, id) {
        ensure_unlocked_group(&layout.groups[idx], force)?;
        if parent_id == Some(id) {
            return Err(OpError::new(
                "group_cycle",
                "a group cannot be its own parent",
            ));
        }
        // Deeper cycles are caught by schema::validate (GroupCycle) in the
        // pipeline — ADR-0053 D13.
        layout.groups[idx].parent_id = parent_id.map(|s| s.to_string());
    } else {
        return Err(OpError::not_found(id));
    }
    normalize_z(layout, &HashMap::new());
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Batch driver
// ─────────────────────────────────────────────────────────────────────────────

/// Apply `ops` in order against `layout`. All-or-nothing at the caller level:
/// on `Err`, the caller must discard the mutated layout (the handler works on
/// a clone of the cached snapshot, so an error simply drops it).
pub fn apply_ops(layout: &mut Layout, ops: &[LayoutOp]) -> Result<ApplyOutcome, (usize, OpError)> {
    let mut outcome = ApplyOutcome::default();
    for (index, op) in ops.iter().enumerate() {
        let result: Result<(), OpError> = match op {
            LayoutOp::Move { id, x, y, force } => op_move(layout, id, *x, *y, *force),
            LayoutOp::Resize { id, w, h, force } => op_resize(layout, id, *w, *h, *force),
            LayoutOp::Show { id, force } => {
                op_set_visibility(layout, id, Visibility::Visible, *force)
            }
            LayoutOp::Hide { id, force } => {
                op_set_visibility(layout, id, Visibility::Hidden, *force)
            }
            LayoutOp::Minimize { id, force } => op_minimize(layout, id, true, *force),
            LayoutOp::Restore { id, force } => op_minimize(layout, id, false, *force),
            LayoutOp::Label { id, label, force } => {
                op_label(layout, id, label.as_deref(), *force)
            }
            LayoutOp::Raise { id, force } => raise_with_lock(layout, id, ZAction::Raise, *force),
            LayoutOp::RaiseTop { id, force } => {
                raise_with_lock(layout, id, ZAction::RaiseTop, *force)
            }
            LayoutOp::Lower { id, force } => raise_with_lock(layout, id, ZAction::Lower, *force),
            LayoutOp::LowerBottom { id, force } => {
                raise_with_lock(layout, id, ZAction::LowerBottom, *force)
            }
            LayoutOp::Edit { id, fields, force } => op_edit(layout, id, fields, *force),
            LayoutOp::Create {
                item_type,
                x,
                y,
                w,
                h,
                fields,
            } => op_create(layout, item_type, *x, *y, *w, *h, fields.as_ref())
                .map(|id| outcome.created_ids.push(id)),
            LayoutOp::Spawn { x, y, w, h } => op_spawn(layout, *x, *y, *w, *h).map(|id| {
                outcome.created_ids.push(id.clone());
                outcome.spawned_terminal_uuids.push(id);
            }),
            LayoutOp::Mount { uuid, x, y, w, h } => op_mount(layout, uuid, *x, *y, *w, *h),
            LayoutOp::Delete {
                id,
                kill_terminal,
                force,
            } => op_delete(layout, id, *kill_terminal, *force, &mut outcome),
            LayoutOp::GroupCreate { ids, label } => {
                op_group_create(layout, ids, label.as_deref())
                    .map(|gid| outcome.created_ids.push(gid))
            }
            LayoutOp::Ungroup { group_id, force } => op_ungroup(layout, group_id, *force),
            LayoutOp::Reparent {
                id,
                parent_id,
                force,
            } => op_reparent(layout, id, parent_id.as_deref(), *force),
        };
        if let Err(e) = result {
            return Err((index, e));
        }
    }
    Ok(outcome)
}

/// Z actions are mutations too — locked targets need `force` (ADR-0053 D6).
fn raise_with_lock(
    layout: &mut Layout,
    id: &str,
    action: ZAction,
    force: bool,
) -> Result<(), OpError> {
    if let Some(idx) = item_idx(layout, id) {
        ensure_unlocked_item(&layout.items[idx], force)?;
    } else if let Some(idx) = group_idx(layout, id) {
        ensure_unlocked_group(&layout.groups[idx], force)?;
    } else {
        return Err(OpError::not_found(id));
    }
    apply_z_action(layout, id, action)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Image dimension sniffing (ADR-0053 D10 amend ② (b))
// ─────────────────────────────────────────────────────────────────────────────

/// Best-effort pixel dimensions from magic bytes — PNG / GIF / JPEG / WebP
/// (the `sniff_image` allowlist minus SVG, which has no fixed pixel size).
/// Used by `create image` to derive `original_w`/`original_h` when the caller
/// leaves them unset. `None` = leave the fields absent (they are optional).
pub(crate) fn image_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    // PNG: 8-byte signature, 4-byte IHDR length, "IHDR", then w/h BE u32.
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        if b.len() >= 24 && &b[12..16] == b"IHDR" {
            let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
            let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
            return Some((w, h));
        }
        return None;
    }
    // GIF: logical screen size, LE u16 at offset 6/8.
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        if b.len() >= 10 {
            let w = u16::from_le_bytes([b[6], b[7]]) as u32;
            let h = u16::from_le_bytes([b[8], b[9]]) as u32;
            return Some((w, h));
        }
        return None;
    }
    // JPEG: scan markers for an SOFn segment (C0..CF minus C4/C8/CC).
    if b.starts_with(b"\xFF\xD8\xFF") {
        let mut i = 2usize;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            if marker == 0xFF {
                i += 1;
                continue;
            }
            if (0xD0..=0xD9).contains(&marker) {
                i += 2; // RSTn / SOI / EOI carry no length field.
                continue;
            }
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
        return None;
    }
    // WebP: RIFF container, first chunk decides the flavor.
    if b.len() >= 30 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        match &b[12..16] {
            b"VP8 " => {
                // Lossy: 3-byte frame tag then 9D 01 2A start code, w/h LE
                // u16 (low 14 bits).
                if b[23] == 0x9D && b[24] == 0x01 && b[25] == 0x2A {
                    let w = (u16::from_le_bytes([b[26], b[27]]) & 0x3FFF) as u32;
                    let h = (u16::from_le_bytes([b[28], b[29]]) & 0x3FFF) as u32;
                    return Some((w, h));
                }
            }
            b"VP8L" => {
                // Lossless: 0x2F signature then 14+14 bits, minus-one coded.
                if b[20] == 0x2F {
                    let bits = u32::from_le_bytes([b[21], b[22], b[23], b[24]]);
                    let w = (bits & 0x3FFF) + 1;
                    let h = ((bits >> 14) & 0x3FFF) + 1;
                    return Some((w, h));
                }
            }
            b"VP8X" => {
                // Extended: canvas size, 24-bit LE minus-one coded.
                let w = 1 + u32::from_le_bytes([b[24], b[25], b[26], 0]);
                let h = 1 + u32::from_le_bytes([b[27], b[28], b[29], 0]);
                return Some((w, h));
            }
            _ => {}
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests (pure core)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::schema::{self, ItemCommon};

    const A: &str = "7f3a0000-b9e2-4111-8222-00000000000a";
    const B: &str = "7f3a0000-b9e2-4111-8222-00000000000b";
    const C: &str = "7f3a0000-b9e2-4111-8222-00000000000c";
    const G1: &str = "0d990000-0000-4111-8222-0000000000a1";
    const G2: &str = "0d990000-0000-4111-8222-0000000000a2";

    fn common(id: &str, z: i32) -> ItemCommon {
        ItemCommon {
            id: id.to_string(),
            parent_id: None,
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
            z,
            visibility: Visibility::Visible,
            locked: false,
            label: String::new(),
            description: String::new(),
            minimized: false,
        }
    }

    fn rect(id: &str, z: i32) -> Item {
        serde_json::from_value(serde_json::json!({
            "type": "rect",
            "id": id, "parent_id": null,
            "x": 0.0, "y": 0.0, "w": 100.0, "h": 50.0, "z": z,
            "visibility": "visible", "locked": false, "minimized": false,
            "stroke": "#000", "fill": "#fff", "stroke_width": 2
        }))
        .unwrap()
    }

    fn layout_with(items: Vec<Item>) -> Layout {
        let mut l = Layout::empty();
        l.items = items;
        l
    }

    #[test]
    fn z_raise_top_normalizes_consecutively() {
        let mut l = layout_with(vec![rect(A, 0), rect(B, 1), rect(C, 2)]);
        apply_z_action(&mut l, A, ZAction::RaiseTop).unwrap();
        let z_of = |l: &Layout, id: &str| l.items[item_idx(l, id).unwrap()].common().z;
        assert_eq!(z_of(&l, B), 0);
        assert_eq!(z_of(&l, C), 1);
        assert_eq!(z_of(&l, A), 2);
    }

    #[test]
    fn z_raise_swaps_one_step() {
        let mut l = layout_with(vec![rect(A, 0), rect(B, 1), rect(C, 2)]);
        apply_z_action(&mut l, A, ZAction::Raise).unwrap();
        let z_of = |l: &Layout, id: &str| l.items[item_idx(l, id).unwrap()].common().z;
        assert_eq!(z_of(&l, B), 0);
        assert_eq!(z_of(&l, A), 1);
        assert_eq!(z_of(&l, C), 2);
    }

    #[test]
    fn z_lower_at_bottom_is_noop() {
        let mut l = layout_with(vec![rect(A, 0), rect(B, 1)]);
        apply_z_action(&mut l, A, ZAction::Lower).unwrap();
        let z_of = |l: &Layout, id: &str| l.items[item_idx(l, id).unwrap()].common().z;
        assert_eq!(z_of(&l, A), 0);
        assert_eq!(z_of(&l, B), 1);
    }

    #[test]
    fn group_create_reparents_and_labels() {
        let mut l = layout_with(vec![rect(A, 0), rect(B, 1), rect(C, 2)]);
        let gid = op_group_create(&mut l, &[A.to_string(), B.to_string()], None).unwrap();
        assert!(schema::validate(&l).is_ok());
        assert_eq!(l.groups.len(), 1);
        assert_eq!(l.groups[0].id, gid);
        assert_eq!(l.groups[0].label, "Group 1");
        assert_eq!(l.groups[0].parent_id, None);
        let parent_of = |l: &Layout, id: &str| {
            l.items[item_idx(l, id).unwrap()].common().parent_id.clone()
        };
        assert_eq!(parent_of(&l, A).as_deref(), Some(gid.as_str()));
        assert_eq!(parent_of(&l, B).as_deref(), Some(gid.as_str()));
        assert_eq!(parent_of(&l, C), None);
        // New group lands on top: its members carry the highest z values.
        let z_of = |l: &Layout, id: &str| l.items[item_idx(l, id).unwrap()].common().z;
        assert_eq!(z_of(&l, C), 0);
        assert_eq!(z_of(&l, A), 1);
        assert_eq!(z_of(&l, B), 2);
    }

    #[test]
    fn ungroup_promotes_children_in_place() {
        let mut l = layout_with(vec![rect(A, 0), rect(B, 1), rect(C, 2)]);
        let gid = op_group_create(&mut l, &[A.to_string(), B.to_string()], None).unwrap();
        op_ungroup(&mut l, &gid, false).unwrap();
        assert!(l.groups.is_empty());
        for id in [A, B, C] {
            assert_eq!(
                l.items[item_idx(&l, id).unwrap()].common().parent_id,
                None
            );
        }
        assert!(schema::validate(&l).is_ok());
    }

    #[test]
    fn group_create_empty_targets_rejected() {
        let mut l = layout_with(vec![]);
        let err = op_group_create(&mut l, &[], None).unwrap_err();
        assert_eq!(err.code, "group_targets_empty");
    }

    #[test]
    fn reparent_missing_parent_rejected() {
        let mut l = layout_with(vec![rect(A, 0)]);
        let err = op_reparent(&mut l, A, Some(G1), false).unwrap_err();
        assert_eq!(err.code, "parent_not_found");
    }

    #[test]
    fn reparent_group_cycle_caught_by_validate() {
        let mut l = layout_with(vec![rect(A, 0)]);
        l.groups.push(Group {
            id: G1.to_string(),
            parent_id: None,
            label: "g1".into(),
            color: None,
            visibility: Visibility::Visible,
            locked: false,
            order: 0,
        });
        l.groups.push(Group {
            id: G2.to_string(),
            parent_id: Some(G1.to_string()),
            label: "g2".into(),
            color: None,
            visibility: Visibility::Visible,
            locked: false,
            order: 1,
        });
        // Reparent G1 under its own descendant G2 — structural op succeeds,
        // the pipeline validate rejects (ADR-0053 D13).
        op_reparent(&mut l, G1, Some(G2), false).unwrap();
        let err = schema::validate(&l).unwrap_err();
        assert_eq!(err.code(), "group_cycle");
    }

    #[test]
    fn edit_rejects_type_change_and_common_fields() {
        let mut l = layout_with(vec![rect(A, 0)]);
        let err = op_edit(&mut l, A, &serde_json::json!({ "type": "note" }), false).unwrap_err();
        assert_eq!(err.code, "edit_field_immutable");
        let err = op_edit(&mut l, A, &serde_json::json!({ "x": 5.0 }), false).unwrap_err();
        assert_eq!(err.code, "edit_field_not_allowed");
        // description is the one allowed common field.
        op_edit(&mut l, A, &serde_json::json!({ "description": "hi" }), false).unwrap();
        assert_eq!(l.items[0].common().description, "hi");
        // payload fields merge.
        op_edit(&mut l, A, &serde_json::json!({ "stroke": "#123" }), false).unwrap();
        match &l.items[0] {
            Item::Rect { stroke, .. } => assert_eq!(stroke, "#123"),
            other => panic!("unexpected variant {other:?}"),
        }
    }

    #[test]
    fn locked_item_requires_force() {
        let mut l = layout_with(vec![rect(A, 0)]);
        set_common(&mut l.items[0], |c| c.locked = true);
        let err = op_move(&mut l, A, 10.0, 10.0, false).unwrap_err();
        assert!(err.locked);
        assert_eq!(err.code, "locked");
        op_move(&mut l, A, 10.0, 10.0, true).unwrap();
        assert_eq!(l.items[0].common().x, 10.0);
    }

    #[test]
    fn create_defaults_center_on_viewport_and_top_z() {
        let mut l = layout_with(vec![rect(A, 3)]);
        let id = op_create(&mut l, "text", None, None, None, None, None).unwrap();
        let idx = item_idx(&l, &id).unwrap();
        let c = l.items[idx].common();
        // Default viewport (0,0,zoom 1) → nominal center (960, 540); text
        // default size 160x56 → top-left (880, 512).
        assert_eq!(c.w, 160.0);
        assert_eq!(c.h, 56.0);
        assert_eq!(c.x, 960.0 - 80.0);
        assert_eq!(c.y, 540.0 - 28.0);
        assert_eq!(c.z, 4);
        assert_eq!(c.parent_id, None);
        assert!(schema::validate(&l).is_ok());
    }

    #[test]
    fn create_terminal_rejected() {
        let mut l = Layout::empty();
        let err = op_create(&mut l, "terminal", None, None, None, None, None).unwrap_err();
        assert_eq!(err.code, "create_terminal_not_allowed");
    }

    #[test]
    fn create_snippets_issues_entry_ids() {
        let mut l = Layout::empty();
        let fields = serde_json::json!({
            "entries": [ { "key": "k1", "body": "b1" }, { "key": "k2", "body": "" } ]
        });
        let id = op_create(&mut l, "snippets", None, None, None, None, Some(&fields)).unwrap();
        let idx = item_idx(&l, &id).unwrap();
        match &l.items[idx] {
            Item::Snippets { entries, .. } => {
                assert_eq!(entries.len(), 2);
                for e in entries {
                    assert_eq!(e.id.len(), 36, "entry id must be a server-issued UUID");
                }
            }
            other => panic!("unexpected variant {other:?}"),
        }
        assert!(schema::validate(&l).is_ok());
    }

    #[test]
    fn spawn_defaults_center_top_z_and_reports_uuid() {
        let mut l = layout_with(vec![rect(A, 3)]);
        let ops = vec![LayoutOp::Spawn {
            x: None,
            y: None,
            w: None,
            h: None,
        }];
        let outcome = apply_ops(&mut l, &ops).unwrap();
        assert_eq!(outcome.created_ids.len(), 1);
        assert_eq!(outcome.spawned_terminal_uuids, outcome.created_ids);
        let id = &outcome.created_ids[0];
        assert_eq!(id.len(), 36, "spawn id is a server-issued UUID");
        let idx = item_idx(&l, id).unwrap();
        match &l.items[idx] {
            Item::Terminal { common } => {
                // Default viewport (0,0,zoom 1) → nominal center (960, 540);
                // terminal default 480x320 → top-left (720, 380).
                assert_eq!(common.w, TERMINAL_DEFAULT_W);
                assert_eq!(common.h, TERMINAL_DEFAULT_H);
                assert_eq!(common.x, 960.0 - TERMINAL_DEFAULT_W / 2.0);
                assert_eq!(common.y, 540.0 - TERMINAL_DEFAULT_H / 2.0);
                assert_eq!(common.z, 4);
                assert_eq!(common.parent_id, None);
            }
            other => panic!("unexpected variant {other:?}"),
        }
        assert!(schema::validate(&l).is_ok());
    }

    #[test]
    fn mount_adds_item_and_rejects_duplicate() {
        let uuid = "11111111-2222-4333-8444-5555555555ee";
        let mut l = Layout::empty();
        op_mount(&mut l, uuid, Some(10.0), Some(20.0), None, None).unwrap();
        let idx = item_idx(&l, uuid).unwrap();
        match &l.items[idx] {
            Item::Terminal { common } => {
                assert_eq!(common.x, 10.0);
                assert_eq!(common.y, 20.0);
                assert_eq!(common.w, TERMINAL_DEFAULT_W);
                assert_eq!(common.h, TERMINAL_DEFAULT_H);
            }
            other => panic!("unexpected variant {other:?}"),
        }
        assert!(schema::validate(&l).is_ok());
        // Same UUID mounted twice in one session layout → explicit reject.
        let err = op_mount(&mut l, uuid, None, None, None, None).unwrap_err();
        assert_eq!(err.code, "already_mounted");
    }

    #[test]
    fn spawn_rejects_bad_geometry() {
        let mut l = Layout::empty();
        let err = op_spawn(&mut l, None, None, Some(-5.0), None).unwrap_err();
        assert_eq!(err.code, "bad_geometry");
        let err = op_spawn(&mut l, Some(f64::NAN), None, None, None).unwrap_err();
        assert_eq!(err.code, "bad_geometry");
        assert!(l.items.is_empty());
    }

    #[test]
    fn delete_collects_kill_uuid_for_terminal() {
        let mut l = layout_with(vec![Item::Terminal { common: common(A, 0) }]);
        let mut outcome = ApplyOutcome::default();
        op_delete(&mut l, A, true, false, &mut outcome).unwrap();
        assert!(l.items.is_empty());
        assert_eq!(outcome.kill_terminal_uuids, vec![A.to_string()]);
    }

    #[test]
    fn minimize_restore_parity() {
        let mut l = layout_with(vec![Item::Terminal { common: common(A, 0) }]);
        op_minimize(&mut l, A, true, false).unwrap();
        let c = l.items[0].common();
        assert!(c.minimized);
        assert_eq!(c.h, PANEL_STRIP_H);
        op_minimize(&mut l, A, false, false).unwrap();
        let c = l.items[0].common();
        assert!(!c.minimized);
        assert_eq!(c.h, PANEL_RESTORE_H);
        // Unsupported type rejected.
        let mut l2 = layout_with(vec![rect(B, 0)]);
        let err = op_minimize(&mut l2, B, true, false).unwrap_err();
        assert_eq!(err.code, "minimize_unsupported");
    }

    #[test]
    fn image_dimensions_png_gif() {
        let mut png = Vec::new();
        png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&48u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert_eq!(image_dimensions(&png), Some((64, 48)));

        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&120u16.to_le_bytes());
        gif.extend_from_slice(&80u16.to_le_bytes());
        assert_eq!(image_dimensions(&gif), Some((120, 80)));

        assert_eq!(image_dimensions(b"<svg></svg>"), None);
    }

    #[test]
    fn apply_ops_reports_failed_index() {
        let mut l = layout_with(vec![rect(A, 0)]);
        let ops = vec![
            LayoutOp::Move {
                id: A.to_string(),
                x: 5.0,
                y: 5.0,
                force: false,
            },
            LayoutOp::Move {
                id: B.to_string(),
                x: 1.0,
                y: 1.0,
                force: false,
            },
        ];
        let (idx, err) = apply_ops(&mut l, &ops).unwrap_err();
        assert_eq!(idx, 1);
        assert_eq!(err.code, "item_not_found");
    }
}
