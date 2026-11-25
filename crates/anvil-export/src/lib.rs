//! Gerber, Excellon and raster export for Anvil.
//!
//! Every file is generated from real board geometry. The preview path parses
//! the generated Gerber back before rasterizing, so a preview failure exposes a
//! writer bug instead of hiding it.

pub mod drill;
pub mod gerber;
pub mod png;
pub mod raster;

use anvil_core::geom::Point;
use anvil_pcb::board::{Board, Layer};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl ExportFile {
    pub fn text(name: impl Into<String>, text: String) -> Self {
        ExportFile {
            name: name.into(),
            bytes: text.into_bytes(),
        }
    }

    pub fn is_utf8(&self) -> bool {
        std::str::from_utf8(&self.bytes).is_ok()
    }

    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.bytes).ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportError {
    Gerber(String),
    Drill(String),
    Raster(String),
    Png(String),
    Empty,
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Gerber(s) => write!(f, "Gerber error: {s}"),
            ExportError::Drill(s) => write!(f, "drill error: {s}"),
            ExportError::Raster(s) => write!(f, "raster error: {s}"),
            ExportError::Png(s) => write!(f, "PNG error: {s}"),
            ExportError::Empty => write!(f, "board has nothing to export"),
        }
    }
}

impl std::error::Error for ExportError {}

/// Filename base for a board, shared by Gerber and drill output.
pub fn file_base(board: &Board) -> String {
    let mut s: String = board
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').to_string();
    if s.is_empty() {
        "board".to_string()
    } else if s.len() > 80 {
        s[..80].to_string()
    } else {
        s
    }
}

/// All Gerber and drill files for a board.
pub fn export_board(board: &Board) -> Vec<ExportFile> {
    let base = file_base(board);
    let mut out: Vec<ExportFile> = gerber::write_board(board)
        .into_iter()
        .map(|(name, text)| ExportFile::text(with_base(&base, &name), text))
        .collect();
    let drill = drill::write_board(board);
    out.push(ExportFile::text(format!("{}.drl", base), drill));
    out
}

fn with_base(base: &str, name: &str) -> String {
    // gerber::write_board already uses the sanitised board name as its base; if
    // it differs because of collisions, normalise both sides.
    if name.starts_with(base) {
        name.to_string()
    } else if let Some(pos) = name.find("-F_Cu") {
        format!("{}{}", base, &name[pos..])
    } else if let Some(pos) = name.find("-B_Cu") {
        format!("{}{}", base, &name[pos..])
    } else if let Some(pos) = name.find("-Edge") {
        format!("{}{}", base, &name[pos..])
    } else if let Some(pos) = name.find("-Silk") {
        format!("{}{}", base, &name[pos..])
    } else if let Some(pos) = name.find("-In1_Cu") {
        format!("{}{}", base, &name[pos..])
    } else if let Some(pos) = name.find("-In2_Cu") {
        format!("{}{}", base, &name[pos..])
    } else {
        format!("{}-{}", base, name)
    }
}

pub fn layer_text(board: &Board, layer: Layer) -> String {
    gerber::write_copper(board, layer)
}

pub fn outline_text(board: &Board) -> String {
    gerber::write_outline(board)
}

pub fn silkscreen_text(board: &Board) -> String {
    gerber::write_silkscreen(board)
}

pub fn drill_text(board: &Board) -> String {
    drill::write_board(board)
}

fn drilled_holes(board: &Board) -> Vec<(Point, i64)> {
    let file = drill::from_board(board);
    file.hits
        .iter()
        .filter_map(|h| file.tool(h.tool).map(|t| (h.at, t.diameter)))
        .collect()
}

/// Render a PNG preview of top copper with real drilled holes punched out.
pub fn preview_png(board: &Board, target_width: u32) -> Result<Vec<u8>, ExportError> {
    if board.pad_count() == 0 && board.traces.is_empty() && board.vias.is_empty() {
        return Err(ExportError::Empty);
    }
    let text = gerber::write_copper(board, Layer::Top);
    let doc = gerber::parse(&text).map_err(|e| ExportError::Gerber(e.to_string()))?;
    if doc.primitives.is_empty() {
        return Err(ExportError::Empty);
    }
    let opts = raster::fit_to_width(&doc, target_width)
        .ok_or_else(|| ExportError::Raster("cannot fit preview".to_string()))?;
    let img = raster::render_with_holes(&doc, &drilled_holes(board), &opts)
        .map_err(|e| ExportError::Raster(e.to_string()))?;
    img.to_png().map_err(|e| ExportError::Png(format!("{:?}", e)))
}

/// Render bottom copper as PNG.
pub fn preview_bottom_png(board: &Board, target_width: u32) -> Result<Vec<u8>, ExportError> {
    let text = gerber::write_copper(board, Layer::Bottom);
    let doc = gerber::parse(&text).map_err(|e| ExportError::Gerber(e.to_string()))?;
    if doc.primitives.is_empty() {
        return Err(ExportError::Empty);
    }
    let opts = raster::fit_to_width(&doc, target_width)
        .ok_or_else(|| ExportError::Raster("cannot fit preview".to_string()))?;
    let img = raster::render_with_holes(&doc, &drilled_holes(board), &opts)
        .map_err(|e| ExportError::Raster(e.to_string()))?;
    img.to_png().map_err(|e| ExportError::Png(format!("{:?}", e)))
}

/// Rasterize a Gerber string, returning the image (used by tests and Studio).
pub fn rasterize_gerber(text: &str, pixels_per_mm: f64) -> Result<raster::RasterImage, ExportError> {
    let opts = raster::RasterOptions {
        pixels_per_mm,
        ..Default::default()
    };
    raster::render_text(text, &opts).map_err(|e| ExportError::Raster(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::library::Library;
    use anvil_pcb::board::{DesignRules, MM};

    fn routed_board() -> Board {
        let lib = Library::standard();
        let mut b = Board::new_rect(40 * MM, 30 * MM, 2, DesignRules::default());
        let n = b.add_net("N");
        let fp = lib.footprint("R_0805").unwrap();
        b.add_footprint(fp, "R1", Point::new(10 * MM, 10 * MM), 0, &[Some(n), Some(n)])
            .unwrap();
        b.add_footprint(fp, "R2", Point::new(25 * MM, 18 * MM), 0, &[Some(n), Some(n)])
            .unwrap();
        b.add_trace(
            n,
            Layer::Top,
            200_000,
            vec![Point::new(10 * MM, 10 * MM), Point::new(25 * MM, 18 * MM)],
        )
        .unwrap();
        b
    }

    #[test]
    fn exports_round_trip() {
        let mut b = routed_board();
        b.add_via(
            b.footprints[0].pads[0].net.unwrap(),
            Point::new(20 * MM, 10 * MM),
            Layer::Top,
            Layer::Bottom,
        )
        .unwrap();
        let files = export_board(&b);
        assert!(files.len() >= 5);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert!(names.iter().any(|n| n.ends_with(".drl")));
        assert!(names.iter().any(|n| n.ends_with("-F_Cu.gbr")));
        for f in &files {
            let text = f.as_str().unwrap();
            assert!(!text.is_empty());
            if f.name.ends_with(".gbr") {
                let doc = gerber::parse(text).unwrap();
                if f.name.ends_with("-F_Cu.gbr") {
                    assert!(!doc.primitives.is_empty());
                }
            } else {
                let d = drill::parse(text).unwrap();
                assert!(!d.tools.is_empty());
            }
        }
    }

    #[test]
    fn preview_is_png() {
        let b = routed_board();
        let png = preview_png(&b, 600).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        // Bottom has no copper on this board, so the preview reports Empty.
        assert_eq!(preview_bottom_png(&b, 600), Err(ExportError::Empty));
    }

    #[test]
    fn gerber_reparse_rasterizes_identically() {
        let b = routed_board();
        let text = gerber::write_copper(&b, Layer::Top);
        let a = rasterize_gerber(&text, 15.0).unwrap();
        let b2 = rasterize_gerber(&text, 15.0).unwrap();
        assert_eq!(a, b2);
        assert!(a.dark_pixels() > 100);
    }

    #[test]
    fn empty_board_preview_errors() {
        let b = Board::new_rect(10 * MM, 10 * MM, 2, DesignRules::default());
        assert_eq!(preview_png(&b, 400), Err(ExportError::Empty));
    }
}
