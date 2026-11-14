//! `transit-relay` against a stand-in transit relay in Rust (each end's
//! line, `ok` to both when the second comes, the bytes of the first to the
//! second), against stand-ins that answer `impatient`, that pair and carry
//! nothing, and that carry other bytes, and against magic-wormhole's own
//! transit relay (magic-wormhole-transit-relay, in Python) where this host
//! has it. `POD_WORMHOLE_TEST_PYTHON` names the interpreter;
//! `POD_WORMHOLE_TEST_SERVER=required` turns a skip into a failure.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::common::{endpoint, file, one, raw, text};

const WATCH: &str = "[[watch]]\nkind = \"transit-relay\"\nendpoint = \"relay\"\n";

/// What a stand-in relay does once two ends of a token came.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// It carries the bytes of each first end to its second.
    Carries,
    /// It says `ok` and carries nothing.
    Swallows,
    /// It carries other bytes.
    Changes,
}

/// The first end of each token, by token, until its second end comes.
type Waiting = Arc<Mutex<HashMap<String, TcpStream>>>;

fn line(io: &mut dyn Read) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    while !bytes.ends_with(b"\n") && bytes.len() < 256 && io.read(&mut byte).is_ok_and(|n| n == 1) {
        bytes.push(byte[0]);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The bytes of `from` to `to`, as `mode` carries them, until `from` ends.
fn carry(mut from: TcpStream, mut to: TcpStream, mode: Mode) {
    let mut chunk = [0u8; 4096];
    while let Ok(n @ 1..) = from.read(&mut chunk) {
        let out = match mode {
            Mode::Carries => chunk[..n].to_vec(),
            Mode::Swallows => continue,
            Mode::Changes => vec![b'x'; n],
        };
        if to.write_all(&out).is_err() {
            break;
        }
    }
    let _ = to.shutdown(Shutdown::Write);
}

/// A relay on the loopback that pairs the two ends of a token, as
/// magic-wormhole's does: `ok` to both when the second comes, then the
/// bytes of each to the other, as `mode` carries them. Its port.
fn relay(mode: Mode) -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port of the loopback");
    let port = listener.local_addr().expect("its address").port();
    let waiting: Waiting = Arc::default();
    std::thread::spawn(move || {
        for mut tcp in listener.incoming().flatten() {
            let waiting = waiting.clone();
            std::thread::spawn(move || {
                let said = line(&mut tcp);
                let Some(token) = said.strip_prefix("please relay ").and_then(|rest| rest.split(' ').next()) else {
                    let _ = tcp.write_all(b"bad handshake\n");
                    return;
                };
                let mut waiting = waiting.lock().unwrap();
                let Some(mut first) = waiting.remove(token) else {
                    waiting.insert(token.to_string(), tcp);
                    return;
                };
                drop(waiting);
                if first.write_all(b"ok\n").is_err() || tcp.write_all(b"ok\n").is_err() {
                    return;
                }
                let (Ok(first_out), Ok(second_out)) = (first.try_clone(), tcp.try_clone()) else { return };
                std::thread::spawn(move || carry(first, second_out, mode));
                carry(tcp, first_out, mode);
            });
        }
    });
    port
}

fn service(id: &str, port: u16) -> String {
    file(id, &(endpoint("relay", "tcp", "127.0.0.1", port, "") + WATCH))
}

#[tokio::test]
async fn transit_relay_pairs_two_ends_and_carries_the_bytes_of_one_to_the_other() {
    pod_testkit::need_loopback!();
    let good = relay(Mode::Carries);
    let report = one(&service("t1", good)).await;
    assert!(report.holds(), "{}", text(&report));
    assert!(text(&report).contains("paired two ends of one token and carried"), "{}", text(&report));
    for (id, mode, says) in [("t2", Mode::Swallows, "carried no bytes"), ("t3", Mode::Changes, "other bytes")] {
        let bad = relay(mode);
        let report = one(&service(id, bad)).await;
        assert!(!report.holds() && text(&report).contains(says), "{id}: {}", text(&report));
    }
    let impatient = raw(|io| {
        let mut byte = [0u8; 1];
        while io.read(&mut byte).is_ok_and(|n| n == 1 && byte[0] != b'\n') {}
        let _ = io.write_all(b"impatient\n");
    });
    let report = one(&service("t4", impatient.port())).await;
    assert!(!report.holds() && text(&report).contains("impatient"), "{}", text(&report));
    let wss = file("t5", &(endpoint("relay", "wss", "127.0.0.1", good, "") + WATCH));
    let report = one(&wss).await;
    assert!(!report.holds() && text(&report).contains("not tcp"), "{}", text(&report));
}

/// magic-wormhole's transit relay on the loopback: the process, killed on
/// drop.
struct Python(Child);

impl Drop for Python {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn python_relay() -> Option<(Python, u16)> {
    let required = std::env::var("POD_WORMHOLE_TEST_SERVER").is_ok_and(|v| v == "required");
    let python = std::env::var("POD_WORMHOLE_TEST_PYTHON").unwrap_or_else(|_| "python3".into());
    let probe = Command::new(&python).args(["-c", "import wormhole_transit_relay, twisted"]).output();
    if !probe.is_ok_and(|out| out.status.success()) {
        let why = "no Python with magic-wormhole-transit-relay here (pip install magic-wormhole-transit-relay)";
        assert!(!required, "POD_WORMHOLE_TEST_SERVER=required: {why}");
        eprintln!("did not run: {why}");
        return None;
    }
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).and_then(|l| l.local_addr()).expect("a port").port();
    let child = Command::new(&python)
        .args(["-m", "twisted", "transitrelay"])
        .arg(format!("--port=tcp:{port}:interface=127.0.0.1"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the transit relay starts");
    let relay = Python(child);
    let at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let deadline = Instant::now() + Duration::from_secs(60);
    while TcpStream::connect_timeout(&at, Duration::from_millis(200)).is_err() {
        assert!(Instant::now() < deadline, "the transit relay did not listen in time");
        std::thread::sleep(Duration::from_millis(100));
    }
    Some((relay, port))
}

#[tokio::test]
async fn transit_relay_holds_against_magic_wormholes_own_relay() {
    pod_testkit::need_loopback!();
    let Some((_relay, port)) = python_relay() else { return };
    let report = one(&service("t6", port)).await;
    assert!(report.holds(), "{}", text(&report));
}
