//! One request and its answer: the credentials of a request (USERNAME, or
//! USERHASH when the nonce asks for it), the send again over UDP, the 401
//! challenge and the 438 nonce with the security features of the nonce, a
//! 300 redirect, the integrity of the answer; and each other message that
//! comes while a request waits, which becomes an event.

use std::time::Duration;

use crate::stun::{
    self, ALTERNATE_DOMAIN, ALTERNATE_SERVER, Class, DATA, DATA_METHOD, HEADER, Message, NONCE, PASSWORD_ALGORITHM,
    PASSWORD_ALGORITHMS, PasswordAlgorithm, REALM, SOFTWARE, SecurityFeature, TransactionId, USERHASH, USERNAME,
    XOR_PEER_ADDRESS, class_of, decode, message, message_type, method_of,
};

use super::error::method_name;
use super::wire::Frame;
use super::{Client, Error, Event};

/// The ids of past requests that the client keeps: a late answer to one is
/// dropped, not given as an event.
const RECENT: usize = 16;

/// The credentials of the allocation after the challenge: the realm and the
/// nonce of the server, the algorithm of the key, and the key. The user and
/// the password never change on a live allocation.
pub struct Auth {
    pub realm: String,
    pub nonce: Vec<u8>,
    pub algorithm: PasswordAlgorithm,
    /// The PASSWORD-ALGORITHMS of the challenge, sent back with each request
    /// (RFC 8489, 9.2.4); `None` from a server that sent none (MD5 alone).
    pub offered: Option<Vec<u8>>,
    pub key: Vec<u8>,
    /// The USERHASH that each request sends in place of USERNAME, when the
    /// nonce announces username anonymity (RFC 8489, 9.2.5).
    pub userhash: Option<[u8; 32]>,
}

/// The value of an attribute of a request: bytes, or a peer's address,
/// which is XORed with the id of each send (XOR-PEER-ADDRESS).
pub enum Value {
    Bytes(Vec<u8>),
    Peer(std::net::SocketAddr),
}

impl Drop for Auth {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.key.zeroize();
    }
}

impl Client {
    /// Send a request of `method` with `attributes` and give its success
    /// answer, whole. A 401 to the first request with no credentials gives
    /// the realm and the nonce, and the request goes again with them; a 438
    /// gives a fresh nonce, and the request goes again with the same user
    /// and key, at most `stale_nonces` times. Each other error answer fails.
    pub(super) async fn transact(&mut self, method: u16, attributes: &[(u16, Value)]) -> Result<Vec<u8>, Error> {
        let (mut stale, mut challenged) = (0u32, false);
        loop {
            let id = self.new_id()?;
            let request = self.request(method, &id, attributes);
            let reply = self.exchange(method, id, &request).await?;
            let read = decode(&reply).map_err(Error::Decode)?;
            let kind = read.kind;
            if kind == message_type(method, Class::Success) {
                if self.auth.is_some() {
                    self.check(&reply, kind)?;
                }
                return Ok(reply);
            }
            if kind != message_type(method, Class::Error) {
                let why = format!("an answer of another type to a {} request", method_name(method));
                return Err(Error::Protocol { kind, why });
            }
            let (code, reason) = match read.error() {
                Some(Ok(error)) => error,
                Some(Err(why)) => return Err(Error::Protocol { kind, why }),
                None => return Err(Error::Protocol { kind, why: "an error answer with no ERROR-CODE".into() }),
            };
            match code {
                401 if self.auth.is_none() && !challenged => {
                    challenged = true;
                    self.challenge(&read, false)?;
                }
                438 if self.auth.is_some() && stale < self.limits.stale_nonces => {
                    stale += 1;
                    tracing::debug!(method = method_name(method), stale, "TURN: a stale nonce; the request goes again");
                    self.challenge(&read, true)?;
                }
                438 if self.auth.is_some() => return Err(Error::StaleNonce { method, tries: stale + 1 }),
                300 => return Err(self.alternate(&read, &reply, method, reason)),
                _ => {
                    if self.auth.is_some() && stun::integrity_of(&read.attributes).is_some() {
                        self.check(&reply, kind)?;
                    }
                    return Err(Error::Server { method, code, reason });
                }
            }
        }
    }

    /// A new transaction id from the caller's entropy.
    pub(super) fn new_id(&mut self) -> Result<TransactionId, Error> {
        let mut id = [0u8; 12];
        self.entropy.fill(&mut id).map_err(|_| Error::NoRandom)?;
        Ok(id)
    }

    /// The bytes of a request: its attributes, SOFTWARE, and with the
    /// credentials of the allocation the user, the realm, the nonce, the
    /// algorithms, and the integrity of the chosen algorithm last.
    fn request(&self, method: u16, id: &TransactionId, attributes: &[(u16, Value)]) -> Vec<u8> {
        request_with(self.auth.as_ref(), self.credentials.username(), self.software.as_deref(), method, id, attributes)
    }

    /// Take the realm, the nonce and the algorithms of a 401 (`stale` false)
    /// or of a 438 (`stale` true), and make the key. A realm that the
    /// settings do not expect fails. A 438 keeps the realm when it names
    /// none. A nonce that announces PASSWORD-ALGORITHMS in a challenge with
    /// none fails (a downgrade); one that announces username anonymity makes
    /// each request send USERHASH.
    fn challenge(&mut self, read: &Message<'_>, stale: bool) -> Result<(), Error> {
        let kind = read.kind;
        let protocol = |why: &str| Error::Protocol { kind, why: why.to_string() };
        let nonce = read.attribute(NONCE).ok_or_else(|| protocol("a challenge with no NONCE"))?.to_vec();
        if downgraded(read) {
            return Err(Error::Downgrade { method: method_of(kind) });
        }
        let realm = match (read.attribute(REALM), &self.auth) {
            (Some(value), _) => String::from_utf8(value.to_vec()).map_err(|_| protocol("a REALM that is no UTF-8"))?,
            (None, Some(auth)) if stale => auth.realm.clone(),
            (None, _) => return Err(protocol("a challenge with no REALM")),
        };
        if let Some(expected) = &self.realm
            && *expected != realm
        {
            return Err(Error::Realm { expected: expected.clone(), offered: realm });
        }
        let (algorithm, offered) = match read.attribute(PASSWORD_ALGORITHMS) {
            None => (PasswordAlgorithm::Md5, None),
            Some(value) => {
                let list = stun::password_algorithms(value).map_err(|why| Error::Protocol { kind, why })?;
                let has = |a: PasswordAlgorithm| list.iter().any(|(n, _)| *n == a.number());
                let algorithm = if has(PasswordAlgorithm::Sha256) {
                    PasswordAlgorithm::Sha256
                } else if has(PasswordAlgorithm::Md5) {
                    PasswordAlgorithm::Md5
                } else {
                    return Err(protocol("a challenge with no password algorithm that this client knows"));
                };
                (algorithm, Some(value.to_vec()))
            }
        };
        let user = self.credentials.username();
        let key = algorithm.key(user, &realm, self.credentials.password());
        let userhash = SecurityFeature::UsernameAnonymity.announced_by(&nonce).then(|| stun::userhash(user, &realm));
        self.auth = Some(Auth { realm, nonce, algorithm, offered, key, userhash });
        Ok(())
    }

    /// A 300 (Try Alternate) to a request of `method`: the ALTERNATE-SERVER
    /// to follow, with the ALTERNATE-DOMAIN only when MESSAGE-INTEGRITY-SHA256
    /// with the key of the allocation signs the answer (RFC 8489, 10); an
    /// answer whose integrity fails is refused. A 300 with no
    /// ALTERNATE-SERVER is the server's error.
    fn alternate(&self, read: &Message<'_>, reply: &[u8], method: u16, reason: String) -> Error {
        let kind = read.kind;
        let signed = match (&self.auth, stun::integrity_of(&read.attributes)) {
            (Some(auth), Some(_)) => match self.check(reply, kind) {
                Ok(()) => auth.algorithm == PasswordAlgorithm::Sha256,
                Err(error) => return error,
            },
            _ => false,
        };
        let Some(value) = read.attribute(ALTERNATE_SERVER) else { return Error::Server { method, code: 300, reason } };
        let to = match stun::read_address(value, None) {
            Ok(to) => to,
            Err(why) => return Error::Protocol { kind, why: format!("ALTERNATE-SERVER: {why}") },
        };
        let domain = read.attribute(ALTERNATE_DOMAIN).filter(|_| signed);
        Error::Alternate { method, to, domain: domain.map(|v| String::from_utf8_lossy(v).into_owned()) }
    }

    /// The integrity of an answer, with the key of the chosen algorithm: an
    /// answer with only the weaker one fails.
    fn check(&self, reply: &[u8], kind: u16) -> Result<(), Error> {
        check_with(self.auth.as_ref(), reply, kind)
    }

    /// Send `request` and wait for the answer of `id`. Over UDP the request
    /// goes again after `rto`, which doubles each time, `retransmits` times;
    /// each transport waits at most `request_timeout` in all.
    async fn exchange(&mut self, method: u16, id: TransactionId, request: &[u8]) -> Result<Vec<u8>, Error> {
        let start = self.clock.now();
        let end = start + self.limits.request_timeout;
        let again = if self.conn.padded() { 0 } else { self.limits.retransmits };
        let mut rto = self.limits.rto;
        self.recent.push_back(id);
        if self.recent.len() > RECENT {
            self.recent.pop_front();
        }
        for send in 0..=again {
            self.conn.send(request).await?;
            let wait = if send < again { (self.clock.now() + rto).min(end) } else { end };
            rto = rto.saturating_mul(2);
            loop {
                let frame = tokio::select! {
                    frame = self.conn.recv() => frame?,
                    () = self.clock.sleep_until(wait) => break,
                };
                match frame {
                    Frame::Stun(bytes) if answers(&bytes, &id) && !ignored(&bytes) => return Ok(bytes),
                    other => self.incoming(other),
                }
            }
            if self.clock.now() >= end {
                break;
            }
        }
        let waited = self.clock.now().saturating_duration_since(start);
        Err(Error::Timeout { method, waited: waited.max(Duration::from_millis(1)) })
    }

    /// A message that is no answer to the request in flight: data becomes
    /// an event; an error answer that no request waits for becomes an
    /// event; the rest is dropped. A full queue drops the event.
    pub(super) fn incoming(&mut self, frame: Frame) {
        let event = match frame {
            Frame::Channel { number, data } => match self.channels.get(&number) {
                Some((peer, _)) => Event::Data { peer: *peer, channel: Some(number), data },
                None => {
                    tracing::debug!(number, "TURN: ChannelData of a channel that is not bound is dropped");
                    return;
                }
            },
            Frame::Stun(bytes) => match self.stun_event(&bytes) {
                Some(event) => event,
                None => return,
            },
        };
        if self.queue.len() >= self.limits.queue {
            tracing::debug!("TURN: the queue of events is full; an event is dropped");
            return;
        }
        self.queue.push_back(event);
    }

    fn stun_event(&self, bytes: &[u8]) -> Option<Event> {
        let read = match decode(bytes) {
            Ok(read) => read,
            Err(e) => {
                tracing::debug!(%e, "TURN: a message that does not decode is dropped");
                return None;
            }
        };
        match (class_of(read.kind), method_of(read.kind)) {
            (Class::Indication, DATA_METHOD) => {
                let peer = stun::read_address(read.attribute(XOR_PEER_ADDRESS)?, Some(&read.id)).ok()?;
                Some(Event::Data { peer, channel: None, data: read.attribute(DATA)?.to_vec() })
            }
            (Class::Error, method) if !self.recent.contains(&read.id) => {
                let (code, reason) = read.error()?.ok()?;
                Some(Event::ServerError { method, code, reason })
            }
            _ => None,
        }
    }
}

/// True when the NONCE of `read` announces PASSWORD-ALGORITHMS and `read`
/// holds none (RFC 8489, 9.2.5).
fn downgraded(read: &Message<'_>) -> bool {
    read.attribute(NONCE).is_some_and(|nonce| SecurityFeature::PasswordAlgorithms.announced_by(nonce))
        && read.attribute(PASSWORD_ALGORITHMS).is_none()
}

/// True for an answer that the client must drop as if it never came: one
/// with a downgraded nonce that is no 401 or 438 (RFC 8489, 9.2.5). The
/// request then waits on, and goes again over UDP.
fn ignored(bytes: &[u8]) -> bool {
    let Ok(read) = decode(bytes) else { return false };
    let challenge = matches!(read.error(), Some(Ok((401 | 438, _))));
    !challenge && downgraded(&read)
}

/// The bytes of a request with `auth`, as [`Client::request`] writes it:
/// the attributes, SOFTWARE, and with the credentials of an allocation the
/// user, the realm, the nonce, the algorithms, and the integrity last.
pub(super) fn request_with(
    auth: Option<&Auth>,
    username: &str,
    software: Option<&str>,
    method: u16,
    id: &TransactionId,
    attributes: &[(u16, Value)],
) -> Vec<u8> {
    let kind = message_type(method, Class::Request);
    let peers: Vec<Vec<u8>> = attributes
        .iter()
        .filter_map(|(_, v)| match v {
            Value::Peer(peer) => Some(stun::write_address(*peer, Some(id))),
            Value::Bytes(_) => None,
        })
        .collect();
    let mut peers = peers.iter();
    let mut list: Vec<(u16, &[u8])> = Vec::with_capacity(attributes.len() + 8);
    for (k, v) in attributes {
        match v {
            Value::Bytes(bytes) => list.push((*k, bytes)),
            Value::Peer(_) => list.extend(peers.next().map(|p| (*k, p.as_slice()))),
        }
    }
    if let Some(software) = software {
        list.push((SOFTWARE, software.as_bytes()));
    }
    let Some(auth) = auth else { return message(kind, id, &list) };
    match &auth.userhash {
        Some(hash) => list.push((USERHASH, hash)),
        None => list.push((USERNAME, username.as_bytes())),
    }
    list.push((REALM, auth.realm.as_bytes()));
    list.push((NONCE, &auth.nonce));
    let chosen = auth.algorithm.attribute();
    if let Some(offered) = &auth.offered {
        list.push((PASSWORD_ALGORITHMS, offered));
        list.push((PASSWORD_ALGORITHM, &chosen));
    }
    let unsigned = message(kind, id, &list);
    match auth.algorithm {
        PasswordAlgorithm::Md5 => stun::signed(&unsigned, &auth.key),
        PasswordAlgorithm::Sha256 => stun::signed_sha256(&unsigned, &auth.key),
    }
}

/// The integrity of an answer, with the key of the chosen algorithm.
pub(super) fn check_with(auth: Option<&Auth>, reply: &[u8], kind: u16) -> Result<(), Error> {
    let Some(auth) = auth else { return Ok(()) };
    let result = match auth.algorithm {
        PasswordAlgorithm::Md5 => stun::integrity(reply, &auth.key),
        PasswordAlgorithm::Sha256 => stun::integrity_sha256(reply, &auth.key),
    };
    result.map_err(|error| Error::Integrity { kind, error })
}

/// Send `request` on `conn` and wait for the answer of `id`, for
/// `limits.request_timeout`: a stream does not send a request again. This
/// is the transaction of a data connection and of the keeper, which have no
/// [`Client`] to queue the other messages.
pub(super) async fn exchange_on(
    conn: &mut super::wire::Conn,
    request: &[u8],
    id: &TransactionId,
    clock: &dyn super::Clock,
    limits: &super::Limits,
    method: u16,
) -> Result<Vec<u8>, Error> {
    let start = clock.now();
    let end = start + limits.request_timeout;
    conn.send(request).await?;
    loop {
        let frame = tokio::select! {
            frame = conn.recv() => frame?,
            () = clock.sleep_until(end) => {
                let waited = clock.now().saturating_duration_since(start);
                return Err(Error::Timeout { method, waited: waited.max(Duration::from_millis(1)) });
            }
        };
        if let Frame::Stun(bytes) = frame
            && answers(&bytes, id)
        {
            return Ok(bytes);
        }
    }
}

/// True when `bytes` is an answer (a success or an error) of `id`.
pub(super) fn answers(bytes: &[u8], id: &TransactionId) -> bool {
    bytes.len() >= HEADER
        && bytes[8..HEADER] == id[..]
        && matches!(class_of(u16::from_be_bytes([bytes[0], bytes[1]])), Class::Success | Class::Error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stun::{ALLOCATE, ERROR_CODE};

    /// An answer with a nonce that announces PASSWORD-ALGORITHMS and holds
    /// none is dropped, unless it is a 401 or a 438, which fail instead.
    #[test]
    fn an_answer_with_a_downgraded_nonce_is_dropped() {
        let nonce = stun::nonce_cookie(SecurityFeature::PasswordAlgorithms.bit()) + "rest";
        let offered = stun::write_password_algorithms(&[PasswordAlgorithm::Md5]);
        let answer = |class, code: Option<u16>, algorithms: bool| {
            let error = code.map(|c| vec![0, 0, (c / 100) as u8, (c % 100) as u8]);
            let mut list: Vec<(u16, &[u8])> = vec![(NONCE, nonce.as_bytes())];
            list.extend(error.as_deref().map(|e| (ERROR_CODE, e)));
            if algorithms {
                list.push((PASSWORD_ALGORITHMS, offered.as_slice()));
            }
            message(message_type(ALLOCATE, class), &[3; 12], &list)
        };
        assert!(ignored(&answer(Class::Success, None, false)));
        assert!(ignored(&answer(Class::Error, Some(400), false)));
        assert!(!ignored(&answer(Class::Success, None, true)));
        assert!(!ignored(&answer(Class::Error, Some(401), false)), "a 401 fails as a downgrade");
        assert!(!ignored(&answer(Class::Error, Some(438), false)), "a 438 fails as a downgrade");
        let plain = message(message_type(ALLOCATE, Class::Success), &[3; 12], &[(NONCE, b"AAABrest")]);
        assert!(!ignored(&plain), "no cookie");
    }
}
