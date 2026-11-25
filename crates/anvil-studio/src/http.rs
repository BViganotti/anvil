//! Minimal HTTP/1.1 server primitives for the Studio: bounded request parsing
//! and response writing. One request per connection.

use super::json::{self, JsonError, Value};
use anvil_core::util::limits;
use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::TcpStream;

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(|s| s.as_str())
    }

    pub fn query(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(|s| s.as_str())
    }

    pub fn json(&self) -> Result<Value, HttpError> {
        let text = std::str::from_utf8(&self.body)
            .map_err(|_| HttpError::bad_request("body is not UTF-8"))?;
        json::parse(text).map_err(|e: JsonError| HttpError::bad_request(e.to_string()))
    }

    pub fn json_or_empty(&self) -> Result<Value, HttpError> {
        if self.body.is_empty() {
            Ok(Value::Object(BTreeMap::new()))
        } else {
            self.json()
        }
    }
}

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Response {
            status,
            content_type: content_type.to_string(),
            headers: Vec::new(),
            body,
        }
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Response::new(status, "text/plain; charset=utf-8", body.into().into_bytes())
    }

    pub fn html(body: impl Into<String>) -> Self {
        Response::new(200, "text/html; charset=utf-8", body.into().into_bytes())
    }

    pub fn json(value: &Value) -> Self {
        Response::new(
            200,
            "application/json; charset=utf-8",
            value.to_json().into_bytes(),
        )
    }

    pub fn json_status(status: u16, value: &Value) -> Self {
        let mut r = Response::json(value);
        r.status = status;
        r
    }

    pub fn bytes(content_type: &str, body: Vec<u8>) -> Self {
        Response::new(200, content_type, body)
    }

    pub fn empty(status: u16) -> Self {
        Response::new(status, "text/plain; charset=utf-8", Vec::new())
    }

    pub fn error(status: u16, message: impl Into<String>) -> Self {
        let mut v = Value::object();
        v.set("error", Value::str(message.into()));
        Response::json_status(status, &v)
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpError {
    pub status: u16,
    pub message: String,
}

impl HttpError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        HttpError {
            status: 400,
            message: message.into(),
        }
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        HttpError {
            status: 413,
            message: message.into(),
        }
    }

    pub fn io(message: impl Into<String>) -> Self {
        HttpError {
            status: 400,
            message: message.into(),
        }
    }

    pub fn response(&self) -> Response {
        Response::error(self.status, self.message.clone())
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.message)
    }
}

impl std::error::Error for HttpError {}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn percent_decode(s: &str) -> Result<String, HttpError> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(HttpError::bad_request("bad percent escape"));
                }
                let hi = (bytes[i + 1] as char)
                    .to_digit(16)
                    .ok_or_else(|| HttpError::bad_request("bad percent escape"))?;
                let lo = (bytes[i + 2] as char)
                    .to_digit(16)
                    .ok_or_else(|| HttpError::bad_request("bad percent escape"))?;
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| HttpError::bad_request("bad percent encoding"))
}

fn parse_query(target: &str) -> Result<(String, BTreeMap<String, String>), HttpError> {
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (target, None),
    };
    if path.is_empty() || path.len() > 2048 {
        return Err(HttpError::bad_request("bad path"));
    }
    if path.contains("..") || path.contains('\\') || path.contains('\0') {
        return Err(HttpError::bad_request("bad path"));
    }
    let mut map = BTreeMap::new();
    if let Some(q) = query {
        if q.len() > 4096 {
            return Err(HttpError::bad_request("query too long"));
        }
        for pair in q.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let k = percent_decode(k)?;
            let v = percent_decode(v)?;
            if k.len() > 128 || v.len() > 1024 {
                return Err(HttpError::bad_request("query field too long"));
            }
            if map.len() >= 64 {
                return Err(HttpError::bad_request("too many query fields"));
            }
            map.insert(k, v);
        }
    }
    Ok((path.to_string(), map))
}

pub fn read_request(stream: &mut TcpStream) -> Result<Request, HttpError> {
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut header_end = None;
    let mut chunk = [0u8; 4096];
    while header_end.is_none() {
        if buf.len() > limits::MAX_HTTP_HEADER {
            return Err(HttpError::payload_too_large("headers too large"));
        }
        let n = stream
            .read(&mut chunk)
            .map_err(|e| HttpError::io(format!("read failed: {e}")))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        header_end = find_header_end(&buf);
    }
    let header_end = header_end.ok_or_else(|| HttpError::bad_request("incomplete headers"))?;
    if header_end > limits::MAX_HTTP_HEADER {
        return Err(HttpError::payload_too_large("headers too large"));
    }
    let header_text = std::str::from_utf8(&buf[..header_end])
        .map_err(|_| HttpError::bad_request("headers are not UTF-8"))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| HttpError::bad_request("missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| HttpError::bad_request("missing method"))?
        .to_ascii_uppercase();
    let target = parts
        .next()
        .ok_or_else(|| HttpError::bad_request("missing target"))?;
    let version = parts.next().unwrap_or("HTTP/1.1");
    if parts.next().is_some() {
        return Err(HttpError::bad_request("malformed request line"));
    }
    if method != "GET" && method != "POST" {
        return Err(HttpError {
            status: 405,
            message: "method not allowed".into(),
        });
    }
    if !(version == "HTTP/1.1" || version == "HTTP/1.0") {
        return Err(HttpError::bad_request("unsupported HTTP version"));
    }
    let (path, query) = parse_query(target)?;
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if headers.len() >= 100 {
            return Err(HttpError::bad_request("too many headers"));
        }
        let (k, v) = line
            .split_once(':')
            .ok_or_else(|| HttpError::bad_request("malformed header"))?;
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        if k.is_empty() || k.len() > 64 || v.len() > 4096 {
            return Err(HttpError::bad_request("bad header"));
        }
        if headers.contains_key(&k) {
            return Err(HttpError::bad_request("duplicate header"));
        }
        headers.insert(k, v.to_string());
    }
    if headers
        .get("transfer-encoding")
        .map(|v| !v.eq_ignore_ascii_case("identity"))
        .unwrap_or(false)
    {
        return Err(HttpError::bad_request("chunked encoding is not supported"));
    }
    let content_length = match headers.get("content-length") {
        Some(v) => {
            let n: usize = v
                .parse()
                .map_err(|_| HttpError::bad_request("bad content-length"))?;
            if n > limits::MAX_HTTP_BODY {
                return Err(HttpError::payload_too_large("body too large"));
            }
            n
        }
        None => 0,
    };
    let body_start = header_end + 4;
    let mut body = buf[body_start..].to_vec();
    if body.len() > content_length {
        body.truncate(content_length);
    }
    while body.len() < content_length {
        let need = content_length - body.len();
        let mut chunk = vec![0u8; need.min(65536)];
        let n = stream
            .read(&mut chunk)
            .map_err(|e| HttpError::io(format!("read failed: {e}")))?;
        if n == 0 {
            return Err(HttpError::bad_request("short body"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(Request {
        method,
        target: target.to_string(),
        path,
        query,
        headers,
        body,
    })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Response",
    }
}

pub fn write_response(stream: &mut TcpStream, res: &Response) -> std::io::Result<()> {
    let mut head = String::with_capacity(256);
    head.push_str(&format!("HTTP/1.1 {} {}\r\n", res.status, reason(res.status)));
    head.push_str(&format!("Content-Type: {}\r\n", res.content_type));
    head.push_str(&format!("Content-Length: {}\r\n", res.body.len()));
    head.push_str("Connection: close\r\n");
    head.push_str("Cache-Control: no-store\r\n");
    head.push_str("X-Content-Type-Options: nosniff\r\n");
    head.push_str("Referrer-Policy: no-referrer\r\n");
    for (k, v) in &res.headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&res.body)?;
    stream.flush()
}

/// Write SSE headers; the caller then streams events.
pub fn write_sse_headers(stream: &mut TcpStream) -> std::io::Result<()> {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nX-Accel-Buffering: no\r\n\r\n";
    stream.write_all(head.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parsing() {
        let (path, q) = parse_query("/api/x?a=1&b=hello%20world").unwrap();
        assert_eq!(path, "/api/x");
        assert_eq!(q.get("a").unwrap(), "1");
        assert_eq!(q.get("b").unwrap(), "hello world");
    }

    #[test]
    fn rejects_bad_paths() {
        assert!(parse_query("/../etc/passwd").is_err());
        assert!(parse_query("/?a=%GG").is_err());
        assert!(parse_query("").is_err());
    }

    #[test]
    fn response_json_is_valid() {
        let v = Value::str("ok");
        let r = Response::json(&v);
        assert_eq!(r.status, 200);
        assert_eq!(std::str::from_utf8(&r.body).unwrap(), "\"ok\"");
    }
}
