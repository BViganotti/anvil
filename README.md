# Anvil

Anvil is a local browser EDA workbench: draw a schematic, simulate it, lay out a
board, run design rule checks, autoroute every trace, and export Gerber and
Excellon drill files. It runs entirely on your machine, binds only to
`127.0.0.1`, and ships as a single Rust binary with its UI compiled in.

No external crates. No CDN. No runtime downloads. Rust standard library only.

```
┌────────────┐   ┌────────────┐   ┌────────────┐   ┌────────────┐
│ Schematic  │ → │  Netlist   │ → │ Simulation │ → │  PCB / DRC │
└────────────┘   └────────────┘   └────────────┘   └────────────┘
                                                        ↓
                              ┌────────────┐   ┌────────────┐
                              │  Gerber /  │ ← │ Autorouter │
                              │  Excellon  │   └────────────┘
                              └────────────┘
```

## Quick start

```sh
cargo run --release
```

The launcher starts the Studio on a random loopback port, prints the URL in the
browser it opens, and keeps the server alive until you stop it. The Studio loads
the demo project automatically.

Other entry points (used by tests, benchmarks and batch export, not a CLI
product):

```sh
cargo run --release -- --port 8787       # fixed port, still loopback only
cargo run --release -- --no-browser      # server only
cargo run --release -- export <dir>      # batch export the demo to Gerber/drill
cargo run --release -- generate-examples # write deterministic example projects
```

## What it does

- **Schematic capture** — components, pins, polyline wires, junctions, local /
  global / power labels, buses, a built-in library of passives, diodes,
  sources, switches, logic gates, a D flip-flop and connectors.
- **Netlist extraction** — exact integer geometry, deterministic naming,
  diagnostics for conflicting labels, dangling pins and bad bus references.
- **Simulation** — modified nodal analysis, dense LU with partial pivoting,
  Newton iteration for Shockley diodes with series resistance, DC operating
  point, backward-Euler and trapezoidal transient analysis, and an event-driven
  digital engine for gates and flip-flops. Tested against analytic solutions.
- **PCB** — board outline, up to four copper layers, footprints, SMD and
  through-hole pads, traces, vias, ratsnest.
- **DRC** — clearance, track width, drill, annular ring, shorts, board-edge and
  unrouted checks, with spatial indexing and deterministic reports.
- **Autorouter** — deterministic grid A* on two layers with vias, net ordering,
  rip-up and retry; zero-DRC acceptance on generated boards.
- **Export** — Gerber RS-274X copper, silkscreen and outline; Excellon drill;
  built-in Gerber parser and rasterizer for verification, with PNG preview.
- **Studio** — original HTML/CSS/JS served from the binary: schematic canvas,
  simulation plots, board canvas, DRC panel, autoroute progress over SSE and
  Gerber preview.

## Workspace

| Crate | Purpose |
|---|---|
| `anvil-core` | schematic, library, netlist, simulation |
| `anvil-pcb` | board model, footprints, DRC, autorouter, board generation |
| `anvil-export` | Gerber, Excellon, parser, rasterizer, PNG |
| `anvil-studio` | JSON, HTTP/SSE, API and embedded assets |
| `anvil` (root) | launcher and batch tools |
| `bench` | standard-library benchmarks |
| `fuzz` | malformed input harness |

## Development

```sh
cargo build --release          # zero warnings
cargo test                     # debug tests
cargo test --release           # release tests
cargo run --release -p anvil-bench
cargo run --release -p anvil-fuzz -- 2000
```

The acceptance suite lives in `tests/` and covers analytic circuits, generated
boards, Gerber round trips and malformed input. Browser coverage lives in
`tests/studio.spec.mjs` and uses Playwright when available.

## Determinism and soundness

- All geometry uses integer nanometres; connectivity and DRC are exact.
- All randomness is a documented seeded xorshift generator.
- Every parser has hard limits on bytes, tokens, nesting and element counts.
- Malformed input returns `Result`, never panics; tests and fuzz harnesses
  enforce this under `debug_assertions`.
- Simulation and DRC report real results or errors. Nothing is fabricated.

## License

MIT. See [LICENSE](LICENSE). No third-party code is vendored.
