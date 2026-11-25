//! Deterministic two-layer grid autorouter with A*, vias, net ordering and
//! rip-up/retry.
//!
//! The router never claims success: it returns real DRC results from the same
//! checks used by the DRC panel. A net is only marked routed when traces and
//! vias were actually added; rip-up removes them again.

use crate::board::{Board, Layer};
use crate::connect::Connectivity;
use crate::drc::{self, DrcReport};
use crate::geom::pad_bbox;
use anvil_core::geom::{point_in_polygon, point_segment_dist2, Point};
use anvil_core::library::PadLayer;
use anvil_core::util::Rng;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

#[derive(Clone, Debug)]
pub struct RouterOptions {
    pub grid: i64,
    pub width: i64,
    pub via_cost: u32,
    pub max_ripups: usize,
    pub max_expansions: usize,
    pub seed: u64,
    pub retries: usize,
}

impl Default for RouterOptions {
    fn default() -> Self {
        RouterOptions {
            grid: 600_000,
            width: 200_000,
            via_cost: 300,
            max_ripups: 24,
            max_expansions: 400_000,
            seed: 1,
            retries: 3,
        }
    }
}

impl RouterOptions {
    pub fn for_board(board: &Board) -> Self {
        let mut o = RouterOptions::default();
        o.width = board.rules.min_trace.max(200_000);
        let min_grid = o.width + board.rules.min_clearance + 100_000;
        o.grid = o.grid.max(min_grid);
        o
    }

    pub fn validate(&self) -> Result<(), RouteError> {
        if self.grid <= 0 || self.width <= 0 {
            return Err(RouteError::InvalidOptions("grid and width must be positive"));
        }
        if self.width < 50_000 {
            return Err(RouteError::InvalidOptions("track width too small"));
        }
        if self.max_expansions < 100 || self.max_expansions > 5_000_000 {
            return Err(RouteError::InvalidOptions("search limit"));
        }
        if self.retries == 0 || self.retries > 16 {
            return Err(RouteError::InvalidOptions("retries"));
        }
        if self.max_ripups > 10_000 {
            return Err(RouteError::InvalidOptions("ripups"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteError {
    InvalidOptions(&'static str),
    Board(String),
    Engine(String),
}

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouteError::InvalidOptions(s) => write!(f, "invalid router options: {s}"),
            RouteError::Board(s) => write!(f, "router board error: {s}"),
            RouteError::Engine(s) => write!(f, "router engine error: {s}"),
        }
    }
}

impl std::error::Error for RouteError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteFailure {
    pub net: usize,
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct RouteProgress {
    pub attempt: usize,
    pub phase: &'static str,
    pub net: Option<usize>,
    pub net_name: String,
    pub routed: usize,
    pub total: usize,
}

#[derive(Clone, Debug)]
pub struct RouteReport {
    pub nets_total: usize,
    pub nets_routed: usize,
    pub edges_total: usize,
    pub edges_routed: usize,
    pub failures: Vec<RouteFailure>,
    pub drc: DrcReport,
    pub clean: bool,
    pub attempts: usize,
    pub ripups: usize,
}

struct Workspace {
    min: Point,
    cols: i32,
    rows: i32,
    grid: i64,
    plane: usize,
    inside: Vec<bool>,
}

impl Workspace {
    fn new(board: &Board, opts: &RouterOptions) -> Result<Workspace, RouteError> {
        if board.outline.len() < 3 {
            return Err(RouteError::Board("outline has fewer than three points".into()));
        }
        let bbox = anvil_core::geom::Rect::from_points(&board.outline)
            .ok_or_else(|| RouteError::Board("empty outline".into()))?;
        let margin = board.rules.edge_clearance
            + board.rules.via_diameter / 2
            + opts.grid;
        let min = Point::new(bbox.min.x + margin, bbox.min.y + margin);
        let max = Point::new(bbox.max.x - margin, bbox.max.y - margin);
        if max.x <= min.x || max.y <= min.y {
            return Err(RouteError::Board("board too small for routing margin".into()));
        }
        let cols = ((max.x - min.x) / opts.grid + 1).max(1);
        let rows = ((max.y - min.y) / opts.grid + 1).max(1);
        let cells = (cols as i64).saturating_mul(rows as i64);
        if cells > 400_000 {
            return Err(RouteError::Board("routing grid exceeds the cell limit".into()));
        }
        let cols = cols as i32;
        let rows = rows as i32;
        let plane = (cols as usize) * (rows as usize);
        let mut inside = vec![false; plane];
        for iy in 0..rows {
            for ix in 0..cols {
                let p = Point::new(min.x + ix as i64 * opts.grid, min.y + iy as i64 * opts.grid);
                inside[(iy as usize) * (cols as usize) + ix as usize] =
                    point_in_polygon(p, &board.outline);
            }
        }
        Ok(Workspace {
            min,
            cols,
            rows,
            grid: opts.grid,
            plane,
            inside,
        })
    }

    fn cell_of(&self, p: Point) -> (i32, i32) {
        let ix = (p.x - self.min.x).div_euclid(self.grid).clamp(0, self.cols as i64 - 1);
        let iy = (p.y - self.min.y).div_euclid(self.grid).clamp(0, self.rows as i64 - 1);
        (ix as i32, iy as i32)
    }

    fn center(&self, ix: i32, iy: i32) -> Point {
        Point::new(
            self.min.x + ix as i64 * self.grid,
            self.min.y + iy as i64 * self.grid,
        )
    }

    fn index(&self, layer: usize, ix: i32, iy: i32) -> usize {
        layer * self.plane + (iy as usize) * (self.cols as usize) + ix as usize
    }

    fn valid(&self, layer: usize, ix: i32, iy: i32) -> bool {
        layer < 2
            && ix >= 0
            && ix < self.cols
            && iy >= 0
            && iy < self.rows
            && self.inside[(iy as usize) * (self.cols as usize) + ix as usize]
    }

    fn range(&self, min: Point, max: Point) -> Option<(i32, i32, i32, i32)> {
        let ix0 = (min.x - self.min.x).div_euclid(self.grid);
        let iy0 = (min.y - self.min.y).div_euclid(self.grid);
        let ix1 = (max.x - self.min.x).div_euclid(self.grid) + 1;
        let iy1 = (max.y - self.min.y).div_euclid(self.grid) + 1;
        let ix0 = ix0.max(0) as i32;
        let iy0 = iy0.max(0) as i32;
        let ix1 = ix1.min(self.cols as i64 - 1) as i32;
        let iy1 = iy1.min(self.rows as i64 - 1) as i32;
        if ix1 < ix0 || iy1 < iy0 {
            None
        } else {
            Some((ix0, iy0, ix1, iy1))
        }
    }
}

struct Masks {
    trace: Vec<bool>,
    via: Vec<bool>,
}

impl Masks {
    fn new(ws: &Workspace) -> Self {
        Masks {
            trace: vec![false; ws.plane * 2],
            via: vec![false; ws.plane * 2],
        }
    }

    fn mark_cells<F: Fn(Point) -> bool>(
        &mut self,
        ws: &Workspace,
        layer: usize,
        min: Point,
        max: Point,
        which: u8,
        pred: F,
    ) {
        if let Some((ix0, iy0, ix1, iy1)) = ws.range(min, max) {
            for iy in iy0..=iy1 {
                for ix in ix0..=ix1 {
                    let p = ws.center(ix, iy);
                    if pred(p) {
                        let idx = ws.index(layer, ix, iy);
                        if which & 1 != 0 {
                            self.trace[idx] = true;
                        }
                        if which & 2 != 0 {
                            self.via[idx] = true;
                        }
                    }
                }
            }
        }
    }
}

fn mask_for_net(board: &Board, ws: &Workspace, net: usize, opts: &RouterOptions) -> Masks {
    let mut masks = Masks::new(ws);
    let via_r = board.rules.via_diameter / 2;
    let trace_half = opts.width / 2;
    let clearance = board.rules.min_clearance;

    for f in &board.footprints {
        for p in &f.pads {
            if p.net == Some(net) {
                continue;
            }
            // Unassigned copper still blocks routing (mechanical pad).
            let layers: Vec<usize> = match p.layers {
                PadLayer::Top => vec![0],
                PadLayer::Bottom => vec![1],
                PadLayer::Through => vec![0, 1],
            };
            let rt = clearance + trace_half;
            let rv = clearance + via_r;
            let r = rt.max(rv);
            let b = pad_bbox(p).expand(r);
            for layer in layers {
                masks.mark_cells(ws, layer, b.min, b.max, 3, |_| true);
            }
        }
    }
    for t in &board.traces {
        if Some(t.net) == Some(net) {
            continue;
        }
        let rt = t.width / 2 + clearance + trace_half;
        let rv = t.width / 2 + clearance + via_r;
        let layer = t.layer.index();
        for (a, b) in t.segments() {
            let min = Point::new(a.x.min(b.x) - rt.max(rv), a.y.min(b.y) - rt.max(rv));
            let max = Point::new(a.x.max(b.x) + rt.max(rv), a.y.max(b.y) + rt.max(rv));
            masks.mark_cells(ws, layer, min, max, 1, |p| {
                point_segment_dist2(p, a, b) <= (rt as i128).pow(2)
            });
            masks.mark_cells(ws, layer, min, max, 2, |p| {
                point_segment_dist2(p, a, b) <= (rv as i128).pow(2)
            });
        }
    }
    for v in &board.vias {
        if v.net == net {
            continue;
        }
        let rt = via_r + clearance + trace_half;
        let rv = via_r + clearance + via_r;
        let r = rt.max(rv);
        let min = Point::new(v.at.x - r, v.at.y - r);
        let max = Point::new(v.at.x + r, v.at.y + r);
        for layer in [0usize, 1] {
            if !v.spans(Layer::from_index(layer).unwrap_or(Layer::Top)) {
                continue;
            }
            masks.mark_cells(ws, layer, min, max, 1, |p| p.dist2(v.at) <= (rt as i128).pow(2));
            masks.mark_cells(ws, layer, min, max, 2, |p| p.dist2(v.at) <= (rv as i128).pow(2));
        }
    }
    masks
}

fn pad_layers(pad: &crate::board::Pad) -> Vec<usize> {
    match pad.layers {
        PadLayer::Top => vec![0],
        PadLayer::Bottom => vec![1],
        PadLayer::Through => vec![0, 1],
    }
}

fn pad_anchor(
    ws: &Workspace,
    masks: &Masks,
    pad: &crate::board::Pad,
    layer: usize,
) -> Option<(i32, i32)> {
    let (cx, cy) = ws.cell_of(pad.at);
    let hb = pad_bbox(pad).expand(ws.grid * 2);
    let mut best: Option<(i128, i32, i32)> = None;
    for dy in -6..=6i32 {
        for dx in -6..=6i32 {
            let ix = cx + dx;
            let iy = cy + dy;
            if !ws.valid(layer, ix, iy) {
                continue;
            }
            let p = ws.center(ix, iy);
            if !hb.contains(p) {
                continue;
            }
            if masks.trace[ws.index(layer, ix, iy)] {
                continue;
            }
            let d = p.dist2(pad.at);
            if best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                best = Some((d, ix, iy));
            }
        }
    }
    best.map(|(_, ix, iy)| (ix, iy))
}

fn heuristic(ws: &Workspace, layer: usize, ix: i32, iy: i32, gl: usize, gx: i32, gy: i32, via_cost: u32) -> u64 {
    let d = (ix - gx).unsigned_abs() as u64 + (iy - gy).unsigned_abs() as u64;
    let l = if layer == gl { 0 } else { via_cost as u64 };
    let _ = ws;
    d * 10 + l
}

fn astar(
    ws: &Workspace,
    masks: &Masks,
    start: (usize, i32, i32),
    goal: (usize, i32, i32),
    via_cost: u32,
    max_expansions: usize,
) -> Option<Vec<u32>> {
    let (sl, sx, sy) = start;
    let (gl, gx, gy) = goal;
    if !ws.valid(sl, sx, sy) || !ws.valid(gl, gx, gy) {
        return None;
    }
    let s_idx = ws.index(sl, sx, sy);
    let g_idx = ws.index(gl, gx, gy);
    if masks.trace[s_idx] || masks.trace[g_idx] {
        return None;
    }
    let total = ws.plane * 2;
    let mut g = vec![u64::MAX; total];
    let mut prev = vec![u32::MAX; total];
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::new();
    g[s_idx] = 0;
    heap.push(Reverse((heuristic(ws, sl, sx, sy, gl, gx, gy, via_cost), s_idx as u32)));
    let mut expansions = 0usize;
    while let Some(Reverse((f, idx))) = heap.pop() {
        expansions += 1;
        if expansions > max_expansions {
            return None;
        }
        let idx = idx as usize;
        let layer = idx / ws.plane;
        let rem = idx % ws.plane;
        let ix = (rem % ws.cols as usize) as i32;
        let iy = (rem / ws.cols as usize) as i32;
        let cur_g = g[idx];
        if idx == g_idx {
            let mut path = Vec::new();
            let mut cur = idx as u32;
            while cur != u32::MAX {
                path.push(cur);
                cur = prev[cur as usize];
            }
            path.reverse();
            return Some(path);
        }
        if f > cur_g + heuristic(ws, layer, ix, iy, gl, gx, gy, via_cost) {
            continue;
        }
        for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
            let nx = ix + dx;
            let ny = iy + dy;
            if !ws.valid(layer, nx, ny) {
                continue;
            }
            let nidx = ws.index(layer, nx, ny);
            if masks.trace[nidx] {
                continue;
            }
            let ng = cur_g + 10;
            if ng < g[nidx] {
                g[nidx] = ng;
                prev[nidx] = idx as u32;
                let h = heuristic(ws, layer, nx, ny, gl, gx, gy, via_cost);
                heap.push(Reverse((ng + h, nidx as u32)));
            }
        }
        // Via to the other copper layer at the same cell.
        let other = 1 - layer;
        let vidx = ws.index(other, ix, iy);
        if !masks.via[idx] && !masks.via[vidx] && !masks.trace[vidx] {
            let ng = cur_g + via_cost as u64;
            if ng < g[vidx] {
                g[vidx] = ng;
                prev[vidx] = idx as u32;
                let h = heuristic(ws, other, ix, iy, gl, gx, gy, via_cost);
                heap.push(Reverse((ng + h, vidx as u32)));
            }
        }
    }
    None
}

fn simplify(points: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for &p in points {
        if out.last() == Some(&p) {
            continue;
        }
        out.push(p);
    }
    if out.len() < 3 {
        return out;
    }
    let mut res = Vec::with_capacity(out.len());
    res.push(out[0]);
    for i in 1..out.len() - 1 {
        let a = out[i - 1];
        let b = out[i];
        let c = out[i + 1];
        let abx = b.x - a.x;
        let aby = b.y - a.y;
        let bcx = c.x - b.x;
        let bcy = c.y - b.y;
        if abx * bcy - aby * bcx != 0 {
            res.push(b);
        }
    }
    res.push(out[out.len() - 1]);
    res
}

fn mst_edges(pads: &[Point]) -> Vec<(usize, usize)> {
    let n = pads.len();
    if n < 2 {
        return Vec::new();
    }
    let mut in_tree = vec![false; n];
    in_tree[0] = true;
    let mut edges = Vec::with_capacity(n - 1);
    for _ in 1..n {
        let mut best: Option<(i128, usize, usize)> = None;
        for i in 0..n {
            if !in_tree[i] {
                continue;
            }
            for j in 0..n {
                if in_tree[j] {
                    continue;
                }
                let d = pads[i].dist2(pads[j]);
                if best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                    best = Some((d, i, j));
                }
            }
        }
        if let Some((_, i, j)) = best {
            in_tree[j] = true;
            edges.push((i, j));
        }
    }
    edges
}

struct NetPads {
    pads: Vec<Point>,
    refs: Vec<(usize, usize)>,
    layers: Vec<Vec<usize>>,
}

fn net_pads(board: &Board, net: usize) -> NetPads {
    let mut pads = Vec::new();
    let mut refs = Vec::new();
    let mut layers = Vec::new();
    for (fi, f) in board.footprints.iter().enumerate() {
        for (pi, p) in f.pads.iter().enumerate() {
            if p.net == Some(net) {
                pads.push(p.at);
                refs.push((fi, pi));
                layers.push(pad_layers(p));
            }
        }
    }
    // Stable geometry order independent of footprint insertion details.
    let mut order: Vec<usize> = (0..pads.len()).collect();
    order.sort_by(|&a, &b| {
        pads[a]
            .x
            .cmp(&pads[b].x)
            .then(pads[a].y.cmp(&pads[b].y))
            .then(refs[a].0.cmp(&refs[b].0))
            .then(refs[a].1.cmp(&refs[b].1))
    });
    NetPads {
        pads: order.iter().map(|&i| pads[i]).collect(),
        refs: order.iter().map(|&i| refs[i]).collect(),
        layers: order.iter().map(|&i| layers[i].clone()).collect(),
    }
}

fn route_net(
    board: &mut Board,
    ws: &Workspace,
    net: usize,
    opts: &RouterOptions,
) -> Result<usize, String> {
    let np = net_pads(board, net);
    if np.pads.len() < 2 {
        return Ok(0);
    }
    let edges = mst_edges(&np.pads);
    if edges.is_empty() {
        return Ok(0);
    }
    let masks = mask_for_net(board, ws, net, opts);
    let mut routed = 0usize;
    for (ia, ib) in edges {
        let la = *np.layers[ia].first().unwrap_or(&0);
        let lb = *np.layers[ib].first().unwrap_or(&0);
        let (pa_fi, pa_pi) = np.refs[ia];
        let (pb_fi, pb_pi) = np.refs[ib];
        let pa = board.footprints[pa_fi].pads[pa_pi].clone();
        let pb = board.footprints[pb_fi].pads[pb_pi].clone();
        let pa_label = format!("{}.{}", board.footprints[pa_fi].refdes, pa.number);
        let pb_label = format!("{}.{}", board.footprints[pb_fi].refdes, pb.number);
        let sa = match pad_anchor(ws, &masks, &pa, la) {
            Some(v) => v,
            None => {
                board.remove_net_routes(net);
                return Err(format!("no free escape cell at pad {}", pa_label));
            }
        };
        let gb = match pad_anchor(ws, &masks, &pb, lb) {
            Some(v) => v,
            None => {
                board.remove_net_routes(net);
                return Err(format!("no free escape cell at pad {}", pb_label));
            }
        };
        let path = match astar(
            ws,
            &masks,
            (la, sa.0, sa.1),
            (lb, gb.0, gb.1),
            opts.via_cost,
            opts.max_expansions,
        ) {
            Some(p) => p,
            None => {
                board.remove_net_routes(net);
                return Err(format!("no path from {} to {}", pa_label, pb_label));
            }
        };
        if add_path(board, ws, net, &path, pa.at, pb.at, opts).is_err() {
            board.remove_net_routes(net);
            return Err("failed to add path geometry".to_string());
        }
        routed += 1;
    }
    Ok(routed)
}

fn add_path(
    board: &mut Board,
    ws: &Workspace,
    net: usize,
    path: &[u32],
    start: Point,
    end: Point,
    opts: &RouterOptions,
) -> Result<(), ()> {
    if path.is_empty() {
        return Err(());
    }
    let mut nodes: Vec<(usize, Point)> = Vec::with_capacity(path.len());
    for &idx in path {
        let idx = idx as usize;
        let layer = idx / ws.plane;
        let rem = idx % ws.plane;
        let ix = (rem % ws.cols as usize) as i32;
        let iy = (rem / ws.cols as usize) as i32;
        nodes.push((layer, ws.center(ix, iy)));
    }
    let mut runs: Vec<(usize, Vec<Point>)> = Vec::new();
    for (i, &(layer, p)) in nodes.iter().enumerate() {
        if i == 0 {
            runs.push((layer, vec![p]));
        } else if runs.last().map(|(l, _)| *l == layer).unwrap_or(false) {
            runs.last_mut().map(|(_, pts)| pts.push(p));
        } else {
            runs.push((layer, vec![p]));
        }
    }
    if let Some((_, pts)) = runs.first_mut() {
        if pts.first() != Some(&start) {
            pts.insert(0, start);
        }
    }
    if let Some((_, pts)) = runs.last_mut() {
        if pts.last() != Some(&end) {
            pts.push(end);
        }
    }
    // Add vias at layer transitions.
    for i in 1..runs.len() {
        let layer_a = runs[i - 1].0;
        let layer_b = runs[i].0;
        let at = runs[i].1[0];
        let from = Layer::from_index(layer_a).unwrap_or(Layer::Top);
        let to = Layer::from_index(layer_b).unwrap_or(Layer::Bottom);
        board.add_via(net, at, from, to).map_err(|_| ())?;
    }
    for (layer, pts) in runs {
        let pts = simplify(&pts);
        if pts.len() < 2 {
            continue;
        }
        let layer = Layer::from_index(layer).unwrap_or(Layer::Top);
        board
            .add_trace(net, layer, opts.width, pts)
            .map_err(|_| ())?;
    }
    Ok(())
}

fn difficulty(board: &Board, net: usize) -> i128 {
    let np = net_pads(board, net);
    if np.pads.len() < 2 {
        return 0;
    }
    mst_edges(&np.pads)
        .into_iter()
        .map(|(a, b)| np.pads[a].manhattan(np.pads[b]))
        .sum()
}

struct AttemptResult {
    board: Board,
    routed: Vec<usize>,
    failures: Vec<RouteFailure>,
    edges_routed: usize,
    edges_total: usize,
    ripups: usize,
}

fn route_attempt(
    base: &Board,
    opts: &RouterOptions,
    attempt: usize,
    rng: &mut Rng,
    progress: &mut dyn FnMut(&RouteProgress),
) -> Result<AttemptResult, RouteError> {
    let ws = Workspace::new(base, opts)?;
    let mut board = base.clone();
    board.clear_routes();
    let mut multi: Vec<usize> = (0..board.nets.len())
        .filter(|&n| net_pads(&board, n).pads.len() >= 2)
        .collect();
    multi.sort_by(|&a, &b| {
        difficulty(&board, b)
            .cmp(&difficulty(&board, a))
            .then(a.cmp(&b))
    });
    if attempt > 0 {
        rng.shuffle(&mut multi);
    }
    let total = multi.len();
    let mut pending: VecDeque<usize> = multi.into_iter().collect();
    let mut completed: Vec<usize> = Vec::new();
    let mut failures: HashMap<usize, String> = HashMap::new();
    let mut attempts: HashMap<usize, usize> = HashMap::new();
    let mut ripups = 0usize;
    let mut edges_routed_per_net: HashMap<usize, usize> = HashMap::new();
    let mut edges_total = 0usize;
    for &n in pending.iter() {
        edges_total += mst_edges(&net_pads(&board, n).pads).len();
    }

    progress(&RouteProgress {
        attempt,
        phase: "start",
        net: None,
        net_name: String::new(),
        routed: 0,
        total,
    });

    while let Some(net) = pending.pop_front() {
        let name = board.net_name(net).to_string();
        let tries = attempts.entry(net).or_insert(0);
        *tries += 1;
        if *tries > 4 {
            failures.entry(net).or_insert_with(|| "retry limit".to_string());
            continue;
        }
        match route_net(&mut board, &ws, net, opts) {
            Ok(n) => {
                edges_routed_per_net.insert(net, n);
                completed.push(net);
                progress(&RouteProgress {
                    attempt,
                    phase: "route",
                    net: Some(net),
                    net_name: name,
                    routed: completed.len(),
                    total,
                });
            }
            Err(reason) => {
                board.remove_net_routes(net);
                let can_rip = ripups < opts.max_ripups && !completed.is_empty();
                if can_rip {
                    ripups += 1;
                    let k = 1 + (ripups % 3);
                    let mut ripped = Vec::new();
                    for _ in 0..k {
                        if let Some(v) = completed.pop() {
                            board.remove_net_routes(v);
                            edges_routed_per_net.remove(&v);
                            ripped.push(v);
                        }
                    }
                    // Ripped nets go back to the front, then the failed net.
                    for v in ripped.into_iter().rev() {
                        pending.push_front(v);
                    }
                    pending.push_front(net);
                    progress(&RouteProgress {
                        attempt,
                        phase: "ripup",
                        net: Some(net),
                        net_name: name,
                        routed: completed.len(),
                        total,
                    });
                } else {
                    failures.insert(net, reason);
                    progress(&RouteProgress {
                        attempt,
                        phase: "fail",
                        net: Some(net),
                        net_name: name,
                        routed: completed.len(),
                        total,
                    });
                }
            }
        }
    }
    let mut fails: Vec<RouteFailure> = failures
        .into_iter()
        .map(|(net, reason)| RouteFailure {
            net,
            name: board.net_name(net).to_string(),
            reason,
        })
        .collect();
    fails.sort_by(|a, b| a.net.cmp(&b.net));
    let edges_routed = edges_routed_per_net.values().copied().sum();
    Ok(AttemptResult {
        board,
        routed: completed,
        failures: fails,
        edges_routed,
        edges_total,
        ripups,
    })
}

fn all_pads_connected(board: &Board) -> bool {
    let mut conn = Connectivity::build(board);
    for net in 0..board.nets.len() {
        let groups = crate::connect::pad_groups(board, net, &mut conn);
        let pads = board.pads().filter(|(_, p)| p.net == Some(net)).count();
        if pads > 1 && groups.len() > 1 {
            return false;
        }
    }
    true
}

/// Route every multi-pad net. Existing traces and vias are cleared first; the
/// result is verified with DRC and connectivity before being written back.
pub fn route_all(
    board: &mut Board,
    opts: &RouterOptions,
    progress: &mut dyn FnMut(&RouteProgress),
) -> Result<RouteReport, RouteError> {
    opts.validate()?;
    board.validate_structure().map_err(|e| RouteError::Board(format!("{e}")))?;
    if board.copper_layers < 2 {
        return Err(RouteError::Board("at least two copper layers are required".into()));
    }
    let base = board.clone();
    let mut rng = Rng::new(opts.seed);
    let mut best: Option<(Board, AttemptResult, DrcReport, usize, bool)> = None;
    let mut attempts_used = 0usize;
    for attempt in 0..opts.retries {
        attempts_used = attempt + 1;
        let res = route_attempt(&base, opts, attempt, &mut rng, progress)?;
        let drc = drc::check(&res.board);
        let connected = all_pads_connected(&res.board);
        let score = (
            res.failures.len() + if connected { 0 } else { 1 },
            drc.errors(),
            res.edges_total.saturating_sub(res.edges_routed),
        );
        let better = match &best {
            None => true,
            Some((_, prev, prev_drc, _, prev_connected)) => {
                let pscore = (
                    prev.failures.len() + if *prev_connected { 0 } else { 1 },
                    prev_drc.errors(),
                    prev.edges_total.saturating_sub(prev.edges_routed),
                );
                score < pscore
            }
        };
        let clean = drc.is_clean() && res.failures.is_empty() && connected;
        if better {
            best = Some((res.board.clone(), res, drc, attempt, connected));
        }
        if clean {
            break;
        }
    }
    let (best_board, res, drc, attempt, connected) =
        best.ok_or_else(|| RouteError::Engine("router produced no attempts".into()))?;
    *board = best_board;
    let nets_total = (0..board.nets.len())
        .filter(|&n| net_pads(&board, n).pads.len() >= 2)
        .count();
    let mut failures = res.failures;
    if !connected {
        for net in 0..board.nets.len() {
            if net_pads(&board, net).pads.len() >= 2
                && !failures.iter().any(|f| f.net == net)
                && !all_pads_connected_for_net(&board, net)
            {
                failures.push(RouteFailure {
                    net,
                    name: board.net_name(net).to_string(),
                    reason: "pads remain disconnected".to_string(),
                });
            }
        }
        failures.sort_by(|a, b| a.net.cmp(&b.net));
    }
    let clean = drc.is_clean() && failures.is_empty() && all_pads_connected(board);
    progress(&RouteProgress {
        attempt,
        phase: "done",
        net: None,
        net_name: String::new(),
        routed: res.routed.len(),
        total: nets_total,
    });
    Ok(RouteReport {
        nets_total,
        nets_routed: res.routed.len(),
        edges_total: res.edges_total,
        edges_routed: res.edges_routed,
        failures,
        drc,
        clean,
        attempts: attempts_used,
        ripups: res.ripups,
    })
}

fn all_pads_connected_for_net(board: &Board, net: usize) -> bool {
    let mut conn = Connectivity::build(board);
    let groups = crate::connect::pad_groups(board, net, &mut conn);
    let pads = board.pads().filter(|(_, p)| p.net == Some(net)).count();
    pads <= 1 || groups.len() <= 1
}

/// Route a single net in place, returning an error string when it cannot be
/// completed. Exposed for tests and incremental tooling.
pub fn route_one(board: &mut Board, net: usize, opts: &RouterOptions) -> Result<(), RouteError> {
    let ws = Workspace::new(board, opts)?;
    route_net(board, &ws, net, opts).map(|_| ()).map_err(RouteError::Engine)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Board, DesignRules, MM};
    use crate::generate::generated_board;
    use anvil_core::library::Library;

    #[test]
    fn grid_router_connects_two_pads() {
        let lib = Library::standard();
        let mut b = Board::new_rect(60 * MM, 40 * MM, 2, DesignRules::default());
        let n = b.add_net("N");
        let fp = lib.footprint("R_0805").unwrap();
        b.add_footprint(fp, "R1", Point::new(15 * MM, 15 * MM), 0, &[Some(n), None])
            .unwrap();
        b.add_footprint(fp, "R2", Point::new(45 * MM, 25 * MM), 0, &[Some(n), None])
            .unwrap();
        let opts = RouterOptions::for_board(&b);
        let mut progress = |_p: &RouteProgress| {};
        let r = route_all(&mut b, &opts, &mut progress).unwrap();
        assert!(r.clean, "failures={:?} drc={:?}", r.failures, r.drc.violations);
        assert!(!b.traces.is_empty());
    }

    #[test]
    fn generated_board_routes_clean() {
        let mut b = generated_board(3);
        let opts = RouterOptions::for_board(&b);
        let mut progress = |_p: &RouteProgress| {};
        let r = route_all(&mut b, &opts, &mut progress).unwrap();
        assert!(r.clean, "failures={:?} drc={:?}", r.failures, r.drc.violations.iter().take(4).collect::<Vec<_>>());
    }

    #[test]
    fn twenty_generated_boards_route_clean() {
        for seed in 1..=20u64 {
            let mut b = generated_board(seed);
            let opts = RouterOptions::for_board(&b);
            let mut progress = |_p: &RouteProgress| {};
            let r = route_all(&mut b, &opts, &mut progress).unwrap();
            assert!(
                r.clean,
                "seed {seed}: failures={:?} drc={:?}",
                r.failures,
                r.drc.violations.iter().take(3).collect::<Vec<_>>()
            );
        }
    }
}
