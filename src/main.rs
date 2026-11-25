//! Anvil launcher. The product is the local Studio, not a CLI. This binary
//! starts the loopback server and opens the browser; batch export and example
//! generation exist for tests, documentation and benchmarks.

use anvil_core::library::Library;
use anvil_core::netlist::extract;
use anvil_export::export_board;
use anvil_pcb::generate::{board_from_netlist, BuildOptions};
use anvil_pcb::router::{route_all, RouterOptions};
use anvil_studio::project::Project;
use anvil_studio::{ServerOptions, start};
use std::fs;
use std::path::Path;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }
    let mut port = 0u16;
    let mut open_browser = true;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse::<u16>().ok()) {
                    Some(p) => port = p,
                    None => {
                        eprintln!("anvil: --port needs a number between 0 and 65535");
                        std::process::exit(2);
                    }
                }
            }
            "--no-browser" => open_browser = false,
            "--version" | "-V" => {
                println!("anvil {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "export" => {
                let dir = args.get(i + 1).map(|s| s.as_str()).unwrap_or("examples/out");
                if let Err(e) = batch_export(Path::new(dir)) {
                    eprintln!("anvil: export failed: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "generate-examples" => {
                if let Err(e) = generate_examples(Path::new("examples")) {
                    eprintln!("anvil: generate-examples failed: {e}");
                    std::process::exit(1);
                }
                return;
            }
            other => {
                // Unknown flags are ignored only if they were meant for the
                // subcommands above; anything else is a mistake.
                if other.starts_with('-') {
                    eprintln!("anvil: unknown option {other}");
                    std::process::exit(2);
                }
            }
        }
        i += 1;
    }

    match start(&ServerOptions {
        port,
        open_browser,
    }) {
        Ok(server) => {
            println!("Anvil Studio: {}", server.url);
            println!("Loopback only. Token is required by the API.");
            // No terminal interaction: the server runs until the process exits.
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
        Err(e) => {
            eprintln!("anvil: {e}");
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!(
        "Anvil {} — local browser EDA workbench\n\
         \n\
         Usage:\n\
           anvil                     launch Studio (random loopback port)\n\
           anvil --port 8787         launch on a fixed loopback port\n\
           anvil --no-browser        launch without opening the browser\n\
           anvil export [dir]        batch export the demo board\n\
           anvil generate-examples   write example projects to examples/\n\
           anvil --help              this text\n\
         \n\
         The Studio URL includes a random token; the API rejects requests without it.",
        env!("CARGO_PKG_VERSION")
    );
}

fn safe_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "project".to_string()
    } else {
        s
    }
}

fn generate_examples(dir: &Path) -> Result<(), String> {
    let lib = Library::standard();
    let projects = Project::examples(&lib)?;
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    for p in projects {
        let name = safe_name(&p.name);
        let path = dir.join(format!("{name}.json"));
        let text = p.to_json().to_json();
        fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

fn batch_export(dir: &Path) -> Result<(), String> {
    let lib = Library::standard();
    let project = Project::demo(&lib)?;
    let netlist = extract(&project.schematic, &lib);
    if netlist.errors() > 0 {
        return Err(format!("demo netlist has {} errors", netlist.errors()));
    }
    let board = board_from_netlist(&project.schematic, &netlist, &lib, &BuildOptions::default())
        .map_err(|e| e.to_string())?;
    let opts = RouterOptions::for_board(&board);
    let mut routed = board;
    let mut progress = |_p: &anvil_pcb::router::RouteProgress| {};
    let report = route_all(&mut routed, &opts, &mut progress).map_err(|e| e.to_string())?;
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let files = export_board(&routed);
    for f in &files {
        let path = dir.join(&f.name);
        fs::write(&path, &f.bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    println!(
        "exported {} files to {} ({} traces, {} vias, clean={}, DRC={})",
        files.len(),
        dir.display(),
        routed.traces.len(),
        routed.vias.len(),
        report.clean,
        report.drc.errors()
    );
    if !report.clean {
        println!("WARNING: autoroute did not reach zero DRC; files reflect actual geometry.");
    }
    Ok(())
}
