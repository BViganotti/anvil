//! Launcher batch contract: help/version, export files really parse.

use anvil_export::{drill, gerber};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn temp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("anvil-test-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn version_and_help() {
    let bin = env!("CARGO_BIN_EXE_anvil");
    let v = Command::new(bin).arg("--version").output().unwrap();
    assert!(v.status.success());
    assert!(String::from_utf8_lossy(&v.stdout).contains("anvil"));
    let h = Command::new(bin).arg("--help").output().unwrap();
    assert!(h.status.success());
    let text = String::from_utf8_lossy(&h.stdout);
    assert!(text.contains("launch Studio"));
    assert!(text.contains("export"));
}

#[test]
fn export_writes_parseable_clean_files() {
    let bin = env!("CARGO_BIN_EXE_anvil");
    let dir = temp_dir("export");
    let out = Command::new(bin)
        .args(["export", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("clean=true"), "{text}");
    let mut gerbers = 0;
    let mut drills = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let content = fs::read_to_string(&path).unwrap();
        if name.ends_with(".gbr") {
            gerber::parse(&content).unwrap_or_else(|e| panic!("{name}: {e}"));
            gerbers += 1;
        } else if name.ends_with(".drl") {
            let d = drill::parse(&content).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!d.hits.is_empty());
            drills += 1;
        }
    }
    assert_eq!(gerbers, 4);
    assert_eq!(drills, 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unknown_option_fails_with_status_two() {
    let bin = env!("CARGO_BIN_EXE_anvil");
    let out = Command::new(bin).arg("--definitely-not").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
