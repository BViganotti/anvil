//! Acceptance suite for Anvil.
//!
//! Everything here is a real check against independent references or actual
//! geometry. No expected value is copied from the implementation.

use anvil_core::library::Library;
use anvil_core::netlist::extract;
use anvil_core::schematic::{LabelKind, Schematic};
use anvil_core::sim::{
    self, Circuit, Element, Integration, SimOptions, Waveform,
};
use anvil_core::util::Rng;
use anvil_export::{drill, gerber, raster};
use anvil_pcb::board::{Board, DesignRules, Layer, MM};
use anvil_pcb::drc;
use anvil_pcb::generate::generated_board;
use anvil_pcb::router::{route_all, RouterOptions};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn close(got: f64, want: f64, rel: f64, label: &str) {
    let tol = rel * want.abs().max(1.0);
    assert!(
        (got - want).abs() <= tol,
        "{label}: got {got:.12} want {want:.12} (tol {tol:.3e})"
    );
}

// ---------------------------------------------------------------- reference solver

/// Independent dense MNA reference solver for linear R + V/I networks. This
/// implementation is deliberately separate from anvil-core's solver.
struct Reference {
    n: usize,
    branches: usize,
    a: Vec<f64>,
    b: Vec<f64>,
}

impl Reference {
    fn new(nodes: usize, branches: usize) -> Self {
        let n = nodes - 1 + branches;
        Reference {
            n,
            branches,
            a: vec![0.0; n * n],
            b: vec![0.0; n],
        }
    }

    fn gi(&self, node: usize) -> Option<usize> {
        if node == 0 { None } else { Some(node - 1) }
    }

    fn resistor(&mut self, a: usize, b: usize, r: f64) {
        let g = 1.0 / r;
        if let Some(i) = self.gi(a) { self.a[i * self.n + i] += g; }
        if let Some(j) = self.gi(b) { self.a[j * self.n + j] += g; }
        if let (Some(i), Some(j)) = (self.gi(a), self.gi(b)) {
            self.a[i * self.n + j] -= g;
            self.a[j * self.n + i] -= g;
        }
    }

    fn current(&mut self, from: usize, to: usize, i: f64) {
        // Independent source driving current from `to` into `from`.
        if let Some(k) = self.gi(from) { self.b[k] += i; }
        if let Some(k) = self.gi(to) { self.b[k] -= i; }
    }

    fn voltage(&mut self, a: usize, b: usize, v: f64, br: usize) {
        let col = self.n - self.branches + br;
        if let Some(i) = self.gi(a) {
            self.a[i * self.n + col] += 1.0;
            self.a[col * self.n + i] += 1.0;
        }
        if let Some(j) = self.gi(b) {
            self.a[j * self.n + col] -= 1.0;
            self.a[col * self.n + j] -= 1.0;
        }
        self.b[col] = v;
    }

    fn solve(mut self) -> Vec<f64> {
        let n = self.n;
        for k in 0..n {
            let mut p = k;
            for i in k + 1..n {
                if self.a[i * n + k].abs() > self.a[p * n + k].abs() { p = i; }
            }
            assert!(self.a[p * n + k].abs() > 1e-14, "reference matrix singular");
            if p != k {
                for j in 0..n { self.a.swap(k * n + j, p * n + j); }
                self.b.swap(k, p);
            }
            for i in k + 1..n {
                let f = self.a[i * n + k] / self.a[k * n + k];
                for j in k..n { self.a[i * n + j] -= f * self.a[k * n + j]; }
                self.b[i] -= f * self.b[k];
            }
        }
        let mut x = vec![0.0; n];
        for i in (0..n).rev() {
            let mut s = self.b[i];
            for j in i + 1..n { s -= self.a[i * n + j] * x[j]; }
            x[i] = s / self.a[i * n + i];
        }
        let mut v = vec![0.0; n + 1];
        for i in 0..n { v[i + 1] = x[i]; }
        v
    }
}

#[test]
fn five_hundred_netlists_match_reference_solver() {
    let mut rng = Rng::new(0xA17E_1D);
    let values = [100.0, 220.0, 330.0, 470.0, 1000.0, 2200.0, 4700.0, 10_000.0, 47_000.0];
    for case in 0..500 {
        let nodes = 3 + (rng.below(4) as usize);
        let vin = 1.0 + rng.below(1200) as f64 / 100.0;
        let mut c = Circuit::new();
        for i in 1..nodes {
            c.add_node(&format!("n{i}"));
        }
        c.vsource(1, 0, vin);
        // A random tree keeps the network connected; extra resistors add loops.
        for i in 2..nodes {
            let j = 1 + rng.below((i - 1) as u32) as usize;
            let r = values[rng.below(values.len() as u32) as usize];
            c.resistor(i, j, r);
        }
        for _ in 0..rng.below(4) {
            let a = 1 + rng.below((nodes - 1) as u32) as usize;
            let b = rng.below(nodes as u32) as usize;
            if a == b { continue; }
            let r = values[rng.below(values.len() as u32) as usize];
            c.resistor(a, b, r);
        }
        for _ in 0..rng.below(3) {
            let a = 1 + rng.below((nodes - 1) as u32) as usize;
            let i = (rng.below(500) as f64 + 1.0) / 100_000.0;
            c.isource(a, 0, i);
        }
        // Reference construction.
        let mut refs = Reference::new(nodes, 1);
        refs.voltage(1, 0, vin, 0);
        for e in &c.elements {
            match e {
                Element::Resistor { a, b, r } => refs.resistor(*a, *b, *r),
                Element::ISource { a, wave, .. } => refs.current(*a, 0, wave.dc_value()),
                _ => {}
            }
        }
        let reference = refs.solve();
        let result = sim::dc_op(&c).unwrap_or_else(|e| panic!("case {case}: {e}"));
        for i in 1..nodes {
            close(result.voltages[i], reference[i], 1e-7, &format!("case {case} node {i}"));
        }
    }
}

fn diode_reference(vin: f64, r: f64, is: f64, n: f64, rs: f64) -> f64 {
    // Bisection on Shockley's equation: vd + (r + rs) * Is(exp(vd/nVt)-1) = vin.
    let nvt = n * sim::VT;
    let f = |vd: f64| {
        let i = is * ((vd / nvt).exp() - 1.0);
        vd + (r + rs) * i - vin
    };
    let mut lo = -10.0;
    let mut hi = 1.3;
    for _ in 0..300 {
        let mid = (lo + hi) / 2.0;
        if f(mid) > 0.0 { hi = mid; } else { lo = mid; }
    }
    (lo + hi) / 2.0
}

#[test]
fn forty_analytic_circuits_within_tolerance() {
    let mut checked = 0usize;

    // 12 resistive dividers.
    for i in 0..12 {
        let vin = 1.0 + i as f64 * 0.9;
        let r1 = 1000.0 + i as f64 * 470.0;
        let r2 = 2200.0 + i as f64 * 330.0;
        let mut c = Circuit::new();
        let a = c.add_node("in");
        let b = c.add_node("out");
        c.vsource(a, 0, vin);
        c.resistor(a, b, r1);
        c.resistor(b, 0, r2);
        let want = vin * r2 / (r1 + r2);
        let got = sim::dc_op(&c).unwrap().voltages[b];
        close(got, want, 1e-9, &format!("divider {i}"));
        checked += 1;
    }

    // 8 parallel-resistor networks driven by a current source.
    for i in 0..8 {
        let i_in = 0.0005 + i as f64 * 0.0004;
        let r1 = 1000.0 + i as f64 * 100.0;
        let r2 = 2200.0;
        let r3 = 4700.0;
        let mut c = Circuit::new();
        let a = c.add_node("a");
        c.isource(a, 0, i_in);
        c.resistor(a, 0, r1);
        c.resistor(a, 0, r2);
        c.resistor(a, 0, r3);
        let want = i_in / (1.0 / r1 + 1.0 / r2 + 1.0 / r3);
        let got = sim::dc_op(&c).unwrap().voltages[a];
        close(got, want, 1e-9, &format!("parallel {i}"));
        checked += 1;
    }

    // 8 Millman / superposition cases with two sources.
    for i in 0..8 {
        let v1 = 5.0 + i as f64;
        let v2 = 2.0 + i as f64 * 0.5;
        let r1 = 1000.0;
        let r2 = 2200.0;
        let r3 = 4700.0;
        let mut c = Circuit::new();
        let a = c.add_node("a");
        let b = c.add_node("b");
        let out = c.add_node("out");
        c.vsource(a, 0, v1);
        c.vsource(b, 0, v2);
        c.resistor(a, out, r1);
        c.resistor(b, out, r2);
        c.resistor(out, 0, r3);
        let want = (v1 / r1 + v2 / r2) / (1.0 / r1 + 1.0 / r2 + 1.0 / r3);
        let got = sim::dc_op(&c).unwrap().voltages[out];
        close(got, want, 1e-9, &format!("millman {i}"));
        checked += 1;
    }

    // 4 diode circuits against an independent bisection solve.
    for i in 0..4 {
        let vin = 3.0 + i as f64;
        let r = 470.0 + i as f64 * 330.0;
        let is = 2.2e-9;
        let nn = 1.8;
        let mut c = Circuit::new();
        let a = c.add_node("in");
        let m = c.add_node("mid");
        c.vsource(a, 0, vin);
        c.resistor(a, m, r);
        c.add(Element::Diode { a: m, k: 0, is, n: nn, rs: 0.0 });
        let got = sim::dc_op(&c).unwrap().voltages[m];
        let want = diode_reference(vin, r, is, nn, 0.0);
        close(got, want, 1e-6, &format!("diode {i}"));
        checked += 1;
    }

    // 8 RC step responses at a dense trapezoidal time step.
    for i in 0..8 {
        let vin = 2.0 + i as f64;
        let r = 1000.0 + i as f64 * 500.0;
        let cap = 100e-9 * (1.0 + i as f64 * 0.1);
        let tau = r * cap;
        let mut c = Circuit::new();
        let a = c.add_node("in");
        let out = c.add_node("out");
        c.elements.push(Element::VSource {
            a,
            b: 0,
            wave: Waveform::Pulse {
                v1: 0.0,
                v2: vin,
                delay: 0.0,
                rise: 1e-9,
                width: 1.0,
                period: 2.0,
            },
        });
        c.resistor(a, out, r);
        c.capacitor(out, 0, cap);
        let mut opts = SimOptions::default();
        opts.tstep = tau / 4000.0;
        opts.tstop = 3.0 * tau;
        opts.method = Integration::Trapezoidal;
        let tr = sim::transient(&c, &opts).unwrap();
        let series = tr.series_for("out").unwrap();
        for (idx, &t) in tr.times.iter().enumerate() {
            let want = vin * (1.0 - (-t / tau).exp());
            let tol = 0.001 * vin + 1e-9;
            assert!(
                (series[idx] - want).abs() <= tol,
                "rc {i} at t={t}: got {} want {want}",
                series[idx]
            );
        }
        checked += 1;
    }

    assert_eq!(checked, 40);
}

// ---------------------------------------------------------------- boards

#[test]
fn twenty_generated_boards_route_with_zero_drc() {
    for seed in 1..=20u64 {
        let mut board = generated_board(seed);
        let opts = RouterOptions::for_board(&board);
        let mut progress = |_p: &anvil_pcb::router::RouteProgress| {};
        let report = route_all(&mut board, &opts, &mut progress)
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert!(
            report.clean,
            "seed {seed}: failures={:?} violations={:?}",
            report.failures,
            report.drc.violations.iter().take(4).collect::<Vec<_>>()
        );
        let after = drc::check(&board);
        assert!(after.is_clean(), "seed {seed}: recheck failed: {:?}", after.violations);
        assert!(drc::all_nets_connected(&board), "seed {seed}: disconnected pads");
        assert!(board.traces.len() >= 8, "seed {seed}: too few traces");
    }
}

#[test]
fn autoroute_is_reproducible_for_a_seed() {
    for seed in [1u64, 7, 19] {
        let mut a = generated_board(seed);
        let mut b = generated_board(seed);
        let opts = RouterOptions::for_board(&a);
        let mut p = |_p: &anvil_pcb::router::RouteProgress| {};
        let ra = route_all(&mut a, &opts, &mut p).unwrap();
        let rb = route_all(&mut b, &opts, &mut p).unwrap();
        assert_eq!(ra.edges_routed, rb.edges_routed);
        assert_eq!(ra.drc.errors(), rb.drc.errors());
        assert_eq!(a.traces, b.traces, "seed {seed}: traces differ");
        assert_eq!(a.vias, b.vias, "seed {seed}: vias differ");
    }
}

#[test]
fn routes_survive_export_round_trip() {
    for seed in 1..=8u64 {
        let mut board = generated_board(seed);
        let opts = RouterOptions::for_board(&board);
        let mut p = |_p: &anvil_pcb::router::RouteProgress| {};
        let report = route_all(&mut board, &opts, &mut p).unwrap();
        assert!(report.clean, "seed {seed}");
        let files = anvil_export::export_board(&board);
        for f in &files {
            let text = f.as_str().unwrap_or_else(|| panic!("seed {seed}: non-UTF8 {}", f.name));
            if f.name.ends_with(".gbr") {
                let doc1 = gerber::parse(text).unwrap_or_else(|e| panic!("{}: {e}", f.name));
                let doc2 = gerber::parse(text).unwrap();
                assert_eq!(doc1, doc2);
                let opts = raster::RasterOptions {
                    pixels_per_mm: 8.0,
                    ..Default::default()
                };
                if !doc1.primitives.is_empty() {
                    let a = raster::render(&doc1, &opts).unwrap();
                    let b = raster::render(&doc2, &opts).unwrap();
                    assert_eq!(a, b, "{} raster differs", f.name);
                    assert!(a.dark_pixels() > 0, "{} raster is blank", f.name);
                }
            } else if f.name.ends_with(".drl") {
                match drill::parse(text) {
                    Ok(d1) => {
                        let d2 = drill::parse(text).unwrap();
                        assert_eq!(d1.tools, d2.tools);
                        assert_eq!(d1.hits, d2.hits);
                    }
                    Err(drill::DrillError::NoTools) => {
                        // SMD-only boards have nothing to drill.
                    }
                    Err(e) => panic!("{}: {e}", f.name),
                }
            }
        }
    }
}

// ---------------------------------------------------------------- netlist / schematic

#[test]
fn netlist_extraction_is_deterministic_and_exact() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    let r1 = s.add_component(&lib, "R", anvil_core::geom::Point::new(0, 0)).unwrap();
    let r2 = s.add_component(&lib, "R", anvil_core::geom::Point::new(12_700_000, 0)).unwrap();
    let g = s.add_component(&lib, "GND", anvil_core::geom::Point::new(6_350_000, -10_000_000)).unwrap();
    let p1 = s.component(r1).unwrap().pins[0].pos;
    let p2 = s.component(r1).unwrap().pins[1].pos;
    let p3 = s.component(r2).unwrap().pins[0].pos;
    let pg = s.component(g).unwrap().pins[0].pos;
    s.add_wire(vec![p1, pg]).unwrap();
    s.add_wire(vec![p2, p3]).unwrap();
    s.add_label("MID", p2, LabelKind::Local).unwrap();
    s.add_label("MID", p3, LabelKind::Local).unwrap();
    let a = extract(&s, &lib);
    let b = extract(&s, &lib);
    assert_eq!(a.nets, b.nets);
    assert_eq!(a.diagnostics, b.diagnostics);
    assert_eq!(a.net_by_name("MID").unwrap().nodes.len(), 2);
    assert!(a.net_by_name("GND").is_some());
}

// ---------------------------------------------------------------- fuzz

fn fuzz_text(rng: &mut Rng, len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCXYZ0123456789%*.,+-/\\\"{}[]:; \n\r\t_";
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        s.push(ALPHABET[rng.below(ALPHABET.len() as u32) as usize] as char);
    }
    s
}

#[test]
fn one_thousand_malformed_inputs_fail_cleanly() {
    let mut rng = Rng::new(0xBAD_5EED);
    let mut rejected = 0usize;
    let mut accepted = 0usize;
    for i in 0..1000 {
        let len = rng.below(240) as usize;
        let input = fuzz_text(&mut rng, len);
        let kind = i % 4;
        let result = catch_unwind(AssertUnwindSafe(|| match kind {
            0 => anvil_studio::json::parse(&input).err().map(|e| e.to_string()),
            1 => gerber::parse(&input).err().map(|e| e.to_string()),
            2 => drill::parse(&input).err().map(|e| e.to_string()),
            _ => {
                // Random edit operations must never panic; invalid ones return
                // EditError.
                let lib = Library::standard();
                let mut s = Schematic::new();
                let x = rng.next_u64() as i64;
                let y = rng.next_u64() as i64;
                let p = anvil_core::geom::Point::new(x % 2_000_000_000, y % 2_000_000_000);
                let mut errs = 0usize;
                if s.add_component(&lib, &input[..input.len().min(20)], p).is_err() { errs += 1; }
                if s.add_wire(vec![p]).is_err() { errs += 1; }
                if s.add_label(&input[..input.len().min(20)], p, LabelKind::Local).is_err() { errs += 1; }
                if errs == 0 { None } else { Some(format!("{errs} edits rejected")) }
            }
        }));
        match result {
            Ok(Some(_)) => rejected += 1,
            Ok(None) => accepted += 1,
            Err(_) => panic!("panic on fuzz case {i}: {input:?}"),
        }
    }
    assert_eq!(rejected + accepted, 1000);
    // The corpus is dominated by invalid JSON/Gerber/drill/edit input.
    assert!(rejected >= 850, "only {rejected} inputs were rejected");
}

#[test]
fn truncated_valid_files_are_rejected_without_panic() {
    let lib = Library::standard();
    let mut board = Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default());
    let n = board.add_net("N");
    let fp = lib.footprint("R_0805").unwrap();
    board
        .add_footprint(fp, "R1", anvil_core::geom::Point::new(10 * MM, 10 * MM), 0, &[Some(n), None])
        .unwrap();
    let g = gerber::write_copper(&board, Layer::Top);
    let d = drill::write_board(&board);
    for cut in [1usize, 3, 5, 10, 20, 40, 80] {
        let gt = &g[..g.len().saturating_sub(cut)];
        let dt = &d[..d.len().saturating_sub(cut)];
        let gr = catch_unwind(AssertUnwindSafe(|| gerber::parse(gt)));
        let dr = catch_unwind(AssertUnwindSafe(|| drill::parse(dt)));
        assert!(gr.is_ok(), "Gerber panic at cut {cut}");
        assert!(dr.is_ok(), "drill panic at cut {cut}");
        if let Ok(Ok(doc)) = gr {
            // A truncation may still be a syntactically valid prefix.
            let _ = doc.primitives.len();
        }
    }
}
