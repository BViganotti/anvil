//! PCB board model in integer nanometres.
//!
//! Layers are numbered from the top: 0 = top copper, 1 = bottom copper, 2 and 3
//! are optional internal layers. A board may be 2 or 4 layers. Traces live on
//! one layer; through vias span a contiguous layer range.

use crate::geom::{pad_bbox, polygon_bounds};
use anvil_core::geom::{Point, Rect};
use anvil_core::library::{Footprint as FootprintDef, PadLayer, PadShape};
use anvil_core::util::limits;
use std::fmt;

pub const MM: i64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    Top,
    Bottom,
    Inner1,
    Inner2,
}

impl Layer {
    pub fn index(self) -> usize {
        match self {
            Layer::Top => 0,
            Layer::Bottom => 1,
            Layer::Inner1 => 2,
            Layer::Inner2 => 3,
        }
    }

    pub fn from_index(i: usize) -> Option<Layer> {
        match i {
            0 => Some(Layer::Top),
            1 => Some(Layer::Bottom),
            2 => Some(Layer::Inner1),
            3 => Some(Layer::Inner2),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Top => "top",
            Layer::Bottom => "bottom",
            Layer::Inner1 => "inner1",
            Layer::Inner2 => "inner2",
        }
    }

    pub fn parse(s: &str) -> Option<Layer> {
        match s.trim().to_ascii_lowercase().as_str() {
            "top" | "f.cu" | "front" => Some(Layer::Top),
            "bottom" | "b.cu" | "back" => Some(Layer::Bottom),
            "inner1" | "in1.cu" => Some(Layer::Inner1),
            "inner2" | "in2.cu" => Some(Layer::Inner2),
            _ => None,
        }
    }

    pub fn all() -> [Layer; 4] {
        [Layer::Top, Layer::Bottom, Layer::Inner1, Layer::Inner2]
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pad {
    pub number: String,
    /// Absolute board position.
    pub at: Point,
    /// Footprint-local offset (unrotated).
    pub offset: Point,
    pub size: (i64, i64),
    pub shape: PadShape,
    pub drill: i64,
    pub layers: PadLayer,
    pub net: Option<usize>,
}

impl Pad {
    pub fn label(&self, refdes: &str) -> String {
        format!("{}.{}", refdes, self.number)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Footprint {
    pub refdes: String,
    pub name: String,
    pub at: Point,
    pub rotation: u16,
    pub pads: Vec<Pad>,
    pub courtyard: (i64, i64),
}

impl Footprint {
    pub fn bounds(&self) -> Rect {
        let mut r: Option<Rect> = None;
        for p in &self.pads {
            let b = pad_bbox(p);
            r = Some(match r {
                Some(x) => x.union(b),
                None => b,
            });
        }
        r.unwrap_or_else(|| {
            Rect::new(
                Point::new(
                    self.at.x - self.courtyard.0 / 2,
                    self.at.y - self.courtyard.1 / 2,
                ),
                Point::new(
                    self.at.x + self.courtyard.0 / 2,
                    self.at.y + self.courtyard.1 / 2,
                ),
            )
        })
    }

    pub fn pad(&self, number: &str) -> Option<&Pad> {
        self.pads.iter().find(|p| p.number == number)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    pub net: usize,
    pub layer: Layer,
    pub width: i64,
    pub points: Vec<Point>,
}

impl Trace {
    pub fn segments(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        self.points.windows(2).map(|w| (w[0], w[1]))
    }

    pub fn bounds(&self) -> Rect {
        let r = Rect::from_points(&self.points).unwrap_or(Rect::new(Point::ORIGIN, Point::ORIGIN));
        r.expand(self.width / 2)
    }

    pub fn length(&self) -> i128 {
        self.points
            .windows(2)
            .map(|w| w[0].manhattan(w[1]))
            .sum()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Via {
    pub net: usize,
    pub at: Point,
    pub drill: i64,
    pub diameter: i64,
    pub from: Layer,
    pub to: Layer,
}

impl Via {
    pub fn spans(&self, layer: Layer) -> bool {
        layer.index() >= self.from.index() && layer.index() <= self.to.index()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesignRules {
    pub min_trace: i64,
    pub min_clearance: i64,
    pub min_drill: i64,
    pub min_annular: i64,
    pub edge_clearance: i64,
    pub via_drill: i64,
    pub via_diameter: i64,
}

impl Default for DesignRules {
    fn default() -> Self {
        DesignRules {
            min_trace: 200_000,
            min_clearance: 200_000,
            min_drill: 300_000,
            min_annular: 100_000,
            edge_clearance: 300_000,
            via_drill: 400_000,
            via_diameter: 800_000,
        }
    }
}

impl DesignRules {
    pub fn validate(&self) -> Result<(), BoardError> {
        if self.min_trace <= 0
            || self.min_clearance < 0
            || self.min_drill <= 0
            || self.min_annular < 0
            || self.edge_clearance < 0
            || self.via_drill <= 0
            || self.via_diameter <= self.via_drill
        {
            return Err(BoardError::Invalid("design rules"));
        }
        Ok(())
    }

    pub fn mm(&self) -> [f64; 7] {
        [
            self.min_trace as f64 / 1e6,
            self.min_clearance as f64 / 1e6,
            self.min_drill as f64 / 1e6,
            self.min_annular as f64 / 1e6,
            self.edge_clearance as f64 / 1e6,
            self.via_drill as f64 / 1e6,
            self.via_diameter as f64 / 1e6,
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardError {
    Invalid(&'static str),
    InvalidCoordinate(Point),
    UnknownNet(usize),
    Limit(&'static str),
    DuplicateRefdes(String),
    PadMismatch,
    UnknownLayer,
}

impl fmt::Display for BoardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoardError::Invalid(what) => write!(f, "invalid board: {what}"),
            BoardError::InvalidCoordinate(p) => write!(f, "invalid coordinate {},{}", p.x, p.y),
            BoardError::UnknownNet(n) => write!(f, "unknown net id {n}"),
            BoardError::Limit(what) => write!(f, "board limit exceeded: {what}"),
            BoardError::DuplicateRefdes(r) => write!(f, "duplicate footprint refdes {r}"),
            BoardError::PadMismatch => write!(f, "pad count does not match footprint"),
            BoardError::UnknownLayer => write!(f, "unknown layer"),
        }
    }
}

impl std::error::Error for BoardError {}

#[derive(Clone, Debug)]
pub struct Board {
    pub name: String,
    pub outline: Vec<Point>,
    pub copper_layers: u8,
    pub footprints: Vec<Footprint>,
    pub traces: Vec<Trace>,
    pub vias: Vec<Via>,
    pub nets: Vec<String>,
    pub rules: DesignRules,
}

impl Board {
    pub fn new_rect(width: i64, height: i64, copper_layers: u8, rules: DesignRules) -> Self {
        let mut outline = vec![
            Point::new(0, 0),
            Point::new(width, 0),
            Point::new(width, height),
            Point::new(0, height),
        ];
        if width < 0 || height < 0 {
            outline = vec![
                Point::new(width.min(0), height.min(0)),
                Point::new(width.max(0), height.min(0)),
                Point::new(width.max(0), height.max(0)),
                Point::new(width.min(0), height.max(0)),
            ];
        }
        Board {
            name: "Untitled board".to_string(),
            outline,
            copper_layers: copper_layers.clamp(2, 4),
            footprints: Vec::new(),
            traces: Vec::new(),
            vias: Vec::new(),
            nets: Vec::new(),
            rules,
        }
    }

    pub fn demo() -> Self {
        Board::new_rect(50 * MM, 35 * MM, 2, DesignRules::default())
    }

    pub fn add_net(&mut self, name: &str) -> usize {
        if let Some(i) = self.nets.iter().position(|n| n == name) {
            return i;
        }
        self.nets.push(name.to_string());
        self.nets.len() - 1
    }

    pub fn net_id(&self, name: &str) -> Option<usize> {
        self.nets.iter().position(|n| n == name)
    }

    pub fn net_name(&self, id: usize) -> &str {
        self.nets.get(id).map(|s| s.as_str()).unwrap_or("?")
    }

    pub fn bounds(&self) -> Rect {
        polygon_bounds(&self.outline)
    }

    pub fn pads(&self) -> impl Iterator<Item = (&Footprint, &Pad)> {
        self.footprints
            .iter()
            .flat_map(|f| f.pads.iter().map(move |p| (f, p)))
    }

    pub fn pad_count(&self) -> usize {
        self.footprints.iter().map(|f| f.pads.len()).sum()
    }

    pub fn trace_count(&self) -> usize {
        self.traces.len()
    }

    pub fn via_count(&self) -> usize {
        self.vias.len()
    }

    /// Add a footprint from the library with explicit net assignment by pad
    /// order. `pad_nets` must have one entry per pad.
    pub fn add_footprint(
        &mut self,
        def: &FootprintDef,
        refdes: &str,
        at: Point,
        rotation: u16,
        pad_nets: &[Option<usize>],
    ) -> Result<usize, BoardError> {
        if self.footprints.len() >= limits::MAX_BOARD_ITEMS / 4 {
            return Err(BoardError::Limit("footprints"));
        }
        if !at.is_valid() {
            return Err(BoardError::InvalidCoordinate(at));
        }
        if refdes.trim().is_empty() || refdes.len() > limits::MAX_NAME {
            return Err(BoardError::Invalid("refdes"));
        }
        if self.footprints.iter().any(|f| f.refdes == refdes) {
            return Err(BoardError::DuplicateRefdes(refdes.to_string()));
        }
        if def.pads.len() != pad_nets.len() {
            return Err(BoardError::PadMismatch);
        }
        for n in pad_nets.iter().flatten() {
            if *n >= self.nets.len() {
                return Err(BoardError::UnknownNet(*n));
            }
        }
        let rotation = rotation - rotation % 90;
        let mut pads = Vec::with_capacity(def.pads.len());
        for (pd, net) in def.pads.iter().zip(pad_nets.iter()) {
            let off = pd.offset.rotate(rotation);
            let mut size = pd.size;
            if rotation % 180 == 90 {
                size = (size.1, size.0);
            }
            let drill = pd.drill.max(0);
            pads.push(Pad {
                number: pd.number.clone(),
                at: Point::new(at.x.saturating_add(off.x), at.y.saturating_add(off.y)),
                offset: pd.offset,
                size,
                shape: pd.shape,
                drill,
                layers: if drill > 0 {
                    PadLayer::Through
                } else {
                    match pd.layer {
                        PadLayer::Through => PadLayer::Through,
                        other => other,
                    }
                },
                net: *net,
            });
        }
        self.footprints.push(Footprint {
            refdes: refdes.to_string(),
            name: def.name.clone(),
            at,
            rotation,
            pads,
            courtyard: def.courtyard,
        });
        Ok(self.footprints.len() - 1)
    }

    pub fn move_footprint(&mut self, index: usize, at: Point) -> Result<(), BoardError> {
        if !at.is_valid() {
            return Err(BoardError::InvalidCoordinate(at));
        }
        let fp = self
            .footprints
            .get_mut(index)
            .ok_or(BoardError::Invalid("unknown footprint"))?;
        fp.at = at;
        for p in &mut fp.pads {
            let off = p.offset.rotate(fp.rotation);
            p.at = Point::new(at.x.saturating_add(off.x), at.y.saturating_add(off.y));
        }
        Ok(())
    }

    pub fn add_trace(
        &mut self,
        net: usize,
        layer: Layer,
        width: i64,
        points: Vec<Point>,
    ) -> Result<usize, BoardError> {
        if net >= self.nets.len() {
            return Err(BoardError::UnknownNet(net));
        }
        if self.traces.len() >= limits::MAX_BOARD_ITEMS {
            return Err(BoardError::Limit("traces"));
        }
        if layer.index() >= self.copper_layers as usize {
            return Err(BoardError::UnknownLayer);
        }
        if width <= 0 {
            return Err(BoardError::Invalid("trace width"));
        }
        if points.len() < 2 || points.len() > limits::MAX_TRACE_POINTS {
            return Err(BoardError::Invalid("trace points"));
        }
        let mut clean = Vec::with_capacity(points.len());
        for p in points {
            if !p.is_valid() {
                return Err(BoardError::InvalidCoordinate(p));
            }
            if clean.last() != Some(&p) {
                clean.push(p);
            }
        }
        if clean.len() < 2 {
            return Err(BoardError::Invalid("zero length trace"));
        }
        self.traces.push(Trace {
            net,
            layer,
            width,
            points: clean,
        });
        Ok(self.traces.len() - 1)
    }

    pub fn add_via(
        &mut self,
        net: usize,
        at: Point,
        from: Layer,
        to: Layer,
    ) -> Result<usize, BoardError> {
        if net >= self.nets.len() {
            return Err(BoardError::UnknownNet(net));
        }
        if self.vias.len() >= limits::MAX_BOARD_ITEMS {
            return Err(BoardError::Limit("vias"));
        }
        if !at.is_valid() {
            return Err(BoardError::InvalidCoordinate(at));
        }
        if from.index() >= self.copper_layers as usize || to.index() >= self.copper_layers as usize {
            return Err(BoardError::UnknownLayer);
        }
        let (from, to) = if from.index() <= to.index() {
            (from, to)
        } else {
            (to, from)
        };
        self.vias.push(Via {
            net,
            at,
            drill: self.rules.via_drill,
            diameter: self.rules.via_diameter,
            from,
            to,
        });
        Ok(self.vias.len() - 1)
    }

    pub fn clear_routes(&mut self) {
        self.traces.clear();
        self.vias.clear();
    }

    pub fn remove_net_routes(&mut self, net: usize) {
        self.traces.retain(|t| t.net != net);
        self.vias.retain(|v| v.net != net);
    }

    pub fn layer_enabled(&self, layer: Layer) -> bool {
        layer.index() < self.copper_layers as usize
    }

    pub fn contains(&self, p: Point) -> bool {
        anvil_core::geom::point_in_polygon(p, &self.outline)
    }

    pub fn validate_structure(&self) -> Result<(), BoardError> {
        if self.outline.len() < 3 || self.outline.len() > 10_000 {
            return Err(BoardError::Invalid("outline"));
        }
        if self.outline.iter().any(|p| !p.is_valid()) {
            return Err(BoardError::Invalid("outline coordinate"));
        }
        if anvil_core::geom::polygon_area2(&self.outline) == 0 {
            return Err(BoardError::Invalid("zero area outline"));
        }
        self.rules.validate()?;
        if self.copper_layers < 2 || self.copper_layers > 4 {
            return Err(BoardError::Invalid("copper layer count"));
        }
        let mut refs: Vec<&str> = self.footprints.iter().map(|f| f.refdes.as_str()).collect();
        refs.sort_unstable();
        let before = refs.len();
        refs.dedup();
        if refs.len() != before {
            return Err(BoardError::Invalid("duplicate refdes"));
        }
        for f in &self.footprints {
            if f.pads.len() > limits::MAX_PINS_PER_COMPONENT {
                return Err(BoardError::Limit("footprint pads"));
            }
            for p in &f.pads {
                if !p.at.is_valid() {
                    return Err(BoardError::Invalid("pad coordinate"));
                }
                if p.size.0 <= 0 || p.size.1 <= 0 {
                    return Err(BoardError::Invalid("pad size"));
                }
                if let Some(n) = p.net {
                    if n >= self.nets.len() {
                        return Err(BoardError::UnknownNet(n));
                    }
                }
            }
        }
        for t in &self.traces {
            if t.net >= self.nets.len() {
                return Err(BoardError::UnknownNet(t.net));
            }
            if t.width <= 0 || t.points.len() < 2 {
                return Err(BoardError::Invalid("trace"));
            }
            if t.layer.index() >= self.copper_layers as usize {
                return Err(BoardError::UnknownLayer);
            }
            if t.points.iter().any(|p| !p.is_valid()) {
                return Err(BoardError::Invalid("trace coordinate"));
            }
        }
        for v in &self.vias {
            if v.net >= self.nets.len() {
                return Err(BoardError::UnknownNet(v.net));
            }
            if v.drill <= 0 || v.diameter <= v.drill {
                return Err(BoardError::Invalid("via size"));
            }
            if v.from.index() > v.to.index() {
                return Err(BoardError::Invalid("via layer span"));
            }
        }
        Ok(())
    }

    pub fn generate_refdes(&self, prefix: &str) -> String {
        for i in 1..=99_999 {
            let candidate = format!("{}{}", prefix, i);
            if !self.footprints.iter().any(|f| f.refdes == candidate) {
                return candidate;
            }
        }
        format!("{}X", prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::library::Library;

    #[test]
    fn board_outline_and_nets() {
        let mut b = Board::new_rect(50 * MM, 40 * MM, 2, DesignRules::default());
        assert_eq!(b.add_net("GND"), 0);
        assert_eq!(b.add_net("VCC"), 1);
        assert_eq!(b.add_net("GND"), 0);
        assert!(b.contains(Point::new(25 * MM, 20 * MM)));
        assert!(!b.contains(Point::new(-1, 0)));
        b.validate_structure().unwrap();
    }

    #[test]
    fn footprint_pads_are_placed() {
        let lib = Library::standard();
        let mut b = Board::new_rect(50 * MM, 40 * MM, 2, DesignRules::default());
        let n = b.add_net("N1");
        let fp = lib.footprint("R_0805").unwrap();
        b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n), None])
            .unwrap();
        assert_eq!(b.pad_count(), 2);
        let f = &b.footprints[0];
        assert_eq!(f.pads[0].at, Point::new(10 * MM - 950_000, 10 * MM));
        assert_eq!(f.pads[1].at, Point::new(10 * MM + 950_000, 10 * MM));
    }

    #[test]
    fn trace_validation() {
        let mut b = Board::new_rect(50 * MM, 40 * MM, 2, DesignRules::default());
        let n = b.add_net("N");
        assert!(b
            .add_trace(
                n,
                Layer::Top,
                200_000,
                vec![Point::new(MM, MM), Point::new(2 * MM, MM)]
            )
            .is_ok());
        assert!(b
            .add_trace(n, Layer::Inner2, 200_000, vec![Point::new(0, 0), Point::new(MM, 0)])
            .is_err());
        assert!(b.add_trace(n, Layer::Top, 0, vec![Point::new(0, 0), Point::new(MM, 0)]).is_err());
    }
}
