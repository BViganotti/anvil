//! Malformed HTTP request campaign against the real server.

use anvil_core::util::Rng;
use anvil_studio::{start, ServerOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn raw(addr: std::net::SocketAddr, text: &[u8]) {
    if let Ok(mut stream) = TcpStream::connect(addr) {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
        let _ = stream.write_all(text);
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
    }
}

#[test]
fn server_survives_malformed_requests() {
    let server = start(&ServerOptions { port: 0, open_browser: false }).unwrap();
    let mut rng = Rng::new(0xF00D_BEEF);
    let methods = ["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "BOGUS"];
    let paths = ["/", "/api/state", "/api/simulate", "/../../etc/passwd", "/api/x?y=%", "/app.js"];
    for i in 0..60 {
        let method = methods[rng.below(methods.len() as u32) as usize];
        let path = paths[rng.below(paths.len() as u32) as usize];
        let len = if i % 11 == 0 { "abc".to_string() } else { "0".to_string() };
        let mut req = format!(
            "{} {} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n",
            method, path, len
        );
        if rng.below(2) == 0 {
            req.push_str(&format!("X-Anvil-Token: {}\r\n", server.token));
        }
        if i % 7 == 0 {
            req.push_str("BadHeader\r\n");
        }
        req.push_str("\r\n");
        let n = rng.below(48) as usize;
        for _ in 0..n {
            req.push((b' ' + (rng.below(90) as u8)) as char);
        }
        raw(server.addr, req.as_bytes());
    }
    // A binary blob with no request line.
    raw(server.addr, &[0xff, 0x00, 0x13, 0x37, b'\r', b'\n', b'\r', b'\n']);
    // The server is still healthy.
    let mut stream = TcpStream::connect(server.addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let req = format!(
        "GET /api/health HTTP/1.1\r\nHost: localhost\r\nX-Anvil-Token: {}\r\nConnection: close\r\n\r\n",
        server.token
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200"), "{text}");
    assert!(text.contains("\"ok\":true"));
}
