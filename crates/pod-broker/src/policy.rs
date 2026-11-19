//! The policy of a job, version 1 (`schemas/broker-policy.v1.schema.json`):
//! a mode, the allow rules and the extra denies. The fixed denies of
//! [`crate::deny`] hold in each mode; no policy lifts them.
//!
//! ```toml
//! schema = 1
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

use crate::credentials::{Checked, Credential};
use crate::deny::{DenyList, NameRule};
use crate::target::{Authority, Host, normal_path, parse_host};

/// The one version of the policy that this crate reads: the constant of
/// its field `schema`, as in each schema of `schemas/`.
pub const SCHEMA: u32 = 1;

/// The rule of a CONNECT whose host has rules with a path only: the broker
/// sees no path in a tunnel until a TLS end does ([`crate::TunnelEnd`]).
pub const PATH_NEEDS_TLS_END: &str = "policy:path-needs-tls-end";

/// How a policy treats a target that no rule names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Only the targets of an allow rule.
    Allow,
    /// Each target that no deny holds. An open policy has no allow rule.
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

/// One allow rule. The audit names a rule by its index, `allow[N]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// A host name (matched in lower case, with no dot at its end), `*.NAME`
    /// (each name below NAME, on whole labels, not NAME itself), an IPv4
    /// address in four decimal parts, or an IPv6 address in brackets.
    #[cfg_attr(any(test, feature = "schema"), schemars(length(min = 1, max = 255)))]
    pub host: String,
    /// A path prefix on whole segments: `/v1/repos` holds `/v1/repos` and
    /// `/v1/repos/x`, not `/v1/reposx`. No dot segment, no empty segment and
    /// no escape of a dot or a slash. None: each path. A tunnel (CONNECT)
    /// shows no path: a rule with a path allows a tunnel only when the host
    /// gives a TLS end that reads each request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(any(test, feature = "schema"), schemars(regex(pattern = r"^/[^?#\\]*$")))]
    pub path: Option<String>,
    /// The ports; empty: each port. Port 25 is denied in each mode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(any(test, feature = "schema"), schemars(inner(range(min = 1))))]
    pub ports: Vec<u16>,
}

/// The policy of a job, as a job source writes it
/// (`schemas/broker-policy.v1.schema.json` comes from this type).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[cfg_attr(any(test, feature = "schema"), schemars(
    title = "The policy of one job of the egress broker (pod-broker), version 1",
    description = "The JSON form of pod_broker::Policy; the TOML form has the same keys. The fixed denies of \
        crates/pod-broker/README.md hold in each mode, and no policy lifts them. A policy that does not agree with \
        this schema is a failure of the class usage (exit 64).",
    transform = crate::json_schema::policy_rules
))]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// The version of the schema: [`SCHEMA`], as in each schema of
    /// `schemas/`.
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::policy_version"))]
    pub schema: u32,
    pub mode: Mode,
    /// The allow rules, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<Rule>,
    /// Extra denies after the fixed ones: a net (`ADDR/PREFIX`), an
    /// address, a name or `*.NAME`. The audit names an entry
    /// `policy:deny[N]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(any(test, feature = "schema"), schemars(inner(length(min = 1))))]
    pub deny: Vec<String>,
    /// The placeholder credentials: the job holds each placeholder, and the
    /// broker swaps it for the real value of its secret on the way to its
    /// hosts, inside its TLS end ([`crate::credentials`]). A policy names a
    /// secret, never its value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<Credential>,
}

fn usage(words: String) -> Failure {
    Failure::new(Stage::Proxy, Class::Usage, words)
}

impl Policy {
    /// The open policy with no extra deny.
    pub fn open() -> Policy {
        Policy { schema: SCHEMA, mode: Mode::Open, allow: Vec::new(), deny: Vec::new(), credentials: Vec::new() }
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
        if self.schema != SCHEMA {
            return Err(usage(format!("the policy is schema {}: this broker reads schema {SCHEMA}", self.schema)));
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
        let flagged =
            self.allow.iter().filter(|r| crate::names::open_suffix(&r.host).is_some()).map(|r| r.host.clone());
        let credentials = crate::credentials::compile(&self.credentials).map_err(usage)?;
        Ok(Compiled { mode: self.mode, rules, deny, flagged: flagged.collect(), credentials })
    }
}

/// The host of a rule or of a credential: a name, `*.NAME` or an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostRule {
    Name(NameRule),
    Ip(IpAddr),
}

impl HostRule {
    /// The host of `text`: a name, `*.NAME`, or an address.
    pub(crate) fn parse(text: &str) -> Result<HostRule, String> {
        Ok(match text.strip_prefix("*.") {
            Some(_) => HostRule::Name(NameRule::parse(text)?),
            None => match parse_host(text).map_err(|e| e.0)? {
                Host::Name(name) => HostRule::Name(NameRule::Exact(name)),
                Host::Ip(ip) => HostRule::Ip(crate::deny::plain(ip)),
            },
        })
    }

    pub(crate) fn holds(&self, host: &Host) -> bool {
        match (self, host) {
            (HostRule::Name(rule), Host::Name(name)) => rule.holds(name),
            (HostRule::Ip(rule), Host::Ip(ip)) => *rule == crate::deny::plain(*ip),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompiledRule {
    host: HostRule,
    path: Option<String>,
    ports: Vec<u16>,
}

fn compile_rule(rule: &Rule) -> Result<CompiledRule, String> {
    let host = HostRule::parse(&rule.host)?;
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

/// The half of the rules that failed for a target of [`Verdict::Deny`]
/// ([`Compiled::no_rule`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoRule {
    /// No allow rule names the host.
    Host,
    /// The rules of the host take these ports only.
    Port(Vec<u16>),
    /// A rule names the host and the port, but no rule holds the path of
    /// the plain request.
    Path,
}

impl NoRule {
    /// The words for `target`: `no allow rule names the host example.com`,
    /// `the rules of the host example.com take the port 443, not 80`.
    pub fn words(&self, target: &Authority) -> String {
        let host = &target.host;
        match self {
            NoRule::Host => format!("no allow rule names the host {host}"),
            NoRule::Port(ports) => {
                let words = if ports.len() == 1 { "port" } else { "ports" };
                let ports: Vec<String> = ports.iter().map(u16::to_string).collect();
                format!("the rules of the host {host} take the {words} {}, not {}", ports.join(", "), target.port)
            }
            NoRule::Path => format!("no allow rule of {target} holds the path"),
        }
    }
}

/// A policy in its checked form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    mode: Mode,
    rules: Vec<CompiledRule>,
    deny: DenyList,
    flagged: Vec<String>,
    credentials: Vec<Checked>,
}

impl Compiled {
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The number of allow rules.
    pub fn rules(&self) -> usize {
        self.rules.len()
    }

    /// The host of each allow rule under a suffix where anyone can register
    /// a name ([`crate::names::open_suffix`]).
    pub fn flagged(&self) -> &[String] {
        &self.flagged
    }

    /// The extra denies of the policy.
    pub fn deny(&self) -> &DenyList {
        &self.deny
    }

    /// The checked credentials, with no secret value.
    pub fn credentials(&self) -> &[Checked] {
        &self.credentials
    }

    /// Whether a TLS end reads the tunnels of this policy: a credential, or
    /// a rule with a path.
    pub fn needs_tls_end(&self) -> bool {
        !self.credentials.is_empty() || self.rules.iter().any(|r| r.path.is_some())
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

    /// Why no rule allows `target`: the words of a [`Verdict::Deny`], so
    /// that the job finds the fix. An open policy denies no target.
    pub fn no_rule(&self, target: &Authority) -> NoRule {
        let named: Vec<&CompiledRule> = self.rules.iter().filter(|r| r.holds_host(&target.host)).collect();
        if named.is_empty() {
            return NoRule::Host;
        }
        if named.iter().any(|r| r.ports.is_empty() || r.ports.contains(&target.port)) {
            return NoRule::Path;
        }
        let mut ports: Vec<u16> = named.iter().flat_map(|r| r.ports.iter().copied()).collect();
        ports.sort_unstable();
        ports.dedup();
        NoRule::Port(ports)
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
        self.host.holds(host)
    }
}

/// Whether the prefix `prefix` holds `path` on whole segments:
/// `/v1/repos` holds `/v1/repos` and `/v1/repos/x`, not `/v1/reposx`.
pub fn path_holds(prefix: &str, path: &str) -> bool {
    prefix == "/" || path == prefix || (path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/'))
}
