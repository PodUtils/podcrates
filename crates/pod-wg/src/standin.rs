//! A stand-in of the WireGuard front door of WARP, for the tests of this
//! crate and of a consumer (feature `standin`). It answers each handshake.
//! It takes the data messages as [`DataPath`] says: a TCP SYN gets a
//! SYN-ACK, and each other packet comes back.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Instant;

use pod_core::entropy::OsEntropy;

use crate::blake2s::{hash, kdf};
use crate::cookie::{cookie_key, mac2};
use crate::crypto::{open, random, seal, x25519, x25519_base, xseal};
use crate::handshake::{
    CONSTRUCTION, IDENTIFIER, INITIATION_LEN, Keys, MSG_COOKIE, MSG_DATA, MSG_INITIATION, MSG_RESPONSE, mac1,
};
use crate::session::Session;

/// What the stand-in does with the data messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataPath {
    /// It takes each message.
    All,
    /// It drops a message with zero reserved bytes (bytes 1 to 3).
    NeedsReserved,
    /// It drops each message: the handshakes stay fresh, and no data goes.
    Dead,
}

/// The responder of the handshake: it checks the initiation and answers as
/// a WireGuard peer does. It gives the response, the send key, the receive
/// key and the index of the responder.
///
/// # Panics
///
/// When the initiation fails a check: the stand-in serves tests only.
pub fn respond(
    peer_private: &[u8; 32],
    initiator_public: &[u8; 32],
    init: &[u8],
) -> (Vec<u8>, [u8; 32], [u8; 32], u32) {
    let e_priv = random::<32>(&mut OsEntropy).expect("random bytes");
    respond_with(peer_private, initiator_public, init, &e_priv, 77)
}

/// [`respond`] with the ephemeral key `e_priv` and the index `sender` of the
/// responder: the same values give the same response.
pub fn respond_with(
    peer_private: &[u8; 32],
    initiator_public: &[u8; 32],
    init: &[u8],
    e_priv: &[u8; 32],
    sender: u32,
) -> (Vec<u8>, [u8; 32], [u8; 32], u32) {
    respond_preshared(peer_private, initiator_public, None, init, e_priv, sender)
}

/// [`respond_with`] with the preshared key `preshared` (`None`: the zero
/// key), as [`crate::Keys::preshared`] of the initiator.
///
/// # Panics
///
/// When the initiation fails a check, as [`respond`].
pub fn respond_preshared(
    peer_private: &[u8; 32],
    initiator_public: &[u8; 32],
    preshared: Option<&[u8; 32]>,
    init: &[u8],
    e_priv: &[u8; 32],
    sender: u32,
) -> (Vec<u8>, [u8; 32], [u8; 32], u32) {
    let peer_public = x25519_base(peer_private);
    assert_eq!(init.len(), INITIATION_LEN);
    assert_eq!(mac1(&peer_public, &init[..116]), init[116..132]);
    let mut c = hash(&[CONSTRUCTION]);
    let mut h = hash(&[&c, IDENTIFIER]);
    h = hash(&[&h, &peer_public]);
    let e_pub: [u8; 32] = init[8..40].try_into().unwrap();
    [c] = kdf::<1>(&c, &e_pub);
    h = hash(&[&h, &e_pub]);
    let [c2, k] = kdf::<2>(&c, &x25519(peer_private, &e_pub));
    c = c2;
    let s_pub = open(&k, 0, &init[40..88], &h).unwrap();
    assert_eq!(s_pub, initiator_public);
    h = hash(&[&h, &init[40..88]]);
    let [c3, k] = kdf::<2>(&c, &x25519(peer_private, initiator_public));
    c = c3;
    let time = open(&k, 0, &init[88..116], &h).unwrap();
    assert_eq!(time.len(), 12);
    h = hash(&[&h, &init[88..116]]);
    let e_r = x25519_base(e_priv);
    [c] = kdf::<1>(&c, &e_r);
    h = hash(&[&h, &e_r]);
    [c] = kdf::<1>(&c, &x25519(e_priv, &e_pub));
    [c] = kdf::<1>(&c, &x25519(e_priv, initiator_public));
    let [c4, tau, k] = kdf::<3>(&c, preshared.unwrap_or(&[0u8; 32]));
    c = c4;
    h = hash(&[&h, &tau]);
    let empty = seal(&k, 0, &[], &h);
    let mut m = vec![MSG_RESPONSE, 0, 0, 0];
    m.extend_from_slice(&sender.to_le_bytes());
    m.extend_from_slice(&init[4..8]);
    m.extend_from_slice(&e_r);
    m.extend_from_slice(&empty);
    let mac = mac1(initiator_public, &m);
    m.extend_from_slice(&mac);
    m.extend_from_slice(&[0; 16]);
    let [recv_key, send_key] = kdf::<2>(&c, &[]);
    (m, send_key, recv_key, sender)
}

/// The cookie reply of a responder under load to the initiation `init`:
/// `cookie` sealed with the key of `responder_public`, under `nonce`, with
/// the mac1 of `init`.
pub fn cookie_reply(responder_public: &[u8; 32], init: &[u8], cookie: &[u8; 16], nonce: &[u8; 24]) -> Vec<u8> {
    let mut m = vec![MSG_COOKIE, 0, 0, 0];
    m.extend_from_slice(&init[4..8]);
    m.extend_from_slice(nonce);
    m.extend_from_slice(&xseal(&cookie_key(responder_public), nonce, cookie, &init[116..132]));
    m
}

/// A session with `key` in both directions, for a test of [`Session::seal`].
pub fn session(key: [u8; 32]) -> Session {
    Session::new(key, key, 1, 2, Instant::now())
}

/// The keys of a client and of the stand-in: (client keys, private key of
/// the stand-in).
pub fn keys(reserved: [u8; 3]) -> (Keys, [u8; 32]) {
    let peer_private = random::<32>(&mut OsEntropy).expect("random bytes");
    let keys = Keys {
        private: random::<32>(&mut OsEntropy).expect("random bytes"),
        peer_public: x25519_base(&peer_private),
        reserved,
        preshared: None,
    };
    (keys, peer_private)
}

/// Start the stand-in on the loopback for a new client. It gives the keys
/// of the client and the address of the stand-in (the device address of the
/// client is 172.16.0.2).
pub async fn start(reserved: [u8; 3], path: DataPath) -> (Keys, SocketAddr) {
    let (keys, peer_private) = self::keys(reserved);
    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let endpoint = sock.local_addr().unwrap();
    tokio::spawn(serve(sock, peer_private, keys.public(), path));
    (keys, endpoint)
}

/// Start the stand-in on the loopback with a new preshared key: the keys of
/// the client hold it in [`Keys::preshared`].
pub async fn start_preshared(reserved: [u8; 3], path: DataPath) -> (Keys, SocketAddr) {
    let (mut keys, peer_private) = self::keys(reserved);
    let preshared = random::<32>(&mut OsEntropy).expect("random bytes");
    keys.preshared = Some(preshared);
    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let endpoint = sock.local_addr().unwrap();
    tokio::spawn(serve_with(sock, peer_private, keys.public(), path, None, Some(preshared)));
    (keys, endpoint)
}

/// Start the stand-in on the loopback under load: it answers an initiation
/// with no mac2 of its cookie with a cookie reply, and an initiation with
/// it as [`start`] does.
pub async fn start_under_load(reserved: [u8; 3], path: DataPath) -> (Keys, SocketAddr) {
    let (keys, peer_private) = self::keys(reserved);
    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let endpoint = sock.local_addr().unwrap();
    let cookie = random::<16>(&mut OsEntropy).expect("random bytes");
    tokio::spawn(serve_with(sock, peer_private, keys.public(), path, Some(cookie), None));
    (keys, endpoint)
}

/// Serve the client of `client_public` on `sock`.
pub async fn serve(sock: tokio::net::UdpSocket, peer_private: [u8; 32], client_public: [u8; 32], path: DataPath) {
    serve_with(sock, peer_private, client_public, path, None, None).await
}

/// Serve as [`serve`] does; with `cookie`, under load; with `preshared`,
/// the preshared key of the client.
async fn serve_with(
    sock: tokio::net::UdpSocket,
    peer_private: [u8; 32],
    client_public: [u8; 32],
    path: DataPath,
    cookie: Option<[u8; 16]>,
    preshared: Option<[u8; 32]>,
) {
    // For each client address: the send key, the receive key, the index
    // of the client and the send counter.
    let mut sessions = HashMap::new();
    let mut buf = vec![0u8; 2048];
    loop {
        let Ok((n, from)) = sock.recv_from(&mut buf).await else {
            return;
        };
        let m = &buf[..n];
        // An empty datagram has no type; the stand-in waits for the next.
        let Some(&kind) = m.first() else { continue };
        if kind == MSG_INITIATION && n == INITIATION_LEN {
            if let Some(cookie) = &cookie
                && m[132..] != mac2(cookie, &m[..132])
            {
                let nonce = random::<24>(&mut OsEntropy).expect("random bytes");
                let reply = cookie_reply(&x25519_base(&peer_private), m, cookie, &nonce);
                let _ = sock.send_to(&reply, from).await;
                continue;
            }
            let e_priv = random::<32>(&mut OsEntropy).expect("random bytes");
            let (response, send, recv, _) =
                respond_preshared(&peer_private, &client_public, preshared.as_ref(), m, &e_priv, 77);
            let receiver: [u8; 4] = m[4..8].try_into().unwrap();
            sessions.insert(from, (send, recv, receiver, 0u64));
            let _ = sock.send_to(&response, from).await;
            continue;
        }
        let dropped = match path {
            DataPath::All => false,
            DataPath::NeedsReserved => m.get(1..4) == Some(&[0, 0, 0][..]),
            DataPath::Dead => true,
        };
        if kind != MSG_DATA || n < 32 || dropped {
            continue;
        }
        let Some((send, recv, receiver, counter)) = sessions.get_mut(&from) else {
            continue;
        };
        let c = u64::from_le_bytes(m[8..16].try_into().unwrap());
        let Some(plain) = open(recv, c, &m[16..], &[]) else {
            continue;
        };
        if plain.is_empty() {
            continue;
        }
        let back = syn_ack(&plain).unwrap_or(plain);
        let mut out = vec![MSG_DATA, 0, 0, 0];
        out.extend_from_slice(&receiver[..]);
        out.extend_from_slice(&counter.to_le_bytes());
        out.extend_from_slice(&seal(send, *counter, &back, &[]));
        *counter += 1;
        let _ = sock.send_to(&out, from).await;
    }
}

/// The SYN-ACK for an IPv4 TCP SYN (the far end of a TCP probe).
fn syn_ack(p: &[u8]) -> Option<Vec<u8>> {
    if p.len() < 40 || p[0] != 0x45 || p[9] != 6 || p[33] != 0x02 {
        return None;
    }
    let ip = |at: usize| Ipv4Addr::new(p[at], p[at + 1], p[at + 2], p[at + 3]);
    let port = |at: usize| u16::from_be_bytes([p[at], p[at + 1]]);
    let seq = u32::from_be_bytes(p[24..28].try_into().unwrap());
    Some(tcp_packet(ip(16), ip(12), port(22), port(20), 0x0bad_cafe, seq.wrapping_add(1), 0x12))
}

/// An IPv4 packet with a TCP header of 20 bytes and no data.
fn tcp_packet(src: Ipv4Addr, dst: Ipv4Addr, sport: u16, dport: u16, seq: u32, ack: u32, flags: u8) -> Vec<u8> {
    let mut p = vec![0u8; 40];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&40u16.to_be_bytes());
    p[6] = 0x40; // DF
    p[8] = 64;
    p[9] = 6;
    p[12..16].copy_from_slice(&src.octets());
    p[16..20].copy_from_slice(&dst.octets());
    let ip_sum = checksum(0, &p[..20]);
    p[10..12].copy_from_slice(&ip_sum.to_be_bytes());
    let t = &mut p[20..];
    t[0..2].copy_from_slice(&sport.to_be_bytes());
    t[2..4].copy_from_slice(&dport.to_be_bytes());
    t[4..8].copy_from_slice(&seq.to_be_bytes());
    t[8..12].copy_from_slice(&ack.to_be_bytes());
    t[12] = 5 << 4;
    t[13] = flags;
    t[14..16].copy_from_slice(&64240u16.to_be_bytes());
    let mut pseudo = [0u8; 12];
    pseudo[0..4].copy_from_slice(&src.octets());
    pseudo[4..8].copy_from_slice(&dst.octets());
    pseudo[9] = 6;
    pseudo[10..12].copy_from_slice(&20u16.to_be_bytes());
    let sum = checksum(sum_of(0, &pseudo), t);
    t[16..18].copy_from_slice(&sum.to_be_bytes());
    p
}

fn sum_of(mut acc: u32, data: &[u8]) -> u32 {
    for pair in data.chunks(2) {
        let word = u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]);
        acc += u32::from(word);
    }
    acc
}

fn checksum(acc: u32, data: &[u8]) -> u16 {
    let mut acc = sum_of(acc, data);
    while acc > 0xffff {
        acc = (acc & 0xffff) + (acc >> 16);
    }
    !(acc as u16)
}

#[cfg(test)]
mod tests;
