//! Schematic editor contract tests.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_core::schematic::{EditError, LabelKind, Schematic};

fn setup() -> (Library, Schematic) {
    (Library::standard(), Schematic::new())
}

#[test]
fn refdes_is_automatic_and_unique() {
    let (lib, mut s) = setup();
    let a = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    let b = s.add_component(&lib, "R", Point::new(5_000_000, 0)).unwrap();
    assert_eq!(s.component(a).unwrap().refdes, "R1");
    assert_eq!(s.component(b).unwrap().refdes, "R2");
    assert_eq!(s.add_component_with_refdes(&lib, "R", Point::ORIGIN, "R1"), Err(EditError::DuplicateRefdes("R1".into())));
}

#[test]
fn move_rotate_mirror_recompute_pins() {
    let (lib, mut s) = setup();
    let id = s.add_component(&lib, "R", Point::new(10_000_000, 10_000_000)).unwrap();
    let original = s.component(id).unwrap().pins[0].pos;
    s.move_component(id, Point::new(20_000_000, 10_000_000)).unwrap();
    assert_eq!(s.component(id).unwrap().pins[0].pos, original.add(Point::new(10_000_000, 0)));
    s.rotate_component(id, 90).unwrap();
    let pin = s.component(id).unwrap().pins[0].pos;
    assert_eq!(pin, Point::new(20_000_000, 10_000_000 + 2_540_000));
    s.mirror_component(id).unwrap();
    let mirrored = s.component(id).unwrap().pins[0].pos;
    assert_eq!(mirrored, Point::new(20_000_000, 10_000_000 - 2_540_000));
    assert!(s.move_component(id, Point::new(i64::MAX / 2, 0)).is_err());
}

#[test]
fn wire_edits_clean_duplicates() {
    let (_, mut s) = setup();
    let id = s
        .add_wire(vec![
            Point::new(0, 0),
            Point::new(0, 0),
            Point::new(10, 0),
        ])
        .unwrap();
    assert_eq!(s.wire(id).unwrap().points.len(), 2);
    assert!(s.add_wire(vec![Point::new(1, 1)]).is_err());
    assert!(s.add_wire(vec![Point::new(1, 1), Point::new(1, 1)]).is_err());
    s.remove_wire(id).unwrap();
    assert!(s.wire(id).is_none());
}

#[test]
fn labels_and_buses_validate() {
    let (_, mut s) = setup();
    let a = s
        .add_label("CLK", Point::new(0, 0), LabelKind::Local)
        .unwrap();
    let b = s
        .add_label("CLK", Point::new(0, 0), LabelKind::Local)
        .unwrap();
    assert_eq!(a, b);
    assert!(s.add_label("", Point::ORIGIN, LabelKind::Local).is_err());
    let bus = s.add_bus("DATA", &["D0", "D1"]).unwrap();
    assert!(s.add_bus("DATA", &["X"]).is_err());
    s.remove_bus(bus).unwrap();
    assert!(s.add_label("OK", Point::ORIGIN, LabelKind::Global).is_ok());
}

#[test]
fn find_pin_and_component_hits() {
    let (lib, mut s) = setup();
    let id = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    let pin = s.component(id).unwrap().pins[1].pos;
    assert_eq!(s.pin_near(pin.add(Point::new(100, 0)), 1000), Some((id, 1)));
    assert_eq!(s.component_at(Point::ORIGIN), Some(id));
    assert_eq!(s.component_at(Point::new(100_000_000, 0)), None);
    s.remove_component(id).unwrap();
    assert!(s.component(id).is_none());
}
