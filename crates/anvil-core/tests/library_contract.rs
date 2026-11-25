//! Library contract: every advertised part is usable end to end.

use anvil_core::library::{Library, PadShape};

#[test]
fn every_part_resolves_symbol_and_footprint() {
    let lib = Library::standard();
    assert!(lib.parts().len() >= 20);
    for part in lib.parts() {
        let sym = lib.symbol(&part.symbol).expect("symbol");
        let fp = lib.footprint(&part.footprint).expect("footprint");
        assert!(!sym.pins.is_empty(), "{} symbol has no pins", part.name);
        assert!(!fp.pads.is_empty(), "{} footprint has no pads", part.name);
        assert!(!sym.graphics.is_empty(), "{} symbol has no graphics", part.name);
        assert!(part.name.len() < 64);
    }
}

#[test]
fn known_parts_exist() {
    let lib = Library::standard();
    for name in [
        "R", "C", "L", "D", "LED", "V", "I", "GND", "VCC", "SW", "AND2", "OR2", "XOR2",
        "NAND2", "NOR2", "NOT", "DFF", "CONN2", "CONN4", "CONN8",
    ] {
        assert!(lib.part(name).is_some(), "missing part {name}");
    }
    assert_eq!(lib.part("V").unwrap().footprint, "PINHDR_2");
}

#[test]
fn symbols_have_unique_pin_numbers_and_names() {
    let lib = Library::standard();
    for sym in lib.symbols() {
        let mut numbers: Vec<&str> = sym.pins.iter().map(|p| p.number.as_str()).collect();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), sym.pins.len(), "{} duplicate pin numbers", sym.name);
        let mut names: Vec<&str> = sym.pins.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), sym.pins.len(), "{} duplicate pin names", sym.name);
    }
}

#[test]
fn footprints_have_sane_pads() {
    let lib = Library::standard();
    for fp in lib.footprints() {
        for pad in &fp.pads {
            assert!(pad.size.0 > 0 && pad.size.1 > 0);
            assert!(pad.size.0 < 20_000_000 && pad.size.1 < 20_000_000);
            if pad.drill > 0 {
                assert!(pad.drill < pad.size.0.min(pad.size.1));
            }
            if matches!(pad.shape, PadShape::Circle | PadShape::Oval) {
                assert!(pad.size.0 == pad.size.1 || pad.shape == PadShape::Oval);
            }
        }
        assert!(fp.courtyard.0 > 0 && fp.courtyard.1 > 0);
    }
}

#[test]
fn search_matches_categories_and_keywords() {
    let lib = Library::standard();
    assert!(lib.search("gate").len() >= 5);
    assert!(lib.search("led").iter().any(|p| p.name == "LED"));
    assert_eq!(lib.search("definitely-not-a-part").len(), 0);
}
