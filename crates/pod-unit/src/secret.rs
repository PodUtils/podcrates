//! No secret in a unit. Each user of a host may read a unit of systemd, a
//! plist, the command line of a service and the process list, so a word of a
//! unit that looks like a secret is refused before any form is rendered.
//! The value of a secret goes in a private file that the host resolves at the
//! start ([`crate::Secret`]: `LoadCredential=` of systemd, else the path in a
//! variable), or stays with a broker that swaps a placeholder
//! ([`crate::Env::placeholder`]).
//!
//! The rules, each with its name in the refusal: the prefix of a known
//! token, a private key, a password in a URL, a key that names a secret, a
//! flag that takes a secret, and a long random word. The refusal names the
//! place of the word and the rule, never the word.

use pod_core::Failure;

use crate::model::{Unit, usage};

/// The prefixes of known tokens, each with the length of the rest.
const PREFIXES: &[(&str, usize, &str)] = &[
    ("ghp_", 30, "a GitHub token"),
    ("gho_", 30, "a GitHub token"),
    ("ghu_", 30, "a GitHub token"),
    ("ghs_", 30, "a GitHub token"),
    ("ghr_", 30, "a GitHub token"),
    ("github_pat_", 30, "a GitHub token"),
    ("glpat-", 20, "a GitLab token"),
    ("xoxb-", 20, "a Slack token"),
    ("xoxp-", 20, "a Slack token"),
    ("xapp-", 20, "a Slack token"),
    ("sk-", 20, "an API key"),
    ("AKIA", 16, "an AWS access key"),
    ("ASIA", 16, "an AWS access key"),
    ("AIza", 30, "a Google API key"),
    ("npm_", 30, "an npm token"),
    ("pypi-", 30, "a PyPI token"),
    ("hf_", 30, "a Hugging Face token"),
    ("dop_v1_", 30, "a DigitalOcean token"),
    ("eyJ", 30, "a token of base64 JSON (a JWT, a tunnel token)"),
];

/// The words that name a secret in a key or a flag.
const SECRET_WORDS: &[&str] =
    &["TOKEN", "SECRET", "PASSWORD", "PASSWD", "PASSPHRASE", "APIKEY", "CREDENTIAL", "CREDENTIALS", "PAT"];

/// Two words in a row that name a secret.
const SECRET_PAIRS: &[[&str; 2]] = &[["API", "KEY"], ["PRIVATE", "KEY"], ["ACCESS", "KEY"], ["AUTH", "KEY"]];

/// The last words of a key that names a place of a secret, not the secret.
const PLACE_WORDS: &[&str] = &["FILE", "PATH", "DIR", "DIRECTORY", "FD", "URL", "NAME", "ID"];

/// The shortest random word that is refused, and its least entropy in bits
/// for each character. A random word of 32 characters of base62 has about
/// 4.7; a long name of words has less than 4.
const RANDOM_LEN: usize = 32;
const RANDOM_BITS: f64 = 4.3;

/// The rule of a word that looks like a secret, or None.
pub fn looks_secret(word: &str) -> Option<&'static str> {
    if word.contains("PRIVATE KEY-----") || word.contains("-----BEGIN OPENSSH") {
        return Some("a private key");
    }
    if password_in_url(word) {
        return Some("a password in a URL");
    }
    for run in runs(word) {
        for (prefix, rest, rule) in PREFIXES {
            if let Some(tail) = run.strip_prefix(prefix)
                && tail.len() >= *rest
            {
                return Some(rule);
            }
        }
        if random(run) {
            return Some("a long random word");
        }
    }
    None
}

/// True when `key` (a variable, or a flag with no `--`) names a secret and
/// not its place: `GITHUB_TOKEN`, `api-key`, but not `GITHUB_TOKEN_FILE`.
pub fn names_secret(key: &str) -> bool {
    let words: Vec<String> = key.split(['_', '-']).filter(|w| !w.is_empty()).map(|w| w.to_ascii_uppercase()).collect();
    if words.last().is_some_and(|w| PLACE_WORDS.contains(&w.as_str())) {
        return false;
    }
    words.iter().any(|w| SECRET_WORDS.contains(&w.as_str()))
        || words.windows(2).any(|pair| SECRET_PAIRS.iter().any(|p| pair[0] == p[0] && pair[1] == p[1]))
}

/// Refuse a unit with a word that looks like a secret.
pub fn check(unit: &Unit) -> Result<(), Failure> {
    for (place, word) in unit.words() {
        if let Some(rule) = looks_secret(&word) {
            return Err(refusal(&place, rule));
        }
    }
    for env in &unit.env {
        if !env.placeholder && !env.value.is_empty() && names_secret(&env.key) {
            return Err(refusal(&format!("the variable {}", env.key), "a key that names a secret"));
        }
    }
    let mut args = unit.args.iter().enumerate().peekable();
    while let Some((n, arg)) = args.next() {
        let Some(flag) = arg.strip_prefix("--").or_else(|| arg.strip_prefix('-')) else { continue };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (flag, None),
        };
        if !names_secret(name) {
            continue;
        }
        // The value of the flag: after `=`, else the next word.
        let value = inline.or_else(|| args.peek().map(|(_, next)| next.as_str()).filter(|v| !v.starts_with('-')));
        if value.is_some_and(|v| !v.is_empty()) {
            return Err(refusal(&format!("argument {}", n + 1), "a flag that takes a secret"));
        }
    }
    Ok(())
}

fn refusal(place: &str, rule: &str) -> Failure {
    usage(format!(
        "{place} looks like a secret ({rule}): a unit is readable by each user; give the secret in a private file \
         (a secret file of the unit), or a placeholder of the broker"
    ))
}

/// `scheme://user:password@host`: a userinfo with a password.
fn password_in_url(word: &str) -> bool {
    let Some(at) = word.find("://") else { return false };
    let rest = &word[at + 3..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    match authority.rsplit_once('@') {
        Some((userinfo, _)) => userinfo.split_once(':').is_some_and(|(_, password)| !password.is_empty()),
        None => false,
    }
}

/// The runs of a word between the bytes that no token holds (`=` ends a
/// key and pads base64).
fn runs(word: &str) -> impl Iterator<Item = &str> {
    word.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/'))).flat_map(|run| {
        // A path splits at `/`; a token of base64 keeps it when it is long.
        if run.starts_with('/') { run.split('/').collect::<Vec<_>>() } else { vec![run] }
    })
}

/// A long word with upper case, lower case and digits, whose characters
/// spread as a random word's do.
fn random(run: &str) -> bool {
    if run.len() < RANDOM_LEN {
        return false;
    }
    let has = |f: fn(&u8) -> bool| run.as_bytes().iter().any(f);
    if !(has(u8::is_ascii_uppercase) && has(u8::is_ascii_lowercase) && has(u8::is_ascii_digit)) {
        return false;
    }
    let mut counts = [0usize; 256];
    for b in run.bytes() {
        counts[b as usize] += 1;
    }
    let len = run.len() as f64;
    let bits: f64 = counts.iter().filter(|c| **c > 0).map(|c| *c as f64 / len).map(|p| -p * p.log2()).sum();
    bits >= RANDOM_BITS
}

#[cfg(test)]
#[path = "secret_tests.rs"]
mod tests;
