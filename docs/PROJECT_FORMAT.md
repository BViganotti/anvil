# Project format (`anvil-project-1`)

Anvil project files are JSON. They contain the schematic; the PCB is regenerated
from the netlist when needed. Units are nanometres. Files are deterministic and
contain no timestamps or machine-specific data.

## Top level

```json
{
  "format": "anvil-project-1",
  "name": "Anvil demo",
  "sheet": { "title": "...", "company": "...", "revision": "A", "date": "" },
  "components": [ ... ],
  "wires": [ ... ],
  "junctions": [ ... ],
  "labels": [ ... ],
  "buses": [ ... ]
}
```

Unknown fields are ignored by the loader. `format` must match when present.
Missing optional arrays are treated as empty. Limits match the schematic editor.

## Component

```json
{
  "id": 1,
  "refdes": "R1",
  "part": "R",
  "value": "1k",
  "footprint": "R_0805",
  "at": [30480000, 38100000],
  "rotation": 0,
  "mirror": false,
  "properties": { "note": "optional" }
}
```

- `part` is a library part name; pins are reconstructed from its symbol.
- `at` is `[x, y]` in nanometres.
- `rotation` is a multiple of 90 degrees clockwise.
- `id`, `footprint` and `pins` are informational on load; ids are reassigned.
- Duplicate refdes and unknown parts are errors.

## Wire and junction

```json
{ "id": 3, "points": [[0,0],[2540000,0]] }
{ "id": 1, "at": [0,0] }
```

Wires are polylines with 2–2000 points. Consecutive duplicate points are removed.
A T connection requires a junction; crossings without one do not connect.

## Label and bus

```json
{ "id": 1, "name": "MID", "kind": "local", "at": [0,0], "rotation": 0 }
{ "id": 2, "name": "DATA", "members": ["D0","D1"], "points": [] }
```

`kind` is `local`, `global` or `power`. Labels with the same resolved name
connect by name. A label `DATA[1]` resolves to the member at index 1 of bus
`DATA`; a missing bus or out-of-range index is a diagnostic.

## Relationship to API JSON

The Studio `GET /api/state` response is not the project format: it adds absolute
pin positions, netlists, diagnostics, board summaries and route state for
rendering. Only the fields above are accepted by `Project::from_json`.

## Regenerating examples

`anvil generate-examples` writes every shipped example from the builders in
`crates/anvil-studio/src/project.rs`. `tests/examples_load.rs` compares the files
to the builders, so stale examples fail the test suite.
