//! The one made-up RSA key of the fake GitHub, and RS256 and HMAC-SHA256
//! over it. The key is a test value in the repository (`made-up-key.pem`,
//! listed in `.github/secrets-allow.txt`): the test of a consumer signs
//! its App JWT with it, and each runner of a JIT configuration signs its
//! client assertion with it. No real GitHub knows it.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RSA_PKCS1_SHA256, RsaKeyPair, RsaPublicKeyComponents};
use serde_json::{Value, json};

/// The made-up key, PKCS#1 in PEM, the form of the key of a GitHub App.
pub const MADE_UP_KEY_PEM: &str = include_str!("made-up-key.pem");

/// An RSA key: the pair of ring and the numbers of PKCS#1.
pub struct Key {
    pair: RsaKeyPair,
    /// version, n, e, d, p, q, dp, dq, qinv, each big-endian with no
    /// leading zero byte.
    numbers: Vec<Vec<u8>>,
}

impl Key {
    /// The made-up key of the fake.
    pub fn made_up() -> Key {
        Key::from_pem(MADE_UP_KEY_PEM).expect("the made-up key of the fake parses")
    }

    /// A PKCS#1 key in PEM (`BEGIN RSA PRIVATE KEY`).
    pub fn from_pem(pem: &str) -> Result<Key, String> {
        let body: String = pem.lines().filter(|l| !l.starts_with("-----")).map(str::trim).collect();
        let der = STANDARD.decode(body).map_err(|e| format!("the PEM body: {e}"))?;
        let pair = RsaKeyPair::from_der(&der).map_err(|e| format!("the PKCS#1 key: {e}"))?;
        let numbers = pkcs1_numbers(&der).ok_or("the PKCS#1 key: not nine integers")?;
        Ok(Key { pair, numbers })
    }

    /// An RS256 JWT of `claims`.
    pub fn jwt(&self, claims: &Value) -> String {
        let head = URL_SAFE_NO_PAD.encode(json!({ "alg": "RS256", "typ": "JWT" }).to_string());
        let signed = format!("{head}.{}", URL_SAFE_NO_PAD.encode(claims.to_string()));
        let mut sig = vec![0u8; self.pair.public().modulus_len()];
        self.pair
            .sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), signed.as_bytes(), &mut sig)
            .expect("an RS256 signature");
        format!("{signed}.{}", URL_SAFE_NO_PAD.encode(sig))
    }

    /// The claims of `token` when it is an RS256 JWT that this key signed.
    /// The caller checks the claims.
    pub fn verify(&self, token: &str) -> Option<Value> {
        let (signed, sig) = token.trim().rsplit_once('.')?;
        let (head, claims) = signed.split_once('.')?;
        let head: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(head).ok()?).ok()?;
        if head["alg"] != "RS256" {
            return None;
        }
        let public = RsaPublicKeyComponents { n: &self.numbers[1], e: &self.numbers[2] };
        public.verify(&RSA_PKCS1_2048_8192_SHA256, signed.as_bytes(), &URL_SAFE_NO_PAD.decode(sig).ok()?).ok()?;
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).ok()?).ok()
    }

    /// The key in the form of the runner's `.credentials_rsaparams`. .NET
    /// takes each number big-endian with no sign byte, `d` as long as the
    /// modulus, and the five numbers of the CRT half as long.
    pub fn rsaparams(&self) -> String {
        let k = self.numbers[1].len();
        let padded = |i: usize, len: usize| {
            let raw = &self.numbers[i];
            let mut v = vec![0u8; len.saturating_sub(raw.len())];
            v.extend_from_slice(raw);
            STANDARD.encode(v)
        };
        json!({
            "modulus": padded(1, k), "exponent": STANDARD.encode(&self.numbers[2]), "d": padded(3, k),
            "p": padded(4, k / 2), "q": padded(5, k / 2), "dp": padded(6, k / 2), "dq": padded(7, k / 2),
            "inverseQ": padded(8, k / 2),
        })
        .to_string()
    }
}

/// The nine integers of a PKCS#1 `RSAPrivateKey` (RFC 8017, A.1.2), each
/// with no leading zero byte.
fn pkcs1_numbers(der: &[u8]) -> Option<Vec<Vec<u8>>> {
    let (tag, mut rest, _) = tlv(der)?;
    if tag != 0x30 {
        return None;
    }
    let mut out = Vec::new();
    while !rest.is_empty() {
        let (tag, value, after) = tlv(rest)?;
        if tag != 0x02 {
            return None;
        }
        let start = value.iter().position(|b| *b != 0).unwrap_or(value.len());
        out.push(value[start..].to_vec());
        rest = after;
    }
    (out.len() == 9).then_some(out)
}

/// One DER element: its tag, its value and the bytes after it.
fn tlv(b: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, b) = b.split_first()?;
    let (&first, mut b) = b.split_first()?;
    let len = match first {
        n if n < 0x80 => usize::from(n),
        n => {
            let count = usize::from(n & 0x7f);
            if count == 0 || count > 4 || b.len() < count {
                return None;
            }
            let (bytes, after) = b.split_at(count);
            b = after;
            bytes.iter().fold(0usize, |acc, x| (acc << 8) | usize::from(*x))
        }
    };
    (b.len() >= len).then(|| (tag, &b[..len], &b[len..]))
}

/// The value of `X-Hub-Signature-256` for `body` and the webhook secret
/// `secret`: `sha256=` and the HMAC-SHA256 in lower-case hex.
pub fn hub_signature(secret: &str, body: &[u8]) -> String {
    let tag = ring::hmac::sign(&ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes()), body);
    format!("sha256={}", hex(tag.as_ref()))
}

/// `n` random bytes in lower-case hex.
pub(crate) fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    SystemRandom::new().fill(&mut b).expect("the random source of the system");
    hex(&b)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Bytes equal in a time that does not depend on where they differ.
pub(crate) fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers of the runner file are the numbers of the pair of ring:
    /// a signature of the pair checks with the modulus and the exponent.
    #[test]
    fn the_rsaparams_hold_the_key_of_the_pair() {
        let key = Key::made_up();
        let params: Value = serde_json::from_str(&key.rsaparams()).unwrap();
        let len = |k: &str| STANDARD.decode(params[k].as_str().unwrap()).unwrap().len();
        assert_eq!(len("modulus"), 256);
        assert_eq!(len("d"), 256);
        for k in ["p", "q", "dp", "dq", "inverseQ"] {
            assert_eq!(len(k), 128, "{k}");
        }
        let n = STANDARD.decode(params["modulus"].as_str().unwrap()).unwrap();
        let e = STANDARD.decode(params["exponent"].as_str().unwrap()).unwrap();
        assert_eq!(e, [1, 0, 1]);
        let token = key.jwt(&json!({ "iss": "1" }));
        let (signed, sig) = token.rsplit_once('.').unwrap();
        let sig = URL_SAFE_NO_PAD.decode(sig).unwrap();
        let n = &n[n.iter().position(|b| *b != 0).unwrap()..];
        RsaPublicKeyComponents { n, e: &e[..] }.verify(&RSA_PKCS1_2048_8192_SHA256, signed.as_bytes(), &sig).unwrap();
    }

    #[test]
    fn a_changed_or_unsigned_token_does_not_verify() {
        let key = Key::made_up();
        let token = key.jwt(&json!({ "iss": "7" }));
        assert_eq!(key.verify(&token).unwrap()["iss"], "7");
        let (head, rest) = token.split_once('.').unwrap();
        let (_, sig) = rest.rsplit_once('.').unwrap();
        let forged = format!("{head}.{}.{sig}", URL_SAFE_NO_PAD.encode(r#"{"iss":"8"}"#));
        assert!(key.verify(&forged).is_none());
        let none = URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#);
        assert!(key.verify(&format!("{none}.{}.", URL_SAFE_NO_PAD.encode(r#"{"iss":"7"}"#))).is_none());
        for bad in ["", "a", "a.b", "a.b.c"] {
            assert!(key.verify(bad).is_none(), "{bad}");
        }
    }

    /// The example of GitHub's documents on the validation of webhook
    /// deliveries.
    #[test]
    fn the_hub_signature_is_the_one_of_github_s_example() {
        assert_eq!(
            hub_signature("It's a Secret to Everybody", b"Hello, World!"),
            "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17"
        );
    }
}
