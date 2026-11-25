# Limitations

Anvil is honest about what it does not do. The following gaps are real; none of
them is hidden behind a fake result.

## Simulation

- Device subset only: R, C, L, independent V/I sources, Shockley diodes, static
  ideal switches, and the built-in logic gates/DFF. No MOSFETs, BJTs, op-amps,
  transformers, transmission lines, crystals, thermals or noise.
- No AC sweep, pole-zero, harmonic balance, Monte Carlo or worst-case analysis.
- Diode reverse voltage is clamped at −20 V in the junction update; currents far
  below the model's useful range are not meaningful.
- Switch state is static (a property), not controlled by a node voltage.
- Digital delays shorter than the transient maximum step are quantised by that
  step. The event queue clamps to scheduled events, but sub-step delays cannot
  create sub-step transitions.
- One flat sheet; no hierarchical schematics.
- Initial conditions: capacitor `ic` and inductor `ic` are supported, but the
  recorded t=0 point still comes from the DC operating point when a source is
  active.
- Long runs are decimated after 20k samples per series (64 nodes max recorded).

## Schematic and netlist

- Buses are naming bundles. `BUS[3]` resolves to the declared member, but there
  is no bus wire connectivity or bus ripping.
- Labels are global within the single sheet; local vs global scope is recorded
  but does not create multiple sheets.
- Wire-to-wire interior connections require a junction or label/pin at the
  point. A pin anywhere on a wire connects.
- No ERC for drive strength, input loading, power sequencing or analog ranges.
- The editor has no undo/redo, clipboard, hierarchical sheets, custom symbols,
  custom footprints or file open/save in the browser.

## PCB and DRC

- Footprints are placed on the top side; no bottom-side placement UI.
- 2 or 4 copper layers are modelled, but the autorouter uses top and bottom only.
- Pad shapes: circle and rectangle are exact; oval is conservatively treated as
  its bounding rectangle. No rounded rectangles, polygons, slots or custom pads.
- No copper pours/zones, keepouts, thermal relief, solder mask, paste, silkscreen
  text strokes, impedance or differential-pair rules.
- Board-edge checks for pads use conservative extents; they can report a
  violation that an exact corner calculation would allow.
- DRC is not a manufacturing signoff.

## Autorouter

- Grid-aligned orthogonal tracks only; no 45° routing, arcs or teardrops.
- Two routing layers; no blind/buried vias, no via-in-pad rules.
- No length matching, differential pairs, shielding, keepouts or current
  capacity.
- Dense or adversarial boards may remain unrouted. The router reports failures
  and DRC errors; it does not fake completion.
- Full-board reroute only: starting autoroute clears manual traces.
- No cancel button; a job runs to completion or its budget.
- Deterministic for a given seed, board and options, but a retry with a different
  seed can produce different (also valid) routes.

## Board generation

- Shelf placement with fixed margins; no component rotation search or
  mechanical constraints.
- Schematic power symbols are skipped (they are annotations); sources use a
  2-pin header footprint.
- Through-hole pads exist in the library, but synthetic acceptance boards are
  SMD-only.
- No board outline editing UI beyond the generated rectangle.

## Export

- Gerber subset: no arcs, regions, macros, step-and-repeat, clear polarity,
  polygon fills or text. Unsupported input is rejected.
- Excellon uses decimal metric coordinates; fixed-format integer coordinates are
  rejected as ambiguous.
- Gerber coordinates are 1 nm; drill is rounded to 0.1 µm.
- No solder mask, paste, assembly or pick-and-place files.
- The rasterizer is for verification, not photorealistic or manufacture-ready
  rendering.

## Studio and runtime

- One in-memory project. There is no open/save dialog, autosave, undo or
  multi-document support.
- No user accounts, TLS, remote binding or multi-user collaboration.
- The UI is desktop-oriented; small-screen layout is functional but not designed.
- The binary always runs a server; there is no headless library-only mode in the
  launcher, though all crates are usable headlessly.
- The launcher prints its URL and token to stdout once; it has no interactive
  terminal UI.

## Tooling

- No CI/CD by requirement. Tests, fuzzing, benchmarks and Playwright are run
  manually.
- No external crates means no serde, clap, rand, tiny-skia, png or test runner
  helpers; these are implemented in-tree and tested.
- Line coverage is measured locally with `cargo llvm-cov`; there is no coverage
  gate.
