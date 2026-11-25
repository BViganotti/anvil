//! Netlist connectivity rules beyond the inline tests.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_core::netlist::{extract, Severity};
use anvil_core::schematic::{LabelKind, Schematic};

#[test]
fn t_junction_needs_a_junction_dot() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    let r1 = s.add_component(&lib, "R", Point::new(-7_540_000, 0)).unwrap();
    let r2 = s.add_component(&lib, "R", Point::new(0, -7_540_000)).unwrap();
    s.rotate_component(r2, 90).unwrap();
    let p1 = s.component(r1).unwrap().pins[1].pos;
    let p2 = s.component(r2).unwrap().pins[0].pos;
    assert_eq!(p1, Point::new(-5_000_000, 0));
    assert_eq!(p2, Point::new(0, -5_000_000));
    s.add_wire(vec![p1, Point::new(5_000_000, 0)]).unwrap();
    s.add_wire(vec![Point::new(0, 0), p2]).unwrap();
    let before = extract(&s, &lib);
    let a = before.pin_net("R1", "2").unwrap().id;
    let b = before.pin_net("R2", "1").unwrap().id;
    assert_ne!(a, b, "interior T without junction must not connect");
    s.add_junction(Point::new(0, 0)).unwrap();
    let after = extract(&s, &lib);
    let a2 = after.pin_net("R1", "2").unwrap().id;
    let b2 = after.pin_net("R2", "1").unwrap().id;
    assert_eq!(a2, b2, "junction must connect the T");
}

#[test]
fn global_and_power_labels_merge_by_name() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    let a = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    let b = s.add_component(&lib, "R", Point::new(30_000_000, 0)).unwrap();
    let pa = s.component(a).unwrap().pins[0].pos;
    let pb = s.component(b).unwrap().pins[0].pos;
    s.add_label("NET", pa, LabelKind::Global).unwrap();
    s.add_label("NET", pb, LabelKind::Global).unwrap();
    let p1 = s.component(a).unwrap().pins[1].pos;
    let p2 = s.component(b).unwrap().pins[1].pos;
    s.add_label("5V", p1, LabelKind::Power).unwrap();
    s.add_label("5V", p2, LabelKind::Power).unwrap();
    let nl = extract(&s, &lib);
    assert_eq!(nl.net_by_name("NET").unwrap().nodes.len(), 2);
    assert!(nl.net_by_name("5V").unwrap().power);
}

#[test]
fn undefined_bus_label_reports_error() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    let r = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    let p = s.component(r).unwrap().pins[0].pos;
    s.add_label("DATA[4]", p, LabelKind::Local).unwrap();
    let nl = extract(&s, &lib);
    assert!(nl.diagnostics.iter().any(|d| d.code == "E_BUS_MISSING" && d.severity == Severity::Error));
}

#[test]
fn bus_index_out_of_range_reports_error() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    s.add_bus("DATA", &["D0", "D1"]).unwrap();
    let r = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    let p = s.component(r).unwrap().pins[0].pos;
    s.add_label("DATA[9]", p, LabelKind::Local).unwrap();
    let nl = extract(&s, &lib);
    assert!(nl.diagnostics.iter().any(|d| d.code == "E_BUS_INDEX"));
}

#[test]
fn unknown_symbols_and_single_pins_warn() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    let id = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    s.component_mut(id).unwrap().symbol = "NOT_IN_LIB".into();
    let nl = extract(&s, &lib);
    assert!(nl.diagnostics.iter().any(|d| d.code == "W_UNKNOWN_SYMBOL"));
    assert!(nl.diagnostics.iter().any(|d| d.code == "W_SINGLE_PIN"));
}

#[test]
fn conflicting_labels_report_error_and_stay_deterministic() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    let r = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    let p = s.component(r).unwrap().pins[0].pos;
    s.add_wire(vec![p, p.add(Point::new(0, 5_000_000))]).unwrap();
    s.add_label("A", p, LabelKind::Local).unwrap();
    s.add_label("B", p.add(Point::new(0, 5_000_000)), LabelKind::Local).unwrap();
    let a = extract(&s, &lib);
    let b = extract(&s, &lib);
    assert_eq!(a.nets, b.nets);
    assert!(a.diagnostics.iter().any(|d| d.code == "E_NET_NAMES"));
}
