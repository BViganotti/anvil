//! Dependency-free benchmarks. Numbers are measured, never invented.
//!
//! Run with `cargo run --release -p anvil-bench`.

use anvil_core::library::Library;
use anvil_core::netlist::extract;
use anvil_core::schematic::Schematic;
use anvil_core::sim::{self, Circuit, Integration, SimOptions};
use anvil_export::{gerber, raster};
use anvil_pcb::board::Layer;
use anvil_pcb::drc;
use anvil_pcb::generate::generated_board;
use anvil_pcb::router::{route_all, RouteProgress, RouterOptions};
use anvil_studio::project::Project;
use std::time::Instant;

fn time<F: FnMut()>(name: &str, iterations: u32, mut f: F) {
    // Warm up once so allocator/page effects do not dominate tiny runs.
    f();
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    let per = elapsed.as_secs_f64() * 1000.0 / iterations as f64;
    println!(
        "{:<34} {:>9.3} ms/iter  {:>8.1} iters/s  ({} iters)",
        name,
        per,
        iterations as f64 / elapsed.as_secs_f64(),
        iterations
    );
}

fn demo_schematic() -> (Schematic, Library) {
    let lib = Library::standard();
    let p = Project::demo(&lib).unwrap();
    (p.schematic, lib)
}

fn rc_ladder() -> Circuit {
    let mut c = Circuit::new();
    let a = c.add_node("in");
    c.vsource(a, 0, 5.0);
    let mut prev = a;
    for i in 0..20 {
        let n = c.add_node(&format!("n{i}"));
        c.resistor(prev, n, 1000.0 + i as f64 * 10.0);
        c.capacitor(n, 0, 10e-9);
        prev = n;
    }
    c
}

fn main() {
    println!("Anvil benchmarks (std::time::Instant, single thread)");
    println!("{}", "-".repeat(78));

    let (sch, lib) = demo_schematic();
    let mut it = 0;
    time("netlist extract (demo)", 2000, || {
        let nl = extract(&sch, &lib);
        assert!(nl.errors() == 0);
        it += 1;
    });
    let _ = it;

    let nl = extract(&sch, &lib);
    let circuit = sim::compile_schematic(
        &sch,
        &nl,
        &lib,
        &anvil_core::sim::CompileOptions::default(),
    )
    .unwrap();
    time("DC operating point (demo)", 2000, || {
        let r = sim::dc_op(&circuit).unwrap();
        assert!(r.voltages.iter().all(|v| v.is_finite()));
    });

    let mut opts = SimOptions::default();
    opts.tstep = 1e-6;
    opts.tstop = 2e-3;
    opts.method = Integration::Trapezoidal;
    time("transient 2000 steps (demo)", 20, || {
        let r = sim::transient(&circuit, &opts).unwrap();
        assert!(r.steps > 1000);
    });

    let big = rc_ladder();
    let mut big_opts = SimOptions::default();
    big_opts.tstep = 1e-6;
    big_opts.tstop = 1e-3;
    time("transient 1000 steps (20-node RC)", 20, || {
        let r = sim::transient(&big, &big_opts).unwrap();
        assert!(r.steps > 500);
    });

    for seed in 1..=5u64 {
        let mut board = generated_board(seed);
        let ropts = RouterOptions::for_board(&board);
        let start = Instant::now();
        let mut progress = |_p: &RouteProgress| {};
        let report = route_all(&mut board, &ropts, &mut progress).unwrap();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{:<34} {:>9.3} ms       {} nets, {} traces, {} vias, clean={}",
            format!("autoroute generated board {seed}"),
            ms,
            report.nets_total,
            board.traces.len(),
            board.vias.len(),
            report.clean
        );
        assert!(report.clean);
    }

    // Export benchmark on a routed board.
    let mut board = generated_board(3);
    let ropts = RouterOptions::for_board(&board);
    let mut p = |_p: &RouteProgress| {};
    route_all(&mut board, &ropts, &mut p).unwrap();
    let text = gerber::write_copper(&board, Layer::Top);
    time("DRC (routed generated board)", 20, || {
        let r = drc::check(&board);
        assert!(r.is_clean());
    });
    time("Gerber write (top copper)", 1000, || {
        let s = gerber::write_copper(&board, Layer::Top);
        assert!(s.contains("M02*"));
    });
    time("Gerber parse (top copper)", 1000, || {
        let d = gerber::parse(&text).unwrap();
        assert!(!d.primitives.is_empty());
    });
    let doc = gerber::parse(&text).unwrap();
    let ropts = raster::RasterOptions {
        pixels_per_mm: 10.0,
        ..Default::default()
    };
    time("Gerber raster 10 px/mm", 50, || {
        let img = raster::render(&doc, &ropts).unwrap();
        assert!(img.dark_pixels() > 0);
    });
    println!("{}", "-".repeat(78));
    println!("done");
}
