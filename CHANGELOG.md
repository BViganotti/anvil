# Changelog

All notable changes to Anvil are recorded here. The project is pre-1.0 and
follows a milestone plan in `PLAN.md`.

## [0.1.0] — unreleased

### Added
- Cargo workspace with `anvil-core`, `anvil-pcb`, `anvil-export`,
  `anvil-studio`, launcher, benchmark and fuzz crates. Standard library only.
- Integer nanometre geometry with segment, rectangle, polygon and raster helpers.
- Schematic model: components, pins, polyline wires, junctions, labels, buses,
  sheet metadata, structural validation and edit operations.
- Component library with symbols, footprints and parts for passives, diodes,
  LED, voltage/current sources, ground/supply, switch, logic gates, D flip-flop
  and 2/4/8-pin connectors.
- Netlist extraction with union-find connectivity, junction rules, label merge
  by name, bus member expansion, deterministic net naming and diagnostics.
- Simulation: modified nodal analysis, dense LU with partial pivoting, DC
  operating point, Newton diode solve, backward-Euler and trapezoidal transient
  integration, and an event-driven digital engine.
- Analytic tests for resistive networks, diodes, RC transients and logic gates.

### Changed
- Initial public plan and requirements.

### Known gaps
- See `LIMITATIONS.md` as milestones land.
