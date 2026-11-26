//! Shadowsocks servers on the loopback, written from shadowsocks.org's
//! `aead.html` and `sip022.html` with the AEAD crates and BLAKE3 themselves,
//! not this crate's codec ([`Spec`]), each for one connection over TCP:
//!
//! * [`aead`]: SIP004's AEAD form. The key of a stream is HKDF-SHA1 with
//!   `ss-subkey` over the master key (EVP_BytesToKey of MD5, [`evp`]) and
//!   the salt.
//! * [`sip022`]: Shadowsocks 2022. The key of a stream is BLAKE3's
//!   `derive_key` with "shadowsocks 2022 session subkey" over the key and
//!   the salt; the request's fixed header (type 0, a timestamp, the next
//!   header's length) and its variable one (the address, then padding); the
//!   response's fixed header (type 1, a timestamp, the request's salt, the
//!   first payload's length). A timestamp more than 30 s off is a close, as
//!   shadowsocks-rust does.
//!
//! With the right key a server answers its banner, then echoes; with another
//! it stays silent until the client goes. Its task gives the target that it
//! read.

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Aes256Gcm};
use chacha20poly1305::ChaCha20Poly1305;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::shadowsocks::{Method, parse_address};

/// The context of BLAKE3's key of a stream of Shadowsocks 2022.
const CONTEXT: &str = "shadowsocks 2022 session subkey";

/// The task of a server: the target that it read, when the key was right.
pub type Served = tokio::task::JoinHandle<Option<String>>;

/// The specification's cipher of one direction, with the count of
/// operations as the nonce, from zero.
pub enum Spec {
    Chacha(Box<ChaCha20Poly1305>),
    Aes256(Box<Aes256Gcm>),
    Aes128(Box<Aes128Gcm>),
}

impl Spec {
    /// SIP004's cipher of a stream: HKDF-SHA1 of `master` and `salt` with
    /// `ss-subkey`. None for a method of Shadowsocks 2022.
    pub fn aead(method: Method, master: &[u8], salt: &[u8]) -> Option<Spec> {
        let mut key = vec![0u8; master.len()];
        hkdf::Hkdf::<sha1::Sha1>::new(Some(salt), master).expand(b"ss-subkey", &mut key).ok()?;
        match method {
            Method::Chacha20IetfPoly1305 => Some(Spec::Chacha(Box::new(ChaCha20Poly1305::new_from_slice(&key).ok()?))),
            Method::Aes256Gcm => Some(Spec::Aes256(Box::new(Aes256Gcm::new_from_slice(&key).ok()?))),
            Method::Aes128Gcm => Some(Spec::Aes128(Box::new(Aes128Gcm::new_from_slice(&key).ok()?))),
            _ => None,
        }
    }

    /// SIP022's cipher of a stream: BLAKE3's `derive_key` of `psk` and
    /// `salt`. None for a method of the AEAD form.
    pub fn sip022(method: Method, psk: &[u8], salt: &[u8]) -> Option<Spec> {
        let mut hasher = blake3::Hasher::new_derive_key(CONTEXT);
        hasher.update(psk).update(salt);
        let mut key = vec![0u8; psk.len()];
        hasher.finalize_xof().fill(&mut key);
        match method {
            Method::Blake3Chacha20Poly1305 => {
                Some(Spec::Chacha(Box::new(ChaCha20Poly1305::new_from_slice(&key).ok()?)))
            }
            Method::Blake3Aes256Gcm => Some(Spec::Aes256(Box::new(Aes256Gcm::new_from_slice(&key).ok()?))),
            Method::Blake3Aes128Gcm => Some(Spec::Aes128(Box::new(Aes128Gcm::new_from_slice(&key).ok()?))),
            _ => None,
        }
    }

    fn nonce(n: u64) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[..8].copy_from_slice(&n.to_le_bytes());
        nonce
    }

    /// `plain` sealed with the nonce of operation `n`.
    pub fn seal(&self, n: u64, plain: &[u8]) -> Vec<u8> {
        let nonce = Spec::nonce(n);
        let gcm: aes_gcm::Nonce<aes_gcm::aead::consts::U12> = nonce.into();
        let sealed = match self {
            Spec::Chacha(c) => c.encrypt(&chacha20poly1305::Nonce::from(nonce), plain),
            Spec::Aes256(c) => c.encrypt(&gcm, plain),
            Spec::Aes128(c) => c.encrypt(&gcm, plain),
        };
        // An AEAD of RustCrypto fails to seal only past its length bound.
        sealed.unwrap_or_default()
    }

    /// `sealed` opened with the nonce of operation `n`, or none.
    pub fn open(&self, n: u64, sealed: &[u8]) -> Option<Vec<u8>> {
        let nonce = Spec::nonce(n);
        let gcm: aes_gcm::Nonce<aes_gcm::aead::consts::U12> = nonce.into();
        match self {
            Spec::Chacha(c) => c.decrypt(&chacha20poly1305::Nonce::from(nonce), sealed).ok(),
            Spec::Aes256(c) => c.decrypt(&gcm, sealed).ok(),
            Spec::Aes128(c) => c.decrypt(&gcm, sealed).ok(),
        }
    }
}

/// EVP_BytesToKey with MD5 from its definition: D1 = MD5(password),
/// Dn = MD5(Dn-1 || password), the key the first bytes of D1 || D2 ...
pub fn evp(password: &[u8], len: usize) -> Vec<u8> {
    let (mut out, mut last) = (Vec::new(), Vec::new());
    while out.len() < len {
        let mut input = last.clone();
        input.extend_from_slice(password);
        last = md5::compute(&input).0.to_vec();
        out.extend_from_slice(&last);
    }
    out.truncate(len);
    out
}

/// Read until `wire` holds `n` bytes; false when the client went first.
async fn fill(tcp: &mut TcpStream, wire: &mut Vec<u8>, n: usize) -> bool {
    let mut buf = vec![0u8; 65536];
    while wire.len() < n {
        match tcp.read(&mut buf).await {
            Ok(0) | Err(_) => return false,
            Ok(k) => wire.extend_from_slice(&buf[..k]),
        }
    }
    true
}

/// Silence until the client goes, as a server is to another key.
async fn silent(tcp: &mut TcpStream) {
    let mut buf = [0u8; 4096];
    while tcp.read(&mut buf).await.unwrap_or(0) > 0 {}
}

/// Each chunk after the header, `rest` and what comes, back sealed with
/// `writer`, until the client goes or a chunk does not open.
async fn echo(tcp: &mut TcpStream, reader: &Spec, writer: &Spec, mut rest: Vec<u8>) {
    let (mut n_in, mut n_out) = (2u64, 2u64);
    loop {
        while rest.len() >= 18 {
            let Some(l) = reader.open(n_in, &rest[..18]) else { return };
            let l = usize::from(u16::from_be_bytes([l[0], l[1]]));
            if rest.len() < 18 + l + 16 {
                break;
            }
            let Some(plain) = reader.open(n_in + 1, &rest[18..18 + l + 16]) else { return };
            n_in += 2;
            rest.drain(..18 + l + 16);
            let mut back = writer.seal(n_out, &(plain.len() as u16).to_be_bytes());
            back.extend(writer.seal(n_out + 1, &plain));
            n_out += 2;
            if tcp.write_all(&back).await.is_err() {
                return;
            }
        }
        let more = rest.len() + 1;
        if !fill(tcp, &mut rest, more).await {
            return;
        }
    }
}

/// A server of the AEAD form for one connection on a free port of the
/// loopback, with the master key of `password` ([`evp`]): it reads the salt
/// and the address, then answers `banner` and echoes.
pub async fn aead(method: Method, password: &[u8], banner: &'static [u8]) -> io::Result<(u16, Served)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let master = evp(password, method.key_len());
    let task = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.ok()?;
        let at = method.salt_len();
        let mut wire = Vec::new();
        if !fill(&mut tcp, &mut wire, at + 18).await {
            return None;
        }
        let reader = Spec::aead(method, &master, &wire[..at])?;
        let Some(len) = reader.open(0, &wire[at..at + 18]) else {
            silent(&mut tcp).await;
            return None;
        };
        let len = usize::from(u16::from_be_bytes([len[0], len[1]]));
        if !fill(&mut tcp, &mut wire, at + 18 + len + 16).await {
            return None;
        }
        let header = reader.open(1, &wire[at + 18..at + 18 + len + 16])?;
        let target = parse_address(&header).map(|(host, port, _)| format!("{host}:{port}"));
        let salt = vec![0x5au8; method.salt_len()];
        let writer = Spec::aead(method, &master, &salt)?;
        let mut answer = salt;
        answer.extend(writer.seal(0, &(banner.len() as u16).to_be_bytes()));
        answer.extend(writer.seal(1, banner));
        tcp.write_all(&answer).await.ok()?;
        let rest = wire.split_off(at + 18 + len + 16);
        echo(&mut tcp, &reader, &writer, rest).await;
        target
    });
    Ok((port, task))
}

/// How a server of Shadowsocks 2022 answers: its clock's offset from this
/// host's, and whether it names another request's salt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer {
    pub skew: i64,
    pub other_salt: bool,
}

/// The answer of a server that is right.
pub const RIGHT: Answer = Answer { skew: 0, other_salt: false };

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

/// A server of Shadowsocks 2022 for one connection on a free port of the
/// loopback, with the key `psk`: with a timestamp within 30 s it answers
/// `banner` as `answer` says, and echoes. A request whose type is not 0,
/// whose padding is not 1 to 900 bytes (no payload came with the header),
/// or with bytes after its padding, is no request: the task gives none.
pub async fn sip022(method: Method, psk: Vec<u8>, banner: &'static [u8], answer: Answer) -> io::Result<(u16, Served)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.ok()?;
        let salt_len = method.salt_len();
        let mut wire = Vec::new();
        if !fill(&mut tcp, &mut wire, salt_len + 11 + 16).await {
            return None;
        }
        let request_salt = wire[..salt_len].to_vec();
        let reader = Spec::sip022(method, &psk, &request_salt)?;
        let Some(fixed) = reader.open(0, &wire[salt_len..salt_len + 27]) else {
            silent(&mut tcp).await;
            return None;
        };
        let stamp = u64::from_be_bytes(fixed[1..9].try_into().ok()?);
        if fixed[0] != 0 || stamp.abs_diff(now()) > 30 {
            return None;
        }
        let var_len = usize::from(u16::from_be_bytes([fixed[9], fixed[10]]));
        let at = salt_len + 27;
        if !fill(&mut tcp, &mut wire, at + var_len + 16).await {
            return None;
        }
        let variable = reader.open(1, &wire[at..at + var_len + 16])?;
        let (host, port, used) = parse_address(&variable)?;
        let padding = usize::from(u16::from_be_bytes([*variable.get(used)?, *variable.get(used + 1)?]));
        if !(1..=900).contains(&padding) || variable.len() != used + 2 + padding {
            return None;
        }
        let target = Some(format!("{host}:{port}"));
        let server_salt = vec![0x5au8; salt_len];
        let writer = Spec::sip022(method, &psk, &server_salt)?;
        let named = if answer.other_salt { vec![0xa5u8; salt_len] } else { request_salt };
        let mut head = vec![1u8];
        head.extend_from_slice(&((now() as i64 + answer.skew) as u64).to_be_bytes());
        head.extend_from_slice(&named);
        head.extend_from_slice(&(banner.len() as u16).to_be_bytes());
        let mut out = server_salt;
        out.extend(writer.seal(0, &head));
        out.extend(writer.seal(1, banner));
        tcp.write_all(&out).await.ok()?;
        let rest = wire.split_off(at + var_len + 16);
        echo(&mut tcp, &reader, &writer, rest).await;
        target
    });
    Ok((port, task))
}
