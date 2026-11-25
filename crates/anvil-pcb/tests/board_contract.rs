//! Board model contract.

use anvil_core::geom::Point;
use anvil_core::library::{Library, PadLayer};
use anvil_pcb::board::{Board, BoardError, DesignRules, Layer, MM};

fn empty() -> Board {
    Board::new_rect(50 * MM, 40 * MM, 2, DesignRules::default())
}

#[test]
fn nets_dedupe_and_size() {
    let mut b = empty();
    assert_eq!(b.add_net("GND"), 0);
    assert_eq!(b.add_net("VCC"), 1);
    assert_eq!(b.add_net("GND"), 0);
    assert_eq!(b.net_id("VCC"), Some(1));
    assert_eq!(b.net_name(0), "GND");
    assert_eq!(b.net_name(99), "?");
    let bounds = b.bounds();
    assert_eq!(bounds.width(), 50 * MM);
    assert_eq!(bounds.height(), 40 * MM);
    assert!(b.contains(Point::new(25 * MM, 20 * MM)));
}

#[test]
fn footprint_errors_are_reported() {
    let lib = Library::standard();
    let mut b = empty();
    let n = b.add_net("N");
    let fp = lib.footprint("R_0805").unwrap();
    assert!(matches!(
        b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n)]),
        Err(BoardError::PadMismatch)
    ));
    b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n), None])
        .unwrap();
    assert!(matches!(
        b.add_footprint(fp, "R1", Point::new(20 * MM, 10 * MM), 0, &[None, None]),
        Err(BoardError::DuplicateRefdes(_))
    ));
    assert!(matches!(
        b.add_footprint(fp, "R2", Point::new(20 * MM, 10 * MM), 0, &[Some(9), None]),
        Err(BoardError::UnknownNet(9))
    ));
}

#[test]
fn moving_a_footprint_moves_pads() {
    let lib = Library::standard();
    let mut b = empty();
    let n = b.add_net("N");
    b.add_footprint(
        lib.footprint("R_0805").unwrap(),
        "R1",
        Point::new(10 * MM, 10 * MM),
        0,
        &[Some(n), None],
    )
    .unwrap();
    let old = b.footprints[0].pads[0].at;
    b.move_footprint(0, Point::new(20 * MM, 15 * MM)).unwrap();
    assert_eq!(
        b.footprints[0].pads[0].at,
        old.add(Point::new(10 * MM, 5 * MM))
    );
    assert!(b.move_footprint(3, Point::ORIGIN).is_err());
}

#[test]
fn trace_and_via_validation() {
    let mut b = empty();
    let n = b.add_net("N");
    assert!(matches!(
        b.add_trace(5, Layer::Top, 200_000, vec![Point::new(0, 0), Point::new(MM, 0)]),
        Err(BoardError::UnknownNet(5))
    ));
    assert!(b
        .add_trace(0, Layer::Inner1, 200_000, vec![Point::new(0, 0), Point::new(MM, 0)])
        .is_err());
    assert!(b
        .add_trace(0, Layer::Top, 100_000, vec![Point::new(0, 0), Point::new(0, 0)])
        .is_err());
    assert!(b
        .add_trace(0, Layer::Top, 100_000, vec![Point::new(0, 0), Point::new(MM, 0)])
        .is_ok());
    let v = b.add_via(0, Point::new(5 * MM, 5 * MM), Layer::Bottom, Layer::Top).unwrap();
    assert_eq!(b.vias[v].from, Layer::Top);
    assert_eq!(b.vias[v].to, Layer::Bottom);
    assert!(b.add_via(0, Point::new(6 * MM, 6 * MM), Layer::Top, Layer::Inner2).is_err());
    b.remove_net_routes(0);
    assert!(b.traces.is_empty() && b.vias.is_empty());
    let _ = n;
}

#[test]
fn structure_validation_catches_bad_rules_and_outline() {
    let mut b = empty();
    b.rules.min_trace = 0;
    assert!(b.validate_structure().is_err());
    let mut b = empty();
    b.outline = vec![Point::new(0, 0), Point::new(1, 1)];
    assert!(b.validate_structure().is_err());
    let mut b = empty();
    b.add_net("N");
    b.add_footprint(
        Library::standard().footprint("R_0805").unwrap(),
        "R1",
        Point::ORIGIN,
        0,
        &[Some(0), None],
    )
    .unwrap();
    b.footprints[0].pads[1].layers = PadLayer::Through;
    b.validate_structure().unwrap();
}

#[test]
fn refdes_generation_skips_used_names() {
    let lib = Library::standard();
    let mut b = empty();
    b.add_footprint(
        lib.footprint("R_0805").unwrap(),
        "R1",
        Point::new(10 * MM, 10 * MM),
        0,
        &[None, None],
    )
    .unwrap();
    assert_eq!(b.generate_refdes("R"), "R2");
    assert_eq!(b.generate_refdes("C"), "C1");
}
