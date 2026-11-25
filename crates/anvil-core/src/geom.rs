//! Integer geometry shared by the schematic, PCB and export layers.
//!
//! All coordinates are nanometres (1 nm = 1e-9 m). Using integers everywhere
//! keeps connectivity, DRC and Gerber output exact and deterministic; nothing
//! in the toolchain depends on floating point coincidence.
//!
//! Arithmetic that can overflow `i64` is promoted to `i128`. A coordinate
//! beyond ±1e12 nm (1 km) is rejected at the parsing boundary, so squares stay
//! well inside `i128`.

use std::cmp::{max, min};

/// Maximum absolute coordinate accepted by any parser or editor entry point.
pub const MAX_COORD: i64 = 1_000_000_000_000;

/// A point in nanometres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Point {
    pub x: i64,
    pub y: i64,
}

impl Point {
    pub const ORIGIN: Point = Point { x: 0, y: 0 };

    pub const fn new(x: i64, y: i64) -> Self {
        Point { x, y }
    }

    pub fn is_valid(self) -> bool {
        self.x.abs() <= MAX_COORD && self.y.abs() <= MAX_COORD
    }

    pub fn add(self, o: Point) -> Point {
        Point::new(self.x.saturating_add(o.x), self.y.saturating_add(o.y))
    }

    pub fn sub(self, o: Point) -> Point {
        Point::new(self.x.saturating_sub(o.x), self.y.saturating_sub(o.y))
    }

    pub fn scale(self, n: i64) -> Point {
        Point::new(self.x.saturating_mul(n), self.y.saturating_mul(n))
    }

    pub fn dist2(self, o: Point) -> i128 {
        let dx = (self.x as i128) - (o.x as i128);
        let dy = (self.y as i128) - (o.y as i128);
        dx * dx + dy * dy
    }

    pub fn dist(self, o: Point) -> f64 {
        (self.dist2(o) as f64).sqrt()
    }

    pub fn manhattan(self, o: Point) -> i128 {
        ((self.x as i128) - (o.x as i128)).abs() + ((self.y as i128) - (o.y as i128)).abs()
    }

    /// Rotate by a multiple of 90 degrees clockwise around the origin.
    pub fn rotate(self, degrees: u16) -> Point {
        match degrees % 360 {
            0 => self,
            90 => Point::new(self.y, -self.x),
            180 => Point::new(-self.x, -self.y),
            270 => Point::new(-self.y, self.x),
            _ => self,
        }
    }

    /// Rotate by a multiple of 90 degrees around `origin`.
    pub fn rotate_around(self, origin: Point, degrees: u16) -> Point {
        self.sub(origin).rotate(degrees).add(origin)
    }
}

/// Axis aligned rectangle with half-open semantics (`max` is exclusive for
/// containment tests, but inclusive for distance queries).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    pub fn new(min: Point, max: Point) -> Self {
        Rect {
            min: Point::new(min.x.min(max.x), min.y.min(max.y)),
            max: Point::new(min.x.max(max.x), min.y.max(max.y)),
        }
    }

    pub fn from_points(points: &[Point]) -> Option<Rect> {
        let first = *points.first()?;
        let mut r = Rect {
            min: first,
            max: first,
        };
        for p in &points[1..] {
            r.min.x = r.min.x.min(p.x);
            r.min.y = r.min.y.min(p.y);
            r.max.x = r.max.x.max(p.x);
            r.max.y = r.max.y.max(p.y);
        }
        Some(r)
    }

    pub fn width(&self) -> i64 {
        self.max.x.saturating_sub(self.min.x)
    }

    pub fn height(&self) -> i64 {
        self.max.y.saturating_sub(self.min.y)
    }

    pub fn center(&self) -> Point {
        Point::new(
            self.min.x.saturating_add(self.width() / 2),
            self.min.y.saturating_add(self.height() / 2),
        )
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn expand(&self, d: i64) -> Rect {
        Rect {
            min: Point::new(self.min.x.saturating_sub(d), self.min.y.saturating_sub(d)),
            max: Point::new(self.max.x.saturating_add(d), self.max.y.saturating_add(d)),
        }
    }

    pub fn intersects(&self, o: Rect) -> bool {
        self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
    }

    pub fn union(&self, o: Rect) -> Rect {
        Rect {
            min: Point::new(min(self.min.x, o.min.x), min(self.min.y, o.min.y)),
            max: Point::new(max(self.max.x, o.max.x), max(self.max.y, o.max.y)),
        }
    }

    /// Squared distance from a point to the rectangle (zero inside).
    pub fn dist2_point(&self, p: Point) -> i128 {
        let dx = if p.x < self.min.x {
            (self.min.x - p.x) as i128
        } else if p.x > self.max.x {
            (p.x - self.max.x) as i128
        } else {
            0
        };
        let dy = if p.y < self.min.y {
            (self.min.y - p.y) as i128
        } else if p.y > self.max.y {
            (p.y - self.max.y) as i128
        } else {
            0
        };
        dx * dx + dy * dy
    }

    /// Squared distance between two axis aligned rectangles (zero if they
    /// touch or overlap).
    pub fn dist2_rect(&self, o: &Rect) -> i128 {
        let dx = if self.max.x < o.min.x {
            (o.min.x - self.max.x) as i128
        } else if o.max.x < self.min.x {
            (self.min.x - o.max.x) as i128
        } else {
            0
        };
        let dy = if self.max.y < o.min.y {
            (o.min.y - self.max.y) as i128
        } else if o.max.y < self.min.y {
            (self.min.y - o.max.y) as i128
        } else {
            0
        };
        dx * dx + dy * dy
    }
}

fn cross(o: Point, a: Point, b: Point) -> i128 {
    ((a.x as i128) - (o.x as i128)) * ((b.y as i128) - (o.y as i128))
        - ((a.y as i128) - (o.y as i128)) * ((b.x as i128) - (o.x as i128))
}

fn on_segment(a: Point, b: Point, p: Point) -> bool {
    // Assumes p is collinear with a-b.
    p.x >= min(a.x, b.x) && p.x <= max(a.x, b.x) && p.y >= min(a.y, b.y) && p.y <= max(a.y, b.y)
}

/// True if segments `a-b` and `c-d` intersect or touch.
pub fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let d1 = cross(c, d, a);
    let d2 = cross(c, d, b);
    let d3 = cross(a, b, c);
    let d4 = cross(a, b, d);
    if ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0))
        && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))
    {
        return true;
    }
    (d1 == 0 && on_segment(c, d, a))
        || (d2 == 0 && on_segment(c, d, b))
        || (d3 == 0 && on_segment(a, b, c))
        || (d4 == 0 && on_segment(a, b, d))
}

/// Exact test: is `p` collinear with and inside segment `a-b`?
pub fn point_on_segment(p: Point, a: Point, b: Point) -> bool {
    if cross(a, b, p) != 0 {
        return false;
    }
    p.x >= min(a.x, b.x)
        && p.x <= max(a.x, b.x)
        && p.y >= min(a.y, b.y)
        && p.y <= max(a.y, b.y)
}

/// Squared distance from `p` to segment `a-b`.
pub fn point_segment_dist2(p: Point, a: Point, b: Point) -> i128 {
    let abx = (b.x as i128) - (a.x as i128);
    let aby = (b.y as i128) - (a.y as i128);
    let apx = (p.x as i128) - (a.x as i128);
    let apy = (p.y as i128) - (a.y as i128);
    let len2 = abx * abx + aby * aby;
    if len2 == 0 {
        return p.dist2(a);
    }
    let mut t = (apx * abx + apy * aby) as f64 / len2 as f64;
    if !t.is_finite() {
        t = 0.0;
    }
    t = t.clamp(0.0, 1.0);
    // Evaluate the projection in integer space to keep the result exact for
    // grid aligned cases whenever possible.
    let proj = Point::new(
        a.x.saturating_add((t * abx as f64).round() as i64),
        a.y.saturating_add((t * aby as f64).round() as i64),
    );
    p.dist2(proj)
}

/// Squared distance between segments `a-b` and `c-d`.
pub fn segment_segment_dist2(a: Point, b: Point, c: Point, d: Point) -> i128 {
    if segments_intersect(a, b, c, d) {
        return 0;
    }
    point_segment_dist2(a, c, d)
        .min(point_segment_dist2(b, c, d))
        .min(point_segment_dist2(c, a, b))
        .min(point_segment_dist2(d, a, b))
}

/// Orientation of three points: positive for counter-clockwise.
pub fn orient(a: Point, b: Point, c: Point) -> i128 {
    cross(a, b, c)
}

/// Point in polygon test (even-odd rule) for a closed polygon. The polygon is
/// treated as closed even if the last point does not repeat the first.
pub fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let pi = poly[i];
        let pj = poly[j];
        let intersects = (pi.y > p.y) != (pj.y > p.y);
        if intersects {
            let x = (pj.x as i128 - pi.x as i128) * (p.y as i128 - pi.y as i128)
                / (pj.y as i128 - pi.y as i128)
                + pi.x as i128;
            if (p.x as i128) < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Squared distance from a point to a closed polygon boundary.
pub fn point_polygon_dist2(p: Point, poly: &[Point]) -> i128 {
    if poly.len() < 2 {
        return match poly.first() {
            Some(q) => p.dist2(*q),
            None => 0,
        };
    }
    let mut best = i128::MAX;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        best = best.min(point_segment_dist2(p, a, b));
    }
    best
}

/// Signed area (twice the polygon area) using the shoelace formula.
pub fn polygon_area2(poly: &[Point]) -> i128 {
    if poly.len() < 3 {
        return 0;
    }
    let mut s: i128 = 0;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        s += (poly[j].x as i128) * (poly[i].y as i128) - (poly[i].x as i128) * (poly[j].y as i128);
        j = i;
    }
    s
}

/// Rotate a vector by an arbitrary angle in degrees. Used only for display and
/// footprint placement; connectivity checks remain integer based.
pub fn rotate_f64(x: f64, y: f64, degrees: f64) -> (f64, f64) {
    let r = degrees.to_radians();
    (x * r.cos() - y * r.sin(), x * r.sin() + y * r.cos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_distance() {
        assert_eq!(Point::new(3, 4).dist2(Point::ORIGIN), 25);
        assert_eq!(Point::new(-1, 0).manhattan(Point::ORIGIN), 1);
    }

    #[test]
    fn rotation_quarter_turns() {
        let p = Point::new(10, 0);
        assert_eq!(p.rotate(90), Point::new(0, -10));
        assert_eq!(p.rotate(180), Point::new(-10, 0));
        assert_eq!(p.rotate(270), Point::new(0, 10));
        assert_eq!(p.rotate(360), p);
    }

    #[test]
    fn segment_distance() {
        let a = Point::new(0, 0);
        let b = Point::new(100, 0);
        let c = Point::new(50, 10);
        let d = Point::new(50, 20);
        assert_eq!(segment_segment_dist2(a, b, c, d), 100);
        assert_eq!(segment_segment_dist2(a, b, Point::new(50, -5), Point::new(50, 5)), 0);
    }

    #[test]
    fn rectangle_distance() {
        let r = Rect::new(Point::new(0, 0), Point::new(10, 10));
        assert!(r.contains(Point::new(5, 5)));
        assert_eq!(r.dist2_point(Point::new(13, 14)), 25);
        assert_eq!(r.dist2_rect(&Rect::new(Point::new(12, 0), Point::new(20, 10))), 4);
    }

    #[test]
    fn polygon_tests() {
        let poly = [
            Point::new(0, 0),
            Point::new(100, 0),
            Point::new(100, 100),
            Point::new(0, 100),
        ];
        assert!(point_in_polygon(Point::new(50, 50), &poly));
        assert!(!point_in_polygon(Point::new(150, 50), &poly));
        assert_eq!(point_polygon_dist2(Point::new(150, 50), &poly), 2500);
        assert_eq!(polygon_area2(&poly), 20000);
    }
}
