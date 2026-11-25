# Gerber, Excellon, raster and PNG

Anvil writes the common RS-274X subset, parses it back, rasterizes it and encodes
PNG without external libraries. The preview and round-trip tests use the same
parser; a writer bug cannot hide behind a separate renderer.

## Gerber output

Files per board:

- `*-F_Cu.gbr` top copper
- `*-B_Cu.gbr` bottom copper
- `*-In1_Cu.gbr`, `*-In2_Cu.gbr` for four-layer boards
- `*-Edge.gbr` outline
- `*-Silk.gbr` footprint courtyards

Header:

```
G04 Anvil <name>*
%FSLAX46Y46*%
%MOMM*%
%LPD*%
G01*
```

Coordinates are millimetres in 4.6 format: one unit is 1 nm. Apertures are
`C` (circle), `R` (rectangle) and `O` (oval), emitted as `%ADD<code>...*%`.
Traces use circular apertures at their width; pads and vias flash their real
apertures. The file ends with `M02*`.

Output is deterministic: aperture codes are assigned by first use in a
`BTreeMap`, no timestamps are emitted.

## Gerber parser

Accepted:

- `FS`, `MOMM`, `MOIN`, `LP` (dark only), `AD` with C/R/O
- comments (`G04`), `G01`, `G70/G71`, `G90/G91`, `G75`
- aperture selection, `D01` draw, `D02` move, `D03` flash
- omitted X/Y (retains the previous coordinate)
- metric and inch units with 1–6 fractional digits

Rejected with an explicit error rather than guessed:

- arcs (`G02/G03`), regions (`G36/G37`), aperture macros (`AM`), step-and-repeat
  (`SR`), clear polarity (`LP C`), unknown extended commands, unknown G codes
- non-ASCII, oversized input, too many operations/apertures, coordinates out of
  the ±1e12 nm bound

A header-only layer is accepted as a valid empty layer.

## Excellon drill

The writer emits `M48`, `METRIC,TZ`, `FMAT,2`, tool definitions `TnCd`, `%`,
`G90`, `G05`, tool selections and decimal `X..Y..` hits, ending in `M30`.
Coordinates have four decimal places (0.1 µm resolution). Pads with a drill and
vias become hits; SMD-only boards produce a file with no tools.

The parser accepts metric/imperial decimal coordinates, tool tables and comments.
Fixed-format integer coordinates without a decimal point are rejected because
the implied format is ambiguous; Anvil does not guess.

## Rasterizer

`raster::render` transforms Gerber primitives to an 8-bit grayscale image:

- draws use capsule coverage
- flashes render circle/rect/oval apertures
- maximum image dimension is 3000 px and large documents are scaled down
- y is flipped for screen coordinates
- `render_with_holes` punches real drill holes in white

`fit_to_width` chooses a scale for a target width. The Studio preview adds the
board outline to the copper document before rasterizing, so the preview shows
the physical board rather than only copper extents.

## PNG

The encoder writes an 8-bit grayscale PNG with uncompressed DEFLATE stored
blocks, correct zlib Adler-32 and PNG CRC-32. This keeps the binary dependency
free with modest file sizes (hundreds of KiB for a board preview).

## Tests

- Writer → parser round trips for copper, outline and silk; flash coordinates
  match pad coordinates exactly.
- Truncated files and malformed commands never panic.
- Every generated, routed board exports and re-parses all Gerber/drill files.
- Top-layer Gerber is rasterized twice with identical pixels.
- PNG headers, stored blocks, Adler-32 and known CRC values are tested.

## Limits

- No arcs, regions, macros, step-and-repeat, clear polarity, polygons, text
  strokes or solder mask/paste layers.
- Drill files use decimal coordinates; fixed-format Excellon is rejected.
- Coordinates are 1 nm in Gerber (4.6) and rounded to 0.1 µm in drill.
- The rasterizer is a verification tool, not a manufacturing renderer.
