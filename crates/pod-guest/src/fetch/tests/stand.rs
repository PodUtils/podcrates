//! The store against an HTTP stand-in on the loopback: a break of the body,
//! a resume by Range, a `200` to a Range, and the sizes that do not agree.

use super::*;
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};

use std::time::Duration;

use pod_testkit::stand::{self, Answer, Io};

/// The object of the stand-in: 1,000 bytes that are not all the same.
fn object() -> Vec<u8> {
    (0..1000u32).map(|i| (i % 251) as u8).collect()
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A small HTTP/1.1 client: one GET for each call, `Connection: close`, and
/// a body that is the rest of the connection, with no framing. The store,
/// not the client, checks the size.
struct Client {
    addr: SocketAddr,
    /// The offset of each request.
    asked: Vec<u64>,
}

impl Client {
    fn new(addr: SocketAddr) -> Client {
        Client { addr, asked: Vec::new() }
    }
}

impl Origin for Client {
    type Body = BufReader<TcpStream>;
    fn get(&mut self, from: u64) -> std::io::Result<Reply<Self::Body>> {
        self.asked.push(from);
        let mut tcp = TcpStream::connect(self.addr)?;
        tcp.set_read_timeout(Some(Duration::from_secs(20)))?;
        let range = if from > 0 { format!("Range: bytes={from}-\r\n") } else { String::new() };
        write!(tcp, "GET /image HTTP/1.1\r\nHost: stand\r\nConnection: close\r\n{range}\r\n")?;
        let mut body = BufReader::new(tcp);
        let mut line = String::new();
        body.read_line(&mut line)?;
        let status = line.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let (mut length, mut range) = (None, None);
        loop {
            line.clear();
            body.read_line(&mut line)?;
            let Some((name, value)) = line.trim_end().split_once(':') else { break };
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().ok(),
                "content-range" => range = Some(value.trim().to_string()),
                _ => {}
            }
        }
        Ok(Reply { status, length, range, body })
    }
}

/// The offset of a `Range: bytes=N-` in the head of a request.
fn range_of(request: &stand::Request) -> Option<u64> {
    request.header("range")?.strip_prefix("bytes=")?.strip_suffix('-')?.parse().ok()
}

/// A stand-in that serves `object()` and honours a Range.
fn ranged(request: &stand::Request) -> Answer {
    let o = object();
    match range_of(request) {
        Some(from) => Answer::new(206, o[from as usize..].to_vec())
            .header("Content-Range", format!("bytes {from}-{}/{}", o.len() - 1, o.len())),
        None => Answer::new(200, o),
    }
}

/// Read the head of a request on a raw connection.
fn read_head(io: &mut dyn Io) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && matches!(io.read(&mut byte), Ok(1)) {
        head.push(byte[0]);
    }
    String::from_utf8_lossy(&head).into_owned()
}

/// The body breaks after 400 bytes on the first connection; each later
/// connection gets its Range.
#[test]
fn a_body_that_breaks_once_resumes_by_range_and_lands_whole() {
    pod_testkit::need_loopback!();
    let first = Arc::new(Mutex::new(true));
    let s = stand::raw(move |io| {
        let head = read_head(io);
        let o = object();
        let mut first = first.lock().unwrap_or_else(|e| e.into_inner());
        let out = if std::mem::replace(&mut *first, false) {
            let mut out = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", o.len()).into_bytes();
            out.extend_from_slice(&o[..400]);
            out
        } else {
            let from: usize = head.split("bytes=").nth(1).and_then(|r| r.split('-').next()?.parse().ok()).unwrap();
            let mut out = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {from}-{}/{}\r\n\r\n",
                o.len() - from,
                o.len() - 1,
                o.len()
            )
            .into_bytes();
            out.extend_from_slice(&o[from..]);
            out
        };
        let _ = io.write_all(&out);
    })
    .unwrap();
    let d = dir("break");
    let dest = d.join("win.vhdx");
    let mut c = Client::new(s.addr());
    let got = store(&mut c, &dest, Some(&sha(&object())), ceiling(1 << 20, u64::MAX), 3).unwrap();
    assert_eq!(got, sha(&object()));
    assert_eq!(c.asked, [0, 400], "one break, then a Range from the byte after the last one");
    assert_eq!(std::fs::read(&dest).unwrap(), object());
    assert!(!partial(&dest).exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// A break on the last try keeps the partial file; the next call sends a
/// Range from its end, and a pinned digest covers the bytes of both calls.
#[test]
fn a_later_call_resumes_the_partial_file_of_an_earlier_one() {
    pod_testkit::need_loopback!();
    let d = dir("later");
    let dest = d.join("win.iso");
    std::fs::write(partial(&dest), &object()[..300]).unwrap();
    let s = stand::http(ranged).unwrap();
    let mut c = Client::new(s.addr());
    let want = sha(&object());
    assert_eq!(store(&mut c, &dest, Some(&want), ceiling(1 << 20, u64::MAX), 1).unwrap(), want);
    assert_eq!(c.asked, [300]);
    assert_eq!(std::fs::read(&dest).unwrap(), object());
    // With no pin, a partial file of an earlier call is not resumed: the
    // object can have changed between the calls.
    let dest2 = d.join("unpinned.iso");
    std::fs::write(partial(&dest2), b"junk of another version").unwrap();
    let mut c = Client::new(s.addr());
    assert_eq!(store(&mut c, &dest2, None, ceiling(1 << 20, u64::MAX), 1).unwrap(), want);
    assert_eq!(c.asked, [0]);
    let _ = std::fs::remove_dir_all(&d);
}

/// An origin that ignores the Range gives the whole object with `200`: the
/// file starts again from zero, and the old bytes are not in the digest.
#[test]
fn a_200_to_a_range_starts_the_file_again_from_zero() {
    pod_testkit::need_loopback!();
    let d = dir("restart");
    let dest = d.join("win.iso");
    std::fs::write(partial(&dest), vec![0xEEu8; 300]).unwrap();
    let s = stand::http(|_| Answer::new(200, object())).unwrap();
    let mut c = Client::new(s.addr());
    let want = sha(&object());
    assert_eq!(store(&mut c, &dest, Some(&want), ceiling(1 << 20, u64::MAX), 1).unwrap(), want);
    assert_eq!(c.asked, [300]);
    assert_eq!(std::fs::read(&dest).unwrap(), object());
    let _ = std::fs::remove_dir_all(&d);
}

/// A body past its `Content-Length`, a `206` from another offset, a
/// `Content-Range` that disagrees with its `Content-Length`, and a second
/// answer with another size of the object: each is a refusal, and the
/// partial file goes.
#[test]
fn a_wrong_size_or_range_is_refused_and_the_partial_removed() {
    pod_testkit::need_loopback!();
    let d = dir("size");
    let c = ceiling(1 << 20, u64::MAX);
    // 1,200 bytes under a Content-Length of 1,000.
    // One answer: a later connection closes with no answer, so that a
    // store that takes the long body ends at once.
    let once = Mutex::new(true);
    let long = stand::raw(move |io| {
        read_head(io);
        if !std::mem::replace(&mut *once.lock().unwrap_or_else(|e| e.into_inner()), false) {
            return;
        }
        let mut out = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n".to_vec();
        out.extend_from_slice(&[5u8; 1200]);
        let _ = io.write_all(&out);
    })
    .unwrap();
    let dest = d.join("long.img");
    let e = store(&mut Client::new(long.addr()), &dest, None, c, 3).unwrap_err();
    assert_eq!(refusal(&e), Some(&Refusal::Size { declared: 1000, got: 1200 }), "{e}");
    assert_eq!(e.class, Class::Protocol);
    assert!(!partial(&dest).exists() && !dest.exists());

    let want = sha(&object());
    let answers: [fn(u64, u32) -> Answer; 3] = [
        // A 206 that starts at another offset.
        |_, _| Answer::new(206, vec![1u8; 10]).header("Content-Range", "bytes 0-9/1000"),
        // A Content-Range of 700 bytes over a Content-Length of 10.
        |from, _| Answer::new(206, vec![1u8; 10]).header("Content-Range", format!("bytes {from}-{}/1000", from + 699)),
        // A range of 10 bytes of an object of 1,000; then the object is
        // 2,000 bytes.
        |from, n| {
            let total = if n == 0 { 1000 } else { 2000 };
            Answer::new(206, object()[from as usize..from as usize + 10].to_vec())
                .header("Content-Range", format!("bytes {from}-{}/{total}", from + 9))
        },
    ];
    for (n, answer) in answers.into_iter().enumerate() {
        let count = Mutex::new(0u32);
        let s = stand::http(move |r| {
            let mut count = count.lock().unwrap_or_else(|e| e.into_inner());
            *count += 1;
            answer(range_of(r).unwrap_or(0), *count - 1)
        })
        .unwrap();
        let dest = d.join(format!("range{n}.img"));
        std::fs::write(partial(&dest), &object()[..300]).unwrap();
        let mut origin = Client::new(s.addr());
        let e = store(&mut origin, &dest, Some(&want), c, 3).unwrap_err();
        let r = refusal(&e).unwrap_or_else(|| panic!("{e}"));
        match n {
            2 => {
                assert_eq!(r, &Refusal::Size { declared: 1000, got: 2000 });
                assert_eq!(origin.asked, [300, 310], "a shorter range is no break");
            }
            _ => assert!(matches!(r, Refusal::Range { from: 300, .. }), "{n}: {e}"),
        }
        assert_eq!(e.class, Class::Protocol);
        assert!(!partial(&dest).exists(), "{n}: the partial file goes");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// The ceiling of the entry holds for an origin that declares the size in
/// its `Content-Range` alone.
#[test]
fn a_range_over_the_ceiling_of_the_entry_is_refused() {
    pod_testkit::need_loopback!();
    let d = dir("rangeceiling");
    let dest = d.join("big.iso");
    std::fs::write(partial(&dest), &object()[..300]).unwrap();
    let s = stand::http(|r| {
        let from = range_of(r).unwrap_or(0);
        Answer::new(206, vec![0u8; 8]).header("Content-Range", format!("bytes {from}-{}/9000000", from + 7))
    })
    .unwrap();
    let e = store(&mut Client::new(s.addr()), &dest, Some(&sha(&object())), ceiling(1 << 20, u64::MAX), 1).unwrap_err();
    assert!(matches!(refusal(&e), Some(Refusal::DeclaredTooLarge { declared: 9000000, .. })), "{e}");
    assert!(!partial(&dest).exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// A partial file that holds the whole object (a call that ended before the
/// rename): the origin answers `416` with the size, and the file lands
/// with no more bytes.
#[test]
fn a_whole_partial_file_lands_on_a_416_of_its_size() {
    pod_testkit::need_loopback!();
    let d = dir("whole");
    let dest = d.join("win.iso");
    std::fs::write(partial(&dest), object()).unwrap();
    let s = stand::http(|_| Answer::new(416, Vec::new()).header("Content-Range", "bytes */1000")).unwrap();
    let mut c = Client::new(s.addr());
    let want = sha(&object());
    assert_eq!(store(&mut c, &dest, Some(&want), ceiling(1 << 20, u64::MAX), 1).unwrap(), want);
    assert_eq!(c.asked, [1000]);
    let _ = std::fs::remove_dir_all(&d);
}

/// A `404` is no break: it ends the store at once with its class. A `503`
/// is a break, and the tries end in [`Refusal::Broken`].
#[test]
fn a_status_ends_the_store_and_a_busy_origin_takes_the_tries() {
    pod_testkit::need_loopback!();
    let d = dir("status");
    let gone = stand::http(|_| Answer::new(404, Vec::new())).unwrap();
    let e = store(&mut Client::new(gone.addr()), &d.join("a"), None, ceiling(1024, 1024), 5).unwrap_err();
    assert_eq!(refusal(&e), Some(&Refusal::Status { status: 404 }));
    assert_eq!(e.class, Class::NotFound);
    let busy = stand::http(|_| Answer::new(503, Vec::new())).unwrap();
    let mut c = Client::new(busy.addr());
    let e = store(&mut c, &d.join("b"), None, ceiling(1024, 1024), 3).unwrap_err();
    assert!(matches!(refusal(&e), Some(Refusal::Broken { have: 0, tries: 3, .. })), "{e}");
    assert_eq!(c.asked.len(), 3);
    assert!(!partial(&d.join("b")).exists(), "a partial file with no byte is no resume");
    let _ = std::fs::remove_dir_all(&d);
}
