//! `gtmux layout align` — CLI-side alignment / distribution math
//! (ADR-0053 D13: no server align op; ADR-0027 D4~D8 semantics).
//!
//! This is a line-for-line port of the FE canonical implementation
//! `frontend/src/lib/canvas/alignment.ts` (plan-0010 Task 5):
//! * reference frame = union BBox of the selection (D5)
//! * locked items keep their position but count into the BBox (D7)
//! * line items use their endpoint BBox; the caller sends a plain `move`
//!   op and the *server* translates `x2/y2` (and other absolute payloads)
//!   by the same delta, matching FE `moveItem` parity
//! * distribute is defined for N ≥ 3 (D8) — the two extremes stay fixed
//! * sub-0.5px deltas are dropped (idempotent no-op parity with the FE)
//!
//! Pure module — no I/O; `remote.rs` feeds it boxes derived from the
//! fetched layout and turns the deltas into batch `move` ops (atomic — D5).

/// One alignment participant. `x/y/w/h` is the item's *display BBox*
/// (line = endpoint BBox, everything else = common x/y/w/h).
#[derive(Debug, Clone, PartialEq)]
pub struct AlignBox {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub locked: bool,
}

/// CLI `<mode>` argument. Hyphenated names are the CLI surface (ADR-0053
/// D2); `CenterH` aligns horizontal centers (one shared center-x line),
/// `CenterV` aligns vertical centers (one shared center-y line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AlignMode {
    /// Align left edges to the selection BBox left edge.
    Left,
    /// Align right edges to the selection BBox right edge.
    Right,
    /// Align top edges to the selection BBox top edge.
    Top,
    /// Align bottom edges to the selection BBox bottom edge.
    Bottom,
    /// Align horizontal centers (same center x).
    CenterH,
    /// Align vertical centers (same center y).
    CenterV,
    /// Distribute horizontally — extremes fixed, centers evenly spaced (N ≥ 3).
    DistributeH,
    /// Distribute vertically — extremes fixed, centers evenly spaced (N ≥ 3).
    DistributeV,
}

/// FE parity epsilon — deltas below this are treated as "already aligned".
const EPSILON: f64 = 0.5;

/// Union BBox over all boxes (locked included — ADR-0027 D7).
fn union_bbox(boxes: &[AlignBox]) -> Option<(f64, f64, f64, f64)> {
    if boxes.is_empty() {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for b in boxes {
        min_x = min_x.min(b.x);
        min_y = min_y.min(b.y);
        max_x = max_x.max(b.x + b.w);
        max_y = max_y.max(b.y + b.h);
    }
    Some((min_x, min_y, max_x - min_x, max_y - min_y))
}

/// Compute per-id parallel-translation deltas `(id, dx, dy)` for `mode`.
/// Only *changed, unlocked* boxes appear in the output (FE parity).
///
/// Errors are user errors (too few targets) — the caller surfaces them
/// verbatim on stderr.
pub fn compute_deltas(boxes: &[AlignBox], mode: AlignMode) -> Result<Vec<(String, f64, f64)>, String> {
    match mode {
        AlignMode::DistributeH | AlignMode::DistributeV => {
            if boxes.len() < 3 {
                return Err(format!(
                    "distribute needs at least 3 targets (got {}) — ADR-0027 D8",
                    boxes.len()
                ));
            }
            Ok(distribute_deltas(boxes, matches!(mode, AlignMode::DistributeH)))
        }
        _ => {
            if boxes.len() < 2 {
                return Err(format!(
                    "align needs at least 2 targets (got {})",
                    boxes.len()
                ));
            }
            Ok(align_deltas(boxes, mode))
        }
    }
}

/// Port of FE `alignBoxes` — selection-BBox-relative edge/center alignment.
fn align_deltas(boxes: &[AlignBox], mode: AlignMode) -> Vec<(String, f64, f64)> {
    let Some((bx, by, bw, bh)) = union_bbox(boxes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for b in boxes {
        if b.locked {
            continue;
        }
        let (mut dx, mut dy) = (0.0, 0.0);
        match mode {
            AlignMode::Left => dx = bx - b.x,
            AlignMode::CenterH => dx = bx + bw / 2.0 - (b.x + b.w / 2.0),
            AlignMode::Right => dx = bx + bw - (b.x + b.w),
            AlignMode::Top => dy = by - b.y,
            AlignMode::CenterV => dy = by + bh / 2.0 - (b.y + b.h / 2.0),
            AlignMode::Bottom => dy = by + bh - (b.y + b.h),
            AlignMode::DistributeH | AlignMode::DistributeV => unreachable!("routed above"),
        }
        if dx.abs() < EPSILON && dy.abs() < EPSILON {
            continue;
        }
        out.push((b.id.clone(), dx, dy));
    }
    out
}

/// Port of FE `distributeBoxes` — the two extreme centers stay fixed, the
/// intermediates land on an even-step grid between them. Locked
/// intermediates keep their *slot* (the step index) but skip the move.
fn distribute_deltas(boxes: &[AlignBox], horizontal: bool) -> Vec<(String, f64, f64)> {
    let center = |b: &AlignBox| {
        if horizontal {
            b.x + b.w / 2.0
        } else {
            b.y + b.h / 2.0
        }
    };
    let mut sorted: Vec<&AlignBox> = boxes.iter().collect();
    // Stable sort — ties keep input order, matching JS Array.prototype.sort.
    sorted.sort_by(|a, b| {
        center(a)
            .partial_cmp(&center(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let (Some(first), Some(last)) = (sorted.first(), sorted.last()) else {
        return Vec::new();
    };
    let start = center(first);
    let end = center(last);
    let step = (end - start) / (sorted.len() as f64 - 1.0);

    let mut out = Vec::new();
    for (i, b) in sorted.iter().enumerate().take(sorted.len() - 1).skip(1) {
        if b.locked {
            continue;
        }
        let target = start + step * i as f64;
        let (mut dx, mut dy) = (0.0, 0.0);
        if horizontal {
            dx = target - center(b);
        } else {
            dy = target - center(b);
        }
        if dx.abs() < EPSILON && dy.abs() < EPSILON {
            continue;
        }
        out.push((b.id.clone(), dx, dy));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(id: &str, x: f64, y: f64, w: f64, h: f64) -> AlignBox {
        AlignBox {
            id: id.to_string(),
            x,
            y,
            w,
            h,
            locked: false,
        }
    }

    fn deltas_map(v: Vec<(String, f64, f64)>) -> std::collections::HashMap<String, (f64, f64)> {
        v.into_iter().map(|(id, dx, dy)| (id, (dx, dy))).collect()
    }

    #[test]
    fn align_left_moves_to_bbox_min() {
        let boxes = [bx("a", 0.0, 0.0, 10.0, 10.0), bx("b", 50.0, 20.0, 10.0, 10.0)];
        let out = deltas_map(compute_deltas(&boxes, AlignMode::Left).unwrap());
        // "a" is already at the BBox left — dropped (idempotent parity).
        assert!(!out.contains_key("a"));
        assert_eq!(out["b"], (-50.0, 0.0));
    }

    #[test]
    fn align_right_and_bottom() {
        let boxes = [bx("a", 0.0, 0.0, 10.0, 10.0), bx("b", 50.0, 20.0, 20.0, 30.0)];
        // BBox: x 0..70, y 0..50.
        let right = deltas_map(compute_deltas(&boxes, AlignMode::Right).unwrap());
        assert_eq!(right["a"], (60.0, 0.0)); // 70 - (0+10)
        assert!(!right.contains_key("b"));
        let bottom = deltas_map(compute_deltas(&boxes, AlignMode::Bottom).unwrap());
        assert_eq!(bottom["a"], (0.0, 40.0)); // 50 - (0+10)
    }

    #[test]
    fn align_center_h_shares_center_x() {
        let boxes = [bx("a", 0.0, 0.0, 10.0, 10.0), bx("b", 90.0, 0.0, 10.0, 10.0)];
        // BBox x 0..100 → center 50.
        let out = deltas_map(compute_deltas(&boxes, AlignMode::CenterH).unwrap());
        assert_eq!(out["a"], (45.0, 0.0)); // 50 - 5
        assert_eq!(out["b"], (-45.0, 0.0)); // 50 - 95
    }

    #[test]
    fn locked_excluded_from_moves_but_included_in_bbox() {
        let mut locked = bx("l", 100.0, 0.0, 10.0, 10.0);
        locked.locked = true;
        let boxes = [bx("a", 0.0, 0.0, 10.0, 10.0), locked];
        // BBox right edge = 110 — driven by the locked box.
        let out = deltas_map(compute_deltas(&boxes, AlignMode::Right).unwrap());
        assert_eq!(out["a"], (100.0, 0.0));
        assert!(!out.contains_key("l"));
    }

    #[test]
    fn align_needs_two() {
        let boxes = [bx("a", 0.0, 0.0, 10.0, 10.0)];
        assert!(compute_deltas(&boxes, AlignMode::Left).is_err());
    }

    #[test]
    fn distribute_h_evenly_spaces_centers() {
        let boxes = [
            bx("a", 0.0, 0.0, 10.0, 10.0),   // center 5
            bx("b", 10.0, 0.0, 10.0, 10.0),  // center 15
            bx("c", 100.0, 0.0, 10.0, 10.0), // center 105
        ];
        let out = deltas_map(compute_deltas(&boxes, AlignMode::DistributeH).unwrap());
        // Extremes fixed; "b" center → 5 + (105-5)/2 = 55 → dx 40.
        assert!(!out.contains_key("a"));
        assert!(!out.contains_key("c"));
        assert_eq!(out["b"], (40.0, 0.0));
    }

    #[test]
    fn distribute_v_moves_dy_only() {
        let boxes = [
            bx("a", 0.0, 0.0, 10.0, 10.0),
            bx("b", 0.0, 12.0, 10.0, 10.0),
            bx("c", 0.0, 100.0, 10.0, 10.0),
        ];
        let out = deltas_map(compute_deltas(&boxes, AlignMode::DistributeV).unwrap());
        // Centers 5 / 17 / 105 → "b" target 55 → dy 38.
        assert_eq!(out["b"], (0.0, 38.0));
    }

    #[test]
    fn distribute_needs_three() {
        let boxes = [bx("a", 0.0, 0.0, 1.0, 1.0), bx("b", 5.0, 0.0, 1.0, 1.0)];
        assert!(compute_deltas(&boxes, AlignMode::DistributeH).is_err());
    }

    #[test]
    fn distribute_skips_locked_intermediate_but_keeps_its_slot() {
        let mut locked = bx("b", 10.0, 0.0, 10.0, 10.0);
        locked.locked = true;
        let boxes = [
            bx("a", 0.0, 0.0, 10.0, 10.0),   // center 5
            locked,                          // center 15 (slot 1)
            bx("c", 40.0, 0.0, 10.0, 10.0),  // center 45 (slot 2)
            bx("d", 100.0, 0.0, 10.0, 10.0), // center 105
        ];
        let out = deltas_map(compute_deltas(&boxes, AlignMode::DistributeH).unwrap());
        assert!(!out.contains_key("b"), "locked intermediate must not move");
        // step = (105-5)/3 ≈ 33.333 → "c" target ≈ 71.667 → dx ≈ 26.667.
        let (dx, dy) = out["c"];
        assert!((dx - 26.666_666_666_666_668).abs() < 1e-9);
        assert_eq!(dy, 0.0);
    }

    #[test]
    fn sub_epsilon_deltas_dropped() {
        let boxes = [bx("a", 0.0, 0.0, 10.0, 10.0), bx("b", 0.3, 0.0, 10.0, 10.0)];
        let out = compute_deltas(&boxes, AlignMode::Left).unwrap();
        assert!(out.is_empty(), "0.3px drift is treated as aligned");
    }
}
