//! Studio API: JSON views, schematic editing, simulation, board generation, DRC,
//! autoroute jobs and export endpoints.
//!
//! The API runs against a single in-memory project. It never touches the
//! filesystem; the launcher's batch mode handles writing files.

use crate::http::{Request, Response};
use crate::json::Value;
use crate::project::Project;
use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_core::netlist::{Diagnostic, Netlist, Severity};
use anvil_core::schematic::{Component, LabelKind, Schematic};
use anvil_core::sim::{self, CompileOptions, Integration, SimOptions};
use anvil_export as export;
use anvil_pcb::board::{Board, Layer, Pad};
use anvil_pcb::drc::{self, DrcKind, DrcReport, Violation};
use anvil_pcb::generate::{board_from_netlist, BuildOptions};
use anvil_pcb::router::{self, RouteProgress, RouteReport, RouterOptions};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

pub const VERSION: &str = anvil_core::VERSION;

#[derive(Clone, Debug, Default)]
pub struct JobEvent {
    pub sequence: u64,
    pub phase: String,
    pub net: Option<usize>,
    pub net_name: String,
    pub routed: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Default)]
pub struct RouteJob {
    pub running: bool,
    pub finished: bool,
    pub error: Option<String>,
    pub attempt: usize,
    pub phase: String,
    pub net: Option<usize>,
    pub net_name: String,
    pub routed: usize,
    pub total: usize,
    pub sequence: u64,
    pub events: Vec<JobEvent>,
    pub report: Option<RouteReport>,
}

pub struct StudioState {
    pub lib: Library,
    pub project: Project,
    pub board: Option<Board>,
    pub drc: Option<DrcReport>,
    pub last_route: Option<RouteReport>,
    pub job: RouteJob,
    pub examples: Vec<String>,
    pub boot_ms: u128,
    pub log: Vec<String>,
}

impl StudioState {
    pub fn new() -> StudioState {
        let lib = Library::standard();
        let examples = Project::examples(&lib)
            .map(|v| v.into_iter().map(|p| p.name).collect())
            .unwrap_or_default();
        let boot_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let mut st = StudioState {
            project: Project::new("Untitled"),
            lib,
            board: None,
            drc: None,
            last_route: None,
            job: RouteJob::default(),
            examples,
            boot_ms,
            log: Vec::new(),
        };
        if let Ok(p) = Project::demo(&st.lib) {
            st.project = p;
            if let Err(e) = rebuild_board(&mut st) {
                st.log.push(format!("demo board: {e}"));
            }
        }
        st
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    pub fn board_summary(&self) -> Value {
        let mut v = Value::object();
        match &self.board {
            Some(b) => {
                v.set("present", Value::bool(true));
                v.set("pads", Value::int(b.pad_count() as i64));
                v.set("traces", Value::int(b.traces.len() as i64));
                v.set("vias", Value::int(b.vias.len() as i64));
                v.set("nets", Value::int(b.nets.len() as i64));
                let bd = b.bounds();
                v.set("size_mm", Value::arr(vec![
                    Value::num(bd.width() as f64 / 1e6),
                    Value::num(bd.height() as f64 / 1e6),
                ]));
                let drc = self.drc.as_ref();
                v.set("errors", Value::int(drc.map(|d| d.errors()).unwrap_or(0) as i64));
                v.set("unrouted", Value::int(drc.map(|d| d.unrouted).unwrap_or(0) as i64));
                v.set("clean", Value::bool(drc.map(|d| d.is_clean()).unwrap_or(false)));
            }
            None => {
                v.set("present", Value::bool(false));
            }
        }
        v
    }
}

pub fn lock(state: &Arc<Mutex<StudioState>>) -> MutexGuard<'_, StudioState> {
    match state.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

fn rebuild_board(st: &mut StudioState) -> Result<(), String> {
    let nl = st.project.netlist(&st.lib);
    if nl.errors() > 0 {
        st.board = None;
        st.drc = None;
        return Err(format!("netlist has {} error(s)", nl.errors()));
    }
    let opts = BuildOptions::default();
    let board = board_from_netlist(&st.project.schematic, &nl, &st.lib, &opts)?;
    let report = drc::check(&board);
    st.board = Some(board);
    st.drc = Some(report);
    st.last_route = None;
    st.job = RouteJob::default();
    Ok(())
}

fn invalidate_board(st: &mut StudioState) {
    st.board = None;
    st.drc = None;
    st.last_route = None;
    st.job = RouteJob::default();
}

#[derive(Clone, Debug)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl ApiError {
    fn new(status: u16, msg: impl Into<String>) -> Self {
        ApiError {
            status,
            message: msg.into(),
        }
    }
    fn bad(msg: impl Into<String>) -> Self {
        Self::new(400, msg)
    }
    fn not_found(msg: impl Into<String>) -> Self {
        Self::new(404, msg)
    }
    fn conflict(msg: impl Into<String>) -> Self {
        Self::new(409, msg)
    }
}

impl From<crate::http::HttpError> for ApiError {
    fn from(e: crate::http::HttpError) -> ApiError {
        ApiError::new(e.status, e.message)
    }
}

impl From<ApiError> for Response {
    fn from(e: ApiError) -> Response {
        Response::error(e.status, e.message)
    }
}

type ApiResult = Result<Response, ApiError>;

fn field<'a>(v: &'a Value, key: &str) -> Result<&'a Value, ApiError> {
    v.get(key)
        .ok_or_else(|| ApiError::bad(format!("missing field {key}")))
}

fn str_field<'a>(v: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    field(v, key)?
        .as_str()
        .ok_or_else(|| ApiError::bad(format!("field {key} must be a string")))
}

fn int_field(v: &Value, key: &str, min: i64, max: i64) -> Result<i64, ApiError> {
    let n = field(v, key)?
        .as_i64()
        .ok_or_else(|| ApiError::bad(format!("field {key} must be an integer")))?;
    if n < min || n > max {
        return Err(ApiError::bad(format!("field {key} out of range")));
    }
    Ok(n)
}

fn f64_field(v: &Value, key: &str) -> Result<Option<f64>, ApiError> {
    match v.get(key) {
        None => Ok(None),
        Some(x) => {
            let n = x
                .as_f64()
                .ok_or_else(|| ApiError::bad(format!("field {key} must be a number")))?;
            if !n.is_finite() {
                return Err(ApiError::bad(format!("field {key} is not finite")));
            }
            Ok(Some(n))
        }
    }
}

fn point_field(v: &Value, key: &str) -> Result<Point, ApiError> {
    let a = field(v, key)?
        .as_array()
        .ok_or_else(|| ApiError::bad(format!("field {key} must be [x,y]")))?;
    if a.len() != 2 {
        return Err(ApiError::bad("point needs two coordinates"));
    }
    let x = a[0]
        .as_i64()
        .ok_or_else(|| ApiError::bad("bad x coordinate"))?;
    let y = a[1]
        .as_i64()
        .ok_or_else(|| ApiError::bad("bad y coordinate"))?;
    let p = Point::new(x, y);
    if !p.is_valid() {
        return Err(ApiError::bad("coordinate out of range"));
    }
    Ok(p)
}

fn severity_str(s: Severity) -> &'static str {
    s.as_str()
}

pub fn point_json(p: Point) -> Value {
    Value::arr(vec![Value::int(p.x), Value::int(p.y)])
}

fn diagnostics_json(diags: &[Diagnostic]) -> Value {
    Value::arr(
        diags
            .iter()
            .map(|d| {
                let mut o = Value::object();
                o.set("severity", Value::str(severity_str(d.severity)));
                o.set("code", Value::str(d.code));
                o.set("message", Value::str(d.message.clone()));
                o.set("subject", Value::str(d.subject.clone()));
                o
            })
            .collect(),
    )
}

fn netlist_json(nl: &Netlist) -> Value {
    let mut v = Value::object();
    v.set("errors", Value::int(nl.errors() as i64));
    v.set("warnings", Value::int(nl.warnings() as i64));
    v.set("pins", Value::int(nl.total_pins() as i64));
    let nets: Vec<Value> = nl
        .nets
        .iter()
        .map(|n| {
            let mut o = Value::object();
            o.set("id", Value::int(n.id as i64));
            o.set("name", Value::str(n.name.clone()));
            o.set("power", Value::bool(n.power));
            o.set(
                "nodes",
                Value::arr(
                    n.nodes
                        .iter()
                        .map(|node| {
                            let mut x = Value::object();
                            x.set("refdes", Value::str(node.refdes.clone()));
                            x.set("pin", Value::str(node.pin.clone()));
                            x.set("pin_name", Value::str(node.pin_name.clone()));
                            x
                        })
                        .collect(),
                ),
            );
            o
        })
        .collect();
    v.set("nets", Value::arr(nets));
    v.set("diagnostics", diagnostics_json(&nl.diagnostics));
    v
}

fn component_json(c: &Component) -> Value {
    let mut o = Value::object();
    o.set("id", Value::int(c.id.0 as i64));
    o.set("refdes", Value::str(c.refdes.clone()));
    o.set("value", Value::str(c.value.clone()));
    o.set("symbol", Value::str(c.symbol.clone()));
    o.set("footprint", Value::str(c.footprint.clone()));
    o.set("at", point_json(c.at));
    o.set("rotation", Value::int(c.rotation as i64));
    o.set("mirror", Value::bool(c.mirror_x));
    let pins: Vec<Value> = c
        .pins
        .iter()
        .map(|p| {
            let mut x = Value::object();
            x.set("name", Value::str(p.name.clone()));
            x.set("number", Value::str(p.number.clone()));
            x.set("kind", Value::str(p.kind.as_str()));
            x.set("at", point_json(p.pos));
            x.set("offset", point_json(p.offset));
            x
        })
        .collect();
    o.set("pins", Value::arr(pins));
    let mut props = Value::object();
    for (k, val) in &c.properties {
        props.set(k, Value::str(val.clone()));
    }
    o.set("properties", props);
    o
}

fn schematic_json(s: &Schematic) -> Value {
    let mut v = Value::object();
    let mut sheet = Value::object();
    sheet.set("title", Value::str(s.sheet.title.clone()));
    sheet.set("company", Value::str(s.sheet.company.clone()));
    sheet.set("revision", Value::str(s.sheet.revision.clone()));
    sheet.set("date", Value::str(s.sheet.date.clone()));
    v.set("sheet", sheet);
    v.set(
        "components",
        Value::arr(s.components().iter().map(component_json).collect()),
    );
    v.set(
        "wires",
        Value::arr(
            s.wires()
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
                .collect(),
        ),
    );
    v.set(
        "junctions",
        Value::arr(
            s.junctions()
                .iter()
                .map(|j| {
                    let mut o = Value::object();
                    o.set("id", Value::int(j.id.0 as i64));
                    o.set("at", point_json(j.at));
                    o
                })
                .collect(),
        ),
    );
    v.set(
        "labels",
        Value::arr(
            s.labels()
                .iter()
                .map(|l| {
                    let mut o = Value::object();
                    o.set("id", Value::int(l.id.0 as i64));
                    o.set("name", Value::str(l.name.clone()));
                    o.set("kind", Value::str(l.kind.as_str()));
                    o.set("at", point_json(l.at));
                    o
                })
                .collect(),
        ),
    );
    v.set(
        "buses",
        Value::arr(
            s.buses()
                .iter()
                .map(|b| {
                    let mut o = Value::object();
                    o.set("id", Value::int(b.id.0 as i64));
                    o.set("name", Value::str(b.name.clone()));
                    o.set(
                        "members",
                        Value::arr(b.members.iter().map(|m| Value::str(m.clone())).collect()),
                    );
                    o
                })
                .collect(),
        ),
    );
    let b = s.bounds();
    v.set(
        "bounds",
        Value::arr(vec![
            Value::int(b.min.x),
            Value::int(b.min.y),
            Value::int(b.max.x),
            Value::int(b.max.y),
        ]),
    );
    v
}

fn rules_json(b: &Board) -> Value {
    let mut o = Value::object();
    o.set("min_trace", Value::int(b.rules.min_trace));
    o.set("min_clearance", Value::int(b.rules.min_clearance));
    o.set("min_drill", Value::int(b.rules.min_drill));
    o.set("min_annular", Value::int(b.rules.min_annular));
    o.set("edge_clearance", Value::int(b.rules.edge_clearance));
    o.set("via_drill", Value::int(b.rules.via_drill));
    o.set("via_diameter", Value::int(b.rules.via_diameter));
    o
}

fn pad_json(p: &Pad, net_name: &str) -> Value {
    let mut o = Value::object();
    o.set("number", Value::str(p.number.clone()));
    o.set("at", point_json(p.at));
    o.set("size", Value::arr(vec![Value::int(p.size.0), Value::int(p.size.1)]));
    o.set("shape", Value::str(p.shape.as_str()));
    o.set("drill", Value::int(p.drill));
    o.set("layers", Value::str(p.layers.as_str()));
    o.set("net", p.net.map(|n| Value::int(n as i64)).unwrap_or(Value::Null));
    o.set("net_name", Value::str(net_name));
    o
}

pub fn board_json(b: &Board, report: Option<&DrcReport>) -> Value {
    let bd = b.bounds();
    let mut v = Value::object();
    v.set("name", Value::str(b.name.clone()));
    v.set(
        "outline",
        Value::arr(b.outline.iter().map(|p| point_json(*p)).collect()),
    );
    v.set("layers", Value::int(b.copper_layers as i64));
    v.set("rules", rules_json(b));
    v.set(
        "nets",
        Value::arr(
            b.nets
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let mut o = Value::object();
                    o.set("id", Value::int(i as i64));
                    o.set("name", Value::str(n.clone()));
                    o
                })
                .collect(),
        ),
    );
    let footprints: Vec<Value> = b
        .footprints
        .iter()
        .map(|f| {
            let mut o = Value::object();
            o.set("refdes", Value::str(f.refdes.clone()));
            o.set("name", Value::str(f.name.clone()));
            o.set("at", point_json(f.at));
            o.set("rotation", Value::int(f.rotation as i64));
            o.set(
                "pads",
                Value::arr(
                    f.pads
                        .iter()
                        .map(|p| {
                            let nn = p.net.map(|n| b.net_name(n)).unwrap_or("");
                            pad_json(p, nn)
                        })
                        .collect(),
                ),
            );
            o
        })
        .collect();
    v.set("footprints", Value::arr(footprints));
    v.set(
        "traces",
        Value::arr(
            b.traces
                .iter()
                .map(|t| {
                    let mut o = Value::object();
                    o.set("net", Value::int(t.net as i64));
                    o.set("net_name", Value::str(b.net_name(t.net).to_string()));
                    o.set("layer", Value::str(t.layer.as_str()));
                    o.set("width", Value::int(t.width));
                    o.set(
                        "points",
                        Value::arr(t.points.iter().map(|p| point_json(*p)).collect()),
                    );
                    o
                })
                .collect(),
        ),
    );
    v.set(
        "vias",
        Value::arr(
            b.vias
                .iter()
                .map(|vi| {
                    let mut o = Value::object();
                    o.set("net", Value::int(vi.net as i64));
                    o.set("net_name", Value::str(b.net_name(vi.net).to_string()));
                    o.set("at", point_json(vi.at));
                    o.set("drill", Value::int(vi.drill));
                    o.set("diameter", Value::int(vi.diameter));
                    o.set("from", Value::str(vi.from.as_str()));
                    o.set("to", Value::str(vi.to.as_str()));
                    o
                })
                .collect(),
        ),
    );
    let rats = anvil_pcb::connect::ratsnest(b);
    v.set(
        "ratsnest",
        Value::arr(
            rats.iter()
                .map(|e| {
                    let mut o = Value::object();
                    o.set("net", Value::int(e.net as i64));
                    o.set("net_name", Value::str(e.net_name.clone()));
                    o.set("a", point_json(e.a));
                    o.set("b", point_json(e.b));
                    o.set("a_label", Value::str(e.a_label.clone()));
                    o.set("b_label", Value::str(e.b_label.clone()));
                    o
                })
                .collect(),
        ),
    );
    v.set(
        "bounds",
        Value::arr(vec![
            Value::int(bd.min.x),
            Value::int(bd.min.y),
            Value::int(bd.max.x),
            Value::int(bd.max.y),
        ]),
    );
    if let Some(d) = report {
        v.set("drc", drc_summary_json(d));
    }
    v
}

fn violation_json(v: &Violation, board: Option<&Board>) -> Value {
    let mut o = Value::object();
    o.set("kind", Value::str(v.kind.as_str()));
    o.set("severity", Value::str(v.severity.as_str()));
    o.set("layer", v.layer.map(|l| Value::str(l.as_str())).unwrap_or(Value::Null));
    o.set("at", point_json(v.at));
    o.set("message", Value::str(v.message.clone()));
    o.set("net_a", v.net_a.map(|n| Value::int(n as i64)).unwrap_or(Value::Null));
    o.set("net_b", v.net_b.map(|n| Value::int(n as i64)).unwrap_or(Value::Null));
    if let Some(b) = board {
        if let Some(n) = v.net_a {
            o.set("net_a_name", Value::str(b.net_name(n).to_string()));
        }
        if let Some(n) = v.net_b {
            o.set("net_b_name", Value::str(b.net_name(n).to_string()));
        }
    }
    o.set(
        "refs",
        Value::arr(v.refs.iter().map(|s| Value::str(s.clone())).collect()),
    );
    o
}

pub fn drc_summary_json(d: &DrcReport) -> Value {
    let mut o = Value::object();
    o.set("clean", Value::bool(d.is_clean()));
    o.set("errors", Value::int(d.errors() as i64));
    o.set("warnings", Value::int(d.warnings() as i64));
    o.set("checked", Value::int(d.checked_items as i64));
    o.set("nets", Value::int(d.nets as i64));
    o.set("unrouted", Value::int(d.unrouted as i64));
    let mut counts = Value::object();
    for (k, c) in d.counts() {
        counts.set(kind_key(k), Value::int(c as i64));
    }
    o.set("counts", counts);
    o
}

pub fn drc_json(d: &DrcReport, board: Option<&Board>) -> Value {
    let mut o = drc_summary_json(d);
    o.set(
        "violations",
        Value::arr(
            d.violations
                .iter()
                .take(500)
                .map(|v| violation_json(v, board))
                .collect(),
        ),
    );
    o.set("truncated", Value::bool(d.violations.len() > 500));
    o
}

fn kind_key(k: DrcKind) -> &'static str {
    k.as_str()
}

fn route_json(r: &RouteReport, board: Option<&Board>) -> Value {
    let mut o = Value::object();
    o.set("clean", Value::bool(r.clean));
    o.set("nets_total", Value::int(r.nets_total as i64));
    o.set("nets_routed", Value::int(r.nets_routed as i64));
    o.set("edges_total", Value::int(r.edges_total as i64));
    o.set("edges_routed", Value::int(r.edges_routed as i64));
    o.set("attempts", Value::int(r.attempts as i64));
    o.set("ripups", Value::int(r.ripups as i64));
    o.set(
        "failures",
        Value::arr(
            r.failures
                .iter()
                .map(|f| {
                    let mut x = Value::object();
                    x.set("net", Value::int(f.net as i64));
                    x.set("name", Value::str(f.name.clone()));
                    x.set("reason", Value::str(f.reason.clone()));
                    x
                })
                .collect(),
        ),
    );
    o.set("drc", drc_json(&r.drc, board));
    o
}

fn job_json(j: &RouteJob, board: Option<&Board>) -> Value {
    let mut o = Value::object();
    o.set("running", Value::bool(j.running));
    o.set("finished", Value::bool(j.finished));
    o.set("error", j.error.as_ref().map(|s| Value::str(s.clone())).unwrap_or(Value::Null));
    o.set("attempt", Value::int(j.attempt as i64));
    o.set("phase", Value::str(j.phase.clone()));
    o.set("net", j.net.map(|n| Value::int(n as i64)).unwrap_or(Value::Null));
    o.set("net_name", Value::str(j.net_name.clone()));
    o.set("routed", Value::int(j.routed as i64));
    o.set("total", Value::int(j.total as i64));
    o.set("sequence", Value::int(j.sequence as i64));
    o.set(
        "events",
        Value::arr(
            j.events
                .iter()
                .rev()
                .take(30)
                .rev()
                .map(|e| {
                    let mut x = Value::object();
                    x.set("sequence", Value::int(e.sequence as i64));
                    x.set("phase", Value::str(e.phase.clone()));
                    x.set("net_name", Value::str(e.net_name.clone()));
                    x.set("routed", Value::int(e.routed as i64));
                    x.set("total", Value::int(e.total as i64));
                    x
                })
                .collect(),
        ),
    );
    if let Some(r) = &j.report {
        o.set("report", route_json(r, board));
    }
    o
}

fn sheet_json(s: &Schematic) -> Value {
    let mut o = Value::object();
    o.set("title", Value::str(s.sheet.title.clone()));
    o.set("company", Value::str(s.sheet.company.clone()));
    o.set("revision", Value::str(s.sheet.revision.clone()));
    o.set("date", Value::str(s.sheet.date.clone()));
    o
}

pub fn state_json(st: &StudioState) -> Value {
    let nl = st.project.netlist(&st.lib);
    let mut v = Value::object();
    v.set("name", Value::str(st.project.name.clone()));
    v.set("version", Value::str(VERSION));
    v.set("sheet", sheet_json(&st.project.schematic));
    v.set("schematic", schematic_json(&st.project.schematic));
    v.set("netlist", netlist_json(&nl));
    v.set("board", st.board_summary());
    v.set(
        "drc",
        st.drc
            .as_ref()
            .map(drc_summary_json)
            .unwrap_or(Value::Null),
    );
    v.set(
        "route",
        st.last_route
            .as_ref()
            .map(|r| route_json(r, st.board.as_ref()))
            .unwrap_or(Value::Null),
    );
    v.set("job", job_json(&st.job, st.board.as_ref()));
    v.set(
        "examples",
        Value::arr(st.examples.iter().map(|s| Value::str(s.clone())).collect()),
    );
    v.set("log", Value::arr(st.log.iter().map(|s| Value::str(s.clone())).collect()));
    v
}

fn library_json(lib: &Library) -> Value {
    let mut v = Value::object();
    let parts: Vec<Value> = lib
        .parts()
        .iter()
        .map(|p| {
            let mut o = Value::object();
            o.set("name", Value::str(p.name.clone()));
            o.set("symbol", Value::str(p.symbol.clone()));
            o.set("footprint", Value::str(p.footprint.clone()));
            o.set("value", Value::str(p.value.clone()));
            o.set("category", Value::str(p.category.clone()));
            o.set("description", Value::str(p.description.clone()));
            o
        })
        .collect();
    v.set("parts", Value::arr(parts));
    let symbols: Vec<Value> = lib
        .symbols()
        .iter()
        .map(|s| {
            let mut o = Value::object();
            o.set("name", Value::str(s.name.clone()));
            o.set("description", Value::str(s.description.clone()));
            o.set("body", Value::arr(vec![Value::int(s.body.0), Value::int(s.body.1)]));
            let pins: Vec<Value> = s
                .pins
                .iter()
                .map(|p| {
                    let mut x = Value::object();
                    x.set("name", Value::str(p.name.clone()));
                    x.set("number", Value::str(p.number.clone()));
                    x.set("at", point_json(p.offset));
                    x.set("direction", Value::int(p.direction as i64));
                    x.set("length", Value::int(p.length));
                    x.set("kind", Value::str(p.kind.as_str()));
                    x
                })
                .collect();
            o.set("pins", Value::arr(pins));
            let graphics: Vec<Value> = s
                .graphics
                .iter()
                .map(|g| match g {
                    anvil_core::library::Graphic::Line { a, b } => {
                        let mut x = Value::object();
                        x.set("type", Value::str("line"));
                        x.set("a", point_json(*a));
                        x.set("b", point_json(*b));
                        x
                    }
                    anvil_core::library::Graphic::Rect { min, max } => {
                        let mut x = Value::object();
                        x.set("type", Value::str("rect"));
                        x.set("a", point_json(*min));
                        x.set("b", point_json(*max));
                        x
                    }
                    anvil_core::library::Graphic::Circle { center, radius } => {
                        let mut x = Value::object();
                        x.set("type", Value::str("circle"));
                        x.set("at", point_json(*center));
                        x.set("r", Value::int(*radius));
                        x
                    }
                    anvil_core::library::Graphic::Polyline { points } => {
                        let mut x = Value::object();
                        x.set("type", Value::str("poly"));
                        x.set(
                            "points",
                            Value::arr(points.iter().map(|p| point_json(*p)).collect()),
                        );
                        x
                    }
                    anvil_core::library::Graphic::Text { at, text, size } => {
                        let mut x = Value::object();
                        x.set("type", Value::str("text"));
                        x.set("at", point_json(*at));
                        x.set("text", Value::str(text.clone()));
                        x.set("size", Value::int(*size));
                        x
                    }
                })
                .collect();
            o.set("graphics", Value::arr(graphics));
            o
        })
        .collect();
    v.set("symbols", Value::arr(symbols));
    let fps: Vec<Value> = lib
        .footprints()
        .iter()
        .map(|f| {
            let mut o = Value::object();
            o.set("name", Value::str(f.name.clone()));
            o.set("description", Value::str(f.description.clone()));
            o.set("courtyard", Value::arr(vec![Value::int(f.courtyard.0), Value::int(f.courtyard.1)]));
            o.set("pads", Value::int(f.pads.len() as i64));
            o
        })
        .collect();
    v.set("footprints", Value::arr(fps));
    v
}

fn simulation_json(
    mode: &str,
    dc: Option<&sim::DcResult>,
    tran: Option<&sim::TranResult>,
    diagnostics: &[Diagnostic],
    method: Integration,
) -> Value {
    let mut v = Value::object();
    v.set("mode", Value::str(mode));
    if let Some(dc) = dc {
        let nodes: Vec<Value> = dc
            .node_names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let mut o = Value::object();
                o.set("name", Value::str(name.clone()));
                o.set("voltage", Value::num(dc.voltages.get(i).copied().unwrap_or(0.0)));
                o
            })
            .collect();
        v.set("nodes", Value::arr(nodes));
        v.set("iterations", Value::int(dc.iterations as i64));
    }
    if let Some(tr) = tran {
        let target = 3000usize;
        let stride = (tr.times.len() / target).max(1);
        let idx: Vec<usize> = (0..tr.times.len()).step_by(stride).collect();
        let times: Vec<Value> = idx
            .iter()
            .map(|&i| Value::num(tr.times[i] * 1000.0))
            .collect();
        v.set("time_ms", Value::arr(times));
        let colors = [
            "#7ee787", "#79c0ff", "#ffa657", "#ff7b72", "#d2a8ff", "#56d4dd", "#f2cc60", "#ff9bce",
        ];
        let series: Vec<Value> = tr
            .node_names
            .iter()
            .enumerate()
            .map(|(si, name)| {
                let mut o = Value::object();
                o.set("name", Value::str(name.clone()));
                o.set("color", Value::str(colors[si % colors.len()]));
                o.set(
                    "values",
                    Value::arr(
                        idx.iter()
                            .map(|&i| Value::num(tr.series[si].get(i).copied().unwrap_or(0.0)))
                            .collect(),
                    ),
                );
                o
            })
            .collect();
        v.set("series", Value::arr(series));
        v.set("steps", Value::int(tr.steps as i64));
        v.set("truncated", Value::bool(tr.truncated));
        v.set("method", Value::str(method.as_str()));
        v.set("samples", Value::int(idx.len() as i64));
    }
    v.set("diagnostics", diagnostics_json(diagnostics));
    v
}

fn find_comp(s: &Schematic, id: i64) -> Option<anvil_core::schematic::CompId> {
    if id <= 0 || id > u32::MAX as i64 {
        return None;
    }
    s.components()
        .iter()
        .find(|c| c.id.0 as i64 == id)
        .map(|c| c.id)
}

fn api_simulate(st: &StudioState, body: &Value) -> ApiResult {
    let mode = body.get("mode").and_then(|m| m.as_str()).unwrap_or("tran");
    let nl = st.project.netlist(&st.lib);
    if nl.errors() > 0 {
        let mut v = Value::object();
        v.set("error", Value::str("netlist has errors"));
        v.set("diagnostics", diagnostics_json(&nl.diagnostics));
        return Ok(Response::json_status(400, &v));
    }
    let circuit = match sim::compile_schematic(
        &st.project.schematic,
        &nl,
        &st.lib,
        &CompileOptions::default(),
    ) {
        Ok(c) => c,
        Err(e) => {
            let mut v = Value::object();
            v.set("error", Value::str(e.to_string()));
            v.set("diagnostics", diagnostics_json(&nl.diagnostics));
            return Ok(Response::json_status(400, &v));
        }
    };
    if mode == "dc" {
        return match sim::dc_op(&circuit) {
            Ok(dc) => Ok(Response::json(&simulation_json(
                "dc",
                Some(&dc),
                None,
                &nl.diagnostics,
                Integration::BackwardEuler,
            ))),
            Err(e) => Err(ApiError::bad(format!("simulation failed: {e}"))),
        };
    }
    let tstep = f64_field(body, "tstep")?.unwrap_or(1e-6);
    let tstop = f64_field(body, "tstop")?.unwrap_or(2e-3);
    if !(tstep >= 1e-9 && tstep <= 0.1) {
        return Err(ApiError::bad("tstep must be between 1 ns and 100 ms"));
    }
    if !(tstop > 0.0 && tstop <= 0.2) {
        return Err(ApiError::bad("tstop must be between 0 and 200 ms"));
    }
    if tstop / tstep > 200_000.0 {
        return Err(ApiError::bad("too many time steps; increase tstep"));
    }
    let method = match body
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("trap")
    {
        "be" => Integration::BackwardEuler,
        "trap" => Integration::Trapezoidal,
        _ => return Err(ApiError::bad("method must be be or trap")),
    };
    let opts = SimOptions {
        tstep,
        tstop,
        method,
        max_steps: 200_000,
        record_nodes: Vec::new(),
        reltol: 1e-6,
        abstol: 1e-9,
    };
    match sim::transient(&circuit, &opts) {
        Ok(tr) => Ok(Response::json(&simulation_json(
            "tran",
            None,
            Some(&tr),
            &nl.diagnostics,
            method,
        ))),
        Err(e) => Err(ApiError::bad(format!("simulation failed: {e}"))),
    }
}

fn new_board(st: &mut StudioState) -> Result<(), ApiError> {
    let nl = st.project.netlist(&st.lib);
    if nl.errors() > 0 {
        return Err(ApiError::bad(format!(
            "netlist has {} error(s); fix the schematic first",
            nl.errors()
        )));
    }
    let board = board_from_netlist(&st.project.schematic, &nl, &st.lib, &BuildOptions::default())
        .map_err(ApiError::bad)?;
    let report = drc::check(&board);
    st.board = Some(board);
    st.drc = Some(report);
    st.last_route = None;
    st.job = RouteJob::default();
    Ok(())
}

fn post_edit(st: &mut StudioState, req: &Request) -> ApiResult {
    let body = req.json_or_empty()?;
    let mut changed = true;
    match req.path.as_str() {
        "/api/schematic/place" => {
            let part = str_field(&body, "part")?;
            let at = point_field(&body, "at")?;
            let refdes = body.get("refdes").and_then(|v| v.as_str());
            let id = match refdes {
                Some(r) => st
                    .project
                    .schematic
                    .add_component_with_refdes(&st.lib, part, at, r),
                None => st.project.schematic.add_component(&st.lib, part, at),
            }
            .map_err(|e| ApiError::bad(e.to_string()))?;
            st.log(format!("placed {} at {},{}", part, at.x, at.y));
            invalidate_board(st);
            let mut out = Value::object();
            out.set("id", Value::int(id.0 as i64));
            out.set("state", state_json(st));
            return Ok(Response::json(&out));
        }
        "/api/schematic/move" => {
            let id = int_field(&body, "id", 1, u32::MAX as i64)?;
            let at = point_field(&body, "at")?;
            let cid = find_comp(&st.project.schematic, id)
                .ok_or_else(|| ApiError::not_found("component"))?;
            st.project
                .schematic
                .move_component(cid, at)
                .map_err(|e| ApiError::bad(e.to_string()))?;
        }
        "/api/schematic/rotate" => {
            let id = int_field(&body, "id", 1, u32::MAX as i64)?;
            let delta = int_field(&body, "delta", -270, 270)?;
            let cid = find_comp(&st.project.schematic, id)
                .ok_or_else(|| ApiError::not_found("component"))?;
            st.project
                .schematic
                .rotate_component(cid, ((delta % 360 + 360) % 360) as u16)
                .map_err(|e| ApiError::bad(e.to_string()))?;
        }
        "/api/schematic/mirror" => {
            let id = int_field(&body, "id", 1, u32::MAX as i64)?;
            let cid = find_comp(&st.project.schematic, id)
                .ok_or_else(|| ApiError::not_found("component"))?;
            st.project
                .schematic
                .mirror_component(cid)
                .map_err(|e| ApiError::bad(e.to_string()))?;
        }
        "/api/schematic/delete" => {
            let kind = body.get("kind").and_then(|v| v.as_str()).unwrap_or("component");
            let id = int_field(&body, "id", 1, u32::MAX as i64)?;
            match kind {
                "component" => {
                    let cid = find_comp(&st.project.schematic, id)
                        .ok_or_else(|| ApiError::not_found("component"))?;
                    st.project
                        .schematic
                        .remove_component(cid)
                        .map_err(|e| ApiError::bad(e.to_string()))?;
                }
                "wire" => {
                    st.project
                        .schematic
                        .remove_wire(anvil_core::schematic::WireId(id as u32))
                        .map_err(|e| ApiError::bad(e.to_string()))?;
                }
                "label" => {
                    st.project
                        .schematic
                        .remove_label(anvil_core::schematic::LabelId(id as u32))
                        .map_err(|e| ApiError::bad(e.to_string()))?;
                }
                _ => return Err(ApiError::bad("unknown delete kind")),
            }
        }
        "/api/schematic/wire" => {
            let pts = field(&body, "points")?
                .as_array()
                .ok_or_else(|| ApiError::bad("points must be an array"))?;
            if pts.len() > 2000 {
                return Err(ApiError::bad("too many wire points"));
            }
            let mut points = Vec::with_capacity(pts.len());
            for p in pts {
                let a = p
                    .as_array()
                    .ok_or_else(|| ApiError::bad("wire point must be [x,y]"))?;
                if a.len() != 2 {
                    return Err(ApiError::bad("wire point needs two coordinates"));
                }
                let x = a[0].as_i64().ok_or_else(|| ApiError::bad("bad x"))?;
                let y = a[1].as_i64().ok_or_else(|| ApiError::bad("bad y"))?;
                let pt = Point::new(x, y);
                if !pt.is_valid() {
                    return Err(ApiError::bad("wire point out of range"));
                }
                points.push(pt);
            }
            st.project
                .schematic
                .add_wire(points)
                .map_err(|e| ApiError::bad(e.to_string()))?;
        }
        "/api/schematic/label" => {
            let name = str_field(&body, "name")?;
            let at = point_field(&body, "at")?;
            let kind = body
                .get("kind")
                .and_then(|v| v.as_str())
                .and_then(LabelKind::parse)
                .unwrap_or(LabelKind::Local);
            st.project
                .schematic
                .add_label(name, at, kind)
                .map_err(|e| ApiError::bad(e.to_string()))?;
        }
        "/api/schematic/junction" => {
            let at = point_field(&body, "at")?;
            st.project
                .schematic
                .add_junction(at)
                .map_err(|e| ApiError::bad(e.to_string()))?;
        }
        "/api/schematic/value" => {
            let id = int_field(&body, "id", 1, u32::MAX as i64)?;
            let value = str_field(&body, "value")?;
            let cid = find_comp(&st.project.schematic, id)
                .ok_or_else(|| ApiError::not_found("component"))?;
            st.project
                .schematic
                .set_value(cid, value)
                .map_err(|e| ApiError::bad(e.to_string()))?;
        }
        "/api/schematic/property" => {
            let id = int_field(&body, "id", 1, u32::MAX as i64)?;
            let key = str_field(&body, "key")?;
            let value = str_field(&body, "value")?;
            let cid = find_comp(&st.project.schematic, id)
                .ok_or_else(|| ApiError::not_found("component"))?;
            st.project
                .schematic
                .set_property(cid, key, value)
                .map_err(|e| ApiError::bad(e.to_string()))?;
        }
        "/api/project/reset" => {
            st.project = Project::new("Untitled");
            invalidate_board(st);
            st.log("new project");
        }
        "/api/project/load" => {
            let name = str_field(&body, "name")?;
            let lib = st.lib.clone();
            let projects = Project::examples(&lib).map_err(ApiError::bad)?;
            let p = projects
                .into_iter()
                .find(|p| p.name == name)
                .ok_or_else(|| ApiError::not_found("example"))?;
            st.project = p;
            st.log(format!("loaded example {name}"));
            let _ = new_board(st);
        }
        "/api/project/demo" => {
            let p = Project::demo(&st.lib).map_err(ApiError::bad)?;
            st.project = p;
            invalidate_board(st);
            let _ = new_board(st);
            st.log("loaded demo");
        }
        "/api/board/generate" => {
            new_board(st)?;
            st.log("generated board from schematic");
        }
        "/api/board/clear-routes" => {
            if let Some(b) = &mut st.board {
                b.clear_routes();
                let report = drc::check(b);
                st.drc = Some(report);
                st.last_route = None;
                st.job = RouteJob::default();
            }
        }
        _ => {
            changed = false;
        }
    }
    if !changed {
        return Err(ApiError::not_found("unknown endpoint"));
    }
    if st.board.is_some()
        && !matches!(
            req.path.as_str(),
            "/api/board/generate"
                | "/api/board/clear-routes"
                | "/api/route/start"
                | "/api/route/run-sync"
                | "/api/project/load"
                | "/api/project/demo"
        )
    {
        invalidate_board(st);
    }
    st.log(format!("{} ok", req.path));
    Ok(Response::json(&state_json(st)))
}

fn preview_response(st: &StudioState, layer: &str) -> ApiResult {
    let board = st
        .board
        .as_ref()
        .ok_or_else(|| ApiError::not_found("board is not generated"))?;
    let png = match layer {
        "bottom" => export::preview_bottom_png(board, 1200),
        _ => export::preview_png(board, 1200),
    };
    match png {
        Ok(bytes) => Ok(Response::bytes("image/png", bytes)),
        Err(export::ExportError::Empty) => Err(ApiError::not_found("layer has no copper")),
        Err(e) => Err(ApiError::bad(e.to_string())),
    }
}

/// Spawn the autoroute thread. Called by the API with the shared state.
pub fn spawn_route(state: Arc<Mutex<StudioState>>) -> Result<(), ApiError> {
    let (board, opts) = {
        let mut st = lock(&state);
        if st.job.running {
            return Err(ApiError::conflict("autoroute already running"));
        }
        let board = match st.board.clone() {
            Some(b) => b,
            None => {
                new_board(&mut st)?;
                st.board.clone().ok_or_else(|| ApiError::bad("cannot generate board"))?
            }
        };
        let opts = RouterOptions::for_board(&board);
        st.job = RouteJob {
            running: true,
            phase: "start".to_string(),
            sequence: 1,
            ..RouteJob::default()
        };
        st.log("autoroute started");
        (board, opts)
    };
    let thread_state = state.clone();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let cb_state = thread_state.clone();
            let mut progress = move |p: &RouteProgress| {
                let mut st = lock(&cb_state);
                st.job.running = true;
                st.job.attempt = p.attempt;
                st.job.phase = p.phase.to_string();
                st.job.net = p.net;
                st.job.net_name = p.net_name.clone();
                st.job.routed = p.routed;
                st.job.total = p.total;
                let seq = st.job.sequence.wrapping_add(1);
                st.job.sequence = seq;
                st.job.events.push(JobEvent {
                    sequence: seq,
                    phase: p.phase.to_string(),
                    net: p.net,
                    net_name: p.net_name.clone(),
                    routed: p.routed,
                    total: p.total,
                });
                if st.job.events.len() > 300 {
                    st.job.events.remove(0);
                }
            };
            let mut b = board;
            let report = router::route_all(&mut b, &opts, &mut progress);
            (b, report)
        }));
        let mut st = lock(&thread_state);
        st.job.running = false;
        st.job.finished = true;
        st.job.sequence = st.job.sequence.wrapping_add(1);
        match result {
            Ok((b, Ok(report))) => {
                st.log(format!(
                    "autoroute finished: {}/{} nets, {} DRC errors",
                    report.nets_routed,
                    report.nets_total,
                    report.drc.errors()
                ));
                st.drc = Some(report.drc.clone());
                st.last_route = Some(report.clone());
                st.job.report = Some(report);
                st.board = Some(b);
                st.job.phase = "done".to_string();
            }
            Ok((b, Err(e))) => {
                st.log(format!("autoroute failed: {e}"));
                st.job.error = Some(e.to_string());
                st.job.phase = "error".to_string();
                st.board = Some(b);
            }
            Err(_) => {
                st.log("autoroute panicked (caught)");
                st.job.error = Some("autoroute panicked".to_string());
                st.job.phase = "error".to_string();
            }
        }
    });
    Ok(())
}

fn run_route_sync(st: &mut StudioState) {
    let board = match st.board.take() {
        Some(b) => b,
        None => return,
    };
    let opts = RouterOptions::for_board(&board);
    let mut progress = |_p: &RouteProgress| {};
    let mut b = board;
    match router::route_all(&mut b, &opts, &mut progress) {
        Ok(report) => {
            st.log(format!(
                "autoroute: {}/{} nets, clean={}",
                report.nets_routed, report.nets_total, report.clean
            ));
            st.drc = Some(report.drc.clone());
            st.last_route = Some(report);
            st.board = Some(b);
        }
        Err(e) => {
            st.log(format!("autoroute failed: {e}"));
            st.board = Some(b);
        }
    }
    st.job = RouteJob {
        running: false,
        finished: true,
        phase: "done".into(),
        sequence: 1,
        ..RouteJob::default()
    };
}

fn get_api(st: &Arc<Mutex<StudioState>>, req: &Request) -> ApiResult {
    match req.path.as_str() {
        "/api/health" => {
            let mut v = Value::object();
            v.set("ok", Value::bool(true));
            v.set("version", Value::str(VERSION));
            Ok(Response::json(&v))
        }
        "/api/state" => {
            let st = lock(st);
            Ok(Response::json(&state_json(&st)))
        }
        "/api/library" => {
            let st = lock(st);
            Ok(Response::json(&library_json(&st.lib)))
        }
        "/api/board" => {
            let st = lock(st);
            match &st.board {
                Some(b) => Ok(Response::json(&board_json(b, st.drc.as_ref()))),
                None => Err(ApiError::not_found("board is not generated")),
            }
        }
        "/api/project/json" => {
            let st = lock(st);
            Ok(Response::json(&st.project.to_json()))
        }
        "/api/netlist" => {
            let st = lock(st);
            let nl = st.project.netlist(&st.lib);
            Ok(Response::json(&netlist_json(&nl)))
        }
        "/api/drc" => {
            let st = lock(st);
            match &st.board {
                Some(b) => {
                    let r = drc::check(b);
                    Ok(Response::json(&drc_json(&r, Some(b))))
                }
                None => Err(ApiError::not_found("board is not generated")),
            }
        }
        "/api/route/state" => {
            let st = lock(st);
            Ok(Response::json(&job_json(&st.job, st.board.as_ref())))
        }
        "/api/route/report" => {
            let st = lock(st);
            match &st.last_route {
                Some(r) => Ok(Response::json(&route_json(r, st.board.as_ref()))),
                None => Err(ApiError::not_found("no autoroute has run")),
            }
        }
        "/api/examples" => {
            let st = lock(st);
            let mut names = Vec::new();
            let mut details = Vec::new();
            for n in &st.examples {
                names.push(Value::str(n.clone()));
                let mut o = Value::object();
                o.set("name", Value::str(n.clone()));
                details.push(o);
            }
            let mut v = Value::object();
            v.set("names", Value::arr(names));
            v.set("examples", Value::arr(details));
            Ok(Response::json(&v))
        }
        "/api/export/manifest" => {
            let st = lock(st);
            let b = st
                .board
                .as_ref()
                .ok_or_else(|| ApiError::not_found("board is not generated"))?;
            let files = export::export_board(b);
            let mut v = Value::object();
            v.set(
                "files",
                Value::arr(
                    files
                        .iter()
                        .map(|f| {
                            let mut o = Value::object();
                            o.set("name", Value::str(f.name.clone()));
                            o.set("bytes", Value::int(f.bytes.len() as i64));
                            o
                        })
                        .collect(),
                ),
            );
            Ok(Response::json(&v))
        }
        "/api/export/preview.png" => {
            let layer = req.query("layer").unwrap_or("top");
            let st = lock(st);
            preview_response(&st, layer)
        }
        "/api/export/gerber" => {
            let layer = req.query("layer").unwrap_or("top");
            let st = lock(st);
            let b = st
                .board
                .as_ref()
                .ok_or_else(|| ApiError::not_found("board is not generated"))?;
            let (name, text) = match layer {
                "top" => ("top", export::layer_text(b, Layer::Top)),
                "bottom" => ("bottom", export::layer_text(b, Layer::Bottom)),
                "outline" => ("outline", export::outline_text(b)),
                "silk" => ("silk", export::silkscreen_text(b)),
                _ => return Err(ApiError::bad("layer must be top, bottom, outline or silk")),
            };
            let base = export::file_base(b);
            Ok(Response::text(200, text).with_header(
                "Content-Disposition",
                &format!("attachment; filename=\"{}-{}.gbr\"", base, name),
            ))
        }
        "/api/export/drill" => {
            let st = lock(st);
            let b = st
                .board
                .as_ref()
                .ok_or_else(|| ApiError::not_found("board is not generated"))?;
            let base = export::file_base(b);
            Ok(Response::text(200, export::drill_text(b)).with_header(
                "Content-Disposition",
                &format!("attachment; filename=\"{}.drl\"", base),
            ))
        }
        _ => Err(ApiError::not_found("unknown endpoint")),
    }
}

fn post_api(st: &Arc<Mutex<StudioState>>, req: &Request) -> ApiResult {
    match req.path.as_str() {
        "/api/simulate" => {
            let st = lock(st);
            let body = req.json_or_empty()?;
            api_simulate(&st, &body)
        }
        "/api/route/start" => {
            {
                let mut s = lock(st);
                if s.job.running {
                    return Err(ApiError::conflict("autoroute already running"));
                }
                if s.board.is_none() {
                    new_board(&mut s)?;
                }
            }
            spawn_route(st.clone())?;
            let s = lock(st);
            Ok(Response::json_status(202, &job_json(&s.job, s.board.as_ref())))
        }
        "/api/route/run-sync" => {
            let mut s = lock(st);
            if s.job.running {
                return Err(ApiError::conflict("autoroute already running"));
            }
            if s.board.is_none() {
                new_board(&mut s)?;
            }
            run_route_sync(&mut s);
            Ok(Response::json(&state_json(&s)))
        }
        "/api/board/generate" | "/api/project/demo" | "/api/project/load" | "/api/project/reset"
        | "/api/board/clear-routes" | "/api/schematic/place" | "/api/schematic/move"
        | "/api/schematic/rotate" | "/api/schematic/mirror" | "/api/schematic/delete"
        | "/api/schematic/wire" | "/api/schematic/label" | "/api/schematic/junction"
        | "/api/schematic/value" | "/api/schematic/property" => {
            let mut s = lock(st);
            post_edit(&mut s, req)
        }
        _ => Err(ApiError::not_found("unknown endpoint")),
    }
}

/// Dispatch an authenticated API request.
pub fn dispatch(state: &Arc<Mutex<StudioState>>, req: &Request) -> Response {
    let result = match req.method.as_str() {
        "GET" => get_api(state, req),
        "POST" => post_api(state, req),
        _ => Err(ApiError::new(405, "method not allowed")),
    };
    match result {
        Ok(r) => r,
        Err(e) => Response::from(e),
    }
}

/// Snapshot for an SSE event.
pub fn job_snapshot(state: &Arc<Mutex<StudioState>>) -> (u64, Value, bool) {
    let st = lock(state);
    (
        st.job.sequence,
        job_json(&st.job, st.board.as_ref()),
        st.job.finished || !st.job.running,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_has_demo_board() {
        let st = StudioState::new();
        assert!(!st.project.schematic.components().is_empty());
        assert!(st.board.is_some());
        let j = state_json(&st);
        assert_eq!(j.get("name").unwrap().as_str(), Some("Anvil demo"));
    }

    #[test]
    fn demo_board_routes_clean() {
        let mut st = StudioState::new();
        let mut board = st.board.take().unwrap();
        let opts = RouterOptions::for_board(&board);
        let mut progress = |_p: &RouteProgress| {};
        let report = router::route_all(&mut board, &opts, &mut progress).unwrap();
        assert!(
            report.clean,
            "failures={:?} violations={:?}",
            report.failures,
            report.drc.violations.iter().take(5).collect::<Vec<_>>()
        );
        st.board = Some(board);
    }

    #[test]
    fn board_json_has_geometry() {
        let st = StudioState::new();
        let b = st.board.unwrap();
        let j = board_json(&b, st.drc.as_ref());
        assert!(!j.get("footprints").unwrap().as_array().unwrap().is_empty());
        assert!(j.get("outline").unwrap().as_array().unwrap().len() >= 4);
    }
}
