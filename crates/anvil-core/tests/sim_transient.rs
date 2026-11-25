//! Transient integration contract beyond analytic acceptance tests.

use anvil_core::sim::{transient, Circuit, Element, Integration, SimError, SimOptions, Waveform};

fn pulse(v1: f64, v2: f64, width: f64) -> Waveform {
    Waveform::Pulse {
        v1,
        v2,
        delay: 0.0,
        rise: 1e-9,
        width,
        period: 10.0,
    }
}

#[test]
fn capacitor_initial_condition_discharges() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    c.resistor(n, 0, 1000.0);
    c.elements.push(Element::Capacitor {
        a: n,
        b: 0,
        c: 1e-6,
        ic: Some(5.0),
    });
    let tau = 1e-3;
    let mut opts = SimOptions::default();
    opts.tstep = tau / 2000.0;
    opts.tstop = 3.0 * tau;
    opts.method = Integration::Trapezoidal;
    let tr = transient(&c, &opts).unwrap();
    let s = tr.series_for("n").unwrap();
    for (i, &t) in tr.times.iter().enumerate() {
        if t < 0.2 * tau {
            continue;
        }
        let want = 5.0 * (-t / tau).exp();
        assert!((s[i] - want).abs() < 0.05, "t={t} got={} want={want}", s[i]);
    }
}

#[test]
fn pwl_source_drives_rc() {
    let mut c = Circuit::new();
    let inp = c.add_node("in");
    let out = c.add_node("out");
    c.elements.push(Element::VSource {
        a: inp,
        b: 0,
        wave: Waveform::Pwl(vec![(0.0, 0.0), (1e-3, 5.0)]),
    });
    c.resistor(inp, out, 1000.0);
    c.capacitor(out, 0, 1e-6);
    let mut opts = SimOptions::default();
    opts.tstep = 1e-6;
    opts.tstop = 1e-3;
    let tr = transient(&c, &opts).unwrap();
    let last = tr.last("out").unwrap();
    assert!(last > 1.0 && last < 5.0, "last={last}");
}

#[test]
fn record_nodes_selects_series() {
    let mut c = Circuit::new();
    let a = c.add_node("a");
    let b = c.add_node("b");
    c.vsource(a, 0, 1.0);
    c.resistor(a, b, 1000.0);
    c.resistor(b, 0, 1000.0);
    let mut opts = SimOptions::default();
    opts.tstep = 1e-6;
    opts.tstop = 1e-5;
    opts.record_nodes = vec![b];
    let tr = transient(&c, &opts).unwrap();
    assert_eq!(tr.node_names, vec!["b".to_string()]);
    assert!((tr.last("b").unwrap() - 0.5).abs() < 1e-9);
}

#[test]
fn invalid_time_options_are_rejected() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    c.vsource(n, 0, 1.0);
    c.resistor(n, 0, 1000.0);
    let mut opts = SimOptions::default();
    opts.tstep = 0.0;
    assert!(matches!(transient(&c, &opts), Err(SimError::BadOption(_))));
    opts.tstep = 1e-6;
    opts.tstop = 0.0;
    assert!(matches!(transient(&c, &opts), Err(SimError::BadOption(_))));
    opts.tstop = -1.0;
    assert!(matches!(transient(&c, &opts), Err(SimError::BadOption(_))));
    opts.tstop = 1e-3;
    opts.tstep = 1e-2;
    assert!(matches!(transient(&c, &opts), Err(SimError::BadOption(_))));
}

#[test]
fn step_budget_truncates_instead_of_hanging() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    c.vsource(n, 0, 1.0);
    c.resistor(n, 0, 1000.0);
    let mut opts = SimOptions::default();
    opts.tstep = 1e-9;
    opts.tstop = 1e-3;
    opts.max_steps = 10;
    let tr = transient(&c, &opts).unwrap();
    assert!(tr.truncated);
    assert_eq!(tr.steps, 10);
}

#[test]
fn no_reactive_elements_equals_dc() {
    let mut c = Circuit::new();
    let a = c.add_node("a");
    let b = c.add_node("b");
    c.vsource(a, 0, 9.0);
    c.resistor(a, b, 1000.0);
    c.resistor(b, 0, 2000.0);
    let mut opts = SimOptions::default();
    opts.tstep = 1e-6;
    opts.tstop = 5e-6;
    let tr = transient(&c, &opts).unwrap();
    assert!((tr.last("b").unwrap() - 6.0).abs() < 1e-9);
}

#[test]
fn pulse_source_zero_width_stays_low() {
    let mut c = Circuit::new();
    let n = c.add_node("n");
    c.elements.push(Element::VSource {
        a: n,
        b: 0,
        wave: pulse(0.0, 5.0, 0.0),
    });
    c.resistor(n, 0, 1000.0);
    let mut opts = SimOptions::default();
    opts.tstep = 1e-6;
    opts.tstop = 1e-5;
    let tr = transient(&c, &opts).unwrap();
    assert!(tr.last("n").unwrap() < 1e-6);
}
