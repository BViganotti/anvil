//! Repository hygiene and delivery contract.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn required_documents_exist() {
    let r = root();
    for name in [
        "README.md",
        "LICENSE",
        "CHANGELOG.md",
        "LIMITATIONS.md",
        "PLAN.md",
        "REQUIREMENTS.md",
        "Makefile",
        "Cargo.toml",
        "Cargo.lock",
    ] {
        assert!(r.join(name).exists(), "missing {name}");
    }
    for doc in [
        "ARCHITECTURE.md",
        "SIMULATION.md",
        "ROUTER.md",
        "DRC.md",
        "GERBER.md",
        "STUDIO.md",
        "TESTING.md",
        "PROJECT_FORMAT.md",
        "LIBRARY.md",
        "SECURITY.md",
        "BENCHMARKS.md",
        "COVERAGE.txt",
    ] {
        assert!(r.join("docs").join(doc).exists(), "missing docs/{doc}");
    }
    let license = fs::read_to_string(r.join("LICENSE")).unwrap();
    assert!(license.contains("MIT License"));
    let readme = fs::read_to_string(r.join("README.md")).unwrap();
    assert!(readme.contains("Anvil"));
    assert!(readme.contains("docs/ARCHITECTURE.md"));
    assert!(readme.contains("LIMITATIONS.md"));
}

#[test]
fn no_ci_and_no_workflows() {
    let r = root();
    assert!(!r.join(".github").join("workflows").exists());
    for name in [".gitlab-ci.yml", "azure-pipelines.yml", ".travis.yml", "Jenkinsfile"] {
        assert!(!r.join(name).exists(), "unexpected CI file {name}");
    }
}

#[test]
fn dependencies_are_workspace_paths_only() {
    let r = root();
    let mut manifests = vec![r.join("Cargo.toml")];
    for dir in ["crates/anvil-core", "crates/anvil-pcb", "crates/anvil-export", "crates/anvil-studio", "bench", "fuzz"] {
        manifests.push(r.join(dir).join("Cargo.toml"));
    }
    for m in manifests {
        let text = fs::read_to_string(&m).unwrap();
        let mut in_deps = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_deps = line == "[dependencies]" || line == "[dev-dependencies]" || line == "[build-dependencies]";
                continue;
            }
            if in_deps && !line.is_empty() && !line.starts_with('#') {
                assert!(
                    line.contains("path =") || line.contains("workspace = true"),
                    "external dependency in {}: {line}",
                    m.display()
                );
            }
        }
    }
    let lock = fs::read_to_string(r.join("Cargo.lock")).unwrap();
    assert!(!lock.contains("registry+"), "Cargo.lock contains registry packages");
}

#[test]
fn assets_are_original_and_offline() {
    let r = root().join("assets");
    for name in ["index.html", "app.js", "app.css", "favicon.svg"] {
        let text = fs::read_to_string(r.join(name)).unwrap();
        assert!(!text.is_empty(), "{name} is empty");
        if name != "favicon.svg" {
            assert!(!text.contains("http://"), "{name} contains an external URL");
            assert!(!text.contains("https://"), "{name} contains an external URL");
        }
    }
    let index = fs::read_to_string(r.join("index.html")).unwrap();
    assert!(!index.contains("cdn"));
    assert!(index.contains("/app.js"));
    assert!(index.contains("/app.css"));
    let js = fs::read_to_string(r.join("app.js")).unwrap();
    assert!(!js.contains("eval("));
    assert!(!js.contains("document.write"));
}

#[test]
fn examples_and_tools_are_present() {
    let r = root();
    let examples = fs::read_dir(r.join("examples"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .count();
    assert!(examples >= 24, "only {examples} example projects");
    assert!(r.join("examples/gerbers").exists());
    assert!(r.join("bench/src/main.rs").exists());
    assert!(r.join("fuzz/src/main.rs").exists());
    assert!(r.join("tests/studio_playwright.py").exists());
    let makefile = fs::read_to_string(r.join("Makefile")).unwrap();
    for target in ["test:", "bench:", "fuzz:", "run:"] {
        assert!(makefile.contains(target));
    }
}

#[test]
fn no_stray_build_products_in_sources() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                if p.file_name().map(|n| n == "target").unwrap_or(false) {
                    continue;
                }
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root(), &mut files);
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy();
        assert!(!name.ends_with(".png") || f.starts_with(root().join("docs")), "stray image {}", f.display());
        assert!(!name.ends_with(".gbr") || f.starts_with(root().join("examples")), "stray Gerber {}", f.display());
        assert!(!name.ends_with(".drl") || f.starts_with(root().join("examples")), "stray drill {}", f.display());
    }
}
