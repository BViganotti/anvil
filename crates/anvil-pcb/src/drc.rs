//! Design rule checking.
//!
//! Every check is real geometry: no simulated passes, no fabricated results.
//! Reports are deterministic and sorted.

use crate::board::{Board, BoardError, DesignRules, Layer, Pad, Trace, Via};
use crate::connect::{ratsnest, Connectivity};
use crate::geom::{
    pad_bbox, pad_center, pad_pad_within, pad_trace_within, point_boundary_dist2,
    segment_boundary_dist2, segment_inside_polygon, via_bbox, via_pad_within, via_trace_within,
    via_via_within,
};
use anvil_core::geom::{point_in_polygon, Point, Rect};
use anvil_core::netlist::Severity;
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DrcKind {
    Structure,
    Clearance,
    Short,
    TraceWidth,
    Drill,
    AnnularRing,
    BoardEdge,
    OutsideBoard,
    Unrouted,
    ViaDrill,
    ViaAnnular,
}

impl DrcKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DrcKind::Structure => "structure",
            DrcKind::Clearance => "clearance",
            DrcKind::Short => "short",
            DrcKind::TraceWidth => "trace_width",
            DrcKind::Drill => "drill",
            DrcKind::AnnularRing => "annular_ring",
            DrcKind::BoardEdge => "board_edge",
            DrcKind::OutsideBoard => "outside_board",
            DrcKind::Unrouted => "unrouted",
            DrcKind::ViaDrill => "via_drill",
            DrcKind::ViaAnnular => "via_annular",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub kind: DrcKind,
    pub severity: Severity,
    pub layer: Option<Layer>,
    pub at: Point,
    pub message: String,
    pub net_a: Option<usize>,
    pub net_b: Option<usize>,
    pub refs: Vec<String>,
}

impl Violation {
    pub fn new(kind: DrcKind, at: Point, message: impl Into<String>) -> Self {
        Violation {
            kind,
            severity: Severity::Error,
            layer: None,
            at,
            message: message.into(),
            net_a: None,
            net_b: None,
            refs: Vec::new(),
        }
    }

    pub fn with_layer(mut self, layer: Layer) -> Self {
        self.layer = Some(layer);
        self
    }

    pub fn with_nets(mut self, a: Option<usize>, b: Option<usize>) -> Self {
        self.net_a = a;
        self.net_b = b;
        self
    }

    pub fn with_refs(mut self, refs: Vec<String>) -> Self {
        self.refs = refs;
        self
    }

    pub fn warning(mut self) -> Self {
        self.severity = Severity::Warning;
        self
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}] at ({},{}): {}",
            self.severity,
            self.kind.as_str(),
            self.at.x,
            self.at.y,
            self.message
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct DrcReport {
    pub violations: Vec<Violation>,
    pub checked_items: usize,
    pub nets: usize,
    pub unrouted: usize,
}

impl DrcReport {
    pub fn errors(&self) -> usize {
        self.violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.violations
            .iter()
            .filter(|v| v.severity == Severity::Warning)
            .count()
    }

    pub fn is_clean(&self) -> bool {
        self.errors() == 0
    }

    pub fn by_kind(&self, kind: DrcKind) -> usize {
        self.violations.iter().filter(|v| v.kind == kind).count()
    }

    pub fn counts(&self) -> Vec<(DrcKind, usize)> {
        let mut kinds: Vec<DrcKind> = self.violations.iter().map(|v| v.kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        kinds
            .into_iter()
            .map(|k| (k, self.by_kind(k)))
            .collect()
    }
}

#[derive(Clone, Copy)]
enum ShapeRef<'a> {
    Pad(&'a Pad),
    Trace(&'a Trace),
    Via(&'a Via),
}

struct Item<'a> {
    id: usize,
    net: Option<usize>,
    shape: ShapeRef<'a>,
    bbox: Rect,
    label: String,
}

impl<'a> Item<'a> {
    fn boundary_point(&self) -> Point {
        match self.shape {
            ShapeRef::Pad(p) => pad_center(p),
            ShapeRef::Trace(t) => {
                let n = t.points.len();
                if n == 0 {
                    Point::ORIGIN
                } else {
                    let a = t.points[n / 2 - usize::from(n > 1)];
                    let b = t.points[n / 2];
                    Point::new((a.x + b.x) / 2, (a.y + b.y) / 2)
                }
            }
            ShapeRef::Via(v) => v.at,
        }
    }
}

fn different_net(a: Option<usize>, b: Option<usize>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x != y,
        _ => true,
    }
}

fn within(a: &Item<'_>, b: &Item<'_>, gap: i64) -> bool {
    match (a.shape, b.shape) {
        (ShapeRef::Pad(pa), ShapeRef::Pad(pb)) => pad_pad_within(pa, pb, gap),
        (ShapeRef::Pad(p), ShapeRef::Trace(t)) => t
            .segments()
            .any(|(s, e)| pad_trace_within(p, s, e, t.width / 2, gap)),
        (ShapeRef::Trace(t), ShapeRef::Pad(p)) => t
            .segments()
            .any(|(s, e)| pad_trace_within(p, s, e, t.width / 2, gap)),
        (ShapeRef::Pad(p), ShapeRef::Via(v)) => via_pad_within(v, p, gap),
        (ShapeRef::Via(v), ShapeRef::Pad(p)) => via_pad_within(v, p, gap),
        (ShapeRef::Trace(t1), ShapeRef::Trace(t2)) => {
            if t1.layer != t2.layer {
                return false;
            }
            t1.segments().any(|(a1, a2)| {
                t2.segments()
                    .any(|(b1, b2)| crate::geom::trace_trace_within(a1, a2, t1.width, b1, b2, t2.width, gap))
            })
        }
        (ShapeRef::Trace(t), ShapeRef::Via(v)) => {
            if !v.spans(t.layer) {
                return false;
            }
            t.segments()
                .any(|(s, e)| via_trace_within(v, s, e, t.width / 2, gap))
        }
        (ShapeRef::Via(v), ShapeRef::Trace(t)) => {
            if !v.spans(t.layer) {
                return false;
            }
            t.segments()
                .any(|(s, e)| via_trace_within(v, s, e, t.width / 2, gap))
        }
        (ShapeRef::Via(a), ShapeRef::Via(b)) => {
            let overlap = a.from.index().max(b.from.index()) <= a.to.index().min(b.to.index());
            overlap && via_via_within(a, b, gap)
        }
    }
}

fn pair_at(a: &Item<'_>, b: &Item<'_>) -> Point {
    let pa = a.boundary_point();
    let pb = b.boundary_point();
    Point::new(
        (pa.x / 2).saturating_add(pb.x / 2),
        (pa.y / 2).saturating_add(pb.y / 2),
    )
}

fn cell_key(p: Point, cell: i64) -> (i32, i32) {
    let cx = (p.x / cell).clamp(-1_000_000, 1_000_000) as i32;
    let cy = (p.y / cell).clamp(-1_000_000, 1_000_000) as i32;
    (cx, cy)
}

fn insert_grid(
    grid: &mut HashMap<(i32, i32), Vec<usize>>,
    idx: usize,
    bbox: Rect,
    cell: i64,
) {
    let min = cell_key(bbox.min, cell);
    let max = cell_key(bbox.max, cell);
    let cells = (max.0 - min.0 + 1) as i64 * (max.1 - min.1 + 1) as i64;
    if cells > 8192 || cells < 0 {
        // Degenerate or huge item: put it in a sentinel bucket that every query
        // scans. This keeps memory bounded for malformed input.
        grid.entry((i32::MIN, i32::MIN)).or_default().push(idx);
        return;
    }
    for y in min.1..=max.1 {
        for x in min.0..=max.0 {
            grid.entry((x, y)).or_default().push(idx);
        }
    }
}

fn query_grid(
    grid: &HashMap<(i32, i32), Vec<usize>>,
    bbox: Rect,
    cell: i64,
    out: &mut Vec<usize>,
) {
    let min = cell_key(bbox.min, cell);
    let max = cell_key(bbox.max, cell);
    for y in min.1..=max.1 {
        for x in min.0..=max.0 {
            if let Some(v) = grid.get(&(x, y)) {
                out.extend_from_slice(v);
            }
        }
    }
    if let Some(v) = grid.get(&(i32::MIN, i32::MIN)) {
        out.extend_from_slice(v);
    }
    out.sort_unstable();
    out.dedup();
}

fn check_board_edge(
    board: &Board,
    violations: &mut Vec<Violation>,
    rules: &DesignRules,
) {
    let poly = &board.outline;
    let edge = rules.edge_clearance;
    for f in &board.footprints {
        for p in &f.pads {
            let c = pad_center(p);
            if !point_in_polygon(c, poly) {
                violations.push(
                    Violation::new(
                        DrcKind::OutsideBoard,
                        c,
                        format!("pad {} is outside the board outline", p.label(&f.refdes)),
                    )
                    .with_refs(vec![p.label(&f.refdes)]),
                );
                continue;
            }
            let need = (p.size.0.max(p.size.1) / 2) as f64 / 1e6 + edge as f64 / 1e6;
            let have = (point_boundary_dist2(c, poly) as f64).sqrt() / 1e6;
            if have < need {
                violations.push(
                    Violation::new(
                        DrcKind::BoardEdge,
                        c,
                        format!(
                            "pad {} is {:.3} mm from the board edge (need {:.3} mm)",
                            p.label(&f.refdes),
                            have,
                            need
                        ),
                    )
                    .with_refs(vec![p.label(&f.refdes)]),
                );
            }
        }
    }
    for t in &board.traces {
        let mut outside = false;
        for (a, b) in t.segments() {
            if !segment_inside_polygon(a, b, poly) {
                outside = true;
            }
            let need = t.width as f64 / 2.0 / 1e6 + edge as f64 / 1e6;
            let have = (segment_boundary_dist2(a, b, poly) as f64).sqrt() / 1e6;
            if have < need {
                let mid = Point::new((a.x + b.x) / 2, (a.y + b.y) / 2);
                violations.push(
                    Violation::new(
                        DrcKind::BoardEdge,
                        mid,
                        format!(
                            "trace of net {} is {:.3} mm from the board edge (need {:.3} mm)",
                            board.net_name(t.net),
                            have,
                            need
                        ),
                    )
                    .with_layer(t.layer)
                    .with_nets(Some(t.net), None),
                );
            }
        }
        if outside {
            violations.push(
                Violation::new(
                    DrcKind::OutsideBoard,
                    t.points[0],
                    format!("trace of net {} leaves the board outline", board.net_name(t.net)),
                )
                .with_layer(t.layer)
                .with_nets(Some(t.net), None),
            );
        }
    }
    for v in &board.vias {
        if !point_in_polygon(v.at, poly) {
            violations.push(
                Violation::new(DrcKind::OutsideBoard, v.at, "via is outside the board outline")
                    .with_nets(Some(v.net), None),
            );
            continue;
        }
        let need = v.diameter as f64 / 2.0 / 1e6 + edge as f64 / 1e6;
        let have = (point_boundary_dist2(v.at, poly) as f64).sqrt() / 1e6;
        if have < need {
            violations.push(
                Violation::new(
                    DrcKind::BoardEdge,
                    v.at,
                    format!("via is {:.3} mm from the board edge (need {:.3} mm)", have, need),
                )
                .with_nets(Some(v.net), None),
            );
        }
    }
}

fn check_sizes(board: &Board, violations: &mut Vec<Violation>) {
    let rules = &board.rules;
    for f in &board.footprints {
        for p in &f.pads {
            let label = p.label(&f.refdes);
            if p.drill > 0 {
                if p.drill < rules.min_drill {
                    violations.push(
                        Violation::new(
                            DrcKind::Drill,
                            p.at,
                            format!(
                                "pad {} drill {} is below minimum {}",
                                label, p.drill, rules.min_drill
                            ),
                        )
                        .with_refs(vec![label.clone()]),
                    );
                }
                let ring = (p.size.0.min(p.size.1) - p.drill) / 2;
                if ring < rules.min_annular {
                    violations.push(
                        Violation::new(
                            DrcKind::AnnularRing,
                            p.at,
                            format!(
                                "pad {} annular ring {} is below minimum {}",
                                label, ring, rules.min_annular
                            ),
                        )
                        .with_refs(vec![label.clone()]),
                    );
                }
            }
        }
    }
    for (i, t) in board.traces.iter().enumerate() {
        if t.width < rules.min_trace {
            let at = t.points.first().copied().unwrap_or(Point::ORIGIN);
            violations.push(
                Violation::new(
                    DrcKind::TraceWidth,
                    at,
                    format!(
                        "trace {} width {} is below minimum {}",
                        i, t.width, rules.min_trace
                    ),
                )
                .with_layer(t.layer)
                .with_nets(Some(t.net), None),
            );
        }
    }
    for (i, v) in board.vias.iter().enumerate() {
        if v.drill < rules.via_drill {
            violations.push(
                Violation::new(
                    DrcKind::ViaDrill,
                    v.at,
                    format!(
                        "via {} drill {} is below minimum {}",
                        i, v.drill, rules.via_drill
                    ),
                )
                .with_nets(Some(v.net), None),
            );
        }
        let ring = (v.diameter - v.drill) / 2;
        if ring < rules.min_annular {
            violations.push(
                Violation::new(
                    DrcKind::ViaAnnular,
                    v.at,
                    format!(
                        "via {} annular ring {} is below minimum {}",
                        i, ring, rules.min_annular
                    ),
                )
                .with_nets(Some(v.net), None),
            );
        }
    }
}

fn check_clearance(board: &Board, violations: &mut Vec<Violation>, checked: &mut usize) {
    let gap = board.rules.min_clearance;
    let cell = (4 * crate::board::MM).max(gap.saturating_mul(8)).max(1);
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for layer in Layer::all() {
        if !board.layer_enabled(layer) {
            continue;
        }
        let mut items: Vec<Item<'_>> = Vec::new();
        let mut pad_base = 0usize;
        for f in &board.footprints {
            for p in &f.pads {
                let id = pad_base;
                pad_base += 1;
                if !crate::geom::pad_on_layer(p, layer) {
                    continue;
                }
                items.push(Item {
                    id,
                    net: p.net,
                    shape: ShapeRef::Pad(p),
                    bbox: pad_bbox(p),
                    label: p.label(&f.refdes),
                });
            }
        }
        let trace_base = pad_base;
        for (ti, t) in board.traces.iter().enumerate() {
            if t.layer != layer {
                continue;
            }
            items.push(Item {
                id: trace_base + ti,
                net: Some(t.net),
                shape: ShapeRef::Trace(t),
                bbox: t.bounds(),
                label: format!("trace {}", ti),
            });
        }
        let via_base = trace_base + board.traces.len();
        for (vi, v) in board.vias.iter().enumerate() {
            if !v.spans(layer) {
                continue;
            }
            items.push(Item {
                id: via_base + vi,
                net: Some(v.net),
                shape: ShapeRef::Via(v),
                bbox: via_bbox(v),
                label: format!("via {}", vi),
            });
        }
        *checked += items.len();

        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, item) in items.iter().enumerate() {
            insert_grid(&mut grid, i, item.bbox.expand(gap), cell);
        }
        let mut candidates: Vec<usize> = Vec::new();
        for i in 0..items.len() {
            candidates.clear();
            query_grid(&mut grid, items[i].bbox.expand(gap), cell, &mut candidates);
            for &j in &candidates {
                if j <= i {
                    continue;
                }
                let a = &items[i];
                let b = &items[j];
                if !different_net(a.net, b.net) {
                    continue;
                }
                let key = (a.id.min(b.id), a.id.max(b.id));
                if seen.contains(&key) {
                    continue;
                }
                if within(a, b, gap) {
                    seen.insert(key);
                    let short = within(a, b, 0);
                    let kind = if short { DrcKind::Short } else { DrcKind::Clearance };
                    let at = pair_at(a, b);
                    let msg = if short {
                        format!(
                            "{} and {} overlap on net {} / {}",
                            a.label,
                            b.label,
                            a.net.map(|n| board.net_name(n)).unwrap_or("-"),
                            b.net.map(|n| board.net_name(n)).unwrap_or("-")
                        )
                    } else {
                        format!(
                            "{} and {} are closer than {:.3} mm (nets {} / {})",
                            a.label,
                            b.label,
                            gap as f64 / 1e6,
                            a.net.map(|n| board.net_name(n)).unwrap_or("-"),
                            b.net.map(|n| board.net_name(n)).unwrap_or("-")
                        )
                    };
                    violations.push(
                        Violation::new(kind, at, msg)
                            .with_layer(layer)
                            .with_nets(a.net, b.net)
                            .with_refs(vec![a.label.clone(), b.label.clone()]),
                    );
                }
            }
        }
    }
}

fn check_unrouted(board: &Board, violations: &mut Vec<Violation>) -> usize {
    let edges = ratsnest(board);
    for e in &edges {
        violations.push(
            Violation::new(
                DrcKind::Unrouted,
                Point::new((e.a.x / 2).saturating_add(e.b.x / 2), (e.a.y / 2).saturating_add(e.b.y / 2)),
                format!(
                    "unrouted connection on net {} between {} and {}",
                    e.net_name, e.a_label, e.b_label
                ),
            )
            .with_nets(Some(e.net), None)
            .with_refs(vec![e.a_label.clone(), e.b_label.clone()]),
        );
    }
    edges.len()
}

/// Run all DRC checks on a board.
pub fn check(board: &Board) -> DrcReport {
    let mut report = DrcReport {
        nets: board.nets.len(),
        ..DrcReport::default()
    };
    if let Err(e) = board.validate_structure() {
        report.violations.push(Violation::new(
            DrcKind::Structure,
            board.outline.first().copied().unwrap_or(Point::ORIGIN),
            format!("{e}"),
        ));
        return report;
    }
    let mut violations = Vec::new();
    let mut checked = 0usize;
    check_sizes(board, &mut violations);
    check_board_edge(board, &mut violations, &board.rules);
    check_clearance(board, &mut violations, &mut checked);
    report.unrouted = check_unrouted(board, &mut violations);
    report.checked_items = checked;

    violations.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .reverse()
            .then(a.kind.cmp(&b.kind))
            .then(a.layer.map(|l| l.index()).cmp(&b.layer.map(|l| l.index())))
            .then(a.at.x.cmp(&b.at.x))
            .then(a.at.y.cmp(&b.at.y))
            .then(a.message.cmp(&b.message))
    });
    report.violations = violations;
    report
}

/// Convenience: true when there are no errors.
pub fn is_clean(board: &Board) -> bool {
    check(board).is_clean()
}

/// Connectivity helper exposed for the router so it can verify every net.
pub fn all_nets_connected(board: &Board) -> bool {
    let mut conn = Connectivity::build(board);
    for net in 0..board.nets.len() {
        let groups = crate::connect::pad_groups(board, net, &mut conn);
        let pads = board
            .pads()
            .filter(|(_, p)| p.net == Some(net))
            .count();
        if pads > 0 && groups.len() > 1 {
            return false;
        }
    }
    true
}

/// Turn a structural board error into a printable diagnostic.
pub fn structure_error(e: BoardError) -> String {
    format!("{e}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Board, DesignRules, Layer, MM};
    use anvil_core::geom::Point;
    use anvil_core::library::Library;

    fn two_resistor_board() -> Board {
        let lib = Library::standard();
        let mut b = Board::new_rect(30 * MM, 30 * MM, 2, DesignRules::default());
        let n1 = b.add_net("N1");
        let n2 = b.add_net("N2");
        let fp = lib.footprint("R_0805").unwrap();
        b.add_footprint(fp, "R1", Point::new(8 * MM, 10 * MM), 0, &[Some(n1), Some(n2)])
            .unwrap();
        b.add_footprint(fp, "R2", Point::new(16 * MM, 18 * MM), 0, &[Some(n1), Some(n2)])
            .unwrap();
        b
    }

    #[test]
    fn unrouted_reported() {
        let b = two_resistor_board();
        let r = check(&b);
        assert!(!r.is_clean());
        assert!(r.by_kind(DrcKind::Unrouted) >= 2);
    }

    #[test]
    fn routed_board_clean() {
        let lib = Library::standard();
        let mut b = Board::new_rect(30 * MM, 30 * MM, 2, DesignRules::default());
        let n = b.add_net("N");
        let fp = lib.footprint("R_0805").unwrap();
        b.add_footprint(fp, "R1", Point::new(8 * MM, 10 * MM), 0, &[Some(n), Some(n)])
            .unwrap();
        b.add_footprint(fp, "R2", Point::new(20 * MM, 20 * MM), 0, &[Some(n), Some(n)])
            .unwrap();
        let p1 = b.footprints[0].pads[0].at;
        let p2 = b.footprints[1].pads[0].at;
        b.add_trace(n, Layer::Top, 200_000, vec![p1, p2]).unwrap();
        let q1 = b.footprints[0].pads[1].at;
        let q2 = b.footprints[1].pads[1].at;
        b.add_trace(n, Layer::Top, 200_000, vec![q1, q2]).unwrap();
        b.add_trace(n, Layer::Top, 200_000, vec![p1, q1]).unwrap();
        assert!(ratsnest(&b).is_empty());
        let r = check(&b);
        assert!(r.is_clean(), "{:?}", r.violations);
    }

    #[test]
    fn clearance_violation_detected() {
        let mut b = Board::new_rect(30 * MM, 30 * MM, 2, DesignRules::default());
        let n1 = b.add_net("A");
        let n2 = b.add_net("B");
        b.add_trace(
            n1,
            Layer::Top,
            200_000,
            vec![Point::new(5 * MM, 5 * MM), Point::new(20 * MM, 5 * MM)],
        )
        .unwrap();
        // 0.3 mm below: edge gap 0.3 - 0.1 - 0.1 = 0.1 mm < 0.2 mm clearance.
        b.add_trace(
            n2,
            Layer::Top,
            200_000,
            vec![Point::new(5 * MM, 5_300_000), Point::new(20 * MM, 5_300_000)],
        )
        .unwrap();
        let r = check(&b);
        assert!(r.by_kind(DrcKind::Clearance) >= 1);
    }
}
