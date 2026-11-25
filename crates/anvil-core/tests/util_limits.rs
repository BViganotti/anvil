//! Utility and limit contract.

use anvil_core::util::{fmt_mm, limits, parse_eng, parse_i64_limited, Rng};
use anvil_core::{snap, GRID_NM};

fn approx(got: Option<f64>, want: f64) {
    let got = got.unwrap_or_else(|| panic!("expected {want}, got None"));
    assert!(
        (got - want).abs() <= 1e-12 * want.abs().max(1e-300),
        "got {got}, want {want}"
    );
}

#[test]
fn engineering_suffixes() {
    approx(parse_eng("1p"), 1e-12);
    approx(parse_eng("2.2n"), 2.2e-9);
    approx(parse_eng("4.7u"), 4.7e-6);
    approx(parse_eng("3m"), 3e-3);
    approx(parse_eng("1K"), 1000.0);
    approx(parse_eng("2M"), 2e6);
    approx(parse_eng("1G"), 1e9);
    approx(parse_eng("1T"), 1e12);
    approx(parse_eng(" 5 "), 5.0);
    approx(parse_eng("1e-6"), 1e-6);
    approx(parse_eng("4.7kOhm"), 4700.0);
    for bad in ["", ".", "k", "1kkkkkkkkkk", "1e", "nan", "inf", "--1", "1 2"] {
        assert_eq!(parse_eng(bad), None, "{bad:?}");
    }
}

#[test]
fn integer_parsing_is_bounded() {
    assert_eq!(parse_i64_limited("0", -10, 10), Some(0));
    assert_eq!(parse_i64_limited("-10", -10, 10), Some(-10));
    assert_eq!(parse_i64_limited("11", -10, 10), None);
    assert_eq!(parse_i64_limited("x", -10, 10), None);
    assert_eq!(parse_i64_limited("", -10, 10), None);
}

#[test]
fn rng_ranges_and_shuffle() {
    let mut a = Rng::new(99);
    let mut b = Rng::new(99);
    for _ in 0..200 {
        assert_eq!(a.below(7), b.below(7));
        assert_eq!(a.range_i64(-3, 3), b.range_i64(-3, 3));
    }
    let mut values: Vec<u32> = (0..100).collect();
    a.shuffle(&mut values);
    values.sort_unstable();
    assert_eq!(values, (0..100).collect::<Vec<_>>());
    assert_eq!(Rng::new(1).below(0), 0);
    assert_eq!(Rng::new(1).range_i64(5, 5), 5);
}

#[test]
fn formatting_and_snap() {
    assert_eq!(fmt_mm(0), "0");
    assert_eq!(fmt_mm(1_270_000), "1.27");
    assert_eq!(fmt_mm(-1_000_000), "-1");
    assert_eq!(snap(0), 0);
    assert_eq!(snap(GRID_NM), GRID_NM);
    assert_eq!(snap(GRID_NM + 1000), GRID_NM);
    assert_eq!(snap(GRID_NM * 3 / 2), GRID_NM * 2);
}

#[test]
fn limits_are_positive_and_ordered() {
    assert!(limits::MAX_COMPONENTS >= 1000);
    assert!(limits::MAX_WIRES >= limits::MAX_COMPONENTS);
    assert!(limits::MAX_JSON_DEPTH >= 16);
    assert!(limits::MAX_JSON_BYTES >= 1_000_000);
    assert!(limits::MAX_HTTP_BODY >= 1_000_000);
    assert!(limits::MAX_RASTER >= 1000);
    assert!(limits::MAX_SIM_NODES >= 64);
}
