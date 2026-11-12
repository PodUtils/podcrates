//! Stand-ins of the services of other parties, on the loopback, for the
//! offline tests of a watch or a client (feature `stand`).
//!
//! * [`http`]: an HTTP/1.1 server that answers each request with what the
//!   caller's handler gives; a `101` answer keeps the connection and writes
//!   the bytes of the answer's body after the head (a WebSocket's frames).
//! * [`raw`]: a TCP server that gives each connection to the caller's
//!   function (an SSH server's first bytes, a proxy's exchange).
//! * [`udp`]: a UDP server that answers each datagram with what the
//!   caller's function gives (STUN).
//!
//! A stand-in speaks bytes alone. TLS is the caller's: the `wrap` of
//! [`Stand::http_over`] and [`Stand::raw_over`] turns each accepted
//! connection into the caller's stream (rustls's `StreamOwned`). Each
//! stand-in runs on threads; it lives until the end of the test binary.
//! Each stand-in keeps the bytes that each connection sent
//! ([`Stand::seen`]), so that a test can check what a client asked.

pub mod warp;

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// A stream that a stand-in reads and writes.
pub trait Io: Read + Write + Send {}
impl<T: Read + Write + Send> Io for T {}

/// The turn of an accepted connection into the caller's stream.
pub type Wrap = Arc<dyn Fn(TcpStream) -> io::Result<Box<dyn Io>> + Send + Sync>;

/// One request that the HTTP stand-in read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// The request target: the path and the query.
    pub target: String,
    /// The header names in lower case, the values trimmed, in order.
    pub headers: Vec<(String, String)>,
    /// The body of `Content-Length` bytes (at most [`MAX_BODY`]).
    pub body: Vec<u8>,
}

impl Request {
    /// The first value of a header, by its name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }

    /// The path of the target, with no query.
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or_default()
    }
}

/// The answer of the HTTP stand-in to one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Answer {
    /// An answer of `status` with `body` and no header of its own.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Answer {
        Answer { status, headers: Vec::new(), body: body.into() }
    }

    /// The same answer with one more header.
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Answer {
        self.headers.push((name.to_string(), value.into()));
        self
    }
}

/// What the HTTP stand-in answers to each request.
pub type Handler = Arc<dyn Fn(&Request) -> Answer + Send + Sync>;

/// A running stand-in: its address and what each connection sent.
#[derive(Clone)]
pub struct Stand {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl std::fmt::Debug for Stand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stand").field("addr", &self.addr).finish_non_exhaustive()
    }
}

/// The longest head of a request that the stand-in reads.
const MAX_HEAD: usize = 64 * 1024;
/// The longest body of a request that the stand-in reads.
pub const MAX_BODY: usize = 1024 * 1024;
/// The bound of each read: a client that stops does not hold a thread.
const READ_LIMIT: Duration = Duration::from_secs(20);

/// An HTTP stand-in on plain TCP.
pub fn http(handler: impl Fn(&Request) -> Answer + Send + Sync + 'static) -> io::Result<Stand> {
    Stand::http_over(handler, Arc::new(|tcp| Ok(Box::new(tcp) as Box<dyn Io>)))
}

/// A TCP stand-in on plain TCP that gives each connection to `serve`.
pub fn raw(serve: impl Fn(&mut dyn Io) + Send + Sync + 'static) -> io::Result<Stand> {
    Stand::raw_over(serve, Arc::new(|tcp| Ok(Box::new(tcp) as Box<dyn Io>)))
}

/// A UDP stand-in on 127.0.0.1 that answers each datagram with what
/// `answer` gives, and sends nothing for `None`.
pub fn udp(answer: impl Fn(&[u8]) -> Option<Vec<u8>> + Send + 'static) -> io::Result<Stand> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    let addr = socket.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            lock(&kept).push(buf[..n].to_vec());
            if let Some(out) = answer(&buf[..n]) {
                let _ = socket.send_to(&out, from);
            }
        }
    });
    Ok(Stand { addr, seen })
}

impl Stand {
    /// An HTTP stand-in whose connections `wrap` turns into the caller's
    /// stream first (TLS).
    pub fn http_over(handler: impl Fn(&Request) -> Answer + Send + Sync + 'static, wrap: Wrap) -> io::Result<Stand> {
        let handler: Handler = Arc::new(handler);
        Stand::raw_over(move |io| serve_http(io, &handler), wrap)
    }

    /// A TCP stand-in whose connections `wrap` turns into the caller's
    /// stream, then `serve` takes.
    pub fn raw_over(serve: impl Fn(&mut dyn Io) + Send + Sync + 'static, wrap: Wrap) -> io::Result<Stand> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        let serve = Arc::new(serve);
        thread::spawn(move || {
            for tcp in listener.incoming().flatten() {
                let (serve, wrap, kept) = (serve.clone(), wrap.clone(), kept.clone());
                thread::spawn(move || {
                    let _ = tcp.set_read_timeout(Some(READ_LIMIT));
                    let Ok(io) = wrap(tcp) else { return };
                    let mut io = Kept { io, bytes: Vec::new() };
                    serve(&mut io);
                    let _ = io.flush();
                    lock(&kept).push(io.bytes);
                });
            }
        });
        Ok(Stand { addr, seen })
    }

    /// The address of the stand-in on the loopback.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The port of the stand-in.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// The bytes that each connection (each datagram of [`udp`]) sent, for
    /// each one that ended, in the order of their ends.
    pub fn seen(&self) -> Vec<Vec<u8>> {
        lock(&self.seen).clone()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A stream that keeps each byte that it read.
struct Kept {
    io: Box<dyn Io>,
    bytes: Vec<u8>,
}

impl Read for Kept {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.io.read(buf)?;
        self.bytes.extend_from_slice(&buf[..n]);
        Ok(n)
    }
}

impl Write for Kept {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.io.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.io.flush()
    }
}

/// Each request of one connection, until the client closes, asks to close,
/// or upgrades.
fn serve_http(io: &mut dyn Io, handler: &Handler) {
    let mut buffered = Vec::new();
    loop {
        let Some((request, rest)) = read_request(io, std::mem::take(&mut buffered)) else { return };
        buffered = rest;
        let answer = handler(&request);
        let mut head = format!("HTTP/1.1 {} {}\r\n", answer.status, reason(answer.status));
        for (name, value) in &answer.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if answer.status == 101 {
            head.push_str("\r\n");
            let mut out = head.into_bytes();
            out.extend_from_slice(&answer.body);
            if io.write_all(&out).and_then(|_| io.flush()).is_err() {
                return;
            }
            // The rest of the connection is the client's: keep its bytes.
            let mut sink = [0u8; 4096];
            while matches!(io.read(&mut sink), Ok(n) if n > 0) {}
            return;
        }
        let close = request.header("connection").is_some_and(|v| v.eq_ignore_ascii_case("close"));
        head.push_str(&format!("Content-Length: {}\r\n", answer.body.len()));
        if close {
            head.push_str("Connection: close\r\n");
        }
        head.push_str("\r\n");
        let mut out = head.into_bytes();
        if request.method != "HEAD" {
            out.extend_from_slice(&answer.body);
        }
        if io.write_all(&out).and_then(|_| io.flush()).is_err() || close {
            return;
        }
    }
}

/// One head of a request and the bytes after it; none when the client
/// closed or sent no request.
fn read_request(io: &mut dyn Io, mut buffered: Vec<u8>) -> Option<(Request, Vec<u8>)> {
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(at) = buffered.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        if buffered.len() > MAX_HEAD {
            return None;
        }
        match io.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buffered.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buffered[..end]).into_owned();
    let mut rest = buffered.split_off(end + 4);
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let mut request = Request { method, target, headers, body: Vec::new() };
    let length: usize = request.header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if length > MAX_BODY {
        return None;
    }
    while rest.len() < length {
        match io.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => rest.extend_from_slice(&chunk[..n]),
        }
    }
    let after = rest.split_off(length);
    request.body = rest;
    Some((request, after))
}

fn reason(status: u16) -> &'static str {
    match status {
        101 => "Switching Protocols",
        200 => "OK",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Status",
    }
}
