//! Project JSON and example contract.

use anvil_core::library::Library;
use anvil_studio::json::{self, Value};
use anvil_studio::project::Project;

#[test]
fn all_examples_round_trip() {
    let lib = Library::standard();
    let projects = Project::examples(&lib).unwrap();
    assert_eq!(projects.len(), 9);
    for p in projects {
        let j = p.to_json();
        let text = j.to_json();
        let parsed = json::parse(&text).unwrap();
        let back = Project::from_json(&lib, &parsed).unwrap();
        assert_eq!(back.name, p.name);
        assert_eq!(
            back.schematic.components().len(),
            p.schematic.components().len()
        );
        assert_eq!(back.schematic.wires().len(), p.schematic.wires().len());
        assert_eq!(back.schematic.labels().len(), p.schematic.labels().len());
        assert_eq!(back.netlist(&lib).errors(), 0, "{}", p.name);
    }
}

#[test]
fn invalid_projects_are_rejected() {
    let lib = Library::standard();
    let mut v = Value::object();
    v.set("format", Value::str("other"));
    assert!(Project::from_json(&lib, &v).is_err());
    let mut v = Value::object();
    v.set("format", Value::str("anvil-project-1"));
    v.set("name", Value::str("Bad"));
    v.set("components", Value::arr(vec![Value::object()]));
    assert!(Project::from_json(&lib, &v).is_err());
    let mut v = Value::object();
    v.set("name", Value::str("Bad"));
    v.set("components", Value::arr(vec![]));
    v.set("wires", Value::arr(vec![Value::arr(vec![Value::int(1)])]));
    assert!(Project::from_json(&lib, &v).is_err());
}

#[test]
fn json_includes_sheet_parts_and_properties() {
    let lib = Library::standard();
    let p = Project::demo(&lib).unwrap();
    let j = p.to_json();
    assert_eq!(j.get("format").unwrap().as_str(), Some("anvil-project-1"));
    assert!(j.get("sheet").unwrap().get("title").unwrap().as_str().is_some());
    let comps = j.get("components").unwrap().as_array().unwrap();
    let v1 = comps
        .iter()
        .find(|c| c.get("refdes").unwrap().as_str() == Some("V1"))
        .unwrap();
    assert_eq!(v1.get("part").unwrap().as_str(), Some("V"));
    assert_eq!(
        v1.get("properties").unwrap().get("wave").unwrap().as_str(),
        Some("pulse")
    );
    let wires = j.get("wires").unwrap().as_array().unwrap();
    assert!(!wires.is_empty());
    assert_eq!(wires[0].get("points").unwrap().as_array().unwrap()[0].as_array().unwrap().len(), 2);
}

#[test]
fn missing_optional_fields_get_defaults() {
    let lib = Library::standard();
    let v = json::parse(
        r#"{"format":"anvil-project-1","name":"Minimal","components":[]}"#,
    )
    .unwrap();
    let p = Project::from_json(&lib, &v).unwrap();
    assert_eq!(p.name, "Minimal");
    assert_eq!(p.schematic.components().len(), 0);
    assert_eq!(p.schematic.sheet.title, "Untitled");
}

#[test]
fn demo_has_expected_nets() {
    let lib = Library::standard();
    let p = Project::demo(&lib).unwrap();
    let nl = p.netlist(&lib);
    assert_eq!(nl.errors(), 0);
    for name in ["VIN", "MID", "NLED", "GND"] {
        assert!(nl.net_by_name(name).is_some(), "{name}");
    }
    assert_eq!(nl.net_by_name("MID").unwrap().nodes.len(), 3);
}
