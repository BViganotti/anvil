//! Raster options, fitting, inversions and drilling.

use anvil_core::geom::Point;
use anvil_export::gerber;
use anvil_export::raster::{self, RasterError, RasterOptions};

const SIMPLE: &str = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.200000*%\n%ADD11R,1.000000X1.500000*%\nD10*\nX0Y0D02*\nX10000000Y0D01*\nD11*\nX5000000Y5000000D03*\nM02*\n";

#[test]
fn fit_to_width_respects_margin() {
    let doc = gerber::parse(SIMPLE).unwrap();
    let opts = raster::fit_to_width(&doc, 800).unwrap();
    let img = raster::render(&doc, &opts).unwrap();
    assert!(img.width <= 800 && img.width > 700, "{}", img.width);
    assert!(img.dark_pixels() > 100);
}

#[test]
fn invert_swaps_foreground_and_background() {
    let doc = gerber::parse(SIMPLE).unwrap();
    let normal = raster::render(&doc, &RasterOptions::default()).unwrap();
    let mut inv = RasterOptions::default();
    inv.invert = true;
    let inverted = raster::render(&doc, &inv).unwrap();
    assert_eq!(normal.width, inverted.width);
    assert_eq!(normal.dark_pixels(), inverted.pixels.iter().filter(|&&p| p > 127).count());
}

#[test]
fn holes_reduce_dark_pixels() {
    let doc = gerber::parse(SIMPLE).unwrap();
    let opts = RasterOptions {
        pixels_per_mm: 12.0,
        ..Default::default()
    };
    let a = raster::render(&doc, &opts).unwrap();
    let b = raster::render_with_holes(&doc, &[(Point::new(5_000_000, 5_000_000), 2_000_000)], &opts).unwrap();
    assert!(b.dark_pixels() < a.dark_pixels());
}

#[test]
fn bad_options_are_rejected() {
    let doc = gerber::parse(SIMPLE).unwrap();
    let bad = RasterOptions {
        pixels_per_mm: 0.0,
        ..Default::default()
    };
    assert_eq!(raster::render(&doc, &bad), Err(RasterError::BadOptions));
    let bad = RasterOptions {
        pixels_per_mm: 1.0,
        max_pixels: 16,
        ..Default::default()
    };
    let r = raster::render(&doc, &bad);
    assert!(matches!(r, Ok(_) | Err(RasterError::TooLarge)));
}

#[test]
fn render_is_deterministic_and_png_works() {
    let doc = gerber::parse(SIMPLE).unwrap();
    let a = raster::render(&doc, &RasterOptions::default()).unwrap();
    let b = raster::render(&doc, &RasterOptions::default()).unwrap();
    assert_eq!(a, b);
    let png = a.to_png().unwrap();
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
}

#[test]
fn empty_and_malformed_documents() {
    let empty = gerber::GerberDoc {
        apertures: Vec::new(),
        primitives: Vec::new(),
        units_mm: true,
        decimals: 6,
    };
    assert_eq!(
        raster::render(&empty, &RasterOptions::default()),
        Err(RasterError::Empty)
    );
    assert!(raster::render_text("not gerber", &RasterOptions::default()).is_err());
}
