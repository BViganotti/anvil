//! Router options, single-net routing and failure reporting.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_pcb::board::{Board, DesignRules, Layer, MM};
use anvil_pcb::connect::ratsnest;
use anvil_pcb::drc;
use anvil_pcb::router::{route_all, route_one, RouteError, RouteProgress, RouterOptions};

fn two_pad_board() -> (Board, usize) {
    let lib = Library::standard();
    let mut b = Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default());
    let n = b.add_net("N");
    let fp = lib.footprint("R_0805").unwrap();
    b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n), None])
        .unwrap();
    b.add_footprint(fp, "R2", Point::new(30 * MM, 20 * MM), 0, &[Some(n), None])
        .unwrap();
    (b, n)
}

#[test]
fn option_validation() {
    let mut o = RouterOptions::default();
    o.grid = 0;
    assert!(matches!(o.validate(), Err(RouteError::InvalidOptions(_))));
    let mut o = RouterOptions::default();
    o.retries = 0;
    assert!(o.validate().is_err());
    let mut o = RouterOptions::default();
    o.max_expansions = 1;
    assert!(o.validate().is_err());
    assert!(RouterOptions::default().validate().is_ok());
}

#[test]
fn for_board_raises_grid_and_width() {
    let mut b = Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default());
    b.rules.min_trace = 400_000;
    b.rules.min_clearance = 300_000;
    let o = RouterOptions::for_board(&b);
    assert!(o.grid >= b.rules.min_trace + b.rules.min_clearance);
    assert_eq!(o.width, 400_000);
}

#[test]
fn route_one_connects_two_pads() {
    let (mut b, n) = two_pad_board();
    let opts = RouterOptions::for_board(&b);
    route_one(&mut b, n, &opts).unwrap();
    assert!(!b.traces.is_empty());
    assert!(ratsnest(&b).is_empty());
    let report = drc::check(&b);
    assert!(report.is_clean(), "{:?}", report.violations);
}

#[test]
fn route_all_with_no_connections_is_clean() {
    let mut b = Board::new_rect(30 * MM, 30 * MM, 2, DesignRules::default());
    let n = b.add_net("NC");
    b.add_footprint(
        Library::standard().footprint("TP_1MM").unwrap(),
        "TP1",
        Point::new(15 * MM, 15 * MM),
        0,
        &[Some(n)],
    )
    .unwrap();
    let opts = RouterOptions::for_board(&b);
    let mut p = |_p: &RouteProgress| {};
    let report = route_all(&mut b, &opts, &mut p).unwrap();
    assert!(report.clean);
    assert_eq!(report.nets_total, 0);
    assert!(b.traces.is_empty());
}

#[test]
fn tiny_board_returns_a_board_error_not_a_panic() {
    let mut b = Board::new_rect(1 * MM, 1 * MM, 2, DesignRules::default());
    b.add_net("N");
    let opts = RouterOptions::for_board(&b);
    let mut p = |_p: &RouteProgress| {};
    let result = route_all(&mut b, &opts, &mut p);
    assert!(matches!(result, Err(RouteError::Board(_))), "{result:?}");
}

#[test]
fn four_layer_board_uses_top_and_bottom() {
    let (mut b, _) = two_pad_board();
    b.copper_layers = 4;
    b.validate_structure().unwrap();
    let opts = RouterOptions::for_board(&b);
    let mut p = |_p: &RouteProgress| {};
    let report = route_all(&mut b, &opts, &mut p).unwrap();
    assert!(report.clean);
    assert!(b.traces.iter().all(|t| t.layer == Layer::Top || t.layer == Layer::Bottom));
}
