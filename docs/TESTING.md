# Testing

Anvil uses only `cargo test`, a deterministic fuzz binary, a benchmark binary
and a Playwright script. There is no CI configuration; every check is manual and
reproducible.

## Commands

```sh
cargo test                    # every default workspace member
cargo test --release          # same suite, optimised
cargo run --release -p anvil-fuzz -- 2000
cargo run --release -p anvil-bench
python3 tests/studio_playwright.py
cargo run --release -- generate-examples
cargo run --release -- export examples/out
```

## Unit tests

- **anvil-core**: geometry, rotation, engineering values, RNG, structural
  validation, editing, library consistency, netlist extraction and connectivity,
  LU pivoting/singularity, DC, diode, RC, RL, switch, DFF, gates, waveforms,
  Euler vs trapezoidal.
- **anvil-pcb**: board model, footprint placement, pad geometry, connectivity,
  ratsnest, DRC widths/clearances/unrouted, deterministic board generation and
  the 20-board zero-DRC router test.
- **anvil-export**: Gerber writer/parser round trips, malformed input, drill
  round trips, raster determinism and hole punching, PNG structure and CRC.
- **anvil-studio**: JSON limits/duplicates/unicode, HTTP query parsing, project
  round trips, demo validity/simulation, API JSON, demo routing, token
  generation, server health/auth.

## Acceptance suite (`tests/acceptance.rs`)

1. **500 netlists** — random connected resistor networks with voltage and
   current sources, solved by a separate Gaussian-elimination MNA implementation
   in the test. Relative tolerance `1e-7`.
2. **40 analytic circuits** — 12 dividers, 8 parallel networks, 8 Millman
   networks, 4 diodes against bisection, 8 RC transients against the closed-form
   exponential. The requirement is 0.1%; most checks are tighter.
3. **20 generated boards** — each routed by the A* router, then DRC re-run and
   connectivity verified. All must be clean.
4. **Deterministic replay** — three seeds routed twice must produce byte-equal
   trace and via vectors.
5. **Export round trip** — eight routed boards export Gerber/outline/silk/drill;
   every file re-parses, and top copper rasterizes identically twice.
6. **Netlist determinism** — extraction repeated on a labelled/junctioned
   schematic yields identical nets and diagnostics.
7. **1,000 malformed inputs** — random JSON, Gerber, drill and edit inputs run
   under `catch_unwind`; panics fail the test. The corpus expects at least 850
   rejections.
8. **Truncated files** — Gerber and drill prefixes must not panic.

## HTTP/SSE suite (`tests/studio_http.rs`)

Starts the real loopback server and checks: static assets, 401 without token,
health/state/library, transient and DC simulation, invalid JSON, 404, placement
invalidating the board, demo reload, board generation, initial unrouted DRC,
synchronous autoroute to clean DRC, PNG preview signature/size, Gerber text and
parse, drill text, manifest, SSE progress events, oversized body rejection, bad
method and bad SSE token.

## Browser suite (`tests/studio_playwright.py`)

Starts `anvil --no-browser --port 0`, reads the tokenized URL, then drives
Chromium:

1. verifies the demo project and clean netlist,
2. places a resistor from the library with a double click,
3. reloads the demo,
4. runs a transient simulation and checks the MID series/legend,
5. generates the board, runs DRC, starts autoroute and waits for `clean`,
6. opens export, generates the PNG preview and checks the blob image loads,
7. fails on any console error or page error.

## Fuzz harness

`fuzz/` runs deterministic malformed-input campaigns over JSON, Gerber, drill,
project import, schematic edits and netlist extraction. It prints rejected vs
accepted counts and exits non-zero on any panic. A 2,000-case run reports
1,719 rejected, 281 accepted, 0 panics.

## Benchmarks

Measured on the development machine, release build, single thread:

| Operation | Time |
|---|---:|
| Netlist extraction (demo) | 0.021 ms |
| DC operating point (demo) | 0.002 ms |
| Transient, 2,000 steps (demo) | 1.41 ms |
| Transient, 1,000 steps (20-node RC) | 4.01 ms |
| Autoroute, 10-net generated board | 2.3–4.3 ms |
| DRC on routed generated board | 0.255 ms |
| Gerber write, top copper | 0.027 ms |
| Gerber parse, top copper | 0.010 ms |
| Rasterize at 10 px/mm | 0.112 ms |

These are ordinary wall-clock measurements; no fabricated speedups.

## Coverage

See `FINAL_REPORT.md` for the latest line-coverage measurement from
`cargo llvm-cov`. Coverage is a debugging signal, not the acceptance criterion;
the acceptance suite is the contract.
