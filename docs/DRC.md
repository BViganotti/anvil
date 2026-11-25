# Design rule checking

DRC is the acceptance gate for layout and autorouting. Every result is computed
from integer board geometry; the router cannot declare success without it.

## Checks

| Check | Kind | What is measured |
|---|---|---|
| Structure | `structure` | outline area, layer count, rules, duplicate refdes, pad/trace/via bounds |
| Track width | `trace_width` | trace width vs `min_trace` |
| Drill | `drill` | through-hole pad drill vs `min_drill` |
| Annular ring | `annular_ring` | `(min(pad width,height) - drill) / 2` vs `min_annular` |
| Via drill | `via_drill` | via drill vs `via_drill` |
| Via annular | `via_annular` | `(diameter - drill) / 2` vs `min_annular` |
| Clearance | `clearance` | different-net copper edge distance vs `min_clearance` |
| Short | `short` | different-net copper that touches or overlaps |
| Board edge | `board_edge` | copper edge to outline distance vs `edge_clearance` |
| Outside board | `outside_board` | pads/traces/vias outside the outline |
| Unrouted | `unrouted` | ratsnest edges remaining after copper connectivity |

Copper includes SMD pads on their layer, through-hole pads on every layer, traces
on their layer, and vias across their span. Same-net copper is not reported for
clearance; unassigned copper still participates in clearance checks.

## Geometry

- Pad shapes: circle, rectangle/square; oval is conservatively treated as its
  bounding rectangle.
- Trace clearance uses capsule distance: segment-to-segment distance compared
  with `(w1 + w2)/2 + clearance`.
- Pad-to-trace and pad-to-pad use shape-specific distance predicates.
- All comparisons square the threshold and use `i128`, never floating point.
- Board-edge checks use point/segment distance to the outline polygon plus
  conservative pad extents.

## Spatial indexing

Clearance checks use a per-layer uniform grid keyed by cell. Items are inserted
into every cell their expanded bounding box touches; each item queries its own
expanded box. Pair results are deduplicated by stable item ids so a through-hole
pad is reported once, not once per layer. Items too large for the cell budget
fall into a sentinel bucket scanned by all queries, which keeps memory bounded.

## Connectivity and ratsnest

Pads, traces and vias form a union-find graph:

- pads touch when their shapes overlap on a shared layer
- pads touch traces that pass through them
- traces touch when their capsules overlap on the same layer
- vias connect traces on all layers they span
- vias connect pads and other vias

For each net, connected pad groups are reduced to a minimum spanning tree using
Euclidean distance; each remaining tree edge is an `unrouted` violation and a
ratsnest line in the UI. A fully routed board has no ratsnest edges.

## Report

Violations carry severity, kind, optional layer, coordinate, nets, reference
labels and a human-readable message. Reports are sorted by severity (errors
first), kind, layer, coordinate and message so two runs are byte-identical.
The API truncates the JSON list at 500 entries and sets `truncated` accordingly.

## Acceptance

- `twenty_generated_boards_route_with_zero_drc` routes 20 synthetic boards and
  re-runs DRC after routing, asserting zero errors and full connectivity.
- `routes_survive_export_round_trip` routes eight boards and re-parses every
  exported Gerber/drill file.
- A dedicated clearance test places two traces 0.3 mm apart with 0.2 mm width
  and 0.2 mm clearance and requires exactly that violation.
- Routed demo and generated boards are checked again in the Studio HTTP test.

## Limits

- No copper pours/zones, keepouts, solder mask, paste, impedance or thermal
  checks.
- Oval pads are conservatively rectangular; arcs and rounded rectangles are not
  modelled.
- Board edge checks for large pads use a conservative extent rather than exact
  corner distance.
- DRC does not prove manufacturability beyond these checks.
