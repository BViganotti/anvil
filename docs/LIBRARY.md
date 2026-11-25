# Component library

The library is compiled into `anvil-core`. Symbols use nanometre offsets relative
to the component anchor; footprints use nanometre offsets relative to the
footprint origin.

## Parts

| Part | Symbol | Footprint | Pins |
|---|---|---|---|
| `R` | resistor | `R_0805` | 1, 2 |
| `C` | capacitor | `C_0805` | 1, 2 |
| `L` | inductor | `L_0805` | 1, 2 |
| `D` | diode | `D_SOD123` | A, K |
| `LED` | LED | `D_SOD123` | A, K |
| `V` | voltage source | `PINHDR_2` | +, − |
| `I` | current source | `PINHDR_2` | +, − |
| `SW` | ideal switch | `SOT23_SW` | 1, 2 |
| `GND` | ground | `TP_1MM` | 1 (annotation) |
| `VCC` | supply | `TP_1MM` | 1 (annotation) |
| `AND2` | AND | `SOT23_3` | A, B, Y |
| `OR2` | OR | `SOT23_3` | A, B, Y |
| `XOR2` | XOR | `SOT23_3` | A, B, Y |
| `NAND2` | NAND | `SOT23_3` | A, B, Y |
| `NOR2` | NOR | `SOT23_3` | A, B, Y |
| `NOT` | inverter | `SOT23_3` | A, Y |
| `DFF` | D flip-flop | `SOIC14` | D, CLK, Q, QN |
| `CONN2` | 2-pin header | `PINHDR_2` | 1, 2 |
| `CONN4` | 4-pin header | `PINHDR_4` | 1–4 |
| `CONN8` | 8-pin header | `PINHDR_8` | 1–8 |

Additional footprints exist for the 1 mm test point (`TP_1MM`), SOT-23 switch,
SOIC-14 and pin headers.

## Symbol graphics

`Graphic` variants are line, rectangle, circle, polyline and text, all in
nanometres. The Studio receives them from `GET /api/library` and renders the
same geometry on the canvas. No external SVG or font is used.

## Footprints

Pads carry number, local offset, width/height, shape (circle/rect/oval/square),
drill and layer (top/bottom/through). Through-hole pads get `drill > 0`; SMD pads
use `drill = 0`. Courtyard size is used by board generation and silkscreen output.

## Adding a part

1. Add a `Symbol` and `Footprint` in `Library::standard` (or a new builder).
2. Add a `Part` referencing their names, a default value and category.
3. If it is simulative, teach `sim::compile` its symbol and pin mapping.
4. Add a DRC/router test for its footprint if it has unusual pad geometry.
5. Regenerate examples and run the library contract test.

Unsupported symbols compile with an error naming the refdes; they are never
silently ignored. Connectors and power symbols are connective only.
