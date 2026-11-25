# Autorouter

The router is deterministic, grid based, two-layer, and gated by the same DRC
engine used by the board panel. It never reports a clean route unless DRC
actually returns zero errors.

## Inputs

- Board outline (polygon), two copper layers (top and bottom).
- Pads with nets and SMD/through-hole layer sets.
- Design rules: minimum trace, clearance, via drill/diameter, edge clearance.
- `RouterOptions`: grid, width, via cost, search limit, rip-up budget, retries,
  seed.

Defaults choose a 0.6 mm grid and 0.2 mm track width for the default 0.2 mm
clearance. The grid must be larger than track width + clearance so adjacent grid
cells remain legal; `RouterOptions::for_board` raises the grid if needed.

## Grid and obstacles

A workspace covers the outline inset by edge clearance, via radius and one grid
step. Cell centers must be inside the polygon. Cells are indexed by layer and
`(x, y)`.

For each net, three obstacle sets are built from every other net's geometry:

- Trace mask: pad/trace/via inflated by `clearance + half track width`.
- Via mask: inflated by `clearance + via radius`.
- Board edge: outside the workspace.

Masks are computed per net from the actual board, so a newly routed trace blocks
later nets without any global mutable grid state. Unassigned copper (mechanical
pads) also blocks routing. The net's own copper is traversable because same-net
clearance is not required.

Moves are four-directional. This is deliberate: orthogonal segments between
adjacent cell centers cannot cut a blocked corner, which keeps A* paths
consistent with the clearance masks. Vias connect the two layers at a cell when
both via masks allow it.

## A*

State is `(layer, ix, iy)`. Edge cost is 10 per cell, via cost 300 (configurable),
heuristic is Manhattan distance plus a layer penalty. A binary heap with a
`g`/predecessor array is used; expansion is capped (default 400k). There is no
randomness inside A*, so a given occupancy state produces a stable path.

Pads are anchored at the nearest free cell within a small search box. The trace
includes the exact pad centre, so connectivity uses the real pad geometry.

## Net ordering and rip-up

Pads of a net are connected by a deterministic Euclidean MST. Nets are initially
ordered by descending MST length (harder nets first). Each net is routed edge by
edge. On failure:

1. That net's partial traces/vias are removed.
2. Up to `1 + ripups % 3` of the most recently completed nets are ripped up.
3. The ripped nets and the failed net are retried at the front of the queue.
4. Each net has a retry cap; the board can be retried with a new seeded order.

Every attempt ends with a full DRC and connectivity check. The best attempt
(fewest failures, then fewest DRC errors, then fewest unrouted edges) is kept.
Existing routes are cleared before routing; this is a full-board autorouter, not
an incremental one.

## Results and limits

Acceptance covers 20 generated boards; every board routes all nets with zero DRC
errors in the test suite. Measured on the development machine (release build),
ten-net generated boards route in roughly 2.8–4.3 ms each.

Limits:

- Only top and bottom layers are routed. Four-layer boards still use two routing
  layers; inner layers are available for manual routing and export.
- Grid-aligned tracks only; no 45-degree geometry, arcs or teardrops.
- No differential pairs, length matching, impedance rules or keepouts.
- Dense or adversarial boards can fail. Failures are reported with the net and
  reason, and DRC remains authoritative.
- No cancellation token; a running job finishes or hits its search/rip-up budget.
