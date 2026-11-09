//! Which registries podvm may reach over plain HTTP, and whose certificate it
//! may decline to verify.
//!
//! [`TODO/image.md`](../../../TODO/image.md) T-0213.
//!
//! ⛔ **The default is unchanged and is still HTTPS with full verification.**
//! [T-0201](../../../TODO/image.md) refused a plain-HTTP *fallback* because
//! tcp/80 egress is broken on the runtime podvm targets, so an automatic
//! downgrade **hangs** instead of failing. That finding stands. What it does
//! not justify is refusing a registry the caller has explicitly named, which is
//! the ordinary case of a local registry on loopback with a self-signed
//! certificate or none at all.
//!
//! ⭐ **The distinction this module draws is between a fallback and an
//! instruction.** podvm never decides on its own to stop verifying or to speak
//! HTTP. It does either when a caller named that registry, and it says so on
//! stderr every time, because the audience is automated and an agent cannot
//! notice that its transport was downgraded the way a person might.
//!
//! ⚠ **A correction to what this file used to say.** `crates/podvm-image/src/tls.rs`
//! stated that "`docs/security/remote-ops.md` and the environment note in
//! `AGENTS.md` both say never to disable verification". Checked on
//! 2026-09-09: `remote-ops.md` says nothing about TLS at all, and
//! `AGENTS.md`'s note is an instruction to **the agent** about this
//! development container's intercepting proxy, not a rule about what podvm may
//! offer its users. The rule that was cited did not exist where it was cited.

use std::collections::BTreeSet;

use crate::error::{Error, Result};

/// `docker --insecure-registry`'s own environment spelling, plus podvm's.
const ENV_INSECURE: &str = "PODVM_INSECURE_REGISTRIES";

// ⛔ A loopback registry is never reached through a proxy, and `NO_PROXY` is
// honoured. [`crate::net`] and `pod-proxy` decide, for each request.

/// One host per line, `#` comments. ⚠ Deliberately line-based rather than TOML:
/// podman's `registries.conf` is a schema this would have to track, and a file
/// podvm reads has to be readable by a shell script in
/// `experiments/` without a parser.
const CONFIG_ENV: &str = "PODVM_CONFIG";
const CONFIG_LEAF: &str = "podvm/registries.conf";

/// What a caller asked for, resolved once and then carried by value.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// Registries permitted plain HTTP **and** unverified TLS. docker's
    /// `--insecure-registry` means both, and so does this.
    insecure: BTreeSet<String>,
    /// ⚠ `Some(false)` is podman's `--tls-verify=false` and applies to every
    /// registry this invocation touches, not to a named one. `None` is "not
    /// mentioned", which is not the same as `Some(true)`: an explicit
    /// `--tls-verify=true` overrides the config file and the environment, and
    /// silence does not.
    tls_verify: Option<bool>,
    /// Whether a permitted host may be retried over HTTP after HTTPS fails to
    /// connect. ⛔ Only ever for a host already named insecure.
    pub http_fallback: bool,
}

/// What transport a single request should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub scheme: &'static str,
    pub verify: bool,
}

impl Policy {
    /// Build the policy from the flags, then the environment, then the config
    /// file. ⚠ A flag never *loses* to a file: the sets are unioned, and the
    /// only scalar, `tls_verify`, takes the flag when there is one.
    pub fn resolve(insecure_flags: &[String], tls_verify_flag: Option<bool>) -> Result<Policy> {
        let mut insecure = BTreeSet::new();
        for h in insecure_flags {
            insecure.insert(normalise_host(h)?);
        }
        if let Ok(v) = std::env::var(ENV_INSECURE) {
            for h in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                insecure.insert(
                    normalise_host(h).map_err(|e| {
                        Error::Usage(format!("${ENV_INSECURE} lists a bad host: {e}"))
                    })?,
                );
            }
        }
        for h in read_config()? {
            insecure.insert(h);
        }
        Ok(Policy {
            insecure,
            tls_verify: tls_verify_flag,
            http_fallback: true,
        })
    }

    /// ⚠ For tests and for callers that have already resolved their own set.
    pub fn with_insecure(hosts: &[&str]) -> Policy {
        Policy {
            insecure: hosts.iter().map(|h| (*h).to_string()).collect(),
            tls_verify: None,
            http_fallback: true,
        }
    }

    pub fn is_insecure(&self, endpoint: &str) -> bool {
        self.insecure.contains(endpoint)
    }

    /// Whether this endpoint's certificate is verified.
    ///
    /// ⛔ `--tls-verify=true` wins over everything, including a host listed in
    /// the config file. A caller who says "verify" this time means it, and a
    /// file they may not have written must not quietly override them.
    pub fn verify(&self, endpoint: &str) -> bool {
        match self.tls_verify {
            Some(v) => v,
            None => !self.is_insecure(endpoint),
        }
    }

    /// The first transport to try for an endpoint.
    pub fn first(&self, endpoint: &str) -> Decision {
        Decision {
            scheme: "https",
            verify: self.verify(endpoint),
        }
    }

    /// The transport to try after `first` failed to connect, if any.
    ///
    /// ⛔ `None` unless the caller named this endpoint insecure. podvm does not
    /// discover that a registry speaks HTTP; it is told. That is the whole of
    /// [T-0201](../../../TODO/image.md)'s finding kept intact: an automatic
    /// downgrade on a network where tcp/80 is black-holed is a hang, and a hang
    /// costs an unattended caller its whole run.
    pub fn after_connect_failure(&self, endpoint: &str) -> Option<Decision> {
        if self.http_fallback && self.is_insecure(endpoint) {
            Some(Decision {
                scheme: "http",
                verify: false,
            })
        } else {
            None
        }
    }

    /// Whether an explicit `http://` in a reference is honoured.
    pub fn permits_explicit_http(&self, endpoint: &str) -> bool {
        self.is_insecure(endpoint)
    }

    /// One line, for the caller to print on stderr. ⛔ Never silent: an agent
    /// cannot notice a downgraded transport the way a person might, and
    /// `AGENTS.md` says the honesty rules are the product.
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
        Some(format!(
            "podvm: {endpoint} is configured as an insecure registry: {}",
            what.join(", ")
        ))
    }

    /// Every endpoint named, for `podvm inspect` and for a diagnostic.
    pub fn named(&self) -> Vec<&str> {
        self.insecure.iter().map(String::as_str).collect()
    }
}

/// ⚠ A host, never a URL. `--insecure-registry https://x/` is a common mistake
/// and is refused by name rather than stored as a key nothing will match.
fn normalise_host(h: &str) -> Result<String> {
    let h = h.trim();
    if h.is_empty() {
        return Err(Error::Usage(
            "an empty --insecure-registry names nothing. Write a host, for \
             example localhost:5000"
                .into(),
        ));
    }
    for bad in ["http://", "https://"] {
        if let Some(rest) = h.strip_prefix(bad) {
            return Err(Error::Usage(format!(
                "--insecure-registry takes a HOST and not a URL: write \
                 {:?} rather than {h:?}",
                rest.trim_end_matches('/')
            )));
        }
    }
    if h.contains('/') {
        return Err(Error::Usage(format!(
            "--insecure-registry takes a host, optionally with a port, and \
             {h:?} contains a path"
        )));
    }
    Ok(h.to_string())
}

/// `$PODVM_CONFIG`, else `$XDG_CONFIG_HOME/podvm/registries.conf`, else
/// `$HOME/.config/podvm/registries.conf`. ⚠ A missing file is not an error:
/// most machines have none and that is the default.
fn read_config() -> Result<Vec<String>> {
    let path = match std::env::var_os(CONFIG_ENV) {
        Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
        _ => {
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
                });
            match base {
                Some(b) => b.join(CONFIG_LEAF),
                None => return Ok(Vec::new()),
            }
        }
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(path.display().to_string(), e)),
    };
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        // ⛔ A bad line in a config file is named with its line number and
        // refused, never skipped. A silently ignored entry is a registry the
        // caller believes is permitted and is not.
        out.push(
            normalise_host(line)
                .map_err(|e| Error::Usage(format!("{}:{}: {e}", path.display(), n + 1)))?,
        );
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
        assert_eq!(
            p.after_connect_failure("localhost:5000"),
            Some(Decision {
                scheme: "http",
                verify: false
            })
        );
        let said = p.disclosure("localhost:5000").expect("a disclosure");
        assert!(said.contains("NOT verified"), "{said}");
        // ⛔ And it changes nothing for any OTHER registry.
        assert!(p.verify("ghcr.io"));
        assert_eq!(p.after_connect_failure("ghcr.io"), None);
    }

    #[test]
    fn tls_verify_true_beats_a_host_named_insecure() {
        // ⛔ A caller who says "verify" this time means it, and a config file
        // they may not have written must not quietly override them.
        let p = Policy {
            tls_verify: Some(true),
            ..Policy::with_insecure(&["localhost:5000"])
        };
        assert!(p.verify("localhost:5000"));
    }

    #[test]
    fn tls_verify_false_applies_to_every_registry_this_invocation_touches() {
        // ⚠ podman's semantics: the flag is per-command, not per-host.
        let p = Policy {
            tls_verify: Some(false),
            ..Policy::default()
        };
        assert!(!p.verify("ghcr.io"));
        // ⛔ But it does NOT permit plain HTTP. Not verifying a certificate and
        // not having one are different asks, and only --insecure-registry is
        // the second.
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
        assert_eq!(
            normalise_host(" localhost:5000 ").unwrap(),
            "localhost:5000"
        );
    }
}
