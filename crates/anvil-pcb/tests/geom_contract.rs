//! PCB shape and clearance predicate contract.

use anvil_core::geom::Point;
use anvil_core::library::{PadLayer, PadShape};
use anvil_pcb::board::{Pad, Via, Layer, MM};
use anvil_pcb::geom::{
    pad_bbox, pad_contains, pad_on_layer, pad_pad_within, pad_trace_within, point_boundary_dist2,
    segment_inside_polygon, via_pad_within, via_trace_within, via_via_within,
};

fn pad(at: Point, w: i64, h: i64, shape: PadShape) -> Pad {
    Pad {
        number: "1".into(),
        at,
        offset: Point::ORIGIN,
        size: (w, h),
        shape,
        drill: 0,
        layers: PadLayer::Top,
        net: Some(0),
    }
}

fn via(at: Point) -> Via {
    Via {
        net: 0,
        at,
        drill: 400_000,
        diameter: 800_000,
        from: Layer::Top,
        to: Layer::Bottom,
    }
}

#[test]
fn pad_shapes_contain_points() {
    let circle = pad(Point::ORIGIN, MM, MM, PadShape::Circle);
    assert!(pad_contains(&circle, Point::new(400_000, 0)));
    assert!(!pad_contains(&circle, Point::new(600_000, 0)));
    let rect = pad(Point::ORIGIN, MM, MM / 2, PadShape::Rect);
    assert!(pad_contains(&rect, Point::new(490_000, 240_000)));
    assert!(!pad_contains(&rect, Point::new(0, 260_000)));
    let bbox = pad_bbox(&rect);
    assert_eq!(bbox.width(), MM);
}

#[test]
fn layer_sets() {
    let mut p = pad(Point::ORIGIN, MM, MM, PadShape::Rect);
    assert!(pad_on_layer(&p, Layer::Top));
    assert!(!pad_on_layer(&p, Layer::Bottom));
    p.layers = PadLayer::Through;
    assert!(pad_on_layer(&p, Layer::Bottom));
    assert!(pad_on_layer(&p, Layer::Inner1));
    let v = via(Point::ORIGIN);
    assert!(v.spans(Layer::Top) && v.spans(Layer::Bottom));
}

#[test]
fn pad_and_via_clearances() {
    let a = pad(Point::ORIGIN, MM, MM, PadShape::Rect);
    let b = pad(Point::new(2 * MM, 0), MM, MM, PadShape::Rect);
    assert!(!pad_pad_within(&a, &b, 100_000));
    assert!(pad_pad_within(&a, &b, 1_000_000 + 1));
    let t0 = Point::new(-3 * MM, 0);
    let t1 = Point::new(3 * MM, 0);
    assert!(pad_trace_within(&a, t0, t1, 100_000, 0));
    let v = via(Point::new(2_800_000, 0));
    assert!(via_pad_within(&v, &b, 0));
    assert!(via_trace_within(&v, t0, t1, 100_000, 0));
    assert!(via_via_within(&v, &via(Point::new(3 * MM, 0)), 0));
    assert!(!via_via_within(&v, &via(Point::new(6 * MM, 0)), 0));
}

#[test]
fn trace_trace_distance() {
    use anvil_pcb::geom::trace_trace_within;
    let a0 = Point::new(0, 0);
    let a1 = Point::new(10 * MM, 0);
    let b0 = Point::new(0, MM);
    let b1 = Point::new(10 * MM, MM);
    assert!(!trace_trace_within(a0, a1, 200_000, b0, b1, 200_000, 200_000));
    assert!(trace_trace_within(
        a0,
        a1,
        200_000,
        Point::new(0, 300_000),
        Point::new(10 * MM, 300_000),
        200_000,
        200_000
    ));
}

#[test]
fn polygon_containment_and_distance() {
    let poly = [
        Point::new(0, 0),
        Point::new(10 * MM, 0),
        Point::new(10 * MM, 10 * MM),
        Point::new(0, 10 * MM),
    ];
    assert!(segment_inside_polygon(
        Point::new(MM, MM),
        Point::new(2 * MM, 2 * MM),
        &poly
    ));
    assert!(!segment_inside_polygon(
        Point::new(-MM, MM),
        Point::new(2 * MM, 2 * MM),
        &poly
    ));
    assert_eq!(point_boundary_dist2(Point::new(11 * MM, 5 * MM), &poly), (MM * MM) as i128);
}
