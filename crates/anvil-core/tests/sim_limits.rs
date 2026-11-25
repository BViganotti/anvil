//! Solver size and validation limits.

use anvil_core::sim::{
    dc_op, transient, Circuit, Digital, Element, GateKind, SimError, SimOptions, Waveform,
};

#[test]
fn node_limit_is_enforced() {
    let mut c = Circuit::new();
    for i in 0..600 {
        c.add_node(&format!("n{i}"));
    }
    assert!(matches!(dc_op(&c), Err(SimError::TooLarge(_))));
}

#[test]
fn element_limit_is_enforced() {
    let mut c = Circuit::new();
    let a = c.add_node("a");
    c.vsource(a, 0, 1.0);
    for _ in 0..5000 {
        c.resistor(a, 0, 1000.0);
    }
    assert!(matches!(dc_op(&c), Err(SimError::TooLarge(_))));
}

#[test]
fn non_finite_and_zero_values_are_rejected() {
    let mut c = Circuit::new();
    c.resistor(1, 0, f64::NAN);
    assert!(c.validate().is_err());
    let mut c = Circuit::new();
    c.resistor(1, 0, 0.0);
    assert!(c.validate().is_err());
    let mut c = Circuit::new();
    c.capacitor(1, 0, -1.0);
    assert!(c.validate().is_err());
    let mut c = Circuit::new();
    c.inductor(1, 0, f64::INFINITY);
    assert!(c.validate().is_err());
}

#[test]
fn oversized_pwl_is_rejected() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    let points: Vec<(f64, f64)> = (0..10_001).map(|i| (i as f64, 1.0)).collect();
    c.elements.push(Element::VSource {
        a: n,
        b: 0,
        wave: Waveform::Pwl(points),
    });
    c.resistor(n, 0, 1000.0);
    assert!(matches!(c.validate(), Err(SimError::TooLarge(_))));
}

#[test]
fn digital_shape_limits() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    c.add(Element::Digital(Digital {
        gate: GateKind::Dff,
        inputs: vec![n],
        outputs: vec![n],
        delay: 1e-9,
        vdd: 5.0,
        rout: 100.0,
        initial: false,
    }));
    assert!(c.validate().is_err());
    let mut c = Circuit::new();
    let n = c.add_node("n");
    c.add(Element::Digital(Digital {
        gate: GateKind::And,
        inputs: vec![n],
        outputs: vec![n],
        delay: -1.0,
        vdd: 5.0,
        rout: 100.0,
        initial: false,
    }));
    assert!(c.validate().is_err());
}

#[test]
fn too_many_record_nodes_is_rejected() {
    let mut c = Circuit::new();
    let a = c.add_node("a");
    c.vsource(a, 0, 1.0);
    for i in 0..90 {
        let n = c.add_node(&format!("n{i}"));
        c.resistor(a, n, 1000.0);
        c.resistor(n, 0, 1000.0);
    }
    let mut opts = SimOptions::default();
    opts.tstep = 1e-6;
    opts.tstop = 1e-5;
    opts.record_nodes = (1..=65).collect();
    assert!(matches!(transient(&c, &opts), Err(SimError::TooLarge(_))));
}

#[test]
fn step_limit_option_is_validated() {
    let mut c = Circuit::new();
    let a = c.add_node("a");
    c.vsource(a, 0, 1.0);
    c.resistor(a, 0, 1000.0);
    let mut opts = SimOptions::default();
    opts.max_steps = 0;
    assert!(matches!(transient(&c, &opts), Err(SimError::BadOption(_))));
    opts.max_steps = 10_000_000;
    assert!(matches!(transient(&c, &opts), Err(SimError::BadOption(_))));
}
