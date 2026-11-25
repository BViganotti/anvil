//! Compile a schematic netlist into a simulation circuit.
//!
//! The mapping is by symbol name and pin name. Unsupported symbols are reported
//! instead of silently ignored, except for power symbols, connectors and test
//! points, which are purely connective.

use super::{Circuit, Digital, Element, GateKind, SimError, Waveform};
use crate::library::Library;
use crate::netlist::Netlist;
use crate::schematic::{Component, Schematic};
use crate::util::{limits, parse_eng};

#[derive(Clone, Debug)]
pub struct CompileOptions {
    pub diode_is: f64,
    pub diode_n: f64,
    pub diode_rs: f64,
    pub digital_delay: f64,
    pub vdd: f64,
    pub rout: f64,
}

impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions {
            diode_is: 2.2e-9,
            diode_n: 1.8,
            diode_rs: 0.0,
            digital_delay: 1e-8,
            vdd: 5.0,
            rout: 100.0,
        }
    }
}

fn err(what: impl std::fmt::Display) -> SimError {
    SimError::Compile(what.to_string())
}

fn ground_names(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "GND" | "0" | "VSS" | "AGND" | "DGND" | "GNDA" | "GNDD" | "GROUND"
    )
}

fn parse_value(comp: &Component, field: &str) -> Result<f64, SimError> {
    parse_eng(field)
        .filter(|v| v.is_finite())
        .ok_or_else(|| err(format!("{}: cannot parse value {:?}", comp.refdes, field)))
}

fn prop<'a>(comp: &'a Component, key: &str) -> Option<&'a str> {
    comp.property(key)
}

fn prop_value(comp: &Component, key: &str, default: f64) -> Result<f64, SimError> {
    match prop(comp, key) {
        Some(s) => {
            let v = parse_eng(s)
                .filter(|v| v.is_finite())
                .ok_or_else(|| err(format!("{}: bad property {}={:?}", comp.refdes, key, s)))?;
            Ok(v)
        }
        None => Ok(default),
    }
}

fn prop_bool(comp: &Component, key: &str, default: bool) -> bool {
    match prop(comp, key) {
        Some(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "closed" | "on"
        ),
        None => default,
    }
}

fn pin_node(
    comp: &Component,
    pin_name: &str,
    netlist: &Netlist,
    net_nodes: &[Option<usize>],
) -> Result<usize, SimError> {
    let pin = comp
        .pin_by_name(pin_name)
        .or_else(|| comp.pin_by_number(pin_name))
        .ok_or_else(|| {
            err(format!(
                "{}: symbol {} has no pin {}",
                comp.refdes, comp.symbol, pin_name
            ))
        })?;
    let net = netlist
        .pin_net(&comp.refdes, &pin.number)
        .ok_or_else(|| err(format!("{}: pin {} is not on any net", comp.refdes, pin.number)))?;
    net_nodes
        .get(net.id)
        .copied()
        .flatten()
        .ok_or_else(|| err(format!("{}: net {} has no node", comp.refdes, net.name)))
}

fn parse_waveform(comp: &Component, dc: f64) -> Result<Waveform, SimError> {
    let kind = prop(comp, "wave").unwrap_or("dc").trim().to_ascii_lowercase();
    match kind.as_str() {
        "" | "dc" => Ok(Waveform::Dc(dc)),
        "pulse" | "square" => {
            let v1 = prop_value(comp, "v1", 0.0)?;
            let v2 = prop_value(comp, "v2", dc)?;
            let delay = prop_value(comp, "delay", 0.0)?;
            let rise = prop_value(comp, "rise", 1e-9)?;
            let width = prop_value(comp, "width", 1.0)?;
            let period = prop_value(comp, "period", 2.0)?;
            if rise < 0.0 || width < 0.0 || period < 0.0 || delay < 0.0 {
                return Err(err(format!("{}: negative pulse parameter", comp.refdes)));
            }
            Ok(Waveform::Pulse {
                v1,
                v2,
                delay,
                rise,
                width,
                period,
            })
        }
        "sin" | "sine" => {
            let offset = prop_value(comp, "offset", 0.0)?;
            let amp = prop_value(comp, "amp", dc)?;
            let freq = prop_value(comp, "freq", 1000.0)?;
            let delay = prop_value(comp, "delay", 0.0)?;
            let phase = prop_value(comp, "phase", 0.0)?;
            if delay < 0.0 || !freq.is_finite() {
                return Err(err(format!("{}: bad sine parameter", comp.refdes)));
            }
            Ok(Waveform::Sin {
                offset,
                amp,
                freq,
                delay,
                phase,
            })
        }
        "pwl" => {
            let spec = prop(comp, "pwl")
                .ok_or_else(|| err(format!("{}: pwl source needs a pwl property", comp.refdes)))?;
            let mut points: Vec<(f64, f64)> = Vec::new();
            for pair in spec.split(',') {
                let pair = pair.trim();
                if pair.is_empty() {
                    continue;
                }
                let (t, v) = pair
                    .split_once(':')
                    .ok_or_else(|| err(format!("{}: malformed pwl point", comp.refdes)))?;
                let t = parse_eng(t).ok_or_else(|| err(format!("{}: bad pwl time", comp.refdes)))?;
                let v = parse_eng(v).ok_or_else(|| err(format!("{}: bad pwl value", comp.refdes)))?;
                if !t.is_finite() || !v.is_finite() || t < 0.0 {
                    return Err(err(format!("{}: bad pwl point", comp.refdes)));
                }
                if points.last().map(|p| t <= p.0).unwrap_or(false) {
                    return Err(err(format!("{}: pwl times must increase", comp.refdes)));
                }
                points.push((t, v));
                if points.len() > 1000 {
                    return Err(err(format!("{}: pwl table too long", comp.refdes)));
                }
            }
            if points.is_empty() {
                return Err(err(format!("{}: empty pwl table", comp.refdes)));
            }
            Ok(Waveform::Pwl(points))
        }
        other => Err(err(format!(
            "{}: unsupported waveform {:?}",
            comp.refdes, other
        ))),
    }
}

/// Build a [`Circuit`] from a schematic and its extracted netlist.
pub fn compile_schematic(
    sch: &Schematic,
    netlist: &Netlist,
    lib: &Library,
    opts: &CompileOptions,
) -> Result<Circuit, SimError> {
    if sch.components().len() > limits::MAX_COMPONENTS {
        return Err(SimError::TooLarge("components"));
    }
    let grounds: Vec<&crate::netlist::Net> = netlist
        .nets
        .iter()
        .filter(|n| ground_names(&n.name))
        .collect();
    if grounds.len() > 1 {
        return Err(err("more than one ground net"));
    }
    let ground_id = grounds.first().map(|n| n.id);
    if ground_id.is_none() && !sch.components().is_empty() {
        return Err(err(
            "no ground net (add a GND symbol or label a net GND)",
        ));
    }

    let mut circuit = Circuit::new();
    let mut net_nodes: Vec<Option<usize>> = vec![None; netlist.nets.len()];
    for net in &netlist.nets {
        if Some(net.id) == ground_id {
            net_nodes[net.id] = Some(0);
        } else {
            let node = circuit.add_node(&net.name);
            net_nodes[net.id] = Some(node);
        }
    }
    if circuit.nodes.len() > limits::MAX_SIM_NODES {
        return Err(SimError::TooLarge("nodes"));
    }

    for comp in sch.components() {
        let sym = comp.symbol.as_str();
        match sym {
            "GND" | "VCC" => continue,
            "R" => {
                let r = parse_value(comp, &comp.value)?;
                if r <= 0.0 {
                    return Err(err(format!("{}: resistance must be positive", comp.refdes)));
                }
                let a = pin_node(comp, "1", netlist, &net_nodes)?;
                let b = pin_node(comp, "2", netlist, &net_nodes)?;
                circuit.resistor(a, b, r);
            }
            "C" => {
                let c = parse_value(comp, &comp.value)?;
                if c < 0.0 {
                    return Err(err(format!("{}: capacitance must be >= 0", comp.refdes)));
                }
                let a = pin_node(comp, "1", netlist, &net_nodes)?;
                let b = pin_node(comp, "2", netlist, &net_nodes)?;
                let ic = match prop(comp, "ic") {
                    Some(s) => Some(parse_eng(s).ok_or_else(|| {
                        err(format!("{}: bad capacitor ic {:?}", comp.refdes, s))
                    })?),
                    None => None,
                };
                circuit.elements.push(Element::Capacitor { a, b, c, ic });
            }
            "L" => {
                let l = parse_value(comp, &comp.value)?;
                if l <= 0.0 {
                    return Err(err(format!("{}: inductance must be positive", comp.refdes)));
                }
                let a = pin_node(comp, "1", netlist, &net_nodes)?;
                let b = pin_node(comp, "2", netlist, &net_nodes)?;
                let ic = match prop(comp, "ic") {
                    Some(s) => Some(
                        parse_eng(s)
                            .ok_or_else(|| err(format!("{}: bad inductor ic", comp.refdes)))?,
                    ),
                    None => None,
                };
                circuit.elements.push(Element::Inductor { a, b, l, ic });
            }
            "V" => {
                let dc = parse_value(comp, &comp.value)?;
                let a = pin_node(comp, "+", netlist, &net_nodes)?;
                let b = pin_node(comp, "-", netlist, &net_nodes)?;
                let wave = parse_waveform(comp, dc)?;
                circuit.elements.push(Element::VSource { a, b, wave });
            }
            "I" => {
                let dc = parse_value(comp, &comp.value)?;
                let a = pin_node(comp, "+", netlist, &net_nodes)?;
                let b = pin_node(comp, "-", netlist, &net_nodes)?;
                let wave = parse_waveform(comp, dc)?;
                circuit.elements.push(Element::ISource { a, b, wave });
            }
            "D" | "LED" => {
                let a = pin_node(comp, "A", netlist, &net_nodes)?;
                let k = pin_node(comp, "K", netlist, &net_nodes)?;
                let is = prop_value(comp, "is", opts.diode_is)?;
                let n = prop_value(comp, "n", opts.diode_n)?;
                let rs = prop_value(comp, "rs", opts.diode_rs)?;
                if is <= 0.0 || n <= 0.0 || rs < 0.0 {
                    return Err(err(format!("{}: invalid diode model", comp.refdes)));
                }
                circuit.elements.push(Element::Diode { a, k, is, n, rs });
            }
            "SW" => {
                let a = pin_node(comp, "1", netlist, &net_nodes)?;
                let b = pin_node(comp, "2", netlist, &net_nodes)?;
                let ron = prop_value(comp, "ron", 0.01)?;
                let roff = prop_value(comp, "roff", 1e9)?;
                if ron <= 0.0 || roff <= ron {
                    return Err(err(format!("{}: invalid switch resistance", comp.refdes)));
                }
                circuit.elements.push(Element::Switch {
                    a,
                    b,
                    ron,
                    roff,
                    closed: prop_bool(comp, "closed", false),
                });
            }
            "AND2" | "OR2" | "NAND2" | "NOR2" | "XOR2" | "NOT" | "DFF" => {
                let gate = GateKind::parse(sym).ok_or_else(|| err("unknown gate"))?;
                let digital = compile_digital(comp, gate, netlist, &net_nodes, opts)?;
                circuit.elements.push(Element::Digital(digital));
            }
            other => {
                let category = lib.symbol(other).map(|_| "").unwrap_or("");
                // Connectors and test points are connective only.
                if comp.symbol.starts_with("CONN") || comp.footprint.starts_with("PINHDR") {
                    continue;
                }
                let _ = category;
                return Err(err(format!(
                    "{}: symbol {} is not supported by the simulator",
                    comp.refdes, other
                )));
            }
        }
    }

    circuit.validate()?;
    Ok(circuit)
}

fn compile_digital(
    comp: &Component,
    gate: GateKind,
    netlist: &Netlist,
    net_nodes: &[Option<usize>],
    opts: &CompileOptions,
) -> Result<Digital, SimError> {
    let delay = prop_value(comp, "delay", opts.digital_delay)?.max(0.0);
    let vdd = prop_value(comp, "vdd", opts.vdd)?;
    let rout = prop_value(comp, "rout", opts.rout)?;
    if vdd <= 0.0 || rout <= 0.0 {
        return Err(err(format!("{}: invalid logic levels", comp.refdes)));
    }
    let initial = prop_bool(comp, "initial", false);
    match gate {
        GateKind::Dff => {
            let d = pin_node(comp, "D", netlist, net_nodes)?;
            let clk = pin_node(comp, "CLK", netlist, net_nodes)?;
            let q = pin_node(comp, "Q", netlist, net_nodes)?;
            let qn = pin_node(comp, "QN", netlist, net_nodes)?;
            if q == qn {
                return Err(err(format!("{}: Q and QN are shorted", comp.refdes)));
            }
            Ok(Digital {
                gate,
                inputs: vec![d, clk],
                outputs: vec![q, qn],
                delay,
                vdd,
                rout,
                initial,
            })
        }
        _ => {
            let mut inputs = Vec::new();
            if gate == GateKind::Not {
                inputs.push(pin_node(comp, "A", netlist, net_nodes)?);
            } else {
                inputs.push(pin_node(comp, "A", netlist, net_nodes)?);
                inputs.push(pin_node(comp, "B", netlist, net_nodes)?);
            }
            let y = pin_node(comp, "Y", netlist, net_nodes)?;
            Ok(Digital {
                gate,
                inputs,
                outputs: vec![y],
                delay,
                vdd,
                rout,
                initial,
            })
        }
    }
}
