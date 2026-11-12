//! HTTP Digest authentication (RFC 7616) for a `CONNECT`: the challenge of
//! a `Proxy-Authenticate` header, and the answer with the user and the
//! password of the proxy URL. The algorithms are MD5, MD5-sess, SHA-256 and
//! SHA-256-sess, with `qop=auth` when the proxy offers it.

use pod_core::entropy::{Entropy, NoRandom};
use sha2::Digest as _;

/// MD5 (RFC 1321), for the `MD5` algorithm of Digest: the one copy of
/// `pod-turn`, which the long-term key of STUN takes too.
pub use pod_turn::md5::md5;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A Digest challenge of a `Proxy-Authenticate` header.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    /// `MD5`, `MD5-sess`, `SHA-256` or `SHA-256-sess`.
    pub algorithm: String,
    /// `auth` when the server offers it.
    pub qop: Option<String>,
}

/// Split a list of a header value at the commas that are not in quotes.
fn split_list(value: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let (mut start, mut in_quotes, mut escaped) = (0, false, false);
    for (i, c) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if in_quotes && c == '\\' {
            escaped = true;
        } else if c == '"' {
            in_quotes = !in_quotes;
        } else if c == ',' && !in_quotes {
            items.push(&value[start..i]);
            start = i + 1;
        }
    }
    items.push(&value[start..]);
    items
}

/// The challenges of a `Proxy-Authenticate` value: the scheme and the
/// parameters of each. One value can hold several challenges:
/// `Basic realm="a", Digest realm="b", nonce="n"` (RFC 9110 section 11.6.1).
fn challenges(value: &str) -> Vec<(&str, Vec<&str>)> {
    let mut out: Vec<(&str, Vec<&str>)> = Vec::new();
    for item in split_list(value) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let end = item.find([' ', '\t', '=']).unwrap_or(item.len());
        let after = item[end..].trim_start();
        if after.starts_with('=') {
            // A parameter of the challenge before.
            if let Some((_, params)) = out.last_mut() {
                params.push(item);
            }
        } else {
            let params = if after.is_empty() { Vec::new() } else { vec![after] };
            out.push((&item[..end], params));
        }
    }
    out
}

/// The Digest challenge of a `Proxy-Authenticate` value. The scheme name
/// has any case, and the value can hold other challenges before or after
/// it. `None` when the value has no Digest challenge, or the challenge
/// names an algorithm that this module does not have.
pub fn parse_challenge(value: &str) -> Option<Challenge> {
    let (_, params) = challenges(value).into_iter().find(|(scheme, _)| scheme.eq_ignore_ascii_case("digest"))?;
    parse_digest_params(&params.join(", "))
}

/// The Digest challenge of the `Proxy-Authenticate` headers of a response
/// head: the first one that this module answers.
pub fn challenge_in(head: &str) -> Option<Challenge> {
    head.split("\r\n").skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case("proxy-authenticate").then(|| parse_challenge(value)).flatten()
    })
}

/// Parse the parameters `a="b", c=d, ...` of a Digest challenge.
fn parse_digest_params(rest: &str) -> Option<Challenge> {
    let mut c = Challenge { algorithm: "MD5".into(), ..Challenge::default() };
    let mut chars = rest.chars().peekable();
    loop {
        while matches!(chars.peek(), Some(' ' | ',' | '\t')) {
            chars.next();
        }
        let key: String = chars.by_ref().take_while(|ch| *ch != '=').collect();
        if key.is_empty() {
            break;
        }
        let value: String = if chars.peek() == Some(&'"') {
            chars.next();
            let mut v = String::new();
            while let Some(ch) = chars.next() {
                match ch {
                    '\\' => {
                        if let Some(n) = chars.next() {
                            v.push(n);
                        }
                    }
                    '"' => break,
                    other => v.push(other),
                }
            }
            v
        } else {
            chars.by_ref().take_while(|ch| *ch != ',').collect::<String>().trim().to_string()
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "realm" => c.realm = value,
            "nonce" => c.nonce = value,
            "opaque" => c.opaque = Some(value),
            "algorithm" => c.algorithm = value,
            "qop" if value.split(',').any(|q| q.trim() == "auth") => {
                c.qop = Some("auth".into());
            }
            _ => {}
        }
    }
    (!c.nonce.is_empty() && ALGORITHMS.iter().any(|a| a.eq_ignore_ascii_case(&c.algorithm))).then_some(c)
}

/// The algorithms of Digest that this module answers.
pub const ALGORITHMS: [&str; 4] = ["MD5", "MD5-sess", "SHA-256", "SHA-256-sess"];

fn digest_hex(algorithm: &str, text: &str) -> String {
    if algorithm.to_ascii_uppercase().starts_with("SHA-256") {
        hex(&sha2::Sha256::digest(text.as_bytes()))
    } else {
        hex(&md5(text.as_bytes()))
    }
}

/// The value of `Proxy-Authorization` for `challenge`, `method` and
/// `uri`, with the client nonce `cnonce` and the count `nc` (1 for the
/// first request).
pub fn authorization(
    challenge: &Challenge,
    user: &str,
    password: &str,
    method: &str,
    uri: &str,
    cnonce: &str,
    nc: u32,
) -> String {
    let alg = &challenge.algorithm;
    let mut ha1 = digest_hex(alg, &format!("{user}:{}:{password}", challenge.realm));
    if alg.to_ascii_lowercase().ends_with("-sess") {
        ha1 = digest_hex(alg, &format!("{ha1}:{}:{cnonce}", challenge.nonce));
    }
    let ha2 = digest_hex(alg, &format!("{method}:{uri}"));
    let nc = format!("{nc:08x}");
    let response = match &challenge.qop {
        Some(qop) => digest_hex(alg, &format!("{ha1}:{}:{nc}:{cnonce}:{qop}:{ha2}", challenge.nonce)),
        None => digest_hex(alg, &format!("{ha1}:{}:{ha2}", challenge.nonce)),
    };
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut out = format!(
        "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", algorithm={}, response=\"{response}\"",
        quote(user),
        quote(&challenge.realm),
        quote(&challenge.nonce),
        quote(uri),
        challenge.algorithm,
    );
    if let Some(qop) = &challenge.qop {
        out.push_str(&format!(", qop={qop}, nc={nc}, cnonce=\"{}\"", quote(cnonce)));
    }
    if let Some(opaque) = &challenge.opaque {
        out.push_str(&format!(", opaque=\"{}\"", quote(opaque)));
    }
    out
}

/// A client nonce: 16 random bytes of `entropy` in hex. A source that gives
/// none is an error, never a weaker nonce.
pub fn cnonce(entropy: &mut dyn Entropy) -> Result<String, NoRandom> {
    let mut bytes = [0u8; 16];
    entropy.fill(&mut bytes)?;
    Ok(hex(&bytes))
}

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;
