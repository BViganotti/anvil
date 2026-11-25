//! Geometry edge cases that the inline tests do not cover.

use anvil_core::geom::{
    point_in_polygon, point_on_segment, point_polygon_dist2, point_segment_dist2,
    polygon_area2, segment_segment_dist2, segments_intersect, Point, Rect, MAX_COORD,
};

#[test]
fn collinear_overlap_and_touching() {
    let a = Point::new(0, 0);
    let b = Point::new(100, 0);
    assert!(point_on_segment(Point::new(50, 0), a, b));
    assert!(!point_on_segment(Point::new(101, 0), a, b));
    assert!(!point_on_segment(Point::new(50, 1), a, b));
    assert!(segments_intersect(a, b, Point::new(100, 0), Point::new(200, 0)));
    assert!(segments_intersect(a, b, Point::new(5, -5), Point::new(5, 5)));
    assert!(!segments_intersect(a, b, Point::new(5, 1), Point::new(5, 5)));
    assert_eq!(
        segment_segment_dist2(a, b, Point::new(50, 10), Point::new(50, 20)),
        100
    );
}

#[test]
fn degenerate_segments_and_points() {
    let p = Point::new(3, 4);
    assert_eq!(point_segment_dist2(p, p, p), 0);
    assert_eq!(point_segment_dist2(Point::ORIGIN, Point::ORIGIN, Point::new(0, 0)), 0);
    assert_eq!(segment_segment_dist2(p, p, Point::ORIGIN, Point::ORIGIN), 25);
}

#[test]
fn rectangles_union_and_expand() {
    let a = Rect::new(Point::new(0, 0), Point::new(10, 10));
    let b = Rect::new(Point::new(-5, 5), Point::new(5, 20));
    let u = a.union(b);
    assert_eq!(u.min, Point::new(-5, 0));
    assert_eq!(u.max, Point::new(10, 20));
    assert!(a.expand(2).contains(Point::new(-2, 12)));
    assert_eq!(a.center(), Point::new(5, 5));
    assert_eq!(a.width(), 10);
    assert_eq!(a.height(), 10);
}

#[test]
fn polygon_edges_and_area() {
    let square = [
        Point::new(0, 0),
        Point::new(10, 0),
        Point::new(10, 10),
        Point::new(0, 10),
    ];
    assert_eq!(polygon_area2(&square), 200);
    assert!(point_in_polygon(Point::new(5, 5), &square));
    assert!(!point_in_polygon(Point::new(-1, 5), &square));
    assert_eq!(point_polygon_dist2(Point::new(11, 5), &square), 1);
    // Reversed winding has negative area.
    let rev: Vec<Point> = square.iter().rev().copied().collect();
    assert_eq!(polygon_area2(&rev), -200);
}

#[test]
fn coordinate_limit() {
    assert!(Point::new(MAX_COORD, -MAX_COORD).is_valid());
    assert!(!Point::new(MAX_COORD + 1, 0).is_valid());
    assert_eq!(Point::new(5, 0).rotate(180), Point::new(-5, 0));
    assert_eq!(Point::new(5, 0).rotate(45), Point::new(5, 0));
}
