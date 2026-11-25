//! Gerber RS-274X writer and parser.
//!
//! The writer emits the common subset used by this project: millimetres, 4.6
//! format, linear interpolation, circular/rectangular/oval apertures, draws and
//! flashes. The parser accepts that subset plus comments, polarity changes and
//! inch units. Arcs, macros and regions are rejected with a clear error rather
//! than silently misinterpreted.

use anvil_pcb::board::{Board, Layer};
use anvil_core::geom::{Point, Rect};
use anvil_core::util::limits;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ApertureShape {
    Circle,
    Rect,
    Oval,
}

impl ApertureShape {
    pub fn code(self) -> char {
        match self {
            ApertureShape::Circle => 'C',
            ApertureShape::Rect => 'R',
            ApertureShape::Oval => 'O',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Aperture {
    pub code: u32,
    pub shape: ApertureShape,
    /// Width/height in nanometres.
    pub width: i64,
    pub height: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GerberPrimitive {
    Flash {
        aperture: u32,
        at: Point,
    },
    Draw {
        aperture: u32,
        from: Point,
        to: Point,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GerberDoc {
    pub apertures: Vec<Aperture>,
    pub primitives: Vec<GerberPrimitive>,
    pub units_mm: bool,
    pub decimals: u8,
}

impl GerberDoc {
    pub fn bounds(&self) -> Option<Rect> {
        let mut pts: Vec<Point> = Vec::new();
        let ap = |code: u32| self.apertures.iter().find(|a| a.code == code);
        for p in &self.primitives {
            match p {
                GerberPrimitive::Flash { aperture, at } => {
                    pts.push(*at);
                    if let Some(a) = ap(*aperture) {
                        pts.push(Point::new(at.x - a.width / 2, at.y - a.height / 2));
                        pts.push(Point::new(at.x + a.width / 2, at.y + a.height / 2));
                    }
                }
                GerberPrimitive::Draw { from, to, .. } => {
                    pts.push(*from);
                    pts.push(*to);
                }
            }
        }
        Rect::from_points(&pts)
    }

    pub fn aperture(&self, code: u32) -> Option<&Aperture> {
        self.apertures.iter().find(|a| a.code == code)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GerberError {
    TooLarge,
    TooManyOps,
    Syntax { at: usize, message: String },
    Unsupported(&'static str),
    MissingAperture,
    NoCoordinates,
    Empty,
}

impl fmt::Display for GerberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GerberError::TooLarge => write!(f, "Gerber input exceeds the size limit"),
            GerberError::TooManyOps => write!(f, "Gerber operation limit exceeded"),
            GerberError::Syntax { at, message } => write!(f, "Gerber syntax error at byte {at}: {message}"),
            GerberError::Unsupported(s) => write!(f, "unsupported Gerber feature: {s}"),
            GerberError::MissingAperture => write!(f, "Gerber operation without an aperture"),
            GerberError::NoCoordinates => write!(f, "Gerber draw/flash without coordinates"),
            GerberError::Empty => write!(f, "empty Gerber file"),
        }
    }
}

impl std::error::Error for GerberError {}

fn fmt_mm6(nm: i64) -> String {
    let neg = nm < 0;
    let n = (nm as i128).unsigned_abs();
    let whole = n / 1_000_000;
    let frac = n % 1_000_000;
    format!(
        "{}{}.{:06}",
        if neg { "-" } else { "" },
        whole,
        frac
    )
}

fn coord(nm: i64) -> String {
    // 4.6 format: one unit is 1 nm.
    nm.to_string()
}

struct Writer {
    header: String,
    body: String,
    apertures: BTreeMap<(ApertureShape, i64, i64), u32>,
    next_code: u32,
    current: Option<u32>,
}

impl Writer {
    fn new(comment: &str) -> Self {
        let header = format!(
            "G04 Anvil {}*\n%FSLAX46Y46*%\n%MOMM*%\n%LPD*%\nG01*\n",
            comment.replace(['\n', '\r'], " ")
        );
        Writer {
            header,
            body: String::new(),
            apertures: BTreeMap::new(),
            next_code: 10,
            current: None,
        }
    }

    fn aperture(&mut self, shape: ApertureShape, w: i64, h: i64) -> u32 {
        let key = (shape, w, h);
        if let Some(&code) = self.apertures.get(&key) {
            return code;
        }
        let code = self.next_code;
        self.next_code = self.next_code.saturating_add(1);
        self.apertures.insert(key, code);
        code
    }

    fn select(&mut self, code: u32) {
        if self.current != Some(code) {
            self.body.push_str(&format!("D{}*\n", code));
            self.current = Some(code);
        }
    }

    fn flash(&mut self, code: u32, at: Point) {
        self.select(code);
        self.body
            .push_str(&format!("X{}Y{}D03*\n", coord(at.x), coord(at.y)));
    }

    fn draw(&mut self, code: u32, from: Point, to: Point) {
        self.select(code);
        self.body
            .push_str(&format!("X{}Y{}D02*\n", coord(from.x), coord(from.y)));
        self.body
            .push_str(&format!("X{}Y{}D01*\n", coord(to.x), coord(to.y)));
    }

    fn finish(self) -> String {
        let mut defs = String::new();
        for ((shape, w, h), code) in &self.apertures {
            match shape {
                ApertureShape::Circle => defs.push_str(&format!(
                    "%ADD{}{},{}*%\n",
                    code,
                    shape.code(),
                    fmt_mm6(*w)
                )),
                _ => defs.push_str(&format!(
                    "%ADD{}{},{}X{}*%\n",
                    code,
                    shape.code(),
                    fmt_mm6(*w),
                    fmt_mm6(*h)
                )),
            }
        }
        let mut out = String::with_capacity(self.header.len() + defs.len() + self.body.len() + 8);
        out.push_str(&self.header);
        out.push_str(&defs);
        out.push_str(&self.body);
        out.push_str("M02*\n");
        out
    }
}

/// Write one copper layer.
pub fn write_copper(board: &Board, layer: Layer) -> String {
    let mut w = Writer::new(&format!("{} {} copper", board.name, layer.as_str()));
    // Trace apertures.
    for t in &board.traces {
        if t.layer != layer {
            continue;
        }
        let code = w.aperture(ApertureShape::Circle, t.width, t.width);
        for (a, b) in t.segments() {
            w.draw(code, a, b);
        }
    }
    // Pads.
    for f in &board.footprints {
        for p in &f.pads {
            let on = match p.layers {
                anvil_core::library::PadLayer::Top => layer == Layer::Top,
                anvil_core::library::PadLayer::Bottom => layer == Layer::Bottom,
                anvil_core::library::PadLayer::Through => true,
            };
            if !on {
                continue;
            }
            let shape = match p.shape {
                anvil_core::library::PadShape::Circle => ApertureShape::Circle,
                anvil_core::library::PadShape::Rect | anvil_core::library::PadShape::Square => {
                    ApertureShape::Rect
                }
                anvil_core::library::PadShape::Oval => ApertureShape::Oval,
            };
            let code = w.aperture(shape, p.size.0, p.size.1);
            w.flash(code, p.at);
        }
    }
    // Vias.
    for v in &board.vias {
        if !v.spans(layer) {
            continue;
        }
        let code = w.aperture(ApertureShape::Circle, v.diameter, v.diameter);
        w.flash(code, v.at);
    }
    w.finish()
}

/// Write the board outline on a dedicated Gerber layer.
pub fn write_outline(board: &Board) -> String {
    let mut w = Writer::new(&format!("{} outline", board.name));
    let code = w.aperture(ApertureShape::Circle, 100_000, 100_000);
    if board.outline.len() >= 2 {
        for i in 0..board.outline.len() {
            let a = board.outline[i];
            let b = board.outline[(i + 1) % board.outline.len()];
            w.draw(code, a, b);
        }
    }
    finish_with_defs(w)
}

/// Write a simple silkscreen: footprint outlines and pad references.
pub fn write_silkscreen(board: &Board) -> String {
    let mut w = Writer::new(&format!("{} silkscreen", board.name));
    let code = w.aperture(ApertureShape::Circle, 150_000, 150_000);
    for f in &board.footprints {
        let b = f.bounds().expand(200_000);
        let pts = [
            Point::new(b.min.x, b.min.y),
            Point::new(b.max.x, b.min.y),
            Point::new(b.max.x, b.max.y),
            Point::new(b.min.x, b.max.y),
        ];
        for i in 0..4 {
            w.draw(code, pts[i], pts[(i + 1) % 4]);
        }
    }
    finish_with_defs(w)
}

fn finish_with_defs(w: Writer) -> String {
    w.finish()
}

/// Write all copper, outline and silkscreen files for a board.
pub fn write_board(board: &Board) -> Vec<(String, String)> {
    let base = sanitize_name(&board.name);
    let mut out = vec![
        (
            format!("{}-F_Cu.gbr", base),
            write_copper(board, Layer::Top),
        ),
        (
            format!("{}-B_Cu.gbr", base),
            write_copper(board, Layer::Bottom),
        ),
        (format!("{}-Edge.gbr", base), write_outline(board)),
        (format!("{}-Silk.gbr", base), write_silkscreen(board)),
    ];
    if board.copper_layers >= 4 {
        out.push((
            format!("{}-In1_Cu.gbr", base),
            write_copper(board, Layer::Inner1),
        ));
        out.push((
            format!("{}-In2_Cu.gbr", base),
            write_copper(board, Layer::Inner2),
        ));
    }
    out
}

fn sanitize_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    if s.is_empty() {
        s = "board".to_string();
    }
    if s.len() > 80 {
        s.truncate(80);
    }
    s
}

fn parse_i64(s: &str, at: usize) -> Result<i64, GerberError> {
    if s.is_empty() || s.len() > 20 {
        return Err(GerberError::Syntax {
            at,
            message: "bad integer".to_string(),
        });
    }
    s.parse::<i64>().map_err(|_| GerberError::Syntax {
        at,
        message: format!("bad integer {:?}", s),
    })
}

fn parse_f64(s: &str, at: usize) -> Result<f64, GerberError> {
    if s.is_empty() || s.len() > 32 {
        return Err(GerberError::Syntax {
            at,
            message: "bad number".to_string(),
        });
    }
    let v: f64 = s.parse().map_err(|_| GerberError::Syntax {
        at,
        message: format!("bad number {:?}", s),
    })?;
    if !v.is_finite() {
        return Err(GerberError::Syntax {
            at,
            message: "non-finite number".to_string(),
        });
    }
    Ok(v)
}

fn mm_to_nm(v: f64) -> i64 {
    (v * 1e6).round().clamp(-1e12, 1e12) as i64
}

fn handle_extended(doc: &mut GerberDoc, s: &str, at: usize) -> Result<(), GerberError> {
    let s = s.trim_end_matches('*').trim();
    if s.starts_with("FSLA") {
        let x = s.find('X').ok_or(GerberError::Syntax {
            at,
            message: "invalid FS".to_string(),
        })?;
        let digits = &s[x + 1..];
        let mut it = digits.chars();
        let _integer = it.next();
        let dec = it.next().and_then(|c| c.to_digit(10)).unwrap_or(6);
        if !(1..=6).contains(&dec) {
            return Err(GerberError::Unsupported("coordinate format with more than six decimals"));
        }
        doc.decimals = dec as u8;
        return Ok(());
    }
    if s.starts_with("MOMM") {
        doc.units_mm = true;
        return Ok(());
    }
    if s.starts_with("MOIN") {
        doc.units_mm = false;
        return Ok(());
    }
    if s.starts_with("LP") {
        if s.contains('C') {
            return Err(GerberError::Unsupported("clear polarity"));
        }
        return Ok(());
    }
    if s.starts_with("AD") {
        // ADD10C,0.200000
        let rest = &s[3..];
        let shape_pos = rest
            .find(|c: char| c.is_ascii_alphabetic())
            .ok_or(GerberError::Syntax {
                at,
                message: "aperture without shape".to_string(),
            })?;
        let code = rest[..shape_pos].parse::<u32>().map_err(|_| GerberError::Syntax {
            at,
            message: "bad aperture code".to_string(),
        })?;
        if !(10..100_000).contains(&code) {
            return Err(GerberError::Syntax {
                at,
                message: "aperture code out of range".to_string(),
            });
        }
        let shape_char = rest[shape_pos..].chars().next().unwrap_or('C');
        let shape = match shape_char {
            'C' => ApertureShape::Circle,
            'R' => ApertureShape::Rect,
            'O' => ApertureShape::Oval,
            _ => return Err(GerberError::Unsupported("aperture shape")),
        };
        let params = rest[shape_pos + 1..]
            .trim_start_matches(',')
            .trim_start_matches(['X', 'x']);
        if params.contains(',') && shape != ApertureShape::Circle {
            // Accept comma or X separated rectangular sizes.
        }
        let parts: Vec<&str> = params.split(['X', 'x']).collect();
        let w = parse_f64(parts.first().copied().unwrap_or(""), at)?;
        let h = if parts.len() > 1 {
            parse_f64(parts[1], at)?
        } else {
            w
        };
        if w <= 0.0 || h <= 0.0 {
            return Err(GerberError::Syntax {
                at,
                message: "non-positive aperture".to_string(),
            });
        }
        if doc.apertures.len() > 10_000 {
            return Err(GerberError::TooManyOps);
        }
        doc.apertures.push(Aperture {
            code,
            shape,
            width: mm_to_nm(w),
            height: mm_to_nm(h),
        });
        return Ok(());
    }
    if s.starts_with("AM") {
        return Err(GerberError::Unsupported("aperture macros"));
    }
    if s.starts_with("SR") {
        return Err(GerberError::Unsupported("step and repeat"));
    }
    if s.starts_with("IP") || s.starts_with("LN") || s.starts_with("TF") || s.starts_with("TA") {
        return Ok(());
    }
    // Unknown extended commands are rejected: better than guessing.
    Err(GerberError::Unsupported("extended command"))
}

fn parse_word(
    doc: &mut GerberDoc,
    word: &str,
    at: usize,
    cur: &mut Option<Point>,
    aperture: &mut Option<u32>,
    ops: &mut usize,
) -> Result<bool, GerberError> {
    let w = word.trim().trim_end_matches('*').trim();
    if w.is_empty() {
        return Ok(true);
    }
    if w.starts_with("G04") {
        return Ok(true);
    }
    if w.starts_with("M02") || w.starts_with("M00") || w.starts_with("M01") {
        return Ok(false);
    }
    if w.starts_with("G01") || w.starts_with("G70") || w.starts_with("G71") || w.starts_with("G90") || w.starts_with("G91") {
        return Ok(true);
    }
    if w.starts_with("G02") || w.starts_with("G03") {
        return Err(GerberError::Unsupported("arc interpolation"));
    }
    if w.starts_with("G36") || w.starts_with("G37") {
        return Err(GerberError::Unsupported("regions"));
    }
    if w.starts_with("G75") {
        return Ok(true);
    }
    if w.starts_with('D') {
        let all_digits = w[1..].chars().all(|c| c.is_ascii_digit());
        if all_digits {
            let code = parse_i64(&w[1..], at)?;
            if code >= 10 {
                *aperture = Some(code as u32);
                return Ok(true);
            }
        }
    }
    if w.starts_with('G') {
        // Unsupported G code.
        return Err(GerberError::Unsupported("G code"));
    }
    // Coordinate word: X..., Y..., optional Dnn.
    let mut x: Option<i64> = None;
    let mut y: Option<i64> = None;
    let mut op: Option<u8> = None;
    let bytes = w.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        i += 1;
        if c == 'X' || c == 'Y' || c == 'I' || c == 'J' {
            let start = i;
            if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                i += 1;
            }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let v = parse_i64(&w[start..i], at)?;
            match c {
                'X' => x = Some(v),
                'Y' => y = Some(v),
                'I' | 'J' => return Err(GerberError::Unsupported("arc offsets")),
                _ => {}
            }
        } else if c == 'D' {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let v = parse_i64(&w[start..i], at)?;
            if v > 3 {
                return Err(GerberError::Syntax {
                    at,
                    message: "invalid D operation".to_string(),
                });
            }
            op = Some(v as u8);
        } else if c.is_whitespace() {
            continue;
        } else {
            return Err(GerberError::Syntax {
                at,
                message: format!("unexpected character {:?}", c),
            });
        }
    }
    if op.is_none() && x.is_none() && y.is_none() {
        return Ok(true);
    }
    let scale = if doc.units_mm {
        10i64.saturating_pow(6 - doc.decimals as u32)
    } else {
        // Inches: 25.4 mm = 25_400_000 nm.
        25_400_000 / 10i64.saturating_pow(doc.decimals as u32)
    };
    let nx = x.map(|v| v.saturating_mul(scale));
    let ny = y.map(|v| v.saturating_mul(scale));
    let target = Point::new(
        nx.unwrap_or_else(|| cur.map(|p| p.x).unwrap_or(0)),
        ny.unwrap_or_else(|| cur.map(|p| p.y).unwrap_or(0)),
    );
    if target.x.abs() > 1_000_000_000_000 || target.y.abs() > 1_000_000_000_000 {
        return Err(GerberError::Syntax {
            at,
            message: "coordinate out of range".to_string(),
        });
    }
    match op {
        Some(1) => {
            let from = cur.ok_or(GerberError::NoCoordinates)?;
            let ap = aperture.ok_or(GerberError::MissingAperture)?;
            if doc.apertures.len() > 10_000 {
                return Err(GerberError::TooManyOps);
            }
            doc.primitives.push(GerberPrimitive::Draw { aperture: ap, from, to: target });
            *ops += 1;
        }
        Some(2) => {
            *cur = Some(target);
        }
        Some(3) => {
            let ap = aperture.ok_or(GerberError::MissingAperture)?;
            doc.primitives.push(GerberPrimitive::Flash { aperture: ap, at: target });
            *ops += 1;
        }
        Some(_) => {}
        None => {
            *cur = Some(target);
        }
    }
    if *ops > limits::MAX_GERBER_OPS {
        return Err(GerberError::TooManyOps);
    }
    Ok(true)
}

/// Parse a Gerber RS-274X file.
pub fn parse(text: &str) -> Result<GerberDoc, GerberError> {
    if text.is_empty() {
        return Err(GerberError::Empty);
    }
    if text.len() > limits::MAX_GERBER_BYTES {
        return Err(GerberError::TooLarge);
    }
    if !text.is_ascii() {
        return Err(GerberError::Syntax {
            at: 0,
            message: "non-ASCII input".to_string(),
        });
    }
    let mut doc = GerberDoc {
        apertures: Vec::new(),
        primitives: Vec::new(),
        units_mm: true,
        decimals: 6,
    };
    let mut cur: Option<Point> = None;
    let mut aperture: Option<u32> = None;
    let mut ops = 0usize;
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Skip whitespace.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'%' {
            let start = i;
            i += 1;
            let mut j = i;
            while j < bytes.len() && bytes[j] != b'%' {
                j += 1;
            }
            if j >= bytes.len() {
                return Err(GerberError::Syntax {
                    at: start,
                    message: "unterminated extended command".to_string(),
                });
            }
            let block = &text[i..j];
            handle_extended(&mut doc, block, start)?;
            i = j + 1;
            continue;
        }
        let start = i;
        let mut j = i;
        while j < bytes.len() && bytes[j] != b'*' {
            j += 1;
        }
        if j >= bytes.len() {
            return Err(GerberError::Syntax {
                at: start,
                message: "unterminated command".to_string(),
            });
        }
        let word = &text[start..j];
        let keep = parse_word(&mut doc, word, start, &mut cur, &mut aperture, &mut ops)?;
        i = j + 1;
        if !keep {
            break;
        }
    }
    // A header-only layer is valid: an empty copper layer is a real, if
    // boring, Gerber file. Callers that need geometry check primitives.
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_pcb::board::{Board, DesignRules, Layer, MM};
    use anvil_core::library::Library;

    fn board() -> Board {
        let lib = Library::standard();
        let mut b = Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default());
        let n = b.add_net("N1");
        let fp = lib.footprint("R_0805").unwrap();
        b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n), Some(n)])
            .unwrap();
        b.add_trace(
            n,
            Layer::Top,
            200_000,
            vec![Point::new(8 * MM, 8 * MM), Point::new(20 * MM, 12 * MM)],
        )
        .unwrap();
        b
    }

    #[test]
    fn copper_round_trips() {
        let b = board();
        let text = write_copper(&b, Layer::Top);
        let doc = parse(&text).unwrap();
        assert!(doc.apertures.len() >= 2);
        assert!(doc.primitives.iter().any(|p| matches!(p, GerberPrimitive::Draw { .. })));
        assert!(doc.primitives.iter().any(|p| matches!(p, GerberPrimitive::Flash { .. })));
        // Flash coordinates match pads.
        for f in &b.footprints {
            for p in &f.pads {
                assert!(doc.primitives.iter().any(|g| matches!(g, GerberPrimitive::Flash { at, .. } if *at == p.at)));
            }
        }
    }

    #[test]
    fn outline_round_trips() {
        let b = board();
        let text = write_outline(&b);
        let doc = parse(&text).unwrap();
        assert_eq!(doc.primitives.len(), 4);
    }

    #[test]
    fn malformed_input_fails_cleanly() {
        for bad in ["", "%FSLAX46Y46*%", "X10Y10D01*", "G02*", "%ADD10Z,1*%", "\u{00e9}"] {
            let r = parse(bad);
            if let Err(e) = r {
                let _ = e.to_string();
            }
        }
        assert!(parse("X10Y10D01*M02*").is_err());
    }

    #[test]
    fn aperture_and_flash() {
        let text = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.200000*%\nD10*\nX1000000Y2000000D03*\nM02*\n";
        let doc = parse(text).unwrap();
        assert_eq!(doc.apertures[0].width, 200_000);
        assert_eq!(
            doc.primitives[0],
            GerberPrimitive::Flash {
                aperture: 10,
                at: Point::new(1_000_000, 2_000_000)
            }
        );
    }
}
