// The TCP relay of the mock TURN server (RFC 6062) over plain TCP: the
// control connection with the 401, its Allocate of REQUESTED-TRANSPORT TCP,
// CreatePermission, Refresh, Connect and its CONNECTION-ID, the second
// connection with ConnectionBind, and a ConnectionAttempt indication when a
// peer opens the relayed address. The data connection carries the framed
// bytes of this crate's RelayedStream, so the mock speaks that frame too.
// It shares the codec and the knobs of `mock.rs`; it is a second server
// face, not a second STUN parser.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_turn::stun::{self, Class, HEADER, message_type};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use super::mock::{Control, Knobs, Mock, SUCCESS, Session, Shared};

/// One peer connection that waits for its ConnectionBind.
struct Pending {
    stream: TcpStream,
    key: Vec<u8>,
}

/// The relayed state of the whole mock: the connections that wait for a
/// bind, the next id, and the knobs of a wrong id and of a cut frame.
#[derive(Default)]
struct Relays {
    pending: Mutex<HashMap<u32, Pending>>,
    next: AtomicU32,
    bind_salt: u32,
    truncated: bool,
}

/// One TCP allocation: the owner, the key of its answers, the listener of
/// its relayed address, and its permissions.
struct TcpAlloc {
    owner: String,
    key: Vec<u8>,
    listener: Arc<TcpListener>,
    permissions: HashSet<IpAddr>,
    inbound: tokio::task::JoinHandle<()>,
}

type Alloc = Arc<Mutex<TcpAlloc>>;

/// The mock over plain TCP on 127.0.0.1: one control connection for each
/// allocation, and a data connection for each ConnectionBind.
pub async fn tcp_mock(knobs: Knobs) -> super::Running {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let control: Shared = Arc::new(Mutex::new(Control::default()));
    let relays = Arc::new(Relays { bind_salt: knobs.bind_salt, truncated: knobs.truncated_frame, ..Relays::default() });
    let (relayed, _rx) = mpsc::unbounded_channel();
    let shared = control.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _from)) = listener.accept().await {
            let mock = Mock { knobs: knobs.clone(), control: shared.clone(), relayed: relayed.clone() };
            let relays = relays.clone();
            tokio::spawn(async move { serve(mock, relays, tcp).await });
        }
    });
    super::Running { addr, control, roots: None, acceptor: None }
}

/// The first message says which connection this is: a ConnectionBind is a
/// data connection, anything else starts a control connection.
async fn serve(mock: Mock, relays: Arc<Relays>, mut tcp: TcpStream) {
    let Ok(first) = read_stun(&mut tcp).await else { return };
    let Ok(read) = stun::decode(&first) else { return };
    if stun::method_of(read.kind) == stun::CONNECTION_BIND && stun::class_of(read.kind) == Class::Request {
        serve_data(&mock, &relays, tcp, &first).await;
    } else {
        serve_control(mock, relays, tcp, first).await;
    }
}

async fn serve_control(mock: Mock, relays: Arc<Relays>, tcp: TcpStream, first: Vec<u8>) {
    let (mut reader, writer) = tcp.into_split();
    let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let output = tokio::spawn(async move {
        let mut writer = writer;
        let mut rx = rx;
        while let Some(bytes) = rx.recv().await {
            if writer.write_all(&bytes).await.is_err() {
                return;
            }
        }
    });
    let mut session = Session::new("127.0.0.1:0".parse().unwrap());
    let mut alloc: Option<Alloc> = None;
    let mut message = Some(first);
    loop {
        let bytes = match message.take() {
            Some(bytes) => bytes,
            None => match read_stun(&mut reader).await {
                Ok(bytes) => bytes,
                Err(_) => break,
            },
        };
        handle_control(&mock, &relays, &tx, &mut session, &mut alloc, &bytes).await;
    }
    if let Some(alloc) = alloc {
        alloc.lock().unwrap().inbound.abort();
    }
    drop(tx);
    let _ = output.await;
}

async fn handle_control(
    mock: &Mock,
    relays: &Arc<Relays>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    session: &mut Session,
    alloc: &mut Option<Alloc>,
    bytes: &[u8],
) {
    let Ok(read) = stun::decode(bytes) else { return };
    let (method, class, id) = (stun::method_of(read.kind), stun::class_of(read.kind), read.id);
    if class != Class::Request {
        return;
    }
    let user = read.attribute(stun::USERNAME).or(read.attribute(stun::USERHASH));
    let signed =
        read.attribute(stun::MESSAGE_INTEGRITY).is_some() || read.attribute(stun::MESSAGE_INTEGRITY_SHA256).is_some();
    if user.is_none() || !signed {
        mock.note(method, &read, 401);
        let _ = tx.send(mock.challenge(session, method, &id, 401));
        return;
    }
    if read.attribute(stun::NONCE) != Some(session.nonce.as_slice()) {
        mock.note(method, &read, 438);
        let _ = tx.send(mock.challenge(session, method, &id, 438));
        return;
    }
    let Some((user, key)) = mock.user_and_key(&read).filter(|(_, key)| mock.verify(bytes, &read, key).is_ok()) else {
        mock.note(method, &read, 401);
        let _ = tx.send(mock.challenge(session, method, &id, 401));
        return;
    };
    if let Some(alloc) = alloc.as_ref() {
        let owner = alloc.lock().unwrap().owner.clone();
        if user != owner {
            mock.note(method, &read, 441);
            let _ = tx.send(mock.error(method, &id, 441, &[], Some(&key)));
            return;
        }
    }
    let (code, answer) = match method {
        stun::ALLOCATE => allocate(mock, relays, tx, alloc, &read, &user, &key).await,
        stun::REFRESH => refresh(mock, alloc, &read, &key),
        stun::CREATE_PERMISSION => permission(mock, alloc, &read, &key),
        stun::CONNECT => connect(mock, relays, alloc, &read, &key).await,
        _ => (400, mock.error(method, &id, 400, &[], Some(&key))),
    };
    mock.note(method, &read, code);
    let _ = tx.send(answer);
}

async fn allocate(
    mock: &Mock,
    relays: &Arc<Relays>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    alloc: &mut Option<Alloc>,
    read: &stun::Message<'_>,
    user: &str,
    key: &[u8],
) -> (u16, Vec<u8>) {
    let id = read.id;
    let fail = |code| (code, mock.error(stun::ALLOCATE, &id, code, &[], Some(key)));
    if alloc.is_some() {
        return fail(437);
    }
    if read.attribute(stun::REQUESTED_TRANSPORT) != Some(&[stun::TRANSPORT_TCP, 0, 0, 0][..]) {
        return fail(442);
    }
    if read.attribute(stun::DONT_FRAGMENT).is_some()
        || read.attribute(stun::EVEN_PORT).is_some()
        || read.attribute(stun::RESERVATION_TOKEN).is_some()
    {
        return fail(400);
    }
    let Ok(listener) = TcpListener::bind("127.0.0.1:0").await else { return fail(508) };
    let relayed = listener.local_addr().unwrap();
    let listener = Arc::new(listener);
    let key = key.to_vec();
    let state = Arc::new(Mutex::new(TcpAlloc {
        owner: user.into(),
        key: key.clone(),
        listener: listener.clone(),
        permissions: HashSet::new(),
        inbound: tokio::spawn(async {}),
    }));
    let inbound = spawn_inbound(relays.clone(), state.clone(), tx.clone());
    state.lock().unwrap().inbound = inbound;
    *alloc = Some(state);
    let address = stun::write_address(relayed, Some(&id));
    let mapped = stun::write_address("127.0.0.1:0".parse().unwrap(), Some(&id));
    let life = mock.knobs.max_lifetime.min(600).to_be_bytes();
    let attributes = [
        (stun::XOR_RELAYED_ADDRESS, address.as_slice()),
        (stun::XOR_MAPPED_ADDRESS, mapped.as_slice()),
        (stun::LIFETIME, life.as_slice()),
    ];
    (SUCCESS, mock.answer(stun::ALLOCATE, Class::Success, &id, &attributes, Some(&key)))
}

fn refresh(mock: &Mock, alloc: &mut Option<Alloc>, read: &stun::Message<'_>, key: &[u8]) -> (u16, Vec<u8>) {
    let id = read.id;
    if alloc.is_none() {
        return (437, mock.error(stun::REFRESH, &id, 437, &[], Some(key)));
    }
    let asked = read.attribute(stun::LIFETIME).map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]));
    let life = match asked {
        Some(0) => {
            let state = alloc.take().unwrap();
            state.lock().unwrap().inbound.abort();
            0
        }
        Some(secs) => secs.min(mock.knobs.max_lifetime),
        None => mock.knobs.max_lifetime.min(600),
    };
    let value = life.to_be_bytes();
    (SUCCESS, mock.answer(stun::REFRESH, Class::Success, &id, &[(stun::LIFETIME, &value)], Some(key)))
}

fn permission(mock: &Mock, alloc: &mut Option<Alloc>, read: &stun::Message<'_>, key: &[u8]) -> (u16, Vec<u8>) {
    let id = read.id;
    let Some(state) = alloc.as_ref() else {
        return (400, mock.error(stun::CREATE_PERMISSION, &id, 400, &[], Some(key)));
    };
    let Some(value) = read.attribute(stun::XOR_PEER_ADDRESS) else {
        return (400, mock.error(stun::CREATE_PERMISSION, &id, 400, &[], Some(key)));
    };
    let Ok(peer) = stun::read_address(value, Some(&id)) else {
        return (400, mock.error(stun::CREATE_PERMISSION, &id, 400, &[], Some(key)));
    };
    state.lock().unwrap().permissions.insert(peer.ip());
    (SUCCESS, mock.answer(stun::CREATE_PERMISSION, Class::Success, &id, &[], Some(key)))
}

async fn connect(
    mock: &Mock,
    relays: &Arc<Relays>,
    alloc: &Option<Alloc>,
    read: &stun::Message<'_>,
    key: &[u8],
) -> (u16, Vec<u8>) {
    let id = read.id;
    let Some(_state) = alloc.as_ref() else {
        return (437, mock.error(stun::CONNECT, &id, 437, &[], Some(key)));
    };
    if let Some(code) = mock.knobs.refuse_connect {
        return (code, mock.error(stun::CONNECT, &id, code, &[], Some(key)));
    }
    let Some(value) = read.attribute(stun::XOR_PEER_ADDRESS) else {
        return (400, mock.error(stun::CONNECT, &id, 400, &[], Some(key)));
    };
    let Ok(peer) = stun::read_address(value, Some(&id)) else {
        return (400, mock.error(stun::CONNECT, &id, 400, &[], Some(key)));
    };
    let dial = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(peer)).await;
    let Ok(Ok(stream)) = dial else {
        return (447, mock.error(stun::CONNECT, &id, 447, &[], Some(key)));
    };
    let number = relays.next.fetch_add(1, Ordering::Relaxed) + 1;
    relays.pending.lock().unwrap().insert(number, Pending { stream, key: key.to_vec() });
    let value = number.to_be_bytes();
    (SUCCESS, mock.answer(stun::CONNECT, Class::Success, &id, &[(stun::CONNECTION_ID, &value)], Some(key)))
}

/// Accept each peer at the relayed address: a peer with no permission is
/// closed; another becomes a ConnectionAttempt indication.
fn spawn_inbound(relays: Arc<Relays>, alloc: Alloc, tx: mpsc::UnboundedSender<Vec<u8>>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let listener = alloc.lock().unwrap().listener.clone();
            let (stream, peer) = match listener.accept().await {
                Ok(got) => got,
                Err(_) => return,
            };
            let key = {
                let alloc = alloc.lock().unwrap();
                if !alloc.permissions.contains(&peer.ip()) {
                    continue;
                }
                alloc.key.clone()
            };
            let number = relays.next.fetch_add(1, Ordering::Relaxed) + 1;
            relays.pending.lock().unwrap().insert(number, Pending { stream, key });
            let txid: [u8; 12] = std::array::from_fn(|i| peer.port().to_be_bytes()[i % 2] ^ i as u8);
            let address = stun::write_address(peer, Some(&txid));
            let id = number.to_be_bytes();
            let attributes = [(stun::XOR_PEER_ADDRESS, address.as_slice()), (stun::CONNECTION_ID, id.as_slice())];
            let message = stun::message(message_type(stun::CONNECTION_ATTEMPT, Class::Indication), &txid, &attributes);
            if tx.send(message).is_err() {
                return;
            }
        }
    })
}

/// The data connection: a ConnectionBind names the pending peer, then the
/// bytes of this crate's frame go both ways.
async fn serve_data(mock: &Mock, relays: &Arc<Relays>, mut tcp: TcpStream, first: &[u8]) {
    let Ok(read) = stun::decode(first) else { return };
    let id = read.id;
    let bound = read.attribute(stun::CONNECTION_ID).and_then(|v| <[u8; 4]>::try_from(v).ok()).map(u32::from_be_bytes);
    let pending = bound.and_then(|number| relays.pending.lock().unwrap().remove(&(number ^ relays.bind_salt)));
    let Some(mut pending) = pending else {
        let answer = mock.error(stun::CONNECTION_BIND, &id, 400, &[], None);
        let _ = tcp.write_all(&answer).await;
        return;
    };
    let answer = mock.answer(stun::CONNECTION_BIND, Class::Success, &id, &[], Some(&pending.key));
    if tcp.write_all(&answer).await.is_err() {
        return;
    }
    if relays.truncated {
        let _ = tcp.write_all(&[0x00, 0x0a, b'a', b'b', b'c']).await;
        let _ = tcp.shutdown().await;
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut pending.stream).await;
}

/// One STUN message of a stream, cut by the length in its header (RFC 8489,
/// 6.2.2). The mock reads no ChannelData here: a TCP allocation has none.
async fn read_stun<S: AsyncRead + Unpin>(io: &mut S) -> io::Result<Vec<u8>> {
    let mut head = [0u8; HEADER];
    io.read_exact(&mut head).await?;
    let len = usize::from(u16::from_be_bytes([head[2], head[3]]));
    let mut out = vec![0u8; HEADER + len];
    out[..HEADER].copy_from_slice(&head);
    io.read_exact(&mut out[HEADER..]).await?;
    Ok(out)
}

/// The address of a TCP peer that echoes each read with `echo:` in front.
/// RFC 6062 carries a bare stream, so the peer frames nothing.
pub async fn plain_echo_peer() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 16 * 1024];
                loop {
                    let got = match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(got) => got,
                    };
                    if stream.write_all(&buf[..got]).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

/// The address of a TCP peer that echoes each read with `echo:` in front.
pub async fn echo_peer() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 16 * 1024];
                loop {
                    let got = match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(got) => got,
                    };
                    let out = [b"echo:".as_slice(), &buf[..got]].concat();
                    if stream.write_all(&out).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}
