//! Schematic-to-circuit compiler contract.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_core::netlist::extract;
use anvil_core::schematic::{LabelKind, Schematic};
use anvil_core::sim::{compile_schematic, CompileOptions, Element, SimError};

fn label(s: &mut Schematic, refdes: &str, pin: &str, net: &str) -> anvil_core::schematic::CompId {
    let id = s
        .components()
        .iter()
        .find(|c| c.refdes == refdes)
        .map(|c| c.id)
        .unwrap();
    let pos = s.component(id).unwrap().pin_by_name(pin).unwrap().pos;
    s.add_label(net, pos, if net == "GND" { LabelKind::Power } else { LabelKind::Local })
        .unwrap();
    id
}

#[test]
fn compiles_divider() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    s.add_component_with_refdes(&lib, "V", Point::new(0, 0), "V1").unwrap();
    s.add_component(&lib, "R", Point::new(10_000_000, 0)).unwrap();
    s.add_component(&lib, "R", Point::new(10_000_000, 8_000_000)).unwrap();
    label(&mut s, "V1", "+", "VIN");
    label(&mut s, "V1", "-", "GND");
    label(&mut s, "R1", "1", "VIN");
    label(&mut s, "R1", "2", "OUT");
    label(&mut s, "R2", "1", "OUT");
    label(&mut s, "R2", "2", "GND");
    let nl = extract(&s, &lib);
    assert_eq!(nl.errors(), 0, "{:?}", nl.diagnostics);
    let c = compile_schematic(&s, &nl, &lib, &CompileOptions::default()).unwrap();
    assert_eq!(c.elements.len(), 3);
    assert!(c.elements.iter().any(|e| matches!(e, Element::VSource { .. })));
    assert_eq!(c.elements.iter().filter(|e| matches!(e, Element::Resistor { .. })).count(), 2);
    let dc = anvil_core::sim::dc_op(&c).unwrap();
    assert!((dc.voltage("OUT").unwrap() - 2.5).abs() < 1e-9);
}

#[test]
fn missing_ground_is_an_error() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    s.add_component_with_refdes(&lib, "V", Point::new(0, 0), "V1").unwrap();
    label(&mut s, "V1", "+", "A");
    label(&mut s, "V1", "-", "B");
    let nl = extract(&s, &lib);
    let r = compile_schematic(&s, &nl, &lib, &CompileOptions::default());
    assert!(matches!(r, Err(SimError::Compile(_))), "{r:?}");
}

#[test]
fn bad_value_is_reported_with_refdes() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    let r = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    s.set_value(r, "not-a-number").unwrap();
    label(&mut s, "R1", "1", "A");
    label(&mut s, "R1", "2", "GND");
    let nl = extract(&s, &lib);
    let err = compile_schematic(&s, &nl, &lib, &CompileOptions::default()).unwrap_err();
    assert!(err.to_string().contains("R1"), "{err}");
}

#[test]
fn unsupported_symbol_is_reported() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    let id = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    s.component_mut(id).unwrap().symbol = "WEIRD".into();
    label(&mut s, "R1", "1", "A");
    label(&mut s, "R1", "2", "GND");
    let nl = extract(&s, &lib);
    let err = compile_schematic(&s, &nl, &lib, &CompileOptions::default()).unwrap_err();
    assert!(err.to_string().contains("WEIRD") || err.to_string().contains("unsupported"), "{err}");
}

#[test]
fn connectors_are_connective_only() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    s.add_component(&lib, "CONN2", Point::new(0, 0)).unwrap();
    label(&mut s, "J1", "1", "GND");
    label(&mut s, "J1", "2", "SIG");
    let nl = extract(&s, &lib);
    let c = compile_schematic(&s, &nl, &lib, &CompileOptions::default()).unwrap();
    assert!(c.elements.is_empty());
}

#[test]
fn pulse_properties_compile() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    let id = s.add_component_with_refdes(&lib, "V", Point::new(0, 0), "V1").unwrap();
    s.set_value(id, "5").unwrap();
    s.set_property(id, "wave", "pulse").unwrap();
    s.set_property(id, "v1", "0").unwrap();
    s.set_property(id, "v2", "5").unwrap();
    s.set_property(id, "width", "1m").unwrap();
    s.set_property(id, "period", "2m").unwrap();
    s.add_component(&lib, "R", Point::new(10_000_000, 0)).unwrap();
    label(&mut s, "V1", "+", "A");
    label(&mut s, "V1", "-", "GND");
    label(&mut s, "R1", "1", "A");
    label(&mut s, "R1", "2", "GND");
    let nl = extract(&s, &lib);
    let c = compile_schematic(&s, &nl, &lib, &CompileOptions::default()).unwrap();
    match &c.elements[0] {
        Element::VSource { wave, .. } => {
            assert!(matches!(wave, anvil_core::sim::Waveform::Pulse { .. }));
            assert_eq!(wave.value(0.0), 0.0);
            assert_eq!(wave.value(1e-3), 5.0);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn two_ground_nets_are_rejected() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    s.add_component(&lib, "R", Point::new(10_000_000, 0)).unwrap();
    label(&mut s, "R1", "1", "GND");
    label(&mut s, "R1", "2", "0");
    label(&mut s, "R2", "1", "GND");
    label(&mut s, "R2", "2", "0");
    let nl = extract(&s, &lib);
    let err = compile_schematic(&s, &nl, &lib, &CompileOptions::default()).unwrap_err();
    assert!(err.to_string().contains("ground"), "{err}");
}
