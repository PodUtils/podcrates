//! The policy of a job, version 1 (`schemas/broker-policy.v1.schema.json`):
//! a mode, the allow rules and the extra denies. The fixed denies of
//! [`crate::deny`] hold in each mode; no policy lifts them.
//!
//! ```toml
//! version = 1
//! mode = "allow"                # or "open": allow each target that no deny holds
//! deny = ["203.0.113.0/24", "*.ads.example"]
//!
//! [[allow]]
//! host = "api.github.com"
//! path = "/repos/owner"         # whole segments: /repos/owner/x, not /repos/ownerx
//! ports = [443]                 # no ports: each port
//!
//! [[allow]]
//! host = "*.githubusercontent.com"
//! ```

use std::net::IpAddr;
use std::path::Path;

use pod_core::{Class, Failure, Stage};
use serde::{Deserialize, Serialize};

use crate::deny::{DenyList, NameRule};
use crate::target::{Authority, Host, normal_path, parse_host};

/// The one version of the policy that this crate reads.
pub const VERSION: u32 = 1;

/// The rule of a CONNECT whose host has rules with a path only: the broker
/// sees no path in a tunnel until a TLS end does ([`crate::TunnelEnd`]).
pub const PATH_NEEDS_TLS_END: &str = "policy:path-needs-tls-end";

/// How a policy treats a target that no rule names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Only the targets of an allow rule.
    Allow,
    /// Each target that no deny holds.
    Open,
}

impl Mode {
    pub fn word(self) -> &'static str {
        match self {
            Mode::Allow => "allow",
            Mode::Open => "open",
        }
    }
}

/// One allow rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// A name, `*.NAME` (each name below it, on whole labels), or an
    /// address.
    pub host: String,
    /// A path prefix on whole segments; none: each path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The ports; empty: each port.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<u16>,
}

/// The policy of a job, as a job source writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u32,
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<Rule>,
    /// Extra denies: a net, an address, a name or `*.NAME`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
}

fn usage(words: String) -> Failure {
    Failure::new(Stage::Proxy, Class::Usage, words)
}

impl Policy {
    /// The open policy with no extra deny.
    pub fn open() -> Policy {
        Policy { version: VERSION, mode: Mode::Open, allow: Vec::new(), deny: Vec::new() }
    }

    /// A policy in JSON. A bad policy is a failure of the class `Usage`.
    pub fn from_json(text: &str) -> Result<Policy, Failure> {
        serde_json::from_str(text).map_err(|e| usage(format!("the policy is not a policy of version 1: {e}")))
    }

    /// A policy as a JSON value (the `policy` of a `load`).
    pub fn from_value(value: serde_json::Value) -> Result<Policy, Failure> {
        serde_json::from_value(value).map_err(|e| usage(format!("the policy is not a policy of version 1: {e}")))
    }

    /// A policy in TOML.
    pub fn from_toml(text: &str) -> Result<Policy, Failure> {
        toml::from_str(text).map_err(|e| usage(format!("the policy is not a policy of version 1: {e}")))
    }

    /// The policy file at `path`: TOML for a name that ends `.toml`, else
    /// JSON.
    pub fn read(path: &Path) -> Result<Policy, Failure> {
        let text = std::fs::read_to_string(path).map_err(|e| Failure::io(Stage::Host, e))?;
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("toml")) {
            Policy::from_toml(&text)
        } else {
            Policy::from_json(&text)
        }
    }

    /// The checked form of the policy, or why it cannot be used.
    pub fn compile(&self) -> Result<Compiled, Failure> {
        if self.version != VERSION {
            return Err(usage(format!("the policy is version {}: this broker reads version {VERSION}", self.version)));
        }
        if self.mode == Mode::Open && !self.allow.is_empty() {
            return Err(usage("an open policy has no allow rule: give mode \"allow\" with the rules".into()));
        }
        let mut rules = Vec::with_capacity(self.allow.len());
        for (n, rule) in self.allow.iter().enumerate() {
            rules.push(compile_rule(rule).map_err(|e| usage(format!("allow rule {n}: {e}")))?);
        }
        let mut deny = DenyList::new();
        for (n, entry) in self.deny.iter().enumerate() {
            deny.add(entry, format!("policy:deny[{n}]")).map_err(|e| usage(format!("deny entry {n}: {e}")))?;
        }
        Ok(Compiled { mode: self.mode, rules, deny })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HostRule {
    Name(NameRule),
    Ip(IpAddr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompiledRule {
    host: HostRule,
    path: Option<String>,
    ports: Vec<u16>,
}

fn compile_rule(rule: &Rule) -> Result<CompiledRule, String> {
    let host = match rule.host.strip_prefix("*.") {
        Some(_) => HostRule::Name(NameRule::parse(&rule.host)?),
        None => match parse_host(&rule.host).map_err(|e| e.0)? {
            Host::Name(name) => HostRule::Name(NameRule::Exact(name)),
            Host::Ip(ip) => HostRule::Ip(crate::deny::plain(ip)),
        },
    };
    let path = match &rule.path {
        None => None,
        Some(path) => {
            if path.contains(['?', '#']) {
                return Err(format!("{path:?}: a path prefix has no query and no fragment"));
            }
            let normal = normal_path(path).map_err(|e| e.0)?;
            let trimmed = normal.trim_end_matches('/');
            Some(if trimmed.is_empty() { "/".to_string() } else { trimmed.to_string() })
        }
    };
    if rule.ports.contains(&0) {
        return Err("port 0 is not a port".into());
    }
    Ok(CompiledRule { host, path, ports: rule.ports.clone() })
}

/// What a policy says about a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Allowed by the rule of this name (`open`, `allow[2]`).
    Allow(String),
    /// A tunnel to a host whose rules each have a path: a TLS end must check
    /// each request ([`PATH_NEEDS_TLS_END`]).
    PathsOnly,
    /// No rule allows it (`policy:no-rule`).
    Deny(String),
}

/// A policy in its checked form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    mode: Mode,
    rules: Vec<CompiledRule>,
    deny: DenyList,
}

impl Compiled {
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The number of allow rules.
    pub fn rules(&self) -> usize {
        self.rules.len()
    }

    /// The extra denies of the policy.
    pub fn deny(&self) -> &DenyList {
        &self.deny
    }

    /// The verdict for `target`: `path` is the normal path of a plain
    /// request, `None` for a tunnel. The denies are not checked here.
    pub fn verdict(&self, target: &Authority, path: Option<&str>) -> Verdict {
        if self.mode == Mode::Open {
            return Verdict::Allow("open".into());
        }
        let mut paths_only = false;
        for (n, rule) in self.rules.iter().enumerate() {
            if !rule.holds_host(&target.host) || !(rule.ports.is_empty() || rule.ports.contains(&target.port)) {
                continue;
            }
            match (&rule.path, path) {
                (None, _) => return Verdict::Allow(format!("allow[{n}]")),
                (Some(prefix), Some(path)) if path_holds(prefix, path) => return Verdict::Allow(format!("allow[{n}]")),
                (Some(_), None) => paths_only = true,
                (Some(_), Some(_)) => {}
            }
        }
        if paths_only { Verdict::PathsOnly } else { Verdict::Deny("policy:no-rule".into()) }
    }

    /// The rule that allows `path` on `target` inside a tunnel, for a TLS
    /// end that reads each request ([`crate::TunnelEnd`]).
    pub fn allows_path(&self, target: &Authority, path: &str) -> Option<String> {
        match self.verdict(target, Some(path)) {
            Verdict::Allow(rule) => Some(rule),
            _ => None,
        }
    }
}

impl CompiledRule {
    fn holds_host(&self, host: &Host) -> bool {
        match (&self.host, host) {
            (HostRule::Name(rule), Host::Name(name)) => rule.holds(name),
            (HostRule::Ip(rule), Host::Ip(ip)) => *rule == crate::deny::plain(*ip),
            _ => false,
        }
    }
}

/// Whether the prefix `prefix` holds `path` on whole segments:
/// `/v1/repos` holds `/v1/repos` and `/v1/repos/x`, not `/v1/reposx`.
pub fn path_holds(prefix: &str, path: &str) -> bool {
    prefix == "/" || path == prefix || (path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/'))
}
