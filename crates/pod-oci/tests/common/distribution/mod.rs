//! The parts of the test target `distribution` (T-353, call C151): the
//! CNCF distribution registry on 127.0.0.1 ([`server`]), the certificates
//! of its TLS and of its tokens ([`tls`]), the push of an index of two
//! platforms over the distribution API ([`push`]), and the token server of
//! the test ([`token`]).
//!
//! The login is a made-up value. `.github/secrets-allow.txt` tells why it
//! is not a credential.

pub mod push;
pub mod server;
pub mod tls;
pub mod token;

/// The user of the made-up login.
pub const USER: &str = "pod-oci-test";
/// The password of the made-up login.
pub const PASSWORD: &str = "made-up-test-login-of-pod-oci";
/// The bcrypt hash of [`PASSWORD`] (cost 5), as distribution reads an
/// htpasswd file. Made by libxcrypt and checked by PHP's
/// `password_verify`: two implementations that this crate did not write.
pub const PASSWORD_BCRYPT: &str = "$2b$05$podocitestsaltnotsecre7qJ32PfT6PWoggpyVu3xbkf2MIhI1Hm";
/// The `iss` of each token, and the `issuer` of the registry.
pub const ISSUER: &str = "pod-oci-test-issuer";
/// The `aud` of each token, and the `service` of the registry.
pub const SERVICE: &str = "pod-oci-test-registry";
/// The address of each server, which the leaf certificate names.
pub const AT: &str = "127.0.0.1";

/// A port of the loopback that is free now.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind((AT, 0)).unwrap().local_addr().unwrap().port()
}

/// `user:password` as the value of a `Basic` header.
pub fn basic(user: &str, password: &str) -> String {
    use base64::Engine as _;
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}")))
}

/// The value of `name` in the query of `path`, percent-decoded. A name
/// that occurs more than once gives each value.
pub fn query_values(path: &str, name: &str) -> Vec<String> {
    let Some((_, query)) = path.split_once('?') else { return Vec::new() };
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(k, _)| *k == name)
        .map(|(_, v)| percent_decode(v))
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
        match (bytes[i], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
