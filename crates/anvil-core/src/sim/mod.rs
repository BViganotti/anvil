//! Circuit simulation: modified nodal analysis with a dense LU solver, Newton
//! iteration for diodes, transient integration and an event-driven digital
//! engine.
//!
//! Scope is deliberately documented: linear R/L/C, independent voltage and
//! current sources, Shockley diodes with series resistance, ideal switches,
//! and the digital gates in [`GateKind`] with a Thevenin output driver. This is
//! enough for textbook analog and digital circuits and is tested against
//! independent analytic/reference solutions. It is not a full SPICE.

pub mod compile;
mod lu;

pub use compile::{compile_schematic, CompileOptions};
pub use lu::{Lu, LuError};

use std::fmt;

/// Thermal voltage at 300 K.
pub const VT: f64 = 0.025_852_0;
const GMIN: f64 = 1e-15;

/// A time dependent source waveform.
#[derive(Clone, Debug, PartialEq)]
pub enum Waveform {
    Dc(f64),
    /// Rectangular pulse train with linear rise/fall.
    Pulse {
        v1: f64,
        v2: f64,
        delay: f64,
        rise: f64,
        width: f64,
        period: f64,
    },
    /// Sine wave; `phase` is in degrees.
    Sin {
        offset: f64,
        amp: f64,
        freq: f64,
        delay: f64,
        phase: f64,
    },
    /// Piecewise linear, points sorted by time. Before/after the table the end
    /// values are held.
    Pwl(Vec<(f64, f64)>),
}

impl Waveform {
    pub fn dc(v: f64) -> Self {
        Waveform::Dc(v)
    }

    pub fn value(&self, t: f64) -> f64 {
        let t = if t.is_finite() { t.max(0.0) } else { 0.0 };
        match self {
            Waveform::Dc(v) => *v,
            Waveform::Pulse {
                v1,
                v2,
                delay,
                rise,
                width,
                period,
            } => {
                let t = t - delay;
                if t < 0.0 {
                    return *v1;
                }
                let rise = rise.max(0.0);
                let width = width.max(0.0);
                let period = period.max(0.0);
                let mut tt = t;
                if period > 1e-15 {
                    tt %= period;
                }
                if rise > 1e-15 && tt < rise {
                    return v1 + (v2 - v1) * (tt / rise);
                }
                if tt < rise + width {
                    return *v2;
                }
                if rise > 1e-15 && tt < 2.0 * rise + width {
                    return v2 + (v1 - v2) * ((tt - rise - width) / rise);
                }
                *v1
            }
            Waveform::Sin {
                offset,
                amp,
                freq,
                delay,
                phase,
            } => {
                if t < *delay {
                    *offset
                } else {
                    let w = 2.0 * std::f64::consts::PI * freq * (t - delay)
                        + phase.to_radians();
                    offset + amp * w.sin()
                }
            }
            Waveform::Pwl(points) => {
                if points.is_empty() {
                    return 0.0;
                }
                if t <= points[0].0 {
                    return points[0].1;
                }
                for w in points.windows(2) {
                    if t <= w[1].0 {
                        let span = w[1].0 - w[0].0;
                        if span <= 0.0 {
                            return w[1].1;
                        }
                        let f = (t - w[0].0) / span;
                        return w[0].1 + (w[1].1 - w[0].1) * f;
                    }
                }
                points[points.len() - 1].1
            }
        }
    }

    /// Value used for DC operating point analysis.
    pub fn dc_value(&self) -> f64 {
        match self {
            Waveform::Dc(v) => *v,
            Waveform::Pulse { v1, .. } => *v1,
            Waveform::Sin { offset, .. } => *offset,
            Waveform::Pwl(points) => points.first().map(|p| p.1).unwrap_or(0.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateKind {
    And,
    Or,
    Nand,
    Nor,
    Xor,
    Not,
    Dff,
}

impl GateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GateKind::And => "and",
            GateKind::Or => "or",
            GateKind::Nand => "nand",
            GateKind::Nor => "nor",
            GateKind::Xor => "xor",
            GateKind::Not => "not",
            GateKind::Dff => "dff",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "and" | "and2" => Some(GateKind::And),
            "or" | "or2" => Some(GateKind::Or),
            "nand" | "nand2" => Some(GateKind::Nand),
            "nor" | "nor2" => Some(GateKind::Nor),
            "xor" | "xor2" => Some(GateKind::Xor),
            "not" => Some(GateKind::Not),
            "dff" => Some(GateKind::Dff),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Digital {
    pub gate: GateKind,
    /// Input node indices. For DFF: [D, CLK].
    pub inputs: Vec<usize>,
    /// Output node indices. For DFF: [Q, QN].
    pub outputs: Vec<usize>,
    pub delay: f64,
    pub vdd: f64,
    pub rout: f64,
    pub initial: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    Resistor {
        a: usize,
        b: usize,
        r: f64,
    },
    Capacitor {
        a: usize,
        b: usize,
        c: f64,
        /// Initial voltage across the capacitor for transient analysis.
        ic: Option<f64>,
    },
    Inductor {
        a: usize,
        b: usize,
        l: f64,
        ic: Option<f64>,
    },
    VSource {
        a: usize,
        b: usize,
        wave: Waveform,
    },
    ISource {
        a: usize,
        b: usize,
        wave: Waveform,
    },
    Diode {
        a: usize,
        k: usize,
        is: f64,
        n: f64,
        rs: f64,
    },
    Switch {
        a: usize,
        b: usize,
        ron: f64,
        roff: f64,
        closed: bool,
    },
    Digital(Digital),
}

/// A circuit to simulate. Node 0 is always ground.
#[derive(Clone, Debug, PartialEq)]
pub struct Circuit {
    pub nodes: Vec<String>,
    pub elements: Vec<Element>,
}

impl Default for Circuit {
    fn default() -> Self {
        Circuit::new()
    }
}

impl Circuit {
    pub fn new() -> Self {
        Circuit {
            nodes: vec!["0".to_string()],
            elements: Vec::new(),
        }
    }

    pub fn add_node(&mut self, name: &str) -> usize {
        if let Some(i) = self.nodes.iter().position(|n| n == name) {
            return i;
        }
        self.nodes.push(name.to_string());
        self.nodes.len() - 1
    }

    pub fn node(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n == name)
    }

    pub fn add(&mut self, e: Element) {
        self.elements.push(e);
    }

    pub fn resistor(&mut self, a: usize, b: usize, r: f64) {
        self.elements.push(Element::Resistor { a, b, r });
    }

    pub fn capacitor(&mut self, a: usize, b: usize, c: f64) {
        self.elements.push(Element::Capacitor { a, b, c, ic: None });
    }

    pub fn inductor(&mut self, a: usize, b: usize, l: f64) {
        self.elements.push(Element::Inductor { a, b, l, ic: None });
    }

    pub fn vsource(&mut self, a: usize, b: usize, v: f64) {
        self.elements
            .push(Element::VSource { a, b, wave: Waveform::Dc(v) });
    }

    pub fn isource(&mut self, a: usize, b: usize, i: f64) {
        self.elements
            .push(Element::ISource { a, b, wave: Waveform::Dc(i) });
    }

    pub fn validate(&self) -> Result<(), SimError> {
        if self.nodes.is_empty() {
            return Err(SimError::Invalid("circuit has no nodes"));
        }
        if self.nodes.len() > crate::util::limits::MAX_SIM_NODES {
            return Err(SimError::TooLarge("nodes"));
        }
        if self.elements.len() > crate::util::limits::MAX_SIM_ELEMENTS {
            return Err(SimError::TooLarge("elements"));
        }
        let n = self.nodes.len();
        for e in &self.elements {
            match e {
                Element::Resistor { a, b, r } => {
                    node_ok(*a, n)?;
                    node_ok(*b, n)?;
                    finite(*r)?;
                    if *r <= 0.0 {
                        return Err(SimError::Invalid("resistor must be positive"));
                    }
                }
                Element::Capacitor { a, b, c, ic } => {
                    node_ok(*a, n)?;
                    node_ok(*b, n)?;
                    finite(*c)?;
                    if *c < 0.0 {
                        return Err(SimError::Invalid("capacitor must be non-negative"));
                    }
                    if let Some(ic) = ic {
                        finite(*ic)?;
                    }
                }
                Element::Inductor { a, b, l, ic } => {
                    node_ok(*a, n)?;
                    node_ok(*b, n)?;
                    finite(*l)?;
                    if *l <= 0.0 {
                        return Err(SimError::Invalid("inductor must be positive"));
                    }
                    if let Some(ic) = ic {
                        finite(*ic)?;
                    }
                }
                Element::VSource { a, b, wave } | Element::ISource { a, b, wave } => {
                    node_ok(*a, n)?;
                    node_ok(*b, n)?;
                    finite(wave.dc_value())?;
                    if let Waveform::Pwl(points) = wave {
                        if points.len() > 10_000 {
                            return Err(SimError::TooLarge("pwl points"));
                        }
                        for (t, v) in points {
                            finite(*t)?;
                            finite(*v)?;
                        }
                    }
                }
                Element::Diode { a, k, is, n: nn, rs } => {
                    node_ok(*a, n)?;
                    node_ok(*k, n)?;
                    finite(*is)?;
                    finite(*nn)?;
                    finite(*rs)?;
                    if *is <= 0.0 || *nn <= 0.0 || *rs < 0.0 {
                        return Err(SimError::Invalid("invalid diode model"));
                    }
                }
                Element::Switch { a, b, ron, roff, .. } => {
                    node_ok(*a, n)?;
                    node_ok(*b, n)?;
                    finite(*ron)?;
                    finite(*roff)?;
                    if *ron <= 0.0 || *roff <= *ron {
                        return Err(SimError::Invalid("invalid switch resistance"));
                    }
                }
                Element::Digital(d) => {
                    if d.inputs.is_empty() || d.outputs.is_empty() {
                        return Err(SimError::Invalid("digital element needs inputs and outputs"));
                    }
                    for &i in &d.inputs {
                        node_ok(i, n)?;
                    }
                    for &o in &d.outputs {
                        node_ok(o, n)?;
                    }
                    finite(d.delay)?;
                    finite(d.vdd)?;
                    finite(d.rout)?;
                    if d.vdd <= 0.0 || d.rout <= 0.0 || d.delay < 0.0 {
                        return Err(SimError::Invalid("invalid digital driver"));
                    }
                    if d.gate == GateKind::Dff && (d.inputs.len() < 2 || d.outputs.len() < 2) {
                        return Err(SimError::Invalid("DFF needs D, CLK, Q, QN"));
                    }
                    if d.gate != GateKind::Dff && d.outputs.is_empty() {
                        return Err(SimError::Invalid("gate needs an output"));
                    }
                }
            }
        }
        Ok(())
    }
}

fn node_ok(n: usize, count: usize) -> Result<(), SimError> {
    if n < count {
        Ok(())
    } else {
        Err(SimError::BadNode(n))
    }
}

fn finite(v: f64) -> Result<(), SimError> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(SimError::Invalid("non-finite value"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimError {
    TooLarge(&'static str),
    BadNode(usize),
    Invalid(&'static str),
    Singular,
    NotConverged,
    Numerical,
    BadOption(&'static str),
    Compile(String),
    Empty,
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::TooLarge(what) => write!(f, "simulation too large: {what}"),
            SimError::BadNode(n) => write!(f, "node index out of range: {n}"),
            SimError::Invalid(what) => write!(f, "invalid circuit: {what}"),
            SimError::Singular => write!(f, "singular circuit matrix"),
            SimError::NotConverged => write!(f, "Newton iteration did not converge"),
            SimError::Numerical => write!(f, "numerical failure"),
            SimError::BadOption(what) => write!(f, "invalid simulation option: {what}"),
            SimError::Compile(what) => write!(f, "cannot compile circuit: {what}"),
            SimError::Empty => write!(f, "empty circuit"),
        }
    }
}

impl std::error::Error for SimError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integration {
    BackwardEuler,
    Trapezoidal,
}

impl Integration {
    pub fn as_str(self) -> &'static str {
        match self {
            Integration::BackwardEuler => "be",
            Integration::Trapezoidal => "trap",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SimOptions {
    pub tstep: f64,
    pub tstop: f64,
    pub method: Integration,
    pub max_steps: u32,
    pub record_nodes: Vec<usize>,
    pub reltol: f64,
    pub abstol: f64,
}

impl Default for SimOptions {
    fn default() -> Self {
        SimOptions {
            tstep: 1e-6,
            tstop: 1e-3,
            method: Integration::BackwardEuler,
            max_steps: 200_000,
            record_nodes: Vec::new(),
            reltol: 1e-6,
            abstol: 1e-9,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DcResult {
    /// Node voltages; ground is 0.0.
    pub voltages: Vec<f64>,
    /// Branch currents for voltage sources and inductors, in element order.
    pub branch_currents: Vec<f64>,
    pub iterations: usize,
    pub node_names: Vec<String>,
}

impl DcResult {
    pub fn voltage(&self, name: &str) -> Option<f64> {
        self.node_names
            .iter()
            .position(|n| n == name)
            .and_then(|i| self.voltages.get(i).copied())
    }
}

#[derive(Clone, Debug)]
pub struct TranResult {
    pub times: Vec<f64>,
    pub node_names: Vec<String>,
    /// One series per node name, aligned with `times`.
    pub series: Vec<Vec<f64>>,
    pub steps: u32,
    pub truncated: bool,
    pub method: Integration,
}

impl TranResult {
    pub fn series_for(&self, name: &str) -> Option<&[f64]> {
        self.node_names
            .iter()
            .position(|n| n == name)
            .map(|i| self.series[i].as_slice())
    }

    pub fn last(&self, name: &str) -> Option<f64> {
        self.series_for(name).and_then(|s| s.last().copied())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Dc,
    Be(f64),
    Trap(f64),
}

struct State {
    switch_closed: Vec<bool>,
    digital_out: Vec<bool>,
    digital_prev_clk: Vec<bool>,
    digital_pending: Vec<Option<(f64, bool)>>,
    cap_v: Vec<f64>,
    cap_i: Vec<f64>,
    ind_i: Vec<f64>,
    ind_v: Vec<f64>,
    diode_vj: Vec<f64>,
}

impl State {
    fn new(c: &Circuit) -> Self {
        let mut st = State {
            switch_closed: vec![false; c.elements.len()],
            digital_out: vec![false; c.elements.len()],
            digital_prev_clk: vec![false; c.elements.len()],
            digital_pending: vec![None; c.elements.len()],
            cap_v: vec![0.0; c.elements.len()],
            cap_i: vec![0.0; c.elements.len()],
            ind_i: vec![0.0; c.elements.len()],
            ind_v: vec![0.0; c.elements.len()],
            diode_vj: vec![0.5; c.elements.len()],
        };
        for (i, e) in c.elements.iter().enumerate() {
            match e {
                Element::Switch { closed, .. } => st.switch_closed[i] = *closed,
                Element::Digital(d) => st.digital_out[i] = d.initial,
                _ => {}
            }
        }
        st
    }
}

struct Layout {
    nodes: usize,
    branches: usize,
    branch_of: Vec<Option<usize>>,
}

impl Layout {
    fn new(c: &Circuit) -> Result<Layout, SimError> {
        let mut branches = 0;
        let mut branch_of = vec![None; c.elements.len()];
        for (i, e) in c.elements.iter().enumerate() {
            if matches!(e, Element::VSource { .. } | Element::Inductor { .. }) {
                branch_of[i] = Some(branches);
                branches += 1;
            }
        }
        let size = c.nodes.len().saturating_sub(1).saturating_add(branches);
        if size > 2048 {
            return Err(SimError::TooLarge("matrix"));
        }
        Ok(Layout {
            nodes: c.nodes.len(),
            branches,
            branch_of,
        })
    }

    fn size(&self) -> usize {
        self.nodes.saturating_sub(1) + self.branches
    }
}

fn add_g(m: &mut [f64], size: usize, a: usize, b: usize, g: f64) {
    if !g.is_finite() || g == 0.0 {
        return;
    }
    if a > 0 {
        let ra = a - 1;
        if b > 0 {
            let rb = b - 1;
            m[ra * size + ra] += g;
            m[rb * size + rb] += g;
            m[ra * size + rb] -= g;
            m[rb * size + ra] -= g;
        } else {
            m[ra * size + ra] += g;
        }
    } else if b > 0 {
        let rb = b - 1;
        m[rb * size + rb] += g;
    }
}

fn add_current(rhs: &mut [f64], a: usize, b: usize, i: f64) {
    if !i.is_finite() || i == 0.0 {
        return;
    }
    if a > 0 {
        rhs[a - 1] -= i;
    }
    if b > 0 {
        rhs[b - 1] += i;
    }
}

fn stamp_voltage_branch(
    m: &mut [f64],
    rhs: &mut [f64],
    size: usize,
    nodes: usize,
    a: usize,
    b: usize,
    br: usize,
    value: f64,
) {
    let col = (nodes - 1) + br;
    if a > 0 {
        m[(a - 1) * size + col] += 1.0;
    }
    if b > 0 {
        m[(b - 1) * size + col] -= 1.0;
    }
    if a > 0 {
        m[col * size + (a - 1)] += 1.0;
    }
    if b > 0 {
        m[col * size + (b - 1)] -= 1.0;
    }
    rhs[col] += value;
}

fn diode_iv(vj: f64, is: f64, n: f64) -> (f64, f64) {
    let nvt = n * VT;
    let x = (vj / nvt).clamp(-40.0, 40.0);
    let e = x.exp();
    let i = is * (e - 1.0) + GMIN * vj;
    let g = is * e / nvt + GMIN;
    (i, g)
}

fn solve_matrix(
    m: Vec<f64>,
    rhs: Vec<f64>,
    size: usize,
) -> Result<Vec<f64>, SimError> {
    if size == 0 {
        return Ok(Vec::new());
    }
    let lu = Lu::factor(&m, size).map_err(|e| match e {
        LuError::TooLarge => SimError::TooLarge("matrix"),
        LuError::Empty => SimError::Empty,
        LuError::NonFinite => SimError::Numerical,
        LuError::Singular => SimError::Singular,
    })?;
    lu.solve(&rhs).map_err(|e| match e {
        LuError::TooLarge => SimError::TooLarge("matrix"),
        LuError::Empty => SimError::Empty,
        LuError::NonFinite => SimError::Numerical,
        LuError::Singular => SimError::Singular,
    })
}

fn build_matrix(
    c: &Circuit,
    layout: &Layout,
    st: &State,
    mode: Mode,
    t: f64,
) -> (Vec<f64>, Vec<f64>) {
    let size = layout.size();
    let mut m = vec![0.0; size.saturating_mul(size)];
    let mut rhs = vec![0.0; size];
    if size == 0 {
        return (m, rhs);
    }
    let nodes = layout.nodes;
    for (ei, e) in c.elements.iter().enumerate() {
        match e {
            Element::Resistor { a, b, r } => add_g(&mut m, size, *a, *b, 1.0 / r.max(1e-12)),
            Element::Capacitor { a, b, c: cap, .. } => {
                let (g, hist) = match mode {
                    Mode::Dc => continue,
                    Mode::Be(dt) => {
                        let g = cap / dt.max(1e-18);
                        (g, g * st.cap_v[ei])
                    }
                    Mode::Trap(dt) => {
                        let g = 2.0 * cap / dt.max(1e-18);
                        (g, g * st.cap_v[ei] + st.cap_i[ei])
                    }
                };
                add_g(&mut m, size, *a, *b, g);
                // Inject the history current into node a.
                add_current(&mut rhs, *b, *a, hist);
            }
            Element::Inductor { a, b, l, .. } => {
                let br = match layout.branch_of[ei] {
                    Some(b) => b,
                    None => continue,
                };
                let col = (nodes - 1) + br;
                if *a > 0 {
                    m[(*a - 1) * size + col] += 1.0;
                    m[col * size + (*a - 1)] += 1.0;
                }
                if *b > 0 {
                    m[(*b - 1) * size + col] -= 1.0;
                    m[col * size + (*b - 1)] -= 1.0;
                }
                match mode {
                    Mode::Dc => {}
                    Mode::Be(dt) => {
                        let ldt = l / dt.max(1e-18);
                        m[col * size + col] -= ldt;
                        rhs[col] = -ldt * st.ind_i[ei];
                    }
                    Mode::Trap(dt) => {
                        let ldt = 2.0 * l / dt.max(1e-18);
                        m[col * size + col] -= ldt;
                        rhs[col] = -ldt * st.ind_i[ei] - st.ind_v[ei];
                    }
                }
            }
            Element::VSource { a, b, wave } => {
                let br = match layout.branch_of[ei] {
                    Some(b) => b,
                    None => continue,
                };
                let v = match mode {
                    Mode::Dc => wave.dc_value(),
                    _ => wave.value(t),
                };
                stamp_voltage_branch(&mut m, &mut rhs, size, nodes, *a, *b, br, v);
            }
            Element::ISource { a, b, wave } => {
                let i = match mode {
                    Mode::Dc => wave.dc_value(),
                    _ => wave.value(t),
                };
                // Positive current is delivered into node a and removed from b.
                add_current(&mut rhs, *b, *a, i);
            }
            Element::Diode { a, k, is, n, rs } => {
                let vj = st.diode_vj[ei];
                let (i0, gj) = diode_iv(vj, *is, *n);
                let geff = gj / (1.0 + gj * rs);
                let veq = vj - i0 / gj.max(1e-30);
                let ieq = geff * veq;
                add_g(&mut m, size, *a, *k, geff);
                // Equivalent current source from cathode to anode.
                add_current(&mut rhs, *k, *a, ieq);
            }
            Element::Switch {
                a, b, ron, roff, ..
            } => {
                let r = if st.switch_closed[ei] { *ron } else { *roff };
                add_g(&mut m, size, *a, *b, 1.0 / r.max(1e-12));
            }
            Element::Digital(d) => {
                let out_state = st.digital_out[ei];
                for (idx, &node) in d.outputs.iter().enumerate() {
                    let high = if d.gate == GateKind::Dff && idx == 1 {
                        !out_state
                    } else {
                        out_state
                    };
                    add_g(&mut m, size, node, 0, 1.0 / d.rout.max(1e-12));
                    if high {
                        add_current(&mut rhs, 0, node, d.vdd / d.rout.max(1e-12));
                    }
                }
            }
        }
    }
    // Minimum conductance to ground keeps floating nodes solvable.
    for n in 1..nodes {
        add_g(&mut m, size, n, 0, GMIN);
    }
    (m, rhs)
}

fn node_voltages(c: &Circuit, x: &[f64]) -> Vec<f64> {
    let mut v = vec![0.0; c.nodes.len()];
    for (i, slot) in v.iter_mut().enumerate().skip(1) {
        if i - 1 < x.len() {
            *slot = x[i - 1];
        }
    }
    v
}

fn branch_currents(layout: &Layout, x: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; layout.branches];
    for br in 0..layout.branches {
        let idx = (layout.nodes - 1) + br;
        if idx < x.len() {
            out[br] = x[idx];
        }
    }
    out
}

fn update_diodes(
    c: &Circuit,
    st: &mut State,
    v: &[f64],
    reltol: f64,
    abstol: f64,
) -> f64 {
    let mut max_delta = 0.0f64;
    for (ei, e) in c.elements.iter().enumerate() {
        if let Element::Diode { a, k, is, n, rs } = e {
            let vterm = v[*a] - v[*k];
            let old = st.diode_vj[ei];
            let mut vj = old;
            if *rs > 0.0 {
                for _ in 0..16 {
                    let (i, g) = diode_iv(vj, *is, *n);
                    let f = vj + rs * i - vterm;
                    let df = 1.0 + rs * g;
                    if !df.is_finite() || df.abs() < 1e-30 {
                        break;
                    }
                    let mut next = vj - f / df;
                    if !next.is_finite() {
                        break;
                    }
                    // Damped junction update.
                    if next > vj + 0.2 {
                        next = vj + 0.2;
                    } else if next < vj - 0.5 {
                        next = vj - 0.5;
                    }
                    if (next - vj).abs() < 1e-12 {
                        vj = next;
                        break;
                    }
                    vj = next;
                }
            } else {
                vj = vterm;
            }
            vj = vj.clamp(-20.0, 1.5);
            if !vj.is_finite() {
                vj = old;
            }
            max_delta = max_delta.max((vj - old).abs());
            st.diode_vj[ei] = vj;
        }
    }
    let _ = (reltol, abstol);
    max_delta
}

fn settle_digital(c: &Circuit, st: &mut State, v: &[f64]) -> bool {
    let mut changed = false;
    for (ei, e) in c.elements.iter().enumerate() {
        if let Element::Digital(d) = e {
            if d.gate == GateKind::Dff {
                continue;
            }
            let desired = eval_gate(d, v);
            if desired != st.digital_out[ei] {
                st.digital_out[ei] = desired;
                changed = true;
            }
        }
    }
    changed
}

fn eval_gate(d: &Digital, v: &[f64]) -> bool {
    let th = d.vdd * 0.5;
    let input = |i: usize| -> bool {
        d.inputs
            .get(i)
            .and_then(|&n| v.get(n))
            .map(|&x| x > th)
            .unwrap_or(false)
    };
    match d.gate {
        GateKind::And => d.inputs.iter().enumerate().all(|(i, _)| input(i)),
        GateKind::Or => d.inputs.iter().enumerate().any(|(i, _)| input(i)),
        GateKind::Nand => !d.inputs.iter().enumerate().all(|(i, _)| input(i)),
        GateKind::Nor => !d.inputs.iter().enumerate().any(|(i, _)| input(i)),
        GateKind::Xor => d.inputs.iter().enumerate().filter(|(i, _)| input(*i)).count() % 2 == 1,
        GateKind::Not => !input(0),
        GateKind::Dff => false,
    }
}

fn newton(
    c: &Circuit,
    layout: &Layout,
    st: &mut State,
    mode: Mode,
    t: f64,
    max_iter: usize,
    reltol: f64,
    abstol: f64,
) -> Result<(Vec<f64>, Vec<f64>, usize), SimError> {
    let mut x = Vec::new();
    let mut iterations = 0;
    let mut v = vec![0.0; c.nodes.len()];
    for iter in 0..max_iter {
        iterations = iter + 1;
        let (m, rhs) = build_matrix(c, layout, st, mode, t);
        let solved = solve_matrix(m, rhs, layout.size())?;
        let new_v = node_voltages(c, &solved);
        if new_v.iter().any(|v| !v.is_finite()) {
            return Err(SimError::Numerical);
        }
        let mut delta = 0.0f64;
        for (i, &nv) in new_v.iter().enumerate() {
            delta = delta.max((nv - v[i]).abs());
        }
        let dj = update_diodes(c, st, &new_v, reltol, abstol);
        v = new_v;
        x = solved;
        if iter > 0 && delta <= abstol + reltol * 1.0 && dj <= 1e-7 {
            break;
        }
        if iter + 1 == max_iter {
            // If only linear devices are present a single solve is exact; the
            // delta above can still be large on the first iteration.
            let has_nonlinear = c
                .elements
                .iter()
                .any(|e| matches!(e, Element::Diode { .. }));
            if has_nonlinear {
                return Err(SimError::NotConverged);
            }
        }
    }
    Ok((v, x, iterations))
}

fn solve_dc(c: &Circuit, max_rounds: usize) -> Result<(DcResult, State), SimError> {
    c.validate()?;
    if c.elements.is_empty() {
        return Ok((
            DcResult {
                voltages: vec![0.0; c.nodes.len()],
                branch_currents: Vec::new(),
                iterations: 0,
                node_names: c.nodes.clone(),
            },
            State::new(c),
        ));
    }
    let layout = Layout::new(c)?;
    let mut st = State::new(c);
    let mut total_iters = 0;
    for _round in 0..max_rounds {
        let (v, x, iters) = newton(c, &layout, &mut st, Mode::Dc, 0.0, 100, 1e-6, 1e-9)?;
        total_iters += iters;
        let changed = settle_digital(c, &mut st, &v);
        if !changed {
            return Ok((
                DcResult {
                    voltages: v,
                    branch_currents: branch_currents(&layout, &x),
                    iterations: total_iters,
                    node_names: c.nodes.clone(),
                },
                st,
            ));
        }
        if _round + 1 == max_rounds {
            return Err(SimError::NotConverged);
        }
    }
    Err(SimError::NotConverged)
}

/// DC operating point. Capacitors are open, inductors are shorts, and digital
/// outputs settle combinationally.
pub fn dc_op(c: &Circuit) -> Result<DcResult, SimError> {
    solve_dc(c, 50).map(|(r, _)| r)
}

struct Recorder {
    times: Vec<f64>,
    series: Vec<Vec<f64>>,
    stride: u64,
    count: u64,
    limit: usize,
}

impl Recorder {
    fn new(nodes: usize, limit: usize) -> Self {
        Recorder {
            times: Vec::new(),
            series: vec![Vec::new(); nodes],
            stride: 1,
            count: 0,
            limit: limit.max(16),
        }
    }

    fn push(&mut self, t: f64, values: &[f64]) {
        let take = self.count % self.stride == 0;
        self.count = self.count.saturating_add(1);
        if !take {
            return;
        }
        self.times.push(t);
        for (i, s) in self.series.iter_mut().enumerate() {
            s.push(values.get(i).copied().unwrap_or(0.0));
        }
        if self.times.len() >= self.limit {
            // Decimate in place; every second sample is kept.
            let mut new_times = Vec::with_capacity(self.times.len() / 2 + 1);
            for (i, t) in self.times.iter().enumerate() {
                if i % 2 == 0 {
                    new_times.push(*t);
                }
            }
            self.times = new_times;
            for s in &mut self.series {
                let mut ns = Vec::with_capacity(s.len() / 2 + 1);
                for (i, v) in s.iter().enumerate() {
                    if i % 2 == 0 {
                        ns.push(*v);
                    }
                }
                *s = ns;
            }
            self.stride = self.stride.saturating_mul(2).max(1);
        }
    }
}

/// Transient analysis. Returns recorded waveforms and a truncation flag.
pub fn transient(c: &Circuit, opts: &SimOptions) -> Result<TranResult, SimError> {
    c.validate()?;
    if !(opts.tstep > 0.0) || !opts.tstep.is_finite() {
        return Err(SimError::BadOption("tstep"));
    }
    if !(opts.tstop > 0.0) || !opts.tstop.is_finite() {
        return Err(SimError::BadOption("tstop"));
    }
    if opts.tstep > opts.tstop {
        return Err(SimError::BadOption("tstep exceeds tstop"));
    }
    if opts.max_steps == 0 || opts.max_steps > crate::util::limits::MAX_SIM_STEPS {
        return Err(SimError::BadOption("max_steps"));
    }
    let reltol = opts.reltol.clamp(1e-12, 1e-2);
    let abstol = opts.abstol.clamp(1e-15, 1e-3);

    let layout = Layout::new(c)?;
    let (dc, mut st) = solve_dc(c, 50)?;

    let record_nodes: Vec<usize> = if opts.record_nodes.is_empty() {
        (1..c.nodes.len().min(65)).collect()
    } else {
        let mut r = opts.record_nodes.clone();
        r.sort_unstable();
        r.dedup();
        if r.len() > 64 {
            return Err(SimError::TooLarge("record nodes"));
        }
        for &n in &r {
            node_ok(n, c.nodes.len())?;
        }
        r
    };
    let names: Vec<String> = record_nodes
        .iter()
        .map(|&n| c.nodes[n].clone())
        .collect();
    let mut rec = Recorder::new(names.len(), 20_000);

    // Initial capacitor and inductor state.
    for (ei, e) in c.elements.iter().enumerate() {
        match e {
            Element::Capacitor { a, b, ic, .. } => {
                st.cap_v[ei] = ic.unwrap_or(dc.voltages[*a] - dc.voltages[*b]);
                st.cap_i[ei] = 0.0;
            }
            Element::Inductor { ic, .. } => {
                if let Some(ic) = ic {
                    st.ind_i[ei] = *ic;
                }
            }
            _ => {}
        }
    }
    // Branch currents are in element order for voltage sources and inductors.
    let mut br = 0;
    for (ei, e) in c.elements.iter().enumerate() {
        if matches!(e, Element::VSource { .. } | Element::Inductor { .. }) {
            if let Element::Inductor { ic, .. } = e {
                if ic.is_none() {
                    st.ind_i[ei] = dc.branch_currents.get(br).copied().unwrap_or(0.0);
                }
            }
            br += 1;
        }
    }

    let initial: Vec<f64> = record_nodes.iter().map(|&n| dc.voltages[n]).collect();
    rec.push(0.0, &initial);

    let tstep = opts.tstep;
    let tstop = opts.tstop;
    let mut t = 0.0f64;
    let mut steps = 0u32;
    let mut truncated = false;
    while t < tstop - 1e-18 {
        if steps >= opts.max_steps {
            truncated = true;
            break;
        }
        // Clamp the step to the next digital event.
        let mut dt = tstep.min(tstop - t);
        for p in st.digital_pending.iter().flatten() {
            let ev = p.0;
            if ev > t + 1e-15 && ev - t < dt {
                dt = ev - t;
            }
        }
        if !(dt > 0.0) || !dt.is_finite() {
            truncated = true;
            break;
        }
        let t_next = t + dt;
        if !t_next.is_finite() || t_next > tstop + 1e-18 {
            break;
        }
        // Apply events that are due.
        for ei in 0..c.elements.len() {
            if let Some((etime, value)) = st.digital_pending[ei] {
                if etime <= t_next + 1e-18 {
                    st.digital_out[ei] = value;
                    st.digital_pending[ei] = None;
                }
            }
        }
        let mode = match opts.method {
            Integration::BackwardEuler => Mode::Be(dt),
            Integration::Trapezoidal => Mode::Trap(dt),
        };
        let (v, x, _iters) = newton(c, &layout, &mut st, mode, t_next, 100, reltol, abstol)?;

        // Update reactive element history.
        for (ei, e) in c.elements.iter().enumerate() {
            match e {
                Element::Capacitor { a, b, c: cap, .. } => {
                    let vn = v[*a] - v[*b];
                    let i_new = match opts.method {
                        Integration::BackwardEuler => {
                            let g = cap / dt.max(1e-18);
                            g * (vn - st.cap_v[ei])
                        }
                        Integration::Trapezoidal => {
                            let g = 2.0 * cap / dt.max(1e-18);
                            g * (vn - st.cap_v[ei]) - st.cap_i[ei]
                        }
                    };
                    st.cap_v[ei] = vn;
                    st.cap_i[ei] = i_new;
                }
                Element::Inductor { .. } => {
                    if let Some(br) = layout.branch_of[ei] {
                        let idx = layout.nodes - 1 + br;
                        st.ind_i[ei] = x.get(idx).copied().unwrap_or(0.0);
                        if let Element::Inductor { a, b, .. } = e {
                            st.ind_v[ei] = v[*a] - v[*b];
                        }
                    }
                }
                _ => {}
            }
        }

        // Sample digital inputs and schedule output events.
        for (ei, e) in c.elements.iter().enumerate() {
            if let Element::Digital(d) = e {
                let th = d.vdd * 0.5;
                let read = |n: usize| v.get(n).map(|&x| x > th).unwrap_or(false);
                if d.gate == GateKind::Dff {
                    let dval = d.inputs.first().map(|&n| read(n)).unwrap_or(false);
                    let clk = d.inputs.get(1).map(|&n| read(n)).unwrap_or(false);
                    if clk && !st.digital_prev_clk[ei] {
                        if dval != st.digital_out[ei] {
                            let when = t_next + d.delay.max(1e-12);
                            st.digital_pending[ei] = Some((when, dval));
                        }
                    }
                    st.digital_prev_clk[ei] = clk;
                } else {
                    let desired = eval_gate(d, &v);
                    if desired == st.digital_out[ei] {
                        st.digital_pending[ei] = None;
                    } else if st.digital_pending[ei].is_none() {
                        let when = t_next + d.delay.max(1e-12);
                        st.digital_pending[ei] = Some((when, desired));
                    }
                }
            }
        }

        let values: Vec<f64> = record_nodes.iter().map(|&n| v[n]).collect();
        rec.push(t_next, &values);
        t = t_next;
        steps = steps.saturating_add(1);
    }

    Ok(TranResult {
        times: rec.times,
        node_names: names,
        series: rec.series,
        steps,
        truncated,
        method: opts.method,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn voltage_divider() {
        let mut c = Circuit::new();
        let vin = c.add_node("in");
        let out = c.add_node("out");
        c.vsource(vin, 0, 10.0);
        c.resistor(vin, out, 1000.0);
        c.resistor(out, 0, 3000.0);
        let r = dc_op(&c).unwrap();
        assert!(close(r.voltages[out], 7.5, 1e-12));
        assert!(close(r.voltages[vin], 10.0, 1e-12));
    }

    #[test]
    fn parallel_resistors() {
        let mut c = Circuit::new();
        let a = c.add_node("a");
        c.isource(a, 0, 0.01);
        c.resistor(a, 0, 1000.0);
        c.resistor(a, 0, 1000.0);
        let r = dc_op(&c).unwrap();
        // 10 mA into 500 ohm.
        assert!(close(r.voltages[a], 5.0, 1e-12));
    }

    #[test]
    fn current_source_direction() {
        let mut c = Circuit::new();
        let a = c.add_node("a");
        c.isource(a, 0, 0.001);
        c.resistor(a, 0, 1000.0);
        let r = dc_op(&c).unwrap();
        assert!(close(r.voltages[a], 1.0, 1e-9));
    }

    #[test]
    fn diode_rectifier_forward() {
        let mut c = Circuit::new();
        let a = c.add_node("a");
        let mid = c.add_node("mid");
        c.vsource(a, 0, 5.0);
        c.resistor(a, mid, 1000.0);
        let k = 0; // ground
        c.add(Element::Diode {
            a: mid,
            k,
            is: 2.2e-9,
            n: 1.8,
            rs: 0.0,
        });
        let r = dc_op(&c).unwrap();
        let vd = r.voltages[mid];
        assert!(vd > 0.4 && vd < 0.9, "vd={vd}");
        let i = (5.0 - vd) / 1000.0;
        assert!(i > 0.003 && i < 0.006, "i={i}");
    }

    #[test]
    fn rc_charge_matches_analytic() {
        let mut c = Circuit::new();
        let in_node = c.add_node("in");
        let out = c.add_node("out");
        c.elements.push(Element::VSource {
            a: in_node,
            b: 0,
            wave: Waveform::Pulse {
                v1: 0.0,
                v2: 5.0,
                delay: 0.0,
                rise: 1e-9,
                width: 1.0,
                period: 2.0,
            },
        });
        let r = 1000.0;
        let cap = 1e-6;
        c.resistor(in_node, out, r);
        c.capacitor(out, 0, cap);
        let tau = r * cap;
        let mut opts = SimOptions::default();
        opts.tstep = tau / 4000.0;
        opts.tstop = 4.0 * tau;
        opts.method = Integration::Trapezoidal;
        let r = transient(&c, &opts).unwrap();
        let s = r.series_for("out").unwrap();
        for (i, &t) in r.times.iter().enumerate() {
            let expected = 5.0 * (1.0 - (-t / tau).exp());
            assert!(
                (s[i] - expected).abs() < 0.002 * 5.0 + 1e-9,
                "t={t} got={} want={expected}",
                s[i]
            );
        }
    }

    #[test]
    fn logic_and_truth_table() {
        for (a, b, want) in [(false, false, false), (true, false, false), (true, true, true)] {
            let mut c = Circuit::new();
            let na = c.add_node("a");
            let nb = c.add_node("b");
            let ny = c.add_node("y");
            c.vsource(na, 0, if a { 5.0 } else { 0.0 });
            c.vsource(nb, 0, if b { 5.0 } else { 0.0 });
            c.add(Element::Digital(Digital {
                gate: GateKind::And,
                inputs: vec![na, nb],
                outputs: vec![ny],
                delay: 1e-9,
                vdd: 5.0,
                rout: 100.0,
                initial: false,
            }));
            let r = dc_op(&c).unwrap();
            assert_eq!(r.voltages[ny] > 2.5, want);
        }
    }
}
