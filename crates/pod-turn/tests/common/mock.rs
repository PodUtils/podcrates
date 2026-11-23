// The core of a mock TURN server (RFC 8656) for the tests: the 401
// challenge, the nonce and its 438, the long-term credentials of MD5 or of
// SHA-256, the 441 of a user that is not the allocation's, Allocate,
// Refresh, CreatePermission, ChannelBind, Send indications and ChannelData
// to a relay socket on the loopback, and Data indications and ChannelData
// back. Its own reading of the rules, apart from the client; it shares the
// codec, whose vectors are RFC 5769's and a SHA-256 vector computed apart.
// The options of an Allocate are in `mock_alloc.rs`: the nonce cookie,
// USERHASH and the 300 redirect are here.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use pod_turn::stun::{self, Class, PasswordAlgorithm, SecurityFeature, message_type};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// What the mock does: its realm, its users with their passwords, and its
/// algorithms.
#[derive(Clone)]
pub struct Knobs {
    pub realm: String,
    pub users: Vec<(String, String)>,
    /// Offer PASSWORD-ALGORITHMS (SHA-256, then MD5) in each challenge.
    pub sha256: bool,
    /// The most lifetime that it gives, in seconds.
    pub max_lifetime: u32,
    /// The 24 bits of the security features of the nonce cookie; `None`
    /// writes a nonce with no cookie. With username anonymity, a request
    /// must send USERHASH; with none, USERNAME.
    pub cookie: Option<u32>,
    /// A party on the path takes PASSWORD-ALGORITHMS out of each challenge.
    pub strip_algorithms: bool,
    /// The mock sets the DF bit for DONT-FRAGMENT; else it answers 420.
    pub dont_fragment: bool,
    /// The mock gives IPv6 relayed addresses ([::1] and the port of its IPv4
    /// relay socket, as the host can have no IPv6); else it answers 440.
    pub ipv6: bool,
    /// The TLS mock serves the ALPN `stun.turn`.
    pub alpn: bool,
    /// A Connect that the TCP mock refuses with this code, as a server with
    /// no route to the peer does (447) or a policy does (403).
    pub refuse_connect: Option<u16>,
    /// The TCP mock stores a pending connection under its id xor this
    /// value: a bind with the id that it gave is refused when it is 1.
    pub bind_salt: u32,
    /// After a successful ConnectionBind the TCP mock writes a length with
    /// too few bytes and closes: a truncated frame.
    pub truncated_frame: bool,
}

/// A 300 (Try Alternate) of each Allocate: before the challenge as coturn
/// sends it, or after it, signed with the key of the user.
#[derive(Debug, Clone)]
pub struct Alternate {
    pub to: SocketAddr,
    pub after_challenge: bool,
    pub domain: Option<String>,
}

/// One request that the mock read, and its answer (0 for a success).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub method: u16,
    pub user: Option<String>,
    /// The request sent USERHASH.
    pub userhash: bool,
    /// The types of its attributes, in order.
    pub attributes: Vec<u16>,
    pub nonce: Option<Vec<u8>>,
    pub algorithm: Option<u16>,
    pub code: u16,
}

/// What a test changes while the mock runs.
#[derive(Default)]
pub struct Control {
    /// The next authenticated requests to answer with 438 and a new nonce.
    pub stale_next: u32,
    /// The user that the allocations belong to from now on, in place of the
    /// user that made them: each request of the old user then gets 441.
    pub owner: Option<String>,
    /// The requests that the mock read, in order.
    pub seen: Vec<Seen>,
    /// The Send indications and the ChannelData that it relayed.
    pub sent: u32,
    pub channel_data: u32,
    /// The Send indications with DONT-FRAGMENT that it relayed.
    pub sent_dont_fragment: u32,
    /// The redirect of each Allocate.
    pub alternate: Option<Alternate>,
    /// The relay sockets that a RESERVATION-TOKEN keeps, by the token.
    pub reserved: HashMap<[u8; 8], Arc<UdpSocket>>,
    /// Each success carries a NONCE that announces PASSWORD-ALGORITHMS, and
    /// no PASSWORD-ALGORITHMS: the answer of a party on the path.
    pub downgraded_successes: bool,
}

pub type Shared = Arc<Mutex<Control>>;

/// Data of a peer that a relay socket read: the client's key, the peer,
/// the bytes.
pub type Relayed = (SocketAddr, SocketAddr, Vec<u8>);

/// The state of one client (one UDP 5-tuple, or one TLS connection).
pub struct Session {
    pub client: SocketAddr,
    pub(super) nonce: Vec<u8>,
    nonces: u32,
    pub(super) alloc: Option<Alloc>,
}

pub(super) struct Alloc {
    pub owner: String,
    pub relay: Arc<UdpSocket>,
    pub permissions: HashSet<IpAddr>,
    pub channels: HashMap<u16, SocketAddr>,
}

impl Session {
    pub fn new(client: SocketAddr) -> Session {
        Session { client, nonce: Vec::new(), nonces: 0, alloc: None }
    }
}

pub(super) const SUCCESS: u16 = 0;

pub struct Mock {
    pub knobs: Knobs,
    pub control: Shared,
    /// Where the relay sockets send what peers sent.
    pub relayed: mpsc::UnboundedSender<Relayed>,
}

impl Mock {
    fn algorithm(&self) -> PasswordAlgorithm {
        if self.knobs.sha256 { PasswordAlgorithm::Sha256 } else { PasswordAlgorithm::Md5 }
    }

    /// The user of a request and the key: by USERHASH when the nonce
    /// cookie announces username anonymity, else by USERNAME; `None` for a
    /// user that the mock does not know, or for the other attribute.
    pub(super) fn user_and_key(&self, read: &stun::Message<'_>) -> Option<(String, Vec<u8>)> {
        let realm = &self.knobs.realm;
        let anonymity = self.knobs.cookie.is_some_and(|bits| bits & SecurityFeature::UsernameAnonymity.bits() != 0);
        let (name, password) = match (anonymity, read.attribute(stun::USERHASH), read.attribute(stun::USERNAME)) {
            (true, Some(hash), None) => {
                self.knobs.users.iter().find(|(name, _)| stun::userhash(name, realm) == hash)?
            }
            (false, None, Some(user)) => self.knobs.users.iter().find(|(name, _)| name.as_bytes() == user)?,
            _ => return None,
        };
        Some((name.clone(), self.algorithm().key(name, realm, password)))
    }

    fn offered(&self) -> Vec<u8> {
        stun::write_password_algorithms(&[PasswordAlgorithm::Sha256, PasswordAlgorithm::Md5])
    }

    fn fresh_nonce(&self, session: &mut Session) -> Vec<u8> {
        session.nonces += 1;
        let prefix = self.knobs.cookie.map_or_else(|| "mock-nonce-".to_string(), stun::nonce_cookie);
        session.nonce = format!("{prefix}{}", session.nonces).into_bytes();
        session.nonce.clone()
    }

    /// An answer of `method` with `class` and `attributes`, signed with
    /// `key` unless it is a challenge.
    pub(super) fn answer(
        &self,
        method: u16,
        class: Class,
        id: &[u8; 12],
        attributes: &[(u16, &[u8])],
        key: Option<&[u8]>,
    ) -> Vec<u8> {
        let cookie = stun::nonce_cookie(SecurityFeature::PasswordAlgorithms.bit()) + "path";
        let mut attributes = attributes.to_vec();
        if class == Class::Success && self.control.lock().unwrap().downgraded_successes {
            attributes.push((stun::NONCE, cookie.as_bytes()));
        }
        let bytes = stun::message(message_type(method, class), id, &attributes);
        let Some(key) = key else { return bytes };
        match self.algorithm() {
            PasswordAlgorithm::Md5 => stun::signed(&bytes, key),
            PasswordAlgorithm::Sha256 => stun::signed_sha256(&bytes, key),
        }
    }

    pub(super) fn error(
        &self,
        method: u16,
        id: &[u8; 12],
        code: u16,
        more: &[(u16, &[u8])],
        key: Option<&[u8]>,
    ) -> Vec<u8> {
        let value = [[0, 0, (code / 100) as u8, (code % 100) as u8].as_slice(), b"mock".as_slice()].concat();
        let mut attributes = vec![(stun::ERROR_CODE, value.as_slice())];
        attributes.extend_from_slice(more);
        self.answer(method, Class::Error, id, &attributes, key)
    }

    pub(super) fn challenge(&self, session: &mut Session, method: u16, id: &[u8; 12], code: u16) -> Vec<u8> {
        let nonce = self.fresh_nonce(session);
        let offered = self.offered();
        let mut more = vec![(stun::REALM, self.knobs.realm.as_bytes()), (stun::NONCE, nonce.as_slice())];
        if self.knobs.sha256 && !self.knobs.strip_algorithms {
            more.push((stun::PASSWORD_ALGORITHMS, offered.as_slice()));
        }
        self.error(method, id, code, &more, None)
    }

    pub(super) fn note(&self, method: u16, read: &stun::Message<'_>, code: u16) {
        let text = |k| read.attribute(k).map(|v| String::from_utf8_lossy(v).into_owned());
        let algorithm = read.attribute(stun::PASSWORD_ALGORITHM).map(|v| u16::from_be_bytes([v[0], v[1]]));
        let seen = Seen {
            method,
            user: text(stun::USERNAME),
            userhash: read.attribute(stun::USERHASH).is_some(),
            attributes: read.attributes.iter().map(|(k, _)| *k).collect(),
            nonce: read.attribute(stun::NONCE).map(<[u8]>::to_vec),
            algorithm,
            code,
        };
        self.control.lock().unwrap().seen.push(seen);
    }

    /// The answers to one STUN message of the client; a ChannelData of the
    /// client goes to `channel_data`.
    pub async fn handle(&self, session: &mut Session, bytes: &[u8]) -> Vec<Vec<u8>> {
        let Ok(read) = stun::parse(bytes) else { return Vec::new() };
        let (method, class, id) = (stun::method_of(read.kind), stun::class_of(read.kind), read.id);
        if class == Class::Indication {
            if method == stun::SEND {
                self.send_indication(session, &read).await;
            }
            return Vec::new();
        }
        if class != Class::Request {
            return Vec::new();
        }
        let alternate = self.control.lock().unwrap().alternate.clone().filter(|_| method == stun::ALLOCATE);
        if let Some(alternate) = alternate.as_ref().filter(|a| !a.after_challenge) {
            self.note(method, &read, 300);
            return vec![self.redirect(&id, alternate, None)];
        }
        let user = read.attribute(stun::USERNAME).or(read.attribute(stun::USERHASH));
        let signed = read.attribute(stun::MESSAGE_INTEGRITY).is_some()
            || read.attribute(stun::MESSAGE_INTEGRITY_SHA256).is_some();
        if user.is_none() || !signed {
            self.note(method, &read, 401);
            return vec![self.challenge(session, method, &id, 401)];
        }
        if read.attribute(stun::NONCE) != Some(session.nonce.as_slice()) {
            self.note(method, &read, 438);
            return vec![self.challenge(session, method, &id, 438)];
        }
        let Some((user, key)) = self.user_and_key(&read).filter(|(_, key)| self.verify(bytes, &read, key).is_ok())
        else {
            self.note(method, &read, 401);
            return vec![self.challenge(session, method, &id, 401)];
        };
        let stale = {
            let mut control = self.control.lock().unwrap();
            let stale = control.stale_next > 0 && method != stun::ALLOCATE;
            control.stale_next -= u32::from(stale);
            stale
        };
        if stale {
            self.note(method, &read, 438);
            return vec![self.challenge(session, method, &id, 438)];
        }
        if let Some(alternate) = &alternate {
            self.note(method, &read, 300);
            return vec![self.redirect(&id, alternate, Some(&key))];
        }
        let owner = self.control.lock().unwrap().owner.clone();
        if let Some(alloc) = &session.alloc {
            let owner = owner.unwrap_or_else(|| alloc.owner.clone());
            if user != owner {
                self.note(method, &read, 441);
                return vec![self.error(method, &id, 441, &[], Some(&key))];
            }
        }
        let (code, answer) = self.method(session, method, &read, &user, &key).await;
        self.note(method, &read, code);
        vec![answer]
    }

    /// The integrity of the algorithm of the challenge: SHA-256 with the
    /// algorithms sent back as offered, else MD5.
    pub(super) fn verify(&self, bytes: &[u8], read: &stun::Message<'_>, key: &[u8]) -> Result<(), String> {
        if !self.knobs.sha256 {
            return stun::integrity(bytes, key).map_err(|e| e.to_string());
        }
        if read.attribute(stun::PASSWORD_ALGORITHMS) != Some(self.offered().as_slice()) {
            return Err("PASSWORD-ALGORITHMS is not the one offered".into());
        }
        if read.attribute(stun::PASSWORD_ALGORITHM) != Some(PasswordAlgorithm::Sha256.attribute().as_slice()) {
            return Err("PASSWORD-ALGORITHM is not SHA-256".into());
        }
        stun::integrity_sha256(bytes, key).map_err(|e| e.to_string())
    }

    /// A 300 with ALTERNATE-SERVER and the ALTERNATE-DOMAIN of
    /// `alternate`, signed with `key` after the challenge.
    fn redirect(&self, id: &[u8; 12], alternate: &Alternate, key: Option<&[u8]>) -> Vec<u8> {
        let to = stun::write_address(alternate.to, None);
        let mut more = vec![(stun::ALTERNATE_SERVER, to.as_slice())];
        more.extend(alternate.domain.as_ref().map(|d| (stun::ALTERNATE_DOMAIN, d.as_bytes())));
        self.error(stun::ALLOCATE, id, 300, &more, key)
    }

    async fn method(
        &self,
        session: &mut Session,
        method: u16,
        read: &stun::Message<'_>,
        user: &str,
        key: &[u8],
    ) -> (u16, Vec<u8>) {
        let id = read.id;
        let peer = read.attribute(stun::XOR_PEER_ADDRESS).and_then(|v| stun::read_address(v, Some(&id)).ok());
        let asked = read.attribute(stun::LIFETIME).map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]));
        let lifetime = asked.unwrap_or(600).min(self.knobs.max_lifetime);
        let fail = |code| (code, self.error(method, &id, code, &[], Some(key)));
        match method {
            stun::ALLOCATE => self.allocate(session, read, user, key, lifetime).await,
            stun::REFRESH => {
                if session.alloc.is_none() {
                    return fail(437);
                }
                let lifetime = if asked == Some(0) { 0 } else { lifetime };
                if lifetime == 0 {
                    session.alloc = None;
                }
                let life = lifetime.to_be_bytes();
                (SUCCESS, self.answer(method, Class::Success, &id, &[(stun::LIFETIME, &life)], Some(key)))
            }
            stun::CREATE_PERMISSION | stun::CHANNEL_BIND => {
                let (Some(alloc), Some(peer)) = (session.alloc.as_mut(), peer) else { return fail(400) };
                if method == stun::CHANNEL_BIND {
                    let Some(value) = read.attribute(stun::CHANNEL_NUMBER) else { return fail(400) };
                    let number = u16::from_be_bytes([value[0], value[1]]);
                    let taken = alloc.channels.iter().any(|(n, p)| (*n == number) != (*p == peer));
                    if !(0x4000..=0x4fff).contains(&number) || taken {
                        return fail(400);
                    }
                    alloc.channels.insert(number, peer);
                }
                alloc.permissions.insert(peer.ip());
                (SUCCESS, self.answer(method, Class::Success, &id, &[], Some(key)))
            }
            _ => fail(400),
        }
    }

    async fn send_indication(&self, session: &mut Session, read: &stun::Message<'_>) {
        let peer = read.attribute(stun::XOR_PEER_ADDRESS).and_then(|v| stun::read_address(v, Some(&read.id)).ok());
        let (Some(alloc), Some(peer), Some(data)) = (&session.alloc, peer, read.attribute(stun::DATA)) else { return };
        if alloc.permissions.contains(&peer.ip()) {
            {
                let mut control = self.control.lock().unwrap();
                control.sent += 1;
                control.sent_dont_fragment += u32::from(read.attribute(stun::DONT_FRAGMENT).is_some());
            }
            let _ = alloc.relay.send_to(data, peer).await;
        }
    }

    /// ChannelData of the client: to the channel's peer.
    pub async fn channel_data(&self, session: &Session, number: u16, data: &[u8]) {
        let Some(alloc) = &session.alloc else { return };
        if let Some(peer) = alloc.channels.get(&number) {
            self.control.lock().unwrap().channel_data += 1;
            let _ = alloc.relay.send_to(data, *peer).await;
        }
    }

    /// What a peer sent to the relay, for the client: ChannelData when the
    /// peer has a channel (padded on a stream), else a Data indication;
    /// nothing with no permission.
    pub fn to_client(&self, session: &Session, peer: SocketAddr, data: &[u8], padded: bool) -> Option<Vec<u8>> {
        let alloc = session.alloc.as_ref()?;
        if !alloc.permissions.contains(&peer.ip()) {
            return None;
        }
        if let Some((number, _)) = alloc.channels.iter().find(|(_, p)| **p == peer) {
            let mut out = [number.to_be_bytes(), (data.len() as u16).to_be_bytes()].concat();
            out.extend_from_slice(data);
            if padded {
                out.resize(out.len() + (4 - data.len() % 4) % 4, 0);
            }
            return Some(out);
        }
        let id: [u8; 12] = std::array::from_fn(|i| i as u8 ^ data.len() as u8);
        let address = stun::write_address(peer, Some(&id));
        let attributes = [(stun::XOR_PEER_ADDRESS, address.as_slice()), (stun::DATA, data)];
        Some(stun::message(message_type(stun::DATA_METHOD, Class::Indication), &id, &attributes))
    }
}

/// Read what peers send to `relay`, for the session of `client`.
pub(super) fn spawn_relay(relay: Arc<UdpSocket>, client: SocketAddr, out: mpsc::UnboundedSender<Relayed>) {
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65_536];
        while let Ok((n, peer)) = relay.recv_from(&mut buf).await {
            if out.send((client, peer, buf[..n].to_vec())).is_err() {
                return;
            }
        }
    });
}
