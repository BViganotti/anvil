//! Board generation contract.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_core::netlist::extract;
use anvil_core::schematic::Schematic;
use anvil_pcb::board::DesignRules;
use anvil_pcb::generate::{board_from_netlist, generated_board, BuildOptions};

#[test]
fn generated_boards_have_connective_nets() {
    for seed in 1..=10u64 {
        let b = generated_board(seed);
        b.validate_structure().unwrap();
        assert!(b.pad_count() >= 18);
        let assigned = b.pads().filter(|(_, p)| p.net.is_some()).count();
        assert!(assigned >= 18, "seed {seed} assigned {assigned}");
        let multi = (0..b.nets.len())
            .filter(|&n| b.pads().filter(|(_, p)| p.net == Some(n)).count() >= 2)
            .count();
        assert!(multi >= 8, "seed {seed} multi nets {multi}");
    }
}

#[test]
fn generation_is_deterministic_per_seed() {
    for seed in [1u64, 5, 11] {
        let a = generated_board(seed);
        let b = generated_board(seed);
        assert_eq!(a.footprints, b.footprints);
        assert_eq!(a.nets, b.nets);
        assert_eq!(a.outline, b.outline);
        assert_ne!(generated_board(seed + 1).footprints, a.footprints);
    }
}

#[test]
fn netlist_import_skips_power_symbols() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    s.add_component_with_refdes(&lib, "V", Point::new(0, 0), "V1").unwrap();
    s.add_component_with_refdes(&lib, "GND", Point::new(0, -10_000_000), "GND1").unwrap();
    s.add_component_with_refdes(&lib, "VCC", Point::new(10_000_000, -10_000_000), "VCC1").unwrap();
    s.add_component_with_refdes(&lib, "R", Point::new(10_000_000, 0), "R1").unwrap();
    let pins = [
        ("V1", "+", "VIN"),
        ("V1", "-", "GND"),
        ("R1", "1", "VIN"),
        ("R1", "2", "GND"),
        ("GND1", "1", "GND"),
        ("VCC1", "1", "VCC"),
    ];
    for (refdes, pin, net) in pins {
        let id = s.components().iter().find(|c| c.refdes == refdes).unwrap().id;
        let pos = s.component(id).unwrap().pin_by_name(pin).unwrap().pos;
        s.add_label(net, pos, anvil_core::schematic::LabelKind::Local).unwrap();
    }
    let nl = extract(&s, &lib);
    assert_eq!(nl.errors(), 0, "{:?}", nl.diagnostics);
    let b = board_from_netlist(&s, &nl, &lib, &BuildOptions::default()).unwrap();
    assert_eq!(b.footprints.len(), 2, "power symbols are annotations");
    assert!(b.nets.iter().any(|n| n == "VIN"));
    let v = b.footprints.iter().find(|f| f.refdes == "V1").unwrap();
    assert_eq!(v.pads.len(), 2);
    assert!(v.pads.iter().all(|p| p.net.is_some()));
}

#[test]
fn empty_schematic_is_rejected() {
    let lib = Library::standard();
    let s = Schematic::new();
    let nl = extract(&s, &lib);
    assert!(board_from_netlist(&s, &nl, &lib, &BuildOptions::default()).is_err());
}

#[test]
fn invalid_rules_are_rejected() {
    let lib = Library::standard();
    let mut s = Schematic::new();
    s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
    let nl = extract(&s, &lib);
    let mut opts = BuildOptions::default();
    opts.rules = DesignRules {
        min_trace: 0,
        ..DesignRules::default()
    };
    assert!(board_from_netlist(&s, &nl, &lib, &opts).is_err());
}
