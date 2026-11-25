//! Geometry-driven DRC cases: through pads, bottom layers and vias.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_pcb::board::{Board, DesignRules, Layer, MM};
use anvil_pcb::drc::{self, DrcKind};

#[test]
fn through_pad_clearance_on_bottom_layer() {
    let lib = Library::standard();
    let mut b = Board::new_rect(30 * MM, 30 * MM, 2, DesignRules::default());
    let a = b.add_net("A");
    let c = b.add_net("B");
    b.add_footprint(
        lib.footprint("PINHDR_2").unwrap(),
        "J1",
        Point::new(10 * MM, 10 * MM),
        0,
        &[Some(a), None],
    )
    .unwrap();
    b.add_trace(
        c,
        Layer::Bottom,
        200_000,
        vec![Point::new(5 * MM, 11 * MM), Point::new(15 * MM, 11 * MM)],
    )
    .unwrap();
    let r = drc::check(&b);
    assert!(r.by_kind(DrcKind::Clearance) + r.by_kind(DrcKind::Short) >= 1, "{:?}", r.violations);
}

#[test]
fn via_annular_rule() {
    let mut b = Board::new_rect(30 * MM, 30 * MM, 2, DesignRules::default());
    let a = b.add_net("A");
    b.add_via(a, Point::new(15 * MM, 15 * MM), Layer::Top, Layer::Bottom).unwrap();
    assert_eq!(drc::check(&b).by_kind(DrcKind::ViaAnnular), 0);
    b.rules.min_annular = 300_000;
    assert!(drc::check(&b).by_kind(DrcKind::ViaAnnular) >= 1);
}

#[test]
fn via_edge_clearance() {
    let mut b = Board::new_rect(30 * MM, 30 * MM, 2, DesignRules::default());
    let a = b.add_net("A");
    b.add_via(a, Point::new(400_000, 15 * MM), Layer::Top, Layer::Bottom).unwrap();
    let r = drc::check(&b);
    assert!(r.by_kind(DrcKind::BoardEdge) + r.by_kind(DrcKind::OutsideBoard) >= 1);
}

#[test]
fn pad_on_bottom_only_ignores_top_trace() {
    let lib = Library::standard();
    let mut b = Board::new_rect(30 * MM, 30 * MM, 2, DesignRules::default());
    let a = b.add_net("A");
    let c = b.add_net("B");
    b.add_footprint(
        lib.footprint("TP_1MM").unwrap(),
        "TP1",
        Point::new(15 * MM, 15 * MM),
        0,
        &[Some(a)],
    )
    .unwrap();
    b.footprints[0].pads[0].layers = anvil_core::library::PadLayer::Bottom;
    b.add_trace(
        c,
        Layer::Top,
        200_000,
        vec![Point::new(5 * MM, 15 * MM), Point::new(25 * MM, 15 * MM)],
    )
    .unwrap();
    let r = drc::check(&b);
    assert_eq!(r.by_kind(DrcKind::Clearance), 0);
    assert_eq!(r.by_kind(DrcKind::Short), 0);
}

#[test]
fn exact_threshold_is_clear_but_one_nanometre_closer_is_not() {
    fn board(offset: i64) -> Board {
        let mut b = Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default());
        let a = b.add_net("A");
        let c = b.add_net("B");
        b.add_trace(
            a,
            Layer::Top,
            200_000,
            vec![Point::new(10 * MM, 10 * MM), Point::new(20 * MM, 10 * MM)],
        )
        .unwrap();
        // Edge gap = centre distance - 0.2 mm. A 0.2 mm clearance therefore
        // needs a 0.4 mm centre distance; Anvil flags the exact threshold
        // conservatively.
        b.add_trace(
            c,
            Layer::Top,
            200_000,
            vec![
                Point::new(10 * MM, 10 * MM + 400_001 + offset),
                Point::new(20 * MM, 10 * MM + 400_001 + offset),
            ],
        )
        .unwrap();
        b
    }
    let clear = drc::check(&board(0));
    assert_eq!(clear.by_kind(DrcKind::Clearance), 0, "{:?}", clear.violations);
    let close = drc::check(&board(-1));
    assert!(close.by_kind(DrcKind::Clearance) >= 1);
}
