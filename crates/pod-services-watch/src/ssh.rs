//! `ssh-banner` (the server sends an SSH banner) and `ssh-host-key` (the
//! SHA-256 fingerprint of each host key that the server offers). The key
//! comes from the first exchange of keys (curve25519-sha256, RFC 8731): the
//! server sends its key in its reply, before any check of a signature. The
//! watch reads which key the server offers; it does not prove that the
//! server holds the private key.

use base64::Engine;
use pod_core::BoxedIo;
use pod_core::entropy::{Entropy, OsEntropy};
use pod_services::{Service, Watch};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::args::{Args, named, value_texts};
use crate::ws::banner_line;
use crate::{Checked, Finding, Net, Outcome};

/// The banner of the client (RFC 4253, 4.2).
const BANNER: &[u8] = b"SSH-2.0-pod-services-watch\r\n";
/// The lines that a server may send before its banner.
const MAX_LINES: usize = 16;
/// The largest packet read before the keys: a reply holds a host key of
/// RSA of 16384 bits at most.
const MAX_PACKET: usize = 35_000;
const KEXINIT: u8 = 20;
const KEX_ECDH_INIT: u8 = 30;
const KEX_ECDH_REPLY: u8 = 31;
/// The messages that the server may send at any time, skipped: DISCONNECT
/// is a failure.
const DISCONNECT: u8 = 1;
const SKIPPED: [u8; 3] = [2, 3, 4];
/// One exchange for each kind of host key, so that the server offers each
/// key that it has.
const KEY_KINDS: [&[&str]; 3] = [
    &["ssh-ed25519"],
    &["ecdsa-sha2-nistp256", "ecdsa-sha2-nistp384", "ecdsa-sha2-nistp521"],
    &["rsa-sha2-512", "rsa-sha2-256", "ssh-rsa"],
];

/// `ssh-banner`: after the banner of the client, the endpoint sends a line
/// that starts with `SSH-`.
pub async fn banner(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let endpoint = Args::new(service, watch).endpoint()?;
    let mut io = net.tcp(endpoint.host(), endpoint.port()).await?;
    // A server may wait for the client's banner first (Pinggy does).
    io.write_all(BANNER).await.map_err(|e| Finding::failed(format!("the banner of the client: {e}")))?;
    let (line, _) = read_banner(&mut io, net).await?;
    Ok(Outcome { seen: vec![format!("{} sent {line:?}", endpoint.authority())], findings: Vec::new(), skipped: None })
}

/// `ssh-host-key`: each fingerprint of the fact is the SHA-256 of a host
/// key that the endpoint offers.
pub async fn host_key(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let (endpoint, fact) = (args.endpoint()?, args.fact("fact")?);
    let mut offered = Vec::new();
    for kinds in KEY_KINDS {
        let mut io = net.tcp(endpoint.host(), endpoint.port()).await?;
        let exchange = tokio::time::timeout(net.limit, offer(&mut io, net, kinds)).await;
        match exchange
            .map_err(|_| Finding::failed(format!("{} sent no host key within {:?}", endpoint.authority(), net.limit)))?
        {
            Ok(Some(key)) => offered.push(fingerprint(&key)),
            Ok(None) => {}
            Err(finding) => return Err(finding),
        }
    }
    let file = value_texts(fact.value());
    let mut out = Outcome::default();
    if file.iter().all(|f| offered.contains(f)) {
        out.saw(format!("{} offers {}", endpoint.authority(), offered.join(", ")));
    } else {
        out.found(Finding::moved(named(fact), file.join(", "), offered.join(", ")));
    }
    Ok(out)
}

/// The fingerprint of a host key as OpenSSH writes it.
pub fn fingerprint(key: &[u8]) -> String {
    format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(key)))
}

/// The banner of the server and the bytes after its line.
async fn read_banner(io: &mut BoxedIo, net: &Net) -> Result<(String, Vec<u8>), Finding> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 1024];
    for _ in 0..MAX_LINES {
        loop {
            if let Some(end) = bytes.iter().position(|b| *b == b'\n') {
                if bytes.starts_with(b"SSH-") {
                    let line = banner_line(&bytes)?.unwrap_or_default();
                    return Ok((line, bytes.split_off(end + 1)));
                }
                bytes.drain(..=end);
                break;
            }
            let n = tokio::time::timeout(net.limit, io.read(&mut chunk))
                .await
                .map_err(|_| Finding::failed("the server sent no banner"))?
                .map_err(|e| Finding::failed(format!("the read of the banner: {e}")))?;
            if n == 0 {
                return Err(Finding::failed("the server closed before its banner"));
            }
            bytes.extend_from_slice(&chunk[..n]);
            if bytes.len() > MAX_PACKET {
                return Err(Finding::failed("the server sent no line of a banner"));
            }
        }
    }
    Err(Finding::failed("the server sent no SSH banner"))
}

/// The host key of one exchange that offers the host key algorithms
/// `kinds`: none when the server has no key of them.
async fn offer(io: &mut BoxedIo, net: &Net, kinds: &[&str]) -> Result<Option<Vec<u8>>, Finding> {
    let failed = |e: std::io::Error| Finding::failed(format!("the exchange of keys: {e}"));
    io.write_all(BANNER).await.map_err(failed)?;
    let (_, mut buffered) = read_banner(io, net).await?;
    io.write_all(&packet(&kexinit(kinds)?)).await.map_err(failed)?;
    let theirs = read_packet(io, &mut buffered, KEXINIT).await?;
    let lists = name_lists(&theirs)?;
    let keys = lists.get(1).cloned().unwrap_or_default();
    if !kinds.iter().any(|k| keys.split(',').any(|s| s == *k)) {
        return Ok(None);
    }
    let mut ecdh = vec![KEX_ECDH_INIT];
    let mut public = [0u8; 32];
    OsEntropy.fill(&mut public).map_err(|e| Finding::failed(e.to_string()))?;
    put_string(&mut ecdh, &public);
    io.write_all(&packet(&ecdh)).await.map_err(failed)?;
    let reply = read_packet(io, &mut buffered, KEX_ECDH_REPLY).await?;
    let (key, _) = get_string(&reply, 1)?;
    Ok(Some(key.to_vec()))
}

fn kexinit(kinds: &[&str]) -> Result<Vec<u8>, Finding> {
    let mut out = vec![KEXINIT];
    let mut cookie = [0u8; 16];
    OsEntropy.fill(&mut cookie).map_err(|e| Finding::failed(e.to_string()))?;
    out.extend_from_slice(&cookie);
    let ciphers = "chacha20-poly1305@openssh.com,aes256-gcm@openssh.com,aes128-ctr";
    let macs = "hmac-sha2-256-etm@openssh.com,hmac-sha2-256";
    let kinds = kinds.join(",");
    for list in
        ["curve25519-sha256,curve25519-sha256@libssh.org", &kinds, ciphers, ciphers, macs, macs, "none", "none", "", ""]
    {
        put_string(&mut out, list.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&0u32.to_be_bytes());
    Ok(out)
}

/// A packet with no encryption and no MAC (RFC 4253, 6), padded to 8.
fn packet(payload: &[u8]) -> Vec<u8> {
    let mut padding = 8 - (payload.len() + 5) % 8;
    if padding < 4 {
        padding += 8;
    }
    let mut out = ((payload.len() + padding + 1) as u32).to_be_bytes().to_vec();
    out.push(padding as u8);
    out.extend_from_slice(payload);
    out.extend(std::iter::repeat_n(0u8, padding));
    out
}

/// The payload of the next packet of type `want`, after the messages that
/// the server may send at any time.
async fn read_packet(io: &mut BoxedIo, buffered: &mut Vec<u8>, want: u8) -> Result<Vec<u8>, Finding> {
    loop {
        while buffered.len() < 4 || buffered.len() < 4 + be32(buffered) {
            if buffered.len() >= 4 && be32(buffered) > MAX_PACKET {
                return Err(Finding::failed(format!("a packet of {} bytes before the keys", be32(buffered))));
            }
            let mut chunk = [0u8; 4096];
            let n = io.read(&mut chunk).await.map_err(|e| Finding::failed(format!("the exchange of keys: {e}")))?;
            if n == 0 {
                return Err(Finding::failed("the server closed during the exchange of keys"));
            }
            buffered.extend_from_slice(&chunk[..n]);
        }
        let length = be32(buffered);
        let whole: Vec<u8> = buffered.drain(..4 + length).collect();
        let padding = usize::from(*whole.get(4).unwrap_or(&0));
        let payload = whole.get(5..(4 + length).saturating_sub(padding)).unwrap_or_default().to_vec();
        match payload.first() {
            Some(kind) if *kind == want => return Ok(payload),
            Some(kind) if SKIPPED.contains(kind) => continue,
            Some(&DISCONNECT) => {
                let why =
                    get_string(&payload, 5).map(|(s, _)| String::from_utf8_lossy(s).into_owned()).unwrap_or_default();
                return Err(Finding::failed(format!("the server ended the exchange of keys: {why}")));
            }
            Some(kind) => return Err(Finding::failed(format!("the server sent the message {kind}, not {want}"))),
            None => return Err(Finding::failed("the server sent an empty packet")),
        }
    }
}

fn be32(bytes: &[u8]) -> usize {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize
}

fn put_string(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u32).to_be_bytes());
    out.extend_from_slice(value);
}

/// The string at `at` of `payload`, and the place after it.
fn get_string(payload: &[u8], at: usize) -> Result<(&[u8], usize), Finding> {
    let short = || Finding::failed("a message of the server ends inside a string");
    let length = payload.get(at..at + 4).map(be32).ok_or_else(short)?;
    let value = payload.get(at + 4..at + 4 + length).ok_or_else(short)?;
    Ok((value, at + 4 + length))
}

/// The ten name-lists of a KEXINIT.
fn name_lists(payload: &[u8]) -> Result<Vec<String>, Finding> {
    let mut at = 17;
    let mut out = Vec::new();
    for _ in 0..10 {
        let (value, next) = get_string(payload, at)?;
        out.push(String::from_utf8_lossy(value).into_owned());
        at = next;
    }
    Ok(out)
}
