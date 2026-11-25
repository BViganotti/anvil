//! Gerber parser/writer dialect contract.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_export::gerber::{self, ApertureShape, GerberPrimitive};
use anvil_pcb::board::{Board, DesignRules, Layer, MM};

#[test]
fn inch_coordinates_are_converted() {
    let text = "%FSLAX24Y24*%\n%MOIN*%\n%ADD10C,0.010000*%\nD10*\nX1000Y2000D03*\nM02*\n";
    let doc = gerber::parse(text).unwrap();
    assert!(!doc.units_mm);
    assert_eq!(
        doc.primitives[0],
        GerberPrimitive::Flash {
            aperture: 10,
            at: Point::new(2_540_000, 5_080_000)
        }
    );
}

#[test]
fn omitted_coordinates_are_retained() {
    let text = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.200000*%\nD10*\nX1000000Y2000000D02*\nX3000000D01*\nY4000000D01*\nM02*\n";
    let doc = gerber::parse(text).unwrap();
    assert_eq!(doc.primitives.len(), 2);
    match &doc.primitives[0] {
        GerberPrimitive::Draw { from, to, .. } => {
            assert_eq!(*from, Point::new(1_000_000, 2_000_000));
            assert_eq!(*to, Point::new(3_000_000, 2_000_000));
        }
        _ => panic!("expected draw"),
    }
    match &doc.primitives[1] {
        GerberPrimitive::Draw { from, to, .. } => {
            assert_eq!(*from, Point::new(3_000_000, 2_000_000));
            assert_eq!(*to, Point::new(3_000_000, 4_000_000));
        }
        _ => panic!("expected draw"),
    }
}

#[test]
fn aperture_shapes() {
    let text = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.200000*%\n%ADD11R,1.000000X1.500000*%\n%ADD12O,2.000000X1.000000*%\nD10*\nD11*\nD12*\nM02*\n";
    let doc = gerber::parse(text).unwrap();
    assert_eq!(doc.apertures.len(), 3);
    assert_eq!(doc.apertures[0].shape, ApertureShape::Circle);
    assert_eq!(doc.apertures[1].shape, ApertureShape::Rect);
    assert_eq!(doc.apertures[1].width, 1_000_000);
    assert_eq!(doc.apertures[1].height, 1_500_000);
    assert_eq!(doc.apertures[2].shape, ApertureShape::Oval);
}

#[test]
fn unsupported_features_are_explicit_errors() {
    for text in [
        "%FSLAX46Y46*%\nG02*\nM02*\n",
        "%FSLAX46Y46*%\nG36*\nM02*\n",
        "%FSLAX46Y46*%\n%AMMACRO*%\nM02*\n",
        "%FSLAX46Y46*%\n%LPC*%\nM02*\n",
        "%FSLAX46Y46*%\n%SRX2Y2*%\nM02*\n",
    ] {
        assert!(gerber::parse(text).is_err(), "{text:?}");
    }
}

#[test]
fn header_only_layer_is_valid() {
    let doc = gerber::parse("%FSLAX46Y46*%\n%MOMM*%\nG01*\nM02*\n").unwrap();
    assert!(doc.primitives.is_empty());
    assert!(doc.apertures.is_empty());
}

#[test]
fn four_layer_board_writes_inner_files() {
    let mut b = Board::new_rect(30 * MM, 20 * MM, 4, DesignRules::default());
    let n = b.add_net("N");
    b.add_trace(n, Layer::Inner1, 200_000, vec![Point::new(5 * MM, 5 * MM), Point::new(10 * MM, 5 * MM)])
        .unwrap();
    b.add_trace(n, Layer::Inner2, 200_000, vec![Point::new(5 * MM, 8 * MM), Point::new(10 * MM, 8 * MM)])
        .unwrap();
    let files = gerber::write_board(&b);
    assert!(files.iter().any(|(name, _)| name.contains("In1_Cu")));
    assert!(files.iter().any(|(name, _)| name.contains("In2_Cu")));
    for (name, text) in files {
        let doc = gerber::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        if name.contains("In1_Cu") || name.contains("In2_Cu") {
            assert!(!doc.primitives.is_empty());
        }
    }
}

#[test]
fn writer_flashes_match_pad_positions() {
    let lib = Library::standard();
    let mut b = Board::new_rect(30 * MM, 20 * MM, 2, DesignRules::default());
    let n = b.add_net("N");
    b.add_footprint(
        lib.footprint("R_0805").unwrap(),
        "R1",
        Point::new(10 * MM, 10 * MM),
        0,
        &[Some(n), None],
    )
    .unwrap();
    let doc = gerber::parse(&gerber::write_copper(&b, Layer::Top)).unwrap();
    for f in &b.footprints {
        for p in &f.pads {
            assert!(doc
                .primitives
                .iter()
                .any(|g| matches!(g, GerberPrimitive::Flash { at, .. } if *at == p.at)));
        }
    }
}
