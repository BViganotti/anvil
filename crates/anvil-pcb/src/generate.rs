//! Board construction: import a schematic netlist, and generate deterministic
//! synthetic boards used by autorouter acceptance tests.

use crate::board::{Board, DesignRules, Layer, MM};
use crate::connect::ratsnest;
use anvil_core::geom::Point;
use anvil_core::library::{Footprint as FootprintDef, Library};
use anvil_core::netlist::Netlist;
use anvil_core::schematic::Schematic;
use anvil_core::util::Rng;

#[derive(Clone, Debug)]
pub struct BuildOptions {
    pub board_width: i64,
    pub margin: i64,
    pub gap: i64,
    pub copper_layers: u8,
    pub rules: DesignRules,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            board_width: 100 * MM,
            margin: 6 * MM,
            gap: 5 * MM,
            copper_layers: 2,
            rules: DesignRules {
                min_trace: 200_000,
                min_clearance: 200_000,
                min_drill: 300_000,
                min_annular: 100_000,
                edge_clearance: 400_000,
                via_drill: 400_000,
                via_diameter: 800_000,
            },
        }
    }
}

/// Import every component into a rectangular board, assigning pads to nets by
/// pin number. Components are arranged in shelves with generous channels.
pub fn board_from_netlist(
    sch: &Schematic,
    netlist: &Netlist,
    lib: &Library,
    opts: &BuildOptions,
) -> Result<Board, String> {
    if sch.components().is_empty() {
        return Err("schematic has no components".to_string());
    }
    if sch.components().len() > 200 {
        return Err("too many components for automatic board generation".to_string());
    }
    opts.rules
        .validate()
        .map_err(|e| format!("invalid design rules: {e}"))?;
    let mut board = Board::new_rect(opts.board_width, 40 * MM, opts.copper_layers, opts.rules.clone());
    board.name = format!("{} board", sch.sheet.title);

    // Nets in netlist order; keep empty nets too so ids are stable.
    let mut net_map: Vec<Option<usize>> = Vec::with_capacity(netlist.nets.len());
    for n in &netlist.nets {
        net_map.push(Some(board.add_net(&n.name)));
    }

    struct Placement {
        comp_index: usize,
        def: FootprintDef,
        width: i64,
        height: i64,
    }
    let mut placements: Vec<Placement> = Vec::new();
    for (ci, c) in sch.components().iter().enumerate() {
        let def = lib
            .footprint(&c.footprint)
            .cloned()
            .or_else(|| lib.footprint("TP_1MM").cloned())
            .ok_or_else(|| format!("no footprint for {}", c.refdes))?;
        let b = def.bounds();
        let mut w = b.width() + 2_540_000;
        let mut h = b.height() + 2_540_000;
        if c.rotation % 180 == 90 {
            std::mem::swap(&mut w, &mut h);
        }
        placements.push(Placement {
            comp_index: ci,
            def,
            width: w,
            height: h,
        });
    }

    let usable = (opts.board_width - 2 * opts.margin).max(20 * MM);
    let mut x = opts.margin;
    let mut y = opts.margin;
    let mut row_h = 0;
    let mut positions: Vec<(i64, i64)> = Vec::with_capacity(placements.len());
    for p in &placements {
        if x + p.width > opts.margin + usable && x > opts.margin {
            x = opts.margin;
            y += row_h + opts.gap;
            row_h = 0;
        }
        let cx = x + p.width / 2;
        let cy = y + p.height / 2;
        positions.push((cx, cy));
        x += p.width + opts.gap;
        row_h = row_h.max(p.height);
    }
    let height = (y + row_h + opts.margin).max(40 * MM);
    board.outline = vec![
        Point::new(0, 0),
        Point::new(opts.board_width, 0),
        Point::new(opts.board_width, height),
        Point::new(0, height),
    ];

    for (pi, p) in placements.iter().enumerate() {
        let comp = &sch.components()[p.comp_index];
        let (cx, cy) = positions[pi];
        let mut pad_nets: Vec<Option<usize>> = Vec::with_capacity(p.def.pads.len());
        for pd in &p.def.pads {
            let pin = comp.pin_by_number(&pd.number);
            let net = pin.and_then(|pin| netlist.pin_net(&comp.refdes, &pin.number));
            pad_nets.push(net.and_then(|n| net_map.get(n.id).copied().flatten()));
        }
        board
            .add_footprint(&p.def, &comp.refdes, Point::new(cx, cy), comp.rotation, &pad_nets)
            .map_err(|e| format!("placing {}: {e}", comp.refdes))?;
    }
    // Remove unused empty nets? Keep them; they are harmless and stable.
    let _ = &net_map;
    board.validate_structure().map_err(|e| e.to_string())?;
    Ok(board)
}

/// Deterministic synthetic board: nine small SMD footprints on a 100 x 80 mm
/// board with two-pad signal nets. Used for zero-DRC autorouter acceptance.
pub fn generated_board(seed: u64) -> Board {
    let lib = Library::standard();
    let mut rng = Rng::new(seed.wrapping_mul(0x9e37_79b9).wrapping_add(17));
    let mut board = Board::new_rect(100 * MM, 80 * MM, 2, BuildOptions::default().rules);
    board.name = format!("Generated board {}", seed);
    let footprints = ["R_0805", "D_SOD123", "SOT23_3"];
    let cols = [20 * MM, 50 * MM, 80 * MM];
    let rows = [18 * MM, 42 * MM, 66 * MM];
    let mut slot = 0usize;
    let mut pads: Vec<(usize, usize)> = Vec::new();
    for row in rows {
        for col in cols {
            let name = footprints[(slot + seed as usize) % footprints.len()];
            let def = match lib.footprint(name) {
                Some(d) => d,
                None => continue,
            };
            let jx = rng.range_i64(-2 * MM, 2 * MM);
            let jy = rng.range_i64(-2 * MM, 2 * MM);
            let at = Point::new(col + jx, row + jy);
            let pad_nets = vec![None; def.pads.len()];
            if let Ok(fi) = board.add_footprint(def, &format!("U{}", slot + 1), at, 0, &pad_nets) {
                for pi in 0..def.pads.len() {
                    pads.push((fi, pi));
                }
            }
            slot += 1;
        }
    }
    // Shuffle pad order deterministically, then pair pads into nets.
    rng.shuffle(&mut pads);
    // Avoid pairing pads of the same footprint when possible.
    let mut remaining = pads;
    let mut net_number = 0usize;
    while remaining.len() >= 2 {
        let a = remaining.remove(0);
        let mut partner = None;
        for (i, b) in remaining.iter().enumerate() {
            if b.0 != a.0 {
                partner = Some(i);
                break;
            }
        }
        let idx = partner.unwrap_or(0);
        let b = remaining.remove(idx);
        net_number += 1;
        let name = match net_number {
            1 => "GND".to_string(),
            2 => "VCC".to_string(),
            n => format!("N{}", n - 2),
        };
        let net = board.add_net(&name);
        board.footprints[a.0].pads[a.1].net = Some(net);
        board.footprints[b.0].pads[b.1].net = Some(net);
    }
    if let Some(last) = remaining.pop() {
        let net = board.add_net("NC");
        board.footprints[last.0].pads[last.1].net = Some(net);
    }
    let _ = Layer::Top;
    let _ = ratsnest(&board);
    board
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::netlist::extract;
    use anvil_core::schematic::{LabelKind, Schematic};

    #[test]
    fn generated_boards_are_well_formed() {
        for seed in 1..=20 {
            let b = generated_board(seed);
            b.validate_structure().unwrap();
            assert!(b.pad_count() >= 18);
            assert!(b.nets.len() >= 8);
        }
    }

    #[test]
    fn generated_board_is_deterministic() {
        let a = generated_board(7);
        let b = generated_board(7);
        assert_eq!(a.footprints, b.footprints);
        assert_eq!(a.nets, b.nets);
    }

    #[test]
    fn netlist_import_places_components() {
        let lib = Library::standard();
        let mut s = Schematic::new();
        let r = s.add_component(&lib, "R", Point::new(0, 0)).unwrap();
        let c = s.add_component(&lib, "C", Point::new(5 * MM, 0)).unwrap();
        let g = s.add_component(&lib, "GND", Point::new(0, 10 * MM)).unwrap();
        let p1 = s.component(r).unwrap().pins[0].pos;
        let p2 = s.component(c).unwrap().pins[0].pos;
        let pg = s.component(g).unwrap().pins[0].pos;
        s.add_wire(vec![p1, pg]).unwrap();
        s.add_wire(vec![p2, pg]).unwrap();
        s.add_label("OUT", s.component(r).unwrap().pins[1].pos, LabelKind::Local)
            .unwrap();
        let nl = extract(&s, &lib);
        let b = board_from_netlist(&s, &nl, &lib, &BuildOptions::default()).unwrap();
        assert_eq!(b.footprints.len(), 3);
        assert_eq!(b.pad_count(), 5);
        assert!(b.nets.iter().any(|n| n == "GND"));
    }
}
