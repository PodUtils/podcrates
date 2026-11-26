//! A Wisp server on the loopback, written from the protocol's document
//! (MercuryWorkshop/wisp-protocol:protocol.md, branch v2) with its own
//! reading of each byte, not the codec of this crate. It speaks version 1
//! (its CONTINUE first) or version 2 (its INFO, the client's INFO, then its
//! CONTINUE or its CLOSE). A CONNECT's host says what the stream does:
//! `hold.test` takes DATA and forwards none, so that its credit is never
//! given back; `close-XX.test` is closed at once with the reason XX
//! (hexadecimal); any other host is dialed, and its bytes carried. The
//! stand-in gives a stream its credit again only once the client spent it,
//! when no DATA of the client's can be on its way: so each DATA past the
//! credit is counted exactly, as an overrun. It takes a WebSocket with no
//! TLS.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

use pod_ws::{RelaySession, frame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

use super::{Task, locked};

type Session = RelaySession<TcpStream>;

/// How the stand-in opens a connection.
#[derive(Clone, Debug)]
pub enum Speaks {
    /// Version 1: its CONTINUE on stream 0 first, and no INFO.
    V1,
    /// Version 2, to a client that offers a protocol: its INFO with these
    /// extensions (id, metadata), then its CONTINUE on stream 0, or its CLOSE
    /// with the reason of `refuse`.
    V2 { extensions: Vec<(u8, Vec<u8>)>, refuse: Option<u8> },
}

/// What the stand-in saw, over each connection.
#[derive(Default, Debug)]
pub struct Seen {
    /// The header names of each upgrade, lower-cased.
    pub headers: Vec<Vec<String>>,
    /// The `Sec-WebSocket-Protocol` of each upgrade.
    pub protocols: Vec<Option<String>>,
    /// The payload of each INFO of the client's.
    pub client_infos: Vec<Vec<u8>>,
    /// Each CONNECT: its stream, its host, its port.
    pub connects: Vec<(u32, String, u16)>,
    /// The DATA packets of each stream.
    pub data: HashMap<u32, usize>,
    /// The DATA packets that came past their stream's credit.
    pub overruns: usize,
    /// Each CLOSE of the client's: its stream and its reason.
    pub closes: Vec<(u32, u8)>,
}

/// One connection: its WebSocket, and the credit left of each stream.
struct Conn {
    session: Arc<Session>,
    left: Mutex<HashMap<u32, u32>>,
}

/// A running stand-in.
pub struct Stand {
    pub port: u16,
    pub seen: Arc<Mutex<Seen>>,
    conns: Arc<Mutex<Vec<Arc<Conn>>>>,
    _task: Task,
}

/// A packet: its type, its stream id (little-endian), its payload.
pub fn packet(kind: u8, id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn continue_packet(id: u32, left: u32) -> Vec<u8> {
    packet(0x03, id, &left.to_le_bytes())
}

fn close_packet(id: u32, reason: u8) -> Vec<u8> {
    packet(0x04, id, &[reason])
}

/// An INFO of version 2.1 with `extensions`, each as its id, its length
/// and its metadata.
fn info_packet(extensions: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut payload = vec![2, 1];
    for (id, metadata) in extensions {
        payload.push(*id);
        payload.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        payload.extend_from_slice(metadata);
    }
    packet(0x05, 0, &payload)
}

/// Whether the payload of an INFO lists extension `id`.
fn lists(info: &[u8], id: u8) -> bool {
    let mut rest = info.get(2..).unwrap_or_default();
    while rest.len() >= 5 {
        let len = u32::from_le_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize;
        if rest[0] == id {
            return true;
        }
        rest = rest.get(5 + len..).unwrap_or_default();
    }
    false
}

impl Stand {
    /// A stand-in that speaks `speaks`, with `buffer` packets of credit for
    /// each stream.
    pub async fn start(speaks: Speaks, buffer: u32) -> io::Result<Stand> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let seen: Arc<Mutex<Seen>> = Arc::default();
        let conns: Arc<Mutex<Vec<Arc<Conn>>>> = Arc::default();
        let (kept, held) = (seen.clone(), conns.clone());
        let task = tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                tokio::spawn(serve(tcp, speaks.clone(), buffer, kept.clone(), held.clone()));
            }
        });
        Ok(Stand { port, seen, conns, _task: Task::of(task) })
    }

    /// The DATA packets that stream `id` sent.
    pub fn data_of(&self, id: u32) -> usize {
        locked(&self.seen).data.get(&id).copied().unwrap_or(0)
    }

    pub fn overruns(&self) -> usize {
        locked(&self.seen).overruns
    }

    /// A CONTINUE of `left` for stream `id` of the last connection; false
    /// when there is no connection, or its WebSocket ended.
    pub async fn grant(&self, id: u32, left: u32) -> bool {
        let Some(conn) = locked(&self.conns).last().cloned() else { return false };
        locked(&conn.left).insert(id, left);
        conn.session.send_binary(&continue_packet(id, left)).await.is_ok()
    }
}

async fn serve(
    mut tcp: TcpStream,
    speaks: Speaks,
    buffer: u32,
    seen: Arc<Mutex<Seen>>,
    conns: Arc<Mutex<Vec<Arc<Conn>>>>,
) {
    let Ok((head, pending)) = pod_proxy::http::read_request(&mut tcp, Vec::new()).await else { return };
    let protocol = head.headers.iter().find(|(n, _)| n == "sec-websocket-protocol").map(|(_, v)| v.clone());
    {
        let mut seen = locked(&seen);
        seen.headers.push(head.headers.iter().map(|(n, _)| n.clone()).collect());
        seen.protocols.push(protocol.clone());
    }
    let Ok(key) = pod_ws::server::check_upgrade(&head) else {
        let _ = tcp.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n").await;
        return;
    };
    if tcp.write_all(&pod_ws::server::switching(&key)).await.is_err() {
        return;
    }
    let session = Arc::new(pod_ws::server::session(tcp, pending, None));
    let mut confirm = false;
    // Version 2 only where the upgrade offered a protocol.
    if let (Speaks::V2 { extensions, refuse }, true) = (&speaks, protocol.is_some()) {
        if session.send_binary(&info_packet(extensions)).await.is_err() {
            return;
        }
        let Ok(f) = session.read_frame().await else { return };
        let theirs = match f.payload.first() {
            Some(0x05) => f.payload[5..].to_vec(),
            Some(0x04) => {
                let reason = f.payload.get(5).copied().unwrap_or(0);
                locked(&seen).closes.push((0, reason));
                return;
            }
            _ => return,
        };
        locked(&seen).client_infos.push(theirs.clone());
        confirm = extensions.iter().any(|(id, _)| *id == 0x05) && lists(&theirs, 0x05);
        if let Some(reason) = refuse {
            let _ = session.send_binary(&close_packet(0, *reason)).await;
            let _ = session.send_close(1000, "").await;
            return;
        }
    }
    if session.send_binary(&continue_packet(0, buffer)).await.is_err() {
        return;
    }
    let conn = Arc::new(Conn { session: session.clone(), left: Mutex::default() });
    locked(&conns).push(conn.clone());
    let mut targets: HashMap<u32, OwnedWriteHalf> = HashMap::new();
    let mut held: HashSet<u32> = HashSet::new();
    while let Ok(f) = session.read_frame().await {
        if f.opcode == frame::OPCODE_CLOSE {
            break;
        }
        if f.opcode != frame::OPCODE_BINARY || f.payload.len() < 5 {
            continue;
        }
        let (kind, body) = (f.payload[0], &f.payload[5..]);
        let id = u32::from_le_bytes([f.payload[1], f.payload[2], f.payload[3], f.payload[4]]);
        match kind {
            0x01 if body.len() >= 3 => {
                let port = u16::from_le_bytes([body[1], body[2]]);
                let host = String::from_utf8_lossy(&body[3..]).into_owned();
                locked(&seen).connects.push((id, host.clone(), port));
                locked(&conn.left).insert(id, buffer);
                let closing = host.strip_prefix("close-").and_then(|h| h.strip_suffix(".test"));
                if host == "hold.test" {
                    held.insert(id);
                } else if let Some(reason) = closing.and_then(|r| u8::from_str_radix(r, 16).ok()) {
                    let _ = session.send_binary(&close_packet(id, reason)).await;
                } else if let Ok(target) = TcpStream::connect((host.as_str(), port)).await {
                    let (rd, wr) = target.into_split();
                    targets.insert(id, wr);
                    tokio::spawn(back(session.clone(), id, rd));
                    if confirm {
                        // Stream open confirmation: the credit as it stands.
                        let left = locked(&conn.left).get(&id).copied().unwrap_or(buffer);
                        let _ = session.send_binary(&continue_packet(id, left)).await;
                    }
                } else {
                    let _ = session.send_binary(&close_packet(id, 0x44)).await;
                }
            }
            0x02 => {
                let over = {
                    let mut left = locked(&conn.left);
                    match left.get_mut(&id) {
                        Some(0) => true,
                        Some(n) => {
                            *n -= 1;
                            false
                        }
                        None => false,
                    }
                };
                {
                    let mut seen = locked(&seen);
                    *seen.data.entry(id).or_default() += 1;
                    seen.overruns += usize::from(over);
                }
                if held.contains(&id) {
                    continue;
                }
                if let Some(wr) = targets.get_mut(&id) {
                    let _ = wr.write_all(body).await;
                    // The credit again once spent: none of the client's is
                    // then on its way.
                    let spent = locked(&conn.left).get(&id) == Some(&0);
                    if spent {
                        locked(&conn.left).insert(id, buffer);
                        let _ = session.send_binary(&continue_packet(id, buffer)).await;
                    }
                }
            }
            0x04 => {
                locked(&seen).closes.push((id, body.first().copied().unwrap_or(0)));
                // Dropped, the write half ends the target's input.
                targets.remove(&id);
            }
            _ => {}
        }
    }
}

/// The target's bytes to the client in DATA of 32 KiB at most, then a
/// CLOSE 0x02 at its end.
async fn back(session: Arc<Session>, id: u32, mut rd: OwnedReadHalf) {
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        match rd.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if session.send_binary(&packet(0x02, id, &buf[..n])).await.is_err() {
                    return;
                }
            }
        }
    }
    let _ = session.send_binary(&close_packet(id, 0x02)).await;
}
