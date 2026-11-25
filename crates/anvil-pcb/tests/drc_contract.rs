//! DRC rule coverage and deterministic reporting.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_pcb::board::{Board, DesignRules, Layer, MM};
use anvil_pcb::drc::{self, DrcKind};

fn base() -> Board {
    Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default())
}

#[test]
fn width_and_drill_rules() {
    let mut b = base();
    let n = b.add_net("N");
    b.add_trace(n, Layer::Top, 100_000, vec![Point::new(10 * MM, 10 * MM), Point::new(20 * MM, 10 * MM)])
        .unwrap();
    let r = drc::check(&b);
    assert!(r.by_kind(DrcKind::TraceWidth) >= 1);
    assert!(!r.is_clean());
}

#[test]
fn drill_and_annular_checks() {
    let lib = Library::standard();
    let mut b = base();
    let n = b.add_net("N");
    b.add_footprint(
        lib.footprint("PINHDR_2").unwrap(),
        "J1",
        Point::new(15 * MM, 15 * MM),
        0,
        &[Some(n), None],
    )
    .unwrap();
    // Through-hole pads are 1.8 mm with a 1.0 mm drill: annular ring 0.4 mm.
    let r = drc::check(&b);
    assert_eq!(r.by_kind(DrcKind::Drill), 0);
    assert_eq!(r.by_kind(DrcKind::AnnularRing), 0);
    b.rules.min_drill = 1_200_000;
    b.rules.min_annular = 500_000;
    let r = drc::check(&b);
    assert!(r.by_kind(DrcKind::Drill) >= 1);
    assert!(r.by_kind(DrcKind::AnnularRing) >= 1);
}

#[test]
fn board_edge_and_outside_checks() {
    let mut b = base();
    let n = b.add_net("N");
    b.add_trace(
        n,
        Layer::Top,
        200_000,
        vec![Point::new(100_000, 3 * MM), Point::new(10 * MM, 3 * MM)],
    )
    .unwrap();
    let r = drc::check(&b);
    assert!(r.by_kind(DrcKind::BoardEdge) >= 1);
    b.add_trace(
        n,
        Layer::Top,
        200_000,
        vec![Point::new(-MM, -MM), Point::new(-2 * MM, -2 * MM)],
    )
    .unwrap();
    let r = drc::check(&b);
    assert!(r.by_kind(DrcKind::OutsideBoard) >= 1);
}

#[test]
fn same_net_overlap_is_not_a_short() {
    let mut b = base();
    let n = b.add_net("N");
    b.add_trace(n, Layer::Top, 200_000, vec![Point::new(10 * MM, 10 * MM), Point::new(20 * MM, 10 * MM)])
        .unwrap();
    b.add_trace(n, Layer::Top, 200_000, vec![Point::new(10 * MM, 10 * MM), Point::new(10 * MM, 20 * MM)])
        .unwrap();
    let r = drc::check(&b);
    assert_eq!(r.by_kind(DrcKind::Short), 0);
    assert_eq!(r.by_kind(DrcKind::Clearance), 0);
}

#[test]
fn different_net_crossing_is_a_short() {
    let mut b = base();
    let a = b.add_net("A");
    let c = b.add_net("B");
    b.add_trace(a, Layer::Top, 200_000, vec![Point::new(10 * MM, 10 * MM), Point::new(20 * MM, 10 * MM)])
        .unwrap();
    b.add_trace(c, Layer::Top, 200_000, vec![Point::new(15 * MM, 5 * MM), Point::new(15 * MM, 15 * MM)])
        .unwrap();
    let r = drc::check(&b);
    assert!(r.by_kind(DrcKind::Short) >= 1);
}

#[test]
fn different_layers_do_not_conflict() {
    let mut b = base();
    let a = b.add_net("A");
    let c = b.add_net("B");
    b.add_trace(a, Layer::Top, 200_000, vec![Point::new(10 * MM, 10 * MM), Point::new(20 * MM, 10 * MM)])
        .unwrap();
    b.add_trace(c, Layer::Bottom, 200_000, vec![Point::new(10 * MM, 10 * MM), Point::new(20 * MM, 10 * MM)])
        .unwrap();
    let r = drc::check(&b);
    assert_eq!(r.by_kind(DrcKind::Clearance), 0);
    assert_eq!(r.by_kind(DrcKind::Short), 0);
}

#[test]
fn report_order_is_stable() {
    let mut b = base();
    let a = b.add_net("A");
    let c = b.add_net("B");
    for i in 0..5 {
        let y = (5 + i) * MM;
        b.add_trace(a, Layer::Top, 100_000, vec![Point::new(5 * MM, y), Point::new(20 * MM, y)])
            .unwrap();
        b.add_trace(c, Layer::Top, 100_000, vec![Point::new(5 * MM, y + 200_000), Point::new(20 * MM, y + 200_000)])
            .unwrap();
    }
    let r1 = drc::check(&b);
    let r2 = drc::check(&b);
    assert_eq!(r1.violations, r2.violations);
    assert!(r1.by_kind(DrcKind::TraceWidth) > 0);
    let kinds = r1.counts();
    assert!(!kinds.is_empty());
}

#[test]
fn unrouted_and_connectivity_helper() {
    let lib = Library::standard();
    let mut b = base();
    let n = b.add_net("N");
    let fp = lib.footprint("R_0805").unwrap();
    b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n), None]).unwrap();
    b.add_footprint(fp, "R2", Point::new(25 * MM, 15 * MM), 0, &[Some(n), None]).unwrap();
    let r = drc::check(&b);
    assert!(r.unrouted >= 1);
    assert!(!drc::all_nets_connected(&b));
    let a = b.footprints[0].pads[0].at;
    let c = b.footprints[1].pads[0].at;
    b.add_trace(n, Layer::Top, 200_000, vec![a, c]).unwrap();
    assert!(drc::all_nets_connected(&b));
    assert_eq!(drc::check(&b).unrouted, 0);
}
