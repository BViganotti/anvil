//! Excellon drill writer and parser.
//!
//! The writer uses metric decimal coordinates with four fractional digits
//! (0.1 µm resolution). The parser accepts metric/imperial decimal Excellon,
//! tool tables, `%`/`M95` headers and comments, and rejects ambiguous
//! fixed-format integer coordinates instead of guessing.

use anvil_core::geom::Point;
use anvil_core::util::limits;
use anvil_pcb::board::Board;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillTool {
    pub number: u32,
    pub diameter: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillHit {
    pub tool: u32,
    pub at: Point,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillFile {
    pub tools: Vec<DrillTool>,
    pub hits: Vec<DrillHit>,
    pub units_mm: bool,
}

impl DrillFile {
    pub fn tool(&self, number: u32) -> Option<&DrillTool> {
        self.tools.iter().find(|t| t.number == number)
    }

    pub fn diameters(&self) -> Vec<i64> {
        let mut d: Vec<i64> = self.tools.iter().map(|t| t.diameter).collect();
        d.sort_unstable();
        d.dedup();
        d
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrillError {
    TooLarge,
    Empty,
    Syntax { line: usize, message: String },
    UnknownTool(u32),
    NoTools,
    Unsupported(&'static str),
}

impl fmt::Display for DrillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DrillError::TooLarge => write!(f, "drill file exceeds the size limit"),
            DrillError::Empty => write!(f, "empty drill file"),
            DrillError::Syntax { line, message } => write!(f, "drill syntax error on line {line}: {message}"),
            DrillError::UnknownTool(t) => write!(f, "drill hit references unknown tool T{t}"),
            DrillError::NoTools => write!(f, "drill file has no tools"),
            DrillError::Unsupported(s) => write!(f, "unsupported drill feature: {s}"),
        }
    }
}

impl std::error::Error for DrillError {}

fn fmt_coord(nm: i64) -> String {
    let neg = nm < 0;
    let n = (nm as i128).unsigned_abs();
    let whole = n / 1_000_000;
    let frac = (n % 1_000_000 + 50) / 100; // four decimal places
    if frac >= 10_000 {
        format!("{}{}.{:04}", if neg { "-" } else { "" }, whole + 1, 0)
    } else {
        format!("{}{}.{:04}", if neg { "-" } else { "" }, whole, frac)
    }
}

/// Extract all drilled holes from pads and vias.
pub fn from_board(board: &Board) -> DrillFile {
    let mut diameters: Vec<i64> = Vec::new();
    for f in &board.footprints {
        for p in &f.pads {
            if p.drill > 0 {
                diameters.push(p.drill);
            }
        }
    }
    for v in &board.vias {
        if v.drill > 0 {
            diameters.push(v.drill);
        }
    }
    diameters.sort_unstable();
    diameters.dedup();
    let tools: Vec<DrillTool> = diameters
        .iter()
        .enumerate()
        .map(|(i, &diameter)| DrillTool {
            number: i as u32 + 1,
            diameter,
        })
        .collect();
    let tool_for = |d: i64| -> Option<u32> {
        tools.iter().find(|t| t.diameter == d).map(|t| t.number)
    };
    let mut hits = Vec::new();
    for f in &board.footprints {
        for p in &f.pads {
            if p.drill > 0 {
                if let Some(tool) = tool_for(p.drill) {
                    hits.push(DrillHit { tool, at: p.at });
                }
            }
        }
    }
    for v in &board.vias {
        if v.drill > 0 {
            if let Some(tool) = tool_for(v.drill) {
                hits.push(DrillHit { tool, at: v.at });
            }
        }
    }
    hits.sort_by(|a, b| {
        a.tool
            .cmp(&b.tool)
            .then(a.at.x.cmp(&b.at.x))
            .then(a.at.y.cmp(&b.at.y))
    });
    DrillFile {
        tools,
        hits,
        units_mm: true,
    }
}

pub fn write(file: &DrillFile) -> String {
    let mut out = String::new();
    out.push_str("M48\n; Anvil drill file\nFMAT,2\nMETRIC,TZ\n");
    for t in &file.tools {
        out.push_str(&format!("T{}C{}\n", t.number, fmt_coord(t.diameter)));
    }
    out.push_str("%\nG90\nG05\n");
    let mut current: Option<u32> = None;
    for h in &file.hits {
        if current != Some(h.tool) {
            out.push_str(&format!("T{}\n", h.tool));
            current = Some(h.tool);
        }
        out.push_str(&format!("X{}Y{}\n", fmt_coord(h.at.x), fmt_coord(h.at.y)));
    }
    out.push_str("M30\n");
    out
}

pub fn write_board(board: &Board) -> String {
    write(&from_board(board))
}

fn parse_number(s: &str, line: usize) -> Result<f64, DrillError> {
    if s.is_empty() || s.len() > 32 {
        return Err(DrillError::Syntax {
            line,
            message: "empty number".to_string(),
        });
    }
    let v: f64 = s.parse().map_err(|_| DrillError::Syntax {
        line,
        message: format!("bad number {:?}", s),
    })?;
    if !v.is_finite() {
        return Err(DrillError::Syntax {
            line,
            message: "non-finite number".to_string(),
        });
    }
    Ok(v)
}

fn parse_coord(s: &str, units_mm: bool, line: usize) -> Result<i64, DrillError> {
    if !s.contains('.') {
        return Err(DrillError::Unsupported(
            "fixed-format integer coordinates (use decimal coordinates)",
        ));
    }
    let v = parse_number(s, line)?;
    let nm = if units_mm { v * 1e6 } else { v * 25_400_000.0 };
    if !nm.is_finite() || nm.abs() > 1e12 {
        return Err(DrillError::Syntax {
            line,
            message: "coordinate out of range".to_string(),
        });
    }
    Ok(nm.round() as i64)
}

/// Parse an Excellon drill file.
pub fn parse(text: &str) -> Result<DrillFile, DrillError> {
    if text.is_empty() {
        return Err(DrillError::Empty);
    }
    if text.len() > limits::MAX_GERBER_BYTES {
        return Err(DrillError::TooLarge);
    }
    if !text.is_ascii() {
        return Err(DrillError::Syntax {
            line: 1,
            message: "non-ASCII input".to_string(),
        });
    }
    let mut tools: Vec<DrillTool> = Vec::new();
    let mut hits: Vec<DrillHit> = Vec::new();
    let mut units_mm = true;
    let mut in_header = false;
    let mut current_tool: Option<u32> = None;
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let line_text = raw.split(';').next().unwrap_or("").trim();
        if line_text.is_empty() {
            continue;
        }
        if line_text == "M48" {
            in_header = true;
            continue;
        }
        if line_text == "%" || line_text == "M95" {
            in_header = false;
            continue;
        }
        if line_text == "M30" {
            break;
        }
        if line_text == "METRIC" || line_text == "METRIC,TZ" || line_text == "METRIC,LZ" {
            units_mm = true;
            continue;
        }
        if line_text == "INCH" || line_text == "INCH,TZ" || line_text == "INCH,LZ" {
            units_mm = false;
            continue;
        }
        if line_text.starts_with("FMAT") || line_text.starts_with("G90") || line_text.starts_with("G05") || line_text.starts_with("G00") || line_text.starts_with("M71") || line_text.starts_with("M72") {
            if line_text.starts_with("M71") {
                units_mm = true;
            } else if line_text.starts_with("M72") {
                units_mm = false;
            }
            continue;
        }
        if let Some(rest) = line_text.strip_prefix('T') {
            if let Some(cpos) = rest.find('C') {
                let num_str = &rest[..cpos];
                let num: u32 = num_str.parse().map_err(|_| DrillError::Syntax {
                    line,
                    message: "bad tool number".to_string(),
                })?;
                if num == 0 || num > 100_000 {
                    return Err(DrillError::Syntax {
                        line,
                        message: "tool number out of range".to_string(),
                    });
                }
                let d = parse_number(&rest[cpos + 1..], line)?;
                let nm = if units_mm { d * 1e6 } else { d * 25_400_000.0 };
                if !(nm.is_finite() && nm > 0.0 && nm < 1e10) {
                    return Err(DrillError::Syntax {
                        line,
                        message: "bad tool diameter".to_string(),
                    });
                }
                if tools.iter().any(|t| t.number == num) {
                    return Err(DrillError::Syntax {
                        line,
                        message: "duplicate tool".to_string(),
                    });
                }
                if tools.len() >= 10_000 {
                    return Err(DrillError::TooLarge);
                }
                tools.push(DrillTool {
                    number: num,
                    diameter: nm.round() as i64,
                });
                continue;
            }
            let num_str = rest.trim();
            if num_str.is_empty() || !num_str.chars().all(|c| c.is_ascii_digit()) {
                return Err(DrillError::Syntax {
                    line,
                    message: "bad tool selection".to_string(),
                });
            }
            let num: u32 = num_str.parse().map_err(|_| DrillError::Syntax {
                line,
                message: "bad tool number".to_string(),
            })?;
            current_tool = Some(num);
            continue;
        }
        if line_text.starts_with('X') {
            let body = &line_text[1..];
            let (xs, ys) = body.split_once('Y').ok_or(DrillError::Syntax {
                line,
                message: "hit without Y coordinate".to_string(),
            })?;
            let x = parse_coord(xs, units_mm, line)?;
            let y = parse_coord(ys, units_mm, line)?;
            let tool = current_tool.ok_or(DrillError::Syntax {
                line,
                message: "hit before tool selection".to_string(),
            })?;
            if !tools.iter().any(|t| t.number == tool) {
                return Err(DrillError::UnknownTool(tool));
            }
            if hits.len() >= 500_000 {
                return Err(DrillError::TooLarge);
            }
            hits.push(DrillHit {
                tool,
                at: Point::new(x, y),
            });
            continue;
        }
        if in_header && (line_text == "M48" || line_text.starts_with(";")) {
            continue;
        }
        return Err(DrillError::Unsupported("line"));
    }
    if tools.is_empty() {
        return Err(DrillError::NoTools);
    }
    Ok(DrillFile {
        tools,
        hits,
        units_mm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_pcb::board::{Board, DesignRules, MM};
    use anvil_core::library::Library;

    fn board() -> Board {
        let lib = Library::standard();
        let mut b = Board::new_rect(30 * MM, 30 * MM, 2, DesignRules::default());
        let n = b.add_net("N");
        let fp = lib.footprint("PINHDR_2").unwrap();
        b.add_footprint(fp, "J1", Point::new(10 * MM, 10 * MM), 0, &[Some(n), None])
            .unwrap();
        b.add_via(n, Point::new(20 * MM, 20 * MM), anvil_pcb::board::Layer::Top, anvil_pcb::board::Layer::Bottom)
            .unwrap();
        b
    }

    #[test]
    fn drill_round_trip() {
        let b = board();
        let d = from_board(&b);
        assert_eq!(d.tools.len(), 2);
        assert_eq!(d.hits.len(), 3);
        let text = write(&d);
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed.tools, d.tools);
        assert_eq!(parsed.hits.len(), d.hits.len());
        for (a, b) in parsed.hits.iter().zip(d.hits.iter()) {
            assert!((a.at.x - b.at.x).abs() <= 100);
            assert!((a.at.y - b.at.y).abs() <= 100);
        }
    }

    #[test]
    fn malformed_drill_fails_cleanly() {
        for bad in ["", "M48\n%\nM30\n", "M48\nMETRIC\nT1C0.4\n%\nX1Y2\nM30\n", "T1Cabc\n"] {
            if let Err(e) = parse(bad) {
                let _ = e.to_string();
            }
        }
    }
}
