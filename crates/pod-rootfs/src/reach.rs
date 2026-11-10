//! Does this mirror speak HTTPS? One question for each host, which the
//! caller answers.
//!
//! The change of a package source checks first: a source that then gives
//! 404 is worse than the wait that the change was for, as the failure no
//! longer names its cause.
//!
//! The crate makes no request: the caller gives a [`Probe`] with its own
//! transport, its proxy and its trust (`pod-proxy` and `pod-tls` for
//! podvm). The probe asks `https://HOST/` with a bounded time, and each HTTP
//! answer (404 too) is a yes: the question is if the host ends TLS on 443.
//! A connect failure, a TLS failure or a timeout is a no, with its reason.
//!
//! The crate asks only about a DNS name (see [`is_name`]): an address, a
//! loopback name, a user part or a text that is not a name never reaches a
//! probe.

use std::collections::BTreeMap;

/// What one host answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Https,
    /// No, and why: the row of the host says it.
    No(String),
}

impl Answer {
    pub fn ok(&self) -> bool {
        *self == Answer::Https
    }

    pub fn why(&self) -> &str {
        match self {
            Answer::Https => "the host answered over HTTPS",
            Answer::No(w) => w,
        }
    }
}

/// The question of the caller: does `host` answer over HTTPS on 443? The
/// caller bounds the time of each question. A function `FnMut(&str) ->
/// Answer` is a probe.
pub trait Probe {
    fn https(&mut self, host: &str) -> Answer;
}

impl<F: FnMut(&str) -> Answer> Probe for F {
    fn https(&mut self, host: &str) -> Answer {
        self(host)
    }
}

/// A probe that gives one answer with no request, for a caller that wants
/// no question asked and for the tests.
pub struct Fixed(pub bool);

impl Probe for Fixed {
    fn https(&mut self, _host: &str) -> Answer {
        if self.0 { Answer::Https } else { Answer::No("the probe was told to answer no".into()) }
    }
}

/// The probe of the caller, asked one time for each host in one completion.
pub(crate) struct Prober<'a> {
    probe: &'a mut dyn Probe,
    seen: BTreeMap<String, Answer>,
}

impl<'a> Prober<'a> {
    pub(crate) fn new(probe: &'a mut dyn Probe) -> Prober<'a> {
        Prober { probe, seen: BTreeMap::new() }
    }

    pub(crate) fn ask(&mut self, host: &str) -> Answer {
        if let Some(a) = self.seen.get(host) {
            return a.clone();
        }
        let a = self.probe.https(host);
        self.seen.insert(host.to_string(), a.clone());
        a
    }

    /// Each host asked, and its answer, in the order of the names.
    pub(crate) fn asked(&self) -> impl Iterator<Item = (&str, &Answer)> {
        self.seen.iter().map(|(k, a)| (k.as_str(), a))
    }
}

/// Is `host` a DNS name that the crate asks about?
///
/// Each label is 1 to 63 letters, digits and `-`, with no `-` at either end,
/// and the last label is not a number. A host of an address in any form
/// that a resolver takes (`10.0.0.1`, `127.1`, `0x7f000001`, `2130706433`,
/// `[::1]`), `localhost` and a name under `.localhost` are not names: a
/// mirror there is a cache of the caller, and `https://` there is a refused
/// connection, not a fix.
pub fn is_name(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    let lower = host.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") {
        return false;
    }
    let labels: Vec<&str> = lower.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    if !labels.iter().all(label_ok) {
        return false;
    }
    let last = labels[labels.len() - 1];
    let hex = last.strip_prefix("0x").is_some_and(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
    let first_hex = labels[0].strip_prefix("0x").is_some_and(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
    !last.bytes().all(|b| b.is_ascii_digit()) && !hex && !(labels.len() == 1 && first_hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_prober_never_touches_the_network() {
        let mut yes = Fixed(true);
        assert!(Prober::new(&mut yes).ask("deb.debian.org").ok());
        let mut no = Fixed(false);
        assert!(!Prober::new(&mut no).ask("deb.debian.org").ok());
    }

    /// A no carries its reason, so that the row says why a file stayed.
    #[test]
    fn a_refusal_carries_its_reason() {
        let a = Answer::No("connect timed out".into());
        assert!(!a.ok());
        assert_eq!(a.why(), "connect timed out");
    }

    /// The probe of the caller is asked one time for each host.
    #[test]
    fn each_host_is_asked_one_time() {
        let mut asked = Vec::new();
        let mut probe = |h: &str| {
            asked.push(h.to_string());
            Answer::Https
        };
        let mut p = Prober::new(&mut probe);
        for h in ["a.example", "b.example", "a.example"] {
            assert!(p.ask(h).ok());
        }
        assert_eq!(p.asked().count(), 2);
        drop(p);
        assert_eq!(asked, ["a.example", "b.example"]);
    }

    #[test]
    fn only_a_dns_name_is_a_name() {
        for yes in
            ["deb.debian.org", "mirror-1.example", "dl-cdn.alpinelinux.org.", "a1.b2.example", "xn--bcher-kva.example"]
        {
            assert!(is_name(yes), "{yes}");
        }
        for no in [
            "",
            "localhost",
            "repo.localhost",
            "10.0.0.1",
            "127.1",
            "2130706433",
            "0x7f000001",
            "0x7f.1",
            "[::1]",
            "user@mirror.example",
            "mirror.example?x",
            "mirror.example\\@x",
            "-bad.example",
            "a..example",
            "mirror_1.example",
        ] {
            assert!(!is_name(no), "{no}");
        }
    }
}
