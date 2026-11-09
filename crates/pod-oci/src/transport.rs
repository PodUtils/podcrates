//! Which registries the client may reach over plain HTTP, and whose
//! certificate it may not check.
//!
//! The default is HTTPS with a full check of the certificate. The client
//! does not fall back to plain HTTP on its own: on hosts where outbound
//! tcp/80 is dropped, a fallback hangs and does not fail. A registry that the
//! caller named is a different case: a local registry on loopback with a
//! certificate of its own, or with none.
//!
//! A fallback and an instruction are different. The client stops a check or
//! speaks HTTP only for a registry that the caller named, and it says so
//! each time ([`Policy::disclosure`], and an event of
//! [`crate::registry::Client`]): an automated caller cannot see a weaker
//! transport as a person can.
//!
//! The caller fills the policy from its flags, its variables and its file;
//! [`parse_hosts`] reads the lines of a file of hosts.

use std::collections::BTreeSet;

use crate::error::{Result, usage};

/// What a caller asked for, resolved once and then carried by value.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// The registries with plain HTTP and TLS with no check of the
    /// certificate. docker's `--insecure-registry` means both, and so does
    /// this.
    insecure: BTreeSet<String>,
    /// `Some(false)` is podman's `--tls-verify=false`: it applies to each
    /// registry of the call, not to a named one. `None` is "not given", which
    /// is not `Some(true)`: an explicit `--tls-verify=true` wins over the
    /// file and the environment, and no word does not.
    tls_verify: Option<bool>,
    /// Whether a named host gets a new try over HTTP after HTTPS does not
    /// connect. Only for a host that the caller named insecure.
    pub http_fallback: bool,
}

/// What transport a single request should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub scheme: &'static str,
    pub verify: bool,
}

impl Policy {
    /// The policy of the hosts that the caller named insecure, each checked
    /// by [`normalise_host`], and of `--tls-verify`. The caller joins its
    /// flags, its variables and its file into `hosts`: a flag never loses to
    /// a file, as the sets join and `tls_verify` is the flag's.
    pub fn resolve<'a>(hosts: impl IntoIterator<Item = &'a str>, tls_verify: Option<bool>) -> Result<Policy> {
        let mut insecure = BTreeSet::new();
        for h in hosts {
            insecure.insert(normalise_host(h)?);
        }
        Ok(Policy { insecure, tls_verify, http_fallback: true })
    }

    /// The same policy with `tls_verify` (podman's `--tls-verify`).
    pub fn with_tls_verify(self, tls_verify: Option<bool>) -> Policy {
        Policy { tls_verify, ..self }
    }

    /// A policy of hosts with no check: for tests and for callers that
    /// resolved their own set.
    pub fn with_insecure(hosts: &[&str]) -> Policy {
        Policy { insecure: hosts.iter().map(|h| (*h).to_string()).collect(), tls_verify: None, http_fallback: true }
    }

    pub fn is_insecure(&self, endpoint: &str) -> bool {
        self.insecure.contains(endpoint)
    }

    /// Whether this endpoint's certificate is verified.
    ///
    /// `--tls-verify=true` wins over each other source, also over a host of
    /// the file: the caller asked for the check now, and a file that the
    /// caller may not have written does not win with no word.
    pub fn verify(&self, endpoint: &str) -> bool {
        match self.tls_verify {
            Some(v) => v,
            None => !self.is_insecure(endpoint),
        }
    }

    /// The first transport to try for an endpoint.
    pub fn first(&self, endpoint: &str) -> Decision {
        Decision { scheme: "https", verify: self.verify(endpoint) }
    }

    /// The transport to try after `first` failed to connect, if any.
    ///
    /// `None` unless the caller named this endpoint insecure. The client does
    /// not find that a registry speaks HTTP; the caller tells it. A fallback
    /// on its own on a network that drops tcp/80 is a hang, and a hang costs
    /// an automated caller its whole run.
    pub fn after_connect_failure(&self, endpoint: &str) -> Option<Decision> {
        if self.http_fallback && self.is_insecure(endpoint) {
            Some(Decision { scheme: "http", verify: false })
        } else {
            None
        }
    }

    /// Whether an explicit `http://` in a reference is honoured.
    pub fn permits_explicit_http(&self, endpoint: &str) -> bool {
        self.is_insecure(endpoint)
    }

    /// One line for the caller to show, with no name of a program before it.
    /// Never silent: an automated caller cannot see a weaker transport as a
    /// person can.
    pub fn disclosure(&self, endpoint: &str) -> Option<String> {
        let verify = self.verify(endpoint);
        let http = self.is_insecure(endpoint);
        if verify && !http {
            return None;
        }
        let mut what = Vec::new();
        if !verify {
            what.push("its certificate is NOT verified");
        }
        if http && self.http_fallback {
            what.push("plain HTTP is permitted if HTTPS cannot connect");
        }
        Some(format!("{endpoint} is configured as an insecure registry: {}", what.join(", ")))
    }

    /// Each named endpoint, for a report and for a diagnostic.
    pub fn named(&self) -> Vec<&str> {
        self.insecure.iter().map(String::as_str).collect()
    }
}

/// A host, never a URL. `--insecure-registry https://x/` is a frequent
/// mistake: it gets a refusal by name, not a key that matches nothing.
pub fn normalise_host(h: &str) -> Result<String> {
    let h = h.trim();
    if h.is_empty() {
        return Err(usage("an empty --insecure-registry names nothing. Write a host, for example localhost:5000"));
    }
    for bad in ["http://", "https://"] {
        if let Some(rest) = h.strip_prefix(bad) {
            return Err(usage(format!(
                "--insecure-registry takes a HOST and not a URL: write \
                 {:?} rather than {h:?}",
                rest.trim_end_matches('/')
            )));
        }
    }
    if h.contains('/') {
        return Err(usage(format!(
            "--insecure-registry takes a host, optionally with a port, and \
             {h:?} contains a path"
        )));
    }
    Ok(h.to_string())
}

/// The hosts of a file of hosts: one host per line, `#` starts a comment.
/// `label` names the file in an error. A file of lines, not TOML: the
/// `registries.conf` of podman is a schema that the client would follow, and
/// a shell script reads lines with no parser. A bad line gets a refusal
/// with its number, never a skip: a skipped line is a registry that the
/// caller thinks is named and that is not.
pub fn parse_hosts(text: &str, label: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        out.push(normalise_host(line).map_err(|e| usage(format!("{label}:{}: {e}", n + 1)))?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_verifies_everything_and_permits_no_http() {
        let p = Policy::default();
        assert!(p.verify("ghcr.io"));
        assert!(!p.permits_explicit_http("ghcr.io"));
        assert_eq!(p.after_connect_failure("ghcr.io"), None);
        assert_eq!(p.disclosure("ghcr.io"), None);
    }

    #[test]
    fn a_named_registry_gets_http_and_no_verification_and_says_so() {
        let p = Policy::with_insecure(&["localhost:5000"]);
        assert!(!p.verify("localhost:5000"));
        assert!(p.permits_explicit_http("localhost:5000"));
        assert_eq!(p.after_connect_failure("localhost:5000"), Some(Decision { scheme: "http", verify: false }));
        let said = p.disclosure("localhost:5000").expect("a disclosure");
        assert!(said.contains("NOT verified"), "{said}");
        // And it changes nothing for each other registry.
        assert!(p.verify("ghcr.io"));
        assert_eq!(p.after_connect_failure("ghcr.io"), None);
    }

    #[test]
    fn tls_verify_true_beats_a_host_named_insecure() {
        // The caller asked for the check now: a file that the caller may not
        // have written does not win.
        let p = Policy::with_insecure(&["localhost:5000"]).with_tls_verify(Some(true));
        assert!(p.verify("localhost:5000"));
    }

    #[test]
    fn tls_verify_false_applies_to_every_registry_this_invocation_touches() {
        // podman's meaning: the flag is for the command, not for one host.
        let p = Policy::default().with_tls_verify(Some(false));
        assert!(!p.verify("ghcr.io"));
        // It does not allow plain HTTP. No check of a certificate and no
        // certificate are different asks; only --insecure-registry is the
        // second.
        assert!(!p.permits_explicit_http("ghcr.io"));
        assert_eq!(p.after_connect_failure("ghcr.io"), None);
    }

    #[test]
    fn a_url_where_a_host_belongs_is_refused_and_names_the_fix() {
        let e = normalise_host("https://localhost:5000/").unwrap_err();
        let text = format!("{e}");
        assert!(text.contains("localhost:5000"), "{text}");
        assert!(normalise_host("localhost:5000/v2").is_err());
        assert!(normalise_host("  ").is_err());
        assert_eq!(normalise_host(" localhost:5000 ").unwrap(), "localhost:5000");
    }

    #[test]
    fn a_file_of_hosts_names_its_bad_line_and_joins_the_flags() {
        let hosts = parse_hosts("# local\nlocalhost:5000\n\n  reg.lan # lab\n", "hosts.conf").unwrap();
        assert_eq!(hosts, ["localhost:5000", "reg.lan"]);
        let e = parse_hosts("ok.lan\nhttps://x/\n", "hosts.conf").unwrap_err();
        assert!(e.to_string().starts_with("hosts.conf:2: "), "{e}");
        assert_eq!(e.class, pod_core::Class::Usage);
        let p =
            Policy::resolve(["a.lan", " b.lan "].into_iter().chain(hosts.iter().map(String::as_str)), None).unwrap();
        assert_eq!(p.named(), ["a.lan", "b.lan", "localhost:5000", "reg.lan"]);
        assert!(Policy::resolve(["a/b"], None).is_err());
    }
}
