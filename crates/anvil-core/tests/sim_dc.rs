//! DC solver contract and error handling.

use anvil_core::sim::{dc_op, Circuit, Element, SimError};

#[test]
fn voltage_source_branch_current() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    c.vsource(n, 0, 5.0);
    c.resistor(n, 0, 1000.0);
    let r = dc_op(&c).unwrap();
    assert!((r.voltages[n] - 5.0).abs() < 1e-12);
    // Branch current flows from the positive node through the source; its
    // magnitude is V/R.
    assert!((r.branch_currents[0].abs() - 0.005).abs() < 1e-9);
}

#[test]
fn multiple_sources_and_superposition() {
    let mut c = Circuit::new();
    let a = c.add_node("a");
    let b = c.add_node("b");
    let out = c.add_node("out");
    c.vsource(a, 0, 3.0);
    c.vsource(b, 0, 4.0);
    c.resistor(a, out, 1000.0);
    c.resistor(b, out, 1000.0);
    c.resistor(out, 0, 1000.0);
    let r = dc_op(&c).unwrap();
    // (3/1k + 4/1k) / (3/1k) = 7/3 V
    assert!((r.voltages[out] - 7.0 / 3.0).abs() < 1e-9);
}

#[test]
fn inductor_is_a_short_at_dc() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    let m = c.add_node("m");
    c.vsource(n, 0, 5.0);
    c.resistor(n, m, 1000.0);
    c.inductor(m, 0, 1e-3);
    let r = dc_op(&c).unwrap();
    assert!(r.voltages[m].abs() < 1e-12);
    assert!((r.branch_currents[0].abs() - 0.005).abs() < 1e-9);
}

#[test]
fn capacitor_is_open_at_dc() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    let m = c.add_node("m");
    c.vsource(n, 0, 5.0);
    c.resistor(n, m, 1000.0);
    c.capacitor(m, 0, 1e-6);
    let r = dc_op(&c).unwrap();
    assert!((r.voltages[m] - 5.0).abs() < 1e-9);
}

#[test]
fn zero_resistance_is_rejected() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    c.vsource(n, 0, 1.0);
    c.resistor(n, 0, 0.0);
    assert!(matches!(dc_op(&c), Err(SimError::Invalid(_))));
}

#[test]
fn bad_node_is_rejected() {
    let mut c = Circuit::new();
    c.vsource(9, 0, 1.0);
    assert!(matches!(dc_op(&c), Err(SimError::BadNode(9))));
}

#[test]
fn floating_node_is_solved_to_zero() {
    let mut c = Circuit::new();
    let floating = c.add_node("floating");
    let n = c.add_node("n");
    c.vsource(n, 0, 2.0);
    c.resistor(n, 0, 1000.0);
    let r = dc_op(&c).unwrap();
    assert!(r.voltages[floating].is_finite());
    assert!(r.voltages[floating].abs() < 1e-9);
}

#[test]
fn large_but_valid_circuit() {
    let mut c = Circuit::new();
    let mut prev = c.add_node("n0");
    c.vsource(prev, 0, 10.0);
    for i in 1..100 {
        let n = c.add_node(&format!("n{i}"));
        c.resistor(prev, n, 100.0);
        c.resistor(n, 0, 1000.0);
        prev = n;
    }
    let r = dc_op(&c).unwrap();
    assert!(r.voltages.iter().all(|v| v.is_finite()));
    assert!(r.voltages[prev] > 0.0 && r.voltages[prev] < 10.0);
}

#[test]
fn switch_state_is_applied() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    let m = c.add_node("m");
    c.vsource(n, 0, 5.0);
    c.resistor(n, m, 1000.0);
    c.add(Element::Switch { a: m, b: 0, ron: 0.01, roff: 1e9, closed: true });
    assert!(dc_op(&c).unwrap().voltages[m] < 0.001);
}
