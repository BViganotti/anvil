//! Schematic editing operations with validation.
//!
//! Every mutating entry point checks bounds, limits and identity. Invalid edits
//! return an [`EditError`] and leave the schematic unchanged.

use super::{
    Bus, BusId, CompId, Component, Junction, JunctionId, Label, LabelId, LabelKind, Pin, Schematic,
    Wire, WireId,
};
use crate::geom::Point;
use crate::library::Library;
use crate::util::limits;
use std::fmt;

/// A generic edit failure. Codes are stable and surfaced by the Studio API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    UnknownPart(String),
    UnknownComponent(u32),
    UnknownWire(u32),
    UnknownLabel(u32),
    UnknownBus(u32),
    DuplicateRefdes(String),
    InvalidCoordinate(Point),
    Limit(&'static str),
    BadInput(&'static str),
}

impl EditError {
    pub fn code(&self) -> &'static str {
        match self {
            EditError::UnknownPart(_) => "unknown_part",
            EditError::UnknownComponent(_) => "unknown_component",
            EditError::UnknownWire(_) => "unknown_wire",
            EditError::UnknownLabel(_) => "unknown_label",
            EditError::UnknownBus(_) => "unknown_bus",
            EditError::DuplicateRefdes(_) => "duplicate_refdes",
            EditError::InvalidCoordinate(_) => "invalid_coordinate",
            EditError::Limit(_) => "limit",
            EditError::BadInput(_) => "bad_input",
        }
    }
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::UnknownPart(p) => write!(f, "unknown part: {p}"),
            EditError::UnknownComponent(id) => write!(f, "unknown component id: {id}"),
            EditError::UnknownWire(id) => write!(f, "unknown wire id: {id}"),
            EditError::UnknownLabel(id) => write!(f, "unknown label id: {id}"),
            EditError::UnknownBus(id) => write!(f, "unknown bus id: {id}"),
            EditError::DuplicateRefdes(r) => write!(f, "duplicate refdes: {r}"),
            EditError::InvalidCoordinate(p) => write!(f, "invalid coordinate: {},{}", p.x, p.y),
            EditError::Limit(what) => write!(f, "limit exceeded: {what}"),
            EditError::BadInput(what) => write!(f, "bad input: {what}"),
        }
    }
}

impl std::error::Error for EditError {}

/// Result of placing a component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub id: CompId,
}

impl Schematic {
    /// Add a component from the library with an automatically chosen refdes.
    pub fn add_component(
        &mut self,
        lib: &Library,
        part_name: &str,
        at: Point,
    ) -> Result<CompId, EditError> {
        self.add_component_with(lib, part_name, at, None)
    }

    pub fn add_component_with_refdes(
        &mut self,
        lib: &Library,
        part_name: &str,
        at: Point,
        refdes: &str,
    ) -> Result<CompId, EditError> {
        self.add_component_with(lib, part_name, at, Some(refdes))
    }

    fn add_component_with(
        &mut self,
        lib: &Library,
        part_name: &str,
        at: Point,
        refdes: Option<&str>,
    ) -> Result<CompId, EditError> {
        if !at.is_valid() {
            return Err(EditError::InvalidCoordinate(at));
        }
        if self.components.len() >= limits::MAX_COMPONENTS {
            return Err(EditError::Limit("components"));
        }
        let part = lib
            .part(part_name)
            .ok_or_else(|| EditError::UnknownPart(part_name.to_string()))?
            .clone();
        let symbol = lib
            .symbol(&part.symbol)
            .ok_or_else(|| EditError::UnknownPart(part.symbol.clone()))?;
        let refdes = match refdes {
            Some(r) => {
                let r = r.trim();
                if r.is_empty() || r.len() > limits::MAX_NAME {
                    return Err(EditError::BadInput("refdes"));
                }
                if self.refdes_exists(r) {
                    return Err(EditError::DuplicateRefdes(r.to_string()));
                }
                r.to_string()
            }
            None => self.next_refdes(part_prefix(&part)),
        };
        let mut comp = Component {
            id: CompId(self.next_component),
            refdes,
            value: part.value.clone(),
            symbol: part.symbol.clone(),
            footprint: part.footprint.clone(),
            at,
            rotation: 0,
            pins: symbol
                .pins
                .iter()
                .map(|p| Pin {
                    name: p.name.clone(),
                    number: p.number.clone(),
                    offset: p.offset,
                    pos: p.offset.add(at),
                    kind: p.kind,
                })
                .collect(),
            properties: Vec::new(),
            mirror_x: false,
        };
        comp.recompute_pins();
        self.next_component = self.next_component.saturating_add(1);
        let id = comp.id;
        self.push_component(comp);
        Ok(id)
    }

    pub(crate) fn push_component(&mut self, comp: Component) {
        self.components.push(comp);
    }

    pub fn remove_component(&mut self, id: CompId) -> Result<Component, EditError> {
        let idx = self
            .components()
            .iter()
            .position(|c| c.id == id)
            .ok_or(EditError::UnknownComponent(id.0))?;
        Ok(self.components.remove(idx))
    }

    pub fn move_component(&mut self, id: CompId, at: Point) -> Result<(), EditError> {
        if !at.is_valid() {
            return Err(EditError::InvalidCoordinate(at));
        }
        let c = self
            .component_mut(id)
            .ok_or(EditError::UnknownComponent(id.0))?;
        c.at = at;
        c.recompute_pins();
        Ok(())
    }

    pub fn rotate_component(&mut self, id: CompId, delta: u16) -> Result<u16, EditError> {
        let c = self
            .component_mut(id)
            .ok_or(EditError::UnknownComponent(id.0))?;
        c.rotation = (c.rotation + delta) % 360;
        c.rotation -= c.rotation % 90;
        c.recompute_pins();
        Ok(c.rotation)
    }

    pub fn mirror_component(&mut self, id: CompId) -> Result<(), EditError> {
        let c = self
            .component_mut(id)
            .ok_or(EditError::UnknownComponent(id.0))?;
        c.mirror_x = !c.mirror_x;
        c.recompute_pins();
        Ok(())
    }

    pub fn set_value(&mut self, id: CompId, value: &str) -> Result<(), EditError> {
        if value.len() > limits::MAX_NAME {
            return Err(EditError::BadInput("value"));
        }
        let c = self
            .component_mut(id)
            .ok_or(EditError::UnknownComponent(id.0))?;
        c.value = value.to_string();
        Ok(())
    }

    pub fn set_property(&mut self, id: CompId, key: &str, value: &str) -> Result<(), EditError> {
        if key.is_empty() || key.len() > 64 || value.len() > limits::MAX_NAME {
            return Err(EditError::BadInput("property"));
        }
        let c = self
            .component_mut(id)
            .ok_or(EditError::UnknownComponent(id.0))?;
        if let Some(slot) = c.properties.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value.to_string();
        } else {
            if c.properties.len() >= limits::MAX_COMPONENT_PROPERTIES {
                return Err(EditError::Limit("properties"));
            }
            c.properties.push((key.to_string(), value.to_string()));
        }
        Ok(())
    }

    pub fn next_refdes(&self, prefix: &str) -> String {
        for n in 1..=99_999u32 {
            let candidate = format!("{}{}", prefix, n);
            if !self.refdes_exists(&candidate) {
                return candidate;
            }
        }
        format!("{}X", prefix)
    }

    pub fn add_wire(&mut self, points: Vec<Point>) -> Result<WireId, EditError> {
        if self.wires().len() >= limits::MAX_WIRES {
            return Err(EditError::Limit("wires"));
        }
        if points.len() < 2 {
            return Err(EditError::BadInput("wire needs two points"));
        }
        if points.len() > limits::MAX_WIRE_POINTS {
            return Err(EditError::Limit("wire points"));
        }
        for p in &points {
            if !p.is_valid() {
                return Err(EditError::InvalidCoordinate(*p));
            }
        }
        let mut clean: Vec<Point> = Vec::with_capacity(points.len());
        for p in points {
            if clean.last() != Some(&p) {
                clean.push(p);
            }
        }
        if clean.len() < 2 {
            return Err(EditError::BadInput("zero length wire"));
        }
        let id = WireId(self.next_wire);
        self.next_wire = self.next_wire.saturating_add(1);
        self.wires.push(Wire {
            id,
            points: clean,
        });
        Ok(id)
    }

    pub fn remove_wire(&mut self, id: WireId) -> Result<Wire, EditError> {
        let idx = self
            .wires()
            .iter()
            .position(|w| w.id == id)
            .ok_or(EditError::UnknownWire(id.0))?;
        Ok(self.wires.remove(idx))
    }

    pub fn add_junction(&mut self, at: Point) -> Result<JunctionId, EditError> {
        if !at.is_valid() {
            return Err(EditError::InvalidCoordinate(at));
        }
        if let Some(j) = self.junctions().iter().find(|j| j.at == at) {
            return Ok(j.id);
        }
        if self.junctions().len() >= limits::MAX_WIRES {
            return Err(EditError::Limit("junctions"));
        }
        let id = JunctionId(self.next_junction);
        self.next_junction = self.next_junction.saturating_add(1);
        self.junctions.push(Junction { id, at });
        Ok(id)
    }

    pub fn remove_junction(&mut self, id: JunctionId) -> Result<(), EditError> {
        let idx = self
            .junctions()
            .iter()
            .position(|j| j.id == id)
            .ok_or(EditError::BadInput("unknown junction"))?;
        self.junctions.remove(idx);
        Ok(())
    }

    pub fn add_label(
        &mut self,
        name: &str,
        at: Point,
        kind: LabelKind,
    ) -> Result<LabelId, EditError> {
        let name = name.trim();
        if name.is_empty() || name.len() > limits::MAX_NAME {
            return Err(EditError::BadInput("label name"));
        }
        if !at.is_valid() {
            return Err(EditError::InvalidCoordinate(at));
        }
        if self.labels().len() >= limits::MAX_LABELS {
            return Err(EditError::Limit("labels"));
        }
        if let Some(l) = self
            .labels()
            .iter()
            .find(|l| l.at == at && l.name == name && l.kind == kind)
        {
            return Ok(l.id);
        }
        let id = LabelId(self.next_label);
        self.next_label = self.next_label.saturating_add(1);
        self.labels.push(Label {
            id,
            at,
            name: name.to_string(),
            kind,
            rotation: 0,
        });
        Ok(id)
    }

    pub fn remove_label(&mut self, id: LabelId) -> Result<(), EditError> {
        let idx = self
            .labels()
            .iter()
            .position(|l| l.id == id)
            .ok_or(EditError::UnknownLabel(id.0))?;
        self.labels.remove(idx);
        Ok(())
    }

    pub fn add_bus(&mut self, name: &str, members: &[&str]) -> Result<BusId, EditError> {
        let name = name.trim();
        if name.is_empty() || name.len() > limits::MAX_NAME {
            return Err(EditError::BadInput("bus name"));
        }
        if members.len() > limits::MAX_BUS_MEMBERS {
            return Err(EditError::Limit("bus members"));
        }
        if self.buses().len() >= limits::MAX_BUSES {
            return Err(EditError::Limit("buses"));
        }
        if self.buses().iter().any(|b| b.name == name) {
            return Err(EditError::BadInput("duplicate bus"));
        }
        for m in members {
            if m.trim().is_empty() || m.len() > limits::MAX_NAME {
                return Err(EditError::BadInput("bus member"));
            }
        }
        let id = BusId(self.next_bus);
        self.next_bus = self.next_bus.saturating_add(1);
        self.buses.push(Bus {
            id,
            name: name.to_string(),
            members: members.iter().map(|s| s.trim().to_string()).collect(),
            points: Vec::new(),
        });
        Ok(id)
    }

    pub fn remove_bus(&mut self, id: BusId) -> Result<(), EditError> {
        let idx = self
            .buses()
            .iter()
            .position(|b| b.id == id)
            .ok_or(EditError::UnknownBus(id.0))?;
        self.buses.remove(idx);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.components.clear();
        self.wires.clear();
        self.junctions.clear();
        self.labels.clear();
        self.buses.clear();
    }

    /// Nearest pin within `radius` of `p`, if any.
    pub fn pin_near(&self, p: Point, radius: i64) -> Option<(CompId, usize)> {
        let r2 = (radius as i128) * (radius as i128);
        let mut best: Option<(i128, CompId, usize)> = None;
        for c in self.components() {
            for (i, pin) in c.pins.iter().enumerate() {
                let d = pin.pos.dist2(p);
                if d <= r2 && best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                    best = Some((d, c.id, i));
                }
            }
        }
        best.map(|(_, id, i)| (id, i))
    }

    /// Component whose bounding box contains `p`, preferring the smallest.
    pub fn component_at(&self, p: Point) -> Option<CompId> {
        self.components()
            .iter()
            .filter(|c| c.bounds().contains(p))
            .min_by_key(|c| (c.bounds().width() as i128) * (c.bounds().height() as i128))
            .map(|c| c.id)
    }
}

/// Refdes prefix for a part: R, C, L, D, Q for switches, U for logic, J for
/// connectors, TP for test points/sources, and a sanitised fallback.
pub fn part_prefix(part: &crate::library::Part) -> &'static str {
    match part.name.as_str() {
        "R" => "R",
        "C" => "C",
        "L" => "L",
        "D" | "LED" => "D",
        "SW" => "SW",
        "GND" | "VCC" | "V" | "I" => "TP",
        "AND2" | "OR2" | "XOR2" | "NAND2" | "NOR2" | "NOT" | "DFF" => "U",
        _ if part.category == "connector" => "J",
        _ if part.category == "logic" => "U",
        _ => "X",
    }
}
