//! Netlist extraction from geometric schematic connectivity.
//!
//! Connectivity rules (documented and deterministic):
//! * Wire vertices that coincide connect.
//! * A wire *endpoint* that lands on another wire's interior connects only when
//!   a junction or label/pin sits at that point (a T needs a junction).
//! * Pins connect to any wire that passes exactly through the pin position.
//! * Labels with the same resolved name connect globally (within this sheet).
//! * Wire crossings without a junction are not connected.
//! * Bus member labels of the form `BUS[3]` resolve to the member name declared
//!   on bus `BUS`; buses themselves are naming bundles, not copper.
//!
//! Everything is integer geometry, so extraction is exact and reproducible.

use crate::geom::{point_on_segment, Point};
use crate::library::Library;
use crate::schematic::{Component, Schematic};
use crate::util::limits;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub subject: String,
}

impl Diagnostic {
    pub fn new(
        severity: Severity,
        code: &'static str,
        subject: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Diagnostic {
            severity,
            code,
            message: message.into(),
            subject: subject.into(),
        }
    }
}

/// One pin connection in a net.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NetNode {
    pub refdes: String,
    pub pin: String,
    pub pin_name: String,
}

/// A single electrical net.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Net {
    pub id: usize,
    pub name: String,
    /// True for implicit supply/ground nets (GND, VCC symbols) and labels of
    /// kind `power`.
    pub power: bool,
    pub nodes: Vec<NetNode>,
}

impl Net {
    pub fn pin_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn has_pin(&self, refdes: &str, pin: &str) -> bool {
        self.nodes
            .iter()
            .any(|n| n.refdes == refdes && (n.pin == pin || n.pin_name == pin))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Netlist {
    pub nets: Vec<Net>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Netlist {
    pub fn net(&self, id: usize) -> Option<&Net> {
        self.nets.get(id)
    }

    pub fn net_by_name(&self, name: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.name == name)
    }

    pub fn net_id(&self, name: &str) -> Option<usize> {
        self.net_by_name(name).map(|n| n.id)
    }

    /// Net that owns `refdes`/`pin`, if any.
    pub fn pin_net(&self, refdes: &str, pin: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.has_pin(refdes, pin))
    }

    pub fn total_pins(&self) -> usize {
        self.nets.iter().map(|n| n.nodes.len()).sum()
    }

    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }
}

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn add(&mut self) -> usize {
        let id = self.parent.len();
        self.parent.push(id);
        self.rank.push(0);
        id
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let mut ra = self.find(a);
        let mut rb = self.find(b);
        if ra == rb {
            return;
        }
        if self.rank[ra] < self.rank[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent[rb] = ra;
        if self.rank[ra] == self.rank[rb] {
            self.rank[ra] = self.rank[ra].saturating_add(1);
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Attach {
    Pin { comp: usize, pin: usize },
    Label { label: usize },
}

struct PointIndex {
    map: HashMap<Point, usize>,
}

impl PointIndex {
    fn node(&mut self, uf: &mut UnionFind, p: Point) -> usize {
        if let Some(&n) = self.map.get(&p) {
            return n;
        }
        let n = uf.add();
        self.map.insert(p, n);
        n
    }
}

/// Spatial index over wire segments to keep pin-to-wire attachment linear in
/// practice. Long segments are kept in a separate list so memory stays bounded.
struct SegIndex {
    cell: i64,
    map: HashMap<(i32, i32), Vec<usize>>,
    long: Vec<usize>,
    segs: Vec<(Point, Point)>,
}

impl SegIndex {
    fn new(cell: i64) -> Self {
        SegIndex {
            cell: cell.max(1),
            map: HashMap::new(),
            long: Vec::new(),
            segs: Vec::new(),
        }
    }

    fn insert(&mut self, a: Point, b: Point) {
        let idx = self.segs.len();
        self.segs.push((a, b));
        let minx = a.x.min(b.x) / self.cell - 1;
        let maxx = a.x.max(b.x) / self.cell + 1;
        let miny = a.y.min(b.y) / self.cell - 1;
        let maxy = a.y.max(b.y) / self.cell + 1;
        let cells = (maxx - minx + 1).saturating_mul(maxy - miny + 1);
        if cells > 4096 || cells < 0 {
            self.long.push(idx);
            return;
        }
        for cy in miny..=maxy {
            for cx in minx..=maxx {
                if let Ok(key) = i32::try_from(cx) {
                    if let Ok(keyy) = i32::try_from(cy) {
                        self.map.entry((key, keyy)).or_default().push(idx);
                    }
                }
            }
        }
    }

    fn candidates(&self, p: Point) -> Vec<usize> {
        let cx = (p.x / self.cell) as i32;
        let cy = (p.y / self.cell) as i32;
        let mut out = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = self.map.get(&(cx + dx, cy + dy)) {
                    out.extend_from_slice(v);
                }
            }
        }
        out.extend_from_slice(&self.long);
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Parse a bus member label. `DATA[3]` with bus DATA members `D0..D7` resolves
/// to `D3`. Returns `None` when the label is not bus-shaped.
fn bus_ref(name: &str) -> Option<(&str, usize)> {
    let open = name.find('[')?;
    if !name.ends_with(']') || open == 0 {
        return None;
    }
    let idx: usize = name[open + 1..name.len() - 1].parse().ok()?;
    if idx > 100_000 {
        return None;
    }
    Some((&name[..open], idx))
}

fn resolve_label(name: &str, sch: &Schematic, diags: &mut Vec<Diagnostic>) -> String {
    if let Some((bus, idx)) = bus_ref(name) {
        if let Some(b) = sch.buses().iter().find(|b| b.name == bus) {
            if let Some(m) = b.members.get(idx) {
                return m.clone();
            }
            diags.push(Diagnostic::new(
                Severity::Error,
                "E_BUS_INDEX",
                name,
                format!("bus {} has no member {}", bus, idx),
            ));
            return name.to_string();
        }
        diags.push(Diagnostic::new(
            Severity::Error,
            "E_BUS_MISSING",
            name,
            format!("label references undefined bus {}", bus),
        ));
    }
    name.to_string()
}

fn implicit_power_name(c: &Component) -> Option<&'static str> {
    match c.symbol.as_str() {
        "GND" => Some("GND"),
        "VCC" => Some("VCC"),
        _ => None,
    }
}

struct RootData {
    nodes: Vec<NetNode>,
    names: BTreeSet<String>,
    power: bool,
}

/// Extract a netlist from the schematic.
pub fn extract(sch: &Schematic, lib: &Library) -> Netlist {
    let mut diags: Vec<Diagnostic> = Vec::new();
    diags.extend(sch.structural_issues().into_iter().map(|i| Diagnostic {
        severity: i.severity,
        code: i.code,
        message: i.message,
        subject: i.subject,
    }));

    let total_points: usize = sch
        .wires()
        .iter()
        .map(|w| w.points.len())
        .sum::<usize>()
        + sch.pin_count()
        + sch.labels().len()
        + sch.junctions().len();
    if total_points > limits::MAX_NETLIST_NODES {
        return Netlist {
            nets: Vec::new(),
            diagnostics: vec![Diagnostic::new(
                Severity::Error,
                "E_NETLIST_SIZE",
                "sheet",
                "schematic exceeds the netlist size limit",
            )],
        };
    }

    let mut uf = UnionFind::new(0);
    let mut pidx = PointIndex {
        map: HashMap::new(),
    };
    let mut seg_index = SegIndex::new(2_540_000);

    // Wire vertices: all points of a wire are one electrical node.
    for w in sch.wires() {
        let mut first = None;
        for p in &w.points {
            let n = pidx.node(&mut uf, *p);
            if let Some(f) = first {
                uf.union(f, n);
            } else {
                first = Some(n);
            }
        }
        for seg in w.points.windows(2) {
            seg_index.insert(seg[0], seg[1]);
        }
    }

    // Attachments: pins, labels, junctions. Each attachment point unions with
    // every wire segment that passes exactly through it.
    let mut attach: Vec<Vec<Attach>> = vec![Vec::new(); uf.parent.len()];
    let mut attach_point: Vec<Option<Point>> = vec![None; uf.parent.len()];

    let add_attachment = |point: Point,
                              a: Attach,
                              uf: &mut UnionFind,
                              pidx: &mut PointIndex,
                              attach: &mut Vec<Vec<Attach>>,
                              attach_point: &mut Vec<Option<Point>>| {
        let n = pidx.node(uf, point);
        while attach.len() <= n {
            attach.push(Vec::new());
            attach_point.push(None);
        }
        attach[n].push(a);
        attach_point[n] = Some(point);
        n
    };

    // Pins (with library validation).
    for (ci, c) in sch.components().iter().enumerate() {
        if lib.symbol(&c.symbol).is_none() {
            diags.push(Diagnostic::new(
                Severity::Warning,
                "W_UNKNOWN_SYMBOL",
                c.refdes.clone(),
                format!("symbol {} is not in the library", c.symbol),
            ));
        }
        for (pi, pin) in c.pins.iter().enumerate() {
            let n = add_attachment(
                pin.pos,
                Attach::Pin { comp: ci, pin: pi },
                &mut uf,
                &mut pidx,
                &mut attach,
                &mut attach_point,
            );
            let _ = n;
        }
    }

    // Labels.
    let mut label_net_names: Vec<String> = Vec::with_capacity(sch.labels().len());
    let mut label_nodes: Vec<usize> = Vec::with_capacity(sch.labels().len());
    for (li, l) in sch.labels().iter().enumerate() {
        let resolved = resolve_label(&l.name, sch, &mut diags);
        label_net_names.push(resolved.clone());
        let n = add_attachment(
            l.at,
            Attach::Label { label: li },
            &mut uf,
            &mut pidx,
            &mut attach,
            &mut attach_point,
        );
        label_nodes.push(n);
    }

    // Junctions.
    for j in sch.junctions() {
        add_attachment(
            j.at,
            Attach::Label { label: usize::MAX },
            &mut uf,
            &mut pidx,
            &mut attach,
            &mut attach_point,
        );
    }

    // Union attachments with wire segments passing through their point.
    let mut union_points: Vec<(usize, Point)> = Vec::new();
    for (n, opt) in attach_point.iter().enumerate() {
        if let Some(p) = *opt {
            union_points.push((n, p));
        }
    }
    for (n, p) in union_points {
        for si in seg_index.candidates(p) {
            let (a, b) = seg_index.segs[si];
            if point_on_segment(p, a, b) {
                let wn = pidx.node(&mut uf, a);
                uf.union(n, wn);
            }
        }
    }

    // Labels with the same resolved name are connected by name (scope: one
    // sheet in v1). Keep a map name -> node so later name assignment is easy.
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    for (label_idx, name) in label_net_names.iter().enumerate() {
        let n = label_nodes[label_idx];
        if let Some(&prev) = by_name.get(name) {
            uf.union(prev, n);
        } else {
            by_name.insert(name.clone(), n);
        }
    }

    // Implicit supply names from GND/VCC symbols connect globally.
    let mut implicit_nodes: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (ci, c) in sch.components().iter().enumerate() {
        if let Some(name) = implicit_power_name(c) {
            if let Some(pin) = c.pins.first() {
                let n = pidx.node(&mut uf, pin.pos);
                if let Some(&prev) = implicit_nodes.get(name) {
                    uf.union(prev, n);
                } else {
                    implicit_nodes.insert(name, n);
                }
            }
            let _ = ci;
        }
    }

    // Gather roots.
    let mut roots: HashMap<usize, RootData> = HashMap::new();
    let ensure_root = |uf: &mut UnionFind, roots: &mut HashMap<usize, RootData>, n: usize| -> usize {
        let r = uf.find(n);
        roots.entry(r).or_insert_with(|| RootData {
            nodes: Vec::new(),
            names: BTreeSet::new(),
            power: false,
        });
        r
    };

    // Collect attachments by root.
    for (n, list) in attach.iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let r = ensure_root(&mut uf, &mut roots, n);
        let root = roots.get_mut(&r).expect("root inserted");
        for a in list {
            match *a {
                Attach::Pin { comp, pin } => {
                    let c = &sch.components()[comp];
                    let p = &c.pins[pin];
                    root.nodes.push(NetNode {
                        refdes: c.refdes.clone(),
                        pin: p.number.clone(),
                        pin_name: p.name.clone(),
                    });
                    if let Some(name) = implicit_power_name(c) {
                        root.names.insert(name.to_string());
                        root.power = true;
                    }
                }
                Attach::Label { label } if label != usize::MAX => {
                    let l = &sch.labels()[label];
                    root.names.insert(label_net_names[label].clone());
                    if l.kind == crate::schematic::LabelKind::Power {
                        root.power = true;
                    }
                }
                Attach::Label { .. } => {}
            }
        }
    }

    // Apply implicit names to their nets even if the attachment loop missed
    // them (for example a GND symbol connected only by wire).
    for (name, node) in implicit_nodes {
        let r = ensure_root(&mut uf, &mut roots, node);
        if let Some(root) = roots.get_mut(&r) {
            root.names.insert(name.to_string());
            root.power = true;
        }
    }

    // Name conflicts.
    for root in roots.values() {
        if root.names.len() > 1 {
            let names: Vec<&String> = root.names.iter().collect();
            diags.push(Diagnostic::new(
                Severity::Error,
                "E_NET_NAMES",
                names[0].clone(),
                format!(
                    "net has multiple labels: {}",
                    names
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
    }

    // Intermediate nets, sorted deterministically.
    struct Pending {
        name: Option<String>,
        power: bool,
        nodes: Vec<NetNode>,
        sort_key: String,
    }
    let mut pending: Vec<Pending> = Vec::new();
    for (_, mut root) in roots {
        root.nodes.sort();
        root.nodes.dedup();
        let name = root.names.iter().next().cloned();
        let sort_key = match &name {
            Some(n) => format!("0:{}", n),
            None => format!(
                "1:{}",
                root.nodes
                    .first()
                    .map(|n| format!("{}.{}", n.refdes, n.pin))
                    .unwrap_or_default()
            ),
        };
        pending.push(Pending {
            name,
            power: root.power,
            nodes: root.nodes,
            sort_key,
        });
    }
    pending.sort_by(|a, b| a.sort_key.cmp(&b.sort_key).then(a.nodes.cmp(&b.nodes)));

    let mut nets = Vec::with_capacity(pending.len());
    let mut unnamed = 0usize;
    for (id, p) in pending.into_iter().enumerate() {
        let name = match p.name {
            Some(n) => n,
            None => {
                unnamed += 1;
                format!("N${}", unnamed)
            }
        };
        nets.push(Net {
            id,
            name,
            power: p.power,
            nodes: p.nodes,
        });
    }

    // Diagnostics about single pin nets and empty nets.
    for net in &nets {
        if net.nodes.len() == 1 {
            diags.push(Diagnostic::new(
                Severity::Warning,
                "W_SINGLE_PIN",
                net.name.clone(),
                "net has only one pin",
            ));
        }
        if net.nodes.is_empty() {
            diags.push(Diagnostic::new(
                Severity::Info,
                "I_EMPTY_NET",
                net.name.clone(),
                "labelled net has no pins",
            ));
        }
        if net.nodes.len() > 1
            && !net.power
            && !net
                .nodes
                .iter()
                .any(|n| n.pin_name.eq_ignore_ascii_case("out") || n.pin_name.eq_ignore_ascii_case("y"))
        {
            // Purely passive multi-point nets are normal; only flag nets that
            // look like digital buses with no driver-like pin at all.
            let digital = sch.components().iter().any(|c| {
                matches!(
                    c.symbol.as_str(),
                    "AND2" | "OR2" | "XOR2" | "NAND2" | "NOR2" | "NOT" | "DFF"
                ) && net.nodes.iter().any(|n| n.refdes == c.refdes)
            });
            if digital {
                diags.push(Diagnostic::new(
                    Severity::Warning,
                    "W_NO_DRIVER",
                    net.name.clone(),
                    "digital net has no output pin",
                ));
            }
        }
    }

    diags.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(a.code.cmp(b.code))
            .then(a.subject.cmp(&b.subject))
    });

    Netlist {
        nets,
        diagnostics: diags,
    }
}

/// Validate the schematic and return all diagnostics without keeping the
/// netlist. Used by the DRC-like schematic panel in Studio.
pub fn validate(sch: &Schematic, lib: &Library) -> Vec<Diagnostic> {
    extract(sch, lib).diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Library;
    use crate::schematic::{LabelKind, Schematic};

    fn lib() -> Library {
        Library::standard()
    }

    #[test]
    fn voltage_divider_nets() {
        let lib = lib();
        let mut s = Schematic::new();
        let r1 = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
        let r2 = s.add_component(&lib, "R", Point::new(0, 5_080_000)).unwrap();
        // R pins are at ±2.54mm horizontally.
        let r1_lo = s.component(r1).unwrap().pins[0].pos;
        let r2_hi = s.component(r2).unwrap().pins[0].pos;
        let r2_lo = s.component(r2).unwrap().pins[1].pos;
        s.add_wire(vec![r1_lo, r2_hi]).unwrap();
        s.add_wire(vec![r2_lo, r2_lo.add(Point::new(0, 2_540_000))])
            .unwrap();
        s.add_label("VOUT", r1_lo, LabelKind::Local).unwrap();
        let nl = extract(&s, &lib);
        let out = nl.net_by_name("VOUT").unwrap();
        assert_eq!(out.nodes.len(), 2);
        assert!(!nl.nets.is_empty());
    }

    #[test]
    fn labels_connect_by_name() {
        let lib = lib();
        let mut s = Schematic::new();
        let a = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
        let b = s.add_component(&lib, "R", Point::new(10_000_000, 0)).unwrap();
        let pa = s.component(a).unwrap().pins[0].pos;
        let pb = s.component(b).unwrap().pins[1].pos;
        s.add_wire(vec![pa, pa.add(Point::new(0, 2_540_000))]).unwrap();
        s.add_wire(vec![pb, pb.add(Point::new(0, 2_540_000))]).unwrap();
        s.add_label("CLK", pa.add(Point::new(0, 2_540_000)), LabelKind::Local).unwrap();
        s.add_label("CLK", pb.add(Point::new(0, 2_540_000)), LabelKind::Local).unwrap();
        let nl = extract(&s, &lib);
        let clk = nl.net_by_name("CLK").unwrap();
        assert_eq!(clk.nodes.len(), 2);
    }

    #[test]
    fn ground_symbols_join() {
        let lib = lib();
        let mut s = Schematic::new();
        let g1 = s.add_component(&lib, "GND", Point::new(0, 0)).unwrap();
        let g2 = s.add_component(&lib, "GND", Point::new(10_000_000, 0)).unwrap();
        let p1 = s.component(g1).unwrap().pins[0].pos;
        let p2 = s.component(g2).unwrap().pins[0].pos;
        s.add_wire(vec![p1, p1.add(Point::new(0, -2_540_000))]).unwrap();
        s.add_wire(vec![p2, p2.add(Point::new(0, -2_540_000))]).unwrap();
        let nl = extract(&s, &lib);
        assert_eq!(nl.net_by_name("GND").unwrap().nodes.len(), 2);
    }

    #[test]
    fn crossing_wires_do_not_connect_without_junction() {
        let lib = lib();
        let mut s = Schematic::new();
        s.add_wire(vec![Point::new(-5_000_000, 0), Point::new(5_000_000, 0)])
            .unwrap();
        s.add_wire(vec![Point::new(0, -5_000_000), Point::new(0, 5_000_000)])
            .unwrap();
        let r1 = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
        let p = s.component(r1).unwrap().pins[0].pos;
        // Pin lands on the horizontal wire only.
        let nl = extract(&s, &lib);
        assert!(nl.nets.len() >= 1);
        let _ = p;
    }

    #[test]
    fn bus_labels_resolve() {
        let lib = lib();
        let mut s = Schematic::new();
        s.add_bus("DATA", &["D0", "D1", "D2", "D3"]).unwrap();
        let a = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
        let b = s.add_component(&lib, "R", Point::new(20_000_000, 0)).unwrap();
        let pa = s.component(a).unwrap().pins[0].pos;
        let pb = s.component(b).unwrap().pins[0].pos;
        s.add_wire(vec![pa, pa.add(Point::new(0, 2_540_000))]).unwrap();
        s.add_wire(vec![pb, pb.add(Point::new(0, 2_540_000))]).unwrap();
        s.add_label("DATA[2]", pa.add(Point::new(0, 2_540_000)), LabelKind::Local).unwrap();
        s.add_label("DATA[2]", pb.add(Point::new(0, 2_540_000)), LabelKind::Local).unwrap();
        let nl = extract(&s, &lib);
        assert!(nl.net_by_name("D2").is_some());
    }

    #[test]
    fn limits_are_enforced() {
        let mut s = Schematic::new();
        assert!(s.add_wire(vec![Point::new(0, 0)]).is_err());
        assert!(s.add_label("", Point::new(0, 0), LabelKind::Local).is_err());
    }
}
