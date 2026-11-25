//! Deterministic fuzz harness for Anvil. Standard library only.
//!
//! Every target is wrapped in `catch_unwind`; a panic is a hard failure. The
//! harness reports how many inputs were rejected (parse error) versus accepted
//! (valid but unusual), without claiming anything about coverage it does not
//! measure.

use anvil_core::geom::Point;
use anvil_core::library::Library;
use anvil_core::netlist::extract;
use anvil_core::schematic::{LabelKind, Schematic};
use anvil_core::util::Rng;
use anvil_export::{drill, gerber};
use anvil_studio::json;
use anvil_studio::project::Project;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn random_text(rng: &mut Rng, max_len: usize) -> String {
    const SET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789%*.,+-/\\\"'{}[]():;<>#@!?= \n\r\t_|&$^";
    let len = rng.below(max_len as u32) as usize;
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        s.push(SET[rng.below(SET.len() as u32) as usize] as char);
    }
    s
}

fn mutate(rng: &mut Rng, base: &str) -> String {
    const MUT: &[u8] = b"ABCXYZ0123456789%*.,+-/\n";
    let mut bytes = base.as_bytes().to_vec();
    if bytes.is_empty() {
        return random_text(rng, 200);
    }
    let mutations = 1 + rng.below(8) as usize;
    for _ in 0..mutations {
        match rng.below(3) {
            0 => {
                let at = rng.below(bytes.len() as u32) as usize;
                bytes[at] = MUT[rng.below(MUT.len() as u32) as usize];
            }
            1 => {
                let at = rng.below(bytes.len() as u32) as usize;
                bytes.remove(at);
                if bytes.is_empty() {
                    break;
                }
            }
            _ => {
                let at = rng.below(bytes.len() as u32) as usize;
                bytes.insert(at, MUT[rng.below(MUT.len() as u32) as usize]);
            }
        }
    }
    String::from_utf8_lossy(&bytes).to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let count: u64 = args
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000)
        .clamp(1, 1_000_000);
    let seed: u64 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_1234);
    let mut rng = Rng::new(seed);
    let lib = Library::standard();
    let demo = Project::demo(&lib).unwrap();
    let demo_json = demo.to_json().to_json();
    let mut board = anvil_pcb::generate::generated_board(4);
    board.add_trace(
        0,
        anvil_pcb::board::Layer::Top,
        200_000,
        vec![Point::new(5_000_000, 5_000_000), Point::new(20_000_000, 5_000_000)],
    )
    .unwrap_or(0);
    let gerber_base = gerber::write_copper(&board, anvil_pcb::board::Layer::Top);
    let drill_base = drill::write_board(&board);

    let mut panics = 0u64;
    let mut rejected = 0u64;
    let mut accepted = 0u64;
    for i in 0..count {
        let target = rng.below(8);
        let text = if target < 5 {
            random_text(&mut rng, 300)
        } else {
            let base = match target {
                5 => demo_json.as_str(),
                6 => gerber_base.as_str(),
                _ => drill_base.as_str(),
            };
            mutate(&mut rng, base)
        };
        let result = catch_unwind(AssertUnwindSafe(|| -> bool {
            match target {
                0 => json::parse(&text).is_ok(),
                1 => gerber::parse(&text).is_ok(),
                2 => drill::parse(&text).is_ok(),
                3 => Project::from_json(&lib, &json::parse(&text).unwrap_or(json::Value::Null)).is_ok(),
                4 => {
                    let mut s = Schematic::new();
                    let x = (rng.next_u64() % 2_000_000_000) as i64;
                    let y = (rng.next_u64() % 2_000_000_000) as i64;
                    let p = Point::new(x - 1_000_000_000, y - 1_000_000_000);
                    let name = text.chars().take(20).collect::<String>();
                    let mut ok = false;
                    if s.add_component(&lib, &name, p).is_ok() {
                        ok = true;
                    }
                    if s.add_wire(vec![p]).is_ok() {
                        ok = true;
                    }
                    if s.add_label(&name, p, LabelKind::Local).is_ok() {
                        ok = true;
                    }
                    let nl = extract(&s, &lib);
                    ok || nl.errors() == 0
                }
                5 => Project::from_json(&lib, &json::parse(&text).unwrap_or(json::Value::Null)).is_ok(),
                6 => gerber::parse(&text).is_ok(),
                _ => drill::parse(&text).is_ok(),
            }
        }));
        match result {
            Ok(true) => accepted += 1,
            Ok(false) => rejected += 1,
            Err(_) => {
                panics += 1;
                eprintln!("PANIC on case {i} target {target}: {:?}", &text[..text.len().min(200)]);
                if panics > 5 {
                    break;
                }
            }
        }
    }
    println!(
        "fuzz: {count} cases, {rejected} rejected, {accepted} accepted, {panics} panics (seed {seed})"
    );
    if panics > 0 {
        std::process::exit(1);
    }
    println!("fuzz OK: no panics");
}
