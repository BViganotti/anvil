//! Built-in component library: symbols, footprints and parts.
//!
//! The library is compiled into the binary. Symbols use nanometre offsets
//! relative to the component anchor; footprints use nanometre offsets relative
//! to the footprint origin. Everything here is original geometry.

use crate::geom::Point;
use crate::schematic::PinKind;

/// Normalised signal levels for the digital parts in the standard library.
pub const LOGIC_VDD: f64 = 5.0;
pub const LOGIC_ROUT: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadShape {
    Circle,
    Rect,
    Oval,
    /// Rectangular pad with a through-hole drill; rendered as a rect.
    Square,
}

impl PadShape {
    pub fn as_str(self) -> &'static str {
        match self {
            PadShape::Circle => "circle",
            PadShape::Rect => "rect",
            PadShape::Oval => "oval",
            PadShape::Square => "square",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadLayer {
    Top,
    Bottom,
    Through,
}

impl PadLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            PadLayer::Top => "top",
            PadLayer::Bottom => "bottom",
            PadLayer::Through => "through",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "top" => Some(PadLayer::Top),
            "bottom" => Some(PadLayer::Bottom),
            "through" => Some(PadLayer::Through),
            _ => None,
        }
    }
}

/// A footprint pad in footprint-local coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PadDef {
    pub number: String,
    pub offset: Point,
    /// Width and height in nanometers.
    pub size: (i64, i64),
    pub shape: PadShape,
    /// Drill diameter; zero means SMD.
    pub drill: i64,
    pub layer: PadLayer,
}

impl PadDef {
    fn smd(number: &str, x: i64, y: i64, w: i64, h: i64) -> Self {
        PadDef {
            number: number.to_string(),
            offset: Point::new(x, y),
            size: (w, h),
            shape: PadShape::Rect,
            drill: 0,
            layer: PadLayer::Top,
        }
    }

    fn thru(number: &str, x: i64, y: i64, d: i64, drill: i64) -> Self {
        PadDef {
            number: number.to_string(),
            offset: Point::new(x, y),
            size: (d, d),
            shape: PadShape::Circle,
            drill,
            layer: PadLayer::Through,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Footprint {
    pub name: String,
    pub description: String,
    pub pads: Vec<PadDef>,
    /// Courtyard size (width, height) around the origin.
    pub courtyard: (i64, i64),
}

impl Footprint {
    pub fn bounds(&self) -> crate::geom::Rect {
        let hw = self.courtyard.0 / 2;
        let hh = self.courtyard.1 / 2;
        crate::geom::Rect::new(Point::new(-hw, -hh), Point::new(hw, hh))
    }
}

/// Symbol drawing primitives, all in nanometres relative to the symbol anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Graphic {
    Line { a: Point, b: Point },
    Rect { min: Point, max: Point },
    Circle { center: Point, radius: i64 },
    Polyline { points: Vec<Point> },
    Text { at: Point, text: String, size: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinDef {
    pub name: String,
    pub number: String,
    pub offset: Point,
    /// Direction the pin points, in degrees (0 = right, 90 = up). Used for
    /// rendering the pin stub and placing labels.
    pub direction: u16,
    pub length: i64,
    pub kind: PinKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub description: String,
    pub pins: Vec<PinDef>,
    pub graphics: Vec<Graphic>,
    /// Nominal body size used for selection boxes and text placement.
    pub body: (i64, i64),
}

impl Symbol {
    pub fn pin(&self, name: &str) -> Option<&PinDef> {
        self.pins.iter().find(|p| p.name == name)
    }

    pub fn bounds(&self) -> crate::geom::Rect {
        let hw = self.body.0 / 2;
        let hh = self.body.1 / 2;
        crate::geom::Rect::new(Point::new(-hw, -hh), Point::new(hw, hh)).expand(2_540_000)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub name: String,
    pub symbol: String,
    pub footprint: String,
    pub value: String,
    pub category: String,
    pub description: String,
    pub keywords: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Library {
    parts: Vec<Part>,
    symbols: Vec<Symbol>,
    footprints: Vec<Footprint>,
}

const G: i64 = 1_270_000;

fn pin(name: &str, number: &str, x: i64, y: i64, dir: u16, kind: PinKind) -> PinDef {
    PinDef {
        name: name.to_string(),
        number: number.to_string(),
        offset: Point::new(x, y),
        direction: dir,
        length: G,
        kind,
    }
}

fn line(x1: i64, y1: i64, x2: i64, y2: i64) -> Graphic {
    Graphic::Line {
        a: Point::new(x1, y1),
        b: Point::new(x2, y2),
    }
}

fn rect(x1: i64, y1: i64, x2: i64, y2: i64) -> Graphic {
    Graphic::Rect {
        min: Point::new(x1, y1),
        max: Point::new(x2, y2),
    }
}

fn text(x: i64, y: i64, s: &str) -> Graphic {
    Graphic::Text {
        at: Point::new(x, y),
        text: s.to_string(),
        size: G / 2,
    }
}

impl Library {
    pub fn standard() -> Self {
        let mut lib = Library::default();
        lib.add_passives();
        lib.add_semiconductors();
        lib.add_sources();
        lib.add_logic();
        lib.add_connectors();
        lib
    }

    fn add_symbol(&mut self, sym: Symbol) {
        if !self.symbols.iter().any(|s| s.name == sym.name) {
            self.symbols.push(sym);
        }
    }

    fn add_footprint(&mut self, fp: Footprint) {
        if !self.footprints.iter().any(|f| f.name == fp.name) {
            self.footprints.push(fp);
        }
    }

    fn add_part(
        &mut self,
        name: &str,
        symbol: &str,
        footprint: &str,
        value: &str,
        category: &str,
        description: &str,
        keywords: &[&str],
    ) {
        if !self.parts.iter().any(|p| p.name == name) {
            self.parts.push(Part {
                name: name.to_string(),
                symbol: symbol.to_string(),
                footprint: footprint.to_string(),
                value: value.to_string(),
                category: category.to_string(),
                description: description.to_string(),
                keywords: keywords.iter().map(|s| s.to_string()).collect(),
            });
        }
    }

    fn add_passives(&mut self) {
        let mut r = Symbol {
            name: "R".into(),
            description: "Resistor".into(),
            pins: vec![
                pin("1", "1", -2 * G, 0, 180, PinKind::Passive),
                pin("2", "2", 2 * G, 0, 0, PinKind::Passive),
            ],
            graphics: vec![rect(-G, -G / 2, G, G / 2), text(-G, G, "R")],
            body: (2 * G, G),
        };
        r.graphics.push(line(-2 * G, 0, -G, 0));
        r.graphics.push(line(G, 0, 2 * G, 0));
        self.add_symbol(r);

        let mut c = Symbol {
            name: "C".into(),
            description: "Capacitor".into(),
            pins: vec![
                pin("1", "1", -2 * G, 0, 180, PinKind::Passive),
                pin("2", "2", 2 * G, 0, 0, PinKind::Passive),
            ],
            graphics: vec![
                line(-G / 2, -G, -G / 2, G),
                line(G / 2, -G, G / 2, G),
                line(-2 * G, 0, -G / 2, 0),
                line(G / 2, 0, 2 * G, 0),
            ],
            body: (G, 2 * G),
        };
        c.graphics.push(text(-G / 2, G + G / 4, "C"));
        self.add_symbol(c);

        let mut l = Symbol {
            name: "L".into(),
            description: "Inductor".into(),
            pins: vec![
                pin("1", "1", -2 * G, 0, 180, PinKind::Passive),
                pin("2", "2", 2 * G, 0, 0, PinKind::Passive),
            ],
            graphics: vec![
                line(-2 * G, 0, -G, 0),
                line(G, 0, 2 * G, 0),
                Graphic::Circle {
                    center: Point::new(-G / 2, 0),
                    radius: G / 2,
                },
                Graphic::Circle {
                    center: Point::new(0, 0),
                    radius: G / 2,
                },
                Graphic::Circle {
                    center: Point::new(G / 2, 0),
                    radius: G / 2,
                },
            ],
            body: (2 * G, G),
        };
        l.graphics.push(text(-G, G, "L"));
        self.add_symbol(l);

        self.add_footprint(Footprint {
            name: "R_0805".into(),
            description: "0805 resistor".into(),
            pads: vec![
                PadDef::smd("1", -950_000, 0, 1_200_000, 1_400_000),
                PadDef::smd("2", 950_000, 0, 1_200_000, 1_400_000),
            ],
            courtyard: (3_200_000, 2_200_000),
        });
        self.add_footprint(Footprint {
            name: "C_0805".into(),
            description: "0805 capacitor".into(),
            pads: vec![
                PadDef::smd("1", -950_000, 0, 1_200_000, 1_400_000),
                PadDef::smd("2", 950_000, 0, 1_200_000, 1_400_000),
            ],
            courtyard: (3_200_000, 2_200_000),
        });
        self.add_footprint(Footprint {
            name: "L_0805".into(),
            description: "0805 inductor".into(),
            pads: vec![
                PadDef::smd("1", -950_000, 0, 1_200_000, 1_400_000),
                PadDef::smd("2", 950_000, 0, 1_200_000, 1_400_000),
            ],
            courtyard: (3_200_000, 2_200_000),
        });
        self.add_footprint(Footprint {
            name: "TP_1MM".into(),
            description: "1 mm test point".into(),
            pads: vec![PadDef::thru("1", 0, 0, 2_000_000, 1_000_000)],
            courtyard: (2_500_000, 2_500_000),
        });

        for (name, fp, value, desc) in [
            ("R", "R_0805", "1k", "Resistor"),
            ("C", "C_0805", "100n", "Capacitor"),
            ("L", "L_0805", "10u", "Inductor"),
        ] {
            self.add_part(name, name, fp, value, "passive", desc, &[name]);
        }
    }

    fn add_semiconductors(&mut self) {
        let mut d = Symbol {
            name: "D".into(),
            description: "Diode".into(),
            pins: vec![
                pin("A", "1", -2 * G, 0, 180, PinKind::Passive),
                pin("K", "2", 2 * G, 0, 0, PinKind::Passive),
            ],
            graphics: vec![
                line(-2 * G, 0, -G, 0),
                line(G, 0, 2 * G, 0),
                Graphic::Polyline {
                    points: vec![
                        Point::new(-G, -G),
                        Point::new(-G, G),
                        Point::new(G, 0),
                        Point::new(-G, -G),
                    ],
                },
                line(G, -G, G, G),
            ],
            body: (2 * G, 2 * G),
        };
        d.graphics.push(text(-G, G + G / 2, "D"));
        let mut led = d.clone();
        self.add_symbol(d);
        led.name = "LED".into();
        led.description = "Light emitting diode".into();
        led.graphics.push(line(G + G / 2, -G, G + G, -G / 2));
        led.graphics.push(line(G + G / 2, -G / 2, G + G, -G));
        self.add_symbol(led);

        self.add_footprint(Footprint {
            name: "D_SOD123".into(),
            description: "SOD-123 diode".into(),
            pads: vec![
                PadDef::smd("1", -1_100_000, 0, 1_000_000, 1_400_000),
                PadDef::smd("2", 1_100_000, 0, 1_000_000, 1_400_000),
            ],
            courtyard: (3_400_000, 2_200_000),
        });
        for (name, desc) in [("D", "Diode"), ("LED", "LED")] {
            self.add_part(name, name, "D_SOD123", name, "diode", desc, &["diode"]);
        }

        // SOT-23 three pad footprint used by switches and small logic.
        self.add_footprint(Footprint {
            name: "SOT23".into(),
            description: "SOT-23 three pad".into(),
            pads: vec![
                PadDef::smd("1", -1_000_000, -1_000_000, 900_000, 900_000),
                PadDef::smd("2", 1_000_000, 0, 900_000, 900_000),
                PadDef::smd("3", -1_000_000, 1_000_000, 900_000, 900_000),
            ],
            courtyard: (3_000_000, 3_400_000),
        });
    }

    fn add_sources(&mut self) {
        let mut v = Symbol {
            name: "V".into(),
            description: "Voltage source".into(),
            pins: vec![
                pin("+", "1", 0, 2 * G, 90, PinKind::Power),
                pin("-", "2", 0, -2 * G, 270, PinKind::Power),
            ],
            graphics: vec![
                line(0, 2 * G, 0, G),
                Graphic::Circle {
                    center: Point::ORIGIN,
                    radius: G,
                },
                line(0, -G, 0, -2 * G),
                text(-G / 2, 0, "+"),
                text(-G / 2, -G / 2 - G / 4, "-"),
            ],
            body: (2 * G, 4 * G),
        };
        v.graphics.push(text(G, G / 2, "V"));
        let mut i = v.clone();
        self.add_symbol(v);
        i.name = "I".into();
        i.description = "Current source".into();
        i.graphics = vec![
            line(0, 2 * G, 0, G),
            Graphic::Circle {
                center: Point::ORIGIN,
                radius: G,
            },
            line(0, -G, 0, -2 * G),
            line(-G / 2, G / 2, 0, -G / 4),
            line(-G / 2, G / 2, -G / 2, 0),
            line(-G / 2, G / 2, 0, G / 2),
            text(G, -G / 2, "I"),
        ];
        self.add_symbol(i);

        self.add_symbol(Symbol {
            name: "GND".into(),
            description: "Ground".into(),
            pins: vec![pin("1", "1", 0, -G, 270, PinKind::Power)],
            graphics: vec![
                line(0, 0, 0, -G),
                line(-G, -G, G, -G),
                line(-G / 2, -G - G / 2, G / 2, -G - G / 2),
                line(-G / 4, -G - G, G / 4, -G - G),
            ],
            body: (2 * G, 2 * G),
        });
        self.add_symbol(Symbol {
            name: "VCC".into(),
            description: "Positive supply".into(),
            pins: vec![pin("1", "1", 0, -G, 270, PinKind::Power)],
            graphics: vec![
                line(0, 0, 0, -G),
                line(-G / 2, -G, G / 2, -G),
            ],
            body: (2 * G, 2 * G),
        });
        self.add_symbol(Symbol {
            name: "SW".into(),
            description: "Ideal switch".into(),
            pins: vec![
                pin("1", "1", -2 * G, 0, 180, PinKind::Passive),
                pin("2", "2", 2 * G, 0, 0, PinKind::Passive),
            ],
            graphics: vec![
                line(-2 * G, 0, -G, 0),
                line(G, 0, 2 * G, 0),
                Graphic::Circle {
                    center: Point::new(-G, 0),
                    radius: G / 8,
                },
                Graphic::Circle {
                    center: Point::new(G, 0),
                    radius: G / 8,
                },
                line(-G, 0, G, -G),
            ],
            body: (2 * G, 2 * G),
        });
        self.add_footprint(Footprint {
            name: "SOT23_SW".into(),
            description: "SOT-23 switch".into(),
            pads: vec![
                PadDef::smd("1", -1_000_000, -1_000_000, 900_000, 900_000),
                PadDef::smd("2", 1_000_000, 0, 900_000, 900_000),
                PadDef::smd("3", -1_000_000, 1_000_000, 900_000, 900_000),
            ],
            courtyard: (3_000_000, 3_400_000),
        });
        for (name, symbol, fp, value, cat, desc) in [
            ("V", "V", "TP_1MM", "5", "source", "DC voltage source"),
            ("I", "I", "TP_1MM", "1m", "source", "DC current source"),
            ("GND", "GND", "TP_1MM", "GND", "power", "Ground symbol"),
            ("VCC", "VCC", "TP_1MM", "VCC", "power", "Supply symbol"),
            ("SW", "SW", "SOT23_SW", "SW", "switch", "Ideal switch"),
        ] {
            self.add_part(name, symbol, fp, value, cat, desc, &[name]);
        }
    }

    fn add_logic(&mut self) {
        fn gate_shape(shape: &str) -> Vec<Graphic> {
            match shape {
                "and" => vec![
                    line(-2 * G, -G, 0, -G),
                    line(-2 * G, G, 0, G),
                    line(-2 * G, -G, -2 * G, G),
                    Graphic::Circle {
                        center: Point::new(0, 0),
                        radius: G,
                    },
                ],
                "or" => vec![
                    line(-2 * G, -G, -G, -G),
                    line(-2 * G, G, -G, G),
                    Graphic::Polyline {
                        points: vec![
                            Point::new(-2 * G, -G),
                            Point::new(-G, 0),
                            Point::new(-2 * G, G),
                        ],
                    },
                    Graphic::Circle {
                        center: Point::new(0, 0),
                        radius: G,
                    },
                ],
                _ => Vec::new(),
            }
        }

        let sink = |s: &mut Symbol| {
            s.pins.push(pin("Y", "3", 2 * G, 0, 0, PinKind::Output));
        };
        let mut and = Symbol {
            name: "AND2".into(),
            description: "2-input AND".into(),
            pins: vec![
                pin("A", "1", -3 * G, G / 2, 180, PinKind::Input),
                pin("B", "2", -3 * G, -G / 2, 180, PinKind::Input),
            ],
            graphics: gate_shape("and"),
            body: (4 * G, 2 * G),
        };
        sink(&mut and);
        self.add_symbol(and);

        let mut or = Symbol {
            name: "OR2".into(),
            description: "2-input OR".into(),
            pins: vec![
                pin("A", "1", -3 * G, G / 2, 180, PinKind::Input),
                pin("B", "2", -3 * G, -G / 2, 180, PinKind::Input),
            ],
            graphics: gate_shape("or"),
            body: (4 * G, 2 * G),
        };
        sink(&mut or);
        let mut xor = or.clone();
        self.add_symbol(or);
        xor.name = "XOR2".into();
        xor.description = "2-input XOR".into();
        xor.graphics.push(line(-2 * G - G / 2, -G, -2 * G - G / 2, G));
        self.add_symbol(xor);

        let mut not = Symbol {
            name: "NOT".into(),
            description: "Inverter".into(),
            pins: vec![pin("A", "1", -3 * G, 0, 180, PinKind::Input)],
            graphics: vec![
                Graphic::Polyline {
                    points: vec![
                        Point::new(-2 * G, -G),
                        Point::new(-2 * G, G),
                        Point::new(0, 0),
                        Point::new(-2 * G, -G),
                    ],
                },
                Graphic::Circle {
                    center: Point::new(G / 3, 0),
                    radius: G / 6,
                },
            ],
            body: (4 * G, 2 * G),
        };
        not.pins.push(pin("Y", "2", 2 * G, 0, 0, PinKind::Output));
        self.add_symbol(not);

        // NAND and NOR reuse the base shape plus an output bubble.
        for (name, base, desc) in [("NAND2", "and", "2-input NAND"), ("NOR2", "or", "2-input NOR")] {
            let mut s = if name.starts_with("NAND") {
                self.symbol("AND2").cloned().unwrap_or_else(|| and_symbol())
            } else {
                self.symbol("OR2").cloned().unwrap_or_else(|| or_symbol())
            };
            s.name = name.to_string();
            s.description = desc.to_string();
            s.graphics.push(Graphic::Circle {
                center: Point::new(G + G / 3, 0),
                radius: G / 6,
            });
            if base == "and" {
                s.graphics.retain(|g| {
                    !matches!(g, Graphic::Circle { center, radius } if *center == Point::new(0, 0) && *radius == G)
                });
                s.graphics.push(Graphic::Circle {
                    center: Point::new(0, 0),
                    radius: G,
                });
            }
            self.add_symbol(s);
        }

        let mut dff = Symbol {
            name: "DFF".into(),
            description: "D flip-flop".into(),
            pins: vec![
                pin("D", "1", -4 * G, G, 180, PinKind::Input),
                pin("CLK", "2", -4 * G, -G, 180, PinKind::Input),
                pin("Q", "3", 4 * G, G, 0, PinKind::Output),
                pin("QN", "4", 4 * G, -G, 0, PinKind::Output),
            ],
            graphics: vec![
                rect(-2 * G, -2 * G, 2 * G, 2 * G),
                text(-G, G / 2, "D"),
                text(G, G / 2, "Q"),
                line(-4 * G, G, -2 * G, G),
                line(-4 * G, -G, -2 * G, -G),
                line(2 * G, G, 4 * G, G),
                line(2 * G, -G, 4 * G, -G),
                Graphic::Polyline {
                    points: vec![
                        Point::new(-2 * G, -G - G / 4),
                        Point::new(-2 * G + G / 2, -G),
                        Point::new(-2 * G, -G + G / 4),
                    ],
                },
            ],
            body: (4 * G, 4 * G),
        };
        dff.description = "Positive edge triggered D flip-flop".into();
        self.add_symbol(dff);

        // 14-pad SOIC footprint for logic, 3-pad SOT-23 for two input gates.
        self.add_footprint(Footprint {
            name: "SOT23_3".into(),
            description: "SOT-23 three pad logic".into(),
            pads: vec![
                PadDef::smd("1", -1_000_000, -1_000_000, 900_000, 800_000),
                PadDef::smd("2", 1_000_000, 0, 900_000, 800_000),
                PadDef::smd("3", -1_000_000, 1_000_000, 900_000, 800_000),
            ],
            courtyard: (3_000_000, 3_200_000),
        });
        let mut soic_pads = Vec::new();
        for i in 0..7 {
            let y = -3_810_000 + i as i64 * 1_270_000;
            soic_pads.push(PadDef::smd(&format!("{}", i + 1), -2_700_000, y, 1_500_000, 700_000));
            soic_pads.push(PadDef::smd(&format!("{}", 14 - i), 2_700_000, y, 1_500_000, 700_000));
        }
        self.add_footprint(Footprint {
            name: "SOIC14".into(),
            description: "SOIC-14 logic".into(),
            pads: soic_pads,
            courtyard: (8_000_000, 10_000_000),
        });

        for (name, desc) in [
            ("AND2", "2-input AND gate"),
            ("OR2", "2-input OR gate"),
            ("XOR2", "2-input XOR gate"),
            ("NAND2", "2-input NAND gate"),
            ("NOR2", "2-input NOR gate"),
            ("NOT", "Inverter"),
            ("DFF", "D flip-flop"),
        ] {
            let fp = if name == "DFF" { "SOIC14" } else { "SOT23_3" };
            self.add_part(name, name, fp, name, "logic", desc, &[name.to_lowercase().as_str()]);
        }
    }

    fn add_connectors(&mut self) {
        for (n, pitch) in [(2usize, 2_540_000i64), (4, 2_540_000), (8, 2_540_000)] {
            let mut pads = Vec::new();
            for i in 0..n {
                let x = -(n as i64 - 1) * pitch / 2 + i as i64 * pitch;
                pads.push(PadDef::thru(&format!("{}", i + 1), x, 0, 1_800_000, 1_000_000));
            }
            self.add_footprint(Footprint {
                name: format!("PINHDR_{}", n),
                description: format!("{}-pin through-hole header", n),
                pads,
                courtyard: ((n as i64 + 1) * pitch, 3_000_000),
            });

            // Symbol: vertical row of pins on a body box.
            let mut pins = Vec::new();
            let mut graphics = Vec::new();
            let h = (n as i64 + 1) * G;
            graphics.push(rect(-G / 2, -h / 2, G / 2, h / 2));
            for i in 0..n {
                let y = h / 2 - (i as i64 + 1) * G;
                pins.push(pin(
                    &format!("{}", i + 1),
                    &format!("{}", i + 1),
                    2 * G,
                    y,
                    0,
                    PinKind::Passive,
                ));
                graphics.push(line(G / 2, y, 2 * G, y));
            }
            self.add_symbol(Symbol {
                name: format!("CONN{}", n),
                description: format!("{}-pin connector", n),
                pins,
                graphics,
                body: (3 * G, h),
            });
            self.add_part(
                &format!("CONN{}", n),
                &format!("CONN{}", n),
                &format!("PINHDR_{}", n),
                &format!("CONN{}", n),
                "connector",
                &format!("{}-pin header", n),
                &["connector", "header"],
            );
        }
    }

    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    pub fn footprints(&self) -> &[Footprint] {
        &self.footprints
    }

    pub fn part(&self, name: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.name == name)
    }

    pub fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|s| s.name == name)
    }

    pub fn footprint(&self, name: &str) -> Option<&Footprint> {
        self.footprints.iter().find(|f| f.name == name)
    }

    pub fn search(&self, query: &str) -> Vec<&Part> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.parts.iter().collect();
        }
        self.parts
            .iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&q)
                    || p.description.to_lowercase().contains(&q)
                    || p.keywords.iter().any(|k| k.contains(&q))
            })
            .collect()
    }
}

fn and_symbol() -> Symbol {
    Symbol {
        name: "AND2".into(),
        description: "2-input AND".into(),
        pins: Vec::new(),
        graphics: Vec::new(),
        body: (0, 0),
    }
}

fn or_symbol() -> Symbol {
    Symbol {
        name: "OR2".into(),
        description: "2-input OR".into(),
        pins: Vec::new(),
        graphics: Vec::new(),
        body: (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_consistent() {
        let lib = Library::standard();
        assert!(lib.parts().len() >= 20);
        for p in lib.parts() {
            assert!(lib.symbol(&p.symbol).is_some(), "missing symbol for {}", p.name);
            assert!(lib.footprint(&p.footprint).is_some(), "missing footprint for {}", p.name);
        }
    }

    #[test]
    fn symbols_have_unique_pin_names() {
        let lib = Library::standard();
        for s in lib.symbols() {
            let mut names: Vec<&str> = s.pins.iter().map(|p| p.name.as_str()).collect();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), s.pins.len(), "duplicate pin in {}", s.name);
        }
    }

    #[test]
    fn search_works() {
        let lib = Library::standard();
        assert!(lib.search("res").iter().any(|p| p.name == "R"));
        assert!(lib.search("flip").iter().any(|p| p.name == "DFF"));
        assert_eq!(lib.search("zzzz").len(), 0);
    }

    #[test]
    fn footprints_have_pads() {
        let lib = Library::standard();
        for f in lib.footprints() {
            assert!(!f.pads.is_empty(), "footprint {} has no pads", f.name);
            for p in &f.pads {
                assert!(p.size.0 > 0 && p.size.1 > 0);
            }
        }
    }
}
