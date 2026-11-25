//! Regression tests for bugs found during development.

use anvil_export::gerber::{parse as parse_gerber, GerberPrimitive};
use anvil_export::raster::{self, RasterOptions};
use anvil_studio::json::parse as parse_json;

#[test]
fn gerber_current_point_advances_after_draw_and_flash() {
    let text = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.200000*%\nD10*\nX0Y0D02*\nX1000000D01*\nY1000000D01*\nM02*\n";
    let doc = parse_gerber(text).unwrap();
    assert_eq!(doc.primitives.len(), 2);
    match &doc.primitives[1] {
        GerberPrimitive::Draw { from, to, .. } => {
            assert_eq!(*from, anvil_core::geom::Point::new(1_000_000, 0));
            assert_eq!(*to, anvil_core::geom::Point::new(1_000_000, 1_000_000));
        }
        _ => panic!("expected draw"),
    }
}

#[test]
fn json_rejects_leading_zero_numbers() {
    assert!(parse_json("01").is_err());
    assert!(parse_json("{\"x\":-01}").is_err());
    assert!(parse_json("0").is_ok());
    assert!(parse_json("0.5").is_ok());
}

#[test]
fn raster_fit_does_not_exceed_target_width() {
    let text = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.200000*%\nD10*\nX0Y0D02*\nX10000000Y0D01*\nM02*\n";
    let doc = parse_gerber(text).unwrap();
    for target in [64u32, 320, 800, 1200] {
        let opts = raster::fit_to_width(&doc, target).unwrap();
        let img = raster::render(&doc, &opts).unwrap();
        assert!(img.width <= target, "target {target} produced {}", img.width);
    }
    assert!(raster::render(&doc, &RasterOptions::default()).is_ok());
}

#[test]
fn examples_match_the_public_builders() {
    // The full comparison lives in examples_load.rs; this catches accidental
    // drift in the in-memory demo serialization as well.
    let lib = anvil_core::library::Library::standard();
    let p = anvil_studio::project::Project::demo(&lib).unwrap();
    let v = p.to_json();
    let back = anvil_studio::project::Project::from_json(&lib, &v).unwrap();
    assert_eq!(back.name, p.name);
    assert_eq!(back.schematic.components().len(), p.schematic.components().len());
}
