//! JSON parser/serializer edge contract.

use anvil_studio::json::{parse, Value};

#[test]
fn values_and_accessors() {
    let v = parse(r#"{"a":1,"b":-2.5,"c":true,"d":null,"e":[1,"x"],"f":{"g":"h"}}"#).unwrap();
    assert_eq!(v.get("a").unwrap().as_i64(), Some(1));
    assert_eq!(v.get("b").unwrap().as_f64(), Some(-2.5));
    assert_eq!(v.get("c").unwrap().as_bool(), Some(true));
    assert!(v.get("d").unwrap().is_null());
    assert_eq!(v.get("e").unwrap().as_array().unwrap().len(), 2);
    assert_eq!(v.get("f").unwrap().get("g").unwrap().as_str(), Some("h"));
    assert!(v.get("missing").is_none());
}

#[test]
fn escaping_round_trips() {
    let original = Value::str("quote\" slash\\ newline\n tab\t bell\u{7} snowman ☃");
    let text = original.to_json();
    let back = parse(&text).unwrap();
    assert_eq!(back, original);
}

#[test]
fn numbers_serialize_compactly() {
    assert_eq!(parse("1e3").unwrap().to_json(), "1000");
    assert_eq!(parse("1.5").unwrap().to_json(), "1.5");
    assert_eq!(parse("-0.0").unwrap().to_json(), "0");
    let big = Value::int(1_000_000_000_000);
    assert_eq!(parse(&big.to_json()).unwrap(), big);
}

#[test]
fn invalid_inputs_are_rejected() {
    for bad in [
        "", "{", "[", "\"", "tru", "nul", "01", "+1", "1.", ".5", "1e", "{\"a\":}",
        "{\"a\" 1}", "[\"a\" \"b\"]", "{}x", "\"\\q\"", "\"\u{1}\"",
    ] {
        assert!(parse(bad).is_err(), "accepted {bad:?}");
    }
}

#[test]
fn depth_and_duplicate_limits() {
    let deep = format!("{}1{}", "[".repeat(80), "]".repeat(80));
    assert!(parse(&deep).is_err());
    assert!(parse(r#"{"x":1,"x":2}"#).is_err());
}

#[test]
fn unicode_surrogates() {
    let v = parse(r#""\ud83d\ude00""#).unwrap();
    assert_eq!(v.as_str(), Some("😀"));
    assert!(parse(r#""\ud83d""#).is_err());
    assert!(parse(r#""\ude00""#).is_err());
}

#[test]
fn error_positions_are_present() {
    let e = parse("{\"a\": }").unwrap_err();
    assert!(e.at > 0);
    assert!(!e.to_string().is_empty());
}
