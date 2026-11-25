//! Local Studio HTTP server. Binds 127.0.0.1 only, uses a per-launch random
//! token for the API, serves embedded assets and streams autoroute progress
//! over SSE. No filesystem access, no external requests.

use crate::api::{self, StudioState};
use crate::http::{self, Response};
use crate::assets;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub port: u16,
    pub open_browser: bool,
}

impl Default for ServerOptions {
    fn default() -> Self {
        ServerOptions {
            port: 0,
            open_browser: true,
        }
    }
}

pub struct RunningServer {
    pub addr: SocketAddr,
    pub token: String,
    pub url: String,
    shutdown: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl RunningServer {
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Wake the accept loop.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(50));
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn random_token() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(1);
    let stack = &now as *const _ as usize;
    let mut h = RandomState::new().build_hasher();
    h.write_u128(now);
    h.write_usize(stack);
    h.write_u32(std::process::id());
    let a = h.finish();
    let mut h2 = RandomState::new().build_hasher();
    h2.write_u64(a);
    h2.write_u128(now.rotate_left(37));
    let b = h2.finish();
    format!("{:016x}{:016x}", a, b)
}

fn token_ok(provided: &str, token: &str) -> bool {
    if provided.len() != token.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in provided.bytes().zip(token.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn auth_ok(req: &http::Request, token: &str) -> bool {
    if let Some(t) = req.header("x-anvil-token") {
        return token_ok(t, token);
    }
    if let Some(t) = req.query("token") {
        return token_ok(t, token);
    }
    false
}

fn static_response(path: &str) -> Option<Response> {
    match path {
        "/" | "/index.html" => Some(
            Response::html(assets::INDEX).with_header(
                "Content-Security-Policy",
                "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
            ),
        ),
        "/app.css" => Some(Response::new(
            200,
            "text/css; charset=utf-8",
            assets::CSS.as_bytes().to_vec(),
        )),
        "/app.js" => Some(Response::new(
            200,
            "text/javascript; charset=utf-8",
            assets::JS.as_bytes().to_vec(),
        )),
        "/favicon.ico" => Some(Response::empty(204)),
        _ => None,
    }
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    let _ = cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn handle_sse(mut stream: TcpStream, state: Arc<Mutex<StudioState>>) {
    if http::write_sse_headers(&mut stream).is_err() {
        return;
    }
    let start = Instant::now();
    let mut last_seq = u64::MAX;
    let mut last_heartbeat = Instant::now();
    while start.elapsed() < Duration::from_secs(300) {
        let (seq, json, done) = api::job_snapshot(&state);
        if seq != last_seq {
            let event = if done { "done" } else { "progress" };
            let payload = format!("event: {}\ndata: {}\n\n", event, json.to_json());
            if stream.write_all(payload.as_bytes()).is_err() {
                break;
            }
            let _ = stream.flush();
            last_seq = seq;
            last_heartbeat = Instant::now();
            if done {
                break;
            }
        }
        if last_heartbeat.elapsed() > Duration::from_secs(5) {
            if stream.write_all(b": keepalive\n\n").is_err() {
                break;
            }
            let _ = stream.flush();
            last_heartbeat = Instant::now();
        }
        thread::sleep(Duration::from_millis(80));
    }
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<StudioState>>, token: String) {
    // Accepted sockets can inherit non-blocking mode from the listener on some
    // platforms. Studio responses (PNG previews) are much larger than the
    // socket buffer, so force blocking writes with a timeout.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let req = match http::read_request(&mut stream) {
        Ok(r) => r,
        Err(e) => {
            let _ = http::write_response(&mut stream, &e.response());
            return;
        }
    };
    if let Some(res) = static_response(&req.path) {
        // Static assets are public but contain no project data. The API token is
        // required for everything under /api.
        if req.path.starts_with("/api/") {
            // unreachable, handled below
        } else {
            let _ = http::write_response(&mut stream, &res);
            return;
        }
    }
    if !req.path.starts_with("/api/") {
        let _ = http::write_response(&mut stream, &Response::error(404, "not found"));
        return;
    }
    if !auth_ok(&req, &token) {
        let _ = http::write_response(
            &mut stream,
            &Response::error(401, "missing or invalid X-Anvil-Token"),
        );
        return;
    }
    if req.path == "/api/route/events" {
        if req.method != "GET" {
            let _ = http::write_response(&mut stream, &Response::error(405, "method not allowed"));
            return;
        }
        handle_sse(stream, state);
        return;
    }
    let res = api::dispatch(&state, &req);
    if let Err(e) = http::write_response(&mut stream, &res) {
        eprintln!("anvil: response write failed ({} bytes, {}): {e}", res.body.len(), req.path);
    }
}

pub fn start(opts: &ServerOptions) -> Result<RunningServer, String> {
    let listener = TcpListener::bind(("127.0.0.1", opts.port))
        .map_err(|e| format!("cannot bind 127.0.0.1:{}: {e}", opts.port))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("listener setup failed: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("cannot read local address: {e}"))?;
    let token = random_token();
    let state = Arc::new(Mutex::new(StudioState::new()));
    let url = format!("http://{}/?token={}", addr, token);
    let shutdown = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let thread_state = state.clone();
    let thread_token = token.clone();
    let thread_shutdown = shutdown.clone();
    let join = thread::spawn(move || {
        while !thread_shutdown.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if active.load(Ordering::SeqCst) >= 64 {
                        drop(stream);
                        continue;
                    }
                    active.fetch_add(1, Ordering::SeqCst);
                    let st = thread_state.clone();
                    let tk = thread_token.clone();
                    let act = active.clone();
                    thread::spawn(move || {
                        handle_connection(stream, st, tk);
                        act.fetch_sub(1, Ordering::SeqCst);
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(15));
                }
                Err(_) => break,
            }
        }
    });
    let server = RunningServer {
        addr,
        token,
        url: url.clone(),
        shutdown,
        join: Some(join),
    };
    if opts.open_browser {
        open_browser(&url);
    }
    Ok(server)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_generation_and_compare() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert!(token_ok(&a, &a));
        assert!(!token_ok(&b, &a));
        assert!(!token_ok("", &a));
    }

    #[test]
    fn server_starts_and_serves_health() {
        let server = start(&ServerOptions {
            port: 0,
            open_browser: false,
        })
        .unwrap();
        let url = format!("http://{}/api/health", server.addr);
        let mut stream = TcpStream::connect(server.addr).unwrap();
        let req = format!(
            "GET /api/health HTTP/1.1\r\nHost: localhost\r\nX-Anvil-Token: {}\r\nConnection: close\r\n\r\n",
            server.token
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stream, &mut buf);
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200"), "{}", text);
        assert!(text.contains("\"ok\":true"));
        let _ = url;
    }

    #[test]
    fn api_requires_token() {
        let server = start(&ServerOptions {
            port: 0,
            open_browser: false,
        })
        .unwrap();
        let mut stream = TcpStream::connect(server.addr).unwrap();
        let req = "GET /api/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
        stream.write_all(req.as_bytes()).unwrap();
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stream, &mut buf);
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 401"), "{}", text);
    }
}
