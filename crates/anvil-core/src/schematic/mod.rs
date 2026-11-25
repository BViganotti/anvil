//! Schematic data model.
//!
//! A schematic is a flat sheet made of components, polyline wires, junctions,
//! labels and named buses. Connectivity is derived geometrically (see
//! [`crate::netlist`]); the model stores no net ids, which keeps edits cheap and
//! the netlist always in sync with the drawing.
//!
//! Coordinates are nanometres on a 1.27 mm schematic grid.

use crate::geom::Point;
use crate::util::limits;
use std::fmt;

/// Stable component identifier. Ids are never reused within a schematic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CompId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WireId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JunctionId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LabelId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BusId(pub u32);

impl fmt::Display for CompId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "C{}", self.0)
    }
}

macro_rules! display_id {
    ($ty:ty, $prefix:literal) => {
        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", $prefix, self.0)
            }
        }
    };
}

display_id!(WireId, "W");
display_id!(JunctionId, "J");
display_id!(LabelId, "L");
display_id!(BusId, "B");

/// Pin electrical role, used by validation and simulation compilation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PinKind {
    Passive,
    Input,
    Output,
    Bidirectional,
    Power,
    NoConnect,
}

impl PinKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PinKind::Passive => "passive",
            PinKind::Input => "input",
            PinKind::Output => "output",
            PinKind::Bidirectional => "bidir",
            PinKind::Power => "power",
            PinKind::NoConnect => "nc",
        }
    }
}

/// A pin instance. `pos` is the absolute sheet coordinate and is recomputed
/// whenever the owning component moves or rotates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    pub name: String,
    pub number: String,
    pub offset: Point,
    pub pos: Point,
    pub kind: PinKind,
}

/// A component instance placed on the sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub id: CompId,
    pub refdes: String,
    pub value: String,
    pub symbol: String,
    pub footprint: String,
    pub at: Point,
    /// Clockwise rotation in degrees: 0, 90, 180 or 270.
    pub rotation: u16,
    pub pins: Vec<Pin>,
    pub properties: Vec<(String, String)>,
    pub mirror_x: bool,
}

impl Component {
    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn pin_by_name(&self, name: &str) -> Option<&Pin> {
        self.pins.iter().find(|p| p.name == name)
    }

    pub fn pin_by_number(&self, number: &str) -> Option<&Pin> {
        self.pins.iter().find(|p| p.number == number)
    }

    pub fn pin_position(&self, index: usize) -> Option<Point> {
        self.pins.get(index).map(|p| p.pos)
    }

    pub fn recompute_pins(&mut self) {
        for pin in &mut self.pins {
            let mut o = pin.offset;
            if self.mirror_x {
                o.x = -o.x;
            }
            pin.pos = o.rotate(self.rotation).add(self.at);
        }
    }

    pub fn bounds(&self) -> crate::geom::Rect {
        let mut pts: Vec<Point> = self.pins.iter().map(|p| p.pos).collect();
        pts.push(self.at);
        crate::geom::Rect::from_points(&pts).unwrap_or(crate::geom::Rect::new(self.at, self.at))
            .expand(1_270_000)
    }
}

/// Polyline wire. Two points is the normal case; bends are allowed and are
/// preserved by the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wire {
    pub id: WireId,
    pub points: Vec<Point>,
}

impl Wire {
    pub fn segments(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        self.points.windows(2).map(|w| (w[0], w[1]))
    }

    pub fn bounds(&self) -> crate::geom::Rect {
        crate::geom::Rect::from_points(&self.points).unwrap_or(crate::geom::Rect::new(
            Point::ORIGIN,
            Point::ORIGIN,
        ))
    }

    pub fn length(&self) -> i128 {
        self.points
            .windows(2)
            .map(|w| w[0].manhattan(w[1]))
            .sum()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Junction {
    pub id: JunctionId,
    pub at: Point,
}

/// Label scope. Local labels join nets on this sheet; global labels join across
/// sheets; power labels behave globally. Anvil v1 has one sheet but the model
/// and netlist keep the distinction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LabelKind {
    Local,
    Global,
    Power,
}

impl LabelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LabelKind::Local => "local",
            LabelKind::Global => "global",
            LabelKind::Power => "power",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "local" | "label" => Some(LabelKind::Local),
            "global" => Some(LabelKind::Global),
            "power" => Some(LabelKind::Power),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub id: LabelId,
    pub at: Point,
    pub name: String,
    pub kind: LabelKind,
    pub rotation: u16,
}

/// A named bundle of signals. Bus labels of the form `NAME[3]` resolve to the
/// member at index 3; the netlist expands member connectivity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bus {
    pub id: BusId,
    pub name: String,
    pub members: Vec<String>,
    pub points: Vec<Point>,
}

/// Validation issue produced by the schematic editor and netlist extractor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub severity: crate::netlist::Severity,
    pub code: &'static str,
    pub message: String,
    pub subject: String,
}

impl Issue {
    pub fn error(code: &'static str, subject: impl Into<String>, message: impl Into<String>) -> Self {
        Issue {
            severity: crate::netlist::Severity::Error,
            code,
            message: message.into(),
            subject: subject.into(),
        }
    }

    pub fn warning(
        code: &'static str,
        subject: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Issue {
            severity: crate::netlist::Severity::Warning,
            code,
            message: message.into(),
            subject: subject.into(),
        }
    }

    pub fn info(code: &'static str, subject: impl Into<String>, message: impl Into<String>) -> Self {
        Issue {
            severity: crate::netlist::Severity::Info,
            code,
            message: message.into(),
            subject: subject.into(),
        }
    }
}

/// Sheet metadata rendered in the UI and exported with the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    pub title: String,
    pub company: String,
    pub revision: String,
    pub date: String,
}

impl Default for Sheet {
    fn default() -> Self {
        Sheet {
            title: "Untitled".to_string(),
            company: "Anvil".to_string(),
            revision: "A".to_string(),
            date: String::new(),
        }
    }
}

/// The complete schematic sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schematic {
    pub sheet: Sheet,
    components: Vec<Component>,
    wires: Vec<Wire>,
    junctions: Vec<Junction>,
    labels: Vec<Label>,
    buses: Vec<Bus>,
    next_component: u32,
    next_wire: u32,
    next_junction: u32,
    next_label: u32,
    next_bus: u32,
}

impl Default for Schematic {
    fn default() -> Self {
        Schematic {
            sheet: Sheet::default(),
            components: Vec::new(),
            wires: Vec::new(),
            junctions: Vec::new(),
            labels: Vec::new(),
            buses: Vec::new(),
            next_component: 1,
            next_wire: 1,
            next_junction: 1,
            next_label: 1,
            next_bus: 1,
        }
    }
}

impl Schematic {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn components(&self) -> &[Component] {
        &self.components
    }

    pub fn wires(&self) -> &[Wire] {
        &self.wires
    }

    pub fn junctions(&self) -> &[Junction] {
        &self.junctions
    }

    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    pub fn buses(&self) -> &[Bus] {
        &self.buses
    }

    pub fn component(&self, id: CompId) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }

    pub fn component_mut(&mut self, id: CompId) -> Option<&mut Component> {
        self.components.iter_mut().find(|c| c.id == id)
    }

    pub fn wire(&self, id: WireId) -> Option<&Wire> {
        self.wires.iter().find(|w| w.id == id)
    }

    pub fn label(&self, id: LabelId) -> Option<&Label> {
        self.labels.iter().find(|l| l.id == id)
    }

    pub fn bus(&self, id: BusId) -> Option<&Bus> {
        self.buses.iter().find(|b| b.id == id)
    }

    pub fn refdes_exists(&self, refdes: &str) -> bool {
        self.components.iter().any(|c| c.refdes == refdes)
    }

    pub fn component_count(&self) -> usize {
        self.components.len()
    }

    pub fn pin_count(&self) -> usize {
        self.components.iter().map(|c| c.pins.len()).sum()
    }

    /// Bounds of everything on the sheet, in nanometers.
    pub fn bounds(&self) -> crate::geom::Rect {
        let mut rect: Option<crate::geom::Rect> = None;
        for c in &self.components {
            let r = c.bounds();
            rect = Some(match rect {
                Some(x) => x.union(r),
                None => r,
            });
        }
        for w in &self.wires {
            let r = w.bounds();
            rect = Some(match rect {
                Some(x) => x.union(r),
                None => r,
            });
        }
        for l in &self.labels {
            let r = crate::geom::Rect::new(l.at, l.at).expand(2_540_000);
            rect = Some(match rect {
                Some(x) => x.union(r),
                None => r,
            });
        }
        rect.unwrap_or(crate::geom::Rect::new(
            Point::new(-12_700_000, -12_700_000),
            Point::new(12_700_000, 12_700_000),
        ))
    }

    /// Basic structural validation that does not require connectivity.
    pub fn structural_issues(&self) -> Vec<Issue> {
        let mut out = Vec::new();
        for c in &self.components {
            if c.refdes.trim().is_empty() {
                out.push(Issue::error("E_REFDES", c.id.to_string(), "component has no refdes"));
            }
            if c.refdes.len() > limits::MAX_NAME {
                out.push(Issue::error(
                    "E_REFDES_LEN",
                    c.id.to_string(),
                    "refdes exceeds the length limit",
                ));
            }
            if c.pins.is_empty() {
                out.push(Issue::warning(
                    "W_NO_PINS",
                    c.refdes.clone(),
                    "component has no pins",
                ));
            }
            if c.pins.len() > limits::MAX_PINS_PER_COMPONENT {
                out.push(Issue::error(
                    "E_PIN_COUNT",
                    c.refdes.clone(),
                    "component has too many pins",
                ));
            }
            for p in &c.pins {
                if p.name.is_empty() || p.name.len() > limits::MAX_NAME {
                    out.push(Issue::error(
                        "E_PIN_NAME",
                        c.refdes.clone(),
                        "pin has an empty or overlong name",
                    ));
                }
                if !p.pos.is_valid() {
                    out.push(Issue::error(
                        "E_COORD",
                        c.refdes.clone(),
                        "pin position outside the coordinate limit",
                    ));
                }
            }
        }
        for w in &self.wires {
            if w.points.len() < 2 {
                out.push(Issue::error(
                    "E_WIRE_SHORT",
                    w.id.to_string(),
                    "wire needs at least two points",
                ));
            }
            if w.points.len() > limits::MAX_WIRE_POINTS {
                out.push(Issue::error(
                    "E_WIRE_POINTS",
                    w.id.to_string(),
                    "wire exceeds the point limit",
                ));
            }
            if w.points.iter().any(|p| !p.is_valid()) {
                out.push(Issue::error(
                    "E_COORD",
                    w.id.to_string(),
                    "wire point outside the coordinate limit",
                ));
            }
            if w.points.windows(2).any(|s| s[0] == s[1]) {
                out.push(Issue::warning(
                    "W_WIRE_DUP",
                    w.id.to_string(),
                    "wire contains a zero-length segment",
                ));
            }
        }
        for l in &self.labels {
            let valid = !l.name.trim().is_empty() && l.name.len() <= limits::MAX_NAME;
            if !valid {
                out.push(Issue::error(
                    "E_LABEL_NAME",
                    l.id.to_string(),
                    "label has an empty or overlong name",
                ));
            }
        }
        for b in &self.buses {
            if b.name.is_empty() || b.name.len() > limits::MAX_NAME {
                out.push(Issue::error(
                    "E_BUS_NAME",
                    b.id.to_string(),
                    "bus has an empty or overlong name",
                ));
            }
            if b.members.len() > limits::MAX_BUS_MEMBERS {
                out.push(Issue::error(
                    "E_BUS_MEMBERS",
                    b.id.to_string(),
                    "bus exceeds the member limit",
                ));
            }
            for m in &b.members {
                if m.is_empty() || m.len() > limits::MAX_NAME {
                    out.push(Issue::error(
                        "E_BUS_MEMBER",
                        b.id.to_string(),
                        "bus member has an empty or overlong name",
                    ));
                }
            }
        }
        out
    }
}

pub mod edit;
pub use edit::{EditError, Placed};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_schematic_has_bounds() {
        let s = Schematic::new();
        assert_eq!(s.component_count(), 0);
        assert!(s.bounds().width() > 0);
        assert!(s.structural_issues().is_empty());
    }

    #[test]
    fn pin_rotation_is_exact() {
        let mut c = Component {
            id: CompId(1),
            refdes: "R1".into(),
            value: "1k".into(),
            symbol: "R".into(),
            footprint: "R_0805".into(),
            at: Point::new(100, 200),
            rotation: 0,
            pins: vec![
                Pin {
                    name: "1".into(),
                    number: "1".into(),
                    offset: Point::new(-100, 0),
                    pos: Point::ORIGIN,
                    kind: PinKind::Passive,
                },
                Pin {
                    name: "2".into(),
                    number: "2".into(),
                    offset: Point::new(100, 0),
                    pos: Point::ORIGIN,
                    kind: PinKind::Passive,
                },
            ],
            properties: Vec::new(),
            mirror_x: false,
        };
        c.recompute_pins();
        assert_eq!(c.pins[0].pos, Point::new(0, 200));
        c.rotation = 90;
        c.recompute_pins();
        assert_eq!(c.pins[0].pos, Point::new(100, 300));
        c.mirror_x = true;
        c.recompute_pins();
        assert_eq!(c.pins[0].pos, Point::new(100, 100));
    }
}
