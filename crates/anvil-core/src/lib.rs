//! Anvil core: schematic model, netlist extraction, component library and
//! circuit simulation. Standard library only.

pub mod geom;
pub mod library;
pub mod netlist;
pub mod schematic;
pub mod sim;
pub mod util;

pub use geom::{Point, Rect};
pub use library::{Library, Part, Symbol};
pub use netlist::{extract, Diagnostic, Net, Netlist, Severity};
pub use schematic::Schematic;

/// Schematic units: one grid step is 1.27 mm (50 mil), expressed in nm.
pub const GRID_NM: i64 = 1_270_000;
/// Common schematic text size.
pub const TEXT_NM: i64 = 1_270_000;

/// Round a nanometre coordinate to the schematic grid.
pub fn snap(nm: i64) -> i64 {
    ((nm as f64) / (GRID_NM as f64)).round() as i64 * GRID_NM
}

/// Version string shared by the binary, Studio and exported files.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
