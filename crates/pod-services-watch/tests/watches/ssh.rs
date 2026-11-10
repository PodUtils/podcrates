//! `ssh-banner` and `ssh-host-key` against a stand-in SSH server, and
//! `proxy-banner` against stand-in Trojan and VLESS nodes.

use base64::Engine;
use pod_testkit::stand::Io;
use sha2::{Digest, Sha224, Sha256};

use crate::common::{NAME, endpoint, fact, file, one, raw, text, tls_raw};

const BANNER: &[u8] = b"SSH-2.0-stand\r\n";

fn put(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u32).to_be_bytes());
    out.extend_from_slice(value);
}

fn packet(payload: &[u8]) -> Vec<u8> {
    let padding = 4 + (8 - (payload.len() + 9) % 8) % 8;
    let mut out = ((payload.len() + padding + 1) as u32).to_be_bytes().to_vec();
    out.push(padding as u8);
    out.extend_from_slice(payload);
    out.extend(std::iter::repeat_n(0u8, padding));
    out
}

/// An SSH server whose host key algorithms are `kinds` and whose key is
/// `key`: its banner and its KEXINIT, then the reply to the client's
/// `KEX_ECDH_INIT`.
fn serve(io: &mut dyn Io, key: &[u8], kinds: &str) {
    let mut kexinit = vec![20];
    kexinit.extend_from_slice(&[7; 16]);
    for list in [
        "curve25519-sha256",
        kinds,
        "aes128-ctr",
        "aes128-ctr",
        "hmac-sha2-256",
        "hmac-sha2-256",
        "none",
        "none",
        "",
        "",
    ] {
        put(&mut kexinit, list.as_bytes());
    }
    kexinit.extend_from_slice(&[0, 0, 0, 0, 0]);
    let mut out = BANNER.to_vec();
    out.extend(packet(&kexinit));
    if io.write_all(&out).is_err() {
        return;
    }
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let Ok(n) = io.read(&mut chunk) else { return };
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&chunk[..n]);
        let Some(end) = bytes.iter().position(|b| *b == b'\n') else { continue };
        let mut rest = &bytes[end + 1..];
        while rest.len() >= 5 {
            let length = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            if rest.len() < 4 + length {
                break;
            }
            if rest[5] == 30 {
                let mut reply = vec![31];
                put(&mut reply, key);
                put(&mut reply, &[9; 32]);
                put(&mut reply, b"no signature");
                let _ = io.write_all(&packet(&reply));
                return;
            }
            rest = &rest[4 + length..];
        }
    }
}

fn fingerprint(key: &[u8]) -> String {
    format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(key)))
}

#[tokio::test]
async fn ssh_banner_wants_a_line_of_ssh() {
    pod_testkit::need_loopback!();
    let w = "[[watch]]\nkind = \"ssh-banner\"\nendpoint = \"ssh\"\n";
    let good = raw(|io| {
        let _ = io.write_all(b"a line before the banner\r\nSSH-2.0-stand\r\n");
        let _ = io.read(&mut [0u8; 64]);
    });
    let report = one(&file("b1", &(endpoint("ssh", "ssh", "127.0.0.1", good.port(), "") + w))).await;
    assert!(report.holds() && text(&report).contains("SSH-2.0-stand"), "{}", text(&report));
    let bad = raw(|io| {
        let _ = io.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
    });
    let report = one(&file("b2", &(endpoint("ssh", "ssh", "127.0.0.1", bad.port(), "") + w))).await;
    assert!(!report.holds(), "{}", text(&report));
}

#[tokio::test]
async fn ssh_host_key_is_the_key_that_the_server_offers() {
    pod_testkit::need_loopback!();
    let key = b"\x00\x00\x00\x0bssh-ed25519 a stand-in key".to_vec();
    let offered = key.clone();
    let stand = raw(move |io| serve(io, &offered, "ssh-ed25519"));
    let ep = endpoint("ssh", "ssh", "127.0.0.1", stand.port(), "");
    let w = "[[watch]]\nkind = \"ssh-host-key\"\nendpoint = \"ssh\"\nfact = \"key\"\n";
    let good = ep.clone() + &fact("key", &format!("\"{}\"", fingerprint(&key))) + w;
    let report = one(&file("k1", &good)).await;
    assert!(report.holds(), "{}", text(&report));
    let bad = ep + &fact("key", &format!("\"{}\"", fingerprint(b"another key"))) + w;
    let report = one(&file("k2", &bad)).await;
    assert!(text(&report).contains(&format!("the service gives {}", fingerprint(&key))), "{}", text(&report));
}

/// A Trojan node over TLS: the hash of `password`, a CONNECT, then the
/// banner; a wrong hash closes.
fn trojan(io: &mut dyn Io, password: &str) {
    let hash: String = Sha224::digest(password.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    let mut got = Vec::new();
    let mut chunk = [0u8; 512];
    while !got.ends_with(b"\r\n") || got.len() < 58 {
        match io.read(&mut chunk) {
            Ok(n) if n > 0 => got.extend_from_slice(&chunk[..n]),
            _ => return,
        }
    }
    if got[..56] != *hash.as_bytes() || got[58] != 0x01 || !got.windows(10).any(|w| w == b"github.com") {
        return;
    }
    let _ = io.write_all(BANNER);
    let _ = io.read(&mut chunk);
}

fn node(link: &str) -> String {
    fact("link", &format!("\"{link}\""))
        + "[[watch]]\nkind = \"proxy-banner\"\nfact = \"link\"\ntarget = \"github-ssh\"\n"
}

#[tokio::test]
async fn proxy_banner_speaks_trojan_over_tls() {
    pod_testkit::need_loopback!();
    let stand = tls_raw(|io| trojan(io, "a made-up password"));
    let link = format!("trojan://a made-up password@{NAME}:{}?security=tls&sni={NAME}", stand.port());
    let report = one(&file("p1", &node(&link))).await;
    assert!(report.holds() && text(&report).contains("SSH-2.0-stand"), "{}", text(&report));
    let link = format!("trojan://another password@{NAME}:{}?security=tls&sni={NAME}", stand.port());
    let report = one(&file("p2", &node(&link))).await;
    assert!(text(&report).contains("closed before a banner"), "{}", text(&report));
}

#[tokio::test]
async fn proxy_banner_speaks_vless_and_skips_another_transport() {
    pod_testkit::need_loopback!();
    let stand = raw(|io| {
        // The version, the UUID, no addon, CONNECT, the port, a name and its length.
        let mut head = [0u8; 23];
        if io.read_exact(&mut head).is_err() || head[0] != 0 || head[17] != 0 || head[18] != 1 || head[21] != 2 {
            return;
        }
        let port = u16::from_be_bytes([head[19], head[20]]);
        let mut host = vec![0u8; usize::from(head[22])];
        if io.read_exact(&mut host).is_err() || port != 22 || host != b"github.com" {
            return;
        }
        let _ = io.write_all(&[0, 0]);
        let _ = io.write_all(BANNER);
        let _ = io.read(&mut [0u8; 64]);
    });
    let uuid = "00000000-0000-4000-8000-000000000001";
    let report =
        one(&file("p3", &node(&format!("vless://{uuid}@127.0.0.1:{}?security=none&type=tcp", stand.port())))).await;
    assert!(report.holds() && text(&report).contains("SSH-2.0-stand"), "{}", text(&report));
    let report = one(&file("p4", &node(&format!("vless://{uuid}@127.0.0.1:1?security=tls&type=xhttp")))).await;
    assert!(report.holds() && text(&report).starts_with("skipped p4 proxy-banner"), "{}", text(&report));
}
