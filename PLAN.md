# Anvil — Implementation Plan

Status: ready for one-shot implementation.

## Mission

Anvil is a local browser EDA workbench: draw a schematic, simulate it, lay out a
board, run design-rule checks, autoroute every trace, and export Gerbers and
drill files. It is a complete toolchain, not a schematic toy.

No database, no CRDT, no crash analysis, no observability, no CLI product.

## Product shape

- `anvil-core`: schematic model, netlist, component library, simulation.
- `anvil-pcb`: board model, footprints, layers, DRC, autorouter.
- `anvil-export`: Gerber RS-274X, Excellon drill, rasterizer.
- `anvil-studio`: local loopback HTTP/SSE Studio with embedded assets.
- `examples/`: generated boards and demo projects.
- `tests/`, `fuzz/`, `bench/`.

The user-facing product is the Studio and the embeddable library. A CLI may
exist only for tests, benchmarks and batch exports.

## Scope

### In v1
- Schematic editor model: components, pins, wires, junctions, labels, buses.
- Netlist extraction and validation.
- Component library with symbols and footprints.
- Analog/digital simulation subset: resistors, capacitors, inductors, diodes,
  ideal switches, voltage/current sources, logic gates, flip-flops.
- Transient and DC operating-point analysis with an independent dense linear
  solver and event-driven digital engine.
- PCB layout: board outline, layers, pads, traces, vias, footprints, ratsnest.
- Design-rule checks: clearance, width, drill, annular ring, shorts, unrouted.
- Grid-based multi-layer autorouter with rip-up and retry.
- Gerber RS-274X and Excellon drill export.
- Gerber rasterizer for visual verification.
- Studio: schematic canvas, simulation plots, board canvas, DRC panel, autoroute
  progress, Gerber preview.
- Deterministic seeds and reproducible routing.

### Out of v1
- Full SPICE model library, Monte Carlo, thermal, 3D field solving.
- RF simulation, high-speed signal integrity, electromagnetics.
- Manufacturing panelization and assembly files.
- Real CAD interop beyond simple import/export.

## Architecture

```
anvil/
  Cargo.toml
  crates/
    anvil-core/
      src/schematic/    # components, pins, nets, labels
      src/netlist/      # extraction, validation
      src/sim/          # MNA, devices, transient, digital events
    anvil-pcb/
      src/board/        # outline, layers, footprints, pads
      src/drc/          # clearance, width, drill, shorts
      src/router/       # grid router, rip-up, multi-layer
    anvil-export/
      src/gerber.rs
      src/drill.rs
      src/raster.rs
    anvil-studio/
      src/server.rs     # std HTTP/1.1 + SSE
      src/json.rs
      src/assets.rs
  assets/
  examples/
  tests/ fuzz/ bench/
```

Rust std only. Own JSON, HTTP/SSE, rasterizer and UI assets.

## LOC budget (target 15–18k)

| Module | LOC |
|---|---:|
| Schematic/netlist | 2,200 |
| Simulation | 2,800 |
| PCB model/footprints | 2,000 |
| DRC | 1,200 |
| Autorouter | 2,500 |
| Gerber/drill/raster | 1,800 |
| Studio server/API | 1,200 |
| UI assets | 1,600 |
| Tests/fuzz/bench | 1,800 |

## Milestones and commit ranges

### P0 Scaffold (commits 1–8)
Cargo workspace, test harness, README, LICENSE, Makefile, no CI, no crates.

### P1 Schematic and netlist (commits 9–22)
Component/pin model → wires/junctions → labels/buses → netlist extraction →
validation → component library → generated schematic fixtures.

### P2 Simulation (commits 23–38)
MNA matrix assembly → LU solver → DC operating point → transient integration →
diodes/switches → logic gates/flip-flops → event queue → plots → analytic tests.

### P3 PCB model and DRC (commits 39–52)
Board outline/layers → footprints/pads → traces/vias → ratsnest → clearance and
width checks → drill/annular checks → shorts → DRC fixtures.

### P4 Autorouter (commits 53–68)
Grid graph → single-net A* → multi-layer vias → rip-up/retry → ordering → rule
constraints → zero-DRC tests → deterministic seed replay.

### P5 Export and raster (commits 69–78)
Gerber RS-274X → Excellon drill → parser → rasterizer → round-trip visual tests.

### P6 Studio (commits 79–92)
HTTP/JSON/SSE → embedded assets → schematic canvas → simulation plots → board
canvas → DRC panel → autoroute progress → Gerber preview.

### P7 Demo and hardening (commits 93–108)
20 boards → autoroute all with zero DRC → fuzz netlists/Gerbers → ASan-equivalent
debug assertions → benchmarks → docs → FINAL_REPORT.

## Testing

- 500 netlists match an independent analytic/reference solver built into tests.
- 40 textbook circuits match closed-form DC/transient answers.
- 20 generated boards autoroute with zero DRC errors.
- Gerbers re-parse and rasterize identically.
- 1,000 fuzzed schematics/netlists/Gerbers fail cleanly.
- Deterministic autoroute seeds reproduce the same routes.
- Studio Playwright: draw, simulate, route, preview.

## Acceptance criteria

- Build zero warnings; tests pass.
- 40 analytic circuits within 0.1%.
- 20 boards route with zero DRC errors.
- Gerber/drill exports open in the built-in rasterizer and a standard viewer.
- Studio runs the full flow from schematic to Gerber preview.
- No external crates; no CI; no CLI-only product.

## Risks and fallbacks

- Full SPICE is endless: scope to a documented device subset and analytic tests.
- Autorouting can fail: ship a deterministic grid router with rip-up/retry and
  clear failure reporting; never claim zero DRC unless verified.
- Gerber dialect breadth: support the common subset and round-trip through the
  internal parser/rasterizer.
