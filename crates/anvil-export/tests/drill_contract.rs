//! Excellon dialect and error contract.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_export::drill::{self, DrillError};
use anvil_pcb::board::{Board, DesignRules, Layer, MM};

#[test]
fn imperial_decimal_is_supported() {
    let text = "M48\nINCH\nT1C0.0400\n%\nG90\nT1\nX1.0000Y2.0000\nM30\n";
    let d = drill::parse(text).unwrap();
    assert!(!d.units_mm);
    assert_eq!(d.tools.len(), 1);
    assert!(d.tools[0].diameter > 1_000_000 && d.tools[0].diameter < 1_020_000);
    assert_eq!(d.hits[0].at, anvil_core::geom::Point::new(25_400_000, 50_800_000));
}

#[test]
fn fixed_format_integers_are_rejected() {
    let text = "M48\nMETRIC,TZ\nT1C0.4000\n%\nT1\nX1000Y2000\nM30\n";
    assert!(matches!(drill::parse(text), Err(DrillError::Unsupported(_))));
}

#[test]
fn duplicate_tools_and_unknown_hits_error() {
    let dup = "M48\nMETRIC\nT1C0.4\nT1C0.5\n%\nM30\n";
    assert!(drill::parse(dup).is_err());
    let unknown = "M48\nMETRIC\nT1C0.4\n%\nT2\nX1.0Y1.0\nM30\n";
    assert!(matches!(drill::parse(unknown), Err(DrillError::UnknownTool(2))));
}

#[test]
fn header_comments_and_unknown_lines() {
    let text = "M48\n; hello\nMETRIC,TZ\nFMAT,2\nT1C0.4000\n%\nG90\nG05\nT1\nX10.0000Y20.0000\nM30\n";
    let d = drill::parse(text).unwrap();
    assert_eq!(d.hits.len(), 1);
    assert_eq!(d.hits[0].at.x, 10_000_000);
}

#[test]
fn no_tools_is_an_error() {
    assert!(matches!(drill::parse("M48\nMETRIC\n%\nM30\n"), Err(DrillError::NoTools)));
    assert!(matches!(drill::parse(""), Err(DrillError::Empty)));
}

#[test]
fn writer_groups_diameters_and_round_trips() {
    let lib = Library::standard();
    let mut b = Board::new_rect(30 * MM, 20 * MM, 2, DesignRules::default());
    let n = b.add_net("N");
    b.add_footprint(
        lib.footprint("PINHDR_2").unwrap(),
        "J1",
        Point::new(10 * MM, 10 * MM),
        0,
        &[Some(n), None],
    )
    .unwrap();
    b.add_via(n, anvil_core::geom::Point::new(20 * MM, 10 * MM), Layer::Top, Layer::Bottom)
        .unwrap();
    let file = drill::from_board(&b);
    assert_eq!(file.tools.len(), 2);
    assert_eq!(file.hits.len(), 3);
    let text = drill::write(&file);
    assert!(text.starts_with("M48"));
    assert!(text.ends_with("M30\n"));
    let parsed = drill::parse(&text).unwrap();
    assert_eq!(parsed.tools, file.tools);
    assert_eq!(parsed.hits, file.hits);
}
