//! Static asset and security-header contract for the loopback server.

use anvil_studio::{start, RunningServer, ServerOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn get(server: &RunningServer, path: &str, token: Option<&str>) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(server.addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut req = format!("GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n", path);
    if let Some(t) = token {
        req.push_str(&format!("X-Anvil-Token: {}\r\n", t));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = buf[split + 4..].to_vec();
    (status, head, body)
}

#[test]
fn assets_are_public_and_offline() {
    let server = start(&ServerOptions { port: 0, open_browser: false }).unwrap();
    let (status, head, body) = get(&server, "/", None);
    assert_eq!(status, 200);
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("Anvil"));
    assert!(head.contains("Content-Security-Policy"));
    assert!(head.contains("script-src 'self'"));
    assert!(!html.contains("http://") && !html.contains("https://"));
    let (css_status, css_head, css) = get(&server, "/app.css", None);
    assert_eq!(css_status, 200);
    assert!(css_head.contains("text/css"));
    assert!(String::from_utf8(css).unwrap().contains("--bg"));
    let (js_status, _, js) = get(&server, "/app.js", None);
    assert_eq!(js_status, 200);
    assert!(String::from_utf8(js).unwrap().contains("Anvil Studio"));
    let (favicon, _, empty) = get(&server, "/favicon.ico", None);
    assert_eq!(favicon, 204);
    assert!(empty.is_empty());
    let (missing, _, _) = get(&server, "/nope", None);
    assert_eq!(missing, 404);
}

#[test]
fn api_accepts_header_or_query_token() {
    let server = start(&ServerOptions { port: 0, open_browser: false }).unwrap();
    assert_eq!(get(&server, "/api/state", None).0, 401);
    assert_eq!(get(&server, "/api/state", Some(&server.token)).0, 200);
    assert_eq!(
        get(&server, &format!("/api/health?token={}", server.token), None).0,
        200
    );
    assert_eq!(get(&server, "/api/health?token=no", None).0, 401);
}

#[test]
fn responses_advertise_no_store_and_nosniff() {
    let server = start(&ServerOptions { port: 0, open_browser: false }).unwrap();
    let (_, head, _) = get(&server, "/api/health", Some(&server.token));
    assert!(head.contains("Cache-Control: no-store"));
    assert!(head.contains("X-Content-Type-Options: nosniff"));
    assert!(head.contains("Referrer-Policy: no-referrer"));
    assert!(head.contains("Connection: close"));
}
