//! Gerber rasterizer. Renders the parsed Gerber subset to an 8-bit grayscale
//! image for preview and round-trip verification. Deterministic and bounded.

use crate::gerber::{self, ApertureShape, GerberDoc, GerberError, GerberPrimitive};
use crate::png::{self, PngError};
use anvil_core::geom::{Rect, Point};
use anvil_core::util::limits;

#[derive(Clone, Debug)]
pub struct RasterOptions {
    pub pixels_per_mm: f64,
    pub margin: u32,
    pub max_pixels: u32,
    pub invert: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        RasterOptions {
            pixels_per_mm: 20.0,
            margin: 8,
            max_pixels: limits::MAX_RASTER as u32,
            invert: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    /// Row-major, 8-bit grayscale.
    pub pixels: Vec<u8>,
}

impl RasterImage {
    pub fn to_png(&self) -> Result<Vec<u8>, PngError> {
        png::encode_gray(self.width, self.height, &self.pixels)
    }

    pub fn pixel(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 255;
        }
        self.pixels[(y as usize) * (self.width as usize) + (x as usize)]
    }

    pub fn dark_pixels(&self) -> usize {
        self.pixels.iter().filter(|&&p| p < 128).count()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RasterError {
    Empty,
    BadOptions,
    TooLarge,
    Gerber(GerberError),
    Png(PngError),
}

impl std::fmt::Display for RasterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RasterError::Empty => write!(f, "nothing to rasterize"),
            RasterError::BadOptions => write!(f, "invalid raster options"),
            RasterError::TooLarge => write!(f, "raster image too large"),
            RasterError::Gerber(e) => write!(f, "{e}"),
            RasterError::Png(e) => write!(f, "PNG error: {:?}", e),
        }
    }
}

impl std::error::Error for RasterError {}

impl From<GerberError> for RasterError {
    fn from(e: GerberError) -> Self {
        RasterError::Gerber(e)
    }
}

fn prim_bounds(doc: &GerberDoc) -> Option<Rect> {
    let mut rect: Option<Rect> = None;
    let mut add = |r: Rect| {
        rect = Some(match rect {
            Some(x) => x.union(r),
            None => r,
        });
    };
    for p in &doc.primitives {
        match p {
            GerberPrimitive::Flash { aperture, at } => {
                let (w, h) = doc
                    .aperture(*aperture)
                    .map(|a| (a.width, a.height))
                    .unwrap_or((100_000, 100_000));
                add(Rect::new(
                    Point::new(at.x - w / 2, at.y - h / 2),
                    Point::new(at.x + w / 2, at.y + h / 2),
                ));
            }
            GerberPrimitive::Draw { aperture, from, to } => {
                let w = doc.aperture(*aperture).map(|a| a.width).unwrap_or(100_000);
                let r = Rect::new(*from, *to).expand(w / 2);
                add(r);
            }
        }
    }
    rect
}

struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    scale: f64,
    min: Point,
    margin: f64,
}

impl Canvas {
    fn new(bounds: Rect, scale: f64, margin: u32, max_pixels: u32, invert: bool) -> Result<Canvas, RasterError> {
        if !(scale > 0.0) || !scale.is_finite() {
            return Err(RasterError::BadOptions);
        }
        if bounds.width() <= 0 && bounds.height() <= 0 {
            return Err(RasterError::Empty);
        }
        let mut scale = scale;
        let margin = margin.clamp(1, 200);
        let max_pixels = max_pixels.clamp(16, 8192);
        let mut w = ((bounds.width() as f64 / 1e6) * scale).ceil() + (margin * 2) as f64;
        let mut h = ((bounds.height() as f64 / 1e6) * scale).ceil() + (margin * 2) as f64;
        if w > max_pixels as f64 || h > max_pixels as f64 {
            let factor = (max_pixels as f64 - (margin * 2) as f64)
                / (w.max(h) - (margin * 2) as f64).max(1.0);
            scale *= factor.max(0.01);
            w = ((bounds.width() as f64 / 1e6) * scale).ceil() + (margin * 2) as f64;
            h = ((bounds.height() as f64 / 1e6) * scale).ceil() + (margin * 2) as f64;
        }
        let width = (w.ceil() as u32).max(1);
        let height = (h.ceil() as u32).max(1);
        if width > max_pixels || height > max_pixels {
            return Err(RasterError::TooLarge);
        }
        let bg = if invert { 0 } else { 255 };
        Ok(Canvas {
            width,
            height,
            pixels: vec![bg; (width as usize) * (height as usize)],
            scale,
            min: Point::new(bounds.min.x, bounds.min.y),
            margin: margin as f64,
        })
    }

    fn px(&self, p: Point) -> (f64, f64) {
        let x = (p.x - self.min.x) as f64 / 1e6 * self.scale + self.margin;
        let y = (p.y - self.min.y) as f64 / 1e6 * self.scale + self.margin;
        // Raster y grows downward; Gerber y grows upward.
        (x, self.height as f64 - y)
    }

    fn set(&mut self, x: i64, y: i64, value: u8) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return;
        }
        let idx = (y as usize) * (self.width as usize) + x as usize;
        self.pixels[idx] = value;
    }

    fn fill_circle(&mut self, cx: f64, cy: f64, r: f64, value: u8) {
        let r = r.max(0.5);
        let x0 = (cx - r).floor() as i64;
        let x1 = (cx + r).ceil() as i64;
        let y0 = (cy - r).floor() as i64;
        let y1 = (cy + r).ceil() as i64;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f64 + 0.5 - cx;
                let dy = y as f64 + 0.5 - cy;
                if dx * dx + dy * dy <= r * r {
                    self.set(x, y, value);
                }
            }
        }
    }

    fn fill_rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, value: u8) {
        let ix0 = x0.floor() as i64;
        let ix1 = x1.ceil() as i64;
        let iy0 = y0.floor() as i64;
        let iy1 = y1.ceil() as i64;
        for y in iy0..=iy1 {
            for x in ix0..=ix1 {
                self.set(x, y, value);
            }
        }
    }

    fn thick_line(&mut self, a: (f64, f64), b: (f64, f64), r: f64, value: u8) {
        let r = r.max(0.5);
        let x0 = (a.0.min(b.0) - r).floor() as i64;
        let x1 = (a.0.max(b.0) + r).ceil() as i64;
        let y0 = (a.1.min(b.1) - r).floor() as i64;
        let y1 = (a.1.max(b.1) + r).ceil() as i64;
        let abx = b.0 - a.0;
        let aby = b.1 - a.1;
        let len2 = abx * abx + aby * aby;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let px = x as f64 + 0.5;
                let py = y as f64 + 0.5;
                let mut t = 0.0;
                if len2 > 1e-12 {
                    t = ((px - a.0) * abx + (py - a.1) * aby) / len2;
                    t = t.clamp(0.0, 1.0);
                }
                let dx = px - (a.0 + t * abx);
                let dy = py - (a.1 + t * aby);
                if dx * dx + dy * dy <= r * r {
                    self.set(x, y, value);
                }
            }
        }
    }

    fn flash(&mut self, at: Point, shape: ApertureShape, w_nm: i64, h_nm: i64, value: u8) {
        let (cx, cy) = self.px(at);
        let w = (w_nm as f64 / 1e6 * self.scale).max(1.0);
        let h = (h_nm as f64 / 1e6 * self.scale).max(1.0);
        match shape {
            ApertureShape::Circle => self.fill_circle(cx, cy, w.min(h) / 2.0, value),
            ApertureShape::Rect => {
                self.fill_rect(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0, value)
            }
            ApertureShape::Oval => {
                if w >= h {
                    let r = h / 2.0;
                    let half = (w - h) / 2.0;
                    self.thick_line((cx - half, cy), (cx + half, cy), r, value);
                } else {
                    let r = w / 2.0;
                    let half = (h - w) / 2.0;
                    self.thick_line((cx, cy - half), (cx, cy + half), r, value);
                }
            }
        }
    }
}

/// Render a parsed Gerber document.
pub fn render(doc: &GerberDoc, opts: &RasterOptions) -> Result<RasterImage, RasterError> {
    if !(opts.pixels_per_mm > 0.0) || !opts.pixels_per_mm.is_finite() {
        return Err(RasterError::BadOptions);
    }
    let bounds = prim_bounds(doc).ok_or(RasterError::Empty)?;
    let bounds = bounds.expand(100_000);
    let mut canvas = Canvas::new(
        bounds,
        opts.pixels_per_mm,
        opts.margin,
        opts.max_pixels,
        opts.invert,
    )?;
    let fg = if opts.invert { 255 } else { 0 };
    for p in &doc.primitives {
        match p {
            GerberPrimitive::Flash { aperture, at } => {
                if let Some(a) = doc.aperture(*aperture) {
                    canvas.flash(*at, a.shape, a.width, a.height, fg);
                }
            }
            GerberPrimitive::Draw { aperture, from, to } => {
                let w = doc.aperture(*aperture).map(|a| a.width).unwrap_or(100_000);
                let r = (w as f64 / 1e6 * canvas.scale / 2.0).max(0.5);
                canvas.thick_line(canvas.px(*from), canvas.px(*to), r, fg);
            }
        }
    }
    Ok(RasterImage {
        width: canvas.width,
        height: canvas.height,
        pixels: canvas.pixels,
    })
}

/// Parse and render Gerber text.
pub fn render_text(text: &str, opts: &RasterOptions) -> Result<RasterImage, RasterError> {
    let doc = gerber::parse(text)?;
    render(&doc, opts)
}

/// Render a document with drilled holes punched in white. Holes are in the same
/// nanometre coordinate system as the document.
pub fn render_with_holes(
    doc: &GerberDoc,
    holes: &[(Point, i64)],
    opts: &RasterOptions,
) -> Result<RasterImage, RasterError> {
    let bounds = prim_bounds(doc).ok_or(RasterError::Empty)?;
    let mut b = bounds.expand(100_000);
    for (at, d) in holes {
        b = b.union(Rect::new(
            Point::new(at.x - d / 2, at.y - d / 2),
            Point::new(at.x + d / 2, at.y + d / 2),
        ));
    }
    let mut canvas = Canvas::new(
        b,
        opts.pixels_per_mm,
        opts.margin,
        opts.max_pixels,
        opts.invert,
    )?;
    let fg = if opts.invert { 255 } else { 0 };
    let bg = if opts.invert { 0 } else { 255 };
    for p in &doc.primitives {
        match p {
            GerberPrimitive::Flash { aperture, at } => {
                if let Some(a) = doc.aperture(*aperture) {
                    canvas.flash(*at, a.shape, a.width, a.height, fg);
                }
            }
            GerberPrimitive::Draw { aperture, from, to } => {
                let w = doc.aperture(*aperture).map(|a| a.width).unwrap_or(100_000);
                let r = (w as f64 / 1e6 * canvas.scale / 2.0).max(0.5);
                canvas.thick_line(canvas.px(*from), canvas.px(*to), r, fg);
            }
        }
    }
    for (at, d) in holes {
        let (cx, cy) = canvas.px(*at);
        let r = (*d as f64 / 1e6 * canvas.scale / 2.0).max(1.0);
        canvas.fill_circle(cx, cy, r, bg);
    }
    Ok(RasterImage {
        width: canvas.width,
        height: canvas.height,
        pixels: canvas.pixels,
    })
}

/// Choose a scale that roughly fills `target_width` pixels.
pub fn fit_to_width(doc: &GerberDoc, target_width: u32) -> Option<RasterOptions> {
    // Match `render`, which expands the primitive bounds by 0.1 mm.
    let b = prim_bounds(doc)?.expand(100_000);
    if b.width() <= 0 {
        return None;
    }
    let margin = 10u32;
    let target = target_width.clamp(64, limits::MAX_RASTER as u32);
    let scale = (target.saturating_sub(margin * 2) as f64) / (b.width() as f64 / 1e6);
    if !(scale > 0.0) || !scale.is_finite() {
        return None;
    }
    Some(RasterOptions {
        pixels_per_mm: scale,
        margin,
        max_pixels: limits::MAX_RASTER as u32,
        invert: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gerber;

    const SIMPLE: &str = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.200000*%\n%ADD11R,1.000000X1.500000*%\nD10*\nX1000000Y1000000D02*\nX5000000Y1000000D01*\nD11*\nX3000000Y3000000D03*\nM02*\n";

    #[test]
    fn raster_has_copper() {
        let doc = gerber::parse(SIMPLE).unwrap();
        let img = render(&doc, &RasterOptions { pixels_per_mm: 20.0, ..Default::default() }).unwrap();
        assert!(img.width > 100 && img.height > 50);
        assert!(img.dark_pixels() > 50);
        assert!(img.pixel(0, 0) == 255);
    }

    #[test]
    fn render_is_deterministic() {
        let doc1 = gerber::parse(SIMPLE).unwrap();
        let doc2 = gerber::parse(SIMPLE).unwrap();
        let a = render(&doc1, &RasterOptions::default()).unwrap();
        let b = render(&doc2, &RasterOptions::default()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn drilled_holes_are_white() {
        let doc = gerber::parse(SIMPLE).unwrap();
        let mut opts = RasterOptions::default();
        opts.pixels_per_mm = 10.0;
        let img = render(&doc, &opts).unwrap();
        let hole = render_with_holes(&doc, &[(anvil_core::geom::Point::new(3_000_000, 3_000_000), 1_500_000)], &opts).unwrap();
        assert_eq!(img.width, hole.width);
        assert!(hole.dark_pixels() < img.dark_pixels());
    }

    #[test]
    fn empty_document_errors() {
        let doc = GerberDoc {
            apertures: Vec::new(),
            primitives: Vec::new(),
            units_mm: true,
            decimals: 6,
        };
        assert!(matches!(render(&doc, &RasterOptions::default()), Err(RasterError::Empty)));
    }

    #[test]
    fn png_encoding_works() {
        let doc = gerber::parse(SIMPLE).unwrap();
        let img = render(&doc, &RasterOptions::default()).unwrap();
        let png = img.to_png().unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    }
}
