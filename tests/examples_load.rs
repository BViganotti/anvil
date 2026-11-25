//! Shipped examples must match the builders and remain simulatable.

use anvil_core::library::Library;
use anvil_core::sim::{compile_schematic, CompileOptions};
use anvil_studio::json;
use anvil_studio::project::Project;
use std::fs;
use std::path::Path;

#[test]
fn example_files_match_the_builders() {
    let lib = Library::standard();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let built = Project::examples(&lib).unwrap();
    assert!(built.len() >= 9);
    for p in built {
        let slug = p
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect::<String>()
            .replace("--", "-")
            .trim_matches('-')
            .to_string();
        let path = dir.join(format!("{slug}.json"));
        assert!(path.exists(), "missing example {}", path.display());
        let text = fs::read_to_string(&path).unwrap();
        let parsed = json::parse(&text).unwrap();
        let loaded = Project::from_json(&lib, &parsed).unwrap();
        assert_eq!(loaded.name, p.name);
        let rebuilt = json::parse(&p.to_json().to_json()).unwrap();
        assert_eq!(parsed, rebuilt, "{} is stale; regenerate examples", p.name);
        let nl = loaded.netlist(&lib);
        assert_eq!(nl.errors(), 0, "{}: {:?}", p.name, nl.diagnostics);
        compile_schematic(&loaded.schematic, &nl, &lib, &CompileOptions::default())
            .unwrap_or_else(|e| panic!("{}: {e}", p.name));
    }
}

#[test]
fn examples_directory_has_only_expected_projects() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut json_files = 0usize;
    for entry in fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
            json_files += 1;
        }
    }
    assert_eq!(json_files, 9);
}

#[test]
fn routed_gerber_examples_parse() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/gerbers");
    let mut gerbers = 0;
    let mut drills = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = fs::read_to_string(&path).unwrap();
        if name.ends_with(".gbr") {
            let doc = anvil_export::gerber::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!doc.primitives.is_empty() || name.contains("B_Cu"), "{name}");
            gerbers += 1;
        } else if name.ends_with(".drl") {
            let d = anvil_export::drill::parse(&text).unwrap();
            assert!(!d.hits.is_empty());
            drills += 1;
        }
    }
    assert_eq!(gerbers, 4);
    assert_eq!(drills, 1);
}
