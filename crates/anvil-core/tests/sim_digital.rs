//! Digital device contract: truth tables, DFF complement, validation.

use anvil_core::sim::{dc_op, transient, Circuit, Digital, Element, GateKind, Integration, SimOptions};

fn gate_level(gate: GateKind, bits: &[bool]) -> f64 {
    let mut c = Circuit::new();
    let mut inputs = Vec::new();
    for &b in bits {
        let n = c.add_node(&format!("in{}", inputs.len()));
        c.vsource(n, 0, if b { 5.0 } else { 0.0 });
        inputs.push(n);
    }
    let out = c.add_node("out");
    c.add(Element::Digital(Digital {
        gate,
        inputs,
        outputs: vec![out],
        delay: 1e-9,
        vdd: 5.0,
        rout: 100.0,
        initial: false,
    }));
    dc_op(&c)
        .unwrap_or_else(|e| panic!("{gate:?} {bits:?}: {e}"))
        .voltages[out]
}

fn logic(gate: GateKind, bits: &[bool], want: bool) {
    let v = gate_level(gate, bits);
    assert_eq!(v > 2.5, want, "{gate:?} {bits:?} -> {v}");
}

#[test]
fn gate_truth_tables() {
    logic(GateKind::Or, &[false, false], false);
    logic(GateKind::Or, &[false, true], true);
    logic(GateKind::Or, &[true, true], true);
    logic(GateKind::Nand, &[false, false], true);
    logic(GateKind::Nand, &[true, true], false);
    logic(GateKind::Nor, &[false, false], true);
    logic(GateKind::Nor, &[true, false], false);
    logic(GateKind::Xor, &[false, false], false);
    logic(GateKind::Xor, &[true, false], true);
    logic(GateKind::Xor, &[true, true], false);
    logic(GateKind::Not, &[false], true);
    logic(GateKind::Not, &[true], false);
}

#[test]
fn dff_outputs_are_complementary() {
    let mut c = Circuit::new();
    let d = c.add_node("d");
    let clk = c.add_node("clk");
    let q = c.add_node("q");
    let qn = c.add_node("qn");
    c.vsource(d, 0, 5.0);
    c.elements.push(Element::VSource {
        a: clk,
        b: 0,
        wave: anvil_core::sim::Waveform::Pulse {
            v1: 0.0,
            v2: 5.0,
            delay: 1e-6,
            rise: 1e-9,
            width: 2e-6,
            period: 4e-6,
        },
    });
    c.add(Element::Digital(Digital {
        gate: GateKind::Dff,
        inputs: vec![d, clk],
        outputs: vec![q, qn],
        delay: 2e-8,
        vdd: 5.0,
        rout: 100.0,
        initial: false,
    }));
    let mut opts = SimOptions::default();
    opts.tstep = 2e-8;
    opts.tstop = 2.5e-6;
    opts.method = Integration::Trapezoidal;
    let tr = transient(&c, &opts).unwrap();
    let qv = tr.last("q").unwrap();
    let qnv = tr.last("qn").unwrap();
    assert!(qv > 4.5, "q={qv}");
    assert!(qnv < 0.5, "qn={qnv}");
    assert!((qv + qnv - 5.0).abs() < 0.2);
}

#[test]
fn digital_validation_rejects_bad_shapes() {
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
        outputs: Vec::new(),
        delay: 1e-9,
        vdd: 5.0,
        rout: 100.0,
        initial: false,
    }));
    assert!(c.validate().is_err());
}

#[test]
fn gate_delay_changes_edge_time() {
    fn edge_time(delay: f64) -> f64 {
        let mut c = Circuit::new();
        let inp = c.add_node("in");
        let out = c.add_node("out");
        c.elements.push(Element::VSource {
            a: inp,
            b: 0,
            wave: anvil_core::sim::Waveform::Pulse {
                v1: 0.0,
                v2: 5.0,
                delay: 0.0,
                rise: 1e-9,
                width: 1.0,
                period: 2.0,
            },
        });
        c.add(Element::Digital(Digital {
            gate: GateKind::Not,
            inputs: vec![inp],
            outputs: vec![out],
            delay,
            vdd: 5.0,
            rout: 100.0,
            initial: true,
        }));
        let mut opts = SimOptions::default();
        opts.tstep = 1e-8;
        opts.tstop = 5e-7;
        let tr = transient(&c, &opts).unwrap();
        let s = tr.series_for("out").unwrap();
        for (i, &v) in s.iter().enumerate() {
            if v < 2.5 {
                return tr.times[i];
            }
        }
        f64::INFINITY
    }
    let fast = edge_time(1e-8);
    let slow = edge_time(1e-7);
    assert!(fast.is_finite() && slow.is_finite());
    assert!(slow > fast, "fast={fast} slow={slow}");
}
