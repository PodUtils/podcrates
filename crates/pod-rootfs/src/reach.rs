//! Does this mirror speak HTTPS? One bounded question, asked once per host.
//!
//! ⭐ [`TODO/complete.md`](../../../TODO/complete.md) T-0411 clause 4:
//! **verify before rewriting**. Rewriting a source that then 404s is worse than
//! the hang it was meant to fix, because the failure no longer names the cause.
//!
//! ⛔ Bounded, always. [`TODO/RULES.md`](../../../TODO/RULES.md) section 8: a
//! runtime whose audience is automated may not wait unbounded, and this probe
//! runs on the path to starting a container.
//!
//! ⚠ **Any HTTP response is a yes**, 404 included. The question is whether the
//! host terminates TLS on 443, not whether it serves `/`. A connect failure, a
//! TLS failure or a timeout is a no, and the reason travels with it so the
//! banner can say which.

use std::collections::HashMap;
use std::time::Duration;

use podvm_image::net::{Limits, Net, Refusal};

/// ⚠ Two seconds to connect and two to read. The whole point is that the
/// alternative -- a black-holed tcp/80 inside the payload -- costs the package
/// manager's own timeout, which is minutes.
const CONNECT: Duration = Duration::from_secs(2);
const READ: Duration = Duration::from_secs(2);

/// What one host answered. ⛔ Three states: it spoke HTTPS, it did not, or
/// podvm could not ask. The third never reads as either of the first two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Https,
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

/// One question per host, remembered for the life of this call.
#[derive(Default)]
pub struct Prober {
    seen: HashMap<String, Answer>,
    /// ⚠ Test seam, and the only one. A completion run in a unit test may not
    /// reach the network at all: `docs/methodology/experiments.md`'s rule is
    /// that a measurement ships with its script, and a unit test that quietly
    /// depends on a mirror being up is a measurement pretending to be a test.
    pub fixed: Option<bool>,
}

impl Prober {
    pub fn new() -> Prober {
        Prober::default()
    }

    /// A prober that answers `yes` to everything without a packet, for tests.
    pub fn always(yes: bool) -> Prober {
        Prober {
            seen: HashMap::new(),
            fixed: Some(yes),
        }
    }

    pub fn ask(&mut self, host: &str) -> Answer {
        if let Some(y) = self.fixed {
            return if y {
                Answer::Https
            } else {
                Answer::No("this prober was told to answer no".into())
            };
        }
        if let Some(a) = self.seen.get(host) {
            return a.clone();
        }
        let a = probe(host);
        self.seen.insert(host.to_string(), a.clone());
        a
    }

    /// Every host asked, and what it said, for the report.
    pub fn asked(&self) -> Vec<(&str, &Answer)> {
        let mut v: Vec<(&str, &Answer)> = self.seen.iter().map(|(k, a)| (k.as_str(), a)).collect();
        v.sort_by_key(|(k, _)| *k);
        v
    }
}

fn probe(host: &str) -> Answer {
    let (config, _roots) = podvm_image::tls::client_config();
    // ⚠ The environment's proxy where it sets one (`podvm_image::net`). An
    // intercepting proxy is the normal case in the environments podvm is
    // built for, and ignoring it turns a working mirror into a connect
    // timeout.
    let mut net = match Net::from_environment(config) {
        Ok(n) => n,
        Err(why) => return Answer::No(why),
    };
    let limits = Limits {
        head: CONNECT.saturating_add(READ),
        idle: READ,
    };
    match net.send("HEAD", &format!("https://{host}/"), &[], true, limits) {
        Ok(_) => Answer::Https,
        // ⭐ A status is an answer. The host terminated TLS and replied, which
        // is the whole question; that it has nothing at `/` is not.
        Err(Refusal::Status(_)) => Answer::Https,
        Err(e) => Answer::No(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_prober_never_touches_the_network() {
        let mut p = Prober::always(true);
        assert!(p.ask("deb.debian.org").ok());
        let mut n = Prober::always(false);
        assert!(!n.ask("deb.debian.org").ok());
    }

    /// ⛔ The three states stay three. A `No` carries its reason so the banner
    /// can say why a file was left alone.
    #[test]
    fn a_refusal_carries_its_reason() {
        let a = Answer::No("connect timed out".into());
        assert!(!a.ok());
        assert_eq!(a.why(), "connect timed out");
    }
}
