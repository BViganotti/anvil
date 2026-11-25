//! End-to-end Studio HTTP API acceptance tests over the real loopback server.

use anvil_studio::json::{self, Value};
use anvil_studio::{start, RunningServer, ServerOptions};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

struct Http {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl Http {
    fn json(&self) -> Value {
        json::parse(std::str::from_utf8(&self.body).unwrap()).unwrap()
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

fn start_server() -> RunningServer {
    start(&ServerOptions {
        port: 0,
        open_browser: false,
    })
    .unwrap()
}

fn request(server: &RunningServer, method: &str, path: &str, token: Option<&str>, body: &str) -> Http {
    let mut stream = TcpStream::connect(server.addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n",
        method,
        path,
        body.len()
    );
    if let Some(t) = token {
        req.push_str(&format!("X-Anvil-Token: {}\r\n", t));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("headers");
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let mut body = buf[split + 4..].to_vec();
    if let Some(len) = headers.get("content-length") {
        let n: usize = len.parse().unwrap();
        body.truncate(n);
        assert_eq!(body.len(), n, "short response body");
    }
    Http {
        status,
        headers,
        body,
    }
}

#[test]
fn studio_full_flow_over_http() {
    let server = start_server();
    let token = server.token.clone();

    // Static assets are public; the API is not.
    let index = request(&server, "GET", "/", None, "");
    assert_eq!(index.status, 200);
    assert!(index.text().contains("Anvil"));
    assert!(index.headers.get("content-security-policy").unwrap().contains("default-src 'self'"));
    let no_auth = request(&server, "GET", "/api/state", None, "");
    assert_eq!(no_auth.status, 401);
    let bad_auth = request(&server, "GET", "/api/state", Some("deadbeef"), "");
    assert_eq!(bad_auth.status, 401);
    assert_eq!(request(&server, "GET", "/app.js", None, "").status, 200);

    let health = request(&server, "GET", "/api/health", Some(&token), "");
    assert_eq!(health.status, 200);
    assert_eq!(health.json().get("ok").unwrap().as_bool(), Some(true));

    let state = request(&server, "GET", "/api/state", Some(&token), "");
    assert_eq!(state.status, 200);
    let sj = state.json();
    assert_eq!(sj.get("name").unwrap().as_str(), Some("Anvil demo"));
    assert!(!sj.get("netlist").unwrap().get("nets").unwrap().as_array().unwrap().is_empty());

    let library = request(&server, "GET", "/api/library", Some(&token), "");
    assert_eq!(library.status, 200);
    assert!(library.json().get("parts").unwrap().as_array().unwrap().len() >= 20);

    let sim_body = r#"{"mode":"tran","tstep":0.000001,"tstop":0.002,"method":"trap"}"#;
    let sim = request(&server, "POST", "/api/simulate", Some(&token), sim_body);
    assert_eq!(sim.status, 200, "{}", sim.text());
    let simj = sim.json();
    assert_eq!(simj.get("mode").unwrap().as_str(), Some("tran"));
    assert!(!simj.get("time_ms").unwrap().as_array().unwrap().is_empty());
    let series = simj.get("series").unwrap().as_array().unwrap();
    assert!(!series.is_empty());
    assert!(series.iter().any(|s| s.get("name").unwrap().as_str() == Some("MID")));

    let dc = request(
        &server,
        "POST",
        "/api/simulate",
        Some(&token),
        r#"{"mode":"dc"}"#,
    );
    assert_eq!(dc.status, 200);
    assert_eq!(dc.json().get("mode").unwrap().as_str(), Some("dc"));

    let bad_json = request(&server, "POST", "/api/simulate", Some(&token), "{");
    assert_eq!(bad_json.status, 400);
    assert_eq!(
        request(&server, "GET", "/api/nope", Some(&token), "").status,
        404
    );

    let placed = request(
        &server,
        "POST",
        "/api/schematic/place",
        Some(&token),
        r#"{"part":"R","at":[1270000,1270000]}"#,
    );
    assert_eq!(placed.status, 200);
    let pj = placed.json();
    assert_eq!(pj.get("state").unwrap().get("board").unwrap().get("present").unwrap().as_bool(), Some(false));
    // Remove it again by moving on: the demo can be restored for routing.
    let demo = request(&server, "POST", "/api/project/demo", Some(&token), "{}");
    assert_eq!(demo.status, 200);

    let generated = request(&server, "POST", "/api/board/generate", Some(&token), "{}");
    assert_eq!(generated.status, 200);
    let board = request(&server, "GET", "/api/board", Some(&token), "");
    assert_eq!(board.status, 200);
    let bj = board.json();
    assert!(!bj.get("footprints").unwrap().as_array().unwrap().is_empty());
    assert!(!bj.get("outline").unwrap().as_array().unwrap().is_empty());

    let drc0 = request(&server, "GET", "/api/drc", Some(&token), "");
    assert_eq!(drc0.status, 200);
    assert!(drc0.json().get("unrouted").unwrap().as_i64().unwrap() > 0);

    let routed = request(&server, "POST", "/api/route/run-sync", Some(&token), "{}");
    assert_eq!(routed.status, 200, "{}", routed.text());
    let rj = routed.json();
    let route = rj.get("route").unwrap();
    assert_eq!(route.get("clean").unwrap().as_bool(), Some(true), "{}", route.to_json());
    let board2 = request(&server, "GET", "/api/board", Some(&token), "");
    let b2 = board2.json();
    assert!(!b2.get("traces").unwrap().as_array().unwrap().is_empty());

    let drc = request(&server, "GET", "/api/drc", Some(&token), "");
    assert_eq!(drc.json().get("clean").unwrap().as_bool(), Some(true));

    let preview = request(&server, "GET", "/api/export/preview.png?layer=top", Some(&token), "");
    assert_eq!(preview.status, 200);
    assert_eq!(&preview.body[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    assert!(preview.body.len() > 10_000);

    let gerber = request(&server, "GET", "/api/export/gerber?layer=top", Some(&token), "");
    assert_eq!(gerber.status, 200);
    let gt = gerber.text();
    assert!(gt.contains("%FSLAX46Y46*%"));
    assert!(gt.contains("M02*"));
    assert!(anvil_export::gerber::parse(&gt).is_ok());

    let drill = request(&server, "GET", "/api/export/drill", Some(&token), "");
    assert_eq!(drill.status, 200);
    assert!(drill.text().contains("M48"));

    let manifest = request(&server, "GET", "/api/export/manifest", Some(&token), "");
    assert_eq!(manifest.status, 200);
    assert!(manifest.json().get("files").unwrap().as_array().unwrap().len() >= 5);
}

#[test]
fn studio_route_sse_streams_progress() {
    let server = start_server();
    let token = server.token.clone();
    let start = request(&server, "POST", "/api/route/start", Some(&token), "{}");
    assert_eq!(start.status, 202);

    let mut stream = TcpStream::connect(server.addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let req = format!(
        "GET /api/route/events?token={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        token
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200"), "{}", &text[..text.len().min(200)]);
    assert!(text.contains("text/event-stream"));
    assert!(text.contains("event: progress") || text.contains("event: done"));
    assert!(text.contains("\"phase\""));
}

#[test]
fn studio_rejects_oversized_and_malformed_requests() {
    let server = start_server();
    let token = server.token.clone();
    // Too-large declared body is rejected before reading it.
    let mut stream = TcpStream::connect(server.addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let req = format!(
        "POST /api/simulate HTTP/1.1\r\nHost: x\r\nX-Anvil-Token: {}\r\nContent-Length: 999999999\r\n\r\n",
        token
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 413"), "{}", text);
    // Bad method.
    let put = request(&server, "PUT", "/api/state", Some(&token), "");
    assert_eq!(put.status, 405);
    // Bad query token.
    let sse = request(&server, "GET", "/api/route/events?token=wrong", None, "");
    assert_eq!(sse.status, 401);
}
