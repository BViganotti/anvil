//! Anvil launcher. The product is the Studio; this binary starts the loopback
//! server, opens the browser, and offers batch export/example generation used by
//! tests and benchmarks.

fn main() {
    println!("Anvil {} — starting", env!("CARGO_PKG_VERSION"));
}
