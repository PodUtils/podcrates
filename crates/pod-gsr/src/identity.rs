//! The address and the SRP password that a secret gives.
//!
//! The relay names a secret by the first 16 bytes of `SHA256(addr-key ++
//! secret)`. The password of the SRP handshake is the lowercase hex of
//! `SHA256(srp-key ++ secret)`, cut to `password-hex-chars`. The shard is the
//! letter of the sum of the address bytes modulo `shard-count`, in front of
//! the domain (`lib/gsocket-util.c` of gsocket: `GS_ADDR_sec2addr`,
//! `GS_ADDR_get_hostname_id`).

use std::fmt;

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::settings::Settings;

/// The length of the address of a secret, in bytes.
pub const ADDR_LEN: usize = 16;

/// The address of a secret, its SRP password and its place on the relay.
pub struct Identity {
    addr: [u8; ADDR_LEN],
    password: Zeroizing<String>,
}

impl Identity {
    /// The identity that `secret` gives. The caller checks the length of the
    /// secret first; [`crate::open`] does.
    pub fn new(secret: &str, settings: &Settings) -> Identity {
        let addr = digest(&settings.addr_key, secret);
        let hex = hex_lower(&digest(&settings.srp_key, secret));
        let keep = settings.password_chars.min(hex.len());
        let mut address = [0u8; ADDR_LEN];
        address.copy_from_slice(&addr[..ADDR_LEN]);
        Identity { addr: address, password: Zeroizing::new(hex[..keep].to_string()) }
    }

    /// The 16 bytes that name the secret on the relay.
    pub fn addr(&self) -> &[u8; ADDR_LEN] {
        &self.addr
    }

    /// The address as lowercase hex.
    pub fn addr_hex(&self) -> String {
        hex_lower(&self.addr)
    }

    /// The password of the SRP handshake. It is secret material: do not print
    /// it.
    pub fn password(&self) -> &str {
        &self.password
    }

    /// The password in the text that the TLS handshake uses.
    pub fn srp_secret(&self, settings: &Settings) -> Zeroizing<String> {
        Zeroizing::new(format!("{}{}{}", settings.secret_prefix, self.password.as_str(), settings.secret_suffix))
    }

    /// The shard letter: `'a' + the sum of the address bytes % shard-count`.
    ///
    /// [`Settings::of`] refuses a count outside 1 to 26. A count that a
    /// caller wrote into the fields is held to the same range here, so the
    /// letter is always one of `a` to `z`.
    pub fn shard(&self, settings: &Settings) -> char {
        let sum: u32 = self.addr.iter().map(|byte| u32::from(*byte)).sum();
        let count = u32::from(settings.shards.clamp(1, crate::settings::MAX_SHARDS));
        let index = u8::try_from(sum % count).expect("an index below 26");
        char::from(b'a' + index)
    }

    /// The name to dial: the shard letter and the domain, or the domain alone
    /// for a stand-in that serves each address.
    pub fn host(&self, settings: &Settings) -> String {
        if settings.sharded { format!("{}.{}", self.shard(settings), settings.domain) } else { settings.domain.clone() }
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity").field("addr", &self.addr_hex()).finish_non_exhaustive()
    }
}

fn digest(key: &str, secret: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(key.as_bytes());
    hash.update(secret.as_bytes());
    hash.finalize().into()
}

fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::of_data().expect("the data file")
    }

    #[test]
    fn a_secret_gives_the_address_and_the_password_that_gsocket_gives() {
        let settings = settings();
        let identity = Identity::new("testsecret123", &settings);
        assert_eq!(identity.addr_hex(), "5b1d517723aea09d4bd34b7798c50ac6");
        assert_eq!(identity.password(), "23b110ed3d783232124b4bc9ed1a12ac2");
        assert_eq!(identity.srp_secret(&settings).as_str(), "Blah.23b110ed3d783232124b4bc9ed1a12ac2.blubb-SRPSEC");
        assert_eq!(identity.shard(&settings), 'l');
        assert_eq!(identity.host(&settings), "l.gs.thc.org");
    }

    #[test]
    fn the_debug_line_names_the_address_and_not_the_password() {
        let settings = settings();
        let line = format!("{:?}", Identity::new("testsecret123", &settings));
        assert!(line.contains("5b1d517723aea09d4bd34b7798c50ac6"));
        assert!(!line.contains("23b110ed"));
    }

    #[test]
    fn each_shard_letter_is_a_lower_case_letter() {
        let mut settings = settings();
        for shards in [0, 1, 26, 27, 200, 255] {
            settings.shards = shards;
            for secret in ["testsecret123", "a", "pod-gsr", "zzzzzzzzzzzzzzzz"] {
                let letter = Identity::new(secret, &settings).shard(&settings);
                assert!(letter.is_ascii_lowercase(), "{shards} shards, {secret:?}: {letter:?}");
            }
        }
    }

    #[test]
    fn a_fixed_host_is_the_domain_alone() {
        let mut settings = settings();
        settings.sharded = false;
        settings.domain = "127.0.0.1".into();
        let identity = Identity::new("testsecret123", &settings);
        assert_eq!(identity.host(&settings), "127.0.0.1");
    }
}
