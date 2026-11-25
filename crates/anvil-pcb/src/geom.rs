//! PCB geometry helpers: pad shapes, clearance predicates and board polygon
//! containment. All predicates are exact integer comparisons against a squared
//! threshold, so DRC results are deterministic.

use crate::board::{Pad, Via, Layer};
use anvil_core::geom::{
    point_in_polygon, point_segment_dist2, segment_segment_dist2, segments_intersect, Point, Rect,
};
use anvil_core::library::PadShape;

pub fn pad_center(pad: &Pad) -> Point {
    pad.at
}

pub fn pad_radius(pad: &Pad) -> i64 {
    match pad.shape {
        PadShape::Circle => pad.size.0.min(pad.size.1) / 2,
        _ => pad.size.0.min(pad.size.1) / 2,
    }
}

pub fn pad_bbox(pad: &Pad) -> Rect {
    Rect::new(
        Point::new(pad.at.x - pad.size.0 / 2, pad.at.y - pad.size.1 / 2),
        Point::new(pad.at.x + pad.size.0 / 2, pad.at.y + pad.size.1 / 2),
    )
}

pub fn pad_rect(pad: &Pad) -> Rect {
    pad_bbox(pad)
}

pub fn pad_contains(pad: &Pad, p: Point) -> bool {
    match pad.shape {
        PadShape::Circle => p.dist2(pad.at) <= (pad_radius(pad) as i128).pow(2),
        _ => pad_bbox(pad).contains(p),
    }
}

pub fn pad_on_layer(pad: &Pad, layer: Layer) -> bool {
    match pad.layers {
        anvil_core::library::PadLayer::Top => layer == Layer::Top,
        anvil_core::library::PadLayer::Bottom => layer == Layer::Bottom,
        anvil_core::library::PadLayer::Through => layer.index() < 4,
    }
}

pub fn via_bbox(via: &Via) -> Rect {
    let r = via.diameter / 2;
    Rect::new(
        Point::new(via.at.x - r, via.at.y - r),
        Point::new(via.at.x + r, via.at.y + r),
    )
}

pub fn via_on_layer(via: &Via, layer: Layer) -> bool {
    let i = layer.index();
    i >= via.from.index() && i <= via.to.index()
}

fn rect_segment_dist2(r: Rect, a: Point, b: Point) -> i128 {
    if r.contains(a) || r.contains(b) {
        return 0;
    }
    let corners = [
        r.min,
        Point::new(r.max.x, r.min.y),
        r.max,
        Point::new(r.min.x, r.max.y),
    ];
    let mut best = i128::MAX;
    for i in 0..4 {
        let c = corners[i];
        let d = corners[(i + 1) % 4];
        if segments_intersect(a, b, c, d) {
            return 0;
        }
        best = best.min(segment_segment_dist2(a, b, c, d));
    }
    best.min(r.dist2_point(a)).min(r.dist2_point(b))
}

/// True if the pad touches the trace capsule (segment expanded by `half`) with
/// `gap` additional clearance.
pub fn pad_trace_within(pad: &Pad, a: Point, b: Point, half: i64, gap: i64) -> bool {
    match pad.shape {
        PadShape::Circle => {
            let rr = pad_radius(pad).saturating_add(half).saturating_add(gap);
            point_segment_dist2(pad.at, a, b) <= (rr as i128).pow(2)
        }
        _ => {
            let extra = half.saturating_add(gap);
            rect_segment_dist2(pad_rect(pad), a, b) <= (extra as i128).pow(2)
        }
    }
}

pub fn pad_pad_within(a: &Pad, b: &Pad, gap: i64) -> bool {
    match (a.shape, b.shape) {
        (PadShape::Circle, PadShape::Circle) => {
            let r = pad_radius(a)
                .saturating_add(pad_radius(b))
                .saturating_add(gap);
            a.at.dist2(b.at) <= (r as i128).pow(2)
        }
        (PadShape::Circle, _) => {
            let r = pad_radius(a).saturating_add(gap);
            pad_rect(b).dist2_point(a.at) <= (r as i128).pow(2)
        }
        (_, PadShape::Circle) => {
            let r = pad_radius(b).saturating_add(gap);
            pad_rect(a).dist2_point(b.at) <= (r as i128).pow(2)
        }
        _ => pad_rect(a).dist2_rect(&pad_rect(b)) <= (gap as i128).pow(2),
    }
}

pub fn trace_trace_within(
    a1: Point,
    a2: Point,
    wa: i64,
    b1: Point,
    b2: Point,
    wb: i64,
    gap: i64,
) -> bool {
    let r = wa / 2 + wb / 2 + gap;
    segment_segment_dist2(a1, a2, b1, b2) <= (r as i128).pow(2)
}

pub fn via_trace_within(via: &Via, a: Point, b: Point, half: i64, gap: i64) -> bool {
    let rr = via.diameter / 2 + half + gap;
    point_segment_dist2(via.at, a, b) <= (rr as i128).pow(2)
}

pub fn via_pad_within(via: &Via, pad: &Pad, gap: i64) -> bool {
    let r = via.diameter / 2 + gap;
    match pad.shape {
        PadShape::Circle => {
            let rr = r + pad_radius(pad);
            via.at.dist2(pad.at) <= (rr as i128).pow(2)
        }
        _ => pad_rect(pad).dist2_point(via.at) <= (r as i128).pow(2),
    }
}

pub fn via_via_within(a: &Via, b: &Via, gap: i64) -> bool {
    let r = a.diameter / 2 + b.diameter / 2 + gap;
    a.at.dist2(b.at) <= (r as i128).pow(2)
}

/// True if a segment is fully inside the polygon and does not cross its
/// boundary. Touching the boundary is treated as crossing.
pub fn segment_inside_polygon(a: Point, b: Point, poly: &[Point]) -> bool {
    if !point_in_polygon(a, poly) || !point_in_polygon(b, poly) {
        return false;
    }
    for i in 0..poly.len() {
        let c = poly[i];
        let d = poly[(i + 1) % poly.len()];
        if segments_intersect(a, b, c, d) {
            return false;
        }
    }
    true
}

pub fn point_boundary_dist2(p: Point, poly: &[Point]) -> i128 {
    if poly.len() < 2 {
        return 0;
    }
    let mut best = i128::MAX;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        best = best.min(point_segment_dist2(p, a, b));
    }
    best
}

pub fn segment_boundary_dist2(a: Point, b: Point, poly: &[Point]) -> i128 {
    let mut best = i128::MAX;
    for i in 0..poly.len() {
        let c = poly[i];
        let d = poly[(i + 1) % poly.len()];
        best = best.min(segment_segment_dist2(a, b, c, d));
    }
    best
}

pub fn polygon_bounds(poly: &[Point]) -> Rect {
    Rect::from_points(poly).unwrap_or(Rect::new(Point::ORIGIN, Point::ORIGIN))
}

/// Bounding box of a pad, a trace segment or a via. Used by the DRC grid.
pub fn union_rect(a: Rect, b: Rect) -> Rect {
    a.union(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::library::{PadLayer, PadShape};

    fn pad(x: i64, y: i64, w: i64, h: i64, shape: PadShape) -> Pad {
        Pad {
            number: "1".into(),
            at: Point::new(x, y),
            offset: Point::ORIGIN,
            size: (w, h),
            shape,
            drill: 0,
            layers: PadLayer::Top,
            net: Some(0),
        }
    }

    #[test]
    fn pad_trace_clearance() {
        let p = pad(0, 0, 1_000_000, 1_000_000, PadShape::Rect);
        // Trace centerline 0.8 mm above pad edge at y=+0.5 mm: gap 0.3 mm.
        assert!(!pad_trace_within(
            &p,
            Point::new(-2_000_000, 800_000),
            Point::new(2_000_000, 800_000),
            100_000,
            150_000
        ));
        assert!(pad_trace_within(
            &p,
            Point::new(-2_000_000, 700_000),
            Point::new(2_000_000, 700_000),
            100_000,
            150_000
        ));
    }

    #[test]
    fn circle_pad_distance() {
        let p = pad(0, 0, 1_000_000, 1_000_000, PadShape::Circle);
        assert!(pad_trace_within(
            &p,
            Point::new(490_000, 0),
            Point::new(2_000_000, 0),
            100_000,
            0
        ));
        assert!(!pad_trace_within(
            &p,
            Point::new(700_000, 0),
            Point::new(2_000_000, 0),
            50_000,
            0
        ));
    }
}
