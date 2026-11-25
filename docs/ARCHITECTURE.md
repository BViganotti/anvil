# Architecture

Anvil is a Rust workspace with four libraries and one launcher. The only
dependencies are path dependencies inside the workspace; the standard library is
the sole external API. There is no build script, no vendored code and no runtime
download.

```
anvil (binary)
├── anvil-studio ──┬── anvil-export ── anvil-pcb ── anvil-core
│                  └────────────────── anvil-pcb
├── anvil-export
├── anvil-pcb
└── anvil-core
```

## Units and exactness

Every geometric coordinate is an integer number of nanometres (`i64`). The
schematic grid is 1.27 mm (50 mil), but wires and labels may land on arbitrary
nanometre coordinates. Board geometry, clearance tests, Gerber coordinates and
raster transforms all derive from the same integer space. Squares are computed
in `i128` where overflow is possible; parsers reject coordinates beyond ±1e12 nm.

This is a deliberate choice over floating point: connectivity of a T junction,
a clearance exactly equal to the rule, and Gerber round trips must not depend on
rounding accidents.

## Crates

### anvil-core

- `geom`: points, rectangles, exact segment intersection/distance, polygon
  containment and area.
- `util`: deterministic xorshift RNG, engineering-value parsing, hard limits.
- `schematic`: components, pins, wires, junctions, labels, buses, structural
  validation and editing with `Result` errors.
- `library`: original symbols, footprints and parts compiled into the binary.
- `netlist`: geometric connectivity using union-find, label merging, bus member
  expansion, deterministic naming and diagnostics.
- `sim`: modified nodal analysis, dense LU, diode Newton iteration, DC and
  transient analysis, event-driven digital devices, schematic compilation.

### anvil-pcb

- `board`: integer PCB model, layers, footprints, pads, traces, vias, rules.
- `geom`: pad/trace/via clearance predicates and board-edge geometry.
- `connect`: copper connectivity, pad groups, ratsnest MST.
- `drc`: structure, widths, drills, annular rings, clearances, shorts, board
  edges and unrouted connections.
- `generate`: schematic-to-board import and deterministic synthetic boards for
  acceptance testing.
- `router`: two-layer grid A* with vias, ordering, rip-up/retry and DRC gating.

### anvil-export

- `gerber`: RS-274X writer and parser.
- `drill`: Excellon writer and parser.
- `raster`: Gerber rasterizer with drilled-hole punching.
- `png`: stored-deflate PNG encoder.

### anvil-studio

- `json`: bounded JSON parser/serializer.
- `http`: bounded HTTP/1.1 request parsing, response writing and SSE headers.
- `project`: demo/examples and `.anvl` JSON (de)serialization.
- `api`: JSON views, editing endpoints, simulation, DRC, route jobs, exports.
- `server`: loopback listener, token authentication, static assets and SSE.
- `assets`: original HTML/CSS/JS embedded with `include_str!`.

## Data flow

1. The schematic stores no net ids. The netlist is always derived.
2. Board generation imports the netlist, maps pins by number, skips power
   symbols, and places footprints in shelves.
3. DRC and the router share the same integer geometry predicates.
4. Export writes Gerber and drill files; the preview path parses the generated
   Gerber back before rasterizing.
5. Studio serializes the same models to JSON for the browser. There is no second
   source of truth.

## Threading and memory

- The HTTP listener uses one short-lived thread per request, capped at 64
  concurrent connections.
- The current project is guarded by a `Mutex`; poisoned locks are recovered
  rather than panicking.
- Autoroute runs on a worker thread and reports progress through the shared job
  state consumed by SSE.
- All collections are bounded by the constants in `anvil_core::util::limits`.
  Every parser checks input size before allocating proportional memory.
- No recursion is used for untrusted input except the JSON parser, which has a
  depth limit of 64.

## Security posture

- The server binds only `127.0.0.1`.
- A random 128-bit token is generated per launch; every `/api` request must
  present it in `X-Anvil-Token` (or the `token` query parameter for SSE).
- The API has no filesystem access and no shell execution.
- The page uses a strict Content Security Policy. The only inline-style
  permission is for dynamic canvas/UI sizing; scripts are same-origin only.
- Static assets contain no network references.

## Determinism

- RNG is a documented xorshift64* seeded explicitly.
- Netlist extraction sorts all outputs.
- DRC sorts violations by severity, kind, layer, coordinate and message.
- Router net ordering uses deterministic metrics; random jitter is only used for
  retry order and is seeded.
- Gerber and drill writers emit stable output with no timestamps.
