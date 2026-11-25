//! Direct API dispatch contract (no socket layer).

use anvil_studio::api::{self, StudioState};
use anvil_studio::http::Request;
use anvil_studio::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn req(method: &str, target: &str, body: &str) -> Request {
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => {
            let mut map = BTreeMap::new();
            for pair in q.split('&').filter(|s| !s.is_empty()) {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                map.insert(k.to_string(), v.to_string());
            }
            (p.to_string(), map)
        }
        None => (target.to_string(), BTreeMap::new()),
    };
    Request {
        method: method.to_string(),
        target: target.to_string(),
        path,
        query,
        headers: BTreeMap::new(),
        body: body.as_bytes().to_vec(),
    }
}

fn json_body(r: &anvil_studio::http::Response) -> anvil_studio::json::Value {
    json::parse(std::str::from_utf8(&r.body).unwrap()).unwrap()
}

fn state() -> Arc<Mutex<StudioState>> {
    Arc::new(Mutex::new(StudioState::new()))
}

#[test]
fn state_and_library_endpoints() {
    let st = state();
    let r = api::dispatch(&st, &req("GET", "/api/state", ""));
    assert_eq!(r.status, 200);
    let v = json_body(&r);
    assert_eq!(v.get("name").unwrap().as_str(), Some("Anvil demo"));
    let l = api::dispatch(&st, &req("GET", "/api/library", ""));
    assert_eq!(l.status, 200);
    assert!(json_body(&l).get("parts").unwrap().as_array().unwrap().len() >= 20);
}

#[test]
fn simulation_endpoint() {
    let st = state();
    let tran = api::dispatch(
        &st,
        &req(
            "POST",
            "/api/simulate",
            r#"{"mode":"tran","tstep":0.000001,"tstop":0.001,"method":"be"}"#,
        ),
    );
    assert_eq!(tran.status, 200, "{}", String::from_utf8_lossy(&tran.body));
    let j = json_body(&tran);
    assert_eq!(j.get("mode").unwrap().as_str(), Some("tran"));
    assert!(j.get("series").unwrap().as_array().unwrap().iter().any(|s| {
        s.get("name").unwrap().as_str() == Some("MID")
    }));
    let dc = api::dispatch(&st, &req("POST", "/api/simulate", r#"{"mode":"dc"}"#));
    assert_eq!(dc.status, 200);
    let bad = api::dispatch(
        &st,
        &req("POST", "/api/simulate", r#"{"mode":"tran","tstep":1,"tstop":0.001}"#),
    );
    assert_eq!(bad.status, 400);
}

#[test]
fn schematic_edit_round_trip() {
    let st = state();
    let placed = api::dispatch(
        &st,
        &req("POST", "/api/schematic/place", r#"{"part":"R","at":[1270000,1270000]}"#),
    );
    assert_eq!(placed.status, 200);
    let pj = json_body(&placed);
    let id = pj.get("id").unwrap().as_i64().unwrap();
    assert_eq!(
        pj.get("state").unwrap().get("board").unwrap().get("present").unwrap().as_bool(),
        Some(false)
    );
    let moved = api::dispatch(&st, &req("POST", "/api/schematic/move", &format!(r#"{{"id":{id},"at":[2540000,2540000]}}"#)));
    assert_eq!(moved.status, 200);
    let rotated = api::dispatch(&st, &req("POST", "/api/schematic/rotate", &format!(r#"{{"id":{id},"delta":90}}"#)));
    assert_eq!(rotated.status, 200);
    let mirrored = api::dispatch(&st, &req("POST", "/api/schematic/mirror", &format!(r#"{{"id":{id}}}"#)));
    assert_eq!(mirrored.status, 200);
    let valued = api::dispatch(&st, &req("POST", "/api/schematic/value", &format!(r#"{{"id":{id},"value":"4.7k"}}"#)));
    assert_eq!(valued.status, 200);
    let prop = api::dispatch(&st, &req("POST", "/api/schematic/property", &format!(r#"{{"id":{id},"key":"note","value":"x"}}"#)));
    assert_eq!(prop.status, 200);
    let wire = api::dispatch(
        &st,
        &req("POST", "/api/schematic/wire", r#"{"points":[[0,0],[1270000,0]]}"#),
    );
    assert_eq!(wire.status, 200);
    let label = api::dispatch(
        &st,
        &req("POST", "/api/schematic/label", r#"{"name":"N","at":[0,0],"kind":"local"}"#),
    );
    assert_eq!(label.status, 200);
    let del = api::dispatch(&st, &req("POST", "/api/schematic/delete", &format!(r#"{{"kind":"component","id":{id}}}"#)));
    assert_eq!(del.status, 200);
    let bad_del = api::dispatch(&st, &req("POST", "/api/schematic/delete", r#"{"kind":"label","id":9999}"#));
    assert_eq!(bad_del.status, 400);
}

#[test]
fn board_drc_route_and_export_endpoints() {
    let st = state();
    let gen = api::dispatch(&st, &req("POST", "/api/board/generate", "{}"));
    assert_eq!(gen.status, 200);
    let board = api::dispatch(&st, &req("GET", "/api/board", ""));
    assert_eq!(board.status, 200);
    assert!(!json_body(&board).get("footprints").unwrap().as_array().unwrap().is_empty());
    let drc = api::dispatch(&st, &req("GET", "/api/drc", ""));
    assert_eq!(drc.status, 200);
    assert!(json_body(&drc).get("unrouted").unwrap().as_i64().unwrap() > 0);
    let route = api::dispatch(&st, &req("POST", "/api/route/run-sync", "{}"));
    assert_eq!(route.status, 200);
    let rj = json_body(&route);
    assert_eq!(rj.get("route").unwrap().get("clean").unwrap().as_bool(), Some(true));
    let preview = api::dispatch(&st, &req("GET", "/api/export/preview.png?layer=top", ""));
    assert_eq!(preview.status, 200);
    assert_eq!(&preview.body[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    let gerber = api::dispatch(&st, &req("GET", "/api/export/gerber?layer=top", ""));
    assert_eq!(gerber.status, 200);
    assert!(String::from_utf8_lossy(&gerber.body).contains("M02*"));
    let drill = api::dispatch(&st, &req("GET", "/api/export/drill", ""));
    assert_eq!(drill.status, 200);
    let manifest = api::dispatch(&st, &req("GET", "/api/export/manifest", ""));
    assert_eq!(manifest.status, 200);
    assert!(json_body(&manifest).get("files").unwrap().as_array().unwrap().len() >= 5);
    let clear = api::dispatch(&st, &req("POST", "/api/board/clear-routes", "{}"));
    assert_eq!(clear.status, 200);
    let board = api::dispatch(&st, &req("GET", "/api/board", ""));
    assert!(json_body(&board).get("traces").unwrap().as_array().unwrap().is_empty());
}

#[test]
fn project_endpoints_and_errors() {
    let st = state();
    let load = api::dispatch(&st, &req("POST", "/api/project/load", r#"{"name":"RC step response"}"#));
    assert_eq!(load.status, 200);
    assert_eq!(json_body(&load).get("name").unwrap().as_str(), Some("RC step response"));
    let load_bad = api::dispatch(&st, &req("POST", "/api/project/load", r#"{"name":"nope"}"#));
    assert_eq!(load_bad.status, 404);
    let demo = api::dispatch(&st, &req("POST", "/api/project/demo", "{}"));
    assert_eq!(demo.status, 200);
    let reset = api::dispatch(&st, &req("POST", "/api/project/reset", "{}"));
    assert_eq!(reset.status, 200);
    let reset_json = json_body(&reset);
    assert_eq!(reset_json.get("name").unwrap().as_str(), Some("Untitled"));
    assert_eq!(reset_json.get("schematic").unwrap().get("components").unwrap().as_array().unwrap().len(), 0);
    assert_eq!(api::dispatch(&st, &req("GET", "/api/does-not-exist", "")).status, 404);
    assert_eq!(api::dispatch(&st, &req("DELETE", "/api/state", "")).status, 405);
}
