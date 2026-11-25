//! Copper connectivity and ratsnest extraction.
//!
//! Connectivity is the same integer geometry used by DRC: same-net copper that
//! touches or overlaps on a shared layer is connected. The resulting pad groups
//! drive the ratsnest, the DRC unrouted check, and router verification.

use crate::board::{Board, Layer, Pad, Via};
use crate::geom::{
    pad_pad_within, pad_trace_within, via_pad_within, via_trace_within, via_via_within,
};
use anvil_core::geom::Point;
use anvil_core::library::PadLayer;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RatsEdge {
    pub net: usize,
    pub net_name: String,
    pub a: Point,
    pub b: Point,
    pub a_label: String,
    pub b_label: String,
}

impl RatsEdge {
    pub fn length(&self) -> f64 {
        self.a.dist(self.b)
    }
}

struct Dsu {
    parent: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Dsu {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.parent[r] != r {
            r = self.parent[r];
        }
        let mut c = x;
        while self.parent[c] != c {
            let next = self.parent[c];
            self.parent[c] = r;
            c = next;
        }
        r
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            // Deterministic: smaller index is the root.
            if ra < rb {
                self.parent[rb] = ra;
            } else {
                self.parent[ra] = rb;
            }
        }
    }
}

fn pads_share_layer(a: &Pad, b: &Pad) -> bool {
    match (a.layers, b.layers) {
        (PadLayer::Through, _) | (_, PadLayer::Through) => true,
        (PadLayer::Top, PadLayer::Top) => true,
        (PadLayer::Bottom, PadLayer::Bottom) => true,
        _ => false,
    }
}

fn pad_has_layer(pad: &Pad, layer: Layer) -> bool {
    match pad.layers {
        PadLayer::Through => true,
        PadLayer::Top => layer == Layer::Top,
        PadLayer::Bottom => layer == Layer::Bottom,
    }
}

fn pad_via_share(pad: &Pad, via: &Via) -> bool {
    match pad.layers {
        PadLayer::Through => true,
        PadLayer::Top => via.spans(Layer::Top),
        PadLayer::Bottom => via.spans(Layer::Bottom),
    }
}

/// Connectivity over pads, traces and vias. Returns a union-find parent vector
/// plus index maps, exposed through helper functions.
pub struct Connectivity {
    dsu: Dsu,
    pad_count: usize,
    trace_offset: usize,
    via_offset: usize,
    total: usize,
}

impl Connectivity {
    pub fn build(board: &Board) -> Connectivity {
        let pad_count = board.pad_count();
        let trace_offset = pad_count;
        let via_offset = trace_offset + board.traces.len();
        let total = via_offset + board.vias.len();
        let mut dsu = Dsu::new(total.max(1));
        // Per-net item lists.
        let mut pads_by_net: Vec<Vec<(usize, usize)>> = vec![Vec::new(); board.nets.len()];
        for (fi, f) in board.footprints.iter().enumerate() {
            for (pi, p) in f.pads.iter().enumerate() {
                if let Some(net) = p.net {
                    if net < pads_by_net.len() {
                        pads_by_net[net].push((fi, pi));
                    }
                }
            }
        }
        let mut traces_by_net: Vec<Vec<usize>> = vec![Vec::new(); board.nets.len()];
        for (ti, t) in board.traces.iter().enumerate() {
            if t.net < traces_by_net.len() {
                traces_by_net[t.net].push(ti);
            }
        }
        let mut vias_by_net: Vec<Vec<usize>> = vec![Vec::new(); board.nets.len()];
        for (vi, v) in board.vias.iter().enumerate() {
            if v.net < vias_by_net.len() {
                vias_by_net[v.net].push(vi);
            }
        }

        let pad_global = |fi: usize, pi: usize| -> usize {
            let mut base = 0;
            for f in board.footprints.iter().take(fi) {
                base += f.pads.len();
            }
            base + pi
        };

        for net in 0..board.nets.len() {
            let pads = &pads_by_net[net];
            let traces = &traces_by_net[net];
            let vias = &vias_by_net[net];

            // Pad to pad.
            for i in 0..pads.len() {
                for j in (i + 1)..pads.len() {
                    let a = &board.footprints[pads[i].0].pads[pads[i].1];
                    let b = &board.footprints[pads[j].0].pads[pads[j].1];
                    if pads_share_layer(a, b) && pad_pad_within(a, b, 0) {
                        dsu.union(pad_global(pads[i].0, pads[i].1), pad_global(pads[j].0, pads[j].1));
                    }
                }
            }
            // Pad to trace.
            for &(fi, pi) in pads {
                let a = &board.footprints[fi].pads[pi];
                let ga = pad_global(fi, pi);
                for &ti in traces {
                    let t = &board.traces[ti];
                    if !pad_has_layer(a, t.layer) {
                        continue;
                    }
                    for (s, e) in t.segments() {
                        if pad_trace_within(a, s, e, t.width / 2, 0) {
                            dsu.union(ga, trace_offset + ti);
                            break;
                        }
                    }
                }
            }
            // Pad to via.
            for &(fi, pi) in pads {
                let a = &board.footprints[fi].pads[pi];
                let ga = pad_global(fi, pi);
                for &vi in vias {
                    let v = &board.vias[vi];
                    if pad_via_share(a, v) && via_pad_within(v, a, 0) {
                        dsu.union(ga, via_offset + vi);
                    }
                }
            }
            // Trace to trace (same layer).
            for i in 0..traces.len() {
                for j in (i + 1)..traces.len() {
                    let a = &board.traces[traces[i]];
                    let b = &board.traces[traces[j]];
                    if a.layer != b.layer {
                        continue;
                    }
                    'pair: for (a1, a2) in a.segments() {
                        for (b1, b2) in b.segments() {
                            if crate::geom::trace_trace_within(a1, a2, a.width, b1, b2, b.width, 0) {
                                dsu.union(trace_offset + traces[i], trace_offset + traces[j]);
                                break 'pair;
                            }
                        }
                    }
                }
            }
            // Trace to via.
            for &ti in traces {
                let t = &board.traces[ti];
                for &vi in vias {
                    let v = &board.vias[vi];
                    if !v.spans(t.layer) {
                        continue;
                    }
                    for (s, e) in t.segments() {
                        if via_trace_within(v, s, e, t.width / 2, 0) {
                            dsu.union(trace_offset + ti, via_offset + vi);
                            break;
                        }
                    }
                }
            }
            // Via to via.
            for i in 0..vias.len() {
                for j in (i + 1)..vias.len() {
                    let a = &board.vias[vias[i]];
                    let b = &board.vias[vias[j]];
                    let overlap = a.from.index().max(b.from.index()) <= a.to.index().min(b.to.index());
                    if overlap && via_via_within(a, b, 0) {
                        dsu.union(via_offset + vias[i], via_offset + vias[j]);
                    }
                }
            }
        }

        Connectivity {
            dsu,
            pad_count,
            trace_offset,
            via_offset,
            total,
        }
    }

    pub fn total_items(&self) -> usize {
        self.total
    }

    pub fn pad_count(&self) -> usize {
        self.pad_count
    }

    pub fn trace_offset(&self) -> usize {
        self.trace_offset
    }

    pub fn via_offset(&self) -> usize {
        self.via_offset
    }

    pub fn root(&mut self, index: usize) -> usize {
        if index >= self.total {
            return index;
        }
        self.dsu.find(index)
    }
}

/// Pad groups for a net: each inner vector contains global pad indices that are
/// electrically connected by copper. Pads not touched by copper are singleton
/// groups. Only pads belonging to `net` are included.
pub fn pad_groups(board: &Board, net: usize, connectivity: &mut Connectivity) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut index = 0usize;
    for f in &board.footprints {
        for p in &f.pads {
            if p.net == Some(net) {
                let g = connectivity.root(index);
                let mut found = false;
                for grp in groups.iter_mut() {
                    if connectivity.root(grp[0]) == g {
                        grp.push(index);
                        found = true;
                        break;
                    }
                }
                if !found {
                    groups.push(vec![index]);
                }
            }
            index += 1;
        }
    }
    groups
}

fn pad_at_global(board: &Board, mut index: usize) -> Option<&Pad> {
    for f in &board.footprints {
        if index < f.pads.len() {
            return f.pads.get(index);
        }
        index -= f.pads.len();
    }
    None
}

fn pad_label(board: &Board, mut index: usize) -> String {
    for f in &board.footprints {
        if index < f.pads.len() {
            return f.pads[index].label(&f.refdes);
        }
        index -= f.pads.len();
    }
    "?".to_string()
}

/// Compute the minimum spanning ratsnest for every net. Deterministic ordering.
pub fn ratsnest(board: &Board) -> Vec<RatsEdge> {
    if board.nets.is_empty() {
        return Vec::new();
    }
    let mut conn = Connectivity::build(board);
    let mut edges = Vec::new();
    for net in 0..board.nets.len() {
        let groups = pad_groups(board, net, &mut conn);
        if groups.len() < 2 {
            continue;
        }
        let mut representatives: Vec<usize> = groups.iter().map(|g| g[0]).collect();
        representatives.sort_unstable();
        let mut pairs: Vec<(i128, usize, usize)> = Vec::new();
        for i in 0..representatives.len() {
            for j in (i + 1)..representatives.len() {
                let pa = pad_at_global(board, representatives[i]);
                let pb = pad_at_global(board, representatives[j]);
                if let (Some(pa), Some(pb)) = (pa, pb) {
                    pairs.push((pa.at.dist2(pb.at), representatives[i], representatives[j]));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut local: Vec<usize> = (0..groups.len()).collect();
        let group_of = |local: &Vec<usize>, mut idx: usize| -> usize {
            while local[idx] != idx {
                idx = local[idx];
            }
            idx
        };
        let index_of = |reps: &Vec<usize>, global: usize| -> usize {
            reps.iter().position(|&r| r == global).unwrap_or(0)
        };
        let mut used = 0usize;
        for (_, ga, gb) in pairs {
            let ia = index_of(&representatives, ga);
            let ib = index_of(&representatives, gb);
            let ra = group_of(&local, ia);
            let rb = group_of(&local, ib);
            if ra == rb {
                continue;
            }
            local[rb] = ra;
            used += 1;
            if let (Some(pa), Some(pb)) = (pad_at_global(board, ga), pad_at_global(board, gb)) {
                edges.push(RatsEdge {
                    net,
                    net_name: board.net_name(net).to_string(),
                    a: pa.at,
                    b: pb.at,
                    a_label: pad_label(board, ga),
                    b_label: pad_label(board, gb),
                });
            }
            if used + 1 == groups.len() {
                break;
            }
        }
    }
    edges.sort_by(|a, b| {
        a.net
            .cmp(&b.net)
            .then(a.a.x.cmp(&b.a.x))
            .then(a.a.y.cmp(&b.a.y))
            .then(a.b.x.cmp(&b.b.x))
            .then(a.b.y.cmp(&b.b.y))
    });
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Board, DesignRules, Layer, MM};
    use anvil_core::library::Library;

    #[test]
    fn ratsnest_connects_two_pads() {
        let lib = Library::standard();
        let mut b = Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default());
        let n = b.add_net("N1");
        let fp = lib.footprint("R_0805").unwrap();
        b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n), None])
            .unwrap();
        b.add_footprint(fp, "R2", Point::new(20 * MM, 10 * MM), 0, &[Some(n), None])
            .unwrap();
        let r = ratsnest(&b);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].net_name, "N1");
    }

    #[test]
    fn routed_trace_removes_ratsnest() {
        let lib = Library::standard();
        let mut b = Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default());
        let n = b.add_net("N1");
        let fp = lib.footprint("R_0805").unwrap();
        b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n), None])
            .unwrap();
        b.add_footprint(fp, "R2", Point::new(20 * MM, 10 * MM), 0, &[Some(n), None])
            .unwrap();
        let p1 = b.footprints[0].pads[0].at;
        let p2 = b.footprints[1].pads[0].at;
        b.add_trace(n, Layer::Top, 200_000, vec![p1, p2]).unwrap();
        assert!(ratsnest(&b).is_empty());
    }
}
