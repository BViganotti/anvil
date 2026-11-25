//! Copper connectivity and ratsnest contract.

use anvil_core::geom::Point;
use anvil_core::library::{Library, PadLayer};
use anvil_pcb::board::{Board, DesignRules, Layer, MM};
use anvil_pcb::connect::{pad_groups, ratsnest, Connectivity};

fn board() -> (Board, usize) {
    let mut b = Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default());
    let n = b.add_net("N");
    let lib = Library::standard();
    let fp = lib.footprint("R_0805").unwrap();
    b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n), None])
        .unwrap();
    b.add_footprint(fp, "R2", Point::new(25 * MM, 15 * MM), 0, &[Some(n), None])
        .unwrap();
    (b, n)
}

#[test]
fn ratsnest_minimum_spanning_tree() {
    let (mut b, n) = board();
    let lib = Library::standard();
    let fp = lib.footprint("R_0805").unwrap();
    b.add_footprint(fp, "R3", Point::new(32 * MM, 20 * MM), 0, &[Some(n), None])
        .unwrap();
    let edges = ratsnest(&b);
    assert_eq!(edges.len(), 2, "three pads need two edges");
    assert!(edges.iter().all(|e| e.net == n));
    assert!(edges[0].length() > 0.0);
}

#[test]
fn direct_trace_connects_and_clears_ratsnest() {
    let (mut b, n) = board();
    let a = b.footprints[0].pads[0].at;
    let c = b.footprints[1].pads[0].at;
    b.add_trace(n, Layer::Top, 200_000, vec![a, c]).unwrap();
    assert!(ratsnest(&b).is_empty());
}

#[test]
fn via_connects_top_and_bottom() {
    let (mut b, n) = board();
    b.footprints[0].pads[0].layers = PadLayer::Through;
    b.footprints[1].pads[0].layers = PadLayer::Through;
    let a = b.footprints[0].pads[0].at;
    let c = b.footprints[1].pads[0].at;
    let via_at = Point::new(18 * MM, 12 * MM);
    b.add_trace(n, Layer::Top, 200_000, vec![a, via_at]).unwrap();
    b.add_trace(n, Layer::Bottom, 200_000, vec![via_at, c]).unwrap();
    // Without the via the layers are disconnected.
    assert!(!ratsnest(&b).is_empty());
    b.add_via(n, via_at, Layer::Top, Layer::Bottom).unwrap();
    assert!(ratsnest(&b).is_empty());
}

#[test]
fn different_nets_do_not_connect() {
    let mut b = Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default());
    let lib = Library::standard();
    let n1 = b.add_net("A");
    let n2 = b.add_net("B");
    let fp = lib.footprint("R_0805").unwrap();
    b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n1), None])
        .unwrap();
    b.add_footprint(fp, "R2", Point::new(20 * MM, 10 * MM), 0, &[Some(n2), Some(n2)])
        .unwrap();
    let a = b.footprints[0].pads[0].at;
    let c = b.footprints[1].pads[0].at;
    b.add_trace(n1, Layer::Top, 200_000, vec![a, c]).unwrap();
    // The trace only belongs to net A; net B's pad remains unconnected.
    assert!(ratsnest(&b).iter().any(|e| e.net == n2));
    assert!(!ratsnest(&b).iter().any(|e| e.net == n1));
}

#[test]
fn pad_groups_track_same_footprint_shorts() {
    let mut b = Board::new_rect(30 * MM, 30 * MM, 2, DesignRules::default());
    let lib = Library::standard();
    let n = b.add_net("N");
    b.add_footprint(
        lib.footprint("R_0805").unwrap(),
        "R1",
        Point::new(10 * MM, 10 * MM),
        0,
        &[Some(n), Some(n)],
    )
    .unwrap();
    let mut conn = Connectivity::build(&b);
    let groups = pad_groups(&b, n, &mut conn);
    // R_0805 pads are 1.9 mm apart and do not touch, so two groups.
    assert_eq!(groups.len(), 2);
    assert_eq!(conn.pad_count(), 2);
}
