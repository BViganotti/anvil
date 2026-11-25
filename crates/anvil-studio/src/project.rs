//! Studio project model: a schematic plus helpers to build the demo and the
//! shipped examples, and JSON (de)serialization for `.anvl` project files.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_core::netlist::{self, Netlist};
use anvil_core::schematic::{CompId, LabelKind, Schematic};
use super::json::Value;

pub const MM: i64 = 1_000_000;

#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    pub schematic: Schematic,
}

impl Project {
    pub fn new(name: &str) -> Self {
        Project {
            name: name.to_string(),
            schematic: Schematic::new(),
        }
    }

    pub fn netlist(&self, lib: &Library) -> Netlist {
        netlist::extract(&self.schematic, lib)
    }

    pub fn diagnostics(&self, lib: &Library) -> Vec<anvil_core::netlist::Diagnostic> {
        self.netlist(lib).diagnostics
    }

    pub fn to_json(&self) -> Value {
        let mut root = Value::object();
        root.set("format", Value::str("anvil-project-1"));
        root.set("name", Value::str(self.name.clone()));
        let mut sheet = Value::object();
        sheet.set("title", Value::str(self.schematic.sheet.title.clone()));
        sheet.set("company", Value::str(self.schematic.sheet.company.clone()));
        sheet.set("revision", Value::str(self.schematic.sheet.revision.clone()));
        sheet.set("date", Value::str(self.schematic.sheet.date.clone()));
        root.set("sheet", sheet);
        let components: Vec<Value> = self
            .schematic
            .components()
            .iter()
            .map(|c| {
                let mut o = Value::object();
                o.set("id", Value::int(c.id.0 as i64));
                o.set("refdes", Value::str(c.refdes.clone()));
                o.set("part", Value::str(c.symbol.clone()));
                o.set("value", Value::str(c.value.clone()));
                o.set("footprint", Value::str(c.footprint.clone()));
                o.set("at", point_json(c.at));
                o.set("rotation", Value::int(c.rotation as i64));
                o.set("mirror", Value::bool(c.mirror_x));
                let mut props = Value::object();
                for (k, v) in &c.properties {
                    props.set(k, Value::str(v.clone()));
                }
                o.set("properties", props);
                Value::Object(o.as_object().cloned().unwrap_or_default())
            })
            .collect();
        root.set("components", Value::arr(components));
        let wires: Vec<Value> = self
            .schematic
            .wires()
            .iter()
            .map(|w| {
                let mut o = Value::object();
                o.set("id", Value::int(w.id.0 as i64));
                o.set(
                    "points",
                    Value::arr(w.points.iter().map(|p| point_json(*p)).collect()),
                );
                o
            })
            .collect();
        root.set("wires", Value::arr(wires));
        let junctions: Vec<Value> = self
            .schematic
            .junctions()
            .iter()
            .map(|j| {
                let mut o = Value::object();
                o.set("id", Value::int(j.id.0 as i64));
                o.set("at", point_json(j.at));
                o
            })
            .collect();
        root.set("junctions", Value::arr(junctions));
        let labels: Vec<Value> = self
            .schematic
            .labels()
            .iter()
            .map(|l| {
                let mut o = Value::object();
                o.set("id", Value::int(l.id.0 as i64));
                o.set("name", Value::str(l.name.clone()));
                o.set("kind", Value::str(l.kind.as_str()));
                o.set("at", point_json(l.at));
                o.set("rotation", Value::int(l.rotation as i64));
                o
            })
            .collect();
        root.set("labels", Value::arr(labels));
        let buses: Vec<Value> = self
            .schematic
            .buses()
            .iter()
            .map(|b| {
                let mut o = Value::object();
                o.set("id", Value::int(b.id.0 as i64));
                o.set("name", Value::str(b.name.clone()));
                o.set(
                    "members",
                    Value::arr(b.members.iter().map(|m| Value::str(m.clone())).collect()),
                );
                o.set(
                    "points",
                    Value::arr(b.points.iter().map(|p| point_json(*p)).collect()),
                );
                o
            })
            .collect();
        root.set("buses", Value::arr(buses));
        root
    }

    pub fn from_json(lib: &Library, v: &Value) -> Result<Project, String> {
        let name = v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("Imported project")
            .to_string();
        if name.len() > 128 {
            return Err("project name too long".to_string());
        }
        if let Some(fmt) = v.get("format").and_then(|x| x.as_str()) {
            if fmt != "anvil-project-1" {
                return Err(format!("unsupported project format {fmt}"));
            }
        }
        let mut p = Project::new(&name);
        if let Some(sheet) = v.get("sheet") {
            if let Some(s) = sheet.get("title").and_then(|x| x.as_str()) {
                p.schematic.sheet.title = truncate(s, 128);
            }
            if let Some(s) = sheet.get("company").and_then(|x| x.as_str()) {
                p.schematic.sheet.company = truncate(s, 128);
            }
            if let Some(s) = sheet.get("revision").and_then(|x| x.as_str()) {
                p.schematic.sheet.revision = truncate(s, 32);
            }
            if let Some(s) = sheet.get("date").and_then(|x| x.as_str()) {
                p.schematic.sheet.date = truncate(s, 64);
            }
        }
        let comps = v
            .get("components")
            .and_then(|x| x.as_array())
            .ok_or_else(|| "missing components array".to_string())?;
        if comps.len() > 5000 {
            return Err("too many components".to_string());
        }
        for c in comps {
            let part = c
                .get("part")
                .and_then(|x| x.as_str())
                .ok_or_else(|| "component missing part".to_string())?;
            let refdes = c
                .get("refdes")
                .and_then(|x| x.as_str())
                .ok_or_else(|| "component missing refdes".to_string())?;
            let at = parse_point(c.get("at")).ok_or_else(|| "component bad position".to_string())?;
            let id = p
                .schematic
                .add_component_with_refdes(lib, part, at, refdes)
                .map_err(|e| e.to_string())?;
            if let Some(val) = c.get("value").and_then(|x| x.as_str()) {
                p.schematic.set_value(id, val).map_err(|e| e.to_string())?;
            }
            if let Some(rot) = c.get("rotation").and_then(|x| x.as_i64()) {
                let rot = (((rot % 360) + 360) % 360) as u16;
                p.schematic.rotate_component(id, rot).map_err(|e| e.to_string())?;
            }
            if c.get("mirror").and_then(|x| x.as_bool()).unwrap_or(false) {
                p.schematic.mirror_component(id).map_err(|e| e.to_string())?;
            }
            if let Some(props) = c.get("properties").and_then(|x| x.as_object()) {
                for (k, val) in props {
                    if let Some(s) = val.as_str() {
                        p.schematic
                            .set_property(id, k, s)
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        if let Some(wires) = v.get("wires").and_then(|x| x.as_array()) {
            for w in wires {
                let pts = w
                    .get("points")
                    .and_then(|x| x.as_array())
                    .ok_or_else(|| "wire missing points".to_string())?;
                let mut points = Vec::with_capacity(pts.len());
                for pt in pts {
                    points.push(parse_point(Some(pt)).ok_or_else(|| "bad wire point".to_string())?);
                }
                p.schematic.add_wire(points).map_err(|e| e.to_string())?;
            }
        }
        if let Some(junctions) = v.get("junctions").and_then(|x| x.as_array()) {
            for j in junctions {
                if let Some(jp) = parse_point(j.get("at")) {
                    p.schematic.add_junction(jp).map_err(|e| e.to_string())?;
                }
            }
        }
        if let Some(labels) = v.get("labels").and_then(|x| x.as_array()) {
            for l in labels {
                let name = l
                    .get("name")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "label missing name".to_string())?;
                let at = parse_point(l.get("at")).ok_or_else(|| "bad label point".to_string())?;
                let kind = l
                    .get("kind")
                    .and_then(|x| x.as_str())
                    .and_then(LabelKind::parse)
                    .unwrap_or(LabelKind::Local);
                p.schematic
                    .add_label(name, at, kind)
                    .map_err(|e| e.to_string())?;
            }
        }
        if let Some(buses) = v.get("buses").and_then(|x| x.as_array()) {
            for b in buses {
                let name = b
                    .get("name")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "bus missing name".to_string())?;
                let members: Vec<&str> = b
                    .get("members")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|m| m.as_str()).collect())
                    .unwrap_or_default();
                p.schematic.add_bus(name, &members).map_err(|e| e.to_string())?;
            }
        }
        Ok(p)
    }

    /// The built-in demo: a biased RC node driven by a pulse, plus an LED
    /// branch. Small enough to autoroute cleanly, rich enough to show DC,
    /// transient, DRC, routing and export.
    pub fn demo(lib: &Library) -> Result<Project, String> {
        let mut p = Project::new("Anvil demo");
        p.schematic.sheet.title = "RC divider + LED demo".to_string();
        p.schematic.sheet.revision = "A".to_string();
        add(&mut p, lib, "V", "V1", 12.7, 33.0)?;
        add(&mut p, lib, "R", "R1", 30.5, 38.0)?;
        add(&mut p, lib, "R", "R2", 30.5, 27.9)?;
        add(&mut p, lib, "C", "C1", 45.7, 33.0)?;
        add(&mut p, lib, "R", "R3", 30.5, 17.8)?;
        add(&mut p, lib, "D", "D1", 45.7, 22.9)?;
        add(&mut p, lib, "GND", "GND1", 12.7, 22.9)?;
        p.schematic
            .set_property(comp_id(&p, "V1").ok_or("V1 missing")?, "wave", "pulse")
            .map_err(|e| e.to_string())?;
        for (k, v) in [
            ("v1", "0"),
            ("v2", "5"),
            ("delay", "0"),
            ("rise", "1u"),
            ("width", "500u"),
            ("period", "1m"),
        ] {
            p.schematic
                .set_property(comp_id(&p, "V1").ok_or("V1 missing")?, k, v)
                .map_err(|e| e.to_string())?;
        }
        p.schematic
            .set_value(comp_id(&p, "R1").ok_or("R1 missing")?, "1k")
            .map_err(|e| e.to_string())?;
        p.schematic
            .set_value(comp_id(&p, "R2").ok_or("R2 missing")?, "2k")
            .map_err(|e| e.to_string())?;
        p.schematic
            .set_value(comp_id(&p, "C1").ok_or("C1 missing")?, "100n")
            .map_err(|e| e.to_string())?;
        p.schematic
            .set_value(comp_id(&p, "R3").ok_or("R3 missing")?, "330")
            .map_err(|e| e.to_string())?;
        let pairs = [
            ("V1", "+", "VIN"),
            ("V1", "-", "GND"),
            ("R1", "1", "VIN"),
            ("R1", "2", "MID"),
            ("R2", "1", "MID"),
            ("R2", "2", "GND"),
            ("C1", "1", "MID"),
            ("C1", "2", "GND"),
            ("R3", "1", "VIN"),
            ("R3", "2", "NLED"),
            ("D1", "A", "NLED"),
            ("D1", "K", "GND"),
            ("GND1", "1", "GND"),
        ];
        label_pins(&mut p, &pairs)?;
        // A few wires make the schematic read like a drawing while labels keep
        // the netlist unambiguous.
        pin_wire(&mut p, "V1", "+", "R1", "1")?;
        pin_wire(&mut p, "R1", "2", "R2", "1")?;
        pin_wire(&mut p, "C1", "1", "R2", "1")?;
        pin_wire(&mut p, "R3", "2", "D1", "A")?;
        Ok(p)
    }

    /// Eight shipped examples. Returned in a stable order.
    pub fn examples(lib: &Library) -> Result<Vec<Project>, String> {
        let mut out = Vec::new();
        out.push(Self::demo(lib)?);
        out.push(divider(lib)?);
        out.push(rc(lib)?);
        out.push(halfwave(lib)?);
        out.push(led(lib)?);
        out.push(wheatstone(lib)?);
        out.push(rlc(lib)?);
        out.push(logic(lib)?);
        out.push(rectifier(lib)?);
        Ok(out)
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        let mut t = s[..n].to_string();
        while !t.is_char_boundary(t.len()) {
            t.pop();
        }
        t
    }
}

fn point_json(p: Point) -> Value {
    Value::arr(vec![Value::int(p.x), Value::int(p.y)])
}

fn parse_point(v: Option<&Value>) -> Option<Point> {
    let a = v?.as_array()?;
    if a.len() != 2 {
        return None;
    }
    let x = a[0].as_i64()?;
    let y = a[1].as_i64()?;
    let p = Point::new(x, y);
    if p.is_valid() {
        Some(p)
    } else {
        None
    }
}

fn comp_id(p: &Project, refdes: &str) -> Option<CompId> {
    p.schematic
        .components()
        .iter()
        .find(|c| c.refdes == refdes)
        .map(|c| c.id)
}

fn add(
    p: &mut Project,
    lib: &Library,
    part: &str,
    refdes: &str,
    x_mm: f64,
    y_mm: f64,
) -> Result<CompId, String> {
    let at = Point::new((x_mm * MM as f64).round() as i64, (y_mm * MM as f64).round() as i64);
    p.schematic
        .add_component_with_refdes(lib, part, at, refdes)
        .map_err(|e| format!("{refdes}: {e}"))
}

fn label_pins(p: &mut Project, pairs: &[(&str, &str, &str)]) -> Result<(), String> {
    for (refdes, pin_name, net) in pairs {
        let id = comp_id(p, refdes).ok_or_else(|| format!("missing {refdes}"))?;
        let c = p.schematic.component(id).ok_or("component vanished")?;
        let pin = c
            .pin_by_name(pin_name)
            .ok_or_else(|| format!("{refdes} has no pin {pin_name}"))?;
        let at = pin.pos;
        let kind = if *net == "GND" || *net == "VCC" {
            LabelKind::Power
        } else {
            LabelKind::Local
        };
        p.schematic
            .add_label(net, at, kind)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn pin_wire(p: &mut Project, ra: &str, pa: &str, rb: &str, pb: &str) -> Result<(), String> {
    let ia = comp_id(p, ra).ok_or_else(|| format!("missing {ra}"))?;
    let ib = comp_id(p, rb).ok_or_else(|| format!("missing {rb}"))?;
    let a = p
        .schematic
        .component(ia)
        .and_then(|c| c.pin_by_name(pa))
        .map(|pin| pin.pos)
        .ok_or_else(|| format!("{ra}.{pa} missing"))?;
    let b = p
        .schematic
        .component(ib)
        .and_then(|c| c.pin_by_name(pb))
        .map(|pin| pin.pos)
        .ok_or_else(|| format!("{rb}.{pb} missing"))?;
    if a == b {
        return Ok(());
    }
    p.schematic.add_wire(vec![a, b]).map_err(|e| e.to_string())?;
    Ok(())
}

fn divider(lib: &Library) -> Result<Project, String> {
    let mut p = Project::new("Voltage divider");
    p.schematic.sheet.title = "10 V divider".to_string();
    add(&mut p, lib, "V", "V1", 15.0, 30.0)?;
    add(&mut p, lib, "R", "R1", 35.0, 35.0)?;
    add(&mut p, lib, "R", "R2", 35.0, 25.0)?;
    add(&mut p, lib, "GND", "GND1", 15.0, 20.0)?;
    p.schematic
        .set_value(comp_id(&p, "V1").unwrap(), "10")
        .map_err(|e| e.to_string())?;
    p.schematic
        .set_value(comp_id(&p, "R1").unwrap(), "1k")
        .map_err(|e| e.to_string())?;
    p.schematic
        .set_value(comp_id(&p, "R2").unwrap(), "3k")
        .map_err(|e| e.to_string())?;
    label_pins(
        &mut p,
        &[
            ("V1", "+", "VIN"),
            ("V1", "-", "GND"),
            ("R1", "1", "VIN"),
            ("R1", "2", "VOUT"),
            ("R2", "1", "VOUT"),
            ("R2", "2", "GND"),
            ("GND1", "1", "GND"),
        ],
    )?;
    pin_wire(&mut p, "R1", "2", "R2", "1")?;
    Ok(p)
}

fn rc(lib: &Library) -> Result<Project, String> {
    let mut p = Project::new("RC step response");
    p.schematic.sheet.title = "RC low-pass step".to_string();
    add(&mut p, lib, "V", "V1", 15.0, 30.0)?;
    add(&mut p, lib, "R", "R1", 32.0, 33.0)?;
    add(&mut p, lib, "C", "C1", 47.0, 28.0)?;
    add(&mut p, lib, "GND", "GND1", 15.0, 20.0)?;
    let v = comp_id(&p, "V1").unwrap();
    p.schematic.set_property(v, "wave", "pulse").map_err(|e| e.to_string())?;
    for (k, val) in [("v1", "0"), ("v2", "5"), ("rise", "1u"), ("width", "2m"), ("period", "4m")] {
        p.schematic.set_property(v, k, val).map_err(|e| e.to_string())?;
    }
    label_pins(
        &mut p,
        &[
            ("V1", "+", "IN"),
            ("V1", "-", "GND"),
            ("R1", "1", "IN"),
            ("R1", "2", "OUT"),
            ("C1", "1", "OUT"),
            ("C1", "2", "GND"),
            ("GND1", "1", "GND"),
        ],
    )?;
    Ok(p)
}

fn halfwave(lib: &Library) -> Result<Project, String> {
    let mut p = Project::new("Half-wave rectifier");
    p.schematic.sheet.title = "Half-wave rectifier".to_string();
    add(&mut p, lib, "V", "V1", 15.0, 30.0)?;
    add(&mut p, lib, "D", "D1", 32.0, 33.0)?;
    add(&mut p, lib, "R", "R1", 48.0, 27.0)?;
    add(&mut p, lib, "GND", "GND1", 15.0, 20.0)?;
    let v = comp_id(&p, "V1").unwrap();
    p.schematic.set_property(v, "wave", "sin").map_err(|e| e.to_string())?;
    for (k, val) in [("amp", "10"), ("freq", "1k"), ("offset", "0")] {
        p.schematic.set_property(v, k, val).map_err(|e| e.to_string())?;
    }
    p.schematic.set_value(comp_id(&p, "R1").unwrap(), "1k").map_err(|e| e.to_string())?;
    label_pins(
        &mut p,
        &[
            ("V1", "+", "VIN"),
            ("V1", "-", "GND"),
            ("D1", "A", "VIN"),
            ("D1", "K", "OUT"),
            ("R1", "1", "OUT"),
            ("R1", "2", "GND"),
            ("GND1", "1", "GND"),
        ],
    )?;
    Ok(p)
}

fn led(lib: &Library) -> Result<Project, String> {
    let mut p = Project::new("LED indicator");
    p.schematic.sheet.title = "LED indicator".to_string();
    add(&mut p, lib, "V", "V1", 15.0, 30.0)?;
    add(&mut p, lib, "R", "R1", 32.0, 33.0)?;
    add(&mut p, lib, "LED", "D1", 49.0, 28.0)?;
    add(&mut p, lib, "GND", "GND1", 15.0, 20.0)?;
    p.schematic.set_value(comp_id(&p, "V1").unwrap(), "5").map_err(|e| e.to_string())?;
    p.schematic.set_value(comp_id(&p, "R1").unwrap(), "330").map_err(|e| e.to_string())?;
    label_pins(
        &mut p,
        &[
            ("V1", "+", "VIN"),
            ("V1", "-", "GND"),
            ("R1", "1", "VIN"),
            ("R1", "2", "NLED"),
            ("D1", "A", "NLED"),
            ("D1", "K", "GND"),
            ("GND1", "1", "GND"),
        ],
    )?;
    Ok(p)
}

fn wheatstone(lib: &Library) -> Result<Project, String> {
    let mut p = Project::new("Wheatstone bridge");
    p.schematic.sheet.title = "Wheatstone bridge".to_string();
    add(&mut p, lib, "V", "V1", 15.0, 30.0)?;
    add(&mut p, lib, "R", "R1", 33.0, 40.0)?;
    add(&mut p, lib, "R", "R2", 33.0, 30.0)?;
    add(&mut p, lib, "R", "R3", 50.0, 40.0)?;
    add(&mut p, lib, "R", "R4", 50.0, 30.0)?;
    add(&mut p, lib, "GND", "GND1", 15.0, 20.0)?;
    p.schematic.set_value(comp_id(&p, "V1").unwrap(), "10").map_err(|e| e.to_string())?;
    for (r, val) in [("R1", "1k"), ("R2", "1k"), ("R3", "2k"), ("R4", "1k")] {
        p.schematic
            .set_value(comp_id(&p, r).unwrap(), val)
            .map_err(|e| e.to_string())?;
    }
    label_pins(
        &mut p,
        &[
            ("V1", "+", "VIN"),
            ("V1", "-", "GND"),
            ("R1", "1", "VIN"),
            ("R1", "2", "A"),
            ("R2", "1", "A"),
            ("R2", "2", "GND"),
            ("R3", "1", "VIN"),
            ("R3", "2", "B"),
            ("R4", "1", "B"),
            ("R4", "2", "GND"),
            ("GND1", "1", "GND"),
        ],
    )?;
    Ok(p)
}

fn rlc(lib: &Library) -> Result<Project, String> {
    let mut p = Project::new("RLC ringing");
    p.schematic.sheet.title = "Series RLC".to_string();
    add(&mut p, lib, "V", "V1", 15.0, 30.0)?;
    add(&mut p, lib, "R", "R1", 29.0, 35.0)?;
    add(&mut p, lib, "L", "L1", 43.0, 30.0)?;
    add(&mut p, lib, "C", "C1", 57.0, 26.0)?;
    add(&mut p, lib, "GND", "GND1", 15.0, 20.0)?;
    let v = comp_id(&p, "V1").unwrap();
    p.schematic.set_property(v, "wave", "pulse").map_err(|e| e.to_string())?;
    for (k, val) in [("v1", "0"), ("v2", "5"), ("rise", "100n"), ("width", "2m"), ("period", "4m")] {
        p.schematic.set_property(v, k, val).map_err(|e| e.to_string())?;
    }
    p.schematic.set_value(comp_id(&p, "R1").unwrap(), "10").map_err(|e| e.to_string())?;
    p.schematic.set_value(comp_id(&p, "L1").unwrap(), "1m").map_err(|e| e.to_string())?;
    p.schematic.set_value(comp_id(&p, "C1").unwrap(), "1u").map_err(|e| e.to_string())?;
    label_pins(
        &mut p,
        &[
            ("V1", "+", "IN"),
            ("V1", "-", "GND"),
            ("R1", "1", "IN"),
            ("R1", "2", "N1"),
            ("L1", "1", "N1"),
            ("L1", "2", "N2"),
            ("C1", "1", "N2"),
            ("C1", "2", "GND"),
            ("GND1", "1", "GND"),
        ],
    )?;
    Ok(p)
}

fn logic(lib: &Library) -> Result<Project, String> {
    let mut p = Project::new("Logic gates");
    p.schematic.sheet.title = "Pulse-driven logic".to_string();
    add(&mut p, lib, "V", "V1", 15.0, 35.0)?;
    add(&mut p, lib, "V", "V2", 15.0, 20.0)?;
    add(&mut p, lib, "AND2", "U1", 38.0, 32.0)?;
    add(&mut p, lib, "NOT", "U2", 56.0, 26.0)?;
    add(&mut p, lib, "GND", "GND1", 15.0, 10.0)?;
    for (refdes, period) in [("V1", "1m"), ("V2", "700u")] {
        let v = comp_id(&p, refdes).unwrap();
        p.schematic.set_property(v, "wave", "pulse").map_err(|e| e.to_string())?;
        for (k, val) in [("v1", "0"), ("v2", "5"), ("rise", "10n"), ("width", "300u"), ("period", period)] {
            p.schematic.set_property(v, k, val).map_err(|e| e.to_string())?;
        }
    }
    label_pins(
        &mut p,
        &[
            ("V1", "+", "A"),
            ("V1", "-", "GND"),
            ("V2", "+", "B"),
            ("V2", "-", "GND"),
            ("U1", "A", "A"),
            ("U1", "B", "B"),
            ("U1", "Y", "Y1"),
            ("U2", "A", "Y1"),
            ("U2", "Y", "Y2"),
            ("GND1", "1", "GND"),
        ],
    )?;
    Ok(p)
}

fn rectifier(lib: &Library) -> Result<Project, String> {
    let mut p = Project::new("Full-wave rectifier");
    p.schematic.sheet.title = "Bridge rectifier".to_string();
    add(&mut p, lib, "V", "V1", 15.0, 30.0)?;
    add(&mut p, lib, "D", "D1", 33.0, 40.0)?;
    add(&mut p, lib, "D", "D2", 33.0, 20.0)?;
    add(&mut p, lib, "D", "D3", 50.0, 40.0)?;
    add(&mut p, lib, "D", "D4", 50.0, 20.0)?;
    add(&mut p, lib, "R", "R1", 70.0, 33.0)?;
    add(&mut p, lib, "C", "C1", 85.0, 28.0)?;
    add(&mut p, lib, "GND", "GND1", 15.0, 10.0)?;
    let v = comp_id(&p, "V1").unwrap();
    p.schematic.set_property(v, "wave", "sin").map_err(|e| e.to_string())?;
    for (k, val) in [("amp", "12"), ("freq", "1k"), ("offset", "0")] {
        p.schematic.set_property(v, k, val).map_err(|e| e.to_string())?;
    }
    p.schematic.set_value(comp_id(&p, "R1").unwrap(), "1k").map_err(|e| e.to_string())?;
    p.schematic.set_value(comp_id(&p, "C1").unwrap(), "10u").map_err(|e| e.to_string())?;
    label_pins(
        &mut p,
        &[
            ("V1", "+", "AC1"),
            ("V1", "-", "AC2"),
            ("D1", "A", "AC1"),
            ("D1", "K", "VP"),
            ("D2", "A", "AC2"),
            ("D2", "K", "VP"),
            ("D3", "A", "GND"),
            ("D3", "K", "AC1"),
            ("D4", "A", "GND"),
            ("D4", "K", "AC2"),
            ("R1", "1", "VP"),
            ("R1", "2", "GND"),
            ("C1", "1", "VP"),
            ("C1", "2", "GND"),
            ("GND1", "1", "GND"),
        ],
    )?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_is_valid() {
        let lib = Library::standard();
        let p = Project::demo(&lib).unwrap();
        let nl = p.netlist(&lib);
        assert_eq!(nl.errors(), 0, "{:?}", nl.diagnostics);
        assert!(nl.net_by_name("MID").is_some());
        assert!(nl.net_by_name("GND").is_some());
        assert!(p.schematic.wires().len() >= 4);
    }

    #[test]
    fn demo_compiles_to_simulation() {
        let lib = Library::standard();
        let p = Project::demo(&lib).unwrap();
        let nl = p.netlist(&lib);
        let circuit = anvil_core::sim::compile_schematic(
            &p.schematic,
            &nl,
            &lib,
            &anvil_core::sim::CompileOptions::default(),
        )
        .unwrap();
        assert!(circuit.elements.len() >= 5);
        let dc = anvil_core::sim::dc_op(&circuit).unwrap();
        assert!(dc.voltages.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn examples_are_valid() {
        let lib = Library::standard();
        for p in Project::examples(&lib).unwrap() {
            let nl = p.netlist(&lib);
            assert_eq!(nl.errors(), 0, "{}: {:?}", p.name, nl.diagnostics);
            let circuit = anvil_core::sim::compile_schematic(
                &p.schematic,
                &nl,
                &lib,
                &anvil_core::sim::CompileOptions::default(),
            );
            assert!(circuit.is_ok(), "{}: {:?}", p.name, circuit.err());
        }
    }

    #[test]
    fn project_json_round_trip() {
        let lib = Library::standard();
        let p = Project::demo(&lib).unwrap();
        let j = p.to_json();
        let q = Project::from_json(&lib, &j).unwrap();
        assert_eq!(q.schematic.components().len(), p.schematic.components().len());
        assert_eq!(q.schematic.wires().len(), p.schematic.wires().len());
        assert_eq!(q.schematic.labels().len(), p.schematic.labels().len());
        assert_eq!(q.netlist(&lib).errors(), 0);
    }
}
