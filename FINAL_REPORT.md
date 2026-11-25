# Anvil — Final Report

Status: **complete against the Definition of Done in `REQUIREMENTS.md`**.
Version 0.1.0, MIT, Rust standard library only. Repository:
<https://github.com/BViganotti/anvil>

This report contains measured results only. Where something is not done, it is
listed under known gaps and in `LIMITATIONS.md`.

## Definition of Done

| # | Requirement | Status |
|---:|---|---|
| 1 | `cargo build --release` succeeds with zero warnings | yes, checked after the final change |
| 2 | `cargo test` and `cargo test --release` pass | yes, 269 test cases, 45 suites |
| 3 | Project-specific acceptance tests pass | yes: 500 netlists, 40 analytic circuits, 20 zero-DRC boards, Gerber round trips, 1,000 malformed inputs, Playwright |
| 4 | 1,000 malformed inputs fail cleanly, never panic | yes: acceptance fuzz + 2,000-case harness, 0 panics |
| 5 | Browser Studio opens and demo works via Playwright | yes: draw, simulate, DRC, autoroute, PNG preview |
| 6 | README, docs, CHANGELOG, LICENSE, LIMITATIONS, FINAL_REPORT | yes |
| 7 | Final report lists LOC, commits, tests, coverage, benchmarks, gaps | this document |
| 8 | Repository committed and pushed | yes, public GitHub repository, 95 commits |
| 9 | No external crate, no CI, no CLI-only product | yes: path-only deps, no `.github`, Studio is the product |

## Size

| Area | Lines |
|---|---:|
| Production Rust (`crates/` + `src/`) | 17,809 |
| Crate/root tests, benchmark and fuzz (Rust) | 1,423 |
| Embedded UI (HTML/CSS/JS/SVG) | 1,566 |
| **Code total** | **~20,800** |
| Documentation (Markdown) | not included above |
| Example projects | 24 JSON files |
| Example Gerber/drill set | 5 files |

Production Rust is inside the requested 15–18k range; the total including tests,
tools and UI is larger.

## Commits

**95 commits**, one per vertical slice or verified fix, starting from the
scaffold. Every commit builds and passes the test suite for its tree. The work
was pushed to `BViganotti/anvil` (public). There is no CI configuration.

## Tests

`cargo test --workspace` runs 45 suites / **269 test cases**, all passing. They
break down as:

- **anvil-core (98)** — geometry; schematic editing; netlist junctions, scopes,
  buses and conflicts; library contract; DC solver; transient IC/PWL/limits;
  digital truth tables, DFF, delays; compilation errors; utilities.
- **anvil-pcb (56)** — board model; pad/trace/via geometry; connectivity and
  ratsnest; DRC width/drill/annular/edge/short/unrouted; router options and
  single-net routing; board generation; layer and threshold regressions.
- **anvil-export (40)** — Gerber/Excellon writer-parser round trips; dialect
  rejections; inch units; four layers; raster fit/invert/holes; PNG blocks.
- **anvil-studio (47)** — JSON limits; HTTP parsing over real sockets; project
  round trips; API dispatch; server auth/CSP/static assets; background route job
  lifecycle.
- **Root acceptance (28)** — 500 reference netlists, 40 analytic circuits, 20
  zero-DRC boards, deterministic replay, export round trips, 1,000 malformed
  inputs, HTTP/SSE end-to-end, CLI batch export, examples, repository hygiene,
  malformed HTTP campaign, regressions.

Browser coverage is a separate Playwright script (not counted in 269).

### Acceptance evidence

- **500 netlists** — random connected resistor/source networks solved by an
  independently implemented Gaussian MNA reference in the test. Relative error
  tolerance `1e-7`; all pass.
- **40 analytic circuits** — 12 dividers, 8 parallel networks, 8 Millman
  networks, 4 diodes against an independent bisection Shockley solve, 8 RC
  transients against the closed-form exponential. Requirement 0.1%; checks are
  tighter (`1e-9` relative linear, `1e-6` diode).
- **20 generated boards** — every board routes all multi-pad nets, DRC is clean
  on rerun and all pads are connected. Replaying three seeds produces
  byte-identical traces and vias.
- **Gerber/drill** — every routed board from eight seeds exports copper,
  outline, silk and drill; all files re-parse, top copper rasterizes pixel
  identically twice.
- **Fuzzing** — 1,000 malformed JSON/Gerber/drill/edit inputs in acceptance
  (at least 850 rejections, 0 panics); 2,000-case standalone harness reports
  1,719 rejected, 281 accepted, 0 panics. 60 malformed HTTP requests plus a
  binary blob leave the server healthy.
- **Studio** — Playwright launches the real binary, places a resistor, reloads
  the demo, runs a transient, generates a board, runs DRC, autoroutes to
  `clean`, and loads the PNG preview. No console or page errors.

## Coverage

Measured with `cargo llvm-cov --workspace --summary-only` (raw table in
`docs/COVERAGE.txt`):

| Metric | Covered / Total | Percent |
|---|---:|---:|
| Lines | 19,740 / 23,171 | **85.19%** |
| Regions | 10,260 / 12,086 | **84.89%** |
| Functions | 899 / 1,230 | **73.09%** |

Highlights: LU 92% lines, simulation 87%, library 94%, netlist 85%, DRC 81%,
router 89%, Gerber 82%, raster 88%, API 88%, JSON 86%, project 85%. Lower
coverage is concentrated in CLI-only batch paths, fuzz/bench binaries and
defensive branches. Coverage is a signal, not the acceptance contract.

## Benchmarks

Release build, single thread, development machine. Raw run is reproducible with
`cargo run --release -p anvil-bench`.

| Operation | Time |
|---|---:|
| Netlist extraction, demo | 0.021 ms |
| DC operating point, demo | 0.002 ms |
| Transient, 2,000 steps, demo | 1.409 ms |
| Transient, 1,000 steps, 20-node RC ladder | 4.007 ms |
| DRC, routed 10-net generated board | 0.255 ms |
| Gerber write / parse, top copper | 0.027 / 0.010 ms |
| Gerber raster, 10 px/mm | 0.112 ms |
| Autoroute, 10-net generated board | 2.3–4.3 ms |

Autoroute timings include obstacle masks, all A* searches and a full DRC pass.
No performance numbers are estimated or scaled.

## Deliverables

- Source: `crates/anvil-core`, `anvil-pcb`, `anvil-export`, `anvil-studio`,
  launcher, `bench`, `fuzz`.
- Embedded Studio assets: `assets/` (original HTML/CSS/JS/SVG).
- 24 example projects in `examples/` plus a real routed Gerber/drill set.
- Documentation: architecture, simulation, router, DRC, Gerber, Studio, project
  format, library, security, testing, benchmarks, coverage, limitations.
- Acceptance, HTTP/SSE, CLI, examples, fuzz, regression and repository tests.
- Playwright browser test and screenshots.
- MIT `LICENSE`, `CHANGELOG.md`, `README.md`, `Makefile`.

## Verification commands

```sh
cargo build --release                 # zero warnings
cargo test                            # debug suite
cargo test --release                  # release suite
cargo run --release -p anvil-fuzz -- 2000
cargo run --release -p anvil-bench
python3 tests/studio_playwright.py
cargo run --release -- export examples/out
cargo run --release -- generate-examples
```

## Known gaps and honest limitations

- Simulation is a documented subset (no MOSFET/BJT/op-amp/AC/noise/thermal);
  digital delays are quantised by the transient step.
- Autorouting is two-layer, orthogonal and grid-based; dense boards can fail and
  manual routing is not preserved when autoroute starts.
- DRC lacks copper pours, keepouts, solder mask and impedance rules.
- Gerber supports the common subset only; arcs, regions, macros and clear
  polarity are rejected explicitly.
- The Studio holds one in-memory project; no file open/save dialog, undo or
  multi-user support.
- No CI/CD, by requirement; verification is manual and documented.

Full details: [LIMITATIONS.md](LIMITATIONS.md).
