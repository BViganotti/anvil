# Changelog

All notable changes to Anvil are recorded here. The project is pre-1.0 and
follows the milestone plan in `PLAN.md`.

## [0.1.0] — unreleased

### Added
- Cargo workspace with `anvil-core`, `anvil-pcb`, `anvil-export`,
  `anvil-studio`, launcher, benchmark and fuzz crates. Standard library only,
  no external crates, no CI.
- Integer nanometre geometry: segment/rectangle/polygon distance and containment.
- Schematic model and editor: components, pins, polyline wires, junctions,
  labels, buses, sheet metadata, validation and bounded edit operations.
- Built-in library: resistors, capacitors, inductors, diodes/LED, voltage and
  current sources, ground/supply, ideal switch, AND/OR/XOR/NAND/NOR/NOT, DFF,
  and 2/4/8-pin connectors with original symbols and footprints.
- Netlist extraction: union-find connectivity, junction rules, label merge,
  bus member expansion, deterministic names and diagnostics.
- Simulation: MNA assembly, dense LU with partial pivoting, DC operating point,
  diode Newton iteration, backward-Euler and trapezoidal transient analysis,
  event-driven digital gates and D flip-flop. Schematic-to-circuit compiler.
- PCB model: outline, 2/4 layers, footprints, pads, traces, vias, design rules,
  ratsnest and copper connectivity.
- DRC: structure, widths, drills, annular rings, clearances, shorts, board edge,
  outside-board and unrouted checks with spatial indexing.
- Autorouter: deterministic two-layer grid A*, vias, MST ordering, rip-up/retry,
  multi-attempt retry and DRC gating. 20 generated boards route clean.
- Export: Gerber RS-274X writer/parser, Excellon drill writer/parser, Gerber
  rasterizer with drilled-hole punching, and a dependency-free PNG encoder.
- Studio: loopback HTTP/1.1 server, per-launch token, strict CSP, JSON API,
  SSE autoroute progress, project model, demo, 24 examples, schematic canvas,
  simulation plots, board canvas, DRC panel, Gerber preview and downloads.
- Launcher: random/fixed port, browser auto-open, batch export and example
  generation.
- Tests: 269 Rust test cases, 500 reference netlists, 40 analytic circuits, 20
  zero-DRC boards, deterministic replay, Gerber round trips, 1,000 malformed
  inputs, HTTP/SSE end-to-end, Playwright browser flow.
- Deterministic fuzz harness and measured standard-library benchmarks.
- Docs: architecture, simulation, router, DRC, Gerber, Studio, testing,
  examples, limitations and final report.

### Changed
- Workspace default members include every crate so bare `cargo test` runs the
  complete suite.
- Studio accepted sockets are explicitly blocking, fixing partial writes of
  large PNG previews.

### Known gaps
- See `LIMITATIONS.md`: bounded simulation subset, two-layer autorouting,
  Gerber subset, single in-memory Studio project, no CI by requirement.
