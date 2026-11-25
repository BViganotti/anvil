# Benchmarks

All numbers are measured with `cargo run --release -p anvil-bench` on a single
thread on the development machine (Apple silicon, macOS, stable Rust). The
benchmark harness uses `std::time::Instant`, warms up once, and prints
milliseconds per iteration and iterations per second. Nothing is estimated.

## Latest run

| Operation | Time | Throughput |
|---|---:|---:|
| Netlist extraction, demo schematic | 0.021 ms | 48,188 /s |
| DC operating point, demo circuit | 0.002 ms | 564,938 /s |
| Transient, 2,000 steps, demo | 1.409 ms | 710 /s |
| Transient, 1,000 steps, 20-node RC ladder | 4.007 ms | 250 /s |
| DRC, routed 10-net generated board | 0.255 ms | 3,914 /s |
| Gerber write, top copper | 0.027 ms | 36,496 /s |
| Gerber parse, top copper | 0.010 ms | 102,505 /s |
| Gerber raster, 10 px/mm | 0.112 ms | 8,967 /s |

## Autoroute

Generated 10-net boards, full route with DRC verification:

| Seed | Time | Nets | Traces | Vias | Clean |
|---:|---:|---:|---:|---:|:---:|
| 1 | 3.750 ms | 10 | 16 | 6 | yes |
| 2 | 2.289 ms | 10 | 16 | 6 | yes |
| 3 | 2.737 ms | 10 | 14 | 4 | yes |
| 4 | 4.286 ms | 10 | 22 | 12 | yes |
| 5 | 2.803 ms | 10 | 14 | 4 | yes |

## Reading the numbers

- The demo transient is a 4-net circuit; the 20-node RC ladder is a denser matrix
  and shows the expected O(n²)–O(n³) effect of dense LU.
- Gerber parsing is fast because the dialect is bounded and the file is small;
  rasterization dominates export preview time.
- Autoroute times include obstacle-mask construction, A* for every MST edge,
  and a full DRC pass. They do not include browser rendering or PNG encoding.

## Reproduce

```sh
cargo run --release -p anvil-bench
cargo run --release -p anvil-fuzz -- 2000
python3 tests/studio_playwright.py
```

Debug builds are roughly 5–20× slower for simulation and routing; the acceptance
suite is still designed to finish quickly in debug.
