//! HTTP parsing and response writing contract over real sockets.

use anvil_studio::http::{read_request, write_response, write_sse_headers, Response};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

fn serve_once(request: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let req = request.to_string();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let r = read_request(&mut stream).unwrap();
        let body = format!(
            "{}|{}|{}|{}|{}",
            r.method,
            r.path,
            r.query("a").unwrap_or(""),
            r.header("content-type").unwrap_or(""),
            String::from_utf8_lossy(&r.body)
        );
        let res = Response::text(200, body);
        write_response(&mut stream, &res).unwrap();
    });
    let mut client = TcpStream::connect(addr).unwrap();
    client.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).unwrap();
    handle.join().unwrap();
    String::from_utf8_lossy(&buf).to_string()
}

#[test]
fn get_with_query() {
    let out = serve_once(
        "GET /api/x?a=hello%20world HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert!(out.starts_with("HTTP/1.1 200"));
    assert!(out.contains("GET|/api/x|hello world||"), "{out}");
    assert!(out.contains("Content-Length:"));
}

#[test]
fn post_with_body() {
    let body = "{\"k\":1}";
    let req = format!(
        "POST /api/y HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let out = serve_once(&req);
    assert!(out.contains("POST|/api/y||application/json|{\"k\":1}"), "{out}");
}

#[test]
fn malformed_requests_error_before_hanging() {
    for req in [
        "PUT / HTTP/1.1\r\nHost: x\r\n\r\n",
        "GET / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n",
        "GET / HTTP/1.1\r\nHost: x\r\nContent-Length: 999999999\r\n\r\n",
        "GET /../x HTTP/1.1\r\nHost: x\r\n\r\n",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let req = req.to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let err = read_request(&mut stream).unwrap_err();
            write_response(&mut stream, &err.response()).unwrap();
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(req.as_bytes()).unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).unwrap();
        handle.join().unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(
            text.starts_with("HTTP/1.1 4") || text.starts_with("HTTP/1.1 405"),
            "{text}"
        );
    }
}

#[test]
fn sse_headers_are_streaming() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        write_sse_headers(&mut stream).unwrap();
        stream.write_all(b"event: done\ndata: {}\n\n").unwrap();
    });
    let mut client = TcpStream::connect(addr).unwrap();
    let mut buf = Vec::new();
    client.read_to_end(&mut buf).unwrap();
    handle.join().unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains("text/event-stream"));
    assert!(text.contains("event: done"));
}

#[test]
fn response_status_and_headers() {
    let r = Response::error(404, "missing").with_header("X-Test", "yes");
    assert_eq!(r.status, 404);
    assert!(r.body.len() > 2);
    assert_eq!(r.headers.len(), 1);
    assert!(!r.is_success());
    let ok = Response::json(&anvil_studio::json::Value::bool(true));
    assert!(ok.is_success());
}
